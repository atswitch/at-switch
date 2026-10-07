/** 顶层页面只有「模型切换首页」和「设置中心」两级。智能体、模型供应商等配置
 * 全部收纳进设置中心的分类，不再作为独立顶层页面。 */
export type PageId = "overview" | "settings";

/** 设置中心的分类导航项。 */
export type SettingsTab =
  | "agents"
  | "providers"
  | "general"
  | "proxy"
  | "about";

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
  /**
   * 用户在「本地代理」页是否打开了该 Agent 的"用量统计"开关。
   * 与 binding/mode 解耦——开启只表示该 Agent 的流量经过本地代理并被统计。
   */
  proxyPrefEnabled?: boolean;
  needsRestart: boolean;
  automaticRestartSupported: boolean;
  activationRequired?: boolean;
  requiresAccountConnection?: boolean;
  message?: string;
  /**
   * 当 Agent 无法被 AT-Switch 自动恢复到出厂默认模型（用户从未被接管过，磁盘
   * 上没有 baseline）时，由后端给出的人工恢复步骤，前端在弹窗里逐条展示给用户。
   * 正常情况为 undefined。
   */
  manualRecoverySteps?: ManualRecoveryStep[];
}

export interface ManualRecoveryStep {
  title: string;
  detail: string;
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
  /** 最近的请求摘要，新的在前；只在内存中保留，代理重启后清空。 */
  recentRequests: ProxyRequestLogEntry[];
  /** 当前已被代理路由的 Agent ID 列表（开关已开启 + 存在有效 binding）。 */
  proxiedAgents?: string[];
  error?: string;
}

/** 一条请求摘要，仅含展示字段——不含请求体、响应体或凭据。 */
export interface ProxyRequestLogEntry {
  at: string;
  agentId: string;
  providerId: string;
  providerName: string;
  model: string;
  /** 上游 HTTP 状态码；连不上上游时为 502。 */
  status: number;
  inputTokens?: number;
  outputTokens?: number;
  /** 上游命中 Prompt Cache 的 token 数；未返回或协议不支持时缺省。 */
  cacheReadTokens?: number;
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
  /** 全量使用日志（代理请求 + 切换操作），新的在前。 */
  usageLog: UsageLogEntry[];
  settings: AppSettings;
}

/** GitHub Releases 返回的最新可用版本。 */
export interface ReleaseInfo {
  tagName: string;
  version: string;
  htmlUrl: string;
  publishedAt: string;
  bodyPreview?: string;
}

/** `request` 走代理的真实请求；`switch` 切换操作（直连模式下唯一可观察的事件）。 */
export type UsageKind = "request" | "switch";

export type UsageOutcome = "ok" | "failed";

export interface UsageLogEntry {
  kind: UsageKind;
  at: string;
  agentId: string;
  providerId: string;
  providerName: string;
  model: string;
  /** 上游 HTTP 状态码；切换类记录没有。 */
  status?: number;
  outcome: UsageOutcome;
  inputTokens?: number;
  outputTokens?: number;
  errorCode?: string;
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
