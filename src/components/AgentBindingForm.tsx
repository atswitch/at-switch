import {
  ArrowRight,
  FolderOpen,
  KeyRound,
  LoaderCircle,
  Network,
  RotateCcw,
  ShieldCheck,
} from "lucide-react";
import { useMemo, useState } from "react";
import { useLanguage } from "../i18n";
import { supportsDirectBinding } from "../lib/agentCapabilities";
import { protocolLabels } from "../lib/format";
import { modelIsReady } from "../lib/modelCapabilities";
import type {
  AgentBindingDraft,
  AgentSummary,
  ProviderSummary,
} from "../types";

interface AgentBindingFormProps {
  agent: AgentSummary;
  providers: ProviderSummary[];
  mode: AgentBindingDraft["mode"];
  busy: boolean;
  onSubmit: (draft: AgentBindingDraft) => void;
  platform?: string;
  installPathBusy?: boolean;
  onSelectInstallPath?: () => void;
  onClearInstallPath?: () => void;
}

export function AgentBindingForm({
  agent,
  providers,
  mode,
  busy,
  onSubmit,
  platform = "unknown",
  installPathBusy = false,
  onSelectInstallPath = () => undefined,
  onClearInstallPath = () => undefined,
}: AgentBindingFormProps) {
  const { text } = useLanguage();
  const availableProviders = useMemo(
    () =>
      providers
        .filter(
          (provider) =>
            provider.isEnabled &&
            provider.hasApiKey &&
            provider.models.some(modelIsReady),
        )
        .sort(
          (left, right) =>
            Number(right.isRecommended) - Number(left.isRecommended),
        ),
    [providers],
  );
  const initialProvider =
    availableProviders.find((provider) => provider.id === agent.providerId) ??
    availableProviders[0];
  const [providerId, setProviderId] = useState(initialProvider?.id ?? "");
  const selectedProvider =
    availableProviders.find((provider) => provider.id === providerId) ??
    initialProvider;
  const selectedModels =
    selectedProvider?.models.filter(modelIsReady) ?? [];
  const initialModelId =
    selectedModels.find((model) => model.modelId === agent.modelId)
      ?.modelId ??
    selectedModels.find(
      (model) => model.modelId === selectedProvider?.defaultModelId,
    )?.modelId ??
    selectedModels[0]?.modelId ??
    "";
  const [modelId, setModelId] = useState(initialModelId);
  const directCompatible = selectedProvider
    ? supportsDirectBinding(agent.id, selectedProvider)
    : false;
  const canSubmit =
    Boolean(selectedProvider && modelId) &&
    selectedModels.some((model) => model.modelId === modelId) &&
    (mode === "proxy" || directCompatible) &&
    !busy;

  const chooseProvider = (nextProviderId: string) => {
    const nextProvider = availableProviders.find(
      (provider) => provider.id === nextProviderId,
    );
    const nextModels =
      nextProvider?.models.filter(modelIsReady) ?? [];
    setProviderId(nextProviderId);
    setModelId(
      nextModels.find(
        (model) => model.modelId === nextProvider?.defaultModelId,
      )?.modelId ?? nextModels[0]?.modelId ?? "",
    );
  };

  if (mode === "direct") {
    const currentProvider = providers.find(
      (provider) => provider.id === agent.providerId,
    );
    const currentModel = currentProvider?.models.find(
      (model) => model.modelId === agent.modelId,
    );
    const supportsInstallSelection =
      platform === "macos" || platform === "windows";

    return (
      <section
        className="binding-form"
        aria-label={text("智能体配置详情", "Agent configuration details")}
      >
        <div className="binding-route">
          <div>
            <small>{text("目标智能体", "Target agent")}</small>
            <strong>{agent.displayName}</strong>
          </div>
          <ArrowRight size={18} aria-hidden="true" />
          <div>
            <small>{text("当前模型来源", "Current model source")}</small>
            <strong>
              {currentProvider?.name ??
                text("Agent 原生路由", "Agent native route")}
            </strong>
            {currentModel && <span>{currentModel.displayName}</span>}
          </div>
        </div>

        <div className="binding-metadata">
          <span>
            {text("上游协议", "Upstream protocol")}
            <strong>
              {currentProvider ? protocolLabels[currentProvider.protocol] : "—"}
            </strong>
          </span>
          <span>
            {text("配置文件", "Configuration file")}
            <code title={agent.configPath}>
              {agent.configPath ??
                text("首次切换时创建", "Created on first switch")}
            </code>
          </span>
          {supportsInstallSelection && (
            <span className="binding-metadata__actions">
              <button
                type="button"
                className="button button--small button--secondary"
                disabled={installPathBusy}
                onClick={onSelectInstallPath}
              >
                {installPathBusy ? (
                  <LoaderCircle className="is-spinning" size={15} />
                ) : (
                  <FolderOpen size={15} />
                )}
                {agent.customInstallPath
                  ? text("重新选择", "Choose again")
                  : text("选择安装位置", "Choose installation")}
              </button>
              {agent.customInstallPath && (
                <button
                  type="button"
                  className="button button--small button--secondary"
                  disabled={installPathBusy}
                  onClick={onClearInstallPath}
                >
                  <RotateCcw size={15} />
                  {text("恢复自动发现", "Use automatic discovery")}
                </button>
              )}
            </span>
          )}
        </div>

        <fieldset className="binding-mode binding-mode--fixed">
          <legend>{text("接入方式", "Connection mode")}</legend>
          <div className="binding-mode__option is-selected">
            <KeyRound size={18} />
            <span>
              <strong>
                {text("智能体直连（默认）", "Agent direct (default)")}
              </strong>
              <small>
                {text(
                  "模型切换统一在主页面完成；请求不经过 AT-Switch，本页只展示配置与安装位置。",
                  "Switch models from the main page. Requests bypass AT-Switch; this view only shows configuration and installation details.",
                )}
              </small>
            </span>
          </div>
        </fieldset>

        <div className="binding-safety">
          <ShieldCheck size={18} />
          <p>
            {text(
              "模型切换前会创建加密备份；只更新 AT-Switch 管理的配置项，并保留智能体中的其他设置。",
              "Model switches create an encrypted backup first. Only AT-Switch-managed fields are updated; all other agent settings are preserved.",
            )}
          </p>
        </div>
      </section>
    );
  }

  if (!availableProviders.length) {
    return (
      <div className="binding-empty">
        <KeyRound size={24} />
        <h3>{text("还没有可分发的模型供应商", "No distributable provider yet")}</h3>
        <p>
          {text(
            "请先在“模型供应商与大模型”页面保存 API Key 并至少配置一个模型；文本模型还需要通过连接测试。",
            "Save an API key and configure at least one model on the Model providers & LLMs page. Text models must also pass a connection test.",
          )}
        </p>
      </div>
    );
  }

  return (
    <form
      className="binding-form"
      onSubmit={(event) => {
        event.preventDefault();
        if (!selectedProvider || !canSubmit) return;
        onSubmit({
          agentId: agent.id,
          providerId: selectedProvider.id,
          modelId,
          mode: "proxy",
        });
      }}
    >
      <div className="binding-route">
        <div>
          <small>{text("目标智能体", "Target agent")}</small>
          <strong>{agent.displayName}</strong>
        </div>
        <ArrowRight size={18} aria-hidden="true" />
        <div>
          <small>{text("模型来源", "Model source")}</small>
          <strong>{selectedProvider?.name}</strong>
        </div>
      </div>

      <div className="form-grid">
        <label className="field">
          <span>{text("模型供应商", "Provider")}</span>
          <select
            aria-label={text("模型供应商", "Provider")}
            value={selectedProvider?.id ?? ""}
            onChange={(event) => chooseProvider(event.target.value)}
          >
            {availableProviders.map((provider) => (
              <option value={provider.id} key={provider.id}>
                {provider.name}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>{text("模型", "Model")}</span>
          <select
            aria-label={text("模型", "Model")}
            value={modelId}
            onChange={(event) => setModelId(event.target.value)}
          >
            {selectedModels.map((model) => (
              <option value={model.modelId} key={model.id}>
                {model.displayName} · {model.modelId}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className="binding-metadata">
        <span>
          {text("上游协议", "Upstream protocol")}
          <strong>
            {selectedProvider
              ? protocolLabels[selectedProvider.protocol]
              : "—"}
          </strong>
        </span>
        <span>
          {text("配置文件", "Configuration file")}
          <code>
            {agent.configPath ??
              text("首次应用时创建", "Created on first application")}
          </code>
        </span>
      </div>

      <fieldset className="binding-mode binding-mode--fixed">
        <legend>{text("接入方式", "Connection mode")}</legend>
        <div className="binding-mode__option is-selected">
          <Network size={18} />
          <span>
            <strong>{text("本地代理（高级）", "Local proxy (advanced)")}</strong>
            <small>
              {text(
                "通过 AT-Switch 转发并适配协议；上游 API Key 留在系统凭据库。保存配置不会自动启动代理，请返回本地代理页面手动启动。",
                "Route through AT-Switch with protocol adaptation; the upstream API key stays in the system credential store. Saving does not start the proxy; return to the Local proxy page and start it manually.",
              )}
            </small>
          </span>
        </div>
      </fieldset>

      <div className="binding-safety">
        <ShieldCheck size={18} />
        <p>
          {text(
            "应用前会创建加密备份；只更新 AT-Switch 管理的配置项，并保留智能体中的其他设置。",
            "An encrypted backup is created before applying. Only AT-Switch-managed fields are updated; all other agent settings are preserved.",
          )}
        </p>
      </div>

      <button
        className="button button--primary binding-submit"
        disabled={!canSubmit}
        type="submit"
      >
        {busy
          ? text("正在安全写入…", "Writing securely…")
          : text(
              `应用到 ${agent.displayName}`,
              `Apply to ${agent.displayName}`,
            )}
      </button>
    </form>
  );
}
