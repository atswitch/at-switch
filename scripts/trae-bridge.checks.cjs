// Run explicitly with: node --test scripts/trae-bridge.checks.cjs
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const vm = require('node:vm');

// Exercise the actual embedded bridge expressions against isolated SDK/storage
// contracts; no installed application, real account or credential is accessed.
function fixture() {
  const storageValues = new Map();
  const recent = {};
  const calls = [];
  const storage = {
    get: (key, scope, fallback) => storageValues.get(`${scope}:${key}`) ?? fallback,
    store: (key, value, scope) => storageValues.set(`${scope}:${key}`, value),
    remove: (key, scope) => storageValues.delete(`${scope}:${key}`),
    flush: async () => calls.push('flush'),
  };
  const domain = {
    batchRefreshModels: () => {},
    setRecentUserSelection: (label, value) => {
      if (value) recent[label] = { mode: value.mode, modelId: value.modelId };
      else delete recent[label];
    },
    getRecentUserSelectionMap: () => recent,
  };
  const app = {
    modelDomainService: domain,
    restoreUserSelection: async () => {},
    convertToRawModelListMap: groups => groups,
    agentLabelToAgentType: label => label,
    modelPersistenceService: {
      saveModelList: async () => {},
      saveRecentUserSelection: async value => storage.store(
        'qa:AI.agent.model.recent_user_selection_by_agent_label', JSON.stringify(value), -1),
      saveSessionSelectedModel: async () => {},
    },
    sessionStore: { relation: {
      setSelectedModel: (...args) => calls.push(['session', ...args]),
      removeSelectedModel: (...args) => calls.push(['removeSession', ...args]),
    } },
  };
  const window = {
    __atSwitchTraeAccount: 'qa', __atSwitchTraeStorage: storage,
    __atSwitchTraeRequire: id => id === 6970
      ? { cN: () => null, u7: { getState: () => ({ route: {} }) } }
      : { N6: app },
  };
  const renderer = vm.createContext({ window, Object, JSON });
  const evaluate = expression => vm.runInContext(expression, renderer);
  const pipe = { on() {}, write() {}, destroy() {} };
  const source = readFileSync(resolve(__dirname, '../src-tauri/src/agents/trae_bridge.cjs'), 'utf8');
  const controller = vm.createContext({
    require: name => name === 'node:child_process'
      ? { spawn: () => ({ stdio: [null, null, null, pipe, pipe], on() {} }) }
      : require(name),
    process: { argv: ['', '', 'fake-trae', 'trae', '', 'true'], env: {}, stdout: { write() {} }, platform: 'win32' },
    Buffer, setTimeout, clearTimeout, Date, Object, JSON,
    runExpression: evaluate,
  });
  vm.runInContext(source.slice(0, source.indexOf("browser.on('error', () => emit")) + `
    evaluate = async expression => runExpression(expression);
    globalThis.bridgeTest = {syncCore, syncIde, handleStorage, singleIdeTarget, canonicalModelSource, desiredRecent, desiredSessions,
      useIde: () => {ideSession='test-ide';}};
  `, controller);
  return { storage, calls, recent, window, evaluate, bridge: controller.bridgeTest };
}

test('selection verification compares semantic values, not property order', async () => {
  const f = fixture();
  f.bridge.desiredRecent.solo_agent_lite = { modelId: 'next', mode: 0 };
  await f.bridge.syncCore('solo_agent_lite');
  assert.deepEqual(f.recent.solo_agent_lite, { mode: 0, modelId: 'next' });
  assert.ok(f.calls.includes('flush'));
});

test('authoritative current-operation choices repair stale whole-map writeback', async () => {
  const f = fixture();
  f.bridge.desiredRecent.solo_agent_lite = { modelId: 'new-agent', mode: 0 };
  f.bridge.desiredRecent.solo_coder = { modelId: 'new-ide', mode: 0 };
  f.storage.store('qa:AI.agent.model.recent_user_selection_by_agent_label',
    JSON.stringify({ solo_agent_lite: { modelId: 'old-agent', mode: 0 }, unrelated: { modelId: 'keep', mode: 1 } }), -1);
  await f.bridge.syncCore('solo_coder');
  const persisted = JSON.parse(f.storage.get('qa:AI.agent.model.recent_user_selection_by_agent_label', -1));
  assert.equal(persisted.solo_agent_lite.modelId, 'new-agent');
  assert.equal(persisted.solo_coder.modelId, 'new-ide');
  assert.equal(persisted.unrelated.modelId, 'keep');
});

test('restoring absent selection removes the key instead of leaving a dangling model', async () => {
  const f = fixture();
  f.storage.store('qa:AI.agent.model.recent_user_selection_by_agent_label',
    JSON.stringify({ solo_agent_lite: { modelId: 'temporary', mode: 0 } }), -1);
  f.bridge.desiredRecent.solo_agent_lite = null;
  await f.bridge.syncCore('solo_agent_lite', { activeSessionId: 'session', sessionPresent: false });
  assert.equal(f.storage.get('qa:AI.agent.model.recent_user_selection_by_agent_label', -1), undefined);
  assert.equal(f.recent.solo_agent_lite, undefined);
  assert.ok(f.calls.some(call => call[0] === 'removeSession'));
});

test('current-session selections repair stale whole-map writeback across selector labels', async () => {
  const f = fixture();
  f.bridge.desiredSessions.active = {
    solo_agent_lite: { modelId: 'new-lite', mode: 0 },
    solo_agent: { modelId: 'new-agent', mode: 0 },
  };
  f.storage.store('qa:AI.agent.model.session_selected_model', JSON.stringify({
    active: { solo_agent: { modelId: 'old-agent', mode: 0 }, unrelated: { modelId: 'keep', mode: 1 } },
    otherSession: { unrelated: { modelId: 'other', mode: 0 } },
  }), -1);
  await f.bridge.syncCore('solo_agent');
  const saved = JSON.parse(f.storage.get('qa:AI.agent.model.session_selected_model', -1));
  assert.equal(saved.active.solo_agent_lite.modelId, 'new-lite');
  assert.equal(saved.active.solo_agent.modelId, 'new-agent');
  assert.equal(saved.active.unrelated.modelId, 'keep');
  assert.equal(saved.otherSession.unrelated.modelId, 'other');
  assert.ok(f.calls.some(call => call[0] === 'removeSession' && call[1] === 'active' && call.length === 2));
  assert.ok(f.calls.some(call => call[0] === 'session' && call[4] === 'solo_agent_lite'));
});

test('current-session rollback removes an absent explicit lite label without dropping other labels', async () => {
  const f = fixture();
  f.bridge.desiredSessions.active = { solo_agent_lite: null };
  f.storage.store('qa:AI.agent.model.session_selected_model', JSON.stringify({
    active: { solo_agent_lite: { modelId: 'temporary', mode: 0 }, unrelated: { modelId: 'keep', mode: 0 } },
  }), -1);
  await f.bridge.syncCore('solo_agent_lite');
  const saved = JSON.parse(f.storage.get('qa:AI.agent.model.session_selected_model', -1));
  assert.equal(Object.hasOwn(saved.active, 'solo_agent_lite'), false);
  assert.equal(saved.active.unrelated.modelId, 'keep');
});

test('all selector labels share one native catalogue request per operation', async () => {
  const f = fixture();
  const requests = [];
  f.window.__atSwitchTraeRequest = { request: async request => {
    requests.push(request);
    return { code: 0, data: { list: request.data.functions.map(label => ({ function: label, models: [{ name: label }] })) } };
  } };
  for (const label of ['solo_agent_lite', 'solo_agent', 'solo_coder']) {
    await f.evaluate(`(async()=>{${f.bridge.canonicalModelSource}return loadModels('${label}')})()`);
  }
  assert.equal(requests.length, 1);
  assert.equal(requests[0].data.force_refresh, false);
  delete f.window.__atSwitchModelCatalog;
  f.window.__atSwitchCatalogDirty = true;
  await f.evaluate(`(async()=>{${f.bridge.canonicalModelSource}return loadModels('solo_coder')})()`);
  assert.equal(requests.length, 2);
  assert.equal(requests[1].data.force_refresh, true);
  assert.equal(f.window.__atSwitchCatalogDirty, false);
});

test('invalid storage fails closed instead of overwriting unrelated data', async () => {
  const f = fixture();
  f.storage.store('qa:AI.agent.model.recent_user_selection_by_agent_label', '[]', -1);
  await assert.rejects(f.bridge.syncCore('solo_agent_lite'), /trae_native_storage_invalid/);
  assert.equal(f.storage.get('qa:AI.agent.model.recent_user_selection_by_agent_label', -1), '[]');
});

test('Agent-only IDE switch and restore await durable storage flush', async () => {
  const f = fixture();
  let completed = false;
  let finish;
  f.storage.flush = () => new Promise(resolve => { finish = resolve; });
  const pending = f.bridge.syncIde(undefined, true).then(() => { completed = true; });
  await Promise.resolve();
  assert.equal(completed, false);
  assert.equal(typeof finish, 'function');
  finish();
  await pending;
  assert.equal(completed, true);
  completed = false;
  const restore = f.bridge.syncIde({ workspace: {}, app: {} }).then(() => { completed = true; });
  await Promise.resolve();
  assert.equal(completed, false);
  finish();
  await restore;
  assert.equal(completed, true);
});

test('Agent-only IDE flush failure is not reported as success', async () => {
  const f = fixture();
  f.storage.flush = async () => { throw new Error('disk unavailable'); };
  await assert.rejects(f.bridge.syncIde(undefined, true), /disk unavailable/);
});

function ideFixture() {
  const f = fixture();
  const state = {
    globalModelMap: { unrelated: 'keep' }, globalModeMap: { unrelated: 1 },
    sessionModelMap: { active: { unrelated: 'keep' } },
    sessionModeMap: { active: { unrelated: 1 } },
  };
  f.window.__atSwitchIdeRelation = {
    getState: () => state,
    store: { setState: next => Object.assign(state, next) },
    _modelStore: { getState: () => ({ setOriginModelListMap() {} }) },
    _sessionStore: { getState: () => ({ currentSessionId: 'active' }) },
    getCurrentMode: () => state.sessionModeMap.active?.solo_coder ?? state.globalModeMap.solo_coder,
    persistSessionModelMap: async () => f.storage.store('qa_ai-chat:sessionRelation:modelMap', JSON.stringify(state.sessionModelMap), 1),
    persistSessionModeMap: async () => f.storage.store('qa_ai-chat:sessionRelation:modeMap', JSON.stringify(state.sessionModeMap), 1),
  };
  f.bridge.useIde();
  f.storage.store('qa_ai-chat:sessionRelation:globalModelMap', JSON.stringify({ solo_coder: 'new-model' }), 1);
  f.storage.store('qa_ai-chat:sessionRelation:globalModeMap', JSON.stringify({ solo_coder: 0 }), 1);
  return { ...f, state };
}

test('IDE current-session override and global selector switch together without losing unrelated fields', async () => {
  const f = ideFixture();
  await f.bridge.syncIde(undefined, true);
  assert.equal(f.state.globalModelMap.solo_coder, 'new-model');
  assert.equal(f.state.sessionModelMap.active.solo_coder, 'new-model');
  assert.equal(f.state.sessionModeMap.active.solo_coder, 0);
  assert.equal(f.state.sessionModelMap.active.unrelated, 'keep');
});

test('IDE rollback restores absence and does not create empty persistent maps', async () => {
  const f = ideFixture();
  f.state.sessionModelMap = { active: { solo_coder: 'temporary' } };
  f.state.sessionModeMap = { active: { solo_coder: 0 } };
  await f.bridge.syncIde({ session: { id: 'active', modelKey: null, mode: null,
    modelStoragePresent: false, modeStoragePresent: false } });
  assert.equal(f.state.sessionModelMap.active, undefined);
  assert.equal(f.storage.get('qa_ai-chat:sessionRelation:modelMap', 1), undefined);
  assert.equal(f.storage.get('qa_ai-chat:sessionRelation:modeMap', 1), undefined);
});

test('multiple IDE workspaces fail closed instead of reporting a partial switch as success', () => {
  const f = fixture();
  const page = { type: 'page', url: 'vscode-file://vscode-app/workbench/workbench.html' };
  assert.equal(f.bridge.singleIdeTarget([page]), page);
  assert.equal(f.bridge.singleIdeTarget([]), undefined);
  assert.throws(() => f.bridge.singleIdeTarget([page, { ...page }]), /trae_multiple_ide_windows/);
});
