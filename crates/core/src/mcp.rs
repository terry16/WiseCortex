//! MCP（Model Context Protocol）客户端：连接配置的 MCP 服务器，把它们的能力暴露给 agent。
//!
//! 传输：**stdio**（本地子进程，JSON-RPC over stdin/stdout，换行分隔）与
//! **Streamable HTTP**（`url` 有值时；单端点 POST，响应可为 JSON 或 SSE）。
//! 握手 `initialize`(广告 roots+sampling 客户端能力) → `notifications/initialized` →
//! 按服务端能力发现 tools / resources / prompts。
//!
//! 能力消费：
//! - **tools**：`tools/list` 直接暴露为 `mcp__<srv>__<工具>`。
//! - **resources / prompts**：合成工具 `list_resources`/`read_resource`/`list_prompts`/`get_prompt`，
//!   调用时映射到 `resources/*`、`prompts/*`。
//! - **sampling / roots**（仅 stdio 反向请求）：读循环处理服务端→客户端请求；`roots/list` 直接答，
//!   `sampling/createMessage` 转交注册的 [`set_sampling_handler`]（由 server 用 LLM 客户端实现）。
//!
//! ⚠️ Streamable HTTP 暂不支持服务端→客户端反向通道（需另开 GET SSE）；sampling/roots 仅 stdio 生效。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};

use crate::config::McpServerConfig;

const PROTOCOL_VERSION: &str = "2025-03-26";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// 合成工具映射到哪种 MCP 调用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpToolKind {
    /// 真实 `tools/call`。
    Tool,
    /// `resources/list`。
    ListResources,
    /// `resources/read`。
    ReadResource,
    /// `prompts/list`。
    ListPrompts,
    /// `prompts/get`。
    GetPrompt,
}

/// 一个暴露给 agent 的 MCP 工具（name/description 泄漏为 'static 以适配 Tool trait；只在发现时泄漏一次）。
#[derive(Clone)]
pub struct McpToolInfo {
    pub server: String,
    /// 对 `Tool` 是真实工具名；对合成工具是底层名（如 `read_resource`）。
    pub tool: String,
    pub kind: McpToolKind,
    pub full_name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

// ── 客户端能力 / roots / sampling handler ─────────────────────────────────────

/// 我们作为客户端广告的能力。
fn client_capabilities() -> Value {
    json!({ "roots": { "listChanged": true }, "sampling": {} })
}

/// 响应 `roots/list` 的 roots（配置 `mcp_roots`，空则用当前工作目录）。
fn client_roots() -> Vec<Value> {
    let cfg = crate::config::load();
    let roots = if cfg.mcp_roots.is_empty() {
        std::env::current_dir()
            .ok()
            .map(|p| vec![p.to_string_lossy().to_string()])
            .unwrap_or_default()
    } else {
        cfg.mcp_roots.clone()
    };
    roots
        .into_iter()
        .map(|r| {
            let uri = if r.contains("://") {
                r.clone()
            } else {
                format!("file:///{}", r.trim_start_matches('/').replace('\\', "/"))
            };
            json!({ "uri": uri, "name": r })
        })
        .collect()
}

/// sampling 处理器：把 MCP `sampling/createMessage` 的 params 跑成 LLM 补全，返回 MCP result。
/// 由 server 在启动时用其 LLM 客户端注册（core 不直接依赖 LLM 运行时配置）。
pub type SamplingHandler = Arc<dyn Fn(&Value) -> Result<Value, String> + Send + Sync>;

fn sampling_slot() -> &'static Mutex<Option<SamplingHandler>> {
    static S: OnceLock<Mutex<Option<SamplingHandler>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

/// 注册 sampling 处理器（server 启动时调用一次）。
pub fn set_sampling_handler(h: SamplingHandler) {
    *sampling_slot().lock().unwrap() = Some(h);
}

fn sampling_handler() -> Option<SamplingHandler> {
    sampling_slot().lock().unwrap().clone()
}

/// 处理服务端→客户端请求。返回 Ok(result) 或 Err((code, message))（JSON-RPC error code）。
fn handle_server_request(method: &str, params: &Value) -> Result<Value, (i64, String)> {
    match method {
        "ping" => Ok(json!({})),
        "roots/list" => Ok(json!({ "roots": client_roots() })),
        "sampling/createMessage" => match sampling_handler() {
            Some(h) => h(params).map_err(|e| (-32603, e)),
            None => Err((-32601, "sampling 未启用".to_string())),
        },
        other => Err((-32601, format!("不支持的方法: {other}"))),
    }
}

// ── 传输抽象 ──────────────────────────────────────────────────────────────────

trait Transport: Send + Sync {
    fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String>;
    fn notify(&self, method: &str, params: Value);
}

/// 从 JSON-RPC 响应里取 result（有 error 则转 Err）。
fn result_of(v: &Value) -> Result<Value, String> {
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        return Err(format!("MCP 错误: {msg}"));
    }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

// ── stdio 传输 ────────────────────────────────────────────────────────────────

struct StdioConn {
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Arc<Mutex<HashMap<i64, mpsc::Sender<Value>>>>,
    next_id: AtomicI64,
    #[allow(dead_code)]
    child: Mutex<Child>,
}

impl StdioConn {
    fn initialize(&self) -> Result<Value, String> {
        let r = self.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": client_capabilities(),
                "clientInfo": { "name": "wisecortex", "version": env!("CARGO_PKG_VERSION") }
            }),
            CONNECT_TIMEOUT,
        )?;
        self.notify("notifications/initialized", json!({}));
        Ok(r.get("capabilities").cloned().unwrap_or_else(|| json!({})))
    }
}

impl Transport for StdioConn {
    fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel::<Value>();
        self.pending.lock().unwrap().insert(id, tx);
        let line = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string();
        {
            let mut w = self.stdin.lock().unwrap();
            writeln!(w, "{line}").map_err(|e| format!("写入 MCP 请求失败: {e}"))?;
            w.flush().map_err(|e| format!("flush 失败: {e}"))?;
        }
        let resp = match rx.recv_timeout(timeout) {
            Ok(v) => v,
            Err(RecvTimeoutError::Timeout) => {
                self.pending.lock().unwrap().remove(&id);
                return Err(format!("MCP {method} 超时"));
            }
            Err(RecvTimeoutError::Disconnected) => return Err("MCP 连接已断开".into()),
        };
        result_of(&resp)
    }

    fn notify(&self, method: &str, params: Value) {
        let line = json!({"jsonrpc":"2.0","method":method,"params":params}).to_string();
        if let Ok(mut w) = self.stdin.lock() {
            let _ = writeln!(w, "{line}");
            let _ = w.flush();
        }
    }
}

/// stdio 读循环：路由响应；处理服务端→客户端请求（写回 stdin）；忽略通知。
fn stdio_read_loop(
    stdout: std::process::ChildStdout,
    pending: Arc<Mutex<HashMap<i64, mpsc::Sender<Value>>>>,
    stdin: Arc<Mutex<ChildStdin>>,
) {
    let mut buf = BufReader::new(stdout);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match buf.read_until(b'\n', &mut bytes) {
            Ok(0) => break,
            Ok(_) => {
                let text = String::from_utf8_lossy(&bytes);
                let line = text.trim();
                if line.is_empty() {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                let has_method = v.get("method").and_then(Value::as_str);
                match (v.get("id").cloned(), has_method) {
                    // 服务端→客户端请求（有 id + method）：处理并回写。
                    (Some(id), Some(method)) if !id.is_null() => {
                        let params = v.get("params").cloned().unwrap_or(Value::Null);
                        let resp = match handle_server_request(method, &params) {
                            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
                            Err((code, msg)) => {
                                json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":msg}})
                            }
                        };
                        if let Ok(mut w) = stdin.lock() {
                            let _ = writeln!(w, "{resp}");
                            let _ = w.flush();
                        }
                    }
                    // 响应（有 id 无 method）：按 id 路由。
                    (Some(id), None) => {
                        if let Some(idn) = id.as_i64() {
                            if v.get("result").is_some() || v.get("error").is_some() {
                                if let Some(tx) = pending.lock().unwrap().remove(&idn) {
                                    let _ = tx.send(v);
                                }
                            }
                        }
                    }
                    // 通知（无 id）：忽略。
                    _ => {}
                }
            }
            Err(_) => break,
        }
    }
}

/// 连接一个 stdio MCP 服务器并完成握手，返回 (传输, 服务端能力)。
fn connect_stdio(name: &str, cfg: &McpServerConfig) -> Result<(Arc<dyn Transport>, Value), String> {
    if cfg.command.trim().is_empty() {
        return Err("command 为空".into());
    }
    let mut cmd = Command::new(&cfg.command);
    cmd.args(&cfg.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(env) = &cfg.env {
        for (k, v) in env {
            cmd.env(k, v);
        }
    }
    for (k, v) in crate::net::proxy_env() {
        cmd.env(k, v);
    }
    crate::proc::no_window(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 MCP 服务器 {name} 失败: {e}"))?;

    let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("无 stdin")?));
    let stdout = child.stdout.take().ok_or("无 stdout")?;
    let pending: Arc<Mutex<HashMap<i64, mpsc::Sender<Value>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let pend = pending.clone();
    let stdin_rd = stdin.clone();
    std::thread::spawn(move || stdio_read_loop(stdout, pend, stdin_rd));

    let conn = StdioConn {
        stdin,
        pending,
        next_id: AtomicI64::new(1),
        child: Mutex::new(child),
    };
    let caps = conn.initialize()?;
    Ok((Arc::new(conn), caps))
}

// ── Streamable HTTP 传输 ─────────────────────────────────────────────────────

struct HttpConn {
    client: reqwest::blocking::Client,
    url: String,
    headers: Vec<(String, String)>,
    session_id: Mutex<Option<String>>,
    next_id: AtomicI64,
}

/// 从 SSE 文本里取出 id 匹配的 JSON-RPC 响应（纯函数）。
fn sse_find_response(stream: &str, id: i64) -> Option<Value> {
    for line in stream.lines() {
        let line = line.trim();
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(data.trim()) else {
            continue;
        };
        if v.get("id").and_then(Value::as_i64) == Some(id)
            && (v.get("result").is_some() || v.get("error").is_some())
        {
            return Some(v);
        }
    }
    None
}

impl HttpConn {
    fn initialize(&self) -> Result<Value, String> {
        let r = self.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": client_capabilities(),
                "clientInfo": { "name": "wisecortex", "version": env!("CARGO_PKG_VERSION") }
            }),
            CONNECT_TIMEOUT,
        )?;
        self.notify("notifications/initialized", json!({}));
        Ok(r.get("capabilities").cloned().unwrap_or_else(|| json!({})))
    }

    fn base_builder(&self, timeout: Duration) -> reqwest::blocking::RequestBuilder {
        let mut rb = self
            .client
            .post(&self.url)
            .timeout(timeout)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", PROTOCOL_VERSION);
        for (k, v) in &self.headers {
            rb = rb.header(k, v);
        }
        if let Some(sid) = self.session_id.lock().unwrap().clone() {
            rb = rb.header("mcp-session-id", sid);
        }
        rb
    }
}

impl Transport for HttpConn {
    fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        let resp = self
            .base_builder(timeout)
            .json(&body)
            .send()
            .map_err(|e| format!("HTTP 请求失败: {e}"))?;
        // 捕获会话 id（initialize 时服务端下发）。
        if let Some(sid) = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|h| h.to_str().ok())
        {
            *self.session_id.lock().unwrap() = Some(sid.to_string());
        }
        let status = resp.status();
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_string();
        let text = resp.text().map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("HTTP {status}: {text}"));
        }
        let v = if ct.contains("text/event-stream") {
            sse_find_response(&text, id).ok_or("SSE 响应中无匹配 id")?
        } else {
            serde_json::from_str::<Value>(&text).map_err(|e| format!("响应非 JSON: {e}"))?
        };
        result_of(&v)
    }

    fn notify(&self, method: &str, params: Value) {
        let body = json!({"jsonrpc":"2.0","method":method,"params":params});
        let _ = self.base_builder(CONNECT_TIMEOUT).json(&body).send();
    }
}

/// 连接一个 Streamable HTTP MCP 服务器并完成握手，返回 (传输, 服务端能力)。
fn connect_http(name: &str, cfg: &McpServerConfig) -> Result<(Arc<dyn Transport>, Value), String> {
    let url = cfg
        .url
        .clone()
        .filter(|u| !u.trim().is_empty())
        .ok_or("url 为空")?;
    let client = crate::net::blocking_builder()
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败 ({name}): {e}"))?;
    let headers = cfg
        .headers
        .as_ref()
        .map(|h| h.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    let conn = HttpConn {
        client,
        url,
        headers,
        session_id: Mutex::new(None),
        next_id: AtomicI64::new(1),
    };
    let caps = conn.initialize()?;
    Ok((Arc::new(conn), caps))
}

// ── 工具发现 ──────────────────────────────────────────────────────────────────

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// 构造一个 McpToolInfo（合成或真实）。
fn mk(server: &str, tool: &str, kind: McpToolKind, desc: &str, schema: Value) -> McpToolInfo {
    McpToolInfo {
        server: server.to_string(),
        tool: tool.to_string(),
        kind,
        full_name: leak(format!("mcp__{server}__{tool}")),
        description: leak(desc.to_string()),
        input_schema: schema,
    }
}

/// 解析 `tools/list` 结果为 McpToolInfo 列表（kind=Tool）。
fn parse_tools(server: &str, result: &Value) -> Vec<McpToolInfo> {
    let mut out = Vec::new();
    let Some(tools) = result.get("tools").and_then(Value::as_array) else {
        return out;
    };
    for t in tools {
        let Some(tool) = t.get("name").and_then(Value::as_str) else {
            continue;
        };
        let desc = t
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let schema = t
            .get("inputSchema")
            .cloned()
            .unwrap_or_else(|| json!({"type":"object"}));
        out.push(mk(server, tool, McpToolKind::Tool, &desc, schema));
    }
    out
}

/// resources 能力 → 合成 list_resources / read_resource 工具。
fn synth_resource_tools(server: &str) -> Vec<McpToolInfo> {
    vec![
        mk(
            server,
            "list_resources",
            McpToolKind::ListResources,
            "列出该 MCP 服务器暴露的资源（返回 uri/name/description 的 JSON）。无参数。",
            json!({ "type": "object", "properties": {} }),
        ),
        mk(
            server,
            "read_resource",
            McpToolKind::ReadResource,
            "读取一个 MCP 资源的内容。先用 list_resources 拿到 uri 再调用。",
            json!({
                "type": "object",
                "properties": { "uri": { "type": "string", "description": "资源 uri" } },
                "required": ["uri"]
            }),
        ),
    ]
}

/// prompts 能力 → 合成 list_prompts / get_prompt 工具。
fn synth_prompt_tools(server: &str) -> Vec<McpToolInfo> {
    vec![
        mk(
            server,
            "list_prompts",
            McpToolKind::ListPrompts,
            "列出该 MCP 服务器暴露的 prompt 模板（返回 name/description/arguments 的 JSON）。无参数。",
            json!({ "type": "object", "properties": {} }),
        ),
        mk(
            server,
            "get_prompt",
            McpToolKind::GetPrompt,
            "取一个 prompt 模板渲染后的消息。参数 name（必填）与 arguments（对象，按模板要求填）。",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "prompt 名（先用 list_prompts 获取）" },
                    "arguments": { "type": "object", "description": "模板参数" }
                },
                "required": ["name"]
            }),
        ),
    ]
}

/// 按服务端能力发现工具：tools/list + 资源/提示词合成工具。
fn discover(server: &str, transport: &dyn Transport, caps: &Value) -> Vec<McpToolInfo> {
    let mut out = Vec::new();
    if caps.get("tools").is_some() {
        match transport.request("tools/list", json!({}), CONNECT_TIMEOUT) {
            Ok(r) => out.extend(parse_tools(server, &r)),
            Err(e) => crate::seprintln!("[mcp] {server} tools/list 失败: {e}"),
        }
    }
    if caps.get("resources").is_some() {
        out.extend(synth_resource_tools(server));
    }
    if caps.get("prompts").is_some() {
        out.extend(synth_prompt_tools(server));
    }
    out
}

// ── 结果抽取 ──────────────────────────────────────────────────────────────────

/// 从 `tools/call` 结果里抽取文本（content[].text 拼接）；isError=true 视为错误。
fn extract_text(result: &Value) -> Result<String, String> {
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut text = String::new();
    if let Some(content) = result.get("content").and_then(Value::as_array) {
        for c in content {
            match c.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(t) = c.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                }
                Some(other) => {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&format!("[{other} content]"));
                }
                None => {}
            }
        }
    }
    if text.is_empty() {
        text = result.to_string();
    }
    if is_error {
        Err(text)
    } else {
        Ok(text)
    }
}

/// 从 `resources/read` 结果抽取文本（contents[].text；blob 给占位）。
fn extract_resource_text(result: &Value) -> String {
    let mut out = String::new();
    if let Some(contents) = result.get("contents").and_then(Value::as_array) {
        for c in contents {
            if let Some(t) = c.get("text").and_then(Value::as_str) {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(t);
            } else if c.get("blob").is_some() {
                let mime = c
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .unwrap_or("binary");
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&format!("[binary {mime}]"));
            }
        }
    }
    if out.is_empty() {
        out = result.to_string();
    }
    out
}

/// 从 `prompts/get` 结果抽取文本（messages[].content.text，标注 role）。
fn extract_prompt_text(result: &Value) -> String {
    let mut out = String::new();
    if let Some(msgs) = result.get("messages").and_then(Value::as_array) {
        for m in msgs {
            let role = m.get("role").and_then(Value::as_str).unwrap_or("");
            let text = m
                .get("content")
                .and_then(|c| c.get("text").and_then(Value::as_str))
                .unwrap_or("");
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&format!("[{role}] {text}"));
        }
    }
    if out.is_empty() {
        out = result.to_string();
    }
    out
}

// ── Manager / 公共 API ────────────────────────────────────────────────────────

struct Manager {
    conns: HashMap<String, Arc<dyn Transport>>,
    tools: Vec<McpToolInfo>,
    started: bool,
}

fn manager() -> &'static Mutex<Manager> {
    static M: OnceLock<Mutex<Manager>> = OnceLock::new();
    M.get_or_init(|| {
        Mutex::new(Manager {
            conns: HashMap::new(),
            tools: Vec::new(),
            started: false,
        })
    })
}

/// 启动时连接所有已配置（未禁用）的 MCP 服务器并发现能力。幂等：只做一次。
pub fn ensure_started() {
    {
        let m = manager().lock().unwrap();
        if m.started {
            return;
        }
    }
    let cfg = crate::config::load();
    let mut conns: HashMap<String, Arc<dyn Transport>> = HashMap::new();
    let mut tools = Vec::new();
    for (name, sc) in &cfg.mcp_servers {
        if sc.disabled {
            continue;
        }
        let is_http = sc
            .url
            .as_deref()
            .map(|u| !u.trim().is_empty())
            .unwrap_or(false);
        let connected = if is_http {
            connect_http(name, sc)
        } else {
            connect_stdio(name, sc)
        };
        match connected {
            Ok((conn, caps)) => {
                tools.extend(discover(name, conn.as_ref(), &caps));
                conns.insert(name.clone(), conn);
            }
            Err(e) => crate::seprintln!("[mcp] 连接 {name} 失败: {e}"),
        }
    }
    let mut m = manager().lock().unwrap();
    m.conns = conns;
    m.tools = tools;
    m.started = true;
}

/// 丢弃所有连接与缓存（配置变更后热重连用；下次 ensure_started 会重连）。
pub fn reset() {
    let mut m = manager().lock().unwrap();
    m.conns.clear();
    m.tools.clear();
    m.started = false;
}

/// 已发现的 MCP 工具（供 agent 构建工具表）。
pub fn tool_infos() -> Vec<McpToolInfo> {
    manager().lock().unwrap().tools.clone()
}

/// 调用某服务器的某（真实或合成）工具，返回文本结果。
pub fn call_tool(
    server: &str,
    tool: &str,
    kind: McpToolKind,
    args: Value,
) -> Result<String, String> {
    let conn = {
        let m = manager().lock().unwrap();
        m.conns.get(server).cloned()
    };
    let conn = conn.ok_or_else(|| format!("MCP 服务器未连接: {server}"))?;
    match kind {
        McpToolKind::Tool => {
            let r = conn.request(
                "tools/call",
                json!({ "name": tool, "arguments": args }),
                CALL_TIMEOUT,
            )?;
            extract_text(&r)
        }
        McpToolKind::ListResources => {
            let r = conn.request("resources/list", json!({}), CALL_TIMEOUT)?;
            Ok(r.to_string())
        }
        McpToolKind::ReadResource => {
            let uri = args
                .get("uri")
                .and_then(Value::as_str)
                .ok_or("缺少参数 uri")?;
            let r = conn.request("resources/read", json!({ "uri": uri }), CALL_TIMEOUT)?;
            Ok(extract_resource_text(&r))
        }
        McpToolKind::ListPrompts => {
            let r = conn.request("prompts/list", json!({}), CALL_TIMEOUT)?;
            Ok(r.to_string())
        }
        McpToolKind::GetPrompt => {
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .ok_or("缺少参数 name")?;
            let arguments = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let r = conn.request(
                "prompts/get",
                json!({ "name": name, "arguments": arguments }),
                CALL_TIMEOUT,
            )?;
            Ok(extract_prompt_text(&r))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tools_builds_namespaced_infos() {
        let result = json!({
            "tools": [
                { "name": "read_file", "description": "Read a file", "inputSchema": {"type":"object","properties":{"path":{"type":"string"}}} },
                { "name": "noschema" }
            ]
        });
        let infos = parse_tools("fs", &result);
        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].full_name, "mcp__fs__read_file");
        assert_eq!(infos[0].tool, "read_file");
        assert_eq!(infos[0].kind, McpToolKind::Tool);
        assert_eq!(infos[0].description, "Read a file");
        assert_eq!(infos[1].input_schema, json!({"type":"object"}));
    }

    #[test]
    fn synth_tools_have_expected_kinds_and_names() {
        let r = synth_resource_tools("srv");
        assert_eq!(r[0].full_name, "mcp__srv__list_resources");
        assert_eq!(r[0].kind, McpToolKind::ListResources);
        assert_eq!(r[1].full_name, "mcp__srv__read_resource");
        assert_eq!(r[1].kind, McpToolKind::ReadResource);
        let p = synth_prompt_tools("srv");
        assert_eq!(p[0].kind, McpToolKind::ListPrompts);
        assert_eq!(p[1].full_name, "mcp__srv__get_prompt");
        assert_eq!(p[1].kind, McpToolKind::GetPrompt);
        // get_prompt 要求 name 参数。
        assert_eq!(p[1].input_schema["required"], json!(["name"]));
    }

    #[test]
    fn sse_find_response_picks_matching_id() {
        let stream = "event: message\n\
            data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"a\":1}}\n\n\
            event: message\n\
            data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"b\":2}}\n\n";
        assert_eq!(
            sse_find_response(stream, 2).unwrap()["result"],
            json!({"b":2})
        );
        // 通知（无 id/result）不匹配。
        assert!(sse_find_response("data: {\"jsonrpc\":\"2.0\",\"method\":\"x\"}\n", 1).is_none());
        assert!(sse_find_response(stream, 9).is_none());
    }

    #[test]
    fn extract_resource_and_prompt_text() {
        let res =
            json!({"contents":[{"uri":"u","text":"hello"},{"blob":"==","mimeType":"image/png"}]});
        assert_eq!(extract_resource_text(&res), "hello\n[binary image/png]");
        let pr = json!({"messages":[
            {"role":"user","content":{"type":"text","text":"hi"}},
            {"role":"assistant","content":{"type":"text","text":"yo"}}
        ]});
        assert_eq!(extract_prompt_text(&pr), "[user] hi\n[assistant] yo");
    }

    #[test]
    fn server_request_roots_ping_and_unknown() {
        // ping → 空 result。
        assert_eq!(
            handle_server_request("ping", &json!({})).unwrap(),
            json!({})
        );
        // roots/list → 至少一个 root（默认当前目录）。
        let roots = handle_server_request("roots/list", &json!({})).unwrap();
        assert!(!roots["roots"].as_array().unwrap().is_empty());
        assert!(roots["roots"][0]["uri"].as_str().unwrap().contains("://"));
        // 未知方法 → -32601。
        let err = handle_server_request("nope/x", &json!({})).unwrap_err();
        assert_eq!(err.0, -32601);
    }

    #[test]
    fn sampling_dispatches_to_registered_handler() {
        set_sampling_handler(Arc::new(|params: &Value| {
            let n = params
                .get("messages")
                .and_then(Value::as_array)
                .map(|a| a.len())
                .unwrap_or(0);
            Ok(json!({ "echoed": n }))
        }));
        let out = handle_server_request(
            "sampling/createMessage",
            &json!({ "messages": [{"role":"user"}, {"role":"assistant"}] }),
        )
        .unwrap();
        assert_eq!(out, json!({ "echoed": 2 }));
    }

    /// 用 node 起一个极小 mock MCP 服务器，跑通真实 stdio 握手 + 发现 + 调用。
    /// node 不存在则跳过（不破坏其它机器的测试）。
    #[test]
    fn live_stdio_handshake_discover_and_call() {
        if Command::new("node")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| !s.success())
            .unwrap_or(true)
        {
            eprintln!("[skip] node 不可用，跳过 MCP live 测试");
            return;
        }
        const SCRIPT: &str = "const rl=require('readline').createInterface({input:process.stdin});\
            function send(id,result){process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result})+'\\n')}\
            rl.on('line',l=>{let m;try{m=JSON.parse(l)}catch(e){return}\
            if(m.id===undefined)return;\
            if(m.method==='initialize')send(m.id,{protocolVersion:'2025-03-26',capabilities:{tools:{}},serverInfo:{name:'mock',version:'1'}});\
            else if(m.method==='tools/list')send(m.id,{tools:[{name:'echo',description:'echo back',inputSchema:{type:'object',properties:{text:{type:'string'}}}}]});\
            else if(m.method==='tools/call')send(m.id,{content:[{type:'text',text:'echoed:'+((m.params&&m.params.arguments&&m.params.arguments.text)||'')}]});});";
        let cfg = McpServerConfig {
            command: "node".into(),
            args: vec!["-e".into(), SCRIPT.into()],
            ..Default::default()
        };
        let (conn, caps) = connect_stdio("mock", &cfg).expect("连接 mock MCP 失败");
        assert!(caps.get("tools").is_some());
        let infos = discover("mock", conn.as_ref(), &caps);
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].full_name, "mcp__mock__echo");

        let called = conn
            .request(
                "tools/call",
                json!({"name":"echo","arguments":{"text":"hi"}}),
                Duration::from_secs(10),
            )
            .expect("tools/call 失败");
        assert_eq!(extract_text(&called).unwrap(), "echoed:hi");
    }

    #[test]
    fn extract_text_joins_content_and_flags_errors() {
        let ok = json!({"content":[{"type":"text","text":"hello"},{"type":"text","text":"world"}]});
        assert_eq!(extract_text(&ok).unwrap(), "hello\nworld");

        let err = json!({"content":[{"type":"text","text":"boom"}],"isError":true});
        assert_eq!(extract_text(&err).unwrap_err(), "boom");

        let img = json!({"content":[{"type":"image","data":"..."}]});
        assert_eq!(extract_text(&img).unwrap(), "[image content]");
    }
}
