// English — canonical base locale. `MessageKey` is derived from these keys;
// every other locale must define exactly this set (enforced at compile time via
// `satisfies Record<MessageKey, string>` and at runtime by i18n.test.ts).
export const en = {
  // generic
  "common.done": "Done",
  "common.add": "Add",
  "common.remove": "Remove",
  "common.itemCount": "{n} items",

  // nav / shell
  "brand.sub": "Self-hosted AI agent",
  "nav.newTask": "New task",
  "nav.chat": "Chat",
  "nav.tasks": "Scheduled",
  "nav.jobs": "Background",
  "nav.skills": "Skills",
  "nav.channels": "Channels",
  "nav.settings": "Settings",

  // theme
  "theme.toggle": "Toggle theme (light / dark)",

  // footer (rail)
  "foot.modelLabel": "Current model",
  "foot.costLabel": "Cost today",
  "foot.model.unset": "Not configured",
  "foot.access.locked": "access_key locked",
  "foot.access.public": "Public access",
  "foot.versionTitle": "Server version",

  // offline
  "offline.banner": "Connection lost — reconnecting…",

  // login gate
  "login.sub": "This instance is protected. Enter the access key to continue.",
  "login.placeholder": "Access key",
  "login.enter": "Enter",
  "login.err.wrong": "Wrong key, please try again",
  "login.err.unreachable": "Cannot reach the server, please retry later",
  "auth.prompt": "This WiseCortex requires an access key, please enter it:",

  // attachments
  "attach.removeHint": "Click to remove",

  // composer
  "composer.placeholder": "Assign a task to WiseCortex…  (Enter to send, Shift+Enter for newline)",
  "composer.more": "Expand actions",
  "composer.stop": "Stop",
  "composer.cwd.label": "Working directory",
  "composer.cwd.defaultDir": "Default directory",
  "composer.cwd.task": "Task working directory: {dir}",
  "composer.cwd.locked": " (task started, locked)",
  "composer.cwd.global": "Working directory (global): {dir}",
  "composer.cwd.pickPrompt": "Task working directory (empty = use the global workspace):",
  "composer.model.title": "Model for this task (defaults to global)",
  "composer.effort.title": "Thinking effort for this chat (ignored if the model lacks it)",
  "composer.effort.opt.default": "Thinking: default",
  "composer.effort.opt.off": "Thinking: off",
  "composer.effort.opt.low": "Thinking: low",
  "composer.effort.opt.medium": "Thinking: medium",
  "composer.effort.opt.high": "Thinking: high",
  "composer.effort.opt.xhigh": "Thinking: xhigh",
  "composer.effort.opt.max": "Thinking: max",
  "composer.skills.pinned": "Skills for this task ({n}): {list}",
  "composer.skills.empty": "Skills for this task (auto-selected by default, click to pin)",
  "composer.perm.title": "Tool action permission",
  "composer.perm.default": "Default permission",
  "composer.perm.solo": "solo (auto-approve)",
  "composer.perm.strict": "Strict (confirm each step)",
  "composer.plan.on":
    "Plan mode: ON (read-only exploration + a plan, no edits). After approval, click here to turn it off, then say “execute”.",
  "composer.plan.off": "Plan mode: read-only exploration + a plan, no edits. Click to enable.",
  "composer.kb.pinned": "Knowledge base for this chat ({n}):\n{list}",
  "composer.kb.empty": "Knowledge base (this chat — click to add a folder/file)",
  "composer.mem.title": "Memory (view / edit what the AI remembers)",

  // skills picker
  "skillsPicker.title": "Skills for this task",
  "skillsPicker.note":
    "Pin 0 = the AI auto-selects from all skills; pin some = this task uses only those skills (more stable, no wrong picks).",
  "skillsPicker.empty": "No skills available.",
  "skillsPicker.projectBadge": "Project",
  "skillsPicker.projectBadge.title": "From this task's working directory skills/",

  // knowledge base
  "kb.title": "Knowledge base (this chat)",
  "mem.title": "Memory",
  "mem.note":
    "The AI records these itself with the remember tool, and they are injected on every turn. Here you can see what it actually remembers and correct anything it got wrong. Entries under <b>Lessons</b> are mistakes not to repeat — the AI is told to obey those first, and they are the last thing dropped when memory fills up.",
  "mem.project": "Project memory — {dir}",
  "mem.projectHint":
    "<b>Shared by every session</b> in this working directory, including new conversations. Put rules you want the AI to always follow here.",
  "mem.session": "This conversation only",
  "mem.sessionHint": "Belongs to this chat alone; deleting the chat deletes it.",
  "mem.empty": "(empty)",
  "kb.note":
    "Mount folders or files as a knowledge base for this chat; when you ask, the AI retrieves with knowledge_search before answering (local only, this chat only).",
  "kb.addLabel": "Add path (folder or file, absolute path)",
  "kb.input.placeholder": "e.g. D:\\kb or ~/wisecortex/workspace/kb/faq.md",
  "kb.pick.title": "Pick a folder",
  "kb.empty": "No knowledge base paths mounted yet.",
  "kb.pickPrompt": "Knowledge base folder absolute path:",

  // chat lifecycle (used by ws-dispatcher)
  "chat.done": "Done ({n} turns, {cost})",
  "chat.done.duration": " · {duration}s",
  "chat.done.cache": "Cache hit {rate}% ({hits}/{total}, {tokens})",
  "chat.interrupted": "Interrupted",
  "chat.queued": "⏳ Queued — will run automatically after the current turn",
  "chat.retry": "Retry",
  "chat.retrying": "Retrying…",
  "error.insufficient_credit": "Insufficient balance",
  "error.insufficient_credit.action": "Top up",

  // artifacts
  "artifact.rendered": "Generated · click to preview on the right",
  "artifact.source": "Generated · click to view source",

  // hero (empty state)
  "greet.night": "Up late",
  "greet.morning": "Good morning",
  "greet.afternoon": "Good afternoon",
  "greet.evening": "Good evening",
  "hero.title": "{greet} — what can I do for you?",
  "hero.sub":
    "WiseCortex can write code, run scripts, research online, call skills, and push results to your IM.",
  "suggest.landing.title": "Build a product landing page",
  "suggest.landing.sub": "A responsive one-pager from a brief",
  "suggest.research.title": "Research a topic online",
  "suggest.research.sub": "Gather sources and summarize key points",
  "suggest.debug.title": "Help me track down a bug",
  "suggest.debug.sub": "Find the root cause, then fix it",
  "suggest.chart.title": "Turn data into charts",
  "suggest.chart.sub": "CSV / tables → visualization",

  // settings — language
  "settings.language.title": "Interface language",
  "settings.language.sub": "Takes effect immediately (the page reloads).",
  "settings.language.label": "Language",

  // settings — generic
  "common.cancel": "Cancel",
  "common.save": "Save",
  "settings.pick": "Choose…",
  "settings.keySetPlaceholder": "Already set (leave empty = keep)",

  // settings — model access
  "settings.models.title": "Model access",
  "settings.models.sub": "BYOK with your own API key; set per-model prices for cost accounting",
  "settings.models.add": "Add model",
  "settings.models.default": "Default model",
  "settings.models.empty": "No models yet — click “Add model” to configure one.",
  "settings.models.badge.default": "Default",
  "settings.models.badge.vision": "Vision",
  "settings.models.badge.visionTitle": "Supports image input",
  "settings.models.priceIn": "in",
  "settings.models.priceOut": "out",
  "settings.models.priceUnit": "¥ / million tokens",
  "settings.models.keySet": "Key set",
  "settings.models.keyUnset": "Not configured",
  "settings.models.subscription": "Subscription",
  "settings.models.subscriptionTitle":
    "This model draws on your subscription login (OAuth); no API key needed",
  "settings.models.configure": "Configure",
  "settings.models.delete": "Delete",
  "settings.models.optionLabel": "{model} ({provider})",

  // settings — workspace
  "settings.workspace.title": "Working directory",
  "settings.workspace.sub":
    "Global home for AI-generated scripts / knowledge base / self-learning; each chat can set a temporary directory in the composer",
  "settings.workspace.label": "Global working directory",
  "settings.workspace.placeholder": "Empty = use system default",
  "settings.workspace.hintTauri":
    "Click “Choose…” to pick a directory via the system dialog; you can also edit the path directly.",
  "settings.workspace.hintWeb": "WebUI: enter an absolute path on the server.",
  "settings.workspace.pickPrompt": "Global working directory (empty = system default):",

  // settings — proxy
  "settings.proxy.title": "Network proxy",
  "settings.proxy.sub":
    "When set, all outbound access (LLM, skills/marketplace, web tools) goes through the proxy; empty = direct",
  "settings.proxy.label": "Proxy address",
  "settings.proxy.placeholder": "http://host:port or socks5://host:port",
  "settings.proxy.hint":
    "Supports http/https/socks5. Applies immediately (hot reload). Can also be overridden by the WC_PROXY env var.",

  // settings — Claude subscription OAuth
  "settings.claudeOauth.title": "Claude subscription login",
  "settings.claudeOauth.sub":
    "Run inference locally using your Claude subscription (Pro/Max) quota for side-by-side model comparison. After logging in, add a model in “Model management” with “Claude subscription” checked.",
  "settings.claudeOauth.step1":
    "If the browser didn't open automatically (common in the desktop shell), click “Open” or “Copy link” to open the authorization page manually:",
  "settings.claudeOauth.step2":
    "After logging in, copy the code shown on the page and paste it below to finish:",
  "settings.claudeOauth.codePlaceholder": "Paste the authorization code (like code#state)",
  "settings.claudeOauth.warn":
    "⚠️ Reuses Claude Code's OAuth client; third-party use is a gray area, for personal local use only; may be restricted by official policy or stop working at any time.",
  "settings.claudeOauth.loggedIn": "✓ Signed in to Claude subscription",

  // settings — ChatGPT subscription OAuth
  "settings.chatgptOauth.title": "ChatGPT subscription login",
  "settings.chatgptOauth.sub":
    "Run Codex models locally using your ChatGPT (Plus/Pro) subscription quota for comparison. After logging in, add a model in “Model management” with “ChatGPT subscription” checked.",
  "settings.chatgptOauth.step1":
    "Click “Open” or “Copy link” to open the authorization page in the browser and sign in:",
  "settings.chatgptOauth.step2":
    "After authorizing, the browser redirects to <code>localhost:1455</code> (it usually won't open — that's normal). Paste the <b>entire URL from the address bar</b>, or just the code, below:",
  "settings.chatgptOauth.codePlaceholder":
    "Paste the code or the full localhost:1455/auth/callback?code=... address",
  "settings.chatgptOauth.warn":
    "⚠️ Reuses Codex's OAuth client; third-party use is a gray area, for personal local use only; may be restricted by official policy or stop working at any time. Requires a proxy in China.",
  "settings.chatgptOauth.loggedIn": "✓ Signed in to ChatGPT subscription",
  "settings.grokOauth.title": "Grok subscription login",
  "settings.grokOauth.sub":
    'Run Grok models on your X Premium / SuperGrok subscription quota (device-code login, works for local and server deployments). After signing in, add a model with "Grok subscription" checked under Model management.',
  "settings.grokOauth.step1":
    "Click Open to visit the authorization page in a browser on any device:",
  "settings.grokOauth.step2":
    "Enter the code below on that page; this page signs in automatically once done:",
  "settings.grokOauth.warn":
    "⚠️ Reuses xAI's official Grok-CLI OAuth client; third-party use is a gray area, for personal use only; may be restricted by official policy or stop working at any time.",
  "settings.grokOauth.loggedIn": "✓ Signed in to Grok subscription",
  "settings.grokOauth.waiting":
    "Waiting for authorization… this page updates automatically once you finish in the browser",
  "settings.grokOauth.expired": "Device code expired — click Login again",

  // settings — OAuth shared
  "settings.oauth.checking": "Checking…",
  "settings.oauth.login": "Log in",
  "settings.oauth.logout": "Log out",
  "settings.oauth.open": "Open",
  "settings.oauth.copyLink": "Copy link",
  "settings.oauth.notLoggedIn": "Not signed in",
  "settings.oauth.relogin": "Re-login",
  "settings.oauth.unknown": "Status unknown",
  "settings.oauth.openedCode": "Authorization page opened — paste the code after signing in",
  "settings.oauth.openedUrl":
    "Authorization page opened — paste the address-bar URL after signing in",
  "settings.oauth.manualOpen":
    "Click “Open” or “Copy link” to open the authorization page manually",
  "settings.oauth.waitingBrowser":
    "Authorization page opened in your browser — finish signing in and this page detects it automatically…",
  "settings.oauth.loopbackCopied":
    "Authorization link copied — open it in your browser and finish signing in; this page detects it automatically…",
  "settings.oauth.loopbackTimeout": "Timed out waiting for authorization — click Login to retry",
  "settings.oauth.loginFailed": "Login failed: {e}",
  "settings.oauth.copied": "Link copied — open it in your browser",
  "settings.oauth.redeeming": "Redeeming…",
  "settings.oauth.failed": "Failed: {e}",
  "settings.oauth.unknownError": "Unknown error",

  // settings — iteration limits
  "settings.maxiter.title": "Tool-call turn limit",
  "settings.maxiter.sub":
    "Max consecutive tool-call turns per task (runaway guard). Interactive chat and unattended tasks are set separately.",
  "settings.maxiter.interactive": "Interactive chat limit",
  "settings.maxiter.interactivePlaceholder": "Empty = default 1000",
  "settings.maxiter.interactiveHint":
    "Used for chats you drive yourself in the desktop/web app — set high; development rarely hits it and you can stop anytime. **Background long tasks (task_start) use this same limit.** Env var WC_MAX_ITERATIONS_INTERACTIVE takes priority.",
  "settings.maxiter.unattended": "Unattended task limit",
  "settings.maxiter.unattendedPlaceholder": "Empty = default 50",
  "settings.maxiter.unattendedHint":
    "Used for scheduled tasks / Feishu·WeCom-triggered tasks (unattended, runaway-cost guard). Raise it for long tasks like multi-file generation + deploy. Env var WC_MAX_ITERATIONS takes priority.",
  "settings.maxiter.subagent": "Sub-task (sub-agent) turn limit",
  "settings.maxiter.subagentPlaceholder": "Empty = default 100",
  "settings.maxiter.subagentHint":
    "Applies to sub-agents spawned by the <b>task</b> tool in chat. Previously hardcoded to 15, which was far too low — reviewing a diff or analysing logs burns that just reading a few files, so sub-tasks almost always ended in 'turn limit reached'. Kept separate from the chat limit because sub-tasks often run several in parallel, so cost multiplies. Env var WC_SUBAGENT_MAX_ITERATIONS takes priority.",

  // settings — log cleanup
  "settings.logclean.title": "Scheduled-task log cleanup",
  "settings.logclean.sub":
    "Scheduled-task execution logs are cleaned by size: only files over the limit are trimmed, oldest whole run first. Logs under the limit are never touched. Checked hourly.",
  "settings.logclean.autoLabel": "Auto-clean logs",
  "settings.logclean.autoSub": "When off, logs are kept forever and you clean them yourself.",
  "settings.logclean.maxLabel": "Per-task log limit (MB)",
  "settings.logclean.placeholder": "Empty = default 10",
  "settings.logclean.hint":
    "Over the limit, whole <b>runs</b> are dropped oldest-first, and the most recent run is always kept — so a task can never show many runs with an empty log. Applies immediately, no restart needed.",

  // settings — web search
  "settings.websearch.title": "Web search",
  "settings.websearch.sub":
    "Provider for the web_search tool; enter an API key or self-host SearXNG, empty = DuckDuckGo (no key)",
  "settings.websearch.provider": "Provider",
  "settings.websearch.ddg": "DuckDuckGo (default, no key)",
  "settings.websearch.searxng": "SearXNG (self-hosted)",
  "settings.websearch.brave": "Brave Search (API key)",
  "settings.websearch.tavily": "Tavily (API key)",
  "settings.websearch.searxngUrl": "SearXNG URL",

  // settings — reasoning effort
  "settings.effort.title": "Reasoning effort (extended thinking)",
  "settings.effort.sub":
    "Let the model think more before answering — steadier on complex tasks / hard bugs; increases latency and cost. Auto-translated per provider: Anthropic uses thinking, OpenAI/Gemini use reasoning_effort, Qwen/Hunyuan use enable_thinking; models with built-in thinking (e.g. DeepSeek) send no parameter.",
  "settings.effort.label": "Effort",
  "settings.effort.off": "Off (default)",
  "settings.effort.xhigh": "xhigh (coding sweet spot)",
  "settings.effort.hint":
    "Applies immediately (hot reload). Can also be overridden by the WC_REASONING_EFFORT env var.",

  // settings — access control
  "settings.access.title": "Access control",
  "settings.access.sub": "Protect the local WS/REST and control dangerous operations",
  "settings.access.enableKey": "Enable access_key",
  "settings.access.enableKeySub":
    "When on, all connections must carry the key (restart the server to apply). <b>Required for external access</b> — the server binds to localhost by default; use an nginx reverse proxy to expose it.",
  "settings.access.keyPlaceholder": "Set the access key",
  "settings.access.envManaged":
    "Set via the WC_ACCESS_KEY environment variable — change it on the server (can't be edited here).",
  "settings.access.confirm": "Confirm before dangerous operations",
  "settings.access.confirmSub":
    "When on, operations like writing files / shell ask for consent first (off = unattended, fully automatic)",
  "settings.access.automem": "Auto memory",
  "settings.access.automemSub":
    "When on, long chats periodically extract key points from the conversation into session memory in the background (extra LLM calls — enable as needed; restart the server to apply)",
  "settings.access.autotrim": "Auto-trim context",
  "settings.access.autotrimSub":
    "Send each image once; later turns omit it, and compaction drops images too",
  "settings.access.autotrimHelp":
    "Images are the most expensive thing in a context — a single screenshot is easily a thousand tokens, and by default it is resent verbatim on every single turn. When on, an image is included only for the turn it was sent in; later turns replace it with a one-line placeholder, and history compaction drops images as well. For UI tweaks or test runs the image is useless once seen; turn this off when you need to compare the same image repeatedly.",

  // settings — MCP
  "settings.mcp.title": "MCP servers",
  "settings.mcp.sub":
    "Connect MCP servers (stdio or Streamable HTTP); their tools/resources/prompts are exposed to the AI as <code>mcp__server__tool</code>; reconnects immediately on change",
  "settings.mcp.label":
    "Server config (JSON: name → stdio {command,args,env} or HTTP {url,headers})",
  "settings.mcp.hint":
    "resources/prompts auto-synthesize <code>list_resources</code>/<code>read_resource</code>/<code>list_prompts</code>/<code>get_prompt</code> tools; sampling/roots reverse requests are handled automatically (stdio only).",
  "settings.mcp.save": "Save and reconnect",
  "settings.mcp.discovered": "Discovered tools",
  "settings.mcp.noTools": "(no connected tools yet)",
  "settings.mcp.savedReconnecting": "Saved, reconnecting in the background…",

  // settings — hooks
  "settings.hooks.title": "Event hooks",
  "settings.hooks.sub":
    "Run commands at event points for guardrails/side effects: PreToolUse can intercept tools, PostToolUse appends context, SessionStart/Stop/UserPromptSubmit",
  "settings.hooks.label":
    "Hook config (JSON: event → [{matcher, command, timeout_ms}]; matcher is a tool-name regex, used only by Pre/PostToolUse)",
  "settings.hooks.save": "Save",
  "settings.hooks.hint":
    'The command receives the event JSON on stdin; block: JSON <code>{"decision":"block","reason":"…"}</code> or a non-zero exit code (reason from stderr); append context: <code>{"additionalContext":"…"}</code>.',

  // settings — statuses
  "settings.status.loading": "Loading…",
  "settings.status.loadFailed": "Load failed: {e}",
  "settings.status.workspaceUpdated": "Working directory updated",
  "settings.status.workspaceReset": "Restored default working directory",
  "settings.status.websearchUpdated": "Web search updated",
  "settings.status.jsonParseFailed": "JSON parse failed: {e}",
  "settings.status.saving": "Saving…",
  "settings.status.saved": "Saved",
  "settings.status.effortUpdated": "Reasoning effort updated",
  "settings.status.proxySet": "Proxy set (hot reloaded)",
  "settings.status.proxyOff": "Proxy off (direct)",
  "settings.status.maxiterSet": "Unattended task limit set to {v} (hot reloaded)",
  "settings.status.maxiterReset": "Restored default limit 50 (hot reloaded)",
  "settings.status.maxiterInteractiveSet": "Interactive chat limit set to {v} (hot reloaded)",
  "settings.status.maxiterInteractiveReset": "Restored default limit 1000 (hot reloaded)",
  "settings.status.maxiterSubagentSet": "Sub-task limit set to {v} (hot reloaded)",
  "settings.status.maxiterSubagentReset": "Restored sub-task default limit 100 (hot reloaded)",
  "settings.status.logcleanDefault": "Default limit 10 MB",
  "settings.status.logcleanOn": "Log auto-cleanup enabled",
  "settings.status.logcleanOff": "Log auto-cleanup disabled (kept forever)",
  "settings.status.logcleanMb": "Limit {v} MB",
  "settings.status.logcleanUpdated": "Log cleanup updated: {msg}",
  "settings.status.accessKeyOff": "Access key disabled (restart the server to apply)",
  "settings.status.accessKeySet": "Access key set (restart the server to apply)",
  "settings.status.updated": "Updated",
  "settings.status.automemOn": "Auto memory enabled (restart the server to apply)",
  "settings.status.autotrimOn": "Auto-trim context on (each image sent once)",
  "settings.status.autotrimOff": "Auto-trim context off (images resent every turn)",
  "settings.status.automemOff": "Auto memory disabled (restart the server to apply)",

  // settings — model modal
  "settings.modal.editTitle": "Configure model",
  "settings.modal.addTitle": "Add model",
  "settings.modal.provider": "Provider",
  "settings.modal.endpoint": "API endpoint",
  "settings.modal.useClaudeOauth": "Use Claude subscription (no API key needed)",
  "settings.modal.useClaudeOauthHint":
    "When checked, this model uses the Claude subscription quota you're signed into locally (fixed Anthropic official endpoint); sign in under “Claude subscription login” first. Set the “Model” above to a Claude model id (e.g. claude-sonnet-4-6).",
  "settings.modal.useChatgptOauth": "Use ChatGPT subscription (no API key needed)",
  "settings.modal.useChatgptOauthHint":
    "When checked, this model uses the ChatGPT subscription quota you're signed into locally (fixed Codex backend + Responses transport); sign in under “ChatGPT subscription login” first. Set the “Model” above to a Codex model id (e.g. gpt-5-codex).",
  "settings.modal.useGrokOauth": "Use Grok subscription (no API key)",
  "settings.modal.useGrokOauthHint":
    "When checked, this model uses your signed-in Grok subscription quota (api.x.ai, OpenAI-compatible transport); sign in under “Grok subscription login” first. Models: grok-code-fast-1, grok-4-1, etc.",
  "settings.modal.useGeminiOauth": "Use Gemini subscription (no API key)",
  "settings.modal.useGeminiOauthHint":
    "When checked, this model uses your signed-in Gemini subscription quota (Code Assist backend, native Gemini transport); sign in under “Gemini subscription login” first. Models: gemini-2.5-pro, gemini-2.5-flash, etc.",
  "settings.geminiOauth.title": "Gemini subscription login",
  "settings.geminiOauth.sub":
    "Run Gemini models on your personal Google account (Gemini Code Assist free/paid quota). After signing in, add a model with “Gemini subscription” checked under Model management.",
  "settings.geminiOauth.step1":
    "Click Open to sign in with your Google account; the authorization page will show a code:",
  "settings.geminiOauth.step2":
    "Paste the code shown on the codeassist.google.com/authcode page here:",
  "settings.geminiOauth.warn":
    "⚠️ Reuses gemini-cli's official OAuth client; third-party use is a gray area, for personal use only; the free tier is bound by Google policy and may be rate-limited or stop working at any time.",
  "settings.geminiOauth.loggedIn": "✓ Signed in to Gemini subscription",
  "settings.geminiOauth.loggedInAs": "✓ Signed in to Gemini subscription ({email})",
  "settings.geminiOauth.codePlaceholder":
    "Paste the code (or the full codeassist.google.com/authcode?code=... URL)",
  "settings.modal.vision": "Supports image input (vision)",
  "settings.modal.visionHint":
    "Whether this model can “see” images. Auto-checked from a built-in list after picking a model; you can change it manually. <b>Turn off and images are stripped before sending</b> (text-only models like deepseek must turn it off, or screenshots get bounced with a 4xx).",
  "settings.modal.apiKeyHint":
    "Stored only on the local backend, never uploaded. Can be left empty when “Use Claude subscription” is checked.",
  "settings.modal.priceLabel": "Price (¥ / million tokens, optional)",
  "settings.modal.priceIn": "¥ in",
  "settings.modal.priceInPlaceholder": "Input price",
  "settings.modal.priceOut": "¥ out",
  "settings.modal.priceOutPlaceholder": "Output price",
  "settings.modal.priceCache": "¥ cache",
  "settings.modal.priceCachePlaceholder": "Cache-read price (empty = input × 0.1)",
  "settings.modal.priceHint":
    "If cache-read price is empty, it's estimated at 1/10 of the input price; for heavy-cache scenarios set it accurately or cost will be inflated.",
  "settings.modal.maxtokLabel": "Max output tokens per call (max_tokens, optional)",
  "settings.modal.maxtokPlaceholder": "Empty = use global default 32768",
  "settings.modal.maxtokHint":
    "Too small truncates tool args when writing large files and causes errors; raise it for large files / long output. Too large may be rejected by the model's real limit (400) — set it to the model's true output cap.",
  "settings.modal.effortLabel": "Reasoning effort (thinking)",
  "settings.modal.effortDefault": "(follow global default)",
  "settings.modal.effortOff": "Off",
  "settings.modal.effortHint":
    "Applies to this model only, overriding global. Auto-translated per provider (Anthropic→thinking, OpenAI/Gemini→reasoning_effort, Qwen/Hunyuan→enable_thinking; models with built-in thinking like DeepSeek send no parameter, so this has no effect). “Off” = this model doesn't think; “follow global default” = use the global reasoning effort above.",
  "settings.modal.test": "Test connection",
  "settings.modal.baseHintPreset": "Preset endpoint (auto-filled)",
  "settings.modal.baseHintCompat":
    "OpenAI-compatible endpoint — enter it yourself, e.g. http://localhost:8000/v1",
  "settings.modal.modelHintPreset": "Preset by the provider — pick the model ID to use.",
  "settings.modal.modelHintCompat": "OpenAI-compatible API — enter the model ID manually.",
  "settings.modal.needEndpoint": "Please enter the Endpoint",
  "settings.modal.needModel": "Please select / enter the Model ID",
  "settings.modal.testing": "Testing…",
  "settings.modal.testOk": "✓ Connection OK",
  "settings.modal.testFail": "✗ Failed: {e}",

  // chat / sidebar / jobs / artifact / platform
  "chat.thinking": "✻ Thinking · {preview}",
  "chat.processing": "Processing…",
  "sidebar.currentTask": "Current task",
  "sidebar.running": "Running…",
  "sidebar.deleteTask": "Delete task",
  "sessions.deleteConfirm.title": "Delete task",
  "sessions.deleteConfirm.message": "Delete task “{name}”? This can’t be undone.",
  "sidebar.renameTask": "Rename task",
  "sidebar.section.tasks": "Tasks",
  "sidebar.section.workspace": "Workspaces",
  "sidebar.newSessionInDir": "New session in this folder",
  "jobs.secAgo": "{n}s ago",
  "jobs.minAgo": "{n}m ago",
  "jobs.hourAgo": "{n}h ago",
  "jobs.dayAgo": "{n}d ago",
  "jobs.sub":
    "Long-running background tasks the AI starts with task_start (async — viewable and stoppable)",
  "jobs.empty": "No background tasks. They appear here after the AI calls task_start.",
  "jobs.running": "Running",
  "jobs.completed": "Done",
  "artifact.tab.preview": "Preview",
  "artifact.tab.code": "Code",
  "common.refresh": "Refresh",
  "common.close": "Close",
  "artifact.readFailed": "Read failed: {e}",
  "platform.cwdPrompt": "Working directory (absolute path, empty = restore default):",

  // generic actions
  "common.edit": "Edit",
  "common.delete": "Delete",

  // scheduled tasks
  "tasks.desc": "Run a prompt on a schedule and push the result to a channel",
  "tasks.add": "＋ New task",
  "tasks.stat.running": "Running",
  "tasks.stat.totalRuns": "Total runs",
  "tasks.stat.disabled": "Disabled",
  "tasks.stat.successRate": "Last success rate",
  "tasks.status.ok": "Success",
  "tasks.status.notRun": "Not run",
  "tasks.status.maxIter": "Unfinished · turn limit",
  "tasks.status.llmError": "Model call failed",
  "tasks.status.workdirNotFound": "Working directory missing",
  "tasks.status.notifyErr": "Notification failed",
  "tasks.empty": "No tasks yet — click “＋ New task” to create one.",
  "tasks.run": "▶ Run",
  "tasks.runTitle": "Run once now and see the result, no need to wait for the schedule",
  "tasks.logs": "Logs",
  "tasks.runsCount": "Ran {n} times",
  "tasks.lastDuration": "last {s}s",
  "tasks.modelTitle": "Model used",
  "tasks.runningBtn": "Running…",
  "tasks.runFailed": "Run failed: {e}",
  "tasks.logsTitle": "{name} · Execution logs",
  "tasks.noLogs": "No logs yet",
  "tasks.interval.days": "Every {n} day(s)",
  "tasks.interval.hours": "Every {n} hour(s)",
  "tasks.interval.minutes": "Every {n} minute(s)",
  "tasks.interval.seconds": "Every {n} second(s)",
  "tasks.form.editTitle": "Edit task",
  "tasks.form.addTitle": "New task",
  "tasks.form.name": "Task name",
  "tasks.form.namePlaceholder": "e.g. “Daily news brief”",
  "tasks.form.prompt": "Task content (prompt)",
  "tasks.form.promptPlaceholder": "The instruction handed to the agent",
  "tasks.form.schedule": "Schedule",
  "tasks.form.byInterval": "By interval",
  "tasks.form.byCron": "Cron expression",
  "tasks.form.cronPlaceholder": "0 30 8 * * *  (sec min hour day month weekday; 5 fields also OK)",
  "tasks.form.cronHint": "Local time zone. e.g. every day 8:30 → <code>30 8 * * *</code>",
  "tasks.form.channel": "Push channel",
  "tasks.form.model": "Model (optional)",
  "tasks.form.modelHint":
    "Empty = use the global default model; you can assign a dedicated model for this task (e.g. a cheaper model for high-frequency tasks).",
  "tasks.form.workdir": "Working directory (optional)",
  "tasks.form.workdirPlaceholder":
    "Empty = global working directory; an absolute path runs this task isolated in that directory",
  "tasks.form.workdirHint":
    "When set, this task runs in that directory and loads its <code>&lt;dir&gt;/skills</code> project-level skills, without polluting the global ones.",
  "tasks.form.create": "Create",
  "tasks.form.noChannel": "(no notification)",
  "tasks.form.defaultModel": "Default model",
  "tasks.form.defaultModelNamed": "Default model ({name})",
  "tasks.form.required": "Task name / content / schedule are required",

  // skills
  "skills.src.builtin": "Built-in",
  "skills.src.installed": "Local / Git",
  "skills.src.workdir": "Project",
  "skills.group.builtin": "Built-in Skills",
  "skills.group.workdir": "Project Skills",
  "skills.group.installed": "Installed Skills",
  "skills.filter.all": "All",
  "skills.filter.builtin": "Built-in",
  "skills.filter.workdir": "Project",
  "skills.filter.installed": "Installed",
  "skills.searchMine": "Search by name or description…",
  "skills.clearSearch": "Clear search",
  "skills.empty.noMatch": "No skill matches “{q}”. Try a shorter keyword, or switch category.",
  "skills.desc":
    "SKILL.md works as you write it; built-ins ship preinstalled, the market lets you switch sources, and you can migrate from openclaw",
  "skills.migrate": "Migrate from openclaw",
  "skills.import": "Import from Git",
  "skills.create": "Create skill",
  "skills.tab.mine": "My skills",
  "skills.tab.market": "Market",
  "skills.source": "Source",
  "skills.searchPlaceholder": "Search market skills…",
  "skills.toggle.on": "Enabled (click to disable)",
  "skills.toggle.off": "Disabled (click to enable)",
  "skills.noDesc": "(no description)",
  "skills.viewSkillMd": "View SKILL.md",
  "skills.uninstall": "Uninstall",
  "skills.status.enabling": "Enabling {name}…",
  "skills.status.disabling": "Disabling {name}…",
  "skills.status.uninstalling": "Uninstalling {name}…",
  "skills.installed": "Installed",
  "skills.install": "Install",
  "skills.status.installing": "Installing {name}…",
  "skills.status.installed": "Installed {name}",
  "skills.status.installFailed": "Install failed: {e}",
  "skills.empty.mine":
    "No skills yet. Install from the “Market”, “Import from Git”, “Migrate from openclaw”, or “Create skill”.",
  "skills.empty.market":
    "No installable skills from this source (it may be unreachable or the search returned nothing). Try another source or clear the search.",
  "skills.customSource": "Custom source…",
  "skills.loadingMarket": "Loading market…",
  "skills.marketLoadFailed": "Market load failed: {e}",
  "skills.switchingSource": "Switching source…",
  "skills.customSourcePrompt": "Custom registry source URL (static JSON):",
  "skills.cannotRead": "(cannot read)",
  "skills.import.title": "Import skills from Git",
  "skills.import.urlLabel": "Git repository URL",
  "skills.import.subLabel": "Subdirectory (optional)",
  "skills.import.subPlaceholder": "default: skills",
  "skills.import.note":
    "Supports whole-repo or single-skill import; <code>@file</code> references are inlined automatically.",
  "skills.import.ok": "Import",
  "skills.import.needUrl": "Please enter the Git URL",
  "skills.import.cloning": "Cloning and importing…",
  "skills.migrate.title": "Migrate skills from openclaw",
  "skills.migrate.note":
    "Auto-detected common openclaw skill locations (~/.config/openclaw, .agents/skills, etc.). Check items to import into WiseCortex.",
  "skills.migrate.scanning": "Scanning…",
  "skills.migrate.importSelected": "Import selected",
  "skills.migrate.empty": "No openclaw skills detected. Try “Import from Git” instead.",
  "skills.migrate.exists": "Already exists",
  "skills.migrate.needOne": "Please select at least one",
  "skills.migrate.importing": "Importing…",
  "skills.migrate.done": "Migrated {n}/{total} skills",
  "skills.creator.step.basic": "Basics",
  "skills.creator.step.trigger": "Trigger",
  "skills.creator.step.body": "Skill body",
  "skills.creator.step.tools": "Tools",
  "skills.creator.step.preview": "Preview & save",
  "skills.creator.prev": "Back",
  "skills.creator.next": "Next",
  "skills.creator.slugLabel": "Skill slug (used by invoke, kebab-case)",
  "skills.creator.descLabel": "One-line description",
  "skills.creator.descPlaceholder": "Aggregate git commits and issues into a weekly report",
  "skills.creator.triggerLabel": "When to use (trigger, natural language)",
  "skills.creator.triggerPlaceholder": "When a weekly report / work summary is mentioned",
  "skills.creator.bodyLabel": "Skill body (Markdown, @path references files)",
  "skills.creator.bodyPlaceholder": "# Steps\n1. …",
  "skills.creator.toolsLabel": "Tools this skill will use",
  "skills.creator.mdWhenUse": "## When to use",
  "skills.creator.mdTools": "## Available tools",
  "skills.creator.previewLabel": "Preview SKILL.md:",
  "skills.creator.save": "Save skill",
  "skills.creator.needSlug": "Please enter a slug",

  // channels
  "common.copy": "Copy",
  "channels.desc": "Connect an IM for two-way chat, or configure outbound push targets",
  "channels.name": "Name",
  "channels.saveFailed": "Save failed",
  "channels.platform.feishu.name": "Feishu",
  "channels.platform.feishu.desc": "Event subscription · group/DM two-way",
  "channels.platform.wecom.name": "WeCom",
  "channels.platform.wecom.desc": "Encrypted callback · app messages",
  "channels.platform.onebot.name": "QQ",
  "channels.platform.onebot.desc": "OneBot / NapCat · groups and DMs",
  "channels.platform.email.name": "Email",
  "channels.platform.email.desc": "SMTP send · outbound notifications",
  "channels.platform.webhook.name": "Generic Webhook",
  "channels.platform.webhook.desc": "Outbound only · push to any HTTP endpoint",
  "channels.callback.feishuSummary":
    "Public deployment (advanced): event subscription callback URL",
  "channels.callback.feishuNote":
    "Only when the server has a public address. On a local machine use the “Long connection” below (no public address needed).",
  "channels.callback.wecomHint":
    " (requires public HTTPS; fill into the WeCom app's “Receive messages”)",
  "channels.callback.genericHint": " (fill into the platform's console)",
  "channels.callback.label": "Inbound callback URL",
  "channels.appPushTarget": "App push → {target}",
  "channels.notConfigured": "Not configured. ",
  "channels.notConfigured.webhook": "Add an outbound webhook target.",
  "channels.notConfigured.generic": "Once connected you can push and have two-way chat.",
  "channels.badge.configured": "Configured",
  "channels.badge.notConnected": "Not connected",
  "channels.feishu.scan": "Connect by QR",
  "channels.feishu.lcOn": "Long conn: on (no public address)",
  "channels.feishu.lcOff": "Long conn: off",
  "channels.feishu.lcTitle":
    "Long connection (WebSocket, no public callback) — messages come through here; the toggle takes effect immediately, no restart",
  "channels.feishu.lcOnStatus": "Long connection enabled (connects within a few seconds)",
  "channels.feishu.lcOffStatus": "Long connection disabled (disconnects within a few seconds)",
  "channels.feishu.appPush": "Reuse app for push",
  "channels.feishu.appPushTitle":
    "Use the QR-connected Feishu app bot for outbound push (selectable by scheduled tasks); no need to create a custom bot",
  "channels.feishu.configOutbound": "Configure outbound push",
  "channels.wecom.credsSet": "Receive credentials: set",
  "channels.wecom.credsConfig": "Configure receive credentials",
  "channels.addTarget": "Add target",
  "channels.connect": "Connect {name}",
  "channels.delete": "Delete {name}",
  "channels.copyOk": "Callback URL copied",
  "channels.copyFail": "Copy failed, please select and copy the text manually",
  "channels.faPush.recentHint":
    "Below are recent chats that have messaged the bot — just pick one.",
  "channels.faPush.noRecentHint":
    "No recent chats yet — first message the bot in Feishu (@ it in a group or DM), then come back and refresh. You can also paste a chat_id directly.",
  "channels.faPush.title": "Feishu app push (reuse QR connection)",
  "channels.faPush.namePlaceholder": "e.g. “Dev group push”",
  "channels.faPush.chatLabel": "Target chat_id",
  "channels.faPush.chatPlaceholder": "oc_… (group) / pick a recent chat",
  "channels.faPush.required": "Name and target chat are required",
  "channels.config.smtpHost": "SMTP server (host:port)",
  "channels.config.onebotBase": "OneBot HTTP base URL",
  "channels.config.smtpPlaceholder": "e.g. smtp.example.com:465",
  "channels.config.recipients": "Recipients (comma-separated)",
  "channels.config.username": "Username",
  "channels.config.usernamePlaceholder": "SMTP login, usually the sender email",
  "channels.config.password": "Password / app password",
  "channels.config.passwordPlaceholder": "SMTP password or app password",
  "channels.config.from": "From (empty = username)",
  "channels.config.emailNote":
    "Port 465 = implicit TLS, 587 = STARTTLS. Credentials are stored only on the local backend.",
  "channels.config.title": "Configure {name}",
  "channels.config.namePlaceholder": "e.g. “Dev group”",
  "channels.config.groupLabel": "Group number (target)",
  "channels.config.groupPlaceholder": "Group number, optional",
  "channels.config.callbackNote":
    "Two-way chat: fill the callback URL above into {name}'s console, and configure the app credentials via CLI.",
  "channels.config.required": "Name and address are required",
  "channels.scan.title": "Connect Feishu by QR",
  "channels.scan.generating": "Generating QR code…",
  "channels.scan.wait": "Please wait…",
  "channels.scan.note":
    "Scan with the <strong>Feishu / Lark App</strong> to authorize and create the app; app_id / app_secret are filled in automatically on success.<br/>Scanning <strong>only creates the app and gets credentials</strong>. To receive messages, also in the Feishu developer console: ① add <code>im:message</code> under permissions; ② choose “Long connection” for event subscription and subscribe to “Receive messages”; ③ publish a version. Then enable “Long connection” on this page and restart the server.",
  "channels.scan.failed": "Cannot start the QR flow: {e}",
  "channels.scan.prompt": "Scan with the Feishu / Lark App to authorize…",
  "channels.scan.connected": "Connected! app_id={id}",
  "channels.scan.denied": "Authorization denied; close and retry.",
  "channels.scan.expired": "QR code expired; close and scan again.",
  "channels.scan.error": "Error: {e}",
  "channels.wecom.title": "WeCom · Receive credentials",
  "channels.wecom.corpId": "Corp ID (corp_id)",
  "channels.wecom.secret": "App Secret (corp_secret)",
  "channels.wecom.secretPlaceholder": "empty = keep",
  "channels.wecom.agentId": "App AgentId (agent_id)",
  "channels.wecom.agentIdPlaceholder": "e.g. 1000002",
  "channels.wecom.token": "Callback Token (callback_token)",
  "channels.wecom.tokenPlaceholder": "the Token in the console's “Receive messages”",
  "channels.wecom.aesKey": "Callback EncodingAESKey",
  "channels.wecom.aesPlaceholder": "43 chars, empty = keep",
  "channels.wecom.note":
    'In the WeCom console → App → “Receive messages”, set API receive: URL = <span class="mono">{cb}</span> (must be reachable over public HTTPS), with Token / EncodingAESKey matching here. Credentials are stored only on the local backend.',
  "channels.wecom.savedReady": "WeCom credentials saved (ready)",
  "channels.wecom.savedIncomplete": "WeCom credentials saved (still missing fields)",
  "channels.platform.qqbot.name": "QQ Bot (official)",
  "channels.platform.qqbot.desc": "QQ Open Platform · AppID/Secret · gateway, no public address",
  "channels.qq.scan": "Bind by QR (recommended)",
  "channels.qq.creds": "Enter credentials manually",
  "channels.qq.credsSet": "Credentials: set",
  "channels.qq.connect": "Gateway: off · click to connect",
  "channels.qq.disconnect": "Gateway: on · click to disconnect",
  "channels.qq.toggleTitle":
    "QQ gateway (WebSocket, no public callback) — toggle takes effect immediately, no restart",
  "channels.qq.onStatus": "QQ gateway enabled (connects within a few seconds)",
  "channels.qq.offStatus": "QQ gateway disabled",
  "channels.qq.appId": "AppID",
  "channels.qq.scanTitle": "Bind QQ Bot by QR",
  "channels.qq.scanNote":
    "Scan with the <strong>mobile QQ app</strong>, then pick the bot to bind — its AppID/AppSecret are filled in automatically; no server address needed. The bind page is hosted by Tencent and shows the integrator as “third-party bot” by default.",
  "channels.qq.scanPrompt": "Scan with mobile QQ to bind…",
  "channels.qq.scanConnected": "Bound! AppID={id}",
  "channels.qq.title": "QQ Bot credentials",
  "channels.qq.appSecret": "AppSecret",
  "channels.qq.appSecretPlaceholder": "empty = keep current",
  "channels.qq.note":
    "Create a bot at q.qq.com, then copy AppID/AppSecret from its settings page. No server address needed — it connects out over a WebSocket gateway. If the gateway closes with code 4914, the bot lacks group/DM message permission on the platform.",
  "channels.qq.savedReady": "Saved (credentials ready, click to connect)",
  "channels.qq.savedIncomplete": "Saved (credentials incomplete)",
  "channels.platform.clawbot.name": "WeChat ClawBot",
  "channels.platform.clawbot.desc": "iLink long-poll · personal DM, no public address",
  "channels.clawbot.botId": "Bot ID",
  "channels.clawbot.scan": "Connect by QR",
  "channels.clawbot.scanTitle": "Connect WeChat ClawBot",
  "channels.clawbot.scanNote":
    "Scan with the <strong>WeChat app on your phone</strong> and confirm. One WeChat account can create exactly one bot, bound 1:1 to you. Text and voice (server-side transcription) work; images and files are not supported yet.",
  "channels.clawbot.scanPrompt": "Scan with WeChat and confirm on your phone…",
  "channels.clawbot.scanConnected": "Connected! {id}",
  "channels.clawbot.connect": "Polling: off · click to start",
  "channels.clawbot.disconnect": "Polling: on · click to stop",
  "channels.clawbot.toggleTitle":
    "iLink long-poll (no public callback) — takes effect immediately, no restart",
  "channels.clawbot.onStatus": "WeChat polling started",
  "channels.clawbot.offStatus": "WeChat polling stopped",
  "channels.clawbot.soloNote":
    "Run this in <strong>one place only</strong>. The update cursor is shared per bot, so polling from two machines at once splits your messages between them at random.",
  "tasks.form.chanGroupFeishu": "Feishu app",
  "tasks.form.chanGroupQq": "QQ bot",
  "tasks.form.feishuChatOpt": "Feishu · {id}",
  "tasks.form.qqC2cOpt": "QQ DM · {id}",
  "tasks.form.qqGroupOpt": "QQ group · {id}",
} as const;

export type MessageKey = keyof typeof en;
