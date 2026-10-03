import { describe, expect, it } from "vitest";
import type { AgentSummary, ProxyRequestLogEntry } from "../types";
import {
  buildAgentUsage,
  formatTokens,
  groupAgentsForRouting,
  isAgentRoutable,
} from "./proxyUsage";

function agent(
  id: string,
  overrides: Partial<AgentSummary> = {},
): AgentSummary {
  return {
    id,
    displayName: id,
    installStatus: "installed",
    runtimeStatus: "not_running",
    configHealth: "healthy",
    adapterVerified: true,
    needsRestart: false,
    automaticRestartSupported: false,
    ...overrides,
  };
}

function request(
  overrides: Partial<ProxyRequestLogEntry> = {},
): ProxyRequestLogEntry {
  return {
    at: "2026-01-01T00:00:00.000Z",
    agentId: "workbuddy",
    providerId: "mongyun",
    providerName: "蒙云智算",
    model: "glm-5.2",
    status: 200,
    inputTokens: 100,
    outputTokens: 50,
    ...overrides,
  };
}

describe("groupAgentsForRouting", () => {
  it("splits agents into routed, idle and unavailable groups", () => {
    const groups = groupAgentsForRouting([
      agent("a", { proxyPrefEnabled: true }),
      agent("b"),
      agent("c", { installStatus: "not_installed" }),
      agent("d", { adapterVerified: false }),
    ]);

    expect(groups.map((group) => group.id)).toEqual([
      "routed",
      "idle",
      "unavailable",
    ]);
    expect(groups[0]?.agents.map((item) => item.id)).toEqual(["a"]);
    expect(groups[2]?.agents.map((item) => item.id)).toEqual(["c", "d"]);
  });

  it("omits empty groups", () => {
    const groups = groupAgentsForRouting([agent("a")]);
    expect(groups).toHaveLength(1);
    expect(groups[0]?.id).toBe("idle");
  });

  it("treats installed and verified agents as routable only", () => {
    expect(isAgentRoutable(agent("a"))).toBe(true);
    expect(isAgentRoutable(agent("a", { installStatus: "not_installed" }))).toBe(
      false,
    );
    expect(isAgentRoutable(agent("a", { adapterVerified: false }))).toBe(false);
  });
});

describe("buildAgentUsage", () => {
  it("counts only agents that are currently routed", () => {
    const rows = buildAgentUsage(
      [agent("workbuddy"), agent("codebuddy")],
      ["workbuddy"],
      [request(), request({ agentId: "codebuddy" })],
    );

    expect(rows).toHaveLength(1);
    expect(rows[0]?.requests).toBe(1);
  });

  it("keeps unknown-token successes out of the success rate", () => {
    const rows = buildAgentUsage(
      [agent("workbuddy")],
      ["workbuddy"],
      [
        request({ inputTokens: 100, outputTokens: 50 }),
        request({ inputTokens: undefined, outputTokens: undefined }),
      ],
    );

    expect(rows[0]?.requests).toBe(2);
    expect(rows[0]?.unknownRequests).toBe(1);
    expect(rows[0]?.successRate).toBe(100);
  });

  it("reports a zero success rate when every success lacks usage", () => {
    const rows = buildAgentUsage(
      [agent("workbuddy")],
      ["workbuddy"],
      [request({ inputTokens: undefined, outputTokens: undefined })],
    );

    expect(rows[0]?.successRate).toBe(0);
  });

  it("breaks usage down per model and scales bars against the largest agent", () => {
    const rows = buildAgentUsage(
      [agent("workbuddy"), agent("codebuddy")],
      ["workbuddy", "codebuddy"],
      [
        request({ inputTokens: 100, outputTokens: 50 }),
        request({ model: "glm-5.1", inputTokens: 300, outputTokens: 100 }),
        request({ agentId: "codebuddy", inputTokens: 200, outputTokens: 100 }),
      ],
    );

    expect(rows[0]?.agentId).toBe("workbuddy");
    expect(rows[0]?.barPercent).toBe(100);
    expect(rows[0]?.models.map((model) => model.modelId)).toEqual([
      "glm-5.1",
      "glm-5.2",
    ]);
    expect(rows[0]?.models[0]?.requests).toBe(1);
    expect(rows[1]?.barPercent).toBe(55);
  });

  it("counts failed requests against the success rate", () => {
    const rows = buildAgentUsage(
      [agent("workbuddy")],
      ["workbuddy"],
      [request(), request({ status: 502 })],
    );

    expect(rows[0]?.requests).toBe(2);
    expect(rows[0]?.successRate).toBe(50);
  });
});

describe("formatTokens", () => {
  it("compacts large token counts", () => {
    expect(formatTokens(950)).toBe("950");
    expect(formatTokens(1_500)).toBe("1.5K");
    expect(formatTokens(12_000)).toBe("12K");
    expect(formatTokens(1_500_000)).toBe("1.5M");
  });
});
