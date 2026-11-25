// ヘルプの浮動ウィンドウ：左にページ（NC Lens について・リンク・主な機能・キー操作・今後の予定・変更履歴）、右に中身。
// 中身は help-data.ts。行は折り返さず、はみ出す分は横スクロール。
import { FloatWin } from "./floatwin";
import { invoke, isDesktop, openExternal } from "./platform";
import { LINKS, LINK_ROWS } from "./links";
import { APP, FEATURES, KEYS, ROADMAP, history } from "./help-data";

const $ = <T extends HTMLElement = HTMLElement>(sel: string) => document.querySelector<T>(sel)!;
const esc = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
const PAGES: [string, string][] = [["about", "NC Lens について"], ["links", "リンク"], ["features", "主な機能"], ["keys", "キー操作"], ["roadmap", "今後の予定"], ["history", "変更履歴"]];
const table = (head: string[], rows: string[][]) =>
  `<table class="ptable htable"><thead><tr>${head.map((h) => `<th>${h}</th>`).join("")}</tr></thead><tbody>${rows.map((r) => `<tr>${r.map((c) => `<td>${esc(c)}</td>`).join("")}</tr>`).join("")}</tbody></table>`;

function render(page: string) {
  switch (page) {
    case "about":
      return `<section class="psec habout"><h2>${APP.name}</h2><p>${esc(APP.summary)}</p>
        ${table(["項目", "内容"], [["版", APP.version], ["ビルド日", APP.buildDate], ["著作権", APP.copyright], ["ライセンス", `${APP.license}（同梱の LICENSE・LICENSE-ja.txt）`],
          ["第三者のライセンス", "同梱の THIRD_PARTY_LICENSES.md（使用したライブラリと各ライセンス）"]])}
        ${isDesktop ? `<div class="pchecks"><button class="pbtn" data-hact="licenses">第三者のライセンスの場所</button></div>` : ""}</section>`;
    case "links":
      return `<section class="psec"><table class="ptable htable hlinks"><thead><tr><th>リンク</th><th>URL</th><th></th><th>内容</th></tr></thead><tbody>${LINK_ROWS.map(([k, name, note]) =>
        `<tr><td>${esc(name)}</td><td><a href="${LINKS[k]}" target="_blank" rel="noopener" data-hlink="${k}">${LINKS[k]}</a></td><td><button class="pbtn" data-hcopy="${k}">コピー</button></td><td>${esc(note)}</td></tr>`).join("")}</tbody></table></section>`;
    case "features":
      return FEATURES.map(([h, items]) => `<section class="psec"><h4>${esc(h)}</h4><ul class="hlist">${items.map((i) => `<li>${esc(i)}</li>`).join("")}</ul></section>`).join("");
    case "keys":
      return `<section class="psec">${table(["キー・操作", "内容"], KEYS)}</section>`;
    case "roadmap":
      return `<section class="psec"><h4>今後の予定<small>目安。要望しだいで前後</small></h4>${table(["時期", "内容"], ROADMAP)}</section>`;
    case "history":
      return history().map((v) => `<section class="psec"><h4>${esc(v.version)}<small>${esc(v.date)}</small></h4><ul class="hlist">${v.items.map((i) => `<li>${esc(i)}</li>`).join("")}</ul></section>`).join("");
  }
  return "";
}

let win: FloatWin | null = null;
let page = "about";

function show(p: string) {
  page = p;
  document.querySelectorAll<HTMLElement>("#help-pages [data-hpage]").forEach((b) => b.classList.toggle("active", b.dataset.hpage === p));
  $("#help-body").innerHTML = render(p);
}

/** ヘルプを開く（開いていれば前へ出してページを切り替える） */
export function openHelp(start = page) {
  const dlg = $<HTMLDialogElement>("#help-dialog");
  if (!win) {
    win = new FloatWin(dlg, "help");
    $("#help-pages").innerHTML = PAGES.map(([k, t]) => `<button data-hpage="${k}">${t}</button>`).join("");
    dlg.addEventListener("click", (e) => {
      const t = e.target as HTMLElement;
      const pg = t.closest<HTMLElement>("[data-hpage]")?.dataset.hpage;
      if (pg) show(pg);
      if (t.closest("[data-hact=licenses]")) void invoke("licenses_reveal").catch(() => {});
      const link = t.closest<HTMLElement>("[data-hlink]")?.dataset.hlink as keyof typeof LINKS | undefined;
      // Web 版は a の target=_blank のまま。デスクトップ版は既定のブラウザで開く
      if (link && isDesktop) { e.preventDefault(); openExternal(LINKS[link]); }
      const copy = t.closest<HTMLButtonElement>("[data-hcopy]");
      if (copy) void navigator.clipboard.writeText(LINKS[copy.dataset.hcopy as keyof typeof LINKS]).then(() => { copy.textContent = "コピー済み"; setTimeout(() => (copy.textContent = "コピー"), 1200); }).catch(() => {});
    });
  }
  show(start);
  win.open();
}
