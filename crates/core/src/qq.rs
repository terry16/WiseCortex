//! QQ 官方机器人（QQ 开放平台）双向通道：WebSocket 网关接入 + REST 被动回复。
//!
//! 两步鉴权后，用 AppID+AppSecret **主动拨出**连 wss，
//! **不需要公网回调地址**——这正是开放平台只给 AppID/AppSecret、没有「服务器地址」选项的原因。
//! ⚠️ 未经真机验证：需用真实 QQ 机器人凭据联网验收，必要时按真机微调字段。
//!
//! 协议：
//!   1. POST https://bots.qq.com/app/getAppAccessToken  {appId,clientSecret} -> {access_token,expires_in}
//!   2. GET  https://api.sgroup.qq.com/gateway  (Authorization: QQBot <token>) -> {url: "wss://..."}
//!   3. wss：HELLO(op10){heartbeat_interval} -> IDENTIFY(op2){token,intents,shard}；每 interval 发 HEARTBEAT(op1)
//!   4. DISPATCH(op0) t=C2C_MESSAGE_CREATE / GROUP_AT_MESSAGE_CREATE
//!   5. 被动回复 REST：POST /v2/users/{openid}/messages | /v2/groups/{group_openid}/messages
//!      body {content,msg_type:0,msg_seq,msg_id}
//!
//! 协议层是纯函数（可单测）；真正的连接/收发驱动在 server 的 `qq_ws.rs`。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";
const API_BASE: &str = "https://api.sgroup.qq.com";

/// v1 intents：群 + 私聊（`GROUP_AND_C2C = 1<<25`），覆盖所选「私聊 + 群@」范围。
///
/// 若机器人在开放平台**未获群/私聊消息权限**，网关会以 close code 4914（intents 不足）拒绝——
/// 那是平台侧授权问题，需去 q.qq.com 申请对应权限，与本地代码无关。
/// 将来要接频道(guild)消息时再 `| (1<<30)`（PUBLIC_GUILD_MESSAGES）等。
pub const INTENTS_V1: u64 = 1 << 25;

// ── 网关 opcode（QQ 开放平台 WebSocket 网关）───────────────────────────────
pub const OP_DISPATCH: u64 = 0;
pub const OP_HEARTBEAT: u64 = 1;
pub const OP_IDENTIFY: u64 = 2;
pub const OP_RECONNECT: u64 = 7;
pub const OP_INVALID_SESSION: u64 = 9;
pub const OP_HELLO: u64 = 10;
pub const OP_HEARTBEAT_ACK: u64 = 11;

// ── 配置（凭据存 wisecortex/qq.json）─────────────────────────────────────────
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QqConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_secret: Option<String>,
    /// 启用网关连接（设置页开关，热生效：开则下次配置检查时连，关则断开）。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub enabled: bool,
}

impl QqConfig {
    pub fn is_ready(&self) -> bool {
        self.app_id.as_deref().is_some_and(|s| !s.is_empty())
            && self.app_secret.as_deref().is_some_and(|s| !s.is_empty())
    }
}

pub fn config_file() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("qq.json"))
}

pub fn load() -> QqConfig {
    config_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &QqConfig) -> std::io::Result<()> {
    let path = config_file()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "无配置目录"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(cfg)?)
}

// ── access_token（带进程内缓存）────────────────────────────────────────────
struct CachedToken {
    token: String,
    app_id: String,
    expires_at: Instant,
}

static TOKEN: Mutex<Option<CachedToken>> = Mutex::new(None);

/// 取 access_token：命中缓存且距到期 >5 分钟直接返回，否则刷新。
///
/// blocking，**不走代理**（bots.qq.com / api.sgroup.qq.com 是国内域名，直连即可）。
pub fn get_access_token(app_id: &str, secret: &str) -> Result<String, String> {
    {
        let g = TOKEN.lock().unwrap();
        if let Some(c) = g.as_ref() {
            if c.app_id == app_id && c.expires_at > Instant::now() + Duration::from_secs(300) {
                return Ok(c.token.clone());
            }
        }
    }
    let v: Value = crate::net::blocking_builder_with(None)
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?
        .post(TOKEN_URL)
        .json(&json!({ "appId": app_id, "clientSecret": secret }))
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let token = v
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("获取 access_token 失败：{v}"))?
        .to_string();
    // expires_in 可能是数字或字符串，缺省 7200。
    let ttl = v
        .get("expires_in")
        .and_then(|x| {
            x.as_u64()
                .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(7200);
    let mut g = TOKEN.lock().unwrap();
    *g = Some(CachedToken {
        token: token.clone(),
        app_id: app_id.to_string(),
        expires_at: Instant::now() + Duration::from_secs(ttl),
    });
    Ok(token)
}

/// 清缓存——鉴权失败/会话失效时调用，下次取强制刷新。
pub fn clear_token_cache() {
    *TOKEN.lock().unwrap() = None;
}

// ── 网关地址 ───────────────────────────────────────────────────────────────
/// 解析 `GET /gateway` 响应（纯函数）。
pub fn parse_gateway(v: &Value) -> Result<String, String> {
    v.get("url")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .ok_or_else(|| format!("网关地址解析失败：{v}"))
}

/// 取 wss 网关地址（blocking，不走代理）。
pub fn get_gateway_url(token: &str) -> Result<String, String> {
    let v: Value = crate::net::blocking_builder_with(None)
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?
        .get(format!("{API_BASE}/gateway"))
        .header("Authorization", format!("QQBot {token}"))
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    parse_gateway(&v)
}

// ── 握手 / 心跳 payload（纯函数）──────────────────────────────────────────
pub fn build_identify(token: &str, intents: u64) -> Value {
    json!({
        "op": OP_IDENTIFY,
        "d": { "token": format!("QQBot {token}"), "intents": intents, "shard": [0, 1] }
    })
}

pub fn build_heartbeat(last_seq: Option<u64>) -> Value {
    json!({ "op": OP_HEARTBEAT, "d": last_seq })
}

// ── 入站消息抽取 ───────────────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq)]
pub enum Scope {
    C2c,
    Group,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Inbound {
    pub scope: Scope,
    /// 回复目标：C2C 为 user_openid，群为 group_openid。
    pub peer_id: String,
    /// 收到的消息 id；被动回复必须回带它（5 分钟免费窗口、不耗主动推送配额）。
    pub msg_id: String,
    pub text: String,
}

/// 从网关帧抽取一条可回复的文本消息。
///
/// 仅认 v1 范围：`C2C_MESSAGE_CREATE`（私聊）与 `GROUP_AT_MESSAGE_CREATE`（群里被@）。
/// 非 DISPATCH 帧、其它事件、空文本一律返回 None。
/// 取字段：事件名在 `t`，负载在 `d`，文本在 `d.content`，消息 id 在 `d.id`。
pub fn extract_message(frame: &Value) -> Option<Inbound> {
    if frame.get("op").and_then(Value::as_u64) != Some(OP_DISPATCH) {
        return None;
    }
    let t = frame.get("t").and_then(Value::as_str)?;
    let d = frame.get("d")?;
    let text = d
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let msg_id = d
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if text.is_empty() {
        return None;
    }
    match t {
        "C2C_MESSAGE_CREATE" => {
            let peer = d
                .get("author")
                .and_then(|a| a.get("user_openid"))
                .and_then(Value::as_str)?
                .to_string();
            Some(Inbound {
                scope: Scope::C2c,
                peer_id: peer,
                msg_id,
                text,
            })
        }
        "GROUP_AT_MESSAGE_CREATE" => {
            let peer = d.get("group_openid").and_then(Value::as_str)?.to_string();
            Some(Inbound {
                scope: Scope::Group,
                peer_id: peer,
                msg_id,
                text,
            })
        }
        _ => None,
    }
}

// ── 被动回复 ───────────────────────────────────────────────────────────────
/// 被动文本回复 body（纯函数）。`msg_seq` 固定 1：每条入站只回一条，同一 msg_id 只用一次。
pub fn build_reply_body(text: &str, msg_id: &str) -> Value {
    json!({ "content": text, "msg_type": 0, "msg_seq": 1, "msg_id": msg_id })
}

/// 回复目标路径（纯函数）。
pub fn reply_path(m: &Inbound) -> String {
    match m.scope {
        Scope::C2c => format!("/v2/users/{}/messages", m.peer_id),
        Scope::Group => format!("/v2/groups/{}/messages", m.peer_id),
    }
}

/// POST 一条消息到 messages 接口（被动/主动共用）。blocking、不走代理。
fn post_message(token: &str, path: &str, body: &Value) -> Result<(), String> {
    let resp = crate::net::blocking_builder_with(None)
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?
        .post(format!("{API_BASE}{path}"))
        .header("Authorization", format!("QQBot {token}"))
        .json(body)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {status}: {text}"));
    }
    // 成功回 {id,timestamp}（无 code）；带 code≠0 视为业务错误。
    if let Ok(v) = serde_json::from_str::<Value>(&text) {
        if let Some(code) = v.get("code").and_then(Value::as_i64) {
            if code != 0 {
                return Err(format!("QQ 业务错误 code={code}：{text}"));
            }
        }
    }
    Ok(())
}

/// 发送被动文本回复（blocking，不走代理）。
pub fn send_reply(token: &str, m: &Inbound, text: &str) -> Result<(), String> {
    post_message(token, &reply_path(m), &build_reply_body(text, &m.msg_id))
}

// ── 主动推送（定时通知用；⚠️ 受 QQ 主动消息配额/审核限制，可能被平台拒）──────
impl Scope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scope::C2c => "c2c",
            Scope::Group => "group",
        }
    }
}

/// 主动消息 body（无 msg_id）。
pub fn build_proactive_body(text: &str) -> Value {
    json!({ "content": text, "msg_type": 0, "msg_seq": 1 })
}

fn target_path(scope: &Scope, id: &str) -> String {
    match scope {
        Scope::C2c => format!("/v2/users/{id}/messages"),
        Scope::Group => format!("/v2/groups/{id}/messages"),
    }
}

/// 解析合成目标规格 `c2c:<openid>` / `group:<group_openid>`。
pub fn parse_target(spec: &str) -> Result<(Scope, &str), String> {
    if let Some(id) = spec.strip_prefix("c2c:") {
        Ok((Scope::C2c, id))
    } else if let Some(id) = spec.strip_prefix("group:") {
        Ok((Scope::Group, id))
    } else {
        Err(format!("无法识别的 QQ 目标：{spec}"))
    }
}

/// 主动推送文本到目标（取 qq.json 凭据换 token）。供定时通知调用。
pub fn push(spec: &str, text: &str) -> Result<(), String> {
    let cfg = load();
    if !cfg.is_ready() {
        return Err("QQ 机器人未连接（先在通知通道页扫码绑定）".into());
    }
    let (scope, id) = parse_target(spec)?;
    if id.is_empty() {
        return Err("QQ 推送目标为空".into());
    }
    let token = get_access_token(
        cfg.app_id.as_deref().unwrap_or_default(),
        cfg.app_secret.as_deref().unwrap_or_default(),
    )?;
    post_message(
        &token,
        &target_path(&scope, id),
        &build_proactive_body(text),
    )
}

// ── 最近会话（供定时通知选目标；与飞书 recent_chats 同模型）────────────────
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerRef {
    /// "c2c" | "group"
    pub scope: String,
    pub id: String,
}

fn peers_file() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("qq_peers.json"))
}

/// 最近收到过消息的会话（最新在前，去重，最多 20）。供出站推送选目标。
pub fn recent_peers() -> Vec<PeerRef> {
    peers_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// 记下一个来源会话（去重置顶，最多 20）。
pub fn record_peer(scope: &str, id: &str) {
    if id.is_empty() {
        return;
    }
    let mut list = recent_peers();
    list.retain(|p| !(p.scope == scope && p.id == id));
    list.insert(
        0,
        PeerRef {
            scope: scope.to_string(),
            id: id.to_string(),
        },
    );
    list.truncate(20);
    if let Some(path) = peers_file() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(t) = serde_json::to_string_pretty(&list) {
            let _ = std::fs::write(path, t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_ready_requires_both() {
        let mut c = QqConfig::default();
        assert!(!c.is_ready());
        c.app_id = Some("1".into());
        assert!(!c.is_ready());
        c.app_secret = Some("s".into());
        assert!(c.is_ready());
        c.app_id = Some(String::new());
        assert!(!c.is_ready());
    }

    #[test]
    fn parse_gateway_reads_url() {
        let v = json!({ "url": "wss://api.sgroup.qq.com/websocket" });
        assert_eq!(
            parse_gateway(&v).unwrap(),
            "wss://api.sgroup.qq.com/websocket"
        );
        assert!(parse_gateway(&json!({ "message": "x", "code": 1 })).is_err());
    }

    #[test]
    fn identify_shape() {
        let v = build_identify("TKN", INTENTS_V1);
        assert_eq!(v["op"], 2);
        assert_eq!(v["d"]["token"], "QQBot TKN");
        assert_eq!(v["d"]["intents"], 1u64 << 25);
        assert_eq!(v["d"]["shard"], json!([0, 1]));
    }

    #[test]
    fn heartbeat_shape() {
        assert_eq!(build_heartbeat(Some(42)), json!({ "op": 1, "d": 42 }));
        assert_eq!(build_heartbeat(None), json!({ "op": 1, "d": null }));
    }

    #[test]
    fn extract_c2c_message() {
        let frame = json!({
            "op": 0, "t": "C2C_MESSAGE_CREATE", "s": 7,
            "d": { "id": "MSGID1", "content": "  hello bot ", "author": { "user_openid": "USER_A" } }
        });
        let m = extract_message(&frame).unwrap();
        assert_eq!(m.scope, Scope::C2c);
        assert_eq!(m.peer_id, "USER_A");
        assert_eq!(m.msg_id, "MSGID1");
        assert_eq!(m.text, "hello bot");
    }

    #[test]
    fn extract_group_at_message() {
        let frame = json!({
            "op": 0, "t": "GROUP_AT_MESSAGE_CREATE",
            "d": { "id": "MSGID2", "content": " /ping", "group_openid": "GRP_X",
                   "author": { "member_openid": "MEMBER_B" } }
        });
        let m = extract_message(&frame).unwrap();
        assert_eq!(m.scope, Scope::Group);
        assert_eq!(m.peer_id, "GRP_X");
        assert_eq!(m.msg_id, "MSGID2");
        assert_eq!(m.text, "/ping");
    }

    #[test]
    fn extract_ignores_non_target() {
        // 非 DISPATCH
        assert!(extract_message(&json!({ "op": 10, "d": {} })).is_none());
        // 其它事件类型
        assert!(
            extract_message(&json!({ "op": 0, "t": "READY", "d": { "session_id": "x" } }))
                .is_none()
        );
        // 空文本
        assert!(extract_message(&json!({
            "op": 0, "t": "C2C_MESSAGE_CREATE",
            "d": { "id": "1", "content": "   ", "author": { "user_openid": "u" } }
        }))
        .is_none());
    }

    #[test]
    fn reply_body_and_path() {
        let b = build_reply_body("hi", "M1");
        assert_eq!(
            b,
            json!({ "content": "hi", "msg_type": 0, "msg_seq": 1, "msg_id": "M1" })
        );
        let c2c = Inbound {
            scope: Scope::C2c,
            peer_id: "U".into(),
            msg_id: "M".into(),
            text: "t".into(),
        };
        assert_eq!(reply_path(&c2c), "/v2/users/U/messages");
        let grp = Inbound {
            scope: Scope::Group,
            peer_id: "G".into(),
            msg_id: "M".into(),
            text: "t".into(),
        };
        assert_eq!(reply_path(&grp), "/v2/groups/G/messages");
    }

    #[test]
    fn proactive_body_has_no_msg_id() {
        assert_eq!(
            build_proactive_body("hi"),
            json!({ "content": "hi", "msg_type": 0, "msg_seq": 1 })
        );
    }

    #[test]
    fn parse_target_and_scope_str() {
        let (s, id) = parse_target("c2c:OPENID_1").unwrap();
        assert_eq!(s.as_str(), "c2c");
        assert_eq!(id, "OPENID_1");
        let (s, id) = parse_target("group:GRP_9").unwrap();
        assert_eq!(s.as_str(), "group");
        assert_eq!(id, "GRP_9");
        assert!(parse_target("weird").is_err());
    }
}
