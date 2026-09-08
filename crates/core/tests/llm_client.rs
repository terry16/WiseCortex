//! 用 mock SSE 服务器验证流式客户端的真实 HTTP 路径（两种 wire format）。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Router;
use tokio::net::TcpListener;
use wisecortex_core::llm::{ChatMessage, LlmClient, LlmRequest, ProviderConfig, WireFormat};

const OPENAI_SSE: &str = "\
data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}\n\n\
data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"}}]}\n\n\
data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1}}\n\n\
data: [DONE]\n\n";

const ANTHROPIC_SSE: &str = "\
event: message_start\n\
data: {\"message\":{\"usage\":{\"input_tokens\":5}}}\n\n\
event: content_block_start\n\
data: {\"index\":0,\"content_block\":{\"type\":\"text\"}}\n\n\
event: content_block_delta\n\
data: {\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n\
event: message_delta\n\
data: {\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n\
event: message_stop\n\
data: {}\n\n";

fn sse(body: &'static str) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/event-stream")], body)
}

async fn start_mock() -> String {
    let app = Router::new()
        .route("/chat/completions", post(|| async { sse(OPENAI_SSE) }))
        .route("/v1/messages", post(|| async { sse(ANTHROPIC_SSE) }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn openai_format_streams_and_parses() {
    let base = start_mock().await;
    let cfg = ProviderConfig::new(base, "test-key", WireFormat::OpenAi);
    let req = LlmRequest::new("gpt-test", vec![ChatMessage::user("hi")]);

    let chunks = Arc::new(AtomicU32::new(0));
    let c = chunks.clone();
    let resp = LlmClient::new()
        .complete(&cfg, &req, move |_u| {
            c.fetch_add(1, Ordering::SeqCst);
        })
        .await
        .unwrap();

    assert_eq!(resp.content.as_deref(), Some("Hello"));
    assert_eq!(resp.finish_reason.as_deref(), Some("stop"));
    assert_eq!(resp.usage.prompt_tokens, 5);
    assert_eq!(resp.usage.completion_tokens, 1);
    assert!(chunks.load(Ordering::SeqCst) > 0, "on_chunk should fire");
}

#[tokio::test]
async fn anthropic_format_streams_and_parses() {
    let base = start_mock().await;
    let cfg = ProviderConfig::new(base, "test-key", WireFormat::Anthropic);
    let req = LlmRequest::new("claude-test", vec![ChatMessage::user("hi")]);

    let resp = LlmClient::new()
        .complete(&cfg, &req, |_u| {})
        .await
        .unwrap();

    assert_eq!(resp.content.as_deref(), Some("Hello"));
    assert_eq!(resp.finish_reason.as_deref(), Some("stop"));
    assert_eq!(resp.usage.completion_tokens, 1);
    assert!(resp.usage.total_is_per_turn);
}

#[tokio::test]
async fn http_error_is_surfaced() {
    // 指向一个不返回成功的路径。
    let app = Router::new().route(
        "/chat/completions",
        post(|| async { (axum::http::StatusCode::UNAUTHORIZED, "bad key") }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let cfg = ProviderConfig::new(format!("http://{addr}"), "x", WireFormat::OpenAi);
    let req = LlmRequest::new("m", vec![ChatMessage::user("hi")]);
    let err = LlmClient::new()
        .complete(&cfg, &req, |_u| {})
        .await
        .unwrap_err();
    match err {
        wisecortex_core::llm::LlmError::Api { status, .. } => assert_eq!(status, 401),
        other => panic!("expected Api error, got {other:?}"),
    }
}

#[tokio::test]
async fn loopback_bypasses_configured_proxy() {
    // 配一个「死代理」（无人监听 127.0.0.1:1）。若 loopback 没被放过，请求会被塞进死代理
    // 而连接失败；放过了就能直连本地 mock。这也对应真实场景：配了代理又接本地模型（Ollama 等）。
    let base = start_mock().await;
    let client = wisecortex_core::net::async_builder_with(Some("http://127.0.0.1:1"))
        .build()
        .unwrap();
    let resp = client.get(&base).send().await;
    assert!(
        resp.is_ok(),
        "loopback 应绕过代理直连 mock，却失败了: {resp:?}"
    );
}
