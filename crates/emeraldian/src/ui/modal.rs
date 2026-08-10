//! Overlay rendering: pickers, prompts, confirmations and help.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Widget};

use emeraldian_theme::Palette;

use crate::app::App;
use crate::modal::{Confirm, Modal, Picker, Prompt};
use crate::ui::{centered, pane_block, scrollbar, truncate};

pub fn draw(frame: &mut Frame, app: &mut App, palette: &Palette, area: Rect) {
    // Read before the modal is borrowed mutably below.
    let vim = app.config.editor.vim;
    let Some(modal) = app.modal.as_mut() else {
        return;
    };
    match modal {
        Modal::Picker(picker) => draw_picker(frame, picker, palette, area),
        // The vim command and search lines are drawn on the status row by
        // `ui::draw`, where every editor puts them; a dialog in the middle of
        // the screen for `:w` would be jarring.
        Modal::Prompt(prompt) if is_command_line(prompt) => {}
        Modal::Prompt(prompt) => draw_prompt(frame, prompt, palette, area),
        Modal::Confirm(confirm) => draw_confirm(frame, confirm, palette, area),
        Modal::Help(scroll) => draw_help(frame, scroll, palette, area, vim),
    }
}

fn draw_picker(frame: &mut Frame, picker: &mut Picker, palette: &Palette, area: Rect) {
    let width = (area.width * 3 / 4).clamp(40, 96);
    let height = (area.height * 2 / 3).clamp(8, 24);
    let rect = centered(area, width, height);

    frame.render_widget(Clear, rect);
    let block = pane_block(picker.kind.title(), true, palette, palette.bg_secondary);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(inner);

    // Query line.
    let query = if picker.query.is_empty() {
        Span::styled(
            picker.kind.placeholder(),
            Style::default().fg(palette.text_faint),
        )
    } else {
        Span::styled(
            picker.query.clone(),
            Style::default().fg(palette.text_normal),
        )
    };
    Paragraph::new(vec![
        Line::from(vec![
            Span::styled("› ", Style::default().fg(palette.text_accent)),
            query,
        ]),
        Line::from(Span::styled(
            "─".repeat(rows[0].width as usize),
            Style::default().fg(palette.border),
        )),
    ])
    .render(rows[0], frame.buffer_mut());

    frame.set_cursor_position((rows[0].x + 2 + picker.cursor as u16, rows[0].y));

    let list = rows[1];
    let visible_height = list.height as usize;
    picker.scroll_into_view(visible_height);

    if picker.is_empty() {
        Paragraph::new(Line::from(Span::styled(
            "  no matches",
            Style::default().fg(palette.text_faint),
        )))
        .render(list, frame.buffer_mut());
        return;
    }

    let selected = picker.selected;
    let scroll = picker.scroll;
    // One column short of the list, because the scrollbar is painted down the
    // last one. Shortcuts are right-aligned, so without this the end of every
    // one of them is covered — `Ctrl+Shift+F` reads as `Ctrl+Shift+`. Reserved
    // whether or not the bar is showing, so entries don't shift sideways by a
    // column as the list is filtered.
    let width = list.width.saturating_sub(1) as usize;

    let lines: Vec<Line> = picker
        .visible()
        .enumerate()
        .skip(scroll)
        .take(visible_height)
        .map(|(index, (entry, positions))| {
            let active = index == selected;
            let background = if active {
                palette.bg_active
            } else {
                palette.bg_secondary
            };

            let mut spans = vec![Span::styled(
                if active { "› " } else { "  " },
                Style::default().fg(palette.text_accent).bg(background),
            )];

            // Underline the characters that matched, so the ranking is legible.
            let label_style = Style::default()
                .fg(if active {
                    palette.text_normal
                } else {
                    palette.text_muted
                })
                .bg(background);
            for (i, ch) in entry.label.chars().enumerate() {
                let matched = positions.contains(&byte_of(&entry.label, i));
                spans.push(Span::styled(
                    ch.to_string(),
                    if matched {
                        label_style
                            .fg(palette.text_accent)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        label_style
                    },
                ));
            }

            let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
            let detail = truncate(&entry.detail, width.saturating_sub(used + 2));
            let pad = width.saturating_sub(used + detail.chars().count());
            spans.push(Span::styled(
                " ".repeat(pad),
                Style::default().bg(background),
            ));
            spans.push(Span::styled(
                detail,
                Style::default().fg(palette.text_faint).bg(background),
            ));

            Line::from(spans)
        })
        .collect();

    Paragraph::new(lines).render(list, frame.buffer_mut());
    scrollbar(frame, palette, list, scroll, picker.len());
}

/// Byte offset of the nth character, matching how fuzzy positions are recorded.
fn byte_of(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(byte, _)| byte)
}

fn draw_prompt(frame: &mut Frame, prompt: &Prompt, palette: &Palette, area: Rect) {
    let rect = centered(area, 60.min(area.width), 3);
    frame.render_widget(Clear, rect);

    let block = pane_block(&prompt.title, true, palette, palette.bg_secondary);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    // A secret is masked, but its length shows, so a paste that arrived short or
    // doubled is visible.
    let shown = if prompt.intent.secret() {
        "•".repeat(prompt.value.chars().count())
    } else {
        prompt.value.clone()
    };

    Paragraph::new(Line::from(vec![
        Span::styled("› ", Style::default().fg(palette.text_accent)),
        Span::styled(shown, Style::default().fg(palette.text_normal)),
    ]))
    .render(inner, frame.buffer_mut());

    frame.set_cursor_position((inner.x + 2 + prompt.cursor as u16, inner.y));
}

fn draw_confirm(frame: &mut Frame, confirm: &Confirm, palette: &Palette, area: Rect) {
    let rect = centered(area, 60.min(area.width), 5);
    frame.render_widget(Clear, rect);

    let block = pane_block("Confirm", true, palette, palette.bg_secondary);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    Paragraph::new(vec![
        Line::from(Span::styled(
            confirm.message.clone(),
            Style::default().fg(palette.text_normal),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "y / Enter",
                Style::default()
                    .fg(palette.text_error)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" confirm    ", Style::default().fg(palette.text_muted)),
            Span::styled(
                "n / Esc",
                Style::default()
                    .fg(palette.text_success)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" cancel", Style::default().fg(palette.text_muted)),
        ]),
    ])
    .render(inner, frame.buffer_mut());
}

/// The keybinding reference.
///
/// Grouped the way the app is: navigation, notes, panes, graph, assistant.
const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "Navigation",
        &[
            ("Ctrl+O", "Quick switcher: open a note by name"),
            ("Ctrl+P", "Command palette"),
            ("Ctrl+Shift+F", "Search all notes"),
            ("Tab / Shift+Tab", "Move between panes"),
            ("hjkl / arrows", "Move within a pane"),
            ("Enter", "Open the selection / follow a link"),
            ("Alt+←", "Back to the previous note"),
            ("Esc", "Close an overlay"),
            ("?", "This help"),
            ("q", "Quit (asks first); Ctrl+Q works while editing"),
        ],
    ),
    (
        "Notes",
        &[
            ("Ctrl+E", "Toggle reading and editing"),
            ("j / k", "Scroll while reading"),
            ("h / l", "Pan across a wide table while reading"),
            ("g / G", "Top / bottom of the note"),
            ("Ctrl+N", "New note"),
            ("Ctrl+S", "Save"),
            ("Ctrl+D", "Today's daily note"),
            ("F2", "Rename the open note"),
            (
                "F3 / Ctrl+W",
                "Close the tab — Ctrl+W unless vim mode is on",
            ),
            ("Ctrl+Tab", "Next tab"),
            ("Ctrl+B / Ctrl+I", "Bold / italic (while editing)"),
            ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
        ],
    ),
    (
        "Editing a note",
        &[
            ("↑ / ↓", "Up and down a line as it's wrapped on screen"),
            ("Home / End", "Start and end of the line on screen"),
            (
                "Enter",
                "New line, carrying a list marker; twice ends the list",
            ),
            (
                "Tab / Shift+Tab",
                "Nest or unnest a list item; a tab in prose",
            ),
            ("click / drag", "Place the cursor / select text"),
            ("Ctrl+Space", "Start a selection without holding Shift"),
            ("Ctrl+A", "Select the whole note"),
            ("Ctrl+Shift+K", "Delete the line"),
            ("Ctrl+←/→", "By word"),
            ("Ctrl+Home / End", "Start and end of the note"),
        ],
    ),
    (
        "Vim mode",
        &[
            ("F4", "Turn vim mode on and off — saved straight away"),
            ("i / a / o", "Insert before, after, or on a new line"),
            ("Esc", "Back to Normal mode; again to stop editing"),
            (
                "h j k l",
                "Left, down, up, right — j/k by line, gj/gk by row",
            ),
            ("w / b / e", "Forwards, back, and to the end of a word"),
            ("0 / ^ / $", "Start of line / first word / end of line"),
            ("gg / G", "Top / bottom of the note"),
            ("{ / }", "Previous / next paragraph"),
            ("f / t", "To a character on this line; ; and , repeat it"),
            ("d c y > <", "Delete, change, yank, indent — over a motion"),
            ("dd cc yy", "The doubled form acts on the whole line"),
            ("diw ci\" da(", "Act on a word, a quoted string, a bracket"),
            ("v / V", "Select by character / by line"),
            ("x / X / s", "Delete forwards, backwards, or and type"),
            ("D / C / Y", "Delete, change or yank to the end of the line"),
            ("p / P", "Put after / before"),
            ("r / J / ~", "Replace a character, join lines, flip case"),
            ("u / Ctrl+R", "Undo / redo"),
            ("3dd, d3w", "A count repeats what follows it"),
            ("Ctrl+A / Ctrl+X", "Increment / decrement the number here"),
            ("Ctrl+D / Ctrl+U", "Half a page down / up"),
            ("Ctrl+F / Ctrl+B", "A page down / up"),
        ],
    ),
    (
        "Vim mode: getting around",
        &[
            ("Space", "The leader menu — every app command, listed"),
            ("Ctrl+W h/j/k/l", "Move to the explorer, note, or sidebar"),
            ("Ctrl+W w / c", "Cycle panes / close the tab"),
            ("[b / ]b", "Previous / next tab"),
            ("Ctrl+O / Ctrl+I", "Back and forward through visited notes"),
        ],
    ),
    (
        "Vim mode: the : and / lines",
        &[
            (":w :wq :q", "Save, save and close, close the tab"),
            (":qa / :qa!", "Quit, asking first or not"),
            (":e <name>", "Open a note, creating it if it's missing"),
            (":42", "Jump to a line number"),
            (":set nu", "nonu, wrap, nowrap, et, noet, ts=4, novim"),
            (":mkconfig", "Write the current settings to config.toml"),
            ("/ and ?", "Search the note forwards or backwards"),
            ("n / N", "Next and previous match; :noh clears it"),
            (".", "Repeat the last change, including what was typed"),
        ],
    ),
    (
        "File explorer",
        &[
            ("Enter / l", "Open the note, or fold the folder"),
            ("Space / h", "Fold and unfold a folder"),
            ("H / L", "Collapse / expand every folder"),
            ("/", "Filter by name"),
            ("s", "Change sort order"),
            ("Esc", "Clear the filter"),
        ],
    ),
    (
        "Panes",
        &[
            ("Ctrl+\\", "Toggle the file explorer"),
            ("Ctrl+]", "Toggle the outline sidebar"),
            ("Ctrl+K", "Cycle outline / backlinks / tags"),
            ("Ctrl+T", "Theme picker"),
        ],
    ),
    (
        "Graph",
        &[
            ("Ctrl+G", "Whole-vault graph"),
            ("Ctrl+Shift+G", "Local graph for the open note"),
            ("arrows", "Walk to the nearest node that way"),
            ("hjkl", "Pan"),
            ("+ / -", "Zoom"),
            ("f / 0", "Fit the whole graph on screen"),
            ("Tab / n", "Next node, by link count"),
            ("Shift+Tab / N", "Previous node"),
            ("c", "Centre on the selected node"),
            ("Enter", "Open the selected node"),
            ("drag", "Move a node, then let the layout resettle"),
            ("L", "Toggle labels"),
            ("u", "Toggle unresolved links"),
            ("t", "Toggle tag nodes"),
            ("a", "Toggle attachments"),
            ("r", "Rebuild the layout"),
        ],
    ),
    (
        "Assistant",
        &[
            ("Ctrl+L", "Toggle the chat panel / focus it"),
            ("Enter", "Send"),
            ("/", "Slash command — ↑↓ to browse, Tab or Enter to pick"),
            ("/provider", "Choose a backend: Anthropic, OpenAI, Ollama…"),
            ("/model", "Choose a model, from what the provider offers"),
            ("/key", "Store an API key for this provider"),
            ("Ctrl+C", "Stop the current turn"),
            ("Ctrl+R", "Clear the conversation"),
        ],
    ),
];

/// Whether a prompt is one of vim's bottom-row lines.
#[must_use]
pub fn is_command_line(prompt: &Prompt) -> bool {
    matches!(
        prompt.intent,
        crate::modal::PromptIntent::VimEx | crate::modal::PromptIntent::VimSearch(_)
    )
}

/// Draws `:` or `/` along the status row, with the caret in it.
pub fn draw_command_line(frame: &mut Frame, prompt: &Prompt, palette: &Palette, area: Rect) {
    let text = format!("{}{}", prompt.title, prompt.value);
    Paragraph::new(Line::from(Span::styled(
        text,
        Style::default().fg(palette.text_normal),
    )))
    .style(Style::default().bg(palette.bg_primary))
    .render(area, frame.buffer_mut());

    let column = prompt.title.chars().count() + prompt.cursor;
    frame.set_cursor_position((area.x + u16::try_from(column).unwrap_or(u16::MAX), area.y));
}

/// The leader menu, drawn while `<Space>` is waiting for its second key.
///
/// A remapped keyboard is only safe if the map is on screen. Rather than vim's
/// timeout — which means guessing how long a person needs to think — this
/// appears at once and the next key dismisses it, so it costs nothing to see
/// and nothing to ignore.
pub fn draw_leader(frame: &mut Frame, palette: &Palette, area: Rect) {
    use crate::vim::LEADER;

    // Two columns of bindings, plus a border and a title.
    let rows = LEADER.len().div_ceil(2);
    let height = u16::try_from(rows + 2).unwrap_or(12).min(area.height);
    let width = 52.min(area.width);
    let rect = centered(area, width, height);

    frame.render_widget(Clear, rect);
    let block = pane_block("Leader — Space", true, palette, palette.bg_secondary);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    // Two columns, each a fixed-width key followed by what it does.
    let column = usize::from(inner.width) / 2;
    let label_width = column.saturating_sub(6);
    let lines: Vec<Line> = LEADER
        .chunks(2)
        .map(|pair| {
            let mut spans = Vec::new();
            for (keys, label, _) in pair {
                spans.push(Span::styled(
                    format!(" {keys:<3} "),
                    Style::default()
                        .fg(palette.text_accent)
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(
                    format!("{:<label_width$}", truncate(label, label_width)),
                    Style::default().fg(palette.text_muted),
                ));
            }
            Line::from(spans)
        })
        .collect();

    Paragraph::new(lines).render(inner, frame.buffer_mut());
}

/// The section that only applies once vim mode is switched on.
///
/// Hidden until then, so `?` reads exactly as it always has for the people who
/// never turn it on — a page of keys that don't work is worse than no page.
/// Prefix marking the sections that only apply once vim mode is on.
const VIM_SECTION: &str = "Vim mode";

fn draw_help(frame: &mut Frame, scroll: &mut usize, palette: &Palette, area: Rect, vim: bool) {
    let rect = centered(
        area,
        72.min(area.width),
        area.height.saturating_sub(4).max(10),
    );
    frame.render_widget(Clear, rect);

    let block = pane_block("Keyboard shortcuts", true, palette, palette.bg_secondary);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let mut lines = Vec::new();
    for (section, bindings) in HELP {
        if section.starts_with(VIM_SECTION) && !vim {
            continue;
        }
        lines.push(Line::from(Span::styled(
            (*section).to_string(),
            Style::default()
                .fg(palette.text_accent)
                .add_modifier(Modifier::BOLD),
        )));
        for (key, description) in *bindings {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {key:<16}"),
                    Style::default().fg(palette.text_normal),
                ),
                Span::styled(
                    (*description).to_string(),
                    Style::default().fg(palette.text_muted),
                ),
            ]));
        }
        lines.push(Line::from(""));
    }

    let height = inner.height as usize;
    *scroll = (*scroll).min(lines.len().saturating_sub(height));

    let visible: Vec<Line> = lines.iter().skip(*scroll).take(height).cloned().collect();
    // The scrollbar owns the last column, so the text stops one short of it.
    let text = Rect {
        width: inner.width.saturating_sub(1),
        ..inner
    };
    Paragraph::new(visible).render(text, frame.buffer_mut());
    scrollbar(frame, palette, inner, *scroll, lines.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_covers_every_section_and_has_no_blank_entries() {
        assert!(HELP.len() >= 5);
        for (section, bindings) in HELP {
            assert!(!section.is_empty());
            assert!(!bindings.is_empty(), "{section} has no bindings");
            for (key, description) in *bindings {
                assert!(!key.is_empty() && !description.is_empty());
            }
        }
    }

    #[test]
    fn the_help_names_both_ways_to_close_a_tab() {
        // `F3` works in both modes and `Ctrl+W` only outside vim, so the help
        // has to say which is which — the hint bar has room for one key, and
        // this is where the rest of the truth goes.
        let entry = HELP
            .iter()
            .flat_map(|(_, bindings)| bindings.iter())
            .find(|(_, description)| description.starts_with("Close the tab"))
            .expect("the help documents closing a tab");

        assert!(
            entry.0.contains("F3"),
            "the key that always works comes first"
        );
        assert!(entry.0.contains("Ctrl+W"), "and the familiar one is named");
        assert!(
            entry.1.contains("vim"),
            "with the condition attached: {:?}",
            entry.1
        );
    }

    #[test]
    fn a_long_shortcut_is_not_clipped_by_the_scrollbar() {
        use crate::app::App;
        use crate::config::Config;
        use emeraldian_core::test_support::TempVault;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        // Shortcuts are right-aligned against the list, and the scrollbar is
        // painted down its last column — so every shortcut used to lose its
        // final character once the palette had enough entries to scroll.
        let vault = TempVault::new("palette-clip");
        vault.write("A.md", "a\n");
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        crate::actions::dispatch(&mut app, crate::app::Action::OpenPalette);

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("terminal");
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let screen: String = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            screen.contains("Ctrl+Shift+Tab"),
            "the longest shortcut lost its tail to the scrollbar:\n{screen}"
        );
        assert!(screen.contains("Ctrl+Shift+F"));
    }

    #[test]
    fn byte_offsets_line_up_with_multi_byte_text() {
        assert_eq!(byte_of("héllo", 0), 0);
        assert_eq!(byte_of("héllo", 2), 3, "é is two bytes");
        assert_eq!(byte_of("héllo", 99), 6);
    }

    /// Every key the help table promises, and the pane it belongs to.
    ///
    /// Documented shortcuts that don't work are worse than undocumented ones —
    /// this catches the drift rather than trusting a proofread.
    fn documented_graph_keys() -> Vec<&'static str> {
        HELP.iter()
            .find(|(section, _)| *section == "Graph")
            .map(|(_, bindings)| bindings.iter().map(|(key, _)| *key).collect())
            .unwrap_or_default()
    }

    #[test]
    fn the_help_table_documents_the_graph_keys_that_exist() {
        let keys = documented_graph_keys();
        for expected in ["hjkl", "+ / -", "f / 0", "Tab / n", "c", "L", "u", "t", "r"] {
            assert!(
                keys.contains(&expected),
                "the graph section should list {expected}, has {keys:?}"
            );
        }
        assert!(
            !keys.contains(&"l"),
            "labels are bound to L, not l; a lowercase l pans right"
        );
    }

    #[test]
    fn quitting_and_help_are_documented_where_a_newcomer_looks_first() {
        let navigation = HELP
            .iter()
            .find(|(section, _)| *section == "Navigation")
            .map(|(_, b)| *b)
            .expect("a Navigation section");
        let keys: Vec<&str> = navigation.iter().map(|(key, _)| *key).collect();
        assert!(keys.contains(&"q"), "q quits: {keys:?}");
        assert!(keys.contains(&"?"), "? opens this table: {keys:?}");
    }

    #[test]
    fn the_assistant_section_mentions_slash_commands() {
        let assistant = HELP
            .iter()
            .find(|(section, _)| *section == "Assistant")
            .map(|(_, b)| *b)
            .expect("an Assistant section");
        assert!(
            assistant.iter().any(|(key, _)| *key == "/"),
            "slash commands are only discoverable if they're listed"
        );
    }
}
