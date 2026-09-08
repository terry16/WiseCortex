import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installExternalLinkHandler } from "./platform";

/** 点一个元素，返回该次 click 事件是否被拦下（preventDefault）。 */
function click(el: Element): boolean {
  const ev = new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 });
  el.dispatchEvent(ev);
  return ev.defaultPrevented;
}

function anchor(html: string): HTMLAnchorElement {
  document.body.innerHTML = html;
  return document.body.querySelector("a") as HTMLAnchorElement;
}

describe("外部链接接管", () => {
  let opened: string[];
  let teardown: () => void;

  beforeEach(() => {
    opened = [];
    // openExternal 在非 Tauri 环境下回退到 window.open；jsdom 没实现，桩掉即可观测。
    vi.stubGlobal(
      "open",
      vi.fn((u: string) => {
        opened.push(u);
        return {} as Window;
      }),
    );
    teardown = installExternalLinkHandler();
  });
  afterEach(() => {
    teardown();
    document.body.innerHTML = "";
    vi.unstubAllGlobals();
  });

  it("http 链接被拦下，改为交给系统打开——否则整个壳会被导航走", () => {
    const prevented = click(anchor('<a href="https://example.com/a">x</a>'));
    expect(prevented).toBe(true);
    expect(opened).toEqual(["https://example.com/a"]);
  });

  it("点在链接内部的子元素上也算——marked 会渲染出 <a><code>…</code></a>", () => {
    document.body.innerHTML = '<a href="https://example.com/b"><code>inner</code></a>';
    const inner = document.body.querySelector("code") as HTMLElement;
    expect(click(inner)).toBe(true);
    expect(opened).toEqual(["https://example.com/b"]);
  });

  it("mailto 也交给系统", () => {
    expect(click(anchor('<a href="mailto:a@b.com">m</a>'))).toBe(true);
    expect(opened).toEqual(["mailto:a@b.com"]);
  });

  it("页内锚点不接管——那是应用自己的跳转", () => {
    expect(click(anchor('<a href="#sec">s</a>'))).toBe(false);
    expect(opened).toEqual([]);
  });

  it("相对链接不接管", () => {
    expect(click(anchor('<a href="/api/x">r</a>'))).toBe(false);
    expect(opened).toEqual([]);
  });

  it("没有 href 的 a 不接管", () => {
    document.body.innerHTML = "<a>no href</a>";
    const a = document.body.querySelector("a") as HTMLElement;
    expect(click(a)).toBe(false);
  });

  it("按钮之类的普通元素完全不受影响", () => {
    document.body.innerHTML = "<button>b</button>";
    const b = document.body.querySelector("button") as HTMLElement;
    expect(click(b)).toBe(false);
    expect(opened).toEqual([]);
  });

  it("卸载后不再接管", () => {
    teardown();
    expect(click(anchor('<a href="https://example.com/c">x</a>'))).toBe(false);
    expect(opened).toEqual([]);
  });
});
