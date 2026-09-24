import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProxyStatus } from "../types";
import { ProxyStatusBar } from "./ProxyStatusBar";

const baseProxy: ProxyStatus = {
  status: "stopped",
  host: "127.0.0.1",
  port: 54187,
  activeConnections: 0,
  completedRequests: 0,
  successfulRequests: 0,
  conversionFailures: 0,
  upstreamFailures: 0,
  recentRequests: [],
};

function renderBar(proxy: ProxyStatus, busy = false) {
  const props = {
    proxy,
    proxyAgentCount: 2,
    busy,
    onStart: vi.fn(),
    onStop: vi.fn(),
  };
  return { ...render(<ProxyStatusBar {...props} />), ...props };
}

beforeEach(() => {
  window.localStorage.setItem("at-switch-language", "zh-CN");
});

describe("ProxyStatusBar", () => {
  it("offers the start action and the endpoint while stopped", async () => {
    const user = userEvent.setup();
    const { onStart } = renderBar(baseProxy);

    expect(
      screen.getByRole("heading", { name: "回环监听器已停止" }),
    ).toBeInTheDocument();
    expect(screen.getByText("127.0.0.1:54187")).toBeInTheDocument();
    expect(screen.getByText("2 个智能体使用代理接管")).toBeInTheDocument();
    expect(screen.getByText("STOPPED")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "启动代理" }));
    expect(onStart).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "停止代理" })).not.toBeInTheDocument();
  });

  it("offers the stop action and runtime telemetry while running", async () => {
    const user = userEvent.setup();
    const { onStop } = renderBar({
      ...baseProxy,
      status: "running",
      startedAt: new Date().toISOString(),
      activeConnections: 3,
      completedRequests: 10,
      successfulRequests: 9,
    });

    expect(
      screen.getByRole("heading", { name: "回环监听器运行中" }),
    ).toBeInTheDocument();
    expect(screen.getByText("RUNNING")).toBeInTheDocument();
    expect(screen.getByText("90%")).toBeInTheDocument();
    expect(screen.getByText("10")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "停止代理" }));
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it("disables the control while a proxy operation is in flight", () => {
    renderBar(baseProxy, true);

    expect(screen.getByRole("button", { name: "启动代理" })).toBeDisabled();
  });
});
