import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AgentSummary } from "../types";
import { AgentRestartConfirmation } from "./AgentRestartConfirmation";

function codexAgent(
  automaticRestartSupported: boolean,
): AgentSummary {
  return {
    id: "codex",
    displayName: "Codex",
    installStatus: "installed",
    runtimeStatus: "running",
    configHealth: "healthy",
    adapterVerified: true,
    needsRestart: true,
    automaticRestartSupported,
  };
}

describe("AgentRestartConfirmation", () => {
  it("offers an automatic restart for a desktop installation", () => {
    render(
      <AgentRestartConfirmation
        agent={codexAgent(true)}
        operation="apply"
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("heading", { name: "重启 Codex 后切换模型" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "切换并自动重启" }),
    ).toBeInTheDocument();
  });

  it("never promises to terminate or restart a CLI installation", () => {
    render(
      <AgentRestartConfirmation
        agent={codexAgent(false)}
        operation="apply"
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("heading", {
        name: "切换模型并在下次启动 Codex 时生效",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/不会终止终端中的任务/),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "保存配置" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /自动重启/ }),
    ).not.toBeInTheDocument();
  });
});
