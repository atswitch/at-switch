import {
  ChevronDown,
  ChevronRight,
  CircleHelp,
  Eye,
  EyeOff,
  Plus,
  Trash2,
} from "lucide-react";
import clsx from "clsx";
import { useEffect, useId, useRef, useState } from "react";
import { useLanguage } from "../i18n";
import type {
  ApiProtocol,
  ModelOutputModality,
  ProviderDraft,
  ProviderKind,
  ProviderSummary,
} from "../types";
import { protocolLabels } from "../lib/format";
import { ProviderLogo } from "./ProviderLogo";
import {
  presetModelDisplayName,
  providerPresetDisplayOrder,
  providerPresetForKind,
  type ProviderPreset,
  type ProviderPresetKind,
} from "../lib/providerPresets";

interface ProviderFormProps {
  initialKind?: ProviderKind;
  initialProvider?: ProviderSummary;
  loadMaskedApiKey?: (providerId: string) => Promise<string>;
  revealApiKey?: (providerId: string) => Promise<string>;
  onSubmit: (draft: ProviderDraft) => void;
  busy: boolean;
}

const kindLabels: Record<ProviderKind, string> = {
  deepseek: "DeepSeek",
  zhipu: "智谱",
  mongyun: "蒙云智算",
  minimax: "MiniMax",
  kimi: "Kimi",
  qwen: "通义千问",
  doubao: "豆包",
  custom: "自定义 Provider",
};

const kindLabelsEn: Record<ProviderKind, string> = {
  deepseek: "DeepSeek",
  zhipu: "Zhipu AI",
  mongyun: "Mongyun",
  minimax: "MiniMax",
  kimi: "Kimi",
  qwen: "Qwen",
  doubao: "Doubao",
  custom: "Custom provider",
};

const presetKinds: Array<[ProviderPresetKind, string]> =
  providerPresetDisplayOrder.map((kind) => [kind, kindLabels[kind]]);

function kindForInput(value: string): ProviderKind {
  const normalized = value.trim().toLocaleLowerCase();
  return (
    presetKinds.find(([kind, label]) =>
      [label, kindLabelsEn[kind]].some(
        (candidate) => candidate.toLocaleLowerCase() === normalized,
      ),
    )?.[0] ??
    "custom"
  );
}

function maskApiKey(value: string) {
  const characters = Array.from(value);
  if (characters.length <= 4) return "•".repeat(characters.length);
  return `${"•".repeat(characters.length - 4)}${characters.slice(-4).join("")}`;
}

type ModelFormRow = {
  rowId: string;
  modelId: string;
  displayName: string;
  outputModality: ModelOutputModality;
  supportsStreaming: boolean;
  supportsTools: boolean;
};

let nextModelRowId = 0;

function createModelRow(
  model?: Omit<ModelFormRow, "rowId">,
): ModelFormRow {
  nextModelRowId += 1;
  return {
    rowId: `model-row-${nextModelRowId}`,
    modelId: model?.modelId ?? "",
    displayName: model?.displayName ?? "",
    outputModality: model?.outputModality ?? "text",
    supportsStreaming: model?.supportsStreaming ?? true,
    supportsTools: model?.supportsTools ?? true,
  };
}

function createPresetModelRows(
  preset: ProviderPreset,
  language: "zh-CN" | "en",
) {
  return preset.models.map((model) =>
    createModelRow({
      modelId: model.modelId,
      displayName: presetModelDisplayName(model, language),
      outputModality: model.outputModality,
      supportsStreaming: model.supportsStreaming,
      supportsTools: model.supportsTools,
    }),
  );
}

export function ProviderForm({
  initialKind = "custom",
  initialProvider,
  loadMaskedApiKey,
  revealApiKey,
  onSubmit,
  busy,
}: ProviderFormProps) {
  const { language, text } = useLanguage();
  const labels = language === "zh-CN" ? kindLabels : kindLabelsEn;
  const resolvedKind = initialProvider?.kind ?? initialKind;
  const initialPreset =
    !initialProvider ||
    (!initialProvider.baseUrl.trim() && initialProvider.models.length === 0)
      ? providerPresetForKind(resolvedKind)
      : undefined;
  const modelPresetListId = useId();
  const [kind, setKind] = useState<ProviderKind>(resolvedKind);
  const [name, setName] = useState(
    initialProvider?.name ??
      (resolvedKind === "custom" ? "" : labels[resolvedKind]),
  );
  const [protocol, setProtocol] =
    useState<ApiProtocol>(
      initialProvider?.protocol ??
        initialPreset?.protocol ??
        "openai_chat_completions",
    );
  const [baseUrl, setBaseUrl] = useState(
    initialProvider?.baseUrl || initialPreset?.baseUrl || "",
  );
  const existingApiKeyDisplay = initialProvider?.hasApiKey
    ? (initialProvider.maskedApiKey ?? "••••••••")
    : "";
  const [apiKey, setApiKey] = useState(existingApiKeyDisplay);
  const [existingApiKeyMask, setExistingApiKeyMask] = useState(
    existingApiKeyDisplay,
  );
  const [apiKeyDirty, setApiKeyDirty] = useState(false);
  const apiKeyDirtyRef = useRef(false);
  const [showApiKey, setShowApiKey] = useState(false);
  const [apiKeyLoading, setApiKeyLoading] = useState(
    Boolean(initialProvider?.hasApiKey && loadMaskedApiKey),
  );
  const [apiKeyError, setApiKeyError] = useState<string>();
  const [models, setModels] = useState<ModelFormRow[]>(() => {
    if (initialProvider && !initialPreset) {
      return initialProvider.models.length
        ? initialProvider.models.map((model) => createModelRow({
          modelId: model.modelId,
          displayName: model.displayName,
          outputModality: model.outputModality,
          supportsStreaming: model.supportsStreaming,
          supportsTools: model.supportsTools,
        }))
        : [createModelRow()];
    }
    return initialPreset
      ? createPresetModelRows(initialPreset, language)
      : [createModelRow()];
  });
  const [isDropdownOpen, setIsDropdownOpen] = useState(false);
  const [isAdvancedOpen, setIsAdvancedOpen] = useState(false);
  const comboboxRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleDocumentClick = (event: MouseEvent) => {
      if (
        comboboxRef.current &&
        !comboboxRef.current.contains(event.target as Node)
      ) {
        setIsDropdownOpen(false);
      }
    };
    document.addEventListener("mousedown", handleDocumentClick);
    return () => {
      document.removeEventListener("mousedown", handleDocumentClick);
    };
  }, []);

  const currentPreset = providerPresetForKind(kind);
  const valid =
    name.trim().length > 0 &&
    baseUrl.trim().length > 0 &&
    (initialProvider?.hasApiKey || apiKey.trim().length > 0) &&
    models.some((model) => model.modelId.trim().length > 0);

  useEffect(() => {
    if (!initialProvider?.hasApiKey || !loadMaskedApiKey) return;
    let active = true;
    setApiKeyLoading(true);
    void loadMaskedApiKey(initialProvider.id)
      .then((masked) => {
        if (!active) return;
        setExistingApiKeyMask(masked);
        if (!apiKeyDirtyRef.current) setApiKey(masked);
      })
      .catch(() => {
        if (!active) return;
        setApiKeyError(
          text(
            "暂时无法读取 API Key 位数；可点击查看重试，或直接输入新值替换。",
            "Could not read the API key length. Try Show again or enter a replacement key.",
          ),
        );
      })
      .finally(() => {
        if (active) setApiKeyLoading(false);
      });
    return () => {
      active = false;
    };
  }, [initialProvider?.hasApiKey, initialProvider?.id, loadMaskedApiKey, text]);

  const toggleApiKeyVisibility = async () => {
    if (showApiKey) {
      setShowApiKey(false);
      if (initialProvider?.hasApiKey && !apiKeyDirtyRef.current) {
        setApiKey(existingApiKeyMask);
      }
      return;
    }
    if (initialProvider?.hasApiKey && !apiKeyDirtyRef.current) {
      if (!revealApiKey) return;
      setApiKeyLoading(true);
      setApiKeyError(undefined);
      try {
        const revealed = await revealApiKey(initialProvider.id);
        setExistingApiKeyMask(maskApiKey(revealed));
        setApiKey(revealed);
        setShowApiKey(true);
      } catch {
        setApiKeyError(
          text(
            "无法从系统凭据库读取完整 API Key，请检查系统授权后重试。",
            "Could not read the full API key from the system credential store. Check system access and try again.",
          ),
        );
      } finally {
        setApiKeyLoading(false);
      }
      return;
    }
    setShowApiKey(true);
  };

  return (
    <form
      className="provider-form"
      onSubmit={(event) => {
        event.preventDefault();
        if (!valid) return;
        const filteredModels = models.filter((model) => model.modelId.trim());
        onSubmit({
          id: initialProvider?.id,
          name: name.trim(),
          kind,
          protocol,
          baseUrl: baseUrl.trim(),
          apiKey: apiKeyDirty || !initialProvider?.hasApiKey ? apiKey : undefined,
          allowInsecureHttp: true,
          // Kept for backward-compatible storage and Provider-page testing.
          // Agent selection is intentionally controlled only by the switchboard.
          defaultModelId: filteredModels[0]?.modelId.trim(),
          models: filteredModels.map((model) => ({
            displayName: model.displayName.trim() || model.modelId.trim(),
            modelId: model.modelId.trim(),
            outputModality: model.outputModality,
            supportsStreaming: model.supportsStreaming,
            supportsTools: model.supportsTools,
          })),
        });
      }}
    >
      <div className="combobox-field" ref={comboboxRef}>
        <label className="field">
          <span>{text("模型供应商名称", "Provider name")}</span>
          <div className="combobox-input-wrap">
            <input
              type="text"
              value={name}
              placeholder={text(
                "选择预设，或直接输入模型供应商名称",
                "Choose a preset or enter a provider name",
              )}
              onFocus={() => {
                if (!name) setIsDropdownOpen(true);
              }}
              onChange={(event) => {
                const nextInput = event.target.value;
                const nextKind = kindForInput(nextInput);
                const nextPreset = providerPresetForKind(nextKind);
                setName(nextInput);
                setKind(nextKind);
                setIsDropdownOpen(false);
                if (nextPreset) {
                  setProtocol(nextPreset.protocol);
                  setBaseUrl(nextPreset.baseUrl);
                  setModels(createPresetModelRows(nextPreset, language));
                }
              }}
            />
            <button
              type="button"
              className="combobox-toggle-button"
              aria-label={text("切换预设下拉", "Toggle preset dropdown")}
              onClick={() => setIsDropdownOpen((prev) => !prev)}
            >
              <ChevronDown
                size={16}
                className={isDropdownOpen ? "is-open" : undefined}
              />
            </button>
          </div>
          <small>
            {text(
              "可选择内置预设，也可直接输入任意模型供应商名称。",
              "Choose a built-in preset or enter any provider name.",
            )}
          </small>
        </label>
        {isDropdownOpen && (
          <ul className="combobox-dropdown" role="listbox">
            {presetKinds.map(([presetKind, label]) => (
              <li
                key={presetKind}
                role="option"
                aria-selected={kind === presetKind}
                className={clsx(
                  "combobox-dropdown__item",
                  kind === presetKind && "is-selected",
                )}
                onClick={() => {
                  const nextPreset = providerPresetForKind(presetKind);
                  const presetName = labels[presetKind];
                  setName(presetName);
                  setKind(presetKind);
                  setIsDropdownOpen(false);
                  if (nextPreset) {
                    setProtocol(nextPreset.protocol);
                    setBaseUrl(nextPreset.baseUrl);
                      setModels(createPresetModelRows(nextPreset, language));
                  }
                }}
              >
                <ProviderLogo
                  provider={{ kind: presetKind, name: labels[presetKind] }}
                  size="row"
                  className="combobox-dropdown__logo"
                  aria-hidden="true"
                />
                <span>{labels[presetKind]}</span>
              </li>
            ))}
          </ul>
        )}
      </div>

      <label className="field">
        <span>Base URL</span>
        <input
          type="url"
          value={baseUrl}
          onChange={(event) => setBaseUrl(event.target.value)}
          placeholder="https://api.example.com/v1"
        />
      </label>

      <label
        className={`field ${!initialProvider?.hasApiKey && !apiKey.trim() ? "field--error" : ""}`}
      >
        <span>
          API Key <b className="required-mark">*</b>
        </span>
        <span className="secret-input">
          <input
            type={showApiKey ? "text" : "password"}
            autoComplete="new-password"
            value={apiKey}
            onFocus={(event) => {
              if (!apiKeyDirty && initialProvider?.hasApiKey) {
                event.currentTarget.select();
              }
            }}
            onChange={(event) => {
              setApiKey(event.target.value);
              setApiKeyDirty(true);
              apiKeyDirtyRef.current = true;
              setApiKeyError(undefined);
            }}
            placeholder={text(
              "保存到系统凭据库；请求时发送给上游",
              "Stored in the system credential store and sent upstream with requests",
            )}
          />
          <button
            type="button"
            className="secret-input__toggle"
            aria-label={
              showApiKey
                ? text("隐藏 API Key", "Hide API key")
                : text("查看 API Key", "Show API key")
            }
            title={
              showApiKey
                ? text("隐藏 API Key", "Hide API key")
                : text("查看 API Key", "Show API key")
            }
            onClick={() => void toggleApiKeyVisibility()}
            disabled={!apiKey || apiKeyLoading}
          >
            {showApiKey ? <EyeOff size={18} /> : <Eye size={18} />}
          </button>
        </span>
        {!initialProvider?.hasApiKey && !apiKey.trim() && (
          <small className="field-error" role="alert">
            {text("请输入 API Key", "API key is required")}
          </small>
        )}
        {apiKeyError && (
          <small className="field-error" role="alert">
            {apiKeyError}
          </small>
        )}
        {initialProvider?.hasApiKey && (
          <small>
            {text(
              "默认按真实位数隐藏；点击眼睛后从系统凭据库临时读取完整值。保持不变将继续使用原密钥，输入新值会替换。",
              "Hidden by default using the real key length. Show temporarily reads the full value from the system credential store. Leave it unchanged to keep the existing key, or enter a new value to replace it.",
            )}
          </small>
        )}
      </label>

      <fieldset className="model-fieldset">
        <legend>{text("模型配置", "Model configuration")}</legend>
        <p className="model-fieldset__hint">
          {text(
            "预设会填入常用模型；模型 ID 仍可从下拉建议中选择或直接自定义。模型能力按输出类型展示，悬浮问号可查看详细说明；智能体当前使用哪个模型，请在主页面切换。",
            "Presets fill common models, while model IDs remain editable. Capabilities are shown by output type; hover the question mark for details. Choose the active agent model on the switchboard.",
          )}
        </p>
        {currentPreset && (
          <datalist id={modelPresetListId}>
            {currentPreset.models.map((model) => (
              <option
                value={model.modelId}
                label={presetModelDisplayName(model, language)}
                key={model.modelId}
              />
            ))}
          </datalist>
        )}
        <div className="model-editor__header">
          <span>{text("模型 ID", "Model ID")}</span>
          <span>{text("模型名称", "Model name")}</span>
          <span>{text("模型能力", "Capabilities")}</span>
          <span>{text("操作", "Actions")}</span>
        </div>
        {models.map((model, index) => {
          const capabilityLabel =
            model.outputModality === "image"
              ? text("生图", "Image generation")
              : model.outputModality === "audio"
                ? text("语音", "Audio")
                : model.outputModality === "video"
                  ? text("视频", "Video")
                  : text("文本", "Text");
          const capabilityTooltip = model.outputModality === "image"
            ? text("图片生成模型。", "Image-generation model.")
            : model.outputModality === "audio"
              ? text("语音生成模型。", "Audio-generation model.")
              : model.outputModality === "video"
                ? text("视频生成模型。", "Video-generation model.")
                : text(
                    `文本模型；${model.supportsStreaming ? "支持" : "不支持"}流式输出，${model.supportsTools ? "支持" : "不支持"}工具调用。`,
                    `Text model; ${model.supportsStreaming ? "supports" : "does not support"} streaming output and ${model.supportsTools ? "supports" : "does not support"} tool calls.`,
                  );
          const capabilityTooltipId = `${modelPresetListId}-${index}-capability-tooltip`;
          return (
            <div
              className="model-editor"
              key={model.rowId}
            >
              <input
                list={currentPreset ? modelPresetListId : undefined}
                value={model.modelId}
                placeholder={text("模型 ID", "Model ID")}
                aria-label={text(
                  `第 ${index + 1} 个模型 ID`,
                  `Model ID ${index + 1}`,
                )}
                title={model.modelId}
                onChange={(event) => {
                  const nextModelId = event.target.value;
                  setModels((current) =>
                    current.map((item, itemIndex) => {
                      if (itemIndex !== index) return item;
                      const previousPresetModel = currentPreset?.models.find(
                        (presetModel) => presetModel.modelId === item.modelId,
                      );
                      const nextPresetModel = currentPreset?.models.find(
                        (presetModel) => presetModel.modelId === nextModelId,
                      );
                      const previousAutomaticDisplayName = previousPresetModel
                        ? presetModelDisplayName(previousPresetModel, language)
                        : item.modelId;
                      const shouldUpdateDisplayName =
                        !item.displayName ||
                        item.displayName === item.modelId ||
                        item.displayName === previousAutomaticDisplayName;
                      return {
                        ...item,
                        modelId: nextModelId,
                        outputModality:
                          nextPresetModel?.outputModality ??
                          item.outputModality,
                        supportsStreaming:
                          nextPresetModel?.supportsStreaming ??
                          item.supportsStreaming,
                        supportsTools:
                          nextPresetModel?.supportsTools ?? item.supportsTools,
                        displayName:
                          nextPresetModel && shouldUpdateDisplayName
                            ? presetModelDisplayName(nextPresetModel, language)
                            : shouldUpdateDisplayName
                              ? ""
                              : item.displayName,
                      };
                    }),
                  );
                }}
              />
              <input
                value={model.displayName}
                placeholder={text(
                  "模型名称（可选）",
                  "Model name (optional)",
                )}
                aria-label={text(
                  `第 ${index + 1} 个模型名称`,
                  `Model name ${index + 1}`,
                )}
                title={model.displayName}
                onChange={(event) =>
                  setModels((current) =>
                    current.map((item, itemIndex) =>
                      itemIndex === index
                        ? { ...item, displayName: event.target.value }
                        : item,
                    ),
                  )
                }
              />
              <div className="model-editor__capability">
                <span
                  className={`model-capability model-capability--${model.outputModality}`}
                >
                  <span className="model-capability__select-wrap">
                    <select
                      className="model-capability__select"
                      value={model.outputModality}
                      aria-label={text(
                        `第 ${index + 1} 个模型能力`,
                        `Model capability ${index + 1}`,
                      )}
                      onChange={(event) => {
                        const outputModality = event.target
                          .value as ModelOutputModality;
                        setModels((current) =>
                          current.map((item, itemIndex) =>
                            itemIndex === index
                              ? {
                                  ...item,
                                  outputModality,
                                  supportsStreaming:
                                    outputModality === "text",
                                  supportsTools: outputModality === "text",
                                }
                              : item,
                          ),
                        );
                      }}
                    >
                      <option value="text">{text("文本", "Text")}</option>
                      <option value="image">
                        {text("生图", "Image generation")}
                      </option>
                      <option value="audio">{text("语音", "Audio")}</option>
                      <option value="video">{text("视频", "Video")}</option>
                    </select>
                    <ChevronDown size={12} aria-hidden="true" />
                  </span>
                  <span
                    className="model-capability__help"
                    aria-label={text(
                      `${capabilityLabel}模型能力说明`,
                      `${capabilityLabel} model capability details`,
                    )}
                    aria-describedby={capabilityTooltipId}
                    tabIndex={0}
                  >
                    <CircleHelp size={13} aria-hidden="true" />
                    <span
                      className="model-capability__tooltip"
                      id={capabilityTooltipId}
                      role="tooltip"
                    >
                      {capabilityTooltip}
                    </span>
                  </span>
                </span>
              </div>
              <button
                type="button"
                className="icon-button"
                onClick={() => {
                  if (models.length === 1) {
                    const replacement = createModelRow();
                    setModels([replacement]);
                    return;
                  }
                  setModels(
                    models.filter((_, itemIndex) => itemIndex !== index),
                  );
                }}
                aria-label={text("删除模型", "Delete model")}
              >
                <Trash2 size={15} />
              </button>
            </div>
          );
        })}
        <button
          type="button"
          className="text-button"
          onClick={() =>
            setModels((current) => [...current, createModelRow()])
          }
        >
          <Plus size={15} /> {text("添加模型", "Add model")}
        </button>
      </fieldset>

      <details
        className="advanced-options"
        open={isAdvancedOpen}
        onToggle={(event) =>
          setIsAdvancedOpen((event.target as HTMLDetailsElement).open)
        }
      >
        <summary className="advanced-options__summary">
          <div className="advanced-options__header">
            <ChevronRight
              size={16}
              className={clsx(
                "advanced-options__chevron",
                isAdvancedOpen && "is-open",
              )}
            />
            <strong>{text("高级选项", "Advanced options")}</strong>
          </div>
          <p className="advanced-options__description">
            {text(
              "包含 API 格式、认证字段、模型映射等配置。大多数场景下保持默认即可。",
              "Includes API format, authentication fields, model mapping configurations. Keep defaults for most scenarios.",
            )}
          </p>
        </summary>
        <div className="advanced-options__content">
          <label className="field">
            <span>
              {text("默认 / 回退 API 协议", "Default / fallback API protocol")}
            </span>
            <select
              value={protocol}
              onChange={(event) =>
                setProtocol(event.target.value as ApiProtocol)
              }
            >
              {Object.entries(protocolLabels).map(([value, label]) => (
                <option value={value} key={value}>
                  {label}
                </option>
              ))}
            </select>
            <small>
              {kind === "mongyun"
                ? text(
                    "蒙云智算内置支持 OpenAI Chat 和 OpenAI Responses；AT-Switch 会优先匹配当前智能体的原生协议。",
                    "Mongyun supports OpenAI Chat and OpenAI Responses. AT-Switch prefers the current agent's native protocol.",
                  )
                : text(
                    "当模型供应商没有与智能体相同的接口时，AT-Switch 会使用此协议并进行转换。",
                    "When a provider does not expose the same interface as the agent, AT-Switch uses this protocol for conversion.",
                  )}
              {" "}
              {text(
                "Base URL 填到 /v1 即可，路径会按实际路由自动拼接。",
                "Set the Base URL through /v1; route paths are appended automatically.",
              )}
            </small>
          </label>
        </div>
      </details>

      <button
        className="button button--primary provider-form__submit"
        type="submit"
        disabled={!valid || busy}
      >
        {busy
          ? text("安全保存中…", "Saving securely…")
          : text("保存模型供应商", "Save provider")}
      </button>
    </form>
  );
}
