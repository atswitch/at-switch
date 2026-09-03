import { describe, expect, it } from "vitest";
import {
  providerPresetDisplayOrder,
  providerPresetDisplayRank,
  providerPresets,
} from "./providerPresets";

describe("provider presets", () => {
  it("defines one fixed display order and places custom providers last", () => {
    expect(providerPresetDisplayOrder).toEqual([
      "deepseek",
      "zhipu",
      "mongyun",
      "minimax",
      "kimi",
      "qwen",
      "doubao",
    ]);
    expect(providerPresetDisplayRank("custom")).toBe(
      providerPresetDisplayOrder.length,
    );
  });

  it("uses curated official direct endpoints and common model IDs", () => {
    expect(providerPresets.deepseek).toMatchObject({
      baseUrl: "https://api.deepseek.com/v1",
      models: [
        { modelId: "deepseek-v4-flash" },
        { modelId: "deepseek-v4-pro" },
      ],
    });
    expect(providerPresets.minimax.models.map((model) => model.modelId)).toEqual([
      "MiniMax-M3",
      "MiniMax-M2.7",
    ]);
    expect(providerPresets.kimi.models.map((model) => model.modelId)).toEqual([
      "kimi-k3",
      "kimi-k2.6",
      "kimi-k2.5",
    ]);
    expect(providerPresets.zhipu).toMatchObject({
      baseUrl: "https://open.bigmodel.cn/api/paas/v4",
      models: [
        { modelId: "GLM-5.2" },
        { modelId: "GLM-5.1" },
      ],
    });
    expect(providerPresets.qwen.baseUrl).toBe(
      "https://dashscope.aliyuncs.com/compatible-mode/v1",
    );
    expect(providerPresets.qwen.models.map((model) => model.modelId)).toEqual([
      "qwen3.8-max",
    ]);
    expect(providerPresets.doubao.baseUrl).toBe(
      "https://ark.cn-beijing.volces.com/api/v3",
    );
    expect(providerPresets.doubao.models.map((model) => model.modelId)).toEqual([
      "doubao-seed-2-0-pro-260215",
      "doubao-seed-2-0-lite-260215",
    ]);
  });

  it("keeps the Mongyun catalog in the product-defined display order", () => {
    expect(providerPresets.mongyun.models.map((model) => model.modelId)).toEqual([
      "NMauto",
      "deepseek-v4-flash",
      "GLM-5.2",
      "doubao-seedream-5.0-lite",
    ]);
    const seedream = providerPresets.mongyun.models.find(
      (model) => model.modelId === "doubao-seedream-5.0-lite",
    );
    expect(seedream).toMatchObject({
      outputModality: "image",
      supportsStreaming: false,
      supportsTools: false,
    });
  });
});
