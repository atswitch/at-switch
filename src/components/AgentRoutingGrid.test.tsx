import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentSummary } from "../types";
import { AgentRoutingGrid } from "./AgentRoutingGrid";
import { buildAgentUsage } from "../lib/proxyUsage";

const agents: AgentSummary[] = [
  {
    id: "workbuddy",
    displayName: "WorkBuddy",
    installStatus: "installed",
    runtimeStatus: "not_running",
    configHealth: "healthy",
    adapterVerified: true,
    proxyPrefEnabled: true,
    providerName: "蒙云智算",
    providerId: "mongyun",
    modelId: "glm-5.2",
    needsRestart: false,
    automaticRestartSupported: false,
  },
  {
    id: "codebuddy",
    displayName: "CodeBuddy",
    installStatus: "installed",
    runtimeStatus: "not_running",
    configHealth: "healthy",
    adapterVerified: true,
    needsRestart: false,
    automaticRestartSupported: false,
  },
  {
    id: "hermes",
    displayName: "Hermes",
    installStatus: "not_installed",
    runtimeStatus: "unknown",
    configHealth: "healthy",
    adapterVerified: true,
    needsRestart: false,
    automaticRestartSupported: false,
  },
];

function renderGrid(overrides: Partial<Parameters<typeof AgentRoutingGrid>[0]> = {}) {
  const props = {
    agents,
    usageByAgent: [],
    onToggleProxyPref: vi.fn(),
    onConfigureProxy: vi.fn(),
    ...overrides,
  };
  const utilities = render(<AgentRoutingGrid {...props} />);
  return { ...utilities, ...props };
}

function cardOf(name: string): HTMLElement {
  const card = screen
    .getAllByText(name)
    .map((node) => node.closest(".agent-card"))
    .find((node): node is HTMLElement => node !== null);
  if (!card) throw new Error(`agent card not found: ${name}`);
  return card;
}

beforeEach(() => {
  window.localStorage.setItem("at-switch-language", "zh-CN");
});

describe("AgentRoutingGrid", () => {
  it("renders one card per agent inside the routing grid", () => {
    const { container } = renderGrid();

    expect(container.querySelectorAll(".agent-card")).toHaveLength(3);
    const grids = container.querySelectorAll(".agent-routing-grid");
    // 已启用 / 未启用 / 暂不可用各占一个网格。
    expect(grids).toHaveLength(3);
    expect(within(cardOf("WorkBuddy")).getByText("已启用")).toBeInTheDocument();
    expect(within(cardOf("CodeBuddy")).getByText("未启用")).toBeInTheDocument();
    expect(within(cardOf("Hermes")).getByText("暂不可用")).toBeInTheDocument();
  });

  it("shows the current model and routing hint per card", () => {
    renderGrid();

    expect(
      within(cardOf("WorkBuddy")).getByText("蒙云智算 · glm-5.2"),
    ).toBeInTheDocument();
    expect(
      within(cardOf("CodeBuddy")).getByText("未启用 · 直连默认配置"),
    ).toBeInTheDocument();
  });

  it("toggles telemetry through the card switch", async () => {
    const user = userEvent.setup();
    const { onToggleProxyPref } = renderGrid();

    await user.click(
      within(cardOf("CodeBuddy")).getByRole("checkbox", {
        name: /CodeBuddy/,
      }),
    );

    expect(onToggleProxyPref).toHaveBeenCalledWith(
      agents[1],
      true,
    );

    await user.click(
      within(cardOf("WorkBuddy")).getByRole("checkbox", { name: /WorkBuddy/ }),
    );
    expect(onToggleProxyPref).toHaveBeenCalledWith(agents[0], false);
  });

  it("keeps unavailable agents read-only and without proxy configuration", () => {
    renderGrid();

    const hermes = cardOf("Hermes");
    expect(hermes.querySelector("input[type=checkbox]")).toBeDisabled();
    expect(
      within(hermes).queryByRole("button", { name: "本地代理配置" }),
    ).not.toBeInTheDocument();
    expect(within(hermes).getByText("该智能体尚未安装")).toBeInTheDocument();
  });

  it("opens the proxy configuration sheet from the card action", async () => {
    const user = userEvent.setup();
    const { onConfigureProxy } = renderGrid();

    await user.click(
      within(cardOf("WorkBuddy")).getByRole("button", { name: "本地代理配置" }),
    );

    expect(onConfigureProxy).toHaveBeenCalledWith(agents[0]);
  });

  it("renders usage counters when the agent is routed", () => {
    const usageByAgent = buildAgentUsage(
      [agents[0]!],
      ["workbuddy"],
      [
        {
          at: "2026-01-01T00:00:00.000Z",
          agentId: "workbuddy",
          providerId: "mongyun",
          providerName: "蒙云智算",
          model: "glm-5.2",
          status: 200,
          inputTokens: 12_000,
          outputTokens: 8_000,
        },
      ],
    );
    renderGrid({ usageByAgent });

    const card = cardOf("WorkBuddy");
    expect(within(card).getByText("1")).toBeInTheDocument();
    expect(within(card).getByText("12K")).toBeInTheDocument();
    expect(within(card).getByText("8.0K")).toBeInTheDocument();
    expect(within(card).getByText("100%")).toBeInTheDocument();
  });

  it("reports an empty catalog in a single hint", () => {
    renderGrid({ agents: [] });

    expect(screen.getByText("尚未发现任何智能体。")).toBeInTheDocument();
  });
});
