//! IM 渠道的公共入口：把一条聊天消息接进持久会话跑一个完整回合。
//!
//! 飞书、微信 ClawBot、QQ OneBot/NapCat 走同一条路（QQ **官方**机器人目前仍是无状态的
//! `run_once`——那条线还卡在 `msg_seq` 固定为 1 和 5 分钟被动回复窗口上，另行处理）。
//! 各渠道自己负责「连接 + 收发」，会话选取、多轮上下文、压缩、任务配置、聊天命令
//! 全部在这里统一交给 `run_turn`，免得每加一个渠道就把这段微妙的绑定逻辑再抄一遍。

use std::sync::{Arc, Mutex};

use crate::agent::Agent;
use crate::registry::{ImOrigin, SessionRegistry};

/// 跑一个回合，返回 (应回给用户的文本, 可选的互动卡片)。
///
/// 会话的选取：该聊天用 `/switch` 绑定过就用绑定的那个，否则回落到默认的
/// `<channel>-<peer>`（绑定的会话被删掉时也回落——绝不能往一个不存在的会话里发消息）。
/// 同会话的并发消息由会话锁串行。
///
/// `title_prefix` 用于首次命名会话（如「飞书 · 帮我看下构建」），让侧栏一眼看出消息来源。
pub async fn run_im_turn(
    agent: &Arc<Mutex<Arc<Agent>>>,
    reg: &SessionRegistry,
    origin: ImOrigin,
    title_prefix: &str,
    text: &str,
) -> (String, Option<serde_json::Value>) {
    let sid = reg
        .active_session(&origin)
        .unwrap_or_else(|| origin.default_sid());
    if !reg.exists(&sid) {
        reg.ensure(&sid);
        let title: String = text.chars().take(16).collect();
        reg.set_name_if_empty(&sid, &format!("{title_prefix} · {title}"));
    }
    let before = reg.history(&sid).len();
    let ag = agent.lock().unwrap().clone();
    let card = ag
        .run_turn(
            reg,
            &sid,
            text.to_string(),
            vec![],
            vec![],
            None,
            Some(origin),
        )
        .await;
    let text = reply_text_since(&reg.history(&sid), before).unwrap_or_else(|| {
        "（本轮执行完毕，但没有产生文本回复。可在 WiseCortex 界面查看该会话的完整过程。）"
            .to_string()
    });
    (text, card)
}

/// 取本回合（history[before..]）内最后一条非空助手文本，作为发回 IM 的回复。
/// `before` 越界（上下文压缩可能截短历史）时回退扫全量。
pub fn reply_text_since(
    history: &[wisecortex_core::llm::ChatMessage],
    before: usize,
) -> Option<String> {
    use wisecortex_core::llm::Role;
    let slice = history.get(before..).unwrap_or(history);
    slice
        .iter()
        .rev()
        .filter(|m| m.role == Role::Assistant)
        .find_map(|m| {
            m.content
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wisecortex_core::llm::ChatMessage;

    #[test]
    fn reply_text_takes_last_assistant_message_since_turn_start() {
        let hist = vec![
            ChatMessage::user("旧消息"),
            ChatMessage::assistant("旧回复"),
            ChatMessage::user("新消息"),
            ChatMessage::assistant("第一段"),
            ChatMessage::assistant("最终回复"),
        ];
        // 本回合从 index 2 开始（前 2 条是历史）→ 取本回合内最后一条助手文本。
        assert_eq!(reply_text_since(&hist, 2).as_deref(), Some("最终回复"));
        // 回合内没有助手文本（如被拦截前就失败）→ None。
        assert_eq!(reply_text_since(&hist, 5), None);
        // 压缩可能把历史截短（before 越界）→ 回退扫全量，仍能拿到回复。
        assert_eq!(reply_text_since(&hist, 99).as_deref(), Some("最终回复"));
        // 空白内容的助手消息不算回复。
        let hist2 = vec![ChatMessage::user("q"), ChatMessage::assistant("  ")];
        assert_eq!(reply_text_since(&hist2, 0), None);
    }
}
