//! 微信 ClawBot 长轮询连接器：getupdates → 映射到持久会话跑 run_turn → sendmessage 回微信。
//!
//! 与飞书/QQ 的根本差别：这里没有 websocket。`getupdates` 是一个挂最多 35 秒的 HTTPS
//! 长轮询，返回后立刻再发一个。没有帧编解码、没有心跳、没有公网回调——因此它在 NAT
//! 后面的桌面端天然可用（Windows / macOS / Linux 同一份代码）。
//!
//! ⚠️ 一个微信号只能建一个 Bot，且 `get_updates_buf` 是**共享游标**：同一个 token 在两处
//! 同时轮询，消息会被随机分走、两边各收到一半。同一时刻只能在一处开启 `enabled`。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use wisecortex_core::clawbot::{self, ClawbotConfig, Fetched, Inbound};

use crate::agent::Agent;
use crate::registry::{ImOrigin, SessionRegistry};

/// 会话超时（errcode=-14）后协议要求暂停的时长。
const SESSION_EXPIRED_PAUSE: Duration = Duration::from_secs(3600);
/// 开关/凭据复查间隔。也是暂停期间的最小响应粒度。
const RECHECK: Duration = Duration::from_secs(5);
/// 「正在输入」的续期间隔（协议建议 5 秒）。
const TYPING_KEEPALIVE: Duration = Duration::from_secs(5);
/// 两次长轮询之间的保底间隔。
///
/// 正常情况服务端会把 `getupdates` 挂满 35 秒，返回即重发是对的。但只要它某次立刻返回
/// （异常、边界、被限流后的快速拒绝），「返回即重发」就变成一个不停打接口的死循环——
/// 而腾讯在条款里明确保留限制调用频率、乃至中断服务的权利。加一道地板兜住。
const MIN_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// 启动长轮询监管循环。**始终常驻**：按 `clawbot.json` 的 `enabled` 开关自动开始/停止，
/// 设置页打开开关即生效，无需重启后端。
pub fn spawn(agent: Arc<Mutex<Arc<Agent>>>, reg: SessionRegistry) {
    wisecortex_core::sprintln!(
        "clawbot: 微信长轮询监管已启动（按开关自动开始/停止，改开关即生效）。"
    );
    tokio::spawn(async move { run_loop(agent, reg).await });
}

/// 一次 `poll_once` 的结束方式。
enum Outcome {
    /// 正常轮询了一轮（可能收到 0 条消息）。
    Polled,
    /// 开关被关或凭据被换 → 回到空闲轮询，按最新配置重来。
    Stopped,
    /// errcode=-14，需要暂停一小时。
    SessionExpired,
}

async fn run_loop(agent: Arc<Mutex<Arc<Agent>>>, reg: SessionRegistry) {
    let mut backoff = 2u64;
    loop {
        let cfg = clawbot::load();
        if !cfg.enabled || !cfg.is_ready() {
            // 未启用：空闲轮询等待开关打开（无需重启）。
            tokio::time::sleep(RECHECK).await;
            backoff = 2;
            continue;
        }
        let started = tokio::time::Instant::now();
        match poll_once(&agent, &reg, &cfg).await {
            Ok(Outcome::Polled) => {
                backoff = 2;
                tokio::time::sleep(poll_cooldown(started.elapsed())).await;
            }
            Ok(Outcome::Stopped) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
                backoff = 2;
            }
            Ok(Outcome::SessionExpired) => {
                let why = "服务端返回 errcode=-14（会话超时），按协议暂停调用 1 小时";
                wisecortex_core::sprintln!("clawbot: {why}。期间关掉再打开开关可立即恢复。");
                wisecortex_core::buglog::record("clawbot", why);
                // 暂停期间仍每 5 秒复查开关：否则用户重新扫码后还得干等满一小时。
                sleep_watching_toggle(SESSION_EXPIRED_PAUSE, &cfg).await;
                backoff = 2;
            }
            Err(e) => {
                wisecortex_core::seprintln!("clawbot 轮询失败：{e}");
                // 只在刚从正常转为失败时落盘一条。持续断网会每 30s 刷一条，
                // 把 error.log 里别的错误顶掉——飞书那边踩过这个坑。
                if backoff == 2 {
                    wisecortex_core::buglog::record("clawbot", &format!("轮询失败：{e}"));
                }
                sleep_watching_toggle(Duration::from_secs(backoff), &cfg).await;
                backoff = (backoff * 2).min(30);
            }
        }
    }
}

/// 本次轮询用了 `elapsed`，下一次发出前还该等多久。见 [`MIN_POLL_INTERVAL`]。
fn poll_cooldown(elapsed: Duration) -> Duration {
    MIN_POLL_INTERVAL.saturating_sub(elapsed)
}

/// 配置是否已经变得和建连时的快照不一样（开关被关，或重新扫码换了 bot）。
fn config_changed(live: &ClawbotConfig) -> bool {
    let latest = clawbot::load();
    !latest.enabled || latest.bot_token != live.bot_token
}

/// 睡一段时间，但每 [`RECHECK`] 检查一次配置；配置一变就提前返回。
async fn sleep_watching_toggle(total: Duration, live: &ClawbotConfig) {
    let mut left = total;
    while !left.is_zero() {
        let step = left.min(RECHECK);
        tokio::time::sleep(step).await;
        left -= step;
        if config_changed(live) {
            return;
        }
    }
}

/// 发一次长轮询并把收到的消息分发出去。
///
/// 长轮询挂在 `spawn_blocking` 里（reqwest blocking，与本仓库其余出站请求一致），
/// 同时用 5 秒一拍的复查抢在它前面响应开关变化：否则关掉开关最多要等 35 秒才停。
/// 提前放弃某次轮询是安全的——游标没写回去，下次会重新取到同样的消息。
async fn poll_once(
    agent: &Arc<Mutex<Arc<Agent>>>,
    reg: &SessionRegistry,
    cfg: &ClawbotConfig,
) -> Result<Outcome, String> {
    let cursor = clawbot::load_cursor();
    let c = cfg.clone();
    let mut task = tokio::task::spawn_blocking(move || clawbot::get_updates(&c, &cursor));
    let mut recheck = tokio::time::interval(RECHECK);
    recheck.tick().await; // 跳过立即触发的首拍

    let fetched = loop {
        tokio::select! {
            r = &mut task => break r.map_err(|e| e.to_string())??,
            _ = recheck.tick() => {
                if config_changed(cfg) {
                    wisecortex_core::sprintln!("clawbot: 开关关闭或凭据已更换，放弃本次轮询。");
                    return Ok(Outcome::Stopped);
                }
            }
        }
    };

    let (msgs, new_cursor) = match fetched {
        Fetched::SessionExpired => return Ok(Outcome::SessionExpired),
        Fetched::Msgs { msgs, cursor } => (msgs, cursor),
    };

    // 先写游标再处理消息：反过来的话，处理途中进程被杀，重启后会把已经回答过的
    // 消息再答一遍。宁可漏一条，不可重复骚扰。
    if let Some(c) = new_cursor {
        clawbot::save_cursor(&c);
    }

    for m in msgs {
        // 先记下这条消息的 context_token，再谈其它。回复必须带一个有效的 token 才会
        // 被投递，而协议里**只有入站消息**会给我们 token——漏存一次，之后就只能干瞪眼。
        // 放在去重之前：重复消息带的 token 一样是新鲜的，没有理由丢掉。
        clawbot::save_context_token(&m.from_user_id, &m.context_token);
        // 入站消息没带 token 是要紧事：协议不给投递回执，这是我们唯一能留下的线索。
        // 只在「没带」时落盘（正常情况一条不写），免得把 error.log 里别的错误顶掉。
        if m.context_token.trim().is_empty() {
            let has_fallback = clawbot::load_context_token(&m.from_user_id).is_some();
            wisecortex_core::buglog::record(
                "clawbot",
                &format!(
                    "入站消息未携带 context_token（message_id={}）——{}",
                    m.message_id,
                    if has_fallback {
                        "回落到上次存下的那个"
                    } else {
                        "且没有可回落的，本条无法回复"
                    }
                ),
            );
        }
        if clawbot::seen_recently(&m.message_id) {
            wisecortex_core::sprintln!("clawbot: 重复消息（message_id={}），跳过", m.message_id);
            continue;
        }
        wisecortex_core::sprintln!(
            "clawbot: 收到消息（from={}, {} 字{}），处理中…",
            m.from_user_id,
            m.text.chars().count(),
            if m.unsupported.is_empty() {
                String::new()
            } else {
                format!("，含不支持的内容：{}", m.unsupported.join("、"))
            }
        );
        // agent 一轮可达分钟级：必须丢后台，否则 35 秒的长轮询窗口全废在这儿，
        // 后续消息全部积压。同会话的并发由 registry 的会话锁 + 消息队列串行。
        let agent = agent.clone();
        let reg = reg.clone();
        let cfg = cfg.clone();
        tokio::spawn(async move { handle_message(&agent, &reg, &cfg, m).await });
    }
    Ok(Outcome::Polled)
}

/// 「本版不支持某类内容」的说明。宁可明说也不要静默吞掉用户的消息。
fn unsupported_note(kinds: &[String]) -> String {
    format!(
        "（本版还不支持{}，已忽略。用文字或语音说给我听就行。）",
        kinds.join("、")
    )
}

async fn handle_message(
    agent: &Arc<Mutex<Arc<Agent>>>,
    reg: &SessionRegistry,
    cfg: &ClawbotConfig,
    m: Inbound,
) {
    let reply = if m.text.trim().is_empty() {
        // 只发了图片/文件之类：没有可跑的内容，直接把说明回过去，不惊动 agent。
        unsupported_note(&m.unsupported)
    } else {
        // 只有真要跑 agent 时才开「正在输入」：瞬时回复开了又关，白打两次接口。
        let typing = start_typing(cfg).await;
        let (text, _card) = crate::im::run_im_turn(
            agent,
            reg,
            ImOrigin::clawbot(&m.from_user_id),
            "微信",
            m.text.trim(),
        )
        .await;
        stop_typing(cfg, typing).await;
        // ClawBot 没有互动卡片，卡片版丢弃、用文本版（命令的文本回复一定存在）。
        if m.unsupported.is_empty() {
            text
        } else {
            format!("{}\n\n{}", text, unsupported_note(&m.unsupported))
        }
    };

    // 取 token 放在 agent 跑完之后：这一轮可能长达几分钟，期间若又来了新消息，
    // 存下来的会是更新的那个。
    let ctx = clawbot::resolve_context_token(&m.from_user_id, &m.context_token).unwrap_or_default();
    let from_this_msg = !m.context_token.trim().is_empty();
    let c = cfg.clone();
    let sent =
        tokio::task::spawn_blocking(move || clawbot::send_text(&c, &m.from_user_id, &ctx, &reply))
            .await;
    match sent {
        // 措辞刻意不写「已回复」：服务端只回执「受理」，送达与否协议不告诉我们。
        // 之前那句「已回复」正是让「微信收不到」这个 bug 藏了这么久的原因。
        Ok(Ok(())) => wisecortex_core::sprintln!(
            "clawbot: 回复已发出（context_token 取自{}）",
            if from_this_msg {
                "本条消息"
            } else {
                "上次存下的"
            }
        ),
        Ok(Err(e)) => report_send_failure(&format!("回复失败：{e}")),
        Err(e) => report_send_failure(&format!("回复任务异常：{e}")),
    }
}

/// 回复没发出去，必须**落盘**，不能只 `eprintln!`。
///
/// 桌面端是 GUI 进程，stderr 没有任何去处；服务端的 `error.log` 也只收 `buglog::record`。
/// 「微信收不到回复」这个问题之所以难查，正是因为唯一的线索被写在了没人看得见的地方——
/// 而 `sendmessage` 又对无效回复照回 200，于是整条链路从头到尾一片祥和。
fn report_send_failure(msg: &str) {
    wisecortex_core::seprintln!("clawbot: {msg}");
    wisecortex_core::buglog::record("clawbot", msg);
}

/// 开启「正在输入」并起一个续期任务，返回它的句柄。
///
/// agent 跑两分钟期间手机上完全没有反馈，用户只会以为机器人死了。全程 best-effort：
/// 拿不到 ticket 就不显示，绝不让它挡住回复。
///
/// 注意用的是配置里的 `ilink_user_id`（机器人主人），不是消息的 `from_user_id`
/// （`xxx@im.wechat`）——typing 系列接口认的是前者。
async fn start_typing(cfg: &ClawbotConfig) -> Option<(tokio::task::JoinHandle<()>, String)> {
    let user = cfg.user_id.clone().filter(|u| !u.is_empty())?;
    let (c, u) = (cfg.clone(), user.clone());
    let ticket = tokio::task::spawn_blocking(move || clawbot::typing_ticket(&c, &u))
        .await
        .ok()?
        .map_err(|e| {
            wisecortex_core::seprintln!("clawbot: 取 typing_ticket 失败（不影响回复）：{e}")
        })
        .ok()?;

    let (c, u, t) = (cfg.clone(), user, ticket.clone());
    let handle = tokio::spawn(async move {
        loop {
            let (c, u, t) = (c.clone(), u.clone(), t.clone());
            let _ = tokio::task::spawn_blocking(move || {
                clawbot::send_typing(&c, &u, &t, clawbot::TYPING_ON)
            })
            .await;
            tokio::time::sleep(TYPING_KEEPALIVE).await;
        }
    });
    Some((handle, ticket))
}

/// 停掉续期任务并显式取消「正在输入」。
async fn stop_typing(cfg: &ClawbotConfig, started: Option<(tokio::task::JoinHandle<()>, String)>) {
    let Some((handle, ticket)) = started else {
        return;
    };
    handle.abort();
    let Some(user) = cfg.user_id.clone() else {
        return;
    };
    let c = cfg.clone();
    let _ = tokio::task::spawn_blocking(move || {
        clawbot::send_typing(&c, &user, &ticket, clawbot::TYPING_OFF)
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(token: &str, enabled: bool) -> ClawbotConfig {
        ClawbotConfig {
            bot_token: Some(token.to_string()),
            enabled,
            ..Default::default()
        }
    }

    #[test]
    fn unsupported_note_lists_every_kind() {
        let n = unsupported_note(&["图片".to_string(), "文件".to_string()]);
        assert!(n.contains("图片"));
        assert!(n.contains("文件"));
        // 得给用户一条出路，而不是光说「不支持」。
        assert!(n.contains("语音") || n.contains("文字"));
    }

    #[tokio::test(start_paused = true)]
    async fn sleep_watching_toggle_returns_early_when_the_switch_flips() {
        // 用一个「文件里根本没有的 token」当快照：config_changed 立刻为真，
        // 于是长睡应当在第一拍（5s）之后就退出，而不是睡满 10 分钟。
        // 虚拟时钟下这一步是瞬时的，测量的是「睡了几个虚拟秒」。
        let live = cfg("token-that-is-not-on-disk", true);
        let started = tokio::time::Instant::now();
        sleep_watching_toggle(Duration::from_secs(600), &live).await;
        let slept = started.elapsed();
        assert!(
            slept <= RECHECK,
            "配置一变就该在一拍之内返回，实际睡了 {slept:?}"
        );
    }

    #[test]
    fn poll_cooldown_only_kicks_in_when_a_poll_returned_too_fast() {
        // 正常情况：服务端挂满 35 秒才回，立刻重发即可，不该额外等。
        assert_eq!(poll_cooldown(Duration::from_secs(35)), Duration::ZERO);
        assert_eq!(poll_cooldown(MIN_POLL_INTERVAL), Duration::ZERO);
        // 异常：立刻返回。补足到地板，避免变成不停打接口的死循环。
        assert_eq!(poll_cooldown(Duration::ZERO), MIN_POLL_INTERVAL);
        assert_eq!(
            poll_cooldown(Duration::from_millis(200)),
            Duration::from_millis(800)
        );
    }

    #[test]
    fn config_changed_detects_a_disabled_switch_or_a_new_bot() {
        // 磁盘上大概率没有 clawbot.json（或至少不是这个 token），两种情况都应判「变了」：
        // 换了 bot 之后还拿旧 token 轮询，新 bot 就一条事件也收不到。
        assert!(config_changed(&cfg("stale-token", true)));
    }
}
