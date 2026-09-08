#!/usr/bin/env bash
# 只把【代码】发布到一个你已在门户建好的 Azure Functions 应用（推荐 Flex 消耗计划，
# 和你 OpenAI 反代同款）。不建资源——建应用交给门户，这里只发布 + 配 PROXY_KEY + 打印地址。
#
# 用法（Git Bash）：
#   APP_NAME=<门户里的函数应用名> RG=<它所在的资源组> bash publish.sh
# 想固定密钥就再带 PROXY_KEY=<自定义长随机串>；不带则自动生成。
set -euo pipefail
cd "$(dirname "$0")"

APP_NAME="${APP_NAME:?请设置 APP_NAME=<门户里建好的函数应用名>}"
RG="${RG:?请设置 RG=<该函数应用所在的资源组名>}"
PROXY_KEY="${PROXY_KEY:-$(openssl rand -hex 24)}"

echo "== 目标 =="
echo "  函数应用 APP_NAME=$APP_NAME"
echo "  资源组   RG=$RG"
echo "  路径密钥 PROXY_KEY=$PROXY_KEY"
echo

command -v az >/dev/null || { echo "[错误] 未找到 az（Azure CLI）。"; exit 1; }
az account show >/dev/null 2>&1 || { echo "[错误] 未登录 Azure：az login"; exit 1; }

echo "== 1/3 应用设置（PROXY_KEY + 启用 v4 worker 索引） =="
az functionapp config appsettings set -n "$APP_NAME" -g "$RG" -o none --settings \
  "PROXY_KEY=$PROXY_KEY" \
  "AzureWebJobsFeatureFlags=EnableWorkerIndexing"

echo "== 2/3 装依赖 =="
npm install --omit=dev --no-audit --no-fund

echo "== 3/3 发布代码 =="
if command -v func >/dev/null; then
  # ⚠️ 必须带 --javascript：func 对 v4 Node 项目常识别不出语言，否则以
  #    「Can't determine project language / project set to None」失败。
  func azure functionapp publish "$APP_NAME" --javascript \
    || echo "   [警告] func 返回非零。若上面只是 sync triggers 抖动通常仍成功；若是别的报错（语言识别 / Flex 不兼容等），下方校验会兜住。"
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
echo "== 校验：确认 gemini-proxy 已在线 =="
DEPLOYED=0
for i in 1 2 3 4 5 6; do
  if az functionapp function list -n "$APP_NAME" -g "$RG" --query "[].name" -o tsv 2>/dev/null | grep -q "gemini-proxy"; then
    DEPLOYED=1; break
  fi
  echo "   索引尚未出现，等 10s 再查（$i/6）…"
  sleep 10
done
if [ "$DEPLOYED" -ne 1 ]; then
  echo "   ❌ 在 Azure 上没查到 gemini-proxy——代码没真正部署成功。"
  echo "      手动重试： func azure functionapp publish $APP_NAME --javascript"
  echo "      （Flex 上若 func 不灵，删掉本机 func 后重跑本脚本，会自动改用 az zip 部署。）"
  exit 1
fi
echo "   ✅ 已确认 gemini-proxy 在线"

BASE="https://$APP_NAME.azurewebsites.net/api/g/$PROXY_KEY"
cat <<EOF

============================================================
  发布完成 ✅  把下面三行配到 WiseCortex 服务端（改完重启进程生效）
============================================================
WC_PROXY_GEMINI_BASE_URL=$BASE/ca
WC_PROXY_GEMINI_AUTH_BASE_URL=$BASE/auth
WC_PROXY_GEMINI_USERINFO_BASE_URL=$BASE/ui

自检（Git Bash 用 curl，PowerShell 用 curl.exe；返回 400/401 即通）：
  curl.exe -i -X POST "$BASE/auth/token" -d "grant_type=refresh_token"
============================================================
EOF
