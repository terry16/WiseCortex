// ── 轻量 i18n（无框架）────────────────────────────────────────────────────────
// 每种语言一个 locales/xx.ts 字典；en 为基准（定义 MessageKey 与最终兜底）。
// 切换语言写 localStorage 后 location.reload()，视图在 mount 时用 t() 重新求值，
// 无需逐视图改造。静态 HTML 用 data-i18n / data-i18n-attr-* 标记，启动时一次性填充。
import { de } from "./locales/de";
import { type MessageKey, en } from "./locales/en";
import { ja } from "./locales/ja";
import { ko } from "./locales/ko";
import { zhCN } from "./locales/zh-CN";
import { zhTW } from "./locales/zh-TW";

export type { MessageKey };
export type Locale = "en" | "zh-CN" | "zh-TW" | "ja" | "de" | "ko";

/** 选择器顺序 + 各语言母语名（让选择器一目了然）。 */
export const LOCALES: { code: Locale; native: string }[] = [
  { code: "en", native: "English" },
  { code: "zh-CN", native: "简体中文" },
  { code: "zh-TW", native: "繁體中文" },
  { code: "ja", native: "日本語" },
  { code: "de", native: "Deutsch" },
  { code: "ko", native: "한국어" },
];

export const dicts: Record<Locale, Record<MessageKey, string>> = {
  en,
  "zh-CN": zhCN,
  "zh-TW": zhTW,
  ja,
  de,
  ko,
};

const STORAGE_KEY = "wc.lang";
let current: Locale = "en";

export function getLocale(): Locale {
  return current;
}

/** 设置内存中的当前语言（不刷新）。供启动与测试使用。 */
export function setLocale(code: Locale): void {
  current = code;
  if (typeof document !== "undefined") document.documentElement.lang = code;
}

function isLocale(x: unknown): x is Locale {
  return typeof x === "string" && LOCALES.some((l) => l.code === x);
}

function matchLocale(raw: string): Locale | null {
  const lc = raw.toLowerCase();
  if (lc.startsWith("zh")) {
    if (lc.includes("tw") || lc.includes("hant") || lc.includes("hk") || lc.includes("mo"))
      return "zh-TW";
    return "zh-CN";
  }
  if (lc.startsWith("en")) return "en";
  if (lc.startsWith("ja")) return "ja";
  if (lc.startsWith("de")) return "de";
  if (lc.startsWith("ko")) return "ko";
  return null;
}

function safeStored(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

/** 优先级：已存语言 → navigator 语言前缀 → 默认 en。opts 仅供测试注入。 */
export function detectLocale(opts?: {
  stored?: string | null;
  languages?: readonly string[];
}): Locale {
  const stored = opts ? opts.stored : safeStored();
  if (isLocale(stored)) return stored;
  const langs =
    opts?.languages ??
    (typeof navigator !== "undefined" ? (navigator.languages ?? [navigator.language]) : []);
  for (const raw of langs) {
    const m = matchLocale(raw);
    if (m) return m;
  }
  return "en";
}

/**
 * 桌面壳（Tauri）里取操作系统语言：经 `@tauri-apps/plugin-os` 的 `locale()`
 * （withGlobalTauri 下挂在 `window.__TAURI__.os`，与 dialog/opener 同一访问方式）。
 * 浏览器/WebUI 或插件不可用时返回 null（回退到 navigator）。这里直接读全局而非
 * import platform.ts，避免与其 `import { t }` 形成循环依赖。
 */
async function tauriOsLocale(): Promise<string | null> {
  try {
    const os = (
      globalThis as {
        __TAURI__?: { os?: { locale?: () => Promise<string | null> } };
      }
    ).__TAURI__?.os;
    if (os?.locale) return (await os.locale()) ?? null;
  } catch {
    /* 插件缺失/调用失败 → 回退 navigator */
  }
  return null;
}

/**
 * 异步探测语言：已存语言 → **桌面端系统语言** → navigator → 默认 en。
 * 桌面壳里 navigator.language 未必反映系统语言，故优先问操作系统（os 插件）。
 * opts 仅供测试注入（osLocale/stored/languages）。
 */
export async function detectLocaleAsync(opts?: {
  stored?: string | null;
  osLocale?: () => Promise<string | null>;
  languages?: readonly string[];
}): Promise<Locale> {
  const stored = opts ? opts.stored : safeStored();
  if (isLocale(stored)) return stored;
  // 桌面端优先用真实系统语言。
  const osRaw = await (opts?.osLocale ?? tauriOsLocale)();
  if (osRaw) {
    const m = matchLocale(osRaw);
    if (m) return m;
  }
  // 回退到 navigator（浏览器/WebUI 的常规路径）→ en 兜底。
  return detectLocale({ stored: null, languages: opts?.languages });
}

/** 取译文：当前语言 → en 兜底 → 原始 key。params 做 {x} 插值。 */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const tmpl = dicts[current][key] ?? dicts.en[key] ?? (key as string);
  if (!params) return tmpl;
  return tmpl.replace(/\{(\w+)\}/g, (m, k) => (k in params ? String(params[k]) : m));
}

/** 翻译静态 HTML：data-i18n=文本；data-i18n-attr-<attr>=属性（如 placeholder/title）。 */
export function applyDomI18n(root: ParentNode = document): void {
  for (const el of root.querySelectorAll<HTMLElement>("[data-i18n]")) {
    const key = el.getAttribute("data-i18n");
    if (key) el.textContent = t(key as MessageKey);
  }
  for (const el of root.querySelectorAll<HTMLElement>("*")) {
    for (const attr of el.getAttributeNames()) {
      if (attr.startsWith("data-i18n-attr-")) {
        const target = attr.slice("data-i18n-attr-".length);
        const key = el.getAttribute(attr);
        if (key) el.setAttribute(target, t(key as MessageKey));
      }
    }
  }
}

/** 启动：探测语言（桌面端含系统语言，故为异步）→ 设置 → 翻译静态 HTML。 */
export async function initI18n(): Promise<void> {
  setLocale(await detectLocaleAsync());
  applyDomI18n(document);
}

/** 用户切换语言：持久化后刷新页面（视图重新 mount 即生效）。 */
export function switchLocale(code: Locale): void {
  try {
    localStorage.setItem(STORAGE_KEY, code);
  } catch {
    /* localStorage 不可用则忽略 */
  }
  location.reload();
}
