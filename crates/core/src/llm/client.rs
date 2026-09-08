//! 流式 LLM 客户端：按 WireFormat 选择端点/头/体，解析 SSE，聚合为 LlmResponse。

use futures_util::StreamExt;
use serde_json::json;

use super::{
    anthropic, gemini, oauth, oauth_gemini, oauth_openai, oauth_xai, openai, responses,
    ChatMessage, LlmRequest, LlmResponse, Role, ThinkingMode, WireFormat,
};

/// 一次调用所需的运行时配置（BYOK）。
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// 例如 https://api.deepseek.com 或 https://api.anthropic.com。
    pub base_url: String,
    pub api_key: String,
    pub format: WireFormat,
    /// OpenAI 线缆下深度思考参数的表达方式（见 [`ThinkingMode`]）。Anthropic 线缆忽略。
    pub thinking: ThinkingMode,
    /// 为 true 时走 Claude 订阅 OAuth：请求时实时取（按需刷新）access token，用
    /// `Authorization: Bearer` + oauth beta 头鉴权，并在 system 前置 Claude Code 身份句。
    /// 仅对 Anthropic 线缆有意义；此时 `api_key` 可空。
    pub use_claude_oauth: bool,
    /// 为 true 时走 ChatGPT/Codex 订阅 OAuth：请求时实时取 access token + account_id，
    /// 用 `Authorization: Bearer` + `ChatGPT-Account-Id` 头鉴权。仅对 OpenAiResponses 线缆有意义。
    pub use_openai_codex: bool,
    /// 为 true 时走 xAI Grok 订阅 OAuth：请求时实时取（按需刷新）access token，
    /// 作 `Authorization: Bearer` 代替 api_key。仅对 OpenAi 线缆有意义；此时 `api_key` 可空。
    pub use_xai_grok: bool,
    /// 为 true 时走 Gemini（Google 账号 / Code Assist）订阅 OAuth：请求时实时取 access token +
    /// project_id，用 `Authorization: Bearer` 鉴权、把 project 塞进 Code Assist 信封。仅对 Gemini
    /// 线缆有意义；此时 `api_key` 可空。
    pub use_gemini_oauth: bool,
    /// 目标模型是否支持图片输入（vision）。**保守默认 false**：为 false 时，发送前会把所有
    /// 消息里的图片剥掉、替换成文字占位（见 [`redact_images`]），避免给 deepseek-v4 这类不支持
    /// 多模态的模型发 image 块被打回 4xx。由调用方按 `providers::supports_vision` 解析后设入。
    pub vision: bool,
}

impl ProviderConfig {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        format: WireFormat,
    ) -> Self {
        ProviderConfig {
            base_url: base_url.into(),
            api_key: api_key.into(),
            format,
            thinking: ThinkingMode::default(),
            use_claude_oauth: false,
            use_openai_codex: false,
            use_xai_grok: false,
            use_gemini_oauth: false,
            vision: false,
        }
    }

    /// 设定深度思考参数的表达方式（按 provider 预设）。
    pub fn with_thinking(mut self, thinking: ThinkingMode) -> Self {
        self.thinking = thinking;
        self
    }

    /// 标记本档走 Claude 订阅 OAuth 鉴权（见 [`ProviderConfig::use_claude_oauth`]）。
    pub fn with_claude_oauth(mut self, v: bool) -> Self {
        self.use_claude_oauth = v;
        self
    }

    /// 标记本档走 ChatGPT/Codex 订阅 OAuth 鉴权（见 [`ProviderConfig::use_openai_codex`]）。
    pub fn with_openai_codex(mut self, v: bool) -> Self {
        self.use_openai_codex = v;
        self
    }

    /// 标记本档走 xAI Grok 订阅 OAuth 鉴权（见 [`ProviderConfig::use_xai_grok`]）。
    pub fn with_xai_grok(mut self, v: bool) -> Self {
        self.use_xai_grok = v;
        self
    }

    /// 标记本档走 Gemini 订阅 OAuth 鉴权（见 [`ProviderConfig::use_gemini_oauth`]）。
    pub fn with_gemini_oauth(mut self, v: bool) -> Self {
        self.use_gemini_oauth = v;
        self
    }

    /// 标记目标模型是否支持图片输入（见 [`ProviderConfig::vision`]）。
    pub fn with_vision(mut self, v: bool) -> Self {
        self.vision = v;
        self
    }
}

/// 流式回调收到的更新：文本增量、最新的 token 用量，或自动重试的等待倒计时。
#[derive(Debug, Clone, PartialEq)]
pub enum StreamUpdate {
    Text(String),
    Usage {
        input: u64,
        output: u64,
    },
    /// 正在等待自动重试。**等待期间会持续推送**（倒计时刷新），让 UI 能显示
    /// 「还有多久重试 · 第几/共几次」，而不是干转 thinking 把用户蒙在鼓里。
    Retrying {
        attempt: u32,
        max: u32,
        wait_secs: u64,
        kind: RetryKind,
    },
}

/// 自动重试的成因。**必须推给 UI**：只写 stderr 等于没写——桌面端 GUI 不接管子进程的 stderr，
/// 那行 `eprintln!` 谁也看不到，用户看到的就是「转了半天、然后报错」，与「根本没重试」无从分辨。
/// 这正是 2026-07-30 那次 529 的用户观感（「还不会自动重试」）：其实一直在重试，只是没人看得见。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryKind {
    /// 短时突发限流（429）的快退避。
    RateLimitFast,
    /// 已判定为账号额度上限（429）的固定长等待。
    RateLimitLong,
    /// 上游服务端故障（5xx）。带上状态码：529 过载和 502 网关挂了，用户的处置完全不同。
    ServerError(u16),
}

impl RetryKind {
    /// 状态行里「在等什么」的说法。面向人，故为中文（同 `Tool::summary`）。
    pub fn label(self) -> String {
        match self {
            RetryKind::RateLimitFast => "瞬时限流".to_string(),
            RetryKind::RateLimitLong => "账号额度限流".to_string(),
            // 529 是上游过载，点名它，免得用户以为是自己配置错了。
            RetryKind::ServerError(529) => "上游过载（529）".to_string(),
            RetryKind::ServerError(s) => format!("上游服务端故障（{s}）"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("HTTP 传输错误: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API 返回 {status}: {body}")]
    Api { status: u16, body: String },
    /// 流式空闲超时：连续 N 秒未收到任何数据，判定连接卡死（区别于「正常但慢」——
    /// 只要还在产出 token/心跳就会刷新计时，不会误杀长思考）。
    #[error("流式响应空闲超时：{0} 秒内无数据，连接疑似卡死")]
    Timeout(u64),
}

impl LlmError {
    /// 是否值得重试：传输层错误（超时/连接/响应体解码失败等）一律可重试；
    /// 空闲超时（连接卡死）可重试；API 错误仅 429（限流）与 5xx（网关/服务端临时故障）
    /// 可重试，4xx 不重试。
    pub fn is_retryable(&self) -> bool {
        match self {
            LlmError::Http(_) => true,
            LlmError::Timeout(_) => true,
            LlmError::Api { status, .. } => *status == 429 || (500..=599).contains(status),
        }
    }

    /// 是否为限流（429）。限流不同于普通瞬时错误：几秒钟的指数退避解决不了「账号额度用尽」，
    /// 只能隔一段时间（默认 10 分钟）再试、等额度窗口恢复——故与 5xx/传输错误分开处理。
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, LlmError::Api { status: 429, .. })
    }

    /// 是否为**上游服务端**故障（5xx；含流内 `overloaded_error`→529、`api_error`→503）。
    ///
    /// 单列出来是因为它和传输层抖动的时间尺度差一个数量级：本地代理/网关瞬断几百毫秒就好，
    /// 而 Anthropic 的 529 overloaded / 503 api_error 是**几十秒到几分钟**级的容量事件
    /// （实测 2026-07-30 一次持续 17 分钟）。拿 500ms/1s/2s 三下、1.5 秒内打完的快退避去接，
    /// 等于不接——用户会突然看见一个原样透出的 503。
    pub fn is_server_error(&self) -> bool {
        self.server_status().is_some()
    }

    /// 上游 5xx 的状态码；非 5xx 返回 `None`。推给 UI 用：529（过载，等等就好）和 502（网关挂了，
    /// 该查代理）对用户是两回事，状态行上必须能分辨。
    pub fn server_status(&self) -> Option<u16> {
        match self {
            LlmError::Api { status, .. } if (500..=599).contains(status) => Some(*status),
            _ => None,
        }
    }
}

/// 把订阅鉴权失败（`valid_access*` 返回的错误串）映射为 [`LlmError`]。
///
/// 关键在**区分可重试与否**：内层若打了 [`crate::net::TRANSIENT_TAG`]，说明是网络层瞬时失败
/// （token 刷新 / onboarding 撞上反代抽风、超时、瞬断）——映射为可重试的 503，交给 `complete()`
/// 的瞬时重试兜住，偶发抖动对用户无感。否则是真正的鉴权问题（未登录 / refresh 失效 / 缺 GCP
/// 项目），映射为终态 401——重试一百次也没用，直接把内层给出的可照做的提示透出来。
/// 展示前剥掉标记（不可打印字符，不该晃到用户眼前）。
fn auth_failure(provider: &str, err: String) -> LlmError {
    let transient = err.contains(crate::net::TRANSIENT_TAG);
    let clean = err.replace(crate::net::TRANSIENT_TAG, "");
    if transient {
        LlmError::Api {
            status: 503,
            body: format!("{provider} 订阅鉴权遇网络问题（自动重试中）: {clean}"),
        }
    } else {
        LlmError::Api {
            status: 401,
            body: format!("{provider} 订阅鉴权失败: {clean}"),
        }
    }
}

/// 把 SSE 流内的 `event: error` 负载映射为 [`LlmError`]。HTTP 响应头已是 200、但上游在流中途
/// 报错（overloaded / 限流 / 网关抖动等）时走这里——否则该错误会被聚合器的 `_ => {}` 忽略，
/// 最终被当成「成功但空」的响应静默返回，让 agent 静默停。映射到合适的状态码以复用既有重试
/// 策略（429/5xx 可重试），可识别的瞬时错误优先映到可重试码。Anthropic 形态：
/// `{"type":"error","error":{"type":"overloaded_error","message":"…"}}`。
fn sse_error_to_llm_error(data: &str) -> LlmError {
    let v: serde_json::Value = serde_json::from_str(data).unwrap_or(json!({}));
    let err = v.get("error").unwrap_or(&v);
    let etype = err
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let msg = err
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(data);
    let status = match etype {
        "overloaded_error" => 529,
        "rate_limit_error" => 429,
        "api_error" | "timeout_error" => 503,
        "authentication_error" => 401,
        "permission_error" => 403,
        "invalid_request_error" => 400,
        // 未知 / 缺省：流内错误绝大多数是瞬时服务端抖动，按可重试的 5xx 处理。
        _ => 500,
    };
    LlmError::Api {
        status,
        body: format!("流内错误 {etype}: {msg}"),
    }
}

/// 流式「空闲」超时秒数：默认 120s，可用 env `WC_STREAM_IDLE_TIMEOUT_SECS` 覆盖。
/// 注意是**相邻数据块之间**的间隔上限，不是整次请求的总时长——正常思考/输出会不断刷新，
/// 只有连接真正卡死（如代理静默断流）才会触发。
fn stream_idle_timeout() -> std::time::Duration {
    let secs = std::env::var("WC_STREAM_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(120);
    std::time::Duration::from_secs(secs)
}

/// 瞬时错误（5xx/传输/超时）第 `attempt` 次失败后的退避：指数 500ms·2^(attempt-1)
/// → 500ms、1s、2s……几秒内快速重试，扛住网关抖动。
fn transient_backoff(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_millis(500u64 * 2u64.pow(attempt.saturating_sub(1)))
}

/// 上游 5xx（过载 / 内部错误）第 `attempt` 次失败后的退避：秒级指数 2s·2^(attempt-1)，封顶 20s
/// → 2s、4s、8s、16s、20s……配合默认 5 次可覆盖约 30 秒的容量抖动。
/// 不复用 [`transient_backoff`] 的理由见 [`LlmError::is_server_error`]。
fn server_error_backoff(attempt: u32) -> std::time::Duration {
    const CAP_SECS: u64 = 20;
    // 用 checked_shl 而非 pow：attempt 由失败次数驱动，理论上可以很大，别在这儿 panic。
    let secs = 1u64
        .checked_shl(attempt.saturating_sub(1))
        .map_or(CAP_SECS, |m| m.saturating_mul(2));
    std::time::Duration::from_secs(secs.min(CAP_SECS))
}

/// 上游 5xx **快退避**的次数：默认 5 次（2+4+8+16+20 ≈ 50 秒），可用 env
/// `WC_SERVER_RETRY_ATTEMPTS` 覆盖。用尽后不放弃，转入 [`SERVER_ERROR_LONG_WAIT`] 的长间隔重试。
fn server_retry_attempts() -> u32 {
    std::env::var("WC_SERVER_RETRY_ATTEMPTS")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(5)
}

/// 上游 5xx 快退避用尽后的固定长间隔。
///
/// 定这个数的依据是实测而非拍脑袋：2026-07-30 那次 Anthropic 过载从 13:53 断续到 17:16，
/// 单次容量窗口是**分钟级**——50 秒的快退避打完就放弃，用户的观感仍是「一撞就报错」。
const SERVER_ERROR_LONG_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

/// 长间隔重试的次数上限：5 次 × 60s = 5 分钟；连同快退避累计约 5 分 50 秒。
/// 不做成限流那样的「等一整天」：5xx 里混着真·故障（反代挂了、base_url 配错），
/// 无限等下去就成了假死；到顶把错误透出来，让用户能去查。等待期间点「停止」可随时中断。
const MAX_SERVER_ERROR_LONG_RETRIES: u32 = 5;

/// 第 `count` 次上游 5xx（1 基）该怎么办。与 [`rate_limit_action`] 同构：先快退避扛住容量抖动，
/// 再转固定长间隔等容量恢复，到顶放弃。纯函数，便于单测。
enum ServerErrorAction {
    Fast { nth: u32, wait: std::time::Duration },
    Long { nth: u32, wait: std::time::Duration },
    GiveUp,
}

fn server_error_action(count: u32) -> ServerErrorAction {
    let fast = server_retry_attempts();
    if count <= fast {
        ServerErrorAction::Fast {
            nth: count,
            wait: server_error_backoff(count),
        }
    } else {
        let nth = count - fast;
        if nth > MAX_SERVER_ERROR_LONG_RETRIES {
            ServerErrorAction::GiveUp
        } else {
            ServerErrorAction::Long {
                nth,
                wait: SERVER_ERROR_LONG_WAIT,
            }
        }
    }
}

/// 限流（429）自动重试间隔，默认 600s（10 分钟），可用 env `WC_RATE_LIMIT_RETRY_SECS` 覆盖。
fn rate_limit_retry_interval() -> std::time::Duration {
    parse_rate_limit_interval(std::env::var("WC_RATE_LIMIT_RETRY_SECS").ok().as_deref())
}

/// 解析限流重试间隔（去空白、要求正整数秒）；缺省 / 非法 / 0 一律回落 600s（10 分钟），
/// 避免误配把「隔 10 分钟再试」退化成忙等空转。
fn parse_rate_limit_interval(raw: Option<&str>) -> std::time::Duration {
    let secs = raw
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(600);
    std::time::Duration::from_secs(secs)
}

/// 429 先按指数退避「快重试」的次数：短时突发限流（并发/每分钟限速等）也是 429，
/// 通常几十秒内就恢复，别一上来就当成账号额度上限傻等 10 分钟。
const RATE_LIMIT_FAST_ATTEMPTS: u32 = 3;

/// 快重试的**起步等待**。默认 500ms 沿用瞬时错误那套（Claude/GPT 的瞬时 429 大多几秒内就好）。
const FAST_BASE_DEFAULT: std::time::Duration = std::time::Duration::from_millis(500);

/// Gemini Code Assist 免费档的起步等待：**15 秒**。
///
/// 实测（2026-07-21，个人 Google 账号 + cloudcode-pa）：pro 系模型的节流窗口约 15–20 秒，
/// 429 后等 0.5s/1s/2s/5s/10s 重试**全部仍是 429**，等到约 18s 才恢复；且该 429 响应里
/// **既没有 retryDelay 也没有 quotaId**，服务端不给任何提示，只能按实测值退避。
/// 用默认的 500ms 起步意味着三次快重试全部落在窗口内必挂，3.5 秒后就误判成「账号额度上限」
/// 罚等 10 分钟——这正是「过不两句就限流」的由来。
const FAST_BASE_GEMINI: std::time::Duration = std::time::Duration::from_secs(15);

/// 快重试的单次等待上限（指数增长的封顶）。
const FAST_CAP: std::time::Duration = std::time::Duration::from_secs(60);

/// 某个 provider 的 429 退避参数。各家限流粒度差异很大，不该一刀切。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateLimitPolicy {
    /// 首次快重试的等待时长，之后每次翻倍（封顶 [`FAST_CAP`]）。
    pub fast_base: std::time::Duration,
    /// 判定为「账号额度上限」前，先做几次快重试。
    pub fast_attempts: u32,
}

impl RateLimitPolicy {
    /// 按 provider 选退避参数：Gemini 订阅用实测出来的 15s 起步，其余保持原有的 500ms。
    pub fn for_provider(cfg: &ProviderConfig) -> Self {
        if cfg.use_gemini_oauth {
            Self {
                fast_base: FAST_BASE_GEMINI,
                fast_attempts: RATE_LIMIT_FAST_ATTEMPTS,
            }
        } else {
            Self {
                fast_base: FAST_BASE_DEFAULT,
                fast_attempts: RATE_LIMIT_FAST_ATTEMPTS,
            }
        }
    }

    /// 第 `nth` 次快重试（1 基）等多久：`fast_base * 2^(nth-1)`，封顶 [`FAST_CAP`]。
    fn fast_wait(&self, nth: u32) -> std::time::Duration {
        let mult = 2u32.saturating_pow(nth.saturating_sub(1));
        self.fast_base.saturating_mul(mult).min(FAST_CAP)
    }
}

/// 给等待时长加 ±20% 抖动，避免多个会话/多台机器同时醒来又一起撞限流。
/// 抖动只在真正 sleep 时施加，不进纯函数 [`rate_limit_action`]（那样就没法测了）。
fn jitter(d: std::time::Duration) -> std::time::Duration {
    use rand::Rng;
    let f = rand::thread_rng().gen_range(0.8f64..1.2f64);
    std::time::Duration::from_secs_f64(d.as_secs_f64() * f)
}
/// 快重试仍持续 429、判定为账号额度上限后，固定长间隔自动重试的上限：默认间隔 10 分钟，
/// 144 次 ≈ 等待一整天额度恢复；到顶才放弃。期间用户点「停止」会 abort 任务、连带取消 sleep。
const MAX_RATE_LIMIT_RETRIES: u32 = 144;

/// 第 `count` 次 429（1 基）该如何处理。前 [`RATE_LIMIT_FAST_ATTEMPTS`] 次当作短时限流走快退避；
/// 之后升级为账号额度上限、固定长间隔 `long_wait` 重试，长等待计数重新起算；超过
/// [`MAX_RATE_LIMIT_RETRIES`] 次放弃。纯函数，便于单测。
enum RateLimitAction {
    /// 短时限流：`nth` 是第几次快重试（1 基），等 `wait`（快退避）。
    Fast { nth: u32, wait: std::time::Duration },
    /// 账号额度上限：`nth` 是第几次长等待（1 基），等 `wait`（固定长间隔）。
    Long { nth: u32, wait: std::time::Duration },
    /// 长等待到顶，放弃。
    GiveUp,
}

fn rate_limit_action(
    count: u32,
    long_wait: std::time::Duration,
    policy: RateLimitPolicy,
) -> RateLimitAction {
    if count <= policy.fast_attempts {
        RateLimitAction::Fast {
            nth: count,
            wait: policy.fast_wait(count),
        }
    } else {
        let nth = count - policy.fast_attempts;
        if nth > MAX_RATE_LIMIT_RETRIES {
            RateLimitAction::GiveUp
        } else {
            RateLimitAction::Long {
                nth,
                wait: long_wait,
            }
        }
    }
}

/// Gemini 订阅：pro 系模型撞持续限流时，降级到同代 flash 的对照表。
///
/// 实测（2026-07-21）：pro 被限流的**同一时刻** flash 仍返回 200——两者配额是分开的，
/// 所以降级是真能救场的，不是聊胜于无。`gemini-3.5-flash` 该账号是 404，故不入表。
/// 返回 `None` 表示没有合适的降级目标（已经是 flash，或不认识的模型名）。
fn gemini_flash_fallback(model: &str) -> Option<&'static str> {
    // 已经是 flash / flash-lite 就别再降了，否则会绕圈。
    if model.contains("flash") {
        return None;
    }
    Some(match model {
        "gemini-2.5-pro" => "gemini-2.5-flash",
        "gemini-3-pro-preview" | "gemini-3.1-pro-preview" => "gemini-3-flash-preview",
        // 不认识的 pro 系模型：退到实测可用的 GA flash，保守但一定能跑。
        m if m.contains("pro") => "gemini-2.5-flash",
        _ => return None,
    })
}

/// 限流等待期间的倒计时推送间隔：每这么久刷新一次「还有多久重试」。
const COUNTDOWN_TICK: std::time::Duration = std::time::Duration::from_secs(15);

/// 等待 `wait`，期间每 [`COUNTDOWN_TICK`] 给调用方推一次 [`StreamUpdate::Retrying`]（带剩余秒数）。
/// 这样 UI 能显示「账号限流 · 还有 9 分 30 秒重试 · 第 3/144 次」，而不是一直干转 thinking。
/// 短等待（快退避）只会推一次就睡完，开销可忽略。
///
/// **所有**自动重试都必须经由这里等待，别直接 `sleep`：静默的重试和不重试，在用户眼里没有区别。
async fn wait_with_countdown<F>(
    on: &mut F,
    attempt: u32,
    max: u32,
    wait: std::time::Duration,
    kind: RetryKind,
) where
    F: FnMut(StreamUpdate),
{
    // ⚠️ 必须用 tokio::time::Instant（不是 std 的）：剩余时间要和 tokio::time::sleep 用同一个时钟，
    // 否则在暂停/快进时钟下（测试）真实时钟不走 → left 永不减 → 死循环狂推。生产下二者等价。
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            break;
        }
        on(StreamUpdate::Retrying {
            attempt,
            max,
            // 向上取整到 1s，避免最后一刻显示「还有 0 秒」。
            wait_secs: left.as_secs().max(1),
            kind,
        });
        tokio::time::sleep(left.min(COUNTDOWN_TICK)).await;
    }
}

/// 把所有消息里的图片就地剥掉，并在文本里留一行占位（让模型仍知道「这里本有图」）。
/// 返回被剥离的图片总数（0 表示无图、无需改动）。
/// 当目标模型不支持图片输入时调用——避免多模态内容被上游打成 4xx（如 deepseek-v4）。
fn redact_images(messages: &mut [ChatMessage]) -> usize {
    let mut total = 0usize;
    for m in messages.iter_mut() {
        let n = m.images.len();
        if n == 0 {
            continue;
        }
        total += n;
        m.images.clear();
        let note = format!("[{n} 张图片已省略：当前模型不支持图片输入]");
        m.content = Some(match m.content.take() {
            Some(c) if !c.is_empty() => format!("{c}\n{note}"),
            _ => note,
        });
    }
    total
}

/// 历史是否「未处于规范配对形态」，需要送上游前修复。规范形态 = 每条带 tool_calls 的 assistant
/// 之后，紧跟一个 tool 块且其 id 集合恰好等于该 assistant 的 tool_call 集合；任何 tool 结果都不
/// 游离在这种块之外。只读预检，便于在确需修复时才克隆请求。
///
/// 命中四类异常（都会被严格校验的上游以 400 拒绝**整段历史**、把会话「焊死」）：
///   1) 悬空 tool_calls：assistant 的某个 tool_use 没有结果（进程退出 / 用户点停 / 工具卡死）。
///   2) 乱序 / 不相邻：tool 结果没紧跟其 tool_use 回合（如两个工具回合间被别的 assistant 隔开、
///      长命令结果迟到落在后面）——即 Anthropic 报的「tool_use_id … must have a corresponding
///      tool_use block in the previous message」。
///   3) 孤立结果：tool 结果的 tool_use 根本不存在。
///   4) 重复 id：同一 tool_use id 跨回合复用（文本式调用恢复每轮都叫 text_call_0），按 id 配对会
///      错乱，须 uniquify。
fn needs_history_repair(messages: &[ChatMessage]) -> bool {
    use std::collections::HashSet;
    let mut seen_ids: HashSet<&str> = HashSet::new();
    let mut i = 0;
    while i < messages.len() {
        match messages[i].role {
            // tool 结果只应作为某 assistant 块的一部分被消费；在顶层遇到即不相邻 / 孤立。
            Role::Tool => return true,
            Role::Assistant if !messages[i].tool_calls.is_empty() => {
                // 全局唯一性：任一 tool_use id（跨回合或本回合内）重复 → 需修复。
                for tc in &messages[i].tool_calls {
                    if !seen_ids.insert(tc.id.as_str()) {
                        return true;
                    }
                }
                let owner: HashSet<&str> = messages[i]
                    .tool_calls
                    .iter()
                    .map(|t| t.id.as_str())
                    .collect();
                i += 1;
                let mut block: HashSet<&str> = HashSet::new();
                while i < messages.len() && messages[i].role == Role::Tool {
                    if let Some(id) = messages[i].tool_call_id.as_deref() {
                        block.insert(id);
                    }
                    i += 1;
                }
                if block != owner {
                    return true;
                }
            }
            _ => i += 1,
        }
    }
    false
}

/// 一次历史修复的统计（用于日志）。
#[derive(Debug, Default, PartialEq)]
struct HistoryRepair {
    /// 为缺失结果补的占位条数。
    synthesized: usize,
    /// 丢弃的孤立 / 重复结果条数。
    dropped: usize,
}

/// 把历史规范化成上游可接受的形态：每条带 tool_calls 的 assistant 之后，**按其 id 顺序**紧跟
/// 对应的 tool 结果（按 id FIFO 配对，从历史任意位置取回；缺失则补占位）。同一 id 跨回合复用时
/// 给重复者换上唯一 id（连带其结果），保证全历史 tool_use id 不重复。游离 / 孤立 / 多余的 tool
/// 结果（其 tool_use 不存在或已配尽）直接丢弃。
///
/// 这能自愈被中断 / 乱序 / 长命令结果迟到 / id 撞车等导致的损坏历史——否则严格校验的上游
/// （Anthropic、OpenAI、DeepSeek…）会以 400 拒绝整段历史，且每次重发同样失败，把会话「焊死」。
/// 三条线缆共用。
fn repair_tool_call_pairs(messages: &mut Vec<ChatMessage>) -> HistoryRepair {
    use std::collections::{HashMap, HashSet, VecDeque};
    // 1) 按 id 收录所有 tool 结果为 FIFO 队列（同 id 多结果按出现序保留），并统计总数。
    let mut results: HashMap<String, VecDeque<ChatMessage>> = HashMap::new();
    let mut total_results = 0usize;
    for m in messages.iter() {
        if m.role == Role::Tool {
            total_results += 1;
            if let Some(id) = &m.tool_call_id {
                results.entry(id.clone()).or_default().push_back(m.clone());
            }
        }
    }
    // 2) 逐回合重建：每个 tool_use 按其 id 取回下一条结果（缺失补占位）；id 跨回合重复者换唯一 id。
    let src = std::mem::take(messages);
    let mut out: Vec<ChatMessage> = Vec::with_capacity(src.len());
    let mut seen: HashSet<String> = HashSet::new();
    let mut uniq = 0u64;
    let mut synthesized = 0usize;
    let mut consumed = 0usize;
    for mut msg in src {
        match msg.role {
            Role::Tool => {} // 跳过；由 assistant 回合按 id 重新带出，孤立 / 多余的自然丢弃。
            Role::Assistant if !msg.tool_calls.is_empty() => {
                let mut paired: Vec<ChatMessage> = Vec::with_capacity(msg.tool_calls.len());
                for tc in msg.tool_calls.iter_mut() {
                    let mut result = match results.get_mut(&tc.id).and_then(|q| q.pop_front()) {
                        Some(r) => {
                            consumed += 1;
                            r
                        }
                        None => {
                            synthesized += 1;
                            ChatMessage::tool_result(
                                tc.id.clone(),
                                "(无结果：上一轮工具调用被中断或未完成)",
                            )
                        }
                    };
                    // 全局唯一化：同一 tool_use id 跨回合重复会让上游配对错乱，给重复者换新 id。
                    if !seen.insert(tc.id.clone()) {
                        uniq += 1;
                        let new_id = format!("call_{uniq}_{}", tc.id);
                        tc.id = new_id.clone();
                        result.tool_call_id = Some(new_id.clone());
                        seen.insert(new_id);
                    }
                    paired.push(result);
                }
                out.push(msg);
                out.extend(paired);
            }
            _ => out.push(msg),
        }
    }
    // 被某个 tool_use 真正配走的结果数；其余（孤立 + 多余）即被丢弃。
    let dropped = total_results.saturating_sub(consumed);
    *messages = out;
    HistoryRepair {
        synthesized,
        dropped,
    }
}

/// 流式 LLM 客户端（包一个复用的 reqwest::Client）。
#[derive(Clone)]
pub struct LlmClient {
    http: reqwest::Client,
}

impl Default for LlmClient {
    fn default() -> Self {
        Self::new()
    }
}

enum Agg {
    OpenAi(openai::OpenAiAggregator),
    Anthropic(anthropic::AnthropicAggregator),
    OpenAiResponses(responses::ResponsesAggregator),
    Gemini(gemini::GeminiAggregator),
}

impl Agg {
    fn progress(&self) -> (u64, u64) {
        match self {
            Agg::OpenAi(a) => a.progress(),
            Agg::Anthropic(a) => a.progress(),
            Agg::OpenAiResponses(a) => a.progress(),
            Agg::Gemini(a) => a.progress(),
        }
    }
    fn into_response(self) -> LlmResponse {
        match self {
            Agg::OpenAi(a) => openai::parse_response(&a.into_value()),
            Agg::Anthropic(a) => anthropic::parse_response(&a.into_value()),
            Agg::OpenAiResponses(a) => a.into_response(),
            Agg::Gemini(a) => a.into_response(),
        }
    }
}

impl LlmClient {
    pub fn new() -> Self {
        // 经 net::async_builder 应用代理（设置了代理则 LLM 流式请求也走代理）。
        LlmClient {
            http: crate::net::async_builder()
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    /// 用自带的 reqwest::Client 构造（注入点，主要供测试绕开代理/自定义超时）。
    pub fn with_http(http: reqwest::Client) -> Self {
        LlmClient { http }
    }

    /// 发起一次流式补全。`on(StreamUpdate)` 在流式过程中回调：
    /// 文本增量（Text）用于逐字渲染，token 用量（Usage）用于进度显示。返回聚合后的最终结果。
    /// 流式补全，带自动重试。仅当本次尝试**尚未流出任何内容**时才重试（避免重复输出）。
    /// 三条重试路径，按时间尺度分开——用错尺度就等于没重试：
    ///   - 传输层抖动（连接/解码失败、空闲超时）：毫秒级指数退避 0.5s/1s，最多 3 次。
    ///   - 上游 5xx（529 overloaded / 503 api_error 等）：秒级指数退避 2s/4s/8s/16s/20s，
    ///     默认最多 5 次（`WC_SERVER_RETRY_ATTEMPTS` 可调），扛住几十秒级的容量事件。
    ///   - 限流（429）：固定长间隔（默认 10 分钟，`WC_RATE_LIMIT_RETRY_SECS` 可调）至多 144 次，
    ///     等账号额度窗口恢复。
    pub async fn complete<F>(
        &self,
        cfg: &ProviderConfig,
        req: &LlmRequest,
        mut on: F,
    ) -> Result<LlmResponse, LlmError>
    where
        F: FnMut(StreamUpdate),
    {
        // 发送前对历史做两处单点修复（三条线缆 OpenAI/Anthropic/Responses 一并覆盖），
        // 只在确需改动时才克隆请求：
        //   1) 模型不支持图片 → 剥离全部图片，替换为文字占位（否则非视觉模型收到 image 块报 4xx）。
        //   2) 悬空的 tool_calls → 补齐缺失的 tool 结果（否则严格校验的上游会以 400 拒绝整段历史，
        //      且每次重发同样失败，把会话焊死）。
        let needs_redact = !cfg.vision && req.messages.iter().any(|m| !m.images.is_empty());
        let needs_repair = needs_history_repair(&req.messages);
        let owned_req;
        let req = if needs_redact || needs_repair {
            let mut r = req.clone();
            if needs_redact {
                let n = redact_images(&mut r.messages);
                crate::seprintln!(
                    "[vision] 模型 {} 不支持图片输入，已剥离 {n} 张图片（降级为纯文本）",
                    r.model
                );
            }
            if needs_repair {
                let rep = repair_tool_call_pairs(&mut r.messages);
                crate::seprintln!(
                    "[history] 历史自愈：补齐 {} 条缺失结果、丢弃 {} 条孤立结果、重排乱序工具回合（避免上游 400）",
                    rep.synthesized, rep.dropped
                );
            }
            owned_req = r;
            &owned_req
        } else {
            req
        };

        // 瞬时错误（5xx/传输/超时）快速重试的上限：几秒内退避几次，扛网关抖动。
        const MAX_TRANSIENT_ATTEMPTS: u32 = 3;
        let rate_limit_wait = rate_limit_retry_interval();
        let policy = RateLimitPolicy::for_provider(cfg);
        let mut transient = 0u32;
        let mut rate_limited = 0u32;
        // 上游 5xx 单独计数：别拿网关抖动的次数预算去算服务端过载的账（两者尺度差一个数量级）。
        let mut server_err = 0u32;
        // Gemini 订阅撞持续限流时把本次调用降级到 flash。只改这一次调用的模型，
        // 不写回配置——下一轮仍从 pro 起步（用户选的是「仅限本轮」）。
        let mut downgraded: Option<LlmRequest> = None;
        loop {
            let req = downgraded.as_ref().unwrap_or(req);
            let mut streamed_any = false;
            let result = self
                .complete_once(cfg, req, |u| {
                    if matches!(u, StreamUpdate::Text(_)) {
                        streamed_any = true;
                    }
                    on(u);
                })
                .await;
            let e = match result {
                Ok(r) => return Ok(r),
                Err(e) => e,
            };
            // Gemini 订阅特例：带 CLI UA 时 Gemini 3 系会被回 404（UA 与模型可用性互斥）。
            // 首次撞上就记下该模型、去掉 UA 重试一次；之后对它一律不带 UA，不再重复踩。
            if cfg.use_gemini_oauth
                && matches!(&e, LlmError::Api { status: 404, .. })
                && !streamed_any
                && oauth_gemini::should_send_user_agent(&req.model)
                && oauth_gemini::deny_user_agent_for(&req.model)
            {
                crate::seprintln!(
                    "[gemini] {} 带 CLI User-Agent 被回 404，改为不带 UA 重试（该模型此后不再带）",
                    req.model
                );
                continue;
            }
            // 已流出内容（重试会重复）、或不可重试错误 → 直接返回。
            if streamed_any || !e.is_retryable() {
                return Err(e);
            }
            // 限流单独一条路：先快退避几次扛短时突发限流，仍持续 429 才升级为账号额度上限、
            // 固定长间隔（默认 10 分钟）多次重试等窗口恢复；不占用瞬时错误的次数预算。
            if e.is_rate_limited() {
                rate_limited += 1;
                let (nth, max, wait, long) =
                    match rate_limit_action(rate_limited, rate_limit_wait, policy) {
                        RateLimitAction::Fast { nth, wait } => {
                            (nth, policy.fast_attempts, wait, false)
                        }
                        RateLimitAction::Long { nth, wait } => {
                            // 快退避用尽仍 429：Gemini 订阅先降级到 flash 再试一把，
                            // 而不是直接罚等 10 分钟——实测此刻 flash 的配额是好的。
                            if nth == 1 && cfg.use_gemini_oauth && downgraded.is_none() {
                                if let Some(to) = gemini_flash_fallback(&req.model) {
                                    crate::seprintln!(
                                    "[rate-limit] {} 持续限流，本次降级到 {to}（下一轮仍用 {}）",
                                    req.model, req.model
                                );
                                    let mut r = req.clone();
                                    r.model = to.to_string();
                                    downgraded = Some(r);
                                    // 计数归零：flash 有自己的配额，别拿 pro 的失败次数算它的账。
                                    rate_limited = 0;
                                    continue;
                                }
                            }
                            (nth, MAX_RATE_LIMIT_RETRIES, wait, true)
                        }
                        RateLimitAction::GiveUp => return Err(e),
                    };
                crate::seprintln!(
                    "[rate-limit] {}，{}s 后自动重试（第 {nth}/{max} 次）：{e}",
                    if long {
                        "触发账号限流"
                    } else {
                        "疑似短时限流"
                    },
                    wait.as_secs()
                );
                // 等待期间持续把倒计时推给调用方 —— 否则 UI 只会干转 thinking，用户不知道在等什么。
                // 加抖动：多个会话同时醒来会又一起撞限流。
                let kind = if long {
                    RetryKind::RateLimitLong
                } else {
                    RetryKind::RateLimitFast
                };
                wait_with_countdown(&mut on, nth, max, jitter(wait), kind).await;
            } else if let Some(status) = e.server_status() {
                // 上游过载/内部错误：秒级退避 + 分钟级长等待，别用毫秒级那套白跑三下就把 529 甩给用户。
                server_err += 1;
                let (nth, max, wait) = match server_error_action(server_err) {
                    ServerErrorAction::Fast { nth, wait } => (nth, server_retry_attempts(), wait),
                    ServerErrorAction::Long { nth, wait } => {
                        (nth, MAX_SERVER_ERROR_LONG_RETRIES, wait)
                    }
                    ServerErrorAction::GiveUp => return Err(e),
                };
                crate::seprintln!(
                    "[server-5xx] 上游服务端故障（{status}），{}s 后自动重试（第 {nth}/{max} 次）：{e}",
                    wait.as_secs()
                );
                // 与限流同路：把倒计时推成可见状态行。只写 stderr 的话桌面端根本收不到，
                // 用户看到的就是「转很久然后报错」——这正是「529 还不会自动重试」的由来。
                wait_with_countdown(
                    &mut on,
                    nth,
                    max,
                    jitter(wait),
                    RetryKind::ServerError(status),
                )
                .await;
            } else {
                transient += 1;
                if transient >= MAX_TRANSIENT_ATTEMPTS {
                    return Err(e);
                }
                tokio::time::sleep(transient_backoff(transient)).await;
            }
        }
    }

    async fn complete_once<F>(
        &self,
        cfg: &ProviderConfig,
        req: &LlmRequest,
        mut on: F,
    ) -> Result<LlmResponse, LlmError>
    where
        F: FnMut(StreamUpdate),
    {
        let base = cfg.base_url.trim_end_matches('/');
        // Claude 订阅：请求前取一个新鲜 access token（按需自动刷新）。
        let oauth_token: Option<String> = if cfg.use_claude_oauth {
            Some(
                oauth::valid_access_token()
                    .await
                    .map_err(|e| auth_failure("Claude", e))?,
            )
        } else {
            None
        };
        // ChatGPT/Codex 订阅：请求前取 access token + account_id（按需刷新）。
        let codex: Option<(String, Option<String>)> = if cfg.use_openai_codex {
            Some(
                oauth_openai::valid_access()
                    .await
                    .map_err(|e| auth_failure("ChatGPT", e))?,
            )
        } else {
            None
        };
        // xAI Grok 订阅：请求前取 access token（按需刷新），OpenAi 线缆的 Bearer 用它代替 api_key。
        let xai_token: Option<String> = if cfg.use_xai_grok {
            Some(
                oauth_xai::valid_access_token()
                    .await
                    .map_err(|e| auth_failure("Grok", e))?,
            )
        } else {
            None
        };
        // Gemini 订阅：请求前取 access token + project_id（按需刷新 / 惰性 onboarding）。
        let gemini_auth: Option<(String, String)> = if cfg.use_gemini_oauth {
            Some(
                oauth_gemini::valid_access()
                    .await
                    .map_err(|e| auth_failure("Gemini", e))?,
            )
        } else {
            None
        };
        let (builder, mut agg) = match cfg.format {
            WireFormat::OpenAi => {
                let mut body = openai::build_request_body(req, cfg.thinking);
                body["stream"] = json!(true);
                body["stream_options"] = json!({ "include_usage": true });
                let b = self
                    .http
                    .post(format!("{base}{}", openai::PATH))
                    .bearer_auth(xai_token.as_deref().unwrap_or(&cfg.api_key))
                    .json(&body);
                (b, Agg::OpenAi(openai::OpenAiAggregator::new()))
            }
            WireFormat::Anthropic => {
                // OAuth 推理要求 system 以 Claude Code 身份句开头，否则 API 拒绝。
                let spoofed;
                let req = if oauth_token.is_some() {
                    let mut r = req.clone();
                    r.messages
                        .insert(0, ChatMessage::system(oauth::CLAUDE_CODE_SPOOF));
                    spoofed = r;
                    &spoofed
                } else {
                    req
                };
                let mut body = anthropic::build_request_body(req);
                body["stream"] = json!(true);
                let b = self
                    .http
                    .post(format!("{base}{}", anthropic::PATH))
                    .header("anthropic-version", anthropic::VERSION);
                let b = match &oauth_token {
                    Some(tok) => b
                        .header("authorization", format!("Bearer {tok}"))
                        .header("anthropic-beta", oauth::OAUTH_BETA_HEADER)
                        // OAuth 推理同样过 WAF：必须带 claude-code/* 的 User-Agent。
                        .header("User-Agent", oauth::CLAUDE_CODE_USER_AGENT)
                        // 官方 CLI 会带；实测接受。UA 保持 claude-code/*（已验证能过 WAF），
                        // 不改成 claude-cli/*：没有证据支持，改了反而可能踩新坑。
                        .header("x-app", "cli"),
                    None => b.header("x-api-key", &cfg.api_key),
                };
                (
                    b.json(&body),
                    Agg::Anthropic(anthropic::AnthropicAggregator::new()),
                )
            }
            WireFormat::OpenAiResponses => {
                let mut body = responses::build_request_body(req, codex.is_some());
                body["stream"] = json!(true);
                let b = self.http.post(format!("{base}{}", responses::PATH));
                let b = match &codex {
                    // 订阅：Bearer + ChatGPT-Account-Id + originator。
                    Some((tok, acct)) => {
                        let mut bb = b
                            .header("authorization", format!("Bearer {tok}"))
                            .header("originator", oauth_openai::ORIGINATOR);
                        if let Some(a) = acct {
                            bb = bb.header("ChatGPT-Account-Id", a);
                        }
                        bb
                    }
                    // 无订阅：当作标准 Responses API，用 api_key。
                    None => b.bearer_auth(&cfg.api_key),
                };
                (
                    b.json(&body),
                    Agg::OpenAiResponses(responses::ResponsesAggregator::new()),
                )
            }
            WireFormat::Gemini => {
                // Gemini 订阅：Code Assist 信封 `{model, project, user_prompt_id, request}`，
                // Bearer 鉴权，流式由端点 `?alt=sse` 决定（不设 body.stream）。
                let (token, project) = gemini_auth.as_ref().ok_or_else(|| LlmError::Api {
                    status: 401,
                    body: "Gemini 线缆需订阅 OAuth，但未取得 access token".to_string(),
                })?;
                let prompt_id = oauth::random_token();
                let body = gemini::build_request_body(req, project, &prompt_id);
                let b = self
                    .http
                    .post(format!("{base}{}", gemini::STREAM_PATH))
                    .header("authorization", format!("Bearer {token}"))
                    .json(&body);
                // ⚠️ 少了这个头，cloudcode-pa 会按最紧的一档节流（实测 6 连发只过 2 次，
                // 带上则 6/6）——付费订阅的额度根本吃不到。详见 oauth_gemini::user_agent。
                // 但 UA 与模型可用性互斥：带 UA 时 Gemini 3 系一律 404，故按模型自愈开关决定。
                let b = if oauth_gemini::should_send_user_agent(&req.model) {
                    b.header("User-Agent", oauth_gemini::user_agent(&req.model))
                } else {
                    b
                };
                (b, Agg::Gemini(gemini::GeminiAggregator::new()))
            }
        };

        // 建连 + 拿到响应头也设空闲超时上限：服务器只完成 TCP 握手却迟迟不回头时不至于干等。
        let idle = stream_idle_timeout();
        let idle_secs = idle.as_secs();
        let resp = match tokio::time::timeout(idle, builder.send()).await {
            Ok(inner) => inner?,
            Err(_) => return Err(LlmError::Timeout(idle_secs)),
        };
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::Api {
                status: status.as_u16(),
                body,
            });
        }

        let mut stream = resp.bytes_stream();
        let mut pending: Vec<u8> = Vec::new();
        let mut current_event = String::from("message");
        let mut last_progress = (0u64, 0u64);

        // 逐块读取：每块之间设空闲超时。卡死（无数据）即报 Timeout（可重试），不再无限期挂起。
        'outer: loop {
            let item = match tokio::time::timeout(idle, stream.next()).await {
                Ok(Some(item)) => item?,  // 正常数据块（传输错误向上抛，仍可重试）
                Ok(None) => break 'outer, // 流正常结束
                Err(_) => return Err(LlmError::Timeout(idle_secs)), // 空闲超时：连接卡死
            };
            pending.extend_from_slice(&item);

            while let Some(pos) = pending.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = pending.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line_bytes[..line_bytes.len() - 1]);
                let line = line.trim_end_matches('\r');

                if line.is_empty() {
                    current_event = String::from("message");
                    continue;
                }
                if let Some(ev) = line.strip_prefix("event:") {
                    current_event = ev.trim().to_string();
                    continue;
                }
                if let Some(data) = line.strip_prefix("data:") {
                    let data = data.trim_start();
                    // 流内错误事件（HTTP 已 200、但上游中途报错）：聚合器会忽略它，必须在此显式
                    // 还原成（多为可重试的）错误，否则会被当成「成功但空」的响应静默返回，让 agent 静默停。
                    if current_event == "error" {
                        return Err(sse_error_to_llm_error(data));
                    }
                    let (done, text) = match &mut agg {
                        Agg::OpenAi(a) => a.handle(data),
                        Agg::Anthropic(a) => (false, a.handle(&current_event, data)),
                        Agg::OpenAiResponses(a) => (false, a.handle(&current_event, data)),
                        Agg::Gemini(a) => (false, a.handle(data)),
                    };

                    if let Some(t) = text {
                        on(StreamUpdate::Text(t));
                    }
                    let p = agg.progress();
                    if p != last_progress {
                        last_progress = p;
                        on(StreamUpdate::Usage {
                            input: p.0,
                            output: p.1,
                        });
                    }
                    if done {
                        break 'outer;
                    }
                }
            }
        }

        Ok(agg.into_response())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_errors_retry_only_on_429_and_5xx() {
        let mk = |s| LlmError::Api {
            status: s,
            body: String::new(),
        };
        assert!(mk(429).is_retryable());
        assert!(mk(500).is_retryable());
        assert!(mk(503).is_retryable());
        // 4xx（除 429）不重试：请求本身有问题，重试也没用。
        assert!(!mk(400).is_retryable());
        assert!(!mk(401).is_retryable());
        assert!(!mk(404).is_retryable());
    }

    #[test]
    fn idle_timeout_is_retryable() {
        // 空闲超时（连接卡死）必须可重试——否则一次卡死就中断整个任务。
        assert!(LlmError::Timeout(120).is_retryable());
    }

    /// 回归（2026-07-30 用户实测撞到）：Anthropic 流内 `api_error`/`overloaded_error` 会被
    /// [`sse_error_to_llm_error`] 映成 503/529，必须走**秒级**退避那条路。若它被并进传输层的
    /// 毫秒级快退避，1.5 秒内三下打完就把 503 原样甩给用户，而容量事件动辄持续几分钟。
    /// 回归（2026-07-30 实测）：Anthropic 的过载不是「几十秒的抖动」，而是断续几小时的容量事件
    /// ——13:53 起 529/503 一直断续到 17:16。只退避到 50 秒就放弃，用户看到的仍是「一撞就报错」。
    /// 快退避用尽后必须还有一段固定长间隔的重试，累计覆盖到分钟级。
    #[test]
    fn server_error_retry_window_covers_minutes_not_seconds() {
        use std::time::Duration;
        let total: Duration = (1..=server_retry_attempts() + MAX_SERVER_ERROR_LONG_RETRIES)
            .map(|n| match server_error_action(n) {
                ServerErrorAction::Fast { wait, .. } | ServerErrorAction::Long { wait, .. } => wait,
                ServerErrorAction::GiveUp => Duration::ZERO,
            })
            .sum();
        assert!(
            total >= Duration::from_secs(300),
            "上游 5xx 的累计重试窗口应覆盖到分钟级，实为 {total:?}"
        );
    }

    #[test]
    fn upstream_5xx_is_separated_from_transport_jitter() {
        let mk = |s| LlmError::Api {
            status: s,
            body: String::new(),
        };
        for s in [500, 502, 503, 504, 529] {
            assert!(mk(s).is_server_error(), "{s} 应归为上游服务端故障");
        }
        // 流内错误映射后也必须落在这一类（这才是用户实际看到的那条）。
        let stream_err = sse_error_to_llm_error(
            r#"{"type":"error","error":{"type":"api_error","message":"x"}}"#,
        );
        assert!(stream_err.is_server_error());
        let overloaded = sse_error_to_llm_error(
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        );
        assert!(overloaded.is_server_error());
        // 传输层抖动 / 卡死 / 限流 / 4xx 都不该走这条路。
        assert!(!LlmError::Timeout(120).is_server_error());
        assert!(!mk(429).is_server_error());
        assert!(!mk(400).is_server_error());
    }

    #[test]
    fn server_error_backoff_is_seconds_scale_and_capped() {
        use std::time::Duration;
        assert_eq!(server_error_backoff(1), Duration::from_secs(2));
        assert_eq!(server_error_backoff(2), Duration::from_secs(4));
        assert_eq!(server_error_backoff(3), Duration::from_secs(8));
        assert_eq!(server_error_backoff(4), Duration::from_secs(16));
        // 封顶 20s：默认 5 次累计约 30 秒，够盖住常见的几十秒抖动。
        assert_eq!(server_error_backoff(5), Duration::from_secs(20));
        // 次数很大也不能 panic（移位溢出）——退避次数由失败次数驱动，不设上界。
        assert_eq!(server_error_backoff(99), Duration::from_secs(20));
        // 关键是**累计等待窗口**要比传输层那套大一个数量级，否则这次改动等于没做：
        // 传输层 3 次只等 0.5+1=1.5s，秒级这条默认 5 次等 2+4+8+16=30s。
        let sum =
            |f: fn(u32) -> Duration, attempts: u32| -> Duration { (1..attempts).map(f).sum() };
        assert!(
            sum(server_error_backoff, server_retry_attempts()) > sum(transient_backoff, 3) * 10,
            "上游 5xx 的累计退避窗口必须远大于传输层抖动那套"
        );
    }

    #[test]
    fn transient_auth_failure_is_retryable_permanent_is_not() {
        // 网络层瞬时失败（内层带 TRANSIENT_TAG，如反代抽风/超时）→ 可重试的 503，
        // 让 complete() 的瞬时重试兜住，偶发抖动对用户无感。
        let tagged = format!(
            "{}请求 token 端点失败: connection timed out",
            crate::net::TRANSIENT_TAG
        );
        let e = auth_failure("Gemini", tagged);
        assert!(e.is_retryable(), "带瞬时标记的鉴权失败应可重试");
        assert!(!e.is_rate_limited());
        if let LlmError::Api { status, body } = &e {
            assert_eq!(*status, 503);
            assert!(
                !body.contains(crate::net::TRANSIENT_TAG),
                "展示前必须剥掉控制字符标记: {body}"
            );
            assert!(
                body.contains("connection timed out"),
                "内层诊断应保留: {body}"
            );
        } else {
            panic!("应为 Api 错误");
        }

        // 真正的鉴权问题（未打标记，如缺 GCP 项目 / refresh 失效）→ 终态 401，重试无用。
        let perm = auth_failure(
            "Gemini",
            "本账号的 Code Assist 档位要求自带 GCP 项目".to_string(),
        );
        assert!(!perm.is_retryable(), "确定性鉴权错误不该重试");
        assert!(matches!(perm, LlmError::Api { status: 401, .. }));
    }

    #[test]
    fn only_429_counts_as_rate_limited() {
        // 429（账号/额度限流）单独归类：走「固定长间隔、多次自动重试」，等额度恢复。
        let rl = LlmError::Api {
            status: 429,
            body: String::new(),
        };
        assert!(rl.is_rate_limited());
        assert!(rl.is_retryable());
        // 5xx / 529 过载 / 传输 / 超时 都不是限流：仍走快速指数退避，不该傻等 10 分钟。
        assert!(!LlmError::Api {
            status: 503,
            body: String::new()
        }
        .is_rate_limited());
        assert!(!LlmError::Api {
            status: 529,
            body: String::new()
        }
        .is_rate_limited());
        assert!(!LlmError::Timeout(120).is_rate_limited());
    }

    #[test]
    fn transient_backoff_is_exponential() {
        // 瞬时错误（5xx/传输/超时）按指数退避：第 1 次失败等 500ms，第 2 次 1s。
        assert_eq!(transient_backoff(1), std::time::Duration::from_millis(500));
        assert_eq!(transient_backoff(2), std::time::Duration::from_millis(1000));
        assert_eq!(transient_backoff(3), std::time::Duration::from_millis(2000));
    }

    #[test]
    fn rate_limit_interval_defaults_to_ten_minutes() {
        use std::time::Duration;
        // 缺省：10 分钟（用户诉求——限流后每 10 分钟自动重试一次）。
        assert_eq!(parse_rate_limit_interval(None), Duration::from_secs(600));
        // 可用 env 覆盖间隔（去空白、正整数）。
        assert_eq!(
            parse_rate_limit_interval(Some("  900 ")),
            Duration::from_secs(900)
        );
        // 非法 / 0 → 回落默认，避免退化成忙等。
        assert_eq!(
            parse_rate_limit_interval(Some("0")),
            Duration::from_secs(600)
        );
        assert_eq!(
            parse_rate_limit_interval(Some("nope")),
            Duration::from_secs(600)
        );
    }

    /// 默认（非 Gemini）退避参数，供沿用旧断言的测试使用。
    fn default_policy() -> RateLimitPolicy {
        RateLimitPolicy {
            fast_base: FAST_BASE_DEFAULT,
            fast_attempts: RATE_LIMIT_FAST_ATTEMPTS,
        }
    }

    #[test]
    fn rate_limit_fast_retries_before_escalating_to_long_wait() {
        use std::time::Duration;
        let long = Duration::from_secs(600);
        let p = default_policy();
        // 前 fast_attempts 次 429 当作短时突发限流：0.5s / 1s / 2s 快退避，
        // 几秒内尽快恢复，不该一上来就傻等 10 分钟。
        assert!(matches!(
            rate_limit_action(1, long, p),
            RateLimitAction::Fast { nth: 1, wait } if wait == Duration::from_millis(500)
        ));
        assert!(matches!(
            rate_limit_action(2, long, p),
            RateLimitAction::Fast { nth: 2, wait } if wait == Duration::from_millis(1000)
        ));
        assert!(matches!(
            rate_limit_action(RATE_LIMIT_FAST_ATTEMPTS, long, p),
            RateLimitAction::Fast { .. }
        ));
        // 快重试仍持续 429 → 认定账号额度上限：转固定长间隔（默认 10 分钟）自动重试，
        // 计数从「长等待第 1 次」重新起算。
        assert!(matches!(
            rate_limit_action(RATE_LIMIT_FAST_ATTEMPTS + 1, long, p),
            RateLimitAction::Long { nth: 1, wait } if wait == long
        ));
        assert!(matches!(
            rate_limit_action(RATE_LIMIT_FAST_ATTEMPTS + 2, long, p),
            RateLimitAction::Long { nth: 2, wait } if wait == long
        ));
    }

    #[test]
    fn gemini_backoff_starts_far_above_the_throttle_window() {
        use std::time::Duration;
        // 实测 Gemini 的节流窗口约 15-20s：0.5s/1s/2s 的快重试全部落在窗口内必挂，
        // 3.5 秒后就误判成账号额度上限、罚等 10 分钟。回归：起步必须 ≥15s。
        let mut cfg = ProviderConfig::new("https://x", "", WireFormat::Gemini);
        cfg = cfg.with_gemini_oauth(true);
        let p = RateLimitPolicy::for_provider(&cfg);
        assert_eq!(p.fast_base, Duration::from_secs(15));
        assert_eq!(p.fast_wait(1), Duration::from_secs(15));
        assert_eq!(p.fast_wait(2), Duration::from_secs(30));
        assert_eq!(p.fast_wait(3), Duration::from_secs(60));
        // 封顶，别无限翻倍。
        assert_eq!(p.fast_wait(9), FAST_CAP);
        // 三次快重试累计 ≥105s，足够覆盖窗口；旧实现累计才 3.5s。
        let total: Duration = (1..=p.fast_attempts).map(|n| p.fast_wait(n)).sum();
        assert!(
            total >= Duration::from_secs(100),
            "累计快重试时长 {total:?} 太短"
        );

        // 其它厂商不受影响：Claude/GPT 的瞬时 429 仍走 0.5s 快重试。
        let other = ProviderConfig::new("https://y", "k", WireFormat::OpenAi);
        assert_eq!(
            RateLimitPolicy::for_provider(&other).fast_base,
            FAST_BASE_DEFAULT
        );
    }

    #[test]
    fn gemini_flash_fallback_targets_verified_models() {
        // 实测：pro 被限流的同一时刻 flash 仍 200，所以降级能救场。
        assert_eq!(
            gemini_flash_fallback("gemini-2.5-pro"),
            Some("gemini-2.5-flash")
        );
        assert_eq!(
            gemini_flash_fallback("gemini-3.1-pro-preview"),
            Some("gemini-3-flash-preview")
        );
        // 不认识的 pro 系 → 退到实测可用的 GA flash。
        assert_eq!(
            gemini_flash_fallback("gemini-9-pro-ultra"),
            Some("gemini-2.5-flash")
        );
        // 已经是 flash 就别再降，否则会绕圈。
        assert_eq!(gemini_flash_fallback("gemini-2.5-flash"), None);
        assert_eq!(gemini_flash_fallback("gemini-3.1-flash-lite"), None);
    }

    #[test]
    fn jitter_stays_within_twenty_percent() {
        use std::time::Duration;
        let base = Duration::from_secs(30);
        for _ in 0..50 {
            let j = jitter(base);
            assert!(
                j >= Duration::from_secs(24) && j <= Duration::from_secs(36),
                "抖动越界: {j:?}"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn countdown_pushes_remaining_time_during_rate_limit_wait() {
        // 回归：限流等待期间必须持续推 Retrying（带剩余秒数），否则 UI 只会干转 thinking。
        use std::time::Duration;
        let mut seen: Vec<StreamUpdate> = Vec::new();
        // 10 分钟长等待：每 15s 推一次 → 应推约 40 次，且剩余秒数递减。
        wait_with_countdown(
            &mut |u| seen.push(u),
            3,
            144,
            Duration::from_secs(600),
            RetryKind::RateLimitLong,
        )
        .await;
        assert!(
            seen.len() >= 39,
            "10 分钟内应按 15s 节奏推约 40 次，实为 {}",
            seen.len()
        );
        // 首条：剩余接近 600s、带上第几/共几次。
        match &seen[0] {
            StreamUpdate::Retrying {
                attempt,
                max,
                wait_secs,
                kind,
            } => {
                assert_eq!((*attempt, *max, *kind), (3, 144, RetryKind::RateLimitLong));
                assert!(*wait_secs > 590, "首条应显示接近 600 秒，实为 {wait_secs}");
            }
            other => panic!("应为 Retrying，实为 {other:?}"),
        }
        // 剩余秒数单调递减，且从不为 0（避免显示「还有 0 秒」）。
        let lefts: Vec<u64> = seen
            .iter()
            .map(|u| match u {
                StreamUpdate::Retrying { wait_secs, .. } => *wait_secs,
                _ => panic!("只应推 Retrying"),
            })
            .collect();
        assert!(
            lefts.windows(2).all(|w| w[0] > w[1]),
            "剩余秒数应递减: {lefts:?}"
        );
        assert!(lefts.iter().all(|&s| s >= 1), "不应出现 0 秒");
    }

    #[tokio::test(start_paused = true)]
    async fn countdown_pushes_once_for_short_fast_backoff() {
        // 快退避（0.5s）：推一次就睡完，别刷屏。
        use std::time::Duration;
        let mut seen = 0usize;
        wait_with_countdown(
            &mut |_| seen += 1,
            1,
            3,
            Duration::from_millis(500),
            RetryKind::RateLimitFast,
        )
        .await;
        assert_eq!(seen, 1);
    }

    /// 回归（用户报「529 还不会自动重试」）：其实一直在重试，只是 5xx 那条路只 `eprintln!` 到
    /// stderr，桌面端 GUI 收不到——静默重试与不重试在用户眼里没有区别。5xx 必须和限流同样把
    /// 倒计时推成可见状态行，并点明是上游过载而非本地配置问题。
    #[tokio::test(start_paused = true)]
    async fn server_error_wait_is_visible_and_names_the_upstream() {
        use std::time::Duration;
        let mut seen: Vec<StreamUpdate> = Vec::new();
        wait_with_countdown(
            &mut |u| seen.push(u),
            2,
            5,
            Duration::from_secs(60),
            RetryKind::ServerError(529),
        )
        .await;
        assert!(!seen.is_empty(), "5xx 等待期间必须推倒计时，不能静默 sleep");
        match &seen[0] {
            StreamUpdate::Retrying {
                attempt, max, kind, ..
            } => {
                assert_eq!((*attempt, *max), (2, 5));
                assert_eq!(*kind, RetryKind::ServerError(529));
                // 状态行得让用户一眼看出是上游的事，别去翻自己的配置。
                assert!(
                    kind.label().contains("529"),
                    "应点名状态码: {}",
                    kind.label()
                );
                assert!(
                    kind.label().contains("上游"),
                    "应点名是上游: {}",
                    kind.label()
                );
            }
            other => panic!("应为 Retrying，实为 {other:?}"),
        }
    }

    #[test]
    fn rate_limit_gives_up_after_max_long_retries() {
        use std::time::Duration;
        let long = Duration::from_secs(600);
        // 长等待的最后一次：count = 快重试次数 + 长等待上限 → 第 MAX 次，仍长等待。
        let last = RATE_LIMIT_FAST_ATTEMPTS + MAX_RATE_LIMIT_RETRIES;
        assert!(matches!(
            rate_limit_action(last, long, default_policy()),
            RateLimitAction::Long { nth, .. } if nth == MAX_RATE_LIMIT_RETRIES
        ));
        // 再多一次 → 到顶放弃。
        assert!(matches!(
            rate_limit_action(last + 1, long, default_policy()),
            RateLimitAction::GiveUp
        ));
    }

    #[test]
    fn sse_error_event_maps_to_retryable_api_error() {
        // 流内 overloaded（HTTP 200 后中途报错）→ 529，可重试（不再被吞成空响应）。
        let e = sse_error_to_llm_error(
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        );
        match &e {
            LlmError::Api { status, body } => {
                assert_eq!(*status, 529);
                assert!(body.contains("Overloaded"));
            }
            other => panic!("应为 Api 错误，实际 {other:?}"),
        }
        assert!(e.is_retryable());
        // 限流 → 429（可重试）。
        let rl = sse_error_to_llm_error(r#"{"error":{"type":"rate_limit_error"}}"#);
        assert!(matches!(rl, LlmError::Api { status: 429, .. }));
        assert!(rl.is_retryable());
        // 鉴权类 → 401（不重试，重试无意义）。
        let auth = sse_error_to_llm_error(r#"{"error":{"type":"authentication_error"}}"#);
        assert!(matches!(auth, LlmError::Api { status: 401, .. }));
        assert!(!auth.is_retryable());
        // 未知 / 畸形负载 → 500（按瞬时服务端错误处理，可重试）。
        let unknown = sse_error_to_llm_error("not json");
        assert!(matches!(unknown, LlmError::Api { status: 500, .. }));
        assert!(unknown.is_retryable());
    }

    #[test]
    fn provider_config_vision_defaults_false_and_builder_sets() {
        // 默认保守：未显式声明则按「不支持图片」处理（宁可降级也不硬报错）。
        let c = ProviderConfig::new("u", "k", WireFormat::OpenAi);
        assert!(!c.vision);
        let c2 = ProviderConfig::new("u", "k", WireFormat::OpenAi).with_vision(true);
        assert!(c2.vision);
    }

    #[test]
    fn redact_images_strips_and_annotates() {
        let mut msgs = vec![
            ChatMessage::user("hi"),
            ChatMessage::user_with_images(
                "看这张".to_string(),
                vec!["data:image/png;base64,AAA".to_string()],
            ),
        ];
        let n = redact_images(&mut msgs);
        assert_eq!(n, 1);
        // 图片被剥掉，原文保留，并补一行占位说明。
        assert!(msgs[1].images.is_empty());
        let body = msgs[1].content.as_deref().unwrap();
        assert!(body.contains("看这张"), "原文应保留: {body}");
        assert!(body.contains("不支持图片"), "应有占位说明: {body}");
        // 无图消息不受影响。
        assert_eq!(msgs[0].content.as_deref(), Some("hi"));
        assert!(msgs[0].images.is_empty());
    }

    #[test]
    fn redact_images_handles_image_only_message_and_counts() {
        let mut msgs = vec![ChatMessage::user_with_images(
            String::new(),
            vec!["a".to_string(), "b".to_string()],
        )];
        let n = redact_images(&mut msgs);
        assert_eq!(n, 2);
        assert!(msgs[0].images.is_empty());
        let body = msgs[0].content.as_deref().unwrap();
        assert!(body.contains('2'), "应报告剥离张数: {body}");
    }

    #[test]
    fn redact_images_noop_without_images() {
        let mut msgs = vec![ChatMessage::user("纯文本")];
        assert_eq!(redact_images(&mut msgs), 0);
        assert_eq!(msgs[0].content.as_deref(), Some("纯文本"));
    }

    #[test]
    fn repair_synthesizes_missing_tool_results() {
        use super::super::{Role, ToolCall};
        let tc = |id: &str| ToolCall {
            id: id.into(),
            name: "shell".into(),
            arguments: "{}".into(),
        };
        // 中断后只回了 a 的结果，b 的丢了。
        let mut msgs = vec![
            ChatMessage::user("做事"),
            ChatMessage::assistant_tool_calls(None, vec![tc("a"), tc("b")]),
            ChatMessage::tool_result("a", "ok"),
        ];
        assert!(needs_history_repair(&msgs));
        let rep = repair_tool_call_pairs(&mut msgs);
        assert_eq!(rep.synthesized, 1);
        // b 被补成占位 tool 消息，紧跟在已有结果之后。
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[3].role, Role::Tool);
        assert_eq!(msgs[3].tool_call_id.as_deref(), Some("b"));
    }

    #[test]
    fn repair_handles_dangling_tool_calls_at_end() {
        use super::super::ToolCall;
        // 最极端：assistant tool_calls 是最后一条（任务在工具执行前就被中断）。
        let mut msgs = vec![ChatMessage::assistant_tool_calls(
            None,
            vec![ToolCall {
                id: "x".into(),
                name: "shell".into(),
                arguments: "{}".into(),
            }],
        )];
        assert!(needs_history_repair(&msgs));
        assert_eq!(repair_tool_call_pairs(&mut msgs).synthesized, 1);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].tool_call_id.as_deref(), Some("x"));
    }

    #[test]
    fn repair_noop_when_all_paired() {
        use super::super::ToolCall;
        let mut msgs = vec![
            ChatMessage::assistant_tool_calls(
                None,
                vec![ToolCall {
                    id: "a".into(),
                    name: "shell".into(),
                    arguments: "{}".into(),
                }],
            ),
            ChatMessage::tool_result("a", "ok"),
            ChatMessage::assistant("done"),
        ];
        assert!(!needs_history_repair(&msgs));
        assert_eq!(repair_tool_call_pairs(&mut msgs), HistoryRepair::default());
        assert_eq!(msgs.len(), 3);
    }

    /// 断言历史满足 Anthropic 不变量：每条 tool 结果都紧跟在「带对应 tool_use 的 assistant」之后，
    /// 且每个 tool_use 都在其紧邻的 tool 块里被应答。
    fn assert_paired(msgs: &[ChatMessage]) {
        use super::super::Role;
        let mut i = 0;
        while i < msgs.len() {
            if msgs[i].role == Role::Tool {
                assert!(
                    i > 0
                        && msgs[i - 1].role == Role::Assistant
                        && !msgs[i - 1].tool_calls.is_empty(),
                    "tool 结果 #{i} 前不是带 tool_calls 的 assistant"
                );
                let owner: std::collections::HashSet<&str> = msgs[i - 1]
                    .tool_calls
                    .iter()
                    .map(|t| t.id.as_str())
                    .collect();
                let mut seen = std::collections::HashSet::new();
                while i < msgs.len() && msgs[i].role == Role::Tool {
                    let id = msgs[i].tool_call_id.as_deref().unwrap();
                    assert!(
                        owner.contains(id),
                        "tool 结果 {id} 的 tool_use 不在前一条 assistant"
                    );
                    seen.insert(id);
                    i += 1;
                }
                assert_eq!(seen.len(), owner.len(), "owner 的部分 tool_use 缺结果");
            } else {
                if msgs[i].role == Role::Assistant && !msgs[i].tool_calls.is_empty() {
                    assert!(
                        i + 1 < msgs.len() && msgs[i + 1].role == Role::Tool,
                        "assistant #{i} 的 tool_calls 后未紧跟 tool 结果"
                    );
                }
                i += 1;
            }
        }
    }

    #[test]
    fn repair_relocates_out_of_order_tool_results() {
        use super::super::ToolCall;
        let tc = |id: &str, name: &str| ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: "{}".into(),
        };
        // 复现真实 400：两个 assistant 工具回合中间没插入结果，结果乱序落在后面。
        //   a(useA) a(useB) t(resB) a(useC) t(resA) t(resC)
        // resA 的 tool_use 在 useA 回合，却落到了 useC 回合之后 → Anthropic 报
        // “unexpected tool_use_id … must have a corresponding tool_use block in the previous message”。
        let mut msgs = vec![
            ChatMessage::user("做事"),
            ChatMessage::assistant_tool_calls(None, vec![tc("A", "shell")]),
            ChatMessage::assistant_tool_calls(None, vec![tc("B", "read_file")]),
            ChatMessage::tool_result("B", "resB"),
            ChatMessage::assistant_tool_calls(None, vec![tc("C", "shell")]),
            ChatMessage::tool_result("A", "resA"),
            ChatMessage::tool_result("C", "resC"),
        ];
        assert!(needs_history_repair(&msgs), "乱序历史应判定需要修复");
        repair_tool_call_pairs(&mut msgs);
        // 规范化后：每个结果都紧跟其 tool_use 回合，且按 id 正确配对。
        assert_paired(&msgs);
        let pos = |id: &str| {
            msgs.iter()
                .position(|m| m.tool_call_id.as_deref() == Some(id))
                .unwrap()
        };
        assert_eq!(msgs[pos("A")].content.as_deref(), Some("resA"));
        assert_eq!(msgs[pos("B")].content.as_deref(), Some("resB"));
        assert_eq!(msgs[pos("C")].content.as_deref(), Some("resC"));
    }

    #[test]
    fn repair_drops_orphan_tool_result() {
        use super::super::ToolCall;
        // 一条结果的 tool_use 根本不存在（孤立）→ 必须丢弃，否则上游 400。
        let mut msgs = vec![
            ChatMessage::assistant_tool_calls(
                None,
                vec![ToolCall {
                    id: "a".into(),
                    name: "shell".into(),
                    arguments: "{}".into(),
                }],
            ),
            ChatMessage::tool_result("a", "ok"),
            ChatMessage::tool_result("ghost", "孤立结果"),
        ];
        assert!(needs_history_repair(&msgs), "存在孤立结果应判定需要修复");
        repair_tool_call_pairs(&mut msgs);
        assert_paired(&msgs);
        assert!(
            !msgs
                .iter()
                .any(|m| m.tool_call_id.as_deref() == Some("ghost")),
            "孤立结果应被丢弃"
        );
    }

    #[test]
    fn repair_uniquifies_duplicate_tool_use_ids_across_turns() {
        use super::super::ToolCall;
        let tc = |id: &str| ToolCall {
            id: id.into(),
            name: "x".into(),
            arguments: "{}".into(),
        };
        // 复现真实焊死：文本式调用恢复每轮都叫 text_call_0，跨回合 id 撞车。
        // 各自本地配对，但相同 id 出现在两条 assistant 上 → 必须各配各的结果且 id 全局唯一，
        // 否则按 id 配对会把两条结果折叠成一条、漏掉另一条 → 悬空 tool_use → 上游 400 焊死。
        let mut msgs = vec![
            ChatMessage::user("做事"),
            ChatMessage::assistant_tool_calls(None, vec![tc("text_call_0")]),
            ChatMessage::tool_result("text_call_0", "r1"),
            ChatMessage::assistant_tool_calls(None, vec![tc("text_call_0")]),
            ChatMessage::tool_result("text_call_0", "r2"),
        ];
        assert!(
            needs_history_repair(&msgs),
            "重复 tool_use id 应判定需要修复"
        );
        repair_tool_call_pairs(&mut msgs);
        assert_paired(&msgs);
        // 两条 assistant 的 tool_use id 必须各不相同（全局唯一）。
        let ids: Vec<String> = msgs
            .iter()
            .filter(|m| !m.tool_calls.is_empty())
            .map(|m| m.tool_calls[0].id.clone())
            .collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1], "重复 id 应被唯一化");
        // 结果仍按回合正确配对：第一条 r1、第二条 r2。
        let results: Vec<String> = msgs
            .iter()
            .filter(|m| m.role == super::super::Role::Tool)
            .filter_map(|m| m.content.clone())
            .collect();
        assert_eq!(results, vec!["r1".to_string(), "r2".to_string()]);
    }

    #[test]
    fn idle_timeout_env_override_parsed() {
        // 复位 → 默认 120s；非法值回落默认；合法值生效。
        std::env::remove_var("WC_STREAM_IDLE_TIMEOUT_SECS");
        assert_eq!(stream_idle_timeout().as_secs(), 120);
        std::env::set_var("WC_STREAM_IDLE_TIMEOUT_SECS", "0"); // 0 视为非法，回落
        assert_eq!(stream_idle_timeout().as_secs(), 120);
        std::env::set_var("WC_STREAM_IDLE_TIMEOUT_SECS", "7");
        assert_eq!(stream_idle_timeout().as_secs(), 7);
        std::env::remove_var("WC_STREAM_IDLE_TIMEOUT_SECS");
    }

    /// 复现 bug：上游回了 200 头却**永不发送响应体**（代理静默断流的典型形态）。
    /// 修复前 `stream.next().await` 会无限期挂起；修复后应在空闲超时后报 `Timeout`，
    /// 且经 `complete()` 的重试耗尽后整体在有限时间内返回（而非干等数小时）。
    #[tokio::test]
    async fn stalled_stream_times_out_instead_of_hanging() {
        use super::super::{ChatMessage, LlmRequest, WireFormat};

        async fn stall() -> axum::response::Response {
            // body 用一个永不产出的流：握手/响应头正常，但读 body 时永远拿不到数据。
            let body = axum::body::Body::from_stream(futures_util::stream::pending::<
                Result<String, std::io::Error>,
            >());
            axum::response::Response::builder()
                .status(200)
                .header("content-type", "text/event-stream")
                .body(body)
                .unwrap()
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let app = axum::Router::new().fallback(stall);
            let _ = axum::serve(listener, app).await;
        });

        std::env::set_var("WC_STREAM_IDLE_TIMEOUT_SECS", "1");
        let cfg = ProviderConfig::new(format!("http://{addr}"), "k", WireFormat::OpenAi);
        let req = LlmRequest::new("m".to_string(), vec![ChatMessage::user("hi")]);
        // 绕开本机可能配置的代理：测试要直连 127.0.0.1（否则会被代理拦成 5xx）。
        let client = LlmClient::with_http(reqwest::Client::builder().no_proxy().build().unwrap());

        let start = std::time::Instant::now();
        let err = client.complete(&cfg, &req, |_| {}).await.unwrap_err();
        std::env::remove_var("WC_STREAM_IDLE_TIMEOUT_SECS");

        assert!(
            matches!(err, LlmError::Timeout(_)),
            "应为空闲超时，实得 {err:?}"
        );
        // 3 次尝试 × ~1s 空闲 + 退避(0.5+1.0) ≈ 4.5s；给足上限确认「不再无限挂起」。
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "应在有限时间内返回，实耗 {:?}",
            start.elapsed()
        );
    }
}
