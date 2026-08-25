@echo off
title AETHER CORTANA AI Launcher
cd /d "%~dp0"

echo ======================================================================
echo Launching AETHER / CORTANA AI Companion
echo ======================================================================

if not exist "venv" (
    echo Setting up Python virtual environment...
    python -m venv venv
    call venv\Scripts\activate.bat
    pip install -r backend\requirements.txt
    python desktop\generate_icons.py
) else (
    call venv\Scripts\activate.bat
)

echo Starting FastAPI Backend on http://localhost:8000...
start "" "%~dp0venv\Scripts\uvicorn.exe" backend.main:app --host 0.0.0.0 --port 8000

timeout /t 2 >nul
echo Starting System Tray Notification Bar App...
start "" "%~dp0venv\Scripts\python.exe" desktop\tray_app.py

echo Opening Holographic Cyberpunk HUD...
start http://localhost:8000

echo ======================================================================
echo AETHER is active. Close this window when done.
echo ======================================================================
pause
