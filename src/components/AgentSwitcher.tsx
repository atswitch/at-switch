import clsx from "clsx";
import { useId } from "react";
import { useLanguage } from "../i18n";
import { isSwitchableAgent } from "../lib/agentCapabilities";
import { agentAvailabilityLabel } from "../lib/format";
import type { AgentSummary } from "../types";
import { AgentLogo } from "./AgentLogo";

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
  const selectableAgents = agents.filter(isSwitchableAgent);
  const activeAgent =
    selectableAgents.find((agent) => agent.id === activeAgentId) ??
    selectableAgents[0];
  const tablistId = `${useId()}-agent-switcher`;

  const statusHint = (agent: AgentSummary) =>
    agentAvailabilityLabel(agent.installStatus, agent.runtimeStatus, language);

  if (!activeAgent) {
    return null;
  }

  return (
    <div
      id={tablistId}
      className="agent-switcher"
      role="tablist"
      aria-label={text("选择智能体", "Select agent")}
    >
      {selectableAgents.map((agent) => {
        const isActive = agent.id === activeAgent.id;
        const installed = agent.installStatus !== "not_installed";
        const ready = installed && agent.adapterVerified;

        return (
          <button
            key={agent.id}
            type="button"
            role="tab"
            className={clsx("agent-switcher__item", isActive && "is-active")}
            aria-selected={isActive}
            title={`${agent.displayName} · ${statusHint(agent)}`}
            onClick={() => onSwitch(agent.id)}
          >
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
            <span>{agent.displayName}</span>
          </button>
        );
      })}
    </div>
  );
}
