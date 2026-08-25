#!/usr/bin/env bash
# ==============================================================================
# Project AETHER1 AI - 1-Click Launch Script
# Starts FastAPI Backend, System Tray Notification App, and Opens HUD
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if [ ! -d "venv" ]; then
    echo "⚡ First-time run detected. Running setup..."
    ./setup.sh
fi

echo "======================================================================"
echo "🌐 LAUNCHING AETHER1 AI CORE"
echo "======================================================================"

# Start Backend Server
echo "🚀 Starting FastAPI Backend on http://localhost:8378..."
./venv/bin/uvicorn backend.main:app --host 0.0.0.0 --port 8378 &
BACKEND_PID=$!

# Wait briefly for backend to initialize
sleep 2

# Start System Tray App (if graphical display is present)
TRAY_PID=""
if [ -n "$DISPLAY" ] || [ -n "$WAYLAND_DISPLAY" ]; then
    echo "🔔 Starting System Tray Notification Companion..."
    ./venv/bin/python desktop/tray_app.py &
    TRAY_PID=$!
fi

# Open HUD in default browser
echo "💻 Opening Holographic Cyberpunk HUD in Browser..."
if command -v xdg-open &> /dev/null; then
    xdg-open "http://localhost:8378" &
elif command -v python3 &> /dev/null; then
    python3 -m webbrowser "http://localhost:8378" &
fi

echo ""
echo "✔ AETHER1 AI is running!"
echo "  - Web HUD: http://localhost:8378"
echo "  - System Tray: Active in your notification panel"
echo "  - Press Ctrl+C to terminate all services"
echo "======================================================================"

# Trap termination signals to kill both background processes
cleanup() {
    echo ""
    echo "🛑 Shutting down AETHER1 services..."
    if [ -n "$BACKEND_PID" ]; then
        kill "$BACKEND_PID" 2>/dev/null || true
    fi
    if [ -n "$TRAY_PID" ]; then
        kill "$TRAY_PID" 2>/dev/null || true
    fi
    exit 0
}

trap cleanup SIGINT SIGTERM

# Wait on backend process
wait $BACKEND_PID
