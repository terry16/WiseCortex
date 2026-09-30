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
//!
//! **用户点「停止」怎么生效**：工具跑在 `spawn_blocking` 的阻塞线程里，而 tokio 的 `abort()`
//! 只能取消 async 任务、**动不了已经在跑的阻塞线程**（实测：abort 后 `is_finished=true`，
//! 阻塞线程照样睡满、跑完命令）。所以中断必须走协作式：[`cancel`] 置标记 + 杀整棵进程树，
//! 子进程一死管道就关，阻塞在 [`Session::run_command`] 里的读循环立刻拿到 EOF/取消标记返回。

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use wait_timeout::ChildExt;

use super::{require_str, Tool, ToolResult};

const MAX_OUTPUT: usize = 30_000;
const DEFAULT_TIMEOUT_MS: u64 = 300_000; // 5 分钟，够装依赖/构建；可按需覆盖
/// 读循环里查「是否被取消」的轮询片长。杀进程树通常能立刻带来 EOF，这一层是兜底：
/// 万一某个孙进程仍攥着管道不让它关，也能在 100ms 内响应中断，而不是干等到超时。
const CANCEL_POLL: Duration = Duration::from_millis(100);

// ── 持久会话 ────────────────────────────────────────────────────────────────

/// 一个会话的取消状态。**故意放在 Session 互斥锁之外**：`run_command` 全程持着那把锁阻塞，
/// 若 `cancel()` 也要拿它才能杀子进程，就会「想中断的人被要中断的人锁在门外」死锁。
/// 这里全用原子量，`cancel()` 免锁即可推进。
struct CancelState {
    /// 中断代际号：每收到一次「停止」就 +1。命令开始时拍下当前值，跑的过程中只要它变了
    /// 就说明「这一下停止是冲着我来的」。
    ///
    /// 为什么不用布尔 flag：flag 无法区分「上一轮残留的停止」和「针对本条命令的停止」。
    /// 停止信号落在两条命令之间（或 agent 正在调 LLM）时 flag 会残留，下一条正常命令一开跑
    /// 就被误杀。代际号天然是「消费一次即失效」的语义，不需要额外的复位步骤。
    gen: AtomicU64,
    /// 会话子进程（cmd/bash）的 pid；0 = 尚未起进程。
    pid: AtomicU32,
    /// 此刻确实有一条命令在跑。
    ///
    /// **必须有这道门**：用户点「停止」时，agent 很可能正在调 LLM（一个普通 async await，
    /// `abort()` 本来就能干净取消），shell 会话此刻空闲且健康。若无条件杀，就会白白弄死
    /// 会话、丢掉用户的 cwd/env/venv，下一条命令直接报「会话已结束」。
    /// 注意：即便这里判定为空闲而不杀，gen 也照样 +1，正在启动中的命令仍会察觉并自行退出。
    busy: AtomicBool,
}

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
    /// 取消状态（与全局 CANCELS 表里同一个 Arc）。
    cancel: Arc<CancelState>,
}

/// 会话表的一个条目：把「会话本体」、「它的取消状态」与「创建互斥」绑在一起登记。
///
/// **为什么必须绑在一起**（曾经分成 SESSIONS / CANCELS 两张表，那会出真问题）：
/// 两张表就可能分歧——并发首次创建同一 key 的会话时，输家清理自己那份取消登记，
/// 会连带把赢家的登记一起抹掉（因为它的 insert 早已覆盖了表里那条）。此后 `cancel(key)`
/// 查不到任何东西，那个**存活**的会话再也停不下来，正是本次要修的故障又回来了。
/// 合成一条目后「入表」与「登记取消状态」是同一次操作，不存在分歧。
struct Entry {
    /// 取消状态。同一 key 的所有创建者复用表里那一份（先到先得），所以谁都不会
    /// 覆盖掉别人的登记。
    cancel: Arc<CancelState>,
    /// `None` = 有创建者正在 spawn。这段最长 15s 的窗口里 `cancel()` 依然能推进 gen，
    /// 好让初始化命令察觉到自己该退（否则「停止」在建会话期间会被空放）。
    /// `Some` = 会话已就绪可用。
    session: Option<Arc<Mutex<Session>>>,
    /// 创建互斥（single-flight）：**同一 key 同时只允许一个创建者在 spawn**。
    ///
    /// `Session::spawn` 会把自己那个子进程的 pid 写进**整个 key 共享的** `cancel.pid`（取消路径
    /// 就靠它立即杀整棵进程树）。并发 spawn 会让这个字段指向一个即将被丢弃的输家子进程，
    /// 于是存活会话的 `cancel()` 只能靠读循环轮询退出（≤100ms 才生效），而不是立即杀树；
    /// 顺带还白起一个 cmd.exe 又马上杀掉。
    ///
    /// 注意：孤儿孙进程**不**靠这把锁兼底（那由 [`kill_tree`] 用本会话自己的 child pid 保证），
    /// 它保的是「停止能立即杀对树」与「不白起进程」。变异测试证实：拿掉它，孙进程照样会死。
    ///
    /// 它只挡同一个 key：不同会话各有各的锁，互不阻塞；全局表锁也不在 spawn 期间持有。
    creating: Arc<Mutex<()>>,
}

static REGISTRY: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);

fn registry() -> &'static Mutex<HashMap<String, Entry>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
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
///
/// 这把全局锁只在**短临界区**内持有（查表 / 填表 / 删表），从不跨 spawn 或 run_command，
/// 所以 `cancel()` 取它不会被正阻塞在命令里的线程挡住。
fn registry_locked() -> std::sync::MutexGuard<'static, HashMap<String, Entry>> {
    registry().lock().unwrap_or_else(|e| e.into_inner())
}

/// run_command 期间持有：离开作用域即复位 busy（含 panic 路径）。
struct BusyGuard<'a>(&'a CancelState);
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}

/// 按 pid 杀掉**整棵进程树**。
///
/// 只杀直接子进程是不够的：会话子进程是 cmd/bash，它派生的孙进程（python / node / ssh / cargo…）
/// 不会因为父进程死了就跟着死——于是「命令已终止」之后进程还在后台跑、端口还被占着。
/// 更糟的是孙进程继承了 stdout/stderr 管道，管道不关，读线程就看不到 EOF，阻塞线程会一直
/// 干等到超时（这正是「点停止没反应」的第二个成因）。
fn kill_tree_by_pid(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(windows)]
    {
        // /T 连带子进程，/F 强制。
        let mut c = Command::new("taskkill");
        c.args(["/PID", &pid.to_string(), "/T", "/F"]);
        crate::proc::no_window(&mut c);
        let _ = c.status();
    }
    #[cfg(unix)]
    {
        // 会话子进程是进程组组长（见 own_process_group），负号表示「杀整个组」。
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{pid}")])
            .status();
        // 兜底：组信号没覆盖到时，至少把直接子进程杀掉。
        let _ = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
}

/// 请求中断某会话正在跑的命令（用户点「停止」时由 registry 调用）。
///
/// 返回 true 表示确实有一个**正在执行命令**的会话被终止。
///
/// 三个关键设计：
/// - **不持有任何会话锁**：`run_command` 全程持着那把锁阻塞，若这里也要拿它才能杀进程，
///   就会「想中断的人被要中断的人锁在门外」死锁。所以只用原子量。
/// - **gen 无条件 +1**：即使此刻空闲（不杀进程），也要让代际号前进，好让「正在启动中」
///   的命令察觉到自己该退。这一步也顺带解决了残留信号误杀下一条命令的问题。
/// - **只在 busy 时杀进程树**：停止多半发生在 LLM 调用期间，那时会话是健康的，
///   杀它只会无故丢掉 cwd/env。
///
/// 推进代际在先、杀进程树在后：即便杀失败，读循环也会在下个轮询片（≤100ms）自己退出。
pub fn cancel(key: &str) -> bool {
    // 只取全局表的短临界区拷一份 Arc 出来，随后全程只用原子量：
    // 绝不能去碰会话锁——`run_command` 正阻塞着持有它，碰了就死锁。
    let Some(cs) = registry_locked().get(key).map(|e| e.cancel.clone()) else {
        return false;
    };
    cs.gen.fetch_add(1, Ordering::SeqCst);
    if !cs.busy.load(Ordering::SeqCst) {
        return false;
    }
    kill_tree_by_pid(cs.pid.load(Ordering::Relaxed));
    true
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
    ///
    /// `cancel` 由调用方**先建好并登记到全局表**再传进来：建会话要起子进程 + 同步初始化
    /// （最长 15s），这段窗口里用户完全可能已经点了「停止」。若等进程起来才登记，
    /// 那一下取消就会空放（`cancel()` 找不到会话），命令照跑到底。
    fn spawn(base: &Path, cancel: Arc<CancelState>) -> std::io::Result<Session> {
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
        // 自成进程组，以便中断时能整棵树一起杀（Windows 上空操作，那边走 taskkill /T）。
        crate::proc::own_process_group(&mut builder);
        let mut child = builder.spawn()?;
        // 先把 pid 公开给取消路径（从此刻起 `cancel()` 能杀到它），再去做耗时的初始化。
        cancel.pid.store(child.id(), Ordering::Release);

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
            cancel: cancel.clone(),
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
        // 其它初始化失败可以容忍（顶多是 prompt/编码不理想），但「已被取消」必须往上抛：
        // 那是用户点的停止，不能吞掉再拿一个半死的会话去跑真命令。
        if let Err(e) = sess.run_command(":", 15_000) {
            if e == "__CANCELLED__" {
                // 抛错前先把刚起的子进程杀掉：sess 被丢弃时并不会自动杀 child（无 Drop 实现）。
                // 多数情况下 cancel() 已经杀过整棵树，这里是为了「取消落在 busy 置位之前、
                // 因而没触发 kill」的那条竞态路径兜底，否则会漏一个孤儿 cmd/bash。
                let mut dying = sess;
                kill_tree(&mut dying);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "shell 会话初始化期间被中断",
                ));
            }
        }
        Ok(sess)
    }

    /// 在会话里跑一条命令；读到哨兵为止，返回 (exit_code, 合并输出)。
    /// 超时返回 Err；被用户中断返回 `__CANCELLED__`；调用方据此销毁会话。
    fn run_command(&mut self, command: &str, timeout_ms: u64) -> Result<(i32, String), String> {
        // 标记「本会话正在跑命令」并拍下当前代际号。cancel() 只杀 busy 的会话，
        // 所以这两步必须赶在写命令之前完成，否则那一下停止会被当成「空闲」而白放。
        let my_gen = self.cancel.gen.load(Ordering::SeqCst);
        self.cancel.busy.store(true, Ordering::SeqCst);
        // 补查一次：拍号与置 busy 之间可能正好插进来一次 cancel（它当时看到 busy=false
        // 就没杀进程）。代际号已变则说明那一下是冲着本条命令来的，立即退出。
        if self.cancel.gen.load(Ordering::SeqCst) != my_gen {
            self.cancel.busy.store(false, Ordering::Release);
            return Err("__CANCELLED__".to_string());
        }
        // busy 必须在所有退出路径上复位（含 panic），否则会话会被当成永远在忙，
        // 之后每次「停止」都会去杀一个空闲会话。
        let _busy_guard = BusyGuard(&self.cancel);

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
            // 协作式中断：本函数跑在 spawn_blocking 的阻塞线程里，tokio 的 abort() 碰不到它，
            // 只能自己查代际号退出。放在循环顶部，保证「刚收到停止就来查」。
            if self.cancel.gen.load(Ordering::SeqCst) != my_gen {
                return Err("__CANCELLED__".to_string());
            }
            let now = Instant::now();
            if now >= deadline {
                return Err("__TIMEOUT__".to_string());
            }
            // 把等待切成 ≤CANCEL_POLL 的片：整段等到 deadline 的话，中断最长要等 5 分钟才有反应。
            let slice = (deadline - now).min(CANCEL_POLL);
            match self.rx.recv_timeout(slice) {
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
                // 一个轮询片内没输出：回循环顶重查代际号/超时（不再直接判超时）。
                Err(RecvTimeoutError::Timeout) => continue,
                // 管道断了 = 进程树已被杀。若代际号已变，报「中断」而非「会话结束」，
                // 让用户看到的是他点的那一下生效了。
                Err(RecvTimeoutError::Disconnected) => {
                    if self.cancel.gen.load(Ordering::SeqCst) != my_gen {
                        return Err("__CANCELLED__".to_string());
                    }
                    return Err("shell 会话已结束".to_string());
                }
            }
        }
    }
}

/// 取或建某 key 的持久会话。
///
/// 建新会话时**不持有全局表锁**：`Session::spawn` 要起子进程并同步初始化（最长 15s），
/// 在临界区里干这些既会阻塞**所有**会话，又把「持锁期间出事 → 全局锁中毒」的窗口拉得很长。
/// 全局锁只在查表/填表的瞬间持有；真正需要排队的只有同一个 key，靠 [`Entry::creating`]。
///
/// 流程（single-flight）：
/// ① 快路径——会话已就绪就直接返回（绝大多数调用走这条，只碰一次全局锁）；
/// ② 取（或建）本 key 的条目，拿到它的 cancel 与 creating 锁；
/// ③ 排 creating 队：同一 key 只有一个线程真正去 spawn，其余的醒来后在 ④ 直接拿到成品；
/// ④ 复查快路径（排队期间别人可能已建好）；
/// ⑤ spawn → 回填 session。
fn session_for(key: &str, base: &Path) -> Result<Arc<Mutex<Session>>, String> {
    // 条目在我们 spawn 期间被撤换（超时/中断触发 drop_session）时需要重来一轮；
    // 给个上限，免得极端情况下空转。
    let mut attempts = 0usize;
    loop {
        attempts += 1;
        if attempts > 8 {
            return Err(format!(
                "启动 shell 会话失败: 会话 {key} 反复被并发销毁，已放弃"
            ));
        }
        // ① 快路径
        if let Some(s) = registry_locked().get(key).and_then(|e| e.session.clone()) {
            return Ok(s);
        }
        // ② 取（或建）条目：cancel 先到先得，creating 锁按 key 排队。
        let (cancel, creating) = {
            let mut map = registry_locked();
            let e = map.entry(key.to_string()).or_insert_with(|| Entry {
                cancel: Arc::new(CancelState {
                    gen: AtomicU64::new(0),
                    pid: AtomicU32::new(0),
                    busy: AtomicBool::new(false),
                }),
                session: None,
                creating: Arc::new(Mutex::new(())),
            });
            (e.cancel.clone(), e.creating.clone())
        };
        // ③ 排队（容忍中毒：持锁者 panic 过，锁本身已释放，数据是空的没得可脏）。
        //    从此刻起到本轮结束，同一 key 不会再有第二个创建者 spawn —— 这就是
        //    `cancel.pid` 不会被互相覆盖的保证。
        let _creating_guard = creating.lock().unwrap_or_else(|e| e.into_inner());

        // ④ 复查：排到队了，先看是不是别人已经建好（那样我们连 spawn 都不用做）。
        if let Some(s) = registry_locked().get(key).and_then(|e| e.session.clone()) {
            return Ok(s);
        }
        // 条目可能在我们排队期间被 drop_session 撤掉（或被换成了新的一份）。
        // 此时手里的 cancel 已经不是表里那份，继续用它会登记到一个没人认的状态上：重来一轮。
        {
            let map = registry_locked();
            let still_mine = map
                .get(key)
                .map(|e| Arc::ptr_eq(&e.cancel, &cancel))
                .unwrap_or(false);
            if !still_mine {
                continue;
            }
        }
        // ⑤ 全局锁外 spawn（耗时；期间 `cancel()` 能通过已登记的 cancel 推进 gen 让它退）。
        let sess = match Session::spawn(base, cancel.clone()) {
            Ok(s) => s,
            Err(e) => {
                // 起会话失败：仅当条目还停在「我登记的、且尚未有人填会话」这个状态时才撤掉，
                // 免得留下一个 pid 指向死进程的僵尸条目（万一 pid 被系统复用，后续一次
                // `cancel()` 就会误杀无关进程）。已被别人填好的条目绝不动。
                let mut map = registry_locked();
                if let Some(en) = map.get(key) {
                    if en.session.is_none() && Arc::ptr_eq(&en.cancel, &cancel) {
                        map.remove(key);
                    }
                }
                return Err(format!("启动 shell 会话失败: {e}"));
            }
        };
        let arc = Arc::new(Mutex::new(sess));
        // 回填。因为持有 creating 锁，正常情况下这一格必然还是空的、cancel 必然还是我们那份。
        let mut filled = false;
        {
            let mut map = registry_locked();
            if let Some(en) = map.get_mut(key) {
                if Arc::ptr_eq(&en.cancel, &cancel) {
                    en.session = Some(arc.clone());
                    filled = true;
                }
            }
        }
        if filled {
            return Ok(arc);
        }
        // 万一真被换掉了（drop_session 抢先撤了条目）：手上这个会话没人认领，就地清理，
        // 别留下没人管的孤儿 cmd.exe，然后重来一轮建个新的。
        // （先把锁结果绑到局部再匹配：临时值析构顺序早于 `arc`，否则借用悬空。）
        let locked = arc.lock();
        if let Ok(mut s) = locked {
            kill_tree(&mut s);
        }
    }
}

/// 关闭并清理某任务的持久 shell 会话（任务删除时调用，避免常驻进程堆积）。
pub fn close_session(key: &str) {
    drop_session(key);
}

/// 杀掉会话进程**整棵树**并回收（cmd/bash 本身 + 它派生的孙进程）。
///
/// 为什么必须杀树：只杀直接子进程的话，孙进程（python / node / ssh / cargo…）不会跟着死，
/// 于是「命令已终止」之后进程还在后台跑、端口还被占着；而它们继承了 stdout/stderr 管道，
/// 管道不关，读线程就看不到 EOF，阻塞线程会一直干等到超时。
fn kill_tree(s: &mut Session) {
    // 用**本会话自己的** child pid 去杀树，而不是共享的 `s.cancel.pid`：
    // 后者是「这个 key 当前那个活会话」的 pid，在并发创建同一 key 时可能是别人的子进程，
    // 照着它杀就会误伤别人的会话（用户侧表现为命令莫名其妙报「shell 会话已结束」）。
    // 清理自己要杀谁，本会话的 child 句柄写得清清楚楚，不必去问全局状态。
    let own_pid = s.child.id();
    // 把公开出去的 pid 归零：会话既然已经要没了，就别让随后的 `cancel()` 拿着一个
    // 已被回收的 pid 去杀——万一系统复用了它，就变成误杀无关进程。
    //
    // 用 CAS 而不是无条件 `swap(0)`：这个字段是整个 key 共享的，只有当它确实还指着
    // **我们**的子进程时才该抹。否则并发创建的输家在这里会把赢家的 pid 抹成 0，
    // 那个存活会话的 `cancel()` 就再也找不到树可杀（只剩轮询退出的兼底路径）。
    let _ = s
        .cancel
        .pid
        .compare_exchange(own_pid, 0, Ordering::AcqRel, Ordering::Relaxed);
    kill_tree_by_pid(own_pid);
    let _ = s.child.kill();
    let _ = s.child.wait();
}

/// 销毁某 key 的会话（超时/中断/异常后调用，下次会重建），连带撤掉它的取消登记。
///
/// 先推进代际号再摘条目：若此刻正有个创建者卡在 spawn 里（条目 session=None），
/// 它的初始化命令会因此察觉到自己该退，不会拿着一个半死的会话回来。
///
/// 会话锁中毒时**同样要杀子进程**：旧代码写的是 `if let Ok(mut s) = arc.lock()`，
/// 中毒即静默跳过 kill，那个 cmd.exe 就变成没人管的孤儿常驻下去（用户侧表现为
/// 「后台一堆 CMD 杀不完」）。中毒的 Session 数据即便可疑，child 句柄也仍然有效，
/// 照杀不误。
fn drop_session(key: &str) {
    let removed = registry_locked().remove(key);
    if let Some(e) = removed {
        e.cancel.gen.fetch_add(1, Ordering::SeqCst);
        if let Some(arc) = e.session {
            let mut s = arc.lock().unwrap_or_else(|e| e.into_inner());
            kill_tree(&mut s);
        }
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
    /// 持久会话执行：状态跨调用保留；超时或被中断则销毁会话并报错。
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

/// 会话被重置的原因文案：超时说清「cwd/env 已丢失」，中断说清是用户点的停止，其余原样透出。
fn reset_reason(e: &str, timeout_ms: u64) -> String {
    if e == "__TIMEOUT__" {
        format!(
            "命令超时（{}s）已被终止，shell 会话已重置（cwd/env 已丢失）",
            timeout_ms / 1000
        )
    } else if e == "__CANCELLED__" {
        "命令已被用户中断（整棵进程树已终止），shell 会话已重置（cwd/env 已丢失）".to_string()
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
            let _guard = registry().lock().unwrap();
            panic!("模拟持全局锁线程 panic");
        });
        assert!(poisoner.join().is_err());
        assert!(registry().lock().is_err(), "全局表锁此时应已中毒");

        // 取锁辅助函数应能穿过中毒标记正常工作。
        let key = format!("test-gpoison-{}", std::process::id());
        let sh = Shell::with_session(std::env::temp_dir(), key.clone());
        let out = sh
            .execute(&json!({ "command": "echo global-ok" }))
            .expect("全局表中毒后仍应可用");
        assert!(out.contains("global-ok"), "got: {out}");
        drop_session(&key);
    }

    /// 回归：并发建同一 key 的会话时，存活会话**不能丢失取消能力**。
    ///
    /// 真实可达的场景：模型一次发多个 `task`，`run_parallel_tasks` 并发跑子 agent，
    /// 而子 agent 用的是主会话那份工具表（含同一个 sid 的持久 shell），于是多个线程
    /// 同时对同一 sid 调 `session_for`。
    ///
    /// 曾经把会话与取消状态分成两张表（SESSIONS / CANCELS），它们的「赢家」可能不是同一个
    /// 线程：输家清理自己那份取消登记时，会把已经覆盖上去的赢家的登记一起抹掉，导致
    /// 那个**存活**的会话 `cancel(key)` 永远返回 false —— 用户点「停止」又停不下来了。
    /// 合成单条目后，登记与会话是同一条，不存在分歧。
    ///
    /// 这个测试反复多轮并发创建（每轮都清干净重来），每轮结束后都断言：会话可用
    /// 且 cancel 能命中它。
    #[test]
    fn concurrent_session_for_keeps_cancel_wired() {
        let key = format!("test-concurrent-{}", std::process::id());
        let base = std::env::temp_dir();

        for round in 0..8 {
            // 多线程同时去取/建同一个 key 的会话。
            let mut handles = Vec::new();
            for t in 0..4 {
                let k = key.clone();
                let b = base.clone();
                handles.push(std::thread::spawn(move || {
                    let sh = Shell::with_session(b, k);
                    sh.execute(&json!({ "command": format!("echo round{round}-t{t}") }))
                }));
            }
            for h in handles {
                let out = h.join().expect("创建线程不应 panic");
                let out = out.expect("并发创建后命令应正常执行");
                assert!(
                    out.contains(&format!("round{round}")),
                    "每线程都应跑成自己那条命令, got: {out}"
                );
            }

            // 关键断言：并发创建之后，取消链路必须仍然接得上。
            let key2 = key.clone();
            let base2 = base.clone();
            let runner = std::thread::spawn(move || {
                let sh = Shell::with_session(base2, key2);
                sh.execute(&json!({ "command": long_cmd(), "timeout_ms": 120_000 }))
            });
            let mut cancelled = false;
            for _ in 0..200 {
                std::thread::sleep(Duration::from_millis(25));
                if cancel(&key) {
                    cancelled = true;
                    break;
                }
            }
            assert!(
                cancelled,
                "第 {round} 轮：并发建会话后 cancel() 应仍命中存活会话"
            );
            let err = runner.join().unwrap().expect_err("被中断的命令应返回 Err");
            assert!(err.contains("中断"), "应报「中断」: {err}");

            drop_session(&key);
        }
    }

    /// 一条跑得久的命令，用于验证中断（约 120s，远超测试里的等待时长）。
    fn long_cmd() -> &'static str {
        if cfg!(windows) {
            // ping 约 121s；输出丢进 nul，避免刷屏。
            "ping 127.0.0.1 -n 121 >nul"
        } else {
            "sleep 120"
        }
    }

    /// 回归（真实事故）：用户点「停止」停不下来。
    ///
    /// 根因是工具跑在 `spawn_blocking` 的阻塞线程里，而 tokio 的 `abort()` **取消不了已经在跑的
    /// 阻塞线程**（本项目实测：abort 返回后 `is_finished=true`，阻塞线程照样睡满、跑完命令）。
    /// 修复后 `cancel(key)` 走协作式取消：推进代际号 + 杀整棵进程树，读循环随即退出。
    /// 断言两点：① 远早于超时/命令时长就返回；② 报错是「中断」而不是「超时」。
    #[test]
    fn cancel_interrupts_running_command() {
        let key = format!("test-cancel-{}", std::process::id());
        let base = std::env::temp_dir();
        let sh = Shell::with_session(base, key.clone());
        // 预热：确保会话已建好，把建会话的耗时排除在计时之外。
        sh.execute(&json!({ "command": "echo warmup" })).unwrap();

        let key2 = key.clone();
        let runner = std::thread::spawn(move || {
            sh.execute(&json!({ "command": long_cmd(), "timeout_ms": 120_000 }))
        });

        // 等命令真的跑起来（busy 已置位），再点「停止」。
        let mut cancelled = false;
        for _ in 0..200 {
            std::thread::sleep(Duration::from_millis(25));
            if cancel(&key2) {
                cancelled = true;
                break;
            }
        }
        assert!(cancelled, "cancel() 应命中正在跑命令的会话（busy 门）");

        let started = Instant::now();
        let res = runner.join().expect("执行线程不应 panic");
        let elapsed = started.elapsed();

        let err = res.expect_err("被中断的命令应返回 Err");
        assert!(err.contains("中断"), "应报「中断」而非其它: {err}");
        assert!(
            elapsed < Duration::from_secs(20),
            "应在远小于命令时长(120s)内返回, 实际 {:?}",
            elapsed
        );

        drop_session(&key);
    }

    /// 中断只该作用于「那一条」命令：会话被销毁后重建，下一条命令必须正常跑完。
    /// 若取消信号会残留，这里就会被立刻误杀（旧设计用布尔 flag 正是这个毛病）。
    #[test]
    fn next_command_after_cancel_runs_normally() {
        let key = format!("test-cancel-next-{}", std::process::id());
        let base = std::env::temp_dir();
        let sh = Shell::with_session(base.clone(), key.clone());
        sh.execute(&json!({ "command": "echo warmup" })).unwrap();

        let key2 = key.clone();
        let runner = std::thread::spawn(move || {
            sh.execute(&json!({ "command": long_cmd(), "timeout_ms": 120_000 }))
        });
        for _ in 0..200 {
            std::thread::sleep(Duration::from_millis(25));
            if cancel(&key2) {
                break;
            }
        }
        let _ = runner.join();

        // 同一 key 再跑一条普通命令：会话应被透明重建，且不被上一次的取消信号误伤。
        let sh2 = Shell::with_session(base, key.clone());
        let out = sh2
            .execute(&json!({ "command": "echo after-cancel" }))
            .expect("中断之后的下一条命令应正常执行");
        assert!(out.contains("after-cancel"), "got: {out}");

        drop_session(&key);
    }

    /// 会话空闲时（agent 正在调 LLM，这才是「停止」最常见的时机）不该杀会话：
    /// 那样会白白丢掉用户的 cwd/env/venv。断言 cancel() 返回 false 且会话仍可用。
    #[test]
    fn cancel_on_idle_session_keeps_it_alive() {
        let key = format!("test-cancel-idle-{}", std::process::id());
        let base = std::env::temp_dir();
        let sh = Shell::with_session(base.clone(), key.clone());
        if cfg!(windows) {
            sh.execute(&json!({ "command": "set WC_ALIVE=yes" }))
                .unwrap();
        } else {
            sh.execute(&json!({ "command": "export WC_ALIVE=yes" }))
                .unwrap();
        }

        // 此刻没有命令在跑：cancel 应明确告知「没打断任何东西」，且不动会话。
        assert!(!cancel(&key), "空闲会话不该被 cancel 命中");

        let read = if cfg!(windows) {
            "echo %WC_ALIVE%"
        } else {
            "echo $WC_ALIVE"
        };
        let out = sh
            .execute(&json!({ "command": read }))
            .expect("空闲时的 cancel 不应弄死会话");
        assert!(out.contains("yes"), "会话状态应完好保留, got: {out}");

        drop_session(&key);
    }

    /// 中断要杀**整棵树**：cmd/bash 的孙进程也得死，否则「已停止」之后它还在后台跑、
    /// 还攥着 stdout 管道（管道不关 → 读线程看不到 EOF → 阻塞线程干等到超时）。
    ///
    /// 做法：前台命令先派生一个长眠的孙进程、把它的 pid 写进文件，然后 wait 住（于是
    /// run_command 一直阻塞）；等 pid 文件落盘后点「停止」，最后校验那个孙进程已不存在。
    #[test]
    fn cancel_kills_grandchild_process() {
        let key = format!("test-cancel-tree-{}", std::process::id());
        let base = std::env::temp_dir();
        let pidfile = base.join(format!("wc-grandchild-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&pidfile);

        let sh = Shell::with_session(base, key.clone());
        sh.execute(&json!({ "command": "echo warmup" })).unwrap();

        // 前台命令：派生长眠孙进程 → 记下它的 pid → 等它（使命令本身一直不结束）。
        let cmd = if cfg!(windows) {
            format!(
                "powershell -NoProfile -Command \"$c = Start-Process cmd -ArgumentList '/c','ping 127.0.0.1 -n 121 >nul' -PassThru -WindowStyle Hidden; Set-Content -LiteralPath '{}' -Value $c.Id; Wait-Process -Id $c.Id\"",
                pidfile.display()
            )
        } else {
            format!("sleep 120 & echo $! > '{}'; wait", pidfile.display())
        };

        let key2 = key.clone();
        let runner = std::thread::spawn(move || {
            sh.execute(&json!({ "command": cmd, "timeout_ms": 120_000 }))
        });

        // 先等孙进程真的起来（pid 文件落盘）再点停止——否则根本没测到「杀孙」。
        let mut gpid: Option<u32> = None;
        for _ in 0..300 {
            std::thread::sleep(Duration::from_millis(50));
            if let Ok(txt) = std::fs::read_to_string(&pidfile) {
                if let Ok(pid) = txt.trim().parse::<u32>() {
                    if pid > 0 {
                        gpid = Some(pid);
                        break;
                    }
                }
            }
        }
        let gpid = gpid.expect("孙进程应已启动并写下 pid");
        assert!(pid_alive(gpid), "孙进程此时应在跑");

        let mut cancelled = false;
        for _ in 0..200 {
            if cancel(&key2) {
                cancelled = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(cancelled, "cancel() 应命中正在跑命令的会话");

        let res = runner.join().expect("执行线程不应 panic");
        let err = res.expect_err("被中断的命令应返回 Err");
        assert!(err.contains("中断"), "应报「中断」: {err}");

        // 关键断言：孙进程必须跟着死。只杀直接子进程的话它会成孤儿，继续跑满 120s。
        let mut dead = false;
        for _ in 0..100 {
            if !pid_alive(gpid) {
                dead = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            dead,
            "孙进程 pid={gpid} 应随进程树一起被杀，否则仍是孤儿（实际状态：{}）",
            state_desc(gpid)
        );

        drop_session(&key);
        std::fs::remove_file(&pidfile).ok();
    }

    /// 回归：并发创建同一 key 之后，存活会话仍必须**杀得掉整棵进程树**（孙进程不能成孤儿）。
    ///
    /// 钉住的是 [`kill_tree`] 用「本会话自己的 child pid」杀树这一设计：`cancel.pid` 是整个 key
    /// 共享的，并发创建时它可能指向一个即将被丢弃的输家子进程；只有靠会话自己的 child 句柄，
    /// 清理才必然杀对目标。曾经写成 `kill_tree_by_pid(s.cancel.pid.swap(0, ..))`，那就是个真 bug。
    ///
    /// 顺带也覆盖 `Entry::creating`（single-flight）：它保的是「停止能立即杀对树」和「不白起
    /// 一个 cmd.exe 又杀掉」，而非本测试断言的孤儿问题——变异测试证实拿掉它孙进程照样会死
    /// （`__CANCELLED__` 后走 `drop_session` → `kill_tree`，与共享 pid 无关）。别把两件事混为一谈。
    ///
    /// 多跑几轮：这个竞态是时序相关的，一轮侥幸通过说明不了问题。
    #[test]
    fn concurrent_creation_still_kills_process_tree() {
        let key = format!("test-concurrent-tree-{}", std::process::id());
        let base = std::env::temp_dir();

        for round in 0..4 {
            // 先把上一轮清干净，保证每轮都是真正的「首次并发创建」。
            drop_session(&key);

            // 多线程同时去取/建同一 key 的会话（模型一次发多个 task 时子 agent 就是这么干的）。
            let mut handles = Vec::new();
            for t in 0..4 {
                let k = key.clone();
                let b = base.clone();
                handles.push(std::thread::spawn(move || {
                    let sh = Shell::with_session(b, k);
                    sh.execute(&json!({ "command": format!("echo r{round}t{t}") }))
                }));
            }
            for h in handles {
                h.join()
                    .expect("创建线程不应 panic")
                    .expect("并发创建后命令应正常执行");
            }

            // 在这个（幸存下来的）会话上跑一个会长眠孙进程的命令。
            let pidfile = base.join(format!("wc-ct-{round}-{}.txt", std::process::id()));
            let _ = std::fs::remove_file(&pidfile);
            let cmd = if cfg!(windows) {
                format!(
                    "powershell -NoProfile -Command \"$c = Start-Process cmd -ArgumentList '/c','ping 127.0.0.1 -n 121 >nul' -PassThru -WindowStyle Hidden; Set-Content -LiteralPath '{}' -Value $c.Id; Wait-Process -Id $c.Id\"",
                    pidfile.display()
                )
            } else {
                format!("sleep 120 & echo $! > '{}'; wait", pidfile.display())
            };
            let key2 = key.clone();
            let sh = Shell::with_session(base.clone(), key2.clone());
            let runner = std::thread::spawn(move || {
                sh.execute(&json!({ "command": cmd, "timeout_ms": 120_000 }))
            });

            // 等孙进程真起来（pid 落盘）再停，否则根本没测到「杀树」。
            let mut gpid: Option<u32> = None;
            for _ in 0..300 {
                std::thread::sleep(Duration::from_millis(50));
                if let Ok(txt) = std::fs::read_to_string(&pidfile) {
                    if let Ok(pid) = txt.trim().parse::<u32>() {
                        if pid > 0 {
                            gpid = Some(pid);
                            break;
                        }
                    }
                }
            }
            let gpid = gpid.expect("孙进程应已启动并写下 pid");
            assert!(pid_alive(gpid), "孙进程此时应在跑");

            let mut cancelled = false;
            for _ in 0..200 {
                if cancel(&key2) {
                    cancelled = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            assert!(cancelled, "第 {round} 轮：cancel() 应命中存活会话");
            let err = runner
                .join()
                .expect("执行线程不应 panic")
                .expect_err("被中断的命令应返回 Err");
            assert!(err.contains("中断"), "第 {round} 轮：应报「中断」: {err}");

            // 关键断言：并发创建过后，杀树能力不能丢。
            let mut dead = false;
            for _ in 0..100 {
                if !pid_alive(gpid) {
                    dead = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(
                dead,
                "第 {round} 轮：孙进程 pid={gpid} 应随进程树一起被杀——\
                 若按共享的 cancel.pid 杀树，就会杀到输家的进程、留下这个孤儿\
                 （实际状态：{}）",
                state_desc(gpid)
            );

            drop_session(&key);
            let _ = std::fs::remove_file(&pidfile);
        }
    }

    /// Unix：取进程的状态字母（`R`/`S`/`D`/`Z`…）；拿不到返回 `None`。
    ///
    /// 为什么不能只靠 `kill -0`：它对**僵尸**（Z，已死但还没被父进程 wait 回收）
    /// 照样返回成功。我们杀的孙进程，其父（会话 bash）也同时被杀了，孤儿没人 reap
    /// 就会长期停在僵尸态——用 `kill -0` 探就会把「已杀掉」误判成「还活着」。
    fn proc_state(pid: u32) -> Option<char> {
        #[cfg(target_os = "linux")]
        {
            // /proc/<pid>/stat 第 3 个字段是状态字母。comm（第 2 字段）可能含空格和
            // 括号，所以要从**最后一个** ')' 之后再切，不能按空格直接取第 3 列。
            let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            let (_, rest) = s.rsplit_once(')')?;
            rest.trim().chars().next()
        }
        #[cfg(not(target_os = "linux"))]
        {
            // macOS 等没有 /proc：问 ps 要状态列。进程不存在时 ps 返回非 0。
            let out = Command::new("ps")
                .args(["-o", "state=", "-p", &pid.to_string()])
                .output()
                .ok()?;
            if !out.status.success() {
                return None;
            }
            String::from_utf8_lossy(&out.stdout).trim().chars().next()
        }
    }

    /// 某 pid 是否**仍在跑**（跨平台）。
    ///
    /// 僵尸不算在跑：它已死、FD 已释放，只是没人 wait 回收。判定分两步而不是一步，
    /// 是为了 macOS 零回归：`ps` 万一拿不到状态（None）就退回「存在即在跑」的老语义，
    /// 绝不会把活进程误判成死。
    fn pid_alive(pid: u32) -> bool {
        if cfg!(windows) {
            let mut c = Command::new("tasklist");
            c.args(["/FI", &format!("PID eq {pid}"), "/NH"]);
            crate::proc::no_window(&mut c);
            c.output()
                .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
                .unwrap_or(false)
        } else {
            // kill -0 不发信号，只探测存在性与权限。
            let exists = Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            exists && proc_state(pid) != Some('Z')
        }
    }

    /// 把进程状态说人话，供断言失败时自证：
    /// 分清「真的还活着」和「已死但成了僵尸没人 reap」——这两者的处置完全相反。
    fn state_desc(pid: u32) -> String {
        if cfg!(windows) {
            return if pid_alive(pid) {
                "存活".to_string()
            } else {
                "不存在".to_string()
            };
        }
        match proc_state(pid) {
            None => "不存在（已回收）".to_string(),
            Some('Z') => "Z 僵尸（已死，只是没人 wait 回收）".to_string(),
            Some(c) => format!("{c} 存活"),
        }
    }
}
