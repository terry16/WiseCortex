// ── 平台适配：桌面(Tauri) vs 浏览器(WebUI) ────────────────────────────────────
// Win/Mac 走 Tauri 桌面壳，可调原生「选择文件夹」对话框；Linux 是浏览器 WebUI，
// 浏览器拿不到任意服务器目录，只能让用户填绝对路径。判定依据是「是否在 Tauri 内」，
// 而非操作系统——这样「Windows 用户偶尔开浏览器版」也能正确回退到文本输入。

import { t } from "./i18n";

interface TauriDialog {
  open(opts: { directory?: boolean; multiple?: boolean; defaultPath?: string }): Promise<
    string | string[] | null
  >;
}
interface TauriGlobal {
  dialog?: TauriDialog;
}

function tauri(): TauriGlobal | undefined {
  return (globalThis as { __TAURI__?: TauriGlobal }).__TAURI__;
}

/** 是否运行在 Tauri 桌面壳内。 */
export function isTauri(): boolean {
  return tauri() !== undefined;
}

/**
 * 选择一个目录：
 * - Tauri 且暴露了 dialog 插件 → 原生选目录对话框。
 * - 否则（浏览器/WebUI）→ 文本框填绝对路径。
 * 取消返回 null；否则返回去空白后的路径（可能为空串=恢复默认）。
 */
export async function pickDirectory(
  current: string,
  promptLabel = t("platform.cwdPrompt"),
): Promise<string | null> {
  const dlg = tauri()?.dialog;
  if (dlg) {
    const picked = await dlg.open({
      directory: true,
      multiple: false,
      defaultPath: current || undefined,
    });
    if (picked == null) return null;
    return (Array.isArray(picked) ? (picked[0] ?? "") : picked).trim();
  }
  const v = window.prompt(promptLabel, current);
  return v === null ? null : v.trim();
}

/**
 * 在系统浏览器打开一个 URL。
 * Tauri 桌面壳里 `window.open` 不会调起系统浏览器；若壳内暴露了 opener/shell 插件则优先用之，
 * 否则回退 `window.open`（浏览器版有效）。返回是否「可能已打开」——失败时调用方应展示 URL 供手动打开。
 */
export async function openExternal(url: string): Promise<boolean> {
  const t = tauri() as
    | (TauriGlobal & {
        opener?: { openUrl?(u: string): Promise<void> };
        shell?: { open?(u: string): Promise<void> };
      })
    | undefined;
  try {
    if (t?.opener?.openUrl) {
      await t.opener.openUrl(url);
      return true;
    }
    if (t?.shell?.open) {
      await t.shell.open(url);
      return true;
    }
  } catch {
    // 落到下方 window.open 兜底
  }
  return !!window.open(url, "_blank", "noopener");
}

/** 交给系统打开的协议。其余（相对路径、页内锚点、javascript: 等）一律走默认行为。 */
const EXTERNAL_SCHEME = /^(https?|mailto):/i;

/**
 * 全局接管链接点击，改为在系统浏览器（桌面壳）/新标签页（浏览器版）打开。
 *
 * 桌面壳本身就是个 webview，`<a href>` 的默认行为会把**整个应用**导航走：聊天界面被目标
 * 网页整个顶掉，而壳里没有地址栏也没有后退键，等于把正在进行的会话弄丢。assistant 消息由
 * marked 渲染（见 sessions.ts 的 `renderMarkdown`），产出的 `<a>` 不带 target，所以必须在
 * 这里统一拦一道，而不是指望每个渲染点自己记得加。
 *
 * 用捕获阶段，抢在任何组件自己的 click 处理之前；返回卸载函数（测试与热重载用）。
 */
export function installExternalLinkHandler(root: Document = document): () => void {
  const onClick = (ev: MouseEvent): void => {
    // 已被别处处理掉的、以及非左键的，一概不管。
    if (ev.defaultPrevented || ev.button !== 0) return;
    const a = (ev.target as Element | null)?.closest?.("a[href]") as HTMLAnchorElement | null;
    if (!a) return;
    const href = a.getAttribute("href") ?? "";
    if (!EXTERNAL_SCHEME.test(href)) return;
    ev.preventDefault();
    void openExternal(href);
  };
  root.addEventListener("click", onClick, true);
  return () => root.removeEventListener("click", onClick, true);
}

/**
 * 复制文本到剪贴板，带降级。
 * `navigator.clipboard` 仅在安全上下文（HTTPS / localhost）可用——纯 HTTP 部署下它 undefined，
 * 故兜底用旧版 `document.execCommand("copy")`（临时 textarea，非安全上下文也能用）。
 * 返回是否成功。
 */
export async function copyText(text: string): Promise<boolean> {
  // 类型上 clipboard 恒存在，但运行时在非安全上下文（HTTP）是 undefined，故转成可选再判。
  const clip = (globalThis.navigator as Navigator | undefined)?.clipboard as Clipboard | undefined;
  if (clip) {
    try {
      await clip.writeText(text);
      return true;
    } catch {
      // 落到下方旧版兜底
    }
  }
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.left = "-9999px";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.focus();
    ta.select();
    const ok = document.execCommand("copy");
    ta.remove();
    return ok;
  } catch {
    return false;
  }
}
