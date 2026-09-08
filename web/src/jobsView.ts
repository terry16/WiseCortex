// ── 后台任务（整页视图）────────────────────────────────────────────────────
// 列出 task_start 启动的后台长任务，可查看状态/结果、可停止。接 /api/jobs。

import { authHeaders } from "./auth";
import { httpBase } from "./backend";
import { t } from "./i18n";

interface Job {
  id: string;
  description: string;
  status: string; // running | done | failed | stopped
  created_ms: number;
  output: string;
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
function ago(ms: number): string {
  const s = Math.max(0, Math.floor((Date.now() - ms) / 1000));
  if (s < 60) return t("jobs.secAgo", { n: s });
  if (s < 3600) return t("jobs.minAgo", { n: Math.floor(s / 60) });
  if (s < 86400) return t("jobs.hourAgo", { n: Math.floor(s / 3600) });
  return t("jobs.dayAgo", { n: Math.floor(s / 86400) });
}

export function mountJobsView(container: HTMLElement): { refresh: () => void } {
  container.innerHTML = `
    <div class="page">
      <div class="set-block-head">
        <span class="ico-tile" data-icon="clock"></span>
        <div><h2>${t("nav.jobs")}</h2><div class="sub">${t("jobs.sub")}</div></div>
      </div>
      <div id="jobs-list"></div>
    </div>`;
  const list = container.querySelector("#jobs-list") as HTMLElement;
  let timer: ReturnType<typeof setInterval> | null = null;

  async function load(): Promise<void> {
    let jobs: Job[] = [];
    try {
      jobs = ((await api("/api/jobs")) as { jobs?: Job[] }).jobs ?? [];
    } catch {
      jobs = [];
    }
    jobs.sort((a, b) => b.created_ms - a.created_ms);
    if (jobs.length === 0) {
      list.innerHTML = `<div class="empty">${t("jobs.empty")}</div>`;
      return;
    }
    list.replaceChildren();
    for (const j of jobs) {
      const card = document.createElement("div");
      card.className = "card card-pad";
      card.style.marginBottom = "10px";
      const badge =
        j.status === "running"
          ? `<span class="badge">${t("jobs.running")}</span>`
          : j.status === "done"
            ? `<span class="badge green">${t("jobs.completed")}</span>`
            : `<span class="badge">${esc(j.status)}</span>`;
      const out = j.output
        ? `<pre class="tool-result" style="margin-top:10px;max-height:240px;overflow:auto">${esc(j.output)}</pre>`
        : "";
      card.innerHTML = `
        <div style="display:flex;align-items:center;gap:10px">
          <strong>${esc(j.description || j.id)}</strong>
          ${badge}
          <span class="t3 mono">${esc(j.id)}</span>
          <span class="t3">· ${ago(j.created_ms)}</span>
          <span class="spacer" style="flex:1"></span>
          ${j.status === "running" ? `<button class="btn btn-sm btn-danger" data-stop="${esc(j.id)}">${t("composer.stop")}</button>` : ""}
        </div>
        ${out}`;
      list.appendChild(card);
    }
    for (const b of list.querySelectorAll<HTMLElement>("[data-stop]")) {
      b.addEventListener("click", () => {
        const id = b.dataset.stop;
        if (!id) return;
        void api(`/api/jobs/${encodeURIComponent(id)}`, { method: "DELETE" }).then(() => load());
      });
    }
  }

  return {
    refresh: () => {
      void load();
      // 视图显示时轮询；切走时停止（setView 会再次 refresh 重启）。
      if (timer) clearInterval(timer);
      timer = setInterval(() => {
        if (container.hidden) {
          if (timer) clearInterval(timer);
          timer = null;
          return;
        }
        void load();
      }, 3000);
    },
  };
}
