#!/usr/bin/env bash
# WiseCortex 服务器部署：拉代码 → 编译后端 → 构建前端 → 重启服务 → 报告版本。
#
# **不做 cd**：在哪个目录执行就部署哪个目录（各服务器路径不同，自己先 cd 过去）。
#   cd /www/wwwroot/aiagent/wisecortex && bash scripts/deploy-ubuntu.sh
#
# 可用环境变量：
#   WC_SERVICE=wisecortex   systemd 服务名
#   WC_SKIP_WEB=1         跳过前端构建（只改了后端时更快）
#   WC_SKIP_PULL=1        跳过 git pull（本地已改好，只想重编重启）
#   WC_STOP_FIRST=1       编译前先停服务（见下方 ETXTBSY 说明）
set -euo pipefail

SERVICE="${WC_SERVICE:-wisecortex}"

# ── 0. 认门：确认当前目录真的是 wisecortex 仓库 ────────────────────────────────
# 不加这道闸，在错的目录里跑就会对着别的仓库 git pull、甚至重启无关服务。
if [ ! -f Cargo.toml ] || ! grep -q 'wisecortex' Cargo.toml 2>/dev/null || [ ! -d crates/server ]; then
  echo "[错误] 当前目录不像 wisecortex 仓库：$(pwd)"
  echo "       请先 cd 到仓库根目录再执行本脚本。"
  exit 1
fi
echo "== 部署目标 =="
echo "   目录   $(pwd)"
echo "   服务   $SERVICE"

# Rust 常装在 ~/.cargo/bin 但不在非交互 shell 的 PATH 里。
export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null || { echo "[错误] 找不到 cargo。装 Rust 或检查 ~/.cargo/bin。"; exit 1; }

# systemctl 通常要 root；已经是 root 就别多套一层 sudo。
SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  command -v sudo >/dev/null && SUDO="sudo" || {
    echo "[错误] 非 root 且没有 sudo，无法重启服务。"; exit 1; }
fi

# ── 1. 拉代码 ───────────────────────────────────────────────────────────────
if [ "${WC_SKIP_PULL:-}" = "1" ]; then
  echo && echo "== 1/4 跳过 git pull（WC_SKIP_PULL=1）=="
else
  echo && echo "== 1/4 拉取代码 =="
  BEFORE="$(git rev-parse --short HEAD)"
  git pull
  AFTER="$(git rev-parse --short HEAD)"
  if [ "$BEFORE" = "$AFTER" ]; then
    echo "   已是最新（$AFTER），无新提交。"
  else
    echo "   $BEFORE → $AFTER"
    git --no-pager log --oneline "$BEFORE..$AFTER" | sed 's/^/     /'
  fi
fi

# ── 2. 编译后端 ─────────────────────────────────────────────────────────────
# ⚠️ 首次按新的 [profile.release]（增量编译）构建会全量重编，慢几分钟，属正常，别当卡死。
echo && echo "== 2/4 编译后端（release）=="
if [ "${WC_STOP_FIRST:-}" = "1" ]; then
  echo "   先停服务（WC_STOP_FIRST=1）"
  $SUDO systemctl stop "$SERVICE" || true
fi
# 链接阶段若报 "Text file busy"(ETXTBSY)，是旧进程正占着要覆盖的可执行文件。
# 自动停服务重试一次——比让用户对着一句晦涩错误发呆强。
if ! cargo build --release -p wisecortex-server -p wisecortex-cli 2>&1 | tee /tmp/wc-build.log; then
  if grep -qi "text file busy" /tmp/wc-build.log; then
    echo "   可执行文件被运行中的进程占用，先停服务再重试…"
    $SUDO systemctl stop "$SERVICE" || true
    cargo build --release -p wisecortex-server -p wisecortex-cli
  else
    echo "[错误] 后端编译失败，服务未重启（仍跑着旧版本）。"
    exit 1
  fi
fi

# ── 3. 构建前端 ─────────────────────────────────────────────────────────────
if [ "${WC_SKIP_WEB:-}" = "1" ]; then
  echo && echo "== 3/4 跳过前端构建（WC_SKIP_WEB=1）=="
elif [ ! -d web ]; then
  echo && echo "== 3/4 无 web 目录，跳过 =="
else
  echo && echo "== 3/4 构建前端 =="
  command -v npm >/dev/null || { echo "[错误] 找不到 npm，装 Node.js(>=20) 或用 WC_SKIP_WEB=1 跳过。"; exit 1; }
  # 用子 shell 进 web/，不改调用者的当前目录。
  # 写成 if/else 而不是 `npm ci || npm install`：后者会在 npm ci 真出错时悄悄退回
  # npm install，把依赖不一致的问题盖过去。
  (
    cd web
    if [ -f package-lock.json ]; then npm ci; else npm install; fi
    npm run build
  )
fi

# ── 4. 重启并核验 ───────────────────────────────────────────────────────────
echo && echo "== 4/4 重启服务 =="
$SUDO systemctl restart "$SERVICE"
sleep 2
if $SUDO systemctl is-active --quiet "$SERVICE"; then
  echo "   ✅ $SERVICE 运行中"
else
  echo "   ❌ $SERVICE 没起来，最近日志："
  $SUDO journalctl -u "$SERVICE" -n 30 --no-pager | sed 's/^/     /'
  exit 1
fi

# 版本号来自编译进二进制的 CARGO_PKG_VERSION：能对上就说明新代码确实生效了，
# 而不只是"命令跑完了没报错"。Web 界面侧栏底部显示的也是这个值。
echo
echo "============================================================"
# `wisecortex --version` 输出形如 "wisecortex 0.9.18"，取后半段即可，别把程序名重复一遍。
echo "  部署完成   版本 $(./target/release/wisecortex --version 2>/dev/null | awk '{print $NF}' || echo '未知')"
# 提交标题不截断：cut -c 在部分实现里按字节切，中文标题会被切成乱码。
echo "  提交       $(git rev-parse --short HEAD)  $(git log -1 --format=%s)"
echo "============================================================"
