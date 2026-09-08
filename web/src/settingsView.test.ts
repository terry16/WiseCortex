import { beforeEach, describe, expect, it } from "vitest";
import { setLocale } from "./i18n";
import { type LlmRow, keyBadge, modelFieldState } from "./settingsView";

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

describe("模型弹窗的端点/模型初值", () => {
  const openai = {
    id: "openai",
    name: "OpenAI",
    default_model: "gpt-5",
    models: ["gpt-5", "gpt-5-mini"],
    base_url: "https://api.openai.com/v1",
  };

  // 需求来源：模型迭代太快，写死下拉清单意味着每出一个新模型都要改代码发版。
  // 现在预设机型只当建议，用户随时能填清单外的新 ID。
  it("预设 provider 把机型清单作为建议给出", () => {
    const st = modelFieldState(openai);
    expect(st.suggestions).toEqual(["gpt-5", "gpt-5-mini"]);
    expect(st.model).toBe("gpt-5"); // 默认模型作初值，省一次输入
    expect(st.baseUrl).toBe("https://api.openai.com/v1");
  });

  // 关键回归：编辑已有档时，用户填过的自定义值不能被预设覆盖回去。
  it("编辑已有档时保留用户填的模型与反代端点", () => {
    const st = modelFieldState(openai, {
      initial: true,
      savedModel: "gpt-6-turbo-preview", // 清单里没有的新模型
      savedBaseUrl: "https://my-proxy.example.com/v1",
    });
    expect(st.model).toBe("gpt-6-turbo-preview");
    expect(st.baseUrl).toBe("https://my-proxy.example.com/v1");
    // 建议仍在，方便用户想切回官方机型
    expect(st.suggestions).toContain("gpt-5");
  });

  // 老档可能只存了模型没存端点（早期端点是只读的，不会写进配置）。
  it("已有档没存端点时回落到 provider 预设", () => {
    const st = modelFieldState(openai, { initial: true, savedModel: "gpt-5", savedBaseUrl: null });
    expect(st.baseUrl).toBe("https://api.openai.com/v1");
  });

  // openai-compatible 走 undefined：没有任何预设，全靠手填。
  it("兼容端点不预填任何值", () => {
    const st = modelFieldState(undefined);
    expect(st.model).toBe("");
    expect(st.baseUrl).toBe("");
    expect(st.suggestions).toEqual([]);
  });

  // 切换 provider（initial=false）要用新 provider 的预设，不能粘着上一个的值。
  it("切换 provider 时改用新 provider 的预设", () => {
    const st = modelFieldState(openai, { initial: false, savedModel: "claude-sonnet-4-6" });
    expect(st.model).toBe("gpt-5");
  });
});
