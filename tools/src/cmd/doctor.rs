//! Diagnosing and repairing the shell's `shell.json`.
//!
//! The shell dumps its own config schema into the environment before running
//! this, so nothing here hardcodes what the config looks like: it is always
//! checked against what the running shell actually accepts.
//!
//! Repairs are deliberately conservative. A key close to a real one is
//! renamed, one with no match is dropped (the shell ignores it anyway), a
//! wrong type is converted when the conversion is unambiguous and otherwise
//! removed so the shell's own default takes over. The original is always
//! kept beside the file first.
//!
//! Ported from `assets/config-doctor.py`.

use std::path::{Path, PathBuf};

use crate::difflib;
use crate::jsonval::{self, Value};

/// The words the shell's schema walk produces for a leaf; anything else in
/// the schema is a subtree to descend into.
const LEAF_TYPES: [&str; 5] = ["boolean", "number", "string", "array", "map"];

/// One thing wrong with the config, and what to do about it.
struct Issue {
    severity: &'static str,
    problem: String,
    action: String,
    fix: Fix,
}

/// Where in the tree a fix applies. `bar.entries` is addressed by index
/// because its items are positional.
enum Fix {
    /// Move a key to a new name, which puts it last — the same as popping and
    /// reinserting into a dict.
    Rename { path: Vec<String>, from: String, to: String },
    Remove { path: Vec<String>, key: String },
    Replace { path: Vec<String>, key: String, value: Value },
    EntryId { index: usize, id: String },
    EntryEnabled { index: usize, value: Value },
    EntryEnabledRemoved { index: usize },
    EntryDropped { index: usize },
}

/// Can this value be turned into what the schema wants without guessing?
fn coerce(value: &Value, expected: &str) -> Option<Value> {
    match expected {
        "boolean" => match value {
            Value::Num { value, .. } if *value == 0.0 || *value == 1.0 => Some(Value::Bool(*value == 1.0)),
            Value::Str(text) => {
                let word = text.trim().to_lowercase();
                match word.as_str() {
                    "true" | "yes" | "on" | "1" => Some(Value::Bool(true)),
                    "false" | "no" | "off" | "0" => Some(Value::Bool(false)),
                    _ => None,
                }
            }
            _ => None,
        },
        "number" => match value {
            Value::Str(text) => {
                let number: f64 = text.trim().parse().ok()?;
                Some(if number.fract() == 0.0 && number.abs() < 9e15 {
                    Value::int(number as i64)
                } else {
                    Value::float(number)
                })
            }
            _ => None,
        },
        "string" => match value {
            Value::Bool(v) => Some(Value::string(if *v { "true" } else { "false" })),
            Value::Num { literal, .. } => Some(Value::string(literal.clone())),
            _ => None,
        },
        _ => None,
    }
}

struct Doctor<'a> {
    schema: &'a Value,
    bar_ids: Vec<&'a str>,
    issues: Vec<Issue>,
}

impl<'a> Doctor<'a> {
    fn check(&mut self, config: &Value) {
        self.walk(config, self.schema, "", &[]);
    }

    fn walk(&mut self, node: &Value, schema: &Value, path: &str, steps: &[String]) {
        let Some(pairs) = node.as_object() else { return };
        for (key, value) in pairs {
            let where_ = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
            let expected = schema.get(key);

            match expected {
                None => self.unknown_key(node, schema, key, &where_, steps),
                Some(Value::Obj(_)) => {
                    if matches!(value, Value::Obj(_)) {
                        let mut deeper = steps.to_vec();
                        deeper.push(key.clone());
                        self.walk(value, expected.unwrap(), &where_, &deeper);
                    } else {
                        self.bad_type(key, &where_, "object", value, steps);
                    }
                }
                Some(Value::Str(word)) if word == "map" || word == "array" => {
                    if where_ == "bar.entries" {
                        if let Value::Arr(entries) = value {
                            self.check_entries(entries);
                            continue;
                        }
                    }
                    // What is inside a map or a free array is the user's own
                    // data; only the shape of the container is checked.
                    if !matches!(value, Value::Obj(_) | Value::Arr(_)) {
                        self.bad_type(key, &where_, word, value, steps);
                    }
                }
                Some(Value::Str(word)) if LEAF_TYPES.contains(&word.as_str()) => {
                    if value.type_name() != word {
                        self.bad_type(key, &where_, word, value, steps);
                    }
                }
                _ => {}
            }
        }
    }

    fn unknown_key(&mut self, node: &Value, schema: &Value, key: &str, where_: &str, steps: &[String]) {
        let taken: Vec<&str> = node.as_object().map(|p| p.iter().map(|(k, _)| k.as_str()).collect()).unwrap_or_default();
        let candidates: Vec<&str> = schema
            .as_object()
            .map(|p| p.iter().map(|(k, _)| k.as_str()).filter(|k| !taken.contains(k)).collect())
            .unwrap_or_default();

        match difflib::closest_match(key, candidates, 0.6) {
            Some(match_) => self.issues.push(Issue {
                severity: "warn",
                problem: format!("{where_}: unknown setting (typo?)"),
                action: format!("rename '{key}' to '{match_}'"),
                fix: Fix::Rename { path: steps.to_vec(), from: key.to_string(), to: match_.to_string() },
            }),
            None => self.issues.push(Issue {
                severity: "warn",
                problem: format!("{where_}: unknown setting, ignored by the shell"),
                action: format!("remove '{key}'"),
                fix: Fix::Remove { path: steps.to_vec(), key: key.to_string() },
            }),
        }
    }

    fn bad_type(&mut self, key: &str, where_: &str, expected: &str, value: &Value, steps: &[String]) {
        match coerce(value, expected) {
            Some(coerced) => self.issues.push(Issue {
                severity: "warn",
                problem: format!(
                    "{where_}: is {} {}, the shell expects {expected}",
                    value.type_name(),
                    value.dump()
                ),
                action: format!("change to {}", coerced.dump()),
                fix: Fix::Replace { path: steps.to_vec(), key: key.to_string(), value: coerced },
            }),
            None => self.issues.push(Issue {
                severity: "warn",
                problem: format!(
                    "{where_}: is {}, the shell expects {expected} and falls back to its default",
                    value.type_name()
                ),
                action: format!("remove '{key}' (the default takes over)"),
                fix: Fix::Remove { path: steps.to_vec(), key: key.to_string() },
            }),
        }
    }

    /// The bar's entry list is the one place the schema cannot describe: each
    /// item names a module, and only the shell knows which names exist.
    fn check_entries(&mut self, entries: &[Value]) {
        for (index, entry) in entries.iter().enumerate() {
            let where_ = format!("bar.entries[{index}]");
            let id = entry.get("id").and_then(Value::as_str);
            let Some(id) = id else {
                self.issues.push(Issue {
                    severity: "warn",
                    problem: format!("{where_}: not an {{\"id\": …}} object — the bar cannot render it"),
                    action: "remove this entry".to_string(),
                    fix: Fix::EntryDropped { index },
                });
                continue;
            };

            if !self.bar_ids.contains(&id) {
                match difflib::closest_match(id, self.bar_ids.iter().copied(), 0.6) {
                    Some(match_) => self.issues.push(Issue {
                        severity: "warn",
                        problem: format!("{where_}: id \"{id}\" is not a bar module (typo?)"),
                        action: format!("rename to \"{match_}\""),
                        fix: Fix::EntryId { index, id: match_.to_string() },
                    }),
                    None => self.issues.push(Issue {
                        severity: "warn",
                        problem: format!(
                            "{where_}: id \"{id}\" is not a bar module — silently dropped. Valid: {}",
                            self.bar_ids.join(", ")
                        ),
                        action: "remove this entry".to_string(),
                        fix: Fix::EntryDropped { index },
                    }),
                }
            }

            if let Some(enabled) = entry.get("enabled") {
                if !matches!(enabled, Value::Bool(_)) {
                    match coerce(enabled, "boolean") {
                        Some(coerced) => self.issues.push(Issue {
                            severity: "warn",
                            problem: format!(
                                "{where_}: enabled is {}, must be true/false",
                                enabled.dump()
                            ),
                            action: format!("change to {}", coerced.dump()),
                            fix: Fix::EntryEnabled { index, value: coerced },
                        }),
                        None => self.issues.push(Issue {
                            severity: "warn",
                            problem: format!(
                                "{where_}: enabled is {}, must be true/false",
                                enabled.dump()
                            ),
                            action: "remove 'enabled' (entry stays visible)".to_string(),
                            fix: Fix::EntryEnabledRemoved { index },
                        }),
                    }
                }
            }
        }
    }
}

fn node_at<'a>(root: &'a mut Value, path: &[String]) -> Option<&'a mut Value> {
    let mut node = root;
    for step in path {
        let pairs = node.as_object_mut()?;
        node = &mut pairs.iter_mut().find(|(k, _)| k == step)?.1;
    }
    Some(node)
}

fn bar_entries(root: &mut Value) -> Option<&mut Vec<Value>> {
    node_at(root, &["bar".to_string()])?.as_object_mut()?.iter_mut().find(|(k, _)| k == "entries")?.1.as_array_mut()
}

/// Applies one round of fixes. Dropped entries go last and in reverse, so the
/// indices reported while checking stay valid until then.
fn apply(config: &mut Value, issues: &[Issue]) {
    let mut dropped = Vec::new();
    for issue in issues {
        match &issue.fix {
            Fix::Rename { path, from, to } => {
                if let Some(pairs) = node_at(config, path).and_then(Value::as_object_mut) {
                    if let Some(at) = pairs.iter().position(|(k, _)| k == from) {
                        let (_, value) = pairs.remove(at);
                        pairs.retain(|(k, _)| k != to);
                        pairs.push((to.clone(), value));
                    }
                }
            }
            Fix::Remove { path, key } => {
                if let Some(pairs) = node_at(config, path).and_then(Value::as_object_mut) {
                    pairs.retain(|(k, _)| k != key);
                }
            }
            Fix::Replace { path, key, value } => {
                if let Some(pairs) = node_at(config, path).and_then(Value::as_object_mut) {
                    if let Some(slot) = pairs.iter_mut().find(|(k, _)| k == key) {
                        slot.1 = value.clone();
                    }
                }
            }
            Fix::EntryId { index, id } => {
                if let Some(entry) = bar_entries(config).and_then(|e| e.get_mut(*index)) {
                    if let Some(pairs) = entry.as_object_mut() {
                        if let Some(slot) = pairs.iter_mut().find(|(k, _)| k == "id") {
                            slot.1 = Value::string(id.clone());
                        }
                    }
                }
            }
            Fix::EntryEnabled { index, value } => {
                if let Some(entry) = bar_entries(config).and_then(|e| e.get_mut(*index)) {
                    if let Some(pairs) = entry.as_object_mut() {
                        if let Some(slot) = pairs.iter_mut().find(|(k, _)| k == "enabled") {
                            slot.1 = value.clone();
                        }
                    }
                }
            }
            Fix::EntryEnabledRemoved { index } => {
                if let Some(entry) = bar_entries(config).and_then(|e| e.get_mut(*index)) {
                    if let Some(pairs) = entry.as_object_mut() {
                        pairs.retain(|(k, _)| k != "enabled");
                    }
                }
            }
            Fix::EntryDropped { index } => dropped.push(*index),
        }
    }

    if dropped.is_empty() {
        return;
    }
    dropped.sort_unstable();
    dropped.dedup();
    if let Some(entries) = bar_entries(config) {
        for index in dropped.into_iter().rev() {
            if index < entries.len() {
                entries.remove(index);
            }
        }
    }
}

/// `<path>.<suffix>`, with a number appended if that is taken.
fn backup_path(path: &Path, suffix: &str) -> PathBuf {
    let base = PathBuf::from(format!("{}.{suffix}", path.display()));
    if !base.exists() {
        return base;
    }
    for n in 1.. {
        let candidate = PathBuf::from(format!("{}.{n}", base.display()));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn run(args: &[String]) -> i32 {
    let Some(config_path) = args.first() else {
        eprintln!("usage: config-doctor <shell.json> [--repair]");
        return 2;
    };
    let config_path = PathBuf::from(config_path);
    let repair = args[1..].iter().any(|a| a == "--repair");

    let raw = std::env::var("CAELESTIA_SCHEMA")
        .or_else(|_| std::env::var("CAELESTIA_FIX"))
        .unwrap_or_default();
    let payload = match jsonval::parse(&raw) {
        Ok(payload) => payload,
        Err(e) => {
            eprintln!("config-doctor: the shell's schema is not readable: {e}");
            return 1;
        }
    };
    let (Some(schema), Some(Value::Arr(ids))) = (payload.get("types"), payload.get("barIds")) else {
        eprintln!("config-doctor: the shell's schema has no types or barIds");
        return 1;
    };
    let bar_ids: Vec<&str> = ids.iter().filter_map(Value::as_str).collect();

    if !config_path.exists() {
        println!("clean");
        return 0;
    }
    let text = match std::fs::read_to_string(&config_path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("config-doctor: {}: {e}", config_path.display());
            return 1;
        }
    };

    let mut reformat = false;
    let mut config = match jsonval::parse(&text) {
        Ok(config) => config,
        Err(strict) => match jsonval::parse_tolerant(&text) {
            Ok(config) => {
                reformat = true;
                config
            }
            Err(_) => return move_aside(&config_path, repair, &format!("not valid JSON: {strict}"), &strict),
        },
    };

    if !matches!(config, Value::Obj(_)) {
        let what = config.type_name();
        if repair {
            let aside = backup_path(&config_path, "broken.bak");
            let _ = std::fs::rename(&config_path, &aside);
            println!("top level is {what}, not an object; moved to {}", file_name(&aside));
            return 0;
        }
        println!("issue|fail|top level is {what}, the shell needs an object|move the file aside and regenerate defaults");
        return 0;
    }

    // Fixing one thing can expose the next — a renamed section becomes
    // checkable — so this converges over a few passes, and the plan it prints
    // is the complete one.
    let mut found: Vec<(&'static str, String, String)> = Vec::new();
    if reformat {
        found.push((
            "warn",
            "not strict JSON (comments or trailing commas)".to_string(),
            "rewrite as clean JSON".to_string(),
        ));
    }
    for _ in 0..4 {
        let mut doctor = Doctor { schema, bar_ids: bar_ids.clone(), issues: Vec::new() };
        doctor.check(&config);
        if doctor.issues.is_empty() {
            break;
        }
        found.extend(doctor.issues.iter().map(|i| (i.severity, i.problem.clone(), i.action.clone())));
        apply(&mut config, &doctor.issues);
    }

    if found.is_empty() {
        println!("clean");
        return 0;
    }
    if !repair {
        for (severity, problem, action) in &found {
            println!("issue|{severity}|{problem}|{action}");
        }
        return 0;
    }

    let kept = backup_path(&config_path, "doctor-bak");
    if let Err(e) = std::fs::copy(&config_path, &kept) {
        eprintln!("config-doctor: cannot keep the original: {e}");
        return 1;
    }
    println!("original kept as {}", file_name(&kept));
    for (_, problem, action) in &found {
        println!("fixed: {problem} -> {action}");
    }
    if let Err(e) = std::fs::write(&config_path, config.dump_indented(2) + "\n") {
        eprintln!("config-doctor: cannot write the repaired config: {e}");
        return 1;
    }
    println!("wrote repaired {} ({} fixes)", file_name(&config_path), found.len());
    0
}

/// A file that is not JSON at all cannot be repaired in place; it goes aside
/// so the shell writes itself a fresh default.
fn move_aside(path: &Path, repair: bool, _problem: &str, strict: &str) -> i32 {
    if repair {
        let aside = backup_path(path, "broken.bak");
        let _ = std::fs::rename(path, &aside);
        println!(
            "not repairable as JSON ({strict}); moved to {} — the shell regenerates defaults",
            file_name(&aside)
        );
        return 0;
    }
    println!("issue|fail|not valid JSON: {strict}|move the file aside (a .broken.bak copy stays) and let the shell regenerate defaults");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coercion_only_happens_when_there_is_one_answer() {
        assert_eq!(coerce(&Value::int(1), "boolean"), Some(Value::Bool(true)));
        assert_eq!(coerce(&Value::int(2), "boolean"), None);
        assert_eq!(coerce(&Value::string(" On "), "boolean"), Some(Value::Bool(true)));
        assert_eq!(coerce(&Value::string("maybe"), "boolean"), None);
        assert_eq!(coerce(&Value::string("12"), "number"), Some(Value::int(12)));
        assert_eq!(coerce(&Value::string("1.5"), "number"), Some(Value::float(1.5)));
        assert_eq!(coerce(&Value::string("lots"), "number"), None);
        assert_eq!(coerce(&Value::Bool(false), "string"), Some(Value::string("false")));
        assert_eq!(coerce(&Value::int(3), "string"), Some(Value::string("3")));
        assert_eq!(coerce(&Value::Arr(vec![]), "string"), None);
    }

    #[test]
    fn a_renamed_key_moves_to_the_end() {
        let mut config = jsonval::parse(r#"{"a": 1, "typo": 2, "c": 3}"#).unwrap();
        apply(
            &mut config,
            &[Issue {
                severity: "warn",
                problem: String::new(),
                action: String::new(),
                fix: Fix::Rename { path: vec![], from: "typo".into(), to: "b".into() },
            }],
        );
        assert_eq!(config.dump(), r#"{"a": 1, "c": 3, "b": 2}"#);
    }

    #[test]
    fn dropped_entries_are_removed_back_to_front() {
        let mut config = jsonval::parse(r#"{"bar": {"entries": [{"id": "a"}, {"id": "b"}, {"id": "c"}]}}"#).unwrap();
        apply(
            &mut config,
            &[
                Issue { severity: "warn", problem: String::new(), action: String::new(), fix: Fix::EntryDropped { index: 0 } },
                Issue { severity: "warn", problem: String::new(), action: String::new(), fix: Fix::EntryDropped { index: 2 } },
            ],
        );
        assert_eq!(config.dump(), r#"{"bar": {"entries": [{"id": "b"}]}}"#);
    }
}
