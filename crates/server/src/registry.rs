//! Session 注册表：保存 session 状态，并为每个 session 提供一个 broadcast 通道，
//! 让多个订阅同一 session 的连接（多 tab）都能收到广播事件。

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{broadcast, oneshot};
use wisecortex_core::llm::ChatMessage;

use crate::proto::{ServerEvent, Session, TaskConfig};

const BROADCAST_CAPACITY: usize = 256;

/// 一个 IM 来源：哪个渠道的哪个聊天（飞书私聊/群的 chat_id）。
/// 「当前会话」指针绑定在它身上——这样同一个飞书聊天可以在多个会话之间切换，
/// 而不再像以前那样被 `feishu-<chat_id>` 死死焊在唯一一个会话上。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImOrigin {
    pub channel: String,
    pub peer: String,
}

impl ImOrigin {
    pub fn feishu(chat_id: &str) -> Self {
        ImOrigin {
            channel: "feishu".to_string(),
            peer: chat_id.to_string(),
        }
    }
    /// 微信 ClawBot：peer 是发送者的 `xxx@im.wechat`。
    /// 一个微信号只能建一个 bot 且只支持私聊，所以实际上只会有一个 peer。
    pub fn clawbot(user_id: &str) -> Self {
        ImOrigin {
            channel: "clawbot".to_string(),
            peer: user_id.to_string(),
        }
    }
    /// QQ OneBot/NapCat 群消息：**一个群 = 一个会话**（与飞书群同语义）。
    /// 群客服要的正是这个——同群的人接着上文问，机器人记得住。
    pub fn onebot_group(group_id: i64) -> Self {
        ImOrigin {
            channel: "onebot".to_string(),
            peer: format!("g{group_id}"),
        }
    }
    /// QQ OneBot/NapCat 私聊：按对方 QQ 号分会话。
    ///
    /// `g`/`u` 前缀不能省：群号和 QQ 号同在一个 i64 空间里，不加前缀时「群 12345」
    /// 和「QQ 12345」会撞进同一个会话，两边的上下文互相污染。
    pub fn onebot_private(user_id: i64) -> Self {
        ImOrigin {
            channel: "onebot".to_string(),
            peer: format!("u{user_id}"),
        }
    }
    /// 绑定表的 key。
    fn key(&self) -> String {
        format!("{}:{}", self.channel, self.peer)
    }
    /// 未绑定时使用的默认会话 id——与历史行为逐字一致（`feishu-<chat_id>`），
    /// 保证老用户升级后聊天还落在原来那个会话里。
    pub fn default_sid(&self) -> String {
        format!("{}-{}", self.channel, self.peer)
    }
}

/// 一个 IM 聊天的绑定状态。
#[derive(Default, Clone, Serialize, Deserialize)]
struct Binding {
    /// 当前驱动的会话 id；None=用 `ImOrigin::default_sid()`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active: Option<String>,
    /// 上次 `/sessions` 列出的顺序（序号 → 会话 id）。`/switch <n>` 只认这份快照，
    /// 因此列表之后怎么重排都不会让用户切错会话。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    listing: Vec<String>,
}

/// 「最近活跃」时间戳（epoch 毫秒），进程内**严格单调递增**：同一毫秒内的连续活动也会
/// 拿到递增值。否则同毫秒的两条消息会打平，/sessions 的排序（及其序号）就不确定了。
fn touch_now() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static LAST: AtomicI64 = AtomicI64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let prev = LAST
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |p| Some(now.max(p + 1)))
        .unwrap_or(0);
    now.max(prev + 1)
}

/// 绑定表文件名。与会话文件同目录，但加载会话时会显式跳过它（见 `with_dir`），
/// 而不是依赖「解析成 PersistedSession 会失败」这种隐式行为。
const BINDINGS_FILE: &str = "im_bindings.json";

fn bindings_path_for(dir: &std::path::Path) -> PathBuf {
    dir.join(BINDINGS_FILE)
}

struct Entry {
    session: Session,
    tx: broadcast::Sender<ServerEvent>,
    /// 该 session 的对话历史（canonical 消息），多轮上下文用。
    history: Vec<ChatMessage>,
    /// 该 session 绑定的知识库路径（目录/文件）；随会话持久化。
    knowledge: Vec<String>,
    /// 任务级配置（模型/技能/auto-approve/工作目录）；随会话持久化。
    config: TaskConfig,
}

/// 落盘格式：session 元数据 + 历史 + 知识库绑定 + 任务配置。
#[derive(Serialize, Deserialize)]
struct PersistedSession {
    session: Session,
    #[serde(default)]
    history: Vec<ChatMessage>,
    #[serde(default)]
    knowledge: Vec<String>,
    #[serde(default)]
    config: TaskConfig,
}

/// 排队中的用户消息（会话工作中发来的，待当前回合结束后按 FIFO 自动续跑）。
#[derive(Clone)]
pub struct QueuedMessage {
    pub content: String,
    pub images: Vec<String>,
    pub files: Vec<Value>,
    pub cwd: Option<String>,
    /// true=重试（对现有历史重跑，不追加用户消息）；false=普通新消息。
    pub retry: bool,
}

/// 每会话的排队状态：待处理消息 + 是否已有「抽干循环」在跑。
#[derive(Default)]
struct PendingState {
    queue: VecDeque<QueuedMessage>,
    draining: bool,
}

/// 可克隆的注册表句柄（内部 Arc<Mutex>）。
#[derive(Clone)]
pub struct SessionRegistry {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
    /// 待回应的确认请求：conf_id → oneshot 发送端。
    confirms: Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
    /// 运行中的 agent 任务：session_id → AbortHandle（用于中断）。
    running: Arc<Mutex<HashMap<String, tokio::task::AbortHandle>>>,
    /// 每会话回合串行锁：session_id → async Mutex。持锁期间该会话独占一轮，杜绝并发回合交错写历史。
    locks: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    /// 每会话「工作中发来的消息」队列：session_id → 待处理消息 + 抽干标记。
    pending: Arc<Mutex<HashMap<String, PendingState>>>,
    /// 全局侧栏 feed：承载轻量生命周期事件（状态/重命名/删除），每个连接都订阅。
    /// 与 per-session 频道分工：重流式事件仍只走 per-session、发给当前订阅的对话。
    global_tx: broadcast::Sender<ServerEvent>,
    /// 持久化目录；None=纯内存（测试用）。
    data_dir: Option<PathBuf>,
    /// IM 聊天 → 当前会话 + 序号快照。key = `ImOrigin::key()`。
    bindings: Arc<Mutex<HashMap<String, Binding>>>,
    /// 绑定表落盘路径（放在 sessions 目录**旁边**，避免被当成一个 session 文件去解析）。
    bindings_path: Option<PathBuf>,
}

impl Default for SessionRegistry {
    fn default() -> Self {
        let (global_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        SessionRegistry {
            inner: Arc::new(Mutex::new(HashMap::new())),
            confirms: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(Mutex::new(HashMap::new())),
            locks: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            global_tx,
            data_dir: None,
            bindings: Arc::new(Mutex::new(HashMap::new())),
            bindings_path: None,
        }
    }
}

impl SessionRegistry {
    /// 纯内存注册表（不落盘）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 带持久化的注册表：从 `dir` 载入已有会话，后续变更写回该目录。
    pub fn with_dir(dir: PathBuf) -> Self {
        let mut map: HashMap<String, Entry> = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let path = e.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                // 绑定表与会话文件同目录，但它不是会话——显式跳过，别去当 session 解析。
                if path.file_name().and_then(|s| s.to_str()) == Some(BINDINGS_FILE) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(p) = serde_json::from_str::<PersistedSession>(&text) else {
                    continue;
                };
                let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
                let mut session = p.session;
                // 重启后内存里没有任何运行中的任务（running 句柄表是空的）。磁盘上残留的
                // "working" 是上次进程被杀时卡住的旧状态——若原样恢复，UI 会一直转圈，且
                // 「停止」找不到可中断的句柄而无效。启动即把它归位成 idle。
                if session.status.as_deref() == Some("working") {
                    session.status = Some("idle".to_string());
                }
                // 老会话文件没有 updated_at（这个字段是后加的）。用文件 mtime 回填，
                // 否则它们全是 None、在 /sessions 里挤在末尾按 id 乱序，第一次列表就没法看。
                if session.updated_at.is_none() {
                    session.updated_at = std::fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as i64);
                }
                map.insert(
                    session.id.clone(),
                    Entry {
                        session,
                        tx,
                        history: p.history,
                        knowledge: p.knowledge,
                        config: p.config,
                    },
                );
            }
        }
        let bindings_path = bindings_path_for(&dir);
        let bindings = std::fs::read_to_string(&bindings_path)
            .ok()
            .and_then(|t| serde_json::from_str::<HashMap<String, Binding>>(&t).ok())
            .unwrap_or_default();
        let (global_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        SessionRegistry {
            inner: Arc::new(Mutex::new(map)),
            confirms: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(Mutex::new(HashMap::new())),
            locks: Arc::new(Mutex::new(HashMap::new())),
            pending: Arc::new(Mutex::new(HashMap::new())),
            global_tx,
            data_dir: Some(dir),
            bindings: Arc::new(Mutex::new(bindings)),
            bindings_path: Some(bindings_path),
        }
    }

    /// 记录某 session 正在运行的任务句柄（用于中断）。
    pub fn set_running(&self, id: &str, handle: tokio::task::AbortHandle) {
        self.running.lock().unwrap().insert(id.to_string(), handle);
    }

    /// 任务正常结束后清除句柄。
    pub fn clear_running(&self, id: &str) {
        self.running.lock().unwrap().remove(id);
    }

    /// 中断运行中的任务，返回是否确实中断了某任务。
    /// 「停止」语义：连同排队中的消息一并清空、复位抽干标记（否则被 abort 的抽干循环不会复位，
    /// 队列会卡死、后续消息再也抽不动）。
    pub fn interrupt(&self, id: &str) -> bool {
        self.pending.lock().unwrap().remove(id);
        if let Some(h) = self.running.lock().unwrap().remove(id) {
            h.abort();
            true
        } else {
            false
        }
    }

    /// 入队一条「工作中发来的消息」。返回 true 表示当前空闲、应由调用方启动「抽干循环」
    /// （按 FIFO 取出并逐条 run_turn）；false 表示已有回合 / 抽干在跑，消息排在其后，稍后自动续跑。
    pub fn enqueue_message(&self, id: &str, msg: QueuedMessage) -> bool {
        let mut p = self.pending.lock().unwrap();
        let st = p.entry(id.to_string()).or_default();
        st.queue.push_back(msg);
        if st.draining {
            false
        } else {
            st.draining = true;
            true
        }
    }

    /// 取下一条排队消息；队列空则复位抽干标记并返回 None（判空与复位在同一把锁内原子完成，
    /// 避免「判空→复位」之间新消息插入导致漏抽）。
    pub fn dequeue_message(&self, id: &str) -> Option<QueuedMessage> {
        let mut p = self.pending.lock().unwrap();
        let st = p.get_mut(id)?;
        if let Some(m) = st.queue.pop_front() {
            Some(m)
        } else {
            st.draining = false;
            None
        }
    }

    /// 抽出可以**就地并入正在跑的回合**的排队消息（从队首连续取普通消息）。
    ///
    /// 用户在 agent 干活途中补充信息，多半正是为了纠偏；若等本回合跑完再当成新消息处理，
    /// 纠偏就晚了——模型已经沿着错误方向跑完一整轮，白烧一堆 token。所以正在跑的回合会在
    /// **每次 LLM 调用之前**把这些消息取走、追加进历史，模型下一轮就能看见。
    ///
    /// 遇到 `retry` 就停手：retry 的语义是「对现有历史重跑、不追加用户消息」，并不进历史，
    /// 只能由抽干循环单独跑；越过它去取后面的消息会打乱 FIFO 顺序。
    ///
    /// 取空队列时**不复位** `draining`：本回合还在跑，抽干循环仍归它管（由 `dequeue_message`
    /// 在回合结束后复位）。
    pub fn drain_injectable(&self, id: &str) -> Vec<QueuedMessage> {
        let mut p = self.pending.lock().unwrap();
        let Some(st) = p.get_mut(id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while st.queue.front().is_some_and(|m| !m.retry) {
            out.push(st.queue.pop_front().expect("刚确认过队首存在"));
        }
        out
    }

    /// 当前排队中的消息条数。
    pub fn pending_len(&self, id: &str) -> usize {
        self.pending
            .lock()
            .unwrap()
            .get(id)
            .map(|s| s.queue.len())
            .unwrap_or(0)
    }

    /// 某 session 的「回合串行锁」：持有其 guard 期间，该 session 的一轮处理独占。
    /// 用于杜绝并发回合（同会话同时来两条消息 / 触发）交错 append 历史——交错会让
    /// tool_use 与其 tool_result 错位，被上游以 400 拒绝整段历史、把会话「焊死」。
    pub fn session_lock(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.locks
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .clone()
    }

    /// 把某 entry 落盘（持久化关闭时为空操作）。
    fn persist_entry(&self, entry: &Entry) {
        let Some(dir) = &self.data_dir else {
            return;
        };
        let payload = PersistedSession {
            session: entry.session.clone(),
            history: entry.history.clone(),
            knowledge: entry.knowledge.clone(),
            config: entry.config.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&payload) {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(dir.join(format!("{}.json", entry.session.id)), json);
        }
    }

    /// 确保 session 存在，返回其 broadcast 发送端（用于订阅 / 发布）。
    pub fn ensure(&self, id: &str) -> broadcast::Sender<ServerEvent> {
        let mut map = self.inner.lock().unwrap();
        let entry = map.entry(id.to_string()).or_insert_with(|| {
            let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
            Entry {
                session: Session {
                    id: id.to_string(),
                    status: Some("idle".to_string()),
                    ..Default::default()
                },
                tx,
                history: Vec::new(),
                knowledge: Vec::new(),
                config: TaskConfig::default(),
            }
        });
        // 注意：不在此处对空会话落盘——空会话不持久化，避免侧边栏堆积无内容的会话。
        // 首条消息（append_message）或命名（set_name_if_empty）时才写盘。
        entry.tx.clone()
    }

    /// 取某 session 绑定的知识库路径。
    pub fn knowledge(&self, id: &str) -> Vec<String> {
        self.inner
            .lock()
            .unwrap()
            .get(id)
            .map(|e| e.knowledge.clone())
            .unwrap_or_default()
    }

    /// 设置某 session 的知识库路径并落盘（确保 session 存在）。
    pub fn set_knowledge(&self, id: &str, paths: Vec<String>) {
        self.ensure(id);
        if let Some(entry) = self.inner.lock().unwrap().get_mut(id) {
            entry.knowledge = paths;
            self.persist_entry(entry);
        }
    }

    /// 取某 session 的任务配置（模型/技能/auto-approve/工作目录）。
    pub fn task_config(&self, id: &str) -> TaskConfig {
        self.inner
            .lock()
            .unwrap()
            .get(id)
            .map(|e| e.config.clone())
            .unwrap_or_default()
    }

    /// 设置某 session 的任务配置并落盘（确保 session 存在）。
    /// 同时把 working_dir 镜像进 session.extra，使侧栏的 session_list/快照能据此分组到「工作空间」。
    /// 并把更新后的快照发到全局侧栏 feed，让其它连接的侧栏即时重新分组。
    pub fn set_task_config(&self, id: &str, config: TaskConfig) {
        self.ensure(id);
        let snapshot = {
            let mut map = self.inner.lock().unwrap();
            map.get_mut(id).map(|entry| {
                match config
                    .working_dir
                    .as_deref()
                    .map(str::trim)
                    .filter(|w| !w.is_empty())
                {
                    Some(w) => {
                        entry
                            .session
                            .extra
                            .insert("working_dir".into(), serde_json::Value::from(w));
                    }
                    None => {
                        entry.session.extra.remove("working_dir");
                    }
                }
                entry.config = config;
                self.persist_entry(entry);
                entry.session.clone()
            })
        };
        if let Some(s) = snapshot {
            self.publish_global(ServerEvent::snapshot_update(s));
        }
    }

    /// 订阅全局侧栏 feed（连接建立时调用）。
    pub fn subscribe_global(&self) -> broadcast::Receiver<ServerEvent> {
        self.global_tx.subscribe()
    }

    /// 向全局侧栏 feed 广播一个轻量生命周期事件。无订阅者时静默丢弃。
    pub fn publish_global(&self, ev: ServerEvent) {
        let _ = self.global_tx.send(ev);
    }

    /// 追加一条对话历史消息，并刷新「最近活跃」时间（/sessions 的排序依据）。
    pub fn append_message(&self, id: &str, msg: ChatMessage) {
        if let Some(entry) = self.inner.lock().unwrap().get_mut(id) {
            entry.history.push(msg);
            entry.session.updated_at = Some(touch_now());
            self.persist_entry(entry);
        }
    }

    /// 用新的消息列表替换该 session 的对话历史（上下文压缩后调用）。
    pub fn replace_history(&self, id: &str, messages: Vec<ChatMessage>) {
        if let Some(entry) = self.inner.lock().unwrap().get_mut(id) {
            entry.history = messages;
            self.persist_entry(entry);
        }
    }

    /// 取该 session 的对话历史副本。
    pub fn history(&self, id: &str) -> Vec<ChatMessage> {
        self.inner
            .lock()
            .unwrap()
            .get(id)
            .map(|e| e.history.clone())
            .unwrap_or_default()
    }

    pub fn exists(&self, id: &str) -> bool {
        self.inner.lock().unwrap().contains_key(id)
    }

    /// 返回 session 快照。
    pub fn snapshot(&self, id: &str) -> Option<Session> {
        self.inner
            .lock()
            .unwrap()
            .get(id)
            .map(|e| e.session.clone())
    }

    /// 列出全部 session：**按最近活跃倒序**（/sessions 的序号依赖它）。
    /// 从未活跃过的（updated_at=None）排在最后，内部按 id 升序兜底，保证输出稳定。
    pub fn list(&self) -> Vec<Session> {
        let map = self.inner.lock().unwrap();
        let mut sessions: Vec<Session> = map.values().map(|e| e.session.clone()).collect();
        sessions.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        sessions
    }

    /// 把绑定表落盘（持久化关闭时为空操作）。
    fn persist_bindings(&self) {
        let Some(path) = &self.bindings_path else {
            return;
        };
        let map = self.bindings.lock().unwrap();
        if let Ok(json) = serde_json::to_string_pretty(&*map) {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, json);
        }
    }

    /// 某 IM 聊天当前绑定的会话 id。返回 None 表示「未绑定」或「绑定的会话已被删除」，
    /// 两种情况调用方都回落到 [`ImOrigin::default_sid`]——绝不能把消息发进一个不存在的会话。
    pub fn active_session(&self, origin: &ImOrigin) -> Option<String> {
        let sid = self
            .bindings
            .lock()
            .unwrap()
            .get(&origin.key())
            .and_then(|b| b.active.clone())?;
        self.exists(&sid).then_some(sid)
    }

    /// 把某 IM 聊天绑定到指定会话（/switch）。
    pub fn bind_session(&self, origin: &ImOrigin, sid: &str) {
        self.bindings
            .lock()
            .unwrap()
            .entry(origin.key())
            .or_default()
            .active = Some(sid.to_string());
        self.persist_bindings();
    }

    /// 记下本次 /sessions 列出的顺序（序号 → 会话 id），供随后的 /switch <n> 寻址。
    pub fn set_listing(&self, origin: &ImOrigin, sids: Vec<String>) {
        self.bindings
            .lock()
            .unwrap()
            .entry(origin.key())
            .or_default()
            .listing = sids;
        self.persist_bindings();
    }

    /// 取上次 /sessions 的序号快照；空=还没列过。
    pub fn listing(&self, origin: &ImOrigin) -> Vec<String> {
        self.bindings
            .lock()
            .unwrap()
            .get(&origin.key())
            .map(|b| b.listing.clone())
            .unwrap_or_default()
    }

    /// 会话无名时设置名称，返回是否设置了（用于触发 session_renamed）。
    /// 设置成功时同时向全局侧栏 feed 广播 session_renamed，让其它连接的侧栏即时刷新标题。
    pub fn set_name_if_empty(&self, id: &str, name: &str) -> bool {
        let set = {
            let mut map = self.inner.lock().unwrap();
            if let Some(entry) = map.get_mut(id) {
                let empty = entry
                    .session
                    .name
                    .as_deref()
                    .map(str::is_empty)
                    .unwrap_or(true);
                if empty {
                    entry.session.name = Some(name.to_string());
                    self.persist_entry(entry);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if set {
            self.publish_global(ServerEvent::SessionRenamed {
                session_id: id.to_string(),
                name: name.to_string(),
            });
        }
        set
    }

    /// 用户主动重命名会话：覆盖已有名称（区别于 set_name_if_empty 的仅首次命名）。
    /// 空白名 / 不存在的会话拒绝并返回 false；成功时落盘并广播 session_renamed 同步各端侧栏。
    pub fn rename_session(&self, id: &str, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let set = {
            let mut map = self.inner.lock().unwrap();
            if let Some(entry) = map.get_mut(id) {
                entry.session.name = Some(name.to_string());
                self.persist_entry(entry);
                true
            } else {
                false
            }
        };
        if set {
            self.publish_global(ServerEvent::SessionRenamed {
                session_id: id.to_string(),
                name: name.to_string(),
            });
        }
        set
    }

    /// 删除会话（内存 + 磁盘文件），并向全局侧栏 feed 广播 session_deleted。
    pub fn remove_session(&self, id: &str) {
        self.inner.lock().unwrap().remove(id);
        if let Some(dir) = &self.data_dir {
            let _ = std::fs::remove_file(dir.join(format!("{id}.json")));
        }
        self.publish_global(ServerEvent::SessionDeleted {
            session_id: id.to_string(),
        });
    }

    /// 更新 session 状态，并向全局侧栏 feed 广播状态变化（让后台任务的运行态在侧栏实时更新）。
    pub fn set_status(&self, id: &str, status: &str) {
        let changed = {
            let mut map = self.inner.lock().unwrap();
            if let Some(entry) = map.get_mut(id) {
                entry.session.status = Some(status.to_string());
                self.persist_entry(entry);
                true
            } else {
                false
            }
        };
        if changed {
            self.publish_global(ServerEvent::status_update(id, status));
        }
    }

    /// 登记一个待确认请求，返回接收端（agent await 它）。
    pub fn register_confirmation(&self, id: String) -> oneshot::Receiver<String> {
        let (tx, rx) = oneshot::channel();
        self.confirms.lock().unwrap().insert(id, tx);
        rx
    }

    /// 用收到的结果回应某确认请求（WS confirmation 消息触发）。
    pub fn resolve_confirmation(&self, id: &str, result: String) {
        if let Some(tx) = self.confirms.lock().unwrap().remove(id) {
            let _ = tx.send(result);
        }
    }

    /// 向某 session 的全部订阅者广播事件。无订阅者时静默丢弃。
    pub fn publish(&self, id: &str, ev: ServerEvent) {
        let map = self.inner.lock().unwrap();
        if let Some(entry) = map.get(id) {
            let _ = entry.tx.send(ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_creates_idle_session() {
        let reg = SessionRegistry::new();
        reg.ensure("s1");
        assert!(reg.exists("s1"));
        assert_eq!(reg.snapshot("s1").unwrap().status.as_deref(), Some("idle"));
    }

    #[test]
    fn list_falls_back_to_id_order_for_sessions_with_no_activity() {
        // 从没收过消息的会话没有 updated_at，按 id 升序兜底（稳定输出）。
        let reg = SessionRegistry::new();
        reg.ensure("b");
        reg.ensure("a");
        let ids: Vec<String> = reg.list().into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn list_sorts_by_most_recent_activity_first() {
        // /sessions 的序号依赖这个顺序：最近活跃的排最前。
        let reg = SessionRegistry::new();
        for id in ["a", "b", "c"] {
            reg.ensure(id);
        }
        reg.append_message("a", ChatMessage::user("1"));
        reg.append_message("b", ChatMessage::user("2"));
        reg.append_message("c", ChatMessage::user("3"));
        // 再动一次 a：它应回到最前（即便与 c 落在同一毫秒，也必须严格排在前面）。
        reg.append_message("a", ChatMessage::user("4"));
        let ids: Vec<String> = reg.list().into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["a".to_string(), "c".to_string(), "b".to_string()]);
        // 有活跃记录的会话一律排在从未活跃过的之前。
        reg.ensure("z");
        let ids: Vec<String> = reg.list().into_iter().map(|s| s.id).collect();
        assert_eq!(ids.last().unwrap(), "z");
    }

    #[test]
    fn im_binding_tracks_active_session_and_listing_snapshot() {
        let reg = SessionRegistry::new();
        let origin = ImOrigin::feishu("oc_1");
        // 默认未绑定 → 调用方回落到默认 sid（保持现有行为不变）。
        assert!(reg.active_session(&origin).is_none());
        assert!(reg.listing(&origin).is_empty());
        assert_eq!(origin.default_sid(), "feishu-oc_1");

        // 绑定到一个已存在的会话（比如桌面 Web 上建的任务）。
        reg.ensure("web-1");
        reg.bind_session(&origin, "web-1");
        assert_eq!(reg.active_session(&origin).as_deref(), Some("web-1"));

        // /sessions 的序号快照可存取，供 /switch <n> 寻址。
        reg.set_listing(&origin, vec!["web-1".into(), "feishu-oc_1".into()]);
        assert_eq!(
            reg.listing(&origin),
            vec!["web-1".to_string(), "feishu-oc_1".to_string()]
        );

        // 绑定的会话被删掉 → active 必须回落 None，否则 IM 会往一个不存在的会话里发消息。
        reg.remove_session("web-1");
        assert!(
            reg.active_session(&origin).is_none(),
            "已删除的会话不应继续被绑定"
        );
    }

    #[test]
    fn onebot_group_and_private_never_share_a_session() {
        // 群号与 QQ 号取值空间重叠，靠 g/u 前缀区分。少了前缀，群 12345 里的客服对话
        // 会和 QQ 12345 的私聊落进同一个会话，上下文互相串味。
        let g = ImOrigin::onebot_group(12345);
        let u = ImOrigin::onebot_private(12345);
        assert_eq!(g.default_sid(), "onebot-g12345");
        assert_eq!(u.default_sid(), "onebot-u12345");
        assert_ne!(g, u);
        // 不同群之间也必须各自独立。
        assert_ne!(
            ImOrigin::onebot_group(1).default_sid(),
            ImOrigin::onebot_group(2).default_sid()
        );
    }

    #[test]
    fn im_bindings_persist_across_restart() {
        let dir = std::env::temp_dir().join(format!("wc-bind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let origin = ImOrigin::feishu("oc_9");
        {
            let reg = SessionRegistry::with_dir(dir.clone());
            reg.ensure("web-42");
            reg.append_message("web-42", ChatMessage::user("hi")); // 非空才落盘
            reg.bind_session(&origin, "web-42");
            reg.set_listing(&origin, vec!["web-42".into()]);
        }
        // 重启：绑定与快照都应从磁盘恢复，否则一重启就切回默认会话。
        let reg2 = SessionRegistry::with_dir(dir.clone());
        assert_eq!(reg2.active_session(&origin).as_deref(), Some("web-42"));
        assert_eq!(reg2.listing(&origin), vec!["web-42".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_status_updates_snapshot() {
        let reg = SessionRegistry::new();
        reg.ensure("s1");
        reg.set_status("s1", "working");
        assert_eq!(
            reg.snapshot("s1").unwrap().status.as_deref(),
            Some("working")
        );
    }

    #[test]
    fn persists_and_reloads_history() {
        use wisecortex_core::llm::ChatMessage;
        let dir = std::env::temp_dir().join(format!("wc-sess-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();

        {
            let reg = SessionRegistry::with_dir(dir.clone());
            reg.ensure("s1");
            reg.append_message("s1", ChatMessage::user("hello"));
            reg.append_message("s1", ChatMessage::assistant("hi there"));
            reg.set_status("s1", "idle");
        }
        // 指向同目录的新注册表应恢复历史。
        let reloaded = SessionRegistry::with_dir(dir.clone());
        assert!(reloaded.exists("s1"));
        let hist = reloaded.history("s1");
        assert_eq!(hist.len(), 2);
        assert_eq!(hist[0].content.as_deref(), Some("hello"));
        assert_eq!(hist[1].content.as_deref(), Some("hi there"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reloads_stale_working_status_as_idle() {
        use wisecortex_core::llm::ChatMessage;
        let dir = std::env::temp_dir().join(format!("wc-sess-working-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        {
            let reg = SessionRegistry::with_dir(dir.clone());
            reg.ensure("s1");
            reg.append_message("s1", ChatMessage::user("hi")); // 让会话落盘
            reg.set_status("s1", "working"); // 模拟进程被杀时卡在 working
            assert_eq!(
                reg.snapshot("s1").unwrap().status.as_deref(),
                Some("working")
            );
        }
        // 重启：磁盘上残留的 working 没有对应运行中的任务，应被归位成 idle。
        let reloaded = SessionRegistry::with_dir(dir.clone());
        assert_eq!(
            reloaded.snapshot("s1").unwrap().status.as_deref(),
            Some("idle")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn persists_and_reloads_task_config() {
        let dir = std::env::temp_dir().join(format!("wc-sess-cfg-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        {
            let reg = SessionRegistry::with_dir(dir.clone());
            reg.set_task_config(
                "s1",
                TaskConfig {
                    working_dir: Some("D:/work/mud".into()),
                    model_id: Some("llm-2".into()),
                    skills: vec!["tdd".into(), "code-reviewer".into()],
                    auto_approve: Some(false),
                    plan_mode: true,
                    reasoning_effort: Some("xhigh".into()),
                },
            );
        }
        let reloaded = SessionRegistry::with_dir(dir.clone());
        let cfg = reloaded.task_config("s1");
        assert_eq!(cfg.working_dir.as_deref(), Some("D:/work/mud"));
        assert_eq!(cfg.model_id.as_deref(), Some("llm-2"));
        assert_eq!(cfg.skills, vec!["tdd".to_string(), "code-reviewer".into()]);
        assert_eq!(cfg.auto_approve, Some(false));
        assert!(cfg.plan_mode);
        assert_eq!(cfg.reasoning_effort.as_deref(), Some("xhigh"));
        // 未设配置的会话返回默认（全空）。
        assert_eq!(reloaded.task_config("nope"), TaskConfig::default());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn global_feed_receives_status_rename_delete() {
        let reg = SessionRegistry::new();
        let mut rx = reg.subscribe_global();
        reg.ensure("s1");
        reg.set_status("s1", "working");
        reg.set_name_if_empty("s1", "做个 mud");
        reg.remove_session("s1");

        // 三个轻量生命周期事件应依次到达全局 feed。
        let mut kinds = Vec::new();
        for _ in 0..3 {
            match rx.recv().await.unwrap() {
                ServerEvent::SessionUpdate { status, .. } => {
                    kinds.push(format!("status:{status:?}"))
                }
                ServerEvent::SessionRenamed { name, .. } => kinds.push(format!("renamed:{name}")),
                ServerEvent::SessionDeleted { .. } => kinds.push("deleted".into()),
                _ => kinds.push("other".into()),
            }
        }
        assert_eq!(
            kinds,
            vec![
                "status:Some(\"working\")".to_string(),
                "renamed:做个 mud".into(),
                "deleted".into(),
            ]
        );
    }

    #[tokio::test]
    async fn rename_session_overwrites_name_and_broadcasts() {
        let reg = SessionRegistry::new();
        reg.ensure("s1");
        reg.set_name_if_empty("s1", "自动生成的很长看不出干嘛的名字");
        let mut rx = reg.subscribe_global();

        assert!(reg.rename_session("s1", "  修 MUD 战斗系统  "));
        assert_eq!(
            reg.snapshot("s1").unwrap().name.as_deref(),
            Some("修 MUD 战斗系统"),
            "重命名应覆盖已有名称并去除首尾空白"
        );
        match rx.recv().await.unwrap() {
            ServerEvent::SessionRenamed { session_id, name } => {
                assert_eq!(session_id, "s1");
                assert_eq!(name, "修 MUD 战斗系统");
            }
            other => panic!("期望 SessionRenamed，实际 {other:?}"),
        }

        // 空白名 / 不存在的会话：拒绝且不广播。
        assert!(!reg.rename_session("s1", "   "));
        assert!(!reg.rename_session("nope", "x"));
        assert!(rx.try_recv().is_err(), "拒绝的重命名不应广播事件");
    }

    #[test]
    fn rename_session_persists_new_name() {
        use wisecortex_core::llm::ChatMessage;
        let dir = std::env::temp_dir().join(format!("wc-sess-ren-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        {
            let reg = SessionRegistry::with_dir(dir.clone());
            reg.ensure("s1");
            reg.append_message("s1", ChatMessage::user("hi"));
            reg.set_name_if_empty("s1", "旧名");
            assert!(reg.rename_session("s1", "新名"));
        }
        let reloaded = SessionRegistry::with_dir(dir.clone());
        assert_eq!(
            reloaded.snapshot("s1").unwrap().name.as_deref(),
            Some("新名")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_session_is_not_persisted_until_it_has_content() {
        use wisecortex_core::llm::ChatMessage;
        let dir = std::env::temp_dir().join(format!("wc-sess-empty-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();

        {
            let reg = SessionRegistry::with_dir(dir.clone());
            reg.ensure("empty"); // 仅创建、无内容 → 不应落盘
        }
        let reloaded = SessionRegistry::with_dir(dir.clone());
        assert!(!reloaded.exists("empty"), "空会话不应被持久化");

        // 一旦有了首条消息，就应落盘并能恢复。
        {
            let reg = SessionRegistry::with_dir(dir.clone());
            reg.ensure("named");
            reg.append_message("named", ChatMessage::user("hi"));
        }
        assert!(SessionRegistry::with_dir(dir.clone()).exists("named"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn session_lock_serializes_concurrent_turns() {
        use wisecortex_core::llm::ChatMessage;
        // 两个并发「回合」各自 append 两条消息，中间让出调度。持有同一 session 锁时，
        // 两块必须各自连续、不得交错（否则 tool_use/tool_result 会错位）。
        let reg = SessionRegistry::new();
        reg.ensure("s");

        let spawn_turn = |reg: SessionRegistry, a: &'static str, b: &'static str| {
            tokio::spawn(async move {
                let _guard = reg.session_lock("s").lock_owned().await;
                reg.append_message("s", ChatMessage::user(a));
                tokio::task::yield_now().await;
                reg.append_message("s", ChatMessage::user(b));
            })
        };
        let t1 = spawn_turn(reg.clone(), "A1", "A2");
        let t2 = spawn_turn(reg.clone(), "B1", "B2");
        let _ = tokio::join!(t1, t2);

        let texts: Vec<String> = reg
            .history("s")
            .into_iter()
            .filter_map(|m| m.content)
            .collect();
        assert!(
            texts == ["A1", "A2", "B1", "B2"] || texts == ["B1", "B2", "A1", "A2"],
            "并发回合不应交错: {texts:?}"
        );
    }

    fn qmsg(text: &str) -> QueuedMessage {
        QueuedMessage {
            content: text.into(),
            images: vec![],
            files: vec![],
            cwd: None,
            retry: false,
        }
    }

    #[test]
    fn enqueue_first_starts_drain_then_queues_and_drains_fifo() {
        let reg = SessionRegistry::new();
        reg.ensure("s");
        // 空闲时第一条 → 应由调用方启动抽干循环。
        assert!(reg.enqueue_message("s", qmsg("m1")), "首条应触发抽干");
        // 抽干中再来 → 排队，不再触发抽干。
        assert!(!reg.enqueue_message("s", qmsg("m2")), "抽干中应只排队");
        assert_eq!(reg.pending_len("s"), 2);
        // 按 FIFO 抽干。
        assert_eq!(
            reg.dequeue_message("s").map(|m| m.content),
            Some("m1".into())
        );
        assert_eq!(
            reg.dequeue_message("s").map(|m| m.content),
            Some("m2".into())
        );
        assert_eq!(reg.pending_len("s"), 0);
        // 队列空 → 复位 draining 并返回 None。
        assert!(reg.dequeue_message("s").is_none());
        // 复位后新消息应能重新触发抽干。
        assert!(
            reg.enqueue_message("s", qmsg("m3")),
            "复位后应能重新触发抽干"
        );
    }

    #[test]
    fn drain_injectable_takes_messages_that_arrived_while_the_turn_is_running() {
        let reg = SessionRegistry::new();
        reg.ensure("s");
        // 抽干循环取走 m1 并开始跑它这一回合。
        assert!(reg.enqueue_message("s", qmsg("m1")));
        assert_eq!(
            reg.dequeue_message("s").map(|m| m.content),
            Some("m1".into())
        );
        // 干活途中用户补充了两条 —— 正在跑的回合应能就地取走，不必等它跑完。
        assert!(!reg.enqueue_message("s", qmsg("补充1")));
        assert!(!reg.enqueue_message("s", qmsg("补充2")));
        let got: Vec<String> = reg
            .drain_injectable("s")
            .into_iter()
            .map(|m| m.content)
            .collect();
        assert_eq!(got, vec!["补充1", "补充2"], "应按 FIFO 全部取走");
        assert_eq!(reg.pending_len("s"), 0);
        // 队列空但回合还在跑 → 不能复位 draining，否则新消息会另起一条抽干循环、并发跑同一会话。
        assert!(
            !reg.enqueue_message("s", qmsg("补充3")),
            "回合仍在跑，新消息只应排队"
        );
    }

    #[test]
    fn drain_injectable_stops_at_retry_and_leaves_it_to_the_drain_loop() {
        // retry 是「对现有历史重跑、不追加用户消息」，并不进历史，不能并入正在跑的回合；
        // 越过它去取后面的消息还会打乱 FIFO。
        let reg = SessionRegistry::new();
        reg.ensure("s");
        reg.enqueue_message("s", qmsg("补充1"));
        reg.enqueue_message(
            "s",
            QueuedMessage {
                retry: true,
                ..qmsg("重跑")
            },
        );
        reg.enqueue_message("s", qmsg("补充2"));
        let got: Vec<String> = reg
            .drain_injectable("s")
            .into_iter()
            .map(|m| m.content)
            .collect();
        assert_eq!(got, vec!["补充1"], "遇到 retry 就停手");
        assert_eq!(reg.pending_len("s"), 2, "retry 及其之后的消息留给抽干循环");
    }

    #[test]
    fn interrupt_clears_pending_queue() {
        let reg = SessionRegistry::new();
        reg.ensure("s");
        assert!(reg.enqueue_message("s", qmsg("m1")));
        assert!(!reg.enqueue_message("s", qmsg("m2")));
        assert_eq!(reg.pending_len("s"), 2);
        reg.interrupt("s"); // 停止应连排队消息一并清空、复位 draining。
        assert_eq!(reg.pending_len("s"), 0);
        assert!(
            reg.enqueue_message("s", qmsg("m3")),
            "停止后应能重新触发抽干"
        );
    }
}
