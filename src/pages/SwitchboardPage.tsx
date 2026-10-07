import {
  Check,
  Cloud,
  FolderOpen,
  KeyRound,
  LoaderCircle,
  Network,
  Pencil,
  Plus,
  Radio,
  RotateCcw,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import clsx from "clsx";
import { useMemo, useState } from "react";
import { useLanguage } from "../i18n";
import { ProviderLogo } from "../components/ProviderLogo";
import { ProviderModelGroup } from "../components/ProviderModelGroup";
import {
  directBindingRequirement,
  directBindingUnavailableReason,
  isProxyRoutedModel,
  isProxyRoutedProvider,
  supportsDirectBinding,
  usesCloudModelSettings,
} from "../lib/agentCapabilities";
import {
  agentAvailabilityLabel,
  verificationLabel,
} from "../lib/format";
import { modelIsReady, modelRequiresVerification } from "../lib/modelCapabilities";
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
  /** 本地代理服务是否正在运行，影响"代理接管"标记和 Provider 组徽章的显示。 */
  proxyRunning?: boolean;
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
  proxyRunning = false,
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
  const cloudModelSettings = usesCloudModelSettings(agent.id);
  const agentReady = installed && agent.adapterVerified;
  const modelCount = providersWithModels.reduce(
    (count, provider) => count + provider.models.length,
    0,
  );
  const hasSwitchableModels = modelCount > 0;
  const supportsInstallSelection =
    platform === "macos" || platform === "windows";

  // 供应商较多的目录会很长，折叠状态按当前会话内的供应商 ID 记录；切换 Agent 时
  // 页面随 key 重挂载，回到全部展开的默认视图。
  const [collapsedProviderIds, setCollapsedProviderIds] = useState<string[]>(
    [],
  );
  const toggleProviderGroup = (providerId: string) => {
    setCollapsedProviderIds((current) =>
      current.includes(providerId)
        ? current.filter((id) => id !== providerId)
        : [...current, providerId],
    );
  };

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
            <span>
              {agent.proxyPrefEnabled
                ? text("代理路由", "Proxy route")
                : text("当前路由", "Current route")}
            </span>
            <i aria-hidden="true">›</i>
            <span>
              {cloudModelSettings && agent.activationRequired
                ? text("模型状态待确认", "Model state needs confirmation")
                : agent.providerName
                  ? `${agent.providerName} · ${agent.modelId ?? text("模型", "Model")}`
                  : cloudModelSettings
                    ? text("ima 原有模型", "Original ima models")
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

        {providersWithModels.map((provider) => (
          <ProviderModelGroup
            key={provider.id}
            provider={provider}
            modelCount={provider.models.length}
            expanded={!collapsedProviderIds.includes(provider.id)}
            inUse={
              agent.providerId === provider.id &&
              provider.models.some((model) => model.modelId === agent.modelId)
            }
            proxyRouted={isProxyRoutedProvider(agent, provider.id, proxyRunning)}
            onToggle={() => toggleProviderGroup(provider.id)}
          >
            {provider.models.map((model) => {
              const configured =
                agent.providerId === provider.id &&
                agent.modelId === model.modelId &&
                agent.configHealth === "healthy";
              const requiresVerification = modelRequiresVerification(model);
              // 验证状态属于具体模型：文本模型必须自己通过连接测试，非文本模型免验证。
              const modelReady = modelIsReady(model);
              const pendingVerification = requiresVerification && !modelReady;
              const directCompatible = supportsDirectBinding(agent.id, provider);
              const active =
                agentReady &&
                configured &&
                !agent.activationRequired &&
                agent.mode === "direct";
              // 代理接管是另一种"正在使用"：目标模型显示「代理中」并提示运行依赖。
              const proxyActive =
                agentReady &&
                configured &&
                !agent.activationRequired &&
                isProxyRoutedModel(agent, provider.id, model.modelId, proxyRunning);
              // 当前已绑定的模型：无论直连/代理、是否待重启、代理是否运行，只要
              // providerId/modelId 匹配且配置健康即视为"已选择"，让首页始终能指认
              // 当前绑定（与配置弹窗口径一致），避免出现"首页没选中但已绑定"的脱节。
              const selected = agentReady && configured;
              const key = `${provider.id}:${model.modelId}`;
              const switching = switchingKey === key;
              const canSwitch =
                agentReady &&
                provider.hasApiKey &&
                modelReady &&
                directCompatible &&
                !switchingKey;

              return (
                <article
                  className={clsx(
                    "model-row",
                    active && "is-active",
                    proxyActive && "is-proxy",
                    selected && "is-selected",
                  )}
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
                    {proxyActive && !pendingVerification && (
                      <span
                        className="verification-copy verification-copy--proxy"
                        title={text(
                          "流量经由本地代理；退出 AT-Switch 后该智能体无法请求",
                          "Traffic goes through the local proxy; this agent cannot make requests after AT-Switch exits",
                        )}
                      >
                        <Network size={11} aria-hidden="true" />
                        {text("代理接管", "Proxied")}
                      </span>
                    )}
                    {pendingVerification && (
                      <button
                        type="button"
                        className="verification-copy verification-copy--pending"
                        onClick={() => onTestProvider(provider.id, model.modelId)}
                        disabled={Boolean(testingId)}
                        title={text(
                          "点击开始连接验证，通过后即可切换或配置本地代理",
                          "Click to start the connection test; the model becomes switchable once it passes",
                        )}
                      >
                        <strong>
                          {verificationLabel(model.verificationStatus, language)}
                        </strong>
                        <span aria-hidden="true">
                          {testingId === key
                            ? text("验证中…", "Verifying…")
                            : text("点击验证", "Click to verify")}
                        </span>
                      </button>
                    )}
                    {!pendingVerification && !directCompatible && (
                      <span className="verification-copy">
                        {text("直连需要", "Direct mode requires")} {" "}
                        {directBindingRequirement(agent.id, language)}
                      </span>
                    )}
                    {selected && !active && !proxyActive && !pendingVerification && directCompatible && (
                      <span
                        className="verification-copy verification-copy--selected"
                        title={text(
                          agent.activationRequired
                            ? "已选择该模型，重启智能体后生效"
                            : !proxyRunning && agent.mode === "proxy"
                              ? "已选择该模型，本地代理未运行，暂未接管流量"
                              : "已选择该模型，等待生效",
                          agent.activationRequired
                            ? "Selected; takes effect after the agent restarts"
                            : !proxyRunning && agent.mode === "proxy"
                              ? "Selected; the local proxy is stopped, so it is not routing traffic yet"
                              : "Selected; pending activation",
                        )}
                      >
                        {agent.activationRequired
                          ? text("已选择 · 待重启生效", "Selected · after restart")
                          : !proxyRunning && agent.mode === "proxy"
                            ? text("已选择 · 代理未运行", "Selected · proxy off")
                            : text("已选择", "Selected")}
                      </span>
                    )}
                  </div>

                  <div className="model-row__actions">
                    {requiresVerification && (
                      <button
                        type="button"
                        className={clsx(
                          "row-icon-button",
                          pendingVerification && "row-icon-button--attention",
                        )}
                        aria-label={
                          pendingVerification
                            ? text(
                                `点击验证 ${model.displayName}`,
                                `Click to verify ${model.displayName}`,
                              )
                            : text(
                                `测试 ${provider.name}`,
                                `Test ${provider.name}`,
                              )
                        }
                        title={
                          pendingVerification
                            ? text(
                                `点击验证 ${model.displayName}：通过后即可切换或配置本地代理`,
                                `Click to verify ${model.displayName}: it becomes switchable once it passes`,
                              )
                            : text(
                                `使用 ${model.displayName} 验证模型供应商的普通响应、流式输出与工具调用能力`,
                                `Use ${model.displayName} to validate normal responses, streaming, and tool calls`,
                              )
                        }
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
                            : pendingVerification
                            ? text(
                                "该模型尚未通过连接验证，请先点击右侧连接测试",
                                "This model has not passed the connection test yet. Run the test on the right first",
                              )
                            : !directCompatible
                            ? directBindingUnavailableReason(agent.id, language)
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
            })}
          </ProviderModelGroup>
        ))}

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
          {cloudModelSettings ? text(
            "同时切换「问问 ima」和「我的 copilot」；保留接管前的模型选择，恢复时只撤销 AT-Switch 的更改。",
            "Switch both Ask ima and My copilot. Preserve their original model selections and undo only AT-Switch changes when restoring.",
          ) : text(
            "切换前自动建立加密备份；只修改 AT-Switch 管理的字段，失败时自动恢复。",
            "An encrypted backup is created before switching. Only AT-Switch-managed fields are changed, with automatic recovery on failure.",
          )}
        </span>
        <b>{cloudModelSettings ? "IMA ACCOUNT" : "LOCAL FIRST"}</b>
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
  const cloudModelSettings = usesCloudModelSettings(agent.id);
  const active =
    agent.installStatus !== "not_installed" &&
    !agent.providerId &&
    (!cloudModelSettings || agent.configHealth === "healthy") &&
    !agent.activationRequired;
  return (
    <article className={clsx("switchboard-native-control", active && "is-active")}>
      <div className="switchboard-native-control__copy">
        <div>
          <strong>{cloudModelSettings ? text("原始模型", "Original models") : text("默认配置", "Default configuration")}</strong>
          <span>
            {cloudModelSettings ? text("接管前的模型选择", "Pre-takeover model selections") : text(
              `${agent.displayName} 自带模型`,
              `${agent.displayName} built-in models`,
            )}
          </span>
        </div>
        <small>
          {cloudModelSettings ? text(
            "恢复两个入口各自原来的模型，保留你已有的自定义模型",
            "Restore each entry's original model and preserve your existing custom models",
          ) : text(
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
