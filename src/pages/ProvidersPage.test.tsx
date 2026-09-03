import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { ProviderSummary } from "../types";
import { ProvidersPage } from "./ProvidersPage";

const provider: ProviderSummary = {
  id: "provider-test",
  name: "测试供应商",
  kind: "custom",
  protocol: "openai_chat_completions",
  baseUrl: "https://api.example.test/v1",
  isRecommended: false,
  isEnabled: true,
  hasApiKey: true,
  maskedApiKey: "••••test",
  verificationStatus: "verified",
  verifiedModelId: "text-model",
  defaultModelId: "text-model",
  models: [
    {
      id: "provider-test:text-model",
      providerId: "provider-test",
      modelId: "text-model",
      displayName: "Text Model",
      outputModality: "text",
      supportsStreaming: true,
      supportsTools: true,
      source: "custom",
      verificationStatus: "verified",
    },
    {
      id: "provider-test:image-model",
      providerId: "provider-test",
      modelId: "image-model",
      displayName: "Image Model",
      outputModality: "image",
      supportsStreaming: false,
      supportsTools: false,
      source: "custom",
      verificationStatus: "draft_unverified",
    },
  ],
};

describe("ProvidersPage", () => {
  it("counts and tests only text models", async () => {
    const user = userEvent.setup();
    const onTest = vi.fn();
    render(
      <ProvidersPage
        providers={[provider]}
        onCreate={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onTest={onTest}
      />,
    );

    expect(screen.getByText("已验证文本模型")).toBeInTheDocument();
    expect(screen.getByText("1/1")).toBeInTheDocument();
    expect(screen.queryByText("1/2")).not.toBeInTheDocument();
  });

  it("triggers onDelete when delete button is clicked", async () => {
    const user = userEvent.setup();
    const onDelete = vi.fn();
    render(
      <ProvidersPage
        providers={[provider]}
        onCreate={vi.fn()}
        onEdit={vi.fn()}
        onDelete={onDelete}
        onTest={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: "删除 测试供应商" }));
    expect(onDelete).toHaveBeenCalledWith(provider);
  });

  it("hides verification status and actions for a non-text-only provider", () => {
    const mediaProvider = {
      ...provider,
      id: "provider-media",
      name: "媒体供应商",
      verificationStatus: "draft_unverified" as const,
      verifiedModelId: undefined,
      defaultModelId: "image-model",
      models: [provider.models[1]!],
    };
    render(
      <ProvidersPage
        providers={[mediaProvider]}
        onCreate={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onTest={vi.fn()}
      />,
    );

    const card = screen.getByRole("heading", { name: "媒体供应商" }).closest("article");
    expect(card).not.toBeNull();
    expect(within(card!).queryByText("未验证")).not.toBeInTheDocument();
    expect(within(card!).queryByText("已验证文本模型")).not.toBeInTheDocument();
    expect(
      within(card!).queryByRole("button", { name: "连接测试" }),
    ).not.toBeInTheDocument();
  });
});
