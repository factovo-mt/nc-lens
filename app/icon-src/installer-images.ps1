# インストーラの画像（24 ビット BMP）を nclens.svg と同じ図形で描く。出力：app/src-tauri/icons/installer/
#   NSIS：header.bmp 150x57（各画面の上）、sidebar.bmp 164x314（ようこそ・完了の画面の左）
#   MSI ：banner.bmp 493x58（各画面の上）、dialog.bmp 493x312（ようこそ・完了の画面の背景）
# 実行：powershell -ExecutionPolicy Bypass -File app/icon-src/installer-images.ps1
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$out = Join-Path $PSScriptRoot '../src-tauri/icons/installer'
New-Item -ItemType Directory -Force -Path $out | Out-Null

function Color([string]$hex) { [System.Drawing.ColorTranslator]::FromHtml($hex) }
$dark = Color '#1e2128'; $dark2 = Color '#14171c'; $gold = Color '#e5c07b'; $grey = Color '#8a93a6'
$tool = Color '#d0d6e0'; $green = Color '#8fd16a'; $red = Color '#f0707a'; $light = Color '#eef1f6'

function New-Canvas([int]$w, [int]$h) {
    $bmp = New-Object System.Drawing.Bitmap $w, $h, ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'; $g.TextRenderingHint = 'AntiAliasGridFit'; $g.InterpolationMode = 'HighQualityBicubic'
    return @{ bmp = $bmp; g = $g }
}

function Pen([System.Drawing.Color]$c, [float]$w) {
    $p = New-Object System.Drawing.Pen $c, $w
    $p.StartCap = 'Round'; $p.EndCap = 'Round'; $p.LineJoin = 'Round'
    return $p
}

# ロゴ（1024 の座標）を (x, y) に size の大きさで描く。frame：角丸の地
function Draw-Logo($g, [float]$x, [float]$y, [float]$size, [bool]$frame = $true) {
    $k = $size / 1024
    $P = { param($a, $b) New-Object System.Drawing.PointF ($x + $a * $k), ($y + $b * $k) }
    if ($frame) {
        $r = 200 * $k; $l = $x + 48 * $k; $t = $y + 48 * $k; $s = 928 * $k
        $path = New-Object System.Drawing.Drawing2D.GraphicsPath
        $path.AddArc($l, $t, 2 * $r, 2 * $r, 180, 90); $path.AddArc($l + $s - 2 * $r, $t, 2 * $r, 2 * $r, 270, 90)
        $path.AddArc($l + $s - 2 * $r, $t + $s - 2 * $r, 2 * $r, 2 * $r, 0, 90); $path.AddArc($l, $t + $s - 2 * $r, 2 * $r, 2 * $r, 90, 90)
        $path.CloseFigure()
        $g.FillPath((New-Object System.Drawing.SolidBrush $dark), $path)
    }
    $g.DrawLine((Pen $green (24 * $k)), (& $P 180 860), (& $P 180 180))
    $g.DrawLine((Pen $red (24 * $k)), (& $P 180 860), (& $P 860 860))
    $rapid = Pen $grey (40 * $k); $rapid.DashPattern = [single[]]@((70 / 40), (60 / 40)); $rapid.DashCap = 'Round'
    $g.DrawLine($rapid, (& $P 220 760), (& $P 560 760))
    $g.DrawLines((Pen $gold (64 * $k)), [System.Drawing.PointF[]]@((& $P 220 760), (& $P 220 300), (& $P 520 300), (& $P 520 560), (& $P 800 560)))
    $c = & $P 800 560; $rr = 78 * $k
    $g.FillEllipse((New-Object System.Drawing.SolidBrush $tool), $c.X - $rr, $c.Y - $rr, 2 * $rr, 2 * $rr)
}

function Fill-Gradient($g, [int]$w, [int]$h) {
    $rect = New-Object System.Drawing.Rectangle 0, 0, $w, $h
    $g.FillRectangle((New-Object System.Drawing.Drawing2D.LinearGradientBrush $rect, (Color '#2b303a'), $dark2, 60), $rect)
}

function Text($g, [string]$s, [string]$face, [float]$pt, [System.Drawing.Color]$c, [float]$x, [float]$y, [string]$style = 'Regular') {
    $f = New-Object System.Drawing.Font $face, $pt, ([System.Drawing.FontStyle]$style), ([System.Drawing.GraphicsUnit]::Pixel)
    $g.DrawString($s, $f, (New-Object System.Drawing.SolidBrush $c), $x, $y)
}
function TextC($g, [string]$s, [string]$face, [float]$pt, [System.Drawing.Color]$c, [float]$cx, [float]$y, [string]$style = 'Regular') {
    $f = New-Object System.Drawing.Font $face, $pt, ([System.Drawing.FontStyle]$style), ([System.Drawing.GraphicsUnit]::Pixel)
    $w = $g.MeasureString($s, $f).Width
    $g.DrawString($s, $f, (New-Object System.Drawing.SolidBrush $c), ($cx - $w / 2), $y)
}

function Save($c, [string]$name) {
    $c.g.Dispose(); $c.bmp.Save((Join-Path $out $name), [System.Drawing.Imaging.ImageFormat]::Bmp); $c.bmp.Dispose()
}

$ui = 'Segoe UI'; $jp = 'Yu Gothic UI'

# NSIS ヘッダー 150x57：白地の右寄せにロゴと名前（左は見出しの文言が重なるため空ける）
$c = New-Canvas 150 57; $g = $c.g
$g.Clear([System.Drawing.Color]::White)
Draw-Logo $g 100 6 45
Text $g 'NC' $ui 15 (Color '#1e2128') 30 16 'Bold'
Text $g 'Lens' $ui 15 (Color '#a26a00') 56 16 'Bold'
Save $c 'header.bmp'

# NSIS サイドバー 164x314：暗い地に大きなロゴ、名前、説明、社名
$c = New-Canvas 164 314; $g = $c.g
Fill-Gradient $g 164 314
Draw-Logo $g 22 34 120 $false
TextC $g 'NC Lens' $ui 24 $light 82 172 'Bold'
$g.FillRectangle((New-Object System.Drawing.SolidBrush $gold), 62, 208, 40, 2)
TextC $g 'マクロ展開と' $jp 12 $grey 82 222
TextC $g 'シミュレーション' $jp 12 $grey 82 240
TextC $g 'FACTOVO' $ui 10 (Color '#5c6472') 82 288
Save $c 'sidebar.bmp'

# MSI バナー 493x58：白地の右端にロゴ（左は見出しの文言）
$c = New-Canvas 493 58; $g = $c.g
$g.Clear([System.Drawing.Color]::White)
Draw-Logo $g 436 5 48
$g.FillRectangle((New-Object System.Drawing.SolidBrush $gold), 0, 56, 493, 2)
Save $c 'banner.bmp'

# MSI ダイアログ 493x312：左 164 px に暗い帯とロゴ、右は白（文言の地）
$c = New-Canvas 493 312; $g = $c.g
$g.Clear([System.Drawing.Color]::White)
$g.SetClip((New-Object System.Drawing.Rectangle 0, 0, 164, 312)); Fill-Gradient $g 164 312; $g.ResetClip()
Draw-Logo $g 22 40 120 $false
TextC $g 'NC Lens' $ui 24 $light 82 176 'Bold'
$g.FillRectangle((New-Object System.Drawing.SolidBrush $gold), 62, 212, 40, 2)
TextC $g 'FACTOVO' $ui 10 (Color '#5c6472') 82 286
Save $c 'dialog.bmp'

Get-ChildItem $out | ForEach-Object { '{0} {1}' -f $_.Name, $_.Length }
