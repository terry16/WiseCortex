//! 定时任务：直接设「间隔 + 任务内容」，带执行日志——不引入 cron 表达式，够用且好懂。
//!
//! 任务定义存 `wisecortex/cron/tasks.json`，每个任务的执行日志存 `wisecortex/cron/<id>.log`。
//! 本模块只管数据/日志（CLI 可直接用）；真正的调度循环在 server（需要 agent）。

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// 一个定时任务。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CronTask {
    pub id: String,
    pub name: String,
    /// 执行间隔（秒）。cron 表达式存在时此项被忽略。
    pub interval_secs: u64,
    /// Cron 表达式（5 或 6 字段，本地时区）。设置后优先于 interval_secs。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,
    /// 任务内容（作为 prompt 交给 agent 执行）。
    pub prompt: String,
    /// 执行后把结果推送到的通道名（可选）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// 该任务的工作目录（可选）。设置后此任务在该目录下隔离执行：工具 cwd 指向它，
    /// 并加载其 `<workdir>/skills` 项目级技能。留空=用全局工作目录（默认）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
    /// 该任务使用的 LLM 档 id（对应设置里配置的模型档）。留空=用全局默认模型。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 上次执行的 epoch 秒。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    /// 累计执行次数。
    #[serde(default)]
    pub runs: u64,
    /// 上次执行耗时（毫秒）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_duration_ms: Option<u64>,
}

impl CronTask {
    /// 是否到点该执行。cron 表达式优先；否则按固定间隔。
    pub fn is_due(&self, now: u64) -> bool {
        match &self.cron {
            Some(expr) => cron_is_due(expr, self.last_run, now),
            None => self
                .last_run
                .map(|lr| now.saturating_sub(lr) >= self.interval_secs)
                .unwrap_or(true),
        }
    }
}

/// 把 5 字段 cron 补成 cron crate 需要的 6 字段（前置秒=0）。
fn normalize_cron(expr: &str) -> String {
    let n = expr.split_whitespace().count();
    if n == 5 {
        format!("0 {}", expr.trim())
    } else {
        expr.trim().to_string()
    }
}

/// 校验 cron 表达式是否可解析（5 或 6 字段）。
pub fn cron_valid(expr: &str) -> bool {
    use std::str::FromStr;
    ::cron::Schedule::from_str(&normalize_cron(expr)).is_ok()
}

/// 依据 cron 表达式判断「自 last_run 之后的下一次触发时间是否已到」。
pub fn cron_is_due(expr: &str, last_run: Option<u64>, now: u64) -> bool {
    use chrono::{Local, TimeZone};
    use std::str::FromStr;
    let Ok(sched) = ::cron::Schedule::from_str(&normalize_cron(expr)) else {
        return false;
    };
    let after = last_run.unwrap_or(0).min(now.saturating_sub(1)) as i64;
    let Some(after_dt) = Local.timestamp_opt(after, 0).single() else {
        return false;
    };
    match sched.after(&after_dt).next() {
        Some(next) => now as i64 >= next.timestamp(),
        None => false,
    }
}

fn default_true() -> bool {
    true
}

pub fn cron_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("cron"))
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("task-{nanos}")
}

pub fn load_tasks() -> Vec<CronTask> {
    cron_dir()
        .map(|d| d.join("tasks.json"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_tasks(tasks: &[CronTask]) -> std::io::Result<()> {
    let dir = cron_dir()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "无配置目录"))?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("tasks.json"), serde_json::to_string_pretty(tasks)?)
}

/// 追加一行执行日志。
pub fn append_log(id: &str, line: &str) {
    let Some(dir) = cron_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{id}.log")))
    {
        let _ = writeln!(f, "[{}] {line}", now_secs());
    }
}

pub fn read_log(id: &str) -> String {
    cron_dir()
        .map(|d| d.join(format!("{id}.log")))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default()
}

/// 一行是否为「某次运行的结束行」——即 `record_run` 写的那条汇总行 `status=… | …`。
///
/// 重试行（`status=llm_error（第 1/3 遍）→ 重试`）也以 `status=` 开头但**不含** `|`，
/// 因此不算结束；否则一次带重试的运行会被误切成好几次。这与前端统计完成次数用的
/// 正则（`tasksView.ts` 里的 `\]\s*status=[^\n]*\|`）是同一套约定。
fn is_run_end(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('[') else {
        return false;
    };
    let Some(end) = rest.find(']') else {
        return false;
    };
    if rest[..end].parse::<u64>().is_err() {
        return false;
    }
    let body = rest[end + 1..].trim_start();
    body.starts_with("status=") && body.contains('|')
}

/// 把日志按「整次运行」从最旧开始裁掉，直到总体积不超过 `max_bytes`。
/// 返回 `Some(新内容)` 表示需要重写，`None` 表示不必动盘。
///
/// 保底规则：**最近一次完整运行永远保留**，哪怕它自己就超限——宁可超一点，
/// 也不能让用户点开日志看到空白（那正是按天清理留下的老毛病）。
/// 找不到运行边界（日志里一条汇总行都没有，例如首次运行还在进行中）时同样不动。
fn trim_to_size(content: &str, max_bytes: u64) -> Option<String> {
    if content.len() as u64 <= max_bytes {
        return None;
    }
    let lines: Vec<&str> = content.lines().collect();
    let bounds: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| is_run_end(l))
        .map(|(i, _)| i)
        .collect();
    // 少于两次完整运行时无处可裁（裁了就把「最近一次」也裁没了）。
    if bounds.len() < 2 {
        return None;
    }
    // 各行在重写后占的字节数（含换行）的后缀和，避免每次试裁都重新拼字符串。
    let mut suffix = vec![0u64; lines.len() + 1];
    for i in (0..lines.len()).rev() {
        suffix[i] = suffix[i + 1] + lines[i].len() as u64 + 1;
    }
    let rebuild = |from: usize| format!("{}\n", lines[from..].join("\n"));

    // 从最旧的运行开始丢，丢到不超限即停——尽量少删。
    for &b in &bounds[..bounds.len() - 1] {
        if suffix[b + 1] <= max_bytes {
            return Some(rebuild(b + 1));
        }
    }
    // 全丢完仍超限：说明最后一次运行自己就很大，裁到只剩它。
    Some(rebuild(bounds[bounds.len() - 2] + 1))
}

/// 按体积清理定时任务执行日志：单个 `<id>.log` 超过 `max_bytes` 时，从最旧的
/// 「整次运行」开始裁剪至不超限。`max_bytes == 0` 表示不清理（用户关掉了自动清理）。
///
/// 只改写 `*.log`，**绝不删除整个日志文件**，也绝不动 `tasks.json`。返回被改写的文件数。
/// 体积没超的日志一律原样不动——所以低频任务（周/月一次）的小日志永远不会被清掉。
pub fn prune_logs(max_bytes: u64) -> usize {
    let Some(dir) = cron_dir() else {
        return 0;
    };
    prune_logs_in(&dir, max_bytes)
}

/// [`prune_logs`] 的实现体，目录可指定（便于测试）。
fn prune_logs_in(dir: &std::path::Path, max_bytes: u64) -> usize {
    if max_bytes == 0 {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut affected = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("log") {
            continue;
        }
        // 先看文件大小，绝大多数日志远小于上限，可免去整份读盘。
        let oversized = entry
            .metadata()
            .map(|m| m.len() > max_bytes)
            .unwrap_or(false);
        if !oversized {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(trimmed) = trim_to_size(&content, max_bytes) {
            if std::fs::write(&path, trimmed).is_ok() {
                affected += 1;
            }
        }
    }
    affected
}

/// 解析间隔：纯数字=秒，或带后缀 s/m/h/d。
pub fn parse_duration(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, mult) = match s.chars().last() {
        Some('s') => (&s[..s.len() - 1], 1),
        Some('m') => (&s[..s.len() - 1], 60),
        Some('h') => (&s[..s.len() - 1], 3600),
        Some('d') => (&s[..s.len() - 1], 86400),
        _ => (s, 1),
    };
    num.trim().parse::<u64>().ok().map(|v| v * mult)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("30"), Some(30));
        assert_eq!(parse_duration("5m"), Some(300));
        assert_eq!(parse_duration("2h"), Some(7200));
        assert_eq!(parse_duration("1d"), Some(86400));
        assert_eq!(parse_duration("x"), None);
    }

    #[test]
    fn task_serde_roundtrip() {
        let t = CronTask {
            id: "t1".into(),
            name: "ping".into(),
            interval_secs: 3600,
            cron: None,
            prompt: "检查服务".into(),
            channel: Some("飞书".into()),
            workdir: Some("/srv/mud".into()),
            model: Some("llm-1".into()),
            enabled: true,
            last_run: None,
            last_status: None,
            runs: 0,
            last_duration_ms: None,
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: CronTask = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
        // enabled 缺省应为 true，runs 缺省 0，workdir 缺省 None
        let minimal: CronTask =
            serde_json::from_str(r#"{"id":"a","name":"b","interval_secs":10,"prompt":"p"}"#)
                .unwrap();
        assert!(minimal.enabled);
        assert_eq!(minimal.runs, 0);
        assert_eq!(minimal.workdir, None);
    }

    #[test]
    fn cron_expression_due_logic() {
        // 5 字段（每分钟）应能解析并判定到点。
        assert!(cron_valid("* * * * *"));
        // 6 字段（每天 8:30:00）。
        assert!(cron_valid("0 30 8 * * *"));
        assert!(!cron_valid("不是 cron"));

        // 每分钟：last_run 在 120 秒前，now 必然已过下一个整分。
        let now = now_secs();
        assert!(cron_is_due("* * * * *", Some(now - 120), now));
        // 刚跑过（同一秒），下一次还没到。
        assert!(!cron_is_due("* * * * *", Some(now), now));
    }

    /// 造一次运行：一条步骤行（用 pad 撑体积）+ 一条汇总行。
    fn run_block(ts: u64, pad: usize) -> String {
        format!(
            "[{ts}] · 第1轮 思考{}\n[{ts}] status=ok | 3轮 ↑10 ↓5 | 完成\n",
            "x".repeat(pad)
        )
    }

    #[test]
    fn run_end_detection_ignores_retry_lines() {
        // record_run 的汇总行 = 一次运行的结束。
        assert!(is_run_end("[1700000000] status=ok | 3轮 ↑10 ↓5 | 完成"));
        assert!(is_run_end("[42] status=max_iterations | "));
        // 重试行同样以 status= 开头但没有 `|`——不能算结束，
        // 否则一次带重试的运行会被误切成多次。
        assert!(!is_run_end("[42] status=llm_error（第 1/3 遍）→ 重试"));
        // 步骤行 / 无时间戳 / 坏时间戳。
        assert!(!is_run_end("[42] · 第1轮 思考"));
        assert!(!is_run_end("status=ok | 没有时间戳"));
        assert!(!is_run_end("[abc] status=ok | 坏时间戳"));
    }

    #[test]
    fn trim_leaves_small_logs_alone() {
        // 没超限就绝不动盘——低频任务的小日志因此永远不会被清掉。
        let log = format!("{}{}", run_block(1, 10), run_block(2, 10));
        assert_eq!(trim_to_size(&log, 1_000_000), None);
    }

    #[test]
    fn trim_drops_oldest_whole_runs_and_keeps_the_newest() {
        let log = format!(
            "{}{}{}",
            run_block(1, 100),
            run_block(2, 100),
            run_block(3, 100)
        );
        let max = (log.len() - 10) as u64; // 略微超限：裁掉最旧一次即可
        let out = trim_to_size(&log, max).expect("超限应重写");
        assert!(!out.contains("[1]"), "最旧一次应被整块裁掉");
        assert!(out.contains("[2]") && out.contains("[3]"));
        assert!(out.len() as u64 <= max);
        // 切口必须落在运行边界上，不能留下上一次运行的半截步骤行。
        assert!(out.starts_with("[2] · 第1轮"));
    }

    #[test]
    fn trim_keeps_the_last_run_even_if_it_alone_exceeds() {
        // 保底：宁可超限，也不能让用户点开日志看到空白。
        let log = format!("{}{}", run_block(1, 50), run_block(2, 5000));
        let out = trim_to_size(&log, 100).expect("超限应重写");
        assert!(!out.contains("[1]"));
        assert!(out.contains("[2] status=ok"));
        assert!(out.len() > 100);
    }

    /// 文件层面的回归测试：低频任务的小日志必须原样存活，任何日志都不许被整个删掉——
    /// 这正是老的「按天清理」踩的坑（整文件过期就 remove_file，于是 UI 里
    /// 「执行过 3 次」却点不出日志）。
    #[test]
    fn prune_never_deletes_a_file_and_leaves_small_logs_untouched() {
        let dir = std::env::temp_dir().join(format!("wc-cron-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 低频任务：只跑过两次、日志很小，且时间戳非常久远（换成老实现会被整个删掉）。
        let small = dir.join("task-rare.log");
        std::fs::write(&small, format!("{}{}", run_block(1, 10), run_block(2, 10))).unwrap();
        // 高频任务：日志很大，应被裁到上限以内。
        let big = dir.join("task-busy.log");
        let mut huge = String::new();
        for i in 0..20 {
            huge.push_str(&run_block(i, 2000));
        }
        std::fs::write(&big, &huge).unwrap();
        // 非日志文件绝不能被碰（tasks.json 是任务定义本体）。
        let tasks = dir.join("tasks.json");
        std::fs::write(&tasks, "[]").unwrap();

        let max = 8_000u64;
        assert_eq!(prune_logs_in(&dir, max), 1, "只有超限的那个应被改写");

        assert!(small.exists(), "小日志文件不许被删除");
        assert_eq!(
            std::fs::read_to_string(&small).unwrap(),
            format!("{}{}", run_block(1, 10), run_block(2, 10)),
            "没超限的日志必须一字不动"
        );
        let after = std::fs::read_to_string(&big).unwrap();
        assert!(
            big.exists() && !after.is_empty(),
            "大日志只该被裁剪，不该被清空"
        );
        assert!(after.len() as u64 <= max);
        assert!(after.contains("[19] status=ok"), "最近一次运行必须留着");
        assert!(!after.contains("[0] "), "最旧的运行应被裁掉");
        assert_eq!(
            std::fs::read_to_string(&tasks).unwrap(),
            "[]",
            "不许动 tasks.json"
        );

        // 关掉自动清理（上限 0）时彻底不作为。
        std::fs::write(&big, &huge).unwrap();
        assert_eq!(prune_logs_in(&dir, 0), 0);
        assert_eq!(std::fs::read_to_string(&big).unwrap().len(), huge.len());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trim_never_empties_a_log_with_a_single_run() {
        // 只有一次运行时无处可裁——这正是老实现会 remove_file 的场景。
        let log = run_block(1, 10_000);
        assert_eq!(trim_to_size(&log, 100), None);
        // 一次汇总行都没有（首次运行进行中）时同样不动。
        assert_eq!(trim_to_size("[1] · 第1轮 思考\n", 5), None);
    }
}
