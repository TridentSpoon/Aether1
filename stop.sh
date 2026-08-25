#!/usr/bin/env bash
# ==============================================================================
# Project AETHER / CORTANA AI - Stop Script
# Gracefully terminates all background AETHER / CORTANA services
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

PID_FILE="$SCRIPT_DIR/logs/cortana.pid"

echo "🛑 Stopping AETHER / CORTANA services..."

STOPPED=0

if [ -f "$PID_FILE" ]; then
    while read -r pid; do
        if [ -n "$pid" ] && ps -p "$pid" > /dev/null 2>&1; then
            kill "$pid" 2>/dev/null || true
            STOPPED=1
        fi
    done < "$PID_FILE"
    rm -f "$PID_FILE"
fi

# Fallback cleanup for any dangling processes
pkill -f "uvicorn backend.main:app" 2>/dev/null || true
pkill -f "desktop/tray_app.py" 2>/dev/null || true

echo "✔ AETHER / CORTANA services stopped successfully."
