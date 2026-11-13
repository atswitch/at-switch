#!/usr/bin/env bash
# 打包期注入会员服务地址：source .env.build 后执行传入的命令（tauri build / dev）。
# 用法：scripts/build-with-account-env.sh npm run tauri build
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ -f .env.build ]]; then
  set -a
  source .env.build
  set +a
  echo "[build-with-account-env] AT_SWITCH_API_BASE=${AT_SWITCH_API_BASE:-<empty>}"
else
  echo "[build-with-account-env] 未找到 .env.build，会员功能将以未配置状态打包（本地功能不受影响）"
fi

exec "$@"
