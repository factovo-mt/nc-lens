// NC のコードの説明（エディタと展開結果のツールチップ）。
// G・M・アドレスの説明は使用中のセットの方言の「解説」の列。空なら基底の組み込みの方言、FANUC の順に探す。
// #変数とマクロ文の説明はここの表。そのブロックの実行後のモーダルを 1 行添える。
import { ncTokenAt } from "./nc-lang";
import { FUNC_LABEL } from "./dialect";
import type { DialectSettings, ModalSnapshot } from "./types";

type Source = () => { dialect: DialectSettings | null; builtins: DialectSettings[] };
/** 使用中のセットの方言と組み込みの方言（main.ts が渡す） */
let source: Source = () => ({ dialect: null, builtins: [] });
export function setHelpSource(fn: Source) {
  source = fn;
}

const MACRO: Record<string, string> = {
  IF: "条件分岐。IF [条件] GOTO n：分岐、IF [条件] THEN 代入：条件付きの代入",
  THEN: "IF の条件が成り立つときの代入",
  GOTO: "シーケンス番号 n への分岐",
  WHILE: "繰り返し。WHILE [条件] DO m 〜 END m を条件の間",
  DO: "繰り返しの範囲の始まり（DO m）",
  END: "繰り返しの範囲の終わり（END m）",
  EQ: "比較：等しい（=）", NE: "比較：等しくない（≠）", GT: "比較：より大きい（>）", LT: "比較：より小さい（<）",
  GE: "比較：以上（≧）", LE: "比較：以下（≦）",
  AND: "論理積（ビットごと）", OR: "論理和（ビットごと）", XOR: "排他的論理和（ビットごと）", MOD: "剰余",
  SIN: "正弦（度）", COS: "余弦（度）", TAN: "正接（度）", ASIN: "逆正弦（度）", ACOS: "逆余弦（度）",
  ATAN: "逆正接（度）。ATAN[a]/[b] で 2 引数", ATN: "逆正接（度）", SQRT: "平方根", SQR: "平方根", ABS: "絶対値",
  BIN: "BCD から 2 進へ", BCD: "2 進から BCD へ", ROUND: "四捨五入", RND: "四捨五入", FIX: "切り捨て", FUP: "切り上げ",
  LN: "自然対数", EXP: "指数関数（e の累乗）", ADP: "小数点の付加", POW: "累乗",
  POPEN: "外部出力の開始", PCLOS: "外部出力の終了", DPRNT: "変数の値の外部出力", BPRNT: "変数の値の外部出力（2 進）",
  SETVN: "変数の名前の設定",
};

/** ローカル変数の番号 → 引数の文字（引数指定 I） */
const ARGS: Record<number, string> = {
  1: "A", 2: "B", 3: "C", 4: "I", 5: "J", 6: "K", 7: "D", 8: "E", 9: "F", 11: "H", 13: "M", 17: "Q", 18: "R", 19: "S", 20: "T",
  21: "U", 22: "V", 23: "W", 24: "X", 25: "Y", 26: "Z",
};
const AXES = ["X", "Y", "Z", "A", "B", "C"];
const MODAL_VAR: Record<number, string> = { 4102: "B", 4107: "D", 4109: "F", 4111: "H", 4113: "M", 4114: "N", 4115: "O", 4119: "S", 4120: "T" };

/** #変数の番号 → 説明（FANUC のカスタムマクロ B の主な番号） */
function variable(n: number): string {
  if (n === 0) return "空（値の無い変数。常に空）";
  if (n >= 1 && n <= 33) return `ローカル変数${ARGS[n] ? `（引数 ${ARGS[n]}）` : ""}。マクロの呼び出しごとに別`;
  if (n >= 100 && n <= 199) return "コモン変数。電源を切ると空";
  if (n >= 500 && n <= 999) return "コモン変数（保持）。電源を切っても残る";
  if (n >= 1000 && n <= 1035) return "入力信号（システム変数）";
  if (n >= 1100 && n <= 1135) return "出力信号（システム変数）";
  if (n >= 2001 && n <= 2400) return "工具補正量（システム変数）";
  const fixed: Record<number, string> = {
    3000: "アラーム。値の番号とメッセージで停止", 3001: "タイマー（ms）", 3002: "タイマー（時間）",
    3003: "シングルブロック停止・補助機能の完了待ちの抑止", 3004: "送りホールド・オーバーライド・イグザクトストップの抑止",
    3006: "停止とメッセージの表示", 3011: "年月日（YYYYMMDD）", 3012: "時分秒（HHMMSS）",
  };
  if (fixed[n]) return fixed[n];
  if (n >= 4001 && n <= 4030) return `モーダル情報：群 ${n - 4000} の G コード（直前のブロック）`;
  if (MODAL_VAR[n]) return `モーダル情報：${MODAL_VAR[n]} の値（直前のブロック）`;
  if (n >= 5001 && n <= 5006) return `ブロックの終点の位置（${AXES[n - 5001]}、ワーク座標）`;
  if (n >= 5021 && n <= 5026) return `現在位置（${AXES[n - 5021]}、機械座標）`;
  if (n >= 5041 && n <= 5046) return `現在位置（${AXES[n - 5041]}、ワーク座標）`;
  if (n >= 5201 && n <= 5206) return `外部ワーク原点オフセット（${AXES[n - 5201]}）`;
  if (n >= 5221 && n <= 5326 && (n - 5221) % 20 < 6) return `ワーク原点オフセット G${54 + Math.floor((n - 5221) / 20)}（${AXES[(n - 5221) % 20]}）`;
  if (n >= 10001 && n <= 19999) return "工具補正量（システム変数）";
  return "変数";
}

const fmt = (v: number) => (Number.isInteger(v) ? String(v) : String(Number(v.toFixed(4))));

/** 使用中の方言・基底・FANUC の順に、ok を満たす値を探す */
function lookup<T>(pick: (d: DialectSettings) => T | undefined, ok: (v: T) => boolean = () => true): T | undefined {
  const { dialect, builtins } = source();
  const base = builtins.find((b) => b.name === (dialect?.base ?? dialect?.name ?? "fanuc"));
  const fanuc = builtins.find((b) => b.name === "fanuc");
  for (const d of [dialect, base, fanuc]) {
    const v = d ? pick(d) : undefined;
    if (v !== undefined && v !== null && ok(v)) return v;
  }
  return undefined;
}

export interface CodeHelp { head: string; body: string; note?: string }

/** 語（G01・X12.5・#100・IF など）の説明。説明の無い語は null。type は色分けの種類 */
export function explainWord(type: string, text: string, modal: ModalSnapshot | null): CodeHelp | null {
  const word = text.replace(/\s+/g, "").toUpperCase();
  if (type === "keyword") return MACRO[word] ? { head: word, body: MACRO[word] } : null;
  if (type === "variableName") {
    const n = Number(word.slice(1));
    return word.length > 1 && Number.isInteger(n) ? { head: `#${n}`, body: variable(n) } : null;
  }
  const m = /^([A-Z])([-+]?[\d.]*)$/.exec(word);
  if (!m) return null;
  const [, letter, num] = m;
  if ((letter === "G" || letter === "M") && num !== "" && Number.isFinite(Number(num))) {
    const k = String(Number(num));
    const table = letter === "G" ? "g_codes" : "m_codes";
    const def = lookup((d) => d[table][k]);
    if (!def) return { head: word, body: "方言の表に無いコード" };
    const body = lookup((d) => d[table][k]?.description, (v) => !!v) ?? FUNC_LABEL[def.function] ?? def.function;
    const group = "group" in def ? def.group : null;
    let note: string | undefined;
    if (group) {
      const cur = modal?.g_codes[String(group)];
      note = `モーダル群 ${group}${cur === undefined ? "" : `：実行後 G${fmt(cur)}`}`;
    }
    return { head: word, body, note };
  }
  // アドレス：方言の読み替え（U → X など）を通して説明を探す
  const role = lookup((d) => d.addresses[letter]) ?? letter;
  const as = role.length === 1 ? role : letter;
  const body = lookup((d) => d.address_descriptions?.[letter], (v) => !!v) ?? lookup((d) => d.address_descriptions?.[as], (v) => !!v);
  if (!body) return null;
  let note: string | undefined;
  if (modal) {
    const axis = AXES.indexOf(as);
    const value: Record<string, number> = { F: modal.feed, S: modal.spindle, T: modal.tool, H: modal.h, D: modal.d };
    if (axis >= 0) note = `実行後の位置 ${as} ${fmt(modal.position[axis])}`;
    else if (value[as] !== undefined) note = `実行後の ${as} ${fmt(value[as])}`;
  }
  return { head: as !== letter ? `${word}（${as} として）` : word, body, note };
}

/** 行の ch（0 始まり）の位置の語の範囲と説明 */
export function explainAt(line: string, ch: number, modal: ModalSnapshot | null) {
  const tok = ncTokenAt(line, ch);
  const help = tok && explainWord(tok.type, tok.text, modal);
  return tok && help ? { from: tok.from, to: tok.to, help } : null;
}

/** ツールチップの中身（折り返さない。長い行は … で省略、全文は title） */
export function helpDom(h: CodeHelp): HTMLElement {
  const el = document.createElement("div");
  el.className = "code-help";
  for (const [cls, text] of [["ch-head", h.head], ["ch-body", h.body], ["ch-note", h.note]] as const) {
    if (!text) continue;
    const d = document.createElement("div");
    d.className = cls;
    d.textContent = text;
    d.title = text;
    el.append(d);
  }
  return el;
}
