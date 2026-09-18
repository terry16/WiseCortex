// ── WS — WebSocket 连接管理器 ──────────────────────────────────────────────
//
// 可注入依赖的 TS 工厂（socket 与鉴权均可替换），便于单测。
// 职责：
//   - 指数退避重连
//   - 断线时缓存出站消息，重连后 flush
//   - 跟踪当前订阅的 session，重连后自动恢复
//   - 把入站服务端事件派发给已注册的 handler
//
// 协议契约见 docs/protocols/ws-protocol.md。
// ──────────────────────────────────────────────────────────────────────────

export type WsMessage = { type: string; [key: string]: unknown };
export type WsEventHandler = (event: WsMessage) => void;

/** 鉴权适配器。 */
export interface WsAuth {
  passed: boolean;
  getKey(): string | null;
  reset(): void;
  check(): void;
  /**
   * 可选：探一次后端，判断 1006 到底是「没连上服务」还是「真的要密钥」。
   * 返回 true = 确实需要/缺少密钥；返回 false = 后端不可达或无需密钥。
   * 不提供时退化为旧行为（1006 即认为鉴权问题）。
   */
  probeNeedsKey?(): Promise<boolean>;
}

export interface WsClientOptions {
  /** 构造 ws URL；默认从 location + access key 推导。 */
  url?: () => string;
  auth?: WsAuth;
  /** WebSocket 构造器，默认 `new WebSocket(url)`；测试时注入 mock。 */
  socketFactory?: (url: string) => WebSocket;
  initialDelay?: number;
  maxDelay?: number;
}

export interface WsClient {
  /** 注册服务端事件 handler。 */
  onEvent(fn: WsEventHandler): void;
  /** 发送消息；未连接时入队。 */
  send(obj: WsMessage): void;
  /** 记录当前订阅的 session（重连恢复用）。 */
  setSubscribedSession(id: string | null): void;
  /** 启动连接，开机调用一次。 */
  connect(): void;
  /** socket 是否就绪。 */
  readonly ready: boolean;
}

const NOOP_AUTH: WsAuth = {
  passed: true,
  getKey: () => null,
  reset() {},
  check() {},
};

function defaultUrlBuilder(auth: WsAuth): () => string {
  return () => {
    const key = auth.getKey();
    const protocol = location.protocol === "https:" ? "wss:" : "ws:";
    return key
      ? `${protocol}//${location.host}/ws?access_key=${encodeURIComponent(key)}`
      : `${protocol}//${location.host}/ws`;
  };
}

export function createWsClient(opts: WsClientOptions = {}): WsClient {
  const auth = opts.auth ?? NOOP_AUTH;
  const initialDelay = opts.initialDelay ?? 1000;
  const maxDelay = opts.maxDelay ?? 30_000;
  const socketFactory = opts.socketFactory ?? ((url) => new WebSocket(url));
  const buildUrl = opts.url ?? defaultUrlBuilder(auth);

  let socket: WebSocket | null = null;
  let ready = false;
  let retryDelay = initialDelay;
  let retryTimer: ReturnType<typeof setTimeout> | null = null;
  const queue: WsMessage[] = [];
  const handlers: WsEventHandler[] = [];
  let subscribedId: string | null = null;

  function dispatch(event: WsMessage): void {
    for (const fn of handlers) {
      try {
        fn(event);
      } catch (e) {
        console.error("[WS] handler error", e);
      }
    }
  }

  function rawSend(obj: WsMessage): void {
    socket?.send(JSON.stringify(obj));
  }

  function flushQueue(): void {
    const pending = queue.splice(0);
    for (const msg of pending) {
      try {
        rawSend(msg);
      } catch {
        queue.unshift(msg); // 发送失败重新入队
      }
    }
  }

  function onOpen(): void {
    ready = true;
    retryDelay = initialDelay; // 连接成功重置退避

    // 重连后总是拉一次最新 session 列表
    rawSend({ type: "list_sessions" });

    // 恢复之前的订阅
    if (subscribedId) {
      rawSend({ type: "subscribe", session_id: subscribedId });
    }

    flushQueue();
    dispatch({ type: "_ws_connected" });
  }

  function onMessage(e: MessageEvent): void {
    let event: WsMessage;
    try {
      event = JSON.parse(e.data as string);
    } catch (ex) {
      console.error("[WS] parse error", ex);
      return;
    }
    dispatch(event);
  }

  function onClose(e: CloseEvent): void {
    // 1006 = 连接异常断开。注意：这**不等于** 401——后端根本没监听（端口被占、
    // 进程没起来）同样是 1006。早先的实现一律当鉴权失败，于是一上来就 clearKey +
    // 弹密钥框，把「服务挂了」误报成「要密钥」，用户看到的是配置全空，以为数据丢了。
    // 故先探一次 /api/auth/status（该接口无需鉴权），确认确实缺密钥再弹框。
    if (e.code === 1006 && !auth.passed) {
      if (!auth.probeNeedsKey) {
        auth.reset();
        auth.check();
        return;
      }
      void auth.probeNeedsKey().then((needsKey) => {
        if (needsKey) {
          auth.reset();
          auth.check();
          return;
        }
        // 后端不可达：别动密钥（清了用户下次还得重输），当普通掉线重连。
        scheduleReconnect();
      });
      return;
    }
    scheduleReconnect();
  }

  function scheduleReconnect(): void {
    ready = false;
    socket = null;
    console.warn(`[WS] closed — retry in ${retryDelay}ms`);
    dispatch({ type: "_ws_disconnected" });

    retryTimer = setTimeout(() => {
      retryDelay = Math.min(retryDelay * 2, maxDelay);
      connect();
    }, retryDelay);
  }

  function onError(err: Event): void {
    console.error("[WS] error", err);
    // onclose 会在 onerror 之后自动触发
  }

  function connect(): void {
    if (retryTimer) {
      clearTimeout(retryTimer);
      retryTimer = null;
    }
    if (
      socket &&
      (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING)
    ) {
      return;
    }
    socket = socketFactory(buildUrl());
    socket.onopen = onOpen;
    socket.onmessage = onMessage;
    socket.onclose = onClose;
    socket.onerror = onError;
  }

  return {
    onEvent(fn) {
      handlers.push(fn);
    },
    send(obj) {
      if (ready && socket) {
        try {
          rawSend(obj);
          return;
        } catch {
          // 落到队列
        }
      }
      queue.push(obj);
    },
    setSubscribedSession(id) {
      subscribedId = id;
    },
    connect,
    get ready() {
      return ready;
    },
  };
}
