import { CircleAlert, Power, RotateCw, ShieldCheck } from "lucide-react";
import { useLanguage } from "../i18n";
import type { AgentSummary } from "../types";
import { Modal } from "./Modal";

interface AgentRestartConfirmationProps {
  agent?: AgentSummary;
  operation: "apply" | "restore";
  onCancel: () => void;
  onConfirm: () => void;
}

export function AgentRestartConfirmation({
  agent,
  operation,
  onCancel,
  onConfirm,
}: AgentRestartConfirmationProps) {
  const { text } = useLanguage();
  const restoring = operation === "restore";
  const action = restoring
    ? text("恢复默认配置", "Restore default configuration")
    : text("切换模型", "Switch model");
  const automatic = agent?.automaticRestartSupported ?? false;
  const title = agent
    ? automatic
      ? text(
          `重启 ${agent.displayName} 后${action}`,
          `Restart ${agent.displayName} to ${action.toLowerCase()}`,
        )
      : text(
          `${action}并在下次启动 ${agent.displayName} 时生效`,
          `${action} when ${agent.displayName} starts next time`,
        )
    : action;

  return (
    <Modal
      open={Boolean(agent)}
      onClose={onCancel}
      eyebrow="RESTART REQUIRED"
      title={title}
      footer={
        <div className="restart-confirmation__actions">
          <button
            className="button button--secondary"
            type="button"
            onClick={onCancel}
          >
            {text("取消", "Cancel")}
          </button>
          <button
            className="button button--primary"
            type="button"
            onClick={onConfirm}
          >
            <RotateCw size={16} />
            {automatic
              ? restoring
                ? text("恢复并自动重启", "Restore and restart")
                : text("切换并自动重启", "Switch and restart")
              : restoring
                ? text("恢复配置", "Restore configuration")
                : text("保存配置", "Save configuration")}
          </button>
        </div>
      }
    >
      {agent && (
        <div className="restart-confirmation">
          <div className="restart-confirmation__agent">
            <span aria-hidden="true">
              <Power size={22} />
            </span>
            <div>
              <small>{text("即将更新", "About to update")}</small>
              <strong>{agent.displayName}</strong>
              <p>
                {restoring
                  ? text(
                      "移除 AT-Switch 路由并恢复接管前的模型配置",
                      "Remove the AT-Switch route and restore the pre-takeover model configuration",
                    )
                  : text(
                      "写入新的模型供应商、模型和本地路由配置",
                      "Write the new provider, model, and local route configuration",
                    )}
              </p>
            </div>
          </div>

          <div className="restart-confirmation__warning">
            <CircleAlert size={18} />
            <div>
              <strong>
                {automatic
                  ? text(
                      "请先保存正在进行的工作",
                      "Save your work before continuing",
                    )
                  : text(
                      "当前只检测到命令行版本",
                      "Only the command-line version was detected",
                    )}
              </strong>
              {automatic ? (
                <p>
                  {text(
                    `如果 ${agent.displayName} 正在运行，AT-Switch 会安全退出并自动重新打开；运行中的生成或工具调用会被中断。`,
                    `If ${agent.displayName} is running, AT-Switch will quit it safely and reopen it automatically. Active generations or tool calls will be interrupted.`,
                  )}
                </p>
              ) : (
                <p>
                  {text(
                    `AT-Switch 不会终止终端中的任务。配置保存后，请重新启动对应的 ${agent.displayName} CLI 任务。`,
                    `AT-Switch will not terminate terminal tasks. Restart the relevant ${agent.displayName} CLI task after saving the configuration.`,
                  )}
                </p>
              )}
            </div>
          </div>

          <div className="restart-confirmation__note">
            <ShieldCheck size={17} />
            <span>
              {automatic
                ? text(
                    `如果 ${agent.displayName} 当前没有运行，只保存配置，不会额外启动应用；下次打开时自动生效。`,
                    `If ${agent.displayName} is not running, only the configuration is saved; the app will not be launched and the change takes effect next time it opens.`,
                  )
                : text(
                    "现有 CLI 任务继续使用旧配置，不会被中断；新启动的任务会读取新配置。",
                    "Existing CLI tasks continue with the old configuration without interruption; new tasks read the new configuration.",
                  )}
            </span>
          </div>
        </div>
      )}
    </Modal>
  );
}
