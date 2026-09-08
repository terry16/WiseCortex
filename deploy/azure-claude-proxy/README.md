# Claude 订阅反代（Azure Functions）

把 WiseCortex 的 Claude 订阅端点经 Azure 中转，服务端出口直连 Anthropic，免挂全局代理。
与 `../azure-gemini-proxy/` 是同一套路数、各自独立部署。

## 它代理什么

| 环境变量 | 指向 | 上游 | 客户端后续拼的路径 |
|---|---|---|---|
| `WC_PROXY_ANTHROPIC_BASE_URL` | `<BASE>/an` | `api.anthropic.com` | `/v1/messages` |
| `WC_PROXY_ANTHROPIC_AUTH_BASE_URL` | `<BASE>/auth` | `platform.claude.com` | `/v1/oauth/token` |

其中 `<BASE> = https://<实际主机名>/api/c/<PROXY_KEY>`。

**授权页（`claude.ai/oauth/authorize`）不走反代**——那是用户浏览器打开的，要吃 claude.ai 登录态，
中转没有意义。所以首次登录那一步，仍需你本机能打开 claude.ai。

## 部署

在门户里建好一个 **Flex 消耗计划** 的 Node 函数应用，然后：

```bash
APP_NAME=<函数应用名> RG=<资源组名> bash publish.sh
```

脚本会：配 `PROXY_KEY` 与 `AzureWebJobsFeatureFlags` → 装依赖 → 发布 → **去 Azure 确认函数真的
索引出来了** → **冒烟自测两个上游是否真打穿** → 打印要配到服务端的两行环境变量。

想固定密钥就带上 `PROXY_KEY=<自定义长随机串>`，不带则每次重新随机生成
（重新生成意味着旧地址立即失效，记得同步改服务端环境变量）。

## 踩过的坑（改之前先读）

- **不要硬拼 `$APP_NAME.azurewebsites.net`。** Flex 消耗计划的主机名带随机后缀
  （形如 `<app>-<随机串>.<区域>-01.azurewebsites.net`），硬拼出来的地址不通。
  `publish.sh` 改成从 `az functionapp show` 查 `defaultHostName`。
  （`../azure-gemini-proxy/publish.sh` 仍是硬拼的老写法，哪天动它记得一并修。）

- **User-Agent 只透传、绝不改写。** 这是硬约束：推理端点过 WAF *要求* `claude-code/*` UA，
  而 token 端点有反滥用规则，自称 `claude-code/*` 的换 token 请求会被回 **429**
  （实测记录见 `crates/core/src/llm/oauth.rs` 的 `CLAUDE_CODE_USER_AGENT` 注释）。
  客户端已按端点分别选好 UA，反代任何"补个默认 UA"的好意都会打破其中一边。

- **必须剥掉 Azure 平台注入头。** App Service 会塞 `x-arr-*` / `x-client-ip` / `disguised-host`
  等一堆头。转给 Anthropic 既泄露真实客户端 IP，又让请求指纹一眼看出经 Azure 转发，
  而 Anthropic 的 WAF 对请求形状相当敏感。`proxy.js` 按前缀整体剥。
  （Claude Code / Anthropic SDK 发的是 `x-stainless-*` 和 `anthropic-*`，不撞这些前缀，误伤不了。）

- **响应必须丢掉 `content-encoding`。** fetch 已经替我们解压过了，再带上这个头客户端会二次
  解压报错。

- **`app.setup({ enableHttpStream: true })` 不能少**，否则 SSE 被整段缓冲——Claude 是逐字流式的，
  缓冲了体验就废了。

- **`func azure functionapp publish` 必须带 `--javascript`**，否则会以
  「Can't determine project language / project set to None」失败。`func` 不在就自动退回
  `az zip` 部署。

- **别 `npm i -g azure-functions-core-tools@4`**（`ERR_REQUIRE_ESM`）。用
  `winget install Microsoft.Azure.FunctionsCoreTools`，或干脆不装、走 zip 部署。

- **Flex，不要 Linux 消耗计划。** 后者冷启动慢、动不动 503，且有 230s 单请求上限——长回答会被切断。

## 自检

```bash
# 打穿 = 返回上游的业务错误码；404 说明密钥/路由不对，000 说明没连上
curl -i -X POST "<BASE>/auth/v1/oauth/token" -d "grant_type=refresh_token"   # 期望 400
curl -i -X POST "<BASE>/an/v1/messages" -H "anthropic-version: 2023-06-01" -d "{}"  # 期望 401
```

`401`（而非 `400 anthropic-version header is required`）本身就是个有用信号：说明
`anthropic-version` 头确实原样穿过去了。
