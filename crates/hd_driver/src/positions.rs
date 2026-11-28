//! Anchors to source lines (`crates/hd_mono/DESIGN.md` §13.8 "Positions", `crates/hd_wasm/DESIGN.md`
//! §15.9). A linked module's `hd.lines` names a call by its anchor: an
//! item's stable path and a TIR instruction index in that item's body. The
//! anchor is resolved against the current sources only when a panic's
//! frames are printed (`cli.test.report.frame`): the module's `check`
//! entry gives the instruction's syntax node and the node's frame site,
//! its code line and the newlines past that line's start
//! ([`code_line_of`]), and the file's skim gives the code line's line. So
//! a comment edit above a panic moves the line it reports, and nothing
//! compiled or cached; and a warm run's positions decode no body's
//! tables and parse no file (#2423). An entry without sites falls back to
//! a parse of the file the run read.
//!
//! A frame names its function as hd names it, not by its stable path: a
//! module's path as its declarations print (trait.typeid.name.*), then the
//! function, a method after its type's name with no generic arguments
//! (`std.collections.List.remove`), and a test case's body as
//! `test "NAME"`. The module carries its printed path, so the line table
//! keeps stable paths and a release build's `hd.names` names the same.

use hd_base::HashMap;
use std::sync::{Arc, OnceLock};

use hd_base::wire::{Reader, Writer};
use hd_base::{DefId, FileId, Hash128, NodeIdx, PathId, SliceSort as _};
use hd_intern::{PathKind, PathTable, ShardedInterner};
use hd_resolve::{Generic, Names, Src, show_ty_full};
use hd_syntax::{Parse, parse};
use hd_types::AdtId;
use hd_types::{Ty, TyData};

/// What resolves anchors for one built program: each of its modules, by
/// module path, with the path its frames print; and its test cases'
/// registered names, by their bodies' stable paths.
pub struct Positions {
    pub(crate) modules: Vec<(String, String, Arc<ModuleSource>)>,
    pub(crate) tests: HashMap<String, String>,
}

/// The modules of the programs one command builds, each kept once: the
/// unit-test program, every integration test and every doc test read the
/// same std modules, which are kept and decoded once for all of them.
#[derive(Default)]
pub struct PositionPool {
    by_path: HashMap<String, Vec<Arc<ModuleSource>>>,
}

/// One module as a run saw it: its file, text and `check` entry. Nothing
/// is read, decoded or parsed until an anchor of the module is asked for,
/// and then once: a warm run reads no text for its positions
/// (`crates/hd_cache/DESIGN.md` §5.5).
pub struct ModuleSource {
    pub file: String,
    /// The hash of its text, which tells two runs' versions apart.
    pub hash: Hash128,
    /// Its text: module `index` of `texts`.
    pub texts: crate::Texts<'static>,
    pub index: usize,
    pub check: Option<Arc<[u8]>>,
    pub input: Arc<ModuleInput>,
    pub read: OnceLock<Option<Module>>,
}

/// Source data the build already read. Missing parses and bodies remain lazy:
/// a warm link need not decode TIR or parse modules solely to retain them.
#[derive(Default)]
pub struct ModuleInput {
    pub parse: Option<Arc<Parse>>,
    pub bodies: HashMap<Hash128, (String, Body)>,
    /// The LF line each of the file's code lines starts (its skim's),
    /// which the frame sites a `check` entry stores are relative to.
    pub code_lines: Option<Arc<[u32]>>,
}

impl ModuleInput {
    pub(super) fn insert(
        &mut self,
        hash: Hash128,
        path: String,
        body: &hd_tir::Body,
        syms: &ShardedInterner,
    ) {
        self.bodies.insert(hash, (path, Body::from_tir(body, syms)));
    }
}

/// One module, read: its bodies, by item path, and the parse of its text,
/// made only when a body without stored sites or a span asks for it.
pub struct Module {
    pub bodies: HashMap<String, Body>,
    pub parse: OnceLock<Arc<Parse>>,
    /// LF line starts, shared by every anchor lookup in this module that
    /// reads the parse. The lexer also records bare CR lines; positions
    /// have always counted LF.
    pub line_starts: OnceLock<Vec<u32>>,
}

/// A body of a module: its syntax node per instruction, and its name in
/// the module (`List.remove`), from the module's frame-name record.
#[derive(Clone)]
pub struct Body {
    pub syn: Arc<[NodeIdx]>,
    pub name: String,
    /// The body of a `@track_caller` function, whose frames a panic's
    /// frame list leaves out (`fn.caller.frames`).
    pub hidden: bool,
    /// Its operations whose `usize` type is a literal's default, by
    /// instruction, each with the name of the local it reads.
    pub fallback: Arc<[(u32, Option<String>)]>,
    /// Each syntax node its instructions name, sorted, with the 1-based
    /// LF line where its frame stands ([`site`]) in the current text,
    /// from the module's `check` entry and the skim's code lines; `None`
    /// when either is missing, and then the module's parse answers.
    pub sites: Option<Arc<[(u32, u32)]>>,
}

impl Body {
    fn from_tir(body: &hd_tir::Body, syms: &ShardedInterner) -> Self {
        Self {
            // A derived instance's nodes belong to its template source (#311).
            syn: if body.kind == hd_tir::ir::BodyKind::DeriveInstance {
                Arc::from([])
            } else {
                Arc::from(body.syn.as_slice())
            },
            name: String::new(),
            hidden: false,
            sites: None,
            fallback: body
                .fallback
                .iter()
                .map(|[inst, local]| {
                    let name = local
                        .checked_sub(1)
                        .and_then(|l| body.local_name.get(l as usize))
                        .map(|s| syms.resolve(*s).to_owned());
                    (*inst, name)
                })
                .collect(),
        }
    }
}

impl Body {
    /// A body from its wire form's locations alone
    /// ([`hd_tir::wire::read_locations`]), as [`Body::from_tir`] reads
    /// a decoded one.
    fn from_locations(at: hd_tir::wire::Locations, derived: bool) -> Self {
        Self {
            syn: if derived {
                Arc::from([])
            } else {
                Arc::from(at.syn)
            },
            name: String::new(),
            hidden: false,
            fallback: Arc::from(at.fallback),
            sites: None,
        }
    }
}

impl ModuleSource {
    fn text(&self) -> &Arc<str> {
        self.texts
            .get(self.index)
            .unwrap_or_else(|| crate::texts::empty())
    }
}

impl PositionPool {
    /// The pool's module of `path` with this file, text (by its hash) and
    /// entry, added when the pool has none.
    fn module(
        &mut self,
        path: &str,
        file: &str,
        (hash, texts, index): (Hash128, &crate::Texts<'static>, usize),
        check: Option<&Arc<[u8]>>,
        input: &Arc<ModuleInput>,
    ) -> Arc<ModuleSource> {
        let same = self.by_path.entry(path.to_owned()).or_default();
        if let Some(m) = same
            .iter()
            .find(|m| m.file == file && m.hash == hash && m.check.as_deref() == check.map(|c| &**c))
        {
            return Arc::clone(m);
        }
        let m = Arc::new(ModuleSource {
            file: file.to_owned(),
            hash,
            texts: texts.clone(),
            index,
            check: check.cloned(),
            input: Arc::clone(input),
            read: OnceLock::new(),
        });
        same.push(Arc::clone(&m));
        m
    }
}

impl Positions {
    pub(crate) fn new(pool: &mut PositionPool, output: &crate::Output) -> Self {
        let modules = output
            .modules
            .iter()
            .enumerate()
            .filter_map(|(m, path)| {
                // A skimmed module's hash comes from its skim, so its text
                // is read only when one of its anchors is asked for.
                let hash = output
                    .source_hashes
                    .get(m)
                    .copied()
                    .flatten()
                    .or_else(|| Some(hd_base::hash128(output.texts.get(m)?.as_bytes())))?;
                let source = pool.module(
                    path,
                    output.files.get(m)?,
                    (hash, &output.texts, m),
                    output.checks.get(m).and_then(Option::as_ref),
                    output.position_inputs.get(m)?,
                );
                Some((path.clone(), output.printed.get(m)?.clone(), source))
            })
            .collect();
        Self {
            modules,
            tests: output.test_bodies.iter().cloned().collect(),
        }
    }

    /// The package-relative file and 1-based line of instruction `inst` in
    /// the body of the item with stable path `item` (`pkg/mod/f`, the path
    /// a function's name holds), or `None` when the run has no such body.
    /// A call uses the end of its argument list; other instructions use
    /// the start of their source expression.
    #[must_use]
    pub fn line(&self, item: &str, inst: u32) -> Option<(&str, u32)> {
        let (_, source) = self.module_of(item)?;
        let module = source.module()?;
        let body = module.bodies.get(item)?;
        let node = *body.syn.get(inst as usize)?;
        if let Some(sites) = &body.sites {
            let i = sites.binary_search_by_key(&node.raw(), |s| s.0).ok()?;
            return Some((source.file.as_str(), sites[i].1));
        }
        let text = source.text();
        let at = site(source.parse(module), text, node)? as usize;
        let line = u32::try_from(
            module
                .line_starts
                .get_or_init(|| text_line_starts(text))
                .partition_point(|&start| start as usize <= at.min(text.len())),
        )
        .ok()?;
        Some((source.file.as_str(), line))
    }

    /// Entry-generated count code has no source expression. Locate it at
    /// the first source instruction of its body, like a function prologue.
    #[cfg(feature = "checked-report")]
    pub(crate) fn entry_line(&self, item: &str) -> Option<(&str, u32)> {
        let (_, source) = self.module_of(item)?;
        let body = source.module()?.bodies.get(item)?;
        (0..body.syn.len())
            .filter_map(|i| self.line(item, u32::try_from(i).ok()?))
            .min_by_key(|(_, line)| *line)
    }

    /// The byte span of instruction `inst` of the body of the item with
    /// stable path `item` in its file's text, with the package-relative
    /// file, or `None` when the run has no such body or the instruction no
    /// source position: what the stage fixtures (`hd_testkit::stages`)
    /// filter a TIR view by.
    #[must_use]
    pub fn span(&self, item: &str, inst: u32) -> Option<(&str, u32, u32)> {
        let (_, source) = self.module_of(item)?;
        let node = *source.module()?.bodies.get(item)?.syn.get(inst as usize)?;
        self.node_span(item, node)
    }

    /// The file and byte span of syntax node `node` of the module that
    /// holds the item with stable path `item`: where an instruction of a
    /// body rewritten after checking stands, by its own `syn` column.
    #[must_use]
    pub fn node_span(&self, item: &str, node: NodeIdx) -> Option<(&str, u32, u32)> {
        let (_, source) = self.module_of(item)?;
        let parse = source.parse(source.module()?);
        if node == NodeIdx::NONE || node.idx() >= parse.tree.len() {
            return None;
        }
        let src = Src {
            parse,
            text: source.text(),
            file: FileId::from_raw(0),
        };
        let s = src.span(parse.tree.node(node));
        Some((source.file.as_str(), s.lo, s.hi))
    }

    /// Whether instruction `inst` of the body of `item` is an operation
    /// whose `usize` type is a literal's default
    /// (`flow.panic.report.fallback`): `Some` with the name of the local
    /// it reads, when it reads one.
    #[must_use]
    pub fn fallback(&self, item: &str, inst: u32) -> Option<Option<&str>> {
        let (_, source) = self.module_of(item)?;
        let body = source.module()?.bodies.get(item)?;
        let at = body.fallback.binary_search_by_key(&inst, |r| r.0).ok()?;
        Some(body.fallback[at].1.as_deref())
    }

    /// The function a frame in the body of the item with stable path
    /// `item` names (`cli.test.report.frame`): a test case's body by its
    /// registered name, as `test "sums prices"`, any other body by its
    /// module's printed path and its name in the module, as
    /// `shop.cart.total` or `std.collections.List.remove`. A body the
    /// run did not check keeps its stable path.
    #[must_use]
    pub fn name(&self, item: &str) -> String {
        if let Some(case) = self.tests.get(item) {
            return format!("test {}", quoted(case));
        }
        let Some((printed, source)) = self.module_of(item) else {
            return item.to_owned();
        };
        match source.module().and_then(|m| m.bodies.get(item)) {
            None => item.to_owned(),
            Some(b) if printed.is_empty() => b.name.clone(),
            Some(b) => format!("{printed}.{}", b.name),
        }
    }

    /// Whether frames in the body of the item with stable path `item` are
    /// left out of a panic's frame list: a `@track_caller` function's body,
    /// so the list names its caller (`fn.caller.frames`).
    #[must_use]
    pub fn hidden(&self, item: &str) -> bool {
        self.module_of(item)
            .and_then(|(_, source)| source.module())
            .and_then(|m| m.bodies.get(item))
            .is_some_and(|b| b.hidden)
    }

    /// The module that declares an item, from its stable path: the package,
    /// then the module's segments, then the item (`PathTable::print`). A
    /// package's root module has no segment of its own. The module comes
    /// with the path its frames print.
    fn module_of(&self, item: &str) -> Option<(&str, &ModuleSource)> {
        let (package, rest) = item.split_once('/')?;
        let find = |path: &str| {
            self.modules
                .iter()
                .find(|(p, _, _)| p == path)
                .map(|(_, printed, m)| (printed.as_str(), &**m))
        };
        rest.split_once('/')
            .and_then(|(module, _)| find(&format!("{package}.{module}")))
            .or_else(|| find(package))
    }
}

impl ModuleSource {
    /// The module, read on its first use.
    fn module(&self) -> Option<&Module> {
        self.read.get_or_init(|| self.read()).as_ref()
    }

    /// The parse of the module's text: the build's, else parsed on first
    /// use.
    fn parse<'m>(&self, module: &'m Module) -> &'m Arc<Parse> {
        module.parse.get_or_init(|| {
            self.input
                .parse
                .clone()
                .unwrap_or_else(|| Arc::new(parse(self.text().as_bytes())))
        })
    }

    /// Reads the module's bodies' locations from its `check` entry: a
    /// body the build decoded already from it, any other from its wire
    /// form's locations alone, never decoding its tables or columns
    /// ([`hd_tir::wire::read_locations`]), so a warm run's positions cost
    /// what a cold run's do. The entry's frame names (section 7) list the
    /// bodies' paths in its bodies' order (section 3).
    fn read(&self) -> Option<Module> {
        let sections = crate::split_sections(self.check.as_ref()?)?;
        let (rows, sites) = read_frames(sections.get(7)?)?;
        let order: Vec<&str> = rows.iter().map(|(path, ..)| path.as_str()).collect();
        let mut names: HashMap<String, (String, bool)> = HashMap::default();
        for (path, name, hidden) in &rows {
            names.insert(path.clone(), (name.clone(), *hidden));
        }
        let mut bodies = HashMap::default();
        let mut r = Reader::new(sections.get(3)?);
        for i in 0..r.count() {
            // The item's path hash, then the body.
            let hash = r.hash();
            let bytes = r.blob();
            let (path, mut body) = if let Some((path, body)) = self.input.bodies.get(&hash) {
                (path.clone(), body.clone())
            } else {
                // An entry with frame sites marks a derived instance, so the
                // tables are read only for fallback rows' local names.
                let marked = sites.as_ref().and_then(|s| s.get(i)).map(|s| s.0);
                let at = hd_tir::wire::read_locations(bytes, marked.is_none())?;
                let derived =
                    marked.unwrap_or(at.kind == Some(hd_tir::ir::BodyKind::DeriveInstance));
                (
                    (*order.get(i)?).to_owned(),
                    Body::from_locations(at, derived),
                )
            };
            if let (Some(lines), Some(sites)) = (
                &self.input.code_lines,
                sites.as_ref().and_then(|s| s.get(i)),
            ) {
                body.sites = sites
                    .1
                    .iter()
                    .map(|&(node, k, newlines)| {
                        let start = if k == RAW { 0 } else { *lines.get(k as usize)? };
                        Some((node, start.checked_add(newlines)?.checked_add(1)?))
                    })
                    .collect();
            }
            (body.name, body.hidden) = names.remove(&path)?;
            bodies.insert(path, body);
        }
        Some(Module {
            bodies,
            parse: OnceLock::new(),
            line_starts: OnceLock::new(),
        })
    }
}

/// Where a frame in syntax node `node` of a module stands, as a byte
/// offset in its text: a call at the end of its argument list, where it
/// panics after evaluating them, a trailing callback's call at the end of
/// its header, any other node at its start. `None` for no node.
pub fn site(parse: &Parse, text: &str, node: NodeIdx) -> Option<u32> {
    if node == NodeIdx::NONE || node.idx() >= parse.tree.len() {
        return None;
    }
    let src = Src {
        parse,
        text,
        file: FileId::from_raw(0),
    };
    let node = parse.tree.node(node);
    // A trailing callback has its own body positions; the invocation
    // is located at the end of its header, before that callback.
    let at = if node.kind() == hd_syntax::SyntaxKind::TrailingCallExpr {
        node.children().next().unwrap_or(node)
    } else {
        node
    };
    let span = src.span(at);
    Some(
        if matches!(
            node.kind(),
            hd_syntax::SyntaxKind::CallExpr | hd_syntax::SyntaxKind::TrailingCallExpr
        ) {
            span.hi.saturating_sub(1).max(span.lo)
        } else {
            span.lo
        },
    )
}

/// In a code line's stored frame site: the site lies before the first
/// code line, and its newlines count from the text's start.
const RAW: u32 = u32::MAX;

/// Where offset `at` of `text` stands as a `check` entry's frame site
/// keeps it: the code line it is on (by its index in `code_lines`, or
/// [`RAW`] before the first) and the newlines from that line's start to
/// `at`, none unless a multi-line string runs between. An edit of
/// comments and blank lines keeps the entry, whose key holds the code
/// lines' contents, and the skim's code line numbers place the site.
#[must_use]
pub fn code_line_of(code_lines: &[u32], text: &str, at: u32) -> (u32, u32) {
    let k = code_lines.partition_point(|&s| s <= at);
    let (line, from) = match k.checked_sub(1) {
        Some(k) => (u32::try_from(k).unwrap_or(RAW), code_lines[k]),
        None => (RAW, 0),
    };
    let between = text
        .get(from as usize..(at as usize).min(text.len()))
        .unwrap_or_default();
    let newlines = between.matches('\n').count();
    (line, u32::try_from(newlines).unwrap_or(u32::MAX))
}

/// The LF line starts of `text`: its start, and each byte after a `\n`,
/// as the lexer's line starts give them, without a parse.
#[must_use]
pub fn text_line_starts(text: &str) -> Vec<u32> {
    std::iter::once(0)
        .chain(
            text.bytes()
                .enumerate()
                .filter(|&(_, b)| b == b'\n')
                .filter_map(|(i, _)| u32::try_from(i + 1).ok()),
        )
        .collect()
}

pub fn path_of(paths: &PathTable, d: DefId) -> String {
    paths.display(PathId::from_raw(d.raw())).to_string()
}

/// Writes the frame names of a module's bodies, a section of its `check`
/// entry: per body, its stable path and the segments of its name in the
/// module (`["List", "remove"]`). An item's segment is its name, an
/// impl's is the name of the type it is for (`impl[T] List[T]` is
/// `List`, as `impl_type` names it from the impl's item), and a hidden segment,
/// such as a default argument's body or a block-local scope, is left out,
/// so its frames name their declaration. The names come from the items the
/// checker built, so no spelling of a path segment decides them. A body
/// of a `@track_caller` function (`hidden`) is marked: its frames are left
/// out of a panic's frame list (`fn.caller.frames`).
pub fn write_frame_names(
    w: &mut Writer,
    paths: &PathTable,
    items: &[DefId],
    impl_type: &dyn Fn(DefId) -> Option<String>,
    hidden: &dyn Fn(DefId) -> bool,
) {
    w.len_of(items);
    for &item in items {
        let mut segs = Vec::new();
        let mut p = PathId::from_raw(item.raw());
        while let Some(q) = p.get() {
            match paths.kind(q) {
                PathKind::Package | PathKind::Module => break,
                PathKind::Hidden | PathKind::Text => {}
                PathKind::Impl => {
                    segs.push(impl_type(DefId::from_raw(q.raw())).unwrap_or_default());
                }
                PathKind::Item | PathKind::Member | PathKind::Variant => {
                    segs.push(paths.segment(q).to_owned());
                }
            }
            p = paths.parent(q);
        }
        segs.reverse();
        w.str(&path_of(paths, item));
        w.len_of(&segs);
        for s in &segs {
            w.str(s);
        }
        w.u8(u8::from(hidden(item)));
    }
}

/// Each body's frame name row: its path, its name, and whether its frames
/// are left out.
pub type FrameRows = Vec<(String, String, bool)>;

/// Each body's frame sites, in the bodies' order: whether it is a derived
/// instance, whose nodes belong to its template's source and name no
/// site, and its syntax nodes, sorted, each with where its frame stands
/// ([`site`]) as [`code_line_of`] keeps it.
pub type FrameSites = Vec<(bool, Vec<(u32, u32, u32)>)>;

/// The rows [`write_frame_names`] wrote, in its order (the bodies' order
/// in the entry): each body's path, its name, and whether its frames are
/// left out; and the frame sites that follow them when the entry has them.
pub(crate) fn read_frames(bytes: &[u8]) -> Option<(FrameRows, Option<FrameSites>)> {
    let mut r = Reader::new(bytes);
    let mut rows = Vec::new();
    for _ in 0..r.count() {
        let path = r.str().to_owned();
        let segs: Vec<&str> = (0..r.count()).map(|_| r.str()).collect();
        let hidden = r.u8() != 0;
        rows.push((path, segs.join("."), hidden));
    }
    let sites = (!r.at_end()).then(|| {
        (0..r.count())
            .map(|_| {
                let derived = r.u8() != 0;
                let at = (0..r.count())
                    .map(|_| (r.u32(), r.u32(), r.u32()))
                    .collect();
                (derived, at)
            })
            .collect()
    });
    r.ok().then_some((rows, sites))
}

/// Writes each body's frame sites ([`FrameSites`]) after its frame names:
/// `site(node)` is where node `node`'s frame stands ([`code_line_of`]).
pub fn write_frame_sites(
    w: &mut Writer,
    bodies: &[hd_tir::Body],
    site: &dyn Fn(NodeIdx) -> Option<(u32, u32)>,
) {
    w.len_of(bodies);
    for b in bodies {
        let derived = b.kind == hd_tir::ir::BodyKind::DeriveInstance;
        w.u8(u8::from(derived));
        let mut nodes: Vec<u32> = if derived {
            Vec::new()
        } else {
            b.syn.iter().map(|n| n.raw()).collect()
        };
        nodes.sort_stable();
        nodes.dedup();
        let at: Vec<(u32, (u32, u32))> = nodes
            .into_iter()
            .filter_map(|n| Some((n, site(NodeIdx::from_raw(n))?)))
            .collect();
        w.len_of(&at);
        for (n, (line, newlines)) in at {
            w.u32(n);
            w.u32(line);
            w.u32(newlines);
        }
    }
}

/// The name an impl's type goes by in a frame, without generic arguments:
/// a declared type by its name, so `T?` as `Option`, a parameter by the name
/// the impl gives it (`generics`), and a tuple by its elements' names. Any
/// other type, such as a function, prints as a type does.
pub(crate) fn self_type_name(names: &Names<'_>, generics: &[Generic], ty: Ty) -> String {
    let pool = names.pool.types();
    match pool.get(ty) {
        TyData::Adt(AdtId::Declared(def), _) => names.display_name(def).to_owned(),
        TyData::Param(p) => generics.get(usize::from(p.index)).map_or_else(
            || show_ty_full(names, ty),
            |g| names.text(g.name).to_owned(),
        ),
        TyData::Adt(AdtId::Product { rest: false }, elems) => {
            let items = pool.list_items(elems);
            if items.is_empty() {
                return "void".to_owned();
            }
            let each: Vec<String> = items
                .iter()
                .map(|t| self_type_name(names, generics, *t))
                .collect();
            format!("({})", each.join(", "))
        }
        _ => show_ty_full(names, ty),
    }
}

/// A test case's name as an hd string literal writes it.
#[must_use]
pub fn quoted(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    out.push('"');
    for c in name.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}
