//! `emeraldian config`: the settings in `config.toml`, read and changed one at
//! a time.

use std::fs;
use std::path::PathBuf;

use serde_json::json;

use super::{Args, Ctx, Failure, Output, Result, fail};
use crate::config::Config;

/// Settings that are absent until set, so they don't appear when the config
/// is written out with nothing in them.
const UNSET_BY_DEFAULT: &[&str] = &["vault", "agent.base_url"];

pub(super) fn config(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let path = ctx.config_path()?;
    if args.flag("path") {
        let shown = path.display().to_string();
        return Ok(Output::new(shown.clone(), json!({ "path": shown })));
    }

    let effective = toml::Value::try_from(&ctx.config)
        .map_err(|err| Failure::Failed(format!("could not read the settings: {err}")))?;
    match (args.get(0), args.get(1)) {
        (None, _) => {
            let text = toml::to_string_pretty(&ctx.config)
                .map_err(|err| Failure::Failed(err.to_string()))?;
            Ok(Output::new(
                text,
                serde_json::to_value(&ctx.config).unwrap_or_default(),
            ))
        }
        (Some(key), None) => match lookup(&effective, key) {
            Some(value) => Ok(Output::new(
                shown(value),
                json!({ "key": key, "value": serde_json::to_value(value).unwrap_or_default() }),
            )),
            None if UNSET_BY_DEFAULT.contains(&key) => {
                Ok(Output::new("", json!({ "key": key, "value": null })))
            }
            None => fail(format!("no setting called `{key}`")),
        },
        (Some(key), Some(raw)) => set(&path, key, raw),
    }
}

impl Ctx {
    /// Where the config file is, or would be.
    fn config_path(&self) -> Result<PathBuf> {
        if let Some(path) = &self.config_file {
            return Ok(path.clone());
        }
        Config::path()
            .ok_or_else(|| Failure::Failed("this platform has no config directory".into()))
    }
}

/// A dotted key's value in a table.
fn lookup<'a>(table: &'a toml::Value, key: &str) -> Option<&'a toml::Value> {
    key.split('.')
        .try_fold(table, |value, part| value.as_table()?.get(part))
}

/// A value as a person would type it back: text without its quotes.
fn shown(value: &toml::Value) -> String {
    match value {
        toml::Value::String(text) => text.clone(),
        toml::Value::Table(_) => toml::to_string_pretty(value).unwrap_or_default(),
        other => other.to_string(),
    }
}

/// Changes one setting, after checking that the app will still read the file
/// and that the key is one it reads at all.
fn set(path: &std::path::Path, key: &str, raw: &str) -> Result<Output> {
    if !path.exists() {
        // The documented defaults first, as the app writes them on its first
        // run, so the file the change lands in explains the rest.
        Config::default().save_to(path)?;
    }

    // TOML where it parses, so `true` and `3` are a switch and a number;
    // otherwise the words as text, so `nord` needs no quotes.
    let value: toml_edit::Value = raw
        .trim()
        .parse()
        .unwrap_or_else(|_| toml_edit::Value::from(raw));

    let text = fs::read_to_string(path)?;
    let mut document: toml_edit::DocumentMut = text.parse().map_err(|err| {
        Failure::Failed(format!(
            "{} doesn't parse, so it was left alone: {err}",
            path.display()
        ))
    })?;
    put(&mut document, key, value.clone())?;

    let edited = document.to_string();
    let config: Config = toml::from_str(&edited)
        .map_err(|err| Failure::Failed(format!("`{key} = {raw}` isn't a value it takes: {err}")))?;
    // An unknown key parses — every section takes defaults — but is then
    // dropped, so it is missing when the config is written back out.
    let reread = toml::Value::try_from(&config).map_err(|err| Failure::Failed(err.to_string()))?;
    if lookup(&reread, key).is_none() {
        return fail(format!("no setting called `{key}`"));
    }

    Config::edit_file(path, |document| {
        let _ = put(document, key, value.clone());
    })?;
    let value = value.to_string();
    let value = value.trim();
    Ok(Output::new(
        format!("{key} = {value}"),
        json!({ "key": key, "value": serde_json::to_value(lookup(&reread, key)).unwrap_or_default() }),
    ))
}

/// Puts a value at a dotted key, making the tables on the way. A value that is
/// already there is replaced in its place, so a comment above it stays.
fn put(document: &mut toml_edit::DocumentMut, key: &str, value: toml_edit::Value) -> Result<()> {
    let parts: Vec<&str> = key.split('.').map(str::trim).collect();
    if parts.iter().any(|part| part.is_empty()) {
        return Err(Failure::Usage(format!("`{key}` isn't a setting's name")));
    }
    let (last, tables) = parts.split_last().expect("split always yields one");

    let mut table: &mut dyn toml_edit::TableLike = document.as_table_mut();
    for part in tables {
        let item = table.entry(part).or_insert(toml_edit::table());
        table = item
            .as_table_like_mut()
            .ok_or_else(|| Failure::Failed(format!("`{part}` in `{key}` isn't a section")))?;
    }
    match table.get_mut(last) {
        Some(item) if item.is_value() => {
            let decor = item.as_value().map(|v| v.decor().clone());
            *item = toml_edit::Item::Value(value);
            if let (Some(decor), Some(value)) = (decor, item.as_value_mut()) {
                *value.decor_mut() = decor;
            }
        }
        Some(_) => return fail(format!("`{key}` is a section, not a setting")),
        None => {
            table.insert(last, toml_edit::Item::Value(value));
        }
    }
    Ok(())
}
