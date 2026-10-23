//! The command line: every operation on a vault, without the interface.
//!
//! `emeraldian <command>` reads and writes a vault the way the app does — the
//! same link resolution, the same renames that rewrite links, the same graph —
//! and prints the result instead of drawing it. It is written as much for
//! scripts and coding agents as for people, and three choices follow from
//! that:
//!
//! - **Output is terse.** Plain lines, no colour, nothing decorative, and every
//!   list capped with a line saying how much was left out, so a broad query
//!   can't pour a vault into an agent's context. `--json` gives the same
//!   results for a program to parse.
//! - **Nothing starts that isn't needed.** No terminal, no themes, no stored
//!   keys. The vault is indexed only for a command that asks about links or
//!   names; reading or writing a note by its path touches that one file.
//! - **A write is all or nothing.** A note is replaced by renaming a finished
//!   copy over it, and appended to in a single write, so the app, Obsidian or a
//!   second agent reading the note never sees half a change.

mod graph;
mod mem;
mod notes;
mod query;
mod retrieve;
mod settings;

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use emeraldian_core::index::VaultIndex;
use emeraldian_core::search;
use emeraldian_core::vault::{Scan, ScanOptions, Vault};
use serde_json::Value;

use crate::config::Config;

/// An option a command accepts.
#[derive(Clone, Copy)]
struct Opt {
    name: &'static str,
    /// What its value is called in the help, or `None` for a switch.
    value: Option<&'static str>,
    about: &'static str,
}

const fn switch(name: &'static str, about: &'static str) -> Opt {
    Opt {
        name,
        value: None,
        about,
    }
}

const fn takes(name: &'static str, value: &'static str, about: &'static str) -> Opt {
    Opt {
        name,
        value: Some(value),
        about,
    }
}

/// Accepted by every command.
const GLOBAL: &[Opt] = &[
    takes(
        "vault",
        "PATH",
        "the vault to use; else $EMERALDIAN_VAULT, the config's `vault`, then Obsidian's",
    ),
    switch("json", "print JSON instead of text"),
    switch("help", "show this help"),
];

const LIMIT: Opt = takes("limit", "N", "show at most N results; 0 for all");

/// The graph's filters, shared by every `graph` command that builds one.
const GRAPH_FILTERS: [Opt; 5] = [
    switch("tags", "include tags as nodes"),
    switch("attachments", "include attachments as nodes"),
    switch(
        "no-unresolved",
        "leave out notes that are linked to but don't exist",
    ),
    switch("no-orphans", "leave out notes with no links"),
    takes("tag", "TAG", "only notes carrying TAG or a tag under it"),
];

const SESSION: Opt = takes(
    "session",
    "NAME",
    "a session other than your latest open one",
);

const MEMORY: [Opt; 2] = [
    takes(
        "project",
        "NAME",
        "the project; else $EMERALDIAN_PROJECT, the git repository's name, or the folder's",
    ),
    takes(
        "agent",
        "NAME",
        "who is working, such as claude or codex; else $EMERALDIAN_AGENT",
    ),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Notes,
    Finding,
    Graph,
    Memory,
    Settings,
}

impl Group {
    const ALL: [Group; 5] = [
        Group::Notes,
        Group::Finding,
        Group::Graph,
        Group::Memory,
        Group::Settings,
    ];

    fn title(self) -> &'static str {
        match self {
            Group::Notes => "NOTES",
            Group::Finding => "FINDING",
            Group::Graph => "GRAPH",
            Group::Memory => "MEMORY, for agents (see `emeraldian help mem`)",
            Group::Settings => "SETTINGS",
        }
    }
}

/// One command: how it is called, what it does, and what runs it.
struct Spec {
    /// The words that select it: `read`, or `graph local`.
    name: &'static str,
    args: &'static str,
    about: &'static str,
    group: Group,
    /// Fewest and most positional arguments.
    arity: (usize, usize),
    /// Further words are joined onto the last positional, so a sentence needs
    /// no quotes: `emeraldian mem log tests pass`.
    greedy: bool,
    options: &'static [Opt],
    /// Said after the options in the command's own help.
    more: &'static str,
    run: fn(&mut Ctx, &Args) -> Result<Output>,
}

/// Every command. The order is the order of the help.
static COMMANDS: &[Spec] = &[
    Spec {
        name: "read",
        args: "<NOTE>",
        about: "print a note",
        group: Group::Notes,
        arity: (1, 1),
        greedy: false,
        options: &[
            switch("body", "leave out the frontmatter"),
            takes(
                "lines",
                "A:B",
                "only lines A to B (1-based; `A:` and `:B` work)",
            ),
            takes("section", "HEADING", "only the section under HEADING"),
            switch("numbered", "put each line's number in front of it"),
            switch(
                "full",
                "print all of a long note rather than its first 20,000 characters",
            ),
        ],
        more: "NOTE is a name, an alias or a vault-relative path, with or without `.md`, \
               resolved the way a [[wikilink]] is.",
        run: notes::read,
    },
    Spec {
        name: "info",
        args: "<NOTE>",
        about: "a note's path, title, tags, links, tasks and dates",
        group: Group::Notes,
        arity: (1, 1),
        greedy: false,
        options: &[],
        more: "",
        run: notes::info,
    },
    Spec {
        name: "path",
        args: "<NOTE>",
        about: "the note's file on disk, for editing it with other tools",
        group: Group::Notes,
        arity: (1, 1),
        greedy: false,
        options: &[],
        more: "",
        run: notes::path,
    },
    Spec {
        name: "outline",
        args: "<NOTE>",
        about: "a note's headings, with their line numbers",
        group: Group::Notes,
        arity: (1, 1),
        greedy: false,
        options: &[],
        more: "",
        run: notes::outline,
    },
    Spec {
        name: "props",
        args: "<NOTE> [KEY]",
        about: "a note's frontmatter properties, or one of them; or change them",
        group: Group::Notes,
        arity: (1, 2),
        greedy: false,
        options: &[
            takes(
                "set",
                "KEY=VALUE",
                "set a property; `[a, b]` makes a list; repeatable",
            ),
            takes("unset", "KEY", "remove a property; repeatable"),
        ],
        more: "",
        run: notes::props,
    },
    Spec {
        name: "new",
        args: "<NOTE> [TEXT|-]",
        about: "create a note; `-` reads its text from stdin",
        group: Group::Notes,
        arity: (1, 2),
        greedy: true,
        options: &[switch(
            "unique",
            "if the name is taken, add a number rather than failing",
        )],
        more: "Folders are created as needed. Without TEXT the note starts with its name as a heading.",
        run: notes::new,
    },
    Spec {
        name: "write",
        args: "<NOTE> <TEXT|->",
        about: "replace a note's text, or create it",
        group: Group::Notes,
        arity: (2, 2),
        greedy: true,
        options: &[takes(
            "section",
            "HEADING",
            "replace only what is under HEADING, adding the section if it is missing",
        )],
        more: "",
        run: notes::write,
    },
    Spec {
        name: "append",
        args: "<NOTE> <TEXT|->",
        about: "add text to the end of a note",
        group: Group::Notes,
        arity: (2, 2),
        greedy: true,
        options: &[
            takes(
                "section",
                "HEADING",
                "add it to the end of that section instead",
            ),
            switch("create", "create the note if there is none"),
        ],
        more: "",
        run: notes::append,
    },
    Spec {
        name: "edit",
        args: "<NOTE> --old A --new B",
        about: "replace one exact piece of a note's text",
        group: Group::Notes,
        arity: (1, 1),
        greedy: false,
        options: &[
            takes(
                "old",
                "TEXT",
                "the text to replace; it must occur exactly once",
            ),
            takes("new", "TEXT", "what to put in its place; may be empty"),
            switch("all", "replace every occurrence"),
        ],
        more: "",
        run: notes::edit,
    },
    Spec {
        name: "rename",
        args: "<NOTE> <NEW NAME>",
        about: "rename a note and rewrite every link to it",
        group: Group::Notes,
        arity: (2, 2),
        greedy: false,
        options: &[],
        more: "",
        run: notes::rename,
    },
    Spec {
        name: "move",
        args: "<NOTE> <FOLDER>",
        about: "move a note to a folder, keeping its links working",
        group: Group::Notes,
        arity: (2, 2),
        greedy: false,
        options: &[],
        more: "",
        run: notes::move_note,
    },
    Spec {
        name: "delete",
        args: "<NOTE>",
        about: "move a note to the vault's .trash, where it can be restored",
        group: Group::Notes,
        arity: (1, 1),
        greedy: false,
        options: &[],
        more: "",
        run: notes::delete,
    },
    Spec {
        name: "link",
        args: "<FROM> <TO>",
        about: "add a [[link]] to TO at the end of FROM",
        group: Group::Notes,
        arity: (2, 2),
        greedy: false,
        options: &[takes("text", "SENTENCE", "words to write before the link")],
        more: "TO need not exist yet: a link to a missing note is how Obsidian plans one.",
        run: notes::link,
    },
    Spec {
        name: "task",
        args: "<NOTE> <LINE>",
        about: "tick the task on a line, as `tasks` numbers them",
        group: Group::Notes,
        arity: (2, 2),
        greedy: false,
        options: &[switch("undo", "untick it instead")],
        more: "",
        run: notes::task,
    },
    Spec {
        name: "daily",
        args: "[TEXT|-]",
        about: "today's daily note, created if need be; with TEXT, append to it",
        group: Group::Notes,
        arity: (0, 1),
        greedy: true,
        options: &[],
        more: "The folder and date format are `daily_folder` and `daily_format` under [editor].",
        run: notes::daily,
    },
    Spec {
        name: "mkdir",
        args: "<FOLDER>",
        about: "create a folder",
        group: Group::Notes,
        arity: (1, 1),
        greedy: false,
        options: &[],
        more: "",
        run: notes::mkdir,
    },
    Spec {
        name: "list",
        args: "[FOLDER]",
        about: "notes, most recently changed first",
        group: Group::Finding,
        arity: (0, 1),
        greedy: false,
        options: &[
            takes(
                "sort",
                "ORDER",
                "modified, modified-asc, created, created-asc, name or name-desc",
            ),
            takes("tag", "TAG", "only notes carrying TAG or a tag under it"),
            takes(
                "where",
                "KEY=VALUE",
                "only notes whose property KEY is VALUE; repeatable",
            ),
            switch("long", "add each note's date, words and tags"),
            LIMIT,
        ],
        more: "",
        run: query::list,
    },
    Spec {
        name: "find",
        args: "<QUERY>",
        about: "notes by name, fuzzily, as the quick switcher finds them",
        group: Group::Finding,
        arity: (1, 1),
        greedy: true,
        options: &[LIMIT],
        more: "",
        run: query::find,
    },
    Spec {
        name: "search",
        args: "<TEXT>",
        about: "notes containing TEXT, with the lines it is on",
        group: Group::Finding,
        arity: (1, 1),
        greedy: true,
        options: &[
            takes("folder", "FOLDER", "only notes in FOLDER"),
            takes("hits", "N", "lines to show from each note (default 3)"),
            LIMIT,
        ],
        more: "Ignores case unless TEXT has a capital letter. For a question rather than \
               exact text, `emeraldian recall`.",
        run: query::search,
    },
    Spec {
        name: "recall",
        args: "<QUESTION>",
        about: "what the vault knows about a question: by words, meaning and links",
        group: Group::Finding,
        arity: (1, 1),
        greedy: true,
        options: &[
            takes(
                "budget",
                "TOKENS",
                "the best passages themselves, in full, up to about TOKENS tokens",
            ),
            takes("folder", "FOLDER", "only notes in FOLDER"),
            LIMIT,
        ],
        more: "Words find names, identifiers and error text; meaning finds what was said \
               in other words or another language; links pull in notes that several \
               answers point to. Meaning needs `emeraldian index` to have been run once; \
               until then, words and links answer.",
        run: retrieve::recall,
    },
    Spec {
        name: "similar",
        args: "<NOTE>",
        about: "the notes closest to a note, and whether each is linked to it yet",
        group: Group::Finding,
        arity: (1, 1),
        greedy: false,
        options: &[LIMIT],
        more: "A close note that isn't linked yet is a link worth adding: \
               `emeraldian link <NOTE> <OTHER>`.",
        run: retrieve::similar,
    },
    Spec {
        name: "index",
        args: "",
        about: "set up recall by meaning, or bring the index up to date",
        group: Group::Finding,
        arity: (0, 0),
        greedy: false,
        options: &[
            switch(
                "status",
                "say what is indexed and whether the model is ready",
            ),
            switch("rebuild", "index every note again"),
        ],
        more: "The first run downloads EmbeddingGemma 2 (about 300 MB) through Ollama, which \
               has to be installed and running, then embeds every note; later runs only \
               what changed. Without Ollama the index holds words and links, and recall \
               works on those. The index is kept in the cache directory, not the vault; \
               the model and server are under [embeddings] in the config.",
        run: retrieve::index,
    },
    Spec {
        name: "tags",
        args: "[TAG]",
        about: "every tag with its count; with TAG, the notes carrying it",
        group: Group::Finding,
        arity: (0, 1),
        greedy: false,
        options: &[LIMIT],
        more: "",
        run: query::tags,
    },
    Spec {
        name: "tasks",
        args: "[NOTE|FOLDER]",
        about: "open tasks, with the note and line each is on",
        group: Group::Finding,
        arity: (0, 1),
        greedy: false,
        options: &[
            switch("done", "ticked tasks instead"),
            switch("all", "both"),
            LIMIT,
        ],
        more: "",
        run: query::tasks,
    },
    Spec {
        name: "links",
        args: "<NOTE>",
        about: "what a note links to, what links to it and where, and broken links",
        group: Group::Finding,
        arity: (1, 1),
        greedy: false,
        options: &[LIMIT],
        more: "",
        run: query::links,
    },
    Spec {
        name: "stats",
        args: "",
        about: "how many notes, links, tags and words",
        group: Group::Finding,
        arity: (0, 0),
        greedy: false,
        options: &[],
        more: "",
        run: query::stats,
    },
    Spec {
        name: "vaults",
        args: "",
        about: "the vaults Obsidian knows about",
        group: Group::Finding,
        arity: (0, 0),
        greedy: false,
        options: &[],
        more: "",
        run: query::vaults,
    },
    Spec {
        name: "graph",
        args: "",
        about: "the graph's size, clusters and best-linked notes",
        group: Group::Graph,
        arity: (0, 0),
        greedy: false,
        options: &GRAPH_FILTERS,
        more: "",
        run: graph::summary,
    },
    Spec {
        name: "graph local",
        args: "<NOTE>",
        about: "the notes within --depth links of NOTE, nearest first",
        group: Group::Graph,
        arity: (1, 1),
        greedy: false,
        options: &[
            takes("depth", "N", "how many links out to go (default 1)"),
            switch("edges", "list the links between them too"),
            LIMIT,
            GRAPH_FILTERS[0],
            GRAPH_FILTERS[1],
            GRAPH_FILTERS[2],
            GRAPH_FILTERS[3],
            GRAPH_FILTERS[4],
        ],
        more: "Each line is how many links away, the note, and in brackets how many links it has.",
        run: graph::local,
    },
    Spec {
        name: "graph path",
        args: "<FROM> <TO>",
        about: "the shortest chain of links between two notes",
        group: Group::Graph,
        arity: (2, 2),
        greedy: false,
        options: &GRAPH_FILTERS,
        more: "`A -> B` means A links to B; `A <- B`, that B links to A.",
        run: graph::path,
    },
    Spec {
        name: "graph hubs",
        args: "",
        about: "the notes with the most links",
        group: Group::Graph,
        arity: (0, 0),
        greedy: false,
        options: &[
            LIMIT,
            GRAPH_FILTERS[0],
            GRAPH_FILTERS[1],
            GRAPH_FILTERS[2],
            GRAPH_FILTERS[4],
        ],
        more: "",
        run: graph::hubs,
    },
    Spec {
        name: "graph orphans",
        args: "",
        about: "notes with no links in or out",
        group: Group::Graph,
        arity: (0, 0),
        greedy: false,
        options: &[LIMIT],
        more: "",
        run: graph::orphans,
    },
    Spec {
        name: "graph unresolved",
        args: "",
        about: "notes that are linked to but don't exist yet, and who links to them",
        group: Group::Graph,
        arity: (0, 0),
        greedy: false,
        options: &[LIMIT],
        more: "",
        run: graph::unresolved,
    },
    Spec {
        name: "graph export",
        args: "",
        about: "the whole graph, as JSON, Graphviz DOT or Mermaid",
        group: Group::Graph,
        arity: (0, 0),
        greedy: false,
        options: &[
            takes("format", "FORMAT", "json (the default), dot or mermaid"),
            switch(
                "layout",
                "lay it out as the app does and include each node's x and y",
            ),
            GRAPH_FILTERS[0],
            GRAPH_FILTERS[1],
            GRAPH_FILTERS[2],
            GRAPH_FILTERS[3],
            GRAPH_FILTERS[4],
        ],
        more: "",
        run: graph::export,
    },
    Spec {
        name: "mem",
        args: "",
        about: "where the project stands; the same as `mem resume`",
        group: Group::Memory,
        arity: (0, 0),
        greedy: false,
        options: &MEMORY,
        more: mem::GUIDE,
        run: mem::resume,
    },
    Spec {
        name: "mem start",
        args: "<TASK>",
        about: "open a session for TASK, printing where the last one left off",
        group: Group::Memory,
        arity: (1, 1),
        greedy: true,
        options: &MEMORY,
        more: "",
        run: mem::start,
    },
    Spec {
        name: "mem resume",
        args: "",
        about: "the project note, recent sessions, open tasks and what is remembered",
        group: Group::Memory,
        arity: (0, 0),
        greedy: false,
        options: &MEMORY,
        more: "",
        run: mem::resume,
    },
    Spec {
        name: "mem log",
        args: "<TEXT|->",
        about: "add a timestamped line to your session's log",
        group: Group::Memory,
        arity: (1, 1),
        greedy: true,
        options: &[MEMORY[0], MEMORY[1], SESSION],
        more: "",
        run: mem::log,
    },
    Spec {
        name: "mem end",
        args: "<SUMMARY|->",
        about: "close your session with a handoff for whoever comes next",
        group: Group::Memory,
        arity: (1, 1),
        greedy: true,
        options: &[
            takes(
                "next",
                "STEP",
                "something still to do, written as a task; repeatable",
            ),
            takes(
                "status",
                "STATUS",
                "done (the default), paused or blocked; the last two stay open",
            ),
            MEMORY[0],
            MEMORY[1],
            SESSION,
        ],
        more: "The branch, the commit and any uncommitted files are added to the handoff.",
        run: mem::end,
    },
    Spec {
        name: "mem remember",
        args: "<TITLE> <TEXT|->",
        about: "keep something worth knowing next time, as a note of its own",
        group: Group::Memory,
        arity: (2, 2),
        greedy: true,
        options: &[
            takes("tag", "TAG", "tag it; repeatable"),
            takes("link", "NOTE", "link it to a related note; repeatable"),
            switch(
                "replace",
                "replace what is kept under TITLE rather than adding to it",
            ),
            MEMORY[0],
            MEMORY[1],
            SESSION,
        ],
        more: "",
        run: mem::remember,
    },
    Spec {
        name: "mem recall",
        args: "<QUERY>",
        about: "search what has been remembered and logged, best match first",
        group: Group::Memory,
        arity: (1, 1),
        greedy: true,
        options: &[
            takes(
                "budget",
                "TOKENS",
                "the best passages themselves, in full, up to about TOKENS tokens",
            ),
            switch("all", "search every project, not only this one"),
            LIMIT,
            MEMORY[0],
            MEMORY[1],
        ],
        more: "Found by words, by meaning once `emeraldian index` has been run, and \
               through links; what was kept ranks above the logs it came from.",
        run: mem::recall,
    },
    Spec {
        name: "mem sessions",
        args: "",
        about: "past sessions, most recent first",
        group: Group::Memory,
        arity: (0, 0),
        greedy: false,
        options: &[LIMIT, MEMORY[0], MEMORY[1]],
        more: "",
        run: mem::sessions,
    },
    Spec {
        name: "mem done",
        args: "<TASK>",
        about: "tick the project's open task whose text contains TASK",
        group: Group::Memory,
        arity: (1, 1),
        greedy: true,
        options: &[MEMORY[0], MEMORY[1], SESSION],
        more: "",
        run: mem::done,
    },
    Spec {
        name: "config",
        args: "[KEY [VALUE]]",
        about: "print the settings, one of them, or change one in config.toml",
        group: Group::Settings,
        arity: (0, 2),
        greedy: false,
        options: &[switch("path", "print where the config file is")],
        more: "KEY is dotted, as in `editor.daily_folder`. VALUE is read as TOML where it \
               can be, so `true` and `3` are a switch and a number, and as text otherwise. \
               The rest of the file, comments included, is left as it was.",
        run: settings::config,
    },
];

/// Runs `emeraldian <command> …`, returning the exit code, or `None` when the
/// first argument names no command and the app should start instead.
pub fn run(raw: &[String]) -> Option<i32> {
    let first = raw.first()?;
    if first == "help" {
        return Some(help(&raw[1..]));
    }
    let (spec, rest) = find(raw)?;

    // As `main` explains, nothing may read the config directory before this.
    crate::config::migrate_legacy_dir();

    let args = match parse(spec, rest) {
        Ok(args) => args,
        Err(failure) => return Some(report(&failure)),
    };
    if args.flag("help") {
        print(&usage(spec));
        return Some(0);
    }

    let (config, config_error) = Config::load();
    if let Some(error) = config_error {
        // Said every time, since a vault or a memory folder set there is
        // being ignored, and the command would otherwise quietly use another.
        eprintln!("emeraldian: config ignored: {error}");
    }
    let mut ctx = Ctx {
        config,
        json: args.flag("json"),
        vault_flag: args.value("vault").map(str::to_string),
        vault: None,
        scan: None,
        index: None,
        config_file: None,
        embedder: None,
        cache_dir: None,
    };
    let code = match (spec.run)(&mut ctx, &args) {
        Ok(output) => {
            if let Some(notice) = &output.notice {
                eprintln!("{notice}");
            }
            if ctx.json {
                print(&output.json.to_string());
            } else if !output.text.is_empty() {
                print(&output.text);
            }
            Some(0)
        }
        Err(failure) => Some(report(&failure)),
    };
    // The process ends as soon as this returns, and its memory goes back to
    // the system all at once: freeing what the command built first, a
    // listing of every note among it, would only make it end later.
    std::mem::forget(ctx);
    code
}

/// The command a list of words names, and the words after it.
fn find(raw: &[String]) -> Option<(&'static Spec, &[String])> {
    if let [group, sub, rest @ ..] = raw {
        let name = format!("{group} {sub}");
        if let Some(spec) = COMMANDS.iter().find(|spec| spec.name == name) {
            return Some((spec, rest));
        }
    }
    let spec = COMMANDS.iter().find(|spec| spec.name == raw[0])?;
    Some((spec, &raw[1..]))
}

/// `emeraldian --help`: the app's options, then every command.
pub fn help_text() -> String {
    format!("{}\n{}", crate::cli::HELP.trim_end(), overview())
}

/// `emeraldian help [COMMAND]`.
fn help(words: &[String]) -> i32 {
    if words.is_empty() {
        print(&help_text());
        return 0;
    }
    let name = words.join(" ");
    match COMMANDS.iter().find(|spec| spec.name == name) {
        Some(spec) => {
            print(&usage(spec));
            0
        }
        None => report(&Failure::Usage(format!("no command called `{name}`"))),
    }
}

/// Every command, grouped, one line each: the COMMANDS part of `--help`.
pub fn overview() -> String {
    let mut out = String::from("\nCOMMANDS (`emeraldian help <COMMAND>` for its options):\n");
    for group in Group::ALL {
        out.push_str(&format!("  {}\n", group.title()));
        for spec in COMMANDS.iter().filter(|spec| spec.group == group) {
            let call = format!("{} {}", spec.name, spec.args);
            out.push_str(&format!("    {:<32} {}\n", call.trim_end(), spec.about));
        }
    }
    out.push_str(
        "  Every command takes --vault <PATH> and --json. A folder named like a command\n  \
         opens in the app as `emeraldian ./name`.\n",
    );
    out
}

/// One command's help.
fn usage(spec: &Spec) -> String {
    let call = format!("{} {}", spec.name, spec.args);
    let mut out = format!("usage: emeraldian {}\n\n{}\n", call.trim_end(), spec.about);
    let options: Vec<&Opt> = spec.options.iter().chain(GLOBAL.iter()).collect();
    out.push_str("\noptions:\n");
    for opt in options {
        let call = match opt.value {
            Some(value) => format!("--{} <{value}>", opt.name),
            None => format!("--{}", opt.name),
        };
        out.push_str(&format!("  {call:<22} {}\n", opt.about));
    }
    if !spec.more.is_empty() {
        out.push_str(&format!("\n{}\n", spec.more));
    }
    out
}

// ---- arguments --------------------------------------------------------------

/// A command's arguments, sorted into positionals, valued options and
/// switches.
#[derive(Debug, Default)]
pub(crate) struct Args {
    positionals: Vec<String>,
    values: Vec<(&'static str, String)>,
    switches: Vec<&'static str>,
}

impl Args {
    pub fn get(&self, index: usize) -> Option<&str> {
        self.positionals.get(index).map(String::as_str)
    }

    /// A positional that parsing has already guaranteed is there.
    pub fn at(&self, index: usize) -> &str {
        self.get(index).unwrap_or_default()
    }

    /// The last value given for an option.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Every value given for a repeatable option, in order.
    pub fn values(&self, name: &str) -> Vec<&str> {
        self.values
            .iter()
            .filter(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn flag(&self, name: &str) -> bool {
        self.switches.contains(&name)
    }

    /// A numeric option, or `default` without one.
    pub fn number(&self, name: &str, default: usize) -> Result<usize> {
        match self.value(name) {
            None => Ok(default),
            Some(text) => text
                .trim()
                .parse()
                .map_err(|_| Failure::Usage(format!("--{name} needs a number, not `{text}`"))),
        }
    }

    /// `--limit`, where 0 means no limit at all.
    pub fn limit(&self, default: usize) -> Result<usize> {
        Ok(match self.number("limit", default)? {
            0 => usize::MAX,
            n => n,
        })
    }

    /// The positional at `index` as text, reading stdin for `-`.
    pub fn text(&self, index: usize) -> Result<Option<String>> {
        match self.get(index) {
            None => Ok(None),
            Some("-") => {
                let mut text = String::new();
                io::stdin()
                    .read_to_string(&mut text)
                    .map_err(|err| Failure::Failed(format!("could not read stdin: {err}")))?;
                Ok(Some(text))
            }
            Some(text) => Ok(Some(text.to_string())),
        }
    }
}

fn parse(spec: &Spec, words: &[String]) -> Result<Args> {
    let mut args = Args::default();
    let mut words = words.iter();
    let mut only_positionals = false;

    while let Some(word) = words.next() {
        // `-h` first, as `emeraldian read -h`, is the help any program gives
        // for it. Anywhere else it is a word like any other.
        if word == "-h" && args.positionals.is_empty() && !only_positionals {
            args.switches.push("help");
            continue;
        }
        let Some(option) = word.strip_prefix("--").filter(|_| !only_positionals) else {
            args.positionals.push(word.clone());
            continue;
        };
        if option.is_empty() {
            only_positionals = true;
            continue;
        }
        let (name, inline) = match option.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (option, None),
        };
        let Some(opt) = spec.options.iter().chain(GLOBAL).find(|o| o.name == name) else {
            return Err(Failure::Usage(format!(
                "`emeraldian {}` has no --{name} option; see `emeraldian {} --help`",
                spec.name, spec.name
            )));
        };
        match opt.value {
            Some(_) => {
                let value = match inline {
                    Some(value) => value,
                    None => words
                        .next()
                        .cloned()
                        .ok_or_else(|| Failure::Usage(format!("--{name} needs a value")))?,
                };
                args.values.push((opt.name, value));
            }
            None if inline.is_some() => {
                return Err(Failure::Usage(format!("--{name} takes no value")));
            }
            None => args.switches.push(opt.name),
        }
    }

    // Asked for help, the arguments needn't be complete.
    if args.flag("help") {
        return Ok(args);
    }
    let (min, max) = spec.arity;
    if spec.greedy && max > 0 && args.positionals.len() > max {
        let tail = args.positionals.split_off(max - 1).join(" ");
        args.positionals.push(tail);
    }
    if args.positionals.len() < min {
        return Err(Failure::Usage(format!(
            "usage: emeraldian {} {}",
            spec.name, spec.args
        )));
    }
    if let Some(extra) = args.positionals.get(max) {
        return Err(Failure::Usage(format!(
            "unexpected `{extra}`; usage: emeraldian {} {}",
            spec.name, spec.args
        )));
    }
    Ok(args)
}

// ---- results ------------------------------------------------------------------

/// Why a command didn't do what it was asked.
#[derive(Debug)]
pub(crate) enum Failure {
    /// The command line itself was wrong. Exits 2, as usage errors do.
    Usage(String),
    /// It was understood but couldn't be done. Exits 1.
    Failed(String),
}

pub(crate) type Result<T> = std::result::Result<T, Failure>;

pub(crate) fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Failure::Failed(message.into()))
}

impl From<emeraldian_core::error::Error> for Failure {
    fn from(err: emeraldian_core::error::Error) -> Self {
        Self::Failed(err.to_string())
    }
}

impl From<io::Error> for Failure {
    fn from(err: io::Error) -> Self {
        Self::Failed(err.to_string())
    }
}

fn report(failure: &Failure) -> i32 {
    match failure {
        Failure::Usage(message) => {
            eprintln!("emeraldian: {message}");
            2
        }
        Failure::Failed(message) => {
            eprintln!("emeraldian: {message}");
            1
        }
    }
}

/// What a command produced: text for a person or an agent, the same as JSON
/// for a program, and anything to say on stderr instead.
pub(crate) struct Output {
    text: String,
    json: Value,
    notice: Option<String>,
}

impl Output {
    pub fn new(text: impl Into<String>, json: Value) -> Self {
        Self {
            text: text.into(),
            json,
            notice: None,
        }
    }

    /// Something worth knowing that isn't the result, such as where a long
    /// note was cut off. Kept out of stdout, so a script piping the result on
    /// gets only the result.
    #[must_use]
    pub fn notice(mut self, notice: impl Into<String>) -> Self {
        self.notice = Some(notice.into());
        self
    }
}

/// Writes to stdout, quietly stopping if the reader has gone, as it has when
/// the output is piped into `head`.
fn print(text: &str) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{}", text.trim_end_matches('\n'));
}

/// Keeps the first `limit` items, returning how many were dropped.
pub(crate) fn cap<T>(items: &mut Vec<T>, limit: usize) -> usize {
    let cut = items.len().saturating_sub(limit);
    items.truncate(limit);
    cut
}

/// The line ending a capped list, saying what it left out.
pub(crate) fn more(cut: usize) -> String {
    format!("… {cut} more (--limit N)")
}

/// Joins lines, with [`more`]'s line at the end if anything was cut.
pub(crate) fn lines(mut lines: Vec<String>, cut: usize) -> String {
    if cut > 0 {
        lines.push(more(cut));
    }
    lines.join("\n")
}

/// A note's name as it is shown and typed: its vault path without `.md`.
pub(crate) fn display(rel: &str) -> &str {
    rel.strip_suffix(".md").unwrap_or(rel)
}

/// Cuts text to `max` characters on a character boundary, marking the cut.
pub(crate) fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
    }
}

// ---- the vault ------------------------------------------------------------------

/// A note found on disk.
#[derive(Debug, Clone)]
pub(crate) struct Located {
    /// Vault-relative, with `.md`.
    pub rel: String,
    pub path: PathBuf,
}

impl Located {
    pub fn name(&self) -> &str {
        display(&self.rel)
    }
}

/// What one run of a command works with. The vault is found, listed and
/// indexed only when a command first asks for each, and each costs more than
/// the last: a listing walks the folders, an index reads every note.
pub(crate) struct Ctx {
    pub config: Config,
    pub json: bool,
    vault_flag: Option<String>,
    vault: Option<Vault>,
    scan: Option<Scan>,
    index: Option<VaultIndex>,
    /// Where `config` reads and writes, when it isn't the real config file.
    /// `None` everywhere but in tests, which must not change the settings of
    /// whoever runs them.
    config_file: Option<PathBuf>,
    /// Stands in for the embeddings server, in tests.
    embedder: Option<Box<dyn emeraldian_core::retrieval::Embed>>,
    /// Where the retrieval index is kept, when it isn't the cache directory.
    /// `None` but in tests, for the same reason as `config_file`.
    cache_dir: Option<PathBuf>,
}

impl Ctx {
    /// The vault: `--vault`, else `$EMERALDIAN_VAULT`, else the config's
    /// `vault`, else the one Obsidian has open or last had open.
    pub fn vault(&mut self) -> Result<&Vault> {
        if self.vault.is_none() {
            let chosen = self
                .vault_flag
                .clone()
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("EMERALDIAN_VAULT")
                        .filter(|value| !value.is_empty())
                        .map(PathBuf::from)
                })
                .or_else(|| self.config.vault.clone())
                .or_else(|| {
                    let known = emeraldian_core::vault::discover();
                    known
                        .iter()
                        .find(|v| v.open)
                        .or_else(|| known.first())
                        .map(|v| v.path.clone())
                });
            let Some(chosen) = chosen else {
                return fail(
                    "no vault: pass --vault <PATH>, set EMERALDIAN_VAULT, \
                     or run `emeraldian config vault <PATH>`",
                );
            };
            let path = crate::absolute(&crate::expand_home(chosen));
            if !path.is_dir() {
                return fail(format!("{} is not a folder", path.display()));
            }
            self.vault = Some(Vault::from_path(path));
        }
        Ok(self.vault.as_ref().expect("just set"))
    }

    /// The vault's folder.
    pub fn root(&mut self) -> Result<PathBuf> {
        Ok(self.vault()?.path.clone())
    }

    /// Which folders the app shows: hidden ones only if it is set to.
    fn scan_options(&self) -> ScanOptions {
        ScanOptions {
            include_hidden: self.config.ui.show_hidden,
            ..Default::default()
        }
    }

    /// Every note's name, path and dates, from a walk of the folders, made the
    /// first time it is needed. Nothing is read from inside a note.
    pub fn scan(&mut self) -> Result<&Scan> {
        if self.scan.is_none() {
            let options = ScanOptions {
                include_attachments: false,
                ..self.scan_options()
            };
            let scan = self.vault()?.scan(&options)?;
            self.scan = Some(scan);
        }
        Ok(self.scan.as_ref().expect("just made"))
    }

    /// The vault's index, built the first time it is needed.
    pub fn index(&mut self) -> Result<&mut VaultIndex> {
        if self.index.is_none() {
            let vault = self.vault()?.clone();
            let index = VaultIndex::build(vault, self.scan_options())?;
            self.index = Some(index);
        }
        Ok(self.index.as_mut().expect("just built"))
    }

    /// Finds a note the way a `[[wikilink]]` would, as cheaply as the name
    /// allows.
    ///
    /// A note's exact vault path is found on disk with no listing at all, which
    /// keeps the commonest call — reading or writing a note by the name another
    /// command printed — as cheap in a vault of ten thousand notes as in one of
    /// ten. A bare name or a different case takes a listing of the folders.
    /// Only an alias, or a name that matches nothing, takes reading every note
    /// into an index.
    pub fn locate(&mut self, name: &str) -> Result<Located> {
        let vault = self.vault()?.clone();
        if let Ok(normal) = normalize(name)
            && let Ok(rel) = vault.note_rel(&normal)
            && let Ok(path) = vault.resolve_path(&rel)
            && exists_exactly(&path)
        {
            return Ok(Located { rel, path });
        }
        if let Some(meta) = self.scan()?.resolve(name) {
            return Ok(Located {
                rel: meta.rel.clone(),
                path: meta.path.clone(),
            });
        }

        let index = self.index()?;
        match index.resolve(name).and_then(|id| index.note(id)) {
            Some(note) => Ok(Located {
                rel: note.meta.rel.clone(),
                path: note.meta.path.clone(),
            }),
            None => {
                let near: Vec<String> = search::suggest(index, name, 3)
                    .into_iter()
                    .filter_map(|id| index.note(id).map(|n| display(&n.meta.rel).to_string()))
                    .collect();
                if near.is_empty() {
                    fail(format!("no note named `{name}`"))
                } else {
                    fail(format!(
                        "no note named `{name}`; did you mean {}?",
                        near.join(", ")
                    ))
                }
            }
        }
    }

    /// Where a note called `name` would be created: its exact path, without
    /// asking whether a note elsewhere already answers to the name.
    pub fn place(&mut self, name: &str) -> Result<Located> {
        let name = normalize(name)?;
        let vault = self.vault()?;
        let rel = vault.note_rel(&name)?;
        let path = vault.resolve_path(&rel)?;
        Ok(Located { rel, path })
    }

    /// A folder inside the vault, checked like a note's path.
    pub fn folder(&mut self, rel: &str) -> Result<(String, PathBuf)> {
        let rel = normalize(rel)?;
        if rel.is_empty() {
            return Ok((rel, self.root()?));
        }
        let path = self.vault()?.resolve_path(&rel)?;
        Ok((rel, path))
    }
}

/// A vault-relative path with its `.` and `..` worked out: `a/./b` is `a/b`
/// and `a/../b` is `b`.
///
/// Left in, they reach a path's text as well as its file, and the text is
/// what a link is written from: moving a note to `.` would rewrite every link
/// to it as `[[./Note]]`, which Obsidian reads from the linking note's own
/// folder. A `..` that would climb out of the vault is refused.
pub(crate) fn normalize(rel: &str) -> Result<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in rel.trim().split(['/', '\\']) {
        match part.trim() {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return fail(format!("path is outside the vault: {}", rel.trim()));
                }
            }
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

/// Whether `path` is a file under exactly that name.
///
/// On a case-insensitive filesystem, `ideas.md` "exists" when `Ideas.md` does,
/// and treating it as the note's name would print, and later link, the wrong
/// case. Listing the folder settles it.
fn exists_exactly(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    fs::read_dir(parent).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.file_name().as_os_str() == name)
    })
}

/// Reads a note, naming it in the error rather than leaving a bare "not
/// found".
pub(crate) fn read(note: &Located) -> Result<String> {
    fs::read_to_string(&note.path)
        .map_err(|err| Failure::Failed(format!("could not read {}: {err}", note.name())))
}

/// Replaces a file's content all at once.
///
/// The new text goes to a hidden file beside it, which is then renamed over
/// the original: a reader sees the old note or the new one, never a half-written
/// one, and a crash part-way leaves the note as it was. The copy is given the
/// original's permissions first, so a note kept private stays private.
///
/// A rename replaces the name, not the file it names, so a note that is a
/// symbolic link, or one of several hard links to a file, is written in place
/// instead: renaming over it would leave the file it stands for unchanged and
/// put a separate copy in the vault. So is a note where the rename is refused,
/// as Windows refuses one while another program holds the file open.
pub(crate) fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let existing = fs::symlink_metadata(path).ok();
    if existing.as_ref().is_some_and(|m| !m.is_file() || shared(m)) {
        fs::write(path, content)?;
        return Ok(());
    }

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let staging = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    fs::write(&staging, content)?;
    if let Some(existing) = &existing {
        let _ = fs::set_permissions(&staging, existing.permissions());
    }
    if fs::rename(&staging, path).is_err() {
        let _ = fs::remove_file(&staging);
        fs::write(path, content)?;
    }
    Ok(())
}

/// Whether a file has other names than this one: hard links, which a rename
/// would part from it.
#[cfg(unix)]
fn shared(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink() > 1
}

#[cfg(not(unix))]
fn shared(_metadata: &fs::Metadata) -> bool {
    false
}

/// Adds text to the end of a file in one write.
///
/// The file is opened for appending, so two agents adding lines at the same
/// moment each land whole, one after the other, rather than one rewriting the
/// file over the other's addition. A newline is put first if the file doesn't
/// end with one, and last if the text doesn't.
pub(crate) fn append_file(path: &Path, text: &str) -> Result<()> {
    let mut file = OpenOptions::new().read(true).append(true).open(path)?;
    let mut chunk = String::with_capacity(text.len() + 2);
    if file.metadata()?.len() > 0 {
        let mut last = [0u8; 1];
        file.seek(SeekFrom::End(-1))?;
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            chunk.push('\n');
        }
    }
    chunk.push_str(text);
    if !chunk.ends_with('\n') {
        chunk.push('\n');
    }
    file.write_all(chunk.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests;
