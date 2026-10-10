// Runs with the target Trae installation's bundled Electron in Node mode.
// Only the control channel uses CDP; model requests remain inside Trae.
const { spawn } = require('node:child_process');
const { readFileSync } = require('node:fs');
const { createInterface } = require('node:readline');

const executable = process.argv[2];
const kind = process.argv[3];
// Code's Agent and IDE composers persist independent selectors.
const modelLabel = kind === 'traework' ? 'solo_work_lite' : 'solo_agent_lite';
const selectionLabels = kind === 'traecode'
  ? ['solo_agent_lite', 'solo_agent', 'solo_coder'] : [modelLabel];
// The IDE composer is owned by ai-modules-chat, not ModelAppService. Its
// global model/mode pair uses the same account-scoped VS Code storage service.
const ideStorage = `const q=window.__atSwitchTraeRequire;
  const app=q(14284).$t.getInstance().resolve(q(97594).B.ModelAppService);
  const di=q(97988).mc,container=di.getInstance();
  di.applyContainerToInstance(app.modelPersistenceService,container);
  di.applyContainerToInstance(app.modelPersistenceService.storagePort,container);
  const account=app.modelPersistenceService.storagePort.getUserId();
  const storage=container.resolveOrUndefined(q(13827).k.IStorageService);
  if(!account||!storage)throw new Error('trae_ide_storage_unavailable');
  const modelStorageKey=account+'_ai-chat:sessionRelation:globalModelMap';
  const modeStorageKey=account+'_ai-chat:sessionRelation:globalModeMap';
  const readScope=(key,scope)=>{const raw=storage.get(key,scope,'');
    if(!raw)return{};const value=JSON.parse(raw);
    if(!value||Array.isArray(value)||typeof value!=='object')
      throw new Error('trae_ide_storage_invalid');
    return value;};
  const workspaceModels=readScope(modelStorageKey,1);
  const workspaceModes=readScope(modeStorageKey,1);
  const appModels=readScope(modelStorageKey,-1);
  const appModes=readScope(modeStorageKey,-1);
  const modelKey=(model)=>model.custom_model_id?
    [model.config_source??'',model.provider??'-',model.name??'',
      model.custom_model_id].join('_'):
    [model.config_source??'',model.provider??'-',model.name??''].join('_');
  const state=app.modelDomainService.modelStore.chatStore.getState().domain.model;
  const known=(key)=>(state.byAgentLabel.solo_coder??[])
    .some(id=>{const model=state.entities[id];return model&&modelKey(model)===key});
  const workspaceValid=workspaceModes.solo_coder===1||
    (workspaceModes.solo_coder===0&&known(workspaceModels.solo_coder));
  const models=workspaceValid?workspaceModels:appModels;
  const modes=workspaceValid?workspaceModes:appModes;`;
// Generic fetchModels does not refresh both visible Code selectors.
const refreshSelectionModels = `await app.fetchModels();
     const native=await window.__atSwitchTraeRequest.request({
       service:'model',method:'model_list',data:{}});
     if(native?.code!==0)throw new Error('trae_native_model_list_failed');
     window.__atSwitchTraeModelListCache=native;
     const nativeRows=(native.data?.model_list??[]).filter(row=>
       row.provider?.startsWith('custom_')&&row.name);
     for(const label of ${JSON.stringify(selectionLabels)}){
       let listed;
       for(let attempt=0;attempt<4;attempt++){
         listed=await app.traeApiPort.model.listModels({
           functions:label,show_custom_model:true});
         if(listed?.code===0)break;
         await new Promise(resolve=>setTimeout(resolve,250));
       }
       if(listed?.code!==0)throw new Error('trae_selection_model_list_failed');
       const canonicalList=(listed.data?.list??[]).map(group=>({
         ...group,models:(group.models??[]).map(model=>{
           if(!model.provider?.startsWith('custom_'))return model;
           const byId=nativeRows.filter(row=>
             row.provider===model.provider&&
             String(row.custom_model_id)===String(model.custom_model_id));
           const byName=nativeRows.filter(row=>
             row.provider===model.provider&&
             row.display_name===model.display_name);
           const match=byId.length===1?byId[0]:byName.length===1?byName[0]:undefined;
           return match?{...model,name:match.name}:model;
         })}));
       app.modelDomainService.batchRefreshModels(
         app.convertToRawModelListMap(canonicalList));
     }`;
const profile = process.argv[4];
const terminateOnEof = process.argv[5] === 'true';
if (!executable) {
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
      if (message.error) reject(new Error(`cdp_error:${method}:${message.error.code}`));
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
        ? 'window.__atSwitchTraeRequire=c'
        : 'window.__atSwitchTraeRequest=this';
      const result = await request('Debugger.evaluateOnCallFrame', {
        callFrameId, expression, returnByValue: false,
      });
      if (!result.exceptionDetails) captured.add(kind);
      await request('Debugger.removeBreakpoint', { breakpointId });
      breakpoints.delete(breakpointId);
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
  if (result.exceptionDetails) throw new Error('trae_internal_evaluation_failed');
  return result.result?.value;
}

async function start() {
  const target = await waitFor(async () => {
    const result = await request('Target.getTargets', {}, undefined);
    return result.targetInfos?.find((item) =>
      item.type === 'page' && item.url?.includes('solo-lite.html'));
  }, 15000, 'workbench');
  sessionId = (await request('Target.attachToTarget', {
    targetId: target.targetId, flatten: true,
  }, undefined)).sessionId;
  if (!sessionId) throw new Error('trae_debugger_attach_failed');
  await request('Debugger.enable');
  const urls = await waitFor(() => {
    const all = [...scripts.keys()];
    const runtime = all.find((url) => url.includes('/solo-lite/dist/index.mjs'));
    const workbench = all.find((url) => url.includes('workbench.desktop.main.solo-lite-slim.js'));
    return runtime && workbench ? { runtime, workbench } : undefined;
  }, 15000, 'scripts');
  const signatures = {
    runtime: ['var p=c(c.s=52069).A;'],
    workbench: [
      'async request(e){const[t,s]=await Promise.all([this.tb(),this.ab()])',
      'async request(e){const[t,n]=await Promise.all([this.getDeviceId(),this.createUserInfo()])',
    ],
  };
  const armCapture = async (kind) => {
    const url = urls[kind];
    const location = positionFor(url, signatures[kind]);
    const result = await request('Debugger.setBreakpointByUrl', { url, ...location });
    if (!result.breakpointId || !result.locations?.length) {
      throw new Error(`trae_debugger_breakpoint_failed:${kind}`);
    }
    breakpoints.set(result.breakpointId, kind);
  };
  for (const kind of ['runtime', 'workbench']) await armCapture(kind);
  await request('Page.enable');
  await request('Page.reload', { ignoreCache: false });
  await waitFor(() => captured.has('runtime'), 15000, 'runtime_capture');
  await waitFor(async () => evaluate('!!window.__atSwitchTraeRequire?.m?.[97594]')
    .catch(() => false),
    30000, 'model_module');
  const modelServiceReady = async () => evaluate('(()=>{try{const q=window.__atSwitchTraeRequire;'
    + 'return !!q(14284).$t.getInstance().resolve(q(97594).B.ModelAppService)'
    + '}catch{return false}})()').catch(() => false);
  try {
    await waitFor(modelServiceReady, 20000, 'model_service');
  } catch {
    // The hidden solo-lite renderer can load before its model DI service.
    // Reload that renderer once without restarting the desktop process.
    for (const breakpointId of breakpoints.keys()) {
      await request('Debugger.removeBreakpoint', { breakpointId });
    }
    breakpoints.clear();
    captured.clear();
    for (const kind of ['runtime', 'workbench']) await armCapture(kind);
    await request('Page.reload', { ignoreCache: false });
    await waitFor(() => captured.has('runtime'), 15000, 'runtime_recapture');
    await waitFor(modelServiceReady, 30000, 'model_service');
  }
  if (!captured.has('workbench')) {
    await evaluate('(async()=>{const q=window.__atSwitchTraeRequire;'
      + 'const a=q(14284).$t.getInstance().resolve(q(97594).B.ModelAppService);'
      + 'await a.ensureModelsLoaded()})()', true);
    await waitFor(() => captured.has('workbench'), 10000, 'request_capture');
  }
  await request('Debugger.disable');
  const health = await evaluate('({request:typeof window.__atSwitchTraeRequest?.request,'
    + 'runtime:typeof window.__atSwitchTraeRequire})');
  if (health.request !== 'function' || health.runtime !== 'function') {
    throw new Error('trae_bridge_not_ready');
  }
}

async function handle(command) {
  if (command.operation === 'snapshot_ide') {
    if (kind !== 'traecode') throw new Error('trae_ide_unsupported');
    return evaluate(`(()=>{${ideStorage}
      const key=models.solo_coder??'',mode=modes.solo_coder;
      const matches=(state.byAgentLabel.solo_coder??[])
        .map(id=>state.entities[id]).filter(model=>model&&modelKey(model)===key);
      const selectedMode=mode===0&&matches.length===1?0:1;
      const scopeState=(scope)=>{
        const modelRaw=storage.get(modelStorageKey,scope,'');
        const modeRaw=storage.get(modeStorageKey,scope,'');
        const scopeModels=readScope(modelStorageKey,scope);
        const scopeModes=readScope(modeStorageKey,scope);
        return {modelKey:Object.hasOwn(scopeModels,'solo_coder')?
          scopeModels.solo_coder:null,
          mode:Object.hasOwn(scopeModes,'solo_coder')?scopeModes.solo_coder:null,
          modelStoragePresent:!!modelRaw,modeStoragePresent:!!modeRaw};
      };
      return {code:0,selection:{mode:selectedMode,modelId:key,
        displayName:selectedMode===0?
          matches[0].display_name??matches[0].name??'':''},
        activeSession:false,activeSessionId:null,
        activeSelection:{mode:selectedMode,modelId:key,displayName:''},models:[],
        ideBaseline:{workspace:scopeState(1),app:scopeState(-1)}};
    })()`);
  }
  if (command.operation === 'restore_ide') {
    if (kind !== 'traecode') throw new Error('trae_ide_unsupported');
    const baseline = JSON.stringify(command.baseline);
    return evaluate(`(()=>{${ideStorage}
      const baseline=${baseline};
      const restore=(scope,saved)=>{
        if(!saved||!(saved.modelKey===null||typeof saved.modelKey==='string')||
          !(saved.mode===null||Number.isInteger(saved.mode))||
          typeof saved.modelStoragePresent!=='boolean'||
          typeof saved.modeStoragePresent!=='boolean')
          throw new Error('trae_ide_baseline_invalid');
        const nextModels={...readScope(modelStorageKey,scope)};
        const nextModes={...readScope(modeStorageKey,scope)};
        if(saved.modelKey===null)delete nextModels.solo_coder;
        else nextModels.solo_coder=saved.modelKey;
        if(saved.mode===null)delete nextModes.solo_coder;
        else nextModes.solo_coder=saved.mode;
        if(!saved.modelStoragePresent&&!Object.keys(nextModels).length)
          storage.remove(modelStorageKey,scope);
        else storage.store(modelStorageKey,JSON.stringify(nextModels),scope,1);
        if(!saved.modeStoragePresent&&!Object.keys(nextModes).length)
          storage.remove(modeStorageKey,scope);
        else storage.store(modeStorageKey,JSON.stringify(nextModes),scope,1);
        const modelsAfter=readScope(modelStorageKey,scope);
        const modesAfter=readScope(modeStorageKey,scope);
        return (modelsAfter.solo_coder??null)===saved.modelKey&&
          (modesAfter.solo_coder??null)===saved.mode;
      };
      const workspaceRestored=restore(1,baseline.workspace);
      const appRestored=restore(-1,baseline.app);
      return {verified:workspaceRestored&&appRestored};
    })()`);
  }
  if (command.operation === 'select_ide') {
    if (kind !== 'traecode') throw new Error('trae_ide_unsupported');
    const name = JSON.stringify(command.displayName);
    return evaluate(`(()=>{${ideStorage}
      const name=${name};let nextKey=known(models.solo_coder)?models.solo_coder:
        undefined,nextMode=1;
      if(name!=='Auto'&&name!=='Auto Mode'){
        const matches=(state.byAgentLabel.solo_coder??[])
          .map(id=>state.entities[id]).filter(model=>
            model?.display_name===name||model?.name===name);
        if(matches.length!==1)return{verified:false,reason:'model_not_unique',
          count:matches.length};
        nextKey=modelKey(matches[0]);nextMode=0;
      }
      const nextModels={...models,solo_coder:nextKey};
      if(nextKey===undefined)delete nextModels.solo_coder;
      const nextModes={...modes,solo_coder:nextMode};
      for(const scope of [1,-1]){
        storage.store(modelStorageKey,JSON.stringify(nextModels),scope,1);
        storage.store(modeStorageKey,JSON.stringify(nextModes),scope,1);
      }
      return{verified:[1,-1].every(scope=>{
        const savedModels=readScope(modelStorageKey,scope);
        const savedModes=readScope(modeStorageKey,scope);
        return savedModels.solo_coder===nextKey&&savedModes.solo_coder===nextMode;
      })};
    })()`);
  }
  if (command.operation === 'restore_selection') {
    return evaluate(`(async()=>{
      const q=window.__atSwitchTraeRequire;
      const app=q(14284).$t.getInstance().resolve(q(97594).B.ModelAppService);
      try {
        await app.ensureModelsLoaded();
        const di=q(97988).mc,container=di.getInstance();
        di.applyContainerToInstance(app.traeApiPort,container);
        di.applyContainerToInstance(app.traeApiPort.model,container);
        ${refreshSelectionModels}
        await app.restoreUserSelection();
        return {restored:true};
      }
      catch(error) { return {restored:false,errorName:error?.name??''}; }
    })()`, true, 30000);
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
    const expression = `(async()=>{
      const input=${input},request=window.__atSwitchTraeRequest.request.bind(
        window.__atSwitchTraeRequest);
      const before=await request({service:'model',method:'model_list',data:{}});
      if(before?.code!==0) return {added:false,reason:'model_list_failed'};
      const duplicates=(before.data?.model_list??[]).filter(model=>
        model.provider===input.provider&&
        (model.name?.split('//').at(-1)===input.modelId||
          model.display_name===input.displayName));
      if(duplicates.length) return {added:false,reason:'model_exists'};
      const response=await request({service:'model',method:'add_custom_model',data:{
        provider:input.provider,model_name:input.modelId,ak:input.apiKey,
        is_custom:true,auth_type:0,base_url:input.endpoint,
        display_name:input.displayName,
        config_detail:{multimodal:false,model_hyper_params:{thinking_enable:0}}
      }});
      if(response?.code!==0) return {added:false,reason:'native_add_rejected',
        code:response?.code??-1};
      for(let attempt=0;attempt<20;attempt++){
        const listed=await request({service:'model',method:'model_list',data:{}});
        const row=(listed.data?.model_list??[]).find(model=>
          model.provider===input.provider&&model.display_name===input.displayName&&
          model.name?.split('//').at(-1)===input.modelId&&
          model.base_url===input.endpoint);
        if(row?.custom_model_id){
          const q=window.__atSwitchTraeRequire;
          const app=q(14284).$t.getInstance().resolve(q(97594).B.ModelAppService);
          const di=q(97988).mc,container=di.getInstance();
          di.applyContainerToInstance(app.modelPersistenceService,container);
          di.applyContainerToInstance(app.modelPersistenceService.storagePort,container);
          di.applyContainerToInstance(app.traeApiPort,container);
          di.applyContainerToInstance(app.traeApiPort.model,container);
          ${refreshSelectionModels}
          const label=${JSON.stringify(modelLabel)};
          const state=app.modelDomainService.modelStore.chatStore.getState().domain.model;
          const selectable=(state.byAgentLabel[label]??[]).some(key=>
            state.entities[key]?.display_name===input.displayName);
          return {added:true,verified:selectable};
        }
        await new Promise(resolve=>setTimeout(resolve,250));
      }
      return {added:true,verified:false};
    })()`;
    return evaluate(expression, true, 30000);
  }
  if (command.operation === 'delete_model') {
    if (!command.displayName) throw new Error('trae_invalid_model_input');
    const expression = `(async()=>{
      const name=${JSON.stringify(command.displayName)};
      const request=window.__atSwitchTraeRequest.request.bind(window.__atSwitchTraeRequest);
      const before=await request({service:'model',method:'model_list',data:{}});
      if(before?.code!==0) return {deleted:false,reason:'model_list_failed'};
      const rows=(before.data?.model_list??[]).filter(model=>
        model.display_name===name&&model.provider?.startsWith('custom_'));
      if(!rows.length)return {deleted:true,alreadyAbsent:true};
      if(rows.length!==1||!rows[0].custom_model_id)
        return {deleted:false,reason:'model_not_unique'};
      const response=await request({service:'model',method:'update_custom_model',
        data:{action:'delete',id:rows[0].custom_model_id}});
      if(response?.code!==0)return {deleted:false,reason:'native_delete_rejected',
        code:response?.code??-1};
      for(let attempt=0;attempt<20;attempt++){
        const listed=await request({service:'model',method:'model_list',data:{}});
        if(listed?.code===0&&!(listed.data?.model_list??[]).some(model=>
          model.custom_model_id===rows[0].custom_model_id)){
          const q=window.__atSwitchTraeRequire;
          const app=q(14284).$t.getInstance().resolve(q(97594).B.ModelAppService);
          const di=q(97988).mc,container=di.getInstance();
          di.applyContainerToInstance(app.modelPersistenceService,container);
          di.applyContainerToInstance(app.modelPersistenceService.storagePort,container);
          di.applyContainerToInstance(app.traeApiPort,container);
          di.applyContainerToInstance(app.traeApiPort.model,container);
          ${refreshSelectionModels}
          return {deleted:true,verified:true};
        }
        await new Promise(resolve=>setTimeout(resolve,250));
      }
      return {deleted:true,verified:false};
    })()`;
    return evaluate(expression, true, 30000);
  }
  if (command.operation === 'bind_storage') {
    return evaluate(`(async()=>{
      const q=window.__atSwitchTraeRequire;
      const app=q(14284).$t.getInstance().resolve(q(97594).B.ModelAppService);
      const di=q(97988).mc,container=di.getInstance();
      if(!container.resolveOrUndefined(q(13827).k.IStorageService))
        return {ready:false,reason:'native_storage_missing'};
      const persistence=app.modelPersistenceService;
      di.applyContainerToInstance(persistence,container);
      di.applyContainerToInstance(persistence.storagePort,container);
      for(let attempt=0;attempt<120&&!persistence.storagePort.getUserId();attempt++){
        await new Promise(resolve=>setTimeout(resolve,250));
      }
      if(!persistence.storagePort.getUserId())
        return {ready:false,reason:'native_account_missing'};
      const saved=await persistence.getUserSelection();
      return {ready:true,hasRecent:!!saved.recentUserSelectionByAgentLabel,
        hasSession:!!saved.sessionSelectedModel};
    })()`, true);
  }
  if (command.operation === 'select_auto') {
    const label = command.label || modelLabel;
    const activeSessionExpression = Object.hasOwn(command, 'sessionId')
      ? JSON.stringify(command.sessionId) : 'q(6970).cN(q(6970).u7.getState().route)';
    const expression = `(async()=>{
      const q=window.__atSwitchTraeRequire;
      const app=q(14284).$t.getInstance().resolve(q(97594).B.ModelAppService);
      const di=q(97988).mc,container=di.getInstance();
      const persistence=app.modelPersistenceService;
      di.applyContainerToInstance(persistence,container);
      di.applyContainerToInstance(persistence.storagePort,container);
      const activeSession=${activeSessionExpression};
      app.updateModelSelection(undefined,${JSON.stringify(label)},'',1);
      if(activeSession) app.updateModelSelection(activeSession,${JSON.stringify(label)},'',1);
      await persistence.saveUserSelection({
        sessionSelectedModel:app.modelDomainService.getSessionSelectedModelMap(),
        recentUserSelectionByAgentLabel:app.modelDomainService.getRecentUserSelectionMap()});
      const saved=await persistence.getUserSelection();
      const recent=saved.recentUserSelectionByAgentLabel?.[${JSON.stringify(label)}];
      const session=activeSession?
        saved.sessionSelectedModel?.[activeSession]?.[${JSON.stringify(label)}]:undefined;
      return {verified:recent?.mode===1&&recent?.modelId===''&&
        (!activeSession||(session?.mode===1&&session?.modelId==='')),
        activeSession:!!activeSession};
    })()`;
    return evaluate(expression, true, 30000);
  }
  if (command.operation === 'select') {
    const label = command.label || modelLabel;
    const activeSessionExpression = Object.hasOwn(command, 'sessionId')
      ? JSON.stringify(command.sessionId) : 'q(6970).cN(q(6970).u7.getState().route)';
    const input = JSON.stringify({
      displayName: command.displayName,
      mode: command.mode,
      label,
    });
    const expression = `(async()=>{
      const input=${input},q=window.__atSwitchTraeRequire;
      const app=q(14284).$t.getInstance().resolve(q(97594).B.ModelAppService);
      const domain=app.modelDomainService;
      const di=q(97988).mc,container=di.getInstance();
      di.applyContainerToInstance(app.traeApiPort,container);
      di.applyContainerToInstance(app.traeApiPort.model,container);
      let keys=[];
      for(let attempt=0;attempt<4;attempt++){
        const state=domain.modelStore.chatStore.getState().domain.model;
        keys=(state.byAgentLabel[input.label]??[]).filter(key=>{
          const model=state.entities[key];
          return model?.display_name===input.displayName||model?.name===input.displayName;
        });
        if(keys.length)break;
        ${refreshSelectionModels}
        await new Promise(resolve=>setTimeout(resolve,250));
      }
      if(keys.length!==1) return {verified:false,reason:'model_not_unique',count:keys.length};
      const persistence=app.modelPersistenceService;
      di.applyContainerToInstance(persistence,container);
      di.applyContainerToInstance(persistence.storagePort,container);
      const activeSession=${activeSessionExpression};
      const mode=Number.isInteger(input.mode)?input.mode:0;
      app.updateModelSelection(undefined,input.label,keys[0],mode);
      if(activeSession) app.updateModelSelection(activeSession,input.label,keys[0],mode);
      await persistence.saveUserSelection({
        sessionSelectedModel:domain.getSessionSelectedModelMap(),
        recentUserSelectionByAgentLabel:domain.getRecentUserSelectionMap()});
      const saved=await persistence.getUserSelection();
      const recent=saved.recentUserSelectionByAgentLabel?.[input.label];
      const session=activeSession?
        saved.sessionSelectedModel?.[activeSession]?.[input.label]:undefined;
      return {verified:recent?.modelId===keys[0]&&recent?.mode===mode&&
        (!activeSession||(session?.modelId===keys[0]&&session?.mode===mode)),
        activeSession:!!activeSession};
    })()`;
    return evaluate(expression, true, 50000);
  }
  if (command.operation === 'snapshot') {
    const label = command.label || modelLabel;
    if (!selectionLabels.includes(label)) throw new Error('trae_invalid_selection_label');
    const expression = `(async()=>{
      const q=window.__atSwitchTraeRequire;
      const app=q(14284).$t.getInstance().resolve(q(97594).B.ModelAppService);
      const domain=app.modelDomainService;
      let activeSession=q(6970).cN(q(6970).u7.getState().route);
      if(!activeSession&&${JSON.stringify(kind === 'traecode')}&&
          !window.__atSwitchTraeRouteWaited){
        window.__atSwitchTraeRouteWaited=true;
        for(let attempt=0;attempt<24&&!activeSession;attempt++){
          await new Promise(resolve=>setTimeout(resolve,250));
          activeSession=q(6970).cN(q(6970).u7.getState().route);
        }
      }
      const selected=domain.getUserSelectedModel(undefined,${JSON.stringify(label)},
        app.getModelSelectionPolicy());
      const current=selected?.modelId?domain.getModelByKey(selected.modelId):undefined;
      const activeSelected=activeSession?domain.getUserSelectedModel(activeSession,
        ${JSON.stringify(label)},app.getModelSelectionPolicy()):undefined;
      const activeModel=activeSelected?.modelId?
        domain.getModelByKey(activeSelected.modelId):undefined;
      const response=window.__atSwitchTraeModelListCache??
        await window.__atSwitchTraeRequest.request({
          service:'model',method:'model_list',data:{}});
      if(response?.code!==0) return {code:response?.code??-1};
      return {code:0,selection:{mode:selected?.mode,
        modelId:selected?.modelId??'',displayName:current?.display_name??current?.name??''},
        activeSession:!!activeSession,activeSessionId:activeSession??null,
        activeSelection:{mode:activeSelected?.mode,
          modelId:activeSelected?.modelId??'',
          displayName:activeModel?.display_name??activeModel?.name??''},
        models:(response.data?.model_list??[]).map(model=>({
          name:model.name??'',
          displayName:model.display_name??'',provider:model.provider??'',
          baseUrl:model.base_url??''}))};
    })()`;
    return evaluate(expression, true, 50000);
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
      catch (error) { emit({ ok: false, error: String(error.message).slice(0, 120) }); }
      if (closing) process.exit(0);
    });
  });
  lines.on('close', async () => {
    if (released) return;
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
  emit({ ready: false, error: String(error.message).slice(0, 120) });
  await terminateBrowser();
  process.exit(1);
});
