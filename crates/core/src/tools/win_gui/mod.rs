//! Windows 本地 GUI 控制工具（仅 #[cfg(windows)]）。
//! Phase 1：list_windows（EnumWindows）、capture_window（PrintWindow + GetDIBits）。
//! Phase 2：window_click/type/key/scroll（SendInput 真实输入；先置前再注入）。

use base64::Engine as _;
use serde_json::{json, Value};

use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
    HGDIOBJ, SRCCOPY,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindowVisible, SetCursorPos, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

use super::gui_input::parse_chord;
use super::{
    capture_scale, format_windows, image_result, img_to_win, require_str, scaled_dims, Tool,
    ToolResult, WinInfo, CAPTURE_MAX_EDGE,
};

/// PrintWindow 的 PW_RENDERFULLCONTENT（截后台/Chromium/3D 内容）。
const PW_RENDERFULLCONTENT: u32 = 0x0000_0002;
/// 视觉截图最长边上限（控 token）。与坐标换算共用 super::CAPTURE_MAX_EDGE，
/// 分开定义会让「图缩多少」和「坐标怎么还原」悄悄对不上。
const MAX_EDGE: u32 = CAPTURE_MAX_EDGE;

// ---------- 枚举 ----------

extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    unsafe {
        let out = &mut *(lparam.0 as *mut Vec<WinInfo>);
        if !IsWindowVisible(hwnd).as_bool() {
            return TRUE;
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return TRUE;
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let n = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..n as usize]);
        if title.trim().is_empty() {
            return TRUE;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        out.push(WinInfo {
            hwnd: hwnd.0 as isize,
            pid,
            title,
            rect: (
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            ),
        });
        TRUE
    }
}

fn enumerate_windows() -> Vec<WinInfo> {
    let mut out: Vec<WinInfo> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut out as *mut _ as isize));
    }
    out
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
        Ok(format_windows(&enumerate_windows()))
    }
}

// ---------- 截图 ----------

/// 对 hwnd 截图，返回 (png 字节, 输出宽, 输出高)。已等比缩到最长边 ≤ MAX_EDGE。
fn capture_window_png(hwnd: HWND) -> Result<(Vec<u8>, u32, u32), String> {
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).map_err(|e| format!("GetWindowRect 失败: {e}"))?;
        let w = (rect.right - rect.left).max(1);
        let h = (rect.bottom - rect.top).max(1);

        let screen = GetDC(None);
        let mem = CreateCompatibleDC(screen);
        let bmp: HBITMAP = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, HGDIOBJ(bmp.0));

        // PrintWindow 渲染整窗（含被遮挡内容）；失败再用 BitBlt 兜底前台内容。
        let ok = PrintWindow(hwnd, mem, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)).as_bool();
        if !ok {
            let _ = BitBlt(mem, 0, 0, w, h, screen, rect.left, rect.top, SRCCOPY);
        }

        // 取像素：top-down 32 位 BGRA。
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // 负 = top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let scan = GetDIBits(
            mem,
            bmp,
            0,
            h as u32,
            Some(buf.as_mut_ptr() as *mut _),
            &mut bi,
            DIB_RGB_COLORS,
        );

        // 清理 GDI。
        SelectObject(mem, old);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);

        if scan == 0 {
            return Err("GetDIBits 取像素失败".to_string());
        }

        // BGRA → RGBA。
        for px in buf.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }

        let img = image::RgbaImage::from_raw(w as u32, h as u32, buf)
            .ok_or_else(|| "像素缓冲构图失败".to_string())?;
        let (ow, oh) = scaled_dims(w as u32, h as u32, MAX_EDGE);
        let img = if (ow, oh) != (w as u32, h as u32) {
            image::imageops::resize(&img, ow, oh, image::imageops::FilterType::Triangle)
        } else {
            img
        };
        let mut png: Vec<u8> = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|e| format!("PNG 编码失败: {e}"))?;
        Ok((png, ow, oh))
    }
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
        let hwnd_i = args
            .get("hwnd")
            .and_then(Value::as_i64)
            .ok_or_else(|| "缺少必填参数: hwnd（整数）".to_string())?;
        let hwnd = HWND(hwnd_i as *mut core::ffi::c_void);
        let (png, w, h) = capture_window_png(hwnd)?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        let data_url = format!("data:image/png;base64,{b64}");
        // 直说图就是坐标基准：模型只需按图上像素报坐标，换算由工具侧负责。
        let text = format!(
            "已截取窗口 hwnd={hwnd_i}，图像 {w}x{h}。点击/滚动请直接用这张图上的像素坐标。"
        );
        Ok(image_result(text, vec![data_url]))
    }
}

// ---------- 输入（SendInput 真实输入） ----------

/// 取窗口句柄，返回 (HWND, 十进制值供回显)。
fn hwnd_arg(args: &Value) -> Result<(HWND, i64), String> {
    let i = args
        .get("hwnd")
        .and_then(Value::as_i64)
        .ok_or_else(|| "缺少必填参数: hwnd（整数）".to_string())?;
    Ok((HWND(i as *mut core::ffi::c_void), i))
}

/// 取必填整数参数。
fn req_i32(args: &Value, key: &str) -> Result<i32, String> {
    args.get(key)
        .and_then(Value::as_i64)
        .map(|v| v as i32)
        .ok_or_else(|| format!("缺少必填参数: {key}（整数）"))
}

/// 把目标窗口置前（SendInput 是全局的，必须前台才打得进去）。
unsafe fn focus_window(hwnd: HWND) {
    if IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_RESTORE);
    }
    let _ = SetForegroundWindow(hwnd);
}

/// 窗口屏幕矩形 (left, top, width, height)。宽高用于重算截图缩放比例。
unsafe fn window_rect(hwnd: HWND) -> Result<(i32, i32, i32, i32), String> {
    let mut rect = RECT::default();
    GetWindowRect(hwnd, &mut rect).map_err(|e| format!("GetWindowRect 失败: {e}"))?;
    Ok((
        rect.left,
        rect.top,
        (rect.right - rect.left).max(1),
        (rect.bottom - rect.top).max(1),
    ))
}

/// 把模型给的「图像空间」坐标还原成窗口坐标，并返回屏幕绝对坐标。
///
/// 模型只能照着 capture_window 给它的那张（可能被缩过的）图报坐标，所以工具这一侧必须
/// 负责还原。比例由窗口尺寸就地重算——与 capture_window 当时用的是同一个纯函数，
/// 因此无需把上次截图的状态存下来。
unsafe fn img_point_to_screen(hwnd: HWND, x: i32, y: i32) -> Result<(i32, i32), String> {
    let (ox, oy, w, h) = window_rect(hwnd)?;
    let scale = capture_scale(w as u32, h as u32, MAX_EDGE);
    let (wx, wy) = img_to_win(x, y, scale);
    Ok((ox + wx, oy + wy))
}

fn mouse_input(flags: MOUSE_EVENT_FLAGS, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn vk_input(vk: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn unicode_input(unit: u16, up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

unsafe fn send(inputs: &[INPUT]) {
    if !inputs.is_empty() {
        SendInput(inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

pub struct WindowClick;

impl Tool for WindowClick {
    fn name(&self) -> &'static str {
        "window_click"
    }
    fn description(&self) -> &'static str {
        "Click inside the given window. Coordinates are read straight off the capture_window image — use the pixel position you see in that screenshot; this tool converts to real window coordinates itself. Brings the window to the foreground first, then clicks with a real mouse event."
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
        let (hwnd, hwnd_i) = hwnd_arg(args)?;
        let x = req_i32(args, "x")?;
        let y = req_i32(args, "y")?;
        let button = args.get("button").and_then(Value::as_str).unwrap_or("left");
        let double = args.get("double").and_then(Value::as_bool).unwrap_or(false);
        let (down, up) = match button {
            "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
            "middle" => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
            _ => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        };
        unsafe {
            let (sx, sy) = img_point_to_screen(hwnd, x, y)?;
            focus_window(hwnd);
            SetCursorPos(sx, sy).map_err(|e| format!("SetCursorPos 失败: {e}"))?;
            let mut inputs = Vec::new();
            for _ in 0..if double { 2 } else { 1 } {
                inputs.push(mouse_input(down, 0));
                inputs.push(mouse_input(up, 0));
            }
            send(&inputs);
        }
        Ok(format!(
            "已点击窗口 hwnd={hwnd_i} ({x},{y}) 键={button}{}",
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
        "Type text into the focused window (supports Unicode/CJK). Brings the given window to the foreground first, then types character by character. Usually call window_click first to focus the input field."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hwnd": { "type": "integer", "description": "Target window handle (brought to the foreground first)" },
                "text": { "type": "string", "description": "Text to type" }
            },
            "required": ["hwnd", "text"]
        })
    }
    fn requires_approval(&self) -> bool {
        true
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "输入文字到窗口 {}",
            args.get("hwnd").and_then(Value::as_i64).unwrap_or(0)
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let (hwnd, hwnd_i) = hwnd_arg(args)?;
        let text = require_str(args, "text")?;
        unsafe {
            focus_window(hwnd);
            let mut inputs = Vec::new();
            let mut buf = [0u16; 2];
            for ch in text.chars() {
                for unit in ch.encode_utf16(&mut buf) {
                    inputs.push(unicode_input(*unit, false));
                    inputs.push(unicode_input(*unit, true));
                }
            }
            send(&inputs);
        }
        Ok(format!(
            "已向窗口 hwnd={hwnd_i} 输入 {} 个字符",
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
        "Send a key or key chord to the given window, e.g. ctrl+s, enter, tab, f5, ctrl+shift+p. Brings the window to the foreground first."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hwnd": { "type": "integer", "description": "Target window handle (brought to the foreground first)" },
                "keys": { "type": "string", "description": "Key chord, e.g. \"ctrl+s\" / \"enter\" / \"tab\"" }
            },
            "required": ["hwnd", "keys"]
        })
    }
    fn requires_approval(&self) -> bool {
        true
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "按键 {} → 窗口 {}",
            args.get("keys").and_then(Value::as_str).unwrap_or(""),
            args.get("hwnd").and_then(Value::as_i64).unwrap_or(0)
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let (hwnd, hwnd_i) = hwnd_arg(args)?;
        let spec = require_str(args, "keys")?;
        let chord = parse_chord(&spec)?;
        unsafe {
            focus_window(hwnd);
            let mut inputs = Vec::new();
            for m in &chord.modifiers {
                inputs.push(vk_input(*m, false));
            }
            inputs.push(vk_input(chord.main, false));
            inputs.push(vk_input(chord.main, true));
            for m in chord.modifiers.iter().rev() {
                inputs.push(vk_input(*m, true));
            }
            send(&inputs);
        }
        Ok(format!("已向窗口 hwnd={hwnd_i} 发送按键 {spec}"))
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
        let (hwnd, hwnd_i) = hwnd_arg(args)?;
        let x = req_i32(args, "x")?;
        let y = req_i32(args, "y")?;
        let amount = req_i32(args, "amount")?;
        let dir = args
            .get("direction")
            .and_then(Value::as_str)
            .unwrap_or("vertical");
        let flag = if dir == "horizontal" {
            MOUSEEVENTF_HWHEEL
        } else {
            MOUSEEVENTF_WHEEL
        };
        unsafe {
            let (sx, sy) = img_point_to_screen(hwnd, x, y)?;
            focus_window(hwnd);
            SetCursorPos(sx, sy).map_err(|e| format!("SetCursorPos 失败: {e}"))?;
            send(&[mouse_input(flag, amount * 120)]);
        }
        Ok(format!(
            "已在窗口 hwnd={hwnd_i} ({x},{y}) 滚动 {amount}（{dir}）"
        ))
    }
}

#[cfg(test)]
mod smoke {
    // 仅 Windows 编译运行（整模块 #[cfg(windows)]），不进 Linux CI。需真实桌面。
    use super::*;

    #[test]
    fn enumerate_returns_windows() {
        let ws = enumerate_windows();
        assert!(!ws.is_empty(), "应至少枚举到一个可见窗口");
    }

    #[test]
    fn capture_first_window_produces_png() {
        let ws = enumerate_windows();
        let Some(w) = ws.iter().find(|w| w.rect.2 > 0 && w.rect.3 > 0) else {
            return; // 没有有尺寸的窗口，跳过
        };
        let hwnd = HWND(w.hwnd as *mut core::ffi::c_void);
        let (png, ow, oh) = capture_window_png(hwnd).expect("截图应成功");
        assert!(ow > 0 && oh > 0);
        assert!(
            png.len() > 8 && &png[..8] == b"\x89PNG\r\n\x1a\n",
            "应是合法 PNG"
        );
    }
}
