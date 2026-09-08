//! openclaw 技能迁移：探测常见 openclaw 技能位置，列出候选；导入复用 [`crate::marketplace::import_dir`]。
//!
//! openclaw 技能与 WiseCortex 同为 `SKILL.md` 目录，故迁移=复制目录。探测覆盖 openclaw 常见路径
//! （`~/.config/openclaw`、`~/.openclaw`、macOS Application Support、工作区 `.agents/skills` 等）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 一个可迁移候选。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MigrationCandidate {
    /// 技能名（目录名）。
    pub name: String,
    /// 技能目录绝对路径。
    pub path: String,
    /// 本地是否已存在同名技能。
    pub installed: bool,
}

/// openclaw 技能可能存在的根目录。
fn candidate_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(h) = dirs::home_dir() {
        roots.push(h.join(".config").join("openclaw"));
        roots.push(h.join(".openclaw"));
        roots.push(
            h.join("Library")
                .join("Application Support")
                .join("openclaw"),
        );
        roots.push(h.join(".agents").join("skills"));
    }
    if let Some(d) = dirs::data_dir() {
        roots.push(d.join("openclaw"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd.join(".agents").join("skills"));
        roots.push(cwd.join("skills"));
    }
    roots
}

/// 探测可迁移技能（去重、标注是否已安装）。
pub fn scan() -> Vec<MigrationCandidate> {
    let installed: BTreeSet<String> = crate::marketplace::installed().into_iter().collect();
    let own = crate::marketplace::skills_dir();
    scan_roots(&candidate_roots(), own.as_deref(), &installed)
}

/// 探测核心（可注入根目录 / 自身目录 / 已安装集合，便于测试）。
pub fn scan_roots(
    roots: &[PathBuf],
    own_skills_dir: Option<&Path>,
    installed: &BTreeSet<String>,
) -> Vec<MigrationCandidate> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(root)
            .max_depth(4)
            .into_iter()
            .flatten()
        {
            if entry.file_name() != "SKILL.md" || !entry.path().is_file() {
                continue;
            }
            let Some(dir) = entry.path().parent() else {
                continue;
            };
            // 跳过 WiseCortex 自己的技能目录（避免把自己列为可迁移）。
            if let Some(own) = own_skills_dir {
                if dir.starts_with(own) {
                    continue;
                }
            }
            let Some(name) = dir.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !seen.insert(name.to_string()) {
                continue; // 同名只取一个
            }
            out.push(MigrationCandidate {
                name: name.to_string(),
                path: dir.to_string_lossy().to_string(),
                installed: installed.contains(name),
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// 导入选定的技能目录到数据目录，返回 (path, 是否成功)。
pub fn import(paths: &[String]) -> Vec<(String, bool)> {
    let Some(dest) = crate::marketplace::skills_dir() else {
        return paths.iter().map(|p| (p.clone(), false)).collect();
    };
    paths
        .iter()
        .map(|p| {
            let names = crate::marketplace::import_dir(Path::new(p), &dest);
            (p.clone(), !names.is_empty())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_finds_skill_dirs_and_marks_installed() {
        let base = std::env::temp_dir().join(format!("wc-ocmig-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        // 造 openclaw 风格 .agents/skills/<name>/SKILL.md
        let root = base.join(".agents").join("skills");
        for name in ["weather", "sql-helper"] {
            let d = root.join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("SKILL.md"), format!("---\nname: {name}\n---\n正文")).unwrap();
        }
        // 非技能目录忽略。
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs").join("readme.md"), "x").unwrap();

        let installed: BTreeSet<String> = ["weather".to_string()].into_iter().collect();
        let cands = scan_roots(std::slice::from_ref(&root), None, &installed);
        let names: Vec<&str> = cands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["sql-helper", "weather"]);
        assert!(
            cands
                .iter()
                .find(|c| c.name == "weather")
                .unwrap()
                .installed
        );
        assert!(
            !cands
                .iter()
                .find(|c| c.name == "sql-helper")
                .unwrap()
                .installed
        );

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn scan_skips_own_skills_dir() {
        let base = std::env::temp_dir().join(format!("wc-ocmig-own-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        let own = base.join("wisecortex").join("skills");
        let d = own.join("mine");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("SKILL.md"), "---\nname: mine\n---\nx").unwrap();

        let cands = scan_roots(std::slice::from_ref(&own), Some(&own), &BTreeSet::new());
        assert!(cands.is_empty(), "自身技能目录不应被列为可迁移");
        std::fs::remove_dir_all(&base).ok();
    }
}
