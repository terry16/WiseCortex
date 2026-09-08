//! knowledge_search：在用户为本会话挂载的知识库（指定的目录/文件）里做轻量关键词检索。
//!
//! 纯本地、无外部依赖：遍历挂载路径下的文本文件，按查询词出现次数排序，返回匹配文件与片段。
//! 不是语义/向量检索；适合"技能/对话里指明知识库位置后让 agent 现查"。

use std::path::PathBuf;

use serde_json::{json, Value};

use super::{require_str, Tool, ToolResult};

const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_FILES_SCANNED: usize = 3000;
const MAX_RESULT_FILES: usize = 8;
const MAX_LINES_PER_FILE: usize = 5;
const MAX_OUTPUT: usize = 8000;

/// 在指定知识库路径里检索。`roots` 可为目录或单文件。
pub struct KnowledgeSearch {
    roots: Vec<PathBuf>,
}

impl KnowledgeSearch {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self { roots }
    }
}

impl Tool for KnowledgeSearch {
    fn name(&self) -> &'static str {
        "knowledge_search"
    }
    fn description(&self) -> &'static str {
        "Search the knowledge base the user mounted for this session (specific directories/files) for \
         content relevant to a query, returning matching file paths and snippets (line numbers + \
         excerpts). When answering anything that touches the knowledge base, search with this first, \
         then answer."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "query": { "type": "string", "description": "Search keyword or phrase" } },
            "required": ["query"]
        })
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "知识库检索 {}",
            args.get("query").and_then(Value::as_str).unwrap_or("?")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let query = require_str(args, "query")?;
        Ok(search_in(&self.roots, &query))
    }
}

/// 收集要扫描的文件（目录递归；单文件直接计入）。
fn collect_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in roots {
        if root.is_file() {
            files.push(root.clone());
        } else if root.is_dir() {
            for entry in walkdir::WalkDir::new(root)
                .max_depth(8)
                .into_iter()
                .flatten()
            {
                if files.len() >= MAX_FILES_SCANNED {
                    break;
                }
                let p = entry.path();
                if p.is_file()
                    && std::fs::metadata(p)
                        .map(|m| m.len() <= MAX_FILE_BYTES)
                        .unwrap_or(false)
                {
                    files.push(p.to_path_buf());
                }
            }
        }
        if files.len() >= MAX_FILES_SCANNED {
            break;
        }
    }
    files
}

/// 把查询拆成检索词：整句 + 按空白分词（去重、小写）。对中文等无空格语言，整句子串匹配仍有效。
fn terms_of(query: &str) -> Vec<String> {
    let q = query.trim().to_lowercase();
    let mut terms = vec![q.clone()];
    for t in q.split_whitespace() {
        if !t.is_empty() && !terms.iter().any(|x| x == t) {
            terms.push(t.to_string());
        }
    }
    terms
}

/// 在 roots 下检索 query，返回排序后的匹配片段文本。
pub fn search_in(roots: &[PathBuf], query: &str) -> String {
    if roots.is_empty() {
        return "（本会话未挂载知识库）".to_string();
    }
    let terms = terms_of(query);
    if terms.iter().all(|t| t.is_empty()) {
        return "（检索词为空）".to_string();
    }

    // (score, path, matching lines) per file
    let mut hits: Vec<(usize, PathBuf, Vec<String>)> = Vec::new();
    for path in collect_files(roots) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue; // 二进制/非 UTF-8 跳过
        };
        let lower = text.to_lowercase();
        let score: usize = terms
            .iter()
            .map(|t| lower.matches(t.as_str()).count())
            .sum();
        if score == 0 {
            continue;
        }
        let mut lines = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let ll = line.to_lowercase();
            if terms.iter().any(|t| ll.contains(t.as_str())) {
                let trimmed = line.trim();
                let snip: String = trimmed.chars().take(200).collect();
                lines.push(format!("  L{}: {}", i + 1, snip));
                if lines.len() >= MAX_LINES_PER_FILE {
                    break;
                }
            }
        }
        hits.push((score, path, lines));
    }

    if hits.is_empty() {
        return format!("知识库中未找到与「{query}」相关的内容。");
    }
    hits.sort_by_key(|h| std::cmp::Reverse(h.0));
    let mut out = String::new();
    for (score, path, lines) in hits.into_iter().take(MAX_RESULT_FILES) {
        out.push_str(&format!("# {} （命中 {score}）\n", path.display()));
        for l in lines {
            out.push_str(&l);
            out.push('\n');
        }
        out.push('\n');
        if out.len() >= MAX_OUTPUT {
            out.push_str("…（结果已截断）\n");
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searches_dir_and_file_ranked() {
        let base = std::env::temp_dir().join(format!("wc-kb-{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("a.md"), "退款政策：7 天内可退款。\n联系客服。").unwrap();
        std::fs::write(base.join("b.md"), "发货说明：48 小时内发货。").unwrap();
        std::fs::write(base.join("c.md"), "退款 退款 退款 多次出现").unwrap();

        let out = search_in(std::slice::from_ref(&base), "退款");
        // c.md 命中最多，应排在 a.md 之前。
        let ca = out.find("c.md").unwrap();
        let aa = out.find("a.md").unwrap();
        assert!(ca < aa, "命中多的文件应排前");
        assert!(!out.contains("b.md"), "无关文件不应出现");
        assert!(out.contains("L1:"), "应带行号片段");

        // 无结果
        assert!(search_in(std::slice::from_ref(&base), "不存在的词").contains("未找到"));
        // 空挂载
        assert!(search_in(&[], "x").contains("未挂载"));

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn single_file_root_works() {
        let base = std::env::temp_dir().join(format!("wc-kb1-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let f = base.join("note.txt");
        std::fs::write(&f, "important: API key rotation每季度一次").unwrap();
        let out = search_in(std::slice::from_ref(&f), "rotation");
        assert!(out.contains("note.txt"));
        std::fs::remove_dir_all(&base).ok();
    }
}
