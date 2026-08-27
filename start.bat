@echo off
title AETHER1 AI Launcher
cd /d "%~dp0"

echo ======================================================================
echo Launching AETHER1 AI Companion
echo ======================================================================

set BIN=src-tauri\target\release\aether1.exe

if not exist "%BIN%" (
    echo No release build found. Building AETHER1 (first build can take a few minutes)...
    pushd src-tauri
    cargo build --release
    popd
)

if not exist "%BIN%" (
    echo.
    echo Build failed -- see the errors above. Make sure Rust is installed:
    echo   https://rustup.rs
    pause
    exit /b 1
)

echo Starting AETHER1 server on http://localhost:8378...
start "AETHER1 Server" "%BIN%" --serve

timeout /t 2 >nul

echo Opening Holographic Cyberpunk HUD...
start http://localhost:8378

echo ======================================================================
echo AETHER1 is active. Close the "AETHER1 Server" window to stop it.
echo ======================================================================
pause
