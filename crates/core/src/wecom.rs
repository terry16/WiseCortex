//! 企业微信（WeCom）双向：接收回调 + 主动发应用消息。
//!
//! 凭据存 `wisecortex/wecom.json`（corp_id / corp_secret / agent_id / callback_token /
//! encoding_aes_key）。回调是 XML + msg_signature(sha1)，正文密文 AES-256-CBC。
//! 我们只做**解密**方向（收消息 / GET echostr 验证），回复走应用消息 API（HTTPS，
//! 无需加密），从而避开「5 秒被动回复」限制并省掉加密层。
//!
//! ⚠️ 微信系 PKCS7 块大小是 32（非 16），明文结构为
//! `random(16) + msg_len(4, 大端) + msg + receiveid + pad`。我们用 NoPadding 解密后
//! 按 msg_len 切片取消息、以尾字节值剥 padding 取 receiveid。

use aes::Aes256;
use base64::alphabet;
use base64::engine::{GeneralPurpose, GeneralPurposeConfig};
use base64::Engine;
use cbc::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::time::Duration;

type Aes256CbcDec = cbc::Decryptor<Aes256>;

const QYAPI: &str = "https://qyapi.weixin.qq.com";

// EncodingAESKey 补 '=' 后是非规范 base64（末字符含非零尾比特），需放宽尾比特校验。
const B64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
);

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WecomConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corp_id: Option<String>,
    /// 本应用的 Secret（gettoken 用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corp_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// 回调配置里的 Token（验签用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callback_token: Option<String>,
    /// 回调配置里的 EncodingAESKey（43 字符）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding_aes_key: Option<String>,
}

impl WecomConfig {
    pub fn is_ready(&self) -> bool {
        self.corp_id.is_some()
            && self.corp_secret.is_some()
            && self.agent_id.is_some()
            && self.callback_token.is_some()
            && self.encoding_aes_key.is_some()
    }
}

pub fn config_file() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("wecom.json"))
}

pub fn load() -> WecomConfig {
    config_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &WecomConfig) -> std::io::Result<()> {
    let path = config_file()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "无配置目录"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(cfg)?)
}

fn client() -> Result<reqwest::blocking::Client, String> {
    crate::net::blocking_builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

/// 取应用 access_token（corpid + 本应用 secret）。
pub fn access_token(cfg: &WecomConfig) -> Result<String, String> {
    let (Some(corp_id), Some(secret)) = (&cfg.corp_id, &cfg.corp_secret) else {
        return Err("未配置 corp_id/corp_secret".to_string());
    };
    let resp = client()?
        .get(format!("{QYAPI}/cgi-bin/gettoken"))
        .query(&[("corpid", corp_id), ("corpsecret", secret)])
        .send()
        .map_err(|e| e.to_string())?;
    let v: Value = resp.json().map_err(|e| e.to_string())?;
    v.get("access_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("取 token 失败: {v}"))
}

/// 用应用消息 API 给某用户发文本（主动发，避开被动回复 5 秒限制）。
pub fn send_text(cfg: &WecomConfig, touser: &str, text: &str) -> Result<(), String> {
    let token = access_token(cfg)?;
    let agent_id: i64 = cfg
        .agent_id
        .as_deref()
        .and_then(|s| s.parse().ok())
        .ok_or("agent_id 非法")?;
    let resp = client()?
        .post(format!("{QYAPI}/cgi-bin/message/send"))
        .query(&[("access_token", token.as_str())])
        .json(&json!({
            "touser": touser,
            "msgtype": "text",
            "agentid": agent_id,
            "text": { "content": text }
        }))
        .send()
        .map_err(|e| e.to_string())?;
    let v: Value = resp.json().map_err(|e| e.to_string())?;
    if v.get("errcode").and_then(Value::as_i64) == Some(0) {
        Ok(())
    } else {
        Err(format!("发送失败: {v}"))
    }
}

/// 从一段 XML 里取 `<tag>` 的文本，自动剥 `<![CDATA[ ]]>`。找不到返回 None。
pub fn xml_field(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    let raw = xml[start..end].trim();
    let inner = raw
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
        .unwrap_or(raw);
    Some(inner.to_string())
}

/// 计算回调签名：sha1(sort([token, timestamp, nonce, encrypt]).join(""))，十六进制小写。
pub fn msg_signature(token: &str, timestamp: &str, nonce: &str, encrypt: &str) -> String {
    let mut parts = [token, timestamp, nonce, encrypt];
    parts.sort_unstable();
    let mut hasher = Sha1::new();
    hasher.update(parts.concat().as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(40);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// 解密 base64 密文，返回 (明文消息, receiveid)。
pub fn decrypt(encoding_aes_key: &str, encrypt_b64: &str) -> Result<(String, String), String> {
    // EncodingAESKey 是 43 字符，补 '=' 后 base64 解出 32 字节 AES key；IV = key 前 16 字节。
    let key = B64
        .decode(format!("{encoding_aes_key}="))
        .map_err(|e| format!("aes key 解码失败: {e}"))?;
    if key.len() != 32 {
        return Err(format!("aes key 长度应为 32，实为 {}", key.len()));
    }
    let mut buf = B64
        .decode(encrypt_b64)
        .map_err(|e| format!("密文 base64 解码失败: {e}"))?;
    if buf.is_empty() || buf.len() % 16 != 0 {
        return Err("密文长度非法".to_string());
    }
    // AES-256-CBC 解密（NoPadding，微信用 32 块 PKCS7，自行剥离）。
    let plain = Aes256CbcDec::new(key.as_slice().into(), key[..16].into())
        .decrypt_padded_mut::<NoPadding>(&mut buf)
        .map_err(|e| format!("AES 解密失败: {e}"))?;
    // 剥尾部 padding（最后一字节即填充长度，1..=32）。
    let pad = *plain.last().ok_or("明文为空")? as usize;
    if pad == 0 || pad > 32 || pad > plain.len() {
        return Err("padding 非法".to_string());
    }
    let content = &plain[..plain.len() - pad];
    // 结构：random(16) + msg_len(4 大端) + msg + receiveid
    if content.len() < 20 {
        return Err("明文过短".to_string());
    }
    let msg_len = u32::from_be_bytes([content[16], content[17], content[18], content[19]]) as usize;
    let rest = &content[20..];
    if msg_len > rest.len() {
        return Err("msg_len 越界".to_string());
    }
    let msg = String::from_utf8_lossy(&rest[..msg_len]).into_owned();
    let receiveid = String::from_utf8_lossy(&rest[msg_len..]).into_owned();
    Ok((msg, receiveid))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 企业微信官方 WXBizMsgCrypt sample 的 GET 验证向量。
    #[test]
    fn official_get_echostr_vector() {
        let token = "QDG6eK";
        let aes = "jWmYm7qr5nMoAUwZRjGtBxmz3KA1tkAj3ykkR6q2B2C";
        let timestamp = "1409659589";
        let nonce = "263014780";
        let echostr = "P9nAzCzyDtyTWESHep1vC5X9xho/qYX3Zpb4yKa9SKld1DsH3Iyt3tP3zNdtp+4RPcs8TgAE7OaBO+FZXvnaqQ==";
        let expect_sig = "5c45ff5e21c57e6ad56bac8758b79b1d9ac89fd3";

        assert_eq!(msg_signature(token, timestamp, nonce, echostr), expect_sig);

        let (plain, receiveid) = decrypt(aes, echostr).unwrap();
        assert_eq!(plain, "1616140317555161061");
        assert_eq!(receiveid, "wx5823bf96d3bd56c7");
    }

    #[test]
    fn parses_xml_fields_with_cdata() {
        let inner = "<xml><ToUserName><![CDATA[corp]]></ToUserName>\
            <FromUserName><![CDATA[zhangsan]]></FromUserName>\
            <MsgType><![CDATA[text]]></MsgType>\
            <Content><![CDATA[你好 wisecortex]]></Content></xml>";
        assert_eq!(
            xml_field(inner, "FromUserName").as_deref(),
            Some("zhangsan")
        );
        assert_eq!(
            xml_field(inner, "Content").as_deref(),
            Some("你好 wisecortex")
        );
        assert_eq!(xml_field(inner, "MsgType").as_deref(), Some("text"));
        assert!(xml_field(inner, "PicUrl").is_none());

        // 外层信封的 Encrypt 用同一个提取器。
        let outer = "<xml><ToUserName><![CDATA[corp]]></ToUserName>\
            <Encrypt><![CDATA[ABC123==]]></Encrypt></xml>";
        assert_eq!(xml_field(outer, "Encrypt").as_deref(), Some("ABC123=="));

        // 非 CDATA 的纯文本字段也能取（如 CreateTime）。
        let plain = "<xml><CreateTime>1348831860</CreateTime></xml>";
        assert_eq!(
            xml_field(plain, "CreateTime").as_deref(),
            Some("1348831860")
        );
    }
}
