# Stages everything the Windows offline installer needs, then compiles it into one .exe
# with Inno Setup (installer\aether1.iss). Counterpart to package_offline_linux.sh -- see
# that script's header for why a single compiled binary is enough here (the frontend is
# baked into it by Tauri; see tauri.conf.json's frontendDist).
#
# Must run on Windows (needs cargo build --release for the .exe, and iscc.exe to compile
# the installer) -- this is what release.yml's windows-latest job runs. Needs internet
# access to fetch Piper/whisper.cpp/the models; the *installer it produces* needs none.
#
# Usage: powershell -File scripts\package_offline_windows.ps1 [-OutDir dist]

# WhisperCppRef must be new enough that the CLI example is named `whisper-cli`
# (examples/cli/) rather than the older `main` (examples/main/) -- confirmed present at
# v1.9.3, confirmed absent at v1.7.2 (the --target whisper-cli build below fails there with
# MSB1009 "Project file does not exist: whisper-cli.vcxproj"). If bumping this ever breaks
# the same way, check examples/cli/CMakeLists.txt exists at the new ref first.
param(
    [string]$OutDir = "dist",
    [string]$PiperVersion = "2023.11.14-2",
    [string]$PiperVoice = "en_US-lessac-medium",
    [string]$WhisperCppRef = "v1.9.3",
    [string]$WhisperModel = "small"
)

$ErrorActionPreference = 'Stop'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $RepoRoot

$Stage = Join-Path $env:TEMP "aether1-offline-stage-$([guid]::NewGuid())"
New-Item -ItemType Directory -Path $Stage | Out-Null
New-Item -ItemType Directory -Path "$Stage\piper", "$Stage\whisper", "$Stage\models" | Out-Null

try {
    Write-Host "======================================================================"
    Write-Host "Packaging AETHER1 offline bundle (Windows x64)"
    Write-Host "======================================================================"

    Write-Host "Building the release binary..."
    Push-Location src-tauri
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }
    Pop-Location
    Copy-Item "src-tauri\target\release\aether1.exe" "$Stage\aether1.exe"

    Write-Host "Fetching Piper $PiperVersion (TTS)..."
    $piperZip = "$Stage\piper.zip"
    Invoke-WebRequest -Uri "https://github.com/rhasspy/piper/releases/download/$PiperVersion/piper_windows_amd64.zip" -OutFile $piperZip
    # The zip unpacks to a piper\ directory holding piper.exe plus the DLLs it needs
    # (onnxruntime.dll, piper_phonemize.dll, etc.) -- those all have to sit next to the
    # exe, which Inno Setup preserves as-is by installing this whole directory verbatim.
    Expand-Archive -Path $piperZip -DestinationPath $Stage -Force
    Remove-Item $piperZip

    Write-Host "Fetching the $PiperVoice voice..."
    $voiceBase = "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium"
    Invoke-WebRequest -Uri "$voiceBase/$PiperVoice.onnx" -OutFile "$Stage\models\$PiperVoice.onnx"
    Invoke-WebRequest -Uri "$voiceBase/$PiperVoice.onnx.json" -OutFile "$Stage\models\$PiperVoice.onnx.json"

    Write-Host "Building whisper.cpp $WhisperCppRef (STT)..."
    $whisperSrc = "$Stage\whisper.cpp-src"
    git clone --depth 1 --branch $WhisperCppRef https://github.com/ggml-org/whisper.cpp $whisperSrc
    $whisperBuild = "$Stage\whisper.cpp-build"
    # Static linking (BUILD_SHARED_LIBS=OFF) so whisper-cli.exe doesn't need whisper.dll /
    # ggml.dll shipped alongside it -- one fewer thing for the installer to get right.
    # GGML_NATIVE=OFF so this doesn't get built for whichever CPU happens to be compiling
    # it (the GitHub Actions runner), which may not match the machine installing it.
    cmake -S $whisperSrc -B $whisperBuild -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DGGML_NATIVE=OFF
    if ($LASTEXITCODE -ne 0) { throw "cmake configure failed" }
    cmake --build $whisperBuild --config Release --target whisper-cli
    if ($LASTEXITCODE -ne 0) { throw "cmake build failed" }
    New-Item -ItemType Directory -Path "$Stage\whisper" -Force | Out-Null
    Copy-Item "$whisperBuild\bin\Release\whisper-cli.exe" "$Stage\whisper\whisper-cli.exe"
    Remove-Item -Recurse -Force $whisperSrc, $whisperBuild

    Write-Host "Fetching the $WhisperModel Whisper model..."
    Invoke-WebRequest -Uri "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-$WhisperModel.bin" -OutFile "$Stage\models\ggml-$WhisperModel.bin"

    Copy-Item "src-tauri\icons\icon.ico" "$Stage\icon.ico"
    Copy-Item "THIRD_PARTY_NOTICES.md" "$Stage\THIRD_PARTY_NOTICES.md"

    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
    $OutDirFull = (Resolve-Path $OutDir).Path

    Write-Host "Compiling the installer with Inno Setup..."
    $iscc = Get-Command iscc.exe -ErrorAction SilentlyContinue
    if (-not $iscc) {
        $iscc = "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
        if (-not (Test-Path $iscc)) {
            throw "iscc.exe (Inno Setup) not found -- install it or add it to PATH."
        }
    }
    & $iscc "/DStageDir=$Stage" "/DOutDir=$OutDirFull" "/DPiperVoice=$PiperVoice" "/DWhisperModel=$WhisperModel" `
        "installer\aether1.iss"
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup compilation failed" }

    Write-Host ""
    Write-Host "======================================================================"
    Write-Host "Done. Installer is in $OutDirFull"
    Write-Host "======================================================================"
} finally {
    Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
}
