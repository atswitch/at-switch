import { Activity, ArrowDownUp, CircleStop, Play, Route, Shield, Timer } from "lucide-react";
import clsx from "clsx";
import { useLanguage } from "../i18n";
import type { ProxyStatus } from "../types";
import { formatDuration, successRate } from "../lib/format";
import { StatusPill } from "./StatusPill";

interface ProxyStatusBarProps {
  proxy: ProxyStatus;
  proxyAgentCount: number;
  busy: boolean;
  onStart: () => void;
  onStop: () => void;
}

export function ProxyStatusBar({
  proxy,
  proxyAgentCount,
  busy,
  onStart,
  onStop,
}: ProxyStatusBarProps) {
  const { text } = useLanguage();
  const running = proxy.status === "running";

  return (
    <article className="panel proxy-status-card">
      <div className="proxy-status-bar">
        <div className="proxy-status-bar__indicator">
          <div className={clsx("proxy-orbit", running && "is-running")}>
            <div className="proxy-orbit__core">
              <Route size={24} />
            </div>
            <span className="proxy-orbit__dot proxy-orbit__dot--one" />
            <span className="proxy-orbit__dot proxy-orbit__dot--two" />
          </div>
          <StatusPill tone={running ? "active" : "neutral"} pulse={running}>
            {proxy.status.toUpperCase()}
          </StatusPill>
        </div>

        <div className="proxy-status-bar__info">
          <p className="eyebrow">PROXY SUPERVISOR</p>
          <h2>
            {running
              ? text("回环监听器运行中", "Loopback listener running")
              : text("回环监听器已停止", "Loopback listener stopped")}
          </h2>
          <strong className="proxy-status-bar__endpoint">
            {proxy.host}:{proxy.port}
          </strong>
          <small>
            {text(
              `${proxyAgentCount} 个智能体使用代理接管`,
              `${proxyAgentCount} agent${proxyAgentCount === 1 ? "" : "s"} routed through the proxy`,
            )}
          </small>
        </div>

        <div className="proxy-status-bar__actions">
          {running ? (
            <button
              type="button"
              className="button button--danger"
              onClick={onStop}
              disabled={busy}
            >
              <CircleStop size={16} />
              {text("停止代理", "Stop proxy")}
            </button>
          ) : (
            <button
              type="button"
              className="button button--primary"
              onClick={onStart}
              disabled={busy}
            >
              <Play size={16} />
              {text("启动代理", "Start proxy")}
            </button>
          )}
        </div>
      </div>

      <div className="proxy-status-metrics">
        <ProxyMetric
          icon={<Activity size={16} />}
          label={text("活跃连接", "Active connections")}
          value={String(proxy.activeConnections)}
        />
        <ProxyMetric
          icon={<ArrowDownUp size={16} />}
          label={text("已完成请求", "Completed requests")}
          value={String(proxy.completedRequests)}
        />
        <ProxyMetric
          icon={<Shield size={16} />}
          label={text("成功率", "Success rate")}
          value={successRate(
            proxy.successfulRequests,
            proxy.completedRequests,
          )}
        />
        <ProxyMetric
          icon={<Timer size={16} />}
          label={text("运行时长", "Uptime")}
          value={formatDuration(proxy.startedAt)}
        />
      </div>
    </article>
  );
}

function ProxyMetric({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
}) {
  return (
    <div className="proxy-status-metric">
      <span aria-hidden="true">{icon}</span>
      <small>{label}</small>
      <strong>{value}</strong>
    </div>
  );
}
