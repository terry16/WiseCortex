//! 联网工具：web_fetch（抓网页正文）/ web_search（DuckDuckGo 搜索）。
//! 工具是同步执行（在 spawn_blocking 线程里），用 reqwest::blocking。

use std::time::Duration;

use serde_json::{json, Value};

use super::{require_str, Tool, ToolResult};

const MAX_OUTPUT: usize = 10_000;
const UA: &str = "Mozilla/5.0 (compatible; WiseCortex/0.0)";

fn http() -> Result<reqwest::blocking::Client, String> {
    crate::net::blocking_builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("HTTP 客户端构建失败: {e}"))
}

fn truncate(mut s: String) -> String {
    if s.len() > MAX_OUTPUT {
        s.truncate(MAX_OUTPUT);
        s.push_str("\n…(已截断)");
    }
    s
}

/// 粗略把 HTML 转纯文本：去掉 script/style，标签换空格，解码几个常见实体，压缩空白。
fn html_to_text(html: &str) -> String {
    // regex crate 不支持反向引用，script/style 分两个模式处理。
    let script = regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    let style = regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    let no_script = script.replace_all(html, " ");
    let no_blocks = style.replace_all(&no_script, " ");
    let tags = regex::Regex::new(r"(?s)<[^>]+>").unwrap();
    let text = tags.replace_all(&no_blocks, " ");
    let text = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let ws = regex::Regex::new(r"[ \t\r\f]+").unwrap();
    let text = ws.replace_all(&text, " ");
    let nl = regex::Regex::new(r"\n\s*\n\s*\n+").unwrap();
    nl.replace_all(&text, "\n\n").trim().to_string()
}

fn strip_tags(s: &str) -> String {
    let tags = regex::Regex::new(r"(?s)<[^>]+>").unwrap();
    tags.replace_all(s, "").trim().to_string()
}

// ── web_fetch ───────────────────────────────────────────────────────────────

pub struct WebFetch;
impl Tool for WebFetch {
    fn name(&self) -> &'static str {
        "web_fetch"
    }
    fn description(&self) -> &'static str {
        "Fetch a URL's page content and return it as plain text (follows redirects, strips HTML tags, truncates to about 10,000 characters)."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "url": { "type": "string", "description": "The URL to fetch" } },
            "required": ["url"]
        })
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "抓取 {}",
            args.get("url").and_then(Value::as_str).unwrap_or("?")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let url = require_str(args, "url")?;
        let resp = http()?
            .get(&url)
            .send()
            .map_err(|e| format!("请求失败: {e}"))?;
        let status = resp.status();
        let body = resp.text().map_err(|e| format!("读取响应失败: {e}"))?;
        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }
        Ok(truncate(html_to_text(&body)))
    }
}

// ── web_search ──────────────────────────────────────────────────────────────

pub struct WebSearch;
impl Tool for WebSearch {
    fn name(&self) -> &'static str {
        "web_search"
    }
    fn description(&self) -> &'static str {
        "Search the web by keyword and return the top results (title + link). Links can then be fetched with web_fetch."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "query": { "type": "string", "description": "Search keywords" } },
            "required": ["query"]
        })
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "搜索 {}",
            args.get("query").and_then(Value::as_str).unwrap_or("?")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let query = require_str(args, "query")?;
        let cfg = crate::config::load().web_search.unwrap_or_default();
        let provider = cfg.resolved_provider();
        let hits = match provider.as_str() {
            "duckduckgo" => search_duckduckgo(&query)?,
            "searxng" => {
                let base = cfg
                    .base_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or("SearXNG 需要在设置里填基地址 (base_url)")?;
                search_searxng(base, &query)?
            }
            "brave" => {
                let key = require_key(&cfg, "Brave")?;
                search_brave(&key, &query)?
            }
            "tavily" => {
                let key = require_key(&cfg, "Tavily")?;
                search_tavily(&key, &query)?
            }
            other => return Err(format!("未知搜索提供商：{other}")),
        };
        if hits.is_empty() {
            return Ok("(无结果，或被搜索引擎限流)".to_string());
        }
        Ok(hits
            .into_iter()
            .take(8)
            .map(|(title, url)| format!("- {title}\n  {url}"))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

fn require_key(cfg: &crate::config::WebSearchConfig, label: &str) -> Result<String, String> {
    cfg.api_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("{label} 搜索需要在设置里填 API Key"))
}

/// DuckDuckGo HTML 抓取（keyless）。
fn search_duckduckgo(query: &str) -> Result<Vec<(String, String)>, String> {
    let html = http()?
        .get("https://html.duckduckgo.com/html/")
        .query(&[("q", query)])
        .send()
        .map_err(|e| format!("搜索请求失败: {e}"))?
        .text()
        .map_err(|e| format!("读取响应失败: {e}"))?;
    let re = regex::Regex::new(r#"(?s)<a[^>]*class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#)
        .unwrap();
    let mut out = Vec::new();
    for cap in re.captures_iter(&html).take(8) {
        let mut href = cap[1].to_string();
        if href.starts_with("//") {
            href = format!("https:{href}");
        } else if href.starts_with('/') {
            href = format!("https://html.duckduckgo.com{href}");
        }
        out.push((strip_tags(&cap[2]), href));
    }
    Ok(out)
}

/// SearXNG JSON API（自部署，无 key）：`{base}/search?q=&format=json`。
fn search_searxng(base: &str, query: &str) -> Result<Vec<(String, String)>, String> {
    let url = format!("{}/search", base.trim_end_matches('/'));
    let v: Value = http()?
        .get(url)
        .query(&[("q", query), ("format", "json")])
        .send()
        .map_err(|e| format!("SearXNG 请求失败: {e}"))?
        .json()
        .map_err(|e| format!("SearXNG 响应解析失败: {e}"))?;
    Ok(parse_results(&v, "results", "url", "title"))
}

/// Brave Search API（X-Subscription-Token）。
fn search_brave(key: &str, query: &str) -> Result<Vec<(String, String)>, String> {
    let v: Value = http()?
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("X-Subscription-Token", key)
        .header("Accept", "application/json")
        .query(&[("q", query)])
        .send()
        .map_err(|e| format!("Brave 请求失败: {e}"))?
        .json()
        .map_err(|e| format!("Brave 响应解析失败: {e}"))?;
    // 结果在 web.results[]。
    Ok(v.get("web")
        .map(|w| parse_results(w, "results", "url", "title"))
        .unwrap_or_default())
}

/// Tavily Search API（POST {api_key, query}）。
fn search_tavily(key: &str, query: &str) -> Result<Vec<(String, String)>, String> {
    let v: Value = http()?
        .post("https://api.tavily.com/search")
        .json(&json!({ "api_key": key, "query": query, "max_results": 8 }))
        .send()
        .map_err(|e| format!("Tavily 请求失败: {e}"))?
        .json()
        .map_err(|e| format!("Tavily 响应解析失败: {e}"))?;
    Ok(parse_results(&v, "results", "url", "title"))
}

/// 从 `obj[arr_key]` 数组抽取 (title, url)。
fn parse_results(
    obj: &Value,
    arr_key: &str,
    url_key: &str,
    title_key: &str,
) -> Vec<(String, String)> {
    obj.get(arr_key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let url = r.get(url_key).and_then(Value::as_str)?;
                    let title = r.get(title_key).and_then(Value::as_str).unwrap_or(url);
                    Some((title.to_string(), url.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_results_extracts_title_url() {
        let v = json!({ "results": [
            { "title": "T1", "url": "https://a" },
            { "url": "https://b" }
        ]});
        let r = parse_results(&v, "results", "url", "title");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0], ("T1".to_string(), "https://a".to_string()));
        assert_eq!(r[1], ("https://b".to_string(), "https://b".to_string())); // title 缺省回退 url
    }

    #[test]
    fn html_to_text_strips_tags_and_scripts() {
        let html = "<html><head><style>x{}</style></head><body><p>Hello &amp; <b>world</b></p>\
            <script>alert(1)</script></body></html>";
        let t = html_to_text(html);
        assert!(t.contains("Hello & world"));
        assert!(!t.contains("alert"));
        assert!(!t.contains('<'));
    }
}
