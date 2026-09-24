import type { PageId } from "../types";

const pageIds: readonly PageId[] = ["overview", "agents", "providers", "settings"];

/** URL 深链 → 页面 ID；无法识别的值返回 undefined，由调用方决定回退行为。 */
export function parsePageId(value: string | null): PageId | undefined {
  if (!value) return undefined;
  return pageIds.find((pageId) => pageId === value);
}
