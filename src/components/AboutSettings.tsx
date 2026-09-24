import { useState } from "react";
import {
  AppWindow,
  CheckCircle2,
  Download,
  Globe2,
  Info,
  LoaderCircle,
  RefreshCw,
  Tag,
} from "lucide-react";
import { useLanguage } from "../i18n";
import { api } from "../lib/api";
import type { ReleaseInfo } from "../types";

interface AboutSettingsProps {
  appVersion: string;
  platform: string;
}

interface InfoRow {
  icon: typeof Tag;
  label: string;
  value: string;
  tone?: "default" | "accent";
}

export function AboutSettings({ appVersion, platform }: AboutSettingsProps) {
  const { text } = useLanguage();
  const [checking, setChecking] = useState(false);
  const [release, setRelease] = useState<ReleaseInfo | null>(null);
  const [checked, setChecked] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const checkForUpdate = async () => {
    setChecking(true);
    setError(null);
    try {
      const result = await api.checkUpdate();
      setRelease(result);
      setChecked(true);
    } catch (err) {
      setRelease(null);
      setChecked(true);
      setError(
        err instanceof Error
          ? err.message
          : text("无法获取最新版本信息", "Unable to fetch release info"),
      );
    } finally {
      setChecking(false);
    }
  };

  const platformLabel =
    platform === "macos"
      ? text("macOS Universal", "macOS Universal")
      : platform === "windows"
        ? text("Windows x64", "Windows x64")
        : platform;

  const rows: InfoRow[] = [
    {
      icon: Tag,
      label: text("当前版本", "Current version"),
      value: `v${appVersion}`,
      tone: "accent",
    },
    {
      icon: AppWindow,
      label: text("运行平台", "Platform"),
      value: platformLabel,
    },
    {
      icon: Globe2,
      label: text("发布渠道", "Release channel"),
      value: text("开源正式版", "Open-source stable"),
    },
    {
      icon: Info,
      label: text("本地优先", "Local-first"),
      value: text(
        "核心能力不依赖自有云服务",
        "Core capabilities do not depend on a hosted cloud",
      ),
    },
  ];

  const updateState: "idle" | "checked" | "available" = release
    ? "available"
    : checked
      ? "checked"
      : "idle";

  return (
    <article className="settings-section">
      <div className="settings-section__title">
        <Info size={19} />
        <div>
          <h2>{text("关于 AT-Switch", "About AT-Switch")}</h2>
          <p>
            {text(
              "应用信息、版本与升级通道。",
              "Application info, version, and the upgrade channel.",
            )}
          </p>
        </div>
      </div>

      <ul className="about-info">
        {rows.map(({ icon: Icon, label, value, tone }) => (
          <li key={label} className="about-info__row">
            <span className="about-info__icon">
              <Icon size={15} />
            </span>
            <div className="about-info__body">
              <dt className="about-info__label">{label}</dt>
              <dd
                className={`about-info__value ${
                  tone === "accent" ? "about-info__value--accent" : ""
                }`}
              >
                {value}
              </dd>
            </div>
          </li>
        ))}
      </ul>

      <div className={`about-update about-update--${updateState}`}>
        {updateState === "idle" && (
          <p className="about-update__hint">
            {text(
              "点击下方按钮检查是否有新版本。",
              "Click below to check whether a newer version is available.",
            )}
          </p>
        )}

        {updateState === "checked" && !error && (
          <p className="about-update__hint about-update__hint--ok">
            <CheckCircle2 size={15} />
            {text("已是最新版本", "You're on the latest version")}
          </p>
        )}

        {updateState === "available" && release && (
          <div className="about-update__available">
            <div className="about-update__available-head">
              <span className="about-update__badge">
                {text("发现新版本", "New version")}
              </span>
              <strong className="about-update__version">v{release.version}</strong>
            </div>
            {release.bodyPreview && (
              <p className="about-update__notes">{release.bodyPreview}</p>
            )}
          </div>
        )}

        {error && <p className="about-update__error">{error}</p>}

        <div className="about-update__actions">
          <button
            type="button"
            className="button button--small"
            disabled={checking}
            onClick={() => void checkForUpdate()}
          >
            {checking ? (
              <LoaderCircle className="is-spinning" size={15} />
            ) : (
              <RefreshCw size={15} />
            )}
            {checked
              ? text("重新检查", "Recheck")
              : text("检查更新", "Check for updates")}
          </button>

          {release && (
            <button
              type="button"
              className="button button--small button--primary"
              onClick={() => void api.openUrl(release.htmlUrl)}
            >
              <Download size={15} />
              {text("前往升级", "Open upgrade page")}
            </button>
          )}
        </div>
      </div>
    </article>
  );
}
