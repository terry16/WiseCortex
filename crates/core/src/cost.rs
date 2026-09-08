//! 当日成本账本：服务端按**本地日期**累计成本，交互对话与定时任务都计入。
//! 持久化到 `<data_dir>/wisecortex/cost/daily.json`（`{ "YYYY-MM-DD": 金额 }`），
//! 进程重启不丢、且**不依赖浏览器在线**——无人值守的定时任务也照样计入当日总额。
//! 前端连上时读一次初值，之后每次变动由服务端广播 `cost_update`。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Local;

/// 只保留最近这么多天，避免账本文件无限增长。
const KEEP_DAYS: usize = 60;

/// 串行化「读→改→写」，避免交互与定时任务并发时丢更新。
static LOCK: Mutex<()> = Mutex::new(());

fn cost_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("cost").join("daily.json"))
}

/// 本地日期 `YYYY-MM-DD`。用本地时区——服务端所在地即用户的「今天」。
pub fn today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn load_map() -> BTreeMap<String, f64> {
    cost_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_map(m: &BTreeMap<String, f64>) {
    let Some(p) = cost_path() else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // BTreeMap 按日期升序；保留最近 KEEP_DAYS 天。
    let pruned: BTreeMap<String, f64> = m
        .iter()
        .rev()
        .take(KEEP_DAYS)
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    if let Ok(s) = serde_json::to_string_pretty(&pruned) {
        let _ = std::fs::write(p, s);
    }
}

/// 给当日累加成本，返回累加后的当日总额。`amount <= 0` 时不改、只回读当前总额。
pub fn add(amount: f64) -> f64 {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let day = today();
    let mut m = load_map();
    let entry = m.entry(day).or_insert(0.0);
    if amount > 0.0 {
        *entry += amount;
    }
    let total = *entry;
    save_map(&m);
    total
}

/// 读当日总额（不修改）。
pub fn today_total() -> f64 {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    *load_map().get(&today()).unwrap_or(&0.0)
}
