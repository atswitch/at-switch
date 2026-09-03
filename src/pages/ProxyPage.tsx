import {
  Activity,
  ArrowDownUp,
  CircleStop,
  Play,
  Route,
  Shield,
  Timer,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useLanguage } from "../i18n";
import type { AgentSummary, ProxyStatus } from "../types";
import { formatDuration, successRate } from "../lib/format";
import { PageHeader } from "../components/PageHeader";
import { StatusPill } from "../components/StatusPill";

interface ProxyPageProps {
  proxy: ProxyStatus;
  agents: AgentSummary[];
  busy: boolean;
  onConfigureAgent: (agent: AgentSummary) => void;
  onStart: () => void;
  onStop: () => void;
  onUpdatePort: (port: number) => void;
}

export function ProxyPage({
  proxy,
  agents,
  busy,
  onConfigureAgent,
  onStart,
  onStop,
  onUpdatePort,
}: ProxyPageProps) {
  const { text } = useLanguage();
  const [port, setPort] = useState(String(proxy.port));
  const running = proxy.status === "running";

  useEffect(() => setPort(String(proxy.port)), [proxy.port]);

  return (
    <>
      <PageHeader
        eyebrow="ADVANCED / LOCAL ROUTER"
        title={text("本地代理", "Local proxy")}
        description={text(
          "高级兼容功能：管理回环监听、智能体独立路由和三类 API 协议转换。",
          "Advanced compatibility: manage loopback listening, per-agent routes, and conversion across three API protocols.",
        )}
        actions={
          running ? (
            <button
              className="button button--danger"
              onClick={onStop}
              disabled={busy}
            >
              <CircleStop size={16} />
              {text("停止代理", "Stop proxy")}
            </button>
          ) : (
            <button
              className="button button--primary"
              onClick={onStart}
              disabled={busy}
            >
              <Play size={16} />
              {text("启动代理", "Start proxy")}
            </button>
          )
        }
      />

      <section className="proxy-console">
        <div className="proxy-console__status">
          <div className={`proxy-orbit ${running ? "is-running" : ""}`}>
            <div className="proxy-orbit__core">
              <Route size={28} />
            </div>
            <span className="proxy-orbit__dot proxy-orbit__dot--one" />
            <span className="proxy-orbit__dot proxy-orbit__dot--two" />
          </div>
          <div>
            <p className="eyebrow">PROXY SUPERVISOR</p>
            <h2>
              {running
                ? text("回环监听器运行中", "Loopback listener running")
                : text("回环监听器已停止", "Loopback listener stopped")}
            </h2>
            <div className="proxy-endpoint">
              <span>HTTP</span>
              <code>
                {proxy.host}:{proxy.port}
              </code>
            </div>
          </div>
          <StatusPill tone={running ? "active" : "neutral"} pulse={running}>
            {proxy.status.toUpperCase()}
          </StatusPill>
        </div>

        <div className="proxy-metrics">
          <Metric
            icon={<Activity size={18} />}
            label={text("活跃连接", "Active connections")}
            value={String(proxy.activeConnections)}
          />
          <Metric
            icon={<ArrowDownUp size={18} />}
            label={text("已完成请求", "Completed requests")}
            value={String(proxy.completedRequests)}
          />
          <Metric
            icon={<Shield size={18} />}
            label={text("成功率", "Success rate")}
            value={successRate(
              proxy.successfulRequests,
              proxy.completedRequests,
            )}
          />
          <Metric
            icon={<Timer size={18} />}
            label={text("运行时长", "Uptime")}
            value={formatDuration(proxy.startedAt)}
          />
        </div>
      </section>

      <section className="proxy-layout">
        <article className="panel">
          <div className="panel__header">
            <div>
              <p className="eyebrow">LISTENER</p>
              <h2>{text("监听设置", "Listener settings")}</h2>
            </div>
          </div>
          <label className="field">
            <span>{text("绑定地址", "Bind address")}</span>
            <input value="127.0.0.1" disabled />
            <small>
              {text(
                "固定为本机回环地址，不能暴露到局域网。",
                "Fixed to the local loopback address and never exposed to the LAN.",
              )}
            </small>
          </label>
          <label className="field">
            <span>{text("端口", "Port")}</span>
            <div className="field__action">
              <input
                type="number"
                min={1024}
                max={65535}
                value={port}
                disabled={running}
                onChange={(event) => setPort(event.target.value)}
              />
              <button
                className="button button--small"
                disabled={running || Number(port) === proxy.port}
                onClick={() => onUpdatePort(Number(port))}
              >
                {text("保存", "Save")}
              </button>
            </div>
            <small>
              {text(
                "默认端口 54187。运行期间不能修改。",
                "Default port: 54187. Stop the proxy before changing it.",
              )}
            </small>
          </label>
        </article>

        <article className="panel">
          <div className="panel__header">
            <div>
              <p className="eyebrow">TAKEOVER MATRIX</p>
              <h2>{text("智能体接管", "Agent takeover")}</h2>
            </div>
            <span className="mono-counter">
              {agents.filter((agent) => agent.mode === "proxy").length}/
              {agents.length}
            </span>
          </div>
          <div className="takeover-list">
            {agents.map((agent) => (
              <div className="takeover-row" key={agent.id}>
                <div>
                  <strong>{agent.displayName}</strong>
                  <small>
                    {agent.mode === "proxy"
                      ? `${agent.providerName ?? "Provider"} · ${agent.modelId ?? text("模型", "Model")}`
                      : text("未使用本地代理", "Not using the local proxy")}
                  </small>
                </div>
                <button
                  className="button button--small"
                  aria-label={text(
                    `配置 ${agent.displayName} 本地代理`,
                    `Configure local proxy for ${agent.displayName}`,
                  )}
                  onClick={() => onConfigureAgent(agent)}
                  disabled={
                    !agent.adapterVerified ||
                    agent.installStatus === "not_installed"
                  }
                  title={
                    agent.adapterVerified
                      ? text(
                          "选择模型供应商和模型后启用代理接管",
                          "Select a provider and model to enable proxy takeover",
                        )
                      : text(
                          "该智能体适配尚未验证",
                          "This agent adapter is not verified",
                        )
                  }
                >
                  {agent.mode === "proxy"
                    ? text("调整", "Adjust")
                    : text("配置", "Configure")}
                </button>
              </div>
            ))}
          </div>
        </article>
      </section>

      <section className="protocol-lane">
        <div>
          <span>OpenAI Chat</span>
          <ArrowDownUp size={15} />
        </div>
        <div>
          <span>Canonical IR</span>
          <small>
            {text(
              "能力检查 · 模型路由 · 工具调用关联",
              "Capability checks · Model routing · Tool-call correlation",
            )}
          </small>
        </div>
        <div>
          <ArrowDownUp size={15} />
          <span>Responses / Anthropic</span>
        </div>
      </section>
    </>
  );
}

function Metric({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
}) {
  return (
    <div className="proxy-metric">
      <span>{icon}</span>
      <small>{label}</small>
      <strong>{value}</strong>
    </div>
  );
}
