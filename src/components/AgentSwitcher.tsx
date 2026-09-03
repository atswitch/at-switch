import clsx from "clsx";
import { useLanguage } from "../i18n";
import { isSwitchableAgent } from "../lib/agentCapabilities";
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
  return (
    <div
      className="agent-switcher"
      role="tablist"
      aria-label={text("选择智能体", "Select agent")}
    >
      {agents.filter(isSwitchableAgent).map((agent) => {
        const installed = agent.installStatus !== "not_installed";
        const active = agent.id === activeAgentId;
        const ready = installed && agent.adapterVerified;

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
            title={
              ready
                ? text(
                    `${agent.displayName} 已就绪`,
                    `${agent.displayName} is ready`,
                  )
                : !installed
                  ? text(
                      `${agent.displayName} 未安装`,
                      `${agent.displayName} is not installed`,
                    )
                  : language === "zh-CN" && agent.message
                    ? agent.message
                    : text(
                        `${agent.displayName} 尚不可配置`,
                        `${agent.displayName} is unavailable`,
                      )
            }
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
  );
}
