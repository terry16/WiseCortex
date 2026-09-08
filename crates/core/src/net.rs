//! 出站网络的统一构造点：按配置/环境应用代理。
//!
//! 解析优先级：环境变量 `WC_PROXY` > 配置文件 `proxy`。支持 `http(s)://` 与 `socks5://`。
//! 所有出站 reqwest 客户端都应经由这里构造，确保「设置了代理就全部走代理」（含 LLM 与技能）。
//! 代理地址解析失败时跳过代理（直连），不阻断联网。

/// 选择生效的代理地址（纯函数，便于测试）：env 优先，空串视为未设。
pub fn select_proxy(env: Option<&str>, cfg: Option<&str>) -> Option<String> {
    let pick = |s: Option<&str>| {
        s.map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    pick(env).or_else(|| pick(cfg))
}

/// 当前生效的代理地址（WC_PROXY > config.proxy）。
pub fn configured_proxy() -> Option<String> {
    let env = std::env::var("WC_PROXY").ok();
    let cfg = crate::config::load().proxy;
    select_proxy(env.as_deref(), cfg.as_deref())
}

#[cfg(test)]
mod diag_tests {
    use super::*;

    #[derive(Debug)]
    struct Err2(&'static str, Option<Box<Err2>>);
    impl std::fmt::Display for Err2 {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }
    impl std::error::Error for Err2 {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.1
                .as_deref()
                .map(|e| e as &(dyn std::error::Error + 'static))
        }
    }

    #[test]
    fn err_chain_unfolds_root_cause() {
        // 回归：顶层只说「发不出去」，真正原因（连接超时）在 source 链里——必须摊出来。
        let e = Err2(
            "error sending request",
            Some(Box::new(Err2(
                "connect error",
                Some(Box::new(Err2("connection timed out", None))),
            ))),
        );
        let s = err_chain(&e);
        assert!(s.contains("error sending request"), "{s}");
        assert!(s.contains("connect error"), "{s}");
        assert!(s.contains("connection timed out"), "根因必须可见: {s}");
    }

    #[test]
    fn err_chain_dedups_repeated_message() {
        let e = Err2("same", Some(Box::new(Err2("same", None))));
        assert_eq!(err_chain(&e), "same");
    }

    #[test]
    fn proxy_hint_says_direct_when_unset() {
        // 说明国内直连必失败的那句提示（不回显代理地址）。
        assert!(proxy_hint().contains("直连") || proxy_hint().contains("经代理"));
    }
}

/// 把错误链摊平成一行。reqwest 的顶层 Display 往往只说「error sending request for url (…)」，
/// **真正原因**（dns 解析失败 / 连接超时 / TLS 握手失败 / 代理拒绝）藏在 `source()` 链里——不展开
/// 就只能干瞪眼。诊断出站网络问题时用它，别直接 `{e}`。
pub fn err_chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut cur = e.source();
    while let Some(c) = cur {
        let msg = c.to_string();
        // hyper/reqwest 有时层层同文案，重复的不再拼。
        if !s.ends_with(&msg) {
            s.push_str(" ← ");
            s.push_str(&msg);
        }
        cur = c.source();
    }
    s
}

/// 内嵌在错误文案里的机器可读标记：表示这是**网络层瞬时失败**（超时 / 连接失败），值得自动重试。
/// 由出站鉴权请求（token 刷新 / Code Assist onboarding）在 reqwest 传输失败时打上，
/// client.rs 据此把「订阅鉴权失败」映射成可重试还是终态 401。
/// 用不可打印控制字符，既不会撞上正常文案，也不会晃到用户眼前（展示前会被剥掉）。
pub const TRANSIENT_TAG: &str = "\u{1}wc-transient\u{1}";

/// reqwest 错误是否为**网络层瞬时失败**：请求超时（含被总超时掐断的代理卡死）或建连失败。
/// 这类失败重试往往能成（反代抽风、瞬断），区别于上游返回的 4xx 业务错误。
pub fn is_transient(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect()
}

/// 若 `e` 是瞬时网络失败，给诊断消息前缀打上 [`TRANSIENT_TAG`]（供上层识别为可自动重试）；
/// 否则原样返回。用在出站请求的 `.map_err` 里，保留各处原有文案、只附加可重试信号。
pub fn maybe_transient(e: &reqwest::Error, msg: String) -> String {
    if is_transient(e) {
        format!("{TRANSIENT_TAG}{msg}")
    } else {
        msg
    }
}

/// 上游返回错误状态时：`5xx` / `429` 视为瞬时（打 [`TRANSIENT_TAG`]，供上层自动重试）；其余原样。
/// 反代 / 网关抽风常以 502/503/504 现形，与传输超时一样值得重试；而 4xx（如 invalid_grant、
/// 权限不足）是确定性错误，重试无用。
pub fn maybe_transient_status(status: reqwest::StatusCode, msg: String) -> String {
    if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        format!("{TRANSIENT_TAG}{msg}")
    } else {
        msg
    }
}

/// 出站是否经代理的简述，用于错误信息里点明「是不是在直连」——国内直连 Google/OpenAI 必失败，
/// 这一句能让人立刻定位。**不回显代理地址**（可能含账号密码）。
pub fn proxy_hint() -> &'static str {
    if configured_proxy().is_some() {
        "经代理"
    } else {
        "直连·未配置代理"
    }
}

/// 连接阶段超时：代理/网络异常时，避免 TCP 连接阶段无限期挂起。
/// 仅约束「建连」，不约束「建连后传输」（流式响应靠调用方的空闲超时兜底）。
const CONNECT_TIMEOUT_SECS: u64 = 30;

/// **非流式**请求的总超时（含建连 + 读完整个响应）。仅用于 OAuth token 刷新 / Code Assist
/// onboarding / userinfo 这类短 JSON 调用——它们本该几秒内返回，一旦代理**连上后**静默卡死
/// （Azure 冷启动、上游 stall），只有 `connect_timeout` 是拦不住的（连接早已建立），会**永久**
/// 挂住：这些请求不是流，没有「空闲超时」可兜底，于是整个 agent 循环卡在鉴权阶段、
/// 会话 status 永远停在 `working`（真机踩过：Gemini 订阅第 2 轮请求发出后再无返回）。
///
/// ⚠️ **绝不可**用于 LLM 流式请求：正常的流式回复本就持续数分钟，加总超时会把长回复腰斩。
/// 流式请求继续用 [`async_builder`]（只设 connect_timeout），由 client.rs 的逐块空闲超时兜底。
/// 默认 60s，可用 env `WC_REQUEST_TIMEOUT_SECS` 覆盖（0/非法回落默认；也便于测试用小值）。
fn request_timeout_secs() -> u64 {
    std::env::var("WC_REQUEST_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(60)
}

/// 构造「放过 loopback」的代理：远程照走代理，但 `127.0.0.1` / `localhost` / `::1` 直连。
/// reqwest 对**显式设定**的代理**不会**自动放过 localhost，须手动挂 `NoProxy`，否则本地请求
/// （单测的 mock 服务器、或本地模型如 Ollama/LM Studio）会被错误地塞进代理而连不上。
fn proxy_for(url: &str) -> Option<reqwest::Proxy> {
    let no = reqwest::NoProxy::from_string("127.0.0.1,localhost,::1");
    reqwest::Proxy::all(url).ok().map(|p| p.no_proxy(no))
}

/// 阻塞 client builder，按配置应用代理（loopback 直连）。
pub fn blocking_builder() -> reqwest::blocking::ClientBuilder {
    blocking_builder_with(configured_proxy().as_deref())
}

/// 同 `blocking_builder`，但代理地址显式注入（便于测试，绕开全局 config/env）。
pub fn blocking_builder_with(proxy: Option<&str>) -> reqwest::blocking::ClientBuilder {
    let b = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS));
    match proxy.and_then(proxy_for) {
        Some(p) => b.proxy(p),
        None => b,
    }
}

/// 异步 client builder，按配置应用代理（loopback 直连）。**不设总请求超时**——供流式 LLM 用。
pub fn async_builder() -> reqwest::ClientBuilder {
    async_builder_with(configured_proxy().as_deref())
}

/// 带**总请求超时**的异步 client builder（见 [`request_timeout_secs`]），按配置应用代理。
/// 用于非流式的短请求（OAuth token / onboarding / userinfo），防止代理连上后静默卡死导致永久挂起。
pub fn async_builder_timed() -> reqwest::ClientBuilder {
    async_builder().timeout(std::time::Duration::from_secs(request_timeout_secs()))
}

/// 同 `async_builder`，但代理地址显式注入（便于测试，绕开全局 config/env）。
pub fn async_builder_with(proxy: Option<&str>) -> reqwest::ClientBuilder {
    let b = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS));
    match proxy.and_then(proxy_for) {
        Some(p) => b.proxy(p),
        None => b,
    }
}

/// 给子进程（如 git）注入的代理环境变量键值对（无代理时为空）。
pub fn proxy_env() -> Vec<(&'static str, String)> {
    match configured_proxy() {
        Some(u) => vec![
            ("HTTP_PROXY", u.clone()),
            ("HTTPS_PROXY", u.clone()),
            ("ALL_PROXY", u),
        ],
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_proxy_prefers_env_then_cfg_skips_empty() {
        assert_eq!(
            select_proxy(Some("http://e"), Some("http://c")).as_deref(),
            Some("http://e")
        );
        assert_eq!(
            select_proxy(None, Some("http://c")).as_deref(),
            Some("http://c")
        );
        assert_eq!(
            select_proxy(Some("  "), Some("http://c")).as_deref(),
            Some("http://c")
        );
        assert_eq!(select_proxy(Some(""), None), None);
        assert_eq!(select_proxy(None, None), None);
    }

    #[test]
    fn builders_accept_http_and_socks5() {
        // 代理地址能被 reqwest 接受（不 panic / 不报错）。
        assert!(reqwest::Proxy::all("http://127.0.0.1:8080").is_ok());
        assert!(reqwest::Proxy::all("socks5://127.0.0.1:1080").is_ok());
        // builder 在无配置时也能构建。
        assert!(blocking_builder().build().is_ok());
        assert!(async_builder().build().is_ok());
        assert!(async_builder_timed().build().is_ok());
    }

    #[test]
    fn status_transient_only_tags_5xx_and_429() {
        use reqwest::StatusCode;
        // 反代/网关抽风的 5xx 与限流 429 → 打标记（可重试）。
        for s in [
            StatusCode::BAD_GATEWAY,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::GATEWAY_TIMEOUT,
            StatusCode::TOO_MANY_REQUESTS,
        ] {
            assert!(
                maybe_transient_status(s, "m".into()).starts_with(TRANSIENT_TAG),
                "{s} 应判瞬时"
            );
        }
        // 确定性 4xx（凭证失效/权限不足）→ 不打标记（重试无用）。
        for s in [
            StatusCode::BAD_REQUEST,
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
        ] {
            assert_eq!(maybe_transient_status(s, "m".into()), "m", "{s} 不该判瞬时");
        }
    }

    #[tokio::test]
    async fn timed_builder_times_out_where_plain_builder_would_hang() {
        // 模拟真机故障形态：代理**连上后静默卡死**——接受 TCP 连接，但永不回响应头。
        // connect_timeout 对此无能为力（连接早已建立）。这正是 Gemini 订阅第 2 轮请求发出后
        // 再无返回、会话 status 永远停在 working 的根因：非流式的鉴权请求没有总超时可兜底。
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut held = Vec::new();
            loop {
                if let Ok((sock, _)) = listener.accept().await {
                    held.push(sock); // 攥住 server 端 socket，别让连接被 RST，制造真正的「卡死」
                }
            }
        });
        let url = format!("http://{addr}/");

        // 带总超时的 builder：~1s 内以**超时错误**返回，不永久挂起。
        std::env::set_var("WC_REQUEST_TIMEOUT_SECS", "1");
        let timed = async_builder_timed().no_proxy().build().unwrap();
        std::env::remove_var("WC_REQUEST_TIMEOUT_SECS");
        let start = std::time::Instant::now();
        let e = timed.get(&url).send().await.unwrap_err();
        assert!(e.is_timeout(), "带超时的 builder 应超时返回，实得 {e}");
        assert!(
            start.elapsed() < std::time::Duration::from_secs(5),
            "应尽快超时，实测 {:?}",
            start.elapsed()
        );
        // 这个真实的超时错误必须被判为「瞬时」——否则鉴权层不会把它标成可重试，自动重试就失效。
        assert!(is_transient(&e), "代理卡死超时应判为瞬时: {e}");
        assert!(
            maybe_transient(&e, "x".into()).starts_with(TRANSIENT_TAG),
            "瞬时失败应被打上标记"
        );

        // 对照：无总超时的 async_builder 在同一卡死服务器上**不会**自行返回——
        // 用外层 2s tokio 超时去套，必然是外层先触发（证明请求本身还挂着，即旧行为会永久卡死）。
        let plain = async_builder().no_proxy().build().unwrap();
        let hung =
            tokio::time::timeout(std::time::Duration::from_secs(2), plain.get(&url).send()).await;
        assert!(hung.is_err(), "无总超时的请求本应一直挂着，被外层 2s 兜底");
    }
}
