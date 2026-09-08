//! 模型定价（**人民币元 / 百万 token**），用于把用量换算成成本。前端显示 ¥。
//!
//! DeepSeek 为官方人民币现价（精确）；其余家族由美元价 ×7.2 近似换算，**可按需调整**。
//! 不做 >200K 分档（绝大多数会话用不到）。未知模型返回 None → 显示 N/A。
//!
//! 成本口径与 Usage 的归一一致：prompt_tokens 含 cache_read，故
//!   regular_input = prompt_tokens − cache_read（按 input 费率）
//!   cache_read（按 read 费率）、cache_creation（按 write 费率）、completion（按 output 费率）。

use super::Usage;

/// 每百万 token 的费率（USD）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pricing {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl Pricing {
    const fn new(input: f64, output: f64, cache_read: f64, cache_write: f64) -> Self {
        Pricing {
            input,
            output,
            cache_read,
            cache_write,
        }
    }
}

/// 按模型名（家族子串）查费率。
pub fn lookup(model: &str) -> Option<Pricing> {
    let m = model.to_ascii_lowercase();
    let has = |s: &str| m.contains(s);

    // ── DeepSeek（官方人民币现价；无单独 cache-write 计费，按 miss 费率）──
    // flash: 命中 0.02 / 未命中 1 / 输出 2 ；pro: 命中 0.025 / 未命中 3 / 输出 6（元/百万）。
    if has("deepseek") {
        if has("flash") {
            return Some(Pricing::new(1.0, 2.0, 0.02, 1.0));
        }
        if has("pro") {
            return Some(Pricing::new(3.0, 6.0, 0.025, 3.0));
        }
        // deepseek-chat / deepseek-reasoner 及默认：按 flash 近似（用 v4 模型名可得精确价）。
        return Some(Pricing::new(1.0, 2.0, 0.02, 1.0));
    }

    // ── 以下为美元价 ×7.2 的人民币近似，按需校准 ──

    // Claude 家族
    if has("claude") {
        if has("opus") {
            return Some(Pricing::new(36.0, 180.0, 3.6, 45.0));
        }
        if has("haiku") {
            return Some(Pricing::new(7.2, 36.0, 0.72, 9.0));
        }
        // sonnet / 其它 Claude
        return Some(Pricing::new(21.6, 108.0, 2.16, 27.0));
    }

    // OpenAI GPT / o 系列
    if has("gpt") || has("o3") || has("o4") {
        if has("mini") || has("nano") {
            return Some(Pricing::new(1.08, 4.32, 0.54, 0.0));
        }
        return Some(Pricing::new(18.0, 72.0, 9.0, 0.0));
    }

    // Qwen
    if has("qwen") {
        if has("flash") {
            return Some(Pricing::new(0.36, 2.88, 0.14, 0.0));
        }
        return Some(Pricing::new(2.88, 8.64, 1.15, 0.0));
    }

    // Gemini
    if has("gemini") {
        if has("flash") {
            return Some(Pricing::new(2.16, 18.0, 0.54, 0.0));
        }
        return Some(Pricing::new(9.0, 72.0, 2.23, 0.0));
    }

    None
}

/// 用用量计算单次调用成本。未知模型且无自定义价时返回 None。
/// `override_io`：(输入价, 输出价) 元/百万 token；存在时覆盖内置费率表
/// （缓存读/写按输入价近似，因自定义只提供两档价）。
/// `override_price`：自定义费率 `(input, output, cache_read, cache_write)`，元/百万 token。
/// 缓存读/写费率显式传入（调用方据配置或默认折扣计算），不再误用全价输入价。
pub fn calculate_cost_with(
    model: &str,
    usage: &Usage,
    override_price: Option<(f64, f64, f64, f64)>,
) -> Option<f64> {
    let p = match override_price {
        Some((input, output, cache_read, cache_write)) => {
            Pricing::new(input, output, cache_read, cache_write)
        }
        None => lookup(model)?,
    };
    let cache_read = usage.cache_read_input_tokens as f64;
    let cache_write = usage.cache_creation_input_tokens as f64;
    let regular_input = (usage.prompt_tokens as f64 - cache_read).max(0.0);
    let output = usage.completion_tokens as f64;
    let cost = regular_input * p.input
        + cache_read * p.cache_read
        + cache_write * p.cache_write
        + output * p.output;
    Some(cost / 1_000_000.0)
}

/// 用用量计算单次调用成本（内置费率表）。未知模型返回 None。
pub fn calculate_cost(model: &str, usage: &Usage) -> Option<f64> {
    let p = lookup(model)?;
    let cache_read = usage.cache_read_input_tokens as f64;
    let cache_write = usage.cache_creation_input_tokens as f64;
    // prompt_tokens 含 cache_read，扣掉得到按标准 input 计费的部分。
    let regular_input = (usage.prompt_tokens as f64 - cache_read).max(0.0);
    let output = usage.completion_tokens as f64;

    let cost = regular_input * p.input
        + cache_read * p.cache_read
        + cache_write * p.cache_write
        + output * p.output;
    Some(cost / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_families() {
        // Opus 5 与 4.8 同价（$5/$25 每百万），走同一条 opus 分支。
        assert_eq!(lookup("claude-opus-5"), lookup("claude-opus-4-8"));
        assert!(lookup("claude-opus-4-8").is_some());
        assert!(lookup("deepseek-chat").is_some());
        assert!(lookup("gpt-5.5").is_some());
        assert!(lookup("qwen3.7-max").is_some());
        assert!(lookup("gemini-2.5-pro").is_some());
        assert!(lookup("some-unknown-model").is_none());
    }

    #[test]
    fn cost_accounts_for_cache_read_discount() {
        // 100k prompt 中 90k 命中缓存，10k 输出，deepseek-chat。
        let usage = Usage {
            prompt_tokens: 100_000,
            completion_tokens: 10_000,
            cache_read_input_tokens: 90_000,
            ..Default::default()
        };
        let cost = calculate_cost("deepseek-chat", &usage).unwrap();
        // 从费率表推导期望值，避免硬编码（改价不破坏测试）。
        let p = lookup("deepseek-chat").unwrap();
        let expected =
            (10_000.0 * p.input + 90_000.0 * p.cache_read + 10_000.0 * p.output) / 1_000_000.0;
        assert!((cost - expected).abs() < 1e-12);
        // 缓存确实更便宜：同量无缓存应更贵。
        let no_cache = Usage {
            prompt_tokens: 100_000,
            completion_tokens: 10_000,
            ..Default::default()
        };
        assert!(calculate_cost("deepseek-chat", &no_cache).unwrap() > cost);
    }

    #[test]
    fn override_price_applies_cache_read_discount() {
        // 自定义费率：input=10, output=20, cache_read=1（=input×0.1）, cache_write=0。
        // 100k prompt 中 90k 命中缓存。缓存读应按 1 计，而非全价 10。
        let usage = Usage {
            prompt_tokens: 100_000,
            completion_tokens: 10_000,
            cache_read_input_tokens: 90_000,
            ..Default::default()
        };
        let cost = calculate_cost_with("x", &usage, Some((10.0, 20.0, 1.0, 0.0))).unwrap();
        let expected = (10_000.0 * 10.0 + 90_000.0 * 1.0 + 10_000.0 * 20.0) / 1_000_000.0;
        assert!((cost - expected).abs() < 1e-12);
        // 若误把缓存按全价（旧 bug）算，成本会高得多——确认现在没有。
        let full_price_bug = (100_000.0 * 10.0 + 10_000.0 * 20.0) / 1_000_000.0;
        assert!(cost < full_price_bug * 0.5, "缓存折扣应显著低于全价");
    }

    #[test]
    fn unknown_model_has_no_cost() {
        assert!(calculate_cost("mystery", &Usage::default()).is_none());
    }
}
