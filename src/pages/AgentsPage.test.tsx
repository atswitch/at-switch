import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AgentSummary } from "../types";
import { AgentsPage } from "./AgentsPage";

const agents: AgentSummary[] = [
  "workbuddy",
  "codebuddy",
  "qclaw",
  "autoclaw",
  "codex",
].map((id) => ({
  id,
  displayName: id,
  installStatus: "installed",
  runtimeStatus: "not_running",
  configHealth: "healthy",
  adapterVerified: true,
  needsRestart: false,
  automaticRestartSupported: false,
}));

describe("AgentsPage", () => {
  it("shows the packaged logo for every supported Agent", () => {
    const { container } = render(
      <AgentsPage agents={agents} onRefresh={vi.fn()} onConfigure={vi.fn()} />,
    );

    for (const agent of agents) {
      expect(
        container.querySelector(
          `.agent-row [data-agent-logo="${agent.id}"] img`,
        ),
      ).toBeInTheDocument();
    }
  });

  it("lets Windows users choose and clear a custom installation location", async () => {
    const user = userEvent.setup();
    const onSelectInstallPath = vi.fn();
    const onClearInstallPath = vi.fn();
    const customAgent: AgentSummary = {
      ...agents[0]!,
      customInstallPath: "D:/Agents/WorkBuddy",
      usingCustomInstallPath: true,
    };
    render(
      <AgentsPage
        agents={[customAgent]}
        platform="windows"
        onRefresh={vi.fn()}
        onConfigure={vi.fn()}
        onSelectInstallPath={onSelectInstallPath}
        onClearInstallPath={onClearInstallPath}
      />,
    );

    await user.click(screen.getByRole("button", { name: "安装位置" }));
    await user.click(screen.getByRole("button", { name: "自动" }));

    expect(onSelectInstallPath).toHaveBeenCalledWith(customAgent);
    expect(onClearInstallPath).toHaveBeenCalledWith(customAgent);
  });

  it("offers the same custom installation recovery on macOS", async () => {
    const user = userEvent.setup();
    const onSelectInstallPath = vi.fn();
    render(
      <AgentsPage
        agents={[{ ...agents[0]!, installStatus: "not_installed" }]}
        platform="macos"
        onRefresh={vi.fn()}
        onConfigure={vi.fn()}
        onSelectInstallPath={onSelectInstallPath}
      />,
    );

    await user.click(screen.getByRole("button", { name: "安装位置" }));

    expect(onSelectInstallPath).toHaveBeenCalledOnce();
  });

  it("explains when a stale custom location fell back to automatic discovery", () => {
    render(
      <AgentsPage
        agents={[
          {
            ...agents[0]!,
            customInstallPath: "D:/Old/WorkBuddy",
            usingCustomInstallPath: false,
          },
        ]}
        platform="windows"
        onRefresh={vi.fn()}
        onConfigure={vi.fn()}
      />,
    );

    expect(screen.getByText("自定义位置失效，已自动发现")).toBeInTheDocument();
  });
});
