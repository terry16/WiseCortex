import { t } from "./i18n";
// 通用确认对话框：复用现有 .overlay/.modal 样式，返回 Promise<boolean>（确定=true / 取消=false）。
// Esc / 点遮罩 / 取消 = false；Enter / 确定 = true。用于删除等不可撤销操作前的二次确认。
import { icon } from "./icons";

function esc(s: string): string {
  return s.replace(
    /[&<>"']/g,
    (c) =>
      (
        ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }) as Record<
          string,
          string
        >
      )[c],
  );
}

export interface ConfirmOpts {
  title: string;
  message: string;
  /** 确定按钮文案，默认「删除」。 */
  confirmLabel?: string;
  /** 确定按钮用红色危险样式（删除类操作）。 */
  danger?: boolean;
}

export function confirmDialog(opts: ConfirmOpts): Promise<boolean> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "overlay";
    const okCls = opts.danger ? "btn btn-danger" : "btn btn-primary";
    overlay.innerHTML = `
      <div class="modal" style="width:min(420px,92vw)">
        <div class="modal-head"><span class="ico-tile">${icon("trash", 17)}</span><h3>${esc(opts.title)}</h3></div>
        <div class="modal-body"><p style="margin:0;line-height:1.6;color:var(--text)">${esc(opts.message)}</p></div>
        <div class="modal-foot">
          <button data-cancel class="btn">${esc(t("common.cancel"))}</button>
          <button data-ok class="${okCls}">${esc(opts.confirmLabel ?? t("common.delete"))}</button>
        </div>
      </div>`;
    document.body.appendChild(overlay);

    const done = (v: boolean): void => {
      overlay.remove();
      document.removeEventListener("keydown", onKey);
      resolve(v);
    };
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        e.preventDefault();
        done(false);
      } else if (e.key === "Enter") {
        e.preventDefault();
        done(true);
      }
    };
    document.addEventListener("keydown", onKey);
    overlay.addEventListener("click", (e) => {
      if (e.target === overlay) done(false);
    });
    overlay
      .querySelector<HTMLButtonElement>("[data-cancel]")
      ?.addEventListener("click", () => done(false));
    const ok = overlay.querySelector<HTMLButtonElement>("[data-ok]");
    ok?.addEventListener("click", () => done(true));
    ok?.focus();
  });
}
