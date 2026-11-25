// アプリ設定の画面：左にページの一覧（外観・文字の大きさ・表示・動作・ファイル）、右にページの中身。
// モーダルではないダイアログ（開いたまま本体を操作できる）。変更はすぐ反映して設定ファイルへ保存（main.ts のフック）。
import { settings, MODAL_ITEMS, type ModalItem, type ThemeMode } from "./settings";
import { ITEM_LABEL } from "./modal";
import { FloatWin } from "./floatwin";
import type { BlockSkipUsage } from "./types";
import { isDesktop } from "./platform";

export type ZoomArea = "editor" | "expanded" | "modal" | "peek";
const $ = <T extends HTMLElement = HTMLElement>(sel: string) => document.querySelector<T>(sel)!;
const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

/** 画面への反映と保存（main.ts が渡す） */
export interface AppHooks {
  theme(mode: ThemeMode): void;
  /** 狭い幅の配置（デスクトップ版） */
  narrow(mode: "auto" | "normal"): void;
  zoom(area: ZoomArea, z: number): void;
  font(v: string): void;
  layout(l: "single" | "quad"): void;
  modal(patch: { visible?: boolean; items?: ModalItem[] }): void;
  speed(v: number): void;
  /** 完成形の薄表示・M01 で止める・ブロックスキップ（null はポストの既定） */
  switches(patch: { ghost?: boolean; optionalStop?: boolean; blockSkip?: boolean | null; blockSkipLevels?: number[] | null }): void;
  peek(patch: { open?: boolean; width?: number }): void;
  /** 今のポストのブロックスキップの既定 */
  postSkip(): boolean;
  /** 今のセットのスキップの番号（expand.block_skip_levels） */
  postLevels(): number[];
  /** 直前の実行で NC に出たスキップの番号と行数 */
  skipUsage(): BlockSkipUsage[];
  clearRecent(kind: "files" | "posts"): void;
  splash(on: boolean): void;
  /** 開くダイアログの最初のフォルダ：表示用の文言、変更（フォルダを選ぶ）、初期化（サンプルのフォルダ） */
  dirLabel(): string;
  pickDir(): Promise<void>;
  resetDir(): void;
  reveal(): void;
  reset(): void;
  place(): string;
}

const PAGES: [string, string][] = [["look", "外観"], ["text", "文字の大きさ"], ["view", "表示"], ["behavior", "動作"], ["files", "ファイル"]];
/** 等幅の字体の候補（空は既定） */
const FONTS = ["", "Cascadia Mono", "BIZ UDGothic", "Consolas", "MS Gothic", "Source Han Code JP", "Courier New"];
const radio = (name: string, v: string, cur: string, label: string) =>
  `<label class="pc"><input type="radio" name="${name}" value="${v}"${v === cur ? " checked" : ""} />${label}</label>`;
const check = (key: string, on: boolean, label: string) => `<label class="pc"><input type="checkbox" data-a="${key}"${on ? " checked" : ""} />${label}</label>`;

/** スキップの番号の選択肢：NC に出る番号は強調して行数、出ない番号は薄く */
function levelChecks(h: AppHooks) {
  const picked = settings.blockSkipLevels ?? h.postLevels();
  const used = new Map(h.skipUsage().map((u) => [u.level, u.line_count]));
  return Array.from({ length: 9 }, (_, i) => {
    const n = i + 1, count = used.get(n);
    const tip = count === undefined ? "NC に無い番号" : `NC の ${count} 行`;
    return `<label class="pc skip-lv${count === undefined ? " unused" : " used"}" title="${tip}"><input type="checkbox" data-level="${n}"${picked.includes(n) ? " checked" : ""} />/${n}${count === undefined ? "" : `<em>${count} 行</em>`}</label>`;
  }).join("");
}

function render(page: string, h: AppHooks) {
  const s = settings;
  switch (page) {
    case "look":
      return `<section class="psec"><h4>テーマ</h4><div class="pchecks col">
          ${radio("a-theme", "auto", s.theme, "自動（OS の設定）")}${radio("a-theme", "light", s.theme, "ライト")}${radio("a-theme", "dark", s.theme, "ダーク")}
        </div></section>
        <section class="psec"><h4>起動画面<small>次回の起動から反映</small></h4><div class="pchecks col">
          ${check("splash", s.splash, "起動画面（ロゴと読み込み状況）の表示")}
        </div></section>
        <section class="psec"><h4>色<small>座標軸・工具の色（今のテーマ）</small></h4><div class="aswatch">
          ${["--axis-x", "--axis-y", "--axis-z", "--axis-a", "--axis-b", "--axis-c"].map((v) => `<span><i style="background:var(${v})"></i>${v.slice(-1).toUpperCase()}</span>`).join("")}
          ${Array.from({ length: 7 }, (_, i) => `<span><i style="background:var(--tool-${i})"></i>工具 ${i + 1}</span>`).join("")}
        </div></section>`;
    case "text": {
      const z = (area: ZoomArea, label: string) =>
        `<label class="pf"><span>${label}</span><input type="range" min="50" max="400" step="10" data-zoom="${area}" value="${Math.round(s.zoom[area] * 100)}" /><em>${Math.round(s.zoom[area] * 100)}%</em></label>`;
      return `<section class="psec"><h4>倍率<small>Ctrl＋ホイール：各欄の倍率。Ctrl+0：既定</small></h4><div class="pgrid acol">
          ${z("editor", "エディタ")}${z("expanded", "展開結果・下部の表")}${z("modal", "モーダルパネル")}${z("peek", "使用コード一覧")}
        </div></section>
        <section class="psec"><h4>等幅の字体<small>エディタ・展開結果・モーダルパネル・座標の数字</small></h4><div class="pgrid acol">
          <label class="pf"><span>字体</span><select data-a="fontPick">${[...new Set([...FONTS, s.fontMono])].map((f) => `<option value="${esc(f)}"${f === s.fontMono ? " selected" : ""}>${f ? esc(f) : "既定（Cascadia Mono ほか）"}</option>`).join("")}</select></label>
          <label class="pf"><span>直接入力</span><input data-a="font" value="${esc(s.fontMono)}" spellcheck="false" placeholder="例：&quot;Cascadia Mono&quot;, monospace" /></label>
        </div><p class="pnote afont" style="font-family:var(--font-mono)">G01 X12.345 Y-6.789 F1000（0O 1lI 確認用）</p></section>`;
    }
    case "view":
      return `<section class="psec"><h4>描画の分け方<small>ツールバーでも切り替え可。4 分割の境界：ドラッグで大きさ</small></h4><div class="pchecks col">
          ${radio("a-layout", "single", s.view?.layout === "quad" ? "quad" : "single", "1 画面")}${radio("a-layout", "quad", s.view?.layout === "quad" ? "quad" : "single", "4 分割（XY・3D・XZ・YZ）")}
        </div></section>${isDesktop ? `
        <section class="psec"><h4>狭い幅の配置<small>自動：ウィンドウの幅が 1200px 未満でタブレット、768px 未満でスマートフォンの配置</small></h4><div class="pchecks col">
          ${radio("a-narrow", "auto", s.narrowLayout, "自動（既定）")}${radio("a-narrow", "normal", s.narrowLayout, "常に通常の配置")}
        </div></section>` : ""}
        <section class="psec"><h4>モーダルパネル</h4><div class="pchecks col">
          ${check("modalVisible", s.modal.visible, "モーダルパネルの表示")}
        </div><div class="pchecks">
          ${MODAL_ITEMS.map((k) => `<label class="pc"><input type="checkbox" data-item="${k}"${s.modal.items.includes(k) ? " checked" : ""} />${ITEM_LABEL[k]}</label>`).join("")}
        </div></section>
        <section class="psec" data-slot="w15-ghost"><h4>完成形<small>ツールバーの完成形のアイコン・▾ でも設定可</small></h4><div class="pchecks col">
          ${check("ghost", s.ghost, "完成形の表示（未再生の経路）")}
        </div></section>
        <section class="psec" data-slot="w15-peek"><h4>使用コード一覧<small>描画の見出しのアイコン：開閉。左端のドラッグ：幅</small></h4><div class="pchecks col">
          ${check("peekOpen", s.peek.open, "使用コード一覧の表示")}
        </div><div class="pgrid acol">
          <label class="pf"><span>幅</span><input type="range" min="164" max="800" step="1" data-a="peekWidth" value="${s.peek.width}" /><em>${s.peek.width} px</em></label>
        </div></section>`;
    case "behavior":
      return `<section class="psec"><h4>再生</h4><div class="pgrid acol">
          <label class="pf"><span>再生速度（既定）</span><select data-a="speed">${$("#speed").innerHTML}</select></label>
        </div></section>
        <section class="psec" data-slot="w15-stop"><h4>オプショナルストップ・ブロックスキップ<small>ツールバーのアイコン・描画の右クリックでも切り替え可</small></h4><div class="pchecks col">
          ${check("optionalStop", s.optionalStop, "ON：M01 で停止（オプショナルストップ）")}
        </div><div class="pchecks col">
          ${radio("a-skip", "post", s.blockSkip === null ? "post" : s.blockSkip ? "on" : "off", `/ のブロック：ポストの既定（今のポスト：${h.postSkip() ? "スキップ" : "実行"}）`)}
          ${radio("a-skip", "on", s.blockSkip === null ? "post" : s.blockSkip ? "on" : "off", "/ のブロック：スキップ")}
          ${radio("a-skip", "off", s.blockSkip === null ? "post" : s.blockSkip ? "on" : "off", "/ のブロック：実行")}
        </div><div class="pchecks">
          <span class="pc">スキップの番号</span>${levelChecks(h)}
        </div><div class="pchecks"><button class="pbtn" data-aact="skip-post"${s.blockSkipLevels === null ? " disabled" : ""}>セットの既定へ</button></div>
        <p class="pnote">番号の優先：アプリ設定の選択、未選択の間はセットの既定（${h.postLevels().map((n) => `/${n}`).join(" ") || "なし"}）。/ ＝ /1</p></section>
        <p class="pnote">展開出力への反映：ポスト・コード定義の「展開出力」（ポストごと）</p>`;
    case "files": {
      const rec = (title: string, kind: "files" | "posts", list: string[]) =>
        `<section class="psec"><h4>${title}<small>${list.length} 件（最大 10 件）</small></h4>
          <ol class="arecent">${list.map((p) => `<li title="${esc(p)}">${esc(p)}</li>`).join("") || "<li class=\"none\">なし</li>"}</ol>
          <button class="pbtn" data-clear="${kind}"${list.length ? "" : " disabled"}>一覧の消去</button></section>`;
      return `<section class="psec"><h4>既定のフォルダ<small>開くダイアログの最初のフォルダ</small></h4><p class="pnote aplace">${esc(h.dirLabel())}</p>
          <div class="pchecks"><button class="pbtn" data-aact="pick-dir">変更…</button><button class="pbtn" data-aact="reset-dir"${s.defaultDir ? "" : " disabled"}>初期化（同梱のサンプル）</button></div></section>
        ${rec("最近使ったファイル", "files", s.recent.files)}${rec("最近使ったポスト", "posts", s.recent.posts)}
        <section class="psec"><h4>設定ファイル</h4><p class="pnote aplace">${esc(h.place())}</p>
          <div class="pchecks"><button class="pbtn" data-aact="reveal">設定ファイルの場所</button><button class="pbtn" data-aact="reset">表示の状態の初期化…</button></div></section>`;
    }
  }
  return "";
}

let page = "look";
let hooks: AppHooks | null = null;
let win: FloatWin | null = null;

/** 開いていれば今のページを描き直す（メニューやツールバーで設定を変えたとき） */
export function refreshAppSettings() {
  const dlg = $<HTMLDialogElement>("#app-settings");
  if (dlg.open && hooks) show(page);
}

function show(p: string) {
  page = p;
  document.querySelectorAll<HTMLElement>("#app-pages [data-apage]").forEach((b) => b.classList.toggle("active", b.dataset.apage === p));
  $("#app-body").innerHTML = render(p, hooks!);
  const speed = document.querySelector<HTMLSelectElement>('#app-body [data-a="speed"]');
  if (speed) speed.value = String(settings.playSpeed);
}

/** アプリ設定を開く（モーダルではない）。開いていれば前へ出すだけ。 */
export function openAppSettings(h: AppHooks, start?: string) {
  const dlg = $<HTMLDialogElement>("#app-settings");
  hooks = h;
  if (!dlg.dataset.bound) {
    dlg.dataset.bound = "1";
    win = new FloatWin(dlg, "appSettings");
    $("#app-pages").innerHTML = PAGES.map(([k, t]) => `<button data-apage="${k}">${t}</button>`).join("");
    dlg.addEventListener("click", (e) => {
      const t = e.target as HTMLElement;
      const pg = t.closest<HTMLElement>("[data-apage]")?.dataset.apage;
      if (pg) return show(pg);
      const act = t.closest<HTMLElement>("[data-aact]")?.dataset.aact;
      if (act === "reveal") return hooks!.reveal();
      if (act === "reset") return hooks!.reset();
      if (act === "pick-dir") return void hooks!.pickDir().then(() => show(page));
      if (act === "reset-dir") { hooks!.resetDir(); return show(page); }
      if (act === "skip-post") { hooks!.switches({ blockSkipLevels: null }); return show(page); }
      const clear = t.closest<HTMLElement>("[data-clear]")?.dataset.clear as "files" | "posts" | undefined;
      if (clear) { hooks!.clearRecent(clear); show(page); }
    });
    const onChange = (e: Event) => {
      const el = e.target as HTMLInputElement;
      const hk = hooks!;
      if (el.name === "a-theme") hk.theme(el.value as ThemeMode);
      else if (el.name === "a-layout") hk.layout(el.value as "single" | "quad");
      else if (el.name === "a-narrow") hk.narrow(el.value as "auto" | "normal");
      else if (el.dataset.zoom) {
        hk.zoom(el.dataset.zoom as ZoomArea, Number(el.value) / 100);
        el.nextElementSibling!.textContent = `${el.value}%`;
        return;
      } else if (el.dataset.item) {
        const on = new Set([...dlg.querySelectorAll<HTMLInputElement>("[data-item]:checked")].map((x) => x.dataset.item as ModalItem));
        hk.modal({ items: MODAL_ITEMS.filter((k) => on.has(k)) });
        return;
      } else if (el.dataset.a === "modalVisible") hk.modal({ visible: el.checked });
      else if (el.dataset.a === "speed") hk.speed(Number(el.value));
      else if (el.dataset.a === "splash") hk.splash(el.checked);
      else if (el.dataset.a === "ghost" || el.dataset.a === "optionalStop") hk.switches({ [el.dataset.a]: el.checked });
      else if (el.name === "a-skip") hk.switches({ blockSkip: el.value === "post" ? null : el.value === "on" });
      else if (el.dataset.level) {
        const lv = [...document.querySelectorAll<HTMLInputElement>("#app-body [data-level]")].filter((c) => c.checked).map((c) => Number(c.dataset.level));
        hk.switches({ blockSkipLevels: lv.length ? lv : [1] });
      }
      else if (el.dataset.a === "peekOpen") hk.peek({ open: el.checked });
      else if (el.dataset.a === "peekWidth") {
        hk.peek({ width: Number(el.value) });
        el.nextElementSibling!.textContent = `${el.value} px`;
        return;
      }
      else if (el.dataset.a === "fontPick" || el.dataset.a === "font") {
        hk.font(el.value.trim());
        if (e.type === "input") return; // 入力中は描き直さない
      } else return;
      show(page);
    };
    dlg.addEventListener("change", onChange);
    dlg.addEventListener("input", (e) => {
      const el = e.target as HTMLInputElement;
      if (el.dataset.zoom || el.dataset.a === "font" || el.dataset.a === "peekWidth") onChange(e);
    });
  }
  show(start ?? page);
  win!.open();
}
