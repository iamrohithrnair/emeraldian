---
name: emeraldian-memory
description: Keep and resume project state across agent sessions, and look things up in the user's notes, with the emeraldian CLI. Use at the start of work in a repository, before investigating something that may already be known, whenever you learn something a successor would need, and before you stop or run low on context.
---

# Project memory with emeraldian

Memory is plain notes in the user's Obsidian vault, under `Agents/<project>/`,
shared by every agent on the project. The project is the git repository's
name, so run commands from inside it.

## Setup, once

    brew install iamrohithrnair/tap/emeraldian   # or: cargo install emeraldian
    brew install --cask ollama-app               # macOS; else https://ollama.com/download. Start it.
    emeraldian index                             # downloads EmbeddingGemma 2 (about 300 MB) through Ollama, indexes the vault

The first `index` can take a while on a large vault: run it in the background.
Without Ollama everything still works, by words and links. To pin the vault:
`emeraldian config vault <PATH>` (`emeraldian vaults` lists Obsidian's).

## Start of a task

    emeraldian mem start "<task in a few words>" --agent claude

Prints the project note, the last handoff and log, open tasks and memory
titles. Carry on from there. To re-read state without opening a session
(mid-task, after compaction): `emeraldian mem resume`.

## Before digging into a problem

    emeraldian mem recall "<question>" --budget 800

Returns what was remembered and logged, as passages. `--all` covers every project.
An empty answer means nothing relevant is stored: carry on rather than rephrasing.

## While working

    emeraldian mem log "<one line>" --agent claude
    emeraldian mem remember "<Title>" "<durable fact>" --tag decision --agent claude
    emeraldian mem done "<words from an open task>" --agent claude

Log findings, decisions with their reasons, and dead ends. Remember what
outlasts the task. `-` in place of the text reads stdin.

## Before stopping, or when context runs low

    emeraldian mem end "<where things stand>" --next "<step>" --next "<step>" --agent claude

Add `--status paused` if unfinished, `--status blocked` if waiting on the user.
Branch, commit and uncommitted files are recorded for you.

## The rest of the vault

    emeraldian recall "<question>" --budget 800   # passages, by meaning, words and links
    emeraldian recall "<question>"                # notes, each with its best line
    emeraldian similar "<note>"                   # closest notes; `linked` marks existing links
    emeraldian search "<exact text>"              # literal text
    emeraldian read "<note>" --section "<heading>"

If a recall mentions `emeraldian index`, run it.

Keep the project note (`Agents/<project>/<project>`) short: it is shown at
every start. Change a line with `emeraldian edit <note> --old A --new B`, or
rewrite it with `emeraldian write <note> -` (text on stdin).

More: `emeraldian help mem`, `emeraldian help <command>`.
