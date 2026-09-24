import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { AgentSummary, ProxyStatus } from "../types";
import { buildAgentUsage } from "../lib/proxyUsage";
import { ProxyRecentRequests } from "./ProxyRecentRequests";
import { ProxyUsageByAgent } from "./ProxyUsageByAgent";

const agent: AgentSummary = {
  id: "workbuddy",
  displayName: "WorkBuddy",
  installStatus: "installed",
  runtimeStatus: "not_running",
  configHealth: "healthy",
  adapterVerified: true,
  proxyPrefEnabled: true,
  needsRestart: false,
  automaticRestartSupported: false,
};

const request = {
  at: "2026-01-01T10:20:30.000Z",
  agentId: "workbuddy",
  providerId: "mongyun",
  providerName: "蒙云智算",
  model: "glm-5.2",
  status: 200,
  inputTokens: 12_000,
  outputTokens: 8_000,
};

const runningProxy: ProxyStatus = {
  status: "running",
  host: "127.0.0.1",
  port: 54187,
  startedAt: "2026-01-01T10:00:00.000Z",
  activeConnections: 1,
  completedRequests: 2,
  successfulRequests: 1,
  conversionFailures: 0,
  upstreamFailures: 0,
  recentRequests: [
    request,
    // 2xx 但上游没有返回 usage：计入「用量未知」，不计入成功计费。
    { ...request, at: "2026-01-01T10:21:00.000Z", inputTokens: undefined, outputTokens: undefined },
    {
      ...request,
      at: "2026-01-01T10:22:00.000Z",
      status: 502,
      inputTokens: undefined,
      outputTokens: undefined,
    },
  ],
  proxiedAgents: ["workbuddy"],
};

const stoppedProxy: ProxyStatus = {
  ...runningProxy,
  status: "stopped",
  startedAt: undefined,
  recentRequests: [],
  proxiedAgents: [],
};

describe("ProxyUsageByAgent", () => {
  it("shows metrics, share bar and per-model breakdown", () => {
    const usageByAgent = buildAgentUsage(
      [agent],
      ["workbuddy"],
      runningProxy.recentRequests,
    );
    render(<ProxyUsageByAgent proxy={runningProxy} usageByAgent={usageByAgent} />);

    expect(screen.getByText("3 次请求")).toBeInTheDocument();
    expect(screen.getByText("12K")).toBeInTheDocument();
    expect(screen.getByText("8.0K")).toBeInTheDocument();
    expect(screen.getByText("20K")).toBeInTheDocument();
    // 3 条请求中 1 条 502、1 条用量未知：可计费 2 条中成功 1 条 → 50%，进入告警配色。
    expect(screen.getByText("50%")).toBeInTheDocument();
    expect(screen.getByText("50%")).toHaveClass("is-warn");
    expect(screen.getByText("glm-5.2")).toBeInTheDocument();
    expect(screen.getByText("蒙云智算")).toBeInTheDocument();
    expect(
      screen.getByText("1 条请求的上游未返回 token 用量"),
    ).toBeInTheDocument();
  });

  it("tells the user how to start recording when the proxy is stopped", () => {
    render(<ProxyUsageByAgent proxy={stoppedProxy} usageByAgent={[]} />);

    expect(screen.getByText("还没有用量记录")).toBeInTheDocument();
    expect(screen.getByText(/启动代理/)).toBeInTheDocument();
  });
});

describe("ProxyRecentRequests", () => {
  it("renders one row per request with a status chip", () => {
    render(<ProxyRecentRequests proxy={runningProxy} />);

    const table = screen.getByRole("table");
    expect(within(table).getAllByRole("row")).toHaveLength(4); // 表头 + 3 条请求
    const okChips = within(table).getAllByText("200");
    expect(okChips).toHaveLength(2);
    for (const chip of okChips) expect(chip).toHaveClass("status-chip--ok");
    expect(within(table).getByText("502")).toHaveClass("status-chip--warn");
    expect(within(table).getByText("12000")).toBeInTheDocument();
    expect(within(table).getByText("8000")).toBeInTheDocument();
    // 两类没有 usage 的请求（用量未知 2xx 与失败请求）都显示「未知」。
    expect(within(table).getAllByText("未知")).toHaveLength(2);
  });

  it("renders an empty state when nothing has been logged", () => {
    render(<ProxyRecentRequests proxy={stoppedProxy} />);

    expect(screen.getByText("还没有请求记录")).toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });
});
