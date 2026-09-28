//! WiseCortex HTTP/WS server（Linux WebUI 与桌面后端共用）。
//! WS 协议契约见 docs/protocols/ws-protocol.md。

pub mod agent;
pub mod clawbot_poll;
pub mod commands;
pub mod feishu_ws;
pub mod im;
pub mod jobs;
pub mod mcp_sampling;
pub mod oauth_loopback;
/// WS 协议模型已抽到 core 供 server 与 tui 共用；此处 re-export 保持 `crate::proto::…` 不变。
pub use wisecortex_core::proto;
pub mod qq_ws;
pub mod registry;
pub mod rest;
pub mod scheduler;
pub mod ws;

pub use agent::Agent;
pub use registry::SessionRegistry;
pub use ws::{app, AppState};

/// 启动横幅，包含 core 版本。
pub fn banner() -> String {
    format!("wisecortex-server (core {})", wisecortex_core::version())
}

/// 外网绑定护栏：绑非回环地址（对外网暴露）时必须设了 access_key。
/// 回环地址不受限（本机/经 nginx 反代的标准场景）。
pub fn external_bind_allowed(ip: std::net::IpAddr, has_access_key: bool) -> bool {
    ip.is_loopback() || has_access_key
}

/// 桌面端端口顺延的尝试上限：首选端口被占就往后数，最多试这么多个。
/// 有界是故意的：真碰上整段被占，应该报错给人看，而不是无限扫端口。
pub const PORT_SCAN_LIMIT: u16 = 10;

/// 从 `start` 开始依次尝试绑定，返回第一个绑得上的 listener。
///
/// 桌面端专用：端口被占（上次退出残留的进程、别的软件占了 7070）时，旧实现
/// 直接 bind 失败 → 内嵌后端静默死掉 → 前端连不上只看到「要密钥」和空配置，
/// 真正的原因（端口冲突）用户无从得知。顺延一个可用端口就能继续跑。
///
/// 只对「端口被占用」顺延；其它错误（如权限不足、地址不可用）直接上报——
/// 那些换个端口也好不了，掩盖它们只会把故障变得更难查。
pub async fn bind_with_fallback(
    start: std::net::SocketAddr,
    tries: u16,
) -> std::io::Result<tokio::net::TcpListener> {
    let mut last_err = None;
    for offset in 0..tries.max(1) {
        let port = match start.port().checked_add(offset) {
            Some(p) => p,
            None => break, // 端口号溢出（贴近 65535），不再往后试。
        };
        let mut addr = start;
        addr.set_port(port);
        match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => return Ok(l),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                last_err = Some(e);
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err
        .unwrap_or_else(|| std::io::Error::new(std::io::ErrorKind::AddrInUse, "没有可用端口")))
}

/// 在 `addr` 上起 WS server（从配置/环境构造 Agent）。供 CLI 二进制与桌面壳共用。
pub async fn run(addr: std::net::SocketAddr) -> std::io::Result<()> {
    // 护栏必须在 bind **之前**：否则绑 0.0.0.0 会有一个真实监听的瞬时窗口，
    // 哪怕随后立刻报错退出，那一瞬间服务已经对外网可达了。
    bind_guard(addr)?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    serve_on(listener).await
}

/// 外网绑定护栏检查：绑非回环却没设 access_key 时返回错误。
fn bind_guard(addr: std::net::SocketAddr) -> std::io::Result<()> {
    let has_key = std::env::var("WC_ACCESS_KEY")
        .ok()
        .or_else(|| wisecortex_core::config::load().access_key)
        .is_some_and(|k| !k.is_empty());
    if external_bind_allowed(addr.ip(), has_key) {
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        format!(
            "拒绝启动：绑定到非回环地址 {addr} 暴露到外网，但未设置 access_key。\n\
             请先设置 access_key（设置面板 / WC_ACCESS_KEY），或改回绑定 127.0.0.1 并用 nginx 反代。"
        ),
    ))
}

/// 准备好 `AppState` 并拉起各后台任务（调度器、飞书/QQ/微信长连接）。
/// 抽出来是为了让 [`run`] 与 [`serve_on`] 共用同一套启动流程，避免两份实现漂移。
async fn prepare_state(addr: std::net::SocketAddr) -> std::io::Result<AppState> {
    // 护栏：绑到非回环地址（对外网）却没设 access_key → 拒绝启动，避免裸奔。
    // run() 已在 bind 前查过一次；这里再查是给直接调 serve_on 的调用方（桌面壳）兜底。
    bind_guard(addr)?;
    // 启动即把内置技能播种到数据目录（开箱即带、可卸载、卸载后不回来）。
    wisecortex_core::marketplace::seed_builtins();
    // MCP sampling：注册处理器（服务端反向请求我们跑 LLM），须在连接 MCP 前就绪。
    let rt = tokio::runtime::Handle::current();
    wisecortex_core::mcp::set_sampling_handler(std::sync::Arc::new(
        move |params: &serde_json::Value| mcp_sampling::handle(&rt, params),
    ));
    // 后台连接已配置的 MCP 服务器并发现工具（不阻塞启动；连上后下一轮对话即可用）。
    tokio::task::spawn_blocking(wisecortex_core::mcp::ensure_started);

    let agent = std::sync::Arc::new(Agent::configure());
    wisecortex_core::sprintln!("agent: {}", agent.describe());

    // 访问密钥：环境变量 WC_ACCESS_KEY > 配置文件。
    let access_key = std::env::var("WC_ACCESS_KEY")
        .ok()
        .or_else(|| wisecortex_core::config::load().access_key)
        .filter(|k| !k.is_empty());

    wisecortex_core::sprintln!(
        "access: {}",
        if access_key.is_some() {
            "需要密钥"
        } else {
            "公开模式（无密钥）"
        }
    );

    // 会话持久化：有数据目录则从磁盘恢复，否则纯内存。
    let registry = match wisecortex_core::config::sessions_dir() {
        Some(dir) => {
            wisecortex_core::sprintln!("sessions: {}", dir.display());
            SessionRegistry::with_dir(dir)
        }
        None => SessionRegistry::new(),
    };
    let state = AppState::new(registry, agent).with_access_key(access_key);

    // 启动定时任务调度器（即使没有 WS 客户端也在跑）。
    scheduler::spawn(state.agent.clone(), state.registry.clone());

    // 飞书长连接（仅当启用且凭据就绪时；免公网回调）。
    feishu_ws::spawn(state.agent.clone(), state.registry.clone());

    // QQ 官方机器人网关（仅当启用且凭据就绪时；免公网回调）。
    qq_ws::spawn(state.agent.clone());

    // 微信 ClawBot 长轮询（仅当启用且已扫码时；免公网回调，故桌面端同样可用）。
    clawbot_poll::spawn(state.agent.clone(), state.registry.clone());

    Ok(state)
}

/// 在已绑定的 listener 上提供服务。
///
/// 与 [`run`] 的区别：调用方自己完成 bind，因而能在**启动前**拿到真实端口——
/// 桌面端需要这个来把实际端口注入前端（端口可能因冲突顺延，不再恒为 7070）。
pub async fn serve_on(listener: tokio::net::TcpListener) -> std::io::Result<()> {
    serve_on_ready(listener, |_| {}).await
}

/// 同 [`serve_on`]，但在**即将开始接受请求之前**回调一次 `on_ready(addr)`。
///
/// 为什么需要它：`prepare_state` 要播种内置技能、注册 MCP、恢复会话，冷启动时这段
/// 可能耗时数秒。而 listener 早已 bind 成功，端口处于「已监听但服务未就绪」的状态——
/// 此时前端的请求会被 TCP 接受却得不到响应，比「连接被拒」更难判断。
/// 桌面端据此在后端真正可服务之后才加载页面，从根上消掉这个竞态。
pub async fn serve_on_ready<F>(
    listener: tokio::net::TcpListener,
    on_ready: F,
) -> std::io::Result<()>
where
    F: FnOnce(std::net::SocketAddr),
{
    let addr = listener.local_addr()?;
    let state = prepare_state(addr).await?;
    wisecortex_core::sprintln!("listening on http://{addr} (ws: /ws)");
    on_ready(addr);
    axum::serve(listener, app(state)).await
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    #[test]
    fn banner_mentions_core() {
        assert!(super::banner().contains("core"));
    }

    #[tokio::test]
    async fn bind_with_fallback_skips_occupied_port() {
        // 先占住一个端口，再从它开始要求顺延：应拿到别的端口，而不是失败。
        let squatter = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let taken = squatter.local_addr().unwrap();

        let got = super::bind_with_fallback(taken, 5).await.unwrap();
        let got_addr = got.local_addr().unwrap();
        assert_ne!(got_addr.port(), taken.port(), "应跳过被占端口");
        assert!(got_addr.port() > taken.port(), "应往后顺延");
    }

    #[tokio::test]
    async fn bind_with_fallback_reports_when_all_taken() {
        // tries=1 且该端口被占 → 没有退路，必须报 AddrInUse 而不是静默成功。
        let squatter = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let taken = squatter.local_addr().unwrap();

        let err = super::bind_with_fallback(taken, 1).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
    }

    #[test]
    fn external_bind_requires_access_key() {
        let loop_v4: IpAddr = "127.0.0.1".parse().unwrap();
        let loop_v6: IpAddr = "::1".parse().unwrap();
        let any: IpAddr = "0.0.0.0".parse().unwrap();
        let lan: IpAddr = "192.168.1.10".parse().unwrap();

        // 回环：无论是否有 key 都允许。
        assert!(super::external_bind_allowed(loop_v4, false));
        assert!(super::external_bind_allowed(loop_v6, false));
        // 非回环：无 key 拒绝，有 key 允许。
        assert!(!super::external_bind_allowed(any, false));
        assert!(!super::external_bind_allowed(lan, false));
        assert!(super::external_bind_allowed(any, true));
        assert!(super::external_bind_allowed(lan, true));
    }
}
