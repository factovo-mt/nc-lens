// コード定義のページ（アドレス表・G コード表・M コード表・ユーザー定義・マクロ構文・マクロ記号）の表示と読み取り。
// ニーモニック・固有（nclens-core の DialectSettings）はポスト（セット）の一部。画面の枠は sets.ts。
// ニーモニック・固有だけの書き出し・読み込み（TOML・JSON）は他の PC へ渡す補助。検査はコアの validate_dialect。
import { invoke } from "./platform";
import { ADDRESS_ROLES, type CustomCode, type DialectSettings, type GCodeDefinition, type MCodeDefinition, type MacroSettings } from "./types";

const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
/** コード定義のページ */
export const DIALECT_TABS = ["addr", "g", "m", "custom", "macro", "sym"] as const;

/** 機能の名前（コアの組み込み FANUC の表の名前）→ 画面の説明 */
export const FUNC_LABEL: Record<string, string> = {
  ignore: "無視", unsupported: "未対応（診断・読み飛ばし）", dwell_feed: "休止（F：秒）", work_select_h: "ワーク座標（G15 H）", rapid: "早送り", linear: "直線補間", arc_cw: "円弧 CW", arc_ccw: "円弧 CCW", dwell: "休止",
  set_offsets: "データ設定（G10）", plane_xy: "平面 XY", plane_zx: "平面 ZX", plane_yz: "平面 YZ", inch: "インチ", millimeter: "ミリ",
  reference_return: "原点復帰", secondary_reference_return: "第 2 原点復帰", radius_cancel: "径補正キャンセル", radius_left: "径補正 左",
  radius_right: "径補正 右", length_positive: "長さ補正 ＋", length_negative: "長さ補正 −", length_cancel: "長さ補正キャンセル",
  length_dynamic: "長さ補正（G43.1）", spindle_limit: "回転数の上限", scaling: "スケーリング", local_coordinates: "ローカル座標",
  machine_coordinates: "機械座標", work_1: "ワーク 1（G54）", work_2: "ワーク 2（G55）", work_3: "ワーク 3（G56）", work_4: "ワーク 4（G57）",
  work_5: "ワーク 5（G58）", work_6: "ワーク 6（G59）", exact_stop: "イグザクトストップ", continuous: "連続切削", macro_call: "マクロ呼び出し",
  modal_call: "モーダル呼び出し", modal_cancel: "モーダル呼び出しキャンセル", rotate: "座標回転", rotate_cancel: "座標回転キャンセル",
  cycle_high_speed_peck: "高速深穴（G73）", cycle_cancel: "固定サイクルキャンセル", cycle_drill: "ドリル（G81）", cycle_dwell: "ドリル休止（G82）",
  cycle_peck: "深穴（G83）", cycle_tap: "タップ（G84）", cycle_bore: "ボーリング（G85）", cycle_bore_stop: "ボーリング停止（G86）",
  cycle_back_bore: "バックボーリング（G87）", cycle_bore_manual: "ボーリング手動（G88）", cycle_bore_dwell: "ボーリング休止（G89）",
  absolute: "絶対値", incremental: "増分値", inverse_time: "逆時間送り", feed_per_minute: "毎分送り", feed_per_revolution: "毎回転送り",
  constant_surface_speed: "周速一定", constant_rpm: "回転数一定", cycle_initial_return: "初期点復帰", cycle_r_return: "R 点復帰",
  stop: "プログラムストップ", optional_stop: "オプショナルストップ", end: "プログラム終了", spindle_cw: "主軸正転", spindle_ccw: "主軸逆転",
  spindle_stop: "主軸停止", tool_change: "工具交換", coolant_mist: "ミスト", coolant_on: "クーラント入", coolant_off: "クーラント切",
  rewind: "終了・巻き戻し", subprogram_call: "サブプログラム呼び出し", subprogram_return: "サブプログラム終了", call: "マクロ呼び出し（O 番号）",
};
const ROLE_LABEL: Record<string, string> = { DwellSeconds: "休止の秒（X・P）", Repeat: "繰り返し回数（L）" };
const FIELD_LABEL: Record<string, string> = { name: "名前", base: "基底", addresses: "アドレス表", g_codes: "G コード表", m_codes: "M コード表", custom: "ユーザー定義", macro: "マクロ記号" };

let builtins: DialectSettings[] = [];
let gFuncs: string[] = [];
let mFuncs: string[] = [];

/** 組み込みのニーモニック・固有（FANUC・OSP）。初回の起動で一覧の元にする。 */
export async function builtinDialects(): Promise<DialectSettings[]> {
  if (!builtins.length) {
    builtins = await invoke<DialectSettings[]>("dialect_builtin");
    // 機能の選択肢：組み込みのニーモニック・固有に出てくる名前すべてと、表に無い特別な名前
    gFuncs = [...new Set(["ignore", "unsupported", ...builtins.flatMap((b) => Object.values(b.g_codes).map((d) => d.function)), "length_dynamic"])];
    mFuncs = [...new Set(["ignore", "call", ...builtins.flatMap((b) => Object.values(b.m_codes).map((d) => d.function))])];
  }
  return structuredClone(builtins);
}

const funcOptions = (list: string[], v: string) =>
  [...new Set([...list, v])].map((f) => `<option value="${esc(f)}"${f === v ? " selected" : ""}>${esc(FUNC_LABEL[f] ? `${FUNC_LABEL[f]}（${f}）` : f)}</option>`).join("");
const input = (col: string, v: string | number | null, cls = "", hint = "") =>
  `<td><input data-col="${col}" class="${cls}" value="${esc(v === null ? "" : String(v))}"${hint ? ` placeholder="${esc(hint)}" title="空：${esc(hint)}"` : ""} spellcheck="false" /></td>`;

/** 表示中のニーモニック・固有（解説の列：空の行は基底の組み込みの解説を薄く出す） */
let shown: DialectSettings | null = null;
function baseDesc(tab: "addr" | "g" | "m", k: string): string {
  const base = builtins.find((b) => b.name === (shown?.base ?? shown?.name ?? "fanuc"));
  const fanuc = builtins.find((b) => b.name === "fanuc");
  for (const b of [base, fanuc]) {
    const v = !b ? "" : tab === "addr" ? b.address_descriptions?.[k] : (tab === "g" ? b.g_codes : b.m_codes)[k]?.description;
    if (v) return v;
  }
  return "";
}
const del = `<td><button class="pdel" title="行を削除">✕</button></td>`;
const numSort = (a: [string, unknown], b: [string, unknown]) => Number(a[0]) - Number(b[0]) || a[0].localeCompare(b[0]);

/** 表の 1 行（新しい行は空で作る） */
export const ROWS: Record<string, (k: string, v: unknown) => string> = {
  addr: (k, v) =>
    `<tr data-key="${esc(k)}">${input("key", k, "short")}<td><select data-col="role">${ADDRESS_ROLES.map((r) => `<option value="${r}"${r === v ? " selected" : ""}>${ROLE_LABEL[r] ?? r}</option>`).join("")}</select></td>${input("desc", shown?.address_descriptions?.[k] ?? "", "wide", baseDesc("addr", k))}${del}</tr>`,
  g: (k, v) => {
    const d = (v ?? { function: "linear", group: null }) as GCodeDefinition;
    return `<tr data-key="${esc(k)}">${input("key", k, "short")}<td><select data-col="function">${funcOptions(gFuncs, d.function)}</select></td>${input("group", d.group, "short")}${input("desc", d.description ?? "", "wide", baseDesc("g", k))}${del}</tr>`;
  },
  m: (k, v) => {
    const d = (v ?? { function: "call", program: null }) as MCodeDefinition;
    return `<tr data-key="${esc(k)}">${input("key", k, "short")}<td><select data-col="function">${funcOptions(mFuncs, d.function)}</select></td>${input("program", d.program, "short")}${input("desc", d.description ?? "", "wide", baseDesc("m", k))}${del}</tr>`;
  },
  custom: (k, v) => {
    const c = (v ?? { call: 9100 }) as CustomCode;
    const kind = "call" in c ? "call" : "replace";
    const val = "call" in c ? c.call : c.replace;
    return `<tr data-key="${esc(k)}">${input("key", k, "short")}<td><select data-col="kind"><option value="call"${kind === "call" ? " selected" : ""}>マクロ呼び出し（O 番号）</option><option value="replace"${kind === "replace" ? " selected" : ""}>置き換え（NC の語）</option></select></td>${input("value", val, "wide")}${del}</tr>`;
  },
};
const HEADS: Record<string, [string, string][]> = {
  addr: [["アドレス", "NC に書く文字"], ["役割", "FANUC での意味"], ["解説", "ツールチップの文言（空：基底の解説）"]],
  g: [["G コード", "数値・小数可（例：43.1）"], ["機能", ""], ["モーダル群", "1〜99（空・0：非モーダル）"], ["解説", "ツールチップの文言（空：基底の解説）"]],
  m: [["M コード", "整数"], ["機能", ""], ["プログラム O", "機能「マクロ呼び出し」のみ"], ["解説", "ツールチップの文言（空：基底の解説）"]],
  custom: [["コード", "例：G100、M50、A100"], ["種類", ""], ["値", "O 番号／置換先の NC の語（例：G01 Z-5.）"]],
};
const NOTES: Record<string, string> = {
  addr: "NC の文字 → FANUC の役割の読み替え表。表に無い文字：元の役割",
  g: "G コードの機能・モーダル群（基底のニーモニック・固有への追加・上書き）",
  m: "M コードの機能（マクロ呼び出し：プログラム番号の指定）",
  custom: "独自コードの置換（マクロ呼び出し／別の NC の語）。複数登録可",
};

function tableHtml(tab: string, entries: [string, unknown][]) {
  const head = HEADS[tab].map(([h, sub]) => `<th>${h}${sub ? `<small>${sub}</small>` : ""}</th>`).join("") + "<th></th>";
  return `<p class="pnote">${NOTES[tab]}</p><table class="ptable dtable" data-dtable="${tab}"><thead><tr>${head}</tr></thead><tbody>${entries.map(([k, v]) => ROWS[tab](k, v)).join("")}</tbody></table>
    <button class="pbtn" data-addrow="${tab}">＋ 行を追加</button>`;
}

/** G・M の番号のキーをそろえる（G01 → 1、43.10 → 43.1）。数でなければそのまま */
const codeKey = (s: string) => {
  const t = s.trim().replace(/^[GgMm]/, "");
  return t !== "" && Number.isFinite(Number(t)) ? String(Number(t)) : s.trim();
};

/** 表示中の表 → ニーモニック・固有（行の並びのとおり。キーの重複は後の行が勝つので、検査で印を付ける） */
function readTab(d: DialectSettings, tab: string, body: HTMLElement): string[] {
  const errs: string[] = [];
  const rows = [...body.querySelectorAll<HTMLTableRowElement>(`[data-dtable="${tab}"] tbody tr`)];
  const val = (tr: HTMLElement, col: string) => tr.querySelector<HTMLInputElement | HTMLSelectElement>(`[data-col="${col}"]`)!.value.trim();
  const seen = new Set<string>();
  const key = (tr: HTMLElement, k: string) => {
    const el = tr.querySelector<HTMLInputElement>('[data-col="key"]')!;
    const dup = k === "" || seen.has(k);
    el.classList.toggle("bad", dup);
    el.title = k === "" ? "必須" : dup ? "重複" : "";
    if (dup) errs.push(`${k || "（空）"}：${k === "" ? "必須" : "重複"}`);
    seen.add(k);
    return !dup;
  };
  const optNum = (tr: HTMLElement, col: string) => {
    const v = val(tr, col);
    const el = tr.querySelector<HTMLInputElement>(`[data-col="${col}"]`)!;
    const ok = v === "" || (Number.isInteger(Number(v)) && Number(v) >= 0);
    el.classList.toggle("bad", !ok);
    if (!ok) errs.push(`${v}：0 以上の整数`);
    return v === "" || !ok ? null : Number(v);
  };
  if (tab === "addr") {
    d.addresses = {};
    d.address_descriptions = {};
    for (const tr of rows) {
      const k = val(tr, "key").toUpperCase();
      if (!key(tr, k)) continue;
      d.addresses[k] = val(tr, "role") as DialectSettings["addresses"][string];
      if (val(tr, "desc") && /^[A-Z]$/.test(k)) d.address_descriptions[k] = val(tr, "desc");
    }
  } else if (tab === "g") {
    d.g_codes = {};
    for (const tr of rows) {
      const k = codeKey(val(tr, "key"));
      if (key(tr, k)) d.g_codes[k] = { function: val(tr, "function"), group: optNum(tr, "group"), description: val(tr, "desc") || undefined };
    }
  } else if (tab === "m") {
    d.m_codes = {};
    for (const tr of rows) {
      const k = codeKey(val(tr, "key"));
      if (key(tr, k)) d.m_codes[k] = { function: val(tr, "function"), program: optNum(tr, "program"), description: val(tr, "desc") || undefined };
    }
  } else if (tab === "custom") {
    d.custom = {};
    for (const tr of rows) {
      const k = val(tr, "key").toUpperCase().replace(/\s+/g, "");
      if (!key(tr, k)) continue;
      const v = val(tr, "value");
      const call = val(tr, "kind") === "call";
      const el = tr.querySelector<HTMLInputElement>('[data-col="value"]')!;
      const ok = call ? /^\d+$/.test(v.replace(/^O/i, "")) : v !== "";
      el.classList.toggle("bad", !ok);
      if (!ok) { errs.push(`${k}：${call ? "O 番号（整数）" : "置き換える語"}`); continue; }
      d.custom[k] = call ? { call: Number(v.replace(/^O/i, "")) } : { replace: v };
    }
  } else if (tab === "macro") {
    const m = body.querySelector<HTMLInputElement>('input[name="dmacro"]:checked')?.value;
    const macro: MacroSettings = { ...d.macro };
    if (m === "fanuc" || m === "osp") macro.syntax = m;
    else delete macro.syntax;
    d.macro = macro;
    const base = body.querySelector<HTMLSelectElement>("[data-base]")!.value;
    d.base = base || null;
  } else if (tab === "sym") {
    errs.push(...readSymbols(d, body));
  }
  return errs;
}

function macroHtml(d: DialectSettings) {
  const radio = (v: string, label: string, note: string) =>
    `<label class="pc"><input type="radio" name="dmacro" value="${v}"${(d.macro?.syntax ?? "") === v ? " checked" : ""} />${label}<em>${note}</em></label>`;
  return `<section class="psec"><h4>マクロ構文</h4><div class="pchecks col">
      ${radio("fanuc", "FANUC", "カスタムマクロ B（#変数・IF・GOTO・WHILE）")}
      ${radio("osp", "OSP", "OSP ユーザータスク（VC 変数・IF・GOTO・CALL。暫定・基本範囲）")}
      ${radio("", "基底と同じ", "基底のニーモニック・固有の構文")}
    </div></section>
    <section class="psec"><h4>ニーモニック・固有の名前<small>レポート表示用。セット名とは別</small></h4><div class="pgrid">
      <label class="pf"><span>名前</span><input data-dname value="${esc(d.name)}" spellcheck="false" /></label>
    </div><div class="pchecks">
      <button class="pbtn" data-dfile="import" title="ニーモニック・固有のみファイルから読み込み（このセットのニーモニック・固有の置き換え）">ニーモニック・固有の読み込み…</button>
      <button class="pbtn" data-dfile="export" title="このセットのニーモニック・固有のみファイルへ書き出し">ニーモニック・固有の書き出し…</button>
    </div></section>
    <section class="psec"><h4>基底のニーモニック・固有<small>表に無いコード：基底の定義</small></h4><div class="pgrid">
      <label class="pf"><span>基底</span><select data-base>${["", ...builtins.map((b) => b.name)]
        .map((b) => `<option value="${b}"${(d.base ?? "") === b ? " selected" : ""}>${b ? b.toUpperCase() : "なし（この表のみ）"}</option>`).join("")}</select></label>
    </div></section>`;
}

/** コアの検査の文言（Invalid dialect setting g_codes.100）を画面の言葉に直す。 */
export function explain(msg: string) {
  const m = /setting\s+(\w+)(?:\.(.+))?$/.exec(msg);
  if (!m) return msg;
  const sub = m[1] === "macro" && m[2] ? MACRO_HEADS[m[2] as MacroTable]?.[0][0] ?? MACRO_HINT[m[2]]?.split("の")[0] : undefined;
  return `${FIELD_LABEL[m[1]] ?? m[1]}${m[2] ? ` の ${sub ?? m[2]}` : ""}：誤り`;
}

/** ニーモニック・固有のページ 6 つの HTML（ページ名 → 中身）。 */
export function renderDialect(d: DialectSettings): Record<(typeof DIALECT_TABS)[number], string> {
  shown = d;
  return {
    addr: tableHtml("addr", Object.entries(d.addresses).sort()),
    g: tableHtml("g", Object.entries(d.g_codes).sort(numSort)),
    m: tableHtml("m", Object.entries(d.m_codes).sort(numSort)),
    custom: tableHtml("custom", Object.entries(d.custom).sort()),
    macro: macroHtml(d),
    sym: symbolsHtml(d),
  };
}

/** ニーモニック・固有のページ → ニーモニック・固有。誤りの文言の一覧を返す（名前の重複はセット側で見る）。 */
export function collectDialect(base: DialectSettings, body: HTMLElement): { dialect: DialectSettings; errors: string[] } {
  const d = structuredClone(base);
  const errors = DIALECT_TABS.flatMap((t) => readTab(d, t, body));
  const name = body.querySelector<HTMLInputElement>("[data-dname]");
  if (name) {
    const v = name.value.trim();
    name.classList.toggle("bad", !v);
    if (v) d.name = v;
    else errors.push("ニーモニック・固有の名前：必須");
  }
  return { dialect: d, errors };
}

// ---- マクロ記号（[macro]）：括弧・代入・コメントと、変数・演算子・比較・制御文・関数の表 ----
// 画面には実際に使う値（このセットの指定 → 構文が OSP なら組み込み OSP → 組み込み FANUC）を出す。
// 読み取りでは、表示時の値から変えた項目だけをこのセットの指定に残す（変えていない項目は基底に従う）。

type MacroTable = "variables" | "operators" | "comparisons" | "controls" | "functions";
const MACRO_TABLES: MacroTable[] = ["variables", "operators", "comparisons", "controls", "functions"];
const MACRO_SCALARS = ["expression", "arguments", "assignment", "block_skip", "comment_pair", "line_comments"] as const;
const MACRO_FIELDS = ["syntax", ...MACRO_SCALARS, ...MACRO_TABLES] as const;
type Loose = Record<string, unknown>;

/** このセットの指定が無い項目の値（構文が OSP なら組み込み OSP、それ以外は組み込み FANUC） */
function inheritedMacro(d: DialectSettings): MacroSettings {
  const fanuc = (builtins.find((b) => b.name === "fanuc")?.macro ?? {}) as Loose;
  const osp = (builtins.find((b) => b.name === "osp")?.macro ?? {}) as Loose;
  const out: Loose = {};
  for (const f of MACRO_FIELDS) out[f] = structuredClone((ospSyntax(d) ? osp[f] : undefined) ?? fanuc[f]);
  return out as MacroSettings;
}
const ospSyntax = (d: DialectSettings) => (d.macro?.syntax ? d.macro.syntax === "osp" : (d.base ?? d.name) === "osp");
/** 表示時の基底の値（変えた項目の判定に使う） */
let symInherited: MacroSettings = {};
/** 画面のマクロ記号（全項目。行の検査で表を 1 行ずつ抜いて試すのに使う） */
let symShown: MacroSettings = {};

const opt = (pairs: [string, string][], v: string) => pairs.map(([k, l]) => `<option value="${esc(k)}"${k === v ? " selected" : ""}>${esc(l)}</option>`).join("");
const KIND_LABEL: [string, string][] = [["empty", "空（empty）"], ["local", "ローカル（local）"], ["common", "共通（common）"], ["retained", "保持（retained）"], ["system", "システム（system）"]];
const OP_LABEL: [string, string][] = [["add", "加算（add）"], ["sub", "減算（sub）"], ["mul", "乗算（mul）"], ["div", "除算（div）"], ["mod", "剰余（mod）"], ["and", "論理積（and）"], ["or", "論理和（or）"], ["xor", "排他的論理和（xor）"]];
const CMP_LABEL: [string, string][] = [["eq", "等しい（eq）"], ["ne", "等しくない（ne）"], ["gt", "より大きい（gt）"], ["lt", "より小さい（lt）"], ["ge", "以上（ge）"], ["le", "以下（le）"]];
const CTRL_LABEL: [string, string][] = [["if", "条件分岐（if）"], ["then", "条件付き代入（then）"], ["goto", "分岐（goto）"], ["while", "繰り返し（while）"], ["do", "繰り返しの始まり（do）"], ["end", "繰り返しの終わり（end）"], ["call", "呼び出し（call・OSP）"], ["rts", "呼び出しの終わり（rts・OSP）"], ["modin", "モーダル呼び出し（modin・OSP）"], ["modout", "モーダル呼び出しの解除（modout・OSP）"]];
const FN_LABEL: [string, string][] = ["sin", "cos", "tan", "asin", "acos", "atan", "sqrt", "abs", "round", "fix", "fup", "ln", "exp", "pow", "bin", "bcd", "adp"].map((f) => [f, f.toUpperCase()]);
const UNIT_LABEL: [string, string][] = [["degrees", "度"], ["radians", "ラジアン"]];
const PAIRS: [string, string][] = [["[]", "[ ]"], ["()", "( )"], ["{}", "{ }"]];
const msgCell = `<td class="mmsg"></td>`;

/** マクロ記号の表の 1 行（引数なし：新しい行） */
export const MACRO_ROWS: Record<MacroTable, (v?: Loose) => string> = {
  variables: (v = { prefix: "#", kind: "common", range: [0, 0], description: "" }) => {
    const range = v.range as [number, number] | undefined;
    return `<tr>${input("prefix", String(v.prefix), "short", "名前付き")}<td><select data-col="kind">${opt(KIND_LABEL, String(v.kind))}</select></td>${input("lo", range?.[0] ?? null, "short", "範囲なし")}${input("hi", range?.[1] ?? null, "short", "範囲なし")}${input("offset", (v.offset as number) || null, "short", "0")}${input("desc", String(v.description ?? ""), "wide")}${del}${msgCell}</tr>`;
  },
  operators: (v = { symbol: "", operation: "add", precedence: 4, description: "" }) =>
    `<tr>${input("symbol", String(v.symbol), "short")}<td><select data-col="operation">${opt(OP_LABEL, String(v.operation))}</select></td>${input("precedence", v.precedence as number, "short")}${input("desc", String(v.description ?? ""), "wide")}${del}${msgCell}</tr>`,
  comparisons: (v = { symbol: "", operation: "eq", precedence: 3, description: "" }) =>
    `<tr>${input("symbol", String(v.symbol), "short")}<td><select data-col="operation">${opt(CMP_LABEL, String(v.operation))}</select></td>${input("precedence", v.precedence as number, "short")}${input("desc", String(v.description ?? ""), "wide")}${del}${msgCell}</tr>`,
  controls: (v = { keyword: "", statement: "if", description: "" }) =>
    `<tr>${input("keyword", String(v.keyword), "short")}<td><select data-col="statement">${opt(CTRL_LABEL, String(v.statement))}</select></td>${input("desc", String(v.description ?? ""), "wide")}${del}${msgCell}</tr>`,
  functions: (v = { name: "", function: "sin", arity: [1, 1], angle_unit: "degrees", description: "" }) => {
    const [min, max] = v.arity as [number, number];
    return `<tr>${input("name", String(v.name), "short")}<td><select data-col="function">${opt(FN_LABEL, String(v.function))}</select></td>${input("min", min, "short")}${input("max", max, "short")}<td><select data-col="unit">${opt(UNIT_LABEL, String(v.angle_unit ?? "degrees"))}</select></td>${input("desc", String(v.description ?? ""), "wide")}${del}${msgCell}</tr>`;
  },
};
/** 表の見出し：先頭は表の題と注記、続いて列 */
const MACRO_HEADS: Record<MacroTable, [string, string][]> = {
  variables: [["変数", "番号の範囲と種類（例：#100〜#199 は共通変数）"], ["接頭辞", "例：#、VC。空：名前付き（OSP）"], ["種類", ""], ["番号の始め", "空：範囲なし"], ["番号の終わり", ""], ["ずらし", "内部の番号への加算"], ["説明", ""]],
  operators: [["演算子", "四則演算・剰余・ビット演算"], ["記号", "例：+、MOD"], ["演算", ""], ["優先順位", "1〜254。大きいほど先"], ["説明", ""]],
  comparisons: [["比較", "IF・WHILE の条件"], ["記号", "例：EQ、=="], ["比較", ""], ["優先順位", "1〜254"], ["説明", ""]],
  controls: [["制御文", "分岐・繰り返し・呼び出し"], ["語", "英大文字・数字・_"], ["文", ""], ["説明", ""]],
  functions: [["関数", "別名も可（例：SQR を平方根に）"], ["名前", "英大文字・数字・_"], ["関数", ""], ["引数の最少", ""], ["引数の最多", "ATAN は 2 まで"], ["角度", "三角関数のみ"], ["説明", ""]],
};
/** 検査で誤りになった項目・行の文言 */
const MACRO_HINT: Record<string, string> = {
  variables: "接頭辞・範囲の重なり・種類の組み合わせの誤り",
  operators: "記号の重複・形式、演算の種類、優先順位の誤り",
  comparisons: "記号の重複・形式、優先順位の誤り",
  controls: "語の重複・形式、構文に無い文、IF・THEN・WHILE・DO・END の組の誤り",
  functions: "名前の重複・形式、引数の個数の誤り",
  expression: "括弧の組の誤り", arguments: "括弧の組の誤り", assignment: "記号の形式の誤り",
  block_skip: "記号の形式・代入との重複", comment_pair: "式・引数の括弧との重複", line_comments: "記号の形式・重複",
};

function symbolsHtml(d: DialectSettings): string {
  symInherited = inheritedMacro(d);
  const m = { ...symInherited, ...d.macro } as Required<MacroSettings>;
  const pairSel = (col: string, v: string[], none = false) =>
    `<select data-mcol="${col}">${opt([...(none ? [["", "なし"] as [string, string]] : []), ...PAIRS], v.join(""))}</select>`;
  const text = (col: string, v: string, hint = "") => `<input data-mcol="${col}" value="${esc(v)}"${hint ? ` placeholder="${esc(hint)}"` : ""} spellcheck="false" />`;
  const scalar = (label: string, col: string, html: string) => `<label class="pf"><span>${label}</span>${html}<em class="mmsg" data-mmsg="${col}"></em></label>`;
  const table = (t: MacroTable) => {
    const [[title, note], ...cols] = MACRO_HEADS[t];
    return `<section class="psec"><h4>${title}<small>${note}</small></h4><table class="ptable dtable" data-mtable="${t}"><thead><tr>${cols.map(([h, sub]) => `<th>${h}${sub ? `<small>${sub}</small>` : ""}</th>`).join("")}<th></th><th>検査</th></tr></thead><tbody>${(m[t] as unknown as Loose[]).map((v) => MACRO_ROWS[t](v)).join("")}</tbody></table>
      <button class="pbtn" data-maddrow="${t}">＋ 行を追加</button></section>`;
  };
  return `<p class="pnote">マクロの記号の読み替え。変えていない項目は組み込みの ${ospSyntax(d) ? "OSP" : "FANUC"} の値。ヘルプ → マクロ一覧に同じ定義の一覧</p>
    <div class="pchecks"><button class="pbtn" data-mact="check" title="この画面の定義をコアで検査（誤りの行に印）">検査</button><button class="pbtn" data-mact="reset" title="このセットのマクロ記号の指定を消して組み込みの値へ">組み込みの値へ戻す</button><span class="mresult"></span></div>
    <section class="psec"><h4>括弧・代入・コメント</h4><div class="pgrid">
      ${scalar("式の括弧", "expression", pairSel("expression", m.expression))}
      ${scalar("引数の括弧", "arguments", pairSel("arguments", m.arguments))}
      ${scalar("代入", "assignment", text("assignment", m.assignment))}
      ${scalar("ブロックスキップ", "block_skip", text("block_skip", m.block_skip))}
      ${scalar("コメントの括弧", "comment_pair", pairSel("comment_pair", m.comment_pair, true))}
      ${scalar("行末コメント", "line_comments", text("line_comments", m.line_comments.join(" "), "なし（空白で区切って複数）"))}
    </div></section>
    ${MACRO_TABLES.map(table).join("")}`;
}

/** マクロ記号のページを作り直す。構文・基底を変えたとき（このセットで変えた項目は残す）と、組み込みの値へ戻すとき。 */
export function refreshSymbols(base: DialectSettings, body: HTMLElement, reset = false) {
  const { dialect } = collectDialect(base, body);
  if (reset) dialect.macro = dialect.macro?.syntax ? { syntax: dialect.macro.syntax } : {};
  const page = body.querySelector<HTMLElement>('[data-page="sym"]');
  if (page) page.innerHTML = symbolsHtml(dialect);
}

function readSymbols(d: DialectSettings, body: HTMLElement): string[] {
  const page = body.querySelector<HTMLElement>('[data-page="sym"]');
  if (!page) return [];
  const errs: string[] = [];
  page.querySelectorAll(".mmsg, .mresult").forEach((el) => (el.textContent = ""));
  page.querySelectorAll("tr.mbad").forEach((el) => el.classList.remove("mbad"));
  page.querySelectorAll(".bad").forEach((el) => el.classList.remove("bad"));
  const mval = (col: string) => page.querySelector<HTMLInputElement | HTMLSelectElement>(`[data-mcol="${col}"]`)!.value.trim();
  const full: Loose = {
    expression: [...mval("expression")],
    arguments: [...mval("arguments")],
    assignment: mval("assignment"),
    block_skip: mval("block_skip"),
    comment_pair: [...mval("comment_pair")],
    line_comments: mval("line_comments").split(/[\s,]+/).filter(Boolean),
  };
  for (const [col, label] of [["assignment", "代入"], ["block_skip", "ブロックスキップ"]]) {
    if (full[col]) continue;
    page.querySelector(`[data-mcol="${col}"]`)!.classList.add("bad");
    errs.push(`マクロ記号の${label}：必須`);
  }
  for (const t of MACRO_TABLES) {
    const list: Loose[] = [];
    for (const [i, tr] of page.querySelectorAll<HTMLTableRowElement>(`[data-mtable="${t}"] tbody tr`).entries()) {
      const cell = (col: string) => tr.querySelector<HTMLInputElement | HTMLSelectElement>(`[data-col="${col}"]`)!;
      const rowErrs: string[] = [];
      const text = (col: string, required: boolean) => {
        const v = cell(col).value.trim();
        if (required && !v) { cell(col).classList.add("bad"); rowErrs.push("必須"); }
        return v;
      };
      const num = (col: string, min: number, max: number, optional = false) => {
        const raw = cell(col).value.trim();
        const n = Number(raw);
        if (optional && raw === "") return null;
        if (raw === "" || !Number.isInteger(n) || n < min || n > max) { cell(col).classList.add("bad"); rowErrs.push(`${min}〜${max} の整数`); return 0; }
        return n;
      };
      const description = cell("desc").value.trim();
      if (t === "variables") {
        const lo = num("lo", 0, 0xffffffff, true), hi = num("hi", 0, 0xffffffff, true), offset = num("offset", 0, 0xffffffff, true) ?? 0;
        if ((lo === null) !== (hi === null)) { cell(lo === null ? "lo" : "hi").classList.add("bad"); rowErrs.push("番号の始めと終わりは両方とも指定か両方とも空"); }
        list.push({ prefix: text("prefix", false), kind: cell("kind").value, ...(lo !== null && hi !== null ? { range: [lo, hi] } : {}), ...(offset ? { offset } : {}), description });
      } else if (t === "operators" || t === "comparisons") {
        list.push({ symbol: text("symbol", true), operation: cell("operation").value, precedence: num("precedence", 1, 254), description });
      } else if (t === "controls") {
        list.push({ keyword: text("keyword", true), statement: cell("statement").value, description });
      } else {
        list.push({ name: text("name", true), function: cell("function").value, arity: [num("min", 0, 2), num("max", 0, 2)], angle_unit: cell("unit").value, description });
      }
      if (rowErrs.length) {
        tr.classList.add("mbad");
        tr.querySelector(".mmsg")!.textContent = `✕ ${rowErrs.join("・")}`;
        errs.push(`マクロ記号の${MACRO_HEADS[t][0][0]} ${i + 1} 行目：${rowErrs[0]}`);
      }
    }
    full[t] = list;
  }
  symShown = full as MacroSettings;
  // 表示時の基底の値から変えた項目だけをこのセットの指定に残す
  const macro: Loose = d.macro?.syntax ? { syntax: d.macro.syntax } : {};
  for (const f of [...MACRO_SCALARS, ...MACRO_TABLES]) {
    if (JSON.stringify(full[f]) !== JSON.stringify((symInherited as Loose)[f])) macro[f] = full[f];
  }
  d.macro = macro as MacroSettings;
  return errs;
}

/**
 * コアの検査（validate_dialect）の macro.項目 の誤りを、マクロ記号のページの行に印で出す。
 * 表は 1 行ずつ抜いて検査し直し、抜くと誤りが消える行を誤りの行とする（重複は両方の行に印）。
 * 誤りが無ければ「誤りなし」。
 */
export async function markMacroErrors(d: DialectSettings, errors: string[], body: HTMLElement) {
  const page = body.querySelector<HTMLElement>('[data-page="sym"]');
  if (!page) return;
  const result = page.querySelector(".mresult")!;
  result.textContent = errors.length ? "" : "✓ 誤りなし";
  result.classList.toggle("ok", !errors.length);
  for (const e of errors) {
    const field = /setting macro\.(\w+)$/.exec(e)?.[1];
    if (!field) continue;
    if (!(MACRO_TABLES as string[]).includes(field)) {
      page.querySelector(`[data-mcol="${field}"]`)?.classList.add("bad");
      const msg = page.querySelector(`[data-mmsg="${field}"]`);
      if (msg) msg.textContent = `✕ ${MACRO_HINT[field] ?? "誤り"}`;
      continue;
    }
    const t = field as MacroTable;
    const all = (symShown[t] ?? []) as unknown as Loose[];
    let found = false;
    for (const [i, tr] of page.querySelectorAll<HTMLTableRowElement>(`[data-mtable="${t}"] tbody tr`).entries()) {
      const trial = structuredClone(d);
      trial.macro = { ...trial.macro, [t]: all.filter((_, j) => j !== i) };
      if ((await invoke<string[]>("dialect_validate", { dialect: trial })).includes(e)) continue;
      found = true;
      tr.classList.add("mbad");
      tr.querySelector("input")?.classList.add("bad");
      tr.querySelector(".mmsg")!.textContent = `✕ ${MACRO_HINT[t]}`;
    }
    if (!found) {
      page.querySelector(`[data-mtable="${t}"] input`)?.classList.add("bad");
      result.textContent = `✕ ${MACRO_HEADS[t][0][0]}の表：${MACRO_HINT[t]}（足りない行の可能性。例：加算・減算は必須）`;
    }
  }
}
