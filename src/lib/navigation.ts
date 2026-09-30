import type { SettingsTab } from "../types";

const settingsTabIds: readonly SettingsTab[] = [
  "agents",
  "providers",
  "general",
  "proxy",
  "about",
];

/** URL 深链 → 设置中心分类。旧的独立页面深链（agents/providers/proxy）以及
 * `tab` 参数都收敛到设置中心的对应分类，保证历史链接不失效。 */
export function parseSettingsTab(
  page: string | null,
  tab: string | null,
): SettingsTab {
  if (page === "agents") return "agents";
  if (page === "providers") return "providers";
  if (page === "proxy") return "proxy";
  if (tab && (settingsTabIds as readonly string[]).includes(tab)) {
    return tab as SettingsTab;
  }
  return "general";
}
