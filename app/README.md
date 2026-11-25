# NC Lens の開発ビルド

Windows でのデスクトップ版の開発に必要なもの

- Rust stable（MSVC）と Visual Studio Build Tools の C++ デスクトップ開発コンポーネント
- Node.js と pnpm
- WebView2 Runtime

リポジトリのルートから実行します。

```powershell
cd app
pnpm install --frozen-lockfile
pnpm tauri dev
pnpm build
pnpm tauri build
```

- pnpm build で TypeScript の型チェックとフロントエンドのビルド
- pnpm tauri build で実行ファイルと NSIS・MSI インストーラの作成

Rust のテストはリポジトリのルートで `cargo test --workspace` を実行します。

## Web 版

リポジトリのルートから実行します。`wasm-bindgen-cli` のバージョンはルートの `Cargo.lock` の `wasm-bindgen` に合わせます。

```powershell
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked --root scratch/wasm-bindgen
cd app
pnpm install --frozen-lockfile
pnpm build:web
```

ファビコン・ホーム画面のアイコン（`web-public` の favicon.ico・favicon.svg・apple-touch-icon.png・icon-192.png・icon-512.png）は Windows 版と同じ `icon-src/nclens.svg` から作成：`node icon-src/web-icons.mjs`。`index.html` への link は Web 版のビルドだけ（`vite.config.ts`）。

開発起動（変更の即時反映）：`pnpm dev:web` → `http://localhost:5174/`。デバッグは VS Code の「Web デバッグ（Edge）」、Zed の「Web デバッグ（Chrome）」。
