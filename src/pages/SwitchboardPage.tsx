import {
  Check,
  Cloud,
  FolderOpen,
  KeyRound,
  LoaderCircle,
  Pencil,
  Plus,
  Radio,
  RotateCcw,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import clsx from "clsx";
import { useMemo } from "react";
import { useLanguage } from "../i18n";
import { ProviderLogo } from "../components/ProviderLogo";
import {
  directBindingRequirement,
  supportsDirectBinding,
} from "../lib/agentCapabilities";
import {
  agentAvailabilityLabel,
} from "../lib/format";
import { modelRequiresVerification } from "../lib/modelCapabilities";
import { providerPresetDisplayRank } from "../lib/providerPresets";
import type {
  AgentSummary,
  ModelSummary,
  ProviderSummary,
} from "../types";

interface SwitchboardPageProps {
  agent: AgentSummary;
  providers: ProviderSummary[];
  testingId?: string;
  switchingKey?: string;
  onCreateProvider: () => void;
  onEditProvider: (provider: ProviderSummary) => void;
  onDeleteModel?: (provider: ProviderSummary, model: ModelSummary) => void;
  onTestProvider: (providerId: string, modelId: string) => void;
  onSwitchModel: (
    provider: ProviderSummary,
    model: ModelSummary,
  ) => void;
  onRestoreNative: () => void;
  platform?: string;
  installPathBusy?: boolean;
  onSelectInstallPath?: () => void;
  onClearInstallPath?: () => void;
}

export function SwitchboardPage({
  agent,
  providers,
  testingId,
  switchingKey,
  onCreateProvider,
  onEditProvider,
  onDeleteModel,
  onTestProvider,
  onSwitchModel,
  onRestoreNative,
  platform = "unknown",
  installPathBusy = false,
  onSelectInstallPath = () => undefined,
  onClearInstallPath = () => undefined,
}: SwitchboardPageProps) {
  const { language, text } = useLanguage();
  const orderedProviders = useMemo(
    () =>
      [...providers]
        .filter((provider) => provider.isEnabled)
        .sort(
          (left, right) =>
            providerPresetDisplayRank(left.kind) -
            providerPresetDisplayRank(right.kind),
        ),
    [providers],
  );

  // 首页只展示「有可切换模型」的供应商；空 models 的供应商属于未完成配置，
  // 交给用户在「模型供应商」管理页补全，不在首页占用位置，避免出现无法切换的
  // 空壳卡片。
  const providersWithModels = useMemo(
    () => orderedProviders.filter((provider) => provider.models.length > 0),
    [orderedProviders],
  );

  const installed = agent.installStatus !== "not_installed";
  const agentReady = installed && agent.adapterVerified;
  const modelCount = providersWithModels.reduce(
    (count, provider) => count + provider.models.length,
    0,
  );
  const hasSwitchableModels = modelCount > 0;
  const supportsInstallSelection =
    platform === "macos" || platform === "windows";

  return (
    <div className={clsx("switchboard", !installed && "is-unavailable")}>
      <header
        className="switchboard__header"
        aria-label={text("当前智能体状态", "Current agent status")}
      >
        <h1 className="visually-hidden">{agent.displayName}</h1>
        <div className="switchboard__route">
          <strong>
            <span>{agent.displayName}</span>
            <i aria-hidden="true">›</i>
            <span>{text("当前路由", "Current route")}</span>
            <i aria-hidden="true">›</i>
            <span>
              {agent.providerName
                ? `${agent.providerName} · ${agent.modelId ?? text("模型", "Model")}`
                : text("Agent 原生路由", "Agent native route")}
            </span>
          </strong>
          <span
            className={clsx(
              "switchboard__agent-status",
              agentReady && "is-ready",
            )}
          >
            {agent.detectedVersion ? `v${agent.detectedVersion} · ` : null}
            {agentAvailabilityLabel(
              agent.installStatus,
              agent.runtimeStatus,
              language,
            )}
          </span>
        </div>

        <NativeRouteControl
          agent={agent}
          busy={switchingKey === `native:${agent.id}`}
          disabled={!agentReady || Boolean(switchingKey)}
          onRestore={onRestoreNative}
        />
      </header>

      {!agentReady && (
        <div className="switchboard-alert" role="status">
          <div>
            <strong>
              {!installed
                ? text(
                    `没有检测到 ${agent.displayName}`,
                    `${agent.displayName} was not detected`,
                  )
                : text(
                    `${agent.displayName} 当前保持只读`,
                    `${agent.displayName} is currently read-only`,
                  )}
            </strong>
            <span>
              {!installed
                ? text(
                    `未在标准目录、系统应用索引、运行进程或 PATH 中检测到 ${agent.displayName}。下载智能体即可使用以下大模型`,
                    `${agent.displayName} was not detected in standard directories, app index, running processes, or PATH. Download the agent to use the following models.`,
                  )
                : language === "zh-CN" && agent.message
                ? agent.message
                : text(
                    "请确认安装位置和版本，刷新状态后再进行模型切换。",
                    "Check the installation path and version, then refresh status before switching models.",
                  )}
            </span>
          </div>
          {supportsInstallSelection && (
            <div className="switchboard-alert__actions">
              <button
                type="button"
                className="button button--small"
                disabled={installPathBusy}
                onClick={onSelectInstallPath}
              >
                {installPathBusy ? (
                  <LoaderCircle className="is-spinning" size={15} />
                ) : (
                  <FolderOpen size={15} />
                )}
                {text("选择安装位置", "Choose installation folder")}
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
            </div>
          )}
        </div>
      )}

      {agent.message && agentReady && (
        <div
          className={clsx(
            "switchboard-alert",
            agent.configHealth === "healthy" && "switchboard-alert--info",
          )}
          role="status"
        >
          <div>
            <strong>
              {agent.configHealth === "healthy"
                ? text(
                    `${agent.displayName} 模型切换状态`,
                    `${agent.displayName} model switch status`,
                  )
                : text(
                    `${agent.displayName} 配置校验未通过`,
                    `${agent.displayName} configuration validation failed`,
                  )}
            </strong>
            <span>
              {language === "zh-CN"
                ? agent.message
                : agent.configHealth === "healthy"
                  ? "The agent is ready. New model switches update its managed configuration safely."
                  : "Review the agent installation and configuration, then refresh status before switching models."}
            </span>
          </div>
        </div>
      )}

      <div
        className="model-list"
        aria-label={text(
          `${agent.displayName} 模型列表`,
          `${agent.displayName} model list`,
        )}
      >
        <div className="switchboard-models__header">
          <h2>{text("供应商模型", "Provider models")}</h2>
        </div>

        {!hasSwitchableModels && (
          <div className="model-list__empty-hint" role="status">
            <Cloud size={20} />
            <div>
              <strong>
                {text(
                  `还没有可切换的模型`,
                  `No switchable models yet`,
                )}
              </strong>
              <span>
                {text(
                  `首页不预置任何模型供应商。点击下方「添加模型供应商与大模型」，保存一个 API Key 并配置模型后即可在此切换。`,
                  `The switchboard ships with no providers. Tap "Add provider and models" below, save an API key, configure models, then switch here.`,
                )}
              </span>
            </div>
          </div>
        )}

        {providersWithModels.flatMap((provider) => {
          return provider.models.map((model) => {
            const configured =
              agent.providerId === provider.id &&
              agent.modelId === model.modelId &&
              agent.configHealth === "healthy";
            const requiresVerification = modelRequiresVerification(model);
            const directCompatible = supportsDirectBinding(agent.id, provider);
            const active =
              agentReady &&
              configured &&
              !agent.activationRequired &&
              agent.mode === "direct";
            const key = `${provider.id}:${model.modelId}`;
            const switching = switchingKey === key;
            const canSwitch =
              agentReady &&
              provider.hasApiKey &&
              directCompatible &&
              !switchingKey;

            return (
              <article
                className={clsx("model-row", active && "is-active")}
                key={key}
              >
                <ProviderMark provider={provider} />

                <div className="model-row__identity">
                  <div className="model-row__title">
                    <strong>{model.displayName}</strong>
                    <span>{provider.name}</span>
                  </div>
                  <div className="model-row__meta">
                    <button
                      className="model-row__endpoint"
                      type="button"
                      onClick={() => onEditProvider(provider)}
                      title={
                        provider.baseUrl ||
                        text("尚未填写 Endpoint", "Endpoint not provided")
                      }
                    >
                      {provider.baseUrl ||
                        text("尚未填写 Endpoint", "Endpoint not provided")}
                    </button>
                    <span className="model-row__model-id">{model.modelId}</span>
                  </div>
                </div>

                <div className="model-row__status">
                  {!directCompatible && (
                    <span className="verification-copy">
                      {text("直连需要", "Direct mode requires")} {" "}
                      {directBindingRequirement(agent.id, language)}
                    </span>
                  )}
                </div>

                <div className="model-row__actions">
                  {requiresVerification && (
                    <button
                      type="button"
                      className="row-icon-button"
                      aria-label={text(
                        `测试 ${provider.name}`,
                        `Test ${provider.name}`,
                      )}
                      title={text(
                        `使用 ${model.displayName} 验证模型供应商的普通响应、流式输出与工具调用能力`,
                        `Use ${model.displayName} to validate normal responses, streaming, and tool calls`,
                      )}
                      onClick={() => onTestProvider(provider.id, model.modelId)}
                      disabled={testingId === key}
                    >
                      <Radio
                        size={16}
                        className={
                          testingId === key ? "is-pulsing" : undefined
                        }
                      />
                    </button>
                  )}
                  <button
                    type="button"
                    className="row-icon-button"
                    aria-label={text(
                      `编辑 ${provider.name}`,
                      `Edit ${provider.name}`,
                    )}
                    title={text("编辑模型供应商", "Edit provider")}
                    onClick={() => onEditProvider(provider)}
                  >
                    <Pencil size={16} />
                  </button>
                  {onDeleteModel && (
                    <button
                      type="button"
                      className="row-icon-button row-icon-button--danger"
                      aria-label={text(
                        `删除 ${model.displayName}`,
                        `Delete ${model.displayName}`,
                      )}
                      title={text("删除模型", "Delete model")}
                      onClick={() => onDeleteModel(provider, model)}
                    >
                      <Trash2 size={16} />
                    </button>
                  )}
                  {active ? (
                    <span className="current-button">
                      <Check size={15} />
                      {text("使用中", "In use")}
                    </span>
                  ) : (
                    <button
                      type="button"
                      className="switch-button"
                      disabled={!canSwitch}
                      title={
                        !provider.hasApiKey
                          ? text(
                              "请先编辑模型供应商并保存 API Key",
                              "Edit the model provider and save an API key first",
                            )
                          : !directCompatible
                          ? text(
                              `该模型供应商未提供 ${directBindingRequirement(agent.id, language)}；如需协议转换，请前往高级设置使用本地代理`,
                              `This provider does not offer ${directBindingRequirement(agent.id, language)}. Use the local proxy in Advanced settings for protocol conversion.`,
                            )
                          : !agentReady
                            ? text(
                                "智能体尚不可配置",
                                "Agent is not configurable",
                              )
                            : undefined
                      }
                      onClick={() => onSwitchModel(provider, model)}
                    >
                      {switching ? (
                        <>
                          <LoaderCircle className="is-spinning" size={15} />
                          {text("切换中", "Switching")}
                        </>
                      ) : (
                        text("切换", "Switch")
                      )}
                    </button>
                  )}
                </div>
              </article>
            );
          });
        })}

        <button
          type="button"
          className="model-row model-row--add"
          onClick={onCreateProvider}
        >
          <span className="model-row--add__icon">
            <Plus size={21} />
          </span>
          <span>
            <strong>
              {text("添加模型供应商与大模型", "Add provider and models")}
            </strong>
            <small>
              {text(
                "保存一个 API Key，配置多个可切换模型",
                "Save one API key and configure multiple switchable models",
              )}
            </small>
          </span>
        </button>
      </div>

      <footer className="switchboard__safety">
        <ShieldCheck size={18} />
        <span>
          {text(
            "切换前自动建立加密备份；只修改 AT-Switch 管理的字段，失败时自动恢复。",
            "An encrypted backup is created before switching. Only AT-Switch-managed fields are changed, with automatic recovery on failure.",
          )}
        </span>
        <b>LOCAL FIRST</b>
      </footer>
    </div>
  );
}

function NativeRouteControl({
  agent,
  busy,
  disabled,
  onRestore,
}: {
  agent: AgentSummary;
  busy: boolean;
  disabled: boolean;
  onRestore: () => void;
}) {
  const { text } = useLanguage();
  const active =
    agent.installStatus !== "not_installed" &&
    !agent.providerId &&
    !agent.activationRequired;
  return (
    <article className={clsx("switchboard-native-control", active && "is-active")}>
      <div className="switchboard-native-control__copy">
        <div>
          <strong>{text("默认配置", "Default configuration")}</strong>
          <span>
            {text(
              `${agent.displayName} 自带模型`,
              `${agent.displayName} built-in models`,
            )}
          </span>
        </div>
        <small>
          {text(
            "恢复接管前配置，不经过 AT-Switch 模型供应商",
            "Restore the pre-takeover configuration without an AT-Switch provider",
          )}
        </small>
      </div>
      <div className="switchboard-native-control__action">
        {active ? (
          <span className="current-button">
            <Check size={15} />
            {text("使用中", "In use")}
          </span>
        ) : (
          <button
            type="button"
            className="switch-button"
            disabled={disabled}
            onClick={onRestore}
          >
            {busy ? (
              <>
                <LoaderCircle className="is-spinning" size={15} />
                {text("恢复中", "Restoring")}
              </>
            ) : (
              text("切换", "Switch")
            )}
          </button>
        )}
      </div>
    </article>
  );
}

function ProviderMark({ provider }: { provider: ProviderSummary }) {
  return <ProviderLogo provider={provider} size="row" />;
}
