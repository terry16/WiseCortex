//! 图片输入的体检与净化：把模型解不了的图挡在历史之外，并给已经进去的提供退路。
//!
//! **为什么必须有这一层**：一张坏图会把会话*永久*卡死。历史每轮完整重发，坏图永远在里面，
//! 于是每一轮都 400，而 400 按规矩不重试（见 [`super::client::LlmError::is_retryable`]）——
//! 用户从此发不出任何消息，且没有任何自救手段，只能手工去改 session 文件。
//!
//! 实测的触发者是苹果私有的 **CgBI** 变体 PNG（Xcode / iOS 资源管线产物，macOS 某些
//! 复制粘贴路径也会产出）：它在 IHDR **之前**插了一个 `CgBI` 块。标准解码器若按固定偏移
//! 去读尺寸，读到的是 `CgBI` 的内容——于是算出十几亿像素的天文数字。Anthropic 因此回
//! 「At least one of the image dimensions exceed max allowed size: 8000 pixels」。
//!
//! **报错说的是尺寸，真正的病因是格式。** 所以体检不能只比较宽高，必须按块走完 PNG 结构；
//! 只看尺寸的实现会对这张 152×152 的图给出「通过」，然后继续 400。

use super::ChatMessage;

/// 单边像素上限。Anthropic 的硬限制；其它厂商只会更宽松，取最严的即可。
pub const MAX_EDGE: u32 = 8000;

/// 体检不通过的原因。附在日志与历史注记里，便于事后定位是哪一类坏图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// base64 解不开（截断、被别的东西污染）。
    NotBase64,
    /// 认不出的文件签名——既不是 PNG/JPEG/GIF/WebP。
    UnknownFormat,
    /// 苹果私有 CgBI 变体 PNG：IDAT 不是标准 zlib、通道预乘 BGRA，通用解码器读不了。
    AppleCgBi,
    /// PNG 结构损坏（IHDR 不在首块、块长越界等）。
    MalformedPng,
    /// 读不出尺寸（JPEG 里找不到 SOF 帧等）。
    NoDimensions,
    /// 尺寸为 0。
    ZeroSize { w: u32, h: u32 },
    /// 单边超过 [`MAX_EDGE`]。
    TooLarge { w: u32, h: u32 },
}

impl Problem {
    /// 面向人的说明（同 `Tool::summary`，故为中文）。
    pub fn summary(&self) -> String {
        match self {
            Problem::NotBase64 => "base64 数据损坏".to_string(),
            Problem::UnknownFormat => "无法识别的图片格式".to_string(),
            Problem::AppleCgBi => "苹果私有 CgBI 变体 PNG，通用解码器读不了".to_string(),
            Problem::MalformedPng => "PNG 结构损坏".to_string(),
            Problem::NoDimensions => "读不出图片尺寸".to_string(),
            Problem::ZeroSize { w, h } => format!("尺寸为 0（{w}x{h}）"),
            Problem::TooLarge { w, h } => format!("单边超过 {MAX_EDGE} 像素（{w}x{h}）"),
        }
    }
}

/// 从 data URL（或裸 base64）取出字节。宽容处理缺失的 `=` 补位。
fn decode(data_url: &str) -> Result<Vec<u8>, Problem> {
    use base64::Engine;
    let payload = match data_url.split_once(";base64,") {
        Some((_, rest)) => rest,
        None => data_url,
    };
    let payload: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(&payload)
        .or_else(|_| {
            let mut p = payload.clone();
            while !p.len().is_multiple_of(4) {
                p.push('=');
            }
            base64::engine::general_purpose::STANDARD.decode(&p)
        })
        .map_err(|_| Problem::NotBase64)
}

/// 走一遍 PNG 的块结构，返回 (宽, 高)。
///
/// **必须按块走，不能按固定偏移读**——IHDR 通常紧跟签名，但规范并不保证之前没有别的块，
/// 苹果的 CgBI 就插在那儿。按偏移读的实现正是本模块要修的那个 bug。
fn png_dimensions(b: &[u8]) -> Result<(u32, u32), Problem> {
    let mut i = 8usize; // 跳过 8 字节签名
    let mut first = true;
    while i + 8 <= b.len() {
        let len = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        let typ = &b[i + 4..i + 8];
        if typ == b"CgBI" {
            return Err(Problem::AppleCgBi);
        }
        if typ == b"IHDR" {
            if !first {
                // IHDR 之前混了别的块：非标准，通用解码器行为不可预期。
                return Err(Problem::MalformedPng);
            }
            if i + 16 > b.len() {
                return Err(Problem::MalformedPng);
            }
            let w = u32::from_be_bytes([b[i + 8], b[i + 9], b[i + 10], b[i + 11]]);
            let h = u32::from_be_bytes([b[i + 12], b[i + 13], b[i + 14], b[i + 15]]);
            return Ok((w, h));
        }
        first = false;
        if typ == b"IEND" {
            break;
        }
        // 12 = 4 长度 + 4 类型 + 4 CRC。用 checked 防止畸形长度绕回来变成死循环。
        i = match i.checked_add(12).and_then(|x| x.checked_add(len)) {
            Some(n) => n,
            None => return Err(Problem::MalformedPng),
        };
    }
    Err(Problem::MalformedPng)
}

/// 扫 JPEG 的 SOF 帧取尺寸。
fn jpeg_dimensions(b: &[u8]) -> Result<(u32, u32), Problem> {
    let mut i = 2usize;
    while i + 9 <= b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = b[i + 1];
        // SOF0..SOF15，跳过 SOF4(DHT)/SOF8/SOF12。
        if matches!(m, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
            let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
            return Ok((w, h));
        }
        if m == 0xD8 || m == 0xD9 || (0xD0..=0xD7).contains(&m) {
            i += 2;
            continue;
        }
        let seg = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        i = match i.checked_add(2).and_then(|x| x.checked_add(seg)) {
            Some(n) => n,
            None => return Err(Problem::NoDimensions),
        };
    }
    Err(Problem::NoDimensions)
}

/// 给一张图做体检。通过则返回 (宽, 高)。
pub fn inspect(data_url: &str) -> Result<(u32, u32), Problem> {
    let b = decode(data_url)?;
    if b.len() < 16 {
        return Err(Problem::UnknownFormat);
    }
    let (w, h) = if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        png_dimensions(&b)?
    } else if b.starts_with(b"\xff\xd8") {
        jpeg_dimensions(&b)?
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        (
            u16::from_le_bytes([b[6], b[7]]) as u32,
            u16::from_le_bytes([b[8], b[9]]) as u32,
        )
    } else if b.starts_with(b"RIFF") && b.len() > 30 && &b[8..12] == b"WEBP" {
        // 只认扩展格式（VP8X）；VP8/VP8L 的尺寸位域另说，读不出就按「读不出」处理，
        // 不猜——猜错会把好图误删。
        if &b[12..16] == b"VP8X" {
            (
                (u32::from(b[24]) | u32::from(b[25]) << 8 | u32::from(b[26]) << 16) + 1,
                (u32::from(b[27]) | u32::from(b[28]) << 8 | u32::from(b[29]) << 16) + 1,
            )
        } else {
            return Err(Problem::NoDimensions);
        }
    } else {
        return Err(Problem::UnknownFormat);
    };

    if w == 0 || h == 0 {
        return Err(Problem::ZeroSize { w, h });
    }
    if w > MAX_EDGE || h > MAX_EDGE {
        return Err(Problem::TooLarge { w, h });
    }
    Ok((w, h))
}

/// 一次净化的结果。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Cleaned {
    /// 被移除的图片数。
    pub removed: usize,
    /// 各类问题的说明（去重后），用于日志与提示。
    pub reasons: Vec<String>,
}

impl Cleaned {
    pub fn is_empty(&self) -> bool {
        self.removed == 0
    }
}

/// 剔除历史里体检不通过的图片，并在该消息正文留一行说明。
///
/// 只删图不删消息：那一轮的文字（模型对图的描述、用户的提问）价值最高，必须留着。
/// 返回被删的张数与原因。
pub fn sanitize(messages: &mut [ChatMessage]) -> Cleaned {
    let mut out = Cleaned::default();
    for m in messages.iter_mut() {
        if m.images.is_empty() {
            continue;
        }
        let mut kept = Vec::with_capacity(m.images.len());
        let mut dropped = 0usize;
        for img in std::mem::take(&mut m.images) {
            match inspect(&img) {
                Ok(_) => kept.push(img),
                Err(p) => {
                    dropped += 1;
                    let s = p.summary();
                    if !out.reasons.contains(&s) {
                        out.reasons.push(s);
                    }
                }
            }
        }
        m.images = kept;
        if dropped > 0 {
            out.removed += dropped;
            let note = format!("（注：此处原附的 {dropped} 张图片模型无法解码，已移除。）");
            m.content = Some(match m.content.take() {
                Some(c) if !c.trim().is_empty() => format!("{}\n{note}", c.trim_end()),
                _ => note,
            });
        }
    }
    out
}

/// 这条 API 错误是不是「图片惹的祸」。
///
/// 各家的措辞不一样，但都会点到 image/图片相关的字眼。**故意放宽**：宁可对一条本与图片
/// 无关的 400 多做一次「剥图重试」（代价是丢掉本轮图片），也不要让会话永久卡死——
/// 后者用户完全无法自救。
pub fn looks_like_image_error(status: u16, body: &str) -> bool {
    if status != 400 {
        return false;
    }
    let b = body.to_ascii_lowercase();
    b.contains("image")
        || b.contains("media_type")
        || b.contains("dimensions")
        || b.contains("图片")
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    fn url(bytes: &[u8]) -> String {
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    }

    /// 造一个 PNG：可选在 IHDR 前插入一个 CgBI 块。
    fn png(w: u32, h: u32, cgbi: bool) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        if cgbi {
            b.extend_from_slice(&4u32.to_be_bytes());
            b.extend_from_slice(b"CgBI");
            b.extend_from_slice(&[0x50, 0x00, 0x20, 0x06]);
            b.extend_from_slice(&[0; 4]); // CRC 占位
        }
        b.extend_from_slice(&13u32.to_be_bytes());
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0]);
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(b"IEND");
        b.extend_from_slice(&[0; 4]);
        b
    }

    #[test]
    fn accepts_a_normal_png() {
        assert_eq!(inspect(&url(&png(1144, 792, false))), Ok((1144, 792)));
    }

    /// 本模块存在的理由：这张图只有 152×152，**按尺寸判断会放行**，
    /// 然后每一轮都被上游回 400。必须按格式识别出来。
    #[test]
    fn rejects_apple_cgbi_png_even_though_it_is_tiny() {
        let e = inspect(&url(&png(152, 152, true))).unwrap_err();
        assert_eq!(e, Problem::AppleCgBi);
    }

    #[test]
    fn rejects_oversized_and_zero_sized() {
        assert_eq!(
            inspect(&url(&png(9000, 100, false))),
            Err(Problem::TooLarge { w: 9000, h: 100 })
        );
        assert_eq!(
            inspect(&url(&png(0, 100, false))),
            Err(Problem::ZeroSize { w: 0, h: 100 })
        );
    }

    #[test]
    fn rejects_garbage_and_unknown_formats() {
        assert_eq!(
            inspect("data:image/png;base64,@@@@"),
            Err(Problem::NotBase64)
        );
        assert_eq!(
            inspect(&url(b"not an image at all!!")),
            Err(Problem::UnknownFormat)
        );
    }

    #[test]
    fn reads_jpeg_and_gif_dimensions() {
        let mut j = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
        j.extend_from_slice(&600u16.to_be_bytes()); // 高
        j.extend_from_slice(&800u16.to_be_bytes()); // 宽
        j.extend_from_slice(&[0; 8]);
        assert_eq!(inspect(&url(&j)), Ok((800, 600)));

        let mut g = b"GIF89a".to_vec();
        g.extend_from_slice(&320u16.to_le_bytes());
        g.extend_from_slice(&240u16.to_le_bytes());
        g.extend_from_slice(&[0; 16]);
        assert_eq!(inspect(&url(&g)), Ok((320, 240)));
    }

    /// 畸形块长不能把扫描器拖进死循环或 panic。
    #[test]
    fn malformed_chunk_length_terminates() {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        b.extend_from_slice(&u32::MAX.to_be_bytes());
        b.extend_from_slice(b"junk");
        b.extend_from_slice(&[0; 16]);
        assert_eq!(inspect(&url(&b)), Err(Problem::MalformedPng));
    }

    #[test]
    fn sanitize_drops_only_the_bad_image_and_keeps_the_text() {
        let good = url(&png(100, 100, false));
        let bad = url(&png(152, 152, true));
        let mut hist = vec![
            ChatMessage::user("没有图的一条"),
            ChatMessage::user_with_images(
                "（以上工具返回的截图）".to_string(),
                vec![good.clone(), bad.clone()],
            ),
        ];
        let r = sanitize(&mut hist);
        assert_eq!(r.removed, 1);
        assert_eq!(hist[1].images, vec![good]);
        let c = hist[1].content.as_deref().unwrap();
        assert!(c.starts_with("（以上工具返回的截图）"), "原文必须保留：{c}");
        assert!(c.contains("已移除"), "要留下痕迹，不能悄悄删：{c}");
        // 无图消息不受影响。
        assert_eq!(hist[0].content.as_deref(), Some("没有图的一条"));
    }

    /// 全是好图时必须原样不动——净化不能有副作用。
    #[test]
    fn sanitize_is_a_noop_when_everything_is_fine() {
        let good = url(&png(10, 10, false));
        let mut hist = vec![ChatMessage::user_with_images(
            "看图".to_string(),
            vec![good],
        )];
        let before = hist.clone();
        assert!(sanitize(&mut hist).is_empty());
        assert_eq!(hist, before);
    }

    #[test]
    fn image_error_detection() {
        let real = r#"{"type":"error","error":{"message":"messages.26.content.1.image.source.base64.data: At least one of the image dimensions exceed max allowed size: 8000 pixels"}}"#;
        assert!(looks_like_image_error(400, real));
        // 非 400 不算，免得把限流/服务端故障也当成图片问题去剥图。
        assert!(!looks_like_image_error(429, real));
        assert!(!looks_like_image_error(400, "context length exceeded"));
    }
}
