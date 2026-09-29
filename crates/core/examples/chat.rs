//! 用真实 API key 测试 LLM 客户端的小工具。
//!
//! 用法（PowerShell）：
//!   $env:WC_PROVIDER="deepseek"; $env:WC_API_KEY="sk-..."; `
//!   cargo run -p wisecortex-core --example chat -- "用一句话介绍你自己"
//!
//! 可选环境变量：
//!   WC_PROVIDER  内置预设 id：openai | anthropic | deepseek | qwen | gemini（默认 deepseek）
//!   WC_MODEL     覆盖默认模型
//!   WC_BASE_URL  覆盖 base_url（BYOK 自定义端点）
//!   WC_EFFORT    推理档位（low/medium/high/xhigh/max）；不设=关闭思考
//!   WC_THINKING  覆盖预设的 thinking 模式（如 qwen_thinking / reasoning_effort），便于对比
//!   WC_API_KEY   必填
//!
//! 流式输出 token 估计，结束打印最终内容与用量。

use std::io::Write;

use wisecortex_core::llm::{
    providers, ChatMessage, LlmClient, LlmRequest, ProviderConfig, WireFormat,
};

#[tokio::main]
async fn main() {
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Hello!".to_string());
    let provider_id = std::env::var("WC_PROVIDER").unwrap_or_else(|_| "deepseek".to_string());
    let api_key = std::env::var("WC_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        eprintln!("请设置 WC_API_KEY 环境变量。");
        std::process::exit(1);
    }

    let preset = providers::get(&provider_id);
    let base_url = std::env::var("WC_BASE_URL")
        .ok()
        .or_else(|| preset.map(|p| p.base_url.to_string()))
        .expect("未知 provider 且未提供 WC_BASE_URL");
    let model = std::env::var("WC_MODEL")
        .ok()
        .or_else(|| preset.map(|p| p.default_model.to_string()))
        .expect("未知 provider 且未提供 WC_MODEL");
    let format = preset.map(|p| p.format).unwrap_or(WireFormat::OpenAi);
    // 跟真实 agent 一样把预设的 thinking 模式套上（否则默认 ReasoningEffort，测不到 QwenThinking
    // 这类按预设分流的翻译）。WC_THINKING 可显式覆盖，便于比对两种模式对同一模型的行为。
    let thinking = std::env::var("WC_THINKING")
        .ok()
        .map(|s| serde_json::from_value(serde_json::Value::String(s)).unwrap_or_default())
        .unwrap_or_else(|| preset.map(|p| p.thinking).unwrap_or_default());

    eprintln!("→ provider={provider_id} model={model} base_url={base_url} format={format:?} thinking={thinking:?}");

    let cfg = ProviderConfig::new(base_url, api_key, format).with_thinking(thinking);
    let mut req = LlmRequest::new(
        model,
        vec![
            ChatMessage::system("You are a helpful assistant."),
            ChatMessage::user(prompt),
        ],
    );
    // 不设 WC_EFFORT = 关闭思考（reasoning_effort=None），与前端「关闭」档一致。
    if let Ok(eff) = std::env::var("WC_EFFORT") {
        if !eff.trim().is_empty() {
            req.reasoning_effort = Some(eff.trim().to_string());
        }
    }
    eprintln!("→ effort={:?}", req.reasoning_effort);

    let client = LlmClient::new();
    print!("\n=== 回复 ===\n");
    let _ = std::io::stdout().flush();
    let result = client
        .complete(&cfg, &req, |update| match update {
            // 逐字打印流式文本。
            wisecortex_core::llm::StreamUpdate::Text(t) => {
                print!("{t}");
                let _ = std::io::stdout().flush();
            }
            wisecortex_core::llm::StreamUpdate::Usage { input, output } => {
                eprint!("\r… tokens in={input} out~{output}   ");
                let _ = std::io::stderr().flush();
            }
            // 限流 / 上游 5xx 在自动重试：显示倒计时与第几次，别让人以为卡死了。
            wisecortex_core::llm::StreamUpdate::Retrying {
                attempt,
                max,
                wait_secs,
                kind,
            } => {
                eprint!(
                    "\r⏳ {} · {wait_secs}s 后重试 · 第 {attempt}/{max} 次   ",
                    kind.label()
                );
                let _ = std::io::stderr().flush();
            }
        })
        .await;

    eprintln!();
    match result {
        Ok(resp) => {
            let _ = &resp.content;
            if !resp.tool_calls.is_empty() {
                println!("\n=== 工具调用 ===");
                for tc in &resp.tool_calls {
                    println!("  {} {}", tc.name, tc.arguments);
                }
            }
            let u = resp.usage;
            println!(
                "\n=== 用量 === prompt={} completion={} total={} cache_read={} cache_write={}",
                u.prompt_tokens,
                u.completion_tokens,
                u.total_tokens,
                u.cache_read_input_tokens,
                u.cache_creation_input_tokens,
            );
        }
        Err(e) => {
            eprintln!("调用失败: {e}");
            std::process::exit(1);
        }
    }
}
