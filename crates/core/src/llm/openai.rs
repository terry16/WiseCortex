//! OpenAI 兼容 wire format：请求构造 / 响应解析 / SSE 流聚合。

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::providers::ThinkingMode;
use super::{ChatMessage, LlmRequest, LlmResponse, Role, ToolCall, Usage};

/// API 路径（拼在 base_url 之后）。
pub const PATH: &str = "/chat/completions";

/// 构造请求体（不含 stream 标志，由 client 合并）。
/// `thinking` 决定深度思考参数如何表达（各家不同，见 [`ThinkingMode`]）。
pub fn build_request_body(req: &LlmRequest, thinking: ThinkingMode) -> Value {
    let messages: Vec<Value> = req.messages.iter().map(message_to_api).collect();
    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    body.insert("max_tokens".into(), json!(req.max_tokens));
    body.insert("messages".into(), json!(messages));

    if !req.tools.is_empty() {
        let mut tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect();
        // caching：给最后一个工具打 cache_control（仅对经 OpenRouter 的 Claude 有效，其余无害）。
        if req.caching_enabled {
            if let Some(last) = tools.last_mut() {
                last["cache_control"] = json!({ "type": "ephemeral" });
            }
        }
        body.insert("tools".into(), json!(tools));
    }

    // 深度思考：同一个「开启」意图，按 provider 翻成不同参数。空 effort=未开启（多数模式不发参数）；
    // 但有的模式（如火山的 minimal=不思考）即便关闭也要显式发一个值，故这里统一拿 &str（"" 表关闭）。
    let effort = req.reasoning_effort.as_deref().unwrap_or("");
    match thinking {
        ThinkingMode::ReasoningEffort => {
            // 档位天花板按**模型**定（GPT-5.6 到 max、5.2+ 到 xhigh、其余到 high），
            // 而非按 provider 一刀切——见 providers::clamp_effort。
            if let Some(eff) = super::providers::clamp_effort(&req.model, effort) {
                body.insert("reasoning_effort".into(), json!(eff));
            }
        }
        ThinkingMode::EnableThinking => {
            if !effort.is_empty() {
                body.insert("enable_thinking".into(), json!(true));
            }
        }
        // DeepSeek V4 等：思考开关用对象、强度仍走 reasoning_effort，两者一起发。
        // DeepSeek 思考强度只有 high / max 两档：max 保留，其余一律按 high（避免非法档位被上游拒）。
        ThinkingMode::ThinkingEnabledEffort => {
            if !effort.is_empty() {
                let eff = if effort == "max" { "max" } else { "high" };
                body.insert("thinking".into(), json!({ "type": "enabled" }));
                body.insert("reasoning_effort".into(), json!(eff));
            }
        }
        // 火山引擎/豆包：reasoning_effort 取 minimal/low/medium/high；关闭=显式 minimal（不思考）；
        // 高于 high 的档位（xhigh/max）钳到 high（火山没有这些档，避免被拒）。
        ThinkingMode::ReasoningEffortMinimal => {
            let eff = match effort {
                "" | "minimal" => "minimal",
                "low" => "low",
                "medium" => "medium",
                _ => "high",
            };
            body.insert("reasoning_effort".into(), json!(eff));
        }
        // Moonshot/Kimi：thinking:{type:enabled|disabled} 开关，无强度档；始终显式发以便真正关闭。
        ThinkingMode::ThinkingObjectToggle => {
            let ty = if effort.is_empty() {
                "disabled"
            } else {
                "enabled"
            };
            body.insert("thinking".into(), json!({ "type": ty }));
        }
        // 思考由模型内置或不支持：不发任何参数，避免被未知字段拒绝。
        ThinkingMode::None => {}
    }

    Value::Object(body)
}

fn message_to_api(msg: &ChatMessage) -> Value {
    match msg.role {
        Role::Assistant if !msg.tool_calls.is_empty() => {
            let tcs: Vec<Value> = msg
                .tool_calls
                .iter()
                .map(|tc| {
                    json!({
                        "id": tc.id,
                        "type": "function",
                        "function": { "name": tc.name, "arguments": tc.arguments },
                    })
                })
                .collect();
            json!({
                "role": "assistant",
                "content": msg.content,
                "tool_calls": tcs,
            })
        }
        Role::Tool => json!({
            "role": "tool",
            "tool_call_id": msg.tool_call_id,
            "content": msg.content.clone().unwrap_or_default(),
        }),
        // 带图片：content 用块数组（text + image_url）。
        role if !msg.images.is_empty() => {
            let mut blocks: Vec<Value> = Vec::new();
            if let Some(t) = msg.content.as_ref().filter(|s| !s.is_empty()) {
                blocks.push(json!({ "type": "text", "text": t }));
            }
            for url in &msg.images {
                blocks.push(json!({ "type": "image_url", "image_url": { "url": url } }));
            }
            json!({ "role": role.as_str(), "content": blocks })
        }
        role => json!({
            "role": role.as_str(),
            "content": msg.content.clone().unwrap_or_default(),
        }),
    }
}

/// 解析非流式响应（或聚合器产出的等价结构）为归一结果。
pub fn parse_response(data: &Value) -> LlmResponse {
    let message = data
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .cloned()
        .unwrap_or(Value::Null);

    let content = message
        .get("content")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    // 推理文本：DeepSeek/Qwen 等放在 reasoning_content，部分代理放 reasoning。
    let thinking = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|tc| {
                    let func = tc.get("function")?;
                    let name = func.get("name").and_then(Value::as_str)?;
                    let arguments = func.get("arguments").and_then(Value::as_str)?;
                    Some(ToolCall {
                        id: tc
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned(),
                        name: name.to_owned(),
                        arguments: arguments.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let finish_reason = data
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("finish_reason"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    LlmResponse {
        content,
        thinking,
        tool_calls,
        finish_reason,
        usage: parse_usage(data.get("usage").unwrap_or(&Value::Null)),
        ..Default::default()
    }
}

fn parse_usage(u: &Value) -> Usage {
    let g = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    let mut usage = Usage {
        prompt_tokens: g("prompt_tokens"),
        completion_tokens: g("completion_tokens"),
        total_tokens: g("total_tokens"),
        cache_read_input_tokens: g("cache_read_input_tokens"),
        cache_creation_input_tokens: g("cache_creation_input_tokens"),
        total_is_per_turn: false,
    };
    // OpenRouter 把缓存信息放在 prompt_tokens_details 下。
    if let Some(d) = u.get("prompt_tokens_details") {
        let cached = d.get("cached_tokens").and_then(Value::as_u64).unwrap_or(0);
        if cached > 0 {
            usage.cache_read_input_tokens = cached;
        }
    }
    // DeepSeek 用 prompt_cache_hit_tokens / prompt_cache_miss_tokens。
    // prompt_tokens 已含命中部分，这里只需取命中数作 cache_read。
    let ds_hit = u
        .get("prompt_cache_hit_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if ds_hit > 0 {
        usage.cache_read_input_tokens = ds_hit;
    }
    usage
}

// ── SSE 流聚合 ──────────────────────────────────────────────────────────────

#[derive(Default)]
struct ToolSlot {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

/// 把 OpenAI chat-completion 流帧重组为非流式响应结构。
#[derive(Default)]
pub struct OpenAiAggregator {
    content: String,
    /// 推理增量累积（DeepSeek/Qwen 的 reasoning_content）；仅展示，不回灌历史。
    reasoning: String,
    role: String,
    finish_reason: Option<String>,
    tool_calls: BTreeMap<i64, ToolSlot>,
    usage: Option<Value>,
}

impl OpenAiAggregator {
    pub fn new() -> Self {
        OpenAiAggregator {
            role: "assistant".to_string(),
            ..Default::default()
        }
    }

    /// 处理一行 `data:` 负载（不含 "data: " 前缀）。
    /// 返回 (是否终止帧[DONE], 本帧的文本增量)。
    pub fn handle(&mut self, data_str: &str) -> (bool, Option<String>) {
        if data_str.trim() == "[DONE]" {
            return (true, None);
        }
        let Ok(data) = serde_json::from_str::<Value>(data_str) else {
            return (false, None);
        };

        let mut text_delta = None;
        if let Some(choice) = data.get("choices").and_then(|c| c.get(0)) {
            if let Some(delta) = choice.get("delta") {
                if let Some(r) = delta.get("role").and_then(Value::as_str) {
                    self.role = r.to_owned();
                }
                if let Some(c) = delta.get("content").and_then(Value::as_str) {
                    self.content.push_str(c);
                    if !c.is_empty() {
                        text_delta = Some(c.to_owned());
                    }
                }
                // 推理增量：DeepSeek/Qwen 放 reasoning_content，部分代理放 reasoning。
                if let Some(r) = delta
                    .get("reasoning_content")
                    .or_else(|| delta.get("reasoning"))
                    .and_then(Value::as_str)
                {
                    self.reasoning.push_str(r);
                }
                if let Some(tcs) = delta.get("tool_calls").and_then(Value::as_array) {
                    for tc in tcs {
                        self.merge_tool_call(tc);
                    }
                }
            }
            if let Some(fr) = choice.get("finish_reason").and_then(Value::as_str) {
                self.finish_reason = Some(fr.to_owned());
            }
        }

        if let Some(u) = data.get("usage") {
            if !u.is_null() {
                self.usage = Some(u.clone());
            }
        }
        (false, text_delta)
    }

    fn merge_tool_call(&mut self, tc: &Value) {
        let idx = tc.get("index").and_then(Value::as_i64).unwrap_or(0);
        let slot = self.tool_calls.entry(idx).or_default();
        if let Some(id) = tc.get("id").and_then(Value::as_str) {
            slot.id.get_or_insert_with(|| id.to_owned());
        }
        if let Some(func) = tc.get("function") {
            if let Some(name) = func.get("name").and_then(Value::as_str) {
                slot.name.get_or_insert_with(|| name.to_owned());
            }
            if let Some(args) = func.get("arguments").and_then(Value::as_str) {
                slot.arguments.push_str(args);
            }
        }
    }

    /// 当前 (input_tokens, output_tokens) 估计：有 usage 帧用真实值，否则按字符/4 估算。
    pub fn progress(&self) -> (u64, u64) {
        if let Some(u) = &self.usage {
            let i = u.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0);
            let o = u
                .get("completion_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            (i, o)
        } else {
            let chars = self.content.len()
                + self
                    .tool_calls
                    .values()
                    .map(|t| t.arguments.len())
                    .sum::<usize>();
            (0, (chars as u64).div_ceil(4))
        }
    }

    /// 重建非流式响应结构。
    pub fn into_value(self) -> Value {
        let tool_calls: Vec<Value> = self
            .tool_calls
            .into_values()
            .map(|tc| {
                json!({
                    "id": tc.id,
                    "type": "function",
                    "function": { "name": tc.name, "arguments": tc.arguments },
                })
            })
            .collect();

        let mut message = Map::new();
        message.insert("role".into(), json!(self.role));
        message.insert(
            "content".into(),
            if self.content.is_empty() {
                Value::Null
            } else {
                json!(self.content)
            },
        );
        if !tool_calls.is_empty() {
            message.insert("tool_calls".into(), json!(tool_calls));
        }
        if !self.reasoning.is_empty() {
            message.insert("reasoning_content".into(), json!(self.reasoning));
        }

        json!({
            "choices": [{ "index": 0, "message": message, "finish_reason": self.finish_reason }],
            "usage": self.usage.unwrap_or_else(|| json!({})),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_basic_request() {
        let req = LlmRequest::new("gpt-5.5", vec![ChatMessage::user("hi")]);
        let body = build_request_body(&req, ThinkingMode::ReasoningEffort);
        assert_eq!(body["model"], "gpt-5.5");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "hi");
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn thinking_param_varies_by_mode() {
        let mut req = LlmRequest::new("m", vec![ChatMessage::user("hi")]);
        req.reasoning_effort = Some("high".into());

        // OpenAI 一族：reasoning_effort: <level>。
        let b = build_request_body(&req, ThinkingMode::ReasoningEffort);
        assert_eq!(b["reasoning_effort"], "high");
        assert!(b.get("enable_thinking").is_none());

        // 千问/混元：enable_thinking: true（布尔，不带强度）。
        let b = build_request_body(&req, ThinkingMode::EnableThinking);
        assert_eq!(b["enable_thinking"], true);
        assert!(b.get("reasoning_effort").is_none());

        // 内置/不支持：两个都不发。
        let b = build_request_body(&req, ThinkingMode::None);
        assert!(b.get("reasoning_effort").is_none());
        assert!(b.get("enable_thinking").is_none());

        // DeepSeek V4：thinking 对象 + reasoning_effort 一起发。
        let b = build_request_body(&req, ThinkingMode::ThinkingEnabledEffort);
        assert_eq!(b["thinking"]["type"], "enabled");
        assert_eq!(b["reasoning_effort"], "high");
        assert!(b.get("enable_thinking").is_none());
    }

    #[test]
    fn deepseek_effort_clamped_to_high_or_max() {
        let mk = |e: &str| {
            let mut req = LlmRequest::new("m", vec![ChatMessage::user("hi")]);
            req.reasoning_effort = Some(e.into());
            build_request_body(&req, ThinkingMode::ThinkingEnabledEffort)
        };
        // DeepSeek 只有 high / max 两档：max 保留，其余（low/medium/high/xhigh）一律按 high。
        assert_eq!(mk("max")["reasoning_effort"], "max");
        assert_eq!(mk("high")["reasoning_effort"], "high");
        assert_eq!(mk("low")["reasoning_effort"], "high");
        assert_eq!(mk("medium")["reasoning_effort"], "high");
        assert_eq!(mk("xhigh")["reasoning_effort"], "high");
        // 始终带 thinking 对象开关。
        assert_eq!(mk("low")["thinking"]["type"], "enabled");
    }

    #[test]
    fn volcengine_reasoning_effort_minimal_mapping() {
        let mk = |e: Option<&str>| {
            let mut req = LlmRequest::new("m", vec![ChatMessage::user("hi")]);
            req.reasoning_effort = e.map(str::to_string);
            build_request_body(&req, ThinkingMode::ReasoningEffortMinimal)
        };
        // 火山：minimal/low/medium/high 四档；关闭(None/"")=显式 minimal；高于 high 钳到 high。
        assert_eq!(mk(None)["reasoning_effort"], "minimal");
        assert_eq!(mk(Some(""))["reasoning_effort"], "minimal");
        assert_eq!(mk(Some("low"))["reasoning_effort"], "low");
        assert_eq!(mk(Some("medium"))["reasoning_effort"], "medium");
        assert_eq!(mk(Some("high"))["reasoning_effort"], "high");
        assert_eq!(mk(Some("xhigh"))["reasoning_effort"], "high");
        assert_eq!(mk(Some("max"))["reasoning_effort"], "high");
        // 不发 thinking 对象 / enable_thinking。
        assert!(mk(Some("high")).get("thinking").is_none());
        assert!(mk(Some("high")).get("enable_thinking").is_none());
    }

    #[test]
    fn reasoning_effort_clamps_anthropic_only_levels() {
        let mk = |e: &str| {
            let mut req = LlmRequest::new("m", vec![ChatMessage::user("hi")]);
            req.reasoning_effort = Some(e.into());
            build_request_body(&req, ThinkingMode::ReasoningEffort)
        };
        // OpenAI 兼容端点只认 low/medium/high；xhigh/max 是 Anthropic 专有，钳到 high。
        assert_eq!(mk("low")["reasoning_effort"], "low");
        assert_eq!(mk("medium")["reasoning_effort"], "medium");
        assert_eq!(mk("high")["reasoning_effort"], "high");
        assert_eq!(mk("xhigh")["reasoning_effort"], "high");
        assert_eq!(mk("max")["reasoning_effort"], "high");
    }

    #[test]
    fn moonshot_thinking_object_toggle() {
        let mk = |e: Option<&str>| {
            let mut req = LlmRequest::new("m", vec![ChatMessage::user("hi")]);
            req.reasoning_effort = e.map(str::to_string);
            build_request_body(&req, ThinkingMode::ThinkingObjectToggle)
        };
        // 任意非空档位 → enabled；关闭(None/"") → 显式 disabled。无 reasoning_effort、无强度。
        assert_eq!(mk(Some("high"))["thinking"]["type"], "enabled");
        assert_eq!(mk(Some("low"))["thinking"]["type"], "enabled");
        assert_eq!(mk(None)["thinking"]["type"], "disabled");
        assert_eq!(mk(Some(""))["thinking"]["type"], "disabled");
        assert!(mk(Some("high")).get("reasoning_effort").is_none());
        assert!(mk(Some("high")).get("enable_thinking").is_none());
    }

    #[test]
    fn no_thinking_param_when_effort_empty() {
        // 未开启思考时（effort 为空），即便是 enable_thinking 模式也不发参数。
        let req = LlmRequest::new("m", vec![ChatMessage::user("hi")]);
        let b = build_request_body(&req, ThinkingMode::EnableThinking);
        assert!(b.get("enable_thinking").is_none());
    }

    #[test]
    fn aggregates_content_and_usage_stream() {
        let mut agg = OpenAiAggregator::new();
        agg.handle(r#"{"choices":[{"index":0,"delta":{"role":"assistant"}}]}"#);
        agg.handle(r#"{"choices":[{"index":0,"delta":{"content":"Hel"}}]}"#);
        agg.handle(r#"{"choices":[{"index":0,"delta":{"content":"lo"}}]}"#);
        agg.handle(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#);
        agg.handle(
            r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":3,"prompt_tokens_details":{"cached_tokens":2}}}"#,
        );
        let (done, _) = agg.handle("[DONE]");
        assert!(done);

        let resp = parse_response(&agg.into_value());
        assert_eq!(resp.content.as_deref(), Some("Hello"));
        assert_eq!(resp.finish_reason.as_deref(), Some("stop"));
        assert_eq!(resp.usage.prompt_tokens, 12);
        assert_eq!(resp.usage.completion_tokens, 3);
        assert_eq!(resp.usage.cache_read_input_tokens, 2);
    }

    #[test]
    fn user_images_become_image_url_blocks() {
        let req = LlmRequest::new(
            "m",
            vec![ChatMessage::user_with_images(
                "look",
                vec!["data:image/png;base64,AAA".to_string()],
            )],
        );
        let content =
            &build_request_body(&req, ThinkingMode::ReasoningEffort)["messages"][0]["content"];
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(content[1]["image_url"]["url"], "data:image/png;base64,AAA");
    }

    #[test]
    fn aggregates_streamed_tool_call() {
        let mut agg = OpenAiAggregator::new();
        agg.handle(
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_x","function":{"name":"shell","arguments":"{\"cmd"}}]}}]}"#,
        );
        agg.handle(
            r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\":\"ls\"}"}}]}}]}"#,
        );
        agg.handle(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#);

        let resp = parse_response(&agg.into_value());
        assert_eq!(resp.tool_calls.len(), 1);
        let tc = &resp.tool_calls[0];
        assert_eq!(tc.id, "call_x");
        assert_eq!(tc.name, "shell");
        assert_eq!(tc.arguments, r#"{"cmd":"ls"}"#);
        assert_eq!(resp.finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn aggregates_reasoning_content_stream() {
        let mut agg = OpenAiAggregator::new();
        agg.handle(
            r#"{"choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"Hmm "}}]}"#,
        );
        agg.handle(r#"{"choices":[{"index":0,"delta":{"reasoning_content":"think."}}]}"#);
        agg.handle(r#"{"choices":[{"index":0,"delta":{"content":"Answer"}}]}"#);
        agg.handle(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#);

        let resp = parse_response(&agg.into_value());
        assert_eq!(resp.thinking.as_deref(), Some("Hmm think."));
        assert_eq!(resp.content.as_deref(), Some("Answer"));
    }
}
