//! axum WebSocket 端点：升级连接、按协议处理客户端消息、向订阅者广播。
//!
//! Phase 1 阶段 agent 为「回声」占位（message → assistant_message "echo: ..."），
//! Phase 2 会替换为真实 agent。协议见 docs/protocols/ws-protocol.md。

use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;

use crate::agent::Agent;
use crate::proto::{ClientMsg, ServerEvent};
use crate::registry::{QueuedMessage, SessionRegistry};

/// 路由共享状态。`agent` 用 Mutex 包裹以支持 /api/config 热重载。
#[derive(Clone)]
pub struct AppState {
    pub registry: SessionRegistry,
    pub agent: Arc<Mutex<Arc<Agent>>>,
    /// 访问密钥；None=公开模式。在启动时解析（改 key 需重启）。
    pub access_key: Option<String>,
}

impl AppState {
    pub fn new(registry: SessionRegistry, agent: Arc<Agent>) -> Self {
        AppState {
            registry,
            agent: Arc::new(Mutex::new(agent)),
            access_key: None,
        }
    }

    pub fn with_access_key(mut self, key: Option<String>) -> Self {
        self.access_key = key.filter(|k| !k.is_empty());
        self
    }

    /// 校验给定密钥是否放行（公开模式恒为 true）。
    pub fn auth_ok(&self, presented: Option<&str>) -> bool {
        match &self.access_key {
            None => true,
            Some(k) => presented == Some(k.as_str()),
        }
    }
}

/// 构建 Router：WS + REST（/api/*），CORS + 访问密钥校验。
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .merge(crate::rest::routes())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::rest::require_access_key,
        ))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// 入队一条消息并按需启动「抽干循环」：空闲则起一个任务按 FIFO 逐条跑（retry=对现有历史重跑，
/// 否则正常 run_turn）直到队空；工作中则只排队并广播 message_queued 供前端反馈。整个抽干共用
/// 一个 abort 句柄，「停止」精确命中当前回合。
fn enqueue_and_drain(
    agent: &Arc<Mutex<Arc<Agent>>>,
    reg: &SessionRegistry,
    sid: &str,
    msg: QueuedMessage,
) {
    if reg.enqueue_message(sid, msg) {
        let agent = agent.lock().unwrap().clone(); // 取当前（可能已热替换的）agent
        let reg_turn = reg.clone();
        let sid_task = sid.to_string();
        let task = tokio::spawn(async move {
            while let Some(m) = reg_turn.dequeue_message(&sid_task) {
                if m.retry {
                    agent.run_retry(&reg_turn, &sid_task, m.cwd).await;
                } else {
                    agent
                        .run_turn(
                            &reg_turn, &sid_task, m.content, m.images, m.files, m.cwd, None,
                        )
                        .await;
                }
            }
            reg_turn.clear_running(&sid_task);
        });
        reg.set_running(sid, task.abort_handle());
    } else {
        reg.publish(
            sid,
            ServerEvent::MessageQueued {
                session_id: sid.to_string(),
            },
        );
    }
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let reg = state.registry.clone();
    let agent = state.agent.clone();
    let (mut sink, mut stream) = socket.split();

    // 每个连接单一出站路径：直接回复与广播转发都汇入此 mpsc，由 writer 串行写出。
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<ServerEvent>();
    let writer = tokio::spawn(async move {
        while let Some(ev) = out_rx.recv().await {
            let Ok(text) = serde_json::to_string(&ev) else {
                continue;
            };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    // 全局侧栏 feed：所有连接都订阅，承载轻量生命周期事件（状态/重命名/删除），
    // 让后台任务（未订阅的对话）的运行态也能在侧栏实时更新。
    let global_task = {
        let mut grx = reg.subscribe_global();
        let out = out_tx.clone();
        tokio::spawn(async move {
            while let Ok(ev) = grx.recv().await {
                if out.send(ev).is_err() {
                    break;
                }
            }
        })
    };

    // 连上即推一次当日成本初值（服务端账本），之后变动由全局 feed 的 cost_update 增量刷新。
    let _ = out_tx.send(ServerEvent::CostUpdate {
        cost_today: wisecortex_core::cost::today_total(),
    });

    let mut sub_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut current_session: Option<String> = None;

    while let Some(Ok(msg)) = stream.next().await {
        let text = match msg {
            Message::Text(t) => t.as_str().to_owned(),
            Message::Close(_) => break,
            _ => continue,
        };

        match serde_json::from_str::<ClientMsg>(&text) {
            Ok(ClientMsg::ListSessions) => {
                let _ = out_tx.send(ServerEvent::SessionList {
                    sessions: reg.list(),
                    has_more: false,
                    cron_count: 0,
                });
            }

            Ok(ClientMsg::Subscribe { session_id }) => {
                let tx = reg.ensure(&session_id);
                current_session = Some(session_id.clone());

                // 重新订阅前停掉旧的转发任务。
                if let Some(handle) = sub_task.take() {
                    handle.abort();
                }
                let mut rx = tx.subscribe();
                let out = out_tx.clone();
                sub_task = Some(tokio::spawn(async move {
                    while let Ok(ev) = rx.recv().await {
                        if out.send(ev).is_err() {
                            break;
                        }
                    }
                }));

                let _ = out_tx.send(ServerEvent::Subscribed {
                    session_id: session_id.clone(),
                });
                if let Some(snap) = reg.snapshot(&session_id) {
                    let _ = out_tx.send(ServerEvent::snapshot_update(snap));
                }
            }

            Ok(ClientMsg::Message {
                session_id,
                content,
                images,
                files,
                cwd,
            }) => {
                if let Some(sid) = session_id.or_else(|| current_session.clone()) {
                    // 确保 session 存在（允许未先 subscribe 直接发 message）。
                    reg.ensure(&sid);
                    let msg = QueuedMessage {
                        content,
                        images,
                        files,
                        cwd,
                        retry: false,
                    };
                    enqueue_and_drain(&agent, &reg, &sid, msg);
                }
            }

            Ok(ClientMsg::Ping) => {
                let _ = out_tx.send(ServerEvent::Pong);
            }

            Ok(ClientMsg::Confirmation { id, result, .. }) => {
                reg.resolve_confirmation(&id, result);
            }

            Ok(ClientMsg::Interrupt { session_id }) => {
                if let Some(sid) = session_id.or_else(|| current_session.clone()) {
                    if reg.interrupt(&sid) {
                        // set_status 会把 idle 发到全局侧栏 feed（含当前对话）；这里只补发 interrupted。
                        reg.set_status(&sid, "idle");
                        reg.publish(
                            &sid,
                            ServerEvent::Interrupted {
                                session_id: sid.clone(),
                            },
                        );
                    }
                }
            }

            Ok(ClientMsg::Retry { session_id }) => {
                if let Some(sid) = session_id.or_else(|| current_session.clone()) {
                    reg.ensure(&sid);
                    let msg = QueuedMessage {
                        content: String::new(),
                        images: vec![],
                        files: vec![],
                        cwd: None,
                        retry: true,
                    };
                    enqueue_and_drain(&agent, &reg, &sid, msg);
                }
            }

            // 占位：run_task 暂不处理。
            Ok(ClientMsg::RunTask { .. }) => {}

            Err(e) => {
                let _ = out_tx.send(ServerEvent::Error {
                    session_id: None,
                    message: format!("Invalid JSON: {e}"),
                    code: None,
                    top_up_url: None,
                });
            }
        }
    }

    if let Some(handle) = sub_task.take() {
        handle.abort();
    }
    global_task.abort();
    writer.abort();
}
