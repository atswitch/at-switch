import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AppSettings, ProxyStatus } from "../types";
import { SettingsPage } from "./SettingsPage";

const settings: AppSettings = {
  language: "zh-CN",
  theme: "system",
  startAtLogin: false,
  keepRunningInBackground: false,
};

const proxy: ProxyStatus = {
  status: "stopped",
  host: "127.0.0.1",
  port: 54187,
  activeConnections: 0,
  completedRequests: 0,
  successfulRequests: 0,
  conversionFailures: 0,
  upstreamFailures: 0,
};

describe("SettingsPage", () => {
  it("shows the runtime version and only the current platform", () => {
    const props = {
      appVersion: "3.15.1",
      settings,
      proxy,
      proxyAgentCount: 0,
      onOpenProxy: vi.fn(),
      onUpdate: vi.fn(),
    };
    const { rerender } = render(
      <SettingsPage {...props} platform="macos" />,
    );

    expect(screen.getByText("v3.15.1")).toBeInTheDocument();
    expect(screen.getByText("macOS")).toBeInTheDocument();
    expect(screen.queryByText("Windows")).not.toBeInTheDocument();

    rerender(<SettingsPage {...props} platform="windows" />);

    expect(screen.getByText("Windows")).toBeInTheDocument();
    expect(screen.queryByText("macOS")).not.toBeInTheDocument();
  });
});
