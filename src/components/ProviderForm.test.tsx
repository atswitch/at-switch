import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { LanguageProvider } from "../i18n";
import type { ProviderSummary } from "../types";
import { ProviderForm } from "./ProviderForm";

const provider: ProviderSummary = {
  id: "provider-test",
  name: "Test Provider",
  kind: "custom",
  protocol: "openai_chat_completions",
  baseUrl: "https://api.example.test/v1",
  isRecommended: false,
  isEnabled: true,
  hasApiKey: true,
  maskedApiKey: "••••test",
  verificationStatus: "stale",
  defaultModelId: "model-a",
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
      verificationStatus: "stale",
    },
    {
      id: "provider-test:model-b",
      providerId: "provider-test",
      modelId: "model-b",
      displayName: "Model B",
      outputModality: "text",
      supportsStreaming: true,
      supportsTools: false,
      source: "custom",
      verificationStatus: "stale",
    },
  ],
};

describe("ProviderForm", () => {
  it("marks a missing API key in red", () => {
    render(<ProviderForm onSubmit={vi.fn()} busy={false} />);

    const warning = screen.getByRole("alert");
    expect(warning).toHaveTextContent("请输入 API Key");
    expect(warning.closest("label")).toHaveClass("field--error");
  });

  it("uses an equal-length mask and reveals the full key only on request", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    const fullApiKey = "sk-full-test-1234";
    const equalLengthMask = `${"•".repeat(fullApiKey.length - 4)}1234`;
    render(
      <ProviderForm
        initialProvider={provider}
        loadMaskedApiKey={async () => equalLengthMask}
        revealApiKey={async () => fullApiKey}
        onSubmit={onSubmit}
        busy={false}
      />,
    );

    const apiKey = screen.getByLabelText(/^API Key/);
    await waitFor(() => expect(apiKey).toHaveValue(equalLengthMask));
    expect(apiKey).toHaveAttribute("type", "password");
    await user.click(screen.getByRole("button", { name: "查看 API Key" }));
    await waitFor(() => {
      expect(apiKey).toHaveAttribute("type", "text");
      expect(apiKey).toHaveValue(fullApiKey);
    });
    await user.click(screen.getByRole("button", { name: "隐藏 API Key" }));
    expect(apiKey).toHaveAttribute("type", "password");
    expect(apiKey).toHaveValue(equalLengthMask);
    await user.click(screen.getByRole("button", { name: "保存模型供应商" }));
    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({ apiKey: undefined }),
    );
  });

  it("accepts a manually typed Provider and exposes Kimi and Zhipu presets", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(<ProviderForm onSubmit={onSubmit} busy={false} />);

    await user.click(screen.getByRole("button", { name: "切换预设下拉" }));
    expect(screen.getByRole("option", { name: "Kimi" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "智谱" })).toBeInTheDocument();

    const nameInput = screen.getByPlaceholderText(
      "选择预设，或直接输入模型供应商名称",
    );
    await user.clear(nameInput);
    await user.type(nameInput, "OpenRouter");
    await user.type(
      screen.getByPlaceholderText("https://api.example.com/v1"),
      "https://openrouter.ai/api/v1",
    );
    await user.type(
      screen.getByPlaceholderText("保存到系统凭据库；请求时发送给上游"),
      "test-key",
    );
    await user.type(screen.getByPlaceholderText("模型 ID"), "test-model");
    await user.click(screen.getByRole("button", { name: "保存模型供应商" }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "custom", name: "OpenRouter" }),
    );
  });

  it("describes Mongyun multi-protocol routing with Chat as the fallback", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <ProviderForm
        initialKind="mongyun"
        onSubmit={onSubmit}
        busy={false}
      />,
    );

    expect(
      screen.getByRole("combobox", { name: /^默认 \/ 回退 API 协议/ }),
    ).toHaveValue("openai_chat_completions");
    expect(
      screen.getByText(/内置支持 OpenAI Chat 和 OpenAI Responses/),
    ).toBeInTheDocument();
    expect(
      screen.getByPlaceholderText("https://api.example.com/v1"),
    ).toHaveValue("https://api.g2claw.com/v1");
    expect(
      screen.getAllByPlaceholderText("模型 ID").map((input) => input.getAttribute("value")),
    ).toEqual([
      "NMauto",
      "deepseek-v4-flash",
      "GLM-5.2",
      "doubao-seedream-5.0-lite",
    ]);
    expect(screen.getByText("模型 ID")).toBeInTheDocument();
    expect(screen.getByText("模型名称")).toBeInTheDocument();
    expect(
      screen
        .getAllByRole("combobox", { name: /模型能力/ })
        .map((select) => (select as HTMLSelectElement).value),
    ).toEqual(["text", "text", "text", "image"]);
    expect(
      screen.getAllByRole("tooltip", {
        name: "文本模型；支持流式输出，支持工具调用。",
      }),
    ).toHaveLength(3);
    await user.type(
      screen.getByPlaceholderText("保存到系统凭据库；请求时发送给上游"),
      "test-key",
    );
    await user.click(screen.getByRole("button", { name: "保存模型供应商" }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        kind: "mongyun",
        protocol: "openai_chat_completions",
        baseUrl: "https://api.g2claw.com/v1",
        models: expect.arrayContaining([
          expect.objectContaining({ modelId: "NMauto" }),
          expect.objectContaining({ modelId: "deepseek-v4-flash" }),
          expect.objectContaining({ modelId: "GLM-5.2" }),
          expect.objectContaining({ modelId: "doubao-seedream-5.0-lite" }),
        ]),
      }),
    );
  });

  it("fills official DeepSeek defaults and keeps Base URL and model IDs editable", async () => {
    const user = userEvent.setup();
    render(<ProviderForm onSubmit={vi.fn()} busy={false} />);

    await user.type(
      screen.getByPlaceholderText("选择预设，或直接输入模型供应商名称"),
      "DeepSeek",
    );

    const baseUrl = screen.getByPlaceholderText("https://api.example.com/v1");
    expect(baseUrl).toHaveValue("https://api.deepseek.com/v1");
    const modelIdInputs = screen.getAllByPlaceholderText("模型 ID");
    expect(modelIdInputs.map((input) => input.getAttribute("value"))).toEqual([
      "deepseek-v4-flash",
      "deepseek-v4-pro",
    ]);
    expect(modelIdInputs[0]).toHaveAttribute("list");
    expect(
      document.querySelector('option[value="deepseek-v4-flash"]'),
    ).toBeInTheDocument();

    await user.clear(baseUrl);
    await user.type(baseUrl, "https://gateway.example.test/v1");
    await user.clear(modelIdInputs[0]!);
    await user.type(modelIdInputs[0]!, "company-deepseek-route");

    expect(baseUrl).toHaveValue("https://gateway.example.test/v1");
    expect(modelIdInputs[0]).toHaveValue("company-deepseek-route");
    expect(screen.getAllByPlaceholderText("模型名称（可选）")[0]).toHaveValue("");
  });

  it("includes Kimi K3 in the Kimi preset", async () => {
    const user = userEvent.setup();
    render(<ProviderForm onSubmit={vi.fn()} busy={false} />);

    await user.type(
      screen.getByPlaceholderText("选择预设，或直接输入模型供应商名称"),
      "Kimi",
    );

    expect(
      screen.getByPlaceholderText("https://api.example.com/v1"),
    ).toHaveValue("https://api.moonshot.ai/v1");
    expect(
      screen.getAllByPlaceholderText("模型 ID").map((input) => input.getAttribute("value")),
    ).toEqual(["kimi-k3", "kimi-k2.6", "kimi-k2.5"]);
  });

  it("localizes model column labels", () => {
    window.localStorage.setItem("at-switch-language", "en");
    render(
      <LanguageProvider>
        <ProviderForm
          initialKind="mongyun"
          onSubmit={vi.fn()}
          busy={false}
        />
      </LanguageProvider>,
    );

    expect(screen.getByText("Model ID")).toBeInTheDocument();
    expect(screen.getByText("Model name")).toBeInTheDocument();
    window.localStorage.removeItem("at-switch-language");
  });

  it("does not overwrite an existing provider until a preset is reselected", () => {
    render(
      <ProviderForm
        initialProvider={{
          ...provider,
          kind: "deepseek",
          baseUrl: "https://deepseek-proxy.example.test/v1",
          models: [
            {
              ...provider.models[0]!,
              modelId: "deepseek-company-alias",
              displayName: "Company DeepSeek",
            },
          ],
        }}
        onSubmit={vi.fn()}
        busy={false}
      />,
    );

    expect(
      screen.getByPlaceholderText("https://api.example.com/v1"),
    ).toHaveValue("https://deepseek-proxy.example.test/v1");
    expect(screen.getByPlaceholderText("模型 ID")).toHaveValue(
      "deepseek-company-alias",
    );
  });

  it("fills an untouched built-in provider that has no endpoint or models yet", () => {
    render(
      <ProviderForm
        initialProvider={{
          ...provider,
          kind: "mongyun",
          name: "蒙云智算",
          baseUrl: "",
          hasApiKey: false,
          maskedApiKey: undefined,
          models: [],
        }}
        onSubmit={vi.fn()}
        busy={false}
      />,
    );

    expect(
      screen.getByPlaceholderText("https://api.example.com/v1"),
    ).toHaveValue("https://api.g2claw.com/v1");
    expect(screen.getAllByPlaceholderText("模型 ID")).toHaveLength(4);
  });

  it("edits the model catalog without exposing an Agent default selector", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <ProviderForm
        initialProvider={provider}
        onSubmit={onSubmit}
        busy={false}
      />,
    );

    expect(screen.queryByRole("radio")).not.toBeInTheDocument();
    expect(
      screen.getByText(/模型能力按输出类型展示/),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("checkbox", { name: "支持流式输出" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("checkbox", { name: "支持工具调用" }),
    ).not.toBeInTheDocument();
    expect(
      screen
        .getAllByRole("combobox", { name: /模型能力/ })
        .map((select) => (select as HTMLSelectElement).value),
    ).toEqual(["text", "text"]);
    expect(
      screen.getByRole("tooltip", {
        name: "文本模型；支持流式输出，不支持工具调用。",
      }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "保存模型供应商" }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        defaultModelId: "model-a",
        models: [
          expect.objectContaining({
            modelId: "model-a",
            outputModality: "text",
            supportsStreaming: true,
            supportsTools: true,
          }),
          expect.objectContaining({
            modelId: "model-b",
            outputModality: "text",
            supportsStreaming: true,
            supportsTools: false,
          }),
        ],
      }),
    );
  });

  it("defaults added models to text and lets users select another output type", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <ProviderForm
        initialProvider={provider}
        onSubmit={onSubmit}
        busy={false}
      />,
    );

    await user.click(screen.getByRole("button", { name: "添加模型" }));
    const capability = screen.getByRole("combobox", {
      name: "第 3 个模型能力",
    });
    expect(capability).toHaveValue("text");
    expect(
      Array.from((capability as HTMLSelectElement).options).map(
        (option) => option.value,
      ),
    ).toEqual(["text", "image", "audio", "video"]);

    await user.selectOptions(capability, "image");
    expect(capability).toHaveValue("image");
    expect(
      screen.getByRole("tooltip", { name: "图片生成模型。" }),
    ).toBeInTheDocument();
    await user.type(screen.getByLabelText("第 3 个模型 ID"), "image-model");
    await user.type(screen.getByLabelText("第 3 个模型名称"), "Image Model");
    await user.click(screen.getByRole("button", { name: "保存模型供应商" }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        models: expect.arrayContaining([
          expect.objectContaining({
            modelId: "image-model",
            outputModality: "image",
            supportsStreaming: false,
            supportsTools: false,
          }),
        ]),
      }),
    );
  });

  it("uses the first configured model only as the Provider-page test fallback", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    render(
      <ProviderForm
        initialProvider={provider}
        onSubmit={onSubmit}
        busy={false}
      />,
    );

    const modelIdInputs = screen.getAllByPlaceholderText("模型 ID");
    await user.clear(modelIdInputs[0]!);
    await user.type(modelIdInputs[0]!, "model-a-v2");
    await user.click(screen.getByRole("button", { name: "保存模型供应商" }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({ defaultModelId: "model-a-v2" }),
    );
  });
});
