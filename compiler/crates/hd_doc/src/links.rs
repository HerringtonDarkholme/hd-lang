//! `` [`NAME`] `` links in documentation text (`cli.doc.link.*`). NAME is
//! resolved in the module scope the resolver built for the documented
//! item's module (`Package::scopes`): its own declarations and the names
//! its `use` declarations bind. A dotted NAME walks modules and members
//! from there. A name the scope does not bind is broken, as is a private
//! item of a run without `--private`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use hd_base::{DefId, Symbol};
use hd_intern::{PathTable, ShardedInterner};
use hd_resolve::{ItemData, KnownItems, Named, Names, decode_items};
use hd_types::{InternPool, TyData};

use crate::items::{self, Declared};
use crate::{Package, module_path, page_stem, stdref};

/// A link that names no documented item (`cli.doc.link.broken`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokenLink {
    /// The module whose documentation holds the link, as `pkg` or
    /// `shop.cart`.
    pub module: String,
    /// The NAME as written.
    pub name: String,
    /// Whether NAME resolves to a private item that this run does not
    /// document, rather than to nothing.
    pub private: bool,
}

/// What a NAME resolved to.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// An item or module of this package, on its page.
    Page {
        stem: String,
        anchor: Option<String>,
    },
    /// A link to the standard library reference.
    Std(&'static str),
    /// Checked, but shown as code: a dependency item, a std module.
    Plain,
    Broken {
        private: bool,
    },
}

/// Where a walk stands.
enum At {
    Module(String),
    Item {
        module: String,
        name: String,
        member: Option<String>,
    },
}

/// What a page's links are relative to.
pub(crate) struct Site<'a> {
    /// The documented module's full path.
    pub module: &'a str,
    /// The directory the text is read from, below the output directory.
    pub dir: &'a str,
    /// The page's own stem: a link to it is the bare anchor.
    pub here: Option<&'a str>,
    /// `md` or `html`: the extension of the pages linked to.
    pub ext: &'a str,
}

pub(crate) struct Links<'a> {
    p: &'a Package<'a>,
    scopes: HashMap<&'a str, HashMap<&'a str, &'a Named>>,
    /// The package's declarations by full module path.
    decls: HashMap<String, Vec<Declared>>,
    /// The anchors each package page has.
    anchors: HashMap<String, HashSet<String>>,
    /// The dependencies' public items, by `module.name` and
    /// `module.name.member`.
    dep_items: HashSet<String>,
    /// Re-exports: (module, name) to the declaring module and name.
    exports: HashMap<(String, String), (String, String)>,
    modules: HashSet<String>,
    broken: RefCell<Vec<BrokenLink>>,
}

impl<'a> Links<'a> {
    pub(crate) fn new(p: &'a Package<'a>) -> Self {
        let scopes = p
            .scopes
            .iter()
            .map(|(m, names)| {
                let map = names.iter().map(|(n, t)| (n.as_str(), t)).collect();
                (m.as_str(), map)
            })
            .collect();
        let mut decls = HashMap::new();
        let mut anchors = HashMap::new();
        let mut modules = HashSet::new();
        for m in p.modules {
            let parse = hd_syntax::parse(m.text.as_bytes());
            let path = module_path(p, m.path);
            let shown = items::items(m.text, &parse, p.private);
            let set = shown
                .iter()
                .flat_map(|it| {
                    std::iter::once(it.section.anchor.clone())
                        .chain(it.members.iter().map(|s| s.anchor.clone()))
                })
                .collect();
            anchors.insert(path.clone(), set);
            decls.insert(path.clone(), items::declared(m.text, &parse));
            modules.insert(path);
        }
        let mut links = Self {
            p,
            scopes,
            decls,
            anchors,
            dep_items: HashSet::new(),
            exports: HashMap::new(),
            modules,
            broken: RefCell::default(),
        };
        links.read_ifaces();
        links
    }

    /// The exports and public items of the folders outside this package and
    /// `std`, which the checks of dependency links read.
    fn read_ifaces(&mut self) {
        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let known = KnownItems::new(&paths);
        let names = Names {
            pool: &pool,
            paths: &paths,
            syms: &syms,
            known: &known,
        };
        let below = format!("{}.", self.p.root);
        for (folder, blob) in self.p.ifaces {
            let root = folder.split('.').next().unwrap_or("");
            if root == "std" || folder == self.p.root || folder.starts_with(&below) {
                continue;
            }
            let Some((items, exports)) = decode_items(&names, blob) else {
                continue;
            };
            let public: HashSet<DefId> = items.iter().filter(|i| i.public).map(|i| i.def).collect();
            for it in items.iter().filter(|i| i.public) {
                let module = names.module_of(it.def);
                let name = names.display_name(it.def).to_owned();
                self.modules.insert(module.clone());
                let key = format!("{module}.{name}");
                let members: Vec<Symbol> = match &it.data {
                    ItemData::Data(fields) => {
                        fields.iter().filter(|f| f.public).map(|f| f.name).collect()
                    }
                    ItemData::Enum { shared, variants } => shared
                        .iter()
                        .filter(|f| f.public)
                        .map(|f| f.name)
                        .chain(variants.iter().map(|v| v.name))
                        .collect(),
                    ItemData::Trait(t) => t
                        .methods
                        .iter()
                        .map(|m| m.0)
                        .chain(t.assoc.iter().map(|a| a.0))
                        .collect(),
                    _ => Vec::new(),
                };
                for m in members {
                    self.dep_items.insert(format!("{key}.{}", names.text(m)));
                }
                if let ItemData::Impl {
                    trait_,
                    self_ty,
                    methods,
                    ..
                } = &it.data
                    && *trait_ == DefId::NONE
                    && let TyData::Adt { def, .. } = pool.get(*self_ty)
                {
                    let owner = format!("{}.{}", names.module_of(def), names.display_name(def));
                    for (m, _) in methods.iter().filter(|(_, d)| public.contains(d)) {
                        self.dep_items.insert(format!("{owner}.{}", names.text(*m)));
                    }
                }
                self.dep_items.insert(key);
            }
            for e in exports {
                let to = (names.module_of(e.def), names.display_name(e.def).to_owned());
                self.modules.insert(e.module.clone());
                self.exports
                    .insert((e.module, names.text(e.name).to_owned()), to);
            }
        }
    }

    /// The links that resolved to nothing so far, once each.
    pub(crate) fn broken(&self) -> Vec<BrokenLink> {
        self.broken.borrow().clone()
    }

    /// `text` with each `` [`NAME`] `` replaced: by a link to its target
    /// when it resolves, else by NAME in code.
    pub(crate) fn resolve(&self, text: &str, at: &Site<'_>) -> String {
        let mut out = String::new();
        let mut fenced = false;
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
            }
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
            }
            if fenced || line.trim_start().starts_with("```") {
                out.push_str(line);
            } else {
                self.line(line, at, &mut out);
            }
        }
        out
    }

    /// One line of text: links in it are resolved, code spans copied as
    /// written.
    fn line(&self, line: &str, at: &Site<'_>, out: &mut String) {
        let mut rest = line;
        while let Some(i) = rest.find('`') {
            // A `[` right before the backtick opens a link, not a code span.
            let start = if rest[..i].ends_with('[') { i - 1 } else { i };
            out.push_str(&rest[..start]);
            rest = &rest[start..];
            if let Some((name, used)) = shortcut(rest) {
                self.link(name, at, out);
                rest = &rest[used..];
                continue;
            }
            if rest.starts_with('[') {
                out.push('[');
                rest = &rest[1..];
            }
            let ticks = rest.len() - rest.trim_start_matches('`').len();
            let fence = "`".repeat(ticks);
            let end = rest[ticks..]
                .find(&fence)
                .map_or(rest.len(), |e| ticks + e + ticks);
            out.push_str(&rest[..end]);
            rest = &rest[end..];
        }
        out.push_str(rest);
    }

    fn link(&self, name: &str, at: &Site<'_>, out: &mut String) {
        match self.walk(at.module, name) {
            Outcome::Page { stem, anchor } => {
                let href = href(&stem, anchor.as_deref(), at);
                let _ = write!(out, "[`{name}`]({href})");
            }
            Outcome::Std(url) => {
                let _ = write!(out, "[`{name}`]({url})");
            }
            Outcome::Plain => {
                let _ = write!(out, "`{name}`");
            }
            Outcome::Broken { private } => {
                let b = BrokenLink {
                    module: at.module.to_owned(),
                    name: name.to_owned(),
                    private,
                };
                let mut all = self.broken.borrow_mut();
                if !all.contains(&b) {
                    all.push(b);
                }
                let _ = write!(out, "`{name}`");
            }
        }
    }

    /// NAME resolved from the scope of `module`.
    fn walk(&self, module: &str, name: &str) -> Outcome {
        let segs: Vec<&str> = name.split('.').collect();
        if segs.iter().any(|s| s.is_empty()) {
            return Outcome::Broken { private: false };
        }
        let (mut at, rest) = match segs[0] {
            "std" => (At::Module("std".to_owned()), &segs[1..]),
            "pkg" => (At::Module(self.p.root.to_owned()), &segs[1..]),
            "dep" if segs.len() > 1 && self.p.deps.iter().any(|d| d.0 == segs[1]) => {
                let root = self.p.deps.iter().find(|d| d.0 == segs[1]);
                let root = root.map_or("", |d| d.1.as_str());
                (At::Module(root.to_owned()), &segs[2..])
            }
            first => {
                let bound = self.scopes.get(module).and_then(|s| s.get(first));
                match bound {
                    Some(Named::Module(m)) => (At::Module(m.clone()), &segs[1..]),
                    Some(Named::Item { module, name }) => (Self::item(module, name), &segs[1..]),
                    None => return Outcome::Broken { private: false },
                }
            }
        };
        for seg in rest {
            at = match at {
                At::Module(m) => {
                    let child = format!("{m}.{seg}");
                    if self.module_exists(&child) {
                        At::Module(child)
                    } else {
                        let (dm, dn) = self
                            .exports
                            .get(&(m.clone(), (*seg).to_owned()))
                            .cloned()
                            .unwrap_or((m, (*seg).to_owned()));
                        Self::item(&dm, &dn)
                    }
                }
                At::Item {
                    module,
                    name,
                    member: None,
                } => At::Item {
                    module,
                    name,
                    member: Some((*seg).to_owned()),
                },
                At::Item { .. } => return Outcome::Broken { private: false },
            };
        }
        self.outcome(&at)
    }

    fn item(module: &str, name: &str) -> At {
        At::Item {
            module: module.to_owned(),
            name: name.to_owned(),
            member: None,
        }
    }

    fn module_exists(&self, m: &str) -> bool {
        self.modules.contains(m) || (m.starts_with("std.") && stdref::has_module(m))
    }

    fn outcome(&self, at: &At) -> Outcome {
        let module = match at {
            At::Module(m) | At::Item { module: m, .. } => m,
        };
        let root = module.split('.').next().unwrap_or("");
        if root == "std" {
            return match at {
                At::Module(m) if stdref::has_module(m) => Outcome::Plain,
                At::Item {
                    module,
                    name,
                    member,
                } => {
                    let item = member
                        .as_ref()
                        .map_or_else(|| name.clone(), |m| format!("{name}.{m}"));
                    stdref::link(module, &item)
                        .map_or(Outcome::Broken { private: false }, Outcome::Std)
                }
                At::Module(_) => Outcome::Broken { private: false },
            };
        }
        if root != self.p.root {
            let known = match at {
                At::Module(m) => self.modules.contains(m),
                At::Item {
                    module,
                    name,
                    member: None,
                } => self.dep_items.contains(&format!("{module}.{name}")),
                At::Item {
                    module,
                    name,
                    member: Some(m),
                } => self.dep_items.contains(&format!("{module}.{name}.{m}")),
            };
            return if known {
                Outcome::Plain
            } else {
                Outcome::Broken { private: false }
            };
        }
        self.package(at)
    }

    /// A target in this package: documented on a page, or not.
    fn package(&self, at: &At) -> Outcome {
        let path_of = |m: &str| {
            if m == self.p.root {
                String::new()
            } else {
                m[self.p.root.len() + 1..].to_owned()
            }
        };
        match at {
            At::Module(m) => {
                if self.decls.contains_key(m) {
                    Outcome::Page {
                        stem: page_stem(&path_of(m)),
                        anchor: None,
                    }
                } else {
                    Outcome::Broken {
                        private: self.modules.contains(m),
                    }
                }
            }
            At::Item {
                module,
                name,
                member,
            } => {
                let Some(d) = self
                    .decls
                    .get(module)
                    .and_then(|ds| ds.iter().find(|d| d.name == *name))
                else {
                    return Outcome::Broken { private: false };
                };
                let shown = |public: bool| public || self.p.private;
                let mut public = d.public;
                let mut anchor = name.clone();
                if let Some(m) = member {
                    let Some((_, member_public)) = d.members.iter().find(|(n, _)| n == m) else {
                        return Outcome::Broken { private: false };
                    };
                    public = public && *member_public;
                    let own = format!("{name}.{m}");
                    if self.anchors.get(module).is_some_and(|a| a.contains(&own)) {
                        anchor = own;
                    }
                }
                if !shown(public) {
                    return Outcome::Broken { private: true };
                }
                Outcome::Page {
                    stem: page_stem(&path_of(module)),
                    anchor: Some(anchor),
                }
            }
        }
    }
}

/// `` [`NAME`] `` at the start of `text`, not followed by a link target or
/// a reference: NAME and the bytes used.
fn shortcut(text: &str) -> Option<(&str, usize)> {
    let tail = text.strip_prefix("[`")?;
    let name = &tail[..tail.find("`]")?];
    let used = 2 + name.len() + 2;
    let ok = !name.is_empty()
        && !name.contains(['`', '[', ']', ' '])
        && !text[used..].starts_with(['(', '[']);
    ok.then_some((name, used))
}

/// The relative link from the text of `at` to a page and anchor
/// (`cli.doc.relative`).
fn href(stem: &str, anchor: Option<&str>, at: &Site<'_>) -> String {
    let frag = anchor.map_or_else(String::new, |a| format!("#{a}"));
    if at.here == Some(stem) && anchor.is_some() {
        return frag;
    }
    let from: Vec<&str> = at.dir.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = stem.split('/').collect();
    let common = from
        .iter()
        .zip(&to[..to.len() - 1])
        .take_while(|(a, b)| a == b)
        .count();
    let mut path = "../".repeat(from.len() - common);
    path.push_str(&to[common..].join("/"));
    format!("{path}.{}{frag}", at.ext)
}
