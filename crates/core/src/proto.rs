//! WiseCortex WebSocket 协议的 serde 模型。
//!
//! 是前后端的契约，严格对应 `docs/protocols/ws-protocol.md`（v0）。
//! 与 TS 侧 `web/src/ws-dispatcher.ts` 一一对应。
//!
//! 约定：所有消息都有 `type` 标签（serde 内部标签 + snake_case）。服务端事件大多带
//! `session_id`；少数全局事件（`pong` / `server_stop`）不带。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// 客户端 → 服务端消息（协议 §2，共 7 类）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// 绑定连接到 session。
    Subscribe { session_id: String },
    /// 用户发消息。`session_id` 缺省取连接当前绑定的。
    Message {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        content: String,
        /// 文件附件（任意对象 `{data_url,name,mime_type,...}`）。
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<Value>,
        /// 兼容字段：data_url 数组，服务端会规整进 `files`。
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<String>,
        /// 工作目录：本轮 agent 的文件/shell 操作以此为根（缺省=服务端启动目录）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    /// 回应 `request_confirmation`。
    Confirmation {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        id: String,
        result: String,
    },
    /// 中断当前任务。
    Interrupt {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// 重试上一轮：LLM 响应失败后，对**现有历史**（末尾是那条没拿到回复的用户消息）重跑一遍
    /// agent 循环，不再追加新的用户消息（避免重复气泡）。
    Retry {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// 拉取最新 session 列表。
    ListSessions,
    /// 订阅完成后触发 agent 执行 pending 任务。
    RunTask {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    /// 心跳。
    Ping,
}

/// Session 对象（用于 `session_list` / `session_update` 快照 / `session_restored`）。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tasks: Option<u64>,
    /// 最近活跃时间（epoch 毫秒），每收到一条消息就刷新。会话列表按它倒序排，
    /// `/sessions` 的序号依赖这个顺序。None=从未活跃过（排在最后，按 id 兜底）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    /// 其余字段透传（working_dir / latest_latency / created_at 等）。
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// 任务级配置：随 session 持久化。覆盖全局默认（模型/技能/auto-approve/工作目录）。
/// 在任务首条消息发出前由前端绑定，之后冻结。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TaskConfig {
    /// 任务工作目录；空=用全局工作空间。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    /// 覆盖全局 active_llm 的模型 id；None=用全局。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// 钉选的技能名；空=自动选择（现行全量行为）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    /// 任务级 auto-approve（solo/无人值守）；None=用全局。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_approve: Option<bool>,
    /// 计划模式：开启后只读探索 + 产出计划，禁止改动（写/改/shell/MCP/后台任务）直到关闭。
    #[serde(default)]
    pub plan_mode: bool,
    /// 本会话（聊天）的推理强度覆盖：None=跟随模型档/全局；Some("")=本会话关闭；
    /// Some("low".."max")=本会话按此档。优先级高于模型档与全局（定时任务不读此项，仍用模型档默认）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

/// 缓存统计（`complete.cache_stats`），前端据此算命中率。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CacheStats {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_requests: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_hit_requests: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
}

/// 服务端 → 客户端事件（协议 §3）。内部标签 `type` + snake_case。
///
/// 注意：`SessionUpdate` 用可选字段同时覆盖两种形态——
///   形态①携带 `session`（完整对象），形态②携带顶层 `cost/tasks/status/latency`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    // ── Session 生命周期 ──────────────────────────────────────────────────
    Subscribed {
        session_id: String,
    },
    SessionList {
        sessions: Vec<Session>,
        has_more: bool,
        #[serde(default)]
        cron_count: u64,
    },
    SessionUpdate {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        /// 形态①：完整 session 对象。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<Session>,
        /// 形态②：实时增量。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tasks: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        latency: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost_source: Option<String>,
    },
    SessionRenamed {
        session_id: String,
        name: String,
    },
    SessionDeleted {
        session_id: String,
    },
    SessionRestored {
        session: Session,
    },

    // ── 对话消息 ──────────────────────────────────────────────────────────
    HistoryUserMessage {
        session_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        created_at: Option<Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<String>,
    },
    AssistantMessage {
        session_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<Value>,
    },
    /// 助手文本的流式增量（逐字渲染）；最终仍发一条 assistant_message 定稿。
    AssistantDelta {
        session_id: String,
        delta: String,
    },
    /// 模型的思考/推理文本（extended thinking）：思考结束后留一条可折叠块在聊天区，
    /// 避免转录里只剩工具调用。仅用于展示，不回灌历史。
    AssistantThinking {
        session_id: String,
        content: String,
    },
    ToolCall {
        session_id: String,
        name: String,
        args: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    ToolResult {
        session_id: String,
        result: Value,
    },
    ToolStdout {
        session_id: String,
        lines: Vec<String>,
    },
    ToolError {
        session_id: String,
        error: String,
    },
    TokenUsage {
        session_id: String,
        #[serde(flatten)]
        data: Map<String, Value>,
    },
    Progress {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        progress_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        started_at: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        elapsed: Option<f64>,
    },
    Complete {
        session_id: String,
        iterations: u64,
        cost: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_stats: Option<CacheStats>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        awaiting_user_feedback: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost_source: Option<String>,
    },
    Interrupted {
        session_id: String,
    },
    /// 会话工作中又收到用户消息：该消息已排队，将在当前回合结束后自动处理（供前端给出可见反馈）。
    MessageQueued {
        session_id: String,
    },
    /// 当日成本总额（服务端账本，交互对话 + 定时任务都计入）。
    /// 连接建立时推一次初值，之后每次成本变动广播一次，前端据此刷新「今日成本」。
    CostUpdate {
        cost_today: f64,
    },
    Output {
        session_id: String,
        content: String,
    },

    // ── 文件 / shell 预览 ─────────────────────────────────────────────────
    FilePreview {
        session_id: String,
        path: String,
        operation: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_new_file: Option<bool>,
    },
    FileError {
        session_id: String,
        error: String,
    },
    ShellPreview {
        session_id: String,
        command: String,
    },
    Diff {
        session_id: String,
        old_size: u64,
        new_size: u64,
    },

    // ── 阻塞式交互 ────────────────────────────────────────────────────────
    RequestConfirmation {
        session_id: String,
        id: String,
        message: String,
        default: bool,
    },
    RequestFeedback {
        session_id: String,
        question: String,
        context: String,
        options: Vec<Value>,
    },

    // ── 状态消息 ──────────────────────────────────────────────────────────
    Info {
        session_id: String,
        message: String,
    },
    Warning {
        session_id: String,
        message: String,
    },
    Success {
        session_id: String,
        message: String,
    },
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        top_up_url: Option<String>,
    },
    Log {
        session_id: String,
        level: String,
        message: String,
    },
    TodoUpdate {
        session_id: String,
        todos: Vec<Value>,
    },

    // ── 全局（无 session_id） ─────────────────────────────────────────────
    Pong,
    ServerStop,
}

impl ServerEvent {
    /// 形态②：实时状态更新（仅 status）。
    pub fn status_update(session_id: impl Into<String>, status: impl Into<String>) -> Self {
        ServerEvent::SessionUpdate {
            session_id: Some(session_id.into()),
            session: None,
            cost: None,
            tasks: None,
            status: Some(status.into()),
            latency: None,
            cost_source: None,
        }
    }

    /// 形态①：携带完整 session 快照。
    pub fn snapshot_update(session: Session) -> Self {
        ServerEvent::SessionUpdate {
            session_id: None,
            session: Some(session),
            cost: None,
            tasks: None,
            status: None,
            latency: None,
            cost_source: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn type_of(v: &Value) -> &str {
        v["type"].as_str().unwrap()
    }

    #[test]
    fn deserializes_subscribe() {
        let msg: ClientMsg =
            serde_json::from_str(r#"{"type":"subscribe","session_id":"s1"}"#).unwrap();
        assert_eq!(
            msg,
            ClientMsg::Subscribe {
                session_id: "s1".into()
            }
        );
    }

    #[test]
    fn message_session_id_is_optional() {
        let msg: ClientMsg = serde_json::from_str(r#"{"type":"message","content":"hi"}"#).unwrap();
        match msg {
            ClientMsg::Message {
                session_id,
                content,
                files,
                images,
                cwd,
            } => {
                assert!(session_id.is_none());
                assert_eq!(content, "hi");
                assert!(files.is_empty() && images.is_empty());
                assert!(cwd.is_none());
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn message_accepts_images_array() {
        let msg: ClientMsg =
            serde_json::from_str(r#"{"type":"message","content":"x","images":["data:..."]}"#)
                .unwrap();
        match msg {
            ClientMsg::Message { images, .. } => assert_eq!(images, vec!["data:...".to_string()]),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn list_sessions_and_ping_are_unit_tagged() {
        assert_eq!(
            serde_json::from_str::<ClientMsg>(r#"{"type":"list_sessions"}"#).unwrap(),
            ClientMsg::ListSessions
        );
        assert_eq!(
            serde_json::from_str::<ClientMsg>(r#"{"type":"ping"}"#).unwrap(),
            ClientMsg::Ping
        );
    }

    #[test]
    fn serializes_subscribed_with_type_tag() {
        let ev = ServerEvent::Subscribed {
            session_id: "s1".into(),
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(type_of(&v), "subscribed");
        assert_eq!(v["session_id"], "s1");
    }

    #[test]
    fn session_update_shape1_carries_session_object() {
        let ev = ServerEvent::SessionUpdate {
            session_id: None,
            session: Some(Session {
                id: "s1".into(),
                status: Some("idle".into()),
                ..Default::default()
            }),
            cost: None,
            tasks: None,
            status: None,
            latency: None,
            cost_source: None,
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(type_of(&v), "session_update");
        assert_eq!(v["session"]["id"], "s1");
        assert!(v.get("cost").is_none()); // 形态②字段被省略
    }

    #[test]
    fn session_update_shape2_carries_inline_fields() {
        let ev = ServerEvent::SessionUpdate {
            session_id: Some("s1".into()),
            session: None,
            cost: Some(3.0),
            tasks: Some(2),
            status: Some("working".into()),
            latency: Some(120.0),
            cost_source: None,
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["session_id"], "s1");
        assert_eq!(v["cost"], 3.0);
        assert!(v.get("session").is_none());
    }

    #[test]
    fn complete_roundtrips_with_cache_stats() {
        let ev = ServerEvent::Complete {
            session_id: "s1".into(),
            iterations: 3,
            cost: 0.01,
            duration: None,
            cache_stats: Some(CacheStats {
                total_requests: Some(10),
                cache_hit_requests: Some(9),
                cache_read_input_tokens: Some(2000),
            }),
            awaiting_user_feedback: None,
            cost_source: Some("actual".into()),
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(type_of(&v), "complete");
        assert_eq!(v["cache_stats"]["cache_hit_requests"], 9);
        let back: ServerEvent = serde_json::from_value(v).unwrap();
        assert_eq!(back, ev);
    }

    #[test]
    fn error_without_session_id_is_global() {
        let v = serde_json::to_value(ServerEvent::Error {
            session_id: None,
            message: "boom".into(),
            code: None,
            top_up_url: None,
        })
        .unwrap();
        assert_eq!(type_of(&v), "error");
        assert!(v.get("session_id").is_none());
    }

    #[test]
    fn deserializes_real_progress_event() {
        let raw = json!({
            "type": "progress", "session_id": "s1", "message": "thinking",
            "progress_type": "thinking", "phase": "active", "status": "start",
            "started_at": 1_700_000_000_000_i64
        });
        let ev: ServerEvent = serde_json::from_value(raw).unwrap();
        match ev {
            ServerEvent::Progress {
                phase, started_at, ..
            } => {
                assert_eq!(phase.as_deref(), Some("active"));
                assert_eq!(started_at, Some(1_700_000_000_000));
            }
            _ => panic!("wrong variant"),
        }
    }
}
