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

/// 在 `addr` 上起 WS server（从配置/环境构造 Agent）。供 CLI 二进制与桌面壳共用。
pub async fn run(addr: std::net::SocketAddr) -> std::io::Result<()> {
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

    // 护栏：绑到非回环地址（对外网）却没设 access_key → 拒绝启动，避免裸奔。
    if !external_bind_allowed(addr.ip(), access_key.is_some()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "拒绝启动：绑定到非回环地址 {addr} 暴露到外网，但未设置 access_key。\n\
                 请先设置 access_key（设置面板 / WC_ACCESS_KEY），或改回绑定 127.0.0.1 并用 nginx 反代。"
            ),
        ));
    }
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

    let listener = tokio::net::TcpListener::bind(addr).await?;
    wisecortex_core::sprintln!("listening on http://{addr} (ws: /ws)");
    axum::serve(listener, app(state)).await
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    #[test]
    fn banner_mentions_core() {
        assert!(super::banner().contains("core"));
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
