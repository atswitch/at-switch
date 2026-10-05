import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AgentSummary } from "../types";
import { AgentAccountConnectionConfirmation } from "./AgentAccountConnectionConfirmation";

const ima: AgentSummary = {
  id: "ima", displayName: "ima", installStatus: "installed",
  runtimeStatus: "running", configHealth: "healthy", adapterVerified: true,
  needsRestart: true, automaticRestartSupported: true,
  requiresAccountConnection: true,
};

describe("AgentAccountConnectionConfirmation", () => {
  it("includes account access, cloud storage and restart in one confirmation", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(<AgentAccountConnectionConfirmation agent={ima} operation="apply" onCancel={vi.fn()} onConfirm={onConfirm} />);
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    expect(screen.getByText(/读取 ima 的登录凭据/)).toBeInTheDocument();
    expect(screen.getByText(/API Key 和模型名.*腾讯 ima/)).toBeInTheDocument();
    expect(screen.getByText(/安全退出并重新打开 ima/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "连接并切换" }));
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("explains exact original-model preservation when reconnecting to restore", async () => {
    const user = userEvent.setup();
    const onCancel = vi.fn();
    const onConfirm = vi.fn();
    render(<AgentAccountConnectionConfirmation agent={{ ...ima, runtimeStatus: "not_running" }} operation="restore" onCancel={onCancel} onConfirm={onConfirm} />);
    expect(screen.getByRole("dialog", { name: "连接 ima 并恢复原始模型" })).toBeInTheDocument();
    expect(screen.getByText(/保留你已有的自定义模型/)).toBeInTheDocument();
    expect(screen.queryByText(/安全退出并重新打开 ima/)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "取消" }));
    expect(onCancel).toHaveBeenCalledOnce();
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
