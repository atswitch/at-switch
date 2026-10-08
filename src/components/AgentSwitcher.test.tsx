import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AgentSummary } from "../types";
import { AgentSwitcher } from "./AgentSwitcher";

function agent(
  id: string,
  displayName: string,
  installStatus: AgentSummary["installStatus"],
): AgentSummary {
  return {
    id,
    displayName,
    installStatus,
    runtimeStatus: installStatus === "not_installed" ? "unknown" : "not_running",
    configHealth:
      installStatus === "not_installed" ? "unsupported_version" : "healthy",
    adapterVerified: installStatus !== "not_installed",
    needsRestart: true,
    automaticRestartSupported: true,
  };
}

describe("AgentSwitcher", () => {
  const allAgents = [
    agent("workbuddy", "WorkBuddy", "installed"),
    agent("codebuddy", "CodeBuddy", "installed"),
    agent("qclaw", "QClaw", "installed"),
    agent("autoclaw", "AutoClaw", "installed"),
    agent("codex", "Codex", "installed"),
    agent("dumate", "DuMate", "installed"),
    agent("ima", "ima", "installed"),
    agent("traecode", "TraeCode", "installed"),
    agent("traework", "TraeWork", "installed"),
  ];

  it("shows ima in the same selectable top bar before account connection", async () => {
    const user = userEvent.setup();
    const onSwitch = vi.fn();
    render(<AgentSwitcher agents={[
      agent("workbuddy", "WorkBuddy", "installed"),
      { ...agent("ima", "ima", "installed"), requiresAccountConnection: true },
    ]} activeAgentId="workbuddy" onSwitch={onSwitch} />);
    await user.click(screen.getByRole("tab", { name: "ima" }));
    expect(onSwitch).toHaveBeenCalledWith("ima");
  });

  it("allows selecting uninstalled Agents without greying out top bar tabs", async () => {
    const user = userEvent.setup();
    const onSwitch = vi.fn();
    render(
      <AgentSwitcher
        agents={[
          agent("workbuddy", "WorkBuddy", "installed"),
          agent("codebuddy", "CodeBuddy", "not_installed"),
        ]}
        activeAgentId="workbuddy"
        onSwitch={onSwitch}
      />,
    );

    const uninstalledTab = screen.getByRole("tab", { name: "CodeBuddy" });
    expect(uninstalledTab).not.toBeDisabled();
    expect(uninstalledTab).not.toHaveClass("is-unavailable");
    await user.click(uninstalledTab);
    expect(onSwitch).toHaveBeenCalledWith("codebuddy");
  });

  it("shows TraeCode and TraeWork as independent selectable agents", async () => {
    const user = userEvent.setup();
    const onSwitch = vi.fn();
    render(<AgentSwitcher agents={[
      agent("traecode", "TraeCode", "installed"),
      agent("traework", "TraeWork", "installed"),
    ]} activeAgentId="traecode" onSwitch={onSwitch} />);
    await user.click(screen.getByRole("tab", { name: "TraeWork" }));
    expect(onSwitch).toHaveBeenCalledWith("traework");
  });

  it("keeps six Agents visible and selects the rest from a compact menu", async () => {
    const user = userEvent.setup();
    const onSwitch = vi.fn();
    render(<AgentSwitcher agents={allAgents} activeAgentId="workbuddy" onSwitch={onSwitch} />);

    expect(screen.getAllByRole("tab")).toHaveLength(6);
    expect(screen.queryByRole("tab", { name: "TraeWork" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "更多智能体，3 个" }));
    await user.click(screen.getByRole("menuitemradio", { name: /TraeWork/ }));
    expect(onSwitch).toHaveBeenCalledWith("traework");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("keeps the first six Agents fixed while marking an active overflow Agent in the menu", async () => {
    const user = userEvent.setup();
    render(<AgentSwitcher agents={allAgents} activeAgentId="traework" onSwitch={vi.fn()} />);

    expect(screen.getAllByRole("tab")).toHaveLength(6);
    expect(screen.getByRole("tab", { name: "DuMate" })).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: "TraeWork" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "更多智能体，3 个" }));
    expect(screen.getByRole("menuitemradio", { name: /TraeWork/ })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });
});
