import { Cloud, KeyRound, RotateCw, ShieldCheck } from "lucide-react";
import { useLanguage } from "../i18n";
import type { AgentSummary, ManualRecoveryStep } from "../types";
import { Modal } from "./Modal";

interface AgentAccountConnectionConfirmationProps {
  agent?: AgentSummary;
  operation: "apply" | "restore" | "manual_recovery";
  /** 手动恢复步骤列表，仅 operation=manual_recovery 时使用 */
  manualRecoverySteps?: ManualRecoveryStep[];
  onCancel: () => void;
  onConfirm: () => void;
}

export function AgentAccountConnectionConfirmation({
  agent,
  operation,
  manualRecoverySteps,
  onCancel,
  onConfirm,
}: AgentAccountConnectionConfirmationProps) {
  const { text } = useLanguage();
  const manual = operation === "manual_recovery";
  const restoring = manual || operation === "restore";
  const title = restoring
    ? text("连接 ima 并恢复原始模型", "Connect ima and restore original models")
    : text("连接 ima 并切换模型", "Connect ima and switch model");

  return (
    <Modal
      open={Boolean(agent)}
      onClose={onCancel}
      eyebrow="CONNECT IMA"
      title={title}
      footer={
        <div className="restart-confirmation__actions">
          <button className="button button--secondary" type="button" onClick={onCancel}>
            {text("取消", "Cancel")}
          </button>
          <button className="button button--primary" type="button" onClick={onConfirm}>
            <KeyRound size={16} />
            {restoring ? text("连接并恢复", "Connect and restore") : text("连接并切换", "Connect and switch")}
          </button>
        </div>
      }
    >
      <div className="restart-confirmation">
        <div className="restart-confirmation__agent">
          <span aria-hidden="true"><Cloud size={22} /></span>
          <div>
            <strong>{text("使用本机已登录的 ima 账号", "Use the ima account signed in on this computer")}</strong>
            <p>{text(
              "AT-Switch 将读取 ima 的登录凭据，无需复制或填写。macOS 可能提示允许访问钥匙串，Windows 使用当前用户的系统加密凭据；同一账号连接成功后，无需再次确认连接。",
              "AT-Switch reads ima's sign-in credentials; no copying or typing is needed. macOS may request keychain access; Windows uses credentials encrypted for the current user. After a successful connection, the same account needs no further connection confirmation.",
            )}</p>
          </div>
        </div>
        {!restoring && (
          <div className="restart-confirmation__note">
            <Cloud size={18} />
            <span>{text(
              "所选模型的接口地址、API Key 和模型名会按照 ima 自定义模型的机制保存到腾讯 ima，并同时用于「问问 ima」和「我的 copilot」。",
              "The selected endpoint, API key, and model name are saved to Tencent ima using its custom-model settings, and selected for both Ask ima and My copilot.",
            )}</span>
          </div>
        )}
        <div className="restart-confirmation__note">
          <ShieldCheck size={18} />
          <span>{text(
            "首次切换前保存两个入口各自原来的模型选择。恢复原始模型时保留你已有的自定义模型；切换失败会尝试自动恢复，并明确提示需要处理的异常。",
            "The original model selection for each entry is saved before the first switch. Restore preserves your existing custom models. A failed switch attempts automatic recovery and reports any issue that needs attention.",
          )}</span>
        </div>
        {manual && (
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
                  "请在 ima 内手动将默认模型改回你需要的选项。",
                  "Please manually select the desired default model inside ima.",
                )}
              </p>
            )}
          </div>
        )}
        {agent?.needsRestart && agent.runtimeStatus === "running" && (
          <div className="restart-confirmation__warning">
            <RotateCw size={18} />
            <div>
              <strong>{text("请先等待当前生成完成", "Wait for the current generation to finish")}</strong>
              <p>{text(
                "为确保两个入口读取新的模型选择，AT-Switch 会安全退出并重新打开 ima。",
                "AT-Switch will safely quit and reopen ima so both entries load the new model selections.",
              )}</p>
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}
