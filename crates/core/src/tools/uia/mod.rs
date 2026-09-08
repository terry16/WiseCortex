//! Windows UI Automation 工具（仅 #[cfg(windows)]）：走 PowerShell 托管
//! `System.Windows.Automation`（混合方案——UIA 非高频，托管 API 比原生 COM 省事得多）。
//!
//! 安全：脚本是**静态**的，经 `-EncodedCommand` 传入；模型给的可变值（元素名、要设的值、
//! hwnd/坐标）一律经**环境变量**传给子进程，脚本内用 `$env:WC_UIA_*` 读取——彻底免脚本注入。
//! 健壮：`wait-timeout` 给每次调用兜底超时（UIA 在大应用上可能很慢），超时即杀进程。

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};
use wait_timeout::ChildExt;

use super::gui_input::encode_ps_command;
use super::{capture_scale, img_to_win, require_str, Tool, ToolResult, CAPTURE_MAX_EDGE};

/// 单次 UIA 调用的超时（秒）。UIA 大树可能慢，超时即放弃。
const UIA_TIMEOUT_SECS: u64 = 20;

/// 本窗口的截图缩放比例。**必须**用 GetWindowRect——capture_window 就是按它取的图，
/// 用别的来源（比如 UIA 自己的 BoundingRectangle）会和图对不上。
/// 取不到窗口时退化成 1.0（不缩放），让脚本自己去报「未找到窗口」。
fn window_scale(hwnd: i64) -> f64 {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let mut r = RECT::default();
    let h = HWND(hwnd as *mut core::ffi::c_void);
    if unsafe { GetWindowRect(h, &mut r) }.is_err() {
        return 1.0;
    }
    let w = (r.right - r.left).max(1) as u32;
    let ht = (r.bottom - r.top).max(1) as u32;
    capture_scale(w, ht, CAPTURE_MAX_EDGE)
}

/// 跑一段 PowerShell 脚本：`-EncodedCommand` 传静态脚本，`env` 传可变值；超时则杀掉。
/// 返回 stdout（trim 后）。stdout 为空且 stderr 非空时按错误返回。
fn run_ps(script: &str, env: &[(&str, String)], timeout_secs: u64) -> Result<String, String> {
    // 关键：强制子进程 stdout 用 UTF-8 编码——否则中文按本地码页(GBK)写入管道，
    // Rust 端读取会得到非 UTF-8 字节。try/catch 兜底（极端无 console 场景）。
    let full = format!(
        "try{{[Console]::OutputEncoding=[System.Text.Encoding]::UTF8}}catch{{}}\n\
         $ProgressPreference='SilentlyContinue'\n{script}"
    );
    let encoded = encode_ps_command(&full);
    let mut builder = Command::new("powershell");
    builder
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
        .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::proc::no_window(&mut builder);
    let mut child = builder
        .spawn()
        .map_err(|e| format!("启动 PowerShell 失败: {e}"))?;

    // 单独线程把 stdout/stderr 抽干，避免管道写满与 wait 互相死锁。
    let mut out_pipe = child.stdout.take().unwrap();
    let mut err_pipe = child.stderr.take().unwrap();
    let out_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out_pipe.read_to_end(&mut b);
        String::from_utf8_lossy(&b).into_owned()
    });
    let err_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err_pipe.read_to_end(&mut b);
        String::from_utf8_lossy(&b).into_owned()
    });

    let timed_out = match child
        .wait_timeout(Duration::from_secs(timeout_secs))
        .map_err(|e| e.to_string())?
    {
        Some(_) => false,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            true
        }
    };
    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();
    if timed_out {
        return Err(format!("UIA 操作超时（>{timeout_secs}s）"));
    }
    let out = stdout.trim();
    if out.is_empty() && !stderr.trim().is_empty() {
        return Err(format!("UIA 脚本出错: {}", stderr.trim()));
    }
    Ok(out.to_string())
}

/// 脚本输出以 `ERR:` 开头视为错误。
fn ps_result(out: String) -> ToolResult {
    match out.strip_prefix("ERR:") {
        Some(e) => Err(e.trim().to_string()),
        None => Ok(out),
    }
}

/// 必填 hwnd → 起始 env（WC_UIA_HWND）。
fn base_env(args: &Value) -> Result<Vec<(&'static str, String)>, String> {
    let hwnd = args
        .get("hwnd")
        .and_then(Value::as_i64)
        .ok_or_else(|| "缺少必填参数: hwnd（整数）".to_string())?;
    // WC_UIA_SCALE：脚本把元素的窗口坐标乘上它，输出成「图像空间」——也就是模型
    // 在 capture_window 那张图上看到的坐标。不换算的话，同一个位置从 ui_tree 读出来
    // 和从图上量出来会是两个数，模型必然点错。
    Ok(vec![
        ("WC_UIA_HWND", hwnd.to_string()),
        ("WC_UIA_SCALE", window_scale(hwnd).to_string()),
    ])
}

/// 把 name/role/automation_id 三个可选定位条件塞进 env；返回是否至少给了一个。
fn push_query(args: &Value, env: &mut Vec<(&'static str, String)>) -> bool {
    let mut any = false;
    for (arg_key, env_key) in [
        ("name", "WC_UIA_NAME"),
        ("role", "WC_UIA_ROLE"),
        ("automation_id", "WC_UIA_AUTOID"),
    ] {
        let v = args
            .get(arg_key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if !v.is_empty() {
            any = true;
        }
        env.push((env_key, v));
    }
    any
}

const UI_TREE_PS: &str = r#"
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$AE=[System.Windows.Automation.AutomationElement]
$TS=[System.Windows.Automation.TreeScope]::Descendants
$CT=[System.Windows.Automation.Condition]::TrueCondition
$hwnd=[IntPtr][int64]$env:WC_UIA_HWND
try{$win=$AE::FromHandle($hwnd)}catch{$win=$null}
if($null -eq $win){Write-Output 'ERR: 未找到窗口';exit 0}
$wr=$win.Current.BoundingRectangle
$sc=[double]$env:WC_UIA_SCALE
if($sc -le 0){$sc=1.0}
$els=$win.FindAll($TS,$CT)
$n=0;$max=250
foreach($e in $els){
  if($n -ge $max){break}
  try{
    $ct=$e.Current.ControlType.ProgrammaticName -replace 'ControlType\.',''
    $nm=$e.Current.Name;$id=$e.Current.AutomationId;$r=$e.Current.BoundingRectangle
    $rx=[int](($r.X-$wr.X)*$sc);$ry=[int](($r.Y-$wr.Y)*$sc)
    $line="[$ct] `"$nm`" ($rx,$ry $([int]($r.Width*$sc))x$([int]($r.Height*$sc)))"
    if(-not $e.Current.IsEnabled){$line+=' disabled'}
    if($id){$line+=" id=$id"}
    Write-Output $line
    $n++
  }catch{}
}
if($n -eq 0){Write-Output '(无可见 UIA 元素——该程序可能未暴露 UIA，如自绘界面)'}
if($n -ge $max){Write-Output "...(已截断，仅显示前 $max 个元素)"}
"#;

const FIND_PS: &str = r#"
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$AE=[System.Windows.Automation.AutomationElement]
$TS=[System.Windows.Automation.TreeScope]::Descendants
$CT=[System.Windows.Automation.Condition]::TrueCondition
$hwnd=[IntPtr][int64]$env:WC_UIA_HWND
try{$win=$AE::FromHandle($hwnd)}catch{$win=$null}
if($null -eq $win){Write-Output 'ERR: 未找到窗口';exit 0}
$wr=$win.Current.BoundingRectangle
$sc=[double]$env:WC_UIA_SCALE
if($sc -le 0){$sc=1.0}
$qn=$env:WC_UIA_NAME;$qr=$env:WC_UIA_ROLE;$qi=$env:WC_UIA_AUTOID
$els=$win.FindAll($TS,$CT)
$hit=0;$max=200
foreach($e in $els){
  if($hit -ge $max){break}
  try{
    $ct=$e.Current.ControlType.ProgrammaticName -replace 'ControlType\.',''
    $nm=$e.Current.Name;$id=$e.Current.AutomationId
    $ok=$true
    if($qn -and ($nm -notlike "*$qn*")){$ok=$false}
    if($qr -and ($ct -notlike "*$qr*")){$ok=$false}
    if($qi -and ($id -ne $qi)){$ok=$false}
    if($ok){
      $r=$e.Current.BoundingRectangle
      $rx=[int](($r.X-$wr.X)*$sc);$ry=[int](($r.Y-$wr.Y)*$sc)
      $line="[$ct] `"$nm`" ($rx,$ry $([int]($r.Width*$sc))x$([int]($r.Height*$sc)))"
      if($id){$line+=" id=$id"}
      Write-Output $line;$hit++
    }
  }catch{}
}
if($hit -eq 0){Write-Output '(没有匹配的元素)'}
"#;

const POINT_PS: &str = r#"
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$AE=[System.Windows.Automation.AutomationElement]
$hwnd=[IntPtr][int64]$env:WC_UIA_HWND
try{$win=$AE::FromHandle($hwnd)}catch{$win=$null}
if($null -eq $win){Write-Output 'ERR: 未找到窗口';exit 0}
$wr=$win.Current.BoundingRectangle
$px=$wr.X+[int]$env:WC_UIA_X;$py=$wr.Y+[int]$env:WC_UIA_Y
$pt=New-Object System.Windows.Point($px,$py)
$el=$AE::FromPoint($pt)
if($null -eq $el){Write-Output '(该点无 UIA 元素)';exit 0}
$ct=$el.Current.ControlType.ProgrammaticName -replace 'ControlType\.',''
$line="[$ct] `"$($el.Current.Name)`""
if($el.Current.AutomationId){$line+=" id=$($el.Current.AutomationId)"}
Write-Output $line
"#;

/// 共享：按 name/role/automation_id 在窗口里找第一个匹配元素到 $el。
const FIND_FIRST_FN: &str = r#"
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$AE=[System.Windows.Automation.AutomationElement]
$TS=[System.Windows.Automation.TreeScope]::Descendants
$CT=[System.Windows.Automation.Condition]::TrueCondition
$hwnd=[IntPtr][int64]$env:WC_UIA_HWND
try{$win=$AE::FromHandle($hwnd)}catch{$win=$null}
if($null -eq $win){Write-Output 'ERR: 未找到窗口';exit 0}
$wr=$win.Current.BoundingRectangle
$sc=[double]$env:WC_UIA_SCALE
if($sc -le 0){$sc=1.0}
$qn=$env:WC_UIA_NAME;$qr=$env:WC_UIA_ROLE;$qi=$env:WC_UIA_AUTOID
$els=$win.FindAll($TS,$CT)
$el=$null
foreach($e in $els){
  try{
    $ct=$e.Current.ControlType.ProgrammaticName -replace 'ControlType\.',''
    $nm=$e.Current.Name;$id=$e.Current.AutomationId
    $ok=$true
    if($qn -and ($nm -notlike "*$qn*")){$ok=$false}
    if($qr -and ($ct -notlike "*$qr*")){$ok=$false}
    if($qi -and ($id -ne $qi)){$ok=$false}
    if($ok){$el=$e;break}
  }catch{}
}
if($null -eq $el){Write-Output 'ERR: 没有匹配的元素';exit 0}
"#;

const CLICK_TAIL: &str = r#"
try{
  $p=$el.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
  $p.Invoke()
  Write-Output ("已 Invoke 元素 `"" + $el.Current.Name + "`"")
}catch{
  $r=$el.Current.BoundingRectangle
  $cx=[int](($r.X-$wr.X+$r.Width/2)*$sc);$cy=[int](($r.Y-$wr.Y+$r.Height/2)*$sc)
  Write-Output ("该元素不支持 Invoke；其中心在截图上的坐标为 ($cx,$cy)，可用 window_click 点击")
}
"#;

const SETVAL_TAIL: &str = r#"
$val=$env:WC_UIA_VALUE
try{
  $p=$el.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
  $p.SetValue($val)
  Write-Output ("已设值元素 `"" + $el.Current.Name + "`"")
}catch{
  $r=$el.Current.BoundingRectangle
  $cx=[int](($r.X-$wr.X+$r.Width/2)*$sc);$cy=[int](($r.Y-$wr.Y+$r.Height/2)*$sc)
  Write-Output ("该元素不支持 ValuePattern；可 window_click ($cx,$cy)（截图上的坐标）聚焦后 window_type 输入")
}
"#;

pub struct UiTree;

impl Tool for UiTree {
    fn name(&self) -> &'static str {
        "ui_tree"
    }
    fn description(&self) -> &'static str {
        "Read the UIA element tree of a window (control type / name / coordinates as measured on the capture_window image / AutomationId). Capped at the first 250 elements. Works for standard apps (Notepad, File Explorer, WPF, Office); may be empty for custom-drawn UIs."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hwnd": { "type": "integer", "description": "Target window handle (from list_windows)" }
            },
            "required": ["hwnd"]
        })
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "读取 UIA 树 窗口 {}",
            args.get("hwnd").and_then(Value::as_i64).unwrap_or(0)
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let env = base_env(args)?;
        ps_result(run_ps(UI_TREE_PS, &env, UIA_TIMEOUT_SECS)?)
    }
}

pub struct FindElement;

impl Tool for FindElement {
    fn name(&self) -> &'static str {
        "find_element"
    }
    fn description(&self) -> &'static str {
        "Find UIA elements in a window by name (substring), role (substring), or automation_id (exact). Returns matches with coordinates as measured on the capture_window image. Capped at the first 200 matches."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hwnd": { "type": "integer", "description": "Target window handle" },
                "name": { "type": "string", "description": "Name substring match" },
                "role": { "type": "string", "description": "Control type substring, e.g. Button/Edit/MenuItem" },
                "automation_id": { "type": "string", "description": "AutomationId exact match" }
            },
            "required": ["hwnd"]
        })
    }
    fn summary(&self, _args: &Value) -> String {
        "查找 UIA 元素".to_string()
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let mut env = base_env(args)?;
        if !push_query(args, &mut env) {
            return Err("请至少提供 name/role/automation_id 之一".to_string());
        }
        ps_result(run_ps(FIND_PS, &env, UIA_TIMEOUT_SECS)?)
    }
}

pub struct ElementAtPoint;

impl Tool for ElementAtPoint {
    fn name(&self) -> &'static str {
        "element_at_point"
    }
    fn description(&self) -> &'static str {
        "Return the UIA element under a point in a window. The point is read straight off the capture_window image."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hwnd": { "type": "integer", "description": "Target window handle" },
                "x": { "type": "integer", "description": "X as measured on the capture_window image" },
                "y": { "type": "integer", "description": "Y as measured on the capture_window image" }
            },
            "required": ["hwnd", "x", "y"]
        })
    }
    fn summary(&self, _args: &Value) -> String {
        "坐标处 UIA 元素".to_string()
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let mut env = base_env(args)?;
        let x = args
            .get("x")
            .and_then(Value::as_i64)
            .ok_or_else(|| "缺少必填参数: x".to_string())?;
        let y = args
            .get("y")
            .and_then(Value::as_i64)
            .ok_or_else(|| "缺少必填参数: y".to_string())?;
        // 入参是模型从截图上量的「图像空间」坐标，这里换回窗口坐标再交给脚本。
        // 换算放 Rust 而不是脚本里：img_to_win 是有单测覆盖的纯函数，PowerShell 里再写一遍
        // 既无法测试也容易和 Rust 侧算法漂移。
        let hwnd = args.get("hwnd").and_then(Value::as_i64).unwrap_or(0);
        let (wx, wy) = img_to_win(x as i32, y as i32, window_scale(hwnd));
        env.push(("WC_UIA_X", wx.to_string()));
        env.push(("WC_UIA_Y", wy.to_string()));
        ps_result(run_ps(POINT_PS, &env, UIA_TIMEOUT_SECS)?)
    }
}

pub struct ClickElement;

impl Tool for ClickElement {
    fn name(&self) -> &'static str {
        "click_element"
    }
    fn description(&self) -> &'static str {
        "Find the first element in a window matching name/role/automation_id and Invoke it (press a button or menu item) via InvokePattern. If the element does not support InvokePattern, returns its center coordinates (as measured on the capture_window image) so you can retry with window_click."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hwnd": { "type": "integer", "description": "Target window handle" },
                "name": { "type": "string", "description": "Name substring match" },
                "role": { "type": "string", "description": "Control type substring, e.g. Button" },
                "automation_id": { "type": "string", "description": "AutomationId exact match" }
            },
            "required": ["hwnd"]
        })
    }
    fn requires_approval(&self) -> bool {
        true
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "点击元素 {}",
            args.get("name").and_then(Value::as_str).unwrap_or("")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let mut env = base_env(args)?;
        if !push_query(args, &mut env) {
            return Err("请至少提供 name/role/automation_id 之一".to_string());
        }
        let script = format!("{FIND_FIRST_FN}{CLICK_TAIL}");
        ps_result(run_ps(&script, &env, UIA_TIMEOUT_SECS)?)
    }
}

pub struct SetElementValue;

impl Tool for SetElementValue {
    fn name(&self) -> &'static str {
        "set_element_value"
    }
    fn description(&self) -> &'static str {
        "Find the first element matching name/role/automation_id and set its value via ValuePattern (text box, combo box). If the element does not support ValuePattern, returns its center coordinates (as measured on the capture_window image) so you can retry with window_click to focus it then window_type to enter text."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "hwnd": { "type": "integer", "description": "Target window handle" },
                "value": { "type": "string", "description": "Value to set" },
                "name": { "type": "string", "description": "Name substring match" },
                "role": { "type": "string", "description": "Control type substring, e.g. Edit" },
                "automation_id": { "type": "string", "description": "AutomationId exact match" }
            },
            "required": ["hwnd", "value"]
        })
    }
    fn requires_approval(&self) -> bool {
        true
    }
    fn summary(&self, _args: &Value) -> String {
        "设置元素值".to_string()
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let mut env = base_env(args)?;
        let value = require_str(args, "value")?;
        if !push_query(args, &mut env) {
            return Err("请至少提供 name/role/automation_id 之一".to_string());
        }
        env.push(("WC_UIA_VALUE", value));
        let script = format!("{FIND_FIRST_FN}{SETVAL_TAIL}");
        ps_result(run_ps(&script, &env, UIA_TIMEOUT_SECS)?)
    }
}

#[cfg(test)]
mod smoke {
    // 仅 Windows 编译运行；会真起 powershell（~270ms），不进 Linux CI。
    use super::*;

    #[test]
    fn run_ps_echoes_env_value() {
        // 验证运行器：env 传值 + -EncodedCommand 编码 + 抽干管道 都通。
        let out = run_ps(
            "Write-Output $env:WC_TEST_IN",
            &[("WC_TEST_IN", "hello-uia".to_string())],
            15,
        )
        .expect("powershell 应可运行");
        assert_eq!(out, "hello-uia");
    }

    // 下面 5 个：传 hwnd=0 → FromHandle 取不到窗口 → 脚本应回 "未找到窗口"。
    // 这同时证明：每个静态脚本能被 PowerShell 解析（语法对）、env 传值通、整链路通。
    // 不依赖任何真实窗口、无副作用；稳定可重复。

    fn assert_window_not_found(r: ToolResult) {
        match r {
            Err(e) => assert!(e.contains("未找到窗口"), "应是「未找到窗口」，实际: {e}"),
            Ok(o) => panic!("hwnd=0 应报错，却得到: {o}"),
        }
    }

    #[test]
    fn ui_tree_script_parses() {
        assert_window_not_found(UiTree.execute(&json!({ "hwnd": 0 })));
    }

    #[test]
    fn find_element_script_parses() {
        assert_window_not_found(FindElement.execute(&json!({ "hwnd": 0, "name": "x" })));
    }

    #[test]
    fn element_at_point_script_parses() {
        assert_window_not_found(ElementAtPoint.execute(&json!({ "hwnd": 0, "x": 1, "y": 1 })));
    }

    #[test]
    fn click_element_script_parses() {
        assert_window_not_found(ClickElement.execute(&json!({ "hwnd": 0, "name": "x" })));
    }

    #[test]
    fn set_element_value_script_parses() {
        assert_window_not_found(
            SetElementValue.execute(&json!({ "hwnd": 0, "value": "v", "name": "x" })),
        );
    }
}
