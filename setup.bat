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
set "BUILD_LOG=%TEMP%\aether1_build_%RANDOM%.log"
cargo build --release > "!BUILD_LOG!" 2>&1
set BUILD_ERRORLEVEL=!ERRORLEVEL!
type "!BUILD_LOG!"
popd

if not "!BUILD_ERRORLEVEL!"=="0" (
    echo.
    REM --- errorlevel 4551 from rustc/cargo means Windows itself refused to launch
    REM     the compiler (Smart App Control or a managed WDAC/AppLocker policy), not
    REM     a problem with AETHER1's code -- a plain "build failed" message here would
    REM     send people chasing a bug in the repo instead of their Windows settings.
    findstr /C:"Application Control policy has blocked this file" "!BUILD_LOG!" >nul 2>&1
    if not errorlevel 1 (
        echo ======================================================================
        echo   SETUP FAILED -- Windows blocked rustc.exe from running
        echo ======================================================================
        echo.
        echo   This is NOT a problem with AETHER1's code. Windows itself refused to
        echo   launch the Rust compiler ^(error 4551, "Application Control policy
        echo   has blocked this file"^). This is almost always one of:
        echo.
        echo     1. Smart App Control ^(on by default on many new/reset Windows 11
        echo        installs^) blocking rustc.exe because it isn't signed the way
        echo        Microsoft-trusted binaries are.
        echo     2. A company-managed PC's WDAC/AppLocker policy that only allows
        echo        programs to run from approved folders ^(e.g. Program Files^),
        echo        which excludes your .rustup folder under your user profile.
        echo.
        echo   To check: Settings -^> Privacy ^& security -^> Windows Security -^>
        echo   App ^& browser control -^> Smart App Control. If it's On and shows
        echo   "Evaluation", you can turn it off there.
        echo.
        echo   If this is a work/managed laptop, this is IT's call -- ask them to
        echo   allow-list rustc.exe/cargo.exe or your .rustup/.cargo folders.
        echo.
        echo   Workaround: reinstall rustup after pointing it at a system path
        echo   Windows already trusts instead of your user profile, e.g.:
        echo     setx RUSTUP_HOME C:\ProgramData\rustup
        echo     setx CARGO_HOME C:\ProgramData\cargo
        echo   then open a NEW terminal and reinstall from https://rustup.rs
        echo ======================================================================
    ) else (
        echo ======================================================================
        echo   SETUP FAILED -- the app did not build, so nothing was installed.
        echo   The errors above say what went wrong.
        echo ======================================================================
    )
    del "!BUILD_LOG!" >nul 2>&1
    pause
    exit /b 1
)
del "!BUILD_LOG!" >nul 2>&1
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
