//! notify 工具：把一条消息推到已配置的通知通道（飞书/企业微信/QQ-OneBot/webhook）。

use serde_json::{json, Value};

use super::{require_str, Tool, ToolResult};
use crate::notify;

pub struct Notify;

impl Tool for Notify {
    fn name(&self) -> &'static str {
        "notify"
    }
    fn description(&self) -> &'static str {
        "Push a message to a notification channel (Feishu/WeCom/QQ group/webhook). If channel is omitted, the first channel is used. \
         Available channels are pre-configured by the user."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "The text to push" },
                "channel": { "type": "string", "description": "Channel name (omit to use the first channel)" }
            },
            "required": ["message"]
        })
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "通知 {}",
            args.get("channel")
                .and_then(Value::as_str)
                .unwrap_or("默认")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let message = require_str(args, "message")?;
        let channels = notify::load_channels();
        if channels.is_empty() {
            return Err("未配置任何通知通道（用 `wisecortex channel add` 添加）".to_string());
        }
        let ch = match args.get("channel").and_then(Value::as_str) {
            Some(name) => {
                notify::find(&channels, name).ok_or_else(|| format!("未找到通道「{name}」"))?
            }
            None => channels[0].clone(),
        };
        notify::send(&ch, &message)?;
        Ok(format!("已通过通道「{}」推送", ch.name))
    }
}
