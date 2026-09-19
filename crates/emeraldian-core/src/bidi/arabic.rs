//! Arabic contextual joining forms.
//!
//! Only used by [`super::Emit::Presentation`], for terminals that do not shape
//! Arabic themselves. A terminal that does shape wants the plain letters, since
//! handing it pre-joined forms would leave it nothing to do and risk it shaping
//! them a second time.
//!
//! (base, isolated, final, initial, medial). A zero initial/medial means the
//! letter is right-joining: it connects to what precedes it and never to what
//! follows.
const FORMS: &[(char, char, char, char, char)] = &[
    ('\u{0621}', '\u{FE80}', '\0', '\0', '\0'),
    ('\u{0622}', '\u{FE81}', '\u{FE82}', '\0', '\0'),
    ('\u{0623}', '\u{FE83}', '\u{FE84}', '\0', '\0'),
    ('\u{0624}', '\u{FE85}', '\u{FE86}', '\0', '\0'),
    ('\u{0625}', '\u{FE87}', '\u{FE88}', '\0', '\0'),
    ('\u{0626}', '\u{FE89}', '\u{FE8A}', '\u{FE8B}', '\u{FE8C}'),
    ('\u{0627}', '\u{FE8D}', '\u{FE8E}', '\0', '\0'),
    ('\u{0628}', '\u{FE8F}', '\u{FE90}', '\u{FE91}', '\u{FE92}'),
    ('\u{0629}', '\u{FE93}', '\u{FE94}', '\0', '\0'),
    ('\u{062A}', '\u{FE95}', '\u{FE96}', '\u{FE97}', '\u{FE98}'),
    ('\u{062B}', '\u{FE99}', '\u{FE9A}', '\u{FE9B}', '\u{FE9C}'),
    ('\u{062C}', '\u{FE9D}', '\u{FE9E}', '\u{FE9F}', '\u{FEA0}'),
    ('\u{062D}', '\u{FEA1}', '\u{FEA2}', '\u{FEA3}', '\u{FEA4}'),
    ('\u{062E}', '\u{FEA5}', '\u{FEA6}', '\u{FEA7}', '\u{FEA8}'),
    ('\u{062F}', '\u{FEA9}', '\u{FEAA}', '\0', '\0'),
    ('\u{0630}', '\u{FEAB}', '\u{FEAC}', '\0', '\0'),
    ('\u{0631}', '\u{FEAD}', '\u{FEAE}', '\0', '\0'),
    ('\u{0632}', '\u{FEAF}', '\u{FEB0}', '\0', '\0'),
    ('\u{0633}', '\u{FEB1}', '\u{FEB2}', '\u{FEB3}', '\u{FEB4}'),
    ('\u{0634}', '\u{FEB5}', '\u{FEB6}', '\u{FEB7}', '\u{FEB8}'),
    ('\u{0635}', '\u{FEB9}', '\u{FEBA}', '\u{FEBB}', '\u{FEBC}'),
    ('\u{0636}', '\u{FEBD}', '\u{FEBE}', '\u{FEBF}', '\u{FEC0}'),
    ('\u{0637}', '\u{FEC1}', '\u{FEC2}', '\u{FEC3}', '\u{FEC4}'),
    ('\u{0638}', '\u{FEC5}', '\u{FEC6}', '\u{FEC7}', '\u{FEC8}'),
    ('\u{0639}', '\u{FEC9}', '\u{FECA}', '\u{FECB}', '\u{FECC}'),
    ('\u{063A}', '\u{FECD}', '\u{FECE}', '\u{FECF}', '\u{FED0}'),
    ('\u{0641}', '\u{FED1}', '\u{FED2}', '\u{FED3}', '\u{FED4}'),
    ('\u{0642}', '\u{FED5}', '\u{FED6}', '\u{FED7}', '\u{FED8}'),
    ('\u{0643}', '\u{FED9}', '\u{FEDA}', '\u{FEDB}', '\u{FEDC}'),
    ('\u{0644}', '\u{FEDD}', '\u{FEDE}', '\u{FEDF}', '\u{FEE0}'),
    ('\u{0645}', '\u{FEE1}', '\u{FEE2}', '\u{FEE3}', '\u{FEE4}'),
    ('\u{0646}', '\u{FEE5}', '\u{FEE6}', '\u{FEE7}', '\u{FEE8}'),
    ('\u{0647}', '\u{FEE9}', '\u{FEEA}', '\u{FEEB}', '\u{FEEC}'),
    ('\u{0648}', '\u{FEED}', '\u{FEEE}', '\0', '\0'),
    ('\u{0649}', '\u{FEEF}', '\u{FEF0}', '\0', '\0'),
    ('\u{064A}', '\u{FEF1}', '\u{FEF2}', '\u{FEF3}', '\u{FEF4}'),
];

/// (lam + alef variant) -> (isolated ligature, final ligature).
const LIGATURES: &[(char, char, char)] = &[
    ('\u{0622}', '\u{FEF5}', '\u{FEF6}'),
    ('\u{0623}', '\u{FEF7}', '\u{FEF8}'),
    ('\u{0625}', '\u{FEF9}', '\u{FEFA}'),
    ('\u{0627}', '\u{FEFB}', '\u{FEFC}'),
];

fn entry(ch: char) -> Option<&'static (char, char, char, char, char)> {
    FORMS.iter().find(|e| e.0 == ch)
}

/// Joins to what follows it: dual-joining letters and tatweel.
fn joins_forward(ch: char) -> bool {
    ch == '\u{0640}' || entry(ch).is_some_and(|e| e.3 != '\0')
}

/// Accepts a join from what precedes it: every Arabic letter does.
fn joins_backward(ch: char) -> bool {
    ch == '\u{0640}' || entry(ch).is_some()
}

/// Maps a logical Arabic string to presentation forms, still in logical order.
///
/// Returns one entry per output character, carrying how many input characters
/// it consumed, so a lam-alef ligature can report that it swallowed two.
pub fn shape(chars: &[char]) -> Vec<(char, usize)> {
    let mut out = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        let prev = i.checked_sub(1).map(|p| chars[p]);
        let after_prev = prev.is_some_and(joins_forward);

        // Lam + alef always collapses into one glyph.
        if ch == '\u{0644}'
            && let Some(&next) = chars.get(i + 1)
            && let Some(lig) = LIGATURES.iter().find(|l| l.0 == next)
        {
            out.push((if after_prev { lig.2 } else { lig.1 }, 2));
            i += 2;
            continue;
        }

        let Some(e) = entry(ch) else {
            out.push((ch, 1));
            i += 1;
            continue;
        };
        let dual = e.3 != '\0';
        let before_next = dual && chars.get(i + 1).copied().is_some_and(joins_backward);
        let form = match (after_prev, before_next) {
            (true, true) => e.4,
            (true, false) => e.2,
            (false, true) => e.3,
            (false, false) => e.1,
        };
        out.push((if form == '\0' { e.1 } else { form }, 1));
        i += 1;
    }
    out
}
