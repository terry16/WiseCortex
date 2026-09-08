// ── 通知通道（整页视图 P6）────────────────────────────────────────────────────
// 平台卡片（飞书/微信 ClawBot/企业微信/QQ/Webhook）+ 配置弹窗。
// 飞书本地优先「长连接」（免公网）；微信 ClawBot 走 iLink 长轮询（同样免公网，扫码接入）；
// 企业微信入站凭据在 UI 配置（需公网 HTTPS 回调）。
// 出站通道接 /api/channels；回调地址仅公网部署时需要。

import { authHeaders } from "./auth";
import { httpBase } from "./backend";
import { type MessageKey, t } from "./i18n";
import { copyText } from "./platform";

interface Channel {
  name: string;
  kind: string;
  url: string;
  target?: string | null;
}

interface Platform {
  kind: string;
  nameKey: MessageKey; // 名称/描述按当前语言在渲染时解析（PLATFORMS 在模块加载时已建）
  abbr: string;
  color: string;
  descKey: MessageKey;
  callback?: string; // 入站回调路径
}

const PLATFORMS: Platform[] = [
  {
    kind: "feishu",
    nameKey: "channels.platform.feishu.name",
    abbr: "Fs",
    color: "#3370ff",
    descKey: "channels.platform.feishu.desc",
    callback: "/api/im/feishu",
  },
  {
    // 微信 ClawBot（腾讯 2026-03 通过 OpenClaw 开放的个人号 Bot API，协议 iLink）。
    // 纯 HTTPS 长轮询，免公网回调，因此桌面端（Win/macOS）同样可用。
    // 双向，由 clawbot.json 状态驱动（bot_token + 轮询开关），不建出站通道、无回调地址。
    kind: "clawbot",
    nameKey: "channels.platform.clawbot.name",
    abbr: "微",
    color: "#1aad19",
    descKey: "channels.platform.clawbot.desc",
  },
  {
    kind: "wecom",
    nameKey: "channels.platform.wecom.name",
    abbr: "企",
    color: "#07c160",
    descKey: "channels.platform.wecom.desc",
    callback: "/api/im/wecom",
  },
  {
    kind: "onebot",
    nameKey: "channels.platform.onebot.name",
    abbr: "Q",
    color: "#12b7f5",
    descKey: "channels.platform.onebot.desc",
    callback: "/api/im/onebot",
  },
  {
    // QQ 官方机器人（开放平台 AppID/AppSecret，WebSocket 网关，免公网回调）。
    // 双向，由 qq.json 状态驱动（凭据 + 连接开关），不建出站 /api/channels 条目、无回调地址。
    kind: "qqbot",
    nameKey: "channels.platform.qqbot.name",
    abbr: "QQ",
    color: "#1479d7",
    descKey: "channels.platform.qqbot.desc",
  },
  {
    kind: "email",
    nameKey: "channels.platform.email.name",
    abbr: "@",
    color: "#d97706",
    descKey: "channels.platform.email.desc",
  },
  {
    kind: "webhook",
    nameKey: "channels.platform.webhook.name",
    abbr: "{}",
    color: "#7a7a86",
    descKey: "channels.platform.webhook.desc",
  },
];

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

// 入站回调地址展示。飞书优先长连接，故把回调折叠为「公网部署（高级）」；
// 企业微信只能走公网回调，明确标注需公网 HTTPS。
function callbackHtml(p: Platform): string {
  if (!p.callback) return "";
  // 公网回调地址：浏览器用页面同源（经 nginx），Tauri 用内嵌后端。
  const url = `${httpBase() || location.origin}${p.callback}`;
  const copyBtn = `<div class="callback-url"><span class="mono">${esc(url)}</span><button class="btn btn-sm" data-copy="${esc(url)}">${t("common.copy")}</button></div>`;
  if (p.kind === "feishu") {
    return `<details class="callback-box"><summary class="t3" style="font-size:12px;cursor:pointer">${t("channels.callback.feishuSummary")}</summary>
      <div style="margin-top:8px">${copyBtn}</div>
      <div class="inline-note" style="margin-top:6px">${t("channels.callback.feishuNote")}</div></details>`;
  }
  const hint =
    p.kind === "wecom" ? t("channels.callback.wecomHint") : t("channels.callback.genericHint");
  return `<div class="callback-box"><div class="t3" style="font-size:12px;margin-bottom:6px">${t("channels.callback.label")}${esc(hint)}</div>${copyBtn}</div>`;
}

export function mountChannelsView(container: HTMLElement): { refresh: () => void } {
  container.innerHTML = `
    <div class="page">
      <div class="section-head">
        <h2>${t("nav.channels")}</h2>
        <div class="desc">${t("channels.desc")}</div>
      </div>
      <div class="grid grid-2" id="cv-grid"></div>
      <div id="cv-status" class="muted" style="font-size:13px;margin-top:14px"></div>
    </div>`;

  const $ = <T extends HTMLElement>(s: string) => container.querySelector(s) as T;
  const setStatus = (s: string) => {
    $("#cv-status").textContent = s;
  };
  let channels: Channel[] = [];
  let feishuCfg = { ready: false, long_conn: false, recent_chats: [] as string[] };
  let qqCfg = { ready: false, enabled: false, appId: "" };
  let clawbotCfg = { ready: false, enabled: false, botId: "" };
  let wecomReady = false;

  function render(): void {
    const grid = $("#cv-grid");
    grid.replaceChildren();
    for (const p of PLATFORMS) {
      // 飞书卡片同时收纳「复用应用」出站通道（kind=feishu_app）。
      const existing = channels.filter(
        (c) => c.kind === p.kind || (p.kind === "feishu" && c.kind === "feishu_app"),
      );
      // 「状态驱动」的渠道（QQ 官方机器人 / 微信 ClawBot）：连接状态来自后端各自的配置文件，
      // 不建出站 /api/channels 条目、没有回调地址，因此也不显示「添加/连接」按钮。
      const statusDriven = p.kind === "qqbot" || p.kind === "clawbot";
      const connected =
        p.kind === "qqbot"
          ? qqCfg.enabled
          : p.kind === "clawbot"
            ? clawbotCfg.enabled
            : existing.length > 0;
      const card = document.createElement("div");
      card.className = "chan-card";
      const chanDetail = (c: Channel): string =>
        c.kind === "feishu_app"
          ? t("channels.appPushTarget", { target: esc(c.target ?? "?") })
          : esc(c.url || "—");
      const idRow = (label: string, value: string): string =>
        `<div class="ci-row"><span class="t3">${label}</span><span class="mono">${esc(value || "—")}</span></div>`;
      const notConfigured = `<div class="chan-empty t3">${t("channels.notConfigured")}${p.kind === "webhook" ? t("channels.notConfigured.webhook") : t("channels.notConfigured.generic")}</div>`;
      const info =
        p.kind === "qqbot"
          ? qqCfg.ready
            ? idRow(t("channels.qq.appId"), qqCfg.appId)
            : notConfigured
          : p.kind === "clawbot"
            ? clawbotCfg.ready
              ? // 同步游标按 Bot 共享，两处同时轮询会把消息随机分走——提示紧挨着开关放。
                `${idRow(t("channels.clawbot.botId"), clawbotCfg.botId)}<div class="inline-note" style="margin-top:8px">${t("channels.clawbot.soloNote")}</div>`
              : notConfigured
            : connected
              ? existing
                  .map(
                    (c) =>
                      `<div class="ci-row"><span class="t3">${esc(c.name)}</span><span class="mono">${chanDetail(c)}</span></div>`,
                  )
                  .join("")
              : notConfigured;
      const callback = callbackHtml(p);
      card.innerHTML = `
        <div class="chan-head">
          <div class="chan-logo" style="background:${p.color}">${esc(p.abbr)}</div>
          <div class="chan-id"><strong>${esc(t(p.nameKey))}</strong><div class="sub">${esc(t(p.descKey))}</div></div>
          <span class="badge ${connected ? "green" : ""}">${connected ? t("channels.badge.configured") : t("channels.badge.notConnected")}</span>
        </div>
        ${callback}
        <div class="chan-body">${info}</div>
        <div class="chan-foot"></div>`;
      const foot = card.querySelector(".chan-foot") as HTMLElement;
      // 飞书：扫码接入（device-code 注册，自动拿 app_id/secret）+ 长连接开关。
      if (p.kind === "feishu") {
        const scan = document.createElement("button");
        scan.className = "btn btn-sm btn-primary";
        scan.textContent = t("channels.feishu.scan");
        scan.onclick = () => void openFeishuScan();
        foot.appendChild(scan);

        // 长连接是本地（无公网）收消息的推荐路径；未开启且已有凭据时高亮提示。
        const lc = document.createElement("button");
        lc.className =
          !feishuCfg.long_conn && feishuCfg.ready ? "btn btn-sm btn-primary" : "btn btn-sm";
        lc.textContent = feishuCfg.long_conn
          ? t("channels.feishu.lcOn")
          : t("channels.feishu.lcOff");
        lc.title = t("channels.feishu.lcTitle");
        lc.onclick = async () => {
          const next = !feishuCfg.long_conn;
          const r = (await api("/api/feishu/longconn", {
            method: "POST",
            body: JSON.stringify({ enabled: next }),
          })) as { ok?: boolean };
          if (r.ok) {
            feishuCfg.long_conn = next;
            setStatus(next ? t("channels.feishu.lcOnStatus") : t("channels.feishu.lcOffStatus"));
            render();
          }
        };
        foot.appendChild(lc);

        // 复用已连接的应用机器人做出站推送（免再建自定义机器人）。
        if (feishuCfg.ready) {
          const appPush = document.createElement("button");
          appPush.className = "btn btn-sm";
          appPush.textContent = t("channels.feishu.appPush");
          appPush.title = t("channels.feishu.appPushTitle");
          appPush.onclick = () => openFeishuAppPush();
          foot.appendChild(appPush);
        }
      }
      // 企业微信：入站凭据在 UI 配置（corp_id/secret/agent_id/token/aes_key）。
      if (p.kind === "wecom") {
        const creds = document.createElement("button");
        creds.className = wecomReady ? "btn btn-sm" : "btn btn-sm btn-primary";
        creds.textContent = wecomReady
          ? t("channels.wecom.credsSet")
          : t("channels.wecom.credsConfig");
        creds.onclick = () => openWecomCreds();
        foot.appendChild(creds);
      }
      // QQ 官方机器人：扫码绑定（推荐）/ 手填凭据 + 网关连接开关（状态由 qq.json 驱动）。
      if (p.kind === "qqbot") {
        const scan = document.createElement("button");
        scan.className = "btn btn-sm btn-primary";
        scan.textContent = t("channels.qq.scan");
        scan.onclick = () => void openQqScan();
        foot.appendChild(scan);

        const creds = document.createElement("button");
        creds.className = "btn btn-sm";
        creds.textContent = qqCfg.ready ? t("channels.qq.credsSet") : t("channels.qq.creds");
        creds.onclick = () => void openQqCreds();
        foot.appendChild(creds);

        const tg = document.createElement("button");
        tg.className = qqCfg.ready && !qqCfg.enabled ? "btn btn-sm btn-primary" : "btn btn-sm";
        tg.textContent = qqCfg.enabled ? t("channels.qq.disconnect") : t("channels.qq.connect");
        tg.title = t("channels.qq.toggleTitle");
        tg.disabled = !qqCfg.ready;
        tg.onclick = async () => {
          const next = !qqCfg.enabled;
          const r = (await api("/api/qq/enable", {
            method: "POST",
            body: JSON.stringify({ enabled: next }),
          })) as { ok?: boolean };
          if (r.ok) {
            qqCfg.enabled = next;
            setStatus(next ? t("channels.qq.onStatus") : t("channels.qq.offStatus"));
            render();
          }
        };
        foot.appendChild(tg);
      }
      // 微信 ClawBot：扫码接入 + 长轮询开关（状态由 clawbot.json 驱动）。
      if (p.kind === "clawbot") {
        const scan = document.createElement("button");
        scan.className = "btn btn-sm btn-primary";
        scan.textContent = t("channels.clawbot.scan");
        scan.onclick = () => void openClawbotScan();
        foot.appendChild(scan);

        const tg = document.createElement("button");
        tg.className =
          clawbotCfg.ready && !clawbotCfg.enabled ? "btn btn-sm btn-primary" : "btn btn-sm";
        tg.textContent = clawbotCfg.enabled
          ? t("channels.clawbot.disconnect")
          : t("channels.clawbot.connect");
        tg.title = t("channels.clawbot.toggleTitle");
        tg.disabled = !clawbotCfg.ready;
        tg.onclick = async () => {
          const next = !clawbotCfg.enabled;
          const r = (await api("/api/clawbot/enable", {
            method: "POST",
            body: JSON.stringify({ enabled: next }),
          })) as { ok?: boolean };
          if (r.ok) {
            clawbotCfg.enabled = next;
            setStatus(next ? t("channels.clawbot.onStatus") : t("channels.clawbot.offStatus"));
            render();
          }
        };
        foot.appendChild(tg);
      }
      // 出站「添加/连接」按钮：状态驱动的渠道不走出站通道，跳过。
      if (!statusDriven) {
        const add = document.createElement("button");
        const addPrimary = !connected && p.kind !== "feishu";
        add.className = addPrimary ? "btn btn-sm btn-primary" : "btn btn-sm";
        add.textContent = connected
          ? t("channels.addTarget")
          : p.kind === "feishu"
            ? t("channels.feishu.configOutbound")
            : t("channels.connect", { name: t(p.nameKey) });
        add.onclick = () => openConfig(p);
        foot.appendChild(add);
      }
      for (const c of existing) {
        const del = document.createElement("button");
        del.className = "btn btn-sm btn-danger";
        del.textContent = t("channels.delete", { name: c.name });
        del.onclick = async () => {
          await api(`/api/channels/${encodeURIComponent(c.name)}`, { method: "DELETE" });
          void refresh();
        };
        foot.appendChild(del);
      }
      // 复制回调（HTTP 无安全上下文时 clipboard 不可用，copyText 内有 execCommand 兜底）
      card.querySelector("[data-copy]")?.addEventListener("click", (e) => {
        const v = (e.currentTarget as HTMLElement).dataset.copy ?? "";
        void copyText(v).then((ok) =>
          setStatus(ok ? t("channels.copyOk") : t("channels.copyFail")),
        );
      });
      grid.appendChild(card);
    }
  }

  async function refresh(): Promise<void> {
    setStatus(t("settings.status.loading"));
    try {
      channels = ((await api("/api/channels")).channels as Channel[]) ?? [];
      try {
        const fc = (await api("/api/feishu/config")) as {
          ready?: boolean;
          long_conn?: boolean;
          recent_chats?: string[];
        };
        feishuCfg = {
          ready: !!fc.ready,
          long_conn: !!fc.long_conn,
          recent_chats: fc.recent_chats ?? [],
        };
      } catch {
        /* 飞书状态可选 */
      }
      try {
        const qc = (await api("/api/qq/config")) as {
          ready?: boolean;
          enabled?: boolean;
          app_id?: string | null;
        };
        qqCfg = { ready: !!qc.ready, enabled: !!qc.enabled, appId: qc.app_id ?? "" };
      } catch {
        /* QQ 官方机器人状态可选 */
      }
      try {
        const cb = (await api("/api/clawbot/config")) as {
          ready?: boolean;
          enabled?: boolean;
          bot_id?: string | null;
        };
        clawbotCfg = { ready: !!cb.ready, enabled: !!cb.enabled, botId: cb.bot_id ?? "" };
      } catch {
        /* 微信 ClawBot 状态可选 */
      }
      try {
        const wc = (await api("/api/wecom/config")) as { ready?: boolean };
        wecomReady = !!wc.ready;
      } catch {
        /* 企业微信状态可选 */
      }
      render();
      setStatus("");
    } catch (e) {
      setStatus(t("settings.status.loadFailed", { e: String(e) }));
    }
  }

  // 复用扫码连接的飞书应用机器人，新建一个出站推送通道（kind=feishu_app，target=目标 chat_id）。
  function openFeishuAppPush(): void {
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    const chats = feishuCfg.recent_chats ?? [];
    const opts = chats.map((c) => `<option value="${esc(c)}"></option>`).join("");
    const recentHint = chats.length
      ? t("channels.faPush.recentHint")
      : t("channels.faPush.noRecentHint");
    overlay.innerHTML = `
      <div class="modal">
        <div class="modal-head"><h3>${t("channels.faPush.title")}</h3></div>
        <div class="modal-body">
          <div class="field"><label>${t("channels.name")}</label><input id="fa-name" class="input" placeholder="${t("channels.faPush.namePlaceholder")}" /></div>
          <div class="field"><label>${t("channels.faPush.chatLabel")}</label>
            <input id="fa-chat" class="input mono" list="fa-chats" placeholder="${t("channels.faPush.chatPlaceholder")}" />
            <datalist id="fa-chats">${opts}</datalist>
            <div class="hint">${recentHint}</div>
          </div>
        </div>
        <div class="modal-foot"><span id="fa-msg" class="muted" style="flex:1;font-size:12.5px"></span><button id="fa-cancel" class="btn">${t("common.cancel")}</button><button id="fa-save" class="btn btn-primary">${t("common.save")}</button></div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;
    const close = () => overlay.remove();
    q("#fa-cancel").addEventListener("click", close);
    // 真·模态：不做「点遮罩关闭」——本弹窗要填表，拖选文字到框外松手时 click 的 target
    // 会变成遮罩，会把填了一半的内容关掉。出口只留显式按钮。
    q<HTMLButtonElement>("#fa-save").addEventListener("click", async () => {
      const name = q<HTMLInputElement>("#fa-name").value.trim();
      const target = q<HTMLInputElement>("#fa-chat").value.trim();
      if (!name || !target) {
        q("#fa-msg").textContent = t("channels.faPush.required");
        return;
      }
      const res = (await api("/api/channels", {
        method: "POST",
        body: JSON.stringify({ name, kind: "feishu_app", target }),
      })) as { ok?: boolean };
      if (res.ok) {
        close();
        void refresh();
      } else {
        q("#fa-msg").textContent = t("channels.saveFailed");
      }
    });
  }

  function openConfig(p: Platform): void {
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    const isEmail = p.kind === "email";
    const urlLabel = isEmail
      ? t("channels.config.smtpHost")
      : p.kind === "onebot"
        ? t("channels.config.onebotBase")
        : "Webhook URL";
    const urlPlaceholder = isEmail ? t("channels.config.smtpPlaceholder") : "https://…";
    const emailFields = isEmail
      ? `
          <div class="field"><label>${t("channels.config.recipients")}</label><input id="cc-target" class="input mono" placeholder="a@x.com, b@y.com" /></div>
          <div class="field"><label>${t("channels.config.username")}</label><input id="cc-username" class="input mono" placeholder="${t("channels.config.usernamePlaceholder")}" /></div>
          <div class="field"><label>${t("channels.config.password")}</label><input id="cc-password" class="input mono" type="password" placeholder="${t("channels.config.passwordPlaceholder")}" /></div>
          <div class="field"><label>${t("channels.config.from")}</label><input id="cc-from" class="input mono" placeholder="bot@x.com" /></div>
          <div class="inline-note">${t("channels.config.emailNote")}</div>`
      : "";
    overlay.innerHTML = `
      <div class="modal">
        <div class="modal-head"><h3>${t("channels.config.title", { name: esc(t(p.nameKey)) })}</h3></div>
        <div class="modal-body">
          <div class="field"><label>${t("channels.name")}</label><input id="cc-name" class="input" placeholder="${t("channels.config.namePlaceholder")}" /></div>
          <div class="field"><label>${urlLabel}</label><input id="cc-url" class="input mono" placeholder="${urlPlaceholder}" /></div>
          ${p.kind === "onebot" ? `<div class="field"><label>${t("channels.config.groupLabel")}</label><input id="cc-target" class="input mono" placeholder="${t("channels.config.groupPlaceholder")}" /></div>` : ""}
          ${emailFields}
          ${p.callback ? `<div class="inline-note">${t("channels.config.callbackNote", { name: esc(t(p.nameKey)) })}</div>` : ""}
        </div>
        <div class="modal-foot"><span id="cc-msg" class="muted" style="flex:1;font-size:12.5px"></span><button id="cc-cancel" class="btn">${t("common.cancel")}</button><button id="cc-save" class="btn btn-primary">${t("common.save")}</button></div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;
    const close = () => overlay.remove();
    q("#cc-cancel").addEventListener("click", close);
    // 真·模态：不做「点遮罩关闭」——本弹窗要填表，拖选文字到框外松手时 click 的 target
    // 会变成遮罩，会把填了一半的内容关掉。出口只留显式按钮。
    q<HTMLButtonElement>("#cc-save").addEventListener("click", async () => {
      const name = q<HTMLInputElement>("#cc-name").value.trim();
      const url = q<HTMLInputElement>("#cc-url").value.trim();
      if (!name || !url) {
        q("#cc-msg").textContent = t("channels.config.required");
        return;
      }
      const body: Record<string, unknown> = { name, kind: p.kind, url };
      const target = overlay.querySelector<HTMLInputElement>("#cc-target")?.value.trim();
      if (target) body.target = target;
      if (isEmail) {
        const v = (s: string) => overlay.querySelector<HTMLInputElement>(s)?.value.trim();
        if (v("#cc-username")) body.username = v("#cc-username");
        if (v("#cc-password")) body.password = v("#cc-password");
        if (v("#cc-from")) body.from = v("#cc-from");
      }
      const res = (await api("/api/channels", { method: "POST", body: JSON.stringify(body) })) as {
        ok?: boolean;
      };
      if (res.ok) {
        close();
        void refresh();
      } else {
        q("#cc-msg").textContent = t("channels.saveFailed");
      }
    });
  }

  // 飞书扫码接入：begin 取二维码 → 轮询 poll 直到扫码授权拿到凭据。
  async function openFeishuScan(): Promise<void> {
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    overlay.innerHTML = `
      <div class="modal">
        <div class="modal-head"><h3>${t("channels.scan.title")}</h3></div>
        <div class="modal-body" style="text-align:center">
          <div id="fs-qr" class="fs-qr">${t("channels.scan.generating")}</div>
          <div id="fs-msg" class="muted" style="font-size:13px;margin-top:12px">${t("channels.scan.wait")}</div>
          <div class="inline-note" style="text-align:left;margin-top:14px">${t("channels.scan.note")}</div>
        </div>
        <div class="modal-foot"><span style="flex:1"></span><button id="fs-close" class="btn">${t("common.close")}</button></div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;
    const close = () => overlay.remove();
    q("#fs-close").addEventListener("click", close);
    // 真·模态：不做「点遮罩关闭」——本弹窗要填表，拖选文字到框外松手时 click 的 target
    // 会变成遮罩，会把填了一半的内容关掉。出口只留显式按钮。
    const alive = () => document.body.contains(overlay);
    const msg = (s: string) => {
      if (alive()) q("#fs-msg").textContent = s;
    };

    const begin = (await api("/api/feishu/register/begin", {
      method: "POST",
      body: JSON.stringify({ domain: "feishu" }),
    })) as {
      ok?: boolean;
      device_code?: string;
      qr_svg?: string;
      interval?: number;
      domain?: string;
      error?: string;
    };
    if (!begin.ok || !begin.device_code || !begin.qr_svg) {
      q("#fs-qr").textContent = "";
      msg(t("channels.scan.failed", { e: begin.error ?? t("settings.oauth.unknownError") }));
      return;
    }
    q("#fs-qr").innerHTML = begin.qr_svg; // 后端生成的可信 SVG
    msg(t("channels.scan.prompt"));
    let domain = begin.domain ?? "feishu";
    const interval = Math.max(2, begin.interval ?? 5);
    const deviceCode = begin.device_code;

    const tick = async (): Promise<void> => {
      if (!alive()) return;
      const r = (await api("/api/feishu/register/poll", {
        method: "POST",
        body: JSON.stringify({ device_code: deviceCode, domain }),
      })) as { status?: string; domain?: string | null; app_id?: string; error?: string };
      if (!alive()) return;
      switch (r.status) {
        case "success":
          msg(t("channels.scan.connected", { id: r.app_id ?? "" }));
          void refresh();
          setTimeout(() => alive() && close(), 1800);
          return;
        case "denied":
          msg(t("channels.scan.denied"));
          return;
        case "expired":
          msg(t("channels.scan.expired"));
          return;
        case "error":
          msg(t("channels.scan.error", { e: r.error ?? "" }));
          return;
        default: // pending
          if (r.domain) domain = r.domain; // lark 切域
          setTimeout(() => void tick(), interval * 1000);
      }
    };
    setTimeout(() => void tick(), interval * 1000);
  }

  // 企业微信入站凭据配置：5 个字段写入后端 wecom.json（留空保留原值）。
  async function openWecomCreds(): Promise<void> {
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    const cb = esc(`${httpBase() || location.origin}/api/im/wecom`);
    overlay.innerHTML = `
      <div class="modal">
        <div class="modal-head"><h3>${t("channels.wecom.title")}</h3></div>
        <div class="modal-body">
          <div class="field"><label>${t("channels.wecom.corpId")}</label><input id="wc-corp" class="input mono" placeholder="ww……" /></div>
          <div class="field"><label>${t("channels.wecom.secret")}</label><input id="wc-secret" class="input mono" type="password" placeholder="${t("channels.wecom.secretPlaceholder")}" /></div>
          <div class="field"><label>${t("channels.wecom.agentId")}</label><input id="wc-agent" class="input mono" placeholder="${t("channels.wecom.agentIdPlaceholder")}" /></div>
          <div class="field"><label>${t("channels.wecom.token")}</label><input id="wc-token" class="input mono" placeholder="${t("channels.wecom.tokenPlaceholder")}" /></div>
          <div class="field"><label>${t("channels.wecom.aesKey")}</label><input id="wc-aes" class="input mono" type="password" placeholder="${t("channels.wecom.aesPlaceholder")}" /></div>
          <div class="inline-note">${t("channels.wecom.note", { cb })}</div>
        </div>
        <div class="modal-foot"><span id="wc-msg" class="muted" style="flex:1;font-size:12.5px"></span><button id="wc-cancel" class="btn">${t("common.cancel")}</button><button id="wc-save" class="btn btn-primary">${t("common.save")}</button></div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;
    const close = () => overlay.remove();
    q("#wc-cancel").addEventListener("click", close);
    // 真·模态：不做「点遮罩关闭」——本弹窗要填表，拖选文字到框外松手时 click 的 target
    // 会变成遮罩，会把填了一半的内容关掉。出口只留显式按钮。
    q<HTMLButtonElement>("#wc-save").addEventListener("click", async () => {
      const v = (s: string) => q<HTMLInputElement>(s).value.trim();
      const res = (await api("/api/wecom/config", {
        method: "POST",
        body: JSON.stringify({
          corp_id: v("#wc-corp"),
          corp_secret: v("#wc-secret"),
          agent_id: v("#wc-agent"),
          callback_token: v("#wc-token"),
          encoding_aes_key: v("#wc-aes"),
        }),
      })) as { ok?: boolean; ready?: boolean };
      if (res.ok) {
        close();
        setStatus(res.ready ? t("channels.wecom.savedReady") : t("channels.wecom.savedIncomplete"));
        void refresh();
      } else {
        q("#wc-msg").textContent = t("channels.saveFailed");
      }
    });
  }

  // QQ 官方机器人扫码绑定：begin 取二维码 → 轮询 poll 直到手机 QQ 扫码授权，自动拿 AppID/AppSecret。
  async function openQqScan(): Promise<void> {
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    overlay.innerHTML = `
      <div class="modal">
        <div class="modal-head"><h3>${t("channels.qq.scanTitle")}</h3></div>
        <div class="modal-body" style="text-align:center">
          <div id="qs-qr" class="fs-qr">${t("channels.scan.generating")}</div>
          <div id="qs-msg" class="muted" style="font-size:13px;margin-top:12px">${t("channels.scan.wait")}</div>
          <div class="inline-note" style="text-align:left;margin-top:14px">${t("channels.qq.scanNote")}</div>
        </div>
        <div class="modal-foot"><span style="flex:1"></span><button id="qs-close" class="btn">${t("common.close")}</button></div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;
    const close = () => overlay.remove();
    q("#qs-close").addEventListener("click", close);
    // 真·模态：不做「点遮罩关闭」——本弹窗要填表，拖选文字到框外松手时 click 的 target
    // 会变成遮罩，会把填了一半的内容关掉。出口只留显式按钮。
    const alive = () => document.body.contains(overlay);
    const msg = (s: string) => {
      if (alive()) q("#qs-msg").textContent = s;
    };

    const begin = (await api("/api/qq/scan/begin", { method: "POST" })) as {
      ok?: boolean;
      task_id?: string;
      key?: string;
      qr_svg?: string;
      interval?: number;
      error?: string;
    };
    if (!begin.ok || !begin.task_id || !begin.key || !begin.qr_svg) {
      q("#qs-qr").textContent = "";
      msg(t("channels.scan.failed", { e: begin.error ?? t("settings.oauth.unknownError") }));
      return;
    }
    q("#qs-qr").innerHTML = begin.qr_svg; // 后端生成的可信 SVG
    msg(t("channels.qq.scanPrompt"));
    const interval = Math.max(2, begin.interval ?? 2);
    const taskId = begin.task_id;
    const key = begin.key;

    const tick = async (): Promise<void> => {
      if (!alive()) return;
      const r = (await api("/api/qq/scan/poll", {
        method: "POST",
        body: JSON.stringify({ task_id: taskId, key }),
      })) as { status?: string; app_id?: string; error?: string };
      if (!alive()) return;
      switch (r.status) {
        case "success":
          qqCfg = { ready: true, enabled: true, appId: r.app_id ?? "" };
          msg(t("channels.qq.scanConnected", { id: r.app_id ?? "" }));
          render();
          setTimeout(() => alive() && close(), 1800);
          return;
        case "expired":
          msg(t("channels.scan.expired"));
          return;
        case "error":
          msg(t("channels.scan.error", { e: r.error ?? "" }));
          return;
        default: // pending
          setTimeout(() => void tick(), interval * 1000);
      }
    };
    setTimeout(() => void tick(), interval * 1000);
  }

  // 微信 ClawBot 扫码接入：begin 取二维码 → 轮询直到手机微信扫码确认，自动拿 bot_token。
  async function openClawbotScan(): Promise<void> {
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    overlay.innerHTML = `
      <div class="modal">
        <div class="modal-head"><h3>${t("channels.clawbot.scanTitle")}</h3></div>
        <div class="modal-body" style="text-align:center">
          <div id="cb-qr" class="fs-qr">${t("channels.scan.generating")}</div>
          <div id="cb-msg" class="muted" style="font-size:13px;margin-top:12px">${t("channels.scan.wait")}</div>
          <div class="inline-note" style="text-align:left;margin-top:14px">${t("channels.clawbot.scanNote")}</div>
        </div>
        <div class="modal-foot"><span style="flex:1"></span><button id="cb-close" class="btn">${t("common.close")}</button></div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;
    const close = () => overlay.remove();
    q("#cb-close").addEventListener("click", close);
    // 真·模态：不做「点遮罩关闭」——扫码期间误关等于白扫一次（二维码最多刷新 3 次）。
    const alive = () => document.body.contains(overlay);
    const msg = (s: string) => {
      if (alive()) q("#cb-msg").textContent = s;
    };

    const begin = (await api("/api/clawbot/scan/begin", { method: "POST" })) as {
      ok?: boolean;
      qrcode?: string;
      qr_img?: string;
      interval?: number;
      error?: string;
    };
    if (!begin.ok || !begin.qrcode || !begin.qr_img) {
      q("#cb-qr").textContent = "";
      msg(t("channels.scan.failed", { e: begin.error ?? t("settings.oauth.unknownError") }));
      return;
    }
    // 二维码图片由微信服务端给（data URL）；拿不到时后端已回退成自绘 SVG，同样是 data URL。
    q("#cb-qr").innerHTML =
      `<img src="${esc(begin.qr_img)}" alt="" style="width:100%;max-width:220px;height:auto" />`;
    msg(t("channels.clawbot.scanPrompt"));
    const interval = Math.max(2, begin.interval ?? 2);
    const qrcode = begin.qrcode;
    // 扫码后服务端可能要求切到就近 IDC 继续轮询；不跟着切就会一直停在「等待确认」。
    let base: string | undefined;

    const tick = async (): Promise<void> => {
      if (!alive()) return;
      const r = (await api("/api/clawbot/scan/poll", {
        method: "POST",
        body: JSON.stringify({ qrcode, base }),
      })) as { status?: string; base?: string | null; bot_id?: string; error?: string };
      if (!alive()) return;
      switch (r.status) {
        case "success":
          clawbotCfg = { ready: true, enabled: true, botId: r.bot_id ?? "" };
          msg(t("channels.clawbot.scanConnected", { id: r.bot_id ?? "" }));
          render();
          setTimeout(() => alive() && close(), 1800);
          return;
        case "expired":
          msg(t("channels.scan.expired"));
          return;
        case "error":
          msg(t("channels.scan.error", { e: r.error ?? "" }));
          return;
        default: // pending
          if (r.base) base = r.base; // IDC 重定向
          setTimeout(() => void tick(), interval * 1000);
      }
    };
    setTimeout(() => void tick(), interval * 1000);
  }

  // QQ 官方机器人手填凭据：AppID/AppSecret 写入后端 qq.json（AppSecret 留空＝保留原值）。
  async function openQqCreds(): Promise<void> {
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    overlay.innerHTML = `
      <div class="modal">
        <div class="modal-head"><h3>${t("channels.qq.title")}</h3></div>
        <div class="modal-body">
          <div class="field"><label>${t("channels.qq.appId")}</label><input id="qc-appid" class="input mono" placeholder="102xxxxxxx" value="${esc(qqCfg.appId)}" /></div>
          <div class="field"><label>${t("channels.qq.appSecret")}</label><input id="qc-secret" class="input mono" type="password" placeholder="${t("channels.qq.appSecretPlaceholder")}" /></div>
          <div class="inline-note">${t("channels.qq.note")}</div>
        </div>
        <div class="modal-foot"><span id="qc-msg" class="muted" style="flex:1;font-size:12.5px"></span><button id="qc-cancel" class="btn">${t("common.cancel")}</button><button id="qc-save" class="btn btn-primary">${t("common.save")}</button></div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;
    const close = () => overlay.remove();
    q("#qc-cancel").addEventListener("click", close);
    // 真·模态：不做「点遮罩关闭」——本弹窗要填表，拖选文字到框外松手时 click 的 target
    // 会变成遮罩，会把填了一半的内容关掉。出口只留显式按钮。
    q<HTMLButtonElement>("#qc-save").addEventListener("click", async () => {
      const v = (s: string) => q<HTMLInputElement>(s).value.trim();
      const res = (await api("/api/qq/config", {
        method: "POST",
        body: JSON.stringify({ app_id: v("#qc-appid"), app_secret: v("#qc-secret") }),
      })) as { ok?: boolean; ready?: boolean };
      if (res.ok) {
        close();
        setStatus(res.ready ? t("channels.qq.savedReady") : t("channels.qq.savedIncomplete"));
        void refresh();
      } else {
        q("#qc-msg").textContent = t("channels.saveFailed");
      }
    });
  }

  return { refresh };
}
