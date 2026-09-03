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
});
