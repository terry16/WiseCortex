//! **原生 Gemini（Code Assist）线缆**：请求构造 + SSE 流聚合。
//!
//! Gemini 订阅（个人 Google 账号）走 Code Assist 后端 `cloudcode-pa.googleapis.com`，协议既不是
//! OpenAI 也不是 Anthropic：内容是 `contents/parts`，请求被包进 `{model, project, request:{…}}`
//! 信封，流式响应每个 chunk 外面还裹一层 `.response`（`CaGenerateContentResponse`）。
//!
//! 与 anthropic.rs / openai.rs / responses.rs 同构：`build_request_body` 出站、`GeminiAggregator`
//! 聚合 SSE。鉴权与 project id 由 [`super::oauth_gemini`] 提供，client.rs 组装请求时注入。
//!
//! 几个易错点（都按官方 google-gemini/gemini-cli 源钉死）：
//!   - **函数结果按 name 配对**：Gemini 无 call id，`functionResponse.name` 必须从历史里
//!     `tool_call_id → 函数名` 映射回填，否则模型认不出这是哪次调用的结果。
//!   - **SSE 要剥一层 `.response`**：chunk 是 `CaGenerateContentResponse`，真正的 candidates/
//!     usageMetadata 在 `chunk.response` 里。
//!   - **JSON-Schema 要清洗**：Code Assist 用 OpenAPI 子集，`additionalProperties`/`$schema`/`$ref`
//!     等关键字会被 400 拒，出站前递归剥掉。

use serde_json::{json, Map, Value};

use super::{LlmRequest, LlmResponse, Role, ToolCall, Usage};

/// 拼在 base_url（= `oauth_gemini::api_base()`）之后的流式路径。
pub const STREAM_PATH: &str = "/v1internal:streamGenerateContent?alt=sse";

/// 拿不到真实思考签名时用的官方占位符，可跳过校验。
///
/// Gemini 3 系要求「本轮里每步第一个 functionCall」原样回传模型给出的 `thoughtSignature`，
/// 否则报 `400 Function call is missing a thought_signature`。但有两类调用天生没有签名：
/// 本次改动之前落盘的历史，以及从别家模型迁移/人工构造的工具调用。
/// 官方为此给了两个占位串（见 thought-signatures 文档），发它即跳过校验——
/// 总好过让整个会话被 400 焊死。
pub const SKIP_THOUGHT_SIGNATURE: &str = "skip_thought_signature_validator";

/// Code Assist / Gemini 的函数声明 schema 不认的 JSON-Schema 关键字（出站前递归剥除，
/// 否则整段 tools 会被 `400 INVALID_ARGUMENT` 拒掉）。保守清单：宁可丢约束也不让请求被打回。
const UNSUPPORTED_SCHEMA_KEYS: &[&str] = &[
    "additionalProperties",
    "$schema",
    "$ref",
    "$defs",
    "definitions",
    "patternProperties",
    "format",
    "pattern",
    "const",
    "examples",
    "default",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "minLength",
    "maxLength",
];

/// 递归清洗 JSON-Schema：剥掉 Gemini 不支持的**关键字**。纯函数，便于单测。
///
/// ⚠️ **只在 schema 节点上剥关键字，并按结构递归**——`properties` 下面的键是**用户定义的属性名**
/// （如 grep 工具的 `pattern` 参数），不是 schema 关键字。早先版本无差别按键名剥，把名叫 `pattern`
/// 的**属性**删了，而 `required:["pattern"]` 还在，被 Google 以
/// 「required fields ['pattern'] are not defined in the schema properties」400 拒掉整个请求（实测踩过）。
pub fn sanitize_schema(v: &Value) -> Value {
    let Value::Object(map) = v else {
        return v.clone();
    };
    let mut out = Map::new();
    for (k, val) in map {
        if UNSUPPORTED_SCHEMA_KEYS.contains(&k.as_str()) {
            continue;
        }
        let cleaned = match k.as_str() {
            // 键=属性名（原样保留，绝不当关键字剥），值=子 schema（递归清洗）。
            "properties" => match val {
                Value::Object(p) => Value::Object(
                    p.iter()
                        .map(|(name, sub)| (name.clone(), sanitize_schema(sub)))
                        .collect(),
                ),
                other => other.clone(),
            },
            // 值本身就是子 schema。
            "items" | "not" => sanitize_schema(val),
            // 子 schema 数组。
            "anyOf" | "oneOf" | "allOf" => match val {
                Value::Array(a) => Value::Array(a.iter().map(sanitize_schema).collect()),
                other => sanitize_schema(other),
            },
            // 其余（type/description/enum/required/minimum/maxItems…）值是标量或字符串数组，
            // 不是 schema，原样保留、不递归。
            _ => val.clone(),
        };
        out.insert(k.clone(), cleaned);
    }
    Value::Object(out)
}

/// 从一段 data URL（`data:<mime>;base64,<data>`）拆出 (mimeType, base64)。非 data URL 返回 None。
fn parse_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let mime = meta.strip_suffix(";base64").unwrap_or(meta);
    if mime.is_empty() {
        return None;
    }
    Some((mime.to_string(), data.to_string()))
}

/// 构造 Code Assist 请求信封：`{model, project, user_prompt_id, request:{contents,systemInstruction,
/// tools,generationConfig}}`（不含 stream 标志——流式由端点 `?alt=sse` 决定）。
pub fn build_request_body(req: &LlmRequest, project: &str, prompt_id: &str) -> Value {
    // system → systemInstruction（多条拼接）。
    let system: String = req
        .messages
        .iter()
        .filter(|m| m.role == Role::System)
        .filter_map(|m| m.content.clone())
        .collect::<Vec<_>>()
        .join("\n\n");

    // tool_call_id → 函数名映射（functionResponse 按 name 回填用）。
    let mut id_to_name: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for m in &req.messages {
        for tc in &m.tool_calls {
            id_to_name.insert(tc.id.as_str(), tc.name.as_str());
        }
    }

    // 其余消息 → contents。
    let mut contents: Vec<Value> = Vec::new();
    for m in req.messages.iter().filter(|m| m.role != Role::System) {
        match m.role {
            Role::User => {
                let mut parts: Vec<Value> = Vec::new();
                if let Some(t) = m.content.as_ref().filter(|s| !s.is_empty()) {
                    parts.push(json!({ "text": t }));
                }
                for url in &m.images {
                    if let Some((mime, data)) = parse_data_url(url) {
                        parts.push(json!({ "inlineData": { "mimeType": mime, "data": data } }));
                    }
                }
                if parts.is_empty() {
                    parts.push(json!({ "text": "" }));
                }
                contents.push(json!({ "role": "user", "parts": parts }));
            }
            Role::Assistant => {
                let mut parts: Vec<Value> = Vec::new();
                if let Some(t) = m.content.as_ref().filter(|s| !s.is_empty()) {
                    parts.push(json!({ "text": t }));
                }
                for (i, tc) in m.tool_calls.iter().enumerate() {
                    let args: Value =
                        serde_json::from_str(&tc.arguments).unwrap_or_else(|_| json!({}));
                    let mut part = json!({ "functionCall": { "name": tc.name, "args": args } });
                    // 思考签名只挂**本步第一个** functionCall（Gemini 3 规范如此；挂到后续的会被拒）。
                    if i == 0 {
                        part["thoughtSignature"] = json!(m
                            .thought_signature
                            .as_deref()
                            .unwrap_or(SKIP_THOUGHT_SIGNATURE));
                    }
                    parts.push(part);
                }
                if parts.is_empty() {
                    parts.push(json!({ "text": "" }));
                }
                contents.push(json!({ "role": "model", "parts": parts }));
            }
            Role::Tool => {
                // Gemini 无 call id：按 tool_call_id 反查函数名（找不到时退回用 id 当名，尽量不丢）。
                let id = m.tool_call_id.as_deref().unwrap_or_default();
                let name = id_to_name.get(id).copied().unwrap_or(id);
                let content = m.content.clone().unwrap_or_default();
                contents.push(json!({
                    "role": "user",
                    "parts": [{
                        "functionResponse": {
                            "name": name,
                            "response": { "result": content },
                        }
                    }],
                }));
            }
            Role::System => {}
        }
    }

    // Gemini 要求 contents 里 user/model 交替：把相邻同角色的 content 合并（parts 拼接）。
    // WiseCortex 历史里会出现连续 user——并行工具结果各成一条 functionResponse、以及「干活途中
    // 补充的消息」会连续追加多条 user 文本——不合并会被 Gemini 以「须交替」400 拒。
    let contents = coalesce_same_role(contents);

    // request 主体。
    let mut request = Map::new();
    request.insert("contents".into(), json!(contents));
    if !system.is_empty() {
        request.insert(
            "systemInstruction".into(),
            json!({ "parts": [{ "text": system }] }),
        );
    }
    if !req.tools.is_empty() {
        let decls: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": sanitize_schema(&t.parameters),
                })
            })
            .collect();
        request.insert("tools".into(), json!([{ "functionDeclarations": decls }]));
    }
    request.insert("generationConfig".into(), generation_config(req));

    let mut envelope = Map::new();
    envelope.insert("model".into(), json!(req.model));
    // 免费/托管账号无显式 project（Code Assist 从账号推断托管项目）→ 省略该字段，别发空串。
    if !project.is_empty() {
        envelope.insert("project".into(), json!(project));
    }
    envelope.insert("user_prompt_id".into(), json!(prompt_id));
    envelope.insert("request".into(), Value::Object(request));
    Value::Object(envelope)
}

/// 合并相邻同 `role` 的 content（把后者的 parts 拼进前者），保证 user/model 交替。
fn coalesce_same_role(contents: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::with_capacity(contents.len());
    for c in contents {
        let same = out
            .last()
            .and_then(|last| last.get("role"))
            .zip(c.get("role"))
            .map(|(a, b)| a == b)
            .unwrap_or(false);
        if same {
            let last = out.last_mut().expect("same 为真则 out 非空");
            if let (Some(lp), Some(cp)) = (
                last.get_mut("parts").and_then(Value::as_array_mut),
                c.get("parts").and_then(Value::as_array),
            ) {
                lp.extend(cp.iter().cloned());
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// generationConfig：maxOutputTokens + 思考档（仅 reasoning_effort 给出时带 thinkingConfig）。
fn generation_config(req: &LlmRequest) -> Value {
    let mut cfg = Map::new();
    cfg.insert("maxOutputTokens".into(), json!(req.max_tokens));
    if let Some(eff) = req
        .reasoning_effort
        .as_deref()
        .filter(|e| matches!(*e, "low" | "medium" | "high" | "xhigh"))
    {
        // 按 Google OpenAI 兼容层官方映射对齐真实 budget（low=1024/medium=8192/high=24576）；
        // xhigh → -1（动态，让模型尽量深思）。
        // 注意：gemini-2.5-pro 最低预算为 128，无法关闭思考——用 1024 作为 low 档最小有效值，
        // 避免发 thinkingBudget=0 对 Pro 无效（2.5-flash 可发 0，但统一用 1024 更安全）。
        let budget: i32 = match eff {
            "low" => 1024,
            "medium" => 8192,
            "high" => 24576,
            _ => -1, // xhigh → 动态
        };
        cfg.insert(
            "thinkingConfig".into(),
            json!({ "includeThoughts": true, "thinkingBudget": budget }),
        );
    }
    Value::Object(cfg)
}

// ── SSE 流聚合 ──────────────────────────────────────────────────────────────

/// 把 Code Assist streamGenerateContent 的 SSE 流聚合为最终 [`LlmResponse`]。
#[derive(Default)]
pub struct GeminiAggregator {
    /// 跨 `data:` 行的 JSON 累积缓冲（应对单个 chunk 被拆到多行的少见情形）。
    buf: String,
    text: String,
    thinking: String,
    tool_calls: Vec<ToolCall>,
    call_seq: u64,
    /// 思考签名：按规范只有「本步第一个 functionCall」带，故只记第一个。
    thought_signature: Option<String>,
    finish_reason: Option<String>,
    // usageMetadata（取流中出现的最新值）。
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    cached_tokens: u64,
}

impl GeminiAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// 处理一个 `data:` 负载，返回本次可见文本增量（若有）。chunk 可能跨多行 → 累积到能整体解析。
    pub fn handle(&mut self, data: &str) -> Option<String> {
        self.buf.push_str(data);
        let chunk: Value = match serde_json::from_str(&self.buf) {
            Ok(v) => v,
            Err(_) => return None, // 尚不完整，等下一行
        };
        self.buf.clear();
        self.process(&chunk)
    }

    fn process(&mut self, chunk: &Value) -> Option<String> {
        // 剥一层 `.response`（Code Assist 比原生 Gemini API 多包一层）；缺失则就地用 chunk。
        let resp = chunk.get("response").unwrap_or(chunk);

        // usageMetadata（累积值，取最新）。
        if let Some(u) = resp.get("usageMetadata") {
            let g = |k: &str| u.get(k).and_then(Value::as_u64);
            if let Some(v) = g("promptTokenCount") {
                self.prompt_tokens = v;
            }
            if let Some(v) = g("candidatesTokenCount") {
                self.completion_tokens = v;
            }
            if let Some(v) = g("totalTokenCount") {
                self.total_tokens = v;
            }
            if let Some(v) = g("cachedContentTokenCount") {
                self.cached_tokens = v;
            }
        }

        let mut delta = String::new();
        if let Some(cand) = resp
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
        {
            if let Some(fr) = cand.get("finishReason").and_then(Value::as_str) {
                self.finish_reason = Some(fr.to_string());
            }
            if let Some(parts) = cand
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(Value::as_array)
            {
                for part in parts {
                    // functionCall part。
                    if let Some(fc) = part.get("functionCall") {
                        let name = fc
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let args = fc.get("args").cloned().unwrap_or_else(|| json!({}));
                        // 思考签名挂在 part 上（与 functionCall 同级）；个别版本放在 functionCall
                        // 内部，两处都认一下。只取第一个——规范就是只给第一个 functionCall 发。
                        if self.thought_signature.is_none() {
                            self.thought_signature = part
                                .get("thoughtSignature")
                                .or_else(|| fc.get("thoughtSignature"))
                                .and_then(Value::as_str)
                                .filter(|s| !s.is_empty())
                                .map(str::to_string);
                        }
                        if !name.is_empty() {
                            let id = format!("gemini_call_{}", self.call_seq);
                            self.call_seq += 1;
                            self.tool_calls.push(ToolCall {
                                id,
                                name,
                                arguments: args.to_string(),
                            });
                        }
                        continue;
                    }
                    // 文本 part：thought=true 归推理，否则归可见文本。
                    if let Some(t) = part.get("text").and_then(Value::as_str) {
                        if part
                            .get("thought")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                        {
                            self.thinking.push_str(t);
                        } else {
                            self.text.push_str(t);
                            delta.push_str(t);
                        }
                    }
                }
            }
        }

        (!delta.is_empty()).then_some(delta)
    }

    /// 当前 (input_tokens, output_tokens)。
    pub fn progress(&self) -> (u64, u64) {
        (self.prompt_tokens, self.completion_tokens)
    }

    /// 聚合为最终结果。
    pub fn into_response(self) -> LlmResponse {
        let content = (!self.text.is_empty()).then_some(self.text);
        let thinking = (!self.thinking.is_empty()).then_some(self.thinking);
        let finish_reason = if !self.tool_calls.is_empty() {
            Some("tool_calls".to_string())
        } else {
            Some(
                self.finish_reason
                    .map(|s| s.to_lowercase())
                    .unwrap_or_else(|| "stop".to_string()),
            )
        };
        let total = if self.total_tokens > 0 {
            self.total_tokens
        } else {
            self.prompt_tokens + self.completion_tokens
        };
        let usage = Usage {
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            total_tokens: total,
            cache_read_input_tokens: self.cached_tokens,
            cache_creation_input_tokens: 0,
            total_is_per_turn: true,
        };
        LlmResponse {
            content,
            thinking,
            tool_calls: self.tool_calls,
            finish_reason,
            usage,
            thought_signature: self.thought_signature,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{ChatMessage, ToolDef};

    fn req(messages: Vec<ChatMessage>) -> LlmRequest {
        LlmRequest::new("gemini-2.5-pro", messages)
    }

    #[test]
    fn envelope_wraps_model_project_and_request() {
        let body = build_request_body(&req(vec![ChatMessage::user("hi")]), "proj-9", "pid-1");
        assert_eq!(body["model"], "gemini-2.5-pro");
        assert_eq!(body["project"], "proj-9");
        assert_eq!(body["user_prompt_id"], "pid-1");
        // 用户消息 → contents[0] role=user, parts[0].text
        assert_eq!(body["request"]["contents"][0]["role"], "user");
        assert_eq!(body["request"]["contents"][0]["parts"][0]["text"], "hi");
    }

    #[test]
    fn empty_project_is_omitted_for_free_tier() {
        // 免费/托管账号 project 为空 → 信封里不出现 project 字段（发空串会被 Code Assist 拒）。
        let body = build_request_body(&req(vec![ChatMessage::user("hi")]), "", "pid");
        assert!(
            body.get("project").is_none(),
            "空 project 应省略，实为 {body}"
        );
        assert_eq!(body["model"], "gemini-2.5-pro");
        assert_eq!(body["user_prompt_id"], "pid");
        // 非空 project 仍带上。
        let body2 = build_request_body(&req(vec![ChatMessage::user("hi")]), "proj-x", "pid");
        assert_eq!(body2["project"], "proj-x");
    }

    fn call(name: &str) -> ToolCall {
        ToolCall {
            id: format!("c-{name}"),
            name: name.into(),
            arguments: "{}".into(),
        }
    }

    #[test]
    fn thought_signature_goes_only_on_the_first_function_call() {
        // Gemini 3 规范：签名只挂本步第一个 functionCall，挂到后续的会被拒。
        let msg = ChatMessage::assistant_tool_calls(None, vec![call("a"), call("b")])
            .with_thought_signature(Some("SIG-XYZ".into()));
        let body = build_request_body(&req(vec![msg]), "p", "id");
        let parts = &body["request"]["contents"][0]["parts"];
        assert_eq!(parts[0]["thoughtSignature"], "SIG-XYZ");
        assert!(
            parts[1].get("thoughtSignature").is_none(),
            "第二个 functionCall 不该带签名: {}",
            parts[1]
        );
    }

    #[test]
    fn missing_signature_falls_back_to_the_official_placeholder() {
        // 本次改动之前落盘的历史没有签名；不兜底的话整段历史会被 400 焊死。
        let msg = ChatMessage::assistant_tool_calls(None, vec![call("remember")]);
        let body = build_request_body(&req(vec![msg]), "p", "id");
        assert_eq!(
            body["request"]["contents"][0]["parts"][0]["thoughtSignature"],
            SKIP_THOUGHT_SIGNATURE
        );
    }

    #[test]
    fn aggregator_captures_thought_signature_from_the_first_call_only() {
        let mut agg = GeminiAggregator::new();
        agg.handle(
            &json!({"candidates":[{"content":{"parts":[
                {"functionCall":{"name":"a","args":{}},"thoughtSignature":"SIG-1"},
                {"functionCall":{"name":"b","args":{}},"thoughtSignature":"SIG-2"}
            ]}}]})
            .to_string(),
        );
        let r = agg.into_response();
        assert_eq!(r.tool_calls.len(), 2);
        assert_eq!(r.thought_signature.as_deref(), Some("SIG-1"), "只取第一个");
    }

    #[test]
    fn aggregator_tolerates_signature_nested_in_function_call() {
        // 个别版本把签名放在 functionCall 内部，两处都得认。
        let mut agg = GeminiAggregator::new();
        agg.handle(
            &json!({"candidates":[{"content":{"parts":[
                {"functionCall":{"name":"a","args":{},"thoughtSignature":"NESTED"}}
            ]}}]})
            .to_string(),
        );
        assert_eq!(
            agg.into_response().thought_signature.as_deref(),
            Some("NESTED")
        );
    }

    #[test]
    fn system_goes_to_system_instruction_not_contents() {
        let body = build_request_body(
            &req(vec![
                ChatMessage::system("be brief"),
                ChatMessage::user("hi"),
            ]),
            "p",
            "id",
        );
        assert_eq!(
            body["request"]["systemInstruction"]["parts"][0]["text"],
            "be brief"
        );
        // system 不进 contents；contents 只有 user。
        assert_eq!(body["request"]["contents"].as_array().unwrap().len(), 1);
        assert_eq!(body["request"]["contents"][0]["role"], "user");
    }

    #[test]
    fn assistant_role_is_model_and_tool_calls_become_function_call_parts() {
        let msg = ChatMessage::assistant_tool_calls(
            Some("thinking done".into()),
            vec![ToolCall {
                id: "c1".into(),
                name: "shell".into(),
                arguments: r#"{"cmd":"ls"}"#.into(),
            }],
        );
        let body = build_request_body(&req(vec![msg]), "p", "id");
        let c0 = &body["request"]["contents"][0];
        assert_eq!(c0["role"], "model");
        assert_eq!(c0["parts"][0]["text"], "thinking done");
        assert_eq!(c0["parts"][1]["functionCall"]["name"], "shell");
        // args 必须是对象（已从字符串解析），而非字符串。
        assert_eq!(c0["parts"][1]["functionCall"]["args"]["cmd"], "ls");
        assert!(c0["parts"][1]["functionCall"]["args"].is_object());
    }

    #[test]
    fn tool_result_maps_to_function_response_keyed_by_name() {
        // 关键：Gemini 无 call id，functionResponse.name 必须从历史里按 tool_call_id 反查函数名。
        let msgs = vec![
            ChatMessage::assistant_tool_calls(
                None,
                vec![ToolCall {
                    id: "c1".into(),
                    name: "read_file".into(),
                    arguments: "{}".into(),
                }],
            ),
            ChatMessage::tool_result("c1", "file contents"),
        ];
        let body = build_request_body(&req(msgs), "p", "id");
        // contents[1] = 工具结果，role=user，functionResponse.name = read_file（不是 c1！）
        let fr = &body["request"]["contents"][1]["parts"][0]["functionResponse"];
        assert_eq!(body["request"]["contents"][1]["role"], "user");
        assert_eq!(fr["name"], "read_file");
        assert_eq!(fr["response"]["result"], "file contents");
    }

    #[test]
    fn images_become_inline_data() {
        let msg =
            ChatMessage::user_with_images("look", vec!["data:image/png;base64,QUJD".to_string()]);
        let body = build_request_body(&req(vec![msg]), "p", "id");
        let parts = &body["request"]["contents"][0]["parts"];
        assert_eq!(parts[0]["text"], "look");
        assert_eq!(parts[1]["inlineData"]["mimeType"], "image/png");
        assert_eq!(parts[1]["inlineData"]["data"], "QUJD");
    }

    #[test]
    fn tools_become_function_declarations_with_sanitized_schema() {
        let mut r = req(vec![ChatMessage::user("go")]);
        r.tools = vec![ToolDef {
            name: "write".into(),
            description: "write a file".into(),
            parameters: json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "path": { "type": "string", "format": "uri", "minLength": 1 }
                },
                "required": ["path"]
            }),
        }];
        let body = build_request_body(&r, "p", "id");
        let decl = &body["request"]["tools"][0]["functionDeclarations"][0];
        assert_eq!(decl["name"], "write");
        // 不支持的关键字被剥掉：additionalProperties / format / minLength。
        let params = &decl["parameters"];
        assert!(params.get("additionalProperties").is_none());
        assert!(params["properties"]["path"].get("format").is_none());
        assert!(params["properties"]["path"].get("minLength").is_none());
        // 支持的保留：type / properties / required。
        assert_eq!(params["type"], "object");
        assert_eq!(params["properties"]["path"]["type"], "string");
        assert_eq!(params["required"][0], "path");
    }

    #[test]
    fn generation_config_maps_reasoning_effort() {
        let mut r = req(vec![ChatMessage::user("go")]);
        r.max_tokens = 2048;
        // 无 reasoning：仅 maxOutputTokens，不带 thinkingConfig。
        let b0 = build_request_body(&r, "p", "id");
        assert_eq!(b0["request"]["generationConfig"]["maxOutputTokens"], 2048);
        assert!(b0["request"]["generationConfig"]
            .get("thinkingConfig")
            .is_none());
        // high → 24576 token 预算 + 回传思考。
        r.reasoning_effort = Some("high".into());
        let bh = build_request_body(&r, "p", "id");
        let tc = &bh["request"]["generationConfig"]["thinkingConfig"];
        assert_eq!(tc["thinkingBudget"], 24576);
        assert_eq!(tc["includeThoughts"], true);
        // medium → 8192。
        r.reasoning_effort = Some("medium".into());
        let bm = build_request_body(&r, "p", "id");
        assert_eq!(
            bm["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            8192
        );
        // low → 1024（2.5 Pro 无法关思考，最低有效预算）。
        r.reasoning_effort = Some("low".into());
        let bl = build_request_body(&r, "p", "id");
        assert_eq!(
            bl["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            1024
        );
        assert_eq!(
            bl["request"]["generationConfig"]["thinkingConfig"]["includeThoughts"],
            true
        );
        // xhigh → -1（动态最大）。
        r.reasoning_effort = Some("xhigh".into());
        let bx = build_request_body(&r, "p", "id");
        assert_eq!(
            bx["request"]["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            -1
        );
    }

    #[test]
    fn sanitize_keeps_properties_named_like_schema_keywords() {
        // 复现真机 400：`required fields ['pattern'] are not defined in the schema properties`。
        // grep 工具有个**名叫 pattern 的参数**——它在 properties 下是「属性名」，不是 schema 关键字，
        // 绝不能当关键字剥掉，否则 required 引用到不存在的属性，Google 直接拒整个请求。
        let s = json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "正则" },
                "format":  { "type": "string" },
                "default": { "type": "string" },
                "path":    { "type": "string", "pattern": "^/" }
            },
            "required": ["pattern", "path"]
        });
        let out = sanitize_schema(&s);
        let props = &out["properties"];
        assert!(
            props.get("pattern").is_some(),
            "名为 pattern 的属性不能被剥: {out}"
        );
        assert!(
            props.get("format").is_some(),
            "名为 format 的属性不能被剥: {out}"
        );
        assert!(
            props.get("default").is_some(),
            "名为 default 的属性不能被剥: {out}"
        );
        assert_eq!(props["pattern"]["type"], "string");
        assert_eq!(props["pattern"]["description"], "正则");
        // 而属性**内部**的 pattern（真·约束关键字）仍应剥掉。
        assert!(
            props["path"].get("pattern").is_none(),
            "属性内的 pattern 约束应剥: {out}"
        );
        // required 原样保留，且其引用的属性都还在。
        assert_eq!(out["required"], json!(["pattern", "path"]));
    }

    #[test]
    fn every_real_tool_schema_survives_sanitize() {
        // 不变量（覆盖全部真实工具）：清洗后，required 里的每个字段都必须仍在 properties 里。
        // 破坏它就是 Google 的「required fields [...] are not defined in the schema properties」400
        // ——glob 工具的 `pattern` 参数就这么把整条 Gemini 推理打挂过。以后任何工具参数名撞上
        // schema 关键字，这条会当场抓住，而不是等真机 400。
        let reg = crate::tools::ToolRegistry::with_defaults(std::env::temp_dir());
        let mut checked = 0;
        for def in reg.defs() {
            let s = sanitize_schema(&def.parameters);
            let Some(req) = s.get("required").and_then(Value::as_array) else {
                continue;
            };
            let props = s.get("properties").and_then(Value::as_object);
            for name in req.iter().filter_map(Value::as_str) {
                assert!(
                    props.is_some_and(|p| p.contains_key(name)),
                    "工具 `{}` 清洗后 required 的 `{name}` 在 properties 里没了 → Gemini 会 400。清洗后 schema: {s}",
                    def.name
                );
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "应至少校验到一个带 required 的工具，否则这条测试是空转"
        );
    }

    #[test]
    fn sanitize_schema_strips_nested_unsupported_keys() {
        let s = json!({
            "type": "object",
            "$schema": "http://json-schema.org/draft-07/schema#",
            "additionalProperties": false,
            "properties": {
                "items": {
                    "type": "array",
                    "items": { "type": "string", "pattern": "^x", "format": "email" }
                }
            }
        });
        let out = sanitize_schema(&s);
        assert!(out.get("$schema").is_none());
        assert!(out.get("additionalProperties").is_none());
        // 深层数组元素里的 pattern/format 也被剥。
        let leaf = &out["properties"]["items"]["items"];
        assert!(leaf.get("pattern").is_none());
        assert!(leaf.get("format").is_none());
        assert_eq!(leaf["type"], "string");
    }

    #[test]
    fn consecutive_user_messages_are_merged_for_role_alternation() {
        // 并行工具结果 + 中途插入的补充消息都会产生连续 user——Gemini 要求交替，必须合并。
        let msgs = vec![
            ChatMessage::assistant_tool_calls(
                None,
                vec![
                    ToolCall {
                        id: "a".into(),
                        name: "f1".into(),
                        arguments: "{}".into(),
                    },
                    ToolCall {
                        id: "b".into(),
                        name: "f2".into(),
                        arguments: "{}".into(),
                    },
                ],
            ),
            ChatMessage::tool_result("a", "r1"),
            ChatMessage::tool_result("b", "r2"),
            ChatMessage::user("补充：也看看 X"),
        ];
        let body = build_request_body(&req(msgs), "p", "id");
        let contents = body["request"]["contents"].as_array().unwrap();
        // model（2 functionCall） + 合并后的单个 user（2 functionResponse + 1 text）。
        assert_eq!(contents.len(), 2, "相邻 user 应被合并成一条");
        assert_eq!(contents[0]["role"], "model");
        assert_eq!(contents[1]["role"], "user");
        let parts = contents[1]["parts"].as_array().unwrap();
        assert_eq!(
            parts.len(),
            3,
            "两条 functionResponse + 一条 text 应并到同一 user content"
        );
        assert_eq!(parts[0]["functionResponse"]["name"], "f1");
        assert_eq!(parts[1]["functionResponse"]["name"], "f2");
        assert_eq!(parts[2]["text"], "补充：也看看 X");
    }

    #[test]
    fn aggregator_unwraps_response_and_streams_text() {
        let mut agg = GeminiAggregator::new();
        // chunk 外裹 .response，内含 candidates[].content.parts[].text
        let d1 = r#"{"response":{"candidates":[{"content":{"parts":[{"text":"Hel"}]}}]}}"#;
        assert_eq!(agg.handle(d1), Some("Hel".to_string()));
        let d2 = r#"{"response":{"candidates":[{"content":{"parts":[{"text":"lo"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":2,"totalTokenCount":12}}}"#;
        assert_eq!(agg.handle(d2), Some("lo".to_string()));
        let resp = agg.into_response();
        assert_eq!(resp.content.as_deref(), Some("Hello"));
        assert_eq!(resp.finish_reason.as_deref(), Some("stop"));
        assert_eq!(resp.usage.prompt_tokens, 10);
        assert_eq!(resp.usage.completion_tokens, 2);
        assert_eq!(resp.usage.total_tokens, 12);
    }

    #[test]
    fn aggregator_separates_thought_parts_from_visible_text() {
        let mut agg = GeminiAggregator::new();
        // thought:true → 归推理，不作为可见增量返回。
        assert_eq!(
            agg.handle(r#"{"response":{"candidates":[{"content":{"parts":[{"text":"reason ","thought":true}]}}]}}"#),
            None
        );
        assert_eq!(
            agg.handle(
                r#"{"response":{"candidates":[{"content":{"parts":[{"text":"answer"}]}}]}}"#
            ),
            Some("answer".to_string())
        );
        let resp = agg.into_response();
        assert_eq!(resp.thinking.as_deref(), Some("reason "));
        assert_eq!(resp.content.as_deref(), Some("answer"));
    }

    #[test]
    fn aggregator_collects_function_calls() {
        let mut agg = GeminiAggregator::new();
        agg.handle(
            r#"{"response":{"candidates":[{"content":{"parts":[{"functionCall":{"name":"shell","args":{"cmd":"ls"}}}]}}]}}"#,
        );
        let resp = agg.into_response();
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "shell");
        // args 序列化回字符串，供 canonical ToolCall。
        let args: Value = serde_json::from_str(&resp.tool_calls[0].arguments).unwrap();
        assert_eq!(args["cmd"], "ls");
        // 有工具调用 → finish_reason=tool_calls（让 agent 去执行）。
        assert_eq!(resp.finish_reason.as_deref(), Some("tool_calls"));
        // 合成 id 非空且唯一前缀。
        assert!(resp.tool_calls[0].id.starts_with("gemini_call_"));
    }

    #[test]
    fn aggregator_buffers_chunk_split_across_data_lines() {
        let mut agg = GeminiAggregator::new();
        // 半条 JSON：不完整 → 返回 None、缓冲等待。
        assert_eq!(
            agg.handle(r#"{"response":{"candidates":[{"content":{"parts":[{"text":"hi"#),
            None
        );
        // 补齐后整体解析、吐出文本。
        assert_eq!(agg.handle(r#""}]}}]}}"#), Some("hi".to_string()));
    }
}
