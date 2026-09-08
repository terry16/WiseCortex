//! 写失败静默的打印宏：替代裸 `println!` / `eprintln!`。
//!
//! 为什么需要：Rust 的 `println!` 在写 stdout 失败时**直接 panic**，而不是返回错误。
//! 服务端进程的 stdout 往往是父进程（桌面端 / 启动器）持有的管道，父进程退出或重启后
//! 管道读端关闭，此时任何一次 `println!` 都会 panic（Windows 上是 os error 232
//! "管道正在被关闭"）。
//!
//! 真实事故：某线程恰好**持有 shell 会话表的全局锁**时 panic 于此，std 遂把该锁永久
//! 标记为 poisoned，此后每次 `.lock().unwrap()` 都二次 panic —— shell 工具（agent
//! 执行命令的唯一入口）彻底瘫痪，连负责善后的清理函数也被同一把毒锁挡在门外，只能重启
//! 进程。日志里那条孤零零的
//! `[panic] failed printing to stdout: 管道正在被关闭。 (os error 232)` 就是病根。
//!
//! 因此：**日志输出永远不该有能力杀死业务线程**。这两个宏写失败即静默丢弃。

/// `println!` 的安全替代：写失败静默丢弃，绝不 panic。
#[macro_export]
macro_rules! sprintln {
    () => {{
        use std::io::Write;
        let _ = writeln!(std::io::stdout());
    }};
    ($($arg:tt)*) => {{
        use std::io::Write;
        let _ = writeln!(std::io::stdout(), $($arg)*);
    }};
}

/// `eprintln!` 的安全替代：写失败静默丢弃，绝不 panic。
#[macro_export]
macro_rules! seprintln {
    () => {{
        use std::io::Write;
        let _ = writeln!(std::io::stderr());
    }};
    ($($arg:tt)*) => {{
        use std::io::Write;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

#[cfg(test)]
mod tests {
    /// 宏能正常输出且不 panic（管道关闭的场景无法在单测内构造，此处保证基本可用性）。
    #[test]
    fn macros_do_not_panic() {
        crate::sprintln!("safeprint test {}", 1);
        crate::seprintln!("safeprint test {}", 2);
        crate::sprintln!();
        crate::seprintln!();
    }
}
