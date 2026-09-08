//! 回归：Gemini 订阅的推理请求必须带 `GeminiCLI/*` 的 User-Agent。
//!
//! 少了它，`cloudcode-pa` 会按最紧的一档节流（实测连发 6 次只过 2 次，带上则 6/6），
//! 付费订阅的额度根本吃不到，表现就是「过不两句就限流」。见
//! `wisecortex_core::llm::oauth_gemini::user_agent` 的实测表。
//!
//! 本测试**不打真实上游**：起一个本地 HTTP 服务器冒充 Code Assist，把请求头截下来断言。
//! 因此不消耗任何订阅额度，也不需要登录（token 取不到时自动跳过）。

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc;

use wisecortex_core::llm::client::{LlmClient, ProviderConfig};
use wisecortex_core::llm::WireFormat;
use wisecortex_core::llm::{ChatMessage, LlmRequest};

/// 起一个只读一次请求的极简 HTTP 服务器，把收到的请求头行发回来。
fn spawn_capture() -> (String, mpsc::Receiver<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let mut reader = BufReader::new(&stream);
        let mut headers = Vec::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            if line.trim().is_empty() {
                break; // 头读完了，body 不关心
            }
            headers.push(line.trim_end().to_string());
        }
        let _ = tx.send(headers);
        // 回一个最简单的 SSE 结束响应，让客户端别卡着等。
        let mut s = &stream;
        let _ = s.write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n\r\n",
        );
    });
    (format!("http://{addr}"), rx)
}

#[tokio::test]
async fn gemini_inference_sends_gemini_cli_user_agent() {
    // 没登录 Gemini 订阅就跳过——CI 上没有凭据，不该因此挂红。
    if !wisecortex_core::llm::oauth_gemini::is_logged_in() {
        eprintln!("未登录 Gemini 订阅，跳过该回归测试");
        return;
    }
    let (base, rx) = spawn_capture();
    let cfg = ProviderConfig::new(base, "", WireFormat::Gemini).with_gemini_oauth(true);
    let req = LlmRequest::new("gemini-2.5-pro".to_string(), vec![ChatMessage::user("hi")]);

    // 假服务器不会给出合法响应，complete 必然报错——我们只关心它发出去的请求头。
    let client = LlmClient::new();
    let _ = client.complete(&cfg, &req, |_| {}).await;

    let headers = rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("没收到请求");
    let ua = headers
        .iter()
        .find(|h| h.to_ascii_lowercase().starts_with("user-agent:"))
        .unwrap_or_else(|| panic!("请求里没有 User-Agent 头！收到的头: {headers:#?}"));

    assert!(
        ua.contains("GeminiCLI/"),
        "User-Agent 必须以 GeminiCLI/ 开头（Google 只对这个前缀放行），实际: {ua}"
    );
    assert!(
        ua.contains("gemini-2.5-pro"),
        "UA 里应带上本次使用的模型（对齐 gemini-cli 格式），实际: {ua}"
    );
}
