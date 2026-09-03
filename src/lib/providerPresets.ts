import type {
  ApiProtocol,
  AppLanguage,
  ModelOutputModality,
  ProviderKind,
} from "../types";

export type ProviderPresetKind = Exclude<ProviderKind, "custom">;

export interface ProviderPresetModel {
  modelId: string;
  displayName: string;
  displayNameZh?: string;
  displayNameEn?: string;
  outputModality: ModelOutputModality;
  supportsStreaming: boolean;
  supportsTools: boolean;
}

export interface ProviderPreset {
  kind: ProviderPresetKind;
  baseUrl: string;
  protocol: ApiProtocol;
  models: readonly ProviderPresetModel[];
}

/**
 * Shared display order for the preset picker and the switchboard. Custom
 * providers are not part of the preset catalog and are displayed afterwards.
 */
export const providerPresetDisplayOrder: readonly ProviderPresetKind[] = [
  "deepseek",
  "zhipu",
  "mongyun",
  "minimax",
  "kimi",
  "qwen",
  "doubao",
];

const providerPresetDisplayRanks = new Map<ProviderKind, number>(
  providerPresetDisplayOrder.map((kind, index) => [kind, index]),
);

export function providerPresetDisplayRank(kind: ProviderKind): number {
  return (
    providerPresetDisplayRanks.get(kind) ?? providerPresetDisplayOrder.length
  );
}

function textModel(
  modelId: string,
  displayName: string,
): ProviderPresetModel {
  return {
    modelId,
    displayName,
    outputModality: "text",
    supportsStreaming: true,
    supportsTools: true,
  };
}

function imageModel(
  modelId: string,
  displayName: string,
): ProviderPresetModel {
  return {
    modelId,
    displayName,
    outputModality: "image",
    supportsStreaming: false,
    supportsTools: false,
  };
}

/**
 * Curated defaults are intentionally suggestions rather than an allowlist.
 * Provider APIs evolve, so every Base URL and model ID remains editable in the form.
 */
export const providerPresets: Record<ProviderPresetKind, ProviderPreset> = {
  mongyun: {
    kind: "mongyun",
    baseUrl: "https://api.g2claw.com/v1",
    protocol: "openai_chat_completions",
    models: [
      textModel("NMauto", "NMauto"),
      textModel("deepseek-v4-flash", "DeepSeek V4 Flash"),
      textModel("GLM-5.2", "GLM-5.2"),
      imageModel("doubao-seedream-5.0-lite", "Doubao Seedream 5.0 Lite"),
    ],
  },
  deepseek: {
    kind: "deepseek",
    baseUrl: "https://api.deepseek.com/v1",
    protocol: "openai_chat_completions",
    models: [
      textModel("deepseek-v4-flash", "DeepSeek V4 Flash"),
      textModel("deepseek-v4-pro", "DeepSeek V4 Pro"),
    ],
  },
  minimax: {
    kind: "minimax",
    baseUrl: "https://api.minimaxi.com/v1",
    protocol: "openai_chat_completions",
    models: [
      textModel("MiniMax-M3", "MiniMax M3"),
      textModel("MiniMax-M2.7", "MiniMax M2.7"),
    ],
  },
  kimi: {
    kind: "kimi",
    baseUrl: "https://api.moonshot.ai/v1",
    protocol: "openai_chat_completions",
    models: [
      textModel("kimi-k3", "Kimi K3"),
      textModel("kimi-k2.6", "Kimi K2.6"),
      textModel("kimi-k2.5", "Kimi K2.5"),
    ],
  },
  zhipu: {
    kind: "zhipu",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    protocol: "openai_chat_completions",
    models: [
      textModel("GLM-5.2", "GLM-5.2"),
      textModel("GLM-5.1", "GLM-5.1"),
    ],
  },
  qwen: {
    kind: "qwen",
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    protocol: "openai_chat_completions",
    models: [textModel("qwen3.8-max", "Qwen 3.8 Max")],
  },
  doubao: {
    kind: "doubao",
    baseUrl: "https://ark.cn-beijing.volces.com/api/v3",
    protocol: "openai_chat_completions",
    models: [
      textModel("doubao-seed-2-0-pro-260215", "Doubao Seed 2.0 Pro"),
      textModel("doubao-seed-2-0-lite-260215", "Doubao Seed 2.0 Lite"),
    ],
  },
};

export function providerPresetForKind(
  kind: ProviderKind,
): ProviderPreset | undefined {
  return kind === "custom" ? undefined : providerPresets[kind];
}

export function presetModelDisplayName(
  model: ProviderPresetModel,
  language: AppLanguage,
) {
  if (language === "zh-CN") {
    return model.displayNameZh ?? model.displayName;
  }
  return model.displayNameEn ?? model.displayName;
}
