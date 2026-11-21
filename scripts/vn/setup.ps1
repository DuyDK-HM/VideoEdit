# Sets up a Windows machine to build and run Concat from a fresh clone.
#
#   setup.bat               (or: powershell -ExecutionPolicy Bypass -File scripts\vn\setup.ps1)
#
# Installs, through winget, what is missing of: Git, rustup, the MSVC build
# tools, CMake and LLVM (libclang for bindgen). Then downloads the FFmpeg
# shared build CI uses (scripts/ci/deps.lock, sha256-checked) into vendor\
# and writes vendor\env.cmd, which run.bat calls to put it all on PATH.
# Safe to rerun: installed tools and a verified download are skipped.
# ONNX Runtime needs no step: the `ort` crate downloads its own binaries.
$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot\..\..").Path
$vendor = Join-Path $root "vendor"

function Install-WingetPackage([string]$id, [string[]]$extra = @(), [string]$command = "") {
    if ($command -and (Get-Command $command -ErrorAction SilentlyContinue)) { Write-Host "ok      $id ($command)"; return }
    winget list --id $id -e --accept-source-agreements *> $null
    if ($LASTEXITCODE -eq 0) { Write-Host "ok      $id"; return }
    Write-Host "install $id"
    winget install --id $id -e --silent --accept-package-agreements --accept-source-agreements @extra
    if ($LASTEXITCODE -ne 0) { throw "winget could not install $id (exit $LASTEXITCODE)" }
}

Install-WingetPackage "Git.Git" -command git
Install-WingetPackage "Rustlang.Rustup" -command rustup
# The C++ workload brings the MSVC linker and Windows SDK that rustc's msvc target needs.
Install-WingetPackage "Microsoft.VisualStudio.2022.BuildTools" @(
    "--override", "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended")
Install-WingetPackage "Kitware.CMake"
Install-WingetPackage "LLVM.LLVM"
# Downloads from a link (TikTok, YouTube, Douyin): the fork's Download dialog runs it.
Install-WingetPackage "yt-dlp.yt-dlp" -command yt-dlp

$llvmBin = Join-Path $env:ProgramFiles "LLVM\bin"
if (-not (Test-Path (Join-Path $llvmBin "libclang.dll"))) { throw "libclang.dll not found in $llvmBin" }
$cmakeBin = Join-Path $env:ProgramFiles "CMake\bin"

# FFmpeg: the same build CI links, fetched and checked by scripts/ci/fetch.sh.
# Git's bash beside wherever git.exe is (Git need not be in Program Files).
$git = Get-Command git -ErrorAction SilentlyContinue
$bash = if ($git) { Join-Path (Split-Path (Split-Path $git.Source)) "bin\bash.exe" } else { "" }
if (-not (Test-Path $bash)) { throw "Git's bash not found at $bash (open a new terminal after Git installs, then rerun)" }
New-Item -ItemType Directory -Force $vendor | Out-Null
$zip = Join-Path $vendor "ffmpeg-win.zip"
& $bash (Join-Path $root "scripts/ci/fetch.sh") ffmpeg-win64 ($zip -replace '\\', '/')
if ($LASTEXITCODE -ne 0) { throw "the FFmpeg download failed" }
$ffRoot = Join-Path $vendor "ffmpeg"
$marker = Join-Path $ffRoot ".from"
$zipHash = (Get-FileHash $zip -Algorithm SHA256).Hash
if (-not ((Test-Path $marker) -and ((Get-Content $marker) -eq $zipHash))) {
    if (Test-Path $ffRoot) { Remove-Item -Recurse -Force $ffRoot }
    Expand-Archive $zip -DestinationPath $ffRoot
    Set-Content $marker $zipHash
}
$ffDir = (Get-ChildItem $ffRoot -Directory | Select-Object -First 1).FullName

# The old project's AI, for the fork's batch subtitles and translation: found
# beside this repository, its AIVoice venv's Python runs sidecar\old_project_bridge.py.
$oldRoot = Join-Path (Split-Path $root) "ToolAutoMakeCartoonVideo2DFromComics"
$oldPython = Join-Path $oldRoot "AIVoice\.venv\Scripts\python.exe"
$sidecar = ""
if (Test-Path $oldPython) {
    $sidecar = @"
set "CONCAT_OLD_ROOT=$oldRoot"
set "CONCAT_SIDECAR_PYTHON=$oldPython"
set "CONCAT_SIDECAR_BRIDGE=$root\sidecar\old_project_bridge.py"
"@
} else {
    Write-Host "note: $oldPython not found; batch subtitles and translation stay off"
}

# rustup reads src\rust-toolchain.toml on the first cargo call and fetches that toolchain.
@"
@rem Written by scripts\vn\setup.ps1 for this machine. Do not commit.
set "FFMPEG_DIR=$ffDir"
set "LIBCLANG_PATH=$llvmBin"
set "PATH=$ffDir\bin;$llvmBin;$cmakeBin;%USERPROFILE%\.cargo\bin;%PATH%"
$sidecar
"@ | Set-Content -Encoding ascii (Join-Path $vendor "env.cmd")

Write-Host "done: vendor\env.cmd written. Start the editor with run.bat."
