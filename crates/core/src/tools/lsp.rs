//! lsp 工具：把语义能力暴露给 agent（定义跳转/查找引用/悬停类型/文档符号/诊断）。
//! 需本机装有对应语言服务器；未装则返回提示。只读，免确认。

use std::path::PathBuf;

use serde_json::{json, Value};

use super::{require_str, resolve, Tool, ToolResult};

pub struct Lsp {
    base: PathBuf,
}
impl Lsp {
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }
}

impl Tool for Lsp {
    fn name(&self) -> &'static str {
        "lsp"
    }
    fn description(&self) -> &'static str {
        "Semantic code navigation (language server): operation ∈ definition|references|hover|documentSymbol|diagnostics. \
         definition/references/hover require file plus 1-based line/character (the line/column shown in an editor); \
         documentSymbol/diagnostics require only file. More accurate than grep (understands symbols and types). \
         Requires the matching language server installed locally (rust-analyzer / typescript-language-server / pyright / gopls / clangd)."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": { "type": "string", "enum": ["definition","references","hover","documentSymbol","diagnostics"] },
                "file": { "type": "string", "description": "File path (relative to the working directory, or absolute)" },
                "line": { "type": "integer", "description": "Line number (1-based); required for definition/references/hover" },
                "character": { "type": "integer", "description": "Column number (1-based); required for definition/references/hover" }
            },
            "required": ["operation", "file"]
        })
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "lsp {} {}",
            args.get("operation").and_then(Value::as_str).unwrap_or("?"),
            args.get("file").and_then(Value::as_str).unwrap_or("?")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let operation = require_str(args, "operation")?;
        let file = require_str(args, "file")?;
        let line = args.get("line").and_then(Value::as_u64).unwrap_or(0) as u32;
        let character = args.get("character").and_then(Value::as_u64).unwrap_or(0) as u32;
        if matches!(operation.as_str(), "definition" | "references" | "hover")
            && (line == 0 || character == 0)
        {
            return Err(format!("{operation} 需要 1-based 的 line 与 character"));
        }
        let abs = resolve(&self.base, &file);
        crate::lsp::run(&self.base, &abs, &operation, line, character)
    }
}
