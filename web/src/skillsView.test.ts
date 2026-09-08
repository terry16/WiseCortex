import { beforeEach, describe, expect, it, vi } from "vitest";
import { filterSkills, mountSkillsView, skillCounts } from "./skillsView";

type E = { name: string; description: string; source: string; enabled: boolean };
const mk = (name: string, source: string, description = ""): E => ({
  name,
  description,
  source,
  enabled: true,
});

// 刻意混入 "git" 这种后端目前不会返回的来源：将来多加一种安装方式时，它必须自动落进
// 「已安装」，而不是从所有分类里同时消失、变成谁都看不见的幽灵。
const ENTRIES: E[] = [
  mk("brainstorming", "builtin", "Explore user intent before implementation"),
  mk("web-search", "builtin", "Search the web"),
  mk("deploy-mud", "installed", "Deploy MUD changes to the server"),
  mk("create-quest", "workdir", "Author a new quest"),
  mk("legacy-thing", "git", "Imported the old way"),
];

const names = (es: E[]) => es.map((e) => e.name).sort();

describe("filterSkills", () => {
  it("空查询 + 全部：原样返回", () => {
    expect(filterSkills(ENTRIES, "all", "")).toHaveLength(5);
  });

  it("按分类筛：内置只留 builtin", () => {
    expect(names(filterSkills(ENTRIES, "builtin", ""))).toEqual(["brainstorming", "web-search"]);
  });

  it("按分类筛：项目技能单独成类，不再混进已安装", () => {
    expect(names(filterSkills(ENTRIES, "workdir", ""))).toEqual(["create-quest"]);
  });

  it("按分类筛：既非内置也非项目的来源统统算「已安装」", () => {
    expect(names(filterSkills(ENTRIES, "installed", ""))).toEqual(["deploy-mud", "legacy-thing"]);
  });

  it("搜索匹配名称，且不区分大小写", () => {
    expect(names(filterSkills(ENTRIES, "all", "WEB-SE"))).toEqual(["web-search"]);
  });

  it("搜索也匹配描述——名字里没有的词也能找到", () => {
    expect(names(filterSkills(ENTRIES, "all", "server"))).toEqual(["deploy-mud"]);
  });

  it("分类与搜索是「与」的关系", () => {
    expect(filterSkills(ENTRIES, "installed", "web")).toEqual([]);
    expect(names(filterSkills(ENTRIES, "builtin", "web"))).toEqual(["web-search"]);
  });

  it("纯空白查询等同于没查询——别让误敲的空格清空整页", () => {
    expect(filterSkills(ENTRIES, "all", "   ")).toHaveLength(5);
  });
});

describe("skillCounts", () => {
  it("无查询时是各分类的总数", () => {
    expect(skillCounts(ENTRIES, "")).toEqual({ all: 5, builtin: 2, workdir: 1, installed: 2 });
  });

  it("有查询时计的是命中数——这样标签上就能看出「要找的在哪一类」", () => {
    expect(skillCounts(ENTRIES, "search")).toEqual({
      all: 1,
      builtin: 1,
      workdir: 0,
      installed: 0,
    });
  });

  it("三个分类的命中数之和必须等于总数——不能有技能落在分类之外", () => {
    const c = skillCounts(ENTRIES, "e");
    expect(c.builtin + c.workdir + c.installed).toBe(c.all);
  });
});

// ── 视图层：把真实 DOM 挂起来，验证「标签 + 搜索」确实在筛卡片 ──────────────
// 纯函数绿了不代表页面对：上一版分组标题也「逻辑正确」，但屏幕上一个技能都没少。

/** 桩掉 fetch：api() 只用到 r.json()，返回这一个方法就够。 */
function stubCatalog(entries: unknown[]): void {
  globalThis.fetch = vi.fn(async () => ({
    json: async () => ({ entries }),
  })) as unknown as typeof fetch;
}

const CATALOG = [
  mk("brainstorming", "builtin", "Explore user intent"),
  mk("web-search", "builtin", "Search the web"),
  mk("deploy-mud", "installed", "Deploy MUD changes to the server"),
];

async function mountWith(entries: unknown[] = CATALOG) {
  stubCatalog(entries);
  const el = document.createElement("div");
  document.body.appendChild(el);
  const view = mountSkillsView(el);
  await (view.refresh() as unknown as Promise<void>);
  const cards = () => [...el.querySelectorAll(".sk-name")].map((n) => n.textContent);
  const catBtn = (c: string) => el.querySelector<HTMLElement>(`.sk-cats button[data-cat="${c}"]`);
  const search = el.querySelector<HTMLInputElement>("#sk-mine-q");
  const type = (v: string) => {
    if (!search) throw new Error("搜索框没渲染出来");
    search.value = v;
    search.dispatchEvent(new Event("input"));
  };
  return { el, cards, catBtn, search, type };
}

describe("技能页视图", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("渲染四个分类标签，角标是各类命中数", async () => {
    const { el, catBtn } = await mountWith();
    expect(el.querySelectorAll(".sk-cats button")).toHaveLength(4);
    expect(catBtn("all")?.querySelector(".sk-cat-n")?.textContent).toBe("3");
    expect(catBtn("builtin")?.querySelector(".sk-cat-n")?.textContent).toBe("2");
    expect(catBtn("installed")?.querySelector(".sk-cat-n")?.textContent).toBe("1");
  });

  it("没有项目技能时「项目」标签收起，不留一个恒为 0 的空档", async () => {
    const { catBtn } = await mountWith();
    expect(catBtn("workdir")?.hidden).toBe(true);
  });

  it("有项目技能时「项目」标签出现", async () => {
    const { catBtn } = await mountWith([
      ...CATALOG,
      mk("create-quest", "workdir", "Author a quest"),
    ]);
    expect(catBtn("workdir")?.hidden).toBe(false);
    expect(catBtn("workdir")?.querySelector(".sk-cat-n")?.textContent).toBe("1");
  });

  it("点分类标签，网格里真的只剩那一类的卡片", async () => {
    const { cards, catBtn } = await mountWith();
    expect(cards()).toHaveLength(3);
    catBtn("builtin")?.click();
    expect(cards().sort()).toEqual(["brainstorming", "web-search"]);
    catBtn("installed")?.click();
    expect(cards()).toEqual(["deploy-mud"]);
  });

  it("搜索按名称/描述即时收窄，且清空按钮跟着出现", async () => {
    const { el, cards, type } = await mountWith();
    expect(el.querySelector<HTMLElement>("#sk-mine-clear")?.hidden).toBe(true);
    type("server"); // 只在 deploy-mud 的描述里
    expect(cards()).toEqual(["deploy-mud"]);
    expect(el.querySelector<HTMLElement>("#sk-mine-clear")?.hidden).toBe(false);
  });

  it("搜索没命中时给的是「没匹配」而不是「一个技能都没有」", async () => {
    const { el, cards, type } = await mountWith();
    type("zzz-nonexistent");
    expect(cards()).toHaveLength(0);
    expect(el.querySelector(".empty")?.textContent).toContain("zzz-nonexistent");
  });

  it("清空按钮把结果还原", async () => {
    const { el, cards, type, search } = await mountWith();
    type("server");
    el.querySelector<HTMLElement>("#sk-mine-clear")?.click();
    expect(search?.value).toBe("");
    expect(cards()).toHaveLength(3);
  });
});
