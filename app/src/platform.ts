import { invoke as nativeInvoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { emitTo } from "@tauri-apps/api/event";
import { open as nativeOpen, save as nativeSave } from "@tauri-apps/plugin-dialog";
import type { MessageDialogOptions, OpenDialogOptions, SaveDialogOptions } from "@tauri-apps/plugin-dialog";
import type { Report, RunResult } from "./types";

export const isDesktop = isTauri();
export const desktopWindow = isDesktop ? getCurrentWindow() : null;

/** 起動画面へ読み込み状況を知らせる（起動画面が無ければ何もしない）。 */
export function splashStatus(text: string) {
  if (isDesktop) void emitTo("splash", "splash-status", text).catch(() => {});
}

/** 準備の完了：起動画面を閉じて本体を表示する（Rust の app_ready）。 */
export function appReady() {
  if (isDesktop) void nativeInvoke("app_ready").catch(() => {});
}

export async function resetDesktopWindow() {
  if (!desktopWindow) return;
  if (await desktopWindow.isMaximized()) await desktopWindow.unmaximize();
  await desktopWindow.setSize(new LogicalSize(1400, 860));
  await desktopWindow.center();
}

export async function open(options: OpenDialogOptions): Promise<string | File | null> {
  if (isDesktop) return nativeOpen({ ...options, multiple: false, directory: false });
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.hidden = true;
    const extensions = options.filters?.flatMap((filter) => filter.extensions) ?? [];
    if (extensions.length && !extensions.includes("*")) input.accept = extensions.map((ext) => `.${ext}`).join(",");
    const finish = (file: File | null) => {
      input.remove();
      resolve(file);
    };
    input.addEventListener("change", () => finish(input.files?.[0] ?? null), { once: true });
    input.addEventListener("cancel", () => finish(null), { once: true });
    document.body.append(input);
    input.click();
  });
}

/** フォルダを選ぶ（デスクトップ版のみ）。 */
export async function openDir(defaultPath?: string): Promise<string | null> {
  if (!isDesktop) return null;
  const r = await nativeOpen({ directory: true, multiple: false, defaultPath });
  return typeof r === "string" ? r : null;
}

export async function save(options: SaveDialogOptions): Promise<string | null> {
  if (isDesktop) return nativeSave(options);
  const name = options.defaultPath ?? null;
  // Web 版は保存のダイアログが無いので、形式（フィルタ）が 2 つ以上なら先に選ぶ
  const kinds = options.filters?.filter((f) => !f.extensions.includes("*")) ?? [];
  if (!name || kinds.length < 2) return name;
  const labels = kinds.map((f) => f.extensions[0].toUpperCase());
  const r = await message("ダウンロードするファイルの形式の選択", { title: "保存の形式", buttons: { yes: labels[0], no: labels[1], cancel: "キャンセル" } });
  const ext = r === labels[0] ? kinds[0].extensions[0] : r === labels[1] ? kinds[1].extensions[0] : null;
  return ext ? withExtension(name, ext) : null;
}

/** 拡張子の付け替え（.toml・.json の付け替え。それ以外は後ろに足す） */
function withExtension(path: string, ext: string) {
  return /\.(toml|json)$/i.test(path) ? path.replace(/\.(toml|json)$/i, `.${ext}`) : `${path}.${ext}`;
}

const isJsonText = (text: string) => text.replace(/^﻿/, "").trimStart().startsWith("{");
const isJsonPath = (path: unknown) => /\.json$/i.test(String(path));

const ICON: Record<string, string> = {
  warning: '<path d="M12 3.2L22 20.5H2z" fill="currentColor" fill-opacity=".14" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/><path d="M12 9.5v5" stroke="currentColor" stroke-width="2" stroke-linecap="round"/><circle cx="12" cy="17.6" r="1.2" fill="currentColor"/>',
  error: '<circle cx="12" cy="12" r="9.5" fill="currentColor" fill-opacity=".14" stroke="currentColor" stroke-width="1.6"/><path d="M8.5 8.5l7 7M15.5 8.5l-7 7" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>',
  info: '<circle cx="12" cy="12" r="9.5" fill="currentColor" fill-opacity=".14" stroke="currentColor" stroke-width="1.6"/><path d="M12 11v6" stroke="currentColor" stroke-width="2" stroke-linecap="round"/><circle cx="12" cy="7.6" r="1.2" fill="currentColor"/>',
};
const HEADING: Record<string, string> = { warning: "確認", error: "エラー", info: "お知らせ" };

/**
 * 確認メッセージ（デスクトップ版・Web 版で共通のアプリ内の小さなモーダル）。押したボタンの文言、Esc は "Cancel"。
 * title：題（体言止め。"NC Lens" なら種類から）、text：本文 1〜2 行。
 * ボタン：yes（既定・強調）・no（注意の色）・cancel。ok と cancel だけで kind が warning なら ok を注意の色、既定は cancel。
 */
export async function message(text: string, options: MessageDialogOptions = {}): Promise<string> {
  const kind = options.kind ?? "info";
  const b: Record<string, string | undefined> = typeof options.buttons === "object" && options.buttons ? options.buttons as Record<string, string | undefined> : { ok: typeof options.buttons === "string" && options.buttons !== "Ok" ? options.buttons : "OK" };
  const risky = kind === "warning" && !("yes" in b) && "cancel" in b;
  const order = ["yes", "ok", "no", "cancel"].filter((k) => b[k]);
  const def = risky ? "cancel" : order[0];
  const title = !options.title || options.title === "NC Lens" ? HEADING[kind] : options.title;
  return new Promise((resolve) => {
    const dialog = document.createElement("dialog");
    dialog.className = "adlg";
    dialog.dataset.kind = kind;
    dialog.setAttribute("aria-labelledby", "adlg-title");
    dialog.innerHTML = `<div class="ad-main"><span class="ad-icon" aria-hidden="true"><svg viewBox="0 0 24 24">${ICON[kind] ?? ICON.info}</svg></span><div class="ad-text"><h2 id="adlg-title"></h2><div class="ad-body"></div></div></div><form method="dialog"></form>`;
    dialog.querySelector("h2")!.textContent = title;
    dialog.querySelector(".ad-body")!.textContent = text;
    const form = dialog.querySelector("form")!;
    for (const k of order) {
      const button = document.createElement("button");
      button.value = button.textContent = b[k]!;
      if (k === def && k !== "cancel") button.className = "primary";
      if (k === "no" || (risky && k === "ok")) button.className = "danger";
      button.autofocus = k === def;
      form.append(button);
    }
    dialog.addEventListener("close", () => {
      dialog.remove();
      resolve(dialog.returnValue || "Cancel");
    }, { once: true });
    document.body.append(dialog);
    dialog.showModal();
    form.querySelector<HTMLButtonElement>("[autofocus]")?.focus();
  });
}

const SETTINGS_KEY = "nclens.settings.v1";
let lastReport: Report | null = null;
let wasmModule: Promise<typeof import("nclens-wasm")> | undefined;

function wasm() {
  if (import.meta.env.MODE !== "web") throw new Error("Web 版は pnpm build:web でビルドしてください");
  const [load] = Object.values(import.meta.glob<typeof import("nclens-wasm")>("../.wasm/nclens_wasm.js"));
  if (!load) throw new Error("WebAssembly がありません。pnpm build:web を実行してください");
  return wasmModule ??= load().then(async (module) => {
    await module.default();
    return module;
  }).catch((error: unknown) => {
    wasmModule = undefined;
    throw error;
  });
}

function download(name: string, blob: Blob) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function pickedFile(path: unknown): File {
  if (path instanceof File) return path;
  throw new Error("「開く」からファイルを選び直してください");
}

async function readSettingsFile(path: unknown) {
  const bytes = await pickedFile(path).arrayBuffer();
  return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
}

async function invokeWeb(command: string, args: Record<string, unknown>) {
  switch (command) {
    case "settings_load": {
      const path = "ブラウザの保存領域";
      const raw = localStorage.getItem(SETTINGS_KEY);
      return { path, fallback: false, value: raw === null ? null : JSON.parse(raw), backup: null };
    }
    case "settings_save":
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(args.value));
      return;
    case "settings_reveal":
      return message("設定の保存先：このブラウザの保存領域", { title: "設定の場所" });
    case "last_report":
      return lastReport;
  }

  const core = await wasm();
  switch (command) {
    case "run_nc": {
      const post = JSON.stringify(args.post ?? null);
      const timestamp = BigInt(Math.floor(Date.now() / 1000));
      const text = String(args.text);
      const levels = Array.isArray(args.skipLevels) ? Uint8Array.from(args.skipLevels as number[]) : null;
      const data: { result: RunResult; report: Report } = JSON.parse(levels
        ? core.run_nc_with_skip_levels(text, post, timestamp, levels)
        : core.run_nc(text, post, timestamp));
      lastReport = data.report;
      return data.result;
    }
    case "open_file":
      return JSON.parse(core.decode_file(new Uint8Array(await pickedFile(args.path).arrayBuffer())));
    case "save_file": {
      const bytes = core.encode_file(String(args.text), String(args.encoding));
      return download(String(args.path), new Blob([bytes]));
    }
    case "post_default":
      return JSON.parse(core.post_default());
    case "post_load": {
      const text = await readSettingsFile(args.path);
      return JSON.parse(isJsonText(text) ? core.parse_post_json(text) : core.parse_post(text));
    }
    case "post_save": {
      const post = JSON.stringify(args.post);
      return download(String(args.path), new Blob([isJsonPath(args.path) ? core.format_post_json(post) : core.format_post(post)], { type: "text/plain;charset=utf-8" }));
    }
    case "dialect_builtin":
      return JSON.parse(core.builtin_dialects());
    case "dialect_load": {
      const text = await readSettingsFile(args.path);
      return JSON.parse(isJsonText(text) ? core.parse_dialect_json(text) : core.parse_dialect(text));
    }
    case "dialect_save": {
      const dialect = JSON.stringify(args.dialect);
      return download(String(args.path), new Blob([isJsonPath(args.path) ? core.format_dialect_json(dialect) : core.format_dialect(dialect)], { type: "text/plain;charset=utf-8" }));
    }
    case "dialect_validate":
      return JSON.parse(core.validate_dialect(JSON.stringify(args.dialect)));
    case "macro_reference":
      return JSON.parse(core.macro_reference(JSON.stringify(args.mnemonic)));
    default:
      throw new Error(`未対応の操作：${command}`);
  }
}

export async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  return isDesktop ? nativeInvoke<T>(command, args) : await invokeWeb(command, args) as T;
}

/** 外部のリンクを開く：デスクトップ版は既定のブラウザ（Rust の open_url）、Web 版は新しいタブ */
export function openExternal(url: string) {
  if (isDesktop) void nativeInvoke("open_url", { url }).catch(() => {});
  else window.open(url, "_blank", "noopener");
}
