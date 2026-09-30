import { ScrollText } from "lucide-react";
import clsx from "clsx";
import { useLanguage } from "../i18n";
import type { ProxyStatus } from "../types";
import { formatClock } from "../lib/proxyUsage";
import { PanelEmptyState } from "./PanelEmptyState";

interface ProxyRecentRequestsProps {
  proxy: ProxyStatus;
}

export function ProxyRecentRequests({ proxy }: ProxyRecentRequestsProps) {
  const { text } = useLanguage();

  return (
    <article className="panel">
      <div className="panel__header">
        <div>
          <p className="eyebrow">RECENT REQUESTS</p>
          <h2>{text("最近请求", "Recent requests")}</h2>
        </div>
        <span className="mono-counter">
          {`${proxy.recentRequests.length} / 200`}
        </span>
      </div>
      {proxy.recentRequests.length === 0 ? (
        <PanelEmptyState
          icon={<ScrollText size={18} />}
          title={text("还没有请求记录", "No requests yet")}
          description={text(
            "代理收到的请求会按时间倒序显示在这里，只保留最近 200 条摘要，不含请求正文。",
            "Requests received by the proxy appear here in reverse chronological order. Only the latest 200 summaries are kept, without request bodies.",
          )}
        />
      ) : (
        <div className="usage-log">
          <table className="usage-log__table">
            <thead>
              <tr>
                <th scope="col">{text("时间", "Time")}</th>
                <th scope="col">{text("智能体", "Agent")}</th>
                <th scope="col">{text("供应商 / 模型", "Provider / Model")}</th>
                <th scope="col">{text("状态", "Status")}</th>
                <th scope="col" className="usage-log__cell--numeric">
                  {text("输入 / 输出 / 缓存", "In / Out / Cache")}
                </th>
              </tr>
            </thead>
            <tbody>
              {proxy.recentRequests.slice(0, 20).map((row, index) => (
                <tr key={`${row.at}-${index}`}>
                  <td className="usage-log__time">{formatClock(row.at)}</td>
                  <td>
                    <span className="usage-log__agent">{row.agentId}</span>
                  </td>
                  <td className="usage-log__model">
                    <strong>{row.model}</strong>
                    <small>{row.providerName}</small>
                  </td>
                  <td>
                    <span
                      className={clsx(
                        "status-chip",
                        row.status >= 200 && row.status < 300
                          ? "status-chip--ok"
                          : "status-chip--warn",
                      )}
                    >
                      {row.status}
                    </span>
                  </td>
                  <td className="usage-log__tokens">
                    {row.inputTokens == null &&
                    row.outputTokens == null &&
                    row.cacheReadTokens == null ? (
                      <span className="usage-log__unknown">
                        {text("未知", "unknown")}
                      </span>
                    ) : (
                      <>
                        <span>{row.inputTokens ?? "—"}</span>
                        <i aria-hidden="true">/</i>
                        <span>{row.outputTokens ?? "—"}</span>
                        <i aria-hidden="true">/</i>
                        <span>
                          {row.cacheReadTokens == null
                            ? text("未知", "unknown")
                            : row.cacheReadTokens}
                        </span>
                      </>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </article>
  );
}
