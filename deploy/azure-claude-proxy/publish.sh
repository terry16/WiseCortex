#!/usr/bin/env bash
# 只把【代码】发布到一个你已在门户建好的 Azure Functions 应用（Flex 消耗计划）。
# 不建资源——建应用交给门户，这里只发布 + 配 PROXY_KEY + 冒烟自测 + 打印地址。
#
# 用法（Git Bash）：
#   APP_NAME=claude-gr RG=gemini-rs bash publish.sh
# 想固定密钥就再带 PROXY_KEY=<自定义长随机串>；不带则自动生成。
set -euo pipefail
cd "$(dirname "$0")"

APP_NAME="${APP_NAME:?请设置 APP_NAME=<门户里建好的函数应用名>}"
RG="${RG:?请设置 RG=<该函数应用所在的资源组名>}"
PROXY_KEY="${PROXY_KEY:-$(openssl rand -hex 24)}"
FUNC_NAME="claude-proxy"

command -v az >/dev/null || { echo "[错误] 未找到 az（Azure CLI）。"; exit 1; }
az account show >/dev/null 2>&1 || { echo "[错误] 未登录 Azure：az login"; exit 1; }

# ⚠️ 不要硬拼 $APP_NAME.azurewebsites.net：Flex 消耗计划的主机名带随机后缀
#    （形如 <app>-<随机串>.<区域>-01.azurewebsites.net），拼出来的地址不通。
HOSTNAME="$(az functionapp show -n "$APP_NAME" -g "$RG" \
  --query "properties.defaultHostName || defaultHostName" -o tsv 2>/dev/null || true)"
[ -n "$HOSTNAME" ] || { echo "[错误] 查不到 $APP_NAME 的主机名——应用名/资源组对吗？"; exit 1; }

echo "== 目标 =="
echo "  函数应用 APP_NAME=$APP_NAME"
echo "  资源组   RG=$RG"
echo "  主机名   $HOSTNAME"
echo "  路径密钥 PROXY_KEY=$PROXY_KEY"
echo

echo "== 1/4 应用设置（PROXY_KEY + 启用 v4 worker 索引） =="
az functionapp config appsettings set -n "$APP_NAME" -g "$RG" -o none --settings \
  "PROXY_KEY=$PROXY_KEY" \
  "AzureWebJobsFeatureFlags=EnableWorkerIndexing"

echo "== 2/4 装依赖 =="
npm install --omit=dev --no-audit --no-fund

echo "== 3/4 发布代码 =="
if command -v func >/dev/null; then
  # ⚠️ 必须带 --javascript：func 对 v4 Node 项目常识别不出语言，否则以
  #    「Can't determine project language / project set to None」失败。
  func azure functionapp publish "$APP_NAME" --javascript \
    || echo "   [警告] func 返回非零。若只是 sync triggers 抖动通常仍成功；下方校验会兜住。"
else
  echo "   （未检测到 func，改用 az zip 部署）"
  ZIP="$(pwd)/.deploy-package.zip"; rm -f "$ZIP"
  if command -v zip >/dev/null; then
    zip -r -q "$ZIP" host.json package.json src node_modules
  elif command -v powershell >/dev/null; then
    powershell -NoProfile -Command \
      "Compress-Archive -Force -Path host.json,package.json,src,node_modules -DestinationPath '$ZIP'"
  fi
  [ -f "$ZIP" ] || { echo "[错误] 打包失败：装 zip 或 PowerShell 任一。"; exit 1; }
  az functionapp deployment source config-zip -g "$RG" -n "$APP_NAME" --src "$ZIP" -o none
  rm -f "$ZIP"
fi

# 真实校验：去 Azure 查函数是否真的索引出来了（func 可能假成功）。给几次机会等索引传播。
echo "== 4/4 校验：确认 $FUNC_NAME 已在线 =="
DEPLOYED=0
for i in 1 2 3 4 5 6; do
  if az functionapp function list -n "$APP_NAME" -g "$RG" --query "[].name" -o tsv 2>/dev/null \
       | grep -q "$FUNC_NAME"; then
    DEPLOYED=1; break
  fi
  echo "   索引尚未出现，等 10s 再查（$i/6）…"
  sleep 10
done
if [ "$DEPLOYED" -ne 1 ]; then
  echo "   ❌ 在 Azure 上没查到 $FUNC_NAME——代码没真正部署成功。"
  echo "      手动重试： func azure functionapp publish $APP_NAME --javascript"
  echo "      （Flex 上若 func 不灵，删掉本机 func 后重跑本脚本，会自动改用 az zip 部署。）"
  exit 1
fi
echo "   ✅ 已确认 $FUNC_NAME 在线"

BASE="https://$HOSTNAME/api/c/$PROXY_KEY"

# ── 冒烟自测：不光看"函数在不在"，还要看请求是否真打穿到了 Anthropic ──────────
# 判据：上游的业务错误码 = 穿透成功（404 才是路由/密钥错，000 是没连上）。
code() { curl -s -o /dev/null -w "%{http_code}" --max-time 45 "$@" 2>/dev/null || echo "000"; }

echo
echo "== 冒烟自测（首个请求含冷启动，可能慢十几秒）=="

T1="$(code -X POST "$BASE/auth/v1/oauth/token" \
  -H 'content-type: application/json' \
  -d '{"grant_type":"refresh_token","refresh_token":"wisecortex-proxy-smoketest"}')"
echo "  token 端点(platform.claude.com) → HTTP $T1  [期望 400/401 = 已打穿]"

T2="$(code -X POST "$BASE/an/v1/messages" \
  -H 'anthropic-version: 2023-06-01' -H 'content-type: application/json' -d '{}')"
echo "  推理端点(api.anthropic.com)     → HTTP $T2  [期望 401/400 = 已打穿]"

T3="$(code -X POST "https://$HOSTNAME/api/c/wrong-key-on-purpose/an/v1/messages" -d '{}')"
echo "  错误密钥                        → HTTP $T3  [期望 404 = 加固生效]"

OK=1
case "$T1" in 4*) ;; *) OK=0; echo "  ⚠️ token 端点未打穿（404=密钥/路由错，000=连不上，5xx=上游/冷启动）";; esac
case "$T2" in 4*) ;; *) OK=0; echo "  ⚠️ 推理端点未打穿";; esac
[ "$T3" = "404" ] || { OK=0; echo "  ⚠️ 错误密钥没被挡成 404——加固有问题"; }

if [ "$OK" -eq 1 ]; then
  echo "  ✅ 两个上游都已打穿，密钥加固生效"
else
  echo "  ❌ 自测未全绿，先别急着改服务端环境变量——按上面的提示排查。"
fi

cat <<EOF

============================================================
  发布完成  把下面两行配到 WiseCortex 服务端（改完重启进程生效）
============================================================
WC_PROXY_ANTHROPIC_BASE_URL=$BASE/an
WC_PROXY_ANTHROPIC_AUTH_BASE_URL=$BASE/auth

注：授权页（claude.ai/oauth/authorize）不走反代，登录那一步仍需你本机能开 claude.ai。
手动复测（PowerShell 用 curl.exe）：
  curl.exe -i -X POST "$BASE/auth/v1/oauth/token" -d "grant_type=refresh_token"
============================================================
EOF
