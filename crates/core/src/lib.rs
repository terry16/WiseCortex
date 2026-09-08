//! WiseCortex agent 核心：token 压缩、缓存、tools、skills 等将逐步落在此 crate。

pub mod buglog;
pub mod clawbot;
pub mod clawbot_register;
pub mod clawhub;
pub mod config;
pub mod cost;
pub mod cron;
pub mod feishu;
pub mod feishu_longconn;
pub mod feishu_register;
pub mod hooks;
pub mod llm;
pub mod lsp;
pub mod marketplace;
pub mod mcp;
pub mod memory;
pub mod net;
pub mod notify;
pub mod openclaw_migrate;
pub mod proc;
pub mod proto;
pub mod qq;
pub mod qq_register;
pub mod registry_sources;
pub mod safeprint;
pub mod skill;
pub mod skills_state;
pub mod tools;
pub mod upload;
pub mod wecom;

/// 返回 core crate 的版本号。
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_nonempty() {
        assert!(!super::version().is_empty());
    }
}
