// ── 后端地址解析 ────────────────────────────────────────────────────────────
// 浏览器 WebUI：前端由静态服务器 / nginx 托管，后端经反代同源可达——REST 走相对
//   路径（`/api/...`）、WS 走页面同源（location.host）。nginx 只需把 /api、/ws
//   反代到内网后端即可，无需对外暴露后端端口。
// Tauri 桌面：前端从 tauri.localhost / 自定义协议加载，同源指向 webview 自身，
//   必须显式指向内嵌后端 127.0.0.1:<端口>（否则请求会打回 webview 自己，拿到 index.html）。
//   端口默认 7070，但被占用时后端会顺延，故实际端口由桌面壳注入 window.__WC_PORT__。

const FALLBACK_PORT = 7070;

type Loc = { hostname: string; protocol: string; host?: string };

/** 桌面壳注入的全局：内嵌后端实际绑定的端口。 */
declare global {
  interface Window {
    __WC_PORT__?: unknown;
  }
}

/** backendPort 的入参形状（便于单测注入，不必伪造整个 window）。 */
type PortCarrier = { __WC_PORT__?: unknown };

/**
 * 内嵌后端的实际端口。
 *
 * 桌面壳启动时把真实绑定端口注入成 `window.__WC_PORT__`：7070 被占时后端会顺延
 * 到 7071、7072…，写死 7070 会让前端连到一个根本不存在（或别人的）服务上。
 */
export function backendPort(w: PortCarrier = window): number {
  const p = w.__WC_PORT__;
  return typeof p === "number" && Number.isInteger(p) && p > 0 && p < 65536 ? p : FALLBACK_PORT;
}

/** 是否运行在 Tauri 桌面壳：非 http(s) 页面 / tauri.localhost / 空 host。 */
export function isTauri(loc: Loc = location): boolean {
  return (
    !loc.hostname ||
    loc.hostname === "tauri.localhost" ||
    (loc.protocol !== "http:" && loc.protocol !== "https:")
  );
}

/** 解析后端主机名。Tauri 环境回退到 127.0.0.1；浏览器用页面主机名。 */
export function backendHost(loc: Loc = location): string {
  return isTauri(loc) ? "127.0.0.1" : loc.hostname;
}

/**
 * 后端 HTTP 基址，拼在 `/api/...` 前面。
 * - 浏览器：返回空串 → 请求走**相对路径**（页面同源），由 nginx 反代到后端。
 * - Tauri：返回 `http://127.0.0.1:7070`（内嵌后端，必须绝对地址）。
 */
export function httpBase(loc: Loc = location): string {
  return isTauri(loc) ? `http://127.0.0.1:${backendPort()}` : "";
}

/**
 * 后端 WebSocket 基址。
 * - 浏览器：页面同源（`ws(s)://location.host`），由 nginx 反代 /ws 到后端。
 * - Tauri：`ws://127.0.0.1:7070`（内嵌后端）。
 */
export function wsBase(loc: Loc = location): string {
  if (isTauri(loc)) return `ws://127.0.0.1:${backendPort()}`;
  const scheme = loc.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${loc.host ?? loc.hostname}`;
}
