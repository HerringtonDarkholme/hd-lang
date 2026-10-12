//! Anchors to source lines (codegen.md §13.8 "Positions", wasm-layout.md
//! §15.9). A linked module's `hd.lines` names a call by its anchor: an
//! item's stable path and a TIR instruction index in that item's body. The
//! anchor is resolved against the current sources only when a panic's
//! frames are printed (`cli.test.report.frame`): the module's `check`
//! entry gives the instruction's syntax node, and a parse of the file the
//! run read gives the node's line. So a comment edit above a panic moves
//! the line it reports, and nothing compiled or cached.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use hd_base::wire::Reader;
use hd_base::{DefId, FileId, NodeIdx, PathId};
use hd_intern::{PathTable, ShardedInterner};
use hd_resolve::Src;
use hd_syntax::{Parse, parse};
use hd_types::InternPool;

/// What resolves anchors for one built program: each of its modules, by
/// module path.
pub struct Positions {
    modules: Vec<(String, Arc<ModuleSource>)>,
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

/// One module, read: each body's syntax node per instruction, by the
/// body's item path, and the parse of its text.
struct Module {
    syn: HashMap<String, Vec<NodeIdx>>,
    parse: Parse,
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
                Some((path.clone(), source))
            })
            .collect();
        Self { modules }
    }

    /// The package-relative file and 1-based line of instruction `inst` in
    /// the body of the item with stable path `item` (`pkg/mod/f`, the path
    /// a function's name holds), or `None` when the run has no such body.
    #[must_use]
    pub fn line(&self, item: &str, inst: u32) -> Option<(&str, u32)> {
        let source = self.module_of(item)?;
        let module = source.read.get_or_init(|| source.read()).as_ref()?;
        let node = *module.syn.get(item)?.get(inst as usize)?;
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

    /// The module that declares an item, from its stable path: the package,
    /// then the module's segments, then the item (`PathTable::print`). A
    /// package's root module has no segment of its own.
    fn module_of(&self, item: &str) -> Option<&ModuleSource> {
        let (package, rest) = item.split_once('/')?;
        let find = |path: &str| {
            self.modules
                .iter()
                .find(|(p, _)| p == path)
                .map(|(_, m)| &**m)
        };
        rest.split_once('/')
            .and_then(|(module, _)| find(&format!("{package}.{module}")))
            .or_else(|| find(package))
    }
}

impl ModuleSource {
    fn read(&self) -> Option<Module> {
        let sections = crate::split_sections(self.check.as_ref()?)?;
        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let mut syn = HashMap::new();
        let mut r = Reader::new(sections.get(3)?);
        for _ in 0..r.count() {
            let b = hd_tir::wire::read_body(r.blob(), &pool, &paths, &syms)?;
            syn.insert(path_of(&paths, b.item), b.syn);
        }
        Some(Module {
            syn,
            parse: parse(self.text.as_bytes()),
        })
    }
}

fn path_of(paths: &PathTable, d: DefId) -> String {
    paths.display(PathId::from_raw(d.raw())).to_string()
}
