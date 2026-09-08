//! REST 接口测试。用 tower oneshot 直接打 Router（不绑端口、不写真实配置文件）。
//! 只测只读端点（providers/config GET），避免 POST 改动用户实际 config.json。

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt; // oneshot
use wisecortex_server::{app, Agent, AppState, SessionRegistry};

fn test_app() -> axum::Router {
    let state = AppState::new(SessionRegistry::new(), Arc::new(Agent::Echo));
    app(state)
}

async fn get_json(uri: &str) -> (StatusCode, Value) {
    let resp = test_app()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, v)
}

#[tokio::test]
async fn providers_lists_presets() {
    let (status, v) = get_json("/api/providers").await;
    assert_eq!(status, StatusCode::OK);
    let arr = v["providers"].as_array().unwrap();
    assert!(arr.len() >= 5);
    assert!(arr.iter().any(|p| p["id"] == "deepseek"));
}

#[tokio::test]
async fn config_get_returns_shape_without_raw_key() {
    let (status, v) = get_json("/api/config").await;
    assert_eq!(status, StatusCode::OK);
    // 不回传明文 api_key，只回「当前档能不能用」的布尔。
    assert!(v.get("api_key").is_none());
    assert!(v.get("llm_ready").and_then(|r| r.as_bool()).is_some());
    assert!(v.get("auto_approve").is_some());
    // 暴露当前全局工作目录（绝对路径字符串）。
    assert!(v.get("workspace").and_then(|w| w.as_str()).is_some());
    // 暴露出站代理字段（默认空串）。
    assert!(v.get("proxy").and_then(|w| w.as_str()).is_some());
}

fn app_with_key() -> axum::Router {
    let state = AppState::new(SessionRegistry::new(), Arc::new(Agent::Echo))
        .with_access_key(Some("secret".to_string()));
    app(state)
}

async fn status_of(app: axum::Router, uri: &str, header: Option<(&str, &str)>) -> StatusCode {
    let mut b = Request::builder().uri(uri);
    if let Some((k, v)) = header {
        b = b.header(k, v);
    }
    app.oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn sessions_list_messages_and_delete() {
    use wisecortex_core::llm::ChatMessage;
    let reg = SessionRegistry::new();
    reg.ensure("s1");
    reg.append_message(
        "s1",
        ChatMessage::user_with_images("look", vec!["data:image/png;base64,AAAA".to_string()]),
    );
    reg.append_message("s1", ChatMessage::assistant("hello"));
    let app = app(AppState::new(reg, Arc::new(Agent::Echo)));

    // 列表
    let (st, v) = {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let s = resp.status();
        let b = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (s, serde_json::from_slice::<Value>(&b).unwrap())
    };
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["sessions"][0]["id"], "s1");

    // 历史回放
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/sessions/s1/messages")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let b = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["messages"].as_array().unwrap().len(), 2);
    assert_eq!(v["messages"][0]["role"], "user");
    // 历史回放需带回图片，前端据此重画缩略图。
    assert_eq!(v["messages"][0]["images"][0], "data:image/png;base64,AAAA");
    assert_eq!(v["messages"][1]["content"], "hello");

    // 删除
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/api/sessions/s1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn session_rename_endpoint_updates_name() {
    let reg = SessionRegistry::new();
    reg.ensure("s1");
    reg.set_name_if_empty("s1", "自动名");
    let app = app(AppState::new(reg, Arc::new(Agent::Echo)));

    let post = |uri: &'static str, body: &'static str| {
        let app = app.clone();
        async move {
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(uri)
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            let st = resp.status();
            let b = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            (st, serde_json::from_slice::<Value>(&b).unwrap())
        }
    };

    // 正常重命名 → ok，列表里名字已更新。
    let (st, v) = post("/api/sessions/s1/name", r#"{"name":"修 MUD 战斗"}"#).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["ok"], true);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let b = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["sessions"][0]["name"], "修 MUD 战斗");

    // 空白名 / 不存在的会话 → ok:false。
    let (_, v) = post("/api/sessions/s1/name", r#"{"name":"   "}"#).await;
    assert_eq!(v["ok"], false);
    let (_, v) = post("/api/sessions/nope/name", r#"{"name":"x"}"#).await;
    assert_eq!(v["ok"], false);
}

#[tokio::test]
async fn oauth_xai_status_endpoint_is_routed() {
    // 只验证路由与响应形状（logged_in 布尔）；start/poll 需要真网络，不在单测覆盖。
    let (status, v) = get_json("/api/oauth/xai/status").await;
    assert_eq!(status, StatusCode::OK);
    assert!(v.get("logged_in").and_then(Value::as_bool).is_some());
}

#[tokio::test]
async fn channels_and_cron_list_endpoints() {
    let (s1, v1) = get_json("/api/channels").await;
    assert_eq!(s1, StatusCode::OK);
    assert!(v1.get("channels").is_some());
    let (s2, v2) = get_json("/api/cron").await;
    assert_eq!(s2, StatusCode::OK);
    assert!(v2.get("tasks").is_some());
}

#[tokio::test]
async fn skills_catalog_returns_entries_array() {
    // 「我的技能」反映数据目录里已安装的技能（内置由 server 启动时播种），
    // 这里只校验端点形状（entries 为数组），避免依赖全局数据目录状态。
    let (status, v) = get_json("/api/skills/catalog").await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["entries"].as_array().is_some());
}

#[tokio::test]
async fn skills_sources_lists_builtin_sources() {
    let (status, v) = get_json("/api/skills/sources").await;
    assert_eq!(status, StatusCode::OK);
    let arr = v["sources"].as_array().unwrap();
    // 精选源至少含静态默认源与 ClawHub。
    assert!(arr.iter().any(|s| s["kind"] == "static"));
    assert!(arr.iter().any(|s| s["kind"] == "clawhub"));
    // 当前源默认有值。
    assert!(v["current"]["url"].as_str().is_some());
}

#[tokio::test]
async fn im_onebot_ignores_non_message_events() {
    let app = app(AppState::new(SessionRegistry::new(), Arc::new(Agent::Echo)));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/im/onebot")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"post_type":"meta_event"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn feishu_url_verification_echoes_challenge() {
    let app = app(AppState::new(SessionRegistry::new(), Arc::new(Agent::Echo)));
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/im/feishu")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"type":"url_verification","challenge":"abc123"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let b = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["challenge"], "abc123");
}

#[tokio::test]
async fn access_key_required_when_configured() {
    // 无密钥 → 401
    assert_eq!(
        status_of(app_with_key(), "/api/providers", None).await,
        StatusCode::UNAUTHORIZED
    );
    // query 带正确密钥 → 200
    assert_eq!(
        status_of(app_with_key(), "/api/providers?access_key=secret", None).await,
        StatusCode::OK
    );
    // header 带正确密钥 → 200
    assert_eq!(
        status_of(
            app_with_key(),
            "/api/providers",
            Some(("x-access-key", "secret"))
        )
        .await,
        StatusCode::OK
    );
    // 错误密钥 → 401
    assert_eq!(
        status_of(app_with_key(), "/api/providers?access_key=wrong", None).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn auth_status_reports_required_and_authorized() {
    async fn body_of(
        app: axum::Router,
        uri: &str,
        header: Option<(&str, &str)>,
    ) -> (StatusCode, Value) {
        let mut b = Request::builder().uri(uri);
        if let Some((k, v)) = header {
            b = b.header(k, v);
        }
        let resp = app.oneshot(b.body(Body::empty()).unwrap()).await.unwrap();
        let st = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (st, serde_json::from_slice(&bytes).unwrap())
    }

    // 启用密钥：status 自身免密可访问（200），required=true。
    // 无密钥 → authorized=false。
    let (st, v) = body_of(app_with_key(), "/api/auth/status", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["required"], true);
    assert_eq!(v["authorized"], false);
    // 正确密钥（header）→ authorized=true。
    let (_, v) = body_of(
        app_with_key(),
        "/api/auth/status",
        Some(("x-access-key", "secret")),
    )
    .await;
    assert_eq!(v["authorized"], true);
    // 错误密钥 → authorized=false。
    let (_, v) = body_of(app_with_key(), "/api/auth/status?access_key=wrong", None).await;
    assert_eq!(v["authorized"], false);

    // 公开模式（未设密钥）→ required=false, authorized=true。
    let (st, v) = get_json("/api/auth/status").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["required"], false);
    assert_eq!(v["authorized"], true);
}

/// probe=1 只回「在不在」，不带 content——前端在每轮结束时要逐个核对产物入口是否还指向
/// 真实文件（写失败、或 agent 本轮里又把文件挪走的，入口不该留下），只为判存在把几 MB
/// 内容传一遍纯属浪费。同时钉住：不存在的路径无论 probe 与否都必须 ok=false。
#[tokio::test]
async fn artifact_probe_reports_existence_without_content() {
    let dir = std::env::temp_dir().join("wc_artifact_probe_test");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("probe_me.md");
    std::fs::write(&file, "# hello\n").unwrap();
    let enc = |s: &str| {
        s.chars()
            .map(|c| match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
                c => c.to_string().bytes().map(|b| format!("%{b:02X}")).collect(),
            })
            .collect::<String>()
    };
    let p = enc(&file.to_string_lossy());

    // probe=1：ok 且带 name，但不含 content。
    let (st, v) = get_json(&format!("/api/artifact?probe=1&path={p}")).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["ok"], true);
    assert_eq!(v["name"], "probe_me.md");
    assert!(v.get("content").is_none(), "probe 不该回内容：{v}");

    // 不带 probe：照常回内容。
    let (_, v) = get_json(&format!("/api/artifact?path={p}")).await;
    assert_eq!(v["ok"], true);
    assert_eq!(v["content"], "# hello\n");

    // 不存在的文件：probe 也必须 ok=false，否则死入口照样留下。
    let gone = enc(&dir.join("not_here.md").to_string_lossy());
    let (_, v) = get_json(&format!("/api/artifact?probe=1&path={gone}")).await;
    assert_eq!(v["ok"], false);

    std::fs::remove_file(&file).ok();
}
