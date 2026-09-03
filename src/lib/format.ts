import type {
  AgentConfigHealth,
  AgentInstallStatus,
  AgentRuntimeStatus,
  AppLanguage,
  ApiProtocol,
  ProviderSummary,
  VerificationStatus,
} from "../types";

export const protocolLabels: Record<ApiProtocol, string> = {
  openai_chat_completions: "OpenAI Chat",
  openai_responses: "OpenAI Responses",
  anthropic_messages: "Anthropic Messages",
};

export function providerProtocolLabel(
  provider: Pick<ProviderSummary, "kind" | "protocol">,
): string {
  const protocols: ApiProtocol[] =
    provider.kind === "mongyun"
      ? ["openai_chat_completions", "openai_responses"]
      : [provider.protocol];
  if (!protocols.includes(provider.protocol)) {
    protocols.push(provider.protocol);
  }
  return protocols.map((protocol) => protocolLabels[protocol]).join(" + ");
}

export const verificationLabels: Record<VerificationStatus, string> = {
  draft_unverified: "未验证",
  verifying: "验证中",
  verified: "已验证",
  stale: "验证已失效",
  failed: "验证失败",
};

const verificationLabelsEn: Record<VerificationStatus, string> = {
  draft_unverified: "Unverified",
  verifying: "Verifying",
  verified: "Verified",
  stale: "Verification expired",
  failed: "Verification failed",
};

export function verificationLabel(
  status: VerificationStatus,
  language: AppLanguage = "zh-CN",
): string {
  return language === "zh-CN"
    ? verificationLabels[status]
    : verificationLabelsEn[status];
}

export const agentInstallLabels: Record<AgentInstallStatus, string> = {
  not_installed: "未安装",
  installed_uninitialized: "尚未初始化",
  installed: "已安装",
};

export function agentAvailabilityLabel(
  installStatus: AgentInstallStatus,
  runtimeStatus: AgentRuntimeStatus,
  language: AppLanguage = "zh-CN",
): string {
  if (language === "en") {
    if (installStatus === "not_installed") return "Not installed";
    if (runtimeStatus === "running") return "Installed and running";
    if (runtimeStatus === "not_running") return "Installed, not running";
    return installStatus === "installed_uninitialized"
      ? "Not initialized"
      : "Installed";
  }
  if (installStatus === "not_installed") return "未安装";
  if (runtimeStatus === "running") return "已安装已开启";
  if (runtimeStatus === "not_running") return "已安装未开启";
  return agentInstallLabels[installStatus];
}

export const agentHealthLabels: Record<AgentConfigHealth, string> = {
  healthy: "配置正常",
  unreadable: "无法读取",
  unparseable: "无法解析",
  unwritable: "权限不足",
  unsupported_version: "适配待验证",
  external_changed: "外部配置已变化",
  takeover_interrupted: "代理接管中断",
  manual_recovery_required: "需要人工恢复",
};

const agentHealthLabelsEn: Record<AgentConfigHealth, string> = {
  healthy: "Configuration healthy",
  unreadable: "Unreadable",
  unparseable: "Cannot parse",
  unwritable: "Insufficient permissions",
  unsupported_version: "Adapter validation required",
  external_changed: "External changes detected",
  takeover_interrupted: "Proxy takeover interrupted",
  manual_recovery_required: "Manual recovery required",
};

export function agentHealthLabel(
  health: AgentConfigHealth,
  language: AppLanguage = "zh-CN",
): string {
  return language === "zh-CN"
    ? agentHealthLabels[health]
    : agentHealthLabelsEn[health];
}

export function formatDuration(startedAt?: string): string {
  if (!startedAt) return "—";
  const seconds = Math.max(
    0,
    Math.floor((Date.now() - new Date(startedAt).getTime()) / 1000),
  );
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

export function successRate(successful: number, completed: number): string {
  if (completed === 0) return "—";
  return `${Math.round((successful / completed) * 100)}%`;
}
