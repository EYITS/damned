$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$packagePath = Join-Path $repoRoot 'dist/damned-windows-x64'
$exePath = Join-Path $repoRoot 'target/release/damned.exe'
if (!(Test-Path -LiteralPath $exePath)) { throw "EXE bulunamadı: $exePath" }
New-Item -ItemType Directory -Path $packagePath -Force | Out-Null
Copy-Item -LiteralPath $exePath -Destination $packagePath
Copy-Item -LiteralPath (Join-Path $repoRoot 'README.md') -Destination $packagePath
Copy-Item -LiteralPath (Join-Path $repoRoot 'LICENSE') -Destination $packagePath

# Slint's software renderer embeds the compiled UI; no Qt/Slint DLL or assets.
# Ship the compiler's matching app-local VC runtime rather than assuming the
# destination PC already has the Visual C++ redistributable installed.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (!(Test-Path -LiteralPath $vswhere)) { throw 'vswhere bulunamadı; VC runtime paketlenemedi.' }
$vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$vsPath) { throw 'Visual C++ araçları bulunamadı.' }
$runtime = Get-ChildItem -Path (Join-Path $vsPath 'VC/Redist/MSVC/*/x64/Microsoft.VC*.CRT') -Directory |
    Sort-Object FullName -Descending | Select-Object -First 1
if (!$runtime) { throw 'x64 VC CRT dizini bulunamadı.' }
$dlls = @(Get-ChildItem -LiteralPath $runtime.FullName -Filter '*.dll')
if ($dlls.Count -eq 0) { throw 'VC runtime DLL listesi boş.' }
$dlls | Copy-Item -Destination $packagePath
Get-ChildItem -LiteralPath $packagePath -File | Where-Object Name -ne 'SHA256SUMS.txt' | Get-FileHash -Algorithm SHA256 |
    ForEach-Object { '{0}  {1}' -f $_.Hash, (Split-Path -Leaf $_.Path) } |
    Set-Content -LiteralPath (Join-Path $packagePath 'SHA256SUMS.txt') -Encoding utf8
