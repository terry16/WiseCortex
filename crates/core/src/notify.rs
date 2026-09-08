//! 出站通知通道：把一段文本推到飞书/企业微信/通用 webhook/QQ(OneBot)。
//!
//! 统一抽象为"往一个 URL POST 一个 JSON"，按 kind 决定 body 形状。
//! 通道配置存于数据目录 `wisecortex/channels.json`，可用 CLI 管理。
//! 双向接收（从 IM 反向对话）不在此模块范围。

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

/// 一个通知通道。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub name: String,
    /// feishu | wecom | onebot | webhook | email
    pub kind: String,
    /// 飞书/企业微信机器人 webhook、通用 URL、OneBot HTTP 基地址，或 email 的 SMTP 服务器（host[:port]）。
    pub url: String,
    /// onebot 的群号；email 的收件人（逗号分隔）等附加目标。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// email：SMTP 登录用户名。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// email：SMTP 密码 / 授权码。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// email：发件人地址（缺省用 username）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// 通道配置文件路径。
pub fn channels_file() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("channels.json"))
}

pub fn load_channels() -> Vec<Channel> {
    channels_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_channels(channels: &[Channel]) -> std::io::Result<()> {
    let path = channels_file()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "无配置目录"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(channels)?)
}

pub fn find(channels: &[Channel], name: &str) -> Option<Channel> {
    channels.iter().find(|c| c.name == name).cloned()
}

/// 把定时任务里的 `channel` 字段解析成一个可发送的通道。
///
/// 支持两类：
/// - **命名通道**：`channels.json` 里按 name 存的出站通道（原有行为）。
/// - **合成规格**（扫码连接的双向机器人直接当通知目标，不落 channels.json）：
///   - `feishu_app:<chat_id>` —— 复用飞书应用机器人推到某会话
///   - `qqbot:c2c:<openid>` / `qqbot:group:<group_openid>` —— QQ 主动推送
pub fn resolve(name: &str) -> Option<Channel> {
    if let Some(chat) = name.strip_prefix("feishu_app:") {
        return Some(Channel {
            name: name.to_string(),
            kind: "feishu_app".to_string(),
            url: String::new(),
            target: Some(chat.to_string()),
            username: None,
            password: None,
            from: None,
        });
    }
    if let Some(rest) = name.strip_prefix("qqbot:") {
        return Some(Channel {
            name: name.to_string(),
            kind: "qqbot".to_string(),
            url: String::new(),
            target: Some(rest.to_string()), // "c2c:<openid>" | "group:<gid>"
            username: None,
            password: None,
            from: None,
        });
    }
    find(&load_channels(), name)
}

/// 按 kind 构造 POST 的 (目标URL, body)。
pub fn build_request(ch: &Channel, text: &str) -> (String, serde_json::Value) {
    match ch.kind.as_str() {
        "feishu" => (
            ch.url.clone(),
            json!({ "msg_type": "text", "content": { "text": text } }),
        ),
        "wecom" => (
            ch.url.clone(),
            json!({ "msgtype": "text", "text": { "content": text } }),
        ),
        "onebot" => {
            let url = format!("{}/send_group_msg", ch.url.trim_end_matches('/'));
            let group_id = ch
                .target
                .as_deref()
                .and_then(|t| t.parse::<i64>().ok())
                .unwrap_or(0);
            (url, json!({ "group_id": group_id, "message": text }))
        }
        _ => (ch.url.clone(), json!({ "text": text })),
    }
}

/// OneBot 私聊回复（send_private_msg）。`base` 为 OneBot HTTP 基地址。
pub fn send_onebot_private(base: &str, user_id: i64, text: &str) -> Result<(), String> {
    let url = format!("{}/send_private_msg", base.trim_end_matches('/'));
    let body = json!({ "user_id": user_id, "message": text });
    post_json(&url, &body)
}

fn post_json(url: &str, body: &serde_json::Value) -> Result<(), String> {
    let client = crate::net::blocking_builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(url)
        .json(body)
        .send()
        .map_err(|e| format!("发送失败: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", resp.status()))
    }
}

/// 解析 email 通道为发送参数（纯函数，便于测试）。
/// url=`host` 或 `host:port`；port 缺省 587。from 缺省用 username。to 用 target 逗号分隔。
pub struct EmailParts {
    pub host: String,
    pub port: u16,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
}

pub fn email_parts(ch: &Channel, text: &str) -> Result<EmailParts, String> {
    let (host, port) = match ch.url.trim().rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => {
            (h.to_string(), p.parse::<u16>().unwrap_or(587))
        }
        _ => (ch.url.trim().to_string(), 587),
    };
    if host.is_empty() {
        return Err("缺少 SMTP 服务器（url=host[:port]）".to_string());
    }
    let from = ch
        .from
        .as_deref()
        .or(ch.username.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("缺少发件人 / 用户名")?
        .to_string();
    let to: Vec<String> = ch
        .target
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if to.is_empty() {
        return Err("缺少收件人（target，逗号分隔）".to_string());
    }
    // 主题取正文首行（截断），正文用全文。
    let subject = text
        .lines()
        .next()
        .map(|l| l.chars().take(78).collect::<String>())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "WiseCortex 通知".to_string());
    Ok(EmailParts {
        host,
        port,
        from,
        to,
        subject,
        body: text.to_string(),
    })
}

/// 通过 SMTP 发送邮件（端口 465=隐式 TLS，其余=STARTTLS）。
fn send_email(ch: &Channel, text: &str) -> Result<(), String> {
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{Message, SmtpTransport, Transport};

    let p = email_parts(ch, text)?;
    let mut builder = Message::builder()
        .from(p.from.parse().map_err(|e| format!("发件人地址非法: {e}"))?)
        .subject(p.subject);
    for to in &p.to {
        builder = builder.to(to
            .parse()
            .map_err(|e| format!("收件人地址非法 {to}: {e}"))?);
    }
    let email = builder
        .body(p.body)
        .map_err(|e| format!("构造邮件失败: {e}"))?;

    let transport = if p.port == 465 {
        SmtpTransport::relay(&p.host) // 隐式 TLS
    } else {
        SmtpTransport::starttls_relay(&p.host) // STARTTLS
    }
    .map_err(|e| format!("SMTP 连接失败: {e}"))?
    .port(p.port);
    let transport = match (&ch.username, &ch.password) {
        (Some(u), Some(pw)) if !u.is_empty() => {
            transport.credentials(Credentials::new(u.clone(), pw.clone()))
        }
        _ => transport,
    }
    .build();

    transport
        .send(&email)
        .map(|_| ())
        .map_err(|e| format!("邮件发送失败: {e}"))
}

/// 同步发送通知（reqwest blocking / SMTP；在异步处用 spawn_blocking 调用）。
pub fn send(ch: &Channel, text: &str) -> Result<(), String> {
    if ch.kind == "email" {
        return send_email(ch, text);
    }
    // 复用「扫码连接」的飞书应用机器人发消息（凭据取 feishu.json，target=目标 chat_id）。
    if ch.kind == "feishu_app" {
        let cfg = crate::feishu::load();
        if !cfg.is_ready() {
            return Err("飞书应用未连接（先在通知通道页扫码连接）".into());
        }
        let chat_id = ch
            .target
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("飞书应用推送需指定目标 chat_id")?;
        return crate::feishu::send_text(&cfg, chat_id, text);
    }
    // QQ 官方机器人主动推送（target = "c2c:<openid>" | "group:<gid>"）。
    // ⚠️ 受 QQ 主动消息配额/审核限制，可能被平台拒（错误会进任务日志）。
    if ch.kind == "qqbot" {
        let spec = ch
            .target
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("QQ 推送需指定目标（c2c:<openid> 或 group:<gid>）")?;
        return crate::qq::push(spec, text);
    }
    let (url, body) = build_request(ch, text);
    post_json(&url, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(kind: &str) -> Channel {
        Channel {
            name: "n".into(),
            kind: kind.into(),
            url: "https://x/hook".into(),
            target: Some("123".into()),
            username: None,
            password: None,
            from: None,
        }
    }

    #[test]
    fn feishu_and_wecom_bodies() {
        let (_, b) = build_request(&ch("feishu"), "hi");
        assert_eq!(b["msg_type"], "text");
        assert_eq!(b["content"]["text"], "hi");
        let (_, b) = build_request(&ch("wecom"), "hi");
        assert_eq!(b["text"]["content"], "hi");
    }

    #[test]
    fn onebot_targets_group_and_path() {
        let (url, b) = build_request(&ch("onebot"), "hi");
        assert!(url.ends_with("/send_group_msg"));
        assert_eq!(b["group_id"], 123);
        assert_eq!(b["message"], "hi");
    }

    #[test]
    fn generic_webhook_body() {
        let (_, b) = build_request(&ch("webhook"), "hi");
        assert_eq!(b["text"], "hi");
    }

    #[test]
    fn email_parts_parses_host_port_recipients_subject() {
        let c = Channel {
            name: "mail".into(),
            kind: "email".into(),
            url: "smtp.example.com:465".into(),
            target: Some("a@x.com, b@y.com".into()),
            username: Some("bot@x.com".into()),
            password: Some("pw".into()),
            from: None,
        };
        let p = email_parts(&c, "构建失败\n详情……").unwrap();
        assert_eq!(p.host, "smtp.example.com");
        assert_eq!(p.port, 465);
        assert_eq!(p.from, "bot@x.com"); // from 缺省用 username
        assert_eq!(p.to, vec!["a@x.com".to_string(), "b@y.com".to_string()]);
        assert_eq!(p.subject, "构建失败"); // 主题=首行
        assert!(p.body.contains("详情"));

        // 默认端口 587。
        let c2 = Channel {
            url: "smtp.example.com".into(),
            ..c.clone()
        };
        assert_eq!(email_parts(&c2, "x").unwrap().port, 587);
        // 缺收件人报错。
        let c3 = Channel { target: None, ..c };
        assert!(email_parts(&c3, "x").is_err());
    }
}
