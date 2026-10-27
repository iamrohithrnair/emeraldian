//! Obsidian vault discovery and scanning.
//!
//! A vault is just a directory of Markdown files, which is what makes it
//! possible to open one from a terminal at all. Obsidian additionally keeps a
//! registry of the vaults it knows about in `obsidian.json`, so emeraldian
//! can offer the same vault list the desktop app shows rather than asking the
//! user to type a path.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::UNIX_EPOCH;

use serde::Deserialize;

use crate::error::Error;
use crate::gitignore::Gitignore;
use crate::note::{NoteMeta, Stamp};

/// Directory Obsidian uses for its own per-vault settings. It contains no
/// notes, so scanning skips it entirely.
pub const OBSIDIAN_DIR: &str = ".obsidian";

/// The trash folder Obsidian moves deleted notes into.
pub const TRASH_DIR: &str = ".trash";

/// Folders a code project fills with dependencies, build output and caches.
///
/// None of them holds anyone's notes, and between them they can hold hundreds
/// of thousands of files: a JavaScript project's `node_modules` alone is often
/// most of the folder. Editors leave them out of their file lists for the same
/// reason. Only skipped in a folder that isn't an Obsidian vault, see
/// [`ScanOptions::skip_project_folders`].
pub const PROJECT_FOLDERS: &[&str] = &[
    "node_modules",
    "bower_components",
    "target",
    "dist",
    "build",
    "vendor",
    "venv",
    "__pycache__",
];

/// A vault Obsidian knows about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vault {
    /// Display name, taken from the directory name.
    pub name: String,
    pub path: PathBuf,
    /// Whether Obsidian currently has this vault open.
    pub open: bool,
    /// Obsidian's last-opened timestamp (milliseconds), used for ordering.
    pub ts: u64,
}

impl Vault {
    /// Builds a vault from a directory path, without consulting Obsidian.
    ///
    /// This is what makes emeraldian usable on a plain folder of Markdown
    /// files — no Obsidian install required.
    #[must_use]
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("vault")
            .to_string();
        Self {
            name,
            path,
            open: false,
            ts: 0,
        }
    }

    /// Whether this directory looks like a vault Obsidian has opened before.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.path.join(OBSIDIAN_DIR).is_dir()
    }

    /// Resolves a vault-relative path to an absolute one inside the vault.
    ///
    /// Rejects absolute paths and any `..` that would escape the root — the
    /// check is on the *lexical* path, before touching the filesystem, so a
    /// traversal attempt never becomes a real file operation.
    ///
    /// A question about the vault's root rather than its contents, so it needs
    /// no index: the command line checks a path this way without reading every
    /// note first.
    pub fn resolve_path(&self, rel: &str) -> crate::error::Result<PathBuf> {
        let rel = rel.trim().trim_start_matches('/');
        if rel.is_empty() {
            return Err(Error::InvalidName(rel.to_string()));
        }

        let candidate = Path::new(rel);
        let mut depth = 0i32;
        for component in candidate.components() {
            match component {
                Component::Normal(_) => depth += 1,
                Component::CurDir => {}
                Component::ParentDir => {
                    depth -= 1;
                    if depth < 0 {
                        return Err(Error::OutsideVault(candidate.to_path_buf()));
                    }
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(Error::OutsideVault(candidate.to_path_buf()));
                }
            }
        }

        Ok(self.path.join(candidate))
    }

    /// Normalizes a note name into a vault-relative `.md` path.
    ///
    /// Accepts `Note`, `Note.md`, `Folder/Note` and `Folder/Note.md` alike,
    /// because that's what a user types and what a model emits.
    pub fn note_rel(&self, name: &str) -> crate::error::Result<String> {
        let name = name.trim().trim_start_matches('/');
        if name.is_empty() {
            return Err(Error::InvalidName(name.to_string()));
        }

        let (folder, file) = match name.rsplit_once('/') {
            Some((folder, file)) => (folder, file),
            None => ("", name),
        };
        let stem = file.strip_suffix(".md").unwrap_or(file);
        crate::ops::validate_name(stem)?;

        let rel = if folder.is_empty() {
            format!("{stem}.md")
        } else {
            format!("{folder}/{stem}.md")
        };

        // Run the traversal guard over the assembled path.
        self.resolve_path(&rel)?;
        Ok(rel)
    }

    /// Walks the vault and returns every note, sorted by relative path.
    ///
    /// Sorting here rather than at each call site means the explorer, the quick
    /// switcher and the graph all agree on note order regardless of the order
    /// the filesystem happened to hand entries back in.
    pub fn scan(&self, options: &ScanOptions) -> Result<Scan, std::io::Error> {
        // An unreadable *subdirectory* is tolerated during the walk, but an
        // unreadable vault root is a real failure — on macOS it means the
        // terminal hasn't been granted access to the folder, and showing an
        // empty vault instead of saying so sends the user hunting for a bug in
        // their notes.
        fs::read_dir(&self.path).map_err(|err| {
            std::io::Error::new(
                err.kind(),
                format!("cannot read {}: {err}", self.path.display()),
            )
        })?;

        let mut scan = walk(&self.path, options)?;
        // Every path is there once, so an unstable sort is as good as any.
        scan.folders.sort_unstable();
        scan.attachments.sort_unstable();
        Ok(scan)
    }
}

/// Controls what a vault scan includes.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Include dot-directories and dot-files other than `.obsidian`/`.trash`,
    /// which are always skipped.
    pub include_hidden: bool,
    /// Record non-Markdown files (images, PDFs) as attachments.
    pub include_attachments: bool,
    /// Guards against symlink loops and pathological trees.
    pub max_depth: usize,
    /// Additional vault-relative folder names to skip.
    pub excluded_folders: Vec<String>,
    /// Skip the folders in [`PROJECT_FOLDERS`] and whatever the folder's
    /// `.gitignore` files name, the way an editor does.
    ///
    /// For a folder that isn't an Obsidian vault, a code project say, opened
    /// to read one of its notes. Obsidian itself does neither, so a real vault
    /// is scanned with this off and shows exactly what Obsidian shows.
    pub skip_project_folders: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            include_hidden: false,
            include_attachments: true,
            max_depth: 32,
            excluded_folders: Vec::new(),
            skip_project_folders: false,
        }
    }
}

/// The result of walking a vault.
#[derive(Debug, Default, Clone)]
pub struct Scan {
    pub notes: Vec<NoteMeta>,
    /// Vault-relative folder paths, including empty ones so the explorer can
    /// show a folder the user just created.
    pub folders: Vec<String>,
    /// Vault-relative paths of non-Markdown files.
    pub attachments: Vec<String>,
}

impl Scan {
    /// Finds the note a link target names, from file names and paths alone.
    ///
    /// The same answer [`crate::index::VaultIndex::resolve`] gives — a path
    /// before a file name, the first note in path order when two share a name,
    /// case ignored — for every target but an alias. Aliases live in each
    /// note's frontmatter, and not having to read every note to find them is
    /// the point: a scan lists a large vault in a fraction of the time an index
    /// takes to read it. `None` means no file answers to the name, and only an
    /// index can say whether an alias does.
    #[must_use]
    pub fn resolve(&self, target: &str) -> Option<&NoteMeta> {
        let key = target.trim().trim_end_matches('/').to_lowercase();
        if key.is_empty() {
            return None;
        }
        // `[[folder/Note]]` written with a leading `./` or `/`.
        let bare = key.trim_start_matches(['.', '/']);
        for key in [key.as_str(), bare] {
            let by_path = self.notes.iter().find(|note| {
                let rel = note.rel.to_lowercase();
                rel == key || rel.strip_suffix(".md") == Some(key)
            });
            if by_path.is_some() {
                return by_path;
            }
            let by_name = self
                .notes
                .iter()
                .find(|note| note.stem.to_lowercase() == key);
            if by_name.is_some() {
                return by_name;
            }
        }
        None
    }
}

/// How many of a directory's notes one worker stats at a time: enough that
/// handing them over costs little, few enough that a folder of thousands is
/// shared out.
const STAT_BATCH: usize = 128;

/// The most threads that stat notes alongside the one reading directories.
/// Asking for a file's times scales with threads where reading a directory
/// doesn't, and stops scaling at about this many.
const STAT_WORKERS: usize = 5;

/// Notes found in one directory, to be statted: its path in the vault, and
/// their entries.
type Batch = (Arc<str>, Vec<fs::DirEntry>);

/// The batches waiting for a worker, and whether more will come.
struct Batches {
    waiting: Vec<Batch>,
    done: bool,
}

/// Walks the vault: this thread reads the directories, one after another,
/// while workers ask for the times and sizes of the notes it finds, which is
/// most of a walk's cost and, unlike reading directories, is done no faster
/// by one thread than by several. Each worker sorts what it found, and the
/// sorted runs are merged.
fn walk(root: &Path, options: &ScanOptions) -> Result<Scan, std::io::Error> {
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get().saturating_sub(1))
        .clamp(1, STAT_WORKERS);
    let queue = Mutex::new(Batches {
        waiting: Vec::new(),
        done: false,
    });
    let ready = Condvar::new();
    let mut scan = Scan::default();
    let runs = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| scope.spawn(|| stat_notes(&queue, &ready)))
            .collect();
        let read = read_directories(root, options, &mut scan, &mut |batch| {
            lock(&queue).waiting.push(batch);
            ready.notify_one();
        });
        lock(&queue).done = true;
        ready.notify_all();
        let runs: Vec<Vec<NoteMeta>> = handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect();
        read.map(|()| runs)
    })?;
    scan.notes = merge_runs(runs);
    Ok(scan)
}

/// The queue, even if a worker panicked holding it: what is in it is still
/// whole, and the panic is passed on when the worker is joined.
fn lock(queue: &Mutex<Batches>) -> std::sync::MutexGuard<'_, Batches> {
    queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Reads every directory under `root` in turn, recording folders and
/// attachments, and handing the notes found to `found` a batch at a time.
fn read_directories(
    root: &Path,
    options: &ScanOptions,
    scan: &mut Scan,
    found: &mut dyn FnMut(Batch),
) -> Result<(), std::io::Error> {
    let mut stack: Vec<(PathBuf, Arc<str>, usize, Ignores)> =
        vec![(root.to_path_buf(), Arc::from(""), 0, None)];
    while let Some((dir, prefix, depth, ignores)) = stack.pop() {
        if depth > options.max_depth {
            continue;
        }
        // Read before the folder's entries, since its rules apply to them.
        let ignores = if options.skip_project_folders {
            with_gitignore(&dir, ignores)
        } else {
            ignores
        };
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            // A directory we can't read (permissions, a broken mount) shouldn't
            // abort the whole scan — the rest of the vault is still usable.
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => continue,
            Err(err) => return Err(err),
        };
        let mut notes: Vec<fs::DirEntry> = Vec::new();
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if name == OBSIDIAN_DIR || name == TRASH_DIR {
                continue;
            }
            if name.starts_with('.') && !options.include_hidden {
                continue;
            }
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if options.skip_project_folders
                && ((is_dir && PROJECT_FOLDERS.contains(&name))
                    || is_ignored(ignores.as_deref(), &entry.path(), is_dir))
            {
                continue;
            }
            if is_dir {
                let rel = join(&prefix, name);
                if options.excluded_folders.iter().any(|f| f == &rel) {
                    continue;
                }
                scan.folders.push(rel.clone());
                stack.push((entry.path(), Arc::from(rel), depth + 1, ignores.clone()));
            } else if is_markdown(Path::new(name)) {
                notes.push(entry);
                if notes.len() == STAT_BATCH {
                    found((Arc::clone(&prefix), std::mem::take(&mut notes)));
                }
            } else if options.include_attachments {
                scan.attachments.push(join(&prefix, name));
            }
        }
        if !notes.is_empty() {
            found((prefix, notes));
        }
    }
    Ok(())
}

/// A worker: takes batches of notes until there are no more, and gives back
/// what it made of them, sorted by path.
fn stat_notes(queue: &Mutex<Batches>, ready: &Condvar) -> Vec<NoteMeta> {
    let mut made = Vec::new();
    loop {
        let batch = {
            let mut batches = lock(queue);
            loop {
                if let Some(batch) = batches.waiting.pop() {
                    break Some(batch);
                }
                if batches.done {
                    break None;
                }
                batches = ready
                    .wait(batches)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        let Some((prefix, entries)) = batch else {
            break;
        };
        made.extend(entries.into_iter().map(|entry| note_of(&prefix, &entry)));
    }
    made.sort_unstable_by(|a, b| a.rel.cmp(&b.rel));
    made
}

/// A note as a scan lists it, from its directory entry.
fn note_of(prefix: &str, entry: &fs::DirEntry) -> NoteMeta {
    let file_name = entry.file_name();
    let name = file_name.to_str().unwrap_or_default();
    let metadata = entry.metadata().ok();
    let stamp = |t: std::io::Result<std::time::SystemTime>| {
        t.ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
    };
    let modified = metadata
        .as_ref()
        .and_then(|m| stamp(m.modified()))
        .unwrap_or(0);
    // Windows and most Linux filesystems report a birth time; older
    // ext4 and some network mounts don't. Falling back to `modified`
    // keeps created-order sorting sane instead of piling every note
    // at the epoch.
    let created = metadata
        .as_ref()
        .and_then(|m| stamp(m.created()))
        .unwrap_or(modified);
    let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(name)
        .to_string();
    NoteMeta {
        path: entry.path(),
        rel: join(prefix, name),
        title: stem.clone(),
        stem,
        modified,
        created,
        size,
        stamp: metadata.as_ref().map(Stamp::of).unwrap_or_default(),
    }
}

/// Merges runs each sorted by path into one.
fn merge_runs(runs: Vec<Vec<NoteMeta>>) -> Vec<NoteMeta> {
    let total = runs.iter().map(Vec::len).sum();
    let mut out = Vec::with_capacity(total);
    let mut runs: Vec<std::vec::IntoIter<NoteMeta>> =
        runs.into_iter().map(Vec::into_iter).collect();
    let mut heads: Vec<Option<NoteMeta>> = runs.iter_mut().map(Iterator::next).collect();
    loop {
        let mut least: Option<usize> = None;
        for (i, head) in heads.iter().enumerate() {
            let Some(head) = head else {
                continue;
            };
            if least.is_none_or(|l| heads[l].as_ref().is_some_and(|other| head.rel < other.rel)) {
                least = Some(i);
            }
        }
        let Some(least) = least else {
            break;
        };
        out.extend(heads[least].take());
        heads[least] = runs[least].next();
    }
    out
}

/// A vault-relative path: a name in the folder at `prefix`.
fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

/// The `.gitignore` files met on the way down to a folder, nearest first.
/// Shared, since the walk goes depth-first off a stack rather than by
/// recursion, and every folder below keeps the chain it was found under.
type Ignores = Option<Arc<IgnoreLink>>;

/// One `.gitignore`, and the chain above it.
struct IgnoreLink {
    rules: Gitignore,
    parent: Ignores,
}

/// The chain for a folder: the one above it, with the folder's own
/// `.gitignore` in front if it has one that says anything.
fn with_gitignore(dir: &Path, ignores: Ignores) -> Ignores {
    match Gitignore::read(dir) {
        Some(rules) => Some(Arc::new(IgnoreLink {
            rules,
            parent: ignores,
        })),
        None => ignores,
    }
}

/// Whether the `.gitignore` files above a path leave it out. The nearest file
/// with an opinion decides, so a deeper `!keep.md` can bring back what a
/// shallower one ignored.
fn is_ignored(mut link: Option<&IgnoreLink>, path: &Path, is_dir: bool) -> bool {
    while let Some(here) = link {
        if let Some(ignored) = here.rules.decide(path, is_dir) {
            return ignored;
        }
        link = here.parent.as_deref();
    }
    false
}

/// Whether a path is a note. Obsidian only treats `.md` as notes.
#[must_use]
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

// ---------------------------------------------------------------------------
// obsidian.json
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ObsidianConfigJson {
    #[serde(default)]
    vaults: BTreeMap<String, VaultJson>,
}

#[derive(Deserialize)]
struct VaultJson {
    path: String,
    #[serde(default)]
    open: bool,
    #[serde(default)]
    ts: u64,
}

/// The directories Obsidian may keep `obsidian.json` in, most likely first.
#[must_use]
pub fn config_locations() -> Vec<PathBuf> {
    let mut locations = Vec::new();

    // macOS: ~/Library/Application Support/obsidian
    // Linux:  $XDG_CONFIG_HOME/obsidian
    // Windows: %APPDATA%\obsidian
    if let Some(dir) = dirs::config_dir() {
        locations.push(dir.join("obsidian"));
    }
    // Linux Obsidian has historically also used ~/.config even when
    // XDG_CONFIG_HOME points elsewhere.
    if let Some(home) = dirs::home_dir() {
        let dot_config = home.join(".config").join("obsidian");
        if !locations.contains(&dot_config) {
            locations.push(dot_config);
        }
        // Flatpak installs keep their own config tree.
        locations.push(
            home.join(".var")
                .join("app")
                .join("md.obsidian.Obsidian")
                .join("config")
                .join("obsidian"),
        );
    }

    locations
}

/// Loads the vaults Obsidian knows about.
///
/// Returns an empty list rather than an error when Obsidian isn't installed —
/// that's an ordinary situation for a terminal user, not a failure.
#[must_use]
pub fn discover() -> Vec<Vault> {
    for dir in config_locations() {
        if let Some(vaults) = load_from(&dir)
            && !vaults.is_empty()
        {
            return vaults;
        }
    }
    Vec::new()
}

/// Loads `obsidian.json` from a specific directory.
///
/// Vaults whose directory no longer exists are filtered out — Obsidian keeps
/// stale entries in the registry after a folder is moved or deleted, and
/// offering to open one would just fail later.
#[must_use]
pub fn load_from(config_dir: &Path) -> Option<Vec<Vault>> {
    let path = config_dir.join("obsidian.json");
    let text = fs::read_to_string(path).ok()?;
    let config: ObsidianConfigJson = serde_json::from_str(&text).ok()?;

    let mut vaults: Vec<Vault> = config
        .vaults
        .into_values()
        .map(|v| {
            let path = PathBuf::from(v.path);
            Vault {
                name: path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("vault")
                    .to_string(),
                path,
                open: v.open,
                ts: v.ts,
            }
        })
        .filter(|v| v.path.is_dir())
        .collect();

    // Most-recently-used first, which is the order the user thinks in.
    vaults.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| a.name.cmp(&b.name)));
    Some(vaults)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "emeraldian-vault-{tag}-{}-{:?}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
        fn write(&self, rel: &str, content: &str) {
            let path = self.0.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(path, content).expect("write file");
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    #[test]
    fn scan_finds_notes_and_folders_sorted() {
        let dir = TempDir::new("scan");
        dir.write("Zeta.md", "z");
        dir.write("Alpha.md", "a");
        dir.write("Projects/Idea.md", "i");

        let vault = Vault::from_path(dir.path());
        let scan = vault.scan(&ScanOptions::default()).expect("scan");

        let rels: Vec<_> = scan.notes.iter().map(|n| n.rel.as_str()).collect();
        assert_eq!(rels, vec!["Alpha.md", "Projects/Idea.md", "Zeta.md"]);
        assert_eq!(scan.folders, vec!["Projects"]);
    }

    #[test]
    fn scan_skips_obsidian_trash_and_hidden() {
        let dir = TempDir::new("skip");
        dir.write("Real.md", "r");
        dir.write(".obsidian/workspace.json", "{}");
        dir.write(".trash/Deleted.md", "d");
        dir.write(".secret/Hidden.md", "h");

        let scan = Vault::from_path(dir.path())
            .scan(&ScanOptions::default())
            .expect("scan");
        let rels: Vec<_> = scan.notes.iter().map(|n| n.rel.as_str()).collect();
        assert_eq!(rels, vec!["Real.md"]);
    }

    fn notes_and_files(dir: &TempDir, options: &ScanOptions) -> (Vec<String>, Vec<String>) {
        let scan = Vault::from_path(dir.path()).scan(options).expect("scan");
        let notes = scan.notes.into_iter().map(|n| n.rel).collect();
        (notes, scan.attachments)
    }

    #[test]
    fn a_project_s_dependency_and_build_folders_are_skipped_when_asked() {
        let dir = TempDir::new("project");
        dir.write("README.md", "r");
        dir.write("docs/Guide.md", "g");
        dir.write("node_modules/left-pad/README.md", "n");
        dir.write("web/node_modules/x/README.md", "n");
        dir.write("target/debug/notes.md", "t");
        dir.write("__pycache__/a.pyc", "p");

        let skipping = ScanOptions {
            skip_project_folders: true,
            ..ScanOptions::default()
        };
        let (notes, files) = notes_and_files(&dir, &skipping);
        assert_eq!(notes, vec!["README.md", "docs/Guide.md"]);
        assert!(files.is_empty(), "got {files:?}");

        // A vault keeps every one of them, as Obsidian does.
        let (notes, _) = notes_and_files(&dir, &ScanOptions::default());
        assert_eq!(notes.len(), 5, "got {notes:?}");
    }

    #[test]
    fn a_project_s_gitignore_is_honoured_when_asked() {
        let dir = TempDir::new("gitignore");
        dir.write(".gitignore", "/out\n*.log\ncache/\n");
        dir.write("Kept.md", "k");
        dir.write("out/Generated.md", "o");
        dir.write("src/out/Nested.md", "a folder only ignored at the top");
        dir.write("debug.log", "l");
        dir.write("src/cache/Old.md", "c");
        // A deeper file can bring back what a shallower one left out.
        dir.write("logs/.gitignore", "!keep.log\n");
        dir.write("logs/keep.log", "k");
        dir.write("logs/drop.log", "d");

        let skipping = ScanOptions {
            skip_project_folders: true,
            ..ScanOptions::default()
        };
        let (notes, files) = notes_and_files(&dir, &skipping);
        assert_eq!(notes, vec!["Kept.md", "src/out/Nested.md"]);
        assert_eq!(files, vec!["logs/keep.log"]);

        let (notes, files) = notes_and_files(&dir, &ScanOptions::default());
        assert_eq!(notes.len(), 4, "got {notes:?}");
        assert_eq!(files.len(), 3, "got {files:?}");
    }

    #[test]
    fn hidden_files_can_be_opted_into_but_obsidian_dir_never_is() {
        let dir = TempDir::new("hidden");
        dir.write(".secret/Hidden.md", "h");
        dir.write(".obsidian/plugin/notes.md", "p");

        let options = ScanOptions {
            include_hidden: true,
            ..Default::default()
        };
        let scan = Vault::from_path(dir.path()).scan(&options).expect("scan");
        let rels: Vec<_> = scan.notes.iter().map(|n| n.rel.as_str()).collect();
        assert_eq!(
            rels,
            vec![".secret/Hidden.md"],
            ".obsidian holds settings, never notes"
        );
    }

    #[test]
    fn attachments_are_separated_from_notes() {
        let dir = TempDir::new("attach");
        dir.write("Note.md", "n");
        dir.write("image.png", "binary");

        let scan = Vault::from_path(dir.path())
            .scan(&ScanOptions::default())
            .expect("scan");
        assert_eq!(scan.notes.len(), 1);
        assert_eq!(scan.attachments, vec!["image.png"]);
    }

    #[test]
    fn an_unreadable_vault_root_is_an_error_not_an_empty_vault() {
        let vault = Vault::from_path("/definitely/not/a/real/vault");
        let err = vault
            .scan(&ScanOptions::default())
            .expect_err("a missing vault must not look like an empty one");
        assert!(err.to_string().contains("cannot read"));
    }

    #[test]
    fn markdown_detection_is_case_insensitive() {
        assert!(is_markdown(Path::new("a.md")));
        assert!(is_markdown(Path::new("a.MD")));
        assert!(!is_markdown(Path::new("a.markdown")));
        assert!(!is_markdown(Path::new("a.txt")));
    }

    #[test]
    fn obsidian_json_is_parsed_and_stale_vaults_dropped() {
        let dir = TempDir::new("config");
        let live = dir.path().join("Live");
        fs::create_dir_all(&live).expect("create vault dir");

        let json = format!(
            r#"{{"vaults":{{
                "aaa":{{"path":{live:?},"ts":200,"open":true}},
                "bbb":{{"path":"/definitely/missing/Vault","ts":300}}
            }}}}"#
        );
        fs::write(dir.path().join("obsidian.json"), json).expect("write config");

        let vaults = load_from(dir.path()).expect("parse config");
        assert_eq!(
            vaults.len(),
            1,
            "a vault whose folder is gone is not offered"
        );
        assert_eq!(vaults[0].name, "Live");
        assert!(vaults[0].open);
    }

    #[test]
    fn a_scan_resolves_names_as_the_index_does_short_of_aliases() {
        let dir = TempDir::new("scan-resolve");
        dir.write("Target.md", "---\naliases: [Nickname]\n---\n");
        dir.write("A/Same.md", "a");
        dir.write("B/Same.md", "b");
        dir.write("B/Target.md", "deeper");

        let vault = Vault::from_path(dir.path());
        let scan = vault.scan(&ScanOptions::default()).expect("scan");
        let index = crate::index::VaultIndex::build(vault, ScanOptions::default()).expect("index");

        for target in [
            "Target",
            "target.md",
            "B/Target",
            "./B/Target",
            "/b/target.md",
            "Same",
            "B/Same",
            "Nowhere",
        ] {
            let from_index = index
                .resolve(target)
                .and_then(|id| index.note(id))
                .map(|note| note.meta.rel.as_str());
            assert_eq!(
                scan.resolve(target).map(|note| note.rel.as_str()),
                from_index,
                "{target}"
            );
        }
        assert_eq!(
            scan.resolve("Nickname"),
            None,
            "an alias takes the index to find"
        );
    }

    /// The walk as it was before it was shared out among threads: one
    /// directory after another, every note statted where it was found, the
    /// lists sorted at the end, `.gitignore` files kept on a stack as it
    /// recurses. What the new walk must give, exactly.
    fn reference_scan(root: &Path, options: &ScanOptions) -> Scan {
        fn walk(
            root: &Path,
            dir: &Path,
            options: &ScanOptions,
            ignores: &mut Vec<Gitignore>,
            out: &mut Scan,
            depth: usize,
        ) {
            if depth > options.max_depth {
                return;
            }
            let pushed = options.skip_project_folders
                && Gitignore::read(dir)
                    .map(|rules| ignores.push(rules))
                    .is_some();
            walk_entries(root, dir, options, ignores, out, depth);
            if pushed {
                ignores.pop();
            }
        }
        fn walk_entries(
            root: &Path,
            dir: &Path,
            options: &ScanOptions,
            ignores: &mut Vec<Gitignore>,
            out: &mut Scan,
            depth: usize,
        ) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if name == OBSIDIAN_DIR || name == TRASH_DIR {
                    continue;
                }
                if name.starts_with('.') && !options.include_hidden {
                    continue;
                }
                let Some(rel) = crate::note::relative_path(root, &path) else {
                    continue;
                };
                let ignored = ignores
                    .iter()
                    .rev()
                    .find_map(|rules| rules.decide(&path, is_dir))
                    .unwrap_or(false);
                if options.skip_project_folders
                    && ((is_dir && PROJECT_FOLDERS.contains(&name)) || ignored)
                {
                    continue;
                }
                if is_dir {
                    if options.excluded_folders.iter().any(|f| f == &rel) {
                        continue;
                    }
                    out.folders.push(rel);
                    walk(root, &path, options, ignores, out, depth + 1);
                } else if is_markdown(&path) {
                    let metadata = entry.metadata().ok();
                    let stamp = |t: std::io::Result<std::time::SystemTime>| {
                        t.ok()
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                    };
                    let modified = metadata
                        .as_ref()
                        .and_then(|m| stamp(m.modified()))
                        .unwrap_or(0);
                    let created = metadata
                        .as_ref()
                        .and_then(|m| stamp(m.created()))
                        .unwrap_or(modified);
                    let stem = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(name)
                        .to_string();
                    out.notes.push(NoteMeta {
                        path: path.clone(),
                        rel,
                        title: stem.clone(),
                        stem,
                        modified,
                        created,
                        size: metadata.as_ref().map(|m| m.len()).unwrap_or(0),
                        stamp: metadata.as_ref().map(Stamp::of).unwrap_or_default(),
                    });
                } else if options.include_attachments {
                    out.attachments.push(rel);
                }
            }
        }
        let mut scan = Scan::default();
        walk(root, root, options, &mut Vec::new(), &mut scan, 0);
        scan.notes.sort_by(|a, b| a.rel.cmp(&b.rel));
        scan.folders.sort();
        scan.attachments.sort();
        scan
    }

    #[test]
    fn the_shared_walk_finds_what_the_plain_one_did() {
        let dir = TempDir::new("walk-same");
        // Enough notes in one folder to be shared out in batches, and names
        // whose order as paths isn't their order as names.
        for i in 0..700 {
            dir.write(&format!("Big/n{i:04}.md"), "x");
        }
        for rel in [
            "a.md",
            "a-b.md",
            "a/inner.md",
            "a/deeper/most.MD",
            "a b/space.md",
            "Zed.md",
            "é/accent.md",
            "folder.md/not-a-note.txt",
            ".md",
            "notes.markdown",
            "image.png",
            "Excluded/hidden.md",
            "Kept/Excluded/shown.md",
            ".hidden/secret.md",
            ".obsidian/workspace.md",
            ".trash/gone.md",
            "d1/d2/d3/d4/deep.md",
            "node_modules/pkg/README.md",
            "app/target/out.md",
            "app/build.log",
            "app/docs/keep.md",
            "app/docs/draft.md",
            "app/docs/sub/draft.md",
            "generated/api.md",
        ] {
            dir.write(rel, "y");
        }
        fs::create_dir_all(dir.path().join("Empty")).expect("empty folder");
        dir.write(".gitignore", "*.log\n/generated/\n");
        dir.write("app/.gitignore", "docs/*.md\n!docs/keep.md\n");
        dir.write("app/docs/sub/.gitignore", "!draft.md\n");

        let options = [
            ScanOptions::default(),
            ScanOptions {
                include_hidden: true,
                include_attachments: false,
                excluded_folders: vec!["Excluded".into()],
                max_depth: 2,
                skip_project_folders: false,
            },
            ScanOptions {
                skip_project_folders: true,
                ..ScanOptions::default()
            },
            ScanOptions {
                include_hidden: true,
                skip_project_folders: true,
                ..ScanOptions::default()
            },
            ScanOptions {
                max_depth: 0,
                ..ScanOptions::default()
            },
        ];
        for options in &options {
            let scan = Vault::from_path(dir.path()).scan(options).expect("scan");
            let reference = reference_scan(dir.path(), options);
            assert_eq!(scan.notes, reference.notes, "{options:?}");
            assert_eq!(scan.folders, reference.folders, "{options:?}");
            assert_eq!(scan.attachments, reference.attachments, "{options:?}");
        }
        let scan = Vault::from_path(dir.path())
            .scan(&ScanOptions::default())
            .expect("scan");
        assert_eq!(scan.notes.len(), 700 + 10 + 6);
        let skipping = Vault::from_path(dir.path())
            .scan(&ScanOptions {
                skip_project_folders: true,
                ..ScanOptions::default()
            })
            .expect("scan");
        let rels: Vec<&str> = skipping.notes.iter().map(|n| n.rel.as_str()).collect();
        assert!(rels.contains(&"app/docs/keep.md"));
        assert!(rels.contains(&"app/docs/sub/draft.md"));
        for gone in [
            "node_modules/pkg/README.md",
            "app/target/out.md",
            "app/docs/draft.md",
            "generated/api.md",
        ] {
            assert!(!rels.contains(&gone), "{gone}");
        }
        assert!(!skipping.attachments.iter().any(|a| a == "app/build.log"));
        assert!(scan.notes.windows(2).all(|pair| pair[0].rel < pair[1].rel));
        assert!(scan.notes.iter().all(|note| note.stamp.written > 0));
    }

    #[test]
    fn missing_obsidian_json_is_not_an_error() {
        assert!(load_from(Path::new("/nonexistent/obsidian")).is_none());
    }
}
