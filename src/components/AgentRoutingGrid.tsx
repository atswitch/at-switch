import { Network } from "lucide-react";
import clsx from "clsx";
import { useMemo } from "react";
import { useLanguage } from "../i18n";
import type { AgentSummary } from "../types";
import {
  formatTokens,
  groupAgentsForRouting,
  isAgentRoutable,
  type AgentRoutingGroup,
  type AgentRoutingGroupId,
  type AgentUsageRow,
} from "../lib/proxyUsage";
import { ProviderLogo } from "./ProviderLogo";

interface AgentRoutingGridProps {
  agents: AgentSummary[];
  busyAgentId?: string;
  usageByAgent: AgentUsageRow[];
  onToggleProxyPref: (agent: AgentSummary, enabled: boolean) => void;
  onConfigureProxy?: (agent: AgentSummary) => void;
}

export function AgentRoutingGrid({
  agents,
  busyAgentId,
  usageByAgent,
  onToggleProxyPref,
  onConfigureProxy,
}: AgentRoutingGridProps) {
  const { text } = useLanguage();
  const groups = useMemo(() => groupAgentsForRouting(agents), [agents]);
  const usageByAgentId = useMemo(() => {
    const map = new Map<string, AgentUsageRow>();
    for (const row of usageByAgent) map.set(row.agentId, row);
    return map;
  }, [usageByAgent]);

  if (agents.length === 0) {
    return (
      <p className="panel__hint">
        {text("尚未发现任何智能体。", "No agents detected yet.")}
      </p>
    );
  }

  return (
    <div className="takeover-list takeover-list--grouped">
      {groups.map((group) => (
        <AgentRoutingCardGroup
          key={group.id}
          group={group}
          busyAgentId={busyAgentId}
          usageByAgentId={usageByAgentId}
          onToggleProxyPref={onToggleProxyPref}
          onConfigureProxy={onConfigureProxy}
        />
      ))}
    </div>
  );
}

function AgentRoutingCardGroup({
  group,
  busyAgentId,
  usageByAgentId,
  onToggleProxyPref,
  onConfigureProxy,
}: {
  group: AgentRoutingGroup;
  busyAgentId?: string;
  usageByAgentId: Map<string, AgentUsageRow>;
  onToggleProxyPref: (agent: AgentSummary, enabled: boolean) => void;
  onConfigureProxy?: (agent: AgentSummary) => void;
}) {
  const { text } = useLanguage();

  return (
    <section className="takeover-group">
      <header className="takeover-group__header">
        <span
          className={clsx(
            "takeover-group__badge",
            `takeover-group__badge--${group.id}`,
          )}
          aria-hidden="true"
        />
        <strong>{text(groupLabel(group.id), groupLabelEn(group.id))}</strong>
        <small>{text(groupHint(group.id), groupHintEn(group.id))}</small>
        <span className="takeover-group__count">{group.agents.length}</span>
      </header>
      <div className="agent-routing-grid">
        {group.agents.map((agent) => (
          <AgentRoutingCard
            key={agent.id}
            agent={agent}
            pending={busyAgentId === agent.id}
            usage={usageByAgentId.get(agent.id)}
            onToggleProxyPref={onToggleProxyPref}
            onConfigureProxy={onConfigureProxy}
          />
        ))}
      </div>
    </section>
  );
}

function groupLabel(id: AgentRoutingGroupId): string {
  switch (id) {
    case "routed":
      return "已启用用量统计";
    case "idle":
      return "未启用用量统计";
    case "unavailable":
      return "暂不可用";
  }
}

function groupLabelEn(id: AgentRoutingGroupId): string {
  switch (id) {
    case "routed":
      return "Telemetry enabled";
    case "idle":
      return "Telemetry off";
    case "unavailable":
      return "Unavailable";
  }
}

function groupHint(id: AgentRoutingGroupId): string {
  switch (id) {
    case "routed":
      return "这些智能体的流量正在（或将会）被本地代理统计。";
    case "idle":
      return "打开卡片右下角开关后，对应流量会经过本地代理并出现在「用量明细」。";
    case "unavailable":
      return "该智能体尚未安装或适配器未校验，无法接入代理。";
  }
}

function groupHintEn(id: AgentRoutingGroupId): string {
  switch (id) {
    case "routed":
      return "Traffic from these agents is (or will be) recorded by the local proxy.";
    case "idle":
      return "Flip the switch to route their traffic through the proxy and surface usage below.";
    case "unavailable":
      return "These agents are not installed or not verified, so they cannot be routed yet.";
  }
}

interface AgentRoutingCardProps {
  agent: AgentSummary;
  pending: boolean;
  usage?: AgentUsageRow;
  onToggleProxyPref: (agent: AgentSummary, enabled: boolean) => void;
  onConfigureProxy?: (agent: AgentSummary) => void;
}

function AgentRoutingCard({
  agent,
  pending,
  usage,
  onToggleProxyPref,
  onConfigureProxy,
}: AgentRoutingCardProps) {
  const { text } = useLanguage();
  const pref = Boolean(agent.proxyPrefEnabled);
  const disabled = !isAgentRoutable(agent);

  let statusTone: "ok" | "off" | "warn";
  let statusLabel: string;
  if (disabled) {
    statusTone = "warn";
    statusLabel = text("暂不可用", "Unavailable");
  } else if (pref) {
    statusTone = "ok";
    statusLabel = text("已启用", "On");
  } else {
    statusTone = "off";
    statusLabel = text("未启用", "Off");
  }

  const modelLine = pref
    ? agent.providerName
      ? `${agent.providerName} · ${agent.modelId ?? text("模型", "Model")}`
      : text("已启用 · 尚未绑定模型", "Telemetry on · no model bound")
    : agent.providerName
      ? `${agent.providerName} · ${agent.modelId ?? text("模型", "Model")}`
      : text("未启用 · 直连默认配置", "Telemetry off · using built-in default");

  const availability = disabled
    ? text(
        agent.installStatus === "not_installed"
          ? "该智能体尚未安装"
          : "适配器尚未校验通过",
        agent.installStatus === "not_installed"
          ? "Agent is not installed"
          : "Adapter has not been verified",
      )
    : null;

  return (
    <div
      className={clsx(
        "agent-card",
        pref && "agent-card--on",
        disabled && "agent-card--disabled",
        pending && "agent-card--pending",
      )}
    >
      <div className="agent-card__header">
        <ProviderLogo
          provider={{ kind: "custom", name: agent.displayName }}
          size="card"
        />
        <strong className="agent-card__title">{agent.displayName}</strong>
        <span
          className={clsx(
            "agent-card__status",
            `agent-card__status--${statusTone}`,
          )}
        >
          {statusLabel}
        </span>
      </div>

      <small className="agent-card__model">{modelLine}</small>
      {availability && <small className="agent-card__warn">{availability}</small>}

      {usage && <AgentUsageInline usage={usage} />}

      <div className="agent-card__actions">
        {!disabled && onConfigureProxy && (
          <button
            type="button"
            className="button button--secondary button--small"
            onClick={() => onConfigureProxy(agent)}
            disabled={pending}
            title={text(
              `为 ${agent.displayName} 打开本地代理配置`,
              `Configure the local proxy for ${agent.displayName}`,
            )}
          >
            <Network size={14} aria-hidden="true" />
            {text("本地代理配置", "Local proxy")}
          </button>
        )}
        <label
          className={clsx(
            "toggle",
            pref && "is-active",
            disabled && "is-disabled",
          )}
          title={
            disabled
              ? text(
                  "该智能体不可用，无法启用用量统计",
                  "This agent is unavailable; telemetry cannot be enabled",
                )
              : pref
                ? text(
                    "关闭后，流量不再经过本地代理，用量不再统计",
                    "Stop routing this agent through the local proxy and stop recording its usage",
                  )
                : agent.providerId && agent.modelId
                  ? text(
                      "开启后，流量经过本地代理并统计用量",
                      "Route this agent through the local proxy and start recording its usage",
                    )
                  : text(
                      "请先在首页为该智能体选择模型后再开启代理统计",
                      "Select a model on the home page first before enabling telemetry",
                    )
          }
        >
          <input
            type="checkbox"
            checked={pref}
            disabled={disabled || pending}
            aria-label={`${text("用量统计", "Usage telemetry")} · ${agent.displayName}`}
            onChange={(event) => onToggleProxyPref(agent, event.target.checked)}
          />
          <span className="toggle__slider" aria-hidden="true" />
        </label>
      </div>
    </div>
  );
}

function AgentUsageInline({ usage }: { usage: AgentUsageRow }) {
  const { text } = useLanguage();
  return (
    <div className="agent-card__usage" aria-label="agent usage">
      <span>
        <small>{text("请求", "req")}</small>
        <strong>{usage.requests}</strong>
      </span>
      <span>
        <small>{text("输入", "in")}</small>
        <strong>{formatTokens(usage.inputTokens)}</strong>
      </span>
      <span>
        <small>{text("输出", "out")}</small>
        <strong>{formatTokens(usage.outputTokens)}</strong>
      </span>
      <span>
        <small>{text("成功率", "ok")}</small>
        <strong>{usage.successRate}%</strong>
      </span>
    </div>
  );
}
