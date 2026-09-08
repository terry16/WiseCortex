import { beforeEach, describe, expect, it } from "vitest";
import { setLocale } from "./i18n";
import { type LlmRow, keyBadge } from "./settingsView";

function row(over: Partial<LlmRow>): LlmRow {
  return { id: "x", api_key_set: false, ...over } as LlmRow;
}

describe("模型行状态徽章", () => {
  beforeEach(() => setLocale("zh-CN"));

  // 回归：订阅档走 OAuth，没有 api_key 是正常的。曾经徽章只看 api_key_set，
  // 于是登录好的 Claude/ChatGPT/Grok/Gemini 档全被标成「未配置」，看着像坏了。
  it("订阅档显示「订阅额度」而不是「未配置」", () => {
    const html = keyBadge(row({ subscription: true }));
    expect(html).toContain("订阅额度");
    expect(html).not.toContain("未配置");
    expect(html).toContain("green"); // 绿色=正常，别让它看着像出问题
  });

  it("有 key 的普通档显示「Key 已配置」", () => {
    const html = keyBadge(row({ api_key_set: true }));
    expect(html).toContain("Key 已配置");
    expect(html).toContain("green");
  });

  // 首配引导不能被误伤：真没配好的档还是要老实说未配置。
  it("既没 key 又不是订阅档才显示「未配置」", () => {
    const html = keyBadge(row({}));
    expect(html).toContain("未配置");
    expect(html).not.toContain("green");
  });

  // subscription 由服务端算（is_subscription），前端不该再看四个开关。
  // 这条钉住：即便某天 api_key_set 为 true，订阅档也优先显示订阅。
  it("订阅档即使带着 key 也按订阅显示", () => {
    const html = keyBadge(row({ subscription: true, api_key_set: true }));
    expect(html).toContain("订阅额度");
  });
});
