//! The agent chat panel.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout as FrameLayout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Widget};

use emeraldian_core::markdown;
use emeraldian_theme::Palette;

use crate::agent::{Entry, ToolStatus};
use crate::app::{App, Focus};
use crate::editor::Layout as TextLayout;
use crate::ui::note::{Ctx, RowStyle, fenced_lines, paint_line, render_document, row_line};
use crate::ui::{pane_block, scrollbar, wrap};

/// Rows of text the input box grows to before it scrolls instead.
///
/// A message longer than this gets a viewport of its own — the editor scrolls
/// to keep the caret visible — rather than pushing the transcript out.
const MAX_INPUT_ROWS: u16 = 8;

/// Tallest the slash-command list gets before it scrolls.
const MAX_COMPLETION_ROWS: u16 = 10;

pub fn draw(frame: &mut Frame, app: &mut App, palette: &Palette, area: Rect) {
    let focused = app.focus == Focus::Chat;
    let title = title(app);
    let block = pane_block(&title, focused, palette, palette.bg_secondary);
    let inner = block.inner(area);

    // The input box is the note editor's sibling: it wraps like a note does and
    // grows to fit what is typed, up to a cap past which it scrolls instead.
    // Text past the pane width therefore lands on the next row — it used to
    // fall off the edge of a single-line paragraph and vanish.
    let text_width = inner.width.saturating_sub(3) as usize;
    let wrap = app.config.editor.wrap;
    let layout = app.chat.input.layout(text_width, wrap);
    let text_rows = (layout.rows().len() as u16).clamp(1, MAX_INPUT_ROWS);
    let context_rows = u16::from(app.chat.context.is_some());
    // The box's own top border row, then the editor's rows, then the footer.
    let input_height = 1 + text_rows + context_rows;

    frame.render_widget(block, area);

    let rows = FrameLayout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(input_height)])
        .split(inner);

    draw_transcript(frame, app, palette, rows[0]);
    draw_input(frame, app, palette, rows[1], focused, &layout, text_width);
    // Drawn last so it sits over the transcript rather than under it.
    if focused {
        draw_completions(frame, app, palette, rows[0]);
    }
}

/// What the panel calls itself.
///
/// Naming the wire is worth the space: "why is nothing happening" is almost
/// always a missing agent binary, and that was invisible before you went looking.
fn title(app: &App) -> String {
    if app.chat.busy {
        return "Assistant  ·  working…".to_string();
    }
    // The agent is a subprocess now; the panel describes the wire, not a provider.
    "Assistant  ·  crow-cli acp".to_string()
}

/// The slash-command list, shown while the user is typing one.
///
/// It grows upward from the input box so the command being typed stays put —
/// the list moving under a fixed cursor is easier to read than the reverse.
fn draw_completions(frame: &mut Frame, app: &App, palette: &Palette, area: Rect) {
    if app.chat.busy || !crate::slash::is_command(&app.chat.input_text()) {
        return;
    }
    let matches = crate::slash::completions(&app.chat.input_text());
    // One exact match with nothing left to choose is not worth a popup.
    if matches.is_empty() || area.height < 2 {
        return;
    }

    let rows = (matches.len() as u16)
        .min(area.height)
        .min(MAX_COMPLETION_ROWS);
    let popup = Rect {
        x: area.x,
        y: area.y + area.height - rows,
        width: area.width,
        height: rows,
    };
    frame.render_widget(Clear, popup);

    // The list is longer than the popup for a bare `/`, so it scrolls to keep
    // the highlight in view rather than letting the arrows walk off the edge.
    let selected = app.chat.completion.min(matches.len() - 1);
    let visible = rows as usize;
    let first = selected
        .saturating_sub(visible - 1)
        .min(matches.len() - visible.min(matches.len()));

    let width = popup.width as usize;
    for (row, command) in matches.iter().skip(first).take(visible).enumerate() {
        let name = match command.argument_hint {
            Some(hint) => format!(" /{} {hint}", command.name),
            None => format!(" /{}", command.name),
        };
        let line = format!("{name:<24}{}", command.description);
        let style = if first + row == selected {
            Style::default()
                .fg(palette.text_accent)
                .bg(palette.bg_active)
        } else {
            Style::default()
                .fg(palette.text_muted)
                .bg(palette.bg_secondary)
        };
        let padded = format!("{line:<width$}");
        frame.buffer_mut().set_string(
            popup.x,
            popup.y + row as u16,
            crate::ui::truncate(&padded, width),
            style,
        );
    }

    if matches.len() > visible {
        let more = format!(" {}/{} ", selected + 1, matches.len());
        let x = popup.x + popup.width.saturating_sub(more.chars().count() as u16 + 1);
        frame.buffer_mut().set_string(
            x,
            popup.y,
            &more,
            Style::default()
                .fg(palette.text_faint)
                .bg(palette.bg_active),
        );
    }
}

fn draw_transcript(frame: &mut Frame, app: &mut App, palette: &Palette, area: Rect) {
    let width = area.width.saturating_sub(1) as usize;
    let lines = transcript_lines(app, palette, width);

    let height = area.height as usize;
    let max_scroll = lines.len().saturating_sub(height);

    // Following means new output stays visible; scrolling up stops it so the
    // user can read without being yanked to the bottom.
    if app.chat.follow {
        app.chat.scroll = max_scroll;
    } else {
        app.chat.scroll = app.chat.scroll.min(max_scroll);
    }

    if lines.is_empty() {
        let hint = if app.chat.settings.allow_writes {
            "Ask about your notes. The assistant can search, read, create and link them."
        } else {
            "Ask about your notes. The assistant can search and read them (writes are off)."
        };
        let hint_lines: Vec<Line> = wrap(hint, width)
            .into_iter()
            .map(|l| Line::from(Span::styled(l, Style::default().fg(palette.text_faint))))
            .collect();
        Paragraph::new(hint_lines).render(area, frame.buffer_mut());
        return;
    }

    let visible: Vec<Line> = lines
        .iter()
        .skip(app.chat.scroll)
        .take(height)
        .cloned()
        .collect();
    Paragraph::new(visible).render(area, frame.buffer_mut());
    scrollbar(frame, palette, area, app.chat.scroll, lines.len());
}

/// Renders the transcript into wrapped, styled lines.
///
/// User and assistant text goes through the same markdown renderer the reading
/// pane uses — one implementation of what markdown looks like, not two — while
/// reasoning, tool calls and errors stay plain: they are status, not prose.
#[must_use]
pub fn transcript_lines(app: &App, palette: &Palette, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for entry in &app.chat.transcript {
        match entry {
            Entry::User(text) => {
                lines.push(Line::from(Span::styled(
                    "You",
                    Style::default()
                        .fg(palette.text_accent)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.extend(rendered(app, palette, text, width));
                lines.push(Line::from(""));
            }

            Entry::Assistant(text) => {
                lines.extend(rendered(app, palette, text, width));
                lines.push(Line::from(""));
            }

            Entry::Reasoning(text) => {
                for line in wrap(text, width.saturating_sub(2)) {
                    lines.push(Line::from(Span::styled(
                        format!("  {line}"),
                        Style::default()
                            .fg(palette.text_faint)
                            .add_modifier(Modifier::ITALIC),
                    )));
                }
            }

            Entry::Tool {
                name,
                detail,
                status,
            } => {
                // Tool calls are shown so the user can see what the agent
                // actually did to their vault — not just what it says it did.
                let (glyph, color) = match status {
                    ToolStatus::Running => ("◌", palette.text_muted),
                    ToolStatus::Ok => ("✓", palette.text_success),
                    ToolStatus::Failed => ("✗", palette.text_error),
                };
                let text = if detail.is_empty() {
                    name.clone()
                } else {
                    format!("{name}  {detail}")
                };
                for (i, line) in wrap(&text, width.saturating_sub(2)).into_iter().enumerate() {
                    lines.push(Line::from(vec![
                        Span::styled(
                            if i == 0 {
                                format!("{glyph} ")
                            } else {
                                "  ".into()
                            },
                            Style::default().fg(color),
                        ),
                        Span::styled(line, Style::default().fg(palette.text_muted)),
                    ]));
                }
            }

            Entry::Context(text) => {
                lines.push(Line::from(Span::styled(
                    format!("⎘ {text}"),
                    Style::default().fg(palette.text_faint),
                )));
            }

            Entry::Error(text) => {
                for line in wrap(text, width) {
                    lines.push(Line::from(Span::styled(
                        line,
                        Style::default().fg(palette.text_error),
                    )));
                }
                lines.push(Line::from(""));
            }
        }
    }

    lines
}

/// One entry's text through the reading pane's renderer.
///
/// No note dir and no image support: the transcript is prose from the wire, so
/// a picture it mentions renders as its alt text rather than reaching into the
/// vault for a file that was never part of the conversation.
fn rendered(app: &App, palette: &Palette, text: &str, width: usize) -> Vec<Line<'static>> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let document = markdown::parse(text);
    let mut ctx = Ctx {
        palette,
        index: &app.index,
        note_dir: None,
        images: None,
        pictures: Vec::new(),
        anchors: Vec::new(),
    };
    render_document(&document, &mut ctx, width)
}

fn draw_input(
    frame: &mut Frame,
    app: &mut App,
    palette: &Palette,
    area: Rect,
    focused: bool,
    layout: &TextLayout,
    text_width: usize,
) {
    let block = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::TOP)
        .border_style(Style::default().fg(palette.border));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // The context footer keeps the bottom row; the editor gets what is above.
    let mut text_rows = inner.height;
    if inner.height > 1
        && let Some(context) = &app.chat.context
    {
        frame.buffer_mut().set_string(
            inner.x,
            inner.y + inner.height - 1,
            crate::ui::truncate(context, inner.width as usize),
            Style::default().fg(palette.text_faint),
        );
        text_rows -= 1;
    }
    let text_rect = Rect {
        x: inner.x + 2,
        y: inner.y,
        width: inner.width.saturating_sub(3),
        height: text_rows,
    };

    // Recorded for key handling: arrows move by these rows, and the caret
    // column spare keeps one past the last character reachable.
    app.chat.input_cols = text_width;
    app.chat.input_rows = text_rect.height as usize;

    let prompt = if app.chat.busy { "…" } else { ">" };
    frame.buffer_mut().set_string(
        inner.x,
        text_rect.y,
        prompt,
        Style::default().fg(palette.text_accent),
    );

    if app.chat.input_is_empty() && !focused {
        Paragraph::new(Line::from(Span::styled(
            "Ctrl+L to focus",
            Style::default().fg(palette.text_faint),
        )))
        .render(text_rect, frame.buffer_mut());
        return;
    }

    app.chat
        .input
        .scroll_into_view(layout, text_rect.height as usize);
    let scroll = app.chat.input.scroll;
    let hscroll = app.chat.input.hscroll;
    let (caret_row, caret_column) = app.chat.input.caret(layout);
    let cursor_line = app.chat.input.cursor().line;
    let selection = app.chat.input.selection();
    let fenced = fenced_lines(app.chat.input.lines());

    // Painting a whole source line at once and slicing it per row keeps one
    // decision — what each character is — in one place, however it wrapped.
    let mut painted: Option<(usize, Vec<(char, Style)>)> = None;
    let mut body: Vec<Line> = Vec::new();
    for row in layout
        .rows()
        .iter()
        .skip(scroll)
        .take(text_rect.height as usize)
    {
        if painted.as_ref().is_none_or(|(line, _)| *line != row.line) {
            let source = app.chat.input.lines()[row.line].clone();
            painted = Some((
                row.line,
                paint_line(&source, row.line == cursor_line, fenced[row.line], palette),
            ));
        }
        let Some((_, chars)) = &painted else { continue };
        body.push(row_line(
            row,
            &chars[row.start.min(chars.len())..row.end.min(chars.len())],
            RowStyle {
                background: palette.bg_secondary,
                selection: palette.bg_selection,
                search: palette.bg_selection,
            },
            selection,
            &[],
            text_rect.width as usize + hscroll,
        ));
    }
    Paragraph::new(body)
        .scroll((0, u16::try_from(hscroll).unwrap_or(u16::MAX)))
        .render(text_rect, frame.buffer_mut());

    // Place the terminal cursor so the user sees a real caret.
    if focused && !app.chat.busy {
        let column = usize::from(caret_column);
        if caret_row >= scroll
            && caret_row - scroll < text_rect.height as usize
            && column >= hscroll
        {
            let x = text_rect.x + u16::try_from(column - hscroll).unwrap_or(u16::MAX);
            frame.set_cursor_position((x, text_rect.y + (caret_row - scroll) as u16));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use emeraldian_core::test_support::TempVault;
    use emeraldian_theme::presets;

    fn app() -> (TempVault, App) {
        let vault = TempVault::new("ui-chat");
        vault.write("A.md", "# A\n");
        let app = App::new(vault.vault(), Config::default()).expect("app");
        (vault, app)
    }

    fn palette() -> Palette {
        Palette::from(&presets::default_theme())
    }

    fn text_of(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn user_and_assistant_turns_are_labeled_and_wrapped() {
        let (_v, mut app) = app();
        app.chat.transcript.push(Entry::User("a question".into()));
        app.chat.transcript.push(Entry::Assistant(
            "a fairly long answer that needs wrapping".into(),
        ));

        let lines = text_of(&transcript_lines(&app, &palette(), 20));

        assert_eq!(lines[0], "You");
        assert!(lines.iter().any(|l| l.contains("a question")));
        for line in &lines {
            assert!(line.chars().count() <= 20, "{line:?} overflows");
        }
    }

    #[test]
    fn tool_calls_show_their_status() {
        let (_v, mut app) = app();
        app.chat.transcript.push(Entry::Tool {
            name: "create_note".into(),
            detail: "created Ideas.md".into(),
            status: ToolStatus::Ok,
        });
        app.chat.transcript.push(Entry::Tool {
            name: "read_note".into(),
            detail: "no such note".into(),
            status: ToolStatus::Failed,
        });

        let lines = text_of(&transcript_lines(&app, &palette(), 40));
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with('✓') && l.contains("create_note"))
        );
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with('✗') && l.contains("read_note"))
        );
    }

    #[test]
    fn errors_render_in_the_error_color() {
        let (_v, mut app) = app();
        app.chat.transcript.push(Entry::Error("no API key".into()));

        let lines = transcript_lines(&app, &palette(), 40);
        assert_eq!(lines[0].spans[0].style.fg, Some(palette().text_error));
    }

    #[test]
    fn an_empty_transcript_renders_nothing() {
        let (_v, app) = app();
        assert!(transcript_lines(&app, &palette(), 40).is_empty());
    }

    /// The rows of the completion popup, top to bottom, as plain text.
    fn popup_rows(app: &App, height: u16) -> Vec<String> {
        let area = Rect::new(0, 0, 60, height);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(area.width, height))
                .expect("terminal");
        terminal
            .draw(|frame| draw_completions(frame, app, &palette(), area))
            .expect("drawn");

        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .filter(|row| !row.is_empty())
            .collect()
    }

    #[test]
    fn the_panel_says_what_is_answering_and_when_it_is_working() {
        let (_vault, mut app) = app();

        // The agent is a subprocess now; the title names the wire, not a provider.
        assert!(title(&app).contains("crow-cli acp"), "{}", title(&app));

        app.chat.busy = true;
        assert!(
            title(&app).contains("working"),
            "while a turn is running, that is the more useful thing to say"
        );
    }

    #[test]
    fn the_command_list_scrolls_to_keep_the_highlight_in_view() {
        let (_v, mut app) = app();
        app.chat.set_input("/");
        let total = crate::slash::completions("/").len();
        assert!(
            total > MAX_COMPLETION_ROWS as usize,
            "this test only means something if the list is too long to show at once"
        );

        let first_page = popup_rows(&app, 20);
        assert_eq!(first_page.len(), MAX_COMPLETION_ROWS as usize);
        assert!(
            first_page[0].contains("/help"),
            "starts at the top: {first_page:?}"
        );
        assert!(first_page[0].contains("1/"), "and counts: {first_page:?}");

        // Down to the very last command.
        app.chat.completion = total - 1;
        let last_page = popup_rows(&app, 20);
        assert!(
            last_page.last().is_some_and(|row| row.contains("/quit")),
            "the last command is reachable rather than off the bottom: {last_page:?}"
        );
        assert!(last_page[0].contains(&format!("{total}/{total}")));
    }

    #[test]
    fn the_highlight_is_the_row_the_arrows_landed_on() {
        let (_v, mut app) = app();
        app.chat.set_input("/");
        app.chat.completion = 2;

        let area = Rect::new(0, 0, 60, 20);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 20)).expect("terminal");
        terminal
            .draw(|frame| draw_completions(frame, &app, &palette(), area))
            .expect("drawn");

        let buffer = terminal.backend().buffer().clone();
        let popup_top = 20 - MAX_COMPLETION_ROWS;
        let accent = palette().text_accent;
        let highlighted: Vec<u16> = (popup_top..20)
            .filter(|&y| buffer[(1, y)].style().fg == Some(accent))
            .collect();
        assert_eq!(
            highlighted,
            vec![popup_top + 2],
            "exactly the third row, and only it"
        );
    }

    #[test]
    fn a_pane_with_no_room_draws_no_popup_rather_than_panicking() {
        let (_v, mut app) = app();
        app.chat.set_input("/");
        assert!(popup_rows(&app, 1).is_empty());
    }

    #[test]
    fn assistant_paragraphs_render_with_their_breaks() {
        let (_v, mut app) = app();
        app.chat
            .transcript
            .push(Entry::Assistant("first\n\nsecond".into()));

        let lines = text_of(&transcript_lines(&app, &palette(), 40));
        assert_eq!(lines[0], "first");
        assert_eq!(lines[1], "");
        assert_eq!(lines[2], "second");
    }

    #[test]
    fn assistant_text_renders_as_markdown() {
        let (_v, mut app) = app();
        app.chat
            .transcript
            .push(Entry::Assistant("plain **bold** text".into()));

        let lines = transcript_lines(&app, &palette(), 40);
        assert!(
            lines
                .iter()
                .any(|l| l.spans.iter().any(|s| {
                    s.content == "bold" && s.style.add_modifier.contains(Modifier::BOLD)
                })),
            "the emphasis run is drawn bold, the way the reading pane draws it"
        );
    }

    #[test]
    fn typing_past_the_pane_width_wraps_onto_the_next_row() {
        let (_v, mut app) = app();
        app.focus = Focus::Chat;
        // Longer than one row of a 40-column pane: the old single-line input
        // clipped everything past the edge, so it looked like the text was
        // never typed at all.
        let message = "z".repeat(80);
        app.chat.set_input(&message);

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 20)).expect("terminal");
        terminal
            .draw(|frame| draw(frame, &mut app, &palette(), Rect::new(0, 0, 40, 20)))
            .expect("drawn");

        let buffer = terminal.backend().buffer().clone();
        let mut count = 0;
        let mut rows: Vec<u16> = Vec::new();
        for y in 0..20u16 {
            for x in 0..40u16 {
                if buffer[(x, y)].symbol() == "z" {
                    count += 1;
                    if !rows.contains(&y) {
                        rows.push(y);
                    }
                }
            }
        }
        assert!(count > 40, "more than one row's worth is visible: {count}");
        assert!(rows.len() >= 2, "wrapped onto a second row: {rows:?}");
    }

    #[test]
    fn the_input_box_grows_to_fit_and_then_caps() {
        let (_v, mut app) = app();
        app.focus = Focus::Chat;

        let rows_with_text = |app: &mut App| {
            let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 30))
                .expect("terminal");
            terminal
                .draw(|frame| draw(frame, app, &palette(), Rect::new(0, 0, 60, 30)))
                .expect("drawn");
            let buffer = terminal.backend().buffer().clone();
            (0..30u16)
                .filter(|y| (0..60u16).any(|x| buffer[(x, *y)].symbol() != " "))
                .filter(|y| {
                    (0..60u16)
                        .map(|x| buffer[(x, *y)].symbol())
                        .collect::<String>()
                        .contains("zork")
                })
                .count()
        };

        app.chat.set_input("one zork");
        assert_eq!(rows_with_text(&mut app), 1, "a short message, one row");

        let mut text = String::new();
        for i in 1..=12 {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&format!("zork {i}"));
        }
        app.chat.set_input(&text);
        assert_eq!(
            rows_with_text(&mut app),
            MAX_INPUT_ROWS as usize,
            "long messages cap rather than pushing the transcript out"
        );
    }
}
