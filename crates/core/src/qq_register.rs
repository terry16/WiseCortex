//! QQ 官方机器人「扫码绑定」：手机 QQ 扫码后自动拿到 AppID/AppSecret，免手填。
//!
//! 用纯 Rust 重写腾讯官方 SDK `@tencent-connect/qqbot-connector`(v1.1.0) 所用的 q.qq.com 接口
//! （HTTPS + AES-256-GCM），不引入 Node。
//! ⚠️ 这些 `/lite/*` 接口不在公开 OpenAPI 文档里，是该官方 SDK 使用的接口，可能变动；未经真机验证。
//! 扫码页由腾讯托管，接入方默认显示「第三方机器人」（自定义品牌需邮件联系腾讯商务）。
//!
//! 流程（对齐 [`crate::feishu_register`] 的 begin/poll）：
//!   1. 本地生成 32 字节 AES key（base64），POST /lite/create_bind_task {key} -> {task_id}
//!   2. 二维码 URL：https://q.qq.com/qqbot/openclaw/connect.html?task_id=..&source=wisecortex&_wv=2
//!   3. 轮询 /lite/poll_bind_result {task_id} -> {status, bot_appid, bot_encrypt_secret}
//!      status：0 NONE / 1 PENDING / 2 COMPLETED / 3 EXPIRED
//!   4. COMPLETED：AES-256-GCM 解密 bot_encrypt_secret（base64 解出 = IV(12)+密文+Tag(16)）得 AppSecret。

use std::time::Duration;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;
use serde_json::Value;

const HOST: &str = "q.qq.com";

/// 扫码绑定状态码（对齐 SDK BindStatus）。
pub const STATUS_COMPLETED: i64 = 2;
pub const STATUS_EXPIRED: i64 = 3;

/// 新建绑定任务：返回 `(task_id, key_base64)`。`key` 需在 poll 解密时复用，由调用方持有回传。
pub fn create_bind_task() -> Result<(String, String), String> {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let key_b64 = B64.encode(raw);
    let v: Value = crate::net::blocking_builder_with(None)
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?
        .post(format!("https://{HOST}/lite/create_bind_task"))
        .json(&serde_json::json!({ "key": key_b64 }))
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    if v.get("retcode").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Err(format!(
            "create_bind_task 失败：{}",
            v.get("msg").and_then(Value::as_str).unwrap_or("unknown")
        ));
    }
    let task_id = v
        .get("data")
        .and_then(|d| d.get("task_id"))
        .and_then(Value::as_str)
        .ok_or("create_bind_task：缺 task_id")?
        .to_string();
    Ok((task_id, key_b64))
}

/// 扫码页二维码 URL（手机 QQ 扫这个）。
pub fn connect_url(task_id: &str, source: &str) -> String {
    format!(
        "https://{HOST}/qqbot/openclaw/connect.html?task_id={}&source={}&_wv=2",
        urlencode(task_id),
        urlencode(source)
    )
}

/// 仅够 task_id/source 用的百分号编码（保留 RFC3986 unreserved）。
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// 轮询结果，返回 `(status, bot_appid, bot_encrypt_secret)`。
pub fn poll_bind_result(task_id: &str) -> Result<(i64, String, String), String> {
    let v: Value = crate::net::blocking_builder_with(None)
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?
        .post(format!("https://{HOST}/lite/poll_bind_result"))
        .json(&serde_json::json!({ "task_id": task_id }))
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    if v.get("retcode").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Err(format!(
            "poll_bind_result 失败：{}",
            v.get("msg").and_then(Value::as_str).unwrap_or("unknown")
        ));
    }
    let d = v.get("data").cloned().unwrap_or(Value::Null);
    let status = d.get("status").and_then(Value::as_i64).unwrap_or(0);
    let app_id = d.get("bot_appid").map(stringify_id).unwrap_or_default();
    let enc = d
        .get("bot_encrypt_secret")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok((status, app_id, enc))
}

/// `bot_appid` 可能是数字或字符串，统一成字符串。
fn stringify_id(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// AES-256-GCM 解密 AppSecret。
///
/// 密文 base64 解出 = `IV(12) + 密文 + Tag(16)`；`key_b64` 是 [`create_bind_task`] 生成的 base64（32 字节）。
/// aes-gcm 的 `decrypt` 期望 `密文‖Tag`，正好是去掉 IV 后的剩余部分。
pub fn decrypt_secret(encrypted_b64: &str, key_b64: &str) -> Result<String, String> {
    let key_bytes = B64
        .decode(key_b64)
        .map_err(|e| format!("key base64 解码失败：{e}"))?;
    if key_bytes.len() != 32 {
        return Err(format!("key 长度应为 32，实际 {}", key_bytes.len()));
    }
    let blob = B64
        .decode(encrypted_b64)
        .map_err(|e| format!("密文 base64 解码失败：{e}"))?;
    if blob.len() < 12 + 16 {
        return Err("密文过短（应至少含 12B IV + 16B Tag）".into());
    }
    let (iv, rest) = blob.split_at(12);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
    let plain = cipher
        .decrypt(Nonce::from_slice(iv), rest)
        .map_err(|_| "AES-GCM 解密失败（key 或密文不匹配）".to_string())?;
    String::from_utf8(plain).map_err(|_| "解密结果非 UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_url_shape() {
        let u = connect_url("TASK 1", "wisecortex");
        assert_eq!(
            u,
            "https://q.qq.com/qqbot/openclaw/connect.html?task_id=TASK%201&source=wisecortex&_wv=2"
        );
    }

    #[test]
    fn decrypt_roundtrip() {
        // 按真实布局 IV(12) + (密文‖Tag) 造密文，验证 decrypt_secret 能还原。
        let key = [7u8; 32];
        let key_b64 = B64.encode(key);
        let iv = [9u8; 12];
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        let ct = cipher
            .encrypt(Nonce::from_slice(&iv), b"my-secret-123".as_ref())
            .unwrap(); // ct 已含 Tag
        let mut blob = Vec::new();
        blob.extend_from_slice(&iv);
        blob.extend_from_slice(&ct);
        let enc_b64 = B64.encode(blob);
        assert_eq!(decrypt_secret(&enc_b64, &key_b64).unwrap(), "my-secret-123");
        // 错 key 必须失败。
        assert!(decrypt_secret(&enc_b64, &B64.encode([1u8; 32])).is_err());
    }

    #[test]
    fn stringify_id_handles_num_and_str() {
        assert_eq!(stringify_id(&serde_json::json!("102")), "102");
        assert_eq!(stringify_id(&serde_json::json!(102)), "102");
        assert_eq!(stringify_id(&Value::Null), "");
    }
}
