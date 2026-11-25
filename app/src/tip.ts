// 共通のツールチップ：コードの説明（エディタ・展開結果）、使用コードのアイコン、ボタン類（title 属性）。
// 見出し行（コード＋機能名）・説明・区切り線・補足。折り返さず、長い行は … で省略。Ctrl を押す間は全文（スクロール）。
// 表示は 300ms 後、消えるときはフェード。画面の端では位置をずらし、矢印は対象を指す。
import type { CodeHelp } from "./codehelp";

export type TipContent = string | CodeHelp;
type Side = "above" | "below" | "right";

const DELAY = 300;
const tip = Object.assign(document.createElement("div"), { id: "app-tip", role: "tooltip" });
tip.hidden = true;
document.body.append(tip);
let timer = 0, fade = 0, anchor: Element | null = null;

/** ツールチップの中身。CodeHelp は見出し行（コード＋説明）と補足 */
export function tipDom(c: TipContent): HTMLElement {
  const el = document.createElement("div");
  el.className = "tip-body";
  const line = (cls: string, ...parts: [string, string][]) => {
    const d = document.createElement("div");
    d.className = cls;
    for (const [k, t] of parts) d.append(Object.assign(document.createElement("span"), { className: k, textContent: t }));
    el.append(d);
  };
  if (typeof c === "string") {
    // 「説明（キー）」はキーを補足の色に
    const m = /^(.*?)(（[^（）]*）)$/.exec(c);
    line("tip-text", ...(m && m[1] ? ([["", m[1]], ["tip-key", m[2]]] as [string, string][]) : ([["", c]] as [string, string][])));
  } else {
    line("tip-head", ["tip-code", c.head], ["tip-name", c.body]);
    if (c.note) {
      el.append(Object.assign(document.createElement("hr"), { className: "tip-sep" }));
      line("tip-note", ["", c.note]);
    }
  }
  return el;
}

function place(r: DOMRect, side: Side) {
  const m = 6, gap = 8, w = tip.offsetWidth, h = tip.offsetHeight;
  let s = side, x: number, y: number;
  if (s === "right" && r.right + gap + w > innerWidth - m) s = "above";
  if (s === "right") {
    x = r.right + gap;
    y = Math.max(m, Math.min(r.top + r.height / 2 - h / 2, innerHeight - h - m));
    tip.style.setProperty("--arrow", `${r.top + r.height / 2 - y}px`);
  } else {
    if (s === "above" && r.top - gap - h < m) s = "below";
    else if (s === "below" && r.bottom + gap + h > innerHeight - m && r.top - gap - h >= m) s = "above";
    x = Math.max(m, Math.min(r.left + r.width / 2 - w / 2, innerWidth - w - m));
    y = s === "above" ? r.top - gap - h : r.bottom + gap;
    tip.style.setProperty("--arrow", `${Math.max(10, Math.min(r.left + r.width / 2 - x, w - 10))}px`);
  }
  tip.dataset.side = s;
  tip.style.left = `${Math.round(x)}px`;
  tip.style.top = `${Math.round(y)}px`;
}

/** 対象の要素に、少し遅らせて出す（delay 0 ですぐ） */
export function showTip(target: Element, content: TipContent, side: Side = "above", delay = DELAY) {
  clearTimeout(timer);
  anchor = target;
  timer = window.setTimeout(() => {
    if (anchor !== target || !target.isConnected) return;
    clearTimeout(fade);
    tip.replaceChildren(tipDom(content));
    tip.classList.remove("show");
    tip.hidden = false;
    place(target.getBoundingClientRect(), side);
    requestAnimationFrame(() => tip.classList.add("show"));
  }, delay);
}

export function hideTip(target?: Element) {
  if (target && target !== anchor) return;
  clearTimeout(timer);
  anchor = null;
  tip.classList.remove("show");
  clearTimeout(fade);
  fade = window.setTimeout(() => (tip.hidden = true), 120);
}

// Ctrl を押す間は全文（幅を広げ、はみ出しはスクロール）
const full = (on: boolean) => {
  document.documentElement.classList.toggle("tip-full", on);
  if (!tip.hidden && anchor) place(anchor.getBoundingClientRect(), (tip.dataset.side as Side) ?? "above");
};
addEventListener("keydown", (e) => e.key === "Control" && full(true));
addEventListener("keyup", (e) => e.key === "Control" && full(false));
addEventListener("blur", () => { full(false); hideTip(); });
addEventListener("scroll", () => hideTip(), true);
addEventListener("pointerdown", (e) => { if (!tip.contains(e.target as Node)) hideTip(); }, true);

// ボタン類の title：ホバー中は title を外して（OS のツールチップを出さず）共通のツールチップで出す
let titled: HTMLElement | null = null, saved = "";
const restore = () => {
  if (titled && !titled.hasAttribute("title")) titled.title = saved;
  titled = null;
};
document.addEventListener("pointerover", (e) => {
  if (e.pointerType === "touch") return;
  const el = (e.target as HTMLElement).closest<HTMLElement>("[title]");
  if (!el || el === titled || el.closest(".cm-editor") || !el.title) return;
  restore();
  titled = el;
  saved = el.title;
  el.removeAttribute("title");
  showTip(el, saved, el.closest("#menubar, #toolbar, .pane-title, #tabs") ? "below" : "above");
});
document.addEventListener("pointerout", (e) => {
  if (!titled || titled.contains(e.relatedTarget as Node)) return;
  hideTip(titled);
  restore();
});
