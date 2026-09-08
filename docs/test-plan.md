# WiseCortex 测试流程与计划（全覆盖）

> 目标：在每次发版前，按本文从「自动化」到「手动集成」逐层验证，确保协议、LLM、
> 工具、技能/市场、IM 三件套、定时任务、桌面端打包均可用。
> 约定：✅=预期通过项；⚠️=已知限制/需人工判断。
>
> **维护约定**：每新增/改动一个功能，就到 §8（近期新增）补一条「测什么 + 怎么测」；稳定后再并入对应分层章节。能自动化的写进 `cargo test`/`vitest`，UI/真机依赖的列为手动并写清步骤。最后更新：2026-06-05（含后台长任务 §8.9 / 计划模式 §8.10 / 国产模型深度思考 §8.12）。

---

## 0. 测试分层总览

| 层 | 范围 | 手段 | 触发 |
|----|------|------|------|
| L1 单元/集成（自动） | core / server / cli / web | `cargo test` + `vitest` | 本地 + CI 每次 push |
| L2 静态质量（自动） | 格式 / lint / 类型 | `cargo fmt --check`、`clippy -D warnings`、`biome`、`tsc` | 本地 + CI |
| L3 端到端（手动） | 浏览器 WebUI ↔ server ↔ LLM | 浏览器 + 真实/Mock API key | 发版前 |
| L4 外部集成（手动） | IM 回调、通知通道、cron | 飞书/企微/QQ 平台 + 公网回调 | 涉及 IM 改动时 |
| L5 桌面端（手动） | Tauri 安装包 | `cargo tauri build` 产物安装运行 | 发版前 |

当前自动化规模基线：**core ~110 · server(lib 21 + rest 9 + ws 4) · web 26 · llm_client 3**（持续增长）。

---

## 1. 环境准备

```powershell
# Rust 上 PATH（本机 cargo 装在 .cargo，未必在 PATH）
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

# 依赖就绪
cargo --version ; node --version ; npm --version
cd web ; npm ci ; cd ..
```

- 配置一次模型（DeepSeek 示例）：
  ```powershell
  cargo run -p wisecortex-cli -- config set --provider deepseek --api-key <KEY> --model deepseek-chat
  cargo run -p wisecortex-cli -- config show     # 确认 api_key_set=true，不回明文 ✅
  ```
- ⚠️ 配置/凭据写在用户数据目录（`%APPDATA%\wisecortex\`），不入库；测试机与生产隔离。
- server 实例若在跑会锁 `wisecortex-server.exe`（Windows 锁运行中的 exe），cargo 链接会失败 → **先停掉实例再跑测试**。
  别用 `--target-dir` 另开构建树绕过：那会多出一份完整产物（几十 GB），且相对路径在子目录下执行会就地再生成一棵。

---

## 2. L1 + L2 自动化测试（先跑这一层，全绿再往下）

### 2.1 Rust
```powershell
cargo fmt --all --check                         # 格式 ✅ 零输出
cargo clippy --all-targets -- -D warnings        # lint ✅ 无 warning
cargo test                                       # 全量单元/集成 ✅
```
覆盖要点：
- **core**：LLM 两种 wire format 流式解析、定价/成本、压缩阈值、skill 解析、cron 解析、
  notify 各通道 body、**wecom 加解密（官方向量）**、**marketplace catalog/parse/install 往返**。
- **server**：providers/config GET、会话列表/历史回放（**含 images**）/删除、channels/cron 列表、
  **skills/catalog 内置项**、IM onebot 忽略非消息、**feishu url_verification 回显**、access_key 鉴权矩阵、WS 订阅广播。
- **cli**：子命令解析（config/skill/channel/cron/im）。

### 2.2 Web
```powershell
cd web
npx biome check .      # ✅
npx tsc --noEmit       # ✅
npx vitest run         # ✅ ws/dispatcher/sessions
cd ..
```
覆盖要点：WS 传输重连、dispatcher 事件路由、sessions 渲染（流式气泡、工具折叠、
历史回放 user/assistant/tool、**图片缩略图回放**）。

### 2.3 CI
- push 到 `main` 或开 PR 自动跑 `rust` + `web` 两个 job。两个都绿即 L1/L2 通过。

---

## 3. L3 端到端（手动，浏览器 WebUI）

启动：
```powershell
cargo run -p wisecortex-server      # 默认 :7070
cd web ; npm run dev              # :5173，浏览器开 http://localhost:5173
```

### 3.1 配置与鉴权
1. 打开 ⚙设置，确认能读到当前 provider/model；不显示明文 key ✅。
2. 改 auto_approve / access_key 保存 → 热重载、无需重启 ✅。
3. 设了 access_key 后，未带 key 的请求被拒（WS 连接失败 / REST 401）；带正确 key 放行 ✅。

### 3.2 LLM 对话
4. 发一条消息：助手**流式增量**逐字出现，结束定稿为 markdown（代码高亮）✅。
5. 多轮上下文连续；长对话触发**压缩**（日志可见）后仍连贯 ✅。
6. 成本：界面/日志显示本轮 token 与**人民币 ¥** 成本；DeepSeek 命中缓存价更低 ✅。
7. 切换 provider（openai/anthropic/deepseek/qwen/hunyuan/gemini）各自能出字 ✅。

### 3.3 工具与 agent 能力
8. 让它**读/改文件**：`read_file`/`write_file`/`edit_file` 生效；auto_approve=false 时执行前弹确认 ✅。
9. `glob`/`grep` 能定位文件与内容 ✅。
10. `shell`：正常输出、超时被中断、`background=true` 返回 pid+日志文件（`read_file` 看日志、`shell kill` 停）✅。
11. `web_fetch`/`web_search`：能取回网页/检索结果 ✅。
12. `todo_write`：计划清单渲染 ✅。
13. **子 agent**：`task` 工具单个执行；一回合多个 `task` **并行**（join_all）✅。
14. `invoke_skill`：加载 SKILL.md 指令并照做 ✅。
15. **中断**：运行中点「停止」即时停下 ✅。

### 3.4 图片输入 + 历史回放（本次新增）
16. 用 📎 附图 + 文本发送：vision 模型能描述图片（DeepSeek 文本-only 则忽略图，⚠️ 预期）✅。
17. 发送后用户气泡显示**缩略图** ✅。
18. **切到别的会话再切回**（或刷新后进该会话）：历史回放时该用户气泡的**图片缩略图重新画出** ✅（本次修复点）。

### 3.5 会话管理
19. 侧边栏：新建/切换/删除会话；首条消息后**自动命名** ✅。
20. 历史回放：user/assistant/tool 消息、工具折叠块、图片均正确重画 ✅。

### 3.6 技能市场（本次新增）
21. 点 🧩技能：列出**内置精选**（commit-helper / pr-writer / refactorer / security-reviewer，标「内置」）✅。
22. 点「安装」→ 状态变「已安装」；磁盘 `%APPDATA%\wisecortex\skills\<name>\SKILL.md` 出现 ✅。
23. 安装后**立即可 invoke_skill**（实时磁盘加载，无需重启）✅。
24. 点「卸载」→ 目录删除、列表回到未安装 ✅。
25. 填一个**远程 registry JSON 地址**保存 → 列表追加「远程」条目；安装远程项会拉取其 SKILL.md ✅。
    - registry JSON 形如：`[{"name":"x","description":"...","url":"https://.../SKILL.md"}]` 或内联 `"content"`。
    - ⚠️ 远程拉取失败时静默只显示内置项（设计如此，避免卡 UI）。

---

## 4. L4 外部集成（手动）

> 这些需公网可达回调与平台侧配置；改 IM/通知相关代码时必测。本机用 `ngrok`/内网穿透
> 暴露 `:7070`。`/api/im/*` 免 access_key（平台无法带我们的密钥）。

### 4.1 通知通道（出站）
26. CLI 建通道并发测试：
    ```powershell
    cargo run -p wisecortex-cli -- channel add --name t --kind webhook --url <URL>
    cargo run -p wisecortex-cli -- channel list
    ```
    feishu / wecom（官方机器人 webhook）/ onebot / 通用 webhook 各能收到消息 ✅。
    ⚠️ 微信**个人号**无官方 API，不支持。

### 4.2 定时任务（cron）
27. ```powershell
    cargo run -p wisecortex-cli -- cron add --name daily --interval 30s --prompt "说早安" --channel t
    cargo run -p wisecortex-cli -- cron list
    ```
    到点（30s tick）自动跑 `run_once` → 结果推到通道 → `cron logs <id>` 有记录 ✅。
28. Web ⏰任务 面板：增删启停、看日志、配通道，与 CLI 等价 ✅。
29. enable/disable 即时生效；disabled 不再触发 ✅。

### 4.3 IM 双向
30. **QQ（OneBot/NapCat）**：自建 NapCat，HTTP 上报到 `:7070/api/im/onebot`，并配一个 onebot 通道做回复基地址。
    群里 `wc <问题>` 触发、私聊直接触发 → 回原群/原人 ✅。
    - **多轮上下文**：同群连问两句（第二句用「刚才那个」指代）→ 答得上 ✅。侧栏应出现名为「QQ · …」的会话。
    - **会话隔离**：两个不同的群各问各的 → 互不串味 ✅（会话 id 为 `onebot-g<群号>` / `onebot-u<QQ号>`）。
    - **不自问自答**：NapCat 若开了 `reportSelfMessage`，私聊里机器人回完不应再触发自己 ✅。
    - **命令**：群里 `wc /sessions`、`wc /switch <序号>` 可用，切完该群就跑在目标会话上
      （带着它的知识库与模型）✅。私聊的绑定与群**互不影响**。
    - **命令不回群**：群里发命令，结果应**私聊**回给发起人，群里一个字不留 ✅
      （`/sessions` 会列出全部会话名，漏进客服群就等于把手上的活念给客户听）。
    - **白名单**：配 `onebot_admins`（或 env `WC_ONEBOT_ADMINS=123,456`）后，名单外的人发
      `/sessions` 应被静默忽略、普通提问照常回答 ✅。留空=不限制（自用群原样）。
    ⚠️ 非官方、有 ToS 风险。详见 `docs/qq-onebot.md`。
31. **飞书**：两条收消息路径，本地优先长连接。
    - **长连接（本地优先，免公网）**：通道页「扫码连接」拿 app_id/secret → 开「长连接」→ **重启服务端**。
      前置（飞书开发者后台）：①权限加 `im:message`；②事件订阅选**长连接**并订阅「接收消息」；③**发布版本**。
      启动日志应见 `feishu: 端点已协商…` → `长连接已建立，等待事件…`；@机器人/私聊文本 →
      `收到消息（chat=…）` → `已回复` ✅。非文本事件打印 `收到非文本事件` 而不静默 ✅。
      ⚠️ 长连接协议为自行实现、**未经真机验证**；连不上/不回时按日志（端点 host、wss 失败原因）排查。
    - **事件订阅回调（仅公网部署）**：有公网地址时把 `http://<公网>:7070/api/im/feishu` 填入事件订阅；
      URL 验证回显 challenge ✅；配了 verify_token 时来源不符被拒 ✅。
      CLI：`wisecortex im feishu --app-id .. --app-secret .. --verify-token ..`。
32. **企业微信**（仅公网回调，重点测加解密）：自建应用，回调 URL `http://<公网>:7070/api/im/wecom`。
    - 配置：**通道页「配置接收凭据」**填 corp_id/secret/agent_id/token/aes_key（→ `POST /api/wecom/config`），
      或 CLI `wisecortex im wecom --corp-id .. --corp-secret .. --agent-id .. --token .. --aes-key ..`。
    - **GET 验证**：平台「保存」回调配置时，服务端验签+解密 echostr 原样返回 → 平台显示**验证成功** ✅。
    - **POST 收消息**：给应用发文本 → 验签通过 → 解密取 Content/FromUserName → `run_once` →
      应用消息 API 主动回到该用户 ✅。
    - 错误用例：篡改 `msg_signature` → 403；错误密文 → 400 ✅。
    - ⚠️ 企业微信**无长连接**，必须公网 HTTPS 回调；主动回复经 `message/send`，需 agent_id 且可见成员。
33. **微信 ClawBot**（个人号官方 Bot API，协议 iLink；纯 HTTPS 长轮询，免公网，桌面端同样可用）。
    - 接入：通道页「扫码接入」→ 手机微信扫码并确认 → 卡片显示 Bot ID、开关自动打开 ✅。
      灰度中，微信里看不到 ClawBot 入口就没法测。
    - 收发：微信里发一句话 → 日志 `clawbot: 收到消息（from=…）` → 侧栏出现「微信 · …」会话 → 微信收到回复 ✅。
    - 多轮与命令：连问两句上下文连贯；`/sessions` 列表、`/switch <n>` 切换、`/reset` 清空 ✅
      （ClawBot 无互动卡片，走纯文本版）。
    - 排队：agent 干活时再发一句 → 排队、不交错 ✅。
    - 语音：发语音 → 按服务端转写文本回答 ✅。
    - 不支持的内容：发图片/文件 → 明确回「本版还不支持图片/文件」，**不静默吞掉** ✅。
    - 正在输入：长任务执行期间手机上能看到「对方正在输入」，结束后消失 ✅（拿不到 ticket 时静默跳过，不影响回复）。
    - 开关：关掉 → 5 秒内停止轮询；重开 → 恢复 ✅。
    - 游标持久化：重启服务端 → **不重复回答**重启前已答过的消息 ✅（游标存 `clawbot_cursor.json`）。
    - ⚠️ 一个微信号只能建一个 Bot，同步游标按 Bot 共享——**同一时刻只能在一处开启**，
      两台机器同开会把消息随机分走一半（现象：一半消息没人回）。

---

## 5. L5 桌面端打包（手动）

```powershell
cd web ; npm run build ; cd ..        # 先产出 web/dist（Tauri frontendDist 指向它）
cargo tauri build                     # 需 tauri-cli（cargo install tauri-cli --version ^2.0）
```
33. 构建产出 Windows 安装包（`desktop/src-tauri/target/release/bundle/` 下 `.msi`/`.exe`）✅。
34. 安装后启动 WiseCortex 桌面窗口，能正常对话（指向本地 server）✅。
35. ⚠️ 目标机需 WebView2 运行时（Win10/11 多数自带；缺则安装器/系统补装）。
36. ⚠️ 默认 `cargo test`/CI **不含 desktop**（避免 Linux 缺 webkit）；桌面端仅在 mac/win 显式构建。
37. ⚠️ **换了图标必须确认真的重嵌进 exe**：tauri-build 没为图标声明 rerun-if-changed，
    我们在 `build.rs` 里补了；若哪天升级 tauri-build 后图标又不更新，先查这里。
    验证手法：`touch desktop/src-tauri/icons/icon.ico` 后 `cargo check -p wisecortex-desktop`
    应打印 `Compiling wisecortex-desktop`（不打印＝又不重嵌了，exe 里还是旧图）。

### 5.1 macOS 签名 / 公证（需付费 Apple 开发者账号）

仓库内已备齐：`Info.plist`（NSAppleEventsUsageDescription）、`entitlements.plist`
（apple-events，且刻意不含 app-sandbox）、`icons/icon.icns`、`bundle.macOS` 配置。

```bash
cd web && npm run build && cd ..
export APPLE_SIGNING_IDENTITY="Developer ID Application: <你的名字> (<TEAMID>)"   # security find-identity -v -p codesigning 可查
# 公证（二选一）
export APPLE_ID="you@example.com" APPLE_PASSWORD="<应用专用密码>" APPLE_TEAM_ID="<TEAMID>"
# 或：APPLE_API_ISSUER / APPLE_API_KEY / APPLE_API_KEY_PATH
cargo tauri build
```

38. 产出 `.app` / `.dmg`（`desktop/src-tauri/target/release/bundle/`）✅。
39. **为什么必须用 Developer ID 而不是 ad-hoc**：TCC（屏幕录制/辅助功能/自动化）把授权
    绑在应用身份上。ad-hoc 签名的身份是 **cdhash**——每次重编都变，于是每编一次就要
    重新授权一遍；Developer ID 的身份是 TeamID + bundle ID，**重编不变，授权一次长期有效**。
40. ⚠️ 首次运行仍需人工在 系统设置 → 隐私与安全性 里开：**屏幕录制**（截图/窗口标题）、
    **辅助功能**（合成点击/按键）。两个开关互相独立。**自动化**（窗口置前）会自动弹框。
41. ⚠️ 别开 App Sandbox：沙箱下 AX API 全部失效（`AXIsProcessTrusted()` 仍返 true 但每个
    调用都 `kAXErrorCannotComplete`），Phase 2 的 ui_tree/find_element 会直接废掉。

---

## 6. 发版前回归 Checklist（精简版）

- [ ] `cargo fmt --check` / `clippy -D warnings` / `cargo test` 全绿
- [ ] `biome` / `tsc` / `vitest` 全绿
- [ ] CI 两 job 绿
- [ ] 浏览器：流式对话、工具执行、中断、成本显示
- [ ] 图片输入 + **图片历史回放**
- [ ] 技能：invoke + **市场安装/卸载/配源**
- [ ] cron 到点执行 + 日志 + 推通道
- [ ] IM：微信 ClawBot 扫码 + 收发；飞书 / 企业微信 GET 验证 + 收发；（涉及 QQ 时）OneBot 收发
- [ ] 鉴权：access_key 开/关两态正确
- [ ] 桌面包构建 + 安装 + 启动对话

---

## 8. 近期新增功能（2026-06 任务重构 + 工具对齐批次）

> 这些大多是 UI/集成/真机依赖，自动化只覆盖了逻辑层；稳定后并入上面分层章节。

### 8.1 任务 / 工作空间 左栏（替代会话历史抽屉）
- **自动**：`vitest` sessions —「任务」段（无工作目录）、「工作空间」段（按目录叶名分组）、运行中行右侧转圈。`cargo test -p wisecortex-server` — TaskConfig 持久化、全局 feed（状态/重命名/删除）。
- **手动**：新建任务→无目录进「任务」、有目录进「工作空间」；**并发不打断**（A 跑时开 B，A 继续，侧栏 A 转圈，完成消失）；多标签页侧栏实时同步；删除任务连带清理其持久 shell。

### 8.2 任务级配置（模型/技能/solo/工作目录）
- **手动**：模型下拉默认显示全局当前模型名、可改；钉选技能后只用这些；权限 solo=不逐个确认且**少打断追问**、严格=每步确认；工作目录首条消息前可改、之后锁定；切任务/重开后配置仍在。

### 8.3 持久 shell（自建开发环境）
- **自动**：`cargo test -p wisecortex-core tools::shell`（env/cwd 跨调用、超时、退出码；Windows 实跑 cmd 路径）。
- **手动（真机）**：venv 建→激活→`pip install`→后续命令仍在 venv；`set/export` 跨命令可读；装工具链后 PATH 立即生效；前台卡死命令 ~5min 被终止并提示会话重置。

### 8.4 MCP 客户端（stdio + Streamable HTTP + resources/prompts + sampling/roots）
- **自动**：`cargo test -p wisecortex-core mcp` + `cargo test -p wisecortex-server mcp_sampling`。覆盖：
  node mock server 真实 stdio 握手→发现→`tools/call`（node 缺则跳过）；SSE 取响应、合成工具生成、
  resource/prompt 结果转文本、服务端反向请求分派（roots/ping/未知/sampling 转 handler）、sampling 参数→消息转换。
- **手动 · stdio（需 node/npx）**：设置页「MCP 服务器」填 `{ "filesystem": { "command":"npx","args":["-y","@modelcontextprotocol/server-filesystem","."] } }`
  保存 → 几秒后「已发现工具」列出 `mcp__filesystem__*` → 让 AI 调用 → `disabled:true` 后工具消失 → 错误 command 不影响其它服务器。
- **手动 · HTTP**：填 `{ "remote": { "url":"https://<你的 MCP 端点>", "headers":{"Authorization":"Bearer .."} } }` →
  initialize 经 POST，响应 JSON 或 SSE 均能解析；带 `Mcp-Session-Id` 的服务器后续请求自动带回。
- **手动 · resources/prompts**：连一个带 resources/prompts 能力的 server（如官方 `server-everything`）→
  「已发现工具」应多出 `mcp__<srv>__list_resources`/`read_resource`/`list_prompts`/`get_prompt` → 让 AI 调用 list 再 read/get。
- **手动 · sampling/roots（仅 stdio）**：连一个会发起 `sampling/createMessage` 的 server →
  需已配置激活 LLM；服务端拿到我们 LLM 的补全；`roots/list` 返回配置 `mcp_roots`（空=当前工作目录）。
  ⚠️ Streamable HTTP 暂不支持反向请求。

### 8.5 撰写区 / 布局
- **手动**：工作区 ~800 宽/圆角/入口无边框仅 hover；模型/权限下拉点击无蓝框描边；`＋` 上传图片走 vision、PDF/文档落 `uploads/` 并附路径；窄窗工具收起成 `›` 点击弹出；**长对话仅聊天区+左栏各自滚动、外壳不随内容拉伸**；产物预览悬浮覆盖右滑出。

### 8.6 进度动画
- **手动**：长回合/压缩上下文时进度有旋转动画 + 每秒计时（「压缩上下文… · Ns」）。

### 8.7 skill-creator（内置）
- **自动**：`cargo test -p wisecortex-core marketplace::tests::skill_creator_is_bundled_builtin`。
- **手动**：让 AI「创建一个技能」，确认**先推断+产草稿、少追问**（solo 下不提问）。

### 8.8 hooks（Phase 3，已实现）
事件：PreToolUse（可拦截工具）/ PostToolUse（追加上下文）/ UserPromptSubmit（可拦截本回合/追加上下文）/ SessionStart / Stop。配置在设置页「事件钩子」JSON：`{ "事件": [ { "matcher": "工具名正则", "command": "…", "timeout_ms": 30000 } ] }`。命令从 **stdin** 收到事件 JSON；拦截=输出 `{"decision":"block","reason":"…"}` 或非 0 退出码；追加上下文=输出 `{"additionalContext":"…"}`。
- **自动**：`cargo test -p wisecortex-core hooks`（matcher 匹配、JSON/退出码决策解析、run_one 非零拦截、run_hooks 聚合）。
- **手动（真机）**：
  1. **PreToolUse 拦截**：配 `{"PreToolUse":[{"matcher":"shell","command":"node -e \"let s='';process.stdin.on('data',d=>s+=d).on('end',()=>{let j=JSON.parse(s);if((j.tool_input.command||'').includes('rm -rf'))console.log(JSON.stringify({decision:'block',reason:'禁止 rm -rf'}))})\""}]}`，让 AI 跑 `rm -rf x` → 被拦截、原因回灌、命令不执行；跑普通命令正常。
  2. **PostToolUse 追加上下文**：配 `{"PostToolUse":[{"matcher":"write_file","command":"echo {\\\"additionalContext\\\":\\\"记得跑格式化\\\"}"}]}`，写文件后结果尾部出现 `[hook]` 提示。
  3. **UserPromptSubmit 拦截**：命令非 0 退出 → 该回合被拦截、回一条说明、不跑 agent。
  4. **SessionStart / Stop**：任务首条消息触发 SessionStart、回合结束触发 Stop（副作用，如写日志/发通知）。
  5. **零开销**：不配 hooks 时不应有额外子进程/延迟。

### 8.9 后台长任务
工具 `task_start`（异步即发即返）/ `task_list` / `task_result` / `task_stop`；「后台任务」导航面板可视化（3s 轮询，可停止）；REST `GET /api/jobs`、`DELETE /api/jobs/{id}`。
- **自动**：`cargo test -p wisecortex-server jobs`（任务管理状态机：finish 写回、二次 finish 不覆盖、stop 标记；不依赖真机 LLM）。
- **手动（真机）**：
  1. 对 AI 说「在后台跑一个长任务做 X」→ 它调 `task_start` 立即返回 job id、对话不被占住。
  2. 「后台任务」面板出现该任务（运行中）→ 完成后变「完成」并显示结果；点「停止」可中止运行中的。
  3. 让 AI `task_list`/`task_result` 自查;`task_stop` 停止。
  4. 与同步 `task` 对比：同步会卡住回合直到出结果，后台不会。

### 8.10 计划模式
任务级开关「计划」（撰写区 pill，随会话持久化）。开启后系统提示注入 PLAN_HINT，且 run_tool **拦截一切改动类工具**（requires_approval 的工具 + `task` + `task_start`）。
- **自动**：`cargo test -p wisecortex-server`（registry TaskConfig 往返含 plan_mode）。
- **手动（真机）**：
  1. 开「计划」→ 让 AI 做一个会改文件的任务 → 它只读探索、产出分步计划、**不动手**；若它试图写文件/跑 shell，被拦截并提示先出计划。
  2. 关「计划」→ 说「执行」→ AI 按计划真正动手。
  3. 与 solo 叠加：计划模式优先（即使 solo 也拦截改动，先出计划）。

### 8.11 编程能力对齐（thinking / 多处编辑 / git / LSP）
- **extended thinking**：`cargo test` 无专项（wire 层已有）；**手动**：设置页「推理强度」设 high → 跑疑难任务，观察是否更稳、耗时增加；env `WC_REASONING_EFFORT` 覆盖。
- **多处编辑**：`cargo test -p wisecortex-core tools::fs::tests::edit_multi_edits_atomic`（edits 数组顺序应用、失败不写盘）。
- **git 工具**：`cargo test -p wisecortex-core tools::git`（只读子命令白名单拒绝写操作）。**手动**：让 AI `git diff`/`git status`(免确认) 审查改动；`git_commit`(需确认) 提交。
- **LSP**：`cargo test -p wisecortex-core lsp`（Content-Length 帧编解码、URI、结果格式化）。**手动（需装语言服务器）**：
  1. 装 rust-analyzer / typescript-language-server / pyright 等；
  2. 让 AI 对某符号 `lsp definition`/`references`/`hover`、对文件 `documentSymbol`/`diagnostics`；
  3. 验证定义/引用定位准确、诊断能列出编译/类型错误；未装服务器时返回友好提示（不崩）。

### 8.12 国产模型深度思考（混元 / 千问 + 按提供商翻译思考参数）
- **背景**：深度思考的「开启」意图统一来自设置页「推理强度」，但各家参数不同；出站时按 provider 预设的 `ThinkingMode` 翻译（见 `providers.rs`）。腾讯混元新增为内置 provider，走 **OpenAI 兼容端点**（`https://api.hunyuan.cloud.tencent.com/v1`，免 TC3 签名）。
- **自动化**：
  - `cargo test -p wisecortex-core llm::openai::tests::thinking_param_varies_by_mode`（reasoning_effort→`reasoning_effort:<level>`；enable_thinking→`enable_thinking:true`；none→都不发）。
  - `cargo test -p wisecortex-core llm::openai::tests::no_thinking_param_when_effort_empty`（未开启时不发参数）。
  - `cargo test -p wisecortex-core llm::providers::tests::thinking_modes_match_provider_quirks`（qwen/hunyuan=enable_thinking、openai/gemini=reasoning_effort、deepseek=none，混元 base_url 为兼容端点）。
  - `cargo test -p wisecortex-core llm::providers::tests::override_can_set_thinking_mode`（`providers.json` 可覆盖 `thinking`）。
- **手动（需真 key）**：
  1. 设置页选 `hunyuan`（或 `qwen`），填 key，模型选混合思考模型（混元 `hunyuan-a13b`/`hunyuan-turbos-latest`，千问 `qwen3.x-max` 系）；
  2. 「推理强度」设非「关闭」→ 发一条需推理的问题，确认能正常出字（说明 `enable_thinking:true` 被接受、未 400）；
  3. 「推理强度」设「关闭」→ 简单问题应更快（思考关闭）；
  4. 混元 vision：选 `hunyuan-vision`，附图提问能识别（兼容端点用标准 `image_url` 块，无需 PascalCase）。
- **加模型不改代码**：编辑配置目录 `providers.json`（模板见 `docs/providers.example.json`，含 `thinking` 字段说明）后重启。

---

## 7. 已知限制（测试时按预期对待）

- 微信个人号已有官方 API（ClawBot / iLink，2026-03 开放），但**灰度发放**，且一号一 Bot、只支持私聊；
  图片/文件收发本版未实现。QQ 走 OneBot/NapCat 仍为非官方方案。
- DeepSeek 为文本模型，vision 图片输入对其无效。
- api_key 明文存用户数据目录（与 aider/claude-code 一致），靠目录权限与不入库保护。
- 技能市场远程源拉取失败时静默降级为仅内置。
- 桌面端不进 CI，需在 win/mac 本地构建验证。
