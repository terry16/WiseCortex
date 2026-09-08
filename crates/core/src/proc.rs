//! 子进程辅助：在 Windows 上隐藏控制台黑窗。

/// 给即将 spawn 的命令打上「不创建控制台窗口」标志（Windows = `CREATE_NO_WINDOW`），
/// 其它平台为空操作。返回同一个 `&mut Command` 以便链式调用。
///
/// 桌面壳是 GUI 子系统进程（release 下 `windows_subsystem = "windows"`），其下派生的
/// 控制台程序（cmd / powershell / git / node / 语言服务器 …）默认会各自弹出一个控制台
/// 黑窗一闪而过。这些子进程的 stdout/stderr 都已走管道回灌到聊天区，窗口纯属噪声——
/// 此标志杜绝其创建。窗口从不出现，自然也无需「执行完再去关闭」。
pub fn no_window(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW：进程不分配控制台窗口（仍有隐藏 console，管道 I/O 正常）。
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
