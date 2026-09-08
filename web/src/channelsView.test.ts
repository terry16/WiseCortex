import { beforeEach, describe, expect, it, vi } from "vitest";
import { mountChannelsView } from "./channelsView";

// ── 渠道页：微信 ClawBot 卡片 ────────────────────────────────────────────────
// ClawBot 是「状态驱动」渠道：连接状态来自后端 clawbot.json，而不是出站通道列表。
// 这条分支很容易在以后加渠道时被改坏（连接判定 / 开关禁用 / 不该有的出站按钮），
// 所以把它钉住。

interface ClawbotState {
  ready?: boolean;
  enabled?: boolean;
  bot_id?: string;
}

/** 记录本次挂载期间发生的写请求，供断言点击确实打了正确的接口。 */
let posts: { url: string; body: unknown }[] = [];

/** 桩掉 fetch：按路径分派；api() 只用到 r.json()。 */
function stubApi(clawbot: ClawbotState): void {
  posts = [];
  globalThis.fetch = vi.fn(async (url: unknown, init?: RequestInit) => {
    const u = String(url);
    if (init?.method === "POST") {
      posts.push({ url: u, body: JSON.parse(String(init.body ?? "null")) });
    }
    const json = async (): Promise<unknown> => {
      if (u.includes("/api/clawbot/config")) return clawbot;
      if (u.includes("/api/clawbot/enable")) return { ok: true };
      if (u.includes("/api/channels")) return { channels: [] };
      // 飞书 / QQ / 企业微信状态：本测试不关心，一律回空对象（视图会当作未配置）。
      return {};
    };
    return { json };
  }) as unknown as typeof fetch;
}

async function mountWith(clawbot: ClawbotState) {
  stubApi(clawbot);
  const el = document.createElement("div");
  document.body.appendChild(el);
  const view = mountChannelsView(el);
  await (view.refresh() as unknown as Promise<void>);

  const cards = () => [...el.querySelectorAll<HTMLElement>(".chan-card")];
  /** ClawBot 卡片按 logo 上的「微」定位——名称会随界面语言变，缩写不会。 */
  const card = (): HTMLElement => {
    const found = cards().find((c) => c.querySelector(".chan-logo")?.textContent === "微");
    if (!found) throw new Error("没找到微信 ClawBot 卡片");
    return found;
  };
  const buttons = () => [...card().querySelectorAll<HTMLButtonElement>(".chan-foot button")];
  return { el, card, buttons };
}

describe("渠道页 · 微信 ClawBot 卡片", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("卡片存在，且不提供出站「添加/连接」按钮（它不走 /api/channels）", async () => {
    const { buttons } = await mountWith({ ready: false, enabled: false });
    // 只有「扫码接入」和「开关」两个按钮。多出来的一般就是误加的出站按钮。
    expect(buttons()).toHaveLength(2);
  });

  it("未扫码时开关禁用——没有 token 可轮询，能点也是白点", async () => {
    const { buttons } = await mountWith({ ready: false, enabled: false });
    const toggle = buttons()[1];
    expect(toggle.disabled).toBe(true);
  });

  it("已扫码未开启时开关可点并高亮，卡片显示 Bot ID", async () => {
    const { card, buttons } = await mountWith({
      ready: true,
      enabled: false,
      bot_id: "b1@im.bot",
    });
    const toggle = buttons()[1];
    expect(toggle.disabled).toBe(false);
    expect(toggle.className).toContain("btn-primary");
    expect(card().textContent).toContain("b1@im.bot");
  });

  it("已开启时徽标是已连接，且提示「只能在一处开」", async () => {
    const { card } = await mountWith({ ready: true, enabled: true, bot_id: "b1@im.bot" });
    expect(card().querySelector(".badge")?.className).toContain("green");
    // 同步游标按 Bot 共享，两处同时轮询会把消息分走一半——这句提示必须在。
    expect(card().querySelector(".inline-note")).not.toBeNull();
  });

  it("点开关会 POST /api/clawbot/enable，并翻转成停止态", async () => {
    const { buttons } = await mountWith({ ready: true, enabled: false, bot_id: "b1@im.bot" });
    buttons()[1].click();
    await vi.waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0].url).toContain("/api/clawbot/enable");
    expect(posts[0].body).toEqual({ enabled: true });
    // 翻转后按钮应变成「点击停止」，而不是还停在「点击开启」。
    await vi.waitFor(() => expect(buttons()[1].textContent).toMatch(/stop|停止|中止|停/i));
  });

  it("clawbot 状态接口挂掉时整页仍能渲染——它只是可选状态，不该拖垮别的渠道", async () => {
    const el = document.createElement("div");
    document.body.appendChild(el);
    posts = [];
    globalThis.fetch = vi.fn(async (url: unknown) => {
      const u = String(url);
      if (u.includes("/api/clawbot/config")) throw new Error("boom");
      return { json: async () => (u.includes("/api/channels") ? { channels: [] } : {}) };
    }) as unknown as typeof fetch;
    const view = mountChannelsView(el);
    await (view.refresh() as unknown as Promise<void>);
    expect(el.querySelectorAll(".chan-card").length).toBeGreaterThan(1);
  });
});
