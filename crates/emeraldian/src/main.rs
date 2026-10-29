//! emeraldian — a terminal UI for your Obsidian vault.

// Nothing here needs `unsafe`, and the one thing that reached for it —
// writing to the environment from a test — was undefined behaviour rather
// than a shortcut. `forbid` rather than `deny` so it cannot be waved through
// locally: re-introducing it should be a deliberate change to this line.
#![forbid(unsafe_code)]

mod actions;
mod agent;
mod app;
mod auth;
mod cli;
mod clipboard;
mod config;
mod copy;
mod editor;
mod explorer;
mod images;
mod keymap;
mod keys;
mod modal;
mod obsidian;
mod session;
mod slash;
mod state;
mod terminal_query;
mod tools;
mod ui;
mod vim;

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event};

use crate::app::{Action, App, View};
use crate::cli::{Args, Command};
use crate::config::Config;

/// How long to wait for input before redrawing.
///
/// Short enough that a settling graph animates and streamed agent text appears
/// promptly; long enough that an idle app uses no measurable CPU, since the
/// loop only redraws when something actually changed.
const TICK: Duration = Duration::from_millis(33);

fn main() -> io::Result<()> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut args = match cli::parse(&raw) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("emeraldian: {}", err.0);
            eprintln!("Try `emeraldian --help`.");
            std::process::exit(2);
        }
    };

    match args.command {
        Command::Help => {
            println!("{}", cli::HELP);
            return Ok(());
        }
        Command::Version => {
            println!("emeraldian {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Command::ListVaults => return list_vaults(),
        Command::Run => {}
    }

    // Before anything reads the config directory: a user upgrading from a build
    // that was still called obsidian-tui keeps their settings, themes, sessions
    // and stored keys.
    //
    // It sits below the commands above because none of them touch that
    // directory — `--list-vaults` reads Obsidian's own registry, not ours. A new
    // command that does read it belongs below this line, not above.
    config::migrate_legacy_dir();

    let (mut config, config_error) = Config::load();
    if let Some(theme) = args.theme.clone() {
        config.theme = theme;
    }

    let Some(target) = resolve_vault(&args, &config) else {
        eprintln!(
            "emeraldian: no vault found.\n\n\
             Pass one on the command line:\n    emeraldian ~/Notes\n    \
             emeraldian .           # the folder you are in\n\n\
             Or set it in the config:\n    vault = \"~/Notes\"\n\n\
             Registered Obsidian vaults can be listed with --list-vaults."
        );
        std::process::exit(1);
    };

    let (vault_path, note) = match locate(&target) {
        Ok(found) => found,
        Err(message) => {
            eprintln!("emeraldian: {message}");
            std::process::exit(1);
        }
    };
    // A note given by `--note` as well is the one that was asked for by name.
    if args.note.is_none() {
        args.note = note;
    }

    let vault = emeraldian_core::vault::Vault::from_path(&vault_path);
    // The terminal shows the vault while its notes are still being read. A
    // one-off question has no screen to show, and wants every note read.
    let opened = if args.prompt.is_some() {
        App::new(vault, config)
    } else {
        App::open(vault, config)
    };
    let mut app = match opened {
        Ok(app) => app,
        Err(err) => {
            eprintln!("emeraldian: {err}");
            if err.kind() == io::ErrorKind::PermissionDenied {
                // macOS gates ~/Documents, ~/Desktop and ~/Downloads behind a
                // per-application privacy permission. The prompt is attached to
                // the terminal, not to this binary, so it has to be granted
                // there.
                eprintln!(
                    "\nOn macOS this usually means your terminal hasn't been granted access to\n\
                     that folder. Grant it in:\n    \
                     System Settings → Privacy & Security → Files and Folders (or Full Disk Access)\n\
                     and restart the terminal."
                );
            }
            std::process::exit(1);
        }
    };

    if let Some(message) = config_error {
        app.error(format!("config ignored: {message}"));
    } else {
        // Write the defaults on first run so there's a documented file to edit
        // rather than a format the user has to guess at.
        let _ = app.config.ensure_exists();
    }

    // Headless mode: answer one question and exit, without taking the terminal.
    if let Some(prompt) = args.prompt.clone() {
        return run_prompt(&mut app, &prompt);
    }

    app.restore_ui_state();
    apply_startup_args(&mut app, &args);
    run(&mut app)
}

fn list_vaults() -> io::Result<()> {
    let vaults = emeraldian_core::vault::discover();
    if vaults.is_empty() {
        println!("No vaults are registered with Obsidian on this machine.");
        return Ok(());
    }
    for vault in vaults {
        println!(
            "{:<24} {}{}",
            vault.name,
            vault.path.display(),
            if vault.open { "  (open)" } else { "" }
        );
    }
    Ok(())
}

/// Picks a vault: the argument, then config, then Obsidian's own list.
fn resolve_vault(args: &Args, config: &Config) -> Option<PathBuf> {
    if let Some(path) = args.vault.clone() {
        return Some(expand_home(path));
    }
    if let Some(path) = config.vault.clone() {
        return Some(expand_home(path));
    }
    // Prefer the vault Obsidian currently has open, else the most recent.
    let vaults = emeraldian_core::vault::discover();
    vaults
        .iter()
        .find(|v| v.open)
        .or_else(|| vaults.first())
        .map(|v| v.path.clone())
}

/// What a path on the command line opens, read the way `code` reads one.
///
/// A folder is the vault itself, Obsidian's or not. A note opens in the vault
/// it belongs to, at that note: the nearest folder above it that Obsidian has
/// opened as a vault, so its links resolve against the rest of the vault, or
/// its own folder when there is none.
fn locate(path: &Path) -> Result<(PathBuf, Option<String>), String> {
    if path.is_dir() {
        return Ok((absolute(path), None));
    }
    if !path.exists() {
        return Err(format!("{} does not exist", path.display()));
    }
    if !emeraldian_core::vault::is_markdown(path) {
        return Err(format!(
            "{} is not a folder or a Markdown note",
            path.display()
        ));
    }

    let file = absolute(path);
    let folder = file
        .parent()
        .map_or_else(|| file.clone(), Path::to_path_buf);
    let vault = folder
        .ancestors()
        .find(|dir| dir.join(emeraldian_core::vault::OBSIDIAN_DIR).is_dir())
        .map_or_else(|| folder.clone(), Path::to_path_buf);
    let note = emeraldian_core::note::relative_path(&vault, &file);
    Ok((vault, note))
}

/// A path made absolute, with `.`, `..` and links resolved.
///
/// So that `emeraldian .` is named after the folder it stands for, rather than
/// after `.`, and what is remembered about a vault is filed under where it
/// really is, not under whichever relative path reached it this time.
fn absolute(path: &Path) -> PathBuf {
    let Ok(resolved) = std::fs::canonicalize(path) else {
        return path.to_path_buf();
    };
    // Windows resolves to a `\\?\C:\…` path, which is correct but is not how
    // anything else on the system shows a folder. A plain drive path loses
    // nothing without the prefix.
    #[cfg(windows)]
    if let Some(plain) = resolved
        .to_str()
        .and_then(|text| text.strip_prefix(r"\\?\"))
        .filter(|plain| !plain.starts_with("UNC\\"))
    {
        return PathBuf::from(plain);
    }
    resolved
}

/// Expands a leading `~`, which a config file or an unexpanded shell arg has.
fn expand_home(path: PathBuf) -> PathBuf {
    let Ok(text) = path.clone().into_os_string().into_string() else {
        return path;
    };
    let Some(rest) = text.strip_prefix('~') else {
        return path;
    };
    match dirs::home_dir() {
        Some(home) => home.join(rest.trim_start_matches('/')),
        None => path,
    }
}

fn apply_startup_args(app: &mut App, args: &Args) {
    if args.daily {
        let name = app.daily_note_name();
        app.open_or_create(&name);
    }
    if let Some(note) = &args.note {
        app.open_or_create(note);
    }
    if args.graph {
        let focus = app.active_note();
        app.open_graph(focus);
    }
    if let Some(query) = &args.search {
        actions::dispatch(app, Action::OpenSearch);
        if let Some(modal::Modal::Picker(picker)) = app.modal.as_mut() {
            for ch in query.chars() {
                picker.insert(ch);
            }
        }
        actions::update_search(app);
    }
}

/// Answers a single prompt on stdout, for scripting.
fn run_prompt(app: &mut App, prompt: &str) -> io::Result<()> {
    app.chat.input = prompt.to_string();
    agent::send(app);

    while app.chat.busy || app.chat.is_running() {
        agent::poll(app);
        std::thread::sleep(Duration::from_millis(20));
    }
    agent::poll(app);

    let mut failed = false;
    for entry in &app.chat.transcript {
        match entry {
            agent::Entry::Assistant(text) => println!("{text}"),
            agent::Entry::Tool { name, detail, .. } => eprintln!("[{name}] {detail}"),
            agent::Entry::Error(text) => {
                eprintln!("error: {text}");
                failed = true;
            }
            _ => {}
        }
    }

    if failed {
        std::process::exit(1);
    }
    Ok(())
}

fn run(app: &mut App) -> io::Result<()> {
    // Restore the terminal even on a panic; a raw-mode terminal left behind is
    // an unusable shell.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableMouseCapture,
            crossterm::event::DisableBracketedPaste,
            crossterm::event::PopKeyboardEnhancementFlags
        );
        // Vim mode leaves a block caret behind, which would otherwise outlive
        // the process and follow the user into their shell.
        reset_cursor_style();
        ratatui::restore();
        hook(info);
    }));

    // Asking the terminal what pictures it can draw means writing to stdout and
    // reading the reply from stdin, so it has to happen while the terminal is
    // still in its normal mode — before the alternate screen below.
    let wanted = images::choice(&app.config.images.protocol);
    app.images = images::Images::probe(
        app.config.images.enabled,
        app.config.images.max_height_percent,
        wanted,
    );
    match app.images.describe() {
        // Nothing to diagnose when the protocol was chosen rather than
        // detected: whatever it looks like, it is what was asked for.
        Some(protocol) if matches!(wanted, images::Choice::Use(_)) => {
            app.info(format!("images: {protocol}, set by config"));
        }
        // A terminal that quietly fell back to half-blocks is otherwise
        // indistinguishable from one that drew the picture badly, and the
        // difference decides whether there is anything to be done about it.
        Some(protocol) if app.images.is_coarse() => app.info(format!(
            "images: {protocol} — this terminal has no graphics protocol, \
             so pictures are coarse. Kitty, Ghostty, WezTerm or iTerm2 draw real pixels."
        )),
        Some(protocol) => app.info(format!("images: {protocol}")),
        None if app.config.images.enabled => {
            app.info("this terminal won't say what it can draw — images will show as alt text");
        }
        None => {}
    }
    // Said after the line above so it is the one left on screen: a
    // misspelled protocol is silently the same as `auto`, which is exactly
    // the sort of thing to spend an afternoon on.
    if wanted == images::Choice::Unknown {
        app.error(format!(
            "images.protocol = \"{}\" is not a protocol — asking the terminal instead. \
             Try auto, kitty, iterm2, sixel or halfblocks.",
            app.config.images.protocol
        ));
    }
    // Last for the same reason: a shortcut that silently does nothing is
    // the bug this section exists to get people out of.
    if let Some(problem) = app.keymap.problems.first() {
        let more = app.keymap.problems.len() - 1;
        let problem = if more == 0 {
            problem.clone()
        } else {
            format!("{problem} (and {more} more in [keys])")
        };
        app.error(problem);
    }

    // `init` panics when there's no terminal — in a pipe, a CI job, or a
    // headless shell. That's a normal way to invoke a program by mistake, so
    // it deserves an explanation rather than a backtrace.
    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(err) => {
            let _ = std::panic::take_hook();
            eprintln!(
                "emeraldian: this needs an interactive terminal ({err}).\n\n\
                 For scripting, use:\n    \
                 emeraldian --list-vaults\n    \
                 emeraldian <vault> --prompt \"your question\""
            );
            std::process::exit(1);
        }
    };

    // Mouse reporting isn't part of ratatui's init, so clicks and scrolling
    // only arrive if it's turned on explicitly. Bracketed paste likewise: with
    // it on, the terminal's own paste arrives as one block of text rather than
    // as keystrokes, each of which could be a command.
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::EnableMouseCapture,
        crossterm::event::EnableBracketedPaste
    );
    // The Kitty keyboard protocol, where the terminal speaks it, is what lets
    // a key arrive as itself: `⌘` on a Mac at all, and `Ctrl+Shift+T`,
    // `Ctrl+Tab` or `Ctrl+I` instead of the plainer key a legacy terminal
    // sends in their place. Asked for only where it is understood, so other
    // terminals see nothing new. Not crossterm's own check, which can wait
    // forever for its answer: this one gives up after a second, and doesn't
    // ask tmux or screen at all.
    let enhanced = terminal_query::supports_keyboard_enhancement();
    if enhanced {
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::PushKeyboardEnhancementFlags(
                crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            )
        );
    }

    let result = event_loop(&mut terminal, app);

    if enhanced {
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::PopKeyboardEnhancementFlags
        );
    }

    // Written here rather than from the quit action, so that driving the app
    // in a test never reaches into the real config directory.
    app.save_ui_state();

    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::DisableMouseCapture,
        crossterm::event::DisableBracketedPaste
    );
    reset_cursor_style();
    ratatui::restore();
    result
}

/// The caret shape that matches a vim mode, or `None` in emeraldian mode.
///
/// A block on a character and a bar between two is how every modal editor says
/// which one it is, and it is read without looking away from the text. `None`
/// means "don't touch it": with vim off the terminal's own caret is left
/// exactly as the user configured it.
fn cursor_style(app: &App) -> Option<crossterm::cursor::SetCursorStyle> {
    use crossterm::cursor::SetCursorStyle;

    if !app.config.editor.vim || !app.editing() || app.focus != app::Focus::Note {
        return None;
    }
    Some(match app.vim.mode {
        vim::VimMode::Insert => SetCursorStyle::SteadyBar,
        _ => SetCursorStyle::SteadyBlock,
    })
}

/// Puts the caret back the way it was found.
fn reset_cursor_style() {
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::cursor::SetCursorStyle::DefaultUserShape
    );
}

fn event_loop(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> io::Result<()> {
    let mut needs_redraw = true;
    let mut last_status = Instant::now();
    // Only written when it changes: re-sending the escape on every frame makes
    // some terminals flicker the caret.
    let mut cursor_shape: Option<crossterm::cursor::SetCursorStyle> = None;

    loop {
        // A settling graph animates and streamed agent text arrives between
        // keystrokes, so those drive redraws too.
        let graph_running = app.view == View::Graph
            && app
                .graph
                .as_ref()
                .is_some_and(|g| !g.simulation.is_settled());

        if needs_redraw || graph_running {
            terminal.draw(|frame| ui::draw(frame, app))?;
            needs_redraw = false;

            let wanted = cursor_style(app);
            if wanted != cursor_shape {
                match wanted {
                    Some(style) => {
                        let _ = crossterm::execute!(std::io::stdout(), style);
                    }
                    None => reset_cursor_style(),
                }
                cursor_shape = wanted;
            }
        }

        // While the vault's notes are being read, look for a key and go back to
        // reading, rather than waiting a whole tick for one.
        let wait = if app.indexing() { Duration::ZERO } else { TICK };
        if event::poll(wait)? {
            match event::read()? {
                Event::Key(key) => {
                    keys::handle(app, key);
                    needs_redraw = true;
                    last_status = Instant::now();
                }
                Event::Resize(_, _) => needs_redraw = true,
                // A click can say something too, as copying a diagram does,
                // and what it says gets the same time on screen as a key's.
                Event::Mouse(mouse) => {
                    if handle_mouse(app, mouse) {
                        needs_redraw = true;
                        last_status = Instant::now();
                    }
                }
                Event::Paste(text) => {
                    keys::paste(app, &text);
                    needs_redraw = true;
                }
                _ => {}
            }
        }

        if agent::poll(app) {
            needs_redraw = true;
        }

        // The vault's notes, read a slice at a time since the first frame.
        if app.poll_index() {
            needs_redraw = true;
        }

        // A picture that finished encoding has to be drawn into the space the
        // last frame already left for it.
        if app.images.poll() {
            needs_redraw = true;
        }

        // A provider that has answered with its model list opens the picker it
        // was asked for.
        if let Some(result) = app.lookup.take() {
            match result {
                Ok(models) => {
                    app.status.text.clear();
                    actions::show_models(app, models);
                }
                Err(err) => app.error(format!("could not list models: {err}")),
            }
            needs_redraw = true;
            last_status = Instant::now();
        }

        // Status messages fade so the bar goes back to showing the mode.
        if !app.status.text.is_empty() && last_status.elapsed() > Duration::from_secs(6) {
            app.status.text.clear();
            needs_redraw = true;
        }

        if app.quit {
            return Ok(());
        }
    }
}

/// Handles clicks and scrolling. Returns whether anything changed.
///
/// Everything is resolved against the regions the last frame recorded, so the
/// mouse hits exactly what was drawn — no second copy of the layout maths.
fn handle_mouse(app: &mut App, mouse: crossterm::event::MouseEvent) -> bool {
    use crossterm::event::{MouseButton, MouseEventKind};

    let point = (mouse.column, mouse.row);

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // A click anywhere dismisses an overlay, the way clicking outside a
            // modal does everywhere else.
            if app.modal.is_some() {
                app.modal = None;
                return true;
            }
            handle_click(app, point)
        }
        // Dragging over text selects it; dragging a node pins it where you put
        // it, which is how you untangle a knot of links that the layout has
        // folded onto itself.
        MouseEventKind::Drag(MouseButton::Left) => {
            place_caret(app, point, true) || drag_graph_node(app, point)
        }
        MouseEventKind::Up(MouseButton::Left) => release_graph_node(app),
        // A middle click closes a tab, as it does in a browser.
        MouseEventKind::Down(MouseButton::Middle) => match tab_at(&app.regions.tabs, point) {
            Some(index) => {
                app.close_tab_at(index);
                true
            }
            None => false,
        },
        // A right click on a diagram's copy mark copies its text, where a
        // left click copies the picture.
        MouseEventKind::Down(MouseButton::Right) if app.modal.is_none() => {
            match tab_at(&app.regions.diagram_marks, point) {
                Some(at) => {
                    copy::diagram(app, Some(at), copy::Format::Text);
                    true
                }
                None => false,
            }
        }
        // Shift turns the wheel sideways, as it does in a browser; a trackpad
        // or a tilting wheel reports sideways scrolling as its own events.
        MouseEventKind::ScrollDown if sideways(mouse) => handle_pan(app, point, keys::PAN_STEP),
        MouseEventKind::ScrollUp if sideways(mouse) => handle_pan(app, point, -keys::PAN_STEP),
        MouseEventKind::ScrollRight => handle_pan(app, point, keys::PAN_STEP),
        MouseEventKind::ScrollLeft => handle_pan(app, point, -keys::PAN_STEP),
        MouseEventKind::ScrollDown => handle_scroll(app, point, 3),
        MouseEventKind::ScrollUp => handle_scroll(app, point, -3),
        _ => false,
    }
}

/// The tab whose region, of those the last frame recorded, holds `point`,
/// or the same for any other list of numbered regions.
fn tab_at(regions: &[(ratatui::layout::Rect, usize)], point: (u16, u16)) -> Option<usize> {
    regions
        .iter()
        .find(|(rect, _)| hit(*rect, point))
        .map(|(_, index)| *index)
}

/// Maps a screen point to graph space using the bounds the last frame recorded.
fn graph_point(
    rect: ratatui::layout::Rect,
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
    (x, y): (u16, u16),
) -> emeraldian_core::graph::Vec2 {
    let fx = f64::from(x.saturating_sub(rect.x)) / f64::from(rect.width.max(1));
    let fy = f64::from(y.saturating_sub(rect.y)) / f64::from(rect.height.max(1));
    emeraldian_core::graph::Vec2::new(
        (x_bounds[0] + fx * (x_bounds[1] - x_bounds[0])) as f32,
        // Canvas y grows upward, terminal rows grow downward.
        (y_bounds[1] - fy * (y_bounds[1] - y_bounds[0])) as f32,
    )
}

fn drag_graph_node(app: &mut App, point: (u16, u16)) -> bool {
    let Some((rect, x_bounds, y_bounds)) = app.regions.graph else {
        return false;
    };
    let Some(graph) = app.graph.as_mut() else {
        return false;
    };

    // The grab happens on the first drag event rather than on the click, so a
    // plain click still just selects.
    let held = match graph.dragging {
        Some(node) => node,
        None => {
            if !hit(rect, point) {
                return false;
            }
            let radius = ((x_bounds[1] - x_bounds[0]) / 20.0) as f32;
            let Some(node) = graph
                .simulation
                .nearest(graph_point(rect, x_bounds, y_bounds, point), radius)
            else {
                return false;
            };
            graph.dragging = Some(node);
            graph.selected = Some(node);
            node
        }
    };

    graph
        .simulation
        .drag(held, graph_point(rect, x_bounds, y_bounds, point));
    true
}

fn release_graph_node(app: &mut App) -> bool {
    let Some(graph) = app.graph.as_mut() else {
        return false;
    };
    let Some(node) = graph.dragging.take() else {
        return false;
    };
    // Released back into the simulation rather than left pinned: a pin the user
    // can't see is a layout that never settles again for reasons they can't
    // explain.
    graph.simulation.release(node);
    true
}

fn hit(rect: ratatui::layout::Rect, (x, y): (u16, u16)) -> bool {
    x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height
}

fn handle_click(app: &mut App, point: (u16, u16)) -> bool {
    // Ribbon buttons run the same actions as their shortcuts.
    if let Some((_, action)) = app
        .regions
        .ribbon
        .iter()
        .find(|(rect, _)| hit(*rect, point))
        .map(|(rect, action)| (*rect, action.clone()))
    {
        actions::dispatch(app, action);
        return true;
    }

    // The close button sits inside its tab, so it is asked first.
    if let Some(index) = tab_at(&app.regions.tab_close, point) {
        app.close_tab_at(index);
        return true;
    }
    if let Some(index) = tab_at(&app.regions.tabs, point) {
        app.active_tab = Some(index);
        app.focus = app::Focus::Note;
        app.side_selected = 0;
        return true;
    }

    // The mark at a diagram's top right copies it as a picture.
    if let Some(at) = tab_at(&app.regions.diagram_marks, point) {
        app.focus = app::Focus::Note;
        copy::diagram(app, Some(at), copy::Format::Image);
        return true;
    }

    if let Some(panel) = app
        .regions
        .side_tabs
        .iter()
        .find(|(rect, _)| hit(*rect, point))
        .map(|(_, panel)| *panel)
    {
        app.side_panel = panel;
        app.side_selected = 0;
        app.focus = app::Focus::Sidebar;
        return true;
    }

    if let Some((rect, scroll)) = app.regions.explorer
        && hit(rect, point)
    {
        app.focus = app::Focus::Explorer;
        let row = scroll + (point.1 - rect.y) as usize;
        if row < app.explorer.len() {
            // One click acts, as it does in any file tree: a note opens, a
            // folder folds. Requiring a second click on folders only would
            // be an inconsistency the user has to learn.
            app.explorer.selected = row;
            match app.explorer.selected_note() {
                Some(id) => app.open_note(id),
                None => {
                    app.explorer.toggle(&app.index);
                }
            }
        }
        return true;
    }

    if let Some((rect, first)) = app.regions.sidebar
        && hit(rect, point)
    {
        app.focus = app::Focus::Sidebar;
        app.side_selected = first + (point.1 - rect.y) as usize;
        return true;
    }

    if let Some(rect) = app.regions.chat
        && hit(rect, point)
    {
        app.focus = app::Focus::Chat;
        return true;
    }

    // Clicking text puts the caret there, as it does in any editor.
    if place_caret(app, point, false) {
        app.focus = app::Focus::Note;
        return true;
    }

    // In the graph, a click selects the node nearest the pointer.
    if let Some((rect, x_bounds, y_bounds)) = app.regions.graph
        && hit(rect, point)
    {
        app.focus = app::Focus::Graph;
        if let Some(graph) = app.graph.as_mut() {
            // Generous radius: a node is a few characters wide on screen.
            let radius = ((x_bounds[1] - x_bounds[0]) / 20.0) as f32;
            let target = graph_point(rect, x_bounds, y_bounds, point);
            if let Some(node) = graph.simulation.nearest(target, radius) {
                graph.selected = Some(node);
            }
        }
        return true;
    }

    if let Some(rect) = app.regions.main
        && hit(rect, point)
    {
        app.focus = if app.view == View::Graph {
            app::Focus::Graph
        } else {
            app::Focus::Note
        };
        return true;
    }

    false
}

/// Puts the caret where the editor was clicked, extending the selection when
/// the pointer is being dragged.
///
/// Resolved against the text rectangle the last frame recorded, so a click lands
/// on the character that was actually drawn there however the line wrapped.
fn place_caret(app: &mut App, point: (u16, u16), extend: bool) -> bool {
    let Some((rect, scroll)) = app.regions.editor else {
        return false;
    };
    if !hit(rect, point) {
        return false;
    }
    let wrap = app.config.editor.wrap;
    let Some(editor) = app.editor_mut() else {
        return false;
    };

    let layout = editor.layout(rect.width as usize, wrap);
    let row = scroll + (point.1 - rect.y) as usize;
    let column = (point.0 - rect.x) as usize + editor.hscroll;
    editor.goto_visual(
        &layout,
        row,
        u16::try_from(column).unwrap_or(u16::MAX),
        extend,
    );
    true
}

fn sideways(mouse: crossterm::event::MouseEvent) -> bool {
    mouse
        .modifiers
        .contains(crossterm::event::KeyModifiers::SHIFT)
}

/// Pans the wide blocks of the note being read, when the pointer is over it.
///
/// Only reading mode pans: the editor follows its cursor sideways instead. The
/// next frame clamps the offset to what the blocks on screen can use.
fn handle_pan(app: &mut App, point: (u16, u16), delta: isize) -> bool {
    let over_note = app.regions.main.is_some_and(|rect| hit(rect, point));
    if !over_note || app.view != View::Notes {
        return false;
    }
    match app.active_mut() {
        Some(tab) if tab.mode == app::Mode::Reading => {
            tab.hscroll = (tab.hscroll as isize + delta).max(0) as usize;
            true
        }
        _ => false,
    }
}

/// Scrolls whichever pane is under the pointer, not whichever has focus.
fn handle_scroll(app: &mut App, point: (u16, u16), delta: isize) -> bool {
    if let Some((rect, _)) = app.regions.explorer
        && hit(rect, point)
    {
        app.explorer.page(delta);
        app.explorer.scroll_into_view(rect.height as usize);
        return true;
    }
    if let Some(rect) = app.regions.chat
        && hit(rect, point)
    {
        app.chat.follow = delta > 0;
        app.chat.scroll = (app.chat.scroll as isize + delta).max(0) as usize;
        return true;
    }
    // While editing, the note's scroll offset lives on the editor — it counts
    // wrapped rows, not lines — so the wheel has to move that one instead.
    let editing = app
        .active()
        .is_some_and(|tab| tab.mode == app::Mode::Editing);
    if editing && let Some(editor) = app.editor_mut() {
        editor.scroll = (editor.scroll as isize + delta).max(0) as usize;
        return true;
    }
    match app.active_mut() {
        Some(tab) => {
            tab.scroll = (tab.scroll as isize + delta).max(0) as usize;
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use emeraldian_core::test_support::TempVault;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn demo() -> (TempVault, App) {
        let vault = TempVault::new("mouse");
        vault.write("Alpha.md", "# Alpha\n\n[[Beta]]\n");
        vault.write("Beta.md", "# Beta\n");
        vault.write("Folder/Gamma.md", "# Gamma\n");
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        let alpha = app.index.id_of_rel("Alpha.md").expect("indexed");
        app.open_note(alpha);
        (vault, app)
    }

    /// Draws a frame so the click handler has regions to resolve against.
    fn lay_out(app: &mut App) {
        let mut terminal = Terminal::new(TestBackend::new(160, 40)).expect("terminal");
        terminal.draw(|frame| ui::draw(frame, app)).expect("draw");
    }

    fn click(app: &mut App, x: u16, y: u16) -> bool {
        handle_mouse(
            app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: crossterm::event::KeyModifiers::empty(),
            },
        )
    }

    fn scroll(app: &mut App, x: u16, y: u16, up: bool) -> bool {
        handle_mouse(
            app,
            MouseEvent {
                kind: if up {
                    MouseEventKind::ScrollUp
                } else {
                    MouseEventKind::ScrollDown
                },
                column: x,
                row: y,
                modifiers: crossterm::event::KeyModifiers::empty(),
            },
        )
    }

    #[test]
    fn sideways_scrolling_and_shift_wheel_pan_the_note_being_read() {
        let (_vault, mut app) = demo();
        app.active_mut().expect("tab").mode = app::Mode::Reading;
        lay_out(&mut app);
        let main = app.regions.main.expect("note pane");
        let (x, y) = (main.x + 2, main.y + 2);
        let wheel = |kind, modifiers| MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers,
        };
        let hscroll = |app: &App| app.active().expect("tab").hscroll;

        assert!(handle_mouse(
            &mut app,
            wheel(MouseEventKind::ScrollRight, KeyModifiers::NONE)
        ));
        assert_eq!(hscroll(&app), keys::PAN_STEP as usize);
        handle_mouse(
            &mut app,
            wheel(MouseEventKind::ScrollDown, KeyModifiers::SHIFT),
        );
        assert_eq!(hscroll(&app), keys::PAN_STEP as usize * 2);
        assert_eq!(
            app.active().expect("tab").scroll,
            0,
            "shift turns it sideways"
        );
        handle_mouse(
            &mut app,
            wheel(MouseEventKind::ScrollLeft, KeyModifiers::NONE),
        );
        handle_mouse(
            &mut app,
            wheel(MouseEventKind::ScrollUp, KeyModifiers::SHIFT),
        );
        assert_eq!(hscroll(&app), 0);

        // Over the explorer, it is not the note's to take.
        let (explorer, _) = app.regions.explorer.expect("explorer");
        let elsewhere = MouseEvent {
            column: explorer.x + 1,
            row: explorer.y + 1,
            ..wheel(MouseEventKind::ScrollRight, KeyModifiers::NONE)
        };
        assert!(!handle_mouse(&mut app, elsewhere));
        assert_eq!(hscroll(&app), 0);
    }

    fn drag(app: &mut App, x: u16, y: u16) -> bool {
        handle_mouse(
            app,
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: x,
                row: y,
                modifiers: crossterm::event::KeyModifiers::empty(),
            },
        )
    }

    /// A note long enough to scroll, opened in the editor and laid out once.
    fn editing(app: &mut App) {
        actions::dispatch(app, app::Action::ToggleMode);
        lay_out(app);
    }

    #[test]
    fn the_wheel_scrolls_the_editor_rather_than_the_reading_view() {
        // The two keep separate offsets — the editor counts wrapped rows — and
        // the wheel used to move the one the editor never reads, so it silently
        // did nothing while editing.
        let vault = TempVault::new("wheel");
        let body: String = (0..200).map(|i| format!("line {i}\n")).collect();
        vault.write("Long.md", &body);
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        let long = app.index.id_of_rel("Long.md").expect("indexed");
        app.open_note(long);
        editing(&mut app);

        assert!(scroll(&mut app, 80, 20, false), "the wheel is handled");
        assert_eq!(
            app.editor_mut().expect("editor").scroll,
            3,
            "the editor scrolled"
        );

        scroll(&mut app, 80, 20, true);
        assert_eq!(app.editor_mut().expect("editor").scroll, 0);
        scroll(&mut app, 80, 20, true);
        assert_eq!(
            app.editor_mut().expect("editor").scroll,
            0,
            "and stops at the top"
        );
    }

    #[test]
    fn clicking_the_editor_puts_the_cursor_where_the_character_is() {
        let (_v, mut app) = demo();
        editing(&mut app);

        let (text, _) = app.regions.editor.expect("the editor was drawn");
        // Row 2 is `[[Beta]]`; column 4 is its third character.
        click(&mut app, text.x + 4, text.y + 2);

        let cursor = app.editor_mut().expect("editor").cursor();
        assert_eq!(cursor.line, 2);
        assert_eq!(cursor.col, 4);
        assert_eq!(app.focus, app::Focus::Note);
    }

    #[test]
    fn dragging_across_the_editor_selects_what_it_crosses() {
        let (_v, mut app) = demo();
        editing(&mut app);

        let (text, _) = app.regions.editor.expect("the editor was drawn");
        click(&mut app, text.x, text.y);
        drag(&mut app, text.x + 5, text.y);

        assert_eq!(
            app.editor_mut().expect("editor").selected_text().as_deref(),
            Some("# Alp")
        );
    }

    /// The colour of the test picture. Half-blocks paint it into cell colours
    /// rather than glyphs, so this is what proves it reached the screen.
    const INK: ratatui::style::Color = ratatui::style::Color::Rgb(200, 30, 30);

    /// Presses a key and redraws, as the event loop would.
    fn press(terminal: &mut Terminal<TestBackend>, app: &mut App, code: KeyCode) {
        keys::handle(app, KeyEvent::new(code, KeyModifiers::empty()));
        terminal.draw(|frame| ui::draw(frame, app)).expect("draw");
    }

    fn type_keys(terminal: &mut Terminal<TestBackend>, app: &mut App, text: &str) {
        for ch in text.chars() {
            press(terminal, app, KeyCode::Char(ch));
        }
    }

    /// Rows of the drawn frame, as plain text.
    fn screen(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    /// The first row painted with the picture's colour.
    fn picture_row(terminal: &Terminal<TestBackend>) -> Option<usize> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .find(|&y| (0..buffer.area.width).any(|x| buffer[(x, y)].bg == INK))
            .map(usize::from)
    }

    /// The leftmost column painted with the picture's colour.
    fn picture_column(terminal: &Terminal<TestBackend>) -> Option<usize> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.width)
            .find(|&x| (0..buffer.area.height).any(|y| buffer[(x, y)].bg == INK))
            .map(usize::from)
    }

    /// Draws until the picture has been encoded, or gives up.
    fn draw_until_loaded(terminal: &mut Terminal<TestBackend>, app: &mut App) {
        for _ in 0..200 {
            terminal.draw(|frame| ui::draw(frame, app)).expect("draw");
            if picture_row(terminal).is_some() {
                return;
            }
            app.images.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!(
            "the picture never arrived:\n{}",
            screen(terminal).join("\n")
        );
    }

    #[test]
    fn a_picture_is_drawn_over_the_rows_reserved_for_it_and_scrolls_with_the_text() {
        let vault = TempVault::new("draw-image");
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(200, 100, {
            let ratatui::style::Color::Rgb(r, g, b) = INK else {
                unreachable!()
            };
            image::Rgb([r, g, b])
        }));
        let mut png = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode png");
        vault.write_bytes("chart.png", &png);
        // Long enough to scroll, and wide enough to pan: a note that fits the
        // pane in either direction never moves.
        let filler = "lorem ipsum\n\n".repeat(60);
        let wide = format!(
            "| {} |\n|{}|\n| {} |\n",
            (0..12)
                .map(|i| format!("col {i}"))
                .collect::<Vec<_>>()
                .join(" | "),
            "---|".repeat(12),
            (0..12)
                .map(|i| format!("val {i}"))
                .collect::<Vec<_>>()
                .join(" | "),
        );
        vault.write(
            "Note.md",
            &format!("# Note\n\n![a chart](chart.png)\n\nAfter\n\n{wide}\n{filler}"),
        );

        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        // The argument is the share of the pane a picture may fill, as in the
        // config; the real draw path supplies the pane height.
        app.images = images::Images::halfblocks(66);
        let note = app.index.id_of_rel("Note.md").expect("indexed");
        app.open_note(note);

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        draw_until_loaded(&mut terminal, &mut app);

        let top = picture_row(&terminal).expect("a row of the picture");
        let before = screen(&terminal);
        assert!(
            before.iter().any(|row| row.contains("After")),
            "the text below it is still there: {before:#?}"
        );
        assert!(
            !before.iter().any(|row| row.contains("a chart")),
            "the alt text gives way to the picture itself"
        );

        // Scrolling moves the picture with the prose rather than pinning it.
        app.active_mut().expect("tab").scroll = 2;
        terminal
            .draw(|frame| ui::draw(frame, &mut app))
            .expect("draw");
        assert_eq!(
            picture_row(&terminal),
            Some(top - 2),
            "it scrolled up with everything else"
        );

        // Panning moves the wide table, not the prose the picture sits in.
        app.active_mut().expect("tab").scroll = 0;
        terminal
            .draw(|frame| ui::draw(frame, &mut app))
            .expect("draw");
        let column = picture_column(&terminal);
        assert!(column.is_some(), "on screen to begin with");

        app.active_mut().expect("tab").hscroll = 3;
        terminal
            .draw(|frame| ui::draw(frame, &mut app))
            .expect("draw");
        assert_eq!(
            picture_column(&terminal),
            column,
            "it stays with the prose, which doesn't pan"
        );

        // Scrolled past its last row, it leaves nothing behind.
        app.active_mut().expect("tab").hscroll = 0;
        app.active_mut().expect("tab").scroll = 40;
        terminal
            .draw(|frame| ui::draw(frame, &mut app))
            .expect("draw");
        assert_eq!(
            picture_row(&terminal),
            None,
            "and scrolls fully out of view"
        );
    }

    #[test]
    fn an_excalidraw_note_is_shown_as_a_diagram_not_as_its_markdown() {
        let scene = r##"{"type": "excalidraw", "elements": [
            {"type": "rectangle", "x": 0, "y": 0, "width": 400, "height": 200,
             "strokeColor": "#e03131", "backgroundColor": "transparent",
             "fillStyle": "solid", "strokeStyle": "solid"},
            {"type": "text", "x": 40, "y": 80, "width": 200, "height": 25,
             "text": "Ingest", "strokeColor": "#1971c2",
             "backgroundColor": "transparent", "fillStyle": "hachure"},
            {"type": "arrow", "x": 400, "y": 100, "width": 120, "height": 0,
             "points": [[0, 0], [120, 0]], "strokeColor": "#2f9e44",
             "backgroundColor": "transparent", "fillStyle": "solid"}
        ]}"##;
        // Compressed, as Obsidian's plugin writes it by default.
        let packed = lz_str::compress_to_base64(scene);
        let vault = TempVault::new("draw-excalidraw");
        vault.write(
            "Flow.excalidraw.md",
            &format!(
                "---\nexcalidraw-plugin: parsed\n---\n\n\
                 # Excalidraw Data\n\n## Text Elements\nIngest ^abc\n\n\
                 ## Drawing\n```compressed-json\n{packed}\n```\n%%\n"
            ),
        );

        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        let note = app.index.id_of_rel("Flow.excalidraw.md").expect("indexed");
        app.open_note(note);

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        terminal
            .draw(|frame| ui::draw(frame, &mut app))
            .expect("draw");
        let rendered = screen(&terminal);
        let all = rendered.join("\n");

        assert!(
            all.chars().any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)),
            "the shapes are drawn as braille: {all}"
        );
        assert!(all.contains("Ingest"), "and the labels are readable: {all}");
        assert!(
            !all.contains("compressed-json") && !all.contains(&packed[..24]),
            "the base64 the drawing is stored as never reaches the screen"
        );
    }

    #[test]
    fn clicking_a_ribbon_icon_runs_its_action() {
        let (_v, mut app) = demo();
        lay_out(&mut app);

        // The graph icon is the third button.
        let (rect, _) = app.regions.ribbon[2];
        assert!(click(&mut app, rect.x + 1, rect.y));
        assert_eq!(
            app.view,
            View::Graph,
            "the ribbon graph icon opens the graph"
        );

        lay_out(&mut app);
        let (chat_rect, _) = app.regions.ribbon[3];
        assert!(click(&mut app, chat_rect.x + 1, chat_rect.y));
        assert!(
            app.config.ui.show_chat,
            "the assistant icon toggles the panel"
        );
    }

    #[test]
    fn clicking_a_tab_switches_to_it() {
        let (_v, mut app) = demo();
        let beta = app.index.id_of_rel("Beta.md").unwrap();
        app.open_note(beta);
        lay_out(&mut app);

        let (first, _) = app.regions.tabs[0];
        assert!(click(&mut app, first.x + 1, first.y));
        assert_eq!(app.active_tab, Some(0));
        assert_eq!(app.focus, app::Focus::Note);
    }

    /// Alpha, Beta and Gamma open, Gamma the active tab, laid out.
    fn three_tabs() -> (TempVault, App) {
        let (vault, mut app) = demo();
        for rel in ["Beta.md", "Folder/Gamma.md"] {
            let id = app.index.id_of_rel(rel).unwrap();
            app.open_note(id);
        }
        lay_out(&mut app);
        assert_eq!((app.tabs.len(), app.active_tab), (3, Some(2)));
        (vault, app)
    }

    fn active_title(app: &App) -> String {
        app.note_title(app.active_note().expect("a tab is open"))
    }

    #[test]
    fn a_tabs_close_button_closes_it_and_leaves_the_active_tab_alone() {
        let (_v, mut app) = three_tabs();
        let (close, index) = app.regions.tab_close[0];
        assert_eq!(index, 0);
        assert!(click(&mut app, close.x, close.y));
        let open: Vec<String> = app.tabs.iter().map(|t| app.note_title(t.note)).collect();
        assert_eq!(open, ["Beta", "Gamma"], "Alpha closed");
        assert_eq!(
            active_title(&app),
            "Gamma",
            "and Gamma is still the one shown"
        );
    }

    #[test]
    fn closing_the_active_tab_by_its_button_shows_the_next_one() {
        let (_v, mut app) = three_tabs();
        app.active_tab = Some(1);
        lay_out(&mut app);
        let (close, _) = app.regions.tab_close[1];
        assert!(click(&mut app, close.x + 1, close.y));
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(active_title(&app), "Gamma", "as F3 would have left it");
    }

    #[test]
    fn a_click_on_a_tabs_title_still_switches_to_it() {
        let (_v, mut app) = three_tabs();
        let (tab, _) = app.regions.tabs[0];
        let (close, _) = app.regions.tab_close[0];
        assert!(tab.x + 1 < close.x, "the title is not the button");
        assert!(click(&mut app, tab.x + 1, tab.y));
        assert_eq!(app.tabs.len(), 3, "nothing closed");
        assert_eq!(active_title(&app), "Alpha");
    }

    #[test]
    fn a_click_on_a_diagrams_mark_copies_it_as_a_picture_and_a_right_click_as_text() {
        let vault = TempVault::new("mouse-diagram");
        vault.write(
            "Flow.md",
            "# Flow\n\n```mermaid\ngraph LR\n  A[Start] --> B[End]\n```\n\nAfter.\n",
        );
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        let flow = app.index.id_of_rel("Flow.md").expect("indexed");
        app.open_note(flow);
        let (clipboard, copied) = clipboard::stand_in::StandIn::new();
        app.clipboard = clipboard;
        let mut terminal = Terminal::new(TestBackend::new(160, 40)).expect("terminal");
        terminal
            .draw(|frame| ui::draw(frame, &mut app))
            .expect("draw");

        let (mark, at) = app.regions.diagram_marks[0];
        assert_eq!(at, 0);
        let drawn = terminal.backend().buffer()[(mark.x + 1, mark.y)]
            .symbol()
            .to_string();
        assert_eq!(drawn, "⧉", "the mark is where its region is");

        assert!(click(&mut app, mark.x + 1, mark.y));
        assert_eq!(copied.borrow().images.len(), 1);
        assert!(copied.borrow().images[0].starts_with(b"\x89PNG"));
        assert!(
            app.status
                .text
                .starts_with("copied the diagram as a picture"),
            "{}",
            app.status.text
        );

        let right = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column: mark.x,
            row: mark.y,
            modifiers: KeyModifiers::empty(),
        };
        assert!(handle_mouse(&mut app, right));
        assert_eq!(copied.borrow().texts.len(), 1);
        assert!(copied.borrow().texts[0].contains("Start"));

        // Anywhere else on the diagram is just the note.
        click(&mut app, mark.x - 4, mark.y + 1);
        assert_eq!(copied.borrow().images.len(), 1, "nothing more copied");
    }

    #[test]
    fn a_middle_click_on_a_tab_closes_it() {
        let (_v, mut app) = three_tabs();
        let (tab, _) = app.regions.tabs[1];
        assert!(handle_mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Middle),
                column: tab.x + 1,
                row: tab.y,
                modifiers: crossterm::event::KeyModifiers::empty(),
            },
        ));
        let open: Vec<String> = app.tabs.iter().map(|t| app.note_title(t.note)).collect();
        assert_eq!(open, ["Alpha", "Gamma"]);
        assert_eq!(active_title(&app), "Gamma");
    }

    #[test]
    fn a_tab_closed_by_its_button_is_saved_first_as_f3_would() {
        let (vault, mut app) = three_tabs();
        app.config.editor.auto_save = true;
        app.active_tab = Some(0);
        let editor = app.editor_mut().expect("editor");
        editor.move_document_end(false);
        editor.insert_str("appended");
        app.active_tab = Some(2);
        lay_out(&mut app);
        let (close, _) = app.regions.tab_close[0];
        assert!(click(&mut app, close.x, close.y));
        assert!(
            vault.read("Alpha.md").contains("appended"),
            "saved, not lost"
        );
        assert_eq!(app.tabs.len(), 2);
    }

    #[test]
    fn clicking_the_explorer_selects_then_opens() {
        let (_v, mut app) = demo();
        lay_out(&mut app);
        let (rect, _) = app.regions.explorer.expect("explorer region");

        // First click on an unselected row selects it; a note opens directly.
        let row_of_beta = app
            .explorer
            .rows()
            .iter()
            .position(|r| r.name() == "Beta")
            .expect("Beta is listed");
        assert!(click(&mut app, rect.x + 2, rect.y + row_of_beta as u16));

        assert_eq!(
            app.note_title(app.active_note().expect("opened")),
            "Beta",
            "one click opens a note"
        );
        assert_eq!(
            app.focus,
            app::Focus::Note,
            "and focus follows it into the editor, as in Obsidian"
        );
    }

    #[test]
    fn clicking_a_folder_row_folds_it() {
        let (_v, mut app) = demo();
        lay_out(&mut app);
        let (rect, _) = app.regions.explorer.expect("explorer region");

        let folder_row = app
            .explorer
            .rows()
            .iter()
            .position(|r| r.name() == "Folder")
            .expect("Folder is listed");
        // Folders start closed, so the first click opens this one.
        let before = app.explorer.len();

        click(&mut app, rect.x + 2, rect.y + folder_row as u16);
        assert!(app.explorer.len() > before, "one click unfolds the folder");

        click(&mut app, rect.x + 2, rect.y + folder_row as u16);
        assert_eq!(app.explorer.len(), before, "and folds it again");
    }

    #[test]
    fn clicking_a_sidebar_tab_switches_panels() {
        let (_v, mut app) = demo();
        lay_out(&mut app);

        let (rect, panel) = app.regions.side_tabs[1];
        assert_eq!(panel, app::SidePanel::Backlinks);
        assert!(click(&mut app, rect.x + 1, rect.y));
        assert_eq!(app.side_panel, app::SidePanel::Backlinks);
    }

    #[test]
    fn clicking_a_graph_node_selects_it() {
        let (_v, mut app) = demo();
        app.open_graph(None);
        lay_out(&mut app);

        let (rect, x_bounds, y_bounds) = app.regions.graph.expect("graph region");
        // Aim at where the first node actually is.
        let node = app.graph.as_ref().unwrap().simulation.graph.nodes[0].pos;
        let fx = (f64::from(node.x) - x_bounds[0]) / (x_bounds[1] - x_bounds[0]);
        let fy = 1.0 - (f64::from(node.y) - y_bounds[0]) / (y_bounds[1] - y_bounds[0]);
        let x = rect.x + (fx * f64::from(rect.width)) as u16;
        let y = rect.y + (fy * f64::from(rect.height)) as u16;

        assert!(click(
            &mut app,
            x.min(rect.x + rect.width - 1),
            y.min(rect.y + rect.height - 1)
        ));
        assert_eq!(
            app.graph.as_ref().unwrap().selected,
            Some(0),
            "clicking a node selects it"
        );
    }

    #[test]
    fn scrolling_targets_the_pane_under_the_pointer() {
        let (_v, mut app) = demo();
        app.focus = app::Focus::Note;
        lay_out(&mut app);

        let (explorer, _) = app.regions.explorer.expect("explorer region");
        let before = app.explorer.selected;
        scroll(&mut app, explorer.x + 2, explorer.y + 2, false);
        assert_ne!(
            app.explorer.selected, before,
            "the explorer scrolls even though the note has focus"
        );

        let main = app.regions.main.expect("main region");
        scroll(&mut app, main.x + 4, main.y + 4, false);
        assert!(app.active().unwrap().scroll > 0, "the note scrolls");
    }

    #[test]
    fn a_click_dismisses_an_open_overlay() {
        let (_v, mut app) = demo();
        actions::dispatch(&mut app, Action::OpenPalette);
        lay_out(&mut app);

        assert!(click(&mut app, 5, 5));
        assert!(app.modal.is_none());
    }

    #[test]
    fn clicks_outside_every_region_are_ignored() {
        let (_v, mut app) = demo();
        lay_out(&mut app);
        assert!(!click(&mut app, 159, 39), "the status bar is not a target");
    }

    /// Setting the assistant up without leaving the keyboard or reading a manual:
    /// open the panel, type a slash, arrow to the command, pick a provider from
    /// the menu, type a key into the masked prompt.
    #[test]
    fn the_assistant_can_be_configured_from_the_panel_alone() {
        let (vault, mut app) = demo();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        app.auth = auth::Auth::at(vault.path().join("auth.json"));

        keys::handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.focus, app::Focus::Chat);

        // `/provider` reached by typing enough of it to be unambiguous.
        type_keys(&mut terminal, &mut app, "/prov");
        press(&mut terminal, &mut app, KeyCode::Enter);
        assert_eq!(app.chat.input, "/provider ", "Enter fills in the command");
        press(&mut terminal, &mut app, KeyCode::Enter);

        // The provider menu, with Anthropic at the top, taken as it stands.
        let Some(modal::Modal::Picker(picker)) = app.modal.as_ref() else {
            panic!("expected the provider menu, got {:?}", app.modal);
        };
        assert_eq!(picker.kind, modal::PickerKind::Providers);
        press(&mut terminal, &mut app, KeyCode::Enter);
        assert_eq!(app.config.agent.provider, "anthropic");

        // And a key, typed into a prompt that shows dots rather than the key.
        type_keys(&mut terminal, &mut app, "/key");
        press(&mut terminal, &mut app, KeyCode::Enter);
        type_keys(&mut terminal, &mut app, "sk-ant-typed");

        let rows = screen(&terminal).join("\n");
        assert!(
            !rows.contains("sk-ant-typed"),
            "the key must not appear on screen"
        );
        assert!(rows.contains("••••"), "masked instead:\n{rows}");

        press(&mut terminal, &mut app, KeyCode::Enter);
        assert_eq!(
            auth::key_for("anthropic", &app.auth).as_deref(),
            Some("sk-ant-typed"),
            "and it is what the next request will use"
        );
        assert!(
            screen(&terminal).join("\n").contains("Anthropic"),
            "the panel now says who is answering"
        );
    }

    #[test]
    fn home_expansion_only_touches_a_leading_tilde() {
        let expanded = expand_home(PathBuf::from("~/Notes"));
        assert!(!expanded.to_string_lossy().starts_with('~'));

        let absolute = PathBuf::from("/absolute/Notes");
        assert_eq!(expand_home(absolute.clone()), absolute);

        let mid = PathBuf::from("/a/~b");
        assert_eq!(expand_home(mid.clone()), mid);
    }

    #[test]
    fn the_command_line_vault_wins_over_config() {
        let args = Args {
            vault: Some(PathBuf::from("/from/args")),
            ..Default::default()
        };
        let config = Config {
            vault: Some(PathBuf::from("/from/config")),
            ..Default::default()
        };
        assert_eq!(
            resolve_vault(&args, &config),
            Some(PathBuf::from("/from/args"))
        );
    }

    #[test]
    fn config_supplies_the_vault_when_no_argument_is_given() {
        let config = Config {
            vault: Some(PathBuf::from("/from/config")),
            ..Default::default()
        };
        assert_eq!(
            resolve_vault(&Args::default(), &config),
            Some(PathBuf::from("/from/config"))
        );
    }

    #[test]
    fn a_folder_on_the_command_line_is_the_vault() {
        let vault = TempVault::new("locate-folder");
        let (path, note) = locate(vault.path()).expect("a folder opens");
        assert_eq!(path, absolute(vault.path()));
        assert_eq!(note, None);
    }

    #[test]
    fn a_relative_folder_is_named_after_the_folder_it_stands_for() {
        // `emeraldian .` used to be a vault called "vault", remembered under
        // ".", which every folder opened that way then shared.
        let (path, _) = locate(Path::new(".")).expect("the current folder opens");
        let here = absolute(&std::env::current_dir().expect("cwd"));
        assert!(path.is_absolute(), "{}", path.display());
        assert_eq!(path, here);
        assert_eq!(
            emeraldian_core::vault::Vault::from_path(&path).name,
            here.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
        );

        let vault = TempVault::new("locate-dots");
        std::fs::create_dir_all(vault.path().join("sub")).expect("mkdir");
        let (path, _) = locate(&vault.path().join("sub").join("..")).expect("opens");
        assert_eq!(path, absolute(vault.path()));
    }

    #[test]
    fn a_note_on_the_command_line_opens_in_its_vault() {
        let vault = TempVault::new("locate-note");
        std::fs::create_dir_all(vault.path().join(".obsidian")).expect("mkdir");
        let plan = vault.write("Projects/Plan.md", "# Plan\n\n[[Other]]\n");
        vault.write("Other.md", "# Other\n");

        let (path, note) = locate(&plan).expect("a note opens");
        assert_eq!(
            path,
            absolute(vault.path()),
            "the vault it belongs to, not just its folder"
        );
        assert_eq!(note.as_deref(), Some("Projects/Plan.md"));

        let mut app = App::new(
            emeraldian_core::vault::Vault::from_path(&path),
            Config::default(),
        )
        .expect("app");
        let args = Args {
            note,
            ..Default::default()
        };
        apply_startup_args(&mut app, &args);
        assert_eq!(
            app.active_note(),
            app.index.id_of_rel("Projects/Plan.md"),
            "and the note is what opens"
        );
    }

    #[test]
    fn a_note_in_no_vault_opens_in_its_own_folder() {
        let vault = TempVault::new("locate-loose");
        let loose = vault.write("Loose/Note.md", "# Note\n");
        let (path, note) = locate(&loose).expect("a note opens");
        assert_eq!(path, absolute(&vault.path().join("Loose")));
        assert_eq!(note.as_deref(), Some("Note.md"));
    }

    #[test]
    fn a_missing_path_or_a_file_that_is_not_a_note_is_reported() {
        let vault = TempVault::new("locate-errors");
        let missing = locate(&vault.path().join("Nope")).expect_err("nothing there");
        assert!(missing.contains("does not exist"), "{missing}");

        let picture = vault.write("chart.png", "not really a png");
        let not_a_note = locate(&picture).expect_err("not a note");
        assert!(
            not_a_note.contains("not a folder or a Markdown note"),
            "{not_a_note}"
        );
    }
}
