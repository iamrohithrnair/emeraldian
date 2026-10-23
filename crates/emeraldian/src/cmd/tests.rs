//! The command line, end to end, against real vaults on disk.

use super::*;
use emeraldian_core::test_support::TempVault;

/// Runs a command the way `emeraldian <words>` would, against `vault`, with
/// its config file inside the vault's hidden `.config` folder rather than in
/// the real config directory, its semantic index in `.cache`, and a stand-in
/// for the embedding model.
fn exec(vault: &TempVault, words: &[&str]) -> Result<Output> {
    exec_with(
        vault,
        words,
        Box::new(emeraldian_core::retrieval::fake::Words::default()),
    )
}

/// [`exec`] with a model of the test's choosing.
fn exec_with(
    vault: &TempVault,
    words: &[&str],
    model: Box<dyn emeraldian_core::retrieval::Embed>,
) -> Result<Output> {
    exec_model(vault, words, Some(model))
}

/// [`exec`] with the model the config names reached over HTTP, as it is
/// outside tests.
fn exec_served(vault: &TempVault, words: &[&str]) -> Result<Output> {
    exec_model(vault, words, None)
}

fn exec_model(
    vault: &TempVault,
    words: &[&str],
    model: Option<Box<dyn emeraldian_core::retrieval::Embed>>,
) -> Result<Output> {
    let raw: Vec<String> = words.iter().map(|w| (*w).to_string()).collect();
    let (spec, rest) = find(&raw).expect("names a command");
    let args = parse(spec, rest)?;
    let config_file = vault.path().join(".config").join("config.toml");
    let mut ctx = Ctx {
        config: Config::load_from(&config_file).0,
        json: args.flag("json"),
        vault_flag: Some(vault.path().display().to_string()),
        vault: None,
        scan: None,
        index: None,
        config_file: Some(config_file),
        embedder: model,
        cache_dir: Some(vault.path().join(".cache")),
    };
    (spec.run)(&mut ctx, &args)
}

/// A command's text, which must have succeeded.
fn ok(vault: &TempVault, words: &[&str]) -> String {
    match exec(vault, words) {
        Ok(output) => output.text,
        Err(failure) => panic!("`{}` failed: {failure:?}", words.join(" ")),
    }
}

/// A command's JSON, which must have succeeded.
fn json(vault: &TempVault, words: &[&str]) -> Value {
    match exec(vault, words) {
        Ok(output) => output.json,
        Err(failure) => panic!("`{}` failed: {failure:?}", words.join(" ")),
    }
}

/// A command's failure message, which it must have failed with.
fn failure(vault: &TempVault, words: &[&str]) -> String {
    match exec(vault, words) {
        Ok(output) => panic!("`{}` succeeded: {}", words.join(" "), output.text),
        Err(Failure::Failed(message) | Failure::Usage(message)) => message,
    }
}

fn sample() -> TempVault {
    let vault = TempVault::new("cmd");
    vault.write(
        "Alpha.md",
        "---\ntags: [project]\nstatus: active\naliases: [First]\n---\n# Alpha\n\nSee [[Beta]] and [[Ghost]].\n\n## Plan\n\n- [ ] write the tests\n- [x] sketch it\n\n## Notes\n\nA needle here.\n",
    );
    vault.write(
        "Beta.md",
        "# Beta\n\nBack to [[Alpha]]. Then [[Projects/Gamma]].\n",
    );
    vault.write(
        "Projects/Gamma.md",
        "# Gamma\n\n#project/sub\nnothing much\n",
    );
    vault.write("Lonely.md", "# Lonely\n\nno links at all\n");
    vault
}

// ---- parsing ------------------------------------------------------------------

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_string()).collect()
}

#[test]
fn a_word_that_is_not_a_command_starts_the_app() {
    assert!(find(&words(&["/some/vault"])).is_none());
    assert!(find(&words(&["--graph"])).is_none());
    assert!(
        find(&words(&["Daily"])).is_none(),
        "commands are lower case"
    );
}

#[test]
fn two_word_commands_win_over_their_group() {
    let raw = words(&["graph", "local", "Alpha"]);
    let (spec, rest) = find(&raw).expect("command");
    assert_eq!(spec.name, "graph local");
    assert_eq!(rest, ["Alpha"]);

    let raw = words(&["graph", "--tags"]);
    let (spec, rest) = find(&raw).expect("command");
    assert_eq!(spec.name, "graph");
    assert_eq!(rest, ["--tags"]);
}

#[test]
fn a_sentence_needs_no_quotes_where_a_command_takes_one() {
    let raw = words(&["mem", "log", "tests", "pass", "--agent", "x"]);
    let (spec, rest) = find(&raw).unwrap();
    let args = parse(spec, rest).expect("parses");
    assert_eq!(args.at(0), "tests pass");
    assert_eq!(args.value("agent"), Some("x"));

    let raw = words(&["rename", "Old", "Note", "New"]);
    let (spec, rest) = find(&raw).unwrap();
    assert!(
        matches!(parse(spec, rest), Err(Failure::Usage(_))),
        "two names can't be told apart without quotes, so it is refused"
    );
}

#[test]
fn options_take_values_either_way_and_double_dash_ends_them() {
    let raw = words(&["edit", "A", "--old=x", "--new", "--y", "--", "--all"]);
    let (spec, rest) = find(&raw).unwrap();
    let error = parse(spec, rest).expect_err("--all after -- is an argument");
    assert!(matches!(error, Failure::Usage(_)));

    let raw = words(&["edit", "A", "--old=x", "--new", "--y"]);
    let (spec, rest) = find(&raw).unwrap();
    let args = parse(spec, rest).expect("parses");
    assert_eq!(args.value("old"), Some("x"));
    assert_eq!(
        args.value("new"),
        Some("--y"),
        "a value may start with dashes"
    );
}

#[test]
fn mistakes_on_the_command_line_are_usage_errors() {
    let vault = sample();
    for bad in [
        vec!["read"],
        vec!["read", "Alpha", "--nope"],
        vec!["list", "--limit"],
        vec!["list", "--limit", "many"],
        vec!["stats", "extra"],
        vec!["read", "Alpha", "--body=yes"],
    ] {
        assert!(
            matches!(exec(&vault, &bad), Err(Failure::Usage(_))),
            "{bad:?} should be a usage error"
        );
    }
}

// ---- reading -------------------------------------------------------------------

#[test]
fn read_prints_a_note_or_the_part_asked_for() {
    let vault = sample();
    assert!(ok(&vault, &["read", "Beta"]).starts_with("# Beta"));
    assert_eq!(ok(&vault, &["read", "Alpha", "--lines", "6:6"]), "# Alpha");
    assert_eq!(
        ok(&vault, &["read", "Alpha", "--lines", "6:7", "--numbered"]),
        "6: # Alpha\n7: "
    );
    assert_eq!(
        ok(&vault, &["read", "Alpha", "--section", "plan"]),
        "## Plan\n\n- [ ] write the tests\n- [x] sketch it\n"
    );
    assert!(ok(&vault, &["read", "Alpha", "--body"]).starts_with("# Alpha"));
    assert!(
        failure(&vault, &["read", "Alpha", "--section", "Nope"])
            .contains("it has Alpha, Plan, Notes")
    );
}

#[test]
fn a_long_note_is_cut_and_says_where() {
    let vault = TempVault::new("cmd-long");
    let line = "x".repeat(99);
    vault.write("Big.md", &format!("{line}\n").repeat(1_000));

    let output = exec(&vault, &["read", "Big"]).expect("reads");
    assert!(output.text.chars().count() <= 20_000);
    assert!(output.text.ends_with(&line), "cut at the end of a line");
    let notice = output.notice.expect("says it was cut");
    assert!(notice.contains("of 1000"), "{notice}");
    assert!(notice.contains("--lines 201:"), "{notice}");

    let full = exec(&vault, &["read", "Big", "--full"]).expect("reads");
    assert!(full.notice.is_none());
    assert_eq!(full.text.lines().count(), 1_000);
}

#[test]
fn notes_are_found_by_path_name_alias_or_case() {
    let vault = sample();
    for name in ["Alpha", "Alpha.md", "alpha", "First", "/Alpha"] {
        assert!(
            ok(&vault, &["read", name]).contains("# Alpha"),
            "{name} should find Alpha"
        );
    }
    assert!(
        ok(&vault, &["read", "Gamma"]).contains("# Gamma"),
        "a bare name"
    );
    assert!(
        ok(&vault, &["path", "alpha"]).ends_with("Alpha.md"),
        "the real case"
    );
}

#[test]
fn a_wrong_name_suggests_the_right_one() {
    let vault = sample();
    let message = failure(&vault, &["read", "Alpah"]);
    assert!(message.contains("did you mean Alpha"), "{message}");
}

#[test]
fn a_note_is_found_with_as_little_reading_as_its_name_allows() {
    let vault = sample();
    let config_file = vault.path().join(".config/config.toml");
    let mut ctx = Ctx {
        config: Config::default(),
        json: false,
        vault_flag: Some(vault.path().display().to_string()),
        vault: None,
        scan: None,
        index: None,
        config_file: Some(config_file),
        embedder: None,
        cache_dir: None,
    };
    ctx.locate("Projects/Gamma").expect("found");
    assert!(ctx.scan.is_none(), "an exact path needs not even a listing");
    ctx.locate("Gamma").expect("found");
    assert!(ctx.scan.is_some(), "a bare name takes a listing");
    assert!(ctx.index.is_none(), "but no index");
    ctx.locate("First").expect("found");
    assert!(ctx.index.is_some(), "an alias does");
}

#[test]
fn info_and_outline_describe_a_note() {
    let vault = sample();
    let info = ok(&vault, &["info", "Alpha"]);
    assert!(info.contains("path: Alpha.md"), "{info}");
    assert!(info.contains("aliases: First"), "{info}");
    assert!(info.contains("tags: #project"), "{info}");
    assert!(info.contains("links: 1 out, 1 in, 1 unresolved"), "{info}");
    assert!(info.contains("tasks: 1 open, 1 done"), "{info}");

    assert_eq!(
        ok(&vault, &["outline", "Alpha"]),
        "6: # Alpha\n10: ## Plan\n15: ## Notes"
    );
}

#[test]
fn properties_are_read_and_changed_one_at_a_time() {
    let vault = sample();
    assert_eq!(ok(&vault, &["props", "Alpha", "status"]), "active");
    assert!(ok(&vault, &["props", "Alpha"]).contains("tags: [project]"));

    ok(
        &vault,
        &[
            "props",
            "Alpha",
            "--set",
            "status=done",
            "--set",
            "owner=[ann, bo]",
            "--unset",
            "aliases",
        ],
    );
    let content = vault.read("Alpha.md");
    assert!(
        content.starts_with(
            "---\ntags: [project]\nstatus: done\nowner:\n  - ann\n  - bo\n---\n# Alpha"
        ),
        "{content}"
    );
    assert!(failure(&vault, &["props", "Alpha", "aliases"]).contains("no property"));
}

// ---- writing -------------------------------------------------------------------

#[test]
fn new_refuses_to_replace_a_note_unless_asked_for_a_fresh_name() {
    let vault = sample();
    assert_eq!(ok(&vault, &["new", "Inbox/Idea"]), "created Inbox/Idea");
    assert_eq!(vault.read("Inbox/Idea.md"), "# Idea\n\n");

    assert!(failure(&vault, &["new", "Inbox/Idea", "other"]).contains("already exists"));
    assert_eq!(vault.read("Inbox/Idea.md"), "# Idea\n\n", "untouched");

    assert_eq!(
        ok(&vault, &["new", "Inbox/Idea", "second", "--unique"]),
        "created Inbox/Idea 1"
    );
    assert_eq!(vault.read("Inbox/Idea 1.md"), "second\n");
}

#[test]
fn write_replaces_a_note_or_one_section_of_it() {
    let vault = sample();
    ok(&vault, &["write", "Beta", "all new"]);
    assert_eq!(vault.read("Beta.md"), "all new\n");

    ok(
        &vault,
        &["write", "Alpha", "--section", "Notes", "replaced"],
    );
    let alpha = vault.read("Alpha.md");
    assert!(alpha.ends_with("## Notes\n\nreplaced\n"), "{alpha}");
    assert!(alpha.contains("- [ ] write the tests"), "the rest is kept");

    assert_eq!(ok(&vault, &["write", "Fresh", "made"]), "created Fresh");
    assert_eq!(vault.read("Fresh.md"), "made\n");
}

#[test]
fn append_adds_to_the_end_or_to_a_section() {
    let vault = sample();
    ok(&vault, &["append", "Beta", "one more line"]);
    assert!(
        vault
            .read("Beta.md")
            .ends_with("[[Projects/Gamma]].\none more line\n")
    );

    ok(
        &vault,
        &["append", "Alpha", "--section", "Plan", "- [ ] another"],
    );
    assert!(
        vault
            .read("Alpha.md")
            .contains("- [x] sketch it\n- [ ] another\n\n## Notes")
    );

    assert!(failure(&vault, &["append", "Nowhere", "x"]).contains("no note named"));
    ok(&vault, &["append", "Nowhere", "x", "--create"]);
    assert_eq!(vault.read("Nowhere.md"), "x\n");
}

#[test]
fn append_adds_a_newline_only_where_one_is_missing() {
    let vault = TempVault::new("cmd-append");
    let path = vault.write("A.md", "no newline");
    append_file(&path, "next").expect("appends");
    append_file(&path, "last\n").expect("appends");
    assert_eq!(vault.read("A.md"), "no newline\nnext\nlast\n");
}

#[test]
fn edit_replaces_exactly_one_piece_unless_told_otherwise() {
    let vault = sample();
    ok(
        &vault,
        &["edit", "Alpha", "--old", "A needle", "--new", "The needle"],
    );
    assert!(vault.read("Alpha.md").contains("The needle here."));

    vault.write("Twice.md", "x and x\n");
    assert!(failure(&vault, &["edit", "Twice", "--old", "x", "--new", "y"]).contains("2 times"));
    ok(
        &vault,
        &["edit", "Twice", "--old", "x", "--new", "y", "--all"],
    );
    assert_eq!(vault.read("Twice.md"), "y and y\n");
    assert!(failure(&vault, &["edit", "Twice", "--old", "zzz", "--new", "y"]).contains("isn't in"));
}

#[test]
fn rename_move_and_delete_keep_the_vault_consistent() {
    let vault = sample();
    let renamed = ok(&vault, &["rename", "Beta", "Bravo"]);
    assert_eq!(
        renamed,
        "renamed Beta to Bravo; updated the links in 1 note"
    );
    assert!(vault.read("Alpha.md").contains("[[Bravo]]"));

    assert!(failure(&vault, &["rename", "Bravo", "X/Y"]).contains("move"));

    assert_eq!(
        ok(&vault, &["move", "Bravo", "Archive"]),
        "moved Bravo to Archive/Bravo"
    );
    assert!(vault.exists("Archive/Bravo.md"));

    let deleted = ok(&vault, &["delete", "Archive/Bravo"]);
    assert!(deleted.contains("restored from .trash/"), "{deleted}");
    assert!(!vault.exists("Archive/Bravo.md"));
}

#[test]
fn link_uses_the_shortest_name_that_still_means_the_note() {
    let vault = sample();
    // Sorted first, so a bare `Gamma` means this one, not Projects/Gamma.
    vault.write("Other/Gamma.md", "# Another Gamma\n");
    ok(&vault, &["link", "Lonely", "Beta", "--text", "See"]);
    ok(&vault, &["link", "Lonely", "Other/Gamma"]);
    ok(&vault, &["link", "Lonely", "Projects/Gamma"]);
    ok(&vault, &["link", "Lonely", "Not yet written"]);
    let lonely = vault.read("Lonely.md");
    assert!(lonely.contains("See [[Beta]]\n[[Gamma]]\n"), "{lonely}");
    assert!(
        lonely.contains("[[Projects/Gamma]]\n"),
        "a bare `Gamma` would reach the other one: {lonely}"
    );
    assert!(lonely.ends_with("[[Not yet written]]\n"), "{lonely}");
}

#[test]
fn tasks_are_listed_and_ticked_by_line() {
    let vault = sample();
    assert_eq!(ok(&vault, &["tasks"]), "Alpha:12 write the tests");
    assert_eq!(
        ok(&vault, &["tasks", "Alpha", "--all"]),
        "Alpha:12 [ ] write the tests\nAlpha:13 [x] sketch it"
    );

    assert_eq!(
        ok(&vault, &["task", "Alpha", "12"]),
        "ticked Alpha:12 write the tests"
    );
    assert_eq!(ok(&vault, &["tasks"]), "no open tasks");
    assert!(failure(&vault, &["task", "Alpha", "6"]).contains("isn't a task"));
    ok(&vault, &["task", "Alpha", "12", "--undo"]);
    assert!(vault.read("Alpha.md").contains("- [ ] write the tests"));
}

#[test]
fn daily_creates_todays_note_once_and_appends_to_it() {
    let vault = sample();
    let name = ok(&vault, &["daily"]);
    assert!(name.starts_with("Daily/20"), "{name}");
    ok(&vault, &["daily", "a", "thought"]);
    let rel = format!("{name}.md");
    let title = name.rsplit('/').next().unwrap();
    assert_eq!(vault.read(&rel), format!("# {title}\n\na thought\n"));
    assert_eq!(
        ok(&vault, &["daily"]),
        name,
        "the same note the second time"
    );
}

#[test]
fn mkdir_makes_a_folder_and_refuses_bad_names() {
    let vault = sample();
    ok(&vault, &["mkdir", "New/Nested"]);
    assert!(vault.path().join("New/Nested").is_dir());
    assert!(failure(&vault, &["mkdir", "New/Nested"]).contains("already exists"));
    assert!(failure(&vault, &["mkdir", "bad:name"]).contains("invalid name"));
    assert!(failure(&vault, &["mkdir", "../outside"]).contains("outside the vault"));
}

#[test]
fn nothing_escapes_the_vault() {
    let vault = sample();
    for words in [
        vec!["new", "../escape", "x"],
        vec!["write", "../../escape", "x"],
        vec!["append", "../escape", "x", "--create"],
    ] {
        let message = failure(&vault, &words);
        assert!(
            message.contains("outside the vault"),
            "{words:?}: {message}"
        );
    }
    assert!(!vault.path().join("../escape.md").exists());
    assert!(!vault.path().join("../../escape.md").exists());

    // Leading dots are dropped when a name is resolved, as they are from a
    // `[[./Alpha]]` link, so this reads the vault's own Alpha.
    assert!(ok(&vault, &["read", "../Alpha"]).contains("# Alpha"));
}

// ---- finding -------------------------------------------------------------------

#[test]
fn list_filters_sorts_and_caps() {
    let vault = sample();
    assert_eq!(
        ok(&vault, &["list", "--sort", "name"]),
        "Alpha\nBeta\nProjects/Gamma\nLonely",
        "by title, as the explorer sorts"
    );
    assert_eq!(ok(&vault, &["list", "Projects"]), "Projects/Gamma");
    assert_eq!(
        ok(&vault, &["list", "--sort", "name", "--limit", "2"]),
        "Alpha\nBeta\n… 2 more (--limit N)"
    );
    assert_eq!(
        ok(&vault, &["list", "--tag", "project", "--sort", "name"]),
        "Alpha\nProjects/Gamma",
        "a nested tag counts"
    );
    assert_eq!(ok(&vault, &["list", "--where", "status=active"]), "Alpha");
    assert_eq!(ok(&vault, &["list", "--where", "aliases"]), "Alpha");
    assert!(failure(&vault, &["list", "Nowhere"]).contains("no folder"));
}

#[test]
fn find_and_search_answer_by_name_and_by_content() {
    let vault = sample();
    assert_eq!(ok(&vault, &["find", "gam"]), "Projects/Gamma");
    assert_eq!(
        ok(&vault, &["search", "needle"]),
        "Alpha\n  17: A needle here."
    );
    assert_eq!(
        ok(&vault, &["search", "nothing", "--folder", "Projects"]),
        "Projects/Gamma\n  4: nothing much"
    );
    assert!(ok(&vault, &["search", "zebra"]).starts_with("no note contains"));
}

#[test]
fn tags_count_and_list_their_notes() {
    let vault = sample();
    assert_eq!(ok(&vault, &["tags"]), "#project 2\n#project/sub 1");
    assert_eq!(ok(&vault, &["tags", "#project/sub"]), "Projects/Gamma");
    assert!(failure(&vault, &["tags", "nope"]).contains("no note is tagged"));
}

#[test]
fn links_show_both_directions_with_where_they_are() {
    let vault = sample();
    assert_eq!(
        ok(&vault, &["links", "Alpha"]),
        "out (1): Beta\nin (1):\n  Beta:3 Back to [[Alpha]]. Then [[Projects/Gamma]].\nunresolved (1): Ghost"
    );
    let json = json(&vault, &["links", "Beta", "--json"]);
    assert_eq!(json["out"], serde_json::json!(["Alpha", "Projects/Gamma"]));
}

#[test]
fn stats_count_the_vault() {
    let vault = sample();
    assert_eq!(
        ok(&vault, &["stats"]),
        "4 notes, 1 folders, 0 attachments, 2 tags, 3 links, 1 unresolved, 41 words"
    );
}

// ---- graph ----------------------------------------------------------------------

#[test]
fn the_graph_summary_names_its_hubs() {
    let vault = sample();
    assert_eq!(
        ok(&vault, &["graph"]),
        "4 notes, 1 unresolved, 3 links, 1 cluster (largest 4), 1 orphan\n\
         hubs: Alpha (2), Beta (2), Projects/Gamma (1)"
    );
}

#[test]
fn the_local_graph_says_how_far_out_each_note_is() {
    let vault = sample();
    assert_eq!(
        ok(
            &vault,
            &["graph", "local", "Projects/Gamma", "--depth", "2"]
        ),
        "0 Projects/Gamma (1)\n1 Beta (2)\n2 Alpha (2)"
    );
    let with_edges = ok(&vault, &["graph", "local", "Alpha", "--edges"]);
    assert!(
        with_edges.contains("1 Ghost (unresolved, 1)"),
        "{with_edges}"
    );
    assert!(with_edges.contains("  Alpha -- Beta"), "{with_edges}");
}

#[test]
fn a_path_says_which_way_each_link_goes() {
    let vault = sample();
    vault.write("Delta.md", "# Delta\n\n[[Projects/Gamma]]\n");
    assert_eq!(
        ok(&vault, &["graph", "path", "Alpha", "Delta"]),
        "Alpha <-> Beta -> Projects/Gamma <- Delta"
    );
    assert!(failure(&vault, &["graph", "path", "Alpha", "Lonely"]).contains("no chain"));
}

#[test]
fn hubs_orphans_and_unresolved_notes_are_listed() {
    let vault = sample();
    assert_eq!(
        ok(&vault, &["graph", "hubs", "--limit", "1"]),
        "Alpha 2\n… 2 more (--limit N)"
    );
    assert_eq!(ok(&vault, &["graph", "orphans"]), "Lonely");
    assert_eq!(ok(&vault, &["graph", "unresolved"]), "Ghost <- Alpha");
}

#[test]
fn the_graph_exports_as_json_dot_and_mermaid() {
    let vault = sample();
    let exported = json(&vault, &["graph", "export", "--layout"]);
    let nodes = exported["nodes"].as_array().expect("nodes");
    assert_eq!(nodes.len(), 5);
    assert!(
        nodes
            .iter()
            .all(|n| n["x"].is_number() && n["y"].is_number())
    );
    assert_eq!(exported["edges"].as_array().map(Vec::len), Some(3));

    let text = ok(&vault, &["graph", "export"]);
    let reparsed: Value = serde_json::from_str(&text).expect("text mode is the JSON too");
    assert_eq!(reparsed["edges"].as_array().map(Vec::len), Some(3));

    let dot = ok(&vault, &["graph", "export", "--format", "dot"]);
    assert!(dot.starts_with("graph vault {"));
    assert!(dot.contains("[label=\"Ghost\", style=dashed]"), "{dot}");

    let mermaid = ok(&vault, &["graph", "export", "--format", "mermaid"]);
    assert!(mermaid.starts_with("graph LR\n"));
    assert!(mermaid.contains(" --- "), "{mermaid}");
}

// ---- memory ----------------------------------------------------------------------

/// A `mem` command, for project `app` and the agent named.
fn mem(vault: &TempVault, agent: &str, words: &[&str]) -> Result<Output> {
    let mut all = vec!["mem"];
    all.extend_from_slice(words);
    all.extend_from_slice(&["--project", "app", "--agent", agent]);
    exec(vault, &all)
}

fn mem_ok(vault: &TempVault, agent: &str, words: &[&str]) -> String {
    match mem(vault, agent, words) {
        Ok(output) => output.text,
        Err(failure) => panic!("`mem {}` failed: {failure:?}", words.join(" ")),
    }
}

/// The session notes of project `app`, oldest first.
fn sessions(vault: &TempVault) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(vault.path().join("Agents/app/Sessions"))
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn an_empty_project_says_how_to_begin() {
    let vault = TempVault::new("mem-empty");
    let text = mem_ok(&vault, "claude", &["resume"]);
    assert_eq!(
        text,
        "app: nothing remembered yet (in Agents/app). `emeraldian mem start <TASK>` begins."
    );
    assert_eq!(mem_ok(&vault, "claude", &[]), text, "bare `mem` is resume");
}

#[test]
fn a_session_records_its_task_log_and_handoff() {
    let vault = TempVault::new("mem-session");
    let started = mem_ok(&vault, "claude", &["start", "Add", "login"]);
    assert!(started.ends_with("Started session") || started.contains("Started session"));
    let names = sessions(&vault);
    assert_eq!(names.len(), 1);
    assert!(names[0].ends_with(" claude.md"), "{names:?}");
    assert!(
        vault.exists("Agents/app/app.md"),
        "the project note is made"
    );

    mem_ok(&vault, "claude", &["log", "chose argon2"]);
    mem_ok(&vault, "claude", &["log", "two\nlines"]);
    mem_ok(
        &vault,
        "claude",
        &[
            "end",
            "Login works.",
            "--next",
            "add remember-me",
            "--next",
            "- [ ] e2e test",
            "--status",
            "paused",
        ],
    );

    let note = vault.read(&format!("Agents/app/Sessions/{}", names[0]));
    assert!(note.contains("status: paused"), "{note}");
    assert!(note.contains("ended: "), "{note}");
    assert!(
        note.contains("# Add login\n\nProject: [[Agents/app/app|app]]"),
        "{note}"
    );
    let handoff = note.find("## Handoff").expect("a handoff");
    let log = note.find("## Log").expect("a log");
    assert!(handoff < log, "the handoff comes before the log");
    assert!(
        note.contains("Login works.\n\nNext:\n- [ ] add remember-me\n- [ ] e2e test"),
        "{note}"
    );
    assert!(note.contains(" chose argon2\n"), "{note}");
    assert!(
        note.contains(" two\n  lines\n"),
        "a long entry stays one item: {note}"
    );
}

#[test]
fn the_next_agent_starts_where_the_last_one_stopped() {
    let vault = TempVault::new("mem-handover");
    mem_ok(&vault, "claude", &["start", "Add login"]);
    mem_ok(&vault, "claude", &["log", "chose argon2"]);
    mem_ok(
        &vault,
        "claude",
        &["end", "Login works.", "--next", "add remember-me"],
    );

    let brief = mem_ok(&vault, "codex", &["start", "Finish login"]);
    assert!(brief.contains("app: 1 session, 0 memories"), "{brief}");
    assert!(brief.contains("Last session: "), "{brief}");
    assert!(brief.contains(" claude, done, "), "{brief}");
    assert!(brief.contains("Task: Add login"), "{brief}");
    assert!(brief.contains("Handoff:\nLogin works."), "{brief}");
    assert!(
        brief.contains("Open tasks (1):\n- add remember-me ("),
        "{brief}"
    );
    assert!(brief.contains("Project note: empty."), "{brief}");

    let names = sessions(&vault);
    assert_eq!(names.len(), 2);
    let codex = names.iter().find(|n| n.contains("codex")).expect("codex's");
    let note = vault.read(&format!("Agents/app/Sessions/{codex}"));
    assert!(
        note.contains("· Previous: [[Agents/app/Sessions/"),
        "{note}"
    );

    // codex's log goes to codex's session, not to claude's closed one.
    mem_ok(&vault, "codex", &["log", "picked it up"]);
    assert!(
        vault
            .read(&format!("Agents/app/Sessions/{codex}"))
            .contains("picked it up")
    );
}

#[test]
fn logging_without_a_session_says_how_to_start_one() {
    let vault = TempVault::new("mem-nosession");
    let message = match mem(&vault, "claude", &["log", "hello"]) {
        Err(Failure::Failed(message)) => message,
        other => panic!("expected a failure, got {:?}", other.map(|o| o.text)),
    };
    assert!(message.contains("mem start"), "{message}");

    mem_ok(&vault, "claude", &["start", "x"]);
    mem_ok(&vault, "claude", &["end", "done"]);
    assert!(
        matches!(
            mem(&vault, "claude", &["log", "late"]),
            Err(Failure::Failed(_))
        ),
        "a finished session takes no more entries"
    );
}

#[test]
fn a_task_called_log_doesnt_confuse_the_log_with_the_title() {
    let vault = TempVault::new("mem-log-title");
    mem_ok(&vault, "claude", &["start", "Log"]);
    mem_ok(&vault, "claude", &["log", "first entry"]);
    mem_ok(&vault, "claude", &["end", "all done"]);
    let name = sessions(&vault).remove(0);
    let note = vault.read(&format!("Agents/app/Sessions/{name}"));
    let title = note.find("# Log\n").expect("the title");
    let handoff = note.find("## Handoff").expect("the handoff");
    let log = note.find("## Log").expect("the log");
    assert!(title < handoff && handoff < log, "{note}");
    assert!(note.trim_end().ends_with(" first entry"), "{note}");
}

#[test]
fn a_log_entry_lands_in_the_log_even_with_a_section_after_it() {
    let vault = TempVault::new("mem-log-middle");
    mem_ok(&vault, "claude", &["start", "x"]);
    let name = sessions(&vault).remove(0);
    let rel = format!("Agents/app/Sessions/{name}");
    let mut note = vault.read(&rel);
    note.push_str("- 10:00 first\n\n## Scratch\n\nhand-written\n");
    vault.write(&rel, &note);

    mem_ok(&vault, "claude", &["log", "second"]);
    let note = vault.read(&rel);
    let second = note.find(" second").expect("logged");
    assert!(second < note.find("## Scratch").expect("kept"), "{note}");
}

#[test]
fn remembering_creates_appends_and_replaces() {
    let vault = sample();
    mem_ok(&vault, "claude", &["start", "x"]);
    let said = mem_ok(
        &vault,
        "claude",
        &[
            "remember",
            "Hashing: which",
            "argon2id",
            "--tag",
            "decision",
            "--link",
            "Alpha",
        ],
    );
    assert_eq!(said, "remembered Agents/app/Memory/Hashing- which");
    let rel = "Agents/app/Memory/Hashing- which.md";
    let note = vault.read(rel);
    assert!(note.contains("type: memory"), "{note}");
    assert!(note.contains("tags:\n  - decision"), "{note}");
    assert!(
        note.contains("Project: [[Agents/app/app|app]] · Related: [[Alpha]]"),
        "{note}"
    );
    assert!(note.ends_with("argon2id\n"), "{note}");

    mem_ok(
        &vault,
        "claude",
        &[
            "remember",
            "Hashing: which",
            "params: default",
            "--tag",
            "security",
        ],
    );
    let note = vault.read(rel);
    assert!(note.ends_with("argon2id\n\nparams: default\n"), "{note}");
    assert!(note.contains("tags:\n  - decision\n  - security"), "{note}");
    assert!(note.contains("updated: "), "{note}");

    mem_ok(
        &vault,
        "claude",
        &["remember", "Hashing: which", "only this", "--replace"],
    );
    let note = vault.read(rel);
    assert!(
        note.ends_with("only this\n") && !note.contains("argon2id"),
        "{note}"
    );
    assert!(
        note.contains("tags:\n  - decision\n  - security"),
        "replacing the text keeps the tags: {note}"
    );

    let session = sessions(&vault).remove(0);
    let log = vault.read(&format!("Agents/app/Sessions/{session}"));
    assert!(
        log.contains("remembered [[Agents/app/Memory/Hashing- which|Hashing- which]]"),
        "the session links to what it remembered: {log}"
    );
}

#[test]
fn recall_ranks_what_was_kept_above_the_log_it_came_from() {
    let vault = TempVault::new("mem-recall");
    mem_ok(&vault, "claude", &["start", "x"]);
    mem_ok(&vault, "claude", &["log", "release needs a tag push"]);
    mem_ok(
        &vault,
        "claude",
        &["remember", "Release", "push a tag; CI does the rest"],
    );

    let found = mem_ok(&vault, "claude", &["recall", "tag", "push"]);
    let first = found.lines().next().unwrap_or_default();
    assert!(first.starts_with("Agents/app/Memory/Release ("), "{found}");
    assert!(
        found.contains("/Sessions/"),
        "the log is searched too: {found}"
    );
    assert!(
        mem_ok(&vault, "claude", &["recall", "zebra"]).starts_with("nothing remembered"),
        "no word of it anywhere"
    );
}

#[test]
fn done_ticks_the_one_task_that_matches() {
    let vault = TempVault::new("mem-done");
    mem_ok(&vault, "claude", &["start", "x"]);
    mem_ok(
        &vault,
        "claude",
        &["end", "s", "--next", "write docs", "--next", "write tests"],
    );
    mem_ok(&vault, "codex", &["start", "y"]);

    assert!(
        matches!(mem(&vault, "codex", &["done", "write"]), Err(Failure::Failed(m)) if m.contains("2 open tasks match"))
    );
    assert!(mem_ok(&vault, "codex", &["done", "docs"]).starts_with("ticked `write docs`"));
    let resumed = mem_ok(&vault, "codex", &["resume"]);
    assert!(
        resumed.contains("Open tasks (1):\n- write tests"),
        "{resumed}"
    );
    assert!(
        resumed.contains("done: write docs"),
        "logged in codex's session: {resumed}"
    );
}

#[test]
fn sessions_are_listed_newest_first() {
    let vault = TempVault::new("mem-list");
    mem_ok(&vault, "claude", &["start", "first task"]);
    mem_ok(&vault, "claude", &["end", "ok"]);
    // A later start than the first, whatever minute it is.
    let names = sessions(&vault);
    let stem = names[0].trim_end_matches(".md");
    vault.write(
        "Agents/app/Sessions/2999-01-01 0000 codex.md",
        "---\nagent: codex\nstatus: active\n---\n# second task\n\n## Log\n\n",
    );
    let listed = mem_ok(&vault, "codex", &["sessions"]);
    let lines: Vec<&str> = listed.lines().collect();
    assert!(
        lines[0].starts_with("2999-01-01 0000 codex, active"),
        "{listed}"
    );
    assert!(lines[1].starts_with(&format!("{stem}, done")), "{listed}");
    assert!(lines[1].ends_with(": first task"), "{listed}");
}

#[test]
fn the_project_note_is_shown_once_it_says_something() {
    let vault = TempVault::new("mem-note");
    mem_ok(&vault, "claude", &["start", "x"]);
    ok(
        &vault,
        &[
            "write",
            "Agents/app/app",
            "# app\n\nRust. Run `cargo test`.",
        ],
    );
    let brief = mem_ok(&vault, "claude", &["resume"]);
    assert!(
        brief.contains("Project note:\nRust. Run `cargo test`."),
        "{brief}"
    );
}

#[test]
fn the_memory_folder_comes_from_the_config() {
    let vault = TempVault::new("mem-folder");
    ok(&vault, &["config", "memory.folder", ""]);
    mem_ok(&vault, "claude", &["start", "x"]);
    assert!(vault.exists("app/app.md"), "at the vault's root");
}

#[test]
fn a_worktree_belongs_to_its_repository() {
    let root = TempVault::new("mem-git");
    let main = root.path().join("project");
    fs::create_dir_all(main.join(".git/worktrees/project-fix")).unwrap();
    let worktree = root.path().join("project-fix");
    fs::create_dir_all(worktree.join("src")).unwrap();
    fs::write(
        worktree.join(".git"),
        format!(
            "gitdir: {}\n",
            main.join(".git/worktrees/project-fix").display()
        ),
    )
    .unwrap();
    let submodule = main.join("vendor/lib");
    fs::create_dir_all(&submodule).unwrap();
    fs::write(submodule.join(".git"), "gitdir: ../../.git/modules/lib\n").unwrap();

    assert_eq!(
        mem::repository_name(&main.join("deep/er")).as_deref(),
        Some("project")
    );
    assert_eq!(
        mem::repository_name(&worktree.join("src")).as_deref(),
        Some("project")
    );
    assert_eq!(mem::repository_name(&submodule).as_deref(), Some("lib"));
}

// ---- settings ----------------------------------------------------------------------

#[test]
fn a_setting_is_read_and_changed_in_place() {
    let vault = TempVault::new("cmd-config");
    assert_eq!(ok(&vault, &["config", "editor.daily_folder"]), "Daily");
    assert_eq!(
        ok(&vault, &["config", "editor.daily_folder", "Journal"]),
        "editor.daily_folder = \"Journal\""
    );
    assert_eq!(ok(&vault, &["config", "editor.daily_folder"]), "Journal");
    assert_eq!(
        ok(&vault, &["config", "ui.show_chat", "true"]),
        "ui.show_chat = true"
    );
    assert_eq!(ok(&vault, &["config", "vault"]), "", "unset until set");

    let file = vault.read(".config/config.toml");
    assert!(file.contains("daily_folder = \"Journal\""), "{file}");
    assert!(
        file.contains("[memory]"),
        "the defaults were written first: {file}"
    );

    assert!(failure(&vault, &["config", "ui.sidebar_widht", "3"]).contains("no setting"));
    assert!(failure(&vault, &["config", "ui.sidebar_width", "wide"]).contains("isn't a value"));
    assert_eq!(
        ok(&vault, &["config", "editor.daily_folder"]),
        "Journal",
        "a refused change leaves the file alone"
    );
}

#[test]
fn a_comment_beside_a_setting_survives_a_change() {
    let vault = TempVault::new("cmd-config-comment");
    vault.write(
        ".config/config.toml",
        "# mine\ntheme = \"nord\" # the dark one\n\n[editor]\n# where days go\ndaily_folder = \"Daily\"\n",
    );
    ok(&vault, &["config", "editor.daily_folder", "Journal"]);
    ok(&vault, &["config", "theme", "gruvbox-dark"]);
    assert_eq!(
        vault.read(".config/config.toml"),
        "# mine\ntheme = \"gruvbox-dark\" # the dark one\n\n[editor]\n# where days go\ndaily_folder = \"Journal\"\n"
    );
}

// ---- help ----------------------------------------------------------------------------

#[test]
fn every_command_has_help_and_appears_in_the_overview() {
    let overview = overview();
    for spec in COMMANDS {
        assert!(
            overview.contains(&format!("    {} ", spec.name)),
            "`{}` is missing from the overview",
            spec.name
        );
        let usage = usage(spec);
        assert!(usage.starts_with(&format!("usage: emeraldian {}", spec.name)));
        assert!(
            usage.contains("--vault <PATH>"),
            "global options are listed"
        );
    }
}

#[test]
fn json_is_on_offer_for_every_command() {
    let vault = sample();
    for words in [
        vec!["read", "Alpha", "--json"],
        vec!["list", "--json"],
        vec!["graph", "--json"],
        vec!["tasks", "--json"],
    ] {
        let output = exec(&vault, &words).expect("runs");
        assert!(output.json.is_object(), "{words:?}");
    }
}

// ---- found in review ------------------------------------------------------------

#[test]
fn a_link_set_as_a_property_stays_a_link() {
    let vault = sample();
    ok(&vault, &["props", "Beta", "--set", "related=[[Alpha]]"]);
    assert_eq!(ok(&vault, &["props", "Beta", "related"]), "[[Alpha]]");
    assert!(
        vault
            .read("Beta.md")
            .starts_with("---\nrelated: \"[[Alpha]]\"\n---\n")
    );

    ok(
        &vault,
        &["props", "Beta", "--set", "both=[[[Alpha]], [[Beta]]]"],
    );
    assert_eq!(
        ok(&vault, &["props", "Beta", "both"]),
        "[[Alpha]]\n[[Beta]]"
    );
}

#[test]
fn dots_in_a_path_are_worked_out_before_it_is_used() {
    let vault = sample();
    vault.write("Sub/Note.md", "# Note\n");
    vault.write("Linker.md", "[[Sub/Note]]\n");
    assert_eq!(
        ok(&vault, &["move", "Sub/Note", "."]),
        "moved Sub/Note to Note"
    );
    assert!(vault.exists("Note.md"));
    assert_eq!(vault.read("Linker.md"), "[[Note]]\n", "not `[[./Note]]`");

    assert_eq!(ok(&vault, &["new", "./Inbox/../Fresh"]), "created Fresh");
    ok(&vault, &["mkdir", "A/./B"]);
    assert!(vault.path().join("A/B").is_dir());
    assert!(failure(&vault, &["move", "Fresh", "../.."]).contains("outside the vault"));
}

#[test]
fn an_agent_logs_to_the_session_it_started_where_it_works() {
    let vault = TempVault::new("mem-workdir");
    mem_ok(&vault, "claude", &["start", "mine"]);
    let mine = sessions(&vault).remove(0);
    // Another agent of the same name, in another worktree, starts after.
    let theirs = "2999-01-01 0000 claude.md";
    vault.write(
        &format!("Agents/app/Sessions/{theirs}"),
        "---\nagent: claude\nstatus: active\nworkdir: /somewhere/else\n---\n# theirs\n\n## Log\n\n",
    );

    mem_ok(&vault, "claude", &["log", "my entry"]);
    assert!(
        vault
            .read(&format!("Agents/app/Sessions/{mine}"))
            .contains("my entry")
    );
    assert!(
        !vault
            .read(&format!("Agents/app/Sessions/{theirs}"))
            .contains("my entry")
    );
}

#[test]
fn an_agent_named_only_at_the_start_still_finds_its_session() {
    let vault = TempVault::new("mem-unnamed");
    mem_ok(&vault, "claude", &["start", "x"]);
    exec(&vault, &["mem", "log", "unnamed entry", "--project", "app"]).expect("logs");
    exec(&vault, &["mem", "end", "finished", "--project", "app"]).expect("ends");
    let name = sessions(&vault).remove(0);
    let note = vault.read(&format!("Agents/app/Sessions/{name}"));
    assert!(
        note.contains("unnamed entry") && note.contains("status: done"),
        "{note}"
    );
}

#[test]
fn headings_and_fences_in_logged_text_leave_the_log_whole() {
    let vault = TempVault::new("mem-defuse");
    mem_ok(&vault, "claude", &["start", "x"]);
    mem_ok(
        &vault,
        "claude",
        &["log", "Findings:\n## Root cause\nthe cache"],
    );
    mem_ok(&vault, "claude", &["log", "```\nunclosed"]);
    mem_ok(&vault, "claude", &["log", "after"]);
    mem_ok(&vault, "claude", &["end", "# Summary\nall good"]);

    let name = sessions(&vault).remove(0);
    let note = vault.read(&format!("Agents/app/Sessions/{name}"));
    assert!(note.contains("\n  \\## Root cause\n"), "{note}");
    assert!(note.contains("\\# Summary"), "{note}");
    let brief = mem_ok(&vault, "claude", &["resume"]);
    assert!(brief.contains("Handoff:\n\\# Summary\nall good"), "{brief}");
    assert!(brief.contains("Root cause the cache"), "{brief}");
    assert!(
        brief.trim_end().contains(" after"),
        "the last entry survives: {brief}"
    );
}

#[test]
fn done_prefers_the_exact_task_and_ticks_every_copy_of_it() {
    let vault = TempVault::new("mem-done-exact");
    mem_ok(&vault, "claude", &["start", "x"]);
    mem_ok(
        &vault,
        "claude",
        &[
            "end",
            "s",
            "--next",
            "Fix CI",
            "--next",
            "Fix CI on Windows",
            "--next",
            "write tests",
            "--status",
            "paused",
        ],
    );
    mem_ok(&vault, "codex", &["start", "y"]);
    mem_ok(
        &vault,
        "codex",
        &["end", "t", "--next", "write tests", "--status", "paused"],
    );

    assert!(mem_ok(&vault, "codex", &["done", "fix ci"]).starts_with("ticked `Fix CI`"));
    let ticked = mem_ok(&vault, "codex", &["done", "write tests"]);
    assert!(ticked.ends_with("(2 copies)"), "{ticked}");
    let left = ok(&vault, &["tasks", "Agents/app"]);
    assert_eq!(left.lines().count(), 1, "{left}");
    assert!(left.ends_with(" Fix CI on Windows"), "{left}");

    mem_ok(&vault, "codex", &["start", "z"]);
    mem_ok(
        &vault,
        "codex",
        &[
            "end",
            "u",
            "--next",
            "Fix CI on Linux",
            "--status",
            "paused",
        ],
    );
    let message = match mem(&vault, "codex", &["done", "fix ci on"]) {
        Err(Failure::Failed(message)) => message,
        other => panic!("expected a failure, got {:?}", other.map(|o| o.text)),
    };
    assert!(message.contains("emeraldian task"), "{message}");
    assert!(
        message.contains("Agents/app/Sessions/"),
        "where each one is: {message}"
    );
}

#[cfg(unix)]
#[test]
fn a_linked_note_is_written_through_and_keeps_its_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let vault = sample();
    let outside = TempVault::new("cmd-link-target");
    let target = outside.write("Real.md", "# Real\n\nold\n");
    std::os::unix::fs::symlink(&target, vault.path().join("Linked.md")).expect("symlink");

    ok(&vault, &["edit", "Linked", "--old", "old", "--new", "new"]);
    assert_eq!(
        outside.read("Real.md"),
        "# Real\n\nnew\n",
        "the file it points at changed"
    );
    assert!(
        fs::symlink_metadata(vault.path().join("Linked.md"))
            .expect("still there")
            .file_type()
            .is_symlink(),
        "and it is still a link"
    );

    let private = vault.write("Private.md", "secret\n");
    fs::set_permissions(&private, fs::Permissions::from_mode(0o600)).expect("chmod");
    ok(&vault, &["write", "Private", "still secret"]);
    let mode = fs::metadata(&private).expect("exists").permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn a_link_and_a_read_agree_on_which_note_a_name_means() {
    let vault = TempVault::new("cmd-alias");
    vault.write("Roadmap.md", "---\naliases: [Plan]\n---\n# Roadmap\n");
    vault.write("Projects/Plan.md", "# Plan\n");
    vault.write("From.md", "x\n");
    assert!(ok(&vault, &["read", "Plan"]).starts_with("# Plan"));
    ok(&vault, &["link", "From", "Plan"]);
    assert!(
        vault.read("From.md").ends_with("[[Plan]]\n"),
        "the file, not the alias"
    );
    assert_eq!(
        ok(&vault, &["path", "Plan"]),
        ok(&vault, &["path", "Projects/Plan"])
    );
}

#[test]
fn every_part_of_links_and_resume_is_capped() {
    let vault = TempVault::new("cmd-caps");
    let missing: String = (0..30).map(|i| format!("[[Missing {i}]] ")).collect();
    vault.write("Many.md", &format!("{missing}\n"));
    let links = ok(&vault, &["links", "Many", "--limit", "5"]);
    assert!(links.contains("unresolved (30): Missing 0,"), "{links}");
    assert!(links.contains("… 25 more"), "{links}");

    mem_ok(&vault, "claude", &["start", "x"]);
    let page = "word ".repeat(2_000);
    mem_ok(&vault, "claude", &["log", &page]);
    let brief = mem_ok(&vault, "claude", &["resume"]);
    assert!(
        brief.len() < 2_000,
        "a page logged is a line resumed: {} bytes",
        brief.len()
    );
}

#[test]
fn recall_looks_at_what_was_written_not_at_the_frontmatter() {
    let vault = TempVault::new("mem-recall-body");
    mem_ok(&vault, "claude", &["start", "x"]);
    mem_ok(
        &vault,
        "claude",
        &["remember", "Build", "cargo test runs everything"],
    );
    // Words that every session's frontmatter, or an unwritten project note,
    // would otherwise answer to.
    for word in ["session", "active", "workdir", "conventions"] {
        let found = mem_ok(&vault, "claude", &["recall", word]);
        assert!(
            found.starts_with("nothing remembered"),
            "`{word}` matched: {found}"
        );
    }
    assert!(mem_ok(&vault, "claude", &["recall", "cargo"]).contains("Memory/Build"));
}

#[test]
fn a_heading_ending_in_a_hash_is_its_own_section() {
    let vault = TempVault::new("cmd-csharp");
    vault.write("Langs.md", "# Langs\n\n## C#\n\nold\n\n## F#\n\nfun\n");
    assert_eq!(
        ok(&vault, &["read", "Langs", "--section", "C#"]),
        "## C#\n\nold\n"
    );
    ok(&vault, &["write", "Langs", "--section", "C#", "new"]);
    assert_eq!(
        vault.read("Langs.md"),
        "# Langs\n\n## C#\n\nnew\n\n## F#\n\nfun\n"
    );
}

#[test]
fn a_task_with_no_words_can_be_ticked() {
    let vault = TempVault::new("cmd-empty-task");
    vault.write("T.md", "- [ ]\n");
    assert_eq!(ok(&vault, &["tasks"]), "T:1 ");
    ok(&vault, &["task", "T", "1"]);
    assert_eq!(vault.read("T.md"), "- [x]\n");
}

#[test]
fn help_is_asked_for_only_where_an_option_could_be() {
    let vault = sample();
    ok(
        &vault,
        &["edit", "Alpha", "--old", "A needle", "--new", "-h"],
    );
    assert!(
        vault.read("Alpha.md").contains("-h here."),
        "`-h` was the new text"
    );
    let raw = words(&["read", "-h"]);
    let (spec, rest) = find(&raw).unwrap();
    assert!(parse(spec, rest).expect("parses").flag("help"));
    let raw = words(&["read", "--help"]);
    let (spec, rest) = find(&raw).unwrap();
    assert!(
        parse(spec, rest)
            .expect("parses without a NOTE")
            .flag("help")
    );
}

#[test]
fn lines_past_the_end_say_how_many_there_are() {
    let vault = sample();
    assert!(failure(&vault, &["read", "Beta", "--lines", "4"]).contains("Beta has 3 lines"));
    assert!(failure(&vault, &["read", "Beta", "--lines", "9:"]).contains("Beta has 3 lines"));
    assert!(matches!(
        exec(&vault, &["read", "Beta", "--lines", "3:2"]),
        Err(Failure::Usage(_))
    ));
    assert_eq!(
        ok(&vault, &["read", "Beta", "--lines", "3:99"]),
        "Back to [[Alpha]]. Then [[Projects/Gamma]]."
    );
}

// ---- recall: words, meaning and links ------------------------------------------------

/// A vault indexed with the stand-in model, which compares words, not
/// meanings: so the notes share words with the questions put to them.
fn indexed() -> TempVault {
    let vault = TempVault::new("cmd-recall");
    vault.write(
        "Hashing.md",
        "# Hashing\n\nPasswords are hashed with argon2 before storage.\n\nSee [[Security]].\n",
    );
    vault.write(
        "Login.md",
        "# Login\n\nThe login form checks passwords hashed in storage.\n\n[[Hashing]] and [[Security]].\n",
    );
    vault.write(
        "Deploy.md",
        "# Deploy\n\nPush a tag and the release workflow builds.\n",
    );
    vault.write("Security.md", "# Security\n\nThreat model.\n");
    ok(&vault, &["index"]);
    vault
}

/// The stand-in model, failing as a stopped server does.
fn broken() -> Box<dyn emeraldian_core::retrieval::Embed> {
    Box::new(emeraldian_core::retrieval::fake::Words {
        broken: true,
        ..Default::default()
    })
}

#[test]
fn index_reads_every_note_once_and_then_only_what_changed() {
    let vault = indexed();
    assert!(
        vault.path().join(".cache/retrieval").is_dir(),
        "kept out of the vault"
    );
    let again = ok(&vault, &["index"]);
    assert!(again.starts_with("4 notes, 4 passages indexed"), "{again}");
    assert!(!again.contains(" read,"), "nothing to do: {again}");

    vault.write("Deploy.md", "# Deploy\n\nTags start the release.\n");
    let changed = ok(&vault, &["index"]);
    assert!(changed.contains("1 read, 1 embedded"), "{changed}");

    let status = ok(&vault, &["index", "--status"]);
    assert!(
        status.contains("4 notes, 4 passages, 4 with meaning; 0 changed since"),
        "{status}"
    );
    assert!(status.contains(": ready"), "{status}");
}

#[test]
fn recall_answers_by_words_before_anything_is_indexed() {
    let vault = sample();
    let output = exec(&vault, &["recall", "needle"]).expect("runs");
    assert!(output.text.starts_with("Alpha  "), "{}", output.text);
    assert!(
        output.text.contains("17: A needle here."),
        "{}",
        output.text
    );
}

#[test]
fn recall_ranks_by_meaning_and_words_and_shows_the_links() {
    let vault = indexed();
    let found = ok(&vault, &["recall", "passwords hashed storage"]);
    assert!(
        found.contains("Hashing  ") && found.contains("Login  "),
        "{found}"
    );
    assert!(
        found.contains("links Hashing"),
        "the link between answers: {found}"
    );
    assert!(!found.contains("Deploy"), "nothing to do with it: {found}");
}

#[test]
fn recall_with_a_budget_returns_the_passages_themselves() {
    let vault = indexed();
    let packed = ok(
        &vault,
        &["recall", "passwords hashed storage", "--budget", "60"],
    );
    assert!(packed.starts_with("### "), "{packed}");
    assert!(
        packed.contains("Passwords are hashed with argon2"),
        "{packed}"
    );
    assert!(
        emeraldian_core::retrieval::text::tokens(&packed) <= 80,
        "kept near the budget: {packed}"
    );
    assert!(failure(&vault, &["recall", "x", "--budget", "5"]).contains("--budget"));
}

#[test]
fn a_budget_gives_a_long_line_once_though_it_is_two_passages() {
    let vault = indexed();
    vault.write("Long.md", &"Rotate the signing keys. ".repeat(60));
    let packed = ok(
        &vault,
        &["recall", "rotate signing keys", "--budget", "1000"],
    );
    assert_eq!(
        packed.matches("### Long:1-1\n").count(),
        2,
        "each passage under its own heading: {packed}"
    );
    assert_eq!(
        packed.matches("Rotate the signing keys.").count(),
        60,
        "and none of the line twice: {packed}"
    );
}

#[test]
fn a_budget_cuts_a_passage_short_after_a_word_rather_than_give_nothing() {
    let vault = indexed();
    // One paragraph on one line, as Obsidian writes them: some 5,000 bytes,
    // five passages, each well within the budget.
    let line: String = (0..100)
        .map(|step| format!("Rotate the signing keys, step {step}, and record it. "))
        .collect();
    vault.write("Long.md", &line);
    let output = exec(
        &vault,
        &["recall", "rotate signing keys", "--budget", "1000"],
    )
    .expect("runs");
    assert!(output.text.starts_with("### Long:1-1\n"), "{}", output.text);
    let given = output.json["passages"].as_array().expect("passages");
    assert!(given.len() > 1, "{}", output.json);
    for step in 0..100 {
        assert!(
            output.text.matches(&format!("step {step},")).count() <= 1,
            "each passage its own part of the line: {}",
            output.text
        );
    }
    let last = given
        .last()
        .and_then(|p| p["text"].as_str())
        .unwrap_or_default();
    let kept = last.strip_suffix("…\n").expect("the last cut short");
    assert!(line.contains(&format!("{kept} ")), "after a word: {last:?}");
    let spent = output.json["tokens"].as_u64().unwrap_or(u64::MAX);
    assert!(spent <= 1000, "{}", output.json);
    assert!(
        emeraldian_core::retrieval::text::tokens(&output.text) as u64 <= spent,
        "{}",
        output.json
    );
}

#[test]
fn a_budget_gives_a_drawing_by_its_words_not_the_scene_it_is_kept_as() {
    let vault = indexed();
    let shapes: String = (0..40)
        .map(|i| {
            format!(
                "{{\"type\":\"rectangle\",\"x\":{i},\"y\":{i},\"width\":10,\"height\":10,\"seed\":123456789{i},\"versionNonce\":987654321{i}}},"
            )
        })
        .collect();
    let drawing = format!(
        "---\nexcalidraw-plugin: parsed\ntags: [excalidraw]\n---\n==⚠  Switch to EXCALIDRAW VIEW in the MORE OPTIONS menu of this document. ⚠==\n\n# Excalidraw Data\n\n## Text Elements\nRotate the signing keys yearly ^abc123\n\n%%\n## Drawing\n```json\n{{\"type\":\"excalidraw\",\"elements\":[{shapes}{{\"type\":\"text\",\"text\":\"Rotate the signing keys yearly\",\"x\":1,\"y\":2}}]}}\n```\n%%\n"
    );
    vault.write("Keys.excalidraw.md", &drawing);
    let packed = ok(
        &vault,
        &["recall", "rotate signing keys yearly", "--budget", "300"],
    );
    let lines = drawing.lines().count();
    assert_eq!(
        packed,
        format!("### Keys.excalidraw:1-{lines}\nRotate the signing keys yearly"),
        "the words in it, not its frontmatter, banner or scene"
    );
}

#[test]
fn a_drawing_found_shows_the_words_in_it_not_a_line_of_its_scene() {
    let vault = indexed();
    vault.write(
        "Keys.excalidraw.md",
        "---\nexcalidraw-plugin: parsed\ntags: [excalidraw]\n---\n==⚠  Switch to EXCALIDRAW VIEW in the MORE OPTIONS menu of this document. ⚠==\n\n# Excalidraw Data\n\n## Text Elements\nRotate the signing keys yearly ^abc123\n\n%%\n## Drawing\n```json\n{\"type\":\"excalidraw\",\"elements\":[{\"type\":\"text\",\"text\":\"Rotate the signing keys yearly\",\"x\":1,\"y\":2},{\"type\":\"text\",\"text\":\"Vault layout\",\"x\":5,\"y\":9}]}\n```\n%%\n",
    );
    let found = ok(&vault, &["recall", "rotate signing keys yearly"]);
    assert!(
        found.contains("Keys.excalidraw") && found.contains("1: Rotate the signing keys yearly"),
        "{found}"
    );
    assert!(!found.contains("EXCALIDRAW VIEW"), "{found}");
}

#[test]
fn a_budget_counts_the_links_and_leaves_them_out_where_they_dont_fit() {
    let vault = indexed();
    let question = "passwords hashed storage";
    let roomy = exec(&vault, &["recall", question, "--budget", "500"]).expect("runs");
    let links = roomy.text.lines().last().unwrap_or_default();
    assert!(links.starts_with("links: "), "{}", roomy.text);
    let spent = roomy.json["tokens"].as_u64().expect("tokens");
    assert!(
        emeraldian_core::retrieval::text::tokens(&roomy.text) as u64 <= spent,
        "the links counted: {}",
        roomy.json
    );

    // Room for every passage, and not for the links after them.
    let tight = (spent - 1).to_string();
    let output = exec(&vault, &["recall", question, "--budget", &tight]).expect("runs");
    assert_eq!(
        output.text,
        roomy
            .text
            .strip_suffix(links)
            .unwrap_or_default()
            .trim_end(),
        "the passages whole, without the links"
    );
    assert_eq!(
        output.json["links"],
        serde_json::json!([]),
        "{}",
        output.json
    );
    assert!(output.json["tokens"].as_u64() < Some(spent));
}

#[test]
fn the_line_shown_has_the_question_s_words_and_the_rarer_of_them() {
    let auth = "# Auth\n\nThe password hashing uses argon2.\nThis is how the old system worked, the history of it.\n";
    assert_eq!(
        super::retrieve::best_line(auth, 3, 4, "how is the password hashed").map(|(n, _)| n),
        Some(3),
        "its words, as the index reads them, not the small words holding it together"
    );
    let rules = "Password length: twelve.\nPassword expiry: never.\nKeys are hashed with argon2.\n";
    assert_eq!(
        super::retrieve::best_line(rules, 1, 3, "password argon2").map(|(n, _)| n),
        Some(3),
        "a word in one line of three says more than one in two"
    );
}

#[test]
fn a_folder_named_in_another_case_is_asked_as_the_disk_spells_it() {
    let vault = indexed();
    vault.write(
        "Projects/Keys.md",
        "# Keys\n\nRotate the signing keys yearly.\n",
    );
    ok(&vault, &["index"]);
    let scoped = json(
        &vault,
        &[
            "recall",
            "rotate signing keys",
            "--folder",
            "projects",
            "--json",
        ],
    );
    let found: Vec<&str> = scoped["notes"]
        .as_array()
        .expect("notes")
        .iter()
        .map(|n| n["note"].as_str().unwrap_or_default())
        .collect();
    // Where the filesystem ignores case, as macOS's does, `projects` is the
    // folder `Projects`; where it doesn't, there is no such folder.
    let ignores_case = vault.path().join("projects").is_dir();
    let expected: &[&str] = if ignores_case {
        &["Projects/Keys.md"]
    } else {
        &[]
    };
    assert_eq!(found, expected, "{scoped}");
    let status = ok(&vault, &["index", "--status"]);
    assert!(
        status.contains("5 notes, 5 passages, 5 with meaning; 0 changed since"),
        "no second copy under the name typed: {status}"
    );
}

#[test]
fn a_note_the_model_has_read_only_in_part_answers_at_the_floor() {
    let vault = TempVault::new("cmd-recall-partial");
    let note = |i: usize, revised: &str| {
        format!("# N{i:02}\n\nRelease notes number {i} about the build{revised}.\n")
    };
    for i in 0..26 {
        vault.write(&format!("N{i:02}.md"), &note(i, ""));
    }
    let pad = "Rotate the signing keys every year with care and record when. ".repeat(5);
    vault.write("X.md", &format!("## Keys\n\n{pad}\n\nSee [[Y]].\n"));
    vault.write("Y.md", "# Y\n\nNothing much here at all.\n");
    ok(&vault, &["index"]);
    // A sync touches more notes than a question embeds, and X gains a
    // section on something new, which the model doesn't get to.
    for i in 0..26 {
        vault.write(&format!("N{i:02}.md"), &note(i, ", revised"));
    }
    vault.write(
        "X.md",
        &format!("## Keys\n\n{pad}\n\nSee [[Y]].\n\n## Zebra\n\nZebra quantum flux capacitor.\n"),
    );
    let output = exec(&vault, &["recall", "zebra quantum flux capacitor"]).expect("runs");
    assert!(
        output
            .notice
            .as_deref()
            .unwrap_or_default()
            .contains("matched by words"),
        "{:?}",
        output.notice
    );
    assert_eq!(
        output.text, "X  0.70\n  9: Zebra quantum flux capacitor.",
        "at the floor, and a little more for its words"
    );
}

#[test]
fn a_new_note_is_recalled_straight_away() {
    let vault = indexed();
    vault.write(
        "Rotation.md",
        "# Rotation\n\nRotate the argon2 pepper yearly.\n",
    );
    let found = ok(&vault, &["recall", "argon2 pepper rotation"]);
    assert!(found.starts_with("Rotation  "), "{found}");
}

#[test]
fn without_the_model_recall_answers_by_words_and_says_so() {
    let vault = indexed();
    let output = exec_with(&vault, &["recall", "argon2"], broken()).expect("runs");
    assert!(output.text.starts_with("Hashing  "), "{}", output.text);
    assert!(
        output
            .notice
            .as_deref()
            .unwrap_or_default()
            .contains("meaning is unavailable"),
        "{:?}",
        output.notice
    );
}

#[test]
fn similar_notes_say_whether_they_are_linked_yet() {
    let vault = indexed();
    vault.write(
        "Hashing notes.md",
        "# Hashing notes\n\nPasswords hashed with argon2 before storage, notes.\n",
    );
    let found = ok(&vault, &["similar", "Hashing"]);
    let lines: Vec<&str> = found.lines().collect();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("Hashing notes  ") && !l.ends_with("linked")),
        "{found}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("Login  ") && l.ends_with("  linked")),
        "{found}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("Hashing  ")),
        "not itself: {found}"
    );
}

#[test]
fn mem_recall_finds_by_meaning_what_the_words_alone_miss() {
    let vault = indexed();
    mem_ok(&vault, "claude", &["start", "x"]);
    mem_ok(
        &vault,
        "claude",
        &[
            "remember",
            "Hashing choice",
            "argon2 keeps passwords stored securely",
        ],
    );
    let found = mem_ok(
        &vault,
        "claude",
        &["recall", "how are passwords stored securely"],
    );
    assert!(
        found.starts_with("Agents/app/Memory/Hashing choice ("),
        "{found}"
    );
    assert!(
        found.contains(": argon2 keeps passwords stored securely"),
        "{found}"
    );
}

#[test]
fn mem_recall_says_how_to_turn_meaning_on_only_when_it_finds_nothing() {
    let vault = TempVault::new("mem-recall-off");
    mem_ok(&vault, "claude", &["start", "x"]);
    mem_ok(&vault, "claude", &["remember", "Hashing", "argon2"]);
    assert!(!mem_ok(&vault, "claude", &["recall", "argon2"]).contains("index"));
    let nothing = mem_ok(&vault, "claude", &["recall", "zebra"]);
    assert!(
        nothing.contains("`emeraldian index` turns it on"),
        "{nothing}"
    );
}

#[test]
fn mem_recall_with_a_budget_packs_memories() {
    let vault = indexed();
    mem_ok(&vault, "claude", &["start", "x"]);
    mem_ok(
        &vault,
        "claude",
        &["remember", "Release", "push a tag; CI does the rest"],
    );
    let packed = mem_ok(
        &vault,
        "claude",
        &["recall", "push a tag", "--budget", "80"],
    );
    assert!(
        packed.starts_with("### Agents/app/Memory/Release:"),
        "{packed}"
    );
    assert!(
        packed.contains(":8-10\npush a tag; CI does the rest"),
        "the passage, without the project line or the blank line after it: {packed}"
    );
}

#[test]
fn mem_recall_puts_a_memory_above_a_log_that_says_the_same() {
    let vault = TempVault::new("mem-recall-prior");
    mem_ok(&vault, "claude", &["start", "x"]);
    // The same words under the same name: as relevant as each other.
    let same = "Rotate the signing keys every quarter.\n";
    vault.write("Agents/app/Memory/Rotation.md", same);
    vault.write("Agents/app/Sessions/Rotation.md", same);
    ok(&vault, &["index"]);
    let found = mem(
        &vault,
        "claude",
        &["recall", "rotate signing keys", "--json"],
    )
    .expect("runs")
    .json;
    assert_eq!(found["semantic"], true, "{found}");
    let results = found["results"].as_array().expect("results");
    let notes: Vec<&str> = results
        .iter()
        .map(|r| r["note"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        notes,
        [
            "Agents/app/Memory/Rotation.md",
            "Agents/app/Sessions/Rotation.md"
        ],
        "{found}"
    );
    let lead = results[0]["score"].as_f64().unwrap_or_default()
        - results[1]["score"].as_f64().unwrap_or_default();
    assert!(
        (0.02..0.03).contains(&lead),
        "what was kept leads by its kind alone, a nudge on a cosine: {found}"
    );
}

#[test]
fn mem_recall_tells_a_memory_from_a_log_by_where_it_is_in_the_project() {
    // A [memory] folder called Memory, and a project called Memory: a log
    // in either is still a log.
    for (folder, project) in [("Notes/Memory", "app"), ("Agents", "Memory")] {
        let vault = TempVault::new("mem-recall-kinds");
        ok(&vault, &["config", "memory.folder", folder]);
        let same = "Rotate the signing keys every quarter.\n";
        let memory = format!("{folder}/{project}/Memory/Rotation.md");
        vault.write(&memory, same);
        vault.write(&format!("{folder}/{project}/Sessions/Rotation.md"), same);
        ok(&vault, &["index"]);
        let found = json(
            &vault,
            &[
                "mem",
                "recall",
                "rotate signing keys",
                "--json",
                "--project",
                project,
                "--agent",
                "claude",
            ],
        );
        let results = found["results"].as_array().expect("results");
        assert_eq!(results[0]["note"], memory.as_str(), "{found}");
        let lead = results[0]["score"].as_f64().unwrap_or_default()
            - results[1]["score"].as_f64().unwrap_or_default();
        assert!((0.02..0.03).contains(&lead), "{folder}, {project}: {found}");
    }
}

#[test]
fn a_changed_model_means_indexing_again() {
    let vault = indexed();
    ok(
        &vault,
        &["config", "embeddings.document_prompt", "passage: "],
    );
    let output = exec(&vault, &["recall", "passwords"]).expect("runs");
    assert!(
        output
            .notice
            .as_deref()
            .unwrap_or_default()
            .contains("`emeraldian index`"),
        "{:?}",
        output.notice
    );
    ok(&vault, &["index"]);
    let found = ok(&vault, &["recall", "passwords"]);
    assert!(found.contains("Hashing  "), "{found}");
}

// ---- recall through a server ------------------------------------------------------

/// A stand-in for Ollama, over HTTP: it answers `/api/embed` with the
/// stand-in model's vectors, and keeps a log of what came, in order — a
/// question, a batch of passages — and of each question answered.
struct Served {
    url: String,
    log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Served {
    /// A server; one that fails every question when `refuse` is set, as a
    /// model that has gone wrong does.
    fn start(refuse: bool) -> Self {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let kept = std::sync::Arc::clone(&log);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let log = std::sync::Arc::clone(&kept);
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                    let mut stream = stream;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 {
                            return;
                        }
                        let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                        let (mut length, mut chunked) = (0usize, false);
                        loop {
                            let mut header = String::new();
                            reader.read_line(&mut header).expect("header");
                            let header = header.trim().to_ascii_lowercase();
                            if header.is_empty() {
                                break;
                            }
                            if let Some(value) = header.strip_prefix("content-length:") {
                                length = value.trim().parse().unwrap_or(0);
                            }
                            if header.starts_with("transfer-encoding:")
                                && header.contains("chunked")
                            {
                                chunked = true;
                            }
                        }
                        let mut body = Vec::new();
                        if chunked {
                            loop {
                                let mut size = String::new();
                                reader.read_line(&mut size).expect("size");
                                let size = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
                                let mut chunk = vec![0u8; size + 2];
                                reader.read_exact(&mut chunk).expect("chunk");
                                if size == 0 {
                                    break;
                                }
                                body.extend_from_slice(&chunk[..size]);
                            }
                        } else {
                            body.resize(length, 0);
                            reader.read_exact(&mut body).expect("body");
                        }
                        let (status, reply) = if path.ends_with("/api/embed") {
                            let request: Value = serde_json::from_slice(&body).expect("json");
                            let texts: Vec<String> = request["input"]
                                .as_array()
                                .expect("input")
                                .iter()
                                .map(|t| t.as_str().unwrap_or_default().to_string())
                                .collect();
                            let question =
                                texts.len() == 1 && texts[0].starts_with("task: search result");
                            log.lock().expect("log").push(if question {
                                "question".to_string()
                            } else {
                                format!("passages {}", texts.len())
                            });
                            if question && refuse {
                                (500, r#"{"error":"the model went wrong"}"#.to_string())
                            } else {
                                let vectors: Vec<Vec<f32>> = texts
                                    .iter()
                                    .map(|t| emeraldian_core::retrieval::fake::vector(t))
                                    .collect();
                                if question {
                                    log.lock().expect("log").push("answered".into());
                                }
                                (
                                    200,
                                    serde_json::json!({ "embeddings": vectors }).to_string(),
                                )
                            }
                        } else {
                            (200, r#"{"version":"0.0.0"}"#.to_string())
                        };
                        let response = format!(
                            "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{reply}",
                            reply.len()
                        );
                        if stream.write_all(response.as_bytes()).is_err() {
                            return;
                        }
                    }
                });
            }
        });
        Self { url, log }
    }

    fn log(&self) -> Vec<String> {
        self.log.lock().expect("log").clone()
    }
}

#[test]
fn a_question_goes_to_the_server_first_and_its_answer_is_the_same() {
    let vault = indexed();
    let served = Served::start(false);
    ok(&vault, &["config", "embeddings.base_url", &served.url]);
    // A note the refresh has to embed, so passages go to the model too.
    vault.write(
        "Rotation.md",
        "# Rotation\n\nRotate the argon2 pepper yearly.\n",
    );

    let by_server = exec_served(&vault, &["recall", "argon2 pepper rotation"]).expect("runs");
    assert!(by_server.notice.is_none(), "{:?}", by_server.notice);
    assert!(
        by_server.text.starts_with("Rotation  "),
        "{}",
        by_server.text
    );
    assert_eq!(
        served.log(),
        ["question", "answered", "passages 1"],
        "the question first, and the passages only once it is answered"
    );

    // The same question of the same index, by the stand-in in process.
    let in_process = ok(&vault, &["recall", "argon2 pepper rotation"]);
    assert_eq!(by_server.text, in_process);

    // Asked again, its vector is remembered.
    let again = exec_served(&vault, &["recall", "argon2 pepper rotation"]).expect("runs");
    assert_eq!(again.text, by_server.text);
    assert_eq!(
        served.log().len(),
        3,
        "no question sent: {:?}",
        served.log()
    );
}

#[test]
fn a_question_the_server_fails_is_answered_by_words_and_the_refresh_does_not_wait_on_it() {
    let vault = indexed();
    let served = Served::start(true);
    ok(&vault, &["config", "embeddings.base_url", &served.url]);
    vault.write(
        "Rotation.md",
        "# Rotation\n\nRotate the argon2 pepper yearly.\n",
    );
    let output = exec_served(&vault, &["recall", "argon2 pepper"]).expect("runs");
    assert!(
        output.text.starts_with("Rotation  ") || output.text.contains("Rotation"),
        "{}",
        output.text
    );
    assert!(
        output
            .notice
            .as_deref()
            .unwrap_or_default()
            .contains("meaning is unavailable"),
        "{:?}",
        output.notice
    );
    assert_eq!(
        served.log(),
        ["question"],
        "no passages sent to a model that failed"
    );
}
