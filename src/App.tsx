import { AlertTriangle, CheckCircle2, LoaderCircle } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AppShell } from "./components/AppShell";
import { AgentBindingForm } from "./components/AgentBindingForm";
import { AgentRestartConfirmation } from "./components/AgentRestartConfirmation";
import { BrandLogo } from "./components/BrandLogo";
import { Modal } from "./components/Modal";
import { ProviderForm } from "./components/ProviderForm";
import { LanguageProvider, useLanguage } from "./i18n";
import { isSwitchableAgent } from "./lib/agentCapabilities";
import { api, getActiveMockSnapshot } from "./lib/api";
import { AgentsPage } from "./pages/AgentsPage";
import { ProvidersPage } from "./pages/ProvidersPage";
import { ProxyPage } from "./pages/ProxyPage";
import { SettingsPage } from "./pages/SettingsPage";
import { SwitchboardPage } from "./pages/SwitchboardPage";
import type {
  AppSettings,
  AppSnapshot,
  AgentBindingDraft,
  AgentSummary,
  CommandError,
  PageId,
  ModelSummary,
  ProviderDraft,
  ProviderSummary,
} from "./types";

type Toast = {
  id: number;
  tone: "good" | "bad";
  title: string;
  message?: string;
};

type PendingAgentAction =
  | {
      kind: "apply";
      agent: AgentSummary;
      draft: AgentBindingDraft;
      modelKey?: string;
    }
  | {
      kind: "restore";
      agent: AgentSummary;
    };

type BindingTarget = {
  agent: AgentSummary;
  mode: AgentBindingDraft["mode"];
};

function describeCommandError(
  error: unknown,
  language: "zh-CN" | "en",
): string {
  const commandError = error as Partial<CommandError>;
  const message = commandError.message ?? String(error);
  const description = commandError.recovery
    ? `${message}；${commandError.recovery}`
    : message;
  if (language === "en" && /[\p{Script=Han}]/u.test(description)) {
    return commandError.code
      ? `Operation failed (${commandError.code}). Review the configuration and try again.`
      : "Operation failed. Review the configuration and try again.";
  }
  return description;
}

function AppContent() {
  const { language, setLanguage, text } = useLanguage();
  const searchParams = useMemo(
    () =>
      typeof window !== "undefined"
        ? new URLSearchParams(window.location.search)
        : new URLSearchParams(),
    [],
  );
  const isRealData = searchParams.get("real_data") === "true";
  const showWindowFrame = searchParams.get("window_frame") === "true";
  const initialPage = (searchParams.get("page") as PageId) || "overview";
  const initialAgent =
    searchParams.get("agent") ||
    (typeof window !== "undefined"
      ? window.localStorage.getItem("at-switch-active-agent") ?? "workbuddy"
      : "workbuddy");

  const [page, setPage] = useState<PageId>(initialPage);
  const [pageHistory, setPageHistory] = useState<PageId[]>([]);
  const [activeAgentId, setActiveAgentId] = useState(initialAgent);
  const [snapshot, setSnapshot] = useState<AppSnapshot | undefined>(() =>
    isRealData ? getActiveMockSnapshot() : undefined,
  );
  const [loading, setLoading] = useState(!isRealData);
  const [refreshing, setRefreshing] = useState(false);
  const [installPathBusyAgentId, setInstallPathBusyAgentId] = useState<string>();
  const [providerModal, setProviderModal] = useState(false);
  const [bindingTarget, setBindingTarget] = useState<BindingTarget>();
  const [editingProvider, setEditingProvider] = useState<ProviderSummary>();
  const [deletingProvider, setDeletingProvider] = useState<ProviderSummary>();
  const [deletingProviderBusy, setDeletingProviderBusy] = useState(false);
  const [deletingModelTarget, setDeletingModelTarget] = useState<{
    provider: ProviderSummary;
    model: ModelSummary;
  }>();
  const [deletingModelBusy, setDeletingModelBusy] = useState(false);

  const totalModelsCount = useMemo(() => {
    if (!snapshot) return 0;
    return snapshot.providers.reduce((sum, p) => sum + p.models.length, 0);
  }, [snapshot]);

  const deletingProviderAlertState = useMemo(() => {
    if (!deletingProvider || !snapshot)
      return { show: false, mode: "none" as const, inUseAgents: [] };
    const inUseAgents = snapshot.agents.filter(
      (a) => a.providerId === deletingProvider.id,
    );
    if (inUseAgents.length === 0) {
      return { show: false, mode: "none" as const, inUseAgents: [] };
    }
    const remainingInOtherProviders =
      totalModelsCount - deletingProvider.models.length;
    if (remainingInOtherProviders > 0) {
      return { show: true, mode: "unbind_only" as const, inUseAgents };
    }
    return { show: true, mode: "restore_native" as const, inUseAgents };
  }, [deletingProvider, snapshot, totalModelsCount]);

  const deletingModelAlertState = useMemo(() => {
    if (!deletingModelTarget || !snapshot)
      return { show: false, mode: "none" as const, inUseAgents: [] };
    const inUseAgents = snapshot.agents.filter(
      (a) =>
        a.providerId === deletingModelTarget.provider.id &&
        a.modelId === deletingModelTarget.model.modelId,
    );
    if (inUseAgents.length === 0) {
      return { show: false, mode: "none" as const, inUseAgents: [] };
    }
    if (totalModelsCount > 1) {
      return { show: true, mode: "unbind_only" as const, inUseAgents };
    }
    return { show: true, mode: "restore_native" as const, inUseAgents };
  }, [deletingModelTarget, snapshot, totalModelsCount]);
  const [savingProvider, setSavingProvider] = useState(false);
  const [savingBinding, setSavingBinding] = useState(false);
  const [switchingKey, setSwitchingKey] = useState<string>();
  const [testingId, setTestingId] = useState<string>();
  const [pendingAgentAction, setPendingAgentAction] =
    useState<PendingAgentAction>();
  const [proxyBusy, setProxyBusy] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const lastAutomaticScanAt = useRef(0);
  const languageRef = useRef(language);

  useEffect(() => {
    languageRef.current = language;
  }, [language]);

  const navigateTo = useCallback((nextPage: PageId) => {
    setPage((currentPage) => {
      if (currentPage === nextPage) return currentPage;
      setPageHistory((history) => [...history, currentPage]);
      return nextPage;
    });
  }, []);

  const goBack = useCallback(() => {
    setPageHistory((history) => {
      setPage(history.at(-1) ?? "overview");
      return history.slice(0, -1);
    });
  }, []);

  const notify = useCallback(
    (tone: Toast["tone"], title: string, message?: string) => {
      const id = Date.now();
      setToasts((current) => [...current, { id, tone, title, message }]);
      window.setTimeout(
        () => setToasts((current) => current.filter((item) => item.id !== id)),
        4200,
      );
    },
    [],
  );

  const loadSnapshot = useCallback(
    async (refresh = false) => {
      if (refresh) setRefreshing(true);
      try {
        const next = refresh ? await api.refresh() : await api.bootstrap();
        setSnapshot(next);
      } catch (error) {
        const currentLanguage = languageRef.current;
        notify(
          "bad",
          currentLanguage === "zh-CN"
            ? "无法读取本地状态"
            : "Unable to read local status",
          describeCommandError(error, currentLanguage),
        );
      } finally {
        setLoading(false);
        setRefreshing(false);
      }
    },
    [notify],
  );

  useEffect(() => {
    void loadSnapshot();
  }, [loadSnapshot]);

  useEffect(() => {
    const scanWhenVisible = () => {
      if (document.visibilityState !== "visible") return;
      const now = Date.now();
      if (now - lastAutomaticScanAt.current < 1_000) return;
      lastAutomaticScanAt.current = now;
      void loadSnapshot(true);
    };
    window.addEventListener("focus", scanWhenVisible);
    document.addEventListener("visibilitychange", scanWhenVisible);
    return () => {
      window.removeEventListener("focus", scanWhenVisible);
      document.removeEventListener("visibilitychange", scanWhenVisible);
    };
  }, [loadSnapshot]);

  useEffect(() => {
    if (!snapshot) return;
    document.documentElement.dataset.theme = snapshot.settings.theme;
  }, [snapshot]);

  useEffect(() => {
    if (!snapshot || snapshot.settings.language === language) return;
    setLanguage(snapshot.settings.language);
  }, [language, setLanguage, snapshot]);

  useEffect(() => {
    if (!snapshot) return;
    const selectableAgents = snapshot.agents.filter(isSwitchableAgent);
    if (!selectableAgents.some((agent) => agent.id === activeAgentId)) {
      const fallback = selectableAgents[0];
      if (fallback) setActiveAgentId(fallback.id);
    }
  }, [activeAgentId, snapshot]);

  const selectAgent = (agentId: string) => {
    const target = snapshot?.agents.find((agent) => agent.id === agentId);
    if (!target) return;
    setActiveAgentId(agentId);
    window.localStorage.setItem("at-switch-active-agent", agentId);
    navigateTo("overview");
  };

  const saveProvider = async (draft: ProviderDraft) => {
    setSavingProvider(true);
    try {
      await api.saveProvider(draft);
      setProviderModal(false);
      await loadSnapshot(true);
      notify(
        "good",
        text("模型供应商已安全保存", "Provider saved securely"),
        text(
          "文本模型首次应用前需要完成连接测试；生图、语音和视频模型无需验证。",
          "Text models require a connection test before first use. Image-generation, audio, and video models do not require verification.",
        ),
      );
    } catch (error) {
      notify(
        "bad",
        text("保存失败", "Save failed"),
        describeCommandError(error, language),
      );
    } finally {
      setSavingProvider(false);
    }
  };

  const testProvider = async (providerId: string, modelId?: string) => {
    const testKey = modelId ? `${providerId}:${modelId}` : providerId;
    setTestingId(testKey);
    try {
      await api.testProvider(providerId, modelId);
      await loadSnapshot(true);
      notify(
        "good",
        text("模型验证通过", "Model verified"),
        text(
          "当前模型已完成普通响应及声明的流式输出、工具调用测试；该模型现在可以切换，其他模型的验证状态保持不变。",
          "This model passed normal response checks and all declared streaming and tool-call checks. It is now switchable; other model verification states are unchanged.",
        ),
      );
    } catch (error) {
      notify(
        "bad",
        text("连接测试失败", "Connection test failed"),
        describeCommandError(error, language),
      );
    } finally {
      setTestingId(undefined);
    }
  };

  const executeRestoreAgentNative = async (agent: AgentSummary) => {
    const key = `native:${agent.id}`;
    setSwitchingKey(key);
    try {
      const restored = await api.restoreAgentNative(agent.id);
      await loadSnapshot(true);
      notify(
        "good",
        text(
          `${restored.displayName} 已恢复默认配置`,
          `${restored.displayName} default configuration restored`,
        ),
        restored.needsRestart
          ? text(
              `请重启 ${restored.displayName}，让默认配置生效。`,
              `Restart ${restored.displayName} to apply its default configuration.`,
            )
          : ((language === "zh-CN" ? restored.message : undefined) ??
            text(
              "AT-Switch 管理的路由已移除，智能体自带模型可继续使用。",
              "The AT-Switch-managed route was removed. Built-in agent models remain available.",
            )),
      );
    } catch (error) {
      notify(
        "bad",
        text(
          "恢复默认配置失败",
          "Failed to restore default agent configuration",
        ),
        describeCommandError(error, language),
      );
    } finally {
      setSwitchingKey(undefined);
    }
  };

  const executeApplyAgentBinding = async (
    draft: AgentBindingDraft,
    modelKey?: string,
  ) => {
    setSavingBinding(true);
    setSwitchingKey(modelKey);
    try {
      const agent = await api.applyAgentBinding(draft);
      setBindingTarget(undefined);
      await loadSnapshot(true);
      notify(
        "good",
        agent.activationRequired
          ? text(
              `${agent.displayName} 路由已配置`,
              `${agent.displayName} route configured`,
            )
          : text(
              `${agent.displayName} 已切换`,
              `${agent.displayName} switched`,
            ),
        agent.needsRestart
          ? ((language === "zh-CN" ? agent.message : undefined) ??
            text(
              `请重启 ${agent.displayName}，让新配置生效。`,
              `Restart ${agent.displayName} to apply the new configuration.`,
            ))
          : ((language === "zh-CN" ? agent.message : undefined) ??
            `${agent.providerName ?? "Provider"} · ${agent.modelId ?? text("模型", "Model")}`),
      );
    } catch (error) {
      notify(
        "bad",
        text("智能体配置失败", "Agent configuration failed"),
        describeCommandError(error, language),
      );
    } finally {
      setSavingBinding(false);
      setSwitchingKey(undefined);
    }
  };

  const requestRestoreAgentNative = (agent: AgentSummary) => {
    if (agent.needsRestart) {
      setPendingAgentAction({ kind: "restore", agent });
      return;
    }
    void executeRestoreAgentNative(agent);
  };

  const requestApplyAgentBinding = (
    agent: AgentSummary,
    draft: AgentBindingDraft,
    modelKey?: string,
  ) => {
    if (agent.needsRestart) {
      setPendingAgentAction({ kind: "apply", agent, draft, modelKey });
      return;
    }
    void executeApplyAgentBinding(draft, modelKey);
  };

  const confirmPendingAgentAction = () => {
    const pending = pendingAgentAction;
    if (!pending) return;
    setPendingAgentAction(undefined);
    if (pending.kind === "restore") {
      void executeRestoreAgentNative(pending.agent);
    } else {
      void executeApplyAgentBinding(pending.draft, pending.modelKey);
    }
  };

  const quickSwitchModel = (
    agent: AgentSummary,
    provider: ProviderSummary,
    model: ModelSummary,
  ) => {
    requestApplyAgentBinding(
      agent,
      {
        agentId: agent.id,
        providerId: provider.id,
        modelId: model.modelId,
        mode: "direct",
      },
      `${provider.id}:${model.modelId}`,
    );
  };

  const replaceAgentSummary = useCallback((agent: AgentSummary) => {
    setSnapshot((current) =>
      current
        ? {
            ...current,
            agents: current.agents.map((item) =>
              item.id === agent.id ? agent : item,
            ),
          }
        : current,
    );
    setBindingTarget((current) =>
      current?.agent.id === agent.id ? { ...current, agent } : current,
    );
  }, []);

  const selectAgentInstallPath = async (agent: AgentSummary) => {
    const selected = await api.selectAgentInstallDirectory(
      text(
        `选择 ${agent.displayName} 安装位置`,
        `Select ${agent.displayName} installation directory`,
      ),
    );
    if (!selected) return;
    setInstallPathBusyAgentId(agent.id);
    try {
      const updated = await api.setAgentInstallPath(agent.id, selected);
      replaceAgentSummary(updated);
      notify(
        "good",
        text("安装位置已保存", "Installation location saved"),
        text(
          `${agent.displayName} 已从所选目录识别，后续刷新会优先校验该位置。`,
          `${agent.displayName} was detected in the selected folder. Future scans will validate this location first.`,
        ),
      );
    } catch (error) {
      notify(
        "bad",
        text("无法使用所选安装位置", "Unable to use selected location"),
        describeCommandError(error, language),
      );
    } finally {
      setInstallPathBusyAgentId(undefined);
    }
  };

  const clearAgentInstallPath = async (agent: AgentSummary) => {
    setInstallPathBusyAgentId(agent.id);
    try {
      const updated = await api.setAgentInstallPath(agent.id);
      replaceAgentSummary(updated);
      notify(
        "good",
        text("已恢复自动发现", "Automatic discovery restored"),
        snapshot?.platform === "macos"
          ? text(
              `${agent.displayName} 将重新使用标准目录、Bundle ID、Spotlight、运行进程和 PATH 进行识别。`,
              `${agent.displayName} will use standard directories, bundle identity, Spotlight, running processes, and PATH again.`,
            )
          : text(
              `${agent.displayName} 将重新使用标准目录、系统注册信息、运行进程和 PATH 进行识别。`,
              `${agent.displayName} will use standard directories, system registration, running processes, and PATH again.`,
            ),
      );
    } catch (error) {
      notify(
        "bad",
        text("无法恢复自动发现", "Unable to restore automatic discovery"),
        describeCommandError(error, language),
      );
    } finally {
      setInstallPathBusyAgentId(undefined);
    }
  };

  const mutateProxy = async (action: "start" | "stop") => {
    setProxyBusy(true);
    try {
      const proxy =
        action === "start" ? await api.startProxy() : await api.stopProxy();
      setSnapshot((current) => (current ? { ...current, proxy } : current));
      notify(
        "good",
        action === "start"
          ? text("本地代理已启动", "Local proxy started")
          : text("本地代理已停止", "Local proxy stopped"),
      );
    } catch (error) {
      notify(
        "bad",
        text("代理操作失败", "Proxy operation failed"),
        describeCommandError(error, language),
      );
    } finally {
      setProxyBusy(false);
    }
  };

  const confirmDeleteProvider = async () => {
    if (!deletingProvider) return;
    try {
      setDeletingProviderBusy(true);
      await api.deleteProvider(deletingProvider.id);
      notify(
        "good",
        text("模型供应商删除成功", "Model provider deleted"),
        text(
          `已删除 ${deletingProvider.name}`,
          `Deleted ${deletingProvider.name}`,
        ),
      );
      setDeletingProvider(undefined);
      await loadSnapshot(false);
    } catch (error) {
      notify(
        "bad",
        text("删除模型供应商失败", "Failed to delete model provider"),
        describeCommandError(error, language),
      );
    } finally {
      setDeletingProviderBusy(false);
    }
  };

  const confirmDeleteModel = async () => {
    if (!deletingModelTarget) return;
    const { provider, model } = deletingModelTarget;
    setDeletingModelBusy(true);
    try {
      if (provider.models.length <= 1) {
        await api.deleteProvider(provider.id);
      } else {
        const remainingModels = provider.models
          .filter((item) => item.modelId !== model.modelId)
          .map((item) => ({
            displayName: item.displayName,
            modelId: item.modelId,
            outputModality: item.outputModality,
            supportsStreaming: item.supportsStreaming,
            supportsTools: item.supportsTools,
          }));
        await api.saveProvider({
          id: provider.id,
          name: provider.name,
          kind: provider.kind,
          protocol: provider.protocol,
          baseUrl: provider.baseUrl,
          allowInsecureHttp: provider.baseUrl
            .toLowerCase()
            .startsWith("http://"),
          defaultModelId: remainingModels[0]?.modelId,
          models: remainingModels,
        });
      }
      notify(
        "good",
        text("模型已成功删除", "Model deleted successfully"),
        text(
          `已删除模型 ${model.displayName}`,
          `Deleted model ${model.displayName}`,
        ),
      );
      setDeletingModelTarget(undefined);
      await loadSnapshot(false);
    } catch (error) {
      notify(
        "bad",
        text("删除模型失败", "Failed to delete model"),
        describeCommandError(error, language),
      );
    } finally {
      setDeletingModelBusy(false);
    }
  };

  const updatePort = async (port: number) => {
    try {
      const proxy = await api.updateProxyPort(port);
      setSnapshot((current) => (current ? { ...current, proxy } : current));
      notify(
        "good",
        text(
          `代理端口已更新为 ${port}`,
          `Proxy port updated to ${port}`,
        ),
      );
    } catch (error) {
      notify(
        "bad",
        text("端口更新失败", "Port update failed"),
        describeCommandError(error, language),
      );
    }
  };

  const updateSettings = async (settings: Partial<AppSettings>) => {
    if (!snapshot) return;
    const previous = snapshot.settings;
    setSnapshot({ ...snapshot, settings: { ...previous, ...settings } });
    try {
      const saved = await api.updateSettings(settings);
      setSnapshot((current) =>
        current ? { ...current, settings: saved } : current,
      );
    } catch (error) {
      setSnapshot((current) =>
        current ? { ...current, settings: previous } : current,
      );
      notify(
        "bad",
        text("设置未保存", "Settings were not saved"),
        describeCommandError(error, language),
      );
    }
  };

  if (loading || !snapshot) {
    return (
      <div className="boot-screen">
        <BrandLogo variant="boot" />
        <LoaderCircle className="is-spinning" size={22} />
        <p>{text("正在准备 AT-Switch…", "Preparing AT-Switch…")}</p>
      </div>
    );
  }

  const activeAgent =
    snapshot.agents.find(
      (agent) => agent.id === activeAgentId && isSwitchableAgent(agent),
    ) ??
    snapshot.agents.find((agent) => isSwitchableAgent(agent));

  return (
    <div className={showWindowFrame ? "screenshot-window-wrapper" : undefined}>
      {showWindowFrame && (
        <div className="window-chrome">
          <div className="window-chrome__lights">
            <i />
            <i />
            <i />
          </div>
          <span className="window-chrome__title">AT-Switch</span>
        </div>
      )}
      <AppShell
        page={page}
        onNavigate={navigateTo}
        onBack={goBack}
        agents={snapshot.agents}
        activeAgentId={activeAgent?.id ?? "workbuddy"}
        refreshing={refreshing}
        onSelectAgent={selectAgent}
        onRefresh={() => void loadSnapshot(true)}
        onToggleLanguage={(nextLanguage) =>
          void updateSettings({ language: nextLanguage })
        }
      >
        {page === "overview" && activeAgent && (
          <SwitchboardPage
            key={activeAgent.id}
            agent={activeAgent}
            providers={snapshot.providers}
            testingId={testingId}
            switchingKey={switchingKey}
            onCreateProvider={() => {
              setEditingProvider(undefined);
              setProviderModal(true);
            }}
            onEditProvider={(provider) => {
              setEditingProvider(provider);
              setProviderModal(true);
            }}
            onDeleteModel={(provider, model) =>
              setDeletingModelTarget({ provider, model })
            }
            onTestProvider={(providerId, modelId) =>
              void testProvider(providerId, modelId)
            }
            onSwitchModel={(provider, model) =>
              quickSwitchModel(activeAgent, provider, model)
            }
            onRestoreNative={() => requestRestoreAgentNative(activeAgent)}
            platform={snapshot.platform}
            installPathBusy={installPathBusyAgentId === activeAgent.id}
            onSelectInstallPath={() => void selectAgentInstallPath(activeAgent)}
            onClearInstallPath={() => void clearAgentInstallPath(activeAgent)}
          />
        )}
        {page === "agents" && (
          <AgentsPage
            agents={snapshot.agents}
            onRefresh={() => void loadSnapshot(true)}
            onConfigure={(agent) =>
              setBindingTarget({ agent, mode: "direct" })
            }
            platform={snapshot.platform}
            installPathBusyAgentId={installPathBusyAgentId}
            onSelectInstallPath={(agent) => void selectAgentInstallPath(agent)}
            onClearInstallPath={(agent) => void clearAgentInstallPath(agent)}
          />
        )}
        {page === "providers" && (
          <ProvidersPage
            providers={snapshot.providers}
            testingId={testingId}
            onCreate={() => {
              setEditingProvider(undefined);
              setProviderModal(true);
            }}
            onEdit={(provider) => {
              setEditingProvider(provider);
              setProviderModal(true);
            }}
            onDelete={(provider) => setDeletingProvider(provider)}
            onTest={(id, modelId) => void testProvider(id, modelId)}
          />
        )}
        {page === "proxy" && (
          <ProxyPage
            proxy={snapshot.proxy}
            agents={snapshot.agents}
            busy={proxyBusy}
            onConfigureAgent={(agent) =>
              setBindingTarget({ agent, mode: "proxy" })
            }
            onStart={() => void mutateProxy("start")}
            onStop={() => void mutateProxy("stop")}
            onUpdatePort={(port) => void updatePort(port)}
          />
        )}
        {page === "settings" && (
          <SettingsPage
            settings={snapshot.settings}
            proxy={snapshot.proxy}
            proxyAgentCount={snapshot.agents.filter(
              (agent) => agent.mode === "proxy",
            ).length}
            onOpenProxy={() => navigateTo("proxy")}
            onUpdate={(settings) => void updateSettings(settings)}
          />
        )}
      </AppShell>

      <Modal
        open={Boolean(bindingTarget) && !pendingAgentAction}
        onClose={() => {
          if (!savingBinding) setBindingTarget(undefined);
        }}
        eyebrow={
          bindingTarget?.mode === "proxy"
            ? "ADVANCED PROXY ROUTING"
            : "AGENT CONFIGURATION"
        }
        title={
          bindingTarget
            ? bindingTarget.mode === "proxy"
              ? text(
                  `本地代理配置 ${bindingTarget.agent.displayName}`,
                  `Configure local proxy for ${bindingTarget.agent.displayName}`,
                )
              : text(
                  `${bindingTarget.agent.displayName} 配置详情`,
                  `${bindingTarget.agent.displayName} configuration details`,
                )
            : text("配置智能体", "Configure agent")
        }
      >
        {bindingTarget && (
          <AgentBindingForm
            key={`${bindingTarget.agent.id}:${bindingTarget.agent.providerId ?? "new"}:${bindingTarget.mode}`}
            agent={bindingTarget.agent}
            providers={snapshot.providers}
            mode={bindingTarget.mode}
            busy={savingBinding}
            platform={snapshot.platform}
            installPathBusy={
              installPathBusyAgentId === bindingTarget.agent.id
            }
            onSelectInstallPath={() =>
              void selectAgentInstallPath(bindingTarget.agent)
            }
            onClearInstallPath={() =>
              void clearAgentInstallPath(bindingTarget.agent)
            }
            onSubmit={(draft) =>
              requestApplyAgentBinding(bindingTarget.agent, draft)
            }
          />
        )}
      </Modal>

      <Modal
        open={providerModal}
        onClose={() => {
          if (!savingProvider) {
            setProviderModal(false);
            setEditingProvider(undefined);
          }
        }}
        eyebrow={editingProvider ? "EDIT MODEL SOURCE" : "NEW MODEL SOURCE"}
        title={
          editingProvider
            ? text(
                `编辑 ${editingProvider.name}`,
                `Edit ${editingProvider.name}`,
              )
            : text("新建模型供应商", "New model provider")
        }
      >
        <ProviderForm
          key={editingProvider?.id ?? "new"}
          initialProvider={editingProvider}
          loadMaskedApiKey={api.getProviderApiKeyMask}
          revealApiKey={api.revealProviderApiKey}
          onSubmit={(draft) => void saveProvider(draft)}
          busy={savingProvider}
        />
      </Modal>

      <Modal
        open={Boolean(deletingProvider)}
        title={text("删除模型供应商", "Delete Model Provider")}
        eyebrow="CONFIRM DELETE"
        onClose={() => {
          if (!deletingProviderBusy) setDeletingProvider(undefined);
        }}
      >
        <div className="delete-provider-modal">
          <p>
            {text(
              `确定要删除模型供应商“${deletingProvider?.name}”吗？此操作无法撤销，对应的 API Key 凭据都将被删除。`,
              `Are you sure you want to delete "${deletingProvider?.name}"? This action cannot be undone and associated API keys will be removed.`,
            )}
          </p>

          {deletingProviderAlertState.show && (
            <div className="affected-agents-alert">
              <p className="affected-agents-alert__title">
                <AlertTriangle size={15} />
                {text(
                  `当前有 ${deletingProviderAlertState.inUseAgents.length} 个智能体正在使用该供应商：`,
                  `Currently in use by ${deletingProviderAlertState.inUseAgents.length} agent(s):`,
                )}
              </p>
              <ul className="affected-agents-alert__list">
                {deletingProviderAlertState.inUseAgents.map((agent) => (
                  <li key={agent.id}>
                    <strong>{agent.displayName}</strong>
                    {agent.modelId && (
                      <span>
                        （{text("绑定模型：", "Model: ")}
                        {agent.modelId}）
                      </span>
                    )}
                  </li>
                ))}
              </ul>
              <p className="affected-agents-alert__note">
                {deletingProviderAlertState.mode === "unbind_only"
                  ? text(
                      "当前模型供应商正在使用中，您确定要删除吗？",
                      "This model provider is currently in use. Are you sure you want to delete it?",
                    )
                  : text(
                      "删除当前模型后上述智能体将自动恢复为官方默认配置，在新建会话或重新启动后生效。",
                      "Deleting this model will automatically restore the above agents to their native default configurations, taking effect on the next launch or new session.",
                    )}
              </p>
            </div>
          )}

          <div className="modal-actions-row">
            <button
              className="button button--secondary"
              onClick={() => setDeletingProvider(undefined)}
              disabled={deletingProviderBusy}
            >
              {text("取消", "Cancel")}
            </button>
            <button
              className="button button--danger"
              onClick={() => void confirmDeleteProvider()}
              disabled={deletingProviderBusy}
            >
              {deletingProviderBusy
                ? text("删除中…", "Deleting…")
                : text("确认删除", "Confirm Delete")}
            </button>
          </div>
        </div>
      </Modal>

      <Modal
        open={Boolean(deletingModelTarget)}
        title={text("删除模型", "Delete Model")}
        eyebrow="CONFIRM DELETE"
        onClose={() => {
          if (!deletingModelBusy) setDeletingModelTarget(undefined);
        }}
      >
        <div className="delete-provider-modal">
          <p>
            {text(
              `确定要删除模型“${deletingModelTarget?.model.displayName}”（${deletingModelTarget?.model.modelId}）吗？`,
              `Are you sure you want to delete the model "${deletingModelTarget?.model.displayName}" (${deletingModelTarget?.model.modelId})?`,
            )}
            {deletingModelTarget &&
              deletingModelTarget.provider.models.length <= 1 && (
                <>
                  <br />
                  <small className="field-error">
                    {text(
                      `这是“${deletingModelTarget.provider.name}”下的最后一个模型，删除后该模型供应商也将一并移除。`,
                      `This is the last model under "${deletingModelTarget.provider.name}". Deleting it will also remove the provider.`,
                    )}
                  </small>
                </>
              )}
          </p>

          {deletingModelAlertState.show && (
            <div className="affected-agents-alert">
              <p className="affected-agents-alert__title">
                <AlertTriangle size={15} />
                {text(
                  `当前有 ${deletingModelAlertState.inUseAgents.length} 个智能体正在使用该模型：`,
                  `Currently in use by ${deletingModelAlertState.inUseAgents.length} agent(s):`,
                )}
              </p>
              <ul className="affected-agents-alert__list">
                {deletingModelAlertState.inUseAgents.map((agent) => (
                  <li key={agent.id}>
                    <strong>{agent.displayName}</strong>
                    {agent.modelId && (
                      <span>
                        （{text("绑定模型：", "Model: ")}
                        {agent.modelId}）
                      </span>
                    )}
                  </li>
                ))}
              </ul>
              <p className="affected-agents-alert__note">
                {deletingModelAlertState.mode === "unbind_only"
                  ? text(
                      "当前模型正在使用中，您确定要删除吗？",
                      "This model is currently in use. Are you sure you want to delete it?",
                    )
                  : text(
                      "删除当前模型后上述智能体将自动恢复为官方默认配置，在新建会话或重新启动后生效。",
                      "Deleting this model will automatically restore the above agents to their native default configurations, taking effect on the next launch or new session.",
                    )}
              </p>
            </div>
          )}

          <div className="modal-actions-row">
            <button
              className="button button--secondary"
              onClick={() => setDeletingModelTarget(undefined)}
              disabled={deletingModelBusy}
            >
              {text("取消", "Cancel")}
            </button>
            <button
              className="button button--danger"
              onClick={() => void confirmDeleteModel()}
              disabled={deletingModelBusy}
            >
              {deletingModelBusy
                ? text("删除中…", "Deleting…")
                : text("确认删除", "Confirm Delete")}
            </button>
          </div>
        </div>
      </Modal>

      <AgentRestartConfirmation
        agent={pendingAgentAction?.agent}
        operation={pendingAgentAction?.kind ?? "apply"}
        onCancel={() => setPendingAgentAction(undefined)}
        onConfirm={confirmPendingAgentAction}
      />

      <div className="toast-stack" aria-live="polite">
        {toasts.map((toast) => (
          <div className={`toast toast--${toast.tone}`} key={toast.id}>
            {toast.tone === "good" ? (
              <CheckCircle2 size={18} />
            ) : (
              <AlertTriangle size={18} />
            )}
            <div>
              <strong>{toast.title}</strong>
              {toast.message && <span>{toast.message}</span>}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function App() {
  return (
    <LanguageProvider>
      <AppContent />
    </LanguageProvider>
  );
}

export default App;
