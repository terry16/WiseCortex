//! QQ 官方机器人网关连接器：token → gateway → wss → IDENTIFY → 心跳 → DISPATCH → run_once → 被动回复。
//!
//! ⚠️ **未经真机验证**——需用真实 QQ 机器人凭据联网验收。
//! 仅当 `qq.json` 的 `enabled=true` 且凭据就绪时连接；设置页开关热生效，无需重启后端。
//! QQ 网关帧是 wss 上的明文 JSON（比飞书的 protobuf 长连接简单：无分片、无 gzip）。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use wisecortex_core::qq::{self, Inbound};

use crate::agent::Agent;

/// 一次连接的结束方式。
enum End {
    /// 正常断开（多半是开关被关）→ 不退避，回到轮询。
    Normal,
    /// 致命（intents 被拒等）→ 不立即重连，等久一点或等开关。
    Fatal,
}

/// 启动网关监管循环。**始终常驻**：按 `qq.json` 的 `enabled` 开关自动连接/断开。
pub fn spawn(agent: Arc<Mutex<Arc<Agent>>>) {
    wisecortex_core::sprintln!(
        "qq: 官方机器人网关监管已启动（按开关自动连接/断开，改开关即生效）。"
    );
    tokio::spawn(async move { run_loop(agent).await });
}

async fn run_loop(agent: Arc<Mutex<Arc<Agent>>>) {
    let mut backoff = 1u64;
    loop {
        let cfg = qq::load();
        if !cfg.enabled || !cfg.is_ready() {
            tokio::time::sleep(Duration::from_secs(5)).await;
            backoff = 1;
            continue;
        }
        match connect_once(&agent, &cfg).await {
            Ok(End::Normal) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
                backoff = 1;
            }
            Ok(End::Fatal) => {
                wisecortex_core::seprintln!(
                    "qq: 致命错误，暂停重连 30s（多半是机器人在开放平台缺少群/私聊消息权限，去 q.qq.com 申请后重试）。"
                );
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
            Err(e) => {
                wisecortex_core::seprintln!("qq 网关断开/失败：{e}");
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(30);
            }
        }
    }
}

async fn connect_once(agent: &Arc<Mutex<Arc<Agent>>>, cfg: &qq::QqConfig) -> Result<End, String> {
    let app_id = cfg.app_id.clone().unwrap_or_default();
    let secret = cfg.app_secret.clone().unwrap_or_default();

    // token + 网关地址（blocking、不走代理）。
    let (token, gateway) =
        tokio::task::spawn_blocking(move || -> Result<(String, String), String> {
            let t = qq::get_access_token(&app_id, &secret)?;
            let g = qq::get_gateway_url(&t)?;
            Ok((t, g))
        })
        .await
        .map_err(|e| e.to_string())??;

    wisecortex_core::sprintln!("qq: access_token 就绪，网关 {gateway}，建立 wss…");
    let (ws, _) = tokio_tungstenite::connect_async(gateway.as_str())
        .await
        .map_err(|e| format!("wss 连接失败：{e}"))?;
    let (mut tx, mut rx) = ws.split();

    let mut last_seq: Option<u64> = None;
    let mut identified = false;
    // HELLO 之前用占位心跳间隔；收到 HELLO 后按服务端 heartbeat_interval 重置。
    let mut hb = tokio::time::interval(Duration::from_secs(30));
    hb.tick().await; // 跳过 interval 立即触发的首拍
                     // 每 5s 查一次开关：被关掉就主动断开。
    let mut toggle = tokio::time::interval(Duration::from_secs(5));
    toggle.tick().await;

    loop {
        tokio::select! {
            m = rx.next() => {
                let Some(m) = m else { return Ok(End::Normal); };
                match m.map_err(|e| e.to_string())? {
                    Message::Text(s) => {
                        let v: serde_json::Value = match serde_json::from_str(&s) {
                            Ok(v) => v,
                            Err(e) => { wisecortex_core::seprintln!("qq: 帧非 JSON：{e}"); continue; }
                        };
                        if let Some(seq) = v.get("s").and_then(|x| x.as_u64()) {
                            last_seq = Some(seq);
                        }
                        match v.get("op").and_then(|x| x.as_u64()).unwrap_or(u64::MAX) {
                            qq::OP_HELLO => {
                                let interval_ms = v
                                    .get("d")
                                    .and_then(|d| d.get("heartbeat_interval"))
                                    .and_then(|x| x.as_u64())
                                    .unwrap_or(30_000)
                                    .max(1_000);
                                hb = tokio::time::interval(Duration::from_millis(interval_ms));
                                hb.tick().await; // 跳过首拍
                                let idf = qq::build_identify(&token, qq::INTENTS_V1).to_string();
                                tx.send(Message::Text(idf)).await.map_err(|e| e.to_string())?;
                                identified = true;
                                wisecortex_core::sprintln!("qq: 已发送 IDENTIFY（intents={}），等待 READY…", qq::INTENTS_V1);
                            }
                            qq::OP_DISPATCH => {
                                if v.get("t").and_then(|x| x.as_str()) == Some("READY") {
                                    wisecortex_core::sprintln!("qq: 网关 READY，机器人已上线。");
                                }
                                if let Some(inb) = qq::extract_message(&v) {
                                    handle_message(agent, &token, inb).await;
                                }
                            }
                            qq::OP_HEARTBEAT_ACK => {}
                            qq::OP_RECONNECT => {
                                return Err("服务端要求重连（op7）".into());
                            }
                            qq::OP_INVALID_SESSION => {
                                qq::clear_token_cache();
                                return Err("会话失效（op9），刷新 token 重连".into());
                            }
                            _ => {}
                        }
                    }
                    Message::Ping(p) => { let _ = tx.send(Message::Pong(p)).await; }
                    Message::Close(cf) => {
                        let code = cf.as_ref().map(|c| u16::from(c.code)).unwrap_or(0);
                        wisecortex_core::seprintln!("qq: 连接关闭 code={code}");
                        // 4914/4915：intents 不足/不被允许 → 致命，别狂重连。
                        if code == 4914 || code == 4915 {
                            return Ok(End::Fatal);
                        }
                        // 4004 鉴权失败 / 4006-4009 会话相关 → 刷 token 再连。
                        if matches!(code, 4004 | 4006 | 4007 | 4009) {
                            qq::clear_token_cache();
                        }
                        return Err(format!("连接关闭 code={code}"));
                    }
                    _ => {}
                }
            }
            _ = hb.tick(), if identified => {
                let beat = qq::build_heartbeat(last_seq).to_string();
                if tx.send(Message::Text(beat)).await.is_err() {
                    return Err("心跳发送失败".into());
                }
            }
            _ = toggle.tick() => {
                if !qq::load().enabled {
                    wisecortex_core::sprintln!("qq: 开关已关闭，断开。");
                    return Ok(End::Normal);
                }
            }
        }
    }
}

async fn handle_message(agent: &Arc<Mutex<Arc<Agent>>>, token: &str, inb: Inbound) {
    wisecortex_core::sprintln!(
        "qq: 收到消息（scope={:?}, {} 字），处理中…",
        inb.scope,
        inb.text.chars().count()
    );
    // 记下来源会话，供定时通知出站推送选目标（与飞书 record_chat 同理）。
    qq::record_peer(inb.scope.as_str(), &inb.peer_id);
    let ag = agent.lock().unwrap().clone();
    let reply = ag.run_once(inb.text.trim()).await;
    let reply = reply.trim().to_string();
    if reply.is_empty() {
        wisecortex_core::seprintln!("qq: 回复为空，跳过发送。");
        return;
    }
    let token = token.to_string();
    match tokio::task::spawn_blocking(move || qq::send_reply(&token, &inb, &reply)).await {
        Ok(Ok(())) => wisecortex_core::sprintln!("qq: 已回复"),
        Ok(Err(e)) => wisecortex_core::seprintln!("qq: 回复失败：{e}"),
        Err(e) => wisecortex_core::seprintln!("qq: 回复任务异常：{e}"),
    }
}
