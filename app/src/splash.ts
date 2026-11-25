// 起動画面：版を出し、本体の読み込み状況（main.ts の splashStatus）を受けて表示する。閉じるのは Rust（app_ready）。
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

document.querySelector("#ver")!.textContent = `版 ${__APP_VERSION__}`;
// 最初の描画の後に知らせる（最短の表示時間はここから数える）
requestAnimationFrame(() => requestAnimationFrame(() => void invoke("splash_shown").catch(() => {})));
void listen<string>("splash-status", (e) => {
  document.querySelector("#status")!.textContent = e.payload;
});
