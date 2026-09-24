import { render, screen, within } from "@testing-library/react";
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
  it("shows every Agent directly instead of collapsing them into a menu", () => {
    const user = userEvent.setup();
    const onSwitch = vi.fn();
    render(
      <AgentSwitcher
        agents={[
          agent("workbuddy", "WorkBuddy", "installed"),
          agent("codebuddy", "CodeBuddy", "installed"),
          agent("qclaw", "QClaw", "not_installed"),
          agent("coze", "扣子", "not_installed"),
        ]}
        activeAgentId="workbuddy"
        onSwitch={() => undefined}
      />,
    );

    const tablist = screen.getByRole("tablist", { name: "选择智能体" });
    const tabs = within(tablist).getAllByRole("tab");

    // Detection-only agents stay out of the switcher: there is no switch to run.
    expect(tabs).toHaveLength(3);
    expect(within(tablist).getByRole("tab", { name: /QClaw/ })).toBeVisible();
    expect(
      within(tablist).queryByRole("tab", { name: /扣子/ }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agent-switcher-trigger"),
    ).not.toBeInTheDocument();
  });

  it("allows selecting uninstalled Agents without greying out the dropdown", async () => {
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

    const activeTab = screen.getByRole("tab", { name: /WorkBuddy/ });
    expect(activeTab).toHaveAttribute("aria-selected", "true");

    const uninstalledTab = screen.getByRole("tab", { name: /CodeBuddy/ });
    expect(uninstalledTab).toBeEnabled();
    expect(uninstalledTab).toHaveAttribute("aria-selected", "false");

    await user.click(uninstalledTab);
    expect(onSwitch).toHaveBeenCalledWith("codebuddy");
  });
});
