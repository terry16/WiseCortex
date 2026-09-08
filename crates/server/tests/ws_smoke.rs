//! 端到端冒烟测试：用真实 WebSocket 客户端跑通 Phase 1 协议骨架。

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use wisecortex_server::{app, Agent, AppState, SessionRegistry};

/// 启动 server 到随机端口，返回 ws URL。用 Echo agent 保持确定性。
async fn start_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = AppState::new(SessionRegistry::new(), Arc::new(Agent::Echo));
    tokio::spawn(async move {
        axum::serve(listener, app(state)).await.unwrap();
    });
    format!("ws://{addr}/ws")
}

/// 读取下一帧文本并解析为 JSON（带超时，避免挂死）。
/// 跳过连上即推、且随时可能广播的 `cost_update`（今日成本）——它是环境噪声，
/// 不属于任何请求的响应序列，断言具体响应时应略过。
async fn next_json<S>(ws: &mut S) -> Value
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timed out waiting for message")
            .expect("stream ended")
            .expect("ws error");
        let v: Value = serde_json::from_str(msg.to_text().unwrap()).unwrap();
        if v["type"] == "cost_update" {
            continue;
        }
        return v;
    }
}

async fn send<S>(ws: &mut S, payload: Value)
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    ws.send(Message::Text(payload.to_string())).await.unwrap();
}

#[tokio::test]
async fn list_sessions_returns_session_list() {
    let url = start_server().await;
    let (mut ws, _) = connect_async(&url).await.unwrap();

    send(&mut ws, serde_json::json!({ "type": "list_sessions" })).await;
    let ev = next_json(&mut ws).await;
    assert_eq!(ev["type"], "session_list");
    assert_eq!(ev["has_more"], false);
}

#[tokio::test]
async fn subscribe_then_message_echoes_full_turn() {
    let url = start_server().await;
    let (mut ws, _) = connect_async(&url).await.unwrap();

    // subscribe → subscribed + session_update(snapshot)
    send(
        &mut ws,
        serde_json::json!({ "type": "subscribe", "session_id": "s1" }),
    )
    .await;
    let subscribed = next_json(&mut ws).await;
    assert_eq!(subscribed["type"], "subscribed");
    assert_eq!(subscribed["session_id"], "s1");

    let snapshot = next_json(&mut ws).await;
    assert_eq!(snapshot["type"], "session_update");
    assert_eq!(snapshot["session"]["id"], "s1");

    // message → working / assistant_message / complete / idle
    send(
        &mut ws,
        serde_json::json!({ "type": "message", "content": "hi" }),
    )
    .await;

    // 一轮里夹杂全局侧栏 feed 的轻量事件（session_renamed / session_update 状态），
    // 故读到 complete 为止（设上限防卡死），再断言关键事件齐全。
    let mut types = Vec::new();
    let mut echoed = None;
    for _ in 0..10 {
        let ev = next_json(&mut ws).await;
        let kind = ev["type"].as_str().unwrap().to_string();
        if ev["type"] == "assistant_message" {
            echoed = Some(ev["content"].as_str().unwrap().to_string());
        }
        let done = kind == "complete";
        types.push(kind);
        if done {
            break;
        }
    }

    assert_eq!(echoed.as_deref(), Some("echo: hi"));
    assert!(types.contains(&"assistant_message".to_string()));
    assert!(types.contains(&"complete".to_string()));
    // 至少有一次 working / idle 的 session_update（经全局侧栏 feed 送达）
    assert!(types.iter().any(|t| t == "session_update"));
}

#[tokio::test]
async fn ping_returns_pong() {
    let url = start_server().await;
    let (mut ws, _) = connect_async(&url).await.unwrap();
    send(&mut ws, serde_json::json!({ "type": "ping" })).await;
    assert_eq!(next_json(&mut ws).await["type"], "pong");
}

#[tokio::test]
async fn invalid_json_returns_error() {
    let url = start_server().await;
    let (mut ws, _) = connect_async(&url).await.unwrap();
    ws.send(Message::Text("not json".into())).await.unwrap();
    let ev = next_json(&mut ws).await;
    assert_eq!(ev["type"], "error");
    assert!(ev["message"].as_str().unwrap().contains("Invalid JSON"));
}
