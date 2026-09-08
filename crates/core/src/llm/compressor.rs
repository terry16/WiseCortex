//! 上下文压缩（insert-then-compress）。
//!
//! 思路：上下文过长时，往当前对话里**插入**一条压缩指令让模型做摘要——这样能复用
//! 已缓存的 system+tools 前缀，只为新指令付费；拿到摘要后用
//! 「system + 摘要 + 最近若干轮」重建历史。
//!
//! 本模块只放纯函数（阈值判断 / 边界选择 / 摘要提取 / 历史重建），便于单测；
//! 真正的 LLM 调用编排在 server 的 agent 里。

use super::{ChatMessage, Role};

/// 压缩指令（作为临时 user 消息插入，不入库、不缓存）。
pub const COMPRESSION_PROMPT: &str = "\
═══════════════════════════════════════════════════════════════
重要：任务切换 —— 进入「记忆压缩」模式
═══════════════════════════════════════════════════════════════
上面的对话已结束。现在你处于记忆压缩模式。

严格要求：
1. 这不是对话的延续；
2. 不要回应上文中的任何请求；
3. 不要调用任何工具/函数；
4. 你的回复必须是纯文本。

你唯一的任务：为上面的对话生成一份全面的摘要。

输出格式：先输出一行 <topics> 列出 3-6 个关键主题（逗号分隔），再用 <summary> 标签包裹完整摘要。

示例：
<topics>Rails 初始化, 数据库配置, 部署流水线</topics>
<summary>
...完整摘要...
</summary>

重点涵盖：用户的明确意图、关键技术概念与代码改动、查看/修改过的文件、遇到的错误与修复、当前进度与待办。

现在开始，记住：纯文本，先 <topics> 再 <summary>。";

/// 单张图片的固定 token 估算。Anthropic 会把大图缩到 ~1.15MP，成本上限约 1600 tok/张；
/// 图片消息的 content 往往只有十几个字符，不计图会让阈值/边界对截图密集会话全盲。
pub const IMAGE_TOKENS_ESTIMATE: usize = 1_600;

/// 粗略估算消息列表的 token 数（字符数 / 4，含工具调用参数；图片按固定估算计入）。
pub fn estimate_tokens(messages: &[ChatMessage]) -> usize {
    let chars: usize = messages
        .iter()
        .map(|m| {
            let body = m.content.as_deref().map(str::len).unwrap_or(0);
            let tools: usize = m
                .tool_calls
                .iter()
                .map(|t| t.arguments.len() + t.name.len())
                .sum();
            body + tools
        })
        .sum();
    let images: usize = messages.iter().map(|m| m.images.len()).sum();
    chars.div_ceil(4) + images * IMAGE_TOKENS_ESTIMATE
}

/// 只保留最近 `keep_recent` 条「带图消息」的图片，更早的剥除并在正文标注。返回剥图的消息数。
///
/// 图片（尤其 GUI 截图）是即时观察，旧图历史价值极低，却各按兆级 base64 占请求体：
/// 攒几十张就会把请求顶过 API 体积上限（413 request_too_large）——届时连「压缩历史」
/// 的摘要调用自己都带着全量图片，同样 413，压缩静默失败，历史只增不减，会话卡死。
/// 摘要救不了字节，必须物理剥除。
pub fn prune_old_images(history: &mut [ChatMessage], keep_recent: usize) -> usize {
    let mut seen = 0usize;
    let mut pruned = 0usize;
    for m in history.iter_mut().rev() {
        if m.images.is_empty() {
            continue;
        }
        seen += 1;
        if seen <= keep_recent {
            continue;
        }
        let n = m.images.len();
        m.images = Vec::new();
        let note = format!(
            "（注：此处原附 {n} 张图片，超出保留窗口已清理；如需查看请重新截图或重新上传。）"
        );
        m.content = Some(
            match m
                .content
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(orig) => format!("{orig}\n{note}"),
                None => note,
            },
        );
        pruned += 1;
    }
    pruned
}

/// 当前回合的起点下标：**最后一条「没有工具调用的 assistant 回复」之后**。
///
/// 那样一条 assistant 消息 = 上一回合真正说完了话；它之后的所有消息（用户提问、
/// 工具调用、工具结果、截图回灌）都属于正在进行的这一回合。
/// 找不到（会话刚开始、或上一回合被中断没给出最终答复）时返回 0，即「整段都算当前回合」——
/// 保守方向：宁可多留图，也不要把用户刚发的图判成旧图剥掉。
pub fn current_turn_start(history: &[ChatMessage]) -> usize {
    history
        .iter()
        .rposition(|m| m.role == Role::Assistant && m.tool_calls.is_empty())
        .map(|i| i + 1)
        .unwrap_or(0)
}

/// 「图片只发一次」：只保留**当前回合**的图片，更早回合的就地剥掉并在正文留一行说明。
/// 返回被剥图的消息数。
///
/// 图片是上下文里最贵的东西——一张截图动辄上千 token，而默认行为是此后**每一轮都原样重发**。
/// UI 改动、跑测试这类场景，图片看过那一轮就没用了：模型的观察结论已经以文字留在历史里。
/// 少数需要反复比对同一张图的场景才需要关掉本开关（配置 `auto_trim_context`）。
///
/// 注意剥的是**调用方手里的副本**（`SessionRegistry::history` 返回克隆），不动落盘历史，
/// 所以聊天记录里的图片照常可见。
pub fn keep_images_of_current_turn_only(messages: &mut [ChatMessage]) -> usize {
    let start = current_turn_start(messages);
    let mut pruned = 0usize;
    for m in messages[..start].iter_mut() {
        if m.images.is_empty() {
            continue;
        }
        let n = m.images.len();
        m.images = Vec::new();
        let note =
            format!("（注：此处原附 {n} 张图片，已在发出的那一轮看过，不再重复带入上下文。）");
        m.content = Some(
            match m
                .content
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(orig) => format!("{orig}\n{note}"),
                None => note,
            },
        );
        pruned += 1;
    }
    pruned
}

/// 剥掉**全部**图片（不留回合）。用于压缩：摘要调用本身不需要看图，
/// 压缩后保留的近期消息也按「图片只发一次」一并丢弃。
pub fn drop_all_images(messages: &mut [ChatMessage]) -> usize {
    let mut pruned = 0usize;
    for m in messages.iter_mut() {
        if m.images.is_empty() {
            continue;
        }
        let n = m.images.len();
        m.images = Vec::new();
        let note = format!("（注：此处原附 {n} 张图片，压缩上下文时已丢弃。）");
        m.content = Some(
            match m
                .content
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(orig) => format!("{orig}\n{note}"),
                None => note,
            },
        );
        pruned += 1;
    }
    pruned
}

/// 选择压缩边界：从末尾保留约 `keep_recent_tokens` token 的最近消息，其余压缩成摘要。
/// 返回「最近段的起始下标」（在该下标之前的消息将被摘要）。
///
/// 关键：边界**吸附到安全起点**（User 或 Assistant 消息）。`Role::Tool`（tool 结果）
/// 不能作为序列起点——否则会与其前面的 assistant `tool_calls` 切开，导致孤立的
/// tool_result，模型 API 报错。这样即使在**单个自主任务内**（只有一条 user 消息、
/// 后面全是 assistant/tool）也能找到边界压缩，而不像按 user 轮次切分那样永不触发。
///
/// 全部消息都在保留预算内、或找不到可压缩前缀时返回 None。
pub fn compress_boundary(messages: &[ChatMessage], keep_recent_tokens: usize) -> Option<usize> {
    if messages.is_empty() {
        return None;
    }
    // 从末尾累计 token，直到达到保留预算，得到候选边界（保留 [idx..]）。
    let mut acc = 0usize;
    let mut idx = messages.len();
    for i in (0..messages.len()).rev() {
        acc += estimate_tokens(std::slice::from_ref(&messages[i]));
        idx = i;
        if acc >= keep_recent_tokens {
            break;
        }
    }
    // 全部消息都在预算内 → 无需压缩。
    if acc < keep_recent_tokens {
        return None;
    }
    // 吸附到安全起点：候选若是 tool 结果，向前回退到拥有它的 assistant/user。
    let mut b = idx;
    while b > 0 && messages[b].role == Role::Tool {
        b -= 1;
    }
    // 必须有可压缩前缀（b>0）且保留段非空（b<len）。
    if b == 0 || b >= messages.len() {
        return None;
    }
    Some(b)
}

/// 从模型回复中提取摘要：优先取 <summary>...</summary>，否则用整体文本。
pub fn extract_summary(text: &str) -> String {
    if let (Some(s), Some(e)) = (text.find("<summary>"), text.find("</summary>")) {
        if e > s {
            return text[s + "<summary>".len()..e].trim().to_string();
        }
    }
    text.trim().to_string()
}

/// 用「摘要 + 最近段」重建历史。`boundary` 来自 [`compress_boundary`]。
pub fn build_compressed_history(
    summary: &str,
    messages: &[ChatMessage],
    boundary: usize,
) -> Vec<ChatMessage> {
    let mut out = vec![ChatMessage::user(format!("[此前对话的摘要]\n{summary}"))];
    out.extend_from_slice(&messages[boundary..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::ToolCall;

    fn convo() -> Vec<ChatMessage> {
        vec![
            ChatMessage::user("turn A question"),
            ChatMessage::assistant("answer a"),
            ChatMessage::user("turn B question"),
            ChatMessage::assistant("answer b"),
            ChatMessage::user("turn C question"),
        ]
    }

    fn img(text: &str) -> ChatMessage {
        ChatMessage::user_with_images(text, vec!["data:image/png;base64,AAAA".to_string()])
    }

    /// 带工具调用的 assistant（回合**未**结束，不该成为回合边界）。
    fn assistant_calling(name: &str) -> ChatMessage {
        ChatMessage::assistant_tool_calls(
            None,
            vec![ToolCall {
                id: "t1".into(),
                name: name.into(),
                arguments: "{}".into(),
            }],
        )
    }

    #[test]
    fn current_turn_starts_after_the_last_finished_assistant_reply() {
        // 「没有工具调用的 assistant」= 上一回合真的说完了话，之后的都属当前回合。
        let h = vec![
            ChatMessage::user("q1"),
            ChatMessage::assistant("a1"), // 回合 1 结束
            ChatMessage::user("q2"),
            assistant_calling("read_file"), // 回合 2 进行中，不算边界
            ChatMessage::tool_result("t1", "…"),
        ];
        assert_eq!(current_turn_start(&h), 2);
        // 空历史 / 上一回合被中断（没有最终答复）→ 0，整段都算当前回合（保守，宁可多留图）。
        assert_eq!(current_turn_start(&[]), 0);
        assert_eq!(
            current_turn_start(&[ChatMessage::user("q"), assistant_calling("x")]),
            0
        );
    }

    #[test]
    fn keeps_images_of_this_turn_and_strips_earlier_ones() {
        let mut h = vec![
            img("上一轮发的截图"),
            ChatMessage::assistant("看到了"), // 回合结束
            img("这一轮发的截图"),
            assistant_calling("edit_file"),
            ChatMessage::tool_result("t1", "ok"),
            img("（以上工具返回的截图）"), // 本回合内的工具截图
        ];
        assert_eq!(
            keep_images_of_current_turn_only(&mut h),
            1,
            "只该剥掉旧的那 1 条"
        );
        assert!(h[0].images.is_empty(), "上一轮的图应被剥掉");
        assert!(
            h[0].content
                .as_deref()
                .unwrap()
                .contains("不再重复带入上下文"),
            "剥掉后要留说明，别让模型以为图从没存在过"
        );
        assert!(
            h[0].content.as_deref().unwrap().contains("上一轮发的截图"),
            "原文要保留"
        );
        // 当前回合的图一张不动——包括回合中途工具返回的截图。
        assert_eq!(h[2].images.len(), 1, "本轮用户发的图必须留着");
        assert_eq!(h[5].images.len(), 1, "本轮工具截图也必须留着");
    }

    #[test]
    fn keeping_images_is_idempotent_and_noop_without_images() {
        let mut h = convo();
        assert_eq!(
            keep_images_of_current_turn_only(&mut h),
            0,
            "没有图就不该改动"
        );
        let mut h2 = vec![
            img("旧图"),
            ChatMessage::assistant("ok"),
            ChatMessage::user("q"),
        ];
        assert_eq!(keep_images_of_current_turn_only(&mut h2), 1);
        let before = h2.clone();
        assert_eq!(
            keep_images_of_current_turn_only(&mut h2),
            0,
            "再跑一次不该重复加说明"
        );
        assert_eq!(h2, before);
    }

    #[test]
    fn drop_all_images_clears_even_the_current_turn() {
        // 压缩场景：摘要不需要看图，当前回合的也一并丢。
        let mut h = vec![img("a"), ChatMessage::assistant("ok"), img("b")];
        assert_eq!(drop_all_images(&mut h), 2);
        assert!(h.iter().all(|m| m.images.is_empty()));
        assert!(h[2]
            .content
            .as_deref()
            .unwrap()
            .contains("压缩上下文时已丢弃"));
        // 幂等。
        assert_eq!(drop_all_images(&mut h), 0);
    }

    /// 造一条 `n_tokens` 大小的消息（estimate_tokens = chars/4，故 content 长度 = n*4）。
    fn sized(role: Role, n_tokens: usize) -> ChatMessage {
        let content = "x".repeat(n_tokens * 4);
        match role {
            Role::User => ChatMessage::user(content),
            Role::Assistant => ChatMessage::assistant(content),
            Role::Tool => ChatMessage::tool_result("tid", content),
            Role::System => ChatMessage::system(content),
        }
    }

    #[test]
    fn estimate_counts_content_and_tool_args() {
        let msgs = vec![ChatMessage::assistant_tool_calls(
            Some("hi".into()),
            vec![ToolCall {
                id: "1".into(),
                name: "shell".into(),
                arguments: "{\"command\":\"ls\"}".into(),
            }],
        )];
        // ("hi"=2) + (args 17 + name 5) = 24 chars → ceil(24/4)=6
        assert_eq!(estimate_tokens(&msgs), 6);
    }

    #[test]
    fn boundary_none_when_everything_fits_budget() {
        let msgs = convo(); // 都是小消息，远不到预算
        assert_eq!(compress_boundary(&msgs, 10_000), None);
    }

    #[test]
    fn boundary_keeps_recent_token_budget() {
        // [user100, assistant100(+tc 视作 assistant), tool100, assistant100, tool100] 共 500t
        let msgs = vec![
            sized(Role::User, 100),
            sized(Role::Assistant, 100),
            sized(Role::Tool, 100),
            sized(Role::Assistant, 100),
            sized(Role::Tool, 100),
        ];
        // 预算 150t：从末尾累计 tool100 → assistant100=200≥150，候选 idx=3（assistant，安全）。
        assert_eq!(compress_boundary(&msgs, 150), Some(3));
    }

    #[test]
    fn boundary_snaps_off_tool_message() {
        // 末尾是大 tool 结果：候选会落在 tool 上，必须回退到拥有它的 assistant。
        let msgs = vec![
            sized(Role::User, 50),
            sized(Role::Assistant, 50),
            sized(Role::Tool, 50),
            sized(Role::Assistant, 50),
            sized(Role::Tool, 300),
        ];
        // 预算 200t：tool300≥200，候选 idx=4(Tool) → 回退到 idx=3(Assistant)。
        let b = compress_boundary(&msgs, 200).unwrap();
        assert_eq!(b, 3);
        assert_ne!(msgs[b].role, Role::Tool); // 边界起点不是 tool，避免孤立 tool_result
    }

    #[test]
    fn boundary_single_autonomous_turn_compresses() {
        // 单个自主任务：仅 1 条 user，后面全是 assistant/tool。旧的按-user-轮次逻辑会永不压缩。
        let mut msgs = vec![sized(Role::User, 100)];
        for _ in 0..8 {
            msgs.push(sized(Role::Assistant, 100));
            msgs.push(sized(Role::Tool, 100));
        }
        // 总量远超预算 → 必须能找到边界（>0），证明单轮内也会压缩。
        let b = compress_boundary(&msgs, 300).expect("单轮任务也应能压缩");
        assert!(b > 0 && b < msgs.len());
        assert_ne!(msgs[b].role, Role::Tool);
    }

    #[test]
    fn estimate_counts_images() {
        // 图片消息的 content 往往只有十几个字符（如「（以上工具返回的截图）」），
        // 真实成本却是兆级 base64；估算必须计入，否则阈值/边界对截图全盲。
        let msg = ChatMessage::user_with_images(
            "看图",
            vec!["data:image/png;base64,xxxx".to_string(); 2],
        );
        // "看图" = 6 字节 → ceil(6/4)=2，加 2 张图的固定估算。
        assert_eq!(estimate_tokens(&[msg]), 2 + 2 * IMAGE_TOKENS_ESTIMATE);
    }

    #[test]
    fn prune_strips_all_but_recent_image_messages() {
        let img = || vec!["data:image/png;base64,AAA".to_string()];
        let mut hist = vec![
            ChatMessage::user("q"),
            ChatMessage::user_with_images("（以上工具返回的截图）", img()), // 最旧 → 剥
            ChatMessage::assistant("看到了"),
            ChatMessage::user_with_images("（以上工具返回的截图）", img()), // 次旧 → 剥
            ChatMessage::user_with_images("对比这两张", img()),             // 保留
            ChatMessage::user_with_images("（以上工具返回的截图）", img()), // 保留
            ChatMessage::assistant("done"),
        ];
        assert_eq!(prune_old_images(&mut hist, 2), 2);
        assert!(hist[1].images.is_empty());
        assert!(hist[3].images.is_empty());
        assert!(!hist[4].images.is_empty());
        assert!(!hist[5].images.is_empty());
        // 被剥的消息正文带清理标注，模型可知图已不在。
        assert!(hist[1].content.as_deref().unwrap().contains("已清理"));
        // 保留的与纯文本消息不受影响。
        assert_eq!(hist[4].content.as_deref(), Some("对比这两张"));
        assert_eq!(hist[2].content.as_deref(), Some("看到了"));
    }

    #[test]
    fn prune_noop_within_keep_budget() {
        let img = || vec!["data:image/png;base64,AAA".to_string()];
        let mut hist = vec![
            ChatMessage::user_with_images("a", img()),
            ChatMessage::user_with_images("b", img()),
        ];
        assert_eq!(prune_old_images(&mut hist, 2), 0);
        assert!(!hist[0].images.is_empty());
        assert_eq!(hist[0].content.as_deref(), Some("a"));
    }

    #[test]
    fn prune_keep_zero_strips_everything() {
        let mut hist = vec![ChatMessage::user_with_images("a", vec!["d".into()])];
        assert_eq!(prune_old_images(&mut hist, 0), 1);
        assert!(hist[0].images.is_empty());
    }

    #[test]
    fn extract_summary_prefers_tag() {
        let t = "<topics>x</topics>\n<summary>\n  核心摘要内容  \n</summary>";
        assert_eq!(extract_summary(t), "核心摘要内容");
        // 无标签 → 整体。
        assert_eq!(extract_summary("just text"), "just text");
    }

    #[test]
    fn rebuild_prepends_summary_and_keeps_recent() {
        let msgs = convo();
        // 摘要替换 boundary 之前的部分，boundary 起保留最近段（此处显式取下标 2=turn B）。
        let rebuilt = build_compressed_history("SUMMARY", &msgs, 2);
        // [摘要, turn B, answer b, turn C]
        assert_eq!(rebuilt.len(), 4);
        assert!(rebuilt[0].content.as_deref().unwrap().contains("SUMMARY"));
        assert_eq!(rebuilt[0].role, Role::User);
        assert_eq!(rebuilt[1].content.as_deref(), Some("turn B question"));
        assert_eq!(rebuilt[3].content.as_deref(), Some("turn C question"));
    }
}
