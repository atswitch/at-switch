import { describe, expect, it } from "vitest";
import {
  agentAvailabilityLabel,
  providerProtocolLabel,
  successRate,
} from "./format";

describe("successRate", () => {
  it("uses the PRD definition and returns a rounded percentage", () => {
    expect(successRate(7, 10)).toBe("70%");
  });

  it("does not invent a rate before a request completes", () => {
    expect(successRate(0, 0)).toBe("—");
  });
});

describe("providerProtocolLabel", () => {
  it("shows both native Mongyun OpenAI protocols", () => {
    expect(
      providerProtocolLabel({
        kind: "mongyun",
        protocol: "openai_chat_completions",
      }),
    ).toBe("OpenAI Chat + OpenAI Responses");
  });

  it("keeps a configured Anthropic capability extensible", () => {
    expect(
      providerProtocolLabel({
        kind: "mongyun",
        protocol: "anthropic_messages",
      }),
    ).toBe("OpenAI Chat + OpenAI Responses + Anthropic Messages");
  });
});

describe("agentAvailabilityLabel", () => {
  it("distinguishes an installed but closed desktop Agent from an absent one", () => {
    expect(agentAvailabilityLabel("installed", "not_running")).toBe(
      "已安装未开启",
    );
    expect(agentAvailabilityLabel("not_installed", "unknown")).toBe("未安装");
  });
});
