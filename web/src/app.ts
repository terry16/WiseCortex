// ── app — 启动装配 ─────────────────────────────────────────────────────────
//
// 把 WS 传输层(ws.ts) + 事件路由(ws-dispatcher.ts) + 渲染(sessions.ts) + DOM 装配起来。
// MVP：单 session 对话，发消息 → 服务端回声 → 渲染。
// ──────────────────────────────────────────────────────────────────────────

import "./style.css";
import { mountArtifact } from "./artifact";
import { authHeaders, createWsAuth, getKey, setKey } from "./auth";
import { httpBase, wsBase } from "./backend";
import { mountChannelsView } from "./channelsView";
import { type MessageKey, initI18n, t } from "./i18n";
import { icon } from "./icons";
import { mountJobsView } from "./jobsView";
import { installExternalLinkHandler, pickDirectory } from "./platform";
import { Sessions } from "./sessions";
import { mountSettingsView } from "./settingsView";
import { mountSkillsView } from "./skillsView";
import { mountTasksView } from "./tasksView";
import { type WsClient, type WsMessage, createWsClient } from "./ws";
import { createDispatcher } from "./ws-dispatcher";

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

// 生成会话 ID。crypto.randomUUID 仅在安全上下文（HTTPS / localhost）可用——纯 HTTP 部署
// （如 http://<公网IP>）下它是 undefined，直接调用会抛错，导致新建会话/点技能·知识库无反应。
// 故降级：优先 randomUUID，其次用非安全上下文也可用的 getRandomValues 拼 UUIDv4，最后兜底。
function genId(): string {
  const c = globalThis.crypto as Crypto | undefined;
  if (c?.randomUUID) return c.randomUUID();
  if (c?.getRandomValues) {
    const b = c.getRandomValues(new Uint8Array(16));
    b[6] = (b[6] & 0x0f) | 0x40; // version 4
    b[8] = (b[8] & 0x3f) | 0x80; // variant
    const h = Array.from(b, (x) => x.toString(16).padStart(2, "0"));
    return `${h.slice(0, 4).join("")}-${h.slice(4, 6).join("")}-${h.slice(6, 8).join("")}-${h.slice(8, 10).join("")}-${h.slice(10, 16).join("")}`;
  }
  return `id-${Date.now()}-${Math.random().toString(16).slice(2, 10)}`;
}

// ── 登录门（access_key）─────────────────────────────────────────────────────
// 服务端启用 access_key 时，未授权不得进入主界面；未启用则直接放行（不显示登录页）。
type AuthStatus = { required: boolean; authorized: boolean };
async function fetchAuthStatus(apiBase: string, key: string | null): Promise<AuthStatus> {
  const headers: Record<string, string> = key ? { "x-access-key": key } : {};
  const r = await fetch(`${apiBase}/api/auth/status`, { headers });
  return (await r.json()) as AuthStatus;
}
async function ensureAuthorized(apiBase: string): Promise<void> {
  let st: AuthStatus;
  try {
    st = await fetchAuthStatus(apiBase, getKey());
  } catch {
    return; // 后端不可达：不拦，主界面照常加载（离线横幅会提示连接断开）
  }
  if (!st.required || st.authorized) return; // 未启用密钥，或已授权 → 放行
  await showLoginGate(apiBase);
}
function showLoginGate(apiBase: string): Promise<void> {
  return new Promise((resolve) => {
    const gate = document.createElement("div");
    gate.className = "login-gate";
    gate.innerHTML = `
      <form class="login-card">
        <div class="login-logo"></div>
        <h1 class="login-title">WiseCortex</h1>
        <p class="login-sub">${t("login.sub")}</p>
        <input type="password" class="login-input" placeholder="${t("login.placeholder")}" autocomplete="current-password" />
        <button type="submit" class="login-btn">${t("login.enter")}</button>
        <div class="login-err" hidden></div>
      </form>`;
    document.body.appendChild(gate);
    const form = gate.querySelector(".login-card") as HTMLFormElement;
    const inp = gate.querySelector(".login-input") as HTMLInputElement;
    const btn = gate.querySelector(".login-btn") as HTMLButtonElement;
    const err = gate.querySelector(".login-err") as HTMLElement;
    inp.focus();
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      const key = inp.value.trim();
      if (!key) return;
      btn.disabled = true;
      err.hidden = true;
      void (async () => {
        try {
          const st = await fetchAuthStatus(apiBase, key);
          if (st.authorized) {
            setKey(key);
            gate.remove();
            resolve();
            return;
          }
          err.textContent = t("login.err.wrong");
        } catch {
          err.textContent = t("login.err.unreachable");
        }
        err.hidden = false;
        btn.disabled = false;
        inp.select();
      })();
    });
  });
}

function $(id: string): HTMLElement {
  const el = document.getElementById(id);
  if (!el) throw new Error(`missing element #${id}`);
  return el;
}

/** icon-only 功能入口的钉选数量小角标：>0 显示右上角徽标，=0 清除。 */
function setPillCount(pill: HTMLElement, n: number): void {
  pill.classList.toggle("set", n > 0);
  pill.classList.toggle("has-count", n > 0);
  if (n > 0) pill.dataset.count = String(n);
  else delete pill.dataset.count;
}

/** 把所有 [data-icon] 占位元素填充为内联 SVG。 */
function paintIcons(root: ParentNode = document): void {
  for (const el of root.querySelectorAll<HTMLElement>("[data-icon]")) {
    const name = el.dataset.icon;
    if (!name || el.dataset.painted) continue;
    el.innerHTML = icon(name, el.classList.contains("brand-mark") ? 19 : 16);
    el.dataset.painted = "1";
  }
}

/** 主题（浅=瓷白 / 暗=墨青）：初始值由 index.html 内联脚本按 localStorage / 系统偏好落好，
    这里只接管切换。图标显示「将切到的模式」：浅色下显示月亮，暗色下显示太阳。 */
function initTheme(): void {
  const btn = document.getElementById("theme-toggle");
  if (!btn) return;
  const slot = btn.querySelector<HTMLElement>("[data-icon]");
  const apply = (): void => {
    if (!slot) return;
    const dark = document.documentElement.dataset.theme === "dark";
    // 图标显示「将切到的模式」：浅色下显示月亮，暗色下显示太阳。
    slot.dataset.icon = dark ? "sun" : "moon";
    delete slot.dataset.painted;
    paintIcons(btn);
  };
  apply();
  btn.addEventListener("click", () => {
    const next = document.documentElement.dataset.theme === "dark" ? "light" : "dark";
    document.documentElement.dataset.theme = next;
    try {
      localStorage.setItem("wc-theme", next);
    } catch {
      /* 隐私模式等场景存不了，降级为仅当前会话生效 */
    }
    apply();
  });
}

export async function bootstrap(): Promise<void> {
  await initI18n(); // 探测语言（桌面端含系统语言）+ 翻译静态 HTML（data-i18n / data-i18n-attr-*）
  const apiBase = httpBase();
  // 登录门：服务端启用了 access_key 且当前未授权时，先拦一道登录页（输对密钥才进主界面）。
  await ensureAuthorized(apiBase);

  paintIcons();
  initTheme();
  const messages = $("messages");
  const input = $("user-input") as HTMLTextAreaElement;
  const sendBtn = $("btn-send") as HTMLButtonElement;
  const banner = $("offline-banner");

  // 左栏常驻任务面板（#task-panel），承载「任务 / 工作空间」两段列表。
  const sidebar = $("task-panel");

  async function loadHistory(id: string): Promise<void> {
    try {
      const r = await fetch(`${apiBase}/api/sessions/${id}/messages`, { headers: authHeaders() });
      const data = await r.json();
      sessions.renderHistory(data.messages ?? []);
    } catch {
      sessions.clearChat();
    }
  }

  function switchSession(id: string): void {
    sessions.setActive(id);
    ws.setSubscribedSession(id);
    ws.send({ type: "subscribe", session_id: id });
    void loadHistory(id);
    void loadKnowledge(id); // 切换会话后从服务端加载该会话的知识库绑定
    void loadTaskConfig(id); // 加载该任务的配置（模型/技能/权限/工作目录）
  }

  function newSession(workingDir?: string): void {
    const id = genId();
    sessions.add({ id, status: "idle" });
    // 从工作空间目录 + 号新建：继承该目录作为工作目录（首条消息发出时随配置一并落库）。
    if (workingDir) cfgOf(id).working_dir = workingDir;
    switchSession(id);
  }

  async function deleteSession(id: string): Promise<void> {
    try {
      await fetch(`${apiBase}/api/sessions/${id}`, { method: "DELETE", headers: authHeaders() });
    } catch {
      /* 仍然本地移除 */
    }
    const wasActive = sessions.activeId === id;
    sessions.remove(id);
    if (wasActive) {
      const ids = sessions.ids();
      if (ids.length > 0) switchSession(ids[0]);
      else newSession();
    } else {
      sessions.renderList();
    }
  }

  const sessions = new Sessions({
    messages,
    sidebar,
    // 点任务行：切到对话视图并切换到该任务（setView 为函数声明，已提升，可在此引用）。
    onSwitch: (id) => {
      setView("chat");
      switchSession(id);
    },
    onDelete: (id) => void deleteSession(id),
    // 工作空间目录名上的 +：新建一个同工作目录的会话。
    onNewSession: (workingDir) => {
      setView("chat");
      newSession(workingDir);
    },
    // 本地已乐观更新；服务端成功后还会广播 session_renamed 同步其它端。
    onRename: (id, name) =>
      void fetch(`${apiBase}/api/sessions/${encodeURIComponent(id)}/name`, {
        method: "POST",
        headers: { "content-type": "application/json", ...authHeaders() },
        body: JSON.stringify({ name }),
      }).catch(() => {}),
  });

  // 开发期前端跑在 vite(5173)，后端在 7070；生产期由后端直接 serve，同源同端口。
  let wsRef: WsClient | null = null;
  const wsAuth = createWsAuth(() => {
    const k = window.prompt(t("auth.prompt"));
    if (k) {
      setKey(k);
      wsRef?.connect();
    }
  });
  const ws = createWsClient({
    auth: wsAuth,
    url: () => {
      const k = getKey();
      const base = `${wsBase()}/ws`;
      return k ? `${base}?access_key=${encodeURIComponent(k)}` : base;
    },
  });
  wsRef = ws;
  // 成功连上即视为通过鉴权（影响 1006 后是否提示输入密钥）。
  ws.onEvent((e) => {
    if (e.type === "_ws_connected") wsAuth.markPassed();
  });

  createDispatcher({
    ws,
    sessions,
    tasks: { load() {} },
    skills: { load() {} },
    router: { current: "session", restoreFromHash() {}, navigate() {} },
    i18n: {
      t: (k, p) => t(k as MessageKey, p as Record<string, string | number> | undefined),
    },
    ui: {
      setOffline: (off) => {
        banner.style.display = off ? "block" : "none";
      },
      enableSend: () => {
        sendBtn.disabled = false;
      },
      focusInput: () => input.focus(),
    },
    escapeHtml,
    showConfirmModal: (id, message) => {
      const ok = window.confirm(message);
      ws.send({ type: "confirmation", id, result: ok ? "yes" : "no" } as WsMessage);
    },
    // 成本以人民币元计（与 pricing.rs 一致），显示 ¥。
    billing: { getCurrencySymbol: () => "¥", convertCost: (n: number) => n },
  });

  // ── 附件：图片走 vision；其它文档（PDF 等）落到工作目录供 agent 用工具处理 ──
  const imgInput = $("img-input") as HTMLInputElement;
  const attachPreview = $("attach-preview");
  let pendingImages: string[] = [];
  let pendingFiles: { name: string; data_url: string }[] = [];
  const fileToDataUrl = (f: File): Promise<string> =>
    new Promise((res, rej) => {
      const r = new FileReader();
      r.onload = () => res(r.result as string);
      r.onerror = rej;
      r.readAsDataURL(f);
    });
  function renderAttachPreview(): void {
    attachPreview.replaceChildren();
    pendingImages.forEach((url, i) => {
      const img = document.createElement("img");
      img.src = url;
      img.className = "attach-thumb";
      img.title = t("attach.removeHint");
      img.addEventListener("click", () => {
        pendingImages.splice(i, 1);
        renderAttachPreview();
      });
      attachPreview.appendChild(img);
    });
    pendingFiles.forEach((f, i) => {
      const chip = document.createElement("button");
      chip.className = "attach-file";
      chip.title = t("attach.removeHint");
      chip.innerHTML = `${icon("doc", 14)}<span></span>`;
      (chip.querySelector("span") as HTMLElement).textContent = f.name;
      chip.addEventListener("click", () => {
        pendingFiles.splice(i, 1);
        renderAttachPreview();
      });
      attachPreview.appendChild(chip);
    });
    attachPreview.style.display =
      pendingImages.length > 0 || pendingFiles.length > 0 ? "flex" : "none";
  }
  $("btn-attach").addEventListener("click", () => imgInput.click());
  imgInput.addEventListener("change", async () => {
    for (const f of Array.from(imgInput.files ?? [])) {
      const url = await fileToDataUrl(f);
      if (f.type.startsWith("image/")) pendingImages.push(url);
      else pendingFiles.push({ name: f.name, data_url: url });
    }
    imgInput.value = "";
    renderAttachPreview();
  });
  // 直接粘贴剪贴板里的图片（截图后 Ctrl+V）。用 paste 事件而非 navigator.clipboard，
  // 后者要 secure context（HTTPS/localhost），纯 HTTP 部署下不可用；paste 事件无此限制。
  input.addEventListener("paste", async (e) => {
    const items = e.clipboardData?.items;
    if (!items) return;
    const files: File[] = [];
    for (const it of Array.from(items)) {
      if (it.kind === "file" && it.type.startsWith("image/")) {
        const f = it.getAsFile();
        if (f) files.push(f);
      }
    }
    if (files.length === 0) return; // 没有图片就放行，普通文本粘贴照常
    e.preventDefault(); // 有图片才拦截，避免把图片塞进文本框
    for (const f of files) pendingImages.push(await fileToDataUrl(f));
    renderAttachPreview();
  });

  // ── 任务配置（按任务绑定：工作目录/模型/技能/solo）──────────────────────
  // 工作目录在任务首条消息发出前可改、之后冻结；模型/技能/solo 任何时候可改。
  interface TaskCfg {
    working_dir: string;
    model_id: string; // "" = 跟随全局
    skills: string[]; // [] = 自动选择
    auto_approve: boolean | null; // null=默认 / true=solo / false=每步确认
    plan_mode: boolean; // 计划模式：只读探索+出计划，禁止改动
    reasoning_effort: string; // "__default__"=跟随模型/全局 / ""=本会话关闭 / low..max=本会话档位
  }
  const blankCfg = (): TaskCfg => ({
    working_dir: "",
    model_id: "",
    skills: [],
    auto_approve: null,
    plan_mode: false,
    reasoning_effort: "__default__",
  });
  const cfgBySession = new Map<string, TaskCfg>();
  const cfgOf = (id: string | null): TaskCfg => {
    if (!id) return blankCfg();
    let c = cfgBySession.get(id);
    if (!c) {
      c = blankCfg();
      cfgBySession.set(id, c);
    }
    return c;
  };
  let workspace = ""; // 全局工作目录（来自 /api/config），作为空工作目录时的展示与落点。
  let llmRows: { id: string; name?: string; model?: string }[] = [];
  let globalActiveLlm = ""; // 全局当前模型 id：任务未指定模型时，下拉默认显示它的名称。
  // 任务是否已开始（首条消息后服务端会命名）——开始后工作目录冻结。
  const started = (id: string | null): boolean => !!(id && sessions.find(id)?.name);
  const leafOf = (p: string): string => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

  // 把某任务配置持久化到服务端（随会话）。
  async function persistTaskConfig(id: string): Promise<void> {
    const c = cfgOf(id);
    await fetch(`${apiBase}/api/sessions/${encodeURIComponent(id)}/config`, {
      method: "POST",
      headers: { "content-type": "application/json", ...authHeaders() },
      body: JSON.stringify({
        working_dir: c.working_dir || null,
        model_id: c.model_id || null,
        skills: c.skills,
        auto_approve: c.auto_approve,
        plan_mode: c.plan_mode,
        // 哨兵 __default__ → null（跟随模型/全局）；其余（含 ""=关闭）原样发。
        reasoning_effort: c.reasoning_effort === "__default__" ? null : c.reasoning_effort,
      }),
    });
  }
  // 已开始的任务：配置改动即时落库；未开始的：仅本地暂存，首条消息发出时一并提交。
  const persistIfStarted = (id: string | null): void => {
    if (id && started(id)) void persistTaskConfig(id);
  };
  async function loadTaskConfig(id: string): Promise<void> {
    // 未开始的任务服务端没有已存配置（随首条消息一并提交），GET 只会返回空默认值，
    // 反而覆盖本地暂存的配置（如 + 号继承的工作目录）→ 直接保留本地。
    if (!started(id)) {
      renderTaskControls();
      return;
    }
    try {
      const r = await fetch(`${apiBase}/api/sessions/${id}/config`, { headers: authHeaders() });
      const d = (await r.json()) as { config?: Partial<TaskCfg> };
      const c = d.config ?? {};
      cfgBySession.set(id, {
        working_dir: c.working_dir ?? "",
        model_id: c.model_id ?? "",
        skills: c.skills ?? [],
        auto_approve: c.auto_approve ?? null,
        plan_mode: c.plan_mode ?? false,
        // 服务端 None 时该字段缺省（null/undefined）→ 跟随默认哨兵。
        reasoning_effort: c.reasoning_effort == null ? "__default__" : c.reasoning_effort,
      });
    } catch {
      cfgBySession.set(id, blankCfg());
    }
    renderTaskControls();
  }

  // 控件：工作目录 / 模型 / 技能 / 权限。
  const cwdPill = $("cwd-pill");
  const cwdLabel = cwdPill.querySelector(".cwd-label") as HTMLElement;
  const modelSel = $("llm-switch") as HTMLSelectElement;
  const effortSel = $("effort-select") as HTMLSelectElement;
  const skillsPill = $("skills-pill");
  const permSel = $("perm-select") as HTMLSelectElement;
  const planPill = $("plan-pill");

  const renderTaskControls = (): void => {
    const id = sessions.activeId;
    const c = cfgOf(id);
    // 工作目录
    const effective = c.working_dir || workspace;
    cwdLabel.textContent = effective ? leafOf(effective) : t("composer.cwd.label");
    const frozen = started(id);
    cwdPill.title = c.working_dir
      ? t("composer.cwd.task", { dir: c.working_dir }) + (frozen ? t("composer.cwd.locked") : "")
      : workspace
        ? t("composer.cwd.global", { dir: workspace })
        : t("composer.cwd.label");
    cwdPill.classList.toggle("set", !!c.working_dir);
    cwdPill.classList.toggle("locked", frozen);
    // 模型：不显示「默认」选项；任务未指定时显示全局当前模型的名称。
    modelSel.innerHTML = llmRows
      .map((l) => `<option value="${l.id}">${l.model || l.name || l.id}</option>`)
      .join("");
    modelSel.value = c.model_id || globalActiveLlm;
    // 本会话思考强度（模型不支持思考时，服务端会忽略该值）
    effortSel.value = c.reasoning_effort || "__default__";
    // 技能（icon-only：钉选数量用右上角小角标体现，无文字）
    setPillCount(skillsPill, c.skills.length);
    skillsPill.title = c.skills.length
      ? t("composer.skills.pinned", { n: c.skills.length, list: c.skills.join(", ") })
      : t("composer.skills.empty");
    // 权限
    permSel.value =
      c.auto_approve === true ? "solo" : c.auto_approve === false ? "strict" : "default";
    // 计划模式
    planPill.classList.toggle("set", c.plan_mode);
    planPill.title = c.plan_mode ? t("composer.plan.on") : t("composer.plan.off");
    // 工具组宽度可能随模型名/技能数变化 → 重算是否需要收起。
    updateToolbar();
  };

  cwdPill.addEventListener("click", () => {
    const id = sessions.activeId;
    if (id && started(id)) return; // 已开始，工作目录锁定
    void (async () => {
      const c = cfgOf(id);
      const v = await pickDirectory(c.working_dir || workspace, t("composer.cwd.pickPrompt"));
      if (v !== null) {
        cfgOf(id).working_dir = v;
        renderTaskControls();
        persistIfStarted(id);
      }
    })();
  });
  modelSel.addEventListener("change", () => {
    const id = sessions.activeId;
    cfgOf(id).model_id = modelSel.value;
    renderTaskControls();
    persistIfStarted(id);
  });
  effortSel.addEventListener("change", () => {
    const id = sessions.activeId;
    cfgOf(id).reasoning_effort = effortSel.value;
    renderTaskControls();
    persistIfStarted(id);
  });
  permSel.addEventListener("change", () => {
    const id = sessions.activeId;
    cfgOf(id).auto_approve =
      permSel.value === "solo" ? true : permSel.value === "strict" ? false : null;
    renderTaskControls();
    persistIfStarted(id);
  });
  planPill.addEventListener("click", () => {
    const id = sessions.activeId;
    const c = cfgOf(id);
    c.plan_mode = !c.plan_mode;
    renderTaskControls();
    persistIfStarted(id);
  });
  skillsPill.addEventListener("click", () => void openSkillsPicker());

  // ── 工具组自适应收起：空间不足时折叠成「›」，点击在输入框上方弹出 ──────────
  const composerBar = document.querySelector(".composer-bar") as HTMLElement;
  const cbTools = $("cb-tools");
  const cbMore = $("cb-more") as HTMLButtonElement;
  const cbActions = document.querySelector(".cb-actions") as HTMLElement;
  function updateToolbar(): void {
    const wasOpen = composerBar.classList.contains("tools-open");
    // 先按展开态测量工具内容真实宽度。
    composerBar.classList.remove("collapsed");
    cbMore.hidden = true;
    const needed = cbTools.scrollWidth;
    const avail = composerBar.clientWidth - cbActions.offsetWidth - 16;
    const collapse = needed > avail;
    composerBar.classList.toggle("collapsed", collapse);
    cbMore.hidden = !collapse;
    if (!collapse) composerBar.classList.remove("tools-open");
    else if (wasOpen) composerBar.classList.add("tools-open");
  }
  cbMore.addEventListener("click", (e) => {
    e.stopPropagation();
    composerBar.classList.toggle("tools-open");
  });
  // 点击弹层外部时收起。
  document.addEventListener("click", (e) => {
    if (!composerBar.classList.contains("tools-open")) return;
    const tgt = e.target as Node;
    if (!cbTools.contains(tgt) && tgt !== cbMore && !cbMore.contains(tgt)) {
      composerBar.classList.remove("tools-open");
    }
  });
  // 撰写区尺寸变化（窗口缩放 / 预览面板开合压缩主区）时重算（rAF 防抖避免 RO 循环告警）。
  new ResizeObserver(() => requestAnimationFrame(updateToolbar)).observe(composerBar);

  async function openSkillsPicker(): Promise<void> {
    const id = sessions.activeId;
    if (!id) {
      newSession();
    }
    const sid = sessions.activeId;
    if (!sid) return;
    let entries: { name: string; description?: string; source?: string }[] = [];
    try {
      // 带上本任务工作目录，让后端把 <workdir>/skills 项目级技能也列出来。
      const wd = cfgOf(sid).working_dir;
      const url = wd
        ? `${apiBase}/api/skills/catalog?workdir=${encodeURIComponent(wd)}`
        : `${apiBase}/api/skills/catalog`;
      const r = await fetch(url, { headers: authHeaders() });
      entries = ((await r.json()) as { entries?: typeof entries }).entries ?? [];
    } catch {
      entries = [];
    }
    const picked = new Set(cfgOf(sid).skills);
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    overlay.innerHTML = `
      <div class="modal">
        <div class="modal-head"><span class="ico-tile">${icon("skill", 18)}</span><h3>${t("skillsPicker.title")}</h3>
          <span class="spacer" style="flex:1"></span>
          <button class="btn btn-sm btn-icon" id="sk-close">${icon("x", 17)}</button>
        </div>
        <div class="modal-body">
          <div class="inline-note">${t("skillsPicker.note")}</div>
          <div id="sk-list" class="mg-list" style="margin-top:12px"></div>
        </div>
        <div class="modal-foot"><span class="spacer" style="flex:1"></span><button id="sk-done" class="btn btn-primary">${t("common.done")}</button></div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;
    const listEl = q("#sk-list");
    if (entries.length === 0) {
      listEl.innerHTML = `<div class="empty">${t("skillsPicker.empty")}</div>`;
    } else {
      for (const e of entries) {
        const row = document.createElement("label");
        row.className = "mg-row sk-row";
        const cb = document.createElement("input");
        cb.type = "checkbox";
        cb.checked = picked.has(e.name);
        cb.addEventListener("change", () => {
          if (cb.checked) picked.add(e.name);
          else picked.delete(e.name);
        });
        const meta = document.createElement("span");
        meta.className = "mg-name";
        const badge =
          e.source === "workdir"
            ? ` <span class="badge accent" style="height:17px;padding:0 6px;font-size:11px" title="${t("skillsPicker.projectBadge.title")}">${t("skillsPicker.projectBadge")}</span>`
            : "";
        meta.innerHTML = `<strong>${escapeHtml(e.name)}${badge}</strong><span class="t3">${escapeHtml(e.description ?? "")}</span>`;
        row.append(cb, meta);
        listEl.appendChild(row);
      }
    }
    const commit = (): void => {
      cfgOf(sid).skills = [...picked];
      renderTaskControls();
      persistIfStarted(sid);
      overlay.remove();
    };
    q("#sk-close").addEventListener("click", commit);
    q("#sk-done").addEventListener("click", commit);
    overlay.addEventListener("click", (e) => {
      if (e.target === overlay) commit();
    });
  }

  // 知识库（按会话绑定，服务端随会话持久化）：本地缓存 + REST 读写。
  const knowledgeBySession = new Map<string, string[]>();
  const kbOf = (id: string | null): string[] => (id ? (knowledgeBySession.get(id) ?? []) : []);
  async function loadKnowledge(id: string): Promise<void> {
    try {
      const r = await fetch(`${apiBase}/api/sessions/${id}/knowledge`, { headers: authHeaders() });
      const data = (await r.json()) as { paths?: string[] };
      knowledgeBySession.set(id, data.paths ?? []);
    } catch {
      knowledgeBySession.set(id, []);
    }
    renderKb();
  }
  const kbPill = $("kb-pill");
  const renderKb = (): void => {
    const list = kbOf(sessions.activeId);
    // 知识库（icon-only：数量走右上角小角标，无文字）
    setPillCount(kbPill, list.length);
    kbPill.title = list.length
      ? t("composer.kb.pinned", { n: list.length, list: list.join("\n") })
      : t("composer.kb.empty");
  };
  function openKbManager(): void {
    const sid = sessions.activeId;
    if (!sid) {
      newSession();
    }
    const id = sessions.activeId;
    if (!id) return;
    const list = [...kbOf(id)];
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    overlay.innerHTML = `
      <div class="modal">
        <div class="modal-head"><span class="ico-tile">${icon("doc", 18)}</span><h3>${t("kb.title")}</h3>
          <span class="spacer" style="flex:1"></span>
          <button class="btn btn-sm btn-icon" id="kb-close">${icon("x", 17)}</button>
        </div>
        <div class="modal-body">
          <div class="inline-note">${t("kb.note")}</div>
          <div id="kb-list" class="mg-list" style="margin-top:12px"></div>
          <div class="field" style="margin-top:14px">
            <label>${t("kb.addLabel")}</label>
            <div class="input-group">
              <input id="kb-input" class="input mono" placeholder="${t("kb.input.placeholder")}" />
              <button id="kb-pick" class="btn btn-sm btn-icon" title="${t("kb.pick.title")}">⋯</button>
              <button id="kb-add" class="btn btn-sm btn-primary" style="margin-left:8px">${t("common.add")}</button>
            </div>
          </div>
        </div>
        <div class="modal-foot"><span class="spacer" style="flex:1"></span><button id="kb-done" class="btn btn-primary">${t("common.done")}</button></div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;
    const renderList = (): void => {
      const el = q("#kb-list");
      el.replaceChildren();
      if (list.length === 0) {
        el.innerHTML = `<div class="empty">${t("kb.empty")}</div>`;
        return;
      }
      list.forEach((p, i) => {
        const row = document.createElement("div");
        row.className = "mg-row";
        row.innerHTML = `<span class="mg-name mono" style="font-weight:400">${escapeHtml(p)}</span><span class="spacer" style="flex:1"></span>`;
        const del = document.createElement("button");
        del.className = "btn btn-sm btn-danger";
        del.textContent = t("common.remove");
        del.onclick = () => {
          list.splice(i, 1);
          renderList();
        };
        row.appendChild(del);
        el.appendChild(row);
      });
    };
    const add = (p: string): void => {
      const v = p.trim();
      if (v && !list.includes(v)) list.push(v);
      renderList();
    };
    q("#kb-add").addEventListener("click", () => {
      add(q<HTMLInputElement>("#kb-input").value);
      q<HTMLInputElement>("#kb-input").value = "";
    });
    q("#kb-pick").addEventListener("click", () => {
      void pickDirectory("", t("kb.pickPrompt")).then((v) => {
        if (v) add(v);
      });
    });
    const commit = (): void => {
      knowledgeBySession.set(id, list);
      renderKb();
      overlay.remove();
      // 持久化到会话（绑定会话、跨重启保留）。
      void fetch(`${apiBase}/api/sessions/${encodeURIComponent(id)}/knowledge`, {
        method: "POST",
        headers: { "content-type": "application/json", ...authHeaders() },
        body: JSON.stringify({ paths: list }),
      });
    };
    q("#kb-close").addEventListener("click", commit);
    q("#kb-done").addEventListener("click", commit);
    overlay.addEventListener("click", (e) => {
      if (e.target === overlay) commit();
    });
    renderList();
  }
  kbPill.addEventListener("click", openKbManager);
  renderKb();

  // 记忆（两层）：项目级=同工作目录所有会话共享，会话级=只属这次对话。
  // 原本完全不给入口（"自动管理"），结果用户既看不到 AI 记住了什么，也改不掉记错的。
  const memPill = $("mem-pill");
  async function openMemory(): Promise<void> {
    const id = sessions.activeId;
    if (!id) {
      newSession();
      return;
    }
    let data: { memory?: string; project_memory?: string; project_dir?: string } = {};
    try {
      const r = await fetch(`${apiBase}/api/sessions/${encodeURIComponent(id)}/memory`, {
        headers: authHeaders(),
      });
      data = (await r.json()) as typeof data;
    } catch {
      /* 读不到就当空的，仍让用户能写 */
    }
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    overlay.innerHTML = `
      <div class="modal">
        <div class="modal-head"><span class="ico-tile">${icon("brain", 18)}</span><h3>${t("mem.title")}</h3>
          <span class="spacer" style="flex:1"></span>
          <button class="btn btn-sm btn-icon" id="mem-close">${icon("x", 17)}</button>
        </div>
        <div class="modal-body">
          <div class="inline-note">${t("mem.note")}</div>
          <div class="field" style="margin-top:14px">
            <label>${t("mem.project", { dir: escapeHtml(data.project_dir ?? "") })}</label>
            <textarea id="mem-project" class="input mono" rows="10" placeholder="${t("mem.empty")}"></textarea>
            <div class="hint">${t("mem.projectHint")}</div>
          </div>
          <div class="field" style="margin-top:14px">
            <label>${t("mem.session")}</label>
            <textarea id="mem-session" class="input mono" rows="6" placeholder="${t("mem.empty")}"></textarea>
            <div class="hint">${t("mem.sessionHint")}</div>
          </div>
        </div>
        <div class="modal-foot"><span class="spacer" style="flex:1"></span>
          <button id="mem-save" class="btn btn-primary">${t("common.save")}</button></div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;
    // 用 value 赋值而非模板插值：记忆内容含 `</textarea>` 之类会破坏标记。
    q<HTMLTextAreaElement>("#mem-project").value = data.project_memory ?? "";
    q<HTMLTextAreaElement>("#mem-session").value = data.memory ?? "";
    const close = (): void => overlay.remove();
    q("#mem-close").addEventListener("click", close);
    overlay.addEventListener("click", (e) => {
      if (e.target === overlay) close();
    });
    q("#mem-save").addEventListener("click", () => {
      const body = {
        project_memory: q<HTMLTextAreaElement>("#mem-project").value,
        memory: q<HTMLTextAreaElement>("#mem-session").value,
      };
      void fetch(`${apiBase}/api/sessions/${encodeURIComponent(id)}/memory`, {
        method: "POST",
        headers: { "content-type": "application/json", ...authHeaders() },
        body: JSON.stringify(body),
      }).then(close);
    });
  }
  memPill.addEventListener("click", () => void openMemory());

  async function send(): Promise<void> {
    const text = input.value.trim();
    if (!text && pendingImages.length === 0 && pendingFiles.length === 0) return;
    input.value = "";
    if (!sessions.activeId) newSession();
    const sid = sessions.activeId;
    if (!sid) return;
    const imgs = pendingImages;
    const docs = pendingFiles;
    pendingImages = [];
    pendingFiles = [];
    renderAttachPreview();
    const thumbs = imgs.map((u) => `<img class="attach-thumb" src="${u}">`).join("");
    const fileTags = docs
      .map((f) => `<span class="attach-file">${escapeHtml(f.name)}</span>`)
      .join("");
    sessions.appendMsg("user", escapeHtml(text) + thumbs + fileTags, { forceScroll: true });
    const c = cfgOf(sid);
    // 先把任务配置落库（模型/技能/权限/工作目录），确保服务端本轮按此配置运行。
    try {
      await persistTaskConfig(sid);
    } catch {
      /* 落库失败仍发送，服务端会用现有/默认配置 */
    }
    ws.send({
      type: "message",
      session_id: sid,
      content: text,
      images: imgs,
      files: docs,
      cwd: c.working_dir || undefined,
    } as WsMessage);
  }

  // 首次拿到会话列表后：默认进入「新对话」（历史会话留在侧栏，按需点选），
  // 不自动恢复上次会话。
  let autoSelected = false;
  ws.onEvent((e) => {
    if (e.type !== "session_list" || autoSelected) return;
    autoSelected = true;
    if (sessions.activeId) return;
    newSession();
  });

  // 运行中显示「停止」按钮（可中断 agent 回合）。
  const stopBtn = $("btn-stop") as HTMLButtonElement;
  function setWorking(on: boolean): void {
    stopBtn.style.display = on ? "inline-block" : "none";
  }
  stopBtn.addEventListener("click", () => {
    if (sessions.activeId) ws.send({ type: "interrupt", session_id: sessions.activeId });
  });
  // 失败消息上的「重试」：对现有历史重跑本轮（不重复发消息）。事件委托，覆盖动态插入的按钮。
  messages.addEventListener("click", (e) => {
    const btn = (e.target as HTMLElement | null)?.closest(".retry-btn") as HTMLButtonElement | null;
    if (!btn) return;
    const sid = btn.dataset.retrySession || sessions.activeId || undefined;
    if (!sid) return;
    ws.send({ type: "retry", session_id: sid } as WsMessage);
    btn.disabled = true;
    btn.textContent = t("chat.retrying");
  });
  ws.onEvent((e) => {
    if (e.session_id && e.session_id !== sessions.activeId) return;
    if (e.type === "session_update") {
      const status =
        (e.session as { status?: string } | undefined)?.status ?? (e.status as string | undefined);
      if (status === "working") setWorking(true);
      else if (status === "idle") setWorking(false);
    } else if (e.type === "complete" || e.type === "interrupted" || e.type === "error") {
      setWorking(false);
    }
  });
  // 当前任务被命名（首条消息后）即视为已开始：刷新控件以锁定工作目录。
  ws.onEvent((e) => {
    if (e.type === "session_renamed" && e.session_id === sessions.activeId) renderTaskControls();
  });

  renderTaskControls();

  // 整页视图（P2–P6）。
  const views = {
    chat: { el: $("chat-view"), title: t("nav.chat"), refresh: () => {} },
    tasks: {
      el: $("tasks-view"),
      title: t("nav.tasks"),
      refresh: mountTasksView($("tasks-view")).refresh,
    },
    jobs: {
      el: $("jobs-view"),
      title: t("nav.jobs"),
      refresh: mountJobsView($("jobs-view")).refresh,
    },
    skills: {
      el: $("skills-view"),
      title: t("nav.skills"),
      // 带上当前会话的工作目录：后端才会把 `<工作目录>/skills` 的项目级技能一并列出，
      // 与这个会话里 agent 实际能调用的那一份保持一致。
      refresh: mountSkillsView($("skills-view"), () => cfgOf(sessions.activeId).working_dir || "")
        .refresh,
    },
    channels: {
      el: $("channels-view"),
      title: t("nav.channels"),
      refresh: mountChannelsView($("channels-view")).refresh,
    },
    settings: {
      el: $("settings-view"),
      title: t("nav.settings"),
      refresh: mountSettingsView($("settings-view")).refresh,
    },
  };
  type ViewKey = keyof typeof views;

  function setView(view: ViewKey): void {
    for (const b of document.querySelectorAll<HTMLElement>(".nav-item")) {
      b.classList.toggle("active", b.dataset.view === view);
    }
    for (const [k, v] of Object.entries(views)) v.el.hidden = k !== view;
    $("page-title").textContent = views[view].title;
    views[view].refresh();
  }
  for (const b of document.querySelectorAll<HTMLElement>(".nav-item")) {
    b.addEventListener("click", () => setView((b.dataset.view as ViewKey) ?? "chat"));
  }
  // 新建任务：切到对话视图并开一个新任务（不打断正在运行的其它任务）。
  $("btn-new-task").addEventListener("click", () => {
    setView("chat");
    newSession();
  });

  // 导航栏底部：当前模型（显示模型 ID）+ 今日成本 + 访问状态。
  let footChecked = false;
  async function refreshFoot(): Promise<void> {
    try {
      const cfg = (await (
        await fetch(`${apiBase}/api/config`, { headers: authHeaders() })
      ).json()) as {
        llms?: { id: string; name?: string; provider?: string; model?: string }[];
        active_llm?: string;
        llm_ready?: boolean;
        access_key_set?: boolean;
        workspace?: string;
        version?: string;
      };
      const active = (cfg.llms ?? []).find((l) => l.id === cfg.active_llm);
      $("foot-model").textContent = active ? active.model || active.id : t("foot.model.unset");
      $("foot-model").title = active?.model ?? "";
      const access = $("foot-access");
      access.textContent = cfg.access_key_set ? t("foot.access.locked") : t("foot.access.public");
      access.className = cfg.access_key_set ? "badge green" : "badge";
      // 服务端版本（不是前端包的版本）：部署后刷新页面就能确认新代码是否真的起来了。
      $("foot-version").textContent = cfg.version ? `v${cfg.version}` : "";
      // 全局工作目录 + 模型列表 + 当前模型：任务配置控件的默认展示与候选。
      workspace = cfg.workspace ?? "";
      llmRows = cfg.llms ?? [];
      globalActiveLlm = cfg.active_llm ?? llmRows[0]?.id ?? "";
      renderTaskControls();
      // 首次未配置任何可用 LLM → 自动切到设置引导。订阅档（OAuth，无 api_key）也算可用，
      // 判据由服务端的 llm_ready 给出，别在这里用「有没有 api_key」自行推断。
      if (!cfg.llm_ready && !footChecked) setView("settings");
      footChecked = true;
    } catch {
      // 后端不可达就不打扰。
    }
  }
  window.addEventListener("wc:llm-changed", () => void refreshFoot());
  void refreshFoot();

  // 今日成本：由服务端按本地日期记账（交互对话 + 定时任务都计入，无人值守也不漏），
  // 前端只负责显示。连上推一次初值，之后每次成本变动推 cost_update。
  const renderCost = (v: number): void => {
    $("foot-cost").textContent = `¥${(Number(v) || 0).toFixed(2)}`;
  };
  renderCost(0);
  ws.onEvent((e) => {
    if (e.type !== "cost_update") return;
    renderCost(Number((e as { cost_today?: number }).cost_today) || 0);
  });

  sendBtn.addEventListener("click", () => void send());
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void send();
    }
  });

  // 链接一律走系统浏览器：桌面壳是个 webview，放任 <a href> 默认跳转会把整个应用顶掉。
  installExternalLinkHandler();

  // 产物预览：agent 写出 .md/.html/代码等文件后，complete 时在对话里给出可点击入口。
  const artifact = mountArtifact();
  let pendingArtifacts: { path: string; name: string; cwd: string }[] = [];
  // 可点击的产物：md/html/svg 渲染预览，其余常见文本/代码以「代码」视图打开。
  const PREVIEWABLE =
    /\.(html?|svg|md|markdown|txt|log|json|jsonc|csv|tsv|ya?ml|toml|ini|xml|css|scss|less|js|mjs|cjs|ts|tsx|jsx|py|rs|go|rb|php|java|kt|c|h|cpp|hpp|cs|sh|bash|sql)$/i;
  const RENDERED = /\.(html?|svg|md|markdown)$/i;
  /** 该产物现在是否真存在（probe=1 只判存在、不传内容）。探测本身出错时返回 true。 */
  const artifactExists = async (a: { path: string; cwd: string }): Promise<boolean> => {
    try {
      const qs = a.cwd ? `&cwd=${encodeURIComponent(a.cwd)}` : "";
      const r = await fetch(
        `${httpBase()}/api/artifact?probe=1&path=${encodeURIComponent(a.path)}${qs}`,
        { headers: authHeaders() },
      );
      return ((await r.json()) as { ok?: boolean }).ok !== false;
    } catch {
      return true;
    }
  };
  const addArtifactRef = (a: { path: string; name: string; cwd: string }): void => {
    const ref = document.createElement("button");
    ref.className = "artifact-ref";
    const ico = RENDERED.test(a.path) ? "globe" : "doc";
    const hint = RENDERED.test(a.path) ? t("artifact.rendered") : t("artifact.source");
    ref.innerHTML = `<span class="ar-ico">${icon(ico, 18)}</span><span class="ar-meta"><strong>${escapeHtml(a.name)}</strong><span class="t3">${hint}</span></span><span class="ar-chev">${icon("chevR", 16)}</span>`;
    ref.addEventListener("click", () => artifact.show(a.path, a.cwd));
    // 先测后改：append 之后 scrollHeight 已经变大，再测就永远不算「贴底」。
    // 用户上滚翻历史时别把他拽回底部（与 sessions.ts 的跟随策略一致）。
    const atBottom = messages.scrollHeight - messages.scrollTop - messages.clientHeight <= 40;
    messages.appendChild(ref);
    if (atBottom) messages.scrollTop = messages.scrollHeight;
  };
  ws.onEvent((e) => {
    if (e.session_id && e.session_id !== sessions.activeId) return;
    if (e.type === "tool_call") {
      const name = (e.name as string) ?? "";
      const path = (e.args as { path?: string } | undefined)?.path;
      if (/^(write_file|edit_file)$/.test(name) && path && PREVIEWABLE.test(path)) {
        // 记下本会话有效工作目录，相对路径预览时据此解析（write_file 也以此为基准写入）。
        // 必须与 effective 一致带上 workspace 回退：任务用默认工作空间（未设 working_dir）时，
        // 产物落在全局 workspace；若只传空 working_dir，后端会错回退到进程 CWD → 预览找不到文件。
        const cwd = cfgOf(sessions.activeId).working_dir || workspace;
        const fname = path.split(/[\\/]/).pop() ?? path;
        // 同一文件多次写入只留一个入口（保留最新）。
        pendingArtifacts = pendingArtifacts.filter((a) => a.path !== path);
        pendingArtifacts.push({ path, name: fname, cwd });
      }
    } else if (e.type === "complete" && pendingArtifacts.length) {
      const arts = pendingArtifacts;
      pendingArtifacts = [];
      // 入口是从 tool_call 参数里抓的，那时还不知道这次写入到底成没成。写失败（比如
      // edit_file 的 old_string 没匹配上）、或 agent 本轮里又把文件挪走/删掉的，都会留下
      // 一个点开就是「文件不存在」的死入口。所以轮末逐个核对一次，只留真的还在的。
      // 探测失败（网络/后端异常）按「留着」处理——宁可留个可能点不开的，也别把真产物吞了。
      void (async () => {
        for (const a of arts) {
          if (await artifactExists(a)) addArtifactRef(a);
        }
      })();
    }
  });

  // 空状态 hero：无消息时显示问候 + 建议卡，点卡片即发送。
  const hour = new Date().getHours();
  const greet = t(
    hour < 6
      ? "greet.night"
      : hour < 12
        ? "greet.morning"
        : hour < 18
          ? "greet.afternoon"
          : "greet.evening",
  );
  const suggestions: [string, string, string][] = [
    ["📄", t("suggest.landing.title"), t("suggest.landing.sub")],
    ["🔎", t("suggest.research.title"), t("suggest.research.sub")],
    ["🐞", t("suggest.debug.title"), t("suggest.debug.sub")],
    ["📊", t("suggest.chart.title"), t("suggest.chart.sub")],
  ];
  const hero = document.createElement("div");
  hero.className = "hero";
  hero.innerHTML = `
    <div class="hero-mark" aria-hidden="true"></div>
    <h1>${escapeHtml(t("hero.title", { greet }))}</h1>
    <p>${t("hero.sub")}</p>
    <div class="suggest-grid">
      ${suggestions
        .map(
          ([i, ttl, sub]) =>
            `<button class="suggest" data-prompt="${escapeHtml(ttl)}"><span class="sg-ico">${i}</span><span class="sg-text"><strong>${escapeHtml(ttl)}</strong><span>${escapeHtml(sub)}</span></span></button>`,
        )
        .join("")}
    </div>`;
  $("chat-view").insertBefore(hero, messages);
  for (const b of hero.querySelectorAll<HTMLElement>("[data-prompt]")) {
    b.addEventListener("click", () => {
      input.value = b.dataset.prompt ?? "";
      void send();
    });
  }
  const toggleHero = (): void => {
    hero.style.display = messages.childElementCount === 0 ? "block" : "none";
  };
  new MutationObserver(toggleHero).observe(messages, { childList: true });
  toggleHero();

  ws.connect();
}

if (typeof document !== "undefined") {
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", bootstrap);
  } else {
    bootstrap();
  }
}
