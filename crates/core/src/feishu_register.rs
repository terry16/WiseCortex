//! 飞书/Lark 扫码接入：OAuth device-code 应用注册流。
//!
//! 流程：init（校验支持 client_secret）→ begin（拿 device_code + 二维码 URL）→ 轮询 poll
//! （用户飞书 App 扫码授权后返回 app_id/app_secret/open_id）。端点 `<accounts>/oauth/v1/app/registration`，
//! 表单 POST。请求经 [`crate::net`] 走代理。只解决「建应用 + 拿凭据」；收消息仍走事件订阅回调。

use std::time::Duration;

use serde_json::Value;

/// 飞书 / Lark 域。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Feishu,
    Lark,
}

impl Domain {
    pub fn as_str(self) -> &'static str {
        match self {
            Domain::Feishu => "feishu",
            Domain::Lark => "lark",
        }
    }
    pub fn parse(s: &str) -> Domain {
        if s.eq_ignore_ascii_case("lark") {
            Domain::Lark
        } else {
            Domain::Feishu
        }
    }
    fn accounts_base(self) -> &'static str {
        match self {
            Domain::Feishu => "https://accounts.feishu.cn",
            Domain::Lark => "https://accounts.larksuite.com",
        }
    }
}

const REGISTRATION_PATH: &str = "/oauth/v1/app/registration";
const SCAN_TP: &str = "ob_cli_app";

/// begin 的结果（含二维码 URL）。
#[derive(Debug, Clone, PartialEq)]
pub struct BeginResult {
    pub device_code: String,
    pub qr_url: String,
    pub interval: u64,
    pub expire: u64,
}

/// 轮询结果。
#[derive(Debug, Clone, PartialEq)]
pub enum PollOutcome {
    /// 等待用户扫码 / 授权中；`switch_to` 非空表示需切到该域重试。
    Pending {
        switch_to: Option<Domain>,
    },
    Success {
        app_id: String,
        app_secret: String,
        open_id: Option<String>,
        domain: Domain,
    },
    Denied,
    Expired,
    Error(String),
}

fn post_form(domain: Domain, params: &[(&str, &str)]) -> Result<Value, String> {
    let url = format!("{}{REGISTRATION_PATH}", domain.accounts_base());
    let resp = crate::net::blocking_builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?
        .post(url)
        .form(params)
        .send()
        .map_err(|e| e.to_string())?;
    // 注册轮询在 pending/error 时也返回带 JSON 体的 4xx，故不按状态码短路。
    resp.json::<Value>().map_err(|e| e.to_string())
}

/// 校验当前环境支持 client_secret 注册。
pub fn init(domain: Domain) -> Result<(), String> {
    let v = post_form(domain, &[("action", "init")])?;
    let ok = v
        .get("supported_auth_methods")
        .and_then(Value::as_array)
        .map(|a| a.iter().any(|m| m.as_str() == Some("client_secret")))
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err("当前环境不支持 client_secret 注册".to_string())
    }
}

/// 解析 begin 响应（纯函数，便于测试）。
pub fn parse_begin(json: &Value) -> Result<BeginResult, String> {
    let device_code = json
        .get("device_code")
        .and_then(Value::as_str)
        .ok_or("缺少 device_code")?
        .to_string();
    let complete = json
        .get("verification_uri_complete")
        .and_then(Value::as_str)
        .ok_or("缺少 verification_uri_complete")?;
    // 追加 tp 参数：飞书据此判定扫码入口类型，缺了会跳到通用授权页而非应用注册页。
    let sep = if complete.contains('?') { '&' } else { '?' };
    let qr_url = format!("{complete}{sep}tp={SCAN_TP}");
    let interval = json.get("interval").and_then(Value::as_u64).unwrap_or(5);
    let expire = json.get("expire_in").and_then(Value::as_u64).unwrap_or(600);
    Ok(BeginResult {
        device_code,
        qr_url,
        interval,
        expire,
    })
}

/// 开始注册：begin 拿 device_code 与二维码 URL。
pub fn begin(domain: Domain) -> Result<BeginResult, String> {
    let v = post_form(
        domain,
        &[
            ("action", "begin"),
            ("archetype", "PersonalAgent"),
            ("auth_method", "client_secret"),
            ("request_user_info", "open_id"),
        ],
    )?;
    parse_begin(&v)
}

/// 解析 poll 响应（纯函数）。`current` 为本次轮询所用域。
pub fn parse_poll(json: &Value, current: Domain) -> PollOutcome {
    // 成功：拿到 client_id + client_secret。
    if let (Some(app_id), Some(app_secret)) = (
        json.get("client_id").and_then(Value::as_str),
        json.get("client_secret").and_then(Value::as_str),
    ) {
        let open_id = json
            .get("user_info")
            .and_then(|u| u.get("open_id"))
            .and_then(Value::as_str)
            .map(str::to_string);
        return PollOutcome::Success {
            app_id: app_id.to_string(),
            app_secret: app_secret.to_string(),
            open_id,
            domain: current,
        };
    }
    // 域自动检测：tenant_brand=lark 且当前非 lark → 切域重试。
    if json
        .get("user_info")
        .and_then(|u| u.get("tenant_brand"))
        .and_then(Value::as_str)
        == Some("lark")
        && current != Domain::Lark
    {
        return PollOutcome::Pending {
            switch_to: Some(Domain::Lark),
        };
    }
    match json.get("error").and_then(Value::as_str) {
        Some("authorization_pending") | Some("slow_down") | None => {
            PollOutcome::Pending { switch_to: None }
        }
        Some("access_denied") => PollOutcome::Denied,
        Some("expired_token") => PollOutcome::Expired,
        Some(e) => {
            let desc = json
                .get("error_description")
                .and_then(Value::as_str)
                .unwrap_or("");
            PollOutcome::Error(format!("{e}: {desc}"))
        }
    }
}

/// 轮询一次。
pub fn poll(device_code: &str, domain: Domain) -> Result<PollOutcome, String> {
    let v = post_form(
        domain,
        &[
            ("action", "poll"),
            ("device_code", device_code),
            ("tp", SCAN_TP),
        ],
    )?;
    Ok(parse_poll(&v, domain))
}

/// 把数据渲染成二维码 SVG（quiet zone 4 模块，每模块 6px）。
pub fn qr_svg(data: &str) -> Result<String, String> {
    use qrcode::{Color, QrCode};
    let code = QrCode::new(data.as_bytes()).map_err(|e| format!("二维码生成失败：{e}"))?;
    let width = code.width();
    let colors = code.to_colors();
    let quiet = 4usize;
    let scale = 6usize;
    let total = (width + quiet * 2) * scale;
    let mut rects = String::new();
    for y in 0..width {
        for x in 0..width {
            if colors[y * width + x] == Color::Dark {
                let px = (x + quiet) * scale;
                let py = (y + quiet) * scale;
                rects.push_str(&format!(
                    "<rect x=\"{px}\" y=\"{py}\" width=\"{scale}\" height=\"{scale}\"/>"
                ));
            }
        }
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{total}\" height=\"{total}\" \
         viewBox=\"0 0 {total} {total}\" shape-rendering=\"crispEdges\">\
         <rect width=\"{total}\" height=\"{total}\" fill=\"#fff\"/>\
         <g fill=\"#111\">{rects}</g></svg>"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_begin_builds_qr_url_with_tp() {
        let v = json!({
            "device_code": "dc_1",
            "verification_uri_complete": "https://example.com/auth?code=abc",
            "interval": 5,
            "expire_in": 600
        });
        let b = parse_begin(&v).unwrap();
        assert_eq!(b.device_code, "dc_1");
        assert!(b.qr_url.contains("code=abc"));
        assert!(b.qr_url.contains("tp=ob_cli_app"));
        assert_eq!(b.interval, 5);
    }

    #[test]
    fn parse_poll_success_extracts_credentials() {
        let v = json!({
            "client_id": "cli_app",
            "client_secret": "secret",
            "user_info": { "open_id": "ou_x" }
        });
        match parse_poll(&v, Domain::Feishu) {
            PollOutcome::Success {
                app_id,
                app_secret,
                open_id,
                domain,
            } => {
                assert_eq!(app_id, "cli_app");
                assert_eq!(app_secret, "secret");
                assert_eq!(open_id.as_deref(), Some("ou_x"));
                assert_eq!(domain, Domain::Feishu);
            }
            o => panic!("应为 success，得到 {o:?}"),
        }
    }

    #[test]
    fn parse_poll_handles_pending_lark_switch_denied_expired() {
        assert_eq!(
            parse_poll(&json!({ "error": "authorization_pending" }), Domain::Feishu),
            PollOutcome::Pending { switch_to: None }
        );
        assert_eq!(
            parse_poll(&json!({ "error": "slow_down" }), Domain::Feishu),
            PollOutcome::Pending { switch_to: None }
        );
        // lark 切域。
        assert_eq!(
            parse_poll(
                &json!({ "user_info": { "tenant_brand": "lark" } }),
                Domain::Feishu
            ),
            PollOutcome::Pending {
                switch_to: Some(Domain::Lark)
            }
        );
        assert_eq!(
            parse_poll(&json!({ "error": "access_denied" }), Domain::Feishu),
            PollOutcome::Denied
        );
        assert_eq!(
            parse_poll(&json!({ "error": "expired_token" }), Domain::Feishu),
            PollOutcome::Expired
        );
        assert!(matches!(
            parse_poll(
                &json!({ "error": "boom", "error_description": "x" }),
                Domain::Feishu
            ),
            PollOutcome::Error(_)
        ));
    }

    #[test]
    fn qr_svg_renders_nonempty_svg() {
        let svg = qr_svg("https://example.com/auth?code=abc").unwrap();
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("<rect"));
        assert!(svg.ends_with("</svg>"));
    }
}
