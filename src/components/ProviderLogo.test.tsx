import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProviderLogo, getProviderBrandKey } from "./ProviderLogo";

describe("ProviderLogo", () => {
  it("resolves provider brand key accurately", () => {
    expect(getProviderBrandKey({ kind: "deepseek", name: "DeepSeek" })).toBe("deepseek");
    expect(getProviderBrandKey({ kind: "custom", name: "DeepSeek-V4" })).toBe("deepseek");
    expect(getProviderBrandKey({ kind: "minimax", name: "MiniMax" })).toBe("minimax");
    expect(getProviderBrandKey({ kind: "kimi", name: "Kimi" })).toBe("kimi");
    expect(getProviderBrandKey({ kind: "zhipu", name: "智谱" })).toBe("zhipu");
    expect(getProviderBrandKey({ kind: "mongyun", name: "蒙云智算" })).toBe("mongyun");
    expect(getProviderBrandKey({ kind: "custom", name: "My Local Ollama" })).toBe("ollama");
    expect(getProviderBrandKey({ kind: "custom", name: "Custom Provider" })).toBe("custom");
  });

  it("renders official brand logo image for recognized providers", () => {
    const { container } = render(
      <ProviderLogo provider={{ kind: "deepseek", name: "DeepSeek" }} size="card" />,
    );
    expect(container.querySelector("img")).toBeInTheDocument();
    expect(container.querySelector(".provider-logo--deepseek")).toBeInTheDocument();
  });

  it("renders the packaged Mongyun brand asset", () => {
    const { container } = render(
      <ProviderLogo provider={{ kind: "mongyun", name: "蒙云智算" }} size="row" />,
    );
    expect(container.querySelector("img")).toHaveAttribute(
      "src",
      expect.stringContaining("mongyun.png"),
    );
  });

  it("renders the packaged Zhipu brand asset", () => {
    const { container } = render(
      <ProviderLogo provider={{ kind: "zhipu", name: "智谱" }} size="row" />,
    );
    expect(container.querySelector("img")).toHaveAttribute(
      "src",
      expect.stringContaining("zhipu.svg"),
    );
  });

  it("renders the packaged Qwen brand asset", () => {
    const { container } = render(
      <ProviderLogo provider={{ kind: "qwen", name: "千问" }} size="row" />,
    );
    expect(container.querySelector("img")).toHaveAttribute(
      "src",
      expect.stringContaining("qwen.svg"),
    );
  });

  it("renders monogram fallback for unknown custom providers", () => {
    render(
      <ProviderLogo provider={{ kind: "custom", name: "Custom AI" }} size="row" />,
    );
    expect(screen.getByText("C")).toBeInTheDocument();
  });
});
