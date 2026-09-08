//! 后台长任务：异步派生一个自治 agent 跑一个自包含 prompt，立即返回 job id，
//! 不占用当前对话回合。可查询状态/结果、可中止。面向「托管自动任务」。
//!
//! 与同步 `task` 子 agent 的区别：`task` 阻塞父回合直到出结果；后台任务**即发即返**、
//! 在后台跑完把结果存起来，之后用 `task_result` 取。
//!
//! agent-facing 工具：`task_start` / `task_list` / `task_result` / `task_stop`。
//! 人类侧：REST `/api/jobs`。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};
use tokio::runtime::Handle;
use tokio::task::AbortHandle;
use wisecortex_core::tools::{require_str, Tool, ToolResult};

use crate::agent::Agent;

#[derive(Clone, Serialize)]
pub struct JobInfo {
    pub id: String,
    pub description: String,
    /// running | done | failed | stopped
    pub status: String,
    pub created_ms: u128,
    /// 完成后的最终结果文本（运行中为空）。
    pub output: String,
}

struct Inner {
    jobs: HashMap<String, JobInfo>,
    handles: HashMap<String, AbortHandle>,
}

fn store() -> &'static Mutex<Inner> {
    static S: OnceLock<Mutex<Inner>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(Inner {
            jobs: HashMap::new(),
            handles: HashMap::new(),
        })
    })
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// 启动一个后台任务：在 `handle` 的运行时上派生一个自治 agent 跑 `prompt`。返回 job id。
pub fn start(handle: &Handle, description: String, prompt: String) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let id = format!("job-{}", SEQ.fetch_add(1, Ordering::Relaxed));
    {
        let mut s = store().lock().unwrap();
        s.jobs.insert(
            id.clone(),
            JobInfo {
                id: id.clone(),
                description,
                status: "running".into(),
                created_ms: now_ms(),
                output: String::new(),
            },
        );
    }
    let id_run = id.clone();
    let task = handle.spawn(async move {
        // 独立配置一份 agent（隔离上下文），跑一个一次性回合。
        let agent = Agent::configure();
        // 后台长任务用交互式那一档上限（见 Agent::run_background）——这是「放着跑的大活」，
        // 不该和 IM/定时那种短触发共用防失控的低上限。
        let out = agent.run_background(&prompt).await;
        finish(&id_run, "done", out);
    });
    store()
        .lock()
        .unwrap()
        .handles
        .insert(id.clone(), task.abort_handle());
    id
}

/// 任务结束时写回状态与结果（仍在 running 才写，避免覆盖 stopped）。
fn finish(id: &str, status: &str, output: String) {
    let mut s = store().lock().unwrap();
    if let Some(j) = s.jobs.get_mut(id) {
        if j.status == "running" {
            j.status = status.to_string();
            j.output = output;
        }
    }
    s.handles.remove(id);
}

pub fn list() -> Vec<JobInfo> {
    let mut v: Vec<JobInfo> = store().lock().unwrap().jobs.values().cloned().collect();
    v.sort_by_key(|j| j.created_ms);
    v
}

pub fn get(id: &str) -> Option<JobInfo> {
    store().lock().unwrap().jobs.get(id).cloned()
}

/// 中止运行中的任务，返回是否确实中止。
pub fn stop(id: &str) -> bool {
    let mut s = store().lock().unwrap();
    let aborted = if let Some(h) = s.handles.remove(id) {
        h.abort();
        true
    } else {
        false
    };
    if let Some(j) = s.jobs.get_mut(id) {
        if j.status == "running" {
            j.status = "stopped".into();
        }
    }
    aborted
}

fn fmt_job(j: &JobInfo) -> String {
    format!("{} [{}] {}", j.id, j.status, j.description)
}

// ── agent-facing 工具 ────────────────────────────────────────────────────────

/// `task_start`：即发即返地启动后台任务。
pub struct TaskStart {
    handle: Handle,
}
impl TaskStart {
    pub fn new(handle: Handle) -> Self {
        Self { handle }
    }
}
impl Tool for TaskStart {
    fn name(&self) -> &'static str {
        "task_start"
    }
    fn description(&self) -> &'static str {
        "启动一个【后台长任务】：派生一个自治子 agent 在后台执行一个自包含的 prompt，立即返回 job id，\
         不阻塞当前对话。适合耗时长、可放着跑的活（大型重构、批量处理、长调研）。\
         之后用 task_list 看进度、task_result 取结果、task_stop 停止。\
         需要立刻拿到结果再继续时，请用同步的 task 工具而非本工具。"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "description": { "type": "string", "description": "任务简述（一行，便于列表识别）" },
                "prompt": { "type": "string", "description": "交给后台子 agent 的完整自包含指令" }
            },
            "required": ["description", "prompt"]
        })
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "后台任务: {}",
            args.get("description")
                .and_then(Value::as_str)
                .unwrap_or("?")
        )
    }
    fn requires_approval(&self) -> bool {
        true // 启动自治后台工作，按危险操作对待（solo 绕过；计划模式拦截）
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let description = require_str(args, "description")?;
        let prompt = require_str(args, "prompt")?;
        let id = start(&self.handle, description, prompt);
        Ok(format!(
            "已启动后台任务 {id}（运行中）。用 task_result(\"{id}\") 查询结果、task_stop(\"{id}\") 停止。"
        ))
    }
}

/// `task_list`：列出全部后台任务及状态。
pub struct TaskList;
impl Tool for TaskList {
    fn name(&self) -> &'static str {
        "task_list"
    }
    fn description(&self) -> &'static str {
        "列出全部后台任务（task_start 启动的）及其状态（running/done/failed/stopped）。"
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn execute(&self, _args: &Value) -> ToolResult {
        let jobs = list();
        if jobs.is_empty() {
            return Ok("（暂无后台任务）".into());
        }
        Ok(jobs.iter().map(fmt_job).collect::<Vec<_>>().join("\n"))
    }
}

/// `task_result`：取某后台任务的状态与结果。
pub struct TaskResult;
impl Tool for TaskResult {
    fn name(&self) -> &'static str {
        "task_result"
    }
    fn description(&self) -> &'static str {
        "取某后台任务的当前状态；已完成则返回其结果文本。参数 id 来自 task_start/task_list。"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "id": { "type": "string", "description": "任务 id（如 job-1）" } },
            "required": ["id"]
        })
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let id = require_str(args, "id")?;
        match get(&id) {
            None => Err(format!("没有这个后台任务: {id}")),
            Some(j) if j.status == "running" => Ok(format!("{id} 仍在运行中…")),
            Some(j) => Ok(format!(
                "{} [{}]\n{}",
                j.id,
                j.status,
                if j.output.is_empty() {
                    "(无输出)"
                } else {
                    &j.output
                }
            )),
        }
    }
}

/// `task_stop`：中止一个后台任务。
pub struct TaskStop;
impl Tool for TaskStop {
    fn name(&self) -> &'static str {
        "task_stop"
    }
    fn description(&self) -> &'static str {
        "中止一个运行中的后台任务。参数 id 来自 task_start/task_list。"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "id": { "type": "string", "description": "任务 id（如 job-1）" } },
            "required": ["id"]
        })
    }
    fn requires_approval(&self) -> bool {
        true
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let id = require_str(args, "id")?;
        if stop(&id) {
            Ok(format!("已中止后台任务 {id}"))
        } else {
            Ok(format!("{id} 不在运行中（可能已结束或不存在）"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 直接验证任务管理状态机（不依赖 Agent::configure，避免真机 LLM 调用）。
    #[test]
    fn manager_state_machine() {
        let id = format!("job-test-{}", std::process::id());
        {
            let mut s = store().lock().unwrap();
            s.jobs.insert(
                id.clone(),
                JobInfo {
                    id: id.clone(),
                    description: "demo".into(),
                    status: "running".into(),
                    created_ms: now_ms(),
                    output: String::new(),
                },
            );
        }
        assert_eq!(get(&id).unwrap().status, "running");
        finish(&id, "done", "the result".into());
        let j = get(&id).unwrap();
        assert_eq!(j.status, "done");
        assert_eq!(j.output, "the result");
        assert!(list().iter().any(|x| x.id == id));
        // 已结束的任务 stop 返回 false；二次 finish 不覆盖已完成状态。
        assert!(!stop(&id));
        finish(&id, "failed", "x".into());
        assert_eq!(get(&id).unwrap().status, "done");
    }

    #[test]
    fn stop_marks_running_job_stopped() {
        let id = format!("job-stoptest-{}", std::process::id());
        {
            let mut s = store().lock().unwrap();
            s.jobs.insert(
                id.clone(),
                JobInfo {
                    id: id.clone(),
                    description: "d".into(),
                    status: "running".into(),
                    created_ms: now_ms(),
                    output: String::new(),
                },
            );
        }
        // 无运行句柄 → stop 返回 false，但把 running 标记为 stopped。
        assert!(!stop(&id));
        assert_eq!(get(&id).unwrap().status, "stopped");
    }
}
