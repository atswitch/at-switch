import { Check, ChevronDown, Ellipsis } from "lucide-react";
import clsx from "clsx";
import { useEffect, useMemo, useRef, useState } from "react";
import { useLanguage } from "../i18n";
import { isSwitchableAgent } from "../lib/agentCapabilities";
import type { AgentSummary } from "../types";
import { AgentLogo } from "./AgentLogo";

const MAX_VISIBLE_AGENTS = 6;

interface AgentSwitcherProps {
  agents: AgentSummary[];
  activeAgentId: string;
  onSwitch: (agentId: string) => void;
}

export function AgentSwitcher({
  agents,
  activeAgentId,
  onSwitch,
}: AgentSwitcherProps) {
  const { language, text } = useLanguage();
  const [menuOpen, setMenuOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const switchableAgents = useMemo(
    () => agents.filter(isSwitchableAgent),
    [agents],
  );
  const { visibleAgents, hiddenAgents } = useMemo(() => {
    if (switchableAgents.length <= MAX_VISIBLE_AGENTS) {
      return { visibleAgents: switchableAgents, hiddenAgents: [] };
    }
    const visible = switchableAgents.slice(0, MAX_VISIBLE_AGENTS);
    const visibleIds = new Set(visible.map((agent) => agent.id));
    return {
      visibleAgents: visible,
      hiddenAgents: switchableAgents.filter(
        (agent) => !visibleIds.has(agent.id),
      ),
    };
  }, [switchableAgents]);

  useEffect(() => setMenuOpen(false), [activeAgentId]);

  useEffect(() => {
    if (!menuOpen) return;
    const closeOnOutsideClick = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setMenuOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setMenuOpen(false);
    };
    document.addEventListener("pointerdown", closeOnOutsideClick);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsideClick);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [menuOpen]);

  const agentState = (agent: AgentSummary) => {
    const installed = agent.installStatus !== "not_installed";
    return {
      installed,
      ready: installed && agent.adapterVerified,
      active: agent.id === activeAgentId,
    };
  };

  const agentTitle = (agent: AgentSummary) => {
    const { installed, ready } = agentState(agent);
    return ready
      ? text(`${agent.displayName} 已就绪`, `${agent.displayName} is ready`)
      : !installed
        ? text(`${agent.displayName} 未安装`, `${agent.displayName} is not installed`)
        : language === "zh-CN" && agent.message
          ? agent.message
          : text(
              `${agent.displayName} 尚不可配置`,
              `${agent.displayName} is unavailable`,
            );
  };

  return (
    <div className="agent-switcher" ref={rootRef}>
      <div
        className="agent-switcher__tabs"
        role="tablist"
        aria-label={text("选择智能体", "Select agent")}
      >
        {visibleAgents.map((agent) => {
          const { installed, ready, active } = agentState(agent);
          return (
            <button
              key={agent.id}
              type="button"
              role="tab"
              aria-selected={active}
              className={clsx(
                "agent-switcher__item",
                active && "is-active",
              )}
              onClick={() => onSwitch(agent.id)}
              title={agentTitle(agent)}
            >
              <span className="agent-switcher__icon">
                <AgentLogo agentId={agent.id} />
                <i
                  className={clsx(
                    "agent-switcher__status",
                    ready ? "is-ready" : installed ? "is-warning" : "",
                  )}
                  aria-hidden="true"
                />
              </span>
              <span>{agent.displayName}</span>
            </button>
          );
        })}
      </div>

      {hiddenAgents.length > 0 && (
        <div className="agent-switcher__overflow">
          <button
            type="button"
            className={clsx(
              "agent-switcher__more",
              menuOpen && "is-open",
            )}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            aria-label={text(
              `更多智能体，${hiddenAgents.length} 个`,
              `${hiddenAgents.length} more agents`,
            )}
            onClick={() => setMenuOpen((open) => !open)}
          >
            <Ellipsis size={17} aria-hidden="true" />
            <span>{text("更多", "More")}</span>
            <b>{hiddenAgents.length}</b>
            <ChevronDown size={14} aria-hidden="true" />
          </button>

          {menuOpen && (
            <div
              className="agent-switcher__menu"
              role="menu"
              aria-label={text("更多智能体", "More agents")}
            >
              <div className="agent-switcher__menu-heading">
                <span>{text("选择智能体", "Select agent")}</span>
                <small>{switchableAgents.length}</small>
              </div>
              {hiddenAgents.map((agent) => {
                const { installed, ready, active } = agentState(agent);
                return (
                  <button
                    key={agent.id}
                    type="button"
                    role="menuitemradio"
                    aria-checked={active}
                    className={clsx(
                      "agent-switcher__menu-item",
                      active && "is-active",
                    )}
                    title={agentTitle(agent)}
                    onClick={() => {
                      onSwitch(agent.id);
                      setMenuOpen(false);
                    }}
                  >
                    <span className="agent-switcher__icon">
                      <AgentLogo agentId={agent.id} />
                      <i
                        className={clsx(
                          "agent-switcher__status",
                          ready
                            ? "is-ready"
                            : installed
                              ? "is-warning"
                              : "",
                        )}
                        aria-hidden="true"
                      />
                    </span>
                    <span>
                      <strong>{agent.displayName}</strong>
                      <small>{agentTitle(agent)}</small>
                    </span>
                    {active && <Check size={16} aria-hidden="true" />}
                  </button>
                );
              })}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
