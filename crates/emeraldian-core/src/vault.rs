//! Obsidian vault discovery and scanning.
//!
//! A vault is just a directory of Markdown files, which is what makes it
//! possible to open one from a terminal at all. Obsidian additionally keeps a
//! registry of the vaults it knows about in `obsidian.json`, so emeraldian
//! can offer the same vault list the desktop app shows rather than asking the
//! user to type a path.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Deserialize;

use crate::gitignore::Gitignore;

use crate::note::{self, NoteMeta};

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

        let mut scan = Scan::default();
        let mut ignores = Vec::new();
        walk(&self.path, &self.path, options, &mut ignores, &mut scan, 0)?;
        scan.notes.sort_by(|a, b| a.rel.cmp(&b.rel));
        scan.folders.sort();
        scan.attachments.sort();
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

/// The `.gitignore` files met on the way down to a folder, nearest last.
type Ignores = Vec<Gitignore>;

fn walk(
    root: &Path,
    dir: &Path,
    options: &ScanOptions,
    ignores: &mut Ignores,
    out: &mut Scan,
    depth: usize,
) -> Result<(), std::io::Error> {
    if depth > options.max_depth {
        return Ok(());
    }

    // Read before the folder's entries, since its rules apply to them.
    let pushed = options.skip_project_folders && push_gitignore(dir, ignores);
    let result = walk_entries(root, dir, options, ignores, out, depth);
    if pushed {
        ignores.pop();
    }
    result
}

fn walk_entries(
    root: &Path,
    dir: &Path,
    options: &ScanOptions,
    ignores: &mut Ignores,
    out: &mut Scan,
    depth: usize,
) -> Result<(), std::io::Error> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        // A directory we can't read (permissions, a broken mount) shouldn't
        // abort the whole scan — the rest of the vault is still usable.
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => return Ok(()),
        Err(err) => return Err(err),
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

        let Some(rel) = note::relative_path(root, &path) else {
            continue;
        };

        if options.skip_project_folders
            && ((is_dir && PROJECT_FOLDERS.contains(&name)) || is_ignored(ignores, &path, is_dir))
        {
            continue;
        }

        if is_dir {
            if options.excluded_folders.iter().any(|f| f == &rel) {
                continue;
            }
            out.folders.push(rel);
            walk(root, &path, options, ignores, out, depth + 1)?;
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
            // Windows and most Linux filesystems report a birth time; older
            // ext4 and some network mounts don't. Falling back to `modified`
            // keeps created-order sorting sane instead of piling every note
            // at the epoch.
            let created = metadata
                .as_ref()
                .and_then(|m| stamp(m.created()))
                .unwrap_or(modified);
            let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);

            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(name)
                .to_string();

            out.notes.push(NoteMeta {
                path,
                rel,
                title: stem.clone(),
                stem,
                modified,
                created,
                size,
            });
        } else if options.include_attachments {
            out.attachments.push(rel);
        }
    }

    Ok(())
}

/// Adds the folder's `.gitignore` to the list, if it has one that says
/// anything. Returns whether it did.
fn push_gitignore(dir: &Path, ignores: &mut Ignores) -> bool {
    match Gitignore::read(dir) {
        Some(rules) => {
            ignores.push(rules);
            true
        }
        None => false,
    }
}

/// Whether the `.gitignore` files above a path leave it out. The nearest file
/// with an opinion decides, so a deeper `!keep.md` can bring back what a
/// shallower one ignored.
fn is_ignored(ignores: &Ignores, path: &Path, is_dir: bool) -> bool {
    ignores
        .iter()
        .rev()
        .find_map(|rules| rules.decide(path, is_dir))
        .unwrap_or(false)
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
    fn missing_obsidian_json_is_not_an_error() {
        assert!(load_from(Path::new("/nonexistent/obsidian")).is_none());
    }
}
