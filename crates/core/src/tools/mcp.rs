//! 把已连接的 MCP 服务器工具包装成 WiseCortex `Tool`，以 `mcp__<server>__<tool>` 暴露给 agent。

use serde_json::Value;

use super::{Tool, ToolResult};
use crate::mcp::McpToolInfo;

pub struct McpTool {
    info: McpToolInfo,
}

impl McpTool {
    pub fn new(info: McpToolInfo) -> Self {
        Self { info }
    }
}

impl Tool for McpTool {
    fn name(&self) -> &'static str {
        self.info.full_name
    }
    fn description(&self) -> &'static str {
        self.info.description
    }
    fn parameters(&self) -> Value {
        self.info.input_schema.clone()
    }
    fn summary(&self, _args: &Value) -> String {
        format!("MCP {}·{}", self.info.server, self.info.tool)
    }
    /// 外部副作用工具：非自动模式下执行前确认（solo 会绕过）。
    fn requires_approval(&self) -> bool {
        true
    }
    fn execute(&self, args: &Value) -> ToolResult {
        crate::mcp::call_tool(
            &self.info.server,
            &self.info.tool,
            self.info.kind,
            args.clone(),
        )
    }
}
