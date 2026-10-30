//! Typesets TeX with utftex, the real engine, for the reading pane.
//!
//! [`crate::ui::math`] sets maths by hand in Unicode: it reads well for what it
//! covers, but it is a reimplementation, and an equation that uses something it
//! does not know is shown as typed. utftex — `libtexprintf`, the C library the
//! `utftex` command line tool is built on — parses the TeX properly and draws
//! it in boxes, which is what this module wraps.
//!
//! The library is vendored (`vendor/libtexprintf`, tag `v.1.31`, GPL-3.0-only)
//! and built by `build.rs`; there is no `utftex` binary to install and nothing
//! is fetched at run time.
//!
//! # What the wrapper has to be careful about
//!
//! * The library keeps mutable globals (`TEXPRINTF_ERR`, the error flags, the
//!   box position cache), so every call is serialised behind one mutex.
//! * `texstring` temporarily NUL-terminates a key in the string it is given
//!   while lexing, then restores it: the final contents are unchanged, but the
//!   buffer has to be writable, so it is always handed a mutable copy rather
//!   than a Rust string's bytes.
//! * Its result is `malloc`ed and must be released with `texfree`. [`run`]
//!   owns the pointer for the few lines between the call and the free, and
//!   frees it on every path, including a failed call.
//! * The result is bytes, not `str`; anything that is not UTF-8 is treated as a
//!   failed render rather than being lossily converted, because the fallback
//!   renderer does better than replacement characters.
//! * An error is reported through a global flag *after* the call, so it is
//!   read while the mutex is still held, and a render with any error is
//!   discarded: a half-parsed formula reads worse than the hand-set one.
//! * The engine is not safe on every input. A leading `\over`/`\choose`
//!   segfaults it, deep nesting overflows its stack, and enough `\over` or
//!   `\choose` commands stall it for seconds. [`unsafe_for_engine`] refuses
//!   those before the call, and [`crate::ui::math`] sets them instead.
//!
//! Nothing unwinds between the call and the free: the UTF-8 conversion is
//! fallible, so its result is owned before the pointer is released, and the
//! only unwind that can reach here comes from a poisoned mutex, which is
//! recovered rather than propagated.

// The one place in the crate that calls into C. The rest of the binary is
// `#![deny(unsafe_code)]`; this module opts back in because a typesetting
// engine cannot be called any other way. The `unsafe` uses below are the whole
// surface: the three calls into C, the result pointer, and the free.
#![allow(unsafe_code)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};

use unicode_width::UnicodeWidthStr;

/// Which sort of maths a formula is.
///
/// utftex sets both the same way, so this only tells the cache apart — and
/// keeps the door open for a per-mode line width if one is ever wanted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// `$…$` in a line of prose.
    Inline,
    /// `$$…$$`, or a `math` code fence, on rows of its own.
    Display,
}

/// A formula utftex set, as rows of terminal text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    rows: Vec<String>,
    width: usize,
}

impl Rendered {
    /// The rows, top to bottom, with any trailing blank rows removed.
    #[must_use]
    pub fn rows(&self) -> &[String] {
        &self.rows
    }

    /// The widest row, in display columns.
    #[must_use]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Whether the formula came out on a single row.
    ///
    /// Inline maths in a paragraph can only use a one-row setting; a fraction
    /// or a root that takes several rows is left to [`crate::ui::math`], which
    /// flattens it onto the line.
    #[must_use]
    pub fn is_single_row(&self) -> bool {
        self.rows.len() == 1
    }
}

/// How many formulas are kept set. A note holds far fewer than this, and a
/// reflow at a new width re-renders every formula on screen; the cap only
/// stops a long reading session from growing without bound.
///
/// The cache is here because the engine is not free: measured on an x86-64
/// Linux machine, one render of `x^2+1` takes 6 µs, of `\frac{a+b}{2}` 12 µs
/// and of `\int_0^1 \frac{x^2}{1+x}\,dx` 38 µs. The reading pane already
/// caches a whole laid-out note, so an unchanged frame never reaches this; a
/// frame that does — a resize, a theme change, a different pane width — lays
/// out every formula on screen again, and a page of thirty equations would
/// otherwise spend up to a millisecond in the C engine each time. A hit here
/// is a hash lookup.
const CACHE_LIMIT: usize = 256;

/// The one lock every call into the library holds.
///
/// The library's globals are not per-thread and not reset by it, so two
/// renders at once would corrupt each other's boxes and error flags.
static ENGINE: Mutex<()> = Mutex::new(());

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(Cache::default()));

/// A formula set once, and when it was last asked for.
struct Entry {
    outcome: Outcome,
    used: u64,
}

#[derive(Clone)]
enum Outcome {
    Set(Arc<Rendered>),
    Failed,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<(Mode, String), Entry>,
    clock: u64,
}

/// Typesets `tex`, or returns `None` if utftex could not set it.
///
/// Results are cached by mode and TeX source, failures included: a formula
/// utftex rejects is not retried on every frame, and the reading pane's own
/// layout cache means a frame that has not changed does not call this at all.
#[must_use]
pub fn typeset(tex: &str, mode: Mode) -> Option<Arc<Rendered>> {
    let key = (mode, tex.to_owned());
    if let Some(outcome) = cache_lookup(&key) {
        return match outcome {
            Outcome::Set(set) => Some(set),
            Outcome::Failed => None,
        };
    }

    let outcome = match run(tex) {
        Some(set) => Outcome::Set(Arc::new(set)),
        None => Outcome::Failed,
    };
    cache_store(key, outcome.clone());
    match outcome {
        Outcome::Set(set) => Some(set),
        Outcome::Failed => None,
    }
}

/// Typesets `tex`, without the cache. `None` on any error, including empty
/// output.
fn run(tex: &str) -> Option<Rendered> {
    // Never hand the engine an input that crashes it or that it cannot set in
    // a frame's time; the hand renderer sets those instead. See
    // `unsafe_for_engine`.
    if unsafe_for_engine(tex) {
        return None;
    }
    // An interior NUL cannot be handed to a C string, and is not TeX either.
    let input = CString::new(tex).ok()?;
    // A writable copy: the lexer NUL-terminates a key in this buffer while it
    // compares it, then restores the byte. A Rust string's bytes are read-only.
    let mut buffer = input.into_bytes_with_nul();

    let _engine = lock(&ENGINE);
    // SAFETY: `buffer` is a NUL-terminated, writable copy of `tex` that
    // outlives the call. `texstring` reads and writes it in place and returns
    // either a freshly `malloc`ed, NUL-terminated string or null.
    let raw = unsafe { texstring(buffer.as_mut_ptr().cast()) };
    // SAFETY: the error state is a global that the call above has just set;
    // reading it under the same lock is what makes it ours rather than a
    // concurrent render's.
    let failed = unsafe { texerror_state() } != 0;
    if raw.is_null() {
        return None;
    }

    // SAFETY: `raw` is non-null and NUL-terminated, as `texstring` promises.
    // The conversion is fallible, so it happens before the free but the result
    // is owned by then; nothing below can unwind while `raw` is live.
    let utf8 = unsafe { CStr::from_ptr(raw) }
        .to_str()
        .ok()
        .map(str::to_owned);
    // SAFETY: `raw` came from `texstring`'s `malloc` and is freed exactly once.
    unsafe { texfree(raw.cast()) };

    if failed {
        return None;
    }
    let text = utf8?;
    let mut rows: Vec<String> = text
        .split('\n')
        .map(|row| row.trim_end_matches('\r').to_owned())
        .collect();
    // `DrawBox` already leaves off the last row's newline, but a stray blank
    // row here would become a gap under an equation, so trim defensively.
    while rows.last().is_some_and(|row| row.trim().is_empty()) {
        rows.pop();
    }
    if rows.is_empty() {
        return None;
    }
    let width = rows.iter().map(|row| row.width()).max().unwrap_or(0);
    Some(Rendered { rows, width })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic elsewhere while the engine lock was held would leave a formula
    // half-parsed; the globals are reset at the start of every call, so the
    // guard is taken back rather than panicking the reader out of the app.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn cache_lookup(key: &(Mode, String)) -> Option<Outcome> {
    let mut cache = lock(&CACHE);
    cache.clock += 1;
    let clock = cache.clock;
    let entry = cache.entries.get_mut(key)?;
    entry.used = clock;
    Some(entry.outcome.clone())
}

fn cache_store(key: (Mode, String), outcome: Outcome) {
    let mut cache = lock(&CACHE);
    cache.clock += 1;
    let used = cache.clock;
    if cache.entries.len() >= CACHE_LIMIT && !cache.entries.contains_key(&key) {
        // Drop the least recently asked for. Scanning is fine at this size,
        // and only happens when the cache is full.
        if let Some(oldest) = cache
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.used)
            .map(|(key, _)| key.clone())
        {
            cache.entries.remove(&oldest);
        }
    }
    cache.entries.insert(key, Entry { outcome, used });
}

/// How many formulas are cached, for the tests that check the cap.
#[cfg(test)]
pub(crate) fn cache_len() -> usize {
    lock(&CACHE).entries.len()
}

/// `tex` with any `\tag{…}` (or `\tag*{…}`) removed.
///
/// utftex has no `\tag`; it reports an unknown command and drops everything
/// after it. The reading pane shows the number itself, from the hand renderer,
/// so the body is set here and the tag added beside it.
#[must_use]
pub fn without_tag(tex: &str) -> Cow<'_, str> {
    let mut out = String::new();
    let mut rest = tex;
    let mut stripped = false;
    while let Some(at) = rest.find("\\tag") {
        let after = &rest[at + "\\tag".len()..];
        let after = after.strip_prefix('*').unwrap_or(after);
        let body = after.trim_start();
        if !body.starts_with('{') {
            // `\tagged`, `\tags`: not the command. Keep it and look further on.
            let end = at + "\\tag".len();
            out.push_str(&rest[..end]);
            rest = &rest[end..];
            continue;
        }
        out.push_str(&rest[..at]);
        // The argument ends at its matching brace; TeX allows `\tag{\text{a}}`.
        let mut depth = 0usize;
        let mut end = None;
        for (i, ch) in body.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = Some(i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            // Unbalanced: leave the rest as it was typed.
            out.push_str(&rest[at..]);
            return Cow::Owned(out);
        };
        rest = &body[end..];
        stripped = true;
    }
    if !stripped {
        return Cow::Borrowed(tex);
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// The longest TeX handed to the engine, in bytes.
///
/// A formula in a note is a few dozen bytes; this is two orders of magnitude
/// more, and it bounds both the work a flat formula can ask for and the size
/// of the rows it returns.
const MAX_INPUT: usize = 4096;

/// How deeply braces may nest before the formula is left to `math`.
///
/// The engine recurses once per group, and its work is superlinear in the
/// depth for some constructs: measured on this machine, 24 nested `\sqrt`
/// takes 30 ms, 32 takes 110 ms, 40 takes 300 ms, and a hundred does not
/// finish in ten seconds. Real formulas nest a handful deep.
const MAX_NESTING: usize = 24;

/// How many `\over` or `\choose` commands a formula may hold.
///
/// Both are rewritten before parsing by a pass whose cost grows sharply with
/// the number of occurrences: 64 `\choose` take 0.35 s, 128 take 5.4 s, and
/// 200 do not finish in eight. `\over` is cheaper but shares the pass, and
/// more than a handful of either in one formula is not something a note has.
const MAX_RELATIONS: usize = 16;

/// Whether `tex` must not be handed to libtexprintf 1.31.
///
/// Three things are refused, each for a measured reason rather than a guess:
/// a leading `\over`/`\choose` (segfault), deep nesting or a huge input
/// (stack overflow or a hang), and many `\over`/`\choose` (a hang). The
/// hand renderer sets whatever is refused, so the formula still reads.
fn unsafe_for_engine(tex: &str) -> bool {
    tex.len() > MAX_INPUT
        || nesting_depth(tex) > MAX_NESTING
        || relation_commands(tex) > MAX_RELATIONS
        || leading_relation_crash(tex)
}

/// The deepest run of unescaped `{` in `tex`.
///
/// `\{` is a literal brace and does not open a group.
fn nesting_depth(tex: &str) -> usize {
    let mut depth = 0usize;
    let mut deepest = 0usize;
    let mut escaped = false;
    for ch in tex.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '{' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    deepest
}

/// How many `\over` and `\choose` commands `tex` holds.
///
/// A command that continues with a letter (`\overline`) is a different
/// command and is not counted.
fn relation_commands(tex: &str) -> usize {
    let mut count = 0;
    let mut rest = tex;
    while let Some(at) = rest.find('\\') {
        let after = &rest[at..];
        for command in ["\\over", "\\choose"] {
            if let Some(tail) = after.strip_prefix(command)
                && !tail
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_alphabetic())
            {
                count += 1;
            }
        }
        rest = &after[1..];
    }
    count
}

/// Whether `tex` is an input that makes libtexprintf 1.31 segfault.
///
/// Upstream bug: `\over` and `\choose` as the *first* token make the lexer
/// slice the numerator from `begin - 1`, one byte before the string, and read
/// out of bounds — a SIGSEGV, not an error flag, so no `catch_unwind` can help
/// and the whole TUI would die with the reader's unsaved work. The same
/// commands after any other token (`a \over b`, `{\over a b}`) and longer
/// commands that merely start with one (`\overline`) are safe.
///
/// The check is deliberately conservative: a leading `\over`/`\choose`
/// followed by anything that is not an ASCII letter (a space, a brace, the
/// end of the string) is refused, and the hand renderer sets the formula
/// instead. Refusing one safe spelling costs a nicer setting, never a crash.
fn leading_relation_crash(tex: &str) -> bool {
    let rest = tex.trim_start();
    for command in ["\\over", "\\choose"] {
        if let Some(after) = rest.strip_prefix(command) {
            // `\overline`, `\overbrace`: the command continues with a letter,
            // so the lexer never takes the one-byte-back slice.
            return !after
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic());
        }
    }
    false
}

unsafe extern "C" {
    /// Sets `input` and returns the rows, `malloc`ed. Briefly writes a NUL
    /// into `input` while lexing, then restores it; the pointer must be
    /// writable but the contents are unchanged afterwards.
    fn texstring(input: *const c_char) -> *mut c_char;
    /// Frees a result of [`texstring`].
    fn texfree(ptr: *mut c_void);
    /// Non-zero when the last call reported an error.
    fn texerror_state() -> c_int;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_formula_is_set_as_utftex_sets_it() {
        let set = run("x^2+1").expect("utftex sets this");
        assert_eq!(set.rows(), ["x²+1"]);
        assert_eq!(set.width(), 4);
        assert!(set.is_single_row());
    }

    #[test]
    fn a_fraction_takes_several_rows() {
        let set = run(r"\frac{a}{b}").expect("utftex sets this");
        assert_eq!(set.rows(), ["a", "─", "b"]);
        assert_eq!(set.width(), 1);
        assert!(!set.is_single_row());
    }

    #[test]
    fn an_unknown_command_is_a_failed_render() {
        assert!(run(r"\notacommand{x}").is_none());
        // And it is not cached as a success.
        assert!(typeset(r"\notacommand{y}", Mode::Display).is_none());
    }

    #[test]
    fn empty_output_is_a_failed_render() {
        assert!(run("").is_none());
        assert!(run("   ").is_none());
    }

    #[test]
    fn a_tag_is_stripped_before_the_engine_sees_it() {
        // utftex does not know `\tag`, so the body is set without it.
        assert!(run(r"E = mc^2 \tag{1}").is_none());
        let set = run(&without_tag(r"E = mc^2 \tag{1}")).expect("the body sets");
        assert_eq!(set.rows(), ["E = mc²"]);
        assert_eq!(without_tag(r"x \tag*{A} y"), "x  y");
        assert_eq!(without_tag(r"\tagged{x}"), r"\tagged{x}");
        assert!(matches!(without_tag("x"), Cow::Borrowed(_)));
    }

    #[test]
    fn line_breaks_become_rows_without_a_blank_one() {
        let set = run("a\\\\b").expect("utftex sets this");
        assert_eq!(set.rows(), ["a", "b"], "one row each, no third");
    }

    #[test]
    fn a_leading_relation_is_kept_from_the_engine() {
        // The upstream SIGSEGV: `\over` or `\choose` as the first token. The
        // predicate is tested rather than the engine, so a regression here
        // fails this test instead of killing the test runner.
        for tex in [
            r"\over a b",
            r"\over",
            r"\choose a b",
            r"\choose",
            r" \over x",
            "\t\\over x",
        ] {
            assert!(
                leading_relation_crash(tex),
                "{tex:?} would crash libtexprintf 1.31"
            );
        }
        for tex in [
            r"\overline{AB}",
            r"\overbrace{a+b}",
            r"\overleftarrow{AB}",
            r"a \over b",
            r"\frac{\over a}{b}",
            r"\chooseX",
        ] {
            assert!(!leading_relation_crash(tex), "{tex:?} is safe");
        }
    }

    #[test]
    fn inputs_that_overflow_the_stack_or_stall_the_engine_are_refused() {
        // The nesting cap is exact: the deepest allowed run passes, one more
        // is refused.
        let nested = |depth: usize| format!("{}x{}", "{".repeat(depth), "}".repeat(depth));
        assert!(!unsafe_for_engine(&nested(MAX_NESTING)));
        assert!(unsafe_for_engine(&nested(MAX_NESTING + 1)));
        // `\{` is a literal brace, not a group.
        assert!(!unsafe_for_engine(r"\{ \{ \{ \{ \{"));

        // The length cap, in bytes.
        assert!(!unsafe_for_engine(&"x".repeat(MAX_INPUT)));
        assert!(unsafe_for_engine(&"x".repeat(MAX_INPUT + 1)));

        // The relation cap, on the commands that the preprocessor rewrites.
        let relations = |n: usize| format!("{}b", "a \\choose ".repeat(n));
        assert!(!unsafe_for_engine(&relations(MAX_RELATIONS)));
        assert!(unsafe_for_engine(&relations(MAX_RELATIONS + 1)));
        let overs = |n: usize| format!("{}b", "a \\over ".repeat(n));
        assert!(!unsafe_for_engine(&overs(MAX_RELATIONS)));
        assert!(unsafe_for_engine(&overs(MAX_RELATIONS + 1)));

        // Long commands that start with one are not relations.
        assert!(!unsafe_for_engine(
            r"\overline{AB} \overbrace{x} \overleftarrow{y}"
        ));
    }

    /// A formula no other test uses, so the global cache cannot be in the way
    /// of what this test is checking.
    fn unique(prefix: &str) -> String {
        format!("{prefix}{{{}}}", std::process::id())
    }

    #[test]
    fn the_cache_returns_the_same_render_and_stays_bounded() {
        let tex = unique("x^");
        let first = typeset(&tex, Mode::Inline).expect("sets");
        let again = typeset(&tex, Mode::Inline).expect("sets");
        assert!(Arc::ptr_eq(&first, &again), "the second ask is a cache hit");
        // The mode is part of the key.
        let display = typeset(&tex, Mode::Display).expect("sets");
        assert!(!Arc::ptr_eq(&first, &display));

        for i in 0..(CACHE_LIMIT + 10) {
            assert!(typeset(&format!("y^{{{i}}}"), Mode::Inline).is_some());
        }
        // Other tests use the cache at the same time, so the cap is what can
        // be asserted, not an exact length.
        assert!(cache_len() <= CACHE_LIMIT, "the cache is capped");
    }

    #[test]
    fn failures_are_cached_too() {
        let tex = format!("\\nope{}", std::process::id());
        assert!(typeset(&tex, Mode::Display).is_none());
        // The miss was recorded as a failure rather than left out, so the
        // next ask is answered without calling the engine again.
        let key = (Mode::Display, tex.clone());
        assert!(matches!(cache_lookup(&key), Some(Outcome::Failed)));
        assert!(typeset(&tex, Mode::Display).is_none());
    }

    #[test]
    fn wide_characters_are_measured_in_columns() {
        let set = run(r"\text{日本}").expect("utftex sets this");
        assert_eq!(set.width(), 4);
    }

    #[test]
    fn an_interior_nul_is_a_failed_render() {
        assert!(run("x\0y").is_none());
    }

    #[test]
    fn a_poisoned_lock_does_not_stop_the_render() {
        // Poison the engine lock from another thread, as a panicking render
        // would, and check the next call still works.
        let _ = std::thread::spawn(|| {
            let _guard = ENGINE.lock().expect("lock");
            panic!("poison the lock");
        })
        .join();
        assert!(run("x").is_some());
    }
}
