//! Doc tests (spec/lang/10-modules.md "Doc Tests", checking-and-tir.md
//! §4.13.9): each fenced `hd` block in a documentation comment of a module
//! under the source root is a test of its own. Extraction reads only the
//! file's text: the `##` blocks (`lex.doc.*`), the declaration each
//! documents, and the fences. Each doc test becomes a synthetic program
//! whose lines map back to the `##` lines they came from
//! (`cli.test.doc.location`).

use std::fmt::Write as _;

/// One doc test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocTest {
    /// `doc <module>.<item>[i]`, or `doc <module>[i]` for module
    /// documentation (`cli.test.doc.name`).
    pub name: String,
    /// The program: the block's `use` lines, then `it(NAME):` with the
    /// rest of the block as its trailing body (`module.test.doc.uses`,
    /// `module.test.doc.body`).
    pub program: String,
    /// The 1-based source line of each program line.
    pub lines: Vec<u32>,
    /// `CODE` of a `# error: CODE` line: a compile-fail doc test
    /// (`module.test.doc.compile-fail`).
    pub compile_fail: Option<String>,
}

/// One `##` block: its first line, indentation and text lines.
struct DocBlock {
    first: usize,
    last: usize,
    indent: usize,
    text: Vec<String>,
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The name a declaration line declares, if it is one: `fn`, `data`,
/// `enum`, `trait` and `type` items, and in a body a field, a variant or a
/// method.
fn declared_name(line: &str, member: bool) -> Option<String> {
    let t = line.trim();
    let t = t.strip_prefix("pub ").unwrap_or(t);
    let ident = |s: &str| -> Option<String> {
        let name: String = s
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        (!name.is_empty()).then_some(name)
    };
    for kw in ["fn ", "data ", "enum ", "trait ", "type "] {
        if let Some(rest) = t.strip_prefix(kw) {
            return ident(rest.trim_start());
        }
    }
    if member && !t.starts_with('#') && !t.starts_with('@') {
        // A field `name: T` or a variant `Name` or `Name(...)`.
        return ident(t);
    }
    None
}

/// The type an `impl`, `data`, `enum` or `trait` header line is about.
fn owner_name(line: &str) -> Option<String> {
    let t = line.trim();
    let t = t.strip_prefix("pub ").unwrap_or(t);
    let after = if let Some(rest) = t.strip_prefix("impl ") {
        let rest = rest.split(':').next().unwrap_or(rest);
        rest.rsplit(" for ").next().unwrap_or(rest).trim()
    } else {
        ["data ", "enum ", "trait "]
            .iter()
            .find_map(|kw| t.strip_prefix(kw))?
    };
    let name: String = after
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// The `##` blocks of a file, in order.
fn doc_blocks(lines: &[&str]) -> Vec<DocBlock> {
    let mut out: Vec<DocBlock> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        let Some(rest) = t.strip_prefix("##") else {
            continue;
        };
        let text = rest.strip_prefix(' ').unwrap_or(rest).to_owned();
        let indent = indent_of(line);
        match out.last_mut() {
            Some(b) if b.last + 1 == i && b.indent == indent => {
                b.last = i;
                b.text.push(text);
            }
            _ => out.push(DocBlock {
                first: i,
                last: i,
                indent,
                text: vec![text],
            }),
        }
    }
    out
}

/// The doc tests of a package-relative file, named by its module path
/// below the package root (`pkg` for `src/lib.hd`). A file outside the
/// source root has none (`module.test.doc.block`).
#[must_use]
pub fn doc_tests(file: &str, text: &str) -> Vec<DocTest> {
    if !file.starts_with("src/") {
        return Vec::new();
    }
    let module = match crate::module_below(file).as_str() {
        "" => "pkg".to_owned(),
        m => m.to_owned(),
    };
    let lines: Vec<&str> = text.lines().collect();
    let blocks = doc_blocks(&lines);
    let mut counts: Vec<(String, usize)> = Vec::new();
    let mut out = Vec::new();
    for (bi, b) in blocks.iter().enumerate() {
        // `lex.doc.module`: the first block, with no token before it and a
        // blank line after it, documents the module.
        let before_blank = lines[..b.first].iter().all(|l| {
            l.trim().is_empty()
                || (l.trim_start().starts_with('#') && !l.trim_start().starts_with("##"))
        });
        let after_blank = lines.get(b.last + 1).is_none_or(|l| l.trim().is_empty());
        let item = if bi == 0 && before_blank && after_blank && b.indent == 0 {
            None
        } else {
            let Some(target) = lines.get(b.last + 1) else {
                continue;
            };
            let member = b.indent > 0;
            let Some(name) = declared_name(target, member) else {
                continue;
            };
            if member {
                let owner = lines[..b.first]
                    .iter()
                    .rev()
                    .find(|l| !l.trim().is_empty() && indent_of(l) < b.indent)
                    .and_then(|l| owner_name(l));
                match owner {
                    Some(o) => Some(format!("{o}.{name}")),
                    None => continue,
                }
            } else {
                Some(name)
            }
        };
        let base = match &item {
            Some(i) => format!("doc {module}.{i}"),
            None => format!("doc {module}"),
        };
        // The fenced `hd` blocks of this comment (`module.test.doc.fence`).
        let mut fence: Option<(bool, Vec<(String, u32)>)> = None;
        for (k, t) in b.text.iter().enumerate() {
            let source_line = u32::try_from(b.first + k + 1).unwrap_or(u32::MAX);
            let trimmed = t.trim();
            if let Some(info) = trimmed.strip_prefix("```") {
                match fence.take() {
                    None => fence = Some((info.trim() == "hd", Vec::new())),
                    Some((true, body)) => {
                        let index =
                            if let Some((_, c)) = counts.iter_mut().find(|(n, _)| *n == base) {
                                *c += 1;
                                *c - 1
                            } else {
                                counts.push((base.clone(), 1));
                                0
                            };
                        out.push(program(&format!("{base}[{index}]"), &body));
                    }
                    Some((false, _)) => {}
                }
                continue;
            }
            if let Some((_, body)) = &mut fence {
                body.push((t.clone(), source_line));
            }
        }
    }
    out
}

/// The synthetic program of one doc test: its leading `use` lines, then
/// the rest as the trailing body of `it(NAME)`.
fn program(name: &str, body: &[(String, u32)]) -> DocTest {
    let split = body
        .iter()
        .position(|(l, _)| {
            let t = l.trim();
            !(t.is_empty() || t.starts_with("use ") || t.starts_with("pub use "))
        })
        .unwrap_or(body.len());
    let mut text = String::new();
    let mut lines = Vec::new();
    for (l, at) in &body[..split] {
        text.push_str(l);
        text.push('\n');
        lines.push(*at);
    }
    let it_line = body
        .get(split)
        .map_or_else(|| body.last().map_or(1, |b| b.1), |b| b.1);
    let _ = writeln!(text, "it(\"{name}\"):");
    lines.push(it_line);
    let rest = &body[split..];
    if rest.iter().all(|(l, _)| l.trim().is_empty()) {
        text.push_str("    pass\n");
        lines.push(it_line);
    }
    for (l, at) in rest {
        text.push_str("    ");
        text.push_str(l);
        text.push('\n');
        lines.push(*at);
    }
    let compile_fail = body.iter().find_map(|(l, _)| {
        l.split_once("# error:").map(|(_, code)| {
            code.split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned()
        })
    });
    DocTest {
        name: name.to_owned(),
        program: text,
        lines,
        compile_fail,
    }
}

#[cfg(test)]
mod tests {
    use super::doc_tests;

    #[test]
    fn names_programs_and_lines() {
        let text = "## Text helpers.\n##\n## ```hd\n## use pkg.text.{slugify}\n##\n## slugify(\"a\")\n## ```\n\n## Turns a title into a slug.\n##\n## ```hd\n## use pkg.text.{slugify}\n## use std.testing.assert_equal\n##\n## assert_equal(slugify(\"A B\"), \"A-B\", reason=\"r\")\n## ```\n##\n## ```text\n## not a test\n## ```\n## ```hd\n## use pkg.text.{slugify}\n##\n## x := slugify(1)  # error: type-mismatch\n## ```\npub fn slugify(title: string) -> string:\n    title\n\npub data Slug:\n    ## The text.\n    ##\n    ## ```hd\n    ## use pkg.text.{Slug}\n    ## ```\n    text: string\n";
        let tests = doc_tests("src/text.hd", text);
        let names: Vec<&str> = tests.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "doc text[0]",
                "doc text.slugify[0]",
                "doc text.slugify[1]",
                "doc text.Slug.text[0]"
            ]
        );
        assert_eq!(
            tests[1].program,
            "use pkg.text.{slugify}\nuse std.testing.assert_equal\n\nit(\"doc text.slugify[0]\"):\n    assert_equal(slugify(\"A B\"), \"A-B\", reason=\"r\")\n"
        );
        assert_eq!(tests[1].lines, [12, 13, 14, 15, 15]);
        assert_eq!(tests[2].compile_fail.as_deref(), Some("type-mismatch"));
        assert!(
            tests[3]
                .program
                .ends_with("it(\"doc text.Slug.text[0]\"):\n    pass\n")
        );
        assert_eq!(
            doc_tests(
                "src/lib.hd",
                "## M.\n\n## ```hd\n## x := 1\n## ```\npub fn f() -> i32: 1\n"
            )[0]
            .name,
            "doc pkg.f[0]"
        );
        assert!(doc_tests("tests/a.hd", text).is_empty());
    }
}
