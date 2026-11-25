// コード定義のページ（アドレス表・G コード表・M コード表・ユーザー定義・マクロ構文）の表示と読み取り。
// 方言（nclens-core の DialectSettings）はポスト（セット）の一部。画面の枠は sets.ts。
// 方言だけの書き出し・読み込み（TOML・JSON）は方言だけを他の PC へ渡す補助。検査はコアの validate_dialect。
import { invoke } from "./platform";
import { ADDRESS_ROLES, type CustomCode, type DialectSettings, type GCodeDefinition, type MCodeDefinition } from "./types";

const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
/** コード定義のページ */
export const DIALECT_TABS = ["addr", "g", "m", "custom", "macro"] as const;

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
const FIELD_LABEL: Record<string, string> = { name: "名前", base: "基底", addresses: "アドレス表", g_codes: "G コード表", m_codes: "M コード表", custom: "ユーザー定義" };

let builtins: DialectSettings[] = [];
let gFuncs: string[] = [];
let mFuncs: string[] = [];

/** 組み込みの方言（FANUC・OSP）。初回の起動で一覧の元にする。 */
export async function builtinDialects(): Promise<DialectSettings[]> {
  if (!builtins.length) {
    builtins = await invoke<DialectSettings[]>("dialect_builtin");
    // 機能の選択肢：組み込みの方言に出てくる名前すべてと、表に無い特別な名前
    gFuncs = [...new Set(["ignore", "unsupported", ...builtins.flatMap((b) => Object.values(b.g_codes).map((d) => d.function)), "length_dynamic"])];
    mFuncs = [...new Set(["ignore", "call", ...builtins.flatMap((b) => Object.values(b.m_codes).map((d) => d.function))])];
  }
  return structuredClone(builtins);
}

const funcOptions = (list: string[], v: string) =>
  [...new Set([...list, v])].map((f) => `<option value="${esc(f)}"${f === v ? " selected" : ""}>${esc(FUNC_LABEL[f] ? `${FUNC_LABEL[f]}（${f}）` : f)}</option>`).join("");
const input = (col: string, v: string | number | null, cls = "", hint = "") =>
  `<td><input data-col="${col}" class="${cls}" value="${esc(v === null ? "" : String(v))}"${hint ? ` placeholder="${esc(hint)}" title="空：${esc(hint)}"` : ""} spellcheck="false" /></td>`;

/** 表示中の方言（解説の列：空の行は基底の組み込みの方言の解説を薄く出す） */
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
  g: "G コードの機能・モーダル群（基底の方言への追加・上書き）",
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

/** 表示中の表 → 方言（行の並びのとおり。キーの重複は後の行が勝つので、検査で印を付ける） */
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
    if (m === "fanuc" || m === "osp") d.macro = m;
    else delete d.macro;
    const base = body.querySelector<HTMLSelectElement>("[data-base]")!.value;
    d.base = base || null;
  }
  return errs;
}

function macroHtml(d: DialectSettings) {
  const radio = (v: string, label: string, note: string) =>
    `<label class="pc"><input type="radio" name="dmacro" value="${v}"${(d.macro ?? "") === v ? " checked" : ""} />${label}<em>${note}</em></label>`;
  return `<section class="psec"><h4>マクロ構文</h4><div class="pchecks col">
      ${radio("fanuc", "FANUC", "カスタムマクロ B（#変数・IF・GOTO・WHILE）")}
      ${radio("osp", "OSP", "OSP ユーザータスク（VC 変数・IF・GOTO・CALL。暫定・基本範囲）")}
      ${radio("", "基底と同じ", "基底の方言の構文")}
    </div></section>
    <section class="psec"><h4>方言の名前<small>レポート表示用。セット名とは別</small></h4><div class="pgrid">
      <label class="pf"><span>名前</span><input data-dname value="${esc(d.name)}" spellcheck="false" /></label>
    </div><div class="pchecks">
      <button class="pbtn" data-dfile="import" title="方言のみファイルから読み込み（このセットの方言の置き換え）">方言の読み込み…</button>
      <button class="pbtn" data-dfile="export" title="このセットの方言のみファイルへ書き出し">方言の書き出し…</button>
    </div></section>
    <section class="psec"><h4>基底の方言<small>表に無いコード：基底の定義</small></h4><div class="pgrid">
      <label class="pf"><span>基底</span><select data-base>${["", ...builtins.map((b) => b.name)]
        .map((b) => `<option value="${b}"${(d.base ?? "") === b ? " selected" : ""}>${b ? b.toUpperCase() : "なし（この表のみ）"}</option>`).join("")}</select></label>
    </div></section>`;
}

/** コアの検査の文言（Invalid dialect setting g_codes.100）を画面の言葉に直す。 */
export function explain(msg: string) {
  const m = /setting\s+(\w+)(?:\.(.+))?$/.exec(msg);
  if (!m) return msg;
  return `${FIELD_LABEL[m[1]] ?? m[1]}${m[2] ? ` の ${m[2]}` : ""}：誤り`;
}

/** 方言のページ 5 つの HTML（ページ名 → 中身）。 */
export function renderDialect(d: DialectSettings): Record<(typeof DIALECT_TABS)[number], string> {
  shown = d;
  return {
    addr: tableHtml("addr", Object.entries(d.addresses).sort()),
    g: tableHtml("g", Object.entries(d.g_codes).sort(numSort)),
    m: tableHtml("m", Object.entries(d.m_codes).sort(numSort)),
    custom: tableHtml("custom", Object.entries(d.custom).sort()),
    macro: macroHtml(d),
  };
}

/** 方言のページ → 方言。誤りの文言の一覧を返す（名前の重複はセット側で見る）。 */
export function collectDialect(base: DialectSettings, body: HTMLElement): { dialect: DialectSettings; errors: string[] } {
  const d = structuredClone(base);
  const errors = DIALECT_TABS.flatMap((t) => readTab(d, t, body));
  const name = body.querySelector<HTMLInputElement>("[data-dname]");
  if (name) {
    const v = name.value.trim();
    name.classList.toggle("bad", !v);
    if (v) d.name = v;
    else errors.push("方言の名前：必須");
  }
  return { dialect: d, errors };
}
