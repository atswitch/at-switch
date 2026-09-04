import {
  ArrowRight,
  Moon,
  Network,
  Power,
  Sun,
} from "lucide-react";
import { useLanguage } from "../i18n";
import type { AppSettings, ProxyStatus } from "../types";
import { PageHeader } from "../components/PageHeader";

interface SettingsPageProps {
  settings: AppSettings;
  proxy: ProxyStatus;
  proxyAgentCount: number;
  onOpenProxy: () => void;
  onUpdate: (settings: Partial<AppSettings>) => void;
}

export function SettingsPage({
  settings,
  proxy,
  proxyAgentCount,
  onOpenProxy,
  onUpdate,
}: SettingsPageProps) {
  const proxyRunning = proxy.status === "running";
  const { text } = useLanguage();

  return (
    <>
      <PageHeader
        eyebrow="DESKTOP SYSTEM / 05"
        title={text("高级设置", "Advanced settings")}
        description={text(
          "管理界面主题、应用生命周期，以及仅在特殊兼容场景下需要的本地代理。",
          "Manage appearance, app lifecycle, and the local proxy for advanced compatibility scenarios.",
        )}
      />

      <section className="settings-grid">
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
            label={text("关闭窗口后继续运行", "Keep running after closing the window")}
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

        <article className="settings-section settings-section--proxy">
          <div className="settings-section__title">
            <Network size={19} />
            <div>
              <h2>{text("本地代理", "Local proxy")}</h2>
              <p>
                {text(
                  "用于跨协议转换或避免把真实 API Key 写入智能体配置。",
                  "Use protocol conversion or keep the real API key out of agent configuration.",
                )}
              </p>
            </div>
          </div>
          <div className="advanced-proxy-summary">
            <span className={proxyRunning ? "is-running" : undefined}>
              {proxyRunning
                ? text("运行中", "Running")
                : text("已停止", "Stopped")}
            </span>
            <strong>
              {proxy.host}:{proxy.port}
            </strong>
            <small>
              {text(
                `${proxyAgentCount} 个智能体使用代理接管`,
                `${proxyAgentCount} agent${proxyAgentCount === 1 ? "" : "s"} routed through the proxy`,
              )}
            </small>
          </div>
          <p className="advanced-proxy-note">
            {text(
              "协议相同时请优先使用首页直连。代理会增加一个本地运行依赖，退出 AT-Switch 后由代理接管的智能体将无法继续请求。",
              "Prefer direct mode when protocols match. The proxy adds a local runtime dependency; agents routed through it cannot make requests after AT-Switch exits.",
            )}
          </p>
          <button
            className="button button--secondary advanced-proxy-action"
            type="button"
            onClick={onOpenProxy}
          >
            {text("打开本地代理设置", "Open local proxy settings")}
            <ArrowRight size={16} />
          </button>
        </article>
      </section>

      <footer className="about-strip">
        <span>AT-SWITCH / LOCAL FIRST</span>
        <strong>v3.14.1</strong>
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
