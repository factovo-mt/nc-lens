// 浮動ウィンドウ：<dialog> をモーダルにせず開き、見出しのドラッグで移動、辺と角で大きさを変える。
// 見出しの右に 最小化（折りたたみ）・最大化／元に戻す・×。位置と大きさはウィンドウごとに設定ファイルへ残す。
import { save, settings, type WinRect } from "./settings";
import { isPhone, pushBack } from "./mobile";

const MIN_W = 360, MIN_H = 220, MARGIN = 4;
/** 上はメニューを隠さない */
const top0 = () => (document.querySelector<HTMLElement>("#menubar")?.getBoundingClientRect().bottom ?? 0);
const EDGES = ["n", "s", "e", "w", "ne", "nw", "se", "sw"] as const;
// Windows のキャプションの記号（10px・線 1px）に合わせる
const ICON = {
  min: '<svg viewBox="0 0 10 10"><path d="M0 5.5h10"/></svg>',
  max: '<svg viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9"/></svg>',
  restore: '<svg viewBox="0 0 10 10"><rect x="0.5" y="2.5" width="7" height="7"/><path d="M2.5 2.5v-2h7v7h-2"/></svg>',
  close: '<svg viewBox="0 0 10 10"><path d="M0 0l10 10M10 0L0 10"/></svg>',
};

let zTop = 100;
const wins: FloatWin[] = [];

export class FloatWin {
  private rect: WinRect;
  private collapsed = false;
  /** 既定の大きさをまだ測っていない（スマホの配置の全画面のシートで開いたとき。広い幅になったら測る） */
  private unsized = false;
  private maxBtn: HTMLButtonElement;
  /** スマホ：端末の戻る操作の登録を外す（全画面のシートとして開いたとき） */
  private unback = () => {};

  /** onClose：× と Esc のとき。false を返すと閉じない（未保存の確認など）。無ければそのまま閉じる */
  constructor(readonly dlg: HTMLDialogElement, private key: string, private onClose?: () => boolean | Promise<boolean>) {
    dlg.classList.add("fw");
    this.rect = { x: 0, y: 0, w: 0, h: 0, max: false };
    const head = dlg.querySelector("header")!;
    head.classList.add("fw-head");
    head.title = "ドラッグで移動。ダブルクリックで最大化／元に戻す";
    const btns = document.createElement("span");
    btns.className = "fw-btns";
    btns.innerHTML =
      `<button tabindex="-1" data-fw="min" title="最小化（折りたたみ）" aria-label="最小化">${ICON.min}</button>` +
      `<button tabindex="-1" data-fw="max" title="最大化" aria-label="最大化">${ICON.max}</button>` +
      `<button tabindex="-1" data-fw="close" class="fw-close" title="閉じる（Esc）" aria-label="閉じる">${ICON.close}</button>`;
    head.append(btns);
    this.maxBtn = btns.querySelector("[data-fw=max]")!;
    btns.addEventListener("click", (e) => {
      const k = (e.target as HTMLElement).closest<HTMLElement>("[data-fw]")?.dataset.fw;
      if (k === "min") this.setCollapsed(!this.collapsed);
      else if (k === "max") this.setMax(!this.rect.max);
      else if (k === "close") void this.requestClose();
    });
    for (const k of EDGES) {
      const h = document.createElement("div");
      h.className = `fw-rz fw-${k}`;
      dlg.append(h);
      this.drag(h, k);
    }
    this.drag(head, "move");
    head.addEventListener("dblclick", (e) => {
      if (isPhone()) return;
      if (!(e.target as HTMLElement).closest("button, input, select, a")) this.setMax(!this.rect.max);
    });
    dlg.addEventListener("pointerdown", () => this.front(), true);
    dlg.addEventListener("focusin", () => this.front());
    dlg.addEventListener("keydown", (e) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      e.preventDefault();
      e.stopPropagation();
      void this.requestClose();
    });
    // モーダルのときの Esc（cancel）は使わない
    dlg.addEventListener("cancel", (e) => e.preventDefault());
    wins.push(this);
  }

  get isOpen() { return this.dlg.open; }

  /** 開く（開いていれば前へ出す）。前回の位置と大きさで開く */
  open() {
    if (!this.dlg.open) {
      const saved = settings.windows[this.key];
      this.dlg.show();
      // show() は最初のボタン（最小化）にフォーカスを置く。枠が出ないよう窓そのものへ
      if (document.activeElement?.closest(".fw-btns")) {
        this.dlg.tabIndex = -1;
        this.dlg.focus({ preventScroll: true });
      }
      this.setCollapsed(false, false);
      if (saved && [saved.x, saved.y, saved.w, saved.h].every(Number.isFinite) && saved.w > 0) {
        this.rect = { ...saved, max: !!saved.max };
      } else {
        // 既定：CSS の大きさで中央
        this.dlg.style.cssText = "";
        this.unsized = true;
        this.measure();
      }
      this.place();
      this.unback = pushBack(() => void this.requestClose());
    }
    this.front();
  }

  close() {
    this.dlg.close();
    this.unback();
    this.unback = () => {};
  }

  async requestClose() {
    if (this.onClose && !(await this.onClose())) return;
    if (this.dlg.open) this.close();
  }

  private front() {
    if (this.dlg.style.zIndex === String(zTop) && this.dlg.classList.contains("fw-front")) return;
    this.dlg.style.zIndex = String(++zTop);
    for (const w of wins) w.dlg.classList.toggle("fw-front", w === this);
  }

  private setCollapsed(on: boolean, apply = true) {
    this.collapsed = on;
    this.dlg.classList.toggle("fw-collapsed", on);
    const b = this.dlg.querySelector<HTMLElement>("[data-fw=min]")!;
    b.title = on ? "元の大きさに戻す" : "最小化（折りたたみ）";
    if (apply) this.place();
  }

  private setMax(on: boolean) {
    this.rect.max = on;
    if (on) this.setCollapsed(false, false);
    this.place();
    this.store();
  }

  /** 画面内に収めて反映する。commit のときは収めた値を覚える（窓を縮めただけでは覚えない） */
  /** 既定の大きさ（CSS）で中央。スマホの配置では測れないので後回し */
  private measure() {
    if (isPhone()) return;
    this.unsized = false;
    Object.assign(this.dlg.style, { left: "", top: "", width: "", height: "" });
    const r = this.dlg.getBoundingClientRect();
    const t = top0();
    this.rect = { x: (innerWidth - r.width) / 2, y: t + Math.max(0, (innerHeight - t - r.height) / 2), w: r.width, h: r.height, max: false };
  }

  place(commit = false) {
    if (this.unsized) {
      this.measure();
      if (this.unsized) return;
    }
    const t = top0();
    const s = this.dlg.style;
    this.maxBtn.innerHTML = this.rect.max ? ICON.restore : ICON.max;
    this.maxBtn.title = this.rect.max ? "元の大きさに戻す" : "最大化";
    this.dlg.classList.toggle("fw-max", this.rect.max);
    if (this.rect.max) {
      Object.assign(s, { left: `${MARGIN}px`, top: `${t + MARGIN}px`, width: `${innerWidth - MARGIN * 2}px`, height: `${innerHeight - t - MARGIN * 2}px` });
      return;
    }
    const r = commit ? this.rect : { ...this.rect };
    r.w = Math.max(Math.min(MIN_W, innerWidth - MARGIN * 2), Math.min(r.w, innerWidth - MARGIN * 2));
    r.h = Math.max(Math.min(MIN_H, innerHeight - t - MARGIN * 2), Math.min(r.h, innerHeight - t - MARGIN * 2));
    const hh = this.collapsed ? this.dlg.querySelector("header")!.getBoundingClientRect().height + 2 : r.h;
    r.x = Math.max(MARGIN, Math.min(r.x, innerWidth - r.w - MARGIN));
    r.y = Math.max(t + MARGIN, Math.min(r.y, innerHeight - hh - MARGIN));
    Object.assign(s, { left: `${r.x}px`, top: `${r.y}px`, width: `${r.w}px`, height: this.collapsed ? "auto" : `${r.h}px` });
  }

  private store() {
    settings.windows[this.key] = { ...this.rect };
    save();
  }

  /** 見出し（move）と辺・角のドラッグ */
  private drag(el: HTMLElement, kind: (typeof EDGES)[number] | "move") {
    el.addEventListener("pointerdown", (e) => {
      if (e.button !== 0 || isPhone()) return; // スマホは全画面のシート（動かさない）
      if (kind === "move" && (e.target as HTMLElement).closest("button, input, select, textarea, a")) return;
      if (this.rect.max) {
        if (kind !== "move") return;
        // 最大化のまま見出しをつかんだら、元の大きさに戻してカーソルの下へ
        const ratio = (e.clientX - MARGIN) / Math.max(1, innerWidth - MARGIN * 2);
        this.rect.max = false;
        this.rect.x = e.clientX - this.rect.w * ratio;
        this.rect.y = e.clientY - 12;
        this.place(true);
      }
      if (this.collapsed && kind !== "move" && kind !== "e" && kind !== "w") return;
      e.preventDefault();
      el.setPointerCapture(e.pointerId);
      this.place(true); // 窓に合わせて縮んだ見た目の値から動かす
      const sx = e.clientX, sy = e.clientY, o = { ...this.rect };
      document.documentElement.classList.add("fw-dragging");
      el.onpointermove = (m) => {
        const dx = m.clientX - sx, dy = m.clientY - sy, r = this.rect;
        if (kind === "move") {
          r.x = o.x + dx;
          r.y = o.y + dy;
        } else {
          if (kind.includes("e")) r.w = Math.max(MIN_W, o.w + dx);
          if (kind.includes("s")) r.h = Math.max(MIN_H, o.h + dy);
          if (kind.includes("w")) { r.w = Math.max(MIN_W, o.w - dx); r.x = o.x + o.w - r.w; }
          if (kind.includes("n")) { r.h = Math.max(MIN_H, o.h - dy); r.y = o.y + o.h - r.h; }
        }
        this.place(true);
      };
      el.onlostpointercapture = () => {
        el.onpointermove = null;
        el.onlostpointercapture = null;
        document.documentElement.classList.remove("fw-dragging");
        this.store();
      };
    });
  }
}

// アプリの窓が小さくなったら、開いているウィンドウを画面内へ戻す
window.addEventListener("resize", () => {
  for (const w of wins) if (w.isOpen) w.place();
});
