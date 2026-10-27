//! The theme model: a flat set of semantic color slots.
//!
//! Slots are named after what they mean in the UI (`tab_active_bg`, `h1`,
//! `graph_node_unresolved`) rather than after a color, which is what lets a
//! single palette drive every widget consistently. The names deliberately track
//! Obsidian's own CSS variables (`background-primary`, `text-muted`,
//! `interactive-accent`, …) so porting a theme from Obsidian is mechanical.
//!
//! Two parallel structs are generated from one slot list:
//!
//! - [`Theme`] holds `String`s. It's what gets serialized, so users can write a
//!   partial TOML theme and have the empty slots inherit from a base theme.
//! - [`Palette`] holds resolved [`Color`]s. It's what the renderer reads, so
//!   parsing happens once at theme-switch time rather than every frame.

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

use crate::color;
use crate::seed::Seed;

/// Declares the slot list once and generates [`Theme`], [`Palette`], the
/// inheritance merge, and the string→color resolution from it. Adding a slot
/// means adding one line here plus one line in [`Theme::from_seed`].
macro_rules! define_slots {
    ($($(#[$meta:meta])* $slot:ident),* $(,)?) => {
        /// A theme as written on disk: every slot is a color string, and an
        /// empty slot means "inherit from the base theme".
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct Theme {
            /// Theme identifier, e.g. `obsidian-dark`. Used by config lookup.
            pub name: String,
            /// Whether this is a dark theme. Drives a few contrast decisions
            /// (e.g. how far to tint hover states) that can't be read off the
            /// individual colors when they're ANSI names rather than RGB.
            #[serde(default = "default_dark")]
            pub dark: bool,
            $(
                $(#[$meta])*
                #[serde(default, skip_serializing_if = "String::is_empty")]
                pub $slot: String,
            )*
        }

        /// A theme with every slot resolved to a concrete terminal color.
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct Palette {
            pub name: String,
            pub dark: bool,
            $($(#[$meta])* pub $slot: Color,)*
        }

        impl Theme {
            /// Fills this theme's empty slots from `base`.
            ///
            /// This is what makes user themes practical to write: overriding
            /// three colors shouldn't require restating the other ninety.
            #[must_use]
            pub fn layered_over(mut self, base: &Self) -> Self {
                $(
                    if self.$slot.is_empty() {
                        self.$slot = base.$slot.clone();
                    }
                )*
                self
            }
        }

        impl From<&Theme> for Palette {
            fn from(theme: &Theme) -> Self {
                Self {
                    name: theme.name.clone(),
                    dark: theme.dark,
                    $($slot: color::parse(&theme.$slot),)*
                }
            }
        }
    };
}

fn default_dark() -> bool {
    true
}

define_slots! {
    // ---- surfaces ------------------------------------------------------
    /// The note pane background — Obsidian's `--background-primary`.
    bg_primary,
    /// Slightly offset primary, used for the active line and alternating rows.
    bg_primary_alt,
    /// Sidebar background — Obsidian's `--background-secondary`.
    bg_secondary,
    /// Ribbon and status bar background.
    bg_secondary_alt,
    /// Row under the cursor in a list.
    bg_hover,
    /// Row that is actually selected/open.
    bg_active,
    /// Text selection background in the editor.
    bg_selection,
    border,
    /// Border of the pane that currently has focus.
    border_focus,

    // ---- chrome --------------------------------------------------------
    ribbon_bg,
    ribbon_icon,
    ribbon_icon_active,
    titlebar_bg,
    titlebar_fg,
    tab_bar_bg,
    tab_active_bg,
    tab_active_fg,
    tab_inactive_fg,
    /// Dot marking a tab with unsaved changes.
    tab_modified,
    statusbar_bg,
    statusbar_fg,
    scrollbar_track,
    scrollbar_thumb,

    // ---- text ----------------------------------------------------------
    text_normal,
    text_muted,
    text_faint,
    text_accent,
    /// Text drawn on top of an accent-filled background.
    text_on_accent,
    text_error,
    text_warning,
    text_success,
    text_info,
    /// `==highlight==` background.
    text_highlight_bg,
    text_highlight_fg,
    accent,
    accent_hover,

    // ---- markdown ------------------------------------------------------
    h1, h2, h3, h4, h5, h6,
    bold,
    italic,
    strikethrough,
    /// A `[[wikilink]]` that resolves to an existing note.
    link,
    /// A `[[wikilink]]` with no matching note — Obsidian dims these.
    link_unresolved,
    /// A markdown `[text](url)` link.
    link_external,
    tag_fg,
    tag_bg,
    code_fg,
    code_bg,
    quote_bar,
    quote_fg,
    hr,
    list_marker,
    checkbox_done,
    checkbox_todo,
    table_header,
    table_border,
    frontmatter_key,
    frontmatter_value,

    // ---- syntax highlighting inside fenced code ------------------------
    syn_keyword,
    syn_string,
    syn_comment,
    syn_number,
    syn_function,
    syn_type,
    syn_punct,

    // ---- callouts (`> [!note]`) ----------------------------------------
    callout_note,
    callout_tip,
    callout_warning,
    callout_danger,
    callout_success,
    callout_question,
    callout_quote,

    // ---- graph view ----------------------------------------------------
    graph_bg,
    graph_node,
    graph_node_focused,
    /// A node adjacent to the focused one.
    graph_node_neighbor,
    /// A link target that has no note behind it yet.
    graph_node_unresolved,
    graph_node_tag,
    graph_edge,
    graph_edge_active,
    graph_label,
    graph_label_focused,

    // ---- editor --------------------------------------------------------
    cursor,
    cursor_line_bg,
    line_number,
    line_number_active,
}

impl Theme {
    /// Expands a compact [`Seed`] into a full theme.
    ///
    /// Themes are defined as ~20 base colors rather than ~90 slots because the
    /// mapping from base colors to UI roles is the same for every theme — it's
    /// Obsidian's design, not the color scheme's. Keeping it in one place means
    /// adding a theme is a matter of picking colors, and a change to how (say)
    /// callouts are colored applies to every theme at once.
    #[must_use]
    pub fn from_seed(seed: &Seed) -> Self {
        let s = seed;
        Self {
            name: s.name.to_string(),
            dark: s.dark,

            bg_primary: s.bg.into(),
            bg_primary_alt: s.bg_alt.into(),
            bg_secondary: s.bg2.into(),
            bg_secondary_alt: s.bg3.into(),
            bg_hover: s.hover.into(),
            bg_active: s.active.into(),
            bg_selection: s.active.into(),
            border: s.border.into(),
            border_focus: s.accent.into(),

            ribbon_bg: s.bg3.into(),
            ribbon_icon: s.faint.into(),
            ribbon_icon_active: s.accent.into(),
            titlebar_bg: s.bg2.into(),
            titlebar_fg: s.muted.into(),
            tab_bar_bg: s.bg2.into(),
            tab_active_bg: s.bg.into(),
            tab_active_fg: s.text.into(),
            tab_inactive_fg: s.faint.into(),
            tab_modified: s.accent.into(),
            statusbar_bg: s.bg3.into(),
            statusbar_fg: s.muted.into(),
            scrollbar_track: s.bg2.into(),
            scrollbar_thumb: s.border.into(),

            text_normal: s.text.into(),
            text_muted: s.muted.into(),
            text_faint: s.faint.into(),
            text_accent: s.accent.into(),
            text_on_accent: "#ffffff".into(),
            text_error: s.red.into(),
            text_warning: s.orange.into(),
            text_success: s.green.into(),
            text_info: s.blue.into(),
            text_highlight_bg: s.yellow.into(),
            text_highlight_fg: s.bg.into(),
            accent: s.accent.into(),
            accent_hover: s.purple.into(),

            // Obsidian doesn't recolor headings by default, it scales them.
            // A terminal can't scale type, so the hierarchy is carried by color
            // instead: warm/bright at H1 fading toward muted by H6.
            h1: s.accent.into(),
            h2: s.blue.into(),
            h3: s.cyan.into(),
            h4: s.green.into(),
            h5: s.yellow.into(),
            h6: s.muted.into(),
            bold: s.text.into(),
            italic: s.text.into(),
            strikethrough: s.faint.into(),
            link: s.accent.into(),
            link_unresolved: s.faint.into(),
            link_external: s.blue.into(),
            tag_fg: s.accent.into(),
            tag_bg: s.hover.into(),
            code_fg: s.pink.into(),
            code_bg: s.bg_alt.into(),
            quote_bar: s.accent.into(),
            quote_fg: s.muted.into(),
            hr: s.border.into(),
            // A quiet grey, as Obsidian draws them, rather than the accent,
            // which put a column of accent dots down every list.
            list_marker: s.muted.into(),
            checkbox_done: s.green.into(),
            checkbox_todo: s.muted.into(),
            table_header: s.text.into(),
            table_border: s.border.into(),
            frontmatter_key: s.purple.into(),
            frontmatter_value: s.muted.into(),

            syn_keyword: s.purple.into(),
            syn_string: s.green.into(),
            syn_comment: s.faint.into(),
            syn_number: s.orange.into(),
            syn_function: s.blue.into(),
            syn_type: s.yellow.into(),
            syn_punct: s.muted.into(),

            callout_note: s.blue.into(),
            callout_tip: s.cyan.into(),
            callout_warning: s.orange.into(),
            callout_danger: s.red.into(),
            callout_success: s.green.into(),
            callout_question: s.yellow.into(),
            callout_quote: s.muted.into(),

            graph_bg: s.bg.into(),
            graph_node: s.text.into(),
            graph_node_focused: s.accent.into(),
            graph_node_neighbor: s.cyan.into(),
            graph_node_unresolved: s.faint.into(),
            graph_node_tag: s.green.into(),
            // Full text weight, not the border tone this used to borrow: a
            // border is drawn to be ignored, and at `#2f2f2f` on a `#1e1e1e`
            // background the links simply weren't there. A graph whose edges
            // can't be seen is a scatter plot.
            graph_edge: s.text.into(),
            graph_edge_active: s.accent.into(),
            graph_label: s.muted.into(),
            graph_label_focused: s.text.into(),

            cursor: s.accent.into(),
            cursor_line_bg: s.bg_alt.into(),
            line_number: s.faint.into(),
            line_number_active: s.muted.into(),
        }
    }
}

/// The least contrast against the page for lines and shapes that mean
/// something, as WCAG asks of them (success criterion 1.4.11).
pub const STROKE_CONTRAST: f32 = 3.0;

/// The least contrast against the page for words: WCAG's level AA for body
/// text (success criterion 1.4.3).
pub const TEXT_CONTRAST: f32 = 4.5;

/// Hues closer than this in OKLab look alike at a glance, like Tokyo Night's
/// red and pink, so only the first of them is used.
const ALIKE: f32 = 0.07;

/// Hues with less chroma than this are greys, which tell nothing apart.
const GREY: f32 = 0.03;

/// A theme's hues, for telling things apart, such as the boxes of a diagram
/// or the people in a conversation.
///
/// They are the colours the theme already gives its callouts and code, so
/// what is drawn with them looks like the rest of the page in every theme,
/// light or dark, and follows a user's theme when it changes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hues {
    /// For lines, outlines and arrowheads, each at least
    /// [`STROKE_CONTRAST`]:1 against the page.
    pub strokes: Vec<Color>,
    /// The same hues for words, in the same order, each at least
    /// [`TEXT_CONTRAST`]:1. Most are the stroke colour itself; on a light
    /// page a pale one is darkened a little further.
    pub words: Vec<Color>,
}

impl Palette {
    /// The theme's hues, each made readable on the page, with any that would
    /// look like one already taken, or like grey, left out. Two that would
    /// round to the same colour in a terminal of 256 colours count as alike
    /// too, so they stay apart there.
    ///
    /// The order puts unlike hues next to each other, so the colour after one
    /// that is taken is a different one to the eye too. A theme of named
    /// terminal colours keeps them as they are, since only the terminal knows
    /// what they look like.
    #[must_use]
    pub fn hues(&self) -> Hues {
        let theirs = [
            self.callout_note,     // blue
            self.callout_warning,  // orange
            self.callout_success,  // green
            self.code_fg,          // pink
            self.callout_tip,      // cyan
            self.callout_question, // yellow
            self.syn_keyword,      // purple
            self.callout_danger,   // red
        ];
        let mut hues = Hues {
            strokes: Vec::with_capacity(theirs.len()),
            words: Vec::with_capacity(theirs.len()),
        };
        for hue in theirs {
            let stroke = color::readable(hue, self.bg_primary, STROKE_CONTRAST);
            let grey = color::chroma(stroke).is_some_and(|chroma| chroma < GREY);
            let rounded = color::xterm_256(stroke);
            let taken = hues.strokes.iter().any(|&kept| {
                kept == stroke
                    || color::difference(kept, stroke).is_some_and(|d| d < ALIKE)
                    || (rounded.is_some() && color::xterm_256(kept) == rounded)
            });
            if stroke == Color::Reset || grey || taken {
                continue;
            }
            hues.strokes.push(stroke);
            hues.words
                .push(color::readable(hue, self.bg_primary, TEXT_CONTRAST));
        }
        hues
    }

    /// Heading color for a level, clamped to the H1–H6 range.
    #[must_use]
    pub fn heading(&self, level: u8) -> Color {
        match level {
            0 | 1 => self.h1,
            2 => self.h2,
            3 => self.h3,
            4 => self.h4,
            5 => self.h5,
            _ => self.h6,
        }
    }

    /// Color for a callout kind, matching Obsidian's built-in callout types.
    /// Unknown kinds fall back to the neutral `note` color, which is also what
    /// Obsidian does.
    #[must_use]
    pub fn callout(&self, kind: &str) -> Color {
        match kind {
            "tip" | "hint" | "important" => self.callout_tip,
            "warning" | "caution" | "attention" => self.callout_warning,
            "danger" | "error" | "bug" | "failure" | "fail" | "missing" => self.callout_danger,
            "success" | "check" | "done" => self.callout_success,
            "question" | "help" | "faq" => self.callout_question,
            "quote" | "cite" => self.callout_quote,
            _ => self.callout_note,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed;

    #[test]
    fn layering_only_fills_empty_slots() {
        let base = Theme::from_seed(&seed::OBSIDIAN_DARK);
        let mut custom = Theme::from_seed(&seed::OBSIDIAN_DARK);
        custom.name = "mine".into();
        custom.accent = "#ff0000".into();
        custom.h1 = String::new();

        let merged = custom.layered_over(&base);
        assert_eq!(merged.accent, "#ff0000", "explicit slot must win");
        assert_eq!(merged.h1, base.h1, "empty slot must inherit");
    }

    #[test]
    fn palette_resolves_every_slot() {
        let theme = Theme::from_seed(&seed::OBSIDIAN_DARK);
        let palette = Palette::from(&theme);
        // A seeded theme sets every slot, so nothing should land on Reset.
        assert_ne!(palette.bg_primary, Color::Reset);
        assert_ne!(palette.graph_node_focused, Color::Reset);
        assert_ne!(palette.syn_keyword, Color::Reset);
    }

    #[test]
    fn graph_edges_stand_out_against_the_graph_background() {
        // Edges used to borrow the border tone, which on obsidian-dark is
        // #2f2f2f against a #1e1e1e background — about 1.3:1, invisible. Every
        // theme is checked, because the seed derivation is shared and a new
        // preset could quietly reintroduce it.
        for seed in crate::presets::ALL {
            let palette = Palette::from(&Theme::from_seed(seed));
            // The `terminal` theme resolves to `Reset` so the emulator's own
            // colours show through. There is nothing to measure: `Reset` as a
            // foreground and `Reset` as a background are different colours on
            // screen, and only the terminal knows what they are.
            let (Some(edge), Some(background)) = (
                crate::luminance(palette.graph_edge),
                crate::luminance(palette.graph_bg),
            ) else {
                continue;
            };
            assert_ne!(
                palette.graph_edge, palette.graph_bg,
                "{}: edges are the background",
                seed.name
            );
            assert!(
                (edge - background).abs() > 0.3,
                "{}: edges at {edge:.2} vanish into a {background:.2} background",
                seed.name
            );
        }
    }

    #[test]
    fn list_bullets_are_quiet_rather_than_the_accent() {
        for seed in crate::presets::ALL {
            let palette = Palette::from(&Theme::from_seed(seed));
            assert_eq!(palette.list_marker, palette.text_muted, "{}", seed.name);
            assert_ne!(palette.list_marker, palette.accent, "{}", seed.name);
        }
    }

    #[test]
    fn heading_levels_clamp() {
        let palette = Palette::from(&Theme::from_seed(&seed::OBSIDIAN_DARK));
        assert_eq!(palette.heading(0), palette.h1);
        assert_eq!(palette.heading(1), palette.h1);
        assert_eq!(palette.heading(9), palette.h6);
    }

    /// Every built-in theme, light and dark, has enough hues to tell a
    /// diagram's boxes apart, each one readable on its page and none like
    /// another.
    #[test]
    fn every_theme_has_hues_that_read_on_its_page_and_differ() {
        for seed in crate::presets::ALL {
            let palette = Palette::from(&Theme::from_seed(seed));
            let hues = palette.hues();
            let name = seed.name;
            assert_eq!(hues.strokes.len(), hues.words.len(), "{name}");
            assert!(
                hues.strokes.len() >= 5,
                "{name}: only {} hues",
                hues.strokes.len()
            );
            let page = palette.bg_primary;
            for (at, (&stroke, &word)) in hues.strokes.iter().zip(&hues.words).enumerate() {
                // The terminal theme's are colour names the terminal decides,
                // and its page is the terminal's own: nothing to measure.
                if let Some(ratio) = color::contrast(stroke, page) {
                    assert!(
                        ratio >= STROKE_CONTRAST,
                        "{name}: line {stroke:?} is {ratio:.2}:1"
                    );
                }
                if let Some(ratio) = color::contrast(word, page) {
                    assert!(
                        ratio >= TEXT_CONTRAST,
                        "{name}: words {word:?} are {ratio:.2}:1"
                    );
                }
                // Words are the same hue as their line, just as readable as
                // words need to be.
                if let Some(apart) = color::difference(stroke, word) {
                    assert!(apart < 0.15, "{name}: {stroke:?} and {word:?}");
                }
                for &earlier in &hues.strokes[..at] {
                    assert_ne!(earlier, stroke, "{name}: a hue twice");
                    if let Some(apart) = color::difference(earlier, stroke) {
                        assert!(
                            apart >= ALIKE,
                            "{name}: {earlier:?} and {stroke:?} look alike"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn hues_leave_out_greys_and_lookalikes_and_follow_the_theme() {
        let tokyo = Palette::from(&Theme::from_seed(&seed::TOKYO_NIGHT));
        let strokes = tokyo.hues().strokes;
        assert!(
            strokes.contains(&tokyo.code_fg) != strokes.contains(&tokyo.callout_danger),
            "Tokyo Night's pink and red are one colour to the eye"
        );

        let mut grey = Theme::from_seed(&seed::OBSIDIAN_DARK);
        grey.callout_note = "#8a8a8a".into();
        grey.callout_tip = "#00ffff".into();
        let palette = Palette::from(&grey);
        let strokes = palette.hues().strokes;
        assert!(!strokes.contains(&palette.callout_note), "a grey is no hue");
        assert!(
            strokes.contains(&palette.callout_tip),
            "a user's own colour is used"
        );

        let terminal = Palette::from(&Theme::from_seed(&seed::TERMINAL));
        let named = terminal.hues();
        assert!(named.strokes.contains(&Color::Blue));
        assert_eq!(
            named.strokes, named.words,
            "names are left for the terminal"
        );
    }

    /// Where 24-bit colour isn't shown, as in tmux without its `RGB` feature,
    /// each colour is rounded to the nearest of 256. A diagram's hues have to
    /// stay apart when they are, or two boxes that differed become one colour.
    #[test]
    fn hues_stay_apart_in_a_256_colour_terminal() {
        for seed in crate::presets::ALL {
            let palette = Palette::from(&Theme::from_seed(seed));
            let rounded: Vec<u8> = palette
                .hues()
                .strokes
                .into_iter()
                .filter_map(color::xterm_256)
                .collect();
            for (at, colour) in rounded.iter().enumerate() {
                assert!(
                    !rounded[..at].contains(colour),
                    "{}: two hues round to colour {colour}: {rounded:?}",
                    seed.name
                );
            }
        }
    }
}
