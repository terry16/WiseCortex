//! 记忆：分**会话级**与**项目级**两层，各存一份 markdown。
//!
//! - 会话级 `<data_dir>/wisecortex/session-memory/<sid>.md`——只属于这一次对话。
//! - 项目级 `<data_dir>/wisecortex/project-memory/<目录名>-<指纹>.md`——同一工作目录下
//!   **所有会话共享**，新开对话也带得走。
//!
//! 分两层的原因：只有会话级时，开一个新对话记忆就从零开始，用户不得不把同一件事
//! 反复交代一遍。项目级按工作目录隔离，则 wisecortex 学到的东西不会串到别的项目去。
//!
//! 每份记忆内部再分两节：**坑**（纠正/踩过的坑，不得重犯）与**记录**（决定、约束、
//! 进展、偏好）。坑单列且清理时最后才丢——它们正是复述成本最高的那类信息。
//!
//! 与上下文压缩互补：压缩会丢早期细节，记忆把要点固化，跨压缩/重开仍每轮注入。

use std::path::{Path, PathBuf};

/// 单份记忆总上限（约 16KB）。超出时按 [`prune`] 的顺序丢弃。
pub const MAX_BYTES: usize = 16 * 1024;

/// 单条记忆上限（字节）。约 130 个汉字。
///
/// 实测用户的记忆文件里平均每条 998 字节，十几条就撑满 16KB 并开始丢最早的教训。
/// 逼短单条是把「能记住多少条」提高一个数量级最直接的办法——记忆要的是结论，
/// 不是把整段调查过程抄进来。
pub const MAX_ENTRY_BYTES: usize = 400;

const LESSON_HEAD: &str = "## Lessons (do not repeat)";
const FACT_HEAD: &str = "## Notes";

/// 记忆条目的类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 踩过的坑 / 用户的纠正。优先级最高，清理时最后才丢。
    Lesson,
    /// 决定、约束、进展、偏好等一般事实。
    Fact,
}

impl Kind {
    /// 从工具参数解析；除 `lesson`/`坑`/`纠正` 外一律当普通记录。
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "lesson" | "坑" | "纠正" => Kind::Lesson,
            _ => Kind::Fact,
        }
    }
}

/// 记忆的归属范围。
#[derive(Debug, Clone, Copy)]
pub enum Scope<'a> {
    /// 单次对话独有。
    Session(&'a str),
    /// 某工作目录下所有对话共享。
    Project(&'a Path),
}

/// 文件名净化：只保留字母数字与 `-_`，其余换成 `_`（sid 通常是 UUID，防御性处理）。
fn sanitize(sid: &str) -> String {
    let s: String = sid
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "session".to_string()
    } else {
        s
    }
}

/// 路径指纹（FNV-1a 64）。**自己实现而不用 DefaultHasher**：后者不保证跨版本/跨平台稳定，
/// 换个 Rust 版本就可能算出不同文件名，把项目记忆丢掉。
fn fingerprint(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 工作目录 → 项目记忆文件名。取「目录名-指纹」：目录名便于人工辨认，
/// 指纹保证不同路径的同名目录（多个 repo 都叫 `web`）不会互相覆盖。
/// 指纹按小写全路径算，Windows 上大小写不同的同一目录仍归一份。
fn project_file(dir: &Path) -> String {
    let full = dir.to_string_lossy().to_lowercase().replace('\\', "/");
    // 目录名也转小写：否则 `D:/apps/wisecortex` 与 `D:\Apps\WiseCortex` 指纹相同、文件名却差
    // 大小写。Windows 大小写不敏感看不出问题，Linux 服务器上会变成两份记忆各记各的。
    let name = dir
        .file_name()
        .map(|s| sanitize(&s.to_string_lossy()).to_lowercase())
        .filter(|s| s != "session")
        .unwrap_or_else(|| "project".to_string());
    format!("{name}-{:016x}.md", fingerprint(&full))
}

/// 某范围的记忆文件路径。
pub fn path_of(scope: Scope<'_>) -> Option<PathBuf> {
    let base = dirs::data_dir()?.join("wisecortex");
    Some(match scope {
        Scope::Session(sid) => base
            .join("session-memory")
            .join(format!("{}.md", sanitize(sid))),
        Scope::Project(dir) => base.join("project-memory").join(project_file(dir)),
    })
}

/// 某会话记忆文件路径（保留旧名，等价于 `path_of(Scope::Session(sid))`）。
pub fn session_path(sid: &str) -> Option<PathBuf> {
    path_of(Scope::Session(sid))
}

/// 存放全部会话记忆的目录（迁移/清理工具遍历用）。
pub fn session_memory_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("session-memory"))
}

/// 读取某范围的记忆（无/不可读则空串）。
pub fn read_scope(scope: Scope<'_>) -> String {
    path_of(scope)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default()
}

/// 读取会话记忆。
pub fn read(sid: &str) -> String {
    read_scope(Scope::Session(sid))
}

/// 写入某范围的记忆（自动建目录；空串则清空文件）。
pub fn write_scope(scope: Scope<'_>, text: &str) -> std::io::Result<()> {
    let path = path_of(scope)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "无数据目录"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)
}

/// 写入会话记忆。
pub fn write(sid: &str, text: &str) -> std::io::Result<()> {
    write_scope(Scope::Session(sid), text)
}

/// 删除会话记忆文件（会话删除时调用）。**不动项目记忆**——那是跨会话资产。
pub fn clear(sid: &str) {
    if let Some(p) = session_path(sid) {
        let _ = std::fs::remove_file(p);
    }
}

/// 解析出的两节内容。
#[derive(Debug, Default, PartialEq)]
struct Sections {
    lessons: Vec<String>,
    facts: Vec<String>,
}

/// 把记忆文本解析成两节。
///
/// **兼容旧格式**：没有小节标题的老文件（一堆裸 `- ` 行）整体当作「记录」，
/// 一条不丢。截断标记行会被丢弃（它只是提示，不是记忆内容）。
fn parse(text: &str) -> Sections {
    let mut out = Sections::default();
    let mut in_lessons = false;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('…') {
            continue;
        }
        if t.starts_with("## ") {
            in_lessons = t == LESSON_HEAD;
            continue;
        }
        let item = t.strip_prefix("- ").unwrap_or(t).trim().to_string();
        if item.is_empty() {
            continue;
        }
        if in_lessons {
            out.lessons.push(item);
        } else {
            out.facts.push(item);
        }
    }
    out
}

/// 渲染回 markdown。空的小节不输出标题，避免产生空壳。
fn render(s: &Sections) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !s.lessons.is_empty() {
        let body: Vec<String> = s.lessons.iter().map(|l| format!("- {l}")).collect();
        parts.push(format!("{LESSON_HEAD}\n{}", body.join("\n")));
    }
    if !s.facts.is_empty() {
        let body: Vec<String> = s.facts.iter().map(|l| format!("- {l}")).collect();
        parts.push(format!("{FACT_HEAD}\n{}", body.join("\n")));
    }
    parts.join("\n\n")
}

/// 归一化用于去重比较：折叠所有空白、去掉首尾。不改写实际存储的文本。
fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 单条限长：超出按字符边界截断并加省略号，绝不切坏 UTF-8。
fn clip_entry(note: &str) -> String {
    let note = note.trim();
    if note.len() <= MAX_ENTRY_BYTES {
        return note.to_string();
    }
    let mut end = MAX_ENTRY_BYTES;
    while end > 0 && !note.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &note[..end])
}

/// 超总上限时按优先级丢弃：**先丢最早的「记录」，记录丢光了才动「坑」**。
/// 坑是复述成本最高的那类，必须最后才丢。
fn prune(s: &mut Sections) {
    while render(s).len() > MAX_BYTES {
        if !s.facts.is_empty() {
            s.facts.remove(0);
        } else if !s.lessons.is_empty() {
            s.lessons.remove(0);
        } else {
            break;
        }
    }
}

/// 由现有记忆 + 新要点合成新内容（纯函数，便于测试）。
///
/// `mode="replace"` 整体替换该类别那一节（另一节保留）；否则追加一条。
/// 追加时自动：单条限长、同节内去重（已有等价条目则原样返回）、超上限按 [`prune`] 丢弃。
pub fn compose(existing: &str, note: &str, kind: Kind, mode: &str) -> String {
    let mut s = parse(existing);
    let entry = clip_entry(note);
    if entry.is_empty() {
        return render(&s);
    }
    let bucket = match kind {
        Kind::Lesson => &mut s.lessons,
        Kind::Fact => &mut s.facts,
    };
    if mode.eq_ignore_ascii_case("replace") {
        *bucket = vec![entry];
    } else {
        let key = norm(&entry);
        if !bucket.iter().any(|e| norm(e) == key) {
            bucket.push(entry);
        }
    }
    prune(&mut s);
    render(&s)
}

/// 迁移专用：把一批**既有**条目并入某一节，去重 + 超总量裁剪，但**不做单条限长**。
///
/// 单条限长（[`MAX_ENTRY_BYTES`]）是约束模型「以后要写得短」的规矩，拿它去切历史条目
/// 只会把内容毁掉——实测老记忆平均每条 998 字节，套限长后 91% 的条目被腰斩。
/// 宁可让总量裁剪丢掉几条最早的**完整**条目，也不要留下一堆残句。
pub fn merge_entries(existing: &str, items: &[String], kind: Kind) -> String {
    let mut s = parse(existing);
    for it in items {
        let entry = it.trim().to_string();
        if entry.is_empty() {
            continue;
        }
        let bucket = match kind {
            Kind::Lesson => &mut s.lessons,
            Kind::Fact => &mut s.facts,
        };
        let key = norm(&entry);
        if !bucket.iter().any(|e| norm(e) == key) {
            bucket.push(entry);
        }
    }
    prune(&mut s);
    render(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_appends_into_the_right_section() {
        let a = compose("", "别用 git add -A", Kind::Lesson, "append");
        assert!(a.starts_with(LESSON_HEAD), "坑应进坑那一节: {a}");
        let b = compose(&a, "决定用 rustls", Kind::Fact, "append");
        assert!(
            b.contains(LESSON_HEAD) && b.contains(FACT_HEAD),
            "两节并存: {b}"
        );
        assert!(b.contains("- 别用 git add -A") && b.contains("- 决定用 rustls"));
        // 坑排在记录前面：注入时先看到最该遵守的。
        assert!(b.find(LESSON_HEAD) < b.find(FACT_HEAD));
    }

    #[test]
    fn compose_dedups_equivalent_entries() {
        // 同一件事换个空白排版不该占两条——这是记忆撑爆的主因之一。
        let a = compose("", "别用 git add -A", Kind::Lesson, "append");
        let b = compose(&a, "别用   git  add -A  ", Kind::Lesson, "append");
        assert_eq!(a, b, "等价条目应被去重");
        assert_eq!(b.matches("- 别用").count(), 1);
    }

    #[test]
    fn compose_clips_overlong_entries() {
        // 实测用户的条目平均 998 字节，逼短才装得下更多条。
        let huge = "很长的教训".repeat(500);
        let out = compose("", &huge, Kind::Fact, "append");
        assert!(
            out.len() < MAX_ENTRY_BYTES + 64,
            "单条应被限长: {}",
            out.len()
        );
        assert!(out.ends_with('…'), "截断应有省略号");
    }

    #[test]
    fn prune_drops_facts_before_lessons() {
        // 上限逼近时，坑必须活到最后。
        let mut s = Sections {
            lessons: vec!["坑A".into(), "坑B".into()],
            facts: (0..400)
                .map(|i| format!("事实{i}{}", "x".repeat(200)))
                .collect(),
        };
        prune(&mut s);
        assert!(render(&s).len() <= MAX_BYTES);
        assert_eq!(
            s.lessons,
            vec!["坑A".to_string(), "坑B".to_string()],
            "坑一条都不该丢"
        );
        assert!(s.facts.len() < 400, "应该丢掉了一些事实");
    }

    #[test]
    fn parse_keeps_legacy_flat_files() {
        // 老文件是没有小节标题的裸 `- ` 行 + 可能的截断标记，升级后一条都不能丢。
        let legacy = "…（较早记忆已截断）\n- 老要点1\n- 老要点2";
        let s = parse(legacy);
        assert!(s.lessons.is_empty());
        assert_eq!(s.facts, vec!["老要点1".to_string(), "老要点2".to_string()]);
        // 追加新条目后老内容仍在。
        let out = compose(legacy, "新要点", Kind::Fact, "append");
        assert!(out.contains("老要点1") && out.contains("新要点"));
    }

    #[test]
    fn replace_only_rewrites_its_own_section() {
        let a = compose("", "坑X", Kind::Lesson, "append");
        let b = compose(&a, "事实Y", Kind::Fact, "append");
        let c = compose(&b, "全新事实", Kind::Fact, "replace");
        assert!(c.contains("坑X"), "replace 记录不该波及坑: {c}");
        assert!(c.contains("全新事实") && !c.contains("事实Y"));
    }

    #[test]
    fn project_files_differ_per_directory_and_are_stable() {
        let a = project_file(Path::new("D:/apps/wisecortex"));
        let b = project_file(Path::new("D:/apps/mud"));
        assert_ne!(a, b, "不同目录必须分开存");
        assert_eq!(
            a,
            project_file(Path::new("D:/apps/wisecortex")),
            "同目录须稳定"
        );
        // 大小写差异归一为同一份（各平台都成立：指纹与目录名都转小写）。
        assert_eq!(a, project_file(Path::new("D:/Apps/WiseCortex")));
        assert!(
            a.starts_with("wisecortex-"),
            "文件名应带可辨认的目录名: {a}"
        );
        // 反斜杠分隔符只在 Windows 上是分隔符：Linux/macOS 的 file_name() 会把
        // `D:\Apps\WiseCortex` 整串当成文件名，算出的目录名自然不同。这条只在 Windows 断言。
        #[cfg(windows)]
        assert_eq!(a, project_file(Path::new("D:\\Apps\\WiseCortex")));
        // 同名不同路径的目录不能互相覆盖。
        assert_ne!(
            project_file(Path::new("D:/a/web")),
            project_file(Path::new("D:/b/web"))
        );
    }

    #[test]
    fn merge_entries_keeps_long_legacy_items_whole() {
        // 迁移不该把老条目腰斩：宁可总量裁剪丢掉最早的几条完整条目。
        let long = "老记忆".repeat(200); // 远超 MAX_ENTRY_BYTES
        let out = merge_entries("", std::slice::from_ref(&long), Kind::Fact);
        assert!(out.contains(&long), "长条目必须原样保留，不得截断");
        assert!(!out.contains('…'), "不该出现截断省略号");
    }

    #[test]
    fn merge_entries_dedups_and_respects_total_cap() {
        // 不超上限时重复并入是幂等的（去重生效）。
        let small: Vec<String> = (0..5).map(|i| format!("条目{i}")).collect();
        let a = merge_entries("", &small, Kind::Fact);
        assert_eq!(a, merge_entries(&a, &small, Kind::Fact), "未超限时应幂等");

        // 超上限时总量收敛，但**已被裁掉的条目会在重跑时回来并挤走最新的**——
        // 所以迁移命令对「已有内容的项目记忆」默认跳过，不能盲目重跑。
        let big: Vec<String> = (0..300)
            .map(|i| format!("条目{i}{}", "x".repeat(200)))
            .collect();
        let out = merge_entries("", &big, Kind::Fact);
        assert!(out.len() <= MAX_BYTES, "总量必须收在上限内");
        assert_ne!(
            out,
            merge_entries(&out, &big, Kind::Fact),
            "超限场景不幂等，这正是迁移要跳过非空目标的原因"
        );
    }

    #[test]
    fn sanitize_keeps_uuid_rejects_separators() {
        assert_eq!(sanitize("3f2a-9b_c1"), "3f2a-9b_c1");
        assert_eq!(sanitize("a/b\\c..d"), "a_b_c__d");
        assert_eq!(sanitize(""), "session");
    }

    #[test]
    fn kind_parses_lesson_aliases() {
        assert_eq!(Kind::parse("lesson"), Kind::Lesson);
        assert_eq!(Kind::parse("坑"), Kind::Lesson);
        assert_eq!(Kind::parse("LESSON"), Kind::Lesson);
        assert_eq!(Kind::parse("fact"), Kind::Fact);
        assert_eq!(Kind::parse(""), Kind::Fact);
    }
}
