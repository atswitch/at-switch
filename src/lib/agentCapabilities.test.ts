import { describe, expect, it } from "vitest";
import {
  directBindingRequirement,
  isDetectionOnlyAgent,
  isSwitchableAgent,
  providerSupportedProtocols,
  supportsDirectBinding,
} from "./agentCapabilities";
import type { AgentSummary, ProviderSummary } from "../types";

type ProtocolProfile = Pick<ProviderSummary, "kind" | "protocol">;

function agent(id: string): AgentSummary {
  return {
    id,
    displayName: id,
    installStatus: "installed",
    runtimeStatus: "not_running",
    configHealth: "healthy",
    adapterVerified: true,
    needsRestart: false,
    automaticRestartSupported: false,
  };
}

describe("Agent protocol capabilities", () => {
  const mongyun: ProtocolProfile = {
    kind: "mongyun",
    protocol: "openai_chat_completions",
  };

  it("recognizes every protocol exposed by a multi-protocol Provider", () => {
    expect(providerSupportedProtocols(mongyun)).toEqual([
      "openai_chat_completions",
      "openai_responses",
    ]);
    expect(supportsDirectBinding("workbuddy", mongyun)).toBe(true);
    expect(supportsDirectBinding("codebuddy", mongyun)).toBe(true);
    expect(supportsDirectBinding("codex", mongyun)).toBe(true);
  });

  it("preserves direct binding behavior for existing agents", () => {
    expect(
      supportsDirectBinding("codex", {
        kind: "custom",
        protocol: "openai_chat_completions",
      }),
    ).toBe(true);
    expect(
      supportsDirectBinding("codebuddy", {
        kind: "custom",
        protocol: "openai_responses",
      }),
    ).toBe(true);
  });

  it("limits DuMate direct binding to OpenAI Chat providers", () => {
    expect(
      supportsDirectBinding("dumate", {
        kind: "custom",
        protocol: "openai_chat_completions",
      }),
    ).toBe(true);
    expect(
      supportsDirectBinding("dumate", {
        kind: "custom",
        protocol: "openai_responses",
      }),
    ).toBe(false);
    expect(supportsDirectBinding("dumate", mongyun)).toBe(true);
  });

  it("keeps Hermes direct bindings on OpenAI Chat providers", () => {
    expect(
      supportsDirectBinding("hermes", {
        kind: "custom",
        protocol: "openai_chat_completions",
      }),
    ).toBe(true);
    expect(
      supportsDirectBinding("hermes", {
        kind: "custom",
        protocol: "openai_responses",
      }),
    ).toBe(false);
    expect(supportsDirectBinding("hermes", mongyun)).toBe(true);
    expect(directBindingRequirement("hermes")).toBe("OpenAI Chat");
  });

  it("treats ZCode as switchable after the personal config channel landed", () => {
    expect(isSwitchableAgent(agent("zcode"))).toBe(true);
    expect(isDetectionOnlyAgent("zcode")).toBe(false);
  });

  it("treats AionClaw as switchable through its embedded OpenClaw config", () => {
    expect(isSwitchableAgent(agent("aionclaw"))).toBe(true);
    expect(isDetectionOnlyAgent("aionclaw")).toBe(false);
  });

  it("treats EasyClaw as switchable through its OpenClaw-format config", () => {
    expect(isSwitchableAgent(agent("easyclaw"))).toBe(true);
    expect(isDetectionOnlyAgent("easyclaw")).toBe(false);
  });

  it("treats ima as read-only because it exposes no provider configuration", () => {
    expect(isDetectionOnlyAgent("ima")).toBe(true);
    expect(isSwitchableAgent(agent("ima"))).toBe(false);
  });

  // Daimon 每次启动都会用服务端下发的默认模型覆盖 `model.current`，连官方模型名也一
  // 样被重置，所以本地写入无法生效——即便运行态 TOML 里确实出现了我们的 Provider。
  it("treats Kimi Work as read-only because Daimon rebuilds the selected model on startup", () => {
    expect(isDetectionOnlyAgent("kimiwork")).toBe(true);
    expect(isSwitchableAgent(agent("kimiwork"))).toBe(false);
  });

  it("keeps the cloud-first clients read-only because their models are server-authorized", () => {
    for (const id of ["qwenwork", "doubaowork", "coze"]) {
      expect(isDetectionOnlyAgent(id)).toBe(true);
      expect(isSwitchableAgent(agent(id))).toBe(false);
    }
  });

  it("treats both Trae apps as switchable between models configured inside them", () => {
    for (const id of ["traework", "traecode"]) {
      expect(isSwitchableAgent(agent(id))).toBe(true);
      expect(isDetectionOnlyAgent(id)).toBe(false);
    }
  });

  it("allows OpenClaw-based Agents to use configured protocols directly", () => {
    const anthropic: ProtocolProfile = {
      kind: "custom",
      protocol: "anthropic_messages",
    };
    expect(supportsDirectBinding("qclaw", anthropic)).toBe(true);
    expect(supportsDirectBinding("autoclaw", anthropic)).toBe(true);
  });
});
