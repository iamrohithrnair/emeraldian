# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.9.0]

### Added

- **A command line for everything the app does to a vault.** `read`, `info`,
  `outline`, `path`, `new`, `write`, `append`, `edit`, `props`, `rename`,
  `move`, `delete`, `link`, `task`, `daily` and `mkdir` work on notes.
  `list`, `find`, `search`, `tags`, `tasks`, `links` and `stats` find them,
  and `vaults` lists the vaults Obsidian knows about. `graph`, `graph local`,
  `graph path`, `graph hubs`, `graph orphans` and `graph unresolved` answer
  questions about the graph, and `graph export` hands it to Graphviz, Mermaid
  or anything that reads JSON. Output is plain lines, every list says how much
  it left out, and `--json` gives the same results to a program. No interface
  starts, and a note named by its path is read or written in a few
  milliseconds whatever the size of the vault. A write never leaves a
  half-written note behind, so it is safe while the app or Obsidian has the
  vault open. `emeraldian help` lists every command.

- **Recall: ask the vault a question.** `emeraldian recall <question>` finds
  what the vault knows about something, whatever words the notes use. It
  looks by meaning, with EmbeddingGemma 2, which finds what was said in other
  words or another language; by words, with BM25, which finds names,
  identifiers and error text exactly; and through links, with a walk over the
  vault's wikilinks that lifts the notes the best answers link to. A question
  nothing in the vault answers gets an empty answer rather than noise, and
  each answer comes with its best line, the answers it links to and the notes
  that bridge them. `--budget N` returns the passages themselves, in full, in
  about N tokens, the best of each answer first, ready for an agent's
  context, and `--folder` and `--limit` narrow it down. `search` is still a
  plain text search.

- **`emeraldian similar <note>`** lists the notes closest to a note and says
  which of them it is already linked to, so a link worth adding stands out.

- **Meaning comes from EmbeddingGemma 2, run by Ollama on your own machine.**
  `emeraldian index` sets it up. The first run checks for Ollama, downloads
  EmbeddingGemma 2 (`embeddinggemma-2:270m`, about 300 MB) through it once
  and indexes the vault, and later runs bring the index up to date.
  `--status` says what is indexed and whether the model is ready, and
  `--rebuild` indexes every note again. Without Ollama, recall works by words
  and links and `index` says how to add meaning; if Ollama is too old for the
  model, it says that instead. Each `recall` brings the index up to date
  first: words and links for every changed note, and vectors for up to 25 of
  them, sending the model only the passages whose text changed. The index
  lives in the cache directory, not the vault, and is read
  from disk as a question needs it rather than loaded into memory. Ollama
  keeps the model loaded for half an hour while questions keep coming, and
  for its own five minutes after one on its own.

- **`[embeddings]` in the config** chooses the server and the model:
  `provider` (`ollama`, or `openai` for LM Studio, llama.cpp's server, vLLM,
  OpenAI or anything else with OpenAI's embeddings API), `base_url`, `model`,
  `dimensions`, `keep_alive`, `query_prompt` and `document_prompt`. Passages
  and questions go to that server and nowhere else, and by default it is
  Ollama on your own machine. A key for a remote server comes from
  `EMERALDIAN_EMBEDDINGS_KEY`. After a change of `model`, `dimensions` or
  `document_prompt`, recall says to run `emeraldian index` to embed the notes
  again, and answers by words and links until then.

- **A skill for Claude Code**, in `skills/emeraldian-memory`, that tells an
  agent when to start a session, log, remember, recall and hand over with
  `emeraldian mem`.

- **Memory for coding agents.** `emeraldian mem` lets Claude Code, Codex or
  any other agent keep what it knows between sessions, as notes in the vault,
  under `Agents/` unless `folder` under `[memory]` says otherwise.
  `mem start` opens a session and prints where the last one left off: the
  project note, its handoff and log, the tasks it left open and what has been
  remembered. `mem log`, `mem remember`, `mem done` and `mem end` keep it up to
  date as the work goes on, and `mem end` records the branch, the commit and
  any uncommitted files by itself. `mem recall <question>` finds what was
  kept and logged by words and links, and by meaning once the vault is
  indexed. It ranks what was kept above the logs it came from, and takes
  `--budget N` as `recall` does. Each session links to its project and to the
  session before it, so a project's history shows in the graph.

- **`emeraldian config`** prints the settings, or changes one of them and
  leaves the rest of `config.toml`, comments included, as it was.

### Changed

- The assistant now suggests the note it thinks you meant when a name is
  misspelt, not only when it is incomplete.

### Fixed

- **A note in a big folder that isn't a vault opens in a second or two.**
  Opening a note there takes its folder as the vault, and a code project
  with `node_modules` holds a hundred thousand files besides its notes.
  Every link that pointed nowhere was checked against each of them in turn,
  so a design doc in a web app's repository took over a minute to open, and
  looked stuck. Links to files are now looked up by name in one step.

- A link to a name goes to the note with that file name, not to a note that
  has the name as an alias. It used to be the other way round whenever both
  existed, so a link could open a different note from the one Obsidian opens.
- A heading that ends in `#`, such as `## C#`, keeps its `#`. It used to be
  shown, and listed in the outline, as `C`.

## [0.8.0] — 2026-10-22

Shortcuts that follow your system, on Windows, Linux and the Mac alike, find
in the note on `Ctrl+F`, a focus mode on `F8`, long headings and words that
wrap in reading mode, and a fraction of the memory on a large vault. Notes and
a `config.toml` from an earlier version work unchanged. One shortcut has moved:
`Ctrl+T` now opens the quick switcher, and the theme picker is in the palette.

### Added

- **Shortcuts that follow your system.** Emeraldian now uses the conventions
  of the machine it runs on, alongside its own. On Windows and Linux,
  `Ctrl+Backspace` and `Ctrl+Delete` delete a word, `Alt+←` and `Alt+→` go
  back and forward through the notes you've visited, `Ctrl+Y` or
  `Ctrl+Shift+Z` redo, `Ctrl+PgUp` and `Ctrl+PgDn` change tab, `Ctrl+F4`
  closes one, `F5` reloads the vault and `F1` opens the help. On a Mac, `⌥←`
  and `⌥→` move by word and `⌥⌫` deletes one, `⌘←` and `⌘→` go to the ends of
  the line, `⌘↑` and `⌘↓` to the ends of the note, `⌘⌫` deletes back to the
  start of the line, `⌘[` and `⌘]` outdent and indent while you type and go
  back and forward otherwise, and `⌘⇧[` and `⌘⇧]` change tab. Every other
  shortcut works with `⌘` in place of `Ctrl`, so `⌘S`, `⌘Z`, `⌘⇧Z`, `⌘F` and
  the rest do what they do in other Mac apps, wherever the terminal passes `⌘`
  on to the app rather than keeping it for its own menus. `Ctrl` works in
  every terminal. Set `keyboard = "mac"`, `"windows"` or `"linux"` under
  `[ui]` to choose, for instance on a Linux machine you reach from a Mac over
  SSH; the default, `"auto"`, follows the machine.

- **Focus mode, on `F8`.** One key hides the explorer, the outline, the
  assistant and the ribbon, and centres the note at a readable width, for
  reading or writing with nothing else on screen. `F8` again puts everything
  back as it was, and so does asking for a sidebar with its own shortcut. The
  hint bar shows the key wherever you are. `F11` does the same in terminals
  that don't keep it for their own full screen, and the key can be moved under
  `[keys]` as `toggle_focus_mode`.

- **Find in the note, on `Ctrl+F`.** The lines of the open note that contain
  what you type are listed as you type it, ignoring case unless you type a
  capital. `Enter` goes to the one picked: while editing, the match is
  selected, so typing replaces it, and while reading it is scrolled into view,
  even halfway through a long paragraph. `Ctrl+F` again searches every note
  for the same text, as `Ctrl+Shift+F` does, which helps in terminals that
  send `Ctrl+Shift+F` as plain `Ctrl+F`. In vim's Normal mode `Ctrl+F` is
  still a page down and `/` still searches; opened from the palette or with
  `⌘F` there, the list puts the cursor on the match the way `/` does, so `n`
  goes on to the next one.

- **`Ctrl+Shift+T` reopens the last tab you closed**, as in a browser. It is
  in the palette too, for terminals that send `Ctrl+Shift+T` as `Ctrl+T`.

- **`Ctrl+K` makes a link while you type**, as in Obsidian. The selection
  becomes `[text]()` with the cursor where the address goes. When you aren't
  typing, `Ctrl+K` still cycles the sidebar's panels.

- In the editor, readline's `Alt+B` and `Alt+F` move by word, `Alt+D`
  deletes one and `Ctrl+U` deletes back to the start of the line, as in a
  shell. They are also what a Mac terminal sends for `⌥←`, `⌥→` and `⌥⌦`, so
  those work over SSH too. `Ctrl+Backspace` works in terminals that send it
  as `Ctrl+H`, as GNOME Terminal does. In vim mode `Ctrl+H` is Backspace, as
  in vim.

- `[keys]` can move `find_in_note`, `reopen_closed_tab` and `forward`, and
  reads `cmd` as `Ctrl`.

### Changed

- **Much less memory, however big the vault or the note.** On a test vault of
  20,000 notes and 176,000 links, Emeraldian now takes about 120 MB once it
  has loaded, down from 400 MB, and about 140 MB with the graph open, down from
  over 600 MB. Paging through a note of a few megabytes stays at the memory
  its first screen took, where it used to climb past 300 MB, and editing such
  a note for a while no longer runs into gigabytes. What changed:

  - The index keeps the stretch of text around each link, not the whole line,
    and keeps it once rather than twice. A note written as one long paragraph
    cost its whole text again for every link in it.
  - Undo history keeps only the lines each edit changed, rather than a copy of
    the whole note for every step, and stays within 32 MB.
  - Reading view lays a note out once and reuses that while you scroll,
    instead of reading, parsing and laying out the whole file on every key.
  - The graph is let go when you leave it. It used to stay in memory, and be
    rebuilt out of sight whenever a note was renamed or deleted. While it is
    settling, its layout reuses the memory each step works in.

- **The graph opens quickly on a large vault.** Building it compared each link
  with every link before it to drop duplicates, which took seven seconds for
  20,000 notes. It now takes milliseconds.

- **`Ctrl+T` opens the quick switcher, the nearest thing to a new tab.** It
  used to open the theme picker, which is now in the palette as "Change
  theme". To keep it on `Ctrl+T`, set `open_theme_picker = "ctrl+t"` under
  `[keys]`.

- For code using the `emeraldian-core` crate: `ResolvedLink::context` and
  `Backlink::context` are now `Arc<str>`, shared between the two, and
  `IndexedNote::frontmatter`, which nothing read, is gone.

### Fixed

- **Long headings and long words wrap in reading mode.** A heading longer
  than the pane was drawn on one row and cut off at its edge, maths and all.
  That is how a chat export pasted in as a single line looked: one heading,
  with every equation in it past the edge of the pane. Headings, callout
  titles and frontmatter values now wrap like paragraphs do, with their maths
  typeset and any `$$` equation set on rows of its own. A word wider than the
  pane, such as a long URL, is broken at the edge instead of running off it,
  and wrapping counts the columns text actually takes, so CJK text and emoji
  wrap where they are drawn.

- **An underscore inside a word stays part of it.** `snake_case_names` and
  footnote markers like `[^1_2]` lost their underscores to italics. As in
  CommonMark and Obsidian, an underscore now only emphasises at the edges of
  a word. Asterisks still work anywhere.

- **The note pane shows when it has the keyboard.** The explorer and the
  outline light up their borders when focused, but the note pane has no
  border, so pressing `Tab` into it looked as if nothing had happened. Its
  active tab now takes the same colour, with a bar in front of the title.

- **Characters typed with AltGr are typed.** Windows reports AltGr as
  `Ctrl+Alt`, so on keyboards like the German one, `@`, `{`, `[` and `€` were
  read as shortcuts: most of them did nothing, and `]` toggled the outline.
  A chord you set under `[keys]` with `Ctrl+Alt` still works.

- In a search box or a name prompt, `Ctrl+V` pastes, and other `Ctrl` and
  `⌘` shortcuts no longer type their letter.

- On Linux, a shortcut with the Super key that gets past the desktop no
  longer types its letter into the note.

## [0.7.0] — 2026-10-22

Maths typeset in reading mode, `Tab` that moves between panes from an open
note, and `emeraldian .` for the folder you are in, the way `code .` works.
Notes and a `config.toml` from an earlier version work unchanged.

### Added

- **Maths in reading mode.** `$…$` and `$$…$$` are typeset with Unicode
  instead of being shown as TeX: Greek letters and operators, scripts like `x²`
  and `aᵢ`, `ℝ` and the other double-struck and script letters, spaced the way
  TeX spaces them. Inline maths stays on its line, so a fraction reads as
  `(a + b)/2`. Display maths takes the rows it needs: fractions stack over a
  rule, sums and limits carry their bounds above and below, roots get a bar,
  and matrices, `cases` and `aligned` blocks get brackets as tall as they are.
  `\tag{1}` puts the number at the right margin, and a command it doesn't know
  is shown as typed rather than dropped.

  A single `$` now follows the same rule as Pandoc and Obsidian, so prices like
  "$5 and $10" stay text, and an underscore or asterisk inside maths is no
  longer paired with one in the text around it as emphasis. In the editor, a
  `$$` block is coloured as maths rather than read as Markdown.

- **`emeraldian .` opens the folder you are in, as `code .` does.** A relative
  path like `.` or `..` used to name the vault "vault" and file its saved state
  under the path as typed, so every folder opened that way shared one set of
  expanded folders, and "Open this note in Obsidian" asked for a vault called
  "vault". The path is now made absolute first. A note works too:
  `emeraldian ~/Notes/Projects/Plan.md` opens the vault the note belongs to,
  with the note open, and a path that doesn't exist says so.

### Fixed

- **`Tab` moves between panes from an open note.** Opening a note from the
  explorer gives it the focus, and the note pane kept `Tab` and `Shift+Tab` for
  itself even while you were reading, where they did nothing. That left no way
  back to the explorer without the mouse. They now move between panes from a
  note you are reading, from an empty note pane, and from vim's Normal and
  Visual modes, where they used to change the note instead. While you are
  typing, `Tab` still nests a list item; press `Esc` first to move on.

## [0.6.0] — 2026-09-26

Copy and paste through the system clipboard, shortcuts that can be moved to
keys every layout can reach, and settings the app writes for you that no
longer disturb the rest of `config.toml`. A `config.toml` written by an earlier
version loads unchanged; the new `[keys]` section is optional.

### Added

- **Copy and paste through the system clipboard.** While editing a note,
  `Ctrl+C`, `Ctrl+X` and `Ctrl+V` copy, cut and paste, so text can finally move
  between a note and another app. A paste replaces the selection, keeps its line
  breaks, and undoes in one step. With nothing selected, `Ctrl+C` and `Ctrl+X`
  say so rather than doing nothing silently.

  The terminal's own paste now arrives as one block, using bracketed paste.
  Into a note it goes in whole; into the chat box, a search, the palette or a
  name prompt it becomes a single line, because a pasted newline used to send
  a half-written chat message.

  Vim mode behaves like vim with `clipboard=unnamedplus`: `y`, `d`, `c`, `x`,
  `s` and the rest put text on the system clipboard, and `p`/`P` put whatever
  was copied last, here or elsewhere. `"+` and `"*` are accepted and mean the
  same register. In Visual mode `p` puts over the selection and keeps what was
  copied, `Ctrl+C` yanks, and `Ctrl+V` pastes over the selection.

  No new dependencies: the clipboard is reached through `pbcopy`/`pbpaste` on
  macOS, `wl-copy`/`wl-paste`, `xclip` or `xsel` on Linux, and PowerShell on
  Windows. Where none of those can copy, as over SSH, it falls back to OSC 52
  and asks the terminal to do it (tmux needs `set -g set-clipboard on`).

- **Shortcuts can be moved to other keys, under `[keys]` in `config.toml`.**
  `Ctrl+\` and `Ctrl+]` can't be typed on layouts where `\` and `]` sit
  behind AltGr, Spanish among them, which left both sidebar toggles out of
  reach from the keyboard. Any app-wide shortcut can now be given a key of
  your own, such as `toggle_left_sidebar = "alt+e"`. The default keeps working
  beside it, the palette shows the new key, and a name or key that isn't
  understood is reported at startup rather than quietly ignored. "Reset
  shortcuts to defaults" in the palette puts every key back, after asking.

### Fixed

- **`f`, `t` and `gg` work in Visual mode.** The key after them was read as a
  Visual command of its own, so `vfx` cut the selection instead of stretching
  it to the `x`, and `vgg` went nowhere. They now extend the selection as they
  do in vim.

- **Toggling vim mode changes one line of `config.toml`, not the whole file.**
  It used to write out every setting from memory, which dropped your comments
  and layout, and saved settings you had only changed for that session. It now
  edits just `vim` under `[editor]`, however it was written there, and leaves a
  file that doesn't parse alone rather than overwriting it. Resetting shortcuts
  works the same way.

## [0.5.0] — 2026-08-10

A minor rather than a patch release: vim mode is a new way to use the editor,
and the keyboard behaves differently while it is on. Nothing changes for anyone
who leaves it off, and a `config.toml` written by an earlier version still
loads — the new key has a default.

### Added

- **`F3` closes the tab**, in vim mode and out. `Ctrl+W` still does it with vim
  off, but vim spends that key on the window prefix, and every plain
  `Ctrl`+letter is already taken by the app, by vim, or by the terminal itself.
  `Ctrl+Shift+W` is not the answer either: without the Kitty protocol the Shift
  cannot be encoded, so it arrives as `Ctrl+W`, arms the prefix and swallows the
  next keystroke. F-keys are unambiguous everywhere, which is why `F4` carries
  the vim toggle too.

- **Vim mode in the note editor, on `F4`.** Normal, Insert, Visual and
  Visual-Line.

  Motions `h j k l`, `gj`/`gk`, `w W b B e E ge`, `0 ^ $`, `gg G`, `{ }`, and
  `f F t T` with `;`/`,` to repeat. `j` and `k` move by source line as they do
  in vim; `gj`/`gk` move by the row on screen.

  Operators `d c y > <` over any motion, or doubled for the line — `dd`, `cc`,
  `yy`, `>>`, `<<` — with counts anywhere they are accepted in vim, so `d3w`
  and `3dw` agree. Text objects `iw aw`, `i" a"`, `i( a(`, `i[ a[`, `i{ a{`,
  with nesting counted so an inner pair wins.

  Single keys `i I a A o O`, `x X s S`, `D C Y`, `p P`, `r`, `J`, `~`, `u`,
  `Ctrl+R`, and `Ctrl+A`/`Ctrl+X` to increment and decrement.

  One resolver defines every motion, and both operators and visual-mode
  movement go through it, so `dw` and `w` cannot disagree about where a word
  ends. Vim's exclusive/inclusive distinction is modelled rather than
  approximated: `dw` stops before the next word and `de` takes the last letter
  of this one.

  Outside Insert mode the Ctrl keys vim defines take vim's meanings rather than
  the app's: `Ctrl+R` redoes, `Ctrl+D`/`Ctrl+U` and `Ctrl+F`/`Ctrl+B` scroll,
  `Ctrl+A`/`Ctrl+X` adjust a number, and `Ctrl+O`/`Ctrl+I` walk the note
  history. In Insert mode only `Ctrl+W` and `Ctrl+U` are claimed, for vim's
  word- and line-delete. Everything else is unchanged there, in every other
  pane, and with vim mode off; shifted combinations are left alone, so
  `Ctrl+Shift+F` still searches.

  The app's commands move onto a `Space` leader and its panes onto `Ctrl+W`.
  Pressing `Space` draws a which-key menu built from the same table that binds
  the keys, so a binding cannot exist without being listed. `Ctrl+W h/j/k/l`
  moves between the explorer, note and sidebar, `[b`/`]b` step through tabs.

  Vim mode applies to the editor and not to reading: a note being read has no
  buffer to act on, so the reading pane keeps every key it always had and
  `Ctrl+E` is still the way in. The two navigation keys are the exception,
  because moving between panes and notes is not editing.

  A `:` line, typed along the bottom row rather than in a dialog: `:w` `:wq`
  `:x` `:q` `:q!` `:qa` `:e <name>` `:42` `:h`, plus `:set` for the handful of
  options worth changing mid-session and `:mkconfig` to write them down. Most
  are an existing action under a different name, so `:w` and `Ctrl+S` are not
  two implementations of saving.

  In-buffer search on `/` and `?`, with `n`/`N` to step and `:noh` to clear.
  A plain substring rather than a regular expression — notes are prose, and a
  half-supported regex dialect would be worse than an honest literal one — and
  case-insensitive until the pattern contains a capital, which is vim's
  `smartcase`. Every match on screen is highlighted, not only the one jumped to.

  `.` repeats the last change, including the text typed during it. It replays
  the keys rather than a parsed command, which is the one representation that
  covers an operator with a motion, a lone `x`, and an insert uniformly.

  Deliberately hard to get stuck in: the mode is named in the status bar, the
  cursor is a block in Normal and a bar in Insert, a pending `2d` is shown as
  it is typed, the hint bar leads with the way out, and the reference opens by
  itself the first time it is ever switched on. `F4` works from anywhere,
  including from inside Normal mode and over an open overlay.

  Off by default, and with it off the editor behaves exactly as it did before.

- `editor.vim` in `config.toml`, also reachable as `/vim on|off` and from the
  command palette. It is the one setting written the moment it changes rather
  than when settings are saved, because it decides what every key does and
  silently losing it on the next launch is a different order of problem.

- `Config::save_to`, mirroring `State::save_to`, so tests that write settings
  do not reach into the config directory of the machine running them.

- Forward navigation. `Action::Back` now records what it stepped away from, so
  there is something to return to; opening a note any other way clears the
  trail, as a browser does. Reachable as `Ctrl+I` in vim mode.

## [0.4.2] — 2026-08-07

Nothing in the app changed. This release exists to carry the packaging work,
and to prove that the release pipeline itself works end to end.

### Added

- `cargo install emeraldian` now works, without a git URL. All four crates are
  on crates.io, so the site and the README point at the registry.
- The crates.io workflow can be run by hand with `verify_only` to check that
  trusted publishing is still configured correctly, without publishing
  anything. Worth doing after renaming this repository or the workflow file,
  since either invalidates the trusted publisher silently.

### Fixed

- **The Homebrew tap is updated by the release, rather than only appearing to
  be.** The step guarded on `git diff`, which reports tracked files only, so a
  formula the tap had never held read as no change at all and was never
  committed. The job reported success on every release from the first one
  onwards while the tap stayed empty, which is why `brew install` had never
  worked.
- Tagging a release no longer fails when every crate is already published. The
  check that made a re-run a no-op sat after authentication, and authenticating
  is itself a failure until a trusted publisher exists.

## [0.4.1] — 2026-08-07

A packaging release. Nothing about using the app has changed.

### Changed

- **The library crates are named after the project**: `emeraldian-core`,
  `emeraldian-theme` and `emeraldian-agent`, where they were `otui-*`. They are
  about to go on crates.io, and a crate name cannot be changed or reclaimed once
  it is taken, so this was cheap now and impossible later.
- Two things deliberately kept their old names. Thread names read
  `emerald-agent` rather than `emeraldian-agent`, because Linux caps a thread
  name at 15 bytes and refuses anything longer, which would have left those
  threads nameless in a debugger. And `OTUI_BIN_DIR`, `OTUI_VERSION`,
  `OTUI_STATE_FILE` and `OTUI_CA_BUNDLE` keep their prefix, because they may
  already be exported in someone's shell profile and renaming them would not
  fail loudly, it would quietly ignore what they had set.

### Added

- The crates carry the metadata crates.io displays: repository, homepage,
  keywords and categories. They had none, and a published version is immutable.
- `.github/workflows/crates-io.yml`, publishing on a release tag through OIDC
  trusted publishing rather than a stored token.

## [0.4.0] — 2026-08-07

The release that renames the project. A minor bump rather than a patch: the
binary you type is a different word, and no amount of care inside the code
makes that anything other than breaking.

### Changed

- **The project is now called emeraldian.** It was `obsidian-tui`, and that name
  had to go: Obsidian is a trademark of Dynalist Inc., registered for exactly this
  class of software, and Obsidian now ships a terminal interface of its own — so
  the old name pointed at the wrong project twice over. One of Obsidian's
  maintainers asked, reasonably, and this is that change. Nothing about what the
  tool does has changed, and it still works on the same plain folder of Markdown
  files it always did.
- **The binary is `emeraldian`.** `emeraldian ~/Notes`, `emeraldian
  --list-vaults`, and so on. The `obsidian://` URIs the desktop app registers are
  still answered, and the `obsidian-dark` and `obsidian-light` themes still have
  the names they had — they describe Obsidian's own colour schemes, and they are
  written into config files that already exist.
- **Your settings move themselves.** Everything kept in
  `~/.config/obsidian-tui` — config, custom themes, saved agent sessions, UI
  state and stored API keys — is moved to `~/.config/emeraldian` the first time
  the renamed build starts. Nothing is copied or deleted, an existing
  `emeraldian` directory always wins, and a failure leaves the old directory
  exactly where it was.
- The Homebrew formula is `emeraldian`, and the npm package is `emeraldian`.
  A Homebrew install has to be replaced rather than upgraded, because the formula
  is a different one:
  `brew uninstall obsidian-tui && brew install iamrohithrnair/tap/emeraldian`.
- The site moved to <https://emeraldian-tui.github.io>, and the repository to
  `iamrohithrnair/emeraldian`. GitHub redirects the old links.

## [0.3.1] — 2026-08-06

### Added

- **`images.protocol` overrules the terminal's own answer.** Terminals are asked
  what they can draw and are believed, which is right almost everywhere; the
  exception is one that claims a protocol it doesn't paint, where the picture
  goes missing entirely and leaves a hole in the page. That is not a failure the
  terminal can be asked about, so it can now be told instead: `auto`, `kitty`,
  `iterm2`, `sixel` or `halfblocks`. Half-blocks are drawn out of ordinary
  coloured cells, so they work anywhere text does. The cell size still comes
  from the terminal, which answers that part correctly either way.

## [0.3.0] — 2026-08-05

The release that makes the editor usable for actually writing in: text wraps,
the cursor goes where it looks like it goes, and Markdown is styled while you
type it instead of only once you stop.

### Added

- **Long lines wrap while editing.** A line wider than the pane used to be cut
  off at the edge, and since the editor had no way to pan sideways either, the
  rest of it was not merely hidden but unreachable. `editor.wrap` has been in the
  config file since the first release, documented as doing exactly this, and was
  read by nothing; it now works, and setting it to `false` genuinely pans instead.
  A wrapped row is marked with a `⤷` in the gutter, so a soft wrap is still
  distinguishable from a real line break, and a wrapped list item is indented to
  sit under its own text rather than under its bullet.
- The arrow keys, `Home`, `End` and `PageUp`/`PageDown` move by the rows on
  screen rather than by lines in the file. On a paragraph that fills four rows,
  four presses of `Down` cross it — which is what every other editor does, and
  the only behavior that makes sense once text wraps.
- **Markdown is styled while you edit it**, the way Obsidian's live preview
  styles it: headings coloured and bold, bullets drawn as `•`, task boxes as
  `[☑]`, quotes as `▎`, and bold, italic, code, links, tags and `==highlights==`
  in the same colours the reading pane uses. Nothing is hidden and nothing moves:
  every glyph substituted in is exactly as wide as the character it stands for,
  which is what keeps the caret exactly where the character is. The line the
  cursor is on shows its syntax at full contrast, so what you edit is what you
  see, and because the widths match, the text does not shift as the cursor
  arrives or leaves. A fenced code block is drawn on its own background and its
  contents are never read as Markdown.
- **Clicking places the cursor, and dragging selects.** Both are resolved against
  the text as it was actually drawn, so they land on the character under the
  pointer however the line wrapped.
- **`Enter` continues a list.** `- `, `* `, `1. ` and `- [ ] ` carry down to the
  next line, an ordered list counts up, and a task always starts unchecked —
  carrying "done" onto a line nobody has done yet would be a lie. Pressing it on
  an item with nothing on it ends the list, which is how you stop.
- **`Tab` and `Shift+Tab` nest and unnest a list item**, or indent every line of
  a selection. In prose `Tab` is still a tab. `Shift+Tab` did nothing at all
  before.
- The status bar shows `Ln 12/40, Col 3` while editing, and how many characters
  are selected.

### Fixed

- **The mouse wheel now scrolls while editing.** It was writing to the reading
  view's scroll offset, which the editor never reads, so it silently did nothing.
- The caret was positioned by counting characters and then clamped to the pane,
  so on a long line it parked at the right margin and lied about where it was,
  and on CJK or emoji text — where one character is two columns wide — it was in
  the wrong place on any line containing one.
- **`Ctrl+E` no longer reflows the note.** Reading laid prose out from the pane's
  left edge while editing started it after the line-number gutter, so the two
  modes wrapped the same paragraph to different widths and every line broke
  somewhere else on the way in and out of the editor. The gutter's width is now
  reserved in both modes — blank while reading — so switching restyles the page
  without moving a word of it. It costs four columns of reading width, and
  nothing at all when line numbers are switched off.
- The scrollbar counted lines rather than rows, so it misreported how far through
  a note with any wrapped lines in it you were.
- The cursor line's highlight and a code block's background now reach the edge of
  the pane instead of stopping wherever the text happened to end.
- A caret was drawn in the note pane even when the keyboard belonged to the chat
  panel or the file explorer.
- The editor no longer scrolls past the end of a note that would fit on screen.
- **The outline scrolled to the wrong place.** Picking a heading set the reading
  view's scroll offset to the heading's line number in the file, but that offset
  counts *rendered rows* — and prose wraps, headings gain a rule under them and a
  picture takes a dozen rows, so the two numbers drift further apart the further
  down the note you go. The draw pass now writes down which row each block landed
  on, the same way it already records where it put every picture, and the jump is
  translated through it.

### Changed

- **Moved to the 2024 edition**, stable since Rust 1.85 and comfortably under
  the 1.90 floor the dependencies already set. Mostly invisible: `cargo fix`
  found three real changes and rustfmt's 2024 style edition reordered imports.
  The substantive one is that `env::set_var` is unsafe in 2024, which correctly
  flagged a bug — a test set an environment variable while other modules read
  the environment from parallel test threads. The state module now takes a path
  (`load_from` / `save_to`) so nothing has to mutate a global to be testable.
- Nested `if let` blocks collapsed into let-chains where 2024 allows it.

## [0.2.0] — 2026-08-05

The release that puts the parts of a vault a text reader could never show —
pictures and drawings — on the screen, and adds an assistant that can be set up
without leaving the app.

### Added

- **Pictures are drawn in the reading pane.** `![[chart.png]]` and
  `![alt](assets/chart.png)` render as real images in terminals that support
  Kitty's graphics protocol, iTerm2's, or sixel, and as half-block mosaics
  everywhere else. The terminal is asked what it can do at startup, before the
  alternate screen is entered, since that question is answered on stdin.
- Obsidian's `![[chart.png|400]]` width, in pixels, is honoured. Anything else
  after the pipe stays an alias, which is only ever used as alt text.
- Decoding happens off the draw loop, so a large photo doesn't stall scrolling.
  The rows a picture will need are worked out from its header on the first
  frame, so text below it doesn't jump when the picture arrives.
- Pictures are resampled with a Lanczos filter rather than nearest-neighbour.
  Nearest-neighbour keeps whichever pixel a sample lands on and discards the
  rest, which deletes most of the strokes that make text in a screenshot or a
  diagram legible. The cost is paid once, on the worker thread.
- `images.enabled` and `images.max_height_percent` in the config. The cap is a
  share of the reading pane rather than a fixed number of rows, so a picture is
  as large as the window allows: a cap small enough for an 80x24 terminal leaves
  a diagram unreadable on a full-screen one. A picture is never scaled up, and
  one wider than the pane is scaled down to fit.
- Startup reports which graphics protocol the terminal is using and how big one
  cell is. A terminal that quietly fell back to half-blocks is otherwise
  indistinguishable from one that drew the picture badly.
- **The reading pane pans sideways.** `←`/`→` (or `h`/`l`) move across content
  wider than the window; `g` returns to the top-left. Tables are laid out at
  their content's width and panned across, instead of being squeezed to fit —
  eight columns divided between a narrow pane left three characters each, which
  is not a narrow table but an unreadable one.
- A picture that can't be drawn — no support in the terminal, a missing file, a
  URL on the web — leaves its alt text in place rather than a hole.
- **Excalidraw notes open as drawings.** A `.excalidraw.md` note used to show a
  warning banner and a wall of compressed base64, which is the one thing in a
  vault that a text reader could make no sense of at all. The scene is now drawn
  as vectors on a braille canvas: rectangles, ellipses, diamonds, lines, arrows
  with heads, freehand strokes, and the text labels, rotation included. Both the
  plain `json` and the compressed `compressed-json` block the Obsidian plugin
  writes are read, as is a legacy `.excalidraw` JSON file.
- The drawing is scaled to the pane's width and scrolls vertically like the prose
  it replaces, since diagrams are usually far taller than a terminal.
- Excalidraw draws near-black ink on white paper. A stroke that would vanish into
  the theme's background is drawn in the theme's text colour instead; every colour
  the author chose deliberately is kept.
- **The assistant can be set up from inside the app.** `/provider` offers the
  eight backends it knows how to reach — Anthropic, OpenAI, Ollama, LM Studio,
  OpenRouter, Groq, a custom endpoint, or off — and choosing one sets its address
  as well, so nobody has to remember Ollama's port.
- `/model` asks the provider which models it actually has and offers the list.
  Better than a table shipped in the binary: names change monthly, and a local
  server's list depends on what you've pulled. The request runs off the draw loop,
  so the app stays usable while it waits.
- `/key` stores an API key for the current provider, typed into a prompt that
  shows dots. Kept in `auth.json` beside the config, mode `0600` — deliberately
  not in `config.toml`, which people commit to dotfiles repositories. An exported
  variable still wins, so nothing that worked before behaves differently.
- The chat panel's title now names the model that will answer, or says what is
  missing and which command fixes it.
- **The assistant works behind a corporate TLS proxy.** Such a proxy re-signs
  traffic with the company's own certificate authority, which is in the machine's
  trust store but not in the root list compiled into the binary, so every request
  failed with "unknown issuer". A CA bundle named by `SSL_CERT_FILE`,
  `CURL_CA_BUNDLE`, `REQUESTS_CA_BUNDLE`, `NODE_EXTRA_CA_CERTS`,
  `CARGO_HTTP_CAINFO`, `SSL_CERT_DIR` or `OTUI_CA_BUNDLE` is used instead — the
  same convention curl and everything built on OpenSSL follow, so a laptop already
  set up for that network needs no configuration. A directory of certificates works
  as well as a single file, and a bundle that can't be read leaves the built-in
  roots in place rather than trusting nothing.
- `/status` reports which roots and which proxy are in use, which is what
  distinguishes a missing CA from a missing proxy.
- Requests now share one HTTP client, so a turn with several tool calls reuses its
  connection instead of repeating the TLS handshake.
- **The vault opens with its folders closed, and reopens how you left it.**
  Listing every note at once buries the structure the folders exist to express.
  Which folders you left open is remembered per vault in `state.json`, beside
  the config — the folders left *open* are what's stored, so a folder added
  since the last run starts closed rather than springing open. `OTUI_STATE_FILE`
  moves the file elsewhere.
- An outline button in the ribbon, alongside the assistant's, so the right
  sidebar can be toggled with the mouse.
- The shortcut bar lists closing a tab and toggling each pane, and spills onto
  a second row when they don't fit one instead of silently dropping the last
  few.

### Fixed

- **`Ctrl+]` and `Ctrl+\` work.** Neither sidebar toggle could ever fire: a
  terminal without the Kitty keyboard protocol has no way to say "Ctrl and this
  punctuation key" and sends a single control byte instead, which arrives as
  `Ctrl+5` and `Ctrl+4`. Both forms are accepted now. The file explorer's toggle
  was broken the same way and went unnoticed only because the ribbon has a
  button for it.
- **Graph links are visible.** They borrowed the theme's border tone — `#2f2f2f`
  against a `#1e1e1e` background on obsidian-dark, about 1.3:1 — so the graph
  read as a scatter plot. Borders are drawn to be ignored; links are the point.
- Selecting a node now lights up its links properly. A braille cell holds one
  colour, so drawing every link in a single pass let an ordinary one rub out the
  highlight wherever they crossed — which is the middle of the picture.
- Shortcuts in the command palette are no longer clipped. They are right-aligned
  against the list and the scrollbar is painted down its last column, so
  `Ctrl+Shift+F` rendered as `Ctrl+Shift+`. The `?` overlay had the same
  collision.
- **The local graph is laid out on its own.** It was a filtered view of the whole
  vault: positioned among every other note, framed to their bounds, and drawn
  with edges running off to nodes outside the neighbourhood, so it arrived as a
  clump in the corner of an empty pane. The neighbourhood is now cut into a graph
  of its own before anything is positioned.
- The graph no longer rescales on every tick while its layout settles.
- **The graph layout comes to a stop.** It was left to an energy threshold that
  a real vault never reached: a few hundred interlinked notes oscillate below it
  indefinitely, so the graph drifted for its whole step budget — minutes of
  motion — and collapsed into an illegible smear as it went. Motion now decays
  every step, which bounds how far the layout can still travel and makes it
  settle in about a second, spread out and static.
- **Edges meet their nodes.** Nodes are glyphs drawn over edges the canvas
  painted, and the two resolved a position to a cell by different arithmetic —
  disagreeing by up to a whole cell, worse toward the right and bottom of the
  pane — so links stopped beside a note rather than at it.

### Changed

- `toml` moves from 0.9 to 1.1. The parts used here — `from_str` and
  `to_string_pretty` — kept their signatures, and an existing `config.toml`
  still loads. Every other dependency was already at its latest release; the
  lock file is refreshed to the newest compatible versions throughout.
- **The minimum supported Rust version is now 1.90**, up from 1.88. Drawing
  pictures means depending on `ratatui-image`, which reaches `quantette` for
  sixel quantization, and that requires 1.90 — the floor is set by the
  dependency chain rather than by anything in this code.
- **Arrow keys walk the graph.** They moved the camera, which left Tab as the
  only way through the picture, and Tab steps by link count so it can jump across
  the vault. Arrows now select the nearest node in the direction pressed; `hjkl`
  still moves the camera.
- Dragging a node with the mouse pins it, which the layout engine always
  supported and nothing called.
- `a` toggles attachments in the graph.
- **Arrow keys walk the slash-command list.** The list appeared on `/` but could
  only be used by typing a name you already knew; `↑`/`↓` now move through it and
  it scrolls to follow. `Esc` abandons a half-typed command before it closes the
  panel.
- `/logout` deletes the stored key rather than only saying the environment still
  has one, and only mentions the environment variable when it is actually set.

## [0.1.2] — 2026-07-27

### Added

- **The file explorer sorts by modification time.** Notes now open with the most
  recently edited at the top, which is usually where you left off. `s` in the
  explorer steps through the six orders (modified, created and file name, each
  both ways), as does `/sort`; `/sort list` shows them with the current one
  marked. Folders stay alphabetical whichever order is chosen, since a folder's
  timestamp changes for reasons that aren't visible on screen.
- The order is stored as `ui.sort_order` in the config, so it survives a
  restart. An unrecognised value falls back to the default rather than costing
  you the rest of the file.
- `NoteMeta` gained a `created` timestamp, falling back to the modification time
  on filesystems that don't record one.
- The `?` overlay gained a "File explorer" section, which it had been missing.

## [0.1.1] — 2026-07-27

### Fixed

- **Graph nodes render as solid discs.** Nodes were drawn by sampling a fixed
  17 points regardless of size, which left gaps as soon as a node was more than
  a couple of dots across and turned the graph into a field of speckle. They are
  now rasterized onto the braille canvas's own dot lattice. Unresolved links and
  attachments draw as hollow rings, so the notes you meant to write stand out.
- **Resetting the graph view frames the graph.** `0` recentred on the origin,
  but the force layout drifts away from it as it settles, so "reset" pushed the
  view off the graph entirely. It now fits to the layout's actual bounds.
- **Labels no longer cover the nodes they name.** Every node's full footprint is
  reserved before any label is placed.
- The graph legend has its own row instead of being painted over the canvas.
- Shortcut hints, the `?` overlay and `--help` listed keys that didn't exist or
  had the wrong case (`l` rather than `L` for labels). They now match the real
  bindings, and tests fail the build if they drift again.
- The README claimed Obsidian has no official CLI. It does.

### Added

- **Slash commands in the assistant panel.** Type `/` for a completion list;
  `Tab` completes, `Enter` runs. Switch backend with `/provider`, `/model` and
  `/base-url`, check credentials with `/login` and `/status`, keep and reload
  conversations with `/save`, `/resume` and `/sessions`, and free up context
  with `/compact`. Commands run locally and never reach the model.
- Conversations are saved as JSON beside the config file, not in the vault.
- **Obsidian CLI integration.** When Obsidian's [official
  CLI](https://obsidian.md/cli) is enabled, `/obsidian` reports its status and
  `/obsidian open` — or "Open this note in Obsidian" in the palette — hands the
  current note to the desktop app. Absent, or with the app closed, obsidian-tui
  says so and carries on.
- Graph keys for fitting (`f`), recentring on the selection (`c`), stepping
  between nodes (`n`/`N`) and rebuilding the layout (`r`).

### Changed

- **`q` quits, and asks first.** It works from the explorer, note pane, sidebar
  and graph, and the prompt names how many notes have unsaved changes. `q` stays
  a letter wherever you might be typing — the editor, the chat box, a search
  field — and `Ctrl+Q` is the way out from there.
- The quit and delete confirmations accept `Enter` as well as `y`; anything else
  still cancels.

## [0.1.0] — 2026-07-27

First release.

### Notes

- Three-pane Obsidian-style layout: icon ribbon, file explorer, note pane with
  tabs, and an outline/backlinks/tags sidebar.
- Live-preview Markdown rendering of Obsidian's dialect — `[[wikilinks]]`
  (dimmed when unresolved), `#tags`, `- [ ]` tasks, `> [!note]` callouts,
  tables, frontmatter, and fenced code with syntax highlighting for Rust,
  Python, JavaScript/TypeScript, Go and shell.
- A text editor with selection, word movement, grouped undo/redo, soft tabs and
  Markdown formatting shortcuts.
- Backlinks with the source line as context, a document outline, and a tag
  browser.
- Following a link to a note that doesn't exist creates it. Renaming a note
  rewrites every wikilink pointing at it. Deletions go to the vault trash.

### Graph

- Force-directed graph view with Barnes-Hut repulsion, so layout is O(n log n)
  and stops consuming CPU once it settles.
- Unresolved links appear as their own nodes — the notes you meant to write.
- Local graph for the open note, filters for tags, orphans and attachments, and
  pan/zoom/select.

### Assistant

- A chat panel backed by a native Rust agent runtime: streaming, tool calling,
  cancellation and usage accounting.
- Fifteen tools over the live vault — search, read, create, append, replace,
  link, rename, delete, list tags, inspect links and neighbourhoods, plus UI
  actions to open a note or focus the graph.
- Tools execute on the UI thread against the same state the user sees, so the
  agent's changes appear immediately.
- Providers: Anthropic Messages API, and any OpenAI-compatible server
  (Ollama, LM Studio, vLLM, OpenRouter) for fully offline use. With no
  credentials the panel explains how to configure one instead of failing.
- `--prompt` answers a single question on stdout without starting the TUI.
- `allow_writes = false` restricts the assistant to reading and searching.

### Interface

- Command palette, quick switcher, global content search, theme picker, vault
  switcher, help overlay, and input/confirmation prompts.
- Context-sensitive shortcut hint bar.
- Mouse support: clickable ribbon buttons, explorer rows, tabs, sidebar panels
  and graph nodes; the scroll wheel targets the pane under the pointer.
- Twenty themes including Obsidian's own light and dark, Catppuccin, Tokyo
  Night, Gruvbox, Nord, Solarized, Dracula, Rosé Pine, Everforest, and a
  `terminal` theme that inherits the terminal's palette. User themes can be
  added as TOML files that inherit from any built-in.

### Vault

- Discovers vaults from Obsidian's own `obsidian.json` on macOS, Linux and
  Windows, and works on any plain folder of Markdown.
- Accepts `obsidian://open`, `new` and `search` URIs alongside ordinary flags.
- Unreadable vaults are reported clearly, including the macOS privacy
  permission that usually causes it.

[0.9.0]: https://github.com/iamrohithrnair/emeraldian/releases/tag/v0.9.0
[0.8.0]: https://github.com/iamrohithrnair/emeraldian/releases/tag/v0.8.0
[0.7.0]: https://github.com/iamrohithrnair/emeraldian/releases/tag/v0.7.0
[0.6.0]: https://github.com/iamrohithrnair/emeraldian/releases/tag/v0.6.0
[0.5.0]: https://github.com/iamrohithrnair/emeraldian/releases/tag/v0.5.0
[0.4.2]: https://github.com/iamrohithrnair/emeraldian/releases/tag/v0.4.2
[0.4.1]: https://github.com/iamrohithrnair/emeraldian/releases/tag/v0.4.1
[0.4.0]: https://github.com/iamrohithrnair/emeraldian/releases/tag/v0.4.0
[0.3.1]: https://github.com/iamrohithrnair/emeraldian/releases/tag/v0.3.1
[0.3.0]: https://github.com/iamrohithrnair/emeraldian/releases/tag/v0.3.0
[0.2.0]: https://github.com/iamrohithrnair/emeraldian/releases/tag/v0.2.0
[0.1.2]: https://github.com/iamrohithrnair/emeraldian/releases/tag/v0.1.2
[0.1.1]: https://github.com/iamrohithrnair/emeraldian/releases/tag/v0.1.1
[0.1.0]: https://github.com/iamrohithrnair/emeraldian/releases/tag/v0.1.0
