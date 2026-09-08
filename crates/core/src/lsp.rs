//! LSP（Language Server Protocol）客户端：给 agent 语义能力——跳转定义、查找引用、悬停类型、
//! 文档符号、诊断。按文件扩展名自动选语言服务器（需本机已装：rust-analyzer / typescript-language-server /
//! pyright-langserver / gopls / clangd），stdio + Content-Length 帧 + JSON-RPC。
//!
//! 与 MCP 的不同：① LSP 用 `Content-Length` 头分帧（非换行）；② 需回应服务端发来的请求
//! （如 workspace/configuration）否则部分服务器会卡住；③ 诊断是异步 publishDiagnostics 通知。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const REQ_TIMEOUT: Duration = Duration::from_secs(20);
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

/// 扩展名 → (服务器命令, 参数, languageId)。
fn server_for_ext(ext: &str) -> Option<(&'static str, &'static [&'static str], &'static str)> {
    match ext {
        "rs" => Some(("rust-analyzer", &[], "rust")),
        "ts" => Some(("typescript-language-server", &["--stdio"], "typescript")),
        "tsx" => Some((
            "typescript-language-server",
            &["--stdio"],
            "typescriptreact",
        )),
        "js" | "mjs" | "cjs" => Some(("typescript-language-server", &["--stdio"], "javascript")),
        "jsx" => Some((
            "typescript-language-server",
            &["--stdio"],
            "javascriptreact",
        )),
        "py" => Some(("pyright-langserver", &["--stdio"], "python")),
        "go" => Some(("gopls", &[], "go")),
        "c" | "h" => Some(("clangd", &[], "c")),
        "cc" | "cpp" | "cxx" | "hpp" | "hxx" => Some(("clangd", &[], "cpp")),
        _ => None,
    }
}

/// 绝对路径 → file:// URI。
pub fn path_to_uri(abs: &Path) -> String {
    let p = abs.to_string_lossy().replace('\\', "/");
    if p.starts_with('/') {
        format!("file://{p}")
    } else {
        format!("file:///{p}") // Windows 盘符路径
    }
}

struct Conn {
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Arc<Mutex<HashMap<i64, mpsc::Sender<Value>>>>,
    diagnostics: Arc<Mutex<HashMap<String, Vec<Value>>>>,
    opened: Mutex<HashMap<String, i32>>,
    next_id: AtomicI64,
    #[allow(dead_code)]
    child: Mutex<Child>,
}

/// 写一条 LSP 消息（Content-Length 分帧）。
fn write_message(w: &mut ChildStdin, v: &Value) -> std::io::Result<()> {
    let body = v.to_string();
    write!(w, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    w.flush()
}

/// 读一条 LSP 消息：解析头部 Content-Length，再读对应字节数的 JSON。
fn read_message<R: BufRead>(r: &mut R) -> Option<Value> {
    let mut len = 0usize;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None; // EOF
        }
        let t = line.trim_end();
        if t.is_empty() {
            break; // 头结束
        }
        if let Some(n) = t.strip_prefix("Content-Length:") {
            len = n.trim().parse().ok()?;
        }
    }
    if len == 0 {
        return None;
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

impl Conn {
    fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel::<Value>();
        self.pending.lock().unwrap().insert(id, tx);
        {
            let mut w = self.stdin.lock().unwrap();
            write_message(
                &mut w,
                &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
            )
            .map_err(|e| format!("写 LSP 请求失败: {e}"))?;
        }
        match rx.recv_timeout(timeout) {
            Ok(resp) => {
                if let Some(err) = resp.get("error") {
                    let m = err.get("message").and_then(Value::as_str).unwrap_or("?");
                    return Err(format!("LSP 错误: {m}"));
                }
                Ok(resp.get("result").cloned().unwrap_or(Value::Null))
            }
            Err(RecvTimeoutError::Timeout) => {
                self.pending.lock().unwrap().remove(&id);
                Err(format!("LSP {method} 超时"))
            }
            Err(RecvTimeoutError::Disconnected) => Err("LSP 连接已断开".into()),
        }
    }

    fn notify(&self, method: &str, params: Value) {
        if let Ok(mut w) = self.stdin.lock() {
            let _ = write_message(
                &mut w,
                &json!({"jsonrpc":"2.0","method":method,"params":params}),
            );
        }
    }

    /// 确保文件已在服务器打开（首次 didOpen，之后 didChange 全量）。
    fn ensure_open(&self, uri: &str, language_id: &str, text: &str) {
        let mut opened = self.opened.lock().unwrap();
        match opened.get(uri).copied() {
            None => {
                self.notify(
                    "textDocument/didOpen",
                    json!({"textDocument":{"uri":uri,"languageId":language_id,"version":1,"text":text}}),
                );
                opened.insert(uri.to_string(), 1);
            }
            Some(v) => {
                let nv = v + 1;
                self.notify(
                    "textDocument/didChange",
                    json!({"textDocument":{"uri":uri,"version":nv},"contentChanges":[{"text":text}]}),
                );
                opened.insert(uri.to_string(), nv);
            }
        }
    }
}

fn manager() -> &'static Mutex<HashMap<String, Arc<Conn>>> {
    static M: OnceLock<Mutex<HashMap<String, Arc<Conn>>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 连接（或复用）某 root 下某语言服务器。
fn conn_for(root: &Path, ext: &str) -> Result<(Arc<Conn>, &'static str), String> {
    let (cmd, args, language_id) = server_for_ext(ext)
        .ok_or_else(|| format!("不支持的文件类型 .{ext}（无对应语言服务器）"))?;
    let key = format!("{}::{cmd}", root.to_string_lossy());
    if let Some(c) = manager().lock().unwrap().get(&key) {
        return Ok((c.clone(), language_id));
    }
    let conn = connect(root, cmd, args)
        .map_err(|e| format!("启动语言服务器 {cmd} 失败（是否已安装？）: {e}"))?;
    let arc = Arc::new(conn);
    manager().lock().unwrap().insert(key, arc.clone());
    Ok((arc, language_id))
}

fn connect(root: &Path, cmd: &str, args: &[&str]) -> Result<Conn, String> {
    let mut builder = Command::new(cmd);
    builder
        .args(args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::proc::no_window(&mut builder);
    let mut child = builder.spawn().map_err(|e| e.to_string())?;
    let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("无 stdin")?));
    let stdout = child.stdout.take().ok_or("无 stdout")?;
    let pending: Arc<Mutex<HashMap<i64, mpsc::Sender<Value>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let diagnostics: Arc<Mutex<HashMap<String, Vec<Value>>>> = Arc::new(Mutex::new(HashMap::new()));

    // 读线程：响应路由 / 诊断收集 / 回应服务端请求。
    let pend = pending.clone();
    let diag = diagnostics.clone();
    let stdin_for_reply = stdin.clone();
    std::thread::spawn(move || {
        let mut r = BufReader::new(stdout);
        while let Some(v) = read_message(&mut r) {
            let has_id = v.get("id").is_some();
            let method = v.get("method").and_then(Value::as_str);
            match (has_id, method) {
                // 服务端→客户端请求：回个最简响应，免得它卡住。
                (true, Some(m)) => {
                    let id = v.get("id").cloned().unwrap_or(Value::Null);
                    let result = if m == "workspace/configuration" {
                        let n = v
                            .get("params")
                            .and_then(|p| p.get("items"))
                            .and_then(Value::as_array)
                            .map(|a| a.len())
                            .unwrap_or(0);
                        Value::Array(vec![Value::Null; n])
                    } else {
                        Value::Null
                    };
                    if let Ok(mut w) = stdin_for_reply.lock() {
                        let _ = write_message(
                            &mut w,
                            &json!({"jsonrpc":"2.0","id":id,"result":result}),
                        );
                    }
                }
                // 响应：按 id 路由。
                (true, None) => {
                    if let Some(id) = v.get("id").and_then(Value::as_i64) {
                        if let Some(tx) = pend.lock().unwrap().remove(&id) {
                            let _ = tx.send(v);
                        }
                    }
                }
                // 通知：收集诊断。
                (false, Some("textDocument/publishDiagnostics")) => {
                    if let Some(p) = v.get("params") {
                        if let Some(uri) = p.get("uri").and_then(Value::as_str) {
                            let ds = p
                                .get("diagnostics")
                                .and_then(Value::as_array)
                                .cloned()
                                .unwrap_or_default();
                            diag.lock().unwrap().insert(uri.to_string(), ds);
                        }
                    }
                }
                _ => {}
            }
        }
    });

    let conn = Conn {
        stdin,
        pending,
        diagnostics,
        opened: Mutex::new(HashMap::new()),
        next_id: AtomicI64::new(1),
        child: Mutex::new(child),
    };
    let root_uri = path_to_uri(root);
    conn.request(
        "initialize",
        json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "definition": {}, "references": {}, "hover": {}, "documentSymbol": {},
                    "publishDiagnostics": {}
                },
                "workspace": { "configuration": true }
            },
            "clientInfo": { "name": "wisecortex" }
        }),
        INIT_TIMEOUT,
    )?;
    conn.notify("initialized", json!({}));
    Ok(conn)
}

// ── 操作 ────────────────────────────────────────────────────────────────────

/// 入参：操作 + 文件 + 1-based 行列（diagnostics/documentSymbol 不需要行列）。
pub fn run(
    root: &Path,
    file_abs: &Path,
    operation: &str,
    line1: u32,
    char1: u32,
) -> Result<String, String> {
    let ext = file_abs
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let (conn, language_id) = conn_for(root, &ext)?;
    let text = std::fs::read_to_string(file_abs).map_err(|e| format!("读文件失败: {e}"))?;
    let uri = path_to_uri(file_abs);
    conn.ensure_open(&uri, language_id, &text);

    // LSP 行列 0-based。
    let pos = json!({ "line": line1.saturating_sub(1), "character": char1.saturating_sub(1) });
    let td = json!({ "uri": uri });

    match operation {
        "definition" | "goToDefinition" => {
            let r = conn.request(
                "textDocument/definition",
                json!({"textDocument":td,"position":pos}),
                REQ_TIMEOUT,
            )?;
            Ok(fmt_locations(&r))
        }
        "references" | "findReferences" => {
            let r = conn.request(
                "textDocument/references",
                json!({"textDocument":td,"position":pos,"context":{"includeDeclaration":true}}),
                REQ_TIMEOUT,
            )?;
            Ok(fmt_locations(&r))
        }
        "hover" => {
            let r = conn.request(
                "textDocument/hover",
                json!({"textDocument":td,"position":pos}),
                REQ_TIMEOUT,
            )?;
            Ok(fmt_hover(&r))
        }
        "documentSymbol" | "symbols" => {
            let r = conn.request(
                "textDocument/documentSymbol",
                json!({"textDocument":td}),
                REQ_TIMEOUT,
            )?;
            Ok(fmt_symbols(&r))
        }
        "diagnostics" => {
            // 诊断异步到达；didOpen 后轮询一小会。
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(ds) = conn.diagnostics.lock().unwrap().get(&uri) {
                    return Ok(fmt_diagnostics(ds));
                }
                if Instant::now() >= deadline {
                    return Ok("(暂无诊断，或语言服务器尚未就绪)".into());
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        }
        other => Err(format!("不支持的操作: {other}")),
    }
}

fn loc_line(v: &Value) -> String {
    let uri = v
        .get("uri")
        .or_else(|| v.get("targetUri"))
        .and_then(Value::as_str)
        .unwrap_or("?")
        .trim_start_matches("file://")
        .to_string();
    let range = v.get("range").or_else(|| v.get("targetRange"));
    let line = range
        .and_then(|r| r.get("start"))
        .and_then(|s| s.get("line"))
        .and_then(Value::as_u64)
        .map(|l| l + 1)
        .unwrap_or(0);
    format!("{uri}:{line}")
}

fn fmt_locations(r: &Value) -> String {
    let arr: Vec<&Value> = match r {
        Value::Array(a) => a.iter().collect(),
        Value::Null => vec![],
        other => vec![other],
    };
    if arr.is_empty() {
        return "(未找到)".into();
    }
    arr.iter()
        .map(|v| loc_line(v))
        .collect::<Vec<_>>()
        .join("\n")
}

fn fmt_hover(r: &Value) -> String {
    let Some(c) = r.get("contents") else {
        return "(无悬停信息)".into();
    };
    let text = match c {
        Value::String(s) => s.clone(),
        Value::Object(o) => o
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        Value::Array(a) => a
            .iter()
            .map(|x| match x {
                Value::String(s) => s.clone(),
                Value::Object(o) => o
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    if text.trim().is_empty() {
        "(无悬停信息)".into()
    } else {
        text
    }
}

fn symbol_kind(k: u64) -> &'static str {
    // LSP SymbolKind 子集。
    match k {
        5 => "class",
        6 => "method",
        12 => "function",
        13 => "variable",
        11 => "interface",
        23 => "struct",
        10 => "enum",
        2 => "module",
        _ => "symbol",
    }
}

fn fmt_symbols(r: &Value) -> String {
    let Some(arr) = r.as_array() else {
        return "(无符号)".into();
    };
    if arr.is_empty() {
        return "(无符号)".into();
    }
    let mut out = Vec::new();
    for s in arr {
        let name = s.get("name").and_then(Value::as_str).unwrap_or("?");
        let kind = s
            .get("kind")
            .and_then(Value::as_u64)
            .map(symbol_kind)
            .unwrap_or("symbol");
        // DocumentSymbol 用 range.start.line；SymbolInformation 用 location.range.start.line。
        let line = s
            .get("range")
            .or_else(|| s.get("location").and_then(|l| l.get("range")))
            .and_then(|r| r.get("start"))
            .and_then(|st| st.get("line"))
            .and_then(Value::as_u64)
            .map(|l| l + 1)
            .unwrap_or(0);
        out.push(format!("{kind} {name} :{line}"));
    }
    out.join("\n")
}

fn fmt_diagnostics(ds: &[Value]) -> String {
    if ds.is_empty() {
        return "(无诊断，干净)".into();
    }
    let sev = |n: u64| match n {
        1 => "error",
        2 => "warning",
        3 => "info",
        4 => "hint",
        _ => "diag",
    };
    ds.iter()
        .map(|d| {
            let line = d
                .get("range")
                .and_then(|r| r.get("start"))
                .and_then(|s| s.get("line"))
                .and_then(Value::as_u64)
                .map(|l| l + 1)
                .unwrap_or(0);
            let s = d
                .get("severity")
                .and_then(Value::as_u64)
                .map(sev)
                .unwrap_or("diag");
            let msg = d.get("message").and_then(Value::as_str).unwrap_or("");
            format!(":{line} [{s}] {msg}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_roundtrips_platform_paths() {
        // Windows 盘符。
        let u = path_to_uri(Path::new("D:/apps/x.rs"));
        assert!(u.starts_with("file:///"), "got {u}");
        assert!(u.contains("D:/apps/x.rs"));
    }

    #[test]
    fn read_write_message_framing() {
        // 编码后能被 read_message 解出同一个对象。
        let v = json!({"jsonrpc":"2.0","id":1,"result":{"ok":true}});
        let body = v.to_string();
        let framed = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut r = BufReader::new(framed.as_bytes());
        let got = read_message(&mut r).unwrap();
        assert_eq!(got, v);
    }

    #[test]
    fn formatters_handle_shapes() {
        let loc = json!({"uri":"file:///x.rs","range":{"start":{"line":4,"character":0}}});
        assert_eq!(fmt_locations(&loc), "/x.rs:5");
        assert_eq!(fmt_locations(&Value::Null), "(未找到)");
        let hov = json!({"contents":{"kind":"markdown","value":"fn foo()"}});
        assert_eq!(fmt_hover(&hov), "fn foo()");
        let diag = json!([{"range":{"start":{"line":2}},"severity":1,"message":"boom"}]);
        assert_eq!(fmt_diagnostics(diag.as_array().unwrap()), ":3 [error] boom");
        assert_eq!(fmt_diagnostics(&[]), "(无诊断，干净)");
    }
}
