// Runs with the target Trae installation's bundled Electron in Node mode.
// Only the control channel uses CDP; model requests remain inside Trae.
const { spawn } = require('node:child_process');
const { readFileSync } = require('node:fs');
const { createInterface } = require('node:readline');

const executable = process.argv[2];
const kind = process.argv[3];
// Visible Code and Work task composers use their lite selectors. The remote
// selectors belong to different workflows and can diverge from the UI.
const modelLabel = kind === 'traework' ? 'solo_work_lite' : 'solo_agent_lite';
// Generic fetchModels requests remote variants. Refresh the visible lite
// selector explicitly after each native model-service mutation.
const refreshSelectionModels = `await app.fetchModels();
     const lite=await app.traeApiPort.model.listModels({
       functions:${JSON.stringify(modelLabel)},show_custom_model:true});
     if(lite?.code!==0)throw new Error('trae_lite_model_list_failed');
     const native=await window.__atSwitchTraeRequest.request({
       service:'model',method:'model_list',data:{}});
     if(native?.code!==0)throw new Error('trae_native_model_list_failed');
     const nativeRows=(native.data?.model_list??[]).filter(row=>
       row.provider?.startsWith('custom_')&&row.name);
     const canonicalList=(lite.data?.list??[]).map(group=>({
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
       app.convertToRawModelListMap(canonicalList));`;
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
  for (const kind of ['runtime', 'workbench']) {
    const url = urls[kind];
    const location = positionFor(url, signatures[kind]);
    const result = await request('Debugger.setBreakpointByUrl', { url, ...location });
    if (!result.breakpointId || !result.locations?.length) {
      throw new Error(`trae_debugger_breakpoint_failed:${kind}`);
    }
    breakpoints.set(result.breakpointId, kind);
  }
  await request('Page.enable');
  await request('Page.reload', { ignoreCache: false });
  await waitFor(() => captured.has('runtime'), 15000, 'runtime_capture');
  await waitFor(async () => evaluate('!!window.__atSwitchTraeRequire?.m?.[97594]'),
    30000, 'model_module');
  await waitFor(async () => evaluate('(()=>{try{const q=window.__atSwitchTraeRequire;'
    + 'return !!q(14284).$t.getInstance().resolve(q(97594).B.ModelAppService)'
    + '}catch{return false}})()'), 30000, 'model_service');
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
      await app.ensureModelsLoaded();
      const domain=app.modelDomainService;
      const di=q(97988).mc,container=di.getInstance();
      di.applyContainerToInstance(app.traeApiPort,container);
      di.applyContainerToInstance(app.traeApiPort.model,container);
      let keys=[];
      for(let attempt=0;attempt<4;attempt++){
        ${refreshSelectionModels}
        const state=domain.modelStore.chatStore.getState().domain.model;
        keys=(state.byAgentLabel[input.label]??[]).filter(key=>{
          const model=state.entities[key];
          return model?.display_name===input.displayName||model?.name===input.displayName;
        });
        if(keys.length===1)break;
        if(keys.length>1)break;
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
    const label = modelLabel;
    const expression = `(async()=>{
      const q=window.__atSwitchTraeRequire;
      const app=q(14284).$t.getInstance().resolve(q(97594).B.ModelAppService);
      await app.ensureModelsLoaded();
      const domain=app.modelDomainService;
      let activeSession=q(6970).cN(q(6970).u7.getState().route);
      if(!activeSession&&${JSON.stringify(kind === 'traecode')}){
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
      const response=await window.__atSwitchTraeRequest.request({
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
    return evaluate(expression, true);
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
