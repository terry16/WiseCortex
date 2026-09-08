//! macOS 输入合成（enigo）与窗口置前（osascript）。
//!
//! 为什么用 enigo 而不是手写 CGEvent：CGEvent 有一串不看源码就写不对的约定——
//! CGEventSource 按值传入故循环里必须 clone、事件之间要有间隔否则会被丢弃、
//! Unicode 字符串要分块、CGEventFlags 的常量名是 CGEventFlagCommand 而非 COMMAND。
//! enigo 已经把这些都趟平了，并且它用的也是左上角原点，与我们的坐标约定一致。

use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard as _, Mouse as _, Settings};

/// 一次输入动作。把 enigo 关在这一层后面，工具层就不必直接依赖它的类型。
pub enum InputKind {
    Click {
        x: i32,
        y: i32,
        button: String,
        double: bool,
    },
    Text(String),
    Chord {
        mods: Vec<String>,
        main: String,
    },
    Scroll {
        x: i32,
        y: i32,
        amount: i32,
        horizontal: bool,
    },
}

/// 修饰键名 → enigo::Key。别名与 Windows 侧保持一致（gui_input::is_modifier_name）。
/// macOS 上 cmd 与 ctrl 是两个不同的键，不像 Windows 那样 ctrl 兜住大部分快捷键。
fn modifier_key(name: &str) -> Option<Key> {
    match name {
        "ctrl" | "control" => Some(Key::Control),
        "shift" => Some(Key::Shift),
        "alt" | "option" => Some(Key::Alt),
        "win" | "meta" | "cmd" | "command" | "super" => Some(Key::Meta),
        _ => None,
    }
}

/// 具名主键 → enigo::Key。认不出的留给调用方按单字符处理。
fn named_key(lower: &str) -> Option<Key> {
    let k = match lower {
        "enter" | "return" => Key::Return,
        "tab" => Key::Tab,
        "esc" | "escape" => Key::Escape,
        "space" => Key::Space,
        "backspace" | "back" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" => Key::PageDown,
        "up" => Key::UpArrow,
        "down" => Key::DownArrow,
        "left" => Key::LeftArrow,
        "right" => Key::RightArrow,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        _ => return None,
    };
    Some(k)
}

/// 主键（具名键或单个字符）→ enigo::Key。
fn main_key(spec: &str) -> Result<Key, String> {
    let lower = spec.to_ascii_lowercase();
    if let Some(k) = named_key(&lower) {
        return Ok(k);
    }
    let mut chars = spec.chars();
    let c = chars.next().ok_or_else(|| "空主键".to_string())?;
    if chars.next().is_some() {
        return Err(format!("无法识别的键：{spec}"));
    }
    // Unicode(小写) —— 大写需要模型自己带 shift，语义更明确。
    Ok(Key::Unicode(c.to_ascii_lowercase()))
}

fn mouse_button(name: &str) -> Button {
    match name {
        "right" => Button::Right,
        "middle" => Button::Middle,
        _ => Button::Left,
    }
}

/// 执行一次输入。
///
/// 每次都新建 Enigo：它在 Drop 里会做必要的收尾（含刷事件的等待），复用一个长命
/// 实例反而要自己操心线程归属。输入不是高频操作，这点开销无所谓。
pub fn run(kind: InputKind) -> Result<(), String> {
    let mut e = Enigo::new(&Settings::default())
        .map_err(|err| format!("初始化输入失败: {err}（通常是缺少「辅助功能」授权）"))?;
    match kind {
        InputKind::Click {
            x,
            y,
            button,
            double,
        } => {
            e.move_mouse(x, y, Coordinate::Abs)
                .map_err(|err| format!("移动鼠标失败: {err}"))?;
            let b = mouse_button(&button);
            let times = if double { 2 } else { 1 };
            for _ in 0..times {
                e.button(b, Direction::Click)
                    .map_err(|err| format!("点击失败: {err}"))?;
            }
        }
        InputKind::Text(t) => {
            e.text(&t).map_err(|err| format!("输入文字失败: {err}"))?;
        }
        InputKind::Chord { mods, main } => {
            let mut held = Vec::new();
            for m in &mods {
                let k = modifier_key(m).ok_or_else(|| format!("无法识别的修饰键：{m}"))?;
                held.push(k);
            }
            let mk = main_key(&main)?;
            // 按下全部修饰键 → 敲主键 → **逆序**释放，避免修饰键卡住。
            for k in &held {
                e.key(*k, Direction::Press)
                    .map_err(|err| format!("按下修饰键失败: {err}"))?;
            }
            let hit = e.key(mk, Direction::Click);
            for k in held.iter().rev() {
                // 主键失败也必须把修饰键放掉，否则系统会一直以为 cmd 还按着。
                let _ = e.key(*k, Direction::Release);
            }
            hit.map_err(|err| format!("按键失败: {err}"))?;
        }
        InputKind::Scroll {
            x,
            y,
            amount,
            horizontal,
        } => {
            e.move_mouse(x, y, Coordinate::Abs)
                .map_err(|err| format!("移动鼠标失败: {err}"))?;
            let axis = if horizontal {
                Axis::Horizontal
            } else {
                Axis::Vertical
            };
            // enigo 的正向 = 向下/向右，与我们「正=上/右」的约定相反，故取反。
            e.scroll(-amount, axis)
                .map_err(|err| format!("滚动失败: {err}"))?;
        }
    }
    Ok(())
}

/// 把 pid 对应的 App 置前。
///
/// macOS 没有 SetForegroundWindow 的等价物：公开途径要么走 AX
/// （kAXRaiseAction，但 AX 只能在主线程调），要么走 NSRunningApplication.activate
/// （需要 AppKit）。这里用 osascript 调 System Events，代价是起个子进程，
/// 好处是不引入 AX 的线程约束、也不引入 objc 依赖——本阶段刻意不碰 AX。
///
/// 注意：这一步需要「自动化」权限（与「辅助功能」是两个独立开关）。失败不致命——
/// 目标窗口可能本来就在前台，所以只在真正出错时返回错误，不做前置阻断。
pub fn focus_app(pid: u32) -> Result<(), String> {
    let script = format!(
        "tell application \"System Events\" to set frontmost of (first process whose unix id is {pid}) to true"
    );
    let out = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("启动 osascript 失败: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let err = err.trim();
    // -1743 = 未获「自动化」授权。指路而不是只回一串错误码。
    if err.contains("-1743") || err.to_lowercase().contains("not allowed") {
        return Err(format!(
            "置前失败：未获得「自动化」授权。请到 系统设置 → 隐私与安全性 → 自动化，\
             允许 wisecortex（或你的终端 App）控制「系统事件(System Events)」。原始错误：{err}"
        ));
    }
    Err(format!("置前失败（pid={pid}）：{err}"))
}

#[cfg(test)]
mod tests {
    //! 这里只测**纯映射**，不实际发事件（CI 无权限、也无 GUI 会话）。
    use super::*;
    use crate::tools::gui_input::split_chord;

    #[test]
    fn chord_maps_modifiers_and_main() {
        let (mods, main) = split_chord("cmd+shift+p").unwrap();
        assert_eq!(mods, vec!["cmd", "shift"]);
        assert!(modifier_key(&mods[0]).is_some());
        assert!(modifier_key(&mods[1]).is_some());
        assert!(matches!(main_key(&main).unwrap(), Key::Unicode('p')));
    }

    #[test]
    fn named_keys_resolve() {
        assert!(matches!(main_key("enter").unwrap(), Key::Return));
        assert!(matches!(main_key("F5").unwrap(), Key::F5));
        assert!(matches!(main_key("Tab").unwrap(), Key::Tab));
    }

    #[test]
    fn cmd_and_ctrl_are_distinct_on_macos() {
        // macOS 上这俩是不同的键；映到同一个就会让 cmd+s 变成 ctrl+s（不保存）。
        let cmd = modifier_key("cmd").unwrap();
        let ctrl = modifier_key("ctrl").unwrap();
        assert!(matches!(cmd, Key::Meta));
        assert!(matches!(ctrl, Key::Control));
    }

    #[test]
    fn bad_main_key_is_rejected() {
        assert!(main_key("nope").is_err());
        assert!(main_key("").is_err());
    }

    #[test]
    fn buttons_map_with_left_default() {
        assert!(matches!(mouse_button("right"), Button::Right));
        assert!(matches!(mouse_button("middle"), Button::Middle));
        assert!(matches!(mouse_button("bogus"), Button::Left));
    }
}
