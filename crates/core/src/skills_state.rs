//! 技能启用 / 停用状态。
//!
//! 技能本身是磁盘上的 SKILL.md；是否「启用」是一份独立的状态，存
//! `<data_dir>/wisecortex/skills-state.json`：
//! ```json
//! { "disabled": ["pr-writer", "refactorer"] }
//! ```
//! 停用的技能：不进 system prompt 的可用技能清单、也不能被 invoke_skill 调用，
//! 但仍保留在磁盘上、仍在技能列表里显示（带停用态），可随时再启用。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 落盘的技能状态。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkillsState {
    /// 已停用的技能名。
    #[serde(default)]
    pub disabled: Vec<String>,
}

impl SkillsState {
    /// 设置某技能启用/停用，返回是否有变化。
    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> bool {
        let present = self.disabled.iter().any(|n| n == name);
        match (enabled, present) {
            // 启用 = 从 disabled 移除。
            (true, true) => {
                self.disabled.retain(|n| n != name);
                true
            }
            // 停用 = 加入 disabled。
            (false, false) => {
                self.disabled.push(name.to_string());
                true
            }
            _ => false,
        }
    }

    pub fn is_disabled(&self, name: &str) -> bool {
        self.disabled.iter().any(|n| n == name)
    }
}

/// 状态文件路径：`<data_dir>/wisecortex/skills-state.json`。
pub fn state_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("skills-state.json"))
}

/// 从指定路径加载（损坏/不存在 → 默认空）。
pub fn load_from(path: &Path) -> SkillsState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// 写入指定路径（自动建父目录）。
pub fn save_to(path: &Path, state: &SkillsState) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(state)?)
}

/// 从标准路径加载（无数据目录 → 默认空）。
pub fn load() -> SkillsState {
    state_path().map(|p| load_from(&p)).unwrap_or_default()
}

/// 当前停用的技能名集合（便捷）。
pub fn disabled() -> Vec<String> {
    load().disabled
}

/// 设置某技能启用/停用并落盘。
pub fn set_enabled(name: &str, enabled: bool) -> std::io::Result<()> {
    let path = state_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "无数据目录"))?;
    let mut state = load_from(&path);
    if state.set_enabled(name, enabled) {
        save_to(&path, &state)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_enabled_toggles_disabled_list() {
        let mut s = SkillsState::default();
        assert!(!s.is_disabled("a"));
        // 停用 a。
        assert!(s.set_enabled("a", false));
        assert!(s.is_disabled("a"));
        // 重复停用无变化。
        assert!(!s.set_enabled("a", false));
        // 启用 a。
        assert!(s.set_enabled("a", true));
        assert!(!s.is_disabled("a"));
        // 重复启用无变化。
        assert!(!s.set_enabled("a", true));
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = std::env::temp_dir().join(format!("wc-skstate-{}", std::process::id()));
        let path = dir.join("skills-state.json");
        let mut s = SkillsState::default();
        s.set_enabled("pr-writer", false);
        save_to(&path, &s).unwrap();
        let back = load_from(&path);
        assert!(back.is_disabled("pr-writer"));
        assert_eq!(back.disabled.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_yields_empty() {
        let path = std::env::temp_dir().join("wc-skstate-nope-xyz.json");
        assert_eq!(load_from(&path), SkillsState::default());
    }
}
