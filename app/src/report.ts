// レポート画面：nclens-core の make_report の結果（合計・工具ごと・プログラムごと・範囲・F の最大と最小）。
// Rust が実行のたびに作って取っておく（last_report）。ダイアログを開いている間は再実行のたびに更新。
import { invoke, message, save } from "./platform";
import type { AxisRange, CoordinateBounds, MotionTotals, Report, ReportBreakdown } from "./types";
import { FloatWin } from "./floatwin";

const $ = <T extends HTMLElement = HTMLElement>(sel: string) => document.querySelector<T>(sel)!;
const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
const mm = (v: number) => v.toFixed(3);
const hms = (t: number) => {
  const sec = Math.round(t);
  return `${Math.floor(sec / 3600)}:${String(Math.floor(sec / 60) % 60).padStart(2, "0")}:${String(sec % 60).padStart(2, "0")}`;
};
const AXES = ["X", "Y", "Z", "A", "B", "C"];

let report: Report | null = null;
/** 今のファイルのパス（無題・見本は名前）を返す */
let fileName: () => string = () => "";

const totalTime = (t: MotionTotals) => t.cut_time_sec + t.rapid_time_sec + t.other_time_sec;

function totalsRow(name: string, t: MotionTotals, lines?: number) {
  return `<tr><td>${esc(name)}</td><td>${mm(t.cut_length_mm)}</td><td>${mm(t.rapid_length_mm)}</td><td>${hms(t.cut_time_sec)}</td><td>${hms(t.rapid_time_sec)}</td><td>${hms(t.other_time_sec)}</td><td><b>${hms(totalTime(t))}</b></td>${lines === undefined ? "" : `<td>${lines}</td>`}</tr>`;
}
const totalsHead = (first: string, lines = false) =>
  `<tr><th>${first}</th><th>切削 mm</th><th>早送り mm</th><th>切削時間</th><th>早送り時間</th><th>その他</th><th>合計時間</th>${lines ? "<th>行数</th>" : ""}</tr>`;

/** 回転軸は動いたときだけ */
const usedAxes = (r: AxisRange) => AXES.map((_, i) => i).filter((i) => i < 3 || r.min[i] !== 0 || r.max[i] !== 0);

function rangeTable(b: CoordinateBounds) {
  const axes = [...new Set([...usedAxes(b.work), ...usedAxes(b.machine)])].sort();
  const row = (name: string, r: AxisRange) => `<tr><td>${name}</td>${axes.map((i) => `<td class="ax-${AXES[i].toLowerCase()}">${mm(r.min[i])} 〜 ${mm(r.max[i])}</td>`).join("")}</tr>`;
  return `<table class="rtable"><thead><tr><th></th>${axes.map((i) => `<th>${AXES[i]}</th>`).join("")}</tr></thead><tbody>${row("ワーク座標", b.work)}${row("機械座標", b.machine)}</tbody></table>`;
}

function render() {
  const body = $("#report-body");
  if (!report) {
    body.innerHTML = `<div class="empty">実行結果がありません</div>`;
    return;
  }
  const r = report;
  $("#report-file").textContent = r.filename;
  const t = r.totals;
  const feed = r.feed_min === null || r.feed_max === null ? "—" : `${r.feed_min} 〜 ${r.feed_max} mm/min`;
  const tools = Object.entries(r.tools).sort((a, b) => Number(a[0]) - Number(b[0]));
  const progs = Object.entries(r.programs);
  body.innerHTML = `
    <section class="psec"><h4>合計</h4>
      <div class="rsum">
        <span><i>合計時間</i><b>${hms(totalTime(t))}</b></span>
        <span><i>切削</i><b>${mm(t.cut_length_mm)} mm</b><small>${hms(t.cut_time_sec)}</small></span>
        <span><i>早送り</i><b>${mm(t.rapid_length_mm)} mm</b><small>${hms(t.rapid_time_sec)}</small></span>
        <span><i>その他（休止・工具交換など）</i><b>${hms(t.other_time_sec)}</b></span>
        <span><i>F（切削送り）</i><b>${feed}</b></span>
        <span><i>エラー／警告</i><b>${r.errors} ／ ${r.warnings}</b></span>
        <span><i>ポスト／方言</i><b>${esc(r.post_name || "—")} ／ ${esc(r.dialect || "—")}</b></span>
        <span><i>作成</i><b>${new Date(r.generated_at_unix_seconds * 1000).toLocaleString("ja-JP")}</b></span>
      </div>
    </section>
    <section class="psec"><h4>範囲<small>プログラム全体</small></h4>${rangeTable(r.bounds)}</section>
    <section class="psec"><h4>工具ごと</h4>
      <table class="rtable"><thead>${totalsHead("工具", true)}</thead><tbody>${tools.map(([k, b]) => totalsRow(`T${k}`, b.totals, b.used_lines.length)).join("") || `<tr><td colspan="8">—</td></tr>`}</tbody></table>
      ${tools.map(([k, b]) => `<details><summary>T${k} の範囲</summary>${rangeTable(b.bounds)}</details>`).join("")}
    </section>
    <section class="psec"><h4>プログラムごと</h4>
      <table class="rtable"><thead>${totalsHead("プログラム", true)}</thead><tbody>${progs.map(([k, b]) => totalsRow(k, b.totals, b.used_lines.length)).join("") || `<tr><td colspan="8">—</td></tr>`}</tbody></table>
    </section>`;
}

/** 最後の実行結果のレポートを読み直す（ダイアログが開いているときだけ描き直す）。 */
export async function refreshReport() {
  const dlg = $<HTMLDialogElement>("#report-dialog");
  if (!dlg.open) return;
  try {
    report = await fetchReport();
    $("#report-state").textContent = "";
  } catch (e) {
    report = null;
    $("#report-state").textContent = `レポートの作成失敗：${e}`;
  }
  render();
}

let win: FloatWin | null = null;

export async function openReport() {
  win ??= new FloatWin($<HTMLDialogElement>("#report-dialog"), "report");
  win.open();
  await refreshReport();
}

const json = (r: Report) => JSON.stringify(r, null, 2);

/** 最後の実行結果のレポート（Rust の last_report）。ファイル名は今のファイル */
export async function fetchReport() {
  const r = await invoke<Report | null>("last_report");
  if (r) r.filename = fileName();
  return r;
}

/** JSON のコピー・保存。結果の文言を返す（空は取り消し） */
export async function reportAction(act: string, r: Report) {
  if (act === "copy") {
    try {
      await navigator.clipboard.writeText(json(r));
      return "JSON のコピー完了";
    } catch (err) {
      return `コピー失敗：${err}`;
    }
  }
  const stem = (r.filename.split(/[\\/]/).pop() ?? "").replace(/^見本：/, "").replace(/\.[^.]*$/, "") || "report";
  const path = await save({ defaultPath: `${stem}_report.json`, filters: [{ name: "JSON", extensions: ["json"] }] });
  if (!path) return "";
  try {
    await invoke("save_file", { path, text: json(r), encoding: "utf-8" });
    return `保存完了：${path}`;
  } catch (err) {
    await message(`保存失敗：${err}`, { title: "NC Lens", kind: "error" });
    return "";
  }
}

/** 行番号の一覧を区間にまとめる（1-5, 8） */
const spans = (lines: number[]) => {
  const out: string[] = [];
  const s = [...new Set(lines)].sort((a, b) => a - b);
  for (let i = 0; i < s.length; i++) {
    let j = i;
    while (j + 1 < s.length && s[j + 1] === s[j] + 1) j++;
    out.push(i === j ? `${s[i]}` : `${s[i]}-${s[j]}`);
    i = j;
  }
  return out.join(", ");
};
const rng = (r: AxisRange, i: number) => `${mm(r.min[i])} 〜 ${mm(r.max[i])}`;

/** 使用コード一覧の「集計」ページ：合計・工具ごと・プログラムごと・F・範囲・エラー。数値は右寄せ・等幅 */
export function summaryHtml(r: Report, dia: (t: string) => number | null | undefined) {
  const t = r.totals;
  const kv = (k: string, v: string, w = "") => `<tr><th>${k}</th><td class="num">${v}</td><td class="num">${w}</td></tr>`;
  const tools = Object.entries(r.tools).sort((a, b) => Number(a[0]) - Number(b[0]));
  const progs = Object.entries(r.programs);
  const b = r.bounds.work;
  const toolRow = ([k, x]: [string, ReportBreakdown]) => {
    const d = dia(k);
    return `<tr><td>T${esc(k)}</td><td class="num">${d == null ? "—" : mm(d)}</td><td class="num">${mm(x.totals.cut_length_mm)}</td><td class="num">${mm(x.totals.rapid_length_mm)}</td><td class="num">${hms(totalTime(x.totals))}</td>${[0, 1, 2].map((i) => `<td class="num">${rng(x.bounds.work, i)}</td>`).join("")}<td>${spans(x.used_lines)}</td></tr>`;
  };
  const progRow = ([k, x]: [string, ReportBreakdown]) =>
    `<tr><td>${esc(k)}</td><td class="num">${mm(x.totals.cut_length_mm)}</td><td class="num">${mm(x.totals.rapid_length_mm)}</td><td class="num">${hms(totalTime(x.totals))}</td><td>${spans(x.used_lines)}</td></tr>`;
  return `
    <div class="sum-acts"><button class="pbtn" data-ract="save">JSON 保存…</button><button class="pbtn" data-ract="copy">コピー</button><span class="sum-state"></span></div>
    <h5>合計</h5>
    <table class="stable"><thead><tr><th>項目</th><th>距離 mm</th><th>時間</th></tr></thead><tbody>
      ${kv("切削", mm(t.cut_length_mm), hms(t.cut_time_sec))}
      ${kv("早送り", mm(t.rapid_length_mm), hms(t.rapid_time_sec))}
      ${kv("その他", mm(t.other_length_mm), hms(t.other_time_sec))}
      ${kv("合計", mm(t.cut_length_mm + t.rapid_length_mm + t.other_length_mm), `<b>${hms(totalTime(t))}</b>`)}
    </tbody></table>
    <h5>F・診断</h5>
    <table class="stable"><tbody>
      ${kv("F 最小", r.feed_min === null ? "—" : String(r.feed_min), "mm/min")}
      ${kv("F 最大", r.feed_max === null ? "—" : String(r.feed_max), "mm/min")}
      ${kv("エラー", String(r.errors))}
      ${kv("警告", String(r.warnings))}
    </tbody></table>
    <h5>範囲（ワーク座標）</h5>
    <table class="stable"><tbody>${[0, 1, 2].map((i) => kv(AXES[i], rng(b, i))).join("")}</tbody></table>
    <h5>工具ごと</h5>
    <table class="stable"><thead><tr><th>工具</th><th>径</th><th>切削 mm</th><th>早送り mm</th><th>時間</th><th>X 範囲</th><th>Y 範囲</th><th>Z 範囲</th><th>使用行</th></tr></thead>
      <tbody>${tools.map(toolRow).join("") || `<tr><td>—</td></tr>`}</tbody></table>
    <h5>プログラムごと</h5>
    <table class="stable"><thead><tr><th>プログラム</th><th>切削 mm</th><th>早送り mm</th><th>時間</th><th>使用行</th></tr></thead>
      <tbody>${progs.map(progRow).join("") || `<tr><td>—</td></tr>`}</tbody></table>`;
}

export function initReport(name: () => string) {
  fileName = name;
  const dlg = $<HTMLDialogElement>("#report-dialog");
  dlg.querySelector("header")!.addEventListener("click", async (e) => {
    const act = (e.target as HTMLElement).closest<HTMLElement>("[data-ract]")?.dataset.ract;
    if (!report || !act) return;
    const msg = await reportAction(act, report);
    if (msg) $("#report-state").textContent = msg;
  });
}
