//! remember：把需长期记住的要点写入记忆（见 [`crate::memory`]）。
//! 记忆会在后续每一轮持续注入给模型，即使上下文被压缩或会话重开也不丢失。
//!
//! 两个维度由模型自己选：
//! - `scope`：`project`（同工作目录所有会话共享，**默认**）/ `session`（只属这次对话）。
//! - `kind`：`lesson`（踩过的坑/用户的纠正，优先级最高）/ `fact`（决定、约束、进展、偏好）。

use std::path::PathBuf;

use serde_json::{json, Value};

use super::{require_str, Tool, ToolResult};
use crate::memory::{Kind, Scope};

/// 记录记忆的工具（绑定某会话 id 与工作目录）。
pub struct Remember {
    sid: String,
    workdir: PathBuf,
}

impl Remember {
    pub fn new(sid: impl Into<String>, workdir: impl Into<PathBuf>) -> Self {
        Self {
            sid: sid.into(),
            workdir: workdir.into(),
        }
    }
}

impl Tool for Remember {
    fn name(&self) -> &'static str {
        "remember"
    }
    fn description(&self) -> &'static str {
        "Record something worth remembering long-term. What you record is injected back on every \
         subsequent turn, surviving context compaction, new sessions, and restarts.\n\
         kind=lesson for a mistake you made or a correction the user gave you — anything of the form \
         \"don't do X again\" / \"X turned out to be wrong\". RECORD THESE EAGERLY: having to repeat the \
         same correction is the single most expensive thing for the user. kind=fact (default) for \
         decisions, constraints, progress, and preferences.\n\
         scope=project (default) shares it with every session in this working directory — use this for \
         almost everything. scope=session keeps it to this conversation only; use it just for things \
         meaningless outside this conversation.\n\
         Write ONE short conclusion per call (under ~130 characters) — the takeaway, not the \
         investigation. Longer entries are truncated, and bloated entries push older memories out. \
         Skip anything trivially re-derivable from the code or history. \
         mode=append (default) or replace (rewrites that kind's section)."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "note": { "type": "string", "description": "One short entry — the conclusion, under ~130 characters" },
                "kind": {
                    "type": "string",
                    "enum": ["lesson", "fact"],
                    "description": "lesson = a mistake/correction not to repeat (record eagerly); fact = decision, constraint, progress, preference (default)"
                },
                "scope": {
                    "type": "string",
                    "enum": ["project", "session"],
                    "description": "project = shared by all sessions in this working directory (default); session = this conversation only"
                },
                "mode": { "type": "string", "enum": ["append", "replace"], "description": "append = add an entry (default); replace = rewrite that kind's section" }
            },
            "required": ["note"]
        })
    }
    fn summary(&self, args: &Value) -> String {
        let kind = match Kind::parse(args.get("kind").and_then(Value::as_str).unwrap_or("")) {
            Kind::Lesson => "坑",
            Kind::Fact => "记录",
        };
        let scope = if is_session_scope(args) {
            "本会话"
        } else {
            "本项目"
        };
        format!("记忆({scope}·{kind})")
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let note = require_str(args, "note")?;
        let kind = Kind::parse(args.get("kind").and_then(Value::as_str).unwrap_or(""));
        let mode = args.get("mode").and_then(Value::as_str).unwrap_or("append");
        // 默认写项目级——只写会话级的话，用户一开新对话就得重新交代一遍。
        let scope = if is_session_scope(args) {
            Scope::Session(&self.sid)
        } else {
            Scope::Project(&self.workdir)
        };
        let existing = crate::memory::read_scope(scope);
        let next = crate::memory::compose(&existing, &note, kind, mode);
        crate::memory::write_scope(scope, &next).map_err(|e| format!("写入记忆失败: {e}"))?;
        Ok(match (scope, kind) {
            (Scope::Session(_), Kind::Lesson) => "已记入本会话记忆（坑）。",
            (Scope::Session(_), Kind::Fact) => "已记入本会话记忆。",
            (Scope::Project(_), Kind::Lesson) => "已记入项目记忆（坑），本目录所有会话共享。",
            (Scope::Project(_), Kind::Fact) => "已记入项目记忆，本目录所有会话共享。",
        }
        .to_string())
    }
}

/// 是否显式要求只写本会话。默认（缺省/非法值）一律走项目级。
fn is_session_scope(args: &Value) -> bool {
    args.get("scope")
        .and_then(Value::as_str)
        .map(|s| s.trim().eq_ignore_ascii_case("session"))
        .unwrap_or(false)
}
