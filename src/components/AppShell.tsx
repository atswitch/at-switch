import { ArrowLeft, CircleUserRound, Globe } from "lucide-react";
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
  onSelectAgent: (agentId: string) => void;
  onToggleLanguage?: (language: AppLanguage) => void;
  /** 会员入口的无障碍名称与悬浮提示（未登录显示「未登录 / 点击登录」）。 */
  memberLabel: string;
  /** 会员入口可见短标签：登录时显示用户名，未登录显示「登录」。 */
  memberDisplayLabel: string;
  /** 有新版本时在会员入口显示提醒红点。 */
  updateBadge?: boolean;
  /** 待更新版本号：有值时在登录名右侧显示「v3.18.1 可更新」提醒。 */
  updateVersion?: string;
  onOpenMemberCenter: () => void;
  children: React.ReactNode;
}

export function AppShell({
  page,
  onNavigate,
  onBack,
  agents,
  activeAgentId,
  onSelectAgent,
  onToggleLanguage,
  memberLabel,
  memberDisplayLabel,
  updateBadge,
  updateVersion,
  onOpenMemberCenter,
  children,
}: AppShellProps) {
  const { language, setLanguage, text } = useLanguage();

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
                "toolbar-icon-button",
                "member-entry",
                "member-entry--labeled",
              )}
              onClick={onOpenMemberCenter}
              aria-label={memberLabel}
              title={memberLabel}
            >
              <CircleUserRound size={25} strokeWidth={1.8} />
              <span className="member-entry__label">{memberDisplayLabel}</span>
              {updateVersion && (
                <span className="member-entry__update">
                  v{updateVersion} {text("可更新", "available")}
                </span>
              )}
              {updateBadge && !updateVersion && (
                <i className="member-entry__dot" aria-hidden="true" />
              )}
            </button>

            <span className="toolbar-divider" aria-hidden="true" />

            {/* 语言切换：并入同一胶囊，纯图标与其他工具按钮等高 */}
            <button
              type="button"
              className="toolbar-icon-button toolbar-language"
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
              <Globe size={25} strokeWidth={1.8} />
            </button>
          </nav>
        </div>
      </header>

      <main className={clsx("workspace", `workspace--${page}`)}>{children}</main>
    </div>
  );
}
