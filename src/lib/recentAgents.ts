const STORAGE_KEY = "at-switch-recent-agents";
const MAX_RECENT = 20;

/** 读取最近选中使用过的智能体 id 列表（最近使用的排在前面）。 */
export function getRecentAgentIds(): string[] {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((id): id is string => typeof id === "string");
  } catch {
    return [];
  }
}

/** 记录一次智能体选择，把它移到栈顶并持久化，返回新的列表。 */
export function recordAgentSelection(agentId: string): string[] {
  const recent = getRecentAgentIds().filter((id) => id !== agentId);
  const next = [agentId, ...recent].slice(0, MAX_RECENT);
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // 忽略隐私模式或配额导致的写入失败。
  }
  return next;
}
