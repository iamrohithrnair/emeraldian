//! A vault's retrieval index on disk: its segments, kept up to date.
//!
//! The index is a short list of [segments](super::segment), oldest first, and
//! a manifest naming them. A note's newest copy wins; a deleted note is listed
//! as deleted until every segment holding it has been merged. An update writes
//! new segments holding only what changed, so keeping up with an agent that
//! edits a note a minute costs kilobytes a time, not a rewrite of the index.
//! When the list grows long, the small newer segments are merged together;
//! the big oldest one is rewritten only when the newer ones come to a quarter
//! of it, or when much of what the segments hold is deleted or superseded.
//!
//! A refresh reads what changed a chunk at a time, a couple of thousand
//! passages, embeds the chunk, writes it out as a segment and lets it go. So
//! it holds a chunk however large the vault, stopping part-way loses a chunk
//! at most, and the segments it wrote are merged once, at the end.
//!
//! Words and links are indexed whenever a note changes, with no model needed.
//! Vectors are added when a model is at hand, so a vault indexed while the
//! model was out of reach is searchable by its words and its graph at once,
//! and by meaning as soon as the model is back. Every vector in an index comes
//! from the same model at the same size: a new model's arrival clears the old
//! vectors rather than mixing the two. A note that changes keeps the vectors
//! of the passages whose text, as the model reads it, didn't, so appending a
//! line to a long note costs the model a passage, not the note.
//!
//! The links between notes are resolved whenever the index changes and kept
//! beside it as a [graph](super::graph), so a question walks them without
//! resolving them again.
//!
//! One process writes at a time, holding the operating system's lock on a
//! file that is never removed, so a writer that dies lets go as it dies.
//! Another that finds the lock held answers from the index as it stands
//! rather than waiting.
//!
//! # What reaches the disk when
//!
//! The index is a cache of the notes, so what a question writes on its way
//! isn't waited for: a small segment, the manifest, the links and the time
//! of the question reach the disk when the system gets round to them. What
//! would take long to make again is waited for: a segment of
//! [`DURABLE_PASSAGES`] passages or more, such as a first `emeraldian index`
//! writes, and the merge that rewrites the oldest segment, are on the disk
//! before anything names them, and before the segments they replace go.
//!
//! So a crash of the whole system can lose the last few seconds of small
//! writes, and damage what they were writing. A damaged segment is caught
//! when it is opened, its header, catalog and passage table being checked,
//! and left out, and the notes it held are read again. A manifest that is
//! gone, can't be read, or names a segment that isn't there is made again
//! from the segments themselves, each of which says which model its vectors
//! are from: safe, because a note's newest copy wins whichever segments are
//! listed, and a note deleted meanwhile is found gone by the next refresh.
//! A process that dies part-way leaves only files no one will read, which
//! the next writer sweeps away.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::graph::Links;
use super::segment::{
    self, NoteInfo, NoteRecord, PassageInfo, PassageRecord, Quantized, Segment, Shape, Source,
};
use super::text;
use super::{Embed, Prompts};
use crate::links;
use crate::note::{self, NoteMeta};

/// The manifest's own layout. How notes are cut into segments is the
/// segments' version to say, not this: a manifest can be made again from
/// the segments, so only they can turn away an index made the old way. 2
/// moved the time of the last question to a file of its own.
const FORMAT: u32 = 2;
const MANIFEST: &str = "manifest.json";
/// The file whose lock a writer holds. Never removed: the lock, not the
/// file, is what says a writer is at work.
const LOCK: &str = "lock";
/// The links between the live notes, resolved: see [`Links`].
const GRAPH: &str = "graph";
/// When a question was last asked: see [`Store::asked`].
const ASKED: &str = "asked";
/// The vectors of questions asked lately: see [`Store::remembered`].
const QUESTIONS: &str = "questions";
const QUESTIONS_MAGIC: &[u8; 8] = b"EMRLQST1";
/// How many questions' vectors are kept: the newest, a few hundred
/// kilobytes of them.
const QUESTIONS_KEPT: usize = 256;
/// Segments kept before the newer ones are merged, whatever their sizes.
const MAX_SEGMENTS: usize = 6;
/// How many components of a vector the first pass of a search compares.
/// EmbeddingGemma 2's first 256 keep 96 to 99 in a hundred of the true ten
/// nearest passages among the forty they rank first, where 128 kept 75 to
/// 85, for twice the bytes read.
pub const PREFIX: usize = 256;
/// Passages sent to the model at once, from as many notes as it takes.
const BATCH: usize = 32;
/// How many passages a refresh reads, embeds and writes out at a time: what
/// it holds at once, and the most that stopping part-way loses.
const CHUNK_PASSAGES: usize = 2_048;
/// How many segments of one size a long refresh writes before it merges
/// them, so that however long it runs it keeps few files open.
const GATHER: usize = 16;
/// A segment or merge of at least this many passages is waited on to reach
/// the disk: see the module's note on what reaches the disk when.
const DURABLE_PASSAGES: usize = CHUNK_PASSAGES;
/// A note whose file was written this close to the refresh that read it, in
/// nanoseconds, may have been written again since without its time or size
/// changing, on a filesystem whose clock is coarse, so the next refresh
/// reads it again: two seconds, the coarsest clock a filesystem keeps.
const RACY: u64 = 2_000_000_000;
/// How many times a reader looks again for the index when a writer has
/// moved it on between the reader's reading the manifest and the segments.
const OPEN_ATTEMPTS: usize = 4;
/// How long a lock that looks held is looked at again: see [`Store::lock`].
const LOCK_PATIENCE: Duration = Duration::from_millis(15);

/// Why a refresh didn't happen.
pub const BUSY: &str = "another emeraldian is updating the index";

/// What the manifest records.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Manifest {
    format: u32,
    /// Which model, size and prompt made the vectors. Empty until one has.
    signature: String,
    dims: usize,
    /// How many times the index has been written: moved on by every write
    /// that changes which notes count, so what was worked out from one
    /// generation, as the links are, is known to be out of date in the next.
    generation: u64,
    /// Segment files, oldest first.
    segments: Vec<String>,
    /// Notes deleted since the segments holding them were written.
    deleted: Vec<String>,
}

/// The model, for a refresh that should add vectors.
pub struct Embedding<'a> {
    pub model: &'a mut dyn Embed,
    pub prompts: &'a Prompts,
    /// Model, size and prompt, as one string: vectors from different ones
    /// can't be compared.
    pub signature: &'a str,
    /// How many components to keep: 0 for all the model gives.
    pub dims: usize,
    /// The most notes to embed in this refresh; the rest wait for the next.
    pub limit: usize,
}

/// What a refresh did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Refreshed {
    /// Notes whose words and links were read again: new, or changed.
    pub indexed: usize,
    /// Notes given vectors by the model.
    pub embedded: usize,
    /// Notes in scope still without vectors: no model, or past its limit.
    pub pending: usize,
    pub removed: usize,
    /// Why the model gave out, if it did. What was done before is kept.
    pub model_error: Option<String>,
}

/// How far a long refresh has got, in passages.
///
/// What is left to embed is known exactly only as the notes are read, a
/// chunk at a time, so `total` starts as an estimate, from what the index
/// last held of each note or from its size, and is put right as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

/// A segment, open, with its catalog and which of its notes count.
pub struct Part {
    pub segment: Segment,
    pub notes: Vec<NoteInfo>,
    pub passages: Vec<PassageInfo>,
    /// Per note: whether this copy is the current one.
    pub live: Vec<bool>,
    /// The global number of this segment's first passage.
    pub base: u32,
}

impl Part {
    fn open(path: &Path) -> io::Result<Self> {
        let segment = Segment::open(path)?;
        let (notes, passages) = segment.catalog()?;
        Ok(Self {
            live: vec![true; notes.len()],
            segment,
            notes,
            passages,
            base: 0,
        })
    }
}

/// The index as it stands, ready to answer questions.
pub struct View {
    pub parts: Vec<Part>,
    /// Every live note, sorted by path, as (part, note).
    pub notes: Vec<(usize, usize)>,
    pub dims: usize,
    /// The links between the live notes, when the index has them resolved
    /// for what it now holds. A question resolves them itself when not.
    pub graph: Option<Links>,
}

impl View {
    /// `parts`, oldest first, read as one index: the newest copy of a note
    /// counts, and a deleted one none.
    ///
    /// Each part's catalog is in path order, so they are read side by side,
    /// as sorted lists are merged: the notes come out in order, and of a
    /// path more than one part holds, the latest part's copy is the one
    /// that counts.
    fn assemble(mut parts: Vec<Part>, deleted: &[String], dims: usize) -> Self {
        let mut base = 0u32;
        for part in &mut parts {
            part.base = base;
            base += part.passages.len() as u32;
        }
        let deleted: HashSet<&str> = deleted.iter().map(String::as_str).collect();
        let mut live: Vec<Vec<bool>> = parts
            .iter()
            .map(|part| vec![false; part.notes.len()])
            .collect();
        let mut notes = Vec::with_capacity(parts.iter().map(|part| part.notes.len()).sum());
        let mut next = vec![0usize; parts.len()];
        loop {
            // The least path; of equal ones, the later part's.
            let mut newest: Option<usize> = None;
            for (p, part) in parts.iter().enumerate() {
                let Some(note) = part.notes.get(next[p]) else {
                    continue;
                };
                if newest.is_none_or(|b| note.rel <= parts[b].notes[next[b]].rel) {
                    newest = Some(p);
                }
            }
            let Some(p) = newest else {
                break;
            };
            let n = next[p];
            let rel = parts[p].notes[n].rel.as_str();
            for (q, part) in parts.iter().enumerate() {
                if q != p
                    && part
                        .notes
                        .get(next[q])
                        .is_some_and(|older| older.rel == rel)
                {
                    next[q] += 1;
                }
            }
            next[p] += 1;
            if !deleted.contains(rel) {
                live[p][n] = true;
                notes.push((p, n));
            }
        }
        for (part, live) in parts.iter_mut().zip(live) {
            part.live = live;
        }
        Self {
            parts,
            notes,
            dims,
            graph: None,
        }
    }

    #[must_use]
    pub fn note(&self, at: (usize, usize)) -> &NoteInfo {
        &self.parts[at.0].notes[at.1]
    }

    /// The live note at a path.
    #[must_use]
    pub fn find(&self, rel: &str) -> Option<(usize, usize)> {
        self.notes
            .binary_search_by(|&at| self.note(at).rel.as_str().cmp(rel))
            .ok()
            .map(|i| self.notes[i])
    }

    /// The size of the global passage numbering, live passages or not.
    #[must_use]
    pub fn passage_total(&self) -> u32 {
        self.parts
            .last()
            .map_or(0, |part| part.base + part.passages.len() as u32)
    }

    /// Which part a global passage number is in, and its number there.
    #[must_use]
    pub fn locate(&self, global: u32) -> (usize, u32) {
        let part = self
            .parts
            .partition_point(|part| part.base <= global)
            .saturating_sub(1);
        (part, global - self.parts[part].base)
    }

    #[must_use]
    pub fn passage_count(&self) -> usize {
        self.notes
            .iter()
            .map(|&at| self.note(at).count as usize)
            .sum()
    }

    /// Live notes `in_scope` accepts that have passages but no vectors yet.
    #[must_use]
    pub fn pending(&self, in_scope: &dyn Fn(&str) -> bool) -> usize {
        self.notes
            .iter()
            .map(|&at| self.note(at))
            .filter(|info| !info.embedded && info.count > 0 && in_scope(&info.rel))
            .count()
    }

    /// The notes among `notes` whose files differ from what was indexed, or
    /// that were never indexed. From names, sizes and times: nothing is read.
    #[must_use]
    pub fn stale<'a>(&self, notes: &'a [NoteMeta]) -> Vec<&'a NoteMeta> {
        notes
            .iter()
            .filter(|meta| match self.find(&meta.rel) {
                Some(at) => {
                    let known = self.note(at);
                    known.stamp != meta.stamp || known.size != meta.size
                }
                None => true,
            })
            .collect()
    }

    /// Live notes `in_scope` accepts that no longer exist.
    #[must_use]
    pub fn gone(&self, notes: &[NoteMeta], in_scope: &dyn Fn(&str) -> bool) -> usize {
        let present: HashSet<&str> = notes.iter().map(|meta| meta.rel.as_str()).collect();
        self.notes
            .iter()
            .map(|&at| &self.note(at).rel)
            .filter(|rel| in_scope(rel) && !present.contains(rel.as_str()))
            .count()
    }
}

/// A vault's index, by the directory it lives in.
pub struct Store {
    dir: PathBuf,
    manifest: Manifest,
    /// The index as the last refresh left it, for [`Store::take_view`].
    fresh: Option<View>,
    /// The index as [`Store::prepare`] opened it, and the manifest it was
    /// opened from.
    prepared: Option<(Manifest, View)>,
    /// Whether the last refresh left the index as it was prepared.
    unchanged: bool,
    /// Passages read per chunk of a refresh: [`CHUNK_PASSAGES`], fewer in
    /// tests.
    chunk: usize,
    /// How close to its reading a note's file must have been written to be
    /// read again: [`RACY`], or none in tests that count what is written.
    racy: u64,
}

/// Holds the index's lock; dropping it lets go, as the process ending does.
struct Lock {
    _file: File,
}

/// What a refresh has to do about a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Need {
    /// Read it: the file is new, or differs from what was indexed.
    Read,
    /// Read it again to be sure of it: its file was written so close to the
    /// refresh that read it that a later write might not show.
    Check,
    /// Give it vectors: it is unchanged, but was indexed without them.
    Vectors,
}

/// A note a refresh has to read.
struct Task<'a> {
    meta: &'a NoteMeta,
    need: Need,
    /// About how many passages it has, for the progress shown while
    /// embedding: what the index last held, or a guess from its size.
    guess: usize,
}

/// Where a note read again may find the vectors it needs.
enum Reuse {
    /// The live note, at the same path, whose text it still is: its
    /// passages are this one's, in order, vectors and all.
    Same((usize, usize)),
    /// Live notes some of whose passages may be this one's: its own last
    /// copy, and a note with the same text elsewhere, renamed or copied. A
    /// passage takes the vector of one the model read the same text from.
    Hashes(Vec<(usize, usize)>),
}

/// A note read for the chunk being worked on.
struct Item {
    record: NoteRecord,
    reuse: Reuse,
    /// Its passages' titles, the note's with the headings over each, and
    /// their text, for the model; empty when no model is at hand.
    texts: Vec<(String, String)>,
    /// How many of its passages went to the model.
    sent: usize,
    /// Whether it is to be written: it changed, was read again to be sure,
    /// or gained vectors.
    changed: bool,
}

/// A refresh under way.
struct Run<'a, 'p> {
    embedding: Option<Embedding<'a>>,
    progress: &'p mut dyn FnMut(Progress),
    report: Refreshed,
    /// Notes to list as deleted: gone from the vault, and not written since.
    deleted: BTreeSet<String>,
    /// The segments this refresh has written, oldest first, with how many
    /// merges each has been through.
    written: Vec<(String, u32)>,
    /// How many more notes it may embed.
    budget: usize,
    /// Passages embedded, and about how many there are to embed.
    done: usize,
    total: usize,
    /// When the refresh began, in nanoseconds since the epoch: when each
    /// note it reads is recorded as read.
    started: u64,
    /// The store's [`Store::racy`].
    racy: u64,
}

impl Run<'_, '_> {
    /// Whether a note may still be given new vectors.
    fn may_embed(&self) -> bool {
        self.embedding.is_some() && self.budget > 0 && self.report.model_error.is_none()
    }
}

impl Store {
    /// The index in `dir`, or an empty one. A manifest that won't parse, or
    /// is from another format, is made again from the segments there.
    #[must_use]
    pub fn open(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            manifest: load(dir),
            fresh: None,
            prepared: None,
            unchanged: false,
            chunk: CHUNK_PASSAGES,
            racy: RACY,
        }
    }

    /// Whether anything has been indexed.
    #[must_use]
    pub fn exists(&self) -> bool {
        !self.manifest.segments.is_empty()
    }

    /// Which model made the vectors, or empty if none has.
    #[must_use]
    pub fn signature(&self) -> &str {
        &self.manifest.signature
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// When a question was last asked of this index, in seconds since the
    /// epoch.
    #[must_use]
    pub fn last_question(&self) -> u64 {
        fs::read_to_string(self.dir.join(ASKED))
            .ok()
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(0)
    }

    /// Records that a question was asked now. A few bytes, written without
    /// the lock and not waited for: all they inform is a guess at how long
    /// to keep the model loaded, which a lost write costs little.
    pub fn asked(&self) {
        let _ = fs::create_dir_all(&self.dir);
        let _ = fs::write(self.dir.join(ASKED), now().to_string());
    }

    /// The vector a question was given when it was last asked, with the
    /// model `signature` names: `question` as the model was given it, its
    /// prompt and all. Asked again, as agents do, it needs no model.
    #[must_use]
    pub fn remembered(&self, signature: &str, question: &str) -> Option<Vec<f32>> {
        let key = question_key(signature, question);
        let file = File::open(self.dir.join(QUESTIONS)).ok()?;
        let length = file.metadata().ok()?.len();
        let (count, width) = questions_header(&file, length)?;
        let mut keys = vec![0u8; count * 8];
        read_exact_at(&file, 16, &mut keys).ok()?;
        let place = keys
            .chunks_exact(8)
            .rposition(|word| u64::from_le_bytes(word.try_into().expect("8")) == key)?;
        let mut vector = vec![0u8; width * 4];
        let at = 16 + (count * 8 + place * width * 4) as u64;
        read_exact_at(&file, at, &mut vector).ok()?;
        Some(
            vector
                .chunks_exact(4)
                .map(|word| f32::from_le_bytes(word.try_into().expect("4")))
                .collect(),
        )
    }

    /// The index as it stands, with its links when they are resolved for it.
    pub fn view(&self) -> io::Result<View> {
        self.current().map(|(_, view)| view)
    }

    /// The index as it stands, and the manifest it was opened from: the one
    /// this store read, or a newer one if a writer has since moved on and
    /// removed what the older named.
    fn current(&self) -> io::Result<(Manifest, View)> {
        let mut manifest = self.manifest.clone();
        let mut attempts = 1;
        loop {
            match open_view(&self.dir, &manifest) {
                Ok(mut view) => {
                    view.graph =
                        Links::read(&self.dir.join(GRAPH), manifest.generation, view.notes.len());
                    return Ok((manifest, view));
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound && attempts < OPEN_ATTEMPTS => {
                    manifest = load(&self.dir);
                    attempts += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// Opens the index as it stands, ahead of a refresh, so that a question
    /// can be asked of it while the vault is still being scanned. The
    /// refresh starts from it if no other process has written meanwhile;
    /// [`Store::unchanged`] then says whether the refresh changed it.
    pub fn prepare(&mut self) -> io::Result<()> {
        self.prepared = Some(self.current()?);
        Ok(())
    }

    /// The index [`Store::prepare`] opened, until a refresh takes it over.
    #[must_use]
    pub fn prepared(&self) -> Option<&View> {
        self.prepared.as_ref().map(|(_, view)| view)
    }

    /// Whether the last refresh left the index as [`Store::prepare`] found
    /// it, so that what was asked of [`Store::prepared`] meanwhile is what
    /// would be asked of [`Store::take_view`]: true when the refresh
    /// changed nothing, or was turned away.
    #[must_use]
    pub fn unchanged(&self) -> bool {
        self.unchanged
    }

    /// The index as the last refresh left it, without reading it again; or
    /// as [`Store::prepare`] opened it, if a refresh was turned away; or as
    /// it stands.
    pub fn take_view(&mut self) -> io::Result<View> {
        if let Some(view) = self.fresh.take() {
            return Ok(view);
        }
        if let Some((_, view)) = self.prepared.take() {
            return Ok(view);
        }
        self.view()
    }

    /// Takes the lock, or `None` while another process holds it.
    ///
    /// A lock just let go can look held for a moment: a process another
    /// thread was starting holds a copy of the file's handle until it has
    /// started. So a held lock is looked at again a few times, for at most
    /// [`LOCK_PATIENCE`] in all, which a writer at work outlasts.
    fn lock(&self) -> Option<Lock> {
        fs::create_dir_all(&self.dir).ok()?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.dir.join(LOCK))
            .ok()?;
        let mut wait = Duration::from_millis(1);
        let mut waited = Duration::ZERO;
        loop {
            match file.try_lock() {
                Ok(()) => return Some(Lock { _file: file }),
                Err(TryLockError::WouldBlock) if waited < LOCK_PATIENCE => {
                    std::thread::sleep(wait);
                    waited += wait;
                    wait *= 2;
                }
                Err(TryLockError::WouldBlock) => return None,
                // A filesystem that can't lock at all is written to
                // unlocked, as it was before there were locks, rather than
                // never.
                Err(TryLockError::Error(_)) => return Some(Lock { _file: file }),
            }
        }
    }

    /// Throws the index away, to be built again from the notes.
    pub fn clear(&mut self) -> Result<(), String> {
        let Some(_lock) = self.lock() else {
            return Err(BUSY.into());
        };
        // A generation of its own: nothing worked out from the old one holds.
        let generation = load_for_writing(&self.dir).0.generation + 1;
        self.manifest = Manifest {
            format: FORMAT,
            generation,
            ..Manifest::default()
        };
        self.fresh = None;
        self.prepared = None;
        self.save_manifest().map_err(|err| err.to_string())?;
        // Every segment, as none is named now.
        self.sweep();
        Ok(())
    }

    /// Brings the notes `in_scope` accepts up to date with `notes`, the vault
    /// as a scan lists it: reads what changed, drops what is gone, and with a
    /// model, embeds what has no vectors, up to the model's limit. The view
    /// it ends with is kept for [`Store::take_view`].
    ///
    /// `Err` means nothing was done: another process holds the lock, or the
    /// index couldn't be written. A model that fails part-way leaves what was
    /// embedded before it did, and says why in [`Refreshed::model_error`].
    pub fn refresh(
        &mut self,
        notes: &[NoteMeta],
        in_scope: &dyn Fn(&str) -> bool,
        embedding: Option<Embedding<'_>>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Refreshed, String> {
        self.unchanged = false;
        self.fresh = None;
        let Some(_lock) = self.lock() else {
            // The index as prepared is the one to answer from.
            self.unchanged = self.prepared.is_some();
            return Err(BUSY.into());
        };
        let started = now_nanos();
        // Another process may have written since this one opened the index.
        let (manifest, recovered) = load_for_writing(&self.dir);
        self.manifest = manifest;
        self.manifest.format = FORMAT;
        self.sweep();
        let (mut view, prepared) = self
            .open_for_refresh(recovered)
            .map_err(|err| err.to_string())?;
        // A new model's vectors can't be compared with the old ones'.
        if let Some(embedding) = &embedding
            && !self.manifest.signature.is_empty()
            && self.manifest.signature != embedding.signature
        {
            view = self.strip_vectors(view).map_err(|err| err.to_string())?;
        }
        let start = self.manifest.generation;
        let before = view.graph.take();

        let mut present: Vec<&NoteMeta> = notes.iter().filter(|m| in_scope(&m.rel)).collect();
        present.sort_by(|a, b| a.rel.cmp(&b.rel));
        let limit = embedding.as_ref().map_or(0, |embedding| embedding.limit);
        let mut run = Run {
            embedding,
            progress,
            report: Refreshed::default(),
            deleted: self.manifest.deleted.iter().cloned().collect(),
            written: Vec::new(),
            budget: limit,
            done: 0,
            total: 0,
            started,
            racy: self.racy,
        };
        let tasks = compare(&view, &present, in_scope, &mut run);
        let embedding_on = run.embedding.is_some();
        if embedding_on {
            run.total = tasks.iter().take(limit).map(|task| task.guess).sum();
        }

        // What needs reading, a chunk at a time.
        let mut by_hash: Option<HashMap<u64, (usize, usize)>> = None;
        let mut chunk: Vec<Item> = Vec::new();
        let mut held = 0;
        for (index, task) in tasks.iter().enumerate() {
            if embedding_on && index < limit {
                run.total = run.total.saturating_sub(task.guess);
            }
            let read = read_note(task, &view, &mut by_hash, &mut run);
            let Some(item) = read.map_err(|err| err.to_string())? else {
                continue;
            };
            held += item.record.passages.len().max(1);
            chunk.push(item);
            if held >= self.chunk {
                self.flush(&view, &mut run, &mut chunk)?;
                held = 0;
            }
        }
        self.flush(&view, &mut run, &mut chunk)?;

        // Deletions, when no segment was written to carry them.
        let deleted: Vec<String> = run.deleted.iter().cloned().collect();
        if deleted != self.manifest.deleted {
            self.manifest.deleted = deleted;
            self.manifest.generation += 1;
            self.save_manifest().map_err(|err| err.to_string())?;
        }
        if self.manifest.generation != start {
            let View { mut parts, .. } = view;
            for (name, _) in &run.written {
                parts.push(Part::open(&self.dir.join(name)).map_err(|err| err.to_string())?);
            }
            view = View::assemble(parts, &self.manifest.deleted, self.manifest.dims);
            if let Some(from) = merge_from(&view) {
                view = self
                    .merge(view, from, false)
                    .map_err(|err| err.to_string())?;
            }
        }
        // Which notes there are and what they say about each other are as
        // they were unless a note was read anew or went.
        let same_links = run.report.indexed == 0 && run.report.removed == 0;
        view.graph = Some(match before {
            Some(links) if self.manifest.generation == start => links,
            before => self.links(&view, before.filter(|_| same_links)),
        });
        run.report.pending = view.pending(in_scope);
        self.unchanged = prepared && self.manifest.generation == start;
        self.fresh = Some(view);
        Ok(run.report)
    }

    /// The index to refresh: the one prepared, if the manifest is as it was
    /// prepared from, and else opened again; and whether it was the one
    /// prepared. A segment that can't be opened is left out and removed,
    /// and the notes it held are read again as notes the index doesn't
    /// have; a manifest made again from the segments is written down.
    fn open_for_refresh(&mut self, recovered: bool) -> io::Result<(View, bool)> {
        if let Some((manifest, view)) = self.prepared.take()
            && manifest == self.manifest
            && !recovered
        {
            return Ok((view, true));
        }
        let mut parts = Vec::with_capacity(self.manifest.segments.len());
        let mut kept = Vec::with_capacity(self.manifest.segments.len());
        for name in &self.manifest.segments {
            match Part::open(&self.dir.join(name)) {
                Ok(part) => {
                    parts.push(part);
                    kept.push(name.clone());
                }
                Err(_) => {
                    let _ = fs::remove_file(self.dir.join(name));
                }
            }
        }
        if recovered || kept.len() != self.manifest.segments.len() {
            self.manifest.segments = kept;
            self.manifest.generation += 1;
            self.save_manifest()?;
        }
        let mut view = View::assemble(parts, &self.manifest.deleted, self.manifest.dims);
        view.graph = Links::read(
            &self.dir.join(GRAPH),
            self.manifest.generation,
            view.notes.len(),
        );
        Ok((view, false))
    }

    /// Removes what no one will read again: the segments the manifest
    /// doesn't name, merged away or left by a writer that died, and the
    /// staging and scratch files of a write that didn't finish. A reader
    /// still holding one open reads on; one about to open one finds the
    /// manifest moved on, and reads that instead.
    fn sweep(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let keep: HashSet<&str> = [MANIFEST, LOCK, GRAPH, ASKED, QUESTIONS]
            .into_iter()
            .chain(self.manifest.segments.iter().map(String::as_str))
            .collect();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let unwanted = name.to_str().is_some_and(|name| !keep.contains(name));
            if unwanted && entry.file_type().is_ok_and(|kind| kind.is_file()) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// Gives the chunk the vectors it already has in the index, embeds what
    /// is still owed them, and writes the chunk out.
    fn flush(
        &mut self,
        view: &View,
        run: &mut Run<'_, '_>,
        chunk: &mut Vec<Item>,
    ) -> Result<(), String> {
        if chunk.is_empty() {
            return Ok(());
        }
        self.reuse(view, run, chunk)
            .map_err(|err| err.to_string())?;
        self.embed(run, chunk);
        let mut records: Vec<NoteRecord> = chunk
            .drain(..)
            .filter(|item| item.changed)
            .map(|item| item.record)
            .collect();
        self.write(run, &mut records).map_err(|err| err.to_string())
    }

    /// Gives the chunk's passages the vectors the index already holds for
    /// them: a note whose text is unchanged keeps its own, passage by
    /// passage, and any other passage takes the vector of one the model
    /// read the very same text from, title and prompt included, among the
    /// notes it may come from. Each part's vectors are read in one go.
    fn reuse(&self, view: &View, run: &Run<'_, '_>, chunk: &mut [Item]) -> io::Result<()> {
        let dims = self.manifest.dims;
        if dims == 0 {
            return Ok(());
        }
        let prompts = run.embedding.as_ref().map(|embedding| embedding.prompts);
        // For each part: which of its passages to read, for which item and
        // passage of the chunk, and the hash of the text it was made from.
        let mut wanted: BTreeMap<usize, Vec<(u32, usize, usize, u64)>> = BTreeMap::new();
        for (i, item) in chunk.iter().enumerate() {
            match &item.reuse {
                Reuse::Same((p, n)) => {
                    let part = &view.parts[*p];
                    if part.segment.dims() != dims {
                        continue;
                    }
                    let info = &part.notes[*n];
                    for (k, hash) in part
                        .segment
                        .hashes(info.first, info.count)?
                        .into_iter()
                        .enumerate()
                    {
                        if hash != 0 {
                            wanted
                                .entry(*p)
                                .or_default()
                                .push((info.first + k as u32, i, k, hash));
                        }
                    }
                }
                Reuse::Hashes(donors) => {
                    let Some(prompts) = prompts else {
                        continue;
                    };
                    let mut held: HashMap<u64, (usize, u32)> = HashMap::new();
                    for &(p, n) in donors {
                        let part = &view.parts[p];
                        if part.segment.dims() != dims {
                            continue;
                        }
                        let info = &part.notes[n];
                        let hashes = part.segment.hashes(info.first, info.count)?;
                        for (k, hash) in hashes.into_iter().enumerate() {
                            if hash != 0 {
                                held.entry(hash).or_insert((p, info.first + k as u32));
                            }
                        }
                    }
                    if held.is_empty() {
                        continue;
                    }
                    for (k, (title, text)) in item.texts.iter().enumerate() {
                        let hash = fnv(model_text(prompts, title, text).as_bytes());
                        if let Some(&(p, local)) = held.get(&hash) {
                            wanted.entry(p).or_default().push((local, i, k, hash));
                        }
                    }
                }
            }
        }
        for (p, mut list) in wanted {
            list.sort_unstable_by_key(|&(local, ..)| local);
            let ids: Vec<u32> = list.iter().map(|&(local, ..)| local).collect();
            let vectors = view.parts[p].segment.vectors(&ids)?;
            for (&(_, i, k, hash), vector) in list.iter().zip(vectors) {
                let Some(vector) = vector else {
                    continue;
                };
                let item = &mut chunk[i];
                if matches!(item.reuse, Reuse::Hashes(_)) {
                    item.changed = true;
                }
                let passage = &mut item.record.passages[k];
                passage.vector = Some(vector);
                passage.hash = hash;
            }
        }
        Ok(())
    }

    /// Embeds the chunk's passages that still have no vector, [`BATCH`] at
    /// a time whichever notes they are from, of as many notes, in path
    /// order, as the model's limit allows. A model that fails stops the
    /// embedding for the rest of the refresh; each vector that came before
    /// is kept.
    fn embed(&mut self, run: &mut Run<'_, '_>, chunk: &mut [Item]) {
        let Some(Embedding {
            model,
            prompts,
            signature,
            dims,
            ..
        }) = run.embedding.as_mut()
        else {
            return;
        };
        let mut wanted: Vec<(usize, usize)> = Vec::new();
        for (i, item) in chunk.iter_mut().enumerate() {
            let missing: Vec<usize> = (0..item.record.passages.len())
                .filter(|&k| item.record.passages[k].vector.is_none())
                .collect();
            if missing.is_empty() || run.budget == 0 || run.report.model_error.is_some() {
                continue;
            }
            run.budget -= 1;
            item.sent = missing.len();
            run.total += missing.len();
            wanted.extend(missing.into_iter().map(|k| (i, k)));
        }
        // The size kept: the index's, or for a new model, what it gives.
        let mut size = (self.manifest.signature == *signature && self.manifest.dims > 0)
            .then_some(self.manifest.dims);
        for batch in wanted.chunks(BATCH) {
            if run.report.model_error.is_some() {
                break;
            }
            let inputs: Vec<String> = batch
                .iter()
                .map(|&(i, k)| {
                    let (title, passage) = &chunk[i].texts[k];
                    model_text(prompts, title, passage)
                })
                .collect();
            match model.embed(&inputs) {
                Ok(got) if got.len() == batch.len() => {
                    let d = *size.get_or_insert_with(|| {
                        let given = got.first().map_or(0, Vec::len);
                        if *dims == 0 {
                            given
                        } else {
                            (*dims).min(given)
                        }
                    });
                    // The model and size the index's vectors are from, now
                    // that some are kept.
                    if self.manifest.signature != *signature {
                        self.manifest.signature = signature.to_string();
                    }
                    self.manifest.dims = d;
                    for ((&(i, k), vector), input) in batch.iter().zip(&got).zip(&inputs) {
                        let item = &mut chunk[i];
                        item.changed = true;
                        let passage = &mut item.record.passages[k];
                        passage.vector = Some(Quantized::from_unit(&fit(vector, d)));
                        passage.hash = fnv(input.as_bytes());
                    }
                    run.done += batch.len();
                    (run.progress)(Progress {
                        done: run.done,
                        total: run.total.max(run.done),
                    });
                }
                Ok(got) => {
                    run.report.model_error = Some(format!(
                        "the model returned {} vectors for {} passages",
                        got.len(),
                        batch.len()
                    ));
                }
                Err(err) => run.report.model_error = Some(err),
            }
        }
        run.report.embedded += chunk
            .iter()
            .filter(|item| item.sent > 0 && item.record.embedded())
            .count();
    }

    /// What the vectors of a segment written now are.
    fn shape(&self) -> Shape<'_> {
        let dims = self.manifest.dims;
        Shape {
            model: &self.manifest.signature,
            dims,
            prefix: PREFIX.min(dims),
        }
    }

    /// Writes `records` as the newest segment, with the deleted notes as they
    /// now stand.
    fn write(&mut self, run: &mut Run<'_, '_>, records: &mut Vec<NoteRecord>) -> io::Result<()> {
        records.sort_by(|a, b| a.rel.cmp(&b.rel));
        records.dedup_by(|b, a| a.rel == b.rel);
        if records.is_empty() {
            return Ok(());
        }
        let passages: usize = records.iter().map(|record| record.passages.len()).sum();
        let name = self.next_name();
        segment::write(
            &self.dir.join(&name),
            records,
            &self.shape(),
            passages >= DURABLE_PASSAGES,
        )?;
        self.manifest.generation += 1;
        for record in records.iter() {
            run.deleted.remove(&record.rel);
        }
        self.manifest.deleted = run.deleted.iter().cloned().collect();
        self.manifest.segments.push(name.clone());
        self.save_manifest()?;
        run.written.push((name, 0));
        self.gather(run)
    }

    /// Merges the last [`GATHER`] segments this refresh wrote when they have
    /// been through as many merges, and again while that makes another such
    /// run: a long refresh keeps a few dozen segments however long it runs,
    /// and each passage is rewritten a few times at most.
    fn gather(&mut self, run: &mut Run<'_, '_>) -> io::Result<()> {
        loop {
            let n = run.written.len();
            if n < GATHER {
                return Ok(());
            }
            let depth = run.written[n - 1].1;
            if run.written[n - GATHER..].iter().any(|&(_, d)| d != depth) {
                return Ok(());
            }
            let names: Vec<String> = run
                .written
                .drain(n - GATHER..)
                .map(|(name, _)| name)
                .collect();
            let first = self.manifest.segments.len() - GATHER;
            debug_assert_eq!(self.manifest.segments[first..], names[..]);
            let name = self.next_name();
            {
                let segments = names
                    .iter()
                    .map(|name| Segment::open(&self.dir.join(name)))
                    .collect::<io::Result<Vec<_>>>()?;
                // What a refresh writes is the newest copy of every note in it.
                let keep: Vec<Vec<bool>> = segments
                    .iter()
                    .map(|segment| vec![true; segment.note_count()])
                    .collect();
                let sources: Vec<Source<'_>> = segments
                    .iter()
                    .zip(&keep)
                    .map(|(segment, keep)| Source { segment, keep })
                    .collect();
                let passages: usize = segments.iter().map(Segment::passage_count).sum();
                segment::merge(
                    &self.dir.join(&name),
                    &sources,
                    &self.shape(),
                    passages >= DURABLE_PASSAGES,
                )?;
            }
            self.manifest.generation += 1;
            self.manifest.segments.truncate(first);
            self.manifest.segments.push(name.clone());
            self.save_manifest()?;
            for old in names {
                let _ = fs::remove_file(self.dir.join(old));
            }
            run.written.push((name, depth + 1));
        }
    }

    /// Merges the segments from `from` on into one holding only their live
    /// notes, without vectors if `strip`, and gives back the view of the
    /// index as it then is. A deleted note stays listed while a segment left
    /// out of the merge still holds it.
    fn merge(&mut self, view: View, from: usize, strip: bool) -> io::Result<View> {
        let View { mut parts, .. } = view;
        if strip {
            self.manifest.signature.clear();
            self.manifest.dims = 0;
        }
        let name = self.next_name();
        {
            let sources: Vec<Source<'_>> = parts[from..]
                .iter()
                .map(|part| Source {
                    segment: &part.segment,
                    keep: &part.live,
                })
                .collect();
            // Rewriting the oldest segment, or much of any, is worth waiting
            // for: a crash then costs nothing to make again.
            let passages: usize = parts[from..]
                .iter()
                .map(|part| part.segment.passage_count())
                .sum();
            segment::merge(
                &self.dir.join(&name),
                &sources,
                &self.shape(),
                from == 0 || passages >= DURABLE_PASSAGES,
            )?;
        }
        self.manifest.generation += 1;
        if !self.manifest.deleted.is_empty() {
            let older: HashSet<&str> = parts[..from]
                .iter()
                .flat_map(|part| part.notes.iter().map(|note| note.rel.as_str()))
                .collect();
            self.manifest
                .deleted
                .retain(|rel| older.contains(rel.as_str()));
        }
        let old = self.manifest.segments.split_off(from);
        self.manifest.segments.push(name.clone());
        self.save_manifest()?;
        // Closed before they are removed, as some systems insist.
        parts.truncate(from);
        for name in old {
            let _ = fs::remove_file(self.dir.join(name));
        }
        parts.push(Part::open(&self.dir.join(&name))?);
        Ok(View::assemble(
            parts,
            &self.manifest.deleted,
            self.manifest.dims,
        ))
    }

    /// Clears every vector, as a new model's arrival does.
    fn strip_vectors(&mut self, view: View) -> io::Result<View> {
        if self.manifest.segments.is_empty() {
            self.manifest.signature.clear();
            self.manifest.dims = 0;
            self.save_manifest()?;
            return Ok(View::assemble(view.parts, &self.manifest.deleted, 0));
        }
        self.merge(view, 0, true)
    }

    /// The links between the live notes of `view`, the index as it now is:
    /// as they were resolved when it was written, if they were; or `before`,
    /// the links as they were, when no note that names or links another has
    /// changed; or resolved now. Kept for the questions after.
    fn links(&self, view: &View, before: Option<Links>) -> Links {
        let path = self.dir.join(GRAPH);
        if let Some(links) = Links::read(&path, self.manifest.generation, view.notes.len()) {
            return links;
        }
        let links = before
            .filter(|links| links.notes() == view.notes.len())
            .unwrap_or_else(|| Links::resolve(view));
        // Without the file, each question resolves the links for itself.
        let _ = links.write(&path, self.manifest.generation);
        links
    }

    /// The name the next segment written is given.
    fn next_name(&self) -> String {
        format!("{:08}.seg", self.manifest.generation + 1)
    }

    /// Writes the manifest, in full or not at all, without waiting for it to
    /// reach the disk: one lost is made again from the segments.
    fn save_manifest(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let staging = self.dir.join(format!("{MANIFEST}.{}", std::process::id()));
        let text = serde_json::to_string(&self.manifest)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        fs::write(&staging, text)?;
        fs::rename(&staging, self.dir.join(MANIFEST))
    }
}

/// Keeps `vector` as the one `question` was given by the model `signature`
/// names, in the index at `dir`, for [`Store::remembered`]: the oldest kept
/// gives way when there are [`QUESTIONS_KEPT`]. A file of its own, written
/// whole and not waited for, without the lock: two processes asking at once
/// may keep only one of their questions, which costs the other a model call
/// next time.
pub fn remember(dir: &Path, signature: &str, question: &str, vector: &[f32]) {
    let path = dir.join(QUESTIONS);
    let width = vector.len();
    let (mut keys, mut vectors) = (Vec::new(), Vec::new());
    if let Ok(bytes) = fs::read(&path)
        && let Some(count) = bytes
            .get(8..12)
            .map(|b| u32::from_le_bytes(b.try_into().expect("4")))
        && bytes.get(..8) == Some(QUESTIONS_MAGIC.as_slice())
        && bytes.get(12..16) == Some((width as u32).to_le_bytes().as_slice())
        && bytes.len() == 16 + count as usize * (8 + width * 4)
    {
        let count = count as usize;
        keys = bytes[16..16 + count * 8].to_vec();
        vectors = bytes[16 + count * 8..].to_vec();
    }
    let kept = keys.len() / 8;
    if kept >= QUESTIONS_KEPT {
        let gone = kept + 1 - QUESTIONS_KEPT;
        keys.drain(..gone * 8);
        vectors.drain(..gone * width * 4);
    }
    keys.extend_from_slice(&question_key(signature, question).to_le_bytes());
    for component in vector {
        vectors.extend_from_slice(&component.to_le_bytes());
    }
    let mut bytes = Vec::with_capacity(16 + keys.len() + vectors.len());
    bytes.extend_from_slice(QUESTIONS_MAGIC);
    bytes.extend_from_slice(&((keys.len() / 8) as u32).to_le_bytes());
    bytes.extend_from_slice(&(width as u32).to_le_bytes());
    bytes.extend_from_slice(&keys);
    bytes.extend_from_slice(&vectors);
    let staging = dir.join(format!("{QUESTIONS}.{}", std::process::id()));
    let written = fs::create_dir_all(dir)
        .and_then(|()| fs::write(&staging, &bytes))
        .and_then(|()| fs::rename(&staging, &path));
    if written.is_err() {
        let _ = fs::remove_file(&staging);
    }
}

/// What a question is kept under: the model's signature and the question,
/// hashed together.
fn question_key(signature: &str, question: &str) -> u64 {
    fnv(format!("{signature}\n{question}").as_bytes())
}

/// How many questions the file of them holds, and how many components each
/// vector has, if the file is whole.
fn questions_header(file: &File, length: u64) -> Option<(usize, usize)> {
    let mut header = [0u8; 16];
    read_exact_at(file, 0, &mut header).ok()?;
    let count = u32::from_le_bytes(header[8..12].try_into().expect("4")) as usize;
    let width = u32::from_le_bytes(header[12..16].try_into().expect("4")) as usize;
    let whole = &header[..8] == QUESTIONS_MAGIC
        && count <= QUESTIONS_KEPT
        && length == 16 + (count * (8 + width * 4)) as u64;
    whole.then_some((count, width))
}

/// Reads `out.len()` bytes from `at` in `file`.
fn read_exact_at(file: &File, at: u64, out: &mut [u8]) -> io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = file;
    file.seek(SeekFrom::Start(at))?;
    file.read_exact(out)
}

/// The index `manifest` names, opened.
fn open_view(dir: &Path, manifest: &Manifest) -> io::Result<View> {
    let parts = manifest
        .segments
        .iter()
        .map(|name| Part::open(&dir.join(name)))
        .collect::<io::Result<Vec<_>>>()?;
    Ok(View::assemble(parts, &manifest.deleted, manifest.dims))
}

/// The manifest in `dir`, if there is one that can be read.
fn read_manifest(dir: &Path) -> Option<Manifest> {
    fs::read_to_string(dir.join(MANIFEST))
        .ok()
        .and_then(|text| serde_json::from_str::<Manifest>(&text).ok())
        .filter(|manifest| manifest.format == FORMAT)
}

/// Whether every segment `manifest` names is there.
fn whole(dir: &Path, manifest: &Manifest) -> bool {
    manifest
        .segments
        .iter()
        .all(|name| dir.join(name).is_file())
}

/// The manifest, for a reader: read again while it names a segment that
/// isn't there, as it may for a moment while a writer merges, which writes
/// the new manifest before it removes the old segments; made from the
/// segments themselves if it never names only what is there, or can't be
/// read.
fn load(dir: &Path) -> Manifest {
    let mut known = None;
    for _ in 0..OPEN_ATTEMPTS {
        match read_manifest(dir) {
            Some(manifest) if whole(dir, &manifest) => return manifest,
            Some(manifest) => known = Some(manifest),
            None => break,
        }
    }
    recover(dir, known)
}

/// The manifest, for the writer, holding the lock: as written, if it can be
/// read and names only what is there; else made from the segments, and then
/// `true`. No other writer is at work to have moved it on.
fn load_for_writing(dir: &Path) -> (Manifest, bool) {
    match read_manifest(dir) {
        Some(manifest) if whole(dir, &manifest) => (manifest, false),
        known => {
            let recovered = recover(dir, known);
            // Nothing to make again from nothing: an index not yet written.
            let made = !recovered.segments.is_empty() || dir.join(MANIFEST).exists();
            (recovered, made)
        }
    }
}

/// A manifest made from the segments in `dir`, for one lost, damaged or out
/// of date: every whole segment, oldest first by name, the newest saying
/// which model and size the vectors are. Safe, because a note's newest copy
/// wins whichever segments are listed; the deletions `known` lists still
/// stand, and any it doesn't the next refresh finds.
fn recover(dir: &Path, known: Option<Manifest>) -> Manifest {
    let mut named: Vec<(u64, String)> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().into_string().ok()?;
                    let number = name.strip_suffix(".seg")?.parse::<u64>().ok()?;
                    Some((number, name))
                })
                .collect()
        })
        .unwrap_or_default();
    named.sort();
    let known = known.unwrap_or_default();
    let mut manifest = Manifest {
        format: FORMAT,
        generation: known
            .generation
            .max(named.last().map_or(0, |&(number, _)| number)),
        deleted: known.deleted,
        ..Manifest::default()
    };
    for (_, name) in named {
        if let Ok(segment) = Segment::open(&dir.join(&name)) {
            manifest.signature = segment.model().to_string();
            manifest.dims = segment.dims();
            manifest.segments.push(name);
        }
    }
    manifest
}

/// What a refresh has to read, from the scan and the index side by side,
/// both in path order: notes new or changed; notes written so close to their
/// reading that they are read again to be sure; and with a model, those owed
/// vectors. Notes in scope the scan no longer lists are added to the run's
/// deleted ones.
fn compare<'a>(
    view: &View,
    present: &[&'a NoteMeta],
    in_scope: &dyn Fn(&str) -> bool,
    run: &mut Run<'_, '_>,
) -> Vec<Task<'a>> {
    let racy = run.racy;
    let embedding_on = run.embedding.is_some();
    let mut tasks = Vec::new();
    let (mut i, mut j) = (0, 0);
    loop {
        let order = match (present.get(i), view.notes.get(j)) {
            (Some(meta), Some(&at)) => meta.rel.as_str().cmp(view.note(at).rel.as_str()),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => break,
        };
        match order {
            Ordering::Less => {
                let meta = present[i];
                let guess = (meta.size as usize).div_ceil(text::PASSAGE_BYTES).max(1);
                tasks.push(Task {
                    meta,
                    need: Need::Read,
                    guess,
                });
                i += 1;
            }
            Ordering::Greater => {
                let rel = &view.note(view.notes[j]).rel;
                if in_scope(rel) {
                    run.deleted.insert(rel.clone());
                    run.report.removed += 1;
                }
                j += 1;
            }
            Ordering::Equal => {
                let (meta, info) = (present[i], view.note(view.notes[j]));
                let last = info.stamp.written.max(info.stamp.changed);
                let need = if info.stamp != meta.stamp || info.size != meta.size {
                    Some(Need::Read)
                } else if info.indexed.saturating_sub(last) < racy {
                    Some(Need::Check)
                } else if embedding_on && !info.embedded && info.count > 0 {
                    Some(Need::Vectors)
                } else {
                    None
                };
                if let Some(need) = need {
                    tasks.push(Task {
                        meta,
                        need,
                        guess: info.count as usize,
                    });
                }
                i += 1;
                j += 1;
            }
        }
    }
    tasks
}

/// Reads a note for the chunk being worked on, with its passages' text if a
/// model is at hand, and where its vectors may come from; `None` if it
/// can't be read, or needs nothing after all.
///
/// A note only owed vectors is read again only if the model may still be
/// given one, which, notes being read in path order, makes the notes
/// embedded the first the model's limit allows.
fn read_note(
    task: &Task<'_>,
    view: &View,
    by_hash: &mut Option<HashMap<u64, (usize, usize)>>,
    run: &mut Run<'_, '_>,
) -> io::Result<Option<Item>> {
    let meta = task.meta;
    if task.need == Need::Vectors && !run.may_embed() {
        return Ok(None);
    }
    let Ok(content) = fs::read_to_string(&meta.path) else {
        return Ok(None);
    };
    let hash = fnv(content.as_bytes());
    let own = view.find(&meta.rel);
    // Its text as the index holds it, if that is still what the file says.
    let same = own.filter(|&at| view.note(at).hash == hash);
    // Counted when its file changed, touched or not; read only to be sure
    // of it, when it turns out to have changed after all.
    if task.need == Need::Read || same.is_none() {
        run.report.indexed += 1;
    }
    let (mut record, texts) = record_of(meta, &content, hash);
    record.indexed = run.started;
    let reuse = match same {
        Some(at) if view.note(at).count as usize == record.passages.len() => Reuse::Same(at),
        _ => {
            let mut donors: Vec<(usize, usize)> = own.into_iter().collect();
            if run.embedding.is_some() {
                let copy = by_hash
                    .get_or_insert_with(|| {
                        view.notes
                            .iter()
                            .map(|&at| (view.note(at).hash, at))
                            .collect()
                    })
                    .get(&hash)
                    .copied();
                if let Some(at) = copy
                    && !donors.contains(&at)
                {
                    donors.push(at);
                }
            }
            Reuse::Hashes(donors)
        }
    };
    // A note read again to be sure of it, and found the same, is written
    // with the time of this reading once that time is far enough past its
    // file's to settle the question; until then, written again it would be
    // as unsure as before, so it is left to be read again next time.
    let last = meta.stamp.written.max(meta.stamp.changed);
    let settled = run.started.saturating_sub(last) >= run.racy;
    let changed = match task.need {
        Need::Read => true,
        Need::Check => same.is_none() || settled,
        Need::Vectors => same.is_none(),
    };
    Ok(Some(Item {
        record,
        reuse,
        texts: if run.embedding.is_some() {
            texts
        } else {
            Vec::new()
        },
        sent: 0,
        changed,
    }))
}

/// Which segments are worth merging now, as the first of a run to merge
/// with all after it.
///
/// All of them, when deleted and superseded notes are a large share of what
/// they hold, or the newer ones come to a quarter of the oldest. Otherwise,
/// when there are too many, the newest: enough to come back under the limit,
/// taking in each older one no bigger than those after it together, so the
/// segments stay in order of size, oldest the biggest, and a merge seldom
/// rewrites one much bigger than what it adds.
fn merge_from(view: &View) -> Option<usize> {
    let sizes: Vec<usize> = view.parts.iter().map(|part| part.notes.len()).collect();
    if sizes.is_empty() {
        return None;
    }
    let held: usize = sizes.iter().sum();
    let dead = held - view.notes.len();
    if dead * 4 > held.max(256) {
        return Some(0);
    }
    if sizes.len() < 2 {
        return None;
    }
    let newer: usize = sizes[1..].iter().sum();
    if newer * 4 > sizes[0].max(256) {
        return Some(0);
    }
    if sizes.len() <= MAX_SEGMENTS {
        return None;
    }
    let mut from = sizes.len() - 1;
    let mut size = sizes[from];
    while from > 1 && (from >= MAX_SEGMENTS || sizes[from - 1] <= size) {
        from -= 1;
        size += sizes[from];
    }
    Some(from)
}

/// A vector cut to `dims` components and made unit length again.
///
/// Models trained the Matryoshka way, as EmbeddingGemma is, put the most
/// meaning in the first components, so a shorter vector is the first part of
/// the full one, renormalised.
#[must_use]
pub fn fit(vector: &[f32], dims: usize) -> Vec<f32> {
    let mut cut: Vec<f32> = vector.iter().take(dims).copied().collect();
    cut.resize(dims, 0.0);
    let length = cut.iter().map(|x| x * x).sum::<f32>().sqrt();
    if length > 0.0 {
        for x in &mut cut {
            *x /= length;
        }
    }
    cut
}

/// The text the model is given for a passage of a note called `title`:
/// what a passage's vector is made from, and what its hash is of.
fn model_text(prompts: &Prompts, title: &str, passage: &str) -> String {
    prompts.document(title, &text::readable(passage))
}

/// A note, read into the record the index keeps — its passages and their
/// terms, its links, aliases and tags, no vectors yet — and for embedding,
/// each passage's title, the note's with the headings over it, and text.
#[must_use]
pub fn record_of(meta: &NoteMeta, content: &str, hash: u64) -> (NoteRecord, Vec<(String, String)>) {
    let (yaml, body, _) = note::split_frontmatter(content);
    let frontmatter = yaml.map(note::parse_frontmatter).unwrap_or_default();
    let title = frontmatter.title().unwrap_or_else(|| meta.stem.clone());
    let mut tags = frontmatter.tags();
    for tag in links::extract_tags(body) {
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    let mut targets: Vec<String> = Vec::new();
    for link in links::extract_links(body) {
        let external = link.target.contains("://") || link.target.starts_with("mailto:");
        if !external && !link.target.is_empty() && !targets.contains(&link.target) {
            targets.push(link.target);
        }
    }

    // The title, and the headings over the passage, count as part of it: a
    // passage about a note's subject seldom repeats its name, or its
    // section's.
    let mut texts = Vec::new();
    let passages = text::passages(&meta.rel, content, &title)
        .into_iter()
        .map(|passage| {
            let passage_title = passage.title(&title);
            let mut counts: HashMap<String, u32> = HashMap::new();
            for term in text::index_terms(&passage_title)
                .into_iter()
                .chain(text::index_terms(&passage.text))
            {
                *counts.entry(term).or_default() += 1;
            }
            let length = counts.values().sum();
            let mut terms: Vec<(String, u32)> = counts.into_iter().collect();
            terms.sort();
            texts.push((passage_title, passage.text));
            PassageRecord {
                start: passage.start as u32,
                end: passage.end as u32,
                length,
                terms,
                vector: None,
                hash: 0,
            }
        })
        .collect();
    let record = NoteRecord {
        rel: meta.rel.clone(),
        title,
        stamp: meta.stamp,
        size: meta.size,
        hash,
        indexed: 0,
        aliases: frontmatter.aliases(),
        links: targets,
        tags,
        passages,
    };
    (record, texts)
}

/// FNV-1a: the same on every machine and in every build, as a hash kept on
/// disk must be.
#[must_use]
pub fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A short name for a path, the same every time: what a vault's index
/// directory is called.
#[must_use]
pub fn key(text: &str) -> String {
    format!("{:016x}", fnv(text.as_bytes()))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::retrieval::fake;
    use crate::test_support::TempVault;
    use crate::vault::ScanOptions;

    fn scan(vault: &TempVault) -> Vec<NoteMeta> {
        vault
            .vault()
            .scan(&ScanOptions::default())
            .expect("scan")
            .notes
    }

    fn all(_: &str) -> bool {
        true
    }

    fn refresh(store: &mut Store, vault: &TempVault, model: Option<&mut fake::Words>) -> Refreshed {
        refresh_with(store, vault, model, usize::MAX, &mut |_| {})
    }

    fn refresh_with(
        store: &mut Store,
        vault: &TempVault,
        model: Option<&mut fake::Words>,
        limit: usize,
        progress: &mut dyn FnMut(Progress),
    ) -> Refreshed {
        let prompts = Prompts::default();
        let embedding = model.map(|model| Embedding {
            model,
            prompts: &prompts,
            signature: "fake",
            dims: 0,
            limit,
        });
        store
            .refresh(&scan(vault), &all, embedding, progress)
            .expect("refresh")
    }

    fn sample() -> TempVault {
        let vault = TempVault::new("store");
        vault.write("A.md", "# A\n\nalpha words about hashing\n\n[[B]]\n");
        vault.write("B.md", "# B\n\nbeta words about deploys\n");
        vault
    }

    /// The live notes' paths, in order.
    fn names(store: &Store) -> Vec<String> {
        let view = store.view().expect("view");
        view.notes
            .iter()
            .map(|&at| view.note(at).rel.clone())
            .collect()
    }

    /// The terms of a live note's passages.
    fn store_terms(view: &View, rel: &str) -> Vec<String> {
        let (p, n) = view.find(rel).expect("live");
        let part = &view.parts[p];
        part.segment
            .records(&part.notes, &[n])
            .expect("records")
            .remove(0)
            .passages
            .into_iter()
            .flat_map(|passage| passage.terms.into_iter().map(|(term, _)| term))
            .collect()
    }

    fn segment_files(store: &Store) -> usize {
        std::fs::read_dir(store.dir())
            .expect("dir")
            .filter(|e| {
                e.as_ref()
                    .is_ok_and(|e| e.path().extension().is_some_and(|x| x == "seg"))
            })
            .count()
    }

    #[test]
    fn words_and_links_are_indexed_without_a_model() {
        let vault = sample();
        let mut store = Store::open(&vault.path().join(".index"));
        let report = refresh(&mut store, &vault, None);
        assert_eq!((report.indexed, report.embedded, report.pending), (2, 0, 2));

        let view = store.view().expect("view");
        assert_eq!(view.notes.len(), 2);
        let a = view.note(view.find("A.md").expect("A"));
        assert_eq!(a.links(), vec!["B"]);
        assert!(!a.embedded);
        assert_eq!(store.signature(), "");
    }

    #[test]
    fn vectors_are_added_when_a_model_comes_and_only_what_changed_is_redone() {
        let vault = sample();
        let mut store = Store::open(&vault.path().join(".index"));
        refresh(&mut store, &vault, None);

        let mut model = fake::Words::default();
        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!((report.indexed, report.embedded, report.pending), (0, 2, 0));
        assert_eq!(model.embedded, 2, "one passage each");
        assert_eq!(store.signature(), "fake");

        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(report, Refreshed::default(), "nothing to do");
        assert_eq!(model.embedded, 2);

        vault.write("A.md", "# A\n\nalpha words changed\n");
        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!((report.indexed, report.embedded), (1, 1));
        assert_eq!(model.embedded, 3);
    }

    #[test]
    fn a_renamed_note_keeps_its_vectors_and_a_deleted_one_goes() {
        let vault = TempVault::new("store-rename");
        vault.write("A.md", "# A\n\nalpha words about hashing\n\n[[B]]\n");
        // Its title in its frontmatter, so the model reads the same text
        // after the rename as before it.
        vault.write(
            "B.md",
            "---\ntitle: Beta\n---\n# B\n\nbeta words about deploys\n",
        );
        let mut store = Store::open(&vault.path().join(".index"));
        let mut model = fake::Words::default();
        refresh(&mut store, &vault, Some(&mut model));

        let b = vault.read("B.md");
        std::fs::remove_file(vault.path().join("B.md")).expect("rm");
        vault.write("Renamed.md", &b);
        std::fs::remove_file(vault.path().join("A.md")).expect("rm");
        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(
            report.embedded, 0,
            "the rename brought its vectors: {report:?}"
        );
        assert_eq!(report.removed, 2);

        let view = store.view().expect("view");
        let names: Vec<&str> = view
            .notes
            .iter()
            .map(|&at| view.note(at).rel.as_str())
            .collect();
        assert_eq!(names, vec!["Renamed.md"]);
        assert!(view.note(view.notes[0]).embedded);
        assert_eq!(view.note(view.notes[0]).title, "Beta");
    }

    #[test]
    fn a_note_renamed_with_its_title_is_read_and_embedded_as_the_new_name() {
        let vault = TempVault::new("store-rename-title");
        vault.write("Old Name.md", "some words about rockets and engines\n");
        let mut store = Store::open(&vault.path().join(".index"));
        let mut model = fake::Words::default();
        refresh(&mut store, &vault, Some(&mut model));

        let text = vault.read("Old Name.md");
        std::fs::remove_file(vault.path().join("Old Name.md")).expect("rm");
        vault.write("Kestrel.md", &text);
        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(
            (report.indexed, report.embedded),
            (1, 1),
            "the model reads the title, so a new one is embedded again"
        );
        let view = store.view().expect("view");
        let note = view.note(view.find("Kestrel.md").expect("renamed"));
        assert_eq!(note.title, "Kestrel");
        assert!(note.embedded);
        let first = view
            .parts
            .iter()
            .find(|part| part.notes.iter().any(|n| n.rel == "Kestrel.md"));
        assert!(first.is_some());
        let terms = store_terms(&view, "Kestrel.md");
        assert!(terms.contains(&"kestrel".to_string()), "{terms:?}");
        assert!(!terms.contains(&"old".to_string()), "{terms:?}");
    }

    #[test]
    fn renamed_notes_bring_their_vectors_from_whichever_segment_holds_them() {
        let vault = TempVault::new("store-renames");
        for name in ["A", "B", "C", "D"] {
            vault.write(
                &format!("{name}.md"),
                &format!("# {name}\n\n{name} words to keep\n"),
            );
        }
        let mut store = Store::open(&vault.path().join(".index"));
        store.chunk = 1;
        let mut model = fake::Words::default();
        refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(store.manifest.segments.len(), 4, "a segment a note");

        for name in ["A", "B", "C", "D"] {
            let text = vault.read(&format!("{name}.md"));
            std::fs::remove_file(vault.path().join(format!("{name}.md"))).expect("rm");
            vault.write(&format!("Moved/{name}.md"), &text);
        }
        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(
            (
                report.indexed,
                report.embedded,
                report.removed,
                report.pending
            ),
            (4, 0, 4, 0)
        );
        assert_eq!(model.embedded, 4, "nothing was embedded twice");
        assert_eq!(
            names(&store),
            ["Moved/A.md", "Moved/B.md", "Moved/C.md", "Moved/D.md"]
        );
        let view = store.view().expect("view");
        assert!(view.notes.iter().all(|&at| view.note(at).embedded));
    }

    #[test]
    fn a_new_model_clears_the_old_vectors() {
        let vault = sample();
        let mut store = Store::open(&vault.path().join(".index"));
        let mut model = fake::Words::default();
        refresh(&mut store, &vault, Some(&mut model));

        let prompts = Prompts::default();
        let report = store
            .refresh(
                &scan(&vault),
                &all,
                Some(Embedding {
                    model: &mut model,
                    prompts: &prompts,
                    signature: "another",
                    dims: 0,
                    limit: 1,
                }),
                &mut |_| {},
            )
            .expect("refresh");
        assert_eq!((report.embedded, report.pending), (1, 1), "{report:?}");
        assert_eq!(store.signature(), "another");
        let view = store.view().expect("view");
        let embedded = view
            .notes
            .iter()
            .filter(|&&at| view.note(at).embedded)
            .count();
        assert_eq!(embedded, 1, "the old model's vectors are gone, not mixed");
    }

    #[test]
    fn a_model_that_fails_keeps_the_words_and_says_why() {
        let vault = sample();
        let mut store = Store::open(&vault.path().join(".index"));
        let mut model = fake::Words {
            broken: true,
            ..Default::default()
        };
        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(report.indexed, 2);
        assert_eq!(report.embedded, 0);
        assert!(
            report
                .model_error
                .as_deref()
                .unwrap_or_default()
                .contains("refused")
        );
        assert_eq!(
            store.view().expect("view").notes.len(),
            2,
            "searchable by words"
        );
        assert_eq!(store.signature(), "", "no vectors, so no model yet");
    }

    #[test]
    fn a_model_that_fails_part_way_keeps_each_note_it_finished() {
        let vault = TempVault::new("store-fail-midway");
        vault.write("A.md", "# A\n\nshort and done\n");
        // Forty passages: its vectors come in two batches.
        let long: Vec<String> = (0..40)
            .map(|i| format!("paragraph {i} {}", "words ".repeat(200)))
            .collect();
        vault.write("B.md", &long.join("\n\n"));
        vault.write("C.md", "# C\n\nshort and waiting\n");
        let mut store = Store::open(&vault.path().join(".index"));
        let mut model = fake::Words {
            fails_after: Some(BATCH),
            ..Default::default()
        };
        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(model.embedded, BATCH, "one batch, of A and most of B");
        assert_eq!((report.indexed, report.embedded), (3, 1), "{report:?}");
        assert!(report.model_error.is_some());
        assert_eq!(report.pending, 2);

        let view = store.view().expect("view");
        let embedded: Vec<(&str, bool)> = view
            .notes
            .iter()
            .map(|&at| (view.note(at).rel.as_str(), view.note(at).embedded))
            .collect();
        assert_eq!(
            embedded,
            [("A.md", true), ("B.md", false), ("C.md", false)],
            "B's vectors came only in part, so it has none"
        );
        let b = view.find("B.md").expect("B");
        assert!(view.note(b).count >= 40);

        // The next refresh finishes the job.
        let mut model = fake::Words::default();
        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!((report.indexed, report.embedded, report.pending), (0, 2, 0));
    }

    #[test]
    fn the_limit_caps_the_notes_embedded_and_progress_adds_up() {
        let vault = TempVault::new("store-limit");
        for i in 0..6 {
            vault.write(&format!("N{i}.md"), &format!("# N{i}\n\nnote {i} words\n"));
        }
        let mut store = Store::open(&vault.path().join(".index"));
        store.chunk = 1;
        refresh(&mut store, &vault, None);

        let mut model = fake::Words::default();
        let mut seen = Vec::new();
        let report = refresh_with(&mut store, &vault, Some(&mut model), 2, &mut |p| {
            seen.push(p);
        });
        assert_eq!((report.indexed, report.embedded, report.pending), (0, 2, 4));
        assert_eq!(model.embedded, 2);
        assert_eq!(
            seen.last(),
            Some(&Progress { done: 2, total: 2 }),
            "{seen:?}"
        );
        let view = store.view().expect("view");
        let embedded: Vec<&str> = view
            .notes
            .iter()
            .filter(|&&at| view.note(at).embedded)
            .map(|&at| view.note(at).rel.as_str())
            .collect();
        assert_eq!(embedded, ["N0.md", "N1.md"], "the first, in path order");
    }

    #[test]
    fn a_big_refresh_is_written_a_chunk_at_a_time_and_merged_once() {
        let vault = TempVault::new("store-chunks");
        for i in 0..300 {
            vault.write(
                &format!("N{i:03}.md"),
                &format!("# N{i}\n\nnote {i} about chunks\n"),
            );
        }
        let mut store = Store::open(&vault.path().join(".index"));
        store.chunk = 64;
        let mut model = fake::Words::default();
        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(
            (report.indexed, report.embedded, report.pending),
            (300, 300, 0)
        );
        assert_eq!(store.manifest.segments.len(), 1);
        assert_eq!(
            store.manifest.generation, 6,
            "five chunks written, then one merge"
        );
        assert_eq!(segment_files(&store), 1);
        assert_eq!(names(&store).len(), 300);
    }

    #[test]
    fn a_very_long_refresh_merges_as_it_goes_to_keep_its_files_few() {
        let vault = TempVault::new("store-gather");
        for i in 0..40 {
            vault.write(
                &format!("N{i:02}.md"),
                &format!("# N{i}\n\nnote {i} gathered\n"),
            );
        }
        let mut store = Store::open(&vault.path().join(".index"));
        store.chunk = 1;
        refresh(&mut store, &vault, None);
        // Forty segments of one note; after sixteen and thirty-two, a merge;
        // at the end, the eight left over.
        assert_eq!(store.manifest.generation, 40 + 2 + 1);
        assert_eq!(store.manifest.segments.len(), 3);
        assert_eq!(segment_files(&store), 3);
        assert_eq!(names(&store).len(), 40);
    }

    #[test]
    fn updates_append_segments_and_many_of_them_merge() {
        let vault = sample();
        let mut store = Store::open(&vault.path().join(".index"));
        refresh(&mut store, &vault, None);
        for i in 0..10 {
            vault.write("A.md", &format!("# A\n\nversion {i}\n"));
            refresh(&mut store, &vault, None);
            assert!(store.manifest.segments.len() <= MAX_SEGMENTS);
        }
        let view = store.view().expect("view");
        assert_eq!(view.notes.len(), 2);
        assert_eq!(
            segment_files(&store),
            store.manifest.segments.len(),
            "merged segments are removed"
        );
    }

    #[test]
    fn a_partial_merge_keeps_the_newest_copies_and_the_deletions_still_held() {
        let vault = TempVault::new("store-partial");
        for name in ["A", "B", "C", "D", "E"] {
            vault.write(
                &format!("{name}.md"),
                &format!("# {name}\n\n{name} first draft\n"),
            );
        }
        let mut store = Store::open(&vault.path().join(".index"));
        // Each refresh writes only what was edited, as a slower hand's would.
        store.racy = 0;
        refresh(&mut store, &vault, None);
        let edits: [&dyn Fn(); 8] = [
            &|| {
                vault.write("A.md", "# A\n\nA second draft\n");
            },
            &|| std::fs::remove_file(vault.path().join("B.md")).expect("rm"),
            &|| {
                vault.write("F.md", "# F\n\nF short lived\n");
            },
            &|| {
                vault.write("A.md", "# A\n\nA third draft\n");
            },
            &|| std::fs::remove_file(vault.path().join("F.md")).expect("rm"),
            &|| {
                vault.write("C.md", "# C\n\nC second draft\n");
            },
            &|| {
                vault.write("D.md", "# D\n\nD second draft\n");
            },
            &|| {
                vault.write("E.md", "# E\n\nE second draft\n");
            },
        ];
        for edit in edits {
            edit();
            refresh(&mut store, &vault, None);
        }
        // The six newer segments merged into one; the first, holding the
        // first drafts and B, stays.
        assert_eq!(store.manifest.segments.len(), 2, "{:?}", store.manifest);
        assert_eq!(store.manifest.deleted, ["B.md"], "F was in no segment left");
        assert_eq!(names(&store), ["A.md", "C.md", "D.md", "E.md"]);
        let view = store.view().expect("view");
        let a = view.note(view.find("A.md").expect("A"));
        assert_eq!(
            (a.size, view.find("A.md").expect("A").0),
            (vault.read("A.md").len() as u64, 1),
            "the newest copy, from the merged segment"
        );
        let held: Vec<&str> = view.parts[0].notes.iter().map(|n| n.rel.as_str()).collect();
        assert_eq!(held, ["A.md", "B.md", "C.md", "D.md", "E.md"]);
        assert!(view.parts[0].live.iter().all(|live| !live));
    }

    #[test]
    fn a_deletion_alone_moves_the_generation_on_and_the_links_with_it() {
        let vault = sample();
        let mut store = Store::open(&vault.path().join(".index"));
        refresh(&mut store, &vault, None);
        let before = store.manifest.generation;
        let path = store.dir().join(GRAPH);
        let links = Links::read(&path, before, 2).expect("resolved when written");
        assert_eq!(links.outgoing(0), [1], "A links to B");
        assert_eq!(links.incoming(1), [0]);

        std::fs::remove_file(vault.path().join("B.md")).expect("rm");
        let report = refresh(&mut store, &vault, None);
        assert_eq!((report.indexed, report.removed), (0, 1));
        assert!(store.manifest.generation > before);
        assert!(
            Links::read(&path, before, 2).is_none(),
            "the old links are out of date"
        );
        let view = store.take_view().expect("view");
        assert_eq!(view.notes.len(), 1);
        let links = view.graph.expect("the links as they are now");
        assert!(links.outgoing(0).is_empty(), "B is gone: {links:?}");
    }

    #[test]
    fn a_refresh_hands_over_the_view_it_ends_with() {
        let vault = sample();
        let mut store = Store::open(&vault.path().join(".index"));
        store.racy = 0;
        refresh(&mut store, &vault, None);
        let view = store.take_view().expect("view");
        assert_eq!(view.notes.len(), 2);
        assert!(view.graph.is_some());
        assert!(store.fresh.is_none(), "handed over, not kept");

        // With nothing to do, the same, and the links are read, not resolved.
        let path = store.dir().join(GRAPH);
        let written = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .expect("graph");
        refresh(&mut store, &vault, None);
        let view = store.take_view().expect("view");
        assert_eq!(view.notes.len(), 2);
        assert_eq!(view.graph, Links::read(&path, store.manifest.generation, 2));
        assert_eq!(
            std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .expect("graph"),
            written
        );
    }

    #[test]
    fn a_held_lock_turns_a_refresh_away_until_it_is_let_go() {
        let vault = sample();
        let dir = vault.path().join(".index");
        let mut store = Store::open(&dir);
        let held = store.lock().expect("lock");
        assert!(
            store.lock().is_none(),
            "one writer at a time, even in one process"
        );
        assert_eq!(
            store.refresh(&scan(&vault), &all, None, &mut |_| {}),
            Err(BUSY.to_string())
        );
        drop(held);
        assert!(
            store
                .refresh(&scan(&vault), &all, None, &mut |_| {})
                .is_ok()
        );
        assert!(
            dir.join(LOCK).exists(),
            "the file stays; only the lock goes"
        );
        assert!(store.lock().is_some());
    }

    #[test]
    fn a_damaged_index_is_rebuilt_from_the_notes() {
        let vault = sample();
        let dir = vault.path().join(".index");
        let mut store = Store::open(&dir);
        refresh(&mut store, &vault, None);
        let name = store.manifest.segments[0].clone();
        std::fs::write(dir.join(&name), b"garbage").expect("write");

        let mut store = Store::open(&dir);
        assert!(store.view().is_err());
        let report = refresh(&mut store, &vault, None);
        assert_eq!(report.indexed, 2);
        assert_eq!(store.view().expect("view").notes.len(), 2);
    }

    #[test]
    fn a_scoped_refresh_leaves_the_rest_alone() {
        let vault = TempVault::new("store-scope");
        vault.write("In/A.md", "inside\n");
        vault.write("Out/B.md", "outside\n");
        let mut store = Store::open(&vault.path().join(".index"));
        store
            .refresh(
                &scan(&vault),
                &|rel| rel.starts_with("In/"),
                None,
                &mut |_| {},
            )
            .expect("refresh");
        let view = store.view().expect("view");
        assert!(view.find("In/A.md").is_some());
        assert!(view.find("Out/B.md").is_none());
        assert_eq!(view.stale(&scan(&vault)).len(), 1);
    }

    #[test]
    fn records_carry_title_terms_links_and_tags() {
        let vault = TempVault::new("store-record");
        let path = vault.write(
            "Note.md",
            "---\ntitle: Release Process\naliases: [Shipping]\ntags: [ops]\n---\n# Steps\n\nPush a tag. See [[CI]] and [site](https://x.y).\n#deploy\n",
        );
        let meta = &scan(&vault)[0];
        let content = std::fs::read_to_string(path).expect("read");
        let (record, texts) = record_of(meta, &content, 1);
        assert_eq!(record.title, "Release Process");
        assert_eq!(record.aliases, vec!["Shipping"]);
        assert_eq!(record.tags, vec!["ops", "deploy"]);
        assert_eq!(record.links, vec!["CI"], "web links are not graph edges");
        assert_eq!(texts.len(), record.passages.len());
        assert_eq!(texts[0].0, "Release Process", "what the model is told");
        let terms: Vec<&str> = record.passages[0]
            .terms
            .iter()
            .map(|(t, _)| t.as_str())
            .collect();
        assert!(
            terms.contains(&"releas") && terms.contains(&"tag"),
            "the title's terms, stemmed: {terms:?}"
        );
    }

    /// A long note, its passages kept apart by headings.
    fn sections(names: &[&str]) -> String {
        names
            .iter()
            .map(|name| {
                format!(
                    "## {name}\n\n{name} {}\n",
                    "words about the section ".repeat(40)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn an_edited_note_sends_the_model_only_the_passages_that_changed() {
        let vault = TempVault::new("store-reuse");
        vault.write(
            "Log.md",
            &sections(&["one", "two", "three", "four", "five"]),
        );
        let mut store = Store::open(&vault.path().join(".index"));
        let mut model = fake::Words::default();
        refresh(&mut store, &vault, Some(&mut model));
        let passages = store.view().expect("view").passage_count();
        assert_eq!((passages, model.embedded), (5, 5));

        // Appended to: the new passage goes to the model, the rest don't.
        vault.write(
            "Log.md",
            &sections(&["one", "two", "three", "four", "five", "six"]),
        );
        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!((report.indexed, report.embedded), (1, 1), "{report:?}");
        assert_eq!(model.embedded, 6, "one passage, not six");

        // Edited in the middle.
        vault.write(
            "Log.md",
            &sections(&["one", "two", "THREE", "four", "five", "six"]),
        );
        refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(model.embedded, 7);

        // Reordered: every passage is one the model has read.
        vault.write(
            "Log.md",
            &sections(&["six", "five", "four", "THREE", "two", "one"]),
        );
        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(model.embedded, 7, "nothing new to read: {report:?}");
        assert_eq!(report.embedded, 0);

        // And each passage has the vector of its own text.
        let view = store.view().expect("view");
        let (p, n) = view.find("Log.md").expect("log");
        let part = &view.parts[p];
        let record = part
            .segment
            .records(&part.notes, &[n])
            .expect("records")
            .remove(0);
        let content = vault.read("Log.md");
        let texts: Vec<(String, String)> = text::passages("Log.md", &content, "Log")
            .into_iter()
            .map(|p| (p.title("Log"), p.text))
            .collect();
        assert_eq!(texts.len(), record.passages.len());
        let prompts = Prompts::default();
        for (passage, (title, text)) in record.passages.iter().zip(&texts) {
            let vector = fake::vector(&model_text(&prompts, title, text));
            let expected = Quantized::from_unit(&fit(&vector, view.dims));
            assert_eq!(passage.vector.as_ref(), Some(&expected), "{text:.20}");
            assert_eq!(
                passage.hash,
                fnv(model_text(&prompts, title, text).as_bytes())
            );
        }
    }

    #[test]
    fn a_model_that_fails_part_way_keeps_the_passages_it_embedded() {
        let vault = TempVault::new("store-reuse-partial");
        let long: Vec<String> = (0..40)
            .map(|i| format!("paragraph {i} {}", "words ".repeat(200)))
            .collect();
        vault.write("B.md", &long.join("\n\n"));
        let mut store = Store::open(&vault.path().join(".index"));
        let mut model = fake::Words {
            fails_after: Some(BATCH),
            ..Default::default()
        };
        refresh(&mut store, &vault, Some(&mut model));
        let mut model = fake::Words::default();
        let report = refresh(&mut store, &vault, Some(&mut model));
        let count = store.view().expect("view").passage_count();
        assert_eq!(report.embedded, 1);
        assert_eq!(
            model.embedded,
            count - BATCH,
            "only what didn't come the first time"
        );
    }

    #[test]
    fn an_edit_a_coarse_clock_hides_is_found_while_the_note_is_racy() {
        let vault = TempVault::new("store-racy");
        vault.write("Task.md", "# Task\n\nstatus: pending\n");
        let mut store = Store::open(&vault.path().join(".index"));
        let first = scan(&vault);
        store
            .refresh(&first, &all, None, &mut |_| {})
            .expect("refresh");

        // An edit of the same size that a coarse filesystem clock doesn't
        // show: the scan finds the file's times and number as they were.
        vault.write("Task.md", "# Task\n\nstatus: shipped\n");
        let mut second = scan(&vault);
        assert_eq!(second[0].size, first[0].size);
        second[0].stamp = first[0].stamp;
        let report = store
            .refresh(&second, &all, None, &mut |_| {})
            .expect("refresh");
        assert_eq!(report.indexed, 1, "read again, and found changed");
        let view = store.view().expect("view");
        let task = view.note(view.find("Task.md").expect("task"));
        assert_eq!(task.hash, fnv(vault.read("Task.md").as_bytes()));

        // A note read long after it was written is trusted, as git trusts
        // it: such an edit would then go unseen until the next.
        store.racy = 0;
        vault.write("Task.md", "# Task\n\nstatus: blocked\n");
        let report = store
            .refresh(&second, &all, None, &mut |_| {})
            .expect("refresh");
        assert_eq!(report.indexed, 0);
    }

    /// Other systems keep no time of change, only of writing.
    #[cfg(unix)]
    #[test]
    fn a_rewrite_that_puts_the_time_back_is_found_by_the_time_of_change() {
        let vault = TempVault::new("store-ctime");
        let path = vault.write("Task.md", "# Task\n\nstatus: pending\n");
        let then = std::fs::metadata(&path)
            .expect("meta")
            .modified()
            .expect("time");
        let mut store = Store::open(&vault.path().join(".index"));
        store.racy = 0;
        refresh(&mut store, &vault, None);
        vault.write("Task.md", "# Task\n\nstatus: shipped\n");
        File::options()
            .write(true)
            .open(&path)
            .expect("open")
            .set_modified(then)
            .expect("time");
        let report = refresh(&mut store, &vault, None);
        assert_eq!(report.indexed, 1);
    }

    #[test]
    fn a_note_read_again_to_be_sure_of_it_is_written_again_once_that_settles_it() {
        let vault = sample();
        let mut store = Store::open(&vault.path().join(".index"));
        // A window short enough to wait out.
        store.racy = 200_000_000;
        let mut model = fake::Words::default();
        refresh(&mut store, &vault, Some(&mut model));
        let generation = store.manifest.generation;

        // Read again so soon that writing it down would settle nothing.
        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(report, Refreshed::default());
        assert_eq!(store.manifest.generation, generation, "nothing written");

        // Read again once its time is far enough behind: written with the
        // time of this reading, not counted as changed, its vectors its own.
        std::thread::sleep(Duration::from_millis(300));
        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(report, Refreshed::default());
        assert!(
            store.manifest.generation > generation,
            "written again, as read"
        );
        assert_eq!(model.embedded, 2, "its vectors are its own still");
        let view = store.view().expect("view");
        assert!(view.notes.iter().all(|&at| view.note(at).embedded));

        // And then it is no longer read at all.
        let generation = store.manifest.generation;
        refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(store.manifest.generation, generation);
    }

    #[test]
    fn a_lost_manifest_is_made_again_from_the_segments() {
        let vault = sample();
        let dir = vault.path().join(".index");
        let mut store = Store::open(&dir);
        store.chunk = 1;
        let mut model = fake::Words::default();
        refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(store.manifest.segments.len(), 2);
        std::fs::write(dir.join(MANIFEST), b"{ torn").expect("damage");

        // A reader makes do with the segments.
        let reader = Store::open(&dir);
        assert_eq!(reader.signature(), "fake", "each segment says which model");
        assert_eq!(reader.view().expect("view").notes.len(), 2);

        // A writer writes the manifest down again, and keeps the vectors.
        let mut store = Store::open(&dir);
        store.racy = 0;
        let report = refresh(&mut store, &vault, Some(&mut model));
        assert_eq!(report, Refreshed::default());
        assert_eq!(model.embedded, 2, "nothing embedded again");
        assert!(read_manifest(&dir).is_some());
        assert_eq!(store.signature(), "fake");
    }

    #[test]
    fn a_manifest_naming_a_segment_that_is_gone_takes_in_the_segments_there() {
        let vault = sample();
        let dir = vault.path().join(".index");
        let mut store = Store::open(&dir);
        store.racy = 0;
        refresh(&mut store, &vault, None);
        vault.write("A.md", "# A\n\nalpha words changed\n");
        refresh(&mut store, &vault, None);
        let before = read_manifest(&dir).expect("manifest");
        assert_eq!(before.segments.len(), 2);
        // A merge, and then a crash before its manifest reached the disk:
        // the one there names the segments the merge removed.
        let view = store.take_view().expect("view");
        store.merge(view, 0, false).expect("merge");
        assert!(before.segments.iter().all(|name| !dir.join(name).exists()));
        std::fs::write(
            dir.join(MANIFEST),
            serde_json::to_string(&before).expect("json"),
        )
        .expect("write");

        let reader = Store::open(&dir);
        assert_eq!(reader.view().expect("view").notes.len(), 2);
        let mut store = Store::open(&dir);
        store.racy = 0;
        let report = refresh(&mut store, &vault, None);
        assert_eq!(
            report.indexed, 0,
            "the newest copies were all there: {report:?}"
        );
        assert_eq!(names(&store), ["A.md", "B.md"]);
        let view = store.view().expect("view");
        assert_eq!(
            view.note(view.find("A.md").expect("A")).hash,
            fnv(vault.read("A.md").as_bytes())
        );
        assert!(whole(
            &dir,
            &read_manifest(&dir).expect("written down again")
        ));
    }

    #[test]
    fn a_refresh_sweeps_away_what_no_one_will_read() {
        let vault = sample();
        let dir = vault.path().join(".index");
        let mut store = Store::open(&dir);
        refresh(&mut store, &vault, None);
        for leftover in [
            "00000099.seg",
            "00000003.part123",
            "00000003.terms123",
            "00000003.postings123",
            "manifest.json.123",
        ] {
            std::fs::write(dir.join(leftover), b"left by a process that died").expect("write");
        }
        store.asked();
        refresh(&mut store, &vault, None);
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .expect("dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        let mut expected: Vec<String> = [ASKED, GRAPH, LOCK, MANIFEST]
            .iter()
            .map(|s| s.to_string())
            .collect();
        expected.extend(store.manifest.segments.iter().cloned());
        expected.sort();
        assert_eq!(files, expected);

        // And --rebuild throws away every segment, listed or not.
        std::fs::write(dir.join("00000098.seg"), b"stray").expect("write");
        store.clear().expect("clear");
        assert_eq!(segment_files(&store), 0);
        assert_eq!(refresh(&mut store, &vault, None).indexed, 2);
    }

    #[test]
    fn the_time_of_a_question_is_kept_without_the_manifest() {
        let vault = sample();
        let dir = vault.path().join(".index");
        let mut store = Store::open(&dir);
        refresh(&mut store, &vault, None);
        assert_eq!(store.last_question(), 0);
        let manifest = std::fs::read(dir.join(MANIFEST)).expect("manifest");
        let held = store.lock().expect("lock");
        store.asked();
        drop(held);
        assert!(
            store.last_question() + 5 >= now(),
            "asked even while another writes"
        );
        assert_eq!(
            std::fs::read(dir.join(MANIFEST)).expect("manifest"),
            manifest
        );
    }

    #[test]
    fn an_answer_from_the_prepared_index_holds_unless_the_refresh_changes_it() {
        let vault = sample();
        let mut store = Store::open(&vault.path().join(".index"));
        store.racy = 0;
        refresh(&mut store, &vault, None);

        let mut store = Store::open(&vault.path().join(".index"));
        store.racy = 0;
        store.prepare().expect("prepare");
        assert_eq!(store.prepared().expect("prepared").notes.len(), 2);
        refresh(&mut store, &vault, None);
        assert!(store.unchanged());
        assert!(store.take_view().expect("view").graph.is_some());

        store.prepare().expect("prepare");
        vault.write("C.md", "# C\n\ngamma\n");
        refresh(&mut store, &vault, None);
        assert!(!store.unchanged());
        assert_eq!(store.take_view().expect("view").notes.len(), 3);

        // Turned away, the prepared index is the one answered from.
        store.prepare().expect("prepare");
        let held = store.lock().expect("lock");
        assert!(refresh_result(&mut store, &vault).is_err());
        assert!(store.unchanged());
        drop(held);
        assert_eq!(store.take_view().expect("view").notes.len(), 3);
    }

    fn refresh_result(store: &mut Store, vault: &TempVault) -> Result<Refreshed, String> {
        store.refresh(&scan(vault), &all, None, &mut |_| {})
    }

    #[test]
    fn many_parts_are_read_as_one_index_in_path_order() {
        let vault = TempVault::new("store-assemble");
        for name in ["B", "D", "F"] {
            vault.write(&format!("{name}.md"), &format!("# {name}\n\nfirst\n"));
        }
        let mut store = Store::open(&vault.path().join(".index"));
        store.racy = 0;
        refresh(&mut store, &vault, None);
        vault.write("A.md", "# A\n\nnew\n");
        vault.write("D.md", "# D\n\nsecond draft\n");
        refresh(&mut store, &vault, None);
        std::fs::remove_file(vault.path().join("B.md")).expect("rm");
        vault.write("E.md", "# E\n\nnew\n");
        refresh(&mut store, &vault, None);
        let view = store.view().expect("view");
        assert!(view.parts.len() > 1, "{}", view.parts.len());
        assert_eq!(names(&store), ["A.md", "D.md", "E.md", "F.md"]);
        let d = view.note(view.find("D.md").expect("D"));
        assert_eq!(d.size, vault.read("D.md").len() as u64, "the newest copy");
        let live: usize = view
            .parts
            .iter()
            .map(|part| part.live.iter().filter(|l| **l).count())
            .sum();
        assert_eq!(live, 4);
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::retrieval::search::{self, Question};
    use crate::retrieval::{Embed, fake};
    use crate::test_support::TempVault;
    use crate::vault::ScanOptions;
    use std::time::{Duration, SystemTime};

    fn scan(vault: &TempVault) -> Vec<NoteMeta> {
        vault
            .vault()
            .scan(&ScanOptions::default())
            .expect("scan")
            .notes
    }

    fn plain(store: &mut Store, vault: &TempVault) -> Result<Refreshed, String> {
        store.refresh(&scan(vault), &|_| true, None, &mut |_| {})
    }

    fn ask(view: &View, text: &str) -> Vec<(String, String)> {
        search::ask(
            view,
            &Question {
                text,
                vector: None,
                in_scope: &|_| true,
                prior: &|_| 0.0,
                floor: 0.6,
                limit: 10,
            },
        )
        .expect("ask")
        .notes
        .into_iter()
        .map(|hit| (hit.rel, hit.title))
        .collect()
    }

    /// A model that, on one call, plays a second process arriving 16 minutes
    /// into the first's refresh: the user has saved a note, and a question
    /// (`emeraldian recall`) refreshes the index.
    struct Interloper<'v> {
        calls: usize,
        at: usize,
        vault: &'v TempVault,
        dir: PathBuf,
        second: Option<Result<Refreshed, String>>,
        lock_gone_after: Option<bool>,
    }

    impl Embed for Interloper<'_> {
        fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
            self.calls += 1;
            if self.calls == self.at {
                let lock = File::options()
                    .write(true)
                    .open(self.dir.join(LOCK))
                    .expect("the first process holds the lock");
                lock.set_modified(SystemTime::now() - Duration::from_secs(16 * 60))
                    .expect("age");
                drop(lock);
                self.vault
                    .write("N00.md", "# N00\n\nedited while indexing\n");
                let mut other = Store::open(&self.dir);
                self.second = Some(plain(&mut other, self.vault));
                self.lock_gone_after = Some(!self.dir.join(LOCK).exists());
            }
            Ok(texts.iter().map(|t| fake::vector(t)).collect())
        }
    }

    #[test]
    fn review_a_refresh_longer_than_the_stale_limit_loses_its_lock() {
        let vault = TempVault::new("review-lock");
        for i in 0..12 {
            vault.write(
                &format!("N{i:02}.md"),
                &format!("# N{i}\n\nnote {i} words\n"),
            );
        }
        let dir = vault.path().join(".index");
        let mut first = Store::open(&dir);
        first.chunk = 1;
        plain(&mut first, &vault).expect("words first");

        let mut model = Interloper {
            calls: 0,
            at: 8,
            vault: &vault,
            dir: dir.clone(),
            second: None,
            lock_gone_after: None,
        };
        let prompts = Prompts::default();
        let result = first.refresh(
            &scan(&vault),
            &|_| true,
            Some(Embedding {
                model: &mut model,
                prompts: &prompts,
                signature: "fake",
                dims: 0,
                limit: usize::MAX,
            }),
            &mut |_| {},
        );
        eprintln!("second process: {:?}", model.second);
        eprintln!(
            "lock file gone after the second let go: {:?}",
            model.lock_gone_after
        );
        eprintln!("first process: {result:?}");
        let manifest = load(&dir);
        eprintln!(
            "manifest now: gen {} {:?}",
            manifest.generation, manifest.segments
        );
        let missing: Vec<&String> = manifest
            .segments
            .iter()
            .filter(|name| !dir.join(name).exists())
            .collect();
        eprintln!("segments the manifest names that are gone: {missing:?}");
        let view = Store::open(&dir).view();
        eprintln!(
            "a reader now: {:?}",
            view.as_ref()
                .map(|v| v.notes.len())
                .map_err(|e| e.to_string())
        );
        assert!(
            view.is_ok(),
            "the index is broken after two writers ran at once"
        );
    }

    #[test]
    fn review_a_reader_that_read_the_manifest_before_a_merge_fails() {
        let vault = TempVault::new("review-reader");
        for i in 0..3 {
            vault.write(&format!("N{i}.md"), &format!("# N{i}\n\nnote {i} words\n"));
        }
        let dir = vault.path().join(".index");
        let mut writer = Store::open(&dir);
        plain(&mut writer, &vault).expect("first");
        for v in 0..5 {
            vault.write(
                "N1.md",
                &format!("# N1\n\nversion {v} words {}\n", "x".repeat(v)),
            );
            plain(&mut writer, &vault).expect("update");
        }
        eprintln!("before: {:?}", writer.manifest.segments);
        // A question starts: Store::open reads the manifest, then the vault is scanned.
        let mut reader = Store::open(&dir);
        // Meanwhile another process, holding the lock, writes and merges.
        vault.write("N2.md", "# N2\n\nchanged words\n");
        plain(&mut writer, &vault).expect("merge");
        eprintln!("after: {:?}", writer.manifest.segments);
        // The reader's refresh is turned away while some writer holds the lock...
        let held = writer.lock().expect("lock");
        assert_eq!(plain(&mut reader, &vault), Err(BUSY.to_string()));
        // ...so it answers from the manifest it read at the start.
        let view = reader.take_view();
        drop(held);
        eprintln!(
            "reader: {:?}",
            view.as_ref()
                .map(|v| v.notes.len())
                .map_err(|e| e.to_string())
        );
        assert!(
            view.is_ok(),
            "a reader fails when a merge removed what its manifest names"
        );
    }

    #[test]
    fn review_a_renamed_note_keeps_its_old_title() {
        let vault = TempVault::new("review-title");
        vault.write("Old Name.md", "some words about rockets and engines\n");
        vault.write("Other.md", "unrelated text about gardening\n");
        let dir = vault.path().join(".index");
        let mut store = Store::open(&dir);
        plain(&mut store, &vault).expect("first");

        let text = vault.read("Old Name.md");
        std::fs::remove_file(vault.path().join("Old Name.md")).expect("rm");
        vault.write("Kestrel.md", &text);
        plain(&mut store, &vault).expect("rename");
        let view = store.view().expect("view");
        let title = view
            .note(view.find("Kestrel.md").expect("renamed"))
            .title
            .clone();
        eprintln!("title after rename: {title:?}");
        eprintln!("ask 'kestrel': {:?}", ask(&view, "kestrel"));
        eprintln!("ask 'old name': {:?}", ask(&view, "old name"));

        // The same notes indexed from scratch.
        let fresh_dir = vault.path().join(".fresh");
        let mut fresh = Store::open(&fresh_dir);
        plain(&mut fresh, &vault).expect("fresh");
        let fview = fresh.view().expect("view");
        eprintln!(
            "from scratch: {:?}",
            fview.note(fview.find("Kestrel.md").unwrap()).title
        );
        eprintln!("from scratch, ask 'kestrel': {:?}", ask(&fview, "kestrel"));
        assert_eq!(title, "Kestrel");
    }

    #[test]
    fn review_a_damaged_passage_table_panics_a_question() {
        let vault = TempVault::new("review-damage");
        vault.write("A.md", "# A\n\nalpha words about hashing\n");
        vault.write("B.md", "# B\n\nbeta words about deploys\n");
        let dir = vault.path().join(".index");
        let mut store = Store::open(&dir);
        plain(&mut store, &vault).expect("first");
        let name = store.manifest.segments[0].clone();
        let path = dir.join(&name);
        let mut bytes = std::fs::read(&path).expect("read");
        let table = u64::from_le_bytes(bytes[44..52].try_into().unwrap()) as usize;
        // One flipped byte: the first passage's note number.
        bytes[table + 3] = 0x40;
        std::fs::write(&path, &bytes).expect("write");

        let mut store = Store::open(&dir);
        let refreshed = plain(&mut store, &vault);
        eprintln!("refresh over the damage: {refreshed:?}");
        let view = store.take_view().expect("the segment opens");
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ask(&view, "hashing")));
        eprintln!("question: {:?}", outcome.as_ref().map_err(|_| "panicked"));
        assert!(
            outcome.is_ok(),
            "a damaged segment panics instead of being rebuilt"
        );
    }

    #[test]
    fn review_an_edit_in_the_same_second_that_keeps_the_size_is_missed() {
        let vault = TempVault::new("review-racy");
        let dir = vault.path().join(".index");
        let mut store = Store::open(&dir);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        std::thread::sleep(
            Duration::from_nanos(u64::from(1_000_000_000 - now.subsec_nanos()))
                + Duration::from_millis(5),
        );
        let path = vault.write("Task.md", "# Task\n\nstatus: pending\n");
        let first = std::fs::metadata(&path).unwrap().modified().unwrap();
        plain(&mut store, &vault).expect("first");
        vault.write("Task.md", "# Task\n\nstatus: shipped\n");
        let second = std::fs::metadata(&path).unwrap().modified().unwrap();
        eprintln!("mtimes {:?} -> {:?}", first, second);
        std::thread::sleep(Duration::from_millis(1500));
        let report = plain(&mut store, &vault).expect("second");
        eprintln!("second refresh: {report:?}");
        let view = store.view().unwrap();
        eprintln!(
            "ask shipped: {:?}; ask pending: {:?}",
            ask(&view, "shipped"),
            ask(&view, "pending")
        );
        assert!(
            !ask(&view, "shipped").is_empty(),
            "the index still holds the first version"
        );
    }

    #[test]
    fn review_a_damaged_document_frequency_allocates_what_it_says() {
        let vault = TempVault::new("review-df");
        vault.write("A.md", "# A\n\nalpha words about hashing\n");
        let dir = vault.path().join(".index");
        let mut store = Store::open(&dir);
        plain(&mut store, &vault).expect("first");
        let path = dir.join(&store.manifest.segments[0]);
        let mut bytes = std::fs::read(&path).expect("read");
        // The vocabulary's offset, the seventh of the header's.
        let vocabulary = u64::from_le_bytes(bytes[84..92].try_into().unwrap()) as usize;
        let count =
            u32::from_le_bytes(bytes[vocabulary..vocabulary + 4].try_into().unwrap()) as usize;
        for i in 0..count {
            let at = vocabulary + 4 + i * 24 + 8;
            bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        std::fs::write(&path, &bytes).expect("write");
        let view = Store::open(&dir).view().expect("opens");
        let t = std::time::Instant::now();
        let hits = ask(&view, "hashing");
        eprintln!("asked in {:?}: {hits:?}", t.elapsed());
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "nothing the size of the count was set aside"
        );
    }
}
