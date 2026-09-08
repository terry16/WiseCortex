//! 错误日志：panic 与关键运行错误落盘到 `wisecortex/logs/error.log`，
//! 供自修复（self-repair）skill 读取定位问题。
//!
//! 设计：追加写、带 epoch 秒时间戳与 scope 标签；超过上限自动滚动（保留后半），
//! 防无限增长。日志写入失败一律静默——日志不该影响主流程。

use std::path::{Path, PathBuf};

/// 单文件大小上限；超过则保留后半截。
const MAX_BYTES: u64 = 512 * 1024;

/// 错误日志路径：`<data_dir>/wisecortex/logs/error.log`。
pub fn log_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("logs").join("error.log"))
}

/// 记录一条错误（带时间戳与 scope）。失败静默。
pub fn record(scope: &str, msg: &str) {
    if let Some(p) = log_path() {
        let _ = append_capped(&p, scope, msg);
    }
}

/// 读取最近 `max_bytes` 字节的日志（供 CLI / Agent 查看）。无日志返回空串。
pub fn read_recent(max_bytes: usize) -> String {
    match log_path() {
        Some(p) => read_recent_from(&p, max_bytes),
        None => String::new(),
    }
}

/// 安装全局 panic hook：先落盘再走原有 hook（保留默认打印/abort 行为）。
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        record("panic", &info.to_string());
        prev(info);
    }));
}

fn append_capped(path: &Path, scope: &str, msg: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 过大则滚动：保留后半，并从下一个换行开始避免半行。
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > MAX_BYTES {
            if let Ok(content) = std::fs::read_to_string(path) {
                let half = &content[content.len() / 2..];
                let kept = half.find('\n').map(|i| &half[i + 1..]).unwrap_or("");
                let _ = std::fs::write(path, kept);
            }
        }
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 单条压成一行，多行内容用 ⏎ 占位，便于 grep。
    let line = format!("[{ts}] [{scope}] {}\n", msg.replace('\n', " ⏎ "));
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    f.write_all(line.as_bytes())
}

fn read_recent_from(path: &Path, max_bytes: usize) -> String {
    let Ok(content) = std::fs::read_to_string(path) else {
        return String::new();
    };
    if content.len() <= max_bytes {
        return content;
    }
    let tail = &content[content.len() - max_bytes..];
    // 从下一个换行起，避免截到半行。
    tail.find('\n')
        .map(|i| tail[i + 1..].to_string())
        .unwrap_or_else(|| tail.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_then_read_recent_roundtrip() {
        let dir = std::env::temp_dir().join(format!("wc-buglog-{}-rt", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let path = dir.join("error.log");

        append_capped(&path, "test", "first error").unwrap();
        append_capped(&path, "cron", "second\nmultiline").unwrap();

        let recent = read_recent_from(&path, 10_000);
        assert!(recent.contains("first error"));
        assert!(recent.contains("[cron]"));
        // 多行被压成一行（换行替换为 ⏎）。
        assert!(recent.contains("second ⏎ multiline"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rotates_when_oversized() {
        let dir = std::env::temp_dir().join(format!("wc-buglog-{}-rot", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("error.log");

        // 预置一个超限文件（>512KB，含多处换行）。
        let big = format!("{}\n", "x".repeat(600 * 1024));
        std::fs::write(&path, &big).unwrap();

        append_capped(&path, "t", "new line after rotate").unwrap();

        let len = std::fs::metadata(&path).unwrap().len();
        assert!(len < 600 * 1024, "超限后应滚动收缩，实际 {len}");
        assert!(read_recent_from(&path, 10_000).contains("new line after rotate"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_recent_missing_file_is_empty() {
        let p =
            std::env::temp_dir().join(format!("wc-buglog-{}-none/error.log", std::process::id()));
        assert_eq!(read_recent_from(&p, 100), "");
    }
}
