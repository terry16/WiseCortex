//! git 工具：
//! - `git`：只读检视（status/diff/log/show/blame…），**免确认**，便于代码审查/迭代时随时看仓库状态与改动。
//! - `git_commit`：暂存并提交（add -A + commit -m），需确认。
//!
//! 写类的其它操作（push/checkout/reset 等）走持久 shell（带确认）。

use std::path::PathBuf;
use std::process::Command;

use serde_json::{json, Value};

use super::{require_str, Tool, ToolResult};

/// 只读、无副作用的 git 子命令白名单（任意 flag 都安全）。
const READONLY: &[&str] = &[
    "status",
    "diff",
    "log",
    "show",
    "blame",
    "rev-parse",
    "ls-files",
    "shortlog",
    "describe",
    "whatchanged",
    "cat-file",
];

fn run_git(base: &PathBuf, args: &[String]) -> Result<String, String> {
    let mut git = Command::new("git");
    git.args(args).current_dir(base);
    let out = crate::proc::no_window(&mut git)
        .output()
        .map_err(|e| format!("启动 git 失败（未安装？）: {e}"))?;
    let mut s = String::from_utf8_lossy(&out.stdout).to_string();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        if !s.is_empty() && !s.ends_with('\n') {
            s.push('\n');
        }
        s.push_str(&err);
    }
    if out.status.success() {
        Ok(if s.trim().is_empty() {
            "(无输出)".into()
        } else {
            s
        })
    } else {
        Err(format!(
            "git 退出码 {}:\n{s}",
            out.status.code().unwrap_or(-1)
        ))
    }
}

// ── git（只读检视）────────────────────────────────────────────────────────────

pub struct Git {
    base: PathBuf,
}
impl Git {
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }
}
impl Tool for Git {
    fn name(&self) -> &'static str {
        "git"
    }
    fn description(&self) -> &'static str {
        "Read-only inspection of the git repository: args is a subcommand string, e.g. \"status\", \
         \"diff\", \"diff HEAD~1\", \"log --oneline -10\", \"show <sha>\". Only read-only subcommands \
         are allowed (status/diff/log/show/blame, etc.); no approval required. Use git_commit to commit, \
         and shell for any other write operation."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "args": { "type": "string", "description": "git subcommand and its arguments, e.g. \"diff HEAD\" (defaults to status)" }
            }
        })
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "git {}",
            args.get("args").and_then(Value::as_str).unwrap_or("status")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let raw = args
            .get("args")
            .and_then(Value::as_str)
            .unwrap_or("status")
            .trim();
        let raw = if raw.is_empty() { "status" } else { raw };
        let parts: Vec<String> = raw.split_whitespace().map(str::to_string).collect();
        let sub = parts.first().map(String::as_str).unwrap_or("");
        if !READONLY.contains(&sub) {
            return Err(format!(
                "git 工具仅支持只读子命令（{}）。提交请用 git_commit，其它写操作请用 shell。",
                READONLY.join("/")
            ));
        }
        run_git(&self.base, &parts)
    }
}

// ── git_commit（暂存 + 提交）──────────────────────────────────────────────────

pub struct GitCommit {
    base: PathBuf,
}
impl GitCommit {
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }
}
impl Tool for GitCommit {
    fn name(&self) -> &'static str {
        "git_commit"
    }
    fn description(&self) -> &'static str {
        "Commit changes: by default runs git add -A first to stage everything (set add_all=false to commit only what is already staged), then git commit -m message."
    }
    fn requires_approval(&self) -> bool {
        true
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "Commit message" },
                "add_all": { "type": "boolean", "description": "Run git add -A before committing (default true)" }
            },
            "required": ["message"]
        })
    }
    fn summary(&self, args: &Value) -> String {
        let m = args.get("message").and_then(Value::as_str).unwrap_or("");
        let first = m.lines().next().unwrap_or("");
        format!("git commit: {first}")
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let message = require_str(args, "message")?;
        let add_all = args.get("add_all").and_then(Value::as_bool).unwrap_or(true);
        if add_all {
            run_git(&self.base, &["add".into(), "-A".into()])?;
        }
        run_git(&self.base, &["commit".into(), "-m".into(), message])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_rejects_non_readonly_subcommand() {
        let g = Git::new(std::env::temp_dir());
        let err = g
            .execute(&json!({ "args": "push origin main" }))
            .unwrap_err();
        assert!(err.contains("只读"));
    }

    #[test]
    fn git_status_runs_in_repo() {
        // 在本仓库（cargo 测试 cwd 在 crate 目录，向上能找到 .git）跑 status 应成功。
        let g = Git::new(std::env::current_dir().unwrap());
        // 仓库内 status 成功；非仓库环境会 Err——两种都不 panic 即可。
        let _ = g.execute(&json!({ "args": "status --short" }));
    }
}
