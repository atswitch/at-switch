import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { AgentSummary } from "../types";
import { AgentReadonlyDetails } from "./AgentReadonlyDetails";

const agent: AgentSummary = {
  id: "aionclaw",
  displayName: "AionClaw",
  installStatus: "installed",
  runtimeStatus: "not_running",
  configHealth: "healthy",
  adapterVerified: false,
  detectedVersion: "1.14.0",
  installPath: "/Applications/AionClaw.app",
  configPath: "/Users/example/Library/Containers/com.quyuanai.aionclaw/Data",
  needsRestart: false,
  automaticRestartSupported: false,
  message:
    "AionClaw 已检测到；AT-Switch 当前只显示安装状态，不修改其内置模型配置。",
};

describe("AgentReadonlyDetails", () => {
  it("explains that the agent is detected but never rewritten", () => {
    render(<AgentReadonlyDetails agent={agent} />);

    expect(
      screen.getByText("AT-Switch 只检测该智能体，不修改其模型配置"),
    ).toBeInTheDocument();
    expect(screen.getByText(agent.message!)).toBeInTheDocument();
    expect(screen.getByText("只读")).toBeInTheDocument();
  });

  it("shows where the agent was detected and which config was probed", () => {
    render(<AgentReadonlyDetails agent={agent} />);

    expect(screen.getByText("/Applications/AionClaw.app")).toBeInTheDocument();
    expect(screen.getByText(agent.configPath!)).toBeInTheDocument();
    expect(screen.getByText("1.14.0")).toBeInTheDocument();
  });

  it("falls back to a generic notice when the adapter reports no message", () => {
    render(<AgentReadonlyDetails agent={{ ...agent, message: undefined }} />);

    expect(
      screen.getByText(
        "该智能体的模型设置由应用自身管理，AT-Switch 未提供稳定的第三方写入通道。",
      ),
    ).toBeInTheDocument();
  });

  it("offers no apply action", () => {
    render(<AgentReadonlyDetails agent={agent} />);

    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
