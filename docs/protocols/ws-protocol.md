# WiseCortex 通信协议规范（v0）

- **日期**: 2026-06-02
- **用途**: WiseCortex 前端（TS）与 Rust server 之间的契约。前后端都必须照此实现，是整个通信层的地基。
- **状态**: v0 = 首个稳定基线。后续演进在此之上加版本号，不破坏 v0 已定义的消息形状。

> 约定：所有消息都是 UTF-8 JSON 文本帧。每条消息都有 `type` 字段。服务端推给客户端的事件**都额外带 `session_id`**（除少数全局事件）。

---

## 1. 传输层（WebSocket）

- **端点**: `GET /ws`，标准 WebSocket 升级。
- **鉴权**: 若服务端设置了 access key，客户端以 query 传入：`/ws?access_key=<urlencoded>`。握手失败时服务端拒绝升级，浏览器收到 close code **1006**；客户端据此触发重新鉴权。
- **重连**: 指数退避，初始 1000ms，每次 ×2，上限 30000ms；连接成功后退避重置为 1000ms。
- **离线队列**: 未连接时 `send()` 的消息入队，连接成功后按序 flush；发送失败重新入队。
- **连接建立后客户端自动**:
  1. 发送 `{type:"list_sessions"}`
  2. 若之前有订阅（`_subscribedId`），重发 `{type:"subscribe", session_id}`
  3. flush 离线队列
  4. 本地派发内部事件 `_ws_connected`
- **断开**: 本地派发内部事件 `_ws_disconnected`，清理进行中的 progress 状态（但**不**强制把 session 状态设为 idle —— 重连后由服务端快照纠正）。
- **内部事件**（不过网，仅前端内部用）: `_ws_connected` / `_ws_disconnected`。Rust 端无需关心，但 TS 前端的 dispatcher 要保留。

---

## 2. 客户端 → 服务端消息

| type | 字段 | 服务端行为 |
|---|---|---|
| `subscribe` | `session_id` | 绑定连接到 session。成功回 `subscribed` + 一帧 `session_update`(快照)，并 `replay_live_state`（若有进行中的命令，补发 progress + 缓冲的 tool_stdout）。session 不存在回 `error`。 |
| `message` | `session_id?`, `content`, `files?`, `images?` | 用户发消息。`images`(data_url 数组) 会被规整成 `files` 条目 `{data_url,name,mime_type}`。若 session 正在运行，先 interrupt 再处理（对齐 CLI 行为）。`session_id` 缺省取连接当前绑定的。 |
| `confirmation` | `session_id?`, `id`, `result` | 回应 `request_confirmation`。`result` 取值 `yes/y/no/n` 或自定义字符串。 |
| `interrupt` | `session_id?` | 中断当前任务。 |
| `list_sessions` | — | 返回最新 20 条 session（多取 1 条判断 `has_more`）。回 `session_list`。 |
| `run_task` | `session_id?` | 订阅完成、确保能收到广播后，触发 agent 开始执行 pending 任务。 |
| `ping` | — | 回 `pong`。 |

> 未知 type → 服务端回 `{type:"error", message:"Unknown message type: ..."}`；非法 JSON → `{type:"error", message:"Invalid JSON: ..."}`。

---

## 3. 服务端 → 客户端事件

所有事件经 `emit(type, **data)` 发出，统一形如 `{type, session_id, ...data}`。下表按用途分组。

### 3.1 Session 生命周期

| type | 字段 | 含义 |
|---|---|---|
| `subscribed` | `session_id` | 订阅成功。前端据此启用发送按钮。 |
| `session_list` | `sessions[]`, `has_more`, `cron_count` | session 列表（初次连接 / 重连）。 |
| `session_update` | **双形态**，见下 | session 状态/成本/任务数实时更新。 |
| `session_renamed` | `session_id`, `name` | 重命名。 |
| `session_deleted` | `session_id` | 删除。 |
| `session_restored` | `session`(完整对象) | 从回收站恢复。 |

**`session_update` 双形态**（前端需分支处理）:
- 形态①（来自 http_server 广播）: `{type, session:{id,name,status,total_cost,total_tasks,...}}` —— 整对象。
- 形态②（来自 web_ui_controller 实时）: `{type, session_id, cost?, tasks?, status?, latency?, cost_source?}` —— 映射到 `total_cost/total_tasks/status/latest_latency`。
- `status` 取 `working` / `idle` 等；变 `idle` 时前端会刷新 tasks/skills 并清理 progress。

### 3.2 对话消息

| type | 字段 | 含义 |
|---|---|---|
| `history_user_message` | `content`, `created_at?`, `images?` | **仅历史回放**时出现（实时不发）。`images` 元素为 data_url 或 `"pdf:<name>"` 哨兵。 |
| `assistant_message` | `content`, `files` | 助手消息（markdown）。本地 image 路径已被改写为 `/api/local-image` 代理 URL。 |
| `tool_call` | `name`, `args`, `summary` | 工具调用开始。`summary` 为人类可读摘要。 |
| `tool_result` | `result` | 工具结果。 |
| `tool_stdout` | `lines[]` | 流式 shell stdout（命令运行中持续推送，并被服务端缓冲供重连回放）。 |
| `tool_error` | `error` | 工具错误。 |
| `tool_args` | `args` | 格式化后的工具参数（增量）。 |
| `token_usage` | `...token_data` | token 用量（实时）。 |
| `complete` | `iterations`, `cost`, `duration?`, `cache_stats?`, `awaiting_user_feedback?`, `cost_source?` | 一轮任务完成。`cache_stats` 含 `total_requests/cache_hit_requests/cache_read_input_tokens`，前端据此算缓存命中率。 |
| `interrupted` | — | 任务被中断。 |
| `output` | `content` | 通用输出追加。 |

### 3.3 文件 / shell 预览（确认前）

| type | 字段 |
|---|---|
| `file_preview` | `path`, `operation`("write"\|"edit"), `is_new_file?` |
| `file_error` | `error` |
| `shell_preview` | `command` |
| `diff` | `old_size`, `new_size`（仅体积，内容不过网） |

### 3.4 进度

| type | 字段 | 含义 |
|---|---|---|
| `progress` | `message`, `progress_type`(默认"thinking"), `phase`("active"\|"done"), `status`("start"\|"stop", 兼容字段), `metadata?`, `started_at?`(ms epoch), `elapsed?`(done 时, 秒) | `phase=="active"` 显示进度/计时器（`started_at` 作计时原点）；否则清除。 |

### 3.5 阻塞式交互

| type | 字段 | 说明 |
|---|---|---|
| `request_confirmation` | `id`, `message`, `default` | 服务端线程**阻塞**等待，超时 300s 用 `default`。前端弹确认框，回 `confirmation`。 |
| `request_feedback` | `question`, `context`, `options[]` | 请求用户反馈（也由 `request_user_feedback` 工具触发）。 |

### 3.6 状态消息

| type | 字段 |
|---|---|
| `info` | `message` |
| `warning` | `message`（前端会美化重试类提示） |
| `success` | `message` |
| `error` | `message`, `code?`, `top_up_url?`（`code=="insufficient_credit"` 特殊渲染） |
| `log` | `level`, `message` |
| `todo_update` | `todos[]` |
| `server_stop` | —（全局，无 session_id） |
| `pong` | —（回应 ping） |

### 3.7 升级流程（version.js）

| type | 字段 |
|---|---|
| `upgrade_log` | 升级日志行 |
| `upgrade_complete` | 升级完成 |

---

## 4. HTTP REST 接口目录

鉴权同 WS（access key）。下表按域分组，标注 **[MVP]**（核心对话必需）与 **[后续]**。

### 会话 [MVP]
- `GET /api/sessions` — 列表
- `GET /api/sessions/:id/messages` — **历史回放**（返回 history_* 事件序列，前端 `_fetchHistory` 用）
- `GET /api/sessions/:id/files` — 会话文件
- `GET /api/sessions/:id/export` — 导出
- `GET /api/sessions/:id/skills` — 会话已启用 skills
- `PATCH /api/sessions/:id` — 改名等
- `PATCH /api/sessions/:id/model` `/reasoning_effort` `/submodel` `/working_dir` — 模型与运行参数
- `POST /api/sessions/:id/benchmark`
- `DELETE /api/sessions/:id` — 删除（软删，进回收站）

### 配置 / 模型 [MVP]
- `GET/PATCH /api/config`、`/api/config/settings`、`/api/config/test`
- `GET /api/config/models`、`POST /api/config/models/:id/default`、`PATCH/DELETE /api/config/models/:id`
- `PATCH /api/config/media/(image|video|audio)`
- `GET /api/providers` — 内置 provider 列表
- `GET /api/version`、`POST /api/version/upgrade`、`POST /api/restart`

### 文件 / 媒体 [MVP]
- `POST /api/upload` — 上传
- `GET /api/local-image` — 本地图片代理（assistant_message 改写后指向这里）
- `GET /api/media/image`、`GET /api/media/types`
- `POST /api/file-action`、`GET /api/exchange-rate`

### Skills [后续]
- `GET /api/skills`、`PATCH /api/skills/:id/toggle`
- `GET /api/store/skills`、`/api/store/skills/...`、`GET /api/creator/skills`
- `POST /api/my-skills/:id/publish`、`POST /api/brand/skills/:id/install`

### Onboard [后续]
- `GET /api/onboard/status`、`POST /api/onboard/complete`、`POST /api/onboard/skip-soul`

### MCP [后续]
- `GET/POST /api/mcp`、`PUT/DELETE /api/mcp/:id`、`PATCH /api/mcp/:id/enabled`
- `POST /api/mcp/:id/probe`、`GET /api/mcp/:id/tools`、`POST /api/mcp/:id/call`

### Channels（IM 集成） [后续]
- `GET/POST /api/channels`、`/api/channels/:id/...`(send/users/test/enabled)、`DELETE /api/channels/:id`

### Billing [后续]
- `GET /api/billing/(summary|daily|records|sessions)`、`POST /api/billing/clear`

### Brand / 授权 [后续]
- `GET /api/brand`、`/api/brand/(status|license)`、`POST /api/brand/(activate)`

### Cron / 定时任务 [后续]
- `GET /api/cron-tasks`、`POST /api/cron-tasks/:id/run`、`PATCH/DELETE /api/cron-tasks/:id`

### Memories [后续]
- `GET /api/memories`、`GET/PUT/DELETE /api/memories/:id`

### 回收站 [后续]
- `GET /api/trash`、`POST /api/trash/restore`、`DELETE /api/trash/sessions/:id`、`POST /api/trash/sessions/restore`

### 浏览器自动化 [后续]
- `GET /api/browser/status`、`POST /api/browser/(configure|reload|toggle)`、`/api/tool/browser`

### Profile [后续]
- `GET /api/profile`

---

## 5. 典型时序

### 5.1 发一条消息并跑任务
```
client                          server
  │  subscribe {session_id}  ──▶ │
  │  ◀── subscribed              │
  │  ◀── session_update(快照)    │
  │  message {content} ───────▶  │
  │  ◀── session_update(working) │
  │  ◀── progress(active)        │  （计时器）
  │  ◀── tool_call / tool_stdout │  （可多次）
  │  ◀── tool_result            │
  │  ◀── assistant_message       │  （markdown 流）
  │  ◀── complete(cost,cache)    │
  │  ◀── session_update(idle)    │
```

### 5.2 需要用户确认（阻塞）
```
  │  ◀── request_confirmation {id,message,default}
  │       （服务端线程阻塞，超时 300s 用 default）
  │  confirmation {id,result} ─▶
```

### 5.3 重连恢复
```
WS 断 → _ws_disconnected（清 progress）
退避重连 → onopen → list_sessions + subscribe(旧 id)
  ◀── session_list / subscribed / session_update(快照)
  ◀── progress(active)+tool_stdout（若命令仍在跑，replay_live_state 补发）
```

---

## 6. 实现注意

- **history vs live**: `history_user_message` 只在 `GET /api/sessions/:id/messages` 回放里出现，实时不发。Rust server 要把"回放"和"实时广播"两条路分清。
- **session_update 双形态**：Rust 端两处来源都要保留，前端必须能处理两种 shape。
- **阻塞确认**：`request_confirmation` 要挂起 agent 直到用户答复或超时。Rust 侧用 oneshot channel + tokio 超时实现，别用阻塞锁占住 runtime 线程。
- **本地图片代理**：assistant_message 里的 `file://`/绝对路径图片要改写成 `/api/local-image?...`，否则浏览器拒载。
- **多 tab 同 session**：一个 session 可被多个连接订阅，广播要发给全部订阅者。
