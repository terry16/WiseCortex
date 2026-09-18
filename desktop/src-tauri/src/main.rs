// WiseCortex 桌面壳：内嵌 Rust 后端（WS on 127.0.0.1，端口默认 7070、被占则顺延），
// 窗口加载打包的前端，前端通过本地 WS 连这个内嵌后端——与 Linux 浏览器走同一条路径。
//
// 窗口在 setup() 里编程创建，tauri.conf.json 的 app.windows 故意留空：
// 只有编程建窗才能挂 initialization_script，而真实端口必须在页面脚本执行前就注入。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::mpsc;
use std::thread;

use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

/// 用户已在退出确认对话框中选择「确定退出」，允许本次关闭。
static EXIT_CONFIRMED: AtomicBool = AtomicBool::new(false);

/// 内嵌后端首选端口；被占时往后顺延（见 `wisecortex_server::PORT_SCAN_LIMIT`）。
const PREFERRED_PORT: u16 = 7070;

/// 内嵌后端实际绑定到的端口（0 = 尚未绑定）。
static ACTUAL_PORT: AtomicU16 = AtomicU16::new(0);

fn main() {
    // 内嵌后端：独立线程跑一个 tokio runtime 起 server。
    //
    // 绑定必须在建窗口**之前**完成：前端需要知道真实端口才能连 WS，而端口可能因
    // 冲突顺延。故这里用 channel 等绑定结果，拿到端口（或失败原因）后再继续。
    let (tx, rx) = mpsc::channel::<Result<u16, String>>();
    thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                let _ = tx.send(Err(format!("无法创建运行时：{e}")));
                return;
            }
        };
        rt.block_on(async move {
            let start: SocketAddr = ([127, 0, 0, 1], PREFERRED_PORT).into();
            let listener = match wisecortex_server::bind_with_fallback(
                start,
                wisecortex_server::PORT_SCAN_LIMIT,
            )
            .await
            {
                Ok(l) => l,
                Err(e) => {
                    let _ = tx.send(Err(format!(
                        "端口 {}-{} 全被占用，无法启动本地服务：{e}",
                        PREFERRED_PORT,
                        PREFERRED_PORT + wisecortex_server::PORT_SCAN_LIMIT - 1,
                    )));
                    return;
                }
            };
            let port = match listener.local_addr() {
                Ok(a) => a.port(),
                Err(e) => {
                    let _ = tx.send(Err(format!("取绑定端口失败：{e}")));
                    return;
                }
            };
            // 先告知端口（主线程靠它建窗），再进入服务循环。
            let _ = tx.send(Ok(port));
            if let Err(e) = wisecortex_server::serve_on(listener).await {
                wisecortex_core::buglog::record("desktop", &format!("内嵌后端退出：{e}"));
            }
        });
    });

    // 等绑定结果。超时不致命（慢机器可能真的慢），先用首选端口先把窗口开起来。
    let bind_result = rx.recv_timeout(std::time::Duration::from_secs(10));
    let port = match &bind_result {
        Ok(Ok(p)) => *p,
        _ => PREFERRED_PORT,
    };
    ACTUAL_PORT.store(port, Ordering::Relaxed);

    // 把真实端口注入窗口：前端的 backend.ts 优先读 __WC_PORT__，读不到才回退 7070。
    let init_script = format!("window.__WC_PORT__ = {port};");

    tauri::Builder::default()
        // 把内嵌后端的真实端口告知前端。必须用 initialization_script：它在页面脚本之前
        // 执行，而前端一加载就会读它去连 WS。
        .setup({
            let script = init_script.clone();
            move |app| {
                // 绑定失败：必须说出来。旧实现只 eprintln，release 下 windows_subsystem="windows"
                // 根本没有控制台，用户看到的只是「要密钥」和空白配置，完全指不到真因。
                if let Ok(Err(msg)) = &bind_result {
                    wisecortex_core::buglog::record("desktop", msg);
                    app.dialog()
                        .message(format!(
                            "{msg}\n\n请先关闭占用该端口的程序（可能是上次未退干净的 WiseCortex），\
                             再重新启动。你的配置和会话都在，不会丢失。"
                        ))
                        .title("WiseCortex - 本地服务启动失败")
                        .kind(MessageDialogKind::Error)
                        .blocking_show();
                }
                // 窗口在这里建而不写在 tauri.conf.json：只有编程建窗才能挂
                // initialization_script，而端口必须在页面脚本跑起来**之前**就存在。
                tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
                    .title("WiseCortex")
                    .inner_size(1200.0, 800.0)
                    .initialization_script(&script)
                    .build()?;
                Ok(())
            }
        })
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
