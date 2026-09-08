//! 文件系统工具：read_file / read_image / write_file / edit_file / glob / grep。

use std::path::PathBuf;

use base64::Engine as _;
use serde_json::{json, Value};

use super::{image_result, require_str, resolve, Tool, ToolResult};

fn obj_schema(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

// ── read_file ───────────────────────────────────────────────────────────────

pub struct ReadFile {
    base: PathBuf,
}
impl ReadFile {
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }
}
impl Tool for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }
    fn description(&self) -> &'static str {
        "Read a text file's contents, output with line numbers. Use offset/limit to read a slice of a large file (by line)."
    }
    fn parameters(&self) -> Value {
        obj_schema(
            json!({
                "path": { "type": "string", "description": "File path (relative to the working directory, or absolute)" },
                "offset": { "type": "integer", "description": "Starting line number (1-based, optional)" },
                "limit": { "type": "integer", "description": "Maximum number of lines to read (optional)" }
            }),
            &["path"],
        )
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "读取 {}",
            args.get("path").and_then(Value::as_str).unwrap_or("?")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let path = require_str(args, "path")?;
        let full = resolve(&self.base, &path);
        let bytes = std::fs::read(&full).map_err(|e| format!("读取失败 {path}: {e}"))?;
        // 图片别按文本读：以前这里只会抛个 UTF-8 错误，模型就会绕道「打开看图软件 + 截图」
        // 那条又慢又糊的路。直接把它指到 read_image。
        if let Some(mime) = sniff_image_mime(&bytes) {
            return Err(format!(
                "{path} 是图片（{mime}），不是文本文件。请改用 read_image 直接读取图片内容——不要打开看图软件再截图。"
            ));
        }
        let text = String::from_utf8(bytes).map_err(|_| {
            format!("读取失败 {path}: 不是 UTF-8 文本文件（二进制内容无法按行读取）")
        })?;

        let offset = args
            .get("offset")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1);
        let limit = args.get("limit").and_then(Value::as_u64);

        let mut out = String::new();
        for (i, line) in text.lines().enumerate() {
            let lineno = (i as u64) + 1;
            if lineno < offset {
                continue;
            }
            if let Some(lim) = limit {
                if lineno >= offset + lim {
                    break;
                }
            }
            out.push_str(&format!("{lineno:>6}\t{line}\n"));
        }
        if out.is_empty() {
            out.push_str("(空文件或区间无内容)");
        }
        Ok(out)
    }
}

// ── read_image ──────────────────────────────────────────────────────────────

/// 单张图片的原始字节上限。上游（Anthropic / OpenAI / Gemini）普遍按 **base64 后约 5MB**
/// 收单图，base64 膨胀 4/3，故原始字节按 3.5MB 收口——宁可在本地报个能照做的错，
/// 也别把 5MB 的 body 发出去换一个 413/400。
const MAX_IMAGE_BYTES: usize = 3_500_000;

/// 按魔数（而非扩展名）嗅探图片类型，返回 MIME。只认视觉模型普遍支持的四种：
/// 认扩展名会被 `.png` 的假图骗到，发出去才被上游打回；魔数是内容本身的事实。
fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    // WebP = RIFF 容器，第 8..12 字节是 "WEBP"。
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

/// 直接把本地图片喂给模型看。
///
/// 存在的理由：没有这件工具时，模型想「看」一张本地图片只能走 GUI 那条路——
/// 用看图软件打开、`capture_window` 截图——既慢、又受窗口缩放/遮挡影响掉画质，
/// 在没有 GUI 会话的 Linux 服务端更是完全走不通。图片本身就是一串字节，
/// base64 塞进 `image_result` 即可，截图纯属绕路。
pub struct ReadImage {
    base: PathBuf,
}
impl ReadImage {
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }
}
impl Tool for ReadImage {
    fn name(&self) -> &'static str {
        "read_image"
    }
    fn description(&self) -> &'static str {
        "View a local image file (PNG/JPEG/GIF/WebP) directly — the image itself is returned for you to look at. \
         Use this for ANY image on disk, including screenshots saved by other tools. \
         Never open an image in a viewer application and screenshot it: that is slower and loses quality."
    }
    fn parameters(&self) -> Value {
        obj_schema(
            json!({
                "path": { "type": "string", "description": "Image file path (relative to the working directory, or absolute)" }
            }),
            &["path"],
        )
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "查看图片 {}",
            args.get("path").and_then(Value::as_str).unwrap_or("?")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let path = require_str(args, "path")?;
        let full = resolve(&self.base, &path);
        let bytes = std::fs::read(&full).map_err(|e| format!("读取失败 {path}: {e}"))?;
        let mime = sniff_image_mime(&bytes).ok_or_else(|| {
            format!("{path} 不是受支持的图片（仅 PNG/JPEG/GIF/WebP，按文件内容判定）")
        })?;
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(format!(
                "{path} 太大（{} KB，上限 {} KB）：请先缩小或裁剪，再读取。",
                bytes.len() / 1024,
                MAX_IMAGE_BYTES / 1024
            ));
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let data_url = format!("data:{mime};base64,{b64}");
        let text = format!("已读取图片 {path}（{mime}，{} KB）。", bytes.len() / 1024);
        Ok(image_result(text, vec![data_url]))
    }
}

// ── write_file ──────────────────────────────────────────────────────────────

pub struct WriteFile {
    base: PathBuf,
}
impl WriteFile {
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }
}
impl Tool for WriteFile {
    fn name(&self) -> &'static str {
        "write_file"
    }
    fn description(&self) -> &'static str {
        "Write a file (overwrites existing contents, creates parent directories automatically)."
    }
    fn requires_approval(&self) -> bool {
        true
    }
    fn parameters(&self) -> Value {
        obj_schema(
            json!({
                "path": { "type": "string", "description": "File path" },
                "content": { "type": "string", "description": "Full file contents" }
            }),
            &["path", "content"],
        )
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "写入 {}",
            args.get("path").and_then(Value::as_str).unwrap_or("?")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let path = require_str(args, "path")?;
        let content = require_str(args, "content")?;
        let full = resolve(&self.base, &path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
        }
        std::fs::write(&full, &content).map_err(|e| format!("写入失败 {path}: {e}"))?;
        Ok(format!("已写入 {} 字节到 {path}", content.len()))
    }
}

// ── edit_file ───────────────────────────────────────────────────────────────

pub struct EditFile {
    base: PathBuf,
}
impl EditFile {
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }
}
impl Tool for EditFile {
    fn name(&self) -> &'static str {
        "edit_file"
    }
    fn description(&self) -> &'static str {
        "Replace old_string with new_string in a file, matching exactly. By default old_string must match uniquely; set replace_all=true to replace every occurrence. \
         To make several changes at once, pass an edits array (applied atomically in order, each entry {old_string,new_string,replace_all?}); the file is written only if all succeed."
    }
    fn requires_approval(&self) -> bool {
        true
    }
    fn parameters(&self) -> Value {
        obj_schema(
            json!({
                "path": { "type": "string" },
                "old_string": { "type": "string", "description": "Text to replace (must be unique unless replace_all is set); omit when using edits" },
                "new_string": { "type": "string", "description": "Replacement text; omit when using edits" },
                "replace_all": { "type": "boolean", "description": "Replace all matches (default false)" },
                "edits": {
                    "type": "array",
                    "description": "Batch edits: each entry {old_string,new_string,replace_all?}, applied in order; the file is written only if all succeed",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": { "type": "string" },
                            "new_string": { "type": "string" },
                            "replace_all": { "type": "boolean" }
                        },
                        "required": ["old_string", "new_string"]
                    }
                }
            }),
            &["path"],
        )
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "编辑 {}",
            args.get("path").and_then(Value::as_str).unwrap_or("?")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let path = require_str(args, "path")?;
        // 归一为一组 (old, new, replace_all)：优先 edits 数组，否则单条 old/new。
        let edits: Vec<(String, String, bool)> = match args.get("edits").and_then(Value::as_array) {
            Some(arr) if !arr.is_empty() => {
                let mut v = Vec::new();
                for (i, e) in arr.iter().enumerate() {
                    let o = e
                        .get("old_string")
                        .and_then(Value::as_str)
                        .ok_or_else(|| format!("edits[{i}] 缺 old_string"))?;
                    let n = e
                        .get("new_string")
                        .and_then(Value::as_str)
                        .ok_or_else(|| format!("edits[{i}] 缺 new_string"))?;
                    let ra = e
                        .get("replace_all")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    v.push((o.to_string(), n.to_string(), ra));
                }
                v
            }
            _ => {
                let old = require_str(args, "old_string")?;
                let new = require_str(args, "new_string")?;
                let ra = args
                    .get("replace_all")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                vec![(old, new, ra)]
            }
        };

        let full = resolve(&self.base, &path);
        let mut text =
            std::fs::read_to_string(&full).map_err(|e| format!("读取失败 {path}: {e}"))?;

        // 顺序应用；任一失败则整体放弃（不写盘）。
        let mut total = 0usize;
        for (i, (old, new, replace_all)) in edits.iter().enumerate() {
            let count = text.matches(old.as_str()).count();
            if count == 0 {
                return Err(format!("edits[{i}]：未找到 old_string（{path}）"));
            }
            if !replace_all && count > 1 {
                return Err(format!(
                    "edits[{i}]：old_string 匹配到 {count} 处，不唯一；请加上下文或设 replace_all"
                ));
            }
            text = if *replace_all {
                text.replace(old.as_str(), new)
            } else {
                text.replacen(old.as_str(), new, 1)
            };
            total += if *replace_all { count } else { 1 };
        }
        std::fs::write(&full, text).map_err(|e| format!("写入失败 {path}: {e}"))?;
        Ok(format!(
            "已应用 {} 条编辑（共替换 {total} 处）于 {path}",
            edits.len()
        ))
    }
}

// ── glob ──────────────────────────────────────────────────────────────────────

pub struct Glob {
    base: PathBuf,
}
impl Glob {
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }
}
impl Tool for Glob {
    fn name(&self) -> &'static str {
        "glob"
    }
    fn description(&self) -> &'static str {
        "Find files by glob pattern, e.g. \"**/*.rs\" or \"src/*.ts\". Returns the list of matching file paths."
    }
    fn parameters(&self) -> Value {
        obj_schema(
            json!({
                "pattern": { "type": "string", "description": "Glob pattern" },
                "path": { "type": "string", "description": "Root directory to search (optional, defaults to the working directory)" }
            }),
            &["pattern"],
        )
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "glob {}",
            args.get("pattern").and_then(Value::as_str).unwrap_or("?")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let pattern = require_str(args, "pattern")?;
        let root = match args.get("path").and_then(Value::as_str) {
            Some(p) => resolve(&self.base, p),
            None => self.base.clone(),
        };
        let full_pattern = root.join(&pattern);
        let pat = full_pattern.to_string_lossy();
        let mut hits = Vec::new();
        for p in glob::glob(&pat)
            .map_err(|e| format!("无效 glob: {e}"))?
            .flatten()
        {
            let shown = p.strip_prefix(&self.base).unwrap_or(&p);
            hits.push(shown.to_string_lossy().replace('\\', "/"));
        }
        if hits.is_empty() {
            return Ok("(无匹配)".to_string());
        }
        hits.sort();
        // 结果数上限：`**/*` 之类在大仓库会匹配上万文件、灌爆上下文。截断并提示收窄。
        const MAX_GLOB: usize = 1000;
        let total = hits.len();
        if total > MAX_GLOB {
            hits.truncate(MAX_GLOB);
            hits.push(format!(
                "(已截断：共 {total} 个匹配，仅显示前 {MAX_GLOB} 个；请用更精确的 pattern/path 缩小范围)"
            ));
        }
        Ok(hits.join("\n"))
    }
}

// ── grep ──────────────────────────────────────────────────────────────────────

pub struct Grep {
    base: PathBuf,
}
impl Grep {
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }
}
impl Tool for Grep {
    fn name(&self) -> &'static str {
        "grep"
    }
    fn description(&self) -> &'static str {
        "Search file contents with a regex, returning path:line:matching line. Optionally use path to limit the directory and glob to filter file names."
    }
    fn parameters(&self) -> Value {
        obj_schema(
            json!({
                "pattern": { "type": "string", "description": "Regular expression" },
                "path": { "type": "string", "description": "Directory to search (optional)" },
                "glob": { "type": "string", "description": "File name glob filter, e.g. \"*.rs\" (optional)" }
            }),
            &["pattern"],
        )
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "grep {}",
            args.get("pattern").and_then(Value::as_str).unwrap_or("?")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let pattern = require_str(args, "pattern")?;
        let re = regex::Regex::new(&pattern).map_err(|e| format!("无效正则: {e}"))?;
        let root = match args.get("path").and_then(Value::as_str) {
            Some(p) => resolve(&self.base, p),
            None => self.base.clone(),
        };
        let name_glob = args
            .get("glob")
            .and_then(Value::as_str)
            .and_then(|g| glob::Pattern::new(g).ok());

        const MAX_HITS: usize = 500;
        // 单行字节上限：MUD 存档/压缩数据常把整个数组写在一行，单行可达数十/百 KB。
        const MAX_LINE: usize = 400;
        // 总输出字节预算：防大量超长匹配行累计灌爆上下文。
        const MAX_BYTES: usize = 64_000;
        let mut hits: Vec<String> = Vec::new();
        let mut out_bytes = 0usize;
        'walk: for entry in walkdir::WalkDir::new(&root).into_iter().flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            // 跳过常见无关目录
            if path.components().any(|c| {
                matches!(
                    c.as_os_str().to_str(),
                    Some(".git" | "node_modules" | "target" | "dist")
                )
            }) {
                continue;
            }
            if let Some(g) = &name_glob {
                let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if !g.matches(fname) {
                    continue;
                }
            }
            let Ok(text) = std::fs::read_to_string(path) else {
                continue; // 跳过二进制/不可读
            };
            let shown = path.strip_prefix(&self.base).unwrap_or(path);
            let shown = shown.to_string_lossy().replace('\\', "/");
            for (i, line) in text.lines().enumerate() {
                if re.is_match(line) {
                    // 超长行截断（按字符边界），避免一条存档/数据行拖来几十 KB。
                    let line = line.trim_end();
                    let shown_line = if line.len() > MAX_LINE {
                        let mut end = MAX_LINE;
                        while end > 0 && !line.is_char_boundary(end) {
                            end -= 1;
                        }
                        format!("{}…(行过长已截断)", &line[..end])
                    } else {
                        line.to_string()
                    };
                    let row = format!("{shown}:{}:{shown_line}", i + 1);
                    out_bytes += row.len() + 1;
                    hits.push(row);
                    if hits.len() >= MAX_HITS {
                        hits.push(format!("(已截断，超过 {MAX_HITS} 条匹配；请缩小范围)"));
                        break 'walk;
                    }
                    if out_bytes >= MAX_BYTES {
                        hits.push(
                            "(已截断：输出过大；请用更精确的 pattern/path/glob 缩小范围)"
                                .to_string(),
                        );
                        break 'walk;
                    }
                }
            }
        }
        if hits.is_empty() {
            return Ok("(无匹配)".to_string());
        }
        Ok(hits.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("wc-tools-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn rand_suffix() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    #[test]
    fn write_read_roundtrip_with_line_numbers() {
        let dir = tmp();
        let w = WriteFile::new(dir.clone());
        w.execute(&json!({ "path": "a.txt", "content": "l1\nl2\nl3" }))
            .unwrap();
        let r = ReadFile::new(dir.clone());
        let out = r.execute(&json!({ "path": "a.txt" })).unwrap();
        assert!(out.contains("     1\tl1"));
        assert!(out.contains("     3\tl3"));
        // offset/limit
        let out2 = r
            .execute(&json!({ "path": "a.txt", "offset": 2, "limit": 1 }))
            .unwrap();
        assert!(out2.contains("l2") && !out2.contains("l1") && !out2.contains("l3"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 最小合法 PNG（1×1）：只为验证魔数识别与 data URL 组装，不需要真图。
    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn sniff_recognizes_common_formats_and_rejects_text() {
        assert_eq!(sniff_image_mime(PNG_1X1), Some("image/png"));
        assert_eq!(
            sniff_image_mime(&[0xff, 0xd8, 0xff, 0xe0]),
            Some("image/jpeg")
        );
        assert_eq!(sniff_image_mime(b"GIF89a...."), Some("image/gif"));
        assert_eq!(
            sniff_image_mime(b"RIFF\0\0\0\0WEBPVP8 "),
            Some("image/webp")
        );
        // 纯文本 / 空文件 / 只有 RIFF 头的音频，都不该被当成图片。
        assert_eq!(sniff_image_mime(b"hello"), None);
        assert_eq!(sniff_image_mime(b""), None);
        assert_eq!(sniff_image_mime(b"RIFF\0\0\0\0WAVEfmt "), None);
    }

    #[test]
    fn read_image_returns_data_url_for_the_agent_loop() {
        let dir = tmp();
        std::fs::write(dir.join("shot.png"), PNG_1X1).unwrap();
        let out = ReadImage::new(dir.clone())
            .execute(&json!({ "path": "shot.png" }))
            .unwrap();
        // 必须走 image_result 的约定格式，否则 agent loop 不会把它回灌成视觉消息。
        let (text, images) = crate::tools::split_image_result(&out);
        assert!(text.contains("shot.png"), "文本要点明读了哪张图: {text}");
        assert_eq!(images.len(), 1);
        assert!(images[0].starts_with("data:image/png;base64,"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_image_rejects_non_image_by_content() {
        let dir = tmp();
        // 扩展名撒谎的假图：必须按内容判定，否则发出去才被上游打回。
        std::fs::write(dir.join("fake.png"), b"not an image at all").unwrap();
        let err = ReadImage::new(dir.clone())
            .execute(&json!({ "path": "fake.png" }))
            .unwrap_err();
        assert!(err.contains("不是受支持的图片"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 回归：read_file 撞到图片时，必须明确把模型指向 read_image。
    /// 光报个 UTF-8 错误会让模型改走「看图软件 + 截图」那条又慢又糊的路。
    #[test]
    fn read_file_on_image_points_to_read_image() {
        let dir = tmp();
        std::fs::write(dir.join("pic.png"), PNG_1X1).unwrap();
        let err = ReadFile::new(dir.clone())
            .execute(&json!({ "path": "pic.png" }))
            .unwrap_err();
        assert!(err.contains("read_image"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn edit_requires_unique_match() {
        let dir = tmp();
        WriteFile::new(dir.clone())
            .execute(&json!({ "path": "b.txt", "content": "x foo y foo z" }))
            .unwrap();
        let e = EditFile::new(dir.clone());
        // 非唯一 → 报错
        assert!(e
            .execute(&json!({ "path": "b.txt", "old_string": "foo", "new_string": "bar" }))
            .is_err());
        // replace_all
        e.execute(&json!({ "path": "b.txt", "old_string": "foo", "new_string": "bar", "replace_all": true }))
            .unwrap();
        let out = ReadFile::new(dir.clone())
            .execute(&json!({ "path": "b.txt" }))
            .unwrap();
        assert!(out.contains("x bar y bar z"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn edit_multi_edits_atomic() {
        let dir = tmp();
        WriteFile::new(dir.clone())
            .execute(&json!({ "path": "c.txt", "content": "alpha beta gamma" }))
            .unwrap();
        let e = EditFile::new(dir.clone());
        // 一次多处编辑：按顺序应用。
        e.execute(&json!({
            "path": "c.txt",
            "edits": [
                { "old_string": "alpha", "new_string": "A" },
                { "old_string": "gamma", "new_string": "G" }
            ]
        }))
        .unwrap();
        let out = ReadFile::new(dir.clone())
            .execute(&json!({ "path": "c.txt" }))
            .unwrap();
        assert!(out.contains("A beta G"), "got: {out}");

        // 其中一条找不到 → 整体失败、不写盘（文件保持上一步结果）。
        let err = e.execute(&json!({
            "path": "c.txt",
            "edits": [
                { "old_string": "beta", "new_string": "B" },
                { "old_string": "NOPE", "new_string": "X" }
            ]
        }));
        assert!(err.is_err());
        let out2 = ReadFile::new(dir.clone())
            .execute(&json!({ "path": "c.txt" }))
            .unwrap();
        assert!(out2.contains("A beta G"), "失败应不写盘, got: {out2}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn glob_and_grep_find_files() {
        let dir = tmp();
        let w = WriteFile::new(dir.clone());
        w.execute(&json!({ "path": "src/main.rs", "content": "fn main() { let answer = 42; }" }))
            .unwrap();
        w.execute(&json!({ "path": "src/lib.rs", "content": "pub fn hi() {}" }))
            .unwrap();

        let g = Glob::new(dir.clone())
            .execute(&json!({ "pattern": "**/*.rs" }))
            .unwrap();
        assert!(g.contains("src/main.rs") && g.contains("src/lib.rs"));

        let gr = Grep::new(dir.clone())
            .execute(&json!({ "pattern": "answer = \\d+", "glob": "*.rs" }))
            .unwrap();
        assert!(gr.contains("src/main.rs:1:"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn glob_caps_result_count() {
        let dir = tmp();
        let w = WriteFile::new(dir.clone());
        for i in 0..1100 {
            w.execute(&json!({ "path": format!("f{i}.txt"), "content": "x" }))
                .unwrap();
        }
        let g = Glob::new(dir.clone())
            .execute(&json!({ "pattern": "**/*.txt" }))
            .unwrap();
        assert!(g.contains("已截断"), "超过 1000 个匹配应截断");
        // 行数 = 1000 条路径 + 1 条截断提示。
        assert_eq!(g.lines().count(), 1001);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn grep_truncates_overlong_lines() {
        let dir = tmp();
        let huge = format!("NEWS {}", "数".repeat(5000)); // 单行极长（仿 .o 存档）
        WriteFile::new(dir.clone())
            .execute(&json!({ "path": "data.o", "content": huge }))
            .unwrap();
        let gr = Grep::new(dir.clone())
            .execute(&json!({ "pattern": "NEWS" }))
            .unwrap();
        assert!(gr.contains("行过长已截断"), "超长行应被截断");
        // 整条匹配输出远小于原始 ~15KB 行。
        assert!(gr.len() < 2000, "截断后输出应很小, got {}", gr.len());
        std::fs::remove_dir_all(&dir).ok();
    }
}
