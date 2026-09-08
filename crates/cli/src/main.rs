use clap::{CommandFactory, Parser, Subcommand};

mod tui;
use wisecortex_core::config;

#[derive(Parser)]
#[command(name = "wisecortex", version, about = "WiseCortex CLI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// 管理模型配置（写入配置文件，配一次永久生效）
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// 管理技能（列出 / 导入 / 从 Git 安装 SKILL.md 技能）
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    /// 管理通知通道（飞书 / 企业微信 / QQ-OneBot / webhook）
    Channel {
        #[command(subcommand)]
        action: ChannelAction,
    },
    /// 管理定时任务（间隔 + 内容 + 执行日志）
    Cron {
        #[command(subcommand)]
        action: CronAction,
    },
    /// 配置 IM 双向接入（飞书 / 企业微信凭据）
    Im {
        #[command(subcommand)]
        action: ImAction,
    },
    /// 把旧的按会话记忆并入按项目记忆（一次性迁移，可先 --dry-run 预览）
    MemoryMigrate {
        /// 只打印将要发生什么，不落盘
        #[arg(long)]
        dry_run: bool,
        /// 目标项目记忆已有内容时也照并（默认跳过，避免重跑打乱已裁剪的内容）
        #[arg(long)]
        force: bool,
    },
    /// 查看错误日志（panic / 任务失败等，供自修复参考）
    Doctor {
        /// 读取的字节数（从末尾），默认 4096
        #[arg(long, default_value_t = 4096)]
        bytes: usize,
    },
    /// 启动终端交互客户端（连本机 wisecortex-server：流式对话 / 切会话 / 订阅登录）
    Tui {
        /// 服务端 WS 地址（默认由 WC_BIND 推出，通常 ws://127.0.0.1:7070/ws）
        #[arg(long)]
        url: Option<String>,
        /// 访问密钥（默认取 WC_ACCESS_KEY 环境变量 / 配置文件）
        #[arg(long)]
        access_key: Option<String>,
        /// 直接进入指定会话（默认选最近活跃会话）
        #[arg(long)]
        session: Option<String>,
        /// 若目标端口没在跑，自动拉起 wisecortex-server（同目录）再连；退出时一并关掉
        #[arg(long)]
        serve: bool,
    },
}

#[derive(Subcommand)]
enum ImAction {
    /// 设置飞书自建应用凭据（事件订阅回调用 /api/im/feishu）
    Feishu {
        #[arg(long)]
        app_id: Option<String>,
        #[arg(long)]
        app_secret: Option<String>,
        #[arg(long)]
        verify_token: Option<String>,
    },
    /// 设置企业微信自建应用凭据（回调用 /api/im/wecom）
    Wecom {
        #[arg(long)]
        corp_id: Option<String>,
        /// 本应用的 Secret
        #[arg(long)]
        corp_secret: Option<String>,
        #[arg(long)]
        agent_id: Option<String>,
        /// 回调配置里的 Token
        #[arg(long)]
        token: Option<String>,
        /// 回调配置里的 EncodingAESKey（43 字符）
        #[arg(long)]
        aes_key: Option<String>,
    },
}

#[derive(Subcommand)]
enum ChannelAction {
    /// 列出通道
    List,
    /// 添加/更新通道（同名覆盖）
    Add {
        #[arg(long)]
        name: String,
        /// 类型：feishu | wecom | onebot | webhook
        #[arg(long)]
        kind: String,
        /// webhook URL，或 OneBot HTTP 基地址
        #[arg(long)]
        url: String,
        /// onebot 的群号等目标
        #[arg(long)]
        target: Option<String>,
    },
    /// 删除通道
    Rm { name: String },
}

#[derive(Subcommand)]
enum CronAction {
    /// 列出任务
    List,
    /// 添加任务
    Add {
        #[arg(long)]
        name: String,
        /// 间隔：纯数字=秒，或 30s/5m/1h/2d
        #[arg(long)]
        interval: String,
        /// 任务内容（作为 prompt 交给 agent）
        #[arg(long)]
        prompt: String,
        /// 执行后把结果推到的通道名
        #[arg(long)]
        channel: Option<String>,
        /// 该任务的工作目录（隔离执行；加载其 <workdir>/skills 项目级技能）。留空=全局工作目录
        #[arg(long)]
        workdir: Option<String>,
    },
    /// 删除任务
    Rm {
        id: String,
    },
    /// 启用 / 停用
    Enable {
        id: String,
    },
    Disable {
        id: String,
    },
    /// 查看执行日志
    Logs {
        id: String,
    },
}

#[derive(Subcommand)]
enum SkillAction {
    /// 列出当前可用技能（./skills 与用户数据目录）
    List,
    /// 导入技能目录到项目 ./skills（兼容 Claude Code / openclacky 的 SKILL.md）
    Import {
        /// 源路径：一个含 SKILL.md 的技能目录，或包含多个技能子目录的目录
        source: String,
    },
    /// 从 git 仓库安装技能到 ./skills（如 https://github.com/obra/superpowers）
    AddGit {
        /// 仓库地址（http/https/ssh 均可）
        url: String,
        /// 仓库内技能根目录；缺省优先用仓库根下的 `skills`，否则用仓库根
        #[arg(long)]
        subdir: Option<String>,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// 显示当前配置与文件路径
    Show,
    /// 打印配置文件路径
    Path,
    /// 设置配置项（只更新提供的字段）
    Set {
        /// provider 预设 id：openai | anthropic | deepseek | qwen | gemini
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        model: Option<String>,
        /// 自定义 base_url（BYOK），留空用预设默认
        #[arg(long)]
        base_url: Option<String>,
        /// 自动批准危险操作(写/改/shell)。true=无人值守全自动；false=执行前确认
        #[arg(long)]
        auto_approve: Option<bool>,
        /// 访问密钥（保护 WS/REST）；设为空字符串可清除回到公开模式
        #[arg(long)]
        access_key: Option<String>,
    },
}

fn main() {
    config::migrate_legacy_dirs();
    let cli = Cli::parse();
    match cli.command {
        // 裸跑 `wisecortex`：打印子命令总览（等价 `--help`），比只报版本有用。
        None => {
            let _ = Cli::command().print_help();
            println!();
        }
        Some(Command::Config { action }) => handle_config(action),
        Some(Command::Skill { action }) => handle_skill(action),
        Some(Command::Channel { action }) => handle_channel(action),
        Some(Command::Cron { action }) => handle_cron(action),
        Some(Command::Im { action }) => handle_im(action),
        Some(Command::MemoryMigrate { dry_run, force }) => handle_memory_migrate(dry_run, force),
        Some(Command::Doctor { bytes }) => handle_doctor(bytes),
        Some(Command::Tui {
            url,
            access_key,
            session,
            serve,
        }) => {
            if let Err(e) = tui::run_blocking(url, access_key, session, serve) {
                eprintln!("tui 退出：{e}");
                std::process::exit(1);
            }
        }
    }
}

/// 把旧的**按会话**记忆并入**按项目**记忆。
///
/// 会话记忆只在同一次对话里可见，开新对话就归零；项目记忆按工作目录共享。历史留下的
/// 会话记忆若不搬过来，等于白记。迁移规则：
/// - 按各会话的 `working_dir` 分组（没设的归全局工作目录）——**绝不把不同项目并成一份**，
///   那正是按目录分层要避免的串味。
/// - 组内逐条并入，去重 + 超总量裁剪，但**不做单条限长**（见 `memory::merge_entries`）。
/// - 原会话记忆文件**原样保留**，不删不改。
/// - **已有内容的项目记忆默认跳过**（除非 --force）：超上限的组会裁掉最早的条目，
///   重跑会把它们又塞回来、反把最新的挤出去，并非幂等。
fn handle_memory_migrate(dry_run: bool, force: bool) {
    use wisecortex_core::memory::{self, Kind, Scope};

    let Some(mem_dir) = memory::session_memory_dir() else {
        eprintln!("找不到数据目录。");
        return;
    };
    let Ok(entries) = std::fs::read_dir(&mem_dir) else {
        println!("没有会话记忆可迁移（{} 不存在）。", mem_dir.display());
        return;
    };
    let fallback = wisecortex_core::config::workspace_default()
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    // workdir -> 该目录下所有会话记忆的条目（保持读入顺序，越靠后越新）。
    let mut groups: std::collections::BTreeMap<std::path::PathBuf, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut files = 0usize;
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let Some(sid) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        files += 1;
        let dir = session_workdir_of(sid).unwrap_or_else(|| fallback.clone());
        let items = groups.entry(dir).or_default();
        for line in text.lines() {
            let t = line.trim();
            // 跳过空行、小节标题、以及旧实现留下的截断提示行。
            if t.is_empty() || t.starts_with("## ") || t.starts_with('…') {
                continue;
            }
            items.push(t.strip_prefix("- ").unwrap_or(t).trim().to_string());
        }
    }

    if groups.is_empty() {
        println!("没有会话记忆可迁移。");
        return;
    }
    println!(
        "扫到 {files} 份会话记忆，按工作目录分成 {} 组{}：\n",
        groups.len(),
        if dry_run {
            "（预览，不落盘）"
        } else {
            ""
        }
    );
    for (dir, items) in &groups {
        let existing = memory::read_scope(Scope::Project(dir));
        let before = existing.len();
        if !existing.trim().is_empty() && !force {
            println!(
                "  {}
    已有项目记忆（{before} 字节），跳过。要合并请加 --force",
                dir.display()
            );
            continue;
        }
        // 一律并入 Notes：旧格式没有类别信息，不该替模型猜哪条是「坑」。
        // 用 merge_entries 而非 compose：老条目本就冗长，套单条限长会把 9 成内容腰斩。
        let merged = memory::merge_entries(&existing, items, Kind::Fact);
        let kept = merged.lines().filter(|l| l.starts_with("- ")).count();
        println!(
            "  {}\n    源 {} 条 → 合并去重后留 {} 条（{} → {} 字节）",
            dir.display(),
            items.len(),
            kept,
            before,
            merged.len()
        );
        if !dry_run {
            match memory::write_scope(Scope::Project(dir), &merged) {
                Ok(()) => println!("    ✓ 已写入项目记忆"),
                Err(e) => eprintln!("    ✗ 写入失败：{e}"),
            }
        }
    }
    println!(
        "\n原会话记忆文件未改动。{}",
        if dry_run {
            "去掉 --dry-run 即执行。"
        } else {
            "已迁移的目标再次运行会被跳过（要强行合并加 --force）。
请在「记忆」面板复核，删掉过时或记错的条目。"
        }
    );
}

/// 读某会话持久化文件里的 `config.working_dir`；无文件/未设/目录不存在均返回 None。
fn session_workdir_of(sid: &str) -> Option<std::path::PathBuf> {
    let p = wisecortex_core::config::sessions_dir()?.join(format!("{sid}.json"));
    let text = std::fs::read_to_string(p).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let raw = v.get("config")?.get("working_dir")?.as_str()?.trim();
    let path = std::path::PathBuf::from(raw);
    (!raw.is_empty() && path.is_dir()).then_some(path)
}

fn handle_doctor(bytes: usize) {
    use wisecortex_core::buglog;
    match buglog::log_path() {
        Some(p) => println!("错误日志: {}", p.display()),
        None => println!("(无法定位数据目录)"),
    }
    let recent = buglog::read_recent(bytes);
    if recent.trim().is_empty() {
        println!("(暂无错误记录 ✓)");
    } else {
        println!("\n--- 最近 {bytes} 字节 ---\n{recent}");
    }
}

fn handle_im(action: ImAction) {
    use wisecortex_core::{feishu, wecom};
    match action {
        ImAction::Wecom {
            corp_id,
            corp_secret,
            agent_id,
            token,
            aes_key,
        } => {
            let mut cfg = wecom::load();
            if corp_id.is_some() {
                cfg.corp_id = corp_id;
            }
            if corp_secret.is_some() {
                cfg.corp_secret = corp_secret;
            }
            if agent_id.is_some() {
                cfg.agent_id = agent_id;
            }
            if token.is_some() {
                cfg.callback_token = token;
            }
            if aes_key.is_some() {
                cfg.encoding_aes_key = aes_key;
            }
            match wecom::save(&cfg) {
                Ok(()) => {
                    println!(
                    "已保存企业微信配置（就绪: {}）。回调 URL 填: http://<本机>:7070/api/im/wecom",
                    if cfg.is_ready() { "是" } else { "否，缺字段" }
                )
                }
                Err(e) => {
                    eprintln!("保存失败: {e}");
                    std::process::exit(1);
                }
            }
        }
        ImAction::Feishu {
            app_id,
            app_secret,
            verify_token,
        } => {
            let mut cfg = feishu::load();
            if app_id.is_some() {
                cfg.app_id = app_id;
            }
            if app_secret.is_some() {
                cfg.app_secret = app_secret;
            }
            if verify_token.is_some() {
                cfg.verify_token = verify_token;
            }
            match feishu::save(&cfg) {
                Ok(()) => println!(
                    "已保存飞书配置（app_id {}）。事件订阅 URL 填: http://<本机>:7070/api/im/feishu",
                    if cfg.app_id.is_some() { "已设置" } else { "未设置" }
                ),
                Err(e) => {
                    eprintln!("保存失败: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

fn handle_channel(action: ChannelAction) {
    use wisecortex_core::notify::{self, Channel};
    match action {
        ChannelAction::List => {
            let chs = notify::load_channels();
            if chs.is_empty() {
                println!("(无通道)");
            }
            for c in chs {
                println!(
                    "- {} [{}] {}{}",
                    c.name,
                    c.kind,
                    c.url,
                    c.target.map(|t| format!(" target={t}")).unwrap_or_default()
                );
            }
        }
        ChannelAction::Add {
            name,
            kind,
            url,
            target,
        } => {
            let mut chs = notify::load_channels();
            chs.retain(|c| c.name != name);
            chs.push(Channel {
                name,
                kind,
                url,
                target,
                username: None,
                password: None,
                from: None,
            });
            match notify::save_channels(&chs) {
                Ok(()) => println!("已保存通道"),
                Err(e) => {
                    eprintln!("保存失败: {e}");
                    std::process::exit(1);
                }
            }
        }
        ChannelAction::Rm { name } => {
            let mut chs = notify::load_channels();
            chs.retain(|c| c.name != name);
            let _ = notify::save_channels(&chs);
            println!("已删除通道 {name}");
        }
    }
}

fn handle_cron(action: CronAction) {
    use wisecortex_core::cron::{self, CronTask};
    match action {
        CronAction::List => {
            let tasks = cron::load_tasks();
            if tasks.is_empty() {
                println!("(无任务)");
            }
            for t in tasks {
                let on = if t.enabled { "✓" } else { "✗" };
                let last = t.last_status.as_deref().unwrap_or("-");
                println!(
                    "{on} {} [{}] 每{}s ch={} 上次={last}\n   {}",
                    t.id,
                    t.name,
                    t.interval_secs,
                    t.channel.as_deref().unwrap_or("-"),
                    t.prompt.lines().next().unwrap_or("")
                );
            }
        }
        CronAction::Add {
            name,
            interval,
            prompt,
            channel,
            workdir,
        } => {
            let Some(secs) = cron::parse_duration(&interval) else {
                eprintln!("无效间隔: {interval}（用 数字 或 30s/5m/1h/2d）");
                std::process::exit(1);
            };
            let mut tasks = cron::load_tasks();
            let task = CronTask {
                id: cron::new_id(),
                name,
                interval_secs: secs,
                cron: None,
                prompt,
                channel,
                workdir: workdir
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
                model: None,
                enabled: true,
                last_run: None,
                last_status: None,
                runs: 0,
                last_duration_ms: None,
            };
            let id = task.id.clone();
            tasks.push(task);
            match cron::save_tasks(&tasks) {
                Ok(()) => println!("已创建任务 {id}（每 {secs}s）"),
                Err(e) => {
                    eprintln!("保存失败: {e}");
                    std::process::exit(1);
                }
            }
        }
        CronAction::Rm { id } => {
            let mut tasks = cron::load_tasks();
            tasks.retain(|t| t.id != id);
            let _ = cron::save_tasks(&tasks);
            println!("已删除任务 {id}");
        }
        CronAction::Enable { id } => set_enabled(&id, true),
        CronAction::Disable { id } => set_enabled(&id, false),
        CronAction::Logs { id } => {
            let log = cron::read_log(&id);
            if log.is_empty() {
                println!("(无日志)");
            } else {
                print!("{log}");
            }
        }
    }
}

fn set_enabled(id: &str, enabled: bool) {
    use wisecortex_core::cron;
    let mut tasks = cron::load_tasks();
    let mut found = false;
    for t in tasks.iter_mut() {
        if t.id == id {
            t.enabled = enabled;
            found = true;
        }
    }
    if found {
        let _ = cron::save_tasks(&tasks);
        println!("任务 {id} 已{}", if enabled { "启用" } else { "停用" });
    } else {
        eprintln!("未找到任务 {id}");
        std::process::exit(1);
    }
}

fn handle_skill(action: SkillAction) {
    use std::path::PathBuf;
    use wisecortex_core::marketplace;
    use wisecortex_core::skill::SkillSet;

    match action {
        SkillAction::List => {
            let mut dirs = vec![PathBuf::from("skills")];
            if let Some(d) = wisecortex_core::config::skills_dir() {
                dirs.push(d);
            }
            let set = SkillSet::load_dirs(&dirs);
            let entries = set.entries();
            if entries.is_empty() {
                println!("(无技能。用 `wisecortex skill import <目录>` 导入，或让 agent 创建)");
                return;
            }
            for (name, desc) in entries {
                println!("- {name}: {}", desc.lines().next().unwrap_or(""));
            }
        }
        SkillAction::Import { source } => {
            let src = PathBuf::from(&source);
            if !src.is_dir() {
                eprintln!("源不是目录: {source}");
                std::process::exit(1);
            }
            let imported = marketplace::import_dir(&src, &PathBuf::from("skills"));
            if imported.is_empty() {
                eprintln!("未找到含 SKILL.md 的技能目录");
                std::process::exit(1);
            }
            println!("已导入到 ./skills: {}", imported.join(", "));
            println!("（重启后端后会出现在 AVAILABLE SKILLS；invoke_skill 可立即调用）");
        }
        SkillAction::AddGit { url, subdir } => {
            println!("克隆并导入 {url} …");
            match marketplace::install_from_git(&url, subdir.as_deref(), &PathBuf::from("skills")) {
                Ok(names) => {
                    println!("已从仓库导入到 ./skills: {}", names.join(", "));
                    println!("（重启后端后会出现在 AVAILABLE SKILLS；invoke_skill 可立即调用）");
                }
                Err(e) => {
                    eprintln!("导入失败: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

fn handle_config(action: ConfigAction) {
    match action {
        ConfigAction::Path => match config::config_path() {
            Some(p) => println!("{}", p.display()),
            None => eprintln!("无法定位配置目录"),
        },
        ConfigAction::Show => {
            let cfg = config::load();
            if let Some(p) = config::config_path() {
                println!("配置文件: {}", p.display());
            }
            if cfg.llms.is_empty() {
                println!("(无 LLM 配置档，用 `config set` 添加)");
            }
            for p in &cfg.llms {
                let active = cfg.active_llm.as_deref() == Some(p.id.as_str());
                println!(
                    "{} [{}] {} · provider={} model={} key={}",
                    if active { "*当前*" } else { "      " },
                    p.id,
                    if p.name.is_empty() {
                        "(未命名)"
                    } else {
                        &p.name
                    },
                    p.provider.as_deref().unwrap_or("-"),
                    p.model.as_deref().unwrap_or("-"),
                    if p.has_key() {
                        "已设置"
                    } else {
                        "(未设置)"
                    },
                );
            }
            println!(
                "auto_approve: {} (危险操作{})",
                cfg.auto_approve
                    .map(|b| b.to_string())
                    .unwrap_or_else(|| "(默认 true)".to_string()),
                if cfg.auto_approve == Some(false) {
                    "需确认"
                } else {
                    "自动执行"
                }
            );
            println!(
                "access_key: {}",
                if cfg
                    .access_key
                    .as_deref()
                    .map(|k| !k.is_empty())
                    .unwrap_or(false)
                {
                    "已设置（鉴权开启）"
                } else {
                    "(未设置 = 公开模式)"
                }
            );
        }
        ConfigAction::Set {
            provider,
            api_key,
            model,
            base_url,
            auto_approve,
            access_key,
        } => {
            let mut cfg = config::load();
            // LLM 字段更新到「当前档」（无档则新建）。
            let touched_llm =
                provider.is_some() || api_key.is_some() || model.is_some() || base_url.is_some();
            if touched_llm {
                let mut prof = cfg.active().cloned().unwrap_or_default();
                if prof.name.is_empty() {
                    prof.name = provider.clone().unwrap_or_else(|| "默认".to_string());
                }
                if provider.is_some() {
                    prof.provider = provider;
                }
                if api_key.is_some() {
                    prof.api_key = api_key;
                }
                if model.is_some() {
                    prof.model = model;
                }
                if base_url.is_some() {
                    prof.base_url = base_url;
                }
                cfg.upsert(prof);
            }
            if auto_approve.is_some() {
                cfg.auto_approve = auto_approve;
            }
            if let Some(k) = access_key {
                cfg.access_key = if k.is_empty() { None } else { Some(k) };
            }
            match config::save(&cfg) {
                Ok(path) => println!("已保存到 {}", path.display()),
                Err(e) => {
                    eprintln!("保存失败: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}
