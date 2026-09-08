//! 桌面 loopback 免粘贴登录：起一次性 `127.0.0.1` 监听，接住 OAuth 回调里的 `code`，交给
//! 兑换闭包落盘 token。**只在桌面成立**（server 与浏览器同机）；远程 webUI 仍走手动粘贴。
//!
//! 流程：REST `/start`（带 `loopback:true`）→ [`bind`] 拿端口 → 用 `http://127.0.0.1:{port}/…`
//! 回调建授权 URL → [`spawn_catch`] 起后台任务等回调 → 浏览器授权后命中监听 → 解析 code →
//! 兑换存盘 → 回浏览器一张「可关闭」页。前端开浏览器后轮询 `/status` 得知已登录。

use std::future::Future;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// 绑定 loopback 监听，返回 (listener, 实际端口)。`addr` 如 `127.0.0.1:0`(临时端口) 或 `127.0.0.1:1455`。
pub async fn bind(addr: &str) -> Result<(TcpListener, u16), String> {
    let l = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("绑定 {addr} 失败：{e}"))?;
    let port = l.local_addr().map_err(|e| e.to_string())?.port();
    Ok((l, port))
}

/// spawn 一次性捕获任务：等一个回调连接（5 分钟超时），解析 code/state，校验 state，调 `exchange`
/// 兑换（成功即由闭包落盘 token），并回浏览器一页提示。任务结束即释放端口。
pub fn spawn_catch<F, Fut>(listener: TcpListener, expect_state: String, exchange: F)
where
    F: FnOnce(String, String) -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), String>> + Send,
{
    tokio::spawn(async move {
        let accepted = tokio::time::timeout(Duration::from_secs(300), listener.accept()).await;
        let mut sock = match accepted {
            Ok(Ok((s, _))) => s,
            _ => {
                wisecortex_core::seprintln!("[oauth] loopback 等待回调超时/失败");
                return;
            }
        };
        let mut buf = [0u8; 8192];
        let n = sock.read(&mut buf).await.unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let (code, state) = parse_callback(&req);

        let (title, detail) = if code.is_empty() {
            (
                "登录未完成",
                "回调里没有 code，可关闭本页重试。".to_string(),
            )
        } else if !expect_state.is_empty() && state != expect_state {
            (
                "登录未完成",
                "state 不匹配（疑似跨站），已拒绝。可关闭本页重试。".to_string(),
            )
        } else {
            match exchange(code, state).await {
                Ok(_) => (
                    "登录成功 ✓",
                    "已完成授权，可关闭本页回到 WiseCortex。".to_string(),
                ),
                Err(e) => {
                    wisecortex_core::seprintln!("[oauth] loopback 兑换失败：{e}");
                    (
                        "登录失败",
                        format!("兑换 token 失败：{e}。可关闭本页重试。"),
                    )
                }
            }
        };
        let html = format!(
            "<!doctype html><meta charset=utf-8><title>WiseCortex</title>\
             <body style='font-family:sans-serif;text-align:center;padding-top:4em;color:#333'>\
             <h2>{title}</h2><p>{detail}</p></body>"
        );
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            html.len(),
            html
        );
        let _ = sock.write_all(resp.as_bytes()).await;
        let _ = sock.flush().await;
    });
}

/// 从 HTTP 请求首行 `GET /path?code=..&state=.. HTTP/1.1` 解析 (code, state)。纯函数，便于单测。
pub fn parse_callback(req: &str) -> (String, String) {
    let first = req.lines().next().unwrap_or("");
    let path = first.split_whitespace().nth(1).unwrap_or("");
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut code = String::new();
    let mut state = String::new();
    for kv in query.split('&') {
        match kv.split_once('=') {
            Some(("code", v)) => code = urldecode(v),
            Some(("state", v)) => state = urldecode(v),
            _ => {}
        }
    }
    (code, state)
}

/// 基础 percent-decode（`%XX` + `+`→空格）。够解 OAuth 回调 query。
fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_code_and_state_from_get_line() {
        let req = "GET /oauth2callback?code=4%2F0Axyz&state=st8&scope=x HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n";
        let (code, state) = parse_callback(req);
        assert_eq!(code, "4/0Axyz"); // %2F 解码回 /
        assert_eq!(state, "st8");
    }

    #[test]
    fn missing_code_yields_empty() {
        let (code, state) = parse_callback("GET /oauth2callback?error=access_denied HTTP/1.1\r\n");
        assert!(code.is_empty());
        assert!(state.is_empty());
    }

    #[test]
    fn no_query_is_empty() {
        let (code, _) = parse_callback("GET /favicon.ico HTTP/1.1\r\n");
        assert!(code.is_empty());
    }

    #[test]
    fn urldecode_handles_percent_and_plus() {
        assert_eq!(urldecode("a%2Fb"), "a/b");
        assert_eq!(urldecode("a+b"), "a b");
        assert_eq!(urldecode("plain"), "plain");
        assert_eq!(urldecode("100%25"), "100%");
    }
}
