//! Measuring and laying out one line of chrome text.
//!
//! The thin terminal-facing layer over [`emeraldian_core::bidi`], for the parts
//! of the app that are not the note: file names, outline entries, tab titles,
//! the status bar, and the single-line inputs.
//!
//! Two habits it exists to replace. The first is measuring text with
//! `chars().count()`, which is not its width — a CJK ideograph is two columns
//! and a combining mark is none — and which was wrong in twenty-odd places
//! here before this module existed. The second is placing a caret at
//! `x + cursor`, which assumes one character is one column and one column is
//! one character, and neither holds.
//!
//! Direction is always resolved per string. A file named in Arabic reads
//! right-to-left whatever the open note is set to, because the name is its own
//! sentence and has nothing to do with the note.

use emeraldian_core::bidi::{self, Affinity, Direction, DirectionMode, Emit, Resolved};

use crate::ui::truncate;

/// Display width in terminal columns.
#[must_use]
pub fn width(text: &str) -> usize {
    bidi::display_width(text)
}

/// Lays a string out in the order it should be drawn.
#[must_use]
pub fn shape(text: &str, emit: Emit) -> String {
    if !bidi::has_rtl(text) {
        return text.to_string();
    }
    let base = bidi::resolve(text, DirectionMode::Auto, Direction::Ltr);
    let count = text.chars().count();
    Resolved::new(text, base).row(0..count, emit).text()
}

/// Whether a string reads right-to-left, so a caller can align it.
#[must_use]
pub fn is_rtl(text: &str) -> bool {
    bidi::has_rtl(text) && bidi::base_direction(text) == Some(Direction::Rtl)
}

/// Truncates to `max` columns and lays the result out, for a list entry.
///
/// Truncating first and laying out after is the only order that works: the
/// clipping is a fact about the text, and doing it to already-reordered text
/// would cut a word out of the middle of the line.
#[must_use]
pub fn label(text: &str, max: usize, emit: Emit) -> String {
    shape(&truncate(text, max), emit)
}

/// The display column a caret sits at, given a cursor counted in characters.
///
/// Replaces `x + cursor`, which is only right for text that is entirely
/// left-to-right and entirely single-width.
#[must_use]
pub fn caret_x(text: &str, cursor: usize, emit: Emit) -> u16 {
    let column = if bidi::has_rtl(text) {
        let base = bidi::resolve(text, DirectionMode::Auto, Direction::Ltr);
        let count = text.chars().count();
        Resolved::new(text, base)
            .row(0..count, emit)
            .caret_column(cursor, Affinity::default())
    } else {
        // Still not a character count: a wide glyph before the cursor pushes it
        // along by two.
        text.chars().take(cursor).map(char_width).sum()
    };
    u16::try_from(column).unwrap_or(u16::MAX)
}

fn char_width(ch: char) -> usize {
    unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_counts_columns_not_characters() {
        assert_eq!(width("hello"), 5);
        // Three ideographs, two columns each.
        assert_eq!(width("日本語"), 6);
        assert_eq!(width("مرحبا"), 5);
    }

    #[test]
    fn a_caret_clears_a_wide_glyph_in_one_step() {
        // The bug this replaces: `x + cursor` would put the caret inside the
        // second ideograph rather than after the first.
        assert_eq!(caret_x("日本語", 1, Emit::default()), 2);
        assert_eq!(caret_x("日本語", 2, Emit::default()), 4);
    }

    #[test]
    fn a_caret_in_rtl_text_is_measured_from_the_right() {
        // Typing Arabic starts at the right-hand end of what is there.
        assert_eq!(caret_x("مرحبا", 0, Emit::default()), 5);
        assert_eq!(caret_x("مرحبا", 5, Emit::default()), 0);
    }

    #[test]
    fn ltr_text_is_handed_back_untouched() {
        assert_eq!(shape("plain text", Emit::default()), "plain text");
        assert!(!is_rtl("plain text"));
    }

    #[test]
    fn an_arabic_label_is_laid_out_and_reported_rtl() {
        assert!(is_rtl("مرحبا بالعالم"));
        assert_eq!(shape("مرحبا بالعالم", Emit::Runs), "بالعالم مرحبا");
    }

    #[test]
    fn a_caret_in_a_masked_or_empty_field_is_at_the_start() {
        assert_eq!(caret_x("", 0, Emit::default()), 0);
    }

    #[test]
    fn a_latin_word_inside_an_arabic_field_keeps_its_order() {
        let out = shape("مرحبا world", Emit::Runs);
        assert!(out.contains("world"), "got {out:?}");
    }

    #[test]
    fn a_caret_walks_an_arabic_field_without_repeating_a_column() {
        // What makes typing in the search box feel right: each character has
        // its own column, and they march the way the script reads.
        let text = "مرحبا";
        let columns: Vec<u16> = (0..=text.chars().count())
            .map(|at| caret_x(text, at, Emit::Runs))
            .collect();
        assert!(
            columns.windows(2).all(|w| w[0] > w[1]),
            "a caret should walk right to left: {columns:?}"
        );
    }

    #[test]
    fn a_label_is_clipped_before_it_is_laid_out() {
        // Clipping after reordering would take the words out of the middle.
        let out = label("مرحبا بالعالم", 6, Emit::Runs);
        assert!(width(&out) <= 6, "{out:?} should fit in six columns");
    }
}
