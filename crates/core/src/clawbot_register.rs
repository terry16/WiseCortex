//! 微信 ClawBot 扫码接入：取二维码 → 轮询扫码状态 → 拿 bot_token。
//!
//! 流程（全是 GET，只带公共头，此时还没有 token）：
//! ```text
//! GET /ilink/bot/get_bot_qrcode?bot_type=3   → { qrcode, qrcode_img_content }
//! GET /ilink/bot/get_qrcode_status?qrcode=…  → wait / scaned / scaned_but_redirect / expired / confirmed
//!                                 confirmed  → { bot_token, baseurl, ilink_bot_id, ilink_user_id }
//! ```
//!
//! ⚠️ `qrcode_img_content` **不是图片**，是一个要被编码进二维码里的链接
//! （实测形如 `https://liteapp.weixin.qq.com/q/XXXXXX?qrcode=…&bot_type=3`），
//! 由我们渲染成二维码。详见 [`qr_data_url`]。
//! 二维码有效期约 5 分钟，最多刷新 3 次。

use std::time::Duration;

use serde_json::Value;

use crate::clawbot::{ClawbotConfig, CLIENT_VERSION, DEFAULT_BASE};

/// 建 bot 用的类型参数。协议文档里没解释它的含义，但所有已知实现都传 3。
const BOT_TYPE: &str = "3";

/// 取二维码用的超时。
const QR_TIMEOUT: Duration = Duration::from_secs(15);
/// 查扫码状态是长轮询（服务端最多挂 35 秒），给足冗余。
const STATUS_TIMEOUT: Duration = Duration::from_secs(50);

/// 建议的轮询间隔（秒）。状态接口本身是长轮询，前端不必催得太急。
pub const POLL_INTERVAL: u64 = 2;

/// `begin` 的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct BeginResult {
    /// 会话标识，后续查状态要带上。
    pub qrcode: String,
    /// 可直接塞进 `<img src>` 的二维码（data URL）。
    pub qr_img: String,
}

/// 轮询结果。
#[derive(Debug, Clone, PartialEq)]
pub enum PollOutcome {
    /// 等待扫码 / 等待用户在手机上确认。`redirect_host` 非空表示要切到该域名继续轮询。
    Pending {
        redirect_host: Option<String>,
    },
    Success {
        bot_token: String,
        base_url: String,
        bot_id: Option<String>,
        user_id: Option<String>,
    },
    /// 二维码过期（可重新生成）。
    Expired,
    Error(String),
}

fn get(base: &str, path: &str, query: &[(&str, &str)], timeout: Duration) -> Result<Value, String> {
    let url = format!("{}{path}", base.trim_end_matches('/'));
    let resp = crate::net::blocking_builder()
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())?
        .get(url)
        .header("iLink-App-Id", "bot")
        .header("iLink-App-ClientVersion", CLIENT_VERSION.to_string())
        .query(query)
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().map_err(|e| e.to_string())?;
    serde_json::from_str::<Value>(&text).map_err(|e| format!("HTTP {status}，响应非 JSON（{e}）"))
}

/// 把服务端给的 `qrcode_img_content` 变成前端能直接塞进 `<img src>` 的值。
///
/// ⚠️ 实测（2026-08）微信返回的是**一个链接**，形如
/// `https://liteapp.weixin.qq.com/q/XXXXXX?qrcode=<qrcode>&bot_type=3`——
/// 也就是「要被编码进二维码里的内容」，**不是图片本身**，得由我们把它画成二维码。
///
/// 最初依据的逆向文档写的是「base64 PNG」，那是错的。照那个写会拼出
/// `data:image/png;base64,https://liteapp…`，前端只能显示一张碎图——
/// 「二维码显示不出来」这个 bug 就是这么来的。
pub fn qr_data_url(content: &str) -> Result<String, String> {
    let c = content.trim();
    if c.is_empty() {
        return Err("服务端没有返回二维码内容（qrcode_img_content 为空）".to_string());
    }
    // 已经是能直接显示的图片，原样用。
    if c.starts_with("data:") {
        return Ok(c.to_string());
    }
    // 常规情况：是要被扫的链接，自己渲染成二维码。
    if c.starts_with("http://") || c.starts_with("https://") {
        return svg_data_url(&crate::feishu_register::qr_svg(c)?);
    }
    // 兜底：万一哪天真给了裸 base64 图片数据。
    Ok(format!("data:image/png;base64,{c}"))
}

fn svg_data_url(svg: &str) -> Result<String, String> {
    use base64::Engine;
    Ok(format!(
        "data:image/svg+xml;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(svg)
    ))
}

/// 解析 `get_bot_qrcode` 的响应（纯函数）。
///
/// 拿不到 `qrcode_img_content` 时**直接报错**，不再用 `qrcode` 串自己画一张兜底：
/// 那个串是会话标识、不是扫码目标，画出来的码扫了什么也不会发生——
/// 给用户一张扫不动的码，比明说「取二维码失败」更糟。
pub fn parse_begin(v: &Value) -> Result<BeginResult, String> {
    // 接口用 ret 报错；非 0 时后面的字段没有意义。
    if let Some(ret) = v.get("ret").and_then(Value::as_i64).filter(|r| *r != 0) {
        return Err(format!("取二维码失败 ret={ret}：{v}"));
    }
    let qrcode = v
        .get("qrcode")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("响应里没有 qrcode：{v}"))?
        .to_string();
    let qr_img = qr_data_url(
        v.get("qrcode_img_content")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )?;
    Ok(BeginResult { qrcode, qr_img })
}

/// 取二维码。`base` 传 None 用默认域名。
pub fn begin(base: Option<&str>) -> Result<BeginResult, String> {
    let v = get(
        base.unwrap_or(DEFAULT_BASE),
        "/ilink/bot/get_bot_qrcode",
        &[("bot_type", BOT_TYPE)],
        QR_TIMEOUT,
    )?;
    parse_begin(&v)
}

/// 解析扫码状态（纯函数）。`current_base` 用于在服务端没回 `baseurl` 时兜底。
pub fn parse_status(v: &Value, current_base: &str) -> PollOutcome {
    let status = v.get("status").and_then(Value::as_str).unwrap_or("");
    match status {
        "confirmed" => {
            let Some(token) = v
                .get("bot_token")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                return PollOutcome::Error(format!("已确认但没拿到 bot_token：{v}"));
            };
            let base_url = v
                .get("baseurl")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(current_base)
                .to_string();
            PollOutcome::Success {
                bot_token: token.to_string(),
                base_url,
                bot_id: v
                    .get("ilink_bot_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                user_id: v
                    .get("ilink_user_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            }
        }
        // 扫了码但落在别的 IDC：后续轮询必须切到 redirect_host，否则永远停在「等待确认」。
        "scaned_but_redirect" => PollOutcome::Pending {
            redirect_host: v
                .get("redirect_host")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        },
        "wait" | "scaned" | "" => PollOutcome::Pending {
            redirect_host: None,
        },
        "expired" => PollOutcome::Expired,
        other => PollOutcome::Error(format!("未知扫码状态：{other}")),
    }
}

/// 轮询一次扫码状态。
pub fn poll(base: Option<&str>, qrcode: &str) -> Result<PollOutcome, String> {
    let base = base.unwrap_or(DEFAULT_BASE);
    let v = get(
        base,
        "/ilink/bot/get_qrcode_status",
        &[("qrcode", qrcode)],
        STATUS_TIMEOUT,
    )?;
    Ok(parse_status(&v, base))
}

/// 把扫码结果写进 `clawbot.json`，并顺手打开开关（扫都扫了，显然是要用）。
pub fn save_credentials(
    bot_token: String,
    base_url: String,
    bot_id: Option<String>,
    user_id: Option<String>,
) -> std::io::Result<()> {
    let mut cfg = crate::clawbot::load();
    cfg.bot_token = Some(bot_token);
    cfg.base_url = Some(base_url);
    if bot_id.is_some() {
        cfg.bot_id = bot_id;
    }
    if user_id.is_some() {
        cfg.user_id = user_id;
    }
    cfg.enabled = true;
    crate::clawbot::save(&cfg)
}

/// 「谁在用这个 bot」的展示串：优先 bot_id，其次 user_id。
pub fn describe(cfg: &ClawbotConfig) -> String {
    cfg.bot_id
        .clone()
        .or_else(|| cfg.user_id.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_begin_renders_the_real_wechat_response() {
        // 回归：这是 2026-08 从 ilinkai.weixin.qq.com 实测抓到的真实响应。
        // qrcode_img_content 是**要被扫的链接**，不是图片——必须由我们画成二维码。
        // 早先按逆向文档当成 base64 PNG，拼出 `data:image/png;base64,https://…`，
        // 前端只显示一张碎图，也就是「二维码显示不出来」。
        let v = json!({
            "qrcode": "1f70f1eb14553cff36976ab6d2f8e97e",
            "qrcode_img_content":
                "https://liteapp.weixin.qq.com/q/7GiQu1?qrcode=1f70f1eb14553cff36976ab6d2f8e97e&bot_type=3",
            "ret": 0
        });
        let b = parse_begin(&v).unwrap();
        assert_eq!(b.qrcode, "1f70f1eb14553cff36976ab6d2f8e97e");
        assert!(
            b.qr_img.starts_with("data:image/svg+xml;base64,"),
            "链接应被渲染成二维码 SVG，实际：{}",
            &b.qr_img[..b.qr_img.len().min(60)]
        );
        assert!(
            !b.qr_img.contains("https://"),
            "链接不能被原样塞进 data URL：{}",
            &b.qr_img[..b.qr_img.len().min(80)]
        );
    }

    #[test]
    fn qr_data_url_handles_each_content_shape() {
        // 链接 → 自己渲染成二维码。
        let u = qr_data_url("https://liteapp.weixin.qq.com/q/7GiQu1?qrcode=abc").unwrap();
        assert!(u.starts_with("data:image/svg+xml;base64,"));
        // 已经是图片 → 原样用。
        assert_eq!(
            qr_data_url("data:image/png;base64,AAAA").unwrap(),
            "data:image/png;base64,AAAA"
        );
        // 裸 base64 → 当 PNG 兜底。
        assert_eq!(qr_data_url("AAAA").unwrap(), "data:image/png;base64,AAAA");
        // 空 → 报错，而不是画一张扫不动的码。
        assert!(qr_data_url("   ").is_err());
    }

    #[test]
    fn parse_begin_errors_instead_of_making_an_unscannable_code() {
        assert!(parse_begin(&json!({})).is_err());
        assert!(parse_begin(&json!({ "qrcode": "" })).is_err());
        // 有 qrcode 但没有图片内容：qrcode 是会话标识、不是扫码目标，
        // 拿它画码等于给用户一张扫了没反应的图 —— 必须报错。
        assert!(parse_begin(&json!({ "qrcode": "QR-2" })).is_err());
        // 接口自己报错时也要冒出来。
        assert!(parse_begin(&json!({ "ret": -1, "qrcode": "x" })).is_err());
    }

    #[test]
    fn parse_status_matches_the_real_wait_response() {
        // 回归：实测抓到的未扫码返回（服务端挂满 30s 后给出）。
        assert_eq!(
            parse_status(&json!({ "ret": 0, "status": "wait" }), DEFAULT_BASE),
            PollOutcome::Pending {
                redirect_host: None
            }
        );
    }

    #[test]
    fn parse_status_pending_for_wait_and_scaned() {
        for s in ["wait", "scaned", ""] {
            assert_eq!(
                parse_status(&json!({ "status": s }), DEFAULT_BASE),
                PollOutcome::Pending {
                    redirect_host: None
                },
                "status={s} 应为 pending"
            );
        }
    }

    #[test]
    fn parse_status_carries_idc_redirect_host() {
        // 漏了这一条的表现是「扫了码但一直停在等待确认」——后续轮询发去了错误的 IDC。
        let v = json!({ "status": "scaned_but_redirect", "redirect_host": "idc2.example.com" });
        assert_eq!(
            parse_status(&v, DEFAULT_BASE),
            PollOutcome::Pending {
                redirect_host: Some("idc2.example.com".to_string())
            }
        );
    }

    #[test]
    fn parse_status_success_extracts_credentials() {
        let v = json!({
            "status": "confirmed",
            "ilink_bot_id": "b1@im.bot",
            "bot_token": "TOKEN-1",
            "baseurl": "https://idc2.example.com",
            "ilink_user_id": "u1"
        });
        match parse_status(&v, DEFAULT_BASE) {
            PollOutcome::Success {
                bot_token,
                base_url,
                bot_id,
                user_id,
            } => {
                assert_eq!(bot_token, "TOKEN-1");
                assert_eq!(base_url, "https://idc2.example.com");
                assert_eq!(bot_id.as_deref(), Some("b1@im.bot"));
                assert_eq!(user_id.as_deref(), Some("u1"));
            }
            o => panic!("应为 Success，得到 {o:?}"),
        }
    }

    #[test]
    fn parse_status_success_falls_back_to_current_base() {
        let v = json!({ "status": "confirmed", "bot_token": "T" });
        match parse_status(&v, "https://cur.example.com") {
            PollOutcome::Success { base_url, .. } => {
                assert_eq!(base_url, "https://cur.example.com")
            }
            o => panic!("应为 Success，得到 {o:?}"),
        }
    }

    #[test]
    fn parse_status_confirmed_without_token_is_an_error() {
        // 「已确认」却没给 token：当成错误报出来，不要静默存一个空 token。
        assert!(matches!(
            parse_status(&json!({ "status": "confirmed" }), DEFAULT_BASE),
            PollOutcome::Error(_)
        ));
    }

    #[test]
    fn parse_status_expired_and_unknown() {
        assert_eq!(
            parse_status(&json!({ "status": "expired" }), DEFAULT_BASE),
            PollOutcome::Expired
        );
        assert!(matches!(
            parse_status(&json!({ "status": "什么鬼" }), DEFAULT_BASE),
            PollOutcome::Error(_)
        ));
    }

    #[test]
    fn describe_prefers_bot_id() {
        let mut cfg = ClawbotConfig::default();
        assert_eq!(describe(&cfg), "");
        cfg.user_id = Some("u1".into());
        assert_eq!(describe(&cfg), "u1");
        cfg.bot_id = Some("b1@im.bot".into());
        assert_eq!(describe(&cfg), "b1@im.bot");
    }
}
