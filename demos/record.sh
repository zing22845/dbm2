#!/usr/bin/env bash
# Record demos/demo.gif without a browser: asciinema captures a tmux session
# that is driven with tmux send-keys, then agg renders the cast to a GIF.
#
#   ./demos/record.sh
#
# Overridable: DEMO_DATA_DIR, DEMO_COLS, DEMO_ROWS, DEMO_FONT_SIZE, DBM
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$here")

DATA_DIR="${DEMO_DATA_DIR:-/tmp/dbm-demo-data}"
DBM="${DBM:-$root/target/release/dbm}"
SESSION="${DEMO_SESSION:-dbmdemo}"
COLS="${DEMO_COLS:-180}"
ROWS="${DEMO_ROWS:-46}"
FONT_SIZE="${DEMO_FONT_SIZE:-14}"
CAST="$here/demo.cast"
GIF="$here/demo.gif"

tmux() { command tmux -f /dev/null "$@"; }

# ---- make sure the demo database and store are in a known state ----
"$here/setup.sh" >/dev/null

# ---- start the TUI inside a detached tmux session ----
tmux kill-session -t "$SESSION" 2>/dev/null || true
tmux new-session -d -s "$SESSION" -x "$COLS" -y "$ROWS" \
    "DBM_DATA_DIR=$DATA_DIR exec $DBM i"
tmux set-option -t "$SESSION" status off

send() { tmux send-keys -t "$SESSION" "$@"; }
type_text() { tmux send-keys -t "$SESSION" -l "$1"; }
pause() { sleep "${1:-0.5}"; }

# ---- drive the demo (keyboard only) ----
(
    pause 4
    # 1. explorer: expand the instance, open its connection
    type_text "I";  pause 0.6
    type_text "l";  pause 1.2
    type_text "j";  pause 0.4
    send Enter;     pause 2.5
    # 2. SQL workspace: type a query and run it (Alt+Enter)
    type_text "i";  pause 0.5
    type_text "SELECT id, status, qty, total, placed_at FROM orders ORDER BY placed_at DESC LIMIT 50;"
    pause 0.8
    send -H 1b 0d              # ESC + CR = Alt+Enter (run query)
    pause 3
    # 3. results grid: scroll, jump, count rows
    send C-j;       pause 1.2
    type_text "j"; type_text "j"; type_text "j"; pause 1
    type_text "G";  pause 1.2
    type_text "g";  pause 1
    type_text "c";  pause 2
    # 4. leave
    send C-d;       pause 1
) &
driver=$!

# ---- record the attached session ----
asciinema rec "$CAST" \
    --window-size "${COLS}x${ROWS}" \
    -c "tmux -f /dev/null attach -t $SESSION" \
    --overwrite --idle-time-limit 2

wait "$driver" 2>/dev/null || true
tmux kill-session -t "$SESSION" 2>/dev/null || true

# ---- render the GIF ----
agg --font-size "$FONT_SIZE" --theme dracula "$CAST" "$GIF"
echo "wrote $GIF"
