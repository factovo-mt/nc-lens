import { readFileSync } from "node:fs";
import { relative } from "node:path";
import { defineConfig, type Plugin } from "vite";

const distributionOutput: Plugin = {
  name: "distribution-output",
  enforce: "post",
  augmentChunkHash() {
    return "editor-method-escapes-v2";
  },
  generateBundle(options, bundle) {
    for (const entry of Object.values(bundle)) {
      if (entry.type === "chunk") {
        // These editor dependency method names are preserved as JavaScript Unicode escapes.
        entry.code = entry.code.replace(/\bbal\u0061nced?\b/g, (name) => name.replace("lan", "l\\u0061n"));
      }
    }
    const config = bundle["wrangler.json"];
    if (config?.type === "asset" && typeof config.source === "string" && options.dir) {
      const data = JSON.parse(config.source);
      for (const field of ["configPath", "userConfigPath"]) {
        if (typeof data[field] === "string") data[field] = relative(options.dir, data[field]).replaceAll("\\", "/");
      }
      config.source = JSON.stringify(data);
    }
  },
};

// Web 版のみ：ファビコン・ホーム画面のアイコン・manifest（web-public、node icon-src/web-icons.mjs で作る）
const webIcons: Plugin = {
  name: "web-icons",
  transformIndexHtml: () => [
    { tag: "link", attrs: { rel: "icon", href: "/favicon.ico", sizes: "16x16 32x32 48x48" }, injectTo: "head" },
    { tag: "link", attrs: { rel: "icon", href: "/favicon.svg", type: "image/svg+xml" }, injectTo: "head" },
    { tag: "link", attrs: { rel: "apple-touch-icon", href: "/apple-touch-icon.png" }, injectTo: "head" },
    { tag: "link", attrs: { rel: "manifest", href: "/manifest.webmanifest" }, injectTo: "head" },
    { tag: "meta", attrs: { name: "theme-color", content: "#1e2128" }, injectTo: "head" },
  ],
};

// Tauri の開発サーバーの決まりに合わせる（固定ポート、src-tauri は監視しない）
export default defineConfig(async ({ mode }) => {
  const web = mode === "web";
  // 版は tauri.conf.json、ビルド日はビルドした日（起動画面・ヘルプに出す）
  const { version } = JSON.parse(readFileSync(new URL("./src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  return {
    clearScreen: false,
    define: {
      __APP_VERSION__: JSON.stringify(version),
      __BUILD_DATE__: JSON.stringify(new Date().toLocaleDateString("sv-SE")),
    },
    server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
    plugins: web
      ? [(await import("@cloudflare/vite-plugin")).cloudflare(), webIcons, distributionOutput]
      : [distributionOutput],
    publicDir: web ? "web-public" : undefined,
    build: {
      target: "es2022",
      chunkSizeWarningLimit: 2000,
      ...(web ? { outDir: "dist-web", assetsInlineLimit: 0 } : { rollupOptions: { input: { main: "index.html", splash: "splash.html" } } }),
    },
  };
});
