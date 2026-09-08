//! Provider 预设注册表。
//!
//! 主流 provider 归结为两种 wire format：
//!   - OpenAI 兼容（/chat/completions）：OpenAI、DeepSeek、Qwen、Gemini(兼容端点)
//!   - Anthropic（/v1/messages）：Claude
//!
//! BYOK：用户可用任意 base_url + 这两种 format 之一接入未列出的 provider。
//!
//! **模型表可外置维护**：因为各家 LLM 更新频繁，除内置 [`PRESETS`] 外，运行时还会读取
//! 用户配置目录下的 `wisecortex/providers.json` 覆盖文件（见 [`load_merged`]）。同 `id` 的
//! provider 用文件里给出的字段覆盖内置（`models` 给出即整表替换），文件里新增的 provider
//! 直接加入。文件缺失/损坏则纯用内置，保证开箱即用。加模型 = 编辑该文件后重启，无需重编译。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 与 provider 通信使用的线缆格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WireFormat {
    /// OpenAI 兼容 /chat/completions。
    #[default]
    OpenAi,
    /// Anthropic /v1/messages。
    Anthropic,
    /// OpenAI Responses API（/responses）。ChatGPT/Codex 订阅后端走这个。
    #[serde(rename = "openai_responses")]
    OpenAiResponses,
    /// 原生 Gemini（Code Assist `v1internal:streamGenerateContent`）。Gemini 订阅走这个。
    Gemini,
}

/// 该 provider 在 **OpenAI 兼容线缆**下如何表达「深度思考 / 推理强度」。
///
/// 各家把同一件事写成了不同参数，统一在出站时按本枚举翻译（见 [`super::openai::build_request_body`]）：
///   - `reasoning_effort`：发 `reasoning_effort: <level>`（OpenAI / o 系列 / Gemini 兼容端点）。
///     档位上限**按模型**定而非按 provider，见 [`clamp_effort`]。
///   - `enable_thinking`：发 `enable_thinking: true`（Qwen / 混元 等「混合思考」模型，布尔开关）。
///   - `thinking_enabled_effort`：发 `thinking: {"type":"enabled"}` + `reasoning_effort: <level>`
///     （DeepSeek V4：思考开关用对象、强度仍用 reasoning_effort，两者一起发）。
///   - `none`：不发任何思考参数（思考由模型内置或不支持）。
///
/// Anthropic 线缆不看本字段（思考由 [`super::anthropic`] 自行处理）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingMode {
    /// 发 `reasoning_effort: <level>`。BYOK / 未指定时的默认（保守地沿用 OpenAI 行为）。
    #[default]
    ReasoningEffort,
    /// 发 `enable_thinking: true`。
    EnableThinking,
    /// 发 `thinking: {"type": "enabled"}` + `reasoning_effort: <level>`（DeepSeek V4 等：思考开关用
    /// 对象、强度仍用 reasoning_effort，两者一起发）。
    ThinkingEnabledEffort,
    /// 发 `reasoning_effort: <minimal|low|medium|high>`，其中 `minimal`=不思考（火山引擎/豆包）。
    /// 与 `ReasoningEffort` 的区别：关闭=显式发 `minimal`（而非省略），且只有这四档（高于 high 钳到 high）。
    ReasoningEffortMinimal,
    /// 发 `thinking: {"type": "enabled" | "disabled"}`（开/关，无强度档；Moonshot/Kimi K2）。
    /// 开=enabled、关=disabled（始终显式发，以便真正关闭——默认是 enabled）。
    ThinkingObjectToggle,
    /// 不发思考参数。
    None,
}

/// 内置 provider 预设。
#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub format: WireFormat,
    /// 深度思考参数的表达方式（仅 OpenAI 线缆下生效）。
    pub thinking: ThinkingMode,
    pub default_model: &'static str,
    pub models: &'static [&'static str],
    /// 支持图片输入（vision）的模型白名单。**保守策略**：只有在此显式列出的模型才会被
    /// 发送图片；未登记的一律按「不支持」处理（发图前剥离 + 文字占位），宁可降级也不让
    /// 多模态内容把上游打成 4xx（如 deepseek-v4 收到 image_url 块直接拒）。
    pub vision_models: &'static [&'static str],
}

/// OpenAI 模型表。GPT-5.6 有三个变体（sol / terra / luna），全系支持图片输入，故与
/// `vision_models` 共用一张表。
const OPENAI_MODELS: &[&str] = &[
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
    "gpt-5.5",
    "gpt-5.4",
    "gpt-5.4-mini",
    "o4-mini",
    "o3",
];

/// 推理档位阶梯（弱 → 强）。只列 WiseCortex 档位选择器**能选出来**的档：
/// 空串=不发（走上游默认），其余五档见 `web/src/settingsView.ts` 的 `#sv-effort`。
/// OpenAI 另有比 `low` 更弱的 `none` / `minimal`，但 UI 给不出来，故不参与钳位。
const EFFORT_LADDER: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// 该模型认得的**最高**推理档位。各家档位不齐，发超纲的档会被上游 400 打回，故出站前钳到天花板。
///
/// 档位随模型代次逐步放开（对齐 OpenAI 各模型页；`xhigh` 自 GPT-5.2 起、`max` 自 GPT-5.6 起）：
///   - GPT-5.6 系（sol/terra/luna）：到 `max`
///   - GPT-5.2 ~ 5.5、GPT-5.2+ 的 codex / codex-max：到 `xhigh`
///   - 其余（GPT-5.1、裸 gpt-5、gpt-5-codex、o3/o4、混元 hy3、Gemini 兼容层、未知 BYOK）：到 `high`
fn effort_ceiling(model: &str) -> &'static str {
    let id = model.to_ascii_lowercase();
    // gpt-5 家族版本号：`gpt-5.6-sol` → 6、`gpt-5.4-codex` → 4；裸 `gpt-5` / `gpt-5-codex` → None。
    // 锚在头部或 `/` 之后，免得 `gpt-50` / 某些 BYOK 前缀误命中。
    let version = id
        .split_once("gpt-5.")
        .filter(|(head, _)| head.is_empty() || head.ends_with('/'))
        .and_then(|(_, rest)| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u32>().ok()
        });
    match version {
        Some(v) if v >= 6 && !id.contains("codex") => "max",
        Some(v) if v >= 2 => "xhigh",
        _ if id.contains("codex-max") => "xhigh",
        _ => "high",
    }
}

/// 把用户选的档位钳到 `model` 真的支持的范围（超纲档取天花板）。`None` = 不发该参数。
///
/// 供 **OpenAI 线缆**（chat completions）与 **Responses 线缆**（ChatGPT 订阅/Codex）共用——
/// 两边曾各写一份且都不对：Responses 直接把 `xhigh`/`max` **整个丢掉**（于是退回上游默认 medium，
/// 表现为「选了 xhigh 却答得飞快」），chat completions 则无条件砍到 `high`。
pub fn clamp_effort(model: &str, effort: &str) -> Option<&'static str> {
    let want = EFFORT_LADDER.iter().position(|e| *e == effort)?;
    let cap = EFFORT_LADDER
        .iter()
        .position(|e| *e == effort_ceiling(model))
        .unwrap_or(2);
    Some(EFFORT_LADDER[want.min(cap)])
}

/// 全部内置预设。BYOK 自定义端点不在此列。
pub const PRESETS: &[Provider] = &[
    Provider {
        id: "openai",
        name: "OpenAI (GPT)",
        base_url: "https://api.openai.com/v1",
        format: WireFormat::OpenAi,
        thinking: ThinkingMode::ReasoningEffort,
        default_model: "gpt-5.6-sol",
        models: OPENAI_MODELS,
        // GPT-5.x 与 o 系列均支持图片输入。
        vision_models: OPENAI_MODELS,
    },
    Provider {
        id: "anthropic",
        name: "Anthropic (Claude)",
        base_url: "https://api.anthropic.com",
        format: WireFormat::Anthropic,
        // Anthropic 线缆自行处理 thinking，本字段不参与。
        thinking: ThinkingMode::None,
        default_model: "claude-sonnet-4-6",
        // 模型 id 都是无日期后缀的固定串（claude-opus-5 而非 claude-opus-5-2026xxxx）。
        models: &[
            "claude-opus-5",
            "claude-opus-4-8",
            "claude-opus-4-7",
            "claude-sonnet-4-6",
            "claude-haiku-4-5",
        ],
        // Claude 4/5 全系支持图片输入。
        vision_models: &[
            "claude-opus-5",
            "claude-opus-4-8",
            "claude-opus-4-7",
            "claude-sonnet-4-6",
            "claude-haiku-4-5",
        ],
    },
    Provider {
        id: "deepseek",
        name: "DeepSeek",
        base_url: "https://api.deepseek.com",
        format: WireFormat::OpenAi,
        // DeepSeek V4（pro/flash）：思考用 `thinking:{type:enabled}` 开关 + `reasoning_effort` 强度，
        // 两者一起发（旧的 deepseek-chat/reasoner 模型 2026-07-24 弃用）。
        thinking: ThinkingMode::ThinkingEnabledEffort,
        default_model: "deepseek-v4-pro",
        models: &["deepseek-v4-pro", "deepseek-v4-flash"],
        // DeepseeK 现有模型均不支持图片输入（发图会被打回）。
        vision_models: &[],
    },
    Provider {
        id: "qwen",
        name: "Qwen (Alibaba)",
        base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        format: WireFormat::OpenAi,
        // 千问混合思考模型用布尔开关 enable_thinking（需流式，我们本就全程流式）。
        thinking: ThinkingMode::EnableThinking,
        default_model: "qwen3.7-max",
        models: &["qwen3.7-max", "qwen3.7-plus", "qwen3.6-flash"],
        // qwen3.7-plus 与 qwen3.6-flash 为多模态，支持图片输入；qwen3.7-max 为纯文本。
        vision_models: &["qwen3.7-plus", "qwen3.6-flash"],
    },
    Provider {
        // 腾讯混元 TokenHub 的 OpenAI 兼容端点（免签名，直接 Bearer）。
        id: "hunyuan",
        name: "腾讯混元 (OpenAI 兼容)",
        base_url: "https://tokenhub.tencentmaas.com/v1",
        format: WireFormat::OpenAi,
        // hy3-preview 用标准 reasoning_effort（low/medium/high）。
        thinking: ThinkingMode::ReasoningEffort,
        default_model: "hy3-preview",
        models: &["hy3-preview"],
        // hy3-preview 不支持图片输入。
        vision_models: &[],
    },
    Provider {
        // Google Gemini 的 OpenAI 兼容端点。
        // 勾选「Gemini 订阅」后，线缆自动切换为原生 Code Assist（不走此 base_url），
        // 故 2.5 系（GA stable）和 3 系（preview）的模型 ID 可以并列在此。
        // 2.5 系（gemini-2.5-pro / gemini-2.5-flash）是 Code Assist OAuth 路径最稳定的模型：
        //   带 GeminiCLI UA → 6/6 成功；3 系带 UA → 404（自愈逻辑会去掉 UA 重试，但限流更重）。
        id: "gemini",
        name: "Google Gemini (OpenAI 兼容)",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        format: WireFormat::OpenAi,
        // Gemini 默认思考；OpenAI 兼容层支持 reasoning_effort(low/medium/high) 调节强度。
        thinking: ThinkingMode::ReasoningEffort,
        default_model: "gemini-2.5-pro",
        models: &[
            // GA stable（Code Assist OAuth 推荐）
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            // Preview（只能 API Key 或 OAuth 不带 UA 路径）
            "gemini-3.1-pro-preview",
            "gemini-3.5-flash",
            "gemini-3.1-flash-lite",
        ],
        // Gemini 2.5/3.x 全系多模态，支持图片输入。
        vision_models: &[
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-3.1-pro-preview",
            "gemini-3.5-flash",
            "gemini-3.1-flash-lite",
        ],
    },
    Provider {
        id: "volcengine",
        name: "火山引擎 (豆包 Doubao)",
        base_url: "https://ark.cn-beijing.volces.com/api/v3",
        format: WireFormat::OpenAi,
        // 豆包用 reasoning_effort，档位 minimal/low/medium/high（minimal=不思考）。
        thinking: ThinkingMode::ReasoningEffortMinimal,
        default_model: "doubao-seed-2-0-pro",
        // 用不带日期的 base id；若实际可用 id 带日期后缀（如示例 doubao-seed-2-0-pro-260215），
        // 在模型管理 / providers.json 里改成准确 id 即可。
        models: &[
            "doubao-seed-2-0-pro",
            "doubao-seed-2-0-lite",
            "doubao-seed-2-0-mini",
            "doubao-seed-2-0-code",
        ],
        // Doubao Seed 2.0 全系（pro/lite/mini/code）均为多模态，支持图片输入。
        // 注意：vision 白名单按 id 精确匹配，若实际用带日期后缀的 id，务必在此（或 providers.json）登记同名。
        vision_models: &[
            "doubao-seed-2-0-pro",
            "doubao-seed-2-0-lite",
            "doubao-seed-2-0-mini",
            "doubao-seed-2-0-code",
        ],
    },
    Provider {
        id: "moonshot",
        name: "月之暗面 (Kimi)",
        base_url: "https://api.moonshot.cn/v1",
        format: WireFormat::OpenAi,
        // Kimi K2 用 thinking:{type:enabled|disabled} 开关（无强度档）。k2.7-code 思考无法关闭。
        thinking: ThinkingMode::ThinkingObjectToggle,
        default_model: "kimi-k2.7-code",
        models: &["kimi-k2.7-code", "kimi-k2.6"],
        // kimi-k2.7-code 与 kimi-k2.6 均为多模态，支持图片输入。
        vision_models: &["kimi-k2.7-code", "kimi-k2.6"],
    },
    Provider {
        id: "zhipu",
        name: "智谱 (GLM)",
        base_url: "https://open.bigmodel.cn/api/paas/v4",
        format: WireFormat::OpenAi,
        // GLM 与 Kimi 同样用 thinking:{type:enabled|disabled} 开关（无强度档）。
        thinking: ThinkingMode::ThinkingObjectToggle,
        default_model: "glm-5.2",
        models: &["glm-5.2", "glm-5.1"],
        // glm-5.2 与 glm-5.1 均为多模态，支持图片输入。
        vision_models: &["glm-5.2", "glm-5.1"],
    },
    Provider {
        id: "xai",
        name: "xAI (Grok)",
        base_url: "https://api.x.ai/v1",
        format: WireFormat::OpenAi,
        // xAI 只有 grok-3-mini 支持 reasoning_effort，grok-4 系发了会被打回 → 预设不发
        // （grok-4/4-1 自带推理，无需外部开关）。
        thinking: ThinkingMode::None,
        default_model: "grok-code-fast-1",
        models: &[
            "grok-code-fast-1",
            "grok-4-1",
            "grok-4-1-fast",
            "grok-4",
            "grok-4-fast",
        ],
        // grok-4/4-1 系多模态；grok-code-fast-1 纯文本。
        vision_models: &["grok-4-1", "grok-4-1-fast", "grok-4", "grok-4-fast"],
    },
    Provider {
        // 任意 OpenAI 兼容端点（自建/本地/第三方网关）。base_url 必须自填。
        id: "openai-compatible",
        name: "OpenAI 兼容（自填 Endpoint）",
        base_url: "",
        format: WireFormat::OpenAi,
        thinking: ThinkingMode::ReasoningEffort,
        default_model: "",
        models: &[],
        // 自填端点：能力未知，需视觉请在 providers.json 的 vision_models 里登记。
        vision_models: &[],
    },
];

/// 按 id 查预设。
pub fn get(id: &str) -> Option<&'static Provider> {
    PRESETS.iter().find(|p| p.id == id)
}

/// 列出 (id, name)。
pub fn list() -> Vec<(&'static str, &'static str)> {
    PRESETS.iter().map(|p| (p.id, p.name)).collect()
}

/// Provider 的「拥有所有权」版本（运行时可由覆盖文件构造，不再是 `&'static`）。
/// 这是 `/api/providers` 实际返回、前端模型下拉读取的结构。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub format: WireFormat,
    #[serde(default)]
    pub thinking: ThinkingMode,
    #[serde(default)]
    pub default_model: String,
    #[serde(default)]
    pub models: Vec<String>,
    /// 支持图片输入的模型白名单（保守策略，见 [`Provider::vision_models`]）。
    #[serde(default)]
    pub vision_models: Vec<String>,
}

/// 把内置 [`PRESETS`] 转成可变的拥有版本，作为合并起点。
pub fn presets_owned() -> Vec<ProviderInfo> {
    PRESETS
        .iter()
        .map(|p| ProviderInfo {
            id: p.id.to_string(),
            name: p.name.to_string(),
            base_url: p.base_url.to_string(),
            format: p.format,
            thinking: p.thinking,
            default_model: p.default_model.to_string(),
            models: p.models.iter().map(|m| m.to_string()).collect(),
            vision_models: p.vision_models.iter().map(|m| m.to_string()).collect(),
        })
        .collect()
}

/// 覆盖文件里单个 provider 的可选字段（仅给出的字段才覆盖）。
#[derive(Debug, Deserialize)]
struct ProviderOverride {
    id: String,
    name: Option<String>,
    base_url: Option<String>,
    format: Option<WireFormat>,
    /// 思考参数表达方式：reasoning_effort | enable_thinking | none。
    thinking: Option<ThinkingMode>,
    default_model: Option<String>,
    /// 给出即**整表替换**该 provider 的模型列表（便于增删）。
    models: Option<Vec<String>>,
    /// 给出即**整表替换**该 provider 的视觉模型白名单（登记/撤销支持图片的模型）。
    vision_models: Option<Vec<String>>,
}

/// `providers.json` 顶层结构。
#[derive(Debug, Deserialize)]
struct OverrideFile {
    #[serde(default)]
    providers: Vec<ProviderOverride>,
}

/// 把覆盖 JSON 合并进 base：同 id 覆盖给出的字段，新 id 追加。JSON 损坏则原样返回 base。
pub fn merge_overrides(mut base: Vec<ProviderInfo>, json: &str) -> Vec<ProviderInfo> {
    let parsed: OverrideFile = match serde_json::from_str(json) {
        Ok(p) => p,
        Err(_) => return base,
    };
    for ov in parsed.providers {
        if let Some(existing) = base.iter_mut().find(|p| p.id == ov.id) {
            if let Some(v) = ov.name {
                existing.name = v;
            }
            if let Some(v) = ov.base_url {
                existing.base_url = v;
            }
            if let Some(v) = ov.format {
                existing.format = v;
            }
            if let Some(v) = ov.thinking {
                existing.thinking = v;
            }
            if let Some(v) = ov.default_model {
                existing.default_model = v;
            }
            if let Some(v) = ov.models {
                existing.models = v;
            }
            if let Some(v) = ov.vision_models {
                existing.vision_models = v;
            }
        } else {
            let id = ov.id;
            base.push(ProviderInfo {
                name: ov.name.unwrap_or_else(|| id.clone()),
                base_url: ov.base_url.unwrap_or_default(),
                format: ov.format.unwrap_or_default(),
                thinking: ov.thinking.unwrap_or_default(),
                default_model: ov.default_model.unwrap_or_default(),
                models: ov.models.unwrap_or_default(),
                vision_models: ov.vision_models.unwrap_or_default(),
                id,
            });
        }
    }
    base
}

/// 覆盖文件路径：`<配置目录>/wisecortex/providers.json`（与 config.json 同级）。
pub fn providers_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("wisecortex").join("providers.json"))
}

/// 内置预设 + 用户覆盖文件合并后的最终列表（文件缺失/损坏则纯用内置）。
pub fn load_merged() -> Vec<ProviderInfo> {
    let base = presets_owned();
    match providers_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(json) => merge_overrides(base, &json),
        None => base,
    }
}

/// 在给定 provider 表里查 `(provider_id, model)` 是否在视觉白名单中（纯函数，便于测试）。
fn vision_in(list: &[ProviderInfo], provider_id: &str, model: &str) -> bool {
    list.iter()
        .find(|p| p.id == provider_id)
        .is_some_and(|p| p.vision_models.iter().any(|m| m == model))
}

/// 某 provider 的某模型是否支持图片输入（vision）。**保守白名单**：仅当模型在
/// `vision_models` 中显式登记才返回 true；未登记的模型/未知 provider 一律 false。
/// 读取合并表，故 `providers.json` 里登记的视觉模型也即时生效（改文件重启即可，无需重编译）。
pub fn supports_vision(provider_id: &str, model: &str) -> bool {
    vision_in(&load_merged(), provider_id, model)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpt56_keeps_xhigh_and_max_instead_of_being_clamped_or_dropped() {
        // 真机症状：全局选了 xhigh，Responses 线缆却把它整个丢掉（退回上游默认 medium），
        // chat completions 则砍到 high——两边都不是用户选的档。
        for m in ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"] {
            assert_eq!(clamp_effort(m, "xhigh"), Some("xhigh"), "{m}");
            assert_eq!(clamp_effort(m, "max"), Some("max"), "{m}");
            assert_eq!(clamp_effort(m, "low"), Some("low"), "{m}");
        }
    }

    #[test]
    fn effort_is_clamped_to_each_models_ceiling() {
        // xhigh 自 GPT-5.2 起才有 → 5.5/5.4 收 xhigh，max 也钳到 xhigh。
        assert_eq!(clamp_effort("gpt-5.5", "max"), Some("xhigh"));
        assert_eq!(clamp_effort("gpt-5.4", "xhigh"), Some("xhigh"));
        // GPT-5.1 / 裸 gpt-5 / o 系列没有 xhigh → 钳到 high。
        assert_eq!(clamp_effort("gpt-5.1", "xhigh"), Some("high"));
        assert_eq!(clamp_effort("gpt-5-codex", "max"), Some("high"));
        assert_eq!(clamp_effort("o3", "xhigh"), Some("high"));
        // codex-max 认 xhigh；带版本的 5.2+ codex 同理。
        assert_eq!(clamp_effort("gpt-5-codex-max", "max"), Some("xhigh"));
        assert_eq!(clamp_effort("gpt-5.4-codex", "max"), Some("xhigh"));
        // 非 OpenAI 的 OpenAI 兼容端点（混元 / Gemini 兼容层）保持三档不变。
        assert_eq!(clamp_effort("hy3-preview", "max"), Some("high"));
        assert_eq!(
            clamp_effort("gemini-3.1-pro-preview", "xhigh"),
            Some("high")
        );
        // 天花板只往下钳，不会把低档抬上去。
        assert_eq!(clamp_effort("gpt-5.6-sol", "medium"), Some("medium"));
    }

    #[test]
    fn empty_or_unknown_effort_sends_nothing() {
        // "" = 未配置/关闭 → 不发参数（走上游默认），别自作主张塞一个档。
        assert_eq!(clamp_effort("gpt-5.6-sol", ""), None);
        assert_eq!(clamp_effort("gpt-5.6-sol", "bogus"), None);
    }

    #[test]
    fn version_parse_does_not_false_match() {
        // 前缀粘连 / 非 gpt-5 家族的，一律按保守的 high 天花板。
        assert_eq!(clamp_effort("gpt-50.6", "max"), Some("high"));
        assert_eq!(clamp_effort("notgpt-5.6", "max"), Some("high"));
        // 带 provider 前缀（OpenRouter 风格）仍应识别。
        assert_eq!(clamp_effort("openai/gpt-5.6-sol", "max"), Some("max"));
    }

    #[test]
    fn openai_preset_exposes_the_gpt56_variants() {
        let p = PRESETS.iter().find(|p| p.id == "openai").unwrap();
        for m in ["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"] {
            assert!(p.models.contains(&m), "{m} 应在 OpenAI 模型表里");
            assert!(p.vision_models.contains(&m), "{m} 应支持图片输入");
        }
        assert!(p.models.contains(&p.default_model));
    }

    #[test]
    fn known_providers_present() {
        for id in [
            "openai",
            "anthropic",
            "deepseek",
            "qwen",
            "hunyuan",
            "gemini",
        ] {
            assert!(get(id).is_some(), "missing provider {id}");
        }
        assert!(get("nope").is_none());
    }

    #[test]
    fn claude_uses_anthropic_format_others_openai() {
        assert_eq!(get("anthropic").unwrap().format, WireFormat::Anthropic);
        for id in ["openai", "deepseek", "qwen", "hunyuan", "gemini"] {
            assert_eq!(get(id).unwrap().format, WireFormat::OpenAi, "{id}");
        }
    }

    #[test]
    fn thinking_modes_match_provider_quirks() {
        // 千问走布尔开关 enable_thinking。
        assert_eq!(get("qwen").unwrap().thinking, ThinkingMode::EnableThinking);
        // 混元 hy3-preview 走标准 reasoning_effort，端点为 TokenHub。
        assert_eq!(
            get("hunyuan").unwrap().thinking,
            ThinkingMode::ReasoningEffort
        );
        assert_eq!(
            get("hunyuan").unwrap().base_url,
            "https://tokenhub.tencentmaas.com/v1"
        );
        // OpenAI / Gemini 走 reasoning_effort。
        assert_eq!(
            get("openai").unwrap().thinking,
            ThinkingMode::ReasoningEffort
        );
        assert_eq!(
            get("gemini").unwrap().thinking,
            ThinkingMode::ReasoningEffort
        );
        // DeepSeek V4：thinking 对象开关 + reasoning_effort 强度一起发。
        assert_eq!(
            get("deepseek").unwrap().thinking,
            ThinkingMode::ThinkingEnabledEffort
        );
    }

    #[test]
    fn override_can_set_thinking_mode() {
        let json = r#"{"providers":[{"id":"deepseek","thinking":"enable_thinking"}]}"#;
        let merged = merge_overrides(presets_owned(), json);
        let d = merged.iter().find(|p| p.id == "deepseek").unwrap();
        assert_eq!(d.thinking, ThinkingMode::EnableThinking);
    }

    #[test]
    fn list_covers_all_presets() {
        assert_eq!(list().len(), PRESETS.len());
    }

    #[test]
    fn presets_owned_mirrors_presets() {
        let owned = presets_owned();
        assert_eq!(owned.len(), PRESETS.len());
        let anthropic = owned.iter().find(|p| p.id == "anthropic").unwrap();
        assert_eq!(anthropic.format, WireFormat::Anthropic);
        assert!(anthropic.models.iter().any(|m| m == "claude-sonnet-4-6"));
    }

    #[test]
    fn vision_whitelist_is_conservative() {
        let p = presets_owned();
        // 已知视觉模型：Claude 全系。
        assert!(vision_in(&p, "anthropic", "claude-opus-5"));
        assert!(vision_in(&p, "anthropic", "claude-opus-4-8"));
        // 无视觉模型：deepseek（用户踩坑的那家）。
        assert!(!vision_in(&p, "deepseek", "deepseek-v4-pro"));
        // 未登记的模型一律按「不支持」——保守白名单，宁可降级也不硬报错。
        assert!(!vision_in(&p, "anthropic", "claude-made-up-9"));
        // 未知 provider 同理。
        assert!(!vision_in(&p, "nope", "whatever"));
    }

    #[test]
    fn override_can_register_vision_model() {
        // 用户在 providers.json 登记一个新视觉模型（不重编译即生效）。
        let json = r#"{"providers":[{"id":"deepseek","models":["deepseek-vl","deepseek-v4-pro"],"vision_models":["deepseek-vl"]}]}"#;
        let merged = merge_overrides(presets_owned(), json);
        assert!(vision_in(&merged, "deepseek", "deepseek-vl"));
        // 同档其它模型仍按不支持。
        assert!(!vision_in(&merged, "deepseek", "deepseek-v4-pro"));
    }

    #[test]
    fn override_replaces_models_of_existing_provider() {
        let json = r#"{"providers":[{"id":"anthropic","models":["claude-opus-4-9","claude-sonnet-4-7"],"default_model":"claude-opus-4-9"}]}"#;
        let merged = merge_overrides(presets_owned(), json);
        let a = merged.iter().find(|p| p.id == "anthropic").unwrap();
        assert_eq!(a.models, ["claude-opus-4-9", "claude-sonnet-4-7"]);
        assert_eq!(a.default_model, "claude-opus-4-9");
        // 未提及的字段保持内置值。
        assert_eq!(a.format, WireFormat::Anthropic);
        assert_eq!(a.base_url, "https://api.anthropic.com");
        // 未提及的 provider 不受影响。
        assert!(merged.iter().any(|p| p.id == "openai"));
    }

    #[test]
    fn override_adds_new_provider() {
        let json = r#"{"providers":[{"id":"my-gw","name":"我的网关","base_url":"http://localhost:8000/v1","format":"openai","models":["local-7b"]}]}"#;
        let merged = merge_overrides(presets_owned(), json);
        let p = merged.iter().find(|p| p.id == "my-gw").unwrap();
        assert_eq!(p.name, "我的网关");
        assert_eq!(p.format, WireFormat::OpenAi);
        assert_eq!(p.models, ["local-7b"]);
        assert_eq!(merged.len(), PRESETS.len() + 1);
    }

    #[test]
    fn malformed_override_yields_base_unchanged() {
        let base = presets_owned();
        let merged = merge_overrides(base.clone(), "{ not json");
        assert_eq!(merged, base);
    }

    #[test]
    fn shipped_example_file_parses_and_merges() {
        // 保证仓库里的模板始终是合法、可用的覆盖文件（含说明字段被安全忽略、新增网关被加入）。
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/providers.example.json"
        );
        let json = std::fs::read_to_string(path).expect("读取示例文件");
        let merged = merge_overrides(presets_owned(), &json);
        assert!(merged.iter().any(|p| p.id == "my-gateway"));
        let a = merged.iter().find(|p| p.id == "anthropic").unwrap();
        assert!(a.models.iter().any(|m| m == "claude-opus-4-8"));
    }
}
