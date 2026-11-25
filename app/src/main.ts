// 画面の組み立て。表示と入力の受け渡しだけを行い、NC の処理は Rust（nclens-core）に任せる。
import { appReady, desktopWindow as win, invoke, isDesktop, message, open, openDir, openExternal, resetDesktopWindow, save, splashStatus } from "./platform";
import { start } from "./startup";
import { createEditor } from "./editor";
import { PathViewer, toolColor, type Layout, type ViewName } from "./viewer";
import { Player, type PlayPos } from "./player";
import { highlightNc } from "./nc-lang";
import { postFileName } from "./post";
import { openSetDialog } from "./sets";
import { openAppSettings, refreshAppSettings, type AppHooks, type ZoomArea } from "./appsettings";
import { settings, place, loadSettings, save as saveSettings, flush as flushSettings, resetView, pushRecent, type PostSet, type ThemeMode } from "./settings";
import { ModalPanel } from "./modal";
import { initReport, openReport, refreshReport } from "./report";
import { builtinDialects } from "./dialect";
import { explainAt, helpDom, setHelpSource } from "./codehelp";
import { CodePeek } from "./peek";
import { openHelp } from "./help";
import { LINKS } from "./links";
import { bindRunSwitches } from "./breaks";
import { applyLayout, bindColumns, tableHtml, type Col } from "./coltable";
import { initMobile, revealNc, setNarrowLayout } from "./mobile";
import type { DialectSettings, ExpandedLine, PostProfile, RunResult } from "./types";
import sample from "../samples/bolt-circle-pocket.NCD?raw";

const $ = <T extends HTMLElement = HTMLElement>(sel: string) => document.querySelector<T>(sel)!;
const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
const root = document.documentElement.style;
const fmt = (v: number) => v.toFixed(3);
const hms = (t: number) => {
  const sec = Math.round(t);
  return `${Math.floor(sec / 3600)}:${String(Math.floor(sec / 60) % 60).padStart(2, "0")}:${String(sec % 60).padStart(2, "0")}`;
};

// ---- アプリの設定（nclens.settings.json）を最初に読む。テーマ・倍率・分け方などはここから ----
splashStatus("設定の読み込み");
await loadSettings();
splashStatus("エディタと描画の準備");

// ---- テーマ：自動（OS に従う）・ライト・ダーク ----
// 描画と色分けは style.css の CSS 変数を読むので、エディタと描画を作る前に決めておく
const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");
const effectiveTheme = () => (settings.theme === "auto" ? (darkQuery.matches ? "dark" : "light") : settings.theme);
document.documentElement.dataset.theme = effectiveTheme();

let timer = 0;
let runVersion = 0;
let result: RunResult | null = null;

// ---- ファイルの状態 ----
type Encoding = "cp932" | "utf-8";
const file = {
  path: null as string | null,
  name: "見本：bolt-circle-pocket.NCD",
  encoding: "cp932" as Encoding,
  eol: "\r\n",
  dirty: false,
};
let loading = false;

// ---- ポスト・コード定義のセット：一覧と、実行に使うセット（既定）は設定ファイルに残す ----
/** 実行に使うセット */
const currentSet = (): PostSet | undefined => settings.posts.find((s) => s.profile.name === settings.currentPost) ?? settings.posts[0];

/** 実行に渡すポスト（方言を含む） */
const runPost = (): PostProfile | null => currentSet()?.profile ?? null;
/** スキップの番号：アプリ設定の選択を優先、未選択はセットの既定 */
const postLevels = () => runPost()?.expand.block_skip_levels ?? [1];
const pickedLevels = () => [...new Set(settings.blockSkipLevels ?? postLevels())].sort((a, b) => a - b);
/** 実行に使う番号（ブロックスキップ OFF は空） */
const skipLevels = () => (settings.blockSkip ?? runPost()?.expand.block_skip ?? false) ? pickedLevels() : [];

function showPost() {
  const s = currentSet();
  $("#st-post").textContent = `ポスト ${s ? `${s.profile.name}${s.path ? `（${postFileName(s.path)}）` : ""}` : "既定"}`;
}

/** セットの一覧を用意する。初回は旧版の方言の一覧（無ければ組み込みの FANUC・OSP）と最後のポストから作る。 */
async function loadSets() {
  if (settings.posts.length) {
    // 旧版の設定のセットに無い項目（重複指定ほか）は既定の値で補う
    const base = await invoke<PostProfile>("post_default").catch(() => null);
    if (base) for (const s of settings.posts) s.profile.codes = { ...base.codes, ...s.profile.codes };
    return;
  }
  try {
    const base = await invoke<PostProfile>("post_default");
    const dialects = settings.dialects?.length ? settings.dialects : await builtinDialects();
    settings.posts = dialects.map((d) => ({ profile: { ...structuredClone(base), name: d.name, dialect: structuredClone(d) }, path: null }));
    settings.currentPost = settings.defaultDialect && dialects.some((d) => d.name === settings.defaultDialect) ? settings.defaultDialect : dialects[0]?.name ?? "";
    if (settings.lastPost) {
      const s = await loadPostFile(settings.lastPost);
      if (s) settings.currentPost = s.profile.name;
    }
  } catch { /* 既定のポストのまま */ }
  delete settings.dialects;
  delete settings.defaultDialect;
  delete settings.lastPost;
  saveSettings();
}

/** ポストのファイル（TOML・JSON）を読み、一覧へ入れる（同じファイルのセットがあれば中身を入れ替える）。 */
async function loadPostFile(path: string): Promise<PostSet | null> {
  try {
    const profile = await invoke<PostProfile>("post_load", { path });
    let s = settings.posts.find((x) => x.path?.toLowerCase() === path.toLowerCase());
    if (s) {
      if (settings.currentPost === s.profile.name) settings.currentPost = profile.name;
      s.profile = profile;
    } else {
      let name = profile.name.trim() || postFileName(path), k = 2;
      const base = name;
      while (settings.posts.some((x) => x.profile.name === name)) name = `${base} ${k++}`;
      s = { profile: { ...profile, name }, path };
      settings.posts.push(s);
    }
    return s;
  } catch (e) {
    $("#run-state").textContent = `ポストを読めません：${e}`;
    return null;
  }
}

async function postSettings() {
  const r = await openSetDialog({ sets: settings.posts, current: currentSet()?.profile.name ?? "" });
  if (!r) return;
  settings.posts = r.sets;
  settings.currentPost = r.current;
  for (const s of r.sets) if (s.path && s.profile.name === r.current) pushRecent(settings.recent.posts, s.path);
  saveSettings();
  showPost();
  showRecent();
  await runNc(true);
}

// ---- 同梱のサンプル：ファイル → サンプルを開く（メニューを開くたびに一覧を読み直す） ----
const samplesDir = isDesktop ? await invoke<string | null>("samples_dir").catch(() => null) : null;
async function showSamples() {
  const list = await invoke<{ name: string; path: string }[]>("samples_list").catch(() => []);
  let folder = "";
  $("#sample-files").innerHTML = list.map((s, i) => {
    const cut = s.name.lastIndexOf("/");
    const dir = cut < 0 ? "" : s.name.slice(0, cut);
    const head = dir !== folder ? `${i ? "<hr />" : ""}<div class="menu-note">${esc(dir)}</div>` : "";
    folder = dir;
    return `${head}<button data-sample="${i}" title="${esc(s.path)}">${esc(s.name.slice(cut + 1))}</button>`;
  }).join("") || `<div class="menu-note">サンプルがありません${samplesDir ? "" : "（samples フォルダが見つかりません）"}</div>`;
  $("#sample-files").querySelectorAll<HTMLElement>("[data-sample]").forEach((b) =>
    b.addEventListener("click", () => { closeMenus(); void openFile(list[Number(b.dataset.sample)].path); }));
}
if (isDesktop) {
  $("#sample-menu .sub-title").addEventListener("click", (e) => e.stopPropagation());
  $("#sample-menu").closest(".menu")!.querySelector(".menu-title")!.addEventListener("click", () => void showSamples());
} else $("#sample-menu").remove();

// ---- 最近使ったファイル・ポスト（メニューの下に並べる） ----
function showRecent() {
  const item = (cmd: string, p: string, i: number) =>
    `<button data-recent="${cmd}" data-i="${i}" title="${esc(p)}">${i + 1}. ${esc(baseName(p))}<kbd>${esc(p.length > 40 ? `…${p.slice(-38)}` : p)}</kbd></button>`;
  const f = settings.recent.files, ps = settings.recent.posts;
  $("#recent-files").innerHTML = f.length ? `<hr /><div class="menu-note">最近使ったファイル</div>${f.map((p, i) => item("file", p, i)).join("")}` : "";
  $("#recent-posts").innerHTML = ps.length ? `<hr /><div class="menu-note">最近使ったポスト</div>${ps.map((p, i) => item("post", p, i)).join("")}` : "";
}
document.addEventListener("click", async (e) => {
  const b = (e.target as HTMLElement).closest<HTMLElement>("[data-recent]");
  if (!b) return;
  closeMenus();
  const kind = b.dataset.recent, i = Number(b.dataset.i);
  if (kind === "file") return void openFile(settings.recent.files[i]);
  const s = await loadPostFile(settings.recent.posts[i]);
  if (!s) return;
  settings.currentPost = s.profile.name;
  pushRecent(settings.recent.posts, s.path!);
  showPost();
  showRecent();
  await runNc(true);
});

const editor = createEditor(
  $("#editor"),
  () => {
    if (!loading && !file.dirty) { file.dirty = true; showFile(); }
    clearTimeout(timer);
    timer = window.setTimeout(() => runNc(true), 500);
  },
  (line) => {
    $("#st-cursor").textContent = `行 ${line}`;
    viewer.highlightLine(line);
    if (!loading && result) showModalForLine(line);
  },
  // コードの説明：その行から出た最初の展開行の、実行後のモーダルを添える
  (line, text, ch) => {
    const i = firstRow.get(line);
    const h = explainAt(text, ch, i === undefined ? null : result?.expanded[i]?.modal ?? null);
    return h && { from: h.from, to: h.to, dom: helpDom(h.help) };
  },
);

const viewer = new PathViewer(
  $("#viewer"),
  (line) => { revealNc(); editor.focusLine(line); },
  (text) => ($("#coord").textContent = text),
  (layout, active, maximized) => showLayout(layout, active, maximized),
);

function applyTheme() {
  const theme = effectiveTheme();
  document.documentElement.dataset.theme = theme;
  editor.setDark(theme === "dark");
  viewer.setTheme();
  if (result) onPlay(player.at(player.time));
  refreshAppSettings();
}

function setTheme(mode: ThemeMode) {
  settings.theme = mode;
  saveSettings();
  applyTheme();
}
darkQuery.addEventListener("change", () => { if (settings.theme === "auto") applyTheme(); });

// ---- モーダルパネル：カーソル行・再生位置・展開結果の行の、実行後のモーダル状態 ----
const modal = new ModalPanel($("#modal-panel"), () => settings.modal, () => {
  saveSettings();
  refreshAppSettings();
});
/** 元の行 → その行から出た最初の展開行 */
let firstRow = new Map<number, number>();

function showModalForLine(line: number) {
  const i = firstRow.get(line);
  modal.show(i === undefined ? null : result!.expanded[i], i === undefined ? `行 ${line}（実行されない行）` : `カーソル 行 ${line}`);
}
function showModalForRow(i: number, source: string) {
  const row: ExpandedLine | undefined = result?.expanded[i];
  modal.show(row ?? null, source);
}

// ---- 再生 ----
let playLine = -1, playRow = -1;
const player = new Player(onPlay, (dir) => {
  document.querySelectorAll<HTMLElement>("#play [data-dir]").forEach((b) => b.classList.toggle("on", Number(b.dataset.dir) === dir));
});
const applyRunSwitches = bindRunSwitches(viewer, player, () => runPost()?.expand.block_skip ?? false, pickedLevels, () => void runNc(true),
  () => openAppSettings(appHooks, "behavior"));
const peek = new CodePeek(editor, (rows) => viewer.setCodeRows(rows), () => setTab("expanded"));
peek.toolDia = (t) => runPost()?.variables.tools[t]?.diameter_mm;

function onPlay(p: PlayPos) {
  const segs = result?.path ?? [];
  const total = player.total;
  $("#play-time").textContent = `${hms(p.t)} / ${hms(total)}`;
  $<HTMLInputElement>("#seek").value = String(total > 0 ? Math.round((p.t / total) * 10000) : 10000);
  if (player.stoppedAt) {
    const { kind, src_line, expanded_index } = player.stoppedAt;
    viewer.setProgress(p.k, p.frac);
    setTab("expanded");
    markPlay(src_line, expanded_index);
    $("#tab-expanded").querySelector("tr.play")?.scrollIntoView({ block: "nearest" });
    showModalForRow(expanded_index, `${kind} で停止 行 ${src_line}`);
    $("#st-play").textContent = `${kind} で停止　行 ${src_line}　${hms(p.t)}`;
    return;
  }
  if (p.k >= segs.length) {
    // 末尾：全体を描く（工具は終点）
    viewer.setProgress(segs.length, 0);
    markPlay(-1, -1);
    $("#st-play").textContent = segs.length ? `再生 末尾  ${hms(p.t)}` : "";
    return;
  }
  viewer.setProgress(p.k, p.frac);
  const s = segs[p.k];
  const pos = s.from.map((a, i) => a + (s.to[i] - a) * p.frac);
  $("#st-play").textContent = `再生 X ${fmt(pos[0])} Y ${fmt(pos[1])} Z ${fmt(pos[2])}  行 ${s.src_line}  ${hms(p.t)}`;
  markPlay(s.src_line, s.expanded_index);
  // 再生中・途中で止めているときは、再生位置の行のモーダル
  showModalForRow(s.expanded_index, `再生 行 ${s.src_line}`);
}

/** 再生位置の行を、エディタと展開結果で強調（変わったときだけ）。 */
function markPlay(line: number, row: number) {
  if (line !== playLine) {
    playLine = line;
    editor.markLine(line > 0 ? line : null);
  }
  if (row !== playRow) {
    $("#tab-expanded").querySelector("tr.play")?.classList.remove("play");
    playRow = row;
    const tr = row >= 0 ? ($("#tab-expanded").querySelector("tbody")?.children[row] as HTMLElement | undefined) : undefined;
    if (tr) {
      tr.classList.add("play");
      if ($("#tab-expanded").classList.contains("active")) tr.scrollIntoView({ block: "nearest" });
    }
  }
}

async function runNc(keepView = false) {
  clearTimeout(timer);
  player.stop();
  applyRunSwitches();
  const version = ++runVersion;
  const text = editor.text();
  const profile = runPost();
  $("#run-state").textContent = "実行中…";
  const t0 = performance.now();
  try {
    const r = await invoke<RunResult>("run_nc", { text, post: profile, skipLevels: skipLevels() });
    if (version !== runVersion || text !== editor.text()) return;
    result = r;
    firstRow = new Map();
    r.expanded.forEach((e, i) => { if (!firstRow.has(e.src_line)) firstRow.set(e.src_line, i); });
    modal.setBounds(r.bounds ?? null, r.expanded);
    viewer.setPath(r.path, keepView);
    player.load(r.path, r.stops);
    editor.markLine(null);
    playLine = playRow = -1;
    editor.setErrors(r.diagnostics);
    editor.setWarnings(dupWarnings(r));
    showResult(r);
    refreshAppSettings();
    const dialect = profile?.dialect;
    const builtins = await builtinDialects().catch(() => []);
    if (version !== runVersion) return;
    const base = builtins.find((entry) => entry.name === (dialect?.base ?? dialect?.name ?? "fanuc"));
    peek.load(r, [base, dialect].filter((entry) => entry !== undefined));
    onPlay(player.at(player.time));
    $("#run-state").textContent = `実行 ${Math.round(performance.now() - t0)} ms`;
    if (player.time >= player.total) showModalForLine(editor.view.state.doc.lineAt(editor.view.state.selection.main.head).number);
    void refreshReport();
  } catch (e) {
    if (version === runVersion) $("#run-state").textContent = `実行できません：${e}`;
  }
}

/** 同一アドレスの重複の警告（ポストの『コード設定』の重複指定） */
const dupWarnings = (r: RunResult) => (r.warnings ?? []).filter((w) => /^(Duplicate |Multiple M words|M word count)/.test(w.message));

function showResult(r: RunResult) {
  // 展開結果（展開後の列はエディタと同じ色分け）
  $("#n-expanded").textContent = String(r.expanded.length);
  $("#tab-expanded").innerHTML = r.expanded.length
    ? table("expanded", [{ key: "n", label: "#" }, { key: "line", label: "元の行" }, { key: "call", label: "呼び出し" }, { key: "text", label: "展開後", wide: true }],
      r.expanded.map((e, i) => [String(i + 1), String(e.src_line), esc(e.call_stack.join(" › ")), highlightNc(e.text)]), r.expanded.map((e) => e.src_line))
    : empty("展開結果はありません");
  // 変数の追跡
  $("#n-vars").textContent = String(r.var_trace.length);
  $("#tab-vars").innerHTML = r.var_trace.length
    ? table("vars", [{ key: "line", label: "元の行" }, { key: "var", label: "変数" }, { key: "value", label: "値" }],
      r.var_trace.map((v) => [String(v.src_line), `#${v.var}`, v.value === null ? "（空）" : String(v.value)]), r.var_trace.map((v) => v.src_line))
    : empty("変数の変化はありません");
  // エラーと重複の警告（行の順）
  const issues = [...r.diagnostics.map((d) => ({ ...d, kind: "エラー" })), ...dupWarnings(r).map((d) => ({ ...d, kind: "警告" }))]
    .sort((a, b) => a.src_line - b.src_line);
  $("#n-errors").textContent = String(issues.length);
  $("#n-errors").classList.toggle("bad", r.diagnostics.length > 0);
  $("#tab-errors").innerHTML = issues.length
    ? table("errors", [{ key: "line", label: "行" }, { key: "kind", label: "種別" }, { key: "msg", label: "内容", wide: true }], issues.map((d) => [String(d.src_line), d.kind, esc(d.message)]), issues.map((d) => d.src_line))
    : empty("エラー・警告はありません");
  document.querySelectorAll<HTMLTableElement>("#tab-body table.ctable").forEach(applyLayout);
  // ステータスバー
  const s = r.stats;
  $("#st-cut").textContent = fmt(s.cut_length_mm);
  $("#st-rapid").textContent = fmt(s.rapid_length_mm);
  $("#st-time").textContent = hms(s.time_sec);
  $("#st-range").textContent = ["X", "Y", "Z"].map((a, i) => `${a} ${fmt(s.min[i])}〜${fmt(s.max[i])}`).join("  ");
  $("#st-segs").textContent = `線分 ${r.path.length}`;
  // 凡例（使われた工具）
  const tools = [...new Set(r.path.filter((p) => p.kind === "Cut").map((p) => p.tool_index))].sort((a, b) => a - b);
  $("#legend").innerHTML =
    `<span><i class="dash"></i>早送り</span>` +
    tools.map((t) => `<span><i style="background:${toolColor(t)}"></i>切削 工具 ${t + 1}</span>`).join("");
}

/** 下部の表。列の順・表示・幅は coltable.ts（表ごとに設定ファイルへ） */
function table(id: string, cols: Col[], rows: string[][], lines: number[]) {
  return tableHtml(id, cols, rows, lines);
}
const empty = (msg: string) => `<div class="empty">${msg}</div>`;

// ---- コードの説明（ツールチップ）：使用中のセットの方言の解説。展開結果の表は語の上に 0.35 秒置くと出す ----
let helpBuiltins: DialectSettings[] = [];
void builtinDialects().then((b) => (helpBuiltins = b)).catch(() => {});
setHelpSource(() => ({ dialect: runPost()?.dialect ?? null, builtins: helpBuiltins }));
const codeTip = Object.assign(document.createElement("div"), { id: "code-tip", hidden: true });
document.body.append(codeTip);
let tipTimer = 0, tipSpan: Element | null = null;
const hideTip = () => { clearTimeout(tipTimer); tipSpan = null; codeTip.hidden = true; };
$("#tab-expanded").addEventListener("mouseover", (e) => {
  const span = (e.target as HTMLElement).closest("td span");
  if (span === tipSpan) return;
  hideTip();
  tipSpan = span;
  if (!span) return;
  tipTimer = window.setTimeout(() => {
    const row = Number(span.closest<HTMLElement>("tr[data-i]")?.dataset.i);
    const h = explainAt(span.textContent ?? "", 0, result?.expanded[row]?.modal ?? null);
    if (!h) return;
    codeTip.replaceChildren(helpDom(h.help));
    codeTip.hidden = false;
    const r = span.getBoundingClientRect();
    codeTip.style.left = `${Math.max(4, Math.min(r.left, window.innerWidth - codeTip.offsetWidth - 4))}px`;
    codeTip.style.top = `${r.top - codeTip.offsetHeight - 4 >= 0 ? r.top - codeTip.offsetHeight - 4 : r.bottom + 4}px`;
  }, 350);
});
$("#tab-expanded").addEventListener("mouseleave", hideTip);
$("#tab-body").addEventListener("scroll", hideTip, true);

// ---- ファイル：新規・開く・保存 ----
const NC_FILTERS = [{ name: "NC（*.NCD・*.NC・*.MIN・*.nc・*.txt）", extensions: ["NCD", "NC", "MIN", "nc", "txt"] }, { name: "すべて", extensions: ["*"] }];

function showFile() {
  const title = `${file.dirty ? "● " : ""}${file.name} - NC Lens`;
  $("#file-name").textContent = `${file.dirty ? "● " : ""}${file.path ?? file.name}`;
  $("#st-enc").textContent = `${file.encoding === "cp932" ? "CP932" : "UTF-8"} ${file.eol === "\r\n" ? "CRLF" : "LF"}`;
  document.title = title;
  if (win) win.setTitle(title).catch(() => {});
  else document.title = title;
}

function setDoc(text: string) {
  loading = true;
  editor.setText(text);
  loading = false;
  file.dirty = false;
  showFile();
}

const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p;

/** 未保存の変更があれば確かめる。続けてよければ true。 */
async function confirmDiscard(): Promise<boolean> {
  if (!file.dirty) return true;
  const yes = "保存", no = "保存しない";
  const r = await message(`「${file.name}」への変更を保存しますか？`, {
    title: "NC Lens",
    kind: "warning",
    buttons: { yes, no, cancel: "キャンセル" },
  });
  if (r === yes || r === "Yes") return saveFile(false);
  return r === no || r === "No";
}

async function newFile() {
  if (!(await confirmDiscard())) return;
  Object.assign(file, { path: null, name: "無題", encoding: "cp932", eol: "\r\n" });
  setDoc("");
  await runNc();
}

async function openFile(given?: string) {
  if (!(await confirmDiscard())) return;
  const path = given ?? (await open({ multiple: false, filters: NC_FILTERS, defaultPath: settings.defaultDir || samplesDir || undefined }));
  if (!path) return;
  try {
    const f = await invoke<{ text: string; encoding: Encoding }>("open_file", { path });
    // 改行は最初に見つかったものに合わせる（無ければ CRLF）
    const m = /\r\n|\n|\r/.exec(f.text);
    const nativePath = typeof path === "string" ? path : null;
    Object.assign(file, { path: nativePath, name: typeof path === "string" ? baseName(path) : path.name, encoding: f.encoding, eol: m ? m[0] : "\r\n" });
    setDoc(f.text);
    if (nativePath) pushRecent(settings.recent.files, nativePath);
    showRecent();
    await runNc();
  } catch (e) {
    $("#run-state").textContent = `開けません：${e}`;
  }
}

async function writeText(path: string, text: string, encoding: Encoding) {
  await invoke("save_file", { path, text, encoding });
}

/** 上書き保存（as なら名前を付けて保存）。保存できたら true。 */
async function saveFile(as: boolean): Promise<boolean> {
  let path = file.path;
  if (as || !path) {
    const p = await save({ defaultPath: file.path ?? (file.name === "無題" ? "新規.NCD" : file.name.replace(/^見本：/, "")), filters: NC_FILTERS });
    if (!p) return false;
    path = p;
  }
  try {
    await writeText(path, editor.text().replace(/\n/g, file.eol), file.encoding);
    Object.assign(file, { path: isDesktop ? path : null, name: baseName(path), dirty: false });
    showFile();
    if (isDesktop) pushRecent(settings.recent.files, path);
    showRecent();
    $("#run-state").textContent = `保存しました：${path}`;
    return true;
  } catch (e) {
    await message(`保存できません：${e}`, { title: "NC Lens", kind: "error" });
    return false;
  }
}

/** 展開結果をストレートパスとして保存。文字コードと改行は今のファイルに合わせる。 */
async function saveExpanded() {
  if (!result?.expanded.length) return void (await message("展開結果がありません", { title: "NC Lens" }));
  const stem = file.name.replace(/^見本：/, "").replace(/\.[^.]*$/, "");
  const path = await save({ defaultPath: `${stem}_展開.NCD`, filters: NC_FILTERS });
  if (!path) return;
  try {
    await writeText(path, expandedText(result.expanded.map((_, i) => i)) + file.eol, file.encoding);
    $("#run-state").textContent = `展開結果を保存しました：${path}`;
  } catch (e) {
    await message(`保存できません：${e}`, { title: "NC Lens", kind: "error" });
  }
}

// ---- 展開結果のコピーと選択 ----
const expandedText = (rows: number[], eol = file.eol) => rows.map((i) => result!.expanded[i].text).join(eol);

function selectedRows() {
  return [...$("#tab-expanded").querySelectorAll<HTMLElement>("tr.sel")].map((tr) => Number(tr.dataset.i));
}

async function copyRows(all: boolean) {
  if (!result?.expanded.length) return;
  const rows = all ? result.expanded.map((_, i) => i) : selectedRows();
  if (!rows.length) return void ($("#run-state").textContent = "コピーする行を選んでください");
  try {
    await navigator.clipboard.writeText(expandedText(rows, "\r\n"));
    $("#run-state").textContent = `${rows.length} 行をコピーしました`;
  } catch (e) {
    $("#run-state").textContent = `コピーできません：${e}`;
  }
}

let anchorRow = -1;
bindColumns($("#tab-body"));
// 表の行をクリック → エディタの元の行を強調。展開結果は Ctrl・Shift で複数選択
$("#tab-body").addEventListener("click", (e) => {
  const tr = (e.target as HTMLElement).closest<HTMLElement>("tr[data-line]");
  if (!tr) return;
  const page = tr.closest(".tab-page")!;
  const multi = page.id === "tab-expanded";
  const i = Number(tr.dataset.i);
  if (multi && e.shiftKey && anchorRow >= 0) {
    const rows = page.querySelector("tbody")!.children;
    const [a, b] = anchorRow < i ? [anchorRow, i] : [i, anchorRow];
    for (let k = 0; k < rows.length; k++) rows[k].classList.toggle("sel", k >= a && k <= b);
    window.getSelection()?.removeAllRanges();
  } else if (multi && e.ctrlKey) {
    tr.classList.toggle("sel");
    anchorRow = i;
  } else {
    page.querySelectorAll("tr.sel").forEach((el) => el.classList.remove("sel"));
    tr.classList.add("sel");
    anchorRow = i;
  }
  editor.focusLine(Number(tr.dataset.line));
  // 展開結果の行：その行の実行後のモーダル（エディタのカーソル移動より後に出す）
  if (multi) showModalForRow(i, `展開結果 ${i + 1} 行目`);
});

// 展開結果の右クリックメニュー
$("#tab-expanded").addEventListener("contextmenu", (e) => {
  if ((e.target as HTMLElement).closest("thead")) return; // 見出しは列のメニュー（coltable.ts）
  e.preventDefault();
  $("#view-ctx").classList.remove("open");
  const tr = (e.target as HTMLElement).closest<HTMLElement>("tr[data-i]");
  if (tr && !tr.classList.contains("sel")) {
    $("#tab-expanded").querySelectorAll("tr.sel").forEach((el) => el.classList.remove("sel"));
    tr.classList.add("sel");
    anchorRow = Number(tr.dataset.i);
  }
  const ctx = $("#ctx");
  ctx.classList.add("open");
  ctx.style.left = `${Math.min(e.clientX, window.innerWidth - ctx.offsetWidth - 4)}px`;
  ctx.style.top = `${Math.min(e.clientY, window.innerHeight - ctx.offsetHeight - 4)}px`;
});

// ---- 操作の配線 ----
const commands: Record<string, () => void> = {
  new: () => void newFile(),
  open: () => void openFile(),
  save: () => void saveFile(false),
  saveAs: () => void saveFile(true),
  saveExpanded: () => void saveExpanded(),
  postSettings: () => void postSettings(),
  appSettings: () => openAppSettings(appHooks),
  report: () => void openReport(),
  help: () => openHelp("features"),
  helpKeys: () => openHelp("keys"),
  helpLinks: () => openHelp("links"),
  openWeb: () => openExternal(LINKS.web),
  openDownload: () => openExternal(LINKS.download),
  about: () => openHelp("about"),
  zoomReset: () => setZoom(zoomArea, 1),
  resetView: () => void resetAll(),
  revealSettings: () => void invoke("settings_reveal").catch((e) => ($("#run-state").textContent = `開けません：${e}`)),
  copyAll: () => void copyRows(true),
  copySel: () => void copyRows(false),
  fit: () => viewer.fit(),
  first: () => { player.stop(); player.seek(0); },
  last: () => { player.stop(); player.seek(player.total); },
  stepBack: () => player.step(-1),
  stepFwd: () => player.step(1),
  playBack: () => player.play(-1),
  play: () => player.play(1),
  pause: () => player.stop(),
  toggle: () => (player.playing ? player.stop() : player.play(1)),
};
const closeMenus = () => document.querySelectorAll(".menu.open, #ctx.open, #view-ctx.open, #ghost-menu.open").forEach((m) => m.classList.remove("open"));
document.querySelectorAll<HTMLElement>("[data-cmd]").forEach((b) =>
  b.addEventListener("click", () => {
    closeMenus();
    b.blur(); // Space の再生と、ボタンの押下が重ならないように
    commands[b.dataset.cmd!]();
  }),
);
document.querySelectorAll<HTMLElement>(".menu-title").forEach((t) =>
  t.addEventListener("click", (e) => {
    e.stopPropagation();
    const menu = t.closest(".menu")!;
    const wasOpen = menu.classList.contains("open");
    closeMenus();
    menu.classList.toggle("open", !wasOpen);
  }),
);
document.addEventListener("click", closeMenus);

$<HTMLSelectElement>("#speed").addEventListener("change", (e) => {
  player.speed = settings.playSpeed = Number((e.target as HTMLSelectElement).value);
  saveSettings();
});
// ---- アプリ設定の画面からの変更：すぐ反映して保存 ----
const appHooks: AppHooks = {
  theme: (mode) => setTheme(mode),
  narrow: (mode) => { settings.narrowLayout = mode; setNarrowLayout(mode); saveSettings(); },
  zoom: (area, z) => setZoom(area, z),
  font: (v) => { settings.fontMono = v; applyFont(); saveSettings(); },
  layout: (l) => setLayout(l),
  modal: (patch) => modal.set(patch),
  speed: (v) => { settings.playSpeed = v; applySpeed(); saveSettings(); },
  switches: (patch) => {
    const before = skipLevels().join();
    Object.assign(settings, patch);
    const rerun = skipLevels().join() !== before;
    applyRunSwitches();
    saveSettings();
    if (rerun) void runNc(true);
  },
  peek: (patch) => { Object.assign(settings.peek, patch); peek.apply(); saveSettings(); },
  postSkip: () => runPost()?.expand.block_skip ?? false,
  postLevels,
  skipUsage: () => result?.block_skip_usage ?? [],
  clearRecent: (kind) => { settings.recent[kind] = []; showRecent(); saveSettings(); },
  splash: (on) => { settings.splash = on; saveSettings(); },
  dirLabel: () => settings.defaultDir || (samplesDir ? `${samplesDir}（同梱のサンプル）` : "（なし）"),
  pickDir: async () => {
    const dir = await openDir(settings.defaultDir || samplesDir || undefined);
    if (dir) { settings.defaultDir = dir; saveSettings(); }
  },
  resetDir: () => { settings.defaultDir = ""; saveSettings(); },
  reveal: () => void invoke("settings_reveal").catch((e) => ($("#run-state").textContent = `開けません：${e}`)),
  reset: () => void resetAll(),
  place: () => `保存先：${place.path || "（なし）"}${place.fallback ? "（exe のフォルダに書けないため %APPDATA%）" : ""}`,
};
/** 等幅の字体（空は style.css の既定） */
function applyFont() {
  if (settings.fontMono) root.setProperty("--font-mono", settings.fontMono);
  else root.removeProperty("--font-mono");
  editor.remeasure();
}
function applySpeed() {
  const sel = $<HTMLSelectElement>("#speed");
  sel.value = String(settings.playSpeed);
  if (sel.selectedIndex < 0) sel.value = "10";
  player.speed = Number(sel.value);
}
$<HTMLInputElement>("#seek").addEventListener("input", (e) => {
  player.stop();
  player.seek((Number((e.target as HTMLInputElement).value) / 10000) * player.total);
});

// ---- 視点と画面の分け方：1 画面は視点を選ぶ。4 分割は XY・3D・XZ・YZ。各ビューの視点・拡大・回転は設定ファイルに残す ----
let currentView: ViewName = "xy";
const viewLabel = (v: ViewName) => (v === "3d" ? "3D" : v.toUpperCase());

/** ツールバーと見出しを今の状態に合わせる（ビューの最大化・復帰でも呼ばれる）。 */
function showLayout(layout: Layout, active: ViewName, maximized: boolean) {
  const quad = layout === "quad" || maximized;
  document.querySelectorAll<HTMLElement>("[data-layout]").forEach((b) => b.classList.toggle("active", b.dataset.layout === (quad ? "quad" : "single")));
  document.querySelectorAll<HTMLElement>("[data-view]").forEach((b) => b.classList.toggle("active", layout === "single" && b.dataset.view === active));
  $("#view-name").textContent = layout === "quad" ? "　4 分割" : maximized ? `　${viewLabel(active)}（最大化。ダブルクリックで 4 分割へ）` : `　${viewLabel(active)}`;
}

function setView(v: ViewName) {
  currentView = v;
  viewer.setView(v);
  showLayout("single", v, false);
}

function setLayout(layout: Layout) {
  if (layout === "single") return setView(currentView);
  viewer.setLayout("quad");
  showLayout("quad", currentView, false);
}
document.querySelectorAll<HTMLElement>("[data-view]").forEach((b) => b.addEventListener("click", () => setView(b.dataset.view as ViewName)));
document.querySelectorAll<HTMLElement>("[data-layout]").forEach((b) => b.addEventListener("click", () => { b.blur(); setLayout(b.dataset.layout as Layout); }));

function setTab(tab: string) {
  const b = document.querySelector<HTMLElement>(`[data-tab="${tab}"]`) ?? $("[data-tab=expanded]");
  document.querySelectorAll("[data-tab], .tab-page").forEach((el) => el.classList.remove("active"));
  b.classList.add("active");
  $(`#tab-${b.dataset.tab}`).classList.add("active");
  $("#expanded-tools").classList.toggle("hidden", b.dataset.tab !== "expanded");
  settings.bottomTab = b.dataset.tab!;
}
document.querySelectorAll<HTMLElement>("[data-tab]").forEach((b) =>
  b.addEventListener("click", () => {
    setTab(b.dataset.tab!);
    saveSettings();
  }),
);

// ---- 文字の大きさ：エディタ・下部の表（展開結果ほか）・モーダルパネルを別々に。Ctrl＋ホイールで拡大・縮小、Ctrl+0 で既定 ----
const ZOOM_NAME: Record<ZoomArea, string> = { editor: "エディタ", expanded: "展開結果", modal: "モーダル", peek: "使用コード" };
/** Ctrl+0 で戻す対象：最後にマウスを置いた・クリックした方 */
let zoomArea: ZoomArea = "editor";
let zoomTimer = 0;
function applyZoom() {
  root.setProperty("--ed-zoom", String(settings.zoom.editor));
  root.setProperty("--exp-zoom", String(settings.zoom.expanded));
  root.setProperty("--mp-zoom", String(settings.zoom.modal));
  root.setProperty("--peek-zoom", String(settings.zoom.peek));
  editor.remeasure();
}
function setZoom(area: ZoomArea, z: number) {
  settings.zoom[area] = Math.round(Math.min(4, Math.max(0.5, z)) * 100) / 100;
  applyZoom();
  saveSettings();
  // 倍率はステータスバーに 1.5 秒だけ出す
  const st = $("#st-zoom");
  st.textContent = `${ZOOM_NAME[area]} ${Math.round(settings.zoom[area] * 100)}%`;
  clearTimeout(zoomTimer);
  zoomTimer = window.setTimeout(() => (st.textContent = ""), 1500);
}
for (const [sel, area] of [["#editor", "editor"], ["#tab-body", "expanded"], ["#modal-panel", "modal"], ["#code-peek", "peek"]] as const) {
  const el = $(sel);
  el.addEventListener("pointerenter", () => (zoomArea = area));
  el.addEventListener("pointerdown", () => (zoomArea = area));
  el.addEventListener(
    "wheel",
    (e) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      zoomArea = area;
      setZoom(area, settings.zoom[area] * (e.deltaY < 0 ? 1.1 : 1 / 1.1));
    },
    { passive: false, capture: true },
  );
}

// WebView の既定キー（F5 の再読み込み、Space・矢印のスクロールほか）を止めてアプリの操作へ
// 再生のキー（Space・矢印）はエディタと入力欄の外だけで効かせる
const GLOBAL_KEYS: Record<string, string> = {
  F5: "toggle", "Ctrl+o": "open", "Ctrl+n": "new", "Ctrl+s": "save", "Ctrl+Shift+s": "saveAs", Home: "fit",
  "Ctrl+r": "report", "Ctrl+0": "zoomReset",
};
const PLAY_KEYS: Record<string, string> = {
  " ": "toggle", ArrowLeft: "stepBack", ArrowRight: "stepFwd", "Shift+ArrowLeft": "playBack", "Shift+ArrowRight": "play",
  "Ctrl+ArrowLeft": "first", "Ctrl+ArrowRight": "last", "Ctrl+c": "copySel",
};
window.addEventListener(
  "keydown",
  (e) => {
    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    const key = (e.ctrlKey ? "Ctrl+" : "") + (e.shiftKey ? "Shift+" : "") + k;
    const target = e.target instanceof HTMLElement ? e.target : document.body;
    // ダイアログの中はダイアログの操作だけ。WebView の既定（再読み込みなど）は止める
    if (target.closest("dialog[open]")) {
      if (GLOBAL_KEYS[key] || key === "Ctrl+r" || key === "Ctrl+p") e.preventDefault();
      return;
    }
    const inEditor = !!target.closest(".cm-editor");
    const inInput = target.matches("input, select, textarea");
    if (inEditor) zoomArea = "editor";
    if (key === "Escape") closeMenus();
    if (key === "Home" && inEditor) return;
    let cmd = GLOBAL_KEYS[key];
    if (!cmd && !inEditor && !inInput) {
      cmd = PLAY_KEYS[key];
      // 文字を選んでいるときの Ctrl+C は通常のコピー
      if (cmd === "copySel" && !window.getSelection()?.isCollapsed) cmd = "";
    }
    if (cmd || key === "Ctrl+r" || key === "Ctrl+p") {
      e.preventDefault();
      if (cmd) commands[cmd]();
    }
  },
  true,
);

// 区切り線のドラッグで大きさを変える
function splitter(id: string, apply: (dx: number, dy: number) => void) {
  const el = $(id);
  el.addEventListener("pointerdown", (e) => {
    el.setPointerCapture(e.pointerId);
    let x = e.clientX, y = e.clientY;
    const move = (m: PointerEvent) => { apply(m.clientX - x, m.clientY - y); x = m.clientX; y = m.clientY; };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", () => el.removeEventListener("pointermove", move), { once: true });
  });
}
function applySplit() {
  const sp = settings.split;
  sp.editorW = Math.min(0.8, Math.max(0.15, sp.editorW));
  sp.bottomH = Math.min(Math.max(window.innerHeight - 200, 80), Math.max(80, sp.bottomH));
  root.setProperty("--editor-w", `${sp.editorW * 100}%`);
  root.setProperty("--bottom-h", `${sp.bottomH}px`);
}
splitter("#split-v", (dx) => { settings.split.editorW += dx / window.innerWidth; applySplit(); saveSettings(); });
splitter("#split-h", (_dx, dy) => { settings.split.bottomH -= dy; applySplit(); saveSettings(); });

// ---- ウィンドウの位置・大きさ・最大化：最大化していないときの位置と大きさを残す（起動時に Rust が戻す） ----
let winTimer = 0;
function trackWindow() {
  const desktop = win;
  if (!desktop) return;
  clearTimeout(winTimer);
  winTimer = window.setTimeout(async () => {
    try {
      if (await desktop.isMinimized()) return;
      if (await desktop.isMaximized()) {
        // 最大化の前の位置と大きさが無ければ、最大化だけを残す（大きさ 0 は起動時に使わない）
        settings.window = { ...(settings.window ?? { x: 0, y: 0, width: 0, height: 0 }), maximized: true };
      } else {
        const [p, s] = [await desktop.outerPosition(), await desktop.innerSize()]; // 大きさは中身（起動時の set_size と同じ基準）
        settings.window = { x: p.x, y: p.y, width: s.width, height: s.height, maximized: false };
      }
      saveSettings();
    } catch { /* ブラウザだけで開いたとき */ }
  }, 300);
}
win?.onMoved(trackWindow).catch(() => {});
win?.onResized(trackWindow).catch(() => {});

// 閉じる前に未保存の確認。閉じるときは設定をすぐ書く
win?.onCloseRequested(async (e) => {
    if (!(await confirmDiscard())) return e.preventDefault();
    await flushSettings();
  })
  .catch(() => {});
if (!isDesktop) {
  const saveButton = $('[data-cmd="save"]');
  if (saveButton.firstChild) saveButton.firstChild.textContent = "ダウンロード";
  $('[data-cmd="saveAs"]').remove();
  window.addEventListener("pagehide", () => void flushSettings());
  window.addEventListener("beforeunload", (e) => {
    if (file.dirty) e.preventDefault();
  });
}

// ---- 表示の状態を初期化：確認してから既定へ（方言・最近使ったファイル・ポストは残す） ----
async function resetAll() {
  const ok = "初期化";
  const r = await message(
    `表示の状態（ウィンドウの位置と大きさ、パネルの分割、視点、テーマ、文字の大きさ、下部のタブ、モーダルパネル、再生の速さ）を既定に戻しますか？\n\n設定ファイル：${place.path}`,
    { title: "表示の状態を初期化", kind: "warning", buttons: { ok, cancel: "キャンセル" } },
  );
  if (r !== ok && r !== "Ok") return;
  resetView();
  applyAll();
  setView("xy");
  modal.apply();
  try {
    await resetDesktopWindow();
  } catch { /* 位置は次回の起動で既定 */ }
  clearTimeout(winTimer);
  settings.window = null;
  const err = await flushSettings();
  $("#run-state").textContent = err || "表示の状態を初期化しました";
}

/** 設定の値を画面へ（起動時と初期化） */
function applyAll() {
  applyTheme();
  applySplit();
  applyZoom();
  applySpeed();
  applyFont();
  peek.apply();
  applyRunSwitches();
  setTab(settings.bottomTab);
  refreshAppSettings();
}

// ---- 起動：設定の状態を戻し、同梱の見本を読み込んで実行。保存した視点は最初の実行の後に戻す ----
setDoc(sample);
const savedView = settings.view;
setView("xy");
applyAll();
showRecent();
splashStatus("ポスト・コード定義の読み込み");
await loadSets();
showPost();
splashStatus("見本の実行");
initReport(() => file.path ?? file.name);
await start(runPost(), showPost, openFile, runNc);
if (place.backup) $("#run-state").textContent = `設定ファイルが壊れていたため既定で起動（元のファイル：${place.backup}）`;
if (savedView) {
  viewer.setState(savedView);
  currentView = savedView.active;
}
// 視点の変更を設定へ（起動時のフィットでは書かない）
viewer.onChange = () => {
  settings.view = viewer.getState();
  saveSettings();
};
// ---- 狭い幅の配置（mobile.ts。Web 版・デスクトップ版）：タッチの操作、下部のタブ、メニューのシート ----
viewer.touchGestures = !isDesktop || matchMedia("(any-pointer: coarse)").matches; // デスクトップ版はタッチ対応の端末だけ
initMobile({
  zoom: (area, z) => setZoom(area, z),
  zoomOf: (area) => settings.zoom[area],
  single: () => { const st = viewer.getState(); if (st.layout === "quad" || st.maximized) setLayout("single"); },
  remeasure: () => editor.remeasure(),
});
appReady();
