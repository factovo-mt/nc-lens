// モーダルパネル：エディタの上に、選んだ行（カーソル行・再生位置・展開結果の行）の実行後のモーダル状態と、
// プログラム全体の範囲を出す。値が変わった欄は背景色を 0.6 秒変えて戻す。
import type { ExpandedLine, ModalSnapshot, CoordinateBounds } from "./types";
import { MODAL_ITEMS, type ModalItem } from "./settings";

const AXES = ["X", "Y", "Z", "A", "B", "C"];
export const ITEM_LABEL: Record<ModalItem, string> = {
  g: "G のモーダル群", f: "F 送り", s: "S 回転数", t: "T 工具", h: "H 長さ補正", d: "D 径補正",
  work: "ワーク座標", plane: "平面", abs: "絶対／相対", pos: "現在位置 X Y Z A B C", range: "プログラム全体の範囲",
};
/** 値が変わった欄の背景色を戻すまでの時間（style.css の mp-flash と同じ） */
const FLASH_MS = 600;
const PLANE: Record<string, string> = { "17": "XY", "18": "ZX", "19": "YZ" };

const num = (v: number, places = 3) => (Number.isFinite(v) ? v.toFixed(places).replace(/\.?0+$/, "") || "0" : "—");
/** 1 → G01、54.1 → G54.1 */
const gWord = (g: number) => {
  const [i, f] = String(g).split(".");
  return `G${i.padStart(2, "0")}${f ? `.${f}` : ""}`;
};
const axisClass = (a: string) => `ax-${a.toLowerCase()}`;

/**
 * 1 つの欄。w：値の欄の幅（ch）、span：欄の幅（単位幅の個数）。どちらも項目の種類ごとに固定で、
 * 値の長さで欄の幅・位置が変わらない。単位幅は style.css の --mp-unit。全欄が単位幅の倍数なので、折り返しても縦の線がそろう。
 * first：まとまりの先頭（1 列目から始める）。
 */
interface Cell { key: string; label: string; value: string; w: number; span: number; first?: boolean; title?: string; cls?: string }

type Kind = "g" | "f" | "s" | "t" | "h" | "d" | "work" | "plane" | "abs" | "pos" | "range";
/** 値の欄の幅（ch）。G43.1・G54.1、-99999.999 など、その種類で最長の値が収まる幅 */
const W: Record<Kind, number> = { g: 5, f: 9, s: 7, t: 4, h: 3, d: 3, work: 6, plane: 6, abs: 8, pos: 10, range: 24 };
/** 欄の幅（単位幅の個数）。見出し＋値の欄（W）が収まる最小の個数 */
const SPAN: Record<Kind, number> = { g: 2, f: 3, s: 3, t: 2, h: 2, d: 2, work: 3, plane: 3, abs: 3, pos: 3, range: 7 };

/**
 * 割り付けは固定：G のモーダル群 → F S T H D ワーク 平面 指令 → XYZABC → 範囲。各まとまりは 1 列目から始め、
 * 幅が足りなければ欄の単位で次の行へ回る。m が無い（行を選んでいない）ときも枠だけ出す。
 * G の群は、プログラム全体で出てくる群すべてを番号順に並べる（値が無い群は空の枠）。表示しない項目は詰める。
 */
function cells(m: ModalSnapshot | undefined, items: Set<ModalItem>, groups: string[], bounds: CoordinateBounds | null): Cell[][] {
  const v = (f: (m: ModalSnapshot) => string) => (m ? f(m) : "");
  const c = (kind: Kind, x: Omit<Cell, "w" | "span">): Cell => ({ ...x, w: W[kind], span: SPAN[kind] });
  const rows: Cell[][] = [];
  if (items.has("g")) {
    rows.push(groups.map((grp) => {
      const g = m?.g_codes[grp];
      return c("g", { key: `g${grp}`, label: grp, value: g === undefined ? "" : gWord(g), title: `モーダル群 ${grp}` });
    }));
  }
  const r: Cell[] = [];
  if (items.has("f")) r.push(c("f", { key: "f", label: "F", value: v((m) => num(m.feed)) }));
  if (items.has("s")) r.push(c("s", { key: "s", label: "S", value: v((m) => num(m.spindle)) }));
  if (items.has("t")) r.push(c("t", { key: "t", label: "T", value: v((m) => String(m.tool)) }));
  if (items.has("h")) r.push(c("h", { key: "h", label: "H", value: v((m) => String(m.h)) }));
  if (items.has("d")) r.push(c("d", { key: "d", label: "D", value: v((m) => String(m.d)) }));
  if (items.has("work")) r.push(c("work", { key: "work", label: "ワーク", value: v((m) => gWord(m.work_coordinate)) }));
  if (items.has("plane")) r.push(c("plane", { key: "plane", label: "平面", value: v((m) => `${gWord(m.plane)} ${PLANE[String(m.plane)] ?? ""}`.trim()), cls: "txt" }));
  if (items.has("abs")) r.push(c("abs", { key: "abs", label: "指令", value: v((m) => (m.absolute ? "G90 絶対" : "G91 相対")), cls: "txt" }));
  rows.push(r);
  if (items.has("pos")) rows.push(AXES.map((a, i) => c("pos", { key: `p${a}`, label: a, value: v((m) => num(m.position[i])), cls: axisClass(a) })));
  if (items.has("range")) {
    // 回転軸は動いたときだけ（プログラムごとに決まるので、再生中に並びは変わらない）
    rows.push(
      AXES.flatMap((a, i) =>
        i >= 3 && (!bounds || (bounds.work.min[i] === 0 && bounds.work.max[i] === 0))
          ? []
          : [c("range", {
              key: `r${a}`, label: `${a} 範囲`, cls: axisClass(a), title: "プログラム全体（ワーク座標）",
              value: bounds ? `${num(bounds.work.min[i]).padStart(W.pos)} 〜 ${num(bounds.work.max[i]).padStart(W.pos)}` : "",
            })],
      ),
    );
  }
  for (const row of rows) if (row[0]) row[0].first = true;
  return rows;
}

type PanelState = { visible: boolean; collapsed: boolean; items: ModalItem[]; height: number };

export class ModalPanel {
  private body: HTMLElement;
  private prev = new Map<string, string>();
  /** 欄ごとの、値が変わった時刻。描き直しても 0.6 秒の色の戻りを途中から続ける */
  private changedAt = new Map<string, number>();
  private row: ExpandedLine | null = null;
  private bounds: CoordinateBounds | null = null;
  /** プログラム全体で出てくる G のモーダル群（番号順）。再生中に G の欄の並びを変えない */
  private groups: string[] = [];

  private get state() { return this.stateOf(); }

  constructor(
    private root: HTMLElement,
    /** 設定の modal（初期化で入れ替わるので、毎回読む） */
    private stateOf: () => PanelState,
    private onChange: () => void,
  ) {
    this.body = root.querySelector(".mp-body")!;
    const pick = root.querySelector<HTMLElement>("#mp-items")!;
    pick.innerHTML = MODAL_ITEMS.map((k) => `<label class="pc"><input type="checkbox" data-item="${k}" />${ITEM_LABEL[k]}</label>`).join("");
    root.querySelector(".mp-fold")!.addEventListener("click", () => this.set({ collapsed: !this.state.collapsed }));
    root.querySelector(".mp-close")!.addEventListener("click", () => this.set({ visible: false }));
    root.querySelector(".mp-pick")!.addEventListener("click", (e) => {
      e.stopPropagation();
      pick.classList.toggle("open");
    });
    pick.addEventListener("click", (e) => e.stopPropagation());
    pick.addEventListener("change", () => {
      const items = MODAL_ITEMS.filter((k) => pick.querySelector<HTMLInputElement>(`[data-item="${k}"]`)!.checked);
      this.set({ items });
    });
    document.addEventListener("click", () => pick.classList.remove("open"));
    // 下の境界のドラッグで、中身の高さの上限を変える（超えたら縦スクロール）
    const grip = root.appendChild(document.createElement("div"));
    grip.className = "mp-grip";
    grip.title = "ドラッグでパネルの高さを変える";
    grip.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      grip.setPointerCapture(e.pointerId);
      const y0 = e.clientY, h0 = this.body.getBoundingClientRect().height;
      const move = (ev: PointerEvent) => (this.body.style.maxHeight = `${Math.round(Math.min(800, Math.max(24, h0 + ev.clientY - y0)))}px`);
      grip.addEventListener("pointermove", move);
      grip.addEventListener("pointerup", () => {
        grip.removeEventListener("pointermove", move);
        this.set({ height: parseInt(this.body.style.maxHeight) || this.state.height });
      }, { once: true });
    });
    this.apply();
  }

  /** 表示の有無・折りたたみ・項目・高さを変える（設定の初期化からも呼ぶ）。 */
  set(s: Partial<PanelState>) {
    Object.assign(this.state, s);
    this.apply();
    this.onChange();
  }

  /** 設定の値を画面に合わせる。 */
  apply() {
    this.root.classList.toggle("hidden", !this.state.visible);
    this.root.classList.toggle("collapsed", this.state.collapsed);
    this.root.querySelector(".mp-fold")!.textContent = this.state.collapsed ? "▸" : "▾";
    this.body.style.maxHeight = `${this.state.height}px`;
    for (const k of MODAL_ITEMS) this.root.querySelector<HTMLInputElement>(`[data-item="${k}"]`)!.checked = this.state.items.includes(k);
    this.prev.clear();
    this.changedAt.clear();
    this.render(false);
  }

  /** 実行のたびに、全体の範囲と、出てくる G のモーダル群を入れ替える。 */
  setBounds(b: CoordinateBounds | null, expanded: ExpandedLine[] = []) {
    this.bounds = b;
    const gs = new Set<string>();
    for (const e of expanded) for (const k in e.modal.g_codes) gs.add(k);
    this.groups = [...gs].sort((a, b) => Number(a) - Number(b));
    this.render(false);
  }

  /** 出す行を変える（どこから来たかを見出しに出す）。 */
  show(row: ExpandedLine | null, source: string) {
    if (row === this.row && this.root.querySelector("#mp-src")!.textContent === source) return;
    this.row = row;
    this.root.querySelector("#mp-src")!.textContent = source;
    this.render(true);
  }

  private render(flash: boolean) {
    if (!this.state.visible || this.state.collapsed) return;
    const m = this.row?.modal;
    const src = this.root.querySelector("#mp-src")!;
    if (!m && !src.textContent) src.textContent = "行を選ぶと、その行の実行後のモーダル状態を表示";
    const rows = cells(m, new Set(this.state.items), this.groups, this.bounds);
    const now = performance.now();
    this.body.innerHTML = `<div class="mp-grid">${rows
      .flat()
      .map((c) => {
        if (flash && this.prev.has(c.key) && this.prev.get(c.key) !== c.value) this.changedAt.set(c.key, now);
        const age = now - (this.changedAt.get(c.key) ?? -1e9);
        const chg = age < FLASH_MS ? " chg" : "";
        const style = `grid-column:${c.first ? "1 / " : ""}span ${c.span}${chg ? `;animation-delay:${-Math.round(age)}ms` : ""}`;
        return `<span class="mp-cell ${c.cls ?? ""}${chg}" style="${style}" data-key="${c.key}"${c.title ? ` title="${c.title}"` : ""}><i>${c.label}</i><b style="width:${c.w}ch">${c.value}</b></span>`;
      })
      .join("")}</div>`;
    // 行を選んでいない（値が空）ときは、次に選んだ行で点滅させない
    this.prev = m ? new Map(rows.flat().map((c) => [c.key, c.value])) : new Map();
  }
}
