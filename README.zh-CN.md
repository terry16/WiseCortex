<p align="center">
  <img src="web/public/icon.svg" alt="WiseCortex" width="96" height="96">
</p>

<h1 align="center">WiseCortex</h1>

<p align="center">自托管 AI Agent：密钥、数据、机器，都归你自己。</p>

<p align="center">
  <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-MIT-blue?style=flat-square"></a>
  <a href="https://github.com/terry16/wisecortex/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/terry16/wisecortex/ci.yml?style=flat-square"></a>
  <img alt="Rust" src="https://img.shields.io/badge/rust-stable-orange?style=flat-square">
  <img alt="Platforms" src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey?style=flat-square">
</p>

<p align="center">
  <a href="README.md">English</a> |
  <a href="README.zh-CN.md">简体中文</a>
</p>

---

WiseCortex 是一个自托管的通用 AI Agent。编码只是其能力之一：调研检索、数据处理、桌面操作、
定时执行，以及在微信和飞书里直接应答，共用同一套工具带与技能系统。

后端是单个 Rust 二进制。前端为无框架 TypeScript，一份代码产出三种客户端——Windows / macOS
上的 Tauri 桌面应用、Linux 上的浏览器 WebUI、以及终端客户端，三者共用同一套本地
HTTP / WebSocket 接口。

模型侧自带密钥（BYOK）。没有账号、没有遥测、没有本项目的服务端——出站流量只有你显式发起的
那些（模型调用、web 工具、IM、技能市场等），而模型调用本身还可以指向本地端点。

## 定位

同类开源项目多为终端里的编程助手。WiseCortex 的前提不同：**Agent 是一个常驻服务，
接入方式随人走。**

|  | WiseCortex | Claude Code | opencode |
|---|:--:|:--:|:--:|
| 进程形态 | 常驻服务 | 会话式 | 会话式 |
| 桌面 GUI 控制 | ● | ○ | ○ |
| IM 双向接入 <sup>1</sup> | ● | ○ | ○ |
| 定时 / 后台自治任务 | ● | ◐ <sup>2</sup> | ○ |
| 语义代码导航（LSP） | ● | ○ | ● |
| 技能 / MCP / 事件钩子 | ● | ● | ◐ <sup>3</sup> |
| 模型来源 | 任意 | Anthropic <sup>4</sup> | 任意 |
| 分发形态 | 单二进制 | npm | npm |

● 内置　◐ 部分或需外挂　○ 无

1. 指微信、飞书、企业微信、QQ 的双向会话。其中微信与飞书**不需要公网地址**。
2. Claude Code 具备后台任务与云端定时 Agent，本地侧无常驻调度器。
3. opencode 以 plugin 与 `AGENTS.md` 覆盖同类需求，机制不同。
4. 含 Bedrock 与 Vertex 转发。

> 对照依据各项目 2026-08 的公开文档。如有出入，欢迎提 issue 指正。

表格之外还有两点：三种客户端共享同一批会话与历史，打开哪个都接得上；shell 工具按任务维持
长生命周期会话，`cd`、环境变量、`venv`、`PATH` 跨调用保留，Agent 因此能一步步给自己
搭出开发环境。

## 功能

**对话**
- 流式增量渲染，Markdown 与代码高亮，随时可中断。
- 图片 / Vision 输入；历史回放时缩略图一并重现，纯文本模型自动忽略图片。
- **推理强度** low / medium / high，按提供商自动翻译：Anthropic 走 `thinking`，
  OpenAI 与 Gemini 走 `reasoning_effort`，千问与混元走 `enable_thinking`，
  DeepSeek 这类思考内置的模型不下发参数。
- 成本统计计入缓存命中；上下文超阈值自动压缩（压缩指令插入当前对话，复用已缓存前缀）。
- **图片只发一次**：截图即缩放，且只在发出的那一轮进入上下文——默认的每轮重发是上下文里
  最大的一笔浪费，GUI 自动化连截十几张就能把请求体撑爆。聊天记录里仍照常可见。

**工具**
- 文件：`read_file` / `write_file` / `edit_file` / `glob` / `grep`。
- Shell：按任务维持的持久会话，带超时与后台进程。
- **git 一等公民**：只读 `git`（status / diff / log / show，免确认，以免拖慢审查迭代）
  与 `git_commit`；其余写操作走 shell。
- 联网：`web_fetch`、`web_search`（默认 DuckDuckGo，免 key；可切 SearXNG / Brave / Tavily）。
- **语义代码导航（LSP）**：对接 rust-analyzer / typescript-language-server / pyright /
  gopls / clangd，提供定义跳转、引用查找、悬停类型、文档符号与诊断。基于符号与类型解析，
  精度远高于文本匹配。需本机预装对应语言服务器。
- **桌面控制**：`list_windows`、`capture_window`、`ui_tree`、`window_click`、
  `window_type`、`window_key`、`window_scroll`（Windows 与 macOS）。
- `todo_write`、`notify`、`invoke_skill`。

**扩展机制**
- **技能**：一个 `SKILL.md` 即一个技能。内置 21 个，首次启动播种——一组设计技能
  （[ui-ux-pro-max](https://github.com/nextlevelbuilder/ui-ux-pro-max-skill) 与 ClaudeKit 的
  `ckm:*` 系列）与一组工作流技能
  （[superpowers](https://github.com/obra/superpowers)：TDD、系统化调试、头脑风暴、写计划、
  代码评审等），均为 MIT，可随时卸载。市场支持下拉切换源检索安装，亦可从任意 Git 仓库导入，
  或从 openclaw 一键迁移。
- **自我进化**：Agent 在使用过程中发现自身技能的缺漏，直接改写或新建 `SKILL.md`。
  `invoke_skill` 每次调用都从盘上重读，改过的技能**下次调用即生效，不必重启**。
  把工作目录指向 WiseCortex 仓库，它同样能改自己的源码——持久 shell 负责编译、git 一等公民、
  rust-analyzer 提供语义导航——只是源码这类改动要重启进程才生效，技能不用。
- **MCP 客户端**：以 **stdio** 子进程或 **Streamable HTTP** 连接 MCP 服务器，其
  **tools / resources / prompts** 一律以 `mcp__<服务器>__<工具>` 暴露给模型。同时响应服务端
  发起的 **sampling**（用本机 LLM 跑补全）与 **roots**（仅 stdio）。设置页以 JSON 配置，
  格式对齐 Claude Code 的 `mcpServers`，保存即重连。
- **事件钩子**：在 `PreToolUse` / `PostToolUse` / `UserPromptSubmit` / `SessionStart` /
  `Stop` 上挂载命令，用于护栏与副作用——拦截危险工具调用、写文件后追加提示、回合结束发通知。
  命令自 stdin 接收事件 JSON，以 `{"decision":"block"}` 或退出码决定放行还是拦截。

**任务与执行**
- **任务**：左栏常驻两段列表——未指定工作目录的归入「任务」，指定了的按目录叶名归入
  「工作空间」。**新建任务不打断在跑的任务**，运行态在侧栏实时显示。每个任务携带各自的模型、
  **钉选技能**（只启用选中的若干个，避免技能过多导致自动选择失准）、审批模式与工作目录。
- **计划模式**：Agent 先只读探索并产出可执行的分步计划，期间写文件、shell、MCP、后台任务
  全部拦截。审核通过后关闭计划模式再下达执行。
- **子 Agent 与后台任务**：`task` 派生同步子 Agent（单回合内可并行）；`task_start` 启动
  自治的后台长任务，以 `task_list` / `task_result` / `task_stop` 查询与停止。
- **知识库**：按会话挂载目录或文件，Agent 经 `knowledge_search` 检索后作答。纯本地关键词检索，
  不依赖向量库与外部服务。
- **记忆**：分**会话级**与**项目级**两层，项目级按工作目录共享，新开对话也带得走。
  由 Agent 在干活过程中以 `remember` 自行写入决定、约束与进度，不是手工维护的 `AGENTS.md`；
  每轮持续注入，**跨上下文压缩与会话重开依然保留**。可选自动抽取（默认关闭，会增加 LLM 调用）。

**主动触达**
- **出站通道**：飞书、企业微信、QQ（OneBot）、邮件（SMTP）、通用 webhook。
- **定时任务**：按间隔执行 prompt，结果推送至通道，附执行日志，CLI 与网页面板均可管理。
- **双向 IM**：见[在 IM 里直接使用](#在-im-里直接使用)。

**访问控制**
- 可设 `access_key` 保护 WS 与 REST。**绑定非回环地址却未设密钥将直接拒绝启动**，
  避免误将服务暴露在公网。
- 危险操作可要求执行前确认。

## 安装

### 通用前置依赖

- **Rust**，经 [rustup](https://rustup.rs) 安装（具体版本锁在 `rust-toolchain.toml`，rustup 自动切换，无需手动指定）
- **Node.js ≥ 20**（建议 22 LTS）与 npm
- **Git**

以 `cargo --version` / `node --version` / `git --version` 验证。

本项目 TLS 全栈基于 rustls，**不依赖** OpenSSL / `libssl-dev`，编译仅需一个 C 链接器。

### 获取源码

```bash
git clone https://github.com/terry16/wisecortex.git
cd wisecortex
```

### 快速开始 — 任意系统

```bash
# 1. 编译并启动后端（监听 127.0.0.1:7070）
cargo run -p wisecortex-server

# 2. 另开终端启动前端
cd web
npm install
npm run dev            # http://localhost:5173
```

浏览器打开该地址即可。Linux 建议直接使用这一方式。

Windows 另有一键脚本：

```bat
scripts\build-windows.bat fast      目标可选 all | fast | backend | web | desktop
```

### Windows — 桌面安装包

1. 安装 **Visual Studio C++ 生成工具**（勾选「使用 C++ 的桌面开发」工作负载）。
2. 确认 **WebView2 运行时**存在（Win10/11 多数自带；缺失则从微软官网安装 Evergreen 运行时）。
3. 打包：

   ```powershell
   cargo install tauri-cli --version "^2.0" --locked
   cd web ; npm install ; npm run build ; cd ..
   cargo tauri build
   ```

   或直接执行 `scripts\build-windows.bat desktop`。

4. 产物位于 `target/release/bundle/`：
   - `msi/WiseCortex_<版本>_x64_en-US.msi`
   - `nsis/WiseCortex_<版本>_x64-setup.exe`

桌面端在进程内内嵌后端：无需单独启动服务进程，无需 nginx，无需额外配置。

### macOS — 桌面应用

1. `xcode-select --install`
2. 打包：

   ```bash
   cargo install tauri-cli --version "^2.0" --locked
   cd web && npm install && npm run build && cd ..
   cargo tauri build
   ```

3. 产物位于 `target/release/bundle/`（`.dmg` / `.app`）。

未签名包首次打开需右键「打开」，或在「系统设置 → 隐私与安全性」中放行。

### Linux / 服务器 — WebUI

Linux 不提供桌面包，部署形态为「后端 + 静态前端」。以下以 **Ubuntu 22.04 / 24.04** 为例，
自干净系统至生产部署，命令可直接复制。

**1. 系统依赖**

```bash
sudo apt update
sudo apt install -y build-essential pkg-config curl git
```

**2. 安装 Rust**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
cargo --version
```

**3. 安装 Node.js ≥ 20**

```bash
curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
sudo apt install -y nodejs
node --version && npm --version
```

**4. 编译**

```bash
git clone https://github.com/terry16/wisecortex.git
cd wisecortex

cargo build --release                          # 产出 target/release/{wisecortex-server,wisecortex}
cd web && npm install && npm run build && cd .. # 产出 web/dist
```

**5. 配置模型**（未配置时无法发起对话）

```bash
./target/release/wisecortex config set --provider deepseek --api-key <你的KEY> --model deepseek-v4-pro
```

**6. 试运行**

```bash
./target/release/wisecortex-server        # 127.0.0.1:7070
cd web && npx vite preview --port 5173  # web/dist 为纯静态，任意静态服务器均可
```

**7. 注册为常驻服务**

新建 `/etc/systemd/system/wisecortex.service`，按实际情况调整 `User` 与路径：

```ini
[Unit]
Description=WiseCortex server
After=network.target

[Service]
Type=simple
User=ubuntu
WorkingDirectory=/home/ubuntu/wisecortex
ExecStart=/home/ubuntu/wisecortex/target/release/wisecortex-server
# 对公网暴露时必填；仅本机或经 nginx 反代可省略
# Environment=WC_ACCESS_KEY=改成你的密钥
Restart=on-failure
RestartSec=3

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now wisecortex
journalctl -u wisecortex -f
```

**8. 对外访问 — nginx**

前端为纯静态文件，在浏览器中**全部同源访问**：REST 走 `/api/*`，WebSocket 走 `/ws`。
因此 nginx 只需做两件事——托管 `web/dist`，并将这两个路径反代至内网后端。后端继续绑定
`127.0.0.1:7070`，**无需对外开放端口**。

先设置 `access_key`（设置面板或 `WC_ACCESS_KEY`），随后：

```nginx
server {
    listen 80;                                  # 具备证书时改用 443 ssl
    server_name wisecortex.example.com;

    root /srv/wisecortex/web/dist;                # 替换为 web/dist 的绝对路径
    index index.html;

    location / {
        try_files $uri $uri/ /index.html;       # SPA 回退
    }
    location /api {
        proxy_pass http://127.0.0.1:7070;
        proxy_set_header Host $host;
    }
    location /ws {
        proxy_pass http://127.0.0.1:7070;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_read_timeout 1d;
    }
}
```

访问 nginx 监听的端口，**而非后端的 7070**。防火墙仅需放行 80（或 443）。

> 纯 HTTP 下页面不构成安全上下文，`navigator.clipboard` 与 `crypto.randomUUID` 均不可用。
> WiseCortex 内部有降级兜底，仍建议配置 TLS。

若暂不配置 nginx，可直接暴露后端做快速自测：

```bash
WC_ACCESS_KEY=你的密钥 WC_BIND=0.0.0.0:7070 ./target/release/wisecortex-server
```

绑定非回环地址却未设密钥会被拒绝启动，此为有意设计。

## 配置模型

```bash
# 预设提供商
wisecortex config set --provider deepseek --api-key <KEY> --model deepseek-v4-pro

# 任意 OpenAI 兼容端点（vLLM、Ollama、LM Studio、第三方网关等）
# 此形态下 --base-url 必填
wisecortex config set --provider openai-compatible \
  --base-url http://localhost:8000/v1 --api-key <KEY> --model <模型名>

wisecortex config show      # 不回显明文 key
```

预设提供商：`openai` / `anthropic` / `deepseek` / `qwen` / `hunyuan` / `gemini`。
以上均可在网页 **⚙ 设置** 面板调整，保存即热重载，无需重启。

- **新增模型无需重新编译**：在配置目录放置 `providers.json` 覆盖内置预设——`id` 相同则覆盖
  字段（给出 `models` 数组即整表替换该提供商的可选模型），`id` 为新值则新增提供商。
  模板见 [`docs/providers.example.json`](docs/providers.example.json)。
- **解析优先级**：环境变量 `WC_*` > 配置文件 > 内置默认。常用项包括 `WC_PROVIDER`、
  `WC_API_KEY`、`WC_MODEL`、`WC_BASE_URL`、`WC_ACCESS_KEY`、`WC_BIND`、`WC_PROXY`。
- **网络代理**：配置单一地址后，**全部出站访问**均经此代理——LLM、技能市场、web 工具、
  IM、git 导入。支持 `http(s)://` 与 `socks5://`。
- **工作目录**：分两层——全局工作目录（AI 脚本、知识库、自我学习的落点），外加可选的
  「当前对话工作目录」。默认位置：`%APPDATA%\wisecortex\workspace`（Windows）/
  `~/Library/Application Support/wisecortex/workspace`（macOS）/ `~/wisecortex/workspace`（Linux）。

### 订阅登录（可选）

除 API Key 外，WiseCortex 支持以 OAuth 登录各家的**订阅套餐**，令推理消耗既有订阅额度而非
按 token 计费。在 **⚙ 设置** 中登录：浏览器打开授权页，将返回的 code 粘回，模型即可选，
全程无需 API Key。

| 提供商 | 套餐 | 方式 | 状态 |
|---|---|---|---|
| ChatGPT | Plus / Pro | Codex OAuth | 已实现，验证有限 |
| Google | Gemini Code Assist | Google 账号 | 已实现，验证有限 |
| xAI | Grok | 设备码流 | 已实现，未实测 |
| Anthropic | Claude Pro / Max | OAuth | 可用，但见下方说明 |

> **关于 Claude Pro / Max。** Anthropic 的条款禁止以第三方工具驱动订阅套餐。OpenCode 曾以
> 内置插件形式提供该能力，并在 [1.3.0 中移除](https://opencode.ai/docs/providers)，理由正是
> 这一条；同时它继续支持 ChatGPT Plus、GitHub Copilot、GitLab Duo——这几家是允许的。
> 此处保留相关代码，是因为个人单机自用确有便利，但风险由使用者自行承担，
> **不适合团队部署**。另外三家无此限制。

> 配置、会话与技能存放于用户数据目录下的 `wisecortex/`（`%APPDATA%` / `~/Library/Application
> Support` / `~/.local/share`）。**凭据为明文存储**，依赖目录权限保护，与 aider、Claude Code
> 一致。请勿将该目录纳入对外同步的备份。

## 在 IM 里直接使用

分两类：**免公网**（微信、飞书）与**回调式**（需公网 HTTPS）。

**微信 ClawBot** —— 接入成本最低。通道页点击**「扫码接入」**，手机微信扫码确认即可，
无需填写凭据，无需重启。这是腾讯 2026-03 经 OpenClaw 开放的**个人号** Bot API（协议 iLink），
走 HTTPS 长轮询，因此 NAT 后的桌面端同样可用。支持文字与语音（服务端转写），
图片与文件暂不支持。

> ⚠️ 一个微信号只能创建一个 Bot，且同步游标按 Bot 共享。**同一时刻只能在一处开启轮询**，
> 两台机器同时开启会将消息随机分走一半。

**飞书** —— 通道页点击**「扫码连接」**自动建应用并回填 `app_id` / `app_secret`，
随后打开**「长连接」**开关并重启后端。机器人主动外拨 wss，不需要公网回调。
飞书开发者后台还需完成三项：① 权限添加 `im:message`；② 事件订阅选择**长连接**并订阅
「接收消息」；③ **发布版本**。

**具备公网地址时**，将后端暴露出去，回调地址分别填写：

- 飞书事件订阅：`https://<域名>/api/im/feishu`
- 企业微信：`https://<域名>/api/im/wecom`（企业微信**无长连接模式**，必须 HTTPS）
- QQ（OneBot / NapCat）：`https://<域名>/api/im/onebot`

`/api/im/*` 免 access_key，由平台签名与网络层保护。

接入后，聊天窗口内即为完整的 Agent：多轮上下文、`/sessions` 列任务、`/switch <n>` 切换、
`/reset`、`/model`、`/workdir`。

## 命令行与终端客户端

```bash
wisecortex tui                       # 终端客户端：流式对话、切换会话
wisecortex tui --serve               # 目标端口未在运行时顺带拉起后端

wisecortex skill list
wisecortex skill import <本地技能目录>
wisecortex skill add-git https://github.com/obra/superpowers

wisecortex channel add --name team --kind feishu --url <webhook>
wisecortex cron add --name daily --interval 1h --prompt "巡检构建状态" --channel team
wisecortex cron logs <任务ID>

wisecortex im feishu --app-id .. --app-secret .. --verify-token ..
wisecortex doctor                    # 导出错误日志（panic、任务失败等）
```

源码目录下以 `cargo run -p wisecortex-cli --` 加同样参数调用。

## 架构

| 目录 | 说明 |
|------|------|
| `crates/core` | Agent 核心：LLM 抽象、工具、技能与市场、配置、cron、通知、IM 客户端 |
| `crates/server` | axum 服务：WebSocket + REST、Agent 运行、定时调度、IM 连接器 |
| `crates/cli` | `wisecortex` 命令行与终端客户端 |
| `desktop/src-tauri` | Tauri 2 桌面壳（仅 macOS / Windows），**进程内内嵌后端** |
| `web` | 无框架 TypeScript + Vite 前端 |

桌面壳运行的是与独立二进制**同一个** `wisecortex_server::run()`，这是全部服务端能力
（含各 IM 连接器）在 Windows、macOS、Linux 上表现一致的原因。

WebSocket 协议契约见 [`docs/protocols/`](docs/protocols/)。

## 开发与测试

```bash
# Rust
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test

# 前端
cd web
npx biome check .
npx tsc --noEmit
npx vitest run
```

CI 执行的即是以上命令。分层测试流程（含手工集成验收）见
[`docs/test-plan.md`](docs/test-plan.md)。

欢迎提交 issue 与 PR。

## 许可证

[MIT](LICENSE)

内置技能来自第三方，各自沿用其 MIT 许可证：
[superpowers](https://github.com/obra/superpowers)（Jesse Vincent）、
[ui-ux-pro-max](https://github.com/nextlevelbuilder/ui-ux-pro-max-skill)（Next Level Builder）、
以及 ClaudeKit（`ckm:*` 系列设计技能）。完整归属见 [NOTICE.md](NOTICE.md)，在此一并致谢。
