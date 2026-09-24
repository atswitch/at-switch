import { useMemo, useState } from "react";
import {
  Network,
  Power,
  SlidersHorizontal,
  Sun,
  Moon,
  Info,
  type LucideIcon,
} from "lucide-react";
import { useLanguage } from "../i18n";
import type {
  AgentSummary,
  AppSettings,
  ProxyStatus,
} from "../types";
import { proxyAgentUsage } from "../lib/proxyUsage";
import { PageHeader } from "../components/PageHeader";
import { AgentRoutingGrid } from "../components/AgentRoutingGrid";
import { AboutSettings } from "../components/AboutSettings";
import { ProxyListenerSettings } from "../components/ProxyListenerSettings";
import { ProxyRecentRequests } from "../components/ProxyRecentRequests";
import { ProxyStatusBar } from "../components/ProxyStatusBar";
import { ProxyUsageByAgent } from "../components/ProxyUsageByAgent";

export type SettingsTab = "general" | "proxy" | "about";

interface SettingsPageProps {
  appVersion: string;
  platform: string;
  settings: AppSettings;
  proxy: ProxyStatus;
  agents: AgentSummary[];
  proxyAgentCount: number;
  proxyBusy: boolean;
  proxyBusyAgentId?: string;
  /** 从别处跳转过来时直接展开的页签，例如首页要求使用本地代理。 */
  initialTab?: SettingsTab;
  onUpdate: (settings: Partial<AppSettings>) => void;
  onStartProxy: () => void;
  onStopProxy: () => void;
  onUpdateProxyPort: (port: number) => void;
  onToggleProxyPref: (agent: AgentSummary, enabled: boolean) => void;
  onConfigureProxy?: (agent: AgentSummary) => void;
}

export function SettingsPage({
  appVersion,
  platform,
  settings,
  proxy,
  agents,
  proxyAgentCount,
  proxyBusy,
  proxyBusyAgentId,
  initialTab = "general",
  onUpdate,
  onStartProxy,
  onStopProxy,
  onUpdateProxyPort,
  onToggleProxyPref,
  onConfigureProxy,
}: SettingsPageProps) {
  const { text } = useLanguage();
  const [tab, setTab] = useState<SettingsTab>(initialTab);
  const agentUsage = useMemo(() => proxyAgentUsage(proxy, agents), [
    proxy,
    agents,
  ]);

  const tabs: { id: SettingsTab; label: string; icon: LucideIcon }[] = [
    {
      id: "general",
      label: text("外观与生命周期", "Appearance & lifecycle"),
      icon: SlidersHorizontal,
    },
    { id: "proxy", label: text("本地代理", "Local proxy"), icon: Network },
    { id: "about", label: text("关于", "About"), icon: Info },
  ];

  return (
    <>
      <PageHeader
        eyebrow="DESKTOP SYSTEM / 05"
        title={text("高级设置", "Advanced settings")}
        description={text(
          "按类别管理：外观与应用生命周期、本地代理，以及代理的使用统计。",
          "Manage by category: appearance and lifecycle, the local proxy, and its usage statistics.",
        )}
      />

      <div
        className="settings-tabs"
        role="tablist"
        aria-label={text("设置分类", "Settings sections")}
      >
        {tabs.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={tab === id}
            className={`settings-tab ${tab === id ? "is-active" : ""}`}
            onClick={() => setTab(id)}
          >
            <Icon size={15} />
            {label}
          </button>
        ))}
      </div>

      <section className={`settings-grid ${tab === "general" ? "" : "settings-grid--single"}`}>
        {tab === "general" && (
          <>
            <article className="settings-section">
              <div className="settings-section__title">
                <Sun size={19} />
                <div>
                  <h2>{text("界面", "Appearance")}</h2>
                  <p>
                    {text(
                      "选择明暗主题，默认跟随系统。",
                      "Choose a light or dark theme. The default follows your system.",
                    )}
                  </p>
                </div>
              </div>
              <div
                className="segmented"
                role="group"
                aria-label={text("主题", "Theme")}
              >
                {(["system", "light", "dark"] as const).map((theme) => (
                  <button
                    key={theme}
                    className={settings.theme === theme ? "is-active" : ""}
                    onClick={() => onUpdate({ theme })}
                  >
                    {theme === "system" && text("跟随系统", "System")}
                    {theme === "light" && text("浅色", "Light")}
                    {theme === "dark" && (
                      <>
                        <Moon size={14} /> {text("深色", "Dark")}
                      </>
                    )}
                  </button>
                ))}
              </div>
            </article>

            <article className="settings-section">
              <div className="settings-section__title">
                <Power size={19} />
                <div>
                  <h2>{text("应用生命周期", "App lifecycle")}</h2>
                  <p>
                    {text(
                      "使用高级代理功能时建议保持后台常驻。",
                      "Keep the app running in the background when using the advanced proxy.",
                    )}
                  </p>
                </div>
              </div>
              <SettingToggle
                label={text("登录时启动 AT-Switch", "Launch AT-Switch at login")}
                description={text(
                  "当前仅启动桌面应用；本地代理仍需手动启动。",
                  "This launches only the desktop app; the local proxy still starts manually.",
                )}
                checked={settings.startAtLogin}
                onChange={(startAtLogin) => onUpdate({ startAtLogin })}
              />
              <SettingToggle
                label={text(
                  "关闭窗口后继续运行",
                  "Keep running after closing the window",
                )}
                description={text(
                  "主窗口隐藏到系统托盘或菜单栏。",
                  "Hide the main window in the system tray or menu bar.",
                )}
                checked={settings.keepRunningInBackground}
                onChange={(keepRunningInBackground) =>
                  onUpdate({ keepRunningInBackground })
                }
              />
            </article>
          </>
        )}

        {tab === "proxy" && (
          <>
            <ProxyStatusBar
              proxy={proxy}
              proxyAgentCount={proxyAgentCount}
              busy={proxyBusy}
              onStart={onStartProxy}
              onStop={onStopProxy}
            />

            <ProxyListenerSettings
              proxy={proxy}
              onUpdatePort={onUpdateProxyPort}
            />

            <article className="panel">
              <div className="panel__header">
                <div>
                  <p className="eyebrow">AGENT ROUTING</p>
                  <h2>{text("智能体用量统计", "Agent usage telemetry")}</h2>
                </div>
                <span className="mono-counter">
                  {agents.filter((agent) => agent.proxyPrefEnabled).length}/
                  {agents.length}
                </span>
              </div>
              <p className="panel__hint">
                {text(
                  "选择哪些智能体的流量经过本地代理并被统计用量；不影响已切换的模型。",
                  "Choose which agents route through the local proxy for usage telemetry. The selected model is unaffected.",
                )}
              </p>
              <AgentRoutingGrid
                agents={agents}
                busyAgentId={proxyBusyAgentId}
                usageByAgent={agentUsage}
                onToggleProxyPref={onToggleProxyPref}
                onConfigureProxy={onConfigureProxy}
              />
            </article>

            <ProxyUsageByAgent proxy={proxy} usageByAgent={agentUsage} />

            <ProxyRecentRequests proxy={proxy} />
          </>
        )}

        {tab === "about" && (
          <AboutSettings appVersion={appVersion} platform={platform} />
        )}
      </section>

      <footer className="about-strip">
        <span>AT-SWITCH / LOCAL FIRST</span>
        <strong>v3.14.2</strong>
        <span>Windows x64 · macOS Universal</span>
      </footer>
    </>
  );
}

function SettingToggle({
  label,
  description,
  checked,
  onChange,
}: {
  label: string;
  description: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="setting-toggle">
      <span>
        <strong>{label}</strong>
        <small>{description}</small>
      </span>
      <button
        className={`toggle ${checked ? "is-active" : ""}`}
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
      >
        <span />
      </button>
    </label>
  );
}
