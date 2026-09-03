import { describe, expect, it } from "vitest";
import {
  providerSupportedProtocols,
  supportsDirectBinding,
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

  it("allows direct binding for all providers and agents", () => {
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

  it("allows OpenClaw-based Agents to use configured protocols directly", () => {
    const anthropic: ProtocolProfile = {
      kind: "custom",
      protocol: "anthropic_messages",
    };
    expect(supportsDirectBinding("qclaw", anthropic)).toBe(true);
    expect(supportsDirectBinding("autoclaw", anthropic)).toBe(true);
  });
});
