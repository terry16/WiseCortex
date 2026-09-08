import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { type WsAuth, createWsClient } from "./ws";

// Minimal controllable WebSocket double. jsdom provides WebSocket constants
// (OPEN/CONNECTING) which ws.ts reads, but not a connectable server, so we
// inject this via socketFactory.
class MockWebSocket {
  static instances: MockWebSocket[] = [];
  readyState = 0; // CONNECTING
  url: string;
  sent: string[] = [];
  onopen: ((e: unknown) => void) | null = null;
  onmessage: ((e: { data: string }) => void) | null = null;
  onclose: ((e: { code: number }) => void) | null = null;
  onerror: ((e: unknown) => void) | null = null;

  constructor(url: string) {
    this.url = url;
    MockWebSocket.instances.push(this);
  }
  send(data: string) {
    if (this.readyState !== 1) throw new Error("not open");
    this.sent.push(data);
  }
  fireOpen() {
    this.readyState = 1; // OPEN
    this.onopen?.({});
  }
  fireMessage(obj: unknown) {
    this.onmessage?.({ data: JSON.stringify(obj) });
  }
  fireClose(code = 1006) {
    this.readyState = 3; // CLOSED
    this.onclose?.({ code });
  }
  static reset() {
    MockWebSocket.instances = [];
  }
  static get last() {
    return MockWebSocket.instances[MockWebSocket.instances.length - 1];
  }
}

const factory = (url: string) => new MockWebSocket(url) as unknown as WebSocket;
const parse = (s: string) => JSON.parse(s) as { type: string; [k: string]: unknown };

beforeEach(() => MockWebSocket.reset());
afterEach(() => vi.useRealTimers());

describe("ws transport", () => {
  it("queues sends while disconnected and flushes on open", () => {
    const ws = createWsClient({ socketFactory: factory, url: () => "ws://x/ws" });
    ws.connect();
    ws.send({ type: "message", content: "hi" }); // queued (not open yet)

    const sock = MockWebSocket.last;
    expect(sock.sent).toHaveLength(0);

    sock.fireOpen();
    const types = sock.sent.map((s) => parse(s).type);
    // onOpen auto-sends list_sessions, then flushes the queued message
    expect(types).toContain("list_sessions");
    expect(types).toContain("message");
  });

  it("re-subscribes the tracked session on (re)connect", () => {
    const ws = createWsClient({ socketFactory: factory, url: () => "ws://x/ws" });
    ws.setSubscribedSession("s1");
    ws.connect();
    MockWebSocket.last.fireOpen();

    const subscribe = MockWebSocket.last.sent.map(parse).find((m) => m.type === "subscribe");
    expect(subscribe).toMatchObject({ type: "subscribe", session_id: "s1" });
  });

  it("reconnects with exponential backoff (1s -> 2s)", () => {
    vi.useFakeTimers();
    const ws = createWsClient({ socketFactory: factory, url: () => "ws://x/ws" });
    ws.connect();
    expect(MockWebSocket.instances).toHaveLength(1);

    MockWebSocket.last.fireOpen();
    MockWebSocket.last.fireClose(); // schedule reconnect in 1000ms
    vi.advanceTimersByTime(1000);
    expect(MockWebSocket.instances).toHaveLength(2); // reconnected

    MockWebSocket.last.fireClose(); // next backoff = 2000ms
    vi.advanceTimersByTime(1000);
    expect(MockWebSocket.instances).toHaveLength(2); // not yet
    vi.advanceTimersByTime(1000);
    expect(MockWebSocket.instances).toHaveLength(3); // reconnected at 2000ms total
  });

  it("dispatches _ws_connected / _ws_disconnected lifecycle events", () => {
    const events: string[] = [];
    const ws = createWsClient({ socketFactory: factory, url: () => "ws://x/ws" });
    ws.onEvent((e) => events.push(e.type));
    ws.connect();
    MockWebSocket.last.fireOpen();
    MockWebSocket.last.fireClose();
    expect(events).toContain("_ws_connected");
    expect(events).toContain("_ws_disconnected");
  });

  it("on 1006 close with failed auth, resets+rechecks instead of reconnecting", () => {
    vi.useFakeTimers();
    const auth: WsAuth = { passed: false, getKey: () => null, reset: vi.fn(), check: vi.fn() };
    const ws = createWsClient({ socketFactory: factory, url: () => "ws://x/ws", auth });
    ws.connect();
    MockWebSocket.last.fireOpen();
    MockWebSocket.last.fireClose(1006);
    expect(auth.reset).toHaveBeenCalled();
    expect(auth.check).toHaveBeenCalled();
    vi.advanceTimersByTime(60_000);
    expect(MockWebSocket.instances).toHaveLength(1); // no reconnect attempted
  });
});
