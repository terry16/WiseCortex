//! 微信 ClawBot（iLink 协议）：配置读写 + 收发消息客户端。
//!
//! 腾讯 2026-03 通过 OpenClaw 开放的微信**个人号** Bot API，官方名「微信 ClawBot 插件」，
//! 底层协议 iLink（智联），域名 `ilinkai.weixin.qq.com`。凭据存 `wisecortex/clawbot.json`
//! （扫码拿到，见 [`crate::clawbot_register`]）。
//!
//! 与已有三条 IM 链路的根本差别：**纯 HTTPS 长轮询**，没有 websocket、没有帧编解码、
//! 不需要公网回调。`getupdates` 挂最多 35 秒返回。也正因为不需要公网入站，
//! 它在 NAT 后面的桌面端天然可用。
//!
//! ⚠️ 一个微信号只能建一个 Bot，且 `get_updates_buf` 是**共享游标**——同一个 token
//! 在两处同时轮询，消息会被随机分走、两边各收到一半。同一时刻只能在一处开启。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// 默认接入域名。扫码成功时服务端会回一个 `baseurl`，以那个为准（可能是就近 IDC）。
pub const DEFAULT_BASE: &str = "https://ilinkai.weixin.qq.com";

/// 请求体里声明的协议版本。2.1.1 起 `CDNMedia.full_url` 与「context_token 可选」才可用。
pub const CHANNEL_VERSION: &str = "2.1.1";

/// `iLink-App-ClientVersion` 头的编码：`0x00MMNNPP`（主<<16 | 次<<8 | 修订）。
pub const fn encode_version(major: u32, minor: u32, patch: u32) -> u32 {
    (major << 16) | (minor << 8) | patch
}

/// `iLink-App-ClientVersion` 头的值。与 [`CHANNEL_VERSION`] 保持一致——
/// 声明一个比协议新的版本没意义，比它旧则可能被拒。
pub const CLIENT_VERSION: u32 = encode_version(2, 1, 1);

/// 服务端长轮询最多挂 35 秒；给 15 秒网络冗余，免得自己先超时把连接掐了。
const POLL_TIMEOUT: Duration = Duration::from_secs(50);
/// 普通请求超时。
const CALL_TIMEOUT: Duration = Duration::from_secs(15);

/// `message_type`：用户发的。
pub const MSG_TYPE_USER: i64 = 1;
/// `message_state`：流式生成中（半截消息，别处理）。
pub const MSG_STATE_GENERATING: i64 = 1;

/// `message_item.type`。
pub const ITEM_TEXT: i64 = 1;
pub const ITEM_IMAGE: i64 = 2;
pub const ITEM_VOICE: i64 = 3;
pub const ITEM_FILE: i64 = 4;
pub const ITEM_VIDEO: i64 = 5;

/// 会话超时错误码。协议要求收到后**暂停全部 API 调用 1 小时**。
pub const ERRCODE_SESSION_EXPIRED: i64 = -14;

/// `sendtyping` 的状态。
pub const TYPING_ON: i64 = 1;
pub const TYPING_OFF: i64 = 2;

/// 单条消息的字符上限。协议没公开长度限制，agent 的回复动辄上千字，
/// 与其赌服务端截断/报错，不如自己切开分条发。
pub const SEND_CHUNK_CHARS: usize = 1800;

// ── 配置 ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClawbotConfig {
    /// 扫码拿到的 bot token（Bearer）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,
    /// 扫码返回的接入域名；缺省用 [`DEFAULT_BASE`]。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// 机器人自己的 id（`xxx@im.bot`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_id: Option<String>,
    /// 绑定的微信用户 id（`ilink_user_id`），发「正在输入」要用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// 启用长轮询。开关热生效，无需重启后端。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub enabled: bool,
}

impl ClawbotConfig {
    pub fn is_ready(&self) -> bool {
        self.bot_token.as_deref().is_some_and(|t| !t.is_empty())
    }
    /// 实际使用的接入域名（去掉尾部斜杠）。
    pub fn base(&self) -> String {
        let b = self
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_BASE);
        b.trim_end_matches('/').to_string()
    }
}

pub fn config_file() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("clawbot.json"))
}

pub fn load() -> ClawbotConfig {
    config_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &ClawbotConfig) -> std::io::Result<()> {
    let path = config_file()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "无配置目录"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(cfg)?)
}

// ── 同步游标 ──────────────────────────────────────────────────────────────────

fn cursor_file() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("clawbot_cursor.json"))
}

/// 读同步游标（`get_updates_buf`）。
///
/// 必须跨重启持久化：协议里没有「只取新消息」的语义，游标丢了服务端会把窗口内的
/// 消息重投一遍——表现就是重启后 bot 把用户上次问过的问题再答一遍。
pub fn load_cursor() -> String {
    cursor_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("cursor").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default()
}

/// 写同步游标。best-effort，失败静默（下轮会重投几条，比崩了强）。
///
/// 单独一个文件、不塞进 `clawbot.json`：游标每 ≤35 秒就要更新一次，而配置文件
/// 由 UI 侧读-改-写（开关）。混在一起的话，用户刚关掉开关，轮询循环手里的旧快照
/// 又会把 `enabled=true` 写回去。
pub fn save_cursor(cursor: &str) {
    let Some(path) = cursor_file() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, json!({ "cursor": cursor }).to_string());
}

// ── context_token（按用户持久化） ─────────────────────────────────────────────

/// `context_token` 存盘位置。与游标同理**不放进 `clawbot.json`**：它每来一条消息就要
/// 更新，而配置文件由 UI 侧读-改-写。
fn token_file() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("clawbot_tokens.json"))
}

/// 读某用户最近一次拿到的 `context_token`。
pub fn load_context_token(user_id: &str) -> Option<String> {
    token_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get(user_id).and_then(Value::as_str).map(str::to_string))
        .filter(|s| !s.is_empty())
}

/// 记住某用户最新的 `context_token`。
///
/// 空值直接忽略：宁可留着上一个（可能仍然有效）也不能把它抹成空——空着发出去的回复
/// 会被微信静默丢弃，见 [`parse_send_result`]。
pub fn save_context_token(user_id: &str, token: &str) {
    let token = token.trim();
    if token.is_empty() || user_id.is_empty() {
        return;
    }
    let Some(path) = token_file() else { return };
    if load_context_token(user_id).as_deref() == Some(token) {
        return; // 没变，不必每条消息都写盘
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut all = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    all[user_id] = Value::from(token);
    let _ = std::fs::write(path, all.to_string());
}

/// 回复该带哪个 `context_token`：优先本条消息自带的，否则回落到该用户最近存下的。
///
/// 📌 更正（2026-08-17）：这里原先写着「6 条不带 token 的探针全部石沉大海，所以
/// `context_token` 是投递的硬要求」。那个归因是**错的**——那 6 条探针同样没带
/// `client_id`，真正把它们吃掉的是去重（见 [`new_client_id`]）。token 是被冤枉的。
///
/// `context_token` 到底是不是硬要求，**至今没有单独验证过**（没做过「带 client_id、
/// 不带 token」的对照）。之所以照带不误：所有实测**确认送达**的报文形状里都有它，
/// 而带上它的成本是零。真要下结论，得再跑一次单变量对照，别再靠推理。
pub fn resolve_context_token(user_id: &str, from_message: &str) -> Option<String> {
    pick_context_token(from_message, load_context_token(user_id))
}

/// [`resolve_context_token`] 的纯逻辑部分（不碰磁盘，便于测试）。
pub fn pick_context_token(from_message: &str, stored: Option<String>) -> Option<String> {
    let own = from_message.trim();
    if !own.is_empty() {
        return Some(own.to_string());
    }
    stored.filter(|s| !s.trim().is_empty())
}

// ── 请求构造 ──────────────────────────────────────────────────────────────────

/// `X-WECHAT-UIN` 头：base64(随机 u32 的十进制字符串)。每个请求重新生成，服务端用于防重放。
pub fn uin_header() -> String {
    use rand::Rng;
    let n: u32 = rand::thread_rng().gen();
    base64::engine::general_purpose::STANDARD.encode(n.to_string())
}

/// 所有请求都要带的公共头。
fn public_headers(req: reqwest::blocking::RequestBuilder) -> reqwest::blocking::RequestBuilder {
    req.header("iLink-App-Id", "bot")
        .header("iLink-App-ClientVersion", CLIENT_VERSION.to_string())
}

/// POST 请求的鉴权头（在公共头之上追加）。
fn auth_headers(
    req: reqwest::blocking::RequestBuilder,
    token: &str,
) -> reqwest::blocking::RequestBuilder {
    public_headers(req)
        .header("AuthorizationType", "ilink_bot_token")
        .header("Authorization", format!("Bearer {token}"))
        .header("X-WECHAT-UIN", uin_header())
}

fn client(timeout: Duration) -> Result<reqwest::blocking::Client, String> {
    crate::net::blocking_builder()
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())
}

/// 发一个带鉴权的 POST 并取回 JSON。`body` 会自动补上 `base_info.channel_version`。
fn post(
    cfg: &ClawbotConfig,
    path: &str,
    mut body: Value,
    timeout: Duration,
) -> Result<Value, String> {
    let token = cfg
        .bot_token
        .as_deref()
        .ok_or("未配置 bot_token（请先扫码接入）")?;
    if let Some(obj) = body.as_object_mut() {
        obj.insert(
            "base_info".to_string(),
            json!({ "channel_version": CHANNEL_VERSION }),
        );
    }
    let url = format!("{}{path}", cfg.base());
    let resp = auth_headers(client(timeout)?.post(url), token)
        .json(&body)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().map_err(|e| e.to_string())?;
    if text.trim().is_empty() {
        // sendmessage 成功时就是 200 空体。
        return if status.is_success() {
            Ok(Value::Null)
        } else {
            Err(format!("HTTP {status}（空响应体）"))
        };
    }
    let v: Value = serde_json::from_str(&text)
        .map_err(|e| format!("HTTP {status}，响应非 JSON（{e}）：{}", brief(&text)))?;
    if !status.is_success() {
        return Err(format!("HTTP {status}：{v}"));
    }
    Ok(v)
}

/// 截断一段文本用于报错，避免把整个响应体糊进日志。
fn brief(s: &str) -> String {
    let t = s.trim();
    if t.chars().count() <= 200 {
        return t.to_string();
    }
    format!("{}…", t.chars().take(200).collect::<String>())
}

// ── 收消息 ────────────────────────────────────────────────────────────────────

/// 一条收到的用户消息。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inbound {
    pub message_id: String,
    /// 发送者（`xxx@im.wechat`），当作 IM 会话的 peer。
    pub from_user_id: String,
    /// 回复时必须原样带回，用于关联到正确的会话窗口。
    pub context_token: String,
    /// 文本内容（文本项 + 语音转写，多项以换行拼接）。
    pub text: String,
    /// 本版不支持、被跳过的内容类型（中文标签，用于回给用户一句说明）。
    pub unsupported: Vec<String>,
}

/// 一次 `getupdates` 的结果。
#[derive(Debug, Clone, PartialEq)]
pub enum Fetched {
    Msgs {
        msgs: Vec<Inbound>,
        /// 新游标；None=服务端没给（保留旧的）。
        cursor: Option<String>,
    },
    /// `errcode = -14`：会话超时。协议要求暂停全部 API 调用 1 小时。
    SessionExpired,
}

/// 这条消息是不是「用户发来的、已完成的」。
///
/// 机器人自己发的（`message_type=2`）必须滤掉，否则自激回环。缺字段时放行——
/// 宁可多处理不可漏消息，与飞书 `sender_is_user` 同样的取舍。
pub fn is_actionable(msg: &Value) -> bool {
    let from_user = msg
        .get("message_type")
        .and_then(Value::as_i64)
        .is_none_or(|t| t == MSG_TYPE_USER);
    // 流式生成中的半截消息跳过，等 FINISH 那条。
    let finished = msg
        .get("message_state")
        .and_then(Value::as_i64)
        .is_none_or(|s| s != MSG_STATE_GENERATING);
    from_user && finished
}

/// 内容项类型的中文标签（用于「本版不支持」的说明）。
fn item_label(ty: i64) -> Option<&'static str> {
    match ty {
        ITEM_IMAGE => Some("图片"),
        ITEM_FILE => Some("文件"),
        ITEM_VIDEO => Some("视频"),
        _ => None,
    }
}

/// 从 `item_list` 抽出可处理的文本，以及被跳过的内容类型。
///
/// 文本项直接取；语音项取服务端给的转写（`voice_item.text`）——转写是白送的，
/// 不需要我们碰 silk 编码。图片/文件/视频本版不支持，记下类型好回一句说明，
/// 而不是静默把用户的消息吞掉。
pub fn extract_text(item_list: &Value) -> (String, Vec<String>) {
    let mut parts: Vec<String> = Vec::new();
    let mut unsupported: Vec<String> = Vec::new();
    let note = |label: &str, unsupported: &mut Vec<String>| {
        let l = label.to_string();
        if !unsupported.contains(&l) {
            unsupported.push(l);
        }
    };
    for item in item_list.as_array().map(Vec::as_slice).unwrap_or_default() {
        let ty = item.get("type").and_then(Value::as_i64).unwrap_or(0);
        match ty {
            ITEM_TEXT => {
                if let Some(t) = item
                    .get("text_item")
                    .and_then(|x| x.get("text"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    parts.push(t.to_string());
                }
            }
            ITEM_VOICE => {
                match item
                    .get("voice_item")
                    .and_then(|x| x.get("text"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    Some(t) => parts.push(t.to_string()),
                    // 服务端没转写出来（噪音/方言/太短）。别装作没收到。
                    None => note("语音（未能转写）", &mut unsupported),
                }
            }
            other => {
                if let Some(l) = item_label(other) {
                    note(l, &mut unsupported);
                }
            }
        }
    }
    (parts.join("\n"), unsupported)
}

/// 解析 `getupdates` 的响应（纯函数，便于测试）。
pub fn parse_updates(v: &Value) -> Fetched {
    if v.get("errcode").and_then(Value::as_i64) == Some(ERRCODE_SESSION_EXPIRED) {
        return Fetched::SessionExpired;
    }
    let cursor = v
        .get("get_updates_buf")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let mut msgs = Vec::new();
    for m in v
        .get("msgs")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        if !is_actionable(m) {
            continue;
        }
        let (text, unsupported) = extract_text(m.get("item_list").unwrap_or(&Value::Null));
        if text.is_empty() && unsupported.is_empty() {
            continue; // 空消息（纯系统项等），没什么可做的
        }
        msgs.push(Inbound {
            message_id: m
                .get("message_id")
                .map(|x| match x {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default(),
            from_user_id: m
                .get("from_user_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            context_token: m
                .get("context_token")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            text,
            unsupported,
        });
    }
    Fetched::Msgs { msgs, cursor }
}

/// 长轮询取新消息。服务端最多挂 35 秒；有新消息则立即返回。
pub fn get_updates(cfg: &ClawbotConfig, cursor: &str) -> Result<Fetched, String> {
    let v = post(
        cfg,
        "/ilink/bot/getupdates",
        json!({ "get_updates_buf": cursor }),
        POLL_TIMEOUT,
    )?;
    Ok(parse_updates(&v))
}

// ── 发消息 ────────────────────────────────────────────────────────────────────

/// 把长回复切成若干条。优先在换行处断开，读起来才不至于把一句话腰斩；
/// 单行超长时才硬切。`limit` 按**字符**计（中文一个字算一个）。
pub fn split_for_send(text: &str, limit: usize) -> Vec<String> {
    let limit = limit.max(1);
    if text.chars().count() <= limit {
        let t = text.trim();
        return if t.is_empty() {
            Vec::new()
        } else {
            vec![t.to_string()]
        };
    }
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0usize;
    let flush = |cur: &mut String, cur_len: &mut usize, out: &mut Vec<String>| {
        let t = cur.trim();
        if !t.is_empty() {
            out.push(t.to_string());
        }
        cur.clear();
        *cur_len = 0;
    };
    for line in text.split_inclusive('\n') {
        let mut rest = line;
        // 单行本身就超限：硬切成 limit 大小的块。
        while rest.chars().count() > limit {
            flush(&mut cur, &mut cur_len, &mut out);
            let head: String = rest.chars().take(limit).collect();
            let consumed = head.len();
            out.push(head.trim().to_string());
            rest = &rest[consumed..];
        }
        let n = rest.chars().count();
        if cur_len + n > limit {
            flush(&mut cur, &mut cur_len, &mut out);
        }
        cur.push_str(rest);
        cur_len += n;
    }
    flush(&mut cur, &mut cur_len, &mut out);
    out.retain(|s| !s.is_empty());
    out
}

/// 判定 `sendmessage` 的响应是否**被受理**。
///
/// ⚠️ 只能判「受理」，判不了「送达」，而且这个区别在这条协议上**大得能藏住一个 bug**：
/// 被服务端按 `client_id` 去重掉的回复，同样回 `200 {"message_id":…}`，微信端却永远收不到
/// （见 [`new_client_id`]）。协议不给任何投递回执，所以这里的 `Ok` 只意味着「服务端收下了」。
///
/// 判据按实测响应来（`{"message_id":7494772986059182472}`），而不是按最初那份逆向文档
/// 说的「成功是 200 空体」——那句是错的，照它写会把任何空响应都当成功。
pub fn parse_send_result(v: &Value) -> Result<(), String> {
    if let Some(r) = v.get("ret").and_then(Value::as_i64).filter(|r| *r != 0) {
        return Err(format!("发送失败 ret={r}：{v}"));
    }
    if v.get("message_id").is_some() {
        return Ok(());
    }
    Err(format!(
        "发送未被受理：响应里既没有 message_id 也没有 ret=0（实测正常应答形如 \
         {{\"message_id\":…}}）。原始响应：{v}"
    ))
}

/// 生成一个 `client_id`：消息的幂等键，**每条必须唯一**。
///
/// ⚠️⚠️ 这是「微信收不到回复」的真正根因，2026-08-17 实测钉死的。
///
/// 不带 `client_id` 时，`sendmessage` 照样返回 `200 {"message_id":…}`——请求是被**受理**
/// 了的，但服务端在**投递**阶段按 `client_id` 去重。我们每条回复都不带，于是它们在
/// 服务端折叠成同一个身份：历史上第一条被投递（并就地建出那个聊天记录），
/// 之后每一条都被当作重复静默丢弃。
///
/// 对照实验（同一条入站消息、同一个 `context_token`、20 秒内连发 6 个变体）：
/// - 带 `client_id` 的 U2 / U5 / U6 —— **全部送达**
/// - 不带的 U1（加 `from_user_id`）、U3（加时间戳）、U4（加 `session_id`/`root_id`/
///   `parent_id`）—— **全部石沉大海**
///
/// U6 同时证明：条目级的 `is_completed` / `msg_id` / 时间戳 / `button_item_list` /
/// `at_bot_username_list`，以及消息级的 `from_user_id`，**一个都不需要**。只差这一个。
///
/// 内容只要求唯一，不要求特定格式（微信自己用的是
/// `mmassistant_bypmsg_inbox_<hex>_8_<epoch_s>`，我们用 128 位随机数即可）。
/// 刻意不掺时间戳：那样单元测试就得跟时钟打交道，而唯一性本来就该由随机数保证。
pub fn new_client_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    format!(
        "wisecortex_{:016x}{:016x}",
        rng.gen::<u64>(),
        rng.gen::<u64>()
    )
}

/// 构造 `sendmessage` 的 `msg` 体（纯函数，便于测试）。
///
/// 字段集刻意保持最小：实测证明除了这几个，其余全是可选的。多发字段没有好处，
/// 只会在协议演进时多几个出错面。
pub fn build_send_msg(to_user_id: &str, context_token: &str, text: &str, client_id: &str) -> Value {
    json!({
        "to_user_id": to_user_id,
        "client_id": client_id, // 幂等键；缺了会被静默去重，见 new_client_id
        "message_type": 2,      // BOT
        "message_state": 2,     // FINISH
        "context_token": context_token,
        "item_list": [ { "type": ITEM_TEXT, "text_item": { "text": text } } ],
    })
}

/// 发一条文本。`context_token` 原样带回收到的那条消息的值，用于关联会话窗口。
fn send_one(
    cfg: &ClawbotConfig,
    to_user_id: &str,
    context_token: &str,
    text: &str,
) -> Result<(), String> {
    // 每条（含分条后的每一条）都重新生成，绝不能复用：复用等于告诉服务端「这条我发过了」。
    let msg = build_send_msg(to_user_id, context_token, text, &new_client_id());
    let v = post(
        cfg,
        "/ilink/bot/sendmessage",
        json!({ "msg": msg }),
        CALL_TIMEOUT,
    )?;
    parse_send_result(&v)
}

/// 发文本回复，超长自动分条。
///
/// `context_token` 为空直接报错、**不发**：空着发出去服务端照样回成功，但微信端收不到，
/// 而我们会打印一行「已回复」——用户那边石沉大海，日志里却一片祥和。见
/// [`parse_send_result`] 与 [`resolve_context_token`]。
pub fn send_text(
    cfg: &ClawbotConfig,
    to_user_id: &str,
    context_token: &str,
    text: &str,
) -> Result<(), String> {
    if context_token.trim().is_empty() {
        return Err(
            "没有可用的 context_token，本条回复未发送。iLink 要求回复必须带它才能路由到\
             对话窗口；不带的话服务端仍返回成功、微信端却收不到。请在微信里再发一条消息\
             以取得新的 context_token。"
                .to_string(),
        );
    }
    let chunks = split_for_send(text, SEND_CHUNK_CHARS);
    if chunks.is_empty() {
        return Ok(());
    }
    for c in chunks {
        send_one(cfg, to_user_id, context_token, &c)?;
    }
    Ok(())
}

// ── 「正在输入」 ──────────────────────────────────────────────────────────────

/// typing_ticket 缓存：user_id → (ticket, 取得时刻)。协议说 TTL 24 小时。
fn ticket_cache() -> &'static Mutex<HashMap<String, (String, Instant)>> {
    static C: OnceLock<Mutex<HashMap<String, (String, Instant)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

const TICKET_TTL: Duration = Duration::from_secs(23 * 3600);

/// 取（并缓存）typing_ticket。
pub fn typing_ticket(cfg: &ClawbotConfig, user_id: &str) -> Result<String, String> {
    if let Some((t, at)) = ticket_cache().lock().unwrap().get(user_id) {
        if at.elapsed() < TICKET_TTL {
            return Ok(t.clone());
        }
    }
    let v = post(
        cfg,
        "/ilink/bot/getconfig",
        json!({ "ilink_user_id": user_id }),
        CALL_TIMEOUT,
    )?;
    let ticket = v
        .get("typing_ticket")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("响应里没有 typing_ticket：{v}"))?
        .to_string();
    ticket_cache()
        .lock()
        .unwrap()
        .insert(user_id.to_string(), (ticket.clone(), Instant::now()));
    Ok(ticket)
}

/// 发「正在输入」状态。`status` 用 [`TYPING_ON`] / [`TYPING_OFF`]。
///
/// agent 一轮可达分钟级，期间手机上完全没有反馈，用户只会以为机器人死了。
/// 协议要求开启后每 5 秒续一次。
pub fn send_typing(
    cfg: &ClawbotConfig,
    user_id: &str,
    ticket: &str,
    status: i64,
) -> Result<(), String> {
    post(
        cfg,
        "/ilink/bot/sendtyping",
        json!({
            "ilink_user_id": user_id,
            "typing_ticket": ticket,
            "status": status,
        }),
        CALL_TIMEOUT,
    )?;
    Ok(())
}

/// 进程级共享去重表（容量 256）。游标异常或重投时同一条消息会到两次，
/// 不去重就会重复回答。
pub fn seen_recently(message_id: &str) -> bool {
    static D: OnceLock<Mutex<crate::feishu::Dedup>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(crate::feishu::Dedup::new(256)))
        .lock()
        .unwrap()
        .seen(message_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_version_uses_00mmnnpp_encoding() {
        // 协议文档给的唯一一个例子：v0.3.0 → 768。拿它锚住编码方式。
        assert_eq!(encode_version(0, 3, 0), 768);
        // 我们声明的版本要和请求体里的 channel_version 对得上。
        assert_eq!(CHANNEL_VERSION, "2.1.1");
        assert_eq!(CLIENT_VERSION, encode_version(2, 1, 1));
        assert_eq!(CLIENT_VERSION, 0x0002_0101);
        // 高位必须留空（协议写的是 0x00MMNNPP）。
        assert_eq!(CLIENT_VERSION >> 24, 0);
    }

    #[test]
    fn uin_header_is_base64_of_a_decimal_number() {
        for _ in 0..20 {
            let h = uin_header();
            let raw = base64::engine::general_purpose::STANDARD
                .decode(&h)
                .expect("应是合法 base64");
            let s = String::from_utf8(raw).expect("应是 UTF-8");
            assert!(
                s.chars().all(|c| c.is_ascii_digit()),
                "应是十进制数字串，实为 {s:?}"
            );
            s.parse::<u32>().expect("应能解析成 u32");
        }
        // 每次重新生成（防重放）。撞一次是巧合，20 次全同就是没随机。
        let all: std::collections::HashSet<String> = (0..20).map(|_| uin_header()).collect();
        assert!(all.len() > 1, "每个请求应重新生成 UIN");
    }

    #[test]
    fn base_falls_back_to_default_and_trims_slash() {
        let mut cfg = ClawbotConfig::default();
        assert_eq!(cfg.base(), DEFAULT_BASE);
        cfg.base_url = Some("  ".into());
        assert_eq!(cfg.base(), DEFAULT_BASE, "空白应回落默认域名");
        cfg.base_url = Some("https://idc2.example.com/".into());
        assert_eq!(cfg.base(), "https://idc2.example.com");
    }

    #[test]
    fn is_ready_needs_a_nonempty_token() {
        let mut cfg = ClawbotConfig::default();
        assert!(!cfg.is_ready());
        cfg.bot_token = Some(String::new());
        assert!(!cfg.is_ready(), "空 token 不算就绪");
        cfg.bot_token = Some("t".into());
        assert!(cfg.is_ready());
    }

    #[test]
    fn extract_text_takes_text_items() {
        let items = json!([
            { "type": ITEM_TEXT, "text_item": { "text": "帮我看下构建" } },
        ]);
        let (text, un) = extract_text(&items);
        assert_eq!(text, "帮我看下构建");
        assert!(un.is_empty());
    }

    #[test]
    fn extract_text_uses_server_side_voice_transcription() {
        // 语音转写是服务端白送的，不需要我们碰 silk 编码。
        let items = json!([
            { "type": ITEM_VOICE, "voice_item": { "text": "重启一下服务器", "playtime": 2100 } },
        ]);
        let (text, un) = extract_text(&items);
        assert_eq!(text, "重启一下服务器");
        assert!(un.is_empty());
    }

    #[test]
    fn extract_text_flags_voice_without_transcription() {
        // 转写失败（噪音/方言/太短）时不能装作没收到。
        let items = json!([{ "type": ITEM_VOICE, "voice_item": { "text": "" } }]);
        let (text, un) = extract_text(&items);
        assert!(text.is_empty());
        assert_eq!(un, vec!["语音（未能转写）".to_string()]);
    }

    #[test]
    fn extract_text_joins_multiple_items_and_reports_unsupported() {
        let items = json!([
            { "type": ITEM_TEXT,  "text_item": { "text": "看看这个" } },
            { "type": ITEM_IMAGE, "image_item": { "aeskey": "ff" } },
            { "type": ITEM_FILE,  "file_item": { "file_name": "a.log" } },
            { "type": ITEM_IMAGE, "image_item": { "aeskey": "ee" } },
            { "type": ITEM_TEXT,  "text_item": { "text": "急" } },
        ]);
        let (text, un) = extract_text(&items);
        assert_eq!(text, "看看这个\n急");
        // 同类型只报一次，顺序保持首次出现。
        assert_eq!(un, vec!["图片".to_string(), "文件".to_string()]);
    }

    #[test]
    fn extract_text_ignores_blank_and_unknown_items() {
        let items = json!([
            { "type": ITEM_TEXT, "text_item": { "text": "   " } },
            { "type": 99 },
            { "type": ITEM_TEXT },
        ]);
        let (text, un) = extract_text(&items);
        assert!(text.is_empty());
        assert!(un.is_empty(), "未知类型不该冒充「不支持的内容」");
        // 非数组一律当空。
        assert_eq!(extract_text(&Value::Null), (String::new(), Vec::new()));
    }

    #[test]
    fn is_actionable_filters_bot_echo_and_partial_messages() {
        // 机器人自己发的必须滤掉，否则自激回环。
        assert!(!is_actionable(
            &json!({ "message_type": 2, "message_state": 2 })
        ));
        // 流式半截消息跳过，等 FINISH 那条。
        assert!(!is_actionable(
            &json!({ "message_type": 1, "message_state": 1 })
        ));
        assert!(is_actionable(
            &json!({ "message_type": 1, "message_state": 2 })
        ));
        // 缺字段放行：宁可多处理不可漏消息。
        assert!(is_actionable(&json!({})));
    }

    #[test]
    fn parse_updates_extracts_messages_and_cursor() {
        let v = json!({
            "ret": 0,
            "msgs": [{
                "message_id": 90001,
                "from_user_id": "u1@im.wechat",
                "to_user_id": "b1@im.bot",
                "message_type": 1,
                "message_state": 2,
                "context_token": "CTX-1",
                "item_list": [{ "type": 1, "text_item": { "text": "你好" } }]
            }],
            "get_updates_buf": "CURSOR-2",
            "longpolling_timeout_ms": 35000
        });
        let Fetched::Msgs { msgs, cursor } = parse_updates(&v) else {
            panic!("应为 Msgs");
        };
        assert_eq!(cursor.as_deref(), Some("CURSOR-2"));
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].from_user_id, "u1@im.wechat");
        assert_eq!(msgs[0].context_token, "CTX-1");
        assert_eq!(msgs[0].text, "你好");
        // message_id 可能是数字，统一成字符串好做去重 key。
        assert_eq!(msgs[0].message_id, "90001");
    }

    #[test]
    fn parse_updates_handles_empty_poll() {
        // 长轮询挂满 35 秒无消息：msgs 空、游标可能不变。
        let Fetched::Msgs { msgs, cursor } = parse_updates(&json!({ "ret": 0, "msgs": [] })) else {
            panic!("应为 Msgs");
        };
        assert!(msgs.is_empty());
        assert_eq!(cursor, None, "服务端没给游标时应保留旧的（返回 None）");
        // 空字符串游标同样视作「没给」——写回去等于把游标清空，会导致重投。
        let Fetched::Msgs { cursor, .. } = parse_updates(&json!({ "get_updates_buf": "" })) else {
            panic!("应为 Msgs");
        };
        assert_eq!(cursor, None);
    }

    #[test]
    fn parse_updates_detects_session_expiry() {
        // errcode=-14：协议要求暂停全部 API 调用 1 小时。
        let v =
            json!({ "ret": -1, "errcode": ERRCODE_SESSION_EXPIRED, "errmsg": "session timeout" });
        assert_eq!(parse_updates(&v), Fetched::SessionExpired);
    }

    #[test]
    fn parse_updates_drops_bot_echo() {
        let v = json!({
            "msgs": [
                { "message_type": 2, "from_user_id": "b1@im.bot",
                  "item_list": [{ "type": 1, "text_item": { "text": "我刚回的" } }] },
                { "message_type": 1, "from_user_id": "u1@im.wechat",
                  "item_list": [{ "type": 1, "text_item": { "text": "用户说的" } }] }
            ]
        });
        let Fetched::Msgs { msgs, .. } = parse_updates(&v) else {
            panic!("应为 Msgs");
        };
        assert_eq!(msgs.len(), 1, "机器人自己的消息应被滤掉");
        assert_eq!(msgs[0].text, "用户说的");
    }

    #[test]
    fn parse_updates_keeps_unsupported_only_messages() {
        // 只发了一张图：没有文本，但要留着这条，好回一句「本版不支持图片」。
        let v = json!({
            "msgs": [{ "message_type": 1, "from_user_id": "u1@im.wechat",
                       "item_list": [{ "type": 2, "image_item": {} }] }]
        });
        let Fetched::Msgs { msgs, .. } = parse_updates(&v) else {
            panic!("应为 Msgs");
        };
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].text.is_empty());
        assert_eq!(msgs[0].unsupported, vec!["图片".to_string()]);
    }

    #[test]
    fn split_for_send_keeps_short_text_as_one_chunk() {
        assert_eq!(split_for_send("短回复", 100), vec!["短回复".to_string()]);
        assert!(split_for_send("   ", 100).is_empty(), "纯空白不该发出去");
        assert!(split_for_send("", 100).is_empty());
    }

    #[test]
    fn split_for_send_prefers_line_boundaries() {
        let text = "第一行内容\n第二行内容\n第三行内容";
        let chunks = split_for_send(text, 12);
        assert!(chunks.len() > 1);
        // 每块都不超限，且都是整行（没把一行腰斩）。
        for c in &chunks {
            assert!(c.chars().count() <= 12, "块超限：{c:?}");
            assert!(text.contains(c.trim()), "块不该跨行腰斩：{c:?}");
        }
        // 内容不丢：拼回去（去掉分块引入的边界）应覆盖原文每一行。
        let joined = chunks.join("\n");
        for line in text.lines() {
            assert!(joined.contains(line), "丢了一行：{line:?}");
        }
    }

    #[test]
    fn split_for_send_hard_splits_an_overlong_single_line() {
        // 一行就超限（比如一大段没有换行的日志），只能硬切。
        let text = "长".repeat(25);
        let chunks = split_for_send(&text, 10);
        assert_eq!(chunks.len(), 3);
        for c in &chunks {
            assert!(c.chars().count() <= 10);
        }
        assert_eq!(chunks.concat(), text, "硬切不该丢字符");
    }

    #[test]
    fn split_for_send_is_multibyte_safe() {
        // 按字符切，不能按字节——按字节会把 UTF-8 切碎 panic。
        let text = "中文夹English和🐉表情".repeat(20);
        let chunks = split_for_send(&text, 7);
        for c in &chunks {
            assert!(c.chars().count() <= 7, "块超限：{c:?}");
        }
        assert!(!chunks.is_empty());
    }

    #[test]
    fn parse_send_result_requires_a_message_id() {
        // 实测的成功应答就是这个形状（抓自 ilinkai.weixin.qq.com，2026-08）。
        assert!(parse_send_result(&json!({ "message_id": 7494772986059182472i64 })).is_ok());
        // 服务端自己报错。
        assert!(parse_send_result(&json!({ "ret": -1, "errmsg": "bad" })).is_err());
        // ⚠️ 回归：最初按逆向文档写的是「成功就是 200 空体」，于是空响应被当成成功。
        // 那是错的，也正是「二维码之后」第二次被同一份文档坑——空体必须判失败。
        assert!(parse_send_result(&Value::Null).is_err());
        assert!(parse_send_result(&json!({})).is_err());
        // ret=0 但没有 message_id：同样不认，别再放一个「看着像成功」的洞出来。
        assert!(parse_send_result(&json!({ "ret": 0 })).is_err());
    }

    #[test]
    fn every_message_gets_its_own_client_id() {
        // 幂等键复用 = 告诉服务端「这条我发过了」→ 微信端静默丢弃。这正是
        // 「只有第一条能收到」的根因，所以唯一性必须由测试守住。
        let ids: std::collections::HashSet<String> = (0..1000).map(|_| new_client_id()).collect();
        assert_eq!(ids.len(), 1000, "client_id 必须每条唯一");
        assert!(ids.iter().all(|s| !s.trim().is_empty()));
    }

    #[test]
    fn build_send_msg_carries_a_client_id_and_a_real_item_array() {
        let m = build_send_msg("u1@im.wechat", "CTX", "回复内容", "cid-1");
        // 少了 client_id，服务端照回 200 + message_id，用户却永远收不到。
        assert_eq!(m["client_id"], "cid-1");
        assert_eq!(m["to_user_id"], "u1@im.wechat");
        assert_eq!(m["context_token"], "CTX");
        assert_eq!(m["message_type"], 2); // BOT
        assert_eq!(m["message_state"], 2); // FINISH
                                           // item_list 必须是**数组**。排查时我的探针脚本把它写成了对象，服务端回
                                           // `{"ret":-1,"errmsg":"invalid request"}`，差点被当成协议结论。
        let items = m["item_list"].as_array().expect("item_list 必须是数组");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["type"], ITEM_TEXT);
        assert_eq!(items[0]["text_item"]["text"], "回复内容");
    }

    #[test]
    fn send_text_refuses_to_fire_without_a_context_token() {
        // 空 token 必须在**发出去之前**就失败：发出去的话服务端照样回
        // 200 + message_id，我们打印「已回复」，而用户微信里什么都没有。
        let cfg = ClawbotConfig {
            bot_token: Some("t".into()),
            ..Default::default()
        };
        let e = send_text(&cfg, "u1@im.wechat", "   ", "回复内容").unwrap_err();
        assert!(e.contains("context_token"), "错误里要点名根因：{e}");
        // 也要给用户一条出路，而不是只说「失败了」。
        assert!(e.contains("再发一条"), "错误里要写清怎么恢复：{e}");
    }

    #[test]
    fn pick_context_token_prefers_the_one_on_this_message() {
        // 本条消息自带的最新，优先用。
        assert_eq!(
            pick_context_token("CTX-NEW", Some("CTX-OLD".into())).as_deref(),
            Some("CTX-NEW")
        );
        // 本条没带 → 回落到存下来的那个（协议要求回复必须带，宁可旧也不能空）。
        assert_eq!(
            pick_context_token("", Some("CTX-OLD".into())).as_deref(),
            Some("CTX-OLD")
        );
        assert_eq!(
            pick_context_token("  ", Some("CTX-OLD".into())).as_deref(),
            Some("CTX-OLD")
        );
        // 两边都没有 → None，由调用方明确报错，绝不空着发。
        assert_eq!(pick_context_token("", None), None);
        assert_eq!(pick_context_token("  ", Some("   ".into())), None);
    }

    #[test]
    fn dedup_catches_repeated_message_ids() {
        // 游标异常或重投时同一条消息会到两次，不去重就重复回答。
        let mut d = crate::feishu::Dedup::new(4);
        assert!(!d.seen("m1"));
        assert!(d.seen("m1"));
    }
}
