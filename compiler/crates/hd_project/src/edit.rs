//! Edits of `hd.toml` text that keep everything else as written: the
//! commands that change a manifest (`hd new`, the dependency commands)
//! touch only the lines they must.

use toml::de::{DeTable, DeValue};

/// The byte span of `table.key`'s value, and of the whole line(s) from its
/// key to its value's end.
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
    let start = text[..k.span().start].rfind('\n').map_or(0, |i| i + 1);
    let end = text[v.span().end..]
        .find('\n')
        .map_or(text.len(), |i| v.span().end + i + 1);
    Some((v.span(), start..end))
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
    let m = crate::parse_manifest(text).ok()?;
    let w = m.workspace?;
    if w.members.iter().any(|x| x == member) {
        return Some(text.to_owned());
    }
    let mut members = w.members;
    members.push(member.to_owned());
    let list = format!(
        "[{}]",
        members
            .iter()
            .map(|x| quoted(x))
            .collect::<Vec<_>>()
            .join(", ")
    );
    if let Some((value, _)) = spans(text, "workspace", "members") {
        return Some(format!(
            "{}{list}{}",
            &text[..value.start],
            &text[value.end..]
        ));
    }
    // No `members` key yet: it goes right under the `[workspace]` header.
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
        "{}{newline}members = {list}\n{}",
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

#[cfg(test)]
mod tests {
    use super::{add_member, remove_key};

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
}
