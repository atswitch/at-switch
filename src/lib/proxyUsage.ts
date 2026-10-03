import type {
  AgentSummary,
  ProxyRequestLogEntry,
  ProxyStatus,
} from "../types";

export type AgentRoutingGroupId = "routed" | "idle" | "unavailable";

export interface AgentRoutingGroup {
  id: AgentRoutingGroupId;
  agents: AgentSummary[];
}

export interface AgentUsageRow {
  agentId: string;
  displayName: string;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  requests: number;
  successful: number;
  /** 请求已 2xx 完成但后端没有读到 token（流式 + usage 事件缺失）。 */
  unknownRequests: number;
  successRate: number;
  barPercent: number;
  models: Array<{
    providerId: string;
    providerName: string;
    modelId: string;
    inputTokens: number;
    outputTokens: number;
    cacheReadTokens: number;
    requests: number;
  }>;
}

/**
 * 按"用户当前能做什么"把 Agent 分成三组，避免把十余行堆在一个无视觉锚点的列表里。
 * 分组依据是可用性（是否安装 / 适配器是否校验）与"是否加入代理用量统计"。
 */
export function groupAgentsForRouting(
  agents: AgentSummary[],
): AgentRoutingGroup[] {
  const routed: AgentSummary[] = [];
  const idle: AgentSummary[] = [];
  const unavailable: AgentSummary[] = [];
  for (const agent of agents) {
    if (!isAgentRoutable(agent)) {
      unavailable.push(agent);
      continue;
    }
    if (agent.proxyPrefEnabled) {
      routed.push(agent);
    } else {
      idle.push(agent);
    }
  }
  const groups: AgentRoutingGroup[] = [];
  if (routed.length > 0) groups.push({ id: "routed", agents: routed });
  if (idle.length > 0) groups.push({ id: "idle", agents: idle });
  if (unavailable.length > 0) {
    groups.push({ id: "unavailable", agents: unavailable });
  }
  return groups;
}

export function isAgentRoutable(agent: AgentSummary): boolean {
  return Boolean(agent.adapterVerified) &&
    agent.installStatus !== "not_installed";
}

export function buildAgentUsage(
  agents: AgentSummary[],
  proxiedAgents: string[],
  recent: ProxyRequestLogEntry[],
): AgentUsageRow[] {
  const proxied = new Set(proxiedAgents);
  // 只统计当前处于"代理路由中"的 Agent；其它 Agent 即使出现在 recent 里
  // （例如开关刚关掉的瞬间）也不计入"用量明细"。
  const byAgent = new Map<string, AgentUsageRow>();
  for (const agent of agents) {
    if (!proxied.has(agent.id)) continue;
    byAgent.set(agent.id, {
      agentId: agent.id,
      displayName: agent.displayName,
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      requests: 0,
      successful: 0,
      unknownRequests: 0,
      successRate: 0,
      barPercent: 0,
      models: [],
    });
  }
  for (const row of recent) {
    const target = byAgent.get(row.agentId);
    if (!target) continue;
    target.requests += 1;
    if (row.inputTokens != null) target.inputTokens += row.inputTokens;
    if (row.outputTokens != null) target.outputTokens += row.outputTokens;
    if (row.cacheReadTokens != null) target.cacheReadTokens += row.cacheReadTokens;
    const isSuccess = row.status >= 200 && row.status < 300;
    const hasUsage = row.inputTokens != null || row.outputTokens != null;
    if (isSuccess) {
      if (hasUsage) {
        target.successful += 1;
      } else {
        // 状态 200 但 token 都缺失：上游没发 usage 事件（例如流式无 token），
        // 不能算"成功计费"，但也不要让前端 successRate 假装是 100%。
        target.unknownRequests += 1;
      }
    }
  }
  // 按模型再次细分，便于看到"切换模型"后的用量差异。
  for (const target of byAgent.values()) {
    const modelMap = new Map<string, AgentUsageRow["models"][number]>();
    for (const row of recent) {
      if (row.agentId !== target.agentId) continue;
      const key = `${row.providerId}::${row.model}`;
      let entry = modelMap.get(key);
      if (!entry) {
        entry = {
          providerId: row.providerId,
          providerName: row.providerName,
          modelId: row.model,
          inputTokens: 0,
          outputTokens: 0,
          cacheReadTokens: 0,
          requests: 0,
        };
        modelMap.set(key, entry);
      }
      entry.requests += 1;
      if (row.inputTokens != null) entry.inputTokens += row.inputTokens;
      if (row.outputTokens != null) entry.outputTokens += row.outputTokens;
      if (row.cacheReadTokens != null) entry.cacheReadTokens += row.cacheReadTokens;
    }
    target.models = [...modelMap.values()].sort(
      (a, b) => b.inputTokens + b.outputTokens - (a.inputTokens + a.outputTokens),
    );
    const billable = target.requests - target.unknownRequests;
    target.successRate =
      billable === 0
        ? 0
        : Math.round((target.successful / billable) * 100);
  }
  const rows = [...byAgent.values()];
  const maxTokens = Math.max(
    1,
    ...rows.map((row) => row.inputTokens + row.outputTokens),
  );
  for (const row of rows) {
    row.barPercent = Math.min(
      100,
      Math.round(((row.inputTokens + row.outputTokens) / maxTokens) * 100),
    );
  }
  return rows.sort(
    (a, b) =>
      b.inputTokens + b.outputTokens - (a.inputTokens + a.outputTokens),
  );
}

export function proxyAgentUsage(
  proxy: ProxyStatus,
  agents: AgentSummary[],
): AgentUsageRow[] {
  return buildAgentUsage(
    agents,
    proxy.proxiedAgents ?? [],
    proxy.recentRequests,
  );
}

export function formatTokens(value: number): string {
  if (value >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(value >= 10_000_000 ? 0 : 1)}M`;
  }
  if (value >= 1_000) {
    return `${(value / 1_000).toFixed(value >= 10_000 ? 0 : 1)}K`;
  }
  return value.toString();
}

export function formatClock(iso: string): string {
  // RFC 3339 → HH:MM:SS（代理日志全部带时区信息，但这里只展示本地时钟便于人眼比较）。
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleTimeString(undefined, { hour12: false });
}
