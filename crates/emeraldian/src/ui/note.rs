//! The note pane: tab bar, reading mode and the editor.
//!
//! Reading mode renders the markdown model into styled lines the way Obsidian's
//! preview does — headings colored by level, wikilinks in the accent color and
//! unresolved ones dimmed, callouts with a tinted bar, code blocks on their own
//! background. Editing mode shows the source with a line-number gutter.

use std::path::{Path, PathBuf};

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use ratatui_image::sliced::{SignedPosition, SlicedImage};
use unicode_width::UnicodeWidthStr;

use emeraldian_core::excalidraw;
use emeraldian_core::index::VaultIndex;
use emeraldian_core::markdown::{self, Align, Block, BlockKind, Marker, SpanKind, Table};
use emeraldian_theme::{Hues, Palette, STROKE_CONTRAST, TEXT_CONTRAST, readable};

use crate::app::{App, Mode, Regions};
use crate::editor::{Cursor, Row};
use crate::images::{self, Images};
use crate::ui::{drawing, icons, math, mermaid, scrollbar, truncate};

/// Left padding inside the note pane, matching Obsidian's generous margins.
const PADDING: u16 = 2;

/// Columns reserved for the line-number gutter when it is switched on.
///
/// Fixed rather than sized to the note, and reserved while *reading* too, where
/// it is left blank. Both modes then wrap the same prose to the same width in
/// the same column, which is what makes `Ctrl+E` restyle the page instead of
/// reflowing it. A note past 999 lines spends the separating space on a fourth
/// digit rather than moving the text.
const GUTTER: u16 = 4;

/// Widest a single table column may be drawn, in characters.
///
/// Tables are laid out at their content's width and panned across, so this is
/// only here to stop one cell of prose from pushing every column after it out
/// of reach.
const MAX_COLUMN: usize = 40;

/// Everything the markdown renderer needs beyond the blocks themselves.
///
/// Bundled because pictures made the parameter list unwieldy: laying one out
/// needs the vault to find the file, the note's own folder to resolve a
/// relative path against, and the terminal's font size to know how many rows
/// it will fill.
pub struct Ctx<'a> {
    pub palette: &'a Palette,
    pub index: &'a VaultIndex,
    /// The folder of the note being read, where a relative `![](chart.png)` is
    /// looked for first.
    pub note_dir: Option<PathBuf>,
    /// `None` renders every picture as its alt text, which is what happens in
    /// a terminal that cannot draw and in tests.
    pub images: Option<&'a mut Images>,
    /// Where each picture ended up, filled in as blocks are laid out.
    pub pictures: Vec<Picture>,
    /// Which rendered row each block's source line ended up on, filled in as
    /// blocks are laid out.
    ///
    /// A source line and a rendered row are not the same number — prose wraps,
    /// headings gain a rule, a picture takes a dozen rows — so scrolling to a
    /// heading the outline points at needs the translation written down.
    pub anchors: Vec<(usize, usize)>,
    /// The blocks laid out wider than the pane, filled in as blocks are laid
    /// out.
    pub pans: Vec<Pan>,
    /// The colours diagrams are drawn in, worked out the first time the note
    /// has one rather than once for each.
    inks: Option<DiagramInks>,
}

impl<'a> Ctx<'a> {
    /// A context that renders text only.
    #[cfg(test)]
    #[must_use]
    pub fn text(palette: &'a Palette, index: &'a VaultIndex) -> Self {
        Self {
            palette,
            index,
            note_dir: None,
            images: None,
            pictures: Vec::new(),
            anchors: Vec::new(),
            pans: Vec::new(),
            inks: None,
        }
    }
}

/// The colours diagrams are drawn in: the theme's hues for the things in
/// them, and quieter colours for what is no one's, each held to the contrast
/// against the page that lines or words need.
struct DiagramInks {
    hues: Hues,
    /// Lines that are no one's, like the rule of a `loop` block.
    quiet: Color,
    /// Words that are no one's.
    quiet_words: Color,
    /// Notes, and the words on a `loop` or `alt` block's rule.
    note: Color,
}

impl DiagramInks {
    fn new(palette: &Palette) -> Self {
        let page = palette.bg_primary;
        Self {
            hues: palette.hues(),
            quiet: readable(palette.text_muted, page, STROKE_CONTRAST),
            quiet_words: readable(palette.text_muted, page, TEXT_CONTRAST),
            note: readable(palette.callout_note, page, TEXT_CONTRAST),
        }
    }
}

/// A picture and the rows reserved for it.
#[derive(Debug, Clone)]
pub struct Picture {
    /// Index of its first row in the rendered lines.
    pub line: usize,
    /// Columns from the left edge of the pane.
    pub indent: u16,
    pub size: Size,
    pub path: PathBuf,
}

/// A block laid out wider than the pane: a table, a code block, a diagram.
///
/// These are the only things in a note that can't wrap, so they are the only
/// things that pan. The prose around them stays where it is, rather than
/// sliding off the left edge to show the far side of one table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pan {
    /// Index of its first row in the rendered lines.
    pub line: usize,
    pub rows: usize,
    /// Columns on the left that stay put: the indent, and the bars of any
    /// quote or callout it sits in.
    pub keep: usize,
    /// Its widest row, in columns, `keep` included.
    pub width: usize,
}

impl Pan {
    fn contains(&self, row: usize) -> bool {
        (self.line..self.line + self.rows).contains(&row)
    }
}

/// The note last laid out for reading, kept until something it was laid out
/// from changes.
///
/// Every frame used to read the note from disk, parse it and lay all of it out,
/// to show one screen of it. On a long note that was most of the work of each
/// keypress, and most of the memory the app touched: paging through a note of a
/// few megabytes took it past half a gigabyte. A frame that changes nothing the
/// layout depends on now reuses it.
#[derive(Default)]
pub struct ReadingCache {
    key: Option<ReadingKey>,
    lines: Vec<Line<'static>>,
    anchors: Vec<(usize, usize)>,
    pictures: Vec<Picture>,
    pans: Vec<Pan>,
}

/// Everything a reading layout depends on.
#[derive(PartialEq)]
struct ReadingKey {
    note: usize,
    /// When the file was last written, and its length: a cheap stand-in for
    /// its content that needs no reading.
    modified: Option<std::time::SystemTime>,
    len: u64,
    /// Which links resolve, and which attachments exist.
    index: u64,
    /// What each picture measures.
    images: u64,
    width: usize,
    height: u16,
    palette: Palette,
}

impl ReadingCache {
    /// Forgets the layout, for when something outside its key has changed.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

pub fn draw(
    frame: &mut Frame,
    app: &mut App,
    palette: &Palette,
    area: Rect,
    regions: &mut Regions,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);

    draw_tab_bar(frame, app, palette, rows[0], regions);

    let body = Rect {
        x: rows[1].x + PADDING,
        y: rows[1].y,
        width: rows[1].width.saturating_sub(PADDING * 2),
        height: rows[1].height,
    };

    frame.render_widget(
        ratatui::widgets::Block::default().style(Style::default().bg(palette.bg_primary)),
        rows[1],
    );

    if app.active().is_none() {
        draw_empty_state(frame, palette, body);
        return;
    }

    // Reserved in both modes so the prose column never moves between them.
    let gutter = if app.config.ui.line_numbers {
        GUTTER.min(body.width)
    } else {
        0
    };

    match app.active().map(|t| t.mode) {
        Some(Mode::Reading) => draw_reading(frame, app, palette, body, gutter, regions),
        Some(Mode::Editing) => draw_editing(frame, app, palette, body, gutter, regions),
        None => {}
    }
}

/// The prose column inside the note pane: the gutter and the scrollbar's column
/// taken off.
fn prose_area(area: Rect, gutter: u16) -> Rect {
    Rect {
        x: area.x + gutter,
        y: area.y,
        width: area.width.saturating_sub(gutter),
        height: area.height,
    }
}

/// Width and height the editor's text was last drawn at.
///
/// Key handling needs it to move between wrapped rows, and reads it from the
/// last frame rather than recomputing the layout arithmetic a second time.
#[must_use]
pub fn edit_viewport(app: &App) -> (usize, usize) {
    app.regions.editor.map_or((0, 0), |(rect, _)| {
        (rect.width as usize, rect.height as usize)
    })
}

fn draw_tab_bar(
    frame: &mut Frame,
    app: &App,
    palette: &Palette,
    area: Rect,
    regions: &mut Regions,
) {
    frame.render_widget(
        ratatui::widgets::Block::default().style(Style::default().bg(palette.tab_bar_bg)),
        area,
    );
    if app.tabs.is_empty() {
        return;
    }

    // The side panes light their borders up when they have the keyboard. The
    // note pane has no border, so its active tab carries the same colour.
    let focused = app.focus == crate::app::Focus::Note;
    let mut spans = Vec::new();
    let mut x = area.x;
    for (index, tab) in app.tabs.iter().enumerate() {
        if x >= area.right() {
            // Past the edge: nothing of it is drawn, so nothing can be clicked.
            break;
        }
        let active = app.active_tab == Some(index);
        let title = app.note_title(tab.note);
        let title = truncate(&title, 22);
        let title_width = title.width() as u16;

        // `" {title}"`, then the close button with a space either side. A
        // tab with unsaved changes shows a dot there instead, as Obsidian and
        // VS Code do, and the dot closes it all the same.
        let width = title_width + 4;
        let clip = |rect: Rect| rect.intersection(area);
        regions.tabs.push((
            clip(Rect {
                x,
                y: area.y,
                width,
                height: 1,
            }),
            index,
        ));
        // The glyph and the space after it: two columns are an easier target
        // than one, and the space before it still belongs to the title.
        regions.tab_close.push((
            clip(Rect {
                x: x + 1 + title_width + 1,
                y: area.y,
                width: 2,
                height: 1,
            }),
            index,
        ));
        x += width;

        let style = if active {
            Style::default()
                .bg(palette.tab_active_bg)
                .fg(palette.tab_active_fg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .bg(palette.tab_bar_bg)
                .fg(palette.tab_inactive_fg)
        };

        if active && focused {
            spans.push(Span::styled("▎", style.fg(palette.border_focus)));
            spans.push(Span::styled(title.clone(), style.fg(palette.text_accent)));
        } else {
            spans.push(Span::styled(format!(" {title}"), style));
        }
        let (glyph, colour) = if tab.is_modified() {
            (icons::MODIFIED, palette.tab_modified)
        } else {
            (icons::CLOSE, palette.tab_inactive_fg)
        };
        spans.push(Span::styled(
            format!(" {glyph} "),
            style.fg(colour).remove_modifier(Modifier::BOLD),
        ));
    }

    Paragraph::new(Line::from(spans)).render(area, frame.buffer_mut());
}

fn draw_empty_state(frame: &mut Frame, palette: &Palette, area: Rect) {
    let hints = [
        ("Ctrl+O", "open a note"),
        ("Ctrl+P", "command palette"),
        ("Ctrl+N", "new note"),
        ("Ctrl+G", "graph view"),
        ("Ctrl+L", "assistant"),
        ("?", "all shortcuts"),
    ];

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "emeraldian",
            Style::default()
                .fg(palette.text_accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    for (key, description) in hints {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {key:<10}"),
                Style::default().fg(palette.text_accent),
            ),
            Span::styled(description, Style::default().fg(palette.text_muted)),
        ]));
    }

    Paragraph::new(lines).render(area, frame.buffer_mut());
}

// ---------------------------------------------------------------------------
// Reading mode
// ---------------------------------------------------------------------------

fn draw_reading(
    frame: &mut Frame,
    app: &mut App,
    palette: &Palette,
    area: Rect,
    gutter: u16,
    regions: &mut Regions,
) {
    let Some(id) = app.active_note() else { return };
    // Taken whether or not this frame can use it, so it can't go off later on
    // some other note.
    let reveal = app.reveal.take();
    let Some(path) = app.index.note(id).map(|note| note.meta.path.clone()) else {
        return;
    };
    let unreadable = |frame: &mut Frame| {
        Paragraph::new("could not read this note").render(area, frame.buffer_mut());
    };
    // The scrollbar stays on the pane's edge; the prose sits inside the gutter.
    let full = area;
    let area = prose_area(area, gutter);

    // An Excalidraw note is a wrapper around a scene: its Markdown is a warning
    // banner and a wall of compressed base64, and the drawing is the content.
    // Judged by the file name, so an ordinary note that happens to have a
    // `## Drawing` heading is still read as prose.
    let is_drawing = app
        .index
        .note(id)
        .is_some_and(|note| excalidraw::is_drawing(&note.meta.rel));
    if is_drawing {
        let Ok(content) = app.index.read(id) else {
            return unreadable(frame);
        };
        let scroll = app.active().map_or(0, |tab| tab.scroll);
        if let Some(scene) = app.scenes.get(&content) {
            let rows = drawing::draw(frame, palette, area, scene, scroll);
            let max_scroll = rows.saturating_sub(area.height as usize);
            if let Some(tab) = app.active_mut() {
                tab.scroll = tab.scroll.min(max_scroll);
            }
            return;
        }
    }
    let file = std::fs::metadata(&path).ok();

    let width = area.width.saturating_sub(1) as usize;
    // Pictures are capped at a share of the pane, so the layout pass has to
    // know how tall it is before it measures the first one.
    app.images.set_pane_height(area.height);

    let key = ReadingKey {
        note: id,
        modified: file.as_ref().and_then(|file| file.modified().ok()),
        len: file.as_ref().map_or(0, std::fs::Metadata::len),
        index: app.index.revision(),
        images: app.images.generation(),
        width,
        height: area.height,
        palette: palette.clone(),
    };
    // A file that can't be looked at can't be trusted to be unchanged either.
    if file.is_none() || app.reading.key.as_ref() != Some(&key) {
        let Ok(content) = app.index.read(id) else {
            app.reading.clear();
            return unreadable(frame);
        };
        let document = markdown::parse(&content);
        let note_dir = app
            .index
            .note(id)
            .map(|note| app.index.vault.path.join(&note.meta.rel))
            .and_then(|path| path.parent().map(Path::to_path_buf));
        let mut ctx = Ctx {
            palette,
            index: &app.index,
            note_dir,
            images: Some(&mut app.images),
            pictures: Vec::new(),
            anchors: Vec::new(),
            pans: Vec::new(),
            inks: None,
        };
        let lines = render_document(&document, &mut ctx, width);
        app.reading = ReadingCache {
            key: Some(key),
            lines,
            anchors: std::mem::take(&mut ctx.anchors),
            pictures: std::mem::take(&mut ctx.pictures),
            pans: std::mem::take(&mut ctx.pans),
        };
    }
    regions.anchors.clone_from(&app.reading.anchors);

    let total = app.reading.lines.len();
    let height = area.height as usize;
    if let Some(reveal) = reveal {
        let start = anchor_row(&regions.anchors, reveal.line);
        let row = row_showing(
            &app.reading.lines,
            &regions.anchors,
            start,
            &reveal.text,
            reveal.nth,
        )
        .unwrap_or(start);
        if let Some(tab) = app.active_mut() {
            // In the upper part of the pane, with a little of what leads up to
            // it above.
            tab.scroll = row.saturating_sub(height / 3);
        }
    }
    let max_scroll = total.saturating_sub(height);
    let scroll = app.active().map_or(0, |t| t.scroll).min(max_scroll);
    // Panning is held to what the wide blocks on screen can use, so `h` always
    // moves something you can see. Scrolled away from them, it settles back to
    // the left edge, where the next wide block is met from its start.
    // The last column is the scrollbar's, which is why the layout was one
    // narrower than the pane too.
    let room = pan_room(&app.reading.pans, scroll, height, width);
    if let Some(tab) = app.active_mut() {
        tab.scroll = scroll;
        tab.hscroll = tab.hscroll.min(room);
    }
    let hscroll = app.active().map_or(0, |t| t.hscroll);
    regions.pan = room;

    let visible = visible_lines(
        &app.reading.lines,
        &app.reading.pans,
        scroll,
        height,
        hscroll,
        width,
        palette,
    );
    Paragraph::new(visible).render(area, frame.buffer_mut());
    draw_pictures(
        frame,
        &mut app.images,
        palette,
        area,
        &app.reading.pictures,
        scroll,
    );
    scrollbar(frame, palette, full, scroll, total);
}

/// How far the wide blocks showing in rows `scroll..scroll + height` can pan
/// before the widest of them shows its last column.
fn pan_room(pans: &[Pan], scroll: usize, height: usize, width: usize) -> usize {
    pans.iter()
        .filter(|pan| pan.line < scroll + height && pan.line + pan.rows > scroll)
        .map(|pan| pan.width.saturating_sub(width))
        .max()
        .unwrap_or(0)
}

/// The rows on screen, each wide block panned `hscroll` columns or as far as
/// it goes, and everything else as it was laid out.
///
/// Only these rows are touched, and only sliced: a diagram is drawn once when
/// the note is laid out, never again for panning.
fn visible_lines(
    lines: &[Line<'static>],
    pans: &[Pan],
    scroll: usize,
    height: usize,
    hscroll: usize,
    width: usize,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let marker = Style::default().fg(palette.text_faint);
    // The blocks are in row order, so one walk forward finds each row's.
    let mut pans = pans
        .iter()
        .skip_while(|pan| pan.line + pan.rows <= scroll)
        .peekable();
    lines
        .iter()
        .enumerate()
        .skip(scroll)
        .take(height)
        .map(|(row, line)| {
            while pans.next_if(|pan| pan.line + pan.rows <= row).is_some() {}
            match pans.peek() {
                Some(pan) if pan.contains(row) && pan.width > width => {
                    let offset = hscroll.min(pan.width - width);
                    pan_line(line, pan.keep, offset, width, marker)
                }
                _ => line.clone(),
            }
        })
        .collect()
}

/// `line` as it shows `width` columns wide with its first `keep` columns held
/// in place and the rest moved `offset` columns to the left.
///
/// A character two columns wide that the edge would cut in half is shown as
/// blank cells instead, so nothing after it moves out of line. Where there is
/// more to either side, the edge cell says so with `‹` or `›`, the way `less`
/// and vim mark a line that runs off the screen.
fn pan_line(
    line: &Line<'static>,
    keep: usize,
    offset: usize,
    width: usize,
    marker: Style,
) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }
    let keep = keep.min(width);
    let start = keep + offset;
    let end = keep + offset + (width - keep);
    // What lies past each edge, counting only what can be seen: a code
    // block's background running on is not more to read.
    let mut before = false;
    let mut after = false;
    let mut column = 0;
    for grapheme in line.styled_graphemes(Style::default()) {
        let wide = grapheme.symbol.width();
        if !grapheme.symbol.trim().is_empty() {
            before |= column < start && column + wide > keep;
            after |= column + wide > end;
        }
        column += wide;
    }
    // The marker takes the edge cell, so the window it frames is one narrower.
    let left = start + usize::from(before);
    let right = (end - usize::from(after)).max(left);

    let mut kept = Vec::new();
    let mut shown = Vec::new();
    let mut lead = None;
    let mut trail = None;
    let mut column = 0;
    for grapheme in line.styled_graphemes(Style::default()) {
        let wide = grapheme.symbol.width();
        let (from, to) = (column, column + wide);
        column = to;
        if from < keep {
            let cells = to.min(keep) - from;
            put(
                &mut kept,
                grapheme.symbol,
                cells == wide,
                cells,
                grapheme.style,
            );
            continue;
        }
        // The cells the markers stand in, whose background they keep.
        if (from..to).contains(&start) {
            lead = Some(grapheme.style);
        }
        if (from..to).contains(&(end - 1)) {
            trail = Some(grapheme.style);
        }
        let cells = to.min(right).saturating_sub(from.max(left));
        if cells > 0 || (wide == 0 && (left..right).contains(&from)) {
            put(
                &mut shown,
                grapheme.symbol,
                cells == wide,
                cells,
                grapheme.style,
            );
        }
    }

    let mut spans = kept;
    if before {
        let style = lead.unwrap_or_default().patch(marker);
        spans.push(Span::styled("‹", style));
    }
    spans.extend(shown);
    if after {
        let style = trail.unwrap_or_default().patch(marker);
        spans.push(Span::styled("›", style));
    }
    Line::from(spans)
}

/// Adds a grapheme to `spans`, whole if it fits, or as `cells` blank cells
/// where an edge cuts it.
fn put(spans: &mut Vec<Span<'static>>, symbol: &str, whole: bool, cells: usize, style: Style) {
    let blank;
    let text = if whole {
        symbol
    } else {
        blank = " ".repeat(cells);
        blank.as_str()
    };
    match spans.last_mut() {
        Some(last) if last.style == style => last.content.to_mut().push_str(text),
        _ => spans.push(Span::styled(text.to_string(), style)),
    }
}

/// Draws the pictures whose rows are on screen, over the blanks left for them.
fn draw_pictures(
    frame: &mut Frame,
    images: &mut Images,
    palette: &Palette,
    area: Rect,
    pictures: &[Picture],
    scroll: usize,
) {
    for picture in pictures {
        // Where the top of the picture sits relative to the pane, which is
        // above it — a negative row — once it is partly scrolled off.
        let Ok(top) = i32::try_from(picture.line).map(|line| line - scroll as i32) else {
            continue;
        };
        if top >= i32::from(area.height) || top + i32::from(picture.size.height) <= 0 {
            continue;
        }
        let Ok(top) = i16::try_from(top) else {
            continue;
        };
        // Pictures sit in the prose, which never pans, so they stay put
        // while a wide table beside them moves.
        if picture.indent >= area.width {
            continue;
        }
        let Ok(left) = i16::try_from(picture.indent) else {
            continue;
        };

        match images.get(&picture.path, picture.size) {
            Some(protocol) => {
                let position = SignedPosition::from((left, top));
                frame.render_widget(SlicedImage::new(protocol, position), area);
            }
            // Still encoding. The rows are already the right size, so the
            // picture appears without moving anything around it.
            None => {
                let row = area.y as i32 + i32::from(top).max(0);
                let column = area.x as i32 + i32::from(left).max(0);
                if let (Ok(y), Ok(x)) = (u16::try_from(row), u16::try_from(column)) {
                    let placeholder = Rect {
                        x: x.min(area.x + area.width),
                        y,
                        width: picture.size.width.min(area.width),
                        height: 1,
                    };
                    Paragraph::new(Line::from(Span::styled(
                        format!("{} …", icons::IMAGE),
                        Style::default().fg(palette.text_faint),
                    )))
                    .render(placeholder.intersection(area), frame.buffer_mut());
                }
            }
        }
    }
}

/// Turns a parsed document into styled terminal lines.
///
/// Pictures are laid out here too, as runs of blank lines with their positions
/// collected in `ctx.pictures`, so that scrolling and the scrollbar count them
/// like any other content.
pub fn render_document(
    document: &markdown::Document,
    ctx: &mut Ctx,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for block in &document.blocks {
        ctx.anchors.push((block.line, lines.len()));
        render_block(block, ctx, width, 0, &mut lines);
    }
    lines
}

/// The rendered row a source line was drawn on.
///
/// Only block starts are recorded, so a line inside a block resolves to the row
/// its block began on — which is what scrolling to a heading wants anyway.
#[must_use]
pub fn anchor_row(anchors: &[(usize, usize)], line: usize) -> usize {
    anchors
        .iter()
        .take_while(|(source, _)| *source <= line)
        .last()
        .map_or(line, |(_, row)| *row)
}

/// The row, from `start` to the end of its block, showing the `nth` time
/// `text` occurs there, counting from 0 and ignoring case. A row and the one
/// after it are read together, so text wrapped onto the next row is found as
/// well. Short of `nth`, the last one found.
fn row_showing(
    lines: &[Line],
    anchors: &[(usize, usize)],
    start: usize,
    text: &str,
    nth: usize,
) -> Option<usize> {
    let needle = text.to_lowercase();
    if needle.trim().is_empty() {
        return None;
    }
    let end = anchors
        .iter()
        .map(|(_, row)| *row)
        .find(|row| *row > start)
        .unwrap_or(lines.len())
        .min(lines.len());
    let row_text = |row: usize| -> String {
        lines.get(row).map_or_else(String::new, |line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .to_lowercase()
        })
    };
    let count = |text: &str| text.matches(needle.as_str()).count();
    let mut seen = 0;
    let mut last = None;
    for row in start..end {
        let here = row_text(row);
        let next = row_text(row + 1);
        let joined = format!("{} {}", here.trim_end(), next.trim_start());
        // What this row shows, and what only shows across its end.
        let found = count(&here) + count(&joined).saturating_sub(count(&here) + count(&next));
        if found > 0 {
            last = Some(row);
            if seen + found > nth {
                return last;
            }
            seen += found;
        }
    }
    last
}

fn render_block(
    block: &Block,
    ctx: &mut Ctx,
    width: usize,
    indent: usize,
    out: &mut Vec<Line<'static>>,
) {
    let palette = ctx.palette;
    let pad = " ".repeat(indent);
    let inner_width = width.saturating_sub(indent).max(8);

    match &block.kind {
        BlockKind::Blank => out.push(Line::from("")),

        BlockKind::Frontmatter(entries) => {
            for (key, value) in entries {
                let entry = [
                    (
                        format!("{key}: "),
                        Style::default().fg(palette.frontmatter_key),
                    ),
                    (
                        value.clone(),
                        Style::default().fg(palette.frontmatter_value),
                    ),
                ];
                out.extend(wrap_spans(&entry, width, &pad, &format!("{pad}  ")));
            }
            out.push(Line::from(Span::styled(
                "─".repeat(width.min(60)),
                Style::default().fg(palette.hr),
            )));
            out.push(Line::from(""));
        }

        BlockKind::Heading { level, spans } => {
            let style = Style::default()
                .fg(palette.heading(*level))
                .add_modifier(Modifier::BOLD);
            // Wrapped like a paragraph is. A heading can be longer than the
            // pane, and one pasted in from elsewhere can carry a whole page of
            // text and maths on its single line.
            let start = out.len();
            render_styled_spans(spans, ctx, inner_width, indent, &pad, &pad, style, out);
            // Obsidian underlines H1 and H2; a rule is the terminal equivalent
            // of the larger type it uses to separate sections.
            if *level <= 2 {
                let widest = out[start..]
                    .iter()
                    .map(Line::width)
                    .max()
                    .unwrap_or(0)
                    .saturating_sub(indent);
                out.push(Line::from(Span::styled(
                    format!("{pad}{}", "─".repeat(inner_width.min(widest + 8))),
                    Style::default().fg(palette.border),
                )));
            }
        }

        BlockKind::Paragraph(spans) => {
            render_spans(spans, ctx, inner_width, indent, &pad, &pad, out);
        }

        BlockKind::ListItem {
            depth,
            marker,
            spans,
        } => {
            let list_indent = " ".repeat(indent + depth * 2);
            let (glyph, glyph_style) = match marker {
                Marker::Bullet => (
                    icons::BULLET.to_string(),
                    Style::default().fg(palette.list_marker),
                ),
                Marker::Ordered(n) => (format!("{n}."), Style::default().fg(palette.list_marker)),
                Marker::Task(true) => (
                    icons::TASK_DONE.to_string(),
                    Style::default().fg(palette.checkbox_done),
                ),
                Marker::Task(false) => (
                    icons::TASK_TODO.to_string(),
                    Style::default().fg(palette.checkbox_todo),
                ),
            };

            let prefix_width = glyph.chars().count() + 1;
            let continuation = format!("{list_indent}{}", " ".repeat(prefix_width));
            let text_indent = indent + depth * 2 + prefix_width;

            let mark = ctx.pictures.len();
            let mut wrapped = Vec::new();
            render_spans(
                spans,
                ctx,
                inner_width.saturating_sub(depth * 2 + prefix_width),
                text_indent,
                "",
                "",
                &mut wrapped,
            );
            // Completed tasks are struck through, as in Obsidian.
            if matches!(marker, Marker::Task(true)) {
                for line in &mut wrapped {
                    for span in &mut line.spans {
                        span.style = span
                            .style
                            .add_modifier(Modifier::CROSSED_OUT)
                            .fg(palette.text_faint);
                    }
                }
            }
            shift(&mut ctx.pictures[mark..], out.len(), 0);

            if wrapped.is_empty() {
                wrapped.push(Line::from(""));
            }
            for (i, line) in wrapped.into_iter().enumerate() {
                let mut spans = Vec::new();
                if i == 0 {
                    spans.push(Span::raw(list_indent.clone()));
                    spans.push(Span::styled(format!("{glyph} "), glyph_style));
                } else {
                    spans.push(Span::raw(continuation.clone()));
                }
                spans.extend(line.spans);
                out.push(Line::from(spans));
            }
        }

        BlockKind::Quote(body) => {
            let mark = ctx.pictures.len();
            let pans = ctx.pans.len();
            let mut inner = Vec::new();
            for child in body {
                render_block(child, ctx, width.saturating_sub(2), 0, &mut inner);
            }
            // Every inner line gains a quote bar and lands after what is
            // already in `out`, so anything laid out inside moves with it.
            shift(&mut ctx.pictures[mark..], out.len(), indent as u16 + 2);
            shift_pans(&mut ctx.pans[pans..], out.len(), indent + 2);
            for line in inner {
                let mut spans = vec![
                    Span::raw(pad.clone()),
                    Span::styled(
                        format!("{} ", icons::QUOTE_BAR),
                        Style::default().fg(palette.quote_bar),
                    ),
                ];
                spans.extend(line.spans.into_iter().map(|mut span| {
                    span.style = span.style.fg(palette.quote_fg);
                    span
                }));
                out.push(Line::from(spans));
            }
        }

        BlockKind::Callout { kind, title, body } => {
            let color = palette.callout(kind);
            let fallback = [markdown::Span::plain(capitalize(kind))];
            let title: &[markdown::Span] = if title.is_empty() { &fallback } else { title };

            // The title wraps under its bar, like the body below it.
            let mark = ctx.pictures.len();
            let mut heading = Vec::new();
            let style = Style::default().fg(color).add_modifier(Modifier::BOLD);
            render_styled_spans(
                title,
                ctx,
                width.saturating_sub(2),
                0,
                "",
                "",
                style,
                &mut heading,
            );
            shift(&mut ctx.pictures[mark..], out.len(), indent as u16 + 2);
            for line in heading {
                let mut spans = vec![
                    Span::raw(pad.clone()),
                    Span::styled(format!("{} ", icons::QUOTE_BAR), Style::default().fg(color)),
                ];
                spans.extend(line.spans);
                out.push(Line::from(spans));
            }

            let mark = ctx.pictures.len();
            let pans = ctx.pans.len();
            let mut inner = Vec::new();
            for child in body {
                render_block(child, ctx, width.saturating_sub(2), 0, &mut inner);
            }
            shift(&mut ctx.pictures[mark..], out.len(), indent as u16 + 2);
            shift_pans(&mut ctx.pans[pans..], out.len(), indent + 2);
            for line in inner {
                let mut spans = vec![
                    Span::raw(pad.clone()),
                    Span::styled(format!("{} ", icons::QUOTE_BAR), Style::default().fg(color)),
                ];
                spans.extend(line.spans);
                out.push(Line::from(spans));
            }
        }

        BlockKind::Code { lang, lines: code } => {
            // A Mermaid block is drawn as its diagram when it can be, and
            // shown as code otherwise.
            let start = out.len();
            if lang.eq_ignore_ascii_case("mermaid")
                && let Some(diagram) = mermaid::render(code, inner_width)
            {
                let inks = ctx.inks.get_or_insert_with(|| DiagramInks::new(palette));
                diagram_lines(&diagram, palette, inks, &pad, out);
                note_pan(ctx, out, start, indent, width);
                return;
            }
            let background = Style::default().bg(palette.code_bg);
            if !lang.is_empty() {
                out.push(Line::from(Span::styled(
                    format!("{pad} {lang} "),
                    background.fg(palette.text_faint),
                )));
            }
            let body = out.len();
            for line in code {
                let mut spans = vec![Span::styled(format!("{pad} "), background)];
                spans.extend(highlight(line, lang, palette, background));
                out.push(Line::from(spans));
            }
            // Pad to the pane width so the block reads as a filled panel, or
            // to its longest line, so the panel stays filled when panned.
            let fill = out[body..]
                .iter()
                .map(Line::width)
                .max()
                .unwrap_or(0)
                .max(width);
            for line in &mut out[body..] {
                let used = line.width();
                if used < fill {
                    line.spans
                        .push(Span::styled(" ".repeat(fill - used), background));
                }
            }
            note_pan(ctx, out, start, indent, width);
        }

        BlockKind::Math(tex) => {
            let start = out.len();
            display_math(tex, palette, inner_width, &pad, out);
            note_pan(ctx, out, start, indent, width);
        }

        BlockKind::Table(table) => {
            let start = out.len();
            render_table(table, palette, &pad, out);
            note_pan(ctx, out, start, indent, width);
        }

        BlockKind::Rule => out.push(Line::from(Span::styled(
            format!("{pad}{}", "─".repeat(inner_width)),
            Style::default().fg(palette.hr),
        ))),
    }
}

/// A drawn diagram as styled lines. It keeps its own width: one wider than the
/// pane is panned across like a wide table, never squeezed.
///
/// Each node or participant is drawn in one of the theme's hues, and what it
/// sends in the same one, while the words inside boxes keep the body colour.
/// This runs when the note is laid out and the colours are kept with the
/// lines, so a frame costs what it always did.
fn diagram_lines(
    diagram: &mermaid::Diagram,
    palette: &Palette,
    inks: &DiagramInks,
    pad: &str,
    out: &mut Vec<Line<'static>>,
) {
    use mermaid::Role;

    let picked = diagram.colours(inks.hues.strokes.len());
    let style = |ink: mermaid::Ink| {
        let hue = ink
            .owner
            .and_then(|owner| picked.get(usize::from(owner)))
            .map(|&hue| usize::from(hue));
        Style::default().fg(match (ink.role, hue) {
            (Role::Text, _) => palette.text_normal,
            (Role::Note, _) => inks.note,
            (Role::Label, Some(hue)) => inks.hues.words[hue],
            (Role::Label, None) => inks.quiet_words,
            (Role::Border | Role::Line | Role::Arrow, Some(hue)) => inks.hues.strokes[hue],
            (Role::Border | Role::Line | Role::Arrow, None) => inks.quiet,
        })
    };
    // Runs that come out the same colour, like an edge and its arrowhead,
    // share a span. This holds where each span's runs end, and its style.
    let mut spans_of: Vec<(usize, Style)> = Vec::new();
    for row in &diagram.rows {
        spans_of.clear();
        for (at, (_, ink)) in row.iter().enumerate() {
            let style = style(*ink);
            match spans_of.last_mut() {
                Some((end, last)) if *last == style => *end = at + 1,
                _ => spans_of.push((at + 1, style)),
            }
        }
        let mut spans = Vec::with_capacity(spans_of.len() + usize::from(!pad.is_empty()));
        if !pad.is_empty() {
            spans.push(Span::raw(pad.to_string()));
        }
        let mut start = 0;
        for &(end, style) in &spans_of {
            let runs = &row[start..end];
            let mut text = String::with_capacity(runs.iter().map(|(text, _)| text.len()).sum());
            for (run, _) in runs {
                text.push_str(run);
            }
            spans.push(Span::styled(text, style));
            start = end;
        }
        out.push(Line::from(spans));
    }
}

/// Lays out inline spans, lifting any picture out onto rows of its own.
///
/// A picture is a block in a line of text: the words around it wrap as usual
/// above and below, and it gets however many rows it needs in between. When it
/// can't be drawn — no support in the terminal, a missing file, a link to the
/// web — nothing is lifted out and it stays inline as its alt text.
#[allow(clippy::too_many_arguments)]
fn render_spans(
    spans: &[markdown::Span],
    ctx: &mut Ctx,
    width: usize,
    indent: usize,
    first_pad: &str,
    rest_pad: &str,
    out: &mut Vec<Line<'static>>,
) {
    let base = Style::default().fg(ctx.palette.text_normal);
    render_styled_spans(spans, ctx, width, indent, first_pad, rest_pad, base, out);
}

/// [`render_spans`] for text whose plain parts take `base` rather than the
/// body colour, such as a heading or a callout title. Links, tags and maths
/// inside it are still coloured as they are anywhere else.
#[allow(clippy::too_many_arguments)]
fn render_styled_spans(
    spans: &[markdown::Span],
    ctx: &mut Ctx,
    width: usize,
    indent: usize,
    first_pad: &str,
    rest_pad: &str,
    base: Style,
    out: &mut Vec<Line<'static>>,
) {
    let mut run: Vec<markdown::Span> = Vec::new();
    // Text resuming after something lifted out starts a line of its own, and
    // shouldn't start it with the space that separated them.
    let mut lifted = false;
    for span in spans {
        let picture = match span.kind {
            // `$$…$$` written mid-paragraph is lifted out the same way, since
            // Obsidian sets it apart from the text around it.
            SpanKind::Math { display: true } => None,
            _ => match measure_picture(span, ctx, width) {
                Some(picture) => Some(picture),
                None => {
                    let mut span = span.clone();
                    if lifted && run.is_empty() {
                        span.text = span.text.trim_start().to_string();
                    }
                    if !span.text.is_empty() {
                        run.push(span);
                    }
                    continue;
                }
            },
        };
        lifted = true;

        if !run.is_empty() {
            let pad = if out.is_empty() { first_pad } else { rest_pad };
            let styled = style_spans(&run, ctx.palette, ctx.index, base);
            out.extend(wrap_spans(&styled, width, pad, rest_pad));
            run.clear();
        }

        let Some((path, size)) = picture else {
            let pad = if out.is_empty() { first_pad } else { rest_pad };
            display_math(&span.text, ctx.palette, width, pad, out);
            continue;
        };
        ctx.pictures.push(Picture {
            line: out.len(),
            indent: indent as u16,
            size,
            path,
        });
        // The picture is drawn over these rows once it has been encoded. They
        // are blank so that the text below sits where it will finally sit, and
        // so a half-scrolled picture leaves no debris behind.
        out.extend(std::iter::repeat_n(Line::from(""), size.height as usize));
    }

    if !run.is_empty() || out.is_empty() {
        let pad = if out.is_empty() { first_pad } else { rest_pad };
        let styled = style_spans(&run, ctx.palette, ctx.index, base);
        out.extend(wrap_spans(&styled, width, pad, rest_pad));
    }
}

/// Works out where a span's picture is and how much room it needs.
fn measure_picture(span: &markdown::Span, ctx: &mut Ctx, width: usize) -> Option<(PathBuf, Size)> {
    let (target, embed) = match &span.kind {
        SpanKind::Image { src } => (src, false),
        // `![[x]]` embeds a note as often as a picture, and only the vault
        // knows which; a note embed finds no image file and stays a link.
        SpanKind::WikiLink {
            target,
            embed: true,
            ..
        } => (target, true),
        _ => return None,
    };

    let path = ctx
        .index
        .attachment_path(target, ctx.note_dir.as_deref())
        .filter(|path| images::is_image(path))?;

    let images = ctx.images.as_deref_mut()?;
    let mut room = Size::new(u16::try_from(width).unwrap_or(u16::MAX), u16::MAX);
    // Obsidian sizes an embed with `![[chart.png|400]]`, in pixels. Anything
    // else after the pipe is an alias, which is only ever alt text.
    if let Some(pixels) = embed.then(|| width_hint(&span.text)).flatten() {
        room.width = room.width.min(images.cells_wide(pixels)?);
    }

    let size = images.measure(&path, room)?;
    (size.width > 0 && size.height > 0).then_some((path, size))
}

/// Reads Obsidian's `|400` or `|400x300` embed size, in pixels.
fn width_hint(alias: &str) -> Option<u32> {
    let width = alias.split_once('x').map_or(alias, |(width, _)| width);
    width.trim().parse().ok().filter(|&pixels| pixels > 0)
}

/// Records the rows from `start` as a block that pans, if any of them is wider
/// than the `width` it was laid out for.
fn note_pan(ctx: &mut Ctx, out: &[Line], start: usize, keep: usize, width: usize) {
    let widest = out[start..].iter().map(Line::width).max().unwrap_or(0);
    if widest > width {
        ctx.pans.push(Pan {
            line: start,
            rows: out.len() - start,
            keep,
            width: widest,
        });
    }
}

/// Moves wide blocks laid out in a nested buffer to where that buffer landed,
/// behind `keep` more columns of bars that stay put.
fn shift_pans(pans: &mut [Pan], lines: usize, keep: usize) {
    for pan in pans {
        pan.line += lines;
        pan.keep += keep;
        pan.width += keep;
    }
}

/// Moves pictures laid out in a nested buffer to where that buffer landed.
fn shift(pictures: &mut [Picture], lines: usize, indent: u16) {
    for picture in pictures {
        picture.line += lines;
        picture.indent += indent;
    }
}

/// Applies theme colors to parsed inline spans, starting from `base`.
fn style_spans(
    spans: &[markdown::Span],
    palette: &Palette,
    index: &emeraldian_core::index::VaultIndex,
    base: Style,
) -> Vec<(String, Style)> {
    spans
        .iter()
        .map(|span| {
            let mut style = base;

            match &span.kind {
                SpanKind::WikiLink { target, .. } => {
                    // A link to a note that doesn't exist yet is dimmed rather
                    // than hidden — that's the signal to go write it.
                    style = if index.resolve(target).is_some() {
                        style.fg(palette.link).add_modifier(Modifier::UNDERLINED)
                    } else {
                        style.fg(palette.link_unresolved)
                    };
                }
                SpanKind::Link { .. } => {
                    style = style
                        .fg(palette.link_external)
                        .add_modifier(Modifier::UNDERLINED);
                }
                // Reached only when the picture could not be drawn, so this is
                // the alt text standing in for it.
                SpanKind::Image { .. } => style = style.fg(palette.text_faint),
                SpanKind::Tag(_) => {
                    style = style.fg(palette.tag_fg).bg(palette.tag_bg);
                }
                SpanKind::Math { .. } => {
                    style = style.fg(palette.text_accent);
                    return (
                        math::inline(&span.text),
                        emphasis(style, span.style, palette),
                    );
                }
                SpanKind::Text => {}
            }

            (span.text.clone(), emphasis(style, span.style, palette))
        })
        .collect()
}

/// Inline spans as the reading pane shows them, for the places that draw them
/// as a single run: headings, callout titles and table cells.
fn shown_text(spans: &[markdown::Span]) -> String {
    spans
        .iter()
        .map(|span| match span.kind {
            SpanKind::Math { .. } => math::inline(&span.text),
            _ => span.text.clone(),
        })
        .collect()
}

/// Sets an equation on rows of its own, centred as Obsidian centres it.
///
/// One wider than the pane starts at the margin instead and is panned across,
/// like a wide table. `width` counts the `pad` in front of each row, as it does
/// for [`wrap_spans`].
fn display_math(
    tex: &str,
    palette: &Palette,
    width: usize,
    pad: &str,
    out: &mut Vec<Line<'static>>,
) {
    let set = math::display(tex);
    let style = Style::default().fg(palette.text_accent);
    let room = width.saturating_sub(pad.width());
    let tag = set.tag.as_deref().map(|tag| (tag, tag.width()));
    // A tag sits at the right margin, so the equation is centred clear of it.
    let reserve = tag.map_or(0, |(_, tag_width)| 2 * (tag_width + 2));
    let left = if set.width + reserve <= room {
        (room - set.width) / 2
    } else {
        0
    };

    for (at, row) in set.rows.iter().enumerate() {
        let row = row.trim_end();
        let mut spans = vec![
            Span::raw(format!("{pad}{}", " ".repeat(left))),
            Span::styled(row.to_string(), style),
        ];
        if at == set.baseline
            && let Some((tag, tag_width)) = tag
        {
            let used = left + row.width();
            let gap = room.saturating_sub(used + tag_width).max(2);
            spans.push(Span::raw(" ".repeat(gap)));
            spans.push(Span::styled(
                tag.to_string(),
                Style::default().fg(palette.text_muted),
            ));
        }
        out.push(Line::from(spans));
    }
}

/// Wraps styled spans to `width`, breaking on spaces and keeping styles intact.
///
/// Widths are display columns, so wide characters wrap where they are drawn.
/// A word wider than a whole line, such as a long URL, is broken at the edge
/// of the pane rather than left running off it.
#[must_use]
pub fn wrap_spans(
    spans: &[(String, Style)],
    width: usize,
    first_indent: &str,
    indent: &str,
) -> Vec<Line<'static>> {
    if width == 0 {
        return Vec::new();
    }

    let indent_width = indent.width();
    let mut lines: Vec<Line> = Vec::new();
    let mut current: Vec<Span> = vec![Span::raw(first_indent.to_string())];
    let mut used = first_indent.width();
    let mut break_line = |current: &mut Vec<Span<'static>>, used: &mut usize| {
        lines.push(Line::from(std::mem::take(current)));
        current.push(Span::raw(indent.to_string()));
        *used = indent_width;
    };

    for (text, style) in spans {
        // Splitting inclusively keeps the space attached to the preceding word,
        // so styles don't fragment on every gap.
        for word in text.split_inclusive(' ') {
            let mut word = word;
            if used + word.width() > width && used > indent_width {
                break_line(&mut current, &mut used);
                // A wrapped line shouldn't start with the space that ended the
                // previous one.
                word = word.trim_start();
                if word.is_empty() {
                    continue;
                }
            }
            // Checked for emptiness as well as width: with an indent as wide as
            // the pane, even the last character of a word is too wide.
            while !word.trim_end().is_empty() && used + word.trim_end().width() > width {
                let (head, rest) = split_at_column(word, width.saturating_sub(used));
                current.push(Span::styled(head.to_string(), *style));
                break_line(&mut current, &mut used);
                word = rest;
            }
            if !word.is_empty() {
                current.push(Span::styled(word.to_string(), *style));
                used += word.width();
            }
        }
    }

    lines.push(Line::from(current));
    lines
}

/// Splits `text` after as many characters as fit in `columns`, and after one
/// at the least, so that breaking a word always makes progress.
fn split_at_column(text: &str, columns: usize) -> (&str, &str) {
    use unicode_width::UnicodeWidthChar;

    let mut used = 0;
    for (at, ch) in text.char_indices() {
        let width = ch.width().unwrap_or(0);
        if at > 0 && used + width > columns {
            return text.split_at(at);
        }
        used += width;
    }
    (text, "")
}

fn render_table(table: &Table, palette: &Palette, pad: &str, out: &mut Vec<Line<'static>>) {
    let columns = table
        .header
        .len()
        .max(table.rows.iter().map(Vec::len).max().unwrap_or(0));
    if columns == 0 {
        return;
    }

    // Size columns to their content.
    //
    // A table wider than the pane is left wide and scrolled across
    // horizontally, rather than squeezed to fit: dividing the pane between
    // eight columns leaves three characters each, which is not a narrow table
    // but an unreadable one. The only limit is a ceiling on a single runaway
    // column, so one essay-length cell can't push the rest off the far side.
    let mut widths = vec![0usize; columns];
    let cell_text = |cells: &Vec<Vec<markdown::Span>>, i: usize| -> String {
        cells
            .get(i)
            .map(|spans| shown_text(spans))
            .unwrap_or_default()
    };

    // Measured in columns rather than characters: typeset maths puts its
    // accents on as combining marks, which take no room of their own.
    for (i, width) in widths.iter_mut().enumerate() {
        *width = (*width).max(cell_text(&table.header, i).width());
        for row in &table.rows {
            *width = (*width).max(cell_text(row, i).width());
        }
        *width = (*width).clamp(1, MAX_COLUMN);
    }

    let border = Style::default().fg(palette.table_border);
    let rule = |left: &str, mid: &str, right: &str| -> Line<'static> {
        let mut text = String::from(left);
        for (i, w) in widths.iter().enumerate() {
            text.push_str(&"─".repeat(w + 2));
            text.push_str(if i + 1 == widths.len() { right } else { mid });
        }
        Line::from(vec![Span::raw(pad.to_string()), Span::styled(text, border)])
    };

    let render_row = |cells: &Vec<Vec<markdown::Span>>, header: bool| -> Line<'static> {
        let mut spans = vec![Span::raw(pad.to_string()), Span::styled("│", border)];
        for (i, w) in widths.iter().enumerate() {
            let text = truncate(&cell_text(cells, i), *w);
            let padding = w.saturating_sub(text.width());
            let (before, after) = match table.aligns.get(i).copied().unwrap_or(Align::Left) {
                Align::Left => (0, padding),
                Align::Right => (padding, 0),
                Align::Center => (padding / 2, padding - padding / 2),
            };
            let style = if header {
                Style::default()
                    .fg(palette.table_header)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette.text_normal)
            };
            spans.push(Span::raw(format!(" {}", " ".repeat(before))));
            spans.push(Span::styled(text, style));
            spans.push(Span::raw(format!("{} ", " ".repeat(after))));
            spans.push(Span::styled("│", border));
        }
        Line::from(spans)
    };

    out.push(rule("┌", "┬", "┐"));
    out.push(render_row(&table.header, true));
    out.push(rule("├", "┼", "┤"));
    for row in &table.rows {
        out.push(render_row(row, false));
    }
    out.push(rule("└", "┴", "┘"));
}

/// Lightweight syntax highlighting for fenced code.
///
/// Keyword, string, comment and number recognition covers what makes code
/// readable at a glance. A full grammar per language would be a large
/// dependency for a feature used a few lines at a time.
fn highlight(line: &str, lang: &str, palette: &Palette, base: Style) -> Vec<Span<'static>> {
    let keywords = keywords_for(lang);
    let comment = comment_prefix(lang);

    if let Some(prefix) = comment
        && line.trim_start().starts_with(prefix)
    {
        return vec![Span::styled(line.to_string(), base.fg(palette.syn_comment))];
    }

    let mut spans = Vec::new();
    let mut current = String::new();
    let mut in_string: Option<char> = None;

    macro_rules! flush {
        ($style:expr) => {
            if !current.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut current), $style));
            }
        };
    }

    for ch in line.chars() {
        if let Some(quote) = in_string {
            current.push(ch);
            if ch == quote {
                flush!(base.fg(palette.syn_string));
                in_string = None;
            }
            continue;
        }

        if ch == '"' || ch == '\'' || ch == '`' {
            flush!(base.fg(palette.text_normal));
            current.push(ch);
            in_string = Some(ch);
            continue;
        }

        if ch.is_alphanumeric() || ch == '_' {
            current.push(ch);
            continue;
        }

        // A word boundary: classify what we collected.
        if !current.is_empty() {
            let style = if keywords.contains(&current.as_str()) {
                base.fg(palette.syn_keyword).add_modifier(Modifier::BOLD)
            } else if current.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                base.fg(palette.syn_number)
            } else if ch == '(' {
                base.fg(palette.syn_function)
            } else if current.chars().next().is_some_and(char::is_uppercase) {
                base.fg(palette.syn_type)
            } else {
                base.fg(palette.text_normal)
            };
            flush!(style);
        }
        spans.push(Span::styled(ch.to_string(), base.fg(palette.syn_punct)));
    }

    if !current.is_empty() {
        let style = if in_string.is_some() {
            base.fg(palette.syn_string)
        } else if keywords.contains(&current.as_str()) {
            base.fg(palette.syn_keyword).add_modifier(Modifier::BOLD)
        } else if current.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            base.fg(palette.syn_number)
        } else {
            base.fg(palette.text_normal)
        };
        spans.push(Span::styled(current, style));
    }

    spans
}

fn keywords_for(lang: &str) -> &'static [&'static str] {
    const RUST: &[&str] = &[
        "fn", "let", "mut", "pub", "use", "mod", "struct", "enum", "impl", "trait", "for", "while",
        "loop", "if", "else", "match", "return", "self", "Self", "const", "static", "async",
        "await", "move", "where", "type", "as", "in", "ref", "dyn", "crate", "true", "false",
    ];
    const PYTHON: &[&str] = &[
        "def", "class", "import", "from", "return", "if", "elif", "else", "for", "while", "try",
        "except", "finally", "with", "as", "lambda", "yield", "async", "await", "pass", "raise",
        "in", "not", "and", "or", "None", "True", "False", "self",
    ];
    const JS: &[&str] = &[
        "function",
        "const",
        "let",
        "var",
        "return",
        "if",
        "else",
        "for",
        "while",
        "class",
        "extends",
        "import",
        "export",
        "from",
        "async",
        "await",
        "new",
        "this",
        "try",
        "catch",
        "finally",
        "throw",
        "typeof",
        "interface",
        "type",
        "true",
        "false",
        "null",
        "undefined",
    ];
    const SHELL: &[&str] = &[
        "if", "then", "else", "fi", "for", "while", "do", "done", "case", "esac", "function",
        "export", "local", "return", "echo", "cd", "set",
    ];
    const GO: &[&str] = &[
        "func",
        "package",
        "import",
        "var",
        "const",
        "type",
        "struct",
        "interface",
        "return",
        "if",
        "else",
        "for",
        "range",
        "go",
        "defer",
        "chan",
        "select",
        "switch",
        "case",
        "map",
        "nil",
        "true",
        "false",
    ];

    match lang.to_lowercase().as_str() {
        "rust" | "rs" => RUST,
        "python" | "py" => PYTHON,
        "javascript" | "js" | "typescript" | "ts" | "tsx" | "jsx" => JS,
        "bash" | "sh" | "shell" | "zsh" => SHELL,
        "go" => GO,
        _ => &[],
    }
}

fn comment_prefix(lang: &str) -> Option<&'static str> {
    match lang.to_lowercase().as_str() {
        "rust" | "rs" | "javascript" | "js" | "typescript" | "ts" | "go" | "c" | "cpp" | "java" => {
            Some("//")
        }
        "python" | "py" | "bash" | "sh" | "shell" | "zsh" | "yaml" | "yml" | "toml" | "ruby"
        | "rb" => Some("#"),
        "sql" | "lua" | "haskell" => Some("--"),
        _ => None,
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Editing mode
// ---------------------------------------------------------------------------

fn draw_editing(
    frame: &mut Frame,
    app: &mut App,
    palette: &Palette,
    area: Rect,
    gutter: u16,
    regions: &mut Regions,
) {
    let wrap = app.config.editor.wrap;
    // A caret in an unfocused pane claims input that would go somewhere else.
    let focused = app.focus == crate::app::Focus::Note;
    // Vim's Normal-mode cursor covers a character rather than sitting between
    // two, so on an empty or just-ended line the block would otherwise be drawn
    // one column past the text it is supposed to be on.
    let on_character = app.config.editor.vim && app.vim.mode.is_normal_like();
    // Highlighting only the match jumped to would hide the rest, which is most
    // of what a search is for. Read before the editor is borrowed below.
    let search = app
        .config
        .editor
        .vim
        .then(|| app.vim.search.as_ref().map(|s| s.pattern.clone()))
        .flatten();
    let prose = prose_area(area, gutter);
    // One column is left for the scrollbar, which also gives the caret a place
    // to sit at the end of a full row.
    let text = Rect {
        width: prose.width.saturating_sub(1),
        ..prose
    };
    let height = text.height as usize;

    let Some(editor) = app.editor_mut() else {
        return;
    };
    if on_character {
        editor.clamp_normal();
    }
    let layout = editor.layout(text.width as usize, wrap);
    editor.scroll_into_view(&layout, height);

    let scroll = editor.scroll;
    let hscroll = editor.hscroll;
    let (caret_row, caret_column) = editor.caret(&layout);
    let selection = editor.selection();
    let cursor_line = editor.cursor().line;
    let fenced = fenced_lines(editor.lines());

    // Painting a whole source line at once and slicing it per row keeps one
    // decision — what each character is — in one place, however it wrapped.
    let mut painted: Option<(usize, Vec<(char, Style)>)> = None;
    let mut numbers: Vec<Line> = Vec::new();
    let mut body: Vec<Line> = Vec::new();

    for row in layout.rows().iter().skip(scroll).take(height) {
        let on_cursor_line = row.line == cursor_line;
        numbers.push(gutter_line(row, on_cursor_line, gutter, palette));

        if painted.as_ref().is_none_or(|(line, _)| *line != row.line) {
            let source = &editor.lines()[row.line];
            painted = Some((
                row.line,
                paint_line(source, on_cursor_line, fenced[row.line], palette),
            ));
        }
        let Some((_, chars)) = &painted else { continue };

        let hits = match &search {
            Some(pattern) => editor.matches_on(row.line, pattern),
            None => Vec::new(),
        };
        body.push(row_line(
            row,
            &chars[row.start.min(chars.len())..row.end.min(chars.len())],
            RowStyle {
                background: if fenced[row.line] == Fence::Code {
                    palette.code_bg
                } else if on_cursor_line {
                    palette.cursor_line_bg
                } else {
                    palette.bg_primary
                },
                selection: palette.bg_selection,
                search: palette.bg_selection,
            },
            selection,
            &hits,
            text.width as usize + hscroll,
        ));
    }

    if gutter > 0 {
        Paragraph::new(numbers).render(
            Rect {
                width: gutter,
                ..area
            },
            frame.buffer_mut(),
        );
    }
    Paragraph::new(body)
        .scroll((0, u16::try_from(hscroll).unwrap_or(u16::MAX)))
        .render(text, frame.buffer_mut());
    scrollbar(frame, palette, area, scroll, layout.rows().len());

    regions.editor = Some((text, scroll));

    // Place the terminal cursor so the user sees a real caret.
    let column = usize::from(caret_column);
    if focused && caret_row >= scroll && caret_row - scroll < height && column >= hscroll {
        let x = text.x + u16::try_from(column - hscroll).unwrap_or(u16::MAX);
        if x < prose.x + prose.width {
            frame.set_cursor_position((x, text.y + (caret_row - scroll) as u16));
        }
    }
}

/// Colours for the row a line is drawn on, behind whatever it says.
struct RowStyle {
    background: ratatui::style::Color,
    selection: ratatui::style::Color,
    /// Behind a search hit. Same colour as a selection, since both mean
    /// "this is the text you asked about".
    search: ratatui::style::Color,
}

/// The gutter cell for one row: a line number, or the wrap marker on a row that
/// is the continuation of the one above.
///
/// The marker lives here rather than in the text so that distinguishing a soft
/// wrap from a real line break costs no reading width at all.
fn gutter_line(row: &Row, on_cursor_line: bool, gutter: u16, palette: &Palette) -> Line<'static> {
    if gutter == 0 {
        return Line::from("");
    }
    let width = usize::from(gutter) - 1;
    let (text, color) = if row.first {
        (
            format!("{:>width$} ", row.line + 1),
            if on_cursor_line {
                palette.line_number_active
            } else {
                palette.line_number
            },
        )
    } else {
        (format!("{:>width$} ", icons::WRAP), palette.text_faint)
    };
    Line::from(Span::styled(text, Style::default().fg(color)))
}

/// Builds one terminal row from the painted characters it shows.
///
/// The row is padded to the full width so a code block's background and the
/// cursor line's highlight reach the edge of the pane rather than stopping at
/// the end of the text.
fn row_line(
    row: &Row,
    chars: &[(char, Style)],
    colors: RowStyle,
    selection: Option<(Cursor, Cursor)>,
    hits: &[(usize, usize)],
    width: usize,
) -> Line<'static> {
    let selected = |col: usize| {
        selection.is_some_and(|(start, end)| {
            let at = (row.line, col);
            at >= (start.line, start.col) && at < (end.line, end.col)
        })
    };
    let matched = |col: usize| hits.iter().any(|(start, end)| col >= *start && col < *end);
    // A background already on the character — inline code — outranks the row's,
    // a search hit outranks that, and a selection outranks everything: the
    // more deliberate the act, the louder it is drawn.
    let resolve = |style: Style, col: usize| {
        if selected(col) {
            return style.bg(colors.selection);
        }
        if matched(col) {
            return style.bg(colors.search);
        }
        match style.bg {
            Some(_) => style,
            None => style.bg(colors.background),
        }
    };

    let mut spans = vec![Span::styled(
        " ".repeat(usize::from(row.indent)),
        Style::default().bg(colors.background),
    )];
    let mut run = String::new();
    let mut current: Option<Style> = None;

    for (offset, (ch, style)) in chars.iter().enumerate() {
        let style = resolve(*style, row.start + offset);
        if current.is_some_and(|open| open != style) {
            spans.push(Span::styled(std::mem::take(&mut run), current.unwrap()));
        }
        current = Some(style);
        run.push(*ch);
    }
    if let Some(style) = current {
        spans.push(Span::styled(run, style));
    }

    // Trailing fill, including the selected newline at the end of a line the
    // selection runs through.
    let used: usize = spans.iter().map(Span::width).sum();
    let end = selected(row.end).then_some(colors.selection);
    if let Some(color) = end {
        spans.push(Span::styled(" ", Style::default().bg(color)));
    }
    let pad = width.saturating_sub(used + usize::from(end.is_some()));
    spans.push(Span::styled(
        " ".repeat(pad),
        Style::default().bg(colors.background),
    ));
    Line::from(spans)
}

/// What a source line belongs to, as far as painting it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fence {
    Prose,
    /// Inside a fenced code block, the fences included.
    Code,
    /// Inside a `$$` equation set on lines of its own, the delimiters included.
    Math,
}

/// Which lines sit inside a fenced code block or a `$$` equation, so their
/// content is never read as Markdown while it is being edited. TeX is full of
/// `_` and `*`, which would otherwise be dimmed and italicised as emphasis.
fn fenced_lines(lines: &[String]) -> Vec<Fence> {
    let is_fence = |line: &String| {
        let trimmed = line.trim_start();
        trimmed.starts_with("```") || trimmed.starts_with("~~~")
    };
    let mut fences = vec![Fence::Prose; lines.len()];
    let mut at = 0;
    while at < lines.len() {
        if is_fence(&lines[at]) {
            // The fence lines belong to the block they open and close, and an
            // unclosed one runs to the end.
            let end = lines[at + 1..]
                .iter()
                .position(is_fence)
                .map_or(lines.len() - 1, |offset| at + 1 + offset);
            fences[at..=end].fill(Fence::Code);
            at = end + 1;
            continue;
        }
        // The same test the reading pane uses, so the two agree on where an
        // equation starts and stops. One on a single line is left to
        // `scan_inline`, which already paints it.
        if lines[at].trim_start().starts_with("$$") {
            let rest: Vec<&str> = lines[at..].iter().map(String::as_str).collect();
            if let Some(math) = markdown::math_block(&rest)
                && math.lines > 1
            {
                fences[at..at + math.lines].fill(Fence::Math);
                at += math.lines;
                continue;
            }
        }
        at += 1;
    }
    fences
}

/// Styles one source line character by character, ready to be sliced across
/// however many rows it wrapped onto.
///
/// Markers are dimmed and given the glyph the reading pane would use, but only
/// where that glyph is exactly as wide as what it replaces — a `-` becomes a
/// `•`, an `x` inside `[x]` becomes a `☑`. Nothing is hidden and nothing moves,
/// so the caret is always where the character is, and `on_cursor_line` can show
/// the line exactly as typed without the text shifting underneath.
fn paint_line(
    text: &str,
    on_cursor_line: bool,
    fence: Fence,
    palette: &Palette,
) -> Vec<(char, Style)> {
    let chars: Vec<char> = text.chars().collect();
    match fence {
        Fence::Code => {
            let style = Style::default().fg(palette.code_fg).bg(palette.code_bg);
            return chars.into_iter().map(|ch| (ch, style)).collect();
        }
        Fence::Math => {
            // TeX in the maths colour, with its `$$` dimmed like any other
            // delimiter.
            let paint = |ink| {
                let paint = markdown::Paint {
                    style: markdown::Style::default(),
                    ink,
                };
                paint_style(paint, on_cursor_line, palette)
            };
            let (tex, marker) = (paint(markdown::Ink::Math), paint(markdown::Ink::Marker));
            let mut out: Vec<(char, Style)> = chars.iter().map(|&ch| (ch, tex)).collect();
            for at in 1..chars.len() {
                if chars[at - 1] == '$' && chars[at] == '$' {
                    out[at - 1].1 = marker;
                    out[at].1 = marker;
                }
            }
            return out;
        }
        Fence::Prose => {}
    }

    let mut out: Vec<(char, Style)> = markdown::scan_inline(text)
        .into_iter()
        .zip(&chars)
        .map(|(paint, ch)| (*ch, paint_style(paint, on_cursor_line, palette)))
        .collect();

    decorate_markers(&chars, on_cursor_line, palette, &mut out);
    out
}

/// Theme colors for one inline character of source.
fn paint_style(paint: markdown::Paint, on_cursor_line: bool, palette: &Palette) -> Style {
    use markdown::Ink;

    let mut style = Style::default().fg(match paint.ink {
        // On the cursor's own line the syntax is what you are editing, so it is
        // shown at full contrast rather than pushed into the background.
        Ink::Marker if on_cursor_line => palette.text_muted,
        Ink::Marker => palette.text_faint,
        // Whether a link resolves is the reading pane's business: it has the
        // link target, and this only has the characters.
        Ink::WikiLink => palette.link,
        Ink::Link => palette.link_external,
        Ink::Tag => palette.tag_fg,
        Ink::Math => palette.text_accent,
        Ink::Text => palette.text_normal,
    });
    if paint.ink == Ink::Tag {
        style = style.bg(palette.tag_bg);
    }
    emphasis(style, paint.style, palette)
}

/// Applies bold, italic, strikethrough, inline code and highlight colors.
///
/// Shared with the reading pane so a bold word can't end up a different color
/// depending on which mode you are looking at it in.
fn emphasis(mut style: Style, md: markdown::Style, palette: &Palette) -> Style {
    if md.code {
        style = style.fg(palette.code_fg).bg(palette.code_bg);
    }
    if md.bold {
        style = style.fg(palette.bold).add_modifier(Modifier::BOLD);
    }
    if md.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if md.strikethrough {
        style = style
            .fg(palette.strikethrough)
            .add_modifier(Modifier::CROSSED_OUT);
    }
    if md.highlight {
        style = style
            .bg(palette.text_highlight_bg)
            .fg(palette.text_highlight_fg);
    }
    style
}

/// Restyles a line's leading Markdown marker, and swaps in the glyph the
/// reading pane uses wherever it is exactly as wide.
fn decorate_markers(
    chars: &[char],
    on_cursor_line: bool,
    palette: &Palette,
    out: &mut [(char, Style)],
) {
    let indent = chars
        .iter()
        .take_while(|ch| **ch == ' ' || **ch == '\t')
        .count();
    let rest: String = chars[indent..].iter().collect();
    let at = |offset: usize| indent + offset;

    // A heading keeps its hashes, dimmed, and colors its title.
    let hashes = rest.chars().take_while(|ch| *ch == '#').count();
    if (1..=6).contains(&hashes) && rest.chars().nth(hashes) == Some(' ') {
        let level = u8::try_from(hashes).unwrap_or(6);
        let title = Style::default()
            .fg(palette.heading(level))
            .add_modifier(Modifier::BOLD);
        for entry in out.iter_mut().skip(at(hashes + 1)) {
            entry.1 = title;
        }
        return;
    }

    // `- [x] `: color the box, tick it, and strike the item through.
    if let Some(checked) = task(&rest) {
        let color = if checked {
            palette.checkbox_done
        } else {
            palette.checkbox_todo
        };
        substitute(out, at(0), bullet_glyph(on_cursor_line), color);
        for offset in [2, 4] {
            if let Some(entry) = out.get_mut(at(offset)) {
                entry.1 = Style::default().fg(palette.text_faint);
            }
        }
        let tick = if checked { icons::TASK_DONE } else { ' ' };
        substitute(out, at(3), (!on_cursor_line).then_some(tick), color);
        if checked {
            for entry in out.iter_mut().skip(at(6)) {
                entry.1 = entry
                    .1
                    .fg(palette.text_faint)
                    .add_modifier(Modifier::CROSSED_OUT);
            }
        }
        return;
    }

    if matches!(rest.as_bytes().first(), Some(b'-' | b'*' | b'+'))
        && rest.as_bytes().get(1) == Some(&b' ')
    {
        substitute(
            out,
            at(0),
            bullet_glyph(on_cursor_line),
            palette.list_marker,
        );
        return;
    }

    // `1. ` keeps its number; only the color says it is a marker.
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits > 0
        && matches!(rest.as_bytes().get(digits), Some(b'.' | b')'))
        && rest.as_bytes().get(digits + 1) == Some(&b' ')
    {
        for entry in out.iter_mut().skip(indent).take(digits + 1) {
            entry.1 = Style::default().fg(palette.list_marker);
        }
        return;
    }

    // Every `>` of a quote, at every level, becomes the bar the reading pane
    // draws — and the body it introduces is tinted to match.
    if rest.starts_with('>') {
        let prefix = rest
            .bytes()
            .take_while(|b| matches!(b, b'>' | b' '))
            .count();
        for offset in 0..prefix {
            if chars.get(at(offset)) == Some(&'>') {
                substitute(
                    out,
                    at(offset),
                    (!on_cursor_line).then_some(icons::QUOTE_BAR),
                    palette.quote_bar,
                );
            }
        }
        for entry in out.iter_mut().skip(at(prefix)) {
            if entry.1.fg == Some(palette.text_normal) {
                entry.1 = entry.1.fg(palette.quote_fg);
            }
        }
    }
}

/// The glyph a list bullet is drawn as, or nothing on the cursor's own line
/// where the character as typed is what matters.
fn bullet_glyph(on_cursor_line: bool) -> Option<char> {
    (!on_cursor_line).then_some(icons::BULLET)
}

/// Recolors one character, optionally drawing a different glyph in its place.
///
/// Only ever called with a replacement the same width as the original, so the
/// column a character occupies never depends on how it is styled.
fn substitute(
    out: &mut [(char, Style)],
    at: usize,
    glyph: Option<char>,
    color: ratatui::style::Color,
) {
    if let Some(entry) = out.get_mut(at) {
        if let Some(glyph) = glyph {
            entry.0 = glyph;
        }
        entry.1 = Style::default().fg(color);
    }
}

/// Whether a line is a task, and whether it is done.
fn task(rest: &str) -> Option<bool> {
    let bytes = rest.as_bytes();
    let valid = matches!(bytes.first(), Some(b'-' | b'*' | b'+'))
        && bytes.get(1) == Some(&b' ')
        && bytes.get(2) == Some(&b'[')
        && bytes.get(4) == Some(&b']')
        && bytes.get(5) == Some(&b' ');
    valid.then(|| matches!(bytes.get(3), Some(b'x' | b'X')))
}

#[cfg(test)]
mod tests {
    use super::*;
    use emeraldian_core::test_support::TempVault;
    use emeraldian_theme::{Palette, presets};

    fn palette() -> Palette {
        Palette::from(&presets::default_theme())
    }

    #[test]
    fn the_reading_layout_is_reused_until_the_note_changes() {
        let vault = TempVault::new("reading-cache");
        vault.write("A.md", "first version\n");
        let mut app = App::new(vault.vault(), crate::config::Config::default()).expect("app");
        let id = app.index.id_of_rel("A.md").expect("indexed");
        app.open_note(id);
        let screen = |app: &mut App| -> String {
            let backend = ratatui::backend::TestBackend::new(80, 20);
            let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
            terminal
                .draw(|frame| crate::ui::draw(frame, app))
                .expect("draw");
            let buffer = terminal.backend().buffer();
            buffer.content.iter().map(|cell| cell.symbol()).collect()
        };

        assert!(screen(&mut app).contains("first version"));
        let laid_out = app.reading.lines.as_ptr();
        screen(&mut app);
        assert_eq!(
            app.reading.lines.as_ptr(),
            laid_out,
            "a redraw lays nothing out again"
        );

        // Changed on disk and indexed again, as the watcher does.
        vault.write("A.md", "the second version\n");
        app.index.refresh_note(id).expect("refresh");
        assert!(screen(&mut app).contains("the second version"));
    }

    fn text_of(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn wrap_spans_preserves_styles_across_lines() {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let spans = vec![
            ("hello ".to_string(), bold),
            ("world again".to_string(), bold),
        ];
        let lines = wrap_spans(&spans, 11, "", "");

        assert!(lines.len() > 1, "should wrap");
        for line in &lines {
            for span in &line.spans {
                if !span.content.trim().is_empty() {
                    assert!(span.style.add_modifier.contains(Modifier::BOLD));
                }
            }
        }
    }

    #[test]
    fn wrap_spans_applies_a_hanging_indent() {
        let spans = vec![("one two three four".to_string(), Style::default())];
        let lines = wrap_spans(&spans, 10, "", "    ");
        let rendered = text_of(&lines);

        assert!(rendered.len() > 1);
        assert!(
            rendered[1].starts_with("    "),
            "continuation lines are indented: {rendered:?}"
        );
    }

    /// A real PNG of the given size, since the layout reads its header.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::new(width, height));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encode png");
        bytes
    }

    /// One cell is 10x20 pixels, so a 400x200 picture is 40x10 cells.
    ///
    /// The cap is a share of the pane, so a pane of `max_rows` rows with the
    /// share set to all of it caps pictures at exactly that many rows.
    fn images(max_rows: u16) -> Images {
        let mut images = Images::halfblocks(100);
        images.set_pane_height(max_rows);
        images
    }

    fn drawing<'a>(palette: &'a Palette, index: &'a VaultIndex, images: &'a mut Images) -> Ctx<'a> {
        Ctx {
            palette,
            index,
            note_dir: None,
            images: Some(images),
            pictures: Vec::new(),
            anchors: Vec::new(),
            pans: Vec::new(),
            inks: None,
        }
    }

    #[test]
    fn a_picture_reserves_its_rows_where_the_text_leaves_off() {
        let vault = TempVault::new("render-image");
        vault.write_bytes("chart.png", &png(400, 200));
        let index = vault.index();
        let palette = palette();
        let mut images = images(20);

        let document = markdown::parse("Before\n\n![a chart](chart.png)\n\nAfter\n");
        let mut ctx = drawing(&palette, &index, &mut images);
        let lines = render_document(&document, &mut ctx, 60);

        let picture = match ctx.pictures.as_slice() {
            [picture] => picture,
            other => panic!("expected one picture, got {}", other.len()),
        };
        assert_eq!(
            (picture.size.width, picture.size.height),
            (40, 10),
            "400x200 pixels is 40x10 cells at this font size"
        );

        let rendered = text_of(&lines);
        assert!(
            rendered[picture.line..picture.line + 10]
                .iter()
                .all(|line| line.is_empty()),
            "the rows it will be drawn over are left blank: {rendered:?}"
        );
        let after = rendered
            .iter()
            .position(|line| line.contains("After"))
            .expect("text after the picture");
        assert!(
            after >= picture.line + 10,
            "text below it is pushed past its rows, so nothing moves when it loads"
        );
    }

    #[test]
    fn a_picture_too_wide_for_the_pane_is_scaled_to_fit() {
        let vault = TempVault::new("render-image-wide");
        vault.write_bytes("wide.png", &png(4000, 1000));
        let index = vault.index();
        let palette = palette();
        let mut images = images(20);

        let mut ctx = drawing(&palette, &index, &mut images);
        let document = markdown::parse("![](wide.png)\n");
        render_document(&document, &mut ctx, 30);

        assert_eq!(ctx.pictures[0].size.width, 30, "bounded by the pane");
        assert_eq!(ctx.pictures[0].size.height, 4, "and keeps its proportions");
    }

    #[test]
    fn a_tall_picture_stops_at_the_configured_row_cap() {
        let vault = TempVault::new("render-image-tall");
        vault.write_bytes("tall.png", &png(400, 4000));
        let index = vault.index();
        let palette = palette();
        let mut images = images(8);

        let mut ctx = drawing(&palette, &index, &mut images);
        render_document(&markdown::parse("![](tall.png)\n"), &mut ctx, 60);

        assert_eq!(
            ctx.pictures[0].size.height, 8,
            "a portrait photo shouldn't take several screens on its own"
        );
    }

    #[test]
    fn a_picture_grows_with_the_pane_rather_than_stopping_at_a_fixed_height() {
        // A cap in rows that suits an 80x24 terminal leaves a diagram
        // unreadable on a full-screen one, which is what made pictures look
        // like mush regardless of how well they were resampled.
        let vault = TempVault::new("render-image-share");
        vault.write_bytes("tall.png", &png(400, 4000));
        let index = vault.index();
        let palette = palette();

        let height_in = |rows: u16| {
            let mut images = Images::halfblocks(50);
            images.set_pane_height(rows);
            let mut ctx = drawing(&palette, &index, &mut images);
            render_document(&markdown::parse("![](tall.png)\n"), &mut ctx, 60);
            ctx.pictures[0].size.height
        };

        assert_eq!(height_in(24), 12, "half of a short pane");
        assert_eq!(height_in(60), 30, "and half of a tall one");
    }

    #[test]
    fn an_obsidian_embed_of_an_image_is_a_picture_but_of_a_note_is_not() {
        let vault = TempVault::new("render-embed");
        vault.write_bytes("assets/chart.png", &png(200, 100));
        vault.write("Other.md", "body\n");
        let index = vault.index();
        let palette = palette();
        let mut images = images(20);

        let mut ctx = drawing(&palette, &index, &mut images);
        // The picture is found by bare filename, from a subfolder, as Obsidian
        // resolves it.
        render_document(&markdown::parse("![[chart.png]]\n"), &mut ctx, 60);
        assert_eq!(ctx.pictures.len(), 1, "an image embed draws");

        let mut ctx = drawing(&palette, &index, &mut images);
        render_document(&markdown::parse("![[Other]]\n"), &mut ctx, 60);
        assert!(ctx.pictures.is_empty(), "a note embed is not a picture");
    }

    #[test]
    fn an_obsidian_width_hint_shrinks_the_picture() {
        let vault = TempVault::new("render-image-hint");
        vault.write_bytes("chart.png", &png(400, 200));
        let index = vault.index();
        let palette = palette();
        let mut images = images(20);

        let mut ctx = drawing(&palette, &index, &mut images);
        render_document(&markdown::parse("![[chart.png]]\n"), &mut ctx, 60);
        assert_eq!(ctx.pictures[0].size.width, 40, "40 cells at its own size");

        // 200 pixels is 20 cells at this font.
        let mut ctx = drawing(&palette, &index, &mut images);
        render_document(&markdown::parse("![[chart.png|200]]\n"), &mut ctx, 60);
        assert_eq!(ctx.pictures[0].size.width, 20, "the hint wins");
        assert_eq!(ctx.pictures[0].size.height, 5, "and it keeps its shape");

        // A hint wider than the pane is still bounded by the pane.
        let mut ctx = drawing(&palette, &index, &mut images);
        render_document(&markdown::parse("![[chart.png|9000]]\n"), &mut ctx, 30);
        assert_eq!(ctx.pictures[0].size.width, 30);

        // Everything else after the pipe is an alias, not a size.
        let mut ctx = drawing(&palette, &index, &mut images);
        render_document(&markdown::parse("![[chart.png|a diagram]]\n"), &mut ctx, 60);
        assert_eq!(ctx.pictures[0].size.width, 40);
    }

    #[test]
    fn a_picture_that_cannot_be_drawn_falls_back_to_its_alt_text() {
        let vault = TempVault::new("render-image-missing");
        vault.write_bytes("chart.png", &png(200, 100));
        let index = vault.index();
        let palette = palette();

        // Every reason a picture might not be drawable, and all of them should
        // leave readable text behind rather than a hole.
        let cases = [
            ("![a chart](chart.png)", "a terminal that cannot draw"),
            ("![gone](missing.png)", "a file that isn't there"),
            ("![remote](https://e.com/x.png)", "a picture on the web"),
        ];
        for (source, reason) in cases {
            let mut images = match reason {
                "a terminal that cannot draw" => Images::disabled(),
                _ => images(20),
            };
            let mut ctx = drawing(&palette, &index, &mut images);
            let lines = render_document(&markdown::parse(source), &mut ctx, 60);

            assert!(ctx.pictures.is_empty(), "nothing is reserved for {reason}");
            let rendered = text_of(&lines).join(" ");
            let alt = source
                .split_once('[')
                .and_then(|(_, rest)| rest.split_once(']'))
                .map(|(alt, _)| alt)
                .unwrap_or_default();
            assert!(
                rendered.contains(alt),
                "{reason} still reads as {alt:?}: {rendered:?}"
            );
        }
    }

    #[test]
    fn a_picture_inside_a_quote_moves_with_the_quote() {
        let vault = TempVault::new("render-image-quote");
        vault.write_bytes("chart.png", &png(200, 100));
        let index = vault.index();
        let palette = palette();
        let mut images = images(20);

        let mut ctx = drawing(&palette, &index, &mut images);
        let lines = render_document(
            &markdown::parse("Intro\n\n> quoted\n> ![](chart.png)\n"),
            &mut ctx,
            60,
        );

        let picture = &ctx.pictures[0];
        assert!(
            picture.indent >= 2,
            "it sits to the right of the quote bar, not under it"
        );
        let rendered = text_of(&lines);
        assert!(
            rendered[picture.line]
                .trim_end()
                .ends_with(icons::QUOTE_BAR),
            "its first row is a quote bar and then blank: {:?}",
            rendered[picture.line]
        );
    }

    #[test]
    fn headings_are_colored_by_level_and_underlined() {
        let vault = TempVault::new("render-heading");
        let index = vault.index();
        let palette = palette();
        let document = markdown::parse("# Title\n\n### Deep\n");

        let lines = render_document(&document, &mut Ctx::text(&palette, &index), 40);
        let rendered = text_of(&lines);

        assert!(rendered.iter().any(|l| l.contains("Title")));
        assert!(
            rendered.iter().any(|l| l.starts_with('─')),
            "H1 gets a rule under it: {rendered:?}"
        );
        assert!(rendered.iter().any(|l| l.contains("Deep")));
    }

    #[test]
    fn unresolved_wikilinks_render_dimmed() {
        let vault = TempVault::new("render-link");
        vault.write("Real.md", "x");
        let index = vault.index();
        let palette = palette();

        let document = markdown::parse("See [[Real]] and [[Missing]].\n");
        let lines = render_document(&document, &mut Ctx::text(&palette, &index), 60);

        let styles: Vec<_> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .filter(|s| s.content.contains("Real") || s.content.contains("Missing"))
            .map(|s| (s.content.to_string(), s.style.fg))
            .collect();

        let real = styles
            .iter()
            .find(|(t, _)| t.contains("Real"))
            .expect("Real");
        let missing = styles
            .iter()
            .find(|(t, _)| t.contains("Missing"))
            .expect("Missing");

        assert_eq!(real.1, Some(palette.link));
        assert_eq!(missing.1, Some(palette.link_unresolved));
    }

    #[test]
    fn tasks_render_with_checkboxes_and_strike_completed_ones() {
        let vault = TempVault::new("render-task");
        let index = vault.index();
        let palette = palette();

        let document = markdown::parse("- [ ] todo\n- [x] done\n");
        let lines = render_document(&document, &mut Ctx::text(&palette, &index), 40);
        let rendered = text_of(&lines);

        assert!(rendered.iter().any(|l| l.contains(icons::TASK_TODO)));
        assert!(rendered.iter().any(|l| l.contains(icons::TASK_DONE)));

        let done_line = lines
            .iter()
            .find(|l| l.spans.iter().any(|s| s.content.contains("done")))
            .expect("done line");
        assert!(
            done_line
                .spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::CROSSED_OUT))
        );
    }

    #[test]
    fn callouts_render_with_a_colored_bar_and_title() {
        let vault = TempVault::new("render-callout");
        let index = vault.index();
        let palette = palette();

        let document = markdown::parse("> [!warning] Careful\n> body\n");
        let lines = render_document(&document, &mut Ctx::text(&palette, &index), 40);
        let rendered = text_of(&lines);

        assert!(rendered[0].contains(icons::QUOTE_BAR));
        assert!(rendered[0].contains("Careful"));
        assert!(rendered.len() > 1, "the body renders too");
    }

    #[test]
    fn mermaid_blocks_render_as_diagrams_and_bad_ones_as_code() {
        let vault = TempVault::new("render-mermaid");
        let index = vault.index();
        let palette = palette();
        let document = markdown::parse(
            "```mermaid\ngraph LR\n  A[Start] --> B[End]\n```\n\n```mermaid\ngitGraph\n  commit\n  branch develop\n```\n",
        );
        let lines = render_document(&document, &mut Ctx::text(&palette, &index), 60);
        let rendered = text_of(&lines);

        let start = rendered.iter().position(|l| l.contains("Start")).unwrap();
        assert!(rendered[start].contains("End"), "drawn side by side");
        assert!(rendered[start].contains('▶'));
        assert!(
            !rendered.iter().any(|l| l.contains("graph LR")),
            "the source is not shown"
        );
        // The arrow is the colour of the box it leaves, and the words in the
        // boxes are body text.
        let colour_of = |row: usize, needle: char| {
            lines[row]
                .spans
                .iter()
                .find(|s| s.content.contains(needle))
                .and_then(|s| s.style.fg)
        };
        let start_box = colour_of(start - 1, '┌');
        assert_eq!(colour_of(start, '▶'), start_box);
        assert_ne!(start_box, Some(palette.accent));
        assert_eq!(colour_of(start, 'S'), Some(palette.text_normal));

        // A kind it doesn't draw stays code, header and all.
        assert!(rendered.iter().any(|l| l.contains("mermaid")));
        assert!(rendered.iter().any(|l| l.contains("branch develop")));
    }

    #[test]
    fn two_boxes_in_a_diagram_get_two_of_the_themes_colours() {
        let vault = TempVault::new("render-mermaid-colours");
        let index = vault.index();
        let palette = palette();
        let document = markdown::parse("```mermaid\ngraph LR\n  A[Start] --> B[End]\n```\n");
        let lines = render_document(&document, &mut Ctx::text(&palette, &index), 60);
        let top = lines
            .iter()
            .find(|line| line.spans.iter().any(|s| s.content.contains('┌')))
            .expect("the boxes' top edges");
        let outlines: Vec<_> = top
            .spans
            .iter()
            .filter(|s| s.content.contains('┌'))
            .filter_map(|s| s.style.fg)
            .collect();
        assert_eq!(outlines.len(), 2, "{top:?}");
        assert_ne!(outlines[0], outlines[1], "two boxes, two colours");
        let hues = palette.hues().strokes;
        assert!(outlines.iter().all(|colour| hues.contains(colour)));
    }

    /// Whatever the theme, light or dark, every colour a diagram is drawn in
    /// stands out from the page: words at 4.5:1 and lines at 3:1.
    #[test]
    fn diagrams_read_on_the_page_in_every_theme() {
        let vault = TempVault::new("render-mermaid-contrast");
        let index = vault.index();
        let document = markdown::parse(concat!(
            "```mermaid\ngraph TD\n  A[One] -->|yes| B[Two]\n  A -.->|no| C((Three))\n",
            "  B & C ==> D{Four}\n  D --> E[[Five]]\n  E --> F[Six] --> G[Seven] --> H[Eight] --> A\n```\n\n",
            "```mermaid\nsequenceDiagram\n  participant P as Pat\n  actor Q as Quinn\n  P->>Q: hello\n",
            "  loop again\n    Q-->>P: back\n  end\n  Note over P,Q: aside\n  Q->>Q: think\n```\n\n",
            "```mermaid\nC4Container\n  title Shop\n  Person(c, \"Customer\", \"Buys things\")\n",
            "  Container_Boundary(b, \"Shop\") {\n    Container(w, \"Web\", \"Rust\", \"Serves pages\")\n",
            "    ContainerDb(d, \"Orders\", \"Postgres\")\n  }\n  System_Ext(m, \"Mail\")\n",
            "  Rel(c, w, \"Uses\", \"HTTPS\")\n  Rel(w, d, \"Stores\")\n  Rel(w, m, \"Sends\")\n```\n\n",
            "```mermaid\nclassDiagram\n  class Animal {\n    <<abstract>>\n    +int age\n    +mate()\n  }\n",
            "  Animal <|-- Duck\n  Duck \"1\" *-- \"many\" Egg : lays\n  note for Duck \"quacks\"\n```\n\n",
            "```mermaid\nstateDiagram-v2\n  [*] --> Still\n  state Moving {\n    [*] --> Slow\n  }\n",
            "  Still --> Moving : go\n  Moving --> [*]\n  note right of Still : resting\n```\n\n",
            "```mermaid\nerDiagram\n  CUSTOMER ||--o{ ORDER : places\n  CUSTOMER {\n",
            "    string name PK \"the name\"\n  }\n```\n\n",
            "```mermaid\nmindmap\n  root((Plans))\n    Work\n      Ship it\n    Home\n```\n\n",
            "```mermaid\ntimeline\n  title Years\n  section Early\n    2002 : LinkedIn\n```\n\n",
            "```mermaid\njourney\n  title Day\n  section Work\n    Make tea: 5: Me, Cat\n```\n\n",
            "```mermaid\npie showData title Pets\n  \"Dogs\" : 386\n  \"Cats\" : 85\n  \"Rats\" : 15\n```\n\n",
            "```mermaid\ngantt\n  dateFormat YYYY-MM-DD\n  section Build\n    Design : done, a1, 2024-01-01, 5d\n",
            "    Code : after a1, 10d\n    Ship : milestone, 2024-01-20, 0d\n```\n\n",
            "```mermaid\nxychart-beta\n  title \"Sales\"\n  x-axis [jan, feb, mar]\n",
            "  bar \"won\" [5, 6, 7.5]\n  line \"lost\" [3, 4, 2]\n```\n",
        ));
        for theme in presets::builtin() {
            let palette = Palette::from(&theme);
            let lines = render_document(&document, &mut Ctx::text(&palette, &index), 100);
            let rendered = text_of(&lines);
            for drawn in [
                "Quinn",
                "[Container: Rust]",
                "<<abstract>>",
                "Still",
                "places",
                "Ship it",
                "LinkedIn",
                "Make tea",
                "Dogs",
                "Design",
                "Sales",
            ] {
                assert!(
                    rendered.iter().any(|line| line.contains(drawn)),
                    "{drawn} is drawn, not left as code"
                );
            }
            assert!(
                !rendered
                    .iter()
                    .any(|line| line.trim_start().starts_with("mermaid")),
                "no block is left as code"
            );
            for span in lines.iter().flat_map(|line| &line.spans) {
                let Some(fg) = span.style.fg else { continue };
                // The terminal theme leaves its colours to the terminal.
                let Some(ratio) = emeraldian_theme::contrast(fg, palette.bg_primary) else {
                    continue;
                };
                let words = span.content.chars().any(char::is_alphanumeric);
                let least = if words {
                    TEXT_CONTRAST
                } else {
                    STROKE_CONTRAST
                };
                assert!(
                    ratio >= least,
                    "{}: {:?} in {fg:?} is {ratio:.2}:1",
                    theme.name,
                    span.content
                );
            }
        }
    }

    #[test]
    fn a_wide_diagram_keeps_its_width_for_panning() {
        let vault = TempVault::new("render-mermaid-wide");
        let index = vault.index();
        let mut source = String::from("```mermaid\ngraph TD\n  A --> ");
        let fan: Vec<String> = (0..20).map(|at| format!("n{at}[node {at}]")).collect();
        source.push_str(&fan.join(" & "));
        source.push_str("\n```\n");
        let document = markdown::parse(&source);
        let lines = render_document(&document, &mut Ctx::text(&palette(), &index), 40);

        let widest = lines.iter().map(Line::width).max().unwrap();
        assert!(widest > 40, "wider than the pane rather than squeezed");
        let rendered = text_of(&lines);
        assert!(rendered.iter().any(|l| l.contains("node 19")));
    }

    #[test]
    fn a_callout_without_a_title_uses_its_kind() {
        let vault = TempVault::new("render-callout-2");
        let index = vault.index();
        let document = markdown::parse("> [!tip]\n> body\n");
        let lines = render_document(&document, &mut Ctx::text(&palette(), &index), 40);

        assert!(text_of(&lines)[0].contains("Tip"));
    }

    #[test]
    fn tables_render_with_borders_and_fit_the_width() {
        let vault = TempVault::new("render-table");
        let index = vault.index();
        let document = markdown::parse("| A | B |\n|---|---|\n| 1 | 2 |\n");
        let lines = render_document(&document, &mut Ctx::text(&palette(), &index), 40);
        let rendered = text_of(&lines);

        assert!(rendered.iter().any(|l| l.contains('┌')));
        assert!(rendered.iter().any(|l| l.contains('│')));
        for line in &rendered {
            assert!(line.chars().count() <= 44, "{line:?} overflows");
        }
    }

    #[test]
    fn a_wide_table_keeps_its_columns_rather_than_being_crushed() {
        // Eight columns divided between a pane this narrow used to leave three
        // characters each — a table that fits and says nothing. It is laid out
        // at its own width now and panned across instead.
        let vault = TempVault::new("render-wide-table");
        let index = vault.index();

        let header: Vec<String> = (0..8).map(|i| format!("column {i}")).collect();
        let cells: Vec<String> = (0..8).map(|i| format!("value number {i}")).collect();
        let source = format!(
            "| {} |\n|{}|\n| {} |\n",
            header.join(" | "),
            "---|".repeat(8),
            cells.join(" | ")
        );

        let document = markdown::parse(&source);
        let lines = render_document(&document, &mut Ctx::text(&palette(), &index), 40);
        let rendered = text_of(&lines);

        let row = rendered
            .iter()
            .find(|line| line.contains("value number 0"))
            .expect("the body row");
        for cell in &cells {
            assert!(
                row.contains(cell.as_str()),
                "{cell:?} was truncated: {row:?}"
            );
        }
        assert!(
            row.chars().count() > 40,
            "the table is wider than the pane, which is what makes panning necessary"
        );
    }

    #[test]
    fn a_runaway_column_cannot_push_the_others_out_of_reach() {
        let vault = TempVault::new("render-essay-table");
        let index = vault.index();
        let essay = "e".repeat(300);
        let source = format!("| A | B |\n|---|---|\n| {essay} | end |\n");

        let lines = render_document(
            &markdown::parse(&source),
            &mut Ctx::text(&palette(), &index),
            40,
        );
        let row = text_of(&lines)
            .into_iter()
            .find(|line| line.contains('e'))
            .expect("the body row");
        assert!(
            row.contains("end"),
            "the last column is still reachable: {row:?}"
        );
        assert!(row.chars().count() < 300, "the essay cell was capped");
    }

    #[test]
    fn code_blocks_highlight_keywords() {
        let palette = palette();
        let spans = highlight("let x = 1;", "rust", &palette, Style::default());

        let keyword = spans
            .iter()
            .find(|s| s.content == "let")
            .expect("keyword span");
        assert_eq!(keyword.style.fg, Some(palette.syn_keyword));

        let number = spans
            .iter()
            .find(|s| s.content == "1")
            .expect("number span");
        assert_eq!(number.style.fg, Some(palette.syn_number));
    }

    #[test]
    fn comments_and_strings_are_highlighted() {
        let palette = palette();

        let comment = highlight("# a note", "python", &palette, Style::default());
        assert_eq!(comment[0].style.fg, Some(palette.syn_comment));

        let string = highlight("x = \"hi\"", "python", &palette, Style::default());
        assert!(
            string
                .iter()
                .any(|s| s.style.fg == Some(palette.syn_string) && s.content.contains("hi"))
        );
    }

    #[test]
    fn an_unknown_language_still_renders() {
        let palette = palette();
        let spans = highlight("some text", "brainfuck", &palette, Style::default());
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "some text");
    }

    #[test]
    fn frontmatter_renders_as_a_metadata_header() {
        let vault = TempVault::new("render-fm");
        let index = vault.index();
        let document = markdown::parse("---\ntitle: T\ntags: [a]\n---\nbody\n");
        let lines = render_document(&document, &mut Ctx::text(&palette(), &index), 40);
        let rendered = text_of(&lines);

        assert!(rendered.iter().any(|l| l.contains("title: T")));
        assert!(rendered.iter().any(|l| l.contains("tags: a")));
        assert!(rendered.iter().any(|l| l.contains("body")));
    }

    #[test]
    fn rendering_never_produces_lines_wider_than_the_pane() {
        let vault = TempVault::new("render-width");
        let index = vault.index();
        let long = "word ".repeat(60);
        let document = markdown::parse(&format!("{long}\n\n- {long}\n\n> {long}\n"));

        for width in [20usize, 40, 80] {
            let lines = render_document(&document, &mut Ctx::text(&palette(), &index), width);
            for line in text_of(&lines) {
                assert!(
                    line.chars().count() <= width + 4,
                    "width {width}: {:?} is {} chars",
                    line,
                    line.chars().count()
                );
            }
        }
    }

    /// The widest rendered row, in display columns, ignoring trailing space.
    fn widest(lines: &[String]) -> usize {
        lines
            .iter()
            .map(|line| line.trim_end().width())
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn a_long_heading_wraps_instead_of_running_off_the_pane() {
        // One line of an exported chat, pasted as a single heading. It used to
        // be drawn as one row, so everything past the first few words — maths
        // included — was off the edge of the pane.
        let rows = shown(&format!("# {}\n", "a long heading ".repeat(20)), 40);
        assert!(rows.len() > 5, "{rows:?}");
        assert!(widest(&rows) <= 40, "{rows:?}");
        let rule = rows.iter().position(|row| row.starts_with('─'));
        assert_eq!(
            rule,
            Some(rows.len() - 1),
            "the rule goes under the last row"
        );
    }

    #[test]
    fn maths_in_a_heading_is_typeset_and_display_maths_set_apart() {
        let rows = shown(
            "# Such as $\\pi$, and $$ \\pi = 4\\arctan(1) $$ which takes a few symbols\n",
            40,
        );
        assert!(rows[0].starts_with("Such as π,"), "{rows:?}");
        assert!(
            rows.iter().any(|row| row.trim() == "π = 4 arctan(1)"),
            "the display maths has a row of its own: {rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.starts_with("which takes")),
            "{rows:?}"
        );
    }

    #[test]
    fn a_word_longer_than_the_pane_is_broken_at_its_edge() {
        let url =
            "https://docs.example.com/r/en-US/a-very-long-path/Floating-Point-Numerical-Formats";
        let rows = shown(&format!("See {url} for more.\n"), 30);
        assert!(widest(&rows) <= 30, "{rows:?}");
        let joined: String = rows.concat().split_whitespace().collect();
        assert_eq!(
            joined,
            format!("See{url}formore."),
            "nothing is lost along the way"
        );
    }

    #[test]
    fn wide_characters_wrap_by_the_columns_they_take() {
        let rows = shown(&format!("{}\n", "日本語のテキスト ".repeat(6)), 20);
        assert!(widest(&rows) <= 20, "{rows:?}");
    }

    #[test]
    fn a_callout_title_and_a_frontmatter_value_wrap() {
        let title = "a callout title that is far too long for the pane ";
        let rows = shown(&format!("> [!note] {title}\n> body\n"), 24);
        assert!(widest(&rows) <= 24, "{rows:?}");
        assert!(rows.len() > 3, "the title takes several rows: {rows:?}");

        let rows = shown(
            &format!("---\nsummary: {}\n---\nbody\n", "words ".repeat(20)),
            30,
        );
        assert!(widest(&rows) <= 30, "{rows:?}");
        assert!(
            rows[1].starts_with("  "),
            "continued under the value: {rows:?}"
        );
    }

    #[test]
    fn wrapping_inside_an_indent_wider_than_the_pane_still_ends() {
        let spans = [("abcdef".to_string(), Style::default())];
        let lines = wrap_spans(&spans, 2, "", "    ");
        assert!(lines.len() >= 6, "one character a row, without looping");
    }

    /// Rendered rows with trailing space trimmed and non-breaking spaces shown
    /// as the plain ones they look like.
    fn shown(source: &str, width: usize) -> Vec<String> {
        let vault = TempVault::new("render-maths");
        let index = vault.index();
        let lines = render_document(
            &markdown::parse(source),
            &mut Ctx::text(&palette(), &index),
            width,
        );
        text_of(&lines)
            .into_iter()
            .map(|line| line.replace('\u{a0}', " ").trim_end().to_string())
            .collect()
    }

    #[test]
    fn inline_maths_is_typeset_in_the_line() {
        assert_eq!(
            shown("Energy is $E=mc^2$, roughly.\n", 60),
            ["Energy is E = mc², roughly."]
        );
        // A price is still a price.
        assert_eq!(shown("From $5 to $10.\n", 60), ["From $5 to $10."]);
    }

    #[test]
    fn inline_maths_is_drawn_in_the_maths_colour() {
        let vault = TempVault::new("render-maths-colour");
        let index = vault.index();
        let palette = palette();
        let lines = render_document(
            &markdown::parse("a $x$ b\n"),
            &mut Ctx::text(&palette, &index),
            40,
        );
        let maths = lines[0]
            .spans
            .iter()
            .find(|span| span.content == "x")
            .expect("the maths has a span of its own");
        assert_eq!(maths.style.fg, Some(palette.text_accent));
    }

    #[test]
    fn a_display_equation_is_centred_on_rows_of_its_own() {
        assert_eq!(
            shown("Before\n\n$$\n\\frac{a+b}{2}\n$$\n\nAfter\n", 21),
            [
                "Before",
                "",
                "        a + b",
                "        ─────",
                "          2",
                "",
                "After"
            ]
        );
    }

    #[test]
    fn display_maths_in_the_middle_of_a_paragraph_is_set_apart() {
        assert_eq!(
            shown("so that $$x^2 = 4$$ which means\n", 20),
            ["so that", "       x² = 4", "which means"]
        );
    }

    #[test]
    fn an_equation_wider_than_the_pane_starts_at_the_margin() {
        let tex = "a + b + c + d + e + f + g + h";
        let rows = shown(&format!("$${tex}$$\n"), 10);
        assert_eq!(rows, [tex], "panned across rather than wrapped");
    }

    #[test]
    fn an_equation_number_sits_at_the_right_margin() {
        assert_eq!(
            shown("$$E = mc^2 \\tag{1}$$\n", 30),
            ["           E = mc²         (1)"]
        );
    }

    #[test]
    fn maths_in_headings_and_tables_is_typeset_too() {
        let rows = shown(
            "# The $\\alpha$ case\n\n| a | b |\n|---|---|\n| $x^2$ | $\\hat{y}$ |\n",
            40,
        );
        assert!(rows.contains(&"The α case".to_string()), "{rows:?}");
        // The combining hat takes no room, so the borders still line up.
        let widths: Vec<usize> = rows
            .iter()
            .filter(|row| row.starts_with(['┌', '│', '└']))
            .map(|row| row.width())
            .collect();
        assert!(widths.windows(2).all(|pair| pair[0] == pair[1]), "{rows:?}");
        assert!(rows.iter().any(|row| row.contains("x²")), "{rows:?}");
    }

    #[test]
    fn a_display_equation_is_tex_in_the_editor() {
        let lines: Vec<String> = ["$$", "x_1 + y_2 * z", "$$", "a_b c_d"]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            fenced_lines(&lines),
            [Fence::Math, Fence::Math, Fence::Math, Fence::Prose]
        );

        // The underscores are TeX, not emphasis to dim and italicise.
        let palette = palette();
        let painted = paint_line(&lines[1], false, Fence::Math, &palette);
        assert!(
            painted
                .iter()
                .all(|(_, style)| style.fg == Some(palette.text_accent)
                    && !style.add_modifier.contains(Modifier::ITALIC))
        );
        let delimiters = paint_line(&lines[0], false, Fence::Math, &palette);
        assert!(
            delimiters
                .iter()
                .all(|(_, style)| style.fg == Some(palette.text_faint)),
            "the dollars are dimmed like any delimiter"
        );
    }

    #[test]
    fn an_unclosed_equation_does_not_take_over_the_editor() {
        let lines: Vec<String> = ["$$", "x^2", "", "# Next"]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(
            fenced_lines(&lines)
                .iter()
                .all(|fence| *fence == Fence::Prose)
        );
    }

    #[test]
    fn code_fences_still_paint_as_code() {
        let lines: Vec<String> = ["```", "$$", "```", "text", "```"]
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            fenced_lines(&lines),
            [
                Fence::Code,
                Fence::Code,
                Fence::Code,
                Fence::Prose,
                Fence::Code
            ],
            "and an unclosed fence still runs to the end"
        );
    }

    fn plain(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn panning_a_line_holds_its_indent_and_marks_both_edges() {
        let line = Line::from(vec![Span::raw("> "), Span::raw("abcdefghijklmnop")]);
        let marker = Style::default();

        // Unpanned, only the far side is hidden, and the marker says so.
        let shown = pan_line(&line, 2, 0, 10, marker);
        assert_eq!(plain(&shown), "> abcdefg›");

        // Panned, the indent stays and the content moves under it.
        let shown = pan_line(&line, 2, 4, 10, marker);
        assert_eq!(plain(&shown), "> ‹fghijk›");
        assert_eq!(shown.width(), 10);

        // At the end, only the near side is marked.
        let shown = pan_line(&line, 2, 8, 10, marker);
        assert_eq!(plain(&shown), "> ‹jklmnop");
    }

    #[test]
    fn panning_never_splits_a_wide_character() {
        let line = Line::from("漢字テキスト and then some more text");
        for offset in 0..12 {
            let shown = pan_line(&line, 0, offset, 9, Style::default());
            assert_eq!(shown.width(), 9, "offset {offset}: {:?}", plain(&shown));
            // Every visible character is whole: a cut one is blank cells.
            let text = plain(&shown);
            let whole = "漢字テキスト and then some more text";
            for ch in text
                .chars()
                .filter(|ch| !ch.is_ascii() && !"‹›".contains(*ch))
            {
                assert!(whole.contains(ch), "{text:?}");
            }
        }
        // Panned one column, the first character is cut in half, and goes.
        // Panned one column, the marker takes the half of 漢 left showing, and
        // ス, cut by the far edge, leaves a blank cell rather than half itself.
        assert_eq!(
            plain(&pan_line(&line, 0, 1, 9, Style::default())),
            "‹字テキ ›"
        );
        // Emoji are two columns too.
        let line = Line::from("abcd🎉efgh");
        assert_eq!(plain(&pan_line(&line, 0, 0, 6, Style::default())), "abcd ›");
    }

    #[test]
    fn a_marker_keeps_the_background_it_stands_on() {
        let background = Style::default().bg(palette().code_bg);
        let line = Line::from(Span::styled("x".repeat(40), background));
        let shown = pan_line(&line, 0, 5, 10, Style::default().fg(palette().text_faint));
        let first = shown.spans.first().expect("a marker");
        assert_eq!(first.content, "‹");
        assert_eq!(first.style.bg, background.bg);
        assert_eq!(first.style.fg, Some(palette().text_faint));
    }

    #[test]
    fn only_wide_blocks_pan_and_each_stops_at_its_own_edge() {
        let vault = TempVault::new("render-pans");
        let index = vault.index();
        let source = format!(
            "Prose that wraps nicely.\n\n```\n{}\n```\n\n```\n{}\n```\n",
            "x".repeat(30),
            "z".repeat(60),
        );
        let document = markdown::parse(&source);
        let palette = palette();
        let mut ctx = Ctx::text(&palette, &index);
        let lines = render_document(&document, &mut ctx, 20);
        assert_eq!(ctx.pans.len(), 2, "the prose is not a wide block");

        let shown = visible_lines(&lines, &ctx.pans, 0, lines.len(), 25, 20, &palette);
        let rows: Vec<String> = shown.iter().map(plain).collect();
        assert_eq!(rows[0], plain(&lines[0]), "prose stays put: {rows:#?}");
        let short = rows
            .iter()
            .find(|row| row.contains('x'))
            .expect("first block");
        let long = rows
            .iter()
            .find(|row| row.contains('z'))
            .expect("second block");
        // 31 columns of block in 20 of pane leaves 11 to pan, not 25.
        assert!(short.starts_with('‹') && !short.ends_with('›'), "{short:?}");
        assert!(long.starts_with('‹') && long.ends_with('›'), "{long:?}");

        // Panning is held to the blocks in view.
        let first = ctx.pans[0].line;
        assert_eq!(pan_room(&ctx.pans, first, 1, 20), ctx.pans[0].width - 20);
        assert_eq!(pan_room(&ctx.pans, 0, 1, 20), 0, "nothing wide on row 0");
        assert_eq!(
            pan_room(&ctx.pans, 0, lines.len(), 20),
            ctx.pans[1].width - 20
        );
    }

    #[test]
    fn a_wide_block_in_a_quote_pans_behind_its_bar() {
        let vault = TempVault::new("render-pans-quote");
        let index = vault.index();
        let source = format!("> ```\n> {}\n> ```\n", "q".repeat(50));
        let document = markdown::parse(&source);
        let palette = palette();
        let mut ctx = Ctx::text(&palette, &index);
        let lines = render_document(&document, &mut ctx, 20);
        let pan = ctx.pans.first().expect("a wide block").clone();
        assert_eq!(pan.keep, 2, "the bar and its space stay put");

        let shown = visible_lines(&lines, &ctx.pans, 0, lines.len(), 10, 20, &palette);
        let row = plain(&shown[pan.line]);
        assert!(row.starts_with('▎'), "{row:?}");
        assert_eq!(row.chars().nth(2), Some('‹'), "{row:?}");
    }

    #[test]
    fn a_wide_code_block_stays_filled_when_panned() {
        let vault = TempVault::new("render-code-fill");
        let index = vault.index();
        let source = format!("```\n{}\nshort\n```\n", "c".repeat(50));
        let document = markdown::parse(&source);
        let lines = render_document(&document, &mut Ctx::text(&palette(), &index), 20);
        let widths: Vec<usize> = lines.iter().map(Line::width).collect();
        assert_eq!(
            widths[0], widths[1],
            "the short line is filled to the long one"
        );
    }
}
