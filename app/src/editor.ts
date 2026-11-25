// CodeMirror 6 のエディタ：行番号、NC の色分け、エラー行の印、警告の波線、指定行の強調。
import { EditorView, basicSetup } from "codemirror";
import { Compartment, EditorState, StateEffect, StateField, RangeSet } from "@codemirror/state";
import { Decoration, DecorationSet, GutterMarker, gutter, hoverTooltip } from "@codemirror/view";
import { ncLanguage, ncHighlight } from "./nc-lang";

const setErrors = StateEffect.define<Map<number, string>>();
const setFocus = StateEffect.define<number | null>();
const setCodeMarks = StateEffect.define<DecorationSet>();
const setWarnings = StateEffect.define<DecorationSet>();
/** 警告の行の波線（文言はマウスを重ねたときの title） */
const warnField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    for (const effect of tr.effects) if (effect.is(setWarnings)) return effect.value;
    return value.map(tr.changes);
  },
  provide: (field) => EditorView.decorations.from(field),
});
const codeMarks = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    for (const effect of tr.effects) if (effect.is(setCodeMarks)) return effect.value;
    return tr.docChanged ? Decoration.none : value;
  },
  provide: (field) => EditorView.decorations.from(field),
});

class ErrorMarker extends GutterMarker {
  constructor(readonly message: string) { super(); }
  toDOM() {
    const el = document.createElement("span");
    el.className = "cm-err-mark";
    el.textContent = "●";
    el.title = this.message;
    return el;
  }
}

/** エラー行（1 始まりの行番号 → 文言）。 */
const errorField = StateField.define<Map<number, string>>({
  create: () => new Map(),
  update(v, tr) {
    for (const e of tr.effects) if (e.is(setErrors)) return e.value;
    return v;
  },
});

/** 強調する行（展開結果・経路のクリック先）。 */
const focusField = StateField.define<number | null>({
  create: () => null,
  update(v, tr) {
    for (const e of tr.effects) if (e.is(setFocus)) return e.value;
    return tr.docChanged ? null : v;
  },
});

function lineDecos(state: EditorState): DecorationSet {
  const ranges = [];
  const n = state.doc.lines;
  for (const line of [...state.field(errorField).keys()].sort((a, b) => a - b)) {
    if (line >= 1 && line <= n) ranges.push(Decoration.line({ class: "cm-err-line" }).range(state.doc.line(line).from));
  }
  const f = state.field(focusField);
  if (f !== null && f >= 1 && f <= n) ranges.push(Decoration.line({ class: "cm-focus-line" }).range(state.doc.line(f).from));
  return Decoration.set(ranges, true);
}

const lineDecoField = StateField.define<DecorationSet>({
  create: lineDecos,
  update: (_v, tr) => lineDecos(tr.state),
  provide: (f) => EditorView.decorations.from(f),
});

const errorGutter = gutter({
  class: "cm-err-gutter",
  markers(view) {
    const errs = view.state.field(errorField);
    const doc = view.state.doc;
    const marks = [...errs.entries()]
      .filter(([l]) => l >= 1 && l <= doc.lines)
      .sort((a, b) => a[0] - b[0])
      .map(([l, m]) => new ErrorMarker(m).range(doc.line(l).from));
    return RangeSet.of(marks, true);
  },
  initialSpacer: () => new ErrorMarker(""),
});

// 色は style.css の CSS 変数。暗色・明色の区別は darkMode の区画で切り替える
const theme = EditorView.theme({
  "&": { height: "100%", backgroundColor: "var(--bg-editor)", color: "var(--ed-fg)", fontSize: "calc(13px * var(--ed-zoom, 1))" },
  ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.55" },
  ".cm-content": { caretColor: "var(--ed-caret)" },
  ".cm-cursor": { borderLeftColor: "var(--ed-caret)", borderLeftWidth: "2px" },
  ".cm-gutters": { backgroundColor: "var(--bg-editor)", color: "var(--ed-gutter)", border: "none" },
  ".cm-activeLineGutter": { backgroundColor: "var(--ed-active)", color: "var(--fg)" },
  ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--ed-active) 40%, transparent)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "var(--ed-sel) !important" },
  ".cm-err-line": { backgroundColor: "var(--ed-err-line)" },
  ".cm-focus-line": { backgroundColor: "var(--ed-focus)" },
  ".cm-err-gutter": { width: "14px" },
  ".cm-warn": { textDecoration: "underline wavy #d08a20", textDecorationSkipInk: "none", textUnderlineOffset: "3px", cursor: "help" },
  ".cm-err-mark": { color: "var(--bad)", fontSize: "10px", cursor: "help" },
  ".cm-panels": { backgroundColor: "var(--bg-bar)", color: "var(--fg)" },
  ".cm-searchMatch": { backgroundColor: "var(--ed-focus)" },
  // ツールチップは共通の見た目（style.css の .tip-body・#app-tip と同じ色・角丸・影・矢印）
  ".cm-tooltip.cm-tooltip-hover": { backgroundColor: "var(--tip-bg)", color: "var(--fg)", border: "1px solid var(--tip-line)", borderRadius: "6px", boxShadow: "0 6px 18px var(--shadow)", animation: "tip-in 120ms ease-out" },
  ".cm-tooltip .cm-tooltip-arrow:before": { borderTopColor: "var(--tip-line)", borderBottomColor: "var(--tip-line)" },
  ".cm-tooltip .cm-tooltip-arrow:after": { borderTopColor: "var(--tip-bg)", borderBottomColor: "var(--tip-bg)" },
});
const darkMode = new Compartment();

export interface NcEditor {
  view: EditorView;
  text(): string;
  setText(s: string): void;
  setErrors(errs: { src_line: number; message: string }[]): void;
  /** 警告の行に波線（文言はツールチップ） */
  setWarnings(warns: { src_line: number; message: string }[]): void;
  focusLine(line: number): void;
  /** 再生位置の行を強調して見える所へ送る（カーソルとフォーカスは動かさない）。 */
  markLine(line: number | null): void;
  markCodes(ranges: { from: number; to: number }[]): void;
  setDark(dark: boolean): void;
  /** 文字の大きさを変えた後に、行の高さを測り直す。 */
  remeasure(): void;
}

/** マウスを重ねた語の説明（行番号は 1 始まり、ch は行の中の 0 始まりの位置）。説明が無ければ null */
export type HoverHelp = (line: number, text: string, ch: number) => { from: number; to: number; dom: HTMLElement } | null;

export function createEditor(parent: HTMLElement, onChange: () => void, onCursor: (line: number) => void, hover: HoverHelp): NcEditor {
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: "",
      extensions: [
        basicSetup,
        ncLanguage,
        ncHighlight,
        theme,
        darkMode.of(EditorView.darkTheme.of(true)),
        errorField,
        focusField,
        codeMarks,
        warnField,
        lineDecoField,
        errorGutter,
        hoverTooltip((view, pos) => {
          const line = view.state.doc.lineAt(pos);
          const h = hover(line.number, line.text, pos - line.from);
          return h && { pos: line.from + h.from, end: line.from + h.to, above: true, arrow: true, create: () => ({ dom: h.dom }) };
        }, { hoverTime: 300 }),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) onChange();
          if (u.docChanged || u.selectionSet) onCursor(u.state.doc.lineAt(u.state.selection.main.head).number);
        }),
      ],
    }),
  });

  return {
    view,
    // doc.toString は改行を \n にそろえる（CR だけの行末のファイルにも対応）
    text: () => view.state.doc.toString(),
    setText(s) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: s }, selection: { anchor: 0 } });
    },
    setErrors(errs) {
      const m = new Map<number, string>();
      for (const e of errs) m.set(e.src_line, m.has(e.src_line) ? `${m.get(e.src_line)}\n${e.message}` : e.message);
      view.dispatch({ effects: setErrors.of(m) });
    },
    setWarnings(warns) {
      const doc = view.state.doc;
      const m = new Map<number, string>();
      for (const w of warns) if (w.src_line >= 1 && w.src_line <= doc.lines) m.set(w.src_line, m.has(w.src_line) ? `${m.get(w.src_line)}
${w.message}` : w.message);
      const ranges = [...m].sort((a, b) => a[0] - b[0]).flatMap(([l, msg]) => {
        const line = doc.line(l);
        return line.length ? [Decoration.mark({ class: "cm-warn", attributes: { title: msg } }).range(line.from, line.to)] : [];
      });
      view.dispatch({ effects: setWarnings.of(Decoration.set(ranges)) });
    },
    focusLine(line) {
      if (line < 1 || line > view.state.doc.lines) return;
      const pos = view.state.doc.line(line).from;
      view.dispatch({
        selection: { anchor: pos },
        effects: [setFocus.of(line), EditorView.scrollIntoView(pos, { y: "center" })],
      });
      view.focus();
    },
    setDark(dark) {
      view.dispatch({ effects: darkMode.reconfigure(EditorView.darkTheme.of(dark)) });
    },
    remeasure() {
      view.requestMeasure();
    },
    markLine(line) {
      if (line !== null && (line < 1 || line > view.state.doc.lines)) return;
      const effects: StateEffect<unknown>[] = [setFocus.of(line)];
      if (line !== null) effects.push(EditorView.scrollIntoView(view.state.doc.line(line).from, { y: "nearest", yMargin: 60 }));
      view.dispatch({ effects });
    },
    markCodes(ranges) {
      view.dispatch({ effects: setCodeMarks.of(Decoration.set(ranges.map(({ from, to }) =>
        Decoration.mark({ class: "cm-code-match" }).range(from, to)), true)) });
    },
  };
}
