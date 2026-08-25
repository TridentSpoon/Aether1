#!/usr/bin/env bash
# ==============================================================================
# Project AETHER1 AI - Background Daemon Launcher
# Starts all services silently in the background (No terminal window needed!)
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

mkdir -p "$SCRIPT_DIR/logs"
PID_FILE="$SCRIPT_DIR/logs/cortana.pid"
LOG_FILE="$SCRIPT_DIR/logs/cortana.log"

# Run setup if first time
if [ ! -d "venv" ]; then
    ./setup.sh >> "$LOG_FILE" 2>&1
fi

# Check if already running
if [ -f "$PID_FILE" ]; then
    OLD_PID=$(head -n 1 "$PID_FILE" 2>/dev/null || true)
    if [ -n "$OLD_PID" ] && ps -p "$OLD_PID" > /dev/null 2>&1; then
        echo "✔ AETHER1 is already running in background (PID: $OLD_PID)."
        if command -v xdg-open &> /dev/null; then
            xdg-open "http://localhost:8378" > /dev/null 2>&1 &
        fi
        exit 0
    fi
fi

echo "🚀 Launching AETHER1 in background daemon mode..."

# 1. Start Backend Server in background
nohup ./venv/bin/uvicorn backend.main:app --host 0.0.0.0 --port 8378 >> "$LOG_FILE" 2>&1 &
BACKEND_PID=$!

# Wait briefly for backend
sleep 1.5

# 2. Start System Tray Notification App in background
TRAY_PID=""
if [ -n "$DISPLAY" ] || [ -n "$WAYLAND_DISPLAY" ]; then
    nohup ./venv/bin/python desktop/tray_app.py >> "$LOG_FILE" 2>&1 &
    TRAY_PID=$!
fi

# Save PIDs
echo "$BACKEND_PID" > "$PID_FILE"
if [ -n "$TRAY_PID" ]; then
    echo "$TRAY_PID" >> "$PID_FILE"
fi

# 3. Open HUD in default browser
if command -v xdg-open &> /dev/null; then
    xdg-open "http://localhost:8378" > /dev/null 2>&1 &
elif command -v python3 &> /dev/null; then
    python3 -m webbrowser "http://localhost:8378" > /dev/null 2>&1 &
fi

echo "======================================================================"
echo "✨ AETHER1 IS RUNNING IN THE BACKGROUND!"
echo "  - Web HUD: http://localhost:8378"
echo "  - Notification Bar: 🤖 Robot icon active in your panel"
echo "  - Logs: $LOG_FILE"
echo "  - To stop at any time: ./stop.sh"
echo "======================================================================"

# Exit immediately so terminal does not need to stay open
exit 0
