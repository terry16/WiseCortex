import { describe, expect, it, vi } from "vitest";
import type { WsMessage } from "./ws";
import { type DispatcherDeps, createDispatcher } from "./ws-dispatcher";

function makeDeps(activeId: string | null = "s1") {
  const sessions = {
    activeId,
    setAll: vi.fn(),
    renderList: vi.fn(),
    find: vi.fn((id: string) => (id === activeId ? { id } : undefined)),
    clearAllProgress: vi.fn(),
    takePendingRunTask: vi.fn((): string | null => null),
    takePendingMessage: vi.fn((): { session_id: string; content: string } | null => null),
    appendMsg: vi.fn(),
    appendDelta: vi.fn(),
    appendThinking: vi.fn(),
    patch: vi.fn(),
    updateStatusBar: vi.fn(),
    updateInfoBar: vi.fn(),
    updateChatHeader: vi.fn(),
    remove: vi.fn(),
    add: vi.fn(),
    clearProgress: vi.fn(),
    appendToolCall: vi.fn(),
    appendToolResult: vi.fn(),
    appendToolStdout: vi.fn(),
    appendTokenUsage: vi.fn(),
    showProgress: vi.fn(),
    collapseToolGroup: vi.fn(),
    appendInfo: vi.fn(),
    showFeedbackRequest: vi.fn(),
  };
  const deps: DispatcherDeps = {
    ws: { send: vi.fn(), onEvent: vi.fn() },
    sessions,
    tasks: { load: vi.fn() },
    skills: { load: vi.fn() },
    router: { current: "session", restoreFromHash: vi.fn(), navigate: vi.fn() },
    i18n: { t: (key: string) => key },
    ui: { setOffline: vi.fn(), enableSend: vi.fn(), focusInput: vi.fn() },
    escapeHtml: (s: string) => s,
    showConfirmModal: vi.fn(),
    billing: { getCurrencySymbol: () => "$", convertCost: (n: number) => n },
  };
  return { deps, sessions };
}

const send = (h: (e: WsMessage) => void, e: WsMessage) => h(e);

describe("ws-dispatcher", () => {
  it("session_update shape① (full session object) patches by session.id", () => {
    const { deps, sessions } = makeDeps("s1");
    const h = createDispatcher(deps);
    send(h, { type: "session_update", session: { id: "s1", status: "idle", total_cost: 5 } });
    expect(sessions.patch).toHaveBeenCalledWith("s1", { id: "s1", status: "idle", total_cost: 5 });
  });

  it("session_update shape② (partial fields) maps cost/tasks/status/latency", () => {
    const { deps, sessions } = makeDeps("s1");
    const h = createDispatcher(deps);
    send(h, {
      type: "session_update",
      session_id: "s1",
      cost: 3,
      tasks: 2,
      status: "working",
      latency: 120,
    });
    expect(sessions.patch).toHaveBeenCalledWith("s1", {
      total_cost: 3,
      total_tasks: 2,
      status: "working",
      latest_latency: 120,
    });
  });

  it("assistant_message is ignored when not the active session", () => {
    const { deps, sessions } = makeDeps("s1");
    const h = createDispatcher(deps);
    send(h, { type: "assistant_message", session_id: "other", content: "hi" });
    expect(sessions.appendMsg).not.toHaveBeenCalled();
  });

  it("assistant_thinking renders a thinking block for the active session", () => {
    const { deps, sessions } = makeDeps("s1");
    const h = createDispatcher(deps);
    send(h, { type: "assistant_thinking", session_id: "s1", content: "让我想想…" });
    expect(sessions.appendThinking).toHaveBeenCalledWith("让我想想…");
  });

  it("assistant_message renders for the active session", () => {
    const { deps, sessions } = makeDeps("s1");
    const h = createDispatcher(deps);
    send(h, { type: "assistant_message", session_id: "s1", content: "hello" });
    expect(sessions.appendMsg).toHaveBeenCalledWith("assistant", "hello");
  });

  it("error renders a retry button carrying the session id", () => {
    const { deps, sessions } = makeDeps("s1");
    const h = createDispatcher(deps);
    send(h, { type: "error", session_id: "s1", message: "LLM 调用失败: boom" });
    expect(sessions.appendMsg).toHaveBeenCalledTimes(1);
    const [role, html] = sessions.appendMsg.mock.calls[0];
    expect(role).toBe("error");
    expect(html).toContain("retry-btn");
    expect(html).toContain('data-retry-session="s1"');
    expect(html).toContain("LLM 调用失败: boom");
  });

  it("tool_call appends a tool item for the active session", () => {
    const { deps, sessions } = makeDeps("s1");
    const h = createDispatcher(deps);
    send(h, {
      type: "tool_call",
      session_id: "s1",
      name: "write",
      args: { path: "a" },
      summary: "write a",
    });
    expect(sessions.appendToolCall).toHaveBeenCalledWith("write", { path: "a" }, "write a");
  });

  it("complete computes a cache line when cache_stats present", () => {
    const { deps, sessions } = makeDeps("s1");
    const h = createDispatcher(deps);
    send(h, {
      type: "complete",
      session_id: "s1",
      iterations: 3,
      cost: 0.01,
      cost_source: "actual",
      cache_stats: { total_requests: 10, cache_hit_requests: 9, cache_read_input_tokens: 2000 },
    });
    // appendInfo(mainLine, cacheLine) — second arg is the (i18n) cache line key, not null
    expect(sessions.appendInfo).toHaveBeenCalledTimes(1);
    const [, cacheLine] = sessions.appendInfo.mock.calls[0];
    expect(cacheLine).toBe("chat.done.cache");
  });

  it("error insufficient_credit renders a top-up link", () => {
    const { deps, sessions } = makeDeps("s1");
    const h = createDispatcher(deps);
    send(h, {
      type: "error",
      session_id: "s1",
      code: "insufficient_credit",
      top_up_url: "https://pay",
    });
    expect(sessions.appendMsg).toHaveBeenCalledTimes(1);
    const [role, html] = sessions.appendMsg.mock.calls[0];
    expect(role).toBe("error");
    expect(String(html)).toContain("https://pay");
  });

  it("subscribed enables send and fires queued run_task", () => {
    const { deps, sessions } = makeDeps("s1");
    sessions.takePendingRunTask.mockReturnValue("s1");
    const h = createDispatcher(deps);
    send(h, { type: "subscribed", session_id: "s1" });
    expect(deps.ui.enableSend).toHaveBeenCalled();
    expect(deps.ws.send).toHaveBeenCalledWith({ type: "run_task", session_id: "s1" });
  });
});
