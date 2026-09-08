// ── 技能（整页视图 P5）────────────────────────────────────────────────────────
// 我的技能 / 市场 两个标签。接 /api/skills/*。
//   我的技能：已安装技能（含开箱预装的内置），来源徽章 + 启用/停用开关 + 查看 SKILL.md + 卸载；
//     上方一条工具条做「分类标签 + 搜索」——技能上百个之后，光靠分组标题根本找不着东西。
//   市场：下拉切换源（static / clawhub / 自定义）+ 搜索，按当前源列出并一键安装。
// 入口：创建技能（5 步向导）/ 从 Git 导入 / 从 openclaw 迁移（自动探测 + 勾选导入）。

import { authHeaders } from "./auth";
import { httpBase } from "./backend";
import { type MessageKey, getLocale, t } from "./i18n";
import { icon } from "./icons";

interface MineEntry {
  name: string;
  description: string;
  source: string; // builtin | workdir | installed，见 [`SkillCat`]
  enabled: boolean;
}
interface MarketEntry {
  name: string;
  description: string;
  version?: string | null;
  source: string; // 源标签
  installed: boolean;
}
interface Source {
  label: string;
  url: string;
  kind: "static" | "clawhub";
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

// 「我的技能」来源 → 展示（标签 key / badge 样式 / 图标）。标签在渲染时按当前语言解析。
const SRC_META: Record<string, { labelKey: MessageKey; cls: string; ico: string }> = {
  builtin: { labelKey: "skills.src.builtin", cls: "accent", ico: "bolt" },
  installed: { labelKey: "skills.src.installed", cls: "", ico: "code" },
  workdir: { labelKey: "skills.src.workdir", cls: "", ico: "folder" },
};

/**
 * 「我的技能」的分类维度，与后端 `/api/skills/catalog` 的 `source` 一一对应：
 * `builtin` 随程序分发，`workdir` 是 `<工作目录>/skills` 里的项目级技能（agent 自己写的
 * SKILL.md 就落这儿），其余（Git 导入 / 市场安装 / openclaw 迁移 / 创建向导）统称 `installed`。
 *
 * 注意 `installed` 这一堆内部**分不出来源**：四条安装路径都写进同一个数据目录且不留任何
 * 来源记录，所以「装来的」和「自己造的」在这一档里无法再细分。要细分得先在安装时打戳。
 */
export type SkillCat = "all" | "builtin" | "workdir" | "installed";
export type SkillCatReal = Exclude<SkillCat, "all">;

interface Filterable {
  name: string;
  description: string;
  source: string;
}

/** 来源 → 分类。认不出的来源一律归入 installed，后端将来多一种安装方式也不会凭空消失。 */
function catOf(e: Filterable): SkillCatReal {
  if (e.source === "builtin") return "builtin";
  if (e.source === "workdir") return "workdir";
  return "installed";
}

/**
 * 按分类 + 关键词筛选技能，两个条件取「与」。
 *
 * 关键词在**名称和描述里都找**、不区分大小写：一百多个技能里要找的那个，多半只记得
 * 它是干什么的，记不住 slug 叫什么。纯空白查询按「没查询」处理，否则误敲一个空格就
 * 把整页清空、看起来像页面坏了。
 */
export function filterSkills<T extends Filterable>(
  entries: T[],
  cat: SkillCat,
  query: string,
): T[] {
  const q = query.trim().toLowerCase();
  return entries.filter((e) => {
    if (cat !== "all" && catOf(e) !== cat) return false;
    if (!q) return true;
    return e.name.toLowerCase().includes(q) || (e.description ?? "").toLowerCase().includes(q);
  });
}

/**
 * 各分类在当前关键词下的**命中数**，用于分类标签上的角标。
 *
 * 计命中数而不是总数是有意的：搜 "feishu" 时标签直接显示「内置 0 / 其它 3」，
 * 一眼就知道要找的在哪一类，不用挨个点过去试。
 */
export function skillCounts<T extends Filterable>(
  entries: T[],
  query: string,
): Record<SkillCat, number> {
  const hit = filterSkills(entries, "all", query);
  const n = (c: SkillCatReal) => hit.filter((e) => catOf(e) === c).length;
  return {
    all: hit.length,
    builtin: n("builtin"),
    workdir: n("workdir"),
    installed: n("installed"),
  };
}

/** 技能名按当前界面语言做不区分大小写的字母排序（localeCompare 对中文等也稳定）。 */
function byName(a: MineEntry, b: MineEntry): number {
  try {
    return a.name.localeCompare(b.name, getLocale(), { sensitivity: "base" });
  } catch {
    return a.name.toLowerCase().localeCompare(b.name.toLowerCase());
  }
}

/**
 * @param getWorkdir 当前会话的工作目录（可选）。传了才会把 `<工作目录>/skills` 里的项目级
 *   技能一并列出——后端要靠 `?workdir=` 才知道去哪找，不传就只有全局数据目录那一份。
 */
export function mountSkillsView(
  container: HTMLElement,
  getWorkdir?: () => string,
): { refresh: () => void } {
  container.innerHTML = `
    <div class="page page-wide">
      <div class="section-head">
        <h2>${t("nav.skills")}</h2>
        <div class="desc">${t("skills.desc")}</div>
        <span class="spacer" style="flex:1"></span>
        <button id="sk-migrate" class="btn btn-sm">${icon("refresh", 16)}${t("skills.migrate")}</button>
        <button id="sk-install" class="btn btn-sm">${icon("code", 16)}${t("skills.import")}</button>
        <button id="sk-create" class="btn btn-primary btn-sm">${icon("plus", 16)}${t("skills.create")}</button>
      </div>
      <div class="seg tabs">
        <button data-tab="mine" class="on">${t("skills.tab.mine")}</button>
        <button data-tab="market">${t("skills.tab.market")}</button>
      </div>
      <div id="sk-mine-bar" class="market-bar">
        <span class="seg sk-cats">
          <button data-cat="all" class="on">${t("skills.filter.all")}<span class="sk-cat-n"></span></button>
          <button data-cat="builtin">${t("skills.filter.builtin")}<span class="sk-cat-n"></span></button>
          <button data-cat="workdir" hidden>${t("skills.filter.workdir")}<span class="sk-cat-n"></span></button>
          <button data-cat="installed">${t("skills.filter.installed")}<span class="sk-cat-n"></span></button>
        </span>
        <div class="market-search">
          <span class="market-search-ico">${icon("search", 15)}</span>
          <input id="sk-mine-q" class="input" type="text" autocomplete="off" spellcheck="false"
                 placeholder="${t("skills.searchMine")}" aria-label="${t("skills.searchMine")}" />
          <button id="sk-mine-clear" class="market-search-clear" hidden
                  title="${t("skills.clearSearch")}" aria-label="${t("skills.clearSearch")}">${icon("x", 14)}</button>
        </div>
      </div>
      <div id="sk-market-bar" class="market-bar" hidden>
        <label class="market-src">
          <span class="t3">${t("skills.source")}</span>
          <select id="sk-source" class="select"></select>
        </label>
        <div class="market-search">
          <span class="market-search-ico">${icon("search", 15)}</span>
          <input id="sk-q" class="input" placeholder="${t("skills.searchPlaceholder")}" />
        </div>
      </div>
      <div id="sk-grid" class="grid grid-3"></div>
      <div id="sk-status" class="muted" style="font-size:13px;margin-top:14px"></div>
    </div>`;

  const $ = <T extends HTMLElement>(s: string) => container.querySelector(s) as T;
  const setStatus = (s: string) => {
    $("#sk-status").textContent = s;
  };
  let mineEntries: MineEntry[] = [];
  let marketEntries: MarketEntry[] = [];
  let sources: Source[] = [];
  let tab: "mine" | "market" = "mine";
  let marketLoaded = false;
  let cat: SkillCat = "all";
  let mineQuery = "";

  function mineCard(e: MineEntry): HTMLElement {
    const meta = SRC_META[e.source];
    const label = meta ? t(meta.labelKey) : e.source;
    const cls = meta?.cls ?? "";
    const ico = meta?.ico ?? "skill";
    const card = document.createElement("div");
    card.className = `skill-card${e.enabled ? "" : " off"}`;
    card.innerHTML = `
      <div class="sk-top">
        <span class="sk-ico">${icon(ico, 18)}</span>
        <button class="toggle ${e.enabled ? "on" : ""}" data-act="toggle" title="${e.enabled ? t("skills.toggle.on") : t("skills.toggle.off")}"></button>
      </div>
      <div class="sk-name">${esc(e.name)}</div>
      <div class="sk-desc">${esc(e.description || t("skills.noDesc"))}</div>
      <div class="sk-foot">
        <span class="badge ${cls}">${label}</span>
        <button class="btn btn-sm btn-icon" data-act="view" title="${t("skills.viewSkillMd")}" style="margin-left:auto">${icon("doc", 15)}</button>
        <button class="btn btn-sm btn-danger" data-act="uninstall">${t("skills.uninstall")}</button>
      </div>`;
    const q = (s: string) => card.querySelector(s) as HTMLElement;
    q('[data-act="toggle"]').onclick = async () => {
      setStatus(
        t(e.enabled ? "skills.status.disabling" : "skills.status.enabling", { name: e.name }),
      );
      await api(`/api/skills/${encodeURIComponent(e.name)}/enabled`, {
        method: "POST",
        body: JSON.stringify({ enabled: !e.enabled }),
      });
      void refresh();
    };
    q('[data-act="view"]').onclick = () => void openDetail(e.name);
    q('[data-act="uninstall"]').onclick = async () => {
      setStatus(t("skills.status.uninstalling", { name: e.name }));
      await api(`/api/skills/${encodeURIComponent(e.name)}`, { method: "DELETE" });
      void refresh();
    };
    return card;
  }

  function marketCard(e: MarketEntry): HTMLElement {
    const card = document.createElement("div");
    card.className = "skill-card";
    const ver = e.version ? `<span class="chip mono">v${esc(e.version)}</span>` : "";
    card.innerHTML = `
      <div class="sk-top">
        <span class="sk-ico">${icon("globe", 18)}</span>
        <span class="badge">${esc(e.source)}</span>
      </div>
      <div class="sk-name">${esc(e.name)}</div>
      <div class="sk-desc">${esc(e.description || t("skills.noDesc"))}</div>
      <div class="sk-foot">
        ${ver}
        <button class="btn btn-sm ${e.installed ? "" : "btn-primary"}" data-act="install" style="margin-left:auto" ${e.installed ? "disabled" : ""}>${e.installed ? t("skills.installed") : t("skills.install")}</button>
      </div>`;
    const btn = card.querySelector('[data-act="install"]') as HTMLButtonElement;
    if (!e.installed) {
      btn.onclick = async () => {
        btn.disabled = true;
        setStatus(t("skills.status.installing", { name: e.name }));
        const r = (await api("/api/skills/install", {
          method: "POST",
          body: JSON.stringify({ name: e.name, version: e.version ?? undefined }),
        })) as { ok?: boolean; error?: string };
        setStatus(
          r.ok
            ? t("skills.status.installed", { name: e.name })
            : t("skills.status.installFailed", { e: r.error ?? "" }),
        );
        await refresh();
        await refreshMarket();
      };
    }
    return card;
  }

  /** 「我的技能」：刷新分类角标，再按 分类 ∧ 关键词 铺卡片。 */
  function renderMine(grid: HTMLElement): void {
    const counts = skillCounts(mineEntries, mineQuery);
    // 项目技能是可选的（没设工作目录、或目录里没有 skills/ 就一个都没有），此时那一档
    // 恒为 0，摆着只是噪音——直接收起来。收起时若正停在该档，退回「全部」，否则会卡在
    // 一个看不见的分类上、页面空白得像坏了。
    const hasWorkdir = mineEntries.some((e) => e.source === "workdir");
    if (!hasWorkdir && cat === "workdir") cat = "all";
    for (const b of container.querySelectorAll<HTMLElement>(".sk-cats button")) {
      const c = (b.dataset.cat ?? "all") as SkillCat;
      if (c === "workdir") b.hidden = !hasWorkdir;
      (b.querySelector(".sk-cat-n") as HTMLElement).textContent = String(counts[c]);
      b.classList.toggle("on", c === cat);
    }

    if (mineEntries.length === 0) {
      grid.innerHTML = `<div class="empty" style="grid-column:1/-1">${t("skills.empty.mine")}</div>`;
      return;
    }
    const hits = filterSkills(mineEntries, cat, mineQuery).sort(byName);
    if (hits.length === 0) {
      // 空态分两种：一个技能都没有 vs 搜了没命中——提示得不一样，否则用户以为技能丢了。
      grid.innerHTML = `<div class="empty" style="grid-column:1/-1">${esc(
        t("skills.empty.noMatch", { q: mineQuery.trim() }),
      )}</div>`;
      return;
    }
    // 单独一类时组头就是废话（标签已经写着了），直接铺卡片；「全部」下才保留组头分段。
    if (cat !== "all") {
      for (const e of hits) grid.appendChild(mineCard(e));
      return;
    }
    const groups: { cat: SkillCatReal; labelKey: MessageKey; entries: MineEntry[] }[] = [
      { cat: "workdir", labelKey: "skills.group.workdir", entries: [] },
      { cat: "builtin", labelKey: "skills.group.builtin", entries: [] },
      { cat: "installed", labelKey: "skills.group.installed", entries: [] },
    ];
    for (const e of hits) {
      groups.find((x) => x.cat === catOf(e))?.entries.push(e);
    }
    for (const g of groups) {
      if (g.entries.length === 0) continue;
      const head = document.createElement("div");
      head.className = "sk-group-head";
      head.innerHTML = `${esc(t(g.labelKey))}<span class="sk-group-count">${g.entries.length}</span>`;
      grid.appendChild(head);
      for (const e of g.entries) grid.appendChild(mineCard(e));
    }
  }

  function render(): void {
    const grid = $("#sk-grid");
    grid.replaceChildren();
    $("#sk-market-bar").hidden = tab !== "market";
    $("#sk-mine-bar").hidden = tab !== "mine";
    if (tab === "mine") {
      renderMine(grid);
    } else {
      if (marketEntries.length === 0) {
        grid.innerHTML = `<div class="empty" style="grid-column:1/-1">${t("skills.empty.market")}</div>`;
        return;
      }
      for (const e of marketEntries) grid.appendChild(marketCard(e));
    }
  }

  async function refresh(): Promise<void> {
    setStatus(t("settings.status.loading"));
    try {
      const wd = getWorkdir?.().trim() ?? "";
      const qs = wd ? `?workdir=${encodeURIComponent(wd)}` : "";
      mineEntries = ((await api(`/api/skills/catalog${qs}`)).entries as MineEntry[]) ?? [];
      if (tab === "mine") render();
      setStatus("");
    } catch (e) {
      setStatus(t("settings.status.loadFailed", { e: String(e) }));
    }
  }

  async function loadSources(): Promise<void> {
    const res = (await api("/api/skills/sources")) as { sources?: Source[]; current?: Source };
    sources = res.sources ?? [];
    const sel = $<HTMLSelectElement>("#sk-source");
    sel.innerHTML = `${sources
      .map((s, i) => `<option value="${i}">${esc(s.label)}</option>`)
      .join("")}<option value="__custom__">${t("skills.customSource")}</option>`;
    const curIdx = sources.findIndex((s) => s.url === res.current?.url);
    if (curIdx >= 0) sel.value = String(curIdx);
  }

  async function refreshMarket(): Promise<void> {
    setStatus(t("skills.loadingMarket"));
    try {
      const q = $<HTMLInputElement>("#sk-q").value.trim();
      const res = (await api(`/api/skills/market${q ? `?q=${encodeURIComponent(q)}` : ""}`)) as {
        entries?: MarketEntry[];
      };
      marketEntries = res.entries ?? [];
      if (tab === "market") render();
      setStatus("");
    } catch (e) {
      setStatus(t("skills.marketLoadFailed", { e: String(e) }));
    }
  }

  // 切换市场源（持久化后刷新列表）。
  async function switchSource(url: string, kind: "static" | "clawhub"): Promise<void> {
    setStatus(t("skills.switchingSource"));
    await api("/api/skills/source", { method: "POST", body: JSON.stringify({ url, kind }) });
    await refreshMarket();
  }

  $<HTMLSelectElement>("#sk-source").addEventListener("change", () => {
    const sel = $<HTMLSelectElement>("#sk-source");
    if (sel.value === "__custom__") {
      const url = window.prompt(t("skills.customSourcePrompt"), "https://…/registry.json");
      if (url?.trim()) {
        void switchSource(url.trim(), "static").then(loadSources);
      } else {
        void loadSources(); // 取消则恢复选中
      }
      return;
    }
    const s = sources[Number(sel.value)];
    if (s) void switchSource(s.url, s.kind);
  });

  let qTimer: ReturnType<typeof setTimeout> | undefined;
  $<HTMLInputElement>("#sk-q").addEventListener("input", () => {
    clearTimeout(qTimer);
    qTimer = setTimeout(() => void refreshMarket(), 250);
  });

  // ── 「我的技能」分类 + 搜索 ──
  // 两者都是纯前端过滤（catalog 一次就全拿回来了），所以不防抖、按键即出结果。
  // 只重建 #sk-grid、不动工具条，因此输入框焦点和光标位置不会被打断。
  for (const b of container.querySelectorAll<HTMLElement>(".sk-cats button")) {
    b.addEventListener("click", () => {
      cat = (b.dataset.cat as SkillCat) ?? "all";
      render();
    });
  }
  const mineQ = $<HTMLInputElement>("#sk-mine-q");
  const applyQuery = (v: string): void => {
    mineQuery = v;
    $("#sk-mine-clear").hidden = v === "";
    render();
  };
  mineQ.addEventListener("input", () => applyQuery(mineQ.value));
  mineQ.addEventListener("keydown", (ev) => {
    if (ev.key !== "Escape" || mineQ.value === "") return;
    ev.stopPropagation(); // 别让 Esc 冒到全局快捷键上：这一下只是清搜索框
    mineQ.value = "";
    applyQuery("");
  });
  $("#sk-mine-clear").addEventListener("click", () => {
    mineQ.value = "";
    applyQuery("");
    mineQ.focus();
  });

  // 查看 SKILL.md 全文。
  async function openDetail(name: string): Promise<void> {
    const res = (await api(`/api/skills/${encodeURIComponent(name)}`)) as {
      ok?: boolean;
      content?: string;
      error?: string;
    };
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    overlay.innerHTML = `
      <div class="modal">
        <div class="modal-head">
          <span class="ico-tile">${icon("doc", 18)}</span>
          <div><h3>${esc(name)}</h3><div class="t3 mono" style="font-size:12px">skills/${esc(name)}/SKILL.md</div></div>
          <span class="spacer" style="flex:1"></span>
          <button class="btn btn-sm btn-icon" id="d-close">${icon("x", 17)}</button>
        </div>
        <div class="modal-body">
          <pre class="ap-code" style="margin:0;max-height:360px;overflow:auto">${esc(res.content ?? res.error ?? t("skills.cannotRead"))}</pre>
        </div>
      </div>`;
    document.body.appendChild(overlay);
    const close = () => overlay.remove();
    (overlay.querySelector("#d-close") as HTMLElement).onclick = close;
    // 真·模态：不做「点遮罩关闭」——本弹窗要填表，拖选文字到框外松手时 click 的 target
    // 会变成遮罩，会把填了一半的内容关掉。出口只留显式按钮。
  }

  for (const b of container.querySelectorAll<HTMLElement>(".tabs button")) {
    b.addEventListener("click", () => {
      tab = (b.dataset.tab as "mine" | "market") ?? "mine";
      for (const x of container.querySelectorAll(".tabs button")) x.classList.toggle("on", x === b);
      render();
      if (tab === "market" && !marketLoaded) {
        marketLoaded = true;
        void loadSources().then(refreshMarket);
      }
    });
  }

  // 从 Git 仓库导入技能。
  function openInstall(): void {
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    overlay.innerHTML = `
      <div class="modal">
        <div class="modal-head"><span class="ico-tile">${icon("code", 18)}</span><h3>${t("skills.import.title")}</h3></div>
        <div class="modal-body">
          <div class="field"><label>${t("skills.import.urlLabel")}</label><input id="it-url" class="input mono" placeholder="https://github.com/obra/superpowers" /></div>
          <div class="field"><label>${t("skills.import.subLabel")}</label><input id="it-sub" class="input mono" placeholder="${t("skills.import.subPlaceholder")}" /></div>
          <div class="inline-note">${t("skills.import.note")}</div>
        </div>
        <div class="modal-foot"><span id="it-msg" class="muted" style="flex:1;font-size:12.5px"></span><button id="it-cancel" class="btn">${t("common.cancel")}</button><button id="it-ok" class="btn btn-primary">${t("skills.import.ok")}</button></div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;
    const close = () => overlay.remove();
    q("#it-cancel").addEventListener("click", close);
    // 真·模态：不做「点遮罩关闭」——本弹窗要填表，拖选文字到框外松手时 click 的 target
    // 会变成遮罩，会把填了一半的内容关掉。出口只留显式按钮。
    q<HTMLButtonElement>("#it-ok").addEventListener("click", async () => {
      const url = q<HTMLInputElement>("#it-url").value.trim();
      if (!url) {
        q("#it-msg").textContent = t("skills.import.needUrl");
        return;
      }
      q("#it-msg").textContent = t("skills.import.cloning");
      const r = (await api("/api/skills/git", {
        method: "POST",
        body: JSON.stringify({ url, subdir: q<HTMLInputElement>("#it-sub").value.trim() }),
      })) as { ok?: boolean; imported?: string[]; error?: string };
      if (r.ok) {
        close();
        void refresh();
      } else {
        q("#it-msg").textContent = t("settings.oauth.failed", { e: r.error ?? "" });
      }
    });
  }

  // 从 openclaw 迁移技能（自动探测 + 勾选导入）。
  async function openMigrate(): Promise<void> {
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    overlay.innerHTML = `
      <div class="modal wide">
        <div class="modal-head"><span class="ico-tile">${icon("refresh", 18)}</span><h3>${t("skills.migrate.title")}</h3>
          <span class="spacer" style="flex:1"></span>
          <button class="btn btn-sm btn-icon" id="mg-close">${icon("x", 17)}</button>
        </div>
        <div class="modal-body">
          <div class="inline-note">${t("skills.migrate.note")}</div>
          <div id="mg-list" class="mg-list">${t("skills.migrate.scanning")}</div>
        </div>
        <div class="modal-foot"><span id="mg-msg" class="muted" style="flex:1;font-size:12.5px"></span><button id="mg-cancel" class="btn">${t("common.cancel")}</button><button id="mg-ok" class="btn btn-primary">${t("skills.migrate.importSelected")}</button></div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;
    const close = () => overlay.remove();
    q("#mg-close").addEventListener("click", close);
    q("#mg-cancel").addEventListener("click", close);
    // 真·模态：不做「点遮罩关闭」——本弹窗要填表，拖选文字到框外松手时 click 的 target
    // 会变成遮罩，会把填了一半的内容关掉。出口只留显式按钮。

    const res = (await api("/api/skills/openclaw/scan")) as {
      candidates?: { name: string; path: string; installed: boolean }[];
    };
    const cands = res.candidates ?? [];
    const listEl = q("#mg-list");
    if (cands.length === 0) {
      listEl.innerHTML = `<div class="empty">${t("skills.migrate.empty")}</div>`;
      return;
    }
    listEl.replaceChildren();
    for (const c of cands) {
      const row = document.createElement("label");
      row.className = "mg-row";
      row.innerHTML = `
        <input type="checkbox" value="${esc(c.path)}" ${c.installed ? "" : "checked"} />
        <span class="mg-name">${esc(c.name)}</span>
        ${c.installed ? `<span class="badge">${t("skills.migrate.exists")}</span>` : ""}
        <span class="mg-path mono t3">${esc(c.path)}</span>`;
      listEl.appendChild(row);
    }
    q<HTMLButtonElement>("#mg-ok").addEventListener("click", async () => {
      const paths = [
        ...overlay.querySelectorAll<HTMLInputElement>("input[type=checkbox]:checked"),
      ].map((x) => x.value);
      if (paths.length === 0) {
        q("#mg-msg").textContent = t("skills.migrate.needOne");
        return;
      }
      q("#mg-msg").textContent = t("skills.migrate.importing");
      const r = (await api("/api/skills/openclaw/import", {
        method: "POST",
        body: JSON.stringify({ paths }),
      })) as { ok?: boolean; imported?: number; total?: number };
      close();
      setStatus(t("skills.migrate.done", { n: r.imported ?? 0, total: r.total ?? paths.length }));
      void refresh();
    });
  }

  // ── 创建技能向导（5 步）──
  const ALL_TOOLS = [
    "read",
    "write",
    "edit",
    "glob",
    "grep",
    "shell",
    "web_fetch",
    "web_search",
    "todo",
    "notify",
    "invoke_skill",
    "task",
  ];
  function openCreator(): void {
    const draft = { slug: "", description: "", trigger: "", body: "", tools: [] as string[] };
    const steps = [
      t("skills.creator.step.basic"),
      t("skills.creator.step.trigger"),
      t("skills.creator.step.body"),
      t("skills.creator.step.tools"),
      t("skills.creator.step.preview"),
    ];
    let step = 0;
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    overlay.innerHTML = `
      <div class="modal wide">
        <div class="modal-head"><h3>${t("skills.create")}</h3></div>
        <div class="modal-body">
          <div class="stepper" id="cw-steps"></div>
          <div id="cw-pane"></div>
        </div>
        <div class="modal-foot">
          <span id="cw-msg" class="muted" style="flex:1;font-size:12.5px"></span>
          <button id="cw-prev" class="btn">${t("skills.creator.prev")}</button>
          <button id="cw-next" class="btn btn-primary">${t("skills.creator.next")}</button>
        </div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;

    function renderSteps(): void {
      q("#cw-steps").innerHTML = steps
        .map((s, i) => {
          const cls = i === step ? "step active" : i < step ? "step done" : "step";
          const line = i < steps.length - 1 ? '<div class="step-line"></div>' : "";
          return `<div class="${cls}"><span class="num">${i + 1}</span><span>${s}</span></div>${line}`;
        })
        .join("");
    }
    function renderPane(): void {
      const pane = q("#cw-pane");
      if (step === 0) {
        pane.innerHTML = `
          <div class="field"><label>${t("skills.creator.slugLabel")}</label><div class="input-group"><span class="input-affix">skills/</span><input id="cw-slug" class="input mono" placeholder="weekly-report" /></div></div>
          <div class="field"><label>${t("skills.creator.descLabel")}</label><input id="cw-desc" class="input" placeholder="${t("skills.creator.descPlaceholder")}" /></div>`;
        q<HTMLInputElement>("#cw-slug").value = draft.slug;
        q<HTMLInputElement>("#cw-desc").value = draft.description;
      } else if (step === 1) {
        pane.innerHTML = `<div class="field"><label>${t("skills.creator.triggerLabel")}</label><textarea id="cw-trig" class="textarea" rows="4" placeholder="${t("skills.creator.triggerPlaceholder")}"></textarea></div>`;
        q<HTMLTextAreaElement>("#cw-trig").value = draft.trigger;
      } else if (step === 2) {
        pane.innerHTML = `<div class="field"><label>${t("skills.creator.bodyLabel")}</label><textarea id="cw-body" class="textarea mono" rows="9" placeholder="${t("skills.creator.bodyPlaceholder")}"></textarea></div>`;
        q<HTMLTextAreaElement>("#cw-body").value = draft.body;
      } else if (step === 3) {
        pane.innerHTML = `<div class="field"><label>${t("skills.creator.toolsLabel")}</label><div class="tool-grid" id="cw-tools"></div></div>`;
        const grid = q("#cw-tools");
        for (const t of ALL_TOOLS) {
          const b = document.createElement("button");
          b.className = `tool-pick${draft.tools.includes(t) ? " on" : ""}`;
          b.textContent = t;
          b.onclick = () => {
            if (draft.tools.includes(t)) draft.tools = draft.tools.filter((x) => x !== t);
            else draft.tools.push(t);
            b.classList.toggle("on");
          };
          grid.appendChild(b);
        }
      } else {
        const md = [
          `---\nname: ${draft.slug}\ndescription: ${draft.description}\n---`,
          draft.body.trim(),
          draft.trigger.trim() ? `\n${t("skills.creator.mdWhenUse")}\n${draft.trigger.trim()}` : "",
          draft.tools.length ? `\n${t("skills.creator.mdTools")}\n${draft.tools.join(", ")}` : "",
        ]
          .filter(Boolean)
          .join("\n");
        pane.innerHTML = `<div class="hint" style="margin-bottom:8px">${t("skills.creator.previewLabel")}</div><pre class="ap-code" style="border-radius:10px;max-height:320px">${esc(md)}</pre>`;
      }
      q<HTMLButtonElement>("#cw-prev").disabled = step === 0;
      q("#cw-next").textContent =
        step === steps.length - 1 ? t("skills.creator.save") : t("skills.creator.next");
    }
    function capture(): void {
      if (step === 0) {
        draft.slug = q<HTMLInputElement>("#cw-slug")?.value.trim() ?? draft.slug;
        draft.description = q<HTMLInputElement>("#cw-desc")?.value.trim() ?? draft.description;
      } else if (step === 1) {
        draft.trigger = q<HTMLTextAreaElement>("#cw-trig")?.value ?? draft.trigger;
      } else if (step === 2) {
        draft.body = q<HTMLTextAreaElement>("#cw-body")?.value ?? draft.body;
      }
    }
    const close = () => overlay.remove();
    // 真·模态：不做「点遮罩关闭」——本弹窗要填表，拖选文字到框外松手时 click 的 target
    // 会变成遮罩，会把填了一半的内容关掉。出口只留显式按钮。
    q("#cw-prev").addEventListener("click", () => {
      capture();
      if (step > 0) step--;
      renderSteps();
      renderPane();
    });
    q("#cw-next").addEventListener("click", async () => {
      capture();
      if (step === 0 && !draft.slug) {
        q("#cw-msg").textContent = t("skills.creator.needSlug");
        return;
      }
      if (step < steps.length - 1) {
        step++;
        renderSteps();
        renderPane();
        q("#cw-msg").textContent = "";
        return;
      }
      // 保存
      q("#cw-msg").textContent = t("settings.status.saving");
      const res = (await api("/api/skills/create", {
        method: "POST",
        body: JSON.stringify(draft),
      })) as { ok?: boolean; error?: string };
      if (res.ok) {
        close();
        void refresh();
      } else {
        q("#cw-msg").textContent = t("settings.oauth.failed", { e: res.error ?? "" });
      }
    });
    renderSteps();
    renderPane();
  }

  $("#sk-create").addEventListener("click", openCreator);
  $("#sk-install").addEventListener("click", openInstall);
  $("#sk-migrate").addEventListener("click", () => void openMigrate());
  return { refresh };
}
