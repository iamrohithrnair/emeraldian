//! Text as retrieval sees it: notes cut into passages, passages cut into
//! terms, and an estimate of what a passage costs a model to read.

use rust_stemmers::{Algorithm, Stemmer};

use crate::excalidraw;
use crate::links;
use crate::note;

/// A stretch of a note, retrieved on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    /// The first and last file lines it covers, 1-based and inclusive.
    pub start: usize,
    pub end: usize,
    pub text: String,
    /// The headings it sits under, second level down, as they stand at its
    /// first line: that line's own, if it is one.
    pub headings: Vec<String>,
}

impl Passage {
    /// The title it is read under: the note's, then the headings it sits
    /// under, as `Note > Section > Part`. A passage seldom repeats what the
    /// section it is in is about, and without this one that says "it" doesn't
    /// say what about.
    #[must_use]
    pub fn title(&self, note: &str) -> String {
        let mut title = note.to_string();
        for heading in &self.headings {
            title.push_str(" > ");
            title.push_str(heading);
        }
        title
    }
}

/// The most of a passage, in bytes: a few paragraphs, a few hundred tokens.
/// Long enough to hold a thought whole, short enough that the passage found is
/// the place to read and not the whole note, and the size a budgeted answer is
/// assembled from.
pub const PASSAGE_BYTES: usize = 1_200;

/// A passage shorter than this takes in what follows it, even a new heading,
/// rather than standing alone as a heading with nothing under it.
const PASSAGE_MIN_BYTES: usize = 200;

/// Cuts a note called `title` into passages: its paragraphs, packed together
/// up to [`PASSAGE_BYTES`], with a heading starting a new passage. A paragraph
/// too long for one passage is cut where it least breaks a thought: between
/// its lines, then between its sentences, and only then between words.
///
/// Frontmatter is left out, and so is a first heading that only repeats the
/// title: the title is given with every passage already, and counted again in
/// the first it would make that one look more about the note than the rest.
/// An Excalidraw drawing is represented by the words in it, not by the
/// compressed scene it is stored as.
#[must_use]
pub fn passages(rel: &str, content: &str, title: &str) -> Vec<Passage> {
    if excalidraw::is_drawing(rel) {
        let words: Vec<String> = excalidraw::parse(content)
            .map(|drawing| {
                drawing
                    .elements
                    .into_iter()
                    .map(|element| element.text.trim().to_string())
                    .filter(|text| !text.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        return if words.is_empty() {
            Vec::new()
        } else {
            let end = content.lines().count().max(1);
            vec![Passage {
                start: 1,
                end,
                text: words.join("\n"),
                headings: Vec::new(),
            }]
        };
    }

    let (_, body, offset) = note::split_frontmatter(content);
    let lines: Vec<&str> = body.lines().collect();
    // The heading on each line, if it has one, read with the code blocks
    // around it: `# install the dependencies` in a block of shell is a line
    // of the block, not a section.
    let mut headings: Vec<Option<links::Heading>> = vec![None; lines.len()];
    for heading in links::extract_headings(body) {
        let line = heading.line;
        headings[line] = Some(heading);
    }
    let from = after_title(&lines, &headings, title);

    // Blocks: runs of lines between blank ones, each heading starting its own,
    // as (first line, last line, text) with 0-based body lines.
    let mut blocks: Vec<(usize, usize, String)> = Vec::new();
    let mut current: Option<(usize, usize, String)> = None;
    for (index, line) in lines.iter().enumerate().skip(from) {
        let blank = line.trim().is_empty();
        if blank || headings[index].is_some() {
            blocks.extend(current.take());
        }
        if blank {
            continue;
        }
        match &mut current {
            Some((_, last, text)) => {
                *last = index;
                text.push('\n');
                text.push_str(line);
            }
            None => current = Some((index, index, (*line).to_string())),
        }
    }
    blocks.extend(current);

    // Packed greedily, in order: an atom joins the passage before it while
    // they fit together, unless it opens a section and the passage before it
    // already says something.
    let mut packed: Vec<(usize, usize, String)> = Vec::new();
    let mut packing: Option<(usize, usize, String)> = None;
    for atom in atoms(blocks, &headings) {
        let joins = packing.as_ref().is_some_and(|(_, _, text)| {
            text.len() + atom.separator.len() + atom.text.len() <= PASSAGE_BYTES
                && (text.len() < PASSAGE_MIN_BYTES || !atom.heading)
        });
        if joins {
            let (_, end, text) = packing.as_mut().expect("checked");
            *end = atom.last;
            text.push_str(atom.separator);
            text.push_str(&atom.text);
        } else {
            packed.extend(packing.take());
            packing = Some((atom.first, atom.last, atom.text));
        }
    }
    packed.extend(packing);

    // The headings in force at each passage's first line, its own included,
    // from one pass down the lines. A first-level heading starts again.
    let mut path: Vec<(u8, String)> = Vec::new();
    let mut next = from;
    packed
        .into_iter()
        .map(|(first, last, text)| {
            while next <= first {
                if let Some(heading) = &headings[next] {
                    path.retain(|(level, _)| *level < heading.level);
                    path.push((heading.level, readable(&heading.text)));
                }
                next += 1;
            }
            Passage {
                start: offset + first + 1,
                end: offset + last + 1,
                text,
                headings: path
                    .iter()
                    .filter(|(level, _)| *level >= 2)
                    .map(|(_, text)| text.clone())
                    .collect(),
            }
        })
        .collect()
}

/// Where a note's text starts once a first heading that only repeats its
/// title, `# Title` under a note called that, is passed over: the line after
/// it, or the first line when there is no such heading. A note that is its
/// title and nothing more keeps it, or nothing of it could be found.
/// `headings` are its lines' headings, as [`passages`] reads them.
fn after_title(lines: &[&str], headings: &[Option<links::Heading>], title: &str) -> usize {
    let Some(first) = lines.iter().position(|line| !line.trim().is_empty()) else {
        return 0;
    };
    let title = title.trim().to_lowercase();
    // As written or as shown: `# C#` is a title of `C#` whatever Markdown
    // makes of the closing hash.
    let written = lines[first]
        .trim_start()
        .strip_prefix("# ")
        .unwrap_or_default();
    let repeats = headings[first].as_ref().is_some_and(|heading| {
        heading.level == 1
            && (heading.text.trim().to_lowercase() == title
                || written.trim().to_lowercase() == title)
    });
    let more = lines[first + 1..]
        .iter()
        .any(|line| !line.trim().is_empty());
    if repeats && more { first + 1 } else { 0 }
}

/// The smallest pieces a passage is packed from, each of which fits one.
struct Atom {
    /// Its first and last lines, 0-based in the body.
    first: usize,
    last: usize,
    text: String,
    /// What goes between it and what comes before, in the same passage: a
    /// blank line between blocks, a line break between the lines of one, and
    /// a space between the sentences or words of one line.
    separator: &'static str,
    /// Whether it opens a section, so starts a passage of its own.
    heading: bool,
}

/// The blocks as pieces that each fit a passage: a block whole where it
/// fits, else its lines, else a long line's sentences, else a long
/// sentence's words. `headings` are the body's lines' headings.
fn atoms(blocks: Vec<(usize, usize, String)>, headings: &[Option<links::Heading>]) -> Vec<Atom> {
    let mut atoms = Vec::new();
    for (first, last, block) in blocks {
        let heading = headings[first].is_some();
        if block.len() <= PASSAGE_BYTES {
            atoms.push(Atom {
                first,
                last,
                text: block,
                separator: "\n\n",
                heading,
            });
            continue;
        }
        for (k, line) in block.split('\n').enumerate() {
            let pieces: Vec<&str> = if line.len() <= PASSAGE_BYTES {
                vec![line]
            } else {
                sentences(line)
                    .into_iter()
                    .flat_map(|sentence| {
                        if sentence.len() <= PASSAGE_BYTES {
                            vec![sentence]
                        } else {
                            words(sentence)
                        }
                    })
                    .collect()
            };
            for (j, piece) in pieces.into_iter().enumerate() {
                let separator = match (k, j) {
                    (0, 0) => "\n\n",
                    (_, 0) => "\n",
                    _ => " ",
                };
                atoms.push(Atom {
                    first: first + k,
                    last: first + k,
                    text: piece.to_string(),
                    separator,
                    heading: heading && k == 0 && j == 0,
                });
            }
        }
    }
    atoms
}

/// A line's sentences: it is cut after a full stop, question mark or
/// exclamation mark that whitespace follows, and the whitespace left out.
///
/// Chinese and Japanese leave no space after a sentence, so their own stops
/// end one whether whitespace follows or not: with nothing else to cut at, a
/// long line of them would be cut through a word. Another stop after one, or
/// the quote or bracket it closes, ends the same sentence: `本当？！` and
/// `「はい。」` stay whole.
fn sentences(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = line.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let unspaced = is_unspaced_stop(c);
        if !unspaced && !matches!(c, '.' | '!' | '?') {
            continue;
        }
        let mut end = at + c.len_utf8();
        if unspaced {
            while let Some(&(next_at, next)) = chars.peek() {
                if !(is_unspaced_stop(next) || matches!(next, '.' | '!' | '?') || closes(next)) {
                    break;
                }
                end = next_at + next.len_utf8();
                chars.next();
            }
        }
        let mut resume = end;
        while let Some(&(next_at, next)) = chars.peek() {
            if !next.is_whitespace() {
                break;
            }
            resume = next_at + next.len_utf8();
            chars.next();
        }
        if unspaced || resume > end {
            out.push(&line[start..end]);
            start = resume;
        }
    }
    out.push(&line[start..]);
    out.retain(|sentence| !sentence.is_empty());
    out
}

/// The stops that end a sentence in Chinese and Japanese: the ideographic
/// full stop and its halfwidth form, and the fullwidth full stop,
/// exclamation mark and question mark.
fn is_unspaced_stop(c: char) -> bool {
    matches!(c, '。' | '｡' | '．' | '！' | '？')
}

/// The quotes and brackets that close what a sentence in Chinese or
/// Japanese quotes.
fn closes(c: char) -> bool {
    matches!(c, '」' | '』' | '）' | '】' | '〉' | '》' | '”' | '’')
}

/// A sentence too long for a passage, in pieces that each fit: cut at the
/// last space or tab before the limit, when that leaves more than half a
/// passage, and otherwise at a character boundary, as a run with no spaces in
/// it, a link or a hash, has to be.
fn words(mut text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    while text.len() > PASSAGE_BYTES {
        let mut cut = PASSAGE_BYTES;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        let mut head = &text[..cut];
        if let Some(space) = head.rfind([' ', '\t'])
            && space > head.len() / 2
        {
            head = &head[..space];
        }
        out.push(head);
        text = text[head.len()..].trim_start();
    }
    if !text.is_empty() {
        out.push(text);
    }
    out
}

/// Text as a model should read it: a `[[Folder/Note|shown]]` link reads as
/// what it shows, and a bare `[[Folder/Note]]` as the note's name. The path is
/// for the vault; the words are for the meaning.
#[must_use]
pub fn readable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = &after[..end];
        let shown = match inner.split_once('|') {
            Some((_, alias)) => alias,
            None => {
                let target = inner.split('#').next().unwrap_or(inner);
                target.rsplit('/').next().unwrap_or(target)
            }
        };
        out.push_str(shown);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// The longest term kept, in characters. Anything longer is a hash, a key or
/// a run of base64, which nobody searches for by its whole.
const MAX_TERM_CHARS: usize = 40;

/// A text's terms as written, before they are stemmed.
///
/// Words are lowercased, and a compound identifier counts as itself and as
/// its parts, so `parse_frontmatter` and `parseFrontmatter` are each found by
/// `frontmatter`. Chinese, Japanese and Korean, written without spaces between
/// words, are indexed as overlapping pairs of characters, which is how search
/// engines without a dictionary find words in them. Single characters other
/// than those are left out.
#[must_use]
pub fn terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut run: Vec<char> = Vec::new();

    for c in text.chars() {
        if is_cjk(c) {
            flush_word(&mut word, &mut out);
            run.push(c);
        } else {
            flush_run(&mut run, &mut out);
            if c.is_alphanumeric() || c == '_' {
                word.push(c);
            } else {
                flush_word(&mut word, &mut out);
            }
        }
    }
    flush_word(&mut word, &mut out);
    flush_run(&mut run, &mut out);
    out
}

/// The terms the lexical index records for a text: its [`terms`], each in
/// the form [`stem`] gives it.
#[must_use]
pub fn index_terms(text: &str) -> Vec<String> {
    terms(text).into_iter().map(stem).collect()
}

/// The terms a question is looked up by: its [`terms`] without the words
/// that only hold a question together, `how`, `the`, `does`, and stemmed as
/// the index's are. A question made of nothing else keeps them all, or it
/// would ask nothing.
#[must_use]
pub fn question_terms(text: &str) -> Vec<String> {
    let all = terms(text);
    let kept: Vec<String> = all
        .iter()
        .filter(|term| STOPWORDS.binary_search(&term.as_str()).is_err())
        .cloned()
        .collect();
    let chosen = if kept.is_empty() { all } else { kept };
    chosen.into_iter().map(stem).collect()
}

/// A term as the index keeps it: a word of letters alone cut to its stem by
/// Snowball's English stemmer, so `hashing`, `hashed` and `hashes` are one
/// term; anything with a digit or an underscore in it, a version or an
/// identifier, just as it is.
#[must_use]
pub fn stem(term: String) -> String {
    if !term.chars().all(char::is_alphabetic) {
        return term;
    }
    Stemmer::create(Algorithm::English).stem(&term).into_owned()
}

/// Words left out of a question: Lucene's English stopwords, with pronouns,
/// auxiliaries and question words added. Not prepositions or quantifiers,
/// which carry meaning in a technical question: `site down`, `opt out`, `not
/// found`. Sorted, to be searched.
const STOPWORDS: &[&str] = &[
    "a",
    "am",
    "an",
    "and",
    "are",
    "as",
    "at",
    "be",
    "been",
    "being",
    "but",
    "by",
    "can",
    "could",
    "did",
    "do",
    "does",
    "doing",
    "for",
    "had",
    "has",
    "have",
    "having",
    "he",
    "her",
    "hers",
    "him",
    "his",
    "how",
    "i",
    "if",
    "in",
    "into",
    "is",
    "it",
    "its",
    "itself",
    "may",
    "me",
    "might",
    "mine",
    "must",
    "my",
    "myself",
    "no",
    "not",
    "of",
    "on",
    "or",
    "our",
    "ours",
    "ourselves",
    "shall",
    "she",
    "should",
    "such",
    "that",
    "the",
    "their",
    "theirs",
    "them",
    "themselves",
    "then",
    "there",
    "these",
    "they",
    "this",
    "to",
    "us",
    "was",
    "we",
    "were",
    "what",
    "when",
    "where",
    "which",
    "who",
    "whom",
    "whose",
    "why",
    "will",
    "with",
    "would",
    "you",
    "your",
    "yours",
    "yourself",
];

fn flush_word(word: &mut String, out: &mut Vec<String>) {
    if word.is_empty() {
        return;
    }
    let whole = word.trim_matches('_');
    let length = whole.chars().count();
    if (2..=MAX_TERM_CHARS).contains(&length) {
        out.push(whole.to_lowercase());
    }
    if length >= 2 {
        let parts = identifier_parts(whole);
        if parts.len() > 1 {
            out.extend(
                parts
                    .into_iter()
                    .filter(|part| (2..=MAX_TERM_CHARS).contains(&part.chars().count()))
                    .map(|part| part.to_lowercase()),
            );
        }
    }
    word.clear();
}

/// `parse_frontmatter` → `parse`, `frontmatter`; `parseFrontMatter` →
/// `parse`, `Front`, `Matter`; `HTTPServer` → `HTTP`, `Server`.
fn identifier_parts(word: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    for piece in word.split('_').filter(|p| !p.is_empty()) {
        let chars: Vec<(usize, char)> = piece.char_indices().collect();
        let mut start = 0;
        for i in 1..chars.len() {
            let (at, c) = chars[i];
            let prev = chars[i - 1].1;
            let next_lower = chars.get(i + 1).is_some_and(|(_, n)| n.is_lowercase());
            let boundary = (prev.is_lowercase() && c.is_uppercase())
                || (prev.is_uppercase() && c.is_uppercase() && next_lower);
            if boundary {
                parts.push(&piece[start..at]);
                start = at;
            }
        }
        parts.push(&piece[start..]);
    }
    parts
}

fn flush_run(run: &mut Vec<char>, out: &mut Vec<String>) {
    match run.len() {
        0 => {}
        1 => out.push(run[0].to_string()),
        _ => out.extend(run.windows(2).map(|pair| pair.iter().collect::<String>())),
    }
    run.clear();
}

/// Han, Hiragana, Katakana and Hangul: scripts searched by pairs of
/// characters.
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF   // Hiragana, Katakana
        | 0x3400..=0x4DBF // CJK Extension A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xAC00..=0xD7AF // Hangul syllables
        | 0xF900..=0xFAFF // CJK Compatibility Ideographs
        | 0x20000..=0x2FA1F) // CJK Extensions B onwards
}

/// About how many tokens a text costs a language model to read: roughly four
/// characters of English to a token, one Chinese or Japanese character, and
/// two of most other scripts. An estimate, deliberately on the generous side,
/// so a budget it enforces is a budget kept.
#[must_use]
pub fn tokens(text: &str) -> usize {
    let quarters: usize = text
        .chars()
        .map(|c| {
            if c.is_ascii() {
                1
            } else if is_cjk(c) {
                4
            } else {
                2
            }
        })
        .sum();
    quarters.div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(found: &[Passage]) -> Vec<(usize, usize)> {
        found.iter().map(|p| (p.start, p.end)).collect()
    }

    #[test]
    fn passages_follow_paragraphs_and_headings_with_their_lines() {
        let note = format!(
            "---\ntags: [x]\n---\n# Title\n\n{}\n\n## Part\n\n{}\n\n\n{}\n",
            "a".repeat(300),
            "b".repeat(700),
            "c".repeat(700)
        );
        let found = passages("Note.md", &note, "Note");
        assert_eq!(spans(&found), vec![(4, 6), (8, 10), (13, 13)], "{found:#?}");
        assert!(
            found[1].text.starts_with("## Part\n\nbbb"),
            "a heading starts one"
        );
        assert!(found.iter().all(|p| p.text.len() <= PASSAGE_BYTES));
        assert!(
            !found[0].text.contains("tags"),
            "frontmatter is not a passage"
        );

        let small = passages("Note.md", "# Title\n\nIntro.\n\n## Part\n\nText.\n", "Note");
        assert_eq!(small.len(), 1, "a heading with little above it joins it");
        assert_eq!((small[0].start, small[0].end), (1, 7));
    }

    #[test]
    fn a_heading_starts_a_passage_once_the_one_before_says_something() {
        let note = format!(
            "{}\n\n## Next\n\n{}\n\n## Last\n\n{}\n",
            "a".repeat(150),
            "b".repeat(100),
            "c".repeat(100)
        );
        let found = passages("Note.md", &note, "Note");
        assert_eq!(
            spans(&found),
            vec![(1, 5), (7, 9)],
            "under 200 bytes, the first takes in the next heading; past it, the last heading \
             starts its own: {found:#?}"
        );
        assert!(found[1].text.starts_with("## Last"));
    }

    #[test]
    fn a_long_paragraph_is_cut_between_lines_then_sentences_then_words() {
        let sentence = |c: char| format!("{} {}.", "Word".repeat(10), c.to_string().repeat(150));
        // Line 1 fits with room to spare; line 2 is many sentences; line 3 is
        // one sentence of words longer than a passage.
        let line1 = "Opening line.".to_string();
        let line2 = (0..8)
            .map(|i| sentence(char::from(b'a' + i)))
            .collect::<Vec<_>>()
            .join(" ");
        let words_line = std::iter::repeat_n("lorem", 300)
            .collect::<Vec<_>>()
            .join(" ");
        let note = format!("{line1}\n{line2}\n{words_line}\n");
        let found = passages("Note.md", &note, "Note");
        assert!(
            found.iter().all(|p| p.text.len() <= PASSAGE_BYTES),
            "{found:#?}"
        );

        // The first passage starts with the first line, joined by a line
        // break, and stops between two of the second line's sentences.
        assert!(found[0].text.starts_with("Opening line.\nWordWord"));
        assert_eq!((found[0].start, found[0].end), (1, 2));
        assert!(found[0].text.ends_with('.'), "{:?}", found[0].text);
        // The second line's sentences go on in the next, each one whole.
        assert_eq!((found[1].start, found[1].end), (2, 2));
        assert!(found[1].text.starts_with("WordWord"));
        // The third line's words are cut at a space, never mid-word.
        let last: Vec<&Passage> = found.iter().filter(|p| p.start == 3).collect();
        assert!(last.len() >= 2, "{found:#?}");
        for passage in &last {
            assert!(passage.text.split(' ').all(|w| w == "lorem"), "{passage:?}");
        }
        let lorems: usize = last.iter().map(|p| p.text.split(' ').count()).sum();
        assert_eq!(lorems, 300, "no word lost or cut");
    }

    #[test]
    fn a_line_longer_than_a_passage_is_split_on_character_boundaries() {
        let long = "é".repeat(PASSAGE_BYTES);
        let found = passages("Note.md", &long, "Note");
        assert!(found.len() >= 2);
        assert!(
            found
                .iter()
                .all(|p| p.text.len() <= PASSAGE_BYTES && p.start == 1 && p.end == 1)
        );
        assert_eq!(
            found.iter().map(|p| p.text.as_str()).collect::<String>(),
            long
        );
    }

    #[test]
    fn sentences_end_at_a_stop_with_space_after() {
        assert_eq!(
            sentences("One. Two?  Three! v1.2 is out. e.g.this"),
            vec!["One.", "Two?", "Three!", "v1.2 is out.", "e.g.this"]
        );
        assert_eq!(sentences("Ends here. "), vec!["Ends here."]);
    }

    #[test]
    fn chinese_and_japanese_sentences_end_at_their_own_stops() {
        assert_eq!(
            sentences("鍵を交換する。記録する！ 本当？！「はい。」記録した．終わり"),
            vec![
                "鍵を交換する。",
                "記録する！",
                "本当？！",
                "「はい。」",
                "記録した．",
                "終わり"
            ],
            "with a space after or without, a run of stops and the quote they close kept together"
        );
        let line = "鍵は毎年交換して記録する。".repeat(60);
        let found = passages("J.md", &line, "J");
        assert!(found.len() >= 2, "{found:#?}");
        assert!(
            found
                .iter()
                .all(|p| p.text.ends_with('。') && p.text.len() <= PASSAGE_BYTES),
            "a line too long for a passage is cut between its sentences, not through a word: \
             {found:#?}"
        );
    }

    #[test]
    fn a_hash_in_a_code_block_is_a_line_of_it_not_a_heading() {
        let pad = "padding ".repeat(30);
        let note = format!(
            "## Setup\n\nInstall it like this. {pad}\n\n```bash\n# install the dependencies\nnpm install\n```\n\nMore about setup after the block. {pad}\n\n~~~\n## not a section either\n~~~\n\nThe end of setup. {pad}\n"
        );
        let found = passages("Note.md", &note, "Note");
        let titles: Vec<String> = found.iter().map(|p| p.title("Note")).collect();
        assert!(
            titles.iter().all(|t| t == "Note > Setup"),
            "{titles:?}: {found:#?}"
        );
        assert!(
            found.iter().any(|p| p
                .text
                .contains("```bash\n# install the dependencies\nnpm install\n```")),
            "the block stays whole: {found:#?}"
        );
    }

    #[test]
    fn a_first_heading_that_repeats_the_title_is_left_out_and_lines_still_count() {
        let note =
            "---\ntitle: Release Process\n---\n\n# release process \n\nTag it.\n\nPush it.\n";
        let found = passages("Release.md", note, "Release Process");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "Tag it.\n\nPush it.");
        assert_eq!((found[0].start, found[0].end), (7, 9), "file lines");

        let other = passages("Release.md", "# Something else\n\nTag it.\n", "Release");
        assert!(
            other[0].text.starts_with("# Something else"),
            "a heading saying more than the title stays"
        );
        let stub = passages("Release.md", "# Release\n", "Release");
        assert_eq!(stub.len(), 1, "a note that is only its title keeps it");
        assert_eq!(stub[0].text, "# Release");
        let hash = passages("Sharp.md", "# C#\n\nIt compiles.\n", "C#");
        assert_eq!(hash[0].text, "It compiles.", "the title as written");
    }

    #[test]
    fn each_passage_knows_the_headings_it_sits_under() {
        let note = format!(
            "# Guide\n\n## Setup\n\n{}\n\n### Keys\n\n{}\n\n{}\n\n## Use [[Tools/Shell|the shell]]\n\n{}\n\n# Appendix\n\n{}\n",
            "a".repeat(300),
            "b".repeat(700),
            "c".repeat(700),
            "d".repeat(300),
            "e".repeat(300),
        );
        let found = passages("Guide.md", &note, "Guide");
        let headings: Vec<Vec<&str>> = found
            .iter()
            .map(|p| p.headings.iter().map(String::as_str).collect())
            .collect();
        assert_eq!(
            headings,
            vec![
                vec!["Setup"],
                vec!["Setup", "Keys"],
                vec!["Setup", "Keys"],
                vec!["Use the shell"],
                vec![],
            ],
            "{found:#?}"
        );
        assert_eq!(found[1].title("Guide"), "Guide > Setup > Keys");
        assert_eq!(found[4].title("Guide"), "Guide");
        assert!(
            !found[0].text.contains("# Guide"),
            "the title heading is left out"
        );
    }

    #[test]
    fn links_read_as_the_words_they_show() {
        assert_eq!(
            readable("see [[Projects/Auth|the auth note]] and [[Decisions/Hashing#Why]]"),
            "see the auth note and Hashing"
        );
        assert_eq!(readable("broken [[link"), "broken [[link");
    }

    #[test]
    fn identifiers_are_terms_whole_and_in_parts() {
        assert_eq!(
            terms("Call parse_frontmatter, then HTTPServer.start()"),
            vec![
                "call",
                "parse_frontmatter",
                "parse",
                "frontmatter",
                "then",
                "httpserver",
                "http",
                "server",
                "start"
            ]
        );
        assert_eq!(
            terms("parseFrontMatter"),
            vec!["parsefrontmatter", "parse", "front", "matter"]
        );
        assert_eq!(
            terms("a I x9 é"),
            vec!["x9"],
            "single letters are not terms"
        );
        assert!(
            terms(&"k".repeat(80)).is_empty(),
            "nor are base64-length runs"
        );
    }

    #[test]
    fn words_are_stemmed_and_identifiers_and_versions_are_not() {
        assert_eq!(
            index_terms("Hashing passwords hashed with parse_frontmatter in v2"),
            vec![
                "hash",
                "password",
                "hash",
                "with",
                "parse_frontmatter",
                "pars",
                "frontmatt",
                "in",
                "v2"
            ]
        );
        // Snowball's own examples, as its Python package stems them.
        for (word, stemmed) in [
            ("running", "run"),
            ("generously", "generous"),
            ("happiness", "happi"),
            ("studies", "studi"),
            ("dying", "die"),
            ("skies", "sky"),
            ("news", "news"),
            ("proceeding", "proceed"),
            ("relational", "relat"),
            ("succeeded", "succeed"),
            ("hopping", "hop"),
            ("café", "café"),
        ] {
            assert_eq!(stem(word.to_string()), stemmed, "{word}");
        }
    }

    #[test]
    fn a_question_drops_the_words_that_only_hold_it_together() {
        assert_eq!(
            question_terms("How are the passwords hashed?"),
            vec!["password", "hash"]
        );
        assert_eq!(
            question_terms("Are canned beans healthy?"),
            vec!["can", "bean", "healthi"],
            "a word is dropped as it is written, not as it stems"
        );
        assert_eq!(
            question_terms("how are they"),
            vec!["how", "are", "they"],
            "a question of nothing else keeps them"
        );
        assert!(
            STOPWORDS.windows(2).all(|pair| pair[0] < pair[1]),
            "sorted, to be searched"
        );
    }

    #[test]
    fn scripts_without_spaces_are_indexed_in_pairs() {
        assert_eq!(
            terms("東京都に住む"),
            vec!["東京", "京都", "都に", "に住", "住む"]
        );
        assert_eq!(
            terms("rust 語"),
            vec!["rust", "語"],
            "a lone character stands alone"
        );
        assert_eq!(terms("검색엔진"), vec!["검색", "색엔", "엔진"]);
        assert_eq!(
            index_terms("東京都"),
            vec!["東京", "京都"],
            "and not stemmed"
        );
    }

    #[test]
    fn token_estimates_err_on_the_generous_side() {
        assert_eq!(tokens("abcd"), 1);
        assert_eq!(tokens("abcde"), 2);
        assert_eq!(tokens("東京"), 2);
        assert_eq!(tokens(""), 0);
    }
}
