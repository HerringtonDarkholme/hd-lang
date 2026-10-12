#![forbid(unsafe_code)]
//! `hd_doc`: the pages of `hd doc` (spec/cli/command-line.md
//! "Documentation", `future-work/compiler/doc.md`). Each module's page is
//! rendered from its source, whose declarations give the signatures as
//! written and the `##` documentation, and from the package's folder
//! interfaces, which give each type's implementations. Pages are built as
//! strings: the Markdown copies the documentation unchanged, and the HTML
//! holds the same content under the same anchors.

mod html;
mod impls;
mod items;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use items::{Item, Section};

/// One documented module.
pub struct Module<'a> {
    /// Its path below the package: empty for the root module `src/lib.hd`,
    /// `text` for `src/text.hd`, `main` for `src/main.hd`.
    pub path: &'a str,
    pub text: &'a str,
}

/// What `hd doc` documents.
pub struct Package<'a> {
    /// The package's name, as its manifest gives it.
    pub name: &'a str,
    /// Its module root, the first segment of its module paths in the
    /// interfaces.
    pub root: &'a str,
    pub modules: &'a [Module<'a>],
    /// The run's folder interface blobs, by folder path.
    pub ifaces: &'a [(String, Arc<[u8]>)],
    /// `--private`: the private items and members too (`cli.doc.private`).
    pub private: bool,
}

/// One output file, by its path below the output directory.
#[derive(Debug, PartialEq, Eq)]
pub struct DocFile {
    pub path: String,
    pub text: String,
}

/// The file name of a module's pages without the extension
/// (`cli.doc.files`): `pkg` for the root module (`cli.doc.root-page`),
/// `index/index` for a top-level `index` (`cli.doc.index-module`), else
/// the path with each `.` a `/`.
#[must_use]
pub fn page_stem(module: &str) -> String {
    match module {
        "" => "pkg".to_owned(),
        "index" => "index/index".to_owned(),
        m => m.replace('.', "/"),
    }
}

/// The module's name on its page: `pkg` for the root module.
fn shown_name(module: &str) -> &str {
    if module.is_empty() { "pkg" } else { module }
}

/// The first sentence of a documentation text (`cli.doc.summary`): its
/// first paragraph up to the first `.`, `!` or `?` that ends a sentence.
#[must_use]
pub fn summary(doc: &str) -> String {
    let text = doc
        .lines()
        .map(str::trim)
        .take_while(|l| !l.is_empty() && !l.starts_with("```"))
        .collect::<Vec<_>>()
        .join(" ");
    for (i, c) in text.char_indices() {
        if matches!(c, '.' | '!' | '?')
            && text[i + 1..].chars().next().is_none_or(char::is_whitespace)
        {
            return text[..=i].to_owned();
        }
    }
    text
}

/// A documentation text with each doc test labelled in place
/// (`cli.doc.example`, `cli.doc.example.compile-fail`): `Example` and the
/// doc test's name before each fenced `hd` block, which is copied as
/// written. `base` is `doc <module>.<item>` or `doc <module>`, and
/// `counts` numbers the blocks of each base as `cli.test.doc.name` does.
fn with_examples(doc: &str, base: &str, counts: &mut HashMap<String, usize>) -> String {
    let lines: Vec<&str> = doc.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim();
        let Some(info) = t.strip_prefix("```") else {
            out.push(lines[i].to_owned());
            i += 1;
            continue;
        };
        let close = (i + 1..lines.len())
            .find(|&k| lines[k].trim().starts_with("```"))
            .unwrap_or(lines.len());
        if info.trim() == "hd" {
            let n = counts.entry(base.to_owned()).or_insert(0);
            let name = format!("{base}[{n}]");
            *n += 1;
            let code = lines[i + 1..close].iter().find_map(|l| {
                l.split_once("# error:")
                    .and_then(|(_, c)| c.split_whitespace().next())
            });
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(match code {
                Some(code) => format!("Example `{name}`, Does not compile ({code}):"),
                None => format!("Example `{name}`:"),
            });
        }
        let end = close.min(lines.len().saturating_sub(1));
        out.extend(lines[i..=end].iter().map(|l| (*l).to_owned()));
        i = end + 1;
    }
    out.join("\n")
}

/// The two forms of one page, built together.
#[derive(Default)]
struct Page {
    md: String,
    html: String,
}

impl Page {
    fn heading(&mut self, level: usize, prefix: &str, code: &str, anchor: Option<&str>) {
        if let Some(a) = anchor {
            let _ = writeln!(self.md, "<a id=\"{}\"></a>", html::escape(a));
        }
        let _ = writeln!(self.md, "{} {prefix}`{code}`\n", "#".repeat(level));
        let id = anchor.map_or_else(String::new, |a| format!(" id=\"{}\"", html::escape(a)));
        let _ = writeln!(
            self.html,
            "<h{level}{id}>{}<code>{}</code></h{level}>",
            html::escape(prefix),
            html::escape(code)
        );
    }
    fn text(&mut self, md: &str) {
        if md.trim().is_empty() {
            return;
        }
        let _ = writeln!(self.md, "{md}\n");
        self.html.push_str(&html::blocks(md));
    }
    fn signature(&mut self, sig: &str) {
        let _ = writeln!(self.md, "```hd\n{sig}\n```\n");
        let _ = writeln!(
            self.html,
            "<pre><code class=\"language-hd\">{}</code></pre>",
            html::escape(sig)
        );
    }
    /// A list of links: (code, Markdown target, HTML target, summary).
    fn links(&mut self, entries: &[(String, String, String, String)]) {
        if entries.is_empty() {
            return;
        }
        self.html.push_str("<ul>\n");
        for (code, md_href, html_href, summary) in entries {
            let tail = if summary.is_empty() {
                String::new()
            } else {
                format!(": {summary}")
            };
            let _ = writeln!(self.md, "- [`{code}`]({md_href}){tail}");
            let _ = writeln!(
                self.html,
                "<li><a href=\"{}\"><code>{}</code></a>{}</li>",
                html::escape(html_href),
                html::escape(code),
                html::inline(&tail)
            );
        }
        self.md.push('\n');
        self.html.push_str("</ul>\n");
    }
    /// Plain list lines, as Markdown inline text.
    fn lines(&mut self, lines: &[String]) {
        if lines.is_empty() {
            return;
        }
        self.html.push_str("<ul>\n");
        for l in lines {
            let _ = writeln!(self.md, "- {l}");
            let _ = writeln!(self.html, "<li>{}</li>", html::inline(l));
        }
        self.md.push('\n');
        self.html.push_str("</ul>\n");
    }
    fn section(
        &mut self,
        level: usize,
        s: &Section,
        base: &str,
        counts: &mut HashMap<String, usize>,
    ) {
        self.heading(level, "", &s.anchor, Some(&s.anchor));
        self.signature(&s.signature);
        self.text(&with_examples(
            &s.doc,
            &format!("{base}.{}", s.anchor),
            counts,
        ));
        self.lines(&s.impls);
    }
}

/// One module's page (`cli.doc.page`, `cli.doc.item`), and its module
/// documentation's summary.
fn module_page(m: &Module<'_>, p: &Package<'_>, impl_lines: &impls::ImplLines) -> (Page, String) {
    let parse = hd_syntax::parse(m.text.as_bytes());
    let module_doc = hd_syntax::module_doc_text(m.text, &parse.tokens);
    let mut items: Vec<Item> = items::items(m.text, &parse, p.private);
    let full = if m.path.is_empty() {
        p.root.to_owned()
    } else {
        format!("{}.{}", p.root, m.path)
    };
    for it in &mut items {
        if let Some(lines) = it
            .type_name
            .as_ref()
            .and_then(|t| impl_lines.get(&(full.clone(), t.clone())))
        {
            it.section.impls.clone_from(lines);
        }
    }
    let name = shown_name(m.path);
    let base = format!("doc {name}");
    let mut counts = HashMap::new();
    let mut page = Page::default();
    page.heading(1, "Module ", name, None);
    page.text(&with_examples(&module_doc, &base, &mut counts));
    let entries: Vec<_> = items
        .iter()
        .map(|it| {
            let a = &it.section.anchor;
            (
                a.clone(),
                format!("#{a}"),
                format!("#{a}"),
                summary(&it.section.doc),
            )
        })
        .collect();
    page.links(&entries);
    for it in &items {
        page.section(2, &it.section, &base, &mut counts);
        for member in &it.members {
            page.section(3, member, &base, &mut counts);
        }
    }
    (page, summary(&module_doc))
}

/// Every output file of `hd doc` (`cli.doc.files`): each module's pages,
/// the entry page, `llms.txt` and `llms-full.txt`.
#[must_use]
pub fn render(p: &Package<'_>) -> Vec<DocFile> {
    let impl_lines = impls::impl_lines(p.root, p.ifaces);
    let mut modules: Vec<&Module<'_>> = p.modules.iter().collect();
    // `cli.doc.llms-full`: the root module first, the others in path order.
    modules.sort_by(|a, b| (!a.path.is_empty(), a.path).cmp(&(!b.path.is_empty(), b.path)));
    let mut files = Vec::new();
    let mut index = Page::default();
    let mut entries = Vec::new();
    let mut llms_modules = String::new();
    let mut full = String::new();
    let mut root_summary = String::new();
    for m in &modules {
        let (page, sum) = module_page(m, p, &impl_lines);
        let stem = page_stem(m.path);
        let name = shown_name(m.path);
        if m.path.is_empty() {
            root_summary.clone_from(&sum);
        }
        let tail = if sum.is_empty() {
            String::new()
        } else {
            format!(": {sum}")
        };
        let _ = writeln!(llms_modules, "- [{name}]({stem}.md){tail}");
        entries.push((
            name.to_owned(),
            format!("{stem}.md"),
            format!("{stem}.html"),
            sum,
        ));
        if !full.is_empty() {
            full.push('\n');
        }
        full.push_str(&page.md);
        let title = format!("Module {name} - {}", p.name);
        files.push(DocFile {
            path: format!("{stem}.md"),
            text: page.md,
        });
        files.push(DocFile {
            path: format!("{stem}.html"),
            text: html::page(&title, &page.html),
        });
    }
    let _ = writeln!(index.md, "# {}\n", p.name);
    let _ = writeln!(index.html, "<h1>{}</h1>", html::escape(p.name));
    index.links(&entries);
    files.push(DocFile {
        path: "index.md".to_owned(),
        text: index.md,
    });
    files.push(DocFile {
        path: "index.html".to_owned(),
        text: html::page(p.name, &index.html),
    });
    let mut llms = format!("# {}\n\n", p.name);
    if !root_summary.is_empty() {
        let _ = writeln!(llms, "> {root_summary}\n");
    }
    let _ = write!(llms, "## Modules\n{llms_modules}");
    files.push(DocFile {
        path: "llms.txt".to_owned(),
        text: llms,
    });
    files.push(DocFile {
        path: "llms-full.txt".to_owned(),
        text: full,
    });
    files
}

#[cfg(test)]
mod tests;
