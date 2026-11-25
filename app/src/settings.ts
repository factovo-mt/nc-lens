// アプリの設定（表示の状態）：exe のフォルダの nclens.settings.json（書けなければ %APPDATA%\NC Lens\）。
// 読み書きは platform に任せ、Web 版ではブラウザの保存領域を使う。
// 変更のたびに save() を呼ぶと、間引いて（1 秒）書く。終了時は flush() ですぐ書く。
import { invoke } from "./platform";
import type { DialectSettings, PostProfile } from "./types";

export type ThemeMode = "auto" | "light" | "dark";
export interface PortState { az: number; el: number; scale: number; target: [number, number, number] }
export interface ViewState { layout: "single" | "quad"; active: "xy" | "yz" | "xz" | "3d"; maximized: boolean; split?: { x: number; y: number }; ports: Record<string, PortState> }

/** モーダルパネルに出せる項目 */
export const MODAL_ITEMS = ["g", "f", "s", "t", "h", "d", "work", "plane", "abs", "pos", "range"] as const;
export type ModalItem = (typeof MODAL_ITEMS)[number];

/** ポスト・コード定義のセット：ポスト（方言を含む）と、結び付いたポストのファイル（TOML・JSON）（無ければ null） */
export interface PostSet { profile: PostProfile; path: string | null }

/** 浮動ウィンドウの位置と大きさ（CSS px） */
export interface WinRect { x: number; y: number; w: number; h: number; max: boolean }

export interface AppSettings {
  version: 1;
  /** 物理画素。起動時に Rust が戻す（settings.rs の restore_window） */
  window: { x: number; y: number; width: number; height: number; maximized: boolean } | null;
  /** エディタの幅（作業域に対する割合）と下部パネルの高さ（px） */
  split: { editorW: number; bottomH: number };
  /** スマホ（Web 版）の描画の大きさ：縦向きの高さと横向きの幅（画面に対する割合。0.25〜0.7） */
  mobile: { viewH: number; viewW: number };
  /** 狭い幅の配置（デスクトップ版）：auto は幅に応じてスマホ・タブレットの配置、normal は常に通常の配置 */
  narrowLayout: "auto" | "normal";
  view: ViewState | null;
  theme: ThemeMode;
  /** 文字の倍率（1 ＝ 既定） */
  zoom: { editor: number; expanded: number; modal: number; peek: number };
  bottomTab: string;
  /** height：モーダルパネルの中身の高さの上限（px）。超えたら縦スクロール */
  modal: { visible: boolean; collapsed: boolean; items: ModalItem[]; height: number };
  recent: { files: string[]; posts: string[] };
  /** ポスト・コード定義のセットの一覧と、実行に使うセットの名前（既定） */
  posts: PostSet[];
  currentPost: string;
  /** 旧版（W12 まで）の方言の一覧・既定の方言・最後のポスト。起動時にセットの一覧へ移して消す */
  dialects?: DialectSettings[];
  defaultDialect?: string;
  lastPost?: string | null;
  playSpeed: number;
  /** 等幅の字体（空は既定。CSS の font-family の書き方） */
  fontMono: string;
  /** 右のピーク（使用コード一覧）の開閉と幅 */
  /** group：左のアイコンで選んだグループ（all・T・D・H・G・M） */
  peek: { open: boolean; width: number; group: string };
  /** 完成形の薄表示（ゴースト） */
  ghost: boolean;
  /** 完成形の薄さ（0＝既定の濃さ〜0.85）と色（空はテーマの既定） */
  ghostFade: number;
  ghostColor: string;
  optionalStop: boolean;
  /** null の間はポストの既定を使う */
  blockSkip: boolean | null;
  /** 有効にするブロックスキップの番号（/1〜/9。/ は /1）。null の間はセットの expand.block_skip_levels */
  blockSkipLevels: number[] | null;
  /** 下部の表ごとの列の順・非表示・幅（coltable.ts） */
  tables: Record<string, { order?: string[]; hidden?: string[]; width?: Record<string, number> }>;
  /** 浮動ウィンドウ（アプリ設定・セット・レポート）ごとの位置と大きさ */
  windows: Record<string, WinRect>;
  /** 起動画面（スプラッシュ）の表示。Rust が起動時に読む（settings.rs の splash） */
  splash: boolean;
  /** 開くダイアログの最初のフォルダ（空は同梱のサンプルのフォルダ） */
  defaultDir: string;
}

export const defaults = (): AppSettings => ({
  version: 1,
  window: null,
  split: { editorW: 0.42, bottomH: 220 },
  mobile: { viewH: 0.45, viewW: 0.5 },
  narrowLayout: "auto",
  view: null,
  theme: "auto",
  zoom: { editor: 1, expanded: 1, modal: 1, peek: 1 },
  bottomTab: "expanded",
  modal: { visible: true, collapsed: false, items: [...MODAL_ITEMS], height: 220 },
  recent: { files: [], posts: [] },
  posts: [],
  currentPost: "",
  playSpeed: 10,
  fontMono: "",
  peek: { open: false, width: 235, group: "all" },
  ghost: true,
  ghostFade: 0,
  ghostColor: "",
  optionalStop: false,
  blockSkip: null,
  blockSkipLevels: null,
  tables: {},
  windows: {},
  splash: true,
  defaultDir: "",
});

interface Loaded { path: string; fallback: boolean; value: Partial<AppSettings> | null; backup: string | null }

export const settings: AppSettings = defaults();
export const place = { path: "", fallback: false, backup: null as string | null };

/** 読んだ値を既定に重ねる（項目が欠けていても、型の違う値でも既定で補う）。 */
function merge<T>(base: T, v: unknown): T {
  if (v === undefined || v === null) return base;
  if (base === null) return v as T;
  if (Array.isArray(base)) return (Array.isArray(v) ? v : base) as T;
  if (typeof base === "object") {
    if (typeof v !== "object" || Array.isArray(v)) return base;
    const out: Record<string, unknown> = { ...(base as Record<string, unknown>) };
    for (const [k, x] of Object.entries(v as Record<string, unknown>)) out[k] = k in out ? merge(out[k], x) : x;
    return out as T;
  }
  return (typeof v === typeof base ? v : base) as T;
}

/** 起動時に保存済みの設定を読む。 */
export async function loadSettings() {
  let r: Loaded;
  try {
    r = await invoke<Loaded>("settings_load");
  } catch {
    return; // 保存領域を読めなければ既定のまま
  }
  Object.assign(place, { path: r.path, fallback: r.fallback, backup: r.backup });
  if (r.value) Object.assign(settings, merge(defaults(), r.value));
}

let timer = 0;
let lastError = "";
/** 変更を知らせる。1 秒間引いて書く。 */
export function save() {
  clearTimeout(timer);
  timer = window.setTimeout(() => void flush(), 1000);
}

/** すぐ書く。書けなければ文言を返す（同じ誤りは 1 度だけ）。 */
export async function flush(): Promise<string> {
  clearTimeout(timer);
  try {
    await invoke("settings_save", { value: settings });
    lastError = "";
    return "";
  } catch (e) {
    const msg = `設定を保存できません：${e}`;
    if (msg === lastError) return "";
    lastError = msg;
    return msg;
  }
}

/** 表示の状態を既定へ戻す（セット・最近使ったファイル・ポストは残す）。 */
export function resetView() {
  const { recent, posts, currentPost, optionalStop, blockSkip, blockSkipLevels, splash, defaultDir } = settings;
  const keep = { recent, posts, currentPost, optionalStop, blockSkip, blockSkipLevels, splash, defaultDir };
  Object.assign(settings, defaults(), keep);
}

/** 最近使ったファイルの先頭に入れる（10 件まで）。 */
export function pushRecent(list: string[], path: string) {
  const i = list.findIndex((p) => p.toLowerCase() === path.toLowerCase());
  if (i >= 0) list.splice(i, 1);
  list.unshift(path);
  list.length = Math.min(list.length, 10);
  save();
}
