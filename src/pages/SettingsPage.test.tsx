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
  recentRequests: [],
};

function renderSettingsPage(platform: string) {
  return render(
    <SettingsPage
      appVersion="3.15.1"
      platform={platform}
      settings={settings}
      proxy={proxy}
      agents={[]}
      providers={[]}
      proxyAgentCount={0}
      proxyBusy={false}
      onUpdate={vi.fn()}
      onStartProxy={vi.fn()}
      onStopProxy={vi.fn()}
      onUpdateProxyPort={vi.fn()}
      onToggleProxyPref={vi.fn()}
      onRefresh={vi.fn()}
      onConfigure={vi.fn()}
      onSelectInstallPath={vi.fn()}
      onClearInstallPath={vi.fn()}
      onCreateProvider={vi.fn()}
      onEditProvider={vi.fn()}
      onDeleteProvider={vi.fn()}
      onTestProvider={vi.fn()}
    />,
  );
}

describe("SettingsPage", () => {
  it("shows the runtime version and only the current platform", () => {
    renderSettingsPage("macos");

    expect(screen.getByText("v3.15.1")).toBeInTheDocument();
    expect(screen.getByText("macOS Universal")).toBeInTheDocument();
    expect(screen.queryByText("Windows x64")).not.toBeInTheDocument();
  });

  it("falls back to the raw platform label outside macOS and Windows", () => {
    renderSettingsPage("linux");

    expect(screen.getByText("linux")).toBeInTheDocument();
    expect(screen.queryByText("macOS Universal")).not.toBeInTheDocument();
  });
});
