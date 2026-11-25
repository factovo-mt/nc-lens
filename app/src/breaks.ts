import { save, settings } from "./settings";
import { refreshAppSettings } from "./appsettings";
import type { Player } from "./player";
import type { PathViewer } from "./viewer";

export function bindRunSwitches(viewer: PathViewer, player: Player, postSkip: () => boolean, levels: () => number[], rerun: () => void, openSettings: () => void) {
  const menu = document.querySelector<HTMLElement>("#view-ctx")!;
  const status = document.querySelector<HTMLElement>("#break-status")!;
  const buttons = document.querySelectorAll<HTMLButtonElement>("[data-switch]");
  const ghostMenu = document.querySelector<HTMLElement>("#ghost-menu")!;
  const fade = document.querySelector<HTMLInputElement>("#ghost-fade")!;
  const fadeOut = document.querySelector<HTMLOutputElement>("#ghost-fade-v")!;
  const color = document.querySelector<HTMLInputElement>("#ghost-color")!;
  /** ツールバーのアイコンの名前（ツールチップは「名前：ON／OFF」） */
  const NAMES = { ghost: "完成形の表示", optionalStop: "オプショナルストップ（M01）", blockSkip: "ブロックスキップ（/）" };
  // NC リストの上の表示：クリックで切り替え、右クリック・⚙ で設定
  status.addEventListener("contextmenu", (e) => { e.preventDefault(); openSettings(); });
  status.querySelector<HTMLElement>(".bs-gear")!.onclick = (e) => { e.stopPropagation(); openSettings(); };
  const apply = () => {
    viewer.setGhost(settings.ghost);
    viewer.setGhostStyle(settings.ghostColor, settings.ghostFade);
    fade.value = String(settings.ghostFade);
    fadeOut.textContent = `${Math.round(settings.ghostFade * 100)}%`;
    color.value = toHex(settings.ghostColor || getComputedStyle(document.documentElement).getPropertyValue("--view-ghost").trim());
    player.optionalStop = settings.optionalStop;
    const skip = settings.blockSkip ?? postSkip();
    for (const b of buttons) {
      const key = b.dataset.switch as "ghost" | "optionalStop" | "blockSkip";
      const on = key === "blockSkip" ? skip : settings[key];
      b.setAttribute("aria-pressed", String(on));
      b.classList.toggle("checked", on);
      if (b.closest("#toolbar")) b.title = `${NAMES[key]}：${on ? "ON" : "OFF"}`;
      if (b.closest("#break-status")) {
        b.textContent = key === "optionalStop" ? `オプショナルストップ ${on ? "ON" : "OFF"}`
          : `ブロックスキップ ${on ? levels().map((n) => `/${n}`).join(" ") : "OFF"}`;
      }
    }
  };
  viewer.onContextMenu = (e) => {
    document.querySelectorAll(".menu.open, #ctx.open").forEach((el) => el.classList.remove("open"));
    menu.classList.add("open");
    menu.style.left = `${Math.max(0, Math.min(e.clientX, window.innerWidth - menu.offsetWidth - 4))}px`;
    menu.style.top = `${Math.max(0, Math.min(e.clientY, window.innerHeight - menu.offsetHeight - 4))}px`;
  };
  // 完成形の設定：▾ と、アイコンの右クリックで開く
  const openGhostMenu = (anchor: HTMLElement) => {
    document.querySelectorAll(".menu.open, #ctx.open, #view-ctx.open").forEach((el) => el.classList.remove("open"));
    apply();
    ghostMenu.classList.add("open");
    const r = anchor.getBoundingClientRect();
    ghostMenu.style.left = `${Math.max(4, Math.min(r.left, window.innerWidth - ghostMenu.offsetWidth - 4))}px`;
    ghostMenu.style.top = `${r.bottom + 2}px`;
  };
  const more = document.querySelector<HTMLElement>("#ghost-more")!;
  const ghostBtn = more.parentElement!.querySelector<HTMLElement>("[data-switch=ghost]")!;
  more.onclick = (e) => {
    e.stopPropagation();
    if (ghostMenu.classList.contains("open")) ghostMenu.classList.remove("open");
    else openGhostMenu(more.parentElement!);
    more.blur();
  };
  ghostBtn.oncontextmenu = (e) => {
    e.preventDefault();
    e.stopPropagation();
    openGhostMenu(more.parentElement!);
  };
  ghostMenu.addEventListener("click", (e) => e.stopPropagation());
  fade.oninput = () => {
    settings.ghostFade = Number(fade.value);
    apply();
  };
  color.oninput = () => {
    settings.ghostColor = color.value;
    apply();
  };
  fade.onchange = color.onchange = () => { save(); refreshAppSettings(); };
  document.querySelector<HTMLElement>("#ghost-color-reset")!.onclick = () => {
    settings.ghostColor = "";
    apply();
    save();
    refreshAppSettings();
  };
  for (const b of buttons) {
    b.onclick = () => {
      const key = b.dataset.switch as "ghost" | "optionalStop" | "blockSkip";
      if (key === "blockSkip") settings.blockSkip = !(settings.blockSkip ?? postSkip());
      else settings[key] = !settings[key];
      apply();
      save();
      refreshAppSettings();
      b.blur();
      if (key === "blockSkip") rerun();
    };
  }
  return apply;
}

/** CSS の色（#rgb・#rrggbb・rgb()）→ input[type=color] の #rrggbb */
function toHex(css: string) {
  const c = document.createElement("canvas").getContext("2d")!;
  c.fillStyle = "#000000";
  c.fillStyle = css;
  return c.fillStyle.startsWith("#") ? c.fillStyle : "#888888";
}
