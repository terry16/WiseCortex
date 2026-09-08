//! todo_write：让 agent 把计划写成清单（无状态——模型每次提交完整清单，工具校验并回显）。
//! 与 Claude Code 的 TodoWrite 同理，便于模型组织多步任务，结果显示在工具折叠区。

use serde_json::{json, Value};

use super::{Tool, ToolResult};

pub struct TodoWrite;

impl Tool for TodoWrite {
    fn name(&self) -> &'static str {
        "todo_write"
    }
    fn description(&self) -> &'static str {
        "Maintain the todo list for the current task. This tool is stateless: pass the COMPLETE list every time, not just the changes. Use it to plan and track progress on multi-step tasks."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "The complete todo list (every item, not just changed ones)",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": { "type": "string" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed"]
                            }
                        },
                        "required": ["content", "status"]
                    }
                }
            },
            "required": ["todos"]
        })
    }
    fn summary(&self, args: &Value) -> String {
        let n = args
            .get("todos")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        format!("更新待办（{n} 项）")
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let todos = args
            .get("todos")
            .and_then(Value::as_array)
            .ok_or_else(|| "缺少 todos 数组".to_string())?;
        let mut out = String::new();
        for t in todos {
            let content = t.get("content").and_then(Value::as_str).unwrap_or("");
            let status = t.get("status").and_then(Value::as_str).unwrap_or("pending");
            let mark = match status {
                "completed" => "[x]",
                "in_progress" => "[~]",
                _ => "[ ]",
            };
            out.push_str(&format!("{mark} {content}\n"));
        }
        if out.is_empty() {
            out.push_str("(清单为空)");
        }
        Ok(out.trim_end().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_checklist() {
        let out = TodoWrite
            .execute(&json!({ "todos": [
                { "content": "读代码", "status": "completed" },
                { "content": "改 bug", "status": "in_progress" },
                { "content": "写测试", "status": "pending" }
            ]}))
            .unwrap();
        assert!(out.contains("[x] 读代码"));
        assert!(out.contains("[~] 改 bug"));
        assert!(out.contains("[ ] 写测试"));
    }
}
