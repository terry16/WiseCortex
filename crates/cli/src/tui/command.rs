//! 斜杠命令解析（纯函数，便于单测）。

/// 用户一行输入解析后的动作。
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// 空行，忽略。
    Empty,
    /// 普通消息，发给 agent。
    Message(String),
    Help,
    Status,
    /// 列会话。
    Sessions,
    /// 切换会话（参数：列表序号或 session id；空=当作列表）。
    Switch(String),
    /// 新会话。
    New,
    /// 列/切模型。
    Model(String),
    /// 订阅登录（参数：claude|openai|xai|gemini）。
    Login(String),
    /// 重试上一轮。
    Retry,
    /// 打断当前回合。
    Interrupt,
    /// 退出。
    Quit,
    /// 未知斜杠命令（参数：原样命令名）。
    Unknown(String),
}

/// 解析一行输入：`/xxx …` → 命令；其余 → 消息；空 → Empty。
pub fn parse(input: &str) -> Action {
    let s = input.trim();
    if s.is_empty() {
        return Action::Empty;
    }
    let Some(rest) = s.strip_prefix('/') else {
        return Action::Message(s.to_string());
    };
    let mut it = rest.splitn(2, char::is_whitespace);
    let name = it.next().unwrap_or("").to_ascii_lowercase();
    let arg = it.next().unwrap_or("").trim().to_string();
    match name.as_str() {
        "help" | "h" | "?" => Action::Help,
        "status" | "st" => Action::Status,
        "sessions" | "ls" => Action::Sessions,
        "switch" | "sw" => Action::Switch(arg),
        "new" | "n" => Action::New,
        "model" | "models" | "m" => Action::Model(arg),
        "login" => Action::Login(arg.to_ascii_lowercase()),
        "retry" | "r" => Action::Retry,
        "interrupt" | "stop" | "abort" => Action::Interrupt,
        "quit" | "exit" | "q" => Action::Quit,
        other => Action::Unknown(other.to_string()),
    }
}

/// `/help` 的文本（TUI 内命令一览）。
pub fn help_text() -> &'static str {
    "命令：\n\
     \x20 /help            显示本帮助\n\
     \x20 /status          当前会话 / 模型 / 花费\n\
     \x20 /sessions  /ls   列出会话\n\
     \x20 /switch <序号|id> 切换会话\n\
     \x20 /new             新建会话\n\
     \x20 /model [序号|id]  列出 / 切换模型档\n\
     \x20 /login <claude|openai|xai|gemini>  登录订阅\n\
     \x20 /retry           重试上一轮\n\
     \x20 /interrupt  Esc  打断当前回合\n\
     \x20 /quit  /exit     退出\n\
     直接输入文字即发给 agent；!<命令> 本地跑 shell；Ctrl+C 清空/退出，Ctrl+D 退出。"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_message() {
        assert_eq!(parse("你好"), Action::Message("你好".into()));
        assert_eq!(
            parse("  帮我看看 config.rs  "),
            Action::Message("帮我看看 config.rs".into())
        );
    }

    #[test]
    fn empty_is_empty() {
        assert_eq!(parse(""), Action::Empty);
        assert_eq!(parse("   "), Action::Empty);
    }

    #[test]
    fn slash_commands_and_aliases() {
        assert_eq!(parse("/help"), Action::Help);
        assert_eq!(parse("/h"), Action::Help);
        assert_eq!(parse("/?"), Action::Help);
        assert_eq!(parse("/sessions"), Action::Sessions);
        assert_eq!(parse("/ls"), Action::Sessions);
        assert_eq!(parse("/new"), Action::New);
        assert_eq!(parse("/retry"), Action::Retry);
        assert_eq!(parse("/QUIT"), Action::Quit); // 大小写不敏感
        assert_eq!(parse("/exit"), Action::Quit);
    }

    #[test]
    fn switch_takes_arg() {
        assert_eq!(parse("/switch 3"), Action::Switch("3".into()));
        assert_eq!(parse("/sw abc123"), Action::Switch("abc123".into()));
        // 无参 = 空串（分发层可当作「列表」处理）。
        assert_eq!(parse("/switch"), Action::Switch(String::new()));
    }

    #[test]
    fn login_lowercases_provider() {
        assert_eq!(parse("/login Gemini"), Action::Login("gemini".into()));
        assert_eq!(parse("/login  XAI "), Action::Login("xai".into()));
    }

    #[test]
    fn model_optional_keyword() {
        assert_eq!(parse("/model"), Action::Model(String::new()));
        assert_eq!(parse("/model gemini"), Action::Model("gemini".into()));
    }

    #[test]
    fn unknown_slash_is_reported() {
        assert_eq!(parse("/frobnicate"), Action::Unknown("frobnicate".into()));
    }

    #[test]
    fn leading_slash_only_is_unknown_empty_name() {
        // 只有一个「/」→ 命令名为空 → Unknown("")，分发层提示未知命令。
        assert_eq!(parse("/"), Action::Unknown(String::new()));
    }
}
