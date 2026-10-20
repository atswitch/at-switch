import { describe, expect, it } from "vitest";
import {
  SWITCHABLE_AGENT_IDS,
  providerSupportedProtocols,
  supportsDirectBinding,
  supportsProxyBinding,
} from "./agentCapabilities";
import type { ProviderSummary } from "../types";

type ProtocolProfile = Pick<ProviderSummary, "kind" | "protocol">;

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

  it("supports unified TRAE direct mode for all official custom-model protocols", () => {
    for (const agentId of ["trae"]) {
      expect(supportsDirectBinding(agentId, mongyun)).toBe(true);
      expect(supportsDirectBinding(agentId, {
        kind: "custom",
        protocol: "openai_responses",
      })).toBe(true);
      expect(supportsDirectBinding(agentId, {
        kind: "custom",
        protocol: "anthropic_messages",
      })).toBe(true);
      expect(supportsProxyBinding(agentId)).toBe(false);
    }
  });

  // ZCode 通过个人 Provider 通道接受三种协议，直连与代理都可用，
  // 前端能力声明必须与适配器实际支持的组合一致。
  it("supports ZCode direct and proxy binding for every ZCode protocol", () => {
    for (const protocol of [
      "openai_chat_completions",
      "openai_responses",
      "anthropic_messages",
    ] as const) {
      expect(supportsDirectBinding("zcode", { kind: "custom", protocol })).toBe(true);
    }
    expect(supportsProxyBinding("zcode")).toBe(true);
    expect(SWITCHABLE_AGENT_IDS).toContain("zcode");
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
