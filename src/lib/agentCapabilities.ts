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
  "dsh",
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
  // ima keeps its authoritative model settings on the account side, so writes
  // only target the current logged-in account.
  "ima",
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
// - Accio (Alibaba's e-commerce AI agent) is server-locked: its model list is
//   served as opaque server codes and every AI request goes through its own
//   `phoenix-gw.alibaba.com` gateway, with no local `baseUrl` / `apiKey` /
//   custom-provider surface.
export function isDetectionOnlyAgent(
  agentId: string,
): agentId is "qwenwork" | "doubaowork" | "coze" | "kimiwork" | "accio" {
  return (
    agentId === "qwenwork" ||
    agentId === "doubaowork" ||
    agentId === "coze" ||
    agentId === "kimiwork" ||
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
  provider: Pick<ProviderSummary, "kind" | "protocol"> &
    Partial<Pick<ProviderSummary, "baseUrl">>,
): boolean {
  if (usesCloudModelSettings(agentId)) {
    return (
      providerSupportedProtocols(provider).includes("openai_chat_completions") &&
      hasPublicEndpoint(provider.baseUrl)
    );
  }
  if (
    agentId === "dumate" ||
    agentId === "hermes" ||
    agentId === "opencode" ||
    agentId === "dsh"
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

export function usesCloudModelSettings(agentId: string): boolean {
  return agentId === "ima";
}

export function supportsProxyBinding(agentId: string): boolean {
  return !usesCloudModelSettings(agentId);
}

// This is a UI capability check; the native service performs authoritative URL
// validation before sending any credentials to the target agent's account.
function hasPublicEndpoint(value?: string): boolean {
  if (!value) return false;
  try {
    const url = new URL(value);
    if (!["https:", "http:"].includes(url.protocol)) return false;
    if (url.username || url.password) return false;
    const hostname = url.hostname.toLowerCase().replace(/\.$/, "");
    if (hostname.startsWith("[")) {
      const ipv6 = hostname.slice(1, -1);
      const mapped = /^::ffff:([0-9a-f]{1,4}):([0-9a-f]{1,4})$/.exec(ipv6);
      if (mapped?.[1] && mapped[2]) {
        const high = parseInt(mapped[1], 16);
        const low = parseInt(mapped[2], 16);
        return isPublicIpv4([high >> 8, high & 255, low >> 8, low & 255]);
      }
      return !(
        ipv6 === "::" || ipv6 === "::1" ||
        /^f[cd]/.test(ipv6) || /^fe[89ab]/.test(ipv6) || /^ff/.test(ipv6)
      );
    }
    if (
      !hostname.includes(".") ||
      hostname.endsWith(".localhost") ||
      hostname.endsWith(".local") ||
      hostname.endsWith(".internal") ||
      hostname.endsWith(".lan") ||
      hostname.endsWith(".home")
    ) return false;
    const ipv4 = hostname.split(".").map(Number);
    if (ipv4.length === 4 && ipv4.every(Number.isInteger)) {
      return isPublicIpv4(ipv4);
    }
    return true;
  } catch {
    return false;
  }
}

function isPublicIpv4(octets: number[]): boolean {
  const [first, second] = octets;
  if (first === undefined || second === undefined) return false;
  return !(
    first === 0 || first === 10 || first === 127 || first >= 224 ||
    (first === 100 && second >= 64 && second <= 127) ||
    (first === 169 && second === 254) ||
    (first === 172 && second >= 16 && second <= 31) ||
    (first === 192 && second === 168)
  );
}

export function directBindingUnavailableReason(
  agentId: string,
  language: AppLanguage = "zh-CN",
): string {
  if (usesCloudModelSettings(agentId)) {
    return language === "zh-CN"
      ? "ima 需要公网可访问的 OpenAI Chat 接口，无法使用本机或局域网地址。"
      : "ima requires a public OpenAI Chat endpoint. Local and private-network addresses are unavailable.";
  }
  return language === "zh-CN"
    ? `该模型供应商未提供 ${directBindingRequirement(agentId, language)}；如需协议转换，请前往高级设置使用本地代理`
    : `This provider does not offer ${directBindingRequirement(agentId, language)}. Use the local proxy in Advanced settings for protocol conversion.`;
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
  if (usesCloudModelSettings(agentId)) {
    return language === "zh-CN" ? "公网 OpenAI Chat 接口" : "a public OpenAI Chat endpoint";
  }
  if (
    agentId === "workbuddy" ||
    agentId === "codebuddy" ||
    agentId === "dumate" ||
    agentId === "hermes" ||
    agentId === "opencode" ||
    agentId === "dsh"
  )
    return "OpenAI Chat";
  if (agentId === "codex") return "OpenAI Responses";
  return language === "zh-CN"
    ? "智能体支持的原生协议"
    : "a protocol natively supported by the agent";
}
