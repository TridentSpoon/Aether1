@echo off
setlocal enabledelayedexpansion
title AETHER1 Setup
cd /d "%~dp0"

echo ======================================================================
echo   INITIALIZING AETHER1 SETUP
echo ======================================================================
echo.

set BIN=src-tauri\target\release\aether1.exe

REM --- Rust is the one prerequisite. Unlike Linux, Windows needs no system libraries:
REM     Tauri uses the WebView2 runtime, which ships with Windows 11 and with every
REM     up-to-date Windows 10.
where cargo >nul 2>&1
if errorlevel 1 (
    echo Rust is not installed ^(the "cargo" command was not found on PATH^).
    echo AETHER1 is written in Rust and needs it to build the app. This is a one-time
    echo requirement -- the resulting .exe does not need Rust to run.
    echo.
    echo   Install it from https://rustup.rs
    echo.
    echo Then open a NEW terminal ^(so PATH is refreshed^) and run this script again.
    echo.
    pause
    exit /b 1
)

echo Building AETHER1 ^(the first build takes a few minutes^)...
pushd src-tauri
cargo build --release
set BUILD_ERRORLEVEL=!ERRORLEVEL!
popd

if not "!BUILD_ERRORLEVEL!"=="0" (
    echo.
    echo ======================================================================
    echo   SETUP FAILED -- the app did not build, so nothing was installed.
    echo   The errors above say what went wrong.
    echo ======================================================================
    pause
    exit /b 1
)
if not exist "%BIN%" (
    echo.
    echo Build reported success but %BIN% is missing. Please report this.
    pause
    exit /b 1
)

echo.
echo Creating shortcuts...
powershell -NoProfile -ExecutionPolicy Bypass -File "scripts\install_windows.ps1" -RepoRoot "%CD%"
if errorlevel 1 (
    echo   Could not create the shortcuts. The app still works -- run start.bat,
    echo   or launch %BIN% directly.
)

echo.
echo ======================================================================
echo   SETUP COMPLETE
echo.
echo   Launch AETHER1 from the Start Menu or the desktop shortcut, or run
echo   start.bat. It puts an icon in the notification area ^(system tray^);
echo   click it to show the HUD, and closing the window leaves it running
echo   there.
echo ======================================================================
echo.
pause
