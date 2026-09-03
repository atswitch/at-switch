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
] as const;

export function isSwitchableAgent(agent: AgentSummary): boolean {
  return SWITCHABLE_AGENT_IDS.includes(
    agent.id as (typeof SWITCHABLE_AGENT_IDS)[number],
  );
}

export function supportsDirectBinding(
  _agentId: string,
  _provider: Pick<ProviderSummary, "kind" | "protocol">,
): boolean {
  return true;
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
  if (agentId === "workbuddy" || agentId === "codebuddy") return "OpenAI Chat";
  if (agentId === "codex") return "OpenAI Responses";
  return language === "zh-CN"
    ? "智能体支持的原生协议"
    : "a protocol natively supported by the agent";
}
