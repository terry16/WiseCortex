//! OpenAI **Responses API**（`/responses`）线缆：请求构造 + SSE 流聚合。
//!
//! ChatGPT / Codex **订阅**后端（`https://chatgpt.com/backend-api/codex`）说这套协议——
//! 它不是 chat/completions，而是 Responses：system 走 `instructions`、历史走 `input` 数组
//! （message / function_call / function_call_output 三种 item），流式事件是 `response.*`。
//!
//! 与 anthropic.rs / openai.rs 同构：`build_request_body` 出站、`ResponsesAggregator` 聚合 SSE。

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::{LlmRequest, LlmResponse, Role, ToolCall, Usage};

/// 拼在 base_url 之后的路径。
pub const PATH: &str = "/responses";

/// 构造 Responses 请求体（不含 stream 标志，由 client 合并）。
///
/// `subscription` 为 true 时面向 ChatGPT/Codex **订阅**后端
/// （`chatgpt.com/backend-api/codex`）——它不接受 `max_output_tokens`，会以
/// `400 {"detail":"Unsupported parameter: max_output_tokens"}` 拒绝；订阅按额度计费、
/// 本就无需输出上限。仅标准 Responses API（按量付费、用 api_key）才带输出上限。
pub fn build_request_body(req: &LlmRequest, subscription: bool) -> Value {
    // system → instructions（多条拼接）。
    let instructions: String = req
        .messages
        .iter()
        .filter(|m| m.role == Role::System)
        .filter_map(|m| m.content.clone())
        .collect::<Vec<_>>()
        .join("\n\n");

    // 其余消息 → input items。
    let mut input: Vec<Value> = Vec::new();
    for m in req.messages.iter().filter(|m| m.role != Role::System) {
        match m.role {
            Role::User => {
                let text = m.content.clone().unwrap_or_default();
                let mut content: Vec<Value> = Vec::new();
                if !text.is_empty() {
                    content.push(json!({ "type": "input_text", "text": text }));
                }
                for url in &m.images {
                    content.push(json!({ "type": "input_image", "image_url": url }));
                }
                if content.is_empty() {
                    content.push(json!({ "type": "input_text", "text": "" }));
                }
                input.push(json!({ "type": "message", "role": "user", "content": content }));
            }
            Role::Assistant => {
                if let Some(t) = m.content.as_ref().filter(|s| !s.is_empty()) {
                    input.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": t }],
                    }));
                }
                for tc in &m.tool_calls {
                    input.push(json!({
                        "type": "function_call",
                        "call_id": tc.id,
                        "name": tc.name,
                        "arguments": tc.arguments,
                    }));
                }
            }
            Role::Tool => {
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": m.tool_call_id.clone().unwrap_or_default(),
                    "output": m.content.clone().unwrap_or_default(),
                }));
            }
            Role::System => {}
        }
    }

    let mut body = Map::new();
    body.insert("model".into(), json!(req.model));
    if !instructions.is_empty() {
        body.insert("instructions".into(), json!(instructions));
    }
    body.insert("input".into(), json!(input));
    // 订阅后端不持久化对话。
    body.insert("store".into(), json!(false));
    // 订阅后端会拒绝 max_output_tokens（400 Unsupported parameter）；只有标准
    // Responses API 才带输出上限。
    if !subscription {
        body.insert("max_output_tokens".into(), json!(req.max_tokens));
    }

    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                })
            })
            .collect();
        body.insert("tools".into(), json!(tools));
        body.insert("tool_choice".into(), json!("auto"));
    }

    // 档位按模型钳（GPT-5.6 到 max、5.2+/codex-max 到 xhigh、其余到 high）。
    // 早先这里是 `filter(matches!(low|medium|high))`——**超纲档被整个丢掉**而不是钳下来，
    // 于是选了 xhigh/max 反而连 reasoning 字段都不发，静默退回上游默认（medium）。
    if let Some(eff) = super::providers::clamp_effort(
        &req.model,
        req.reasoning_effort.as_deref().unwrap_or_default(),
    ) {
        body.insert("reasoning".into(), json!({ "effort": eff }));
    }

    Value::Object(body)
}

// ── SSE 流聚合 ──────────────────────────────────────────────────────────────

#[derive(Default)]
struct FnCall {
    call_id: String,
    name: String,
    args: String,
}

/// 把 Responses SSE 流聚合为最终 [`LlmResponse`]。
#[derive(Default)]
pub struct ResponsesAggregator {
    text: String,
    reasoning: String,
    /// function_call 按 item_id 累积；order 记录出现顺序。
    calls: BTreeMap<String, FnCall>,
    order: Vec<String>,
    usage: Option<Value>,
    stop_reason: Option<String>,
}

impl ResponsesAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    fn call_mut(&mut self, item_id: &str) -> &mut FnCall {
        if !self.calls.contains_key(item_id) {
            self.order.push(item_id.to_string());
            self.calls.insert(item_id.to_string(), FnCall::default());
        }
        self.calls.get_mut(item_id).unwrap()
    }

    /// 处理一个 SSE 事件，返回本次可见文本增量（若有）。
    pub fn handle(&mut self, event: &str, data_str: &str) -> Option<String> {
        let Ok(data) = serde_json::from_str::<Value>(data_str) else {
            return None;
        };
        match event {
            "response.output_text.delta" => {
                let d = data.get("delta").and_then(Value::as_str).unwrap_or("");
                if !d.is_empty() {
                    self.text.push_str(d);
                    return Some(d.to_owned());
                }
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                let d = data.get("delta").and_then(Value::as_str).unwrap_or("");
                self.reasoning.push_str(d);
            }
            "response.output_item.added" => {
                let item = data.get("item").cloned().unwrap_or(Value::Null);
                if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    let id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    let c = self.call_mut(&id);
                    if let Some(cid) = item.get("call_id").and_then(Value::as_str) {
                        c.call_id = cid.to_owned();
                    }
                    if let Some(n) = item.get("name").and_then(Value::as_str) {
                        c.name = n.to_owned();
                    }
                    if let Some(a) = item.get("arguments").and_then(Value::as_str) {
                        c.args = a.to_owned();
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                let id = data
                    .get("item_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let d = data
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                self.call_mut(&id).args.push_str(&d);
            }
            "response.function_call_arguments.done" => {
                if let Some(args) = data.get("arguments").and_then(Value::as_str) {
                    let id = data
                        .get("item_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    self.call_mut(&id).args = args.to_owned();
                }
            }
            "response.output_item.done" => {
                let item = data.get("item").cloned().unwrap_or(Value::Null);
                if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    let id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    let c = self.call_mut(&id);
                    if let Some(cid) = item.get("call_id").and_then(Value::as_str) {
                        c.call_id = cid.to_owned();
                    }
                    if let Some(n) = item.get("name").and_then(Value::as_str) {
                        c.name = n.to_owned();
                    }
                    if let Some(a) = item.get("arguments").and_then(Value::as_str) {
                        c.args = a.to_owned();
                    }
                }
            }
            "response.completed" => {
                if let Some(u) = data
                    .get("response")
                    .and_then(|r| r.get("usage"))
                    .filter(|v| !v.is_null())
                {
                    self.usage = Some(u.clone());
                }
                self.stop_reason.get_or_insert_with(|| "stop".to_string());
            }
            "response.failed" | "response.error" | "error" => {
                self.stop_reason = Some("error".to_string());
            }
            _ => {}
        }
        None
    }

    /// 当前 (input_tokens, output_tokens) 估计：有 usage 用真实值，否则按字符/4 估算。
    pub fn progress(&self) -> (u64, u64) {
        if let Some(u) = &self.usage {
            let g = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
            (g("input_tokens"), g("output_tokens"))
        } else {
            let chars = self.text.len() + self.calls.values().map(|c| c.args.len()).sum::<usize>();
            (0, (chars as u64).div_ceil(4))
        }
    }

    /// 聚合为最终结果。
    pub fn into_response(self) -> LlmResponse {
        let content = (!self.text.is_empty()).then_some(self.text);
        let thinking = (!self.reasoning.is_empty()).then_some(self.reasoning);
        let tool_calls: Vec<ToolCall> = self
            .order
            .iter()
            .filter_map(|id| self.calls.get(id))
            .filter(|c| !c.name.is_empty())
            .map(|c| ToolCall {
                id: c.call_id.clone(),
                name: c.name.clone(),
                arguments: c.args.clone(),
            })
            .collect();
        let finish_reason = if !tool_calls.is_empty() {
            Some("tool_calls".to_string())
        } else {
            self.stop_reason.or_else(|| Some("stop".to_string()))
        };
        let usage = parse_usage(self.usage.as_ref().unwrap_or(&Value::Null));
        LlmResponse {
            content,
            thinking,
            tool_calls,
            finish_reason,
            usage,
            ..Default::default()
        }
    }
}

fn parse_usage(u: &Value) -> Usage {
    let g = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    let input = g("input_tokens");
    let output = g("output_tokens");
    let cached = u
        .get("input_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Usage {
        prompt_tokens: input,
        completion_tokens: output,
        total_tokens: if g("total_tokens") > 0 {
            g("total_tokens")
        } else {
            input + output
        },
        cache_read_input_tokens: cached,
        cache_creation_input_tokens: 0,
        total_is_per_turn: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{ChatMessage, ToolDef};

    #[test]
    fn request_maps_system_to_instructions_and_input() {
        let req = LlmRequest::new(
            "gpt-5-codex",
            vec![ChatMessage::system("be brief"), ChatMessage::user("hi")],
        );
        let body = build_request_body(&req, false);
        assert_eq!(body["instructions"], "be brief");
        assert_eq!(body["input"][0]["type"], "message");
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
        assert_eq!(body["input"][0]["content"][0]["text"], "hi");
        assert_eq!(body["store"], false);
    }

    #[test]
    fn xhigh_reaches_the_wire_instead_of_being_silently_dropped() {
        // 回归：这里曾用 filter(low|medium|high) 过滤，**超纲档被整个丢掉**——于是用户选了
        // xhigh，body 里连 reasoning 字段都没有，静默退回上游默认 medium（表现为「答得飞快」）。
        let mut req = LlmRequest::new("gpt-5.6-sol", vec![ChatMessage::user("hi")]);
        req.reasoning_effort = Some("xhigh".into());
        assert_eq!(
            build_request_body(&req, true)["reasoning"]["effort"],
            "xhigh"
        );

        req.reasoning_effort = Some("max".into());
        assert_eq!(build_request_body(&req, true)["reasoning"]["effort"], "max");

        // 认不得 max 的模型钳到自己的天花板，而不是丢掉。
        let mut old = LlmRequest::new("gpt-5-codex", vec![ChatMessage::user("hi")]);
        old.reasoning_effort = Some("max".into());
        assert_eq!(
            build_request_body(&old, true)["reasoning"]["effort"],
            "high"
        );

        // 未配置 → 照旧不发 reasoning。
        let bare = LlmRequest::new("gpt-5.6-sol", vec![ChatMessage::user("hi")]);
        assert!(build_request_body(&bare, true).get("reasoning").is_none());
    }

    #[test]
    fn subscription_omits_max_output_tokens_standard_keeps_it() {
        let mut req = LlmRequest::new("gpt-5-codex", vec![ChatMessage::user("ping")]);
        req.max_tokens = 16;
        // 订阅后端：带 max_output_tokens 会被 400「Unsupported parameter」拒掉，必须省略。
        let sub = build_request_body(&req, true);
        assert!(
            sub.get("max_output_tokens").is_none(),
            "订阅后端不能带 max_output_tokens"
        );
        // 标准 Responses API：仍带输出上限。
        let std = build_request_body(&req, false);
        assert_eq!(std["max_output_tokens"], 16);
    }

    #[test]
    fn request_maps_tools_and_history() {
        let mut req = LlmRequest::new(
            "gpt-5-codex",
            vec![
                ChatMessage::assistant_tool_calls(
                    None,
                    vec![ToolCall {
                        id: "call_1".into(),
                        name: "shell".into(),
                        arguments: r#"{"cmd":"ls"}"#.into(),
                    }],
                ),
                ChatMessage::tool_result("call_1", "file1\nfile2"),
            ],
        );
        req.tools = vec![ToolDef {
            name: "shell".into(),
            description: "run".into(),
            parameters: json!({ "type": "object" }),
        }];
        let body = build_request_body(&req, false);
        // tool def → tools[].type=function
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["name"], "shell");
        // assistant tool call → function_call item
        assert_eq!(body["input"][0]["type"], "function_call");
        assert_eq!(body["input"][0]["call_id"], "call_1");
        assert_eq!(body["input"][0]["arguments"], r#"{"cmd":"ls"}"#);
        // tool result → function_call_output item
        assert_eq!(body["input"][1]["type"], "function_call_output");
        assert_eq!(body["input"][1]["call_id"], "call_1");
        assert_eq!(body["input"][1]["output"], "file1\nfile2");
    }

    #[test]
    fn aggregates_text_and_usage() {
        let mut agg = ResponsesAggregator::new();
        assert_eq!(
            agg.handle("response.output_text.delta", r#"{"delta":"Hel"}"#),
            Some("Hel".to_string())
        );
        agg.handle("response.output_text.delta", r#"{"delta":"lo"}"#);
        agg.handle(
            "response.completed",
            r#"{"response":{"usage":{"input_tokens":12,"output_tokens":3,"input_tokens_details":{"cached_tokens":4}}}}"#,
        );
        let resp = agg.into_response();
        assert_eq!(resp.content.as_deref(), Some("Hello"));
        assert_eq!(resp.finish_reason.as_deref(), Some("stop"));
        assert_eq!(resp.usage.prompt_tokens, 12);
        assert_eq!(resp.usage.completion_tokens, 3);
        assert_eq!(resp.usage.cache_read_input_tokens, 4);
    }

    #[test]
    fn aggregates_function_call() {
        let mut agg = ResponsesAggregator::new();
        agg.handle(
            "response.output_item.added",
            r#"{"item":{"type":"function_call","id":"fc_1","call_id":"call_x","name":"shell"}}"#,
        );
        agg.handle(
            "response.function_call_arguments.delta",
            r#"{"item_id":"fc_1","delta":"{\"cmd"}"#,
        );
        agg.handle(
            "response.function_call_arguments.done",
            r#"{"item_id":"fc_1","arguments":"{\"cmd\":\"ls\"}"}"#,
        );
        agg.handle("response.completed", r#"{"response":{}}"#);
        let resp = agg.into_response();
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "call_x");
        assert_eq!(resp.tool_calls[0].name, "shell");
        assert_eq!(resp.tool_calls[0].arguments, r#"{"cmd":"ls"}"#);
        assert_eq!(resp.finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn aggregates_reasoning() {
        let mut agg = ResponsesAggregator::new();
        agg.handle(
            "response.reasoning_summary_text.delta",
            r#"{"delta":"think "}"#,
        );
        agg.handle(
            "response.reasoning_summary_text.delta",
            r#"{"delta":"hard"}"#,
        );
        agg.handle("response.output_text.delta", r#"{"delta":"ok"}"#);
        agg.handle("response.completed", r#"{"response":{}}"#);
        let resp = agg.into_response();
        assert_eq!(resp.thinking.as_deref(), Some("think hard"));
        assert_eq!(resp.content.as_deref(), Some("ok"));
    }
}
