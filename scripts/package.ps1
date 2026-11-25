#Requires -Version 7.0
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$config = Get-Content -Raw -Encoding UTF8 (Join-Path $root 'app/src-tauri/tauri.conf.json') | ConvertFrom-Json
$name = $config.productName
$version = $config.version
$distributionName = $name.Replace(' ', '_')
if ($name -notmatch '^[\p{L}\p{N}_-][\p{L}\p{N}_. -]*$' -or
    $version -notmatch '^\d+\.\d+\.\d+([+.-][A-Za-z0-9.+-]+)?$') {
    throw 'productName or version is unsuitable for a distribution path.'
}

Push-Location $root
try {
    $metadata = cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json -Depth 30
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed.' }
    $manifest = Join-Path $root 'app/src-tauri/Cargo.toml'
    $package = $metadata.packages | Where-Object { $_.manifest_path -eq $manifest }
    $bins = @($package.targets | Where-Object { $_.kind -contains 'bin' })
    if ($bins.Count -ne 1) { throw 'Expected one Tauri binary target.' }
    $target = 'x86_64-pc-windows-msvc'
    Push-Location app
    try {
        pnpm install --frozen-lockfile
        if ($LASTEXITCODE -ne 0) { throw 'pnpm install failed.' }
        pnpm tauri build --target $target --bundles nsis --ci -- --locked
        if ($LASTEXITCODE -ne 0) { throw 'Tauri release build failed.' }
    } finally {
        Pop-Location
    }

    $release = Join-Path $metadata.target_directory "$target/release"
    $binaryName = $config.mainBinaryName
    if (-not $binaryName) { $binaryName = $bins[0].name }
    $installer = Join-Path $release "bundle/nsis/${distributionName}_${version}_x64-setup.exe"
    $exe = Join-Path $release "$binaryName.exe"

    $stem = "${distributionName}_${version}"
    $dist = Join-Path $root 'dist'
    $output = Join-Path $dist $stem
    $portable = Join-Path $output 'portable'
    New-Item -ItemType Directory -Force -Path $portable | Out-Null
    Copy-Item -LiteralPath $installer -Destination $output
    Copy-Item -LiteralPath $exe -Destination (Join-Path $portable "$binaryName.exe")
    $assets = 'default.post.toml', 'fanuc.dialect.toml', 'osp.dialect.toml'
    foreach ($asset in $assets) {
        Copy-Item -LiteralPath (Join-Path $root "crates/nclens-core/assets/$asset") -Destination $portable
    }
    # Samples next to the exe: the build stages them in gen/samples without the local-only _ folders.
    $samples = Join-Path $portable 'samples'
    if (Test-Path -LiteralPath $samples) { Remove-Item -LiteralPath $samples -Recurse -Force }
    Copy-Item -LiteralPath (Join-Path $root 'app/src-tauri/gen/samples') -Destination $samples -Recurse
    $sampleItems = @(Get-ChildItem -LiteralPath $samples -Recurse -File |
        ForEach-Object { 'portable/samples/' + [System.IO.Path]::GetRelativePath($samples, $_.FullName).Replace('\', '/') })
    if ($sampleItems.Count -eq 0 -or ($sampleItems | Where-Object { $_ -match '/_' })) { throw 'Unexpected sample payload.' }
    foreach ($file in 'LICENSE', 'LICENSE-ja.txt', 'THIRD_PARTY_LICENSES.md', 'CHANGELOG.md') {
        Copy-Item -LiteralPath (Join-Path $root $file) -Destination $output
    }
    Copy-Item -LiteralPath (Join-Path $root 'docs/user/README_配布用.md') -Destination $output

    # Build the ZIP from the known payload even when dist contains earlier output.
    $leaf = Split-Path -Leaf $installer
    $items = @($leaf, 'LICENSE', 'LICENSE-ja.txt', 'THIRD_PARTY_LICENSES.md', 'CHANGELOG.md', 'README_配布用.md', "portable/$binaryName.exe")
    $items += $assets | ForEach-Object { "portable/$_" }
    $items += $sampleItems
    $zip = Join-Path $dist "$stem.zip"
    $zipTemp = Join-Path $dist "$stem.$([guid]::NewGuid()).tmp"
    try {
        $archive = [System.IO.Compression.ZipFile]::Open($zipTemp, [System.IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($item in $items) {
                [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, (Join-Path $output $item), $item) | Out-Null
            }
        } finally {
            $archive.Dispose()
        }
        Move-Item -LiteralPath $zipTemp -Destination $zip -Force
    } finally {
        if (Test-Path -LiteralPath $zipTemp) { Remove-Item -LiteralPath $zipTemp }
    }
    $hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $stem.zip" | Set-Content -Encoding UTF8 "$zip.sha256"
    Write-Output "$hash  $zip"
} finally {
    Pop-Location
}
