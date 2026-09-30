import type {
  AppLanguage,
  AgentSummary,
  ApiProtocol,
  ProviderSummary,
} from "../types";

export const SWITCHABLE_AGENT_IDS = [
  "workbuddy",
  "codebuddy",
  "qclaw",
  "autoclaw",
  "codex",
  "dumate",
  "hermes",
  "opencode",
  "zcode",
  // AionClaw embeds the OpenClaw runtime inside a macOS sandbox container, so it
  // uses the same `models.providers` / `agents.defaults.model` shape as QClaw.
  "aionclaw",
  // Neither Trae app can receive credentials (its `ak` is encrypted), but
  // AT-Switch can switch between the models the user configured inside Trae
  // itself. Both keep the active model as plain JSON in
  // `User/globalStorage/state.vscdb`, under the same key names and id shape.
  "traework",
  // TRAE SOLO CN only qualifies from 0.1.69 on: 0.1.66 kept the active model in
  // an encrypted database (ModularData/ai-agent/database.db).
  "traecode",
  // EasyClaw ships the OpenClaw runtime (gateway.asar/openclaw.mjs) and keeps its
  // authoritative config at `~/.easyclaw/easyclaw.json`, so it reuses the same
  // `models.providers` / `agents.defaults.model.primary` write kernel as QClaw.
  "easyclaw",
] as const;

export type SwitchableAgentId = (typeof SWITCHABLE_AGENT_IDS)[number];

// Cloud-first clients with no user-level provider configuration.
//
// - Coze and Doubao Work hold nothing locally but Chromium state and network
//   settings.
// - Qwen Work *does* have a BYOK table in a plain SQLite database, but a custom
//   model is gated twice: the account's `allowBYOK` (client side) and the model
//   gateway's per-model authorization (server side). Unlocking only the client
//   gets as far as a selectable model that still fails with HTTP 403
//   ("You do not have access to this model service"), so nothing is written.
// - Kimi Work *does* accept a custom provider — the generated runtime TOML really
//   contains it — but Daimon rewrites `model.current` on every startup with the
//   server-served default, and only then derives `default_model` from it. Writing
//   an official model name there is reset too, so no local write can take effect.
// - ima has no user-level provider surface at all: its data directory is plain
//   Chromium state, and the bundle contains zero occurrences of `baseUrl`,
//   `apiKey`, `customModel` or `modelProvider`.
// - Accio (Alibaba's e-commerce AI agent) is server-locked: its model list is
//   served as opaque server codes and every AI request goes through its own
//   `phoenix-gw.alibaba.com` gateway, with no local `baseUrl` / `apiKey` /
//   custom-provider surface.
export function isDetectionOnlyAgent(
  agentId: string,
): agentId is "qwenwork" | "doubaowork" | "coze" | "kimiwork" | "ima" | "accio" {
  return (
    agentId === "qwenwork" ||
    agentId === "doubaowork" ||
    agentId === "coze" ||
    agentId === "kimiwork" ||
    agentId === "ima" ||
    agentId === "accio"
  );
}

export function isSwitchableAgent(agent: AgentSummary): boolean {
  return SWITCHABLE_AGENT_IDS.includes(
    agent.id as SwitchableAgentId,
  );
}

export function supportsDirectBinding(
  agentId: string,
  provider: Pick<ProviderSummary, "kind" | "protocol">,
): boolean {
  if (
    agentId === "dumate" ||
    agentId === "hermes" ||
    agentId === "opencode"
  ) {
    return providerSupportedProtocols(provider).includes("openai_chat_completions");
  }
  return true;
}

/**
 * 该 Agent 是否正通过本地代理使用某个供应商。代理接管意味着退出 AT-Switch 后
 * 该 Agent 无法继续请求，因此首页必须显式标识出来。标记仅在代理实际运行时显示，
 * 总开关关闭（代理未运行）时不显示，避免"代理停了但标记还在"的误导。
 *
 * 语义由 `proxyPrefEnabled`（用户偏好）和代理运行状态共同决定：
 * - `proxyPrefEnabled === true` 且代理在运行：显示"代理接管"标记
 * - `proxyPrefEnabled === true` 但代理已停止：不显示标记（代理未实际接管流量）
 * - `proxyPrefEnabled === false`：不显示标记（与 `binding.mode` 解耦，开关关闭后
 *   即便 `binding.mode === "proxy"`，代理也不会接管流量）
 */
export function isProxyRoutedProvider(
  agent: AgentSummary,
  providerId: string,
  proxyRunning: boolean,
): boolean {
  return (
    Boolean(agent.proxyPrefEnabled) &&
    agent.providerId === providerId &&
    proxyRunning
  );
}

/** 代理接管下正在使用的具体模型。 */
export function isProxyRoutedModel(
  agent: AgentSummary,
  providerId: string,
  modelId: string,
  proxyRunning: boolean,
): boolean {
  return (
    isProxyRoutedProvider(agent, providerId, proxyRunning) &&
    agent.modelId === modelId
  );
}

export function providerSupportedProtocols(
  provider: Pick<ProviderSummary, "kind" | "protocol">,
): ApiProtocol[] {
  if (provider.kind === "mongyun") {
    return ["openai_chat_completions", "openai_responses"];
  }
  return [provider.protocol];
}

export function directBindingRequirement(
  agentId: string,
  language: AppLanguage = "zh-CN",
): string {
  if (
    agentId === "workbuddy" ||
    agentId === "codebuddy" ||
    agentId === "dumate" ||
    agentId === "hermes" ||
    agentId === "opencode"
  )
    return "OpenAI Chat";
  if (agentId === "codex") return "OpenAI Responses";
  return language === "zh-CN"
    ? "智能体支持的原生协议"
    : "a protocol natively supported by the agent";
}
