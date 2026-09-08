//! LLM 客户端层：统一抽象 + 两种 wire format（OpenAI 兼容 / Anthropic）。
//!
//! 设计要点：内部消息统一用 OpenAI 风格的「canonical」格式，出站时按目标
//! provider 的 wire format 转换。覆盖 OpenAI / DeepSeek / Qwen / Gemini(兼容端点)
//! 走 OpenAI 格式；Claude 走 Anthropic 格式。
//!
//! 本阶段聚焦文本对话 + 工具调用 + 流式 + 用量/缓存统计；图片/vision 输入后续再加。

pub mod anthropic;
pub mod client;
pub mod compressor;
pub mod gemini;
pub mod image;
pub mod oauth;
pub mod oauth_gemini;
pub mod oauth_openai;
pub mod oauth_xai;
pub mod openai;
pub mod pricing;
pub mod providers;
pub mod responses;

pub use client::{LlmClient, LlmError, ProviderConfig, RetryKind, StreamUpdate};
pub use providers::{Provider, ThinkingMode, WireFormat};
use serde::{Deserialize, Serialize};

/// 消息角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// 一次工具调用（助手发起）。`arguments` 是 JSON 字符串（与上游一致，便于增量拼接）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// 工具定义（OpenAI function 风格；Anthropic 出站时转 input_schema）。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema（OpenAI 的 parameters / Anthropic 的 input_schema）。
    pub parameters: serde_json::Value,
}

/// canonical 消息（OpenAI 风格）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    /// 文本内容（assistant 工具调用回合可为 None）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// 助手发起的工具调用。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// 工具结果消息引用的调用 id（role == Tool 时）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// 图片输入（data URL，如 `data:image/png;base64,...`）；vision 模型可见。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    /// 是否在此消息处放置缓存断点（Anthropic cache_control: ephemeral）。运行时重算，不持久化。
    #[serde(default, skip_serializing)]
    pub cache_breakpoint: bool,
    /// Gemini 3 系的**思考签名**：模型返回工具调用时一并给出，下一轮必须原样回传，
    /// 否则报 `400 Function call is missing a thought_signature`。
    ///
    /// 按规范只有「每步第一个 functionCall」带签名，而一个 step 恰好对应一条 assistant 消息，
    /// 所以放在消息级而不是 [`ToolCall`] 级。其它厂商不产生也不使用它。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

impl ChatMessage {
    pub fn system(text: impl Into<String>) -> Self {
        Self::text(Role::System, text)
    }
    pub fn user(text: impl Into<String>) -> Self {
        Self::text(Role::User, text)
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::text(Role::Assistant, text)
    }
    /// 带图片的用户消息（vision 输入）。
    pub fn user_with_images(text: impl Into<String>, images: Vec<String>) -> Self {
        ChatMessage {
            images,
            ..Self::text(Role::User, text)
        }
    }
    /// 助手发起工具调用的回合（content 可为空）。
    ///
    /// 进历史前**规范化每个调用的 arguments**：模型/上游偶尔吐出残缺的参数 JSON
    /// （截断、坏引号、空串等），本地执行本就回退为空对象；若把残缺串塞进历史，
    /// 下一轮整段重发时会被上游 JSON 解析打回 400，且原样重试无解、反复失败。
    /// 故无法解析的 arguments 一律归一为 `"{}"`，与实际执行保持一致。
    pub fn assistant_tool_calls(content: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        let tool_calls = tool_calls
            .into_iter()
            .map(|mut tc| {
                if serde_json::from_str::<serde_json::Value>(&tc.arguments).is_err() {
                    tc.arguments = "{}".to_string();
                }
                tc
            })
            .collect();
        ChatMessage {
            role: Role::Assistant,
            content,
            tool_calls,
            tool_call_id: None,
            images: Vec::new(),
            cache_breakpoint: false,
            thought_signature: None,
        }
    }
    /// 附上 Gemini 的思考签名（其它厂商传 None 即可，不产生任何影响）。
    pub fn with_thought_signature(mut self, sig: Option<String>) -> Self {
        self.thought_signature = sig;
        self
    }
    pub fn text(role: Role, text: impl Into<String>) -> Self {
        ChatMessage {
            role,
            content: Some(text.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
            images: Vec::new(),
            cache_breakpoint: false,
            thought_signature: None,
        }
    }
    /// 工具结果消息。
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        ChatMessage {
            role: Role::Tool,
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
            images: Vec::new(),
            cache_breakpoint: false,
            thought_signature: None,
        }
    }
    pub fn with_cache_breakpoint(mut self) -> Self {
        self.cache_breakpoint = true;
        self
    }
}

/// 用量统计（归一为 OpenAI 风格，供成本/缓存计算）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    /// true 时 total_tokens 已是「本回合新增」而非累计（Anthropic 约定）。
    pub total_is_per_turn: bool,
}

/// 一次 LLM 调用请求。
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDef>,
    pub max_tokens: u32,
    /// 推理强度（low/medium/high）。
    pub reasoning_effort: Option<String>,
    /// 是否启用 prompt 缓存标记。
    pub caching_enabled: bool,
}

impl LlmRequest {
    pub fn new(model: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        LlmRequest {
            model: model.into(),
            messages,
            tools: Vec::new(),
            max_tokens: 4096,
            reasoning_effort: None,
            caching_enabled: false,
        }
    }
}

/// 一次 LLM 调用的归一结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LlmResponse {
    pub content: Option<String>,
    /// 模型的思考/推理文本（extended thinking）。None=无思考或提供商不回传。仅用于展示，不回灌历史。
    pub thinking: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: Option<String>,
    pub usage: Usage,
    /// 从「文本里的 `<invoke …>`」抢救出来的工具调用数量（模型偶发把工具调用当普通文本吐出，
    /// 而非走结构化通道）。>0 表示 tool_calls 里至少有这么多是文本抢救来的，UI 可据此提示。
    pub recovered_text_tool_calls: usize,
    /// 文本里出现了工具调用标记、但一个也解析不出来（残缺/畸形）。此时 tool_calls 仍为空，
    /// 本轮会停；UI 应明确提示「模型把工具调用写成了文本但无法解析」，而非静默停。
    pub had_unparsed_tool_markup: bool,
    /// Gemini 3 系的思考签名（见 [`ChatMessage::thought_signature`]）。回灌历史时必须原样带回。
    pub thought_signature: Option<String>,
}

/// `recover_text_tool_calls` 的结果：抢救出的调用 + 剥掉标记后的干净文本 + 是否见过标记。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecoveredToolCalls {
    pub calls: Vec<ToolCall>,
    pub cleaned: String,
    pub had_markup: bool,
}

/// 工具调用标记的起始 token（容错匹配，兼容 `antml:` 命名空间前缀）。
/// 第 3 个是「缺前导 `<`」变体：原生工具调用作为文本泄漏时，首个 invoke 标签的 `<` 常被上游吃掉
/// （实测形如 `antml:invoke name=...`，其后 parameter / 闭合标签仍带 `<`），否则不匹配 → 静默停。
const INVOKE_MARKERS: [&str; 3] = [
    "<invoke name=",
    "<\u{0061}ntml:invoke name=",
    "\u{0061}ntml:invoke name=",
];

/// 把一段助手文本里「被写成文本的工具调用」抢救成结构化 `ToolCall`。
///
/// 背景：Claude（尤其长上下文 / 订阅 OAuth 路径）偶发把工具调用写成原生
/// `<invoke name="..."><parameter name="...">值</parameter></invoke>` 文本，而不走结构化
/// tool_use 通道。WiseCortex 解析时它落进 content、tool_calls 为空 → agent 循环判「无工具调用即
/// 结束」→ 静默停住、且原始标记回灌历史会让模型照着学（污染滚雪球）。此函数做容错抢救：
/// - 解析出每个 invoke 的 name 与各 parameter（值原样保留为字符串，仅纯数字/true/false/null
///   且单行时强转标量；JSON 形/多行内容一律当字符串，避免写文件内容被解析掉）；
/// - `cleaned`：剥掉所有 invoke 块（及残缺标记其后内容）后的纯文本，供展示与回灌（不污染）；
/// - `had_markup`：是否见到过 invoke 标记（即便一个也没解析出来，也为 true，供上层提示/中止）。
///
/// 非严格 XML 解析：参数值里可能含 `<`/`>`/引号/代码，故按 `</parameter>`、`</invoke>` 的
/// 首次出现就近切分（与上游同源的容错策略）。
///
/// 合成的 tool_use id 进程内全局唯一（见下方自增计数器），避免每轮恢复都从 0 编号撞车。
static TEXT_CALL_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn recover_text_tool_calls(content: &str) -> RecoveredToolCalls {
    let find_marker = |from: usize| -> Option<(usize, usize)> {
        // 返回最靠前的 invoke 标记 (起点, 标记长度)。
        INVOKE_MARKERS
            .iter()
            .filter_map(|m| content[from..].find(m).map(|i| (from + i, m.len())))
            .min_by_key(|(pos, _)| *pos)
    };

    let mut had_markup = false;
    let mut calls: Vec<ToolCall> = Vec::new();
    let mut cleaned = String::new();
    let mut cursor = 0usize;

    while let Some((p, mlen)) = find_marker(cursor) {
        had_markup = true;
        // 标记之前的散文照搬。
        cleaned.push_str(&content[cursor..p]);

        // 开标签结束的 '>'。
        let Some(gt_rel) = content[p..].find('>') else {
            break; // 残缺：丢弃其后
        };
        let gt = p + gt_rel;
        let open_tag = &content[p + mlen..gt]; // name="..." 部分
        let name = extract_attr(open_tag).unwrap_or_default();

        // 匹配的 </invoke>（兼容命名空间）。
        let Some(end_rel) = find_close(&content[gt + 1..], "invoke") else {
            break; // 残缺：丢弃其后
        };
        let inner = &content[gt + 1..gt + 1 + end_rel.0];
        let block_end = gt + 1 + end_rel.1;

        if !name.is_empty() {
            let mut args = serde_json::Map::new();
            let mut pc = 0usize;
            while let Some((pp, pmlen)) = find_param(inner, pc) {
                let Some(pgt_rel) = inner[pp..].find('>') else {
                    break;
                };
                let pgt = pp + pgt_rel;
                let pname = extract_attr(&inner[pp + pmlen..pgt]).unwrap_or_default();
                let Some(pend) = find_close(&inner[pgt + 1..], "parameter") else {
                    break;
                };
                let value = &inner[pgt + 1..pgt + 1 + pend.0];
                if !pname.is_empty() {
                    args.insert(pname, coerce_param(value));
                }
                pc = pgt + 1 + pend.1;
            }
            calls.push(ToolCall {
                // 进程内全局自增，跨「每次恢复」唯一——避免每轮都从 0 编号导致 text_call_0 跨回合
                // 撞车（历史里 tool_use id 重复 → 上游配对错乱、400）。client 侧规范化再兜一层。
                id: format!(
                    "text_call_{}",
                    TEXT_CALL_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ),
                name,
                arguments: serde_json::Value::Object(args).to_string(),
            });
        }
        cursor = block_end;
    }

    // 没有残缺中断（即遍历到底）时把剩余散文补上；残缺中断时 break 已让剩余被丢弃。
    if cursor <= content.len() && find_marker(cursor).is_none() {
        cleaned.push_str(&content[cursor..]);
    }

    // 抹掉游离的 <function_calls> / </function_calls> 包裹标记。
    for tag in ["<function_calls>", "</function_calls>"] {
        if cleaned.contains(tag) {
            cleaned = cleaned.replace(tag, "");
        }
    }
    let cleaned = cleaned.trim().to_string();

    RecoveredToolCalls {
        calls,
        cleaned,
        had_markup,
    }
}

/// 取片段里第一个双引号字符串的内容（标记已消费掉 `name=`，余下形如 `"xxx">`）。
fn extract_attr(s: &str) -> Option<String> {
    let i = s.find('"')? + 1;
    let rest = &s[i..];
    let j = rest.find('"')?;
    Some(rest[..j].to_string())
}

/// 在 s 中找下一个 `<parameter name=`（兼容 antml: 前缀），返回 (起点, 标记长度)。
fn find_param(s: &str, from: usize) -> Option<(usize, usize)> {
    const MARKERS: [&str; 2] = ["<parameter name=", "<\u{0061}ntml:parameter name="];
    MARKERS
        .iter()
        .filter_map(|m| s[from..].find(m).map(|i| (from + i, m.len())))
        .min_by_key(|(pos, _)| *pos)
}

/// 在 s 中找 `</tag>`（兼容 `</tag>`），返回 (闭标签起点, 闭标签之后的偏移)。
fn find_close(s: &str, tag: &str) -> Option<(usize, usize)> {
    let plain = format!("</{tag}>");
    let ns = format!("</\u{0061}ntml:{tag}>");
    [plain, ns]
        .iter()
        .filter_map(|c| s.find(c.as_str()).map(|i| (i, i + c.len())))
        .min_by_key(|(pos, _)| *pos)
}

/// 参数值取值策略：单行且整体能解析成 number/bool/null 时取标量；否则原样当字符串
/// （路径、代码、JSON 形内容、多行文本都保字符串，避免写文件内容被「聪明地」解析掉）。
fn coerce_param(raw: &str) -> serde_json::Value {
    let t = raw.trim();
    if !t.is_empty() && !raw.contains('\n') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
            if v.is_number() || v.is_boolean() || v.is_null() {
                return v;
            }
        }
    }
    serde_json::Value::String(raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tc(args: &str) -> ToolCall {
        ToolCall {
            id: "c1".into(),
            name: "do".into(),
            arguments: args.into(),
        }
    }

    #[test]
    fn assistant_tool_calls_normalizes_broken_arguments() {
        // 合法 JSON：原样保留（含含义、不重格式化）。
        let m = ChatMessage::assistant_tool_calls(None, vec![tc(r#"{"x":1}"#)]);
        assert_eq!(m.tool_calls[0].arguments, r#"{"x":1}"#);

        // 截断 / 坏引号 / 空串：归一为 "{}"，与本地执行回退一致，避免污染历史触发上游 400。
        let truncated = ChatMessage::assistant_tool_calls(None, vec![tc(r#"{"x":1"#)]);
        assert_eq!(truncated.tool_calls[0].arguments, "{}");
        let empty = ChatMessage::assistant_tool_calls(None, vec![tc("")]);
        assert_eq!(empty.tool_calls[0].arguments, "{}");
        // id/name 不受影响。
        assert_eq!(truncated.tool_calls[0].id, "c1");
        assert_eq!(truncated.tool_calls[0].name, "do");
    }

    #[test]
    fn recover_extracts_single_text_tool_call() {
        let r = recover_text_tool_calls(
            "好的\n<invoke name=\"git\">\n<parameter name=\"args\">status</parameter>\n</invoke>",
        );
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.calls[0].name, "git");
        assert_eq!(r.calls[0].arguments, r#"{"args":"status"}"#);
        assert_eq!(r.cleaned, "好的");
        assert!(r.had_markup);
    }

    #[test]
    fn recover_handles_invoke_marker_missing_leading_angle() {
        // 真实泄漏形态（订阅路径）：原生工具调用作为文本泄漏时，首个 invoke 标签的前导 '<' 被上游
        // 吃掉，变成 `antml:invoke name=...`（其后的 parameter / 闭合标签仍带 '<'）。须照样抢救，
        // 否则标记不匹配 → 落进 content、tool_calls 空 → agent 静默停。
        let input = "call\n\nantml:invoke name=\"read_file\">\n\
                     <parameter name=\"limit\">45</parameter>\n\
                     <parameter name=\"offset\">340</parameter>\n\
                     <parameter name=\"path\">C:\\tmp\\a.cpp</parameter>\n</invoke>";
        let r = recover_text_tool_calls(input);
        assert!(r.had_markup, "应识别出缺前导 '<' 的 antml:invoke 标记");
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.calls[0].name, "read_file");
        let args: serde_json::Value = serde_json::from_str(&r.calls[0].arguments).unwrap();
        assert_eq!(args["limit"], 45);
        assert_eq!(args["offset"], 340);
        assert_eq!(args["path"], "C:\\tmp\\a.cpp");
    }

    #[test]
    fn recover_synthesizes_globally_unique_ids() {
        // 每轮恢复都从 0 编号会让多轮的 text_call_0 撞车（历史里 tool_use id 重复 → 上游配对
        // 错乱、400 焊死）。同一文本调用连发两次，合成 id 必须不同。
        let input = "<invoke name=\"git\"><parameter name=\"args\">status</parameter></invoke>";
        let a = recover_text_tool_calls(input);
        let b = recover_text_tool_calls(input);
        assert_eq!(a.calls.len(), 1);
        assert_eq!(b.calls.len(), 1);
        assert_ne!(a.calls[0].id, b.calls[0].id, "跨调用的恢复 id 应全局唯一");
    }

    #[test]
    fn recover_handles_multiple_calls_and_numeric_coercion() {
        let r = recover_text_tool_calls(
            "<invoke name=\"a\"><parameter name=\"x\">1</parameter></invoke>\
             <invoke name=\"b\"><parameter name=\"y\">hi</parameter></invoke>",
        );
        assert_eq!(r.calls.len(), 2);
        assert_eq!(r.calls[0].name, "a");
        assert_eq!(r.calls[0].arguments, r#"{"x":1}"#); // 纯数字 → number
        assert_eq!(r.calls[1].name, "b");
        assert_eq!(r.calls[1].arguments, r#"{"y":"hi"}"#); // 非标量 → string
    }

    #[test]
    fn recover_keeps_multiline_code_param_verbatim() {
        let code = "// c\nint main(){ if(a<b) return 0; }\n";
        let input = format!(
            "<invoke name=\"write_file\"><parameter name=\"content\">{code}</parameter></invoke>"
        );
        let r = recover_text_tool_calls(&input);
        assert_eq!(r.calls.len(), 1);
        let args: serde_json::Value = serde_json::from_str(&r.calls[0].arguments).unwrap();
        // 原样保留：含 '<'、换行、尾部换行，不被强转。
        assert_eq!(args["content"], code);
    }

    #[test]
    fn recover_keeps_json_looking_content_as_string() {
        // 写文件内容本身是 JSON 时，必须当字符串，绝不能被解析成对象丢内容。
        let input =
            "<invoke name=\"write_file\"><parameter name=\"content\">{\"a\":1}</parameter></invoke>";
        let r = recover_text_tool_calls(input);
        let args: serde_json::Value = serde_json::from_str(&r.calls[0].arguments).unwrap();
        assert!(args["content"].is_string());
        assert_eq!(args["content"], "{\"a\":1}");
    }

    #[test]
    fn recover_no_markup_is_noop() {
        let r = recover_text_tool_calls("just a normal answer, no tools here");
        assert!(r.calls.is_empty());
        assert!(!r.had_markup);
        assert_eq!(r.cleaned, "just a normal answer, no tools here");
    }

    #[test]
    fn recover_dangling_markup_flagged_unrecoverable_and_stripped() {
        // 残缺标记（无闭合 </invoke>）：判定为「见到标记但解析不出」，且把残缺标记连同其后
        // 内容剥掉，防止原样回灌历史给模型当范例（污染源）。
        let r = recover_text_tool_calls("prefix text <invoke name=\"x\">\n<parameter name=\"p\">v");
        assert!(r.had_markup);
        assert!(r.calls.is_empty());
        assert_eq!(r.cleaned, "prefix text");
    }
}
