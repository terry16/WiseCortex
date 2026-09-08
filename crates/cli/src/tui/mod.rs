//! `wisecortex tui`：终端瘦客户端。连本机 `wisecortex-server` 的 `/ws`，流式对话 / 切会话 / 订阅登录。
//!
//! 见 docs/plans/2026-07-15-tui-design.md。形态=轻量流式 REPL（模态：空闲编辑 vs 忙碌流式），
//! 不抢全屏。协议模型复用 [`wisecortex_core::proto`]。

mod command;
mod editor;
mod render;

use std::io::{stdout, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;
use futures_util::stream::{SplitSink, StreamExt};
use futures_util::SinkExt;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use command::Action;
use editor::LineEditor;
use wisecortex_core::proto::{ClientMsg, ServerEvent, Session};

type WsSink = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, WsMessage>;

/// `--serve` 拉起的 server 子进程守卫：TUI 退出（含出错/panic）时自动关掉它。
struct ServerGuard(Option<std::process::Child>);
impl Drop for ServerGuard {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
        }
    }
}

/// 同步入口：建单线程 runtime 跑异步 `run`。
pub fn run_blocking(
    url: Option<String>,
    access_key: Option<String>,
    session: Option<String>,
    serve: bool,
) -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("建 runtime 失败: {e}"))?;
    rt.block_on(run(url, access_key, session, serve))
}

// ── URL 处理（纯函数，便于单测）─────────────────────────────────────────────
/// 默认 WS 地址：由 `WC_BIND`（host:port，默认 127.0.0.1:7070）推出。
fn default_bind() -> String {
    std::env::var("WC_BIND")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "127.0.0.1:7070".to_string())
}

/// 规整成 `ws://…/ws`：补 scheme（http→ws / https→wss / 无→ws）、补 `/ws` 路径。
fn normalize_ws_url(input: &str) -> String {
    let s = input.trim();
    let (scheme, rest) = if let Some(r) = s.strip_prefix("wss://") {
        ("wss", r)
    } else if let Some(r) = s.strip_prefix("ws://") {
        ("ws", r)
    } else if let Some(r) = s.strip_prefix("https://") {
        ("wss", r)
    } else if let Some(r) = s.strip_prefix("http://") {
        ("ws", r)
    } else {
        ("ws", s)
    };
    // rest = host[:port][/path]
    let (host, path) = match rest.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (rest, String::new()),
    };
    let path = if path.is_empty() || path == "/" {
        "/ws".to_string()
    } else {
        path
    };
    format!("{scheme}://{host}{path}")
}

/// 给 URL 追加 `access_key` query（已有 query 用 `&`）。
fn append_access_key(url: &str, key: &str) -> String {
    if key.is_empty() {
        return url.to_string();
    }
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}access_key={key}")
}

/// 由 ws 地址推出 REST base：ws→http / wss→https，去掉路径与 query，只留 `scheme://host`。
fn http_base_from_ws(ws_url: &str) -> String {
    let (scheme, rest) = if let Some(r) = ws_url.strip_prefix("wss://") {
        ("https", r)
    } else if let Some(r) = ws_url.strip_prefix("ws://") {
        ("http", r)
    } else {
        ("http", ws_url)
    };
    let host = rest.split(['/', '?']).next().unwrap_or(rest);
    format!("{scheme}://{host}")
}

/// 从 ws 地址取出 `host:port`（供 TcpStream 探活 / 拉起 server 的 WC_BIND）。
fn host_port_from_ws(ws_url: &str) -> String {
    let base = http_base_from_ws(ws_url); // http(s)://host:port
    base.trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_string()
}

/// wisecortex-server 可执行路径（默认与 wisecortex 同目录）。
fn server_binary() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if cfg!(windows) {
        "wisecortex-server.exe"
    } else {
        "wisecortex-server"
    };
    let cand = dir.join(name);
    cand.exists().then_some(cand)
}

/// `--serve`：端口没在跑就拉起 server，返回子进程句柄（供退出时关掉）；已在跑则返回 None。
async fn ensure_server(host_port: &str) -> Result<Option<std::process::Child>, String> {
    if tokio::net::TcpStream::connect(host_port).await.is_ok() {
        return Ok(None); // 已有 server 在跑
    }
    let bin = server_binary().ok_or("找不到 wisecortex-server（应与 wisecortex 同目录）")?;
    println!("正在拉起 wisecortex-server（WC_BIND={host_port}）…");
    let _ = stdout().flush();
    let child = std::process::Command::new(bin)
        .env("WC_BIND", host_port)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("启动 wisecortex-server 失败：{e}"))?;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if tokio::net::TcpStream::connect(host_port).await.is_ok() {
            return Ok(Some(child));
        }
    }
    Err("wisecortex-server 启动后 10s 内仍未就绪".into())
}

fn new_session_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("tui-{nanos}")
}

// ── 终端绘制小工具（原始模式：换行须 \r\n）──────────────────────────────────
fn emit_line(text: &str) {
    print!("\r\x1b[2K{}\r\n", text.replace('\n', "\r\n"));
    let _ = stdout().flush();
}

fn emit_rendered(line: &render::Line) {
    emit_line(&render::paint(line));
}

fn redraw_prompt(ed: &LineEditor) {
    print!("\r\x1b[2K\x1b[1m你 ›\x1b[0m {}", ed.buffer());
    let _ = stdout().flush();
}

// ── 主流程 ──────────────────────────────────────────────────────────────────
async fn send(write: &mut WsSink, msg: &ClientMsg) -> Result<(), String> {
    let txt = serde_json::to_string(msg).map_err(|e| e.to_string())?;
    write
        .send(WsMessage::Text(txt))
        .await
        .map_err(|e| format!("发送失败: {e}"))
}

struct Ui {
    session_id: String,
    sessions: Vec<Session>,
    cost_today: f64,
    busy: bool,
    /// 助手流式行是否已起头（打 `它 · ` 前缀）。
    streaming: bool,
    /// 当前是否停在未换行的流式行中（下条转录行前需补 \r\n）。
    mid_line: bool,
    /// 待处理的阻塞式确认。
    awaiting_confirm: Option<(String, bool)>, // (id, default)
    /// 待粘贴 code 的订阅登录：(provider, verifier, state)。
    awaiting_login: Option<(String, String, String)>,
    editor: LineEditor,
    http: reqwest::Client,
    http_base: String,
    access_key: String,
}

async fn run(
    url: Option<String>,
    access_key: Option<String>,
    session: Option<String>,
    serve: bool,
) -> Result<(), String> {
    let raw = url.unwrap_or_else(|| format!("ws://{}/ws", default_bind()));
    let ws_url_clean = normalize_ws_url(&raw);
    let key = access_key
        .or_else(|| std::env::var("WC_ACCESS_KEY").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    let ws_url = append_access_key(&ws_url_clean, &key);
    let http_base = http_base_from_ws(&ws_url_clean);

    // --serve：端口没在跑就拉起 server（退出时关掉）。
    let mut server_child = if serve {
        ensure_server(&host_port_from_ws(&ws_url_clean)).await?
    } else {
        None
    };

    println!("WiseCortex TUI · 连接 {ws_url_clean} …");
    let _ = stdout().flush(); // 非 tty 下也立刻可见（stdout 被重定向时默认块缓冲）
    let connected = connect_async(&ws_url).await;
    let (stream, _) = match connected {
        Ok(s) => s,
        Err(e) => {
            if let Some(mut c) = server_child.take() {
                let _ = c.kill();
            }
            return Err(format!(
                "连不上 {ws_url_clean}：{e}\n先启动 wisecortex-server 了吗？（默认 127.0.0.1:7070，或加 --serve 自动拉起）"
            ));
        }
    };
    let (mut write, mut read) = stream.split();
    // 从此刻起任何返回路径（含 panic）都会经 Drop 关掉自动拉起的 server。
    let _server_guard = ServerGuard(server_child.take());

    // 选会话：拉列表，选 --session / 最近活跃 / 新建。
    send(&mut write, &ClientMsg::ListSessions).await?;
    let mut sessions: Vec<Session> = Vec::new();
    let mut cost_today = 0.0;
    let deadline = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            msg = read.next() => match msg {
                Some(Ok(WsMessage::Text(t))) => {
                    if let Ok(ev) = serde_json::from_str::<ServerEvent>(t.as_str()) {
                        match ev {
                            ServerEvent::SessionList { sessions: s, .. } => { sessions = s; break; }
                            ServerEvent::CostUpdate { cost_today: c } => cost_today = c,
                            _ => {}
                        }
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(format!("连接错误: {e}")),
                None => return Err("连接已关闭".into()),
            }
        }
    }
    let session_id = session
        .filter(|s| !s.is_empty())
        .or_else(|| sessions.first().map(|s| s.id.clone()))
        .unwrap_or_else(new_session_id);
    send(
        &mut write,
        &ClientMsg::Subscribe {
            session_id: session_id.clone(),
        },
    )
    .await?;

    let http = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let mut ui = Ui {
        session_id,
        sessions,
        cost_today,
        busy: false,
        streaming: false,
        mid_line: false,
        awaiting_confirm: None,
        awaiting_login: None,
        editor: LineEditor::new(),
        http,
        http_base,
        access_key: key,
    };

    // 进原始模式。
    terminal::enable_raw_mode().map_err(|e| format!("进原始模式失败: {e}"))?;
    let result = event_loop(&mut write, &mut read, &mut ui).await;
    let _ = terminal::disable_raw_mode();
    print!("\r\n");
    let _ = stdout().flush();
    result
}

async fn event_loop(
    write: &mut WsSink,
    read: &mut futures_util::stream::SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    ui: &mut Ui,
) -> Result<(), String> {
    emit_line(&format!(
        "已连接 · 会话 {} · 今日 ¥{:.4}",
        short(&ui.session_id),
        ui.cost_today
    ));
    emit_line("输入消息，/help 看命令，/quit 退出");
    redraw_prompt(&ui.editor);

    let mut keys = EventStream::new();
    loop {
        tokio::select! {
            ws = read.next() => match ws {
                Some(Ok(WsMessage::Text(t))) => {
                    if let Ok(ev) = serde_json::from_str::<ServerEvent>(t.as_str()) {
                        on_server_event(ui, ev).await?;
                    }
                }
                Some(Ok(WsMessage::Close(_))) | None => {
                    emit_line("（服务端已断开）");
                    break;
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => { emit_line(&format!("（连接错误：{e}）")); break; }
            },
            key = keys.next() => match key {
                Some(Ok(Event::Key(k))) => {
                    if on_key(write, ui, k).await? { break; }
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => { emit_line(&format!("（读键错误：{e}）")); break; }
                None => break,
            }
        }
    }
    Ok(())
}

fn short(id: &str) -> String {
    if id.chars().count() > 12 {
        format!("{}…", id.chars().take(12).collect::<String>())
    } else {
        id.to_string()
    }
}

/// 处理一条服务端事件。返回 Err 仅用于致命错误。
async fn on_server_event(ui: &mut Ui, ev: ServerEvent) -> Result<(), String> {
    match &ev {
        ServerEvent::AssistantDelta { delta, .. } => {
            if !ui.streaming {
                ensure_newline(ui);
                print!("\x1b[1m它 ·\x1b[0m ");
                ui.streaming = true;
            }
            print!("{}", delta.replace('\n', "\r\n"));
            ui.mid_line = !delta.ends_with('\n');
            let _ = stdout().flush();
        }
        ServerEvent::AssistantThinking { .. } => { /* 略：v1 不展开思考流 */ }
        ServerEvent::CostUpdate { cost_today } => ui.cost_today = *cost_today,
        ServerEvent::SessionList { sessions, .. } => {
            ui.sessions = sessions.clone();
            print_session_list(ui);
            redraw_prompt(&ui.editor);
        }
        ServerEvent::RequestConfirmation {
            id,
            message,
            default,
            ..
        } => {
            ensure_newline(ui);
            let hint = if *default { "[Y/n]" } else { "[y/N]" };
            emit_line(&format!("\x1b[33m⚠ {message} {hint}\x1b[0m"));
            ui.awaiting_confirm = Some((id.clone(), *default));
        }
        ServerEvent::Complete { .. } => {
            ensure_newline(ui);
            for l in render::render(&ev) {
                emit_rendered(&l);
            }
            end_turn(ui);
        }
        ServerEvent::Interrupted { .. } | ServerEvent::Error { .. } => {
            ensure_newline(ui);
            for l in render::render(&ev) {
                emit_rendered(&l);
            }
            end_turn(ui);
        }
        ServerEvent::Subscribed { .. } | ServerEvent::Pong | ServerEvent::SessionUpdate { .. } => {}
        _ => {
            let lines = render::render(&ev);
            if !lines.is_empty() {
                ensure_newline(ui);
                for l in lines {
                    emit_rendered(&l);
                }
            }
        }
    }
    Ok(())
}

/// 若停在未换行的流式行中，补一个换行，让后续转录行另起。
fn ensure_newline(ui: &mut Ui) {
    if ui.mid_line {
        print!("\r\n");
        let _ = stdout().flush();
        ui.mid_line = false;
    }
}

fn end_turn(ui: &mut Ui) {
    ui.busy = false;
    ui.streaming = false;
    ui.mid_line = false;
    redraw_prompt(&ui.editor);
}

fn print_session_list(ui: &Ui) {
    emit_line("会话列表：");
    for (i, s) in ui.sessions.iter().enumerate() {
        let name = s.name.clone().unwrap_or_else(|| short(&s.id));
        let mark = if s.id == ui.session_id { "▶" } else { " " };
        let cost = s.total_cost.unwrap_or(0.0);
        emit_line(&format!("{mark} {}. {name}  ¥{cost:.4}", i + 1));
    }
    emit_line("用 /switch <序号|id> 切换。");
}

/// 处理一次按键。返回 Ok(true) 表示要退出。
async fn on_key(write: &mut WsSink, ui: &mut Ui, k: KeyEvent) -> Result<bool, String> {
    // 阻塞式确认：优先把按键当 y/n 答复。
    if let Some((id, default)) = ui.awaiting_confirm.clone() {
        let ans = match k.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => Some(true),
            KeyCode::Char('n') | KeyCode::Char('N') => Some(false),
            KeyCode::Enter => Some(default),
            KeyCode::Esc => Some(false),
            _ => None,
        };
        if let Some(v) = ans {
            ui.awaiting_confirm = None;
            emit_line(if v {
                "  → 已确认"
            } else {
                "  → 已拒绝"
            });
            send(
                write,
                &ClientMsg::Confirmation {
                    session_id: Some(ui.session_id.clone()),
                    id,
                    result: v.to_string(),
                },
            )
            .await?;
        }
        return Ok(false);
    }

    // Ctrl+C / Ctrl+D。
    if k.modifiers.contains(KeyModifiers::CONTROL) {
        match k.code {
            KeyCode::Char('c') => {
                if ui.busy {
                    send(
                        write,
                        &ClientMsg::Interrupt {
                            session_id: Some(ui.session_id.clone()),
                        },
                    )
                    .await?;
                    return Ok(false);
                }
                if ui.editor.is_empty() {
                    return Ok(true); // 空行再按 Ctrl+C → 退出
                }
                ui.editor.clear();
                redraw_prompt(&ui.editor);
                return Ok(false);
            }
            KeyCode::Char('d') => return Ok(true),
            _ => return Ok(false),
        }
    }

    // 忙碌中：只认 Esc（打断）。
    if ui.busy {
        if k.code == KeyCode::Esc {
            send(
                write,
                &ClientMsg::Interrupt {
                    session_id: Some(ui.session_id.clone()),
                },
            )
            .await?;
        }
        return Ok(false);
    }

    // 空闲：行编辑。
    match k.code {
        KeyCode::Char(c) => {
            ui.editor.insert(c);
            redraw_prompt(&ui.editor);
        }
        KeyCode::Backspace => {
            ui.editor.backspace();
            redraw_prompt(&ui.editor);
        }
        KeyCode::Up => {
            ui.editor.history_prev();
            redraw_prompt(&ui.editor);
        }
        KeyCode::Down => {
            ui.editor.history_next();
            redraw_prompt(&ui.editor);
        }
        KeyCode::Enter => {
            let line = ui.editor.take();
            // 把输入定稿成一行转录。
            print!("\r\x1b[2K\x1b[1m你 ›\x1b[0m {}\r\n", line);
            let _ = stdout().flush();
            return dispatch(write, ui, &line).await;
        }
        _ => {}
    }
    Ok(false)
}

/// 分发一行输入。返回 Ok(true) 表示退出。
async fn dispatch(write: &mut WsSink, ui: &mut Ui, line: &str) -> Result<bool, String> {
    // 正在等粘贴登录 code？
    if let Some((provider, verifier, state)) = ui.awaiting_login.clone() {
        ui.awaiting_login = None;
        if line.trim().is_empty() || line.trim() == "/cancel" {
            emit_line("  已取消登录。");
        } else {
            finish_login(ui, &provider, line.trim(), &verifier, &state).await;
        }
        redraw_prompt(&ui.editor);
        return Ok(false);
    }

    // `!cmd`：本地 shell。
    if let Some(cmd) = line.strip_prefix('!') {
        run_local_shell(cmd.trim()).await;
        redraw_prompt(&ui.editor);
        return Ok(false);
    }

    match command::parse(line) {
        Action::Empty => redraw_prompt(&ui.editor),
        Action::Message(text) => {
            ui.busy = true;
            ui.streaming = false;
            send(
                write,
                &ClientMsg::Message {
                    session_id: Some(ui.session_id.clone()),
                    content: text,
                    files: Vec::new(),
                    images: Vec::new(),
                    cwd: None,
                },
            )
            .await?;
        }
        Action::Help => {
            emit_line(command::help_text());
            redraw_prompt(&ui.editor);
        }
        Action::Status => {
            emit_line(&format!(
                "会话 {} · 今日 ¥{:.4} · 服务端 {}",
                short(&ui.session_id),
                ui.cost_today,
                ui.http_base
            ));
            redraw_prompt(&ui.editor);
        }
        Action::Sessions => {
            send(write, &ClientMsg::ListSessions).await?;
        }
        Action::Switch(arg) => {
            switch_session(write, ui, &arg).await?;
            redraw_prompt(&ui.editor);
        }
        Action::New => {
            ui.session_id = new_session_id();
            send(
                write,
                &ClientMsg::Subscribe {
                    session_id: ui.session_id.clone(),
                },
            )
            .await?;
            emit_line(&format!("  新会话 {}", short(&ui.session_id)));
            redraw_prompt(&ui.editor);
        }
        Action::Model(arg) => {
            model_command(ui, &arg).await;
            redraw_prompt(&ui.editor);
        }
        Action::Login(provider) => {
            start_login(ui, &provider).await;
            redraw_prompt(&ui.editor);
        }
        Action::Retry => {
            ui.busy = true;
            ui.streaming = false;
            send(
                write,
                &ClientMsg::Retry {
                    session_id: Some(ui.session_id.clone()),
                },
            )
            .await?;
        }
        Action::Interrupt => {
            send(
                write,
                &ClientMsg::Interrupt {
                    session_id: Some(ui.session_id.clone()),
                },
            )
            .await?;
            redraw_prompt(&ui.editor);
        }
        Action::Quit => return Ok(true),
        Action::Unknown(name) => {
            emit_line(&format!("  未知命令 /{name}，/help 看可用命令。"));
            redraw_prompt(&ui.editor);
        }
    }
    Ok(false)
}

async fn switch_session(write: &mut WsSink, ui: &mut Ui, arg: &str) -> Result<(), String> {
    if arg.is_empty() {
        send(write, &ClientMsg::ListSessions).await?;
        return Ok(());
    }
    let target = if let Ok(n) = arg.parse::<usize>() {
        ui.sessions.get(n.wrapping_sub(1)).map(|s| s.id.clone())
    } else {
        Some(arg.to_string())
    };
    match target {
        Some(id) => {
            ui.session_id = id.clone();
            send(
                write,
                &ClientMsg::Subscribe {
                    session_id: id.clone(),
                },
            )
            .await?;
            emit_line(&format!("  已切到会话 {}", short(&id)));
        }
        None => emit_line("  序号超出范围，先 /sessions 看列表。"),
    }
    Ok(())
}

// ── REST：模型 / 登录 ────────────────────────────────────────────────────────
fn rest_get(ui: &Ui, path: &str) -> reqwest::RequestBuilder {
    let mut b = ui.http.get(format!("{}{path}", ui.http_base));
    if !ui.access_key.is_empty() {
        b = b.header("x-access-key", &ui.access_key);
    }
    b
}

fn rest_post(ui: &Ui, path: &str, body: serde_json::Value) -> reqwest::RequestBuilder {
    let mut b = ui.http.post(format!("{}{path}", ui.http_base)).json(&body);
    if !ui.access_key.is_empty() {
        b = b.header("x-access-key", &ui.access_key);
    }
    b
}

/// `/model`：无参→列出已配置的模型档（标记当前）；有参→按序号/id/名切换 active_llm 并热重载。
async fn model_command(ui: &Ui, arg: &str) {
    let cfg = match rest_get(ui, "/api/config").await_json().await {
        Ok(v) => v,
        Err(e) => return emit_line(&format!("  拉配置失败：{e}")),
    };
    let llms = cfg
        .get("llms")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let active = cfg.get("active_llm").and_then(|x| x.as_str()).unwrap_or("");
    if llms.is_empty() {
        return emit_line("  还没配置模型档。先在 webUI 或 `wisecortex config` 里加一个。");
    }
    let field = |p: &serde_json::Value, k: &str| {
        p.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
    };
    if arg.is_empty() {
        emit_line("模型档（/model <序号|id> 切换）：");
        for (i, p) in llms.iter().enumerate() {
            let id = field(p, "id");
            let name = {
                let n = field(p, "name");
                if n.is_empty() {
                    id.clone()
                } else {
                    n
                }
            };
            let mark = if id == active { "▶" } else { " " };
            emit_line(&format!(
                "{mark} {}. {name}  ({} · {})",
                i + 1,
                field(p, "provider"),
                field(p, "model")
            ));
        }
        return;
    }
    // 解析目标档 id：序号 / id / 名。
    let target = if let Ok(n) = arg.parse::<usize>() {
        llms.get(n.wrapping_sub(1)).map(|p| field(p, "id"))
    } else {
        llms.iter()
            .find(|p| field(p, "id") == arg || field(p, "name") == arg)
            .map(|p| field(p, "id"))
    };
    let Some(id) = target.filter(|s| !s.is_empty()) else {
        return emit_line("  没找到该档，先 /model 看列表。");
    };
    match rest_post(ui, "/api/config", serde_json::json!({ "active_llm": id }))
        .await_json()
        .await
    {
        Ok(v) => {
            if let Some(err) = v
                .get("error")
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
            {
                emit_line(&format!("  ✗ 切换失败：{err}"));
            } else {
                let model = llms
                    .iter()
                    .find(|p| field(p, "id") == id)
                    .map(|p| field(p, "model"))
                    .unwrap_or_default();
                emit_line(&format!("  ✓ 已切到 {id}（{model}）·下条消息生效"));
            }
        }
        Err(e) => emit_line(&format!("  ✗ 切换失败：{e}")),
    }
}

/// `!cmd`：本地执行一条 shell 命令，回显前若干行输出（服务器上省得另开终端）。
async fn run_local_shell(cmd: &str) {
    if cmd.is_empty() {
        return;
    }
    let out = if cfg!(windows) {
        tokio::process::Command::new("cmd")
            .arg("/C")
            .arg(cmd)
            .output()
            .await
    } else {
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .output()
            .await
    };
    match out {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            let err = String::from_utf8_lossy(&o.stderr);
            let mut n = 0;
            for line in text.lines().chain(err.lines()) {
                if n >= 40 {
                    emit_line("  …（输出已截断）");
                    break;
                }
                emit_line(&format!("  {line}"));
                n += 1;
            }
            if n == 0 {
                emit_line(&format!("  （无输出，退出码 {:?}）", o.status.code()));
            }
        }
        Err(e) => emit_line(&format!("  shell 失败：{e}")),
    }
}

/// 订阅档规格：(档 id, 展示名, provider 预设 id, 默认模型, 订阅开关字段)。
fn sub_spec(
    provider: &str,
) -> Option<(
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
)> {
    match provider {
        "claude" => Some((
            "sub-claude",
            "Claude 订阅",
            "anthropic",
            "claude-sonnet-4-6",
            "claude_oauth",
        )),
        "openai" => Some((
            "sub-codex",
            "ChatGPT 订阅",
            "openai",
            "gpt-5-codex",
            "openai_codex",
        )),
        "xai" | "grok" => Some((
            "sub-grok",
            "Grok 订阅",
            "xai",
            "grok-code-fast-1",
            "xai_grok",
        )),
        "gemini" => Some((
            "sub-gemini",
            "Gemini 订阅",
            "gemini",
            "gemini-2.5-pro",
            "gemini_oauth",
        )),
        _ => None,
    }
}

/// 登录成功后：建好（幂等，固定 id）该订阅的模型档 + 设为当前档 + 热重载，一步到位可用。
async fn enable_subscription_profile(ui: &Ui, provider: &str) {
    let Some((id, name, prov, model, flag)) = sub_spec(provider) else {
        return;
    };
    let mut profile =
        serde_json::json!({ "id": id, "name": name, "provider": prov, "model": model });
    profile[flag] = serde_json::json!(true);
    if let Err(e) = rest_post(ui, "/api/config/llm", profile).await_json().await {
        return emit_line(&format!(
            "  （自动建档失败：{e}；可在 webUI 手动建「{name}」）"
        ));
    }
    match rest_post(ui, "/api/config", serde_json::json!({ "active_llm": id }))
        .await_json()
        .await
    {
        Ok(v) => {
            if let Some(err) = v
                .get("error")
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
            {
                emit_line(&format!("  （启用失败：{err}）"));
            } else {
                emit_line(&format!("  已启用「{name}」（{model}）· 直接发消息即可"));
            }
        }
        Err(e) => emit_line(&format!("  （启用失败：{e}）")),
    }
}

async fn start_login(ui: &mut Ui, provider: &str) {
    match provider {
        "claude" | "openai" | "gemini" => {
            let path = format!("/api/oauth/{provider}/start");
            match rest_post(ui, &path, serde_json::json!({}))
                .await_json()
                .await
            {
                Ok(v) => {
                    let url = v.get("url").and_then(|x| x.as_str()).unwrap_or("");
                    let verifier = v
                        .get("verifier")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let state = v
                        .get("state")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    emit_line(&format!(
                        "  在浏览器打开授权，登录后把 code 粘回来回车（/cancel 取消）：\n  {url}"
                    ));
                    ui.awaiting_login = Some((provider.to_string(), verifier, state));
                }
                Err(e) => emit_line(&format!("  登录发起失败：{e}")),
            }
        }
        "xai" | "grok" => {
            xai_login(ui).await;
        }
        other => emit_line(&format!(
            "  未知订阅：{other}。支持 claude | openai | gemini | xai。"
        )),
    }
}

async fn finish_login(ui: &Ui, provider: &str, code: &str, verifier: &str, state: &str) {
    let path = format!("/api/oauth/{provider}/finish");
    let body = serde_json::json!({ "code": code, "verifier": verifier, "state": state });
    match rest_post(ui, &path, body).await_json().await {
        Ok(v) => {
            if v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false) {
                let email = v.get("email").and_then(|x| x.as_str()).unwrap_or("");
                emit_line(&format!("  ✓ {provider} 登录成功 {email}"));
                enable_subscription_profile(ui, provider).await;
            } else {
                let err = v
                    .get("error")
                    .and_then(|x| x.as_str())
                    .unwrap_or("未知错误");
                emit_line(&format!("  ✗ 登录失败：{err}"));
            }
        }
        Err(e) => emit_line(&format!("  ✗ 兑换失败：{e}")),
    }
}

async fn xai_login(ui: &Ui) {
    let start = match rest_post(ui, "/api/oauth/xai/start", serde_json::json!({}))
        .await_json()
        .await
    {
        Ok(v) => v,
        Err(e) => return emit_line(&format!("  Grok 登录发起失败：{e}")),
    };
    let device_code = start
        .get("device_code")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let user_code = start
        .get("user_code")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let uri = start
        .get("verification_uri_complete")
        .and_then(|x| x.as_str())
        .or_else(|| start.get("verification_uri").and_then(|x| x.as_str()))
        .unwrap_or("");
    let interval = start
        .get("interval")
        .and_then(|x| x.as_u64())
        .unwrap_or(5)
        .max(1);
    if device_code.is_empty() {
        return emit_line("  Grok 登录：未拿到 device_code。");
    }
    emit_line(&format!("  在浏览器打开 {uri}\n  输入代码 {user_code}，完成后本处自动登录（约 {interval}s 轮询一次）"));
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let poll = rest_post(
            ui,
            "/api/oauth/xai/poll",
            serde_json::json!({ "device_code": device_code }),
        )
        .await_json()
        .await;
        match poll {
            Ok(v) => {
                if v.get("logged_in")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false)
                {
                    emit_line("  ✓ Grok 登录成功");
                    enable_subscription_profile(ui, "xai").await;
                    return;
                }
                if let Some(err) = v.get("error").and_then(|x| x.as_str()) {
                    return emit_line(&format!("  ✗ Grok 登录失败：{err}"));
                }
                // pending / slow_down → 继续等
            }
            Err(e) => return emit_line(&format!("  ✗ Grok 轮询失败：{e}")),
        }
    }
    emit_line("  Grok 登录超时，请重试 /login xai");
}

/// 小扩展：RequestBuilder → 发送并解析 JSON。
trait AwaitJson {
    async fn await_json(self) -> Result<serde_json::Value, String>;
}
impl AwaitJson for reqwest::RequestBuilder {
    async fn await_json(self) -> Result<serde_json::Value, String> {
        let resp = self.send().await.map_err(|e| e.to_string())?;
        resp.json::<serde_json::Value>()
            .await
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_ws_url_adds_scheme_and_ws_path() {
        assert_eq!(normalize_ws_url("127.0.0.1:7070"), "ws://127.0.0.1:7070/ws");
        assert_eq!(
            normalize_ws_url("ws://127.0.0.1:7070"),
            "ws://127.0.0.1:7070/ws"
        );
        assert_eq!(
            normalize_ws_url("ws://127.0.0.1:7070/ws"),
            "ws://127.0.0.1:7070/ws"
        );
        // http/https → ws/wss
        assert_eq!(normalize_ws_url("http://h:1/ws"), "ws://h:1/ws");
        assert_eq!(normalize_ws_url("https://h:1"), "wss://h:1/ws");
        // 自定义路径保留
        assert_eq!(normalize_ws_url("ws://h:1/custom"), "ws://h:1/custom");
    }

    #[test]
    fn append_access_key_uses_right_separator() {
        assert_eq!(
            append_access_key("ws://h/ws", "k"),
            "ws://h/ws?access_key=k"
        );
        assert_eq!(
            append_access_key("ws://h/ws?x=1", "k"),
            "ws://h/ws?x=1&access_key=k"
        );
        assert_eq!(append_access_key("ws://h/ws", ""), "ws://h/ws"); // 空 key 不加
    }

    #[test]
    fn http_base_derives_from_ws() {
        assert_eq!(
            http_base_from_ws("ws://127.0.0.1:7070/ws"),
            "http://127.0.0.1:7070"
        );
        assert_eq!(
            http_base_from_ws("wss://h:8/ws?access_key=k"),
            "https://h:8"
        );
    }

    #[test]
    fn short_truncates_long_ids() {
        assert_eq!(short("abc"), "abc");
        assert_eq!(short("0123456789abcdef"), "0123456789ab…");
    }
}
