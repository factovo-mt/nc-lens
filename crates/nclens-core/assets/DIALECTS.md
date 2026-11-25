# 方言の設定

`PostProfile.dialect`に方言を保存する。CLIでは `--dialect fanuc`、`--dialect osp`、または `--dialect sample.dialect.toml`で選べる。`.post.toml`の`[dialect]`を省略した場合は FANUCを使う。

次は `U` を X 軸、`G101.5` を直線補間として扱う例。

```toml
name = "sample"
base = "fanuc"
macro = "fanuc"

[addresses]
U = "X"
K = "Repeat"

[g_codes]
"101.5" = { function = "linear", group = 1 }

[m_codes]
"70" = { function = "call", program = 9001 }

[custom]
G120 = { call = 9002 }
M200 = { replace = "M08 G04 P1000" }
Z999 = { replace = "G00 U10." }
```

`g_codes` と `m_codes` のキーは数字のみ。機能名と既定の群番号は [FANUC の定義](fanuc.dialect.toml) を参照。`group` の省略または 0 は、モーダル情報に記録しないコードを表す。`ignore` は実行と展開の両方から除く。`unsupported` を指定した G コードは診断を出してブロックを飛ばす。動的工具長補正を設定する場合は `length_dynamic` を使う。Z の値で工具長を設定し、G43、G44、G49 で解除する。

`custom` のキーはアドレスと数値の組み合わせ。置き換えは同じブロック内で行い、置き換えたワードにも方言の定義を適用する。`DwellSeconds` はドウェルの秒、`Repeat` は繰り返し回数の役割を割り当てる。ポスト設定で無視するよう指定したコードは、方言の定義より優先する。

## OSP の対応範囲

OSP-P200M/P20M の解説書 5228-E の第10章と第11章を参照した。対象は `VC1` 以降のコモン変数、名前付きローカル変数、四則演算と比較、`IF [条件] Nラベル`、`GOTO Nラベル`、`CALL O名前 引数=値`、`RTS`、`MODIN` と `MODOUT`。各プログラムは同じ NC テキストに記述する。 [Okuma CNC Programming Manual](https://studylib.net/doc/25445131/okuma-osp-p200m-p200ma-p20m-p200m-r-p200ma-r-p20m-r-progm...)

`G15 H1`〜`H6` のワーク座標、`G04 F` の秒指定、`G56 H` の Z 軸工具長補正を暫定実装した。実機による照合は未実施。整数入力の単位と工具長の設定はポストに従う。`MODIN` は移動ブロックの後に呼び出し、名前付き引数は指令時に評価する。機種やオプションによる違いは未確認。

範囲外はOSPの全関数、ユーザタスク2の入出力や外部プログラムの登録など、OSP固有の固定サイクルと旋盤の複合サイクル。OSPの検査NC4本と期待値は `tests/osp/`と`tests/dialect.rs`に収録した。

## モーダル情報とレポート

`ExpandedLine.modal` に展開行ごとの状態を保持する。XYZ は mm、ABC は度。XYZ は現在のワーク座標系で表す。機械の回転速度設定がないため、ABC の回転は位置と範囲に記録する。距離と時間の推計には直線軸を用いる。

`nclens-cli report --json path/to/program.nc`で詳細レポートを出す。時刻はUTCのUnix秒。ワーク座標と機械座標の範囲をそれぞれ出す。工具とプログラムの使用行は元ファイルの1始まりの行番号。ドウェル、工具交換、サイクルの補正時間は「その他」の時間に加える。
送りや主軸回転数が未指定で切削時間を求められない場合は、そのブロックを警告件数に加える。
