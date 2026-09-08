// ── 产物预览面板（artifact）──────────────────────────────────────────────────
// agent 生成 .md/.html/.svg/代码等文件后，右侧滑出面板预览（预览/代码切换）。
// .md 渲染为 HTML、.html/.svg 直接渲染；其余文本/代码只给「代码」视图。
// 内容经 GET /api/artifact?path= 读取。

import { marked } from "marked";
import { authHeaders } from "./auth";
import { httpBase } from "./backend";
import { t } from "./i18n";
import { icon } from "./icons";

const MD_EXT = /\.(md|markdown)$/i;
const HTML_EXT = /\.(html?|svg)$/i;

/** iframe 内 markdown 渲染的基础样式（瓷白亮色、易读；与主界面青花瓷配色一致）。 */
const MD_CSS = `
  :root { color-scheme: light; }
  body { margin: 0; padding: 24px 28px; max-width: 860px;
    font: 15px/1.7 -apple-system,"Segoe UI",Roboto,"Helvetica Neue",Arial,"PingFang SC","Microsoft YaHei",sans-serif;
    color: #1c2a33; background: #fbfcfc; word-wrap: break-word; }
  h1,h2,h3,h4 { line-height: 1.3; margin: 1.4em 0 .6em; font-weight: 650; }
  h1 { font-size: 1.7em; } h2 { font-size: 1.4em; border-bottom: 1px solid #d7dfe2; padding-bottom: .3em; }
  h3 { font-size: 1.18em; } p { margin: .7em 0; }
  a { color: #2e5fa3; } code { font-family: ui-monospace,SFMono-Regular,Consolas,monospace; font-size: .9em;
    background: #e9eef1; padding: .15em .4em; border-radius: 4px; }
  pre { background: #f3f6f7; border: 1px solid #d7dfe2; border-radius: 8px; padding: 12px 14px; overflow: auto; }
  pre code { background: none; padding: 0; font-size: .86em; line-height: 1.55; }
  blockquote { margin: .8em 0; padding: .2em 1em; border-left: 3px solid #c2ced2; color: #4e6170; }
  table { border-collapse: collapse; margin: .9em 0; display: block; overflow: auto; }
  th,td { border: 1px solid #d7dfe2; padding: 6px 11px; text-align: left; }
  th { background: #f3f6f7; } img { max-width: 100%; }
  ul,ol { padding-left: 1.5em; } li { margin: .25em 0; } hr { border: none; border-top: 1px solid #d7dfe2; margin: 1.5em 0; }
`;

/** 把 markdown 渲染成自包含的 HTML 文档（带样式），用于 iframe srcdoc。 */
function renderMarkdownDoc(md: string): string {
  const body = marked.parse(md, { async: false }) as string;
  return `<!doctype html><html><head><meta charset="utf-8"><style>${MD_CSS}</style></head><body>${body}</body></html>`;
}

export function mountArtifact(): { show: (path: string, cwd?: string) => void } {
  const panel = document.createElement("div");
  panel.className = "artifact-panel";
  panel.hidden = true;
  panel.innerHTML = `
    <div class="ap-head">
      <span class="ap-title">${icon("doc", 16)}<span id="ap-name">artifact</span></span>
      <span class="seg ap-seg"><button data-t="preview" class="on">${t("artifact.tab.preview")}</button><button data-t="code">${t("artifact.tab.code")}</button></span>
      <span style="flex:1"></span>
      <button id="ap-refresh" class="btn btn-ghost btn-sm btn-icon" title="${t("common.refresh")}">${icon("refresh", 16)}</button>
      <button id="ap-close" class="btn btn-ghost btn-sm btn-icon" title="${t("common.close")}">${icon("x", 17)}</button>
    </div>
    <iframe id="ap-frame" class="ap-frame" sandbox="allow-scripts"></iframe>
    <pre id="ap-code" class="ap-code" hidden></pre>`;
  document.body.appendChild(panel);

  const q = <T extends HTMLElement>(s: string) => panel.querySelector(s) as T;
  const frame = q<HTMLIFrameElement>("#ap-frame");
  const code = q<HTMLPreElement>("#ap-code");
  const previewBtn = q<HTMLButtonElement>('.ap-seg button[data-t="preview"]');
  let current = "";
  let currentCwd = "";

  /** 切换「预览/代码」视图；canPreview=false 时隐藏「预览」按钮并强制代码视图。 */
  function setView(active: "preview" | "code", canPreview: boolean): void {
    previewBtn.hidden = !canPreview;
    const view = canPreview ? active : "code";
    for (const b of panel.querySelectorAll<HTMLElement>(".ap-seg button")) {
      b.classList.toggle("on", b.dataset.t === view);
    }
    frame.hidden = view !== "preview";
    code.hidden = view !== "code";
  }

  async function load(path: string, cwd = ""): Promise<void> {
    current = path;
    currentCwd = cwd;
    const qs = cwd ? `&cwd=${encodeURIComponent(cwd)}` : "";
    const r = await fetch(`${httpBase()}/api/artifact?path=${encodeURIComponent(path)}${qs}`, {
      headers: authHeaders(),
    });
    const data = (await r.json()) as {
      ok?: boolean;
      name?: string;
      content?: string;
      error?: string;
    };
    if (!data.ok) {
      code.textContent = t("artifact.readFailed", { e: data.error ?? "" });
      frame.srcdoc = "";
      setView("code", false);
      return;
    }
    q("#ap-name").textContent = data.name ?? "artifact";
    const content = data.content ?? "";
    code.textContent = content;
    if (MD_EXT.test(path)) {
      frame.srcdoc = renderMarkdownDoc(content);
      setView("preview", true);
    } else if (HTML_EXT.test(path)) {
      frame.srcdoc = content;
      setView("preview", true);
    } else {
      // 普通文本/代码：无可渲染预览，直接看源码。
      frame.srcdoc = "";
      setView("code", false);
    }
  }

  for (const b of panel.querySelectorAll<HTMLElement>(".ap-seg button")) {
    b.addEventListener("click", () => {
      const t = b.dataset.t;
      for (const x of panel.querySelectorAll(".ap-seg button")) x.classList.toggle("on", x === b);
      frame.hidden = t !== "preview";
      code.hidden = t !== "code";
    });
  }
  q("#ap-refresh").addEventListener("click", () => void load(current, currentCwd));
  q("#ap-close").addEventListener("click", () => {
    panel.hidden = true;
  });

  return {
    show: (path: string, cwd = "") => {
      panel.hidden = false;
      void load(path, cwd);
    },
  };
}
