export type PageId = "overview" | "agents" | "providers" | "proxy" | "settings";

export type AppLanguage = "zh-CN" | "en";

export type ApiProtocol =
  | "openai_chat_completions"
  | "openai_responses"
  | "anthropic_messages";

export type ProviderKind =
  | "mongyun"
  | "deepseek"
  | "minimax"
  | "kimi"
  | "zhipu"
  | "qwen"
  | "doubao"
  | "custom";

export type VerificationStatus =
  | "draft_unverified"
  | "verifying"
  | "verified"
  | "stale"
  | "failed";

export type ModelOutputModality = "text" | "image" | "audio" | "video";

export interface ModelSummary {
  id: string;
  providerId: string;
  modelId: string;
  displayName: string;
  outputModality: ModelOutputModality;
  supportsStreaming: boolean;
  supportsTools: boolean;
  source: "builtin" | "remote" | "custom";
  verificationStatus: VerificationStatus;
}

export interface ProviderSummary {
  id: string;
  name: string;
  kind: ProviderKind;
  protocol: ApiProtocol;
  baseUrl: string;
  isRecommended: boolean;
  isEnabled: boolean;
  hasApiKey: boolean;
  maskedApiKey?: string;
  verificationStatus: VerificationStatus;
  verifiedModelId?: string;
  defaultModelId?: string;
  models: ModelSummary[];
}

export type AgentInstallStatus =
  | "not_installed"
  | "installed_uninitialized"
  | "installed";

export type AgentRuntimeStatus = "running" | "not_running" | "unknown";

export type AgentConfigHealth =
  | "healthy"
  | "unreadable"
  | "unparseable"
  | "unwritable"
  | "unsupported_version"
  | "external_changed"
  | "takeover_interrupted"
  | "manual_recovery_required";

export interface AgentSummary {
  id: string;
  displayName: string;
  installStatus: AgentInstallStatus;
  runtimeStatus: AgentRuntimeStatus;
  configHealth: AgentConfigHealth;
  adapterVerified: boolean;
  detectedVersion?: string;
  isLatestVersion?: boolean;
  installPath?: string;
  customInstallPath?: string;
  usingCustomInstallPath?: boolean;
  configPath?: string;
  providerName?: string;
  providerId?: string;
  modelId?: string;
  mode?: "direct" | "proxy";
  needsRestart: boolean;
  automaticRestartSupported: boolean;
  activationRequired?: boolean;
  message?: string;
}

export interface AgentBindingDraft {
  agentId: string;
  providerId: string;
  modelId: string;
  mode: "direct" | "proxy";
}

export type ProxyRuntimeStatus =
  | "stopped"
  | "starting"
  | "running"
  | "draining"
  | "error";

export interface ProxyStatus {
  status: ProxyRuntimeStatus;
  host: string;
  port: number;
  startedAt?: string;
  activeConnections: number;
  completedRequests: number;
  successfulRequests: number;
  conversionFailures: number;
  upstreamFailures: number;
  error?: string;
}

export interface AppSettings {
  language: AppLanguage;
  theme: "system" | "light" | "dark";
  startAtLogin: boolean;
  keepRunningInBackground: boolean;
}

export interface AppSnapshot {
  appVersion: string;
  platform: string;
  providers: ProviderSummary[];
  agents: AgentSummary[];
  proxy: ProxyStatus;
  settings: AppSettings;
}

export interface ProviderDraft {
  id?: string;
  name: string;
  kind: ProviderKind;
  protocol: ApiProtocol;
  baseUrl: string;
  apiKey?: string;
  allowInsecureHttp?: boolean;
  defaultModelId?: string;
  models: Array<{
    modelId: string;
    displayName: string;
    outputModality: ModelOutputModality;
    supportsStreaming: boolean;
    supportsTools: boolean;
  }>;
}

export interface CommandError {
  code: string;
  message: string;
  recovery?: string;
}
