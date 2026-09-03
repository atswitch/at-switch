import React from "react";
import type { ProviderSummary } from "../types";

import deepseekImg from "../assets/providers/deepseek.png";
import doubaoImg from "../assets/providers/doubao.png";
import kimiImg from "../assets/providers/kimi.png";
import minimaxImg from "../assets/providers/minimax.png";
import mongyunImg from "../assets/providers/mongyun.png";
import qwenImg from "../assets/providers/qwen.svg";
import zhipuImg from "../assets/providers/zhipu.svg";

interface ProviderLogoProps {
  provider: Pick<ProviderSummary, "kind" | "name"> & { isRecommended?: boolean };
  size?: "card" | "row";
  className?: string;
  "aria-hidden"?: boolean | "true" | "false";
}

export function getProviderBrandKey(provider: Pick<ProviderSummary, "kind" | "name">): string {
  const kind = (provider.kind || "").toLowerCase();
  const name = (provider.name || "").toLowerCase();

  if (kind === "deepseek" || name.includes("deepseek")) return "deepseek";
  if (kind === "minimax" || name.includes("minimax")) return "minimax";
  if (kind === "kimi" || name.includes("kimi") || name.includes("moonshot")) return "kimi";
  if (kind === "zhipu" || name.includes("zhipu") || name.includes("智谱") || name.includes("glm")) return "zhipu";
  if (kind === "mongyun" || name.includes("蒙云")) return "mongyun";
  if (kind === "qwen" || name.includes("qwen") || name.includes("通义")) return "qwen";
  if (kind === "doubao" || name.includes("doubao") || name.includes("豆包")) return "doubao";
  if (name.includes("openai") || name.includes("chatgpt")) return "openai";
  if (name.includes("anthropic") || name.includes("claude")) return "anthropic";
  if (name.includes("silicon") || name.includes("硅基")) return "siliconflow";
  if (name.includes("ollama")) return "ollama";
  if (name.includes("gemini") || name.includes("google")) return "gemini";

  return "custom";
}

const officialLogos: Record<string, string> = {
  deepseek: deepseekImg,
  mongyun: mongyunImg,
  minimax: minimaxImg,
  kimi: kimiImg,
  zhipu: zhipuImg,
  qwen: qwenImg,
  doubao: doubaoImg,
};

export function ProviderLogo({
  provider,
  size = "row",
  className = "",
  "aria-hidden": ariaHidden,
}: ProviderLogoProps) {
  const brandKey = getProviderBrandKey(provider);
  const containerClass = `provider-logo provider-logo--${size} provider-logo--${brandKey} ${className}`;
  const isHidden = ariaHidden === true || ariaHidden === "true";
  const ariaProps = isHidden
    ? { "aria-hidden": "true" as const }
    : { title: provider.name, "aria-label": provider.name };

  const officialSrc = officialLogos[brandKey];

  if (officialSrc) {
    return (
      <div className={containerClass} {...ariaProps}>
        <img src={officialSrc} alt={provider.name} className="provider-logo__img" />
      </div>
    );
  }

  const char = (provider.name || "P").slice(0, 1).toUpperCase();
  return (
    <div className={containerClass} {...ariaProps}>
      <span className="provider-logo__monogram">{char}</span>
    </div>
  );
}
