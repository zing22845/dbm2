# dbm2 demo — recorded with VHS (https://github.com/charmbracelet/vhs)
#
# One-time setup:
#   brew install vhs
#   cargo build --release -p dbm-cli
#
# Record (from the repository root):
#   vhs demos/demo.tape        # writes demos/demo.gif
#
# The tape prepares its own throwaway store via demos/setup.sh, so every run
# starts from the same state. The TUI is driven with the keyboard only;
# queries are executed with Alt+Enter.

Output demos/demo.gif

Set Shell bash
Set FontSize 16
Set Width 1440
Set Height 900
Set Padding 12
Set Theme "Dracula"
Set Framerate 24
Set TypingSpeed 50ms
Set CursorBlink false

# --- prepare a clean, pre-seeded store and start the TUI (not recorded) ---
Hide
Type "./demos/setup.sh && DBM_DATA_DIR=/tmp/dbm-demo-data ./target/release/dbm i"
Enter
Sleep 14s
Show

# --- 1. discovery: open the Discover modal, tweak the scan target, scan, close ---
Enter
Sleep 2.5s
Ctrl+j
Sleep 1s
Type "j"
Sleep 600ms
Type "l"
Sleep 600ms
Type "i"
Sleep 1s
Escape
Sleep 800ms
Type "s"
Sleep 6s
Escape
Sleep 800ms
Type "y"
Sleep 1.5s

# --- 2. explorer: expand the instance, open its connection ---
Sleep 1.5s
Type "I"
Sleep 500ms
Type "l"
Sleep 1.2s
Type "j"
Sleep 400ms
Enter
Sleep 2.5s

# --- 2. SQL workspace: type a query and run it (Alt+Enter) ---
Type "i"
Type@20ms "SELECT id, status, qty, total, placed_at FROM orders ORDER BY placed_at DESC LIMIT 50;"
Sleep 700ms
Alt+Enter
Sleep 3s

# --- 3. results grid: scroll, jump, count rows ---
Ctrl+j
Sleep 1.2s
Type "j"
Type "j"
Type "j"
Sleep 1s
Type "G"
Sleep 1.2s
Type "g"
Sleep 1s
Type "c"
Sleep 2s

# --- 4. leave ---
Sleep 800ms
Ctrl+D
Sleep 1s
