//! 聊天框命令：以 `/` 开头的已知命令由服务端直接执行，不进 LLM。
//! 在 `run_turn` 入口拦截，故 Web / 飞书 /（后续 QQ、企微）全渠道统一生效。
//! 未知的 `/xxx` 原样交给 LLM（避免误伤「/etc/hosts 怎么改」这类正常提问）。

use serde_json::{json, Value};
use wisecortex_core::config::{self, LlmProfile};
use wisecortex_core::llm::ChatMessage;

use crate::proto::Session;
use crate::registry::{ImOrigin, SessionRegistry};

/// 命令的执行结果。`text` 永远有（进历史、Web 显示、以及不支持卡片的渠道的回落）；
/// `card` 仅在 IM 且该命令有富交互版本时给出（飞书互动卡片，带按钮）。
pub struct CommandOutcome {
    pub text: String,
    pub card: Option<Value>,
}

/// `/sessions` 一次最多列多少条：IM（尤其手机）上刷屏没法看，超出只列最近活跃的这些。
const SESSIONS_LIMIT: usize = 20;

#[derive(Debug)]
pub(crate) enum Cmd {
    Help,
    Workdir,
    SetWorkdir(String),
    Reset,
    /// None=查看当前模型；Some=按 id/名称/模型名切换。
    Model(Option<String>),
    ModelList,
    /// 列出全部会话（带序号），并记下序号快照供 /switch 寻址。
    Sessions,
    /// 按 /sessions 的序号切换本 IM 聊天当前驱动的会话。
    Switch(String),
}

const HELP: &str = "可用命令：\n\
    /sessions — 列出所有会话（带序号）\n\
    /switch <序号> — 切换到某个会话（序号来自 /sessions；仅 IM 可用）\n\
    /setworkdir <路径> — 设置本任务的工作目录\n\
    /workdir — 查看当前生效的工作目录\n\
    /model <id或名称> — 切换本任务使用的模型（不带参数=查看当前）\n\
    /modellist — 列出已配置的模型\n\
    /reset — 清空本会话上下文，重新开始\n\
    /help — 显示本帮助";

/// 把单行 `/xxx args` 解析为已知命令；未知/多行/普通文本返回 None（交给 LLM）。
pub(crate) fn parse(text: &str) -> Option<Cmd> {
    let t = text.trim();
    if !t.starts_with('/') || t.lines().count() > 1 {
        return None;
    }
    let (head, rest) = match t.split_once(char::is_whitespace) {
        Some((h, r)) => (h, r.trim()),
        None => (t, ""),
    };
    match head.to_ascii_lowercase().as_str() {
        "/help" => Some(Cmd::Help),
        "/workdir" => Some(Cmd::Workdir),
        "/setworkdir" => Some(Cmd::SetWorkdir(rest.to_string())),
        "/reset" => Some(Cmd::Reset),
        "/model" => Some(Cmd::Model((!rest.is_empty()).then(|| rest.to_string()))),
        "/modellist" => Some(Cmd::ModelList),
        "/sessions" => Some(Cmd::Sessions),
        "/switch" => Some(Cmd::Switch(rest.to_string())),
        _ => None,
    }
}

/// 档位显示名：优先自定义名称，其次模型名，最后 id。
fn display(p: &LlmProfile) -> String {
    if !p.name.trim().is_empty() {
        return p.name.clone();
    }
    p.model.clone().unwrap_or_else(|| p.id.clone())
}

/// 一行档位描述："名称（模型名, id=xx）"。
fn describe(p: &LlmProfile) -> String {
    let model = p.model.as_deref().unwrap_or("?");
    format!("{}（{}, id={}）", display(p), model, p.id)
}

/// 会话当前生效的档：任务覆盖优先，否则全局 active，最后第一个档。
fn effective<'a>(
    llms: &'a [LlmProfile],
    active: Option<&str>,
    task_model: Option<&str>,
) -> Option<&'a LlmProfile> {
    task_model
        .and_then(|id| llms.iter().find(|p| p.id == id))
        .or_else(|| active.and_then(|id| llms.iter().find(|p| p.id == id)))
        .or_else(|| llms.first())
}

/// `/model` 纯逻辑：返回（要写入任务配置的新 model_id，回复文本）。
/// arg=None 查看当前；arg=Some 按 id/名称/模型名精确匹配切换。
pub(crate) fn model_switch_reply(
    llms: &[LlmProfile],
    active: Option<&str>,
    task_model: Option<&str>,
    arg: Option<&str>,
) -> (Option<String>, String) {
    let Some(q) = arg else {
        let reply = match effective(llms, active, task_model) {
            Some(p) if task_model.is_some() => {
                format!(
                    "当前模型：{}（任务级覆盖）。用 /model <id或名称> 切换。",
                    describe(p)
                )
            }
            Some(p) => format!(
                "当前模型：{}（全局默认）。用 /model <id或名称> 切换。",
                describe(p)
            ),
            None => "尚未配置任何模型，请先到「模型管理」添加。".to_string(),
        };
        return (None, reply);
    };
    match llms
        .iter()
        .find(|p| p.id == q || p.name == q || p.model.as_deref() == Some(q))
    {
        Some(p) => (
            Some(p.id.clone()),
            format!("本任务模型已切换为：{}", describe(p)),
        ),
        None => {
            let list = llms
                .iter()
                .map(|p| format!("  {}", describe(p)))
                .collect::<Vec<_>>();
            (
                None,
                format!("没有找到模型「{q}」。可用：\n{}", list.join("\n")),
            )
        }
    }
}

/// `/modellist` 纯逻辑：逐行列出档位，▶ 标记当前生效档。
pub(crate) fn model_list_reply(llms: &[LlmProfile], effective_id: Option<&str>) -> String {
    if llms.is_empty() {
        return "尚未配置任何模型，请先到「模型管理」添加。".to_string();
    }
    let lines: Vec<String> = llms
        .iter()
        .map(|p| {
            let mark = if Some(p.id.as_str()) == effective_id {
                "▶"
            } else {
                "  "
            };
            format!("{mark} {}", describe(p))
        })
        .collect();
    format!(
        "已配置的模型：\n{}\n用 /model <id或名称> 切换本任务模型。",
        lines.join("\n")
    )
}

/// `/sessions` 纯逻辑：把（调用方已按最近活跃倒序排好的）会话渲染成带序号的列表，
/// `▶` 标出当前绑定的会话。除文本外还返回「序号 → 会话 id」的**快照**：`/switch <n>` 只认
/// 这份快照，因此即便之后列表重排（执行命令本身就会刷新当前会话的活跃时间、把它顶到最前），
/// 用户看到的序号依然指向他当时看到的那个会话。
pub(crate) fn sessions_list_reply(
    sessions: &[Session],
    current_sid: &str,
    limit: usize,
) -> (String, Vec<String>) {
    if sessions.is_empty() {
        return ("还没有任何会话。".to_string(), Vec::new());
    }
    let total = sessions.len();
    let mut lines = Vec::new();
    let mut sids = Vec::new();
    for (i, s) in sessions.iter().take(limit).enumerate() {
        let mark = if s.id == current_sid { "▶" } else { " " };
        let name = s
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or("(未命名)");
        let status = s.status.as_deref().unwrap_or("idle");
        // 工作目录要露出来：远程切过去干活前，得知道这个会话在哪个目录。
        let wd = match s.extra.get("working_dir").and_then(|v| v.as_str()) {
            Some(w) if !w.trim().is_empty() => format!(" — {w}"),
            _ => String::new(),
        };
        lines.push(format!("{mark} {}. {name}[{status}]{wd}", i + 1));
        sids.push(s.id.clone());
    }
    let head = if total > limit {
        format!("会话（共 {total} 个，仅列最近活跃的 {limit} 个）：")
    } else {
        format!("会话（共 {total} 个）：")
    };
    (
        format!(
            "{head}\n{}\n用 /switch <序号> 切换到某个会话。",
            lines.join("\n")
        ),
        sids,
    )
}

/// `/sessions` 的**互动卡片**版（飞书）：每个可切换的会话配一个按钮，点一下就切，
/// 不用再打 `/switch 3`。
///
/// 必须是**卡片 JSON 2.0**：1.0 的按钮回调走「消息卡片回传交互（旧）」，而那个
/// **不支持长连接**——CGNAT 后面的机器根本收不到点击事件，按钮就是死的。2.0 的
/// `behaviors: [{type:"callback", value:{…}}]` 才会触发 `card.action.trigger`，
/// 而它可以从我们已有的那条外拨长连接送回来。
pub(crate) fn sessions_card(
    sessions: &[Session],
    current_sid: &str,
    limit: usize,
) -> serde_json::Value {
    let total = sessions.len();
    let mut elements: Vec<serde_json::Value> = Vec::new();
    for (i, s) in sessions.iter().take(limit).enumerate() {
        let is_current = s.id == current_sid;
        let name = s
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or("(未命名)");
        let status = s.status.as_deref().unwrap_or("idle");
        let wd = match s.extra.get("working_dir").and_then(|v| v.as_str()) {
            Some(w) if !w.trim().is_empty() => format!("\n{w}"),
            _ => String::new(),
        };
        let mark = if is_current { "▶ " } else { "" };
        elements.push(json!({
            "tag": "markdown",
            "content": format!("{mark}**{}. {name}**  `{status}`{wd}", i + 1),
        }));
        // 当前会话不给按钮——已经在它上面了，再放一个「切换」纯属噪音。
        if !is_current {
            elements.push(json!({
                "tag": "button",
                "text": { "tag": "plain_text", "content": "切到这个会话" },
                "type": "primary",
                "behaviors": [{
                    "type": "callback",
                    "value": { "action": "switch", "sid": s.id },
                }],
            }));
        }
        elements.push(json!({ "tag": "hr" }));
    }
    let title = if total > limit {
        format!("会话（共 {total} 个，列最近活跃的 {limit} 个）")
    } else {
        format!("会话（共 {total} 个）")
    };
    json!({
        "schema": "2.0",
        "config": { "wide_screen_mode": true },
        "header": { "title": { "tag": "plain_text", "content": title } },
        "body": { "elements": elements },
    })
}

/// `/switch` 纯逻辑：按上次 `/sessions` 的快照顺序，用 1 基序号定位会话 id。
/// 快照为空（没先列表）/ 非数字 / 0 / 越界 → Err(给用户看的提示，一律引导回 `/sessions`)。
pub(crate) fn switch_resolve(listing: &[String], arg: &str) -> Result<String, String> {
    if listing.is_empty() {
        return Err("请先用 /sessions 列出会话，再用 /switch <序号> 切换。".to_string());
    }
    let n: usize = arg.trim().parse().map_err(|_| {
        format!(
            "用法：/switch <序号>（1-{}）。序号来自 /sessions 列出的列表。",
            listing.len()
        )
    })?;
    if n == 0 || n > listing.len() {
        return Err(format!(
            "序号 {n} 超出范围（1-{}）。请重新运行 /sessions 查看最新列表。",
            listing.len()
        ));
    }
    Ok(listing[n - 1].clone())
}

/// 入口：`text` 是已知命令则执行（含历史写入），返回回复文本；否则 None（正常走 LLM）。
/// 历史语义：命令与回复成对追加（user 在前，保证交替合法）；`/reset` 先清空再追加本对。
///
/// `origin`=消息来自哪个 IM 聊天（Web/WS 传 None）。只有 `/switch` 需要它——它要改的是
/// 「这个 IM 聊天当前驱动哪个会话」，而光凭 `sid` 反推不出来源聊天（切换之后 sid 就不再是
/// `feishu-<chat_id>` 了）。
pub fn try_execute(
    reg: &SessionRegistry,
    sid: &str,
    text: &str,
    origin: Option<&ImOrigin>,
) -> Option<CommandOutcome> {
    let cmd = parse(text)?;
    reg.ensure(sid);
    let mut card: Option<Value> = None;
    let reply = match cmd {
        Cmd::Help => HELP.to_string(),
        Cmd::Sessions => {
            // 注意取列表要在下面写历史之前：命令自身的 user/assistant 追加会刷新当前会话的
            // 活跃时间、把它顶到列表最前。快照存的是「用户此刻看到的那个顺序」。
            let sessions = reg.list();
            let (reply, sids) = sessions_list_reply(&sessions, sid, SESSIONS_LIMIT);
            if let Some(o) = origin {
                reg.set_listing(o, sids);
                // IM 里改发互动卡片：每个会话一个按钮，点一下就切。文本版仍然生成——
                // 它要进历史，也是发卡片失败时的回落。
                card = Some(sessions_card(&sessions, sid, SESSIONS_LIMIT));
            }
            reply
        }
        Cmd::Switch(arg) => match origin {
            None => {
                "/switch 仅在 IM（如飞书）中可用；网页端请直接在左侧会话列表里点选。".to_string()
            }
            Some(o) => match switch_resolve(&reg.listing(o), arg.trim()) {
                Err(msg) => msg,
                Ok(target) => {
                    if !reg.exists(&target) {
                        "该会话已不存在，请重新运行 /sessions 查看最新列表。".to_string()
                    } else {
                        reg.bind_session(o, &target);
                        let name = reg
                            .snapshot(&target)
                            .and_then(|s| s.name)
                            .filter(|n| !n.trim().is_empty())
                            .unwrap_or_else(|| target.clone());
                        format!("已切换到「{name}」，后续消息都发往该会话。")
                    }
                }
            },
        },
        Cmd::Workdir => {
            let task = reg
                .task_config(sid)
                .working_dir
                .filter(|w| !w.trim().is_empty());
            match task {
                Some(w) => format!("当前任务工作目录：{w}"),
                None => match config::load().workspace_dir() {
                    Some(ws) => format!(
                        "当前使用全局工作空间：{}（可用 /setworkdir <路径> 为本任务单独设置）",
                        ws.display()
                    ),
                    None => "未设置工作目录（将回退到服务进程目录）。".to_string(),
                },
            }
        }
        Cmd::SetWorkdir(p) => {
            if p.is_empty() {
                "用法：/setworkdir <目录绝对路径>".to_string()
            } else if !std::path::Path::new(&p).is_dir() {
                format!("目录不存在：{p}")
            } else {
                let mut cfg = reg.task_config(sid);
                cfg.working_dir = Some(p.clone());
                reg.set_task_config(sid, cfg);
                format!("已将本任务工作目录设为:{p}")
            }
        }
        Cmd::Reset => {
            reg.replace_history(sid, Vec::new());
            "已清空本会话上下文历史，新对话从头开始。".to_string()
        }
        Cmd::Model(arg) => {
            let c = config::load();
            let task = reg.task_config(sid);
            let (new_id, reply) = model_switch_reply(
                &c.llms,
                c.active_llm.as_deref(),
                task.model_id.as_deref(),
                arg.as_deref(),
            );
            if let Some(id) = new_id {
                let mut cfg = reg.task_config(sid);
                cfg.model_id = Some(id);
                reg.set_task_config(sid, cfg);
            }
            reply
        }
        Cmd::ModelList => {
            let c = config::load();
            let task = reg.task_config(sid);
            let eff = effective(&c.llms, c.active_llm.as_deref(), task.model_id.as_deref())
                .map(|p| p.id.clone());
            model_list_reply(&c.llms, eff.as_deref())
        }
    };
    reg.append_message(sid, ChatMessage::user(text));
    reg.append_message(sid, ChatMessage::assistant(reply.clone()));
    Some(CommandOutcome { text: reply, card })
}

/// 切换成功后追发到聊天里的「近况」。
///
/// 只弹一个 toast 是不够的：toast 转瞬即逝，用户既不知道切换有没有成功，也不知道
/// 切过去的那个会话进行到哪一步了——「否则我怎么知道是否切换成功了」。所以把会话名、
/// 工作目录和最近几轮对话直接甩回聊天里，看完就能接着下指令。
pub(crate) fn session_recap(
    name: &str,
    working_dir: Option<&str>,
    history: &[ChatMessage],
    max_msgs: usize,
) -> String {
    use wisecortex_core::llm::Role;
    let mut out = format!("▶ 已切换到「{name}」");
    if let Some(w) = working_dir.map(str::trim).filter(|w| !w.is_empty()) {
        out.push_str(&format!("\n工作目录：{w}"));
    }
    let mut recent: Vec<String> = history
        .iter()
        .rev()
        .filter(|m| matches!(m.role, Role::User | Role::Assistant))
        .filter_map(|m| {
            let t = m.content.as_deref()?.trim();
            (!t.is_empty()).then(|| {
                let who = if m.role == Role::User { "你" } else { "它" };
                format!("{who}：{}", one_line(t, 60))
            })
        })
        .take(max_msgs)
        .collect();
    if recent.is_empty() {
        out.push_str("\n\n（这个会话还没有对话记录）");
        return out;
    }
    recent.reverse(); // 上面是从最新往回取的，发出去要按时间正序
    out.push_str("\n\n最近对话：\n");
    out.push_str(&recent.join("\n"));
    out
}

/// 压成单行并截断（IM 里多行原文会刷屏）。
fn one_line(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    format!("{}…", flat.chars().take(max).collect::<String>())
}

/// 卡片按钮点击的处理结果。
pub struct CardOutcome {
    /// 卡片回调的响应体：飞书要求 **3 秒内**发回，且响应体本身就是结果。
    /// 这里回 toast + 重绘后的卡片（▶ 会跟着挪到新的当前会话，肉眼可见切成功了）。
    pub response: Value,
    /// 切换成功后**再**追发的一条消息（目标会话的近况）。必须等应答发完再发——
    /// 发消息是网络往返，塞进 3 秒的应答窗口里会把回调拖超时。
    pub recap: Option<String>,
}

/// 卡片按钮被点击后的处理（`card.action.trigger` 回调）：目前只有 `switch` 一种动作。
/// 只做一次绑定（亚毫秒级），绝不能碰 LLM——3 秒的响应窗口经不起一轮推理。
pub fn handle_card_action(
    reg: &SessionRegistry,
    action: &wisecortex_core::feishu::CardAction,
) -> CardOutcome {
    let origin = ImOrigin::feishu(&action.chat_id);
    let fail = |msg: &str| CardOutcome {
        response: json!({ "toast": { "type": "error", "content": msg } }),
        recap: None,
    };
    if action.value.get("action").and_then(Value::as_str) != Some("switch") {
        return fail("不认识的按钮动作。");
    }
    let Some(sid) = action.value.get("sid").and_then(Value::as_str) else {
        return fail("按钮缺少目标会话。");
    };
    if !reg.exists(sid) {
        return fail("该会话已不存在，请重新 /sessions。");
    }
    reg.bind_session(&origin, sid);
    let name = reg
        .snapshot(sid)
        .and_then(|s| s.name)
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| sid.to_string());
    // 重绘卡片：▶ 挪到新的当前会话，它的按钮也随之消失——不用读文字就知道切成功了。
    let card = sessions_card(&reg.list(), sid, SESSIONS_LIMIT);
    CardOutcome {
        response: json!({
            "toast": { "type": "success", "content": format!("已切换到「{name}」") },
            "card": { "type": "raw", "data": card },
        }),
        recap: Some(session_recap(
            &name,
            reg.task_config(sid).working_dir.as_deref(),
            &reg.history(sid),
            6,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::SessionRegistry;
    use wisecortex_core::config::LlmProfile;

    fn profile(id: &str, name: &str, model: &str) -> LlmProfile {
        LlmProfile {
            id: id.into(),
            name: name.into(),
            model: Some(model.into()),
            ..Default::default()
        }
    }

    #[test]
    fn parse_recognizes_known_commands_only() {
        assert!(matches!(parse("/help"), Some(Cmd::Help)));
        assert!(matches!(parse("  /workdir  "), Some(Cmd::Workdir)));
        assert!(matches!(parse("/reset"), Some(Cmd::Reset)));
        assert!(matches!(parse("/modellist"), Some(Cmd::ModelList)));
        match parse("/setworkdir D:\\work\\proj") {
            Some(Cmd::SetWorkdir(p)) => assert_eq!(p, "D:\\work\\proj"),
            other => panic!("wrong: {other:?}"),
        }
        match parse("/model") {
            Some(Cmd::Model(None)) => {}
            other => panic!("wrong: {other:?}"),
        }
        match parse("/model gpt-x") {
            Some(Cmd::Model(Some(q))) => assert_eq!(q, "gpt-x"),
            other => panic!("wrong: {other:?}"),
        }
        // 未知命令 / 普通文本 / 路径开头的提问 → 不拦截。
        assert!(parse("/unknown").is_none());
        assert!(parse("hello").is_none());
        assert!(parse("/etc/hosts 怎么改").is_none());
        // 多行文本不是命令（命令都是单行）。
        assert!(parse("/reset\n顺便问一下").is_none());
    }

    #[test]
    fn model_switch_finds_by_id_name_or_model() {
        let llms = vec![
            profile("a1", "主力", "claude-x"),
            profile("b2", "", "gpt-y"),
        ];
        // 按 id / 名称 / 模型名均可命中。
        for q in ["a1", "主力", "claude-x"] {
            let (new_id, reply) = model_switch_reply(&llms, Some("b2"), None, Some(q));
            assert_eq!(new_id.as_deref(), Some("a1"), "query={q}");
            assert!(reply.contains("主力"), "reply={reply}");
        }
        // 未命中：不切换，回复里带可用列表。
        let (new_id, reply) = model_switch_reply(&llms, Some("a1"), None, Some("nope"));
        assert!(new_id.is_none());
        assert!(reply.contains("gpt-y"), "应列出可用模型: {reply}");
    }

    #[test]
    fn model_without_arg_shows_current() {
        let llms = vec![
            profile("a1", "主力", "claude-x"),
            profile("b2", "备用", "gpt-y"),
        ];
        // 无任务覆盖 → 显示全局默认。
        let (n, reply) = model_switch_reply(&llms, Some("a1"), None, None);
        assert!(n.is_none());
        assert!(reply.contains("主力") && reply.contains("全局"), "{reply}");
        // 有任务覆盖 → 显示覆盖档。
        let (_, reply) = model_switch_reply(&llms, Some("a1"), Some("b2"), None);
        assert!(reply.contains("备用") && reply.contains("任务"), "{reply}");
    }

    #[test]
    fn model_list_marks_effective_model() {
        let llms = vec![
            profile("a1", "主力", "claude-x"),
            profile("b2", "备用", "gpt-y"),
        ];
        let s = model_list_reply(&llms, Some("b2"));
        let line_b = s.lines().find(|l| l.contains("备用")).unwrap();
        let line_a = s.lines().find(|l| l.contains("主力")).unwrap();
        assert!(line_b.contains('▶'), "生效档应有标记: {line_b}");
        assert!(!line_a.contains('▶'), "非生效档不应有标记: {line_a}");
    }

    fn sess(id: &str, name: &str, status: &str, workdir: Option<&str>) -> Session {
        let mut s = Session {
            id: id.into(),
            name: Some(name.into()),
            status: Some(status.into()),
            ..Default::default()
        };
        if let Some(w) = workdir {
            s.extra
                .insert("working_dir".into(), serde_json::Value::from(w));
        }
        s
    }

    #[test]
    fn sessions_list_reply_numbers_entries_and_marks_current() {
        // 传进来的顺序即展示顺序（registry.list() 已按最近活跃倒序排好）。
        let list = vec![
            sess(
                "web-uuid-1",
                "修复重试",
                "idle",
                Some("D:\\apps\\wisecortex"),
            ),
            sess("feishu-oc_x", "飞书 · 你好", "working", None),
        ];
        let (reply, sids) = sessions_list_reply(&list, "feishu-oc_x", 20);
        // 序号从 1 起，手机上照着打 /switch <n> 即可。
        assert!(reply.contains("1."), "{reply}");
        assert!(reply.contains("2."), "{reply}");
        assert!(
            reply.contains("修复重试") && reply.contains("飞书 · 你好"),
            "{reply}"
        );
        // ▶ 标出当前绑定的会话（第 2 个），第 1 个不该有。
        let line1 = reply.lines().find(|l| l.contains("修复重试")).unwrap();
        let line2 = reply.lines().find(|l| l.contains("飞书 · 你好")).unwrap();
        assert!(!line1.contains('▶'), "非当前会话不该有标记: {line1}");
        assert!(line2.contains('▶'), "当前会话应有标记: {line2}");
        // 工作目录要露出来（远程操作时得知道这个会话在哪个目录干活）。
        assert!(line1.contains("wisecortex"), "应显示工作目录: {line1}");
        // 返回的 sid 顺序 = 展示顺序，供 /switch <n> 快照寻址。
        assert_eq!(
            sids,
            vec!["web-uuid-1".to_string(), "feishu-oc_x".to_string()]
        );
    }

    #[test]
    fn sessions_list_reply_truncates_to_limit() {
        let list: Vec<Session> = (0..30)
            .map(|i| sess(&format!("s{i}"), &format!("会话{i}"), "idle", None))
            .collect();
        let (reply, sids) = sessions_list_reply(&list, "s0", 20);
        // 只列前 limit 条，免得在手机上刷屏；并且要说明被截断了。
        assert_eq!(sids.len(), 20, "快照只保留列出的那些");
        assert!(reply.contains("会话19"), "{reply}");
        assert!(!reply.contains("会话20"), "超出 limit 的不该列出: {reply}");
        assert!(reply.contains("30"), "应提示总数/截断: {reply}");
    }

    #[test]
    fn sessions_card_puts_a_callback_button_on_each_switchable_session() {
        let list = vec![
            sess("web-1", "桌面任务", "idle", Some("D:\\apps\\wisecortex")),
            sess("feishu-oc_x", "飞书 · 你好", "working", None),
        ];
        let card = sessions_card(&list, "feishu-oc_x", 20);
        // 必须是 schema 2.0：卡片 1.0 的按钮回调走「回传交互（旧）」，而那个**不支持长连接**
        // ——CGNAT 后面的机器根本收不到点击事件。这条断言是这个方案能不能成立的地基。
        assert_eq!(card.get("schema").and_then(|v| v.as_str()), Some("2.0"));
        let elements = card["body"]["elements"].as_array().expect("body.elements");
        let dump = card.to_string();
        assert!(
            dump.contains("桌面任务") && dump.contains("飞书 · 你好"),
            "{dump}"
        );
        assert!(dump.contains("wisecortex"), "应显示工作目录: {dump}");

        // 当前会话（飞书那个）不给按钮——已经在它上面了，再给个「切换」是噪音。
        let buttons: Vec<&serde_json::Value> =
            elements.iter().filter(|e| e["tag"] == "button").collect();
        assert_eq!(buttons.len(), 1, "只有非当前会话才有切换按钮");

        // 回调交互挂在 behaviors 上、type=callback，value 带上目标 sid——
        // 这正是 card.action.trigger 回传给我们的东西。
        let b = buttons[0];
        assert_eq!(b["behaviors"][0]["type"], "callback");
        assert_eq!(b["behaviors"][0]["value"]["action"], "switch");
        assert_eq!(b["behaviors"][0]["value"]["sid"], "web-1");
    }

    #[test]
    fn sessions_list_reply_handles_empty() {
        let (reply, sids) = sessions_list_reply(&[], "nope", 20);
        assert!(sids.is_empty());
        assert!(reply.contains("没有"), "{reply}");
    }

    #[test]
    fn switch_resolve_accepts_index_and_rejects_junk() {
        let listing = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        // 1 基序号 → 对应会话 id。
        assert_eq!(switch_resolve(&listing, "2").unwrap(), "b");
        assert_eq!(switch_resolve(&listing, " 3 ").unwrap(), "c");
        // 0 / 越界 / 非数字 / 空 → 报错（且提示重跑 /sessions）。
        for bad in ["0", "4", "99", "abc", ""] {
            let err = switch_resolve(&listing, bad).unwrap_err();
            assert!(
                err.contains("/sessions"),
                "错误提示应引导重跑 /sessions: {err}"
            );
        }
    }

    #[test]
    fn switch_resolve_without_prior_listing_tells_user_to_list_first() {
        // 没跑过 /sessions（快照为空）→ 明确要求先列表，而不是静默切错。
        let err = switch_resolve(&[], "1").unwrap_err();
        assert!(err.contains("/sessions"), "{err}");
    }

    #[test]
    fn try_execute_sessions_then_switch_rebinds_im_origin() {
        let reg = SessionRegistry::new();
        let origin = ImOrigin::feishu("oc_1");
        let default_sid = origin.default_sid();

        // 一个桌面 Web 上建的任务，和飞书自己那个默认会话。
        reg.ensure("web-1");
        reg.append_message("web-1", ChatMessage::user("桌面上的活"));
        reg.set_name_if_empty("web-1", "桌面任务");
        reg.ensure(&default_sid);
        reg.append_message(&default_sid, ChatMessage::user("hi"));
        reg.set_name_if_empty(&default_sid, "飞书 · hi");

        // /sessions：列出全部（含桌面建的），并把序号快照存进绑定表。
        let out = try_execute(&reg, &default_sid, "/sessions", Some(&origin)).unwrap();
        assert!(
            out.text.contains("桌面任务"),
            "应列出桌面上建的会话: {}",
            out.text
        );
        // 来自 IM → 必须同时产出互动卡片（飞书发卡片，不发那坨文本）。
        let card = out.card.expect("IM 来源应产出互动卡片");
        assert_eq!(card["schema"], "2.0");
        let snapshot = reg.listing(&origin);
        assert_eq!(snapshot.len(), 2);

        // 关键：命令自己也会写历史、把当前会话顶到最前，所以「实时排序」已经变了。
        // /switch 必须认 /sessions 当时的快照，否则用户会切错会话。
        let idx = snapshot.iter().position(|s| s == "web-1").unwrap() + 1;
        let reply = try_execute(&reg, &default_sid, &format!("/switch {idx}"), Some(&origin))
            .unwrap()
            .text;
        assert!(reply.contains("桌面任务"), "{reply}");
        assert_eq!(reg.active_session(&origin).as_deref(), Some("web-1"));
    }

    #[test]
    fn card_button_click_rebinds_session_and_returns_toast() {
        use wisecortex_core::feishu::CardAction;
        let reg = SessionRegistry::new();
        let origin = ImOrigin::feishu("oc_1");
        reg.ensure("web-1");
        reg.append_message("web-1", ChatMessage::user("x"));
        reg.set_name_if_empty("web-1", "桌面任务");

        // 点按钮 → 回调带着我们塞进 behaviors.value 的负载回来。
        let action = CardAction {
            chat_id: "oc_1".into(),
            value: json!({ "action": "switch", "sid": "web-1" }),
        };
        let out = handle_card_action(&reg, &action);
        let resp = &out.response;
        assert_eq!(resp["toast"]["type"], "success");
        assert!(
            resp["toast"]["content"]
                .as_str()
                .unwrap()
                .contains("桌面任务"),
            "{resp}"
        );
        assert_eq!(reg.active_session(&origin).as_deref(), Some("web-1"));
        // 卡片一并重绘：▶ 挪到新的当前会话，不用读文字就知道切成功了。
        assert_eq!(resp["card"]["type"], "raw");
        assert_eq!(resp["card"]["data"]["schema"], "2.0");
        // 并追发一条近况——光弹 toast 用户无从知道切到了哪、它在干嘛。
        assert!(out.recap.unwrap().contains("桌面任务"));

        // 会话已被删 → 回错误 toast，不改绑定，也不追发近况。
        reg.remove_session("web-1");
        let out = handle_card_action(&reg, &action);
        assert_eq!(out.response["toast"]["type"], "error");
        assert!(out.recap.is_none());
    }

    #[test]
    fn session_recap_shows_workdir_and_last_turns_oldest_first() {
        let hist = vec![
            ChatMessage::user("很久以前"),
            ChatMessage::assistant("很久以前的回复"),
            ChatMessage::user("把索引迁移到新库"),
            ChatMessage::assistant("已完成第 3 步"),
        ];
        let r = session_recap("WDC底层迁移C", Some("D:\\work\\wdc"), &hist, 2);
        assert!(r.contains("WDC底层迁移C"), "{r}");
        assert!(r.contains("D:\\work\\wdc"), "{r}");
        // 只取最近 2 条，且按时间正序（用户读起来才顺）。
        let (u, a) = (
            r.find("把索引迁移到新库").unwrap(),
            r.find("已完成第 3 步").unwrap(),
        );
        assert!(u < a, "应按时间正序：{r}");
        assert!(!r.contains("很久以前"), "超出条数的旧消息不该出现：{r}");
        assert!(r.contains("你：") && r.contains("它："), "{r}");
    }

    #[test]
    fn session_recap_says_so_when_history_is_empty() {
        let r = session_recap("空会话", None, &[], 6);
        assert!(r.contains("还没有对话记录"), "{r}");
        assert!(!r.contains("工作目录"), "没有工作目录就别硬塞一行：{r}");
    }

    #[test]
    fn switch_without_im_origin_is_refused() {
        // Web 端有侧栏可直接点，不需要 /switch；没有 IM 来源时明确拒绝，而不是静默切错。
        let reg = SessionRegistry::new();
        let reply = try_execute(&reg, "s", "/switch 1", None).unwrap().text;
        assert!(reply.contains("IM"), "{reply}");
    }

    #[test]
    fn try_execute_setworkdir_updates_task_config_and_history() {
        let reg = SessionRegistry::new();
        let dir = std::env::temp_dir();
        let text = format!("/setworkdir {}", dir.display());
        let reply = try_execute(&reg, "s", &text, None)
            .expect("应作为命令执行")
            .text;
        assert!(reply.contains(&dir.display().to_string()), "{reply}");
        assert_eq!(
            reg.task_config("s").working_dir.as_deref(),
            Some(dir.display().to_string().as_str())
        );
        // 命令与回复成对入历史（user 在前，保证交替合法）。
        let hist = reg.history("s");
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].content.as_deref(), Some(text.as_str()));
        assert_eq!(hist[1].content.as_deref(), Some(reply.as_str()));
        // 不存在的目录：不写配置，回复报错。
        let r2 = try_execute(&reg, "s", "/setworkdir Z:\\no\\such\\dir-xyz", None)
            .unwrap()
            .text;
        assert!(r2.contains("不存在"), "{r2}");
        assert_eq!(
            reg.task_config("s").working_dir.as_deref(),
            Some(dir.display().to_string().as_str()),
            "失败不应改配置"
        );
    }

    #[test]
    fn try_execute_reset_clears_history_then_records_exchange() {
        let reg = SessionRegistry::new();
        reg.ensure("s");
        reg.append_message("s", wisecortex_core::llm::ChatMessage::user("旧的"));
        reg.append_message("s", wisecortex_core::llm::ChatMessage::assistant("旧回复"));
        let reply = try_execute(&reg, "s", "/reset", None)
            .expect("应作为命令执行")
            .text;
        assert!(reply.contains("清空"), "{reply}");
        let hist = reg.history("s");
        // 清空后只剩本次命令对（user-first，交替合法）。
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].content.as_deref(), Some("/reset"));
    }

    #[test]
    fn try_execute_help_lists_all_commands_and_passthrough_returns_none() {
        let reg = SessionRegistry::new();
        let reply = try_execute(&reg, "s", "/help", None).unwrap().text;
        for c in [
            "/setworkdir",
            "/workdir",
            "/model",
            "/modellist",
            "/reset",
            "/help",
            "/sessions",
            "/switch",
        ] {
            assert!(reply.contains(c), "帮助应包含 {c}: {reply}");
        }
        assert!(try_execute(&reg, "s", "普通消息", None).is_none());
        assert!(try_execute(&reg, "s", "/unknown xx", None).is_none());
    }
}
