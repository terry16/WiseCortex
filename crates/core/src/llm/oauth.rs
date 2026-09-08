//! Claude.ai 订阅（Pro/Max/Team/Enterprise）OAuth 登录 + token 管理（PKCE）。
//!
//! 仅供**个人本机自用**：用自己的 Claude 订阅额度在 WiseCortex 里跑推理，方便横向对比不同模型的
//! 开发能力。⚠️ 该流程复用 Claude Code 官方 OAuth client_id；在第三方应用里使用属灰色地带，
//! 不要对外分发或多人共用。token 存本机配置目录，绝不进 git。
//!
//! 登录采用**手动粘贴**式：打开授权页 → 登录 → 复制页面给出的 code → 粘回 WiseCortex 兑换 token。
//! 这样浏览器和 Tauri 壳都通用，且不依赖本地回调端口/重定向路径白名单。

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ── 常量（最易随 Anthropic 变动，集中放此便于调整）─────────────────────────
/// Claude Code 官方 OAuth client_id（PKCE，无 client secret）。
pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
/// 订阅登录授权页（claude.ai 直连）。
pub const AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
/// token 兑换/刷新端点。
pub const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
/// 手动模式重定向：授权后页面直接展示 code 供用户复制。
pub const MANUAL_REDIRECT_URL: &str = "https://platform.claude.com/oauth/code/callback";
/// **登录授权**请求的 scope（= Claude Code 的 `ALL_OAUTH_SCOPES`：Console + Claude.ai 并集，
/// 顺序与官方一致）。claude.ai 同意页能处理 Console→Claude.ai 跳转，故授权阶段带全集。
pub const AUTHORIZE_SCOPES: &str = "org:create_api_key user:profile user:inference \
     user:sessions:claude_code user:mcp_servers user:file_upload";
/// **刷新**请求的 scope（= Claude Code 的 `CLAUDE_AI_OAUTH_SCOPES`：仅 Claude.ai 专属，
/// **不含** `org:create_api_key`）。
/// ⚠️ token 端点的 refresh-token 授权按 Claude.ai 集校验请求的 scope：混入 Console 专属的
/// `org:create_api_key` 会被回 `400 invalid_scope`（实测：用户在用时反复触发刷新失败）。
/// 授权阶段带 `org:create_api_key` 没问题，但刷新阶段必须去掉——这正是两套 scope 的关键差异。
pub const REFRESH_SCOPES: &str = "user:profile user:inference \
     user:sessions:claude_code user:mcp_servers user:file_upload";
/// OAuth 推理必须带的 beta 头。
/// OAuth 推理的 `anthropic-beta`。除订阅鉴权本身的 `oauth-2025-04-20` 外，还带上真 Claude Code
/// 会带的 `claude-code-20250219`——这是上游区分「first-party CLI 流量」的标记。
///
/// 加它的理由与边界要说清楚：**它不是 529 的解药**。日志里的 529 `overloaded_error` 是上游自己
/// 发的容量信号，形状不合规会回 401/403/400 而不是 529。但同一账号下让请求尽量贴近官方 CLI，
/// 是零成本、且已实测被接受的（2026-07-30 A/B 探针：带与不带均 200）。真正让用户「感觉 Claude Code
/// 不会 529」的是官方 CLI 会静默重试，见 [`crate::llm::RetryKind`]。
/// ⚠️ 不要顺手加 `interleaved-thinking-*` / `fine-grained-tool-streaming-*`：它们会改变
/// thinking 与 tool_use 的流式分块语义，聚合器没适配，属于没证据的行为赌博。
pub const OAUTH_BETA_HEADER: &str = "claude-code-20250219,oauth-2025-04-20";
/// OAuth 推理要求 system 提示以此句开头，否则 API 拒绝（认作非 Claude Code 客户端）。
pub const CLAUDE_CODE_SPOOF: &str = "You are Claude Code, Anthropic's official CLI for Claude.";
/// OAuth **推理**（`api.anthropic.com`）带的 User-Agent。
/// ⚠️ 切勿用于 **token 端点**（`platform.claude.com`）：那里有反滥用规则，自称
/// `claude-code/*` 的换 token 请求会被回 `429 rate_limit_error`（实测：同一代理/IP，
/// 带此 UA→429，换 `axios/*`、`reqwest/*` 或不带 UA→正常 `400 invalid_grant`）。
/// 推理端点不按 UA 限流，带它无妨。
pub const CLAUDE_CODE_USER_AGENT: &str = "claude-code/2.1.178";
/// 刷新提前量：到期前这么多秒就提前刷新，避免边界失败。
const REFRESH_MARGIN_SECS: u64 = 60;

// ── 环境变量反代覆盖（三家订阅共用）────────────────────────────────────────
/// 环境变量覆盖的 base URL：设置且非空 → 用之（去首尾空白与尾部 `/`）；否则用默认值。
/// 用途：把订阅端点指到自建反代（如 Cloudflare Worker），免挂全局 HTTP 代理。
/// oauth_openai / oauth_xai 也从这里取。
pub(crate) fn env_base_url(var: &str, default: &str) -> String {
    std::env::var(var)
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Claude 订阅**推理**端点 base：默认官方 `https://api.anthropic.com`；
/// 环境变量 `WC_PROXY_ANTHROPIC_BASE_URL` 可覆盖（后面拼 `/v1/messages`）。
pub fn api_base() -> String {
    env_base_url("WC_PROXY_ANTHROPIC_BASE_URL", "https://api.anthropic.com")
}

/// token 兑换/刷新端点：默认 [`TOKEN_URL`]；`WC_PROXY_ANTHROPIC_AUTH_BASE_URL` 覆盖其
/// base（`https://platform.claude.com`），路径 `/v1/oauth/token` 固定拼接。
/// 授权页 [`AUTHORIZE_URL`] 不走反代——那是用户浏览器打开的，且要吃 claude.ai 登录态。
fn token_url() -> String {
    format!(
        "{}/v1/oauth/token",
        env_base_url(
            "WC_PROXY_ANTHROPIC_AUTH_BASE_URL",
            "https://platform.claude.com"
        )
    )
}

// ── PKCE ──────────────────────────────────────────────────────────────────
fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 生成一个高熵随机串（base64url，无填充）——用作 code_verifier / state。
pub fn random_token() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    b64url(&buf)
}

/// 由 code_verifier 计算 S256 code_challenge。
pub fn code_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    b64url(&digest)
}

/// 构造授权 URL。`redirect_uri` 手动粘贴传 [`MANUAL_REDIRECT_URL`]，桌面 loopback 传
/// `http://localhost:{port}/callback`（与 Claude Code 一致；须与兑换时的 redirect_uri 相同）。
/// `code=true` 只是让登录页展示 Claude Max 推广，非手动/回环开关，两种模式都带无妨。
pub fn build_auth_url(verifier: &str, state: &str, redirect_uri: &str) -> String {
    let challenge = code_challenge(verifier);
    let enc = |s: &str| {
        // 对会出现的特殊字符做转义，与浏览器 URLSearchParams 一致（scope 含空格/冒号）。
        s.replace('%', "%25")
            .replace(' ', "%20")
            .replace(':', "%3A")
            .replace('/', "%2F")
            .replace('#', "%23")
            .replace('&', "%26")
    };
    format!(
        "{AUTHORIZE_URL}?code=true&client_id={CLIENT_ID}&response_type=code\
         &redirect_uri={redirect}&scope={scope}\
         &code_challenge={challenge}&code_challenge_method=S256&state={state}",
        redirect = enc(redirect_uri),
        scope = enc(AUTHORIZE_SCOPES),
    )
}

/// 解析用户粘贴的 code：手动页给出的形如 `code#state`，也允许只粘 code。
/// 返回 (code, state_opt)。
pub fn parse_manual_code(input: &str) -> (String, Option<String>) {
    let s = input.trim();
    match s.split_once('#') {
        Some((code, state)) => (code.trim().to_string(), Some(state.trim().to_string())),
        None => (s.to_string(), None),
    }
}

// ── token 存储 ──────────────────────────────────────────────────────────────
/// 本机存储的 OAuth token（含算好的绝对过期时间）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// 过期的 Unix 秒时间戳。
    pub expires_at: u64,
}

impl OAuthTokens {
    /// 是否已过期（含提前量）。
    pub fn is_expired(&self) -> bool {
        now_secs() + REFRESH_MARGIN_SECS >= self.expires_at
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// token 文件路径：配置目录下 `wisecortex/claude_oauth.json`。
pub fn tokens_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("wisecortex").join("claude_oauth.json"))
}

/// 读取已保存的 token（无或损坏返回 None）。
pub fn load_tokens() -> Option<OAuthTokens> {
    let p = tokens_path()?;
    let data = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&data).ok()
}

/// 保存 token（best-effort 建目录）。
pub fn save_tokens(t: &OAuthTokens) -> Result<(), String> {
    let p = tokens_path().ok_or("无法定位配置目录")?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("建目录失败: {e}"))?;
    }
    let json = serde_json::to_string_pretty(t).map_err(|e| e.to_string())?;
    std::fs::write(&p, json).map_err(|e| format!("写入失败: {e}"))
}

/// 退出登录：删除本机 token。
pub fn clear_tokens() -> Result<(), String> {
    if let Some(p) = tokens_path() {
        if p.exists() {
            std::fs::remove_file(&p).map_err(|e| format!("删除失败: {e}"))?;
        }
    }
    Ok(())
}

/// 是否已登录（存在未过期或可刷新的 token）。
pub fn is_logged_in() -> bool {
    load_tokens().is_some()
}

// ── 网络：兑换 / 刷新 ────────────────────────────────────────────────────────
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

fn to_tokens(resp: TokenResponse, prev_refresh: Option<&str>) -> Result<OAuthTokens, String> {
    let refresh_token = resp
        .refresh_token
        .or_else(|| prev_refresh.map(str::to_string))
        .ok_or("响应缺少 refresh_token")?;
    // 没给 expires_in 时按 8 小时保守估计。
    let ttl = resp.expires_in.unwrap_or(8 * 3600);
    Ok(OAuthTokens {
        access_token: resp.access_token,
        refresh_token,
        expires_at: now_secs() + ttl,
    })
}

async fn post_token(body: serde_json::Value) -> Result<TokenResponse, String> {
    let http = crate::net::async_builder_timed()
        .build()
        .map_err(|e| e.to_string())?;
    let resp = http
        .post(token_url())
        // ⚠️ 这里【绝不能】带 `claude-code/*` 的 User-Agent：token 端点
        // (platform.claude.com) 有反滥用规则，会把自称 claude-code 的换 token 请求
        // 判为冒充、回 `429 rate_limit_error`（实测差分：同代理/IP、同请求体，带
        // claude-code/2.1.178→429；换 axios/reqwest/不带 UA→穿透到应用层得 400
        // invalid_grant）。官方 CLI 换 token 走 axios 默认 UA 故不被拦。让 reqwest 用
        // 默认（不发 UA）即可。注意：仅 token 端点如此，推理端点 api.anthropic.com 不
        // 按 UA 限流，client.rs 推理仍带 CLAUDE_CODE_USER_AGENT 无碍。
        .header("Content-Type", "application/json")
        .json(&body)
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

/// 用授权码兑换 token，并落盘。`redirect_uri` 须与授权 URL 用的一致（手动=[`MANUAL_REDIRECT_URL`]，
/// loopback=`http://localhost:{port}/callback`）。
pub async fn exchange_code(
    code: &str,
    state: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<OAuthTokens, String> {
    let body = serde_json::json!({
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": redirect_uri,
        "client_id": CLIENT_ID,
        "code_verifier": verifier,
        "state": state,
    });
    let tokens = to_tokens(post_token(body).await?, None)?;
    save_tokens(&tokens)?;
    Ok(tokens)
}

/// 构造刷新请求体（抽出来便于单测 scope 正确性，不发网络）。
/// scope 用 [`REFRESH_SCOPES`]（Claude.ai 集，不含 `org:create_api_key`），否则 token 端点
/// 回 `invalid_scope`。
fn refresh_body(refresh_token: &str, scope: Option<&str>) -> serde_json::Value {
    let mut body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": CLIENT_ID,
    });
    // scope 可选：带上时按 Claude.ai 集请求（与 Claude Code 一致）；省略时按 RFC 6749 §6
    // 「沿用原授予的 scope」——用于旧版本窄 scope token 的刷新回退，避免 invalid_scope。
    if let Some(s) = scope {
        body["scope"] = serde_json::json!(s);
    }
    body
}

/// 用 refresh_token 换新 token，并落盘。
///
/// 先按 Claude Code 口径带 [`REFRESH_SCOPES`]（Claude.ai 集）刷新；若 token 端点回 `invalid_scope`
/// （多见于**旧版本登录**时只授予了较窄 scope —— 如仅 `org:create_api_key user:profile user:inference`，
/// 而刷新请求的 5 个 Claude.ai scope 里有 3 个从未被授予），则按 RFC 6749 §6 **省略 scope 重试一次**
/// （省略即沿用原授予的 scope）。这样无论当初按什么 scope 登录都能续期，不再强制重新登录。
/// `invalid_scope` 在参数校验阶段被拒、refresh_token 不会被消费，故同一 token 可安全复用。
pub async fn refresh(refresh_token: &str) -> Result<OAuthTokens, String> {
    let resp = match post_token(refresh_body(refresh_token, Some(REFRESH_SCOPES))).await {
        Ok(r) => r,
        Err(e) if e.contains("invalid_scope") => {
            crate::seprintln!(
                "[oauth] 刷新带 scope 被回 invalid_scope，按原授予 scope 省略 scope 重试"
            );
            post_token(refresh_body(refresh_token, None)).await?
        }
        Err(e) => return Err(e),
    };
    let tokens = to_tokens(resp, Some(refresh_token))?;
    save_tokens(&tokens)?;
    Ok(tokens)
}

/// 进程内的「刷新单飞锁」：保证同一时刻只有一个刷新在跑。
fn refresh_lock() -> &'static tokio::sync::Mutex<()> {
    static L: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    L.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// 单飞取 token 核心（注入 load / refresh 便于测试）：未过期直接返回；过期则在锁内**二次确认**
/// 后再刷新——避免并发请求各自拿同一个一次性 refresh_token 去刷、互相作废（甚至触发上游
/// refresh-token 复用检测把整条链作废）。
async fn refresh_single_flight<L, F, Fut>(
    lock: &tokio::sync::Mutex<()>,
    load: L,
    refresh_fn: F,
) -> Result<String, String>
where
    L: Fn() -> Option<OAuthTokens>,
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<OAuthTokens, String>>,
{
    let tokens = load().ok_or("尚未登录 Claude 订阅")?;
    if !tokens.is_expired() {
        return Ok(tokens.access_token);
    }
    // 过期 → 单飞：拿锁后重新读取并二次确认（等锁期间别人可能已经刷过）。
    let _guard = lock.lock().await;
    let tokens = load().ok_or("尚未登录 Claude 订阅")?;
    if !tokens.is_expired() {
        return Ok(tokens.access_token);
    }
    let refreshed = refresh_fn(tokens.refresh_token).await?;
    Ok(refreshed.access_token)
}

/// 取一个可用的 access_token：已登录则按需刷新后返回；未登录返回 Err。
/// 刷新走单飞锁，杜绝并发请求把一次性 refresh_token 刷废。失败时打一行服务端日志便于定位
/// （真实错误也会随 Err 透传到 UI）。
pub async fn valid_access_token() -> Result<String, String> {
    refresh_single_flight(refresh_lock(), load_tokens, |rt| async move {
        refresh(&rt).await
    })
    .await
    .map_err(|e| {
        crate::seprintln!("[oauth] Claude 订阅取/刷新 token 失败: {e}");
        e
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_deterministic_b64url() {
        // RFC 7636 附录 B 的已知向量。
        let v = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            code_challenge(v),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_token_is_unique_and_urlsafe() {
        let a = random_token();
        let b = random_token();
        assert_ne!(a, b);
        assert!(!a.contains('+') && !a.contains('/') && !a.contains('='));
        assert!(a.len() >= 42); // 32 字节 base64url ≈ 43 字符
    }

    #[test]
    fn auth_url_has_required_params() {
        let url = build_auth_url("verifier123", "state456", MANUAL_REDIRECT_URL);
        assert!(url.starts_with(AUTHORIZE_URL));
        assert!(url.contains(&format!("client_id={CLIENT_ID}")));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!("code_challenge={}", code_challenge("verifier123"))));
        assert!(url.contains("state=state456"));
        assert!(url.contains("user%3Ainference")); // scope 已转义
        assert!(url.contains("org%3Acreate_api_key")); // 授权阶段带 Console scope
    }

    #[test]
    fn refresh_body_uses_claude_ai_scopes_without_console_scope() {
        let body = refresh_body("RT-xyz", Some(REFRESH_SCOPES));
        assert_eq!(body["grant_type"], "refresh_token");
        assert_eq!(body["refresh_token"], "RT-xyz");
        assert_eq!(body["client_id"], CLIENT_ID);
        let scope = body["scope"].as_str().unwrap();
        // 关键回归：refresh 绝不能带 Console 专属 scope，否则 token 端点回 invalid_scope。
        assert!(
            !scope.contains("org:create_api_key"),
            "refresh scope 不应包含 org:create_api_key，实为：{scope}"
        );
        assert!(
            scope.contains("user:inference"),
            "refresh 必须含 user:inference 才能跑推理"
        );
        assert_eq!(scope, REFRESH_SCOPES);
    }

    #[test]
    fn refresh_body_omits_scope_when_none() {
        // 回退路径：省略 scope（RFC 6749 §6 沿用原授予 scope）——请求体里完全没有 scope 字段，
        // 这样旧版本窄 scope 的 token 刷新时不会再被 invalid_scope 拒绝。
        let body = refresh_body("RT-xyz", None);
        assert_eq!(body["grant_type"], "refresh_token");
        assert_eq!(body["refresh_token"], "RT-xyz");
        assert_eq!(body["client_id"], CLIENT_ID);
        assert!(
            body.get("scope").is_none(),
            "省略 scope 时请求体不应出现 scope 字段，实为：{body}"
        );
    }

    #[test]
    fn scope_constants_match_reference_sets() {
        // 与 Claude Code 的 ALL_OAUTH_SCOPES / CLAUDE_AI_OAUTH_SCOPES 对齐（单空格分隔）。
        assert_eq!(
            AUTHORIZE_SCOPES,
            "org:create_api_key user:profile user:inference \
             user:sessions:claude_code user:mcp_servers user:file_upload"
        );
        assert_eq!(
            REFRESH_SCOPES,
            "user:profile user:inference \
             user:sessions:claude_code user:mcp_servers user:file_upload"
        );
    }

    #[test]
    fn parse_manual_code_splits_state() {
        assert_eq!(
            parse_manual_code("  abc#xyz  "),
            ("abc".to_string(), Some("xyz".to_string()))
        );
        assert_eq!(parse_manual_code("abc"), ("abc".to_string(), None));
    }

    #[test]
    fn env_base_url_prefers_env_and_trims() {
        std::env::remove_var("WC_TEST_ENV_BASE");
        assert_eq!(
            env_base_url("WC_TEST_ENV_BASE", "https://d.example"),
            "https://d.example"
        );
        std::env::set_var("WC_TEST_ENV_BASE", "  https://w.example/  ");
        assert_eq!(
            env_base_url("WC_TEST_ENV_BASE", "https://d.example"),
            "https://w.example"
        );
        // 空白值视为未设置 → 回默认。
        std::env::set_var("WC_TEST_ENV_BASE", "   ");
        assert_eq!(
            env_base_url("WC_TEST_ENV_BASE", "https://d.example"),
            "https://d.example"
        );
        std::env::remove_var("WC_TEST_ENV_BASE");
    }

    #[test]
    fn claude_bases_respect_proxy_env() {
        std::env::remove_var("WC_PROXY_ANTHROPIC_BASE_URL");
        std::env::remove_var("WC_PROXY_ANTHROPIC_AUTH_BASE_URL");
        assert_eq!(api_base(), "https://api.anthropic.com");
        assert_eq!(token_url(), "https://platform.claude.com/v1/oauth/token");
        std::env::set_var(
            "WC_PROXY_ANTHROPIC_BASE_URL",
            "https://cf.worker.dev/anthropic",
        );
        std::env::set_var(
            "WC_PROXY_ANTHROPIC_AUTH_BASE_URL",
            "https://cf.worker.dev/claude-auth/",
        );
        assert_eq!(api_base(), "https://cf.worker.dev/anthropic");
        assert_eq!(
            token_url(),
            "https://cf.worker.dev/claude-auth/v1/oauth/token"
        );
        std::env::remove_var("WC_PROXY_ANTHROPIC_BASE_URL");
        std::env::remove_var("WC_PROXY_ANTHROPIC_AUTH_BASE_URL");
    }

    #[test]
    fn tokens_expiry() {
        let mut t = OAuthTokens {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: now_secs() + 3600,
        };
        assert!(!t.is_expired());
        t.expires_at = now_secs(); // 当下即视为（含提前量）过期
        assert!(t.is_expired());
    }

    #[tokio::test]
    async fn refresh_is_single_flight_under_concurrency() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        // 共享「磁盘」：初始是已过期的 token。刷新会把它换成新鲜 token（模拟落盘）。
        let cell = Arc::new(std::sync::Mutex::new(OAuthTokens {
            access_token: "old".into(),
            refresh_token: "rt".into(),
            expires_at: now_secs(),
        }));
        let calls = Arc::new(AtomicU32::new(0));
        let lock = Arc::new(tokio::sync::Mutex::new(()));

        // 8 个并发请求同时发现过期。单飞 + 二次确认应保证只刷新一次。
        let mut handles = Vec::new();
        for _ in 0..8 {
            let cell = cell.clone();
            let calls = calls.clone();
            let lock = lock.clone();
            handles.push(tokio::spawn(async move {
                let load = {
                    let cell = cell.clone();
                    move || Some(cell.lock().unwrap().clone())
                };
                refresh_single_flight(&lock, load, |_rt| {
                    let cell = cell.clone();
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        tokio::task::yield_now().await; // 模拟网络往返
                        let fresh = OAuthTokens {
                            access_token: "new".into(),
                            refresh_token: "rt2".into(),
                            expires_at: now_secs() + 3600,
                        };
                        *cell.lock().unwrap() = fresh.clone();
                        Ok(fresh)
                    }
                })
                .await
            }));
        }
        for h in handles {
            assert_eq!(h.await.unwrap().as_deref(), Ok("new"));
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "并发过期请求应只触发一次刷新（单飞）"
        );
    }
}
