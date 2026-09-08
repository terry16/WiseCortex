//! 持久化的应用配置（配一次，永久生效）。
//!
//! 存放于操作系统标准配置目录下 `wisecortex/config.json`：
//!   - Windows: %APPDATA%\wisecortex\config.json
//!   - macOS:   ~/Library/Application Support/wisecortex/config.json
//!   - Linux:   ~/.config/wisecortex/config.json
//!
//! 支持**多个 LLM 配置档**（`llms`）+ 一个当前档（`active_llm`）；`auto_approve`/`access_key`
//! 为全局项。旧版扁平字段（provider/api_key/model/base_url）会在加载时自动迁移为一个档。
//!
//! 解析优先级（高 → 低）：环境变量 WC_* > 当前档 / 全局配置 > provider 预设默认。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 一个 LLM 配置档。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LlmProfile {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// 输入价格（人民币元 / 百万 tokens）。设置后覆盖内置费率表。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_in: Option<f64>,
    /// 输出价格（人民币元 / 百万 tokens）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_out: Option<f64>,
    /// 缓存命中（cache read）价格（元 / 百万 tokens）。多数 OpenAI/DeepSeek/Anthropic 模型
    /// 缓存读取约为输入价的 1/10；未设时按输入价 × 0.1 估算（而非全价，避免高缓存场景成本虚高）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_cache_read: Option<f64>,
    /// 该模型单次输出 token 上限（max_tokens）。None=用全局默认。写大文件/长输出需要调高；
    /// 但**部分服务商会按模型实际上限拒绝过大的值(HTTP 400)**，按所用模型的真实输出上限设置。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// 为 true 时该档走 **Claude 订阅 OAuth**（用本机已登录的 Pro/Max 订阅额度，不需要 api_key），
    /// 固定走 Anthropic 官方端点。仅个人本机自用；登录见 `/api/oauth/claude/*`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_oauth: Option<bool>,
    /// 为 true 时该档走 **ChatGPT/Codex 订阅 OAuth**（用本机已登录的订阅额度，不需要 api_key），
    /// 固定走 Codex 后端 + Responses 线缆。仅个人本机自用；登录见 `/api/oauth/openai/*`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_codex: Option<bool>,
    /// 为 true 时该档走 **xAI Grok 订阅 OAuth**（SuperGrok 等订阅额度，不需要 api_key），
    /// 固定走 api.x.ai + OpenAI 兼容线缆。仅个人自用；登录见 `/api/oauth/xai/*`（设备码流）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xai_grok: Option<bool>,
    /// 为 true 时该档走 **Gemini 订阅 OAuth**（个人 Google 账号 / Code Assist 免费或付费额度，
    /// 不需要 api_key），固定走 cloudcode-pa 后端 + 原生 Gemini 线缆。仅个人自用；登录见
    /// `/api/oauth/gemini/*`（手动粘贴）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_oauth: Option<bool>,
    /// 该模型是否支持图片输入（vision）。`None`=未指定，按 provider 目录的 `vision_models`
    /// 白名单推断；`Some(true/false)`=用户在模型管理里显式指定（覆盖目录默认）。为 false 时
    /// 发送前会自动剥离图片（见 `llm::client` 的图片降级），避免给纯文本模型发图被打回 4xx。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
    /// 该模型的推理强度 / extended thinking 档位（覆盖全局 `reasoning_effort`）。
    /// `None`=未指定，用全局默认；`Some("")`=显式关闭；`Some("low"|"medium"|"high"|"xhigh"|"max")`
    /// =按档位开启。各家参数差异由线缆层按 provider 的 `ThinkingMode` 自动翻译（见 llm 层）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

impl LlmProfile {
    /// 该档是否配了 api_key。**只表示字面意思**——判断「这个档能不能用」请用
    /// [`is_usable`](Self::is_usable)，订阅档没有 key 但照样能跑。
    pub fn has_key(&self) -> bool {
        self.api_key
            .as_deref()
            .map(|k| !k.is_empty())
            .unwrap_or(false)
    }

    /// 是否订阅档：走本机 OAuth token（Claude / ChatGPT / Grok / Gemini），不需要 api_key。
    pub fn is_subscription(&self) -> bool {
        self.claude_oauth == Some(true)
            || self.openai_codex == Some(true)
            || self.xai_grok == Some(true)
            || self.gemini_oauth == Some(true)
    }

    /// 该档是否**可用**（能真的发起推理）：有 api_key，或是订阅档。
    /// 与 `agent.rs` 里「key 为空且四个订阅开关全关 → 退回 Echo」的判定同源，
    /// 两边必须一致：不然会出现「明明能对话，UI 却说没配置」。
    pub fn is_usable(&self) -> bool {
        self.has_key() || self.is_subscription()
    }
}

/// 联网搜索配置：provider + 可选 api_key，无 key 时走 keyless 兜底。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WebSearchConfig {
    /// duckduckgo | searxng | brave | tavily（空=自动：有 base_url→searxng，否则 duckduckgo）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// brave / tavily 等需要的 API Key。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// 自部署 SearXNG 的基地址（如 https://searx.example.com）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

impl WebSearchConfig {
    /// 解析生效的 provider（小写）。显式优先；否则 base_url→searxng；都无→duckduckgo。
    pub fn resolved_provider(&self) -> String {
        match self
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(p) => p.to_ascii_lowercase(),
            None if self
                .base_url
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false) =>
            {
                "searxng".to_string()
            }
            None => "duckduckgo".to_string(),
        }
    }
}

/// 一个 MCP 服务器配置。对齐 Claude Code 的 `mcpServers` 格式。
/// `url` 有值 = Streamable HTTP 远程传输；否则 = stdio（本地子进程，JSON-RPC over stdin/stdout）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// stdio：启动命令（如 `npx`、`uvx`、可执行文件路径）。
    #[serde(default)]
    pub command: String,
    /// stdio：命令参数。
    #[serde(default)]
    pub args: Vec<String>,
    /// stdio：额外环境变量。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    /// HTTP：Streamable HTTP 端点 URL（如 `https://example.com/mcp`）。设了即走 HTTP 传输。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// HTTP：附加请求头（如 `{"Authorization":"Bearer xxx"}`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    /// 禁用该服务器（保留配置但不连接）。
    #[serde(default)]
    pub disabled: bool,
}

/// 一个 hook：在某事件触发时运行 `command`（shell）。
/// `matcher`（正则，对 PreToolUse/PostToolUse 匹配工具名）为空=匹配全部。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HookConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    #[serde(default)]
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// 落盘的应用配置。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    /// 所有 LLM 配置档。
    #[serde(default)]
    pub llms: Vec<LlmProfile>,
    /// 当前使用的档 id。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_llm: Option<String>,
    /// 是否自动批准危险操作（写/改文件、shell）。None 视为 true（无人值守友好）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_approve: Option<bool>,
    /// 访问密钥；设置后 WS/REST 必须携带。留空=公开模式。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_key: Option<String>,
    /// 全局工作目录（AI 脚本 / 知识库 / 自我学习落脚点）。留空=用三端默认 [`workspace_default`]。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// 出站网络代理（http(s):// 或 socks5://）。设置后所有外网访问（含 LLM、技能/市场）走该代理。
    /// 留空=直连。解析见 [`crate::net`]。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
    /// 联网搜索（web_search 工具）的提供商与凭据。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_search: Option<WebSearchConfig>,
    /// 自动记忆：开启后每隔若干轮在后台从对话抽取要点写入会话记忆（额外 LLM 调用）。默认关闭。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_memory: Option<bool>,
    /// 自动优化上下文：**图片只发一次**——历史里的图片不再随后续每一轮重发，
    /// 压缩上下文时也一并丢弃。None=默认开启。
    ///
    /// 图片是上下文里最贵的东西（一张截图动辄上千 token，且每轮都重发一遍）。
    /// UI 改动、跑测试这类场景，图片看过一次就没用了；少数需要反复比对同一张图的场景
    /// 才需要关掉它。关掉即回到「历史里的图片每轮都原样重发」。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_trim_context: Option<bool>,
    /// 推理强度 / extended thinking：low | medium | high；空/None=关闭（不带 thinking）。
    /// 对 Anthropic 走 thinking+output_config，对 OpenAI 兼容走 reasoning_effort。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    /// 无人值守任务（定时/IM 触发）的工具调用回合上限（防失控）。None=用默认。
    /// 越高单次任务越能跑完，但失控时成本上限也越高。env WC_MAX_ITERATIONS 优先。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<usize>,
    /// 交互式聊天（桌面/网页人工会话）的工具调用回合上限。None=用默认（很高）。
    /// 与 max_iterations 分开：交互开发需要大量工具往返（读→改→编译→测→修…），给一个
    /// 很高的安全上限（默认 1000），既基本不挡开发，又对模型空转留个兜底（你随时可手动停）。
    /// 无人值守任务仍用 max_iterations 防失控成本。env WC_MAX_ITERATIONS_INTERACTIVE 优先。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations_interactive: Option<usize>,
    /// `task` 工具派生的**子 agent** 的工具调用回合上限。None=用默认（100）。
    /// 单独一项而非跟随主会话：子任务常一次并行好几个，跟随主会话那个很高的上限会让
    /// 一次失控的成本成倍放大。env WC_SUBAGENT_MAX_ITERATIONS 优先。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_max_iterations: Option<usize>,
    /// 单次工具输出注入上下文的字节上限（防一次读爬大量源码灌爆上下文）。None=默认；0=不限。
    /// 超限截断并提示模型改用 grep / read_file(offset,limit) 精确获取。env WC_TOOL_OUTPUT_LIMIT 优先。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_output_limit: Option<usize>,
    /// 全局单次输出 token 上限（max_tokens）默认值。None=用内置默认（32768）。太小会把大文件写入的
    /// 工具参数截断成残缺 JSON 而报错。可被单个模型档的 max_tokens 覆盖。env WC_MAX_TOKENS 优先。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// 是否自动清理定时任务执行日志。None=默认开启；Some(false)=永久保留（自己管）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron_log_auto_clean: Option<bool>,
    /// 单个定时任务日志文件的体积上限（MB）。超过则从最旧的「整次运行」开始裁剪，
    /// 至少保留最近一次。None=默认 10 MB。仅在 `cron_log_auto_clean` 开启时生效。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron_log_max_mb: Option<u32>,
    /// MCP 服务器（名字 → 配置）。连接后其工具以 `mcp__<名字>__<工具>` 暴露给 agent。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mcp_servers: BTreeMap<String, McpServerConfig>,
    /// 暴露给 MCP 服务器的 roots（目录/URI）。空=默认用当前工作目录。响应 `roots/list` 用。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_roots: Vec<String>,
    /// 事件钩子（事件名 → 钩子列表）。事件：PreToolUse / PostToolUse / UserPromptSubmit /
    /// SessionStart / Stop。详见 [`crate::hooks`]。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hooks: BTreeMap<String, Vec<HookConfig>>,
    /// QQ OneBot：允许执行 `/` 命令的 QQ 号白名单。**空=不限制**（自用群保持原样）。
    ///
    /// 只管命令，不管普通对话——客服群里的客户照常提问，只是指挥不动机器人：
    /// 切会话、改工作目录、清上下文这些都得是白名单里的人。
    /// env `WC_ONEBOT_ADMINS`（逗号分隔）优先于本字段。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub onebot_admins: Vec<i64>,

    // ── 旧版扁平字段：仅用于从老配置迁移，读入后转入 llms，不再写出。 ──
    #[serde(default, skip_serializing)]
    provider: Option<String>,
    #[serde(default, skip_serializing)]
    api_key: Option<String>,
    #[serde(default, skip_serializing)]
    model: Option<String>,
    #[serde(default, skip_serializing)]
    base_url: Option<String>,
}

/// 单个定时任务日志文件的默认体积上限（MB，未配置时）。
pub const CRON_LOG_MAX_MB_DEFAULT: u32 = 10;

impl AppConfig {
    /// 生效的单任务日志体积上限（字节）。关闭自动清理时返回 0，调用方据此整体跳过。
    /// 配 0 MB 会被抬到 1 MB——避免「上限 0」被理解成把日志清空。
    pub fn cron_log_max_bytes_effective(&self) -> u64 {
        if !self.cron_log_auto_clean.unwrap_or(true) {
            return 0;
        }
        let mb = self
            .cron_log_max_mb
            .unwrap_or(CRON_LOG_MAX_MB_DEFAULT)
            .max(1);
        mb as u64 * 1024 * 1024
    }

    /// 是否开启「自动优化上下文」（图片只发一次）。未配置=开启。
    pub fn auto_trim_context_effective(&self) -> bool {
        self.auto_trim_context.unwrap_or(true)
    }

    /// 当前生效的配置档：优先 active_llm 指向的，否则第一个。
    pub fn active(&self) -> Option<&LlmProfile> {
        match &self.active_llm {
            Some(id) => self
                .llms
                .iter()
                .find(|p| &p.id == id)
                .or_else(|| self.llms.first()),
            None => self.llms.first(),
        }
    }

    /// 新增或更新一个档（按 id 匹配；id 为空则生成新 id）。返回该档 id。
    /// 列表原本为空时，自动把新档设为当前档。
    pub fn upsert(&mut self, mut profile: LlmProfile) -> String {
        if profile.id.is_empty() {
            profile.id = new_id();
        }
        let id = profile.id.clone();
        let was_empty = self.llms.is_empty();
        match self.llms.iter_mut().find(|p| p.id == id) {
            Some(slot) => *slot = profile,
            None => self.llms.push(profile),
        }
        if was_empty {
            self.active_llm = Some(id.clone());
        }
        id
    }

    /// 当前生效的全局工作目录：配置覆盖优先，否则三端默认 [`workspace_default`]。
    pub fn workspace_dir(&self) -> Option<PathBuf> {
        match self
            .workspace
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(p) => Some(PathBuf::from(p)),
            None => workspace_default(),
        }
    }

    /// 删除一个档；若删的是当前档，当前档回退到第一个（或无）。
    pub fn remove(&mut self, id: &str) {
        self.llms.retain(|p| p.id != id);
        if self.active_llm.as_deref() == Some(id) {
            self.active_llm = self.llms.first().map(|p| p.id.clone());
        }
    }

    /// 把旧版扁平字段迁移为一个档（仅当还没有任何档且存在旧字段时）。
    fn migrate(mut self) -> Self {
        if self.llms.is_empty() && (self.provider.is_some() || self.api_key.is_some()) {
            let id = "default".to_string();
            let name = self.provider.clone().unwrap_or_else(|| "默认".to_string());
            self.llms.push(LlmProfile {
                id: id.clone(),
                name,
                provider: self.provider.clone(),
                model: self.model.clone(),
                base_url: self.base_url.clone(),
                api_key: self.api_key.clone(),
                ..Default::default()
            });
            self.active_llm = Some(id);
        }
        // 清空旧字段（已迁移，且不再写出）。
        self.provider = None;
        self.api_key = None;
        self.model = None;
        self.base_url = None;
        self
    }
}

/// 生成一个 LLM 档 id（时间戳纳秒，足够唯一）。
pub fn new_id() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("llm-{n}")
}

/// 配置文件路径（无法定位配置目录时返回 None）。
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("wisecortex").join("config.json"))
}

/// 旧品牌（WiseClaw）的目录名。仅用于一次性迁移，不再写入。
const LEGACY_DIR: &str = "wiseclaw";
const CURRENT_DIR: &str = "wisecortex";

/// 把旧品牌目录整体迁到新目录：仅在「新目录不存在且旧目录存在」时执行一次。
///
/// 改名 WiseClaw → WiseCortex 后，老用户的 config/sessions/skills/workspace 都还在
/// `.../wiseclaw/` 下。这里做一次搬迁（rename，同盘零拷贝），失败则退回逐项复制；
/// 两者都失败就静默放弃——大不了当新装用户，不能让启动因此挂掉。
///
/// 返回是否实际执行了迁移，便于日志与测试断言。
fn migrate_legacy_dir(base: &std::path::Path) -> bool {
    let old = base.join(LEGACY_DIR);
    let new = base.join(CURRENT_DIR);
    // 新目录已存在说明要么早迁过、要么本就是新装，一律不动——绝不覆盖现有数据。
    if !old.is_dir() || new.exists() {
        return false;
    }
    if std::fs::rename(&old, &new).is_ok() {
        return true;
    }
    // 跨卷或被占用时 rename 会失败，退化为复制（保留旧目录，留回滚余地）。
    copy_dir_all(&old, &new).is_ok()
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// 启动时调用一次：迁移旧品牌目录。
///
/// 需要覆盖三个 base——config_dir / data_dir / home_dir（Linux 的 workspace 落在家目录）。
/// 三者在部分平台会重合（macOS 上 config_dir == data_dir），故先去重再逐个处理，
/// 否则第二次调用会看到「新目录已存在」而空转，或在极端情况下重复拷贝。
pub fn migrate_legacy_dirs() {
    let mut bases: Vec<PathBuf> = Vec::new();
    for d in [dirs::config_dir(), dirs::data_dir(), dirs::home_dir()]
        .into_iter()
        .flatten()
    {
        if !bases.contains(&d) {
            bases.push(d);
        }
    }
    for b in bases {
        if migrate_legacy_dir(&b) {
            crate::sprintln!(
                "已迁移旧版配置目录：{} → {}",
                b.join(LEGACY_DIR).display(),
                b.join(CURRENT_DIR).display()
            );
        }
    }
}

/// 全局工作目录的三端默认（必须可写，不能放安装目录）：
///   - Windows: `%APPDATA%\wisecortex\workspace`
///   - macOS:   `~/Library/Application Support/wisecortex/workspace`
///   - Linux:   `~/wisecortex/workspace`（家目录，可见好找）
pub fn workspace_default() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        dirs::home_dir().map(|d| d.join("wisecortex").join("workspace"))
    } else {
        dirs::data_dir().map(|d| d.join("wisecortex").join("workspace"))
    }
}

/// 会话持久化目录（每个会话一个 JSON 文件）。
pub fn sessions_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("sessions"))
}

/// 用户级技能目录（每个子目录一个 SKILL.md）。
pub fn skills_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("wisecortex").join("skills"))
}

/// 从标准路径加载；不存在或损坏时返回默认空配置。
pub fn load() -> AppConfig {
    config_path().map(|p| load_from(&p)).unwrap_or_default()
}

/// 解析逗号分隔的 QQ 号列表（纯函数）。空段与非法数字被丢弃。
pub fn parse_admin_list(s: &str) -> Vec<i64> {
    s.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<i64>().ok())
        .collect()
}

/// OneBot 命令白名单：`None`=未配置（不限制），`Some(list)`=只有名单内的 QQ 号能执行 `/` 命令。
///
/// env `WC_ONEBOT_ADMINS`（逗号分隔）优先于配置文件。**故意 fail-closed**：环境变量填了
/// 但一个都解析不出来（比如照抄成 `WC_ONEBOT_ADMINS=你的QQ号`），返回 `Some(vec![])`
/// ——谁都执行不了命令，而不是悄悄退回「谁都能执行」。安全开关配错了必须立刻显形。
pub fn onebot_admins() -> Option<Vec<i64>> {
    if let Ok(raw) = std::env::var("WC_ONEBOT_ADMINS") {
        if !raw.trim().is_empty() {
            let list = parse_admin_list(&raw);
            if list.is_empty() {
                // 不打印原值：那是一串 QQ 号，日志里没必要留。
                crate::buglog::record(
                    "onebot",
                    "WC_ONEBOT_ADMINS 解析不出任何 QQ 号，已按「禁止所有人执行命令」处理",
                );
            }
            return Some(list);
        }
    }
    let list = load().onebot_admins;
    (!list.is_empty()).then_some(list)
}

/// 从指定路径加载（可测试）。自动迁移旧版扁平配置。
pub fn load_from(path: &Path) -> AppConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<AppConfig>(&t).ok())
        .unwrap_or_default()
        .migrate()
}

/// 写入标准路径，返回写入的路径。
pub fn save(cfg: &AppConfig) -> std::io::Result<PathBuf> {
    let path = config_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "无法定位配置目录"))?;
    save_to(&path, cfg)?;
    Ok(path)
}

/// 写入指定路径（自动创建父目录，可测试）。
pub fn save_to(path: &Path, cfg: &AppConfig) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(cfg)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str, name: &str) -> LlmProfile {
        LlmProfile {
            id: id.into(),
            name: name.into(),
            provider: Some("deepseek".into()),
            model: Some("deepseek-v4-pro".into()),
            api_key: Some("sk-x".into()),
            ..Default::default()
        }
    }

    /// 回归：订阅档（OAuth）没有 api_key，但**完全可用**。
    /// 曾经 `/api/config` 的「当前档是否可用」直接取 `has_key()`，于是订阅档被判成
    /// 「没配好」，客户端每次启动都强制跳到设置页。
    #[test]
    fn subscription_profiles_are_usable_without_api_key() {
        let cases = [
            (
                "claude_oauth",
                LlmProfile {
                    claude_oauth: Some(true),
                    ..Default::default()
                },
            ),
            (
                "openai_codex",
                LlmProfile {
                    openai_codex: Some(true),
                    ..Default::default()
                },
            ),
            (
                "xai_grok",
                LlmProfile {
                    xai_grok: Some(true),
                    ..Default::default()
                },
            ),
            (
                "gemini_oauth",
                LlmProfile {
                    gemini_oauth: Some(true),
                    ..Default::default()
                },
            ),
        ];
        for (name, p) in cases {
            assert!(!p.has_key(), "{name}：订阅档本来就没有 api_key");
            assert!(p.is_subscription(), "{name} 应被认作订阅档");
            assert!(
                p.is_usable(),
                "{name}：订阅档必须算可用，否则会被当成未配置"
            );
        }
    }

    #[test]
    fn usable_covers_key_profiles_and_rejects_empty_ones() {
        let keyed = profile("a", "甲"); // 带 api_key
        assert!(keyed.has_key());
        assert!(!keyed.is_subscription());
        assert!(keyed.is_usable(), "有 key 的普通档当然可用");

        let empty = LlmProfile::default();
        assert!(
            !empty.is_usable(),
            "没 key 又不是订阅档 → 不可用，该去设置页"
        );

        // 显式关掉的订阅开关不算订阅档。
        let off = LlmProfile {
            claude_oauth: Some(false),
            gemini_oauth: Some(false),
            ..Default::default()
        };
        assert!(!off.is_subscription());
        assert!(!off.is_usable());
    }

    #[test]
    fn save_then_load_roundtrips_multi_profiles() {
        let dir = std::env::temp_dir().join(format!("wisecortex-cfg-{}-rt", std::process::id()));
        let path = dir.join("config.json");
        let mut cfg = AppConfig::default();
        cfg.upsert(profile("a", "甲"));
        cfg.upsert(profile("b", "乙"));
        cfg.active_llm = Some("b".into());
        cfg.auto_approve = Some(false);

        save_to(&path, &cfg).unwrap();
        let back = load_from(&path);
        assert_eq!(back.llms.len(), 2);
        assert_eq!(back.active().unwrap().id, "b");
        assert_eq!(back.auto_approve, Some(false));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn migrates_legacy_flat_config() {
        let dir = std::env::temp_dir().join(format!("wisecortex-cfg-{}-mig", std::process::id()));
        let path = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        // 旧版扁平 config.json。
        std::fs::write(
            &path,
            r#"{"provider":"deepseek","api_key":"sk-old","model":"deepseek-v4-pro","auto_approve":false}"#,
        )
        .unwrap();

        let cfg = load_from(&path);
        assert_eq!(cfg.llms.len(), 1, "旧扁平配置应迁移成一个档");
        let p = cfg.active().unwrap();
        assert_eq!(p.provider.as_deref(), Some("deepseek"));
        assert_eq!(p.api_key.as_deref(), Some("sk-old"));
        assert_eq!(cfg.auto_approve, Some(false));
        // 再保存后不应再写出旧扁平字段。
        let saved = serde_json::to_string(&cfg).unwrap();
        assert!(saved.contains("\"llms\""));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn upsert_sets_active_when_first_and_updates_in_place() {
        let mut cfg = AppConfig::default();
        let id = cfg.upsert(profile("", "新档")); // 空 id → 生成
        assert!(!id.is_empty());
        assert_eq!(
            cfg.active_llm.as_deref(),
            Some(id.as_str()),
            "首个档应设为当前"
        );
        // 同 id 更新（不新增）。
        let mut upd = profile(&id, "改名");
        upd.model = Some("deepseek-v4-flash".into());
        cfg.upsert(upd);
        assert_eq!(cfg.llms.len(), 1);
        assert_eq!(cfg.active().unwrap().name, "改名");
    }

    #[test]
    fn remove_fixes_active() {
        let mut cfg = AppConfig::default();
        cfg.upsert(profile("a", "甲"));
        cfg.upsert(profile("b", "乙"));
        cfg.active_llm = Some("a".into());
        cfg.remove("a");
        assert_eq!(cfg.llms.len(), 1);
        assert_eq!(
            cfg.active_llm.as_deref(),
            Some("b"),
            "删当前档后回退到剩下的"
        );
    }

    #[test]
    fn missing_file_yields_default() {
        let path = std::env::temp_dir().join("wisecortex-nope-xyz.json");
        assert_eq!(load_from(&path), AppConfig::default());
    }

    #[test]
    fn workspace_dir_uses_override_then_default() {
        // 覆盖优先。
        let mut cfg = AppConfig {
            workspace: Some("/tmp/my-ws".into()),
            ..Default::default()
        };
        assert_eq!(cfg.workspace_dir(), Some(PathBuf::from("/tmp/my-ws")));
        // 空串/空白视为未设置 → 回退默认（与 workspace_default 一致）。
        cfg.workspace = Some("  ".into());
        assert_eq!(cfg.workspace_dir(), workspace_default());
        cfg.workspace = None;
        assert_eq!(cfg.workspace_dir(), workspace_default());
    }

    #[test]
    fn workspace_default_ends_with_wisecortex_workspace() {
        // 各平台都应落在 .../wisecortex/workspace。
        if let Some(p) = workspace_default() {
            assert!(p.ends_with("workspace"));
            assert!(p.parent().unwrap().ends_with("wisecortex"));
        }
    }

    // ── 旧品牌目录迁移（WiseClaw → WiseCortex）─────────────────────────────
    // 这条路径只在老用户升级时跑一次，出错就意味着「配置全丢」，必须锁死行为。

    #[test]
    fn legacy_dir_is_moved_when_target_is_absent() {
        let base = std::env::temp_dir().join(format!("wc-mig-move-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let old = base.join("wiseclaw");
        std::fs::create_dir_all(old.join("sessions")).unwrap();
        std::fs::write(old.join("config.json"), b"{\"access_key\":\"k\"}").unwrap();
        std::fs::write(old.join("sessions").join("a.json"), b"{}").unwrap();

        assert!(migrate_legacy_dir(&base), "应报告已迁移");

        let new = base.join("wisecortex");
        assert!(new.join("config.json").is_file(), "配置文件要跟着搬过来");
        assert!(
            new.join("sessions").join("a.json").is_file(),
            "子目录也要搬"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn existing_target_is_never_clobbered() {
        // 新目录已有数据时必须原地不动——否则升级会覆盖用户当前配置。
        let base = std::env::temp_dir().join(format!("wc-mig-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("wiseclaw")).unwrap();
        std::fs::write(base.join("wiseclaw").join("config.json"), b"OLD").unwrap();
        std::fs::create_dir_all(base.join("wisecortex")).unwrap();
        std::fs::write(base.join("wisecortex").join("config.json"), b"NEW").unwrap();

        assert!(!migrate_legacy_dir(&base), "目标已存在时不应迁移");

        let kept = std::fs::read(base.join("wisecortex").join("config.json")).unwrap();
        assert_eq!(kept, b"NEW", "现有配置不能被旧数据覆盖");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn missing_legacy_dir_is_a_noop() {
        let base = std::env::temp_dir().join(format!("wc-mig-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        assert!(!migrate_legacy_dir(&base), "没有旧目录时什么都不做");
        assert!(!base.join("wisecortex").exists(), "不应凭空建出新目录");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn web_search_provider_resolution() {
        let ws = WebSearchConfig::default();
        assert_eq!(ws.resolved_provider(), "duckduckgo");
        let ws = WebSearchConfig {
            base_url: Some("https://searx.example.com".into()),
            ..Default::default()
        };
        assert_eq!(ws.resolved_provider(), "searxng");
        let ws = WebSearchConfig {
            provider: Some("Brave".into()),
            api_key: Some("k".into()),
            ..Default::default()
        };
        assert_eq!(ws.resolved_provider(), "brave");
    }

    #[test]
    fn workspace_roundtrips_through_save_load() {
        let dir = std::env::temp_dir().join(format!("wisecortex-cfg-{}-ws", std::process::id()));
        let path = dir.join("config.json");
        let cfg = AppConfig {
            workspace: Some("/data/work".into()),
            ..Default::default()
        };
        save_to(&path, &cfg).unwrap();
        assert_eq!(load_from(&path).workspace.as_deref(), Some("/data/work"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
