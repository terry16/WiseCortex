//! 技能市场：内置精选 catalog + 可选远程 registry，一键安装到数据目录。
//!
//! - **内置 catalog**：编译进二进制的精选技能（含 SKILL.md 全文），离线即用。
//! - **远程 registry**：可配置一个 JSON 清单 URL（`[{name,description,url|content}]`），
//!   安装时按 `url` 拉取 SKILL.md 或直接用内联 `content`。
//! - 安装目标：`wisecortex/skills/<name>/SKILL.md`（与 SkillSet 的数据目录来源一致，
//!   invoke_skill 实时从磁盘加载，装完即可用）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// 市场里的一个技能条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// 内联 SKILL.md 全文（内置 catalog 用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// 远程 SKILL.md 的 URL（远程 registry 用，安装时拉取）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// 来源标签：builtin | remote（运行时填，前端区分用；反序列化远程清单时忽略）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
}

/// 解析远程 registry 的 JSON（一个 CatalogEntry 数组）。
pub fn parse_registry(json: &str) -> Result<Vec<CatalogEntry>, String> {
    let mut entries: Vec<CatalogEntry> =
        serde_json::from_str(json).map_err(|e| format!("registry JSON 解析失败: {e}"))?;
    for e in &mut entries {
        e.source = "remote".to_string();
    }
    Ok(entries)
}

/// 内置精选 catalog（每条都带内联 content，离线可装）。
pub fn builtin_catalog() -> Vec<CatalogEntry> {
    fn entry(name: &str, desc: &str, body: &str) -> CatalogEntry {
        let content = format!("---\nname: {name}\ndescription: {desc}\n---\n{body}");
        CatalogEntry {
            name: name.to_string(),
            description: desc.to_string(),
            content: Some(content),
            url: None,
            source: "builtin".to_string(),
        }
    }
    vec![
        entry(
            "commit-helper",
            "按 Conventional Commits 规范写提交信息",
            "为暂存改动写一条规范 commit message：\n\
             1. 先看 `git diff --staged` 理解改动意图。\n\
             2. 首行 `type(scope): 摘要`（type ∈ feat/fix/docs/refactor/test/chore/perf）；祈使句，≤50 字。\n\
             3. 空行后写正文：说明「为什么」而非「改了什么」；列出影响面与风险。\n\
             4. 破坏性变更用 `BREAKING CHANGE:` 段落标注。\n\
             5. 只描述本次改动，不夸大。",
        ),
        entry(
            "pr-writer",
            "撰写清晰的 Pull Request 描述",
            "为当前分支写 PR 描述：\n\
             1. 用 `git log main..HEAD` 和 diff 梳理改动全貌。\n\
             2. 结构：## 背景/动机 → ## 改了什么 → ## 如何验证 → ## 风险与回滚。\n\
             3. 「如何验证」给出可复制的命令与预期结果。\n\
             4. 关联 issue/工单号；列出需要 reviewer 重点看的文件。\n\
             5. 简洁，避免复述每一行代码。",
        ),
        entry(
            "refactorer",
            "在不改变行为的前提下安全重构",
            "执行一次安全重构：\n\
             1. 先确认有覆盖该区域的测试；没有则先补特征测试（characterization test）。\n\
             2. 小步前进：每次只做一种变换（提取函数/改名/内联/去重），改完即跑测试。\n\
             3. 保持每一步行为不变、测试常绿；绝不在重构里夹带功能改动。\n\
             4. 重构后对比公共 API 是否仍兼容。\n\
             5. 收尾跑一遍 fmt + lint + 全量测试。",
        ),
        entry(
            "security-reviewer",
            "对改动做轻量安全审查",
            "对当前改动做安全审查，重点排查：\n\
             1. 注入：SQL/命令/路径拼接是否参数化、是否校验输入。\n\
             2. 认证授权：新端点是否有鉴权、越权访问是否可能。\n\
             3. 密钥与机密：是否硬编码、是否进日志、是否进版本库。\n\
             4. 反序列化/文件上传/SSRF/XSS 等常见面。\n\
             5. 依赖：新引入的库是否可信、有无已知漏洞。\n\
             逐项给出结论（OK/风险），风险项给最小修复建议。",
        ),
    ]
}

/// 把一个条目安装到指定 skills 根目录下（`<dir>/<name>/SKILL.md`）。
/// 仅处理内联 content；远程 url 由 `install` 先拉取再调用本函数。
pub fn install_to(dir: &Path, name: &str, content: &str) -> std::io::Result<()> {
    let skill_dir = dir.join(name);
    std::fs::create_dir_all(&skill_dir)?;
    std::fs::write(skill_dir.join("SKILL.md"), content)
}

/// 列出某 skills 根目录下已安装的技能名（含 SKILL.md 的子目录）。
pub fn installed_in(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return names;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() && p.join("SKILL.md").is_file() {
            if let Some(n) = p.file_name().and_then(|s| s.to_str()) {
                names.push(n.to_string());
            }
        }
    }
    names.sort();
    names
}

/// 把一个本地目录里的技能导入到 `dest_base` 下（递归复制，带配套文件）。
/// `src` 自身含 SKILL.md → 当作单个技能；否则扫描其一层子目录中含 SKILL.md 的。
/// 返回导入的技能名列表。
pub fn import_dir(src: &Path, dest_base: &Path) -> Vec<String> {
    let mut imported = Vec::new();
    let mut copy_one = |dir: &Path| {
        let Some(name) = dir.file_name().and_then(|s| s.to_str()) else {
            return;
        };
        if copy_dir_recursive(dir, &dest_base.join(name)).is_ok() {
            imported.push(name.to_string());
        }
    };
    if src.join("SKILL.md").is_file() {
        copy_one(src);
    } else if let Ok(entries) = std::fs::read_dir(src) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() && p.join("SKILL.md").is_file() {
                copy_one(&p);
            }
        }
    }
    imported
}

/// 递归复制目录。
pub(crate) fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// clone 一个 git 仓库（浅克隆）到临时目录，把其中的技能导入到 `dest_base`，再清理。
/// `subdir` 指定仓库内的技能根（如 `skills`）；None 时优先用仓库根下的 `skills`，否则用仓库根。
pub fn install_from_git(
    url: &str,
    subdir: Option<&str>,
    dest_base: &Path,
) -> Result<Vec<String>, String> {
    let tmp = std::env::temp_dir().join(format!(
        "wc-git-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let mut git = std::process::Command::new("git");
    git.args(["clone", "--depth", "1", url])
        .arg(&tmp)
        .envs(crate::net::proxy_env()); // 配置了代理则 git 也走代理
    let status = crate::proc::no_window(&mut git)
        .status()
        .map_err(|e| format!("无法运行 git（是否已安装？）: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("git clone 失败: {url}"));
    }
    let src = match subdir {
        Some(s) => tmp.join(s),
        None => {
            let skills = tmp.join("skills");
            if skills.is_dir() {
                skills
            } else {
                tmp.clone()
            }
        }
    };
    let names = if src.is_dir() {
        import_dir(&src, dest_base)
    } else {
        Vec::new()
    };
    let _ = std::fs::remove_dir_all(&tmp);
    if names.is_empty() {
        return Err("仓库中未找到含 SKILL.md 的技能目录".to_string());
    }
    Ok(names)
}

// ── 运行时（数据目录 / 远程拉取 / 安装卸载） ────────────────────────────────

/// 技能安装目录：`<data_dir>/wisecortex/skills`。
pub fn skills_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("skills"))
}

/// 默认静态 registry 源：本仓库托管的 registry.json（GitHub raw）。
/// 「WiseCortex 精选」源指向它，保证市场开箱即有可装技能。源选择见 [`crate::registry_sources`]。
pub const DEFAULT_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/terry16/wisecortex/main/docs/registry.json";

/// 远程拉取静态 registry（GET URL → JSON 数组）。失败/离线由调用方降级。
pub fn fetch_remote(url: &str) -> Result<Vec<CatalogEntry>, String> {
    let client = crate::net::blocking_builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let text = client
        .get(url)
        .send()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())?;
    parse_registry(&text)
}

fn fetch_text(url: &str) -> Result<String, String> {
    let client = crate::net::blocking_builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    client
        .get(url)
        .send()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())
}

/// 市场展示用条目（统一 static / clawhub 两类来源）。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MarketEntry {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// 来源标签（如「WiseCortex 精选」/「ClawHub (clawhub.ai)」），UI 展示用。
    pub source: String,
}

/// 按**当前选中源**列出市场条目（可带搜索词）。源不可达/离线时返回空，不阻断。
pub fn market_list(query: Option<&str>) -> Vec<MarketEntry> {
    use crate::registry_sources::{current_source, SourceKind};
    let src = current_source();
    match src.kind {
        SourceKind::Static => {
            let mut out: Vec<MarketEntry> = fetch_remote(&src.url)
                .unwrap_or_default()
                .into_iter()
                .map(|e| MarketEntry {
                    name: e.name,
                    description: e.description,
                    version: None,
                    source: src.label.clone(),
                })
                .collect();
            if let Some(q) = query.map(str::trim).filter(|q| !q.is_empty()) {
                let ql = q.to_lowercase();
                out.retain(|e| {
                    e.name.to_lowercase().contains(&ql)
                        || e.description.to_lowercase().contains(&ql)
                });
            }
            out
        }
        SourceKind::ClawHub => crate::clawhub::list_or_search(&src.url, query, 40)
            .unwrap_or_default()
            .into_iter()
            .map(|e| MarketEntry {
                description: if e.summary.is_empty() {
                    e.display_name
                } else {
                    e.summary
                },
                name: e.slug,
                version: e.version,
                source: src.label.clone(),
            })
            .collect(),
    }
}

/// 当前已安装的技能名（数据目录）。
pub fn installed() -> Vec<String> {
    skills_dir().map(|d| installed_in(&d)).unwrap_or_default()
}

/// 嵌入二进制的内置技能目录。内含四类来源：superpowers（MIT）、ui-ux-pro-max（MIT）、
/// ClaudeKit（MIT）与本项目自写，各自的许可证与归属见根目录 NOTICE.md。
static BUNDLED_SKILLS: include_dir::Dir<'_> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/assets/skills");

/// 内置（编译期自带）的技能名：内联 catalog + 嵌入的设计技能目录。前端据此标「内置」徽章。
pub fn builtin_names() -> Vec<String> {
    let mut names: Vec<String> = builtin_catalog().into_iter().map(|e| e.name).collect();
    names.extend(BUNDLED_SKILLS.dirs().filter_map(|d| {
        d.path()
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_string)
    }));
    names
}

/// 启动时把内置技能播种到数据目录（无数据目录则空操作）。
pub fn seed_builtins() {
    if let Some(dir) = skills_dir() {
        let _ = seed_builtins_to(&dir);
    }
}

/// 收集一个嵌入目录下的所有文件（递归）。
fn collect_embedded<'a>(d: &'a include_dir::Dir<'a>, out: &mut Vec<&'a include_dir::File<'a>>) {
    for f in d.files() {
        out.push(f);
    }
    for sub in d.dirs() {
        collect_embedded(sub, out);
    }
}

/// 把一个嵌入的技能目录写到 `<base>/<name>/`，并改写 SKILL.md 里的脚本路径以适配本地安装。
fn extract_bundled_skill(
    skill: &include_dir::Dir<'_>,
    name: &str,
    base: &Path,
) -> std::io::Result<()> {
    let dest_root = base.join(name);
    let abs = dest_root.display().to_string();
    // 是否脚本型技能（带 scripts/ 子目录，如设计技能）→ 决定是否加 python/cd 提示。
    let has_scripts = skill
        .dirs()
        .any(|d| d.path().file_name().and_then(|s| s.to_str()) == Some("scripts"));
    let mut files = Vec::new();
    collect_embedded(skill, &mut files);
    for f in files {
        // f.path() 形如 "ui-ux-pro-max/scripts/search.py"，去掉技能名前缀。
        let rel = f.path().strip_prefix(name).unwrap_or(f.path());
        let out = dest_root.join(rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if rel.file_name().and_then(|s| s.to_str()) == Some("SKILL.md") {
            let raw = String::from_utf8_lossy(f.contents());
            std::fs::write(&out, localize_skill_md(&raw, name, &abs, has_scripts))?;
        } else {
            std::fs::write(&out, f.contents())?;
        }
    }
    Ok(())
}

/// 改写 SKILL.md：去掉 `skills/<name>/` 路径前缀（脚本变成相对本目录）；脚本型技能再加一段
/// python/cd 提示（非脚本型如 superpowers 流程技能不加，避免误导）。
fn localize_skill_md(raw: &str, name: &str, abs_dir: &str, has_scripts: bool) -> String {
    let body = raw.replace(&format!("skills/{name}/"), "");
    if has_scripts {
        format!(
            "> **WiseCortex 本地化提示**：本技能已安装在 `{abs_dir}`。运行下文任何 `python3 scripts/...` \
             命令前，请先 `cd \"{abs_dir}\"` 再执行（脚本/数据路径均相对该目录）。需要本机已装 Python3。\n\n{body}"
        )
    } else {
        body
    }
}

/// 把内置技能播种到 `dir`。用标记文件 `<dir>/.builtin-seeded`（已播种过的名字列表）控制：
/// 首次全播；后续只补播标记里没有的「新内置」；用户卸载过的内置因名已在标记里，**不会**被
/// 重新播种。返回本次新播种的技能名。
pub fn seed_builtins_to(dir: &Path) -> std::io::Result<Vec<String>> {
    let marker = dir.join(".builtin-seeded");
    let mut seeded: Vec<String> = std::fs::read_to_string(&marker)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let first_run = !marker.exists();
    let mut newly = Vec::new();
    for e in builtin_catalog() {
        if seeded.iter().any(|n| n == &e.name) {
            continue; // 已播种过（即便后来被卸载也不再回来）
        }
        if let Some(content) = &e.content {
            install_to(dir, &e.name, content)?;
            seeded.push(e.name.clone());
            newly.push(e.name);
        }
    }
    // 嵌入的设计技能目录（多文件，带 scripts/data）。
    for skill in BUNDLED_SKILLS.dirs() {
        let Some(name) = skill.path().file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if seeded.iter().any(|n| n == name) {
            continue;
        }
        extract_bundled_skill(skill, name, dir)?;
        seeded.push(name.to_string());
        newly.push(name.to_string());
    }
    if !newly.is_empty() || first_run {
        std::fs::create_dir_all(dir)?;
        std::fs::write(&marker, serde_json::to_string_pretty(&seeded)?)?;
    }
    Ok(newly)
}

/// 安装一个技能：内置优先（保证卸载后仍可重装），否则按当前源安装。
pub fn install_named(name: &str, version: Option<&str>) -> Result<(), String> {
    let dir = skills_dir().ok_or("无数据目录")?;
    // 内置：直接用内联 content 写回。
    if let Some(content) = builtin_catalog()
        .into_iter()
        .find(|e| e.name == name)
        .and_then(|e| e.content)
    {
        return install_to(&dir, name, &content).map_err(|e| e.to_string());
    }
    install_from_market(name, version)
}

/// 按当前选中源安装：static（inline content / url 拉取）/ clawhub（下载 ZIP 解压）。
pub fn install_from_market(name: &str, version: Option<&str>) -> Result<(), String> {
    use crate::registry_sources::{current_source, SourceKind};
    let dir = skills_dir().ok_or("无数据目录")?;
    let src = current_source();
    match src.kind {
        SourceKind::ClawHub => crate::clawhub::install(&src.url, name, version, &dir),
        SourceKind::Static => {
            let entry = fetch_remote(&src.url)?
                .into_iter()
                .find(|e| e.name == name)
                .ok_or_else(|| format!("源中无技能：{name}"))?;
            let content = match (entry.content, entry.url) {
                (Some(c), _) => c,
                (None, Some(u)) => fetch_text(&u)?,
                (None, None) => return Err("条目既无 content 也无 url".to_string()),
            };
            install_to(&dir, name, &content).map_err(|e| e.to_string())
        }
    }
}

/// 用向导字段拼成 SKILL.md 文本（frontmatter + 正文 + 触发 + 工具）。
pub fn build_skill_md(
    slug: &str,
    description: &str,
    trigger: &str,
    body: &str,
    tools: &[String],
) -> String {
    let mut out = format!("---\nname: {slug}\ndescription: {description}\n---\n");
    out.push_str(body.trim());
    if !trigger.trim().is_empty() {
        out.push_str(&format!("\n\n## 何时使用\n{}", trigger.trim()));
    }
    if !tools.is_empty() {
        out.push_str(&format!("\n\n## 可用工具\n{}", tools.join(", ")));
    }
    out.push('\n');
    out
}

/// 创建一个新技能（写入数据目录 skills/<slug>/SKILL.md）。slug 作为技能名（invoke 用）。
pub fn create_skill(
    slug: &str,
    description: &str,
    trigger: &str,
    body: &str,
    tools: &[String],
) -> Result<(), String> {
    let slug = slug.trim();
    if slug.is_empty() {
        return Err("slug 不能为空".to_string());
    }
    let dir = skills_dir().ok_or("无数据目录")?;
    let content = build_skill_md(slug, description, trigger, body, tools);
    install_to(&dir, slug, &content).map_err(|e| e.to_string())
}

/// 读取已安装技能的 SKILL.md 全文（用于 UI 查看）。名字含路径分隔符/`..` 一律拒绝。
pub fn read_skill(name: &str) -> Result<String, String> {
    if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
        return Err("非法技能名".to_string());
    }
    let path = skills_dir()
        .ok_or("无数据目录")?
        .join(name)
        .join("SKILL.md");
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// 卸载已安装技能（删除数据目录下的技能目录）。
pub fn uninstall(name: &str) -> Result<(), String> {
    let dir = skills_dir().ok_or("无数据目录")?.join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_creator_is_bundled_builtin() {
        let names = builtin_names();
        assert!(
            names.iter().any(|n| n == "skill-creator"),
            "skill-creator 应作为内置技能随二进制自带（agent 提示里引用了它）"
        );
    }

    #[test]
    fn builtin_catalog_entries_are_valid_skills() {
        let cat = builtin_catalog();
        assert!(!cat.is_empty(), "内置 catalog 不应为空");
        for e in &cat {
            assert!(!e.name.is_empty());
            let content = e.content.as_deref().expect("内置条目必须带内联 content");
            // 内联内容应能被 skill 解析出与条目一致的 name。
            let sk = crate::skill::parse(content, &e.name);
            assert_eq!(sk.name, e.name, "SKILL.md 的 name 应与条目一致");
            assert!(!sk.instructions.is_empty(), "技能正文不应为空");
        }
    }

    #[test]
    fn build_skill_md_has_frontmatter_and_sections() {
        let md = build_skill_md(
            "my-skill",
            "做某事",
            "用户要做某事时",
            "# 指令\n照做",
            &["read".into(), "write".into()],
        );
        let sk = crate::skill::parse(&md, "fallback");
        assert_eq!(sk.name, "my-skill");
        assert_eq!(sk.description, "做某事");
        assert!(sk.instructions.contains("照做"));
        assert!(sk.instructions.contains("何时使用"));
        assert!(sk.instructions.contains("read, write"));
    }

    #[test]
    fn parse_registry_reads_entries() {
        let json = r#"[
            {"name":"a","description":"da","url":"https://x/a/SKILL.md"},
            {"name":"b","description":"db","content":"---\nname: b\n---\nhi"}
        ]"#;
        let entries = parse_registry(json).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "a");
        assert_eq!(entries[0].url.as_deref(), Some("https://x/a/SKILL.md"));
        assert_eq!(entries[1].content.as_deref(), Some("---\nname: b\n---\nhi"));
        // 非法 JSON → Err
        assert!(parse_registry("not json").is_err());
    }

    #[test]
    fn import_dir_handles_multi_and_single_with_companion_files() {
        let base = std::env::temp_dir().join(format!("wc-imp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("repo-skills");
        // 两个技能子目录，其中 a 带配套文件。
        std::fs::create_dir_all(src.join("a")).unwrap();
        std::fs::write(src.join("a").join("SKILL.md"), "---\nname: a\n---\n甲").unwrap();
        std::fs::write(src.join("a").join("notes.md"), "配套").unwrap();
        std::fs::create_dir_all(src.join("b")).unwrap();
        std::fs::write(src.join("b").join("SKILL.md"), "---\nname: b\n---\n乙").unwrap();
        // 非技能目录（无 SKILL.md）应被忽略。
        std::fs::create_dir_all(src.join("docs")).unwrap();
        std::fs::write(src.join("docs").join("readme.md"), "x").unwrap();

        // 多技能：指向含子目录的目录。
        let dest = base.join("dest");
        let mut names = import_dir(&src, &dest);
        names.sort();
        assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
        assert!(dest.join("a").join("SKILL.md").is_file());
        // 配套文件一并复制。
        assert!(dest.join("a").join("notes.md").is_file());
        assert!(!dest.join("docs").exists());

        // 单技能：src 自身含 SKILL.md。
        let dest2 = base.join("dest2");
        let names2 = import_dir(&src.join("a"), &dest2);
        assert_eq!(names2, vec!["a".to_string()]);
        assert!(dest2.join("a").join("SKILL.md").is_file());

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn seed_builtins_first_run_then_uninstall_then_no_reseed() {
        let dir = std::env::temp_dir().join(format!("wc-seed-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();

        // 首次：全部内置（内联 catalog + 嵌入设计技能）被播种。
        let newly = seed_builtins_to(&dir).unwrap();
        let builtins = builtin_catalog();
        assert!(newly.len() >= builtins.len(), "首次应播种全部内置");
        let names = installed_in(&dir);
        for e in &builtins {
            assert!(names.contains(&e.name), "内置 {} 应已安装", e.name);
        }
        // 嵌入的设计技能也应被播种（含多文件：SKILL.md + scripts/data）。
        assert!(
            names.contains(&"ui-ux-pro-max".to_string()),
            "应播种 ui-ux-pro-max"
        );
        assert!(
            dir.join("ui-ux-pro-max").join("scripts").is_dir(),
            "应带 scripts 目录"
        );
        // SKILL.md 被本地化改写（去掉 skills/<name>/ 前缀 + 加提示）。
        let md = std::fs::read_to_string(dir.join("ui-ux-pro-max").join("SKILL.md")).unwrap();
        assert!(md.contains("WiseCortex 本地化提示"));
        assert!(
            !md.contains("skills/ui-ux-pro-max/scripts/"),
            "脚本路径前缀应已去除"
        );
        // 非脚本型技能（superpowers 流程技能）也应被播种，且不加 python 提示。
        assert!(
            names.contains(&"brainstorming".to_string()),
            "应播种 superpowers 技能"
        );
        let bm = std::fs::read_to_string(dir.join("brainstorming").join("SKILL.md")).unwrap();
        assert!(
            !bm.contains("WiseCortex 本地化提示"),
            "非脚本技能不应加 python 提示"
        );
        // 标记文件不应被当成技能。
        assert!(!names.iter().any(|n| n == ".builtin-seeded"));

        // 再次播种：无新内置 → 不重复。
        assert!(
            seed_builtins_to(&dir).unwrap().is_empty(),
            "无新内置不应重播"
        );

        // 用户卸载一个内置后再播种：不应回来。
        let victim = &builtins[0].name;
        std::fs::remove_dir_all(dir.join(victim)).unwrap();
        assert!(
            seed_builtins_to(&dir).unwrap().is_empty(),
            "卸载过的内置不应被重播"
        );
        assert!(!installed_in(&dir).contains(victim), "卸载的内置不应回来");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn install_then_list_then_uninstall_roundtrip() {
        let root = std::env::temp_dir().join(format!("wc-mkt-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();

        install_to(&root, "greeter", "---\nname: greeter\n---\n说你好").unwrap();
        let names = installed_in(&root);
        assert!(names.contains(&"greeter".to_string()));

        // 写到了正确路径，内容可读。
        let md = root.join("greeter").join("SKILL.md");
        assert!(md.exists());
        let back = std::fs::read_to_string(&md).unwrap();
        assert!(back.contains("说你好"));

        // 卸载 = 删目录。
        std::fs::remove_dir_all(root.join("greeter")).unwrap();
        assert!(!installed_in(&root).contains(&"greeter".to_string()));

        std::fs::remove_dir_all(&root).ok();
    }
}
