import { beforeEach, describe, expect, it } from "vitest";
import {
  LOCALES,
  applyDomI18n,
  detectLocale,
  detectLocaleAsync,
  dicts,
  setLocale,
  t,
} from "./i18n";

describe("dictionaries parity", () => {
  it("every locale defines exactly the base (en) key set", () => {
    const base = Object.keys(dicts.en).sort();
    for (const { code } of LOCALES) {
      expect(Object.keys(dicts[code]).sort(), `locale ${code}`).toEqual(base);
    }
  });
});

describe("t", () => {
  beforeEach(() => setLocale("en"));

  it("returns the string for the current locale", () => {
    setLocale("zh-CN");
    expect(t("nav.chat")).toBe("对话");
    setLocale("en");
    expect(t("nav.chat")).toBe("Chat");
  });

  it("interpolates {x}-style params", () => {
    setLocale("en");
    expect(t("common.itemCount", { n: 3 })).toBe("3 items");
  });

  it("falls back to the raw key for unknown keys", () => {
    expect(t("does.not.exist" as never)).toBe("does.not.exist");
  });
});

describe("detectLocale", () => {
  it("prefers a valid stored value over navigator", () => {
    expect(detectLocale({ stored: "ja", languages: ["en-US"] })).toBe("ja");
  });

  it("matches navigator language prefix when no stored value", () => {
    expect(detectLocale({ stored: null, languages: ["de-DE", "en"] })).toBe("de");
    expect(detectLocale({ stored: null, languages: ["ko"] })).toBe("ko");
    expect(detectLocale({ stored: null, languages: ["zh-TW"] })).toBe("zh-TW");
    expect(detectLocale({ stored: null, languages: ["zh-Hant"] })).toBe("zh-TW");
    expect(detectLocale({ stored: null, languages: ["zh-CN"] })).toBe("zh-CN");
    expect(detectLocale({ stored: null, languages: ["zh"] })).toBe("zh-CN");
  });

  it("defaults to English for unknown / empty inputs", () => {
    expect(detectLocale({ stored: null, languages: ["fr-FR"] })).toBe("en");
    expect(detectLocale({ stored: "bogus", languages: [] })).toBe("en");
  });
});

describe("detectLocaleAsync (桌面端含系统语言)", () => {
  it("已存语言优先于系统语言与 navigator", async () => {
    expect(
      await detectLocaleAsync({
        stored: "ja",
        osLocale: () => Promise.resolve("de-DE"),
        languages: ["ko"],
      }),
    ).toBe("ja");
  });

  it("无存储时用操作系统语言（优先于 navigator）", async () => {
    expect(
      await detectLocaleAsync({
        stored: null,
        osLocale: () => Promise.resolve("zh-TW"),
        languages: ["en-US"],
      }),
    ).toBe("zh-TW");
  });

  it("系统语言取不到/不识别时回退 navigator → en", async () => {
    expect(
      await detectLocaleAsync({
        stored: null,
        osLocale: () => Promise.resolve(null),
        languages: ["de-DE"],
      }),
    ).toBe("de");
    expect(
      await detectLocaleAsync({
        stored: null,
        osLocale: () => Promise.resolve("fr-FR"),
        languages: ["fr"],
      }),
    ).toBe("en");
  });
});

describe("applyDomI18n", () => {
  beforeEach(() => setLocale("en"));

  it("fills text and attributes from data-i18n markers", () => {
    const root = document.createElement("div");
    root.innerHTML =
      '<span data-i18n="nav.chat"></span><input data-i18n-attr-placeholder="composer.placeholder" />';
    applyDomI18n(root);
    expect(root.querySelector("span")?.textContent).toBe("Chat");
    expect(root.querySelector("input")?.getAttribute("placeholder")).toBe(
      t("composer.placeholder"),
    );
  });
});
