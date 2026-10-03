import { CircleAlert, Power, RotateCw, ShieldCheck, ListChecks } from "lucide-react";
import { useLanguage } from "../i18n";
import type { AgentSummary, ManualRecoveryStep } from "../types";
import { Modal } from "./Modal";

interface AgentRestartConfirmationProps {
  agent?: AgentSummary;
  operation: "apply" | "restore" | "manual_recovery";
  /** 手动恢复步骤列表，仅 operation=manual_recovery 时使用 */
  manualRecoverySteps?: ManualRecoveryStep[];
  onCancel: () => void;
  onConfirm: () => void;
}

export function AgentRestartConfirmation({
  agent,
  operation,
  manualRecoverySteps,
  onCancel,
  onConfirm,
}: AgentRestartConfirmationProps) {
  const { text } = useLanguage();
  const manual = operation === "manual_recovery";
  const restoring = operation === "restore";
  const automatic = agent?.automaticRestartSupported ?? false;

  const action = restoring
    ? text("恢复默认配置", "Restore default configuration")
    : text("切换模型", "Switch model");

  const title = manual
    ? agent
      ? text(
          `需手动恢复 ${agent.displayName} 的默认模型`,
          `Manually restore ${agent.displayName}'s default model`,
        )
      : text("需手动恢复默认模型", "Manual default-model recovery required")
    : agent
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

  const eyebrow = manual
    ? text("需要手动操作", "MANUAL ACTION REQUIRED")
    : "RESTART REQUIRED";

  return (
    <Modal
      open={Boolean(agent)}
      onClose={onCancel}
      eyebrow={eyebrow}
      title={title}
      footer={
        <div className="restart-confirmation__actions">
          <button
            className="button button--secondary"
            type="button"
            onClick={onCancel}
          >
            {manual ? text("关闭", "Close") : text("取消", "Cancel")}
          </button>
          {manual ? (
            <button
              className="button button--primary"
              type="button"
              onClick={onConfirm}
            >
              {text("知道了", "Got it")}
            </button>
          ) : (
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
          )}
        </div>
      }
    >
      {agent && (
        <div className="restart-confirmation">
          {manual ? (
            // ── 手动恢复步骤提示 ──────────────────────────────────────────────
            <>
              <div className="restart-confirmation__agent">
                <span aria-hidden="true">
                  <ListChecks size={22} />
                </span>
                <div>
                  <small>{text("请按以下步骤操作", "Follow these steps")}</small>
                  <strong>{agent.displayName}</strong>
                  <p>
                    {text(
                      "AT-Switch 无法自动恢复该智能体的出厂默认模型，请在应用内手动选择。",
                      "AT-Switch cannot auto-restore the factory default model. Please select it manually inside the app.",
                    )}
                  </p>
                </div>
              </div>

              <div className="restart-confirmation__manual-steps">
                {manualRecoverySteps && manualRecoverySteps.length > 0 ? (
                  <ol className="manual-steps__list">
                    {manualRecoverySteps.map((step, i) => (
                      <li key={i} className="manual-steps__item">
                        <strong className="manual-steps__title">{step.title}</strong>
                        <span className="manual-steps__detail">{step.detail}</span>
                      </li>
                    ))}
                  </ol>
                ) : (
                  <p className="restart-confirmation__note">
                    {text(
                      "请在该智能体内手动将默认模型改回你需要的选项。",
                      "Please manually select the desired default model inside the agent.",
                    )}
                  </p>
                )}
              </div>

              <div className="restart-confirmation__note">
                <ShieldCheck size={17} />
                <span>
                  {text(
                    "完成以上步骤后，新模型将在下次启动或新建会话时生效。",
                    "After completing these steps, the new model will take effect on next launch or new session.",
                  )}
                </span>
              </div>
            </>
          ) : (
            // ── 原有 apply / restore 内容 ────────────────────────────────────
            <>
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
            </>
          )}
        </div>
      )}
    </Modal>
  );
}
