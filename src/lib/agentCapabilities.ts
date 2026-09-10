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
  "ima",
] as const;

export function isSwitchableAgent(agent: AgentSummary): boolean {
  return SWITCHABLE_AGENT_IDS.includes(
    agent.id as (typeof SWITCHABLE_AGENT_IDS)[number],
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
  if (agentId === "dumate") {
    return providerSupportedProtocols(provider).includes("openai_chat_completions");
  }
  return true;
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
    agentId === "dumate"
  )
    return "OpenAI Chat";
  if (agentId === "codex") return "OpenAI Responses";
  return language === "zh-CN"
    ? "智能体支持的原生协议"
    : "a protocol natively supported by the agent";
}
