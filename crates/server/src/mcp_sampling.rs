//! MCP sampling 处理：把服务端的 `sampling/createMessage` 反向请求跑成一次 LLM 补全。
//!
//! core 的 mcp 模块只持一个处理器回调（[`wisecortex_core::mcp::set_sampling_handler`]）以避免依赖
//! LLM 运行时配置；本模块在 server 侧实现：解析当前激活 LLM 档 → 用 [`LlmClient`] 跑非流式补全 →
//! 回 MCP 约定的 result（role/content/model/stopReason）。在 stdio MCP 读线程上同步调用，故用
//! 捕获的 tokio `Handle::block_on` 驱动异步补全。

use serde_json::{json, Value};
use tokio::runtime::Handle;

use wisecortex_core::config;
use wisecortex_core::llm::{
    providers, ChatMessage, LlmClient, LlmRequest, ProviderConfig, WireFormat,
};

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

/// 解析当前激活 LLM 档为 (provider, model)；与 `Agent::configure` 同源（env > 配置 > 预设）。
fn resolve_active() -> Option<(ProviderConfig, String)> {
    let file = config::load();
    let active = file.active().cloned().unwrap_or_default();
    let key = env("WC_API_KEY").or(active.api_key).unwrap_or_default();
    if key.is_empty() {
        return None;
    }
    let provider_id = env("WC_PROVIDER")
        .or(active.provider)
        .unwrap_or_else(|| "deepseek".to_string());
    let preset = providers::get(&provider_id);
    let base_url = env("WC_BASE_URL")
        .or(active.base_url)
        .or_else(|| preset.map(|p| p.base_url.to_string()))?;
    let model = env("WC_MODEL")
        .or(active.model)
        .or_else(|| preset.map(|p| p.default_model.to_string()))?;
    let format = preset.map(|p| p.format).unwrap_or(WireFormat::OpenAi);
    let thinking = preset.map(|p| p.thinking).unwrap_or_default();
    Some((
        ProviderConfig::new(base_url, key, format).with_thinking(thinking),
        model,
    ))
}

/// 取 MCP content（字符串 / {type:text,text} / 数组）里的文本。
fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(_)) => content
            .and_then(|c| c.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|c| c.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// 把 sampling/createMessage 的 params 转成 canonical 消息（systemPrompt + messages[]）。
fn build_messages(params: &Value) -> Vec<ChatMessage> {
    let mut out = Vec::new();
    if let Some(sys) = params.get("systemPrompt").and_then(Value::as_str) {
        if !sys.is_empty() {
            out.push(ChatMessage::system(sys));
        }
    }
    if let Some(arr) = params.get("messages").and_then(Value::as_array) {
        for m in arr {
            let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
            let text = content_text(m.get("content"));
            out.push(match role {
                "assistant" => ChatMessage::assistant(text),
                _ => ChatMessage::user(text),
            });
        }
    }
    out
}

/// 处理一次 sampling/createMessage：跑 LLM 补全并返回 MCP result。
pub fn handle(rt: &Handle, params: &Value) -> Result<Value, String> {
    let (cfg, model) = resolve_active().ok_or("未配置激活 LLM，无法响应 MCP sampling")?;
    let messages = build_messages(params);
    if messages.is_empty() {
        return Err("sampling 请求无消息".to_string());
    }
    let max_tokens = params
        .get("maxTokens")
        .and_then(Value::as_u64)
        .unwrap_or(1024)
        .clamp(1, 32_000) as u32;
    let mut req = LlmRequest::new(model.clone(), messages);
    req.max_tokens = max_tokens;

    let client = LlmClient::new();
    let resp = rt
        .block_on(client.complete(&cfg, &req, |_| {}))
        .map_err(|e| e.to_string())?;
    let text = resp.content.unwrap_or_default();
    Ok(json!({
        "role": "assistant",
        "content": { "type": "text", "text": text },
        "model": model,
        "stopReason": "endTurn"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_text_handles_shapes() {
        assert_eq!(content_text(Some(&json!("hi"))), "hi");
        assert_eq!(
            content_text(Some(&json!({"type":"text","text":"yo"}))),
            "yo"
        );
        assert_eq!(
            content_text(Some(
                &json!([{"type":"text","text":"a"},{"type":"text","text":"b"}])
            )),
            "a\nb"
        );
        assert_eq!(content_text(None), "");
    }

    #[test]
    fn build_messages_includes_system_and_roles() {
        let params = json!({
            "systemPrompt": "be brief",
            "messages": [
                { "role": "user", "content": { "type": "text", "text": "q" } },
                { "role": "assistant", "content": { "type": "text", "text": "a" } }
            ]
        });
        let msgs = build_messages(&params);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].content.as_deref(), Some("be brief"));
        assert_eq!(msgs[1].content.as_deref(), Some("q"));
        assert_eq!(msgs[2].content.as_deref(), Some("a"));
    }
}
