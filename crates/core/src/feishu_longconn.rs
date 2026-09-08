//! 飞书长连接（WebSocket，免公网回调）的协议层：pbbp2 `Frame` 编解码 + 端点协商 + 分片重组。
//!
//! ⚠️ 飞书这套长连接协议官方只提供 Go/Python/Node SDK，无 Rust SDK，故此处自行实现；
//! 无法在缺少真实飞书租户/网络时验证。可单测的部分（帧编解码、端点解析、分片重组、事件抽取）已覆盖；
//! 真实 wss 握手/帧语义需用真实应用联网验收，必要时按真机微调字段。
//!
//! 帧（pbbp2.Frame）：seqid(1) logid(2) service(3) method(4) headers(5) payload_encoding(6)
//! payload_type(7) payload(8) log_id_new(9)；Header{key(1),value(2)}。控制/数据用 header `type` 区分：
//! ping/pong 为心跳；event/card 为数据（header 带 message_id/sum/seq，payload 为事件 JSON，可分片/可 gzip）。

use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;

use serde_json::Value;

// ── protobuf 极简 wire 编解码（仅够本 Frame 用）─────────────────────────────
fn put_varint(buf: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            buf.push(b | 0x80);
        } else {
            buf.push(b);
            break;
        }
    }
}
fn read_varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut shift = 0u32;
    let mut out = 0u64;
    loop {
        let b = *buf.get(*pos)?;
        *pos += 1;
        out |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(out);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
}
fn put_tag(buf: &mut Vec<u8>, field: u32, wire: u32) {
    put_varint(buf, ((field << 3) | wire) as u64);
}
fn put_len_field(buf: &mut Vec<u8>, field: u32, bytes: &[u8]) {
    put_tag(buf, field, 2);
    put_varint(buf, bytes.len() as u64);
    buf.extend_from_slice(bytes);
}

/// 飞书长连接帧。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub seq_id: u64,
    pub log_id: u64,
    pub service: i32,
    pub method: i32,
    pub headers: Vec<(String, String)>,
    pub payload_encoding: String,
    pub payload_type: String,
    pub payload: Vec<u8>,
    pub log_id_new: String,
}

impl Frame {
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::new();
        put_tag(&mut b, 1, 0);
        put_varint(&mut b, self.seq_id);
        put_tag(&mut b, 2, 0);
        put_varint(&mut b, self.log_id);
        put_tag(&mut b, 3, 0);
        put_varint(&mut b, self.service as u64);
        put_tag(&mut b, 4, 0);
        put_varint(&mut b, self.method as u64);
        for (k, v) in &self.headers {
            let mut h = Vec::new();
            put_len_field(&mut h, 1, k.as_bytes());
            put_len_field(&mut h, 2, v.as_bytes());
            put_len_field(&mut b, 5, &h);
        }
        if !self.payload_encoding.is_empty() {
            put_len_field(&mut b, 6, self.payload_encoding.as_bytes());
        }
        if !self.payload_type.is_empty() {
            put_len_field(&mut b, 7, self.payload_type.as_bytes());
        }
        if !self.payload.is_empty() {
            put_len_field(&mut b, 8, &self.payload);
        }
        if !self.log_id_new.is_empty() {
            put_len_field(&mut b, 9, self.log_id_new.as_bytes());
        }
        b
    }

    pub fn decode(buf: &[u8]) -> Result<Frame, String> {
        let mut f = Frame::default();
        let mut pos = 0usize;
        while pos < buf.len() {
            let tag = read_varint(buf, &mut pos).ok_or("帧 tag 截断")?;
            let field = (tag >> 3) as u32;
            let wire = (tag & 7) as u32;
            match (field, wire) {
                (1, 0) => f.seq_id = read_varint(buf, &mut pos).ok_or("seqid")?,
                (2, 0) => f.log_id = read_varint(buf, &mut pos).ok_or("logid")?,
                (3, 0) => f.service = read_varint(buf, &mut pos).ok_or("service")? as i32,
                (4, 0) => f.method = read_varint(buf, &mut pos).ok_or("method")? as i32,
                (5, 2) => {
                    let bytes = read_len(buf, &mut pos)?;
                    f.headers.push(decode_header(&bytes)?);
                }
                (6, 2) => f.payload_encoding = read_len_str(buf, &mut pos)?,
                (7, 2) => f.payload_type = read_len_str(buf, &mut pos)?,
                (8, 2) => f.payload = read_len(buf, &mut pos)?,
                (9, 2) => f.log_id_new = read_len_str(buf, &mut pos)?,
                // 未知字段：按 wire 类型跳过，保持向前兼容。
                (_, 0) => {
                    read_varint(buf, &mut pos).ok_or("跳过 varint")?;
                }
                (_, 2) => {
                    read_len(buf, &mut pos)?;
                }
                (_, w) => return Err(format!("不支持的 wire 类型 {w}")),
            }
        }
        Ok(f)
    }
}

fn read_len(buf: &[u8], pos: &mut usize) -> Result<Vec<u8>, String> {
    let len = read_varint(buf, pos).ok_or("长度截断")? as usize;
    let end = pos.checked_add(len).ok_or("长度溢出")?;
    let slice = buf.get(*pos..end).ok_or("内容截断")?.to_vec();
    *pos = end;
    Ok(slice)
}
fn read_len_str(buf: &[u8], pos: &mut usize) -> Result<String, String> {
    String::from_utf8(read_len(buf, pos)?).map_err(|_| "非 UTF-8 字符串".to_string())
}
fn decode_header(buf: &[u8]) -> Result<(String, String), String> {
    let (mut k, mut v) = (String::new(), String::new());
    let mut pos = 0;
    while pos < buf.len() {
        let tag = read_varint(buf, &mut pos).ok_or("header tag")?;
        match tag >> 3 {
            1 => k = read_len_str(buf, &mut pos)?,
            2 => v = read_len_str(buf, &mut pos)?,
            _ => {
                read_len(buf, &mut pos)?;
            }
        }
    }
    Ok((k, v))
}

/// 是否心跳帧（header type=ping）。
pub fn is_ping(f: &Frame) -> bool {
    f.header("type") == Some("ping")
}

/// 由 ping 构造 pong：沿用 headers，把 type 改成 pong，清空 payload。
pub fn build_pong(ping: &Frame) -> Frame {
    let headers = ping
        .headers
        .iter()
        .map(|(k, v)| {
            if k == "type" {
                (k.clone(), "pong".to_string())
            } else {
                (k.clone(), v.clone())
            }
        })
        .collect();
    Frame {
        seq_id: ping.seq_id,
        log_id: ping.log_id,
        service: ping.service,
        method: ping.method,
        headers,
        ..Default::default()
    }
}

/// 长连接下「回给飞书的应答帧」的 payload 包封。
///
/// 飞书要求 payload 是一个信封，而不是裸的结果 JSON：
/// - 无返回体（普通事件的回执）→ `{"code":200}`
/// - 有返回体（卡片回调的 toast / 新卡片）→ `{"code":200,"data":"<base64(JSON(result))>"}`
///
/// ⚠️ `data` 是把结果 JSON **再 base64 编码一次**的字符串，不是嵌套 JSON。照官方 SDK
/// （Python `lark_oapi/ws/client.py`：`resp.data = base64.b64encode(JSON.marshal(result))`）。
/// 早先把 toast JSON 裸塞进 payload：飞书解不出来 → 3 秒超时 → 按钮转圈后还原、毫无提示。
/// 普通事件当时之所以没事，纯属巧合——`{"code":200}` 恰好就是本信封的「无 data」形态。
pub fn response_payload(result: Option<&Value>) -> Vec<u8> {
    use base64::Engine;
    let mut env = serde_json::Map::new();
    env.insert("code".to_string(), Value::from(200));
    if let Some(v) = result {
        let b64 = base64::engine::general_purpose::STANDARD.encode(v.to_string());
        env.insert("data".to_string(), Value::from(b64));
    }
    serde_json::to_vec(&Value::Object(env)).unwrap_or_else(|_| br#"{"code":200}"#.to_vec())
}

/// 取帧解码后的事件 payload（处理 gzip）。
pub fn decode_payload(f: &Frame) -> Result<Vec<u8>, String> {
    if f.payload_encoding == "gzip" {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&f.payload[..])
            .read_to_end(&mut out)
            .map_err(|e| format!("gzip 解压失败：{e}"))?;
        Ok(out)
    } else {
        Ok(f.payload.clone())
    }
}

/// 分片重组：按 message_id 收集 sum 个分片，齐了返回完整 payload 字节。
#[derive(Default)]
pub struct Reassembler {
    parts: HashMap<String, Vec<Option<Vec<u8>>>>,
}

impl Reassembler {
    /// 投入一个数据帧；返回 Some(完整 payload) 当该 message_id 的分片到齐。
    pub fn push(&mut self, f: &Frame) -> Result<Option<Vec<u8>>, String> {
        let payload = decode_payload(f)?;
        let mid = f.header("message_id").unwrap_or("").to_string();
        let sum: usize = f.header("sum").and_then(|s| s.parse().ok()).unwrap_or(1);
        let seq: usize = f.header("seq").and_then(|s| s.parse().ok()).unwrap_or(0);
        // 不分片：直接返回。
        if sum <= 1 || mid.is_empty() {
            return Ok(Some(payload));
        }
        let slot = self
            .parts
            .entry(mid.clone())
            .or_insert_with(|| vec![None; sum]);
        if seq < slot.len() {
            slot[seq] = Some(payload);
        }
        if slot.iter().all(Option::is_some) {
            let full: Vec<u8> = self
                .parts
                .remove(&mid)
                .unwrap()
                .into_iter()
                .flatten()
                .flatten()
                .collect();
            Ok(Some(full))
        } else {
            Ok(None)
        }
    }
}

/// 端点协商结果。
#[derive(Debug, Clone, PartialEq)]
pub struct Endpoint {
    pub url: String,
    pub ping_interval_secs: u64,
}

/// 解析 `/callback/ws/endpoint` 响应（纯函数）。
pub fn parse_endpoint(v: &Value) -> Result<Endpoint, String> {
    if v.get("code").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Err(format!(
            "端点协商失败：{}",
            v.get("msg").and_then(Value::as_str).unwrap_or("unknown")
        ));
    }
    let data = v.get("data").ok_or("缺少 data")?;
    let url = data
        .get("URL")
        .or_else(|| data.get("url"))
        .and_then(Value::as_str)
        .ok_or("缺少 URL")?
        .to_string();
    let ping = data
        .get("ClientConfig")
        .and_then(|c| c.get("PingInterval"))
        .and_then(Value::as_u64)
        .unwrap_or(120);
    Ok(Endpoint {
        url,
        ping_interval_secs: ping,
    })
}

/// 协商长连接端点（blocking，经代理）。
pub fn endpoint(app_id: &str, app_secret: &str) -> Result<Endpoint, String> {
    let v: Value = crate::net::blocking_builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?
        .post("https://open.feishu.cn/callback/ws/endpoint")
        .json(&serde_json::json!({ "AppID": app_id, "AppSecret": app_secret }))
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    parse_endpoint(&v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn response_payload_without_result_is_bare_code_200() {
        assert_eq!(
            String::from_utf8(response_payload(None)).unwrap(),
            r#"{"code":200}"#
        );
    }

    #[test]
    fn response_payload_base64_encodes_the_result_json() {
        use base64::Engine;
        let toast = json!({ "toast": { "type": "success", "content": "已切换" } });
        let v: Value = serde_json::from_slice(&response_payload(Some(&toast))).unwrap();
        assert_eq!(v["code"], 200);
        // data 必须是 base64 字符串，**不是嵌套 JSON**——裸塞 JSON 正是按钮转圈还原的原因。
        let b64 = v["data"]
            .as_str()
            .expect("data 应是 base64 字符串，不是对象");
        let raw = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&raw).unwrap(), toast);
    }

    #[test]
    fn frame_encode_decode_roundtrip() {
        let f = Frame {
            seq_id: 7,
            log_id: 42,
            service: 1,
            method: 3,
            headers: vec![
                ("type".into(), "event".into()),
                ("message_id".into(), "m1".into()),
                ("sum".into(), "1".into()),
                ("seq".into(), "0".into()),
            ],
            payload_encoding: String::new(),
            payload_type: "json".into(),
            payload: b"{\"a\":1}".to_vec(),
            log_id_new: "ln".into(),
        };
        let back = Frame::decode(&f.encode()).unwrap();
        assert_eq!(back, f);
        assert_eq!(back.header("message_id"), Some("m1"));
    }

    #[test]
    fn ping_detection_and_pong() {
        let ping = Frame {
            headers: vec![("type".into(), "ping".into()), ("k".into(), "v".into())],
            seq_id: 9,
            ..Default::default()
        };
        assert!(is_ping(&ping));
        let pong = build_pong(&ping);
        assert_eq!(pong.header("type"), Some("pong"));
        assert_eq!(pong.header("k"), Some("v"));
        assert_eq!(pong.seq_id, 9);
        assert!(!is_ping(&pong));
    }

    #[test]
    fn reassembler_single_and_chunked() {
        let mut r = Reassembler::default();
        // 不分片直接出。
        let f = Frame {
            headers: vec![
                ("message_id".into(), "a".into()),
                ("sum".into(), "1".into()),
            ],
            payload: b"hello".to_vec(),
            ..Default::default()
        };
        assert_eq!(r.push(&f).unwrap(), Some(b"hello".to_vec()));
        // 两片重组。
        let f0 = Frame {
            headers: vec![
                ("message_id".into(), "b".into()),
                ("sum".into(), "2".into()),
                ("seq".into(), "0".into()),
            ],
            payload: b"AB".to_vec(),
            ..Default::default()
        };
        let f1 = Frame {
            headers: vec![
                ("message_id".into(), "b".into()),
                ("sum".into(), "2".into()),
                ("seq".into(), "1".into()),
            ],
            payload: b"CD".to_vec(),
            ..Default::default()
        };
        assert_eq!(r.push(&f0).unwrap(), None);
        assert_eq!(r.push(&f1).unwrap(), Some(b"ABCD".to_vec()));
    }

    #[test]
    fn parse_endpoint_reads_url_and_ping() {
        let v = json!({
            "code": 0,
            "data": { "URL": "wss://host/ws?x=1", "ClientConfig": { "PingInterval": 120 } }
        });
        let e = parse_endpoint(&v).unwrap();
        assert_eq!(e.url, "wss://host/ws?x=1");
        assert_eq!(e.ping_interval_secs, 120);
        // 错误码。
        assert!(parse_endpoint(&json!({ "code": 1, "msg": "bad" })).is_err());
    }
}
