#!/usr/bin/env bash
# ==============================================================================
# Project AETHER1 AI - 1-Click Launch Script
# Starts the native Rust server (headless --serve mode) and opens the HUD in a browser.
# This is a dev convenience only -- the real way to run AETHER1 is the native desktop app
# (see scripts/install_desktop_app.sh); this just gives the same backend a browser tab.
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

BIN="src-tauri/target/release/aether1"
if [ ! -x "$BIN" ]; then
    echo "⚡ No release build found. Building (first build can take a few minutes)..."
    (cd src-tauri && cargo build --release)
fi

echo "======================================================================"
echo "🌐 LAUNCHING AETHER1 AI CORE"
echo "======================================================================"

echo "🚀 Starting AETHER1 server on http://localhost:8378..."
"$BIN" --serve &
SERVER_PID=$!

# Wait briefly for the server to bind before opening a browser tab at it.
sleep 1

echo "💻 Opening Holographic Cyberpunk HUD in Browser..."
if command -v xdg-open &> /dev/null; then
    xdg-open "http://localhost:8378" &
elif command -v python3 &> /dev/null; then
    python3 -m webbrowser "http://localhost:8378" &
fi

echo ""
echo "✔ AETHER1 AI is running!"
echo "  - Web HUD: http://localhost:8378"
echo "  - Press Ctrl+C to terminate"
echo "======================================================================"

cleanup() {
    echo ""
    echo "🛑 Shutting down AETHER1 server..."
    kill "$SERVER_PID" 2>/dev/null || true
    exit 0
}

trap cleanup SIGINT SIGTERM

wait $SERVER_PID
