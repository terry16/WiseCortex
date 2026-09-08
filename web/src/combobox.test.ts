import { afterEach, describe, expect, it } from "vitest";
import { type ComboboxHandle, createCombobox } from "./combobox";

let box: ComboboxHandle | null = null;

function mount(items: string[], value?: string): ComboboxHandle {
  box = createCombobox({ items, value });
  document.body.appendChild(box.el);
  return box;
}

const input = (): HTMLInputElement =>
  (box as ComboboxHandle).el.querySelector(".cbx-input") as HTMLInputElement;
const pop = (): HTMLElement => document.querySelector(".cbx-pop") as HTMLElement;
const opts = (): string[] =>
  [...pop().querySelectorAll(".cbx-opt")].map((e) => e.textContent ?? "");

/** 展开：组件在 focus 时展开。 */
function openIt(): void {
  input().dispatchEvent(new FocusEvent("focus"));
}

function type(v: string): void {
  input().value = v;
  input().dispatchEvent(new Event("input"));
}

function key(k: string): void {
  input().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
}

afterEach(() => {
  box?.destroy();
  box = null;
});

describe("combobox 可选可填", () => {
  const models = ["gpt-5", "gpt-5-mini", "o3", "gpt-4.1"];

  it("展开时列出全部候选", () => {
    mount(models);
    openIt();
    expect(opts()).toEqual(models);
  });

  // 这是本次返工的核心回归：用 <input list=datalist> 时浏览器会拿输入内容
  // 去过滤候选，预填一个默认模型后列表就只剩一条，用户看到「模型只有一个了」。
  // 自研 combobox 必须无视输入内容，始终给出完整清单。
  it("有初值时展开仍是完整清单，不按输入过滤", () => {
    mount(models, "gpt-5");
    openIt();
    expect(opts()).toEqual(models);
  });

  it("输入清单外的内容后，列表依然完整", () => {
    mount(models);
    openIt();
    type("my-private-model-v9");
    expect(opts()).toEqual(models);
    expect(box?.value()).toBe("my-private-model-v9");
  });

  it("能填任意清单外的模型 ID", () => {
    mount(models);
    type("  spaced-model  ");
    expect(box?.value()).toBe("spaced-model"); // 顺手 trim
  });

  it("点选项即写入输入框并收起", () => {
    mount(models);
    openIt();
    const o3 = [...pop().querySelectorAll(".cbx-opt")].find(
      (e) => e.textContent === "o3",
    ) as HTMLElement;
    o3.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(box?.value()).toBe("o3");
    expect(pop().hidden).toBe(true);
  });

  it("方向键 + 回车可选中", () => {
    mount(models);
    openIt();
    key("ArrowDown"); // gpt-5
    key("ArrowDown"); // gpt-5-mini
    key("Enter");
    expect(box?.value()).toBe("gpt-5-mini");
  });

  it("Escape 只收起浮层，不冒泡去关外层弹窗", () => {
    mount(models);
    openIt();
    let bubbled = false;
    document.addEventListener("keydown", () => {
      bubbled = true;
    });
    key("Escape");
    expect(pop().hidden).toBe(true);
    expect(bubbled).toBe(false);
  });

  it("setItems 换清单、setValue 改值", () => {
    mount(models);
    box?.setItems(["a", "b"]);
    box?.setValue("b");
    openIt();
    expect(opts()).toEqual(["a", "b"]);
    expect(box?.value()).toBe("b");
  });

  it("没有候选项时不展开（兼容端点全靠手填）", () => {
    mount([]);
    openIt();
    expect(pop().hidden).toBe(true);
  });

  // 浮层挂在 body 上（躲开 .modal-body 的 overflow 裁剪），
  // 所以必须显式清理，否则关弹窗后会残留幽灵浮层。
  it("destroy 后浮层从 body 上移除", () => {
    mount(models);
    openIt();
    expect(document.querySelector(".cbx-pop")).not.toBeNull();
    box?.destroy();
    expect(document.querySelector(".cbx-pop")).toBeNull();
    box = null;
  });
});
