//! 技能市场「源」：多源 + 类型（静态 JSON / ClawHub）+ 当前选中。
//!
//! 现有静态 registry 是 JSON 数组；ClawHub 是另一套 API + ZIP 下载，故用 [`SourceKind`] 区分。
//! 内置一份精选源列表（用户可在数据目录放 `registry-sources.json` 追加），当前选中持久化到
//! `marketplace.json`（`{registry_url, registry_kind}`，兼容旧的仅 `registry_url` 写法）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 源类型：决定用哪套协议拉取/安装。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// 静态 JSON registry（`[{name,description,url|content}]`）。
    #[default]
    Static,
    /// ClawHub（clawhub.ai）HTTP API + ZIP 下载。
    ClawHub,
}

/// 一个市场源。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistrySource {
    pub label: String,
    pub url: String,
    #[serde(default)]
    pub kind: SourceKind,
}

/// 内置精选源列表（开箱即用，省得用户满地找）。
pub fn builtin_sources() -> Vec<RegistrySource> {
    vec![
        RegistrySource {
            label: "WiseCortex 精选".to_string(),
            url: crate::marketplace::DEFAULT_REGISTRY_URL.to_string(),
            kind: SourceKind::Static,
        },
        RegistrySource {
            label: "ClawHub (clawhub.ai)".to_string(),
            url: "https://clawhub.ai".to_string(),
            kind: SourceKind::ClawHub,
        },
    ]
}

fn overlay_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("registry-sources.json"))
}

fn marketplace_file() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("marketplace.json"))
}

/// 精选源 + 用户覆盖文件（按 url 去重追加）。
pub fn load_sources() -> Vec<RegistrySource> {
    let mut out = builtin_sources();
    if let Some(extra) = overlay_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Vec<RegistrySource>>(&t).ok())
    {
        for s in extra {
            if !out.iter().any(|x| x.url == s.url) {
                out.push(s);
            }
        }
    }
    out
}

/// 从 marketplace.json 文本解析当前源（抽出便于单测）。无效则返回默认（首个内置源）。
pub fn current_source_from(
    marketplace_json: Option<&str>,
    sources: &[RegistrySource],
) -> RegistrySource {
    let default = || {
        sources.first().cloned().unwrap_or(RegistrySource {
            label: "WiseCortex 精选".to_string(),
            url: crate::marketplace::DEFAULT_REGISTRY_URL.to_string(),
            kind: SourceKind::Static,
        })
    };
    let Some(v) = marketplace_json.and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
    else {
        return default();
    };
    let Some(url) = v
        .get("registry_url")
        .and_then(|u| u.as_str())
        .filter(|s| !s.is_empty())
    else {
        return default();
    };
    let kind = match v.get("registry_kind").and_then(|k| k.as_str()) {
        Some("clawhub") => SourceKind::ClawHub,
        _ => SourceKind::Static,
    };
    // 已知源用其 label，否则视为自定义。
    let label = sources
        .iter()
        .find(|s| s.url == url)
        .map(|s| s.label.clone())
        .unwrap_or_else(|| "自定义源".to_string());
    RegistrySource {
        label,
        url: url.to_string(),
        kind,
    }
}

/// 当前选中的市场源（默认=首个内置源）。
pub fn current_source() -> RegistrySource {
    let text = marketplace_file().and_then(|p| std::fs::read_to_string(p).ok());
    current_source_from(text.as_deref(), &load_sources())
}

/// 设置当前源并持久化到 marketplace.json。
pub fn set_current_source(url: &str, kind: SourceKind) -> std::io::Result<()> {
    let path = marketplace_file()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "无数据目录"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let kind_str = match kind {
        SourceKind::ClawHub => "clawhub",
        SourceKind::Static => "static",
    };
    let body = serde_json::json!({ "registry_url": url, "registry_kind": kind_str });
    std::fs::write(path, serde_json::to_string_pretty(&body)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_sources_include_static_default_and_clawhub() {
        let s = builtin_sources();
        assert_eq!(s[0].kind, SourceKind::Static);
        assert!(s.iter().any(|x| x.kind == SourceKind::ClawHub));
    }

    #[test]
    fn current_source_defaults_to_first_when_unset() {
        let sources = builtin_sources();
        let cur = current_source_from(None, &sources);
        assert_eq!(cur, sources[0]);
        // 损坏 JSON 也回退默认。
        assert_eq!(current_source_from(Some("{ bad"), &sources), sources[0]);
    }

    #[test]
    fn current_source_reads_url_and_kind() {
        let sources = builtin_sources();
        let json = r#"{"registry_url":"https://clawhub.ai","registry_kind":"clawhub"}"#;
        let cur = current_source_from(Some(json), &sources);
        assert_eq!(cur.kind, SourceKind::ClawHub);
        assert_eq!(cur.url, "https://clawhub.ai");
        assert_eq!(cur.label, "ClawHub (clawhub.ai)"); // 命中已知源 label
    }

    #[test]
    fn current_source_legacy_url_only_is_static() {
        let sources = builtin_sources();
        // 旧写法只有 registry_url，无 kind → 视为 static。
        let json = r#"{"registry_url":"https://example.com/r.json"}"#;
        let cur = current_source_from(Some(json), &sources);
        assert_eq!(cur.kind, SourceKind::Static);
        assert_eq!(cur.label, "自定义源");
    }
}
