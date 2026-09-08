<p align="center">
  <img src="web/public/icon.svg" alt="WiseCortex" width="96" height="96">
</p>

<h1 align="center">WiseCortex</h1>

<p align="center">A self-hosted AI agent. Your keys, your data, your machine.</p>

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

WiseCortex is an AI agent you run yourself. Coding is one of the things it does, not
the whole point: it also browses, searches, drives your desktop, runs on a schedule,
and answers you in your chat apps.

The backend is a single Rust binary. The frontend is framework-free TypeScript that
ships three ways from one codebase — a Tauri desktop app on Windows and macOS, a
browser WebUI on Linux, and a terminal client. All three speak the same local
HTTP/WebSocket API.

Bring your own API key. No accounts, no telemetry, no server of ours in the middle — the only
traffic leaving your machine is what you set in motion yourself (model calls, web tools, IM, the
skill marketplace), and the model calls themselves can point at a local endpoint.

## Positioning

Most open-source agents in this space are terminal coding assistants. WiseCortex starts
from a different premise: **the agent is a resident service, and you reach it from
wherever you are.**

|  | WiseCortex | Claude Code | opencode |
|---|:--:|:--:|:--:|
| Process model | resident service | session | session |
| Desktop GUI control | ● | ○ | ○ |
| Two-way IM <sup>1</sup> | ● | ○ | ○ |
| Scheduled / background autonomy | ● | ◐ <sup>2</sup> | ○ |
| Semantic code navigation (LSP) | ● | ○ | ● |
| Skills / MCP / hooks | ● | ● | ◐ <sup>3</sup> |
| Model sources | any | Anthropic <sup>4</sup> | any |
| Distribution | single binary | npm | npm |

● built in &nbsp;·&nbsp; ◐ partial or via add-on &nbsp;·&nbsp; ○ none

1. Two-way conversations over WeChat, Feishu, WeCom, and QQ. WeChat and Feishu need
   **no public IP**.
2. Claude Code has background tasks and cloud-scheduled agents, but no local resident scheduler.
3. opencode covers similar ground through plugins and `AGENTS.md` — a different mechanism.
4. Including Bedrock and Vertex passthrough.

> Compiled from each project's public documentation as of 2026-08. Corrections welcome
> via issue.

Two things the table cannot show: all three clients share the same sessions and history,
so any of them picks up where the last left off; and the shell tool holds a long-lived
session per task — `cd`, env vars, `venv`, and `PATH` survive across calls, so the agent
can build its own toolchain step by step.

## Features

**Conversation**
- Streaming replies with Markdown and syntax highlighting; interrupt any time.
- Image / vision input; thumbnails replay with history. Text-only models silently skip images.
- Extended thinking: low / medium / high, translated per provider (Anthropic `thinking`,
  OpenAI & Gemini `reasoning_effort`, Qwen & Hunyuan `enable_thinking`, none for models
  that reason internally).
- Cost tracking with prompt-cache accounting, and automatic context compression past a threshold
  (the compaction instruction is inserted into the live conversation, reusing the cached prefix).
- **Images are sent once**: screenshots are downscaled at capture and enter the context only on
  the turn they were sent. Re-sending them every turn is the single largest waste in a context
  window — a dozen GUI-automation screenshots is enough to blow up the request body. They stay
  visible in the chat history either way.

**Tools**
- Files: `read_file`, `write_file`, `edit_file`, `glob`, `grep`.
- Shell: persistent per-task session, timeouts, background processes.
- Git as a first-class citizen: read-only `git` (status/diff/log/show, no confirmation
  prompt so review stays fast) plus `git_commit`.
- Web: `web_fetch`, `web_search` (DuckDuckGo by default — no key — or SearXNG, Brave, Tavily).
- Semantic code navigation via LSP: rust-analyzer, typescript-language-server, pyright,
  gopls, clangd. Go-to-definition, find-references, hover types, document symbols,
  diagnostics — it understands symbols and types, unlike grep. Requires the server installed locally.
- Desktop control: `list_windows`, `capture_window`, `ui_tree`, `window_click`,
  `window_type`, `window_key`, `window_scroll` (Windows and macOS).
- `todo_write`, `notify`, `invoke_skill`.

**Extensibility**
- **Skills**: a `SKILL.md` is a skill. 21 ship built in, seeded on first launch — a
  design group ([ui-ux-pro-max](https://github.com/nextlevelbuilder/ui-ux-pro-max-skill) and
  ClaudeKit's `ckm:*` skills) and a
  workflow group ([superpowers](https://github.com/obra/superpowers): TDD, systematic debugging,
  brainstorming, writing plans, code review). All MIT, all removable. Browse and install more
  from a switchable registry, import from any Git repo, or migrate from openclaw in one click.
- **Self-improvement**: the agent notices gaps in its own skills and edits or writes new
  `SKILL.md` files as it works. `invoke_skill` re-reads from disk on every call, so an edited
  skill **takes effect on the next call — no restart**. Point its working directory at the
  WiseCortex repo and it can work on its own source too — the persistent shell builds, git is a
  first-class tool, rust-analyzer handles navigation — though source changes, unlike skills,
  need a process restart to take effect.
- **MCP client**: connect MCP servers over **stdio** or **Streamable HTTP**. Their tools,
  resources, and prompts all surface to the model as `mcp__<server>__<tool>`. WiseCortex also
  answers server-initiated **sampling** requests (running completions on your local LLM) and
  **roots** (stdio only). Configured as JSON, same shape as Claude Code's `mcpServers`;
  saving reconnects.
- **Hooks**: run commands on `PreToolUse` / `PostToolUse` / `UserPromptSubmit` /
  `SessionStart` / `Stop`. Block dangerous tool calls, append notes after writes, notify on
  turn end. The command reads the event as JSON on stdin and decides via
  `{"decision":"block"}` or its exit code.

**Working with it**
- **Tasks**: the sidebar keeps two lists — loose *tasks* and *workspaces* grouped by working
  directory. Starting a new task never interrupts a running one; live status shows in the
  sidebar. Each task carries its own model, pinned skills, approval mode, and working directory.
- **Plan mode**: the agent explores read-only and produces a step-by-step plan while writes,
  shell, MCP, and background tasks are blocked. Review it, turn plan mode off, then say go.
- **Subagents and background tasks**: `task` spawns a synchronous subagent (parallel within a
  turn); `task_start` launches an autonomous background task you poll with
  `task_list` / `task_result` / `task_stop`.
- **Knowledge base**: mount directories or files per session; the agent searches them with
  `knowledge_search`. Local keyword search — no embeddings, no external service.
- **Memory**: two layers — **session** and **project**, the latter scoped to a working directory
  and carried into new conversations under it. The agent writes decisions, constraints, and
  progress itself with `remember` as it works, rather than you maintaining an `AGENTS.md`.
  Re-injected every turn — surviving context compression and session reopen. Optional automatic
  extraction (off by default, costs extra LLM calls).

**Reaching you**
- **Outbound channels**: Feishu, WeCom, QQ (OneBot), email (SMTP), generic webhook.
- **Scheduled tasks**: run a prompt on an interval, push the result to a channel, keep logs.
  Manage from the CLI or the web panel.
- **Two-way IM**: see [Talk to it from IM](#talk-to-it-from-im).

**Access control**
- Optional `access_key` guarding WS and REST. Binding to a non-loopback address without one
  is refused outright, so you cannot accidentally expose it.
- Dangerous operations can require confirmation before they run.

## Install

### Prerequisites (all platforms)

- **Rust** via [rustup](https://rustup.rs) — the exact version is pinned in `rust-toolchain.toml`; rustup switches to it automatically
- **Node.js ≥ 20** (22 LTS recommended) and npm
- **Git**

Check with `cargo --version`, `node --version`, `git --version`.

TLS goes through rustls throughout, so there is **no** OpenSSL / `libssl-dev` dependency.
Building needs only a C linker.

### Get the source

```bash
git clone https://github.com/terry16/wisecortex.git
cd wisecortex
```

### Quick start — any OS

```bash
# 1. build and start the backend (listens on 127.0.0.1:7070)
cargo run -p wisecortex-server

# 2. in another terminal, start the frontend
cd web
npm install
npm run dev            # http://localhost:5173
```

Open the frontend URL. This is also the recommended setup on Linux.

Windows has a helper that does the whole thing:

```bat
scripts\build-windows.bat fast      all | fast | backend | web | desktop
```

### Windows — desktop installer

1. Install **Visual Studio C++ Build Tools** (the "Desktop development with C++" workload).
2. Make sure the **WebView2 runtime** is present. Most Win10/11 machines have it; otherwise
   install the Evergreen runtime from Microsoft.
3. Build:

   ```powershell
   cargo install tauri-cli --version "^2.0" --locked
   cd web ; npm install ; npm run build ; cd ..
   cargo tauri build
   ```

   Or just `scripts\build-windows.bat desktop`.

4. Installers land in `target/release/bundle/`:
   - `msi/WiseCortex_<ver>_x64_en-US.msi`
   - `nsis/WiseCortex_<ver>_x64-setup.exe`

The desktop app embeds the backend — no separate server process, no nginx, nothing to configure.

### macOS — desktop app

1. `xcode-select --install`
2. Build:

   ```bash
   cargo install tauri-cli --version "^2.0" --locked
   cd web && npm install && npm run build && cd ..
   cargo tauri build
   ```

3. Output in `target/release/bundle/` (`.dmg` / `.app`).

Unsigned builds: right-click → Open the first time, or allow it under
System Settings → Privacy & Security.

### Linux / server — WebUI

No desktop package on Linux; run the backend plus the static frontend. Steps below are for
**Ubuntu 22.04 / 24.04** and can be pasted as-is.

**1. System dependencies**

```bash
sudo apt update
sudo apt install -y build-essential pkg-config curl git
```

**2. Rust**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
cargo --version
```

**3. Node.js ≥ 20**

```bash
curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
sudo apt install -y nodejs
node --version && npm --version
```

**4. Build**

```bash
git clone https://github.com/terry16/wisecortex.git
cd wisecortex

cargo build --release                          # -> target/release/{wisecortex-server,wisecortex}
cd web && npm install && npm run build && cd .. # -> web/dist
```

**5. Configure a model** (otherwise there is nothing to talk to)

```bash
./target/release/wisecortex config set --provider deepseek --api-key <YOUR_KEY> --model deepseek-v4-pro
```

**6. Try it**

```bash
./target/release/wisecortex-server        # 127.0.0.1:7070
cd web && npx vite preview --port 5173  # or any static server for web/dist
```

**7. Run it as a service**

Create `/etc/systemd/system/wisecortex.service` (adjust `User`, paths):

```ini
[Unit]
Description=WiseCortex server
After=network.target

[Service]
Type=simple
User=ubuntu
WorkingDirectory=/home/ubuntu/wisecortex
ExecStart=/home/ubuntu/wisecortex/target/release/wisecortex-server
# Required when exposed to the internet; optional for loopback / behind nginx
# Environment=WC_ACCESS_KEY=change-me
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

**8. Exposing it — nginx**

The frontend is plain static files and talks to the backend **same-origin**: REST on `/api/*`,
WebSocket on `/ws`. So nginx does exactly two things — serve `web/dist` and proxy those two
paths. The backend stays on `127.0.0.1:7070` and never needs a public port.

Set an `access_key` first (settings panel or `WC_ACCESS_KEY`), then:

```nginx
server {
    listen 80;                                  # use 443 ssl if you have a certificate
    server_name wisecortex.example.com;

    root /srv/wisecortex/web/dist;                # your web/dist absolute path
    index index.html;

    location / {
        try_files $uri $uri/ /index.html;       # SPA fallback
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

Browse to nginx's port, not 7070. Only 80/443 needs to be open in the firewall.

> Over plain HTTP the page is not a secure context, so `navigator.clipboard` and
> `crypto.randomUUID` are unavailable. WiseCortex falls back gracefully, but TLS is worth setting up.

Quick self-test without nginx — expose the backend directly:

```bash
WC_ACCESS_KEY=your-key WC_BIND=0.0.0.0:7070 ./target/release/wisecortex-server
```

Binding to a non-loopback address without an access key is refused, by design.

## Configure a model

```bash
# a preset provider
wisecortex config set --provider deepseek --api-key <KEY> --model deepseek-v4-pro

# any OpenAI-compatible endpoint (vLLM, Ollama, LM Studio, a gateway...)
# --base-url is mandatory here
wisecortex config set --provider openai-compatible \
  --base-url http://localhost:8000/v1 --api-key <KEY> --model <MODEL>

wisecortex config show      # never echoes the key
```

Presets: `openai`, `anthropic`, `deepseek`, `qwen`, `hunyuan`, `gemini`. You can also do all
of this in the web **⚙ Settings** panel, which hot-reloads without a restart.

- **Adding models without recompiling**: drop a `providers.json` into the config directory to
  override built-in presets (matching `id` overrides fields; a `models` array replaces that
  provider's model list; a new `id` adds a provider). Template:
  [`docs/providers.example.json`](docs/providers.example.json).
- **Resolution order**: `WC_*` environment variables > config file > built-in defaults.
  Common ones: `WC_PROVIDER`, `WC_API_KEY`, `WC_MODEL`, `WC_BASE_URL`, `WC_ACCESS_KEY`,
  `WC_BIND`, `WC_PROXY`.
- **Proxy**: set one address and *all* outbound traffic uses it — LLM calls, skill registry,
  web tools, IM, git imports. `http(s)://` or `socks5://`.
- **Working directory**: a global one (AI scripts, knowledge base, self-learning) plus an
  optional per-conversation override. Defaults to `%APPDATA%\wisecortex\workspace` (Windows),
  `~/Library/Application Support/wisecortex/workspace` (macOS), `~/wisecortex/workspace` (Linux).

### Subscription login (optional)

Besides API keys, WiseCortex can authenticate against provider **subscription** plans over
OAuth, so inference draws on a plan you already pay for instead of per-token credits. Log in
from **⚙ Settings** — the browser opens, you paste the returned code back, and the models
become selectable. No API key involved.

| Provider | Plan | Flow | State |
|---|---|---|---|
| ChatGPT | Plus / Pro | Codex OAuth | implemented, lightly tested |
| Google | Gemini Code Assist | Google account | implemented, lightly tested |
| xAI | Grok | device code | implemented, untested |
| Anthropic | Claude Pro / Max | OAuth | works — but read the note |

> **About Claude Pro/Max.** Anthropic's terms prohibit driving a subscription plan from
> third-party tooling. OpenCode shipped this through bundled plugins and
> [removed them in 1.3.0](https://opencode.ai/docs/providers) for that exact reason, while
> continuing to support ChatGPT Plus, GitHub Copilot, and GitLab Duo — vendors that permit it.
> The code is here because it is useful for personal, single-machine use, but you are the one
> accepting that risk, and it is not something to roll out to a team. The other three providers
> do not carry this restriction.

> Config, sessions, and skills live under `wisecortex/` in your user data directory
> (`%APPDATA%` / `~/Library/Application Support` / `~/.local/share`). **Credentials are stored
> in plaintext**, protected by directory permissions — same as aider and Claude Code. Keep that
> directory out of backups you share.

## Talk to it from IM

Two categories: **no public IP required** (WeChat, Feishu) and **callback-based** (needs a
public HTTPS endpoint).

**WeChat ClawBot** — the easiest. Channels page → **Connect by QR** → scan with your phone.
No credentials to type, no restart. This is Tencent's official personal-account bot API
(protocol: iLink, opened March 2026 via OpenClaw). It is HTTPS long-polling, which is why it
works from a desktop behind NAT. Text and voice (server-side transcription) are supported;
images and files are not yet.

> One WeChat account can create exactly one bot, and the update cursor is shared per bot.
> **Run the polling in one place only** — two machines polling at once will split your
> messages randomly between them.

**Feishu** — Channels page → **Connect by QR** creates the app and fills in
`app_id`/`app_secret`, then flip the **long connection** switch and restart the backend. The
bot dials out over wss, so no public callback is needed. You still have to finish three things
in the Feishu developer console: add the `im:message` permission, choose **long connection**
for event subscription and subscribe to message receipt, and publish a version.

**With a public endpoint**, expose the backend and point callbacks at:

- Feishu event subscription: `https://<host>/api/im/feishu`
- WeCom: `https://<host>/api/im/wecom` (WeCom has no long-connection mode; HTTPS required)
- QQ via OneBot/NapCat: `https://<host>/api/im/onebot`

`/api/im/*` skips the access key — it is protected by platform signatures and your network layer.

Once connected you get the full agent in the chat: multi-turn context, `/sessions` to list
tasks, `/switch <n>` to move between them, `/reset`, `/model`, `/workdir`.

## CLI and terminal client

```bash
wisecortex tui                       # terminal client: streaming chat, switch sessions
wisecortex tui --serve               # also start the backend if it is not running

wisecortex skill list
wisecortex skill import <dir>
wisecortex skill add-git https://github.com/obra/superpowers

wisecortex channel add --name team --kind feishu --url <webhook>
wisecortex cron add --name daily --interval 1h --prompt "check the build" --channel team
wisecortex cron logs <id>

wisecortex im feishu --app-id .. --app-secret .. --verify-token ..
wisecortex doctor                    # dump the error log (panics, failed tasks)
```

From a source checkout, prefix with `cargo run -p wisecortex-cli --`.

## Architecture

| Path | What it is |
|------|------------|
| `crates/core` | Agent core: LLM abstraction, tools, skills and registry, config, cron, notifications, IM clients |
| `crates/server` | axum service: WebSocket + REST, agent execution, scheduler, IM connectors |
| `crates/cli` | `wisecortex` command line and the terminal client |
| `desktop/src-tauri` | Tauri 2 shell (macOS / Windows only) — embeds the server in-process |
| `web` | Framework-free TypeScript + Vite frontend |

The desktop shell runs the same `wisecortex_server::run()` as the standalone binary, which is why
every server-side feature — including the IM connectors — works identically on Windows, macOS,
and Linux.

The WebSocket contract is documented in [`docs/protocols/`](docs/protocols/).

## Development

```bash
# Rust
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test

# Frontend
cd web
npx biome check .
npx tsc --noEmit
npx vitest run
```

CI runs exactly these. The layered test plan, including manual integration steps, is in
[`docs/test-plan.md`](docs/test-plan.md).

Contributions are welcome — issues and pull requests both.

## License

[MIT](LICENSE).

Bundled skills come from third parties and keep their own MIT licenses —
[superpowers](https://github.com/obra/superpowers) (Jesse Vincent),
[ui-ux-pro-max](https://github.com/nextlevelbuilder/ui-ux-pro-max-skill) (Next Level Builder),
and ClaudeKit (the `ckm:*` design skills). Full attribution is in [NOTICE.md](NOTICE.md).
Thanks to all of them.
