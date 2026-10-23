//! The vault's links, resolved once when the index changes rather than on
//! every question.
//!
//! A link is written as a name — a path, a file name or an alias — and what a
//! name means depends on every other note, so resolving them reads the whole
//! catalog: on a large vault, more work than the rest of a question. The
//! index resolves them whenever it is written and keeps the result beside its
//! segments: for each live note, in the order [`View`] lists them, the notes
//! it links to and the notes that link to it, as runs of numbers.
//!
//! ```text
//! header    magic, version, the index generation it was made for, notes,
//!           and how many links each way
//! outgoing  where each note's targets start, and one more for the end;
//!           then the targets
//! incoming  where each note's sources start, and one more for the end;
//!           then the sources
//! ```
//!
//! The generation is the index's own count of its writes: a file made for
//! another one is out of date, and a question resolves the links itself.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::hash::{BuildHasherDefault, Hasher};
use std::io::{self, BufWriter, Read, Write};
use std::ops::Range;
use std::path::Path;

use super::store::View;

const MAGIC: &[u8; 8] = b"EMRLLNK1";
const VERSION: u32 = 1;
/// Magic, version, generation, notes, then how many links each way.
const HEADER_BYTES: usize = 8 + 4 + 8 + 4 + 4 + 4;

/// The links between live notes, both ways, by their places in
/// [`View::notes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Links {
    /// Where each note's targets start in `outgoing`, and one more for the
    /// end.
    outgoing_starts: Vec<u32>,
    outgoing: Vec<u32>,
    /// Where each note's sources start in `incoming`, and one more.
    incoming_starts: Vec<u32>,
    incoming: Vec<u32>,
}

impl Links {
    /// Resolves every live note's links among the live notes, the way a
    /// wikilink is read: a path first, then a file name, then an alias,
    /// whatever their case.
    ///
    /// Every name is lowercased once, into one string, and looked up with a
    /// quick hash, and the notes' links are read where the catalog holds
    /// them and resolved on several threads: on a large vault, this is what
    /// a question that follows an edit waits for.
    #[must_use]
    pub fn resolve(view: &View) -> Self {
        let count = view.notes.len();
        // Every note's path and every alias, lowercased, end to end.
        let mut arena = String::new();
        let mut paths: Vec<(usize, usize)> = Vec::with_capacity(count);
        let mut aliases: Vec<(usize, usize, u32)> = Vec::new();
        for (place, &key) in view.notes.iter().enumerate() {
            let note = view.note(key);
            let start = arena.len();
            push_lowercase(&mut arena, &note.rel);
            paths.push((start, arena.len()));
            note.each_alias(|alias| {
                let start = arena.len();
                push_lowercase(&mut arena, alias);
                aliases.push((start, arena.len(), place as u32));
            });
        }

        // Weaker names go in first, for stronger ones to replace.
        let mut names: Names<'_> =
            HashMap::with_capacity_and_hasher(count * 3 + aliases.len(), Default::default());
        for &(start, end, place) in &aliases {
            names.entry(&arena[start..end]).or_insert(place);
        }
        let mut stems: HashSet<&str, BuildHasherDefault<Quick>> =
            HashSet::with_capacity_and_hasher(count, Default::default());
        for (place, &(start, end)) in paths.iter().enumerate() {
            let rel = &arena[start..end];
            let path = rel.strip_suffix(".md").unwrap_or(rel);
            let stem = path.rsplit('/').next().unwrap_or(path);
            if stems.insert(stem) {
                names.insert(stem, place as u32);
            }
        }
        for (place, &(start, end)) in paths.iter().enumerate() {
            let rel = &arena[start..end];
            if let Some(path) = rel.strip_suffix(".md") {
                names.insert(path, place as u32);
            }
            names.insert(rel, place as u32);
        }

        // Each note's targets, in order, a share of the notes to a thread.
        let threads = if count < PARALLEL_NOTES {
            1
        } else {
            std::thread::available_parallelism()
                .map_or(1, std::num::NonZero::get)
                .min(RESOLVE_THREADS)
        };
        let share = count.div_ceil(threads).max(1);
        let names = &names;
        let pieces: Vec<(Vec<u32>, Vec<u32>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..count)
                .step_by(share)
                .map(|from| {
                    scope.spawn(move || resolve_range(view, names, from..(from + share).min(count)))
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
                .collect()
        });
        let mut outgoing_starts = Vec::with_capacity(count + 1);
        outgoing_starts.push(0);
        let mut outgoing: Vec<u32> = Vec::with_capacity(pieces.iter().map(|(_, t)| t.len()).sum());
        for (ends, targets) in pieces {
            let base = outgoing.len() as u32;
            outgoing_starts.extend(ends.into_iter().map(|end| base + end));
            outgoing.extend(targets);
        }
        let mut sources = vec![0u32; count];
        for &to in &outgoing {
            sources[to as usize] += 1;
        }

        // The same links turned round, each note's sources in order.
        let mut incoming_starts = Vec::with_capacity(count + 1);
        incoming_starts.push(0u32);
        for &n in &sources {
            incoming_starts.push(incoming_starts[incoming_starts.len() - 1] + n);
        }
        let mut filled: Vec<u32> = incoming_starts[..count].to_vec();
        let mut incoming = vec![0u32; outgoing.len()];
        for from in 0..count {
            let row = outgoing_starts[from] as usize..outgoing_starts[from + 1] as usize;
            for &to in &outgoing[row] {
                incoming[filled[to as usize] as usize] = from as u32;
                filled[to as usize] += 1;
            }
        }
        Self {
            outgoing_starts,
            outgoing,
            incoming_starts,
            incoming,
        }
    }

    /// How many notes it covers.
    #[must_use]
    pub fn notes(&self) -> usize {
        self.outgoing_starts.len() - 1
    }

    /// The notes `note` links to, other than itself, in order.
    #[must_use]
    pub fn outgoing(&self, note: usize) -> &[u32] {
        &self.outgoing[self.outgoing_starts[note] as usize..self.outgoing_starts[note + 1] as usize]
    }

    /// The notes linking to `note`, other than itself, in order.
    #[must_use]
    pub fn incoming(&self, note: usize) -> &[u32] {
        &self.incoming[self.incoming_starts[note] as usize..self.incoming_starts[note + 1] as usize]
    }

    /// Writes the links to `path`, made for the index's `generation`.
    pub fn write(&self, path: &Path, generation: u64) -> io::Result<()> {
        let staging = path.with_extension(format!("part{}", std::process::id()));
        let result = self
            .write_to(&staging, generation)
            .and_then(|()| fs::rename(&staging, path));
        if result.is_err() {
            let _ = fs::remove_file(&staging);
        }
        result
    }

    fn write_to(&self, path: &Path, generation: u64) -> io::Result<()> {
        let mut out = BufWriter::with_capacity(64 << 10, File::create(path)?);
        out.write_all(MAGIC)?;
        out.write_all(&VERSION.to_le_bytes())?;
        out.write_all(&generation.to_le_bytes())?;
        for count in [self.notes(), self.outgoing.len(), self.incoming.len()] {
            out.write_all(&(count as u32).to_le_bytes())?;
        }
        for list in [
            &self.outgoing_starts,
            &self.outgoing,
            &self.incoming_starts,
            &self.incoming,
        ] {
            for value in list {
                out.write_all(&value.to_le_bytes())?;
            }
        }
        // Not waited for: links lost to a crash are resolved again.
        out.flush()
    }

    /// The links at `path`, if they were made for `generation` of an index
    /// with `notes` live notes and are whole; `None` otherwise, and they are
    /// to be resolved again.
    #[must_use]
    pub fn read(path: &Path, generation: u64, notes: usize) -> Option<Self> {
        // The header first: links made for another generation, as they are
        // after every write, aren't worth reading the rest of.
        let mut file = File::open(path).ok()?;
        let mut header = [0u8; HEADER_BYTES];
        file.read_exact(&mut header).ok()?;
        if &header[..8] != MAGIC {
            return None;
        }
        let u32_at = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().expect("4"));
        let made_for = u64::from_le_bytes(header[12..20].try_into().expect("8"));
        if u32_at(8) != VERSION || made_for != generation || u32_at(20) as usize != notes {
            return None;
        }
        let (outgoing, incoming) = (u32_at(24) as usize, u32_at(28) as usize);
        let words = 2 * (notes + 1) + outgoing + incoming;
        if file.metadata().ok()?.len() != (HEADER_BYTES + 4 * words) as u64 {
            return None;
        }
        let mut bytes = vec![0u8; 4 * words];
        file.read_exact(&mut bytes).ok()?;
        let mut values = bytes
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().expect("4")));
        let links = Self {
            outgoing_starts: values.by_ref().take(notes + 1).collect(),
            outgoing: values.by_ref().take(outgoing).collect(),
            incoming_starts: values.by_ref().take(notes + 1).collect(),
            incoming: values.by_ref().take(incoming).collect(),
        };
        let whole = |starts: &[u32], entries: &[u32]| {
            starts.first() == Some(&0)
                && starts.last() == Some(&(entries.len() as u32))
                && starts.windows(2).all(|pair| pair[0] <= pair[1])
                && entries.iter().all(|&note| (note as usize) < notes)
        };
        (whole(&links.outgoing_starts, &links.outgoing)
            && whole(&links.incoming_starts, &links.incoming))
        .then_some(links)
    }
}

/// Notes below this many are resolved on one thread: starting more would
/// cost more than it saved.
const PARALLEL_NOTES: usize = 2_000;
/// The most threads links are resolved on.
const RESOLVE_THREADS: usize = 8;

/// Every name a note answers to, lowercased, and the note's place.
type Names<'a> = HashMap<&'a str, u32, BuildHasherDefault<Quick>>;

/// The targets of the notes at `places`, resolved: where each note's end in
/// the targets, and the targets, each note's sorted, without itself or a
/// note twice.
fn resolve_range(view: &View, names: &Names<'_>, places: Range<usize>) -> (Vec<u32>, Vec<u32>) {
    let mut ends = Vec::with_capacity(places.len());
    let mut targets: Vec<u32> = Vec::new();
    let mut key = String::new();
    for place in places {
        let from = targets.len();
        view.note(view.notes[place]).each_link(|target| {
            if let Some(&to) = resolve(names, target, &mut key)
                && to != place as u32
            {
                targets.push(to);
            }
        });
        targets[from..].sort_unstable();
        let mut kept = from;
        for k in from..targets.len() {
            if k == from || targets[k] != targets[kept - 1] {
                targets[kept] = targets[k];
                kept += 1;
            }
        }
        targets.truncate(kept);
        ends.push(targets.len() as u32);
    }
    (ends, targets)
}

/// A link target to a note, as [`crate::index::VaultIndex::resolve`] reads
/// one: a leading `./` or `/` is passed over if the name as written isn't
/// found. `key` is room to lowercase it in.
fn resolve<'m>(names: &'m Names<'_>, target: &str, key: &mut String) -> Option<&'m u32> {
    key.clear();
    push_lowercase(key, target.trim().trim_end_matches('/'));
    if key.is_empty() {
        return None;
    }
    names
        .get(key.as_str())
        .or_else(|| names.get(key.trim_start_matches(['.', '/'])))
}

/// Appends `text` lowercased, as [`str::to_lowercase`] would, the common
/// case of plain ASCII without working through it a character at a time.
fn push_lowercase(out: &mut String, text: &str) {
    if text.is_ascii() {
        let start = out.len();
        out.push_str(text);
        out[start..].make_ascii_lowercase();
    } else {
        out.push_str(&text.to_lowercase());
    }
}

/// A quick hash for the names in a vault: a multiply and a rotate for each
/// eight bytes, where the standard hasher, made to resist collisions chosen
/// by an attacker, takes several times as long. The names are the vault's
/// own.
#[derive(Default)]
struct Quick(u64);

impl Quick {
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl Hasher for Quick {
    fn write(&mut self, bytes: &[u8]) {
        let mut words = bytes.chunks_exact(8);
        for word in &mut words {
            self.add(u64::from_le_bytes(word.try_into().expect("8 bytes")));
        }
        let rest = words.remainder();
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word));
        }
    }

    fn write_u8(&mut self, byte: u8) {
        self.add(u64::from(byte));
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieval::store::Store;
    use crate::test_support::TempVault;
    use crate::vault::ScanOptions;

    /// The live notes of a vault, indexed by words alone.
    fn view_of(vault: &TempVault) -> View {
        let notes = vault
            .vault()
            .scan(&ScanOptions::default())
            .expect("scan")
            .notes;
        let mut store = Store::open(&vault.path().join(".index"));
        store
            .refresh(&notes, &|_| true, None, &mut |_| {})
            .expect("refresh");
        store.take_view().expect("view")
    }

    fn place(view: &View, rel: &str) -> u32 {
        view.notes
            .iter()
            .position(|&at| view.note(at).rel == rel)
            .expect("a live note") as u32
    }

    #[test]
    fn links_resolve_by_path_then_file_name_then_alias() {
        let vault = TempVault::new("links-resolve");
        vault.write("Beta.md", "---\naliases: [Gamma, Nick]\n---\nbeta\n");
        vault.write("Delta.md", "---\naliases: [Nick]\n---\n[[BETA]]\n");
        vault.write(
            "Projects/Alpha.md",
            "[[beta]] [[Projects/Gamma]] [[./Projects/Gamma.md]] [[Nick]] [[Missing]] [[Alpha]]\n",
        );
        vault.write("Projects/Gamma.md", "[[Gamma]] [[delta.md]]\n");
        let view = view_of(&vault);
        let [beta, delta, alpha, gamma] = [
            "Beta.md",
            "Delta.md",
            "Projects/Alpha.md",
            "Projects/Gamma.md",
        ]
        .map(|rel| place(&view, rel));

        let links = Links::resolve(&view);
        assert_eq!(
            links.outgoing(alpha as usize),
            [beta, gamma],
            "a file name, a path, a leading ./ and an alias, but not itself"
        );
        assert_eq!(links.outgoing(delta as usize), [beta], "whatever the case");
        assert_eq!(
            links.outgoing(gamma as usize),
            [delta],
            "a file name outranks another note's alias"
        );
        assert!(links.outgoing(beta as usize).is_empty());
        assert_eq!(links.incoming(beta as usize), [delta, alpha]);
        assert_eq!(links.incoming(gamma as usize), [alpha]);
        assert_eq!(links.incoming(delta as usize), [gamma]);
        assert_eq!(links.notes(), 4);
        assert_eq!(view.graph.as_ref(), Some(&links), "as the index kept them");
    }

    #[test]
    fn kept_links_are_read_back_only_for_the_index_they_were_made_for() {
        let vault = TempVault::new("links-file");
        vault.write("A.md", "[[B]]\n");
        vault.write("B.md", "b\n");
        let links = Links::resolve(&view_of(&vault));
        let path = vault.path().join("links");
        links.write(&path, 7).expect("write");
        assert_eq!(Links::read(&path, 7, 2), Some(links.clone()));
        assert_eq!(Links::read(&path, 8, 2), None, "another generation");
        assert_eq!(Links::read(&path, 7, 3), None, "another number of notes");
        assert_eq!(Links::read(&vault.path().join("absent"), 7, 2), None);

        let bytes = fs::read(&path).expect("read");
        fs::write(&path, &bytes[..bytes.len() - 4]).expect("write");
        assert_eq!(Links::read(&path, 7, 2), None, "cut short");
        let mut wrong = bytes.clone();
        let last = wrong.len() - 4;
        wrong[last..].copy_from_slice(&99u32.to_le_bytes());
        fs::write(&path, &wrong).expect("write");
        assert_eq!(Links::read(&path, 7, 2), None, "a note there isn't");
    }
}
