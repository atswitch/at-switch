import clsx from "clsx";
import { Check, MoreHorizontal, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useLanguage } from "../i18n";
import { isSwitchableAgent } from "../lib/agentCapabilities";
import { agentAvailabilityLabel } from "../lib/format";
import {
  getRecentAgentIds,
  recordAgentSelection,
} from "../lib/recentAgents";
import type { AgentSummary } from "../types";
import { AgentLogo } from "./AgentLogo";

/** 胶囊内直接展示的智能体数量；超出部分收进「更多」弹窗。 */
const VISIBLE_COUNT = 6;

/** 把最近选中使用过的智能体排到前面，其余保持原有相对顺序。 */
function orderByRecent(
  agents: AgentSummary[],
  recentIds: string[],
): AgentSummary[] {
  if (recentIds.length === 0) return agents;
  const rank = new Map(recentIds.map((id, index) => [id, index]));
  const recent = agents
    .filter((agent) => rank.has(agent.id))
    .sort(
      (a, b) => (rank.get(a.id) ?? 0) - (rank.get(b.id) ?? 0),
    );
  const rest = agents.filter((agent) => !rank.has(agent.id));
  return [...recent, ...rest];
}

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
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [recentIds, setRecentIds] = useState(getRecentAgentIds());
  const rootRef = useRef<HTMLDivElement>(null);

  const selectableAgents = useMemo(
    () => agents.filter(isSwitchableAgent),
    [agents],
  );
  const activeAgent =
    selectableAgents.find((agent) => agent.id === activeAgentId) ??
    selectableAgents[0];

  const orderedAgents = useMemo(
    () => orderByRecent(selectableAgents, recentIds),
    [selectableAgents, recentIds],
  );

  const visibleAgents = orderedAgents.slice(0, VISIBLE_COUNT);
  const hiddenCount = Math.max(0, orderedAgents.length - VISIBLE_COUNT);

  const filteredAgents = useMemo(() => {
    const query = search.trim().toLowerCase();
    if (!query) return selectableAgents;
    return selectableAgents.filter((agent) =>
      agent.displayName.toLowerCase().includes(query),
    );
  }, [selectableAgents, search]);

  // 搜索时按匹配结果；否则按最近使用排序。
  const listAgents = search.trim() ? filteredAgents : orderedAgents;

  const handleSwitch = (agentId: string) => {
    setRecentIds(recordAgentSelection(agentId));
    onSwitch(agentId);
  };

  // 点击弹窗外或按 Esc 时关闭。
  useEffect(() => {
    if (!open) return;
    const handlePointerDown = (event: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [open]);

  const statusHint = (agent: AgentSummary) =>
    agentAvailabilityLabel(agent.installStatus, agent.runtimeStatus, language);

  if (!activeAgent) return null;

  const renderIcon = (agent: AgentSummary) => {
    const installed = agent.installStatus !== "not_installed";
    const ready = installed && agent.adapterVerified;
    return (
      <span className="agent-switcher__icon">
        <AgentLogo agentId={agent.id} />
        <i
          className={clsx(
            "agent-switcher__status",
            ready && "is-ready",
            installed && !ready && "is-warning",
          )}
          aria-hidden="true"
        />
      </span>
    );
  };

  return (
    <div className="agent-switcher" ref={rootRef}>
      <div
        className="agent-switcher__capsule"
        role="tablist"
        aria-label={text("选择智能体", "Select agent")}
      >
        {visibleAgents.map((agent) => {
          const isActive = agent.id === activeAgent.id;
          return (
            <button
              key={agent.id}
              type="button"
              role="tab"
              className={clsx(
                "agent-switcher__item",
                isActive && "is-active",
              )}
              aria-selected={isActive}
              title={`${agent.displayName} · ${statusHint(agent)}`}
              onClick={() => handleSwitch(agent.id)}
            >
              {renderIcon(agent)}
              <span>{agent.displayName}</span>
            </button>
          );
        })}

        {hiddenCount > 0 && (
          <button
            type="button"
            className={clsx("agent-switcher__more", open && "is-active")}
            aria-expanded={open}
            aria-label={text("更多智能体", "More agents")}
            title={text(
              `还有 ${hiddenCount} 个智能体`,
              `${hiddenCount} more agents`,
            )}
            onClick={() => setOpen((current) => !current)}
          >
            <MoreHorizontal size={16} />
            <span className="agent-switcher__more-count">+{hiddenCount}</span>
          </button>
        )}
      </div>

      {open && (
        <div
          className="agent-switcher__popover"
          role="dialog"
          aria-label={text("选择智能体", "Select agent")}
        >
          <div className="agent-switcher__popover-head">
            <strong>{text("选择智能体", "Select agent")}</strong>
            <span className="agent-switcher__popover-count">
              {text(
                `${selectableAgents.length} 个`,
                `${selectableAgents.length}`,
              )}
            </span>
          </div>

          <label className="agent-switcher__search">
            <Search size={14} aria-hidden="true" />
            <input
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={text("搜索智能体…", "Search agents…")}
            />
          </label>

          <div className="agent-switcher__popover-list">
            {listAgents.map((agent) => {
              const isActive = agent.id === activeAgent.id;
              return (
                <button
                  key={agent.id}
                  type="button"
                  className={clsx(
                    "agent-switcher__popover-item",
                    isActive && "is-active",
                  )}
                  onClick={() => {
                    handleSwitch(agent.id);
                    setOpen(false);
                  }}
                >
                  {renderIcon(agent)}
                  <span className="agent-switcher__popover-text">
                    <span className="agent-switcher__popover-name">
                      {agent.displayName}
                    </span>
                    <span className="agent-switcher__popover-meta">
                      {agent.detectedVersion
                        ? `v${agent.detectedVersion} · `
                        : ""}
                      {statusHint(agent)}
                    </span>
                  </span>
                  {isActive && (
                    <Check
                      size={16}
                      className="agent-switcher__popover-check"
                      aria-hidden="true"
                    />
                  )}
                </button>
              );
            })}
            {filteredAgents.length === 0 && (
              <p className="agent-switcher__popover-empty">
                {text("没有匹配的智能体", "No matching agents")}
              </p>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
