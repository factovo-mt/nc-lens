import { readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../dist-web/", import.meta.url));
const maxBytes = 25 * 1024 * 1024;
const maxFiles = 20_000;
let count = 0;
let largest = 0;

function check(dir) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      check(path);
      continue;
    }
    if (!entry.isFile()) throw new Error(`通常のファイルではありません: ${path}`);
    const { size } = statSync(path);
    if (size >= maxBytes) throw new Error(`25 MiB 以上: ${path} (${size} bytes)`);
    largest = Math.max(largest, size);
    count++;
  }
}

check(root);
if (count === 0 || count >= maxFiles) throw new Error(`配布ファイル数が範囲外: ${count}`);
console.log(`Web: ${count} files, largest ${largest} bytes`);
