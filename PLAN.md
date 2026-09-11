# PLAN — after the ACP swap: two forks

## Where we are (2026-09-11)

Branch `acp`: the in-process agent is swapped for an ACP v1 client that spawns
`crow-cli acp` (`0079a9a`), and the two launch bugs found while proving it are
fixed (`19c94a2`). Proven end-to-end:

- headless `--prompt` waits for the turn on the wire (`turn_done`), not on a flag
  that is false both before the first event and after the last;
- TUI smoke in tmux: Ctrl+L opens the Assistant panel, a prompt round-trips with
  streamed thinking, tool calls and the final answer in-panel, ctx footer live;
- raw-mode bug root-caused (ratatui-image's probe leaves a thread parked in a
  stdin read past its 2s timeout; the first keypress wakes it, its Busy send
  fails, and its deferred restore slams cooked termios over raw mode) and fixed
  locally by enabling raw mode *before* the probe. Upstream report filed:
  ratatui/ratatui-image#202.

## The two forks (user's spec, 2026-09-10)

Primary first:

1. **crow-cli's TUI in rust** — fork this repo into a NEW repo, completely
   rebrand/refactor: keep ONLY the chat/ACP client, themes and prompt input;
   sidebar hidden (not used; helix-in-a-tab is worse than another terminal);
   clear out everything else — notes/files/graph/outline/editor tabs. "Toad but
   in rust." This is the real target.
2. **Rebranded crow-like emeraldian** — keep the full obsidian-ish app with the
   ACP client in the side panel, rebrand it, long-form atproto later, maybe
   typst instead of markdown. Secondary; later.

## Step order for fork 1 (primary)

1. Copy this repo at branch `acp` into a new workspace sibling; fresh git
   history (or clone and drop the remote). Keep crate/bin names until branding
   is decided.
2. Hide the sidebar: not rendered, no keybind opens it; chat becomes the primary
   pane. Test: on launch the chat fills the window; Ctrl+L has nothing to toggle.
3. Strip the tabs: files/notes/helix/graph/outline/editor — delete their
   modules, keybinds and slash commands; keep chat, themes, prompt input, help.
   Test: fmt/clippy/tests green; tmux smoke shows chat on launch and a prompt
   round-trips.
4. Rebrand crate/bin once thomas picks a name (open question — do not invent).
5. Wire the vault/config paths to whatever makes sense without notes.

## Open questions (thomas's call, do not guess)

- Repo + crate name for fork 1: "keep calling it crow-term for now" and "toad
  but in rust" were both said; an existing `crow-term/` dir collides.
- Relationship to the existing `crow-term/` repo: its PLAN.md pivots to cursive
  (2026-09-01), which predates the emeraldian discovery — that pivot may be moot.
- Whether fork 2's rebrand happens before or after the atproto/typst experiments.
