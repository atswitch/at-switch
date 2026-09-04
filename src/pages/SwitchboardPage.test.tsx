import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AgentSummary, ProviderSummary } from "../types";
import { SwitchboardPage } from "./SwitchboardPage";

describe("SwitchboardPage", () => {
  it("orders provider models by the shared preset order instead of creation order", () => {
    const agent: AgentSummary = {
      id: "workbuddy",
      displayName: "WorkBuddy",
      installStatus: "installed",
      runtimeStatus: "not_running",
      configHealth: "healthy",
      adapterVerified: true,
      needsRestart: false,
      automaticRestartSupported: false,
    };
    const createProvider = (
      kind: ProviderSummary["kind"],
      name: string,
    ): ProviderSummary => ({
      id: `provider-${kind}`,
      name,
      kind,
      protocol: "openai_chat_completions",
      baseUrl: `https://${kind}.example.test/v1`,
      isRecommended: false,
      isEnabled: true,
      hasApiKey: true,
      verificationStatus: "verified",
      models: [
        {
          id: `provider-${kind}:model`,
          providerId: `provider-${kind}`,
          modelId: `${kind}-model`,
          displayName: `${name} 模型`,
          outputModality: "text",
          supportsStreaming: true,
          supportsTools: true,
          source: kind === "custom" ? "custom" : "builtin",
          verificationStatus: "verified",
        },
      ],
    });

    render(
      <SwitchboardPage
        agent={agent}
        providers={[
          createProvider("minimax", "MiniMax"),
          createProvider("doubao", "豆包"),
          createProvider("custom", "自定义"),
          createProvider("qwen", "通义千问"),
          createProvider("mongyun", "蒙云智算"),
          createProvider("zhipu", "智谱"),
          createProvider("kimi", "Kimi"),
          createProvider("deepseek", "DeepSeek"),
        ]}
        onCreateProvider={vi.fn()}
        onEditProvider={vi.fn()}
        onTestProvider={vi.fn()}
        onSwitchModel={vi.fn()}
        onRestoreNative={vi.fn()}
      />,
    );

    const rows = within(
      screen.getByLabelText("WorkBuddy 模型列表"),
    ).getAllByRole("article");
    expect(
      rows.map(
        (row) => row.querySelector(".model-row__title strong")?.textContent,
      ),
    ).toEqual([
      "DeepSeek 模型",
      "智谱 模型",
      "蒙云智算 模型",
      "MiniMax 模型",
      "Kimi 模型",
      "通义千问 模型",
      "豆包 模型",
      "自定义 模型",
    ]);
  });

  it("keeps route context compact and prioritizes provider models", () => {
    const agent: AgentSummary = {
      id: "workbuddy",
      displayName: "WorkBuddy",
      installStatus: "installed",
      runtimeStatus: "not_running",
      configHealth: "healthy",
      adapterVerified: true,
      needsRestart: false,
      automaticRestartSupported: false,
      message: "WorkBuddy 已接入 AT-Switch",
    };

    const { container } = render(
      <SwitchboardPage
        agent={agent}
        providers={[]}
        onCreateProvider={vi.fn()}
        onEditProvider={vi.fn()}
        onTestProvider={vi.fn()}
        onSwitchModel={vi.fn()}
        onRestoreNative={vi.fn()}
      />,
    );

    const routeContext = screen.getByLabelText("当前智能体状态");
    expect(
      within(routeContext).getByText("WorkBuddy", {
        selector: ".switchboard__route strong span",
      }),
    ).toBeInTheDocument();
    expect(within(routeContext).getByText("Agent 原生路由")).toBeInTheDocument();
    expect(within(routeContext).getByText("默认配置")).toBeInTheDocument();
    expect(
      within(routeContext).queryByRole("button", { name: "刷新状态" }),
    ).not.toBeInTheDocument();
    expect(container.querySelector(".agent-current")).not.toBeInTheDocument();
    expect(
      screen.getByText("WorkBuddy 模型切换状态").closest(".switchboard-alert"),
    ).toHaveClass("switchboard-alert--info");
    expect(
      screen.getByRole("heading", { name: "供应商模型" }),
    ).toBeInTheDocument();
    const modelList = screen.getByLabelText("WorkBuddy 模型列表");
    expect(within(modelList).queryByText("默认配置")).not.toBeInTheDocument();
    expect(container.querySelector(".switchboard-native-control"))
      .toBeInTheDocument();
    expect(
      screen.queryByText(/模型管理 弹窗只维护模型/),
    ).not.toBeInTheDocument();
  });

  it("never marks the native model as active for an uninstalled Agent", () => {
    const agent: AgentSummary = {
      id: "codebuddy",
      displayName: "CodeBuddy",
      installStatus: "not_installed",
      runtimeStatus: "unknown",
      configHealth: "unsupported_version",
      adapterVerified: false,
      needsRestart: false,
      automaticRestartSupported: false,
      message: "未检测到 CodeBuddy",
    };

    const { container } = render(
      <SwitchboardPage
        agent={agent}
        providers={[]}
        onCreateProvider={vi.fn()}
        onEditProvider={vi.fn()}
        onTestProvider={vi.fn()}
        onSwitchModel={vi.fn()}
        onRestoreNative={vi.fn()}
      />,
    );

    expect(container.querySelector(".switchboard")).toHaveClass("is-unavailable");
    const nativeRow = screen.getByText("默认配置").closest("article");
    expect(nativeRow).not.toBeNull();
    expect(within(nativeRow!).queryByText("使用中")).not.toBeInTheDocument();
    expect(within(nativeRow!).getByRole("button", { name: "切换" })).toBeDisabled();
  });

  it("offers a Windows installation-folder recovery action when detection fails", async () => {
    const user = userEvent.setup();
    const onSelectInstallPath = vi.fn();
    const onClearInstallPath = vi.fn();
    const agent: AgentSummary = {
      id: "qclaw",
      displayName: "QClaw",
      installStatus: "not_installed",
      runtimeStatus: "unknown",
      configHealth: "unsupported_version",
      adapterVerified: false,
      customInstallPath: "D:/Agents/QClaw",
      needsRestart: false,
      automaticRestartSupported: false,
    };
    render(
      <SwitchboardPage
        agent={agent}
        providers={[]}
        platform="windows"
        onCreateProvider={vi.fn()}
        onEditProvider={vi.fn()}
        onTestProvider={vi.fn()}
        onSwitchModel={vi.fn()}
        onRestoreNative={vi.fn()}
        onSelectInstallPath={onSelectInstallPath}
        onClearInstallPath={onClearInstallPath}
      />,
    );

    await user.click(screen.getByRole("button", { name: "选择安装位置" }));
    await user.click(screen.getByRole("button", { name: "恢复自动发现" }));

    expect(onSelectInstallPath).toHaveBeenCalledOnce();
    expect(onClearInstallPath).toHaveBeenCalledOnce();
  });

  it("offers the same installation-folder recovery action on macOS", async () => {
    const user = userEvent.setup();
    const onSelectInstallPath = vi.fn();
    render(
      <SwitchboardPage
        agent={{
          id: "workbuddy",
          displayName: "WorkBuddy",
          installStatus: "not_installed",
          runtimeStatus: "unknown",
          configHealth: "unsupported_version",
          adapterVerified: false,
          needsRestart: false,
          automaticRestartSupported: false,
        }}
        providers={[]}
        platform="macos"
        onCreateProvider={vi.fn()}
        onEditProvider={vi.fn()}
        onTestProvider={vi.fn()}
        onSwitchModel={vi.fn()}
        onRestoreNative={vi.fn()}
        onSelectInstallPath={onSelectInstallPath}
      />,
    );

    await user.click(screen.getByRole("button", { name: "选择安装位置" }));

    expect(onSelectInstallPath).toHaveBeenCalledOnce();
  });

  it("keeps a previously verified model switchable when a new model is unverified", () => {
    const agent: AgentSummary = {
      id: "workbuddy",
      displayName: "WorkBuddy",
      installStatus: "installed",
      runtimeStatus: "not_running",
      configHealth: "healthy",
      adapterVerified: true,
      needsRestart: true,
      automaticRestartSupported: true,
    };
    const provider: ProviderSummary = {
      id: "provider-test",
      name: "测试供应商",
      kind: "custom",
      protocol: "openai_chat_completions",
      baseUrl: "https://api.example.test/v1",
      isRecommended: false,
      isEnabled: true,
      hasApiKey: true,
      verificationStatus: "draft_unverified",
      verifiedModelId: "model-a",
      defaultModelId: "model-a",
      models: [
        {
          id: "provider-test:model-a",
          providerId: "provider-test",
          modelId: "model-a",
          displayName: "历史模型",
          outputModality: "text",
          supportsStreaming: true,
          supportsTools: true,
          source: "custom",
          verificationStatus: "verified",
        },
        {
          id: "provider-test:model-b",
          providerId: "provider-test",
          modelId: "model-b",
          displayName: "新增模型",
          outputModality: "text",
          supportsStreaming: true,
          supportsTools: true,
          source: "custom",
          verificationStatus: "draft_unverified",
        },
      ],
    };

    render(
      <SwitchboardPage
        agent={agent}
        providers={[provider]}
        onCreateProvider={vi.fn()}
        onEditProvider={vi.fn()}
        onTestProvider={vi.fn()}
        onSwitchModel={vi.fn()}
        onRestoreNative={vi.fn()}
      />,
    );

    const oldRow = screen.getByText("历史模型").closest("article");
    const newRow = screen.getByText("新增模型").closest("article");
    expect(oldRow).not.toBeNull();
    expect(newRow).not.toBeNull();
    expect(within(oldRow!).getByRole("button", { name: "切换" })).toBeEnabled();
    expect(within(newRow!).queryByText("未验证")).not.toBeInTheDocument();
    expect(
      within(newRow!).getByRole("button", { name: "切换" }),
    ).toBeEnabled();
  });

  it("does not show verification controls for non-text models", async () => {
    const user = userEvent.setup();
    const agent: AgentSummary = {
      id: "workbuddy",
      displayName: "WorkBuddy",
      installStatus: "installed",
      runtimeStatus: "not_running",
      configHealth: "healthy",
      adapterVerified: true,
      needsRestart: true,
      automaticRestartSupported: true,
    };
    const provider: ProviderSummary = {
      id: "provider-media",
      name: "媒体供应商",
      kind: "custom",
      protocol: "openai_chat_completions",
      baseUrl: "https://media.example.test/v1",
      isRecommended: false,
      isEnabled: true,
      hasApiKey: true,
      verificationStatus: "draft_unverified",
      defaultModelId: "image-model",
      models: [
        {
          id: "provider-media:image-model",
          providerId: "provider-media",
          modelId: "image-model",
          displayName: "生图模型",
          outputModality: "image",
          supportsStreaming: false,
          supportsTools: false,
          source: "custom",
          verificationStatus: "draft_unverified",
        },
      ],
    };
    const onSwitchModel = vi.fn();

    render(
      <SwitchboardPage
        agent={agent}
        providers={[provider]}
        onCreateProvider={vi.fn()}
        onEditProvider={vi.fn()}
        onTestProvider={vi.fn()}
        onSwitchModel={onSwitchModel}
        onRestoreNative={vi.fn()}
      />,
    );

    const row = screen.getByText("生图模型").closest("article");
    expect(row).not.toBeNull();
    expect(within(row!).queryByText("未验证")).not.toBeInTheDocument();
    expect(
      within(row!).queryByRole("button", { name: "测试 媒体供应商" }),
    ).not.toBeInTheDocument();

    const switchButton = within(row!).getByRole("button", { name: "切换" });
    expect(switchButton).toBeEnabled();
    await user.click(switchButton);
    expect(onSwitchModel).toHaveBeenCalledWith(provider, provider.models[0]);
  });

  it("requires a saved API key before switching a non-text model", () => {
    const agent: AgentSummary = {
      id: "workbuddy",
      displayName: "WorkBuddy",
      installStatus: "installed",
      runtimeStatus: "not_running",
      configHealth: "healthy",
      adapterVerified: true,
      needsRestart: false,
      automaticRestartSupported: false,
    };
    const provider: ProviderSummary = {
      id: "provider-media",
      name: "媒体供应商",
      kind: "custom",
      protocol: "openai_chat_completions",
      baseUrl: "https://media.example.test/v1",
      isRecommended: false,
      isEnabled: true,
      hasApiKey: false,
      verificationStatus: "draft_unverified",
      models: [
        {
          id: "provider-media:image-model",
          providerId: "provider-media",
          modelId: "image-model",
          displayName: "无密钥生图模型",
          outputModality: "image",
          supportsStreaming: false,
          supportsTools: false,
          source: "custom",
          verificationStatus: "draft_unverified",
        },
      ],
    };

    render(
      <SwitchboardPage
        agent={agent}
        providers={[provider]}
        onCreateProvider={vi.fn()}
        onEditProvider={vi.fn()}
        onTestProvider={vi.fn()}
        onSwitchModel={vi.fn()}
        onRestoreNative={vi.fn()}
      />,
    );

    const row = screen.getByText("无密钥生图模型").closest("article");
    expect(row).not.toBeNull();
    expect(within(row!).getByRole("button", { name: "切换" })).toBeDisabled();
    expect(within(row!).getByRole("button", { name: "切换" })).toHaveAttribute(
      "title",
      "请先编辑模型供应商并保存 API Key",
    );
  });

  it("hides providers without models on the switchboard and shows a hint", () => {
    const agent: AgentSummary = {
      id: "workbuddy",
      displayName: "WorkBuddy",
      installStatus: "installed",
      runtimeStatus: "not_running",
      configHealth: "healthy",
      adapterVerified: true,
      needsRestart: false,
      automaticRestartSupported: false,
    };
    // 一个没有任何模型的空壳 provider，模拟遗留占位数据
    const emptyProvider: ProviderSummary = {
      id: "placeholder",
      name: "豪云智算",
      kind: "custom",
      protocol: "openai_chat_completions",
      baseUrl: "",
      isRecommended: false,
      isEnabled: true,
      hasApiKey: false,
      verificationStatus: "draft_unverified",
      models: [],
    };

    render(
      <SwitchboardPage
        agent={agent}
        providers={[emptyProvider]}
        onCreateProvider={vi.fn()}
        onEditProvider={vi.fn()}
        onTestProvider={vi.fn()}
        onSwitchModel={vi.fn()}
        onRestoreNative={vi.fn()}
      />,
    );

    // 空壳 provider 不在首页渲染
    expect(screen.queryByText("豪云智算")).not.toBeInTheDocument();
    expect(screen.queryByText("尚未配置模型")).not.toBeInTheDocument();
    // 显示引导文案
    expect(screen.getByText("还没有可切换的模型")).toBeInTheDocument();
  });
});
