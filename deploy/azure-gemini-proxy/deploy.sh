#!/usr/bin/env bash
# 一键把 Gemini 反代部署到 Azure Functions（日本东部 japaneast）。
#
# 前置（只需一次，交互式，需你本人操作）：
#   1) 装 Azure CLI（必需）:  winget install -e --id Microsoft.AzureCLI   （或 https://aka.ms/azcli）
#   2) 登录:                  az login          （会弹浏览器，需你本人完成）
#   3)（可选）装 Functions Core Tools 让发布更顺：
#        winget install Microsoft.Azure.FunctionsCoreTools
#        ⚠️ 别用 npm 装 func——azure-functions-core-tools@4 的 npm 包有 ERR_REQUIRE_ESM 的
#           安装 bug；本脚本在没有 func 时会自动改用 `az` 的 zip 部署，无需 func。
#
# 然后在本目录跑： bash deploy.sh
# 可用环境变量覆盖默认：APP_NAME / RG / LOCATION / STORAGE / PROXY_KEY
set -euo pipefail
cd "$(dirname "$0")"

LOCATION="${LOCATION:-japaneast}"                       # 日本东部（东京）
RG="${RG:-wisecortex-gemini-rg}"
# 函数应用名须全局唯一：默认加随机后缀，可用 APP_NAME 固定。
APP_NAME="${APP_NAME:-wisecortex-gemini-$RANDOM$RANDOM}"
# 存储账户名：3-24 位小写字母数字、全局唯一。
STORAGE="${STORAGE:-wcgem$RANDOM$RANDOM}"
STORAGE="$(printf '%s' "$STORAGE" | tr -cd 'a-z0-9' | cut -c1-24)"
# 路径密钥：默认随机生成（同时写进 WC_PROXY_* 的 URL）。
PROXY_KEY="${PROXY_KEY:-$(openssl rand -hex 24)}"

echo "== 配置 =="
echo "  资源组   RG=$RG"
echo "  区域     LOCATION=$LOCATION"
echo "  函数应用 APP_NAME=$APP_NAME"
echo "  存储账户 STORAGE=$STORAGE"
echo "  路径密钥 PROXY_KEY=$PROXY_KEY"
echo

command -v az >/dev/null || { echo "[错误] 未找到 az（Azure CLI）。装： winget install -e --id Microsoft.AzureCLI，再 az login。"; exit 1; }
az account show >/dev/null 2>&1 || { echo "[错误] 未登录 Azure。请先运行： az login"; exit 1; }

echo "== 1/5 资源组 =="
az group create -n "$RG" -l "$LOCATION" -o none

echo "== 2/5 存储账户 =="
az storage account create -n "$STORAGE" -g "$RG" -l "$LOCATION" --sku Standard_LRS -o none

# Node 版本：可用 NODE_VERSION 覆盖。20 已 EOL（2026-04-30）被 az 拒绝，默认 24。
NODE_VERSION="${NODE_VERSION:-24}"
echo "== 3/5 函数应用（Node $NODE_VERSION / v4 / 消费计划 Linux） =="
az functionapp create \
  --name "$APP_NAME" --resource-group "$RG" \
  --consumption-plan-location "$LOCATION" \
  --runtime node --runtime-version "$NODE_VERSION" --functions-version 4 \
  --os-type Linux --storage-account "$STORAGE" -o none

echo "== 4/5 应用设置（PROXY_KEY + 启用 v4 worker 索引） =="
az functionapp config appsettings set -n "$APP_NAME" -g "$RG" -o none --settings \
  "PROXY_KEY=$PROXY_KEY" \
  "AzureWebJobsFeatureFlags=EnableWorkerIndexing"

echo "== 5/5 发布代码 =="
npm install --omit=dev --no-audit --no-fund
if command -v func >/dev/null; then
  echo "   （检测到 func，用 func 发布）"
  # sync triggers 偶发 BadRequest（部署已成功、只是触发器登记抖动）——不让它中止脚本，
  # 后面仍打印地址；真不行就按提示 restart 后 curl 自检。
  func azure functionapp publish "$APP_NAME" --javascript \
    || echo "   [提示] func 发布返回非零（多半是 sync triggers 抖动，部署本身通常已成功）。若 curl 自检 404，重启后重试： az functionapp restart -n $APP_NAME -g $RG"
else
  echo "   （未检测到 func，改用 az zip 部署——把源码 + node_modules 打包上传）"
  ZIP="$(pwd)/.deploy-package.zip"
  rm -f "$ZIP"
  # 我们已本地 npm install，随包上传 node_modules，关掉远端构建。
  az functionapp config appsettings set -n "$APP_NAME" -g "$RG" -o none \
    --settings "SCM_DO_BUILD_DURING_DEPLOYMENT=false"
  make_zip() {
    if command -v zip >/dev/null; then
      zip -r -q "$ZIP" host.json package.json src node_modules
    elif command -v powershell >/dev/null; then
      powershell -NoProfile -Command \
        "Compress-Archive -Force -Path host.json,package.json,src,node_modules -DestinationPath '$ZIP'"
    else
      python -c "import shutil,os; [None]; shutil.make_archive('.deploy-package','zip','.')" \
        && mv .deploy-package.zip "$ZIP" 2>/dev/null || true
    fi
  }
  make_zip
  [ -f "$ZIP" ] || { echo "[错误] 打包失败：装 zip 或 PowerShell 或 python 任一即可。"; exit 1; }
  az functionapp deployment source config-zip -g "$RG" -n "$APP_NAME" --src "$ZIP" -o none
  rm -f "$ZIP"
fi

BASE="https://$APP_NAME.azurewebsites.net/api/g/$PROXY_KEY"
cat <<EOF

============================================================
  部署完成 ✅  把下面三行环境变量配到 WiseCortex 服务端（改完重启进程生效）
============================================================
WC_PROXY_GEMINI_BASE_URL=$BASE/ca
WC_PROXY_GEMINI_AUTH_BASE_URL=$BASE/auth
WC_PROXY_GEMINI_USERINFO_BASE_URL=$BASE/ui

自检（应答 400/401 即通，说明穿透到 Google；403/404 多半是密钥或路由不对）：
  curl -i -X POST "$BASE/auth/token" -d 'grant_type=refresh_token'
============================================================
EOF
