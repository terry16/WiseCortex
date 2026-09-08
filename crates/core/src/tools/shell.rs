//! shell 工具：在工作目录下执行命令，返回合并的 stdout+stderr，带超时保护。
//!
//! 两种模式：
//! - **持久会话**（按任务绑定，agent 用）：每个任务维持一个长生命周期 shell 进程，
//!   `cd` / 环境变量 / `venv` 激活 / `PATH` 改动**跨工具调用保留**——像 Claude Code 一样
//!   可以分步搭建开发环境。命令用唯一哨兵分隔、解析退出码。
//! - **无状态**（测试 / CLI）：每次新起进程执行（旧行为）。
//!
//! 跨平台：Windows 用 `cmd`（chcp 65001 切 UTF-8、自定义 prompt 便于剥离），其余用 `bash`。
//! 前台命令超时后杀掉会话进程并返回错误；后台常驻进程（background=true）走独立 detached 日志方式。

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use wait_timeout::ChildExt;

use super::{require_str, Tool, ToolResult};

const MAX_OUTPUT: usize = 30_000;
const DEFAULT_TIMEOUT_MS: u64 = 300_000; // 5 分钟，够装依赖/构建；可按需覆盖

// ── 持久会话 ────────────────────────────────────────────────────────────────

/// 一个长生命周期 shell 会话（保留 cwd/env，跨命令复用）。
struct Session {
    child: Child,
    stdin: ChildStdin,
    /// 合并后的输出行（stdout + stderr，已去掉行尾换行/CR）。
    rx: Receiver<String>,
    /// 命令完成哨兵：输出 `<token><exit_code>` 标记一条命令结束。
    token: String,
    /// Windows 自定义 prompt 前缀（需从输出里剥离）；非 Windows 为空。
    prompt_marker: String,
}

static SESSIONS: OnceLock<Mutex<HashMap<String, Arc<Mutex<Session>>>>> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);

fn sessions() -> &'static Mutex<HashMap<String, Arc<Mutex<Session>>>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 取会话表的锁，**容忍锁中毒**。
///
/// 背景（真实事故）：某线程持锁期间 panic（曾因往已关闭的 stdout 管道 `println!`，
/// os error 232），std 会把这把锁永久标记为 poisoned。此后 `.lock().unwrap()`
/// 会**主动 panic**，于是每一次 shell 调用都死在取锁这一步——包括负责善后的
/// `drop_session`，形成「修锁的钥匙锁在屋里」：进程内再没有任何路径能重置它，
/// 只能重启。锁本身在 panic 时已由守卫析构正常释放，中毒只是「数据可能不一致」
/// 的标记；这里的数据是会话表（HashMap），不一致的最坏情况是某条会话状态可疑，
/// 由调用方按 key 丢弃重建即可，绝不值得让整个 shell 工具永久瘫痪。
fn sessions_locked() -> std::sync::MutexGuard<'static, HashMap<String, Arc<Mutex<Session>>>> {
    sessions().lock().unwrap_or_else(|e| e.into_inner())
}

fn unique_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("__WC_DONE_{nanos:x}_{n}__")
}

/// 起一个读线程把 reader 的每一行送进 channel（去掉行尾 \r）。
/// 按**原始字节**读取再 lossy 解码——Windows cmd 启动横幅在 chcp 切 UTF-8 前是 OEM 码页
/// （如 GBK），不是合法 UTF-8；用 read_line 会报错并杀死读线程导致会话假死，故用 read_until。
fn spawn_reader<R: Read + Send + 'static>(reader: R, tx: mpsc::Sender<String>) {
    std::thread::spawn(move || {
        let mut buf = BufReader::new(reader);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match buf.read_until(b'\n', &mut bytes) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let line = String::from_utf8_lossy(&bytes);
                    let trimmed = line.trim_end_matches(['\n', '\r']).to_string();
                    if tx.send(trimmed).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

impl Session {
    /// 起一个新会话并初始化 cwd / 编码 / prompt。
    fn spawn(base: &Path) -> std::io::Result<Session> {
        let token = unique_token();
        let (tx, rx) = mpsc::channel::<String>();

        let mut builder = if cfg!(windows) {
            // 交互式 cmd（读 stdin）；/Q 关回显。
            let mut c = Command::new("cmd");
            c.arg("/Q")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            c
        } else {
            // 非交互式 bash 从管道读：无 prompt、无命令回显，输出干净。
            let mut c = Command::new("bash");
            c.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            c
        };
        crate::proc::no_window(&mut builder);
        let mut child = builder.spawn()?;

        let stdin = child.stdin.take().expect("piped stdin");
        if let Some(o) = child.stdout.take() {
            spawn_reader(o, tx.clone());
        }
        if let Some(e) = child.stderr.take() {
            spawn_reader(e, tx);
        }

        let prompt_marker = if cfg!(windows) {
            "__WCP__".to_string()
        } else {
            String::new()
        };

        let mut sess = Session {
            child,
            stdin,
            rx,
            token,
            prompt_marker,
        };

        // 初始化：设编码 / prompt / cwd，再用一次哨兵把启动横幅与初始 prompt 排空。
        let base_disp = base.display();
        if cfg!(windows) {
            // prompt 末尾 $_ 插入换行，使 prompt 独占一行、不污染命令首行输出。
            let _ = writeln!(sess.stdin, "prompt __WCP__$G$_");
            let _ = writeln!(sess.stdin, "chcp 65001>nul");
            let _ = writeln!(sess.stdin, "cd /d \"{base_disp}\"");
        } else {
            let _ = writeln!(sess.stdin, "cd '{base_disp}'");
        }
        let _ = sess.stdin.flush();
        // 用一条 no-op 命令同步：丢弃初始化期间的一切输出，直到看到哨兵。
        let _ = sess.run_command(":", 15_000);
        Ok(sess)
    }

    /// 在会话里跑一条命令；读到哨兵为止，返回 (exit_code, 合并输出)。
    /// 超时返回 Err；调用方据此销毁会话。
    fn run_command(&mut self, command: &str, timeout_ms: u64) -> Result<(i32, String), String> {
        // 写命令，再写"打印哨兵+退出码"的一行。
        if cfg!(windows) {
            writeln!(self.stdin, "{command}").map_err(|e| format!("写入命令失败: {e}"))?;
            writeln!(self.stdin, "echo {}%errorlevel%", self.token)
                .map_err(|e| format!("写入哨兵失败: {e}"))?;
        } else {
            writeln!(self.stdin, "{command}").map_err(|e| format!("写入命令失败: {e}"))?;
            writeln!(self.stdin, "printf '%s%d\\n' '{}' \"$?\"", self.token)
                .map_err(|e| format!("写入哨兵失败: {e}"))?;
        }
        self.stdin.flush().map_err(|e| format!("flush 失败: {e}"))?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let mut out = String::new();
        let mut capped = false;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err("__TIMEOUT__".to_string());
            }
            match self.rx.recv_timeout(deadline - now) {
                Ok(line) => {
                    // 剥离 Windows 的自定义 prompt 行。
                    if !self.prompt_marker.is_empty() && line.starts_with(&self.prompt_marker) {
                        // prompt 行可能形如 "__WCP__>"；整行丢弃。
                        let rest = line
                            .trim_start_matches(|c| c != '>')
                            .trim_start_matches('>');
                        if rest.is_empty() {
                            continue;
                        }
                    }
                    if let Some(pos) = line.find(&self.token) {
                        let code: i32 = line[pos + self.token.len()..].trim().parse().unwrap_or(-1);
                        return Ok((code, out));
                    }
                    if !capped {
                        out.push_str(&line);
                        out.push('\n');
                        if out.len() > MAX_OUTPUT {
                            out.truncate(MAX_OUTPUT);
                            out.push_str("\n…(输出已截断)");
                            capped = true;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => return Err("__TIMEOUT__".to_string()),
                Err(RecvTimeoutError::Disconnected) => return Err("shell 会话已结束".to_string()),
            }
        }
    }
}

/// 取或建某 key 的持久会话。
///
/// 建新会话时**不持有全局锁**：`Session::spawn` 要起子进程并同步初始化（最长 15s），
/// 在临界区里干这些既会阻塞其它会话，又把「持锁期间出事 → 全局锁中毒」的窗口拉得很长。
/// 故先放锁再 spawn，最后重新取锁登记；期间若有人抢先建好，用对方的、丢弃自己的。
fn session_for(key: &str, base: &Path) -> Result<Arc<Mutex<Session>>, String> {
    if let Some(s) = sessions_locked().get(key) {
        return Ok(s.clone());
    }
    let sess = Session::spawn(base).map_err(|e| format!("启动 shell 会话失败: {e}"))?;
    let arc = Arc::new(Mutex::new(sess));
    let mut map = sessions_locked();
    if let Some(existing) = map.get(key) {
        // 竞态：别的线程已建好。用既有的，本线程刚起的进程就地清理，避免泄漏。
        let winner = existing.clone();
        drop(map);
        if let Ok(mut s) = arc.lock() {
            let _ = s.child.kill();
            let _ = s.child.wait();
        }
        return Ok(winner);
    }
    map.insert(key.to_string(), arc.clone());
    Ok(arc)
}

/// 关闭并清理某任务的持久 shell 会话（任务删除时调用，避免常驻进程堆积）。
pub fn close_session(key: &str) {
    drop_session(key);
}

/// 销毁某 key 的会话（超时/异常后调用，下次会重建）。
///
/// 会话锁中毒时**同样要杀子进程**：旧代码写的是 `if let Ok(mut s) = arc.lock()`，
/// 中毒即静默跳过 kill，那个 cmd.exe 就变成没人管的孤儿常驻下去（用户侧表现为
/// 「后台一堆 CMD 杀不完」）。中毒的 Session 数据即便可疑，child 句柄也仍然有效，
/// 照杀不误。
fn drop_session(key: &str) {
    let removed = sessions_locked().remove(key);
    if let Some(arc) = removed {
        let mut s = arc.lock().unwrap_or_else(|e| e.into_inner());
        let _ = s.child.kill();
        let _ = s.child.wait();
    }
}

// ── Shell 工具 ──────────────────────────────────────────────────────────────

pub struct Shell {
    base: PathBuf,
    /// 持久会话 key（=任务 sid）；None=无状态（每次新进程，测试/CLI 用）。
    session: Option<String>,
}
impl Shell {
    /// 无状态 shell（每次新起进程）。
    pub fn new(base: PathBuf) -> Self {
        Self {
            base,
            session: None,
        }
    }
    /// 持久会话 shell：以 `key`（任务 sid）维持长生命周期进程，状态跨调用保留。
    pub fn with_session(base: PathBuf, key: String) -> Self {
        Self {
            base,
            session: Some(key),
        }
    }
}

impl Tool for Shell {
    fn name(&self) -> &'static str {
        "shell"
    }
    fn description(&self) -> &'static str {
        "Run a command in a persistent shell session: cd / environment variables / venv activation / PATH \
         changes persist across subsequent commands, so you can set up a dev environment step by step \
         (install python/node/compilers, create a virtualenv, set env vars, then use them). \
         Returns stdout and stderr merged. A foreground command that exceeds the timeout (default 5 minutes) \
         is killed and the session is reset — do not run interactive or long-running commands in the foreground. \
         For long-running processes (e.g. a dev server) set background=true: it is started detached in the \
         background and returns the pid and a log path; then use read_file to read the log and shell kill/taskkill to stop it."
    }
    fn requires_approval(&self) -> bool {
        true
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command line to execute" },
                "timeout_ms": { "type": "integer", "description": "Foreground timeout in milliseconds (default 300000)" },
                "background": { "type": "boolean", "description": "true = start a long-running process detached in the background, returning pid + log path immediately (does not use the persistent session)" }
            },
            "required": ["command"]
        })
    }
    fn summary(&self, args: &Value) -> String {
        let cmd = args.get("command").and_then(Value::as_str).unwrap_or("?");
        format!("$ {cmd}")
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let command = require_str(args, "command")?;
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_MS);
        let background = args
            .get("background")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if background {
            return self.run_background(&command);
        }
        match &self.session {
            Some(key) => self.run_persistent(key, &command, timeout_ms),
            None => self.run_oneshot(&command, timeout_ms),
        }
    }
}

impl Shell {
    /// 持久会话执行：状态跨调用保留；超时则销毁会话并报错。
    ///
    /// 会话锁中毒时**自愈**：说明上一个持锁者是在跑命令的半路 panic 的，这个 Session 的
    /// 读通道/哨兵配对已不可信（可能残留上一条命令的输出）。抢救不如重建——丢弃它、
    /// 起一个干净会话重试一次。历史故障里这里写的是 `.lock().unwrap()`，中毒后每次
    /// 调用都在此二次 panic，shell 工具（我调用命令的唯一入口）从此永久不可用。
    fn run_persistent(&self, key: &str, command: &str, timeout_ms: u64) -> ToolResult {
        let sess = session_for(key, &self.base)?;
        let poisoned = sess.lock().is_err();
        if poisoned {
            // 脏会话直接丢弃重建；重建后的锁是全新的，不会再中毒。
            drop_session(key);
            let fresh = session_for(key, &self.base)?;
            let mut guard = fresh.lock().unwrap_or_else(|e| e.into_inner());
            return match guard.run_command(command, timeout_ms) {
                Ok((code, out)) => finish(code, out),
                Err(e) => {
                    drop(guard);
                    drop_session(key);
                    Err(reset_reason(&e, timeout_ms))
                }
            };
        }
        let mut guard = sess.lock().unwrap_or_else(|e| e.into_inner());
        match guard.run_command(command, timeout_ms) {
            Ok((code, out)) => finish(code, out),
            Err(e) => {
                drop(guard);
                drop_session(key);
                Err(reset_reason(&e, timeout_ms))
            }
        }
    }

    /// 无状态执行（每次新进程）。
    fn run_oneshot(&self, command: &str, timeout_ms: u64) -> ToolResult {
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(format!("chcp 65001>nul & {command}"));
            c
        } else {
            let mut c = Command::new("sh");
            c.arg("-c").arg(command);
            c
        };
        cmd.current_dir(&self.base);
        crate::proc::no_window(&mut cmd);

        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("启动命令失败: {e}"))?;

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

        let status = match child
            .wait_timeout(Duration::from_millis(timeout_ms))
            .map_err(|e| format!("等待命令失败: {e}"))?
        {
            Some(status) => status,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("命令超时（{}s）已被终止", timeout_ms / 1000));
            }
        };

        let stdout = String::from_utf8_lossy(&h_out.join().unwrap_or_default()).to_string();
        let stderr = String::from_utf8_lossy(&h_err.join().unwrap_or_default()).to_string();
        let mut out = stdout;
        if !stderr.trim().is_empty() {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&stderr);
        }
        finish(status.code().unwrap_or(-1), out)
    }

    /// 后台常驻进程：输出重定向到日志文件，立即返回 pid + 日志路径。
    fn run_background(&self, command: &str) -> ToolResult {
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(format!("chcp 65001>nul & {command}"));
            c
        } else {
            let mut c = Command::new("sh");
            c.arg("-c").arg(command);
            c
        };
        cmd.current_dir(&self.base);

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let log = std::env::temp_dir().join(format!("wisecortex-bg-{nanos}.log"));
        let f = File::create(&log).map_err(|e| format!("创建日志失败: {e}"))?;
        let f2 = f.try_clone().map_err(|e| format!("日志句柄失败: {e}"))?;
        cmd.stdout(Stdio::from(f)).stderr(Stdio::from(f2));
        crate::proc::no_window(&mut cmd);
        let child = cmd.spawn().map_err(|e| format!("启动命令失败: {e}"))?;
        let pid = child.id();
        let stop = if cfg!(windows) {
            format!("taskkill /PID {pid} /F")
        } else {
            format!("kill {pid}")
        };
        Ok(format!(
            "已后台启动: pid={pid}\n日志: {}\n用 read_file 查看日志；停止: shell `{stop}`",
            log.display()
        ))
    }
}

/// 会话被重置的原因文案：超时说清「cwd/env 已丢失」，其余原样透出。
fn reset_reason(e: &str, timeout_ms: u64) -> String {
    if e == "__TIMEOUT__" {
        format!(
            "命令超时（{}s）已被终止，shell 会话已重置（cwd/env 已丢失）",
            timeout_ms / 1000
        )
    } else {
        e.to_string()
    }
}

/// 把退出码 + 输出整理为 ToolResult。
fn finish(code: i32, mut out: String) -> ToolResult {
    if out.len() > MAX_OUTPUT {
        out.truncate(MAX_OUTPUT);
        out.push_str("\n…(输出已截断)");
    }
    if code == 0 {
        if out.trim().is_empty() {
            out = "(命令成功，无输出)".to_string();
        }
        Ok(out)
    } else {
        Err(format!("命令退出码 {code}:\n{out}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_runs() {
        let sh = Shell::new(std::env::temp_dir());
        let out = sh
            .execute(&json!({ "command": "echo wisecortex" }))
            .unwrap();
        assert!(out.contains("wisecortex"));
    }

    #[test]
    fn failing_command_is_err_with_code() {
        let sh = Shell::new(std::env::temp_dir());
        let err = sh.execute(&json!({ "command": "exit 3" })).unwrap_err();
        assert!(err.contains("3"));
    }

    #[test]
    fn slow_command_times_out() {
        let sh = Shell::new(std::env::temp_dir());
        let cmd = if cfg!(windows) {
            "ping 127.0.0.1 -n 6 >nul"
        } else {
            "sleep 5"
        };
        let err = sh
            .execute(&json!({ "command": cmd, "timeout_ms": 300 }))
            .unwrap_err();
        assert!(err.contains("超时"));
    }

    #[test]
    fn persistent_session_keeps_env_across_calls() {
        let key = format!("test-sess-{}", std::process::id());
        let sh = Shell::with_session(std::env::temp_dir(), key.clone());
        // 第一条命令设环境变量，第二条命令应能读到（证明同一会话、状态保留）。
        if cfg!(windows) {
            sh.execute(&json!({ "command": "set WC_MARK=hello42" }))
                .unwrap();
            let out = sh.execute(&json!({ "command": "echo %WC_MARK%" })).unwrap();
            assert!(out.contains("hello42"), "env 应跨调用保留, got: {out}");
        } else {
            sh.execute(&json!({ "command": "export WC_MARK=hello42" }))
                .unwrap();
            let out = sh.execute(&json!({ "command": "echo $WC_MARK" })).unwrap();
            assert!(out.contains("hello42"), "env 应跨调用保留, got: {out}");
        }
        drop_session(&key);
    }

    #[test]
    fn persistent_session_keeps_cwd_across_calls() {
        let key = format!("test-cwd-{}", std::process::id());
        let base = std::env::temp_dir();
        // 在 base 下建一个子目录用于切换。
        let sub = base.join(format!("wc-shelltest-{}", std::process::id()));
        std::fs::create_dir_all(&sub).unwrap();
        let sh = Shell::with_session(base, key.clone());
        let cdcmd = if cfg!(windows) {
            format!("cd /d \"{}\"", sub.display())
        } else {
            format!("cd '{}'", sub.display())
        };
        sh.execute(&json!({ "command": cdcmd })).unwrap();
        let pwd = if cfg!(windows) { "cd" } else { "pwd" };
        let out = sh.execute(&json!({ "command": pwd })).unwrap();
        assert!(
            out.to_lowercase()
                .contains(&format!("wc-shelltest-{}", std::process::id())),
            "cwd 应跨调用保留, got: {out}"
        );
        drop_session(&key);
        std::fs::remove_dir_all(&sub).ok();
    }

    /// 回归（真实事故）：某线程持会话锁时 panic → std 把锁永久标记为 poisoned →
    /// 旧代码 `sess.lock().unwrap()` 每次调用都二次 panic，shell 工具永久瘫痪，
    /// 连负责善后的 drop_session 也被同一把毒锁挡在门外，只能重启进程。
    /// 修复后：中毒会话被丢弃重建，命令照常执行。
    #[test]
    fn poisoned_session_lock_self_heals() {
        let key = format!("test-poison-{}", std::process::id());
        let sh = Shell::with_session(std::env::temp_dir(), key.clone());
        // 先建立会话。
        sh.execute(&json!({ "command": "echo warmup" })).unwrap();

        // 人为毒化该会话锁：在持锁线程里 panic（与事故中 println! 打爆管道同构）。
        let arc = session_for(&key, &std::env::temp_dir()).unwrap();
        let poisoner = std::thread::spawn(move || {
            let _guard = arc.lock().unwrap();
            panic!("模拟持锁线程 panic");
        });
        assert!(poisoner.join().is_err(), "该线程应当 panic");
        assert!(
            session_for(&key, &std::env::temp_dir())
                .unwrap()
                .lock()
                .is_err(),
            "锁此时应已中毒"
        );

        // 关键断言：中毒后仍能正常执行命令（旧代码在此 panic）。
        let out = sh
            .execute(&json!({ "command": "echo healed" }))
            .expect("中毒后应自愈并正常执行");
        assert!(out.contains("healed"), "got: {out}");

        drop_session(&key);
    }

    /// 会话表（全局锁）中毒后，取会话/清理会话都不应 panic。
    #[test]
    fn poisoned_global_table_still_usable() {
        // 毒化全局会话表锁。
        let poisoner = std::thread::spawn(|| {
            let _guard = sessions().lock().unwrap();
            panic!("模拟持全局锁线程 panic");
        });
        assert!(poisoner.join().is_err());
        assert!(sessions().lock().is_err(), "全局表锁此时应已中毒");

        // 取锁辅助函数应能穿过中毒标记正常工作。
        let key = format!("test-gpoison-{}", std::process::id());
        let sh = Shell::with_session(std::env::temp_dir(), key.clone());
        let out = sh
            .execute(&json!({ "command": "echo global-ok" }))
            .expect("全局表中毒后仍应可用");
        assert!(out.contains("global-ok"), "got: {out}");
        drop_session(&key);
    }
}
