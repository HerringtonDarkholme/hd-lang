//! Generates the table behind `hd doc std.MODULE.ITEM` (`cli.doc.name.std`)
//! from the standard library reference, `spec/std/*.md`. The CLI must
//! answer offline, so the reference is read here, at build time, and
//! compiled into `hd_doc` as `OUT_DIR/std_table.rs`; nothing is read when
//! `hd` runs.
//!
//! An entry is a signature the reference writes in backticks and a `->`:
//! in a table row (`Rule | Method | Result` rows, `Receiver | Methods`
//! rows, and the like), or a `pub data`, `pub enum`, `pub trait` or
//! `pub type` declaration in a code block. Its summary is the first
//! sentence of the numbered rule named like it, or else the row's result
//! cell. Its reference link is that rule's anchor, or else the section's.

use std::fmt::Write as _;
use std::path::Path;

struct Entry {
    module: String,
    owner: String,
    name: String,
    signature: String,
    summary: String,
    anchor: String,
}

/// The cells of a table row, split at the `|` outside backticks.
fn cells(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut ticks = false;
    for c in line.trim().trim_start_matches('|').chars() {
        match c {
            '`' => {
                ticks = !ticks;
                cur.push(c);
            }
            '|' if !ticks => out.push(std::mem::take(&mut cur).trim().to_owned()),
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_owned());
    }
    out
}

/// The backtick spans of a cell.
fn spans(cell: &str) -> Vec<&str> {
    cell.split('`').skip(1).step_by(2).collect()
}

/// The rule id a text starts with or holds as `r[ID]`.
fn rule_id(text: &str) -> Option<&str> {
    let rest = &text[text.find("r[")? + 2..];
    Some(&rest[..rest.find(']')?])
}

/// `(name, signature)` of a function signature span: an optional `pub`
/// and `fn`, a name, optional generics, a parameter list and a `->`.
fn function(span: &str) -> Option<(String, String)> {
    let s = span.trim();
    let body = s.strip_prefix("pub ").unwrap_or(s);
    let body = body.strip_prefix("fn ").unwrap_or(body);
    let name: String = body
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '!'))
        .collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let mut rest = &body[name.len()..];
    if rest.starts_with('[') {
        let mut depth = 0;
        let mut end = None;
        for (i, c) in rest.char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = &rest[end?..];
    }
    (rest.starts_with('(') && rest.contains("->")).then(|| (name, s.to_owned()))
}

/// The first sentence of a text, ended with a period.
fn sentence(text: &str) -> String {
    let text = text.trim();
    let mut end = text.len();
    let mut ticks = false;
    for (i, c) in text.char_indices() {
        if c == '`' {
            ticks = !ticks;
        }
        if !ticks
            && matches!(c, '.' | '!' | '?')
            && text[i + 1..].chars().next().is_none_or(char::is_whitespace)
        {
            end = i + 1;
            break;
        }
    }
    let mut out = text[..end].to_owned();
    if let Some(first) = out.chars().next() {
        let upper: String = first.to_uppercase().collect();
        out.replace_range(..first.len_utf8(), &upper);
    }
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

/// The anchor GitHub-style renderers give a heading.
fn slug(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

/// The type a receiver cell names: `List` for `` `List[T]`, when `T < Eq` ``.
fn owner_of(cell: &str) -> String {
    spans(cell)
        .first()
        .map(|s| {
            s.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect()
        })
        .unwrap_or_default()
}

fn read_module(module: &str, text: &str, out: &mut Vec<Entry>) {
    let file = format!("spec/std/{module}.md");
    // Numbered rules: (id, first sentence).
    let mut rules: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        let Some((num, rest)) = t.split_once(". ") else {
            continue;
        };
        if num.chars().all(|c| c.is_ascii_digit())
            && !num.is_empty()
            && rest.starts_with("r[")
            && let Some(id) = rule_id(rest)
        {
            let body = rest[rest.find(']').unwrap_or(0) + 1..].trim();
            rules.push((id.to_owned(), sentence(body)));
        }
    }
    let rule_for = |owner: &str, name: &str| -> Option<&(String, String)> {
        let key = name.trim_end_matches('!').replace('_', "-");
        let named = |r: &&(String, String)| r.0.rsplit('.').next() == Some(key.as_str());
        let owner = owner.to_lowercase();
        rules
            .iter()
            .filter(named)
            .find(|r| !owner.is_empty() && r.0.contains(&format!(".{owner}.")))
            .or_else(|| rules.iter().find(named))
    };
    let push = |out: &mut Vec<Entry>,
                owner: &str,
                name: &str,
                signature: &str,
                row: Option<(&str, &str)>,
                section: &str| {
        let (summary, anchor) = match (rule_for(owner, name), row) {
            (Some((id, s)), _) => (s.clone(), format!("r-{id}")),
            (None, Some((id, result))) => (
                if result.is_empty() {
                    String::new()
                } else {
                    sentence(result)
                },
                if id.is_empty() {
                    slug(section)
                } else {
                    format!("r-{id}")
                },
            ),
            (None, None) => (String::new(), slug(section)),
        };
        let entry = Entry {
            module: module.to_owned(),
            owner: owner.to_owned(),
            name: name.to_owned(),
            signature: signature.to_owned(),
            summary,
            anchor: format!("{file}#{anchor}"),
        };
        if !out.iter().any(|e| {
            (&e.module, &e.owner, &e.name, &e.signature)
                == (&entry.module, &entry.owner, &entry.name, &entry.signature)
        }) {
            out.push(entry);
        }
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut section = String::new();
    let mut header: Vec<String> = Vec::new();
    let mut fenced = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            let Some(rest) = line.strip_prefix("pub ") else {
                continue;
            };
            let mut words = rest.split_whitespace();
            let (Some(kind), Some(word)) = (words.next(), words.next()) else {
                continue;
            };
            if !matches!(kind, "data" | "enum" | "trait" | "type") {
                continue;
            }
            let name: String = word
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let mut signature = line.trim_end().to_owned();
            while let Some(next) = lines.get(i)
                && next.starts_with(' ')
                && !next.trim_start().starts_with("```")
            {
                signature.push('\n');
                signature.push_str(next.trim_end());
                i += 1;
            }
            push(out, "", &name, &signature, None, &section);
            continue;
        }
        if let Some(h) = line.strip_prefix('#') {
            h.trim_start_matches('#').trim().clone_into(&mut section);
            header.clear();
            continue;
        }
        if !line.trim_start().starts_with('|') {
            header.clear();
            continue;
        }
        let row = cells(line);
        if header.is_empty() {
            header = row;
            continue;
        }
        if row
            .iter()
            .all(|c| c.chars().all(|c| matches!(c, '-' | ':' | ' ')))
        {
            continue;
        }
        let owner = if header.first().is_some_and(|h| h == "Receiver") {
            owner_of(&row[0])
        } else {
            String::new()
        };
        let id = row.first().and_then(|c| rule_id(c)).unwrap_or("");
        let result = if header.last().is_some_and(|h| h == "Result") {
            row.last().map_or("", String::as_str)
        } else {
            ""
        };
        for cell in &row {
            for span in spans(cell) {
                if let Some((name, signature)) = function(span) {
                    push(out, &owner, &name, &signature, Some((id, result)), &section);
                }
            }
        }
    }
}

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let dir = Path::new(&manifest).join("../../../spec/std");
    println!("cargo::rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    files.sort();
    let mut entries = Vec::new();
    for p in &files {
        let module = p.file_stem().map(|s| s.to_string_lossy().into_owned());
        let Some(module) = module.filter(|m| m != "README") else {
            continue;
        };
        let text = std::fs::read_to_string(p).unwrap_or_default();
        read_module(&module, &text, &mut entries);
    }
    let mut code = String::from(
        "/// The standard library reference's items: (module, owner type, name, signature, summary, reference link).\n\
         pub(crate) static STD_ENTRIES: &[(&str, &str, &str, &str, &str, &str)] = &[\n",
    );
    for e in &entries {
        let _ = writeln!(
            code,
            "    ({:?}, {:?}, {:?}, {:?}, {:?}, {:?}),",
            e.module, e.owner, e.name, e.signature, e.summary, e.anchor
        );
    }
    code.push_str("];\n");
    let out = std::env::var("OUT_DIR").unwrap_or_default();
    if let Err(e) = std::fs::write(Path::new(&out).join("std_table.rs"), code) {
        println!("cargo::error=cannot write the std table: {e}");
    }
}
