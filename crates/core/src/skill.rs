//! 技能（Skill）系统：把可复用的"playbook"做成带元数据的 Markdown（SKILL.md），
//! agent 通过 invoke_skill 工具按需拉取其指令来执行任务。
//!
//! SKILL.md 格式（frontmatter + 正文）：
//! ```text
//! ---
//! name: code-reviewer
//! description: 审查代码改动，给出问题与建议
//! ---
//! # 正文即技能指令（agent 读到后照做）
//! ```
//!
//! 加载来源：项目目录 `./skills/*/SKILL.md` 与数据目录 `wisecortex/skills/*/SKILL.md`。

use std::path::{Path, PathBuf};

/// 一个技能。
#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// 正文指令（SKILL.md frontmatter 之后的内容）。
    pub instructions: String,
    /// 技能所在目录（用于解析正文里的 `@file` 引用）；parse 单独调用时为 None。
    pub dir: Option<PathBuf>,
}

/// 解析 SKILL.md 内容。`fallback_name` 在 frontmatter 无 name 时用（通常是目录名）。
pub fn parse(content: &str, fallback_name: &str) -> Skill {
    let normalized = content.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.lines().collect();

    if lines.first() == Some(&"---") {
        // 找 frontmatter 结束的 ---
        let mut i = 1;
        let mut fm: Vec<&str> = Vec::new();
        while i < lines.len() && lines[i] != "---" {
            fm.push(lines[i]);
            i += 1;
        }
        let body = lines.get(i + 1..).map(|s| s.join("\n")).unwrap_or_default();
        let (name, description) = parse_frontmatter(&fm, fallback_name);
        return Skill {
            name,
            description,
            instructions: body.trim().to_string(),
            dir: None,
        };
    }

    Skill {
        name: fallback_name.to_string(),
        description: String::new(),
        instructions: normalized.trim().to_string(),
        dir: None,
    }
}

/// 把正文里的 `@file` 引用内联：对每个能在 `base_dir` 下解析到的文件，
/// 在末尾追加其内容。只内联 base_dir 内的相对路径文件（拒绝 `..`/绝对路径，防越权）。
pub fn expand_references(text: &str, base_dir: &Path) -> String {
    use std::collections::BTreeSet;
    let re = regex::Regex::new(r"@([A-Za-z0-9_./\\-]+)").unwrap();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut inlined: Vec<(String, String)> = Vec::new();
    for cap in re.captures_iter(text) {
        let mut path = cap[1].to_string();
        // 去掉尾随标点（如 "@file.md," / "file.md."）。
        while path.ends_with(['.', ',', ';', ':', ')', ']']) {
            path.pop();
        }
        // 需含扩展名点；过滤掉 @mention 之类。
        if !path.contains('.') {
            continue;
        }
        // 防越权：拒绝 `..`、绝对路径、Windows 盘符。
        if path.contains("..") || path.starts_with('/') || path.starts_with('\\') {
            continue;
        }
        if seen.contains(&path) {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(base_dir.join(&path)) {
            seen.insert(path.clone());
            inlined.push((path, content));
        }
    }
    if inlined.is_empty() {
        return text.to_string();
    }
    let mut out = text.to_string();
    out.push_str("\n\n---\n## 引用文件内容（@ 自动内联）\n");
    for (path, content) in inlined {
        out.push_str(&format!("\n### @{path}\n{}\n", content.trim_end()));
    }
    out
}

fn parse_frontmatter(fm: &[&str], fallback_name: &str) -> (String, String) {
    let mut name = fallback_name.to_string();
    let mut description = String::new();
    let mut j = 0;
    while j < fm.len() {
        let line = fm[j];
        if let Some(v) = line.strip_prefix("name:") {
            name = v.trim().trim_matches(['"', '\'']).to_string();
            j += 1;
        } else if let Some(v) = line.strip_prefix("description:") {
            let v = v.trim();
            if v == "|" || v == ">" || v.is_empty() {
                // 收集后续缩进行作为多行描述。
                j += 1;
                let mut parts = Vec::new();
                while j < fm.len() && (fm[j].starts_with(' ') || fm[j].starts_with('\t')) {
                    parts.push(fm[j].trim());
                    j += 1;
                }
                description = parts.join(" ");
            } else {
                description = v.trim_matches(['"', '\'']).to_string();
                j += 1;
            }
        } else {
            j += 1;
        }
    }
    (name, description)
}

/// 已加载的技能集合。
#[derive(Debug, Clone, Default)]
pub struct SkillSet {
    skills: Vec<Skill>,
}

impl SkillSet {
    /// 从多个目录加载技能（每个子目录一个 `SKILL.md`）。重名以先加载的为准。
    pub fn load_dirs(dirs: &[std::path::PathBuf]) -> Self {
        let mut set = SkillSet::default();
        for dir in dirs {
            set.load_dir(dir);
        }
        set
    }

    fn load_dir(&mut self, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let sub = e.path();
            if !sub.is_dir() {
                continue;
            }
            let md = sub.join("SKILL.md");
            let Ok(content) = std::fs::read_to_string(&md) else {
                continue;
            };
            let fallback = sub.file_name().and_then(|s| s.to_str()).unwrap_or("skill");
            let mut skill = parse(&content, fallback);
            skill.dir = Some(sub.clone());
            if !self.skills.iter().any(|s| s.name == skill.name) {
                self.skills.push(skill);
            }
        }
        self.skills.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// 去掉停用的技能（按名）。返回 self 以便链式调用。
    /// 停用技能既不进 `prompt_list`，也无法被 `find`（即 invoke_skill）找到。
    pub fn without_disabled(mut self, disabled: &[String]) -> Self {
        if !disabled.is_empty() {
            self.skills
                .retain(|s| !disabled.iter().any(|d| d == &s.name));
        }
        self
    }

    /// 只保留指定名字的技能（任务钉选技能用）。空列表=不过滤（保持全量）。
    /// 钉选后该技能集既只在 `prompt_list` 列出这几个，`find`/invoke_skill 也只在这几个里查。
    pub fn only(mut self, names: &[String]) -> Self {
        if !names.is_empty() {
            self.skills.retain(|s| names.iter().any(|n| n == &s.name));
        }
        self
    }

    pub fn find(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn names(&self) -> Vec<&str> {
        self.skills.iter().map(|s| s.name.as_str()).collect()
    }

    /// (名称, 描述) 列表（CLI 展示用）。
    pub fn entries(&self) -> Vec<(&str, &str)> {
        self.skills
            .iter()
            .map(|s| (s.name.as_str(), s.description.as_str()))
            .collect()
    }

    /// 注入 system prompt 的可用技能清单（空集合返回空串）。
    pub fn prompt_list(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        let mut out = String::from(
            "AVAILABLE SKILLS (call invoke_skill with the skill_name to load its instructions):\n",
        );
        for s in &self.skills {
            let desc = s.description.lines().next().unwrap_or("").trim();
            out.push_str(&format!("- {}: {}\n", s.name, desc));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_and_body() {
        let md = "---\nname: code-reviewer\ndescription: 审查代码\n---\n# 指令\n做审查";
        let s = parse(md, "fallback");
        assert_eq!(s.name, "code-reviewer");
        assert_eq!(s.description, "审查代码");
        assert!(s.instructions.contains("做审查"));
    }

    #[test]
    fn parses_multiline_description() {
        let md = "---\nname: x\ndescription: |\n  line one\n  line two\n---\nbody";
        let s = parse(md, "f");
        assert_eq!(s.description, "line one line two");
    }

    #[test]
    fn no_frontmatter_uses_fallback() {
        let s = parse("just instructions", "mydir");
        assert_eq!(s.name, "mydir");
        assert_eq!(s.instructions, "just instructions");
    }

    #[test]
    fn expand_references_inlines_existing_files_only() {
        let root = std::env::temp_dir().join(format!("wc-expand-{}", std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        let dir = root.join("skill");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.md"), "INLINE_OK 防坑要点").unwrap();
        // base_dir 之外放一个文件，用其内容做越权检测标记。
        std::fs::write(root.join("secret.txt"), "TOPSECRET").unwrap();

        // 存在的引用被内联；不存在的忽略；`..` 越权不内联。
        let text = "请阅读 @notes.md 避免踩坑；@missing.md 不存在；@../secret.txt 越权。";
        let out = expand_references(text, &dir);
        assert!(out.contains("INLINE_OK"), "存在的 @file 应被内联");
        assert!(out.contains("notes.md"), "应标注来源文件名");
        assert!(!out.contains("TOPSECRET"), "越权路径内容不应被内联");

        // 同一文件引用两次只内联一份。
        let out2 = expand_references("@notes.md 然后再 @notes.md", &dir);
        assert_eq!(out2.matches("INLINE_OK").count(), 1, "重复引用只内联一次");

        // 无任何可解析引用时原样返回。
        assert_eq!(expand_references("没有引用", &dir), "没有引用");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn without_disabled_hides_skills_from_list_and_find() {
        let dir = std::env::temp_dir().join(format!("wc-skills-dis-{}", std::process::id()));
        for name in ["alpha", "beta"] {
            let sk = dir.join(name);
            std::fs::create_dir_all(&sk).unwrap();
            std::fs::write(
                sk.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: d\n---\n正文"),
            )
            .unwrap();
        }
        let set =
            SkillSet::load_dirs(std::slice::from_ref(&dir)).without_disabled(&["beta".into()]);
        assert!(set.find("alpha").is_some());
        assert!(set.find("beta").is_none(), "停用技能不应被 find 到");
        assert!(set.prompt_list().contains("alpha"));
        assert!(
            !set.prompt_list().contains("beta"),
            "停用技能不应进可用清单"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn only_keeps_pinned_skills_and_empty_is_noop() {
        let dir = std::env::temp_dir().join(format!("wc-skills-only-{}", std::process::id()));
        for name in ["alpha", "beta", "gamma"] {
            let sk = dir.join(name);
            std::fs::create_dir_all(&sk).unwrap();
            std::fs::write(
                sk.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: d\n---\n正文"),
            )
            .unwrap();
        }
        // 钉选 alpha+gamma：只剩这两个，beta 既不在清单也 find 不到。
        let pinned =
            SkillSet::load_dirs(std::slice::from_ref(&dir)).only(&["alpha".into(), "gamma".into()]);
        assert!(pinned.find("alpha").is_some());
        assert!(pinned.find("gamma").is_some());
        assert!(pinned.find("beta").is_none(), "未钉选的技能应被过滤");
        assert!(!pinned.prompt_list().contains("beta"));

        // 空钉选=不过滤，全量保留。
        let all = SkillSet::load_dirs(std::slice::from_ref(&dir)).only(&[]);
        assert_eq!(all.names().len(), 3, "空钉选不应过滤");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn loads_from_dir() {
        let dir = std::env::temp_dir().join(format!("wc-skills-{}", std::process::id()));
        let sk = dir.join("greeter");
        std::fs::create_dir_all(&sk).unwrap();
        std::fs::write(
            sk.join("SKILL.md"),
            "---\nname: greeter\ndescription: 打招呼\n---\n说你好",
        )
        .unwrap();
        let set = SkillSet::load_dirs(std::slice::from_ref(&dir));
        assert!(set.find("greeter").is_some());
        assert!(set.prompt_list().contains("greeter"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
