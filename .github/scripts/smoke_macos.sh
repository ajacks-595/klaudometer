#!/usr/bin/env bash
# Smoke-test the macOS build on a CI runner. Gives it a fake Claude Code login
# in the Keychain (where Claude Code keeps it on a Mac), points it at the mock
# API, checks it fetched and cached usage and API spend, and takes
# screenshots of the menu bar and the card.
#
#   smoke_macos.sh path/to/Klaudometer.app output-dir
set -euo pipefail

app="$1"
out="$2"
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$out"
port=18790

python3 "$here/mock_anthropic_api.py" "$port" demo > "$out/mock.log" 2>&1 &
mock=$!
name=""
trap 'kill "$mock" 2>/dev/null || true; [ -n "$name" ] && pkill -x "$name" 2>/dev/null || true' EXIT

# Claude Code's Keychain entry. -A lets any app read it without a prompt,
# which is only acceptable on a throwaway CI machine.
expires=$(( ($(date +%s) + 86400) * 1000 ))
login='{"claudeAiOauth":{"accessToken":"fake-oauth-token","expiresAt":'"$expires"',"subscriptionType":"max","rateLimitTier":"default_claude_max_5x"}}'
security add-generic-password -U -A -a "$USER" -s "Claude Code-credentials" -w "$login"

# Show API spend as well, with the full tray label.
settings="$HOME/Library/Application Support/klaudometer"
mkdir -p "$settings"
echo '{"show_api": true, "label": "full"}' > "$settings/desktop.json"

binary="$(find "$app/Contents/MacOS" -type f -perm -u+x | head -1)"
name="$(basename "$binary")"
echo "Launching $binary"
KLAUDOMETER_API_BASE="http://127.0.0.1:$port" ANTHROPIC_ADMIN_KEY="sk-ant-admin01-fake" \
    "$binary" > "$out/app.log" 2>&1 &

cache="$HOME/Library/Caches/klaudometer"
for _ in $(seq 1 60); do
    [ -f "$cache/usage.json" ] && [ -f "$cache/api.json" ] && break
    sleep 1
done
cp "$cache"/*.json "$out/" 2>/dev/null || true

python3 - "$cache" <<'EOF'
import json, sys
cache = sys.argv[1]
usage = json.load(open(f"{cache}/usage.json"))
api = json.load(open(f"{cache}/api.json"))
assert usage["error"] is None, usage["error"]
assert usage["plan"] == "Max 5x", usage
assert usage["windows"]["five_hour"]["used"] == 81.0, usage
assert usage["extra"]["used"] == 12.34, usage
assert api["error"] is None and abs(api["spent"] - 42.75) < 1e-9, api
print("OK: login read from the Keychain; usage and API spend fetched and cached")
EOF

if ! pgrep -x "$name" > /dev/null; then
    echo "The app isn't running:"
    cat "$out/app.log"
    exit 1
fi
echo "OK: app still running"

sleep 3
screencapture -x "$out/menu-bar.png" || echo "screencapture failed (no screen access?)"
# A second launch hands over to the running copy, which opens its card.
"$binary" || true
sleep 3
screencapture -x "$out/card.png" || true
