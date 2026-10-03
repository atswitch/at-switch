import { useEffect, useState } from "react";
import { useLanguage } from "../i18n";
import type { ProxyStatus } from "../types";

interface ProxyListenerSettingsProps {
  proxy: ProxyStatus;
  onUpdatePort: (port: number) => void;
}

export function ProxyListenerSettings({
  proxy,
  onUpdatePort,
}: ProxyListenerSettingsProps) {
  const { text } = useLanguage();
  const [port, setPort] = useState(String(proxy.port));
  const running = proxy.status === "running";

  useEffect(() => setPort(String(proxy.port)), [proxy.port]);

  return (
    <article className="panel">
      <div className="panel__header">
        <div>
          <p className="eyebrow">LISTENER</p>
          <h2>{text("监听设置", "Listener settings")}</h2>
        </div>
      </div>
      <div className="listener-fields">
        <label className="field">
          <span>{text("绑定地址", "Bind address")}</span>
          <input value="127.0.0.1" disabled />
        </label>
        <label className="field">
          <span>{text("端口", "Port")}</span>
          <div className="field__action">
            <input
              type="number"
              min={1024}
              max={65535}
              value={port}
              disabled={running}
              onChange={(event) => setPort(event.target.value)}
            />
            <button
              type="button"
              className="button button--small"
              disabled={running || Number(port) === proxy.port}
              onClick={() => onUpdatePort(Number(port))}
            >
              {text("保存", "Save")}
            </button>
          </div>
        </label>
        <small className="listener-fields__hint">
          {text(
            "绑定地址固定为本机回环地址，不能暴露到局域网；默认端口 54187，运行期间不能修改。",
            "The bind address is fixed to the local loopback address and never exposed to the LAN. Default port: 54187. Stop the proxy before changing it.",
          )}
        </small>
      </div>
    </article>
  );
}
