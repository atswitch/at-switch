import { Activity } from "lucide-react";
import { useLanguage } from "../i18n";
import type { ProxyStatus } from "../types";
import { formatTokens, type AgentUsageRow } from "../lib/proxyUsage";
import { PanelEmptyState } from "./PanelEmptyState";

interface ProxyUsageByAgentProps {
  proxy: ProxyStatus;
  usageByAgent: AgentUsageRow[];
}

export function ProxyUsageByAgent({
  proxy,
  usageByAgent,
}: ProxyUsageByAgentProps) {
  const { text } = useLanguage();

  return (
    <article className="panel">
      <div className="panel__header">
        <div>
          <p className="eyebrow">USAGE BY AGENT</p>
          <h2>{text("用量明细", "Usage by agent")}</h2>
        </div>
        <span className="mono-counter">
          {text(
            `${usageByAgent.length} 个智能体 · ${proxy.recentRequests.length} 条请求`,
            `${usageByAgent.length} agents · ${proxy.recentRequests.length} requests`,
          )}
        </span>
      </div>
      {usageByAgent.length === 0 ? (
        <PanelEmptyState
          icon={<Activity size={18} />}
          title={text("还没有用量记录", "No usage recorded yet")}
          description={
            proxy.status !== "running"
              ? text(
                  "代理监听器未启动，先在上方状态卡点击「启动代理」开始记录用量。",
                  "The proxy listener is stopped. Click 'Start proxy' on the status card above to begin recording usage.",
                )
              : text(
                  "代理已启动，但还没有智能体被路由到代理。在上方「智能体用量统计」打开开关后，流量才会经过代理并产生用量。",
                  "The proxy is running, but no agents are routed through it. Enable a switch above in 'Agent usage telemetry' to route traffic through the proxy.",
                )
          }
        />
      ) : (
        <ul className="usage-by-agent">
          {usageByAgent.map((row) => (
            <li className="usage-agent-card" key={row.agentId}>
              <div className="usage-agent-card__head">
                <strong>{row.displayName}</strong>
                <span>
                  {text(
                    `${row.requests} 次请求`,
                    `${row.requests} requests`,
                  )}
                </span>
              </div>

              <dl className="usage-agent-card__metrics">
                <div>
                  <dt>{text("输入", "In")}</dt>
                  <dd>{formatTokens(row.inputTokens)}</dd>
                </div>
                <div>
                  <dt>{text("输出", "Out")}</dt>
                  <dd>{formatTokens(row.outputTokens)}</dd>
                </div>
                <div>
                  <dt>{text("合计", "Total")}</dt>
                  <dd>{formatTokens(row.inputTokens + row.outputTokens)}</dd>
                </div>
                <div>
                  <dt>{text("缓存", "Cached")}</dt>
                  <dd>{formatTokens(row.cacheReadTokens)}</dd>
                </div>
                <div>
                  <dt>{text("成功率", "Success")}</dt>
                  <dd className={row.successRate < 100 ? "is-warn" : undefined}>
                    {row.successRate}%
                  </dd>
                </div>
              </dl>

              <div className="usage-agent-card__bar">
                <span style={{ width: `${row.barPercent}%` }} aria-hidden="true" />
              </div>

              {row.unknownRequests > 0 && (
                <small className="usage-agent-card__note">
                  {text(
                    `${row.unknownRequests} 条请求的上游未返回 token 用量`,
                    `${row.unknownRequests} requests returned no token usage`,
                  )}
                </small>
              )}

              {row.models.length > 0 && (
                <ul className="usage-agent-card__models">
                  {row.models.map((model) => (
                    <li key={`${model.providerId}:${model.modelId}`}>
                      <span className="usage-agent-card__model-id">
                        {model.modelId}
                      </span>
                      <span className="usage-agent-card__provider">
                        {model.providerName}
                      </span>
                      <span className="usage-agent-card__model-usage">
                        {formatTokens(model.inputTokens)} in ·{" "}
                        {formatTokens(model.outputTokens)} out ·{" "}
                        {model.cacheReadTokens > 0 ? `${formatTokens(model.cacheReadTokens)} cache · ` : ""}
                        {model.requests} req
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </li>
          ))}
        </ul>
      )}
    </article>
  );
}
