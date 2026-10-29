//! The pieces of Mermaid's syntax that more than one kind of diagram reads:
//! keywords, directions, quotes, labels in brackets and annotations, and the
//! things a diagram names.

use std::collections::HashMap;

use super::layered::Direction;

/// What follows `word` at the start of a line, when the line starts with it
/// as a word of its own.
#[must_use]
pub fn keyword<'a>(line: &'a str, word: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(word)?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
}

/// A statement's first word, up to a space or a colon, which names the
/// statements a diagram passes over, such as `style` and `accTitle:`.
#[must_use]
pub fn first_word(line: &str) -> &str {
    line.split(|c: char| c.is_whitespace() || c == ':')
        .next()
        .unwrap_or("")
}

/// The way `TB`, `TD`, `BT`, `LR` and `RL` say a diagram flows.
#[must_use]
pub fn direction(name: &str) -> Option<Direction> {
    match name {
        "TB" | "TD" => Some(Direction::Down),
        "BT" => Some(Direction::Up),
        "LR" => Some(Direction::Right),
        "RL" => Some(Direction::Left),
        _ => None,
    }
}

/// Text without the spaces around it, nor the double quotes, if any.
#[must_use]
pub fn unquote(text: &str) -> &str {
    let text = text.trim();
    text.strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(text)
}

/// Where `needle` first comes in `text` outside quotes, each of `quotes`
/// opening one and closing it.
#[must_use]
pub fn outside_quotes(text: &str, needle: &str, quotes: &[char]) -> Option<usize> {
    let mut quoted = false;
    for (at, c) in text.char_indices() {
        if quotes.contains(&c) {
            quoted = !quoted;
        } else if !quoted && text[at..].starts_with(needle) {
            return Some(at);
        }
    }
    None
}

/// A statement split at its first colon outside quotes into what comes
/// before it and its label, which is empty when there is none.
#[must_use]
pub fn label_after<'a>(line: &'a str, quotes: &[char]) -> (&'a str, &'a str) {
    match outside_quotes(line, ":", quotes) {
        Some(at) => (&line[..at], line[at + 1..].trim()),
        None => (line, ""),
    }
}

/// Text split at each `separator` outside double quotes, or `None` when a
/// quote is left open.
#[must_use]
pub fn split_outside_quotes(text: &str, separator: char) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (at, c) in text.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if c == separator && !quoted {
            parts.push(&text[start..at]);
            start = at + 1;
        }
    }
    parts.push(&text[start..]);
    (!quoted).then_some(parts)
}

/// A name and the label in brackets after it, as `p[Person]`, or `None` when
/// the bracket isn't closed at the end.
#[must_use]
pub fn bracketed(text: &str) -> Option<(&str, Option<&str>)> {
    match text.find('[') {
        Some(open) if text.ends_with(']') => {
            Some((&text[..open], Some(&text[open + 1..text.len() - 1])))
        }
        Some(_) => None,
        None => Some((text, None)),
    }
}

/// Text and the annotation in `<<…>>` in it, as `Shape <<interface>>`: what
/// comes before the annotation, trimmed, and its inside as written, or
/// `None` when it isn't closed.
#[must_use]
pub fn annotated(text: &str) -> Option<(&str, Option<&str>)> {
    let Some(open) = text.find("<<") else {
        return Some((text, None));
    };
    let close = text[open..].find(">>")? + open;
    Some((text[..open].trim(), Some(&text[open + 2..close])))
}

/// Where the thing written `id` is among `items`, made by `make` the first
/// time it is named, or `None` when there are `most` already.
pub fn named<T>(
    ids: &mut HashMap<String, usize>,
    items: &mut Vec<T>,
    id: &str,
    most: usize,
    make: impl FnOnce() -> T,
) -> Option<usize> {
    if let Some(&at) = ids.get(id) {
        return Some(at);
    }
    if items.len() >= most {
        return None;
    }
    items.push(make());
    ids.insert(id.to_string(), items.len() - 1);
    Some(items.len() - 1)
}
