import { ArrowLeft, Boxes, Globe, Settings } from "lucide-react";
import clsx from "clsx";
import { useLanguage } from "../i18n";
import type { AgentSummary, AppLanguage, PageId } from "../types";
import { AgentSwitcher } from "./AgentSwitcher";
import { BrandLogo } from "./BrandLogo";

interface AppShellProps {
  page: PageId;
  onNavigate: (page: PageId) => void;
  onNavigateToModel: () => void;
  onToggleLanguage: (language: AppLanguage) => void;
  onBack: () => void;
  agents: AgentSummary[];
  activeAgentId: string;
  onSelectAgent: (agentId: string) => void;
  children: React.ReactNode;
}

export function AppShell({
  page,
  onNavigate,
  onNavigateToModel,
  onToggleLanguage,
  onBack,
  agents,
  activeAgentId,
  onSelectAgent,
  children,
}: AppShellProps) {
  const { language, setLanguage, text } = useLanguage();

  const toggleLanguage = () => {
    const next = language === "zh-CN" ? "en" : "zh-CN";
    setLanguage(next);
    onToggleLanguage(next);
  };

  return (
    <div className="desktop-shell">
      <header className="desktop-toolbar">
        <div className="desktop-brand-cluster">
          {page !== "overview" && (
            <button
              type="button"
              className="toolbar-back-button"
              onClick={onBack}
              aria-label={text("返回上一页", "Go back")}
              title={text("返回上一页", "Go back")}
            >
              <ArrowLeft size={19} strokeWidth={1.9} />
            </button>
          )}
          <button
            type="button"
            className="desktop-brand"
            onClick={() => onNavigate("overview")}
            aria-label={text("返回模型切换", "Return to model switchboard")}
          >
            <BrandLogo variant="toolbar" />
            <strong>AT-Switch</strong>
          </button>
        </div>

        <AgentSwitcher
          agents={agents}
          activeAgentId={activeAgentId}
          onSwitch={onSelectAgent}
        />

        <div className="desktop-header-right">
          <nav
            className="desktop-actions"
            aria-label={text("工具导航", "Toolbar navigation")}
          >
            <button
              type="button"
              className={clsx(
                "desktop-action",
                page === "settings" && "is-active",
              )}
              onClick={onNavigateToModel}
              title={text("模型供应商与大模型", "Model providers & LLMs")}
            >
              <Boxes size={17} strokeWidth={1.8} aria-hidden="true" />
              <span>{text("模型", "Models")}</span>
            </button>
            <span className="desktop-actions__sep" aria-hidden="true" />
            <button
              type="button"
              className={clsx(
                "desktop-action",
                page === "settings" && "is-active",
              )}
              onClick={() => onNavigate("settings")}
              title={text("打开设置中心", "Open settings")}
            >
              <Settings size={17} strokeWidth={1.8} aria-hidden="true" />
              <span>{text("设置", "Settings")}</span>
            </button>
            <span className="desktop-actions__sep" aria-hidden="true" />
            <button
              type="button"
              className="desktop-action"
              onClick={toggleLanguage}
              title={
                language === "zh-CN"
                  ? "切换界面语言为 English"
                  : "Switch language to 简体中文"
              }
            >
              <Globe size={17} strokeWidth={1.8} aria-hidden="true" />
              <span>{language === "zh-CN" ? "中文" : "EN"}</span>
            </button>
          </nav>
        </div>
      </header>

      <main className={clsx("workspace", `workspace--${page}`)}>{children}</main>
    </div>
  );
}
