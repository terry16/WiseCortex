// 可选可填的下拉框（combobox）。
//
// 原生 HTML 给不了这个组合：<select> 不能输入；<input list=datalist> 会拿输入内容去
// **过滤**候选，用户一填就只剩几条甚至一条，等于没有列表可翻。所以自己实现。
//
// 两条设计约束（都踩过坑）：
//  1. 浮层挂到 document.body 并用 position:fixed —— 弹窗 .modal-body 是 overflow-y:auto，
//     绝对定位的浮层会被直接裁掉。
//  2. 列表**永不按输入过滤**，展开就是完整清单。这正是 datalist 不能用的原因。
export interface ComboboxOptions {
  /** 候选项（完整清单，展开时全部可见）。 */
  items: string[];
  /** 初值。 */
  value?: string;
  placeholder?: string;
  /** 值变化时回调（输入或选中都会触发）。 */
  onChange?: (value: string) => void;
}

export interface ComboboxHandle {
  /** 挂载用的根元素。 */
  el: HTMLElement;
  /** 当前值（已 trim）。 */
  value: () => string;
  /** 覆盖当前值（不触发 onChange）。 */
  setValue: (v: string) => void;
  /** 换一批候选项。 */
  setItems: (items: string[]) => void;
  /** 从 DOM 卸载，清理全局监听与浮层。 */
  destroy: () => void;
}

function escapeHtml(s: string): string {
  return s.replace(
    /[&<>"']/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] as string,
  );
}

/** 滚动到可视区。jsdom 没实现 scrollIntoView，做个存在性判断。 */
function scrollTo(el: Element | null): void {
  el?.scrollIntoView?.({ block: "nearest" });
}

export function createCombobox(opts: ComboboxOptions): ComboboxHandle {
  let items = [...opts.items];
  let open = false;
  let active = -1; // 键盘高亮项

  const root = document.createElement("div");
  root.className = "cbx";
  root.innerHTML = `
    <input class="input mono cbx-input" spellcheck="false" autocomplete="off" />
    <button type="button" class="cbx-caret" tabindex="-1" aria-label="展开"></button>`;
  const input = root.querySelector(".cbx-input") as HTMLInputElement;
  const caret = root.querySelector(".cbx-caret") as HTMLButtonElement;
  input.placeholder = opts.placeholder ?? "";
  input.value = opts.value ?? "";

  // 浮层挂 body：避开 .modal-body 的 overflow 裁剪。
  const pop = document.createElement("div");
  pop.className = "cbx-pop";
  pop.hidden = true;
  document.body.appendChild(pop);

  const emit = (): void => opts.onChange?.(input.value.trim());

  function render(): void {
    const cur = input.value.trim();
    pop.innerHTML = items
      .map((m, i) => {
        const cls = ["cbx-opt", m === cur ? "sel" : "", i === active ? "active" : ""]
          .filter(Boolean)
          .join(" ");
        return `<div class="${cls}" data-v="${escapeHtml(m)}">${escapeHtml(m)}</div>`;
      })
      .join("");
  }

  function place(): void {
    const r = input.getBoundingClientRect();
    const room = window.innerHeight - r.bottom;
    const maxH = Math.min(280, Math.max(room - 12, 160));
    pop.style.left = `${r.left}px`;
    pop.style.width = `${root.getBoundingClientRect().width}px`;
    pop.style.maxHeight = `${maxH}px`;
    // 下方不够就翻到上方展开。
    if (room < 180 && r.top > room) {
      pop.style.top = "auto";
      pop.style.bottom = `${window.innerHeight - r.top + 4}px`;
    } else {
      pop.style.bottom = "auto";
      pop.style.top = `${r.bottom + 4}px`;
    }
  }

  function show(): void {
    if (open || items.length === 0) return;
    open = true;
    active = items.indexOf(input.value.trim());
    render();
    pop.hidden = false;
    place();
    // 让当前项进入可视区
    scrollTo(pop.querySelector(".sel"));
  }

  function hide(): void {
    open = false;
    active = -1;
    pop.hidden = true;
  }

  function commit(v: string): void {
    input.value = v;
    hide();
    emit();
  }

  caret.addEventListener("mousedown", (e) => {
    e.preventDefault(); // 别让输入框失焦
    if (open) hide();
    else {
      input.focus();
      show();
    }
  });

  // 聚焦即展开：符合“可选可填”的直觉——点进去就能看到有哪些。
  input.addEventListener("focus", show);
  input.addEventListener("mousedown", () => {
    if (!open) show();
  });
  // 输入时保持展开且**不过滤**，只更新选中态高亮。
  input.addEventListener("input", () => {
    if (!open) show();
    active = -1;
    render();
    emit();
  });

  input.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) {
        show();
        return;
      }
      const d = e.key === "ArrowDown" ? 1 : -1;
      active = (active + d + items.length) % items.length;
      render();
      scrollTo(pop.querySelector(".active"));
    } else if (e.key === "Enter") {
      if (open && active >= 0) {
        e.preventDefault();
        commit(items[active]);
      }
    } else if (e.key === "Escape") {
      if (open) {
        e.stopPropagation(); // 别把外层弹窗一起关了
        hide();
      }
    } else if (e.key === "Tab") {
      hide();
    }
  });

  // mousedown 而非 click：click 会先触发 input 的 blur，浮层已经关了。
  pop.addEventListener("mousedown", (e) => {
    const t = (e.target as HTMLElement).closest(".cbx-opt") as HTMLElement | null;
    if (!t) return;
    e.preventDefault();
    commit(t.dataset.v ?? "");
  });

  const onDocDown = (e: MouseEvent): void => {
    const t = e.target as Node;
    if (!root.contains(t) && !pop.contains(t)) hide();
  };
  const onReflow = (): void => {
    if (open) place();
  };
  document.addEventListener("mousedown", onDocDown);
  window.addEventListener("resize", onReflow);
  // capture：任何祖先容器滚动都要跟着重定位（浮层是 fixed，不随滚动走）。
  window.addEventListener("scroll", onReflow, true);

  return {
    el: root,
    value: () => input.value.trim(),
    setValue: (v: string) => {
      input.value = v;
      if (open) render();
    },
    setItems: (next: string[]) => {
      items = [...next];
      caret.hidden = items.length === 0;
      if (items.length === 0) hide();
      else if (open) render();
    },
    destroy: () => {
      document.removeEventListener("mousedown", onDocDown);
      window.removeEventListener("resize", onReflow);
      window.removeEventListener("scroll", onReflow, true);
      pop.remove();
      root.remove();
    },
  };
}
