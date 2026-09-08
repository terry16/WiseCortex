//! 上传附件落盘：把前端发来的 data URL（`data:<mime>;base64,<data>`）解码写入目标目录，
//! 供 agent 用 read_file/shell 等工具处理（图片走 vision，其余文档如 PDF 走此路径）。

use std::path::{Path, PathBuf};

use base64::Engine;

/// 把文件名收敛为安全的叶名（去路径分隔与控制字符；空则给默认名）。
fn sanitize_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "upload".to_string()
    } else {
        trimmed
    }
}

/// 解析 data URL 的 base64 负载（无 `base64,` 前缀则视为整体即 base64）。
fn data_url_payload(data_url: &str) -> &str {
    match data_url.find("base64,") {
        Some(i) => &data_url[i + "base64,".len()..],
        None => data_url.split_once(',').map(|(_, b)| b).unwrap_or(data_url),
    }
}

/// 把一个 data URL 解码并写入 `dir`（不存在则创建），返回写入的完整路径。
/// 同名冲突时追加 `-1`、`-2`… 避免覆盖。
pub fn save_data_url(dir: &Path, name: &str, data_url: &str) -> std::io::Result<PathBuf> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_url_payload(data_url).trim())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::create_dir_all(dir)?;

    let safe = sanitize_name(name);
    let mut target = dir.join(&safe);
    if target.exists() {
        let (stem, ext) = match safe.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (safe.clone(), String::new()),
        };
        let mut n = 1;
        loop {
            let cand = dir.join(format!("{stem}-{n}{ext}"));
            if !cand.exists() {
                target = cand;
                break;
            }
            n += 1;
        }
    }
    std::fs::write(&target, bytes)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_deconflicts() {
        let dir = std::env::temp_dir().join(format!("wc-upload-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        // "hello" base64 = aGVsbG8=
        let url = "data:text/plain;base64,aGVsbG8=";
        let p1 = save_data_url(&dir, "a.txt", url).unwrap();
        assert_eq!(std::fs::read_to_string(&p1).unwrap(), "hello");
        // 同名再写 → 去冲突为 a-1.txt
        let p2 = save_data_url(&dir, "a.txt", url).unwrap();
        assert_ne!(p1, p2);
        assert!(p2.file_name().unwrap().to_str().unwrap().contains("a-1"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sanitizes_path_traversal_in_name() {
        let dir = std::env::temp_dir().join(format!("wc-upload-sani-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let p = save_data_url(&dir, "../../evil.txt", "data:;base64,aGVsbG8=").unwrap();
        // 应落在 dir 下、文件名为 evil.txt（路径成分被剥离）。
        assert_eq!(p.parent().unwrap(), dir);
        assert_eq!(p.file_name().unwrap().to_str().unwrap(), "evil.txt");
        std::fs::remove_dir_all(&dir).ok();
    }
}
