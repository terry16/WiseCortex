//! Agent：一次用户消息 → 一轮回复。
//!
//! `Echo` 为占位/无配置回退（保持集成测试确定性）；`Llm` 调真实模型并流式发事件。
//! Phase 3 起会在此加入工具调用循环（推理→工具→结果→再推理）。
//!
//! 配置目前从环境变量读取（WC_PROVIDER / WC_API_KEY / WC_MODEL / WC_BASE_URL），
//! 与 `examples/chat` 一致；后续由 REST /api/config 接管。

use std::sync::Arc;

use serde_json::{json, Value};
use wisecortex_core::hooks;
use wisecortex_core::llm::{
    compressor, pricing, providers, ChatMessage, LlmClient, LlmRequest, ProviderConfig, RetryKind,
    Role, StreamUpdate, ThinkingMode, ToolCall, ToolDef, Usage, WireFormat,
};
use wisecortex_core::skill::SkillSet;
use wisecortex_core::tools::{split_image_result, ToolRegistry};

use crate::proto::{CacheStats, ServerEvent};
use crate::registry::{ImOrigin, SessionRegistry};

const SYSTEM_PROMPT: &str = "You are WiseCortex, a capable autonomous AI coding agent. \
Tools: read_file/glob/grep (explore), read_image (look at an image file on disk — use it instead of \
opening a viewer and screenshotting), write_file/edit_file (change), shell (build/test/install), \
web_fetch/web_search (internet), todo_write (plan), invoke_skill (reusable playbooks), \
task (delegate a self-contained subtask to a sub-agent). \
The working directory is the project root.\n\
\n\
Work like a senior engineer:\n\
1. EXPLORE before changing — but be SURGICAL. Use TARGETED glob/grep scoped by a specific \
   directory and a concrete filename/keyword to learn just the relevant parts and the build system. \
   Do NOT enumerate the whole tree: avoid blanket `**/*` or `**/<manifest>` scans — in a large repo \
   they flood the context with thousands of paths. Narrow by path and keyword, read only the files \
   you need (use read_file offset/limit for big ones). When a skill is active, follow its exploration \
   guidance over this generic step.\n\
2. PLAN multi-step work with todo_write and keep it updated.\n\
3. DELEGATE with `task` when a subtask is self-contained and you only need its conclusion — \
   broad codebase exploration, research across many files, or one independent slice of a bulk change. \
   The sub-agent has the full toolset but an isolated context, so its back-and-forth never pollutes \
   yours. When several subtasks are INDEPENDENT, emit MULTIPLE task calls in ONE reply — they run in \
   parallel and are far faster. Do the work yourself when it is small, needs your existing context, \
   or when each step depends on the previous one.\n\
4. SET UP THE ENVIRONMENT autonomously when needed — install dependencies, create config, or \
   scaffold projects via shell (npm/pnpm/yarn, cargo, pip/uv, go, etc.), choosing the tool that \
   matches the lockfile/manifest present. Don't ask the user to run setup you can run yourself.\n\
5. Make MINIMAL, correct changes that match the existing style; do not over-engineer or reformat unrelated code.\n\
6. VERIFY your work — after changes, build and run the relevant tests/linters via shell; read the \
   output, fix failures, and iterate until green. Don't claim success without checking.\n\
7. Prefer the dedicated tools (read_file/glob/grep/edit_file) over shell for file operations.\n\
8. If a task matches an available skill, invoke_skill it. If while running a skill you notice its \
   instructions are incomplete or wrong, you may improve that SKILL.md with edit_file (self-evolution).\n\
Be concise in prose; let tool calls and results do the work.";

/// 运行环境提示（让模型用对当前操作系统的命令），随 system prompt 一起发出。
fn os_hint() -> String {
    if cfg!(windows) {
        "Environment: Windows. The `shell` tool runs commands via cmd.exe — use Windows commands \
         (dir, where, type, findstr, copy, move, del) and Windows paths (backslashes, drive letters). \
         Do NOT use Unix commands (ls, which, cat, grep, tail) or POSIX paths like /home/...; \
         for file search/read/grep prefer the dedicated glob/read_file/grep tools."
            .to_string()
    } else {
        format!(
            "Environment: {}. The `shell` tool runs commands via sh.",
            std::env::consts::OS
        )
    }
}

/// 后台抽取：根据最近对话 + 已有记忆，让模型输出更新后的会话记忆并写回。
async fn extract_memory(
    client: &LlmClient,
    provider: &ProviderConfig,
    model: &str,
    sid: &str,
    history: &[ChatMessage],
) {
    let convo = render_recent_history(history);
    if convo.trim().is_empty() {
        return;
    }
    let existing = wisecortex_core::memory::read(sid);
    let existing_disp = if existing.trim().is_empty() {
        "(none)"
    } else {
        existing.trim()
    };
    let sys = format!("{MEMORY_EXTRACT_PROMPT}\n\nExisting memory:\n{existing_disp}");
    let mut req = LlmRequest::new(
        model.to_string(),
        vec![ChatMessage::system(sys), ChatMessage::user(convo)],
    );
    // 抽取是「整份重写」，输出被截断就会把记忆尾部永久毁掉。给够额度（记忆上限 16KB，
    // 1024 token 根本重写不完），下面还有一道防截断闸。
    req.max_tokens = 8192;
    let Ok(resp) = client.complete(provider, &req, |_| {}).await else {
        return;
    };
    let text = resp.content.unwrap_or_default();
    let text = text.trim();
    if text.is_empty() || text == "（空）" || text.eq_ignore_ascii_case("none") {
        return;
    }
    // 防截断闸：整份重写若明显比原记忆短一大截，多半是被 max_tokens 截断或模型偷懒漏抄，
    // 此时宁可这轮不更新——记忆丢了没法恢复，晚记一轮只是晚一轮。
    if text.len() * 2 < existing.trim().len() {
        wisecortex_core::buglog::record(
            "memory",
            &format!(
                "跳过一次记忆重写：新内容 {} 字节 < 原有 {} 字节的一半，疑似被截断",
                text.len(),
                existing.trim().len()
            ),
        );
        return;
    }
    if let Err(e) = wisecortex_core::memory::write(sid, text) {
        wisecortex_core::buglog::record("memory", &format!("写入会话记忆失败: {e}"));
    }
}

/// 把最近若干条历史渲染为紧凑文本（截断长内容、跳过空消息）。
fn render_recent_history(history: &[ChatMessage]) -> String {
    let start = history.len().saturating_sub(MEMORY_EXTRACT_RECENT);
    let mut out = String::new();
    for m in &history[start..] {
        let content = m.content.as_deref().unwrap_or("");
        if content.trim().is_empty() {
            continue;
        }
        let snip: String = content.chars().take(600).collect();
        out.push_str(&format!("[{}] {snip}\n", m.role.as_str()));
    }
    out
}

/// 无人值守任务（定时/IM）工具调用回合上限默认值（防失控）。可被配置 / env WC_MAX_ITERATIONS 覆盖。
/// 多文件生成 + 部署类技能往往很长，默认 50；设置面板 / 配置 / env 可再调高。
const DEFAULT_MAX_ITERATIONS: usize = 50;
/// 交互式聊天（人工会话）工具调用回合上限默认值——给很高的安全上限，开发基本不触及，
/// 同时对模型空转留兜底（用户在场可随时手动停）。可被配置 / env WC_MAX_ITERATIONS_INTERACTIVE 覆盖。
const DEFAULT_MAX_ITERATIONS_INTERACTIVE: usize = 1000;
/// 「纯空响应」（无文本、无思考、无工具调用）自动重试次数：上游偶发瞬时抖动会回一轮空响应，
/// 自动重试一次多能拿到正常结果；连续再空才停并给可见告警，避免静默消失也避免无限空转。
const MAX_EMPTY_RETRIES: u32 = 1;
/// 单次输出 token 上限（max_tokens）默认值。8192 太小：写大文件时工具参数会被截断成残缺 JSON，
/// 归一为 {} 后报「缺少必填参数」。给一个较大的默认；可被全局配置 / env WC_MAX_TOKENS / 模型档覆盖。
/// 注意：个别服务商会按模型真实上限拒绝过大的值(400)，此时在「模型管理」给该模型单独调低。
const DEFAULT_MAX_TOKENS: u32 = 32768;
/// 单次工具输出注入上下文的字节上限默认值（防一次读爬大量源码灌爆上下文）。0=不限。
/// 默认 60_000 字节（约 1.5 万~2 万 token），够正常单文件读取，又挡住整目录/整库式爬取。
const DEFAULT_TOOL_OUTPUT_LIMIT: usize = 60_000;

/// 截断过大的工具输出，避免一次工具调用把海量内容灌进上下文（`limit==0` 表示不限）。
/// 截断后追加提示，引导模型改用 grep / read_file(offset,limit) 精确获取所需片段（不丢能力）。
fn cap_tool_output(text: String, limit: usize) -> String {
    if limit == 0 || text.len() <= limit {
        return text;
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let omitted = text.len() - end;
    format!(
        "{}\n\n[⚠ 工具输出过大，已截断：仅显示前 {end} 字节，省略约 {omitted} 字节。\
         如需更多内容，请用 grep 按关键词检索，或用 read_file 的 offset/limit 读取指定片段，\
         不要一次性读取整个大文件或整个目录。]",
        &text[..end]
    )
}
/// 把多行文本压成单行片段，供执行日志逐行追加用：
/// 折叠所有空白为单空格（**务必去掉换行**，否则会破坏 `{id}.log` 一行一条的约定、令 UI 解析错位），
/// 再按字符数截断并加省略号。
fn log_snippet(s: &str, max: usize) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        let head: String = flat.chars().take(max).collect();
        format!("{head}…")
    } else {
        flat
    }
}

/// 子 agent（`task` 工具派生）回合上限默认值。可被配置 / env WC_SUBAGENT_MAX_ITERATIONS 覆盖。
///
/// 曾经硬编码 15，实测太低：审查改动、分析抓包这类活儿光 grep + 读几个文件就用光了，
/// 于是子任务几乎必然以「达到回合上限」收场。给 100——够跑完真活儿，又比主会话上限低一档，
/// 因为子任务常一次并行好几个，成本是乘出来的。
const DEFAULT_SUBAGENT_MAX_ITERATIONS: usize = 100;

/// 子任务步骤轨迹最多保留多少行（随结果回传给发起会话，防超长任务把结果撑爆）。
const SUBAGENT_TRAIL_MAX: usize = 200;

/// 子 agent 每一步明细的去处，两者都可为空（为空即完全静默，是旧的黑箱行为）。
/// `log_id`=定时任务的执行日志；`progress`=聊天派生子任务的发起会话。
#[derive(Clone, Copy, Default)]
struct SubagentSinks<'a> {
    log_id: Option<&'a str>,
    progress: Option<SubagentProgress<'a>>,
}

/// 子任务进度回传目标：把子 agent 的每一步广播回发起它的会话。
///
/// 没有它时子 agent 是纯黑箱——UI 只看得到「子任务开始」和「子任务结束」两条，
/// 中间在读什么文件、卡在哪一步全都看不见（`run_subagent` 原本连 registry 都拿不到）。
#[derive(Clone, Copy)]
struct SubagentProgress<'a> {
    reg: &'a SessionRegistry,
    sid: &'a str,
    desc: &'a str,
}

impl SubagentProgress<'_> {
    /// 实时推一条「当前在干什么」。进度行会被下一条覆盖，所以只作实况指示；
    /// 完整轨迹另由 [`RunOutcome::steps`] 随结果带回，落在该子任务自己的结果块里。
    fn publish(&self, line: &str) {
        self.reg.publish(
            self.sid,
            progress_active(self.sid, &format!("子任务·{}: {}", self.desc, line)),
        );
    }
}

/// 把子任务结果和它的步骤轨迹拼成展示文本：轨迹在前（默认折叠在结果块里），结论在后。
/// 没有轨迹（例如一轮就结束）时退化成纯结果，不制造空壳。
fn format_subagent_result(desc: &str, text: &str, steps: &[String]) -> String {
    if steps.is_empty() {
        return format!("[子任务·{desc}]\n{text}");
    }
    format!(
        "[子任务·{desc}]\n── 执行过程（{} 步）──\n{}\n── 结果 ──\n{text}",
        steps.len(),
        steps.join("\n")
    )
}

/// 一次性运行（定时 / IM 任务）的结果与用量统计。
pub struct RunOutcome {
    pub text: String,
    pub usage: Usage,
    pub cost: f64,
    pub priced: bool,
    pub requests: u64,
    /// 运行的终止原因（系统层面）：
    /// `ok`=模型正常给出最终答复；`max_iterations`=到回合上限被截断；`llm_error`=LLM 调用失败。
    /// 注意：`ok` 只表示**流程跑完**，不代表内容正确或模型没拒答（内容层面无法可靠自动判定）。
    pub status: &'static str,
    /// 子任务的步骤轨迹（调了哪些工具、每步结果摘要）。仅在传了 [`SubagentProgress`] 时收集，
    /// 供发起会话把「它到底干了什么」连同结果一起展示；定时任务路径不用（明细已进执行日志）。
    pub steps: Vec<String>,
}

/// 是否并行执行本轮工具调用：>1 个且全部是 task、且不在计划模式。
/// 混入其它工具时顺序执行（保证 fs 工具/确认顺序）；计划模式必须走 run_tool 的
/// 逐个拦截，否则子任务会绕过计划模式直接执行改动。
fn should_run_parallel(calls: &[ToolCall], plan_mode: bool) -> bool {
    !plan_mode && calls.len() > 1 && calls.iter().all(|tc| tc.name == "task")
}

/// `task` 工具定义（由 agent 主循环拦截执行，派生子 agent）。
fn task_tool_def() -> ToolDef {
    ToolDef {
        name: "task".to_string(),
        description: "Spawn a sub-agent to independently carry out a self-contained, decomposable \
                      subtask and return a summary of the result. The sub-agent has the full toolset \
                      but an isolated context, so its intermediate work never pollutes this conversation. \
                      Good fits: exploratory research, one slice of a bulk refactor, or any job that \
                      needs a lot of back-and-forth but where you only need the conclusion. \
                      **When several subtasks are independent of each other, emit multiple task calls \
                      in a SINGLE reply** — they run in parallel and are significantly faster. \
                      Only dispatch them one round at a time when they genuinely depend on each other."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "description": { "type": "string", "description": "Short one-line description of the subtask" },
                "prompt": { "type": "string", "description": "The complete instruction for the sub-agent — self-contained, including all context it needs" }
            },
            "required": ["description", "prompt"]
        }),
    }
}

/// 历史超过该 token 估算值时触发上下文压缩。
const COMPRESSION_THRESHOLD_TOKENS: usize = 60_000;
/// 压缩后保留的最近消息 token 预算（摘要 + 这段最近消息 = 新历史）。
/// 取得比阈值小不少，使压缩后还有余量、下次需再积累约 (阈值-此值) token 才重触发，避免抖动。
const KEEP_RECENT_TOKENS: usize = 20_000;
/// 待压缩前缀小于此值时跳过：摘要本身要花一次 LLM 调用，前缀太小不值得（也防抖动）。
const MIN_COMPRESS_PREFIX_TOKENS: usize = 10_000;
/// 历史中最多保留最近几条「带图消息」的图片，更早的物理剥除（GUI 截图是即时观察，
/// 旧图一张就兆级 base64，攒几十张请求体直接 413，摘要压缩救不了字节）。
const KEEP_RECENT_IMAGE_MESSAGES: usize = 3;

/// 当前回合使用的 agent 实现。
pub enum Agent {
    /// 回声占位（无 LLM 配置时回退；集成测试使用）。
    Echo,
    /// 真实 LLM。
    Llm(Box<LlmAgent>),
}

/// 一个已解析为可直接调用的 LLM 档（任务级模型覆盖按 id 选取）。
#[derive(Clone)]
struct ResolvedLlm {
    provider: ProviderConfig,
    model: String,
    price: Option<PriceOverride>,
    /// 该模型单次输出 token 上限（档内 max_tokens 覆盖全局，已解析为最终值）。
    max_tokens: u32,
    /// 该模型档内的推理强度档位（覆盖全局；None=该模型未单独设置）。
    reasoning_effort: Option<String>,
}

/// 推理强度优先级：模型档内显式设置（含 `Some("")`=关闭）优先于全局默认。
/// `per_model` 为 None 表示该模型没单独设，回落全局。
fn pick_effort(per_model: Option<String>, global: Option<String>) -> Option<String> {
    per_model.or(global)
}

/// 自定义费率覆盖 `(input, output, cache_read, cache_write)`，元/百万 token。
type PriceOverride = (f64, f64, f64, f64);

/// 从配置档的价格字段构造覆盖费率：两档（输入/输出）齐全才生效；
/// 缓存读价未配则按 **输入价 × 0.1** 估算（多数模型缓存读约为输入的 1/10），
/// 避免高缓存场景按全价虚高；缓存写默认 0（OpenAI 兼容无写入溢价）。
fn price_override(
    price_in: Option<f64>,
    price_out: Option<f64>,
    price_cache_read: Option<f64>,
) -> Option<PriceOverride> {
    match (price_in, price_out) {
        (Some(i), Some(o)) => Some((i, o, price_cache_read.unwrap_or(i * 0.1), 0.0)),
        _ => None,
    }
}

/// 把一个配置档解析为可调用的 (provider, model, price, max_tokens)；无 key 或缺 base_url/model 时返回 None。
/// `global_max_tokens` 为全局默认；档内 `max_tokens` 覆盖之。
fn resolve_profile(
    p: &wisecortex_core::config::LlmProfile,
    global_max_tokens: u32,
) -> Option<ResolvedLlm> {
    let oauth = p.claude_oauth == Some(true);
    let codex = p.openai_codex == Some(true);
    let grok = p.xai_grok == Some(true);
    let gemini = p.gemini_oauth == Some(true);
    let key = p.api_key.clone().unwrap_or_default();
    // OAuth/订阅 档不需要 key；其余档无 key 不可用。
    if !oauth && !codex && !grok && !gemini && key.is_empty() {
        return None;
    }
    let preset = providers::get(p.provider.as_deref().unwrap_or("deepseek"));
    // 订阅档固定走各自后端/线缆；其余按 provider 预设/自定义解析。
    // 订阅端点 base 支持 WC_PROXY_* 环境变量覆盖（Cloudflare Worker 等反代），见各 oauth 模块。
    let (base_url, model, format, thinking) = if codex {
        let model = p.model.clone().unwrap_or_else(|| "gpt-5-codex".to_string());
        (
            wisecortex_core::llm::oauth_openai::codex_base_url(),
            model,
            WireFormat::OpenAiResponses,
            ThinkingMode::default(),
        )
    } else if oauth {
        let model = p
            .model
            .clone()
            .unwrap_or_else(|| "claude-sonnet-4-6".to_string());
        (
            wisecortex_core::llm::oauth::api_base(),
            model,
            WireFormat::Anthropic,
            ThinkingMode::default(),
        )
    } else if grok {
        let model = p
            .model
            .clone()
            .unwrap_or_else(|| wisecortex_core::llm::oauth_xai::DEFAULT_MODEL.to_string());
        (
            wisecortex_core::llm::oauth_xai::api_base(),
            model,
            WireFormat::OpenAi,
            // xAI 只有 grok-3-mini 认 reasoning_effort，grok-4 系发了会 400 → 不发。
            ThinkingMode::None,
        )
    } else if gemini {
        let model = p
            .model
            .clone()
            .unwrap_or_else(|| wisecortex_core::llm::oauth_gemini::DEFAULT_MODEL.to_string());
        (
            wisecortex_core::llm::oauth_gemini::api_base(),
            model,
            WireFormat::Gemini,
            // Gemini 线缆按 req.reasoning_effort 自行处理思考（thinkingConfig），ThinkingMode 不参与。
            ThinkingMode::None,
        )
    } else {
        let base_url = p
            .base_url
            .clone()
            .or_else(|| preset.map(|x| x.base_url.to_string()))?;
        let model = p
            .model
            .clone()
            .or_else(|| preset.map(|x| x.default_model.to_string()))?;
        let format = preset.map(|x| x.format).unwrap_or(WireFormat::OpenAi);
        let thinking = preset.map(|x| x.thinking).unwrap_or_default();
        (base_url, model, format, thinking)
    };
    let price = price_override(p.price_in, p.price_out, p.price_cache_read);
    // 订阅档（Claude/Codex）模型已知支持图片；Grok 订阅默认吃图但可档内显式关（grok-code-fast
    // 纯文本）；其余优先用档内显式 vision，未指定才回落 provider 目录白名单（providers.json 即时生效）。
    let vision = if oauth || codex {
        true
    } else if grok || gemini {
        // Grok-4 系 / Gemini-2.5 系默认多模态，可档内显式关。
        p.vision.unwrap_or(true)
    } else {
        p.vision.unwrap_or_else(|| {
            providers::supports_vision(p.provider.as_deref().unwrap_or("deepseek"), &model)
        })
    };
    Some(ResolvedLlm {
        provider: ProviderConfig::new(base_url, key, format)
            .with_thinking(thinking)
            .with_claude_oauth(oauth)
            .with_openai_codex(codex)
            .with_xai_grok(grok)
            .with_gemini_oauth(gemini)
            .with_vision(vision),
        model,
        price,
        max_tokens: p.max_tokens.unwrap_or(global_max_tokens).max(256),
        reasoning_effort: p.reasoning_effort.clone(),
    })
}

/// 是否因达到 max_tokens 被截断（OpenAI 用 "length"；Anthropic 的 "max_tokens" 已归一为 "length"）。
fn truncated_by_length(finish_reason: &Option<String>) -> bool {
    finish_reason.as_deref() == Some("length")
}

pub struct LlmAgent {
    client: LlmClient,
    provider: ProviderConfig,
    model: String,
    system_prompt: String,
    /// 不含技能目录的基础系统提示；任务钉选技能时据此重建带过滤目录的提示。
    system_prompt_base: String,
    /// 已配置的全部 LLM 档（id → 已解析）；任务级模型覆盖时按 id 选取。
    llms: std::collections::HashMap<String, ResolvedLlm>,
    tools: Arc<ToolRegistry>,
    /// 默认工作目录（按 cwd/knowledge 重建工具表时作基准）。
    workdir: std::path::PathBuf,
    /// true=自动批准危险操作（无人值守）；false=执行前征求确认。
    auto_approve: bool,
    /// 自定义价格 (输入, 输出, 缓存读, 缓存写) 元/百万 token；存在则覆盖内置费率表。
    price: Option<PriceOverride>,
    /// 自动记忆：开启后每隔若干轮后台抽取会话要点写入记忆。
    auto_memory: bool,
    /// 自动优化上下文：图片只发一次（历史里的旧图不随后续每轮重发，压缩时一并丢弃）。
    auto_trim_context: bool,
    /// 全局默认推理强度 / extended thinking（low/medium/high/xhigh/max）；None=关闭。
    reasoning_effort: Option<String>,
    /// 默认（active）模型档内的推理强度档位（覆盖全局；None=该模型未单独设，用全局）。
    /// 任务级模型覆盖时改用被覆盖模型自己的档内值。
    default_reasoning_effort: Option<String>,
    /// 无人值守任务（定时/IM）工具调用回合上限（防失控）。
    max_iterations: usize,
    /// 交互式聊天主循环的工具调用回合上限（远高于 max_iterations；无人值守任务仍用 max_iterations）。
    max_iterations_interactive: usize,
    /// `task` 工具派生的子 agent 的回合上限（独立一项，见 [`DEFAULT_SUBAGENT_MAX_ITERATIONS`]）。
    subagent_max_iterations: usize,
    /// 默认（active）模型单次输出 token 上限；任务级模型覆盖时用各档自己的 max_tokens。
    max_tokens: u32,
    /// 单次工具输出注入上下文的字节上限（0=不限）。
    tool_output_limit: usize,
    /// 各会话上次自动抽取时的历史长度（节流，避免每轮都抽）。
    extract_marks: Arc<std::sync::Mutex<std::collections::HashMap<String, usize>>>,
}

/// 无人值守路径（定时/IM/后台/子任务）的会话记忆占位 sid。
/// 这些上下文没有交互会话，它们该用的是项目级记忆；这个 sid 只是给 `scope=session`
/// 一个不至于报错的落点，不指望它承载什么。
const UNATTENDED_SID: &str = "unattended";

/// 自动记忆触发步长：历史每增长这么多条消息才抽取一次（约 3 轮）。
const MEMORY_EXTRACT_STEP: usize = 6;
/// 自动记忆抽取时喂给模型的最近消息条数。
const MEMORY_EXTRACT_RECENT: usize = 24;
/// 自动记忆抽取的系统提示。
///
/// **必须显式点名「纠正/踩过的坑」并保护它们**：早先的版本只列了 decisions/constraints/
/// progress/preferences，还交代 "do not record transient state"——用户一句「别再这么干」
/// 最容易被归进 transient 而丢掉，于是价值最高的记忆恰好被过滤没了，用户只能反复提醒。
/// 「过时就删」同样危险：抽取只看得到最近 24 条，更早的有效教训会被误判成过时。
const MEMORY_EXTRACT_PROMPT: &str = "You maintain the memory. Given the recent conversation and the \
    existing memory below, output the updated COMPLETE memory in this exact shape (omit a section \
    that would be empty):\n\
    ## Lessons (do not repeat)\n\
    - one line per mistake you made or correction the user gave you\n\
    ## Notes\n\
    - one line per decision, constraint, progress point, or user preference\n\
    \n\
    Rules: NEVER drop or reword an existing Lessons entry — those are corrections the user already had to \
    give once, and losing one makes them repeat it. Add a new Lessons entry whenever the user corrects you or \
    something you did turned out wrong; phrase it as the rule to follow next time. You may merge exact \
    duplicates and drop entries under Notes that are clearly superseded, but when unsure, keep it. \
    Keep every entry under ~130 characters — the conclusion, not the reasoning. Do not record \
    transient state or anything inferable from the code or the history. Output the memory content \
    itself with no explanation, preamble, or trailing remarks. If there is nothing worth recording, \
    reply with exactly: none";

/// solo（无人值守）模式注入的系统提示：自主推进、尽量不向用户提问。
const SOLO_HINT: &str = "SOLO / UNATTENDED MODE: you are expected to complete this task on your own — \
    avoid asking the user clarifying questions. When information is missing, make your own assumptions \
    from the context and sensible defaults and keep going (just state the key assumptions in one line \
    in your reply). Stop and ask ONLY when the task is genuinely finished, or when you hit a critical \
    fork that you cannot decide yourself and that could cause destructive or irreversible consequences. \
    Do not interrupt to confirm details, request permission, or report intermediate thoughts — \
    get the thing done first.";

/// 计划模式注入的系统提示：只读探索 + 产出计划，禁止改动。
const PLAN_HINT: &str = "PLAN MODE: this turn is for research and planning only — \
    **no changes of any kind are permitted**. Do not write or edit files, do not run shell commands \
    that mutate the environment or have side effects, do not call mutating MCP tools, and do not start \
    background tasks. First use the read-only tools (read_file/glob/grep/web_search, etc.) to understand \
    the current state, then output a **concrete, actionable step-by-step plan** (what each step does, \
    which files it touches, how to verify it), and finally ask the user to confirm. Only after the user \
    leaves plan mode and tells you to proceed do you actually make changes, in a later turn. \
    Mutating tools will be blocked if you call them here.";

/// 等待用户确认的超时（秒）。超时按拒绝处理。
const CONFIRMATION_TIMEOUT_SECS: u64 = 300;

impl Agent {
    /// 解析配置并构造。优先级：环境变量 WC_* > 配置文件 > provider 预设默认。
    /// 未配置 api_key 时回退为 Echo。
    pub fn configure() -> Agent {
        let file = wisecortex_core::config::load();
        // 当前 LLM 档（多档中 active_llm 指向的；无则第一个）。
        let active = file.active().cloned().unwrap_or_default();
        let env = |k: &str| std::env::var(k).ok();

        // 订阅档：用本机 OAuth token，不需要 api_key，固定走各自后端/线缆。
        let use_claude_oauth = active.claude_oauth == Some(true);
        let use_openai_codex = active.openai_codex == Some(true);
        let use_xai_grok = active.xai_grok == Some(true);
        let use_gemini_oauth = active.gemini_oauth == Some(true);
        let key = env("WC_API_KEY").or(active.api_key).unwrap_or_default();
        if key.is_empty()
            && !use_claude_oauth
            && !use_openai_codex
            && !use_xai_grok
            && !use_gemini_oauth
        {
            return Agent::Echo;
        }
        let provider_id = env("WC_PROVIDER")
            .or(active.provider)
            .unwrap_or_else(|| "deepseek".to_string());
        let preset = providers::get(&provider_id);
        let base_url = env("WC_BASE_URL")
            .or(active.base_url)
            .or_else(|| preset.map(|p| p.base_url.to_string()));
        let model = env("WC_MODEL")
            .or(active.model)
            .or_else(|| preset.map(|p| p.default_model.to_string()));
        // Grok 订阅走 OpenAi 线缆但不发 reasoning_effort（grok-4 系会打回）。
        let thinking = if use_xai_grok {
            ThinkingMode::None
        } else {
            preset.map(|p| p.thinking).unwrap_or_default()
        };
        // 订阅档强制各自线缆 + 端点；模型缺省给个合理默认。
        let format = if use_openai_codex {
            WireFormat::OpenAiResponses
        } else if use_claude_oauth {
            WireFormat::Anthropic
        } else if use_gemini_oauth {
            WireFormat::Gemini
        } else {
            preset.map(|p| p.format).unwrap_or(WireFormat::OpenAi)
        };
        let base_url = if use_openai_codex {
            Some(wisecortex_core::llm::oauth_openai::codex_base_url())
        } else if use_claude_oauth {
            Some(wisecortex_core::llm::oauth::api_base())
        } else if use_xai_grok {
            Some(wisecortex_core::llm::oauth_xai::api_base())
        } else if use_gemini_oauth {
            Some(wisecortex_core::llm::oauth_gemini::api_base())
        } else {
            base_url
        };
        let model = model
            .or_else(|| use_openai_codex.then(|| "gpt-5-codex".to_string()))
            .or_else(|| use_claude_oauth.then(|| "claude-sonnet-4-6".to_string()))
            .or_else(|| {
                use_xai_grok.then(|| wisecortex_core::llm::oauth_xai::DEFAULT_MODEL.to_string())
            })
            .or_else(|| {
                use_gemini_oauth
                    .then(|| wisecortex_core::llm::oauth_gemini::DEFAULT_MODEL.to_string())
            });

        // auto_approve：环境变量 WC_AUTO_APPROVE > 配置文件 > 默认 true（无人值守友好）。
        let auto_approve = env("WC_AUTO_APPROVE")
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .or(file.auto_approve)
            .unwrap_or(true);
        // auto_memory：环境变量 WC_AUTO_MEMORY > 配置文件 > 默认 **true**。
        // 曾默认关闭，于是记忆全靠模型自觉调 remember，漏记就得用户重新交代一遍。
        // 代价是每约 3 轮多一次小的 LLM 调用（只喂最近 24 条），换不用反复提醒，值。
        let auto_memory = env("WC_AUTO_MEMORY")
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .or(file.auto_memory)
            .unwrap_or(true);
        // auto_trim_context：环境变量 WC_AUTO_TRIM_CONTEXT > 配置文件 > 默认开启。
        // 图片每轮重发是上下文里最大的一笔浪费，默认就该省掉；需要反复比对同一张图时再关。
        let auto_trim_context = env("WC_AUTO_TRIM_CONTEXT")
            .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .or(file.auto_trim_context)
            .unwrap_or(true);
        // 推理强度 / extended thinking：环境变量 WC_REASONING_EFFORT > 配置文件；空白视为关闭。
        let reasoning_effort = env("WC_REASONING_EFFORT")
            .or(file.reasoning_effort.clone())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        // 无人值守任务回合上限：env WC_MAX_ITERATIONS > 配置文件 > 默认；至少 1。
        let max_iterations = env("WC_MAX_ITERATIONS")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .or(file.max_iterations)
            .unwrap_or(DEFAULT_MAX_ITERATIONS)
            .max(1);
        // 交互式聊天回合上限：env WC_MAX_ITERATIONS_INTERACTIVE > 配置文件 > 默认（很高）；至少 1。
        let max_iterations_interactive = env("WC_MAX_ITERATIONS_INTERACTIVE")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .or(file.max_iterations_interactive)
            .unwrap_or(DEFAULT_MAX_ITERATIONS_INTERACTIVE)
            .max(1);
        // 子 agent 回合上限：env WC_SUBAGENT_MAX_ITERATIONS > 配置文件 > 默认 100；至少 1。
        let subagent_max_iterations = env("WC_SUBAGENT_MAX_ITERATIONS")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .or(file.subagent_max_iterations)
            .unwrap_or(DEFAULT_SUBAGENT_MAX_ITERATIONS)
            .max(1);
        // 工具输出字节上限：env WC_TOOL_OUTPUT_LIMIT > 配置文件 > 默认（0=不限）。
        let tool_output_limit = env("WC_TOOL_OUTPUT_LIMIT")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .or(file.tool_output_limit)
            .unwrap_or(DEFAULT_TOOL_OUTPUT_LIMIT);
        // 输出 token 上限：env WC_MAX_TOKENS > 配置文件 > 默认 32768；至少 256。模型档可单独覆盖。
        let global_max_tokens = env("WC_MAX_TOKENS")
            .and_then(|v| v.trim().parse::<u32>().ok())
            .or(file.max_tokens)
            .unwrap_or(DEFAULT_MAX_TOKENS)
            .max(256);
        // 默认（active）模型的输出上限：档内 max_tokens 覆盖全局。
        let max_tokens = active.max_tokens.unwrap_or(global_max_tokens).max(256);

        // 默认工作目录 = 全局工作目录（AI 脚本/知识库/自我学习的落脚点）；
        // 缺失时回退到进程当前目录。每次对话可在请求里用 cwd 覆盖（tools_for）。
        let workdir = file
            .workspace_dir()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| ".".into());
        let _ = std::fs::create_dir_all(&workdir); // best-effort 建好工作目录

        // 技能：项目级 ./skills 与用户级数据目录两处加载。
        let mut skill_dirs = vec![workdir.join("skills")];
        if let Some(d) = wisecortex_core::config::skills_dir() {
            skill_dirs.push(d);
        }
        let skills_prompt = SkillSet::load_dirs(&skill_dirs)
            .without_disabled(&wisecortex_core::skills_state::disabled())
            .prompt_list();
        const MEMORY_HINT: &str = "MEMORY: use the remember tool for anything worth keeping long-term. \
            Above all, whenever the user corrects you or something you did turns out to be wrong, record \
            it immediately with kind=lesson, phrased as the rule to follow next time — being made to give \
            the same correction twice is the most expensive thing you can do to a user. Also record \
            decisions, constraints, progress, and preferences (kind=fact). Default scope=project, so it \
            carries over to new conversations in this directory. Keep each entry to one short conclusion. \
            Record only durable, non-obvious things that cannot be inferred from the code or history. \
            What you record is injected back on every later turn, surviving context compaction.";
        // 基础系统提示（不含技能目录）；任务钉选技能时据此重建带过滤目录的提示。
        let system_prompt_base = format!("{SYSTEM_PROMPT}\n\n{}\n\n{MEMORY_HINT}", os_hint());
        let system_prompt = if skills_prompt.is_empty() {
            system_prompt_base.clone()
        } else {
            format!("{system_prompt_base}\n\n{skills_prompt}")
        };

        // 解析所有已配置 LLM 档为 (provider, model, price)，供任务级模型覆盖时按 id 选取。
        let llms: std::collections::HashMap<String, ResolvedLlm> = file
            .llms
            .iter()
            .filter_map(|p| resolve_profile(p, global_max_tokens).map(|r| (p.id.clone(), r)))
            .collect();

        // 自定义价格（两档齐全才生效；缓存读价未配按输入价 ×0.1 估算）。
        let price = price_override(active.price_in, active.price_out, active.price_cache_read);

        match (base_url, model) {
            (Some(base_url), Some(model)) => {
                // 默认档是否吃图：订阅档已知支持（Grok 档可显式关）；其余档内显式优先，未指定回落目录白名单。
                let vision = use_claude_oauth
                    || use_openai_codex
                    || ((use_xai_grok || use_gemini_oauth) && active.vision.unwrap_or(true))
                    || active
                        .vision
                        .unwrap_or_else(|| providers::supports_vision(&provider_id, &model));
                Agent::Llm(Box::new(LlmAgent {
                    client: LlmClient::new(),
                    provider: ProviderConfig::new(base_url, key, format)
                        .with_thinking(thinking)
                        .with_claude_oauth(use_claude_oauth)
                        .with_openai_codex(use_openai_codex)
                        .with_xai_grok(use_xai_grok)
                        .with_gemini_oauth(use_gemini_oauth)
                        .with_vision(vision),
                    model,
                    system_prompt,
                    system_prompt_base,
                    llms,
                    tools: Arc::new(
                        ToolRegistry::with_defaults(workdir.clone())
                            .with_skills(skill_dirs.clone())
                            // 无人值守路径（定时/IM/后台/子任务）也要能写记忆：这些上下文同样
                            // 继承了「用 remember 记下来」的系统提示，工具表里却一直没有它，
                            // 模型只能幻觉调用或默默丢掉。sid 用固定占位——它们没有交互会话，
                            // 真正有价值的是默认的项目级记忆（不依赖 sid）。
                            .with_memory(UNATTENDED_SID, workdir.clone())
                            .with_local_gui(),
                    ),
                    workdir,
                    auto_approve,
                    price,
                    auto_memory,
                    auto_trim_context,
                    reasoning_effort,
                    default_reasoning_effort: active.reasoning_effort.clone(),
                    max_iterations,
                    max_iterations_interactive,
                    subagent_max_iterations,
                    max_tokens,
                    tool_output_limit,
                    extract_marks: Arc::new(
                        std::sync::Mutex::new(std::collections::HashMap::new()),
                    ),
                }))
            }
            _ => Agent::Echo,
        }
    }

    /// 一次性执行一个 prompt 并返回结果文本（IM 触发用；隔离、无会话副作用）。
    ///
    /// 用**无人值守**那一档回合上限（`max_iterations`）——短触发、无人盯着，低上限防失控成本。
    /// 后台长任务请用 [`run_background`](Self::run_background)，那是另一档。
    pub async fn run_once(&self, prompt: &str) -> String {
        self.run_once_capped(prompt, |a| a.max_iterations).await
    }

    /// 后台长任务（`task_start`）：用**交互式**那一档回合上限（`max_iterations_interactive`）。
    ///
    /// 它的定位就是「耗时长、可放着跑的活：大型重构、批量处理、长调研」，
    /// 跟 IM/定时那种短触发不是一回事。之前它和定时任务共用无人值守的低上限，
    /// 结果是专为长任务准备的工具反而预算最紧，且想调高就会连带把定时任务一起调高。
    pub async fn run_background(&self, prompt: &str) -> String {
        self.run_once_capped(prompt, |a| a.max_iterations_interactive)
            .await
    }

    /// [`run_once`] / [`run_background`] 的共同实现，回合上限由调用方选档。
    async fn run_once_capped(&self, prompt: &str, pick: fn(&LlmAgent) -> usize) -> String {
        match self {
            Agent::Echo => format!("echo: {prompt}"),
            Agent::Llm(a) => {
                let sp = a.with_project_memory(&a.system_prompt, &a.workdir);
                a.run_subagent_with(
                    prompt,
                    &sp,
                    &a.tools,
                    pick(a),
                    None,
                    SubagentSinks::default(),
                )
                .await
                .text
            }
        }
    }

    /// 在指定工作目录下一次性执行（定时任务可隔离到某项目目录）。`workdir=None` 等同 [`run_once`]。
    /// 指定 workdir 时：工具 cwd 指向它，且加载其 `<workdir>/skills` 项目级技能（不污染全局）。
    /// `log_id` 为 Some 时把每一步明细写进该任务的执行日志（定时任务传任务 id）。
    /// `model_id` 为 Some 时用该 LLM 档覆盖全局默认模型（定时任务可指定专用模型）。
    pub async fn run_once_in(
        &self,
        prompt: &str,
        workdir: Option<&std::path::Path>,
        log_id: Option<&str>,
        model_id: Option<&str>,
    ) -> RunOutcome {
        match (self, workdir) {
            (Agent::Echo, _) => RunOutcome {
                text: format!("echo: {prompt}"),
                usage: Usage::default(),
                cost: 0.0,
                priced: false,
                requests: 0,
                status: "ok",
                steps: Vec::new(),
            },
            (Agent::Llm(a), Some(dir)) => {
                let (system_prompt, tools) = a.rebuilt_for_workdir(dir);
                a.run_subagent_with(
                    prompt,
                    &system_prompt,
                    &tools,
                    a.max_iterations,
                    model_id,
                    SubagentSinks {
                        log_id,
                        progress: None,
                    },
                )
                .await
            }
            (Agent::Llm(a), None) => {
                a.run_subagent_with(
                    prompt,
                    &a.system_prompt,
                    &a.tools,
                    a.max_iterations,
                    model_id,
                    SubagentSinks {
                        log_id,
                        progress: None,
                    },
                )
                .await
            }
        }
    }

    /// 人类可读的模式描述（启动日志用）。
    pub fn describe(&self) -> String {
        match self {
            Agent::Echo => "echo (未配置 LLM)".to_string(),
            Agent::Llm(a) => format!("llm model={} format={:?}", a.model, a.provider.format),
        }
    }

    /// 跑一轮：追加用户消息 → working → 回复 → idle。
    /// `origin`=消息来自哪个 IM 聊天（Web/WS 传 None）；仅 `/switch` 命令需要，用来知道
    /// 该把「当前会话」指针重绑到哪个聊天上。
    /// 返回值：命令产生的 **IM 互动卡片**（如 `/sessions` 的按钮卡片）。Some 时 IM 侧改发卡片、
    /// 不发文本；普通 LLM 回合与 Web 侧一律 None。
    #[allow(clippy::too_many_arguments)]
    pub async fn run_turn(
        &self,
        reg: &SessionRegistry,
        sid: &str,
        user_content: String,
        images: Vec<String>,
        files: Vec<Value>,
        cwd: Option<String>,
        origin: Option<ImOrigin>,
    ) -> Option<Value> {
        // 每会话回合串行锁：整轮（含 user 消息入历史 + 工具循环的全部 append）独占该会话，
        // 杜绝并发回合交错写历史——交错会让 tool_use 与其 tool_result 错位，被上游 400 焊死会话。
        // 持锁至本函数结束（RAII）；不同会话各持各锁，互不阻塞。
        let _turn_guard = reg.session_lock(sid).lock_owned().await;

        // 聊天命令（/setworkdir、/model 等）：服务端直接执行并回复，不进 LLM。
        // 放在 run_turn 入口，Web / 飞书等全渠道统一生效（历史写入由命令模块负责）。
        if let Some(out) =
            crate::commands::try_execute(reg, sid, user_content.trim(), origin.as_ref())
        {
            reg.set_status(sid, "working");
            reg.publish(
                sid,
                ServerEvent::AssistantMessage {
                    session_id: sid.to_string(),
                    content: out.text,
                    files: vec![],
                },
            );
            reg.publish(
                sid,
                ServerEvent::Complete {
                    session_id: sid.to_string(),
                    iterations: 0,
                    cost: 0.0,
                    duration: None,
                    cache_stats: None,
                    awaiting_user_feedback: None,
                    cost_source: None,
                },
            );
            reg.set_status(sid, "idle");
            return out.card;
        }

        // 知识库随会话绑定（持久化），从注册表取，不再随消息传。
        let knowledge = reg.knowledge(sid);
        // 工作目录：本条消息显式传的优先；否则用任务绑定的 working_dir（创建任务时确定）。
        let cwd = cwd.or_else(|| {
            reg.task_config(sid)
                .working_dir
                .filter(|w| !w.trim().is_empty())
        });
        // 非图片附件（PDF/文档等）落盘到工作目录的 uploads/，并在消息里附上路径，供 agent 用工具处理。
        let mut user_content = self.save_uploads(cwd.as_deref(), files, user_content);

        // SessionStart 钩子（任务首条消息时）：副作用，如初始化环境。
        if reg.history(sid).is_empty() {
            let _ = fire_hook(
                hooks::event::SESSION_START,
                None,
                json!({"event":"SessionStart","session_id":sid}),
            )
            .await;
        }
        // UserPromptSubmit 钩子：可拦截本回合，或给模型追加上下文。
        let ph = fire_hook(
            hooks::event::USER_PROMPT_SUBMIT,
            None,
            json!({"event":"UserPromptSubmit","session_id":sid,"prompt":user_content}),
        )
        .await;
        let blocked = if ph.block {
            Some(ph.reason.unwrap_or_else(|| "(无原因)".to_string()))
        } else {
            if let Some(ctx) = ph.additional_context {
                user_content.push_str(&format!("\n\n[hook]\n{ctx}"));
            }
            None
        };

        // 首条消息作会话标题（set_name_if_empty 会把 session_renamed 发到全局侧栏 feed）。
        let title = derive_title(&user_content);
        reg.set_name_if_empty(sid, &title);

        // 用户上传的图同样先体检再入库：坏图入库即等于给这个会话下毒，之后每轮都 400。
        // 当场告诉用户哪张不行，好过让他隔一会儿撞上一句莫名其妙的「尺寸超过 8000 像素」。
        let mut msg = [ChatMessage::user_with_images(user_content.clone(), images)];
        let cleaned = wisecortex_core::llm::image::sanitize(&mut msg);
        if !cleaned.is_empty() {
            wisecortex_core::buglog::record(
                "llm",
                &format!(
                    "session={sid} 用户上传的图有 {} 张模型解不了，未入库：{}",
                    cleaned.removed,
                    cleaned.reasons.join("; ")
                ),
            );
            reg.publish(
                sid,
                ServerEvent::Warning {
                    session_id: sid.to_string(),
                    message: format!(
                        "有 {} 张图片模型无法解码（{}），已忽略，其余内容照常处理。",
                        cleaned.removed,
                        cleaned.reasons.join("；")
                    ),
                },
            );
        }
        let [msg] = msg;
        reg.append_message(sid, msg);
        // set_status 会把状态变化发到全局侧栏 feed（所有连接含当前对话都会收到），无需再 per-session 重发。
        reg.set_status(sid, "working");

        // 被 UserPromptSubmit 拦截：回一条说明并结束本回合，不跑 agent。
        if let Some(reason) = blocked {
            let msg = format!("本次请求被 hook 拦截：{reason}");
            reg.append_message(sid, ChatMessage::assistant(msg.clone()));
            reg.publish(
                sid,
                ServerEvent::AssistantMessage {
                    session_id: sid.to_string(),
                    content: msg,
                    files: vec![],
                },
            );
            reg.publish(
                sid,
                ServerEvent::Complete {
                    session_id: sid.to_string(),
                    iterations: 0,
                    cost: 0.0,
                    duration: None,
                    cache_stats: None,
                    awaiting_user_feedback: None,
                    cost_source: None,
                },
            );
            reg.set_status(sid, "idle");
            return None; // 被 hook 拦下的回合，也不产生卡片
        }

        match self {
            Agent::Echo => {
                let reply = format!("echo: {user_content}");
                reg.append_message(sid, ChatMessage::assistant(reply.clone()));
                reg.publish(
                    sid,
                    ServerEvent::AssistantMessage {
                        session_id: sid.to_string(),
                        content: reply,
                        files: vec![],
                    },
                );
                reg.publish(
                    sid,
                    ServerEvent::Complete {
                        session_id: sid.to_string(),
                        iterations: 1,
                        cost: 0.0,
                        duration: None,
                        cache_stats: None,
                        awaiting_user_feedback: None,
                        cost_source: None,
                    },
                );
            }
            Agent::Llm(a) => {
                a.run(reg, sid, cwd.as_deref(), &knowledge).await;
                a.maybe_extract_memory(reg, sid); // 后台自动记忆（按需、不阻塞）
            }
        }

        // Stop 钩子（回合结束）：副作用，如发通知。
        let _ = fire_hook(
            hooks::event::STOP,
            None,
            json!({"event":"Stop","session_id":sid}),
        )
        .await;
        reg.set_status(sid, "idle");
        None // 普通 LLM 回合不产生卡片
    }

    /// 重试上一轮：对**现有历史**重跑 agent 循环，不追加新的用户消息（LLM 失败后用）。
    /// 与 run_turn 共享每会话串行锁与「working/idle + Stop 钩子」收尾；仅省去用户消息入历史、
    /// 上传落盘、标题、UserPromptSubmit 钩子。
    pub async fn run_retry(&self, reg: &SessionRegistry, sid: &str, cwd: Option<String>) {
        let _turn_guard = reg.session_lock(sid).lock_owned().await;
        let knowledge = reg.knowledge(sid);
        let cwd = cwd.or_else(|| {
            reg.task_config(sid)
                .working_dir
                .filter(|w| !w.trim().is_empty())
        });
        reg.set_status(sid, "working");
        if let Agent::Llm(a) = self {
            a.run(reg, sid, cwd.as_deref(), &knowledge).await;
            a.maybe_extract_memory(reg, sid);
        }
        let _ = fire_hook(
            hooks::event::STOP,
            None,
            json!({"event":"Stop","session_id":sid}),
        )
        .await;
        reg.set_status(sid, "idle");
    }

    /// 把非图片附件（data URL）写入工作目录的 `uploads/`，并在用户消息末尾附上相对路径清单，
    /// 让 agent 能用 read_file / shell 等工具处理（图片仍走 vision，不在此列）。
    fn save_uploads(&self, cwd: Option<&str>, files: Vec<Value>, user_content: String) -> String {
        if files.is_empty() {
            return user_content;
        }
        // 落点：本轮有效工作目录（cwd 优先），否则 Llm 的 workdir，再否则进程目录。
        let base = cwd
            .map(str::trim)
            .filter(|c| !c.is_empty() && std::path::Path::new(c).is_dir())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| match self {
                Agent::Llm(a) => a.workdir.clone(),
                Agent::Echo => std::env::current_dir().unwrap_or_else(|_| ".".into()),
            });
        attach_uploads(&base, files, user_content)
    }
}

/// 附件落盘到 `base/uploads/` 并在消息末尾附上相对路径清单（`Agent` 与 `LlmAgent`
/// 两处都要用：前者是回合起手的用户消息，后者是回合进行中插进来的消息）。
fn attach_uploads(base: &std::path::Path, files: Vec<Value>, user_content: String) -> String {
    if files.is_empty() {
        return user_content;
    }
    let dir = base.join("uploads");
    let mut saved: Vec<String> = Vec::new();
    for f in &files {
        let name = f.get("name").and_then(Value::as_str).unwrap_or("upload");
        let data_url = f
            .get("data_url")
            .or_else(|| f.get("dataUrl"))
            .and_then(Value::as_str);
        let Some(data_url) = data_url else { continue };
        if let Ok(p) = wisecortex_core::upload::save_data_url(&dir, name, data_url) {
            let shown = p
                .strip_prefix(base)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            saved.push(shown);
        }
    }
    if saved.is_empty() {
        return user_content;
    }
    format!(
        "{user_content}\n\n（已上传文件，存于工作目录：{}。需要时用 read_file / shell 处理。）",
        saved.join("、")
    )
}

impl LlmAgent {
    /// 本轮使用的工具表：按有效 cwd / 知识库重建，并始终带上本会话的 remember 工具。
    /// `pinned` 非空时限定 invoke_skill 只能调用这些技能（任务钉选）。
    fn tools_for(
        &self,
        cwd: Option<&str>,
        knowledge: &[String],
        sid: &str,
        pinned: &[String],
    ) -> Arc<ToolRegistry> {
        let base = self.effective_base(cwd);
        let roots: Vec<std::path::PathBuf> =
            knowledge.iter().map(std::path::PathBuf::from).collect();
        // 捕获当前运行时句柄，供 task_start 在后台派生任务（execute 在 blocking 线程跑）。
        let handle = tokio::runtime::Handle::current();
        Arc::new(
            ToolRegistry::with_defaults(base.clone())
                .with_persistent_shell(base.clone(), sid)
                .with_skills_pinned(Self::skill_dirs_for(&base), pinned.to_vec())
                .with_knowledge(roots)
                .with_memory(sid, base.clone())
                .with_mcp()
                .with_local_gui()
                .with_tool(Box::new(crate::jobs::TaskStart::new(handle)))
                .with_tool(Box::new(crate::jobs::TaskList))
                .with_tool(Box::new(crate::jobs::TaskResult))
                .with_tool(Box::new(crate::jobs::TaskStop)),
        )
    }

    /// 把「本回合进行中插进来的消息」并入历史，模型**下一轮就能看见**，不必等本回合跑完。
    ///
    /// 为什么必须这样：用户中途补充信息，多半正是发现 agent 跑歪了要纠偏。等一整回合
    /// （可能几十轮工具调用、几分钟）结束再当成新消息处理，纠偏就晚了——弯路已经走完，
    /// token 已经烧掉。
    ///
    /// ⚠️ 调用点必须落在「上一轮 tool_result 已全部落库」之处。若插进
    /// 「assistant(tool_calls) → tool_result」这对配对中间，上游会 400，会话直接焊死。
    /// 主回路每轮开头正是这样的安全点。
    fn inject_pending(&self, reg: &SessionRegistry, sid: &str, cwd: Option<&str>) -> usize {
        let msgs = reg.drain_injectable(sid);
        for m in &msgs {
            let content = attach_uploads(
                &self.effective_base(cwd),
                m.files.clone(),
                m.content.clone(),
            );
            reg.append_message(
                sid,
                ChatMessage::user_with_images(content, m.images.clone()),
            );
        }
        msgs.len()
    }

    /// 本轮有效基准目录：会话 cwd（须为存在的目录）优先，否则全局工作目录。
    fn effective_base(&self, cwd: Option<&str>) -> std::path::PathBuf {
        cwd.map(str::trim)
            .filter(|c| !c.is_empty() && std::path::Path::new(c).is_dir())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.workdir.clone())
    }

    /// 技能目录 = `<base>/skills`（项目级）+ 全局数据目录，与构造时规则一致。
    fn skill_dirs_for(base: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut dirs = vec![base.join("skills")];
        if let Some(d) = wisecortex_core::config::skills_dir() {
            dirs.push(d);
        }
        dirs
    }

    /// 任务系统提示：技能目录按本轮有效工作目录加载（含 `<cwd>/skills` 项目级技能）；
    /// 钉选技能时只把这几个列入。默认 cwd 且未钉选时直接用构造期缓存的提示。
    fn system_prompt_for(&self, pinned: &[String], cwd: Option<&str>) -> String {
        let base = self.effective_base(cwd);
        if pinned.is_empty() && base == self.workdir {
            return self.system_prompt.clone();
        }
        let skills_prompt = SkillSet::load_dirs(&Self::skill_dirs_for(&base))
            .without_disabled(&wisecortex_core::skills_state::disabled())
            .only(pinned)
            .prompt_list();
        if skills_prompt.is_empty() {
            self.system_prompt_base.clone()
        } else {
            format!("{}\n\n{}", self.system_prompt_base, skills_prompt)
        }
    }

    /// 自动记忆：开启且历史增长够多时，后台抽取要点写入会话记忆（不阻塞本轮）。
    fn maybe_extract_memory(&self, reg: &SessionRegistry, sid: &str) {
        if !self.auto_memory {
            return;
        }
        let history = reg.history(sid);
        {
            let mut marks = self.extract_marks.lock().unwrap();
            let last = marks.get(sid).copied().unwrap_or(0);
            if history.len() < last + MEMORY_EXTRACT_STEP {
                return;
            }
            marks.insert(sid.to_string(), history.len());
        }
        // 与主对话同档（任务级覆盖优先）：全局默认档可能是另一家账户，后台抽取会静默失败。
        let resolved = reg
            .task_config(sid)
            .model_id
            .as_deref()
            .and_then(|id| self.llms.get(id).cloned());
        let client = self.client.clone();
        let provider = resolved
            .as_ref()
            .map_or_else(|| self.provider.clone(), |r| r.provider.clone());
        let model = resolved
            .as_ref()
            .map_or_else(|| self.model.clone(), |r| r.model.clone());
        let sid = sid.to_string();
        tokio::spawn(async move {
            extract_memory(&client, &provider, &model, &sid, &history).await;
        });
    }

    async fn run(&self, reg: &SessionRegistry, sid: &str, cwd: Option<&str>, knowledge: &[String]) {
        // 任务级配置：模型 / 技能 / auto-approve 覆盖全局默认。
        let cfg = reg.task_config(sid);
        let resolved = cfg
            .model_id
            .as_deref()
            .and_then(|id| self.llms.get(id).cloned());
        let provider: &ProviderConfig = resolved.as_ref().map_or(&self.provider, |r| &r.provider);
        let model: String = resolved
            .as_ref()
            .map_or_else(|| self.model.clone(), |r| r.model.clone());
        let price: Option<PriceOverride> = resolved.as_ref().map_or(self.price, |r| r.price);
        let max_tokens: u32 = resolved.as_ref().map_or(self.max_tokens, |r| r.max_tokens);
        let auto_approve = cfg.auto_approve.unwrap_or(self.auto_approve);
        // 任务钉选技能时，系统提示只列这几个技能（invoke_skill 也只在这几个里查）。
        // 显式 solo（用户在权限里选了 solo）时，额外注入"自主推进、尽量不打断"的指令。
        let solo = cfg.auto_approve == Some(true);
        let plan_mode = cfg.plan_mode;
        let system_prompt = {
            let mut base = self.system_prompt_for(&cfg.skills, cwd);
            if solo {
                base = format!("{base}\n\n{SOLO_HINT}");
            }
            if plan_mode {
                base = format!("{base}\n\n{PLAN_HINT}");
            }
            base
        };

        // 本轮工具表（按工作目录 / 知识库重建，并带 remember；钉选技能时限定 invoke_skill）。
        let tools = self.tools_for(cwd, knowledge, sid, &cfg.skills);
        // 记忆注入：项目级在前、会话级在后，各自非空才发。
        // 项目级跨会话——新开对话也带得走，正是为了不让用户把同一件事反复交代。
        let base = self.effective_base(cwd);
        let proj_mem =
            wisecortex_core::memory::read_scope(wisecortex_core::memory::Scope::Project(&base));
        let proj_note = (!proj_mem.trim().is_empty()).then(|| {
            format!(
                "PROJECT MEMORY for {} — shared by every session in this directory. \
                 Anything under Lessons is a mistake you already made or a correction the user already \
                 gave you: obey it, and do not make the user repeat it.\n{proj_mem}",
                base.display()
            )
        });
        let mem = wisecortex_core::memory::read(sid);
        let mem_note = (!mem.trim().is_empty()).then(|| {
            format!("SESSION MEMORY for this conversation only (recorded with remember — stay consistent with them):\n{mem}")
        });
        // 挂载了知识库时，附一条本轮 system 提示，引导模型用 knowledge_search 检索。
        let kb_note = (!knowledge.is_empty()).then(|| {
            format!(
                "本会话已挂载知识库（用户指定的目录/文件）：{}。\
                 当问题可能涉及这些资料时，先用 knowledge_search 工具检索，再据检索结果作答。",
                knowledge.join("、")
            )
        });
        let mut total = Usage::default();
        let mut total_cost = 0.0f64;
        let mut priced = false;
        let mut requests = 0u64;
        let mut cache_hits = 0u64;
        // 连续「纯空响应」计数：有产出（工具调用）即清零；达 MAX_EMPTY_RETRIES 仍空则停。
        let mut empty_retries = 0u32;
        // 「坏图自愈」只做一次：修完还 400 说明病根不在图，再剥下去只会白白丢图。
        let mut healed_images = false;

        // 交互式聊天主循环：用「交互上限」（很高，开发基本不触及）；定时/IM 任务走 run_once* 仍用 max_iterations。
        let max_iterations = self.max_iterations_interactive;
        for iteration in 1..=max_iterations {
            // 干活途中用户插进来的消息：在**本轮 LLM 调用之前**就并入历史，模型立刻看得见，
            // 而不是等整个回合跑完才当成新消息处理（那时弯路已经走完、token 已经烧掉）。
            let injected = self.inject_pending(reg, sid, cwd);
            if injected > 0 {
                wisecortex_core::sprintln!("session {sid}: 并入 {injected} 条工作中补充的消息");
            }

            // 每轮进入前按需压缩历史（insert-then-compress，复用缓存前缀）。
            // 关键：放在循环内，使**单个长自主任务内部**（一条 user 多轮工具）也能压缩，
            // 不再像放在循环外那样整任务只增不减。超阈值才真正压（否则廉价早退）。
            self.maybe_compress(reg, sid, provider, &model).await;

            // 起手即显示 ↑0 ↓0：哪怕还没拿到首个 token，也明确「已经在思考」。
            reg.publish(sid, progress_active(sid, &thinking_msg(0, 0)));

            let mut messages = vec![ChatMessage::system(&system_prompt)];
            // 项目记忆在会话记忆之前：跨会话的教训优先级更高，先入眼。
            if let Some(note) = &proj_note {
                messages.push(ChatMessage::system(note));
            }
            if let Some(note) = &mem_note {
                messages.push(ChatMessage::system(note));
            }
            if let Some(note) = &kb_note {
                messages.push(ChatMessage::system(note));
            }
            // 「图片只发一次」：只在**发出图片的那一回合**把图片带进上下文，更早回合的剥掉。
            // 剥的是 history() 返回的克隆，落盘历史与聊天记录里的图片不受影响。
            let mut hist = reg.history(sid);
            if self.auto_trim_context {
                compressor::keep_images_of_current_turn_only(&mut hist);
            }
            messages.extend(hist);
            // 滚动缓存断点：标记末尾 2 条消息，使上一回合的尾消息在本回合成为可命中前缀。
            mark_tail_cache_breakpoints(&mut messages, 2);
            let mut req = LlmRequest::new(model.clone(), messages);
            let mut tool_defs = tools.defs();
            tool_defs.push(task_tool_def()); // 仅主循环提供 task（子 agent 不嵌套）
            req.tools = tool_defs;
            req.max_tokens = max_tokens;
            req.caching_enabled = true;
            // 推理强度优先级：本会话(聊天)覆盖 > 该模型档内值 > 全局默认。
            // 本会话值来自任务配置（聊天底部的思考选择）；不支持思考的模型由线缆层自动忽略该值。
            let per_model = resolved.as_ref().map_or_else(
                || self.default_reasoning_effort.clone(),
                |r| r.reasoning_effort.clone(),
            );
            req.reasoning_effort = pick_effort(
                cfg.reasoning_effort.clone(),
                pick_effort(per_model, self.reasoning_effort.clone()),
            );

            let reg_cb = reg.clone();
            let sid_cb = sid.to_string();
            let result = self
                .client
                .complete(provider, &req, move |update| match update {
                    StreamUpdate::Text(delta) => {
                        reg_cb.publish(
                            &sid_cb,
                            ServerEvent::AssistantDelta {
                                session_id: sid_cb.clone(),
                                delta,
                            },
                        );
                    }
                    StreamUpdate::Usage { input, output } => {
                        reg_cb.publish(
                            &sid_cb,
                            progress_active(&sid_cb, &thinking_msg(input, output)),
                        );
                    }
                    // 限流 / 上游 5xx 自动重试：把「还有多久重试 · 第几/共几次」推成可见状态行，
                    // 否则用户只看到干转的 thinking，不知道在等什么（等待期间会持续刷新）。
                    StreamUpdate::Retrying {
                        attempt,
                        max,
                        wait_secs,
                        kind,
                    } => {
                        reg_cb.publish(
                            &sid_cb,
                            progress_active(&sid_cb, &retry_msg(attempt, max, wait_secs, kind)),
                        );
                    }
                })
                .await;

            reg.publish(sid, progress_done(sid));

            let resp = match result {
                Ok(r) => r,
                Err(e) => {
                    // 坏图自愈。一张模型解不了的图会把会话**永久**卡死：历史每轮完整重发，
                    // 坏图永远在里面；而 400 按规矩不重试，于是用户再也发不出任何消息，
                    // 除了手工去改 session 文件没有任何自救手段。这里破例——确认是图片问题，
                    // 就把坏图从**落盘历史**里剔掉（不是剔本轮那份克隆，否则重启后照样卡）再重跑。
                    if let wisecortex_core::llm::LlmError::Api { status, body } = &e {
                        if !healed_images
                            && wisecortex_core::llm::image::looks_like_image_error(*status, body)
                        {
                            healed_images = true;
                            let mut hist = reg.history(sid);
                            let cleaned = wisecortex_core::llm::image::sanitize(&mut hist);
                            if !cleaned.is_empty() {
                                reg.replace_history(sid, hist);
                                wisecortex_core::buglog::record(
                                    "llm",
                                    &format!(
                                        "session={sid} 图片导致 {status}，已剔除 {} 张坏图并重试：{}",
                                        cleaned.removed,
                                        cleaned.reasons.join("; ")
                                    ),
                                );
                                reg.publish(
                                    sid,
                                    ServerEvent::Warning {
                                        session_id: sid.to_string(),
                                        message: format!(
                                            "有 {} 张图片模型无法解码（{}），已从会话中移除并继续。",
                                            cleaned.removed,
                                            cleaned.reasons.join("；")
                                        ),
                                    },
                                );
                                continue;
                            }
                        }
                    }
                    // 落错误日志（重试已在 client 内尝试过仍失败才到这）：便于 wisecortex doctor 排查。
                    wisecortex_core::buglog::record(
                        "llm",
                        &format!("session={sid} 第 {iteration} 轮 LLM 调用失败: {e}"),
                    );
                    reg.publish(
                        sid,
                        ServerEvent::Error {
                            session_id: Some(sid.to_string()),
                            message: format!("LLM 调用失败: {e}"),
                            code: None,
                            top_up_url: None,
                        },
                    );
                    break;
                }
            };

            // 累计用量与成本。
            requests += 1;
            if resp.usage.cache_read_input_tokens > 0 {
                cache_hits += 1;
            }
            accumulate(&mut total, &resp.usage);
            if let Some(c) = pricing::calculate_cost_with(&model, &resp.usage, price) {
                total_cost += c;
                priced = true;
            }

            // 截断检测：finish_reason=="length" 说明本轮被 max_tokens 截断——大概率把工具参数
            // （尤其大文件写入）截成残缺 JSON，被归一为 {} 后报「缺少必填参数」。给用户可见告警 +
            // 解决办法，而不是静默失败。
            if truncated_by_length(&resp.finish_reason) {
                reg.publish(
                    sid,
                    ServerEvent::Warning {
                        session_id: sid.to_string(),
                        message: format!(
                            "本轮输出达到 token 上限（max_tokens={max_tokens}）被截断；若在写大文件，\
                             工具参数可能被截断导致报错。请把大文件分多次写入（先写一段，再用 edit_file 追加），\
                             或在「模型管理」给该模型调高 max_tokens（也可设 env WC_MAX_TOKENS）。"
                        ),
                    },
                );
            }

            // 思考内容（extended thinking）：思考结束后留一条可折叠块在聊天区，
            // 否则转录里只剩工具调用，看不出推理。仅展示，不回灌历史/不进 API。
            if let Some(think) = resp.thinking.as_ref().filter(|s| !s.trim().is_empty()) {
                reg.publish(
                    sid,
                    ServerEvent::AssistantThinking {
                        session_id: sid.to_string(),
                        content: think.clone(),
                    },
                );
            }

            // 助手的可见文本（工具回合也可能带前导说明）。
            if let Some(text) = resp.content.as_ref().filter(|s| !s.is_empty()) {
                reg.publish(
                    sid,
                    ServerEvent::AssistantMessage {
                        session_id: sid.to_string(),
                        content: text.clone(),
                        files: vec![],
                    },
                );
            }

            // 文本式工具调用兜底（D-info）：模型把 `<invoke …>` 当文本吐出时，解析层已抢救成结构化
            // 调用并把标记从 content 剥掉（防回灌污染历史）。这里只提示用户「发生过、已自动续上」。
            if resp.recovered_text_tool_calls > 0 {
                reg.publish(
                    sid,
                    ServerEvent::Warning {
                        session_id: sid.to_string(),
                        message: format!(
                            "模型把 {} 个工具调用写成了文本（没走结构化通道），已自动解析并继续执行。\
                             这偶发于较长对话；若频繁出现可重试或在「模型管理」换个模型。",
                            resp.recovered_text_tool_calls
                        ),
                    },
                );
            }

            // 无工具调用 → 本轮结束。
            if resp.tool_calls.is_empty() {
                // 「纯空响应」：无文本、无思考、也没有残缺的工具调用标记——模型这一轮什么都没回
                // （多为上游瞬时抖动，或模型空转）。既有兜底（length 截断 / had_unparsed_tool_markup）
                // 都不覆盖它，过去会静默塞一条空 assistant 消息并结束，前端零回显 → 用户只看到「莫名停了」。
                let empty_response = resp.content.as_deref().is_none_or(|s| s.trim().is_empty())
                    && resp.thinking.as_deref().is_none_or(|s| s.trim().is_empty())
                    && !resp.had_unparsed_tool_markup;
                if empty_response && empty_retries < MAX_EMPTY_RETRIES {
                    empty_retries += 1;
                    reg.publish(
                        sid,
                        ServerEvent::Warning {
                            session_id: sid.to_string(),
                            message: "模型返回了空响应（无内容也无工具调用），正在自动重试…"
                                .to_string(),
                        },
                    );
                    continue; // 重试本轮；空消息不入历史（否则污染上下文 / 给上游回灌空 assistant 块）。
                }
                // D-fail：见到工具调用标记但格式残缺、解析不出 → 不要静默停，明确告知用户。
                if resp.had_unparsed_tool_markup {
                    reg.publish(
                        sid,
                        ServerEvent::Warning {
                            session_id: sid.to_string(),
                            message:
                                "检测到模型把工具调用写成了文本、但格式残缺无法解析，本轮已中止\
                                      （未执行任何工具）。请重试，或在「模型管理」换一个模型。"
                                    .to_string(),
                        },
                    );
                } else if empty_response {
                    // 重试后仍空：停并给可见告警，而不是静默消失。
                    reg.publish(
                        sid,
                        ServerEvent::Warning {
                            session_id: sid.to_string(),
                            message: "模型连续返回空响应，本轮已停止。请重试，或在「模型管理」换一个模型。"
                                .to_string(),
                        },
                    );
                }
                // 仅当有可见文本时才入历史（空响应不写，避免给上游回灌空 assistant 块）。
                if let Some(text) = resp.content.filter(|s| !s.is_empty()) {
                    reg.append_message(sid, ChatMessage::assistant(text));
                }
                break;
            }
            // 有工具调用 = 本轮有产出，清零空响应计数（让后续偶发空响应仍能各自重试一次）。
            empty_retries = 0;

            // 记录助手的工具调用回合到历史。
            reg.append_message(
                sid,
                // 带上思考签名：Gemini 3 要求下一轮把它原样回传，否则整段历史被 400 拒。
                ChatMessage::assistant_tool_calls(resp.content.clone(), resp.tool_calls.clone())
                    .with_thought_signature(resp.thought_signature.clone()),
            );

            // 多个 task 调用 → 并行跑子 agent；否则逐个执行（保证 fs 工具/确认顺序）。
            if should_run_parallel(&resp.tool_calls, plan_mode) {
                self.run_parallel_tasks(reg, sid, &resp.tool_calls).await;
            } else {
                for tc in &resp.tool_calls {
                    self.run_tool(reg, sid, tc, &tools, auto_approve, plan_mode)
                        .await;
                }
            }

            if iteration == max_iterations {
                reg.publish(
                    sid,
                    ServerEvent::Warning {
                        session_id: sid.to_string(),
                        message: format!(
                            "已达工具调用上限 {max_iterations}，停止（任务可能未完成）。\
                             可在设置或 WC_MAX_ITERATIONS 调高上限，或对该任务换用更便宜的模型/钉选技能以降本。"
                        ),
                    },
                );
            }
        }

        let cache_stats =
            if total.cache_read_input_tokens > 0 || total.cache_creation_input_tokens > 0 {
                Some(CacheStats {
                    total_requests: Some(requests),
                    cache_hit_requests: Some(cache_hits),
                    cache_read_input_tokens: Some(total.cache_read_input_tokens),
                })
            } else {
                None
            };
        reg.publish(
            sid,
            ServerEvent::Complete {
                session_id: sid.to_string(),
                iterations: requests,
                cost: total_cost,
                duration: None,
                cache_stats,
                awaiting_user_feedback: None,
                // 有定价 → calculated（前端显示金额）；否则 estimated（显示 N/A）。
                cost_source: Some(if priced { "calculated" } else { "estimated" }.to_string()),
            },
        );
        // 计入服务端当日成本账本（仅有定价的真实成本，与前端口径一致），并广播新总额。
        if priced && total_cost > 0.0 {
            let day_total = wisecortex_core::cost::add(total_cost);
            reg.publish_global(ServerEvent::CostUpdate {
                cost_today: day_total,
            });
        }
    }

    /// 并行执行多个 task 调用（各派生子 agent，concurrent）。
    /// 每个子任务完成即更新进度（k/N）并发布其结果事件——长并行不再「静默到全完」；
    /// 历史仍按原调用顺序回灌（稳定可回放，tool_use/tool_result 对齐）。
    async fn run_parallel_tasks(&self, reg: &SessionRegistry, sid: &str, calls: &[ToolCall]) {
        use futures_util::stream::{FuturesUnordered, StreamExt};
        let total = calls.len();
        let futs = FuturesUnordered::new();
        for (idx, tc) in calls.iter().enumerate() {
            let args: Value = serde_json::from_str(&tc.arguments).unwrap_or_else(|_| json!({}));
            let desc = args
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            reg.publish(
                sid,
                ServerEvent::ToolCall {
                    session_id: sid.to_string(),
                    name: "task".to_string(),
                    args: args.clone(),
                    summary: Some(format!("子任务: {desc}")),
                },
            );
            let prompt = args
                .get("prompt")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let id = tc.id.clone();
            futs.push(async move {
                let p = SubagentProgress {
                    reg,
                    sid,
                    desc: &desc,
                };
                let (text, steps) = self.run_subagent(&prompt, p).await;
                (idx, id, desc, text, steps)
            });
        }
        reg.publish(
            sid,
            progress_active(sid, &format!("并行子任务 ×{total} 执行中…")),
        );
        let mut results: Vec<Option<(String, String)>> = vec![None; total];
        let mut futs = futs;
        let mut done = 0usize;
        while let Some((idx, id, desc, text, steps)) = futs.next().await {
            done += 1;
            reg.publish(
                sid,
                progress_active(sid, &format!("并行子任务 ({done}/{total}) 完成：{desc}")),
            );
            reg.publish(
                sid,
                ServerEvent::ToolResult {
                    session_id: sid.to_string(),
                    // 展示带过程；回灌进历史的仍是纯结果（见下），别让轨迹占模型上下文。
                    result: json!(format_subagent_result(&desc, &text, &steps)),
                },
            );
            results[idx] = Some((id, text));
        }
        for (id, text) in results.into_iter().flatten() {
            reg.append_message(sid, ChatMessage::tool_result(id, text));
        }
    }

    /// 子 agent：用隔离的临时历史跑一个工具循环，返回最终文本摘要。
    /// 不广播中间过程、不持久化、无 task 工具（不嵌套）；工具直接执行（不再二次确认）。
    /// 返回 (最终文本, 步骤轨迹)。轨迹由调用方拼进结果块，供用户看「它到底干了什么」。
    async fn run_subagent(
        &self,
        prompt: &str,
        progress: SubagentProgress<'_>,
    ) -> (String, Vec<String>) {
        // 子任务同样吃项目记忆：主会话里纠正过的坑，派下去的活不该再踩一遍。
        let sp = self.with_project_memory(&self.system_prompt, &self.workdir);
        let o = self
            .run_subagent_with(
                prompt,
                &sp,
                &self.tools,
                self.subagent_max_iterations,
                None,
                SubagentSinks {
                    log_id: None,
                    progress: Some(progress),
                },
            )
            .await;
        (o.text, o.steps)
    }

    /// 在 system 提示后附上该工作目录的**项目记忆**（空则原样返回）。
    ///
    /// 每次运行时读盘，所以刚记下的教训下一次任务就能吃到，不必重启。
    /// 无人值守路径（定时/IM/后台/子任务）原本完全读不到任何记忆——同一个坑
    /// 在聊天里纠正过，定时任务照踩不误。
    fn with_project_memory(&self, base: &str, dir: &std::path::Path) -> String {
        let mem = wisecortex_core::memory::read_scope(wisecortex_core::memory::Scope::Project(dir));
        if mem.trim().is_empty() {
            return base.to_string();
        }
        format!(
            "{base}\n\nPROJECT MEMORY for {} — accumulated across sessions. Anything under Lessons is a \
             mistake already made or a correction the user already gave: obey it.\n{mem}",
            dir.display()
        )
    }

    /// 为指定工作目录重建 (system_prompt, tools)，供定时任务隔离执行：
    /// 技能目录 = `<workdir>/skills` + 全局数据目录；系统提示按这些目录的技能重建。
    fn rebuilt_for_workdir(&self, workdir: &std::path::Path) -> (String, Arc<ToolRegistry>) {
        let skill_dirs = Self::skill_dirs_for(workdir);
        let skills_prompt = SkillSet::load_dirs(&skill_dirs)
            .without_disabled(&wisecortex_core::skills_state::disabled())
            .prompt_list();
        let system_prompt = if skills_prompt.is_empty() {
            self.system_prompt_base.clone()
        } else {
            format!("{}\n\n{}", self.system_prompt_base, skills_prompt)
        };
        // 记忆按**该任务自己的**工作目录取，不是 agent 全局那个——定时任务隔离到哪个
        // 项目，就该吃哪个项目的教训。
        let system_prompt = self.with_project_memory(&system_prompt, workdir);
        let tools = Arc::new(
            ToolRegistry::with_defaults(workdir.to_path_buf())
                .with_skills(skill_dirs)
                .with_memory(UNATTENDED_SID, workdir.to_path_buf())
                .with_local_gui(),
        );
        (system_prompt, tools)
    }

    /// 子 agent 主循环，使用传入的 system_prompt 与工具表（定时任务可注入按 workdir 重建的表）。
    /// `model_id` 为 Some 且命中已配置档时，本次用该档的 provider/model/价格覆盖全局默认
    ///（与聊天的任务级模型覆盖同一套机制），否则用 agent 自身默认。
    async fn run_subagent_with(
        &self,
        prompt: &str,
        system_prompt: &str,
        tools: &Arc<ToolRegistry>,
        max_iters: usize,
        model_id: Option<&str>,
        sinks: SubagentSinks<'_>,
    ) -> RunOutcome {
        let SubagentSinks { log_id, progress } = sinks;
        // 任务级模型覆盖：命中则用该档的 provider/model/price，否则回退 agent 默认。
        let resolved = model_id
            .filter(|s| !s.is_empty())
            .and_then(|id| self.llms.get(id).cloned());
        let provider: &ProviderConfig = resolved.as_ref().map_or(&self.provider, |r| &r.provider);
        let model: String = resolved
            .as_ref()
            .map_or_else(|| self.model.clone(), |r| r.model.clone());
        let price: Option<PriceOverride> = resolved.as_ref().map_or(self.price, |r| r.price);
        let max_tokens: u32 = resolved.as_ref().map_or(self.max_tokens, |r| r.max_tokens);
        // 每一步明细的三个去处（互不排斥，按调用方传了什么决定）：
        //   · `log_id` 为 Some（定时任务）→ 追加进该任务的执行日志，事后定位
        //     「status=ok 但其实没做完」的真实卡点（多为工具静默报错）。
        //   · `progress` 为 Some（聊天派生的子任务）→ 实时推给发起会话当进度行，
        //     并攒进 trail 随结果回传。否则子 agent 对用户是纯黑箱。
        // 用 std Mutex 而非 RefCell：本函数的 future 要跨 tokio 任务边界，必须 Send。
        // 锁只在闭包内同步持有、绝不跨 await，不会阻塞运行时。
        let trail: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let step = {
            let trail = trail.clone();
            move |line: String| {
                if let Some(id) = log_id {
                    wisecortex_core::cron::append_log(id, &line);
                }
                if let Some(p) = progress {
                    p.publish(&line);
                    let mut t = trail.lock().unwrap();
                    // 超长任务只留前 N 步，避免把结果块撑爆（尾部另有终止行说明结局）。
                    if t.len() < SUBAGENT_TRAIL_MAX {
                        t.push(line);
                    }
                }
            }
        };
        let mut history = vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(prompt),
        ];
        // 累计用量/成本，供定时任务展示（与聊天回路同口径）。
        let mut usage = Usage::default();
        let mut cost = 0.0f64;
        let mut priced = false;
        let mut requests = 0u64;
        // 各 return 点都经由它构造，顺带把已攒下的步骤轨迹一并带出去。
        let finish = {
            let trail = trail.clone();
            move |text: String, usage: Usage, cost, priced, requests, status| RunOutcome {
                text,
                usage,
                cost,
                priced,
                requests,
                status,
                steps: trail.lock().unwrap().clone(),
            }
        };

        for _ in 0..max_iters {
            // 长任务防膨胀：按需压缩本地历史（与聊天回路一致，定时任务尤其需要）。
            self.maybe_compress_subagent(&mut history, provider, &model)
                .await;

            let mut req = LlmRequest::new(model.clone(), history.clone());
            req.tools = tools.defs(); // 不含 task
            req.max_tokens = max_tokens;
            req.caching_enabled = true;
            let per_model = resolved.as_ref().map_or_else(
                || self.default_reasoning_effort.clone(),
                |r| r.reasoning_effort.clone(),
            );
            req.reasoning_effort = pick_effort(per_model, self.reasoning_effort.clone());

            let resp = match self.client.complete(provider, &req, |_| {}).await {
                Ok(r) => r,
                Err(e) => {
                    wisecortex_core::buglog::record("llm", &format!("子任务 LLM 调用失败: {e}"));
                    step(format!(
                        "✗ 第{}轮 LLM 调用失败：{}",
                        requests + 1,
                        log_snippet(&e.to_string(), 300)
                    ));
                    return finish(
                        format!("子任务失败：{e}"),
                        usage,
                        cost,
                        priced,
                        requests,
                        "llm_error",
                    );
                }
            };

            requests += 1;
            accumulate(&mut usage, &resp.usage);
            if let Some(c) = pricing::calculate_cost_with(&model, &resp.usage, price) {
                cost += c;
                priced = true;
            }

            // 截断检测：本轮被 max_tokens 截断时记一行，便于定位「工具参数残缺/任务没做完」的真因。
            if truncated_by_length(&resp.finish_reason) {
                step(format!(
                    "⚠ 第{requests}轮输出达到 token 上限(max_tokens={max_tokens})被截断，工具参数可能残缺；建议调高 max_tokens 或分多次写入"
                ));
            }

            // 思考首句进日志：思考可能很长，只记一句（与聊天里的折叠预览同源），
            // 避免日志里「全是工具调用、看不出推理」。
            if let Some(t) = resp.thinking.as_deref().filter(|t| !t.trim().is_empty()) {
                step(format!(
                    "✻ 第{requests}轮思考 {}",
                    log_snippet(first_line(t), 200)
                ));
            }

            // 模型本回合的文字输出（无论是否还要调工具都记下，便于复盘它的推理/判断）。
            if let Some(c) = resp.content.as_deref().filter(|c| !c.trim().is_empty()) {
                step(format!("· 第{requests}轮 {}", log_snippet(c, 500)));
            }

            // 文本式工具调用兜底（与主循环同源）：解析层已抢救并续上，这里记一行便于复盘。
            if resp.recovered_text_tool_calls > 0 {
                step(format!(
                    "↻ 第{requests}轮模型把 {} 个工具调用写成了文本，已自动解析并继续",
                    resp.recovered_text_tool_calls
                ));
            }

            if resp.tool_calls.is_empty() {
                // 见到标记但解析不出：判未完成，别当成正常给出答复。
                if resp.had_unparsed_tool_markup {
                    step(format!(
                        "✗ 第{requests}轮模型把工具调用写成了文本但格式残缺、无法解析，判未完成"
                    ));
                    return finish(
                        resp.content.unwrap_or_default(),
                        usage,
                        cost,
                        priced,
                        requests,
                        "text_tool_markup",
                    );
                }
                let answer = resp.content.unwrap_or_default();
                step(format!("✓ 完成（第{requests}轮给出最终答复）"));
                return finish(answer, usage, cost, priced, requests, "ok");
            }

            history.push(
                ChatMessage::assistant_tool_calls(resp.content.clone(), resp.tool_calls.clone())
                    .with_thought_signature(resp.thought_signature.clone()),
            );
            let mut turn_images: Vec<String> = Vec::new();
            for tc in &resp.tool_calls {
                let args: Value = serde_json::from_str(&tc.arguments).unwrap_or_else(|_| json!({}));
                step(format!(
                    "⚙ 调用 {} {}",
                    tc.name,
                    log_snippet(&args.to_string(), 300)
                ));
                let tools = tools.clone();
                let name = tc.name.clone();
                let outcome = tokio::task::spawn_blocking(move || tools.execute(&name, &args))
                    .await
                    .unwrap_or_else(|e| Err(format!("工具线程异常: {e}")));
                let text = match outcome {
                    Ok(out) => {
                        step(format!("  ← {}", log_snippet(&out, 400)));
                        out
                    }
                    Err(err) => {
                        // 工具报错是「跑完却没做成」的高发原因，单独标红记一行。
                        step(format!("  ✗ {} 出错：{}", tc.name, log_snippet(&err, 400)));
                        format!("ERROR: {err}")
                    }
                };
                // 截图类工具返图：先剥出图片再截断（否则 cap 会截断 base64 损坏 JSON）。
                let (text, images) = split_image_result(&text);
                turn_images.extend(images);
                let text = cap_tool_output(text, self.tool_output_limit);
                history.push(ChatMessage::tool_result(tc.id.clone(), text));
            }
            // 本轮有截图 → 追加一条视觉 user 消息，模型下一轮即可“看到”。
            if !turn_images.is_empty() {
                history.push(ChatMessage::user_with_images(
                    "（以上工具返回的截图）".to_string(),
                    turn_images,
                ));
            }
        }
        step(format!(
            "✗ 达到回合上限（{max_iters} 轮）仍未结束，判未完成"
        ));
        finish(
            "子任务达到回合上限，未完成。".to_string(),
            usage,
            cost,
            priced,
            requests,
            "max_iterations",
        )
    }

    /// 子 agent 本地历史的按需压缩。`history[0]` 是 system 提示，保留不动；
    /// 其余消息做 insert-then-compress（与聊天回路同套阈值/预算/防抖动）。
    /// 压缩调用须用与本任务一致的 `provider`/`model`（可能是任务级覆盖的档），不能写死全局默认。
    async fn maybe_compress_subagent(
        &self,
        history: &mut Vec<ChatMessage>,
        provider: &ProviderConfig,
        model: &str,
    ) {
        // 先剥旧图：旧截图按兆级 base64 占请求体，摘要压缩管不住字节层面。
        compressor::prune_old_images(history, KEEP_RECENT_IMAGE_MESSAGES);
        if history.len() < 2 || compressor::estimate_tokens(history) < COMPRESSION_THRESHOLD_TOKENS
        {
            return;
        }
        let system = history[0].clone();
        let body: Vec<ChatMessage> = history[1..].to_vec();
        let Some(boundary) = compressor::compress_boundary(&body, KEEP_RECENT_TOKENS) else {
            return;
        };
        if compressor::estimate_tokens(&body[..boundary]) < MIN_COMPRESS_PREFIX_TOKENS {
            return;
        }
        // 复用缓存前缀：system + 现有 body + 压缩指令（压缩调用不带工具）。
        // 与主回路同理：摘要不需要看图，剥掉能大幅减小请求体。
        let mut for_summary = body.clone();
        if self.auto_trim_context {
            compressor::drop_all_images(&mut for_summary);
        }
        let mut msgs = vec![system.clone()];
        msgs.extend_from_slice(&for_summary);
        msgs.push(ChatMessage::user(compressor::COMPRESSION_PROMPT));
        let mut req = LlmRequest::new(model.to_string(), msgs);
        req.max_tokens = 4096;
        req.caching_enabled = true;
        if let Ok(resp) = self.client.complete(provider, &req, |_| {}).await {
            let summary = compressor::extract_summary(&resp.content.unwrap_or_default());
            if summary.is_empty() {
                return;
            }
            let mut new_body = compressor::build_compressed_history(&summary, &body, boundary);
            if self.auto_trim_context {
                compressor::drop_all_images(&mut new_body);
            }
            let mut new_history = vec![system];
            new_history.extend(new_body);
            *history = new_history;
        }
    }

    /// 历史过长时做一次 insert-then-compress：注入压缩指令让模型摘要，重建历史。
    /// `provider`/`model` 须与本会话主对话一致（任务级覆盖档）：全局默认档可能是
    /// 另一家中转/账户，主对话正常而压缩永远失败（如 403 余额不足），历史只增不减。
    async fn maybe_compress(
        &self,
        reg: &SessionRegistry,
        sid: &str,
        provider: &ProviderConfig,
        model: &str,
    ) {
        // 先剥旧图再谈压缩：旧截图按兆级 base64 占请求体，攒多了不但主调用 413，
        // 连这里的摘要调用自己（带全量历史）也会 413，压缩失效、历史只增不减。
        let mut history = reg.history(sid);
        if compressor::prune_old_images(&mut history, KEEP_RECENT_IMAGE_MESSAGES) > 0 {
            reg.replace_history(sid, history.clone());
        }
        let est = compressor::estimate_tokens(&history);
        if est < COMPRESSION_THRESHOLD_TOKENS {
            return;
        }
        let Some(boundary) = compressor::compress_boundary(&history, KEEP_RECENT_TOKENS) else {
            return;
        };
        // 防抖动：若可压缩前缀太小（如最近一条工具输出本身就很大），压缩省不下多少，
        // 反而白花一次摘要调用并可能每轮重触发——直接跳过，照常带着大上下文继续。
        if compressor::estimate_tokens(&history[..boundary]) < MIN_COMPRESS_PREFIX_TOKENS {
            return;
        }

        // 压缩本身是一次额外 LLM 调用，可能耗时——给可见进度：先报「在压多大」，
        // 再随摘要流式产出刷新 ↓输出 token，让用户看出在干活（配合空闲超时，卡死也会自动重试）。
        reg.publish(
            sid,
            progress_active(
                sid,
                &format!("压缩上下文（约 {} tok）…", fmt_tokens(est as u64)),
            ),
        );

        // 在完整历史后插入压缩指令（复用 system+tools 缓存）；压缩调用不带工具。
        // 摘要本身用不着看图，图片却是请求体里最重的部分——曾因此把压缩调用自己顶成 413。
        let mut for_summary = history.clone();
        if self.auto_trim_context {
            compressor::drop_all_images(&mut for_summary);
        }
        let mut messages = vec![ChatMessage::system(&self.system_prompt)];
        messages.extend(for_summary);
        messages.push(ChatMessage::user(compressor::COMPRESSION_PROMPT));
        let mut req = LlmRequest::new(model.to_string(), messages);
        req.max_tokens = 4096;
        req.caching_enabled = true;

        let reg_cb = reg.clone();
        let sid_cb = sid.to_string();
        let est_tok = est as u64;
        let result = self
            .client
            .complete(provider, &req, move |update| {
                if let StreamUpdate::Usage { output, .. } = update {
                    reg_cb.publish(
                        &sid_cb,
                        progress_active(
                            &sid_cb,
                            &format!(
                                "压缩上下文（约 {} tok）· ↓{}",
                                fmt_tokens(est_tok),
                                fmt_tokens(output)
                            ),
                        ),
                    );
                }
            })
            .await;
        reg.publish(sid, progress_done(sid));

        match result {
            Ok(resp) => {
                let summary = compressor::extract_summary(&resp.content.unwrap_or_default());
                if summary.is_empty() {
                    return;
                }
                let mut new_history =
                    compressor::build_compressed_history(&summary, &history, boundary);
                // 压缩后保留的近期消息也不再留图（用户诉求：压缩上下文时不保留图片）。
                if self.auto_trim_context {
                    compressor::drop_all_images(&mut new_history);
                }
                let new_est = compressor::estimate_tokens(&new_history);
                reg.replace_history(sid, new_history);
                reg.publish(
                    sid,
                    ServerEvent::Info {
                        session_id: sid.to_string(),
                        message: format!("🗜 已压缩上下文（约 {est} → {new_est} tok）"),
                    },
                );
            }
            // 压缩失败不阻断本轮，但必须让用户看见：静默跳过曾掩盖「压缩调用自身 413」
            // 的死亡螺旋（历史越长越压不动，越压不动越长）。
            Err(e) => {
                wisecortex_core::seprintln!("压缩调用失败（sid={sid}）：{e}");
                reg.publish(
                    sid,
                    ServerEvent::Info {
                        session_id: sid.to_string(),
                        message: format!("🗜 上下文压缩失败（{e}），本轮先携带原上下文继续"),
                    },
                );
            }
        }
    }

    /// 执行单个工具调用：发 tool_call →（必要时确认）→ 执行 → 发 tool_result → 入历史。
    async fn run_tool(
        &self,
        reg: &SessionRegistry,
        sid: &str,
        tc: &ToolCall,
        tools: &Arc<ToolRegistry>,
        auto_approve: bool,
        plan_mode: bool,
    ) {
        let args: Value = serde_json::from_str(&tc.arguments).unwrap_or_else(|_| json!({}));
        let summary = tools.summary(&tc.name, &args);
        reg.publish(
            sid,
            ServerEvent::ToolCall {
                session_id: sid.to_string(),
                name: tc.name.clone(),
                args: args.clone(),
                summary: Some(summary.clone()),
            },
        );

        // 计划模式：拦截一切改动类工具（写/改/shell/MCP/后台任务，以及会改动的 task 子 agent）。
        if plan_mode && (tc.name == "task" || tools.requires_approval(&tc.name)) {
            let msg = "计划模式：禁止执行改动类操作。请先用只读工具完成调研，输出可执行的分步计划并请用户确认；用户关闭计划模式后再动手。".to_string();
            reg.publish(
                sid,
                ServerEvent::ToolResult {
                    session_id: sid.to_string(),
                    result: json!(msg),
                },
            );
            reg.append_message(sid, ChatMessage::tool_result(tc.id.clone(), msg));
            return;
        }

        // PreToolUse 钩子：可拦截工具执行（原因回灌给模型），或放行。
        let pre = fire_hook(
            hooks::event::PRE_TOOL_USE,
            Some(tc.name.clone()),
            json!({"event":"PreToolUse","session_id":sid,"tool_name":tc.name,"tool_input":args}),
        )
        .await;
        if pre.block {
            let msg = format!(
                "操作被 hook 拦截：{}",
                pre.reason.as_deref().unwrap_or("(无原因)")
            );
            reg.publish(
                sid,
                ServerEvent::ToolResult {
                    session_id: sid.to_string(),
                    result: json!(msg),
                },
            );
            reg.append_message(sid, ChatMessage::tool_result(tc.id.clone(), msg));
            return;
        }

        // task：派生子 agent（隔离上下文），不走同步工具表。
        if tc.name == "task" {
            let prompt = args
                .get("prompt")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let desc = args
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("子任务")
                .to_string();
            reg.publish(sid, progress_active(sid, "子任务执行中…"));
            let (result, steps) = self
                .run_subagent(
                    &prompt,
                    SubagentProgress {
                        reg,
                        sid,
                        desc: &desc,
                    },
                )
                .await;
            reg.publish(
                sid,
                ServerEvent::ToolResult {
                    session_id: sid.to_string(),
                    // 展示带过程；回灌进历史的仍是纯结果，别让轨迹占模型上下文。
                    result: json!(format_subagent_result(&desc, &result, &steps)),
                },
            );
            reg.append_message(sid, ChatMessage::tool_result(tc.id.clone(), result));
            return;
        }

        // 非自动模式下，危险操作（写/改/shell）执行前征求确认。
        if !auto_approve && tools.requires_approval(&tc.name) {
            let approved = self.confirm(reg, sid, &summary).await;
            if !approved {
                let msg = "用户拒绝了该操作".to_string();
                reg.publish(
                    sid,
                    ServerEvent::ToolResult {
                        session_id: sid.to_string(),
                        result: json!(msg),
                    },
                );
                reg.append_message(sid, ChatMessage::tool_result(tc.id.clone(), msg));
                return;
            }
        }

        // 工具执行是阻塞 IO，放到 blocking 线程，避免卡住 runtime。
        let tools_exec = tools.clone();
        let name = tc.name.clone();
        let args_exec = args.clone();
        let outcome = tokio::task::spawn_blocking(move || tools_exec.execute(&name, &args_exec))
            .await
            .unwrap_or_else(|e| Err(format!("工具线程异常: {e}")));

        let raw_text = match &outcome {
            Ok(out) => out.clone(),
            Err(err) => format!("ERROR: {err}"),
        };
        // 截图类工具返图：先剥出图片（图片不进 hook/截断/tool_result），再走后续。
        let (mut result_text, images) = split_image_result(&raw_text);

        // PostToolUse 钩子：可追加上下文到结果（如自动格式化后的提示）。
        let post = fire_hook(
            hooks::event::POST_TOOL_USE,
            Some(tc.name.clone()),
            json!({"event":"PostToolUse","session_id":sid,"tool_name":tc.name,"tool_input":args,"tool_result":result_text}),
        )
        .await;
        if let Some(ctx) = post.additional_context {
            result_text.push_str(&format!("\n\n[hook]\n{ctx}"));
        }
        // 截断超大输出，防一次工具调用灌爆上下文（最大成本来源）。
        let result_text = cap_tool_output(result_text, self.tool_output_limit);

        reg.publish(
            sid,
            ServerEvent::ToolResult {
                session_id: sid.to_string(),
                result: json!(result_text),
            },
        );
        reg.append_message(sid, ChatMessage::tool_result(tc.id.clone(), result_text));
        // 截图回灌：图片单独作为视觉 user 消息进历史，模型下一轮即可“看到”。
        // 进历史前先体检——坏图一旦入库，之后每一轮都会被重发并触发 400，把会话卡死。
        // 在门口拦掉，比事后靠自愈捞回来代价小得多。
        if !images.is_empty() {
            let mut msg = [ChatMessage::user_with_images(
                "（以上工具返回的截图）".to_string(),
                images,
            )];
            let cleaned = wisecortex_core::llm::image::sanitize(&mut msg);
            if !cleaned.is_empty() {
                wisecortex_core::buglog::record(
                    "llm",
                    &format!(
                        "session={sid} 工具截图有 {} 张模型解不了，未入库：{}",
                        cleaned.removed,
                        cleaned.reasons.join("; ")
                    ),
                );
            }
            let [msg] = msg;
            reg.append_message(sid, msg);
        }
    }

    /// 发起一次确认并阻塞等待用户回应（超时按拒绝）。
    async fn confirm(&self, reg: &SessionRegistry, sid: &str, message: &str) -> bool {
        let id = format!(
            "conf_{}",
            CONF_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let rx = reg.register_confirmation(id.clone());
        reg.publish(
            sid,
            ServerEvent::RequestConfirmation {
                session_id: sid.to_string(),
                id,
                message: format!("允许执行：{message} ?"),
                default: false,
            },
        );
        match tokio::time::timeout(
            std::time::Duration::from_secs(CONFIRMATION_TIMEOUT_SECS),
            rx,
        )
        .await
        {
            Ok(Ok(result)) => matches!(
                result.trim().to_ascii_lowercase().as_str(),
                "yes" | "y" | "true" | "ok" | "approve" | "1"
            ),
            // 超时或通道被丢弃 → 拒绝。
            _ => false,
        }
    }
}

/// 确认请求 id 的进程内自增序号。
static CONF_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// 给末尾 n 条非 system 消息打缓存断点（Anthropic 滚动缓存；其它 format 无副作用）。
fn mark_tail_cache_breakpoints(messages: &mut [ChatMessage], n: usize) {
    let mut marked = 0;
    for m in messages.iter_mut().rev() {
        if marked >= n {
            break;
        }
        if m.role == Role::System {
            continue;
        }
        m.cache_breakpoint = true;
        marked += 1;
    }
}

/// 从首条用户消息派生会话标题（首行，截断 ~30 字符）。
fn derive_title(content: &str) -> String {
    let line = content
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    let mut title: String = line.chars().take(30).collect();
    if line.chars().count() > 30 {
        title.push('…');
    }
    if title.is_empty() {
        title.push_str("新会话");
    }
    title
}

/// 累加用量（跨工具回合）。
fn accumulate(total: &mut Usage, u: &Usage) {
    total.prompt_tokens += u.prompt_tokens;
    total.completion_tokens += u.completion_tokens;
    total.total_tokens += u.total_tokens;
    total.cache_read_input_tokens += u.cache_read_input_tokens;
    total.cache_creation_input_tokens += u.cache_creation_input_tokens;
}

/// 在 blocking 线程里跑某事件的钩子（避免阻塞 runtime）；未配置则零开销返回默认。
async fn fire_hook(
    event: &'static str,
    matcher: Option<String>,
    payload: Value,
) -> hooks::HookOutcome {
    tokio::task::spawn_blocking(move || hooks::for_event(event, matcher.as_deref(), payload))
        .await
        .unwrap_or_default()
}

/// 取首个非空行（思考首句）：思考可能很长，日志/预览只留一句。
fn first_line(s: &str) -> &str {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

/// 把 token 数格式化为简短显示：≥1000 显示成 "1.2k"，否则原样数字。
fn fmt_tokens(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

/// 思考中进度文案：带「↑输入 ↓输出」token 计（含 0），让用户一眼看出在干活、数据在流动。
fn thinking_msg(input: u64, output: u64) -> String {
    format!("thinking · ↑{} ↓{}", fmt_tokens(input), fmt_tokens(output))
}

/// 秒数转人话：90 →「1 分 30 秒」、600 →「10 分」、45 →「45 秒」。
fn fmt_wait(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs} 秒");
    }
    let (m, s) = (secs / 60, secs % 60);
    if s == 0 {
        format!("{m} 分")
    } else {
        format!("{m} 分 {s} 秒")
    }
}

/// 限流等待的状态行：让用户看清「在等什么 · 还要等多久 · 第几次」，取代干转的 thinking。
fn retry_msg(attempt: u32, max: u32, wait_secs: u64, kind: RetryKind) -> String {
    format!(
        "⏳ {} · {} 后自动重试 · 第 {attempt}/{max} 次",
        kind.label(),
        fmt_wait(wait_secs)
    )
}

fn progress_active(sid: &str, msg: &str) -> ServerEvent {
    ServerEvent::Progress {
        session_id: sid.to_string(),
        message: Some(msg.to_string()),
        progress_type: Some("thinking".to_string()),
        phase: Some("active".to_string()),
        status: Some("start".to_string()),
        metadata: None,
        started_at: None,
        elapsed: None,
    }
}

fn progress_done(sid: &str) -> ServerEvent {
    ServerEvent::Progress {
        session_id: sid.to_string(),
        message: None,
        progress_type: None,
        phase: Some("done".to_string()),
        status: Some("stop".to_string()),
        metadata: None,
        started_at: None,
        elapsed: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subagent_result_carries_the_step_trail() {
        // 用户诉求：子任务不能是黑箱——「完全不知道子 AGENT 在干什么」。
        // 结果块里必须能看到它调了什么、卡在哪，而不只是一句结论。
        let steps = vec![
            "⚙ 调用 grep {\"pattern\":\"skill\"}".to_string(),
            "  ← 命中 12 处".to_string(),
            "✗ 达到回合上限（100 轮）仍未结束，判未完成".to_string(),
        ];
        let out = format_subagent_result("审查宗门改动", "子任务达到回合上限，未完成。", &steps);
        assert!(
            out.starts_with("[子任务·审查宗门改动]"),
            "保留原有标题: {out}"
        );
        assert!(out.contains("执行过程（3 步）"), "应标出步数: {out}");
        assert!(out.contains("⚙ 调用 grep"), "应带上每一步: {out}");
        assert!(out.contains("达到回合上限"), "应保留终止原因: {out}");
        assert!(out.contains("── 结果 ──"), "过程与结论要分开: {out}");
    }

    #[test]
    fn subagent_result_without_steps_has_no_empty_shell() {
        // 没有轨迹时不该出现「执行过程（0 步）」这种空壳。
        let out = format_subagent_result("查版本", "1.2.3", &[]);
        assert_eq!(out, "[子任务·查版本]\n1.2.3");
    }

    #[test]
    fn subagent_turn_limit_default_is_not_the_old_hardcoded_15() {
        // 回归：曾硬编码 15，导致子任务几乎必然「达到回合上限」。
        assert_eq!(DEFAULT_SUBAGENT_MAX_ITERATIONS, 100);
        // 设计不变量：子任务上限要低于主会话——子任务常并行好几个，成本是乘出来的。
        const { assert!(DEFAULT_SUBAGENT_MAX_ITERATIONS < DEFAULT_MAX_ITERATIONS_INTERACTIVE) };
    }

    #[test]
    fn fmt_wait_reads_naturally() {
        assert_eq!(fmt_wait(45), "45 秒");
        assert_eq!(fmt_wait(60), "1 分");
        assert_eq!(fmt_wait(90), "1 分 30 秒");
        assert_eq!(fmt_wait(600), "10 分");
    }

    #[test]
    fn retry_msg_shows_remaining_wait_and_attempt() {
        // 用户诉求：撞限流时要看到「在等什么 · 还要等多久 · 第几/共几次」，而不是干转 thinking。
        let m = retry_msg(3, 144, 570, RetryKind::RateLimitLong);
        assert!(m.contains("账号额度限流"), "应点明是账号限流: {m}");
        assert!(m.contains("9 分 30 秒"), "应显示剩余等待: {m}");
        assert!(m.contains("第 3/144 次"), "应显示第几/共几次: {m}");
        // 快退避标为瞬时限流，别吓人。
        assert!(retry_msg(1, 3, 1, RetryKind::RateLimitFast).contains("瞬时限流"));
        // 上游 529 走同一条状态行：点名是上游过载，别让用户去翻自己的配置。
        let s = retry_msg(2, 5, 60, RetryKind::ServerError(529));
        assert!(s.contains("上游过载（529）"), "应点名上游过载: {s}");
        assert!(s.contains("第 2/5 次"), "应显示第几/共几次: {s}");
    }

    #[test]
    fn parallel_only_when_multiple_all_task_and_not_plan_mode() {
        let task = |id: &str| ToolCall {
            id: id.into(),
            name: "task".into(),
            arguments: "{}".into(),
        };
        let other = ToolCall {
            id: "x".into(),
            name: "write_file".into(),
            arguments: "{}".into(),
        };
        assert!(should_run_parallel(&[task("1"), task("2")], false));
        assert!(
            !should_run_parallel(&[task("1")], false),
            "单个 task 走普通路径"
        );
        assert!(
            !should_run_parallel(&[task("1"), other], false),
            "混合调用需顺序执行（保证 fs 工具/确认顺序）"
        );
        assert!(
            !should_run_parallel(&[task("1"), task("2")], true),
            "计划模式必须走 run_tool 的逐个拦截，否则子任务会绕过计划模式直接执行"
        );
    }

    /// 回归：task 工具虽然一直注册着，但 SYSTEM_PROMPT 的工具清单里没提它、也没有派发指引，
    /// 模型就压根不会想到去派子 agent（实测长期一次都没派过）。工具表有 ≠ 模型会用。
    #[test]
    fn system_prompt_advertises_task_delegation() {
        assert!(
            SYSTEM_PROMPT.contains("task ("),
            "task 必须出现在 SYSTEM_PROMPT 的工具清单里，否则模型不知道能派子 agent"
        );
        assert!(
            SYSTEM_PROMPT.contains("DELEGATE"),
            "SYSTEM_PROMPT 必须有明确的派发指引，光列出工具名不够"
        );
        assert!(
            SYSTEM_PROMPT.contains("MULTIPLE task calls in ONE reply"),
            "必须告诉模型独立子任务要一次性发多个 task 才能并行（否则 should_run_parallel 永不触发）"
        );
    }

    /// 回归：喂给模型的提示必须是英文——不是所有模型对中文指令都跟随得好。
    /// 这里只查 CJK 字符，不查语义（注释/日志里的中文不受影响，只查这些常量）。
    #[test]
    fn model_facing_prompts_are_english() {
        let has_cjk = |s: &str| s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
        for (name, text) in [
            ("SYSTEM_PROMPT", SYSTEM_PROMPT),
            ("SOLO_HINT", SOLO_HINT),
            ("PLAN_HINT", PLAN_HINT),
            ("MEMORY_EXTRACT_PROMPT", MEMORY_EXTRACT_PROMPT),
        ] {
            assert!(!has_cjk(text), "{name} 含中文，应改成英文");
        }
        assert!(
            !has_cjk(&task_tool_def().description),
            "task 工具描述含中文，应改成英文"
        );
    }

    #[tokio::test]
    async fn run_turn_intercepts_slash_commands_without_agent() {
        let reg = SessionRegistry::new();
        let agent = Agent::Echo;
        agent
            .run_turn(&reg, "s", "/help".to_string(), vec![], vec![], None, None)
            .await;
        let hist = reg.history("s");
        // 命令回合：user 命令 + 助手回复入历史，且不走 agent（Echo 会回 "echo: ..."）。
        assert_eq!(hist.len(), 2);
        let reply = hist[1].content.as_deref().unwrap();
        assert!(
            reply.contains("/setworkdir"),
            "应返回帮助而非 echo: {reply}"
        );
        assert!(!reply.starts_with("echo:"));
        // 普通消息不受影响，仍走 agent。
        agent
            .run_turn(&reg, "s", "你好".to_string(), vec![], vec![], None, None)
            .await;
        let hist = reg.history("s");
        assert_eq!(hist.last().unwrap().content.as_deref(), Some("echo: 你好"));
        // 回合结束应回到 idle。
        assert_eq!(reg.snapshot("s").unwrap().status.as_deref(), Some("idle"));
    }

    #[test]
    fn fmt_tokens_uses_k_above_thousand() {
        assert_eq!(fmt_tokens(0), "0");
        assert_eq!(fmt_tokens(999), "999");
        assert_eq!(fmt_tokens(1000), "1.0k");
        assert_eq!(fmt_tokens(1234), "1.2k");
        assert_eq!(fmt_tokens(20500), "20.5k");
    }

    #[test]
    fn truncated_by_length_only_on_length() {
        assert!(truncated_by_length(&Some("length".to_string())));
        assert!(!truncated_by_length(&Some("stop".to_string())));
        assert!(!truncated_by_length(&Some("tool_calls".to_string())));
        assert!(!truncated_by_length(&None));
    }

    #[test]
    fn resolve_profile_vision_precedence() {
        use wisecortex_core::config::LlmProfile;
        let base = LlmProfile {
            id: "x".to_string(),
            provider: Some("deepseek".to_string()),
            model: Some("deepseek-v4-pro".to_string()),
            api_key: Some("k".to_string()),
            ..Default::default()
        };
        // 未指定 → 回落 provider 目录白名单：deepseek 无视觉。
        assert!(!resolve_profile(&base, 32768).unwrap().provider.vision);
        // 显式 true → 覆盖目录默认（即便目录里没登记）。
        let mut on = base.clone();
        on.vision = Some(true);
        assert!(resolve_profile(&on, 32768).unwrap().provider.vision);
        // 显式 false → 覆盖目录默认（即便是 claude 这种本应支持的）。
        let mut off = base;
        off.provider = Some("anthropic".to_string());
        off.model = Some("claude-opus-4-8".to_string());
        off.vision = Some(false);
        assert!(!resolve_profile(&off, 32768).unwrap().provider.vision);
    }

    #[test]
    fn resolve_profile_xai_grok_subscription() {
        use wisecortex_core::config::LlmProfile;
        // 订阅档：无 api_key 也可解析，固定 api.x.ai + OpenAi 线缆，thinking 不发（grok-4 系会打回）。
        let p = LlmProfile {
            id: "g".to_string(),
            xai_grok: Some(true),
            ..Default::default()
        };
        let r = resolve_profile(&p, 32768).unwrap();
        assert!(r.provider.use_xai_grok);
        assert_eq!(r.provider.base_url, "https://api.x.ai/v1");
        assert_eq!(r.provider.format, WireFormat::OpenAi);
        assert_eq!(r.provider.thinking, ThinkingMode::None);
        assert_eq!(r.model, "grok-code-fast-1"); // 缺省模型
        assert!(r.provider.vision, "grok 订阅档未指定 vision 时默认吃图");
        // 档内显式指定模型 / 关闭 vision（grok-code-fast 纯文本场景）。
        let p2 = LlmProfile {
            id: "g2".to_string(),
            xai_grok: Some(true),
            model: Some("grok-4-1".to_string()),
            vision: Some(false),
            ..Default::default()
        };
        let r2 = resolve_profile(&p2, 32768).unwrap();
        assert_eq!(r2.model, "grok-4-1");
        assert!(!r2.provider.vision);
    }

    #[test]
    fn resolve_profile_carries_per_model_reasoning_effort() {
        use wisecortex_core::config::LlmProfile;
        let mut p = LlmProfile {
            id: "x".to_string(),
            provider: Some("deepseek".to_string()),
            api_key: Some("k".to_string()),
            ..Default::default()
        };
        // 未设 → None（用全局默认）。
        assert_eq!(resolve_profile(&p, 32768).unwrap().reasoning_effort, None);
        // 设档位 → 原样带出。
        p.reasoning_effort = Some("xhigh".to_string());
        assert_eq!(
            resolve_profile(&p, 32768)
                .unwrap()
                .reasoning_effort
                .as_deref(),
            Some("xhigh")
        );
        // 显式关闭 → Some("")（不回落全局）。
        p.reasoning_effort = Some(String::new());
        assert_eq!(
            resolve_profile(&p, 32768)
                .unwrap()
                .reasoning_effort
                .as_deref(),
            Some("")
        );
    }

    #[test]
    fn pick_effort_prefers_per_model_then_global() {
        assert_eq!(
            pick_effort(Some("high".into()), Some("low".into())).as_deref(),
            Some("high")
        );
        assert_eq!(
            pick_effort(None, Some("low".into())).as_deref(),
            Some("low")
        );
        // 显式关闭（Some("")）优先，不回落全局。
        assert_eq!(
            pick_effort(Some(String::new()), Some("high".into())).as_deref(),
            Some("")
        );
        assert_eq!(pick_effort(None, None), None);
    }

    #[test]
    fn resolve_profile_max_tokens_precedence() {
        use wisecortex_core::config::LlmProfile;
        let base = LlmProfile {
            id: "x".to_string(),
            provider: Some("deepseek".to_string()),
            api_key: Some("k".to_string()),
            ..Default::default()
        };
        // 档内未设 → 用全局默认。
        assert_eq!(resolve_profile(&base, 32768).unwrap().max_tokens, 32768);
        // 档内设置 → 覆盖全局。
        let mut over = base.clone();
        over.max_tokens = Some(8000);
        assert_eq!(resolve_profile(&over, 32768).unwrap().max_tokens, 8000);
        // 过小 → 钳到 256。
        let mut tiny = base;
        tiny.max_tokens = Some(10);
        assert_eq!(resolve_profile(&tiny, 32768).unwrap().max_tokens, 256);
    }

    #[test]
    fn render_recent_history_compacts_and_limits() {
        let mut hist = vec![ChatMessage::system("ignored-too-old")];
        for i in 0..30 {
            hist.push(ChatMessage::user(format!("msg{i}")));
        }
        let out = render_recent_history(&hist);
        // 只取最近 MEMORY_EXTRACT_RECENT 条。
        let lines = out.lines().count();
        assert!(lines <= MEMORY_EXTRACT_RECENT, "应限制条数, got {lines}");
        assert!(out.contains("[user] msg29"), "应含最近消息");
        assert!(!out.contains("ignored-too-old"), "过旧消息应被截掉");
    }

    #[test]
    fn render_recent_history_skips_empty() {
        let hist = vec![ChatMessage::assistant(""), ChatMessage::user("有内容")];
        let out = render_recent_history(&hist);
        assert_eq!(out.trim(), "[user] 有内容");
    }

    #[test]
    fn cap_tool_output_truncates_and_notes() {
        let small = "x".repeat(100);
        // 未超限：原样返回。
        assert_eq!(cap_tool_output(small.clone(), 60_000), small);
        // limit=0：不限。
        let big = "y".repeat(200_000);
        assert_eq!(cap_tool_output(big.clone(), 0).len(), 200_000);
        // 超限：截断到上限内容 + 提示。
        let capped = cap_tool_output(big, 1000);
        assert!(capped.starts_with(&"y".repeat(1000)));
        assert!(capped.contains("已截断"));
        assert!(capped.contains("grep"));
    }

    #[test]
    fn cap_tool_output_respects_char_boundary() {
        // 多字节字符（每个「中」3 字节）；上限落在字符中间应回退到边界，不 panic。
        let s = "中".repeat(100); // 300 字节
        let capped = cap_tool_output(s, 100); // 100 不是 3 的倍数
        assert!(capped.contains("已截断"));
    }
}
