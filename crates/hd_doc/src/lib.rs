#![forbid(unsafe_code)]
//! `hd_doc`: the pages of `hd doc` (spec/cli/command-line.md
//! "Documentation", `f1c55020:future-work/compiler/doc.md`). Each module's page is
//! rendered from its source, whose declarations give the signatures as
//! written and the `##` documentation, and from the package's folder
//! interfaces, which give each type's implementations. Pages are built as
//! strings: the Markdown copies the documentation unchanged, and the HTML
//! holds the same content under the same anchors.

mod html;
mod impls;
mod items;
mod links;
mod lookup;
mod stdref;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use items::{Item, Section};
use links::{Links, Site};

pub use hd_resolve::Named;
pub use links::BrokenLink;

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
    /// The scope of each module, by its full path (`pkg`'s root module is
    /// the package's `root`): what `` [`NAME`] `` links resolve in
    /// (`cli.doc.link.scope`). Empty leaves every link as written.
    pub scopes: &'a [(String, Vec<(String, Named)>)],
    /// The package's dependencies: each key after `dep.` and the root
    /// module of its package.
    pub deps: &'a [(String, String)],
    /// The standard library's modules, `text` for `std.text`: what
    /// `hd doc std.…` renders (`cli.doc.name.std`) and what links into
    /// `std` check (`cli.doc.link.external`).
    pub std: &'a [Module<'a>],
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

/// One entry of a list of links: the code shown, where it links to in the
/// Markdown and in the HTML page, and its summary in each form.
struct Entry {
    code: String,
    md_href: String,
    html_href: String,
    md: String,
    html: String,
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
    /// Documentation text, in its Markdown form and in the form the HTML
    /// page renders (their links differ in the extension of the pages).
    fn text(&mut self, md: &str, html: &str) {
        if md.trim().is_empty() {
            return;
        }
        let _ = writeln!(self.md, "{md}\n");
        self.html.push_str(&html::blocks(html));
    }
    fn signature(&mut self, sig: &str) {
        let _ = writeln!(self.md, "```hd\n{sig}\n```\n");
        let _ = writeln!(
            self.html,
            "<pre><code class=\"language-hd\">{}</code></pre>",
            html::escape(sig)
        );
    }
    /// A list of links to items and modules, each with its summary.
    fn links(&mut self, entries: &[Entry]) {
        if entries.is_empty() {
            return;
        }
        self.html.push_str("<ul>\n");
        for e in entries {
            let tail = |summary: &str| {
                if summary.is_empty() {
                    String::new()
                } else {
                    format!(": {summary}")
                }
            };
            let _ = writeln!(self.md, "- [`{}`]({}){}", e.code, e.md_href, tail(&e.md));
            let _ = writeln!(
                self.html,
                "<li><a href=\"{}\"><code>{}</code></a>{}</li>",
                html::escape(&e.html_href),
                html::escape(&e.code),
                html::inline(&tail(&e.html))
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
    fn section(&mut self, level: usize, s: &Section, base: &str, cx: &mut Cx<'_, '_>) {
        self.heading(level, "", &s.anchor, Some(&s.anchor));
        self.signature(&s.signature);
        let doc = with_examples(&s.doc, &format!("{base}.{}", s.anchor), cx.counts);
        let (md, html) = cx.both(&doc);
        self.text(&md, &html);
        self.lines(&s.impls);
    }
}

/// What a module page is built against.
struct Cx<'a, 'b> {
    counts: &'a mut HashMap<String, usize>,
    /// The links of the package; `None` copies link text unchanged.
    links: Option<&'a Links<'b>>,
    module: String,
    /// The directory the text is read from, and the page it is on.
    dir: String,
    here: Option<String>,
}

impl Cx<'_, '_> {
    /// A documentation text with its links resolved, for the Markdown page
    /// and for the HTML page.
    fn both(&self, text: &str) -> (String, String) {
        let Some(links) = self.links else {
            return (text.to_owned(), text.to_owned());
        };
        let at = |ext| Site {
            module: &self.module,
            dir: &self.dir,
            here: self.here.as_deref(),
            ext,
        };
        (
            links.resolve(text, &at("md")),
            links.resolve(text, &at("html")),
        )
    }
}

/// The path of the directory of a page stem: `shop` for `shop/cart`.
fn dir_of(stem: &str) -> String {
    stem.rsplit_once('/')
        .map_or_else(String::new, |(d, _)| d.to_owned())
}

/// One module's page (`cli.doc.page`, `cli.doc.item`), and its module
/// documentation's summary. `dir` is where the text is read from, which is
/// the page's own directory unless the page is copied elsewhere
/// (`llms-full.txt`); `links` resolves `` [`NAME`] ``.
fn module_page(
    m: &Module<'_>,
    p: &Package<'_>,
    impl_lines: &impls::ImplLines,
    links: Option<&Links<'_>>,
    in_place: bool,
    foreign: bool,
) -> (Page, String) {
    let parse = hd_syntax::parse(m.text.as_bytes());
    let module_doc = hd_syntax::module_doc_text(m.text, &parse.tokens);
    let mut items: Vec<Item> = items::items(m.text, &parse, p.private, foreign);
    let full = module_path(p, m.path);
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
    let stem = page_stem(m.path);
    let mut cx = Cx {
        counts: &mut counts,
        links,
        module: full.clone(),
        dir: if in_place {
            dir_of(&stem)
        } else {
            String::new()
        },
        here: in_place.then_some(stem),
    };
    let mut page = Page::default();
    page.heading(1, "Module ", name, None);
    let doc = with_examples(&module_doc, &base, cx.counts);
    let (md, html) = cx.both(&doc);
    page.text(&md, &html);
    let entries: Vec<_> = items
        .iter()
        .map(|it| {
            let a = &it.section.anchor;
            let (md, html) = cx.both(&summary(&it.section.doc));
            Entry {
                code: a.clone(),
                md_href: format!("#{a}"),
                html_href: format!("#{a}"),
                md,
                html,
            }
        })
        .collect();
    page.links(&entries);
    for it in &items {
        page.section(2, &it.section, &base, &mut cx);
        for member in &it.members {
            page.section(3, member, &base, &mut cx);
        }
    }
    (page, summary(&module_doc))
}

/// The documentation pages and the links they could not resolve.
pub struct Rendered {
    pub files: Vec<DocFile>,
    /// Each link that names no documented item, once
    /// (`cli.doc.link.broken`). The pages are complete all the same.
    pub broken: Vec<BrokenLink>,
}

/// `text`, a module's summary, with its links resolved from the output
/// directory in the pages of `ext`.
fn from_root(links: &Links<'_>, text: &str, module: &str, ext: &str) -> String {
    links.resolve(
        text,
        &Site {
            module,
            dir: "",
            here: None,
            ext,
        },
    )
}

/// Every output file of `hd doc` (`cli.doc.files`): each module's pages,
/// the entry page, `llms.txt` and `llms-full.txt`.
#[must_use]
pub fn render(p: &Package<'_>) -> Rendered {
    let impl_lines = impls::impl_lines(p.root, p.ifaces);
    let links = Links::new(p);
    let links = (!p.scopes.is_empty()).then_some(&links);
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
        let (page, sum) = module_page(m, p, &impl_lines, links, true, false);
        let stem = page_stem(m.path);
        let name = shown_name(m.path);
        let module = module_path(p, m.path);
        let (sum_md, sum_html) = match links {
            Some(l) => (
                from_root(l, &sum, &module, "md"),
                from_root(l, &sum, &module, "html"),
            ),
            None => (sum.clone(), sum),
        };
        if m.path.is_empty() {
            root_summary.clone_from(&sum_md);
        }
        let tail = if sum_md.is_empty() {
            String::new()
        } else {
            format!(": {sum_md}")
        };
        let _ = writeln!(llms_modules, "- [{name}]({stem}.md){tail}");
        entries.push(Entry {
            code: name.to_owned(),
            md_href: format!("{stem}.md"),
            html_href: format!("{stem}.html"),
            md: sum_md,
            html: sum_html,
        });
        if !full.is_empty() {
            full.push('\n');
        }
        // The pages of `llms-full.txt` are read from the output directory.
        full.push_str(&module_page(m, p, &impl_lines, links, false, false).0.md);
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
    Rendered {
        files,
        broken: links.map_or_else(Vec::new, Links::broken),
    }
}

/// A module's full path: the package's `root` for the root module.
fn module_path(p: &Package<'_>, path: &str) -> String {
    if path.is_empty() {
        p.root.to_owned()
    } else {
        format!("{}.{path}", p.root)
    }
}

/// What `hd doc NAME` prints (`cli.doc.name`): the Markdown of one item's
/// section, of a whole module page, or of a standard library entry. `Err`
/// holds the names to suggest when NAME names nothing documented.
pub fn lookup(p: &Package<'_>, name: &str) -> Result<String, Vec<String>> {
    lookup::lookup(p, name)
}

use lookup::rank;

#[cfg(test)]
mod tests;
