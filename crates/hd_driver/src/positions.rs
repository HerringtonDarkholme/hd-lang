//! Anchors to source lines (codegen.md §13.8 "Positions", wasm-layout.md
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

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use hd_base::wire::Reader;
use hd_base::{DefId, FileId, NodeIdx, PathId};
use hd_intern::{PathKind, PathTable, ShardedInterner};
use hd_resolve::Src;
use hd_syntax::{Parse, parse};
use hd_types::InternPool;

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
    read: OnceLock<Option<Module>>,
}

/// One module, read: its bodies, by item path, and the parse of its text.
struct Module {
    bodies: HashMap<String, Body>,
    parse: Parse,
}

/// A body of a module: its syntax node per instruction, and its name in
/// the module (`List.remove_at`).
struct Body {
    syn: Vec<NodeIdx>,
    name: String,
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
            read: OnceLock::new(),
        });
        same.push(Arc::clone(&m));
        m
    }
}

impl Positions {
    pub(crate) fn new(
        pool: &mut PositionPool,
        modules: &[String],
        files: &[String],
        texts: &[Arc<str>],
        checks: &[Option<Arc<[u8]>>],
        printed: &[String],
        cases: &[(String, String)],
    ) -> Self {
        let modules = modules
            .iter()
            .enumerate()
            .filter_map(|(m, path)| {
                let source = pool.module(
                    path,
                    files.get(m)?,
                    texts.get(m)?,
                    checks.get(m).and_then(Option::as_ref),
                );
                Some((path.clone(), printed.get(m)?.clone(), source))
            })
            .collect();
        Self {
            modules,
            tests: cases.iter().cloned().collect(),
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
        if node == NodeIdx::NONE {
            return None;
        }
        let src = Src {
            parse: &module.parse,
            text: &source.text,
            file: FileId::from_raw(0),
        };
        let lo = src.span(module.parse.tree.node(node)).lo as usize;
        let before = source.text.get(..lo.min(source.text.len()))?;
        let line = u32::try_from(before.matches('\n').count() + 1).ok()?;
        Some((source.file.as_str(), line))
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
        let mut bodies = HashMap::new();
        let mut r = Reader::new(sections.get(3)?);
        for _ in 0..r.count() {
            let b = hd_tir::wire::read_body(r.blob(), &pool, &paths, &syms)?;
            let body = Body {
                syn: b.syn,
                name: name_in_module(&paths, b.item),
            };
            bodies.insert(path_of(&paths, b.item), body);
        }
        Some(Module {
            bodies,
            parse: parse(self.text.as_bytes()),
        })
    }
}

fn path_of(paths: &PathTable, d: DefId) -> String {
    paths.display(PathId::from_raw(d.raw())).to_string()
}

/// An item's name in its module: its segments below the module, joined by
/// `.`. An impl's segment is its type's name (`impl [ T ] List [ T ]` is
/// `List`), and a hidden segment, such as a default argument's body or a
/// block-local scope, is left out, so its frames name their declaration.
fn name_in_module(paths: &PathTable, d: DefId) -> String {
    let mut segs = Vec::new();
    let mut p = PathId::from_raw(d.raw());
    while let Some(q) = p.get() {
        match paths.kind(q) {
            PathKind::Package | PathKind::Module => break,
            PathKind::Hidden => {}
            PathKind::Impl => segs.push(impl_type(paths.segment(q))),
            PathKind::Item | PathKind::Member | PathKind::Variant => {
                segs.push(paths.segment(q).to_owned());
            }
        }
        p = paths.parent(q);
    }
    segs.reverse();
    segs.join(".")
}

/// The type an impl segment implements for, by its name without generic
/// arguments. A segment is the impl's head as tokens joined by spaces
/// (`impl [ T < Eq ] Eq for List [ T ]`, `derive Eq for Cart`), with ` #N`
/// after a repeated head: the type follows the last `for` outside
/// brackets, or else the keyword and the impl's generic parameters, and
/// ends at a delegation's `by`. A named type prints its last path segment,
/// `T?` prints `Option`, and any other type, such as a tuple, prints as
/// written.
fn impl_type(seg: &str) -> String {
    let mut toks: Vec<&str> = seg.split(' ').collect();
    if toks.len() > 1 && toks.last().is_some_and(|t| t.starts_with('#')) {
        toks.pop();
    }
    // Each token's bracket depth before it.
    let mut depth = 0i32;
    let depths: Vec<i32> = toks
        .iter()
        .map(|t| {
            let d = depth;
            match *t {
                "[" | "(" => depth += 1,
                "]" | ")" => depth -= 1,
                _ => {}
            }
            d
        })
        .collect();
    let top = |i: usize, word: &str| depths[i] == 0 && toks[i] == word;
    let start = if let Some(i) = (0..toks.len()).rev().find(|&i| top(i, "for")) {
        i + 1
    } else {
        // The keyword, then the impl's generic parameters, if any.
        let mut i = 1;
        if toks.get(i) == Some(&"[") {
            while i < toks.len() && !(depths[i] == 1 && toks[i] == "]") {
                i += 1;
            }
            i += 1;
        }
        i
    };
    let end = (start..toks.len())
        .find(|&i| top(i, "by"))
        .unwrap_or(toks.len());
    let ty = toks.get(start..end).unwrap_or_default();
    // `T?` is an `Option`, whose arguments are left out like any other's.
    if end > start && toks[end - 1] == "?" && depths[end - 1] == 0 {
        return "Option".to_owned();
    }
    // A named type: a path, then its arguments, if any.
    let path: Vec<&str> = ty.iter().take_while(|t| **t != "[").copied().collect();
    let ident = |t: &str| t.chars().all(|c| c.is_alphanumeric() || c == '_');
    let named = path
        .iter()
        .enumerate()
        .all(|(i, t)| if i % 2 == 0 { ident(t) } else { *t == "." });
    if named && path.len() % 2 == 1 {
        return path.last().copied().unwrap_or_default().to_owned();
    }
    ty.concat().replace(',', ", ").replace("->", " -> ")
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
    use super::{impl_type, quoted};

    #[test]
    fn an_impl_names_the_type_it_implements_for() {
        for (seg, ty) in [
            ("impl [ T ] List [ T ]", "List"),
            ("impl Cart", "Cart"),
            ("impl [ T < Eq ] Eq for List [ T ]", "List"),
            ("impl Eq for Parsed #2", "Parsed"),
            ("impl Describe for Service by Logger", "Service"),
            ("impl [ T < Eq ] Eq for T ?", "Option"),
            ("impl Show for model . User", "User"),
            ("derive Eq for Cart", "Cart"),
            ("error Display for FsError", "FsError"),
            ("impl ( i32 , i32 )", "(i32, i32)"),
            ("impl Display for fn ( ) -> i32", "fn() -> i32"),
        ] {
            assert_eq!(impl_type(seg), ty, "{seg}");
        }
    }

    #[test]
    fn a_test_name_is_quoted_as_a_literal() {
        assert_eq!(quoted("sums prices"), "\"sums prices\"");
        assert_eq!(quoted("a \"b\" \\"), "\"a \\\"b\\\" \\\\\"");
    }
}
