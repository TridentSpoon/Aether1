@echo off
setlocal enabledelayedexpansion
title AETHER1 AI Launcher
cd /d "%~dp0"

echo ======================================================================
echo Launching AETHER1 AI Companion
echo ======================================================================

set BIN=src-tauri\target\release\aether1.exe

REM --- Two ways to run. The default is the native app: a real window, an icon in the
REM     notification area, and the global hotkey. `start.bat --browser` runs the headless
REM     server instead and opens the HUD in a browser tab, which is the development flow
REM     and the fallback if the webview misbehaves.
REM
REM     This used to always take the --browser path, which is why the app built fine and
REM     then appeared only as a browser tab with no tray icon anywhere.
set MODE=native
if /i "%~1"=="--browser" set MODE=browser
if /i "%~1"=="-b" set MODE=browser

if "%MODE%"=="native" goto :build_check

REM --- If an AETHER1 instance is already listening on :8378 (e.g. this script was
REM     already run once, or the native app is open), just reopen the HUD instead of
REM     launching a second server -- server.rs's bind() panics if the port's taken, so
REM     starting a duplicate would crash the new window instead of doing anything useful.
REM     Checks an explicit output token rather than trusting PowerShell's bare exit code --
REM     a broken/misbehaving powershell.exe could exit 0 without actually running the
REM     probe, which would otherwise look identical to "port is open" and wrongly skip
REM     starting the server. Falling through to the normal start path is the safe default
REM     if the check can't run at all; the worse failure mode is silently doing nothing.
where powershell >nul 2>&1
if not errorlevel 1 (
    set "PORT_CHECK_FILE=%TEMP%\aether1_port_check_%RANDOM%.txt"
    powershell -NoProfile -Command "try { $c = New-Object System.Net.Sockets.TcpClient; $c.Connect('127.0.0.1', 8378); $c.Close(); Write-Output 'AETHER1_PORT_OPEN' } catch { Write-Output 'AETHER1_PORT_CLOSED' }" > "!PORT_CHECK_FILE!" 2>nul
    findstr /C:"AETHER1_PORT_OPEN" "!PORT_CHECK_FILE!" >nul 2>&1
    set "PORT_IS_OPEN=!ERRORLEVEL!"
    del "!PORT_CHECK_FILE!" >nul 2>&1
    if "!PORT_IS_OPEN!"=="0" (
        echo AETHER1 is already running on http://localhost:8378 -- reopening the HUD.
        start http://localhost:8378
        exit /b 0
    )
)

REM --- Build the release binary if it doesn't exist yet. This is the ONLY thing that
REM     needs to be installed to run AETHER1 -- once built, %BIN% is a single,
REM     self-contained .exe (no Python, no other runtime install) and every future run
REM     of this script skips straight past this whole block, no Rust required again.
:build_check
if not exist "%BIN%" (
    where cargo >nul 2>&1
    if errorlevel 1 (
        echo.
        echo Rust is not installed ^(the "cargo" command was not found on PATH^).
        echo AETHER1 is written in Rust and needs it to build the app -- this is a
        echo one-time requirement; the resulting .exe does not need Rust to run.
        echo Install it from https://rustup.rs, then re-run this script.
        echo.
        pause
        exit /b 1
    )

    echo No release build found. Building AETHER1 ^(first build can take a few minutes^)...
    pushd src-tauri
    cargo build --release
    set BUILD_ERRORLEVEL=!ERRORLEVEL!
    popd

    if not "!BUILD_ERRORLEVEL!"=="0" (
        echo.
        echo Build failed ^(exit code !BUILD_ERRORLEVEL!^) -- see the errors above.
        pause
        exit /b 1
    )
    if not exist "%BIN%" (
        echo.
        echo Build reported success but %BIN% still doesn't exist -- something's wrong
        echo with the build output path. Please report this.
        pause
        exit /b 1
    )
    echo Build complete: %BIN%
    echo This .exe is self-contained -- future runs of this script use it directly.
)

if "%MODE%"=="browser" goto :run_browser

REM --- Native app. Launching it twice is harmless: the second copy hands over to the
REM     one already running and exits, so this doubles as "show the HUD".
echo Starting AETHER1...
start "" "%BIN%"
echo.
echo AETHER1 is running. Look for its icon in the notification area ^(system tray^)
echo -- click it to show or hide the HUD. Closing the window leaves it running there.
echo.
echo Run setup.bat once to add Start Menu and desktop shortcuts.
exit /b 0

:run_browser
echo Starting AETHER1 server on http://localhost:8378...
start "AETHER1 Server" "%BIN%" --serve

timeout /t 2 >nul

echo Opening Holographic Cyberpunk HUD...
start http://localhost:8378

echo ======================================================================
echo AETHER1 is active. Close the "AETHER1 Server" window to stop it.
echo ======================================================================
pause
