import { execFileSync } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const target = "wasm32-unknown-unknown";
const bindgen = join(root, "scratch", "wasm-bindgen", "bin", process.platform === "win32" ? "wasm-bindgen.exe" : "wasm-bindgen");

try {
  execFileSync(bindgen, ["--version"], { stdio: "inherit" });
} catch {
  console.error("wasm-bindgen CLI は app/README.md の「Web 版」の手順で scratch に導入してください。");
  process.exit(1);
}

execFileSync("cargo", ["build", "--locked", "--release", "-p", "nclens-wasm", "--target", target], { cwd: root, stdio: "inherit" });
const { target_directory: targetDir } = JSON.parse(execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], { cwd: root, encoding: "utf8" }));
execFileSync(bindgen, [
  "--target", "web",
  "--out-dir", join(root, "app", ".wasm"),
  join(targetDir, target, "release", "nclens_wasm.wasm"),
], { cwd: root, stdio: "inherit" });
