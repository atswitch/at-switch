import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import App from "./App";
import { api, getActiveMockSnapshot } from "./lib/api";

const seedTestProviders = async () => {
  const provider = await api.saveProvider({
    id: "preset-mongyun",
    name: "蒙云智算",
    kind: "mongyun",
    protocol: "openai_chat_completions",
    baseUrl: "https://api.g2claw.com/v1",
    apiKey: "sk-test-key",
    defaultModelId: "glm-5.2",
    models: [
      {
        modelId: "glm-5.2",
        displayName: "GLM-5.2",
        outputModality: "text",
        supportsStreaming: true,
        supportsTools: true,
      },
      {
        modelId: "glm-5.1",
        displayName: "GLM-5.1",
        outputModality: "text",
        supportsStreaming: true,
        supportsTools: true,
      },
    ],
  });
  await api.testProvider(provider.id, "glm-5.2");
  await api.testProvider(provider.id, "glm-5.1");

  await api.saveProvider({
    id: "preview-deepseek",
    name: "DeepSeek",
    kind: "deepseek",
    protocol: "openai_chat_completions",
    baseUrl: "https://api.deepseek.com/v1",
    defaultModelId: "deepseek-v4-flash",
    models: [
      {
        modelId: "deepseek-v4-flash",
        displayName: "DeepSeek V4 Flash",
        outputModality: "text",
        supportsStreaming: true,
        supportsTools: true,
      },
    ],
  });
};

const selectAgent = async (
  user: ReturnType<typeof userEvent.setup>,
  agentName: string,
) => {
  const tablist = screen.getByRole("tablist", { name: "选择智能体" });
  const tab = within(tablist).queryByRole("tab", { name: agentName });
  if (tab) {
    await user.click(tab);
    return;
  }
  // 超出胶囊可见数量的智能体收进「更多」弹窗。
  await user.click(
    within(tablist).getByRole("button", { name: "更多智能体" }),
  );
  await user.click(
    within(screen.getByRole("dialog", { name: "选择智能体" })).getByRole(
      "button",
      { name: new RegExp(agentName) },
    ),
  );
};

const openSettingsTab = async (
  user: ReturnType<typeof userEvent.setup>,
  tabName: string,
) => {
  await user.click(screen.getByRole("button", { name: "设置" }));
  await user.click(screen.getByRole("tab", { name: tabName }));
};

describe("AT-Switch desktop shell", () => {
  beforeEach(async () => {
    window.localStorage.clear();
    window.localStorage.setItem("at-switch-language", "zh-CN");
    api.resetMock();
    await seedTestProviders();
  });

  it("initializes with an empty provider catalog on fresh install", async () => {
    api.resetMock();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    expect(screen.queryByText("GLM-5.2")).not.toBeInTheDocument();
    expect(screen.queryByText("DeepSeek V4 Flash")).not.toBeInTheDocument();
  });

  it("loads the model switchboard, selects an Agent and opens Provider management", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    expect(screen.getAllByText("默认配置").length).toBeGreaterThan(0);
    expect(screen.getByText("GLM-5.2")).toBeInTheDocument();
    expect(screen.getByText("GLM-5.1")).toBeInTheDocument();

    await selectAgent(user, "QClaw");
    await screen.findByRole("heading", { name: "QClaw" });

    await openSettingsTab(user, "模型供应商");

    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "模型供应商与大模型" }),
      ).toBeInTheDocument();
    });
    expect(screen.getAllByText("蒙云智算").length).toBeGreaterThan(0);
  });

  it("shows model, settings and language in the toolbar capsule", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });

    // 右侧三合一胶囊：模型、设置、语言。
    expect(
      screen.getByRole("button", { name: "模型" }),
    ).toHaveAttribute("title", "模型供应商与大模型");
    expect(
      screen.getByRole("button", { name: "设置" }),
    ).toHaveAttribute("title", "打开设置中心");
    const langButton = screen.getByRole("button", { name: "中文" });
    expect(langButton).toHaveAttribute("title", "切换界面语言为 English");

    // 顶栏直接切换语言。
    await user.click(langButton);
    await waitFor(() => {
      expect(window.localStorage.getItem("at-switch-language")).toBe("en");
    });
    expect(screen.getByRole("button", { name: "EN" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "EN" }));
    await waitFor(() => {
      expect(window.localStorage.getItem("at-switch-language")).toBe("zh-CN");
    });
  });

  it("returns to the switchboard from the settings back button", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await openSettingsTab(user, "智能体");
    await screen.findByRole("heading", { name: "智能体" });

    const back = screen.getByRole("button", { name: "返回上一页" });
    expect(back).toHaveAttribute("title", "返回上一页");
    await user.click(back);
    expect(
      await screen.findByRole("heading", { name: "WorkBuddy" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "返回上一页" }),
    ).not.toBeInTheDocument();
  });

  it("uses the selected Agent name in every model-switch status card", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    expect(
      screen.getByText("WorkBuddy 模型切换状态"),
    ).toBeInTheDocument();

    for (const agentName of ["CodeBuddy", "QClaw", "AutoClaw", "Codex"]) {
      await selectAgent(user, agentName);
      await screen.findByRole("heading", { name: agentName });
      expect(
        screen.getByText(`${agentName} 模型切换状态`),
      ).toBeInTheDocument();
      expect(screen.queryByText("Agent 状态提示")).not.toBeInTheDocument();
    }
  });

  it("keeps unverified Agent adapters read-only", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await openSettingsTab(user, "智能体");

    const detailButtons = await screen.findAllByRole("button", {
      name: "详情",
    });
    // One per registered adapter; the browser mock mirrors the Rust registry.
    expect(detailButtons).toHaveLength(
      getActiveMockSnapshot().agents.length,
    );
    expect(
      detailButtons.every((button) => !button.hasAttribute("disabled")),
    ).toBe(true);
    expect(
      screen.queryAllByRole("button", { name: "需手动" }),
    ).toHaveLength(0);
  });

  it("does not show warning checkbox for plain HTTP addresses", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await openSettingsTab(user, "模型供应商");
    await user.click(
      screen.getByRole("button", { name: "新建模型供应商" }),
    );

    await user.type(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      "http://127.0.0.1:9000/v1",
    );

    expect(
      screen.queryByRole("checkbox", {
        name: /我确认该地址会明文传输 API Key/,
      }),
    ).not.toBeInTheDocument();
  });

  it("shows recovery guidance when a connection test fails", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    const modelRow = screen.getByText("DeepSeek V4 Flash").closest("article");
    expect(modelRow).not.toBeNull();
    // 未验证模型的连接测试入口使用「点击验证」这一可访问名。
    await user.click(
      within(modelRow!).getByRole("button", { name: /点击验证/ }),
    );

    expect(
      await screen.findByText("请先保存 API Key；编辑 Provider 并填写 API Key。"),
    ).toBeInTheDocument();
  });

  it("switches the selected Agent to another visible model", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    const modelRow = screen.getByText("GLM-5.1").closest("article");
    expect(modelRow).not.toBeNull();
    await user.click(within(modelRow!).getByRole("button", { name: "切换" }));
    expect(
      screen.getByRole("heading", {
        name: "重启 WorkBuddy 后切换模型",
      }),
    ).toBeInTheDocument();
    expect(screen.queryByText("WorkBuddy 已切换")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "取消" }));
    expect(
      screen.queryByRole("heading", {
        name: "重启 WorkBuddy 后切换模型",
      }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("WorkBuddy 已切换")).not.toBeInTheDocument();

    await user.click(within(modelRow!).getByRole("button", { name: "切换" }));
    await user.click(
      screen.getByRole("button", { name: "切换并自动重启" }),
    );

    expect(await screen.findByText("WorkBuddy 已切换")).toBeInTheDocument();
    expect(
      screen.getByText("WorkBuddy 已自动重新打开，新配置已经生效。"),
    ).toBeInTheDocument();
    await waitFor(() => {
      expect(
        screen.getByText("蒙云智算 · glm-5.1", {
          selector: ".switchboard__route strong span:last-child",
        }),
      ).toBeInTheDocument();
    });
  });

  it("confirms and automatically restarts Codex when switching", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await selectAgent(user, "Codex");
    await screen.findByRole("heading", { name: "Codex" });
    const modelRow = screen.getByText("GLM-5.1").closest("article");
    expect(modelRow).not.toBeNull();
    await user.click(within(modelRow!).getByRole("button", { name: "切换" }));

    expect(
      screen.getByRole("heading", { name: "重启 Codex 后切换模型" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/运行中的生成或工具调用会被中断/),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "切换并自动重启" }),
    );

    expect(await screen.findByText("Codex 已切换")).toBeInTheDocument();
    expect(
      screen.getByText("Codex 已自动重新打开，新配置已经生效。"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/请重启 Codex/)).not.toBeInTheDocument();
  });

  it("confirms and automatically restarts CodeBuddy when switching", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await selectAgent(user, "CodeBuddy");
    await screen.findByRole("heading", { name: "CodeBuddy" });
    const modelRow = screen.getByText("GLM-5.1").closest("article");
    expect(modelRow).not.toBeNull();
    await user.click(within(modelRow!).getByRole("button", { name: "切换" }));

    expect(
      screen.getByRole("heading", { name: "重启 CodeBuddy 后切换模型" }),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "切换并自动重启" }),
    );
    expect(await screen.findByText("CodeBuddy 已切换")).toBeInTheDocument();
    expect(
      screen.getByText("CodeBuddy 已自动重新打开，新配置已经生效。"),
    ).toBeInTheDocument();
  });

  it("keeps the switchboard direct-only and applies model changes in direct mode", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    expect(
      screen.queryByRole("button", { name: "本地代理" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "直连" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("WorkBuddy 当前仍由本地代理接管"),
    ).not.toBeInTheDocument();

    const modelRow = screen.getByText("GLM-5.1").closest("article");
    expect(modelRow).not.toBeNull();
    await user.click(within(modelRow!).getByRole("button", { name: "切换" }));
    await user.click(
      screen.getByRole("button", { name: "切换并自动重启" }),
    );
    expect(await screen.findByText("WorkBuddy 已切换")).toBeInTheDocument();
  });

  it("keeps local proxy configuration inside settings", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    expect(
      screen.queryByRole("button", { name: "本地代理" }),
    ).not.toBeInTheDocument();

    await openSettingsTab(user, "本地代理");
    expect(
      screen.queryByRole("button", { name: "打开本地代理设置" }),
    ).not.toBeInTheDocument();
    expect(
      await screen.findByRole("heading", { name: "回环监听器已停止" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "监听设置" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "用量明细" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "最近请求" }),
    ).toBeInTheDocument();

    const workbuddyCard = screen
      .getAllByText("WorkBuddy")
      .map((node) => node.closest(".agent-card"))
      .find((card): card is HTMLElement => card !== null);
    expect(workbuddyCard).toBeDefined();
    await user.click(
      within(workbuddyCard!).getByRole("button", { name: "本地代理配置" }),
    );
    expect(
      screen.getByRole("heading", { name: "本地代理配置 WorkBuddy" }),
    ).toBeInTheDocument();
    expect(screen.getByText("本地代理（高级）")).toBeInTheDocument();
    expect(
      screen.queryByRole("radio", { name: /Agent 直连/ }),
    ).not.toBeInTheDocument();
  });

  it("starts and stops the local proxy inside settings", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await openSettingsTab(user, "本地代理");

    await user.click(screen.getByRole("button", { name: "启动代理" }));
    expect(
      await screen.findByRole("heading", { name: "回环监听器运行中" }),
    ).toBeInTheDocument();
    expect(screen.getByText("本地代理已启动")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "启动代理" }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "停止代理" }));
    expect(
      await screen.findByRole("heading", { name: "回环监听器已停止" }),
    ).toBeInTheDocument();
    expect(screen.getByText("本地代理已停止")).toBeInTheDocument();
  });

  it("opens the proxy tab from a legacy local-proxy deep link", async () => {
    window.history.pushState({}, "", "?page=proxy");
    try {
      render(<App />);

      const proxyTab = await screen.findByRole("tab", { name: "本地代理" });
      expect(proxyTab).toHaveAttribute("aria-selected", "true");
      expect(
        await screen.findByRole("heading", { name: "监听设置" }),
      ).toBeInTheDocument();
    } finally {
      window.history.pushState({}, "", "/");
    }
  });

  it("organizes settings into a left navigation with five sections", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await user.click(screen.getByRole("button", { name: "设置" }));

    const settingsTabs = screen.getByRole("tablist", {
      name: "设置分类",
    });
    expect(
      within(settingsTabs)
        .getAllByRole("tab")
        .map((tab) => tab.textContent),
    ).toEqual(["智能体", "模型供应商", "外观与生命周期", "本地代理", "关于"]);

    // 默认停留在外观与生命周期。
    expect(screen.getByRole("heading", { name: "界面" })).toBeInTheDocument();

    // 「关于」展示版本信息与升级按钮。
    await user.click(screen.getByRole("tab", { name: "关于" }));
    expect(screen.getByText(/当前版本/)).toBeInTheDocument();

    // 使用统计已合并到代理分类，设置中心不再单独展示。
    expect(screen.queryByRole("tab", { name: "使用统计" })).not.toBeInTheDocument();
  });

  it("confirms and automatically restarts QClaw when switching", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await selectAgent(user, "QClaw");
    await screen.findByRole("heading", { name: "QClaw" });
    const modelRow = screen.getByText("GLM-5.1").closest("article");
    expect(modelRow).not.toBeNull();
    await user.click(within(modelRow!).getByRole("button", { name: "切换" }));

    expect(
      screen.getByRole("heading", { name: "重启 QClaw 后切换模型" }),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "切换并自动重启" }),
    );
    expect(await screen.findByText("QClaw 已切换")).toBeInTheDocument();
    expect(
      screen.getByText("QClaw 已自动重新打开，新配置已经生效。"),
    ).toBeInTheDocument();
  });

  it("confirms, switches and automatically restarts AutoClaw", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await selectAgent(user, "AutoClaw");
    await screen.findByRole("heading", { name: "AutoClaw" });
    const modelRow = screen.getByText("GLM-5.1").closest("article");
    expect(modelRow).not.toBeNull();
    await user.click(within(modelRow!).getByRole("button", { name: "切换" }));

    expect(
      screen.getByRole("heading", { name: "重启 AutoClaw 后切换模型" }),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "切换并自动重启" }),
    );
    expect(await screen.findByText("AutoClaw 已切换")).toBeInTheDocument();
    expect(
      screen.getByText("AutoClaw 已自动重新打开，新配置已经生效。"),
    ).toBeInTheDocument();
  });

  it("confirms, switches and automatically restarts DuMate", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await selectAgent(user, "百度搭子");
    await screen.findByRole("heading", { name: "百度搭子" });
    const modelRow = screen.getByText("GLM-5.1").closest("article");
    expect(modelRow).not.toBeNull();
    await user.click(within(modelRow!).getByRole("button", { name: "切换" }));

    expect(
      screen.getByRole("heading", { name: "重启 百度搭子 后切换模型" }),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "切换并自动重启" }),
    );
    expect(await screen.findByText("百度搭子 已切换")).toBeInTheDocument();
    expect(
      screen.getByText("百度搭子 已自动重新打开，新配置已经生效。"),
    ).toBeInTheDocument();
  });

  it("restores the Agent original configuration from the switchboard", async () => {
    await api.applyAgentBinding({
      agentId: "workbuddy",
      providerId: "preset-mongyun",
      modelId: "glm-5.2",
      mode: "direct",
    });
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    const nativeRow = screen
      .getAllByText("默认配置")
      .find((el) => el.closest("article"))
      ?.closest("article");
    expect(nativeRow).not.toBeNull();
    await user.click(within(nativeRow!).getByRole("button", { name: "切换" }));
    expect(
      screen.getByRole("heading", {
        name: "重启 WorkBuddy 后恢复默认配置",
      }),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "恢复并自动重启" }),
    );

    expect(
      await screen.findByText("WorkBuddy 已恢复默认配置"),
    ).toBeInTheDocument();
    await waitFor(() => {
      expect(
        screen.getByText("默认配置", {
          selector: ".switchboard-native-control strong",
        }),
      ).toBeInTheDocument();
    });
  });

  it("switches to another Agent when clicked, rendering its heading and switchboard", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await selectAgent(user, "Codex");

    expect(await screen.findByRole("heading", { name: "Codex" })).toBeInTheDocument();
  });

  it("renders unbind-only alert when deleting an in-use provider while other providers exist", async () => {
    await api.applyAgentBinding({
      agentId: "workbuddy",
      providerId: "preset-mongyun",
      modelId: "glm-5.2",
      mode: "direct",
    });
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await openSettingsTab(user, "模型供应商");
    await screen.findByRole("heading", { name: "模型供应商与大模型" });

    await user.click(screen.getByRole("button", { name: "删除 蒙云智算" }));

    expect(
      screen.getByText("当前有 1 个智能体正在使用该供应商："),
    ).toBeInTheDocument();
    expect(
      screen.getByText("当前模型供应商正在使用中，您确定要删除吗？"),
    ).toBeInTheDocument();
  });

  it("renders restore-native alert when deleting the last provider in the system", async () => {
    // Remove preview-deepseek so preset-mongyun is the only provider left
    await api.deleteProvider("preview-deepseek");
    await api.applyAgentBinding({
      agentId: "workbuddy",
      providerId: "preset-mongyun",
      modelId: "glm-5.2",
      mode: "direct",
    });
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await openSettingsTab(user, "模型供应商");
    await screen.findByRole("heading", { name: "模型供应商与大模型" });

    await user.click(screen.getByRole("button", { name: "删除 蒙云智算" }));

    expect(
      screen.getByText("当前有 1 个智能体正在使用该供应商："),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "删除当前模型后上述智能体将自动恢复为官方默认配置，在新建会话或重新启动后生效。",
      ),
    ).toBeInTheDocument();
  });

});
