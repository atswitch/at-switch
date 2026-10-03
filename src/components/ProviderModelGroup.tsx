import { ChevronDown, Network } from "lucide-react";
import clsx from "clsx";
import { useId } from "react";
import { useLanguage } from "../i18n";
import type { ProviderSummary } from "../types";
import { ProviderLogo } from "./ProviderLogo";

interface ProviderModelGroupProps {
  provider: ProviderSummary;
  modelCount: number;
  expanded: boolean;
  /** 该供应商下的模型是否为当前 Agent 正在使用的模型。折叠后仍要能看出绑定位置。 */
  inUse?: boolean;
  /** 该 Agent 是否正通过本地代理使用该供应商。 */
  proxyRouted?: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}

export function ProviderModelGroup({
  provider,
  modelCount,
  expanded,
  inUse = false,
  proxyRouted = false,
  onToggle,
  children,
}: ProviderModelGroupProps) {
  const { text } = useLanguage();
  const bodyId = useId();

  return (
    <section className="model-group">
      <div className="model-group__header">
        <button
          type="button"
          className="model-group__toggle"
          aria-expanded={expanded}
          aria-controls={bodyId}
          aria-label={text(
            `${provider.name} 的模型`,
            `${provider.name} models`,
          )}
          onClick={onToggle}
        >
          <ChevronDown
            size={16}
            className={clsx("model-group__chevron", !expanded && "is-collapsed")}
            aria-hidden="true"
          />
          <ProviderLogo provider={provider} size="row" aria-hidden="true" />
          <span className="model-group__name">{provider.name}</span>
          <span className="model-group__count">
            {text(`${modelCount} 个模型`, `${modelCount} models`)}
          </span>
        </button>
        {proxyRouted && (
          <span
            className="model-group__badge model-group__badge--proxy"
            title={text(
              "该智能体的流量经由本地代理；退出 AT-Switch 后它将无法请求",
              "This agent routes through the local proxy; it cannot make requests after AT-Switch exits",
            )}
          >
            <Network size={11} aria-hidden="true" />
            {text("本地代理", "Local proxy")}
          </span>
        )}
        {inUse && <span className="model-group__badge">{text("使用中", "In use")}</span>}
      </div>
      {expanded && (
        <div className="model-group__body" id={bodyId}>
          {children}
        </div>
      )}
    </section>
  );
}
