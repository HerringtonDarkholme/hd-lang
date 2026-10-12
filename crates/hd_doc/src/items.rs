//! The documented items of one module, read from its source: each
//! declaration's documentation through `hd_syntax::doc_text`, and its
//! signature sliced from the source as written (`cli.doc.signature`), with
//! no body (`cli.doc.concise`) and the members the items table lists.

use hd_base::TokenIdx;
use hd_syntax::{Decl, NodeRef, Parse, SyntaxKind, TokenBuf, TokenKind, doc_text};

/// One heading of a module page: an item (level 2) or a member (level 3).
pub(crate) struct Section {
    /// The anchor, as `cli.test.doc.name` forms `<item>`: `slugify`,
    /// `Slug.new`.
    pub anchor: String,
    pub signature: String,
    pub doc: String,
    /// The implementation heads of a type (`cli.doc.impls`).
    pub impls: Vec<String>,
}

/// An item with its member sections.
pub(crate) struct Item {
    pub section: Section,
    /// The type name inherent `impl` blocks attach methods to.
    pub type_name: Option<String>,
    pub members: Vec<Section>,
}

struct Src<'a> {
    text: &'a str,
    toks: &'a TokenBuf,
}

impl Src<'_> {
    fn start(&self, t: TokenIdx) -> usize {
        self.toks.start[t.idx()] as usize
    }
    fn end(&self, t: TokenIdx) -> usize {
        self.toks.end[t.idx()] as usize
    }
    fn token(&self, t: TokenIdx) -> &str {
        self.toks.text(t, self.text)
    }
    /// The node's own first token, past its decorators: `pub` or the
    /// declaration keyword.
    fn head(node: NodeRef<'_>) -> TokenIdx {
        node.direct_tokens().next().unwrap_or(node.first_token())
    }
    /// The source from the head to the `:` that opens the body, or to the
    /// node's end when it has none, as written.
    fn signature(&self, node: NodeRef<'_>, with_colon: bool) -> String {
        let head = Self::head(node);
        let end = match node.direct_token(self.toks, TokenKind::Colon) {
            Some(colon) if with_colon => self.end(colon),
            Some(colon) => self.start(colon),
            None => self.end(node.last_token()),
        };
        self.slice(self.start(head), end)
    }
    fn whole(&self, node: NodeRef<'_>) -> String {
        self.slice(self.start(Self::head(node)), self.end(node.last_token()))
    }
    /// A slice whose later lines lose the indentation of its first line.
    fn slice(&self, lo: usize, hi: usize) -> String {
        let line_start = self.text[..lo].rfind('\n').map_or(0, |i| i + 1);
        let indent = lo - line_start;
        let mut out = String::new();
        for (i, line) in self.text[lo..hi.max(lo)].trim_end().split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
                let spaces = line.len() - line.trim_start_matches(' ').len();
                out.push_str(&line[spaces.min(indent)..]);
            } else {
                out.push_str(line);
            }
        }
        out
    }
    /// The documentation above the node, and above its head when
    /// decorators stand between (`lex.doc.attach`).
    fn doc(&self, node: NodeRef<'_>) -> String {
        let first = node.first_token();
        let head = Self::head(node);
        let mut parts = vec![doc_text(self.text, self.toks, first)];
        if head != first {
            parts.push(doc_text(self.text, self.toks, head));
        }
        parts.retain(|p| !p.is_empty());
        parts.join("\n")
    }
    fn name(&self, node: NodeRef<'_>) -> Option<String> {
        node.name(self.toks).map(|t| self.token(t).to_owned())
    }
    fn is_pub(&self, node: NodeRef<'_>) -> bool {
        node.direct_token(self.toks, TokenKind::KwPub).is_some()
    }
}

/// Indents every line of a member's text by four spaces.
fn indented(text: &str) -> String {
    text.lines()
        .map(|l| {
            if l.is_empty() {
                String::new()
            } else {
                format!("    {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn members(node: NodeRef<'_>) -> Vec<NodeRef<'_>> {
    Decl::cast(node)
        .and_then(Decl::body)
        .map(|b| b.node().children().collect())
        .unwrap_or_default()
}

/// The items of a module source, in source order, with the members of
/// each and the methods of its inherent `impl` blocks. Private items and
/// members are kept only when `private` (`cli.doc.items`,
/// `cli.doc.private`).
pub(crate) fn items(text: &str, parse: &Parse, private: bool) -> Vec<Item> {
    let s = Src {
        text,
        toks: &parse.tokens,
    };
    let mut out: Vec<Item> = Vec::new();
    let mut methods: Vec<(String, Section)> = Vec::new();
    for node in parse.tree.root().children() {
        if node.kind() == SyntaxKind::ImplDecl {
            methods.extend(inherent_methods(&s, node, private));
            continue;
        }
        if Decl::cast(node).is_none() || !(private || s.is_pub(node)) {
            continue;
        }
        let Some(name) = s.name(node) else { continue };
        out.push(item(&s, node, name, private));
    }
    for (owner, m) in methods {
        if let Some(it) = out
            .iter_mut()
            .find(|it| it.type_name.as_deref() == Some(owner.as_str()))
        {
            it.members.push(m);
        }
    }
    out
}

fn item(s: &Src<'_>, node: NodeRef<'_>, name: String, private: bool) -> Item {
    let mut member_sections = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut omitted = false;
    for m in members(node) {
        let shown = match m.kind() {
            SyntaxKind::DataField => private || s.is_pub(m),
            SyntaxKind::EmbeddedField | SyntaxKind::EnumVariant => true,
            SyntaxKind::FnDecl => node.kind() == SyntaxKind::TraitDecl,
            SyntaxKind::AssociatedTypeDecl => node.kind() == SyntaxKind::TraitDecl,
            _ => false,
        };
        if !shown {
            omitted |= m.kind() == SyntaxKind::DataField;
            continue;
        }
        let text = if m.kind() == SyntaxKind::FnDecl {
            s.signature(m, false)
        } else {
            s.whole(m)
        };
        lines.push(indented(&text));
        let doc = s.doc(m);
        if let Some(member) = s.name(m).filter(|_| !doc.is_empty()) {
            member_sections.push(Section {
                anchor: format!("{name}.{member}"),
                signature: text,
                doc,
                impls: Vec::new(),
            });
        }
    }
    if omitted {
        lines.push("    # private fields omitted".to_owned());
    }
    let kind = node.kind();
    let signature = match kind {
        SyntaxKind::FnDecl => s.signature(node, false),
        SyntaxKind::TypeDecl => s.whole(node),
        _ if lines.is_empty() => s.signature(node, false),
        _ => format!("{}\n{}", s.signature(node, true), lines.join("\n")),
    };
    let is_type = matches!(
        kind,
        SyntaxKind::DataDecl | SyntaxKind::EnumDecl | SyntaxKind::TypeDecl
    );
    Item {
        section: Section {
            anchor: name.clone(),
            signature,
            doc: s.doc(node),
            impls: Vec::new(),
        },
        type_name: is_type.then_some(name),
        members: member_sections,
    }
}

/// The methods of an inherent `impl T:` block, each as `T.method`.
fn inherent_methods(s: &Src<'_>, node: NodeRef<'_>, private: bool) -> Vec<(String, Section)> {
    let Some((None, Some(target))) =
        hd_syntax::ImplDecl::cast(node).map(hd_syntax::ImplDecl::header_types)
    else {
        return Vec::new();
    };
    let Some(owner) = s.name(target) else {
        return Vec::new();
    };
    members(node)
        .into_iter()
        .filter(|m| m.kind() == SyntaxKind::FnDecl && (private || s.is_pub(*m)))
        .filter_map(|m| {
            let name = s.name(m)?;
            Some((
                owner.clone(),
                Section {
                    anchor: format!("{owner}.{name}"),
                    signature: s.signature(m, false),
                    doc: s.doc(m),
                    impls: Vec::new(),
                },
            ))
        })
        .collect()
}

/// A declaration of a module as the link check sees it: whether it is
/// `pub`, and its members by name with whether each is shown without
/// `--private`.
pub(crate) struct Declared {
    pub name: String,
    pub public: bool,
    pub members: Vec<(String, bool)>,
}

/// Every top-level declaration of a module source, private ones included,
/// with the members a link may name: fields, variants, trait methods and
/// the methods of inherent `impl` blocks.
pub(crate) fn declared(text: &str, parse: &Parse) -> Vec<Declared> {
    let s = Src {
        text,
        toks: &parse.tokens,
    };
    let mut out: Vec<Declared> = Vec::new();
    let mut methods: Vec<(String, String, bool)> = Vec::new();
    for node in parse.tree.root().children() {
        if node.kind() == SyntaxKind::ImplDecl {
            let Some((None, Some(target))) =
                hd_syntax::ImplDecl::cast(node).map(hd_syntax::ImplDecl::header_types)
            else {
                continue;
            };
            let Some(owner) = s.name(target) else {
                continue;
            };
            for m in members(node) {
                if m.kind() == SyntaxKind::FnDecl
                    && let Some(name) = s.name(m)
                {
                    methods.push((owner.clone(), name, s.is_pub(m)));
                }
            }
            continue;
        }
        if Decl::cast(node).is_none() {
            continue;
        }
        let Some(name) = s.name(node) else { continue };
        let mut shown = Vec::new();
        for m in members(node) {
            if let Some(member) = s.name(m) {
                let public = match m.kind() {
                    SyntaxKind::DataField => s.is_pub(m),
                    _ => true,
                };
                shown.push((member, public));
            }
        }
        out.push(Declared {
            name,
            public: s.is_pub(node),
            members: shown,
        });
    }
    for (owner, name, public) in methods {
        if let Some(d) = out.iter_mut().find(|d| d.name == owner) {
            d.members.push((name, public));
        }
    }
    out
}
