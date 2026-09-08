//! macOS 本地 GUI 控制工具（仅 #[cfg(target_os = "macos")]）。
//!
//! 与 Windows 侧（win_gui + uia）对位，**工具名刻意保持一致**（list_windows /
//! capture_window / window_click / ...），这样提示词与技能不必区分平台。
//!
//! 本阶段 = 「眼睛 + 手」，刻意**不碰 Accessibility(AX) API**：
//!   - 眼睛：CGWindowListCopyWindowInfo 枚举 + CGDisplay::screenshot 截图
//!   - 手：enigo 合成输入
//!
//! AX 只在读控件树（ui_tree/find_element 等语义工具）时才需要，而它有个硬约束——
//! 只能在主线程调（Apple DTS 口径），和 tokio 的轮换 blocking 线程池天然冲突。
//! 把它推到下一阶段，可以让权限/坐标这两个最大的风险先被单独验证掉。
//!
//! 权限（TCC）：
//!   - 截图需要「屏幕录制」；**枚举不需要**，但没有它 kCGWindowName 会整个键消失
//!     （拿到 None，而不是空串）→ 症状是「窗口列得出来、标题全没」。
//!   - 合成输入需要「辅助功能」。没授权时 CGEventPost 静默成功但什么也不发生。
//!   - 裸二进制常常压根不出现在授权列表里（TCC 认的是 bundle + 稳定签名，
//!     从 Terminal 起的进程会被算到 Terminal.app 头上）——这是打包问题不是代码 bug。

use base64::Engine as _;
use serde_json::{json, Value};

use core_foundation::array::CFArray;
use core_foundation::base::{TCFType, ToVoid};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::access::ScreenCaptureAccess;
use core_graphics::display::CGDisplay;
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use core_graphics::window::{
    copy_window_info, kCGNullWindowID, kCGWindowBounds, kCGWindowImageBoundsIgnoreFraming,
    kCGWindowLayer, kCGWindowListExcludeDesktopElements, kCGWindowListOptionIncludingWindow,
    kCGWindowListOptionOnScreenOnly, kCGWindowName, kCGWindowNumber, kCGWindowOwnerName,
    kCGWindowOwnerPID,
};

use super::gui_input::split_chord;
use super::{
    capture_scale, format_windows, image_result, img_to_win, require_str, scaled_dims, Tool,
    ToolResult, WinInfo, CAPTURE_MAX_EDGE,
};

mod input;
use input::{focus_app, InputKind};

/// 视觉截图最长边上限。与坐标换算共用 super::CAPTURE_MAX_EDGE。
const MAX_EDGE: u32 = CAPTURE_MAX_EDGE;

// ---------- 权限 ----------

/// 「辅助功能」授权状态。合成输入（点击/输入/按键/滚动）必须有它。
/// 自己声明 FFI：core-graphics 不导出它，而 accessibility-sys 本阶段没引入。
/// 注意这里用的是 AXIsProcessTrusted 而非 CGPreflightPostEventAccess——前者正是
/// 系统设置里「辅助功能」那个开关，和用户看到的 UI 对得上，报错才好指路。
fn ax_trusted() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    unsafe { AXIsProcessTrusted() }
}

/// 输入类工具的统一权限门。没授权就直接给出可操作的指路，
/// 而不是让 enigo 静默地「成功但没反应」——那种失败最耗人。
fn require_input_permission() -> Result<(), String> {
    if ax_trusted() {
        return Ok(());
    }
    Err("未获得「辅助功能」授权，无法合成输入。\n\
         请到 系统设置 → 隐私与安全性 → 辅助功能，为运行 wisecortex 的程序打开开关。\n\
         注意：从终端直接跑的裸二进制，系统往往把权限算在终端 App（Terminal/iTerm）头上，\
         甚至根本不出现在列表里——那就给终端 App 授权，或改用打包好的 wisecortex.app。\n\
         另外：若程序开启了 App Sandbox，辅助功能 API 会始终不可用（Apple 明确不支持）。"
        .to_string())
}

/// 「屏幕录制」授权状态。截图必须有它；枚举没有它也能跑，但窗口标题会全部缺失。
fn screen_recording_ok() -> bool {
    // ScreenCaptureAccess 是单元结构体，直接用其值即可（::default() 会被 clippy 判违规）。
    ScreenCaptureAccess.preflight()
}

// ---------- CF 字典读取 ----------

/// 取 extern static 的 CFStringRef 键。包一层是为了把 unsafe 收拢在一处。
fn key_of(k: CFStringRef) -> CFString {
    unsafe { CFString::wrap_under_get_rule(k) }
}

fn dict_i64(d: &CFDictionary, k: CFStringRef) -> Option<i64> {
    let v = d.find(key_of(k).to_void())?;
    unsafe { CFNumber::wrap_under_get_rule(*v as _) }.to_i64()
}

fn dict_string(d: &CFDictionary, k: CFStringRef) -> Option<String> {
    let v = d.find(key_of(k).to_void())?;
    // 必须走 CFString::to_string；CFStringGetCStringPtr 对非 ASCII 会返回 NULL，
    // 那是老代码里静默丢中文/日文窗口标题的经典坑。
    Some(unsafe { CFString::wrap_under_get_rule(*v as _) }.to_string())
}

fn dict_rect(d: &CFDictionary, k: CFStringRef) -> Option<CGRect> {
    let v = d.find(key_of(k).to_void())?;
    let bounds: CFDictionary = unsafe { CFDictionary::wrap_under_get_rule(*v as _) };
    CGRect::from_dict_representation(&bounds)
}

// ---------- 枚举 ----------

/// 一个窗口的原始信息（hwnd 位置放 CGWindowID，与 Windows 侧 WinInfo 同形）。
#[derive(Debug)]
struct MacWin {
    info: WinInfo,
    /// 标题是否缺失（= 没有屏幕录制权限的信号）。
    title_missing: bool,
}

/// 枚举屏幕上的普通窗口。layer != 0 的（菜单栏、Dock、壁纸等）一律排除，
/// 否则会混进几十个系统层窗口把列表淹没。
fn enumerate_windows() -> Vec<MacWin> {
    let opts = kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements;
    let Some(infos): Option<CFArray> = copy_window_info(opts, kCGNullWindowID) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in infos.iter() {
        let d: CFDictionary = unsafe { CFDictionary::wrap_under_get_rule(*item as _) };
        // 只要普通应用窗口。
        if dict_i64(&d, unsafe { kCGWindowLayer }).unwrap_or(-1) != 0 {
            continue;
        }
        let id = dict_i64(&d, unsafe { kCGWindowNumber }).unwrap_or(0);
        if id == 0 {
            continue;
        }
        let pid = dict_i64(&d, unsafe { kCGWindowOwnerPID }).unwrap_or(0) as u32;
        let owner = dict_string(&d, unsafe { kCGWindowOwnerName }).unwrap_or_default();
        // kCGWindowName 在没有屏幕录制权限时**整个键都不存在**（不是空串）。
        let name = dict_string(&d, unsafe { kCGWindowName });
        let title_missing = name.is_none();
        let title = match name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(t) => format!("{owner} — {t}"),
            // 退化：至少给出属主 App 名，模型还能靠它定位。
            None => owner.clone(),
        };
        let r = dict_rect(&d, unsafe { kCGWindowBounds })
            .unwrap_or_else(|| CGRect::new(&CGPoint::new(0.0, 0.0), &CGSize::new(0.0, 0.0)));
        // 过滤退化尺寸的窗口（离屏缓存、0 尺寸的辅助窗口）。
        if r.size.width < 1.0 || r.size.height < 1.0 {
            continue;
        }
        out.push(MacWin {
            info: WinInfo {
                hwnd: id as isize,
                pid,
                title,
                rect: (
                    r.origin.x as i32,
                    r.origin.y as i32,
                    r.size.width as i32,
                    r.size.height as i32,
                ),
            },
            title_missing,
        });
    }
    out
}

/// 按 hwnd(CGWindowID) 找窗口。
fn find_window(hwnd: i64) -> Result<MacWin, String> {
    enumerate_windows()
        .into_iter()
        .find(|w| w.info.hwnd == hwnd as isize)
        .ok_or_else(|| format!("未找到窗口 hwnd={hwnd}（可先用 list_windows 重新获取）"))
}

pub struct ListWindows;

impl Tool for ListWindows {
    fn name(&self) -> &'static str {
        "list_windows"
    }
    fn description(&self) -> &'static str {
        "List all currently visible windows (hwnd, pid, size, title). Use the hwnd to target a window with capture_window and the other window tools."
    }
    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    fn summary(&self, _args: &Value) -> String {
        "列出窗口".to_string()
    }
    fn execute(&self, _args: &Value) -> ToolResult {
        let ws = enumerate_windows();
        let infos: Vec<WinInfo> = ws.iter().map(|w| w.info.clone()).collect();
        let mut text = format_windows(&infos);
        // 标题集体缺失几乎必然是权限问题，直接点破，别让模型对着一堆 App 名瞎猜。
        if !ws.is_empty() && ws.iter().all(|w| w.title_missing) {
            text.push_str(
                "\n\n注意：所有窗口标题都缺失，说明没有「屏幕录制」权限（窗口标题受该权限保护）。\
                 请到 系统设置 → 隐私与安全性 → 屏幕录制 里授权；上面只能显示所属 App 名。",
            );
        }
        Ok(text)
    }
}

// ---------- 截图 ----------

/// CGRectNull：告诉 CGWindowListCreateImage「用窗口自身的边界」。
/// 不用 CGRect::null()——0.25 的方法列表里没查到它，这里按 ABI 定义直接构造。
fn cg_rect_null() -> CGRect {
    CGRect::new(
        &CGPoint::new(f64::INFINITY, f64::INFINITY),
        &CGSize::new(0.0, 0.0),
    )
}

/// 对窗口截图，返回 (png, 输出宽, 输出高)。输出尺寸按**点**（而非像素）缩放。
///
/// Retina 的关键处理：CGImage 给的是**像素**（Retina 上 = 点数 × 2），而窗口边界、
/// 鼠标坐标都是**点**。这里统一把图缩放到「按点算」的目标尺寸，于是最终图像尺寸
/// 只是点数的纯函数 —— window_click 就能用与 Windows 完全相同的 capture_scale
/// 从窗口边界就地重算出比例，无需查后备缩放因子、也无需保存状态。
fn capture_window_png(w: &MacWin) -> Result<(Vec<u8>, u32, u32), String> {
    if !screen_recording_ok() {
        return Err("未获得「屏幕录制」授权，无法截图。\n\
                    请到 系统设置 → 隐私与安全性 → 屏幕录制 里为运行 wisecortex 的程序授权，\
                    然后**重启该程序**（该权限需重启进程才生效）。\n\
                    若列表里找不到它：从终端直跑的裸二进制常常无法被授权，请给终端 App 授权，\
                    或改用打包好的 wisecortex.app。"
            .to_string());
    }
    let (_, _, pw, ph) = w.info.rect;
    let (pw, ph) = (pw.max(1) as u32, ph.max(1) as u32);

    // kCGWindowListOptionIncludingWindow：只要这一个窗口，且不受遮挡影响。
    let img = CGDisplay::screenshot(
        cg_rect_null(),
        kCGWindowListOptionIncludingWindow,
        w.info.hwnd as u32,
        kCGWindowImageBoundsIgnoreFraming,
    )
    .ok_or_else(|| {
        format!(
            "截图失败（hwnd={}）。窗口可能已关闭，或缺少「屏幕录制」权限。",
            w.info.hwnd
        )
    })?;

    let iw = img.width() as u32;
    let ih = img.height() as u32;
    if iw == 0 || ih == 0 {
        return Err("截图结果为空（宽或高为 0）".to_string());
    }

    // CGImage 是 BGRA、premultiplied、**每行有 padding**：bytes_per_row >= width*4。
    // 不按 stride 逐行裁的话，宽度非 16 倍数时整张图会斜切。
    let data = img.data();
    let bytes = data.bytes();
    let stride = img.bytes_per_row();
    let need = (iw as usize) * 4;
    if stride < need || bytes.len() < stride * (ih as usize) {
        return Err(format!(
            "截图像素缓冲异常：stride={stride} 宽字节={need} 缓冲={} 高={ih}",
            bytes.len()
        ));
    }
    let mut buf = Vec::with_capacity((iw as usize) * (ih as usize) * 4);
    for row in 0..ih as usize {
        let start = row * stride;
        buf.extend_from_slice(&bytes[start..start + need]);
    }
    // BGRA → RGBA。
    for px in buf.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }

    let rgba =
        image::RgbaImage::from_raw(iw, ih, buf).ok_or_else(|| "像素缓冲构图失败".to_string())?;
    // 注意按「点」算目标尺寸，不是按像素——见函数头注释。
    let (ow, oh) = scaled_dims(pw, ph, MAX_EDGE);
    let out = if (ow, oh) != (iw, ih) {
        image::imageops::resize(&rgba, ow, oh, image::imageops::FilterType::Triangle)
    } else {
        rgba
    };
    let mut png: Vec<u8> = Vec::new();
    out.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("PNG 编码失败: {e}"))?;
    Ok((png, ow, oh))
}

pub struct CaptureWindow;

impl Tool for CaptureWindow {
    fn name(&self) -> &'static str {
        "capture_window"
    }
    fn description(&self) -> &'static str {
        "Capture a screenshot of the given window (by the hwnd from list_windows). Works on occluded/background windows. Returns the screenshot for you to view."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hwnd": { "type": "integer", "description": "Target window handle (the hwnd returned by list_windows)" }
            },
            "required": ["hwnd"]
        })
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "截图窗口 {}",
            args.get("hwnd").and_then(Value::as_i64).unwrap_or(0)
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let hwnd = hwnd_arg(args)?;
        let w = find_window(hwnd)?;
        let (png, ow, oh) = capture_window_png(&w)?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        let data_url = format!("data:image/png;base64,{b64}");
        let text = format!(
            "已截取窗口 hwnd={hwnd}，图像 {ow}x{oh}。点击/滚动请直接用这张图上的像素坐标。"
        );
        Ok(image_result(text, vec![data_url]))
    }
}

// ---------- 输入 ----------

fn hwnd_arg(args: &Value) -> Result<i64, String> {
    args.get("hwnd")
        .and_then(Value::as_i64)
        .ok_or_else(|| "缺少必填参数: hwnd（整数）".to_string())
}

fn req_i32(args: &Value, key: &str) -> Result<i32, String> {
    args.get(key)
        .and_then(Value::as_i64)
        .map(|v| v as i32)
        .ok_or_else(|| format!("缺少必填参数: {key}（整数）"))
}

/// 把模型给的「图像空间」坐标还原成**屏幕绝对坐标**（点）。
/// 比例由窗口尺寸就地重算，与 capture_window 用的是同一个纯函数。
fn img_point_to_screen(w: &MacWin, x: i32, y: i32) -> (i32, i32) {
    let (ox, oy, pw, ph) = w.info.rect;
    let scale = capture_scale(pw.max(1) as u32, ph.max(1) as u32, MAX_EDGE);
    let (wx, wy) = img_to_win(x, y, scale);
    (ox + wx, oy + wy)
}

pub struct WindowClick;

impl Tool for WindowClick {
    fn name(&self) -> &'static str {
        "window_click"
    }
    fn description(&self) -> &'static str {
        "Click inside the given window. Coordinates are read straight off the capture_window image — use the pixel position you see in that screenshot; this tool converts to real window coordinates itself. Brings the window's app to the foreground first, then clicks with a real mouse event."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hwnd": { "type": "integer", "description": "Target window handle" },
                "x": { "type": "integer", "description": "X as measured on the capture_window image" },
                "y": { "type": "integer", "description": "Y as measured on the capture_window image" },
                "button": { "type": "string", "enum": ["left", "right", "middle"], "description": "Defaults to left" },
                "double": { "type": "boolean", "description": "Whether to double-click; defaults to false" }
            },
            "required": ["hwnd", "x", "y"]
        })
    }
    fn requires_approval(&self) -> bool {
        true
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "点击窗口 {} ({},{})",
            args.get("hwnd").and_then(Value::as_i64).unwrap_or(0),
            args.get("x").and_then(Value::as_i64).unwrap_or(0),
            args.get("y").and_then(Value::as_i64).unwrap_or(0)
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        require_input_permission()?;
        let hwnd = hwnd_arg(args)?;
        let x = req_i32(args, "x")?;
        let y = req_i32(args, "y")?;
        let button = args.get("button").and_then(Value::as_str).unwrap_or("left");
        let double = args.get("double").and_then(Value::as_bool).unwrap_or(false);
        let w = find_window(hwnd)?;
        let (sx, sy) = img_point_to_screen(&w, x, y);
        focus_app(w.info.pid)?;
        input::run(InputKind::Click {
            x: sx,
            y: sy,
            button: button.to_string(),
            double,
        })?;
        Ok(format!(
            "已点击窗口 hwnd={hwnd} ({x},{y}) 键={button}{}",
            if double { " 双击" } else { "" }
        ))
    }
}

pub struct WindowType;

impl Tool for WindowType {
    fn name(&self) -> &'static str {
        "window_type"
    }
    fn description(&self) -> &'static str {
        "Type text into the given window. Brings the window's app to the foreground first, then sends the text as real keyboard input. Click the target field first if it is not already focused."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hwnd": { "type": "integer", "description": "Target window handle (brought to the foreground first)" },
                "text": { "type": "string", "description": "The text to type" }
            },
            "required": ["hwnd", "text"]
        })
    }
    fn requires_approval(&self) -> bool {
        true
    }
    fn summary(&self, _args: &Value) -> String {
        "输入文字".to_string()
    }
    fn execute(&self, args: &Value) -> ToolResult {
        require_input_permission()?;
        let hwnd = hwnd_arg(args)?;
        let text = require_str(args, "text")?;
        let w = find_window(hwnd)?;
        focus_app(w.info.pid)?;
        input::run(InputKind::Text(text.clone()))?;
        Ok(format!(
            "已在窗口 hwnd={hwnd} 输入 {} 个字符",
            text.chars().count()
        ))
    }
}

pub struct WindowKey;

impl Tool for WindowKey {
    fn name(&self) -> &'static str {
        "window_key"
    }
    fn description(&self) -> &'static str {
        "Send a key chord to the given window, e.g. \"cmd+s\" / \"enter\" / \"tab\". Brings the window's app to the foreground first. On macOS use cmd for the usual shortcuts (cmd+s, cmd+c); ctrl is a separate key here, unlike on Windows."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hwnd": { "type": "integer", "description": "Target window handle (brought to the foreground first)" },
                "keys": { "type": "string", "description": "Key chord, e.g. \"cmd+s\" / \"enter\" / \"tab\"" }
            },
            "required": ["hwnd", "keys"]
        })
    }
    fn requires_approval(&self) -> bool {
        true
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "按键 {}",
            args.get("keys").and_then(Value::as_str).unwrap_or("?")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        require_input_permission()?;
        let hwnd = hwnd_arg(args)?;
        let keys = require_str(args, "keys")?;
        // 先解析再置前：组合键写错时不该白白把窗口拉到前台。
        let (mods, main) = split_chord(&keys)?;
        let w = find_window(hwnd)?;
        focus_app(w.info.pid)?;
        input::run(InputKind::Chord { mods, main })?;
        Ok(format!("已在窗口 hwnd={hwnd} 发送组合键 {keys}"))
    }
}

pub struct WindowScroll;

impl Tool for WindowScroll {
    fn name(&self) -> &'static str {
        "window_scroll"
    }
    fn description(&self) -> &'static str {
        "Scroll the mouse wheel at a point in the given window. Coordinates are read straight off the capture_window image. amount positive = up / negative = down; direction: vertical (default) / horizontal."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hwnd": { "type": "integer", "description": "Target window handle (brought to the foreground first)" },
                "x": { "type": "integer", "description": "X as measured on the capture_window image (selects which area to scroll)" },
                "y": { "type": "integer", "description": "Y as measured on the capture_window image" },
                "amount": { "type": "integer", "description": "Number of wheel notches: positive = up/right, negative = down/left" },
                "direction": { "type": "string", "enum": ["vertical", "horizontal"], "description": "Defaults to vertical" }
            },
            "required": ["hwnd", "x", "y", "amount"]
        })
    }
    fn requires_approval(&self) -> bool {
        true
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "滚动窗口 {}",
            args.get("hwnd").and_then(Value::as_i64).unwrap_or(0)
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        require_input_permission()?;
        let hwnd = hwnd_arg(args)?;
        let x = req_i32(args, "x")?;
        let y = req_i32(args, "y")?;
        let amount = req_i32(args, "amount")?;
        let dir = args
            .get("direction")
            .and_then(Value::as_str)
            .unwrap_or("vertical");
        let w = find_window(hwnd)?;
        let (sx, sy) = img_point_to_screen(&w, x, y);
        focus_app(w.info.pid)?;
        input::run(InputKind::Scroll {
            x: sx,
            y: sy,
            amount,
            horizontal: dir == "horizontal",
        })?;
        Ok(format!(
            "已在窗口 hwnd={hwnd} ({x},{y}) 滚动 {amount}（{dir}）"
        ))
    }
}

#[cfg(test)]
mod smoke {
    //! 仅 macOS 编译运行（整模块 cfg）。**不假设有权限、也不假设有桌面**——
    //! CI 的 macos runner 既没有 TCC 授权也没有登录态 GUI 会话，所以这里只验
    //! 「不 panic、错误可读」，真正的功能验证只能在真机上人工做。
    use super::*;

    #[test]
    fn enumerate_does_not_panic() {
        // 无权限时也应安全返回（可能为空），不得 panic。
        let _ = enumerate_windows();
    }

    #[test]
    fn find_missing_window_errors_cleanly() {
        let e = find_window(0).unwrap_err();
        assert!(e.contains("未找到窗口"), "{e}");
    }

    #[test]
    fn capture_unknown_window_errors_not_panics() {
        let r = CaptureWindow.execute(&json!({ "hwnd": 1 }));
        assert!(r.is_err(), "不存在的窗口应报错而不是 panic");
    }

    #[test]
    fn missing_args_are_reported() {
        assert!(CaptureWindow.execute(&json!({})).is_err());
        assert!(WindowClick.execute(&json!({ "hwnd": 1 })).is_err());
    }

    #[test]
    fn img_point_maps_with_scale() {
        // 2560 宽的窗口会触发缩放：图心必须还原回窗口中心（含窗口原点偏移）。
        let w = MacWin {
            info: WinInfo {
                hwnd: 1,
                pid: 1,
                title: String::new(),
                rect: (100, 50, 2560, 1440),
            },
            title_missing: false,
        };
        let (iw, ih) = scaled_dims(2560, 1440, MAX_EDGE);
        let (sx, sy) = img_point_to_screen(&w, (iw / 2) as i32, (ih / 2) as i32);
        assert!((sx - (100 + 1280)).abs() <= 2, "x 还原错: {sx}");
        assert!((sy - (50 + 720)).abs() <= 2, "y 还原错: {sy}");
    }
}
