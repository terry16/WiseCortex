//! OpenAI Codex / ChatGPT **订阅** OAuth（PKCE + 本地回调）。
//! 仅个人本机自用；token 存本机配置目录，绝不进 git。
//!
//! 流程：authorize（auth.openai.com）→ 浏览器登录 → 回调到 `http://localhost:1455/auth/callback`
//! （由 server 临时监听捕获 code）→ **form-urlencoded** 换 token → 从 JWT 取 `chatgpt_account_id`。
//! 推理走 Responses API（[`super::responses`]）打 `chatgpt.com/backend-api/codex`，
//! 头带 `Authorization: Bearer` + `ChatGPT-Account-Id`。

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

// 复用 Claude OAuth 那边的 PKCE 工具（S256 challenge）。
use super::oauth::code_challenge;

// ── 常量（最易随 OpenAI 变动）─────────────────────────────────────────────
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
pub const CALLBACK_PORT: u16 = 1455;
pub const SCOPES: &str = "openid profile email offline_access";
/// 来源标识：OAuth 与推理请求都会带上，授权服务器按已注册的客户端标识放行；属可调项。
pub const ORIGINATOR: &str = "cline";
/// Codex 订阅推理后端（Responses API 的 base，路径再拼 `/responses`）。
pub const CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";

/// 生效的推理 base：默认 [`CODEX_BASE_URL`]；环境变量 `WC_PROXY_OPENAI_BASE_URL`
/// 可覆盖（如 Cloudflare Worker 反代，免全局 HTTP 代理）。
pub fn codex_base_url() -> String {
    super::oauth::env_base_url("WC_PROXY_OPENAI_BASE_URL", CODEX_BASE_URL)
}

/// 生效的 token 端点：`WC_PROXY_OPENAI_AUTH_BASE_URL` 覆盖 base
/// （`https://auth.openai.com`），路径 `/oauth/token` 固定拼接。
fn token_url() -> String {
    format!(
        "{}/oauth/token",
        super::oauth::env_base_url("WC_PROXY_OPENAI_AUTH_BASE_URL", "https://auth.openai.com")
    )
}

const REFRESH_MARGIN_SECS: u64 = 300; // 提前 5 分钟刷新

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 构造授权 URL（本地回调模式）。`verifier`/`state` 由调用方生成（可用 [`super::oauth::random_token`]）。
pub fn build_auth_url(verifier: &str, state: &str) -> String {
    let challenge = code_challenge(verifier);
    let enc = |s: &str| {
        s.replace('%', "%25")
            .replace(' ', "%20")
            .replace(':', "%3A")
            .replace('/', "%2F")
            .replace('#', "%23")
            .replace('&', "%26")
    };
    format!(
        "{AUTHORIZE_URL}?client_id={CLIENT_ID}&redirect_uri={redirect}&scope={scope}\
         &code_challenge={challenge}&code_challenge_method=S256&response_type=code&state={state}\
         &codex_cli_simplified_flow=true&originator={ORIGINATOR}",
        redirect = enc(REDIRECT_URI),
        scope = enc(SCOPES),
    )
}

// ── token 存储 ──────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenAiTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// ChatGPT 账号 id（从 JWT 取），请求时作 `ChatGPT-Account-Id` 头。
    #[serde(default)]
    pub account_id: Option<String>,
    /// 过期的 Unix 秒时间戳。
    pub expires_at: u64,
}

impl OpenAiTokens {
    pub fn is_expired(&self) -> bool {
        now_secs() + REFRESH_MARGIN_SECS >= self.expires_at
    }
}

pub fn tokens_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("wisecortex").join("openai_oauth.json"))
}

pub fn load_tokens() -> Option<OpenAiTokens> {
    let p = tokens_path()?;
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

pub fn save_tokens(t: &OpenAiTokens) -> Result<(), String> {
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

// ── JWT：取 chatgpt_account_id ───────────────────────────────────────────────
fn jwt_payload(token: &str) -> Option<serde_json::Value> {
    let seg = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(seg.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn account_id_from_claims(v: &serde_json::Value) -> Option<String> {
    let nonempty = |s: &str| (!s.is_empty()).then(|| s.to_string());
    if let Some(s) = v.get("chatgpt_account_id").and_then(|x| x.as_str()) {
        if let Some(r) = nonempty(s) {
            return Some(r);
        }
    }
    if let Some(s) = v
        .get("https://api.openai.com/auth")
        .and_then(|a| a.get("chatgpt_account_id"))
        .and_then(|x| x.as_str())
    {
        if let Some(r) = nonempty(s) {
            return Some(r);
        }
    }
    v.get("organizations")
        .and_then(|o| o.as_array())
        .and_then(|a| a.first())
        .and_then(|o| o.get("id"))
        .and_then(|x| x.as_str())
        .and_then(nonempty)
}

/// 优先从 id_token 取，再退到 access_token。
fn extract_account_id(id_token: Option<&str>, access_token: &str) -> Option<String> {
    if let Some(idt) = id_token {
        if let Some(id) = jwt_payload(idt).as_ref().and_then(account_id_from_claims) {
            return Some(id);
        }
    }
    jwt_payload(access_token)
        .as_ref()
        .and_then(account_id_from_claims)
}

// ── 网络：兑换 / 刷新（form-urlencoded）──────────────────────────────────────
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

async fn post_form(params: &[(&str, &str)]) -> Result<TokenResponse, String> {
    let http = crate::net::async_builder_timed()
        .build()
        .map_err(|e| e.to_string())?;
    let resp = http
        .post(token_url())
        .form(params)
        .send()
        .await
        .map_err(|e| crate::net::maybe_transient(&e, format!("请求 token 端点失败: {e}")))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(crate::net::maybe_transient_status(
            status,
            format!("token 端点返回 {status}: {text}"),
        ));
    }
    serde_json::from_str(&text).map_err(|e| format!("解析 token 响应失败: {e}（原文: {text}）"))
}

fn to_tokens(r: TokenResponse, prev: Option<&OpenAiTokens>) -> Result<OpenAiTokens, String> {
    let refresh_token = r
        .refresh_token
        .or_else(|| prev.map(|p| p.refresh_token.clone()))
        .ok_or("响应缺少 refresh_token")?;
    let account_id = extract_account_id(r.id_token.as_deref(), &r.access_token)
        .or_else(|| prev.and_then(|p| p.account_id.clone()));
    let ttl = r.expires_in.unwrap_or(3600);
    Ok(OpenAiTokens {
        access_token: r.access_token,
        refresh_token,
        account_id,
        expires_at: now_secs() + ttl,
    })
}

/// 用授权码换 token 并落盘。
pub async fn exchange_code(code: &str, verifier: &str) -> Result<OpenAiTokens, String> {
    let resp = post_form(&[
        ("grant_type", "authorization_code"),
        ("client_id", CLIENT_ID),
        ("code", code),
        ("redirect_uri", REDIRECT_URI),
        ("code_verifier", verifier),
    ])
    .await?;
    let tokens = to_tokens(resp, None)?;
    save_tokens(&tokens)?;
    Ok(tokens)
}

/// 刷新 token 并落盘。
pub async fn refresh(prev: &OpenAiTokens) -> Result<OpenAiTokens, String> {
    let resp = post_form(&[
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", &prev.refresh_token),
    ])
    .await?;
    let tokens = to_tokens(resp, Some(prev))?;
    save_tokens(&tokens)?;
    Ok(tokens)
}

/// 取 (access_token, account_id)：按需刷新；未登录返回 Err。
pub async fn valid_access() -> Result<(String, Option<String>), String> {
    let tokens = load_tokens().ok_or("尚未登录 ChatGPT 订阅")?;
    let fresh = if tokens.is_expired() {
        refresh(&tokens).await?
    } else {
        tokens
    };
    Ok((fresh.access_token, fresh.account_id))
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
    fn auth_url_has_codex_params() {
        let url = build_auth_url("v", "st");
        assert!(url.starts_with(AUTHORIZE_URL));
        assert!(url.contains(&format!("client_id={CLIENT_ID}")));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("codex_cli_simplified_flow=true"));
        assert!(url.contains("originator=cline"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"));
        assert!(url.contains("state=st"));
    }

    #[test]
    fn account_id_from_root_claim() {
        let jwt = fake_jwt(r#"{"chatgpt_account_id":"acct-root"}"#);
        assert_eq!(extract_account_id(None, &jwt).as_deref(), Some("acct-root"));
    }

    #[test]
    fn account_id_from_nested_auth_claim() {
        let jwt =
            fake_jwt(r#"{"https://api.openai.com/auth":{"chatgpt_account_id":"acct-nested"}}"#);
        assert_eq!(
            extract_account_id(None, &jwt).as_deref(),
            Some("acct-nested")
        );
    }

    #[test]
    fn account_id_from_org() {
        let jwt = fake_jwt(r#"{"organizations":[{"id":"org-1"}]}"#);
        assert_eq!(extract_account_id(None, &jwt).as_deref(), Some("org-1"));
    }

    #[test]
    fn id_token_preferred_over_access() {
        let idt = fake_jwt(r#"{"chatgpt_account_id":"from-id"}"#);
        let act = fake_jwt(r#"{"chatgpt_account_id":"from-access"}"#);
        assert_eq!(
            extract_account_id(Some(&idt), &act).as_deref(),
            Some("from-id")
        );
    }

    #[test]
    fn codex_bases_respect_proxy_env() {
        std::env::remove_var("WC_PROXY_OPENAI_BASE_URL");
        std::env::remove_var("WC_PROXY_OPENAI_AUTH_BASE_URL");
        assert_eq!(codex_base_url(), CODEX_BASE_URL);
        assert_eq!(token_url(), "https://auth.openai.com/oauth/token");
        std::env::set_var("WC_PROXY_OPENAI_BASE_URL", "https://cf.worker.dev/codex/");
        std::env::set_var(
            "WC_PROXY_OPENAI_AUTH_BASE_URL",
            "https://cf.worker.dev/oai-auth",
        );
        assert_eq!(codex_base_url(), "https://cf.worker.dev/codex");
        assert_eq!(token_url(), "https://cf.worker.dev/oai-auth/oauth/token");
        std::env::remove_var("WC_PROXY_OPENAI_BASE_URL");
        std::env::remove_var("WC_PROXY_OPENAI_AUTH_BASE_URL");
    }

    #[test]
    fn tokens_expiry_with_margin() {
        let mut t = OpenAiTokens {
            access_token: "a".into(),
            refresh_token: "r".into(),
            account_id: None,
            expires_at: now_secs() + 3600,
        };
        assert!(!t.is_expired());
        t.expires_at = now_secs() + 60; // 在 5 分钟提前量内 → 视为过期
        assert!(t.is_expired());
    }
}
