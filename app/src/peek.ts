import { EditorView } from "@codemirror/view";
import type { NcEditor } from "./editor";
import { ncCodeWords } from "./nc-lang";
import { save, settings } from "./settings";
import { refreshAppSettings } from "./appsettings";
import type { DialectSettings, Report, RunResult } from "./types";
import { fetchReport, reportAction, summaryHtml } from "./report";
import { hideTip, showTip } from "./tip";

type Occurrence = { from: number; to: number; line: number };
type Code = {
  address: string; value: number; label: string; group: string;
  source: Occurrence[]; rows: Set<number>; active: Set<number>; locations: Map<number, number>; written: number;
};
/** 左のアイコンの列：グループ（W15 の分類）。G はモーダルコード（G のグループ別） */
const RAIL: { key: string; label: string; icon: string }[] = [
  { key: "sum", label: "集計（加工時間・距離・工具ごと）", icon: '<path d="M2 14.5h12"/><path d="M3.5 14.5V9h2.5v5.5M7 14.5V4h2.5v10.5M10.5 14.5V7H13v7.5"/>' },
  { key: "all", label: "すべて", icon: '<rect x="2" y="2" width="5" height="5"/><rect x="9" y="2" width="5" height="5"/><rect x="2" y="9" width="5" height="5"/><rect x="9" y="9" width="5" height="5"/>' },
  { key: "T", label: "工具番号 T", icon: '<path d="M6 1.5h4v5H6zM6 6.5h4l-.6 6L8 14.5l-1.4-2-.6-6"/><path d="M6.4 9l3.2-1.2M6.6 11.5l2.8-1.1"/>' },
  { key: "D", label: "径補正番号 D", icon: '<circle cx="8" cy="8" r="2.6"/><circle cx="8" cy="8" r="6" stroke-dasharray="2 1.6"/><path d="M10.6 8h3.4"/>' },
  { key: "H", label: "長補正番号 H", icon: '<path d="M3 2h10M3 14h10M8 3.5v9M5.8 5.6L8 3.4l2.2 2.2M5.8 10.4L8 12.6l2.2-2.2"/>' },
  { key: "G", label: "モーダル G コード", icon: '<rect x="1.5" y="1.5" width="13" height="13" rx="2.5"/><path d="M10.6 5.6A3.2 3.2 0 1 0 11 9.4V8.2H8.6"/>' },
  { key: "M", label: "M コード", icon: '<rect x="1.5" y="1.5" width="13" height="13" rx="2.5"/><path d="M4.5 11.5v-7L8 9l3.5-4.5v7"/>' },
];
const railKey = (c: { address: string }) => c.address;

const groups: Record<number, string> = {
  1: "移動", 2: "平面", 3: "絶対・増分", 6: "単位", 7: "径補正", 8: "長補正",
  10: "復帰位置", 12: "座標系", 14: "マクロ呼び出し",
};

export class CodePeek {
  private pane = document.querySelector<HTMLElement>("#code-peek")!;
  private toggle = document.querySelector<HTMLButtonElement>("#peek-toggle")!;
  private list = document.querySelector<HTMLElement>("#peek-list")!;
  private rail = document.querySelector<HTMLElement>("#peek-rail")!;
  private table = document.querySelector<HTMLElement>("#tab-expanded")!;
  private prev = this.pane.querySelector<HTMLButtonElement>("[data-offset='-1']")!;
  private next = this.pane.querySelector<HTMLButtonElement>("[data-offset='1']")!;
  private codes = new Map<string, Code>();
  private selected = new Set<string>();
  private occurrences: Occurrence[] = [];
  private rows: number[] = [];
  private index = -1;
  private sourceDoc?: NcEditor["view"]["state"]["doc"];
  /** 集計のページ：最後の実行のレポート。工具径は今のポストから */
  private report: Report | null = null;
  private reportErr = "";
  toolDia: (t: string) => number | null | undefined = () => null;

  constructor(private editor: NcEditor, private onRows: (rows: ReadonlySet<number> | null) => void,
    private showExpanded: () => void) {
    this.toggle.onclick = () => {
      settings.peek.open = !settings.peek.open;
      this.apply();
      save();
      refreshAppSettings();
    };
    this.pane.querySelector<HTMLElement>(".splitter")!.addEventListener("pointerdown", (e) => {
      if (e.button !== 0) return;
      const handle = e.currentTarget as HTMLElement;
      const x = e.clientX;
      const width = this.pane.getBoundingClientRect().width;
      handle.setPointerCapture(e.pointerId);
      handle.onpointermove = (move) => {
        settings.peek.width = Math.round(Math.max(164, Math.min(window.innerWidth * 0.4, width + x - move.clientX)));
        this.apply();
      };
      handle.onlostpointercapture = () => { handle.onpointermove = null; save(); refreshAppSettings(); };
    });
    this.list.onclick = async (e) => {
      const act = (e.target as HTMLElement).closest<HTMLElement>("[data-ract]")?.dataset.ract;
      if (act && this.report) {
        const msg = await reportAction(act, this.report);
        const state = this.list.querySelector(".sum-state");
        if (msg && state) state.textContent = msg;
        return;
      }
      const key = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-code]")?.dataset.code;
      if (!key) return;
      const wasSelected = this.selected.has(key);
      if (!e.ctrlKey) this.selected.clear();
      if (wasSelected) this.selected.delete(key);
      else this.selected.add(key);
      this.highlight();
      if (!wasSelected) this.jump(1);
    };
    this.initRail();
    this.prev.onclick = () => this.jump(-1);
    this.next.onclick = () => this.jump(1);
    window.addEventListener("keydown", (e) => {
      if (!this.selected.size || (e.target instanceof Element && e.target.closest("dialog[open]"))) return;
      if (e.key === "Escape") {
        this.selected.clear();
        this.highlight();
      } else if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
        e.preventDefault();
        e.stopPropagation();
        this.jump(e.key === "ArrowUp" ? -1 : 1);
      }
    }, true);
  }

  apply() {
    this.pane.hidden = !settings.peek.open;
    this.toggle.setAttribute("aria-expanded", String(settings.peek.open));
    this.pane.parentElement!.style.setProperty("--peek-width", settings.peek.open ? `clamp(164px, ${settings.peek.width}px, 40vw)` : "0px");
  }

  load(result: RunResult, dialects: DialectSettings[]) {
    this.codes.clear();
    const modalGroups = new Map<number, number>();
    const aliases: Record<string, string> = {};
    const mFunctions: Record<string, string> = {};
    for (const dialect of dialects) {
      Object.assign(aliases, dialect.addresses);
      for (const [value, def] of Object.entries(dialect.m_codes)) mFunctions[Number(value)] = def.function;
      for (const [value, def] of Object.entries(dialect.g_codes)) {
        if (def.group) modalGroups.set(Number(value), def.group);
        else modalGroups.delete(Number(value));
      }
    }
    for (const { modal, source_g_codes } of result.expanded) {
      for (const [group, value] of Object.entries(modal.g_codes)) modalGroups.set(value, Number(group));
      if (source_g_codes) for (const [group, value] of Object.entries(source_g_codes)) modalGroups.set(value, Number(group));
    }
    const getCode = (letter: string, value: number, create = true) => {
      const address = aliases[letter] ?? letter;
      const modalGroup = modalGroups.get(value);
      const group = address === "T" ? "工具番号" : address === "D" ? "径補正番号" : address === "H" ? "長補正番号"
        : address === "M" ? "M コード" : address !== "G" ? null
        : [73, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89].includes(value) && modalGroup === 1 ? "固定サイクル"
        : modalGroup === undefined ? "その他の G コード" : groups[modalGroup] ?? `G グループ ${modalGroup}`;
      if (!group) return;
      const key = address + value;
      let code = this.codes.get(key);
      if (!code) {
        if (!create) return;
        const number = address === "G" || address === "M" ? String(value).replace(/^\d(?=\.|$)/, "0$&") : String(value);
        code = { address, value, label: address + number, group, source: [], rows: new Set(), active: new Set(), locations: new Map(), written: 0 };
        this.codes.set(key, code);
      }
      return code;
    };
    const doc = this.editor.view.state.doc;
    this.sourceDoc = doc;
    const sourceCodes = new Map<number, Set<Code>>();
    for (let n = 1; n <= doc.lines; n++) {
      const line = doc.line(n);
      for (const word of ncCodeWords(line.text)) {
        const code = getCode(word.address, word.value);
        if (!code) continue;
        code.source.push({ from: line.from + word.from, to: line.from + word.to, line: n });
        if (!sourceCodes.has(n)) sourceCodes.set(n, new Set());
        sourceCodes.get(n)!.add(code);
      }
    }
    result.expanded.forEach(({ text, src_line }, i) => {
      const original = src_line > 0 && src_line <= doc.lines ? doc.line(src_line).text : "";
      const dynamic = new Set([...original.replace(/\([^)]*(?:\)|$)|;.*/g, "").matchAll(/([a-z])\s*(?=[#\[])/gi)].map((m) => m[1].toUpperCase()));
      for (const word of ncCodeWords(text)) {
        const code = getCode(word.address, word.value, dynamic.has(word.address));
        if (!code) continue;
        code.written++;
        code.rows.add(i);
        if (code.address === "M") code.active.add(i);
      }
      for (const code of sourceCodes.get(src_line) ?? []) {
        code.rows.add(i);
        if (!code.locations.has(src_line)) code.locations.set(src_line, i);
      }
    });
    result.expanded.forEach(({ modal, source_g_codes }, i) => {
      const gCodes = source_g_codes ? { ...modal.g_codes, ...source_g_codes } : modal.g_codes;
      const keys = [`T${modal.tool}`, `D${modal.d}`, `H${modal.h}`, ...Object.values(gCodes).map((g) => `G${g}`)];
      for (const key of keys) {
        const code = this.codes.get(key);
        code?.rows.add(i);
        code?.active.add(i);
      }
    });
    for (const call of result.calls ?? []) {
      const i = call.expanded_index_after - (call.kind === "M99" ? 1 : 0);
      const row = result.expanded[i];
      if (!row || (call.kind === "M98" && !row.call_stack.some((site) => site.endsWith(`:${call.src_line}`)))) continue;
      const fn = call.kind === "M98" ? "subprogram_call" : "subprogram_return";
      for (const code of sourceCodes.get(call.src_line) ?? []) {
        if (code.address !== "M" || (mFunctions[code.value] ?? (code.label === call.kind ? fn : "")) !== fn) continue;
        code.rows.add(i);
        if (!code.locations.has(call.src_line)) code.locations.set(call.src_line, i);
      }
    }
    this.selected = new Set([...this.selected].filter((key) => this.codes.has(key)));
    this.render();
    this.highlight();
    void this.refreshSummary();
  }

  /** 集計のページの元（レポート）を読み直す。再実行のたび */
  private async refreshSummary() {
    try {
      this.report = await fetchReport();
      this.reportErr = "";
    } catch (e) {
      this.report = null;
      this.reportErr = `集計の作成失敗：${e}`;
    }
    if (settings.peek.group === "sum") this.render();
  }

  /** アイコンの列：クリック・↑↓（Home・End）で切り替え。ホバーとフォーカスで名前と件数 */
  private initRail() {
    this.rail.innerHTML = RAIL.map((g) =>
      `<button role="tab" data-group="${g.key}" data-grp="${g.key}" aria-label="${g.label}"><svg viewBox="0 0 16 16" aria-hidden="true">${g.icon}</svg></button>`).join("");
    const buttons = [...this.rail.querySelectorAll<HTMLButtonElement>("button")];
    const choose = (key: string, focus = false) => {
      settings.peek.group = key;
      save();
      this.render();
      this.highlight();
      if (focus) this.rail.querySelector<HTMLButtonElement>(`[data-group="${key}"]`)!.focus();
    };
    this.rail.onclick = (e) => {
      const key = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-group]")?.dataset.group;
      if (key) choose(key);
    };
    this.rail.onkeydown = (e) => {
      const i = buttons.findIndex((b) => b.dataset.group === settings.peek.group);
      const j = e.key === "ArrowDown" ? (i + 1) % buttons.length : e.key === "ArrowUp" ? (i - 1 + buttons.length) % buttons.length
        : e.key === "Home" ? 0 : e.key === "End" ? buttons.length - 1 : -1;
      if (j < 0) return;
      e.preventDefault();
      choose(buttons[j].dataset.group!, true);
      this.showTip(buttons[j]);
    };
    for (const b of buttons) {
      b.onpointerenter = () => this.showTip(b);
      b.onfocus = () => this.showTip(b);
      b.onpointerleave = b.onblur = () => hideTip(b);
    }
  }

  private showTip(b: HTMLElement) {
    showTip(b, b.dataset.tip ?? b.getAttribute("aria-label") ?? "", "right");
  }

  /** アイコンの選択と件数（コードの種類の数）を描き直す。選択中のグループを返す */
  private renderRail() {
    const count = (key: string) => key === "all" ? this.codes.size : [...this.codes.values()].filter((c) => railKey(c) === key).length;
    const cur = RAIL.some((g) => g.key === settings.peek.group) ? settings.peek.group : "all";
    for (const b of this.rail.querySelectorAll<HTMLButtonElement>("button[data-group]")) {
      const g = RAIL.find((x) => x.key === b.dataset.group)!;
      const n = count(g.key), on = g.key === cur;
      b.dataset.tip = g.key === "sum" ? g.label : `${g.label}（${n}）`;
      b.setAttribute("aria-label", b.dataset.tip);
      b.setAttribute("aria-selected", String(on));
      b.tabIndex = on ? 0 : -1;
      b.classList.toggle("empty", g.key !== "sum" && n === 0);
    }
    return cur;
  }

  private render() {
    const cur = this.renderRail();
    this.list.replaceChildren();
    this.list.classList.toggle("sum", cur === "sum");
    if (cur === "sum") {
      this.list.innerHTML = this.report ? summaryHtml(this.report, this.toolDia) : `<div class="empty">${this.reportErr || "実行結果なし"}</div>`;
      return;
    }
    const branches = new Map<string, HTMLElement>();
    const modal = document.createElement("details");
    modal.open = true;
    const heading = document.createElement("summary");
    heading.textContent = "モーダルコード";
    heading.dataset.grp = "G";
    modal.append(heading);
    const branchesInOrder = ["工具番号", "径補正番号", "長補正番号"];
    const codes = [...this.codes].sort(([, a], [, b]) => {
      const order = (c: Code) => branchesInOrder.includes(c.group) ? branchesInOrder.indexOf(c.group) : c.address === "G" ? 3 : 4;
      return order(a) - order(b) || a.value - b.value;
    });
    let modalCount = 0;
    for (const [key, code] of codes) {
      if (cur !== "all" && railKey(code) !== cur) continue;
      // G だけを出すときはモーダルコードの見出しを省く
      const parent = code.address === "G" && cur === "all" ? modal : this.list;
      if (parent === modal && !modal.isConnected) this.list.append(modal);
      let branch = branches.get(code.group);
      if (!branch) {
        const details = document.createElement("details");
        details.open = true;
        const summary = document.createElement("summary");
        summary.textContent = code.group;
        summary.dataset.grp = railKey(code);
        branch = document.createElement("div");
        details.append(summary, branch);
        parent.append(details);
        branches.set(code.group, branch);
      }
      const count = code.source.length || code.written;
      if (code.address === "G") modalCount += count;
      const button = document.createElement("button");
      button.dataset.code = key;
      button.textContent = code.label;
      const badge = document.createElement("span");
      badge.className = "count";
      badge.textContent = String(count);
      button.append(badge);
      branch.append(button);
    }
    heading.textContent += ` (${modalCount})`;
    for (const branch of branches.values()) {
      const count = [...branch.querySelectorAll(".count")].reduce((sum, badge) => sum + Number(badge.textContent), 0);
      branch.parentElement!.querySelector("summary")!.textContent += ` (${count})`;
    }
    if (!this.list.childElementCount) this.list.textContent = cur === "all" ? "使用コードなし" : "このグループのコードなし";
  }

  private highlight() {
    const marks: Occurrence[] = [];
    const rows = new Set<number>();
    const active = new Set<number>();
    for (const key of this.selected) {
      const code = this.codes.get(key)!;
      if (this.sourceDoc === this.editor.view.state.doc) marks.push(...code.source);
      for (const row of code.rows) rows.add(row);
      for (const row of code.active) active.add(row);
    }
    this.occurrences = marks.sort((a, b) => a.from - b.from);
    this.rows = [...rows].sort((a, b) => a - b);
    this.index = -1;
    this.prev.disabled = this.next.disabled = !marks.length && !rows.size;
    this.editor.markCodes(marks);
    const tbody = this.table.querySelector("tbody");
    this.table.querySelectorAll(".code-match").forEach((el) => el.classList.remove("code-match"));
    for (const row of rows) tbody?.children[row]?.classList.add("code-match");
    this.list.querySelectorAll<HTMLButtonElement>("button[data-code]").forEach((button) =>
      button.setAttribute("aria-pressed", String(this.selected.has(button.dataset.code!))));
    this.onRows(this.selected.size ? active : null);
  }

  private jump(direction: -1 | 1) {
    const n = this.occurrences.length || this.rows.length;
    if (!n) return;
    const doc = this.editor.view.state.doc;
    this.index = this.index < 0 ? direction > 0 ? 0 : n - 1 : (this.index + direction + n) % n;
    const occurrence = this.occurrences[this.index];
    const tbody = this.table.querySelector("tbody");
    if (occurrence && occurrence.to <= doc.length) {
      this.editor.view.dispatch({ selection: { anchor: occurrence.from }, effects: EditorView.scrollIntoView(occurrence.from, { y: "center" }) });
    }
    const row = occurrence
      ? [...this.selected].map((key) => this.codes.get(key)?.locations.get(occurrence.line)).find((i) => i !== undefined)
      : this.rows[this.index];
    if (row !== undefined) {
      this.showExpanded();
      tbody?.children[row]?.scrollIntoView({ block: "center" });
    }
  }
}
