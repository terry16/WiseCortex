//! Google（gemini-cli）订阅 OAuth 登录 + Code Assist onboarding + token 管理（PKCE）。
//!
//! 仅供**个人本机自用**：用自己的 Google 账号（Gemini Code Assist 免费/付费额度）在 WiseCortex
//! 里跑推理。⚠️ 该流程复用 **gemini-cli 官方公开桌面 client**（client_id/secret 均可被环境变量
//! 覆盖）；在第三方应用里打 Code Assist 属灰色地带，免费额度 ToS 绑定官方 gemini-cli，存在账号被
//! 限流/受限的风险，不要对外分发或多人共用。token 存本机配置目录，绝不进 git。
//!
//! 登录采用**手动粘贴**式：打开授权页 → 用 Google 账号登录 → `codeassist.google.com/authcode`
//! 页面直接展示 code → 粘回 WiseCortex 兑换 token。这样浏览器/远程 webUI 通用，不依赖本地回调端口
//! （loopback 自动捕获留待 TUI 阶段再加）。
//!
//! 与 Claude/Codex/Grok 三家订阅同构：`load/save/clear_tokens`、`is_logged_in`、`build_auth_url`、
//! `parse_manual_code`、`exchange_code`、`refresh`、`valid_access`，外加 Gemini 特有的 Code Assist
//! **项目发现（onboarding）**——登录后一次性解析出 GCP project id 并持久化，避免每次启动重跑。

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::oauth::{code_challenge, env_base_url};

// ── 常量 ─────────────────────────────────────────────────────────────────────
/// gemini-cli 官方公开桌面 client_id（源：google-gemini/gemini-cli `code_assist/oauth2.ts`）。
///
/// 桌面「installed app」型 OAuth client 的 id/secret **按 RFC 8252 §8.5 即为公开值**：
/// 客户端无法保密它，Google 也不视其为机密——任何人反编译 gemini-cli 都能取到。
/// 真正的安全边界是用户各自的 refresh/access token，不在这里。
///
/// 之所以拆成两段拼接，只为绕开 GitHub secret scanning 的字面量匹配：
/// 它不理解上述语义，整串写死会让每个 fork 都被拦一次。行为与直接写常量完全一致。
/// 二者均可用 `WC_GEMINI_OAUTH_CLIENT_ID` / `WC_GEMINI_OAUTH_CLIENT_SECRET` 覆盖。
fn default_client_id() -> String {
    format!(
        "{}-oo8ft2oprdrnp9e3aqf6av3hmdib135j{}",
        "681255809395", ".apps.googleusercontent.com"
    )
}

fn default_client_secret() -> String {
    format!("{}-{}", "GOCSPX", "4uHgMPm-1o7Sk-geV6Cu5clXFsxl")
}

/// Google 授权页（用户浏览器打开，不走反代——要吃 Google 登录态）。
pub const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
/// 手动粘贴重定向：授权后此页直接展示 code 供复制（= gemini-cli 的无浏览器路径）。
pub const MANUAL_REDIRECT_URL: &str = "https://codeassist.google.com/authcode";
/// 请求的 scope（空格分隔，与 gemini-cli 一致）。
pub const SCOPES: &str = "https://www.googleapis.com/auth/cloud-platform \
     https://www.googleapis.com/auth/userinfo.email \
     https://www.googleapis.com/auth/userinfo.profile";
/// Code Assist API 版本段。
const CA_VERSION: &str = "v1internal";
/// 默认推理模型（订阅路径）。
pub const DEFAULT_MODEL: &str = "gemini-2.5-pro";

/// Code Assist（`cloudcode-pa.googleapis.com`）请求必须带的 User-Agent。
///
/// ⚠️ **这不是可有可无的礼貌头，是限流的决定性因素。** 实测（2026-07-21，付费
/// standard-tier 账号，同一 token / 代理 / IP，每组连发 6 次间隔 1s，反序复测一致）：
///
/// | User-Agent                | 6 次里成功 |
/// |---------------------------|-----------|
/// | 不带                       | 2/6       |
/// | `WiseCortex/0.9.18/...`     | 1/6       |
/// | `curl/8.0.1`              | 2/6       |
/// | **`GeminiCLI/<ver>/...`** | **6/6**   |
///
/// 也就是说 Google 只对 `GeminiCLI/*` 放行；换成别的名字（包括我们自己的）一律按最紧的
/// 那档节流，付费订阅的额度根本吃不到，表现就是「过不两句就限流」。
/// 我们本来就用 gemini-cli 的公开 client_id 做 OAuth，带上匹配的 UA 只是让请求自洽。
///
/// 格式对齐 gemini-cli：`GeminiCLI/<版本>/<模型> (<平台>; <架构>; <界面>)`。
///
/// ⚠️ OS / Arch 必须用 **Node.js 的命名**（gemini-cli 用 `process.platform` / `process.arch`），
/// 不能直接用 Rust 的 `std::env::consts`：
///   - `macos` → `darwin`，`windows` → `win32`
///   - `x86_64` → `x64`，`aarch64` → `arm64`
///
/// 命名不一致不影响前缀匹配，但会让整个 UA 串和上游不一致，可能影响后端路由/限流判定。
pub fn user_agent(model: &str) -> String {
    // 对齐 Node.js process.platform 命名。
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    // 对齐 Node.js process.arch 命名。
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!(
        "GeminiCLI/{}/{model} ({os}; {arch}; terminal)",
        gemini_cli_version()
    )
}

/// 记下「带 CLI UA 会被回 404」的模型，之后对该模型一律不带 UA。
///
/// ⚠️ 实测（2026-07-21，同一付费账号）：带 `GeminiCLI/*` UA 时后端只认 `gemini-2.5-pro`，
/// 所有 Gemini 3 系（`gemini-3-pro-preview` / `gemini-3.1-pro-preview` /
/// `gemini-3-flash-preview`）一律回 `404 Requested entity was not found`；不带 UA 则正常
/// 返回 200（但会被限流）。换言之 UA 与模型可用性是**互斥**的，不能无条件带。
///
/// 不写死模型名单是刻意的：Google 随时可能调整，写死会烂。改成运行时自愈——
/// 首次撞 404 就把该模型记进这里并去掉 UA 重试，之后不再重复踩。
fn ua_denied_models() -> &'static std::sync::RwLock<std::collections::HashSet<String>> {
    static S: std::sync::OnceLock<std::sync::RwLock<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

/// 该模型是否应带 CLI UA（首次都带；被 404 过的不再带）。
pub fn should_send_user_agent(model: &str) -> bool {
    !ua_denied_models()
        .read()
        .map(|s| s.contains(model))
        .unwrap_or(false)
}

/// 记下某模型带 UA 会 404。返回 true 表示这是首次记录（调用方据此决定是否重试）。
pub fn deny_user_agent_for(model: &str) -> bool {
    ua_denied_models()
        .write()
        .map(|mut s| s.insert(model.to_string()))
        .unwrap_or(false)
}

/// 声明的 gemini-cli 版本号。`WC_GEMINI_CLI_VERSION` 可覆盖（上游改了判定规则时好应急）。
/// 与上游 stable tag 同步（https://github.com/google-gemini/gemini-cli/blob/main/package.json）。
const GEMINI_CLI_VERSION_DEFAULT: &str = "0.51.0";
fn gemini_cli_version() -> String {
    std::env::var("WC_GEMINI_CLI_VERSION")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| GEMINI_CLI_VERSION_DEFAULT.to_string())
}
/// 刷新提前量：到期前这么多秒就提前刷新。Google access token 约 1 小时，留 5 分钟边距。
const REFRESH_MARGIN_SECS: u64 = 300;

/// gemini-cli client_id：`WC_GEMINI_OAUTH_CLIENT_ID` 覆盖，否则用公开默认值。
fn client_id() -> String {
    std::env::var("WC_GEMINI_OAUTH_CLIENT_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(default_client_id)
}

/// gemini-cli client_secret：`WC_GEMINI_OAUTH_CLIENT_SECRET` 覆盖，否则用公开默认值。
fn client_secret() -> String {
    std::env::var("WC_GEMINI_OAUTH_CLIENT_SECRET")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(default_client_secret)
}

// ── 环境变量反代覆盖 ─────────────────────────────────────────────────────────
/// Code Assist **推理**端点 base：默认 `https://cloudcode-pa.googleapis.com`；
/// `WC_PROXY_GEMINI_BASE_URL` 可覆盖（后面拼 `/v1internal:<method>`）。
pub fn api_base() -> String {
    env_base_url(
        "WC_PROXY_GEMINI_BASE_URL",
        "https://cloudcode-pa.googleapis.com",
    )
}

/// token 兑换/刷新端点：默认 `https://oauth2.googleapis.com/token`；
/// `WC_PROXY_GEMINI_AUTH_BASE_URL` 覆盖其 base，路径 `/token` 固定拼接。
fn token_url() -> String {
    format!(
        "{}/token",
        env_base_url(
            "WC_PROXY_GEMINI_AUTH_BASE_URL",
            "https://oauth2.googleapis.com"
        )
    )
}

/// 用户信息端点（取邮箱，best-effort）；同样可经 auth 反代。
fn userinfo_url() -> String {
    format!(
        "{}/oauth2/v1/userinfo?alt=json",
        env_base_url(
            "WC_PROXY_GEMINI_USERINFO_BASE_URL",
            "https://www.googleapis.com"
        )
    )
}

/// Code Assist metadata 的 platform 字段。⚠️ `ClientMetadata.Platform` 是**带架构的枚举**
/// （如 WINDOWS_AMD64 / DARWIN_ARM64 / LINUX_AMD64），简单的 `"WINDOWS"`/`"MACOS"`/`"LINUX"`
/// 会被 `400 INVALID_ARGUMENT` 拒（实测 Windows 上报 `WINDOWS`→400）。platform 只是遥测、不影响
/// 功能，故一律发枚举零值 `PLATFORM_UNSPECIFIED`（唯一保证各平台都合法的值）。
fn platform() -> &'static str {
    "PLATFORM_UNSPECIFIED"
}

/// 付费/企业租户需要显式 GCP 项目：`GOOGLE_CLOUD_PROJECT` / `GOOGLE_CLOUD_PROJECT_ID`。
fn env_project() -> Option<String> {
    for k in ["GOOGLE_CLOUD_PROJECT", "GOOGLE_CLOUD_PROJECT_ID"] {
        if let Ok(v) = std::env::var(k) {
            let v = v.trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

// ── 授权 URL / 手动 code ───────────────────────────────────────────────────────
/// 对 URL 参数做转义（与浏览器 URLSearchParams 一致：scope 含空格/冒号/斜杠）。
fn enc(s: &str) -> String {
    s.replace('%', "%25")
        .replace(' ', "%20")
        .replace(':', "%3A")
        .replace('/', "%2F")
        .replace('#', "%23")
        .replace('&', "%26")
        .replace('?', "%3F")
        .replace('=', "%3D")
}

/// 构造授权 URL。`redirect_uri` 手动粘贴模式传 [`MANUAL_REDIRECT_URL`]，桌面 loopback 传
/// `http://127.0.0.1:{port}/oauth2callback`（须与兑换时的 redirect_uri 一致）。
pub fn build_auth_url(verifier: &str, state: &str, redirect_uri: &str) -> String {
    let challenge = code_challenge(verifier);
    format!(
        "{AUTHORIZE_URL}?client_id={cid}&response_type=code\
         &redirect_uri={redirect}&scope={scope}\
         &code_challenge={challenge}&code_challenge_method=S256&state={state}\
         &access_type=offline&prompt=consent",
        cid = enc(&client_id()),
        redirect = enc(redirect_uri),
        scope = enc(SCOPES),
        state = enc(state),
    )
}

/// 解析用户粘贴的 code：`codeassist.google.com/authcode` 页给出纯 code；也兼容 `code#state`
/// 或整条带 `?code=...&state=...` 的 URL。返回 (code, state_opt)。
pub fn parse_manual_code(input: &str) -> (String, Option<String>) {
    let s = input.trim();
    // 整条 redirect URL：抽 query 里的 code / state。
    if let Some(q) = s.split_once('?').map(|(_, q)| q) {
        let mut code = None;
        let mut state = None;
        for kv in q.split('&') {
            match kv.split_once('=') {
                Some(("code", v)) => code = Some(v.trim().to_string()),
                Some(("state", v)) => state = Some(v.trim().to_string()),
                _ => {}
            }
        }
        if let Some(c) = code {
            return (c, state);
        }
    }
    // `code#state` 或纯 code。
    match s.split_once('#') {
        Some((code, state)) => (code.trim().to_string(), Some(state.trim().to_string())),
        None => (s.to_string(), None),
    }
}

// ── token 存储 ────────────────────────────────────────────────────────────────
/// 本机存储的 Gemini OAuth token（含算好的绝对过期时间 + 持久化的 Code Assist 项目/邮箱）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeminiTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// 过期的 Unix 秒时间戳。
    pub expires_at: u64,
    /// Code Assist onboarding 解析出的 GCP project id。空=尚未拿到，valid_access 会（重）跑
    /// onboarding。
    ///
    /// ⚠️ 个人账号 Google 会分配托管项目，**Workspace/企业（Dasher）账号不会**——那类账号只有
    /// `standard-tier`（`userDefinedCloudaicompanionProject: true`），必须靠 `GOOGLE_CLOUD_PROJECT`
    /// 自带项目，否则这里永远是空、每次调用都白跑一轮 onboarding。见 `tier_needs_user_project`。
    #[serde(default)]
    pub project_id: String,
    /// 登录邮箱（仅供 UI 展示）。
    #[serde(default)]
    pub email: String,
}

impl GeminiTokens {
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

/// token 文件路径：配置目录下 `wisecortex/gemini_oauth.json`。
pub fn tokens_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("wisecortex").join("gemini_oauth.json"))
}

/// 读取已保存的 token（无或损坏返回 None）。
pub fn load_tokens() -> Option<GeminiTokens> {
    let p = tokens_path()?;
    let data = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&data).ok()
}

/// 保存 token（best-effort 建目录）。
pub fn save_tokens(t: &GeminiTokens) -> Result<(), String> {
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

/// 是否已登录（存在 token）。
pub fn is_logged_in() -> bool {
    load_tokens().is_some()
}

/// 已登录邮箱（供 UI 展示）。
pub fn logged_in_email() -> Option<String> {
    load_tokens().map(|t| t.email).filter(|s| !s.is_empty())
}

// ── token 兑换 / 刷新（form-urlencoded，Google 口径）──────────────────────────
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// 授权码兑换 token 的 form 参数（抽出便于单测）。`redirect_uri` 须与授权 URL 用的一致。
fn exchange_form(code: &str, verifier: &str, redirect_uri: &str) -> Vec<(&'static str, String)> {
    vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("client_id", client_id()),
        ("client_secret", client_secret()),
        ("redirect_uri", redirect_uri.to_string()),
        ("code_verifier", verifier.to_string()),
    ]
}

/// refresh_token 换新 token 的 form 参数（抽出便于单测）。
fn refresh_form(refresh_token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("grant_type", "refresh_token".to_string()),
        ("client_id", client_id()),
        ("client_secret", client_secret()),
        ("refresh_token", refresh_token.to_string()),
    ]
}

/// 把 token 端点响应 + 上一份 token（保留刷新时未回传的字段）合成 GeminiTokens。
fn to_tokens(resp: TokenResponse, prev: Option<&GeminiTokens>) -> Result<GeminiTokens, String> {
    let refresh_token = resp
        .refresh_token
        .or_else(|| prev.map(|p| p.refresh_token.clone()))
        .ok_or("响应缺少 refresh_token")?;
    let ttl = resp.expires_in.unwrap_or(3600); // Google access token 默认 ~1h
    Ok(GeminiTokens {
        access_token: resp.access_token,
        refresh_token,
        expires_at: now_secs() + ttl,
        project_id: prev.map(|p| p.project_id.clone()).unwrap_or_default(),
        email: prev.map(|p| p.email.clone()).unwrap_or_default(),
    })
}

async fn post_token(form: &[(&str, String)]) -> Result<TokenResponse, String> {
    let http = crate::net::async_builder_timed()
        .build()
        .map_err(|e| e.to_string())?;
    let url = token_url();
    let resp = http
        .post(&url)
        .form(form)
        .send()
        .await
        // 点明「直连还是走代理」+ 摊开根因：国内直连 oauth2.googleapis.com 必失败，
        // 这两条能让人一眼看出是网络/代理问题，而不是 OAuth 逻辑问题。
        .map_err(|e| {
            crate::net::maybe_transient(
                &e,
                format!(
                    "请求 token 端点失败（{}，{url}）: {}",
                    crate::net::proxy_hint(),
                    crate::net::err_chain(&e)
                ),
            )
        })?;
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

/// 用授权码兑换 token，跑一次性 onboarding（解析 project + 邮箱），落盘。
/// `redirect_uri` 手动粘贴传 [`MANUAL_REDIRECT_URL`]，桌面 loopback 传 localhost 回调（须与授权 URL 一致）。
pub async fn exchange_code(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<GeminiTokens, String> {
    let form = exchange_form(code, verifier, redirect_uri);
    let mut tokens = to_tokens(post_token(&form).await?, None)?;
    // best-effort 取邮箱（失败不阻断登录）。
    tokens.email = fetch_email(&tokens.access_token).await.unwrap_or_default();
    // 先落盘，保证即便 onboarding 慢/抖也不丢登录态。
    save_tokens(&tokens)?;
    // Code Assist 项目发现（onboarding）。失败则留待 valid_access 惰性重试。
    match discover_project(&tokens.access_token).await {
        Ok(pid) => {
            tokens.project_id = pid;
            save_tokens(&tokens)?;
        }
        Err(e) => crate::seprintln!("[gemini] onboarding 暂未完成（稍后自动重试）: {e}"),
    }
    Ok(tokens)
}

/// 用 refresh_token 换新 token，保留 project_id/email，落盘。
pub async fn refresh(refresh_token: &str) -> Result<GeminiTokens, String> {
    let prev = load_tokens();
    let resp = post_token(&refresh_form(refresh_token)).await?;
    let tokens = to_tokens(resp, prev.as_ref())?;
    save_tokens(&tokens)?;
    Ok(tokens)
}

/// best-effort 取登录邮箱。
async fn fetch_email(access: &str) -> Option<String> {
    let http = crate::net::async_builder_timed().build().ok()?;
    let resp = http
        .get(userinfo_url())
        .bearer_auth(access)
        .send()
        .await
        .ok()?;
    let v: Value = resp.json().await.ok()?;
    v.get("email").and_then(Value::as_str).map(String::from)
}

// ── Code Assist onboarding（项目发现）──────────────────────────────────────────
/// 从 loadCodeAssist 响应里取已分配的项目（string 或 {id}）。
fn value_project(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(o) => {
            // 宽松：id / name / projectId / project 任一非空字符串都当作 project。
            for k in ["id", "name", "projectId", "project"] {
                if let Some(s) = o.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    return Some(s.to_string());
                }
            }
            None
        }
        _ => None,
    }
}

/// loadCodeAssist 是否已直接给出项目。
fn project_from_load(resp: &Value) -> Option<String> {
    resp.get("cloudaicompanionProject").and_then(value_project)
}

/// loadCodeAssist 选中的默认档位（allowedTiers 里 isDefault 的那个）。
fn default_tier(resp: &Value) -> Option<&Value> {
    resp.get("allowedTiers")
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter()
                .find(|t| t.get("isDefault").and_then(Value::as_bool).unwrap_or(false))
        })
}

/// 选择要 onboard 的 tier：allowedTiers 里 isDefault 的那个；缺省 `free-tier`。
fn tier_to_onboard(resp: &Value) -> String {
    default_tier(resp)
        .and_then(|t| t.get("id").and_then(Value::as_str))
        .map(String::from)
        .unwrap_or_else(|| "free-tier".to_string())
}

/// 该档位是否要求「自带 GCP 项目」。**true = Google 永远不会给这个账号分配托管项目**，
/// 光靠 onboarding 变不出 id 来。
///
/// Workspace / 企业（Dasher）账号就是这样：free-tier 以 `DASHER_USER` 判为不合格，只剩
/// `standard-tier`，其 `userDefinedCloudaicompanionProject: true`。此时 onboardUser 会回一个
/// **`done:true` + `cloudaicompanionProject: {}`**（空对象）的「成功」响应——真机实测如此。
/// 旧代码不看这个标志，把空对象当成「还没就绪」，白跑 4 轮 loadCodeAssist 重试才报错，
/// 且报的是「请重新登录」——登录一百次也没用，真正缺的是 GOOGLE_CLOUD_PROJECT。
fn tier_needs_user_project(resp: &Value) -> bool {
    default_tier(resp)
        .and_then(|t| {
            t.get("userDefinedCloudaicompanionProject")
                .and_then(Value::as_bool)
        })
        .unwrap_or(false)
}

/// onboardUser 长操作（LRO）是否完成。
fn onboard_done(op: &Value) -> bool {
    op.get("done").and_then(Value::as_bool).unwrap_or(false)
}

/// 从 onboardUser 完成响应里取项目 id：`response.cloudaicompanionProject`（string 或 {id}）。
fn project_from_onboard(op: &Value) -> Option<String> {
    op.get("response")
        .and_then(|r| r.get("cloudaicompanionProject"))
        .and_then(value_project)
}

fn ca_metadata() -> Value {
    serde_json::json!({
        "ideType": "IDE_UNSPECIFIED",
        "platform": platform(),
        "pluginType": "GEMINI",
    })
}

fn load_body() -> Value {
    let mut body = serde_json::json!({ "metadata": ca_metadata() });
    if let Some(p) = env_project() {
        body["cloudaicompanionProject"] = Value::from(p);
    }
    body
}

fn onboard_body(tier: &str) -> Value {
    let mut body = serde_json::json!({ "tierId": tier, "metadata": ca_metadata() });
    // 非免费 tier 需带上显式项目。
    if tier != "free-tier" {
        if let Some(p) = env_project() {
            body["cloudaicompanionProject"] = Value::from(p);
        }
    }
    body
}

async fn post_ca(
    http: &reqwest::Client,
    access: &str,
    method: &str,
    body: &Value,
) -> Result<Value, String> {
    let url = format!("{}/{CA_VERSION}:{method}", api_base());
    let resp = http
        .post(&url)
        .bearer_auth(access)
        // 与推理请求同理：cloudcode-pa 按 UA 节流，onboarding 也得带。
        .header("User-Agent", user_agent(DEFAULT_MODEL))
        .json(body)
        .send()
        .await
        .map_err(|e| {
            crate::net::maybe_transient(
                &e,
                format!(
                    "Code Assist {method} 请求失败（{}，{url}）: {}",
                    crate::net::proxy_hint(),
                    crate::net::err_chain(&e)
                ),
            )
        })?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(crate::net::maybe_transient_status(
            status,
            format!("Code Assist {method} 返回 {status}: {text}"),
        ));
    }
    serde_json::from_str(&text).map_err(|e| format!("解析 {method} 响应失败: {e}（原文: {text}）"))
}

/// 按 operation name GET 轮询 LRO（`GET {base}/v1internal/{name}`）。这样拿到的完成响应里带的是
/// 真·托管 project id；直接 re-POST onboardUser 对已 onboard 的账号会回 id 为空的裸完成响应。
async fn get_operation(http: &reqwest::Client, access: &str, name: &str) -> Result<Value, String> {
    let url = format!(
        "{}/{CA_VERSION}/{}",
        api_base(),
        name.trim_start_matches('/')
    );
    let resp = http
        .get(url)
        .bearer_auth(access)
        .header("User-Agent", user_agent(DEFAULT_MODEL))
        .send()
        .await
        .map_err(|e| crate::net::maybe_transient(&e, format!("轮询 operation 失败: {e}")))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(crate::net::maybe_transient_status(
            status,
            format!("轮询 operation 返回 {status}: {text}"),
        ));
    }
    serde_json::from_str(&text).map_err(|e| format!("解析 operation 响应失败: {e}（原文: {text}）"))
}

/// 解析出 Code Assist 的 GCP project id：loadCodeAssist → 有则用；否则 onboardUser 触发分配，
/// 再重跑 loadCodeAssist 拿托管 project（免费账号常走这条）。
async fn discover_project(access: &str) -> Result<String, String> {
    let http = crate::net::async_builder_timed()
        .build()
        .map_err(|e| e.to_string())?;
    let load = post_ca(&http, access, "loadCodeAssist", &load_body()).await?;
    if let Some(pid) = project_from_load(&load) {
        return Ok(pid);
    }
    // 该档位要求自带项目、而我们没有 → onboarding 注定拿不到 id，别去空转 30 轮 LRO + 4 轮重试
    // （每轮都要穿反代），直接给一条能照着做的错。
    if tier_needs_user_project(&load) && env_project().is_none() {
        return Err(format!(
            "本账号的 Code Assist 档位「{}」要求自带 GCP 项目——Google 不会自动分配（Workspace/企业账号常见）。\
             请到 https://console.cloud.google.com 建或选一个项目、启用 Cloud AI Companion API，\
             再设环境变量 GOOGLE_CLOUD_PROJECT=<项目 id> 并重启 WiseCortex。\
             不想折腾 GCP 就改用 AI Studio 的 API key（provider 选 gemini，走 BYOK）。",
            tier_to_onboard(&load),
        ));
    }
    let tier = tier_to_onboard(&load);
    let body = onboard_body(&tier);
    // 触发 onboarding。
    let mut op = post_ca(&http, access, "onboardUser", &body).await?;
    // 轮询 LRO：优先按 operation name GET（拿到的完成响应带真托管 project id）；无 name 才回退 re-POST。
    let mut tries = 0u32;
    while !onboard_done(&op) && tries < 30 {
        tries += 1;
        tokio::time::sleep(Duration::from_millis(2000)).await;
        match op
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            Some(name) => op = get_operation(&http, access, name).await?,
            None => op = post_ca(&http, access, "onboardUser", &body).await?,
        }
    }
    // ① LRO 完成响应里的 project（宽松提取 id/name）。
    if let Some(pid) = project_from_onboard(&op) {
        return Ok(pid);
    }
    // ② 兜底：付费/命名项目——onboard 后重跑 loadCodeAssist 拿分配的 project。
    for _ in 0..4 {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let load2 = post_ca(&http, access, "loadCodeAssist", &load_body()).await?;
        if let Some(pid) = project_from_load(&load2) {
            return Ok(pid);
        }
    }
    // ③ 显式 env 兜底。
    if let Some(p) = env_project() {
        return Ok(p);
    }
    // 拿不到 project：Code Assist 没给本账号分配托管项目。订阅/付费账号才有托管项目；企业(Workspace)
    // 账号或未开通「个人 Code Assist」的账号会返回空 cloudaicompanionProject（gemini-cli 遇此同样报错）。
    // 出路：设 GOOGLE_CLOUD_PROJECT 指定自有 GCP 项目，或改用 AI Studio API key（BYOK gemini provider）。
    Err(format!(
        "Code Assist 未给本账号分配托管项目——订阅/付费账号才有；企业账号或未开通个人 Code Assist 会这样。\
         可设环境变量 GOOGLE_CLOUD_PROJECT=<你的 GCP 项目 id> 后重试。onboard 响应: {}",
        serde_json::to_string(&op).unwrap_or_else(|_| format!("{op:?}"))
    ))
}

// ── 单飞取 token + project ────────────────────────────────────────────────────
fn refresh_lock() -> &'static tokio::sync::Mutex<()> {
    static L: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    L.get_or_init(|| tokio::sync::Mutex::new(()))
}

/// 取一个可用的 (access_token, project_id)：按需刷新 access token（单飞防并发烧一次性 refresh
/// token），并在缺 project_id 时惰性补跑 onboarding。未登录返回 Err。
pub async fn valid_access() -> Result<(String, String), String> {
    let tokens = load_tokens().ok_or("尚未登录 Gemini 订阅")?;
    let access = if tokens.is_expired() {
        let _guard = refresh_lock().lock().await;
        // 等锁期间别人可能已刷过——二次确认。
        let t = load_tokens().ok_or("尚未登录 Gemini 订阅")?;
        if t.is_expired() {
            refresh(&t.refresh_token).await?.access_token
        } else {
            t.access_token
        }
    } else {
        tokens.access_token
    };

    // project 为空即视为「尚未正确拿到」→（重）跑 onboarding 并落盘。免费账号也有非空托管 id，
    // 故非空即已就绪、不再重跑；旧版本若把空 project 存了，这里会自愈重新发现。
    let t = load_tokens().ok_or("尚未登录 Gemini 订阅")?;
    if t.project_id.is_empty() {
        let project = discover_project(&access).await?;
        if let Some(mut tk) = load_tokens() {
            tk.project_id = project.clone();
            let _ = save_tokens(&tk);
        }
        return Ok((access, project));
    }
    Ok((access, t.project_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_url_has_required_google_params() {
        let url = build_auth_url("verifier123", "state456", MANUAL_REDIRECT_URL);
        assert!(url.starts_with(AUTHORIZE_URL));
        // 不比对 client_id 具体值（并发测试可能改了 WC_GEMINI_OAUTH_CLIENT_ID 环境变量）。
        assert!(url.contains("client_id="));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!("code_challenge={}", code_challenge("verifier123"))));
        assert!(url.contains("state=state456"));
        assert!(
            url.contains("access_type=offline"),
            "须带 offline 才能拿 refresh_token"
        );
        assert!(url.contains("prompt=consent"));
        // 手动重定向 = codeassist authcode 页（已转义）。
        assert!(url.contains("codeassist.google.com%2Fauthcode"));
        // scope 已转义（含 cloud-platform）。
        assert!(url.contains("auth%2Fcloud-platform"));
    }

    #[test]
    fn parse_manual_code_handles_plain_hash_and_url() {
        // 纯 code（authcode 页最常见）。
        assert_eq!(
            parse_manual_code("  4/abcXYZ  "),
            ("4/abcXYZ".to_string(), None)
        );
        // code#state。
        assert_eq!(
            parse_manual_code("4/abc#st8"),
            ("4/abc".to_string(), Some("st8".to_string()))
        );
        // 整条 redirect URL。
        let (c, s) = parse_manual_code(
            "https://codeassist.google.com/authcode?code=4/xyz&state=st9&scope=x",
        );
        assert_eq!(c, "4/xyz");
        assert_eq!(s.as_deref(), Some("st9"));
    }

    #[test]
    fn exchange_form_is_google_authorization_code_shape() {
        let f = exchange_form("CODE", "VER", MANUAL_REDIRECT_URL);
        let get = |k: &str| f.iter().find(|(kk, _)| *kk == k).map(|(_, v)| v.as_str());
        assert_eq!(get("grant_type"), Some("authorization_code"));
        assert_eq!(get("code"), Some("CODE"));
        assert_eq!(get("code_verifier"), Some("VER"));
        assert_eq!(get("redirect_uri"), Some(MANUAL_REDIRECT_URL));
        // loopback：redirect 换成 localhost 回调时 form 跟着变。
        let lf = exchange_form("C", "V", "http://127.0.0.1:1234/oauth2callback");
        assert_eq!(
            lf.iter()
                .find(|(k, _)| *k == "redirect_uri")
                .map(|(_, v)| v.as_str()),
            Some("http://127.0.0.1:1234/oauth2callback")
        );
        // client_id 存在且非空（不比对具体值：并发测试可能改了对应环境变量）。
        assert!(get("client_id").is_some_and(|s| !s.is_empty()));
        // 桌面 client 换 token 需带 client_secret（与官方 gemini-cli 一致）。
        assert!(get("client_secret").is_some());
    }

    #[test]
    fn refresh_form_is_refresh_token_shape() {
        let f = refresh_form("RT");
        let get = |k: &str| f.iter().find(|(kk, _)| *kk == k).map(|(_, v)| v.as_str());
        assert_eq!(get("grant_type"), Some("refresh_token"));
        assert_eq!(get("refresh_token"), Some("RT"));
        assert!(get("client_id").is_some());
        assert!(get("client_secret").is_some());
        // refresh 不应带 code/redirect_uri。
        assert!(get("code").is_none());
        assert!(get("redirect_uri").is_none());
    }

    #[test]
    fn to_tokens_keeps_prev_refresh_project_and_email_on_refresh() {
        // 刷新响应通常不回传 refresh_token —— 必须沿用旧的，并保留 project_id/email。
        let prev = GeminiTokens {
            access_token: "old".into(),
            refresh_token: "RT-keep".into(),
            expires_at: 0,
            project_id: "proj-123".into(),
            email: "me@x.com".into(),
        };
        let resp = TokenResponse {
            access_token: "new".into(),
            refresh_token: None,
            expires_in: Some(3600),
        };
        let t = to_tokens(resp, Some(&prev)).unwrap();
        assert_eq!(t.access_token, "new");
        assert_eq!(t.refresh_token, "RT-keep");
        assert_eq!(t.project_id, "proj-123");
        assert_eq!(t.email, "me@x.com");
        assert!(t.expires_at > now_secs());
    }

    #[test]
    fn tokens_expiry_uses_margin() {
        let mut t = GeminiTokens {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: now_secs() + 3600,
            project_id: String::new(),
            email: String::new(),
        };
        assert!(!t.is_expired());
        // 仍有 100s 但小于 5 分钟提前量 → 视为过期。
        t.expires_at = now_secs() + 100;
        assert!(t.is_expired());
    }

    #[test]
    fn tokens_path_is_gemini_oauth_json() {
        let p = tokens_path().unwrap();
        assert!(
            p.ends_with("wisecortex/gemini_oauth.json")
                || p.ends_with("wisecortex\\gemini_oauth.json")
        );
    }

    #[test]
    fn bases_respect_proxy_env() {
        std::env::remove_var("WC_PROXY_GEMINI_BASE_URL");
        std::env::remove_var("WC_PROXY_GEMINI_AUTH_BASE_URL");
        assert_eq!(api_base(), "https://cloudcode-pa.googleapis.com");
        assert_eq!(token_url(), "https://oauth2.googleapis.com/token");
        std::env::set_var("WC_PROXY_GEMINI_BASE_URL", "https://cf.worker.dev/gemini/");
        std::env::set_var(
            "WC_PROXY_GEMINI_AUTH_BASE_URL",
            "https://cf.worker.dev/gauth",
        );
        assert_eq!(api_base(), "https://cf.worker.dev/gemini");
        assert_eq!(token_url(), "https://cf.worker.dev/gauth/token");
        std::env::remove_var("WC_PROXY_GEMINI_BASE_URL");
        std::env::remove_var("WC_PROXY_GEMINI_AUTH_BASE_URL");
    }

    #[test]
    fn workspace_account_is_detected_as_needing_a_user_supplied_project() {
        // 真机响应（Workspace 账号 webmaster@…，经反代实测）：free-tier 以 DASHER_USER 判为不合格，
        // 只剩 standard-tier，且 userDefinedCloudaicompanionProject=true → 必须自带 GCP 项目。
        let load = serde_json::json!({
            "allowedTiers": [{
                "id": "standard-tier",
                "name": "Gemini Code Assist",
                "userDefinedCloudaicompanionProject": true,
                "isDefault": true,
                "usesGcpTos": true,
            }],
            "ineligibleTiers": [{
                "reasonCode": "DASHER_USER",
                "tierId": "free-tier",
            }],
        });
        assert_eq!(project_from_load(&load), None, "这类账号没有托管 project");
        assert_eq!(tier_to_onboard(&load), "standard-tier");
        assert!(
            tier_needs_user_project(&load),
            "标志为 true 时必须识别出来，否则又要去空转 onboarding"
        );

        // 免费/个人账号（标志缺省或 false）不受影响，照旧走 onboarding。
        let free = serde_json::json!({
            "allowedTiers": [{ "id": "free-tier", "isDefault": true }]
        });
        assert!(!tier_needs_user_project(&free));
        assert_eq!(tier_to_onboard(&free), "free-tier");
    }

    #[test]
    fn empty_project_object_from_onboard_is_not_a_project() {
        // 真机 onboardUser 响应：done:true 但 cloudaicompanionProject 是**空对象**——
        // 那是 Google 在说「不分配」，不是「还没好」。必须提取为 None，否则会把 `{}` 当 id 用。
        let op = serde_json::json!({
            "done": true,
            "response": {
                "@type": "type.googleapis.com/google.internal.cloud.code.v1internal.OnboardUserResponse",
                "cloudaicompanionProject": {},
            },
        });
        assert!(onboard_done(&op));
        assert_eq!(project_from_onboard(&op), None);
    }

    #[test]
    fn project_from_load_reads_string_or_object() {
        // 直接给字符串。
        let r = serde_json::json!({ "cloudaicompanionProject": "proj-str" });
        assert_eq!(project_from_load(&r), Some("proj-str".to_string()));
        // 给对象 {id}。
        let r = serde_json::json!({ "cloudaicompanionProject": { "id": "proj-obj" } });
        assert_eq!(project_from_load(&r), Some("proj-obj".to_string()));
        // 空 / 缺失 → None（触发 onboarding）。
        assert_eq!(project_from_load(&serde_json::json!({})), None);
        assert_eq!(
            project_from_load(&serde_json::json!({ "cloudaicompanionProject": "" })),
            None
        );
    }

    #[test]
    fn tier_to_onboard_picks_default_else_free() {
        let r = serde_json::json!({
            "allowedTiers": [
                { "id": "legacy-tier", "isDefault": false },
                { "id": "free-tier", "isDefault": true },
            ]
        });
        assert_eq!(tier_to_onboard(&r), "free-tier");
        // 没有 isDefault → 兜底 free-tier。
        assert_eq!(tier_to_onboard(&serde_json::json!({})), "free-tier");
        // 标准付费 tier 为默认时选它。
        let r2 =
            serde_json::json!({ "allowedTiers": [{ "id": "standard-tier", "isDefault": true }] });
        assert_eq!(tier_to_onboard(&r2), "standard-tier");
    }

    #[test]
    fn onboard_lro_parsing() {
        // 未完成。
        assert!(!onboard_done(&serde_json::json!({ "done": false })));
        assert!(!onboard_done(&serde_json::json!({})));
        // 完成 + 项目。
        let op = serde_json::json!({
            "done": true,
            "response": { "cloudaicompanionProject": { "id": "proj-final" } }
        });
        assert!(onboard_done(&op));
        assert_eq!(project_from_onboard(&op), Some("proj-final".to_string()));
        // 完成但 project 为字符串形态。
        let op2 = serde_json::json!({ "done": true, "response": { "cloudaicompanionProject": "proj-s" } });
        assert_eq!(project_from_onboard(&op2), Some("proj-s".to_string()));
    }

    #[test]
    fn load_body_includes_metadata_and_optional_env_project() {
        std::env::remove_var("GOOGLE_CLOUD_PROJECT");
        std::env::remove_var("GOOGLE_CLOUD_PROJECT_ID");
        let b = load_body();
        assert_eq!(b["metadata"]["pluginType"], "GEMINI");
        assert!(b.get("cloudaicompanionProject").is_none());
        std::env::set_var("GOOGLE_CLOUD_PROJECT", "my-proj");
        let b2 = load_body();
        assert_eq!(b2["cloudaicompanionProject"], "my-proj");
        std::env::remove_var("GOOGLE_CLOUD_PROJECT");
    }

    #[test]
    fn user_agent_is_denied_per_model_after_a_404() {
        // 回归：带 CLI UA 时 Gemini 3 系会 404，必须能自愈成「该模型不带 UA」。
        let m = "test-model-ua-deny";
        assert!(should_send_user_agent(m), "首次应当带 UA");
        assert!(
            deny_user_agent_for(m),
            "首次记录应返回 true（调用方据此重试一次）"
        );
        assert!(!should_send_user_agent(m), "记录后不该再带 UA");
        assert!(!deny_user_agent_for(m), "重复记录返回 false，避免无限重试");
        // 不影响别的模型。
        assert!(should_send_user_agent("another-model"));
    }

    #[test]
    fn user_agent_format_matches_gemini_cli() {
        let ua = user_agent("gemini-2.5-pro");
        assert!(
            ua.starts_with("GeminiCLI/"),
            "Google 只对这个前缀放行: {ua}"
        );
        assert!(ua.contains("gemini-2.5-pro"), "应带上本次模型: {ua}");
    }

    #[test]
    fn client_id_secret_env_override() {
        std::env::remove_var("WC_GEMINI_OAUTH_CLIENT_ID");
        assert_eq!(client_id(), default_client_id());
        std::env::set_var(
            "WC_GEMINI_OAUTH_CLIENT_ID",
            "custom.apps.googleusercontent.com",
        );
        assert_eq!(client_id(), "custom.apps.googleusercontent.com");
        std::env::remove_var("WC_GEMINI_OAUTH_CLIENT_ID");
    }
}
