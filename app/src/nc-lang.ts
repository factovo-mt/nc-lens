// NC（FANUC 系）の簡易な色分け。字句の区別だけで、解析は nclens-core が担う。
import { StreamLanguage, StringStream, HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t, Tag } from "@lezer/highlight";

const MACRO_WORDS = new Set([
  "IF", "THEN", "GOTO", "WHILE", "DO", "END",
  "EQ", "NE", "GT", "LT", "GE", "LE", "AND", "OR", "XOR", "MOD",
  "SIN", "COS", "TAN", "ASIN", "ACOS", "ATAN", "ATN", "SQRT", "SQR", "ABS", "BIN", "BCD",
  "ROUND", "RND", "FIX", "FUP", "LN", "EXP", "ADP", "POW", "POPEN", "PCLOS", "DPRNT", "BPRNT", "SETVN",
]);
const COORD = "XYZABCUVWIJKR";

/** 座標のアドレス X・Y・Z と円弧の中心 I・J・K は、描画の座標軸と同じ色。回転軸 A・B・C も軸ごとの色（style.css の --axis-*） */
const AXIS_TOKENS: Record<string, string> = { X: "axisX", Y: "axisY", Z: "axisZ", I: "axisI", J: "axisJ", K: "axisK", A: "axisA", B: "axisB", C: "axisC" };
const tokenTable: Record<string, Tag> = Object.fromEntries(Object.values(AXIS_TOKENS).map((n) => [n, Tag.define()]));

const ncParser = {
  name: "nc",
  token(stream: StringStream): string | null {
    if (stream.eatSpace()) return null;
    const ch = stream.peek();
    if (ch === "(") {
      if (!stream.skipTo(")")) stream.skipToEnd(); else stream.next();
      return "comment";
    }
    if (ch === ";") { stream.skipToEnd(); return "comment"; }
    if (ch === "%") { stream.next(); return "meta"; }
    if (stream.match(/^#\d+/) || stream.match(/^#/)) return "variableName";
    const word = stream.match(/^[A-Za-z]{2,}/, false) as RegExpMatchArray | null;
    if (word) {
      // 既知の語だけマクロ文として拾う。G17G55 のような連続アドレスは下で 1 語ずつ
      const w = word[0].toUpperCase();
      for (let n = w.length; n >= 2; n--) {
        if (MACRO_WORDS.has(w.slice(0, n))) { stream.match(new RegExp(`^.{${n}}`)); return "keyword"; }
      }
    }
    if (stream.match(/^[Gg]\s*-?[\d.]+/)) return "typeName";
    if (stream.match(/^[Mm]\s*\d+/)) return "atom";
    if (stream.match(/^[OoNn]\s*\d+/)) return "labelName";
    if (ch && COORD.includes(ch.toUpperCase()) && stream.match(/^[A-Za-z]\s*[-+]?[\d.]*/)) return AXIS_TOKENS[ch.toUpperCase()] ?? "propertyName";
    if (stream.match(/^[A-Za-z]\s*[-+]?[\d.]*/)) return "number";
    if (stream.match(/^[-+]?[\d.]+/)) return "number";
    if (stream.match(/^[[\]]/)) return "bracket";
    stream.next();
    return "operator";
  },
};

export const ncLanguage = StreamLanguage.define({ ...ncParser, tokenTable });

// 色は style.css の CSS 変数（暗色・明色のテーマで切り替わる）
const ncStyle = HighlightStyle.define([
  { tag: t.comment, color: "var(--hl-comment)", fontStyle: "italic" },
  { tag: t.typeName, color: "var(--hl-g)", fontWeight: "600" },  // G
  { tag: t.atom, color: "var(--hl-m)" },                         // M
  { tag: t.labelName, color: "var(--hl-on)" },                   // O・N
  { tag: tokenTable.axisX, color: "var(--axis-x)" },             // X
  { tag: tokenTable.axisY, color: "var(--axis-y)" },             // Y
  { tag: tokenTable.axisZ, color: "var(--axis-z)" },             // Z
  { tag: tokenTable.axisI, color: "var(--axis-i)" },             // I（X を薄く）
  { tag: tokenTable.axisJ, color: "var(--axis-j)" },             // J
  { tag: tokenTable.axisK, color: "var(--axis-k)" },             // K
  { tag: tokenTable.axisA, color: "var(--axis-a)" },             // A
  { tag: tokenTable.axisB, color: "var(--axis-b)" },             // B
  { tag: tokenTable.axisC, color: "var(--axis-c)" },             // C
  { tag: t.propertyName, color: "var(--hl-on)" },                // U・V・W・R
  { tag: t.number, color: "var(--hl-num)" },                     // F・S・T ほか
  { tag: t.variableName, color: "var(--hl-var)" },               // #変数
  { tag: t.keyword, color: "var(--hl-kw)", fontWeight: "600" },  // マクロ文
  { tag: t.meta, color: "var(--hl-comment)" },
  { tag: [t.bracket, t.operator], color: "var(--hl-op)" },
]);

export const ncHighlight = syntaxHighlighting(ncStyle);

/** NC の語と元の文字位置。コメントとマクロの予約語は色分けと同じ規則で読み飛ばす。 */
export function ncCodeWords(line: string) {
  const stream = new StringStream(line, 4, 2);
  const words: { address: string; value: number; from: number; to: number }[] = [];
  while (!stream.eol()) {
    stream.start = stream.pos;
    const type = ncParser.token(stream);
    if (type !== "typeName" && type !== "atom" && type !== "number" && type !== "propertyName") continue;
    const match = /^([a-z])\s*([-+]?(?:\d+(?:\.\d*)?|\.\d+))$/i.exec(line.slice(stream.start, stream.pos));
    if (match) words.push({ address: match[1].toUpperCase(), value: Number(match[2]), from: stream.start, to: stream.pos });
  }
  return words;
}

const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

/** 1 行を、エディタと同じ配色の HTML にする（展開結果の表で使う）。
 *  クラスはエディタが読み込んだ ncStyle のものなので、色の定義は 1 か所だけ。 */
export function highlightNc(line: string): string {
  const stream = new StringStream(line, 4, 2);
  let html = "";
  while (!stream.eol()) {
    stream.start = stream.pos;
    const type = ncParser.token(stream);
    if (stream.pos === stream.start) stream.next();
    const text = esc(line.slice(stream.start, stream.pos));
    const tag = type ? tokenTable[type] ?? (t as unknown as Record<string, Tag>)[type] : undefined;
    const cls = tag ? ncStyle.style([tag]) : null;
    html += cls ? `<span class="${cls}">${text}</span>` : text;
  }
  return html;
}

/** ch（0 始まり）の位置にある語と種類（色分けと同じ規則）。空白・演算子・コメントの上は null。 */
export function ncTokenAt(line: string, ch: number): { from: number; to: number; type: string; text: string } | null {
  const stream = new StringStream(line, 4, 2);
  while (!stream.eol()) {
    stream.start = stream.pos;
    const type = ncParser.token(stream);
    if (stream.pos === stream.start) stream.next();
    if (ch >= stream.start && ch < stream.pos) {
      const skip = !type || ["operator", "bracket", "comment", "meta"].includes(type);
      return skip ? null : { from: stream.start, to: stream.pos, type, text: line.slice(stream.start, stream.pos) };
    }
  }
  return null;
}
