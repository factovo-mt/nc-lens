// 狭い幅の配置（スマホ・タブレット。W34・W35）。画面の幅で切り替え。Web 版とデスクトップ版（exe）の共通。
// 配置は html.rl のとき（Web は常に、デスクトップはアプリ設定の『狭い幅の配置：自動』のとき）。html.web は Web 版だけ（タッチ前提の文字の大きさ・ページの拡大の抑止など）。
// 見た目は mobile.css。ここは下部のタブ、描画とパネルの境界、メニューのシート、端末の戻る操作、要素の置き場所の入れ替え、各欄のピンチだけ。
import { isDesktop } from "./platform";
import type { ZoomArea } from "./appsettings";
import { settings, save } from "./settings";

/** スマホの条件（mobile.css のメディアクエリと同じ）。横向きのスマホも含める */
const PHONE = "(max-width: 767px), (max-width: 1023px) and (max-height: 540px)";
/** タブレットの条件（mobile.css と同じ） */
const TABLET = "(min-width: 768px) and (max-width: 1199px) and (min-height: 541px)";
const phoneQuery = matchMedia(PHONE);
const tabletQuery = matchMedia(TABLET);

/** 狭い幅の配置を使うか（デスクトップはアプリ設定。Web は常に） */
let enabled = !isDesktop;
/** スマホの配置のとき true（Web 版・デスクトップ版の共通） */
export const isPhone = () => enabled && phoneQuery.matches;
let relayout = () => {};
/** アプリ設定の『狭い幅の配置』（デスクトップ版）。auto：幅に応じて、normal：常に通常の配置 */
export function setNarrowLayout(mode: "auto" | "normal") {
  enabled = !isDesktop || mode !== "normal";
  relayout();
}

let showPanel: (t: string) => void = () => {};
/** スマホで下部のパネルを NC にする（描画をタップして NC の行へ移るとき） */
export const revealNc = () => { if (isPhone()) showPanel("nc"); };

// ---- 端末の戻る操作：開いたシート（メニュー・浮動ウィンドウ）を新しい順に閉じる ----
const backStack: (() => void)[] = [];
let ignorePop = 0;

/** シートを開いたら呼ぶ（スマホのみ履歴に積む）。返す関数はシートを自分で閉じたときに呼ぶ */
export function pushBack(close: () => void): () => void {
  if (isDesktop || !isPhone()) return () => {}; // 端末の戻る操作は Web 版だけ
  history.pushState({ nclensSheet: true }, "");
  backStack.push(close);
  return () => {
    const i = backStack.indexOf(close);
    if (i < 0) return;
    backStack.splice(i, 1);
    ignorePop++;
    history.back();
  };
}
window.addEventListener("popstate", () => {
  if (ignorePop) return void ignorePop--;
  backStack.pop()?.();
});

export interface MobileHooks {
  zoom(area: ZoomArea, z: number): void;
  zoomOf(area: ZoomArea): number;
  /** 描画を 1 画面へ（4 分割はスマホでは使わない） */
  single(): void;
  /** エディタの再計測（隠していたエディタを出したとき） */
  remeasure(): void;
}

export function initMobile(h: MobileHooks) {
  const html = document.documentElement;
  if (!isDesktop) html.classList.add("web");
  enabled = !isDesktop || settings.narrowLayout !== "normal";
  const $ = (sel: string) => document.querySelector<HTMLElement>(sel)!;

  // ---- 下部のタブ：描画の下のパネルを切り替える。中身は隠すだけなので、再生位置・選択はそのまま ----
  // 展開結果と変数の追跡は同じ下部の欄（#bottom）の別ページ。欄の中のタブで替えたときもこちらを合わせる
  const tabs = $("#mtabs");
  const names = [...tabs.querySelectorAll<HTMLElement>("[data-mtab]")].map((b) => b.dataset.mtab!);
  const mark = (t: string) => {
    html.dataset.mtab = t;
    tabs.querySelectorAll<HTMLElement>("[data-mtab]").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.mtab === t)));
    try { sessionStorage.setItem("nclens.mtab", t); } catch { /* 保存できなくても動く */ }
  };
  const setTab = (t: string) => {
    if (!names.includes(t)) t = "nc";
    mark(t);
    if (t === "nc") requestAnimationFrame(() => h.remeasure());
    if (!isPhone()) return; // 通常の配置・タブレットでは下部のタブ（展開結果・変数の追跡）に触れない
    const sub = document.querySelector<HTMLElement>("#tabs [data-tab].active")?.dataset.tab;
    if (t === "vars" && sub !== "vars") $("#tabs [data-tab=vars]").click();
    if (t === "out" && sub === "vars") $("#tabs [data-tab=expanded]").click();
  };
  showPanel = setTab;
  tabs.addEventListener("click", (e) => {
    const t = (e.target as HTMLElement).closest<HTMLElement>("[data-mtab]")?.dataset.mtab;
    if (t) setTab(t);
  });
  $("#tabs").addEventListener("click", (e) => {
    const sub = (e.target as HTMLElement).closest<HTMLElement>("[data-tab]")?.dataset.tab;
    if (sub && isPhone() && (html.dataset.mtab === "out" || html.dataset.mtab === "vars")) mark(sub === "vars" ? "vars" : "out");
  });
  let first = "nc";
  try { first = sessionStorage.getItem("nclens.mtab") || "nc"; } catch { /* 既定のまま */ }
  setTab(first);

  // ---- 描画とパネルの境界：縦向きは描画の高さ、横向きは幅（画面に対する割合 25〜70%）。ドラッグで変え、設定に保存 ----
  const app = $("#app");
  const land = matchMedia("(orientation: landscape)");
  const grip = Object.assign(document.createElement("div"), { id: "m-grip", title: "ドラッグで描画の大きさを変更" });
  grip.setAttribute("role", "separator");
  const clamp = (v: number, d: number) => (Number.isFinite(v) ? Math.min(0.7, Math.max(0.25, v)) : d);
  const applyView = () => {
    const m = settings.mobile;
    m.viewH = clamp(m.viewH, 0.45);
    m.viewW = clamp(m.viewW, 0.5);
    html.style.setProperty("--mview-h", `${m.viewH * 100}%`);
    html.style.setProperty("--mview-w", `${m.viewW * 100}%`);
    grip.setAttribute("aria-orientation", land.matches ? "vertical" : "horizontal");
  };
  grip.addEventListener("pointerdown", (e) => {
    grip.setPointerCapture(e.pointerId);
    grip.classList.add("drag");
  });
  grip.addEventListener("pointermove", (e) => {
    if (!grip.hasPointerCapture(e.pointerId)) return;
    const v = $("#view-pane").getBoundingClientRect(), a = app.getBoundingClientRect();
    if (land.matches) settings.mobile.viewW = (e.clientX - v.left) / a.width;
    else settings.mobile.viewH = (e.clientY - v.top) / a.height;
    applyView();
  });
  const endDrag = () => { if (grip.classList.contains("drag")) { grip.classList.remove("drag"); save(); } };
  grip.addEventListener("pointerup", endDrag);
  grip.addEventListener("pointercancel", endDrag);
  land.addEventListener("change", applyView);
  applyView();

  // 再生の操作の下の 1 行：現在の行と、再生位置の座標（ステータスバーの欄を移す）
  const pos = Object.assign(document.createElement("div"), { id: "m-pos" });

  // ---- メニューのシート（ツールバーの残りとメニューバー） ----
  const sheet = $("#msheet"), menuBtn = $("#m-menu");
  let unback = () => {};
  const closeSheet = () => {
    if (!sheet.classList.contains("open")) return;
    sheet.classList.remove("open");
    menuBtn.setAttribute("aria-expanded", "false");
    unback();
  };
  menuBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    sheet.classList.add("open");
    menuBtn.setAttribute("aria-expanded", "true");
    unback = pushBack(() => { unback = () => {}; closeSheet(); });
  });
  sheet.querySelector(".ms-back")!.addEventListener("click", closeSheet);
  // 項目を選んだら閉じる（切り替えのボタン・完成形の設定は開いたまま）
  sheet.addEventListener("click", (e) => {
    e.stopPropagation(); // 文書のクリックで閉じるメニューの処理へ渡さない
    if ((e.target as HTMLElement).closest(".ms-body [data-cmd], .ms-body [data-recent]")) closeSheet();
  });

  // ---- 置き場所の入れ替え：スマホのときだけ移し、外れたら元の位置へ戻す（目印のコメントの前へ） ----
  const tools = $("#ms-tools");
  const row = (label: string) => {
    const r = document.createElement("div");
    r.className = "ms-row";
    if (label) r.append(Object.assign(document.createElement("span"), { textContent: label }));
    tools.append(r);
    return r;
  };
  const viewTitle = $("#view-pane > .pane-title");
  const moves: [HTMLElement, () => HTMLElement, Element | null][] = [];
  const plan = (el: HTMLElement | null, to: () => HTMLElement, before: Element | null = null) => { if (el) moves.push([el, to, before]); };
  plan($("#views"), () => viewTitle, viewTitle.querySelector(".spacer"));
  plan($("#toolbar [data-cmd=fit]"), () => viewTitle, viewTitle.querySelector(".spacer"));
  const speedRow = row("再生の速さ");
  plan($("#speed"), () => speedRow);
  // 切り替えは 1 行に 1 つ（名前は mobile.css で aria-label を出す）
  for (const sel of ["#toolbar .tb-pair", "#toolbar [data-switch=optionalStop]", "#toolbar [data-switch=blockSkip]"]) {
    const r = row("");
    plan($(sel), () => r);
  }
  document.querySelectorAll<HTMLElement>("#menubar > .menu").forEach((m) => plan(m, () => $("#ms-menus")));
  plan($("#st-cursor"), () => pos);
  plan($("#st-play"), () => pos);
  const marks = moves.map(([el]) => {
    const c = document.createComment("w34");
    el.before(c);
    return c;
  });

  const apply = () => {
    html.classList.toggle("rl", enabled);
    const phone = isPhone();
    html.dataset.mlayout = phone ? "phone" : enabled && tabletQuery.matches ? "tablet" : "desktop";
    if (phone) { app.append(grip); $("#toolbar").append(pos); } else { grip.remove(); pos.remove(); }
    moves.forEach(([el, to, before], i) => {
      if (phone) to().insertBefore(el, before && before.parentNode === to() ? before : null);
      else marks[i].after(el);
    });
    if (phone) { h.single(); setTab(html.dataset.mtab ?? "nc"); }
    else closeSheet();
  };
  relayout = apply;
  apply();
  phoneQuery.addEventListener("change", apply);
  tabletQuery.addEventListener("change", apply);

  // ---- 各欄の 2 本指のピンチ：文字の大きさ（Ctrl＋ホイールの代わり）。ページ全体の拡大は mobile.css で止める ----
  const pinchAreas: [string, ZoomArea][] = [["#editor", "editor"], ["#tab-body", "expanded"], ["#modal-panel", "modal"], ["#code-peek", "peek"]];
  for (const [sel, area] of pinchAreas) {
    const el = $(sel);
    let d0 = 0, z0 = 1;
    const dist = (t: TouchList) => Math.hypot(t[0].clientX - t[1].clientX, t[0].clientY - t[1].clientY);
    el.addEventListener("touchstart", (e) => {
      if (e.touches.length !== 2) return;
      d0 = dist(e.touches);
      z0 = h.zoomOf(area);
    }, { passive: true });
    el.addEventListener("touchmove", (e) => {
      if (e.touches.length !== 2 || !d0) return;
      e.preventDefault();
      h.zoom(area, z0 * dist(e.touches) / d0);
    }, { passive: false });
    el.addEventListener("touchend", (e) => { if (e.touches.length < 2) d0 = 0; });
  }

  // ---- タブレット：使用コードの欄を右へスワイプで閉じる ----
  const peek = $("#code-peek");
  let sx = 0, sy = 0;
  peek.addEventListener("touchstart", (e) => { if (e.touches.length === 1) [sx, sy] = [e.touches[0].clientX, e.touches[0].clientY]; }, { passive: true });
  peek.addEventListener("touchend", (e) => {
    const t = e.changedTouches[0];
    if (html.dataset.mlayout === "tablet" && t && t.clientX - sx > 80 && Math.abs(t.clientY - sy) < 50 && !peek.hidden) $("#peek-toggle").click();
  });
}
