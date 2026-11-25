// 公開用のリンク（ヘルプのリンクのページ・メニューのヘルプ）。URL はここだけに置く。
// 開発用の URL・非公開のリポジトリは載せない。
export const LINKS = {
  web: "https://nclens.factovo-works.com",
  download: "https://github.com/factovo-mt/nc-lens/releases/latest",
  source: "https://github.com/factovo-mt/nc-lens",
  issues: "https://github.com/factovo-mt/nc-lens/issues",
} as const;

/** ヘルプのリンクのページ：[キー, 名前, 説明] */
export const LINK_ROWS: [keyof typeof LINKS, string, string][] = [
  ["web", "Web 版", "ブラウザで利用。NC はブラウザ内で処理、送信なし"],
  ["download", "Windows 版のダウンロード", "インストーラ・MSI・zip"],
  ["source", "ソースコード（GitHub）", "MIT ライセンス"],
  ["issues", "不具合の報告・要望", "GitHub の Issues"],
];
