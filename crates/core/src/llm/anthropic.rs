//! Anthropic wire format：请求构造 / 响应解析 / SSE 流聚合。
//!
//! 关键点（缓存命中率的根基）：cache_control 标记必须稳定——同一前缀每回合字节一致，
//! 否则 cache_read 归零。这里按 ChatMessage.cache_breakpoint 放置 ephemeral 标记。

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::{ChatMessage, LlmRequest, LlmResponse, Role, ToolCall, Usage};

/// API 路径（拼在 base_url 之后）。
pub const PATH: &str = "/v1/messages";
/// 必带的 API 版本头。
pub const VERSION: &str = "2023-06-01";

/// Anthropic 要求 tool_use.id 匹配 ^[a-zA-Z0-9_-]+$ 且 ≤128。
pub fn sanitize_tool_use_id(id: &str) -> String {
    let mut s: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.len() > 128 {
        s.truncate(128);
    }
    s
}

/// 构造请求体（不含 stream 标志，由 client 合并）。
pub fn build_request_body(req: &LlmRequest) -> Value {
    // system 用「文本块数组」而非拼接成单串：Claude 订阅 OAuth 后端会校验 system 的
    // **首块**是否恰为 Claude Code 身份句，单块/单串里夹了别的内容（哪怕首句是身份句）
    // 都会被判非 Claude Code、回 `429 rate_limit_error{message:"Error"}`（实测：
    // [身份块,正文块]→200；[身份句+正文 拼成一块]→429；单串→429）。每条 system 消息
    // 各成一块，client.rs 在 OAuth 时把身份句作为首条 system 消息插入，于是它正好是首块。
    let system_blocks: Vec<Value> = req
        .messages
        .iter()
        .filter(|m| m.role == Role::System)
        .filter_map(|m| m.content.clone())
        .filter(|s| !s.is_empty())
        .map(|s| json!({ "type": "text", "text": s }))
        .collect();

    let messages: Vec<Value> = req
        .messages
        .iter()
        .filter(|m| m.role != Role::System)
        .map(message_to_api)
        .collect();

    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("max_tokens".into(), json!(req.max_tokens));
    body.insert("messages".into(), json!(messages));

    if !system_blocks.is_empty() {
        body.insert("system".into(), json!(system_blocks));
    }

    if !req.tools.is_empty() {
        let mut tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect();
        if req.caching_enabled {
            if let Some(last) = tools.last_mut() {
                last["cache_control"] = json!({ "type": "ephemeral" });
            }
        }
        body.insert("tools".into(), json!(tools));
    }

    if let Some(effort) = normalized_effort(req.reasoning_effort.as_deref(), &req.model) {
        body.insert("thinking".into(), json!({ "type": "adaptive" }));
        body.insert("output_config".into(), json!({ "effort": effort }));
    }

    Value::Object(body)
}

/// data URL / http URL → Anthropic 图片块。
fn url_to_image_block(url: &str) -> Option<serde_json::Value> {
    if url.is_empty() {
        return None;
    }
    if let Some(rest) = url.strip_prefix("data:") {
        if let Some((meta, data)) = rest.split_once(',') {
            let media = meta.split(';').next().unwrap_or("image/png");
            return Some(json!({
                "type": "image",
                "source": { "type": "base64", "media_type": media, "data": data }
            }));
        }
    }
    Some(json!({ "type": "image", "source": { "type": "url", "url": url } }))
}

/// 归一化推理强度，并按模型能力兜底。
///
/// `xhigh` 是 **Opus 独有**的档（写码甜点档，Claude Code 默认）。实测 2026-07-30：
/// sonnet-4-6 带 `effort: xhigh` 会被硬回
/// `400 This model does not support effort level 'xhigh'. Supported levels: high, low, max, medium.`
/// ——全局把 reasoning_effort 设成 xhigh 的用户，一切到 Sonnet 档就整个用不了。
/// 全局设置是「我要最强」的意图表达，不该因为换个模型就把会话打死：非 Opus 上降到 `high`，
/// 而不是报错或干脆关掉思考。
fn normalized_effort<'a>(effort: Option<&'a str>, model: &str) -> Option<&'a str> {
    let e = match effort {
        Some(e @ ("low" | "medium" | "high" | "xhigh" | "max")) => e,
        // 空串/未知=关闭。
        _ => return None,
    };
    if e == "xhigh" && !model.contains("opus") {
        return Some("high");
    }
    Some(e)
}

fn message_to_api(msg: &ChatMessage) -> Value {
    let cc = || json!({ "type": "ephemeral" });

    match msg.role {
        // assistant + tool_calls → content blocks (text? + tool_use[])
        Role::Assistant if !msg.tool_calls.is_empty() => {
            let mut blocks: Vec<Value> = Vec::new();
            if let Some(text) = msg.content.as_ref().filter(|s| !s.is_empty()) {
                blocks.push(json!({ "type": "text", "text": text }));
            }
            for tc in &msg.tool_calls {
                let input: Value = serde_json::from_str(&tc.arguments).unwrap_or(json!({}));
                blocks.push(json!({
                    "type": "tool_use",
                    "id": sanitize_tool_use_id(&tc.id),
                    "name": tc.name,
                    "input": input,
                }));
            }
            json!({ "role": "assistant", "content": blocks })
        }
        // tool result → user message with tool_result block
        Role::Tool => {
            let mut block = json!({
                "type": "tool_result",
                "tool_use_id": sanitize_tool_use_id(msg.tool_call_id.as_deref().unwrap_or("")),
                "content": msg.content.clone().unwrap_or_default(),
            });
            if msg.cache_breakpoint {
                block["cache_control"] = cc();
            }
            json!({ "role": "user", "content": [block] })
        }
        // regular user/assistant text（可带图片）
        role => {
            let text = msg.content.clone().unwrap_or_default();
            let mut blocks: Vec<Value> = Vec::new();
            for url in &msg.images {
                if let Some(b) = url_to_image_block(url) {
                    blocks.push(b);
                }
            }
            let mut tb = if text.is_empty() && blocks.is_empty() {
                json!({ "type": "text", "text": "..." })
            } else if text.is_empty() {
                // 只有图片：补一个占位文本块，满足 API 对非空内容的要求由图片块满足，文本可省略。
                // 这里直接走图片块，不加空文本。
                json!(null)
            } else {
                json!({ "type": "text", "text": text })
            };
            if !tb.is_null() {
                if msg.cache_breakpoint {
                    tb["cache_control"] = cc();
                }
                blocks.insert(0, tb);
            }
            json!({ "role": role.as_str(), "content": blocks })
        }
    }
}

/// 解析非流式响应（或聚合器产出的等价结构）为归一结果。
pub fn parse_response(data: &Value) -> LlmResponse {
    let blocks = data.get("content").and_then(Value::as_array);

    let content = blocks.map(|arr| {
        arr.iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<String>()
    });
    let content = content.filter(|s| !s.is_empty());

    let thinking = blocks.map(|arr| {
        arr.iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("thinking"))
            .filter_map(|b| b.get("thinking").and_then(Value::as_str))
            .collect::<String>()
    });
    let thinking = thinking.filter(|s| !s.is_empty());

    let tool_calls: Vec<ToolCall> = blocks
        .map(|arr| {
            arr.iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
                .map(|b| {
                    let input = b.get("input").cloned().unwrap_or(json!({}));
                    let arguments = if input.is_string() {
                        input.as_str().unwrap().to_owned()
                    } else {
                        input.to_string()
                    };
                    ToolCall {
                        id: b.get("id").and_then(Value::as_str).unwrap_or("").to_owned(),
                        name: b
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        arguments,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let finish_reason = match data.get("stop_reason").and_then(Value::as_str) {
        Some("end_turn") => Some("stop".to_owned()),
        Some("tool_use") => Some("tool_calls".to_owned()),
        Some("max_tokens") => Some("length".to_owned()),
        other => other.map(str::to_owned),
    };

    // 文本式工具调用兜底：模型偶发把 `<invoke …>` 当普通文本吐出（见 recover_text_tool_calls）。
    // 一旦文本里出现标记，就剥掉它（防原样回灌历史污染模型）；结构化 tool_use 为空时再用抢救出的
    // 调用顶上，让 agent 循环能继续而非静默停。结构化调用已存在时只清洗、不重复抢救（避免双重执行）。
    let mut content = content;
    let mut tool_calls = tool_calls;
    let mut recovered_text_tool_calls = 0;
    let mut had_unparsed_tool_markup = false;
    if let Some(raw) = content.clone() {
        let rec = super::recover_text_tool_calls(&raw);
        if rec.had_markup {
            content = Some(rec.cleaned).filter(|s| !s.is_empty());
            if tool_calls.is_empty() {
                if rec.calls.is_empty() {
                    had_unparsed_tool_markup = true;
                } else {
                    recovered_text_tool_calls = rec.calls.len();
                    tool_calls = rec.calls;
                }
            }
        }
    }

    // 兜底安全网：即便认不出开头标记（未知/揉碎的泄漏格式），只要正文里有 `invoke>` 这种强工具
    // 调用闭合信号且没有任何工具调用，就标记为「见到无法解析的工具调用标记」——让 agent 给可见
    // 告警而非静默停（覆盖 recover 没法解析的新格式）。
    if tool_calls.is_empty() && !had_unparsed_tool_markup {
        if let Some(c) = content.as_deref() {
            if c.contains("invoke>") {
                had_unparsed_tool_markup = true;
            }
        }
    }

    // 订阅 OAuth 身份（CLAUDE_CODE_SPOOF）下，模型偶发在工具调用前多吐一个独占一行的 "call"
    // 残词；有工具调用时剥掉它，免得聊天区每次工具前都冒出一个突兀的 "call"。
    if !tool_calls.is_empty() {
        if let Some(c) = content.as_deref() {
            if let Some(cleaned) = strip_trailing_call_token(c) {
                content = (!cleaned.is_empty()).then_some(cleaned);
            }
        }
    }

    LlmResponse {
        content,
        thinking,
        tool_calls,
        finish_reason,
        usage: parse_usage(data.get("usage").unwrap_or(&Value::Null)),
        recovered_text_tool_calls,
        had_unparsed_tool_markup,
        thought_signature: None,
    }
}

/// 剥掉助手正文尾部独占一行的 "call" 残词（订阅 OAuth 身份下工具调用前的偶发产物）。
/// 仅当 "call" 自成一行（前面是换行）或正文就是 "call" 时才剥，避免误删正常以 call 结尾的句子。
/// 返回 Some(清洗后) 表示剥过；None 表示无需改动。
fn strip_trailing_call_token(s: &str) -> Option<String> {
    let t = s.trim_end();
    if t == "call" {
        return Some(String::new());
    }
    let prefix = t.strip_suffix("call")?;
    // 仅当 "call" 独占一行（前面是换行）才剥，避免误删正常以 call 结尾的句子。
    if prefix.ends_with('\n') {
        Some(prefix.trim_end().to_string())
    } else {
        None
    }
}

fn parse_usage(u: &Value) -> Usage {
    let g = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    let raw_input = g("input_tokens");
    let cache_read = g("cache_read_input_tokens");
    let cache_creation = g("cache_creation_input_tokens");
    let output = g("output_tokens");
    Usage {
        prompt_tokens: raw_input + cache_read,
        completion_tokens: output,
        total_tokens: raw_input + cache_creation + output,
        cache_read_input_tokens: cache_read,
        cache_creation_input_tokens: cache_creation,
        total_is_per_turn: true,
    }
}

// ── SSE 流聚合 ──────────────────────────────────────────────────────────────

enum Block {
    Text(String),
    Thinking(String),
    ToolUse {
        id: String,
        name: String,
        input_str: String,
    },
}

/// 把 Anthropic Messages SSE 流重组为非流式响应结构。
#[derive(Default)]
pub struct AnthropicAggregator {
    blocks: BTreeMap<i64, Block>,
    stop_reason: Option<String>,
    usage: Map<String, Value>,
}

impl AnthropicAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// 处理一个 SSE 事件，返回本次的文本增量（若有）。
    pub fn handle(&mut self, event: &str, data_str: &str) -> Option<String> {
        let Ok(data) = serde_json::from_str::<Value>(data_str) else {
            return None;
        };
        let mut text_delta = None;
        match event {
            "message_start" => {
                if let Some(u) = data.get("message").and_then(|m| m.get("usage")) {
                    self.merge_usage(u);
                }
            }
            "content_block_start" => {
                let idx = data.get("index").and_then(Value::as_i64).unwrap_or(0);
                let cb = data.get("content_block").cloned().unwrap_or(Value::Null);
                match cb.get("type").and_then(Value::as_str) {
                    Some("tool_use") => {
                        self.blocks.insert(
                            idx,
                            Block::ToolUse {
                                id: cb
                                    .get("id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_owned(),
                                name: cb
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_owned(),
                                input_str: String::new(),
                            },
                        );
                    }
                    // thinking / redacted_thinking 块：累积可读思考文本（redacted 无文本）。
                    Some("thinking") | Some("redacted_thinking") => {
                        self.blocks.insert(idx, Block::Thinking(String::new()));
                    }
                    _ => {
                        self.blocks.insert(idx, Block::Text(String::new()));
                    }
                }
            }
            "content_block_delta" => {
                let idx = data.get("index").and_then(Value::as_i64).unwrap_or(0);
                let delta = data.get("delta").cloned().unwrap_or(Value::Null);
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        let t = delta.get("text").and_then(Value::as_str).unwrap_or("");
                        match self
                            .blocks
                            .entry(idx)
                            .or_insert_with(|| Block::Text(String::new()))
                        {
                            Block::Text(s) => s.push_str(t),
                            Block::Thinking(_) | Block::ToolUse { .. } => {}
                        }
                        if !t.is_empty() {
                            text_delta = Some(t.to_owned());
                        }
                    }
                    Some("input_json_delta") => {
                        let p = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if let Some(Block::ToolUse { input_str, .. }) = self.blocks.get_mut(&idx) {
                            input_str.push_str(p);
                        }
                    }
                    Some("thinking_delta") => {
                        let t = delta.get("thinking").and_then(Value::as_str).unwrap_or("");
                        if let Some(Block::Thinking(s)) = self.blocks.get_mut(&idx) {
                            s.push_str(t);
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(sr) = data
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    self.stop_reason = Some(sr.to_owned());
                }
                if let Some(u) = data.get("usage") {
                    self.merge_usage(u);
                }
            }
            _ => {}
        }
        text_delta
    }

    fn merge_usage(&mut self, u: &Value) {
        if let Some(obj) = u.as_object() {
            for (k, v) in obj {
                self.usage.insert(k.clone(), v.clone());
            }
        }
    }

    /// 当前 (input_tokens, output_tokens) 估计。
    pub fn progress(&self) -> (u64, u64) {
        let g = |k: &str| self.usage.get(k).and_then(Value::as_u64).unwrap_or(0);
        let known_in = g("input_tokens") + g("cache_read_input_tokens");
        match self.usage.get("output_tokens").and_then(Value::as_u64) {
            Some(o) => (known_in, o),
            None => {
                let chars: usize = self
                    .blocks
                    .values()
                    .map(|b| match b {
                        Block::Text(s) => s.len(),
                        Block::Thinking(s) => s.len(),
                        Block::ToolUse { input_str, .. } => input_str.len(),
                    })
                    .sum();
                (known_in, (chars as u64).div_ceil(4))
            }
        }
    }

    pub fn into_value(self) -> Value {
        let content: Vec<Value> = self
            .blocks
            .into_values()
            .map(|b| match b {
                Block::Text(text) => json!({ "type": "text", "text": text }),
                Block::Thinking(text) => json!({ "type": "thinking", "thinking": text }),
                Block::ToolUse {
                    id,
                    name,
                    input_str,
                } => {
                    let input = if input_str.is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&input_str).unwrap_or(json!(input_str))
                    };
                    json!({ "type": "tool_use", "id": id, "name": name, "input": input })
                }
            })
            .collect();

        json!({ "content": content, "stop_reason": self.stop_reason, "usage": Value::Object(self.usage) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::ToolDef;

    #[test]
    fn build_request_body_maps_effort_levels_to_thinking() {
        let mut req = LlmRequest::new("claude-opus-5", vec![ChatMessage::user("hi")]);
        // xhigh / max 也应被接受（Opus 支持；xhigh 是写码甜点档）。
        req.reasoning_effort = Some("xhigh".into());
        let body = build_request_body(&req);
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["output_config"]["effort"], "xhigh");
        req.reasoning_effort = Some("max".into());
        assert_eq!(build_request_body(&req)["output_config"]["effort"], "max");
        // 仍接受 low/medium/high。
        req.reasoning_effort = Some("high".into());
        assert_eq!(build_request_body(&req)["output_config"]["effort"], "high");
        // 空串 / None / 未知档 → 关闭，不发 thinking 参数。
        req.reasoning_effort = Some(String::new());
        assert!(build_request_body(&req).get("thinking").is_none());
        req.reasoning_effort = None;
        assert!(build_request_body(&req).get("thinking").is_none());
        req.reasoning_effort = Some("turbo".into());
        assert!(build_request_body(&req).get("thinking").is_none());
    }

    /// 回归（2026-07-30 实测）：sonnet 带 xhigh 会被硬回 400
    /// `This model does not support effort level 'xhigh'`——全局设 xhigh 的用户一切到 Sonnet
    /// 就整个用不了。非 Opus 上降到 high，别把会话打死，也别把思考整个关掉。
    #[test]
    fn xhigh_is_clamped_to_high_on_non_opus_models() {
        let mut req = LlmRequest::new("claude-sonnet-4-6", vec![ChatMessage::user("hi")]);
        req.reasoning_effort = Some("xhigh".into());
        let body = build_request_body(&req);
        assert_eq!(
            body["output_config"]["effort"], "high",
            "sonnet 不支持 xhigh"
        );
        // 思考本身不能被顺手关掉：用户要的是「最强」，降档不是关档。
        assert_eq!(body["thinking"]["type"], "adaptive");
        // Opus 原样保留。
        let mut opus = LlmRequest::new("claude-opus-4-8", vec![ChatMessage::user("hi")]);
        opus.reasoning_effort = Some("xhigh".into());
        assert_eq!(
            build_request_body(&opus)["output_config"]["effort"],
            "xhigh"
        );
        // max 是各家都支持的档，不该被误降。
        req.reasoning_effort = Some("max".into());
        assert_eq!(build_request_body(&req)["output_config"]["effort"], "max");
    }

    #[test]
    fn extracts_system_and_builds_blocks() {
        let req = LlmRequest::new(
            "claude-sonnet-4-6",
            vec![ChatMessage::system("be brief"), ChatMessage::user("hi")],
        );
        let body = build_request_body(&req);
        // system 为文本块数组，首块即第一条 system 消息（订阅 OAuth 要求首块=身份句）。
        assert_eq!(body["system"][0]["type"], "text");
        assert_eq!(body["system"][0]["text"], "be brief");
        // system 不应出现在 messages 里
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"][0]["text"], "hi");
    }

    #[test]
    fn tools_become_input_schema_with_cache_on_last() {
        let mut req = LlmRequest::new("claude-sonnet-4-6", vec![ChatMessage::user("x")]);
        req.caching_enabled = true;
        req.tools = vec![ToolDef {
            name: "shell".into(),
            description: "run".into(),
            parameters: json!({ "type": "object" }),
        }];
        let body = build_request_body(&req);
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(body["tools"][0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn cache_breakpoint_marks_message() {
        let req = LlmRequest::new(
            "claude-sonnet-4-6",
            vec![ChatMessage::user("hi").with_cache_breakpoint()],
        );
        let body = build_request_body(&req);
        assert_eq!(
            body["messages"][0]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
    }

    #[test]
    fn sanitizes_tool_use_id() {
        assert_eq!(sanitize_tool_use_id("shell:0"), "shell_0");
    }

    #[test]
    fn user_images_become_image_blocks() {
        let req = LlmRequest::new(
            "claude",
            vec![ChatMessage::user_with_images(
                "look",
                vec!["data:image/png;base64,AAA".to_string()],
            )],
        );
        let content = &build_request_body(&req)["messages"][0]["content"];
        let arr = content.as_array().unwrap();
        assert!(arr.iter().any(|b| b["type"] == "image"));
        assert_eq!(
            arr.iter().find(|b| b["type"] == "image").unwrap()["source"]["media_type"],
            "image/png"
        );
    }

    #[test]
    fn parse_response_normalizes_usage_and_tools() {
        let data = json!({
            "content": [
                { "type": "text", "text": "ok" },
                { "type": "tool_use", "id": "t1", "name": "shell", "input": { "cmd": "ls" } }
            ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 10, "cache_read_input_tokens": 90, "output_tokens": 5 }
        });
        let resp = parse_response(&data);
        assert_eq!(resp.content.as_deref(), Some("ok"));
        assert_eq!(resp.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(resp.tool_calls[0].name, "shell");
        assert_eq!(resp.tool_calls[0].arguments, r#"{"cmd":"ls"}"#);
        assert_eq!(resp.usage.prompt_tokens, 100); // 10 + 90
        assert_eq!(resp.usage.cache_read_input_tokens, 90);
        assert!(resp.usage.total_is_per_turn);
    }

    #[test]
    fn aggregates_sse_text_and_tool_use() {
        let mut agg = AnthropicAggregator::new();
        agg.handle(
            "message_start",
            r#"{"message":{"usage":{"input_tokens":10}}}"#,
        );
        agg.handle(
            "content_block_start",
            r#"{"index":0,"content_block":{"type":"text"}}"#,
        );
        agg.handle(
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"text_delta","text":"Hi"}}"#,
        );
        agg.handle(
            "content_block_start",
            r#"{"index":1,"content_block":{"type":"tool_use","id":"t1","name":"shell"}}"#,
        );
        agg.handle(
            "content_block_delta",
            r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"{\"cmd\":\"ls\"}"}}"#,
        );
        agg.handle(
            "message_delta",
            r#"{"delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":7}}"#,
        );

        let resp = parse_response(&agg.into_value());
        assert_eq!(resp.content.as_deref(), Some("Hi"));
        assert_eq!(resp.tool_calls[0].name, "shell");
        assert_eq!(resp.tool_calls[0].arguments, r#"{"cmd":"ls"}"#);
        assert_eq!(resp.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(resp.usage.completion_tokens, 7);
    }

    #[test]
    fn aggregates_thinking_then_text() {
        let mut agg = AnthropicAggregator::new();
        agg.handle(
            "content_block_start",
            r#"{"index":0,"content_block":{"type":"thinking"}}"#,
        );
        agg.handle(
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"thinking_delta","thinking":"Let me "}}"#,
        );
        agg.handle(
            "content_block_delta",
            r#"{"index":0,"delta":{"type":"thinking_delta","thinking":"reason."}}"#,
        );
        agg.handle(
            "content_block_start",
            r#"{"index":1,"content_block":{"type":"text"}}"#,
        );
        agg.handle(
            "content_block_delta",
            r#"{"index":1,"delta":{"type":"text_delta","text":"Done"}}"#,
        );
        let resp = parse_response(&agg.into_value());
        assert_eq!(resp.thinking.as_deref(), Some("Let me reason."));
        assert_eq!(resp.content.as_deref(), Some("Done"));
    }

    #[test]
    fn parse_response_recovers_text_tool_call_when_no_structured_tool_use() {
        let data = json!({
            "content": [
                { "type": "text", "text": "我来提交\n<invoke name=\"git\">\n<parameter name=\"args\">status</parameter>\n</invoke>" }
            ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 5, "output_tokens": 3 }
        });
        let resp = parse_response(&data);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "git");
        assert_eq!(resp.tool_calls[0].arguments, r#"{"args":"status"}"#);
        assert_eq!(resp.recovered_text_tool_calls, 1);
        assert!(!resp.had_unparsed_tool_markup);
        // 文本里的标记被剥掉，只剩散文（防污染回灌）。
        assert_eq!(resp.content.as_deref(), Some("我来提交"));
    }

    #[test]
    fn parse_response_keeps_structured_call_and_strips_markup_on_double_emission() {
        // 模型同时发了真 tool_use 和文本式调用：保留真调用、剥掉文本标记、不重复抢救。
        let data = json!({
            "content": [
                { "type": "text", "text": "顺手记一下\n<invoke name=\"git\"><parameter name=\"args\">status</parameter></invoke>" },
                { "type": "tool_use", "id": "t1", "name": "remember", "input": { "note": "x" } }
            ],
            "stop_reason": "tool_use",
            "usage": {}
        });
        let resp = parse_response(&data);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "remember"); // 只剩结构化那个
        assert_eq!(resp.recovered_text_tool_calls, 0);
        assert!(!resp.had_unparsed_tool_markup);
        assert_eq!(resp.content.as_deref(), Some("顺手记一下"));
    }

    #[test]
    fn parse_response_flags_unparsable_tool_markup() {
        let data = json!({
            "content": [
                { "type": "text", "text": "开始写\n<invoke name=\"write_file\">\n<parameter name=\"path\">a.txt" }
            ],
            "stop_reason": "end_turn",
            "usage": {}
        });
        let resp = parse_response(&data);
        assert!(resp.tool_calls.is_empty());
        assert!(resp.had_unparsed_tool_markup);
        assert_eq!(resp.recovered_text_tool_calls, 0);
        assert_eq!(resp.content.as_deref(), Some("开始写")); // 残缺标记被剥掉
    }

    #[test]
    fn parse_response_flags_unrecognized_invoke_residue_as_unparsed() {
        // 认不出开头标记的泄漏形态（连 invoke 标签都被揉碎），但只要有 invoke 闭合残留且无工具
        // 调用，就标记 had_unparsed —— 让 agent 给可见告警而非静默停（兜底，覆盖未知格式）。
        let data = json!({
            "content": [{ "type": "text", "text": "嗯\nnvoke nam read_file 340\n</invoke>" }],
            "stop_reason": "end_turn",
            "usage": {}
        });
        let resp = parse_response(&data);
        assert!(resp.tool_calls.is_empty());
        assert!(
            resp.had_unparsed_tool_markup,
            "认不出的工具调用泄漏也应被标记，避免静默停"
        );
    }

    #[test]
    fn parse_response_strips_trailing_call_token_before_tool_use() {
        // 订阅 OAuth 身份下模型在工具调用前多吐一个独占一行的 "call"——有工具调用时应剥掉。
        let data = json!({
            "content": [
                { "type": "text", "text": "现在编译。\n\ncall" },
                { "type": "tool_use", "id": "t1", "name": "shell", "input": { "cmd": "ls" } }
            ],
            "stop_reason": "tool_use",
            "usage": {}
        });
        let resp = parse_response(&data);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.content.as_deref(), Some("现在编译。"));
    }

    #[test]
    fn parse_response_call_only_content_becomes_none() {
        // 正文只有一个 "call" 残词 → 剥成空 → content 置 None（不在聊天区留空泡）。
        let data = json!({
            "content": [
                { "type": "text", "text": "call" },
                { "type": "tool_use", "id": "t1", "name": "shell", "input": {} }
            ],
            "stop_reason": "tool_use",
            "usage": {}
        });
        let resp = parse_response(&data);
        assert_eq!(resp.content, None);
    }

    #[test]
    fn strip_trailing_call_token_only_when_line_isolated() {
        assert_eq!(
            strip_trailing_call_token("做完了。\n\ncall").as_deref(),
            Some("做完了。")
        );
        assert_eq!(strip_trailing_call_token("call").as_deref(), Some(""));
        // 句子里正常以 call 结尾（非独占行）→ 不动。
        assert_eq!(strip_trailing_call_token("let me make the call"), None);
        assert_eq!(strip_trailing_call_token("recall"), None);
        assert_eq!(strip_trailing_call_token("没有残词"), None);
    }
}
