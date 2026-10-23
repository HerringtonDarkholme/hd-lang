//! Anchors to source lines (`crates/hd_mono/DESIGN.md` §13.8 "Positions", `crates/hd_wasm_linear/DESIGN.md`
//! §15.9). A linked module's `hd.lines` names a call by its anchor: an
//! item's stable path and a TIR instruction index in that item's body. The
//! anchor is resolved against the current sources only when a panic's
//! frames are printed (`cli.test.report.frame`): the module's `check`
//! entry gives the instruction's syntax node, and a parse of the file the
//! run read gives the node's line. So a comment edit above a panic moves
//! the line it reports, and nothing compiled or cached.
//!
//! A frame names its function as hd names it, not by its stable path: a
//! module's path as its declarations print (trait.typeid.name.*), then the
//! function, a method after its type's name with no generic arguments
//! (`std.collections.List.remove_at`), and a test case's body as
//! `test "NAME"`. The module carries its printed path, so the line table
//! keeps stable paths and a release build's `hd.names` names the same.

use hd_base::HashMap;
use std::sync::{Arc, OnceLock};

use hd_base::wire::{Reader, Writer};
use hd_base::{DefId, FileId, Hash128, NodeIdx, PathId};
use hd_intern::{PathKind, PathTable, ShardedInterner};
use hd_resolve::{Generic, Names, Src, show_ty};
use hd_syntax::{Parse, parse};
use hd_types::{InternPool, Ty, TyData};

/// What resolves anchors for one built program: each of its modules, by
/// module path, with the path its frames print; and its test cases'
/// registered names, by their bodies' stable paths.
pub struct Positions {
    modules: Vec<(String, String, Arc<ModuleSource>)>,
    tests: HashMap<String, String>,
}

/// The modules of the programs one command builds, each kept once: the
/// unit-test program, every integration test and every doc test read the
/// same std modules, which are kept and decoded once for all of them.
#[derive(Default)]
pub struct PositionPool {
    by_path: HashMap<String, Vec<Arc<ModuleSource>>>,
}

/// One module as a run saw it: its file, text and `check` entry. Nothing
/// is decoded or parsed until an anchor of the module is asked for, and
/// then once.
struct ModuleSource {
    file: String,
    text: Arc<str>,
    check: Option<Arc<[u8]>>,
    input: Arc<ModuleInput>,
    read: OnceLock<Option<Module>>,
}

/// Source data the build already read. Missing parses and bodies remain lazy:
/// a warm link need not decode TIR or parse modules solely to retain them.
#[derive(Default)]
pub(super) struct ModuleInput {
    pub(super) parse: Option<Arc<Parse>>,
    bodies: HashMap<Hash128, (String, Body)>,
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

/// One module, read: its bodies, by item path, and the parse of its text.
struct Module {
    bodies: HashMap<String, Body>,
    parse: Arc<Parse>,
    /// LF line starts, shared by every anchor lookup in this module.
    /// The lexer also records bare CR lines; positions have always counted LF.
    line_starts: Vec<u32>,
}

/// A body of a module: its syntax node per instruction, and its name in
/// the module (`List.remove_at`), from the module's frame-name record.
#[derive(Clone)]
struct Body {
    syn: Arc<[NodeIdx]>,
    name: String,
    /// Its operations whose `usize` type is a literal's default, by
    /// instruction, each with the name of the local it reads.
    fallback: Arc<[(u32, Option<String>)]>,
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

impl PositionPool {
    /// The pool's module of `path` with this file, text and entry, added
    /// when the pool has none.
    fn module(
        &mut self,
        path: &str,
        file: &str,
        text: &Arc<str>,
        check: Option<&Arc<[u8]>>,
        input: &Arc<ModuleInput>,
    ) -> Arc<ModuleSource> {
        let same = self.by_path.entry(path.to_owned()).or_default();
        if let Some(m) = same.iter().find(|m| {
            m.file == file && *m.text == **text && m.check.as_deref() == check.map(|c| &**c)
        }) {
            return Arc::clone(m);
        }
        let m = Arc::new(ModuleSource {
            file: file.to_owned(),
            text: Arc::clone(text),
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
                let source = pool.module(
                    path,
                    output.files.get(m)?,
                    output.texts.get(m)?,
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
    #[must_use]
    pub fn line(&self, item: &str, inst: u32) -> Option<(&str, u32)> {
        let (_, source) = self.module_of(item)?;
        let module = source.module()?;
        let node = *module.bodies.get(item)?.syn.get(inst as usize)?;
        if node == NodeIdx::NONE || node.idx() >= module.parse.tree.len() {
            return None;
        }
        let src = Src {
            parse: &module.parse,
            text: &source.text,
            file: FileId::from_raw(0),
        };
        let lo = src.span(module.parse.tree.node(node)).lo as usize;
        let before = source.text.get(..lo.min(source.text.len()))?;
        let line = u32::try_from(
            module
                .line_starts
                .partition_point(|&start| start as usize <= before.len()),
        )
        .ok()?;
        Some((source.file.as_str(), line))
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
        let module = source.module()?;
        if node == NodeIdx::NONE || node.idx() >= module.parse.tree.len() {
            return None;
        }
        let src = Src {
            parse: &module.parse,
            text: &source.text,
            file: FileId::from_raw(0),
        };
        let s = src.span(module.parse.tree.node(node));
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
    /// `shop.cart.total` or `std.collections.List.remove_at`. A body the
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

    fn read(&self) -> Option<Module> {
        let sections = crate::split_sections(self.check.as_ref()?)?;
        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let mut names = read_frame_names(sections.get(7)?)?;
        let mut bodies = HashMap::default();
        let mut r = Reader::new(sections.get(3)?);
        for _ in 0..r.count() {
            // The item's path hash, then the body.
            let hash = r.hash();
            let bytes = r.blob();
            let (path, mut body) = if let Some((path, body)) = self.input.bodies.get(&hash) {
                (path.clone(), body.clone())
            } else {
                let b = hd_tir::wire::read_body(bytes, &pool, &paths, &syms)?;
                (path_of(&paths, b.item), Body::from_tir(&b, &syms))
            };
            body.name = names.remove(&path)?;
            bodies.insert(path, body);
        }
        let parse = self
            .input
            .parse
            .clone()
            .unwrap_or_else(|| Arc::new(parse(self.text.as_bytes())));
        let line_starts = lf_line_starts(&parse, &self.text);
        Some(Module {
            bodies,
            parse,
            line_starts,
        })
    }
}

fn lf_line_starts(parse: &Parse, text: &str) -> Vec<u32> {
    parse
        .tokens
        .line_start
        .iter()
        .copied()
        .filter(|&start| start == 0 || text.as_bytes().get(start as usize - 1) == Some(&b'\n'))
        .collect()
}

fn path_of(paths: &PathTable, d: DefId) -> String {
    paths.display(PathId::from_raw(d.raw())).to_string()
}

/// Writes the frame names of a module's bodies, a section of its `check`
/// entry: per body, its stable path and the segments of its name in the
/// module (`["List", "remove_at"]`). An item's segment is its name, an
/// impl's is the name of the type it is for (`impl [ T ] List [ T ]` is
/// `List`, as `impl_type` names it from the impl's item), and a hidden segment,
/// such as a default argument's body or a block-local scope, is left out,
/// so its frames name their declaration. The names come from the items the
/// checker built, so no spelling of a path segment decides them.
pub(crate) fn write_frame_names(
    w: &mut Writer,
    paths: &PathTable,
    items: &[DefId],
    impl_type: &dyn Fn(DefId) -> Option<String>,
) {
    w.len_of(items);
    for &item in items {
        let mut segs = Vec::new();
        let mut p = PathId::from_raw(item.raw());
        while let Some(q) = p.get() {
            match paths.kind(q) {
                PathKind::Package | PathKind::Module => break,
                PathKind::Hidden => {}
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
    }
}

/// The frame names [`write_frame_names`] wrote, by body path.
fn read_frame_names(bytes: &[u8]) -> Option<HashMap<String, String>> {
    let mut r = Reader::new(bytes);
    let mut names = HashMap::default();
    for _ in 0..r.count() {
        let path = r.str().to_owned();
        let segs: Vec<&str> = (0..r.count()).map(|_| r.str()).collect();
        names.insert(path, segs.join("."));
    }
    r.ok().then_some(names)
}

/// The name an impl's type goes by in a frame, without generic arguments:
/// a declared type by its name, `T?` as `Option`, a parameter by the name
/// the impl gives it (`generics`), and a tuple by its elements' names. Any
/// other type, such as a function, prints as a type does.
pub(crate) fn self_type_name(names: &Names<'_>, generics: &[Generic], ty: Ty) -> String {
    let pool = names.pool.types();
    match pool.get(ty) {
        TyData::Adt { def, .. } => names.display_name(def).to_owned(),
        TyData::Option(_) => "Option".to_owned(),
        TyData::Param(p) => generics
            .get(usize::from(p.index))
            .map_or_else(|| show_ty(names, ty), |g| names.text(g.name).to_owned()),
        TyData::Tuple { elems, rest: None } => {
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
        _ => show_ty(names, ty),
    }
}

/// A test case's name as an hd string literal writes it.
fn quoted(name: &str) -> String {
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, OnceLock};

    use hd_base::wire::Writer;
    use hd_base::{DefId, FileId, HashMap, NodeIdx, PathId};
    use hd_intern::{PathKind, PathTable};
    use hd_resolve::Src;
    use hd_syntax::parse;

    use super::{
        Body, Module, ModuleInput, ModuleSource, Positions, lf_line_starts, quoted,
        read_frame_names, write_frame_names,
    };

    #[test]
    fn anchor_lines_preserve_lf_positions_for_all_syntax_nodes() {
        for text in [
            "",
            "\n\n",
            "fn f() -> i32:\n    +1",
            "# λ\r\n\r\nfn f() -> string:\r\n    \"é\"\r\n",
            "# first\r# second\r\nfn f() -> i32:\r    +1\n",
            "# before\n\nfn f() -> string:\n    \"first\\nsecond\"\n",
        ] {
            let parse = parse(text.as_bytes());
            let syn: Vec<_> = (0..parse.tree.len())
                .map(|n| NodeIdx::from_raw(u32::try_from(n).unwrap()))
                .chain([NodeIdx::NONE, NodeIdx::from_raw(u32::MAX - 1)])
                .collect();
            let src = Src {
                parse: &parse,
                text,
                file: FileId::from_raw(0),
            };
            let expected: Vec<_> = syn
                .iter()
                .map(|&node| {
                    if node == NodeIdx::NONE || node.idx() >= parse.tree.len() {
                        return None;
                    }
                    let lo = src.span(parse.tree.node(node)).lo as usize;
                    Some((
                        "source.hd",
                        u32::try_from(text[..lo].matches('\n').count()).unwrap() + 1,
                    ))
                })
                .collect();
            let line_starts = lf_line_starts(&parse, text);
            let positions = Positions {
                modules: vec![(
                    "pkg".into(),
                    "pkg".into(),
                    Arc::new(ModuleSource {
                        file: "source.hd".into(),
                        text: text.into(),
                        check: None,
                        input: Arc::new(ModuleInput::default()),
                        read: OnceLock::from(Some(Module {
                            bodies: [(
                                "pkg/f".into(),
                                Body {
                                    syn: syn.into(),
                                    name: "f".into(),
                                    fallback: Arc::from([]),
                                },
                            )]
                            .into_iter()
                            .collect(),
                            parse: Arc::new(parse),
                            line_starts,
                        })),
                    }),
                )],
                tests: HashMap::default(),
            };
            for (inst, expected) in expected.into_iter().enumerate() {
                assert_eq!(
                    positions.line("pkg/f", u32::try_from(inst).unwrap()),
                    expected,
                    "{text:?}"
                );
            }
            assert_eq!(positions.line("pkg/f", u32::MAX), None);
            assert_eq!(positions.line("pkg/missing", 0), None);
        }
    }

    #[test]
    fn retained_sources_match_wire_positions_on_cold_warm_and_comment_edits() {
        use crate::{Executor, Goal, Host, NoClock, PositionPool, build};
        use hd_cache::MemoryStore;
        use hd_project::MemorySources;
        use hd_sched::SerialOrder;

        let original = "fn f() -> usize:\n    let size = 0\n    size - 1\n\npub fn main() -> void $ Console:\n    println(f())\n";
        let edited = format!("# λ: current source lines\n\n{original}");
        let store = MemoryStore::default();
        let mut pool = PositionPool::default();
        for (round, text) in [original, original, edited.as_str()]
            .into_iter()
            .enumerate()
        {
            let mut sources = MemorySources::default();
            sources.insert("main.hd", text);
            let host = Host {
                render_tir: &[],
                sources: &sources,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let mut output = build(
                &host,
                "app",
                &Goal::Program {
                    entry: "main".into(),
                },
            );
            assert!(!output.diags.has_errors(), "{}", output.render());
            let reused = output.positions(&mut pool);
            let missing: Vec<_> = (0..output.modules.len())
                .map(|_| Arc::new(ModuleInput::default()))
                .collect();
            let retained = std::mem::replace(&mut output.position_inputs, missing);
            let wire = output.positions(&mut PositionPool::default());
            output.position_inputs = retained;
            let table = hd_run::frames::LineTable::parse(output.wasm.as_ref().expect("Wasm"))
                .expect("line table");
            let mut anchors = 0;
            for (_, item, insts) in table.functions() {
                assert_eq!(reused.name(&item), wire.name(&item));
                for inst in insts {
                    assert_eq!(reused.line(&item, inst), wire.line(&item, inst));
                    assert_eq!(reused.span(&item, inst), wire.span(&item, inst));
                    assert_eq!(reused.fallback(&item, inst), wire.fallback(&item, inst));
                    anchors += 1;
                }
            }
            assert!(anchors > 10);
            if round == 0 {
                assert!(output.position_inputs.iter().any(|i| !i.bodies.is_empty()));
            }
            for (m, input) in output.position_inputs.iter().enumerate() {
                if let Some(parse) = &input.parse
                    && let Some((_, _, source)) = reused
                        .modules
                        .iter()
                        .find(|(p, _, _)| p == &output.modules[m])
                    && let Some(Some(read)) = source.read.get()
                {
                    assert!(Arc::ptr_eq(parse, &read.parse));
                }
            }
        }
    }

    /// A body's frame name comes from the item the impl segment names, not
    /// from how the segment spells its head: these segments are not impl
    /// heads at all, and the names still follow the recorded types.
    #[test]
    fn a_frame_name_comes_from_the_record_not_the_segment_spelling() {
        let paths = PathTable::new();
        let module = paths.item("pkg", "m", "x");
        let m = PathId::from_raw(module.raw())
            .get()
            .map(|p| paths.parent(p));
        let m = m.expect("module");
        let def = |parent: PathId, kind: PathKind, seg: &str| {
            let id = paths.intern(parent, kind, seg);
            (id, DefId::from_raw(id.raw()))
        };
        let (generic, generic_impl) = def(m, PathKind::Impl, "??? generic spelling ???");
        let (_, generic_method) = def(generic, PathKind::Member, "remove_at");
        let (derived, derived_impl) = def(m, PathKind::Impl, "something else #2");
        let (_, derived_method) = def(derived, PathKind::Member, "eq");
        let (inherent, inherent_impl) = def(m, PathKind::Impl, "impl Cart");
        let (_, inherent_method) = def(inherent, PathKind::Member, "total");
        let (_, free) = def(m, PathKind::Item, "run");
        let record = |d: DefId| -> Option<String> {
            [
                (generic_impl, "List"),
                (derived_impl, "Cart"),
                (inherent_impl, "Cart"),
            ]
            .iter()
            .find(|(i, _)| *i == d)
            .map(|(_, t)| (*t).to_owned())
        };
        let items = [generic_method, derived_method, inherent_method, free];
        let mut w = Writer::default();
        write_frame_names(&mut w, &paths, &items, &record);
        let names = read_frame_names(&w.bytes).expect("record");
        let of = |d: DefId| names[&super::path_of(&paths, d)].clone();
        assert_eq!(of(generic_method), "List.remove_at");
        assert_eq!(of(derived_method), "Cart.eq");
        assert_eq!(of(inherent_method), "Cart.total");
        assert_eq!(of(free), "run");
    }

    #[test]
    fn a_test_name_is_quoted_as_a_literal() {
        assert_eq!(quoted("sums prices"), "\"sums prices\"");
        assert_eq!(quoted("a \"b\" \\"), "\"a \\\"b\\\" \\\\\"");
    }
}
