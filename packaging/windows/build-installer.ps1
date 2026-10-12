# Windows 用のインストーラー(Inno Setup)を作る。事前に cargo build --release を済ませておく。
# 使い方: ./packaging/windows/build-installer.ps1 [-Version v0.2.4]
# 出力: target/installer/ClipSticky-v<版>-windows-x64-setup.exe と .sha256
param([string]$Version = "")

$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")

if ($Version -eq "") {
    $Version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches.Groups[1].Value
}
$Version = $Version.TrimStart("v")

$iscc = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $iscc) {
    # GitHub Actions のランナーなど、Chocolatey がある環境ではそこから入れる
    choco install innosetup -y --no-progress
    $iscc = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
}

# Inno Setup は、スクリプトの場所を基準に相対パスを解く。そのため、絶対パスで渡す
& $iscc /Qp "/DAppVersion=$Version" "/DSourceExe=$PWD\target\release\clipsticky.exe" "/DOutputDir=$PWD\target\installer" packaging\windows\clipsticky.iss
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Get-ChildItem target\installer\*.exe | ForEach-Object {
    (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower() + "  " + $_.Name | Out-File ($_.FullName + ".sha256") -Encoding ascii
    Write-Host "作成しました: $($_.FullName)"
}
