// ポスト・コード定義の画面：セット（ポスト＝方言を含む）の一覧、ページの一覧、ページの中身の 3 列。
// 1 セット＝1 ポスト（TOML・JSON）。一覧は nclens.settings.json に残し、ファイルに結び付いたセットは保存のときにファイルへも書く。
// ページの中身は post.ts（コード設定・変数設定・展開出力）と dialect.ts（アドレス表〜マクロ構文）。
import { invoke, isDesktop, message, open, save } from "./platform";
import { SETTINGS_FILTERS, CHECKS, TABLES, field, rowHtml, renderPost, collectPost } from "./post";
import { DIALECT_TABS, ROWS, builtinDialects, renderDialect, collectDialect, explain } from "./dialect";
import type { PostSet } from "./settings";
import { FloatWin } from "./floatwin";
import type { DialectSettings, PostProfile } from "./types";

const $ = <T extends HTMLElement = HTMLElement>(sel: string) => document.querySelector<T>(sel)!;
const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p;
const TITLE = "ポスト・コード定義";

/** ページ：ポストの 3 つとコード定義の 5 つ */
const PAGES: [string, string][] = [
  ["codes", "コード設定"], ["vars", "変数設定"], ["expand", "展開出力"],
  ["addr", "アドレス表"], ["g", "G コード表"], ["m", "M コード表"], ["custom", "ユーザー定義"], ["macro", "マクロ構文"],
];

export interface SetState { sets: PostSet[]; current: string }

let win: FloatWin | null = null;
/** 開いている画面のキャンセル（× と Esc。未保存の変更があれば確認） */
let cancelOpen: (() => void | Promise<void>) | null = null;

/** ポスト・コード定義の画面を開く。保存なら新しい一覧、キャンセルなら null。 */
export async function openSetDialog(start: SetState, page = "codes"): Promise<SetState | null> {
  const dlg = $<HTMLDialogElement>("#set-dialog");
  win ??= new FloatWin(dlg, "sets", async () => { await cancelOpen?.(); return false; });
  // 開いていれば前へ出すだけ（編集中の内容はそのまま）
  if (win.isOpen) { win.open(); return null; }
  await builtinDialects();
  const body = $("#set-body");
  const st: SetState = structuredClone(start);
  let sel = Math.max(0, st.sets.findIndex((s) => s.profile.name === st.current));
  /** 中身を変えたセット（ファイルに結び付いていれば保存のときに書く） */
  const changed = new Set<PostSet>();
  let dirty = false;

  const cur = () => st.sets[sel];
  const showErrors = (errs: string[], note = "") => {
    $("#set-errors").textContent = errs.length ? `入力の誤り ${errs.length} 件：${errs.slice(0, 3).join("、")}${errs.length > 3 ? " ほか" : ""}` : note;
  };
  const showFile = () => {
    const s = cur();
    $("#set-file").textContent = `${changed.has(s) ? "● " : ""}${s.path ?? "（ファイル無し。ファイルへの保存：書き出し）"}`;
  };
  const list = () => {
    $("#set-list").innerHTML = st.sets
      .map((s, i) => `<button data-sel="${i}" class="${i === sel ? "active" : ""}" title="${esc(s.path ?? "")}">${esc(s.profile.name)}${s.profile.name === st.current ? "<em>既定</em>" : ""}<small>${esc(s.path ? baseName(s.path) : s.profile.dialect.name)}</small></button>`)
      .join("");
  };
  const setPage = (p: string) => {
    page = p;
    dlg.querySelectorAll<HTMLElement>("[data-spage]").forEach((b) => b.classList.toggle("active", b.dataset.spage === p));
    body.querySelectorAll<HTMLElement>(".ppage").forEach((pg) => pg.classList.toggle("active", pg.dataset.page === p));
  };
  /** 選んだセットの全ページを作る（見えるのは 1 ページ） */
  const fill = () => {
    const p = cur().profile;
    $("#set-name").innerHTML = field("セットの名前", "post.name", p.name, "text") + field("説明", "post.comment", p.comment, "free");
    const html: Record<string, string> = { ...renderPost(p), ...renderDialect(p.dialect) };
    body.innerHTML = PAGES.map(([k]) => `<div class="ppage" data-page="${k}">${html[k]}</div>`).join("");
    setPage(page);
    showFile();
  };
  /** 画面 → 選んだセット。誤りがあれば最初の誤りのページを開いて文言を返す。 */
  const sync = async (): Promise<string[]> => {
    const s = cur();
    const root = dlg.querySelector<HTMLElement>(".smain")!;
    const post = collectPost(root, s.profile);
    const dia = collectDialect(s.profile.dialect, body);
    const errors = [...post.errors, ...dia.errors];
    const name = post.profile.name.trim();
    const nameEl = $("#set-name").querySelector<HTMLInputElement>('[data-k="post.name"]')!;
    if (name && st.sets.some((x, i) => i !== sel && x.profile.name === name)) {
      nameEl.classList.add("bad");
      errors.push("セットの名前：重複");
    }
    if (!errors.length) errors.push(...(await invoke<string[]>("dialect_validate", { dialect: dia.dialect })).map(explain));
    if (errors.length) {
      showErrors(errors);
      const bad = dlg.querySelector<HTMLElement>("input.bad");
      const pg = bad?.closest<HTMLElement>(".ppage")?.dataset.page;
      if (pg) setPage(pg);
      bad?.focus();
      return errors;
    }
    const profile: PostProfile = { ...post.profile, name, dialect: dia.dialect };
    if (JSON.stringify(profile) !== JSON.stringify(s.profile)) {
      if (st.current === s.profile.name) st.current = name;
      s.profile = profile;
      changed.add(s);
    }
    showErrors([]);
    list();
    showFile();
    return [];
  };
  const unique = (base: string) => {
    let n = base, k = 2;
    while (st.sets.some((s) => s.profile.name === n)) n = `${base} ${k++}`;
    return n;
  };
  const choose = async (i: number) => {
    if ((await sync()).length) return;
    sel = i;
    list();
    fill();
  };
  const addSet = async (s: PostSet) => {
    s.profile.name = unique(s.profile.name.trim() || "新しいセット");
    st.sets.push(s);
    dirty = true;
    sel = st.sets.length - 1;
    list();
    fill();
  };

  return new Promise((resolve) => {
    const actions: Record<string, () => Promise<void> | void> = {
      async add() {
        if ((await sync()).length) return;
        const profile = await invoke<PostProfile>("post_default");
        profile.name = "新しいセット";
        profile.dialect = (await builtinDialects())[0] ?? profile.dialect;
        await addSet({ profile, path: null });
      },
      async dup() {
        if ((await sync()).length) return;
        await addSet({ profile: { ...structuredClone(cur().profile), name: `${cur().profile.name} のコピー` }, path: null });
      },
      rename() {
        const el = $("#set-name").querySelector<HTMLInputElement>('[data-k="post.name"]')!;
        el.focus();
        el.select();
      },
      async del() {
        if (st.sets.length <= 1) return void (await message("セット：1 つ以上必要", { title: TITLE }));
        const name = cur().profile.name;
        const r = await message(`セット「${name}」の削除（一覧のみ。ポスト設定のファイルは対象外）`, { title: TITLE, kind: "warning", buttons: { ok: "削除", cancel: "キャンセル" } });
        if (r !== "削除" && r !== "Ok") return;
        st.sets.splice(sel, 1);
        if (st.current === name) st.current = st.sets[0].profile.name;
        sel = Math.min(sel, st.sets.length - 1);
        dirty = true;
        list();
        fill();
        showErrors([]);
      },
      async default() {
        if ((await sync()).length) return;
        st.current = cur().profile.name;
        dirty = true;
        list();
      },
      async import() {
        const path = await open({ multiple: false, filters: SETTINGS_FILTERS });
        if (!path) return;
        if ((await sync()).length) return;
        const nativePath = typeof path === "string" ? path : null;
        const same = nativePath ? st.sets.findIndex((s) => s.path?.toLowerCase() === nativePath.toLowerCase()) : -1;
        if (same >= 0) return choose(same);
        try {
          await addSet({ profile: await invoke<PostProfile>("post_load", { path }), path: nativePath });
        } catch (e) {
          await message(`ポストの読み込み失敗：${e}`, { title: TITLE, kind: "error" });
        }
      },
      async export() {
        if ((await sync()).length) return;
        const s = cur();
        const path = await save({ defaultPath: s.path ?? `${s.profile.name}.post.toml`, filters: SETTINGS_FILTERS });
        if (!path) return;
        try {
          await invoke("post_save", { path, post: s.profile });
          s.path = isDesktop ? path : null;
          changed.delete(s);
          dirty = true;
          list();
          showFile();
          showErrors([], `書き出し完了：${path}`);
        } catch (e) {
          await message(`書き出し失敗：${e}`, { title: TITLE, kind: "error" });
        }
      },
      async apply() {
        if ((await sync()).length) return;
        // 全部のセットの方言をコアで検査
        for (const [i, s] of st.sets.entries()) {
          const errs = await invoke<string[]>("dialect_validate", { dialect: s.profile.dialect });
          if (errs.length) {
            sel = i;
            list();
            fill();
            return showErrors(errs.map(explain));
          }
        }
        // ファイルに結び付いたセットは、変えたものだけファイルへも書く
        for (const s of changed) {
          if (!s.path || !st.sets.includes(s)) continue;
          try {
            await invoke("post_save", { path: s.path, post: s.profile });
          } catch (e) {
            sel = st.sets.indexOf(s);
            list();
            fill();
            return void (await message(`保存失敗（${s.path}）：${e}`, { title: TITLE, kind: "error" }));
          }
        }
        finish(st);
      },
      async cancel() {
        if (dirty || changed.size) {
          const r = await message("ポスト・コード定義の変更の破棄", { title: TITLE, kind: "warning", buttons: { ok: "破棄", cancel: "キャンセル" } });
          if (r !== "破棄" && r !== "Ok") return;
        }
        finish(null);
      },
    };
    /** 方言だけの読み込み・書き出し（TOML・JSON） */
    const dialectFile = async (kind: string) => {
      if ((await sync()).length) return;
      const s = cur();
      if (kind === "export") {
        const path = await save({ defaultPath: `${s.profile.dialect.name}.dialect.toml`, filters: SETTINGS_FILTERS });
        if (!path) return;
        try {
          await invoke("dialect_save", { path, dialect: s.profile.dialect });
          showErrors([], `方言の書き出し完了：${path}`);
        } catch (e) {
          await message(`書き出し失敗：${e}`, { title: TITLE, kind: "error" });
        }
        return;
      }
      const path = await open({ multiple: false, filters: SETTINGS_FILTERS });
      if (!path) return;
      try {
        s.profile.dialect = await invoke<DialectSettings>("dialect_load", { path });
        changed.add(s);
        fill();
        showErrors([], `方言の読み込み完了：${typeof path === "string" ? path : path.name}`);
      } catch (e) {
        await message(`方言の読み込み失敗：${e}`, { title: TITLE, kind: "error" });
      }
    };
    const onClick = async (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      const act = t.closest<HTMLElement>("[data-sact]")?.dataset.sact;
      if (act) return void actions[act]();
      const s = t.closest<HTMLElement>("[data-sel]")?.dataset.sel;
      if (s !== undefined) return void (Number(s) !== sel && choose(Number(s)));
      const pg = t.closest<HTMLElement>("[data-spage]")?.dataset.spage;
      if (pg) return setPage(pg);
      const df = t.closest<HTMLElement>("[data-dfile]")?.dataset.dfile;
      if (df) return void dialectFile(df);
      const add = t.closest<HTMLElement>("[data-add]")?.dataset.add;
      if (add) {
        const def = TABLES[add];
        const tbody = body.querySelector(`[data-table="${add}"] tbody`)!;
        tbody.insertAdjacentHTML("beforeend", rowHtml(def, def.cols.map(([, , chk]) => (chk === "num" ? 0 : ""))));
        tbody.querySelector<HTMLInputElement>("tr:last-child input")?.focus();
        return markChanged();
      }
      const addRow = t.closest<HTMLElement>("[data-addrow]")?.dataset.addrow;
      if (addRow && (DIALECT_TABS as readonly string[]).includes(addRow)) {
        const tbody = body.querySelector(`[data-dtable="${addRow}"] tbody`)!;
        tbody.insertAdjacentHTML("beforeend", ROWS[addRow]("", undefined));
        tbody.querySelector<HTMLInputElement>("tr:last-child input")?.focus();
        return markChanged();
      }
      if (t.closest(".pdel")) {
        t.closest("tr")!.remove();
        markChanged();
      }
    };
    const markChanged = () => {
      if (!changed.has(cur())) {
        changed.add(cur());
        showFile();
      }
    };
    const onInput = (e: Event) => {
      const el = e.target as HTMLInputElement;
      if (el.dataset.check !== undefined) {
        const err = CHECKS[el.dataset.check]?.(el.value) ?? "";
        el.classList.toggle("bad", !!err);
        el.title = err;
      }
      // 選んだ項目の説明 1 行（重複指定の選択肢）
      if (el instanceof HTMLSelectElement && el.hasAttribute("data-desc")) {
        const d = el.closest(".pdup")?.querySelector(".pdesc");
        if (d) d.textContent = el.selectedOptions[0]?.dataset.desc ?? "";
      }
      markChanged();
    };
    const finish = (r: SetState | null) => {
      dlg.removeEventListener("click", onClick);
      dlg.removeEventListener("input", onInput);
      dlg.removeEventListener("change", onInput);
      cancelOpen = null;
      win!.close();
      resolve(r);
    };
    dlg.addEventListener("click", onClick);
    dlg.addEventListener("input", onInput);
    dlg.addEventListener("change", onInput);
    $("#set-pages").innerHTML =
      `<div class="spage-group">ポスト</div>${PAGES.slice(0, 3).map(([k, t]) => `<button data-spage="${k}">${t}</button>`).join("")}` +
      `<div class="spage-group">コード定義（方言）</div>${PAGES.slice(3).map(([k, t]) => `<button data-spage="${k}">${t}</button>`).join("")}`;
    list();
    fill();
    showErrors([]);
    cancelOpen = actions.cancel;
    win!.open();
  });
}
