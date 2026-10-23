//! `emeraldian mem`: memory that outlasts an agent's context, kept as notes.
//!
//! An agent that stops — out of context, interrupted, or finished — leaves the
//! next one, of whatever make, to work out again what it knew. These commands
//! give agents one place to keep it, laid out as notes a person can read, link
//! and edit like any other:
//!
//! ```text
//! Agents/                              `[memory] folder` in config.toml
//!   emeraldian/                        one folder per project
//!     emeraldian.md                    the project note, shown first on every start
//!     Sessions/2026-10-22 1830 claude.md
//!     Memory/Release process.md
//! ```
//!
//! A session links to its project note and to the session before it, and
//! everything remembered links to the project note, so in the graph a project
//! is a cluster with its history running through it as a chain.
//!
//! What each command prints is shaped by what an agent pays for: every call,
//! and every line it reads back, is context it no longer has for the work.
//! So opening a session also says where the last one left off, a log line is
//! appended rather than the note rewritten, and every summary is capped.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use emeraldian_core::edit;
use emeraldian_core::links;
use emeraldian_core::note::{self, FmValue};
use emeraldian_core::ops::sanitize_name;
use serde_json::{Value, json};

use super::retrieve;
use super::{
    Args, Ctx, Failure, Located, Output, Result, append_file, cap, clip, display, fail, write_file,
};
use crate::clock::{self, Utc};

/// `emeraldian help mem`.
pub(super) const GUIDE: &str = "\
Memory for coding agents, kept in the vault as ordinary notes so that the next
agent, of any make, carries on where the last one stopped:

  emeraldian mem start \"what you are about to do\"   once, first; prints where things stand
  emeraldian mem recall \"a question\" --budget 800   before digging in: what is known, in ~800 tokens
  emeraldian mem log \"what just happened\"           as you go: findings, decisions, dead ends
  emeraldian mem remember \"Title\" \"what to keep\"    anything worth knowing next time
  emeraldian mem done \"part of a task's text\"       tick an open task you have finished
  emeraldian mem end \"where it stands\" --next \"what is left\"    before stopping

`mem recall` finds by words and links, and by meaning too after one run of
`emeraldian index` with Ollama running. `emeraldian recall` asks the whole
vault the same way.

Under the [memory] folder (Agents/ unless config.toml says otherwise) each
project has a folder holding:
  <project>.md   the project note: shown first on every start, so keep it short
  Sessions/      a note per session: its task, a timestamped log and a handoff
  Memory/        a note per thing remembered

The project is --project, else $EMERALDIAN_PROJECT, else the name of the git
repository you are in (a worktree counts as its repository), else the current
folder's name. The agent is --agent or $EMERALDIAN_AGENT.

`log`, `done` and `end` find your session by themselves: the latest one still
open that was started in the working tree you are in, and by the agent you
name, if you name one. Agents working side by side in one folder should each
pass their own --agent.
`mem end` records the branch, the commit and uncommitted files by itself.";

/// How many recent sessions a resume describes.
const SESSIONS_SHOWN: usize = 3;
/// Log lines shown from the latest session: more when it left no handoff,
/// since then the log is all there is to go on.
const LOG_WITH_HANDOFF: usize = 3;
const LOG_WITHOUT_HANDOFF: usize = 8;
/// Sessions searched for open tasks. Older ones' unticked steps have, in
/// practice, been done or dropped without anyone saying so.
const SESSIONS_FOR_TASKS: usize = 10;
const TASKS_SHOWN: usize = 10;
const MEMORIES_SHOWN: usize = 20;
/// The most of the project note or a handoff a resume prints, in characters.
const NOTE_CHARS: usize = 1_500;
const HANDOFF_CHARS: usize = 1_200;
/// An earlier session gets a line, not a page.
const EARLIER_CHARS: usize = 200;
/// The most changed notes a recall embeds before answering. A project's notes
/// change a few at a time, a session's log most of all, so this is usually
/// all of them, and a recall is never far behind what was written.
const RECALL_FRESHEN: usize = 100;
/// The most of one log entry or task a resume prints. Whole pages are logged
/// now and then, and the next agent needs the gist, not the page.
const ENTRY_CHARS: usize = 200;

/// The project and agent a command is about, and where their notes live.
struct Scope {
    project: String,
    /// What a new session is named for: the agent given, or `agent`.
    agent: String,
    /// Whether an agent was named at all. Only then are sessions told apart by
    /// who opened them: an agent that names itself when it starts a session
    /// and not afterwards still finds the session it started.
    agent_given: bool,
    /// The project's folder, vault-relative.
    dir: String,
    root: PathBuf,
}

impl Scope {
    fn new(ctx: &mut Ctx, args: &Args) -> Result<Self> {
        let project = name_from(args.value("project"), "EMERALDIAN_PROJECT")
            .or_else(|| {
                let here = std::env::current_dir().ok()?;
                repository_name(&here).or_else(|| folder_name(&here))
            })
            .unwrap_or_else(|| "default".to_string());
        let given = name_from(args.value("agent"), "EMERALDIAN_AGENT");
        let agent_given = given.is_some();
        let agent = given.unwrap_or_else(|| "agent".into());

        let folder = ctx
            .config
            .memory
            .folder
            .trim()
            .trim_matches('/')
            .to_string();
        let dir = if folder.is_empty() {
            project.clone()
        } else {
            format!("{folder}/{project}")
        };
        // The folder comes from the config file, so it gets the same check as
        // a name typed on the command line.
        ctx.vault()?.resolve_path(&dir)?;
        Ok(Self {
            project,
            agent,
            agent_given,
            dir,
            root: ctx.root()?,
        })
    }

    fn rel(&self, inside: &str) -> String {
        format!("{}/{inside}", self.dir)
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn note_rel(&self) -> String {
        self.rel(&format!("{}.md", self.project))
    }

    /// A wikilink to a note of this project, by its full path so that no
    /// other note of the same name can catch it, shown by its short name.
    fn link(&self, rel: &str) -> String {
        let target = display(rel);
        match target.rsplit_once('/') {
            Some((_, short)) => format!("[[{target}|{short}]]"),
            None => format!("[[{target}]]"),
        }
    }
}

/// A name given as an option, else in an environment variable, made safe to
/// use as a folder or file name.
fn name_from(given: Option<&str>, variable: &str) -> Option<String> {
    given
        .map(str::to_string)
        .or_else(|| std::env::var(variable).ok())
        .filter(|name| !name.trim().is_empty())
        .map(|name| sanitize_name(&name))
}

fn folder_name(dir: &Path) -> Option<String> {
    dir.file_name()
        .map(|name| sanitize_name(&name.to_string_lossy()))
}

/// The name of the git repository `dir` is in.
///
/// A worktree is named for the repository it belongs to: an agent working in
/// `../emeraldian-fix` is still working on emeraldian, and its sessions belong
/// with the others. Found by walking up to `.git` rather than asking git, so it
/// costs nothing and works where git isn't installed.
pub(super) fn repository_name(dir: &Path) -> Option<String> {
    for folder in dir.ancestors() {
        let git = folder.join(".git");
        if git.is_dir() {
            return folder_name(folder);
        }
        if git.is_file() {
            // `gitdir: /code/emeraldian/.git/worktrees/emeraldian-fix`
            let text = fs::read_to_string(&git).ok()?;
            let target = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
            let target = folder.join(target);
            let in_worktrees = target.parent().filter(|p| p.ends_with("worktrees"));
            let main = in_worktrees
                .and_then(Path::parent)
                .filter(|git| git.ends_with(".git"))
                .and_then(Path::parent);
            // Anything else with a `.git` file, a submodule, is its own project.
            return folder_name(main.unwrap_or(folder));
        }
    }
    None
}

/// Where the agent is working: the top of the git working tree it is in, so
/// moving between the tree's folders doesn't change it, or else the folder
/// itself.
///
/// Two agents of the same name working on a project at once are almost always
/// in different worktrees, and this is what tells their sessions apart.
fn workdir() -> Option<String> {
    let here = std::env::current_dir().ok()?;
    let top = here
        .ancestors()
        .find(|folder| folder.join(".git").exists())
        .unwrap_or(&here);
    Some(top.display().to_string())
}

/// One git command's output, or `None` if it fails or takes longer than
/// [`GIT_PATIENCE`].
///
/// Read on a thread of its own, so the wait has a limit: a git that hangs, or
/// a background process it starts that keeps the output open, can't keep the
/// command from finishing.
fn ask_git(here: &Path, args: &[&str]) -> Option<String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(here)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        let _ = send.send(out);
    });

    let Ok(out) = receive.recv_timeout(GIT_PATIENCE) else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    // The output is complete, so git has finished or is about to.
    let status = child.wait().ok()?;
    status.success().then(|| out.trim_end().to_string())
}

/// Where the repository is, what is checked out, and what isn't committed.
struct Git {
    branch: String,
    commit: String,
    dirty: Vec<String>,
}

/// How long git is given to answer one question.
///
/// `git status` in a very large repository can take a second or two; past
/// this, something is wrong with the repository or the machine, and an agent
/// left waiting on it is worse off than a session that records less.
const GIT_PATIENCE: Duration = Duration::from_secs(3);

/// Asks git about the repository the agent is working in, or `None` outside
/// one. Never fails a command: git missing, failing, or slow to answer only
/// means the session records less.
fn git(dirty: bool) -> Option<Git> {
    let here = std::env::current_dir().ok()?;
    repository_name(&here)?;
    let run = |args: &[&str]| ask_git(&here, args);
    let branch = run(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    let commit = run(&["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let dirty = if dirty {
        run(&["status", "--porcelain"])
            .map(|out| out.lines().map(|l| l.trim().to_string()).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    Some(Git {
        branch,
        commit,
        dirty,
    })
}

// ---- reading what is there ----------------------------------------------------

/// A note in the project's folder, without its content.
#[derive(Debug, Clone)]
struct File {
    rel: String,
    stem: String,
    modified: u64,
}

/// The notes directly in one of the project's folders, most recently changed
/// first.
///
/// Except sessions, which come most recently *started* first: their names
/// begin with when they started. Ticking a task in last week's session
/// changes the file, but it doesn't make that the session to pick up from.
fn files(scope: &Scope, inside: &str) -> Vec<File> {
    let rel_dir = if inside.is_empty() {
        scope.dir.clone()
    } else {
        scope.rel(inside)
    };
    let Ok(entries) = fs::read_dir(scope.path(&rel_dir)) else {
        return Vec::new();
    };
    let mut files: Vec<File> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let stem = name.strip_suffix(".md")?.to_string();
            if stem.starts_with('.') || !entry.file_type().ok()?.is_file() {
                return None;
            }
            let modified = entry
                .metadata()
                .ok()?
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs();
            Some(File {
                rel: format!("{rel_dir}/{name}"),
                stem,
                modified,
            })
        })
        .collect();
    if inside == "Sessions" {
        // `2026-10-22 1830`: fifteen characters that sort as the time they say.
        let started = |f: &File| f.stem.chars().take(15).collect::<String>();
        files.sort_by(|a, b| {
            started(b)
                .cmp(&started(a))
                .then_with(|| b.modified.cmp(&a.modified))
        });
    } else {
        files.sort_by(|a, b| {
            b.modified
                .cmp(&a.modified)
                .then_with(|| a.stem.cmp(&b.stem))
        });
    }
    files
}

/// A session note, read.
#[derive(Debug, Clone)]
struct Session {
    file: File,
    content: String,
    agent: String,
    status: String,
    task: String,
    branch: Option<String>,
    commit: Option<String>,
    workdir: Option<String>,
}

impl Session {
    fn load(scope: &Scope, file: File) -> Option<Self> {
        let content = fs::read_to_string(scope.path(&file.rel)).ok()?;
        let (yaml, body, _) = note::split_frontmatter(&content);
        let props = yaml.map(note::parse_frontmatter).unwrap_or_default();
        let prop = |key: &str| {
            props
                .get(key)
                .map(FmValue::as_scalar)
                .filter(|v| !v.is_empty())
        };
        let task = links::extract_headings(body)
            .into_iter()
            .find(|h| h.level == 1)
            .map_or_else(|| file.stem.clone(), |h| h.text);
        Some(Self {
            agent: prop("agent").unwrap_or_default(),
            status: prop("status").unwrap_or_else(|| "active".into()),
            branch: prop("branch"),
            commit: prop("commit"),
            workdir: prop("workdir"),
            task,
            content,
            file,
        })
    }

    fn is_open(&self) -> bool {
        !self.status.eq_ignore_ascii_case("done")
    }

    /// The lines of a section, without its heading, trimmed of blank edges.
    fn section(&self, heading: &str) -> Option<String> {
        let (start, end) = edit::section(&self.content, heading)?;
        let text = self
            .content
            .lines()
            .skip(start + 1)
            .take(end - start - 1)
            .collect::<Vec<_>>()
            .join("\n");
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    fn handoff(&self) -> Option<String> {
        self.section("## Handoff")
    }

    /// The log's entries, each one line however many it was written over.
    fn log(&self) -> Vec<String> {
        let Some(log) = self.section("## Log") else {
            return Vec::new();
        };
        let mut entries: Vec<String> = Vec::new();
        for line in log.lines() {
            if let Some(entry) = line.strip_prefix("- ") {
                entries.push(entry.to_string());
            } else if let Some(last) = entries.last_mut()
                && !line.trim().is_empty()
            {
                last.push(' ');
                last.push_str(line.trim());
            }
        }
        entries
    }

    /// `2026-10-22 1830 claude, active, 12m ago, on main at 047ec85`.
    fn header(&self, now: u64) -> String {
        let mut text = format!(
            "{}, {}, {}",
            self.file.stem,
            self.status,
            clock::ago(self.file.modified, now)
        );
        if let Some(branch) = &self.branch {
            text.push_str(&format!(", on {branch}"));
            if let Some(commit) = &self.commit {
                text.push_str(&format!(" at {commit}"));
            }
        }
        text
    }

    fn json(&self, now: u64) -> Value {
        json!({
            "note": self.file.rel,
            "name": self.file.stem,
            "agent": self.agent,
            "status": self.status,
            "task": self.task,
            "updated": Utc::at(self.file.modified).iso(),
            "age": clock::ago(self.file.modified, now),
            "branch": self.branch,
            "commit": self.commit,
            "handoff": self.handoff(),
            "log": self.log(),
        })
    }
}

/// The session `--session` names, else the latest still open that was started
/// where this agent is working, else the latest still open at all; of this
/// agent's own, when it gave its name.
fn current(scope: &Scope, args: &Args) -> Result<Session> {
    let sessions = files(scope, "Sessions");
    if let Some(name) = args.value("session") {
        let name = display(name.trim());
        let wanted = name.rsplit('/').next().unwrap_or(name);
        return sessions
            .into_iter()
            .find(|f| f.stem == wanted)
            .and_then(|file| Session::load(scope, file))
            .ok_or_else(|| {
                Failure::Failed(format!("{} has no session `{wanted}`", scope.project))
            });
    }
    let here = workdir();
    let mut fallback = None;
    for session in sessions
        .into_iter()
        .filter_map(|file| Session::load(scope, file))
    {
        if !session.is_open()
            || scope.agent_given && !session.agent.eq_ignore_ascii_case(&scope.agent)
        {
            continue;
        }
        if here.is_some() && session.workdir == here {
            return Ok(session);
        }
        fallback.get_or_insert(session);
    }
    fallback.ok_or_else(|| {
        let whose = if scope.agent_given {
            format!(" for {}", scope.agent)
        } else {
            String::new()
        };
        Failure::Failed(format!(
            "{} has no open session{whose}; begin one with `emeraldian mem start <TASK>`",
            scope.project
        ))
    })
}

/// A task still to do, and where it is.
struct OpenTask {
    rel: String,
    stem: String,
    line: usize,
    text: String,
}

/// The project's open tasks: in its note, in what is remembered, and in its
/// latest sessions, newest first.
fn open_tasks(scope: &Scope, sessions: usize) -> Vec<OpenTask> {
    let mut notes: Vec<File> = Vec::new();
    notes.extend(
        files(scope, "")
            .into_iter()
            .filter(|f| f.rel == scope.note_rel()),
    );
    notes.extend(files(scope, "Sessions").into_iter().take(sessions));
    notes.extend(files(scope, "Memory"));
    notes.sort_by_key(|file| std::cmp::Reverse(file.modified));

    let mut tasks = Vec::new();
    for file in notes {
        let Ok(content) = fs::read_to_string(scope.path(&file.rel)) else {
            continue;
        };
        let (_, body, offset) = note::split_frontmatter(&content);
        for task in links::extract_tasks(body) {
            if !task.done && !task.text.is_empty() {
                tasks.push(OpenTask {
                    rel: file.rel.clone(),
                    stem: file.stem.clone(),
                    line: offset + task.line,
                    text: task.text,
                });
            }
        }
    }
    tasks
}

/// The project note's text, without its frontmatter or title, or `None` while
/// it still says only what it was created with.
fn project_note(scope: &Scope) -> Option<String> {
    let content = fs::read_to_string(scope.path(&scope.note_rel())).ok()?;
    let (_, body, _) = note::split_frontmatter(&content);
    let body = body.trim_start();
    let body = match body.strip_prefix("# ") {
        Some(rest) => rest.split_once('\n').map_or("", |(_, rest)| rest),
        None => body,
    };
    let body = body.trim();
    (!body.is_empty()).then(|| body.to_string())
}

// ---- resume --------------------------------------------------------------------

/// Where a project stands, as text and as JSON: the project note, the latest
/// sessions, open tasks and what has been remembered.
fn brief(scope: &Scope, now: u64) -> (String, Value) {
    let session_files = files(scope, "Sessions");
    let memories = files(scope, "Memory");
    let note_exists = scope.path(&scope.note_rel()).is_file();

    if session_files.is_empty() && memories.is_empty() && !note_exists {
        let text = format!(
            "{}: nothing remembered yet (in {}). `emeraldian mem start <TASK>` begins.",
            scope.project, scope.dir
        );
        return (
            text,
            json!({ "project": scope.project, "folder": scope.dir, "sessions": [], "tasks": [], "memories": [] }),
        );
    }

    let mut out = vec![format!(
        "{}: {} session{}, {} memor{}, in {}",
        scope.project,
        session_files.len(),
        if session_files.len() == 1 { "" } else { "s" },
        memories.len(),
        if memories.len() == 1 { "y" } else { "ies" },
        scope.dir
    )];

    let project = project_note(scope);
    match &project {
        Some(text) => {
            out.push(String::new());
            out.push("Project note:".to_string());
            out.push(clip(text, NOTE_CHARS));
        }
        None => {
            out.push(String::new());
            out.push(format!(
                "Project note: empty. Write what every agent should know first: \
                 `emeraldian write {} -`",
                display(&scope.note_rel())
            ));
        }
    }

    let sessions: Vec<Session> = session_files
        .iter()
        .take(SESSIONS_SHOWN)
        .filter_map(|file| Session::load(scope, file.clone()))
        .collect();
    if let Some(latest) = sessions.first() {
        out.push(String::new());
        out.push(format!("Last session: {}", latest.header(now)));
        out.push(format!("Task: {}", latest.task));
        let handoff = latest.handoff();
        if let Some(handoff) = &handoff {
            out.push(format!("Handoff:\n{}", clip(handoff, HANDOFF_CHARS)));
        }
        let log = latest.log();
        if !log.is_empty() {
            let keep = if handoff.is_some() {
                LOG_WITH_HANDOFF
            } else {
                LOG_WITHOUT_HANDOFF
            };
            let from = log.len().saturating_sub(keep);
            out.push(if from == 0 {
                "Log:".to_string()
            } else {
                format!("Log (last {} of {}):", log.len() - from, log.len())
            });
            out.extend(
                log[from..]
                    .iter()
                    .map(|entry| format!("- {}", clip(entry, ENTRY_CHARS))),
            );
        }
    }
    if sessions.len() > 1 {
        out.push(String::new());
        out.push("Earlier:".to_string());
        for session in &sessions[1..] {
            // The summary alone: its steps are listed under open tasks, and
            // its git details are history by now.
            let gist = session
                .handoff()
                .map(|h| {
                    let summary = h.split("\n\n").next().unwrap_or_default();
                    format!(" Handoff: {}", summary.replace('\n', " "))
                })
                .or_else(|| session.log().last().map(|l| format!(" Last log: {l}")))
                .unwrap_or_default();
            out.push(clip(
                &format!("- {}: {}.{gist}", session.header(now), session.task),
                EARLIER_CHARS,
            ));
        }
    }

    let mut tasks = open_tasks(scope, SESSIONS_FOR_TASKS);
    let total_tasks = tasks.len();
    let cut = cap(&mut tasks, TASKS_SHOWN);
    if !tasks.is_empty() {
        out.push(String::new());
        out.push(format!("Open tasks ({total_tasks}):"));
        out.extend(
            tasks
                .iter()
                .map(|t| format!("- {} ({})", clip(&t.text, ENTRY_CHARS), t.stem)),
        );
        if cut > 0 {
            out.push(format!("… {cut} more: `emeraldian tasks {}`", scope.dir));
        }
    }

    if !memories.is_empty() {
        let shown: Vec<&str> = memories
            .iter()
            .take(MEMORIES_SHOWN)
            .map(|m| m.stem.as_str())
            .collect();
        let rest = memories.len() - shown.len();
        out.push(String::new());
        let mut line = format!("Memories ({}): {}", memories.len(), shown.join("; "));
        if rest > 0 {
            line.push_str(&format!("; … {rest} more"));
        }
        out.push(line);
    }

    let json = json!({
        "project": scope.project,
        "folder": scope.dir,
        "note": project,
        "sessions": sessions.iter().map(|s| s.json(now)).collect::<Vec<_>>(),
        "sessions_total": session_files.len(),
        "tasks": tasks.iter().map(|t| json!({ "note": t.rel, "line": t.line + 1, "text": t.text })).collect::<Vec<_>>(),
        "tasks_total": total_tasks,
        "memories": memories.iter().map(|m| json!({ "note": m.rel, "title": m.stem, "updated": Utc::at(m.modified).iso() })).collect::<Vec<_>>(),
    });
    (out.join("\n"), json)
}

pub(super) fn resume(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let scope = Scope::new(ctx, args)?;
    let (text, json) = brief(&scope, clock::now());
    Ok(Output::new(text, json))
}

// ---- sessions -------------------------------------------------------------------

/// Creates a note that must not exist yet, returning `false` if it does.
fn create_new(path: &Path, content: &str) -> Result<bool> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(content.as_bytes())?;
            Ok(true)
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(err.into()),
    }
}

/// Frontmatter built one property at a time, so each value is written the way
/// YAML will read it back.
fn frontmatter(properties: &[(&str, Option<FmValue>)]) -> String {
    properties
        .iter()
        .fold(String::new(), |content, (key, value)| match value {
            Some(value) => edit::set_property(&content, key, Some(value)),
            None => content,
        })
}

fn text(value: impl Into<String>) -> Option<FmValue> {
    Some(FmValue::Scalar(value.into()))
}

pub(super) fn start(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let scope = Scope::new(ctx, args)?;
    let task = args
        .text(0)?
        .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|t| !t.is_empty())
        .ok_or_else(|| Failure::Usage("mem start needs the task, in a few words".into()))?;
    let now = clock::now();

    // Read before anything is written, so the latest session described is the
    // one before this.
    let (brief_text, brief_json) = brief(&scope, now);
    let previous = files(&scope, "Sessions").into_iter().next();

    let note_rel = scope.note_rel();
    let content = format!(
        "{}# {}\n",
        frontmatter(&[("type", text("project"))]),
        scope.project
    );
    create_new(&scope.path(&note_rel), &content)?;

    let git = git(false);
    let mut properties = vec![
        ("type", text("session")),
        ("project", text(&scope.project)),
        ("agent", text(&scope.agent)),
        ("status", text("active")),
        ("started", text(Utc::at(now).iso())),
    ];
    if let Some(git) = &git {
        properties.push(("branch", text(&git.branch)));
        properties.push(("commit", text(&git.commit)));
    }
    if let Some(here) = workdir() {
        properties.push(("workdir", text(here)));
    }

    let mut links = format!("Project: {}", scope.link(&note_rel));
    if let Some(previous) = &previous {
        links.push_str(&format!(" · Previous: {}", scope.link(&previous.rel)));
    }
    let body = format!(
        "{}# {task}\n\n{links}\n\n## Log\n\n",
        frontmatter(&properties)
    );

    let stamp = Utc::at(now).format("%Y-%m-%d %H%M");
    let mut name = format!("{stamp} {}", scope.agent);
    let mut n = 2;
    let rel = loop {
        let rel = scope.rel(&format!("Sessions/{name}.md"));
        if create_new(&scope.path(&rel), &body)? {
            break rel;
        }
        name = format!("{stamp} {} {n}", scope.agent);
        n += 1;
    };

    let text = format!("{brief_text}\n\nStarted session {name}.");
    Ok(Output::new(
        text,
        json!({ "session": rel, "resume": brief_json }),
    ))
}

/// A log entry: the time, then the text, its later lines indented so the
/// whole of it stays one item of the list.
///
/// The text is defused first, so a heading or an open code fence in it can't
/// cut the log short. A fence that opens the text goes on its own line: after
/// the time it would not read as a fence, and its closing line would then
/// open one instead.
fn entry(text: &str, now: u64) -> String {
    let text = edit::defuse(text.trim());
    let mut lines = text.lines();
    let mut entry = format!("- {}", Utc::at(now).format("%H:%M"));
    if let Some(first) = lines.next().map(str::trim) {
        if first.starts_with("```") || first.starts_with("~~~") {
            entry.push_str("\n  ");
        } else {
            entry.push(' ');
        }
        entry.push_str(first);
    }
    for line in lines {
        entry.push('\n');
        if !line.trim().is_empty() {
            entry.push_str("  ");
            entry.push_str(line.trim_end());
        }
    }
    entry
}

/// Adds an entry to a session's log.
///
/// The log is the last section of a session note, so an entry is normally
/// appended to the file in one write, which can't lose an entry another
/// process adds at the same moment. Only if someone has put something after
/// the log is the note rewritten to slot the entry in.
fn add_to_log(scope: &Scope, session: &Session, entry: &str) -> Result<()> {
    let path = scope.path(&session.file.rel);
    let total = session.content.lines().count();
    match edit::section(&session.content, "## Log") {
        Some((_, end)) if end == total => append_file(&path, entry),
        Some(_) => {
            let updated = edit::append_to_section(&session.content, "## Log", entry)
                .unwrap_or_else(|| session.content.clone());
            write_file(&path, &updated)
        }
        None => write_file(
            &path,
            &edit::write_section(&session.content, "## Log", entry, None),
        ),
    }
}

/// Logs to this agent's open session if it has one, for a command whose main
/// work is elsewhere: what was remembered, what was ticked. Saying so in the
/// session is what links the session to it in the graph.
fn log_if_open(scope: &Scope, args: &Args, text: &str) {
    if let Ok(session) = current(scope, args) {
        let _ = add_to_log(scope, &session, &entry(text, clock::now()));
    }
}

pub(super) fn log(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let scope = Scope::new(ctx, args)?;
    let text = args.text(0)?.unwrap_or_default();
    if text.trim().is_empty() {
        return Err(Failure::Usage("mem log needs something to say".into()));
    }
    let session = current(&scope, args)?;
    let line = entry(&text, clock::now());
    add_to_log(&scope, &session, &line)?;
    Ok(Output::new(
        format!("logged to {}", session.file.stem),
        json!({ "session": session.file.rel, "entry": line }),
    ))
}

pub(super) fn end(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let scope = Scope::new(ctx, args)?;
    let summary = args.text(0)?.unwrap_or_default();
    let summary = summary.trim();
    if summary.is_empty() {
        return Err(Failure::Usage(
            "mem end needs a summary of where things stand".into(),
        ));
    }
    let status = args.value("status").unwrap_or("done").trim().to_lowercase();
    if !matches!(status.as_str(), "done" | "paused" | "blocked") {
        return Err(Failure::Usage(format!(
            "--status takes done, paused or blocked, not `{status}`"
        )));
    }
    let session = current(&scope, args)?;

    let mut handoff = edit::defuse(summary);
    let next: Vec<String> = args
        .values("next")
        .into_iter()
        .map(|step| {
            let step = step.trim();
            let step = step
                .strip_prefix("- [ ]")
                .or_else(|| step.strip_prefix("- "))
                .unwrap_or(step);
            format!("- [ ] {}", step.trim())
        })
        .collect();
    if !next.is_empty() {
        handoff.push_str(&format!("\n\nNext:\n{}", next.join("\n")));
    }
    if let Some(git) = git(true) {
        handoff.push_str(&format!("\n\nGit: {} at {}", git.branch, git.commit));
        if !git.dirty.is_empty() {
            let mut dirty = git.dirty.clone();
            let more = cap(&mut dirty, 15);
            handoff.push_str(&format!(", uncommitted: {}", dirty.join(", ")));
            if more > 0 {
                handoff.push_str(&format!(" and {more} more"));
            }
        }
    }

    let mut content = edit::write_section(&session.content, "## Handoff", &handoff, Some("## Log"));
    content = edit::set_property(&content, "status", text(&status).as_ref());
    content = edit::set_property(&content, "ended", text(Utc::now().iso()).as_ref());
    write_file(&scope.path(&session.file.rel), &content)?;

    Ok(Output::new(
        format!("ended {} ({status})", session.file.stem),
        json!({ "session": session.file.rel, "status": status, "handoff": handoff }),
    ))
}

pub(super) fn sessions(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let scope = Scope::new(ctx, args)?;
    let limit = args.limit(10)?;
    let mut all = files(&scope, "Sessions");
    let cut = cap(&mut all, limit);
    let now = clock::now();

    let sessions: Vec<Session> = all
        .into_iter()
        .filter_map(|file| Session::load(&scope, file))
        .collect();
    let text = if sessions.is_empty() {
        format!("{} has no sessions yet", scope.project)
    } else {
        super::lines(
            sessions
                .iter()
                .map(|s| format!("{}: {}", s.header(now), s.task))
                .collect(),
            cut,
        )
    };
    Ok(Output::new(
        text,
        json!({ "sessions": sessions.iter().map(|s| s.json(now)).collect::<Vec<_>>(), "more": cut }),
    ))
}

// ---- memories -------------------------------------------------------------------

pub(super) fn remember(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let scope = Scope::new(ctx, args)?;
    let title = sanitize_name(args.at(0));
    let body = args.text(1)?.unwrap_or_default();
    let body = body.trim();
    if body.is_empty() {
        return Err(Failure::Usage(
            "mem remember needs a title and what to remember".into(),
        ));
    }
    let tags: Vec<String> = args
        .values("tag")
        .into_iter()
        .map(|t| t.trim().trim_start_matches('#').to_string())
        .filter(|t| !t.is_empty())
        .collect();

    // Related notes by their full names where they exist, so the links hold
    // whatever else in the vault shares a name; as typed where they don't yet.
    let mut related = Vec::new();
    for name in args.values("link") {
        match ctx.locate(name) {
            Ok(found) => related.push(scope.link(&found.rel)),
            Err(_) => related.push(format!("[[{}]]", display(name.trim()))),
        }
    }

    let rel = scope.rel(&format!("Memory/{title}.md"));
    let path = scope.path(&rel);
    let now = Utc::now().iso();
    let existing = fs::read_to_string(&path).ok();
    let created = existing.is_none();

    let content = match existing {
        Some(current) if !args.flag("replace") => {
            let mut content = current.trim_end().to_string();
            content.push_str("\n\n");
            content.push_str(body);
            content.push('\n');
            if !related.is_empty() {
                content.push_str(&format!("\nRelated: {}\n", related.join(", ")));
            }
            let content = with_tags(&content, &tags);
            edit::set_property(&content, "updated", text(&now).as_ref())
        }
        current => {
            let kept = current.as_deref().map(|c| {
                let (yaml, _, _) = note::split_frontmatter(c);
                yaml.map(note::parse_frontmatter).unwrap_or_default()
            });
            let since = kept
                .as_ref()
                .and_then(|fm| fm.get("created").map(FmValue::as_scalar))
                .unwrap_or_else(|| now.clone());
            let mut header = vec![
                ("type", text("memory")),
                ("project", text(&scope.project)),
                ("created", text(&since)),
            ];
            if !created {
                header.push(("updated", text(&now)));
            }
            let mut links = format!("Project: {}", scope.link(&scope.note_rel()));
            if !related.is_empty() {
                links.push_str(&format!(" · Related: {}", related.join(", ")));
            }
            let content = format!("{}# {title}\n\n{links}\n\n{body}\n", frontmatter(&header));
            // What it was tagged with still describes it: replacing the text
            // isn't a reason to lose that.
            let mut all = kept.map(|fm| fm.tags()).unwrap_or_default();
            all.extend(tags.iter().cloned());
            with_tags(&content, &all)
        }
    };
    write_file(&path, &content)?;

    log_if_open(&scope, args, &format!("remembered {}", scope.link(&rel)));
    let verb = if created { "remembered" } else { "updated" };
    Ok(Output::new(
        format!("{verb} {}", display(&rel)),
        json!({ "note": rel, "created": created }),
    ))
}

/// Adds tags to a note's `tags` property, keeping the ones it has.
fn with_tags(content: &str, tags: &[String]) -> String {
    if tags.is_empty() {
        return content.to_string();
    }
    let (yaml, _, _) = note::split_frontmatter(content);
    let mut all = yaml
        .map(note::parse_frontmatter)
        .map(|fm| fm.tags())
        .unwrap_or_default();
    for tag in tags {
        if !all.contains(tag) {
            all.push(tag.clone());
        }
    }
    edit::set_property(content, "tags", Some(&FmValue::List(all)))
}

/// Every note in a folder and the folders under it.
fn walk(dir: &Path, rel: &str, out: &mut Vec<File>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let child = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(&entry.path(), &child, out);
        } else if let Some(stem) = name.strip_suffix(".md") {
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            out.push(File {
                rel: child,
                stem: stem.to_string(),
                modified,
            });
        }
    }
}

/// What a note under the [memory] folder `memory` counts for in a recall, by
/// where it is in its project's folder: two for something remembered, in
/// `Memory/`, none for a session's log, in `Sessions/`, and one for anything
/// else, the project note among them. Read from inside the project's folder,
/// as the [memory] folder, or a project, may itself be called `Memory`.
fn kind(rel: &str, memory: &str) -> f32 {
    let in_memory = if memory.is_empty() {
        Some(rel)
    } else {
        rel.strip_prefix(memory)
            .and_then(|rest| rest.strip_prefix('/'))
    };
    let in_project = in_memory
        .and_then(|rest| rest.split_once('/'))
        .map(|(_, rest)| rest);
    match in_project
        .and_then(|rest| rest.split_once('/'))
        .map(|(folder, _)| folder)
    {
        Some("Memory") => 2.0,
        Some("Sessions") => 0.0,
        _ => 1.0,
    }
}

pub(super) fn recall(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let scope = Scope::new(ctx, args)?;
    let limit = args.limit(8)?;
    let budget = retrieve::budget_of(args)?;
    let query = args.at(0);
    if query.trim().is_empty() {
        return Err(Failure::Usage("mem recall needs words to look for".into()));
    }
    // The [memory] folder, and the folder asked, as the disk spells them:
    // the answer names its notes so, and the prior looks them up by name.
    let memory = retrieve::spelt_on_disk(
        &scope.root,
        ctx.config.memory.folder.trim().trim_matches('/'),
    );
    let folder = if args.flag("all") {
        memory.clone()
    } else {
        retrieve::spelt_on_disk(&scope.root, &scope.dir)
    };

    // What was meant to be kept outranks the log it came out of, and the
    // recent outranks the old, a little: a prior, not a filter. Scores are
    // cosines, a few hundredths apart between a note that answers and one
    // that nearly does, so a memory's lead over a log, a little under three
    // hundredths, and a fresh note's over a year-old one, a little over one,
    // settle a near tie without overruling the model.
    let mut notes = Vec::new();
    walk(&scope.path(&folder), &folder, &mut notes);
    let now = clock::now();
    let known: HashMap<String, (f32, u64)> = notes
        .iter()
        .map(|file| (file.rel.clone(), (kind(&file.rel, &memory), file.modified)))
        .collect();
    let prior = |rel: &str| -> f32 {
        known.get(rel).map_or(0.0, |&(kind, modified)| {
            let days = now.saturating_sub(modified) as f32 / 86_400.0;
            0.013 * kind + 0.013 * (-days / 30.0).exp()
        })
    };

    let asked = retrieve::answer(ctx, query, &folder, &prior, limit, RECALL_FRESHEN)?;
    let (text, json) = if asked.answer.notes.is_empty() {
        let hint = if asked.meaning {
            ""
        } else {
            " (recall by meaning is off; `emeraldian index` turns it on)"
        };
        (
            format!("nothing remembered answers `{query}`{hint}"),
            json!({ "results": [] }),
        )
    } else if let Some(budget) = budget {
        retrieve::pack(ctx, &asked.answer, budget)?
    } else {
        let mut text = Vec::new();
        let mut json = Vec::new();
        for hit in &asked.answer.notes {
            let modified = known.get(&hit.rel).map_or(now, |&(_, m)| m);
            text.push(format!(
                "{} ({})",
                display(&hit.rel),
                clock::ago(modified, now)
            ));
            let content = fs::read_to_string(scope.path(&hit.rel)).unwrap_or_default();
            let line = retrieve::shown_line(&hit.rel, &content, hit.start, hit.end, query);
            if let Some((n, line)) = &line {
                text.push(format!("  {n}: {}", clip(line, 160)));
            }
            json.push(json!({
                "note": hit.rel,
                "score": hit.score,
                "updated": Utc::at(modified).iso(),
                "line": line.as_ref().map(|(n, t)| json!({ "line": n, "text": t })),
            }));
        }
        (
            text.join("\n"),
            json!({ "results": json, "semantic": asked.answer.semantic }),
        )
    };
    let output = Output::new(text, json);
    Ok(match asked.notice {
        Some(notice) => output.notice(notice),
        None => output,
    })
}

pub(super) fn done(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let scope = Scope::new(ctx, args)?;
    let wanted = args.at(0).trim().to_lowercase();
    let tasks = open_tasks(&scope, usize::MAX);
    let matching: Vec<&OpenTask> = tasks
        .iter()
        .filter(|t| t.text.to_lowercase().contains(&wanted))
        .collect();

    // A task that is exactly these words comes before any that only contain
    // them, or `Fix CI` could never be ticked beside `Fix CI on Windows`. And a
    // step carried from one handoff to the next is the same task each time, so
    // every copy of it is ticked together.
    let exact: Vec<&OpenTask> = matching
        .iter()
        .copied()
        .filter(|t| t.text.to_lowercase() == wanted)
        .collect();
    let chosen = if exact.is_empty() { matching } else { exact };
    if chosen.is_empty() {
        return fail(format!(
            "no open task in {} mentions `{}`",
            scope.project,
            args.at(0)
        ));
    }
    if !chosen
        .windows(2)
        .all(|pair| pair[0].text.eq_ignore_ascii_case(&pair[1].text))
    {
        let listed: Vec<String> = chosen
            .iter()
            .take(5)
            .map(|t| format!("  {}:{} {}", display(&t.rel), t.line + 1, t.text))
            .collect();
        return fail(format!(
            "{} open tasks match; give more of the one you mean, or tick it with \
             `emeraldian task <NOTE> <LINE>`:\n{}",
            chosen.len(),
            listed.join("\n")
        ));
    }

    // One note at a time, every line in it ticked before it is written.
    let mut notes: Vec<&str> = chosen.iter().map(|t| t.rel.as_str()).collect();
    notes.dedup();
    for rel in notes {
        let located = Located {
            rel: rel.to_string(),
            path: scope.path(rel),
        };
        let mut content = super::read(&located)?;
        for task in chosen.iter().filter(|t| t.rel == rel) {
            content = edit::set_task(&content, task.line, true).ok_or_else(|| {
                Failure::Failed(format!("the task moved in {}; try again", located.name()))
            })?;
        }
        write_file(&located.path, &content)?;
    }

    let task = chosen[0];
    log_if_open(&scope, args, &format!("done: {}", task.text));
    let copies = if chosen.len() > 1 {
        format!(" ({} copies)", chosen.len())
    } else {
        String::new()
    };
    Ok(Output::new(
        format!("ticked `{}` in {}{copies}", task.text, task.stem),
        json!({
            "text": task.text,
            "ticked": chosen.iter().map(|t| json!({ "note": t.rel, "line": t.line + 1 })).collect::<Vec<_>>(),
        }),
    ))
}
