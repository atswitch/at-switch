// Runs with the target Trae installation's bundled Electron in Node mode.
// Only the control channel uses CDP; model requests remain inside Trae.
const { spawn } = require('node:child_process');
const { readFileSync } = require('node:fs');
const { createInterface } = require('node:readline');

const executable = process.argv[2];
const kind = process.argv[3];
// New TRAE uses the same native host and independent Agent/IDE selectors
// on Windows and macOS; no legacy product or configuration is imported.
const modelLabel = 'solo_agent_lite';
const selectionLabels = ['solo_agent_lite', 'solo_agent', 'solo_coder'];
const profile = process.argv[4];
const terminateOnEof = process.argv[5] === 'true';
if (!executable || kind !== 'trae') {
  process.stdout.write(JSON.stringify({ ready: false, error: 'missing_executable' }) + '\n');
  process.exit(1);
}

const childEnv = { ...process.env };
delete childEnv.ELECTRON_RUN_AS_NODE;
const args = ['--remote-debugging-pipe'];
if (profile) args.push(`--user-data-dir=${profile}`);
const browser = spawn(executable, args, {
  detached: true,
  env: childEnv,
  stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'],
});

let nextId = 0;
let sessionId;
let buffer = Buffer.alloc(0);
const pending = new Map();
const scripts = new Map();
const breakpoints = new Map();
const captured = new Set();
let closing = false;
let released = false;
let startupStage = 'spawn';
let executionGeneration = 0;
let requiredCaptureGeneration = 0;
let runtimeUsesExport = false;
const targetSessions = new Map();
let runtimeUrl;
let keepOnEof = false;
const desiredRecent = {};
const desiredSessions = {};

function emit(value) {
  process.stdout.write(JSON.stringify(value) + '\n');
}

function request(method, params = {}, explicitSession = sessionId, timeoutMs = 10000) {
  const id = ++nextId;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`cdp_timeout:${method}`));
    }, timeoutMs);
    pending.set(id, (message) => {
      clearTimeout(timer);
      if (message.error) reject(new Error(`cdp_error:${method}:${message.error.code}:${message.error.message}`));
      else resolve(message.result);
    });
    browser.stdio[3].write(JSON.stringify({ id, method, params,
      ...(explicitSession ? { sessionId: explicitSession } : {}) }) + '\0');
  });
}

function scriptPath(url) {
  let pathname = decodeURIComponent(new URL(url).pathname);
  if (process.platform === 'win32' && /^\/[a-zA-Z]:\//.test(pathname)) {
    pathname = pathname.slice(1);
  }
  return pathname;
}

function positionFor(url, markers) {
  const source = readFileSync(scriptPath(url), 'utf8');
  const marker = markers.find((candidate) => source.includes(candidate));
  if (!marker) throw new Error('trae_protocol_signature_missing');
  const offset = source.indexOf(marker);
  if (source.indexOf(marker, offset + marker.length) !== -1) {
    throw new Error('trae_protocol_signature_ambiguous');
  }
  const preceding = source.slice(0, offset);
  const lineNumber = preceding.split('\n').length - 1;
  const columnNumber = offset - preceding.lastIndexOf('\n') - 1;
  return { lineNumber, columnNumber };
}

async function paused(message) {
  const breakpointId = message.params.hitBreakpoints?.find((id) => breakpoints.has(id));
  const kind = breakpoints.get(breakpointId);
  const callFrameId = message.params.callFrames?.[0]?.callFrameId;
  try {
    if (kind && callFrameId && !captured.has(kind)) {
      const expression = kind === 'runtime'
        ? 'window.__atSwitchTraeRequire=c;typeof window.__atSwitchTraeRequire'
        : 'window.__atSwitchTraeRequest=this;typeof window.__atSwitchTraeRequest?.request';
      const result = await request('Debugger.evaluateOnCallFrame', {
        callFrameId, expression, returnByValue: false,
      });
      const currentDocument = kind !== 'workbench' ||
        executionGeneration >= requiredCaptureGeneration;
      if (!result.exceptionDetails && result.result?.value === 'function' && currentDocument) {
        captured.add(kind);
        await request('Debugger.removeBreakpoint', { breakpointId });
        breakpoints.delete(breakpointId);
      }
    }
  } finally {
    await request('Debugger.resume');
  }
}

browser.stdio[4].on('data', (part) => {
  buffer = Buffer.concat([buffer, part]);
  let end;
  while ((end = buffer.indexOf(0)) >= 0) {
    let message;
    try { message = JSON.parse(buffer.subarray(0, end).toString()); } catch { /* ignore */ }
    buffer = buffer.subarray(end + 1);
    if (!message) continue;
    if (message.id && pending.has(message.id)) {
      pending.get(message.id)(message);
      pending.delete(message.id);
    } else if (message.method === 'Debugger.scriptParsed') {
      scripts.set(message.params.url, message.params.scriptId);
    } else if (message.method === 'Runtime.executionContextsCleared') {
      executionGeneration += 1;
    } else if (message.method === 'Debugger.paused') {
      paused(message).catch(() => { /* the startup timeout reports failure */ });
    }
  }
});
browser.stdio[3].on('error', () => { /* startup or request timeout reports failure */ });
browser.on('exit', () => {
  if (!closing || released) process.exit(0);
});

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function terminateBrowser() {
  const exited = () => browser.exitCode !== null || browser.signalCode !== null;
  if (exited()) return;
  browser.kill('SIGTERM');
  for (let attempt = 0; attempt < 30 && !exited(); attempt++) {
    await sleep(100);
  }
  if (!exited()) {
    browser.kill('SIGKILL');
    for (let attempt = 0; attempt < 20 && !exited(); attempt++) {
      await sleep(100);
    }
  }
}

async function waitFor(predicate, timeoutMs, label) {
  const deadline = Date.now() + timeoutMs;
  do {
    const value = await predicate();
    if (value) return value;
    await sleep(150);
  } while (Date.now() < deadline);
  throw new Error(`trae_timeout:${label}`);
}

async function evaluate(expression, awaitPromise = false, timeoutMs = 15000) {
  const result = await request('Runtime.evaluate', {
    expression, awaitPromise, returnByValue: true,
  }, sessionId, timeoutMs);
  if (result.exceptionDetails) {
    const detail = String(result.exceptionDetails.exception?.description ??
      result.exceptionDetails.text ?? 'unknown')
      .split('\n')[0].replace(/file:\/\/\/[^ )]+/g, '<app>').slice(0, 240);
    throw new Error(`trae_internal_evaluation_failed:${detail}`);
  }
  return result.result?.value;
}

async function start() {
  startupStage = 'target';
  let observedTargets = [];
  const target = await waitFor(async () => {
    // The native host can take longer to initialize after a controlled exit.
    // Retry pipe handshakes in the same process, never launch a second one.
    const result = await request('Target.getTargets', {}, undefined, 5000)
      .catch(error => { if(error.message==='cdp_timeout:Target.getTargets')return null;
        throw error; });
    if (!result) return undefined;
    observedTargets = result.targetInfos ?? [];
    return observedTargets.find(item=>item.type==='page'&&item.url?.includes('solo-lite.html'))??
      observedTargets.find(item=>item.type==='page'&&item.url?.endsWith('/workbench/workbench.html'));
  }, 60000, 'workbench').catch((error) => {
    const summary = observedTargets.slice(0, 8).map((item) => {
      let tail = '';
      try { tail = new URL(item.url).pathname.split('/').slice(-2).join('/'); }
      catch { tail = String(item.url ?? '').slice(-40); }
      return `${item.type}:${tail}`;
    }).join(',');
    throw new Error(`${error.message}:${summary}`);
  });
  sessionId = (await request('Target.attachToTarget', {
    targetId: target.targetId, flatten: true,
  }, undefined)).sessionId;
  if (!sessionId) throw new Error('trae_debugger_attach_failed');
  targetSessions.set(target.targetId, sessionId);
  startupStage = 'scripts';
  await request('Runtime.enable');
  await request('Debugger.enable');
  const urls = await waitFor(() => {
    const all = [...scripts.keys()];
    const runtime = all.find((url) => url.includes('/solo-lite/dist/index.mjs')) ??
      all.find((url) => url.includes('/ai-modules-chat/dist/index.mjs'));
    const workbench = all.find((url) =>
      url.includes('workbench.desktop.main.solo-lite-slim.js')) ??
      all.find((url) => url.includes('/vs/workbench/workbench.desktop.main.js'));
    return runtime && workbench ? { runtime, workbench } : undefined;
  }, 15000, 'scripts').catch((error) => {
    const candidates = [...scripts.keys()].filter((url) =>
      /solo|workbench|index\.mjs|desktop\.main/i.test(url));
    const summary = candidates.slice(-24).map((url) => {
      try { return new URL(url).pathname.split('/').slice(-3).join('/'); }
      catch { return String(url).slice(-60); }
    }).join(',');
    throw new Error(`${error.message}:${summary}`);
  });
  const signatures = {
    runtime: ['var p=c(c.s=52069).A;'],
    workbench: [
      'const[t,s]=await Promise.all([this.Bb(),this.ib()])',
    ],
  };
  runtimeUsesExport = urls.runtime.includes('/ai-modules-chat/dist/index.mjs');
  runtimeUrl = urls.runtime;
  startupStage = 'breakpoints';
  const armCapture = async (kind) => {
    const url = urls[kind];
    const location = positionFor(url, signatures[kind]);
    const result = await request('Debugger.setBreakpointByUrl', { url, ...location });
    if (!result.breakpointId || !result.locations?.length) {
      throw new Error(`trae_debugger_breakpoint_failed:${kind}`);
    }
    breakpoints.set(result.breakpointId, kind);
  };
  const armCaptures = async () => {
    if (!runtimeUsesExport) await armCapture('runtime');
    await armCapture('workbench');
  };
  const captureRuntime = async () => {
    if (!runtimeUsesExport) {
      await waitFor(() => captured.has('runtime'), 15000, 'runtime_capture');
      return;
    }
    const runtimeUrl = JSON.stringify(urls.runtime);
    await waitFor(async () => evaluate(`import(${runtimeUrl}).then(module=>{
      window.__atSwitchTraeRequire=module.__webpack_require__;
      return typeof window.__atSwitchTraeRequire==='function';
    })`, true, 30000).catch(() => false), 30000, 'runtime_export');
    captured.add('runtime');
  };
  await armCaptures();
  await request('Page.enable');
  startupStage = 'reload';
  requiredCaptureGeneration = executionGeneration + 1;
  await request('Page.reload', { ignoreCache: false });
  await waitFor(() => executionGeneration >= requiredCaptureGeneration,
    10000, 'execution_context');
  startupStage = 'runtime';
  await captureRuntime();

    // Reloading the workbench exercises its native request
    // service. The bridge uses that already-initialized host service directly;
    // it does not depend on the chat-core DI container in this document.
    startupStage = 'request_capture';
    await waitFor(() => captured.has('workbench'), 20000, 'request_capture');

  startupStage = 'health';
  await request('Debugger.disable');
  const health = await evaluate('({request:typeof window.__atSwitchTraeRequest?.request,'
    + 'runtime:typeof window.__atSwitchTraeRequire})');
  if (health.request !== 'function' || health.runtime !== 'function') {
    throw new Error(`trae_bridge_not_ready:request=${health.request},runtime=${health.runtime}`);
  }
}

function nativeStoragePrelude() {
  return `const storage=window.__atSwitchTraeStorage;
    const account=window.__atSwitchTraeAccount;
    if(!storage||!account)throw new Error('trae_native_storage_unavailable');
    const readJson=(key,scope=-1)=>{const raw=storage.get(key,scope,'');
      if(!raw)return{};const value=JSON.parse(raw);
      if(!value||Array.isArray(value)||typeof value!=='object')
        throw new Error('trae_native_storage_invalid');
      return value};
    const writeJson=(key,value,scope=-1)=>
      storage.store(key,JSON.stringify(value),scope,1);`;
}

const nativeModelKeySource = `const modelKey=(model)=>model.custom_model_id?
  [model.config_source??'',model.provider??'-',model.name??'',
    model.custom_model_id].join('_'):
  [model.config_source??'',model.provider??'-',model.name??''].join('_');`;

const canonicalModelSource = `${nativeModelKeySource}
  const persistedKey=(label,model)=>label+'_'+modelKey(model);
  const loadModels=async(label)=>{
    if(window.__atSwitchModelCatalog?.[label])return window.__atSwitchModelCatalog[label];
    const response=await window.__atSwitchTraeRequest.request({service:'model',
      method:'model_list_by_function',data:{functions:${JSON.stringify(selectionLabels)},force_refresh:!!window.__atSwitchCatalogDirty,
        show_custom_model:true}});
    if(response?.code!==0)throw new Error('trae_selection_model_list_failed');
    window.__atSwitchModelCatalog=Object.fromEntries((response.data?.list??[])
      .map(group=>[group.function,group.models??[]]));
    window.__atSwitchCatalogDirty=false;
    return window.__atSwitchModelCatalog[label]??[]};`;

// Use the initialized chat SDK, not an independent DI instance or disk-only
// confirmation. Every selector must consume the same live model catalogue.
const corePrelude = `const q=window.__atSwitchTraeRequire;
  const app=q(40650).N6,domain=app.modelDomainService;
  if(typeof app.restoreUserSelection!=='function'||
    typeof domain.getRecentUserSelectionMap!=='function')
    throw new Error('trae_live_model_service_unavailable');`;
let ideSession;
function singleIdeTarget(targetInfos) {
  const targets=targetInfos.filter(t=>t.type==='page'&&t.url.endsWith('/workbench/workbench.html'));
  if(targets.length>1)throw new Error('trae_multiple_ide_windows');
  return targets[0];
}
async function inSession(sid, action) {
  const previous=sessionId;
  try {sessionId=sid;return await action();} finally {sessionId=previous;}
}
async function prepareLiveContexts() {
  await waitFor(()=>evaluate(`(()=>{try{${corePrelude}return true}catch{return false}})()`),
    15000,'live_model_service');
  await evaluate(`(async()=>{${corePrelude}
    const host=q(${runtimeUsesExport?'32143':'97988'}).mc.getInstance();
    const modelId=[...host.singletons.keys()].find(id=>String(id)==='Symbol(IModelService)');
    if(modelId){const models=host.resolve(modelId);if(models._initPromise)await models._initPromise;}
    await app.restoreUserSelection();
  })()`,true,30000);
  const {targetInfos}=await request('Target.getTargets',{},undefined);
  const ide=singleIdeTarget(targetInfos);
  if(!ide){ideSession=undefined;return;}
  ideSession=targetSessions.get(ide.targetId);
  if(!ideSession){
    ideSession=(await request('Target.attachToTarget',{targetId:ide.targetId,flatten:true},undefined)).sessionId;
    targetSessions.set(ide.targetId,ideSession);
  }
  const account=await evaluate('window.__atSwitchTraeAccount');
  await inSession(ideSession,async()=>{
    await request('Runtime.enable');
    const url=runtimeUrl.replace('/solo-lite/dist/index.mjs','/ai-modules-chat/dist/index.mjs');
    await evaluate(`(async()=>{
      window.__atSwitchTraeRequire=(await import(${JSON.stringify(url)})).__webpack_require__;
      const q=window.__atSwitchTraeRequire;
      const relation=q(32143).mc.getInstance().resolve(q(40656).q);
      const key=await relation._storageService.generateStorageKey('ai-chat:sessionRelation:globalModelMap');
      const account=${JSON.stringify(account)};
      if(key!==account+'_ai-chat:sessionRelation:globalModelMap')
        throw new Error('trae_native_account_mismatch');
      window.__atSwitchIdeRelation=relation;
      window.__atSwitchTraeStorage=relation._storageService._storageFacade._storageService;
      window.__atSwitchTraeAccount=account;
      // Do not let deferred workspace hydration overwrite this operation's
      // selections after it reports success.
      if(relation._initFromCachePromise)await relation._initFromCachePromise;
      const ready=relation._modelStore.getState().modelConfigReadyPromise;
      if(ready&&typeof ready.then==='function')await ready;
    })()`,true);
  });
}
async function syncCore(label,baseline) {
  return evaluate(`(async()=>{${nativeStoragePrelude()}${corePrelude}
    const label=${JSON.stringify(label)},baseline=${JSON.stringify(baseline??null)};
    const catalog=window.__atSwitchModelCatalog;
    if(catalog){
      const converted=app.convertToRawModelListMap(Object.entries(catalog)
        .map(([functionName,models])=>({function:functionName,models})));
      domain.batchRefreshModels(converted);
      await app.modelPersistenceService.saveModelList(converted);
    }
    const recentKey=account+':AI.agent.model.recent_user_selection_by_agent_label';
    const recent=readJson(recentKey),desired=${JSON.stringify(desiredRecent)};
    for(const [current,selection] of Object.entries(desired)){
      if(selection===null)delete recent[current];else recent[current]=selection;
    }
    if(Object.keys(recent).length)writeJson(recentKey,recent);else storage.remove(recentKey,-1);
    for(const current of ${JSON.stringify(selectionLabels)})
      domain.setRecentUserSelection(current,recent[current]);
    const sessionKey=account+':AI.agent.model.session_selected_model';
    const sessions=readJson(sessionKey),desiredSessions=${JSON.stringify(desiredSessions)};
    for(const [id,labels] of Object.entries(desiredSessions)){
      const next={...sessions[id]};
      for(const [current,selection] of Object.entries(labels)){
        if(selection===null)delete next[current];else next[current]=selection;
      }
      if(Object.keys(next).length)sessions[id]=next;else delete sessions[id];
      // removeSelectedModel's agent-type overload derives a legacy label and
      // cannot remove solo_agent_lite precisely. Rebuild just this session's
      // selection map with explicit labels, preserving unrelated selections.
      app.sessionStore.relation.removeSelectedModel(id);
      for(const [current,selection] of Object.entries(next))
        app.sessionStore.relation.setSelectedModel(id,app.agentLabelToAgentType(current),selection,current);
    }
    if(Object.keys(sessions).length)writeJson(sessionKey,sessions);else storage.remove(sessionKey,-1);
    const type=app.agentLabelToAgentType(label);
    for(const [id,labels] of Object.entries(sessions)){
      if(!desiredSessions[id]&&labels?.[label])app.sessionStore.relation.setSelectedModel(id,type,labels[label],label);
    }
    if(baseline?.activeSessionId&&!baseline.sessionPresent&&!desiredSessions[baseline.activeSessionId])
      app.sessionStore.relation.removeSelectedModel(baseline.activeSessionId,type);
    if(Object.keys(recent).length)
      await app.modelPersistenceService.saveRecentUserSelection(recent);
    if(Object.keys(sessions).length)
      await app.modelPersistenceService.saveSessionSelectedModel(sessions);
    await storage.flush();
    const live=domain.getRecentUserSelectionMap()[label];
    if(live?.mode!==recent[label]?.mode||live?.modelId!==recent[label]?.modelId)
      throw new Error('trae_live_selection_mismatch');
    return true;
  })()`,true);
}
async function syncIde(baseline, selecting=false, catalogOnly=false) {
  if(!ideSession){
    // Even with only Agent open, select_ide/restore_ide updated the native
    // host's app/default storage. Await its durable flush: no IDE renderer
    // exists to persist these writes on our behalf. Otherwise verification
    // waits for the storage timer, or a later IDE opens on the previous model.
    if(!catalogOnly)await evaluate(`(async()=>{${nativeStoragePrelude()}
      await storage.flush();return true;})()`,true);
    return;
  }
  const catalog=await evaluate('window.__atSwitchModelCatalog');
  await inSession(ideSession,()=>evaluate(`(async()=>{${nativeStoragePrelude()}
    const relation=window.__atSwitchIdeRelation;
    if(!relation)throw new Error('trae_ide_live_service_unavailable');
    const catalog=${JSON.stringify(catalog??{})};
    relation._modelStore.getState().setOriginModelListMap(catalog);
    if(${catalogOnly})return true;
    const modelStorageKey=account+'_ai-chat:sessionRelation:globalModelMap';
    const modeStorageKey=account+'_ai-chat:sessionRelation:globalModeMap';
    const models=readJson(modelStorageKey,storage.get(modelStorageKey,1,'')?1:-1);
    const modes=readJson(modeStorageKey,storage.get(modeStorageKey,1,'')?1:-1);
    const state=relation.getState();
    const session=relation._sessionStore.getState().currentSessionId;
    const baseline=${JSON.stringify(baseline??null)};
    const nextModels={...state.sessionModelMap},nextModes={...state.sessionModeMap};
    if(${selecting}&&session){
      nextModels[session]={...nextModels[session],solo_coder:models.solo_coder};
      nextModes[session]={...nextModes[session],solo_coder:modes.solo_coder};
    }else if(baseline?.session){
      const saved=baseline.session,id=saved.id;
      nextModels[id]={...nextModels[id]};nextModes[id]={...nextModes[id]};
      if(saved.modelKey===null)delete nextModels[id].solo_coder;else nextModels[id].solo_coder=saved.modelKey;
      if(saved.mode===null)delete nextModes[id].solo_coder;else nextModes[id].solo_coder=saved.mode;
      if(!Object.keys(nextModels[id]).length)delete nextModels[id];
      if(!Object.keys(nextModes[id]).length)delete nextModes[id];
    }
    relation.store.setState({globalModelMap:{...state.globalModelMap,solo_coder:models.solo_coder},
      globalModeMap:{...state.globalModeMap,solo_coder:modes.solo_coder},
      sessionModelMap:nextModels,sessionModeMap:nextModes});
    await Promise.all([relation.persistSessionModelMap(),relation.persistSessionModeMap()]);
    if(baseline?.session){
      const saved=baseline.session;
      if(!saved.modelStoragePresent&&!Object.keys(nextModels).length)
        storage.remove(account+'_ai-chat:sessionRelation:modelMap',1);
      if(!saved.modeStoragePresent&&!Object.keys(nextModes).length)
        storage.remove(account+'_ai-chat:sessionRelation:modeMap',1);
    }
    await storage.flush();
    if(${selecting}&&relation.getCurrentMode()!==modes.solo_coder)
      throw new Error('trae_ide_live_mode_mismatch');
  })()`,true));
  // The IDE host's legacy-to-core bridge may derive a fallback identity while
  // a newly added model is still initializing. Seed its core directly from
  // the same canonical catalogue and selections as the Agent renderer.
  const recent=await evaluate(`(()=>{${nativeStoragePrelude()}
    return readJson(account+':AI.agent.model.recent_user_selection_by_agent_label')})()`);
  await inSession(ideSession,()=>evaluate(`(async()=>{${corePrelude}${nativeStoragePrelude()}
    const catalog=${JSON.stringify(catalog??{})},recent=${JSON.stringify(recent)};
    const key=account+':AI.agent.model.recent_user_selection_by_agent_label';
    if(Object.keys(recent).length)writeJson(key,recent);else storage.remove(key,-1);
    domain.batchRefreshModels(app.convertToRawModelListMap(Object.entries(catalog)
      .map(([functionName,models])=>({function:functionName,models}))));
    for(const label of ${JSON.stringify(selectionLabels)})domain.setRecentUserSelection(label,recent[label]);
    if(Object.keys(recent).length)await app.modelPersistenceService.saveRecentUserSelection(recent);
    await storage.flush();
    return true;
  })()`,true));
}

async function handle(command) {
  const op=command.operation;
  if(op==='retain'){keepOnEof=true;return {retained:true};}
  if(op==='begin_operation'){
    for(const label of Object.keys(desiredRecent))delete desiredRecent[label];
    for(const id of Object.keys(desiredSessions))delete desiredSessions[id];
    await evaluate('delete window.__atSwitchNativeModels;delete window.__atSwitchModelCatalog;true');
    command={...command,operation:'bind_storage'};
    const result=await handle(command);
    if(result.ready)await handle({operation:'restore_selection'});
    return result;
  }
  if(op==='bind_storage'){
    const result=await handleStorage(command);
    if(result.ready)await prepareLiveContexts();
    return result;
  }
  if(op==='snapshot_ide'||op==='select_ide'||op==='restore_ide'){
    if(ideSession){
      const catalog=await evaluate(`(async()=>{${canonicalModelSource}
        await loadModels('solo_coder');return window.__atSwitchModelCatalog})()`,true);
      await inSession(ideSession,()=>evaluate(`window.__atSwitchModelCatalog=${JSON.stringify(catalog)};true`));
    }
    // IDE workspace storage belongs to the IDE renderer, not the Agent host.
    const result=await (ideSession?inSession(ideSession,()=>handleStorage(command)):handleStorage(command));
    if(op==='snapshot_ide'&&ideSession){
      const live=await inSession(ideSession,()=>evaluate(`(()=>{
        const r=window.__atSwitchIdeRelation,s=r.getState(),id=r._sessionStore.getState().currentSessionId;
        return {mode:r.getCurrentMode(),displayName:r.getCurrentModel()?.display_name??r.getCurrentModel()?.name??'',
          session:id?{id,modelKey:s.sessionModelMap[id]?.solo_coder??null,
            mode:s.sessionModeMap[id]?.solo_coder??null,
            modelStoragePresent:!!window.__atSwitchTraeStorage.get(window.__atSwitchTraeAccount+'_ai-chat:sessionRelation:modelMap',1,''),
            modeStoragePresent:!!window.__atSwitchTraeStorage.get(window.__atSwitchTraeAccount+'_ai-chat:sessionRelation:modeMap',1,'')}:null};})()`));
      result.selection={...result.selection,mode:live.mode,displayName:live.displayName};
      result.ideBaseline.session=live.session;
    }else if(op!=='snapshot_ide'&&result.verified){
      await syncIde(command.baseline,op==='select_ide');
      if(op==='select_ide'&&ideSession){
        const live=await inSession(ideSession,()=>evaluate(`(()=>{const r=window.__atSwitchIdeRelation;
          return r.getCurrentMode()===1?'Auto Mode':r.getCurrentModel()?.display_name??r.getCurrentModel()?.name??''})()`));
        if(live!==command.displayName&&!(live==='Auto Mode'&&command.displayName==='Auto'))
          throw new Error('trae_ide_live_model_mismatch');
      }
    }
    return result;
  }
  const result=await handleStorage(command);
  if(['select','select_auto','restore_selection_baseline'].includes(op)&&result.verified){
    const label=command.label??modelLabel;
    desiredRecent[label]=op==='restore_selection_baseline'?
      (command.baseline.present?{modelId:command.baseline.modelId??'',mode:command.baseline.mode??1}:null):result.selection;
    const id=op==='restore_selection_baseline'?command.baseline.activeSessionId:command.sessionId;
    if(typeof id==='string'){
      const selection=op==='restore_selection_baseline'?
        (command.baseline.sessionPresent?{modelId:command.baseline.sessionModelId??'',mode:command.baseline.sessionMode??1}:null):result.selection;
      desiredSessions[id]={...desiredSessions[id],[label]:selection};
    }
    await syncCore(label,command.baseline);
  }
  if(op==='restore_selection'){
    for(const label of selectionLabels)await syncCore(label);
  }
  if(op==='snapshot'){
    const live=await evaluate(`(()=>{${corePrelude}
      const label=${JSON.stringify(command.label??modelLabel)};
      const recent=domain.getRecentUserSelectionMap()[label]??{mode:1,modelId:''};
      const selected=${JSON.stringify(result.activeSessionId??null)}?
        domain.getSessionSelectedModelMap()[${JSON.stringify(result.activeSessionId??null)}]?.[label]??recent:recent;
      const resolve=selection=>({mode:selection.mode,modelId:selection.modelId,
        displayName:domain.getModelByKey(selection.modelId)?.display_name??''});
      return {selection:resolve(recent),activeSelection:resolve(selected)};
    })()`);
    result.selection=live.selection;result.activeSelection=live.activeSelection;
  }
  if(op==='add_model'||op==='delete_model'){
    await evaluate('delete window.__atSwitchModelCatalog;delete window.__atSwitchNativeModels;window.__atSwitchCatalogDirty=true;true');
    if(ideSession)await inSession(ideSession,()=>evaluate('delete window.__atSwitchModelCatalog;true'));
    if(op==='delete_model'&&result.verified){
      await evaluate(`(async()=>{${canonicalModelSource}await loadModels('solo_coder');return true})()`,true);
      for(const label of selectionLabels)await syncCore(label);
      await syncIde(undefined,false,true);
    }
  }
  return result;
}

async function handleStorage(command) {
  if (command.operation === 'snapshot_ide') {
    if (kind !== 'trae') throw new Error('trae_ide_unsupported');

      return evaluate(`(async()=>{${nativeStoragePrelude()}
        ${canonicalModelSource}
        const modelStorageKey=account+'_ai-chat:sessionRelation:globalModelMap';
        const modeStorageKey=account+'_ai-chat:sessionRelation:globalModeMap';
        const workspaceModels=readJson(modelStorageKey,1);
        const workspaceModes=readJson(modeStorageKey,1);
        const appModels=readJson(modelStorageKey,-1);
        const appModes=readJson(modeStorageKey,-1);
        const rows=await loadModels('solo_coder');
        const known=(key)=>rows.some(model=>modelKey(model)===key);
        const workspaceValid=workspaceModes.solo_coder===1||
          (workspaceModes.solo_coder===0&&known(workspaceModels.solo_coder));
        const models=workspaceValid?workspaceModels:appModels;
        const modes=workspaceValid?workspaceModes:appModes;
        const key=models.solo_coder??'',mode=modes.solo_coder;
        const matches=rows.filter(model=>modelKey(model)===key);
        const selectedMode=mode===0&&matches.length===1?0:1;
        const scopeState=(scope)=>{const rawModels=storage.get(modelStorageKey,scope,'');
          const rawModes=storage.get(modeStorageKey,scope,'');
          const scopeModels=readJson(modelStorageKey,scope);
          const scopeModes=readJson(modeStorageKey,scope);
          return {modelKey:Object.hasOwn(scopeModels,'solo_coder')?
            scopeModels.solo_coder:null,
            mode:Object.hasOwn(scopeModes,'solo_coder')?scopeModes.solo_coder:null,
            modelStoragePresent:!!rawModels,modeStoragePresent:!!rawModes}};
        return {code:0,selection:{mode:selectedMode,modelId:key,
          displayName:selectedMode===0?
            matches[0].display_name??matches[0].name??'':''},
          activeSession:false,activeSessionId:null,
          activeSelection:{mode:selectedMode,modelId:key,displayName:''},models:[],
          ideBaseline:{workspace:scopeState(1),app:scopeState(-1)}};
      })()`, true);

  }
  if (command.operation === 'restore_ide') {
    if (kind !== 'trae') throw new Error('trae_ide_unsupported');
    const baseline = JSON.stringify(command.baseline);

      return evaluate(`(()=>{${nativeStoragePrelude()}
        const baseline=${baseline};
        const modelStorageKey=account+'_ai-chat:sessionRelation:globalModelMap';
        const modeStorageKey=account+'_ai-chat:sessionRelation:globalModeMap';
        const restore=(scope,saved)=>{
          if(!saved||!(saved.modelKey===null||typeof saved.modelKey==='string')||
            !(saved.mode===null||Number.isInteger(saved.mode))||
            typeof saved.modelStoragePresent!=='boolean'||
            typeof saved.modeStoragePresent!=='boolean')
            throw new Error('trae_ide_baseline_invalid');
          const nextModels={...readJson(modelStorageKey,scope)};
          const nextModes={...readJson(modeStorageKey,scope)};
          if(saved.modelKey===null)delete nextModels.solo_coder;
          else nextModels.solo_coder=saved.modelKey;
          if(saved.mode===null)delete nextModes.solo_coder;
          else nextModes.solo_coder=saved.mode;
          if(!saved.modelStoragePresent&&!Object.keys(nextModels).length)
            storage.remove(modelStorageKey,scope);
          else writeJson(modelStorageKey,nextModels,scope);
          if(!saved.modeStoragePresent&&!Object.keys(nextModes).length)
            storage.remove(modeStorageKey,scope);
          else writeJson(modeStorageKey,nextModes,scope);
          return (readJson(modelStorageKey,scope).solo_coder??null)===saved.modelKey&&
            (readJson(modeStorageKey,scope).solo_coder??null)===saved.mode};
        return {verified:restore(1,baseline.workspace)&&restore(-1,baseline.app)};
      })()`);

  }
  if (command.operation === 'select_ide') {
    if (kind !== 'trae') throw new Error('trae_ide_unsupported');
    const name = JSON.stringify(command.displayName);

      return evaluate(`(async()=>{${nativeStoragePrelude()}
        ${canonicalModelSource}
        const name=${name};
        const rows=await loadModels('solo_coder');
        let nextKey,nextMode=1;
        if(name!=='Auto'&&name!=='Auto Mode'){
          const matches=rows.filter(model=>
            model?.display_name===name||model?.name===name);
          if(matches.length!==1)return{verified:false,reason:'model_not_unique',
            count:matches.length};
          nextKey=modelKey(matches[0]);nextMode=0;
        }
        const modelStorageKey=account+'_ai-chat:sessionRelation:globalModelMap';
        const modeStorageKey=account+'_ai-chat:sessionRelation:globalModeMap';
        for(const scope of [1,-1]){
          const models={...readJson(modelStorageKey,scope),solo_coder:nextKey};
          if(nextKey===undefined)delete models.solo_coder;
          writeJson(modelStorageKey,models,scope);
          writeJson(modeStorageKey,{...readJson(modeStorageKey,scope),
            solo_coder:nextMode},scope);
        }
        return {verified:[1,-1].every(scope=>
          readJson(modelStorageKey,scope).solo_coder===nextKey&&
          readJson(modeStorageKey,scope).solo_coder===nextMode)};
      })()`, true);

  }
  if (command.operation === 'restore_selection') {

      return evaluate(`(()=>{${nativeStoragePrelude()}
        readJson(account+':AI.agent.model.recent_user_selection_by_agent_label');
        readJson(account+':AI.agent.model.session_selected_model');
        return {restored:true};
      })()`);

  }
  if (command.operation === 'restore_selection_baseline') {

    const label = JSON.stringify(command.label);
    const baseline = JSON.stringify(command.baseline);
    return evaluate(`(()=>{${nativeStoragePrelude()}
      const label=${label},baseline=${baseline};
      if(!baseline||typeof baseline.present!=='boolean'||
        !(baseline.modelId===null||typeof baseline.modelId==='string')||
        !(baseline.mode===null||Number.isInteger(baseline.mode))||
        !(baseline.activeSessionId===null||
          typeof baseline.activeSessionId==='string')||
        typeof baseline.sessionPresent!=='boolean'||
        !(baseline.sessionModelId===null||
          typeof baseline.sessionModelId==='string')||
        !(baseline.sessionMode===null||Number.isInteger(baseline.sessionMode)))
        throw new Error('trae_selection_baseline_invalid');
      const key=account+':AI.agent.model.recent_user_selection_by_agent_label';
      const recent={...readJson(key)};
      if(!baseline.present)delete recent[label];
      else recent[label]={modelId:baseline.modelId??'',mode:baseline.mode??1};
      if(!baseline.present&&!Object.keys(recent).length)storage.remove(key,-1);
      else writeJson(key,recent);
      const sessionKey=account+':AI.agent.model.session_selected_model';
      const sessions={...readJson(sessionKey)};
      if(baseline.activeSessionId){
        const labels={...(sessions[baseline.activeSessionId]??{})};
        if(!baseline.sessionPresent)delete labels[label];
        else labels[label]={modelId:baseline.sessionModelId??'',
          mode:baseline.sessionMode??1};
        if(Object.keys(labels).length)sessions[baseline.activeSessionId]=labels;
        else delete sessions[baseline.activeSessionId];
        if(!Object.keys(sessions).length)storage.remove(sessionKey,-1);
        else writeJson(sessionKey,sessions);
      }
      const saved=readJson(key);
      const value=saved[label];
      const savedSessions=readJson(sessionKey);
      const savedSession=baseline.activeSessionId?
        savedSessions[baseline.activeSessionId]?.[label]:undefined;
      const recentVerified=baseline.present?
        value?.modelId===(baseline.modelId??'')&&value?.mode===(baseline.mode??1):
        !Object.hasOwn(saved,label);
      const sessionVerified=!baseline.activeSessionId||
        (baseline.sessionPresent?
          savedSession?.modelId===(baseline.sessionModelId??'')&&
            savedSession?.mode===(baseline.sessionMode??1):
          !Object.hasOwn(savedSessions[baseline.activeSessionId]??{},label));
      return {verified:recentVerified&&sessionVerified};
    })()`);
  }
  if (command.operation === 'add_model') {
    const provider = {
      openai_chat_completions: 'custom_openai_compatible',
      openai_responses: 'custom_responses_compatible',
      anthropic_messages: 'custom_anthropic_compatible',
    }[command.protocol];
    if (!provider || !command.displayName || !command.modelId || !command.endpoint
      || !command.apiKey) throw new Error('trae_invalid_model_input');
    const input = JSON.stringify({
      provider, displayName: command.displayName, modelId: command.modelId,
      endpoint: command.endpoint, apiKey: command.apiKey,
    });

      return evaluate(`(async()=>{
        const input=${input},request=window.__atSwitchTraeRequest.request.bind(
          window.__atSwitchTraeRequest);
        const before=await request({service:'model',method:'model_list',data:{}});
        if(before?.code!==0)return{added:false,reason:'model_list_failed'};
        const duplicates=(before.data?.model_list??[]).filter(model=>
          model.provider===input.provider&&
          (model.name?.split('//').at(-1)===input.modelId||
            model.display_name===input.displayName));
        if(duplicates.length)return{added:false,reason:'model_exists'};
        const response=await request({service:'model',method:'add_custom_model',data:{
          provider:input.provider,model_name:input.modelId,ak:input.apiKey,
          is_custom:true,auth_type:0,base_url:input.endpoint,
          display_name:input.displayName,
          config_detail:{multimodal:false,model_hyper_params:{thinking_enable:0}}
        }});
        if(response?.code!==0)return{added:false,reason:'native_add_rejected',
          code:response?.code??-1};
        for(let attempt=0;attempt<20;attempt++){
          const listed=await request({service:'model',method:'model_list',data:{}});
          const row=(listed.data?.model_list??[]).find(model=>
            model.provider===input.provider&&model.display_name===input.displayName&&
            model.name?.split('//').at(-1)===input.modelId&&
            model.base_url===input.endpoint);
          if(row?.custom_model_id)return{added:true,verified:true};
          await new Promise(resolve=>setTimeout(resolve,250));
        }
        return {added:true,verified:false};
      })()`, true, 30000);

  }
  if (command.operation === 'delete_model') {
    if (!command.displayName) throw new Error('trae_invalid_model_input');

      const name = JSON.stringify(command.displayName);
      return evaluate(`(async()=>{
        ${nativeStoragePrelude()}
        const name=${name},request=window.__atSwitchTraeRequest.request.bind(
          window.__atSwitchTraeRequest);
        const pruneCache=()=>{
          const keys=[...new Set([account+':AI.agent.model.model_list_map',
            account+'_AI.agent.model.model_list_map'])];
          for(const key of keys){
            const cached={...readJson(key)};
            let changed=false;
            for(const [label,models] of Object.entries(cached)){
              if(!Array.isArray(models))continue;
              const next=models.filter(model=>model?.display_name!==name);
              if(next.length!==models.length){cached[label]=next;changed=true}
            }
            if(changed)writeJson(key,cached);
          }
        };
        const settleCache=async()=>{
          pruneCache();
          await storage.flush();
        };
        const before=await request({service:'model',method:'model_list',data:{}});
        if(before?.code!==0)return{deleted:false,reason:'model_list_failed'};
        const rows=(before.data?.model_list??[]).filter(model=>
          model.display_name===name&&model.provider?.startsWith('custom_'));
        if(!rows.length){await settleCache();return{deleted:true,verified:true,
          alreadyAbsent:true}}
        if(rows.length!==1||!rows[0].custom_model_id)
          return{deleted:false,reason:'model_not_unique'};
        const response=await request({service:'model',method:'update_custom_model',
          data:{action:'delete',id:rows[0].custom_model_id}});
        if(response?.code!==0)return{deleted:false,reason:'native_delete_rejected',
          code:response?.code??-1};
        for(let attempt=0;attempt<20;attempt++){
          const listed=await request({service:'model',method:'model_list',data:{}});
          if(listed?.code===0&&!(listed.data?.model_list??[]).some(model=>
            model.custom_model_id===rows[0].custom_model_id)){
            await settleCache();
            return{deleted:true,verified:true};
          }
          await new Promise(resolve=>setTimeout(resolve,250));
        }
        return{deleted:true,verified:false};
      })()`, true, 30000);

  }
  if (command.operation === 'bind_storage') {

      return evaluate(`(async()=>{
        const client=window.__atSwitchTraeRequest;
        const expectedAccount=String(${JSON.stringify(command.expectedAccount ?? '')});
        // TRAE 3.4.2's minified native host names these members W / ib.
        // Verify the storage capability before using the version-gated member.
        const storage=client?.W;
        if(!storage||typeof storage.get!=='function'||
          typeof storage.store!=='function'||typeof storage.remove!=='function')
          return {ready:false,reason:'native_storage_missing'};
        const user=await client?.ib?.();
        let account=String(user?.user_id??user?.userId??user?.uid??user?.id??'');
        if(!storage)return {ready:false,reason:'native_storage_missing'};
        if(account&&expectedAccount&&account!==expectedAccount)
          return {ready:false,reason:'native_account_mismatch'};
        if(!account)return {ready:false,reason:'native_account_missing:'+
          Object.keys(user??{}).sort().join(',')};
        window.__atSwitchTraeStorage=storage;
        window.__atSwitchTraeAccount=account;
        const read=(suffix)=>{const raw=storage.get(account+':'+suffix,-1,'');
          if(!raw)return{};const value=JSON.parse(raw);
          return value&&typeof value==='object'&&!Array.isArray(value)?value:{}};
        const recent=read('AI.agent.model.recent_user_selection_by_agent_label');
        const sessions=read('AI.agent.model.session_selected_model');
        return {ready:true,hasRecent:!!Object.keys(recent).length,
          hasSession:!!Object.keys(sessions).length};
      })()`, true);

  }
  if (command.operation === 'select_auto') {
    const label = command.label || modelLabel;

      const targetSession = typeof command.sessionId === 'string' ? command.sessionId : null;
      return evaluate(`(()=>{${nativeStoragePrelude()}
        const label=${JSON.stringify(label)},targetSession=${JSON.stringify(targetSession)};
        const recentKey=account+':AI.agent.model.recent_user_selection_by_agent_label';
        const sessionKey=account+':AI.agent.model.session_selected_model';
        const selection={modelId:'',mode:1};
        const recent={...readJson(recentKey),[label]:selection};
        writeJson(recentKey,recent);
        const sessions={...readJson(sessionKey)};
        if(targetSession)sessions[targetSession]={...sessions[targetSession],
          [label]:selection};
        if(targetSession)writeJson(sessionKey,sessions);
        const savedRecent=readJson(recentKey)[label];
        const savedSession=targetSession?readJson(sessionKey)[targetSession]?.[label]:null;
        return {selection,verified:savedRecent?.mode===1&&savedRecent?.modelId===''&&
          (!targetSession||(savedSession?.mode===1&&savedSession?.modelId==='')),
          activeSession:!!targetSession};
      })()`);

  }
  if (command.operation === 'select') {
    const label = command.label || modelLabel;
    const input = JSON.stringify({
      displayName: command.displayName,
      mode: command.mode,
      label,
    });

      const targetSession = typeof command.sessionId === 'string' ? command.sessionId : null;
      return evaluate(`(async()=>{${nativeStoragePrelude()}
        ${canonicalModelSource}
        const input=${input},targetSession=${JSON.stringify(targetSession)};
        const rows=(await loadModels(input.label)).filter(model=>
          model?.display_name===input.displayName||model?.name===input.displayName);
        if(rows.length!==1)return{verified:false,reason:'model_not_unique',
          count:rows.length};
        const mode=Number.isInteger(input.mode)?input.mode:0;
        const selection={modelId:persistedKey(input.label,rows[0]),mode};
        const recentKey=account+':AI.agent.model.recent_user_selection_by_agent_label';
        const sessionKey=account+':AI.agent.model.session_selected_model';
        writeJson(recentKey,{...readJson(recentKey),[input.label]:selection});
        const sessions={...readJson(sessionKey)};
        if(targetSession)sessions[targetSession]={...sessions[targetSession],
          [input.label]:selection};
        if(targetSession)writeJson(sessionKey,sessions);
        const savedRecent=readJson(recentKey)[input.label];
        const savedSession=targetSession?readJson(sessionKey)[targetSession]?.[input.label]:null;
        return {selection,verified:savedRecent?.modelId===selection.modelId&&
          savedRecent?.mode===mode&&(!targetSession||
            (savedSession?.modelId===selection.modelId&&savedSession?.mode===mode)),
          activeSession:!!targetSession};
      })()`, true, 30000);

  }
  if (command.operation === 'snapshot') {
    const label = command.label || modelLabel;
    if (!selectionLabels.includes(label)) throw new Error('trae_invalid_selection_label');

      return evaluate(`(async()=>{${nativeStoragePrelude()}
        ${canonicalModelSource}
        const label=${JSON.stringify(label)};
        const native=window.__atSwitchNativeModels??await window.__atSwitchTraeRequest.request({
          service:'model',method:'model_list',data:{}});
        if(native?.code!==0)return{code:native?.code??-1};
        window.__atSwitchNativeModels=native;
        const rows=await loadModels(label);
        const recent=readJson(account+
          ':AI.agent.model.recent_user_selection_by_agent_label');
        const activeSession=${runtimeUsesExport ? 'null' :
          'window.__atSwitchTraeRequire(6970).cN(window.__atSwitchTraeRequire(6970).u7.getState().route)'};
        const sessions=readJson(account+':AI.agent.model.session_selected_model');
        const stored=recent[label];
        const selected=stored??{modelId:'',mode:1};
        const current=selected.modelId?
          rows.find(model=>persistedKey(label,model)===selected.modelId)??
          (()=>{const selectedKey=String(selected.modelId);
            const stable=selectedKey.slice(0,selectedKey.lastIndexOf('_'));
            const matches=rows.filter(model=>
              {const key=persistedKey(label,model);
                return key.slice(0,key.lastIndexOf('_'))===stable});
            return matches.length===1?matches[0]:undefined})():undefined;
        const activeStored=activeSession?sessions[activeSession]?.[label]:undefined;
        const activeSelected=activeStored??selected;
        const activeCurrent=activeSelected.modelId?
          rows.find(model=>persistedKey(label,model)===activeSelected.modelId)??
          (()=>{const key=String(activeSelected.modelId);
            const stable=key.slice(0,key.lastIndexOf('_'));
            const matches=rows.filter(model=>{const candidate=persistedKey(label,model);
              return candidate.slice(0,candidate.lastIndexOf('_'))===stable});
            return matches.length===1?matches[0]:undefined})():undefined;
        if(selected.mode!==1&&!current)
          throw new Error('trae_selection_model_missing:'+label);
        return {code:0,selection:{mode:selected.mode,modelId:selected.modelId??'',
          displayName:current?.display_name??current?.name??''},
          activeSession:!!activeSession,activeSessionId:activeSession??null,
          activeSelection:{mode:activeSelected.mode,modelId:activeSelected.modelId??'',
            displayName:activeSelected.mode===1?'':
              activeCurrent?.display_name??activeCurrent?.name??''},
          selectionBaseline:{present:Object.hasOwn(recent,label),
            modelId:stored?.modelId??null,mode:Number.isInteger(stored?.mode)?stored.mode:null,
            activeSessionId:activeSession??null,
            sessionPresent:!!activeSession&&Object.hasOwn(sessions[activeSession]??{},label),
            sessionModelId:activeStored?.modelId??null,
            sessionMode:Number.isInteger(activeStored?.mode)?activeStored.mode:null},
          models:(native.data?.model_list??[]).map(model=>({name:model.name??'',
            displayName:model.display_name??'',provider:model.provider??'',
            baseUrl:model.base_url??''}))};
      })()`, true, 30000);

  }
  if (command.operation === 'shutdown') {
    closing = true;
    if (command.terminateApp) await terminateBrowser();
    else {
      browser.stdio[3].destroy();
      browser.stdio[4].destroy();
    }
    return { stopped: true };
  }
  if (command.operation === 'release') {
    // Closing the CDP pipe also closes this Electron instance. Keep only the
    // pipe-owning helper alive until Trae itself exits; no model RPC remains.
    sessionId = undefined;
    scripts.clear();
    breakpoints.clear();
    captured.clear();
    released = true;
    return { released: true };
  }
  throw new Error('trae_bridge_unknown_operation');
}

browser.on('error', () => emit({ ready: false, error: 'trae_launch_failed' }));
start().then(() => {
  emit({ ready: true });
  const lines = createInterface({ input: process.stdin });
  let queue = Promise.resolve();
  lines.on('line', (line) => {
    queue = queue.then(async () => {
      let command;
      try { command = JSON.parse(line); } catch { emit({ ok: false, error: 'invalid_request' }); return; }
      try { emit({ ok: true, result: await handle(command) }); }
      catch (error) { emit({ ok: false, error: String(error.message).slice(0, 600) }); }
      if (closing) process.exit(0);
    });
  });
  lines.on('close', async () => {
    if (released || keepOnEof) return;
    if (!closing) {
      if (terminateOnEof) await terminateBrowser();
      else {
        browser.stdio[3].destroy();
        browser.stdio[4].destroy();
      }
    }
    process.exit(0);
  });
}).catch(async (error) => {
  emit({ ready: false, error: `${startupStage}:${String(error.message)}`.slice(0, 600) });
  await terminateBrowser();
  process.exit(1);
});
