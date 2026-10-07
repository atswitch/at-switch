import { describe, expect, it } from "vitest";
import {
  directBindingRequirement,
  isDetectionOnlyAgent,
  isProxyRoutedModel,
  isProxyRoutedProvider,
  isSwitchableAgent,
  providerSupportedProtocols,
  supportsDirectBinding,
  supportsProxyBinding,
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

function proxyAgent(id: string, providerId: string, modelId: string): AgentSummary {
  return {
    ...agent(id),
    mode: "proxy",
    providerId,
    modelId,
    proxyPrefEnabled: true,
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

  // ima 的权威模型设置在账号侧：AT-Switch 读取登录态后直连写入公网 OpenAI Chat
  // 接口，所以它可切换，但没有本地 Provider 配置面。
  it("treats ima as switchable through its account-side cloud model settings", () => {
    expect(isDetectionOnlyAgent("ima")).toBe(false);
    expect(isSwitchableAgent(agent("ima"))).toBe(true);
  });

  it("treats Accio as read-only because its models are server-locked behind its own gateway", () => {
    expect(isDetectionOnlyAgent("accio")).toBe(true);
    expect(isSwitchableAgent(agent("accio"))).toBe(false);
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

  it("supports ima through public OpenAI Chat endpoints across providers", () => {
    expect(supportsDirectBinding("ima", { ...mongyun, baseUrl: "https://api.example.test/v1" })).toBe(true);
    expect(supportsDirectBinding("ima", { ...mongyun, baseUrl: "https://[2606:4700::1111]/v1" })).toBe(true);
    expect(supportsDirectBinding("ima", { kind: "custom", protocol: "openai_chat_completions", baseUrl: "https://other.example.test/v1/chat/completions" })).toBe(true);
    expect(supportsDirectBinding("ima", { kind: "custom", protocol: "openai_responses", baseUrl: "https://api.example.test/v1" })).toBe(false);
    expect(supportsProxyBinding("ima")).toBe(false);
    expect(supportsProxyBinding("dumate")).toBe(true);
  });

  it.each([
    "http://localhost:1234/v1", "http://127.0.0.1:1234/v1",
    "http://127.1:1234/v1", "http://192.168.1.2/v1", "http://10.1.2.3/v1",
    "http://172.16.2.3/v1", "http://169.254.1.2/v1", "http://100.64.1.2/v1",
    "http://[::1]:1234/v1", "http://[::ffff:127.0.0.1]/v1",
    "http://[fc00::1]/v1", "http://[fe80::1]/v1", "http://[ff00::1]/v1",
    "http://model.lan/v1", "http://model.home/v1",
    "http://model.local/v1", "http://model.internal/v1", "http://model.localhost/v1",
    "file:///local/model", "not-a-url",
  ])("disables ima for cloud-inaccessible endpoint %s", (baseUrl) => {
    expect(supportsDirectBinding("ima", { ...mongyun, baseUrl })).toBe(false);
  });
});

describe("Proxy routing badge", () => {
  const routedAgent = proxyAgent("workbuddy", "provider-1", "model-a");

  it("marks the bound provider as proxy routed while the proxy runs", () => {
    expect(isProxyRoutedProvider(routedAgent, "provider-1", true)).toBe(true);
    expect(
      isProxyRoutedModel(routedAgent, "provider-1", "model-a", true),
    ).toBe(true);
  });

  it("hides the proxy routing badge when the proxy is stopped", () => {
    // 回归：总开关关闭后代理未运行，偏好开关仍为 true，标记必须消失。
    expect(isProxyRoutedProvider(routedAgent, "provider-1", false)).toBe(false);
    expect(
      isProxyRoutedModel(routedAgent, "provider-1", "model-a", false),
    ).toBe(false);
  });

  it("keeps the badge hidden when the proxy preference is off", () => {
    const directAgent = agent("workbuddy");
    expect(isProxyRoutedProvider(directAgent, "provider-1", true)).toBe(false);
    expect(
      isProxyRoutedModel(directAgent, "provider-1", "model-a", true),
    ).toBe(false);
  });

  it("only marks the provider the agent is actually bound to", () => {
    expect(isProxyRoutedProvider(routedAgent, "provider-2", true)).toBe(false);
    expect(
      isProxyRoutedModel(routedAgent, "provider-1", "model-b", true),
    ).toBe(false);
  });
});
