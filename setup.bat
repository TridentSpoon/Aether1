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
    REM --- error 4551 means Windows itself refused to launch something the build
    REM     produced (Smart App Control or a managed WDAC/AppLocker policy), not a
    REM     problem with AETHER1's code -- a plain "build failed" message here would
    REM     send people chasing a bug in the repo instead of their Windows settings.
    REM     What gets blocked is the build scripts Cargo compiles into target/ and
    REM     runs, not rustc: they are unsigned and seconds old, which is exactly what
    REM     these policies refuse. Naming rustc sent the first person who hit this
    REM     looking at their Rust install, where there was nothing to find.
    findstr /C:"Application Control policy has blocked this file" "!BUILD_LOG!" >nul 2>&1
    if not errorlevel 1 (
        echo ======================================================================
        echo   SETUP FAILED -- Windows blocked the build's own programs
        echo ======================================================================
        echo.
        echo   This is NOT a problem with AETHER1's code, and not a problem with
        echo   Rust either. Building this app compiles small helper programs
        echo   ^(Cargo build scripts^) into src-tauri\target\ and then runs them,
        echo   and Windows refused to launch those: error 4551, "An Application
        echo   Control policy has blocked this file". The build output above names
        echo   the exact files it would not run.
        echo.
        echo   They were blocked for being new and unsigned, not for where they
        echo   live -- so moving Rust, or your .rustup and .cargo folders, changes
        echo   nothing at all. Every build produces fresh unsigned programs.
        echo.
        echo   Which policy it is:
        echo.
        echo     Smart App Control, on by default on clean installs of Windows 11
        echo     22H2 and later. Settings -^> Privacy ^& security -^> Windows
        echo     Security -^> App ^& browser control -^> Smart App Control.
        echo.
        echo     WARNING: turning Smart App Control off is a one-way door. It
        echo     cannot be switched back on without reinstalling Windows. Read
        echo     "Windows blocked it" in README.md before you decide.
        echo.
        echo     A managed PC's WDAC or AppLocker policy, if that switch is not
        echo     there or already says Off. Then it is IT's call, and what they
        echo     need to allow is the Rust toolchain AND this checkout's
        echo     src-tauri\target directory, where the blocked programs are built.
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
