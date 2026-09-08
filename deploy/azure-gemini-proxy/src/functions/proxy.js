"use strict";
// Gemini 订阅反代（Azure Functions，Node v4）。
//
// WiseCortex 的 Gemini 订阅走三个 Google 上游；把它们经本函数反代，客户端只需设三个环境变量即可
// 免科学上网（服务端在境外，出口直连 Google）：
//   WC_PROXY_GEMINI_BASE_URL       → .../api/g/<KEY>/ca    （Code Assist：cloudcode-pa.googleapis.com）
//   WC_PROXY_GEMINI_AUTH_BASE_URL  → .../api/g/<KEY>/auth  （token：oauth2.googleapis.com）
//   WC_PROXY_GEMINI_USERINFO_BASE_URL → .../api/g/<KEY>/ui （userinfo：www.googleapis.com）
//
// 路径密钥加固：URL 里的 <KEY> 必须等于应用设置 PROXY_KEY，否则一律 404（不暴露是否存在）。
// SSE 流式：Code Assist 的 :streamGenerateContent?alt=sse 响应体原样透传（见 host 里 enableHttpStream）。

const { app } = require("@azure/functions");

// 打开 HTTP 流式（让 :streamGenerateContent 的 SSE 边生成边回，而非整段缓冲）。
try {
  app.setup({ enableHttpStream: true });
} catch (_) {
  // 旧版 @azure/functions 无此 API：退化为缓冲响应，功能仍在（长响应可能受平台超时限制）。
}

// target 段 → 上游主机。
const UPSTREAM = {
  ca: "cloudcode-pa.googleapis.com",
  auth: "oauth2.googleapis.com",
  ui: "www.googleapis.com",
};

// 不向上游转发的逐跳/由 fetch 自行管理的请求头。
const DROP_REQ_HEADERS = new Set([
  "host",
  "connection",
  "content-length",
  "transfer-encoding",
  "keep-alive",
  "x-forwarded-for",
  "x-forwarded-host",
  "x-forwarded-proto",
  "x-arr-log-id",
  "x-original-url",
  "x-waws-unencoded-url",
  "disguised-host",
  "max-forwards",
]);

// 不回传给客户端的响应头（由平台/fetch 管理）。
const DROP_RESP_HEADERS = new Set([
  "content-length",
  "transfer-encoding",
  "connection",
  "content-encoding", // fetch 已解压；带上会让客户端二次解压出错
]);

app.http("gemini-proxy", {
  methods: ["GET", "POST"],
  authLevel: "anonymous", // 自己用 PROXY_KEY 做鉴权，不用 Azure 的 function key
  route: "g/{key}/{target}/{*restPath}",
  handler: async (request, context) => {
    const expected = process.env.PROXY_KEY || "";
    const { key, target } = request.params;
    // 密钥不对 / 未配置 / target 非法：统一 404，不泄露存在性。
    if (!expected || key !== expected || !UPSTREAM[target]) {
      return { status: 404, body: "Not found" };
    }

    const host = UPSTREAM[target];
    const restPath = request.params.restPath || "";
    // 保留原始 query（含 alt=sse）。
    const search = new URL(request.url).search || "";
    const upstreamUrl = `https://${host}/${restPath}${search}`;

    // 透传请求头（去掉逐跳头与 host）。
    const headers = {};
    for (const [k, v] of request.headers.entries()) {
      if (!DROP_REQ_HEADERS.has(k.toLowerCase())) headers[k] = v;
    }

    // 请求体一般很小（token 表单 / CA JSON），整读后转发；响应体才需要流式。
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
