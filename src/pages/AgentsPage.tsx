import {
  FolderOpen,
  FolderSearch,
  LoaderCircle,
  RotateCcw,
} from "lucide-react";
import clsx from "clsx";
import { useLanguage } from "../i18n";
import type { AgentSummary } from "../types";
import { agentAvailabilityLabel, agentHealthLabel } from "../lib/format";
import { PageHeader } from "../components/PageHeader";
import { StatusPill } from "../components/StatusPill";
import { AgentLogo } from "../components/AgentLogo";

interface AgentsPageProps {
  agents: AgentSummary[];
  onRefresh: () => void;
  onConfigure: (agent: AgentSummary) => void;
  platform?: string;
  installPathBusyAgentId?: string;
  onSelectInstallPath?: (agent: AgentSummary) => void;
  onClearInstallPath?: (agent: AgentSummary) => void;
}

export function AgentsPage({
  agents,
  onRefresh,
  onConfigure,
  platform = "unknown",
  installPathBusyAgentId,
  onSelectInstallPath = () => undefined,
  onClearInstallPath = () => undefined,
}: AgentsPageProps) {
  const { language, text } = useLanguage();
  const visibleAgents = agents.filter(
    (agent) => agent.id !== "ima" && agent.id !== "trae",
  );
  const supportsInstallSelection =
    platform === "macos" || platform === "windows";

  return (
    <>
      <PageHeader
        eyebrow={`TARGET ADAPTERS / ${String(visibleAgents.length).padStart(2, "0")}`}
        title={text("智能体", "Agents")}
        description={text(
          "检测本机安装与配置状态，并将已验证的模型供应商和模型安全分发给目标智能体。",
          "Detect local installation and configuration status, then safely distribute verified providers and models to each agent.",
        )}
        actions={
          <button className="button button--secondary" onClick={onRefresh}>
            <FolderSearch size={16} />
            {text("刷新状态", "Refresh status")}
          </button>
        }
      />

      <section className="agent-list">
        {visibleAgents.map((agent, index) => {
          const healthy =
            agent.adapterVerified && agent.configHealth === "healthy";
          const installed = agent.installStatus !== "not_installed";
          return (
            <article
              className={clsx("agent-row", !installed && "is-unavailable")}
              key={agent.id}
              title={language === "zh-CN" ? agent.message : undefined}
            >
              <div className="agent-row__index">0{index + 1}</div>
              <div className="agent-row__identity">
                <AgentLogo
                  agentId={agent.id}
                  className="agent-logo--agent-row"
                />
                <div>
                  <h2>{agent.displayName}</h2>
                  <p>
                    {agent.customInstallPath &&
                    !agent.usingCustomInstallPath ? (
                      <span title={agent.customInstallPath}>
                        {text(
                          "自定义位置失效，已自动发现",
                          "Custom location unavailable; using automatic discovery",
                        )}
                      </span>
                    ) : agent.usingCustomInstallPath ? (
                      <span title={agent.installPath}>
                        {text("自定义安装位置", "Custom installation")}
                      </span>
                    ) : agent.detectedVersion ? (
                      <span>v{agent.detectedVersion}</span>
                    ) : installed ? (
                      text("已检测，版本未知", "Detected, version unknown")
                    ) : (
                      text("本机未安装", "Not installed")
                    )}
                  </p>
                </div>
              </div>
              <div className="agent-row__state">
                <small>{text("安装状态", "Installation")}</small>
                <strong>
                  {agentAvailabilityLabel(
                    agent.installStatus,
                    agent.runtimeStatus,
                    language,
                  )}
                </strong>
              </div>
              <div className="agent-row__state">
                <small>{text("配置健康", "Configuration health")}</small>
                <strong>
                  {agentHealthLabel(agent.configHealth, language)}
                </strong>
              </div>
              <div className="agent-row__binding">
                <small>{text("当前路由", "Current route")}</small>
                <strong>
                  {agent.providerName ?? text("尚未绑定", "Not bound")}
                </strong>
                <span>
                  {agent.modelId ??
                    (!agent.adapterVerified
                      ? language === "zh-CN"
                        ? agent.message
                        : text("当前只读", "Read-only")
                      : agent.configPath) ??
                    "—"}
                </span>
              </div>
              <div className="agent-row__actions">
                <StatusPill tone={healthy ? "good" : "warn"}>
                  {agent.adapterVerified
                    ? text("可配置", "Configurable")
                    : text("只读", "Read-only")}
                </StatusPill>
                <button
                  className="button button--small"
                  disabled={!agent.adapterVerified || !installed}
                  onClick={() => onConfigure(agent)}
                  title={
                    agent.adapterVerified && installed
                      ? text("查看配置与安装位置", "View configuration and installation")
                      : language === "zh-CN"
                        ? agent.message ?? "当前智能体无法由 AT-Switch 自动配置"
                        : "This agent cannot be configured automatically by AT-Switch"
                  }
                >
                  {text("详情", "Details")}
                </button>
                {supportsInstallSelection && (
                  <button
                    type="button"
                    className="button button--small button--secondary"
                    disabled={installPathBusyAgentId === agent.id}
                    onClick={() => onSelectInstallPath(agent)}
                    title={text(
                      `选择 ${agent.displayName} 主程序所在目录`,
                      `Choose the folder containing ${agent.displayName}`,
                    )}
                  >
                    {installPathBusyAgentId === agent.id ? (
                      <LoaderCircle className="is-spinning" size={15} />
                    ) : (
                      <FolderOpen size={15} />
                    )}
                    {text("安装位置", "Location")}
                  </button>
                )}
                {supportsInstallSelection && agent.customInstallPath && (
                  <button
                    type="button"
                    className="button button--small button--secondary"
                    disabled={installPathBusyAgentId === agent.id}
                    onClick={() => onClearInstallPath(agent)}
                    title={text("恢复自动发现", "Use automatic discovery")}
                  >
                    <RotateCcw size={15} />
                    {text("自动", "Auto")}
                  </button>
                )}
              </div>
            </article>
          );
        })}
      </section>
    </>
  );
}
