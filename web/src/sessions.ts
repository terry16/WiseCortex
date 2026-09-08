// ── Sessions — 最小对话渲染模块 ────────────────────────────────────────────
//
// 实现 ws-dispatcher.ts 的 SessionsModule 契约（MVP 版）。对话相关方法是真实渲染，
// session 列表 / 路由相关方法暂为最小实现，待列表与路由需求定下来后再补齐。
//
// assistant 消息按 markdown 渲染（marked + highlight.js）；其余 role 的 html 由
// 调用方（dispatcher）预先转义，这里按可信 HTML 注入——转义责任归调用方，本模块不重复转义。
// ──────────────────────────────────────────────────────────────────────────

import hljs from "highlight.js/lib/common";
import { marked } from "marked";
import { confirmDialog } from "./confirm";
import { t } from "./i18n";
import { icon } from "./icons";
import type { WsMessage } from "./ws";
import type { Session, SessionId, SessionsModule } from "./ws-dispatcher";

/** 历史回放的一条消息（来自 GET /api/sessions/:id/messages）。 */
export interface HistoryMessage {
  role: string;
  content: string | null;
  tool_calls?: { name: string; arguments: string }[];
  /** 用户消息附带的图片（data URL）；历史回放时重画为缩略图。 */
  images?: string[];
}

export interface SessionsOptions {
  /** 消息流容器。 */
  messages: HTMLElement;
  /** 会话列表侧边栏容器（可选）。 */
  sidebar?: HTMLElement;
  /** 点击会话项切换。 */
  onSwitch?: (id: SessionId) => void;
  /** 点击删除按钮。 */
  onDelete?: (id: SessionId) => void;
  /** 内联重命名提交（已去除首尾空白、保证非空且有变化）。 */
  onRename?: (id: SessionId, name: string) => void;
  /** 以目标会话的工作目录新建会话。 */
  onNewSession?: (workingDir: string) => void;
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/**
 * 距底部 ≤ 此像素即算「贴底」。留点余量：亚像素缩放 / 行高取整下，`scrollTop` 到不了
 * 严丝合缝的底，卡 0 会让跟随随机失效。
 */
const BOTTOM_SLACK_PX = 40;

/** 思考预览：取首个非空行（首句），截断到一行长度，作折叠标题。 */
function thinkingPreview(s: string): string {
  const line =
    s
      .split("\n")
      .map((x) => x.trim())
      .find((x) => x.length > 0) ?? "";
  const max = 80;
  return line.length > max ? `${line.slice(0, max)}…` : line;
}

export class Sessions implements SessionsModule {
  activeId: SessionId | null = null;

  private readonly messages: HTMLElement;
  private readonly sidebar?: HTMLElement;
  private readonly onSwitch?: (id: SessionId) => void;
  private readonly onDelete?: (id: SessionId) => void;
  private readonly onRename?: (id: SessionId, name: string) => void;
  private readonly onNewSession?: (workingDir: string) => void;
  private readonly store = new Map<SessionId, Session>();
  private progressEl: HTMLElement | null = null;
  /** 进度计时器：每秒刷新已用时（让「压缩上下文…」等无明确进度的阶段也明显在动）。 */
  private progressTimer: ReturnType<typeof setInterval> | null = null;
  private progressStart = 0;
  /** 当前流式助手气泡（assistant_delta 追加，assistant_message 定稿）。 */
  private streamingEl: HTMLElement | null = null;
  /** 当前工具项的可折叠正文容器（tool_result/stdout 追加到这里）。 */
  private lastToolBody: HTMLElement | null = null;
  private pendingRunTask: SessionId | null = null;
  private pendingMessage: { session_id: SessionId; content: string } | null = null;

  constructor(opts: SessionsOptions) {
    this.messages = opts.messages;
    this.sidebar = opts.sidebar;
    this.onSwitch = opts.onSwitch;
    this.onDelete = opts.onDelete;
    this.onRename = opts.onRename;
    this.onNewSession = opts.onNewSession;
  }

  /** 清空对话区（切换会话时）。 */
  clearChat(): void {
    this.stopProgressTimer();
    this.messages.innerHTML = "";
    this.progressEl = null;
    this.lastToolBody = null;
    this.streamingEl = null;
  }

  private stopProgressTimer(): void {
    if (this.progressTimer !== null) {
      clearInterval(this.progressTimer);
      this.progressTimer = null;
    }
  }

  /** 设置当前会话并刷新侧边栏高亮。 */
  setActive(id: SessionId | null): void {
    this.activeId = id;
    this.renderList();
  }

  /** 回放历史消息（清空后重画）。 */
  renderHistory(messages: HistoryMessage[]): void {
    this.clearChat();
    for (const m of messages) {
      if (m.role === "user") {
        const thumbs = (m.images ?? [])
          .map((u) => `<img class="attach-thumb" src="${escapeHtml(u)}">`)
          .join("");
        this.appendMsg("user", escapeHtml(m.content ?? "") + thumbs);
      } else if (m.role === "assistant") {
        if (m.content) this.appendMsg("assistant", m.content);
        for (const tc of m.tool_calls ?? []) {
          this.appendToolCall(tc.name, tc.arguments, tc.name);
        }
      } else if (m.role === "tool") {
        this.appendToolResult(m.content ?? "");
      }
    }
  }

  // ── 渲染辅助 ──────────────────────────────────────────────────────────
  private renderMarkdown(src: string): string {
    return marked.parse(src, { async: false }) as string;
  }

  /** 是否贴在底部（= 用户没有手工上滚去看历史）。**必须在改 DOM 之前测**：见 [`follow`]。 */
  private isAtBottom(): boolean {
    const el = this.messages;
    return el.scrollHeight - el.scrollTop - el.clientHeight <= BOTTOM_SLACK_PX;
  }

  /**
   * 跟随滚动到底，但**只在改 DOM 之前就已贴底时**才滚——即用户没有手工上滚去看历史。
   *
   * `pinned` 必须在 append **之前**用 [`isAtBottom`] 测好再传进来：append 之后 scrollHeight
   * 已经变大，那时再测永远算不出「贴底」，跟随会当场全废。滚回底部后下一条 append 自然又
   * 测到贴底、自动恢复跟随——无需任何状态位。此写法与 app.ts / tasksView.ts 一致。
   */
  private follow(pinned: boolean): void {
    if (pinned) this.messages.scrollTop = this.messages.scrollHeight;
  }

  private highlightWithin(el: HTMLElement): void {
    for (const block of el.querySelectorAll<HTMLElement>("pre code")) {
      hljs.highlightElement(block);
    }
  }

  // ── 对话消息（真实渲染） ──────────────────────────────────────────────
  /** 流式增量：追加纯文本到当前流式气泡（首次自动创建）。 */
  appendDelta(text: string): void {
    const pinned = this.isAtBottom(); // 改 DOM 前先测
    if (!this.streamingEl) {
      this.streamingEl = document.createElement("div");
      this.streamingEl.className = "msg msg-assistant streaming";
      this.messages.appendChild(this.streamingEl);
    }
    this.streamingEl.textContent += text;
    this.follow(pinned);
  }

  appendMsg(
    role: string,
    html: string,
    opts?: { time?: Date; forceScroll?: boolean },
  ): HTMLElement {
    const pinned = opts?.forceScroll === true || this.isAtBottom(); // 发送时强制滚底；普通更新仍先测位置
    // assistant 定稿：若有流式气泡，就地渲染 markdown 替换其纯文本。
    if (role === "assistant" && this.streamingEl) {
      const el = this.streamingEl;
      this.streamingEl = null;
      el.classList.remove("streaming");
      el.innerHTML = this.renderMarkdown(html);
      this.highlightWithin(el);
      this.follow(pinned);
      return el;
    }
    const el = document.createElement("div");
    el.className = `msg msg-${role}`;
    if (role === "assistant") {
      el.innerHTML = this.renderMarkdown(html);
      this.highlightWithin(el);
    } else {
      // 包一层 <span>：避免裸文本节点直接挂在 flex 子项上——部分 WebView 下这样无法起始选区
      // （助手消息因 markdown 自带块级元素不受影响，用户消息是裸文本故选不中）。
      el.innerHTML = `<span class="msg-text">${html}</span>`; // 调用方已转义/可信
    }
    this.messages.appendChild(el);
    this.follow(pinned);
    return el;
  }

  /** 思考内容：可折叠块，默认收起只显示首句预览，点开看完整推理；排在本轮答复气泡之前。 */
  appendThinking(content: string): void {
    const text = content.trim();
    if (!text) return;
    const pinned = this.isAtBottom(); // 改 DOM 前先测
    const details = document.createElement("details");
    details.className = "thinking";
    const sum = document.createElement("summary");
    sum.textContent = t("chat.thinking", { preview: thinkingPreview(text) });
    details.appendChild(sum);
    const body = document.createElement("div");
    body.className = "thinking-body";
    body.textContent = text;
    details.appendChild(body);
    // 思考排在「本轮答复」之前：若有进行中的流式气泡，插到它前面，否则直接追加。
    if (this.streamingEl) {
      this.messages.insertBefore(details, this.streamingEl);
    } else {
      this.messages.appendChild(details);
    }
    this.follow(pinned);
  }

  appendInfo(main: string, sub?: string | null): void {
    const pinned = this.isAtBottom(); // 改 DOM 前先测
    const el = document.createElement("div");
    el.className = "msg msg-info";
    el.textContent = sub ? `${main} · ${sub}` : main;
    this.messages.appendChild(el);
    this.follow(pinned);
  }

  appendToolCall(name: string, _args: unknown, summary?: string): void {
    const pinned = this.isAtBottom(); // 改 DOM 前先测
    // 用原生 <details> 折叠：标题常显，结果默认收起，点击展开。
    const details = document.createElement("details");
    details.className = "tool";
    const sum = document.createElement("summary");
    sum.textContent = `🔧 ${summary || name}`;
    details.appendChild(sum);
    const body = document.createElement("div");
    body.className = "tool-body";
    details.appendChild(body);
    this.messages.appendChild(details);
    this.lastToolBody = body;
    this.follow(pinned);
  }

  appendToolResult(result: unknown): void {
    const pinned = this.isAtBottom(); // 改 DOM 前先测
    const el = document.createElement("pre");
    el.className = "tool-result";
    el.textContent = typeof result === "string" ? result : JSON.stringify(result, null, 2);
    (this.lastToolBody ?? this.messages).appendChild(el);
    this.follow(pinned);
  }

  appendToolStdout(lines: string[]): void {
    const pinned = this.isAtBottom(); // 改 DOM 前先测
    const el = document.createElement("pre");
    el.className = "tool-stdout";
    el.textContent = lines.join("\n");
    (this.lastToolBody ?? this.messages).appendChild(el);
    this.follow(pinned);
  }

  appendTokenUsage(_ev: WsMessage): void {
    // MVP：暂不展示 token 明细。
  }

  showProgress(
    message: string | undefined,
    _progressType: string,
    _metadata: Record<string, unknown>,
    _startedAt: number | null,
  ): void {
    // 已经在流式吐字了 → 不再显示「思考中…」：模型明摆着已经在答了。
    //
    // 这里不只是观感问题。服务端在流式期间仍会持续推 progress（每个 Usage 事件一条，
    // 见 agent.rs 的 StreamUpdate::Usage），而 assistant_delta 又会 clearProgress——
    // 于是「建一个 → 删掉 → 再建一个」来回循环，且每次重建都 append 到消息流末尾、
    // 跑到流式气泡**下面**。高度反复增减，就是「thinking 一直刷新 + UI 上下跳」。
    // 定稿（appendMsg）会把 streamingEl 清空，届时 progress 自然恢复显示。
    if (this.streamingEl) {
      this.clearProgress();
      return;
    }
    const pinned = this.isAtBottom(); // 改 DOM 前先测
    if (!this.progressEl) {
      this.progressEl = document.createElement("div");
      this.progressEl.className = "progress";
      this.progressEl.innerHTML = `<span class="spin"></span><span class="progress-msg"></span><span class="progress-time"></span>`;
      this.messages.appendChild(this.progressEl);
      // 启动计时：每秒刷新「· Ns」，配合旋转动画，明确表明在工作（不像挂起）。
      this.progressStart = Date.now();
      const tick = (): void => {
        const t = this.progressEl?.querySelector(".progress-time") as HTMLElement | null;
        if (t) t.textContent = ` · ${Math.floor((Date.now() - this.progressStart) / 1000)}s`;
      };
      this.progressTimer = setInterval(tick, 1000);
      tick();
    }
    const msg = this.progressEl.querySelector(".progress-msg") as HTMLElement | null;
    // 旋转动画始终在转——即便没有明确百分比也表明在工作（如「压缩上下文…」），不像挂起。
    if (msg) msg.textContent = message ?? t("chat.processing");
    else this.progressEl.textContent = message ?? t("chat.processing");
    this.follow(pinned);
  }

  clearProgress(_message?: string): void {
    this.stopProgressTimer();
    this.progressEl?.remove();
    this.progressEl = null;
  }

  clearAllProgress(): void {
    this.clearProgress();
  }

  collapseToolGroup(): void {
    this.lastToolBody = null;
  }

  showFeedbackRequest(question: unknown, _context: unknown, _options: unknown): void {
    this.appendInfo(`❓ ${String(question)}`);
  }

  // ── pending（与 dispatcher 的 subscribed 流程配合） ────────────────────
  setPendingMessage(session_id: SessionId, content: string): void {
    this.pendingMessage = { session_id, content };
  }
  takePendingRunTask(): SessionId | null {
    const id = this.pendingRunTask;
    this.pendingRunTask = null;
    return id;
  }
  takePendingMessage(): { session_id: SessionId; content: string } | null {
    const m = this.pendingMessage;
    this.pendingMessage = null;
    return m;
  }

  // ── session 列表 / 状态（MVP 最小实现） ───────────────────────────────
  setAll(sessions: Session[], _hasMore: boolean, _cronCount: number): void {
    this.store.clear();
    for (const s of sessions) this.store.set(s.id, s);
  }
  /** 某任务的工作目录（来自 session.extra 镜像的 working_dir）；无=空串。 */
  private static workdirOf(s: Session): string {
    const w = (s as { working_dir?: unknown }).working_dir;
    return typeof w === "string" ? w.trim() : "";
  }

  /** 渲染左栏任务列表：分「任务」（无工作目录）与「工作空间」（按目录分组）两段。 */
  renderList(): void {
    if (!this.sidebar) return;
    // 不在历史里堆空会话：仅显示有内容（已命名）、当前选中、或正在运行的会话。
    // 空会话无 name（有首条消息后才自动命名），多次「新建」产生的空会话因此不再堆积。
    const all = [...this.store.values()]
      .filter((s) => !!s.name || s.id === this.activeId || s.status === "working")
      .sort((a, b) => a.id.localeCompare(b.id));
    const leaf = (p: string): string => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
    this.sidebar.replaceChildren();

    const makeRow = (s: Session): HTMLElement => {
      const row = document.createElement("div");
      row.className = `task-row${s.id === this.activeId ? " active" : ""}`;
      const working = s.status === "working";
      // 无名任务即「当前任务」（空会话不持久化、有了首条消息才自动命名）。
      const label = (s.name as string | undefined) || t("sidebar.currentTask");
      const name = document.createElement("span");
      name.className = "task-name";
      name.textContent = label;
      name.title = label;
      row.append(name);
      // 运行中：行右侧转圈 loading。
      if (working) {
        const spin = document.createElement("span");
        spin.className = "task-spin";
        spin.title = t("sidebar.running");
        row.appendChild(spin);
      }
      const ren = document.createElement("button");
      ren.className = "task-ren";
      ren.innerHTML = icon("pencil", 13);
      ren.title = t("sidebar.renameTask");
      ren.addEventListener("click", (e) => {
        e.stopPropagation();
        this.beginRename(row, s);
      });
      row.appendChild(ren);
      const del = document.createElement("button");
      del.className = "task-del";
      del.textContent = "✕";
      del.title = t("sidebar.deleteTask");
      del.addEventListener("click", async (e) => {
        e.stopPropagation();
        // 删会话不可撤销 —— 先弹确认，避免误删。
        const name = (s.name as string | undefined)?.trim() || t("sidebar.currentTask");
        const ok = await confirmDialog({
          title: t("sessions.deleteConfirm.title"),
          message: t("sessions.deleteConfirm.message", { name }),
          confirmLabel: t("common.delete"),
          danger: true,
        });
        if (ok) this.onDelete?.(s.id);
      });
      row.appendChild(del);
      row.addEventListener("click", () => this.onSwitch?.(s.id));
      return row;
    };

    const section = (title: string, count: number): HTMLElement => {
      const det = document.createElement("details");
      det.className = "task-group";
      det.open = true;
      const sum = document.createElement("summary");
      sum.className = "task-group-head";
      sum.textContent = `${title} (${count})`;
      det.appendChild(sum);
      const body = document.createElement("div");
      body.className = "task-group-body";
      det.appendChild(body);
      this.sidebar?.appendChild(det);
      return body;
    };

    const noDir = all.filter((s) => !Sessions.workdirOf(s));
    const withDir = all.filter((s) => Sessions.workdirOf(s));

    // 任务段（无工作目录 / 用默认全局工作空间）。
    if (noDir.length > 0) {
      const body = section(t("sidebar.section.tasks"), noDir.length);
      for (const s of noDir) body.appendChild(makeRow(s));
    }

    // 工作空间段（按工作目录分组，组头=目录叶名）。
    if (withDir.length > 0) {
      const groups = new Map<string, Session[]>();
      for (const s of withDir) {
        const k = Sessions.workdirOf(s);
        const arr = groups.get(k);
        if (arr) arr.push(s);
        else groups.set(k, [s]);
      }
      const body = section(t("sidebar.section.workspace"), groups.size);
      for (const [dir, list] of groups) {
        const folder = document.createElement("div");
        folder.className = "ws-folder";
        const head = document.createElement("div");
        head.className = "ws-folder-head";
        head.title = dir;
        head.innerHTML = `${icon("folder", 14)}<span class="ws-name"></span>`;
        (head.querySelector(".ws-name") as HTMLElement).textContent = leaf(dir);
        // 目录名右侧 +：一键在该目录下新建会话（继承工作目录）。
        const add = document.createElement("button");
        add.className = "ws-new-session";
        add.innerHTML = icon("plus", 12);
        add.title = t("sidebar.newSessionInDir");
        add.addEventListener("click", (e) => {
          e.stopPropagation();
          this.onNewSession?.(dir);
        });
        head.appendChild(add);
        folder.appendChild(head);
        for (const s of list) folder.appendChild(makeRow(s));
        body.appendChild(folder);
      }
    }
  }
  /** 行内重命名：名字换成输入框，Enter/失焦提交（有变化才回调），Esc 取消；重画列表复原。 */
  private beginRename(row: HTMLElement, s: Session): void {
    const nameEl = row.querySelector(".task-name");
    if (!nameEl || row.querySelector(".task-name-input")) return;
    const old = ((s.name as string | undefined) ?? "").trim();
    const input = document.createElement("input");
    input.className = "task-name-input";
    input.value = old;
    input.addEventListener("click", (e) => e.stopPropagation());
    let done = false;
    const finish = (commit: boolean): void => {
      if (done) return; // Enter 提交后紧跟的 blur 不应重复触发
      done = true;
      const next = input.value.trim();
      if (commit && next && next !== old) {
        // 乐观更新：本地立即改名重画，不等服务端 session_renamed 广播。
        this.patch(s.id, { name: next });
        this.onRename?.(s.id, next);
      }
      this.renderList();
    };
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") finish(true);
      else if (e.key === "Escape") finish(false);
    });
    input.addEventListener("blur", () => finish(true));
    nameEl.replaceWith(input);
    input.focus();
    input.select();
  }

  find(id: SessionId): Session | undefined {
    return this.store.get(id);
  }
  /** 当前所有会话 id（排序）。 */
  ids(): SessionId[] {
    return [...this.store.keys()].sort();
  }
  patch(id: SessionId, patch: Record<string, unknown>): void {
    this.store.set(id, { ...(this.store.get(id) ?? { id }), ...patch, id });
  }
  add(session: Session): void {
    if (!this.store.has(session.id)) this.store.set(session.id, session);
  }
  remove(id: SessionId): void {
    this.store.delete(id);
  }
  updateStatusBar(_status: string): void {}
  updateInfoBar(_session: Session | undefined): void {}
  updateChatHeader(_session: Session | undefined): void {}
}
