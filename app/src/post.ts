// ポストのページ（コード設定・変数設定・展開出力）の表示と読み取り。画面の枠は sets.ts（ポスト・コード定義）。
// 入力の検査はコアの PostProfile::validate と同じ規則。保存の前にコア側でもう一度検査する。
import type { PostProfile, Xyz } from "./types";

/** ポスト・ニーモニック・固有のファイル：TOML と JSON（拡張子で判別） */
export const SETTINGS_FILTERS = [{ name: "TOML（*.toml）", extensions: ["toml"] }, { name: "JSON（*.json）", extensions: ["json"] }];
const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

export const postFileName = (p: string) => p.split(/[\\/]/).pop() ?? p;

// ---- 入力の検査（文言を返す。正しければ ""） ----
const isNum = (s: string) => s.trim() !== "" && Number.isFinite(Number(s));
const isInt = (s: string) => isNum(s) && Number.isInteger(Number(s));
export const CHECKS: Record<string, (s: string) => string> = {
  text: (s) => (s.trim() ? "" : "必須"),
  free: () => "",
  num: (s) => (isNum(s) ? "" : "数値"),
  nn: (s) => (isNum(s) && Number(s) >= 0 ? "" : "0 以上の数値"),
  pos: (s) => (isNum(s) && Number(s) > 0 ? "" : "0 より大きい数値"),
  places: (s) => (isInt(s) && Number(s) >= 0 && Number(s) <= 9 ? "" : "0〜9 の整数"),
  mcount: (s) => (isInt(s) && Number(s) >= 1 ? "" : "1 以上の整数"),
  mcode: (s) => (isInt(s) && Number(s) >= 0 ? "" : "0 以上の整数"),
  gcode: (s) => (isNum(s) && Number(s) >= 0 ? "" : "0 以上の数値"),
  mprog: (s) => (isInt(s) && Number(s) >= 9001 && Number(s) <= 9009 ? "" : "9001〜9009"),
  gprog: (s) => (isInt(s) && Number(s) >= 9010 && Number(s) <= 9999 ? "" : "9010〜9999"),
  cvar: (s) => {
    const n = Number(s);
    return isInt(s) && ((n >= 100 && n <= 199) || (n >= 500 && n <= 999)) ? "" : "#100〜#199・#500〜#999";
  },
  tool: (s) => (isInt(s) && Number(s) >= 1 && Number(s) <= 999 ? "" : "1〜999"),
  glist: (s) => (list(s).every((x) => isNum(x) && Number(x) >= 0) ? "" : "0 以上の数値（カンマ区切り）"),
  mlist: (s) => (list(s).every((x) => isInt(x) && Number(x) >= 0) ? "" : "0 以上の整数（カンマ区切り）"),
};
const list = (s: string) => s.split(/[,\s]+/).filter((x) => x !== "");

// ---- 部品 ----
export const field = (label: string, key: string, value: string | number, check: string, unit = "") =>
  `<label class="pf"><span>${label}</span><input data-k="${key}" data-check="${check}" value="${esc(String(value))}" spellcheck="false" />${unit ? `<em>${unit}</em>` : ""}</label>`;
const select = (label: string, key: string, value: string, opts: [string, string][]) =>
  `<label class="pf"><span>${label}</span><select data-k="${key}">${opts.map(([v, t]) => `<option value="${v}"${v === value ? " selected" : ""}>${t}</option>`).join("")}</select></label>`;
const check = (label: string, key: string, value: boolean) =>
  `<label class="pc"><input type="checkbox" data-k="${key}"${value ? " checked" : ""} />${label}</label>`;
/** 選択肢と、選んだ項目の説明 1 行（.pdesc。切り替えは sets.ts）。tip は根拠の要約（ツールチップ） */
const choice = (label: string, key: string, value: string, tip: string, opts: [string, string, string][]) => {
  const cur = opts.find(([v]) => v === value) ?? opts[0];
  const desc = ([, t, d]: [string, string, string]) => esc(`${t}：${d}`);
  return `<div class="pdup" title="${esc(tip)}"><label class="pf"><span>${label}</span><select data-k="${key}" data-desc>${opts
    .map((o) => `<option value="${o[0]}" data-desc="${desc(o)}"${o[0] === value ? " selected" : ""}>${o[1]}</option>`).join("")}</select></label><small class="pdesc">${desc(cur)}</small></div>`;
};
const note = (html: string, tip: string, text: string) => `<div class="pdup" title="${esc(tip)}">${html}<small class="pdesc">${esc(text)}</small></div>`;
const cell = (col: string, value: string | number, chk: string) =>
  `<td><input data-col="${col}" data-check="${chk}" value="${esc(String(value))}" spellcheck="false" /></td>`;

interface TableDef { id: string; cols: [string, string, string][]; keyCheck?: string; addable: boolean }
export const TABLES: Record<string, TableDef> = {
  m_calls: { id: "m_calls", cols: [["key", "M コード", "mcode"], ["p", "プログラム O", "mprog"]], keyCheck: "mcode", addable: true },
  g_calls: { id: "g_calls", cols: [["key", "G コード", "gcode"], ["p", "プログラム O", "gprog"]], keyCheck: "gcode", addable: true },
  common: { id: "common", cols: [["key", "変数 #", "cvar"], ["v", "値", "num"]], keyCheck: "cvar", addable: true },
  offsets: {
    id: "offsets",
    cols: [["key", "補正番号", "tool"], ["h_geometry", "H 形状", "num"], ["h_wear", "H 摩耗", "num"], ["d_geometry", "D 形状", "num"], ["d_wear", "D 摩耗", "num"]],
    keyCheck: "tool",
    addable: true,
  },
  work: { id: "work", cols: [["name", "", ""], ["x", "X", "num"], ["y", "Y", "num"], ["z", "Z", "num"]], addable: false },
  ref: { id: "ref", cols: [["name", "", ""], ["x", "X", "num"], ["y", "Y", "num"], ["z", "Z", "num"]], addable: false },
};

function tableHtml(def: TableDef, rows: (string | number)[][], title: string, note = "") {
  const head = def.cols.map(([, h]) => `<th>${h}</th>`).join("") + (def.addable ? "<th></th>" : "");
  const body = rows.map((r) => rowHtml(def, r)).join("");
  const add = def.addable ? `<button class="pbtn" data-add="${def.id}">＋ 行の追加</button>` : "";
  return `<section class="psec"><h4>${title}${note ? `<small>${note}</small>` : ""}</h4>
    <table class="ptable" data-table="${def.id}"><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table>${add}</section>`;
}
export function rowHtml(def: TableDef, r: (string | number)[]) {
  const cells = def.cols.map(([col, , chk], i) => (chk ? cell(col, r[i], chk) : `<td class="pname" data-col="${col}">${r[i]}</td>`)).join("");
  return `<tr>${cells}${def.addable ? `<td><button class="pdel" title="行の削除">✕</button></td>` : ""}</tr>`;
}

export function renderPost(p: PostProfile) {
  const c = p.codes, v = p.variables, e = p.expand;
  const codes = `
    <section class="psec"><h4>速度・時間</h4><div class="pgrid">
      ${field("早送り速度", "codes.rapid_mm_min", c.rapid_mm_min, "pos", "mm/min")}
      ${field("最高回転数", "codes.max_rpm", c.max_rpm, "nn", "min⁻¹")}
      ${field("ATC 時間", "codes.atc_sec", c.atc_sec, "nn", "秒")}
      ${field("補正時間（1 行）", "codes.compensation_sec", c.compensation_sec, "nn", "秒")}
    </div></section>
    <section class="psec"><h4>固定サイクル・数値</h4><div class="pgrid">
      ${field("G73 戻り量", "codes.g73_retract_mm", c.g73_retract_mm, "nn", "mm")}
      ${field("G83 クリアランス", "codes.g83_clearance_mm", c.g83_clearance_mm, "nn", "mm")}
      ${field("小数桁数", "codes.decimal_places", c.decimal_places, "places")}
      ${select("小数点なし数値の単位", "codes.integer_units", c.integer_units, [["least_increment", "最小設定単位"], ["millimeters", "mm"]])}
    </div></section>
    <section class="psec"><h4>機能</h4><div class="pgrid">
      ${select("径補正タイプ", "codes.compensation", c.compensation, [["FANUC-A", "FANUC A"], ["FANUC-B", "FANUC B"], ["FANUC-C", "FANUC C"]])}
      ${select("工具交換", "codes.tool_change", c.tool_change, [["m6", "M6"], ["t", "T"]])}
      ${select("マクロ変数タイプ", "codes.macro_variables", c.macro_variables, [["15M", "15M"], ["16M", "16M"], ["6M", "6M"]])}
      ${select("プログラム番号アドレス", "codes.program_address", c.program_address, [["O", "O"], [":", ":"], ["L", "L"]])}
      ${field("無視 G コード", "codes.ignore_g", c.ignore_g.join(", "), "glist")}
      ${field("無視 M コード", "codes.ignore_m", c.ignore_m.join(", "), "mlist")}
    </div><div class="pchecks">
      ${check("T コード呼び出し（O9000・#149）", "codes.t_call", c.t_call)}
      ${check("S コード呼び出し（O9029・#147）", "codes.s_call", c.s_call)}
      ${check("B コード呼び出し（O9028・#146）", "codes.b_call", c.b_call)}
    </div></section>
    <section class="psec"><h4>同一アドレスの扱い<small>同じブロック内の重複指定</small></h4><div class="pgrid">
      ${choice("同一アドレスの複数記載", "codes.duplicate_word", c.duplicate_word,
        "FANUC：パラメータ 3403 で重複許可時は後の値。禁止時はアラーム（P/S 5074）。G65 の引数も同じ扱い（引数指定 II の I・J・K の組は別の変数）", [
        ["last", "後方優先", "最後の指定を採用。多くの機械の既定"],
        ["first", "先方優先", "最初の指定を採用。後の指定は無視"],
        ["error", "エラー", "ブロック全体を不採用。エラーとして表示"],
      ])}
      ${choice("同一モーダル群の G", "codes.duplicate_g_group", c.duplicate_g_group,
        "FANUC：同じ群の G の複数指定は最後の G が有効。パラメータ 3403 の重複禁止時は PS5074（G90・G91 などは例外）", [
        ["last", "後方優先", "同じ群の最後の G を採用。FANUC の既定"],
        ["first", "先方優先", "同じ群の最初の G を採用"],
        ["error", "エラー", "ブロック全体を不採用。エラーとして表示"],
      ])}
      ${note(field("1 ブロックの M の上限", "codes.max_m_per_block", c.max_m_per_block, "mcount", "個"),
        "FANUC：パラメータ 3404#7（M3B）で 1 個または最大 3 個。オークマ OSP：最大 8 個の機種あり",
        "上限：超過時は警告。採用は下の『複数の M の扱い』")}
      ${choice("複数の M の扱い", "codes.duplicate_m", c.duplicate_m,
        "FANUC：3404#7 の設定で 1 ブロック最大 3 個。機械側の同時使用の制約は対象外", [
        ["all", "すべて採用", "記載順にすべての M を出力"],
        ["last", "後方優先", "最後の M のみ採用"],
        ["first", "先方優先", "最初の M のみ採用"],
      ])}
      ${note(check("重複の警告", "codes.warn_on_duplicate", c.warn_on_duplicate),
        "警告の表示先：エラーのタブ（種別：警告）とエディタの該当行の波線",
        "重複時：エラーのタブとエディタの波線で通知。M の上限の超過は常に警告")}
    </div></section>
    <div class="pcols">
      ${tableHtml(TABLES.m_calls, Object.entries(c.m_calls).map(([k, n]) => [k, n]), "M コードマクロ呼び出し", "O9001〜O9009")}
      ${tableHtml(TABLES.g_calls, Object.entries(c.g_calls).map(([k, n]) => [k, n]), "G コードマクロ呼び出し", "O9010〜O9999")}
    </div>`;
  const xyz = (name: string, a: Xyz) => [name, ...a];
  const vars = `
    <div class="pcols">
      ${tableHtml(TABLES.work, [...v.work.map((w, i) => xyz(`G${54 + i}`, w)), xyz("外部（EXT）", v.external)], "ワーク座標系", "mm")}
      ${tableHtml(TABLES.ref, [xyz("G28", v.g28), ...v.g30.map((w, i) => xyz(`G30 P${i + 2}`, w))], "原点復帰位置", "機械座標 mm")}
    </div>
    ${tableHtml(TABLES.offsets, Object.entries(v.offsets).map(([k, o]) => [k, o.h_geometry, o.h_wear, o.d_geometry, o.d_wear]), "工具補正（H：長さ・D：径）", "mm")}
    ${tableHtml(TABLES.common, Object.entries(v.common).map(([k, n]) => [k, n]), "コモン変数初期値", "空の変数：記載なし")}`;
  const expand = `
    <section class="psec"><h4>展開出力</h4><div class="pchecks col">
      ${check("固定サイクル展開（G73・G81〜G89 → G0・G1）", "expand.cycles", e.cycles)}
      ${check("G10 絶対値出力", "expand.g10", e.g10)}
      ${check("小数点付加", "expand.decimal_point", e.decimal_point)}
      ${check("移動なし軸の省略", "expand.omit_zero_moves", e.omit_zero_moves)}
      ${check("オプショナルブロックスキップ（/）", "expand.block_skip", e.block_skip)}
    </div><div class="pgrid">
      ${select("座標指令", "expand.distance_mode", e.distance_mode, [["preserve", "元のまま"], ["absolute", "絶対値（G90）"], ["incremental", "増分値（G91）"]])}
    </div></section>`;
  return { codes, vars, expand };
}

/** 画面の中身 → PostProfile（ニーモニック・固有は base のまま）。誤りがあれば入力に印を付けて文言の一覧を返す。 */
export function collectPost(root: HTMLElement, base: PostProfile): { profile: PostProfile; errors: string[] } {
  const p: PostProfile = structuredClone(base);
  const errors: string[] = [];
  const mark = (el: HTMLInputElement, label: string) => {
    const err = CHECKS[el.dataset.check!]?.(el.value) ?? "";
    el.classList.toggle("bad", !!err);
    el.title = err;
    if (err) errors.push(`${label}：${err}`);
    return !err;
  };
  // 単独の項目
  root.querySelectorAll<HTMLInputElement | HTMLSelectElement>("[data-k]").forEach((el) => {
    const [sec, key] = el.dataset.k!.split(".") as [string, string];
    const label = el.closest("label")?.querySelector("span")?.textContent ?? el.closest("label")?.textContent ?? key;
    const obj = (sec === "post" ? p : (p as unknown as Record<string, Record<string, unknown>>)[sec]) as Record<string, unknown>;
    if (el instanceof HTMLInputElement && el.type === "checkbox") obj[key] = el.checked;
    else if (el instanceof HTMLSelectElement) obj[key] = el.value;
    else if (mark(el as HTMLInputElement, label)) {
      const chk = el.dataset.check!;
      obj[key] = chk === "text" || chk === "free" ? el.value : chk.endsWith("list") ? list(el.value).map(Number) : Number(el.value);
    }
  });
  // 表
  const rows = (id: string) =>
    [...root.querySelectorAll<HTMLTableRowElement>(`[data-table="${id}"] tbody tr`)].map((tr) => {
      const r: Record<string, string> = {};
      tr.querySelectorAll<HTMLInputElement>("input[data-col]").forEach((el) => {
        const title = TABLES[id].cols.find(([c]) => c === el.dataset.col)![1];
        if (mark(el, `${title}`)) r[el.dataset.col!] = el.value;
      });
      return { tr, r };
    });
  const keyed = <T>(id: string, val: (r: Record<string, string>) => T): Record<string, T> => {
    const out: Record<string, T> = {};
    for (const { tr, r } of rows(id)) {
      if (r.key === undefined || Object.keys(r).length < TABLES[id].cols.length) continue;
      const key = String(Number(r.key));
      const keyEl = tr.querySelector<HTMLInputElement>('input[data-col="key"]')!;
      if (key in out) {
        keyEl.classList.add("bad");
        keyEl.title = "重複";
        errors.push(`${TABLES[id].cols[0][1]} ${key}：重複`);
        continue;
      }
      out[key] = val(r);
    }
    return out;
  };
  p.codes.m_calls = keyed("m_calls", (r) => Number(r.p));
  p.codes.g_calls = keyed("g_calls", (r) => Number(r.p));
  p.variables.common = keyed("common", (r) => Number(r.v));
  p.variables.offsets = keyed("offsets", (r) => ({ h_geometry: +r.h_geometry, h_wear: +r.h_wear, d_geometry: +r.d_geometry, d_wear: +r.d_wear }));
  const xyz = (id: string) => rows(id).map(({ r }) => [Number(r.x ?? 0), Number(r.y ?? 0), Number(r.z ?? 0)] as Xyz);
  const work = xyz("work");
  p.variables.work = work.slice(0, 6) as PostProfile["variables"]["work"];
  p.variables.external = work[6];
  const ref = xyz("ref");
  p.variables.g28 = ref[0];
  p.variables.g30 = ref.slice(1, 4) as PostProfile["variables"]["g30"];
  return { profile: p, errors };
}
