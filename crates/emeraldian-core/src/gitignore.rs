//! Just enough of `.gitignore` to leave out what a code project leaves out.
//!
//! A folder that isn't an Obsidian vault is most often a code project, opened
//! to read one of its notes, and what git ignores there is build output and
//! downloads nobody wants listed. The rules are git's own: blank lines and
//! `#` comments are skipped, `!` brings a path back, a trailing `/` means
//! folders only, a pattern with a `/` in it is relative to the file's folder
//! and one without matches a name at any depth, and `*`, `?`, `[a-z]` and
//! `**` match as git matches them.
//!
//! Written here rather than taken from a crate, since the one that does this
//! brings a whole regular expression engine with it, and a bigger program is
//! a slower one to start.

use std::fs;
use std::path::{Path, PathBuf};

/// The rules in one `.gitignore` file.
#[derive(Debug, Clone)]
pub(crate) struct Gitignore {
    /// The folder the file is in, which anchored patterns are relative to.
    dir: PathBuf,
    rules: Vec<Rule>,
}

#[derive(Debug, Clone)]
struct Rule {
    pattern: String,
    /// `!pattern`: brings back what an earlier rule left out.
    keep: bool,
    /// `pattern/`: matches folders only.
    folders_only: bool,
    /// The pattern has a `/` before its end, so it names a path from the
    /// file's folder rather than a name at any depth.
    anchored: bool,
}

impl Gitignore {
    /// Reads the folder's `.gitignore`. `None` when it has none, or none that
    /// says anything.
    pub(crate) fn read(dir: &Path) -> Option<Self> {
        let text = fs::read_to_string(dir.join(".gitignore")).ok()?;
        let ignore = Self::parse(dir, &text);
        (!ignore.rules.is_empty()).then_some(ignore)
    }

    pub(crate) fn parse(dir: &Path, text: &str) -> Self {
        let rules = text.lines().filter_map(Rule::parse).collect();
        Self {
            dir: dir.to_path_buf(),
            rules,
        }
    }

    /// What this file says about a path below its folder: `Some(true)` to
    /// leave it out, `Some(false)` to keep it, `None` when no rule names it.
    /// The last rule that matches decides, as in git.
    pub(crate) fn decide(&self, path: &Path, is_dir: bool) -> Option<bool> {
        let rel = path.strip_prefix(&self.dir).ok()?;
        let rel = rel.to_str()?;
        // Rules are written with `/`, whatever the system's separator.
        let rel = rel.replace(std::path::MAIN_SEPARATOR, "/");
        let name = rel.rsplit('/').next().unwrap_or(&rel);

        self.rules.iter().rev().find_map(|rule| {
            if rule.folders_only && !is_dir {
                return None;
            }
            let subject = if rule.anchored { rel.as_str() } else { name };
            matches(rule.pattern.as_bytes(), subject.as_bytes()).then_some(!rule.keep)
        })
    }
}

impl Rule {
    fn parse(line: &str) -> Option<Self> {
        // Trailing spaces are dropped unless escaped; a trailing `\ ` is rare
        // enough not to be worth keeping.
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (keep, line) = match line.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        let (folders_only, line) = match line.strip_suffix('/') {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        let anchored = line.contains('/');
        let pattern = line.strip_prefix('/').unwrap_or(line);
        if pattern.is_empty() {
            return None;
        }
        Some(Self {
            pattern: pattern.to_string(),
            keep,
            folders_only,
            anchored,
        })
    }
}

/// Whether a glob matches a whole `/`-separated path, the way git matches.
fn matches(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.first() {
        None => text.is_empty(),
        Some(b'*') if pattern.get(1) == Some(&b'*') => {
            let rest = &pattern[2..];
            match rest.first() {
                // `**` at the end matches everything that is left.
                None => true,
                // `**/` matches any number of folders, none included.
                Some(b'/') => {
                    let rest = &rest[1..];
                    matches(rest, text)
                        || text
                            .iter()
                            .enumerate()
                            .any(|(at, &byte)| byte == b'/' && matches(rest, &text[at + 1..]))
                }
                // Anywhere else, git treats `**` as `*`.
                Some(_) => star(rest, text),
            }
        }
        Some(b'*') => star(&pattern[1..], text),
        Some(b'?') => match text.first() {
            Some(&byte) if byte != b'/' => matches(&pattern[1..], &text[1..]),
            _ => false,
        },
        Some(b'[') => match (class(&pattern[1..]), text.first()) {
            (Some((set, rest)), Some(&byte)) => {
                byte != b'/' && set.contains(byte) && matches(rest, &text[1..])
            }
            (Some(_), None) => false,
            // No closing `]`: an ordinary `[`.
            (None, _) => literal(b'[', &pattern[1..], text),
        },
        Some(b'\\') if pattern.len() > 1 => literal(pattern[1], &pattern[2..], text),
        Some(&byte) => literal(byte, &pattern[1..], text),
    }
}

/// `*`: anything within one folder name, then the rest of the pattern.
fn star(rest: &[u8], text: &[u8]) -> bool {
    let within = text
        .iter()
        .position(|&byte| byte == b'/')
        .unwrap_or(text.len());
    (0..=within).any(|at| matches(rest, &text[at..]))
}

fn literal(byte: u8, rest: &[u8], text: &[u8]) -> bool {
    text.first() == Some(&byte) && matches(rest, &text[1..])
}

/// A `[...]` class.
struct Class<'a> {
    body: &'a [u8],
    negated: bool,
}

impl Class<'_> {
    fn contains(&self, byte: u8) -> bool {
        let mut found = false;
        let mut at = 0;
        while at < self.body.len() {
            let low = self.body[at];
            if at + 2 < self.body.len() && self.body[at + 1] == b'-' {
                found |= (low..=self.body[at + 2]).contains(&byte);
                at += 3;
            } else {
                found |= low == byte;
                at += 1;
            }
        }
        found != self.negated
    }
}

/// Reads a class from just after its `[`, returning it and what follows its
/// `]`. A `]` straight after the `[` (or `[!`) is part of the class.
fn class(pattern: &[u8]) -> Option<(Class<'_>, &[u8])> {
    let (negated, body) = match pattern.first() {
        Some(b'!' | b'^') => (true, &pattern[1..]),
        _ => (false, pattern),
    };
    let close = body
        .iter()
        .skip(1)
        .position(|&byte| byte == b']')
        .map(|at| at + 1)?;
    Some((
        Class {
            body: &body[..close],
            negated,
        },
        &body[close + 1..],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ignore(text: &str) -> Gitignore {
        Gitignore::parse(Path::new("/p"), text)
    }

    fn says(ignore: &Gitignore, rel: &str, is_dir: bool) -> Option<bool> {
        ignore.decide(&Path::new("/p").join(rel), is_dir)
    }

    #[test]
    fn a_name_matches_at_any_depth() {
        let rules = ignore("*.log\nnode_modules\n");
        assert_eq!(says(&rules, "debug.log", false), Some(true));
        assert_eq!(says(&rules, "a/b/debug.log", false), Some(true));
        assert_eq!(says(&rules, "a/node_modules", true), Some(true));
        assert_eq!(says(&rules, "a/notes.md", false), None);
        assert_eq!(says(&rules, "log", false), None);
    }

    #[test]
    fn a_slash_anchors_a_pattern_to_its_folder() {
        let rules = ignore("/dist\ndocs/build\n");
        assert_eq!(says(&rules, "dist", true), Some(true));
        assert_eq!(says(&rules, "src/dist", true), None);
        assert_eq!(says(&rules, "docs/build", true), Some(true));
        assert_eq!(says(&rules, "x/docs/build", true), None);
    }

    #[test]
    fn a_trailing_slash_means_folders_only() {
        let rules = ignore("cache/\n");
        assert_eq!(says(&rules, "cache", true), Some(true));
        assert_eq!(says(&rules, "cache", false), None);
    }

    #[test]
    fn the_last_rule_that_matches_decides() {
        let rules = ignore("*.md\n!README.md\n");
        assert_eq!(says(&rules, "notes.md", false), Some(true));
        assert_eq!(says(&rules, "README.md", false), Some(false));
    }

    #[test]
    fn double_stars_cross_folders_and_single_ones_do_not() {
        let rules = ignore("a/**/z.md\nlogs/**\n/x/*.md\n**/tmp\n");
        assert_eq!(says(&rules, "a/z.md", false), Some(true));
        assert_eq!(says(&rules, "a/b/c/z.md", false), Some(true));
        assert_eq!(says(&rules, "logs/one/two.txt", false), Some(true));
        assert_eq!(says(&rules, "logs", true), None);
        assert_eq!(says(&rules, "x/y.md", false), Some(true));
        assert_eq!(says(&rules, "x/y/z.md", false), None);
        assert_eq!(says(&rules, "deep/down/tmp", true), Some(true));
        assert_eq!(says(&rules, "tmp", true), Some(true));
    }

    #[test]
    fn classes_question_marks_and_escapes() {
        let rules = ignore("file[0-9].txt\n[!a]?.md\n\\#hash\n");
        assert_eq!(says(&rules, "file7.txt", false), Some(true));
        assert_eq!(says(&rules, "fileX.txt", false), None);
        assert_eq!(says(&rules, "bc.md", false), Some(true));
        assert_eq!(says(&rules, "ac.md", false), None);
        assert_eq!(says(&rules, "#hash", false), Some(true));
    }

    #[test]
    fn comments_blank_lines_and_odd_patterns_are_harmless() {
        let rules = ignore("# a comment\n\n   \n/\n!\n[unclosed\n");
        assert_eq!(says(&rules, "[unclosed", false), Some(true));
        assert_eq!(says(&rules, "anything", false), None);
        assert!(ignore("# only a comment\n").rules.is_empty());
    }
}
