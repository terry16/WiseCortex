//! invoke_skill：按需把某技能的指令拉进对话，让 agent 据此执行任务。
//! 每次调用都从磁盘实时加载技能集合 —— 这样新建的技能无需重启即可调用。
//! 返回技能正文 + 任务提示；找不到则返回可用技能清单供模型自纠。

use std::path::PathBuf;

use serde_json::{json, Value};

use super::{require_str, Tool, ToolResult};
use crate::skill::SkillSet;

pub struct InvokeSkill {
    dirs: Vec<PathBuf>,
    /// 钉选的技能名（任务级）；非空时只在这些技能里查找。空=全量。
    only: Vec<String>,
}

impl InvokeSkill {
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        Self {
            dirs,
            only: Vec::new(),
        }
    }

    /// 限定本工具只能调用 `only` 列出的技能（任务钉选技能用）。
    pub fn with_only(dirs: Vec<PathBuf>, only: Vec<String>) -> Self {
        Self { dirs, only }
    }
}

impl Tool for InvokeSkill {
    fn name(&self) -> &'static str {
        "invoke_skill"
    }
    fn description(&self) -> &'static str {
        "Invoke a skill (a reusable playbook) to handle a specific task. Use it whenever the user's \
         request matches a skill's description: it returns that skill's detailed instructions, which \
         you must follow strictly when carrying out the `task` you passed in. See AVAILABLE SKILLS in \
         the system prompt for what is available. To create a new skill, call \
         invoke_skill(\"skill-creator\", ...)."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "skill_name": { "type": "string", "description": "Name of the skill" },
                "task": { "type": "string", "description": "The task to hand off to that skill" }
            },
            "required": ["skill_name", "task"]
        })
    }
    fn summary(&self, args: &Value) -> String {
        format!(
            "技能 {}",
            args.get("skill_name")
                .and_then(Value::as_str)
                .unwrap_or("?")
        )
    }
    fn execute(&self, args: &Value) -> ToolResult {
        let skill_name = require_str(args, "skill_name")?;
        let task = args.get("task").and_then(Value::as_str).unwrap_or("");
        // 实时加载（新技能即时可用），排除已停用的技能，并按任务钉选过滤。
        let skills = SkillSet::load_dirs(&self.dirs)
            .without_disabled(&crate::skills_state::disabled())
            .only(&self.only);

        match skills.find(&skill_name) {
            Some(skill) => {
                // 内联正文里的 @file 引用（相对技能目录），对齐 Claude Code 行为。
                let instructions = match &skill.dir {
                    Some(dir) => crate::skill::expand_references(&skill.instructions, dir),
                    None => skill.instructions.clone(),
                };
                // 告知技能的绝对目录：正文里的 `reference/...`、`scripts/...` 等相对路径都相对它。
                // 否则 agent 的 cwd 通常是任务工作目录（如 mudlib），裸相对读 reference/ 会失败、
                // 再去 glob 找——白白浪费工具调用、逼近上限。
                let dir_note = match &skill.dir {
                    Some(dir) => format!(
                        "技能目录（SKILL_DIR）= `{0}`\n\
                         正文里出现的 `reference/...`、`scripts/...`、`<技能目录>/...` 等路径，\
                         一律按【绝对路径】拼到此目录下再读取/执行（例如读 `{0}/reference/skill-types.md`、\
                         跑 `python {0}/scripts/gen_skill.py`）。**不要用裸相对路径**——你的工作目录不是技能目录。\n\n",
                        dir.display()
                    ),
                    None => String::new(),
                };
                Ok(format!(
                    "# 技能：{}\n\n{}{}\n\n---\n现在按上述指令处理任务：{}",
                    skill.name, dir_note, instructions, task
                ))
            }
            None => {
                let names = skills.names().join(", ");
                Ok(format!(
                    "未找到技能「{skill_name}」。可用技能：{}",
                    if names.is_empty() { "(无)" } else { &names }
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个临时技能目录并返回 (InvokeSkill, dir)；dir 需在调用后保留。
    /// `tag` 保证不同测试用不同目录，避免并行清理互相干扰。
    fn setup(tag: &str) -> (InvokeSkill, PathBuf) {
        let dir = std::env::temp_dir().join(format!("wc-invskill-{}-{tag}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let sk = dir.join("greeter");
        std::fs::create_dir_all(&sk).unwrap();
        std::fs::write(
            sk.join("SKILL.md"),
            "---\nname: greeter\ndescription: 打招呼\n---\n说你好",
        )
        .unwrap();
        (InvokeSkill::new(vec![dir.clone()]), dir)
    }

    #[test]
    fn returns_instructions_for_known_skill() {
        let (t, dir) = setup("known");
        let out = t
            .execute(&json!({ "skill_name": "greeter", "task": "对小明" }))
            .unwrap();
        assert!(out.contains("说你好"));
        assert!(out.contains("对小明"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inlines_at_file_references_from_skill_dir() {
        let dir = std::env::temp_dir().join(format!("wc-invskill-{}-atref", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let sk = dir.join("tdd");
        std::fs::create_dir_all(&sk).unwrap();
        std::fs::write(
            sk.join("SKILL.md"),
            "---\nname: tdd\ndescription: x\n---\n详见 @anti-patterns.md",
        )
        .unwrap();
        std::fs::write(sk.join("anti-patterns.md"), "别 mock 一切 MARKER").unwrap();

        let t = InvokeSkill::new(vec![dir.clone()]);
        let out = t
            .execute(&json!({ "skill_name": "tdd", "task": "做 TDD" }))
            .unwrap();
        assert!(out.contains("MARKER"), "@file 引用的内容应被内联进指令");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lists_skills_when_not_found() {
        let (t, dir) = setup("notfound");
        let out = t
            .execute(&json!({ "skill_name": "nope", "task": "x" }))
            .unwrap();
        assert!(out.contains("未找到"));
        assert!(out.contains("greeter"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
