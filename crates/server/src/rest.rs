//! REST 接口：模型配置的读写（供设置界面用）。
//!
//! - GET  /api/providers — 内置 provider 预设列表
//! - GET  /api/config    — 当前配置（api_key 不回传明文，只回是否已设置）
//! - POST /api/config    — 保存配置并热重载 agent（无需重启）

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use wisecortex_core::llm::{
    providers, ChatMessage, LlmClient, LlmRequest, ProviderConfig, WireFormat,
};
use wisecortex_core::{config, cron, notify};

use crate::agent::Agent;
use crate::ws::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/status", get(auth_status))
        .route("/api/providers", get(get_providers))
        .route("/api/config", get(get_config).post(post_config))
        .route("/api/config/llm", post(upsert_llm))
        .route("/api/config/llm/test", post(test_llm))
        // Claude 订阅 OAuth 登录（手动粘贴 code 流程；仅个人本机自用）。
        .route("/api/oauth/claude/status", get(oauth_status))
        .route("/api/oauth/claude/start", post(oauth_start))
        .route("/api/oauth/claude/finish", post(oauth_finish))
        .route("/api/oauth/claude/logout", post(oauth_logout))
        // ChatGPT/Codex 订阅 OAuth（同样手动粘贴 code；回调 localhost:1455 会打不开，从地址栏复制）。
        .route("/api/oauth/openai/status", get(oauth_openai_status))
        .route("/api/oauth/openai/start", post(oauth_openai_start))
        .route("/api/oauth/openai/finish", post(oauth_openai_finish))
        .route("/api/oauth/openai/logout", post(oauth_openai_logout))
        // xAI Grok 订阅 OAuth（RFC 8628 设备码流：start 拿码，前端按 interval 轮询 poll）。
        .route("/api/oauth/xai/status", get(oauth_xai_status))
        .route("/api/oauth/xai/start", post(oauth_xai_start))
        .route("/api/oauth/xai/poll", post(oauth_xai_poll))
        .route("/api/oauth/xai/logout", post(oauth_xai_logout))
        .route("/api/oauth/gemini/status", get(oauth_gemini_status))
        .route("/api/oauth/gemini/start", post(oauth_gemini_start))
        .route("/api/oauth/gemini/finish", post(oauth_gemini_finish))
        .route("/api/oauth/gemini/logout", post(oauth_gemini_logout))
        .route("/api/mcp/tools", get(get_mcp_tools))
        .route("/api/jobs", get(get_jobs))
        .route("/api/jobs/{id}", delete(stop_job))
        .route("/api/config/llm/{id}", delete(delete_llm))
        .route("/api/sessions", get(get_sessions))
        .route("/api/sessions/{id}/messages", get(get_messages))
        .route(
            "/api/sessions/{id}/knowledge",
            get(get_session_knowledge).post(post_session_knowledge),
        )
        .route(
            "/api/sessions/{id}/memory",
            get(get_session_memory).post(post_session_memory),
        )
        .route(
            "/api/sessions/{id}/config",
            get(get_session_config).post(post_session_config),
        )
        .route("/api/sessions/{id}/name", post(post_session_name))
        .route("/api/sessions/{id}", delete(delete_session))
        // 产物预览：读取 agent 生成的文件内容（供右侧 artifact 面板预览）
        .route("/api/artifact", get(get_artifact))
        // 通知通道
        .route("/api/channels", get(get_channels).post(post_channel))
        .route("/api/channels/{name}", delete(delete_channel))
        // 定时任务
        .route("/api/cron", get(get_cron).post(post_cron))
        .route("/api/cron/{id}", delete(delete_cron).patch(patch_cron))
        .route("/api/cron/{id}/logs", get(get_cron_logs))
        .route("/api/cron/{id}/run", post(run_cron))
        // 技能市场
        .route("/api/skills/catalog", get(get_skills_catalog))
        .route("/api/skills/market", get(get_skills_market))
        .route("/api/skills/sources", get(get_skills_sources))
        .route("/api/skills/source", post(post_skill_source))
        .route("/api/skills/install", post(post_skill_install))
        .route("/api/skills/git", post(post_skill_git))
        .route("/api/skills/create", post(post_skill_create))
        .route("/api/skills/openclaw/scan", get(get_openclaw_scan))
        .route("/api/skills/openclaw/import", post(post_openclaw_import))
        .route("/api/skills/{name}/enabled", post(post_skill_enabled))
        .route("/api/skills/{name}", get(get_skill).delete(delete_skill))
        // 飞书扫码接入（device-code 注册：拿 app_id/secret 写入 feishu.json）
        .route("/api/feishu/register/begin", post(feishu_register_begin))
        .route("/api/feishu/register/poll", post(feishu_register_poll))
        .route("/api/feishu/config", get(get_feishu_config))
        .route("/api/feishu/longconn", post(post_feishu_longconn))
        // QQ 官方机器人（开放平台 AppID/AppSecret；网关连接，免公网回调）
        .route("/api/qq/config", get(get_qq_config).post(post_qq_config))
        .route("/api/qq/enable", post(post_qq_enable))
        // QQ 扫码绑定（手机 QQ 扫码自动拿 AppID/AppSecret）
        .route("/api/qq/scan/begin", post(qq_scan_begin))
        .route("/api/qq/scan/poll", post(qq_scan_poll))
        // 微信 ClawBot（iLink 长轮询，免公网回调；扫码拿 bot_token 写入 clawbot.json）
        .route("/api/clawbot/config", get(get_clawbot_config))
        .route("/api/clawbot/enable", post(post_clawbot_enable))
        .route("/api/clawbot/scan/begin", post(clawbot_scan_begin))
        .route("/api/clawbot/scan/poll", post(clawbot_scan_poll))
        // 企业微信入站凭据（UI 配置；收消息需公网回调）
        .route(
            "/api/wecom/config",
            get(get_wecom_config).post(post_wecom_config),
        )
        // IM 入站（QQ OneBot 上报事件 / 飞书事件订阅 / 企业微信回调）
        .route("/api/im/onebot", post(im_onebot))
        .route("/api/im/feishu", post(im_feishu))
        .route("/api/im/wecom", get(im_wecom_verify).post(im_wecom))
}

// ── IM 入站：企业微信回调 ─────────────────────────────────────────────────────
// GET：URL 验证，解密 echostr 原样返回。POST：验签+解密消息 → run_once → 应用 API 回复。
// 主动发回复（非被动 XML），避开「5 秒内被动响应」限制。
async fn im_wecom_verify(Query(q): Query<HashMap<String, String>>) -> Response {
    let cfg = wisecortex_core::wecom::load();
    let (Some(token), Some(aes)) = (cfg.callback_token.clone(), cfg.encoding_aes_key.clone())
    else {
        return (StatusCode::SERVICE_UNAVAILABLE, "未配置企业微信").into_response();
    };
    let g = |k: &str| q.get(k).cloned().unwrap_or_default();
    let (sig, ts, nonce, echostr) = (g("msg_signature"), g("timestamp"), g("nonce"), g("echostr"));
    if wisecortex_core::wecom::msg_signature(&token, &ts, &nonce, &echostr) != sig {
        return (StatusCode::FORBIDDEN, "签名不匹配").into_response();
    }
    match wisecortex_core::wecom::decrypt(&aes, &echostr) {
        Ok((plain, _)) => (StatusCode::OK, plain).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

async fn im_wecom(
    State(state): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
    body: String,
) -> Response {
    let cfg = wisecortex_core::wecom::load();
    if !cfg.is_ready() {
        return (StatusCode::SERVICE_UNAVAILABLE, "未配置企业微信").into_response();
    }
    let token = cfg.callback_token.clone().unwrap_or_default();
    let aes = cfg.encoding_aes_key.clone().unwrap_or_default();
    let g = |k: &str| q.get(k).cloned().unwrap_or_default();
    let (sig, ts, nonce) = (g("msg_signature"), g("timestamp"), g("nonce"));

    // 取信封里的 Encrypt → 验签 → 解密。
    let Some(encrypt) = wisecortex_core::wecom::xml_field(&body, "Encrypt") else {
        return (StatusCode::BAD_REQUEST, "无 Encrypt").into_response();
    };
    if wisecortex_core::wecom::msg_signature(&token, &ts, &nonce, &encrypt) != sig {
        return (StatusCode::FORBIDDEN, "签名不匹配").into_response();
    }
    let Ok((plain, _receiveid)) = wisecortex_core::wecom::decrypt(&aes, &encrypt) else {
        return (StatusCode::BAD_REQUEST, "解密失败").into_response();
    };

    // 仅处理 text 消息；其余直接成功应答。
    if wisecortex_core::wecom::xml_field(&plain, "MsgType").as_deref() != Some("text") {
        return (StatusCode::OK, "success").into_response();
    }
    let from = wisecortex_core::wecom::xml_field(&plain, "FromUserName").unwrap_or_default();
    let content = wisecortex_core::wecom::xml_field(&plain, "Content").unwrap_or_default();
    if from.is_empty() || content.trim().is_empty() {
        return (StatusCode::OK, "success").into_response();
    }

    let agent = state.agent.clone();
    tokio::spawn(async move {
        let ag = agent.lock().unwrap().clone();
        let result = ag.run_once(content.trim()).await;
        let _ = tokio::task::spawn_blocking(move || {
            wisecortex_core::wecom::send_text(&cfg, &from, &result)
        })
        .await;
    });

    // 立即应答，回复由上面的异步任务通过应用 API 推送。
    (StatusCode::OK, "success").into_response()
}

// ── IM 入站：飞书事件订阅 ────────────────────────────────────────────────────
// 群里 @机器人 或私聊触发 → run_once → 用飞书 API 回到原会话。
async fn im_feishu(State(state): State<AppState>, Json(event): Json<Value>) -> Json<Value> {
    // URL 验证握手。
    if event.get("type").and_then(Value::as_str) == Some("url_verification") {
        let challenge = event.get("challenge").and_then(Value::as_str).unwrap_or("");
        return Json(json!({ "challenge": challenge }));
    }

    let cfg = wisecortex_core::feishu::load();
    if !cfg.is_ready() {
        return Json(json!({ "ok": false, "error": "未配置飞书应用" }));
    }
    // 去重：飞书对未及时回执的事件会重投；回调 URL 与长连接双开时同一事件也到两次。
    if let Some(eid) = wisecortex_core::feishu::extract_event_id(&event) {
        if wisecortex_core::feishu::seen_recently(&eid) {
            return Json(json!({ "ok": true }));
        }
    }
    if !wisecortex_core::feishu::sender_is_user(&event) {
        return Json(json!({ "ok": true })); // 机器人消息不处理，防自激回环
    }
    let Some((token, chat_id, text)) = wisecortex_core::feishu::extract_message(&event) else {
        return Json(json!({ "ok": true })); // 非 text 消息，忽略
    };
    // 校验来源 token（配置了才校验）。
    if let Some(vt) = cfg.verify_token.clone() {
        if token.as_deref() != Some(vt.as_str()) {
            return Json(json!({ "ok": false, "error": "verify token 不匹配" }));
        }
    }
    if text.trim().is_empty() {
        return Json(json!({ "ok": true }));
    }
    wisecortex_core::feishu::record_chat(&chat_id); // 记下来源会话，供「复用应用」出站推送选目标

    let agent = state.agent.clone();
    let reg = state.registry.clone();
    tokio::spawn(async move {
        // 接入持久会话（多轮上下文）；与长连接共用同一入口。
        let (reply, card) =
            crate::feishu_ws::run_im_turn(&agent, &reg, &chat_id, text.trim()).await;
        let _ = tokio::task::spawn_blocking(move || match card {
            // /sessions 产出互动卡片就发卡片；否则纯文本。
            Some(c) => wisecortex_core::feishu::send_card(&cfg, &chat_id, &c),
            None => wisecortex_core::feishu::send_text(&cfg, &chat_id, &reply),
        })
        .await;
    });

    Json(json!({ "ok": true }))
}

// ── 飞书扫码接入（device-code 注册）──────────────────────────────────────────
/// 开始扫码：init + begin，返回二维码 SVG 与 device_code（前端持有用于轮询）。
async fn feishu_register_begin(Json(body): Json<Value>) -> Json<Value> {
    let domain = wisecortex_core::feishu_register::Domain::parse(
        body.get("domain")
            .and_then(Value::as_str)
            .unwrap_or("feishu"),
    );
    let r = tokio::task::spawn_blocking(move || {
        wisecortex_core::feishu_register::init(domain)?;
        let b = wisecortex_core::feishu_register::begin(domain)?;
        let svg = wisecortex_core::feishu_register::qr_svg(&b.qr_url)?;
        Ok::<_, String>((b, svg))
    })
    .await;
    match r {
        Ok(Ok((b, svg))) => Json(json!({
            "ok": true,
            "device_code": b.device_code,
            "qr_svg": svg,
            "interval": b.interval,
            "expire": b.expire,
            "domain": domain.as_str(),
        })),
        Ok(Err(e)) => Json(json!({ "ok": false, "error": e })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// 轮询扫码结果；成功则把 app_id/app_secret 合并写入 feishu.json（保留既有 verify_token）。
async fn feishu_register_poll(Json(body): Json<Value>) -> Json<Value> {
    let device_code = body
        .get("device_code")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if device_code.is_empty() {
        return Json(json!({ "status": "error", "error": "device_code 必填" }));
    }
    let domain = wisecortex_core::feishu_register::Domain::parse(
        body.get("domain")
            .and_then(Value::as_str)
            .unwrap_or("feishu"),
    );
    let r = tokio::task::spawn_blocking(move || {
        wisecortex_core::feishu_register::poll(&device_code, domain)
    })
    .await;
    use wisecortex_core::feishu_register::PollOutcome;
    match r {
        Ok(Ok(PollOutcome::Success {
            app_id,
            app_secret,
            domain: d,
            ..
        })) => {
            // 合并写入 feishu.json（保留 verify_token）。
            let mut cfg = wisecortex_core::feishu::load();
            cfg.app_id = Some(app_id.clone());
            cfg.app_secret = Some(app_secret);
            let saved = wisecortex_core::feishu::save(&cfg);
            Json(json!({
                "status": "success",
                "app_id": app_id,
                "domain": d.as_str(),
                "saved": saved.is_ok(),
                "error": saved.err().map(|e| e.to_string()),
            }))
        }
        Ok(Ok(PollOutcome::Pending { switch_to })) => Json(json!({
            "status": "pending",
            "domain": switch_to.map(|d| d.as_str()),
        })),
        Ok(Ok(PollOutcome::Denied)) => Json(json!({ "status": "denied" })),
        Ok(Ok(PollOutcome::Expired)) => Json(json!({ "status": "expired" })),
        Ok(Ok(PollOutcome::Error(e))) => Json(json!({ "status": "error", "error": e })),
        Ok(Err(e)) => Json(json!({ "status": "error", "error": e })),
        Err(e) => Json(json!({ "status": "error", "error": e.to_string() })),
    }
}

/// 飞书状态：是否已配置凭据、是否启用长连接。
async fn get_feishu_config() -> Json<Value> {
    let cfg = wisecortex_core::feishu::load();
    Json(json!({
        "ready": cfg.is_ready(),
        "long_conn": cfg.long_conn,
        // 最近收到消息的会话 id，供「复用应用」出站推送时选目标。
        "recent_chats": wisecortex_core::feishu::recent_chats(),
    }))
}

/// 启用/停用飞书长连接（写 feishu.json；重启服务端生效）。
async fn post_feishu_longconn(Json(body): Json<Value>) -> Json<Value> {
    let enabled = body
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut cfg = wisecortex_core::feishu::load();
    cfg.long_conn = enabled;
    match wisecortex_core::feishu::save(&cfg) {
        Ok(()) => Json(json!({ "ok": true, "long_conn": enabled })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// QQ 官方机器人状态：是否已配置凭据、是否启用网关连接、AppID（非敏感，回显）。
async fn get_qq_config() -> Json<Value> {
    let cfg = wisecortex_core::qq::load();
    Json(json!({
        "ready": cfg.is_ready(),
        "enabled": cfg.enabled,
        "app_id": cfg.app_id,
        // 最近收到消息的会话，供定时任务选 QQ 推送目标。
        "recent_peers": wisecortex_core::qq::recent_peers(),
    }))
}

/// 保存 QQ 凭据（合并写 qq.json；留空字段保留原值，AppSecret 不回明文）。
async fn post_qq_config(Json(body): Json<Value>) -> Json<Value> {
    let mut cfg = wisecortex_core::qq::load();
    let s = |k: &str| {
        body.get(k)
            .and_then(Value::as_str)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    if let Some(v) = s("app_id") {
        cfg.app_id = Some(v);
    }
    if let Some(v) = s("app_secret") {
        cfg.app_secret = Some(v);
    }
    match wisecortex_core::qq::save(&cfg) {
        Ok(()) => Json(json!({ "ok": true, "ready": cfg.is_ready() })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// 启用/停用 QQ 网关连接（写 qq.json；开关热生效，无需重启）。
async fn post_qq_enable(Json(body): Json<Value>) -> Json<Value> {
    let enabled = body
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut cfg = wisecortex_core::qq::load();
    cfg.enabled = enabled;
    match wisecortex_core::qq::save(&cfg) {
        Ok(()) => Json(json!({ "ok": true, "enabled": enabled })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// QQ 扫码绑定：新建任务 + 生成二维码 SVG（手机 QQ 扫码用）。
/// 返回 task_id + key，前端轮询 poll 时回传（key 用于服务端解密 AppSecret）。
async fn qq_scan_begin() -> Json<Value> {
    let r = tokio::task::spawn_blocking(|| -> Result<(String, String, String), String> {
        let (task_id, key) = wisecortex_core::qq_register::create_bind_task()?;
        let url = wisecortex_core::qq_register::connect_url(&task_id, "wisecortex");
        let svg = wisecortex_core::feishu_register::qr_svg(&url)?;
        Ok((task_id, key, svg))
    })
    .await;
    match r {
        Ok(Ok((task_id, key, qr_svg))) => Json(
            json!({ "ok": true, "task_id": task_id, "key": key, "qr_svg": qr_svg, "interval": 2 }),
        ),
        Ok(Err(e)) => Json(json!({ "ok": false, "error": e })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// 轮询扫码结果；COMPLETED 则解密 AppSecret，合并写入 qq.json 并自动开启网关。
async fn qq_scan_poll(Json(body): Json<Value>) -> Json<Value> {
    let task_id = body
        .get("task_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let key = body
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if task_id.is_empty() || key.is_empty() {
        return Json(json!({ "status": "error", "error": "缺 task_id/key" }));
    }
    let r = tokio::task::spawn_blocking(move || -> Result<(i64, String, String), String> {
        let (status, app_id, enc) = wisecortex_core::qq_register::poll_bind_result(&task_id)?;
        if status == wisecortex_core::qq_register::STATUS_COMPLETED {
            let secret = wisecortex_core::qq_register::decrypt_secret(&enc, &key)?;
            Ok((status, app_id, secret))
        } else {
            Ok((status, app_id, String::new()))
        }
    })
    .await;
    match r {
        Ok(Ok((status, app_id, secret))) => {
            if status == wisecortex_core::qq_register::STATUS_COMPLETED {
                let mut cfg = wisecortex_core::qq::load();
                cfg.app_id = Some(app_id.clone());
                cfg.app_secret = Some(secret);
                cfg.enabled = true; // 扫码成功即开启网关连接
                match wisecortex_core::qq::save(&cfg) {
                    Ok(()) => Json(json!({ "status": "success", "app_id": app_id })),
                    Err(e) => Json(json!({ "status": "error", "error": e.to_string() })),
                }
            } else if status == wisecortex_core::qq_register::STATUS_EXPIRED {
                Json(json!({ "status": "expired" }))
            } else {
                Json(json!({ "status": "pending" }))
            }
        }
        Ok(Err(e)) => Json(json!({ "status": "error", "error": e })),
        Err(e) => Json(json!({ "status": "error", "error": e.to_string() })),
    }
}

// ── 微信 ClawBot（iLink）────────────────────────────────────────────────────
/// ClawBot 状态：是否已扫码、是否启用长轮询、绑定的 bot id（非敏感，回显）。
async fn get_clawbot_config() -> Json<Value> {
    let cfg = wisecortex_core::clawbot::load();
    Json(json!({
        "ready": cfg.is_ready(),
        "enabled": cfg.enabled,
        "bot_id": wisecortex_core::clawbot_register::describe(&cfg),
    }))
}

/// 启用/停用 ClawBot 长轮询（写 clawbot.json；开关热生效，无需重启）。
async fn post_clawbot_enable(Json(body): Json<Value>) -> Json<Value> {
    let enabled = body
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut cfg = wisecortex_core::clawbot::load();
    cfg.enabled = enabled;
    match wisecortex_core::clawbot::save(&cfg) {
        Ok(()) => Json(json!({ "ok": true, "enabled": enabled })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// 开始扫码：取二维码，返回可直接 `<img src>` 的 data URL 与会话标识。
async fn clawbot_scan_begin() -> Json<Value> {
    match tokio::task::spawn_blocking(|| wisecortex_core::clawbot_register::begin(None)).await {
        Ok(Ok(b)) => Json(json!({
            "ok": true,
            "qrcode": b.qrcode,
            "qr_img": b.qr_img,
            "interval": wisecortex_core::clawbot_register::POLL_INTERVAL,
        })),
        Ok(Err(e)) => Json(json!({ "ok": false, "error": e })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// 轮询扫码结果；成功则把 bot_token 等写入 clawbot.json 并开启长轮询。
///
/// `base` 由前端回传：`scaned_but_redirect` 时服务端会给一个 `redirect_host`，
/// 后续轮询必须打到那个 IDC，否则会一直停在「等待确认」。
async fn clawbot_scan_poll(Json(body): Json<Value>) -> Json<Value> {
    let qrcode = body
        .get("qrcode")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if qrcode.is_empty() {
        return Json(json!({ "status": "error", "error": "qrcode 必填" }));
    }
    let base = body
        .get("base")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let r = tokio::task::spawn_blocking(move || {
        wisecortex_core::clawbot_register::poll(base.as_deref(), &qrcode)
    })
    .await;
    use wisecortex_core::clawbot_register::PollOutcome;
    match r {
        Ok(Ok(PollOutcome::Success {
            bot_token,
            base_url,
            bot_id,
            user_id,
        })) => {
            let shown = bot_id
                .clone()
                .or_else(|| user_id.clone())
                .unwrap_or_default();
            match wisecortex_core::clawbot_register::save_credentials(
                bot_token, base_url, bot_id, user_id,
            ) {
                Ok(()) => Json(json!({ "status": "success", "bot_id": shown })),
                Err(e) => Json(json!({ "status": "error", "error": e.to_string() })),
            }
        }
        Ok(Ok(PollOutcome::Pending { redirect_host })) => Json(json!({
            "status": "pending",
            // 非空表示后续轮询要切到这个域名（IDC 重定向）。协议给的是裸主机名，
            // 但真回了带 scheme 的也照收，免得拼出 https://https://…。
            "base": redirect_host.map(|h| if h.starts_with("http") {
                h
            } else {
                format!("https://{h}")
            }),
        })),
        Ok(Ok(PollOutcome::Expired)) => Json(json!({ "status": "expired" })),
        Ok(Ok(PollOutcome::Error(e))) => Json(json!({ "status": "error", "error": e })),
        Ok(Err(e)) => Json(json!({ "status": "error", "error": e })),
        Err(e) => Json(json!({ "status": "error", "error": e.to_string() })),
    }
}

/// 企业微信状态 + 当前非敏感配置（secret/aes 仅返回是否已设，不回明文）。
async fn get_wecom_config() -> Json<Value> {
    let cfg = wisecortex_core::wecom::load();
    Json(json!({
        "ready": cfg.is_ready(),
        "corp_id": cfg.corp_id,
        "agent_id": cfg.agent_id,
        "has_secret": cfg.corp_secret.is_some(),
        "has_token": cfg.callback_token.is_some(),
        "has_aes": cfg.encoding_aes_key.is_some(),
    }))
}

/// 保存企业微信入站凭据（合并写 wecom.json；留空的字段保留原值，不清空）。
async fn post_wecom_config(Json(body): Json<Value>) -> Json<Value> {
    let mut cfg = wisecortex_core::wecom::load();
    let s = |k: &str| {
        body.get(k)
            .and_then(Value::as_str)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    if let Some(v) = s("corp_id") {
        cfg.corp_id = Some(v);
    }
    if let Some(v) = s("corp_secret") {
        cfg.corp_secret = Some(v);
    }
    if let Some(v) = s("agent_id") {
        cfg.agent_id = Some(v);
    }
    if let Some(v) = s("callback_token") {
        cfg.callback_token = Some(v);
    }
    if let Some(v) = s("encoding_aes_key") {
        cfg.encoding_aes_key = Some(v);
    }
    match wisecortex_core::wecom::save(&cfg) {
        Ok(()) => Json(json!({ "ok": true, "ready": cfg.is_ready() })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

// ── 通知通道 ────────────────────────────────────────────────────────────────

async fn get_channels() -> Json<Value> {
    Json(json!({ "channels": notify::load_channels() }))
}

async fn post_channel(Json(body): Json<Value>) -> Json<Value> {
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return Json(json!({ "ok": false, "error": "name 必填" }));
    }
    let ch = notify::Channel {
        name: name.clone(),
        kind: body
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("webhook")
            .to_string(),
        url: body
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        target: body
            .get("target")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        username: body
            .get("username")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        password: body
            .get("password")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        from: body
            .get("from")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    };
    let mut chs = notify::load_channels();
    chs.retain(|c| c.name != name);
    chs.push(ch);
    let ok = notify::save_channels(&chs).is_ok();
    Json(json!({ "ok": ok }))
}

async fn delete_channel(Path(name): Path<String>) -> Json<Value> {
    let mut chs = notify::load_channels();
    chs.retain(|c| c.name != name);
    let _ = notify::save_channels(&chs);
    Json(json!({ "ok": true }))
}

// ── 定时任务 ────────────────────────────────────────────────────────────────

async fn get_cron() -> Json<Value> {
    Json(json!({ "tasks": cron::load_tasks() }))
}

async fn post_cron(Json(body): Json<Value>) -> Json<Value> {
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let prompt = body
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    // cron 表达式优先；否则用 interval（字符串 30s/5m/1h 或数字秒）。
    let cron_expr = body
        .get("cron")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let Some(expr) = &cron_expr {
        if !cron::cron_valid(expr) {
            return Json(json!({ "ok": false, "error": "cron 表达式无效（5 或 6 字段）" }));
        }
    }
    let interval_secs = if cron_expr.is_some() {
        0
    } else {
        match body.get("interval") {
            Some(Value::String(s)) => cron::parse_duration(s),
            Some(Value::Number(n)) => n.as_u64(),
            _ => None,
        }
        .unwrap_or(0)
    };
    if cron_expr.is_none() && interval_secs == 0 {
        return Json(json!({ "ok": false, "error": "interval 无效" }));
    }
    if name.is_empty() || prompt.is_empty() {
        return Json(json!({ "ok": false, "error": "name/prompt 必填" }));
    }
    let task = cron::CronTask {
        id: cron::new_id(),
        name,
        interval_secs,
        cron: cron_expr,
        prompt,
        channel: body
            .get("channel")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        workdir: body
            .get("workdir")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        model: body
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        enabled: true,
        // 以创建时刻作为「虚拟上次运行」——首次自动运行落在一个间隔之后，
        // 而不是下一拍(30s)就立刻跑。想马上跑用「立即运行」。否则刚加的周/月任务会被调度器即时触发。
        last_run: Some(cron::now_secs()),
        last_status: None,
        runs: 0,
        last_duration_ms: None,
    };
    let id = task.id.clone();
    let mut tasks = cron::load_tasks();
    tasks.push(task);
    let ok = cron::save_tasks(&tasks).is_ok();
    Json(json!({ "ok": ok, "id": id }))
}

async fn delete_cron(Path(id): Path<String>) -> Json<Value> {
    let mut tasks = cron::load_tasks();
    tasks.retain(|t| t.id != id);
    let _ = cron::save_tasks(&tasks);
    Json(json!({ "ok": true }))
}

/// 更新一个任务：只更新 body 里出现的字段（enabled 用于停启切换；其余用于「编辑」）。
/// 调度二选一：给了非空 `cron` 用 cron（清 interval）；否则给了有效 `interval` 用间隔（清 cron）。
/// `channel`/`workdir` 传空串=清除。
async fn patch_cron(Path(id): Path<String>, Json(body): Json<Value>) -> Json<Value> {
    if let Some(expr) = body
        .get("cron")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        if !cron::cron_valid(expr) {
            return Json(json!({ "ok": false, "error": "cron 表达式无效（5 或 6 字段）" }));
        }
    }
    let mut tasks = cron::load_tasks();
    let mut found = false;
    for t in tasks.iter_mut() {
        if t.id != id {
            continue;
        }
        found = true;
        if let Some(en) = body.get("enabled").and_then(Value::as_bool) {
            t.enabled = en;
        }
        if let Some(v) = body.get("name").and_then(Value::as_str).map(str::trim) {
            if !v.is_empty() {
                t.name = v.to_string();
            }
        }
        if let Some(v) = body.get("prompt").and_then(Value::as_str).map(str::trim) {
            if !v.is_empty() {
                t.prompt = v.to_string();
            }
        }
        if let Some(v) = body.get("channel").and_then(Value::as_str).map(str::trim) {
            t.channel = (!v.is_empty()).then(|| v.to_string());
        }
        if let Some(v) = body.get("workdir").and_then(Value::as_str).map(str::trim) {
            t.workdir = (!v.is_empty()).then(|| v.to_string());
        }
        if let Some(v) = body.get("model").and_then(Value::as_str).map(str::trim) {
            t.model = (!v.is_empty()).then(|| v.to_string());
        }
        // 调度方式：cron 优先。
        if let Some(expr) = body
            .get("cron")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            t.cron = Some(expr.to_string());
            t.interval_secs = 0;
        } else if let Some(iv) = body.get("interval") {
            let secs = match iv {
                Value::String(s) => cron::parse_duration(s),
                Value::Number(n) => n.as_u64(),
                _ => None,
            }
            .unwrap_or(0);
            if secs > 0 {
                t.interval_secs = secs;
                t.cron = None;
            }
        }
    }
    if !found {
        return Json(json!({ "ok": false, "error": "任务不存在" }));
    }
    let ok = cron::save_tasks(&tasks).is_ok();
    Json(json!({ "ok": ok }))
}

async fn get_cron_logs(Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "logs": cron::read_log(&id) }))
}

/// 立即运行一个定时任务（不必等到点）。后台执行，结果落入该任务的执行日志，
/// 前端轮询 `/api/cron/{id}/logs` 即可看到运行情况。
async fn run_cron(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    if !cron::load_tasks().iter().any(|t| t.id == id) {
        return Json(json!({ "ok": false, "error": "任务不存在" }));
    }
    let agent = state.agent.clone();
    let reg = state.registry.clone();
    tokio::spawn(async move {
        let _ = crate::scheduler::run_task_now(&agent, &reg, &id).await;
    });
    Json(json!({ "ok": true }))
}

// ── IM 入站：QQ OneBot 上报 ──────────────────────────────────────────────────
// NapCat 等把消息事件 POST 到此；群消息需以前缀 "wc" 触发，私聊直接处理。
// 处理后用已配置的 onebot 通道把回复发回原群/原人。

const ONEBOT_TRIGGER: &str = "wc";

/// 一条已通过触发判定的 OneBot 入站消息。
struct OnebotInbound {
    /// 去掉触发前缀、trim 后的正文。
    prompt: String,
    /// 群号；私聊为 None。
    group_id: Option<i64>,
    /// 发送者 QQ 号。
    user_id: Option<i64>,
}

/// 解析 OneBot v11 上报事件；不该处理的返回 None（非 message 事件、机器人自己发的、
/// 群里没带前缀的、去掉前缀后为空的）。
///
/// **必须挡掉机器人自己的消息**：NapCat 开了 `reportSelfMessage` 时，机器人发出去的回复
/// 会被原样报回来；私聊又是无前缀触发的，于是自问自答无限循环——而且现在每一轮都写进
/// 持久会话历史，一旦跑起来会把这个会话彻底喂坏。
fn parse_onebot_event(event: &Value) -> Option<OnebotInbound> {
    if event.get("post_type").and_then(Value::as_str) != Some("message") {
        return None;
    }
    let user_id = event.get("user_id").and_then(Value::as_i64);
    if user_id.is_some() && user_id == event.get("self_id").and_then(Value::as_i64) {
        return None;
    }
    let msg_type = event
        .get("message_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    let raw = event
        .get("raw_message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    // 触发判定：群聊需前缀，私聊直接用。
    let prompt = if msg_type == "private" {
        raw.to_string()
    } else {
        raw.strip_prefix(ONEBOT_TRIGGER)?.trim().to_string()
    };
    if prompt.is_empty() {
        return None;
    }
    Some(OnebotInbound {
        prompt,
        group_id: event.get("group_id").and_then(Value::as_i64),
        user_id,
    })
}

/// 这条回复发给谁：`Some(qq号)`=私聊发起人，`None`=回原群。
///
/// 命令的结果一律私发，**不在群里回显**：`/sessions` 会列出你全部会话的名字，打进客服群
/// 等于把手上所有活儿念给客户听——哪怕只有白名单里的你能执行它。普通对话仍回原群，
/// 客服要的就是群里看得见。
fn onebot_reply_target(
    is_command: bool,
    group_id: Option<i64>,
    user_id: Option<i64>,
) -> Option<i64> {
    (is_command || group_id.is_none())
        .then_some(user_id)
        .flatten()
}

async fn im_onebot(State(state): State<AppState>, Json(event): Json<Value>) -> Json<Value> {
    let Some(inb) = parse_onebot_event(&event) else {
        return Json(json!({ "ok": true }));
    };

    // `/` 命令能切会话、改工作目录、清上下文——客服群里的客户不该指挥得动机器人。
    // 白名单只管命令，普通对话一律放行（客户照常提问）。未知的 `/xxx` 不算命令
    // （`commands::parse` 返回 None），所以「/etc/hosts 怎么改」这类提问不受影响。
    let is_command = crate::commands::parse(&inb.prompt).is_some();
    if is_command {
        if let Some(admins) = wisecortex_core::config::onebot_admins() {
            if !inb.user_id.is_some_and(|u| admins.contains(&u)) {
                // 静默忽略：回一句「你没权限」等于当着所有客户的面宣布这里有命令可打。
                return Json(json!({ "ok": true }));
            }
        }
    }

    // 找一个 onebot 通道拿基地址（用于回复）。
    let base = notify::load_channels()
        .into_iter()
        .find(|c| c.kind == "onebot")
        .map(|c| c.url);
    let Some(base) = base else {
        // 落盘：只回 JSON 的话，NapCat 那头不显示、服务端 error.log 也没有，
        // 表现就是「消息发进去了、机器人一声不吭」，无从查起。
        wisecortex_core::buglog::record("onebot", "收到消息但未配置 onebot 通道，无法回复");
        return Json(json!({ "ok": false, "error": "未配置 onebot 通道，无法回复" }));
    };

    let agent = state.agent.clone();
    let reg = state.registry.clone();
    tokio::spawn(async move {
        let OnebotInbound {
            prompt,
            group_id,
            user_id,
        } = inb;
        // 会话按群号 / 私聊 QQ 号隔离，走与飞书、微信同一条多轮入口——客服场景没有多轮
        // 上下文是不成立的：「刚才那个订单怎么处理」得知道「刚才」是什么。
        let origin = match (group_id, user_id) {
            (Some(g), _) => crate::registry::ImOrigin::onebot_group(g),
            (None, Some(u)) => crate::registry::ImOrigin::onebot_private(u),
            (None, None) => {
                wisecortex_core::buglog::record(
                    "onebot",
                    "上报事件既无 group_id 也无 user_id，无法定位会话",
                );
                return;
            }
        };
        let (result, _card) = crate::im::run_im_turn(&agent, &reg, origin, "QQ", &prompt).await;
        let reply_to_user = onebot_reply_target(is_command, group_id, user_id);
        // OneBot 没有互动卡片，卡片版丢弃、用文本版。
        let _ = tokio::task::spawn_blocking(move || {
            let sent = if let Some(u) = reply_to_user {
                notify::send_onebot_private(&base, u, &result)
            } else if let Some(g) = group_id {
                let ch = notify::Channel {
                    name: "reply".into(),
                    kind: "onebot".into(),
                    url: base,
                    target: Some(g.to_string()),
                    username: None,
                    password: None,
                    from: None,
                };
                notify::send(&ch, &result)
            } else {
                Err("既无群号也无 QQ 号，无处可回".to_string())
            };
            if let Err(e) = sent {
                // 发不出去必须落盘。error.log 只收 buglog::record，eprintln 到不了任何人眼前。
                wisecortex_core::buglog::record("onebot", &format!("回复发送失败：{e}"));
            }
        })
        .await;
    });

    Json(json!({ "ok": true }))
}

// ── 技能：我的技能 / 市场（多源）/ 迁移 ──────────────────────────────────────
/// 「我的技能」= 数据目录里已安装的技能（含开箱预装的内置），带来源徽章与启用态。
async fn get_skills_catalog(Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    // 可选 `workdir`：把 `<workdir>/skills` 项目级技能一并列出（与对话所选工作目录一致），
    // 否则只列全局数据目录。工作目录技能与全局重名时以工作目录为准（与 agent 加载顺序一致）。
    let workdir = q
        .get("workdir")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from);
    let entries: Vec<Value> = tokio::task::spawn_blocking(move || {
        let mut dirs: Vec<std::path::PathBuf> = Vec::new();
        // 工作目录技能放最前，重名时覆盖全局（load_dirs 首个目录优先）。
        let mut workdir_names: std::collections::BTreeSet<String> = Default::default();
        if let Some(wd) = workdir {
            let sk = wd.join("skills");
            if sk.is_dir() {
                workdir_names =
                    wisecortex_core::skill::SkillSet::load_dirs(std::slice::from_ref(&sk))
                        .names()
                        .into_iter()
                        .map(String::from)
                        .collect();
                dirs.push(sk);
            }
        }
        if let Some(dir) = wisecortex_core::marketplace::skills_dir() {
            dirs.push(dir);
        }
        if dirs.is_empty() {
            return Vec::new();
        }
        let set = wisecortex_core::skill::SkillSet::load_dirs(&dirs);
        let disabled = wisecortex_core::skills_state::disabled();
        let builtin: std::collections::BTreeSet<String> =
            wisecortex_core::marketplace::builtin_names()
                .into_iter()
                .collect();
        set.entries()
            .into_iter()
            .map(|(name, desc)| {
                let source = if workdir_names.contains(name) {
                    "workdir"
                } else if builtin.contains(name) {
                    "builtin"
                } else {
                    "installed"
                };
                json!({
                    "name": name,
                    "description": desc,
                    "source": source,
                    "installed": true,
                    // 工作目录技能不受全局 skills-state 停用名单影响（它们随工作目录走）。
                    "enabled": source == "workdir" || !disabled.iter().any(|d| d == name),
                })
            })
            .collect()
    })
    .await
    .unwrap_or_default();
    Json(json!({ "entries": entries }))
}

/// 市场列表：按当前选中源（static / clawhub）返回，支持搜索词 `q`。
async fn get_skills_market(Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let query = q.get("q").filter(|s| !s.is_empty()).cloned();
    let entries: Vec<Value> = tokio::task::spawn_blocking(move || {
        let installed = wisecortex_core::marketplace::installed();
        wisecortex_core::marketplace::market_list(query.as_deref())
            .into_iter()
            .map(|e| {
                json!({
                    "name": e.name,
                    "description": e.description,
                    "version": e.version,
                    "source": e.source,
                    "installed": installed.contains(&e.name),
                })
            })
            .collect()
    })
    .await
    .unwrap_or_default();
    Json(json!({ "entries": entries }))
}

/// 市场源：精选列表 + 当前选中。
async fn get_skills_sources() -> Json<Value> {
    let (sources, current) = tokio::task::spawn_blocking(|| {
        (
            wisecortex_core::registry_sources::load_sources(),
            wisecortex_core::registry_sources::current_source(),
        )
    })
    .await
    .unwrap_or_else(|_| {
        (wisecortex_core::registry_sources::builtin_sources(), {
            wisecortex_core::registry_sources::builtin_sources()
                .into_iter()
                .next()
                .unwrap()
        })
    });
    let src_json: Vec<Value> = sources
        .iter()
        .map(|s| json!({ "label": s.label, "url": s.url, "kind": s.kind }))
        .collect();
    Json(json!({
        "sources": src_json,
        "current": { "label": current.label, "url": current.url, "kind": current.kind },
    }))
}

/// 切换当前市场源（持久化）。body: { url, kind: "static"|"clawhub" }
async fn post_skill_source(Json(body): Json<Value>) -> Json<Value> {
    let url = body
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if url.is_empty() {
        return Json(json!({ "ok": false, "error": "url 必填" }));
    }
    let kind = match body.get("kind").and_then(Value::as_str) {
        Some("clawhub") => wisecortex_core::registry_sources::SourceKind::ClawHub,
        _ => wisecortex_core::registry_sources::SourceKind::Static,
    };
    match wisecortex_core::registry_sources::set_current_source(&url, kind) {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn post_skill_install(Json(body): Json<Value>) -> Json<Value> {
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return Json(json!({ "ok": false, "error": "name 必填" }));
    }
    let version = body
        .get("version")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let r = tokio::task::spawn_blocking(move || {
        wisecortex_core::marketplace::install_named(&name, version.as_deref())
    })
    .await;
    match r {
        Ok(Ok(())) => Json(json!({ "ok": true })),
        Ok(Err(e)) => Json(json!({ "ok": false, "error": e })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// 探测可从 openclaw 迁移的技能。
async fn get_openclaw_scan() -> Json<Value> {
    let candidates = tokio::task::spawn_blocking(wisecortex_core::openclaw_migrate::scan)
        .await
        .unwrap_or_default();
    Json(json!({ "candidates": candidates }))
}

/// 导入选定的 openclaw 技能目录。body: { paths: [".../weather", ...] }
async fn post_openclaw_import(Json(body): Json<Value>) -> Json<Value> {
    let paths: Vec<String> = body
        .get("paths")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if paths.is_empty() {
        return Json(json!({ "ok": false, "error": "paths 必填" }));
    }
    let results =
        tokio::task::spawn_blocking(move || wisecortex_core::openclaw_migrate::import(&paths))
            .await
            .unwrap_or_default();
    let imported: Vec<&String> = results
        .iter()
        .filter(|(_, ok)| *ok)
        .map(|(p, _)| p)
        .collect();
    Json(json!({ "ok": true, "imported": imported.len(), "total": results.len() }))
}

async fn post_skill_git(Json(body): Json<Value>) -> Json<Value> {
    let url = body
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if url.is_empty() {
        return Json(json!({ "ok": false, "error": "url 必填" }));
    }
    let subdir = body
        .get("subdir")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let r = tokio::task::spawn_blocking(move || {
        let dest = wisecortex_core::marketplace::skills_dir().ok_or("无数据目录")?;
        wisecortex_core::marketplace::install_from_git(&url, subdir.as_deref(), &dest)
    })
    .await;
    match r {
        Ok(Ok(names)) => Json(json!({ "ok": true, "imported": names })),
        Ok(Err(e)) => Json(json!({ "ok": false, "error": e })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn post_skill_create(Json(body): Json<Value>) -> Json<Value> {
    let s = |k: &str| {
        body.get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let slug = s("slug");
    if slug.is_empty() {
        return Json(json!({ "ok": false, "error": "slug 必填" }));
    }
    let tools: Vec<String> = body
        .get("tools")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    match wisecortex_core::marketplace::create_skill(
        &slug,
        &s("description"),
        &s("trigger"),
        &s("body"),
        &tools,
    ) {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

async fn delete_skill(Path(name): Path<String>) -> Json<Value> {
    match wisecortex_core::marketplace::uninstall(&name) {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// 读取一个已安装技能的 SKILL.md 全文（UI 查看用）。
async fn get_skill(Path(name): Path<String>) -> Json<Value> {
    match wisecortex_core::marketplace::read_skill(&name) {
        Ok(content) => Json(json!({ "ok": true, "name": name, "content": content })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// 启用 / 停用一个技能（持久化到 skills-state.json）。停用后不进可用清单、也无法被调用。
async fn post_skill_enabled(Path(name): Path<String>, Json(body): Json<Value>) -> Json<Value> {
    let enabled = body.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    match wisecortex_core::skills_state::set_enabled(&name, enabled) {
        Ok(()) => Json(json!({ "ok": true, "enabled": enabled })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// 解析产物文件的绝对路径，落点须与 agent 写文件时一致：
/// 绝对路径原样；相对路径优先按本轮工作目录 `cwd` 解析，`cwd` 缺省/非目录时回退全局
/// 工作目录 `workspace`（即 agent 的 `effective_base` 同源——`LlmAgent::workdir`），
/// 最后才回退进程目录。早期实现相对路径直接回退进程 CWD，与 agent 落点不符，导致
/// 任务用默认工作空间（未显式设 working_dir）时产物预览一律「文件不存在」。
fn resolve_artifact_path(
    path: &str,
    cwd: Option<&str>,
    workspace: Option<&std::path::Path>,
) -> std::path::PathBuf {
    let raw = std::path::Path::new(path);
    if raw.is_absolute() {
        return raw.to_path_buf();
    }
    let base = cwd
        .map(str::trim)
        .filter(|s| !s.is_empty() && std::path::Path::new(s).is_dir())
        .map(std::path::PathBuf::from)
        .or_else(|| workspace.map(std::path::Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| ".".into());
    base.join(raw)
}

/// 读取一个产物文件的内容（供前端 artifact 预览）。最大 2MB。
async fn get_artifact(Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let Some(path) = q.get("path").filter(|p| !p.is_empty()) else {
        return Json(json!({ "ok": false, "error": "path 必填" }));
    };
    let workspace = config::load().workspace_dir();
    let pb = resolve_artifact_path(path, q.get("cwd").map(String::as_str), workspace.as_deref());
    let p = pb.as_path();
    if !p.is_file() {
        return Json(json!({ "ok": false, "error": "文件不存在" }));
    }
    // probe=1：只回「在不在」，不读内容。前端在每轮结束时要逐个核对产物入口是否还指向
    // 真实文件（写失败、或 agent 中途把文件挪走/删掉的，入口不该留下），只为判存在而把
    // 几 MB 的内容传一遍纯属浪费。
    if q.get("probe").is_some_and(|v| v == "1") {
        return Json(json!({
            "ok": true,
            "name": p.file_name().and_then(|n| n.to_str()).unwrap_or("artifact"),
        }));
    }
    match std::fs::metadata(p).map(|m| m.len()) {
        Ok(len) if len > 2 * 1024 * 1024 => {
            return Json(json!({ "ok": false, "error": "文件过大（>2MB）" }));
        }
        _ => {}
    }
    match std::fs::read_to_string(p) {
        Ok(content) => Json(json!({
            "ok": true,
            "name": p.file_name().and_then(|n| n.to_str()).unwrap_or("artifact"),
            "content": content,
        })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn get_sessions(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "sessions": state.registry.list() }))
}

/// 历史回放：返回该会话的 canonical 消息（前端据此重画对话）。
async fn get_messages(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    let messages: Vec<Value> = state
        .registry
        .history(&id)
        .iter()
        .map(|m| {
            json!({
                "role": m.role.as_str(),
                "content": m.content,
                "images": m.images,
                "tool_calls": m.tool_calls.iter().map(|t| json!({
                    "name": t.name, "arguments": t.arguments,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Json(json!({ "messages": messages }))
}

/// 重命名会话（自动生成的名称看不出用途时，用户手动改）。body: { name: "..." }
async fn post_session_name(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let name = body.get("name").and_then(Value::as_str).unwrap_or("");
    if state.registry.rename_session(&id, name) {
        Json(json!({ "ok": true }))
    } else {
        Json(json!({ "ok": false, "error": "名称为空或会话不存在" }))
    }
}

async fn delete_session(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    state.registry.remove_session(&id);
    wisecortex_core::memory::clear(&id); // 连带删除会话记忆
    wisecortex_core::tools::shell::close_session(&id); // 关闭该任务的持久 shell 会话
    Json(json!({ "ok": true }))
}

/// 某会话生效的工作目录（决定它归哪份项目记忆）。会话没单独设则用全局工作目录。
fn session_workdir(state: &AppState, id: &str) -> std::path::PathBuf {
    state
        .registry
        .task_config(id)
        .working_dir
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty() && std::path::Path::new(c).is_dir())
        .map(std::path::PathBuf::from)
        .or_else(wisecortex_core::config::workspace_default)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// 取某会话看得到的两层记忆：项目级（同目录所有会话共享）+ 会话级。
async fn get_session_memory(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    let dir = session_workdir(&state, &id);
    Json(json!({
        "memory": wisecortex_core::memory::read(&id),
        "project_memory": wisecortex_core::memory::read_scope(wisecortex_core::memory::Scope::Project(&dir)),
        "project_dir": dir.display().to_string(),
    }))
}

/// 设置/清空记忆（整体替换；空串=清空）。
/// body: `{ memory: "..." }` 改会话级，`{ project_memory: "..." }` 改项目级，可同时给。
async fn post_session_memory(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    if let Some(text) = body.get("memory").and_then(Value::as_str) {
        if let Err(e) = wisecortex_core::memory::write(&id, text) {
            return Json(json!({ "ok": false, "error": e.to_string() }));
        }
    }
    if let Some(text) = body.get("project_memory").and_then(Value::as_str) {
        let dir = session_workdir(&state, &id);
        let scope = wisecortex_core::memory::Scope::Project(&dir);
        if let Err(e) = wisecortex_core::memory::write_scope(scope, text) {
            return Json(json!({ "ok": false, "error": e.to_string() }));
        }
    }
    Json(json!({ "ok": true }))
}

/// 取某会话绑定的知识库路径。
async fn get_session_knowledge(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    Json(json!({ "paths": state.registry.knowledge(&id) }))
}

/// 设置某会话绑定的知识库路径（随会话持久化）。body: { paths: [...] }
async fn post_session_knowledge(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let paths: Vec<String> = body
        .get("paths")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    state.registry.set_knowledge(&id, paths);
    Json(json!({ "ok": true }))
}

/// 取某会话的任务配置（模型/技能/auto-approve/工作目录）。
async fn get_session_config(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "config": state.registry.task_config(&id) }))
}

/// 设置某会话的任务配置（随会话持久化）。body = TaskConfig。
/// 在任务首条消息发出前绑定；之后前端冻结编辑。
async fn post_session_config(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<crate::proto::TaskConfig>,
) -> Json<Value> {
    state.registry.set_task_config(&id, body);
    Json(json!({ "ok": true }))
}

/// 访问密钥中间件：公开模式直接放行；否则要求 query `access_key` 或头 `x-access-key` 匹配。
pub async fn require_access_key(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    if state.access_key.is_none() {
        return next.run(req).await;
    }
    // 鉴权状态查询：登录门用它判断「是否需要密钥 / 所带密钥是否有效」，自身免密放行（不泄露密钥）。
    if req.uri().path() == "/api/auth/status" {
        return next.run(req).await;
    }
    // IM 入站 webhook 无法携带我们的密钥（由 OneBot 端密钥/网络层保护），放行。
    if req.uri().path().starts_with("/api/im/") {
        return next.run(req).await;
    }
    let from_query = Query::<HashMap<String, String>>::try_from_uri(req.uri())
        .ok()
        .and_then(|q| q.0.get("access_key").cloned());
    let from_header = req
        .headers()
        .get("x-access-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let presented = from_query.or(from_header);

    if state.auth_ok(presented.as_deref()) {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, "unauthorized").into_response()
    }
}

/// 鉴权状态（免密钥可访问，供前端登录门判断）：
/// `required` = 是否启用了 access_key；`authorized` = 本请求所带密钥是否有效（公开模式恒 true）。
async fn auth_status(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    uri: axum::http::Uri,
) -> Json<Value> {
    let required = state.access_key.is_some();
    let from_query = Query::<HashMap<String, String>>::try_from_uri(&uri)
        .ok()
        .and_then(|q| q.0.get("access_key").cloned());
    let from_header = headers
        .get("x-access-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let presented = from_query.or(from_header);
    let authorized = state.auth_ok(presented.as_deref());
    Json(json!({ "required": required, "authorized": authorized }))
}

async fn get_providers() -> Json<Value> {
    // 内置预设 + 用户 providers.json 覆盖（频繁更新的模型表可外置维护，无需重编译）。
    Json(json!({ "providers": providers::load_merged() }))
}

async fn get_config() -> Json<Value> {
    let c = config::load();
    let llms: Vec<Value> = c
        .llms
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "provider": p.provider,
                "model": p.model,
                "base_url": p.base_url,
                "price_in": p.price_in,
                "price_out": p.price_out,
                "price_cache_read": p.price_cache_read,
                "max_tokens": p.max_tokens,
                "claude_oauth": p.claude_oauth.unwrap_or(false),
                "openai_codex": p.openai_codex.unwrap_or(false),
                "xai_grok": p.xai_grok.unwrap_or(false),
                "gemini_oauth": p.gemini_oauth.unwrap_or(false),
                "api_key_set": p.has_key(),
                // 该档是否走订阅 OAuth（不需要 api_key）。前端据此把徽章显示为「订阅额度」
                // 而不是「未配置」——订阅档没有 key 是正常的，不是没配好。
                // 规则只在服务端算一份：前端再抄一遍四个开关的 OR 迟早会和后端跑偏。
                // 注意它只说明「这档不需要 key」，不代表 OAuth 一定还登录着（登录态见 /api/oauth/*/status）。
                "subscription": p.is_subscription(),
                // 原始档内推理强度：null=跟随全局；""=显式关闭；"low".."max"=具体档位。供编辑弹窗预填。
                "reasoning_effort": p.reasoning_effort,
                // 解析后的视觉能力：订阅档恒为 true（Grok/Gemini 档可显式关）；否则显式 vision 优先，
                // 未指定回落目录白名单。供前端列表显示徽章 + 编辑弹窗预填复选框。
                "vision": p.claude_oauth.unwrap_or(false)
                    || p.openai_codex.unwrap_or(false)
                    || ((p.xai_grok.unwrap_or(false) || p.gemini_oauth.unwrap_or(false))
                        && p.vision.unwrap_or(true))
                    || p.vision.unwrap_or_else(|| {
                        wisecortex_core::llm::providers::supports_vision(
                            p.provider.as_deref().unwrap_or("deepseek"),
                            p.model.as_deref().unwrap_or_default(),
                        )
                    }),
            })
        })
        .collect();
    let workspace = c
        .workspace_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let ws = c.web_search.clone().unwrap_or_default();
    let web_search = json!({
        "provider": ws.provider.unwrap_or_default(),
        "base_url": ws.base_url.unwrap_or_default(),
        "api_key_set": ws.api_key.as_deref().map(|k| !k.is_empty()).unwrap_or(false),
    });
    // 访问密钥来源：环境变量 WC_ACCESS_KEY 优先于 config.json（见 server::run）。
    // UI 据此点亮/锁定开关——环境变量设的密钥只能在服务器改，前端不可更改。
    let access_key_env = std::env::var("WC_ACCESS_KEY")
        .ok()
        .map(|k| !k.is_empty())
        .unwrap_or(false);
    let access_key_cfg = c
        .access_key
        .as_deref()
        .map(|k| !k.is_empty())
        .unwrap_or(false);
    Json(json!({
        "llms": llms,
        "active_llm": c.active_llm,
        "auto_approve": c.auto_approve.unwrap_or(true),
        // config.json 里是否设了密钥（决定输入框「已设置」占位符 / 可否在 UI 清除）。
        "access_key_set": access_key_cfg,
        // 密钥来自环境变量 WC_ACCESS_KEY（优先级更高，UI 锁定为只读）。
        "access_key_env": access_key_env,
        // 服务器当前是否真的要求密钥（env 或 config 任一即生效）——开关据此点亮。
        "access_key_active": access_key_env || access_key_cfg,
        // 当前档是否可用（前端据此决定是否自动弹出设置引导首配）。
        // 用 is_usable() 而不是 has_key()：订阅档走 OAuth，没有 api_key 但照样能跑，
        // 判成「未配置」会让订阅用户每次启动都被强制弹到设置页。
        "llm_ready": c.active().map(|p| p.is_usable()).unwrap_or(false),
        // 当前生效的全局工作目录（绝对路径）。
        "workspace": workspace,
        // 服务端版本：前端显示在侧栏底部，用来一眼确认「拉取/重启到底生效没有」。
        "version": wisecortex_core::version(),
        // 出站网络代理（留空=直连）。
        "proxy": c.proxy.clone().unwrap_or_default(),
        // 联网搜索配置（api_key 不回显，只回是否已设）。
        "web_search": web_search,
        // 自动记忆开关。
        "auto_memory": c.auto_memory.unwrap_or(false),
        // 自动优化上下文（图片只发一次）。未配置=开启。
        "auto_trim_context": c.auto_trim_context_effective(),
        // 推理强度 / extended thinking（""=关闭）。
        "reasoning_effort": c.reasoning_effort.clone().unwrap_or_default(),
        // 无人值守任务（定时/IM）回合上限（0/null=用默认 50；env WC_MAX_ITERATIONS 优先）。
        "max_iterations": c.max_iterations,
        // 交互式聊天回合上限（0/null=用默认 1000；env WC_MAX_ITERATIONS_INTERACTIVE 优先）。
        "max_iterations_interactive": c.max_iterations_interactive,
        "subagent_max_iterations": c.subagent_max_iterations,
        // 全局输出 token 上限默认（0/null=用默认 32768；env WC_MAX_TOKENS 优先；模型档可单独覆盖）。
        "max_tokens": c.max_tokens,
        // 定时任务日志保留天数（null=默认 7；0=不自动清理）。
        "cron_log_auto_clean": c.cron_log_auto_clean,
        "cron_log_max_mb": c.cron_log_max_mb,
        // MCP 服务器配置（name → {command,args,env,disabled}）。
        "mcp_servers": c.mcp_servers,
        // 事件钩子（event → [{matcher,command,timeout_ms}]）。
        "hooks": c.hooks,
    }))
}

/// 后台长任务列表（供「后台任务」面板轮询）。
async fn get_jobs() -> Json<Value> {
    Json(json!({ "jobs": crate::jobs::list() }))
}

/// 中止一个后台长任务。
async fn stop_job(Path(id): Path<String>) -> Json<Value> {
    Json(json!({ "ok": crate::jobs::stop(&id) }))
}

// ── Claude 订阅 OAuth（手动粘贴 code 流程；仅个人本机自用）──────────────────
/// 是否已登录 Claude 订阅。
async fn oauth_status() -> Json<Value> {
    Json(json!({ "logged_in": wisecortex_core::llm::oauth::is_logged_in() }))
}

/// 开始登录。桌面传 `{loopback:true}` → 起临时 127.0.0.1 监听、用 `localhost:{port}/callback`
/// 回调建 URL（与 Claude Code 同 client_id、同 RFC 8252 回环回调），授权后自动接住 code 兑换
/// （前端轮询 status，免粘贴）；否则手动粘贴（返回 verifier/state）。
async fn oauth_start(Json(body): Json<Value>) -> Json<Value> {
    use wisecortex_core::llm::oauth;
    let verifier = oauth::random_token();
    let state = oauth::random_token();
    if body
        .get("loopback")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        match crate::oauth_loopback::bind("127.0.0.1:0").await {
            Ok((listener, port)) => {
                // Claude Code 用 `localhost`（非 127.0.0.1），这里对齐它。
                let redirect = format!("http://localhost:{port}/callback");
                let url = oauth::build_auth_url(&verifier, &state, &redirect);
                let (v, st, r) = (verifier.clone(), state.clone(), redirect.clone());
                crate::oauth_loopback::spawn_catch(
                    listener,
                    state,
                    move |code, cb_state| async move {
                        // Claude 兑换要带 state；用回调里带回的 state（已在 spawn_catch 校验等于我们发的）。
                        let st_use = if cb_state.is_empty() { st } else { cb_state };
                        oauth::exchange_code(&code, &st_use, &v, &r)
                            .await
                            .map(|_| ())
                    },
                );
                return Json(json!({ "url": url, "loopback": true }));
            }
            Err(e) => eprintln!("[oauth] claude loopback 起监听失败，回退手动：{e}"),
        }
    }
    let url = oauth::build_auth_url(&verifier, &state, oauth::MANUAL_REDIRECT_URL);
    Json(json!({ "url": url, "verifier": verifier, "state": state }))
}

/// 完成登录：用粘贴的 code + start 返回的 verifier/state 兑换并保存 token。
async fn oauth_finish(Json(body): Json<Value>) -> Json<Value> {
    use wisecortex_core::llm::oauth;
    let s = |k: &str| {
        body.get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let (code, _) = oauth::parse_manual_code(&s("code"));
    let verifier = s("verifier");
    let state = s("state");
    if code.is_empty() || verifier.is_empty() {
        return Json(json!({ "ok": false, "error": "缺少 code 或 verifier" }));
    }
    match oauth::exchange_code(&code, &state, &verifier, oauth::MANUAL_REDIRECT_URL).await {
        Ok(_) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// 退出 Claude 订阅登录：删除本机 token。
async fn oauth_logout() -> Json<Value> {
    match wisecortex_core::llm::oauth::clear_tokens() {
        Ok(_) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

// ── ChatGPT / Codex 订阅 OAuth ───────────────────────────────────────────────
/// 是否已登录 ChatGPT 订阅。
async fn oauth_openai_status() -> Json<Value> {
    Json(json!({ "logged_in": wisecortex_core::llm::oauth_openai::is_logged_in() }))
}

/// 开始登录：生成 verifier/state + 授权 URL。Codex 换 token 不需要 state，故只回 verifier。
/// 桌面传 `{loopback:true}` → 起 127.0.0.1:1455 监听（Codex client 固定回调该端口），授权后
/// 自动接住 code 兑换（前端轮询 status，免粘贴）；端口被占或非桌面则回退手动粘贴。
async fn oauth_openai_start(Json(body): Json<Value>) -> Json<Value> {
    use wisecortex_core::llm::{oauth, oauth_openai};
    let verifier = oauth::random_token();
    let state = oauth::random_token();
    if body
        .get("loopback")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        // Codex client 的回调固定 http://localhost:1455/auth/callback，必须绑这个端口。
        match crate::oauth_loopback::bind("127.0.0.1:1455").await {
            Ok((listener, _port)) => {
                let url = oauth_openai::build_auth_url(&verifier, &state);
                let v = verifier.clone();
                crate::oauth_loopback::spawn_catch(
                    listener,
                    String::new(),
                    move |code, _st| async move {
                        oauth_openai::exchange_code(&code, &v).await.map(|_| ())
                    },
                );
                return Json(json!({ "url": url, "loopback": true }));
            }
            Err(e) => eprintln!("[oauth] codex loopback 起监听(1455)失败，回退手动：{e}"),
        }
    }
    let url = oauth_openai::build_auth_url(&verifier, &state);
    Json(json!({ "url": url, "verifier": verifier, "state": state }))
}

/// 完成登录：用粘贴的 code（或整段回调 URL）+ verifier 换 token 并保存。
async fn oauth_openai_finish(Json(body): Json<Value>) -> Json<Value> {
    use wisecortex_core::llm::oauth_openai;
    let raw = body
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    // 允许粘贴整段 `http://localhost:1455/auth/callback?code=...&state=...`，自动抽出 code。
    let code = extract_query_param(&raw, "code").unwrap_or(raw);
    let verifier = body
        .get("verifier")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if code.is_empty() || verifier.is_empty() {
        return Json(json!({ "ok": false, "error": "缺少 code 或 verifier" }));
    }
    match oauth_openai::exchange_code(&code, &verifier).await {
        Ok(_) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// 退出 ChatGPT 订阅登录：删除本机 token。
async fn oauth_openai_logout() -> Json<Value> {
    match wisecortex_core::llm::oauth_openai::clear_tokens() {
        Ok(_) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

// ── xAI Grok 订阅 OAuth（RFC 8628 设备码流；仅个人自用）─────────────────────

async fn oauth_xai_status() -> Json<Value> {
    Json(json!({ "logged_in": wisecortex_core::llm::oauth_xai::is_logged_in() }))
}

/// 登录第一步：申请设备码，把 user_code + 验证链接给前端展示；前端随后按 interval 轮询 poll。
async fn oauth_xai_start() -> Json<Value> {
    match wisecortex_core::llm::oauth_xai::request_device_code().await {
        Ok(d) => Json(json!({
            "ok": true,
            "device_code": d.device_code,
            "user_code": d.user_code,
            "verification_uri": d.verification_uri,
            "verification_uri_complete": d.verification_uri_complete,
            "interval": d.interval.unwrap_or(5),
            "expires_in": d.expires_in.unwrap_or(300),
        })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// 轮询一次：pending=继续等；slow_down=间隔+5s 再轮；logged_in=完成（token 已落盘）。
async fn oauth_xai_poll(Json(body): Json<Value>) -> Json<Value> {
    use wisecortex_core::llm::oauth_xai::{self, PollOutcome};
    let device_code = body
        .get("device_code")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if device_code.is_empty() {
        return Json(json!({ "ok": false, "error": "缺少 device_code" }));
    }
    match oauth_xai::poll_device_token(device_code).await {
        Ok(PollOutcome::Authorized) => Json(json!({ "ok": true, "logged_in": true })),
        Ok(PollOutcome::Pending) => Json(json!({ "ok": true, "pending": true })),
        Ok(PollOutcome::SlowDown) => {
            Json(json!({ "ok": true, "pending": true, "slow_down": true }))
        }
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// 退出 Grok 订阅登录：删除本机 token。
async fn oauth_xai_logout() -> Json<Value> {
    match wisecortex_core::llm::oauth_xai::clear_tokens() {
        Ok(_) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

// ── Gemini（Google 账号 / Code Assist）订阅 OAuth（手动粘贴 code 流程；仅个人本机自用）──────
/// 是否已登录 Gemini 订阅（附带展示已登邮箱）。
async fn oauth_gemini_status() -> Json<Value> {
    Json(json!({
        "logged_in": wisecortex_core::llm::oauth_gemini::is_logged_in(),
        "email": wisecortex_core::llm::oauth_gemini::logged_in_email(),
    }))
}

/// 开始登录。桌面传 `{loopback:true}` → 起临时 127.0.0.1 监听、用 localhost 回调建 URL、
/// 授权后自动接住 code 兑换（前端轮询 status 即可，免粘贴）；否则走手动粘贴（返回 verifier/state）。
async fn oauth_gemini_start(Json(body): Json<Value>) -> Json<Value> {
    use wisecortex_core::llm::{oauth, oauth_gemini};
    let verifier = oauth::random_token();
    let state = oauth::random_token();
    if body
        .get("loopback")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        match crate::oauth_loopback::bind("127.0.0.1:0").await {
            Ok((listener, port)) => {
                let redirect = format!("http://127.0.0.1:{port}/oauth2callback");
                let url = oauth_gemini::build_auth_url(&verifier, &state, &redirect);
                let (v, r) = (verifier.clone(), redirect.clone());
                crate::oauth_loopback::spawn_catch(listener, state, move |code, _st| async move {
                    oauth_gemini::exchange_code(&code, &v, &r).await.map(|_| ())
                });
                return Json(json!({ "url": url, "loopback": true }));
            }
            Err(e) => eprintln!("[oauth] gemini loopback 起监听失败，回退手动：{e}"),
        }
    }
    let url = oauth_gemini::build_auth_url(&verifier, &state, oauth_gemini::MANUAL_REDIRECT_URL);
    Json(json!({ "url": url, "verifier": verifier, "state": state }))
}

/// 完成登录：用粘贴的 code（或整条 redirect URL）+ verifier 兑换 token，跑一次性 onboarding，保存。
async fn oauth_gemini_finish(Json(body): Json<Value>) -> Json<Value> {
    use wisecortex_core::llm::oauth_gemini;
    let raw = body
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let (code, _) = oauth_gemini::parse_manual_code(&raw);
    let verifier = body
        .get("verifier")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if code.is_empty() || verifier.is_empty() {
        return Json(json!({ "ok": false, "error": "缺少 code 或 verifier" }));
    }
    match oauth_gemini::exchange_code(&code, &verifier, oauth_gemini::MANUAL_REDIRECT_URL).await {
        Ok(t) => Json(json!({ "ok": true, "email": t.email })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// 退出 Gemini 订阅登录：删除本机 token。
async fn oauth_gemini_logout() -> Json<Value> {
    match wisecortex_core::llm::oauth_gemini::clear_tokens() {
        Ok(_) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// 从一段文本里抽 query 参数值（用户可能粘整段回调 URL；非 URL 时返回 None）。
fn extract_query_param(s: &str, key: &str) -> Option<String> {
    let q = s.split_once('?').map(|(_, q)| q).unwrap_or(s);
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key && !v.is_empty()).then(|| {
            // 简单 URL 解码：%XX → 字节；够用（code 一般无特殊字符）。
            v.split('#').next().unwrap_or(v).to_string()
        })
    })
}

/// 已连接的 MCP 服务器工具（供设置页显示连接状态/已发现工具）。
async fn get_mcp_tools() -> Json<Value> {
    let tools: Vec<Value> = wisecortex_core::mcp::tool_infos()
        .into_iter()
        .map(|t| json!({ "server": t.server, "tool": t.tool, "description": t.description }))
        .collect();
    Json(json!({ "tools": tools }))
}

/// 全局设置：切换当前档 / auto_approve / access_key，并热重载 agent。
async fn post_config(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let mut c = config::load();

    if let Some(v) = body.get("active_llm").and_then(Value::as_str) {
        c.active_llm = Some(v.to_string());
    }
    if let Some(v) = body.get("auto_approve").and_then(Value::as_bool) {
        c.auto_approve = Some(v);
    }
    if let Some(v) = body.get("auto_memory").and_then(Value::as_bool) {
        c.auto_memory = Some(v);
    }
    if let Some(v) = body.get("auto_trim_context").and_then(Value::as_bool) {
        c.auto_trim_context = Some(v);
    }
    // reasoning_effort：显式传入才改；空串=关闭。
    if let Some(v) = body.get("reasoning_effort").and_then(Value::as_str) {
        c.reasoning_effort = if v.trim().is_empty() {
            None
        } else {
            Some(v.trim().to_string())
        };
    }
    // max_iterations：无人值守任务回合上限；传入正整数即设置，0/空表示恢复默认（None）。
    if let Some(v) = body.get("max_iterations") {
        let n = v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()));
        c.max_iterations = match n {
            Some(x) if x > 0 => Some(x as usize),
            _ => None,
        };
    }
    // max_iterations_interactive：交互式聊天回合上限；正整数即设置，0/空表示恢复默认（None→很高）。
    if let Some(v) = body.get("max_iterations_interactive") {
        let n = v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()));
        c.max_iterations_interactive = match n {
            Some(x) if x > 0 => Some(x as usize),
            _ => None,
        };
    }
    // subagent_max_iterations：子 agent 回合上限；正整数即设置，0/空表示恢复默认（None→100）。
    if let Some(v) = body.get("subagent_max_iterations") {
        let n = v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()));
        c.subagent_max_iterations = match n {
            Some(x) if x > 0 => Some(x as usize),
            _ => None,
        };
    }
    // max_tokens：全局输出 token 上限默认；正整数即设置，0/空表示恢复默认（None→32768）。
    if let Some(v) = body.get("max_tokens") {
        let n = v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()));
        c.max_tokens = match n {
            Some(x) if x > 0 => Some(x as u32),
            _ => None,
        };
    }
    // cron_log_auto_clean：是否自动清理定时任务日志。null=恢复默认（开启）。
    if let Some(v) = body.get("cron_log_auto_clean") {
        c.cron_log_auto_clean = if v.is_null() { None } else { v.as_bool() };
    }
    // cron_log_max_mb：单任务日志体积上限（MB）。空串/null=恢复默认（None→10MB）。
    // 两项都即时生效（调度器每次清理前重新读 config）。
    if let Some(v) = body.get("cron_log_max_mb") {
        if v.is_null() || v.as_str().map(|s| s.trim().is_empty()).unwrap_or(false) {
            c.cron_log_max_mb = None;
        } else if let Some(n) = v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        {
            c.cron_log_max_mb = Some(n as u32);
        }
    }
    // access_key：显式传入才改；空串表示清除（回到公开模式，重启生效）。
    if let Some(v) = body.get("access_key").and_then(Value::as_str) {
        c.access_key = if v.is_empty() {
            None
        } else {
            Some(v.to_string())
        };
    }
    // workspace：显式传入才改；空串表示恢复三端默认。
    if let Some(v) = body.get("workspace").and_then(Value::as_str) {
        c.workspace = if v.trim().is_empty() {
            None
        } else {
            Some(v.trim().to_string())
        };
    }
    // proxy：显式传入才改；空串表示直连。
    if let Some(v) = body.get("proxy").and_then(Value::as_str) {
        c.proxy = if v.trim().is_empty() {
            None
        } else {
            Some(v.trim().to_string())
        };
    }
    // web_search：provider/base_url 空串=清除；api_key 空串=保留既有（不回显）。
    if let Some(ws) = body.get("web_search") {
        let mut cur = c.web_search.clone().unwrap_or_default();
        let opt = |k: &str| {
            ws.get(k)
                .and_then(Value::as_str)
                .map(|s| s.trim().to_string())
        };
        if let Some(p) = opt("provider") {
            cur.provider = (!p.is_empty()).then_some(p);
        }
        if let Some(b) = opt("base_url") {
            cur.base_url = (!b.is_empty()).then_some(b);
        }
        if let Some(k) = opt("api_key") {
            if !k.is_empty() {
                cur.api_key = Some(k); // 空串=保留既有
            }
        }
        c.web_search = Some(cur);
    }
    // mcp_servers：整体替换（前端传完整 map）。变更后重连。
    let mut mcp_changed = false;
    if let Some(v) = body.get("mcp_servers") {
        if let Ok(map) = serde_json::from_value::<
            std::collections::BTreeMap<String, config::McpServerConfig>,
        >(v.clone())
        {
            mcp_changed = map != c.mcp_servers;
            c.mcp_servers = map;
        }
    }

    // hooks：整体替换（前端传完整 map）。下次事件即按新配置（for_event 实时读盘）。
    if let Some(v) = body.get("hooks") {
        if let Ok(map) = serde_json::from_value::<
            std::collections::BTreeMap<String, Vec<config::HookConfig>>,
        >(v.clone())
        {
            c.hooks = map;
        }
    }

    let saved = config::save(&c);
    if mcp_changed {
        // 配置变更：丢弃旧连接，后台按新配置重连。
        wisecortex_core::mcp::reset();
        tokio::task::spawn_blocking(wisecortex_core::mcp::ensure_started);
    }
    let describe = hot_reload(&state);
    Json(json!({
        "ok": saved.is_ok(),
        "error": saved.err().map(|e| e.to_string()),
        "agent": describe,
    }))
}

/// 测试一个 LLM 配置是否可用（发一条极小请求）。不落盘。
/// body: { provider, base_url, model, api_key, id? }；api_key 为空且给了 id 时用已存的 key。
async fn test_llm(Json(body): Json<Value>) -> Json<Value> {
    let s = |k: &str| {
        body.get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let provider = s("provider");
    let model = s("model");
    if model.is_empty() {
        return Json(json!({ "ok": false, "error": "请先选择 / 填写 Model ID" }));
    }
    let preset = providers::get(&provider);
    let base_url = {
        let b = s("base_url");
        if !b.is_empty() {
            b
        } else {
            preset.map(|p| p.base_url.to_string()).unwrap_or_default()
        }
    };
    if base_url.is_empty() {
        return Json(json!({ "ok": false, "error": "缺少 Endpoint（OpenAI 兼容需自填）" }));
    }
    let format = preset.map(|p| p.format).unwrap_or(WireFormat::OpenAi);
    // api_key：body 优先；为空且给了 id，则用该档已存的 key。
    let mut api_key = s("api_key");
    if api_key.is_empty() {
        let id = s("id");
        if !id.is_empty() {
            if let Some(p) = config::load().llms.into_iter().find(|p| p.id == id) {
                api_key = p.api_key.unwrap_or_default();
            }
        }
    }
    if api_key.is_empty() {
        return Json(json!({ "ok": false, "error": "缺少 API Key" }));
    }

    let cfg = ProviderConfig::new(base_url, api_key, format);
    let mut req = LlmRequest::new(model, vec![ChatMessage::user("ping")]);
    req.max_tokens = 16;
    // 在阻塞线程外用异步客户端发起；超时由 reqwest 默认控制。
    match LlmClient::new().complete(&cfg, &req, |_| {}).await {
        Ok(_) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e.to_string() })),
    }
}

/// 新增 / 更新一个 LLM 配置档。id 为空 → 新建；空 api_key 表示「不改动已有 key」。
async fn upsert_llm(State(state): State<AppState>, Json(body): Json<Value>) -> Json<Value> {
    let mut c = config::load();
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let str_field = |k: &str| body.get(k).and_then(Value::as_str).map(str::to_string);

    // 保留旧 key：编辑已有档且没填新 key 时不清空。
    let existing_key = c
        .llms
        .iter()
        .find(|p| p.id == id)
        .and_then(|p| p.api_key.clone());
    let new_key = match str_field("api_key") {
        Some(k) if !k.is_empty() => Some(k),
        _ => existing_key,
    };
    let base_url = str_field("base_url").filter(|s| !s.is_empty());
    // 价格可传数字或字符串；空/非法视为未设置。
    let num_field = |k: &str| {
        body.get(k).and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
        })
    };

    // max_tokens 可传数字或字符串；空/非法/0 视为未设置（用全局默认）。
    let max_tokens = body
        .get("max_tokens")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
        })
        .filter(|n| *n > 0)
        .map(|n| n as u32);
    // Claude 订阅档标记：true 时该档走本机 OAuth、不需要 api_key。
    let claude_oauth = body
        .get("claude_oauth")
        .and_then(Value::as_bool)
        .filter(|v| *v)
        .map(|_| true);
    // ChatGPT/Codex 订阅档标记。
    let openai_codex = body
        .get("openai_codex")
        .and_then(Value::as_bool)
        .filter(|v| *v)
        .map(|_| true);
    // xAI Grok 订阅档标记。
    let xai_grok = body
        .get("xai_grok")
        .and_then(Value::as_bool)
        .filter(|v| *v)
        .map(|_| true);
    // Gemini（Google 账号 / Code Assist）订阅档标记。
    let gemini_oauth = body
        .get("gemini_oauth")
        .and_then(Value::as_bool)
        .filter(|v| *v)
        .map(|_| true);
    // 是否支持图片输入：模型管理里的显式开关（true/false 都记下，覆盖 provider 目录默认）；
    // 字段缺失（旧前端/旧配置）才留 None，由目录白名单推断。
    let vision = body.get("vision").and_then(Value::as_bool);
    // 推理强度档位：字段缺失=None（用全局默认）；空串=显式关闭；"low".."max"=按档位开启。
    // 三态都有意义，故此处保留空串（不走会丢空值的 str_field）。
    let reasoning_effort = body
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string());

    let profile = config::LlmProfile {
        id,
        name: str_field("name").unwrap_or_default(),
        provider: str_field("provider"),
        model: str_field("model"),
        base_url,
        api_key: new_key,
        price_in: num_field("price_in"),
        price_out: num_field("price_out"),
        price_cache_read: num_field("price_cache_read"),
        max_tokens,
        claude_oauth,
        openai_codex,
        xai_grok,
        gemini_oauth,
        vision,
        reasoning_effort,
    };
    let new_id = c.upsert(profile);
    let saved = config::save(&c);
    let describe = hot_reload(&state);
    Json(json!({
        "ok": saved.is_ok(),
        "id": new_id,
        "error": saved.err().map(|e| e.to_string()),
        "agent": describe,
    }))
}

async fn delete_llm(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    let mut c = config::load();
    c.remove(&id);
    let saved = config::save(&c);
    let describe = hot_reload(&state);
    Json(json!({ "ok": saved.is_ok(), "agent": describe }))
}

/// 重建 agent（重新读配置 + 环境变量），热替换运行实例，返回其描述。
fn hot_reload(state: &AppState) -> String {
    let new_agent = Arc::new(Agent::configure());
    let describe = new_agent.describe();
    *state.agent.lock().unwrap() = new_agent;
    describe
}

#[cfg(test)]
mod tests {
    use super::{onebot_reply_target, parse_onebot_event, resolve_artifact_path};
    use serde_json::json;
    use std::path::{Path, PathBuf};

    fn group_msg(text: &str) -> serde_json::Value {
        json!({
            "post_type": "message", "message_type": "group",
            "self_id": 100, "user_id": 200, "group_id": 300,
            "raw_message": text,
        })
    }

    #[test]
    fn onebot_group_requires_the_trigger_prefix() {
        let inb = parse_onebot_event(&group_msg("wc  在吗")).expect("带前缀应触发");
        assert_eq!(inb.prompt, "在吗", "前缀与两侧空白都应剥掉");
        assert_eq!(inb.group_id, Some(300));
        assert_eq!(inb.user_id, Some(200));
        // 群里的普通闲聊不该惊动 agent，否则客服群一有人说话就烧 token。
        assert!(parse_onebot_event(&group_msg("今天天气不错")).is_none());
        // 只发了个前缀 → 没有正文可跑。
        assert!(parse_onebot_event(&group_msg("wc   ")).is_none());
    }

    #[test]
    fn onebot_private_needs_no_prefix() {
        let ev = json!({
            "post_type": "message", "message_type": "private",
            "self_id": 100, "user_id": 200, "raw_message": " 帮我查下订单 ",
        });
        let inb = parse_onebot_event(&ev).expect("私聊应直接触发");
        assert_eq!(inb.prompt, "帮我查下订单");
        assert_eq!(inb.group_id, None);
        assert_eq!(inb.user_id, Some(200));
    }

    #[test]
    fn onebot_ignores_the_bots_own_messages() {
        // 回归点：NapCat 开 reportSelfMessage 时会把机器人自己的回复报回来。私聊无前缀触发，
        // 不挡就是自问自答的死循环——现在每轮还会写进持久历史，把会话喂坏。
        let ev = json!({
            "post_type": "message", "message_type": "private",
            "self_id": 100, "user_id": 100, "raw_message": "这是机器人刚发的回复",
        });
        assert!(parse_onebot_event(&ev).is_none());
        // 群里同理（机器人自己发的也可能恰好以 wc 开头）。
        let ev = json!({
            "post_type": "message", "message_type": "group",
            "self_id": 100, "user_id": 100, "group_id": 300, "raw_message": "wc 自己喊自己",
        });
        assert!(parse_onebot_event(&ev).is_none());
    }

    #[test]
    fn onebot_command_results_never_echo_into_the_group() {
        // 回归点：/sessions 会列出全部会话的名字。它要是打进客服群，等于把手上所有活儿
        // 念给客户听——所以命令结果一律私发给发起人，只有普通对话回群。
        assert_eq!(onebot_reply_target(true, Some(300), Some(200)), Some(200));
        assert_eq!(onebot_reply_target(false, Some(300), Some(200)), None);
        // 私聊本来就只有一个去处。
        assert_eq!(onebot_reply_target(false, None, Some(200)), Some(200));
        assert_eq!(onebot_reply_target(true, None, Some(200)), Some(200));
        // 群消息但拿不到 user_id：无处私发，只能回群（调用方会按 None 走群）。
        assert_eq!(onebot_reply_target(true, Some(300), None), None);
    }

    #[test]
    fn onebot_admin_list_parsing_tolerates_spacing_and_junk() {
        use wisecortex_core::config::parse_admin_list;
        assert_eq!(parse_admin_list("123, 456 ,789"), vec![123, 456, 789]);
        // 空段与非法项丢弃，别让一个笔误废掉整份名单。
        assert_eq!(parse_admin_list("123,,abc, 456"), vec![123, 456]);
        assert!(parse_admin_list("").is_empty());
        assert!(parse_admin_list("  ,  ").is_empty());
    }

    #[test]
    fn onebot_ignores_non_message_events() {
        // 心跳、通知、请求等事件占上报的绝大多数，必须静默放过。
        for post_type in ["meta_event", "notice", "request", "message_sent"] {
            let ev = json!({
                "post_type": post_type, "message_type": "private",
                "self_id": 100, "user_id": 200, "raw_message": "在吗",
            });
            assert!(
                parse_onebot_event(&ev).is_none(),
                "{post_type} 不应触发 agent"
            );
        }
    }

    #[test]
    fn artifact_path_absolute_is_unchanged() {
        let abs = if cfg!(windows) {
            "C:/tmp/a.html"
        } else {
            "/tmp/a.html"
        };
        let r = resolve_artifact_path(abs, None, Some(Path::new("D:/ws")));
        assert_eq!(r, PathBuf::from(abs));
    }

    #[test]
    fn artifact_path_relative_falls_back_to_workspace_not_process_cwd() {
        // 回归点：cwd 缺省/空白时，相对路径必须落到全局工作目录（agent 写文件的 base），
        // 而不是服务进程 CWD——否则默认工作空间下的产物预览会「文件不存在」。
        let ws = Path::new("D:/ws");
        assert_eq!(
            resolve_artifact_path("report.html", None, Some(ws)),
            ws.join("report.html")
        );
        assert_eq!(
            resolve_artifact_path("report.html", Some("  "), Some(ws)),
            ws.join("report.html")
        );
    }

    #[test]
    fn artifact_path_relative_prefers_valid_cwd_over_workspace() {
        // 任务显式设了 working_dir（且确为目录）时，以它为准，优先于全局 workspace。
        let tmp = std::env::temp_dir();
        let cwd = tmp.to_string_lossy().to_string();
        let r = resolve_artifact_path("a.txt", Some(&cwd), Some(Path::new("D:/ws")));
        assert_eq!(r, tmp.join("a.txt"));
    }
}
