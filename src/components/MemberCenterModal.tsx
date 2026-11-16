import { useEffect, useState } from "react";
import {
  CircleUserRound,
  HelpCircle,
  LogOut,
  MessageSquare,
  Settings,
  ArrowUpCircle,
  Download,
  RefreshCw,
  CheckCircle2,
  AlertCircle,
  X,
  Bot,
  Boxes,
} from "lucide-react";
import { useLanguage } from "../i18n";
import type { AccountStatus, LoginMethod, PageId } from "../types";

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  /** 真·服务端无更高版本 */
  | { kind: "uptodate" }
  /** 已下载待重启，但用户点了「稍后处理」：静默，但必须仍能点「立即更新」回到重启 */
  | { kind: "dismissed"; version: string }
  | { kind: "available"; version: string }
  | { kind: "downloading"; percent: number; version: string }
  | { kind: "installing"; version: string }
  | { kind: "relaunch"; version: string }
  | { kind: "error"; message: string };

export interface UpdateSectionProps {
  state: UpdateState;
  appVersion: string;
  onCheck: () => void;
  onInstall: () => void;
  onRelaunch: () => void;
  onDismiss?: () => void;
}

/**
 * 右上角会员下拉菜单（参照产品截图）：
 * 顶部显示用户名/未登录入口，菜单项：设置、版本状态(检查更新)、帮助与反馈、退出登录。
 * 登录动作弹独立登录弹窗（选择微信扫码 / 谷歌账号）。
 */

export interface MemberMenuProps {
  open: boolean;
  appVersion: string;
  status?: AccountStatus;
  updateState: UpdateState;
  onCheckUpdate: () => void;
  onInstallUpdate: () => void;
  onRelaunch: () => void;
  /** 「稍后处理」：回退到已完成安装但未重启的静默状态。 */
  onDismissUpdate?: () => void;
  onClose: () => void;
  onOpenLogin: () => void;
  onOpenSettings: () => void;
  onOpenDocs: () => void;
  onOpenFeedback: () => void;
  onLogout: () => void;
  /** 顶部工具入口迁入菜单后的快捷动作；点击后由调用方关闭菜单。 */
  refreshing: boolean;
  onRefresh: () => void;
  onNavigate: (page: PageId) => void;
}

export function MemberMenu(props: MemberMenuProps) {
  const { text } = useLanguage();
  const { status, open } = props;
  const loggedIn = status?.loggedIn === true;

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") props.onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [open, props]);

  if (!open) return null;

  return (
    <>
      <div
        className="member-menu-mask"
        role="presentation"
        onMouseDown={props.onClose}
      />
      <aside className="member-menu" role="menu" aria-label={text("会员中心", "Member center")}>
        <button
          type="button"
          className="member-menu__header"
          onClick={loggedIn ? undefined : props.onOpenLogin}
          disabled={loggedIn}
        >
          <CircleUserRound size={22} />
          <strong>
            {loggedIn
              ? status?.profile?.displayName || status?.profile?.username || "AT"
              : text("未登录 · 点击登录", "Not signed in · Click to sign in")}
          </strong>
        </button>
        {status?.offline && loggedIn && (
          <p className="member-menu__offline">
            {text("会员服务离线", "Member service offline")}
          </p>
        )}

        <div className="member-menu__group member-menu__shortcuts">
          <button
            type="button"
            className="member-menu__item"
            onClick={props.onRefresh}
            disabled={props.refreshing}
            aria-label={text("刷新状态", "Refresh status")}
            title={text("刷新状态", "Refresh status")}
          >
            <RefreshCw
              size={17}
              className={props.refreshing ? "is-spinning" : undefined}
            />
            <span>{text("刷新状态", "Refresh status")}</span>
          </button>
          <button
            type="button"
            className="member-menu__item"
            onClick={() => props.onNavigate("agents")}
            aria-label={text("智能体状态", "Agent status")}
            title={text("查看智能体状态", "View agent status")}
          >
            <Bot size={17} />
            <span>{text("智能体状态", "Agent status")}</span>
          </button>
          <button
            type="button"
            className="member-menu__item"
            onClick={() => props.onNavigate("providers")}
            aria-label={text("模型供应商与大模型", "Model providers & LLMs")}
            title={text("管理模型供应商与大模型", "Manage model providers & LLMs")}
          >
            <Boxes size={17} />
            <span>{text("模型供应商与大模型", "Model providers & LLMs")}</span>
          </button>
        </div>

        <div className="member-menu__group">
          <button
            type="button"
            className="member-menu__item"
            onClick={props.onOpenSettings}
          >
            <Settings size={17} />
            <span>{text("设置", "Settings")}</span>
            <i className="member-menu__chevron">›</i>
          </button>
        </div>

        <div className="member-menu__group">
          <UpdateSection
            state={props.updateState}
            appVersion={props.appVersion}
            onCheck={props.onCheckUpdate}
            onInstall={props.onInstallUpdate}
            onRelaunch={props.onRelaunch}
            onDismiss={props.onDismissUpdate}
          />
          <button
            type="button"
            className="member-menu__item"
            onClick={props.onOpenFeedback}
          >
            <MessageSquare size={17} />
            <span>{text("建议反馈", "Feedback")}</span>
            <i className="member-menu__chevron">›</i>
          </button>
          <button
            type="button"
            className="member-menu__item"
            onClick={props.onOpenDocs}
          >
            <HelpCircle size={17} />
            <span>{text("使用文档", "Documentation")}</span>
            <i className="member-menu__chevron">›</i>
          </button>
        </div>

        {loggedIn && (
          <div className="member-menu__group">
            <button
              type="button"
              className="member-menu__item member-menu__item--danger"
              onClick={props.onLogout}
            >
              <LogOut size={17} />
              <span>{text("退出登录", "Sign out")}</span>
            </button>
          </div>
        )}
      </aside>
    </>
  );
}

/** 登录弹窗：选择微信扫码或谷歌账号（PRD 功能 1 的弹窗形态）。 */
export interface LoginDialogProps {
  open: boolean;
  busy: boolean;
  error: string | null;
  qrUrl: string | null;
  onChoose: (method: LoginMethod) => void;
  onRefreshQr: () => void;
  onCancel: () => void;
  onClose: () => void;
}

export function LoginDialog(props: LoginDialogProps) {
  const { text } = useLanguage();
  const { open, qrUrl } = props;

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") props.onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [open, props]);

  if (!open) return null;

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={props.onClose}>
      <section
        className="login-modal"
        role="dialog"
        aria-modal="true"
        aria-label={text("登录会员", "Sign in")}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="login-modal__hero">
          <div className="login-modal__logo">AT</div>
          <h2>{text("登录会员中心", "Member center")}</h2>
          <p>{text("跨设备同步供应商配置 · 云端备份", "Sync provider configs across devices · Cloud backup")}</p>
        </div>
        <div className="login-modal__body">
          {!qrUrl && (
            <>
              <div className="login-modal__label">{text("选择登录方式", "Choose a sign-in method")}</div>
              <button
                type="button"
                className="login-option login-option--wechat"
                disabled={props.busy}
                onClick={() => props.onChoose("wechat")}
              >
                <span className="login-option__icon">
                  <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
                    <path d="M9.5 4C5.36 4 2 6.69 2 10c0 1.89 1.08 3.56 2.78 4.66l-.7 2.1 2.45-1.23c.93.31 1.93.47 2.97.47.28 0 .55-.01.82-.04A5.1 5.1 0 0 1 10 14.5c0-2.9 2.91-5.25 6.5-5.25.28 0 .55.01.82.04C16.61 6.36 13.36 4 9.5 4zM7 8.75a.95.95 0 1 1 0-1.9.95.95 0 0 1 0 1.9zm5 0a.95.95 0 1 1 0-1.9.95.95 0 0 1 0 1.9zM22 14.5c0-2.76-2.69-5-6-5s-6 2.24-6 5 2.69 5 6 5c.87 0 1.7-.14 2.45-.4l2.05 1.03-.58-1.75C21.13 17.51 22 16.08 22 14.5zm-7.9-.83a.8.8 0 1 1 0-1.6.8.8 0 0 1 0 1.6zm3.8 0a.8.8 0 1 1 0-1.6.8.8 0 0 1 0 1.6z"/>
                  </svg>
                </span>
                <span className="login-option__text">
                  <strong>{text("微信扫码登录", "Sign in with WeChat")}</strong>
                  <span>{text("打开手机微信扫一扫", "Scan with WeChat on your phone")}</span>
                </span>
                <span className="login-option__arrow">›</span>
              </button>
              <button
                type="button"
                className="login-option login-option--google"
                disabled={props.busy}
                onClick={() => props.onChoose("google")}
              >
                <span className="login-option__icon">
                  <svg width="22" height="22" viewBox="0 0 48 48">
                    <path fill="#4285F4" d="M45.12 24.5c0-1.56-.14-3.06-.4-4.5H24v8.51h11.84c-.51 2.75-2.06 5.08-4.39 6.64v5.52h7.11c4.16-3.83 6.56-9.47 6.56-16.17z"/>
                    <path fill="#34A853" d="M24 46c5.94 0 10.92-1.97 14.56-5.33l-7.11-5.52c-1.97 1.32-4.49 2.1-7.45 2.1-5.73 0-10.58-3.87-12.31-9.07H4.34v5.7C7.96 41.07 15.4 46 24 46z"/>
                    <path fill="#FBBC05" d="M11.69 28.18C11.25 26.86 11 25.45 11 24s.25-2.86.69-4.18v-5.7H4.34A21.94 21.94 0 0 0 2 24c0 3.55.85 6.91 2.34 9.88l7.35-5.7z"/>
                    <path fill="#EA4335" d="M24 10.75c3.23 0 6.13 1.11 8.41 3.29l6.31-6.31C34.91 4.18 29.93 2 24 2 15.4 2 7.96 6.93 4.34 14.12l7.35 5.7c1.73-5.2 6.58-9.07 12.31-9.07z"/>
                  </svg>
                </span>
                <span className="login-option__text">
                  <strong>{text("Google 账号登录", "Sign in with Google")}</strong>
                  <span>{text("使用 Gmail 账号快捷授权", "Quick authorize with your Gmail")}</span>
                </span>
                <span className="login-option__arrow">›</span>
              </button>
              {props.busy && (
                <p className="login-modal__hint">{text("正在跳转授权…", "Redirecting to authorize…")}</p>
              )}
            </>
          )}
          {qrUrl && (
            <div className="login-modal__qr">
              <iframe
                src={qrUrl}
                title="WeChat QR"
                sandbox="allow-scripts allow-same-origin"
              />
              <p className="login-modal__qr-tip">
                {text("请使用微信扫描二维码登录", "Scan the QR code with WeChat")}
              </p>
              <div className="login-modal__qr-actions">
                <button type="button" className="login-modal__refresh" onClick={props.onRefreshQr} disabled={props.busy}>
                  {text("刷新二维码", "Refresh QR code")}
                </button>
                <button type="button" className="login-modal__back" onClick={props.onCancel}>
                  {text("返回选择", "Back")}
                </button>
              </div>
            </div>
          )}
          {props.error && <p className="login-modal__error">{props.error}</p>}
        </div>
        <div className="login-modal__foot">
          {text("登录即代表同意服务协议与隐私政策 · API Key 永不上传", "By signing in you agree to our terms · API keys are never uploaded")}
        </div>
      </section>
    </div>
  );
}

/** 版本状态区（PRD 功能 3）：菜单内仅保留版本号与检查入口；进行中状态用顶部悬浮通知条展示。 */
export function UpdateSection({
  state,
  appVersion,
  onCheck,
  onInstall,
  onRelaunch,
  onDismiss,
}: UpdateSectionProps) {
  const { text } = useLanguage();
  const meta = (() => {
    switch (state.kind) {
      case "idle":
        return `v${appVersion}`;
      case "checking":
        return text("检查中…", "Checking…");
      case "uptodate":
        return text("已是最新", "Up to date") + ` v${appVersion}`;
      case "dismissed":
        return `${text("已下载", "Downloaded")} v${state.version}${text("，待重启", ", relaunch to apply")}`;
      case "available":
        return `${text("发现新版本", "New version")} v${state.version}`;
      case "downloading":
        return `${text("下载中", "Downloading")} ${state.percent}%`;
      case "installing":
        return text("安装中…", "Installing…");
      case "relaunch":
        return text("待重启生效", "Relaunch pending");
      case "error":
        return state.message;
    }
  })();

  return (
    <div className="member-menu__item member-menu__item--static member-update">
      <ArrowUpCircle size={17} />
      <span>{text("检查更新", "Check for updates")}</span>
      <em className="member-menu__meta">{meta}</em>
      {(state.kind === "idle" || state.kind === "uptodate" || state.kind === "error") && (
        <button type="button" className="link-button" onClick={onCheck}>
          {state.kind === "error"
            ? text("重试", "Retry")
            : text("检查更新", "Check for updates")}
        </button>
      )}
      {state.kind === "available" && (
        <button type="button" className="link-button" onClick={onInstall}>
          {text("立即更新", "Update now")}
        </button>
      )}
      {state.kind === "relaunch" && (
        <>
          <button type="button" className="link-button" onClick={onRelaunch}>
            {text("立即重启", "Relaunch now")}
          </button>
          <button
            type="button"
            className="link-button"
            onClick={() => onDismiss?.()}
          >
            {text("稍后处理", "Later")}
          </button>
        </>
      )}
      {state.kind === "dismissed" && (
        <button type="button" className="link-button" onClick={onRelaunch}>
          {text("立即重启", "Relaunch now")}
        </button>
      )}
    </div>
  );
}

/**
 * 顶部悬浮更新通知条：检查中 / 下载中 / 待重启 / 失败，
 * 固定于窗口顶部居中，含进度条与操作按钮，可手动关闭（关闭回退到 idle 静默态）。
 */
export interface UpdateToastProps {
  state: UpdateState;
  onClose: () => void;
  onInstall: () => void;
  onRelaunch: () => void;
  onRetry: () => void;
}

export function UpdateToast({ state, onClose, onInstall, onRelaunch, onRetry }: UpdateToastProps) {
  const { text } = useLanguage();
  if (
    state.kind === "idle" ||
    state.kind === "uptodate" ||
    state.kind === "dismissed" ||
    state.kind === "checking"
  ) {
    return null;
  }

  const content = (() => {
    switch (state.kind) {
      case "available":
        return {
          tone: "info" as const,
          text: `${text("已检查到新版本", "New version found")} v${state.version}`,
          action: (
            <button type="button" className="update-toast__action" onClick={onInstall}>
              {text("立即更新", "Update now")}
            </button>
          ),
        };
      case "downloading":
        return {
          tone: "progress" as const,
          text: `${text("已检查到新版本，下载中", "New version found, downloading")} ${state.percent}%`,
        };
      case "installing":
        return {
          tone: "progress" as const,
          text: text("下载完成，正在安装更新…", "Download complete, installing…"),
        };
      case "relaunch":
        return {
          tone: "success" as const,
          text: text("更新完成，重启后生效", "Update complete. Relaunch to apply."),
          action: (
            <>
              <button type="button" className="update-toast__action" onClick={onRelaunch}>
                {text("立即重启", "Relaunch now")}
              </button>
              <button type="button" className="update-toast__action update-toast__action--ghost" onClick={onClose}>
                {text("稍后处理", "Later")}
              </button>
            </>
          ),
        };
      case "error":
        return {
          tone: "error" as const,
          text: state.message,
          action: (
            <button type="button" className="update-toast__action" onClick={onRetry}>
              {text("重试", "Retry")}
            </button>
          ),
        };
    }
  })();

  const Icon = content.tone === "success" ? CheckCircle2
    : content.tone === "error" ? AlertCircle
    : content.tone === "progress" ? Download
    : RefreshCw;

  return (
    <div className={`update-toast update-toast--${content.tone}`} role="status">
      <Icon size={17} className={content.tone === "progress" ? "update-toast__spin" : undefined} />
      <span className="update-toast__text">{content.text}</span>
      {content.action}
      <button type="button" className="update-toast__close" onClick={onClose} aria-label={text("关闭", "Close")}>
        <X size={14} />
      </button>
      {state.kind === "downloading" && (
        <div className="update-toast__progress">
          <div className="update-toast__bar" style={{ width: `${state.percent}%` }} />
        </div>
      )}
    </div>
  );
}

/**
 * 建议反馈弹窗（PRD 功能 4）：反馈内容必填、联系方式选填。
 *
 * 视觉与 LoginDialog 完全共用 login-modal 结构（渐变 hero + 380px 白卡 + 同款按钮），
 * 避免两处弹窗外形割裂。提交成功后直接关闭弹窗并清空输入（失败才留在原地提示重试）。
 */
export interface FeedbackDialogProps {
  open: boolean;
  onClose: () => void;
}

export function FeedbackDialog({ open, onClose }: FeedbackDialogProps) {
  const { text } = useLanguage();
  const [content, setContent] = useState("");
  const [contact, setContact] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  // 每次重新打开都从干净状态开始，避免上次失败提示残留。
  useEffect(() => {
    if (open) return;
    setContent("");
    setContact("");
    setError(null);
    setBusy(false);
  }, [open]);

  if (!open) return null;

  const submit = async () => {
    if (busy || !content.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const { api } = await import("../lib/api");
      await api.submitFeedback(content.trim(), contact.trim() || undefined);
      setContent("");
      setContact("");
      onClose();
    } catch (err) {
      setError(
        `${text("提交失败，请稍后重试", "Submit failed, please retry")}：${(err as Error).message}`,
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        className="login-modal feedback-modal"
        role="dialog"
        aria-modal="true"
        aria-label={text("建议反馈", "Feedback")}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="login-modal__hero">
          <div className="login-modal__logo">
            <MessageSquare size={20} />
          </div>
          <h2>{text("建议反馈", "Feedback")}</h2>
          <p>{text("你的建议会帮助我们把 AT-Switch 做得更好", "Your feedback helps us improve AT-Switch")}</p>
        </div>
        <div className="login-modal__body feedback-modal__body">
          <label className="feedback-field">
            <span className="login-modal__label feedback-field__label">
              {text("反馈内容", "Feedback")}
              <em>{text("必填", "Required")}</em>
            </span>
            <textarea
              className="feedback-input feedback-textarea"
              value={content}
              rows={5}
              maxLength={4000}
              onChange={(event) => setContent(event.target.value)}
              placeholder={text(
                "描述你遇到的问题或希望改进的地方…",
                "Describe the issue or what could be improved…",
              )}
            />
          </label>
          <label className="feedback-field">
            <span className="login-modal__label feedback-field__label">
              {text("联系方式", "Contact")}
              <em>{text("选填", "Optional")}</em>
            </span>
            <input
              className="feedback-input"
              value={contact}
              maxLength={255}
              onChange={(event) => setContact(event.target.value)}
              placeholder={text("邮箱或微信，方便我们回复你", "Email or WeChat so we can reply")}
            />
          </label>
          {error && <p className="login-modal__error">{error}</p>}
          <div className="feedback-modal__actions">
            <button
              type="button"
              className="login-modal__back"
              onClick={onClose}
              disabled={busy}
            >
              {text("取消", "Cancel")}
            </button>
            <button
              type="button"
              className="feedback-modal__submit"
              disabled={busy || !content.trim()}
              onClick={() => void submit()}
            >
              {busy ? text("提交中…", "Submitting…") : text("提交", "Submit")}
            </button>
          </div>
        </div>
        <div className="login-modal__foot">
          {text("反馈会附带设备与版本信息，便于定位问题", "Feedback includes device and version info to help us debug")}
        </div>
      </section>
    </div>
  );
}
