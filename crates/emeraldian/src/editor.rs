//! The note editor's text buffer.
//!
//! A line-vector buffer with a cursor, selection and undo. Notes are small
//! enough — a long one is a few thousand lines — that a rope would be
//! complexity without benefit, while a `Vec<String>` maps one-to-one onto how
//! the buffer is rendered and how Markdown is parsed.
//!
//! Positions are in **characters**, not bytes, so a cursor never lands inside a
//! multi-byte character.
//!
//! A long line is *shown* over several terminal rows, which is what [`Layout`]
//! works out. That mapping is deliberately kept out of the buffer: the buffer
//! stays the plain line-oriented model everything else parses and indexes, and
//! only the parts that draw or move the cursor need to know how it was wrapped.

use unicode_width::UnicodeWidthChar;

/// Display columns a character occupies.
///
/// A tab is one character but is drawn as spaces to the next stop, so measuring
/// it as one column would put the caret in the wrong place on any indented line.
#[must_use]
fn char_width(ch: char, tab_width: usize, column: usize) -> usize {
    if ch == '\t' {
        tab_width - (column % tab_width.max(1))
    } else {
        ch.width().unwrap_or(0)
    }
}

/// Which of vim's three kinds of character a char is, for the word motions.
///
/// `w` steps over a run of one kind at a time, which is why `foo.bar` is three
/// words and not one: the letters and the dot are different kinds. A WORD (`W`)
/// makes no such distinction and stops only at blanks.
#[must_use]
fn class(ch: char, big: bool) -> u8 {
    if ch.is_whitespace() {
        0
    } else if big || ch.is_alphanumeric() || ch == '_' {
        2
    } else {
        1
    }
}

/// A position in the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Cursor {
    pub line: usize,
    /// Character offset within the line.
    pub col: usize,
}

/// One terminal row: the slice of a source line that is drawn on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    /// Index of the source line this row came from.
    pub line: usize,
    /// First character of that line shown here.
    pub start: usize,
    /// One past the last character shown here.
    pub end: usize,
    /// Blank columns before the text, so a wrapped list item lines up under
    /// its own words rather than under its bullet.
    pub indent: u16,
    /// False on a soft-wrap continuation of the row above.
    pub first: bool,
}

/// How the buffer's lines were laid out across the rows of a viewport.
///
/// Rebuilt wherever it is needed rather than cached: it costs one pass over the
/// text, which is what the reading pane already spends parsing Markdown every
/// frame, and a cache here would only be a way to draw a stale note.
#[derive(Debug, Clone)]
pub struct Layout {
    rows: Vec<Row>,
    /// Index into `rows` of the first row of each source line.
    first: Vec<usize>,
    /// Columns the text was wrapped to.
    width: usize,
    wrapped: bool,
}

/// Narrowest column a wrapped line is ever squeezed into.
///
/// A deeply indented list item in a slim pane would otherwise be wrapped to
/// nothing and loop forever; below this the hanging indent is given up instead.
const MIN_WRAP: usize = 12;

impl Layout {
    /// Lays `lines` out for a viewport `width` columns wide.
    ///
    /// With `wrap` off every line gets exactly one row, however long it is, and
    /// the viewport pans sideways instead — which is what someone who turned
    /// wrapping off asked for.
    #[must_use]
    pub fn build(lines: &[String], width: usize, wrap: bool, tab_width: usize) -> Self {
        let mut rows = Vec::with_capacity(lines.len());
        let mut first = Vec::with_capacity(lines.len());

        for (index, text) in lines.iter().enumerate() {
            first.push(rows.len());
            let chars: Vec<char> = text.chars().collect();
            if !wrap || width == 0 {
                rows.push(Row {
                    line: index,
                    start: 0,
                    end: chars.len(),
                    indent: 0,
                    first: true,
                });
                continue;
            }
            wrap_line(&chars, index, width, tab_width, &mut rows);
        }

        // An empty buffer still has one line, so there is always a row to put
        // the cursor on.
        if rows.is_empty() {
            first.push(0);
            rows.push(Row {
                line: 0,
                start: 0,
                end: 0,
                indent: 0,
                first: true,
            });
        }

        Self {
            rows,
            first,
            width,
            wrapped: wrap,
        }
    }

    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    #[must_use]
    pub fn wrapped(&self) -> bool {
        self.wrapped
    }

    /// The row a cursor sits on.
    ///
    /// At a wrap boundary the cursor belongs to the start of the following row,
    /// not the end of the one before, so typing carries on where the text does.
    #[must_use]
    pub fn row_of(&self, cursor: Cursor) -> usize {
        let last = self.rows.len() - 1;
        let Some(&start) = self.first.get(cursor.line) else {
            return last;
        };
        let mut index = start.min(last);
        while index < last
            && self.rows[index + 1].line == cursor.line
            && self.rows[index + 1].start <= cursor.col
        {
            index += 1;
        }
        index
    }

    /// The row and display column a cursor is drawn at.
    #[must_use]
    pub fn position_of(&self, cursor: Cursor, lines: &[String], tab_width: usize) -> (usize, u16) {
        let index = self.row_of(cursor);
        let row = self.rows[index];
        let mut column = usize::from(row.indent);
        if let Some(text) = lines.get(row.line) {
            for ch in text.chars().take(cursor.col).skip(row.start) {
                column += char_width(ch, tab_width, column);
            }
        }
        (index, u16::try_from(column).unwrap_or(u16::MAX))
    }

    /// The cursor at a row and display column — how a click and a vertical move
    /// both land somewhere sensible.
    #[must_use]
    pub fn cursor_at(&self, row: usize, column: u16, lines: &[String], tab_width: usize) -> Cursor {
        let row = self.rows[row.min(self.rows.len() - 1)];
        let target = usize::from(column).saturating_sub(usize::from(row.indent));
        let mut col = row.start;
        let mut at = usize::from(row.indent);

        if let Some(text) = lines.get(row.line) {
            for ch in text.chars().take(row.end).skip(row.start) {
                let width = char_width(ch, tab_width, at);
                // Land on whichever character the column falls closest to, so
                // clicking the right half of a wide glyph lands after it.
                if at + width > usize::from(row.indent) + target {
                    break;
                }
                at += width;
                col += 1;
            }
        }
        Cursor {
            line: row.line,
            col,
        }
    }
}

/// Breaks one line into rows at word boundaries.
fn wrap_line(chars: &[char], line: usize, width: usize, tab_width: usize, out: &mut Vec<Row>) {
    if chars.is_empty() {
        out.push(Row {
            line,
            start: 0,
            end: 0,
            indent: 0,
            first: true,
        });
        return;
    }

    let hanging = hanging_indent(chars, tab_width, width);
    let mut start = 0;
    let mut first = true;

    while start < chars.len() {
        let indent = if first { 0 } else { hanging };
        let available = width.saturating_sub(usize::from(indent)).max(1);

        let mut column = 0;
        let mut at = start;
        // One past the last space that fits, which is where the line would
        // rather break.
        let mut boundary = None;
        while at < chars.len() {
            let next = column + char_width(chars[at], tab_width, column);
            if next > available && at > start {
                break;
            }
            column = next;
            at += 1;
            if chars[at - 1] == ' ' {
                boundary = Some(at);
            }
        }

        // Breaking after the space keeps it on this row, where it is invisible,
        // instead of indenting the next one by a stray blank.
        let end = match boundary {
            Some(boundary) if at < chars.len() && boundary > start => boundary,
            _ => at,
        };

        out.push(Row {
            line,
            start,
            end,
            indent,
            first,
        });
        start = end;
        first = false;
    }
}

/// Columns a wrapped line's continuations are indented by.
///
/// A wrapped bullet reads as one item when its second row starts under the
/// first row's text, and as two items when it starts under the bullet.
fn hanging_indent(chars: &[char], tab_width: usize, width: usize) -> u16 {
    let mut column = 0;
    let mut at = 0;
    while at < chars.len() && (chars[at] == ' ' || chars[at] == '\t') {
        column += char_width(chars[at], tab_width, column);
        at += 1;
    }
    // The marker itself. Every marker is ASCII, so its length in bytes, in
    // characters and in columns are all the same number.
    let rest: String = chars[at..].iter().collect();
    column += marker_len(&rest).unwrap_or(0);

    if width.saturating_sub(column) < MIN_WRAP {
        return 0;
    }
    u16::try_from(column).unwrap_or(0)
}

/// What kind of edit was last applied, used to group undo steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Insert,
    Delete,
    Structural,
}

#[derive(Debug, Clone)]
struct Snapshot {
    lines: Vec<String>,
    cursor: Cursor,
}

pub struct Editor {
    lines: Vec<String>,
    cursor: Cursor,
    /// Display column the cursor wants when moving between rows, so travelling
    /// through a short row and back out preserves the original column.
    ///
    /// Cleared by every other kind of movement, and by editing, so it only ever
    /// holds a column the user actually aimed at.
    desired_col: Option<u16>,
    selection_anchor: Option<Cursor>,
    /// Rows scrolled off the top — terminal rows, not source lines, since one
    /// long line can occupy several of them.
    pub scroll: usize,
    /// Columns scrolled off the left. Only ever non-zero with wrapping off,
    /// where reaching the end of a long line is the whole point.
    pub hscroll: usize,
    modified: bool,
    /// Bumped on every change, so a caller can tell whether one happened.
    revision: u64,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: Option<EditKind>,
    tab_width: usize,
    expand_tabs: bool,
}

/// Undo history depth. Deep enough to recover from a bad paste, bounded so a
/// long session can't grow without limit.
const MAX_UNDO: usize = 500;

impl Editor {
    #[must_use]
    pub fn new(text: &str, tab_width: usize, expand_tabs: bool) -> Self {
        Self {
            lines: split_lines(text),
            cursor: Cursor::default(),
            desired_col: None,
            selection_anchor: None,
            scroll: 0,
            hscroll: 0,
            modified: false,
            revision: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: None,
            tab_width,
            expand_tabs,
        }
    }

    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// How this buffer falls across the rows of a viewport `width` columns wide.
    #[must_use]
    pub fn layout(&self, width: usize, wrap: bool) -> Layout {
        Layout::build(&self.lines, width, wrap, self.tab_width)
    }

    /// The row and display column the caret should be drawn at.
    #[must_use]
    pub fn caret(&self, layout: &Layout) -> (usize, u16) {
        layout.position_of(self.cursor, &self.lines, self.tab_width)
    }

    #[must_use]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    #[must_use]
    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    #[must_use]
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn mark_saved(&mut self) {
        self.modified = false;
    }

    /// Records that the text changed.
    ///
    /// `modified` answers "is there unsaved work", which stays true once set;
    /// `revision` answers "did *that* keystroke change anything", which needs a
    /// number that moves every time. Vim's `.` uses the second to tell a
    /// command worth repeating from a motion that merely moved the cursor.
    fn touch(&mut self) {
        self.modified = true;
        self.revision = self.revision.wrapping_add(1);
    }

    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The buffer as text, always newline-terminated.
    ///
    /// Notes are text files and POSIX text files end with a newline; making it
    /// unconditional keeps diffs clean when a note is edited both here and in
    /// Obsidian.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = self.lines.join("\n");
        out.push('\n');
        out
    }

    /// The current selection as an ordered pair, if any.
    #[must_use]
    pub fn selection(&self) -> Option<(Cursor, Cursor)> {
        let anchor = self.selection_anchor?;
        if anchor == self.cursor {
            return None;
        }
        Some(if anchor <= self.cursor {
            (anchor, self.cursor)
        } else {
            (self.cursor, anchor)
        })
    }

    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        Some(self.slice(start, end))
    }

    pub fn begin_selection(&mut self) {
        self.selection_anchor = Some(self.cursor);
    }

    pub fn select_all(&mut self) {
        self.selection_anchor = Some(Cursor::default());
        self.cursor = self.end_position();
    }

    // ---- movement --------------------------------------------------------

    pub fn move_left(&mut self, extend: bool) {
        self.prepare_move(extend);
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        } else if self.cursor.line > 0 {
            self.cursor.line -= 1;
            self.cursor.col = self.line_len(self.cursor.line);
        }
    }

    pub fn move_right(&mut self, extend: bool) {
        self.prepare_move(extend);
        if self.cursor.col < self.line_len(self.cursor.line) {
            self.cursor.col += 1;
        } else if self.cursor.line + 1 < self.lines.len() {
            self.cursor.line += 1;
            self.cursor.col = 0;
        }
    }

    /// Moves `delta` terminal rows, following the text as it was wrapped.
    ///
    /// Moving by wrapped row rather than by source line is what makes the arrow
    /// keys agree with the screen: on a paragraph that fills four rows, four
    /// presses of `Down` cross it, as they would anywhere else.
    pub fn move_row(&mut self, layout: &Layout, delta: isize, extend: bool) {
        // Read the aimed-for column before `prepare_move` forgets it.
        let (row, column) = layout.position_of(self.cursor, &self.lines, self.tab_width);
        let want = self.desired_col.unwrap_or(column);
        self.prepare_move(extend);

        let last = layout.rows().len().saturating_sub(1) as isize;
        let target = (row as isize + delta).clamp(0, last) as usize;
        self.cursor = layout.cursor_at(target, want, &self.lines, self.tab_width);
        self.desired_col = Some(want);
    }

    /// `Home`: the start of the row on screen.
    ///
    /// On the first row of a line this keeps the behavior every editor settled
    /// on for indented text — the first non-blank, then column zero — because
    /// that is where the interesting positions are. On a wrapped continuation
    /// there is only one sensible answer: where the row begins.
    pub fn move_row_start(&mut self, layout: &Layout, extend: bool) {
        let row = layout.rows()[layout.row_of(self.cursor)];
        if !row.first {
            self.prepare_move(extend);
            self.cursor.col = row.start;
            return;
        }
        self.move_line_start(extend);
    }

    /// `End`: the end of the row on screen, which on the last row of a line is
    /// the end of the line.
    pub fn move_row_end(&mut self, layout: &Layout, extend: bool) {
        let row = layout.rows()[layout.row_of(self.cursor)];
        if row.end == self.line_len(row.line) {
            self.move_line_end(extend);
            return;
        }
        self.prepare_move(extend);
        self.cursor.col = row.end;
    }

    /// Places the cursor at a row and display column, for a click or a drag.
    pub fn goto_visual(&mut self, layout: &Layout, row: usize, column: u16, extend: bool) {
        self.prepare_move(extend);
        self.cursor = layout.cursor_at(row, column, &self.lines, self.tab_width);
    }

    pub fn move_line_start(&mut self, extend: bool) {
        self.prepare_move(extend);
        let indent = self.lines[self.cursor.line]
            .chars()
            .take_while(|c| c.is_whitespace())
            .count();
        self.cursor.col = if self.cursor.col == indent { 0 } else { indent };
    }

    pub fn move_line_end(&mut self, extend: bool) {
        self.prepare_move(extend);
        self.cursor.col = self.line_len(self.cursor.line);
    }

    pub fn move_word_left(&mut self, extend: bool) {
        self.prepare_move(extend);
        if self.cursor.col == 0 {
            if self.cursor.line > 0 {
                self.cursor.line -= 1;
                self.cursor.col = self.line_len(self.cursor.line);
            }
        } else {
            let chars: Vec<char> = self.lines[self.cursor.line].chars().collect();
            let mut col = self.cursor.col;
            while col > 0 && !chars[col - 1].is_alphanumeric() {
                col -= 1;
            }
            while col > 0 && chars[col - 1].is_alphanumeric() {
                col -= 1;
            }
            self.cursor.col = col;
        }
    }

    pub fn move_word_right(&mut self, extend: bool) {
        self.prepare_move(extend);
        let chars: Vec<char> = self.lines[self.cursor.line].chars().collect();
        if self.cursor.col >= chars.len() {
            if self.cursor.line + 1 < self.lines.len() {
                self.cursor.line += 1;
                self.cursor.col = 0;
            }
        } else {
            let mut col = self.cursor.col;
            while col < chars.len() && chars[col].is_alphanumeric() {
                col += 1;
            }
            while col < chars.len() && !chars[col].is_alphanumeric() {
                col += 1;
            }
            self.cursor.col = col;
        }
    }

    /// Moves `delta` **source lines**, which is what vim's `j` and `k` do.
    ///
    /// Distinct from [`Editor::move_row`], which follows the text as it was
    /// wrapped: on a paragraph filling four rows, this crosses it in one press
    /// and `move_row` takes four. Vim binds both — `j` here, `gj` there — so
    /// the two coexist rather than one replacing the other.
    ///
    /// The aimed-for column is kept in display columns, like `move_row`, so
    /// travelling through a short line and back out lands where it started.
    /// `usize::MAX` means "the end of whatever line you land on", which is how
    /// `$` then `j` stays at the end — vim's `curswant`.
    pub fn move_line(&mut self, delta: isize, extend: bool) {
        let want = self
            .desired_col
            .unwrap_or_else(|| u16::try_from(self.display_col(self.cursor)).unwrap_or(u16::MAX));
        self.prepare_move(extend);

        let last = self.lines.len().saturating_sub(1) as isize;
        self.cursor.line = (self.cursor.line as isize + delta).clamp(0, last) as usize;
        self.cursor.col = self.col_at_display(self.cursor.line, want);
        self.desired_col = Some(want);
    }

    /// Sticks the cursor to the end of the line, so a following `j` stays there.
    pub fn move_line_end_sticky(&mut self, extend: bool) {
        self.move_line_end(extend);
        self.desired_col = Some(u16::MAX);
    }

    /// `^`: the first character that isn't blank.
    pub fn move_first_nonblank(&mut self, extend: bool) {
        self.prepare_move(extend);
        self.cursor.col = self.lines[self.cursor.line]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .count()
            .min(self.line_len(self.cursor.line));
    }

    /// Display column a cursor sits at, ignoring wrapping.
    fn display_col(&self, cursor: Cursor) -> usize {
        let mut column = 0;
        if let Some(text) = self.lines.get(cursor.line) {
            for ch in text.chars().take(cursor.col) {
                column += char_width(ch, self.tab_width, column);
            }
        }
        column
    }

    /// The character offset nearest a display column on `line`.
    fn col_at_display(&self, line: usize, want: u16) -> usize {
        let len = self.line_len(line);
        if want == u16::MAX {
            return len;
        }
        let target = usize::from(want);
        let mut column = 0;
        let mut col = 0;
        if let Some(text) = self.lines.get(line) {
            for ch in text.chars() {
                let width = char_width(ch, self.tab_width, column);
                if column + width > target {
                    break;
                }
                column += width;
                col += 1;
            }
        }
        col.min(len)
    }

    /// Jumps to a position a motion worked out, clamped into the buffer.
    ///
    /// Unlike [`Editor::goto`] this keeps the selection, so the same call
    /// serves Normal mode and a visual-mode motion that is extending one.
    pub fn set_cursor(&mut self, at: Cursor) {
        let line = at.line.min(self.lines.len().saturating_sub(1));
        self.cursor = Cursor {
            line,
            col: at.col.min(self.line_len(line)),
        };
        self.desired_col = None;
    }

    /// The end of the word `from` sits in, without stepping into the next one.
    ///
    /// `ge` needs this: it walks back a word and then wants that word's end,
    /// where [`Editor::word_end`] would skip on to the following one.
    #[must_use]
    pub fn word_end_from(&self, from: Cursor, big: bool) -> Cursor {
        let mut at = from;
        let kind = self.char_at(at).map_or(0, |ch| class(ch, big));
        while let Some(next) = self.next_pos(at) {
            if self.char_at(next).map_or(0, |ch| class(ch, big)) != kind {
                break;
            }
            at = next;
        }
        at
    }

    /// Places the cursor without disturbing the selection, for visual mode.
    pub fn goto_extend(&mut self, line: usize, col: usize) {
        let anchor = self.selection_anchor;
        self.goto(line, col);
        self.selection_anchor = anchor;
    }

    /// Pulls the cursor back onto a character.
    ///
    /// Vim's Normal mode cursor sits *on* a character rather than between two,
    /// so it can never rest one past the end of a line the way an insertion
    /// caret does. Called whenever a command finishes in Normal mode.
    pub fn clamp_normal(&mut self) {
        let len = self.line_len(self.cursor.line);
        self.cursor.col = self.cursor.col.min(len.saturating_sub(1));
    }

    pub fn move_document_start(&mut self, extend: bool) {
        self.prepare_move(extend);
        self.cursor = Cursor::default();
    }

    pub fn move_document_end(&mut self, extend: bool) {
        self.prepare_move(extend);
        self.cursor = self.end_position();
    }

    /// Places the cursor at a specific position, clamped into the buffer.
    pub fn goto(&mut self, line: usize, col: usize) {
        self.cursor.line = line.min(self.lines.len().saturating_sub(1));
        self.cursor.col = col.min(self.line_len(self.cursor.line));
        self.desired_col = None;
        self.selection_anchor = None;
    }

    fn prepare_move(&mut self, extend: bool) {
        // Any move that isn't between rows abandons the column the cursor was
        // aiming for; only `move_row` puts one back.
        self.desired_col = None;
        if extend {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
        } else {
            self.selection_anchor = None;
        }
    }

    // ---- editing ---------------------------------------------------------

    pub fn insert_char(&mut self, ch: char) {
        self.push_undo(EditKind::Insert);
        self.delete_selection_inner();

        if ch == '\t' && self.expand_tabs {
            let spaces = self.tab_width - (self.cursor.col % self.tab_width);
            let text = " ".repeat(spaces);
            self.insert_into_line(&text);
            return;
        }
        self.insert_into_line(&ch.to_string());
    }

    pub fn insert_str(&mut self, text: &str) {
        self.push_undo(EditKind::Insert);
        self.delete_selection_inner();
        for (i, part) in text.split('\n').enumerate() {
            if i > 0 {
                self.split_line();
            }
            if !part.is_empty() {
                self.insert_into_line(part.trim_end_matches('\r'));
            }
        }
    }

    /// `Enter`. Carries indentation, and continues a list.
    ///
    /// Continuing the list is what makes a terminal usable for notes at all:
    /// typing a checklist otherwise means retyping `- [ ] ` on every line.
    /// Pressing it on an item with nothing in it ends the list instead, which is
    /// how you stop — the same rule Obsidian uses.
    pub fn newline(&mut self) {
        self.push_undo(EditKind::Structural);
        self.delete_selection_inner();

        let line = self.lines[self.cursor.line].clone();
        let indent: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let marker = marker(&line[indent.len()..]);

        // An item with only a marker on it: clear the line rather than adding
        // another empty one below.
        if let Some(marker) = &marker
            && line[indent.len() + marker.len..].trim().is_empty()
        {
            self.lines[self.cursor.line] = String::new();
            self.cursor.col = 0;
            self.desired_col = None;
            self.touch();
            return;
        }

        self.split_line();
        let carry = match &marker {
            Some(marker) => format!("{indent}{}", marker.next),
            None => indent,
        };
        if !carry.is_empty() {
            self.insert_into_line(&carry);
        }
    }

    /// `Tab` and `Shift+Tab`.
    ///
    /// Inside a list item, or with several lines selected, `Tab` nests rather
    /// than inserting a tab character — in a list that is the only thing it
    /// could reasonably mean. Anywhere else it is still a tab.
    pub fn tab(&mut self, forward: bool) {
        if !forward {
            self.indent(false);
            return;
        }
        let line = &self.lines[self.cursor.line];
        let in_list = marker(line.trim_start()).is_some();
        if in_list || self.selection().is_some() {
            self.indent(true);
        } else {
            self.insert_char('\t');
        }
    }

    /// Indents or outdents every line the cursor or the selection touches.
    pub fn indent(&mut self, forward: bool) {
        self.push_undo(EditKind::Structural);
        let (start, end) = self
            .selection()
            .map_or((self.cursor, self.cursor), |(s, e)| (s, e));

        let step = if self.expand_tabs {
            " ".repeat(self.tab_width)
        } else {
            "\t".to_string()
        };
        let mut moved = 0;
        for line in start.line..=end.line.min(self.lines.len() - 1) {
            if forward {
                self.lines[line].insert_str(0, &step);
                moved = step.chars().count();
                continue;
            }
            // Outdenting takes back a whole stop, or whatever less is there.
            let removable = self.lines[line]
                .chars()
                .take(step.chars().count())
                .take_while(|c| *c == ' ' || *c == '\t')
                .count();
            self.lines[line] = self.lines[line].chars().skip(removable).collect();
            if line == self.cursor.line {
                moved = removable;
            }
        }

        self.cursor.col = if forward {
            self.cursor.col + moved
        } else {
            self.cursor.col.saturating_sub(moved)
        };
        self.desired_col = None;
        self.selection_anchor = None;
        self.touch();
    }

    pub fn backspace(&mut self) {
        if self.selection().is_some() {
            self.push_undo(EditKind::Delete);
            self.delete_selection_inner();
            return;
        }
        if self.cursor.line == 0 && self.cursor.col == 0 {
            return;
        }
        self.push_undo(EditKind::Delete);

        if self.cursor.col > 0 {
            let chars: Vec<char> = self.lines[self.cursor.line].chars().collect();
            // Deleting through soft-tab indentation removes the whole stop.
            let mut remove = 1;
            if self.expand_tabs
                && chars[..self.cursor.col].iter().all(|c| *c == ' ')
                && self.cursor.col.is_multiple_of(self.tab_width)
            {
                remove = self.tab_width.min(self.cursor.col);
            }
            let start = self.cursor.col - remove;
            let kept: String = chars[..start]
                .iter()
                .chain(chars[self.cursor.col..].iter())
                .collect();
            self.lines[self.cursor.line] = kept;
            self.cursor.col = start;
        } else {
            let current = self.lines.remove(self.cursor.line);
            self.cursor.line -= 1;
            self.cursor.col = self.line_len(self.cursor.line);
            self.lines[self.cursor.line].push_str(&current);
        }
        self.desired_col = None;
        self.touch();
    }

    pub fn delete_forward(&mut self) {
        if self.selection().is_some() {
            self.push_undo(EditKind::Delete);
            self.delete_selection_inner();
            return;
        }
        let len = self.line_len(self.cursor.line);
        if self.cursor.col == len && self.cursor.line + 1 >= self.lines.len() {
            return;
        }
        self.push_undo(EditKind::Delete);

        if self.cursor.col < len {
            let chars: Vec<char> = self.lines[self.cursor.line].chars().collect();
            let kept: String = chars[..self.cursor.col]
                .iter()
                .chain(chars[self.cursor.col + 1..].iter())
                .collect();
            self.lines[self.cursor.line] = kept;
        } else {
            let next = self.lines.remove(self.cursor.line + 1);
            self.lines[self.cursor.line].push_str(&next);
        }
        self.touch();
    }

    /// Deletes the current line, or every line the selection touches.
    pub fn delete_line(&mut self) {
        self.push_undo(EditKind::Structural);
        let (start, end) = self
            .selection()
            .map_or((self.cursor, self.cursor), |(s, e)| (s, e));

        let first = start.line;
        let last = end.line.min(self.lines.len() - 1);
        self.lines.drain(first..=last);
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor.line = first.min(self.lines.len() - 1);
        self.cursor.col = 0;
        self.selection_anchor = None;
        self.touch();
    }

    /// `x`: removes characters at the cursor and hands them back.
    ///
    /// Stops at the end of the line rather than pulling the next one up, which
    /// is what vim does and what keeps `5x` on a three-character line from
    /// eating the line break.
    #[must_use]
    pub fn take_chars(&mut self, count: usize) -> String {
        let len = self.line_len(self.cursor.line);
        if self.cursor.col >= len {
            return String::new();
        }
        self.push_undo(EditKind::Delete);

        let chars: Vec<char> = self.lines[self.cursor.line].chars().collect();
        let end = (self.cursor.col + count.max(1)).min(len);
        let taken: String = chars[self.cursor.col..end].iter().collect();
        let kept: String = chars[..self.cursor.col]
            .iter()
            .chain(chars[end..].iter())
            .collect();
        self.lines[self.cursor.line] = kept;
        self.desired_col = None;
        self.touch();
        taken
    }

    /// `r`: overwrites the characters under the cursor, staying in place.
    ///
    /// Refuses when the count runs past the end of the line, exactly as vim
    /// does — a partial replace would be worse than none.
    pub fn replace_char(&mut self, ch: char, count: usize) {
        let len = self.line_len(self.cursor.line);
        let count = count.max(1);
        if self.cursor.col + count > len {
            return;
        }
        self.push_undo(EditKind::Structural);

        let chars: Vec<char> = self.lines[self.cursor.line].chars().collect();
        let mut line: String = chars[..self.cursor.col].iter().collect();
        for _ in 0..count {
            line.push(ch);
        }
        line.extend(chars[self.cursor.col + count..].iter());
        self.lines[self.cursor.line] = line;
        // Vim leaves the cursor on the last character replaced.
        self.cursor.col += count - 1;
        self.desired_col = None;
        self.touch();
    }

    /// Characters on a line, for callers outside this module.
    #[must_use]
    pub fn line_len_at(&self, line: usize) -> usize {
        self.line_len(line)
    }

    /// `dd`: removes whole lines and hands them back to be yanked.
    ///
    /// Distinct from [`Editor::delete_line`], which discards them. Vim's delete
    /// always fills the register, so the text has to come back out.
    #[must_use]
    pub fn take_lines(&mut self, first: usize, count: usize) -> String {
        self.push_undo(EditKind::Structural);
        let first = first.min(self.lines.len().saturating_sub(1));
        let last = (first + count.max(1) - 1).min(self.lines.len() - 1);

        let taken: Vec<String> = self.lines.drain(first..=last).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor.line = first.min(self.lines.len() - 1);
        self.cursor.col = 0;
        self.desired_col = None;
        self.selection_anchor = None;
        self.touch();

        let mut text = taken.join("\n");
        text.push('\n');
        text
    }

    /// `yy`: the lines themselves, left where they are.
    #[must_use]
    pub fn copy_lines(&self, first: usize, count: usize) -> String {
        let first = first.min(self.lines.len().saturating_sub(1));
        let last = (first + count.max(1) - 1).min(self.lines.len() - 1);
        let mut text = self.lines[first..=last].join("\n");
        text.push('\n');
        text
    }

    /// `p` and `P`.
    ///
    /// Linewise text goes on its own line below or above and takes the cursor
    /// with it; charwise text goes after or before the cursor, which is the
    /// distinction that makes `yy`/`p` duplicate a line rather than splice it
    /// into the middle of one.
    pub fn put(&mut self, text: &str, linewise: bool, after: bool) {
        if text.is_empty() {
            return;
        }
        self.push_undo(EditKind::Structural);

        if !linewise {
            if after && self.cursor.col < self.line_len(self.cursor.line) {
                self.cursor.col += 1;
            }
            let start = self.cursor;
            for (i, part) in text.split('\n').enumerate() {
                if i > 0 {
                    self.split_line();
                }
                if !part.is_empty() {
                    self.insert_into_line(part);
                }
            }
            // Vim leaves the cursor on the last character pasted, not past it.
            if start.line == self.cursor.line && self.cursor.col > start.col {
                self.cursor.col -= 1;
            }
            return;
        }

        let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        // A linewise register always ends in a newline, so the split leaves an
        // empty tail that is not a line.
        if lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        if lines.is_empty() {
            return;
        }

        let at = if after {
            self.cursor.line + 1
        } else {
            self.cursor.line
        };
        let at = at.min(self.lines.len());
        for (offset, line) in lines.drain(..).enumerate() {
            self.lines.insert(at + offset, line);
        }
        self.cursor.line = at;
        self.cursor.col = 0;
        self.desired_col = None;
        self.selection_anchor = None;
        self.touch();
    }

    /// `o` and `O`: a blank line below or above, cursor on it.
    ///
    /// Carries indentation and continues a list, exactly as `Enter` does — the
    /// two keys mean the same thing and would be baffling if they disagreed.
    pub fn open_line(&mut self, above: bool) {
        if above {
            self.push_undo(EditKind::Structural);
            self.lines.insert(self.cursor.line, String::new());
            self.cursor.col = 0;
            self.desired_col = None;
            self.touch();
            // Indentation comes from the line that was pushed down, since
            // there may be nothing above to copy.
            let indent: String = self.lines[self.cursor.line + 1]
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            if !indent.is_empty() {
                self.insert_into_line(&indent);
            }
            return;
        }
        self.move_line_end(false);
        self.newline();
    }

    pub fn delete_selection(&mut self) {
        if self.selection().is_some() {
            self.push_undo(EditKind::Delete);
            self.delete_selection_inner();
        }
    }

    /// Wraps the selection (or the word at the cursor) in a marker, or removes
    /// it when already present — how `Ctrl+B` behaves in Obsidian.
    pub fn toggle_wrap(&mut self, marker: &str) {
        self.push_undo(EditKind::Structural);
        let Some((start, end)) = self.selection() else {
            // With no selection, insert the pair and place the cursor inside.
            self.insert_into_line(&format!("{marker}{marker}"));
            self.cursor.col -= marker.chars().count();
            self.touch();
            return;
        };

        let text = self.slice(start, end);
        let unwrapped = text
            .strip_prefix(marker)
            .and_then(|t| t.strip_suffix(marker));

        let replacement = match unwrapped {
            Some(inner) => inner.to_string(),
            None => format!("{marker}{text}{marker}"),
        };

        self.delete_selection_inner();
        for (i, part) in replacement.split('\n').enumerate() {
            if i > 0 {
                self.split_line();
            }
            self.insert_into_line(part);
        }
        self.touch();
    }

    fn delete_selection_inner(&mut self) {
        let Some((start, end)) = self.selection() else {
            return;
        };

        let start_chars: Vec<char> = self.lines[start.line].chars().collect();
        let end_chars: Vec<char> = self.lines[end.line].chars().collect();
        let head: String = start_chars[..start.col.min(start_chars.len())]
            .iter()
            .collect();
        let tail: String = end_chars[end.col.min(end_chars.len())..].iter().collect();

        self.lines.drain(start.line..=end.line);
        self.lines.insert(start.line, format!("{head}{tail}"));

        self.cursor = start;
        self.desired_col = None;
        self.selection_anchor = None;
        self.touch();
    }

    fn insert_into_line(&mut self, text: &str) {
        let chars: Vec<char> = self.lines[self.cursor.line].chars().collect();
        let col = self.cursor.col.min(chars.len());
        let mut line: String = chars[..col].iter().collect();
        line.push_str(text);
        line.extend(chars[col..].iter());
        self.lines[self.cursor.line] = line;
        self.cursor.col = col + text.chars().count();
        self.desired_col = None;
        self.touch();
    }

    fn split_line(&mut self) {
        let chars: Vec<char> = self.lines[self.cursor.line].chars().collect();
        let col = self.cursor.col.min(chars.len());
        let head: String = chars[..col].iter().collect();
        let tail: String = chars[col..].iter().collect();
        self.lines[self.cursor.line] = head;
        self.lines.insert(self.cursor.line + 1, tail);
        self.cursor.line += 1;
        self.cursor.col = 0;
        self.desired_col = None;
        self.touch();
    }

    // ---- undo ------------------------------------------------------------

    /// Records a snapshot, coalescing runs of the same kind of edit.
    ///
    /// Undo should step back by a *thought*, not a keystroke, so consecutive
    /// typing collapses into one entry while a delete after typing starts a new
    /// one.
    fn push_undo(&mut self, kind: EditKind) {
        let coalesce = self.last_edit == Some(kind) && kind != EditKind::Structural;
        self.last_edit = Some(kind);
        self.redo.clear();
        if coalesce {
            return;
        }
        self.undo.push(Snapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
        });
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }

    /// Ends the current undo group, so the next edit starts a new one.
    pub fn commit(&mut self) {
        self.last_edit = None;
    }

    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        self.redo.push(Snapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
        });
        self.lines = snapshot.lines;
        self.cursor = snapshot.cursor;
        self.selection_anchor = None;
        self.last_edit = None;
        self.touch();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop() else {
            return false;
        };
        self.undo.push(Snapshot {
            lines: self.lines.clone(),
            cursor: self.cursor,
        });
        self.lines = snapshot.lines;
        self.cursor = snapshot.cursor;
        self.selection_anchor = None;
        self.last_edit = None;
        self.touch();
        true
    }

    // ---- vim motions -----------------------------------------------------

    /// The character at a position, with `\n` standing for the end of a line.
    ///
    /// Treating the line break as a character is what lets the word motions be
    /// written once and still cross lines: a newline is blank, so `w` at the end
    /// of a line walks onto the next one without a special case.
    #[must_use]
    fn char_at(&self, at: Cursor) -> Option<char> {
        let line = self.lines.get(at.line)?;
        match line.chars().nth(at.col) {
            Some(ch) => Some(ch),
            None if at.line + 1 < self.lines.len() => Some('\n'),
            None => None,
        }
    }

    /// The next position, walking off the end of a line onto the next.
    fn next_pos(&self, at: Cursor) -> Option<Cursor> {
        if at.col < self.line_len(at.line) {
            return Some(Cursor {
                line: at.line,
                col: at.col + 1,
            });
        }
        (at.line + 1 < self.lines.len()).then(|| Cursor {
            line: at.line + 1,
            col: 0,
        })
    }

    /// The previous position, walking back onto the end of the line above.
    fn prev_pos(&self, at: Cursor) -> Option<Cursor> {
        if at.col > 0 {
            return Some(Cursor {
                line: at.line,
                col: at.col - 1,
            });
        }
        at.line.checked_sub(1).map(|line| Cursor {
            line,
            col: self.line_len(line),
        })
    }

    /// `w` and `W`: the start of the next word.
    #[must_use]
    pub fn word_forward(&self, from: Cursor, count: usize, big: bool) -> Cursor {
        let mut at = from;
        for _ in 0..count.max(1) {
            // Step off whatever the cursor is on, then over anything of the
            // same kind, then over the blanks that follow it.
            let start = self.char_at(at).map_or(0, |ch| class(ch, big));
            while let Some(next) = self.next_pos(at) {
                if self.char_at(at).map_or(0, |ch| class(ch, big)) != start {
                    break;
                }
                at = next;
            }
            while let Some(next) = self.next_pos(at) {
                if self.char_at(at).is_some_and(|ch| class(ch, big) != 0) {
                    break;
                }
                at = next;
            }
        }
        at
    }

    /// `b` and `B`: the start of the word before.
    #[must_use]
    pub fn word_back(&self, from: Cursor, count: usize, big: bool) -> Cursor {
        let mut at = from;
        for _ in 0..count.max(1) {
            let Some(prev) = self.prev_pos(at) else { break };
            at = prev;
            // Back over the blanks, then to the front of the word landed in.
            while self.char_at(at).is_some_and(|ch| class(ch, big) == 0) {
                match self.prev_pos(at) {
                    Some(prev) => at = prev,
                    None => break,
                }
            }
            let kind = self.char_at(at).map_or(0, |ch| class(ch, big));
            while let Some(prev) = self.prev_pos(at) {
                if self.char_at(prev).map_or(0, |ch| class(ch, big)) != kind {
                    break;
                }
                at = prev;
            }
        }
        at
    }

    /// `e` and `E`: the last character of the current or next word.
    #[must_use]
    pub fn word_end(&self, from: Cursor, count: usize, big: bool) -> Cursor {
        let mut at = from;
        for _ in 0..count.max(1) {
            let Some(next) = self.next_pos(at) else { break };
            at = next;
            while self.char_at(at).is_some_and(|ch| class(ch, big) == 0) {
                match self.next_pos(at) {
                    Some(next) => at = next,
                    None => break,
                }
            }
            let kind = self.char_at(at).map_or(0, |ch| class(ch, big));
            while let Some(next) = self.next_pos(at) {
                if self.char_at(next).map_or(0, |ch| class(ch, big)) != kind {
                    break;
                }
                at = next;
            }
        }
        at
    }

    /// `{` and `}`: the blank line before or after this block of text.
    #[must_use]
    pub fn paragraph(&self, from: Cursor, forward: bool, count: usize) -> Cursor {
        let blank = |line: usize| {
            self.lines
                .get(line)
                .is_some_and(|text| text.trim().is_empty())
        };
        let mut line = from.line;
        for _ in 0..count.max(1) {
            if forward {
                line += 1;
                while line < self.lines.len() && blank(line) {
                    line += 1;
                }
                while line < self.lines.len() && !blank(line) {
                    line += 1;
                }
                line = line.min(self.lines.len() - 1);
            } else {
                line = line.saturating_sub(1);
                while line > 0 && blank(line) {
                    line -= 1;
                }
                while line > 0 && !blank(line) {
                    line -= 1;
                }
            }
        }
        Cursor { line, col: 0 }
    }

    /// `f`, `F`, `t` and `T`: a character on this line.
    ///
    /// Stays on one line, as vim does — that is the whole point of it as a
    /// motion you can aim by eye.
    #[must_use]
    pub fn find_in_line(
        &self,
        from: Cursor,
        target: char,
        forward: bool,
        till: bool,
        count: usize,
    ) -> Option<Cursor> {
        let chars: Vec<char> = self.lines.get(from.line)?.chars().collect();
        let mut col = from.col;
        for _ in 0..count.max(1) {
            if forward {
                col = (col + 1..chars.len()).find(|&i| chars[i] == target)?;
            } else {
                col = (0..col).rev().find(|&i| chars[i] == target)?;
            }
        }
        // `t` stops one short of the target; `T` stops one after it.
        let col = if till {
            if forward {
                col.checked_sub(1)?
            } else {
                col + 1
            }
        } else {
            col
        };
        Some(Cursor {
            line: from.line,
            col,
        })
    }

    /// The next occurrence of `pattern`, wrapping round the ends of the note.
    ///
    /// A plain substring rather than a regular expression: notes are prose, the
    /// thing being looked for is almost always a word, and a half-supported
    /// regex dialect would be worse than an honest literal one. Case is
    /// ignored unless the pattern has a capital in it, which is vim's
    /// `smartcase` and what people expect without knowing its name.
    #[must_use]
    pub fn find_next(&self, from: Cursor, pattern: &str, forward: bool) -> Option<Cursor> {
        if pattern.is_empty() {
            return None;
        }
        let sensitive = pattern.chars().any(char::is_uppercase);
        let needle = if sensitive {
            pattern.to_string()
        } else {
            pattern.to_lowercase()
        };

        let hay = |line: usize| {
            let text = &self.lines[line];
            if sensitive {
                text.clone()
            } else {
                text.to_lowercase()
            }
        };
        // Byte offsets from `match_indices` have to come back as characters, or
        // a match after an accent lands the cursor mid-glyph.
        let as_chars = |line: usize, byte: usize| self.lines[line][..byte].chars().count();

        let count = self.lines.len();
        for step in 0..=count {
            let line = if forward {
                (from.line + step) % count
            } else {
                (from.line + count - step % count) % count
            };
            let text = hay(line);

            let found = if forward {
                let after = if step == 0 { from.col + 1 } else { 0 };
                let start = text
                    .char_indices()
                    .nth(after)
                    .map_or(text.len(), |(byte, _)| byte);
                text.get(start..)
                    .and_then(|rest| rest.find(&needle).map(|at| at + start))
            } else {
                let before = if step == 0 {
                    text.char_indices()
                        .nth(from.col)
                        .map_or(text.len(), |(byte, _)| byte)
                } else {
                    text.len()
                };
                text.get(..before).and_then(|head| head.rfind(&needle))
            };

            if let Some(byte) = found {
                return Some(Cursor {
                    line,
                    col: as_chars(line, byte),
                });
            }
        }
        None
    }

    /// Every match of `pattern` on one line, as character ranges.
    ///
    /// For the renderer: showing only the match jumped to leaves the other
    /// hits invisible, which is most of what a search is for.
    #[must_use]
    pub fn matches_on(&self, line: usize, pattern: &str) -> Vec<(usize, usize)> {
        if pattern.is_empty() {
            return Vec::new();
        }
        let Some(text) = self.lines.get(line) else {
            return Vec::new();
        };
        let sensitive = pattern.chars().any(char::is_uppercase);
        let (hay, needle) = if sensitive {
            (text.clone(), pattern.to_string())
        } else {
            (text.to_lowercase(), pattern.to_lowercase())
        };
        let width = needle.chars().count();

        hay.match_indices(&needle)
            .map(|(byte, _)| {
                let start = text[..byte].chars().count();
                (start, start + width)
            })
            .collect()
    }

    // ---- text objects ----------------------------------------------------

    /// `iw` and `aw`, as an inclusive character range.
    #[must_use]
    pub fn word_object(&self, at: Cursor, around: bool, big: bool) -> Option<(Cursor, Cursor)> {
        let chars: Vec<char> = self.lines.get(at.line)?.chars().collect();
        if chars.is_empty() {
            return None;
        }
        let col = at.col.min(chars.len() - 1);
        let kind = class(chars[col], big);

        let mut start = col;
        while start > 0 && class(chars[start - 1], big) == kind {
            start -= 1;
        }
        let mut end = col;
        while end + 1 < chars.len() && class(chars[end + 1], big) == kind {
            end += 1;
        }
        // `aw` takes the trailing blanks too, falling back to the leading ones
        // at the end of a line — which is how `daw` on the last word of a line
        // doesn't leave a dangling space behind.
        if around {
            let was = end;
            while end + 1 < chars.len() && class(chars[end + 1], big) == 0 {
                end += 1;
            }
            if end == was {
                while start > 0 && class(chars[start - 1], big) == 0 {
                    start -= 1;
                }
            }
        }
        Some((
            Cursor {
                line: at.line,
                col: start,
            },
            Cursor {
                line: at.line,
                col: end,
            },
        ))
    }

    /// `i"` / `a"` and friends, on the cursor's line.
    #[must_use]
    pub fn quoted_object(&self, at: Cursor, quote: char, around: bool) -> Option<(Cursor, Cursor)> {
        let chars: Vec<char> = self.lines.get(at.line)?.chars().collect();
        // Quotes have no nesting to track, so the pair the cursor is inside is
        // found by counting them from the start of the line.
        let positions: Vec<usize> = chars
            .iter()
            .enumerate()
            .filter(|(_, ch)| **ch == quote)
            .map(|(i, _)| i)
            .collect();
        if positions.len() < 2 {
            return None;
        }
        // The pair the cursor is inside, or — as vim does — the next one along
        // the line, so `ci"` works from the start of the line rather than only
        // from between the quotes.
        let (open, close) = positions
            .chunks(2)
            .filter(|pair| pair.len() == 2)
            .map(|pair| (pair[0], pair[1]))
            .find(|(open, close)| at.col <= *close && at.col >= *open)
            .or_else(|| {
                positions
                    .chunks(2)
                    .filter(|pair| pair.len() == 2)
                    .map(|pair| (pair[0], pair[1]))
                    .find(|(open, _)| *open >= at.col)
            })?;

        let (start, end) = if around {
            (open, close)
        } else {
            if close == open + 1 {
                return None;
            }
            (open + 1, close - 1)
        };
        Some((
            Cursor {
                line: at.line,
                col: start,
            },
            Cursor {
                line: at.line,
                col: end,
            },
        ))
    }

    /// `i(` / `a(` and friends, counting nesting so an inner pair wins.
    #[must_use]
    pub fn bracket_object(
        &self,
        at: Cursor,
        open: char,
        close: char,
        around: bool,
    ) -> Option<(Cursor, Cursor)> {
        let chars: Vec<char> = self.lines.get(at.line)?.chars().collect();

        // Outwards from the cursor in both directions, so a cursor inside
        // `f(g(x))` takes the pair it is actually in.
        let mut depth = 0i32;
        let mut start = None;
        for i in (0..=at.col.min(chars.len().saturating_sub(1))).rev() {
            if chars[i] == close && i != at.col {
                depth += 1;
            } else if chars[i] == open {
                if depth == 0 {
                    start = Some(i);
                    break;
                }
                depth -= 1;
            }
        }
        let start = start?;

        depth = 0;
        let mut end = None;
        for (i, ch) in chars.iter().enumerate().skip(start + 1) {
            if *ch == open {
                depth += 1;
            } else if *ch == close {
                if depth == 0 {
                    end = Some(i);
                    break;
                }
                depth -= 1;
            }
        }
        let end = end?;

        let (start, end) = if around {
            (start, end)
        } else {
            if end == start + 1 {
                return None;
            }
            (start + 1, end - 1)
        };
        Some((
            Cursor {
                line: at.line,
                col: start,
            },
            Cursor {
                line: at.line,
                col: end,
            },
        ))
    }

    // ---- range operations ------------------------------------------------

    /// Removes the text between two positions and returns it.
    ///
    /// `end` is exclusive, matching how a selection is held; callers wanting
    /// vim's inclusive motions add the character themselves.
    pub fn delete_range(&mut self, start: Cursor, end: Cursor) -> String {
        if start >= end {
            return String::new();
        }
        let text = self.slice(start, end);
        self.push_undo(EditKind::Delete);
        self.cursor = start;
        self.selection_anchor = Some(end);
        self.delete_selection_inner();
        text
    }

    /// The same range, left where it is.
    #[must_use]
    pub fn copy_range(&self, start: Cursor, end: Cursor) -> String {
        if start >= end {
            return String::new();
        }
        self.slice(start, end)
    }

    /// `J`: pulls the following line up onto this one.
    ///
    /// Vim leaves exactly one space at the join and none before a closing
    /// bracket, which is the difference between joining prose and mangling it.
    pub fn join_lines(&mut self, count: usize) {
        let joins = count.max(2) - 1;
        self.push_undo(EditKind::Structural);
        for _ in 0..joins {
            if self.cursor.line + 1 >= self.lines.len() {
                break;
            }
            let next = self.lines.remove(self.cursor.line + 1);
            let trimmed = next.trim_start();
            let current = self.lines[self.cursor.line].trim_end().to_string();
            let separator = if current.is_empty() || trimmed.is_empty() || trimmed.starts_with(')')
            {
                ""
            } else {
                " "
            };
            self.cursor.col = current.chars().count();
            self.lines[self.cursor.line] = format!("{current}{separator}{trimmed}");
        }
        self.desired_col = None;
        self.touch();
    }

    /// `~`: flips the case of the characters under the cursor and moves past.
    pub fn toggle_case(&mut self, count: usize) {
        let len = self.line_len(self.cursor.line);
        if self.cursor.col >= len {
            return;
        }
        self.push_undo(EditKind::Structural);
        let end = (self.cursor.col + count.max(1)).min(len);
        let flipped: String = self.lines[self.cursor.line]
            .chars()
            .enumerate()
            .map(|(i, ch)| {
                if i < self.cursor.col || i >= end {
                    ch
                } else if ch.is_uppercase() {
                    ch.to_lowercase().next().unwrap_or(ch)
                } else {
                    ch.to_uppercase().next().unwrap_or(ch)
                }
            })
            .collect();
        self.lines[self.cursor.line] = flipped;
        self.cursor.col = end.min(len.saturating_sub(1));
        self.desired_col = None;
        self.touch();
    }

    /// `Ctrl+A` and `Ctrl+X`: adds to the number at or after the cursor.
    ///
    /// Returns whether there was one. Handles a leading `-`, so decrementing
    /// past zero goes negative rather than mangling the digits.
    pub fn adjust_number(&mut self, delta: i64) -> bool {
        let chars: Vec<char> = self.lines[self.cursor.line].chars().collect();
        // The number under the cursor, else the next one along the line.
        let Some(digit) = (self.cursor.col..chars.len())
            .find(|&i| chars[i].is_ascii_digit())
            .or_else(|| {
                (0..chars.len())
                    .find(|&i| chars[i].is_ascii_digit())
                    .filter(|&i| i >= self.cursor.col)
            })
        else {
            return false;
        };

        let mut start = digit;
        while start > 0 && chars[start - 1].is_ascii_digit() {
            start -= 1;
        }
        let mut end = digit;
        while end + 1 < chars.len() && chars[end + 1].is_ascii_digit() {
            end += 1;
        }
        let negative = start > 0 && chars[start - 1] == '-';
        let text: String = chars[start..=end].iter().collect();
        let Ok(value) = text.parse::<i64>() else {
            return false;
        };

        self.push_undo(EditKind::Structural);
        let from = if negative { start - 1 } else { start };
        let updated = if negative { -value } else { value } + delta;
        let head: String = chars[..from].iter().collect();
        let tail: String = chars[end + 1..].iter().collect();
        let body = updated.to_string();
        self.cursor.col = head.chars().count() + body.chars().count() - 1;
        self.lines[self.cursor.line] = format!("{head}{body}{tail}");
        self.desired_col = None;
        self.touch();
        true
    }

    // ---- helpers ---------------------------------------------------------

    fn line_len(&self, line: usize) -> usize {
        self.lines.get(line).map_or(0, |l| l.chars().count())
    }

    fn end_position(&self) -> Cursor {
        let line = self.lines.len().saturating_sub(1);
        Cursor {
            line,
            col: self.line_len(line),
        }
    }

    fn slice(&self, start: Cursor, end: Cursor) -> String {
        if start.line == end.line {
            return self.lines[start.line]
                .chars()
                .skip(start.col)
                .take(end.col.saturating_sub(start.col))
                .collect();
        }
        let mut out: String = self.lines[start.line].chars().skip(start.col).collect();
        for line in &self.lines[start.line + 1..end.line] {
            out.push('\n');
            out.push_str(line);
        }
        out.push('\n');
        out.extend(self.lines[end.line].chars().take(end.col));
        out
    }

    /// Scrolls so the cursor is visible in a viewport `height` rows tall.
    ///
    /// Rows, not lines: with wrapping on, a paragraph the cursor is halfway
    /// through may be taller than the viewport on its own.
    pub fn scroll_into_view(&mut self, layout: &Layout, height: usize) {
        if height == 0 {
            return;
        }
        let (row, column) = self.caret(layout);
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + height {
            self.scroll = row + 1 - height;
        }
        // Never leave blank rows below a note that would fit further up.
        self.scroll = self.scroll.min(layout.rows().len().saturating_sub(height));

        // Panning sideways only happens with wrapping off; a wrapped row always
        // fits the viewport it was wrapped to.
        if layout.wrapped() || layout.width() == 0 {
            self.hscroll = 0;
            return;
        }
        let column = usize::from(column);
        if column < self.hscroll {
            self.hscroll = column;
        } else if column >= self.hscroll + layout.width() {
            self.hscroll = column + 1 - layout.width();
        }
    }
}

/// A list marker at the start of a line, and what continues it below.
struct Continuation {
    /// Bytes of the marker, including its trailing space.
    len: usize,
    /// What the next line should start with after the same indentation.
    next: String,
}

/// Reads the list marker at the start of `rest`, which must already have its
/// indentation stripped.
///
/// One place knows what a marker looks like, and three things ask it: how far
/// to indent a wrapped row, what `Enter` should carry down, and whether `Tab`
/// means "nest this item" or "insert a tab".
fn marker(rest: &str) -> Option<Continuation> {
    let bytes = rest.as_bytes();
    let bullet = matches!(bytes.first(), Some(b'-' | b'*' | b'+')) && bytes.get(1) == Some(&b' ');

    // A task box, checked or not, always continues as an empty one: carrying
    // "done" down to a line nobody has done yet would be a lie.
    if bullet
        && bytes.get(2) == Some(&b'[')
        && matches!(bytes.get(3), Some(b' ' | b'x' | b'X'))
        && bytes.get(4) == Some(&b']')
        && bytes.get(5) == Some(&b' ')
    {
        return Some(Continuation {
            len: 6,
            next: format!("{} [ ] ", &rest[..1]),
        });
    }
    if bullet {
        return Some(Continuation {
            len: 2,
            next: rest[..2].to_string(),
        });
    }

    // `1. ` or `1) `, continuing with the next number.
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits > 0
        && matches!(bytes.get(digits), Some(b'.' | b')'))
        && bytes.get(digits + 1) == Some(&b' ')
    {
        let number: u64 = rest[..digits].parse().unwrap_or(0);
        return Some(Continuation {
            len: digits + 2,
            next: format!("{}{} ", number + 1, &rest[digits..=digits]),
        });
    }

    // Every level of a `> > ` quote, repeated verbatim.
    let quote = rest
        .bytes()
        .take_while(|byte| matches!(byte, b'>' | b' '))
        .count();
    if quote > 0 && rest.starts_with('>') {
        return Some(Continuation {
            len: quote,
            next: rest[..quote].to_string(),
        });
    }

    None
}

/// Bytes of the list marker at the start of `rest`, if any.
fn marker_len(rest: &str) -> Option<usize> {
    marker(rest).map(|marker| marker.len)
}

fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect();
    // A trailing newline produces an empty final element that isn't a real line.
    if lines.len() > 1 && lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(text: &str) -> Editor {
        Editor::new(text, 4, true)
    }

    #[test]
    fn round_trips_text_with_a_trailing_newline() {
        assert_eq!(editor("a\nb\n").text(), "a\nb\n");
        assert_eq!(editor("a\nb").text(), "a\nb\n", "a newline is added");
        assert_eq!(editor("").text(), "\n");
    }

    #[test]
    fn typing_inserts_and_marks_modified() {
        let mut ed = editor("");
        assert!(!ed.is_modified());
        for ch in "hello".chars() {
            ed.insert_char(ch);
        }
        assert_eq!(ed.text(), "hello\n");
        assert!(ed.is_modified());
        assert_eq!(ed.cursor(), Cursor { line: 0, col: 5 });
    }

    #[test]
    fn newline_carries_indentation() {
        let mut ed = editor("    indented");
        ed.move_line_end(false);
        ed.newline();
        ed.insert_char('x');
        assert_eq!(ed.text(), "    indented\n    x\n");
    }

    #[test]
    fn tab_expands_to_the_next_tab_stop() {
        let mut ed = editor("ab");
        ed.move_line_end(false);
        ed.insert_char('\t');
        assert_eq!(ed.text(), "ab  \n", "2 spaces reach column 4");
    }

    #[test]
    fn backspace_removes_a_whole_soft_tab() {
        let mut ed = editor("    x");
        ed.goto(0, 4);
        ed.backspace();
        assert_eq!(ed.text(), "x\n");
    }

    #[test]
    fn backspace_at_line_start_joins_lines() {
        let mut ed = editor("ab\ncd");
        ed.goto(1, 0);
        ed.backspace();
        assert_eq!(ed.text(), "abcd\n");
        assert_eq!(ed.cursor(), Cursor { line: 0, col: 2 });
    }

    #[test]
    fn delete_forward_joins_the_next_line() {
        let mut ed = editor("ab\ncd");
        ed.move_line_end(false);
        ed.delete_forward();
        assert_eq!(ed.text(), "abcd\n");
    }

    /// A layout wide enough that nothing wraps, so a row is a line.
    fn unwrapped(ed: &Editor) -> Layout {
        ed.layout(200, true)
    }

    #[test]
    fn vertical_movement_remembers_the_desired_column() {
        let mut ed = editor("longer line\nx\nanother long line");
        let layout = unwrapped(&ed);
        ed.goto(0, 9);
        ed.move_row(&layout, 1, false);
        assert_eq!(ed.cursor().col, 1, "clamped to the short line");
        ed.move_row(&layout, 1, false);
        assert_eq!(ed.cursor().col, 9, "restored on the longer line");
    }

    #[test]
    fn typing_forgets_the_column_a_vertical_move_was_aiming_for() {
        // Otherwise the cursor jumps back to a column the user left behind.
        let mut ed = editor("longer line\nx\nanother long line");
        let layout = unwrapped(&ed);
        ed.goto(0, 9);
        ed.move_row(&layout, 1, false);
        ed.insert_char('!');

        let layout = unwrapped(&ed);
        ed.move_row(&layout, 1, false);
        assert_eq!(
            ed.cursor().col,
            2,
            "the column comes from where typing left it"
        );
    }

    #[test]
    fn home_toggles_between_indent_and_column_zero() {
        let mut ed = editor("    text");
        ed.move_line_end(false);
        ed.move_line_start(false);
        assert_eq!(ed.cursor().col, 4, "first stop is the indent");
        ed.move_line_start(false);
        assert_eq!(ed.cursor().col, 0);
    }

    #[test]
    fn word_movement_crosses_words_and_lines() {
        let mut ed = editor("alpha beta\ngamma");
        ed.move_word_right(false);
        assert_eq!(ed.cursor().col, 6, "lands at the start of the next word");
        ed.move_word_right(false);
        assert_eq!(ed.cursor().col, 10);
        ed.move_word_right(false);
        assert_eq!(ed.cursor(), Cursor { line: 1, col: 0 });
    }

    #[test]
    fn selection_spans_lines_and_deletes_cleanly() {
        let mut ed = editor("one\ntwo\nthree");
        ed.goto(0, 1);
        ed.begin_selection();
        ed.goto_extend(2, 2);
        assert_eq!(ed.selected_text().as_deref(), Some("ne\ntwo\nth"));

        ed.delete_selection();
        assert_eq!(ed.text(), "oree\n");
        assert_eq!(ed.cursor(), Cursor { line: 0, col: 1 });
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut ed = editor("hello world");
        ed.goto(0, 0);
        ed.begin_selection();
        ed.goto_extend(0, 5);
        ed.insert_char('X');
        assert_eq!(ed.text(), "X world\n");
    }

    #[test]
    fn toggle_wrap_adds_and_removes_markers() {
        let mut ed = editor("bold me");
        ed.goto(0, 0);
        ed.begin_selection();
        ed.goto_extend(0, 4);
        ed.toggle_wrap("**");
        assert_eq!(ed.text(), "**bold** me\n");

        // Re-selecting the wrapped text removes the markers again.
        ed.goto(0, 0);
        ed.begin_selection();
        ed.goto_extend(0, 8);
        ed.toggle_wrap("**");
        assert_eq!(ed.text(), "bold me\n");
    }

    #[test]
    fn delete_line_removes_the_whole_line() {
        let mut ed = editor("one\ntwo\nthree");
        ed.goto(1, 1);
        ed.delete_line();
        assert_eq!(ed.text(), "one\nthree\n");
        assert_eq!(ed.cursor().line, 1);
    }

    #[test]
    fn deleting_the_last_line_leaves_an_empty_buffer() {
        let mut ed = editor("only");
        ed.delete_line();
        assert_eq!(ed.text(), "\n");
        assert_eq!(ed.line_count(), 1);
    }

    #[test]
    fn undo_groups_a_run_of_typing() {
        let mut ed = editor("");
        for ch in "hello".chars() {
            ed.insert_char(ch);
        }
        assert!(ed.undo());
        assert_eq!(ed.text(), "\n", "one undo removes the whole run");
        assert!(ed.redo());
        assert_eq!(ed.text(), "hello\n");
    }

    #[test]
    fn undo_separates_typing_from_deleting() {
        let mut ed = editor("");
        ed.insert_str("word");
        ed.backspace();
        assert_eq!(ed.text(), "wor\n");

        ed.undo();
        assert_eq!(ed.text(), "word\n", "the delete is its own step");
        ed.undo();
        assert_eq!(ed.text(), "\n");
    }

    #[test]
    fn commit_forces_a_new_undo_group() {
        let mut ed = editor("");
        ed.insert_char('a');
        ed.commit();
        ed.insert_char('b');
        ed.undo();
        assert_eq!(ed.text(), "a\n");
    }

    #[test]
    fn undo_on_an_empty_history_is_a_no_op() {
        let mut ed = editor("text");
        assert!(!ed.undo());
        assert!(!ed.redo());
        assert_eq!(ed.text(), "text\n");
    }

    #[test]
    fn a_new_edit_clears_the_redo_stack() {
        let mut ed = editor("");
        ed.insert_str("one");
        ed.undo();
        ed.insert_str("two");
        assert!(!ed.redo(), "redo is invalidated by a divergent edit");
        assert_eq!(ed.text(), "two\n");
    }

    #[test]
    fn multi_byte_characters_are_handled_by_character_not_byte() {
        let mut ed = editor("héllo → wörld");
        ed.move_line_end(false);
        assert_eq!(ed.cursor().col, 13);

        ed.goto(0, 6);
        ed.insert_char('!');
        assert_eq!(ed.text(), "héllo !→ wörld\n");

        ed.goto(0, 1);
        ed.delete_forward();
        assert_eq!(ed.text(), "hllo !→ wörld\n");
    }

    #[test]
    fn insert_str_handles_multi_line_paste() {
        let mut ed = editor("start");
        ed.move_line_end(false);
        ed.insert_str("\nmiddle\nend");
        assert_eq!(ed.text(), "start\nmiddle\nend\n");
    }

    #[test]
    fn scroll_follows_the_cursor_both_ways() {
        let text: String = (0..100).map(|i| format!("line {i}\n")).collect();
        let mut ed = editor(&text);
        let layout = unwrapped(&ed);

        ed.goto(50, 0);
        ed.scroll_into_view(&layout, 10);
        assert_eq!(ed.scroll, 41, "cursor sits on the last visible row");

        ed.goto(5, 0);
        ed.scroll_into_view(&layout, 10);
        assert_eq!(ed.scroll, 5);
    }

    #[test]
    fn scroll_counts_wrapped_rows_not_lines() {
        // Three lines that each take four rows: the cursor on the last one is 12
        // rows down, not 3, and scrolling by lines would leave it off screen.
        let mut ed = editor("aaa bbb ccc ddd\neee fff ggg hhh\niii jjj kkk lll");
        let layout = ed.layout(4, true);
        assert_eq!(layout.rows().len(), 12);

        ed.goto(2, 15);
        ed.scroll_into_view(&layout, 5);
        let (row, _) = ed.caret(&layout);
        assert!(
            row >= ed.scroll && row < ed.scroll + 5,
            "row {row} is off a viewport starting at {}",
            ed.scroll
        );
    }

    #[test]
    fn a_short_note_never_scrolls_past_its_own_end() {
        let mut ed = editor("one\ntwo\nthree");
        let layout = unwrapped(&ed);
        ed.scroll = 99;
        ed.goto(0, 0);
        ed.scroll_into_view(&layout, 10);
        assert_eq!(ed.scroll, 0, "no blank rows above a note that fits");
    }

    #[test]
    fn wrapping_off_pans_sideways_to_reach_the_end_of_a_long_line() {
        let mut ed = editor(&"x".repeat(200));
        let layout = ed.layout(40, false);
        assert_eq!(layout.rows().len(), 1, "one row, however long the line");

        ed.move_line_end(false);
        ed.scroll_into_view(&layout, 10);
        assert_eq!(ed.hscroll, 161, "the caret is at the right edge");
    }

    #[test]
    fn a_long_line_wraps_at_word_boundaries() {
        let ed = editor("the quick brown fox jumps");
        let layout = ed.layout(10, true);
        let rows: Vec<&str> = layout
            .rows()
            .iter()
            .map(|row| &ed.lines()[row.line][row.start..row.end])
            .collect();

        assert_eq!(rows, vec!["the quick ", "brown fox ", "jumps"]);
        for row in layout.rows() {
            assert!(row.end - row.start <= 10, "no row is wider than the pane");
        }
    }

    #[test]
    fn a_word_longer_than_the_pane_is_broken_rather_than_lost() {
        let ed = editor("supercalifragilistic");
        let layout = ed.layout(8, true);
        assert!(layout.rows().len() > 1);
        assert_eq!(
            layout
                .rows()
                .iter()
                .map(|row| row.end - row.start)
                .sum::<usize>(),
            20,
            "every character is on some row"
        );
    }

    #[test]
    fn a_wrapped_list_item_lines_up_under_its_own_text() {
        let ed = editor("- alpha beta gamma delta epsilon");
        let layout = ed.layout(20, true);
        let rows = layout.rows();

        assert!(rows.len() > 1, "should wrap");
        assert_eq!(rows[0].indent, 0);
        assert_eq!(rows[1].indent, 2, "indented past the bullet, not under it");
    }

    #[test]
    fn the_caret_and_a_click_agree_on_where_a_character_is() {
        // Wide characters occupy two columns, so a column is not a character.
        let mut ed = editor("héllo 日本語 world text here");
        let layout = ed.layout(12, true);

        for col in 0..ed.lines()[0].chars().count() {
            ed.goto(0, col);
            let (row, column) = ed.caret(&layout);
            let back = layout.cursor_at(row, column, ed.lines(), 4);
            assert_eq!(
                back,
                Cursor { line: 0, col },
                "column {column} on row {row} should map back to character {col}"
            );
        }
    }

    #[test]
    fn moving_down_a_wrapped_paragraph_walks_it_row_by_row() {
        let mut ed = editor("aaa bbb ccc ddd eee fff");
        let layout = ed.layout(8, true);
        ed.goto(0, 0);

        let mut rows = vec![ed.caret(&layout).0];
        for _ in 0..2 {
            ed.move_row(&layout, 1, false);
            rows.push(ed.caret(&layout).0);
        }
        assert_eq!(rows, vec![0, 1, 2], "one press, one row");
    }

    #[test]
    fn home_and_end_work_on_the_row_not_the_line() {
        let mut ed = editor("aaa bbb ccc ddd");
        let layout = ed.layout(8, true);
        // Second row: "ccc ddd".
        ed.goto(0, 10);

        ed.move_row_end(&layout, false);
        assert_eq!(ed.cursor().col, 15);
        ed.move_row_start(&layout, false);
        assert_eq!(ed.cursor().col, 8, "the start of what is on screen");
    }

    #[test]
    fn enter_continues_a_list_and_an_empty_item_ends_it() {
        let mut ed = editor("- first");
        ed.move_line_end(false);
        ed.newline();
        assert_eq!(ed.text(), "- first\n- \n");

        ed.insert_char('x');
        ed.newline();
        assert_eq!(ed.text(), "- first\n- x\n- \n");

        // Nothing typed on the new item: Enter ends the list.
        ed.newline();
        assert_eq!(ed.text(), "- first\n- x\n\n");
    }

    #[test]
    fn enter_numbers_the_next_item_and_leaves_a_task_unchecked() {
        let mut ed = editor("3. third");
        ed.move_line_end(false);
        ed.newline();
        assert_eq!(ed.text(), "3. third\n4. \n");

        let mut ed = editor("  - [x] done");
        ed.move_line_end(false);
        ed.newline();
        assert_eq!(
            ed.text(),
            "  - [x] done\n  - [ ] \n",
            "indentation carries and the box starts empty"
        );
    }

    #[test]
    fn enter_carries_a_quote_and_splits_text_mid_item() {
        let mut ed = editor("> quoted");
        ed.move_line_end(false);
        ed.newline();
        assert_eq!(ed.text(), "> quoted\n> \n");

        let mut ed = editor("- alphabeta");
        ed.goto(0, 7);
        ed.newline();
        assert_eq!(ed.text(), "- alpha\n- beta\n");
    }

    #[test]
    fn tab_nests_a_list_item_but_is_still_a_tab_in_prose() {
        let mut ed = editor("- item");
        ed.move_line_end(false);
        ed.tab(true);
        assert_eq!(ed.text(), "    - item\n");
        ed.tab(false);
        assert_eq!(ed.text(), "- item\n");

        let mut ed = editor("prose");
        ed.move_line_end(false);
        ed.tab(true);
        assert_eq!(ed.text(), "prose   \n", "a tab to the next stop");
    }

    #[test]
    fn tab_indents_every_line_of_a_selection() {
        let mut ed = editor("one\ntwo\nthree");
        ed.goto(0, 0);
        ed.begin_selection();
        ed.goto_extend(1, 1);
        ed.tab(true);
        assert_eq!(ed.text(), "    one\n    two\nthree\n");
    }

    #[test]
    fn outdenting_takes_back_only_what_is_there() {
        let mut ed = editor("  two spaces");
        ed.tab(false);
        assert_eq!(ed.text(), "two spaces\n");
        ed.tab(false);
        assert_eq!(ed.text(), "two spaces\n", "and stops at the margin");
    }

    #[test]
    fn select_all_covers_the_buffer() {
        let mut ed = editor("a\nb\nc");
        ed.select_all();
        assert_eq!(ed.selected_text().as_deref(), Some("a\nb\nc"));
    }
}
