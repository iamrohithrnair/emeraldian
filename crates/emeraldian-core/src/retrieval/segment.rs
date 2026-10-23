//! One file of the retrieval index: some notes, their passages, the
//! passages' vectors, and an inverted index of the words in them.
//!
//! A segment is written once and never changed; an update writes a new one.
//! That is what lets a question be answered straight from the file without
//! loading it: the catalog is read, the vectors are streamed past the
//! question a block at a time, and a word's postings are found by a binary
//! search on disk and read on their own. Memory stays flat however large the
//! vault.
//!
//! ```text
//! header      magic, version, sizes, and where each section starts
//! model       the model, size and prompt the vectors were made with
//! catalog     each note: path, title, times written and changed, file
//!             number, size, hash, time read, aliases, links, tags
//! passages    each passage: its note, first and last line, length in terms
//! scales      each passage: its vector's scale, and the length of its prefix
//! prefixes    each passage: the vector's first P components, as bytes
//! rests       each passage: the other D - P components, as bytes
//! hashes      each passage: a hash of the text its vector was made from
//! vocabulary  each term, sorted, with where its postings are
//! postings    each term: the passages it occurs in, and how often
//! forward     each passage: its terms, so a segment can be merged into another
//! ```
//!
//! Vectors are kept in two blocks so the first pass of a search — over every
//! passage, with the first P components — reads a sixth of the bytes, and the
//! rest are read only for the few passages that pass. EmbeddingGemma is
//! trained so that a vector's first components carry the most meaning, which
//! is what makes the prefix a fair first judge.
//!
//! Segments are merged by [`merge`], which streams each section of the old
//! files into the new one rather than reading notes back into memory, so a
//! merge costs a few numbers per note, passage and term, whatever the size of
//! the postings and the vectors.
//!
//! A passage's hash is of the exact text its vector was made from, title and
//! prompt included, so when a note changes, the passages whose text didn't
//! keep their vectors and only the rest go to the model again.
//!
//! What is read is checked before it is used: a note's passages must be its
//! own, a posting's passage must exist, and nothing is set aside for a count
//! larger than the bytes there are to hold it. A damaged segment is refused
//! with [`io::ErrorKind::InvalidData`], to be rebuilt, never read wrongly.

use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BinaryHeap};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::note::Stamp;

const MAGIC: &[u8; 8] = b"EMRLSEG1";
/// Changed with the layout, and with how notes are cut into passages and
/// words: a segment made another way doesn't open, so its notes are read
/// again. Nothing else can turn such a segment away, since a lost manifest
/// is made again from whatever segments open. 2 added each passage's hash,
/// and the time each note was read; 3 cut passages at sentences under their
/// headings, stemmed their words, and left `#` lines in code blocks alone.
const VERSION: u32 = 3;
/// The number of sections, counting the end of the file as one.
const SECTIONS: usize = 10;
/// Magic, version, dims, prefix, notes, passages, total length, then the
/// sections' offsets, the last being the end of the file.
const HEADER_BYTES: u64 = 8 + 4 * 5 + 8 + 8 * SECTIONS as u64;
const VOCAB_ENTRY_BYTES: u64 = 24;
/// The numbers of a catalog entry between its title and its flag: the times
/// written and changed, the file number, the size, the hash and the time
/// read.
const ENTRY_NUMBERS: u64 = 8 * 6;
/// The least a catalog entry can take: two empty strings, its numbers, a
/// flag, where its passages start and how many, and three empty lists.
const MIN_ENTRY_BYTES: u64 = 4 * 2 + ENTRY_NUMBERS + 1 + 4 * 2 + 4 * 3;
/// Passages read per step of a scan: enough to keep reads large, few enough
/// that a scan's buffer stays small.
const SCAN_BLOCK: usize = 4_096;
/// What a merge's read buffers come to, shared among its sources: each
/// source reads up to three sections at once.
const MERGE_BUFFERS: usize = 2 << 20;
/// A passage number no merged passage or term has: the mark of one left out.
const GONE: u32 = u32::MAX;

/// A vector as kept: scaled to bytes.
///
/// Each vector has its own scale, its largest component mapping to 127, which
/// keeps nearly all the precision of the float a vector started as. A scale
/// of 0 marks a passage with no vector yet.
#[derive(Debug, Clone, PartialEq)]
pub struct Quantized {
    pub scale: f32,
    pub values: Vec<i8>,
}

impl Quantized {
    #[must_use]
    pub fn from_unit(vector: &[f32]) -> Self {
        let largest = vector.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        if largest == 0.0 {
            return Self {
                scale: 0.0,
                values: vec![0; vector.len()],
            };
        }
        let scale = largest / 127.0;
        Self {
            scale,
            values: vector
                .iter()
                .map(|&x| (x / scale).round().clamp(-127.0, 127.0) as i8)
                .collect(),
        }
    }

    /// The dot product with a float vector.
    #[must_use]
    pub fn dot(&self, other: &[f32]) -> f32 {
        dot(&self.values, other) * self.scale
    }

    #[must_use]
    pub fn floats(&self) -> Vec<f32> {
        self.values
            .iter()
            .map(|&v| f32::from(v) * self.scale)
            .collect()
    }
}

fn dot(values: &[i8], other: &[f32]) -> f32 {
    values
        .iter()
        .zip(other)
        .map(|(&v, &o)| f32::from(v) * o)
        .sum()
}

/// A passage, ready to be written.
#[derive(Debug, Clone, PartialEq)]
pub struct PassageRecord {
    pub start: u32,
    pub end: u32,
    /// How many terms it has: BM25's measure of its length.
    pub length: u32,
    /// Its distinct terms and how often each occurs.
    pub terms: Vec<(String, u32)>,
    pub vector: Option<Quantized>,
    /// A hash of the text the vector was made from, so a passage whose text
    /// comes round again can have it back without the model. Kept only with
    /// the vector; 0 without one.
    pub hash: u64,
}

/// A note, ready to be written.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteRecord {
    pub rel: String,
    pub title: String,
    /// The file's times and number, as the scan that found it gave them.
    pub stamp: Stamp,
    pub size: u64,
    pub hash: u64,
    /// When the refresh that read it began, in nanoseconds since the epoch:
    /// see [`NoteInfo::indexed`].
    pub indexed: u64,
    pub aliases: Vec<String>,
    /// Link targets as written, resolved when the graph is built: what a name
    /// means can change with every note added or removed.
    pub links: Vec<String>,
    pub tags: Vec<String>,
    pub passages: Vec<PassageRecord>,
}

impl NoteRecord {
    /// Whether every passage has its vector.
    #[must_use]
    pub fn embedded(&self) -> bool {
        self.passages.iter().all(|p| p.vector.is_some())
    }
}

/// A note as the catalog lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteInfo {
    pub rel: String,
    pub title: String,
    /// The file's times and number, as the scan that found it gave them.
    pub stamp: Stamp,
    pub size: u64,
    pub hash: u64,
    /// When the refresh that read it began, in nanoseconds since the epoch.
    /// A file written shortly before it may have been written again since,
    /// on a filesystem whose clock is coarse, without its time or size
    /// changing; such a note is read again to be sure.
    pub indexed: u64,
    /// Its aliases, links and tags, read out when asked for.
    lists: Lists,
    /// Its passages are `first..first + count` in the passage table.
    pub first: u32,
    pub count: u32,
    pub embedded: bool,
}

impl NoteInfo {
    #[must_use]
    pub fn aliases(&self) -> Vec<String> {
        self.lists.read(0)
    }

    /// Link targets as written: see [`NoteRecord::links`].
    #[must_use]
    pub fn links(&self) -> Vec<String> {
        self.lists.read(1)
    }

    #[must_use]
    pub fn tags(&self) -> Vec<String> {
        self.lists.read(2)
    }

    /// Each alias, as the catalog holds it: what resolving every link in a
    /// vault reads, without a string made for each.
    pub(crate) fn each_alias(&self, visit: impl FnMut(&str)) {
        self.lists.each(0, visit);
    }

    /// Each link target as written, as [`NoteInfo::each_alias`].
    pub(crate) fn each_link(&self, visit: impl FnMut(&str)) {
        self.lists.each(1, visit);
    }
}

/// A note's aliases, links and tags as its catalog entry holds them, in the
/// catalog read whole.
///
/// A question needs none of them now that the links are resolved when the
/// index is written, and they are most of what a catalog holds, so they are
/// left as bytes, checked, rather than made into strings for every note of
/// every view.
#[derive(Clone)]
struct Lists {
    catalog: Arc<[u8]>,
    at: usize,
    end: usize,
}

impl Lists {
    /// The list at `index`: 0 for aliases, 1 links, 2 tags.
    fn read(&self, index: usize) -> Vec<String> {
        let mut items = Vec::new();
        self.each(index, |item| items.push(item.to_string()));
        items
    }

    /// Each item of the list at `index`, borrowed from the catalog. The
    /// lists were checked to be whole text when the catalog was read.
    fn each(&self, index: usize, mut visit: impl FnMut(&str)) {
        let mut reader = Reader::new(&self.catalog[self.at..self.end]);
        for _ in 0..index {
            reader.skip_list();
        }
        let n = reader.u32().unwrap_or(0);
        for _ in 0..n {
            let Some(len) = reader.u32() else {
                return;
            };
            let Some(item) = reader
                .take(len as usize)
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
            else {
                return;
            };
            visit(item);
        }
    }

    fn bytes(&self) -> &[u8] {
        &self.catalog[self.at..self.end]
    }
}

impl PartialEq for Lists {
    fn eq(&self, other: &Self) -> bool {
        self.bytes() == other.bytes()
    }
}

impl Eq for Lists {}

impl std::fmt::Debug for Lists {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lists")
            .field("aliases", &self.read(0))
            .field("links", &self.read(1))
            .field("tags", &self.read(2))
            .finish()
    }
}

/// A passage as the passage table lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassageInfo {
    pub note: u32,
    pub start: u32,
    pub end: u32,
    pub length: u32,
}

/// What the header says.
#[derive(Debug, Clone, Copy)]
struct Header {
    dims: u32,
    prefix: u32,
    notes: u32,
    passages: u32,
    total_length: u64,
    catalog: u64,
    table: u64,
    scales: u64,
    prefixes: u64,
    rests: u64,
    hashes: u64,
    vocabulary: u64,
    postings: u64,
    forward: u64,
    end: u64,
}

/// What a segment's vectors are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape<'a> {
    /// The model, size and prompt they were made with, as the store records
    /// them: what lets an index whose manifest is lost be made again from
    /// its segments alone.
    pub model: &'a str,
    /// Components to a vector: 0 for none.
    pub dims: usize,
    /// How many of them the first pass of a search reads.
    pub prefix: usize,
}

/// Writes a segment: the notes in `notes`, which must be sorted by path, with
/// vectors of the given shape.
///
/// `durable` waits for the file to reach the disk itself before it takes
/// its name, for a segment costly to make again: see the [store]'s note on
/// what is synced.
///
/// [store]: super::store
pub fn write(
    path: &Path,
    notes: &[NoteRecord],
    shape: &Shape<'_>,
    durable: bool,
) -> io::Result<()> {
    let shape = Shape {
        prefix: shape.prefix.min(shape.dims),
        ..*shape
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let staging = path.with_extension(format!("part{}", std::process::id()));
    let result =
        write_to(&staging, notes, &shape, durable).and_then(|()| fs::rename(&staging, path));
    if result.is_err() {
        let _ = fs::remove_file(&staging);
    }
    result
}

fn write_to(path: &Path, notes: &[NoteRecord], shape: &Shape<'_>, durable: bool) -> io::Result<()> {
    let Shape { dims, prefix, .. } = *shape;
    let mut out = Counted::new(BufWriter::with_capacity(64 << 10, File::create(path)?));
    out.write_all(&[0u8; HEADER_BYTES as usize])?;
    put_str(&mut out, shape.model)?;

    let passages: Vec<(u32, &PassageRecord)> = notes
        .iter()
        .enumerate()
        .flat_map(|(n, note)| note.passages.iter().map(move |p| (n as u32, p)))
        .collect();

    // The catalog.
    let catalog = out.position;
    let mut first = 0u32;
    for note in notes {
        put_str(&mut out, &note.rel)?;
        put_str(&mut out, &note.title)?;
        for number in [note.stamp.written, note.stamp.changed, note.stamp.inode] {
            out.write_all(&number.to_le_bytes())?;
        }
        out.write_all(&note.size.to_le_bytes())?;
        out.write_all(&note.hash.to_le_bytes())?;
        out.write_all(&note.indexed.to_le_bytes())?;
        // Embedded only with vectors of this segment's size: a note written
        // without them, or with another size's, waits for its vectors.
        let embedded = note.passages.iter().all(|p| {
            p.vector
                .as_ref()
                .is_some_and(|q| q.values.len() == dims && dims > 0)
        });
        out.write_all(&[u8::from(embedded)])?;
        out.write_all(&first.to_le_bytes())?;
        out.write_all(&(note.passages.len() as u32).to_le_bytes())?;
        put_list(&mut out, &note.aliases)?;
        put_list(&mut out, &note.links)?;
        put_list(&mut out, &note.tags)?;
        first += note.passages.len() as u32;
    }

    // The passage table.
    let table = out.position;
    let mut total_length = 0u64;
    for (note, passage) in &passages {
        for value in [*note, passage.start, passage.end, passage.length] {
            out.write_all(&value.to_le_bytes())?;
        }
        total_length += u64::from(passage.length);
    }

    // Vectors: the scales, then every prefix, then every rest.
    let scales = out.position;
    for (_, passage) in &passages {
        let (scale, norm) = match &passage.vector {
            Some(q) if q.values.len() == dims => {
                let head: f32 = q.values[..prefix]
                    .iter()
                    .map(|&v| (f32::from(v) * q.scale).powi(2))
                    .sum();
                (q.scale, head.sqrt())
            }
            _ => (0.0, 0.0),
        };
        out.write_all(&scale.to_le_bytes())?;
        out.write_all(&norm.to_le_bytes())?;
    }
    let prefixes = out.position;
    for (_, passage) in &passages {
        out.write_all(&bytes_of(passage.vector.as_ref(), 0, prefix, dims))?;
    }
    let rests = out.position;
    for (_, passage) in &passages {
        out.write_all(&bytes_of(passage.vector.as_ref(), prefix, dims, dims))?;
    }
    // A hash goes with the vector it describes, and only with it.
    let hashes = out.position;
    for (_, passage) in &passages {
        let kept = passage
            .vector
            .as_ref()
            .is_some_and(|q| q.values.len() == dims && dims > 0);
        let hash = if kept { passage.hash } else { 0 };
        out.write_all(&hash.to_le_bytes())?;
    }

    // The inverted index: every term, sorted, and the passages it is in.
    let mut inverted: BTreeMap<&str, Vec<(u32, u32)>> = BTreeMap::new();
    for (id, (_, passage)) in passages.iter().enumerate() {
        for (term, tf) in &passage.terms {
            inverted.entry(term).or_default().push((id as u32, *tf));
        }
    }
    let mut postings_blob = Vec::new();
    let mut entries: Vec<(u32, u32, u32, u64, u32)> = Vec::with_capacity(inverted.len());
    let mut terms_blob = Vec::new();
    for (term, list) in &inverted {
        let at = postings_blob.len() as u64;
        let mut previous = 0u32;
        for &(id, tf) in list {
            put_varint(&mut postings_blob, u64::from(id - previous));
            put_varint(&mut postings_blob, u64::from(tf));
            previous = id;
        }
        entries.push((
            terms_blob.len() as u32,
            term.len() as u32,
            list.len() as u32,
            at,
            (postings_blob.len() as u64 - at) as u32,
        ));
        terms_blob.extend_from_slice(term.as_bytes());
    }
    let vocabulary = out.position;
    out.write_all(&(entries.len() as u32).to_le_bytes())?;
    for (offset, len, df, at, bytes) in &entries {
        out.write_all(&offset.to_le_bytes())?;
        out.write_all(&len.to_le_bytes())?;
        out.write_all(&df.to_le_bytes())?;
        out.write_all(&at.to_le_bytes())?;
        out.write_all(&bytes.to_le_bytes())?;
    }
    out.write_all(&(terms_blob.len() as u32).to_le_bytes())?;
    out.write_all(&terms_blob)?;
    let postings = out.position;
    out.write_all(&postings_blob)?;

    // Each passage's terms, by their place in the vocabulary.
    let forward = out.position;
    let place: BTreeMap<&str, u32> = inverted
        .keys()
        .enumerate()
        .map(|(i, term)| (*term, i as u32))
        .collect();
    let mut blob = Vec::new();
    for (_, passage) in &passages {
        put_varint(&mut blob, passage.terms.len() as u64);
        for (term, tf) in &passage.terms {
            put_varint(&mut blob, u64::from(place[term.as_str()]));
            put_varint(&mut blob, u64::from(*tf));
        }
    }
    out.write_all(&blob)?;
    let end = out.position;

    let file = out.into_inner()?;
    write_at(
        &file,
        0,
        &header_bytes(
            dims,
            prefix,
            notes.len(),
            passages.len(),
            total_length,
            [
                catalog, table, scales, prefixes, rests, hashes, vocabulary, postings, forward, end,
            ],
        ),
    )?;
    if durable {
        settle(&file)?;
    }
    Ok(())
}

/// Components `from..to` of a vector as bytes, zeros where there is none.
fn bytes_of(vector: Option<&Quantized>, from: usize, to: usize, dims: usize) -> Vec<u8> {
    match vector {
        Some(q) if q.values.len() == dims => q.values[from..to].iter().map(|&v| v as u8).collect(),
        _ => vec![0; to - from],
    }
}

/// A writer that knows how far it has written.
struct Counted<W: Write> {
    inner: W,
    position: u64,
}

impl Counted<BufWriter<File>> {
    fn new(inner: BufWriter<File>) -> Self {
        Self { inner, position: 0 }
    }

    fn into_inner(self) -> io::Result<File> {
        self.inner
            .into_inner()
            .map_err(io::IntoInnerError::into_error)
    }
}

impl<W: Write> Write for Counted<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(bytes)?;
        self.position += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn put_str(out: &mut impl Write, text: &str) -> io::Result<()> {
    out.write_all(&(text.len() as u32).to_le_bytes())?;
    out.write_all(text.as_bytes())
}

fn put_list(out: &mut impl Write, items: &[String]) -> io::Result<()> {
    out.write_all(&(items.len() as u32).to_le_bytes())?;
    for item in items {
        put_str(out, item)?;
    }
    Ok(())
}

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn damaged() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "a damaged index segment")
}

/// Fills `out` from `at` in the file, without moving a position the file's
/// other readers share: one call where a seek and a read would be two, and
/// safe for any number of cursors, and threads, reading one file.
#[cfg(unix)]
fn read_at(file: &File, at: u64, out: &mut [u8]) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(out, at)
}

#[cfg(windows)]
fn read_at(file: &File, mut at: u64, mut out: &mut [u8]) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !out.is_empty() {
        match file.seek_read(out, at) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => {
                out = &mut out[n..];
                at += n as u64;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn read_at(file: &File, at: u64, out: &mut [u8]) -> io::Result<()> {
    let mut file = file;
    file.seek(SeekFrom::Start(at))?;
    file.read_exact(out)
}

/// Writes `bytes` at `at` in the file, as [`read_at`] reads.
#[cfg(unix)]
fn write_at(file: &File, at: u64, bytes: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(bytes, at)
}

#[cfg(windows)]
fn write_at(file: &File, mut at: u64, mut bytes: &[u8]) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !bytes.is_empty() {
        match file.seek_write(bytes, at) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => {
                bytes = &bytes[n..];
                at += n as u64;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn write_at(file: &File, at: u64, bytes: &[u8]) -> io::Result<()> {
    let mut file = file;
    file.seek(SeekFrom::Start(at))?;
    file.write_all(bytes)
}

/// One segment's part in a merge: the segment, and which of its notes, by
/// their place in its catalog, go on into the merged one.
pub struct Source<'a> {
    pub segment: &'a Segment,
    pub keep: &'a [bool],
}

/// Writes one segment holding the notes `sources` keep, with vectors of
/// `dims` components of which the first `prefix` are scanned first: the
/// segment [`write`] would make of the same notes' records, byte for byte,
/// made without reading them into memory.
///
/// Every section is streamed from the sources into the new file in turn, so
/// what is held is a few numbers per note, passage and term — never the
/// postings, the vectors or the text. The notes come out sorted by path and
/// their passages are numbered again in that order; the vocabularies are
/// merged, each term keeping only the postings of passages that go on.
///
/// A path kept by two sources is taken from the later one, the newer copy.
/// A source whose vectors have another size, or another prefix, brings none,
/// as no source does when `dims` is 0. `durable` is as for [`write`].
pub fn merge(
    path: &Path,
    sources: &[Source<'_>],
    shape: &Shape<'_>,
    durable: bool,
) -> io::Result<()> {
    let shape = Shape {
        prefix: shape.prefix.min(shape.dims),
        ..*shape
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let staging = path.with_extension(format!("part{}", std::process::id()));
    let result = (|| {
        let scratch = [
            Scratch::new(path, "terms")?,
            Scratch::new(path, "postings")?,
        ];
        merge_to(&staging, sources, &shape, &scratch, durable)?;
        fs::rename(&staging, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&staging);
    }
    result
}

/// A file a merge writes a section to while what goes before it in the
/// segment is still being worked out. Removed when dropped, whether the merge
/// worked or not.
struct Scratch {
    path: PathBuf,
    file: File,
}

impl Scratch {
    fn new(beside: &Path, what: &str) -> io::Result<Self> {
        let path = beside.with_extension(format!("{what}{}", std::process::id()));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        Ok(Self { path, file })
    }

    /// Appends the first `len` bytes written to it to `out`.
    fn copy_to(&self, len: u64, out: &mut impl Write) -> io::Result<()> {
        let mut file = &self.file;
        file.seek(SeekFrom::Start(0))?;
        let copied = io::copy(&mut file.take(len), out)?;
        if copied == len {
            Ok(())
        } else {
            Err(damaged())
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// A note a source brings to a merge.
#[derive(Debug, Clone, Copy)]
struct Kept {
    /// Its place in the source's catalog.
    index: u32,
    /// Its passages, `first..first + count` in the source.
    first: u32,
    count: u32,
    /// Whether every passage brings a vector, which is what the merged
    /// catalog says of it.
    whole: bool,
}

/// What a merge takes from where.
struct Plan {
    /// For each note of the merged segment, in path order, its source.
    order: Vec<u32>,
    /// What each source brings, in its own catalog's order: path order too.
    kept: Vec<Vec<Kept>>,
    /// Whether each source's vectors are copied, or left out.
    vectors: Vec<bool>,
}

/// Works out a merge's order from the sources' catalogs, read side by side
/// as sorted lists are merged, and which notes' vectors are whole.
fn plan(sources: &[Source<'_>], dims: usize, prefix: usize, size: usize) -> io::Result<Plan> {
    let vectors: Vec<bool> = sources
        .iter()
        .map(|s| dims > 0 && s.segment.dims() == dims && s.segment.prefix() == prefix)
        .collect();
    let mut catalogs: Vec<Catalog<'_>> = sources
        .iter()
        .map(|s| Catalog {
            cursor: s
                .segment
                .cursor(s.segment.header.catalog, s.segment.header.table, size),
            read: 0,
            notes: s.segment.header.notes,
            keep: s.keep,
        })
        .collect();
    let mut heads = Vec::with_capacity(sources.len());
    for catalog in &mut catalogs {
        heads.push(catalog.next_kept()?);
    }
    let mut order = Vec::new();
    let mut kept: Vec<Vec<Kept>> = vec![Vec::new(); sources.len()];
    loop {
        // The least path; of equal ones, the later source's.
        let mut best: Option<usize> = None;
        for (s, head) in heads.iter().enumerate() {
            let Some((rel, _)) = head else {
                continue;
            };
            let least = best
                .and_then(|b| heads[b].as_ref())
                .is_none_or(|(other, _)| rel <= other);
            if least {
                best = Some(s);
            }
        }
        let Some(best) = best else {
            break;
        };
        let (rel, note) = heads[best].take().expect("the least head");
        for s in 0..best {
            if heads[s].as_ref().is_some_and(|(other, _)| *other == rel) {
                heads[s] = catalogs[s].next_kept()?;
            }
        }
        order.push(best as u32);
        kept[best].push(note);
        heads[best] = catalogs[best].next_kept()?;
    }
    drop(catalogs);

    // A note's vectors are whole when every passage has one to bring; with
    // no passages, none are owed.
    for (s, source) in sources.iter().enumerate() {
        let header = &source.segment.header;
        let mut cursor = source.segment.cursor(header.scales, header.prefixes, size);
        for note in &mut kept[s] {
            note.whole = note.count == 0 || vectors[s];
            if note.count == 0 || !vectors[s] {
                continue;
            }
            cursor.seek(header.scales + u64::from(note.first) * 8)?;
            let mut row = [0u8; 8];
            for _ in 0..note.count {
                cursor.read(&mut row)?;
                if f32::from_le_bytes(row[..4].try_into().expect("4")) == 0.0 {
                    note.whole = false;
                }
            }
        }
    }
    Ok(Plan {
        order,
        kept,
        vectors,
    })
}

/// A source's catalog, read entry by entry while a merge is planned.
struct Catalog<'a> {
    cursor: Cursor<'a>,
    /// Entries read so far, of how many.
    read: u32,
    notes: u32,
    keep: &'a [bool],
}

impl Catalog<'_> {
    /// The next note the source keeps, with its path.
    fn next_kept(&mut self) -> io::Result<Option<(Vec<u8>, Kept)>> {
        while self.read < self.notes {
            let index = self.read;
            self.read += 1;
            let cursor = &mut self.cursor;
            if !self.keep.get(index as usize).copied().unwrap_or(false) {
                skip_entry(cursor)?;
                continue;
            }
            let len = cursor.u32()?;
            let rel = cursor.bytes(u64::from(len))?;
            let title = cursor.u32()?;
            // The title, the numbers, and whether it is embedded.
            cursor.skip(u64::from(title) + ENTRY_NUMBERS + 1)?;
            let first = cursor.u32()?;
            let count = cursor.u32()?;
            for _ in 0..3 {
                skip_list(cursor)?;
            }
            return Ok(Some((
                rel,
                Kept {
                    index,
                    first,
                    count,
                    whole: false,
                },
            )));
        }
        Ok(None)
    }
}

fn skip_list(cursor: &mut Cursor<'_>) -> io::Result<()> {
    let n = cursor.u32()?;
    for _ in 0..n {
        let len = cursor.u32()?;
        cursor.skip(u64::from(len))?;
    }
    Ok(())
}

fn skip_entry(cursor: &mut Cursor<'_>) -> io::Result<()> {
    for _ in 0..2 {
        let len = cursor.u32()?;
        cursor.skip(u64::from(len))?;
    }
    cursor.skip(ENTRY_NUMBERS + 1 + 4 * 2)?;
    for _ in 0..3 {
        skip_list(cursor)?;
    }
    Ok(())
}

/// Copies one catalog entry, saying its passages start at `first` and
/// whether they are all `embedded`.
fn copy_entry(
    cursor: &mut Cursor<'_>,
    out: &mut impl Write,
    embedded: bool,
    first: u32,
) -> io::Result<()> {
    for _ in 0..2 {
        let len = cursor.u32()?;
        out.write_all(&len.to_le_bytes())?;
        cursor.copy(u64::from(len), out)?;
    }
    cursor.copy(ENTRY_NUMBERS, out)?;
    cursor.skip(1 + 4)?;
    out.write_all(&[u8::from(embedded)])?;
    out.write_all(&first.to_le_bytes())?;
    cursor.copy(4, out)?;
    for _ in 0..3 {
        let n = cursor.u32()?;
        out.write_all(&n.to_le_bytes())?;
        for _ in 0..n {
            let len = cursor.u32()?;
            out.write_all(&len.to_le_bytes())?;
            cursor.copy(u64::from(len), out)?;
        }
    }
    Ok(())
}

/// Writes `n` zero bytes: the vectors of passages that bring none.
fn zeros(out: &mut impl Write, mut n: u64) -> io::Result<()> {
    const NOTHING: [u8; 4_096] = [0; 4_096];
    while n > 0 {
        let take = n.min(NOTHING.len() as u64) as usize;
        out.write_all(&NOTHING[..take])?;
        n -= take as u64;
    }
    Ok(())
}

/// Writes the merged segment to `path`, the terms and the postings going to
/// `scratch` until the vocabulary is done.
fn merge_to(
    path: &Path,
    sources: &[Source<'_>],
    shape: &Shape<'_>,
    scratch: &[Scratch; 2],
    durable: bool,
) -> io::Result<()> {
    let Shape { dims, prefix, .. } = *shape;
    let [terms_scratch, postings_scratch] = scratch;
    let size = (MERGE_BUFFERS / (3 * sources.len().max(1))).clamp(4 << 10, 64 << 10);
    let plan = &plan(sources, dims, prefix, size)?;
    let mut out = Counted::new(BufWriter::with_capacity(64 << 10, File::create(path)?));
    out.write_all(&[0u8; HEADER_BYTES as usize])?;
    put_str(&mut out, shape.model)?;
    // One cursor per source, on the same section of each.
    let cursors = |from: fn(&Header) -> u64, to: fn(&Header) -> u64| {
        sources
            .iter()
            .map(|s| {
                let header = &s.segment.header;
                s.segment.cursor(from(header), to(header), size)
            })
            .collect::<Vec<_>>()
    };
    // Each note of the merged segment in turn: its source, and what it brings.
    let notes = || {
        let mut next = vec![0usize; sources.len()];
        plan.order.iter().map(move |&s| {
            let s = s as usize;
            next[s] += 1;
            (s, plan.kept[s][next[s] - 1])
        })
    };

    // The catalog: each entry copied, with where its passages now start.
    let catalog = out.position;
    let mut readers = cursors(|h| h.catalog, |h| h.table);
    let mut read = vec![0u32; sources.len()];
    let mut first = 0u32;
    for (s, note) in notes() {
        while read[s] < note.index {
            skip_entry(&mut readers[s])?;
            read[s] += 1;
        }
        copy_entry(&mut readers[s], &mut out, note.whole, first)?;
        read[s] += 1;
        first = first.checked_add(note.count).ok_or_else(damaged)?;
    }
    drop(readers);

    // The passage table, numbering each kept passage again.
    let table = out.position;
    let mut renumber: Vec<Vec<u32>> = sources
        .iter()
        .map(|s| vec![GONE; s.segment.passage_count()])
        .collect();
    let mut readers = cursors(|h| h.table, |h| h.scales);
    let mut total_length = 0u64;
    let mut id = 0u32;
    for (n, (s, note)) in notes().enumerate() {
        let header = &sources[s].segment.header;
        if u64::from(note.first) + u64::from(note.count) > u64::from(header.passages) {
            return Err(damaged());
        }
        readers[s].seek(header.table + u64::from(note.first) * 16)?;
        let mut row = [0u8; 16];
        for local in note.first..note.first + note.count {
            readers[s].read(&mut row)?;
            out.write_all(&(n as u32).to_le_bytes())?;
            out.write_all(&row[4..])?;
            total_length += u64::from(u32::from_le_bytes(row[12..].try_into().expect("4")));
            renumber[s][local as usize] = id;
            id += 1;
        }
    }
    let passages = id;
    drop(readers);

    // Vectors: the scales, then every prefix, then every rest, then the
    // hashes of what they were made from, each copied from a source that
    // brings them and left empty for one that doesn't.
    let mut offsets = [0u64; 4];
    let widths = [8, prefix as u64, (dims - prefix) as u64, 8];
    let sections: [fn(&Header) -> u64; 5] = [
        |h| h.scales,
        |h| h.prefixes,
        |h| h.rests,
        |h| h.hashes,
        |h| h.vocabulary,
    ];
    for k in 0..4 {
        offsets[k] = out.position;
        let mut readers = cursors(sections[k], sections[k + 1]);
        for (s, note) in notes() {
            let width = if plan.vectors[s] { widths[k] } else { 0 };
            let bytes = u64::from(note.count) * widths[k];
            if width == 0 {
                zeros(&mut out, bytes)?;
                continue;
            }
            let start = sections[k](&sources[s].segment.header);
            readers[s].seek(start + u64::from(note.first) * width)?;
            readers[s].copy(bytes, &mut out)?;
        }
    }
    let [scales, prefixes, rests, hashes] = offsets;

    // The vocabularies, merged in order; each term's postings merged from
    // its sources' and renumbered, its place in each source's noted for the
    // forward lists. Entries go straight out, the terms and postings to
    // scratch files, as they must follow every entry.
    let vocabulary = out.position;
    out.write_all(&0u32.to_le_bytes())?;
    let mut vocabularies = sources
        .iter()
        .map(|s| Vocabulary::open(s.segment, size))
        .collect::<io::Result<Vec<_>>>()?;
    let mut places: Vec<Vec<u32>> = vocabularies
        .iter()
        .map(|v| vec![GONE; v.count as usize])
        .collect();
    let mut terms_out = BufWriter::with_capacity(64 << 10, &terms_scratch.file);
    let mut postings_out = BufWriter::with_capacity(64 << 10, &postings_scratch.file);
    let (mut terms_len, mut postings_len) = (0u64, 0u64);
    let mut place = 0u32;
    let mut with: Vec<usize> = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    let mut encoded = Vec::with_capacity(20);
    loop {
        with.clear();
        for (s, vocabulary) in vocabularies.iter().enumerate() {
            let Some(term) = &vocabulary.head else {
                continue;
            };
            match with.first() {
                Some(&w) => {
                    let least = &vocabularies[w].head.as_ref().expect("a head").text;
                    match term.text.cmp(least) {
                        Ordering::Less => {
                            with.clear();
                            with.push(s);
                        }
                        Ordering::Equal => with.push(s),
                        Ordering::Greater => {}
                    }
                }
                None => with.push(s),
            }
        }
        let Some(&w) = with.first() else {
            break;
        };

        let start = postings_len;
        runs.clear();
        for &s in &with {
            let vocabulary = &mut vocabularies[s];
            let term = vocabulary.head.as_ref().expect("a head");
            let from = sources[s].segment.header.postings + term.at;
            let end = from + u64::from(term.bytes);
            vocabulary.postings.seek(from)?;
            let mut run = Run {
                source: s,
                end,
                old: 0,
                head: None,
            };
            run.advance(&mut vocabulary.postings, &renumber[s])?;
            runs.push(run);
        }
        let (mut previous, mut df) = (0u32, 0u32);
        loop {
            let mut least: Option<usize> = None;
            for (r, run) in runs.iter().enumerate() {
                if let Some((id, _)) = run.head
                    && least.is_none_or(|l| runs[l].head.is_some_and(|(other, _)| id < other))
                {
                    least = Some(r);
                }
            }
            let Some(r) = least else {
                break;
            };
            let (id, tf) = runs[r].head.expect("the least run");
            if df > 0 && id <= previous {
                return Err(damaged());
            }
            encoded.clear();
            put_varint(&mut encoded, u64::from(id - previous));
            put_varint(&mut encoded, u64::from(tf));
            postings_out.write_all(&encoded)?;
            postings_len += encoded.len() as u64;
            previous = id;
            df += 1;
            let source = runs[r].source;
            runs[r].advance(&mut vocabularies[source].postings, &renumber[source])?;
        }
        if df > 0 {
            let text = &vocabularies[w].head.as_ref().expect("a head").text;
            out.write_all(&(terms_len as u32).to_le_bytes())?;
            out.write_all(&(text.len() as u32).to_le_bytes())?;
            out.write_all(&df.to_le_bytes())?;
            out.write_all(&start.to_le_bytes())?;
            out.write_all(&((postings_len - start) as u32).to_le_bytes())?;
            terms_out.write_all(text)?;
            terms_len += text.len() as u64;
            for &s in &with {
                let old = vocabularies[s].head.as_ref().expect("a head").place;
                places[s][old as usize] = place;
            }
            place += 1;
        }
        for &s in &with {
            vocabularies[s].advance()?;
        }
    }
    drop(vocabularies);
    terms_out.flush()?;
    drop(terms_out);
    postings_out.flush()?;
    drop(postings_out);
    out.write_all(&(terms_len as u32).to_le_bytes())?;
    terms_scratch.copy_to(terms_len, &mut out)?;
    let postings = out.position;
    postings_scratch.copy_to(postings_len, &mut out)?;

    // Each passage's terms, by their places in the merged vocabulary.
    let forward = out.position;
    let mut readers = cursors(|h| h.forward, |h| h.end);
    let mut listed = vec![0u32; sources.len()];
    for (s, note) in notes() {
        for local in note.first..note.first + note.count {
            let reader = &mut readers[s];
            while listed[s] < local {
                let n = reader.varint()?;
                for _ in 0..n * 2 {
                    reader.varint()?;
                }
                listed[s] += 1;
            }
            let n = reader.varint()?;
            encoded.clear();
            put_varint(&mut encoded, n);
            for _ in 0..n {
                let old = reader.varint()?;
                let tf = reader.varint()?;
                let new = places[s]
                    .get(old as usize)
                    .copied()
                    .filter(|&p| p != GONE)
                    .ok_or_else(damaged)?;
                put_varint(&mut encoded, u64::from(new));
                put_varint(&mut encoded, tf);
            }
            out.write_all(&encoded)?;
            listed[s] += 1;
        }
    }
    drop(readers);
    let end = out.position;

    let file = out.into_inner()?;
    write_at(&file, vocabulary, &place.to_le_bytes())?;
    write_at(
        &file,
        0,
        &header_bytes(
            dims,
            prefix,
            plan.order.len(),
            passages as usize,
            total_length,
            [
                catalog, table, scales, prefixes, rests, hashes, vocabulary, postings, forward, end,
            ],
        ),
    )?;
    if durable {
        settle(&file)?;
    }
    Ok(())
}

/// Waits until what was written to `file` has reached the disk.
///
/// On Apple systems that is a plain `fsync`, not the `F_FULLFSYNC` the
/// standard library's `sync_all` asks for, which also empties the drive's
/// own cache and takes from a few milliseconds to a few hundred: the trade
/// SQLite and git make there by default. The index can be made again from
/// the notes; what this protects is the time that would take.
#[cfg(target_vendor = "apple")]
fn settle(file: &File) -> io::Result<()> {
    rustix::fs::fsync(file).map_err(io::Error::from)
}

#[cfg(not(target_vendor = "apple"))]
fn settle(file: &File) -> io::Result<()> {
    file.sync_data()
}

/// The header, as [`Segment::open`] reads it.
fn header_bytes(
    dims: usize,
    prefix: usize,
    notes: usize,
    passages: usize,
    total_length: u64,
    offsets: [u64; SECTIONS],
) -> Vec<u8> {
    let mut header = Vec::with_capacity(HEADER_BYTES as usize);
    header.extend_from_slice(MAGIC);
    for value in [
        VERSION,
        dims as u32,
        prefix as u32,
        notes as u32,
        passages as u32,
    ] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    header.extend_from_slice(&total_length.to_le_bytes());
    for offset in offsets {
        header.extend_from_slice(&offset.to_le_bytes());
    }
    header
}

/// One source's vocabulary, read in order during a merge, with the
/// postings of the term it is at.
struct Vocabulary<'a> {
    entries: Cursor<'a>,
    terms: Cursor<'a>,
    postings: Cursor<'a>,
    /// How many terms it has, and where their text starts.
    count: u32,
    text_at: u64,
    /// The term it is at; `None` once past the last.
    head: Option<Term>,
    read: u32,
}

/// A term of a source's vocabulary.
struct Term {
    text: Vec<u8>,
    place: u32,
    /// Where its postings are, from the start of the postings, and how long.
    at: u64,
    bytes: u32,
}

impl<'a> Vocabulary<'a> {
    fn open(segment: &'a Segment, size: usize) -> io::Result<Self> {
        let header = &segment.header;
        let (count, entries, text_at) = segment.vocabulary()?;
        let mut vocabulary = Self {
            entries: segment.cursor(entries, text_at - 4, size),
            terms: segment.cursor(text_at, header.postings, size),
            postings: segment.cursor(header.postings, header.forward, size),
            count,
            text_at,
            head: None,
            read: 0,
        };
        vocabulary.advance()?;
        Ok(vocabulary)
    }

    /// Moves to the next term.
    fn advance(&mut self) -> io::Result<()> {
        self.head = None;
        if self.read == self.count {
            return Ok(());
        }
        let offset = self.entries.u32()?;
        let len = self.entries.u32()?;
        let _df = self.entries.u32()?;
        let at = self.entries.u64()?;
        let bytes = self.entries.u32()?;
        self.terms.seek(self.text_at + u64::from(offset))?;
        let text = self.terms.bytes(u64::from(len))?;
        self.head = Some(Term {
            text,
            place: self.read,
            at,
            bytes,
        });
        self.read += 1;
        Ok(())
    }
}

/// One source's postings for a term, renumbered as they are read, during a
/// merge of postings lists.
struct Run {
    source: usize,
    /// Where its postings end.
    end: u64,
    /// The passage of the posting last read, as the source numbered it.
    old: u64,
    /// The next posting that goes on: its new passage number, and how often.
    head: Option<(u32, u32)>,
}

impl Run {
    /// Reads on to the next posting of a passage that goes on.
    fn advance(&mut self, cursor: &mut Cursor<'_>, renumber: &[u32]) -> io::Result<()> {
        self.head = None;
        while cursor.position() < self.end {
            self.old = self.old.checked_add(cursor.varint()?).ok_or_else(damaged)?;
            let tf = cursor.varint()?;
            let id = usize::try_from(self.old)
                .ok()
                .and_then(|old| renumber.get(old))
                .copied()
                .ok_or_else(damaged)?;
            if id != GONE {
                self.head = Some((id, tf as u32));
                return Ok(());
            }
        }
        Ok(())
    }
}

/// Reads part of a segment front to back, a buffer at a time: how a merge
/// reads its sources without holding them.
///
/// Every cursor reads from its own place, so any number of them can share a
/// file.
struct Cursor<'a> {
    file: &'a File,
    /// Where the buffer was read from.
    at: u64,
    /// The end of what this cursor may read.
    end: u64,
    buffer: Vec<u8>,
    filled: usize,
    used: usize,
}

impl<'a> Cursor<'a> {
    fn new(file: &'a File, from: u64, end: u64, size: usize) -> Self {
        Self {
            file,
            at: from,
            end,
            buffer: vec![0; size.max(16)],
            filled: 0,
            used: 0,
        }
    }

    fn position(&self) -> u64 {
        self.at + self.used as u64
    }

    /// Moves to `position`, keeping what is buffered if it is there.
    fn seek(&mut self, position: u64) -> io::Result<()> {
        if position > self.end {
            return Err(damaged());
        }
        if position >= self.at && position <= self.at + self.filled as u64 {
            self.used = (position - self.at) as usize;
        } else {
            self.at = position;
            self.filled = 0;
            self.used = 0;
        }
        Ok(())
    }

    fn skip(&mut self, n: u64) -> io::Result<()> {
        self.seek(self.position().checked_add(n).ok_or_else(damaged)?)
    }

    /// Reads on from where the cursor is; `false` at the end.
    fn fill(&mut self) -> io::Result<bool> {
        self.at += self.used as u64;
        self.filled = 0;
        self.used = 0;
        let want = self
            .end
            .saturating_sub(self.at)
            .min(self.buffer.len() as u64) as usize;
        if want == 0 {
            return Ok(false);
        }
        read_at(self.file, self.at, &mut self.buffer[..want])?;
        self.filled = want;
        Ok(true)
    }

    /// The next bytes, as many as `out` holds.
    fn read(&mut self, out: &mut [u8]) -> io::Result<()> {
        let mut done = 0;
        while done < out.len() {
            if self.used == self.filled && !self.fill()? {
                return Err(damaged());
            }
            let n = (self.filled - self.used).min(out.len() - done);
            out[done..done + n].copy_from_slice(&self.buffer[self.used..self.used + n]);
            self.used += n;
            done += n;
        }
        Ok(())
    }

    /// The next `n` bytes, checked against what is left before anything is
    /// set aside for them.
    fn bytes(&mut self, n: u64) -> io::Result<Vec<u8>> {
        if n > self.end.saturating_sub(self.position()) {
            return Err(damaged());
        }
        let mut out = vec![0; n as usize];
        self.read(&mut out)?;
        Ok(out)
    }

    fn u8(&mut self) -> io::Result<u8> {
        if self.used < self.filled {
            self.used += 1;
            return Ok(self.buffer[self.used - 1]);
        }
        let mut byte = [0u8; 1];
        self.read(&mut byte)?;
        Ok(byte[0])
    }

    fn u32(&mut self) -> io::Result<u32> {
        let mut bytes = [0u8; 4];
        self.read(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn u64(&mut self) -> io::Result<u64> {
        let mut bytes = [0u8; 8];
        self.read(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn varint(&mut self) -> io::Result<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(damaged())
    }

    /// Copies the next `n` bytes to `out`.
    fn copy(&mut self, mut n: u64, out: &mut impl Write) -> io::Result<()> {
        while n > 0 {
            if self.used == self.filled && !self.fill()? {
                return Err(damaged());
            }
            let take = ((self.filled - self.used) as u64).min(n) as usize;
            out.write_all(&self.buffer[self.used..self.used + take])?;
            self.used += take;
            n -= take as u64;
        }
        Ok(())
    }
}

/// A term's document frequency, and its passages with how often it occurs
/// in each.
pub type Postings = (u32, Vec<(u32, u32)>);

/// An open segment.
#[derive(Debug)]
pub struct Segment {
    file: File,
    header: Header,
    /// The model, size and prompt its vectors were made with.
    model: String,
}

/// A score with a total order, for heaps.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Ranked(f32, u32);

impl Eq for Ranked {}

impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Ranked {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .total_cmp(&other.0)
            .then_with(|| other.1.cmp(&self.1))
    }
}

impl Segment {
    /// Opens a segment, checking that it is whole: right magic and version,
    /// sections in order, and as long as it says it is. A file that fails is
    /// one to rebuild, not to read.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let length = file.metadata()?.len();
        let mut bytes = vec![0u8; HEADER_BYTES as usize];
        if length < HEADER_BYTES {
            return Err(damaged());
        }
        read_at(&file, 0, &mut bytes)?;
        if &bytes[..8] != MAGIC {
            return Err(damaged());
        }
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
        let u64_at = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"));
        if u32_at(8) != VERSION {
            return Err(damaged());
        }
        let offsets: [u64; SECTIONS] = std::array::from_fn(|i| u64_at(36 + 8 * i));
        let header = Header {
            dims: u32_at(12),
            prefix: u32_at(16),
            notes: u32_at(20),
            passages: u32_at(24),
            total_length: u64_at(28),
            catalog: offsets[0],
            table: offsets[1],
            scales: offsets[2],
            prefixes: offsets[3],
            rests: offsets[4],
            hashes: offsets[5],
            vocabulary: offsets[6],
            postings: offsets[7],
            forward: offsets[8],
            end: offsets[9],
        };
        let ordered = std::iter::once(HEADER_BYTES)
            .chain(offsets)
            .collect::<Vec<_>>()
            .windows(2)
            .all(|pair| pair[0] <= pair[1]);
        // Only once they are in order are the sections' sizes worked out.
        let m = u64::from(header.passages);
        let sized = || {
            header.prefix <= header.dims
                && header.scales - header.table == m * 16
                && header.prefixes - header.scales == m * 8
                && header.rests - header.prefixes == m * u64::from(header.prefix)
                && header.hashes - header.rests == m * u64::from(header.dims - header.prefix)
                && header.vocabulary - header.hashes == m * 8
                && u64::from(header.notes) * MIN_ENTRY_BYTES <= header.table - header.catalog
        };
        if !ordered || !sized() || header.end != length {
            return Err(damaged());
        }
        // The model, between the header and the catalog.
        let mut model = vec![0u8; (header.catalog - HEADER_BYTES) as usize];
        read_at(&file, HEADER_BYTES, &mut model)?;
        let mut reader = Reader::new(&model);
        let model = reader.str().filter(|_| reader.done()).ok_or_else(damaged)?;
        Ok(Self {
            file,
            header,
            model,
        })
    }

    /// The model, size and prompt the vectors were made with, as the store
    /// recorded them when this segment was written.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    #[must_use]
    pub fn dims(&self) -> usize {
        self.header.dims as usize
    }

    #[must_use]
    pub fn prefix(&self) -> usize {
        self.header.prefix as usize
    }

    #[must_use]
    pub fn passage_count(&self) -> usize {
        self.header.passages as usize
    }

    /// The sum of every passage's length, for BM25's average.
    #[must_use]
    pub fn total_length(&self) -> u64 {
        self.header.total_length
    }

    /// `len` bytes from `at`, which must lie within the file: a length read
    /// from a damaged file is refused rather than set aside.
    fn read(&self, at: u64, len: usize) -> io::Result<Vec<u8>> {
        if at
            .checked_add(len as u64)
            .is_none_or(|end| end > self.header.end)
        {
            return Err(damaged());
        }
        let mut bytes = vec![0u8; len];
        read_at(&self.file, at, &mut bytes)?;
        Ok(bytes)
    }

    /// The notes in the segment, in path order, checked: each note's passages
    /// follow the last one's, and together they are every passage there is.
    pub fn notes(&self) -> io::Result<Vec<NoteInfo>> {
        let catalog: Arc<[u8]> = self
            .read(
                self.header.catalog,
                (self.header.table - self.header.catalog) as usize,
            )?
            .into();
        let mut reader = Reader::new(&catalog);
        // Open checked that the catalog is long enough for this many.
        let mut notes = Vec::with_capacity(self.header.notes as usize);
        let mut next = 0u32;
        for _ in 0..self.header.notes {
            let note = (|| {
                let rel = reader.str()?;
                let title = reader.str()?;
                let stamp = Stamp {
                    written: reader.u64()?,
                    changed: reader.u64()?,
                    inode: reader.u64()?,
                };
                let size = reader.u64()?;
                let hash = reader.u64()?;
                let indexed = reader.u64()?;
                let embedded = reader.u8()? != 0;
                let first = reader.u32()?;
                let count = reader.u32()?;
                let at = reader.at;
                for _ in 0..3 {
                    reader.skip_list()?;
                }
                Some(NoteInfo {
                    rel,
                    title,
                    stamp,
                    size,
                    hash,
                    indexed,
                    lists: Lists {
                        catalog: Arc::clone(&catalog),
                        at,
                        end: reader.at,
                    },
                    first,
                    count,
                    embedded,
                })
            })()
            .filter(|note| note.first == next)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "a damaged catalog"))?;
            next = next
                .checked_add(note.count)
                .filter(|&n| n <= self.header.passages)
                .ok_or_else(damaged)?;
            notes.push(note);
        }
        let ordered = notes.windows(2).all(|pair| pair[0].rel < pair[1].rel);
        if next != self.header.passages || !ordered {
            return Err(damaged());
        }
        Ok(notes)
    }

    /// The passage table, checked: each passage belongs to a note there is,
    /// in order.
    pub fn passages(&self) -> io::Result<Vec<PassageInfo>> {
        let bytes = self.read(self.header.table, self.header.passages as usize * 16)?;
        let table: Vec<PassageInfo> = bytes
            .chunks_exact(16)
            .map(|row| {
                let at =
                    |i: usize| u32::from_le_bytes(row[i * 4..i * 4 + 4].try_into().expect("4"));
                PassageInfo {
                    note: at(0),
                    start: at(1),
                    end: at(2),
                    length: at(3),
                }
            })
            .collect();
        let ordered = table.windows(2).all(|pair| pair[0].note <= pair[1].note);
        if !ordered || table.last().is_some_and(|p| p.note >= self.header.notes) {
            return Err(damaged());
        }
        Ok(table)
    }

    /// The catalog and the passage table, each checked against the other:
    /// every passage is the note's whose run of passages it is in. What a
    /// question is answered from, so a damaged number never sends one to a
    /// note or a passage that isn't there.
    pub fn catalog(&self) -> io::Result<(Vec<NoteInfo>, Vec<PassageInfo>)> {
        let notes = self.notes()?;
        let passages = self.passages()?;
        for (n, note) in notes.iter().enumerate() {
            let run = note.first as usize..(note.first + note.count) as usize;
            if passages[run].iter().any(|p| p.note as usize != n) {
                return Err(damaged());
            }
        }
        Ok((notes, passages))
    }

    /// The hashes of what the vectors of passages `first..first + count` were
    /// made from, 0 for a passage without one.
    pub fn hashes(&self, first: u32, count: u32) -> io::Result<Vec<u64>> {
        if u64::from(first) + u64::from(count) > u64::from(self.header.passages) {
            return Err(damaged());
        }
        let bytes = self.read(
            self.header.hashes + u64::from(first) * 8,
            count as usize * 8,
        )?;
        Ok(bytes
            .chunks_exact(8)
            .map(|word| u64::from_le_bytes(word.try_into().expect("8")))
            .collect())
    }

    /// The `k` passages whose prefixes come closest to `query`, a unit
    /// vector of the prefix's length, among those `keep` accepts.
    ///
    /// The prefix block is streamed past the question a block of passages at
    /// a time, so the cost in memory is a block, whatever the segment's size.
    pub fn coarse(
        &self,
        query: &[f32],
        keep: &dyn Fn(u32) -> bool,
        k: usize,
    ) -> io::Result<Vec<(u32, f32)>> {
        let m = self.header.passages as usize;
        let p = self.header.prefix as usize;
        if p == 0 || m == 0 || k == 0 {
            return Ok(Vec::new());
        }
        // The question as bytes too, so each comparison is a sum of whole
        // numbers, which the processor adds many at a time; the scale comes
        // back at the end. The ranking it gives is the floats' within the
        // rounding of the question's components.
        let largest = query.iter().take(p).fold(0.0f32, |m, &x| m.max(x.abs()));
        if query.len() < p || largest == 0.0 {
            return Ok(Vec::new());
        }
        let step = largest / 127.0;
        let question: Vec<i8> = query[..p]
            .iter()
            .map(|&x| (x / step).round().clamp(-127.0, 127.0) as i8)
            .collect();
        let scales = self.read(self.header.scales, m * 8)?;
        let mut heap: BinaryHeap<Reverse<Ranked>> = BinaryHeap::with_capacity(k + 1);
        let mut block = vec![0u8; SCAN_BLOCK * p];
        let mut id = 0usize;
        while id < m {
            let n = SCAN_BLOCK.min(m - id);
            read_at(
                &self.file,
                self.header.prefixes + (id * p) as u64,
                &mut block[..n * p],
            )?;
            for (row, values) in block[..n * p].chunks_exact(p).enumerate() {
                let passage = (id + row) as u32;
                let at = (id + row) * 8;
                let scale = f32::from_le_bytes(scales[at..at + 4].try_into().expect("4"));
                let norm = f32::from_le_bytes(scales[at + 4..at + 8].try_into().expect("4"));
                if scale == 0.0 || norm == 0.0 || !keep(passage) {
                    continue;
                }
                let sum: i32 = values
                    .iter()
                    .zip(&question)
                    .map(|(&v, &q)| i32::from(v as i8) * i32::from(q))
                    .sum();
                let score = sum as f32 * step * scale / norm;
                // A full heap keeps only what beats its least; a tie goes to
                // the passage already in it, the earlier one.
                if heap.len() == k && heap.peek().is_some_and(|Reverse(least)| score <= least.0) {
                    continue;
                }
                heap.push(Reverse(Ranked(score, passage)));
                if heap.len() > k {
                    heap.pop();
                }
            }
            id += n;
        }
        let mut out: Vec<(u32, f32)> = heap
            .into_iter()
            .map(|Reverse(Ranked(s, id))| (id, s))
            .collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(out)
    }

    /// Full vectors of some passages: the few a search shortlisted, for an
    /// exact score, or a copied note's. A run of consecutive passages, as a
    /// note's are, is read in one go from each block rather than one by one.
    pub fn vectors(&self, ids: &[u32]) -> io::Result<Vec<Option<Quantized>>> {
        if ids.iter().any(|&id| id >= self.header.passages) {
            return Err(damaged());
        }
        let d = self.header.dims as usize;
        let p = self.header.prefix as usize;
        let mut out = Vec::with_capacity(ids.len());
        let mut start = 0;
        while start < ids.len() {
            let mut end = start + 1;
            while end < ids.len() && u64::from(ids[end]) == u64::from(ids[end - 1]) + 1 {
                end += 1;
            }
            let first = u64::from(ids[start]);
            let n = end - start;
            start = end;
            let scales = self.read(self.header.scales + first * 8, n * 8)?;
            if d == 0 {
                out.extend(std::iter::repeat_n(None, n));
                continue;
            }
            let prefixes = self.read(self.header.prefixes + first * p as u64, n * p)?;
            let rests = self.read(self.header.rests + first * (d - p) as u64, n * (d - p))?;
            for k in 0..n {
                let scale = f32::from_le_bytes(scales[k * 8..k * 8 + 4].try_into().expect("4"));
                if scale == 0.0 {
                    out.push(None);
                    continue;
                }
                let mut values: Vec<i8> = Vec::with_capacity(d);
                values.extend(prefixes[k * p..(k + 1) * p].iter().map(|&b| b as i8));
                values.extend(
                    rests[k * (d - p)..(k + 1) * (d - p)]
                        .iter()
                        .map(|&b| b as i8),
                );
                out.push(Some(Quantized { scale, values }));
            }
        }
        Ok(out)
    }

    /// Whether passages `first..first + count` all have vectors: what a
    /// record copied from them would say of itself.
    pub fn has_vectors(&self, first: u32, count: u32) -> io::Result<bool> {
        if count == 0 {
            return Ok(true);
        }
        if self.header.dims == 0 {
            return Ok(false);
        }
        let scales = self.read(
            self.header.scales + u64::from(first) * 8,
            count as usize * 8,
        )?;
        Ok(scales
            .chunks_exact(8)
            .all(|row| f32::from_le_bytes(row[..4].try_into().expect("4")) != 0.0))
    }

    /// How many notes the catalog lists.
    #[must_use]
    pub fn note_count(&self) -> usize {
        self.header.notes as usize
    }

    fn cursor(&self, from: u64, end: u64, size: usize) -> Cursor<'_> {
        Cursor::new(&self.file, from, end, size)
    }

    /// A term's postings, found by a binary search of the vocabulary on disk:
    /// how many passages it is in, and which, with how often in each.
    pub fn postings(&self, term: &str) -> io::Result<Option<Postings>> {
        let (count, entries, text_at) = self.vocabulary()?;
        let (mut low, mut high) = (0u64, u64::from(count));
        let mut entry = [0u8; VOCAB_ENTRY_BYTES as usize];
        while low < high {
            let middle = (low + high) / 2;
            read_at(&self.file, entries + middle * VOCAB_ENTRY_BYTES, &mut entry)?;
            let field = |i: usize| u32::from_le_bytes(entry[i..i + 4].try_into().expect("4"));
            let from = text_at + u64::from(field(0));
            if from + u64::from(field(4)) > self.header.postings {
                return Err(damaged());
            }
            let text = self.read(from, field(4) as usize)?;
            match text.as_slice().cmp(term.as_bytes()) {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => {
                    let df = field(8);
                    let at = u64::from_le_bytes(entry[12..20].try_into().expect("8"));
                    let len = field(20) as usize;
                    let within = self.header.forward - self.header.postings;
                    if at.checked_add(len as u64).is_none_or(|end| end > within) {
                        return Err(damaged());
                    }
                    let bytes = self.read(self.header.postings + at, len)?;
                    let mut reader = Reader::new(&bytes);
                    // Each posting takes two bytes at least, whatever the
                    // count says.
                    let mut list = Vec::with_capacity((df as usize).min(len / 2));
                    let mut id = 0u64;
                    while !reader.done() {
                        let (Some(delta), Some(tf)) = (reader.varint(), reader.varint()) else {
                            return Err(damaged());
                        };
                        id = id.checked_add(delta).ok_or_else(damaged)?;
                        if id >= u64::from(self.header.passages) {
                            return Err(damaged());
                        }
                        list.push((id as u32, tf as u32));
                    }
                    return Ok(Some((df, list)));
                }
            }
        }
        Ok(None)
    }

    /// How many terms the vocabulary has, where its entries start, and where
    /// their text does, checked to fit before the postings.
    fn vocabulary(&self) -> io::Result<(u32, u64, u64)> {
        let mut bytes = [0u8; 4];
        if self.header.vocabulary + 4 > self.header.postings {
            return Err(damaged());
        }
        read_at(&self.file, self.header.vocabulary, &mut bytes)?;
        let count = u32::from_le_bytes(bytes);
        let entries = self.header.vocabulary + 4;
        let text_at = entries + u64::from(count) * VOCAB_ENTRY_BYTES + 4;
        if text_at > self.header.postings {
            return Err(damaged());
        }
        Ok((count, entries, text_at))
    }

    /// The notes `wanted` (indices into [`Segment::notes`]) as records,
    /// vectors and terms included: a segment read back.
    ///
    /// The forward lists are read in one pass, and of the vocabulary only the
    /// terms these notes use, so a few notes cost little whatever the size of
    /// the segment.
    pub fn records(&self, notes: &[NoteInfo], wanted: &[usize]) -> io::Result<Vec<NoteRecord>> {
        let mut ids: Vec<u32> = wanted
            .iter()
            .flat_map(|&index| notes[index].first..notes[index].first + notes[index].count)
            .collect();
        ids.sort_unstable();
        ids.dedup();

        // Their terms, by place, from the forward lists.
        let mut lists: Vec<Vec<(u32, u32)>> = Vec::with_capacity(ids.len());
        let mut cursor = self.cursor(self.header.forward, self.header.end, 64 << 10);
        let mut listed = 0u32;
        for &id in &ids {
            while listed < id {
                let n = cursor.varint()?;
                for _ in 0..n * 2 {
                    cursor.varint()?;
                }
                listed += 1;
            }
            let n = cursor.varint()?;
            let mut list = Vec::with_capacity(n.min(1 << 16) as usize);
            for _ in 0..n {
                let place = u32::try_from(cursor.varint()?).map_err(|_| damaged())?;
                list.push((place, cursor.varint()? as u32));
            }
            lists.push(list);
            listed += 1;
        }
        drop(cursor);
        let mut places: Vec<u32> = lists.iter().flatten().map(|&(place, _)| place).collect();
        places.sort_unstable();
        places.dedup();
        let terms = self.terms(&places)?;
        let vectors = self.vectors(&ids)?;

        let mut out = Vec::with_capacity(wanted.len());
        for &index in wanted {
            let note = &notes[index];
            let rows = self.read(
                self.header.table + u64::from(note.first) * 16,
                note.count as usize * 16,
            )?;
            let hashes = self.hashes(note.first, note.count)?;
            let mut passages = Vec::with_capacity(note.count as usize);
            for ((id, row), hash) in (note.first..).zip(rows.chunks_exact(16)).zip(hashes) {
                let at =
                    |i: usize| u32::from_le_bytes(row[i * 4..i * 4 + 4].try_into().expect("4"));
                let k = ids
                    .binary_search(&id)
                    .expect("every passage wanted was read");
                let terms = lists[k]
                    .iter()
                    .map(|&(place, tf)| {
                        let term = &terms[places.binary_search(&place).expect("read")];
                        (term.clone(), tf)
                    })
                    .collect();
                passages.push(PassageRecord {
                    start: at(1),
                    end: at(2),
                    length: at(3),
                    terms,
                    vector: vectors[k].clone(),
                    hash,
                });
            }
            out.push(NoteRecord {
                rel: note.rel.clone(),
                title: note.title.clone(),
                stamp: note.stamp,
                size: note.size,
                hash: note.hash,
                indexed: note.indexed,
                aliases: note.aliases(),
                links: note.links(),
                tags: note.tags(),
                passages,
            });
        }
        Ok(out)
    }

    /// The terms at `places` in the vocabulary, which must be sorted.
    fn terms(&self, places: &[u32]) -> io::Result<Vec<String>> {
        let (count, entries, text_at) = self.vocabulary()?;
        let mut entry = self.cursor(entries, text_at - 4, 16 << 10);
        let mut text = self.cursor(text_at, self.header.postings, 16 << 10);
        let mut out = Vec::with_capacity(places.len());
        for &place in places {
            if place >= count {
                return Err(damaged());
            }
            entry.seek(entries + u64::from(place) * VOCAB_ENTRY_BYTES)?;
            let offset = entry.u32()?;
            let len = entry.u32()?;
            text.seek(text_at + u64::from(offset))?;
            let bytes = text.bytes(u64::from(len))?;
            out.push(String::from_utf8(bytes).map_err(|_| damaged())?);
        }
        Ok(out)
    }
}

/// Reads the catalog's and the postings' encodings.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let slice = self.bytes.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(slice)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn str(&mut self) -> Option<String> {
        let len = self.u32()? as usize;
        String::from_utf8(self.take(len)?.to_vec()).ok()
    }

    /// Passes over a list, checking that it is whole and its text is text.
    fn skip_list(&mut self) -> Option<()> {
        let n = self.u32()?;
        for _ in 0..n {
            let len = self.u32()? as usize;
            std::str::from_utf8(self.take(len)?).ok()?;
        }
        Some(())
    }

    fn varint(&mut self) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.u8()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempVault;

    /// Vectors of `dims` components, of which the first two are scanned first.
    fn shape(dims: usize) -> Shape<'static> {
        Shape {
            model: if dims > 0 { "test" } else { "" },
            dims,
            prefix: 2,
        }
    }

    fn unit(values: &[f32]) -> Vec<f32> {
        let norm = values.iter().map(|x| x * x).sum::<f32>().sqrt();
        values.iter().map(|x| x / norm).collect()
    }

    fn passage(start: u32, terms: &[(&str, u32)], vector: Option<&[f32]>) -> PassageRecord {
        PassageRecord {
            start,
            end: start + 1,
            length: terms.iter().map(|(_, tf)| tf).sum(),
            terms: terms
                .iter()
                .map(|(t, tf)| ((*t).to_string(), *tf))
                .collect(),
            vector: vector.map(|v| Quantized::from_unit(&unit(v))),
            hash: if vector.is_some() {
                u64::from(start) * 1_000_003
            } else {
                0
            },
        }
    }

    fn note(rel: &str, passages: Vec<PassageRecord>) -> NoteRecord {
        NoteRecord {
            rel: rel.into(),
            title: rel.trim_end_matches(".md").into(),
            stamp: Stamp {
                written: 7,
                changed: 8,
                inode: 6,
            },
            size: 9,
            hash: 11,
            indexed: 13,
            aliases: vec!["alias".into()],
            links: vec!["Other".into()],
            tags: vec!["tag".into()],
            passages,
        }
    }

    fn sample() -> Vec<NoteRecord> {
        vec![
            note(
                "A.md",
                vec![
                    passage(
                        1,
                        &[("hash", 2), ("argon2", 1)],
                        Some(&[1.0, 0.0, 0.0, 0.0]),
                    ),
                    passage(5, &[("login", 1)], Some(&[0.0, 1.0, 0.0, 0.0])),
                ],
            ),
            note("B.md", vec![passage(1, &[("deploy", 3)], None)]),
        ]
    }

    #[test]
    fn a_segment_reads_back_what_was_written() {
        let dir = TempVault::new("segment");
        let path = dir.path().join("one.seg");
        write(&path, &sample(), &shape(4), false).expect("write");
        let segment = Segment::open(&path).expect("open");

        let notes = segment.notes().expect("notes");
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].rel, "A.md");
        assert_eq!(notes[0].links(), vec!["Other"]);
        assert_eq!(notes[0].aliases(), vec!["alias"]);
        assert_eq!(notes[0].tags(), vec!["tag"]);
        assert!(notes[0].embedded && !notes[1].embedded);
        assert_eq!((notes[1].first, notes[1].count), (2, 1));

        let table = segment.passages().expect("passages");
        assert_eq!(table.len(), 3);
        assert_eq!(
            table[1],
            PassageInfo {
                note: 0,
                start: 5,
                end: 6,
                length: 1
            }
        );
        assert_eq!(segment.total_length(), 7);

        let (df, list) = segment.postings("hash").expect("read").expect("found");
        assert_eq!((df, list), (1, vec![(0, 2)]));
        assert_eq!(
            segment.postings("deploy").expect("read").expect("found").1,
            vec![(2, 3)]
        );
        assert!(segment.postings("absent").expect("read").is_none());
        assert!(segment.postings("a").expect("read").is_none());

        let records = segment.records(&notes, &[0, 1]).expect("records");
        assert_eq!(records, sample(), "a merge copies notes exactly");
    }

    #[test]
    fn the_prefix_scan_finds_the_closest_passages_and_skips_the_unembedded() {
        let dir = TempVault::new("segment-scan");
        let path = dir.path().join("one.seg");
        write(&path, &sample(), &shape(4), false).expect("write");
        let segment = Segment::open(&path).expect("open");

        let found = segment.coarse(&[0.0, 1.0], &|_| true, 5).expect("scan");
        assert_eq!(found.len(), 2, "the passage with no vector isn't scored");
        assert_eq!(found[0].0, 1);
        assert!((found[0].1 - 1.0).abs() < 0.01, "{found:?}");

        let kept = segment.coarse(&[0.0, 1.0], &|id| id != 1, 5).expect("scan");
        assert_eq!(kept[0].0, 0, "filtered passages are skipped");

        let vectors = segment.vectors(&[0, 2]).expect("vectors");
        assert!(vectors[1].is_none());
        let exact = vectors[0]
            .as_ref()
            .expect("a vector")
            .dot(&unit(&[1.0, 0.0, 0.0, 0.0]));
        assert!((exact - 1.0).abs() < 0.01);
    }

    #[test]
    fn a_damaged_segment_is_refused() {
        let dir = TempVault::new("segment-damage");
        let path = dir.path().join("one.seg");
        write(&path, &sample(), &shape(4), false).expect("write");
        let mut bytes = std::fs::read(&path).expect("read");
        bytes.truncate(bytes.len() - 1);
        std::fs::write(&path, &bytes).expect("write");
        assert!(Segment::open(&path).is_err());

        std::fs::write(&path, b"not a segment at all, but long enough to have a header of sorts, yes indeed it is long").expect("write");
        assert!(Segment::open(&path).is_err());
    }

    const WORDS: [&str; 8] = [
        "alpha", "beta", "gamma", "delta", "kappa", "omega", "sigma", "theta",
    ];

    /// Notes with up to three passages each, of terms drawn from a small
    /// vocabulary so that segments made of them share terms, and vectors of
    /// `dims` components unless that is 0. The second note's first passage,
    /// if it has one, has a vector of zeros: a model's answer with no
    /// direction, which a segment holds but can't give back.
    fn generated(names: &[&str], seed: u64, dims: usize) -> Vec<NoteRecord> {
        let mut state = seed;
        let mut next = move |n: u64| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) % n
        };
        let mut notes: Vec<NoteRecord> = names
            .iter()
            .map(|name| {
                let passages = (0..next(4))
                    .map(|k| {
                        let mut terms: Vec<(String, u32)> = Vec::new();
                        for word in WORDS {
                            if next(3) == 0 {
                                terms.push((word.to_string(), next(4) as u32 + 1));
                            }
                        }
                        if terms.is_empty() {
                            terms.push((format!("only{}", next(5)), 1));
                        }
                        terms.sort();
                        let vector: Vec<f32> = (0..dims).map(|_| next(200) as f32 - 99.5).collect();
                        PassageRecord {
                            start: k as u32 * 3 + 1,
                            end: k as u32 * 3 + 2,
                            length: terms.iter().map(|(_, tf)| tf).sum(),
                            terms,
                            vector: (dims > 0).then(|| Quantized::from_unit(&unit(&vector))),
                            hash: if dims > 0 { next(1 << 30) + 1 } else { 0 },
                        }
                    })
                    .collect();
                note(name, passages)
            })
            .collect();
        if let Some(passage) = notes.get_mut(1).and_then(|n| n.passages.first_mut())
            && dims > 0
        {
            passage.vector = Some(Quantized::from_unit(&vec![0.0; dims]));
            passage.hash = 0;
        }
        notes
    }

    fn written(dir: &TempVault, name: &str, notes: &[NoteRecord], dims: usize) -> Segment {
        let path = dir.path().join(name);
        write(&path, notes, &shape(dims), false).expect("write");
        Segment::open(&path).expect("open")
    }

    /// The notes `keep` keeps of each segment, as records sorted by path.
    fn kept_records(segments: &[&Segment], keep: &[Vec<bool>]) -> Vec<NoteRecord> {
        let mut records = Vec::new();
        for (segment, keep) in segments.iter().zip(keep) {
            let notes = segment.notes().expect("notes");
            let wanted: Vec<usize> = (0..notes.len()).filter(|&n| keep[n]).collect();
            records.extend(segment.records(&notes, &wanted).expect("records"));
        }
        records.sort_by(|a, b| a.rel.cmp(&b.rel));
        records
    }

    /// Merges `segments` into `name`, keeping what `keep` says.
    fn merged(
        dir: &TempVault,
        name: &str,
        segments: &[&Segment],
        keep: &[Vec<bool>],
        dims: usize,
    ) -> io::Result<Segment> {
        let sources: Vec<Source<'_>> = segments
            .iter()
            .zip(keep)
            .map(|(segment, keep)| Source { segment, keep })
            .collect();
        let path = dir.path().join(name);
        merge(&path, &sources, &shape(dims), false)?;
        Segment::open(&path)
    }

    /// Asserts two segments hold the same, and are the same bytes.
    fn same(dir: &TempVault, merged: (&str, &Segment), direct: (&str, &Segment)) {
        let notes = merged.1.notes().expect("notes");
        assert_eq!(notes, direct.1.notes().expect("notes"));
        let all: Vec<usize> = (0..notes.len()).collect();
        assert_eq!(
            merged.1.records(&notes, &all).expect("records"),
            direct.1.records(&notes, &all).expect("records")
        );
        for term in WORDS
            .iter()
            .map(|w| (*w).to_string())
            .chain((0..5).map(|n| format!("only{n}")))
        {
            assert_eq!(
                merged.1.postings(&term).expect("postings"),
                direct.1.postings(&term).expect("postings"),
                "{term}"
            );
        }
        assert_eq!(
            std::fs::read(dir.path().join(merged.0)).expect("read"),
            std::fs::read(dir.path().join(direct.0)).expect("read"),
            "byte for byte"
        );
    }

    #[test]
    fn a_merge_writes_what_writing_the_merged_records_would() {
        let dir = TempVault::new("segment-merge");
        let old = written(
            &dir,
            "1.seg",
            &generated(&["A.md", "B.md", "C.md", "D.md", "E.md"], 1, 6),
            6,
        );
        let new = written(
            &dir,
            "2.seg",
            &generated(&["B.md", "D.md", "F.md", "G.md"], 2, 6),
            6,
        );
        let newest = written(&dir, "3.seg", &generated(&["A.md", "H.md"], 3, 6), 6);
        let segments = [&old, &new, &newest];
        // The newer copies of A, B and D win; C and G were deleted.
        let keep = vec![
            vec![false, false, false, false, true],
            vec![true, true, true, false],
            vec![true, true],
        ];
        let merged = merged(&dir, "merged.seg", &segments, &keep, 6).expect("merge");
        let records = kept_records(&segments, &keep);
        let direct = written(&dir, "direct.seg", &records, 6);

        let names: Vec<String> = merged
            .notes()
            .expect("notes")
            .into_iter()
            .map(|n| n.rel)
            .collect();
        assert_eq!(names, ["A.md", "B.md", "D.md", "E.md", "F.md", "H.md"]);
        let notes = merged.notes().expect("notes");
        let all: Vec<usize> = (0..notes.len()).collect();
        assert_eq!(merged.records(&notes, &all).expect("records"), records);
        same(&dir, ("merged.seg", &merged), ("direct.seg", &direct));
        assert!(
            merged.passage_count() > 5 && notes.iter().any(|n| n.count > 0 && !n.embedded),
            "passages to renumber, and a zero vector that isn't brought"
        );

        let mut files: Vec<String> = std::fs::read_dir(dir.path())
            .expect("dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        assert_eq!(
            files,
            ["1.seg", "2.seg", "3.seg", "direct.seg", "merged.seg"],
            "no scratch files are left"
        );
    }

    #[test]
    fn a_merge_leaves_out_vectors_it_cant_keep() {
        let dir = TempVault::new("segment-merge-vectors");
        let with = written(
            &dir,
            "1.seg",
            &generated(&["A.md", "B.md", "C.md"], 4, 6),
            6,
        );
        let without = written(&dir, "2.seg", &generated(&["D.md", "E.md"], 5, 0), 0);
        let segments = [&with, &without];
        let keep = vec![vec![true; 3], vec![true; 2]];

        // A source without vectors brings none.
        let brought = merged(&dir, "merged.seg", &segments, &keep, 6).expect("merge");
        let records = kept_records(&segments, &keep);
        assert!(
            records
                .iter()
                .any(|r| r.passages.iter().any(|p| p.vector.is_some()))
        );
        let direct = written(&dir, "direct.seg", &records, 6);
        same(&dir, ("merged.seg", &brought), ("direct.seg", &direct));
        let notes = brought.notes().expect("notes");
        let d = notes.iter().find(|n| n.rel == "D.md").expect("D");
        assert_eq!(d.embedded, d.count == 0);

        // Stripped, none has any.
        let stripped = merged(&dir, "stripped.seg", &segments, &keep, 0).expect("merge");
        let mut bare = records.clone();
        for passage in bare.iter_mut().flat_map(|r| r.passages.iter_mut()) {
            passage.vector = None;
        }
        let direct = written(&dir, "bare.seg", &bare, 0);
        same(&dir, ("stripped.seg", &stripped), ("bare.seg", &direct));
        assert_eq!(stripped.dims(), 0);
        assert!(
            stripped
                .notes()
                .expect("notes")
                .iter()
                .all(|n| n.embedded == (n.count == 0))
        );
    }

    #[test]
    fn a_path_kept_by_two_sources_comes_from_the_later() {
        let dir = TempVault::new("segment-merge-twice");
        let old = written(&dir, "1.seg", &generated(&["A.md", "B.md"], 6, 6), 6);
        let new = written(&dir, "2.seg", &generated(&["B.md"], 7, 6), 6);
        let keep = vec![vec![true, true], vec![true]];
        let merged = merged(&dir, "merged.seg", &[&old, &new], &keep, 6).expect("merge");
        let expected = kept_records(&[&old, &new], &[vec![true, false], vec![true]]);
        let notes = merged.notes().expect("notes");
        assert_eq!(merged.records(&notes, &[0, 1]).expect("records"), expected);
    }

    #[test]
    fn a_merge_that_fails_leaves_nothing_behind() {
        let dir = TempVault::new("segment-merge-damage");
        let path = dir.path().join("1.seg");
        write(&path, &sample(), &shape(4), false).expect("write");
        // Forward lists that never end, in a file of the right length.
        let mut bytes = std::fs::read(&path).expect("read");
        let forward = u64::from_le_bytes(bytes[36 + 8 * 8..36 + 8 * 9].try_into().expect("8"));
        for byte in &mut bytes[forward as usize..] {
            *byte = 0xff;
        }
        std::fs::write(&path, &bytes).expect("write");
        let segment = Segment::open(&path).expect("open");
        let keep = vec![vec![true, true]];
        assert!(merged(&dir, "merged.seg", &[&segment], &keep, 4).is_err());
        let files: Vec<String> = std::fs::read_dir(dir.path())
            .expect("dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files, ["1.seg"]);
    }

    #[test]
    fn vectors_are_read_a_run_at_a_time_and_come_back_the_same() {
        let dir = TempVault::new("segment-vectors");
        let notes = generated(&["A.md", "B.md", "C.md", "D.md"], 8, 6);
        let segment = written(&dir, "1.seg", &notes, 6);
        let all: Vec<u32> = (0..segment.passage_count() as u32).collect();
        let together = segment.vectors(&all).expect("vectors");
        let apart: Vec<Option<Quantized>> = all
            .iter()
            .flat_map(|&id| segment.vectors(&[id]).expect("vector"))
            .collect();
        assert_eq!(together, apart);
        let scattered = [3, 1, 2, 0].map(|i| all.get(i).copied().unwrap_or(0));
        let picked = segment.vectors(&scattered).expect("vectors");
        for (id, vector) in scattered.iter().zip(&picked) {
            assert_eq!(vector, &together[*id as usize]);
        }
    }

    #[test]
    fn quantized_vectors_keep_their_direction() {
        let v = unit(&[0.3, -0.2, 0.9, 0.05, -0.6]);
        let q = Quantized::from_unit(&v);
        assert!((q.dot(&v) - 1.0).abs() < 0.01);
        let floats = q.floats();
        assert!(floats.iter().zip(&v).all(|(a, b)| (a - b).abs() < 0.01));
        assert_eq!(Quantized::from_unit(&[0.0, 0.0]).scale, 0.0);
    }

    #[test]
    fn varints_round_trip() {
        let mut out = Vec::new();
        for value in [0u64, 1, 127, 128, 300, u64::from(u32::MAX), u64::MAX] {
            put_varint(&mut out, value);
        }
        let mut reader = Reader::new(&out);
        for value in [0u64, 1, 127, 128, 300, u64::from(u32::MAX), u64::MAX] {
            assert_eq!(reader.varint(), Some(value));
        }
        assert!(reader.done());
    }
}
