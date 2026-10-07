import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
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
  beforeEach(() => {
    localStorage.clear();
  });

  it("shows every Agent directly when within the visible limit", () => {
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

    const activeTab = screen.getByRole("tab", { name: /WorkBuddy/ });
    expect(activeTab).toHaveAttribute("aria-selected", "true");

    const uninstalledTab = screen.getByRole("tab", { name: /CodeBuddy/ });
    expect(uninstalledTab).toBeEnabled();
    expect(uninstalledTab).toHaveAttribute("aria-selected", "false");

    await user.click(uninstalledTab);
    expect(onSwitch).toHaveBeenCalledWith("codebuddy");
  });

  it("collapses overflow Agents behind a more button", async () => {
    const user = userEvent.setup();
    const onSwitch = vi.fn();
    render(
      <AgentSwitcher
        agents={[
          agent("workbuddy", "WorkBuddy", "installed"),
          agent("codebuddy", "CodeBuddy", "installed"),
          agent("qclaw", "QClaw", "installed"),
          agent("autoclaw", "AutoClaw", "installed"),
          agent("codex", "Codex", "installed"),
          agent("dumate", "百度搭子", "installed"),
          agent("hermes", "Hermes", "installed"),
          agent("opencode", "OpenCode", "installed"),
        ]}
        activeAgentId="workbuddy"
        onSwitch={onSwitch}
      />,
    );

    const tablist = screen.getByRole("tablist", { name: "选择智能体" });
    // 只直接展示前 6 个，其余收进「更多」。
    expect(within(tablist).getAllByRole("tab")).toHaveLength(6);
    expect(
      within(tablist).queryByRole("tab", { name: /Hermes/ }),
    ).not.toBeInTheDocument();

    await user.click(
      within(tablist).getByRole("button", { name: "更多智能体" }),
    );
    const dialog = screen.getByRole("dialog", { name: "选择智能体" });
    await user.click(within(dialog).getByRole("button", { name: /Hermes/ }));
    expect(onSwitch).toHaveBeenCalledWith("hermes");
  });
});
