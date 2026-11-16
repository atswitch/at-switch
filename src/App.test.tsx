import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { api } from "./lib/api";

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

/** 顶部工具入口已迁入会员菜单，测试统一先展开菜单再点击快捷项。 */
const openMemberMenu = async (
  user: ReturnType<typeof userEvent.setup>,
  label = "未登录 / 点击登录",
) => {
  await user.click(screen.getByRole("button", { name: label }));
};

describe("AT-Switch desktop shell", () => {
  afterEach(() => vi.restoreAllMocks());
  beforeEach(async () => {
    window.localStorage.clear();
    window.localStorage.setItem("at-switch-language", "zh-CN");
    api.resetMock();
    await seedTestProviders();
  });

  it("shows confirmed TRAE switches immediately and rejects late refreshes from previous switches", async () => {
    await api.applyAgentBinding({ agentId: "trae", providerId: "preset-mongyun", modelId: "glm-5.2", mode: "direct" }, true);
    const first = await api.bootstrap();
    first.agents.find((agent) => agent.id === "trae")!.needsRestart = false;
    vi.spyOn(api, "bootstrap").mockResolvedValueOnce(first);
    window.localStorage.setItem("at-switch-active-agent", "trae");
    const pending: ((value: typeof first) => void)[] = [];
    vi.spyOn(api, "refresh").mockImplementation(() => new Promise((resolve) => pending.push(resolve)));
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("heading", { name: "TRAE" });
    act(() => window.dispatchEvent(new Event("focus")));
    await waitFor(() => expect(pending).toHaveLength(1));
    const a = screen.getByText("GLM-5.2").closest("article")!;
    const b = screen.getByText("GLM-5.1").closest("article")!;
    await user.click(within(b).getByRole("button", { name: "切换" }));
    await waitFor(() => expect(within(b).getByText("使用中")).toBeInTheDocument());
    const second = await api.bootstrap();
    await act(async () => pending[0]!(first));
    expect(within(b).getByText("使用中")).toBeInTheDocument();
    await user.click(within(a).getByRole("button", { name: "切换" }));
    await waitFor(() => expect(within(a).getByText("使用中")).toBeInTheDocument());
    await waitFor(() => expect(pending).toHaveLength(3));
    await act(async () => pending[1]!(second));
    expect(within(a).getByText("使用中")).toBeInTheDocument();
    await act(async () => pending[2]!(await api.bootstrap()));
    expect(within(a).getByText("使用中")).toBeInTheDocument();
  });

  it("connects ima once, cancels without access, switches again and restores original selections", async () => {
    const user = userEvent.setup();
    const apply = vi.spyOn(api, "applyAgentBinding");
    const restore = vi.spyOn(api, "restoreAgentNative");
    render(<App />);
    await user.click(await screen.findByRole("button", { name: /更多智能体/ }));
    await user.click(screen.getByRole("menuitemradio", { name: /^ima/ }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(apply).not.toHaveBeenCalled();

    const firstRow = screen.getByText("GLM-5.2").closest("article")!;
    await user.click(within(firstRow).getByRole("button", { name: "切换" }));
    let dialog = screen.getByRole("dialog", { name: "连接 ima 并切换模型" });
    expect(within(dialog).getByText(/API Key 和模型名.*腾讯 ima/)).toBeInTheDocument();
    expect(within(dialog).getByText(/恢复原始模型时保留你已有的自定义模型/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "取消" }));
    expect(apply).not.toHaveBeenCalled();

    await user.click(within(firstRow).getByRole("button", { name: "切换" }));
    dialog = screen.getByRole("dialog", { name: "连接 ima 并切换模型" });
    await user.dblClick(within(dialog).getByRole("button", { name: "连接并切换" }));
    await waitFor(() => expect(within(firstRow).getByText("使用中")).toBeInTheDocument());
    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenLastCalledWith(expect.objectContaining({ agentId: "ima", mode: "direct", modelId: "glm-5.2" }), true);

    const nextRow = screen.getByText("GLM-5.1").closest("article")!;
    await user.click(within(nextRow).getByRole("button", { name: "切换" }));
    await waitFor(() => expect(within(nextRow).getByText("使用中")).toBeInTheDocument());
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(apply).toHaveBeenLastCalledWith(expect.objectContaining({ agentId: "ima", modelId: "glm-5.1" }), undefined);

    const original = screen.getByText("原始模型").closest("article")!;
    await user.click(within(original).getByRole("button", { name: "切换" }));
    await waitFor(() => expect(within(original).getByText("使用中")).toBeInTheDocument());
    expect(restore).toHaveBeenCalledWith("ima", undefined);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    const state = await api.bootstrap();
    expect(state.agents.find((agent) => agent.id === "ima")).toEqual(expect.objectContaining({ requiresAccountConnection: false, providerId: undefined }));
  });

  it("keeps ima's current selection after a failed switch and allows retry", async () => {
    await api.applyAgentBinding({ agentId: "ima", providerId: "preset-mongyun", modelId: "glm-5.2", mode: "direct" }, true);
    const apply = vi.spyOn(api, "applyAgentBinding").mockRejectedValueOnce({ code: "ima_switch_failed", message: "切换失败，已恢复先前配置" });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: /更多智能体/ }));
    await user.click(screen.getByRole("menuitemradio", { name: /^ima/ }));
    const currentRow = screen.getByText("GLM-5.2").closest("article")!;
    const nextRow = screen.getByText("GLM-5.1").closest("article")!;
    await user.click(within(nextRow).getByRole("button", { name: "切换" }));
    await screen.findByText("切换失败，已恢复先前配置");
    expect(within(currentRow).getByText("使用中")).toBeInTheDocument();
    expect(within(nextRow).getByRole("button", { name: "切换" })).toBeEnabled();
    await user.click(within(nextRow).getByRole("button", { name: "切换" }));
    await waitFor(() => expect(within(nextRow).getByText("使用中")).toBeInTheDocument());
    expect(apply).toHaveBeenCalledTimes(2);
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

    await user.click(screen.getByRole("tab", { name: "QClaw" }));
    await screen.findByRole("heading", { name: "QClaw" });

    await openMemberMenu(user);
    await user.click(
      screen.getByRole("button", { name: "模型供应商与大模型" }),
    );

    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "模型供应商与大模型" }),
      ).toBeInTheDocument();
    });
    expect(screen.getAllByText("蒙云智算").length).toBeGreaterThan(0);
  });

  it("keeps only member entry and language in the toolbar, with shortcuts in the menu", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    const toolbar = screen.getByRole("navigation", { name: "工具导航" });
    // 工具入口已收进会员菜单：顶部只剩会员入口与语言切换。
    for (const name of [
      "刷新状态",
      "智能体状态",
      "模型供应商与大模型",
      "高级设置",
      "新增模型供应商",
    ]) {
      expect(
        within(toolbar).queryByRole("button", { name }),
      ).not.toBeInTheDocument();
    }
    expect(
      within(toolbar).getByRole("button", {
        name: "切换界面语言为 English",
      }),
    ).toBeInTheDocument();
    const memberEntry = within(toolbar).getByRole("button", {
      name: "未登录 / 点击登录",
    });
    expect(memberEntry).toHaveTextContent("登录");

    await user.click(memberEntry);
    const menu = screen.getByRole("menu", { name: "会员中心" });
    expect(
      within(menu).getByRole("button", { name: "刷新状态" }),
    ).toHaveAttribute("title", "刷新状态");
    expect(
      within(menu).getByRole("button", { name: "智能体状态" }),
    ).toHaveAttribute("title", "查看智能体状态");
    expect(
      within(menu).getByRole("button", { name: "模型供应商与大模型" }),
    ).toHaveAttribute("title", "管理模型供应商与大模型");

    await user.click(within(menu).getByRole("button", { name: /^设置/ }));
    await user.click(screen.getByRole("button", { name: "切换界面语言为 English" }));
    expect(window.localStorage.getItem("at-switch-language")).toBe("en");
    expect(
      await screen.findByRole("heading", { name: "Advanced settings" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("高级设置")).not.toBeInTheDocument();

    const englishToolbar = screen.getByRole("navigation", {
      name: "Toolbar navigation",
    });
    expect(
      within(englishToolbar).getByRole("button", {
        name: "Switch language to 简体中文",
      }),
    ).toBeInTheDocument();
    expect(
      within(englishToolbar).queryByRole("button", { name: "Agent status" }),
    ).not.toBeInTheDocument();

    await openMemberMenu(user, "Not signed in / Click to sign in");
    const englishMenu = screen.getByRole("menu", { name: "Member center" });
    expect(
      within(englishMenu).getByRole("button", { name: "Refresh status" }),
    ).toHaveAttribute("title", "Refresh status");
    expect(
      within(englishMenu).getByRole("button", { name: "Agent status" }),
    ).toHaveAttribute("title", "View agent status");
    expect(
      within(englishMenu).getByRole("button", {
        name: "Model providers & LLMs",
      }),
    ).toHaveAttribute("title", "Manage model providers & LLMs");
    await user.keyboard("{Escape}");

    await user.click(screen.getByRole("button", { name: "Go back" }));
    expect(
      await screen.findByRole("heading", { name: "WorkBuddy" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Current route")).toBeInTheDocument();

    await openMemberMenu(user, "Not signed in / Click to sign in");
    await user.click(screen.getByRole("button", { name: "Agent status" }));
    expect(
      await screen.findByRole("heading", { name: "Agents" }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Installation").length).toBeGreaterThan(0);

    await openMemberMenu(user, "Not signed in / Click to sign in");
    await user.click(
      screen.getByRole("button", { name: "Model providers & LLMs" }),
    );
    expect(
      await screen.findByRole("heading", { name: "Model providers & LLMs" }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Model catalog").length).toBeGreaterThan(0);
    expect(
      screen.getByRole("button", { name: "New model provider" }),
    ).toHaveClass("provider-create-action");

    await user.click(screen.getByRole("button", { name: "Switch language to 简体中文" }));
    await waitFor(() => {
      expect(window.localStorage.getItem("at-switch-language")).toBe("zh-CN");
    });
  });

  it("shows the signed-in member name next to the member entry icon", async () => {
    vi.spyOn(api, "getAccountStatus").mockResolvedValue({
      configured: true,
      loggedIn: true,
      offline: false,
      frozen: false,
      profile: {
        userId: "1",
        username: "tester",
        displayName: "杰王",
        email: "tester@example.test",
        providerType: "google",
      },
    });
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    const entry = await screen.findByTitle("杰王");
    expect(within(entry).getByText("杰王")).toBeInTheDocument();
  });

  it(
    "member center supports login entry, settings migration and confirmed sign-out",
    async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });

    // 未登录：下拉菜单显示未登录入口，点击弹出登录弹窗（微信 / 谷歌）。
    await user.click(screen.getByRole("button", { name: "未登录 / 点击登录" }));
    const loginHeader = await screen.findByText("未登录 · 点击登录");
    expect(loginHeader).toBeInTheDocument();
    await user.click(loginHeader.closest("button")!);
    expect(
      await screen.findByRole("button", { name: "微信扫码登录" }),
    ).toBeEnabled();
    expect(screen.getByRole("button", { name: /谷歌账号登录/ })).toBeEnabled();

    // 未登录时菜单不显示退出登录；设置可打开原设置页。
    await user.click(screen.getByRole("button", { name: "关闭" }));
    await user.click(screen.getByRole("button", { name: "未登录 / 点击登录" }));
    expect(screen.queryByText("退出登录")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /^设置/ }));
    expect(
      await screen.findByRole("heading", { name: "高级设置" }),
    ).toBeInTheDocument();

    // 登录：顶栏图标打开菜单 → 菜单头点击 → 登录弹窗 → 微信 mock 登录成功。
    await user.click(await screen.findByRole("button", { name: "未登录 / 点击登录" }));
    await user.click(await screen.findByText("未登录 · 点击登录"));
    await user.click(await screen.findByRole("button", { name: "微信扫码登录" }));
    // mock 轮询含 2s 真实延时，放宽等待窗口；按钮为纯图标，用 title 定位。
    await screen.findByTitle("预览用户", {}, { timeout: 6000 });
    const signedInEntry = screen.getByTitle("预览用户");
    expect(within(signedInEntry).getByText("预览用户")).toBeInTheDocument();
    await user.click(signedInEntry);
    await user.click(screen.getByRole("button", { name: "退出登录" }));
    expect(
      await screen.findByText(
        "退出后回到未登录态；本机已有的供应商与模型配置不会被清空。",
      ),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "确认退出" }));
    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: "退出登录" }),
      ).not.toBeInTheDocument();
    });
    },
    20000,
  );

  it("returns through page history from the top-left back button", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await openMemberMenu(user);
    await user.click(screen.getByRole("button", { name: "智能体状态" }));
    await screen.findByRole("heading", { name: "智能体" });
    await openMemberMenu(user);
    await user.click(
      screen.getByRole("button", { name: "模型供应商与大模型" }),
    );
    await screen.findByRole("heading", { name: "模型供应商与大模型" });

    const back = screen.getByRole("button", { name: "返回上一页" });
    expect(back).toHaveAttribute("title", "返回上一页");
    await user.click(back);
    expect(
      await screen.findByRole("heading", { name: "智能体" }),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "返回上一页" }));
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
      await user.click(screen.getByRole("tab", { name: agentName }));
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
    await openMemberMenu(user);
    await user.click(screen.getByRole("button", { name: "智能体状态" }));

    const detailButtons = await screen.findAllByRole("button", {
      name: "详情",
    });
    expect(detailButtons).toHaveLength(11);
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
    await openMemberMenu(user);
    await user.click(
      screen.getByRole("button", { name: "模型供应商与大模型" }),
    );
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
    await user.click(within(modelRow!).getByRole("button", { name: "测试 DeepSeek" }));

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
    await user.click(screen.getByRole("tab", { name: "Codex" }));
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
    await user.click(screen.getByRole("tab", { name: "CodeBuddy" }));
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

  it("moves local proxy configuration behind Advanced settings", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    expect(
      screen.queryByRole("button", { name: "本地代理" }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "未登录 / 点击登录" }));
    await user.click(await screen.findByRole("button", { name: /^设置/ }));
    expect(
      await screen.findByRole("heading", { name: "高级设置" }),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "打开本地代理设置" }),
    );
    expect(
      await screen.findByRole("heading", { name: "本地代理" }),
    ).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "配置 WorkBuddy 本地代理" }),
    );
    expect(
      screen.getByRole("heading", { name: "本地代理配置 WorkBuddy" }),
    ).toBeInTheDocument();
    expect(screen.getByText("本地代理（高级）")).toBeInTheDocument();
    expect(
      screen.queryByRole("radio", { name: /Agent 直连/ }),
    ).not.toBeInTheDocument();
  });

  it("confirms and automatically restarts QClaw when switching", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    await user.click(screen.getByRole("tab", { name: "QClaw" }));
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
    await user.click(screen.getByRole("tab", { name: "AutoClaw" }));
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
    await user.click(screen.getByRole("tab", { name: "百度搭子" }));
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
    const codexTab = screen.getByRole("tab", { name: "Codex" });
    await user.click(codexTab);

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
    await openMemberMenu(user);
    await user.click(
      screen.getByRole("button", { name: "模型供应商与大模型" }),
    );
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
    await openMemberMenu(user);
    await user.click(
      screen.getByRole("button", { name: "模型供应商与大模型" }),
    );
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

  it("switches language directly from the standalone header language button", async () => {
    const user = userEvent.setup();
    render(<App />);

    await screen.findByRole("heading", { name: "WorkBuddy" });
    const switchButton = screen.getByRole("button", {
      name: "切换界面语言为 English",
    });
    await user.click(switchButton);

    await waitFor(() => {
      expect(window.localStorage.getItem("at-switch-language")).toBe("en");
    });
    expect(
      await screen.findByRole("button", { name: "Switch language to 简体中文" }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Default configuration").length).toBeGreaterThan(0);

    await user.click(
      screen.getByRole("button", { name: "Switch language to 简体中文" }),
    );
    await waitFor(() => {
      expect(window.localStorage.getItem("at-switch-language")).toBe("zh-CN");
    });
    expect(
      await screen.findByRole("button", { name: "切换界面语言为 English" }),
    ).toBeInTheDocument();
    await api.updateSettings({ language: "zh-CN" });
    api.resetMock();
  });
});
