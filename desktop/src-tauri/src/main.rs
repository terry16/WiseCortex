// WiseCortex 桌面壳：内嵌 Rust 后端（WS on 127.0.0.1:7070），窗口加载打包的前端，
// 前端通过本地 WS 连这个内嵌后端——与 Linux 浏览器走同一条路径。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

/// 用户已在退出确认对话框中选择「确定退出」，允许本次关闭。
static EXIT_CONFIRMED: AtomicBool = AtomicBool::new(false);

fn main() {
    // 内嵌后端：独立线程跑一个 tokio runtime 起 server。
    thread::spawn(|| {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let addr: SocketAddr = "127.0.0.1:7070".parse().expect("addr");
            if let Err(e) = wisecortex_server::run(addr).await {
                eprintln!("内嵌后端退出: {e}");
            }
        });
    });

    tauri::Builder::default()
        // 单实例必须最先注册：再次启动时本回调在已有实例触发（把主窗口唤前），第二实例自行退出。
        // 这样内嵌后端只起一份，杜绝「多窗口共用首实例后端、关掉宿主窗口全断 WS」。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        // 原生「选择文件夹」对话框（工作目录选择）；前端经 window.__TAURI__.dialog 调用。
        .plugin(tauri_plugin_dialog::init())
        // 在系统浏览器打开外部 URL（前端经 window.__TAURI__.opener.openUrl 调用）。
        .plugin(tauri_plugin_opener::init())
        // 操作系统信息：前端经 window.__TAURI__.os.locale() 取系统语言，按之自动设默认界面语言。
        .plugin(tauri_plugin_os::init())
        // 窗口关闭拦截：如有尚未完成的后台任务（task_start），弹确认框，防止误关丢失。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if EXIT_CONFIRMED.load(Ordering::Relaxed) {
                    return; // 用户已在对话框中确认退出，放行。
                }
                let running: Vec<_> = wisecortex_server::jobs::list()
                    .into_iter()
                    .filter(|j| j.status == "running")
                    .collect();
                if running.is_empty() {
                    return; // 无运行中的后台任务，正常关闭。
                }
                api.prevent_close();
                let count = running.len();
                let summary: String = running
                    .iter()
                    .map(|j| format!("  • {} [{}]", j.description, j.id))
                    .collect::<Vec<_>>()
                    .join("\n");
                let msg = format!(
                    "还有 {count} 个后台任务正在运行：\n\n{summary}\n\n退出程序后这些任务将丢失。确定要退出吗？"
                );
                let app = window.app_handle().clone();
                let w = window.clone();
                app.dialog()
                    .message(msg)
                    .title("WiseCortex - 后台任务未完成")
                    .kind(MessageDialogKind::Warning)
                    .buttons(MessageDialogButtons::OkCancelCustom(
                        "确定退出".into(),
                        "继续等待".into(),
                    ))
                    .show(move |confirmed| {
                        if confirmed {
                            EXIT_CONFIRMED.store(true, Ordering::Relaxed);
                            let _ = w.close();
                        }
                    });
            }
        })
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用出错");
}
