//! 飞书长连接连接器：协商端点 → 连 wss → 收事件 → 映射到持久会话跑 run_turn → 回原会话。
//!
//! ⚠️ 实现飞书专有长连接协议（无官方 Rust SDK），**未经真机验证**——需用真实飞书应用联网验收。
//! 仅当 `feishu.json` 的 `long_conn=true` 且凭据就绪时启动。注意：wss 连接本身不经配置代理
//! （端点协商经代理），如需代理 wss 可后续补。收消息逻辑与事件订阅回调一致。
//!
//! 防重复回复三件套：先回执再处理（agent 一轮可达分钟级，晚回执会被飞书判超时重投）、
//! event_id 去重（重投/回调+长连接双开）、发送者过滤（防自激回环）。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use wisecortex_core::feishu;
use wisecortex_core::feishu_longconn::{self as lc, Frame, Reassembler};

use crate::agent::Agent;
use crate::registry::{ImOrigin, SessionRegistry};

/// 启动长连接监管循环。**始终常驻**：按 `feishu.json` 的 `long_conn` 开关自动连接/断开，
/// 设置页打开开关即生效，无需重启后端（关闭则下次配置检查时断开）。
pub fn spawn(agent: Arc<Mutex<Arc<Agent>>>, reg: SessionRegistry) {
    wisecortex_core::sprintln!("feishu: 长连接监管已启动（按开关自动连接/断开，改开关即生效）。");
    tokio::spawn(async move { run_loop(agent, reg).await });
}

async fn run_loop(agent: Arc<Mutex<Arc<Agent>>>, reg: SessionRegistry) {
    let mut backoff = 1u64;
    loop {
        let cfg = feishu::load();
        if !cfg.long_conn || !cfg.is_ready() {
            // 未启用：空闲轮询等待开关打开（无需重启）。
            tokio::time::sleep(Duration::from_secs(5)).await;
            backoff = 1;
            continue;
        }
        match connect_once(&agent, &reg, &cfg).await {
            // 正常断开（多半是开关被关）→ 不退避，回到轮询。
            Ok(()) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
                backoff = 1;
            }
            Err(e) => {
                wisecortex_core::seprintln!("feishu 长连接断开/失败：{e}");
                // 落盘一条——飞书这条链路此前只往 stdout 打印，桌面端 GUI 直接丢弃，
                // error.log 又只收 buglog::record，于是「机器人不回复」两次都查成了悬案。
                // 只在刚从正常转为失败时记（backoff 尚未退避过），否则持续断网会每 30s
                // 刷一条，把 error.log 里别的错误顶掉。
                if backoff == 1 {
                    wisecortex_core::buglog::record("feishu", &format!("长连接断开/失败：{e}"));
                }
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(30);
            }
        }
    }
}

async fn connect_once(
    agent: &Arc<Mutex<Arc<Agent>>>,
    reg: &SessionRegistry,
    cfg: &feishu::FeishuConfig,
) -> Result<(), String> {
    let app_id = cfg.app_id.clone().unwrap_or_default();
    let app_secret = cfg.app_secret.clone().unwrap_or_default();
    let ep = tokio::task::spawn_blocking(move || lc::endpoint(&app_id, &app_secret))
        .await
        .map_err(|e| e.to_string())??;

    let host = ep
        .url
        .split('/')
        .nth(2)
        .unwrap_or(ep.url.as_str())
        .to_string();
    wisecortex_core::sprintln!(
        "feishu: 端点已协商（host={host}, ping={}s），开始建立 wss…",
        ep.ping_interval_secs
    );
    let (ws, _) = tokio_tungstenite::connect_async(ep.url.as_str())
        .await
        .map_err(|e| format!("wss 连接失败（host={host}）：{e}"))?;
    let (mut tx, mut rx) = ws.split();
    let mut reasm = Reassembler::default();
    wisecortex_core::sprintln!("feishu: 长连接已建立，等待事件…");

    // 每 5s 复查一次配置：开关被关、或凭据被换（重新扫码）都主动断开（无需重启即可生效）。
    let mut toggle_check = tokio::time::interval(Duration::from_secs(5));
    toggle_check.tick().await; // 跳过立即触发的首拍

    // 最后一次收到任何帧（含服务端 ping）的时刻，供空闲看门狗判活。见 [`idle_limit`]。
    let mut last_frame = std::time::Instant::now();

    loop {
        let msg = tokio::select! {
            m = rx.next() => match m {
                Some(m) => {
                    last_frame = std::time::Instant::now();
                    m
                }
                None => return Ok(()), // 流结束
            },
            _ = toggle_check.tick() => {
                // 空闲看门狗：僵尸连接不会报错，只会永远安静，必须自己数着时间判死。
                let idle = last_frame.elapsed();
                if idle > idle_limit(ep.ping_interval_secs) {
                    let why = format!(
                        "已 {}s 未收到任何帧（端点声明 ping 间隔 {}s），判定链路已死，主动重连",
                        idle.as_secs(),
                        ep.ping_interval_secs
                    );
                    wisecortex_core::sprintln!("feishu: {why}。");
                    wisecortex_core::buglog::record("feishu", &why);
                    return Ok(());
                }
                match recheck(cfg, &feishu::load()) {
                    Recheck::Keep => continue,
                    Recheck::Disconnect(why) => {
                        wisecortex_core::sprintln!("feishu: {why}，断开当前长连接（将按最新配置重连）。");
                        return Ok(());
                    }
                }
            }
        };
        match msg.map_err(|e| e.to_string())? {
            Message::Binary(bytes) => {
                let frame = match Frame::decode(bytes.as_ref()) {
                    Ok(f) => f,
                    Err(e) => {
                        wisecortex_core::seprintln!("feishu 帧解码失败：{e}");
                        continue;
                    }
                };
                if lc::is_ping(&frame) {
                    let pong = lc::build_pong(&frame).encode();
                    tx.send(Message::Binary(pong))
                        .await
                        .map_err(|e| e.to_string())?;
                    continue;
                }
                // 每个数据帧记一行。卡片回调曾经静默失败（不报错、不回显、日志一片空白），
                // 没有这行就只能靠猜「帧到底有没有到」——一行的代价，换掉整个黑箱。
                wisecortex_core::sprintln!(
                    "feishu: 数据帧 type={} method={} payload={}B",
                    frame.header("type").unwrap_or("?"),
                    frame.method,
                    frame.payload.len()
                );
                match reasm.push(&frame) {
                    Ok(Some(full)) => {
                        // 卡片按钮回调（card.action.trigger）走完全不同的一条路：飞书要求
                        // **3 秒内**响应，且响应体本身就是结果（toast/更新后的卡片）——不能
                        // 「先回执 200 再丢后台」。好在它只改一个绑定（亚毫秒、不碰 LLM），
                        // 直接在读循环里同步做完、把响应塞进回执帧的 payload。
                        if let Some(action) = feishu::parse_card_action(&full) {
                            let started = std::time::Instant::now();
                            let out = crate::commands::handle_card_action(reg, &action);
                            wisecortex_core::sprintln!(
                                "feishu: 卡片回调 value={} → {}",
                                action.value,
                                out.response
                            );
                            let mut ack = build_ack(&frame);
                            ack.headers.push((
                                "biz_rt".to_string(),
                                started.elapsed().as_millis().to_string(),
                            ));
                            ack.payload = lc::response_payload(Some(&out.response));
                            let _ = tx.send(Message::Binary(ack.encode())).await;
                            // 近况另发一条消息：发消息是网络往返，塞进 3 秒应答窗口会拖超时。
                            if let Some(recap) = out.recap {
                                let cfg = cfg.clone();
                                let chat_id = action.chat_id.clone();
                                tokio::spawn(async move {
                                    let sent = tokio::task::spawn_blocking(move || {
                                        feishu::send_text(&cfg, &chat_id, &recap)
                                    })
                                    .await;
                                    if let Ok(Err(e)) = sent {
                                        wisecortex_core::seprintln!(
                                            "feishu: 切换后的近况发送失败：{e}"
                                        );
                                    }
                                });
                            }
                            continue;
                        }
                        // 没认出是卡片回调：把原始 payload 打出来。长连接下的卡片回调结构
                        // 若与 webhook 不同（parse_card_action 按 webhook 的 schema 写的），
                        // 这里就是唯一能看见真实结构的地方。
                        if frame.header("type") == Some("card") {
                            wisecortex_core::sprintln!(
                                "feishu: ⚠ card 帧未被识别为卡片回调，原始 payload：{}",
                                String::from_utf8_lossy(&full)
                            );
                        }
                        // 普通事件（用户发消息）：先回执、处理丢后台——agent 一轮可达分钟级，
                        // 若处理完才回执，飞书早已判超时重投（反复触发、反复回复）；且 await
                        // 会卡死读循环，ping 无人应答 → 断连重连 → 未回执事件又被重投。
                        let _ = tx.send(Message::Binary(build_ack(&frame).encode())).await;
                        let agent = agent.clone();
                        let reg = reg.clone();
                        let cfg = cfg.clone();
                        tokio::spawn(async move { handle_event(&agent, &reg, &cfg, &full).await });
                    }
                    Ok(None) => {}
                    Err(e) => wisecortex_core::seprintln!("feishu 分片重组失败：{e}"),
                }
            }
            Message::Ping(p) => {
                let _ = tx.send(Message::Pong(p)).await;
            }
            Message::Close(_) => return Ok(()),
            _ => {}
        }
    }
}

async fn handle_event(
    agent: &Arc<Mutex<Arc<Agent>>>,
    reg: &SessionRegistry,
    cfg: &feishu::FeishuConfig,
    payload: &[u8],
) {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(payload) else {
        wisecortex_core::seprintln!(
            "feishu: 事件 payload 非 JSON（{} 字节），忽略",
            payload.len()
        );
        return;
    };
    // 去重：飞书对未及时回执的事件会重投；回调 URL 与长连接双开时同一事件也到两次。
    if let Some(eid) = feishu::extract_event_id(&v) {
        if feishu::seen_recently(&eid) {
            wisecortex_core::sprintln!("feishu: 重复事件（event_id={eid}），跳过");
            return;
        }
    }
    if !feishu::sender_is_user(&v) {
        return; // 机器人消息不处理，防自激回环
    }
    let Some((_token, chat_id, text)) = feishu::extract_message(&v) else {
        // 非 text 事件（图片/系统等）忽略。打印事件类型便于排查“无响应”。
        let etype = v
            .get("header")
            .and_then(|h| h.get("event_type"))
            .and_then(|x| x.as_str())
            .unwrap_or("?");
        wisecortex_core::sprintln!("feishu: 收到非文本事件（type={etype}），忽略");
        return;
    };
    if text.trim().is_empty() {
        return;
    }
    feishu::record_chat(&chat_id); // 记下来源会话，供「复用应用」出站推送选目标
    wisecortex_core::sprintln!(
        "feishu: 收到消息（chat={chat_id}, {} 字），处理中…",
        text.chars().count()
    );
    let (reply, card) = run_im_turn(agent, reg, &chat_id, text.trim()).await;
    let cfg = cfg.clone();
    // 命令产出了互动卡片（/sessions）就发卡片，否则发纯文本。
    let sent = tokio::task::spawn_blocking(move || match card {
        Some(c) => feishu::send_card(&cfg, &chat_id, &c),
        None => feishu::send_text(&cfg, &chat_id, &reply),
    })
    .await;
    match sent {
        Ok(Ok(())) => wisecortex_core::sprintln!("feishu: 已回复"),
        Ok(Err(e)) => wisecortex_core::seprintln!("feishu: 回复失败：{e}"),
        Err(e) => wisecortex_core::seprintln!("feishu: 回复任务异常：{e}"),
    }
}

/// 把一条飞书消息接入持久会话跑一个完整回合，返回应回给用户的文本。
/// 会话绑定与回合执行的公共逻辑在 [`crate::im`]（与微信 ClawBot 共用）；默认会话 id
/// 仍是 `feishu-<chat_id>`，与历史行为逐字一致。REST 回调与长连接共用本函数。
pub(crate) async fn run_im_turn(
    agent: &Arc<Mutex<Arc<Agent>>>,
    reg: &SessionRegistry,
    chat_id: &str,
    text: &str,
) -> (String, Option<serde_json::Value>) {
    crate::im::run_im_turn(agent, reg, ImOrigin::feishu(chat_id), "飞书", text).await
}

/// 空闲阈值下限。端点若返回 0 或异常小的 PingInterval，阈值会被压到秒级，
/// 健康连接就会被反复误杀成疯狂重连——用下限兜住。
const IDLE_FLOOR: Duration = Duration::from_secs(60);

/// 多久收不到任何帧就判定链路已死。
///
/// 飞书长连接是**单向保活**：服务端每 `ClientConfig.PingInterval` 秒 ping 我们一次，我们只回
/// pong，自己从不主动发帧。于是链路一旦单向断掉（NAT 老化、运营商回收、对端静默丢弃），
/// `rx.next()` 会永久 pending：我们一个字节都不写，TCP 不会报错，socket 在系统里始终是
/// ESTABLISHED——应用自认为连着，事件全部石沉大海，不报错、不重连、日志一片空白。
/// （对照 `qq_ws.rs`：QQ 主动发心跳，写失败即判死，所以它能自愈。）
///
/// 给协议声明间隔的 2 倍冗余：丢一次 ping 不误杀，丢两次就重连。
fn idle_limit(ping_interval_secs: u64) -> Duration {
    Duration::from_secs(ping_interval_secs.saturating_mul(2)).max(IDLE_FLOOR)
}

/// 连接存活期间的巡检结论。
#[derive(Debug, PartialEq)]
enum Recheck {
    Keep,
    /// 断开当前连接（`run_loop` 随后会按最新配置重连）；附带原因用于日志。
    Disconnect(&'static str),
}

/// 建连时对配置拍的快照（`live`）可能已经过期，拿最新配置（`latest`）比对。
///
/// 光看 `long_conn` 开关不够：**二维码就在桌面 App 的界面里**，重新扫码建新机器人时
/// 这条连接必然正开着，而扫码会换掉 app_id/secret。旧连接靠 ping/pong 能一直活着，
/// 于是新机器人没有任何活连接可推事件——发给它的消息全部石沉大海，且毫无报错。
fn recheck(live: &feishu::FeishuConfig, latest: &feishu::FeishuConfig) -> Recheck {
    if !latest.long_conn {
        return Recheck::Disconnect("长连接开关已关闭");
    }
    if latest.app_id != live.app_id || latest.app_secret != live.app_secret {
        return Recheck::Disconnect("飞书凭据已更换（多半是重新扫码建了新机器人）");
    }
    Recheck::Keep
}

/// 数据帧回执：保留服务端用于关联的路由字段（seq_id/log_id/service/method/headers），
/// 仅把 payload 换成应答信封。早先版本丢了这些字段，服务端可能无法关联回执。
/// payload 的格式见 [`lc::response_payload`]（卡片回调要带 data，普通事件不带）。
fn build_ack(data: &Frame) -> Frame {
    Frame {
        seq_id: data.seq_id,
        log_id: data.log_id,
        service: data.service,
        method: data.method,
        headers: data.headers.clone(),
        payload_encoding: String::new(),
        payload_type: "json".to_string(),
        payload: lc::response_payload(None),
        log_id_new: data.log_id_new.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(app_id: &str, app_secret: &str, long_conn: bool) -> feishu::FeishuConfig {
        feishu::FeishuConfig {
            app_id: Some(app_id.to_string()),
            app_secret: Some(app_secret.to_string()),
            verify_token: None,
            long_conn,
        }
    }

    #[test]
    fn idle_limit_is_double_the_declared_ping_interval() {
        // 端点在 ClientConfig.PingInterval 里声明它多久 ping 我们一次。给 2 倍冗余：
        // 丢一次 ping 不误杀，丢两次就认定链路已死。
        assert_eq!(idle_limit(120), Duration::from_secs(240));
        assert_eq!(idle_limit(90), Duration::from_secs(180));
    }

    #[test]
    fn idle_limit_never_goes_below_the_floor() {
        // 端点返回 0 或异常小的间隔时，阈值不能被压到秒级——否则健康连接会被反复误杀，
        // 变成疯狂重连。下限兜底。
        assert_eq!(idle_limit(0), IDLE_FLOOR);
        assert_eq!(idle_limit(5), IDLE_FLOOR);
    }

    #[test]
    fn recheck_keeps_connection_when_config_unchanged() {
        let live = cfg("cli_a", "secret_a", true);
        assert_eq!(recheck(&live, &live.clone()), Recheck::Keep);
    }

    #[test]
    fn recheck_disconnects_when_toggle_turned_off() {
        let live = cfg("cli_a", "secret_a", true);
        let latest = cfg("cli_a", "secret_a", false);
        assert!(matches!(recheck(&live, &latest), Recheck::Disconnect(_)));
    }

    #[test]
    fn recheck_disconnects_when_credentials_replaced_by_new_scan() {
        // 二维码就在桌面 App 界面里：重新扫码时这条连接必然正开着，且扫码会建一个
        // 全新的机器人（新 app_id/secret）。旧连接靠 ping/pong 能一直活着，若不主动
        // 断开，新机器人就没有任何活连接可推事件 —— 发给它的消息会石沉大海。
        let live = cfg("cli_old", "secret_old", true);
        let latest = cfg("cli_new", "secret_new", true);
        assert!(matches!(recheck(&live, &latest), Recheck::Disconnect(_)));
    }

    #[test]
    fn recheck_disconnects_when_only_secret_rotated() {
        // app_id 不变、仅密钥轮换（重置密钥）：旧连接同样是废的，必须重连。
        let live = cfg("cli_a", "secret_old", true);
        let latest = cfg("cli_a", "secret_new", true);
        assert!(matches!(recheck(&live, &latest), Recheck::Disconnect(_)));
    }

    #[test]
    fn ack_preserves_routing_fields() {
        let data = Frame {
            seq_id: 11,
            log_id: 22,
            service: 3,
            method: 4,
            headers: vec![
                ("type".into(), "event".into()),
                ("message_id".into(), "m9".into()),
            ],
            payload_encoding: "gzip".into(),
            payload_type: "pb".into(),
            payload: b"orig".to_vec(),
            log_id_new: "ln".into(),
        };
        let ack = build_ack(&data);
        assert_eq!(ack.seq_id, 11);
        assert_eq!(ack.log_id, 22);
        assert_eq!(ack.service, 3);
        assert_eq!(ack.method, 4);
        assert_eq!(ack.header("message_id"), Some("m9"));
        assert_eq!(ack.log_id_new, "ln");
        assert_eq!(ack.payload, b"{\"code\":200}");
        // 回执不应继承请求的 gzip 编码（payload 是明文 JSON）。
        assert_eq!(ack.payload_encoding, "");
    }
}
