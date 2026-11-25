# NC Lens サンプル

NC Lensでマクロ展開と工具経路を確認するサンプル。同じ番号のFANUC版（`fanuc/*.NCD`）とOSP版（`osp/*.MIN`）で経路を比較。

| ファイル名 | 展開前の見どころ | 展開後の見どころ |
| --- | --- | --- |
| 01-bolt-circle | 半径と中心を引数で指定。FANUC は `G65` と三角関数、OSP は `CALL` と 60 度ずつの回転式 | X45・Y10 から始まる六つの穴位置と、各位置での Z−4 への送り |
| 02-pocket-zigzag | 行ピッチ 4 mm の増分と X 端点の入れ替え。FANUC は `WHILE`、OSP は条件分岐で反復 | Y3・7・11・15・19 の五つの行で X5 と X29 を交互に往復 |
| 03-modal-holes | FANUC は `G66/G67`、OSP は `MODIN/MODOUT` で呼び出しの範囲を指定 | 四つの XY 位置に Z−3.5 と Z−7 の二段送りを挿入。解除後の原点移動では呼び出しなし |
| 04-nested-repeat | 列ごとのサブから刻み用サブを呼び出し。FANUC は `M98 L`、OSP は `CALL` と回数カウンター | Y6 と Y18 で三つずつ、計六つの刻み |
| 05-tool-branch | `IF` と `GOTO` で工具、回転数、送りを選択 | T3・F85 の加工に続いて T1・F130 の加工 |
| 06-arithmetic | 四則と括弧の優先順位。FANUC は `ROUND`・`FIX`・`SQRT`・`ATAN`、OSP は同じ値を四則で計算 | X14 と X20 で括弧の有無を比較。Y8 と Y7 で FANUC の丸めと切り捨ての差を確認 |
| 07-optional-blocks | `/1`〜`/3` で座標の加算と移動を切り替え、`M01` で区切り | 既定は X34。レベル 1・3 をスキップすると X14 になり、任意停止を有効にすると `M01` までを出力 |
| 08-cutter-comp | 同じ寸法で `G41` と `G42`、ポストの D3 に半径 3 mm を指定 | 展開 NC は寸法を数値化して補正指令を保持。経路表示では左補正と右補正の向きを比較 |

ポストは `posts/fanuc-sample.post.toml` と `posts/osp-sample.post.toml`。両方のポストに工具1〜3とワーク原点X12・Y8・Z0を設定。原点の指定はFANUCが `G54`、OSPが `G15 H1`。早送りはFANUCが6,000 mm/min、OSPが7,500 mm/min。

リポジトリのルートで展開ブロックを確認するコマンド。

```powershell
cargo run -p nclens-cli -- run samples/fanuc/06-arithmetic.NCD --post samples/posts/fanuc-sample.post.toml --include expanded
cargo run -p nclens-cli -- run samples/osp/07-optional-blocks.MIN --post samples/posts/osp-sample.post.toml --include expanded --block-skip-levels 1,3 --optional-stop
```

期待値は `tests/samples/` に保存。07 は既定とレベル 1・3 のスキップおよび任意停止を有効にした場合の両方を照合。
