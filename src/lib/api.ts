import type {
  AgentBindingDraft,
  AgentSummary,
  AppSettings,
  AppSnapshot,
  ProviderDraft,
  ProviderSummary,
  ProxyStatus,
} from "../types";

const isTauri = () => "__TAURI_INTERNALS__" in window;

function browserPlatform(): string {
  const platform = navigator.platform.toLowerCase();
  if (platform.includes("mac")) return "macos";
  if (platform.includes("win")) return "windows";
  return "browser";
}

const mockProviderApiKeys = new Map<string, string>([
  ["preset-mongyun", "sk-browser-preview-40a1"],
]);

function maskApiKey(value: string) {
  const characters = Array.from(value);
  if (characters.length <= 4) return "•".repeat(characters.length);
  return `${"•".repeat(characters.length - 4)}${characters.slice(-4).join("")}`;
}

function aggregateVerificationStatus(
  models: Array<{
    outputModality: "text" | "image" | "audio" | "video";
    verificationStatus: ProviderSummary["verificationStatus"];
  }>,
): ProviderSummary["verificationStatus"] {
  const textModels = models.filter((model) => model.outputModality === "text");
  if (!textModels.length) return "draft_unverified";
  if (textModels.some((model) => model.verificationStatus === "verifying")) {
    return "verifying";
  }
  if (textModels.some((model) => model.verificationStatus === "failed")) {
    return "failed";
  }
  if (textModels.every((model) => model.verificationStatus === "verified")) {
    return "verified";
  }
  if (textModels.some((model) => model.verificationStatus === "stale")) {
    return "stale";
  }
  return "draft_unverified";
}

function normalizedProviderUrl(value: string) {
  try {
    const url = new URL(value.trim());
    url.hash = "";
    url.pathname = url.pathname.replace(/\/+$/, "") || "/";
    return url.toString().replace(/\/$/, "");
  } catch {
    return value.trim().replace(/\/+$/, "");
  }
}

function sameProviderIdentity(
  provider: Pick<ProviderSummary, "name" | "baseUrl">,
  draft: Pick<ProviderDraft, "name" | "baseUrl">,
) {
  return (
    provider.name.trim().toLocaleLowerCase() ===
      draft.name.trim().toLocaleLowerCase() &&
    normalizedProviderUrl(provider.baseUrl) ===
      normalizedProviderUrl(draft.baseUrl)
  );
}

function mergeModelDrafts(
  existingProviders: ProviderSummary[],
  incoming: ProviderDraft["models"],
) {
  const merged: ProviderDraft["models"] = [];
  const upsert = (model: ProviderDraft["models"][number]) => {
    const index = merged.findIndex(
      (candidate) => candidate.modelId.trim() === model.modelId.trim(),
    );
    if (index >= 0) merged[index] = model;
    else merged.push(model);
  };
  existingProviders.forEach((provider) =>
    provider.models.forEach((model) =>
      upsert({
        modelId: model.modelId,
        displayName: model.displayName,
        outputModality: model.outputModality,
        supportsStreaming: model.supportsStreaming,
        supportsTools: model.supportsTools,
      }),
    ),
  );
  incoming.forEach(upsert);
  return merged;
}

const mockSnapshotTemplate: AppSnapshot = {
  appVersion: "0.1.0-dev",
  platform: browserPlatform(),
  providers: [],
  agents: [
    "workbuddy",
    "codebuddy",
    "qclaw",
    "ima",
    "autoclaw",
    "trae",
    "codex",
  ].map((id) => {
    const switchable = id !== "ima" && id !== "trae";
    return {
      id,
      displayName:
        id === "workbuddy"
          ? "WorkBuddy"
          : id === "codebuddy"
            ? "CodeBuddy"
            : id === "qclaw"
              ? "QClaw"
              : id === "ima"
                ? "ima"
                : id === "autoclaw"
                  ? "AutoClaw"
                  : id === "trae"
                    ? "TRAE"
                    : "Codex",
      installStatus: "installed" as const,
      runtimeStatus: "not_running" as const,
      configHealth: switchable
        ? ("healthy" as const)
        : ("unsupported_version" as const),
      adapterVerified: switchable,
      detectedVersion: switchable ? "preview" : undefined,
      providerId: undefined,
      providerName: undefined,
      modelId: undefined,
      mode: undefined,
      needsRestart:
        id === "workbuddy" ||
        id === "codebuddy" ||
        id === "qclaw" ||
        id === "autoclaw" ||
        id === "codex",
      automaticRestartSupported:
        id === "workbuddy" ||
        id === "codebuddy" ||
        id === "qclaw" ||
        id === "autoclaw" ||
        id === "codex",
      message: switchable
        ? "浏览器预览使用演示状态；Tauri 版本会读取本机真实安装"
        : `${id === "trae" ? "TRAE" : "ima"} 自定义模型暂由登录态内部存储管理`,
    };
  }),
  proxy: {
    status: "stopped",
    host: "127.0.0.1",
    port: 54187,
    activeConnections: 0,
    completedRequests: 0,
    successfulRequests: 0,
    conversionFailures: 0,
    upstreamFailures: 0,
  },
  settings: {
    language: "zh-CN",
    theme: "system",
    startAtLogin: false,
    keepRunningInBackground: false,
  },
};

const realSnapshotData: AppSnapshot = {
  appVersion: "0.1.7",
  platform: "macos",
  providers: [
    {
      id: "3e495aa8-a81a-4f85-b9a6-14434b8ddda0",
      name: "蒙云智算",
      kind: "mongyun",
      protocol: "openai_chat_completions",
      baseUrl: "https://api.g2claw.com/v1",
      isRecommended: true,
      isEnabled: true,
      hasApiKey: true,
      maskedApiKey: "•••••••••4214",
      verificationStatus: "draft_unverified",
      defaultModelId: "GLM-5.2",
      models: [
        {
          id: "3e495aa8-a81a-4f85-b9a6-14434b8ddda0:GLM-5.2",
          providerId: "3e495aa8-a81a-4f85-b9a6-14434b8ddda0",
          modelId: "GLM-5.2",
          displayName: "GLM-5.2",
          supportsStreaming: true,
          supportsTools: true,
          source: "custom",
          verificationStatus: "draft_unverified",
          outputModality: "text",
        },
      ],
    },
    {
      id: "266fc18e-9471-412d-866e-21f127d548d9",
      name: "智谱",
      kind: "zhipu",
      protocol: "openai_chat_completions",
      baseUrl: "https://open.bigmodel.cn/api/paas/v4",
      isRecommended: false,
      isEnabled: true,
      hasApiKey: true,
      maskedApiKey: "••••••••••••4325",
      verificationStatus: "draft_unverified",
      defaultModelId: "GLM-5.3",
      models: [
        {
          id: "266fc18e-9471-412d-866e-21f127d548d9:GLM-5.3",
          providerId: "266fc18e-9471-412d-866e-21f127d548d9",
          modelId: "GLM-5.3",
          displayName: "GLM-5.3",
          supportsStreaming: true,
          supportsTools: true,
          source: "custom",
          verificationStatus: "draft_unverified",
          outputModality: "text",
        },
      ],
    },
    {
      id: "da4b6432-cc8e-474c-bc5a-913b23a2ba12",
      name: "DeepSeek",
      kind: "deepseek",
      protocol: "openai_chat_completions",
      baseUrl: "https://api.deepseek.com/v1",
      isRecommended: false,
      isEnabled: true,
      hasApiKey: true,
      maskedApiKey: "•••••••••3214",
      verificationStatus: "draft_unverified",
      defaultModelId: "deepseek-v4-pro",
      models: [
        {
          id: "da4b6432-cc8e-474c-bc5a-913b23a2ba12:deepseek-v4-pro",
          providerId: "da4b6432-cc8e-474c-bc5a-913b23a2ba12",
          modelId: "deepseek-v4-pro",
          displayName: "DeepSeek V4 Pro",
          supportsStreaming: true,
          supportsTools: true,
          source: "custom",
          verificationStatus: "draft_unverified",
          outputModality: "text",
        },
      ],
    },
    {
      id: "3c02874f-d211-4902-ad9b-8c8a9204db5e",
      name: "MiniMax",
      kind: "minimax",
      protocol: "openai_chat_completions",
      baseUrl: "https://api.minimaxi.com/v1",
      isRecommended: false,
      isEnabled: true,
      hasApiKey: true,
      maskedApiKey: "••••••4324",
      verificationStatus: "draft_unverified",
      defaultModelId: "MiniMax-M3",
      models: [
        {
          id: "3c02874f-d211-4902-ad9b-8c8a9204db5e:MiniMax-M3",
          providerId: "3c02874f-d211-4902-ad9b-8c8a9204db5e",
          modelId: "MiniMax-M3",
          displayName: "MiniMax M3",
          supportsStreaming: true,
          supportsTools: true,
          source: "custom",
          verificationStatus: "draft_unverified",
          outputModality: "text",
        },
      ],
    },
    {
      id: "f5003255-f6ea-4e70-8262-84f327d28869",
      name: "Kimi",
      kind: "kimi",
      protocol: "openai_chat_completions",
      baseUrl: "https://api.moonshot.ai/v1",
      isRecommended: false,
      isEnabled: true,
      hasApiKey: true,
      maskedApiKey: "••••••••••3432",
      verificationStatus: "draft_unverified",
      defaultModelId: "kimi-k3",
      models: [
        {
          id: "f5003255-f6ea-4e70-8262-84f327d28869:kimi-k3",
          providerId: "f5003255-f6ea-4e70-8262-84f327d28869",
          modelId: "kimi-k3",
          displayName: "Kimi K3",
          supportsStreaming: true,
          supportsTools: true,
          source: "custom",
          verificationStatus: "draft_unverified",
          outputModality: "text",
        },
      ],
    },
    {
      id: "2534383b-77f5-4d1b-9316-9a1c986a532c",
      name: "通义千问",
      kind: "qwen",
      protocol: "openai_chat_completions",
      baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      isRecommended: false,
      isEnabled: true,
      hasApiKey: true,
      maskedApiKey: "•••••••4234",
      verificationStatus: "draft_unverified",
      defaultModelId: "qwen3.8-max",
      models: [
        {
          id: "2534383b-77f5-4d1b-9316-9a1c986a532c:qwen3.8-max",
          providerId: "2534383b-77f5-4d1b-9316-9a1c986a532c",
          modelId: "qwen3.8-max",
          displayName: "Qwen 3.8 Max",
          supportsStreaming: true,
          supportsTools: true,
          source: "custom",
          verificationStatus: "draft_unverified",
          outputModality: "text",
        },
      ],
    },
  ],
  agents: [
    {
      id: "workbuddy",
      displayName: "WorkBuddy",
      installStatus: "installed",
      runtimeStatus: "not_running",
      configHealth: "healthy",
      adapterVerified: true,
      detectedVersion: "5.4.7",
      installPath: "/Applications/WorkBuddy.app",
      configPath: "/Users/star/.workbuddy/models.json",
      providerId: "3e495aa8-a81a-4f85-b9a6-14434b8ddda0",
      providerName: "蒙云智算",
      modelId: "GLM-5.2",
      mode: "direct",
      needsRestart: true,
      automaticRestartSupported: true,
    },
    {
      id: "codebuddy",
      displayName: "CodeBuddy",
      installStatus: "installed",
      runtimeStatus: "not_running",
      configHealth: "healthy",
      adapterVerified: true,
      detectedVersion: "4.11.3",
      installPath: "/Applications/CodeBuddy.app",
      configPath: "/Users/star/.codebuddy/models.json",
      needsRestart: true,
      automaticRestartSupported: true,
    },
    {
      id: "qclaw",
      displayName: "QClaw",
      installStatus: "installed",
      runtimeStatus: "not_running",
      configHealth: "healthy",
      adapterVerified: true,
      detectedVersion: "0.2.35",
      installPath: "/Applications/QClaw.app",
      configPath: "/Users/star/.qclaw/openclaw.json",
      needsRestart: true,
      automaticRestartSupported: true,
    },
    {
      id: "autoclaw",
      displayName: "AutoClaw",
      installStatus: "not_installed",
      runtimeStatus: "not_running",
      configHealth: "unsupported_version",
      adapterVerified: false,
      needsRestart: true,
      automaticRestartSupported: true,
    },
    {
      id: "codex",
      displayName: "Codex",
      installStatus: "installed",
      runtimeStatus: "not_running",
      configHealth: "healthy",
      adapterVerified: true,
      detectedVersion: "26.825.51511",
      installPath: "/Applications/Codex.app",
      configPath: "/Users/star/.codex/config.toml",
      needsRestart: true,
      automaticRestartSupported: true,
    },
    {
      id: "ima",
      displayName: "ima",
      installStatus: "installed",
      runtimeStatus: "not_running",
      configHealth: "unsupported_version",
      adapterVerified: false,
      detectedVersion: "147.0.7727.5135",
      needsRestart: false,
      automaticRestartSupported: false,
      message: "ima 自定义模型暂由登录态内部存储管理",
    },
    {
      id: "trae",
      displayName: "TRAE",
      installStatus: "installed",
      runtimeStatus: "not_running",
      configHealth: "unsupported_version",
      adapterVerified: false,
      detectedVersion: "3.5.87",
      needsRestart: false,
      automaticRestartSupported: false,
      message: "TRAE 自定义模型暂由登录态内部存储管理",
    },
  ],
  proxy: {
    status: "stopped",
    host: "127.0.0.1",
    port: 54187,
    activeConnections: 0,
    completedRequests: 0,
    successfulRequests: 0,
    conversionFailures: 0,
    upstreamFailures: 0,
  },
  settings: {
    language: "zh-CN",
    theme: "system",
    startAtLogin: false,
    keepRunningInBackground: false,
  },
};

export function getActiveMockSnapshot(): AppSnapshot {
  if (typeof window !== "undefined") {
    const custom = (window as unknown as { __SCREENSHOT_SNAPSHOT__?: AppSnapshot }).__SCREENSHOT_SNAPSHOT__;
    if (custom) return custom;
    const url = new URLSearchParams(window.location.search);
    if (url.get("real_data") === "true") return realSnapshotData;
  }
  return mockSnapshotTemplate;
}

let mockSnapshot = structuredClone(getActiveMockSnapshot());

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    return invokeMock<T>(command, args);
  }

  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

async function invokeMock<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const isReal = typeof window !== "undefined" && new URLSearchParams(window.location.search).get("real_data") === "true";
  if (!isReal) {
    await new Promise((resolve) => window.setTimeout(resolve, 40));
  }

  switch (command) {
    case "bootstrap":
      return structuredClone(mockSnapshot) as T;
    case "refresh_snapshot":
      return structuredClone(mockSnapshot) as T;
    case "set_agent_install_path": {
      const agentId = args?.agentId as string;
      const path = args?.path as string | undefined;
      const agent = mockSnapshot.agents.find((item) => item.id === agentId);
      if (!agent) throw new Error(`Unknown agent: ${agentId}`);
      agent.customInstallPath = path;
      agent.usingCustomInstallPath = Boolean(path);
      if (path) {
        agent.installPath = /mac/i.test(mockSnapshot.platform)
          ? `${path}/MockAgent.app`
          : `${path}/MockAgent.exe`;
      }
      return structuredClone(agent) as T;
    }
    case "save_provider": {
      const draft = args?.draft as ProviderDraft;
      const matchingProviders = draft.id
        ? []
        : mockSnapshot.providers.filter((provider) =>
            sameProviderIdentity(provider, draft),
          );
      const existing = draft.id
        ? mockSnapshot.providers.find((item) => item.id === draft.id)
        : matchingProviders[0];
      const id = existing?.id ?? draft.id ?? crypto.randomUUID();
      const previousProviders = draft.id
        ? existing
          ? [existing]
          : []
        : matchingProviders;
      const draftModels = draft.id
        ? draft.models
        : mergeModelDrafts(previousProviders, draft.models);
      const connectionChanged = Boolean(
        existing &&
          (existing.kind !== draft.kind ||
            existing.protocol !== draft.protocol ||
            normalizedProviderUrl(existing.baseUrl) !==
              normalizedProviderUrl(draft.baseUrl) ||
            draft.apiKey),
      );
      const models = draftModels.map((model) => {
        const previous = previousProviders
          .flatMap((provider) => provider.models)
          .find((item) => item.modelId === model.modelId);
        const capabilitiesUnchanged =
          previous?.outputModality === model.outputModality &&
          previous?.supportsStreaming === model.supportsStreaming &&
          previous?.supportsTools === model.supportsTools;
        return {
          id: `${id}:${model.modelId}`,
          providerId: id,
          modelId: model.modelId,
          displayName: model.displayName,
          outputModality: model.outputModality,
          supportsStreaming: model.supportsStreaming,
          supportsTools: model.supportsTools,
          source: "custom" as const,
          verificationStatus:
            previous && !connectionChanged && capabilitiesUnchanged
              ? previous.verificationStatus
              : previous && previous.verificationStatus !== "draft_unverified"
                ? ("stale" as const)
                : ("draft_unverified" as const),
        };
      });
      const verificationStatus = aggregateVerificationStatus(models);
      const provider: ProviderSummary = {
        id,
        name: draft.name,
        kind: draft.kind,
        protocol: draft.protocol,
        baseUrl: draft.baseUrl,
        isRecommended: draft.kind === "mongyun",
        isEnabled: true,
        hasApiKey: Boolean(draft.apiKey) || Boolean(existing?.hasApiKey),
        maskedApiKey: draft.apiKey
          ? maskApiKey(draft.apiKey)
          : existing?.maskedApiKey,
        verificationStatus,
        verifiedModelId: models.find(
          (model) =>
            model.outputModality === "text" &&
            model.verificationStatus === "verified",
        )?.modelId,
        defaultModelId: existing?.defaultModelId ?? draft.defaultModelId,
        models,
      };
      if (!draft.id && matchingProviders.length > 1) {
        const duplicateIds = new Set(
          matchingProviders.slice(1).map((provider) => provider.id),
        );
        mockSnapshot.providers = mockSnapshot.providers.filter(
          (provider) => !duplicateIds.has(provider.id),
        );
        duplicateIds.forEach((duplicateId) =>
          mockProviderApiKeys.delete(duplicateId),
        );
      }
      const index = mockSnapshot.providers.findIndex((item) => item.id === id);
      if (draft.apiKey) mockProviderApiKeys.set(id, draft.apiKey);
      if (index >= 0) mockSnapshot.providers[index] = provider;
      else mockSnapshot.providers.push(provider);
      return structuredClone(provider) as T;
    }
    case "delete_provider": {
      const providerId = args?.providerId as string;
      mockSnapshot.providers = mockSnapshot.providers.filter(
        (p) => p.id !== providerId,
      );
      mockProviderApiKeys.delete(providerId);
      mockSnapshot.agents.forEach((agent) => {
        if (agent.providerId === providerId) {
          agent.providerId = undefined;
          agent.providerName = undefined;
          agent.modelId = undefined;
          agent.mode = undefined;
        }
      });
      return undefined as T;
    }
    case "get_provider_api_key_mask": {
      const providerId = args?.providerId as string;
      const apiKey = mockProviderApiKeys.get(providerId);
      if (!apiKey) throw new Error("Mock API key not found");
      return maskApiKey(apiKey) as T;
    }
    case "reveal_provider_api_key": {
      const providerId = args?.providerId as string;
      const apiKey = mockProviderApiKeys.get(providerId);
      if (!apiKey) throw new Error("Mock API key not found");
      return apiKey as T;
    }
    case "test_provider": {
      const providerId = args?.providerId as string;
      const provider = mockSnapshot.providers.find(
        (item) => item.id === providerId,
      );
      if (!provider?.hasApiKey) {
        throw {
          code: "secret_missing",
          message: "请先保存 API Key",
          recovery: "编辑 Provider 并填写 API Key。",
        };
      }
      const modelId = args?.modelId as string | undefined;
      const model = modelId
        ? provider.models.find((item) => item.modelId === modelId)
        : provider.models.find((item) => item.outputModality === "text");
      if (!model) {
        throw {
          code: "model_verification_not_required",
          message: "当前模型供应商没有需要连接测试的文本模型",
        };
      }
      if (model.outputModality !== "text") {
        throw {
          code: "model_verification_not_required",
          message: "生图、语音和视频模型无需连接测试",
        };
      }
      model.verificationStatus = "verified";
      provider.verificationStatus = aggregateVerificationStatus(provider.models);
      provider.verifiedModelId = model.modelId;
      return structuredClone(provider) as T;
    }
    case "start_proxy":
      mockSnapshot.proxy = {
        ...mockSnapshot.proxy,
        status: "running",
        startedAt: new Date().toISOString(),
      };
      return structuredClone(mockSnapshot.proxy) as T;
    case "stop_proxy":
      mockSnapshot.proxy = {
        ...mockSnapshot.proxy,
        status: "stopped",
        startedAt: undefined,
        activeConnections: 0,
      };
      return structuredClone(mockSnapshot.proxy) as T;
    case "update_proxy_port":
      mockSnapshot.proxy.port = args?.port as number;
      return structuredClone(mockSnapshot.proxy) as T;
    case "apply_agent_binding": {
      const draft = args?.draft as AgentBindingDraft;
      const agent = mockSnapshot.agents.find(
        (item) => item.id === draft.agentId,
      );
      const provider = mockSnapshot.providers.find(
        (item) => item.id === draft.providerId,
      );
      if (!agent || !provider) throw new Error("Mock binding target missing");
      agent.providerId = provider.id;
      agent.providerName = provider.name;
      agent.modelId = draft.modelId;
      agent.mode = draft.mode;
      agent.configHealth = "healthy";
      const result = structuredClone(agent);
      if (agent.needsRestart) {
        result.needsRestart = false;
        result.message = `${agent.displayName} 已自动重新打开，新配置已经生效。`;
      }
      return result as T;
    }
    case "restore_agent_native": {
      const agent = mockSnapshot.agents.find(
        (item) => item.id === (args?.agentId as string),
      );
      if (!agent) throw new Error("Mock Agent missing");
      agent.providerId = undefined;
      agent.providerName = undefined;
      agent.modelId = undefined;
      agent.mode = undefined;
      agent.configHealth = "healthy";
      const result = structuredClone(agent);
      if (agent.needsRestart) {
        result.needsRestart = false;
        result.message = `${agent.displayName} 已自动重新打开，新配置已经生效。`;
      }
      return result as T;
    }
    case "update_settings":
      mockSnapshot.settings = {
        ...mockSnapshot.settings,
        ...(args?.settings as Partial<AppSettings>),
      };
      return structuredClone(mockSnapshot.settings) as T;
    default:
      throw new Error(`Mock command not implemented: ${command}`);
  }
}

export const api = {
  resetMock: () => {
    mockSnapshot = structuredClone(mockSnapshotTemplate);
    mockSnapshot.settings.language = "zh-CN";
  },
  bootstrap: () => invoke<AppSnapshot>("bootstrap"),
  refresh: () => invoke<AppSnapshot>("refresh_snapshot"),
  selectAgentInstallDirectory: async (title: string) => {
    if (!isTauri()) return undefined;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selection = await open({
      directory: true,
      multiple: false,
      title,
    });
    return typeof selection === "string" ? selection : undefined;
  },
  setAgentInstallPath: (agentId: string, path?: string) =>
    invoke<AgentSummary>("set_agent_install_path", { agentId, path }),
  saveProvider: (draft: ProviderDraft) =>
    invoke<ProviderSummary>("save_provider", { draft }),
  deleteProvider: (providerId: string) =>
    invoke<void>("delete_provider", { providerId }),
  getProviderApiKeyMask: (providerId: string) =>
    invoke<string>("get_provider_api_key_mask", { providerId }),
  revealProviderApiKey: (providerId: string) =>
    invoke<string>("reveal_provider_api_key", { providerId }),
  testProvider: (providerId: string, modelId?: string) =>
    invoke<ProviderSummary>("test_provider", { providerId, modelId }),
  applyAgentBinding: (draft: AgentBindingDraft) =>
    invoke<AgentSummary>("apply_agent_binding", { draft }),
  restoreAgentNative: (agentId: string) =>
    invoke<AgentSummary>("restore_agent_native", { agentId }),
  startProxy: () => invoke<ProxyStatus>("start_proxy"),
  stopProxy: () => invoke<ProxyStatus>("stop_proxy"),
  updateProxyPort: (port: number) =>
    invoke<ProxyStatus>("update_proxy_port", { port }),
  updateSettings: (settings: Partial<AppSettings>) =>
    invoke<AppSettings>("update_settings", { settings }),
};
