// ── 定时任务（整页视图 P4）────────────────────────────────────────────────────
// 统计卡 + 任务卡列表 + 新建任务弹窗 + 日志抽屉。接 /api/cron 与 /api/channels。

import { authHeaders } from "./auth";
import { httpBase } from "./backend";
import { t } from "./i18n";

interface Channel {
  name: string;
  kind: string;
}
interface Llm {
  id: string;
  model?: string | null;
  provider?: string | null;
}
interface CronTask {
  id: string;
  name: string;
  interval_secs: number;
  cron?: string | null;
  prompt: string;
  channel?: string | null;
  workdir?: string | null;
  model?: string | null;
  enabled: boolean;
  last_status?: string | null;
  runs?: number;
  last_duration_ms?: number | null;
}

function esc(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
async function api(path: string, init?: RequestInit): Promise<Record<string, unknown>> {
  const r = await fetch(`${httpBase()}${path}`, {
    ...init,
    headers: { "content-type": "application/json", ...authHeaders() },
  });
  return (await r.json().catch(() => ({}))) as Record<string, unknown>;
}
function fmtInterval(secs: number): string {
  if (secs % 86400 === 0) return t("tasks.interval.days", { n: secs / 86400 });
  if (secs % 3600 === 0) return t("tasks.interval.hours", { n: secs / 3600 });
  if (secs % 60 === 0) return t("tasks.interval.minutes", { n: secs / 60 });
  return t("tasks.interval.seconds", { n: secs });
}

export function mountTasksView(container: HTMLElement): { refresh: () => void } {
  container.innerHTML = `
    <div class="page">
      <div class="section-head">
        <h2>${t("nav.tasks")}</h2>
        <div class="desc">${t("tasks.desc")}</div>
        <span class="spacer"></span>
        <button id="tv-add" class="btn btn-primary btn-sm">${t("tasks.add")}</button>
      </div>
      <div class="stat-row" id="tv-stats"></div>
      <div class="task-list" id="tv-list"></div>
      <div id="tv-status" class="muted" style="font-size:13px;margin-top:14px"></div>
    </div>`;

  const $ = <T extends HTMLElement>(s: string) => container.querySelector(s) as T;
  const setStatus = (s: string) => {
    $("#tv-status").textContent = s;
  };
  let channels: Channel[] = [];
  // 扫码连接的双向机器人也可当通知目标（合成通道，不落 channels.json）：
  // 飞书最近会话 → feishu_app:<chat_id>；QQ 最近会话 → qqbot:<scope>:<id>。
  let feishuChats: string[] = [];
  let qqPeers: { scope: string; id: string }[] = [];
  let llms: Llm[] = [];
  let defaultLlm: string | null = null;
  // 任务卡上显示模型：id → 模型名（找不到则原样显示 id）。
  const modelLabel = (id?: string | null): string => {
    if (!id) return "";
    const l = llms.find((x) => x.id === id);
    return l ? l.model || l.id : id;
  };

  function renderStats(tasks: CronTask[]): void {
    const on = tasks.filter((t) => t.enabled).length;
    const okRate = (() => {
      const ran = tasks.filter((t) => t.last_status);
      if (ran.length === 0) return "—";
      const ok = ran.filter((t) => t.last_status === "ok").length;
      return `${Math.round((ok / ran.length) * 100)}%`;
    })();
    const totalRuns = tasks.reduce((s, t) => s + (t.runs ?? 0), 0);
    const cards = [
      [t("tasks.stat.running"), String(on)],
      [t("tasks.stat.totalRuns"), String(totalRuns)],
      [t("tasks.stat.disabled"), String(tasks.length - on)],
      [t("tasks.stat.successRate"), okRate],
    ];
    $("#tv-stats").innerHTML = cards
      .map(
        ([l, n]) =>
          `<div class="stat-card"><div class="sc-num">${n}</div><div class="sc-label">${l}</div></div>`,
      )
      .join("");
  }

  function statusBadge(s?: string | null): string {
    if (s === "ok") return `<span class="badge green">${t("tasks.status.ok")}</span>`;
    if (!s) return `<span class="badge">${t("tasks.status.notRun")}</span>`;
    // 已知的非成功终止原因给标签，其余原样显示。
    const labels: Record<string, string> = {
      max_iterations: t("tasks.status.maxIter"),
      llm_error: t("tasks.status.llmError"),
      workdir_not_found: t("tasks.status.workdirNotFound"),
    };
    const text = labels[s] ?? (s.startsWith("notify_err") ? t("tasks.status.notifyErr") : s);
    return `<span class="badge" style="background:var(--amber-soft);color:var(--amber)" title="${esc(s)}">${esc(text)}</span>`;
  }

  function renderList(tasks: CronTask[]): void {
    const list = $("#tv-list");
    list.replaceChildren();
    if (tasks.length === 0) {
      list.innerHTML = `<div class="empty">${t("tasks.empty")}</div>`;
      return;
    }
    for (const task of tasks) {
      const card = document.createElement("div");
      card.className = `task-card${task.enabled ? "" : " off"}`;
      card.innerHTML = `
        <button class="toggle ${task.enabled ? "on" : ""}" data-act="toggle"></button>
        <div class="task-main">
          <div class="task-top">
            <strong>${esc(task.name)}</strong>
            ${statusBadge(task.last_status)}
            ${task.channel ? `<span class="badge">${esc(task.channel)}</span>` : ""}
          </div>
          <div class="task-prompt">${esc(task.prompt)}</div>
          <div class="task-meta">
            <span>⏱ ${task.cron ? `cron: ${esc(task.cron)}` : fmtInterval(task.interval_secs)}</span>
            <span class="sep">·</span>
            <span>${t("tasks.runsCount", { n: task.runs ?? 0 })}</span>
            ${task.last_duration_ms != null ? `<span class="sep">·</span><span>${t("tasks.lastDuration", { s: (task.last_duration_ms / 1000).toFixed(1) })}</span>` : ""}
            ${task.model ? `<span class="sep">·</span><span title="${t("tasks.modelTitle")}">🧠 ${esc(modelLabel(task.model))}</span>` : ""}
            ${task.workdir ? `<span class="sep">·</span><span title="${t("composer.cwd.label")}">📁 ${esc(task.workdir)}</span>` : ""}
          </div>
        </div>
        <div class="task-actions">
          <button class="btn btn-sm" data-act="run" title="${t("tasks.runTitle")}">${t("tasks.run")}</button>
          <button class="btn btn-sm" data-act="edit">${t("common.edit")}</button>
          <button class="btn btn-sm" data-act="logs">${t("tasks.logs")}</button>
          <button class="btn btn-sm btn-danger" data-act="del">${t("common.delete")}</button>
        </div>`;
      card.querySelector('[data-act="toggle"]')?.addEventListener("click", async () => {
        await api(`/api/cron/${task.id}`, {
          method: "PATCH",
          body: JSON.stringify({ enabled: !task.enabled }),
        });
        void refresh();
      });
      card.querySelector('[data-act="run"]')?.addEventListener("click", (e) => {
        const btn = e.currentTarget as HTMLButtonElement;
        void runNow(task, btn);
      });
      card.querySelector('[data-act="edit"]')?.addEventListener("click", () => openForm(task));
      card.querySelector('[data-act="logs"]')?.addEventListener("click", () => openLogs(task));
      card.querySelector('[data-act="del"]')?.addEventListener("click", async () => {
        await api(`/api/cron/${task.id}`, { method: "DELETE" });
        void refresh();
      });
      list.appendChild(card);
    }
  }

  async function refresh(): Promise<void> {
    setStatus(t("settings.status.loading"));
    try {
      channels = ((await api("/api/channels")).channels as Channel[]) ?? [];
      // 飞书/QQ 双向机器人的最近会话（可作通知目标），失败不影响主流程。
      try {
        const fc = (await api("/api/feishu/config")) as { recent_chats?: string[] };
        feishuChats = fc.recent_chats ?? [];
      } catch {
        feishuChats = [];
      }
      try {
        const qc = (await api("/api/qq/config")) as {
          recent_peers?: { scope: string; id: string }[];
        };
        qqPeers = qc.recent_peers ?? [];
      } catch {
        qqPeers = [];
      }
      const cfg = await api("/api/config");
      llms = (cfg.llms as Llm[]) ?? [];
      defaultLlm = (cfg.active_llm as string | null) ?? null;
      const tasks = ((await api("/api/cron")).tasks as CronTask[]) ?? [];
      renderStats(tasks);
      renderList(tasks);
      setStatus("");
    } catch (e) {
      setStatus(t("settings.status.loadFailed", { e: String(e) }));
    }
  }

  // 一次运行结束会落一条「汇总行」（含 ` | `，由后端 record_run 写）；中间的逐步明细行和
  // 「重试」提示都不含 ` | `。据此数已完成的运行数，作为「立即运行」何时跑完的判据
  //（不能再用「日志是否变化」——逐步明细会让日志在跑完前就持续变化）。
  function countDone(logs: string): number {
    return (logs.match(/\]\s*status=[^\n]*\|/g) ?? []).length;
  }

  // 立即运行一次：后端后台执行，打开日志抽屉「实时」轮询逐步明细，直到出现新的汇总行才算跑完。
  async function runNow(task: CronTask, btn: HTMLButtonElement): Promise<void> {
    const before = ((await api(`/api/cron/${task.id}/logs`)) as { logs?: string }).logs ?? "";
    btn.disabled = true;
    const label = btn.textContent;
    btn.textContent = t("tasks.runningBtn");
    const res = (await api(`/api/cron/${task.id}/run`, { method: "POST" })) as {
      ok?: boolean;
      error?: string;
    };
    if (!res.ok) {
      btn.disabled = false;
      btn.textContent = label;
      setStatus(t("tasks.runFailed", { e: res.error ?? "" }));
      return;
    }
    // 抽屉边跑边刷；本次产出 1 条新汇总行即视为完成，届时复位按钮并刷新列表（状态/次数）。
    openLogs(task, {
      untilDone: countDone(before) + 1,
      onDone: () => {
        btn.disabled = false;
        btn.textContent = label;
        void refresh();
      },
    });
  }

  // `opts.untilDone` 给定时：每 2s 刷新并续轮询，直到已完成运行数达标或超时，再回调 onDone。
  // 否则只加载一次（纯查看）。日志按时间顺序渲染（旧→新），并默认贴底显示最新一步。
  function openLogs(task: CronTask, opts?: { untilDone?: number; onDone?: () => void }): void {
    const scrim = document.createElement("div");
    scrim.className = "drawer-scrim";
    const drawer = document.createElement("aside");
    drawer.className = "drawer right";
    drawer.innerHTML = `
      <div class="drawer-head"><strong>${t("tasks.logsTitle", { name: esc(task.name) })}</strong><span style="flex:1"></span><button class="btn btn-ghost btn-sm" id="lg-close">✕</button></div>
      <div class="drawer-body scroll" id="lg-body"><div class="empty">${t("settings.status.loading")}</div></div>`;
    let closed = false;
    const close = () => {
      closed = true;
      scrim.remove();
      drawer.remove();
    };
    scrim.addEventListener("click", close);
    drawer.querySelector("#lg-close")?.addEventListener("click", close);
    document.body.append(scrim, drawer);
    const body = drawer.querySelector("#lg-body") as HTMLElement;

    const render = (logs: string): void => {
      const rows = logs
        .split("\n")
        .filter(Boolean)
        .map(
          (line) =>
            `<div class="log-row"><div class="log-main"><div class="log-msg">${esc(line)}</div></div></div>`,
        )
        .join("");
      // 渲染前若已贴底（或首次），渲染后继续贴底，便于追看最新一步；用户向上翻则不打扰。
      const atBottom = body.scrollHeight - body.scrollTop - body.clientHeight < 40;
      body.innerHTML = rows || `<div class="empty">${t("tasks.noLogs")}</div>`;
      if (atBottom) body.scrollTop = body.scrollHeight;
    };

    let tries = 0;
    const load = async (): Promise<void> => {
      if (closed) return;
      const logs = ((await api(`/api/cron/${task.id}/logs`)) as { logs?: string }).logs ?? "";
      render(logs);
      const target = opts?.untilDone;
      if (target === undefined || closed) return;
      tries++;
      if (countDone(logs) >= target || tries > 150) {
        opts?.onDone?.();
        return;
      }
      setTimeout(() => void load(), 2000);
    };
    void load();
  }

  // 秒 → 输入框可识别的时长串（与 parse_duration 对应）。
  function secsToDur(secs: number): string {
    if (secs % 86400 === 0) return `${secs / 86400}d`;
    if (secs % 3600 === 0) return `${secs / 3600}h`;
    if (secs % 60 === 0) return `${secs / 60}m`;
    return `${secs}s`;
  }

  // 新建（existing 省略）或编辑（传入要改的任务）共用一个表单。
  function openForm(existing?: CronTask): void {
    const editing = !!existing;
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    overlay.innerHTML = `
      <div class="modal">
        <div class="modal-head"><h3>${editing ? t("tasks.form.editTitle") : t("tasks.form.addTitle")}</h3></div>
        <div class="modal-body">
          <div class="field"><label>${t("tasks.form.name")}</label><input id="ta-name" class="input" placeholder="${t("tasks.form.namePlaceholder")}" /></div>
          <div class="field"><label>${t("tasks.form.prompt")}</label><textarea id="ta-prompt" class="textarea" rows="3" placeholder="${t("tasks.form.promptPlaceholder")}"></textarea></div>
          <div class="field">
            <label>${t("tasks.form.schedule")}</label>
            <div class="seg" style="margin-bottom:10px"><button data-m="intv" class="on">${t("tasks.form.byInterval")}</button><button data-m="cron">${t("tasks.form.byCron")}</button></div>
            <div id="ta-intv-wrap"><input id="ta-intv" class="input mono" placeholder="30s / 5m / 1h / 2d" /></div>
            <div id="ta-cron-wrap" hidden><input id="ta-cron" class="input mono" placeholder="${t("tasks.form.cronPlaceholder")}" /><div class="hint">${t("tasks.form.cronHint")}</div></div>
          </div>
          <div class="field"><label>${t("tasks.form.channel")}</label><select id="ta-chan" class="select"></select></div>
          <div class="field"><label>${t("tasks.form.model")}</label><select id="ta-model" class="select"></select><div class="hint">${t("tasks.form.modelHint")}</div></div>
          <div class="field"><label>${t("tasks.form.workdir")}</label><input id="ta-workdir" class="input mono" placeholder="${t("tasks.form.workdirPlaceholder")}" /><div class="hint">${t("tasks.form.workdirHint")}</div></div>
        </div>
        <div class="modal-foot"><span id="ta-msg" class="muted" style="flex:1;font-size:12.5px"></span><button id="ta-cancel" class="btn">${t("common.cancel")}</button><button id="ta-save" class="btn btn-primary">${editing ? t("common.save") : t("tasks.form.create")}</button></div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;
    // 频道下拉分四段拼：空选项 + 已配置频道 + 飞书会话组 + QQ 会话组。
    // 各段先取名再组合，而不是一长串 `+`：拼接串里夹注释会被 useTemplate 判违规，
    // 而它的自动修复会把整段压成一个模板字面量并顺手吃掉这些注释。
    const noChanOpt = `<option value="">${t("tasks.form.noChannel")}</option>`;
    const chanOpts = channels
      .map((c) => `<option value="${esc(c.name)}">${esc(c.name)}</option>`)
      .join("");
    // 飞书应用：复用扫码连接的机器人推到某会话（feishu_app:<chat_id>）。
    const feishuOpts = feishuChats.length
      ? `<optgroup label="${t("tasks.form.chanGroupFeishu")}">${feishuChats
          .map(
            (c) =>
              `<option value="feishu_app:${esc(c)}">${t("tasks.form.feishuChatOpt", { id: esc(c) })}</option>`,
          )
          .join("")}</optgroup>`
      : "";
    // QQ 机器人：主动推送到最近会话（qqbot:<scope>:<id>）。⚠️ 受 QQ 主动消息限制。
    const qqOpts = qqPeers.length
      ? `<optgroup label="${t("tasks.form.chanGroupQq")}">${qqPeers
          .map((p) => {
            const short = p.id.length > 12 ? `${p.id.slice(0, 12)}…` : p.id;
            const label =
              p.scope === "group"
                ? t("tasks.form.qqGroupOpt", { id: esc(short) })
                : t("tasks.form.qqC2cOpt", { id: esc(short) });
            return `<option value="qqbot:${esc(p.scope)}:${esc(p.id)}">${label}</option>`;
          })
          .join("")}</optgroup>`
      : "";
    q<HTMLSelectElement>("#ta-chan").innerHTML = `${noChanOpt}${chanOpts}${feishuOpts}${qqOpts}`;
    // 模型下拉：空=跟随全局默认（标注当前默认是哪个），其余为各已配置档。
    const llmLabel = (l: Llm): string => l.model || l.id;
    const defName = llms.find((l) => l.id === defaultLlm);
    q<HTMLSelectElement>("#ta-model").innerHTML =
      `<option value="">${defName ? t("tasks.form.defaultModelNamed", { name: esc(llmLabel(defName)) }) : t("tasks.form.defaultModel")}</option>${llms
        .map((l) => `<option value="${esc(l.id)}">${esc(llmLabel(l))}</option>`)
        .join("")}`;
    // 编辑：回填现有值，并按是否有 cron 决定初始调度方式。
    let mode: "intv" | "cron" = existing?.cron ? "cron" : "intv";
    if (existing) {
      q<HTMLInputElement>("#ta-name").value = existing.name;
      q<HTMLTextAreaElement>("#ta-prompt").value = existing.prompt;
      {
        // 回填通道；若目标已不在最近会话列表里，补一个原值选项避免编辑时丢通道。
        const sel = q<HTMLSelectElement>("#ta-chan");
        sel.value = existing.channel ?? "";
        if (existing.channel && sel.value !== existing.channel) {
          sel.insertAdjacentHTML(
            "beforeend",
            `<option value="${esc(existing.channel)}">${esc(existing.channel)}</option>`,
          );
          sel.value = existing.channel;
        }
      }
      q<HTMLSelectElement>("#ta-model").value = existing.model ?? "";
      q<HTMLInputElement>("#ta-workdir").value = existing.workdir ?? "";
      if (existing.cron) q<HTMLInputElement>("#ta-cron").value = existing.cron;
      else q<HTMLInputElement>("#ta-intv").value = secsToDur(existing.interval_secs);
      for (const x of overlay.querySelectorAll<HTMLElement>(".seg button"))
        x.classList.toggle("on", x.dataset.m === mode);
      q("#ta-intv-wrap").hidden = mode !== "intv";
      q("#ta-cron-wrap").hidden = mode !== "cron";
    }
    for (const b of overlay.querySelectorAll<HTMLElement>(".seg button")) {
      b.addEventListener("click", () => {
        mode = (b.dataset.m as "intv" | "cron") ?? "intv";
        for (const x of overlay.querySelectorAll(".seg button")) x.classList.toggle("on", x === b);
        q("#ta-intv-wrap").hidden = mode !== "intv";
        q("#ta-cron-wrap").hidden = mode !== "cron";
      });
    }
    const close = () => overlay.remove();
    q("#ta-cancel").addEventListener("click", close);
    // 真·模态：不做「点遮罩关闭」——本弹窗要填表，拖选文字到框外松手时 click 的 target
    // 会变成遮罩，会把填了一半的内容关掉。出口只留显式按钮。
    q<HTMLButtonElement>("#ta-save").addEventListener("click", async () => {
      const body: Record<string, unknown> = {
        name: q<HTMLInputElement>("#ta-name").value.trim(),
        prompt: q<HTMLTextAreaElement>("#ta-prompt").value.trim(),
        channel: q<HTMLSelectElement>("#ta-chan").value,
        model: q<HTMLSelectElement>("#ta-model").value,
        workdir: q<HTMLInputElement>("#ta-workdir").value.trim(),
      };
      if (mode === "cron") body.cron = q<HTMLInputElement>("#ta-cron").value.trim();
      else body.interval = q<HTMLInputElement>("#ta-intv").value.trim();
      const sched = mode === "cron" ? body.cron : body.interval;
      if (!body.name || !body.prompt || !sched) {
        q("#ta-msg").textContent = t("tasks.form.required");
        return;
      }
      const res = (await api(editing ? `/api/cron/${existing.id}` : "/api/cron", {
        method: editing ? "PATCH" : "POST",
        body: JSON.stringify(body),
      })) as { ok?: boolean; error?: string };
      if (res.ok) {
        close();
        void refresh();
      } else {
        q("#ta-msg").textContent = t("settings.oauth.failed", { e: res.error ?? "" });
      }
    });
  }

  $("#tv-add").addEventListener("click", () => openForm());
  return { refresh };
}
