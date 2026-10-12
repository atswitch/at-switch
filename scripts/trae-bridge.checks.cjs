// Run explicitly with: node --test scripts/trae-bridge.checks.cjs
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const vm = require('node:vm');

// Exercise the actual embedded bridge expressions against isolated SDK/storage
// contracts; no installed application, real account or credential is accessed.
function fixture(platform = 'win32', immediateTimers = false) {
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
  const document = { querySelectorAll: () => [], querySelector: () => null };
  const renderer = vm.createContext({ window, document, Object, JSON,
    setTimeout: immediateTimers ? callback => callback() : setTimeout, clearTimeout });
  const evaluate = expression => vm.runInContext(expression, renderer);
  const pipe = { on() {}, write() {}, destroy() {} };
  const source = readFileSync(resolve(__dirname, '../src-tauri/src/agents/trae_bridge.cjs'), 'utf8');
  const controller = vm.createContext({
    require: name => name === 'node:child_process'
      ? { spawn: () => ({ stdio: [null, null, null, pipe, pipe], on() {} }) }
      : require(name),
    process: { argv: ['', '', 'fake-trae', 'trae', '', 'true'], env: {}, stdout: { write() {} }, platform },
    Buffer, setTimeout, clearTimeout, Date, Object, JSON,
    runExpression: evaluate,
  });
  vm.runInContext(source.slice(0, source.indexOf("browser.on('error', () => emit")) + `
    evaluate = async expression => runExpression(expression);
    globalThis.bridgeTest = {syncCore, syncIde, handle, handleStorage, singleIdeTarget, canonicalModelSource, desiredRecent, desiredSessions,
      verifyMacAgentPicker,
      beginOperation, recoverMacRenderer,
      useIde: () => {ideSession='test-ide';}};
  `, controller);
  return { storage, calls, recent, window, document, domain, evaluate, controller, bridge: controller.bridgeTest };
}

test('macOS repairs a lost renderer session once without restarting the browser', async () => {
  const f = fixture('darwin');
  let attempts = 0;
  let repairs = 0;
  f.controller.evaluate = async expression => {
    if (expression.startsWith('delete window')) {
      if (++attempts === 1) throw new Error('cdp_error:Runtime.evaluate:-32001:Session not found');
      return true;
    }
    return { request: 'undefined', runtime: 'undefined' };
  };
  f.controller.handle = async () => ({ ready: false });
  f.controller.recoverMacRenderer = async () => { repairs += 1; };
  assert.equal((await f.bridge.beginOperation({ expectedAccount: 'qa' })).ready, false);
  assert.equal(attempts, 2);
  assert.equal(repairs, 1);
});

test('macOS reattaches a live Agent renderer without reloading its page', async () => {
  const f = fixture('darwin');
  const calls = [];
  f.controller.request = async method => {
    calls.push(method);
    if (method === 'Target.getTargets') return { targetInfos: [
      { targetId: 'agent', type: 'page', url: 'vscode-file://vscode-app/solo-lite.html' },
    ] };
    if (method === 'Target.attachToTarget') return { sessionId: 'new-session' };
    return {};
  };
  f.controller.evaluate = async () => ({ request: 'function', runtime: 'function' });
  f.controller.start = async () => { calls.push('reload'); };
  await f.bridge.recoverMacRenderer();
  assert.deepEqual(calls, ['Target.getTargets', 'Target.attachToTarget', 'Runtime.enable']);
});

test('macOS refreshes captures only when the replacement document lost them', async () => {
  const f = fixture('darwin');
  const calls = [];
  f.controller.request = async method => {
    calls.push(method);
    if (method === 'Target.getTargets') return { targetInfos: [
      { targetId: 'agent', type: 'page', url: 'vscode-file://vscode-app/solo-lite.html' },
    ] };
    if (method === 'Target.attachToTarget') return { sessionId: 'new-session' };
    return {};
  };
  f.controller.evaluate = async () => ({ request: 'undefined', runtime: 'undefined' });
  f.controller.start = async () => { calls.push('reload'); };
  await f.bridge.recoverMacRenderer();
  assert.equal(calls.at(-1), 'reload');
});

test('Windows keeps its existing begin-operation failure behavior', async () => {
  const f = fixture('win32');
  let repairs = 0;
  f.controller.evaluate = async () => { throw new Error('cdp_error:Runtime.evaluate:-32001:Session not found'); };
  f.controller.recoverMacRenderer = async () => { repairs += 1; };
  await assert.rejects(f.bridge.beginOperation({ expectedAccount: 'qa' }), /Session not found/);
  assert.equal(repairs, 0);
});

test('macOS does not retry a native operation while its renderer is healthy', async () => {
  const f = fixture('darwin');
  let repairs = 0;
  f.controller.evaluate = async expression => {
    if (expression.startsWith('delete window')) throw new Error('native service rejected request');
    return { request: 'function', runtime: 'function' };
  };
  f.controller.recoverMacRenderer = async () => { repairs += 1; };
  await assert.rejects(f.bridge.beginOperation({ expectedAccount: 'qa' }), /native service rejected request/);
  assert.equal(repairs, 0);
});

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

test('macOS notifies the mounted Agent picker through Trae native model selection', async () => {
  const f = fixture('darwin');
  const notifications = [];
  const app = f.window.__atSwitchTraeRequire(40650).N6;
  app.notifyHostModelSelectionChange = value => notifications.push(value);
  f.document.querySelector = () => ({ getClientRects: () => [1] });
  f.bridge.desiredRecent.solo_agent_lite = { modelId: 'qa-model', mode: 0 };
  await f.bridge.syncCore('solo_agent_lite');
  assert.equal(notifications.length, 1);
  assert.equal(notifications[0].modelId, 'qa-model');
  assert.equal(notifications[0].mode, 0);
  assert.equal(notifications[0].agentType, 'solo_agent_lite');
});

test('macOS refuses success if the visible Agent picker lacks its native notification', async () => {
  const f = fixture('darwin');
  f.document.querySelector = () => ({ getClientRects: () => [1] });
  f.bridge.desiredRecent.solo_agent_lite = { modelId: 'qa-model', mode: 0 };
  await assert.rejects(f.bridge.syncCore('solo_agent_lite'), /trae_agent_model_notification_unavailable/);
});

test('macOS verifies the visible Agent label and rejects a stale mounted picker', async () => {
  const f = fixture('darwin', true);
  const picker = { textContent: 'Old Model', getClientRects: () => [1] };
  f.document.querySelector = () => picker;
  await assert.rejects(f.bridge.verifyMacAgentPicker('Target Model'), /trae_agent_visible_label_mismatch/);
  picker.textContent = 'Target Model';
  await f.bridge.verifyMacAgentPicker('Target Model');
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

test('restoring cached selectors first hydrates Trae native catalogue', async () => {
  const f = fixture('darwin');
  let requested = 0;
  let refreshed = 0;
  f.window.__atSwitchTraeRequest = { request: async () => {
    requested += 1;
    return { code: 0, data: { list: ['solo_agent_lite', 'solo_agent', 'solo_coder']
      .map(label => ({ function: label, models: [] })) } };
  } };
  f.domain.batchRefreshModels = () => { refreshed += 1; };
  const result = await f.bridge.handle({ operation: 'restore_selection' });
  assert.equal(result.restored, true);
  assert.equal(requested, 1);
  assert.equal(refreshed, 3);
});

test('macOS waits for a transient native catalogue mismatch without restarting', async () => {
  const f = fixture('darwin');
  let requested = 0;
  let synced = 0;
  f.window.__atSwitchTraeRequest = { request: async () => {
    requested += 1;
    return { code: 0, data: { list: [] } };
  } };
  f.controller.syncCore = async () => {
    if (++synced === 1) throw new Error('trae_live_selection_mismatch');
  };
  f.controller.sleep = async () => {};
  assert.equal((await f.bridge.handle({ operation: 'restore_selection' })).restored, true);
  assert.equal(requested, 2);
  assert.equal(synced, 4);
});

test('macOS does not retry malformed native storage during restore', async () => {
  const f = fixture('darwin');
  let requested = 0;
  f.window.__atSwitchTraeRequest = { request: async () => {
    requested += 1;
    return { code: 0, data: { list: [] } };
  } };
  f.controller.syncCore = async () => { throw new Error('trae_native_storage_invalid'); };
  await assert.rejects(f.bridge.handle({ operation: 'restore_selection' }), /trae_native_storage_invalid/);
  assert.equal(requested, 1);
});

test('Windows restore keeps its already-verified native operation order', async () => {
  const f = fixture('win32');
  let requested = 0;
  f.window.__atSwitchTraeRequest = { request: async () => { requested += 1; return {}; } };
  const result = await f.bridge.handle({ operation: 'restore_selection' });
  assert.equal(result.restored, true);
  assert.equal(requested, 0);
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

function ideFixture(platform = 'darwin') {
  const f = fixture(platform);
  const service = { notifyModelChanges: change => f.calls.push(['modelBroadcast', change]) };
  const picker = {
    textContent: '', getClientRects: () => [],
    '__reactFiber$test': { memoizedProps: { value: {
      serviceCollection: { getEntries: () => [['iICubeModelManagementService', service]] },
    } }, return: null },
  };
  f.document.querySelectorAll = () => [picker];
  f.document.querySelector = () => picker;
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
  return { ...f, state, picker, service };
}

test('IDE catalogue-only refresh waits for its storage flush', async () => {
  const f = ideFixture();
  f.calls.length = 0;
  await f.bridge.syncIde(undefined, false, true);
  assert.ok(f.calls.includes('flush'));
});

test('IDE current-session override and global selector switch together without losing unrelated fields', async () => {
  const f = ideFixture();
  await f.bridge.syncIde(undefined, true);
  assert.equal(f.state.globalModelMap.solo_coder, 'new-model');
  assert.equal(f.state.sessionModelMap.active.solo_coder, 'new-model');
  assert.equal(f.state.sessionModeMap.active.solo_coder, 0);
  assert.equal(f.state.sessionModelMap.active.unrelated, 'keep');
  assert.ok(f.calls.some(call => call[0] === 'modelBroadcast'));
});

test('IDE visible model label refreshes through the mounted Trae service', async () => {
  const f = ideFixture();
  f.storage.store('qa:AI.agent.model.recent_user_selection_by_agent_label',
    JSON.stringify({ solo_coder: { mode: 0, modelId: 'next' } }), -1);
  f.domain.getModelByKey = () => ({ name: 'qa-model', display_name: 'Target Model', provider: 'qa' });
  f.picker.getClientRects = () => [1];
  f.picker.textContent = 'Old Model';
  f.service.notifyModelChanges = change => {
    f.calls.push(['modelBroadcast', change]);
    f.picker.textContent = 'Target Model';
  };
  await f.bridge.syncIde(undefined, true);
  assert.equal(f.picker.textContent, 'Target Model');
  assert.equal(f.calls.find(call => call[0] === 'modelBroadcast')[1].model.name, 'qa-model');
});

test('IDE refuses a mounted selector without the versioned model service', async () => {
  const f = ideFixture();
  f.picker['__reactFiber$test'].memoizedProps.value.serviceCollection.getEntries = () => [];
  await assert.rejects(f.bridge.syncIde(undefined, true), /trae_ide_model_management_unavailable/);
});

test('Windows IDE keeps its verified selection flow without macOS picker notification', async () => {
  const f = ideFixture('win32');
  f.picker['__reactFiber$test'].memoizedProps.value.serviceCollection.getEntries = () => [];
  await f.bridge.syncIde(undefined, true);
  assert.equal(f.state.globalModelMap.solo_coder, 'new-model');
  assert.equal(f.calls.some(call => call[0] === 'modelBroadcast'), false);
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
