//! ClawHub (clawhub.ai) 只读客户端：列表 / 搜索 / 卡片 / 下载 ZIP。
//!
//! 匿名访问即可（带 token 仅提高限流，本期不做）。技能包是 ZIP（内含 SKILL.md + 配套文件），
//! 下载后解压、定位含 SKILL.md 的根目录，复制进 skills 目录——格式与 WiseCortex 完全一致。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

/// 市场展示用的一条 ClawHub 技能。
#[derive(Debug, Clone, PartialEq)]
pub struct ClawHubEntry {
    pub slug: String,
    pub display_name: String,
    pub summary: String,
    pub version: Option<String>,
}

fn client(timeout_secs: u64) -> Result<reqwest::blocking::Client, String> {
    crate::net::blocking_builder()
        .user_agent("WiseCortex/0.0")
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| e.to_string())
}

fn base_url(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    if b.is_empty() {
        "https://clawhub.ai".to_string()
    } else {
        b.to_string()
    }
}

// ── 响应结构（只取需要的字段；camelCase）────────────────────────────────────
#[derive(Deserialize)]
struct VersionObj {
    version: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchItem {
    slug: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    version: Option<String>,
}
#[derive(Deserialize)]
struct SearchResp {
    #[serde(default)]
    results: Vec<SearchItem>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListItem {
    slug: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    latest_version: Option<VersionObj>,
}
#[derive(Deserialize)]
struct ListResp {
    #[serde(default)]
    items: Vec<ListItem>,
}

/// 解析搜索响应 JSON（抽出便于单测，不打真网）。
pub fn parse_search(json: &str) -> Result<Vec<ClawHubEntry>, String> {
    let resp: SearchResp = serde_json::from_str(json).map_err(|e| e.to_string())?;
    Ok(resp
        .results
        .into_iter()
        .map(|i| ClawHubEntry {
            slug: i.slug,
            display_name: i.display_name,
            summary: i.summary.unwrap_or_default(),
            version: i.version,
        })
        .collect())
}

/// 解析列表响应 JSON（抽出便于单测）。
pub fn parse_list(json: &str) -> Result<Vec<ClawHubEntry>, String> {
    let resp: ListResp = serde_json::from_str(json).map_err(|e| e.to_string())?;
    Ok(resp
        .items
        .into_iter()
        .map(|i| ClawHubEntry {
            slug: i.slug,
            display_name: i.display_name,
            summary: i.summary.unwrap_or_default(),
            version: i.latest_version.and_then(|v| v.version),
        })
        .collect())
}

/// 列出 / 搜索 ClawHub 技能：有查询词走 /search，否则走 /skills 列表。
pub fn list_or_search(
    base: &str,
    query: Option<&str>,
    limit: usize,
) -> Result<Vec<ClawHubEntry>, String> {
    let base = base_url(base);
    let c = client(8)?;
    match query.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => {
            let text = c
                .get(format!("{base}/api/v1/search"))
                .query(&[("q", q), ("limit", &limit.to_string())])
                .send()
                .map_err(|e| e.to_string())?
                .text()
                .map_err(|e| e.to_string())?;
            parse_search(&text)
        }
        None => {
            let text = c
                .get(format!("{base}/api/v1/skills"))
                .query(&[("limit", &limit.to_string())])
                .send()
                .map_err(|e| e.to_string())?
                .text()
                .map_err(|e| e.to_string())?;
            parse_list(&text)
        }
    }
}

/// 取某技能的 SKILL.md 文本（卡片）。
pub fn card(base: &str, slug: &str) -> Result<String, String> {
    let base = base_url(base);
    client(8)?
        .get(format!("{base}/api/v1/skills/{}/card", urlenc(slug)))
        .send()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())
}

/// 下载技能 ZIP 原始字节。
pub fn download_zip(base: &str, slug: &str, version: Option<&str>) -> Result<Vec<u8>, String> {
    let base = base_url(base);
    let mut req = client(60)?
        .get(format!("{base}/api/v1/download"))
        .query(&[("slug", slug)]);
    if let Some(v) = version.filter(|v| !v.is_empty()) {
        req = req.query(&[("version", v)]);
    }
    let resp = req.send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("ClawHub 下载失败：HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().map_err(|e| e.to_string())?;
    Ok(bytes.to_vec())
}

/// 安装：下载 → 解压到临时目录 → 定位含 SKILL.md 的根 → 复制进 `dest_skills_dir/<slug>`。
pub fn install(
    base: &str,
    slug: &str,
    version: Option<&str>,
    dest_skills_dir: &Path,
) -> Result<(), String> {
    let bytes = download_zip(base, slug, version)?;
    let tmp = std::env::temp_dir().join(format!(
        "wc-clawhub-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let result = (|| {
        extract_zip(&bytes, &tmp)?;
        let root = find_skill_root(&tmp).ok_or("ZIP 内未找到 SKILL.md")?;
        let dest = dest_skills_dir.join(slug);
        crate::marketplace::copy_dir_recursive(&root, &dest).map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// 解压 ZIP 字节到 `dest`（zip crate 的 enclosed_name 防 zip-slip 越权）。
pub fn extract_zip(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let reader = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| format!("ZIP 解析失败：{e}"))?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = file.enclosed_name() else {
            continue; // 越权路径跳过
        };
        let out = dest.join(rel);
        if file.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            std::fs::write(&out, buf).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 在解压目录里找含 SKILL.md 的目录（取最浅的一个）。
pub fn find_skill_root(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(usize, PathBuf)> = None;
    for entry in walkdir::WalkDir::new(dir)
        .max_depth(4)
        .into_iter()
        .flatten()
    {
        if entry.file_name() == "SKILL.md" && entry.path().is_file() {
            if let Some(parent) = entry.path().parent() {
                let depth = parent.components().count();
                if best.as_ref().map(|(d, _)| depth < *d).unwrap_or(true) {
                    best = Some((depth, parent.to_path_buf()));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

fn urlenc(s: &str) -> String {
    // 技能 slug 仅含 [A-Za-z0-9._-]，简单转义空格/斜杠足矣。
    s.replace(' ', "%20").replace('/', "%2F")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_search_extracts_entries() {
        let json = r#"{"results":[
            {"score":1.0,"slug":"weather","displayName":"Weather","summary":"查天气","version":"1.2.0"},
            {"score":0.5,"slug":"sql","displayName":"SQL Helper"}
        ]}"#;
        let e = parse_search(json).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].slug, "weather");
        assert_eq!(e[0].version.as_deref(), Some("1.2.0"));
        assert_eq!(e[1].display_name, "SQL Helper");
        assert!(e[1].version.is_none());
    }

    #[test]
    fn parse_list_reads_nested_latest_version() {
        let json = r#"{"items":[
            {"slug":"a","displayName":"A","summary":"s","latestVersion":{"version":"2.0.0"}},
            {"slug":"b","displayName":"B"}
        ],"nextCursor":null}"#;
        let e = parse_list(json).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].version.as_deref(), Some("2.0.0"));
        assert!(e[1].version.is_none());
    }

    #[test]
    fn extract_zip_and_find_root_roundtrip() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        // 造一个内含 sub/weather/SKILL.md 的 zip。
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();
            w.start_file("weather/SKILL.md", opts).unwrap();
            w.write_all(b"---\nname: weather\n---\n\xe6\x9f\xa5\xe5\xa4\xa9\xe6\xb0\x94")
                .unwrap();
            w.start_file("weather/notes.md", opts).unwrap();
            w.write_all(b"companion").unwrap();
            w.finish().unwrap();
        }
        let tmp = std::env::temp_dir().join(format!("wc-clawhub-test-{}", std::process::id()));
        std::fs::remove_dir_all(&tmp).ok();
        extract_zip(&buf, &tmp).unwrap();
        let root = find_skill_root(&tmp).expect("应找到 SKILL.md 根");
        assert!(root.join("SKILL.md").is_file());
        assert!(root.join("notes.md").is_file());
        std::fs::remove_dir_all(&tmp).ok();
    }
}
