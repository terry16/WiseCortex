//! 把 `ServerEvent` 渲染成终端行（纯函数，便于单测）。
//!
//! 只负责「定稿转录」类事件 → 文本行 + 样式；流式增量（`assistant_delta`）、思考流、进度
//! spinner、确认/反馈提示这些由主循环特殊处理，本函数对它们返回空。ANSI 上色在 [`paint`]，
//! 测试只校验文本内容，不掺 ANSI 噪声。

use serde_json::Value;
use wisecortex_core::proto::ServerEvent;

/// 行样式（映射到 ANSI 颜色）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Normal 目前仅测试用到；保留作默认样式。
pub enum Style {
    Normal,
    Dim,
    Tool,
    Info,
    Warn,
    Success,
    Error,
    Cost,
}

/// 一行渲染结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub style: Style,
}

impl Line {
    fn new(style: Style, text: impl Into<String>) -> Self {
        Line {
            style,
            text: text.into(),
        }
    }
}

const RESULT_MAX: usize = 200;
const STDOUT_MAX_LINES: usize = 8;

/// 单行截断（按字符，超出加省略号）。
fn one_line(s: &str, max: usize) -> String {
    let flat = s.replace('\n', " ").replace('\r', "");
    let flat = flat.trim();
    let n = flat.chars().count();
    if n <= max {
        flat.to_string()
    } else {
        let head: String = flat.chars().take(max).collect();
        format!("{head}… (+{} 字)", n - max)
    }
}

/// 把 JSON 值压成一行简短字符串（字符串取原文，其余取紧凑 JSON）。
fn condense(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// 把「定稿转录」类事件渲染成若干行；其它事件返回空 vec（由主循环另行处理）。
pub fn render(ev: &ServerEvent) -> Vec<Line> {
    match ev {
        ServerEvent::ToolCall {
            name,
            args,
            summary,
            ..
        } => {
            let detail = summary
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| one_line(&condense(args), 80));
            let text = if detail.is_empty() {
                format!("  ▸ {name}")
            } else {
                format!("  ▸ {name} {detail}")
            };
            vec![Line::new(Style::Tool, text)]
        }
        ServerEvent::ToolResult { result, .. } => {
            let s = one_line(&condense(result), RESULT_MAX);
            if s.is_empty() {
                vec![]
            } else {
                vec![Line::new(Style::Dim, format!("  → {s}"))]
            }
        }
        ServerEvent::ToolStdout { lines, .. } => {
            let mut out: Vec<Line> = lines
                .iter()
                .take(STDOUT_MAX_LINES)
                .map(|l| Line::new(Style::Dim, format!("  │ {}", one_line(l, RESULT_MAX))))
                .collect();
            if lines.len() > STDOUT_MAX_LINES {
                out.push(Line::new(
                    Style::Dim,
                    format!("  │ …（另有 {} 行）", lines.len() - STDOUT_MAX_LINES),
                ));
            }
            out
        }
        ServerEvent::ToolError { error, .. } => {
            vec![Line::new(
                Style::Error,
                format!("  ✗ {}", one_line(error, RESULT_MAX)),
            )]
        }
        ServerEvent::Complete {
            iterations,
            cost,
            duration,
            ..
        } => {
            let dur = duration.map(|d| format!(" · {d:.1}s")).unwrap_or_default();
            vec![Line::new(
                Style::Cost,
                format!("  [¥{cost:.4} · {iterations} 轮{dur}]"),
            )]
        }
        ServerEvent::Interrupted { .. } => vec![Line::new(Style::Warn, "  (已打断)")],
        ServerEvent::MessageQueued { .. } => {
            vec![Line::new(Style::Dim, "  (已排队，本回合结束后处理)")]
        }
        ServerEvent::Info { message, .. } => vec![Line::new(Style::Info, message.clone())],
        ServerEvent::Success { message, .. } => vec![Line::new(Style::Success, message.clone())],
        ServerEvent::Warning { message, .. } => {
            vec![Line::new(Style::Warn, format!("⚠ {message}"))]
        }
        ServerEvent::Error { message, .. } => vec![Line::new(Style::Error, format!("✗ {message}"))],
        // 其余（delta / thinking / progress / 确认反馈 / 会话生命周期 / pong…）主循环处理。
        _ => vec![],
    }
}

/// 给一行文本套 ANSI 颜色（终端用；non-tty 可直接用 `line.text`）。
pub fn paint(line: &Line) -> String {
    let code = match line.style {
        Style::Normal => "",
        Style::Dim => "\x1b[2m",
        Style::Tool => "\x1b[36m",    // cyan
        Style::Info => "\x1b[34m",    // blue
        Style::Warn => "\x1b[33m",    // yellow
        Style::Success => "\x1b[32m", // green
        Style::Error => "\x1b[31m",   // red
        Style::Cost => "\x1b[90m",    // bright-black
    };
    if code.is_empty() {
        line.text.clone()
    } else {
        format!("{code}{}\x1b[0m", line.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev_tool_call(name: &str, args: Value, summary: Option<&str>) -> ServerEvent {
        ServerEvent::ToolCall {
            session_id: "s".into(),
            name: name.into(),
            args,
            summary: summary.map(String::from),
        }
    }

    #[test]
    fn tool_call_prefers_summary() {
        let lines = render(&ev_tool_call(
            "read_file",
            json!({"path": "a.rs"}),
            Some("读 a.rs"),
        ));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "  ▸ read_file 读 a.rs");
        assert_eq!(lines[0].style, Style::Tool);
    }

    #[test]
    fn tool_call_falls_back_to_args() {
        let lines = render(&ev_tool_call("shell", json!({"cmd": "ls"}), None));
        assert_eq!(lines[0].text, r#"  ▸ shell {"cmd":"ls"}"#);
    }

    #[test]
    fn tool_result_truncates_long_output() {
        let big = "x".repeat(500);
        let lines = render(&ServerEvent::ToolResult {
            session_id: "s".into(),
            result: json!(big),
        });
        assert_eq!(lines.len(), 1);
        assert!(lines[0].text.starts_with("  → "));
        assert!(
            lines[0].text.contains("… (+300 字)"),
            "应截断并标注剩余: {}",
            lines[0].text
        );
    }

    #[test]
    fn empty_tool_result_produces_no_line() {
        let lines = render(&ServerEvent::ToolResult {
            session_id: "s".into(),
            result: json!(""),
        });
        assert!(lines.is_empty());
    }

    #[test]
    fn tool_stdout_caps_lines_and_notes_remainder() {
        let many: Vec<String> = (0..20).map(|i| format!("line{i}")).collect();
        let lines = render(&ServerEvent::ToolStdout {
            session_id: "s".into(),
            lines: many,
        });
        // 8 行内容 + 1 行「另有 12 行」。
        assert_eq!(lines.len(), STDOUT_MAX_LINES + 1);
        assert!(lines.last().unwrap().text.contains("另有 12 行"));
    }

    #[test]
    fn complete_formats_cost_iterations_duration() {
        let lines = render(&ServerEvent::Complete {
            session_id: "s".into(),
            iterations: 3,
            cost: 0.1234,
            duration: Some(12.34),
            cache_stats: None,
            awaiting_user_feedback: None,
            cost_source: None,
        });
        assert_eq!(lines[0].text, "  [¥0.1234 · 3 轮 · 12.3s]");
        assert_eq!(lines[0].style, Style::Cost);
    }

    #[test]
    fn tool_error_is_red() {
        let lines = render(&ServerEvent::ToolError {
            session_id: "s".into(),
            error: "boom".into(),
        });
        assert_eq!(lines[0].style, Style::Error);
        assert!(lines[0].text.contains("✗ boom"));
    }

    #[test]
    fn streaming_and_lifecycle_events_render_nothing() {
        for ev in [
            ServerEvent::AssistantDelta {
                session_id: "s".into(),
                delta: "hi".into(),
            },
            ServerEvent::Pong,
            ServerEvent::Subscribed {
                session_id: "s".into(),
            },
        ] {
            assert!(render(&ev).is_empty(), "{ev:?} 不应产出转录行");
        }
    }

    #[test]
    fn paint_wraps_with_ansi_only_when_styled() {
        let normal = Line::new(Style::Normal, "hi");
        assert_eq!(paint(&normal), "hi");
        let err = Line::new(Style::Error, "boom");
        assert!(paint(&err).starts_with("\x1b[31m") && paint(&err).ends_with("\x1b[0m"));
    }
}
