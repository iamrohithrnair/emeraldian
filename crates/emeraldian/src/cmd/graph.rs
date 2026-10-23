//! `emeraldian graph`: the vault's graph, as text an agent can read or data
//! another tool can draw.
//!
//! The same graph the app shows — built by the same code, with the same
//! nodes for notes that don't exist yet — so what a script reports is what a
//! person would see.

use std::collections::HashSet;

use emeraldian_core::graph::{Graph, GraphOptions, NodeKind, Simulation};
use emeraldian_core::index::{LinkTarget, VaultIndex};
use serde_json::{Value, json};

use super::{Args, Ctx, Failure, Output, Result, cap, display, fail, lines};

/// The graph's options, from the filters on the command line.
///
/// The app's own toggles under `[graph]` are deliberately not read: they are
/// about what is pleasant to look at, and a script asking for orphans should
/// get them even if whoever set up the app hid them from view.
fn options(args: &Args) -> GraphOptions {
    GraphOptions {
        show_unresolved: !args.flag("no-unresolved"),
        show_tags: args.flag("tags"),
        show_attachments: args.flag("attachments"),
        show_orphans: !args.flag("no-orphans"),
        tag_filter: args
            .value("tag")
            .map(|tag| tag.trim().trim_start_matches('#').to_string()),
    }
}

/// How a node is named in text: a note by its path, so the name can be handed
/// straight to another command; anything else by its label, with what it is.
fn name(index: &VaultIndex, graph: &Graph, node: usize) -> String {
    let node = &graph.nodes[node];
    match node.kind {
        NodeKind::Note(id) => index
            .note(id)
            .map_or_else(|| node.label.clone(), |n| display(&n.meta.rel).to_string()),
        NodeKind::Unresolved => format!("{} (unresolved)", node.label),
        NodeKind::Tag => node.label.clone(),
        NodeKind::Attachment => format!("{} (file)", node.label),
    }
}

/// `1 link`, `2 links`.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

fn kind(node: &NodeKind) -> &'static str {
    match node {
        NodeKind::Note(_) => "note",
        NodeKind::Unresolved => "unresolved",
        NodeKind::Tag => "tag",
        NodeKind::Attachment => "attachment",
    }
}

fn node_json(index: &VaultIndex, graph: &Graph, node: usize) -> Value {
    let n = &graph.nodes[node];
    let mut entry = json!({
        "id": node,
        "label": n.label,
        "kind": kind(&n.kind),
        "degree": n.degree,
    });
    if let NodeKind::Note(id) = n.kind
        && let Some(note) = index.note(id)
    {
        entry["note"] = json!(note.meta.rel);
    }
    entry
}

/// The graph node standing for the note `name`.
fn node_of(ctx: &mut Ctx, graph: &Graph, name: &str) -> Result<usize> {
    let found = ctx.locate(name)?;
    let index = ctx.index()?;
    index
        .id_of_rel(&found.rel)
        .and_then(|id| graph.node_of_note(id))
        .ok_or_else(|| Failure::Failed(format!("{} isn't in the graph as filtered", found.name())))
}

pub(super) fn summary(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let index = ctx.index()?;
    let graph = Graph::build(index, &options(args));

    let count = |wanted: &str| {
        graph
            .nodes
            .iter()
            .filter(|n| kind(&n.kind) == wanted)
            .count()
    };
    let notes = count("note");
    let groups = graph.components();
    let clusters = groups.iter().filter(|group| group.len() > 1).count();
    let largest = groups.first().map_or(0, Vec::len);
    let orphans = graph
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Note(_)) && n.degree == 0)
        .count();

    let mut hubs: Vec<usize> = (0..graph.nodes.len())
        .filter(|&n| matches!(graph.nodes[n].kind, NodeKind::Note(_)) && graph.nodes[n].degree > 0)
        .collect();
    hubs.sort_by(|&a, &b| {
        graph.nodes[b]
            .degree
            .cmp(&graph.nodes[a].degree)
            .then(a.cmp(&b))
    });
    hubs.truncate(5);

    let mut first = plural(notes, "note");
    for (what, label) in [
        ("unresolved", "unresolved"),
        ("tag", "tags"),
        ("attachment", "attachments"),
    ] {
        let n = count(what);
        if n > 0 {
            first.push_str(&format!(", {n} {label}"));
        }
    }
    first.push_str(&format!(
        ", {}, {} (largest {largest}), {}",
        plural(graph.edges.len(), "link"),
        plural(clusters, "cluster"),
        plural(orphans, "orphan")
    ));
    let mut text = vec![first];
    if !hubs.is_empty() {
        text.push(format!(
            "hubs: {}",
            hubs.iter()
                .map(|&n| format!("{} ({})", name(index, &graph, n), graph.nodes[n].degree))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let json = json!({
        "notes": notes,
        "unresolved": count("unresolved"),
        "tags": count("tag"),
        "attachments": count("attachment"),
        "links": graph.edges.len(),
        "clusters": clusters,
        "largest": largest,
        "orphans": orphans,
        "hubs": hubs.iter().map(|&n| node_json(index, &graph, n)).collect::<Vec<_>>(),
    });
    Ok(Output::new(text.join("\n"), json))
}

pub(super) fn local(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let depth = args.number("depth", 1)?;
    let limit = args.limit(50)?;
    let graph = Graph::build(ctx.index()?, &options(args));
    let origin = node_of(ctx, &graph, args.at(0))?;
    let index = ctx.index()?;

    let mut hops = graph.hops(origin, depth);
    let cut = cap(&mut hops, limit);
    let shown: HashSet<usize> = hops.iter().map(|&(node, _)| node).collect();

    let mut text: Vec<String> = hops
        .iter()
        .map(|&(node, hop)| {
            let n = &graph.nodes[node];
            match n.kind {
                NodeKind::Note(_) | NodeKind::Tag => {
                    format!("{hop} {} ({})", name(index, &graph, node), n.degree)
                }
                _ => format!("{hop} {} ({}, {})", n.label, kind(&n.kind), n.degree),
            }
        })
        .collect();
    if cut > 0 {
        text.push(super::more(cut));
    }

    let edges: Vec<(usize, usize)> = graph
        .edges
        .iter()
        .filter(|e| shown.contains(&e.from) && shown.contains(&e.to))
        .map(|e| (e.from, e.to))
        .collect();
    if args.flag("edges") {
        text.push(format!("links ({}):", edges.len()));
        for &(from, to) in &edges {
            text.push(format!(
                "  {} -- {}",
                name(index, &graph, from),
                name(index, &graph, to)
            ));
        }
    }

    let json = json!({
        "nodes": hops.iter().map(|&(node, hop)| {
            let mut entry = node_json(index, &graph, node);
            entry["hops"] = json!(hop);
            entry
        }).collect::<Vec<_>>(),
        "edges": edges.iter().map(|&(from, to)| json!({ "source": from, "target": to })).collect::<Vec<_>>(),
        "more": cut,
    });
    Ok(Output::new(text.join("\n"), json))
}

/// Whether node `from` links to node `to`.
///
/// The graph is undirected, as a layout has to be, so direction comes from the
/// index. Only a note can hold a link, so between a note and anything else —
/// a tag, a file, a note not yet written — the note is the one linking.
fn links_to(index: &VaultIndex, graph: &Graph, from: usize, to: usize) -> bool {
    let NodeKind::Note(source) = graph.nodes[from].kind else {
        return false;
    };
    match graph.nodes[to].kind {
        NodeKind::Note(target) => index.note(source).is_some_and(|note| {
            note.links
                .iter()
                .any(|link| link.target == LinkTarget::Note(target))
        }),
        _ => true,
    }
}

pub(super) fn path(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let graph = Graph::build(ctx.index()?, &options(args));
    let from = node_of(ctx, &graph, args.at(0))?;
    let to = node_of(ctx, &graph, args.at(1))?;
    let index = ctx.index()?;

    let Some(path) = graph.shortest_path(from, to) else {
        return fail(format!(
            "no chain of links joins {} and {}",
            name(index, &graph, from),
            name(index, &graph, to)
        ));
    };

    let mut text = name(index, &graph, path[0]);
    let mut steps = Vec::new();
    for pair in path.windows(2) {
        let arrow = match (
            links_to(index, &graph, pair[0], pair[1]),
            links_to(index, &graph, pair[1], pair[0]),
        ) {
            (true, true) => "<->",
            (false, true) => "<-",
            _ => "->",
        };
        steps.push(arrow);
        text.push_str(&format!(" {arrow} {}", name(index, &graph, pair[1])));
    }
    let json = json!({
        "path": path.iter().map(|&n| node_json(index, &graph, n)).collect::<Vec<_>>(),
        "steps": steps,
        "links": path.len() - 1,
    });
    Ok(Output::new(text, json))
}

pub(super) fn hubs(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let limit = args.limit(10)?;
    let index = ctx.index()?;
    let graph = Graph::build(index, &options(args));

    let mut hubs: Vec<usize> = (0..graph.nodes.len())
        .filter(|&n| matches!(graph.nodes[n].kind, NodeKind::Note(_)) && graph.nodes[n].degree > 0)
        .collect();
    hubs.sort_by(|&a, &b| {
        graph.nodes[b]
            .degree
            .cmp(&graph.nodes[a].degree)
            .then(a.cmp(&b))
    });
    let cut = cap(&mut hubs, limit);

    let text = if hubs.is_empty() {
        "no note has any links".to_string()
    } else {
        lines(
            hubs.iter()
                .map(|&n| format!("{} {}", name(index, &graph, n), graph.nodes[n].degree))
                .collect(),
            cut,
        )
    };
    let json = json!({
        "hubs": hubs.iter().map(|&n| node_json(index, &graph, n)).collect::<Vec<_>>(),
        "more": cut,
    });
    Ok(Output::new(text, json))
}

pub(super) fn orphans(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let limit = args.limit(50)?;
    let index = ctx.index()?;
    // A link to a note not yet written still connects a note, as it does in
    // the app's graph, so only a note with no link of any kind is alone.
    let graph = Graph::build(index, &GraphOptions::default());

    let mut alone: Vec<String> = graph
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| matches!(n.kind, NodeKind::Note(_)) && n.degree == 0)
        .map(|(node, _)| name(index, &graph, node))
        .collect();
    let cut = cap(&mut alone, limit);
    let text = if alone.is_empty() {
        "no orphans".to_string()
    } else {
        lines(alone.clone(), cut)
    };
    Ok(Output::new(text, json!({ "orphans": alone, "more": cut })))
}

pub(super) fn unresolved(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let limit = args.limit(50)?;
    let index = ctx.index()?;

    let mut missing: Vec<(&String, Vec<String>)> = index
        .unresolved()
        .iter()
        .map(|(target, sources)| {
            let names = sources
                .iter()
                .filter_map(|&id| index.note(id))
                .map(|note| display(&note.meta.rel).to_string())
                .collect();
            (target, names)
        })
        .collect();
    // The most wanted first: a note five others link to is the one to write.
    missing.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(b.0)));
    let cut = cap(&mut missing, limit);

    let text = if missing.is_empty() {
        "every link resolves".to_string()
    } else {
        lines(
            missing
                .iter()
                .map(|(target, sources)| {
                    let mut shown = sources.clone();
                    let more = cap(&mut shown, 5);
                    let mut line = format!("{target} <- {}", shown.join(", "));
                    if more > 0 {
                        line.push_str(&format!(", … {more} more"));
                    }
                    line
                })
                .collect(),
            cut,
        )
    };
    let json: Vec<Value> = missing
        .iter()
        .map(|(target, sources)| json!({ "name": target, "linked_from": sources }))
        .collect();
    Ok(Output::new(
        text,
        json!({ "unresolved": json, "more": cut }),
    ))
}

pub(super) fn export(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let format = args.value("format").unwrap_or("json");
    if !matches!(format, "json" | "dot" | "mermaid") {
        return Err(Failure::Usage(format!(
            "--format takes json, dot or mermaid, not `{format}`"
        )));
    }
    let index = ctx.index()?;
    let mut graph = Graph::build(index, &options(args));
    let layout = args.flag("layout");
    if layout {
        // Run until it settles, which it always does: the simulation cools
        // as it goes, and the app's own graph is this one, left to come to
        // rest.
        let mut simulation = Simulation::new(graph);
        simulation.run(usize::MAX);
        graph = simulation.graph;
    }

    let nodes: Vec<Value> = (0..graph.nodes.len())
        .map(|n| {
            let mut entry = node_json(index, &graph, n);
            if layout {
                entry["x"] = json!(graph.nodes[n].pos.x);
                entry["y"] = json!(graph.nodes[n].pos.y);
            }
            entry
        })
        .collect();
    let edges: Vec<Value> = graph
        .edges
        .iter()
        .map(|e| json!({ "source": e.from, "target": e.to }))
        .collect();
    let json = json!({ "nodes": nodes, "edges": edges });

    let label = |n: usize| match graph.nodes[n].kind {
        NodeKind::Note(id) => index.note(id).map_or_else(
            || graph.nodes[n].label.clone(),
            |note| display(&note.meta.rel).to_string(),
        ),
        _ => graph.nodes[n].label.clone(),
    };
    let text = match format {
        "dot" => {
            let mut out = String::from("graph vault {\n");
            for n in 0..graph.nodes.len() {
                let style = match graph.nodes[n].kind {
                    NodeKind::Note(_) => "",
                    NodeKind::Unresolved => ", style=dashed",
                    NodeKind::Tag => ", shape=plaintext",
                    NodeKind::Attachment => ", shape=box",
                };
                out.push_str(&format!(
                    "  n{n} [label=\"{}\"{style}];\n",
                    label(n).replace('\\', "\\\\").replace('"', "\\\"")
                ));
            }
            for e in &graph.edges {
                out.push_str(&format!("  n{} -- n{};\n", e.from, e.to));
            }
            out.push('}');
            out
        }
        "mermaid" => {
            let mut out = String::from("graph LR\n");
            for n in 0..graph.nodes.len() {
                out.push_str(&format!(
                    "  n{n}[\"{}\"]\n",
                    label(n).replace('"', "#quot;")
                ));
            }
            for e in &graph.edges {
                out.push_str(&format!("  n{} --- n{}\n", e.from, e.to));
            }
            out
        }
        _ => json.to_string(),
    };
    Ok(Output::new(text, json))
}
