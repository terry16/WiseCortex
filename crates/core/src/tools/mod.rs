//! 工具集：把 agent 从「聊天」变成能干活的「编程 agent」。
//!
//! MVP 核心 6 件：read_file / write_file / edit_file / glob / grep / shell。
//! 工具定义（schema）喂给 LLM；LLM 回 tool_calls 后由 agent loop 执行并回灌结果。
//! 后续补 web_fetch / web_search / todo / invoke_skill 等，目标约 16 件。

pub mod fs;
pub mod git;
pub mod gui_input;
pub mod knowledge;
pub mod lsp;
#[cfg(target_os = "macos")]
pub mod mac_gui;
pub mod mcp;
pub mod memory;
pub mod notify;
pub mod shell;
pub mod skill;
pub mod todo;
#[cfg(windows)]
pub mod uia;
pub mod web;
#[cfg(windows)]
pub mod win_gui;

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::llm::ToolDef;

/// 工具执行结果：Ok(给模型看的输出) / Err(错误信息，也会回灌给模型)。
pub type ToolResult = Result<String, String>;

/// 一个可被 agent 调用的工具。
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// JSON Schema（OpenAI parameters / Anthropic input_schema）。
    fn parameters(&self) -> Value;
    /// UI 上 tool_call 的人类可读摘要。
    fn summary(&self, _args: &Value) -> String {
        self.name().to_string()
    }
    /// 是否属于「危险操作」，在非自动批准模式下需用户确认。
    /// 默认 false（只读类工具）；写/改/执行类工具覆盖为 true。
    fn requires_approval(&self) -> bool {
        false
    }
    fn execute(&self, args: &Value) -> ToolResult;

    fn to_def(&self) -> ToolDef {
        ToolDef {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
        }
    }
}

/// 工具注册表。所有相对路径以 `workdir` 为基准。
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    /// 默认编程工具集，工作目录为 `workdir`。
    pub fn with_defaults(workdir: impl Into<PathBuf>) -> Self {
        let workdir = workdir.into();
        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(fs::ReadFile::new(workdir.clone())),
            Box::new(fs::ReadImage::new(workdir.clone())),
            Box::new(fs::WriteFile::new(workdir.clone())),
            Box::new(fs::EditFile::new(workdir.clone())),
            Box::new(fs::Glob::new(workdir.clone())),
            Box::new(fs::Grep::new(workdir.clone())),
            Box::new(git::Git::new(workdir.clone())),
            Box::new(git::GitCommit::new(workdir.clone())),
            Box::new(lsp::Lsp::new(workdir.clone())),
            Box::new(shell::Shell::new(workdir)),
            Box::new(web::WebFetch),
            Box::new(web::WebSearch),
            Box::new(todo::TodoWrite),
            Box::new(notify::Notify),
        ];
        ToolRegistry { tools }
    }

    /// 把默认的无状态 shell 换成按任务绑定的持久会话 shell（cwd/env/PATH 跨调用保留）。
    pub fn with_persistent_shell(mut self, base: impl Into<PathBuf>, key: &str) -> Self {
        self.tools.retain(|t| t.name() != "shell");
        self.tools.push(Box::new(shell::Shell::with_session(
            base.into(),
            key.to_string(),
        )));
        self
    }

    /// 追加 invoke_skill 工具（携带技能目录；调用时实时加载，新技能即时可用）。
    pub fn with_skills(mut self, skill_dirs: Vec<PathBuf>) -> Self {
        self.tools
            .push(Box::new(skill::InvokeSkill::new(skill_dirs)));
        self
    }

    /// 同 [`with_skills`]，但限定 invoke_skill 只能调用 `only` 列出的技能（任务钉选）。
    /// `only` 为空时等价于 [`with_skills`]（全量）。
    pub fn with_skills_pinned(mut self, skill_dirs: Vec<PathBuf>, only: Vec<String>) -> Self {
        self.tools
            .push(Box::new(skill::InvokeSkill::with_only(skill_dirs, only)));
        self
    }

    /// 追加 knowledge_search 工具（本会话挂载的知识库路径；为空则不加）。
    pub fn with_knowledge(mut self, roots: Vec<PathBuf>) -> Self {
        if !roots.is_empty() {
            self.tools
                .push(Box::new(knowledge::KnowledgeSearch::new(roots)));
        }
        self
    }

    /// 追加 remember 工具。`sid` 定会话级记忆的归属，`workdir` 定项目级记忆的归属
    /// （项目级是默认档：同目录所有会话共享，新开对话也带得走）。
    pub fn with_memory(mut self, sid: impl Into<String>, workdir: impl Into<PathBuf>) -> Self {
        self.tools
            .push(Box::new(memory::Remember::new(sid, workdir)));
        self
    }

    /// 追加任意一个自定义工具（如 server 侧定义的后台任务工具）。
    pub fn with_tool(mut self, tool: Box<dyn Tool>) -> Self {
        self.tools.push(tool);
        self
    }

    /// 追加已连接 MCP 服务器的工具（`mcp__<server>__<tool>`）。需先 `mcp::ensure_started()`。
    pub fn with_mcp(mut self) -> Self {
        for info in crate::mcp::tool_infos() {
            self.tools.push(Box::new(mcp::McpTool::new(info)));
        }
        self
    }

    /// 追加本平台的 GUI 控制工具（截图/枚举/输入）。Windows 与 macOS 各注册自己的实现，
    /// **工具名刻意相同**，故同一份提示词/技能在两个平台上表现一致。
    /// Linux（服务器、cron）为空操作——那里根本没有 GUI，这些工具压根不存在。
    // Linux 上两个 cfg 块都被剔除，self 从未被修改，`mut` 就成了多余——
    // CI 的 `cargo clippy -- -D warnings` 会把 unused_mut 判成错误。
    // 自 a4655a9（07-16 加 macOS 分支）起 ubuntu job 一直因此挂红。
    // 只在「两个平台都不成立」时豁免，Windows/macOS 仍保留该 lint。
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(unused_mut))]
    pub fn with_local_gui(mut self) -> Self {
        #[cfg(windows)]
        {
            self.tools.push(Box::new(win_gui::ListWindows));
            self.tools.push(Box::new(win_gui::CaptureWindow));
            self.tools.push(Box::new(win_gui::WindowClick));
            self.tools.push(Box::new(win_gui::WindowType));
            self.tools.push(Box::new(win_gui::WindowKey));
            self.tools.push(Box::new(win_gui::WindowScroll));
            self.tools.push(Box::new(uia::UiTree));
            self.tools.push(Box::new(uia::FindElement));
            self.tools.push(Box::new(uia::ElementAtPoint));
            self.tools.push(Box::new(uia::ClickElement));
            self.tools.push(Box::new(uia::SetElementValue));
        }
        #[cfg(target_os = "macos")]
        {
            // 「眼睛 + 手」。语义类工具（ui_tree/find_element/click_element/
            // set_element_value）需要 AX，AX 只能在主线程调，留待下一阶段。
            self.tools.push(Box::new(mac_gui::ListWindows));
            self.tools.push(Box::new(mac_gui::CaptureWindow));
            self.tools.push(Box::new(mac_gui::WindowClick));
            self.tools.push(Box::new(mac_gui::WindowType));
            self.tools.push(Box::new(mac_gui::WindowKey));
            self.tools.push(Box::new(mac_gui::WindowScroll));
        }
        self
    }

    /// 工具定义（喂给 LLM）。
    pub fn defs(&self) -> Vec<ToolDef> {
        self.tools.iter().map(|t| t.to_def()).collect()
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|b| b.as_ref())
    }

    pub fn summary(&self, name: &str, args: &Value) -> String {
        self.get(name)
            .map(|t| t.summary(args))
            .unwrap_or_else(|| name.to_string())
    }

    /// 该工具是否需要确认（未知工具保守地视为需要）。
    pub fn requires_approval(&self, name: &str) -> bool {
        self.get(name)
            .map(|t| t.requires_approval())
            .unwrap_or(true)
    }

    /// 执行工具；未知工具返回 Err。
    ///
    /// 用 `catch_unwind` 兜底：工具跑在 `spawn_blocking` 的阻塞线程里，一次 panic 过去会
    /// 直接掀掉线程——若它当时正持有某把全局锁（如 shell 的会话表），那把锁会被 std
    /// 永久标记为 poisoned，之后所有 `.lock().unwrap()` 二次 panic，整个工具永久不可用。
    /// 这里把 panic 就地收成一条错误文本：单次调用失败，但绝不扩散成进程级故障。
    pub fn execute(&self, name: &str, args: &Value) -> ToolResult {
        let Some(t) = self.get(name) else {
            return Err(format!("未知工具: {name}"));
        };
        // AssertUnwindSafe：工具的 &self 是共享引用，panic 后我们不复用其内部状态
        // （shell 会丢弃并重建会话），故这里断言 unwind 安全是成立的。
        let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| t.execute(args)));
        called.unwrap_or_else(|payload| {
            let detail = panic_message(payload.as_ref());
            crate::buglog::record("tool", &format!("工具 {name} panic: {detail}"));
            Err(format!(
                "工具 {name} 内部异常（已隔离，不影响后续调用）: {detail}"
            ))
        })
    }
}

/// 从 panic payload 里尽力取出可读信息（`panic!("...")` 的两种常见载荷）。
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "未知 panic".to_string()
    }
}

/// 相对路径以 base 解析；绝对路径原样。
pub(crate) fn resolve(base: &Path, p: &str) -> PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

/// 从 args 取必填字符串字段。
pub fn require_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("缺少必填参数: {key}"))
}

/// 「带图片的工具结果」标记键。agent loop 据此识别并把图片回灌为视觉消息。
const IMAGE_RESULT_MARKER: &str = "__wisecortex_image_result__";

/// 构造带图片的工具结果：序列化为约定 JSON。`text` 给模型看，`images` 是 data URL。
pub fn image_result(text: impl Into<String>, images: Vec<String>) -> String {
    serde_json::json!({
        IMAGE_RESULT_MARKER: true,
        "text": text.into(),
        "images": images,
    })
    .to_string()
}

/// 解析工具结果：命中约定 JSON 返回 (文本, 图片 data URL 列表)；否则原样返回 (result, 空)。
pub fn split_image_result(result: &str) -> (String, Vec<String>) {
    let parsed: Option<Value> = serde_json::from_str(result).ok();
    let obj = parsed.as_ref().and_then(Value::as_object);
    let marked = obj
        .and_then(|o| o.get(IMAGE_RESULT_MARKER))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let Some(o) = obj.filter(|_| marked) else {
        return (result.to_string(), Vec::new());
    };
    let text = o
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let images = o
        .get("images")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    (text, images)
}

/// 一个窗口的基本信息（供 list_windows 输出；Win32 枚举填充，纯结构体便于跨平台测试）。
#[derive(Debug, Clone, PartialEq)]
pub struct WinInfo {
    /// 窗口句柄（十进制；后续 capture_window / 输入工具用它定位）。
    pub hwnd: isize,
    pub pid: u32,
    pub title: String,
    /// 屏幕矩形 (left, top, width, height)。
    pub rect: (i32, i32, i32, i32),
}

/// 把窗口列表格式化成给模型看的文本（一行一窗口）。
pub fn format_windows(ws: &[WinInfo]) -> String {
    if ws.is_empty() {
        return "(没有可见窗口)".to_string();
    }
    ws.iter()
        .map(|w| {
            let (_, _, width, height) = w.rect;
            format!(
                "hwnd={} pid={} {}x{} | {}",
                w.hwnd, w.pid, width, height, w.title
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 窗口截图的最长边上限。超过就等比缩小后再喂给模型（省 token，也贴近模型的最佳图像尺寸）。
/// 缩放正是「图像空间」约定的由来：模型看到的是缩放后的图，坐标也只能按图上的来。
///
/// 定在 1024 而不是各家的档位上限（旧视觉档 1568、高分档 2576），是因为 token 成本按**面积**
/// 走而不是按边长：4K 屏截图缩到 1568 仍有 138 万像素、单图接近 ~1600 tokens，而带图的历史
/// 消息每一轮都会被完整重发（见 llm/anthropic.rs 的 `msg.images`）——GUI 自动化跑十几张图，
/// 光是重发就吃掉整个上下文。1024 只有 1568 的 43% 面积，代价是坐标精度：4K 屏上 1 个图像
/// 像素对应 3.75 个真实像素，点按钮够用，点 1px 的边框不够。
pub const CAPTURE_MAX_EDGE: u32 = 1024;

/// 截图缩放比例（图像空间 ↔ 窗口空间的换算依据）。只缩不放，故恒 ≤ 1.0。
///
/// 关键性质：它只依赖窗口尺寸与 max_edge，是**纯函数**——所以任何工具都能就地重算出
/// 与 capture_window 当时完全一致的比例，不必把状态存下来传来传去。
pub fn capture_scale(w: u32, h: u32, max_edge: u32) -> f64 {
    let m = w.max(h);
    if m == 0 || m <= max_edge {
        return 1.0;
    }
    max_edge as f64 / m as f64
}

/// 等比缩放到最长边 ≤ max_edge（只缩不放；0 安全返回）。
/// 走 [`capture_scale`] 算比例，保证与坐标换算用的是同一个比例，不会各算各的。
pub fn scaled_dims(w: u32, h: u32, max_edge: u32) -> (u32, u32) {
    let scale = capture_scale(w, h, max_edge);
    if scale >= 1.0 {
        return (w, h);
    }
    let nw = ((w as f64 * scale).round() as u32).max(1);
    let nh = ((h as f64 * scale).round() as u32).max(1);
    (nw, nh)
}

/// 图像空间 → 窗口空间：模型照着缩放后的截图给坐标，真要点下去必须还原回真实窗口坐标。
/// 不做这步的后果是静默错位——窗口越大偏得越狠（1920 宽的窗口图心就偏 176px）。
pub fn img_to_win(x: i32, y: i32, scale: f64) -> (i32, i32) {
    // is_finite 顺带挡掉 NaN/inf；比例只可能是 (0,1)，其余一律按不缩放处理。
    if !scale.is_finite() || scale <= 0.0 || scale >= 1.0 {
        return (x, y);
    }
    (
        (x as f64 / scale).round() as i32,
        (y as f64 / scale).round() as i32,
    )
}

/// 窗口空间 → 图像空间：UIA/AX 报的是真实窗口坐标，要换成模型在图上看得见的坐标，
/// 否则「看图点」和「按元素点」两条路给出的坐标语义会打架。
pub fn win_to_img(x: i32, y: i32, scale: f64) -> (i32, i32) {
    // is_finite 顺带挡掉 NaN/inf；比例只可能是 (0,1)，其余一律按不缩放处理。
    if !scale.is_finite() || scale <= 0.0 || scale >= 1.0 {
        return (x, y);
    }
    (
        (x as f64 * scale).round() as i32,
        (y as f64 * scale).round() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_exposes_default_tools() {
        let reg = ToolRegistry::with_defaults(".");
        assert_eq!(reg.defs().len(), 14);
        for n in [
            "read_file",
            "read_image",
            "write_file",
            "edit_file",
            "glob",
            "grep",
            "git",
            "git_commit",
            "lsp",
            "shell",
            "web_fetch",
            "web_search",
            "todo_write",
            "notify",
        ] {
            assert!(reg.get(n).is_some(), "missing tool {n}");
        }
    }

    #[test]
    fn unknown_tool_errors() {
        let reg = ToolRegistry::with_defaults(".");
        assert!(reg.execute("nope", &serde_json::json!({})).is_err());
    }

    /// 回归（真实事故）：工具跑在 `spawn_blocking` 的阻塞线程里，一次 panic 若发生在
    /// 持有全局锁时，会把锁永久毒化、让该工具彻底不可用（历史上 shell 就是这么瘫的）。
    /// `execute` 必须把 panic 就地收成错误，绝不让它掀翻线程、扩散成进程级故障。
    #[test]
    fn tool_panic_is_contained_as_error() {
        struct Exploding;
        impl Tool for Exploding {
            fn name(&self) -> &'static str {
                "exploding"
            }
            fn description(&self) -> &'static str {
                "always panics"
            }
            fn parameters(&self) -> Value {
                serde_json::json!({"type":"object"})
            }
            fn execute(&self, _args: &Value) -> ToolResult {
                panic!("boom");
            }
        }
        let reg = ToolRegistry {
            tools: vec![Box::new(Exploding)],
        };
        // 静默 panic 的默认打印，保持测试输出干净。
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let out = reg.execute("exploding", &serde_json::json!({}));
        std::panic::set_hook(prev);

        let err = out.expect_err("panic 应被收成 Err 而非掀翻线程");
        assert!(err.contains("exploding"), "错误应点名工具: {err}");
        assert!(err.contains("boom"), "错误应带上 panic 信息: {err}");
    }

    /// 喂给模型的文本必须是英文——不是所有模型对中文指令都跟随得好，工具描述又直接决定
    /// 模型选不选、怎么调这个工具。注意：只管 description/parameters（模型侧），
    /// summary() 是给人看的 UI 文案，中文不受此约束。
    #[test]
    fn tool_descriptions_and_params_are_english() {
        fn cjk(s: &str) -> bool {
            s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
        }
        // 递归查 JSON schema 里所有 "description" 值（参数说明也喂给模型）。
        fn schema_cjk(v: &serde_json::Value, path: &str, bad: &mut Vec<String>) {
            match v {
                serde_json::Value::Object(m) => {
                    for (k, val) in m {
                        if k == "description" {
                            if let Some(s) = val.as_str() {
                                if cjk(s) {
                                    bad.push(format!("{path} → {s}"));
                                }
                            }
                        }
                        schema_cjk(val, path, bad);
                    }
                }
                serde_json::Value::Array(a) => a.iter().for_each(|x| schema_cjk(x, path, bad)),
                _ => {}
            }
        }
        // 覆盖默认工具 + 可选工具（invoke_skill/knowledge_search/remember 同样喂给模型）。
        // 也必须带上 GUI 工具：它们同样进 tool_defs 喂给模型，漏掉就等于给自己开后门
        //（cfg 决定本平台实际注册哪些；两个平台各自在自己的 CI 上把关）。
        // mcp 工具的描述来自远端 MCP server，不归我们管，故不纳入。
        let reg = ToolRegistry::with_defaults(".")
            .with_skills(vec![".".into()])
            .with_knowledge(vec![".".into()])
            .with_memory("t", ".")
            .with_local_gui();
        let mut bad = Vec::new();
        for d in reg.defs() {
            if cjk(&d.description) {
                bad.push(format!("{} (description)", d.name));
            }
            schema_cjk(&d.parameters, &format!("{} (parameters)", d.name), &mut bad);
        }
        assert!(
            bad.is_empty(),
            "以下模型侧文案仍是中文：\n{}",
            bad.join("\n")
        );
    }

    /// 回归：capture_window 把图缩到 1568，window_click 却按原始窗口坐标解释——
    /// 描述里明写「坐标与 capture_window 的图对应」，于是最大化窗口必然静默错位。
    /// 这条钉死「模型照图心点击 → 真落在窗口心」。
    #[test]
    fn image_coords_map_back_to_window_center() {
        for (w, h) in [(1920u32, 1080u32), (2560, 1440), (3840, 2160), (1280, 800)] {
            let (iw, ih) = scaled_dims(w, h, CAPTURE_MAX_EDGE);
            let scale = capture_scale(w, h, CAPTURE_MAX_EDGE);
            // 模型在图上点正中心，还原回窗口应当就是窗口正中心（容 1px 取整误差）。
            let (wx, wy) = img_to_win((iw / 2) as i32, (ih / 2) as i32, scale);
            assert!(
                (wx - (w / 2) as i32).abs() <= 1 && (wy - (h / 2) as i32).abs() <= 1,
                "窗口 {w}x{h}：图心({},{}) 还原成 ({wx},{wy})，应约为 ({},{})",
                iw / 2,
                ih / 2,
                w / 2,
                h / 2
            );
        }
    }

    /// 两个方向必须互逆，否则「看图点」与「按元素点」会各说各话。
    #[test]
    fn img_win_roundtrip_is_stable() {
        let scale = capture_scale(2560, 1440, CAPTURE_MAX_EDGE);
        assert!(scale < 1.0, "2560 宽必须触发缩放，否则这条测试没意义");
        for (x, y) in [(0, 0), (100, 50), (1279, 719), (2559, 1439)] {
            let (ix, iy) = win_to_img(x, y, scale);
            let (bx, by) = img_to_win(ix, iy, scale);
            assert!(
                (bx - x).abs() <= 2 && (by - y).abs() <= 2,
                "窗口({x},{y}) → 图({ix},{iy}) → 窗口({bx},{by})，往返偏差过大"
            );
        }
    }

    /// 未超上限的小窗不缩放，换算必须是恒等——不能反倒把本来正确的坐标改坏。
    #[test]
    fn small_window_coords_are_identity() {
        let scale = capture_scale(800, 600, CAPTURE_MAX_EDGE);
        assert_eq!(scale, 1.0);
        assert_eq!(img_to_win(400, 300, scale), (400, 300));
        assert_eq!(win_to_img(400, 300, scale), (400, 300));
    }

    /// 上限降到 1024 的影响面：1280×800 这类**普通**窗口此前是 1:1，现在也会被缩。
    /// 这是有意的（token 按面积走），但精度代价必须显式钉住而不是悄悄发生。
    #[test]
    fn ordinary_1280_window_now_scales_after_lowering_the_cap() {
        let scale = capture_scale(1280, 800, CAPTURE_MAX_EDGE);
        assert!(scale < 1.0, "1280 宽在 1024 上限下必须缩放");
        assert_eq!(scaled_dims(1280, 800, CAPTURE_MAX_EDGE), (1024, 640));
        // 缩放比 0.8，往返偏差仍在 1px 量级——点按钮不受影响。
        let (ix, iy) = win_to_img(640, 400, scale);
        let (bx, by) = img_to_win(ix, iy, scale);
        assert!((bx - 640).abs() <= 1 && (by - 400).abs() <= 1);
    }

    /// 退化输入不能 panic 或产出 NaN 坐标（0 尺寸窗口、非法比例）。
    #[test]
    fn degenerate_scale_is_safe() {
        assert_eq!(capture_scale(0, 0, CAPTURE_MAX_EDGE), 1.0);
        assert_eq!(img_to_win(10, 10, 0.0), (10, 10));
        assert_eq!(img_to_win(10, 10, f64::NAN), (10, 10));
        assert_eq!(win_to_img(10, 10, -1.0), (10, 10));
    }

    /// scaled_dims 与 capture_scale 必须同源，否则图的尺寸和坐标换算会悄悄对不上。
    #[test]
    fn scaled_dims_agrees_with_capture_scale() {
        for (w, h) in [(1920u32, 1080u32), (3840, 2160), (1000, 500), (1568, 1568)] {
            let (iw, ih) = scaled_dims(w, h, CAPTURE_MAX_EDGE);
            let s = capture_scale(w, h, CAPTURE_MAX_EDGE);
            assert_eq!(
                iw,
                ((w as f64 * s).round() as u32).max(1),
                "宽不一致 {w}x{h}"
            );
            assert_eq!(
                ih,
                ((h as f64 * s).round() as u32).max(1),
                "高不一致 {w}x{h}"
            );
        }
    }

    #[test]
    fn split_image_result_passthrough_plain_text() {
        let (t, imgs) = split_image_result("普通文本结果");
        assert_eq!(t, "普通文本结果");
        assert!(imgs.is_empty());
    }

    #[test]
    fn split_image_result_passthrough_unmarked_json() {
        // 工具碰巧返回 JSON，但没有 marker —— 不能被误判。
        let raw = r#"{"text":"x","images":["y"]}"#;
        let (t, imgs) = split_image_result(raw);
        assert_eq!(t, raw);
        assert!(imgs.is_empty());
    }

    #[test]
    fn image_result_roundtrips() {
        let s = image_result(
            "已截图 800x600",
            vec!["data:image/png;base64,AAA".to_string()],
        );
        let (t, imgs) = split_image_result(&s);
        assert_eq!(t, "已截图 800x600");
        assert_eq!(imgs, vec!["data:image/png;base64,AAA".to_string()]);
    }

    #[test]
    fn split_image_result_marked_without_images() {
        let s = image_result("无图", vec![]);
        let (t, imgs) = split_image_result(&s);
        assert_eq!(t, "无图");
        assert!(imgs.is_empty());
    }

    #[test]
    fn format_windows_lists_each_line() {
        let ws = vec![
            WinInfo {
                hwnd: 131072,
                pid: 1234,
                title: "记事本".into(),
                rect: (0, 0, 800, 600),
            },
            WinInfo {
                hwnd: 65540,
                pid: 9,
                title: "VS Code".into(),
                rect: (10, 20, 1280, 720),
            },
        ];
        let out = format_windows(&ws);
        assert!(out.contains("131072"));
        assert!(out.contains("记事本"));
        assert!(out.contains("800x600"));
        assert!(out.contains("VS Code"));
        assert_eq!(out.lines().count(), 2);
    }

    #[test]
    fn format_windows_empty() {
        assert_eq!(format_windows(&[]), "(没有可见窗口)");
    }

    /// 上限锁在 1024：4K 屏截图必须落到 1024×576。token 成本按面积走，1568 那档在 4K 上
    /// 仍有 138 万像素，而带图历史每轮重发——这条断言就是「别再把上限调回去」的闸。
    #[test]
    fn capture_max_edge_keeps_4k_screenshots_small() {
        assert_eq!(CAPTURE_MAX_EDGE, 1024);
        let (w, h) = scaled_dims(3840, 2160, CAPTURE_MAX_EDGE);
        assert_eq!((w, h), (1024, 576));
        // 面积是 1568 档的一半以下——省的是这个，不是边长。
        let (w16, h16) = scaled_dims(3840, 2160, 1568);
        assert!(
            (w * h) * 2 < w16 * h16,
            "1024 档应不到 1568 档面积的一半：{}x{} vs {}x{}",
            w,
            h,
            w16,
            h16
        );
    }

    #[test]
    fn scaled_dims_no_upscale() {
        assert_eq!(scaled_dims(800, 600, 1568), (800, 600));
    }

    #[test]
    fn scaled_dims_downscales_longest_edge() {
        // 最长边 3000 → 1568，按比例缩。
        assert_eq!(scaled_dims(3000, 1500, 1568), (1568, 784));
    }

    #[test]
    fn scaled_dims_handles_zero() {
        assert_eq!(scaled_dims(0, 0, 1568), (0, 0));
    }

    #[cfg(windows)]
    #[test]
    fn with_local_gui_registers_windows_tools() {
        let reg = ToolRegistry::with_defaults(".").with_local_gui();
        assert!(reg.get("list_windows").is_some(), "list_windows 应注册");
        assert!(reg.get("capture_window").is_some(), "capture_window 应注册");
        // 语义类工具（UIA）Windows 上有，macOS 本阶段还没有。
        assert!(reg.get("ui_tree").is_some(), "ui_tree 应注册");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn with_local_gui_registers_macos_tools() {
        let reg = ToolRegistry::with_defaults(".").with_local_gui();
        // 工具名必须与 Windows 侧一致——提示词/技能不该关心平台。
        for n in [
            "list_windows",
            "capture_window",
            "window_click",
            "window_type",
            "window_key",
            "window_scroll",
        ] {
            assert!(reg.get(n).is_some(), "{n} 应注册");
        }
        // 本阶段刻意不含 AX 语义工具（AX 只能主线程调，留待下一阶段）。
        assert!(reg.get("ui_tree").is_none(), "ui_tree 本阶段不应注册");
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn with_local_gui_noop_without_desktop() {
        // Linux（服务器/cron）：无 GUI，构建器为空操作，工具数不变。
        let base = ToolRegistry::with_defaults(".").defs().len();
        let with = ToolRegistry::with_defaults(".")
            .with_local_gui()
            .defs()
            .len();
        assert_eq!(base, with);
    }
}
