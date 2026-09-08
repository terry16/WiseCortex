// ── WS 事件路由 ────────────────────────────────────────────────────────────
//
// 消费 ws.ts 派发的服务端事件，分发到各业务模块（Sessions/Tasks/Skills/...）。
//
// 设计取舍（便于测试与解耦）：
//   - 依赖通过 DispatcherDeps 注入，而非全局变量。
//   - DOM 操作（offline-banner / btn-send / user-input）收敛到 `ui` 适配器，不散落各处。
//
// 这些接口同时是未来 Sessions/Router/Tasks 等模块要实现的契约。
// 协议契约见 docs/protocols/ws-protocol.md。
// ──────────────────────────────────────────────────────────────────────────

import type { WsMessage } from "./ws";

export type SessionId = string;
export type Session = { id: SessionId; [key: string]: unknown };
export type SessionPatch = Record<string, unknown>;

export interface SessionsModule {
  /** 当前激活的 session id（仅激活 session 的对话事件才渲染）。 */
  readonly activeId: SessionId | null;
  setAll(sessions: Session[], hasMore: boolean, cronCount: number): void;
  renderList(): void;
  find(id: SessionId): Session | undefined;
  clearAllProgress(): void;
  takePendingRunTask(): SessionId | null;
  takePendingMessage(): { session_id: SessionId; content: string } | null;
  appendMsg(role: string, html: string, opts?: { time?: Date; forceScroll?: boolean }): void;
  /** 助手文本流式增量（逐字追加到当前流式气泡）。 */
  appendDelta(text: string): void;
  /** 模型思考文本：留一条可折叠块在转录里（默认收起，显示首句预览）。 */
  appendThinking(content: string): void;
  patch(id: SessionId, patch: SessionPatch): void;
  updateStatusBar(status: string): void;
  updateInfoBar(session: Session | undefined): void;
  updateChatHeader(session: Session | undefined): void;
  remove(id: SessionId): void;
  add(session: Session): void;
  clearProgress(message?: string): void;
  appendToolCall(name: string, args: unknown, summary?: string): void;
  appendToolResult(result: unknown): void;
  appendToolStdout(lines: string[]): void;
  appendTokenUsage(ev: WsMessage): void;
  showProgress(
    message: string | undefined,
    progressType: string,
    metadata: Record<string, unknown>,
    startedAt: number | null,
  ): void;
  collapseToolGroup(): void;
  appendInfo(main: string, sub?: string | null): void;
  showFeedbackRequest(question: unknown, context: unknown, options: unknown): void;
}

export interface RouterModule {
  readonly current: string;
  restoreFromHash(): void;
  navigate(view: string): void;
}

export interface I18nModule {
  t(key: string, params?: Record<string, unknown>): string;
}

export interface BillingModule {
  getCurrencySymbol(): string;
  convertCost(cost: number): number;
}

/** 收敛原先散落在 dispatcher 里的 DOM 操作。 */
export interface UiAdapter {
  setOffline(offline: boolean): void;
  enableSend(): void;
  focusInput(): void;
}

export interface DispatcherDeps {
  ws: {
    send(obj: WsMessage): void;
    onEvent(fn: (e: WsMessage) => void): void;
  };
  sessions: SessionsModule;
  tasks: { load(): void };
  skills: { load(): void };
  router: RouterModule;
  i18n: I18nModule;
  ui: UiAdapter;
  escapeHtml(s: string): string;
  showConfirmModal(id: string, message: string): void;
  /** 可选：未配置时成本显示退化为 "$"。 */
  billing?: BillingModule;
  /** 可选：把重试类 warning 转友好文案；返回 null 表示抑制。默认原样返回。 */
  transformRetryWarning?(message: string): string | null;
}

/**
 * 创建事件路由器：注册到 ws 并返回 handler（便于直接单测）。
 */
export function createDispatcher(deps: DispatcherDeps): (ev: WsMessage) => void {
  const { sessions, router, i18n, ui, escapeHtml } = deps;
  let initialRestoreDone = false;

  const num = (v: unknown): number | undefined => (typeof v === "number" ? v : undefined);
  const str = (v: unknown): string => (v == null ? "" : String(v));
  const isActive = (ev: WsMessage): boolean => ev.session_id === sessions.activeId;

  function renderError(ev: WsMessage): void {
    if (ev.code === "insufficient_credit") {
      const body = escapeHtml(i18n.t("error.insufficient_credit"));
      const action = ev.top_up_url
        ? ` <a href="${escapeHtml(str(ev.top_up_url))}" target="_blank" rel="noopener noreferrer">${escapeHtml(i18n.t("error.insufficient_credit.action"))} →</a>`
        : "";
      sessions.appendMsg("error", `<span>${body}${action}</span>`);
      return;
    }
    // 一般失败（LLM 调用失败等）：附一个「重试」按钮，对现有历史重跑本轮（不重复发消息）。
    const sid = str(ev.session_id) || str(sessions.activeId ?? "");
    const retryBtn = sid
      ? ` <button type="button" class="retry-btn" data-retry-session="${escapeHtml(sid)}">${escapeHtml(i18n.t("chat.retry"))}</button>`
      : "";
    sessions.appendMsg("error", `<span>${escapeHtml(str(ev.message))}</span>${retryBtn}`);
  }

  const handler = (ev: WsMessage): void => {
    switch (ev.type) {
      // ── 内部 WS 生命周期 ───────────────────────────────────────────────
      case "_ws_connected":
        ui.setOffline(false);
        break;

      case "_ws_disconnected":
        ui.setOffline(true);
        sessions.clearAllProgress();
        break;

      // ── Session 列表 ──────────────────────────────────────────────────
      case "session_list": {
        sessions.setAll(
          (ev.sessions as Session[]) || [],
          !!ev.has_more,
          (ev.cron_count as number) || 0,
        );
        sessions.renderList();
        if (!initialRestoreDone) {
          initialRestoreDone = true;
          if (router.current !== "session") router.restoreFromHash();
        } else if (sessions.activeId && !sessions.find(sessions.activeId)) {
          router.navigate("welcome");
        }
        break;
      }

      // ── Session 生命周期 ──────────────────────────────────────────────
      case "subscribed": {
        ui.enableSend();
        ui.focusInput();
        const pendingId = sessions.takePendingRunTask();
        if (pendingId && pendingId === ev.session_id) {
          deps.ws.send({ type: "run_task", session_id: pendingId });
        }
        const pendingMsg = sessions.takePendingMessage();
        if (pendingMsg && pendingMsg.session_id === ev.session_id) {
          // 与 app.ts 的正常发送同理：用户自己发的消息必须立刻可见，不受"是否已贴底"约束。
          // 这条走的是"会话还没订阅上就先发了"的补发路径（如新建任务后马上发送）。
          sessions.appendMsg("user", escapeHtml(pendingMsg.content), {
            time: new Date(),
            forceScroll: true,
          });
          deps.ws.send({
            type: "message",
            session_id: pendingMsg.session_id,
            content: pendingMsg.content,
          });
        }
        break;
      }

      case "session_update": {
        let sid: SessionId | undefined;
        let patch: SessionPatch;
        if (ev.session) {
          const sess = ev.session as Session;
          sid = sess.id;
          patch = sess;
        } else {
          sid = ev.session_id as SessionId | undefined;
          patch = {};
          if (num(ev.cost) !== undefined) patch.total_cost = ev.cost;
          if (num(ev.tasks) !== undefined) patch.total_tasks = ev.tasks;
          if (ev.status !== undefined) patch.status = ev.status;
          if (num(ev.latency) !== undefined) patch.latest_latency = ev.latency;
        }
        if (!sid) break;
        sessions.patch(sid, patch);
        sessions.renderList();
        if (sid === sessions.activeId) {
          const current = sessions.find(sid);
          if (patch.status !== undefined) sessions.updateStatusBar(str(patch.status));
          sessions.updateInfoBar(current);
          sessions.updateChatHeader(current);
        }
        if (patch.status === "idle") {
          deps.tasks.load();
          deps.skills.load();
          sessions.clearProgress(sid);
        }
        break;
      }

      case "session_renamed":
        sessions.patch(str(ev.session_id), { name: ev.name });
        sessions.renderList();
        break;

      case "session_deleted":
        sessions.remove(str(ev.session_id));
        if (ev.session_id === sessions.activeId) router.navigate("welcome");
        sessions.renderList();
        break;

      case "session_restored":
        if (ev.session) {
          sessions.add(ev.session as Session);
          sessions.renderList();
        }
        break;

      // ── 对话消息 ──────────────────────────────────────────────────────
      case "history_user_message":
        // 仅历史回放时出现；由 Sessions._fetchHistory 渲染，此处无操作。
        break;

      case "assistant_delta":
        if (!isActive(ev)) break;
        sessions.clearProgress();
        sessions.appendDelta(str(ev.delta));
        break;

      case "assistant_thinking":
        if (!isActive(ev)) break;
        sessions.clearProgress();
        sessions.appendThinking(str(ev.content));
        break;

      case "assistant_message":
        if (!isActive(ev)) break;
        sessions.clearProgress();
        sessions.appendMsg("assistant", str(ev.content));
        break;

      case "tool_call":
        if (!isActive(ev)) break;
        sessions.clearProgress();
        sessions.appendToolCall(str(ev.name), ev.args, ev.summary ? str(ev.summary) : undefined);
        break;

      case "tool_result":
        if (!isActive(ev)) break;
        sessions.appendToolResult(ev.result);
        break;

      case "tool_stdout":
        if (!isActive(ev)) break;
        sessions.appendToolStdout((ev.lines as string[]) || []);
        break;

      case "tool_error":
        if (!isActive(ev)) break;
        sessions.appendMsg("info", `⚠ Tool error: ${escapeHtml(str(ev.error))}`);
        break;

      case "token_usage":
        if (!isActive(ev)) break;
        sessions.appendTokenUsage(ev);
        break;

      case "progress": {
        if (!isActive(ev)) break;
        if (ev.phase === "active" || ev.status === "start") {
          const progressType = str(ev.progress_type) || "thinking";
          const metadata = (ev.metadata as Record<string, unknown>) || {};
          sessions.showProgress(
            ev.message as string | undefined,
            progressType,
            metadata,
            (ev.started_at as number) || null,
          );
        } else {
          sessions.clearProgress(ev.message as string | undefined);
        }
        break;
      }

      case "complete": {
        if (!isActive(ev)) break;
        sessions.clearProgress();
        sessions.collapseToolGroup();
        const costSource = ev.cost_source as string | undefined;
        const symbol = deps.billing ? deps.billing.getCurrencySymbol() : "$";
        const rawCost = (ev.cost as number) || 0;
        const cost = deps.billing ? deps.billing.convertCost(rawCost) : rawCost;
        const costDisplay =
          !costSource || costSource === "estimated" ? "N/A" : `${symbol}${cost.toFixed(4)}`;
        let mainLine = i18n.t("chat.done", { n: ev.iterations, cost: costDisplay });
        if (typeof ev.duration === "number" && ev.duration > 0) {
          mainLine += i18n.t("chat.done.duration", { duration: ev.duration.toFixed(1) });
        }
        let cacheLine: string | null = null;
        const cs = ev.cache_stats as Record<string, number> | undefined;
        const total = cs?.total_requests;
        const hits = cs?.cache_hit_requests;
        const cachedTokens = cs?.cache_read_input_tokens;
        if (total && total > 0 && cachedTokens && cachedTokens > 0) {
          const rate = (((hits || 0) / total) * 100).toFixed(1);
          const tokensFmt =
            cachedTokens >= 1000 ? `${(cachedTokens / 1000).toFixed(1)}k` : `${cachedTokens}`;
          cacheLine = i18n.t("chat.done.cache", { rate, hits, total, tokens: tokensFmt });
        }
        sessions.appendInfo(`✓ ${mainLine}`, cacheLine);
        break;
      }

      case "request_feedback":
        if (!isActive(ev)) break;
        sessions.showFeedbackRequest(ev.question, ev.context, ev.options);
        break;

      case "request_confirmation":
        if (!isActive(ev)) break;
        deps.showConfirmModal(str(ev.id), str(ev.message));
        break;

      case "interrupted":
        if (!isActive(ev)) break;
        sessions.clearProgress();
        sessions.collapseToolGroup();
        sessions.appendInfo(i18n.t("chat.interrupted"));
        break;

      // 工作中发来的消息已排队：给一条可见信息（当前回合结束后会自动处理）。
      case "message_queued":
        if (!isActive(ev)) break;
        sessions.appendInfo(i18n.t("chat.queued"));
        break;

      // ── 信息 / 错误 ───────────────────────────────────────────────────
      case "info":
        sessions.appendInfo(str(ev.message));
        break;

      case "warning": {
        const transform = deps.transformRetryWarning ?? ((m: string) => m);
        const friendly = transform(str(ev.message));
        if (friendly) sessions.appendInfo(friendly);
        break;
      }

      case "success":
        sessions.appendMsg("success", `✓ ${escapeHtml(str(ev.message))}`);
        break;

      case "error":
        if (!ev.session_id || ev.session_id === sessions.activeId) renderError(ev);
        break;
    }
  };

  deps.ws.onEvent(handler);
  return handler;
}
