import {
  ArrowLeft,
  Bot,
  Boxes,
  Globe,
  RefreshCw,
  Settings,
} from "lucide-react";
import clsx from "clsx";
import { useLanguage } from "../i18n";
import type { AgentSummary, AppLanguage, PageId } from "../types";
import { AgentSwitcher } from "./AgentSwitcher";
import { BrandLogo } from "./BrandLogo";

interface AppShellProps {
  page: PageId;
  onNavigate: (page: PageId) => void;
  onBack: () => void;
  agents: AgentSummary[];
  activeAgentId: string;
  refreshing: boolean;
  onSelectAgent: (agentId: string) => void;
  onRefresh: () => void;
  onToggleLanguage?: (language: AppLanguage) => void;
  children: React.ReactNode;
}

export function AppShell({
  page,
  onNavigate,
  onBack,
  agents,
  activeAgentId,
  refreshing,
  onSelectAgent,
  onRefresh,
  onToggleLanguage,
  children,
}: AppShellProps) {
  const { language, setLanguage, text } = useLanguage();
  const navigation: Array<{
    id: PageId;
    label: string;
    tooltip: string;
    icon: typeof Bot;
  }> = [
    {
      id: "agents",
      label: text("智能体状态", "Agent status"),
      tooltip: text("查看智能体状态", "View agent status"),
      icon: Bot,
    },
    {
      id: "providers",
      label: text("模型供应商与大模型", "Model providers & LLMs"),
      tooltip: text("管理模型供应商与大模型", "Manage model providers & LLMs"),
      icon: Boxes,
    },
    {
      id: "settings",
      label: text("高级设置", "Advanced settings"),
      tooltip: text("打开高级设置", "Open advanced settings"),
      icon: Settings,
    },
  ];

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
              className="toolbar-icon-button"
              onClick={onRefresh}
              aria-label={text("刷新状态", "Refresh status")}
              title={text("刷新状态", "Refresh status")}
              disabled={refreshing}
            >
              <RefreshCw
                size={25}
                className={refreshing ? "is-spinning" : undefined}
              />
            </button>
            {navigation.map((item) => {
              const Icon = item.icon;
              return (
                <button
                  key={item.id}
                  type="button"
                  className={clsx(
                    "toolbar-icon-button",
                    page === item.id && "is-active",
                  )}
                  onClick={() => onNavigate(item.id)}
                  aria-label={item.label}
                  title={item.tooltip}
                >
                  <Icon size={25} strokeWidth={1.8} />
                </button>
              );
            })}
          </nav>

          <button
            type="button"
            className="standalone-language-switch"
            onClick={() => {
              const nextLanguage = language === "zh-CN" ? "en" : "zh-CN";
              setLanguage(nextLanguage);
              onToggleLanguage?.(nextLanguage);
            }}
            aria-label={
              language === "zh-CN"
                ? "切换界面语言为 English"
                : "Switch language to 简体中文"
            }
            title={
              language === "zh-CN"
                ? "切换界面语言为 English"
                : "Switch language to 简体中文"
            }
          >
            <Globe size={14} />
            <span>{language === "zh-CN" ? "中文" : "EN"}</span>
          </button>
        </div>
      </header>

      <main className={clsx("workspace", `workspace--${page}`)}>{children}</main>
    </div>
  );
}
