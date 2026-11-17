<#
.SYNOPSIS
  Make the Microsoft Store package (MSIX) of Margin Notes from the release build.

.DESCRIPTION
  Run scripts/build-windows.sh first. This script then does four things:

    1. It copies margin.exe, the DLLs beside it and windows/msix/Assets into a package
       folder, release/windows/layout.
    2. It writes AppxManifest.xml there, with the version and the CPU filled in.
    3. It makes resources.pri, which lets Windows pick the right logo file for each size
       and scale.
    4. It packs the folder with the winapp CLI.

  The package is not signed. The Store signs it (DECISIONS.md D61). Windows does not
  install an unsigned package, so test the package folder itself:

    winapp run release\windows\layout          # register the folder and start the app
    winapp unregister release\windows\layout   # remove it, with its app data

  docs/development.md, "The Store package", says where a packaged run writes.

.PARAMETER Cpu
  Pack the CPU-only build (build-windows.sh --cpu) in place of the Store build. It has
  no Vulkan loader.

.PARAMETER BuildDir
  The folder with margin.exe and its DLLs. Default: src-tauri/target/vulkan/release, or
  src-tauri/target/release with -Cpu.

.PARAMETER Arch
  The CPU of the build: x64 or arm64.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\package-msix.ps1
#>
[CmdletBinding()]
param(
  [switch]$Cpu,
  [string]$BuildDir,
  [ValidateSet('x64', 'arm64')]
  [string]$Arch = 'x64'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $BuildDir) {
  $BuildDir = Join-Path $root $(if ($Cpu) { 'src-tauri\target\release' } else { 'src-tauri\target\vulkan\release' })
}

if (-not (Get-Command winapp -ErrorAction SilentlyContinue)) {
  throw 'The winapp CLI is not installed. Get it with: winget install Microsoft.WinAppCli'
}

# The package version is (major + 1).minor.patch.0. The Store refuses a first part of 0,
# and it keeps the fourth part for itself (DECISIONS.md D61).
$conf = Get-Content (Join-Path $root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json
if ($conf.version -notmatch '^(\d+)\.(\d+)\.(\d+)$') {
  throw "The version in tauri.conf.json is not three numbers: $($conf.version)"
}
$version = '{0}.{1}.{2}.0' -f ([int]$Matches[1] + 1), [int]$Matches[2], [int]$Matches[3]

# build-windows.sh puts these beside margin.exe. A package without one of them starts on
# the build PC and fails on a clean PC.
$files = @('margin.exe', 'DirectML.dll', 'msvcp140.dll', 'msvcp140_1.dll', 'vcruntime140.dll', 'vcruntime140_1.dll')
# The Store build imports vulkan-1.dll, the Vulkan loader. A PC has it only when a
# graphics driver installed it, so the package has its own copy, with its license.
if (-not $Cpu) { $files += 'vulkan-1.dll', 'VulkanRT-License.txt' }
foreach ($file in $files) {
  if (-not (Test-Path (Join-Path $BuildDir $file))) {
    throw "$file is not in $BuildDir. Run scripts/build-windows.sh first."
  }
}

$out = Join-Path $root 'release\windows'
$layout = Join-Path $out 'layout'
if (Test-Path $layout) { Remove-Item $layout -Recurse -Force }
New-Item -ItemType Directory -Force $layout | Out-Null

foreach ($file in $files) { Copy-Item (Join-Path $BuildDir $file) $layout }
Copy-Item (Join-Path $root 'windows\msix\Assets') (Join-Path $layout 'Assets') -Recurse

$manifest = Get-Content (Join-Path $root 'windows\msix\AppxManifest.xml') -Raw
$manifest = $manifest.Replace('{{VERSION}}', $version).Replace('{{ARCH}}', $Arch)
if ($manifest -match '\{\{[A-Z]+\}\}') { throw "The manifest has a token that this script does not know: $($Matches[0])" }
# No byte order mark: MakeAppx reads the file as the encoding that its first line names.
[IO.File]::WriteAllText((Join-Path $layout 'AppxManifest.xml'), $manifest, (New-Object Text.UTF8Encoding $false))

# resources.pri is the index that lets Windows pick a logo file by size and scale
# (Square44x44Logo.targetsize-24.png for the taskbar, and so on). Without it Windows
# shows the plain file name only, scaled. Make it here, in the package folder, so that a
# test of the folder (winapp run) has it too. winapp can make it while it packs, but then
# it leaves its two work files in the package.
$priConfig = Join-Path $out 'priconfig.xml'
& winapp tool makepri createconfig /cf $priConfig /dq en-US /pv 10.0.0 /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw "makepri createconfig failed with code $LASTEXITCODE" }
# The config asks for one more index file for each scale and language. That is for a
# bundle of resource packages. One package reads resources.pri only, so keep it whole.
$priXml = [xml](Get-Content $priConfig -Raw)
foreach ($node in @($priXml.resources.packaging)) { if ($node) { [void]$priXml.resources.RemoveChild($node) } }
$priXml.Save($priConfig)
& winapp tool makepri new /pr $layout /cf $priConfig /of (Join-Path $layout 'resources.pri') /mn (Join-Path $layout 'AppxManifest.xml') /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw "makepri new failed with code $LASTEXITCODE" }
Remove-Item $priConfig -Force

$msix = Join-Path $out "MarginNotes_${version}_${Arch}.msix"
if (Test-Path $msix) { Remove-Item $msix -Force }
& winapp package $layout --no-sign --skip-pri --output $msix
if ($LASTEXITCODE -ne 0) { throw "winapp package failed with code $LASTEXITCODE" }
if (-not (Test-Path $msix)) { throw "winapp package gave no error and no file: $msix" }

$size = [math]::Round((Get-Item $msix).Length / 1MB, 1)
Write-Host "Package folder: $layout"
Write-Host "Package: $msix ($size MB, version $version, $Arch, not signed)"
