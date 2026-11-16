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

export interface RemoteModelSummary {
  id: string;
  ownedBy?: string;
  contextWindow?: number;
  outputModality: ModelOutputModality;
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
  requiresAccountConnection?: boolean;
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

export type LoginMethod = "wechat" | "google";

export interface AccountProfile {
  userId: string;
  username: string;
  displayName: string;
  email: string;
  providerType: LoginMethod | string;
}

export interface AccountStatus {
  /** 服务端地址是否已在打包期注入；false 时会员入口显示「会员服务未配置」。 */
  configured: boolean;
  loggedIn: boolean;
  /** 校验网络失败时保持登录态并标记离线（不影响本地功能）。 */
  offline: boolean;
  frozen?: boolean;
  profile?: AccountProfile;
}

export interface LoginStart {
  method: LoginMethod;
  loginState: string;
  qrUrl?: string;
}

export interface LoginPoll {
  status: "pending" | "success" | "expired" | "error";
  profile?: AccountProfile;
  errorCode?: string;
  message?: string;
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
