"use strict";
// Claude 订阅反代（Azure Functions，Node v4）。
//
// WiseCortex 的 Claude 订阅走两个 Anthropic 上游；把它们经本函数反代，客户端只需设两个环境变量
// 即可免科学上网（服务端在境外，出口直连 Anthropic）：
//   WC_PROXY_ANTHROPIC_BASE_URL      → .../api/c/<KEY>/an    （推理：api.anthropic.com，拼 /v1/messages）
//   WC_PROXY_ANTHROPIC_AUTH_BASE_URL → .../api/c/<KEY>/auth  （token：platform.claude.com，拼 /v1/oauth/token）
//
// 授权页（claude.ai/oauth/authorize）**不**走反代：那是用户浏览器打开的，要吃 claude.ai 登录态。
//
// 路径密钥加固：URL 里的 <KEY> 必须等于应用设置 PROXY_KEY，否则一律 404（不暴露是否存在）。
// SSE 流式：/v1/messages 的 stream 响应体原样透传（见下方 enableHttpStream）。
//
// ⚠️ 本反代对 User-Agent「只透传、不改写」，这是硬约束而非风格问题：
//    推理端点过 WAF **要求** `claude-code/*` UA；而 token 端点有反滥用规则，
//    自称 `claude-code/*` 的换 token 请求会被回 429（见 crates/core/src/llm/oauth.rs 的实测注释）。
//    客户端已按端点分别选好 UA，反代任何"补一个默认 UA"的好意都会打破其中一边。

const { app } = require("@azure/functions");

// 打开 HTTP 流式（让 /v1/messages 的 SSE 边生成边回，而非整段缓冲）。
try {
  app.setup({ enableHttpStream: true });
} catch (_) {
  // 旧版 @azure/functions 无此 API：退化为缓冲响应，功能仍在（长响应可能受平台超时限制）。
}

// target 段 → 上游主机。
const UPSTREAM = {
  an: "api.anthropic.com",
  auth: "platform.claude.com",
};

// 不向上游转发的逐跳/由 fetch 自行管理的请求头。
const DROP_REQ_HEADERS = new Set([
  "host",
  "connection",
  "content-length",
  "transfer-encoding",
  "keep-alive",
  "max-forwards",
  "client-ip",
  "disguised-host",
  "was-default-hostname",
  "x-original-url",
]);

// Azure App Service 会往入站请求里塞一堆平台头。转给 Anthropic 既泄露真实客户端 IP
// （x-client-ip）、又让请求指纹一眼看出"经 Azure 转发"。Anthropic 的 WAF 对请求形状
// 相当敏感（见上方 UA 注释），所以这里按前缀整体剥掉。
// 注意：Claude Code / Anthropic SDK 发的是 x-stainless-* 与 anthropic-*，不撞这些前缀。
const DROP_REQ_PREFIXES = [
  "x-arr-",
  "x-ms-",
  "x-appservice-",
  "x-waws-",
  "x-site-",
  "x-forwarded-",
  "x-client-",
];

function dropReqHeader(name) {
  const k = name.toLowerCase();
  return DROP_REQ_HEADERS.has(k) || DROP_REQ_PREFIXES.some((p) => k.startsWith(p));
}

// 不回传给客户端的响应头（由平台/fetch 管理）。
const DROP_RESP_HEADERS = new Set([
  "content-length",
  "transfer-encoding",
  "connection",
  "content-encoding", // fetch 已解压；带上会让客户端二次解压出错
]);

app.http("claude-proxy", {
  methods: ["GET", "POST"],
  authLevel: "anonymous", // 自己用 PROXY_KEY 做鉴权，不用 Azure 的 function key
  route: "c/{key}/{target}/{*restPath}",
  handler: async (request, context) => {
    const expected = process.env.PROXY_KEY || "";
    const { key, target } = request.params;
    // 密钥不对 / 未配置 / target 非法：统一 404，不泄露存在性。
    if (!expected || key !== expected || !UPSTREAM[target]) {
      return { status: 404, body: "Not found" };
    }

    const host = UPSTREAM[target];
    const restPath = request.params.restPath || "";
    // 保留原始 query。
    const search = new URL(request.url).search || "";
    const upstreamUrl = `https://${host}/${restPath}${search}`;

    // 透传请求头（去掉逐跳头、host 与 Azure 平台注入头）。
    const headers = {};
    for (const [k, v] of request.headers.entries()) {
      if (!dropReqHeader(k)) headers[k] = v;
    }

    // 请求体一般不大（token 表单 / messages JSON），整读后转发；响应体才需要流式。
    let body;
    if (request.method !== "GET" && request.method !== "HEAD") {
      const buf = Buffer.from(await request.arrayBuffer());
      body = buf.length ? buf : undefined;
    }

    let upstream;
    try {
      upstream = await fetch(upstreamUrl, { method: request.method, headers, body });
    } catch (e) {
      context.error(`upstream fetch failed: ${e}`);
      return { status: 502, body: `Bad gateway: ${e}` };
    }

    const respHeaders = {};
    upstream.headers.forEach((v, k) => {
      if (!DROP_RESP_HEADERS.has(k.toLowerCase())) respHeaders[k] = v;
    });

    // 响应体原样流式透传（SSE 边到边回）。upstream.body 是 Web ReadableStream。
    return { status: upstream.status, headers: respHeaders, body: upstream.body };
  },
});
