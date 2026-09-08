//! 飞书（Lark）双向：接收事件订阅 + 回复消息。
//!
//! 凭据存 `wisecortex/feishu.json`（app_id / app_secret / verify_token）。
//! 接收逻辑在 server 的 REST 层；本模块提供：凭据读写、tenant_access_token 获取、发消息、
//! 从事件里抽取文本。群聊里飞书只在 @机器人 时才推事件，故无需额外前缀。

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const BASE: &str = "https://open.feishu.cn";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FeishuConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_secret: Option<String>,
    /// 事件订阅的 Verification Token（用于校验来源）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_token: Option<String>,
    /// 启用长连接（WebSocket，免公网回调）。需重启服务端生效。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub long_conn: bool,
}

impl FeishuConfig {
    pub fn is_ready(&self) -> bool {
        self.app_id.is_some() && self.app_secret.is_some()
    }
}

pub fn config_file() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("feishu.json"))
}

pub fn load() -> FeishuConfig {
    config_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &FeishuConfig) -> std::io::Result<()> {
    let path = config_file()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "无配置目录"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(cfg)?)
}

fn chats_file() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("feishu_chats.json"))
}

/// 最近收到过消息的 chat_id（最新在前，去重，最多 20 个）。
/// 供「复用应用机器人」做出站推送时选目标会话——省得用户去日志里扒 chat_id。
pub fn recent_chats() -> Vec<String> {
    chats_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// 记录一个收到消息的 chat_id（最新置顶、去重、截断到 20）。best-effort，失败静默。
pub fn record_chat(chat_id: &str) {
    if chat_id.is_empty() {
        return;
    }
    let mut list = recent_chats();
    if list.first().map(|c| c == chat_id).unwrap_or(false) {
        return; // 已是最新，免重复写盘
    }
    list.retain(|c| c != chat_id);
    list.insert(0, chat_id.to_string());
    list.truncate(20);
    if let Some(path) = chats_file() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(
            path,
            serde_json::to_string_pretty(&list).unwrap_or_default(),
        );
    }
}

/// 取事件的全局唯一 id（v2 schema：header.event_id）。用于去重——飞书对未及时
/// 回执的事件会重投，回调与长连接双开时同一事件也会到达两次。
pub fn extract_event_id(event: &Value) -> Option<String> {
    event
        .get("header")
        .and_then(|h| h.get("event_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// 发送者是否为人类用户。机器人（sender_type=="app"）的消息一律不处理，
/// 防自激回环；没有 sender 字段（老 schema/异常）放行，宁可多处理不可漏消息。
pub fn sender_is_user(event: &Value) -> bool {
    event
        .get("event")
        .and_then(|e| e.get("sender"))
        .and_then(|s| s.get("sender_type"))
        .and_then(Value::as_str)
        .is_none_or(|t| t == "user")
}

/// 有界去重器：记住最近 cap 个 id，`seen` 返回该 id 是否已出现过（并记录它）。
/// 空 id 不参与去重（异常 payload 不因此被吞）。
pub struct Dedup {
    cap: usize,
    order: std::collections::VecDeque<String>,
    set: std::collections::HashSet<String>,
}

impl Dedup {
    pub fn new(cap: usize) -> Self {
        Dedup {
            cap,
            order: std::collections::VecDeque::new(),
            set: std::collections::HashSet::new(),
        }
    }

    pub fn seen(&mut self, id: &str) -> bool {
        if id.is_empty() {
            return false;
        }
        if self.set.contains(id) {
            return true;
        }
        self.set.insert(id.to_string());
        self.order.push_back(id.to_string());
        if self.order.len() > self.cap {
            if let Some(old) = self.order.pop_front() {
                self.set.remove(&old);
            }
        }
        false
    }
}

/// 进程级共享去重表（REST 回调与长连接共用，容量 256——重投窗口只有分钟级，足够）。
pub fn seen_recently(event_id: &str) -> bool {
    use std::sync::{Mutex, OnceLock};
    static D: OnceLock<Mutex<Dedup>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(Dedup::new(256)))
        .lock()
        .unwrap()
        .seen(event_id)
}

/// 从事件 body 抽取 (verify_token, chat_id, text)。仅处理 text 消息。
pub fn extract_message(event: &Value) -> Option<(Option<String>, String, String)> {
    // v2 schema：token 在 header.token；事件在 event.message。
    let token = event
        .get("header")
        .and_then(|h| h.get("token"))
        .or_else(|| event.get("token"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let message = event.get("event").and_then(|e| e.get("message"))?;
    if message.get("message_type").and_then(Value::as_str) != Some("text") {
        return None;
    }
    let chat_id = message.get("chat_id").and_then(Value::as_str)?.to_string();
    // content 是一段 JSON 字符串，如 {"text":"hi"}
    let content_str = message.get("content").and_then(Value::as_str)?;
    let text = serde_json::from_str::<Value>(content_str)
        .ok()
        .and_then(|v| v.get("text").and_then(Value::as_str).map(str::to_string))?;
    Some((token, chat_id, text))
}

fn client() -> Result<reqwest::blocking::Client, String> {
    crate::net::blocking_builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

/// 获取 tenant_access_token（自建应用）。
pub fn tenant_token(cfg: &FeishuConfig) -> Result<String, String> {
    let (Some(app_id), Some(app_secret)) = (&cfg.app_id, &cfg.app_secret) else {
        return Err("未配置 app_id/app_secret".to_string());
    };
    let resp = client()?
        .post(format!(
            "{BASE}/open-apis/auth/v3/tenant_access_token/internal"
        ))
        .json(&json!({ "app_id": app_id, "app_secret": app_secret }))
        .send()
        .map_err(|e| e.to_string())?;
    let v: Value = resp.json().map_err(|e| e.to_string())?;
    v.get("tenant_access_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("取 token 失败: {v}"))
}

/// 发一条消息。`content` 按飞书要求是**序列化后的 JSON 字符串**（不是对象）。
fn send_message(
    cfg: &FeishuConfig,
    chat_id: &str,
    msg_type: &str,
    content: String,
) -> Result<(), String> {
    let token = tenant_token(cfg)?;
    let resp = client()?
        .post(format!(
            "{BASE}/open-apis/im/v1/messages?receive_id_type=chat_id"
        ))
        .bearer_auth(token)
        .json(&json!({ "receive_id": chat_id, "msg_type": msg_type, "content": content }))
        .send()
        .map_err(|e| e.to_string())?;
    let v: Value = resp.json().map_err(|e| e.to_string())?;
    if v.get("code").and_then(Value::as_i64) == Some(0) {
        Ok(())
    } else {
        Err(format!("发送失败: {v}"))
    }
}

/// 给某 chat 发文本消息。
pub fn send_text(cfg: &FeishuConfig, chat_id: &str, text: &str) -> Result<(), String> {
    send_message(cfg, chat_id, "text", json!({ "text": text }).to_string())
}

/// 给某 chat 发**互动卡片**（带按钮）。`card` 须是卡片 JSON 2.0——1.0 的按钮回调走
/// 「回传交互（旧）」，而那个不支持长连接，CGNAT 后面的机器收不到点击事件。
pub fn send_card(cfg: &FeishuConfig, chat_id: &str, card: &Value) -> Result<(), String> {
    send_message(cfg, chat_id, "interactive", card.to_string())
}

/// 卡片按钮点击回调（`card.action.trigger`）里我们关心的东西。
#[derive(Debug, Clone, PartialEq)]
pub struct CardAction {
    /// 点击发生在哪个聊天（event.context.open_chat_id）。
    pub chat_id: String,
    /// 按钮上我们自己塞的负载（event.action.value）。
    pub value: Value,
}

/// 把长连接收到的 payload 解析为卡片回调；不是卡片回调则返回 None（交回普通消息处理路径）。
/// 形态（schema 2.0）：`header.event_type = "card.action.trigger"`，
/// 自定义负载在 `event.action.value`，来源聊天在 `event.context.open_chat_id`。
pub fn parse_card_action(payload: &[u8]) -> Option<CardAction> {
    let v: Value = serde_json::from_slice(payload).ok()?;
    if v.get("header")?.get("event_type")?.as_str()? != "card.action.trigger" {
        return None;
    }
    let event = v.get("event")?;
    let chat_id = event
        .get("context")?
        .get("open_chat_id")?
        .as_str()?
        .to_string();
    let value = event.get("action")?.get("value")?.clone();
    Some(CardAction { chat_id, value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_card_action_extracts_chat_and_value() {
        // card.action.trigger 的回调体（schema 2.0）：按钮上自定义的 value 在 event.action.value，
        // 来源聊天在 event.context.open_chat_id。
        let payload = serde_json::to_vec(&json!({
            "schema": "2.0",
            "header": { "event_id": "e1", "event_type": "card.action.trigger", "app_id": "cli_x" },
            "event": {
                "operator": { "open_id": "ou_1" },
                "action": { "tag": "button", "value": { "action": "switch", "sid": "web-1" } },
                "context": { "open_chat_id": "oc_9", "open_message_id": "om_1" }
            }
        }))
        .unwrap();
        let a = parse_card_action(&payload).expect("应识别为卡片回调");
        assert_eq!(a.chat_id, "oc_9");
        assert_eq!(
            a.value.get("action").and_then(Value::as_str),
            Some("switch")
        );
        assert_eq!(a.value.get("sid").and_then(Value::as_str), Some("web-1"));
    }

    #[test]
    fn parse_card_action_ignores_non_card_events() {
        // 普通消息事件不是卡片回调 → None，交回原有的消息处理路径（别把它吃掉）。
        let msg = serde_json::to_vec(&json!({
            "header": { "event_type": "im.message.receive_v1" },
            "event": { "message": { "chat_id": "oc_1" } }
        }))
        .unwrap();
        assert!(parse_card_action(&msg).is_none());
        assert!(parse_card_action(b"not json").is_none());
    }

    #[test]
    fn extracts_event_id_from_header() {
        let ev = json!({ "header": { "event_id": "ev_abc" }, "event": {} });
        assert_eq!(extract_event_id(&ev).as_deref(), Some("ev_abc"));
        // 没有 header.event_id → None（v1 事件或异常 payload）。
        assert_eq!(extract_event_id(&json!({"event": {}})), None);
    }

    #[test]
    fn sender_is_user_filters_bot_senders() {
        let user_ev = json!({ "event": { "sender": { "sender_type": "user" } } });
        let app_ev = json!({ "event": { "sender": { "sender_type": "app" } } });
        let no_sender = json!({ "event": {} });
        assert!(sender_is_user(&user_ev));
        assert!(!sender_is_user(&app_ev), "机器人消息应被过滤，防自激回环");
        // 没有 sender 字段（老 schema/异常）→ 放行，宁可多处理不可漏消息。
        assert!(sender_is_user(&no_sender));
    }

    #[test]
    fn dedup_first_seen_false_repeat_true_with_eviction() {
        let mut d = Dedup::new(3);
        assert!(!d.seen("a"), "首次出现不算重复");
        assert!(d.seen("a"), "再次出现算重复");
        assert!(!d.seen("b"));
        assert!(!d.seen("c"));
        assert!(!d.seen("d")); // 超容量，最老的 a 被逐出
        assert!(!d.seen("a"), "被逐出后 a 应视作新事件");
        assert!(d.seen("d"), "仍在窗口内的 d 应算重复");
        // 空 id 永不算重复（异常 payload 不因此被吞）。
        assert!(!d.seen(""));
        assert!(!d.seen(""));
    }

    #[test]
    fn url_verification_and_extract() {
        // 抽取 text 消息
        let ev = json!({
            "header": { "token": "tok" },
            "event": { "message": {
                "message_type": "text",
                "chat_id": "oc_123",
                "content": "{\"text\":\"你好\"}"
            }}
        });
        let (token, chat, text) = extract_message(&ev).unwrap();
        assert_eq!(token.as_deref(), Some("tok"));
        assert_eq!(chat, "oc_123");
        assert_eq!(text, "你好");

        // 非 text 消息 → None
        let ev2 =
            json!({"event":{"message":{"message_type":"image","chat_id":"x","content":"{}"}}});
        assert!(extract_message(&ev2).is_none());
    }
}
