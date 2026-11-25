// Web 版のファビコン・ホーム画面のアイコンを nclens.svg（Windows 版と同じ元絵）から作る。出力：app/web-public/
//   favicon.ico（16・32・48）、favicon.svg、apple-touch-icon.png（180）、icon-192.png、icon-512.png
// 実行：node app/icon-src/web-icons.mjs（PNG は tauri icon で描く。依存の追加なし）
import { execSync } from "node:child_process";
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const app = fileURLToPath(new URL("../", import.meta.url));
const svg = join(app, "icon-src/nclens.svg");
const out = join(app, "web-public");
const tmp = mkdtempSync(join(tmpdir(), "nclens-icons-"));
try {
  execSync(`pnpm exec tauri icon "${svg}" -o "${tmp}" -p 16,32,48,180,192,512`, { cwd: app, stdio: "inherit" });
  const png = (n) => readFileSync(join(tmp, `${n}x${n}.png`));

  // ICO：PNG をそのまま格納（ヘッダ 6 バイト＋項目 16 バイト×枚数＋PNG）
  const sizes = [16, 32, 48];
  const images = sizes.map(png);
  const head = Buffer.alloc(6 + 16 * sizes.length);
  head.writeUInt16LE(1, 2);
  head.writeUInt16LE(sizes.length, 4);
  let offset = head.length;
  sizes.forEach((n, i) => {
    const e = 6 + 16 * i;
    head.writeUInt8(n, e);
    head.writeUInt8(n, e + 1);
    head.writeUInt16LE(1, e + 4);
    head.writeUInt16LE(32, e + 6);
    head.writeUInt32LE(images[i].length, e + 8);
    head.writeUInt32LE(offset, e + 12);
    offset += images[i].length;
  });
  writeFileSync(join(out, "favicon.ico"), Buffer.concat([head, ...images]));

  copyFileSync(join(tmp, "180x180.png"), join(out, "apple-touch-icon.png"));
  copyFileSync(join(tmp, "192x192.png"), join(out, "icon-192.png"));
  copyFileSync(join(tmp, "512x512.png"), join(out, "icon-512.png"));
  writeFileSync(join(out, "favicon.svg"), readFileSync(svg, "utf8").replace(/\s*<!--[\s\S]*?-->/g, ""));
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
