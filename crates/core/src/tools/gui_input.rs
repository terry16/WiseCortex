//! GUI 输入的纯逻辑（跨平台、可单测）：组合键解析 + 虚拟键码映射 + PowerShell 编码。
//! Win32 SendInput 的实际注入在 `win_gui`（仅 cfg(windows)）里调用这里的结果。

use base64::Engine as _;

/// 把脚本编码成 PowerShell `-EncodedCommand` 期望的格式（UTF-16LE + Base64）。
/// UIA 工具用它传**静态**脚本，模型给的可变值另经环境变量传入，彻底避免脚本注入。
pub fn encode_ps_command(script: &str) -> String {
    let utf16le: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(utf16le)
}

/// 解析后的组合键：修饰键 VK 列表（按下顺序）+ 主键 VK。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChord {
    pub modifiers: Vec<u16>,
    pub main: u16,
}

/// 修饰键名 → VK。ctrl/shift/alt/win 及常见别名。
fn modifier_vk(name: &str) -> Option<u16> {
    match name {
        "ctrl" | "control" => Some(0x11),
        "shift" => Some(0x10),
        "alt" | "option" => Some(0x12),
        "win" | "meta" | "cmd" | "command" | "super" => Some(0x5B),
        _ => None,
    }
}

/// 具名按键 → VK（非字母数字的功能键/编辑键/方向键/F 键）。
pub fn named_vk(name: &str) -> Option<u16> {
    let vk = match name {
        "enter" | "return" => 0x0D,
        "tab" => 0x09,
        "esc" | "escape" => 0x1B,
        "space" => 0x20,
        "backspace" | "back" => 0x08,
        "delete" | "del" => 0x2E,
        "insert" | "ins" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" | "pgup" => 0x21,
        "pagedown" | "pgdn" => 0x22,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "f1" => 0x70,
        "f2" => 0x71,
        "f3" => 0x72,
        "f4" => 0x73,
        "f5" => 0x74,
        "f6" => 0x75,
        "f7" => 0x76,
        "f8" => 0x77,
        "f9" => 0x78,
        "f10" => 0x79,
        "f11" => 0x7A,
        "f12" => 0x7B,
        _ => return None,
    };
    Some(vk)
}

/// 组合键的**平台无关**拆分：`"ctrl+shift+p"` → (["ctrl","shift"], "p")。
///
/// 只做分词与规则校验，不碰任何平台键码——因此 Windows(VK) 与 macOS(enigo::Key)
/// 可以共用这一层，而这层的单测在所有平台都跑得到。
/// 修饰键名一律小写归一；主键保留原样（大小写留给各平台自己处理）。
pub fn split_chord(spec: &str) -> Result<(Vec<String>, String), String> {
    let parts: Vec<&str> = spec
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return Err("空组合键".to_string());
    }
    let mut modifiers = Vec::new();
    let mut main: Option<String> = None;
    for p in parts {
        let lower = p.to_ascii_lowercase();
        if is_modifier_name(&lower) {
            modifiers.push(lower);
            continue;
        }
        if main.is_some() {
            return Err(format!("组合键最多一个主键：{spec}"));
        }
        main = Some(p.to_string());
    }
    let main = main.ok_or_else(|| format!("组合键缺少主键：{spec}"))?;
    Ok((modifiers, main))
}

/// 是否是修饰键名（各平台共用同一套别名，免得两边认的写法不一致）。
pub fn is_modifier_name(lower: &str) -> bool {
    matches!(
        lower,
        "ctrl"
            | "control"
            | "shift"
            | "alt"
            | "option"
            | "win"
            | "meta"
            | "cmd"
            | "command"
            | "super"
    )
}

/// 把 "ctrl+s" / "enter" / "ctrl+shift+p" 解析成修饰键 + 单个主键。
/// 主键：具名键用其 VK；单个 ASCII 字母/数字用其大写 ASCII 码（= 该键 VK）。
pub fn parse_chord(spec: &str) -> Result<KeyChord, String> {
    let parts: Vec<&str> = spec
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return Err("空组合键".to_string());
    }
    let mut modifiers = Vec::new();
    let mut main: Option<u16> = None;
    for p in parts {
        let lower = p.to_ascii_lowercase();
        if let Some(m) = modifier_vk(&lower) {
            modifiers.push(m);
            continue;
        }
        let vk = if let Some(v) = named_vk(&lower) {
            v
        } else {
            let mut chars = p.chars();
            let c = chars.next().unwrap();
            if chars.next().is_some() {
                return Err(format!("无法识别的键：{p}"));
            }
            let u = c.to_ascii_uppercase();
            if !u.is_ascii_alphanumeric() {
                return Err(format!("无法识别的键：{p}"));
            }
            u as u16
        };
        if main.is_some() {
            return Err(format!("组合键最多一个主键：{spec}"));
        }
        main = Some(vk);
    }
    let main = main.ok_or_else(|| format!("组合键缺少主键：{spec}"))?;
    Ok(KeyChord { modifiers, main })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// split_chord 是 Windows(VK) 与 macOS(enigo::Key) 共用的分词层，
    /// 所以它的用例在两个平台上都会跑到——正是「我编译不了 macOS」时最该加固的地方。
    #[test]
    fn split_chord_separates_modifiers_from_main() {
        assert_eq!(
            split_chord("ctrl+shift+p").unwrap(),
            (
                vec!["ctrl".to_string(), "shift".to_string()],
                "p".to_string()
            )
        );
        assert_eq!(split_chord("enter").unwrap(), (vec![], "enter".to_string()));
        // 修饰键名归一到小写，主键保留原样（大小写留给各平台处理）。
        assert_eq!(
            split_chord("CMD+S").unwrap(),
            (vec!["cmd".to_string()], "S".to_string())
        );
        // 空白与多余的 + 应被容忍。
        assert_eq!(
            split_chord("  alt +  tab ").unwrap(),
            (vec!["alt".to_string()], "tab".to_string())
        );
    }

    #[test]
    fn split_chord_rejects_bad_input() {
        assert!(split_chord("").is_err(), "空串");
        assert!(split_chord("+++").is_err(), "只有分隔符");
        assert!(split_chord("ctrl").is_err(), "只有修饰键、缺主键");
        assert!(split_chord("a+b").is_err(), "两个主键");
    }

    /// 回归：两份修饰键别名表必须一致。任一边加了别名而另一边没加，
    /// 结果会是「Windows 认 super、macOS 不认」这类只在单平台复现的怪问题。
    #[test]
    fn modifier_name_list_matches_vk_table() {
        for n in [
            "ctrl", "control", "shift", "alt", "option", "win", "meta", "cmd", "command", "super",
        ] {
            assert!(is_modifier_name(n), "{n} 应被认作修饰键");
            assert!(modifier_vk(n).is_some(), "{n} 应有对应 VK");
        }
        for n in ["s", "enter", "f5", "nope"] {
            assert!(!is_modifier_name(n), "{n} 不该被认作修饰键");
            assert!(modifier_vk(n).is_none(), "{n} 不该有修饰键 VK");
        }
    }

    #[test]
    fn parses_ctrl_letter() {
        assert_eq!(
            parse_chord("ctrl+s"),
            Ok(KeyChord {
                modifiers: vec![0x11],
                main: 0x53
            })
        );
    }

    #[test]
    fn parses_named_key_no_modifier() {
        assert_eq!(
            parse_chord("enter"),
            Ok(KeyChord {
                modifiers: vec![],
                main: 0x0D
            })
        );
        assert_eq!(
            parse_chord("F5"),
            Ok(KeyChord {
                modifiers: vec![],
                main: 0x74
            })
        );
    }

    #[test]
    fn parses_multi_modifier() {
        assert_eq!(
            parse_chord("ctrl+shift+p"),
            Ok(KeyChord {
                modifiers: vec![0x11, 0x10],
                main: 0x50
            })
        );
    }

    #[test]
    fn case_insensitive_and_trimmed() {
        assert_eq!(
            parse_chord(" Ctrl + A "),
            Ok(KeyChord {
                modifiers: vec![0x11],
                main: 0x41
            })
        );
    }

    #[test]
    fn rejects_empty() {
        assert!(parse_chord("").is_err());
        assert!(parse_chord("   ").is_err());
    }

    #[test]
    fn rejects_modifier_only() {
        assert!(parse_chord("ctrl").is_err());
        assert!(parse_chord("ctrl+shift").is_err());
    }

    #[test]
    fn rejects_two_main_keys() {
        assert!(parse_chord("a+b").is_err());
        assert!(parse_chord("ctrl+a+enter").is_err());
    }

    #[test]
    fn rejects_unknown_multichar() {
        assert!(parse_chord("ctrl+foo").is_err());
    }

    #[test]
    fn encode_ps_roundtrips_utf16le() {
        let script = "Write-Output $env:WC_X  # 中文注释";
        let enc = encode_ps_command(script);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&enc)
            .unwrap();
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        assert_eq!(String::from_utf16(&units).unwrap(), script);
    }
}
