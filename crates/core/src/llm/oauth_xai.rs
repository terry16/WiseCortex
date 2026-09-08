//! xAI Grok **订阅** OAuth（RFC 8628 设备码流），复用 xAI 官方 Grok-CLI 的公共 client_id。
//! 仅个人自用；token 存本机配置目录，绝不进 git。
//!
//! 流程：POST device/code 拿 `user_code` + `verification_uri` → 用户在**任意设备**的浏览器
//! 打开链接输码授权 → 前端按 `interval` 轮询 `/api/oauth/xai/poll`（服务端每次对 token
//! 端点打一枪）→ 授权完成即落盘。选设备码而非回环回调：WiseCortex 常部署在服务器上，
//! `127.0.0.1:56121` 回调到不了用户浏览器。
//!
//! 推理走标准 xAI API（`api.x.ai/v1`，OpenAI 兼容线缆），`Authorization: Bearer` 用
//! OAuth access token 代替 API Key。

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

// ── 常量（这几个最容易随 xAI 侧调整而失效，改动优先查这里）────────────────
/// xAI 官方 Grok-CLI 桌面客户端的公共 client_id（auth 服务器只放行注册过的客户端）。
pub const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";
pub const DEVICE_CODE_URL: &str = "https://auth.x.ai/oauth2/device/code";
pub const SCOPES: &str = "openid profile email offline_access grok-cli:access api:access";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// 推理端点（OpenAI 兼容 chat/completions 的 base）。
pub const XAI_API_BASE: &str = "https://api.x.ai/v1";

/// 生效的推理 base：默认 [`XAI_API_BASE`]；环境变量 `WC_PROXY_GROK_BASE_URL` 可覆盖
/// （如 Cloudflare Worker 反代）。注意默认值含 `/v1`——反代时变量值也要指到等价层级
/// （后面拼 `/chat/completions`）。
pub fn api_base() -> String {
    super::oauth::env_base_url("WC_PROXY_GROK_BASE_URL", XAI_API_BASE)
}

/// 生效的 auth base：默认 `https://auth.x.ai`；`WC_PROXY_GROK_AUTH_BASE_URL` 可覆盖。
/// 设备码申请与 token 兑换/刷新都从这里拼路径。
fn auth_base() -> String {
    super::oauth::env_base_url("WC_PROXY_GROK_AUTH_BASE_URL", "https://auth.x.ai")
}

fn token_url() -> String {
    format!("{}/oauth2/token", auth_base())
}

fn device_code_url() -> String {
    format!("{}/oauth2/device/code", auth_base())
}
/// 订阅档缺省模型（agentic 编码模型；档内可改成 grok-4-1 等）。
pub const DEFAULT_MODEL: &str = "grok-code-fast-1";

/// 提前 2 分钟刷新：xAI access token 短命，边距不必像 Codex 那么大。
const REFRESH_MARGIN_SECS: u64 = 120;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── token 存储 ──────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct XaiTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// 过期的 Unix 秒时间戳。
    pub expires_at: u64,
}

impl XaiTokens {
    pub fn is_expired(&self) -> bool {
        now_secs() + REFRESH_MARGIN_SECS >= self.expires_at
    }
}

pub fn tokens_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("wisecortex").join("xai_oauth.json"))
}

pub fn load_tokens() -> Option<XaiTokens> {
    let p = tokens_path()?;
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

pub fn save_tokens(t: &XaiTokens) -> Result<(), String> {
    let p = tokens_path().ok_or("无法定位配置目录")?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败: {e}"))?;
    }
    std::fs::write(
        &p,
        serde_json::to_string_pretty(t).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("写入失败: {e}"))
}

pub fn clear_tokens() -> Result<(), String> {
    if let Some(p) = tokens_path() {
        if p.exists() {
            std::fs::remove_file(&p).map_err(|e| format!("删除失败: {e}"))?;
        }
    }
    Ok(())
}

pub fn is_logged_in() -> bool {
    load_tokens().is_some()
}

// ── JWT exp（xAI 不总回 expires_in，access token 是 JWT 时从 exp 声明取）────
fn jwt_exp(token: &str) -> Option<u64> {
    let seg = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(seg.trim_end_matches('='))
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("exp")?.as_u64()
}

// ── 设备码流 ────────────────────────────────────────────────────────────────
/// device/code 端点的响应（原样透传给前端展示 user_code / 链接）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    /// 设备码有效期（秒）；缺省 300。
    #[serde(default)]
    pub expires_in: Option<u64>,
    /// 建议轮询间隔（秒）；缺省 5。
    #[serde(default)]
    pub interval: Option<u64>,
}

/// 一次轮询的结果（非终态由前端按 interval 继续轮询）。
#[derive(Debug, Clone, PartialEq)]
pub enum PollOutcome {
    /// 授权完成，token 已落盘。
    Authorized,
    /// 用户还没在浏览器完成授权，按原间隔继续。
    Pending,
    /// 服务端要求放慢：间隔 +5s 后继续（RFC 8628 §3.5）。
    SlowDown,
}

/// 按 RFC 8628 §3.5 归类 token 端点的错误响应：非终态返回 Ok，终态返回 Err(用户可读消息)。
fn classify_poll_error(error: &str, description: Option<&str>) -> Result<PollOutcome, String> {
    match error {
        "authorization_pending" => Ok(PollOutcome::Pending),
        "slow_down" => Ok(PollOutcome::SlowDown),
        "access_denied" | "authorization_denied" => Err("授权被拒绝".to_string()),
        "expired_token" => Err("设备码已过期，请重新登录".to_string()),
        other => Err(format!(
            "设备码兑换失败: {}",
            description.filter(|s| !s.is_empty()).unwrap_or(other)
        )),
    }
}

// ── 网络：设备码 / 兑换 / 刷新（form-urlencoded）────────────────────────────
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// token 响应 → 持久化形态。刷新响应可能不回 refresh_token（沿用旧的）；
/// 过期时间优先 expires_in，缺失时从 JWT `exp` 取，再缺省 1 小时。
fn to_tokens(r: TokenResponse, prev: Option<&XaiTokens>) -> Result<XaiTokens, String> {
    let refresh_token = r
        .refresh_token
        .filter(|s| !s.is_empty())
        .or_else(|| prev.map(|p| p.refresh_token.clone()))
        .ok_or("响应缺少 refresh_token")?;
    let expires_at = match r.expires_in {
        Some(ttl) => now_secs() + ttl,
        None => jwt_exp(&r.access_token).unwrap_or_else(|| now_secs() + 3600),
    };
    Ok(XaiTokens {
        access_token: r.access_token,
        refresh_token,
        expires_at,
    })
}

async fn post_form(url: &str, params: &[(&str, &str)]) -> Result<reqwest::Response, String> {
    let http = crate::net::async_builder_timed()
        .build()
        .map_err(|e| e.to_string())?;
    http.post(url)
        .form(params)
        .send()
        .await
        .map_err(|e| crate::net::maybe_transient(&e, format!("请求 {url} 失败: {e}")))
}

/// 申请设备码（登录第一步）。
pub async fn request_device_code() -> Result<DeviceCode, String> {
    let resp = post_form(
        &device_code_url(),
        &[("client_id", CLIENT_ID), ("scope", SCOPES)],
    )
    .await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("设备码申请失败 {status}: {text}"));
    }
    let d: DeviceCode = serde_json::from_str(&text)
        .map_err(|e| format!("解析设备码响应失败: {e}（原文: {text}）"))?;
    if d.device_code.is_empty() || d.user_code.is_empty() || d.verification_uri.is_empty() {
        return Err("设备码响应缺少 device_code/user_code/verification_uri".to_string());
    }
    Ok(d)
}

/// 对 token 端点打一枪（前端轮询一次调一次）。授权完成时 token 已落盘。
pub async fn poll_device_token(device_code: &str) -> Result<PollOutcome, String> {
    let resp = post_form(
        &token_url(),
        &[
            ("grant_type", DEVICE_GRANT),
            ("client_id", CLIENT_ID),
            ("device_code", device_code),
        ],
    )
    .await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if status.is_success() {
        let r: TokenResponse = serde_json::from_str(&text)
            .map_err(|e| format!("解析 token 响应失败: {e}（原文: {text}）"))?;
        let tokens = to_tokens(r, None)?;
        save_tokens(&tokens)?;
        return Ok(PollOutcome::Authorized);
    }
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let error = v.get("error").and_then(|x| x.as_str()).unwrap_or("");
    let desc = v.get("error_description").and_then(|x| x.as_str());
    if error.is_empty() {
        return Err(format!("token 端点返回 {status}: {text}"));
    }
    classify_poll_error(error, desc)
}

/// 刷新 token 并落盘。
pub async fn refresh(prev: &XaiTokens) -> Result<XaiTokens, String> {
    let resp = post_form(
        &token_url(),
        &[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", &prev.refresh_token),
        ],
    )
    .await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("token 刷新失败 {status}: {text}"));
    }
    let r: TokenResponse = serde_json::from_str(&text)
        .map_err(|e| format!("解析刷新响应失败: {e}（原文: {text}）"))?;
    let tokens = to_tokens(r, Some(prev))?;
    save_tokens(&tokens)?;
    Ok(tokens)
}

/// 取可用 access token：按需刷新；未登录返回 Err。
pub async fn valid_access_token() -> Result<String, String> {
    let tokens = load_tokens().ok_or("尚未登录 Grok 订阅")?;
    let fresh = if tokens.is_expired() {
        refresh(&tokens).await?
    } else {
        tokens
    };
    Ok(fresh.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64url(s: &str) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s.as_bytes())
    }
    fn fake_jwt(payload_json: &str) -> String {
        format!("h.{}.s", b64url(payload_json))
    }

    #[test]
    fn to_tokens_prefers_expires_in() {
        let t = to_tokens(
            TokenResponse {
                access_token: "a".into(),
                refresh_token: Some("r".into()),
                expires_in: Some(600),
            },
            None,
        )
        .unwrap();
        let want = now_secs() + 600;
        assert!(t.expires_at >= want - 2 && t.expires_at <= want + 2);
    }

    #[test]
    fn to_tokens_falls_back_to_jwt_exp() {
        let exp = now_secs() + 900;
        let jwt = fake_jwt(&format!(r#"{{"exp":{exp}}}"#));
        let t = to_tokens(
            TokenResponse {
                access_token: jwt,
                refresh_token: Some("r".into()),
                expires_in: None,
            },
            None,
        )
        .unwrap();
        assert_eq!(t.expires_at, exp);
    }

    #[test]
    fn to_tokens_keeps_prev_refresh_when_rotation_omits_it() {
        let prev = XaiTokens {
            access_token: "old".into(),
            refresh_token: "keep-me".into(),
            expires_at: 1,
        };
        let t = to_tokens(
            TokenResponse {
                access_token: "new".into(),
                refresh_token: None,
                expires_in: Some(60),
            },
            Some(&prev),
        )
        .unwrap();
        assert_eq!(t.refresh_token, "keep-me");
        // 没有 prev 又没回 refresh_token → 报错。
        assert!(to_tokens(
            TokenResponse {
                access_token: "x".into(),
                refresh_token: None,
                expires_in: None,
            },
            None,
        )
        .is_err());
    }

    #[test]
    fn classify_poll_error_follows_rfc8628() {
        assert_eq!(
            classify_poll_error("authorization_pending", None),
            Ok(PollOutcome::Pending)
        );
        assert_eq!(
            classify_poll_error("slow_down", None),
            Ok(PollOutcome::SlowDown)
        );
        assert!(classify_poll_error("access_denied", None).is_err());
        assert!(classify_poll_error("authorization_denied", None).is_err());
        let err = classify_poll_error("expired_token", None).unwrap_err();
        assert!(err.contains("过期"));
        let err = classify_poll_error("weird", Some("detail here")).unwrap_err();
        assert!(err.contains("detail here"));
    }

    #[test]
    fn grok_bases_respect_proxy_env() {
        std::env::remove_var("WC_PROXY_GROK_BASE_URL");
        std::env::remove_var("WC_PROXY_GROK_AUTH_BASE_URL");
        assert_eq!(api_base(), XAI_API_BASE);
        assert_eq!(token_url(), "https://auth.x.ai/oauth2/token");
        assert_eq!(device_code_url(), "https://auth.x.ai/oauth2/device/code");
        std::env::set_var("WC_PROXY_GROK_BASE_URL", "https://cf.worker.dev/grok/v1/");
        std::env::set_var(
            "WC_PROXY_GROK_AUTH_BASE_URL",
            "https://cf.worker.dev/grok-auth",
        );
        assert_eq!(api_base(), "https://cf.worker.dev/grok/v1");
        assert_eq!(token_url(), "https://cf.worker.dev/grok-auth/oauth2/token");
        assert_eq!(
            device_code_url(),
            "https://cf.worker.dev/grok-auth/oauth2/device/code"
        );
        std::env::remove_var("WC_PROXY_GROK_BASE_URL");
        std::env::remove_var("WC_PROXY_GROK_AUTH_BASE_URL");
    }

    #[test]
    fn tokens_expiry_with_margin() {
        let mut t = XaiTokens {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: now_secs() + 3600,
        };
        assert!(!t.is_expired());
        t.expires_at = now_secs() + 60; // 在 2 分钟提前量内 → 视为过期
        assert!(t.is_expired());
    }
}
