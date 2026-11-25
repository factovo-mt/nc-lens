import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const app = fileURLToPath(new URL("../", import.meta.url));
const root = dirname(app.replace(/[\\/]$/, ""));
const args = process.argv.slice(2);
const bundles = process.platform === "win32"
  && ["build", "bundle"].includes(args[0])
  && !args.includes("--no-bundle");

if (bundles) {
  // WiX reads Unicode RTF escapes; a UTF-8 text file would be wrapped as ANSI RTF.
  const text = readFileSync(join(root, "LICENSE-ja.txt"), "utf8")
    .replace(/\r?\n/g, "\n")
    .replace(/[\\{}]/g, "\\$&")
    .replace(/[^\x00-\x7f]/g, (char) => {
      const code = char.charCodeAt(0);
      return `\\u${code < 32768 ? code : code - 65536}?`;
    })
    .replace(/\n/g, "\\par\n");
  const scratch = join(root, "scratch");
  mkdirSync(scratch, { recursive: true });
  writeFileSync(join(scratch, "nclens-license.rtf"), `{\\rtf1\\ansi\\ansicpg1252\\deff0\\uc1{\\fonttbl{\\f0 MS Gothic;}}\\f0\\fs21\n${text}}\n`);
}

const cli = fileURLToPath(import.meta.resolve("@tauri-apps/cli/tauri.js"));
try {
  execFileSync(process.execPath, [cli, ...args], { cwd: app, stdio: "inherit" });
} catch (error) {
  if (typeof error.status !== "number") throw error;
  process.exit(error.status);
}

if (bundles) {
  const options = args.slice(1, args.includes("--") ? args.indexOf("--") : args.length);
  const option = (long, short) => options.find((arg) => arg.startsWith(`${long}=`))?.slice(long.length + 1)
    ?? options.find((_, index) => options[index - 1] === long || options[index - 1] === short);
  const { target_directory: targetDirectory } = JSON.parse(execFileSync(
    "cargo", ["metadata", "--locked", "--no-deps", "--format-version", "1"], { cwd: root, encoding: "utf8" },
  ));
  const target = option("--target", "-t") ?? process.env.CARGO_BUILD_TARGET ?? "";
  const profile = options.includes("--debug") || options.includes("-d") ? "debug" : "release";
  const { productName, version } = JSON.parse(readFileSync(join(app, "src-tauri", "tauri.conf.json"), "utf8"));
  const prefix = `${productName}_${version}_`;
  const filePrefix = prefix.replaceAll(" ", "_");
  for (const kind of ["nsis", "msi"]) {
    const dir = join(targetDirectory, target, profile, "bundle", kind);
    if (!existsSync(dir) || filePrefix === prefix) continue;
    for (const file of readdirSync(dir)) {
      if (!file.startsWith(prefix) || !file.endsWith(kind === "nsis" ? "-setup.exe" : ".msi")) continue;
      const renamed = join(dir, filePrefix + file.slice(prefix.length));
      renameSync(join(dir, file), renamed);
      console.log(renamed);
    }
  }
}
