#!/usr/bin/env bash
#
# Starts emeraldian inside tmux on a three-note vault and checks that it draws:
# the listing appears, Alpha opens through the quick switcher, and its body and
# backlinks render. This is the start-up path that a terminal query can hang,
# which unit tests never reach because they have no terminal.
#
# Usage:
#   tests/tmux-render.sh [path/to/emeraldian]
#
# The binary defaults to target/release/emeraldian, then target/debug. Needs
# tmux. HOME is pointed at a scratch directory, so your own config is never
# read or written. Set IMAGES=off to start with images switched off, which
# skips the image query.
#
# Adapted from the render-vault test the pkg.haus Debian packagers wrote.

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bin="${1:-}"
if [ -z "$bin" ]; then
    for candidate in "$root/target/release/emeraldian" "$root/target/debug/emeraldian"; do
        if [ -x "$candidate" ]; then bin="$candidate"; break; fi
    done
fi
if [ -z "$bin" ] || [ ! -x "$bin" ]; then
    echo "no emeraldian binary found; build one or pass its path" >&2
    exit 2
fi
bin="$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")"

work="$(mktemp -d)"
session="emeraldian-render-$$"
pane="$work/pane.txt"
cleanup() {
    tmux kill-session -t "$session" 2>/dev/null || true
    rm -rf "$work"
}
trap cleanup EXIT

export HOME="$work/home"
unset XDG_CONFIG_HOME
mkdir -p "$HOME" "$work/vault"
if [ "${IMAGES:-on}" = off ]; then
    for dir in "$HOME/.config/emeraldian" "$HOME/Library/Application Support/emeraldian"; do
        mkdir -p "$dir"
        printf '[images]\nenabled = false\n' > "$dir/config.toml"
    done
fi

printf '# Alpha\n\nThe hub note.\n'          > "$work/vault/Alpha.md"
printf '# Beta\n\nPoints at [[Alpha]].\n'    > "$work/vault/Beta.md"
printf '# Gamma\n\nAlso points [[Alpha]].\n' > "$work/vault/Gamma.md"

# -f /dev/null so a personal tmux.conf can't change key handling. TERM inside
# the pane is tmux's own, which is the case this test exists for.
tmux -f /dev/null new-session -d -x 200 -y 45 -s "$session" \
    "HOME='$HOME' '$bin' '$work/vault'"

# Poll rather than sleep: a slow machine must not fail and a fast one must not
# wait. 40 x 0.25s is the ceiling, far more than a healthy start needs.
wait_for() { # <pattern> <what>
    for _ in $(seq 1 40); do
        tmux capture-pane -p -t "$session" > "$pane" 2>/dev/null || true
        if grep -q "$1" "$pane"; then return 0; fi
        sleep 0.25
    done
    echo "FAIL: timed out waiting for $2" >&2
    echo "--- last pane ---" >&2
    cat "$pane" >&2
    return 1
}

started=$(date +%s)

# Gamma is the last note listed, so its presence means the vault was read.
wait_for 'Gamma' 'the vault listing to appear'
for n in Alpha Beta Gamma; do
    grep -q "$n" "$pane" || {
        echo "FAIL: the file list does not show $n" >&2; cat "$pane" >&2; exit 1; }
done

# Open Alpha through the quick switcher, waiting for the screen to show each
# step took rather than for a fixed time.
tmux send-keys -t "$session" C-o
wait_for 'Quick switcher' 'the quick switcher to open'
tmux send-keys -t "$session" -l "Alpha"
# The query line: "Alpha" with only spaces after it, unlike the result line,
# which ends in "Alpha.md".
wait_for '› Alpha  *│' 'the switcher to take the query'
tmux send-keys -t "$session" Enter

wait_for 'The hub note' "Alpha's body to render"
# Beta and Gamma both link to Alpha, so a parser that never resolved the
# [[wikilinks]] would say 0 here.
wait_for '2 backlink' "Alpha's two backlinks"

echo "ok: drew the vault and opened Alpha in $(( $(date +%s) - started ))s ($(tmux -V))"
