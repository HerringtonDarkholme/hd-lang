//! Edits of `hd.toml` text that keep everything else as written: the
//! commands that change a manifest (`hd new`, the dependency commands)
//! touch only the lines they must.

use toml::de::{DeTable, DeValue};

/// The byte span of `table.key`'s value, and of the whole line(s) from its
/// key to its value's end. A key that is not written as `key = value` on
/// lines of its own, as a `[table.key]` header or an inline `table = {..}`,
/// has none: an edit leaves it to the user.
fn spans(
    text: &str,
    table: &str,
    key: &str,
) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    let doc = DeTable::parse(text).ok()?;
    let (_, t) = doc.get_ref().iter().find(|(k, _)| {
        let k: &str = k.get_ref();
        k == table
    })?;
    let DeValue::Table(t) = t.get_ref() else {
        return None;
    };
    let (k, v) = t.iter().find(|(k, _)| {
        let k: &str = k.get_ref();
        k == key
    })?;
    entry_lines(text, k.span(), v.span()).map(|lines| (v.span(), lines))
}

/// The lines of an entry `key = value`, from the start of its key's line
/// to the end of its value's line, or `None` when it is not written so.
fn entry_lines(
    text: &str,
    key: std::ops::Range<usize>,
    value: std::ops::Range<usize>,
) -> Option<std::ops::Range<usize>> {
    let start = text[..key.start].rfind('\n').map_or(0, |i| i + 1);
    if !text[start..key.start].trim().is_empty() || !text[key.end..value.start].contains('=') {
        return None;
    }
    let end = text[value.end..]
        .find('\n')
        .map_or(text.len(), |i| value.end + i + 1);
    Some(start..end)
}

/// A TOML basic string.
fn quoted(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The manifest with `member` added to its `[workspace] members`
/// (`cli.new.workspace-member`), or `None` when it has no `[workspace]`
/// table. A member already listed is not added again.
#[must_use]
pub fn add_member(text: &str, member: &str) -> Option<String> {
    add_to_workspace_list(text, "members", member)
}

/// The manifest with `entry` added to its `[workspace]` list `key`, as
/// `members` or `exclude` (`cli.mode.member.unlisted.fix`), or `None` when
/// it has no `[workspace]` table. An entry already listed is not added
/// again.
#[must_use]
pub fn add_to_workspace_list(text: &str, key: &str, entry: &str) -> Option<String> {
    let w = crate::parse_manifest(text).ok()?.workspace?;
    let mut entries = if key == "members" {
        w.members
    } else {
        w.exclude
    };
    if entries.iter().any(|x| x == entry) {
        return Some(text.to_owned());
    }
    entries.push(entry.to_owned());
    let list = format!(
        "[{}]",
        entries
            .iter()
            .map(|x| quoted(x))
            .collect::<Vec<_>>()
            .join(", ")
    );
    if let Some((value, _)) = spans(text, "workspace", key) {
        return Some(format!(
            "{}{list}{}",
            &text[..value.start],
            &text[value.end..]
        ));
    }
    // No such key yet: it goes right under the `[workspace]` header.
    let header = text.find("[workspace]")?;
    let after = text[header..]
        .find('\n')
        .map_or(text.len(), |i| header + i + 1);
    let newline = if after == text.len() && !text.ends_with('\n') {
        "\n"
    } else {
        ""
    };
    Some(format!(
        "{}{newline}{key} = {list}\n{}",
        &text[..after],
        &text[after..]
    ))
}

/// The manifest with the key `table.key` deleted, and the table's header
/// too when that was its last key (`cli.dep.edit.empty-table`), or `None`
/// when the key is not there.
#[must_use]
pub fn remove_key(text: &str, table: &str, key: &str) -> Option<String> {
    let (_, lines) = spans(text, table, key)?;
    let mut out = format!("{}{}", &text[..lines.start], &text[lines.end..]);
    let doc = DeTable::parse(&out).ok()?;
    let empty = doc.get_ref().iter().find_map(|(k, v)| {
        let k: &str = k.get_ref();
        match v.get_ref() {
            DeValue::Table(t) if k == table && t.is_empty() => Some(k.to_owned()),
            _ => None,
        }
    });
    if let Some(name) = empty {
        let header = format!("[{name}]");
        if let Some(at) = out.find(&header) {
            let start = out[..at].rfind('\n').map_or(0, |i| i + 1);
            let end = out[at..].find('\n').map_or(out.len(), |i| at + i + 1);
            out.replace_range(start..end, "");
            // A blank line left before the header goes with it.
            if out[..start].ends_with("\n\n") {
                out.remove(start - 1);
            }
        }
    }
    Some(out)
}

/// The byte offset just past the header line `[table]`, or `None` when the
/// manifest has no such line.
fn header_end(text: &str, table: &str) -> Option<usize> {
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let code = line.split('#').next().unwrap_or("").trim();
        if let Some(name) = code.strip_prefix('[').and_then(|r| r.strip_suffix(']'))
            && !name.starts_with('[')
            && name.trim() == table
        {
            return Some(at + line.len());
        }
        at += line.len();
    }
    None
}

/// The manifest with `table.key` set to the string `value`
/// (`cli.dep.edit`): the key's value changes in place, so a comment on its
/// line stays; or a new line follows the table's last key; or a new table
/// ends the manifest. `key` is a bare TOML key. `None` when the table or
/// key is written in a form that has no such line, as `[table.key]`.
#[must_use]
pub fn set_key(text: &str, table: &str, key: &str, value: &str) -> Option<String> {
    set_value(text, table, key, &quoted(value))
}

/// The value a path requirement on `dir` writes: `{ path = "DIR" }`
/// (`module.path-dep.form`).
#[must_use]
pub fn path_value(dir: &str) -> String {
    format!("{{ path = {} }}", quoted(dir))
}

/// The value a requirement `PATH@VERSION` writes: a TOML string.
#[must_use]
pub fn string_value(value: &str) -> String {
    quoted(value)
}

/// `set_key`, with `written` as the value's TOML text as it is, such as
/// [`path_value`]'s inline table.
#[must_use]
pub fn set_value(text: &str, table: &str, key: &str, written: &str) -> Option<String> {
    if let Some((span, _)) = spans(text, table, key) {
        return Some(format!(
            "{}{written}{}",
            &text[..span.start],
            &text[span.end..]
        ));
    }
    let doc = DeTable::parse(text).ok()?;
    let entries = match doc.get_ref().iter().find(|(k, _)| {
        let k: &str = k.get_ref();
        k == table
    }) {
        None => None,
        Some((_, t)) => match t.get_ref() {
            DeValue::Table(t) => Some(t),
            _ => return None,
        },
    };
    let line = format!("{key} = {written}\n");
    let Some(entries) = entries else {
        // A new table at the end, set off by one blank line.
        let mut out = text.to_owned();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push('[');
        out.push_str(table);
        out.push_str("]\n");
        out.push_str(&line);
        return Some(out);
    };
    let header = header_end(text, table)?;
    let mut at = header;
    for (k, v) in entries {
        at = at.max(entry_lines(text, k.span(), v.span())?.end);
    }
    let mut out = text[..at].to_owned();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&line);
    out.push_str(&text[at..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{add_member, add_to_workspace_list, remove_key, set_key};

    #[test]
    fn members_grow_in_place() {
        assert_eq!(
            add_member("# ws\n[workspace]\nmembers = [\"app\"]  # kept\n", "lib").as_deref(),
            Some("# ws\n[workspace]\nmembers = [\"app\", \"lib\"]  # kept\n")
        );
        assert_eq!(
            add_member("[workspace]\nexclude = []\n", "lib").as_deref(),
            Some("[workspace]\nmembers = [\"lib\"]\nexclude = []\n")
        );
        assert_eq!(add_member("[package]\nname = \"x\"\n", "lib"), None);
    }

    #[test]
    fn an_exclude_entry_grows_in_place() {
        assert_eq!(
            add_to_workspace_list("[workspace]\nmembers = [\"app\"]\n", "exclude", "tools")
                .as_deref(),
            Some("[workspace]\nexclude = [\"tools\"]\nmembers = [\"app\"]\n")
        );
    }

    #[test]
    fn a_removed_last_key_takes_its_table_header() {
        let text = "[package]\nname = \"app\"\n\n[dependencies]\njson = \"github.com/acme/json@2.1.0\"\nyaml = \"github.com/acme/yaml@1.0.0\"\n";
        assert_eq!(
            remove_key(text, "dependencies", "json").as_deref(),
            Some(
                "[package]\nname = \"app\"\n\n[dependencies]\nyaml = \"github.com/acme/yaml@1.0.0\"\n"
            )
        );
        let one =
            "[package]\nname = \"app\"\n\n[dependencies]\njson = \"github.com/acme/json@2.1.0\"\n";
        assert_eq!(
            remove_key(one, "dependencies", "json").as_deref(),
            Some("[package]\nname = \"app\"\n")
        );
        assert_eq!(remove_key(one, "dependencies", "yaml"), None);
    }

    const JSON: &str = "github.com/acme/json@2.1.0";

    #[test]
    fn a_changed_requirement_keeps_its_comments() {
        let text = "# app\n[package]\nname = \"app\"\n\n[dependencies]\n# parsing\njson = \"github.com/acme/json@2.0.0\"  # pinned\nyaml = \"github.com/acme/yaml@1.0.0\"\n";
        assert_eq!(
            set_key(text, "dependencies", "json", JSON).as_deref(),
            Some(
                "# app\n[package]\nname = \"app\"\n\n[dependencies]\n# parsing\njson = \"github.com/acme/json@2.1.0\"  # pinned\nyaml = \"github.com/acme/yaml@1.0.0\"\n"
            )
        );
    }

    #[test]
    fn a_path_requirement_becomes_a_version() {
        let text = "[dependencies]\njson = { path = \"../json\" }  # local\n";
        assert_eq!(
            set_key(text, "dependencies", "json", JSON).as_deref(),
            Some("[dependencies]\njson = \"github.com/acme/json@2.1.0\"  # local\n")
        );
    }

    #[test]
    fn a_new_key_follows_the_last_key_of_its_table() {
        let text = "[dependencies]\nyaml = \"github.com/acme/yaml@1.0.0\"  # data\n\n# tests\n[dev-dependencies]\nfix = \"github.com/acme/fix@1.0.0\"\n";
        assert_eq!(
            set_key(text, "dependencies", "json", JSON).as_deref(),
            Some(
                "[dependencies]\nyaml = \"github.com/acme/yaml@1.0.0\"  # data\njson = \"github.com/acme/json@2.1.0\"\n\n# tests\n[dev-dependencies]\nfix = \"github.com/acme/fix@1.0.0\"\n"
            )
        );
        assert_eq!(
            set_key("[dependencies]\n", "dependencies", "json", JSON).as_deref(),
            Some("[dependencies]\njson = \"github.com/acme/json@2.1.0\"\n")
        );
        assert_eq!(
            set_key("[dependencies]\n# none yet\n", "dependencies", "json", JSON).as_deref(),
            Some("[dependencies]\njson = \"github.com/acme/json@2.1.0\"\n# none yet\n")
        );
    }

    #[test]
    fn a_missing_table_is_added_at_the_end() {
        assert_eq!(
            set_key(
                "[package]\nname = \"app\"\n",
                "dev-dependencies",
                "fix",
                JSON
            )
            .as_deref(),
            Some(
                "[package]\nname = \"app\"\n\n[dev-dependencies]\nfix = \"github.com/acme/json@2.1.0\"\n"
            )
        );
        // No newline at the end, and a manifest that is only a comment.
        assert_eq!(
            set_key("[package]\nname = \"app\"", "dependencies", "json", JSON).as_deref(),
            Some(
                "[package]\nname = \"app\"\n\n[dependencies]\njson = \"github.com/acme/json@2.1.0\"\n"
            )
        );
        assert_eq!(
            set_key("", "dependencies", "json", JSON).as_deref(),
            Some("[dependencies]\njson = \"github.com/acme/json@2.1.0\"\n")
        );
    }

    #[test]
    fn forms_without_a_line_of_their_own_are_not_edited() {
        let inline = "dependencies = { json = \"github.com/acme/json@2.0.0\" }\n";
        assert_eq!(set_key(inline, "dependencies", "json", JSON), None);
        assert_eq!(set_key(inline, "dependencies", "yaml", JSON), None);
        let header = "[dependencies.json]\nversion = \"1\"\n";
        assert_eq!(set_key(header, "dependencies", "json", JSON), None);
        assert_eq!(remove_key(header, "dependencies", "json"), None);
        assert_eq!(
            set_key("dependencies = 1\n", "dependencies", "json", JSON),
            None
        );
    }

    #[test]
    fn a_dev_key_is_removed_with_its_table_header() {
        let text = "[package]\nname = \"app\"\n\n[dev-dependencies]\nfix = \"github.com/acme/fix@1.0.0\"  # tests\n";
        assert_eq!(
            remove_key(text, "dev-dependencies", "fix").as_deref(),
            Some("[package]\nname = \"app\"\n")
        );
        // Another table's comment and blank lines stay.
        let two = "[dependencies]\n# why\njson = \"github.com/acme/json@2.1.0\"\n\n[dev-dependencies]\nfix = \"github.com/acme/fix@1.0.0\"\n";
        assert_eq!(
            remove_key(two, "dev-dependencies", "fix").as_deref(),
            Some("[dependencies]\n# why\njson = \"github.com/acme/json@2.1.0\"\n")
        );
    }

    #[test]
    fn a_path_requirement_is_written_as_an_inline_table() {
        let text =
            "[package]\nname = \"shop\"\n\n[dependencies]\njson = \"github.com/acme/json@1.0.0\"\n";
        let value = super::path_value("money-utils");
        assert_eq!(value, "{ path = \"money-utils\" }");
        let out = super::set_value(text, "dependencies", "money_utils", &value).expect("edit");
        assert!(out.ends_with(
            "json = \"github.com/acme/json@1.0.0\"\nmoney_utils = { path = \"money-utils\" }\n"
        ));
        let back = super::set_value(
            &out,
            "dependencies",
            "money_utils",
            &super::string_value("github.com/acme/m@1.0.0"),
        )
        .expect("edit");
        assert!(back.contains("money_utils = \"github.com/acme/m@1.0.0\"\n"));
    }
}
