//! 事件钩子（hooks）：在 agent 生命周期的关键点运行用户配置的命令，用于护栏与副作用。
//!
//! 事件：
//! - `PreToolUse`  — 工具执行前。可**拦截**（阻止执行，原因回灌给模型）或放行；payload 带 tool_name/tool_input。
//! - `PostToolUse` — 工具执行后。可追加 `additionalContext` 到结果（如自动格式化后的提示）；payload 带 tool_result。
//! - `UserPromptSubmit` — 用户发消息后、跑 agent 前。可拦截该回合或追加上下文。
//! - `SessionStart` — 任务首条消息时（副作用，如初始化环境）。
//! - `Stop` — 一个回合结束后（副作用，如发通知）。
//!
//! 钩子命令通过 shell 运行，事件 JSON 从 **stdin** 传入。决策：
//! - stdout 是 JSON 且含 `{"decision":"block","reason":..}` → 拦截；`additionalContext` → 追加上下文。
//! - 否则按退出码：非 0 → 拦截（原因取 stderr||stdout）；0 → 放行。
//!
//! 未配置该事件的钩子时**零开销**（直接返回）。

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;
use wait_timeout::ChildExt;

use crate::config::HookConfig;

const DEFAULT_HOOK_TIMEOUT_MS: u64 = 30_000;

/// 钩子聚合结果。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct HookOutcome {
    /// 是否拦截（阻止工具执行 / 阻止本回合）。
    pub block: bool,
    /// 拦截原因（回灌给模型）。
    pub reason: Option<String>,
    /// 追加给模型的上下文（多个钩子的合并）。
    pub additional_context: Option<String>,
}

/// 事件名常量。
pub mod event {
    pub const PRE_TOOL_USE: &str = "PreToolUse";
    pub const POST_TOOL_USE: &str = "PostToolUse";
    pub const USER_PROMPT_SUBMIT: &str = "UserPromptSubmit";
    pub const SESSION_START: &str = "SessionStart";
    pub const STOP: &str = "Stop";
}

/// 加载配置并运行某事件的钩子。`matcher_key`=工具名（PreToolUse/PostToolUse 用）。
pub fn for_event(event: &str, matcher_key: Option<&str>, payload: Value) -> HookOutcome {
    let cfg = crate::config::load();
    let Some(hooks) = cfg.hooks.get(event) else {
        return HookOutcome::default();
    };
    if hooks.is_empty() {
        return HookOutcome::default();
    }
    run_hooks(hooks, matcher_key, &payload)
}

/// 运行一组钩子（已知列表），聚合结果。任一钩子拦截即整体拦截。
pub fn run_hooks(hooks: &[HookConfig], matcher_key: Option<&str>, payload: &Value) -> HookOutcome {
    let mut out = HookOutcome::default();
    let mut reasons: Vec<String> = Vec::new();
    let mut contexts: Vec<String> = Vec::new();
    for h in hooks {
        if !matches(h.matcher.as_deref(), matcher_key) {
            continue;
        }
        if h.command.trim().is_empty() {
            continue;
        }
        let r = run_one(
            &h.command,
            payload,
            h.timeout_ms.unwrap_or(DEFAULT_HOOK_TIMEOUT_MS),
        );
        if r.block {
            out.block = true;
            if let Some(reason) = r.reason {
                reasons.push(reason);
            }
        }
        if let Some(ctx) = r.additional_context {
            contexts.push(ctx);
        }
    }
    if !reasons.is_empty() {
        out.reason = Some(reasons.join("\n"));
    }
    if !contexts.is_empty() {
        out.additional_context = Some(contexts.join("\n"));
    }
    out
}

/// matcher 是否命中：None/空=全部；否则按正则匹配，正则非法时退化为子串匹配。
fn matches(matcher: Option<&str>, key: Option<&str>) -> bool {
    let Some(m) = matcher.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    let Some(k) = key else {
        return true; // 无 key 的事件（SessionStart/Stop）不按 matcher 过滤
    };
    match regex::Regex::new(m) {
        Ok(re) => re.is_match(k),
        Err(_) => k.contains(m),
    }
}

/// 运行单个钩子命令，解析其决策。
fn run_one(command: &str, payload: &Value, timeout_ms: u64) -> HookOutcome {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(format!("chcp 65001>nul & {command}"));
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    };
    crate::proc::no_window(&mut cmd);
    let mut child = match cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            // 钩子起不来：当作拦截，把错误回灌（避免静默放过本想护栏的场景）。
            return HookOutcome {
                block: true,
                reason: Some(format!("hook 启动失败: {e}")),
                additional_context: None,
            };
        }
    };
    // 把事件 payload 从 stdin 喂给钩子。
    if let Some(mut sin) = child.stdin.take() {
        let _ = sin.write_all(payload.to_string().as_bytes());
        // drop sin → 关闭 stdin，钩子读到 EOF
    }
    let mut so = child.stdout.take();
    let mut se = child.stderr.take();
    let h_out = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(s) = so.as_mut() {
            let _ = s.read_to_end(&mut b);
        }
        b
    });
    let h_err = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(s) = se.as_mut() {
            let _ = s.read_to_end(&mut b);
        }
        b
    });

    let status = match child.wait_timeout(Duration::from_millis(timeout_ms)) {
        Ok(Some(s)) => s,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            return HookOutcome {
                block: true,
                reason: Some(format!("hook 超时（{}s）", timeout_ms / 1000)),
                additional_context: None,
            };
        }
        Err(e) => {
            return HookOutcome {
                block: true,
                reason: Some(format!("hook 等待失败: {e}")),
                additional_context: None,
            }
        }
    };
    let stdout = String::from_utf8_lossy(&h_out.join().unwrap_or_default()).to_string();
    let stderr = String::from_utf8_lossy(&h_err.join().unwrap_or_default()).to_string();
    let code = status.code().unwrap_or(-1);
    parse_outcome(code, &stdout, &stderr)
}

/// 解析钩子输出为决策：优先 JSON（decision/reason/additionalContext），否则按退出码。
fn parse_outcome(code: i32, stdout: &str, stderr: &str) -> HookOutcome {
    if let Ok(v) = serde_json::from_str::<Value>(stdout.trim()) {
        if v.is_object() {
            let decision = v.get("decision").and_then(Value::as_str);
            let block = decision == Some("block") || decision == Some("deny");
            let reason = v
                .get("reason")
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|s| !s.is_empty());
            // additionalContext 兼容顶层与 hookSpecificOutput.additionalContext。
            let ctx = v
                .get("additionalContext")
                .or_else(|| {
                    v.get("hookSpecificOutput")
                        .and_then(|h| h.get("additionalContext"))
                })
                .and_then(Value::as_str)
                .map(str::to_string)
                .filter(|s| !s.is_empty());
            return HookOutcome {
                block,
                reason: reason.or(if block {
                    Some("hook 拦截".to_string())
                } else {
                    None
                }),
                additional_context: ctx,
            };
        }
    }
    if code != 0 {
        let reason = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("hook 退出码 {code}")
        };
        HookOutcome {
            block: true,
            reason: Some(reason),
            additional_context: None,
        }
    } else {
        HookOutcome::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg(matcher: Option<&str>, command: &str) -> HookConfig {
        HookConfig {
            matcher: matcher.map(str::to_string),
            command: command.to_string(),
            timeout_ms: Some(10_000),
        }
    }

    #[test]
    fn matcher_matches() {
        assert!(matches(None, Some("shell")));
        assert!(matches(Some(""), Some("shell")));
        assert!(matches(Some("shell|write_file"), Some("write_file")));
        assert!(!matches(Some("^shell$"), Some("write_file")));
        assert!(matches(Some("foo"), None)); // 无 key 事件不过滤
    }

    #[test]
    fn parse_outcome_json_block_and_context() {
        let o = parse_outcome(0, r#"{"decision":"block","reason":"nope"}"#, "");
        assert!(o.block && o.reason.as_deref() == Some("nope"));
        let o2 = parse_outcome(0, r#"{"additionalContext":"hi"}"#, "");
        assert!(!o2.block && o2.additional_context.as_deref() == Some("hi"));
    }

    #[test]
    fn nonzero_exit_blocks_with_stderr() {
        let o = parse_outcome(2, "", "danger!");
        assert!(o.block && o.reason.as_deref() == Some("danger!"));
        let ok = parse_outcome(0, "whatever", "");
        assert!(!ok.block);
    }

    #[test]
    fn run_one_blocks_on_nonzero_exit() {
        // 退出码 2 → 拦截（cmd 与 sh 的 `exit 2` 写法一致）。
        let o = run_one("exit 2", &json!({}), 10_000);
        assert!(o.block);
    }

    #[test]
    fn run_hooks_aggregates_block_over_matching() {
        // 一个匹配 shell 且拦截的钩子 + 一个不匹配的。
        let hooks = vec![
            cfg(Some("write_file"), "exit 0"), // 不匹配 shell → 跳过
            cfg(Some("shell"), "exit 2"),      // 匹配 → 拦截
        ];
        let o = run_hooks(&hooks, Some("shell"), &json!({"tool_name":"shell"}));
        assert!(o.block);
        // 只跑不匹配的：不拦截。
        let o2 = run_hooks(&hooks, Some("read_file"), &json!({}));
        assert!(!o2.block);
    }
}
