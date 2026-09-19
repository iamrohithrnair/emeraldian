//! Bidirectional text: the Unicode Bidirectional Algorithm, UAX #9.
//!
//! A terminal is a grid of cells filled left to right. Text that reads
//! right-to-left therefore has to be *reordered by the application* before it
//! is handed over — unlike a browser, where `direction: rtl` does it for free.
//! This module is that reordering, and it is the only place in the project that
//! knows the algorithm, so the reading pane, the editor, the sidebars and the
//! input fields all agree about where a character is.
//!
//! Two orders matter and are deliberately never conflated:
//!
//! * **Logical** — the order the characters are stored in, the order they were
//!   typed. Everything that is not drawing (the buffer, selection, undo, search,
//!   every vim motion) stays in logical order and never learns bidi exists.
//! * **Visual** — the order they appear on screen, left to right. Produced here,
//!   consumed only by the drawing code and the caret.
//!
//! The split is what keeps the change small: [`Shaped`] is the translation
//! between them, and it is built per *display row*, after wrapping.

pub mod arabic;

use std::ops::Range;

use unicode_bidi::{BidiClass, Level, ParagraphBidiInfo, bidi_class};
use unicode_width::UnicodeWidthChar;

/// A resolved direction. Never "auto" — that is [`DirectionMode`]'s job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

impl Direction {
    #[must_use]
    pub fn is_rtl(self) -> bool {
        self == Self::Rtl
    }

    fn level(self) -> Level {
        match self {
            Self::Ltr => Level::ltr(),
            Self::Rtl => Level::rtl(),
        }
    }
}

/// A *declared* direction, as it appears in config, frontmatter or a toggle.
///
/// Distinct from [`Direction`] because "auto" is a real, and the default,
/// answer: it means each block decides for itself from its own first strong
/// character, which is what Obsidian does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DirectionMode {
    #[default]
    Auto,
    Ltr,
    Rtl,
}

impl DirectionMode {
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ltr => "ltr",
            Self::Rtl => "rtl",
        }
    }

    /// The next mode, for a key that cycles through them.
    #[must_use]
    pub fn cycle(self) -> Self {
        match self {
            Self::Auto => Self::Ltr,
            Self::Ltr => Self::Rtl,
            Self::Rtl => Self::Auto,
        }
    }
}

impl std::str::FromStr for DirectionMode {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "ltr" | "left-to-right" => Ok(Self::Ltr),
            "rtl" | "right-to-left" => Ok(Self::Rtl),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for DirectionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.key())
    }
}

/// How reordered text is handed to the terminal.
///
/// Terminals disagree about whose job Arabic shaping is, and getting it wrong
/// is visible: a terminal that shapes will join the letters itself, so it wants
/// the plain letters; one that does not needs the joined forms baked in. There
/// is no way to ask, so this is a setting, defaulting to the common case.
///
/// Emitting explicit bidi control characters is deliberately not an option:
/// ratatui drops every zero-width grapheme before it reaches the terminal
/// (`Buffer::set_stringn` filters on `width > 0`), and every bidi control is
/// zero-width, so they would never arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Emit {
    /// Runs in visual order, characters left in logical order inside each one.
    ///
    /// The default, because most terminals shape Arabic with HarfBuzz, and
    /// HarfBuzz both joins a run *and* lays it out right-to-left. Handing such
    /// a terminal a pre-reversed run makes it reverse the run a second time and
    /// join every letter to the wrong neighbour. Ordering only the runs leaves
    /// the terminal the job it is already doing.
    #[default]
    Runs,
    /// Plain characters in fully visual order, for a terminal that reorders
    /// nothing and shapes nothing.
    Reorder,
    /// Fully visual order with Arabic mapped to its joining forms, for a
    /// terminal that reorders but does not shape.
    Presentation,
}

impl Emit {
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Runs => "runs",
            Self::Reorder => "reorder",
            Self::Presentation => "presentation",
        }
    }
}

impl std::str::FromStr for Emit {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "runs" => Ok(Self::Runs),
            "reorder" => Ok(Self::Reorder),
            "presentation" | "shaped" => Ok(Self::Presentation),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for Emit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.key())
    }
}

/// Which side of a direction boundary a caret is on.
///
/// One logical position between an English and an Arabic word has two honest
/// screen columns — the right edge of the English and the right edge of the
/// Arabic are different places. Affinity is which one the user meant, decided
/// by how they got there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Affinity {
    /// The caret belongs to the character at the cursor.
    #[default]
    Leading,
    /// The caret belongs to the character before the cursor.
    Trailing,
}

/// UAX #9 rules P2 and P3: the direction of the first strong character.
///
/// `None` when there is no strong character at all — a line of digits, or of
/// punctuation — which is the case where a caller falls back to its own base.
#[must_use]
pub fn base_direction(text: &str) -> Option<Direction> {
    for ch in text.chars() {
        match bidi_class(ch) {
            BidiClass::L => return Some(Direction::Ltr),
            BidiClass::R | BidiClass::AL => return Some(Direction::Rtl),
            _ => {}
        }
    }
    None
}

/// Applies a declared mode to a run of text.
#[must_use]
pub fn resolve(text: &str, mode: DirectionMode, fallback: Direction) -> Direction {
    match mode {
        DirectionMode::Ltr => Direction::Ltr,
        DirectionMode::Rtl => Direction::Rtl,
        DirectionMode::Auto => base_direction(text).unwrap_or(fallback),
    }
}

/// Whether the text contains anything that needs reordering at all.
///
/// The fast path for an all-English vault, which is most of them.
#[must_use]
pub fn has_rtl(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            bidi_class(ch),
            BidiClass::R | BidiClass::AL | BidiClass::AN | BidiClass::RLI | BidiClass::RLE
        )
    })
}

/// Display width of a string in terminal columns.
///
/// The one true measurement. Counting characters instead is wrong for CJK,
/// emoji and combining marks, and was wrong in this codebase in 23 places
/// before this module existed.
#[must_use]
pub fn display_width(text: &str) -> usize {
    text.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// One drawable cell, in visual order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    /// Which logical characters this cell covers, as `(start, len)`, relative to
    /// the row. `len` is 1 for everything except an Arabic lam-alef ligature
    /// under [`Emit::Presentation`], which fuses two characters into one glyph.
    pub logical: (usize, usize),
    /// The logical character whose *glyph* is drawn here.
    ///
    /// The same as `logical.0` except under [`Emit::Runs`], where the terminal
    /// lays a run out right-to-left itself, so the glyph written into this cell
    /// is the one from the mirrored end of the run. Styling follows the glyph;
    /// the caret follows `logical`.
    pub draw: usize,
    pub width: u8,
    /// Whether the character reads right-to-left, which decides which edge of
    /// the cell a caret sitting "before" it is drawn on.
    pub rtl: bool,
}

/// One display row, translated between logical and visual order.
#[derive(Debug, Clone, Default)]
pub struct Shaped {
    cells: Vec<Cell>,
    /// Logical character index -> index into `cells`.
    l2c: Vec<usize>,
    base: Direction,
    columns: usize,
    identity: bool,
}

impl Shaped {
    /// A row that needs no reordering: the cells are the characters, in order.
    ///
    /// Callers check [`Shaped::is_identity`] and take their original code path,
    /// which is what makes an English note render byte-for-byte as it did
    /// before this module existed.
    #[must_use]
    pub fn identity(text: &str) -> Self {
        let mut cells = Vec::with_capacity(text.len());
        let mut l2c = Vec::with_capacity(text.len());
        let mut columns = 0;
        for (i, ch) in text.chars().enumerate() {
            l2c.push(cells.len());
            let width = ch.width().unwrap_or(0);
            cells.push(Cell {
                ch,
                logical: (i, 1),
                draw: i,
                width: u8::try_from(width).unwrap_or(1),
                rtl: false,
            });
            columns += width;
        }
        Self {
            cells,
            l2c,
            base: Direction::Ltr,
            columns,
            identity: true,
        }
    }

    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.identity
    }

    #[must_use]
    pub fn base(&self) -> Direction {
        self.base
    }

    /// Total display columns the row occupies.
    #[must_use]
    pub fn columns(&self) -> usize {
        self.columns
    }

    /// Number of logical characters in the row.
    #[must_use]
    pub fn len(&self) -> usize {
        self.l2c.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.l2c.is_empty()
    }

    /// The row's text in visual order, ready to be drawn.
    #[must_use]
    pub fn text(&self) -> String {
        self.cells.iter().map(|c| c.ch).collect()
    }

    /// Left edge of a cell, in columns from the start of the row.
    fn edge(&self, cell: usize) -> usize {
        self.cells[..cell]
            .iter()
            .map(|c| usize::from(c.width))
            .sum()
    }

    /// The display column a caret at `logical` is drawn at.
    ///
    /// A caret sits on the *leading* edge of the character it is on, and which
    /// edge that is depends on the character's direction: the left edge of an
    /// English letter, the right edge of an Arabic one.
    #[must_use]
    pub fn caret_column(&self, logical: usize, affinity: Affinity) -> usize {
        if self.cells.is_empty() {
            return 0;
        }
        // Past the last character: the end of the row, whichever end that is.
        if logical >= self.l2c.len() {
            return match self.base {
                Direction::Ltr => self.columns,
                Direction::Rtl => 0,
            };
        }
        let (index, trailing) = match affinity {
            Affinity::Trailing if logical > 0 => (self.l2c[logical - 1], true),
            _ => (self.l2c[logical], false),
        };
        let cell = self.cells[index];
        let left = self.edge(index);
        // `rtl ^ trailing`: on an RTL cell the caret's leading edge is the right
        // one, and asking for the trailing edge flips it back.
        if cell.rtl != trailing {
            left + usize::from(cell.width)
        } else {
            left
        }
    }

    /// Which logical character a click at `column` names.
    #[must_use]
    pub fn hit(&self, column: usize) -> (usize, Affinity) {
        if self.cells.is_empty() {
            return (0, Affinity::Leading);
        }
        let mut left = 0;
        for cell in &self.cells {
            let width = usize::from(cell.width);
            if column < left + width {
                let (start, len) = cell.logical;
                // Landing on the far half of a cell means the caret belongs
                // after it — and "after" is the left half when the cell is RTL.
                let far = column >= left + width.div_ceil(2);
                return if far != cell.rtl {
                    (start + len, Affinity::Trailing)
                } else {
                    (start, Affinity::Leading)
                };
            }
            left += width;
        }
        // Past the right edge. Which logical position that is comes from the
        // last *visual* cell, not from the base direction: a right-to-left
        // paragraph can still end with an English word.
        let last = self.cells[self.cells.len() - 1];
        let (start, len) = last.logical;
        if last.rtl {
            (start, Affinity::Leading)
        } else {
            (start + len, Affinity::Trailing)
        }
    }
}

/// One source line with its bidi levels resolved.
///
/// Resolving is done once per *source line* and reordering once per *display
/// row*, which is the order UAX #9 requires: rules X1 to I2 decide what
/// direction each character is and need the whole paragraph for context, while
/// rules L1 and L2 reorder and are applied per line after wrapping. Resolving
/// each wrapped row separately would judge a neutral run that straddles a wrap
/// point against the wrong context.
pub struct Resolved<'a> {
    text: &'a str,
    info: Option<ParagraphBidiInfo<'a>>,
    /// Byte offset of each character, plus the total length, so a character
    /// range converts to a byte range in constant time.
    offsets: Vec<usize>,
    base: Direction,
}

impl<'a> Resolved<'a> {
    /// Resolves `text` against a base direction.
    #[must_use]
    pub fn new(text: &'a str, base: Direction) -> Self {
        // The fast path: no right-to-left character means nothing can move, so
        // the algorithm is skipped entirely.
        let info = has_rtl(text).then(|| ParagraphBidiInfo::new(text, Some(base.level())));
        let mut offsets: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        offsets.push(text.len());
        Self {
            text,
            info,
            offsets,
            base,
        }
    }

    #[must_use]
    pub fn has_rtl(&self) -> bool {
        self.info.is_some()
    }

    #[must_use]
    pub fn base(&self) -> Direction {
        self.base
    }

    /// Number of characters in the line.
    #[must_use]
    pub fn len(&self) -> usize {
        self.offsets.len() - 1
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Reorders one display row, given as a character range.
    ///
    /// A character range rather than a byte range because that is what
    /// `editor::Row` stores and what a `Cursor` counts in.
    #[must_use]
    pub fn row(&self, chars: Range<usize>, emit: Emit) -> Shaped {
        let chars = chars.start.min(self.len())..chars.end.min(self.len());
        let bytes = self.offsets[chars.start]..self.offsets[chars.end];
        let slice = &self.text[bytes.clone()];

        let Some(info) = &self.info else {
            return Shaped::identity(slice);
        };

        // Levels come back for the whole line, indexed by character, with rule
        // L1 already applied for this row — so slice out the row's own.
        let levels = info.reordered_levels_per_char(bytes);
        let levels = &levels[chars.clone()];
        let v2l = ParagraphBidiInfo::reorder_visual(levels);

        let row: Vec<char> = slice.chars().collect();
        let cells = emit_cells(&row, &v2l, levels, emit);

        let mut l2c = vec![0usize; row.len()];
        let mut columns = 0;
        for (index, cell) in cells.iter().enumerate() {
            let (start, len) = cell.logical;
            for k in 0..len {
                if let Some(slot) = l2c.get_mut(start + k) {
                    *slot = index;
                }
            }
            columns += usize::from(cell.width);
        }

        Shaped {
            cells,
            l2c,
            base: self.base,
            columns,
            identity: false,
        }
    }
}

/// Builds the visual cells for one row. The only part that differs per [`Emit`].
fn emit_cells(row: &[char], v2l: &[usize], levels: &[Level], emit: Emit) -> Vec<Cell> {
    let shaped = match emit {
        // Runs leaves the joining to the terminal, which is the whole point of
        // it, so there is nothing to pre-shape.
        Emit::Runs | Emit::Reorder => None,
        Emit::Presentation => Some(arabic::shape(row)),
    };

    // Logical index -> which shaped glyph covers it, for Emit::Presentation.
    let glyph_of = shaped.as_ref().map(|glyphs| {
        let mut map = vec![0usize; row.len()];
        let mut at = 0;
        for (g, &(_, consumed)) in glyphs.iter().enumerate() {
            for k in 0..consumed {
                if let Some(slot) = map.get_mut(at + k) {
                    *slot = g;
                }
            }
            at += consumed;
        }
        map
    });

    let mut cells = Vec::with_capacity(row.len());
    let mut emitted = shaped.as_ref().map(|g| vec![false; g.len()]);

    for &logical in v2l {
        let rtl = levels[logical].is_rtl();
        let (ch, span) = match (&shaped, &glyph_of, &mut emitted) {
            (Some(glyphs), Some(map), Some(seen)) => {
                let g = map[logical];
                // A lam-alef ligature covers two characters; draw it once, at
                // whichever of them the visual order reaches first.
                if seen[g] {
                    continue;
                }
                seen[g] = true;
                let (glyph, consumed) = glyphs[g];
                // `logical` may be the second half of a fused pair; the cell
                // always names the first character it covers.
                let mut start = logical;
                while start > 0 && map[start - 1] == g {
                    start -= 1;
                }
                (glyph, (start, consumed))
            }
            _ => (row[logical], (logical, 1)),
        };

        // UAX #9 rule L4: a paired bracket is drawn as its mirror in a
        // right-to-left run, so `(` opens on the right.
        let ch = if rtl { mirror(ch) } else { ch };

        cells.push(Cell {
            ch,
            logical: span,
            draw: span.0,
            width: u8::try_from(ch.width().unwrap_or(0)).unwrap_or(1),
            rtl,
        });
    }

    if emit == Emit::Runs {
        unreverse_runs(&mut cells, levels);
    }
    cells
}

/// Puts each right-to-left *word* back into logical order.
///
/// The cells keep their visual positions, so the caret mapping is untouched;
/// only the glyph written into each cell changes.
///
/// The grouping is one word at a time rather than one bidi run at a time,
/// because that is the unit the terminal works on. A terminal that shapes
/// Arabic reverses the letters of each word it finds, and stops at the space:
/// it has no bidi algorithm, so it never moves whole words past each other.
/// Handing it a run un-reversed across the spaces would therefore come back
/// with every word spelled correctly and all of them in the wrong order.
fn unreverse_runs(cells: &mut [Cell], levels: &[Level]) {
    let letter = |cell: &Cell| {
        levels[cell.logical.0].is_rtl()
            && matches!(bidi_class(cell.ch), BidiClass::R | BidiClass::AL)
    };

    let mut start = 0;
    while start < cells.len() {
        if !letter(&cells[start]) {
            // A space, a digit or a Latin word: the terminal leaves it where it
            // is, so we do too.
            start += 1;
            continue;
        }
        let mut end = start;
        while end < cells.len() && letter(&cells[end]) {
            end += 1;
        }
        // The word is in visual order, so reading it backwards reads it the way
        // it was typed. Mirroring (rule L4) only touches neutrals, which are
        // never in here.
        let taken: Vec<(char, usize)> = cells[start..end]
            .iter()
            .rev()
            .map(|cell| (cell.ch, cell.draw))
            .collect();
        for (cell, (ch, draw)) in cells[start..end].iter_mut().zip(taken) {
            cell.ch = ch;
            cell.draw = draw;
        }
        start = end;
    }
}

/// UAX #9 rule L4 — the Bidi_Mirrored characters that turn up in prose.
///
/// The full property covers some 500 mathematical symbols; these are the ones
/// that appear in a note, and an unlisted character is simply left alone.
#[must_use]
pub fn mirror(ch: char) -> char {
    match ch {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        '≤' => '≥',
        '≥' => '≤',
        '⌈' => '⌉',
        '⌉' => '⌈',
        '⌊' => '⌋',
        '⌋' => '⌊',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visual(text: &str, base: Direction) -> String {
        Resolved::new(text, base)
            .row(0..text.chars().count(), Emit::Reorder)
            .text()
    }

    #[test]
    fn the_first_strong_character_picks_the_direction() {
        assert_eq!(base_direction("hello مرحبا"), Some(Direction::Ltr));
        assert_eq!(base_direction("مرحبا hello"), Some(Direction::Rtl));
        assert_eq!(base_direction("שלום"), Some(Direction::Rtl));
        // Digits and punctuation are not strong, so there is nothing to go on.
        assert_eq!(base_direction("123 !? …"), None);
        assert_eq!(base_direction(""), None);
    }

    #[test]
    fn an_rtl_run_reverses() {
        assert_eq!(visual("مرحبا", Direction::Rtl), "ابحرم");
        assert_eq!(visual("שלום", Direction::Rtl), "םולש");
    }

    #[test]
    fn an_embedded_number_keeps_its_own_order() {
        // The whole point of running the algorithm rather than reversing the
        // string: a naive reversal would render the year as "4202".
        let out = visual("مرحبا 2024 سنة", Direction::Rtl);
        assert!(out.contains("2024"), "digits must not reverse, got {out}");
        assert_eq!(out, "ةنس 2024 ابحرم");
    }

    #[test]
    fn an_embedded_latin_word_keeps_its_own_order() {
        let out = visual("مرحبا world بالعالم", Direction::Rtl);
        assert!(out.contains("world"), "latin must not reverse, got {out}");
    }

    #[test]
    fn an_ltr_line_is_left_exactly_as_it_was() {
        let text = "plain english, 123 — with punctuation";
        let shaped =
            Resolved::new(text, Direction::Ltr).row(0..text.chars().count(), Emit::Reorder);
        assert!(shaped.is_identity(), "no rtl means no work");
        assert_eq!(shaped.text(), text);
    }

    #[test]
    fn brackets_are_mirrored_in_an_rtl_run() {
        // Rule L4. Reordering alone would leave "] [", which reads backwards.
        let out = visual("[مرحبا]", Direction::Rtl);
        assert_eq!(out, "[ابحرم]");
    }

    #[test]
    fn runs_are_ordered_but_their_letters_are_left_alone() {
        // What a shaping terminal needs: the words in the right places, each
        // one still spelled forwards so its letters join correctly.
        let text = "مرحبا world بالعالم";
        let n = text.chars().count();
        let drawn = Resolved::new(text, Direction::Rtl)
            .row(0..n, Emit::Runs)
            .text();
        // Each Arabic word is still spelled forwards, so the terminal joins
        // its letters to the right neighbours — and the word that comes last
        // logically is handed over first, so the terminal's own right-to-left
        // pass lands it on the left.
        let last = drawn.find("بالعالم").expect("spelled forwards: {drawn:?}");
        let first = drawn.find("مرحبا").expect("spelled forwards: {drawn:?}");
        let middle = drawn.find("world").expect("latin is untouched");
        assert!(
            last < middle && middle < first,
            "runs should be in visual order: {drawn:?}"
        );
        // The spaces travel with the run they belong to, which is right: a
        // terminal folds a space into the adjacent Arabic run and reverses it
        // along with the letters, putting it back between the words.
        assert_eq!(drawn.chars().filter(|c| *c == ' ').count(), 2);
    }

    #[test]
    fn the_words_of_a_pure_rtl_line_are_swapped_but_each_is_spelled_forwards() {
        // The terminal reverses the letters of each word and stops at the
        // space, so the words have to arrive already swapped. Getting this
        // wrong is the difference between "spelled right, ordered wrong" and
        // correct.
        let text = "مرحبا بالعالم";
        let n = text.chars().count();
        let drawn = Resolved::new(text, Direction::Rtl)
            .row(0..n, Emit::Runs)
            .text();
        assert_eq!(drawn, "بالعالم مرحبا");
    }

    #[test]
    fn runs_keeps_the_caret_mapping_of_a_full_reorder() {
        // Only the glyph in each cell changes; where each character's caret
        // lives must not, because the terminal puts the glyphs back.
        let text = "مرحبا world بالعالم";
        let n = text.chars().count();
        let resolved = Resolved::new(text, Direction::Rtl);
        let a = resolved.row(0..n, Emit::Reorder);
        let b = resolved.row(0..n, Emit::Runs);
        for logical in 0..=n {
            assert_eq!(
                a.caret_column(logical, Affinity::Leading),
                b.caret_column(logical, Affinity::Leading),
                "character {logical} moved"
            );
        }
    }

    #[test]
    fn the_cell_map_covers_every_character_exactly_once() {
        for text in [
            "مرحبا بالعالم",
            "مرحبا world بالعالم",
            "مرحبا 2024 سنة",
            "hello שלום world",
            "plain english",
        ] {
            for emit in [Emit::Runs, Emit::Reorder, Emit::Presentation] {
                let n = text.chars().count();
                let shaped = Resolved::new(text, Direction::Rtl).row(0..n, emit);
                let mut covered = vec![0usize; n];
                for cell in shaped.cells() {
                    let (start, len) = cell.logical;
                    for k in 0..len {
                        covered[start + k] += 1;
                    }
                }
                assert!(
                    covered.iter().all(|&c| c == 1),
                    "{text:?} under {emit}:every character must be drawn once, got {covered:?}"
                );
                // And the map back agrees with the cells it points at.
                for (logical, &cell) in shaped.l2c.iter().enumerate() {
                    let (start, len) = shaped.cells[cell].logical;
                    assert!(logical >= start && logical < start + len);
                }
            }
        }
    }

    #[test]
    fn a_click_puts_the_caret_where_it_was_clicked() {
        // The property that matters on screen. Going the other way — from a
        // logical index to a column and back — is *not* a round trip at a
        // direction boundary, because one column there honestly names two
        // logical positions. That ambiguity is what `Affinity` exists for.
        for text in ["مرحبا world بالعالم", "مرحبا 2024 سنة", "hello שלום"]
        {
            let n = text.chars().count();
            let shaped = Resolved::new(text, Direction::Rtl).row(0..n, Emit::Reorder);
            for column in 0..=shaped.columns() {
                let (logical, affinity) = shaped.hit(column);
                assert_eq!(
                    shaped.caret_column(logical, affinity),
                    column,
                    "{text:?}: clicking column {column} must leave the caret there"
                );
            }
        }
    }

    #[test]
    fn every_character_has_a_caret_position_inside_the_row() {
        let text = "مرحبا world بالعالم";
        let n = text.chars().count();
        let shaped = Resolved::new(text, Direction::Rtl).row(0..n, Emit::Reorder);
        for logical in 0..=n {
            let column = shaped.caret_column(logical, Affinity::Leading);
            assert!(
                column <= shaped.columns(),
                "character {logical} fell off the row"
            );
        }
    }

    #[test]
    fn the_caret_sits_on_the_right_edge_of_a_right_to_left_word() {
        // Typing into "مرحبا" starts at the right, which is column 5 of 5.
        let shaped = Resolved::new("مرحبا", Direction::Rtl).row(0..5, Emit::Reorder);
        assert_eq!(shaped.caret_column(0, Affinity::Leading), 5);
        // And the end of the line is its left edge.
        assert_eq!(shaped.caret_column(5, Affinity::Leading), 0);
    }

    #[test]
    fn presentation_bakes_in_the_joining_forms_and_fuses_lam_alef() {
        let shaped = Resolved::new("لا", Direction::Rtl).row(0..2, Emit::Presentation);
        // Lam plus alef is one glyph, so two characters occupy one column.
        assert_eq!(shaped.text(), "\u{FEFB}");
        assert_eq!(shaped.columns(), 1);
        assert_eq!(shaped.cells()[0].logical, (0, 2));
    }

    #[test]
    fn modes_and_emits_round_trip_through_their_keys() {
        for mode in [DirectionMode::Auto, DirectionMode::Ltr, DirectionMode::Rtl] {
            assert_eq!(mode.key().parse(), Ok(mode));
        }
        for emit in [Emit::Reorder, Emit::Presentation] {
            assert_eq!(emit.key().parse(), Ok(emit));
        }
        // An unrecognised value is rejected, so the caller can fall back rather
        // than lose the rest of the config file to a typo.
        assert!("sideways".parse::<DirectionMode>().is_err());
    }

    #[test]
    fn resolve_honours_an_override_and_falls_back_when_there_is_nothing_to_go_on() {
        assert_eq!(
            resolve("مرحبا", DirectionMode::Ltr, Direction::Ltr),
            Direction::Ltr,
            "an explicit mode wins over the content"
        );
        assert_eq!(
            resolve("مرحبا", DirectionMode::Auto, Direction::Ltr),
            Direction::Rtl
        );
        assert_eq!(
            resolve("123", DirectionMode::Auto, Direction::Rtl),
            Direction::Rtl,
            "no strong character means the fallback decides"
        );
    }

    #[test]
    fn a_wrapped_row_is_resolved_against_the_whole_line() {
        // "world" sits on its own row, but the line is right-to-left, so the
        // row must still be laid out as part of an RTL paragraph.
        let text = "مرحبا world بالعالم";
        let resolved = Resolved::new(text, Direction::Rtl);
        let row = resolved.row(6..11, Emit::Reorder);
        assert_eq!(row.text(), "world");
        assert_eq!(row.base(), Direction::Rtl);
    }
}
