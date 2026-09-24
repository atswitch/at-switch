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
    // 历史已验证模型不受新增模型影响，仍然可切换。
    expect(within(oldRow!).getByRole("button", { name: "切换" })).toBeEnabled();
    expect(within(oldRow!).queryByText("未验证")).not.toBeInTheDocument();
    // 新增模型未验证：给出标记并阻止切换，直到它自己通过连接测试。
    expect(within(newRow!).getByText("未验证")).toBeInTheDocument();
    expect(
      within(newRow!).getByRole("button", { name: "切换" }),
    ).toBeDisabled();
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

  it("groups models by provider and collapses them independently", async () => {
    const user = userEvent.setup();
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
    const makeProvider = (name: string, modelIds: string[]) => ({
      id: `provider-${name}`,
      name,
      kind: "custom" as const,
      protocol: "openai_chat_completions" as const,
      baseUrl: `https://${name}.example.test/v1`,
      isRecommended: false,
      isEnabled: true,
      hasApiKey: true,
      verificationStatus: "verified" as const,
      models: modelIds.map((modelId) => ({
        id: `provider-${name}:${modelId}`,
        providerId: `provider-${name}`,
        modelId,
        displayName: `${name} ${modelId}`,
        outputModality: "text" as const,
        supportsStreaming: true,
        supportsTools: true,
        source: "custom" as const,
        verificationStatus: "verified" as const,
      })),
    });

    render(
      <SwitchboardPage
        agent={agent}
        providers={[
          makeProvider("蒙云智算", ["glm-5.2", "glm-5.1"]),
          makeProvider("DeepSeek", ["deepseek-v4"]),
        ]}
        onCreateProvider={vi.fn()}
        onEditProvider={vi.fn()}
        onTestProvider={vi.fn()}
        onSwitchModel={vi.fn()}
        onRestoreNative={vi.fn()}
      />,
    );

    // 每个供应商一个分组头，默认全部展开。
    const toggle = screen.getByRole("button", { name: "蒙云智算 的模型" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("2 个模型")).toBeInTheDocument();
    expect(screen.getByText("蒙云智算 glm-5.2")).toBeInTheDocument();

    await user.click(toggle);

    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("蒙云智算 glm-5.2")).not.toBeInTheDocument();
    expect(screen.queryByText("蒙云智算 glm-5.1")).not.toBeInTheDocument();
    // 其他分组不受影响。
    expect(screen.getByText("DeepSeek deepseek-v4")).toBeInTheDocument();

    await user.click(toggle);

    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("蒙云智算 glm-5.2")).toBeInTheDocument();
  });

  it("blocks switching to a text model until its connection test passes", async () => {
    const user = userEvent.setup();
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
    const unverified: ProviderSummary = {
      id: "provider-pending",
      name: "蒙云智算",
      kind: "mongyun",
      protocol: "openai_chat_completions",
      baseUrl: "https://mongyun.example.test/v1",
      isRecommended: false,
      isEnabled: true,
      hasApiKey: true,
      verificationStatus: "draft_unverified",
      defaultModelId: "glm-5.2",
      models: [
        {
          id: "provider-pending:glm-5.2",
          providerId: "provider-pending",
          modelId: "glm-5.2",
          displayName: "GLM-5.2",
          outputModality: "text",
          supportsStreaming: true,
          supportsTools: true,
          source: "custom",
          verificationStatus: "draft_unverified",
        },
      ],
    };
    const onTestProvider = vi.fn();
    render(
      <SwitchboardPage
        agent={agent}
        providers={[unverified]}
        onCreateProvider={vi.fn()}
        onEditProvider={vi.fn()}
        onTestProvider={onTestProvider}
        onSwitchModel={vi.fn()}
        onRestoreNative={vi.fn()}
      />,
    );

    // 未验证文本模型有明确标记，且不能切换。
    expect(screen.getByText("未验证")).toBeInTheDocument();
    const switchButton = screen.getByRole("button", { name: "切换" });
    expect(switchButton).toBeDisabled();
    expect(switchButton).toHaveAttribute(
      "title",
      "该模型尚未通过连接验证，请先点击右侧连接测试",
    );

    // 验证图标改为「点击验证」的可访问名，提示用户点它。
    await user.click(screen.getByRole("button", { name: "点击验证 GLM-5.2" }));
    expect(onTestProvider).toHaveBeenCalledWith("provider-pending", "glm-5.2");

    // 徽标本身就是同一个入口，点它也能开始验证。
    await user.click(screen.getByRole("button", { name: /未验证/ }));
    expect(onTestProvider).toHaveBeenCalledTimes(2);
  });

  it("allows switching once the text model verification passes", () => {
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
    const verified: ProviderSummary = {
      id: "provider-verified",
      name: "蒙云智算",
      kind: "mongyun",
      protocol: "openai_chat_completions",
      baseUrl: "https://mongyun.example.test/v1",
      isRecommended: false,
      isEnabled: true,
      hasApiKey: true,
      verificationStatus: "verified",
      defaultModelId: "glm-5.2",
      models: [
        {
          id: "provider-verified:glm-5.2",
          providerId: "provider-verified",
          modelId: "glm-5.2",
          displayName: "GLM-5.2",
          outputModality: "text",
          supportsStreaming: true,
          supportsTools: true,
          source: "custom",
          verificationStatus: "verified",
        },
        {
          id: "provider-verified:image",
          providerId: "provider-verified",
          modelId: "image-1",
          displayName: "图片模型",
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
        providers={[verified]}
        onCreateProvider={vi.fn()}
        onEditProvider={vi.fn()}
        onTestProvider={vi.fn()}
        onSwitchModel={vi.fn()}
        onRestoreNative={vi.fn()}
      />,
    );

    // 已验证文本模型无需标记且可切换；非文本模型免验证，同样可切换。
    expect(screen.queryByText("未验证")).not.toBeInTheDocument();
    const buttons = screen.getAllByRole("button", { name: "切换" });
    expect(buttons).toHaveLength(2);
    for (const button of buttons) expect(button).toBeEnabled();
  });
});
