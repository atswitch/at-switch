import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AgentSummary, ProviderSummary } from "../types";
import { AgentBindingForm } from "./AgentBindingForm";

const agent: AgentSummary = {
  id: "workbuddy",
  displayName: "WorkBuddy",
  installStatus: "installed",
  runtimeStatus: "not_running",
  configHealth: "healthy",
  adapterVerified: true,
  configPath: "/test/.workbuddy/models.json",
  needsRestart: false,
  automaticRestartSupported: false,
};

const provider: ProviderSummary = {
  id: "provider-test",
  name: "蒙云智算",
  kind: "mongyun",
  protocol: "openai_responses",
  baseUrl: "https://api.example.test/v1",
  isRecommended: false,
  isEnabled: true,
  hasApiKey: true,
  verificationStatus: "verified",
  verifiedModelId: "model-b",
  defaultModelId: "model-b",
  models: [
    {
      id: "provider-test:model-a",
      providerId: "provider-test",
      modelId: "model-a",
      displayName: "Model A",
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
      displayName: "Model B",
      outputModality: "text",
      supportsStreaming: true,
      supportsTools: true,
      source: "custom",
      verificationStatus: "verified",
    },
  ],
};

describe("AgentBindingForm", () => {
  it("keeps direct mode read-only and exposes the macOS installation action inline", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const onSelectInstallPath = vi.fn();
    render(
      <AgentBindingForm
        agent={{
          ...agent,
          providerId: provider.id,
          modelId: "model-b",
        }}
        providers={[provider]}
        mode="direct"
        busy={false}
        platform="macos"
        onSelectInstallPath={onSelectInstallPath}
        onSubmit={onSubmit}
      />,
    );

    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "应用到 WorkBuddy" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("蒙云智算")).toBeInTheDocument();
    expect(screen.getByText("Model B")).toBeInTheDocument();
    expect(screen.getByText("智能体直连（默认）")).toBeInTheDocument();
    expect(screen.getByText("/test/.workbuddy/models.json")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "选择安装位置" }));
    expect(onSelectInstallPath).toHaveBeenCalledOnce();
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("uses the Provider default model and submits proxy mode from Advanced settings", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <AgentBindingForm
        agent={agent}
        providers={[provider]}
        mode="proxy"
        busy={false}
        onSubmit={onSubmit}
      />,
    );

    expect(screen.getByText("本地代理（高级）")).toBeInTheDocument();
    expect(screen.getByText(/保存配置不会自动启动代理/)).toBeInTheDocument();
    expect(screen.queryByRole("radio")).not.toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "模型" })).toHaveValue(
      "model-b",
    );
    await user.click(screen.getByRole("button", { name: "应用到 WorkBuddy" }));
    expect(onSubmit).toHaveBeenCalledWith({
      agentId: "workbuddy",
      providerId: "provider-test",
      modelId: "model-b",
      mode: "proxy",
    });
  });

  it("does not offer unverified providers", () => {
    render(
      <AgentBindingForm
        agent={agent}
        providers={[
          {
            ...provider,
            verificationStatus: "stale",
            models: provider.models.map((model) => ({
              ...model,
              verificationStatus: "stale" as const,
            })),
          },
        ]}
        mode="proxy"
        busy={false}
        onSubmit={vi.fn()}
      />,
    );
    expect(
      screen.getByText("还没有可分发的模型供应商"),
    ).toBeInTheDocument();
  });

  it("offers verified historical models but hides a newly added unverified model", () => {
    render(
      <AgentBindingForm
        agent={agent}
        providers={[
          {
            ...provider,
            verificationStatus: "draft_unverified",
            models: [
              provider.models[0]!,
              {
                ...provider.models[1]!,
                modelId: "model-new",
                id: "provider-test:model-new",
                verificationStatus: "draft_unverified",
              },
            ],
          },
        ]}
        mode="proxy"
        busy={false}
        onSubmit={vi.fn()}
      />,
    );

    expect(screen.getByRole("option", { name: /Model A/ })).toBeInTheDocument();
    expect(
      screen.queryByRole("option", { name: /model-new/ }),
    ).not.toBeInTheDocument();
  });

  it("offers a non-text model without requiring verification", () => {
    const imageModel = {
      ...provider.models[0]!,
      id: "provider-test:image-model",
      modelId: "image-model",
      displayName: "Image Model",
      outputModality: "image" as const,
      verificationStatus: "draft_unverified" as const,
    };
    render(
      <AgentBindingForm
        agent={agent}
        providers={[
          {
            ...provider,
            verificationStatus: "draft_unverified",
            defaultModelId: imageModel.modelId,
            models: [imageModel],
          },
        ]}
        mode="proxy"
        busy={false}
        onSubmit={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("option", { name: /Image Model/ }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "应用到 WorkBuddy" }),
    ).toBeEnabled();
  });

  it("still requires an API key for a non-text model", () => {
    const imageModel = {
      ...provider.models[0]!,
      id: "provider-test:image-model",
      modelId: "image-model",
      outputModality: "image" as const,
      verificationStatus: "draft_unverified" as const,
    };
    render(
      <AgentBindingForm
        agent={agent}
        providers={[
          {
            ...provider,
            hasApiKey: false,
            verificationStatus: "draft_unverified",
            models: [imageModel],
          },
        ]}
        mode="proxy"
        busy={false}
        onSubmit={vi.fn()}
      />,
    );

    expect(
      screen.getByText("还没有可分发的模型供应商"),
    ).toBeInTheDocument();
  });
});
