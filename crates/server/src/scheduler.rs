//! 定时任务调度器：周期性检查 cron 任务，到点就跑 agent、写日志、推通知。

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use wisecortex_core::{buglog, config, cost, cron, notify};

use crate::agent::{Agent, RunOutcome};
use crate::proto::ServerEvent;
use crate::registry::SessionRegistry;

/// 调度器检查间隔（秒）。任务间隔应 >= 此值。
const TICK_SECS: u64 = 30;

/// 整任务最多尝试次数（首跑 + 重试）。即最多重试 2 次、共 3 遍。
const MAX_TASK_ATTEMPTS: u32 = 3;

/// 是否对整任务再跑一次：仅「LLM 调用失败」值得重试——
/// 它常是上游临时故障 / 模型偶发坏响应，重跑会重新生成对话，多半能绕开。
/// 其余失败重试无解：目录不存在、通道找不到/推送失败（任务其实已跑完）、到回合上限（多为确定性）。
fn should_retry(status: &str, attempt: u32) -> bool {
    status == "llm_error" && attempt < MAX_TASK_ATTEMPTS
}

/// 日志清理的检查周期：每小时跑一次（清理本身按配置的保留天数删超期行，幂等且廉价）。
const PRUNE_EVERY: Duration = Duration::from_secs(3600);

/// 正在执行中的任务 id 集合——防止同一任务被并发跑两遍：
/// 任务执行可达数分钟，期间 `last_run` 尚未落盘，若此时「立即运行」与调度器某一拍
/// （或两拍调度）同时判定其 due，就会重复执行（实测：点 3 次却生成 6 个武功）。
fn running_set() -> &'static Mutex<HashSet<String>> {
    static S: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashSet::new()))
}

/// 占用一个任务的执行槽；返回 `None` 表示它已在运行（调用方应跳过）。
/// 返回的 [`RunSlot`] 在 drop 时自动释放（含 await 出错/提前返回也安全）。
fn acquire_run_slot(id: &str) -> Option<RunSlot> {
    let mut set = running_set().lock().unwrap();
    if set.contains(id) {
        return None;
    }
    set.insert(id.to_string());
    Some(RunSlot(id.to_string()))
}

struct RunSlot(String);
impl Drop for RunSlot {
    fn drop(&mut self) {
        running_set().lock().unwrap().remove(&self.0);
    }
}

/// 启动后台调度循环。`reg` 用于把定时任务的成本广播给前端（计入「今日成本」）。
pub fn spawn(agent: Arc<Mutex<Arc<Agent>>>, reg: SessionRegistry) {
    tokio::spawn(async move {
        // 启动即清一次（每次读 config，改了保留天数即时生效，无需重启）。
        prune_logs_now();
        let mut last_prune = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;
            run_due(&agent, &reg).await;
            if last_prune.elapsed() >= PRUNE_EVERY {
                prune_logs_now();
                last_prune = Instant::now();
            }
        }
    });
}

/// 按当前配置清理定时任务日志：只裁超过体积上限的，按「整次运行」从最旧丢起。
/// 关掉自动清理时上限为 0，直接跳过。日志没超限就完全不动——低频任务不会被清空。
fn prune_logs_now() {
    let max_bytes = config::load().cron_log_max_bytes_effective();
    if max_bytes == 0 {
        return;
    }
    let n = cron::prune_logs(max_bytes);
    if n > 0 {
        buglog::record(
            "cron",
            &format!(
                "日志清理：上限 {}MB，裁剪了 {n} 个超限日志文件",
                max_bytes / 1024 / 1024
            ),
        );
    }
}

async fn run_due(agent: &Arc<Mutex<Arc<Agent>>>, reg: &SessionRegistry) {
    let mut tasks = cron::load_tasks();
    let now = cron::now_secs();
    let mut changed = false;

    for t in tasks.iter_mut() {
        if !t.enabled || !t.is_due(now) {
            continue;
        }
        // 已在运行（被「立即运行」触发，或上一拍尚未跑完）→ 本拍跳过，避免同任务并发重复执行。
        let Some(_slot) = acquire_run_slot(&t.id) else {
            continue;
        };
        let (status, result, elapsed_ms, outcome) = execute_with_retry(agent, t).await;
        record_run(t, now, &status, &result, elapsed_ms, outcome.as_ref(), reg);
        changed = true;
    }

    if changed {
        let _ = cron::save_tasks(&tasks);
    }
}

/// 立即运行一个任务（无视 enabled / 调度时间），更新其状态与日志并持久化，返回结果文本。
/// 供「立即运行」按钮（REST `/api/cron/{id}/run`）调用。
pub async fn run_task_now(
    agent: &Arc<Mutex<Arc<Agent>>>,
    reg: &SessionRegistry,
    id: &str,
) -> Result<String, String> {
    let mut tasks = cron::load_tasks();
    let Some(idx) = tasks.iter().position(|t| t.id == id) else {
        return Err("任务不存在".into());
    };
    let now = cron::now_secs();
    // 已在运行（调度器某拍或另一次手动触发）→ 不并发重复跑。
    let Some(_slot) = acquire_run_slot(id) else {
        return Err("任务正在运行中，请稍候".into());
    };
    let (status, result, elapsed_ms, outcome) = execute_with_retry(agent, &tasks[idx]).await;
    record_run(
        &mut tasks[idx],
        now,
        &status,
        &result,
        elapsed_ms,
        outcome.as_ref(),
        reg,
    );
    let _ = cron::save_tasks(&tasks);
    Ok(result)
}

/// 执行任务并按需整任务重试（仅 LLM 调用失败，见 [`should_retry`]）：最多 [`MAX_TASK_ATTEMPTS`] 遍。
/// 重试在 [`execute`] 之上——它会重新跑整个 agent 对话，从而绕开偶发的上游/模型坏响应。
async fn execute_with_retry(
    agent: &Arc<Mutex<Arc<Agent>>>,
    t: &cron::CronTask,
) -> (String, String, u64, Option<RunOutcome>) {
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let r = execute(agent, t).await;
        if !should_retry(&r.0, attempt) {
            return r;
        }
        // 失败可见化：错误日志 + 任务执行日志各记一行（执行日志会显示在 UI 日志抽屉里）。
        buglog::record(
            "cron",
            &format!(
                "task={} llm_error，第 {attempt}/{MAX_TASK_ATTEMPTS} 遍失败，重试…",
                t.name
            ),
        );
        cron::append_log(
            &t.id,
            &format!("status=llm_error（第 {attempt}/{MAX_TASK_ATTEMPTS} 遍）→ 重试"),
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// 执行一次任务：跑 agent（指定 workdir 则隔离）→ 推通道 →
/// 返回 (status, 结果文本, 耗时 ms, 用量统计)。不写任务状态/日志（交给 [`record_run`]）。
async fn execute(
    agent: &Arc<Mutex<Arc<Agent>>>,
    t: &cron::CronTask,
) -> (String, String, u64, Option<RunOutcome>) {
    // 取当前 agent（不跨 await 持锁）。
    let ag = agent.lock().unwrap().clone();
    let started = std::time::Instant::now();

    // 任务指定了工作目录则隔离执行；目录不存在直接判错，避免跑错地方造成副作用。
    let workdir = t.workdir.as_deref().filter(|s| !s.is_empty());
    let bad_workdir = workdir.filter(|d| !std::path::Path::new(d).is_dir());
    let (result, outcome) = match bad_workdir {
        Some(dir) => (format!("工作目录不存在：{dir}"), None),
        None => {
            let o = ag
                .run_once_in(
                    &t.prompt,
                    workdir.map(std::path::Path::new),
                    Some(&t.id),
                    t.model.as_deref(),
                )
                .await;
            (o.text.clone(), Some(o))
        }
    };
    let elapsed_ms = started.elapsed().as_millis() as u64;

    // 基础状态取 agent 运行的真实终止原因（ok / max_iterations / llm_error），
    // 而非一律 ok——否则到回合上限/模型报错也会被算成功，成功率虚高。
    let mut status = if bad_workdir.is_some() {
        String::from("workdir_not_found")
    } else {
        outcome
            .as_ref()
            .map(|o| o.status.to_string())
            .unwrap_or_else(|| "ok".to_string())
    };
    if let Some(chname) = t.channel.clone() {
        // resolve：命名通道（channels.json）或合成规格（feishu_app:/qqbot:）。
        match notify::resolve(&chname) {
            Some(ch) => {
                let msg = format!("[{}]\n{}", t.name, result);
                let r = tokio::task::spawn_blocking(move || notify::send(&ch, &msg))
                    .await
                    .unwrap_or_else(|e| Err(e.to_string()));
                if let Err(e) = r {
                    status = format!("notify_err: {e}");
                }
            }
            None => status = format!("channel_not_found: {chname}"),
        }
    }
    (status, result, elapsed_ms, outcome)
}

/// 把一次运行的结果写入任务字段与执行日志（就地更新 `t`）。
fn record_run(
    t: &mut cron::CronTask,
    now: u64,
    status: &str,
    result: &str,
    elapsed_ms: u64,
    outcome: Option<&RunOutcome>,
    reg: &SessionRegistry,
) {
    // 计入服务端当日成本账本（仅有定价的真实成本，与交互/前端口径一致），并广播新总额，
    // 让「今日成本」即便在无人值守时也把定时任务的花费算进去。
    if let Some(o) = outcome {
        if o.priced && o.cost > 0.0 {
            let day_total = cost::add(o.cost);
            reg.publish_global(ServerEvent::CostUpdate {
                cost_today: day_total,
            });
        }
    }
    // 用量/成本摘要（与聊天 Complete 同口径）：N 轮 ↑输入 ↓输出 [缓存] [· ¥成本]。
    let stat = match outcome {
        Some(o) if o.requests > 0 => {
            let u = &o.usage;
            let cache = if u.cache_read_input_tokens > 0 {
                format!(" 缓存{}", u.cache_read_input_tokens)
            } else {
                String::new()
            };
            let cost = if o.priced {
                format!(" · ¥{:.4}", o.cost)
            } else {
                String::new()
            };
            format!(
                " | {}轮 ↑{} ↓{}{}{}",
                o.requests, u.prompt_tokens, u.completion_tokens, cache, cost
            )
        }
        _ => String::new(),
    };
    // 日志是「一行一条」，把结果换行压成空格再截断，保证单行又尽量多展示最终输出。
    let flat: String = result.split_whitespace().collect::<Vec<_>>().join(" ");
    let snippet: String = flat.chars().take(800).collect();
    cron::append_log(&t.id, &format!("status={status}{stat} | {snippet}"));
    // 任务异常（非 ok）一并落到错误日志，便于自修复定位。
    if status != "ok" {
        buglog::record("cron", &format!("task={} {}", t.name, status));
    }
    t.last_run = Some(now);
    t.last_status = Some(status.to_string());
    t.runs += 1;
    t.last_duration_ms = Some(elapsed_ms);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_only_llm_error_within_limit() {
        // LLM 调用失败：前两遍重试，第 3 遍用尽不再重试。
        assert!(should_retry("llm_error", 1));
        assert!(should_retry("llm_error", 2));
        assert!(!should_retry("llm_error", 3));
        // 其余状态一律不重试。
        assert!(!should_retry("ok", 1));
        assert!(!should_retry("max_iterations", 1));
        assert!(!should_retry("workdir_not_found", 1));
        assert!(!should_retry("notify_err: x", 1));
    }

    #[test]
    fn run_slot_blocks_concurrent_same_id() {
        let a = acquire_run_slot("slot-test-1");
        assert!(a.is_some(), "首次占用应成功");
        assert!(
            acquire_run_slot("slot-test-1").is_none(),
            "同 id 已在运行，应拒绝（防并发重复执行）"
        );
        assert!(
            acquire_run_slot("slot-test-2").is_some(),
            "不同 id 不受影响"
        );
        drop(a);
        assert!(
            acquire_run_slot("slot-test-1").is_some(),
            "释放后应可再次占用"
        );
    }
}
