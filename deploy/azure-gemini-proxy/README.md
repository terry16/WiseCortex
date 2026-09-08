# WiseCortex · Gemini 订阅反代（Azure Functions）

把 Gemini 订阅（Code Assist）用到的三个 Google 上游经 Azure Functions 反代，客户端只配三个环境变量即可**免科学上网**——反代跑在境外（日本东部 `japaneast`），出口直连 Google。和 Claude/Codex/Grok 三家的 `WC_PROXY_*` 反代同一套路，额外加了**路径密钥**加固。

## 反代了什么

| 用途 | 上游 | 客户端环境变量 |
|---|---|---|
| Code Assist 推理 | `cloudcode-pa.googleapis.com` | `WC_PROXY_GEMINI_BASE_URL` |
| OAuth token 兑换/刷新 | `oauth2.googleapis.com` | `WC_PROXY_GEMINI_AUTH_BASE_URL` |
| 登录邮箱（userinfo） | `www.googleapis.com` | `WC_PROXY_GEMINI_USERINFO_BASE_URL` |

> 注意：**Google 授权页**（`accounts.google.com`，用户浏览器打开的那一步）不走反代——它要吃你本地浏览器的 Google 登录态，反代它反而登不上。只有上面三个「机器对机器」的端点才反代。

## 推荐：门户建 Flex 应用 + `publish.sh`（只发布代码）

实测 **Linux 消耗计划**在本场景冷启动慢、爱抽 503；**Flex 消耗计划**才稳（和你已跑通的
OpenAI 反代同款）。所以推荐：**在 Azure 门户里自己建应用**（Flex 消耗、Node 24、Functions v4），
然后只用本目录的 `publish.sh` 把代码发上去：

```bash
# az login 之后，用你在门户建好的应用名 + 资源组：
APP_NAME=<你的函数应用名> RG=<资源组名> bash publish.sh
```

`publish.sh` 只做三件事：配 `PROXY_KEY` → 装依赖 → 发布，最后打印三行 `WC_PROXY_GEMINI_*`。
下面的 `deploy.sh`（自动建全套资源）是 Linux 消耗计划的一键版，留作备选。

## 部署（一条命令，Linux 消耗计划，备选）

前置（只做一次，交互式，**需你本人操作**）：

```powershell
# 1) 装 Azure CLI（必需）
winget install -e --id Microsoft.AzureCLI        # 或 https://aka.ms/azcli
# 2) 登录 Azure（弹浏览器，需你本人完成）
az login
# 3)（可选）装 Functions Core Tools，发布更顺；不装则脚本自动改用 az zip 部署
winget install Microsoft.Azure.FunctionsCoreTools
```

> ⚠️ **别用 npm 装 func**：`npm i -g azure-functions-core-tools@4` 会报 `ERR_REQUIRE_ESM`
> ——这是该 npm 包自身的安装 bug（`install.js` 里 `require('https-proxy-agent')`，而新版
> https-proxy-agent 是 ESM-only）。用上面的 winget 装，或干脆不装（`deploy.sh` 没有 func
> 时会自动用 `az functionapp deployment source config-zip` 部署，只需 `az`）。

然后：

```bash
cd deploy/azure-gemini-proxy
bash deploy.sh
```

脚本会：建资源组 → 建存储账户 → 建函数应用（Node 20 / Functions v4 / 消费计划 Linux，区域 `japaneast`）→ 写入随机 `PROXY_KEY` → 发布代码，最后**打印出要配到 WiseCortex 的三行环境变量**。

想固定名字/密钥就带环境变量覆盖：

```bash
APP_NAME=my-gemini-proxy PROXY_KEY=$(openssl rand -hex 24) bash deploy.sh
```

## 配到 WiseCortex

把脚本末尾打印的三行加到 WiseCortex 服务端环境（Ubuntu 部署即 systemd/宝塔的运行环境），**改完重启进程**才生效：

```
WC_PROXY_GEMINI_BASE_URL=https://<APP>.azurewebsites.net/api/g/<KEY>/ca
WC_PROXY_GEMINI_AUTH_BASE_URL=https://<APP>.azurewebsites.net/api/g/<KEY>/auth
WC_PROXY_GEMINI_USERINFO_BASE_URL=https://<APP>.azurewebsites.net/api/g/<KEY>/ui
```

自检（穿透即返回 400/401，说明打到了 Google；返回 404 多半是密钥或路由不对）：

```bash
curl -i -X POST "https://<APP>.azurewebsites.net/api/g/<KEY>/auth/token" -d 'grant_type=refresh_token'
```

## 加固与限制

- **路径密钥**：URL 里的 `<KEY>` 必须等于应用设置 `PROXY_KEY`，否则一律 `404`（不暴露存在性）。想换密钥：`az functionapp config appsettings set -n <APP> -g <RG> --settings PROXY_KEY=<新值>`，同步改三条 `WC_PROXY_*`。
- **SSE 流式**：`:streamGenerateContent?alt=sse` 的响应体原样边到边透传（`enableHttpStream`）。
- **消费计划有 230s 单请求上限**：绝大多数回合够用；若遇到超长单次生成被截断，升级到 **Flex 消费**或 **Premium** 计划即可（把 `deploy.sh` 里的 `--consumption-plan-location` 换成对应计划）。
- 仅个人自用；`PROXY_KEY` 请用足够长的随机串，别外泄。

## 拆除

```bash
az group delete -n wisecortex-gemini-rg --yes --no-wait
```
