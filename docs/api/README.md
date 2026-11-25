
# 外部連携

`nclens-cli` は NC を標準入力またはファイルから読み、解析結果を標準出力またはファイルに書きます。常駐プロセスから呼ぶときは、同じ処理をローカル HTTP API に送れます。

## CLI

```powershell
nclens-cli run work.NCD --out result.json
nclens-cli run work.NCD --include report,expanded,trace,path
nclens-cli expand work.NCD --out x.NCD --map x.map.json
nclens-cli report work.NCD --json
nclens-cli trace work.NCD --watch '#1,#100'
cmd /c "nclens-cli expand - --encoding cp932 --out x.NCD < work.NCD"
```

- 出力は UTF-8。展開 NC は LF 改行で、先頭と末尾に `%` の行を付けます。
- `-` は標準入力。文字コードは UTF-8、次に CP932 の順で判定します。`--encoding utf8|cp932` で固定できます。
- `run` は JSON を 1 個返します。バージョン 1 の形式は [JSON Schema](run-output.schema.json) を参照してください。
- `--include` の既定は `report,expanded`。診断、警告、工具、停止点、入力、ポストの情報は常に返します。ポストの `hash` は上書き後のポストを UTF-8 のコンパクトな JSON にした値の SHA-256 です。
- `expanded.src_line` は元 NC の 1 始まりの行番号。`path` と `stops` の `expanded_index` は 0 始まりです。`trace` は実行ブロックの順序と変数書き込みを返します。
- 単体の `report` は既定で簡易集計の JSON を返し、`--json` で詳細レポートを返します。

| 終了コード | 意味 |
| --- | --- |
| 0 | 成功 |
| 1 | 警告あり。結果は利用可能 |
| 2 | NC のエラー。実行できた範囲の結果を返す場合あり |
| 3 | 引数・設定のエラー |
| 4 | 入出力のエラー |

`--error-format json` で診断を標準エラーに JSON Lines で出力します。後続工程へ展開 NC を渡す前に終了コードを確認してください。

## ポストとアプリ設定

```powershell
nclens-cli post init machine.post.toml
nclens-cli post check machine.post.toml
nclens-cli settings init integration.json
nclens-cli settings check integration.json
nclens-cli settings show integration.json --set expand.optional_stop=true
nclens-cli post show --post machine.post.toml --dialect fanuc --settings integration.json --set codes.rapid_feed=5000 --format json
nclens-cli run work.NCD --post machine.post.toml --settings integration.json --set variables.work.G54.x=10 --set expand.block_skip=true
```

- 優先順位は既定、ポスト、アプリ設定、`--set` の順。後ろの指定が優先されます。
- `--post` と `--dialect` は `.toml`、`.json` に対応します。`--dialect` では同梱の `fanuc`、`osp` も選べます。
- `post show --format toml|json` の出力は設定ファイルとして読み込めます。`post init` は出力先の拡張子に合わせて初期設定を書き出します。
- `--settings` は GUI と同じ形式の JSON。CLI では `version`、`blockSkip`、`blockSkipLevels`、`optionalStop` を読みます。`blockSkip: null` ならポストのオン・オフを使います。
- `--block-skip` は /1 を有効化し、`--block-skip-levels 1,3,5` は指定番号を有効化します。番号の範囲は 1〜9 です。ポストの `[expand]` では `block_skip` がオンのときに `block_skip_levels`（既定 `[1]`）を使い、空配列なら全番号が無効です。
- HTTP の JSON 本文で `"block_skip_levels": [1, 3, 5]` を指定できます。`"block_skip": false` で全番号を無効化し、`"optional_stop": true` で M01 停止を有効にします。
- FANUC のブロック途中にスキップ用の `/` がある場合は、そこから行末までを読み飛ばし、警告を返します。マクロの式で使う除算の `/` は評価します。[FANUC プログラミングマニュアル B-63944EN/03](https://irp-cdn.multiscreensite.com/23c016fc/files/uploaded/FANUC-30i-63944EN_03-ProgManual.pdf)
- 実行結果の `block_skip_usage` に、NC 本文のスキップ番号 `level` と番号ごとの出現行数 `line_count` が入ります。対象行がなければフィールドを省略します。
- `--set` は繰り返し指定できます。数値、真偽値、配列などは JSON で記述し、`name=Machine A` のような文字列は引数全体を引用符で囲みます。同じ項目は最後の指定を採用します。
- `codes.rapid_feed` は `codes.rapid_mm_min` の別名。`variables.work.G54.x` は G54 の X オフセットです。停止スイッチは `expand.optional_stop` で指定します。
- `post show` は既定で TOML を出力し、M01 のスイッチをコメントに表示します。`--json` では `expand.optional_stop` に値を含めます。

## 一括処理と監視

```powershell
nclens-cli run C:\NC\incoming 'C:\NC\extra\*.NCD' --out-dir C:\NC\results
nclens-cli expand C:\NC\incoming --out-dir C:\NC\results
nclens-cli report C:\NC\incoming --out-dir C:\NC\results
nclens-cli watch C:\NC\incoming --out-dir C:\NC\results --post machine.post.toml
```

`run` は入力ごとに `<名前>.report.json` と `<名前>.expanded.NCD` を出力します。`--include` を指定した場合は選んだ項目の `<名前>.run.json` も出力します。`expand` と `report` はそれぞれ対応するファイルを出力します。ディレクトリ指定では直下の NC 拡張子を対象にします。異なる入力の拡張子を除いた名前が重なる場合は、その入力を失敗として報告します。

一括処理の標準出力は `input`、`status`、`code`、失敗時の任意の `message` を含む JSON 配列です。終了コードは各入力の最大値です。監視は起動時にある NC と追加・更新された NC を対象にし、500 ms 間隔の 2 回の確認で更新時刻とサイズが同じファイルを処理します。結果は 1 ファイルにつき JSON 1 行です。`Ctrl+C` で終了します。

## ローカル HTTP API

```powershell
nclens-server --port 8765
```

サーバーを起動したまま、別の端末から呼び出します。

```powershell
curl.exe http://127.0.0.1:8765/v1/health
curl.exe -H "Content-Type: text/plain" --data-binary "@work.NCD" http://127.0.0.1:8765/v1/run
curl.exe --fail -H "Content-Type: text/plain; charset=cp932" --data-binary "@work.NCD" http://127.0.0.1:8765/v1/expand -o x.NCD
```

| メソッドとパス | 応答 |
| --- | --- |
| `POST /v1/run` | CLI の `run` と同形式の JSON |
| `POST /v1/expand` | 展開 NC の `text/plain` |
| `POST /v1/report` | 詳細レポート JSON |
| `GET /v1/health` | `{"status":"ok"}` |
| `GET /v1/dialects` | 組み込み方言の設定一覧 |

POST の本文には NC のバイト列、または UTF-8 の JSON を送ります。JSON の `nc` は文字列、`post` は TOML または JSON の設定を入れた文字列、`set` は項目名と値の対応表、`include` は項目名の配列です。JSON の `encoding` には、文字列に変換する前の文字コードを指定できます。省略時は UTF-8 として記録します。CP932 のバイト列には `text/plain; charset=cp932` を指定できます。

待ち受けは既定で `127.0.0.1:8765`。`--host` でアドレスを変えると警告を出します。認証と CORS は無効で、Origin ヘッダー付きの POST は 403 を返します。本文の既定上限は 20,000,000 バイトで、`--max-body` で変更できます。`--timeout` の既定値は 30 秒。本文の受信から結果を組み立てるまでの時間を制限します。タイムアウト後も開始済みの計算は完了まで処理枠を使います。

- 成功・警告は HTTP 200。NC のエラーは 422。 `run` と `report` は解析できた範囲の結果を返します。
- 設定や JSON のエラーは 400、タイムアウトは 408、本文超過は 413、未対応の形式は 415、処理枠の上限は 503。
- 詳細は [OpenAPI](openapi.yaml) を参照してください。

### Python から呼び出す

加工データを管理するソフトから、ローカル API に NC と作業座標オフセットを渡す例です。

```python
import json
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen

payload = {
    "nc": Path("work.NCD").read_text(encoding="cp932"),
    "encoding": "cp932",
    "set": {"variables.work.G54.x": 10},
    "include": ["report", "expanded"],
}
request = Request(
    "http://127.0.0.1:8765/v1/run",
    data=json.dumps(payload).encode("utf-8"),
    headers={"Content-Type": "application/json"},
)
try:
    with urlopen(request, timeout=35) as response:
        result = json.load(response)
except HTTPError as error:
    raise RuntimeError(json.load(error)) from error
print(result["report"]["totals"])
```

## GUI の起動引数

```powershell
.\nclens.exe 'C:\NC Jobs\work.NCD' --post machine.post.toml --settings integration.json --set variables.work.G54.x=10 --set expand.block_skip=true
```

指定した NC を開き、ポストとスイッチの設定を反映して実行します。`--post`、`--settings`、`--set` は起動したセッションに適用します。これらを指定したセッションではアプリ設定の自動保存を止めます。

## 依存

- 一括処理は標準ライブラリのスレッド、監視は標準ライブラリのポーリングを使用。グロブと SHA-256 はロック済みの `glob` と `sha2` を使用。
- HTTP は [axum 0.8.9](https://docs.rs/crate/axum/0.8.9) に固定。MIT。HTTP/1.1、ルーティング、本文上限の機能を使い、タイムアウトには既存の Tokio を使用。
