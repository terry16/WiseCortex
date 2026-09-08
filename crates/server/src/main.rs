#[tokio::main]
async fn main() {
    // panic 落盘到错误日志，供自修复 skill 读取。
    wisecortex_core::buglog::install_panic_hook();
    // 改名 WiseClaw → WiseCortex：一次性搬迁老用户的配置/会话/技能目录。
    wisecortex_core::config::migrate_legacy_dirs();
    wisecortex_core::sprintln!("{}", wisecortex_server::banner());
    // 绑定地址：环境变量 WC_BIND（默认只绑本机回环）。外网请用 nginx 反代到本机，
    // 或显式 WC_BIND=0.0.0.0:7070 但必须同时设 access_key（否则护栏会拒绝启动）。
    let bind = std::env::var("WC_BIND").unwrap_or_else(|_| "127.0.0.1:7070".to_string());
    let addr = match bind.parse() {
        Ok(a) => a,
        Err(e) => {
            wisecortex_core::seprintln!("WC_BIND 不是合法的地址 {bind:?}: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = wisecortex_server::run(addr).await {
        wisecortex_core::seprintln!("{e}");
        std::process::exit(1);
    }
}
