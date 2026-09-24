import { Eye, ShieldCheck } from "lucide-react";
import { useLanguage } from "../i18n";
import { agentAvailabilityLabel, agentHealthLabel } from "../lib/format";
import type { AgentSummary } from "../types";
import { StatusPill } from "./StatusPill";

interface AgentReadonlyDetailsProps {
  agent: AgentSummary;
}

/// Detection-only Agents expose no writable model configuration, so the
/// binding form cannot offer a meaningful action for them. This panel keeps
/// the "Details" entry point usable by showing why writes are refused and
/// where the agent was detected.
export function AgentReadonlyDetails({ agent }: AgentReadonlyDetailsProps) {
  const { language, text } = useLanguage();
  const installed = agent.installStatus !== "not_installed";

  return (
    <div className="readonly-details">
      <div className="readonly-details__summary">
        <Eye size={22} aria-hidden="true" />
        <div>
          <h3>
            {text(
              "AT-Switch 只检测该智能体，不修改其模型配置",
              "AT-Switch only detects this agent and never edits its model configuration",
            )}
          </h3>
          <p>
            {agent.message ??
              text(
                "该智能体的模型设置由应用自身管理，AT-Switch 未提供稳定的第三方写入通道。",
                "This agent manages its own model settings; AT-Switch has no stable third-party write channel for it.",
              )}
          </p>
        </div>
        <StatusPill tone="neutral">{text("只读", "Read-only")}</StatusPill>
      </div>

      <dl className="readonly-details__grid">
        <div>
          <dt>{text("安装状态", "Installation")}</dt>
          <dd>
            {agentAvailabilityLabel(
              agent.installStatus,
              agent.runtimeStatus,
              language,
            )}
          </dd>
        </div>
        <div>
          <dt>{text("配置健康", "Configuration health")}</dt>
          <dd>{agentHealthLabel(agent.configHealth, language)}</dd>
        </div>
        <div>
          <dt>{text("检测版本", "Detected version")}</dt>
          <dd>
            {agent.detectedVersion ??
              (installed
                ? text("已检测，版本未知", "Detected, version unknown")
                : "—")}
          </dd>
        </div>
        <div>
          <dt>{text("安装位置", "Installation path")}</dt>
          <dd>
            <code>
              {agent.installPath ?? text("本机未安装", "Not installed")}
            </code>
          </dd>
        </div>
        <div className="readonly-details__wide">
          <dt>{text("配置位置", "Configuration path")}</dt>
          <dd>
            <code>{agent.configPath ?? "—"}</code>
          </dd>
        </div>
      </dl>

      <div className="binding-safety">
        <ShieldCheck size={18} />
        <p>
          {text(
            "请在该智能体的官方设置中配置模型；AT-Switch 会继续展示它的安装状态与检测结果。",
            "Configure the model in this agent's own settings. AT-Switch keeps showing its installation and detection status.",
          )}
        </p>
      </div>
    </div>
  );
}
