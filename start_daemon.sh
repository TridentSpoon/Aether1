#!/usr/bin/env bash
# ==============================================================================
# Project AETHER1 AI - Background Daemon Launcher
# Starts the native Rust server (headless --serve mode) silently in the background.
# This is a dev convenience only -- the real way to run AETHER1 is the native desktop app.
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

mkdir -p "$SCRIPT_DIR/logs"
PID_FILE="$SCRIPT_DIR/logs/aether1.pid"
LOG_FILE="$SCRIPT_DIR/logs/aether1.log"

BIN="src-tauri/target/release/aether1"
if [ ! -x "$BIN" ]; then
    echo "⚡ No release build found. Building (first build can take a few minutes)..."
    (cd src-tauri && cargo build --release) >> "$LOG_FILE" 2>&1
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

nohup "$BIN" --serve >> "$LOG_FILE" 2>&1 &
SERVER_PID=$!
echo "$SERVER_PID" > "$PID_FILE"

# Wait briefly for the server to bind
sleep 1

if command -v xdg-open &> /dev/null; then
    xdg-open "http://localhost:8378" > /dev/null 2>&1 &
elif command -v python3 &> /dev/null; then
    python3 -m webbrowser "http://localhost:8378" > /dev/null 2>&1 &
fi

echo "======================================================================"
echo "✨ AETHER1 IS RUNNING IN THE BACKGROUND!"
echo "  - Web HUD: http://localhost:8378"
echo "  - Logs: $LOG_FILE"
echo "  - To stop at any time: ./stop.sh"
echo "======================================================================"

# Exit immediately so terminal does not need to stay open
exit 0
