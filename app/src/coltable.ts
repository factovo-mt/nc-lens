// 下部の表の列：境界のドラッグで幅、見出しのドラッグで並べ替え、見出しの右クリックで表示・非表示、
// 境界のダブルクリックで内容に合わせた幅。表ごとに設定ファイル（settings.tables）へ残す。
import { save, settings } from "./settings";

export type Col = { key: string; label: string; wide?: boolean };
type Layout = { order?: string[]; hidden?: string[]; width?: Record<string, number> };

const MIN_W = 28;
const esc = (s: string) => s.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);
const state = (id: string): Layout => (settings.tables[id] ??= {});

/** 表の HTML（列は元の順）。DOM に入れた後で applyLayout を呼ぶ */
export function tableHtml(id: string, cols: Col[], rows: string[][], lines: number[]) {
  const th = cols.map((c) =>
    `<th data-col="${c.key}" draggable="true"${c.wide ? ' class="wide"' : ""}>${esc(c.label)}<span class="col-rz" title="ドラッグで幅、ダブルクリックで内容に合わせる"></span></th>`).join("");
  const tr = rows.map((r, i) => `<tr data-line="${lines[i]}" data-i="${i}">${r.map((c) => `<td>${c}</td>`).join("")}</tr>`).join("");
  return `<table class="ctable" data-table="${id}" data-order="${cols.map((c) => c.key).join(",")}" data-def="${cols.map((c) => c.key).join(",")}"><thead><tr>${th}</tr></thead><tbody>${tr}</tbody></table>`;
}

/** 設定の順・表示・幅を表へ反映する */
export function applyLayout(table: HTMLTableElement) {
  const s = state(table.dataset.table!);
  const cur = table.dataset.order!.split(",");
  const order = [...(s.order ?? []).filter((k) => cur.includes(k)), ...cur.filter((k) => !s.order?.includes(k))];
  const hidden = new Set((s.hidden ?? []).filter((k) => cur.includes(k)));
  if (hidden.size >= cur.length) hidden.clear();
  if (order.join(",") !== cur.join(",")) {
    const pos = order.map((k) => cur.indexOf(k));
    for (const tr of table.rows) {
      const cells = [...tr.cells];
      tr.append(...pos.map((p) => cells[p]));
    }
    table.dataset.order = order.join(",");
  }
  const w = s.width ?? {};
  const fixed = Object.keys(w).length > 0;
  let sum = 0;
  for (const tr of table.rows) {
    [...tr.cells].forEach((c, i) => { c.hidden = hidden.has(order[i]); });
  }
  for (const th of table.tHead!.rows[0].cells) {
    const k = th.dataset.col!;
    th.style.width = fixed && w[k] ? `${w[k]}px` : "";
    if (!th.hidden) sum += w[k] ?? 0;
  }
  table.classList.toggle("fixed", fixed);
  table.style.width = fixed ? `${sum}px` : "";
}

/** 今の見た目の幅をすべての列について覚える（固定の幅へ切り替えるとき） */
function measure(table: HTMLTableElement) {
  const s = state(table.dataset.table!);
  s.width ??= {};
  for (const th of table.tHead!.rows[0].cells) {
    if (!th.hidden && !s.width[th.dataset.col!]) s.width[th.dataset.col!] = Math.ceil(th.getBoundingClientRect().width) + 1;
  }
}

/** 列の内容に合わせた幅（自然な幅で並べ直して測る） */
function fitWidth(table: HTMLTableElement, key: string) {
  const th = table.tHead!.rows[0].cells;
  const i = [...th].findIndex((c) => c.dataset.col === key);
  const probe = table.cloneNode(true) as HTMLTableElement;
  probe.classList.remove("fixed");
  probe.style.cssText = "position:absolute;visibility:hidden;width:auto;left:-99999px;top:0";
  for (const c of probe.tHead!.rows[0].cells) c.style.width = "";
  table.parentElement!.append(probe);
  const w = Math.ceil(probe.tHead!.rows[0].cells[i].getBoundingClientRect().width) + 1;
  probe.remove();
  return Math.max(MIN_W, w);
}

let menu: HTMLElement | null = null;
function closeMenu() {
  menu?.remove();
  menu = null;
}

/** 表の入れ物（下部のタブの本体）に一度だけ結び付ける */
export function bindColumns(host: HTMLElement) {
  // 幅：境界のドラッグ
  host.addEventListener("pointerdown", (e) => {
    const rz = (e.target as HTMLElement).closest<HTMLElement>(".col-rz");
    if (!rz || e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const th = rz.parentElement as HTMLTableCellElement, table = th.closest("table")!;
    measure(table);
    applyLayout(table);
    const s = state(table.dataset.table!), k = th.dataset.col!, x0 = e.clientX, w0 = s.width![k];
    rz.setPointerCapture(e.pointerId);
    document.documentElement.classList.add("col-resizing");
    rz.onpointermove = (m) => {
      s.width![k] = Math.max(MIN_W, Math.round(w0 + m.clientX - x0));
      applyLayout(table);
    };
    rz.onlostpointercapture = () => {
      rz.onpointermove = rz.onlostpointercapture = null;
      document.documentElement.classList.remove("col-resizing");
      save();
    };
  });
  // 内容に合わせる：境界（または見出し）のダブルクリック
  host.addEventListener("dblclick", (e) => {
    const th = (e.target as HTMLElement).closest<HTMLTableCellElement>("thead th");
    if (!th) return;
    const table = th.closest("table")!;
    measure(table);
    state(table.dataset.table!).width![th.dataset.col!] = fitWidth(table, th.dataset.col!);
    applyLayout(table);
    save();
  });
  // 並べ替え：見出しのドラッグ
  let dragKey = "";
  host.addEventListener("dragstart", (e) => {
    const th = (e.target as HTMLElement).closest<HTMLElement>("thead th");
    if (!th) return;
    dragKey = th.dataset.col!;
    e.dataTransfer!.effectAllowed = "move";
    e.dataTransfer!.setData("text/plain", dragKey);
    th.classList.add("col-drag");
  });
  const target = (e: DragEvent) => {
    const th = (e.target as HTMLElement).closest<HTMLElement>("thead th");
    return dragKey && th && th.dataset.col !== dragKey ? th : null;
  };
  host.addEventListener("dragover", (e) => {
    const th = target(e);
    host.querySelectorAll(".col-drop-l, .col-drop-r").forEach((c) => c.classList.remove("col-drop-l", "col-drop-r"));
    if (!th) return;
    e.preventDefault();
    const r = th.getBoundingClientRect();
    th.classList.add(e.clientX < r.left + r.width / 2 ? "col-drop-l" : "col-drop-r");
  });
  host.addEventListener("drop", (e) => {
    const th = target(e);
    if (!th) return;
    e.preventDefault();
    const table = th.closest("table")!;
    const order = table.dataset.order!.split(",").filter((k) => k !== dragKey);
    const r = th.getBoundingClientRect();
    order.splice(order.indexOf(th.dataset.col!) + (e.clientX < r.left + r.width / 2 ? 0 : 1), 0, dragKey);
    state(table.dataset.table!).order = order;
    applyLayout(table);
    save();
  });
  host.addEventListener("dragend", () => {
    dragKey = "";
    host.querySelectorAll(".col-drag, .col-drop-l, .col-drop-r").forEach((c) => c.classList.remove("col-drag", "col-drop-l", "col-drop-r"));
  });
  // 表示・非表示：見出しの右クリック
  host.addEventListener("contextmenu", (e) => {
    const th = (e.target as HTMLElement).closest<HTMLElement>("thead th");
    if (!th) return;
    e.preventDefault();
    e.stopPropagation();
    closeMenu();
    const table = th.closest("table")!;
    const s = state(table.dataset.table!);
    menu = document.createElement("div");
    menu.className = "menu-items col-menu";
    for (const c of table.tHead!.rows[0].cells) {
      const b = document.createElement("button");
      b.className = "check";
      b.classList.toggle("checked", !c.hidden);
      b.textContent = c.textContent;
      b.onclick = () => {
        const hidden = new Set(s.hidden ?? []);
        if (hidden.has(c.dataset.col!)) hidden.delete(c.dataset.col!);
        else if (table.dataset.order!.split(",").length - hidden.size > 1) hidden.add(c.dataset.col!);
        s.hidden = [...hidden];
        applyLayout(table);
        save();
        b.classList.toggle("checked", !c.hidden);
      };
      menu.append(b);
    }
    const reset = document.createElement("button");
    reset.textContent = "列の既定に戻す";
    reset.onclick = () => {
      // 既定の順へ並べ直してから、設定を消す（幅・表示も既定）
      settings.tables[table.dataset.table!] = { order: table.dataset.def!.split(",") };
      applyLayout(table);
      delete settings.tables[table.dataset.table!];
      save();
      closeMenu();
    };
    menu.append(document.createElement("hr"), reset);
    menu.addEventListener("click", (ev) => ev.stopPropagation());
    document.body.append(menu);
    menu.style.left = `${Math.max(4, Math.min(e.clientX, innerWidth - menu.offsetWidth - 4))}px`;
    menu.style.top = `${Math.max(4, Math.min(e.clientY, innerHeight - menu.offsetHeight - 4))}px`;
  });
  document.addEventListener("click", closeMenu);
  document.addEventListener("keydown", (e) => { if (e.key === "Escape") closeMenu(); });
}
