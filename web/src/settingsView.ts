// ── 设置视图（整页）──────────────────────────────────────────────────────────
// 模型接入：LLM 列表（含价格列）+ 默认模型下拉 + 配置 + 删除；「添加模型」弹窗。
//   选定提供商后 Model ID 为下拉（取 /api/providers 的 models 表），
//   仅「OpenAI 兼容」需手填 Model ID 与 Endpoint。
// 访问控制：access_key 开关 + 危险操作执行前确认。

import { authHeaders, setKey } from "./auth";
import { httpBase } from "./backend";
import { LOCALES, getLocale, switchLocale, t } from "./i18n";
import { icon } from "./icons";
import { copyText, isTauri, openExternal, pickDirectory } from "./platform";

interface Provider {
  id: string;
  name: string;
  default_model: string;
  models: string[];
  base_url: string;
  vision_models?: string[];
}

/// 模型弹窗里「端点 / 模型 ID」两栏该呈现什么。
///
/// 抽成纯函数是为了能直接测：这块的规则踩过坑——早期预设 provider 会把端点设成
/// readOnly、模型做成 select，结果模型一更新就得改代码发版，反代用户也没法改端点。
/// 现在两栏都可编辑，预设值只作为初值和 datalist 建议。
export function modelFieldState(
  p: Provider | undefined,
  opts: { initial?: boolean; savedModel?: string | null; savedBaseUrl?: string | null } = {},
): { baseUrl: string; model: string; suggestions: string[] } {
  const { initial, savedModel, savedBaseUrl } = opts;
  // 编辑已有档：一律以存下来的值为准（用户可能填过自定义反代/自定义模型）。
  if (initial) {
    return {
      baseUrl: savedBaseUrl?.trim() || p?.base_url || "",
      model: savedModel?.trim() || p?.default_model || "",
      suggestions: p?.models ?? [],
    };
  }
  // 新建或切换 provider：用该 provider 的预设做初值。
  return {
    baseUrl: p?.base_url ?? "",
    model: p?.default_model ?? "",
    suggestions: p?.models ?? [],
  };
}
export interface LlmRow {
  id: string;
  name: string;
  provider?: string | null;
  model?: string | null;
  base_url?: string | null;
  price_in?: number | null;
  price_out?: number | null;
  price_cache_read?: number | null;
  max_tokens?: number | null;
  claude_oauth?: boolean;
  openai_codex?: boolean;
  xai_grok?: boolean;
  gemini_oauth?: boolean;
  vision?: boolean;
  reasoning_effort?: string | null;
  api_key_set: boolean;
  /// 该档走订阅 OAuth，不需要 api_key（由服务端判定，别在前端重算四个开关的 OR）。
  subscription?: boolean;
}

/// 模型行右侧的状态徽章。三态，别退化成「有没有 key」的两态：
/// 订阅档走 OAuth，没有 api_key 是正常的，标成「未配置」会让人以为它坏了。
export function keyBadge(l: LlmRow): string {
  if (l.subscription) {
    return `<span class="badge green" title="${t("settings.models.subscriptionTitle")}">${t("settings.models.subscription")}</span>`;
  }
  return `<span class="badge ${l.api_key_set ? "green" : ""}">${
    l.api_key_set ? t("settings.models.keySet") : t("settings.models.keyUnset")
  }</span>`;
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

function notifyChanged(): void {
  window.dispatchEvent(new Event("wc:llm-changed"));
}

/** 挂载设置视图，返回 refresh()（视图显示时调用）。 */
export function mountSettingsView(container: HTMLElement): { refresh: () => void } {
  let providers: Provider[] = [];
  const provName = (id?: string | null): string =>
    providers.find((p) => p.id === id)?.name ?? id ?? "?";

  container.innerHTML = `
    <div class="page">
      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("globe", 17)}</span>
          <div><h2>${t("settings.language.title")}</h2><div class="sub">${t("settings.language.sub")}</div></div>
        </div>
        <div class="card card-pad" style="padding:14px 18px">
          <div class="set-inline" style="margin:0">
            <span class="t3">${t("settings.language.label")}</span>
            <select id="sv-lang" class="select" style="width:220px">
              ${LOCALES.map((l) => `<option value="${l.code}">${l.native}</option>`).join("")}
            </select>
          </div>
        </div>
      </div>

      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("bolt", 17)}</span>
          <div><h2>${t("settings.models.title")}</h2><div class="sub">${t("settings.models.sub")}</div></div>
          <span class="spacer" style="flex:1"></span>
          <button id="sv-add" class="btn btn-primary btn-sm"><span data-icon="plus"></span>${t("settings.models.add")}</button>
        </div>
        <div id="sv-models" class="card"></div>
        <div class="set-inline">
          <span class="t3">${t("settings.models.default")}</span>
          <select id="sv-default" class="select" style="width:260px"></select>
        </div>
      </div>

      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("folder", 17)}</span>
          <div><h2>${t("settings.workspace.title")}</h2><div class="sub">${t("settings.workspace.sub")}</div></div>
        </div>
        <div class="card card-pad" style="padding:14px 18px">
          <div class="field" style="margin:0">
            <label>${t("settings.workspace.label")}</label>
            <div class="input-group">
              <input id="sv-workspace" class="input mono" placeholder="${t("settings.workspace.placeholder")}" />
              <button id="sv-workspace-pick" class="btn btn-sm">${t("settings.pick")}</button>
            </div>
            <div class="hint" id="sv-workspace-hint"></div>
          </div>
        </div>
      </div>

      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("globe", 17)}</span>
          <div><h2>${t("settings.proxy.title")}</h2><div class="sub">${t("settings.proxy.sub")}</div></div>
        </div>
        <div class="card card-pad" style="padding:14px 18px">
          <div class="field" style="margin:0">
            <label>${t("settings.proxy.label")}</label>
            <input id="sv-proxy" class="input mono" placeholder="${t("settings.proxy.placeholder")}" />
            <div class="hint">${t("settings.proxy.hint")}</div>
          </div>
        </div>
      </div>

      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("bolt", 17)}</span>
          <div><h2>${t("settings.claudeOauth.title")}</h2><div class="sub">${t("settings.claudeOauth.sub")}</div></div>
        </div>
        <div class="card card-pad" style="padding:14px 18px">
          <div class="field" style="margin:0">
            <div style="display:flex;align-items:center;gap:10px;flex-wrap:wrap">
              <span id="sv-oauth-status" class="t3">${t("settings.oauth.checking")}</span>
              <button id="sv-oauth-login" class="btn">${t("settings.oauth.login")}</button>
              <button id="sv-oauth-logout" class="btn" hidden>${t("settings.oauth.logout")}</button>
            </div>
            <div id="sv-oauth-step" hidden style="margin-top:12px">
              <div class="hint" style="margin-bottom:6px">${t("settings.claudeOauth.step1")}</div>
              <div style="display:flex;gap:8px;margin-bottom:8px">
                <input id="sv-oauth-url" class="input mono" style="flex:1" readonly />
                <button id="sv-oauth-open" class="btn">${t("settings.oauth.open")}</button>
                <button id="sv-oauth-copy" class="btn">${t("settings.oauth.copyLink")}</button>
              </div>
              <div class="hint" style="margin-bottom:6px">${t("settings.claudeOauth.step2")}</div>
              <div style="display:flex;gap:8px">
                <input id="sv-oauth-code" class="input mono" style="flex:1" placeholder="${t("settings.claudeOauth.codePlaceholder")}" />
                <button id="sv-oauth-submit" class="btn btn-primary">${t("common.done")}</button>
              </div>
            </div>
            <div class="hint" style="margin-top:8px">${t("settings.claudeOauth.warn")}</div>
          </div>
        </div>
      </div>

      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("bolt", 17)}</span>
          <div><h2>${t("settings.chatgptOauth.title")}</h2><div class="sub">${t("settings.chatgptOauth.sub")}</div></div>
        </div>
        <div class="card card-pad" style="padding:14px 18px">
          <div class="field" style="margin:0">
            <div style="display:flex;align-items:center;gap:10px;flex-wrap:wrap">
              <span id="sv-oai-status" class="t3">${t("settings.oauth.checking")}</span>
              <button id="sv-oai-login" class="btn">${t("settings.oauth.login")}</button>
              <button id="sv-oai-logout" class="btn" hidden>${t("settings.oauth.logout")}</button>
            </div>
            <div id="sv-oai-step" hidden style="margin-top:12px">
              <div class="hint" style="margin-bottom:6px">${t("settings.chatgptOauth.step1")}</div>
              <div style="display:flex;gap:8px;margin-bottom:8px">
                <input id="sv-oai-url" class="input mono" style="flex:1" readonly />
                <button id="sv-oai-open" class="btn">${t("settings.oauth.open")}</button>
                <button id="sv-oai-copy" class="btn">${t("settings.oauth.copyLink")}</button>
              </div>
              <div class="hint" style="margin-bottom:6px">${t("settings.chatgptOauth.step2")}</div>
              <div style="display:flex;gap:8px">
                <input id="sv-oai-code" class="input mono" style="flex:1" placeholder="${t("settings.chatgptOauth.codePlaceholder")}" />
                <button id="sv-oai-submit" class="btn btn-primary">${t("common.done")}</button>
              </div>
            </div>
            <div class="hint" style="margin-top:8px">${t("settings.chatgptOauth.warn")}</div>
          </div>
        </div>
      </div>

      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("bolt", 17)}</span>
          <div><h2>${t("settings.grokOauth.title")}</h2><div class="sub">${t("settings.grokOauth.sub")}</div></div>
        </div>
        <div class="card card-pad" style="padding:14px 18px">
          <div class="field" style="margin:0">
            <div style="display:flex;align-items:center;gap:10px;flex-wrap:wrap">
              <span id="sv-xai-status" class="t3">${t("settings.oauth.checking")}</span>
              <button id="sv-xai-login" class="btn">${t("settings.oauth.login")}</button>
              <button id="sv-xai-logout" class="btn" hidden>${t("settings.oauth.logout")}</button>
            </div>
            <div id="sv-xai-step" hidden style="margin-top:12px">
              <div class="hint" style="margin-bottom:6px">${t("settings.grokOauth.step1")}</div>
              <div style="display:flex;gap:8px;margin-bottom:8px">
                <input id="sv-xai-url" class="input mono" style="flex:1" readonly />
                <button id="sv-xai-open" class="btn">${t("settings.oauth.open")}</button>
                <button id="sv-xai-copy" class="btn">${t("settings.oauth.copyLink")}</button>
              </div>
              <div class="hint" style="margin-bottom:6px">${t("settings.grokOauth.step2")}</div>
              <input id="sv-xai-code" class="input mono" style="width:200px;font-size:20px;letter-spacing:3px;text-align:center" readonly />
            </div>
            <div class="hint" style="margin-top:8px">${t("settings.grokOauth.warn")}</div>
          </div>
        </div>
      </div>

      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("bolt", 17)}</span>
          <div><h2>${t("settings.geminiOauth.title")}</h2><div class="sub">${t("settings.geminiOauth.sub")}</div></div>
        </div>
        <div class="card card-pad" style="padding:14px 18px">
          <div class="field" style="margin:0">
            <div style="display:flex;align-items:center;gap:10px;flex-wrap:wrap">
              <span id="sv-gem-status" class="t3">${t("settings.oauth.checking")}</span>
              <button id="sv-gem-login" class="btn">${t("settings.oauth.login")}</button>
              <button id="sv-gem-logout" class="btn" hidden>${t("settings.oauth.logout")}</button>
            </div>
            <div id="sv-gem-step" hidden style="margin-top:12px">
              <div class="hint" style="margin-bottom:6px">${t("settings.geminiOauth.step1")}</div>
              <div style="display:flex;gap:8px;margin-bottom:8px">
                <input id="sv-gem-url" class="input mono" style="flex:1" readonly />
                <button id="sv-gem-open" class="btn">${t("settings.oauth.open")}</button>
                <button id="sv-gem-copy" class="btn">${t("settings.oauth.copyLink")}</button>
              </div>
              <div class="hint" style="margin-bottom:6px">${t("settings.geminiOauth.step2")}</div>
              <div style="display:flex;gap:8px">
                <input id="sv-gem-code" class="input mono" style="flex:1" placeholder="${t("settings.geminiOauth.codePlaceholder")}" />
                <button id="sv-gem-submit" class="btn btn-primary">${t("common.done")}</button>
              </div>
            </div>
            <div class="hint" style="margin-top:8px">${t("settings.geminiOauth.warn")}</div>
          </div>
        </div>
      </div>

      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("bolt", 17)}</span>
          <div><h2>${t("settings.maxiter.title")}</h2><div class="sub">${t("settings.maxiter.sub")}</div></div>
        </div>
        <div class="card card-pad" style="padding:14px 18px">
          <div class="field" style="margin:0 0 14px">
            <label>${t("settings.maxiter.interactive")}</label>
            <input id="sv-maxiter-interactive" class="input mono" inputmode="numeric" style="width:160px" placeholder="${t("settings.maxiter.interactivePlaceholder")}" />
            <div class="hint">${t("settings.maxiter.interactiveHint")}</div>
          </div>
          <div class="field" style="margin:0 0 14px">
            <label>${t("settings.maxiter.unattended")}</label>
            <input id="sv-maxiter" class="input mono" inputmode="numeric" style="width:160px" placeholder="${t("settings.maxiter.unattendedPlaceholder")}" />
            <div class="hint">${t("settings.maxiter.unattendedHint")}</div>
          </div>
          <div class="field" style="margin:0">
            <label>${t("settings.maxiter.subagent")}</label>
            <input id="sv-maxiter-subagent" class="input mono" inputmode="numeric" style="width:160px" placeholder="${t("settings.maxiter.subagentPlaceholder")}" />
            <div class="hint">${t("settings.maxiter.subagentHint")}</div>
          </div>
        </div>
      </div>

      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("folder", 17)}</span>
          <div><h2>${t("settings.logclean.title")}</h2><div class="sub">${t("settings.logclean.sub")}</div></div>
        </div>
        <div class="card card-pad" style="padding:14px 18px">
          <div class="switch-row" style="padding-top:0">
            <div class="sr-main">
              <div class="sr-title">${t("settings.logclean.autoLabel")}</div>
              <div class="sr-sub">${t("settings.logclean.autoSub")}</div>
            </div>
            <button id="sv-logclean-toggle" class="toggle"></button>
          </div>
          <div id="sv-logmax-row" class="field" style="margin:0">
            <div class="divider"></div>
            <label>${t("settings.logclean.maxLabel")}</label>
            <input id="sv-logmax" class="input mono" inputmode="numeric" style="width:160px" placeholder="${t("settings.logclean.placeholder")}" />
            <div class="hint">${t("settings.logclean.hint")}</div>
          </div>
        </div>
      </div>

      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("globe", 17)}</span>
          <div><h2>${t("settings.websearch.title")}</h2><div class="sub">${t("settings.websearch.sub")}</div></div>
        </div>
        <div class="card card-pad" style="padding:14px 18px">
          <div class="field">
            <label>${t("settings.websearch.provider")}</label>
            <select id="sv-ws-provider" class="select" style="width:260px">
              <option value="">${t("settings.websearch.ddg")}</option>
              <option value="searxng">${t("settings.websearch.searxng")}</option>
              <option value="brave">${t("settings.websearch.brave")}</option>
              <option value="tavily">${t("settings.websearch.tavily")}</option>
            </select>
          </div>
          <div class="field" id="sv-ws-base-row" hidden>
            <label>${t("settings.websearch.searxngUrl")}</label>
            <input id="sv-ws-base" class="input mono" placeholder="https://searx.example.com" />
          </div>
          <div class="field" id="sv-ws-key-row" hidden style="margin:0">
            <label>API Key</label>
            <input id="sv-ws-key" class="input mono" type="password" placeholder="sk-…" />
          </div>
        </div>
      </div>

      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("bolt", 17)}</span>
          <div><h2>${t("settings.effort.title")}</h2><div class="sub">${t("settings.effort.sub")}</div></div>
        </div>
        <div class="card card-pad" style="padding:14px 18px">
          <div class="field" style="margin:0">
            <label>${t("settings.effort.label")}</label>
            <select id="sv-effort" class="select" style="width:260px">
              <option value="">${t("settings.effort.off")}</option>
              <option value="low">low</option>
              <option value="medium">medium</option>
              <option value="high">high</option>
              <option value="xhigh">${t("settings.effort.xhigh")}</option>
              <option value="max">max</option>
            </select>
            <div class="hint">${t("settings.effort.hint")}</div>
          </div>
        </div>
      </div>

      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("shield", 17)}</span>
          <div><h2>${t("settings.access.title")}</h2><div class="sub">${t("settings.access.sub")}</div></div>
        </div>
        <div class="card card-pad" style="padding:6px 18px">
          <div class="switch-row">
            <div class="sr-main">
              <div class="sr-title">${t("settings.access.enableKey")}</div>
              <div class="sr-sub">${t("settings.access.enableKeySub")}</div>
            </div>
            <button id="sv-access-toggle" class="toggle"></button>
          </div>
          <div id="sv-access-env-note" class="sr-sub" style="margin:-6px 0 12px;color:var(--accent)" hidden>${t("settings.access.envManaged")}</div>
          <div id="sv-access-key-row" class="field" style="margin:0 0 14px" hidden>
            <input id="sv-access-key" class="input mono" type="password" placeholder="${t("settings.access.keyPlaceholder")}" />
          </div>
          <div class="divider"></div>
          <div class="switch-row">
            <div class="sr-main">
              <div class="sr-title">${t("settings.access.confirm")}</div>
              <div class="sr-sub">${t("settings.access.confirmSub")}</div>
            </div>
            <button id="sv-confirm-toggle" class="toggle"></button>
          </div>
          <div class="divider"></div>
          <div class="switch-row">
            <div class="sr-main">
              <div class="sr-title">${t("settings.access.automem")}</div>
              <div class="sr-sub">${t("settings.access.automemSub")}</div>
            </div>
            <button id="sv-automem-toggle" class="toggle"></button>
          </div>
          <div class="divider"></div>
          <div class="switch-row">
            <div class="sr-main">
              <div class="sr-title">${t("settings.access.autotrim")}<span class="help-dot" title="${t("settings.access.autotrimHelp")}">${icon("help", 15)}</span></div>
              <div class="sr-sub">${t("settings.access.autotrimSub")}</div>
            </div>
            <button id="sv-autotrim-toggle" class="toggle"></button>
          </div>
        </div>
      </div>

      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("plug", 17)}</span>
          <div><h2>${t("settings.mcp.title")}</h2><div class="sub">${t("settings.mcp.sub")}</div></div>
        </div>
        <div class="card card-pad">
          <div class="field">
            <label>${t("settings.mcp.label")}</label>
            <textarea id="sv-mcp" class="textarea mono" rows="9" spellcheck="false" placeholder='{
  "filesystem": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "."] },
  "remote": { "url": "https://example.com/mcp", "headers": { "Authorization": "Bearer xxx" } }
}'></textarea>
          </div>
          <div class="hint" style="margin-top:8px">${t("settings.mcp.hint")}</div>
          <div style="display:flex;align-items:center;gap:10px;margin-top:8px">
            <button id="sv-mcp-save" class="btn btn-primary btn-sm">${t("settings.mcp.save")}</button>
            <span id="sv-mcp-msg" class="muted" style="font-size:12.5px"></span>
          </div>
          <div class="hint" style="margin-top:10px">${t("settings.mcp.discovered")}</div>
          <div id="sv-mcp-tools" class="mono" style="font-size:12.5px;color:var(--text-2)"></div>
        </div>
      </div>

      <div class="section">
        <div class="set-block-head">
          <span class="ico-tile">${icon("bolt", 17)}</span>
          <div><h2>${t("settings.hooks.title")}</h2><div class="sub">${t("settings.hooks.sub")}</div></div>
        </div>
        <div class="card card-pad">
          <div class="field">
            <label>${t("settings.hooks.label")}</label>
            <textarea id="sv-hooks" class="textarea mono" rows="8" spellcheck="false" placeholder='{
  "PreToolUse": [
    { "matcher": "shell", "command": "node guard.js" }
  ]
}'></textarea>
          </div>
          <div style="display:flex;align-items:center;gap:10px;margin-top:8px">
            <button id="sv-hooks-save" class="btn btn-primary btn-sm">${t("settings.hooks.save")}</button>
            <span id="sv-hooks-msg" class="muted" style="font-size:12.5px"></span>
          </div>
          <div class="hint" style="margin-top:10px">${t("settings.hooks.hint")}</div>
        </div>
      </div>

      <div id="sv-status" class="muted" style="font-size:13px"></div>
    </div>`;

  const $ = <T extends HTMLElement>(s: string) => container.querySelector(s) as T;
  // 界面语言：选中当前语言；切换即持久化并刷新页面（视图重新 mount 后整体生效）。
  const langSel = $<HTMLSelectElement>("#sv-lang");
  langSel.value = getLocale();
  langSel.addEventListener("change", () =>
    switchLocale(langSel.value as ReturnType<typeof getLocale>),
  );
  const status = $("#sv-status");
  const setStatus = (s: string) => {
    status.textContent = s;
  };
  // Claude 订阅 OAuth 登录。
  const oauthStatusEl = $("#sv-oauth-status");
  const oauthLoginBtn = $<HTMLButtonElement>("#sv-oauth-login");
  const oauthLogoutBtn = $<HTMLButtonElement>("#sv-oauth-logout");
  const oauthStep = $("#sv-oauth-step");
  const oauthUrl = $<HTMLInputElement>("#sv-oauth-url");
  const oauthOpen = $<HTMLButtonElement>("#sv-oauth-open");
  const oauthCopy = $<HTMLButtonElement>("#sv-oauth-copy");
  const oauthCode = $<HTMLInputElement>("#sv-oauth-code");
  const oauthSubmit = $<HTMLButtonElement>("#sv-oauth-submit");
  let oauthPending: { verifier: string; state: string } | null = null;
  async function refreshOAuthStatus(): Promise<void> {
    try {
      const r = await api("/api/oauth/claude/status");
      const on = r.logged_in === true;
      oauthStatusEl.textContent = on
        ? t("settings.claudeOauth.loggedIn")
        : t("settings.oauth.notLoggedIn");
      oauthLoginBtn.textContent = on ? t("settings.oauth.relogin") : t("settings.oauth.login");
      oauthLogoutBtn.hidden = !on;
    } catch {
      oauthStatusEl.textContent = t("settings.oauth.unknown");
    }
  }
  oauthLoginBtn.addEventListener("click", async () => {
    try {
      // 桌面走 loopback 免粘贴（Claude Code 同 client_id，回环回调放行）；远程/回退走手动粘贴。
      const r = await api("/api/oauth/claude/start", {
        method: "POST",
        body: JSON.stringify({ loopback: isTauri() }),
      });
      const url = String(r.url);
      const opened = await openExternal(url);
      if (r.loopback === true) {
        oauthStep.hidden = true;
        if (!opened) await copyText(url);
        oauthStatusEl.textContent = opened
          ? t("settings.oauth.waitingBrowser")
          : t("settings.oauth.loopbackCopied");
        void loopbackPoll("claude", oauthStatusEl, refreshOAuthStatus);
      } else {
        oauthPending = { verifier: String(r.verifier), state: String(r.state) };
        oauthUrl.value = url;
        oauthStep.hidden = false;
        oauthStatusEl.textContent = opened
          ? t("settings.oauth.openedCode")
          : t("settings.oauth.manualOpen");
        oauthCode.focus();
      }
    } catch (e) {
      oauthStatusEl.textContent = t("settings.oauth.loginFailed", { e: String(e) });
    }
  });
  oauthOpen.addEventListener("click", () => {
    if (oauthUrl.value) void openExternal(oauthUrl.value);
  });
  oauthCopy.addEventListener("click", async () => {
    if (oauthUrl.value && (await copyText(oauthUrl.value))) {
      oauthStatusEl.textContent = t("settings.oauth.copied");
    }
  });
  oauthSubmit.addEventListener("click", async () => {
    const code = oauthCode.value.trim();
    if (!code || !oauthPending) return;
    oauthStatusEl.textContent = t("settings.oauth.redeeming");
    const res = await api("/api/oauth/claude/finish", {
      method: "POST",
      body: JSON.stringify({ code, verifier: oauthPending.verifier, state: oauthPending.state }),
    });
    if (res.ok) {
      oauthPending = null;
      oauthStep.hidden = true;
      oauthCode.value = "";
      await refreshOAuthStatus();
    } else {
      oauthStatusEl.textContent = t("settings.oauth.failed", {
        e: String(res.error ?? t("settings.oauth.unknownError")),
      });
    }
  });
  oauthLogoutBtn.addEventListener("click", async () => {
    await api("/api/oauth/claude/logout", { method: "POST", body: "{}" });
    await refreshOAuthStatus();
  });

  // 桌面 loopback 免粘贴：开浏览器授权后本地监听自动接住 code，前端轮询 /status 直到已登录
  //（约 2.5 分钟）。远程 webUI 收不到 localhost 回调，故仅桌面走此路，服务端也会在起监听失败时回退手动。
  async function loopbackPoll(
    provider: string,
    statusEl: HTMLElement,
    refresh: () => Promise<void>,
  ): Promise<void> {
    for (let i = 0; i < 75; i++) {
      await new Promise((r) => setTimeout(r, 2000));
      try {
        const s = await api(`/api/oauth/${provider}/status`);
        if (s.logged_in === true) {
          await refresh();
          return;
        }
      } catch {
        /* 瞬时错误忽略，继续轮询 */
      }
    }
    statusEl.textContent = t("settings.oauth.loopbackTimeout");
  }

  // ChatGPT / Codex 订阅 OAuth（与 Claude 同构，端点 /api/oauth/openai/*）。
  const oaiStatusEl = $("#sv-oai-status");
  const oaiLoginBtn = $<HTMLButtonElement>("#sv-oai-login");
  const oaiLogoutBtn = $<HTMLButtonElement>("#sv-oai-logout");
  const oaiStep = $("#sv-oai-step");
  const oaiUrl = $<HTMLInputElement>("#sv-oai-url");
  const oaiOpen = $<HTMLButtonElement>("#sv-oai-open");
  const oaiCopy = $<HTMLButtonElement>("#sv-oai-copy");
  const oaiCode = $<HTMLInputElement>("#sv-oai-code");
  const oaiSubmit = $<HTMLButtonElement>("#sv-oai-submit");
  let oaiVerifier: string | null = null;
  async function refreshOpenAIStatus(): Promise<void> {
    try {
      const r = await api("/api/oauth/openai/status");
      const on = r.logged_in === true;
      oaiStatusEl.textContent = on
        ? t("settings.chatgptOauth.loggedIn")
        : t("settings.oauth.notLoggedIn");
      oaiLoginBtn.textContent = on ? t("settings.oauth.relogin") : t("settings.oauth.login");
      oaiLogoutBtn.hidden = !on;
    } catch {
      oaiStatusEl.textContent = t("settings.oauth.unknown");
    }
  }
  oaiLoginBtn.addEventListener("click", async () => {
    try {
      // 桌面走 loopback 免粘贴；远程/回退走手动粘贴。服务端据 loopback 标志决定并在响应里回传。
      const r = await api("/api/oauth/openai/start", {
        method: "POST",
        body: JSON.stringify({ loopback: isTauri() }),
      });
      const url = String(r.url);
      const opened = await openExternal(url);
      if (r.loopback === true) {
        oaiStep.hidden = true;
        if (!opened) await copyText(url);
        oaiStatusEl.textContent = opened
          ? t("settings.oauth.waitingBrowser")
          : t("settings.oauth.loopbackCopied");
        void loopbackPoll("openai", oaiStatusEl, refreshOpenAIStatus);
      } else {
        oaiVerifier = String(r.verifier);
        oaiUrl.value = url;
        oaiStep.hidden = false;
        oaiStatusEl.textContent = opened
          ? t("settings.oauth.openedUrl")
          : t("settings.oauth.manualOpen");
        oaiCode.focus();
      }
    } catch (e) {
      oaiStatusEl.textContent = t("settings.oauth.loginFailed", { e: String(e) });
    }
  });
  oaiOpen.addEventListener("click", () => {
    if (oaiUrl.value) void openExternal(oaiUrl.value);
  });
  oaiCopy.addEventListener("click", async () => {
    if (oaiUrl.value && (await copyText(oaiUrl.value))) {
      oaiStatusEl.textContent = t("settings.oauth.copied");
    }
  });
  oaiSubmit.addEventListener("click", async () => {
    const code = oaiCode.value.trim();
    if (!code || !oaiVerifier) return;
    oaiStatusEl.textContent = t("settings.oauth.redeeming");
    const res = await api("/api/oauth/openai/finish", {
      method: "POST",
      body: JSON.stringify({ code, verifier: oaiVerifier }),
    });
    if (res.ok) {
      oaiVerifier = null;
      oaiStep.hidden = true;
      oaiCode.value = "";
      await refreshOpenAIStatus();
    } else {
      oaiStatusEl.textContent = t("settings.oauth.failed", {
        e: String(res.error ?? t("settings.oauth.unknownError")),
      });
    }
  });
  oaiLogoutBtn.addEventListener("click", async () => {
    await api("/api/oauth/openai/logout", { method: "POST", body: "{}" });
    await refreshOpenAIStatus();
  });

  // xAI Grok 订阅 OAuth（设备码流：展示 user_code + 链接，按 interval 轮询直至授权/过期）。
  const xaiStatusEl = $("#sv-xai-status");
  const xaiLoginBtn = $<HTMLButtonElement>("#sv-xai-login");
  const xaiLogoutBtn = $<HTMLButtonElement>("#sv-xai-logout");
  const xaiStep = $("#sv-xai-step");
  const xaiUrl = $<HTMLInputElement>("#sv-xai-url");
  const xaiOpen = $<HTMLButtonElement>("#sv-xai-open");
  const xaiCopy = $<HTMLButtonElement>("#sv-xai-copy");
  const xaiCode = $<HTMLInputElement>("#sv-xai-code");
  let xaiPollTimer: ReturnType<typeof setTimeout> | null = null;
  function stopXaiPoll(): void {
    if (xaiPollTimer !== null) {
      clearTimeout(xaiPollTimer);
      xaiPollTimer = null;
    }
  }
  async function refreshXaiStatus(): Promise<void> {
    try {
      const r = await api("/api/oauth/xai/status");
      const on = r.logged_in === true;
      xaiStatusEl.textContent = on
        ? t("settings.grokOauth.loggedIn")
        : t("settings.oauth.notLoggedIn");
      xaiLoginBtn.textContent = on ? t("settings.oauth.relogin") : t("settings.oauth.login");
      xaiLogoutBtn.hidden = !on;
    } catch {
      xaiStatusEl.textContent = t("settings.oauth.unknown");
    }
  }
  xaiLoginBtn.addEventListener("click", async () => {
    stopXaiPoll();
    try {
      const r = await api("/api/oauth/xai/start", { method: "POST", body: "{}" });
      if (r.ok !== true) {
        xaiStatusEl.textContent = t("settings.oauth.failed", {
          e: String(r.error ?? t("settings.oauth.unknownError")),
        });
        return;
      }
      const deviceCode = String(r.device_code);
      const url = String(r.verification_uri_complete || r.verification_uri);
      let intervalMs = Math.max(2, Number(r.interval) || 5) * 1000;
      const deadline = Date.now() + (Number(r.expires_in) || 300) * 1000;
      xaiUrl.value = url;
      xaiCode.value = String(r.user_code);
      xaiStep.hidden = false;
      const opened = await openExternal(url);
      xaiStatusEl.textContent = opened
        ? t("settings.grokOauth.waiting")
        : t("settings.oauth.manualOpen");
      const poll = async (): Promise<void> => {
        if (Date.now() > deadline) {
          xaiStatusEl.textContent = t("settings.grokOauth.expired");
          xaiStep.hidden = true;
          return;
        }
        try {
          const res = await api("/api/oauth/xai/poll", {
            method: "POST",
            body: JSON.stringify({ device_code: deviceCode }),
          });
          if (res.logged_in === true) {
            xaiStep.hidden = true;
            await refreshXaiStatus();
            return;
          }
          if (res.ok !== true) {
            xaiStatusEl.textContent = t("settings.oauth.failed", {
              e: String(res.error ?? t("settings.oauth.unknownError")),
            });
            xaiStep.hidden = true;
            return;
          }
          if (res.slow_down === true) intervalMs += 5000; // RFC 8628：服务端要求放慢
        } catch {
          /* 网络抖动：按原间隔继续轮询 */
        }
        xaiPollTimer = setTimeout(() => void poll(), intervalMs);
      };
      xaiPollTimer = setTimeout(() => void poll(), intervalMs);
    } catch (e) {
      xaiStatusEl.textContent = t("settings.oauth.loginFailed", { e: String(e) });
    }
  });
  xaiOpen.addEventListener("click", () => {
    if (xaiUrl.value) void openExternal(xaiUrl.value);
  });
  xaiCopy.addEventListener("click", async () => {
    if (xaiUrl.value && (await copyText(xaiUrl.value))) {
      xaiStatusEl.textContent = t("settings.oauth.copied");
    }
  });
  xaiLogoutBtn.addEventListener("click", async () => {
    stopXaiPoll();
    await api("/api/oauth/xai/logout", { method: "POST", body: "{}" });
    await refreshXaiStatus();
  });

  // Gemini（Google 账号 / Code Assist）订阅 OAuth（手动粘贴，端点 /api/oauth/gemini/*）。
  const gemStatusEl = $("#sv-gem-status");
  const gemLoginBtn = $<HTMLButtonElement>("#sv-gem-login");
  const gemLogoutBtn = $<HTMLButtonElement>("#sv-gem-logout");
  const gemStep = $("#sv-gem-step");
  const gemUrl = $<HTMLInputElement>("#sv-gem-url");
  const gemOpen = $<HTMLButtonElement>("#sv-gem-open");
  const gemCopy = $<HTMLButtonElement>("#sv-gem-copy");
  const gemCode = $<HTMLInputElement>("#sv-gem-code");
  const gemSubmit = $<HTMLButtonElement>("#sv-gem-submit");
  let gemVerifier: string | null = null;
  async function refreshGeminiStatus(): Promise<void> {
    try {
      const r = await api("/api/oauth/gemini/status");
      const on = r.logged_in === true;
      const email = typeof r.email === "string" && r.email ? r.email : "";
      gemStatusEl.textContent = on
        ? email
          ? t("settings.geminiOauth.loggedInAs", { email })
          : t("settings.geminiOauth.loggedIn")
        : t("settings.oauth.notLoggedIn");
      gemLoginBtn.textContent = on ? t("settings.oauth.relogin") : t("settings.oauth.login");
      gemLogoutBtn.hidden = !on;
    } catch {
      gemStatusEl.textContent = t("settings.oauth.unknown");
    }
  }
  gemLoginBtn.addEventListener("click", async () => {
    try {
      const r = await api("/api/oauth/gemini/start", {
        method: "POST",
        body: JSON.stringify({ loopback: isTauri() }),
      });
      const url = String(r.url);
      const opened = await openExternal(url);
      if (r.loopback === true) {
        gemStep.hidden = true;
        if (!opened) await copyText(url);
        gemStatusEl.textContent = opened
          ? t("settings.oauth.waitingBrowser")
          : t("settings.oauth.loopbackCopied");
        void loopbackPoll("gemini", gemStatusEl, refreshGeminiStatus);
      } else {
        gemVerifier = String(r.verifier);
        gemUrl.value = url;
        gemStep.hidden = false;
        gemStatusEl.textContent = opened
          ? t("settings.oauth.openedCode")
          : t("settings.oauth.manualOpen");
        gemCode.focus();
      }
    } catch (e) {
      gemStatusEl.textContent = t("settings.oauth.loginFailed", { e: String(e) });
    }
  });
  gemOpen.addEventListener("click", () => {
    if (gemUrl.value) void openExternal(gemUrl.value);
  });
  gemCopy.addEventListener("click", async () => {
    if (gemUrl.value && (await copyText(gemUrl.value))) {
      gemStatusEl.textContent = t("settings.oauth.copied");
    }
  });
  gemSubmit.addEventListener("click", async () => {
    const code = gemCode.value.trim();
    if (!code || !gemVerifier) return;
    gemStatusEl.textContent = t("settings.oauth.redeeming");
    const res = await api("/api/oauth/gemini/finish", {
      method: "POST",
      body: JSON.stringify({ code, verifier: gemVerifier }),
    });
    if (res.ok) {
      gemVerifier = null;
      gemStep.hidden = true;
      gemCode.value = "";
      await refreshGeminiStatus();
    } else {
      gemStatusEl.textContent = t("settings.oauth.failed", {
        e: String(res.error ?? t("settings.oauth.unknownError")),
      });
    }
  });
  gemLogoutBtn.addEventListener("click", async () => {
    await api("/api/oauth/gemini/logout", { method: "POST", body: "{}" });
    await refreshGeminiStatus();
  });

  const accessToggle = $<HTMLButtonElement>("#sv-access-toggle");
  const accessKeyRow = $("#sv-access-key-row");
  const accessEnvNote = $("#sv-access-env-note");
  const accessKeyInput = $<HTMLInputElement>("#sv-access-key");
  const confirmToggle = $<HTMLButtonElement>("#sv-confirm-toggle");
  const autoMemToggle = $<HTMLButtonElement>("#sv-automem-toggle");
  const autoTrimToggle = $<HTMLButtonElement>("#sv-autotrim-toggle");
  const defaultSel = $<HTMLSelectElement>("#sv-default");
  const workspaceInput = $<HTMLInputElement>("#sv-workspace");
  const workspacePick = $<HTMLButtonElement>("#sv-workspace-pick");
  const workspaceHint = $("#sv-workspace-hint");
  const proxyInput = $<HTMLInputElement>("#sv-proxy");
  const maxIterInput = $<HTMLInputElement>("#sv-maxiter");
  const maxIterInteractiveInput = $<HTMLInputElement>("#sv-maxiter-interactive");
  const maxIterSubagentInput = $<HTMLInputElement>("#sv-maxiter-subagent");
  const logCleanToggle = $<HTMLButtonElement>("#sv-logclean-toggle");
  const logMaxRow = $("#sv-logmax-row");
  const logMaxInput = $<HTMLInputElement>("#sv-logmax");
  const wsProvider = $<HTMLSelectElement>("#sv-ws-provider");
  const wsBaseRow = $("#sv-ws-base-row");
  const wsBase = $<HTMLInputElement>("#sv-ws-base");
  const wsKeyRow = $("#sv-ws-key-row");
  const wsKey = $<HTMLInputElement>("#sv-ws-key");

  let accessOn = false;
  // 密钥由环境变量 WC_ACCESS_KEY 设定时为 true：开关点亮但锁定（只能在服务器改）。
  let accessEnvManaged = false;
  let confirmOn = false; // = !auto_approve
  let logCleanOn = true; // 定时任务日志自动清理，未配置时默认开启

  function fmtPrice(p?: number | null): string {
    return p === null || p === undefined ? "—" : `¥${p}`;
  }

  function renderModels(llms: LlmRow[], activeId: string | null): void {
    const box = $("#sv-models");
    box.replaceChildren();
    if (llms.length === 0) {
      box.innerHTML = `<div class="empty">${t("settings.models.empty")}</div>`;
    }
    for (const l of llms) {
      const isDefault = l.id === activeId;
      const row = document.createElement("div");
      row.className = "model-row";
      const tile = provName(l.provider).slice(0, 1).toUpperCase();
      row.innerHTML = `
        <div class="model-tile">${esc(tile)}</div>
        <div class="model-main">
          <div class="model-name">${esc(l.model || l.id)}${isDefault ? `<span class="badge accent">${t("settings.models.badge.default")}</span>` : ""}${l.vision ? `<span class="badge" title="${t("settings.models.badge.visionTitle")}">${t("settings.models.badge.vision")}</span>` : ""}</div>
          <div class="model-meta">${esc(provName(l.provider))}</div>
        </div>
        <div class="model-price">
          <div>${fmtPrice(l.price_in)} <span class="t3">${t("settings.models.priceIn")}</span> / ${fmtPrice(l.price_out)} <span class="t3">${t("settings.models.priceOut")}</span></div>
          <div class="t3">${t("settings.models.priceUnit")}</div>
        </div>
        ${keyBadge(l)}
        <div class="model-actions"></div>`;
      const actions = row.querySelector(".model-actions") as HTMLElement;
      const edit = document.createElement("button");
      edit.className = "btn btn-sm";
      edit.textContent = t("settings.models.configure");
      edit.onclick = () => openModal(l);
      const del = document.createElement("button");
      del.className = "btn btn-sm btn-danger";
      del.textContent = t("settings.models.delete");
      del.onclick = () => void removeLlm(l.id);
      actions.append(edit, del);
      box.appendChild(row);
    }

    // 默认模型下拉
    defaultSel.innerHTML = llms
      .map(
        (l) =>
          `<option value="${esc(l.id)}">${t("settings.models.optionLabel", { model: esc(l.model || l.id), provider: esc(provName(l.provider)) })}</option>`,
      )
      .join("");
    defaultSel.disabled = llms.length === 0;
    if (activeId) defaultSel.value = activeId;
  }

  async function refresh(): Promise<void> {
    setStatus(t("settings.status.loading"));
    try {
      if (providers.length === 0) {
        providers = ((await api("/api/providers")).providers as Provider[]) ?? [];
      }
      const cfg = await api("/api/config");
      renderModels((cfg.llms as LlmRow[]) ?? [], (cfg.active_llm as string | null) ?? null);
      // 开关反映「服务器是否真在要求密钥」（env 或 config 任一即生效）；
      // 来自环境变量时锁定为只读（前端改不动，需在服务器修改）。
      accessOn = !!cfg.access_key_active;
      accessEnvManaged = !!cfg.access_key_env;
      confirmOn = cfg.auto_approve === false;
      accessToggle.classList.toggle("on", accessOn);
      accessToggle.disabled = accessEnvManaged;
      accessToggle.style.opacity = accessEnvManaged ? "0.5" : "";
      accessToggle.style.cursor = accessEnvManaged ? "not-allowed" : "";
      accessEnvNote.hidden = !accessEnvManaged;
      // 环境变量管理时隐藏手填输入框（改不了），只显示提示。
      accessKeyRow.hidden = !accessOn || accessEnvManaged;
      accessKeyInput.value = "";
      accessKeyInput.disabled = accessEnvManaged;
      accessKeyInput.placeholder = cfg.access_key_set
        ? t("settings.keySetPlaceholder")
        : t("settings.access.keyPlaceholder");
      confirmToggle.classList.toggle("on", confirmOn);
      autoMemToggle.classList.toggle("on", cfg.auto_memory === true);
      // 未配置时服务端已折算成默认开启，这里直接照搬即可。
      autoTrimToggle.classList.toggle("on", cfg.auto_trim_context !== false);
      workspaceInput.value = (cfg.workspace as string) ?? "";
      workspaceHint.textContent = isTauri()
        ? t("settings.workspace.hintTauri")
        : t("settings.workspace.hintWeb");
      proxyInput.value = (cfg.proxy as string) ?? "";
      maxIterInput.value = cfg.max_iterations != null ? String(cfg.max_iterations) : "";
      maxIterInteractiveInput.value =
        cfg.max_iterations_interactive != null ? String(cfg.max_iterations_interactive) : "";
      maxIterSubagentInput.value =
        cfg.subagent_max_iterations != null ? String(cfg.subagent_max_iterations) : "";
      // 未配置(null)= 默认开启；关掉时体积上限那一行没有意义，直接收起来。
      logCleanOn = cfg.cron_log_auto_clean !== false;
      logCleanToggle.classList.toggle("on", logCleanOn);
      logMaxRow.hidden = !logCleanOn;
      logMaxInput.value = cfg.cron_log_max_mb != null ? String(cfg.cron_log_max_mb) : "";
      const ws = (cfg.web_search as Record<string, unknown>) ?? {};
      wsProvider.value = (ws.provider as string) ?? "";
      wsBase.value = (ws.base_url as string) ?? "";
      wsKey.value = "";
      wsKey.placeholder = ws.api_key_set ? t("settings.keySetPlaceholder") : "sk-…";
      syncWsRows();
      const mcp = (cfg.mcp_servers as Record<string, unknown>) ?? {};
      mcpInput.value = Object.keys(mcp).length ? JSON.stringify(mcp, null, 2) : "";
      void loadMcpTools();
      const hk = (cfg.hooks as Record<string, unknown>) ?? {};
      hooksInput.value = Object.keys(hk).length ? JSON.stringify(hk, null, 2) : "";
      effortSel.value = (cfg.reasoning_effort as string) ?? "";
      void refreshOAuthStatus();
      void refreshOpenAIStatus();
      void refreshXaiStatus();
      void refreshGeminiStatus();
      setStatus("");
    } catch (e) {
      setStatus(t("settings.status.loadFailed", { e: String(e) }));
    }
  }

  async function activate(id: string): Promise<void> {
    await api("/api/config", { method: "POST", body: JSON.stringify({ active_llm: id }) });
    notifyChanged();
    void refresh();
  }
  async function removeLlm(id: string): Promise<void> {
    await api(`/api/config/llm/${encodeURIComponent(id)}`, { method: "DELETE" });
    notifyChanged();
    void refresh();
  }

  defaultSel.addEventListener("change", () => {
    if (defaultSel.value) void activate(defaultSel.value);
  });

  // 工作目录：编辑/选择后保存到 config；notifyChanged 让聊天框工作目录 pill 同步刷新。
  async function saveWorkspace(v: string): Promise<void> {
    await api("/api/config", { method: "POST", body: JSON.stringify({ workspace: v }) });
    notifyChanged();
    setStatus(v ? t("settings.status.workspaceUpdated") : t("settings.status.workspaceReset"));
  }
  workspaceInput.addEventListener("change", () => void saveWorkspace(workspaceInput.value.trim()));

  // 联网搜索：按 provider 显隐 base_url / api_key 行。
  function syncWsRows(): void {
    const p = wsProvider.value;
    wsBaseRow.hidden = p !== "searxng";
    wsKeyRow.hidden = p !== "brave" && p !== "tavily";
  }
  async function saveWebSearch(): Promise<void> {
    const body: Record<string, unknown> = {
      provider: wsProvider.value,
      base_url: wsBase.value.trim(),
      api_key: wsKey.value, // 空串=保留既有
    };
    await api("/api/config", { method: "POST", body: JSON.stringify({ web_search: body }) });
    setStatus(t("settings.status.websearchUpdated"));
  }
  wsProvider.addEventListener("change", () => {
    syncWsRows();
    void saveWebSearch();
  });

  // MCP 服务器：JSON 编辑 → 保存重连；列出已发现工具。
  const mcpInput = $<HTMLTextAreaElement>("#sv-mcp");
  const mcpMsg = $("#sv-mcp-msg");
  const mcpTools = $("#sv-mcp-tools");
  async function loadMcpTools(): Promise<void> {
    try {
      const r = (await api("/api/mcp/tools")) as { tools?: { server: string; tool: string }[] };
      const list = r.tools ?? [];
      mcpTools.textContent = list.length
        ? list.map((x) => `· mcp__${x.server}__${x.tool}`).join("\n")
        : t("settings.mcp.noTools");
    } catch {
      mcpTools.textContent = t("settings.mcp.noTools");
    }
  }
  $<HTMLButtonElement>("#sv-mcp-save").addEventListener("click", () => {
    const raw = mcpInput.value.trim();
    let parsed: unknown = {};
    if (raw) {
      try {
        parsed = JSON.parse(raw);
      } catch (e) {
        mcpMsg.textContent = t("settings.status.jsonParseFailed", { e: String(e) });
        return;
      }
    }
    mcpMsg.textContent = t("settings.status.saving");
    void api("/api/config", { method: "POST", body: JSON.stringify({ mcp_servers: parsed }) }).then(
      () => {
        mcpMsg.textContent = t("settings.mcp.savedReconnecting");
        // 给后台连接一点时间，再刷新已发现工具。
        setTimeout(() => void loadMcpTools(), 2500);
      },
    );
  });

  // 推理强度：选择即保存（热重载生效）。
  const effortSel = $<HTMLSelectElement>("#sv-effort");
  effortSel.addEventListener("change", () => {
    void api("/api/config", {
      method: "POST",
      body: JSON.stringify({ reasoning_effort: effortSel.value }),
    }).then(() => setStatus(t("settings.status.effortUpdated")));
  });

  // 事件钩子：JSON 编辑 → 保存（for_event 实时读盘，下次事件即生效）。
  const hooksInput = $<HTMLTextAreaElement>("#sv-hooks");
  const hooksMsg = $("#sv-hooks-msg");
  $<HTMLButtonElement>("#sv-hooks-save").addEventListener("click", () => {
    const raw = hooksInput.value.trim();
    let parsed: unknown = {};
    if (raw) {
      try {
        parsed = JSON.parse(raw);
      } catch (e) {
        hooksMsg.textContent = t("settings.status.jsonParseFailed", { e: String(e) });
        return;
      }
    }
    hooksMsg.textContent = t("settings.status.saving");
    void api("/api/config", { method: "POST", body: JSON.stringify({ hooks: parsed }) }).then(
      () => {
        hooksMsg.textContent = t("settings.status.saved");
      },
    );
  });
  wsBase.addEventListener("change", () => void saveWebSearch());
  wsKey.addEventListener("change", () => void saveWebSearch());

  // 网络代理：保存到 config 并热重载（notifyChanged 让 agent/LLM 客户端重建带代理）。
  proxyInput.addEventListener("change", () => {
    void api("/api/config", {
      method: "POST",
      body: JSON.stringify({ proxy: proxyInput.value.trim() }),
    }).then(() => {
      notifyChanged();
      setStatus(
        proxyInput.value.trim() ? t("settings.status.proxySet") : t("settings.status.proxyOff"),
      );
    });
  });
  // 无人值守任务工具调用上限：保存到 config 并热重载。
  maxIterInput.addEventListener("change", () => {
    const v = maxIterInput.value.trim();
    void api("/api/config", {
      method: "POST",
      body: JSON.stringify({ max_iterations: v }),
    }).then(() => {
      notifyChanged();
      setStatus(v ? t("settings.status.maxiterSet", { v }) : t("settings.status.maxiterReset"));
    });
  });
  // 交互聊天工具调用上限：保存到 config 并热重载。
  maxIterInteractiveInput.addEventListener("change", () => {
    const v = maxIterInteractiveInput.value.trim();
    void api("/api/config", {
      method: "POST",
      body: JSON.stringify({ max_iterations_interactive: v }),
    }).then(() => {
      notifyChanged();
      setStatus(
        v
          ? t("settings.status.maxiterInteractiveSet", { v })
          : t("settings.status.maxiterInteractiveReset"),
      );
    });
  });
  // 子 agent 工具调用上限：保存到 config 并热重载。
  maxIterSubagentInput.addEventListener("change", () => {
    const v = maxIterSubagentInput.value.trim();
    void api("/api/config", {
      method: "POST",
      body: JSON.stringify({ subagent_max_iterations: v }),
    }).then(() => {
      notifyChanged();
      setStatus(
        v
          ? t("settings.status.maxiterSubagentSet", { v })
          : t("settings.status.maxiterSubagentReset"),
      );
    });
  });
  // 定时任务日志自动清理总开关。关掉=日志永久保留，体积上限那一行一并收起。
  logCleanToggle.addEventListener("click", () => {
    logCleanOn = !logCleanOn;
    logCleanToggle.classList.toggle("on", logCleanOn);
    logMaxRow.hidden = !logCleanOn;
    void api("/api/config", {
      method: "POST",
      body: JSON.stringify({ cron_log_auto_clean: logCleanOn }),
    }).then(() => {
      setStatus(t(logCleanOn ? "settings.status.logcleanOn" : "settings.status.logcleanOff"));
    });
  });
  // 单任务日志体积上限（MB）：空=默认 10。超限时从最旧的整次运行开始裁。即时生效。
  logMaxInput.addEventListener("change", () => {
    const v = logMaxInput.value.trim();
    void api("/api/config", {
      method: "POST",
      body: JSON.stringify({ cron_log_max_mb: v }),
    }).then(() => {
      const msg =
        v === "" ? t("settings.status.logcleanDefault") : t("settings.status.logcleanMb", { v });
      setStatus(t("settings.status.logcleanUpdated", { msg }));
    });
  });
  workspacePick.onclick = () => {
    void (async () => {
      const v = await pickDirectory(
        workspaceInput.value.trim(),
        t("settings.workspace.pickPrompt"),
      );
      if (v !== null) {
        workspaceInput.value = v;
        await saveWorkspace(v);
        void refresh();
      }
    })();
  };

  // 访问控制
  accessToggle.onclick = () => {
    if (accessEnvManaged) return; // 环境变量管理：UI 不可更改。
    accessOn = !accessOn;
    accessToggle.classList.toggle("on", accessOn);
    accessKeyRow.hidden = !accessOn;
    if (!accessOn) {
      // 关闭即清空密钥（公开模式）。
      void api("/api/config", { method: "POST", body: JSON.stringify({ access_key: "" }) }).then(
        () => setStatus(t("settings.status.accessKeyOff")),
      );
    }
  };
  accessKeyInput.addEventListener("change", () => {
    if (accessEnvManaged) return; // 环境变量管理：UI 不可更改。
    const k = accessKeyInput.value;
    if (!k) return;
    setKey(k);
    void api("/api/config", { method: "POST", body: JSON.stringify({ access_key: k }) }).then(() =>
      setStatus(t("settings.status.accessKeySet")),
    );
  });
  confirmToggle.onclick = () => {
    confirmOn = !confirmOn;
    confirmToggle.classList.toggle("on", confirmOn);
    void api("/api/config", {
      method: "POST",
      body: JSON.stringify({ auto_approve: !confirmOn }),
    }).then(() => setStatus(t("settings.status.updated")));
  };
  autoMemToggle.onclick = () => {
    const on = !autoMemToggle.classList.contains("on");
    autoMemToggle.classList.toggle("on", on);
    void api("/api/config", {
      method: "POST",
      body: JSON.stringify({ auto_memory: on }),
    }).then(() => setStatus(on ? t("settings.status.automemOn") : t("settings.status.automemOff")));
  };
  autoTrimToggle.onclick = () => {
    const on = !autoTrimToggle.classList.contains("on");
    autoTrimToggle.classList.toggle("on", on);
    void api("/api/config", {
      method: "POST",
      body: JSON.stringify({ auto_trim_context: on }),
    }).then(() =>
      setStatus(on ? t("settings.status.autotrimOn") : t("settings.status.autotrimOff")),
    );
  };

  // ── 添加 / 编辑模型弹窗 ──
  function openModal(row?: LlmRow): void {
    const editing = row?.id ?? "";
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    overlay.innerHTML = `
      <div class="modal">
        <div class="modal-head"><span class="ico-tile">${icon(editing ? "key" : "plus", 17)}</span><h3>${editing ? t("settings.modal.editTitle") : t("settings.modal.addTitle")}</h3></div>
        <div class="modal-body">
          <div class="field">
            <label>${t("settings.modal.provider")}</label>
            <select id="m-prov" class="select"></select>
          </div>
          <div class="field">
            <label>${t("settings.modal.endpoint")}</label>
            <input id="m-base" class="input mono" placeholder="https://…" />
            <div class="hint" id="m-base-hint"></div>
          </div>
          <div class="field">
            <label>Model ID</label>
            <input id="m-model-txt" class="input mono" list="m-model-list" placeholder="your-model-name" autocomplete="off" />
            <datalist id="m-model-list"></datalist>
            <div class="hint" id="m-model-hint"></div>
          </div>
          <div class="field">
            <label><input id="m-oauth" type="checkbox" style="vertical-align:-2px" /> ${t("settings.modal.useClaudeOauth")}</label>
            <div class="hint">${t("settings.modal.useClaudeOauthHint")}</div>
          </div>
          <div class="field">
            <label><input id="m-codex" type="checkbox" style="vertical-align:-2px" /> ${t("settings.modal.useChatgptOauth")}</label>
            <div class="hint">${t("settings.modal.useChatgptOauthHint")}</div>
          </div>
          <div class="field">
            <label><input id="m-grok" type="checkbox" style="vertical-align:-2px" /> ${t("settings.modal.useGrokOauth")}</label>
            <div class="hint">${t("settings.modal.useGrokOauthHint")}</div>
          </div>
          <div class="field">
            <label><input id="m-gemini" type="checkbox" style="vertical-align:-2px" /> ${t("settings.modal.useGeminiOauth")}</label>
            <div class="hint">${t("settings.modal.useGeminiOauthHint")}</div>
          </div>
          <div class="field">
            <label><input id="m-vision" type="checkbox" style="vertical-align:-2px" /> ${t("settings.modal.vision")}</label>
            <div class="hint">${t("settings.modal.visionHint")}</div>
          </div>
          <div class="field">
            <label>API Key</label>
            <input id="m-key" class="input mono" type="password" placeholder="sk-…" />
            <div class="hint">${t("settings.modal.apiKeyHint")}</div>
          </div>
          <div class="field">
            <label>${t("settings.modal.priceLabel")}</label>
            <div class="price-grid">
              <div class="input-group"><span class="input-affix">${t("settings.modal.priceIn")}</span><input id="m-pin" class="input mono" inputmode="decimal" placeholder="${t("settings.modal.priceInPlaceholder")}" /></div>
              <div class="input-group"><span class="input-affix">${t("settings.modal.priceOut")}</span><input id="m-pout" class="input mono" inputmode="decimal" placeholder="${t("settings.modal.priceOutPlaceholder")}" /></div>
              <div class="input-group"><span class="input-affix">${t("settings.modal.priceCache")}</span><input id="m-pcache" class="input mono" inputmode="decimal" placeholder="${t("settings.modal.priceCachePlaceholder")}" /></div>
            </div>
            <div class="hint">${t("settings.modal.priceHint")}</div>
          </div>
          <div class="field">
            <label>${t("settings.modal.maxtokLabel")}</label>
            <input id="m-maxtok" class="input mono" inputmode="numeric" placeholder="${t("settings.modal.maxtokPlaceholder")}" />
            <div class="hint">${t("settings.modal.maxtokHint")}</div>
          </div>
          <div class="field">
            <label>${t("settings.modal.effortLabel")}</label>
            <select id="m-effort" class="select">
              <option value="__default__">${t("settings.modal.effortDefault")}</option>
              <option value="">${t("settings.modal.effortOff")}</option>
              <option value="low">low</option>
              <option value="medium">medium</option>
              <option value="high">high</option>
              <option value="xhigh">${t("settings.effort.xhigh")}</option>
              <option value="max">max</option>
            </select>
            <div class="hint">${t("settings.modal.effortHint")}</div>
          </div>
        </div>
        <div class="modal-foot">
          <span id="m-msg" class="muted" style="flex:1;font-size:12.5px"></span>
          <button id="m-cancel" class="btn">${t("common.cancel")}</button>
          <button id="m-test" class="btn">${icon("bolt", 14)}${t("settings.modal.test")}</button>
          <button id="m-save" class="btn btn-primary">${editing ? t("common.save") : t("common.add")}</button>
        </div>
      </div>`;
    document.body.appendChild(overlay);
    const q = <T extends HTMLElement>(s: string) => overlay.querySelector(s) as T;
    const prov = q<HTMLSelectElement>("#m-prov");
    const base = q<HTMLInputElement>("#m-base");
    const modelTxt = q<HTMLInputElement>("#m-model-txt");
    const modelList = q<HTMLDataListElement>("#m-model-list");
    const baseHint = q("#m-base-hint");
    const modelHint = q("#m-model-hint");
    prov.innerHTML = providers
      .map((p) => `<option value="${p.id}">${esc(p.name)}</option>`)
      .join("");

    const isCompatNow = (): boolean => prov.value === "openai-compatible";
    const currentModel = (): string => modelTxt.value.trim();

    function applyProvider(initial?: boolean): void {
      const p = providers.find((x) => x.id === prov.value);
      const isCompat = isCompatNow();
      // openai-compatible 没有预设端点/模型，一切靠手填。
      const st = modelFieldState(isCompat ? undefined : p, {
        initial,
        savedModel: row?.model,
        savedBaseUrl: row?.base_url,
      });
      // 端点与模型 ID **始终可编辑**：模型迭代太快，写死清单就得跟着发版；
      // 反代 / 自建网关 / 区域端点也都要能改。预设值只作初值与 datalist 建议。
      base.value = st.baseUrl;
      base.readOnly = false;
      base.classList.remove("locked");
      baseHint.textContent = isCompat
        ? t("settings.modal.baseHintCompat")
        : t("settings.modal.baseHintPreset");
      modelTxt.value = st.model;
      modelList.innerHTML = st.suggestions
        .map((m) => `<option value="${esc(m)}"></option>`)
        .join("");
      modelHint.textContent =
        st.suggestions.length > 0
          ? t("settings.modal.modelHintPreset")
          : t("settings.modal.modelHintCompat");
      // vision 复选框：编辑时用已存值；否则按所选模型是否在该 provider 的视觉清单里自动勾选。
      const visionBox = q<HTMLInputElement>("#m-vision");
      if (initial && row?.vision !== undefined) {
        visionBox.checked = row.vision;
      } else {
        visionBox.checked = !!p?.vision_models?.includes(currentModel());
      }
    }

    // 初值
    prov.value = row?.provider ?? providers[0]?.id ?? "";
    q<HTMLInputElement>("#m-pin").value = row?.price_in != null ? String(row.price_in) : "";
    q<HTMLInputElement>("#m-pout").value = row?.price_out != null ? String(row.price_out) : "";
    q<HTMLInputElement>("#m-maxtok").value = row?.max_tokens != null ? String(row.max_tokens) : "";
    q<HTMLInputElement>("#m-oauth").checked = row?.claude_oauth === true;
    q<HTMLInputElement>("#m-codex").checked = row?.openai_codex === true;
    q<HTMLInputElement>("#m-grok").checked = row?.xai_grok === true;
    q<HTMLInputElement>("#m-gemini").checked = row?.gemini_oauth === true;
    q<HTMLInputElement>("#m-pcache").value =
      row?.price_cache_read != null ? String(row.price_cache_read) : "";
    // 推理强度：undefined/null=跟随全局默认（哨兵 __default__）；""=显式关闭；其余=具体档位。
    q<HTMLSelectElement>("#m-effort").value =
      row?.reasoning_effort == null ? "__default__" : row.reasoning_effort;
    const keyInput = q<HTMLInputElement>("#m-key");
    keyInput.placeholder = row?.api_key_set ? t("settings.keySetPlaceholder") : "sk-…";
    applyProvider(true);

    prov.addEventListener("change", () => applyProvider());
    // 切换模型时按视觉清单重置 vision 默认（用户随后仍可手动改）。
    const syncVisionDefault = (): void => {
      const p = providers.find((x) => x.id === prov.value);
      q<HTMLInputElement>("#m-vision").checked = !!p?.vision_models?.includes(currentModel());
    };
    modelTxt.addEventListener("input", syncVisionDefault);
    modelTxt.addEventListener("change", syncVisionDefault);

    // 测试连接：用当前表单值发一条极小请求，验证模型可用（不保存）。
    q<HTMLButtonElement>("#m-test").addEventListener("click", async () => {
      if (isCompatNow() && !base.value.trim()) {
        q("#m-msg").textContent = t("settings.modal.needEndpoint");
        return;
      }
      const model = currentModel();
      if (!model) {
        q("#m-msg").textContent = t("settings.modal.needModel");
        return;
      }
      const testBtn = q<HTMLButtonElement>("#m-test");
      testBtn.disabled = true;
      q("#m-msg").textContent = t("settings.modal.testing");
      try {
        const res = (await api("/api/config/llm/test", {
          method: "POST",
          body: JSON.stringify({
            id: editing,
            provider: prov.value,
            model,
            base_url: base.value.trim(),
            api_key: keyInput.value, // 空=用已存的 key（编辑时）
          }),
        })) as { ok?: boolean; error?: string };
        q("#m-msg").textContent = res.ok
          ? t("settings.modal.testOk")
          : t("settings.modal.testFail", {
              e: String(res.error ?? t("settings.oauth.unknownError")),
            });
      } finally {
        testBtn.disabled = false;
      }
    });

    const close = () => overlay.remove();
    q("#m-cancel").addEventListener("click", close);
    // 真·模态：**不**做「点遮罩关闭」。本弹窗是要填 provider/端点/模型/Key 的表单，误关就全丢。
    // 尤其：在输入框里按住拖选文字、拖到框外松手时，click 的 target 会变成遮罩 —— 于是「鼠标一划出
    // 就关了、填一半没了」。出口只留显式的 取消 / 保存。
    q<HTMLButtonElement>("#m-save").addEventListener("click", async () => {
      if (isCompatNow() && !base.value.trim()) {
        q("#m-msg").textContent = t("settings.modal.needEndpoint");
        return;
      }
      const model = currentModel();
      if (!model) {
        q("#m-msg").textContent = t("settings.modal.needModel");
        return;
      }
      const body: Record<string, unknown> = {
        id: editing,
        name: model, // 名称即模型 ID（不再单独填写）
        provider: prov.value,
        model,
        base_url: base.value.trim(),
        price_in: q<HTMLInputElement>("#m-pin").value.trim(),
        price_out: q<HTMLInputElement>("#m-pout").value.trim(),
        price_cache_read: q<HTMLInputElement>("#m-pcache").value.trim(),
        max_tokens: q<HTMLInputElement>("#m-maxtok").value.trim(),
        claude_oauth: q<HTMLInputElement>("#m-oauth").checked,
        openai_codex: q<HTMLInputElement>("#m-codex").checked,
        xai_grok: q<HTMLInputElement>("#m-grok").checked,
        gemini_oauth: q<HTMLInputElement>("#m-gemini").checked,
        vision: q<HTMLInputElement>("#m-vision").checked,
      };
      // 推理强度：哨兵 __default__ = 不发该字段（后端置 None=跟随全局）；其余（含 ""=关闭）原样发。
      const effort = q<HTMLSelectElement>("#m-effort").value;
      if (effort !== "__default__") body.reasoning_effort = effort;
      const key = keyInput.value;
      if (key) body.api_key = key;
      q("#m-msg").textContent = t("settings.status.saving");
      const res = await api("/api/config/llm", { method: "POST", body: JSON.stringify(body) });
      if (res.ok) {
        close();
        notifyChanged();
        void refresh();
      } else {
        q("#m-msg").textContent = t("settings.oauth.failed", {
          e: String(res.error ?? t("settings.oauth.unknownError")),
        });
      }
    });
  }

  $("#sv-add").addEventListener("click", () => openModal());

  return { refresh };
}
