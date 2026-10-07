//! `hd_driver`: one run over a source set, through every stage, as tasks of
//! the serial scheduler, with every stage boundary through the cache. It is
//! the only crate that sees every stage; `source` and `node` hold its file
//! system and process access.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hd_base::Hash128;
use hd_cache::{FileApi, check_key, code_key, iface_key, prog_key, toolchain_key};
use hd_check::{Cst, Scope, check_module_bodies, lower_headers, module_scope};
use hd_iface::{
    FolderIface, HeaderItem, KeyHasher, Reader, build_iface, decode_iface, decode_item, encode_item,
    folder_of_module, item_path, put_hash, put_str, put_u32,
};
use hd_mono::{Instance, collect, instance_key};
use hd_sched::{ExtTask, TaskGraph, TaskId, TaskKind};
use hd_syntax::skim;
use hd_syntax::subset::{SubsetParse, parse_subset};
use hd_tir::world::{DefId, World, load_items};
use hd_tir::{Tables, TirBody, read_body, read_tables, tir_hash, write_body, write_tables};
use hd_wasm::{Code, emit, link};

pub mod bench;
pub mod node;
pub mod source;

pub use hd_cache::MemStore;
pub use hd_check::A1Rule;
pub use source::{Program, SourceFile, load_program, module_path};

const COMPILER: &str = "hd 0";
const TARGET: &str = "wasm32-gc";
const PIPELINE: &str = "dev";
const ROLE: &str = "lib";

#[derive(Default, Debug, Clone)]
pub struct Counters {
    pub tasks: BTreeMap<&'static str, usize>,
    pub hits: BTreeMap<&'static str, usize>,
    pub misses: BTreeMap<&'static str, usize>,
    pub modules_checked: Vec<String>,
    pub ifaces_built: Vec<String>,
    pub parsed: Vec<String>,
    /// Modules whose TIR was decoded from their `check` entry.
    pub tir_decoded: Vec<String>,
    pub emitted: usize,
    pub stage_time: BTreeMap<&'static str, Duration>,
    pub deep_hashes: BTreeMap<String, Hash128>,
    pub check_keys: BTreeMap<String, Hash128>,
}

impl Counters {
    #[must_use]
    pub fn hit(&self, kind: &str) -> usize {
        self.hits.get(kind).copied().unwrap_or(0)
    }
    #[must_use]
    pub fn miss(&self, kind: &str) -> usize {
        self.misses.get(kind).copied().unwrap_or(0)
    }
    #[must_use]
    pub fn ran(&self, task: &str) -> usize {
        self.tasks.get(task).copied().unwrap_or(0)
    }
}

/// Options that change what a stage writes; each is part of the toolchain key.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub a1_rule: A1Rule,
}

pub struct RunResult {
    pub wasm: Option<Vec<u8>>,
    pub diagnostics: Vec<String>,
    pub counters: Counters,
}

struct FileSlot {
    module: String,
    folder: String,
    source_hash: Hash128,
    api_text_hash: Hash128,
    uses: Vec<String>,
    parse: Option<SubsetParse>,
}

struct Prep {
    scope: Scope,
    headers: Vec<HeaderItem>,
    errors: Vec<String>,
    bodies: Vec<(String, hd_check::Checked)>,
}

/// A module's `check` entry, read only as far as `prog_key` needs: the rest
/// (private headers and TIR) is decoded at `Collect`, and only on a miss.
struct PendingEntry {
    bytes: Arc<[u8]>,
    rest: usize,
}

struct Run<'s> {
    store: &'s mut MemStore,
    opts: Options,
    toolchain: Hash128,
    package: Hash128,
    c: Counters,
    w: World,
    files: Vec<FileSlot>,
    sources: &'s [SourceFile],
    folders: Vec<String>,
    folder_uses: HashMap<String, BTreeSet<String>>,
    ifaces: HashMap<String, FolderIface>,
    iface_tasks: HashMap<String, TaskId>,
    prep: HashMap<u32, Prep>,
    pending: BTreeMap<String, PendingEntry>,
    tir: HashMap<DefId, TirBody>,
    tir_hashes: HashMap<DefId, Hash128>,
    tir_content: BTreeMap<String, Hash128>,
    diagnostics: Vec<String>,
    package_result: Option<TaskId>,
    entry: String,
    instances: Vec<(Hash128, Instance)>,
    callee_reps: HashMap<Hash128, Hash128>,
    codes: Vec<Option<(Hash128, Code)>>,
    data_types: Vec<String>,
    imports: Vec<u32>,
    prog_key: Hash128,
    wasm: Option<Vec<u8>>,
}

/// One run with default options.
pub fn run(store: &mut MemStore, sources: &[SourceFile], entry_module: &str) -> RunResult {
    run_with(store, sources, entry_module, Options::default())
}

/// One run: a fresh `World` (fresh run IDs) over a shared store.
pub fn run_with(
    store: &mut MemStore,
    sources: &[SourceFile],
    entry_module: &str,
    opts: Options,
) -> RunResult {
    let mut r = Run {
        store,
        opts,
        toolchain: toolchain_key(COMPILER, TARGET, &format!("{:?}", opts.a1_rule)),
        package: KeyHasher::new("root").str("pkg").finish(),
        c: Counters::default(),
        w: World::default(),
        files: Vec::new(),
        sources,
        folders: Vec::new(),
        folder_uses: HashMap::new(),
        ifaces: HashMap::new(),
        iface_tasks: HashMap::new(),
        prep: HashMap::new(),
        pending: BTreeMap::new(),
        tir: HashMap::new(),
        tir_hashes: HashMap::new(),
        tir_content: BTreeMap::new(),
        diagnostics: Vec::new(),
        package_result: None,
        entry: entry_module.to_owned(),
        instances: Vec::new(),
        callee_reps: HashMap::new(),
        codes: Vec::new(),
        data_types: Vec::new(),
        imports: Vec::new(),
        prog_key: Hash128(0),
        wasm: None,
    };
    let mut g = TaskGraph::default();
    let mut skims = Vec::new();
    for (i, f) in sources.iter().enumerate() {
        let m = module_path(&f.path);
        r.files.push(FileSlot {
            folder: folder_of_module(&m).to_owned(),
            module: m,
            source_hash: Hash128(0),
            api_text_hash: Hash128(0),
            uses: Vec::new(),
            parse: None,
        });
        skims.push(g.add(TaskKind::Skim(u32::try_from(i).expect("f")), &[]));
    }
    g.add(TaskKind::FolderGraph, &skims);
    g.run(&mut |id, kind, g| {
        let t0 = Instant::now();
        *r.c.tasks.entry(kind.name()).or_default() += 1;
        r.exec(id, kind, g);
        *r.c.stage_time.entry(kind.name()).or_default() += t0.elapsed();
    });
    RunResult { wasm: r.wasm, diagnostics: r.diagnostics, counters: r.c }
}

/// Depth-first visit for the folder order; a revisit in progress is a
/// `folder-cycle`.
fn visit(
    f: &str,
    uses: &HashMap<String, BTreeSet<String>>,
    order: &mut Vec<String>,
    visiting: &mut BTreeSet<String>,
    diags: &mut Vec<String>,
) {
    if order.iter().any(|o| o == f) {
        return;
    }
    if !visiting.insert(f.to_owned()) {
        diags.push(format!("folder-cycle at {f}"));
        return;
    }
    for u in uses.get(f).into_iter().flatten() {
        visit(u, uses, order, visiting, diags);
    }
    order.push(f.to_owned());
}

impl Run<'_> {
    fn exec(&mut self, id: TaskId, kind: TaskKind, g: &mut TaskGraph) {
        match kind {
            TaskKind::Skim(f) => self.skim(f as usize),
            TaskKind::FolderGraph => self.folder_graph(g),
            TaskKind::FolderIface(f) => self.folder_iface(f as usize),
            TaskKind::ModulePrep(m) => self.module_prep(id, m, g),
            TaskKind::Body(m) => self.body(m),
            TaskKind::ModuleFinish(m) => self.module_finish(m),
            TaskKind::PackageResult => self.package_result(g),
            TaskKind::Ext(ExtTask::Collect) => self.collect(g),
            TaskKind::Ext(ExtTask::Emit(i)) => self.emit(i as usize),
            TaskKind::Ext(ExtTask::Link) => self.link(),
            TaskKind::Parse(_)
            | TaskKind::HeaderCheck(_)
            | TaskKind::TestOverlay(_)
            | TaskKind::Coherence
            | TaskKind::InitOrder(_)
            | TaskKind::Ext(ExtTask::Precompile | ExtTask::RunCase(_)) => {}
        }
    }

    fn lookup(&mut self, kind: &'static str, key: Hash128) -> Option<Arc<[u8]>> {
        let v = self.store.get(kind, key);
        *if v.is_some() { self.c.hits.entry(kind) } else { self.c.misses.entry(kind) }.or_default() += 1;
        v
    }

    fn skim(&mut self, f: usize) {
        let src = &self.sources[f].text;
        let sk = skim(src.as_bytes());
        let slot = &mut self.files[f];
        slot.source_hash = sk.source_hash;
        slot.api_text_hash = sk.api_text_hash;
        slot.uses = source::use_paths(src, &sk);
    }

    /// Lazy full parse ("full parse if needed", M1): only on a miss, as a
    /// `Parse` task the missing task creates (walking skeleton, SK-4).
    fn parse(&mut self, f: usize) {
        if self.files[f].parse.is_none() {
            let t0 = Instant::now();
            let p = parse_subset(&self.sources[f].text);
            for e in &p.errors {
                self.diagnostics.push(format!("{}: parse: {e}", self.sources[f].path));
            }
            self.files[f].parse = Some(p);
            *self.c.tasks.entry("Parse").or_default() += 1;
            self.c.parsed.push(self.files[f].module.clone());
            *self.c.stage_time.entry("Parse").or_default() += t0.elapsed();
        }
    }

    fn folder_graph(&mut self, g: &mut TaskGraph) {
        let mut folders = BTreeSet::new();
        for f in &self.files {
            folders.insert(f.folder.clone());
            let e = self.folder_uses.entry(f.folder.clone()).or_default();
            for u in &f.uses {
                let uf = folder_of_module(u).to_owned();
                if uf != f.folder {
                    e.insert(uf);
                }
            }
        }
        // Topological order (the graph is acyclic in the subset; a cycle
        // would be `folder-cycle`).
        let mut order: Vec<String> = Vec::new();
        let mut visiting = BTreeSet::new();
        for f in &folders {
            visit(f, &self.folder_uses, &mut order, &mut visiting, &mut self.diagnostics);
        }
        self.folders = order.clone();
        for (i, f) in order.iter().enumerate() {
            let deps: Vec<TaskId> = self.folder_uses[f].iter().filter_map(|u| self.iface_tasks.get(u).copied()).collect();
            let t = g.add(TaskKind::FolderIface(u32::try_from(i).expect("f")), &deps);
            self.iface_tasks.insert(f.clone(), t);
        }
        let mut preps = Vec::new();
        for (m, f) in self.files.iter().enumerate() {
            let deps: Vec<TaskId> =
                self.closure(&f.folder).iter().map(|c| self.iface_tasks[c]).collect();
            preps.push(g.add(TaskKind::ModulePrep(u32::try_from(m).expect("m")), &deps));
        }
        let pr = g.add(TaskKind::PackageResult, &preps);
        self.package_result = Some(pr);
    }

    /// `closure(F)`: F plus every folder reachable through use edges.
    fn closure(&self, folder: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut stack = vec![folder.to_owned()];
        while let Some(f) = stack.pop() {
            if out.insert(f.clone()) {
                stack.extend(self.folder_uses.get(&f).into_iter().flatten().cloned());
            }
        }
        out
    }

    fn folder_iface(&mut self, fi: usize) {
        let folder = self.folders[fi].clone();
        let mut files: Vec<usize> =
            (0..self.files.len()).filter(|&i| self.files[i].folder == folder).collect();
        files.sort_by(|a, b| self.files[*a].module.cmp(&self.files[*b].module));
        let mut reach = self.closure(&folder);
        reach.remove(&folder);
        let key = {
            let apis: Vec<FileApi<'_>> = files
                .iter()
                .map(|&f| FileApi {
                    module: &self.files[f].module,
                    role: ROLE,
                    api_text_hash: self.files[f].api_text_hash,
                })
                .collect();
            let reach: Vec<(&str, Hash128)> =
                reach.iter().map(|d| (d.as_str(), self.ifaces[d].deep_hash)).collect();
            iface_key(self.toolchain, self.package, &folder, &apis, &reach)
        };
        let iface = if let Some(blob) = self.lookup("iface", key) {
            decode_iface(&blob)
        } else {
            let mut items = Vec::new();
            for &f in &files {
                self.parse(f);
                let module = self.files[f].module.clone();
                let src = &self.sources[f].text;
                let p = self.files[f].parse.as_ref().expect("parsed");
                let cst = Cst { src, p };
                let scope = module_scope(&cst, &module, &self.ifaces);
                let (hs, errs) = lower_headers(&cst, &module, &scope);
                self.diagnostics.extend(scope.errors.iter().cloned());
                self.diagnostics.extend(errs);
                items.extend(hs.into_iter().filter(|h| h.public));
            }
            let iface = build_iface(&folder, &items, &self.ifaces);
            self.store.put("iface", key, iface.blob.clone());
            self.c.ifaces_built.push(folder.clone());
            iface
        };
        load_items(&mut self.w, &iface.items, Some(&iface.item_hashes));
        self.c.deep_hashes.insert(folder.clone(), iface.deep_hash);
        self.ifaces.insert(folder, iface);
    }

    fn check_key(&self, m: usize) -> Hash128 {
        let f = &self.files[m];
        let closure = self.closure(&f.folder);
        let closure: Vec<(&str, Hash128)> =
            closure.iter().map(|c| (c.as_str(), self.ifaces[c].deep_hash)).collect();
        check_key(self.toolchain, self.package, &f.module, ROLE, f.source_hash, &closure)
    }

    /// `ModulePrep(m)`: the `check` key lookup is its first step; only on a
    /// miss does it parse and create `Body(m)` and `ModuleFinish(m)`
    /// (scheduler.md §6.1; walking skeleton, SK-4).
    fn module_prep(&mut self, id: TaskId, m: u32, g: &mut TaskGraph) {
        let mi = m as usize;
        let key = self.check_key(mi);
        self.c.check_keys.insert(self.files[mi].module.clone(), key);
        if let Some(entry) = self.lookup("check", key) {
            self.read_check_entry(mi, entry);
            return;
        }
        self.parse(mi);
        let module = self.files[mi].module.clone();
        let src = &self.sources[mi].text;
        let p = self.files[mi].parse.as_ref().expect("parsed");
        let cst = Cst { src, p };
        let scope = module_scope(&cst, &module, &self.ifaces);
        let (headers, errs) = lower_headers(&cst, &module, &scope);
        let mut errors = scope.errors.clone();
        errors.extend(errs);
        load_items(&mut self.w, &headers, None);
        self.prep.insert(m, Prep { scope, headers, errors, bodies: Vec::new() });
        let body = g.add(TaskKind::Body(m), &[id]);
        let finish = g.add(TaskKind::ModuleFinish(m), &[body]);
        g.edge(finish, self.package_result.expect("package result"));
    }

    fn body(&mut self, m: u32) {
        let mi = m as usize;
        let module = self.files[mi].module.clone();
        let src = &self.sources[mi].text;
        let p = self.files[mi].parse.as_ref().expect("parsed");
        let cst = Cst { src, p };
        let prep = self.prep.get_mut(&m).expect("prep");
        prep.bodies =
            check_module_bodies(&mut self.w, &cst, &module, &prep.scope, &prep.headers, self.opts.a1_rule);
    }

    /// Writes the `check` entry. Sections, in order: diagnostics; the meta
    /// section (the module's TIR content hash, which `prog_key` reads);
    /// headers (private signatures and private data layouts, by stable path:
    /// walking skeleton, SK-1); TIR, one record per body.
    fn module_finish(&mut self, m: u32) {
        let mi = m as usize;
        let prep = self.prep.remove(&m).expect("prep");
        let module = self.files[mi].module.clone();
        self.c.modules_checked.push(module.clone());
        let mut errors = prep.errors.clone();
        for (path, ck) in &prep.bodies {
            for e in &ck.errors {
                errors.push(format!("{module}: {path}: {e}"));
            }
        }
        let has_tir = errors.is_empty();
        let mut tir = Vec::new();
        let mut content = KeyHasher::new("tir-content");
        let nbodies = if has_tir { prep.bodies.len() } else { 0 };
        put_u32(&mut tir, u32::try_from(nbodies).expect("n"));
        for (path, ck) in prep.bodies.iter().take(nbodies) {
            let mut tables = Tables::default();
            let mut bytes = Vec::new();
            let mut spans = Vec::new();
            write_body(&self.w, &mut tables, &ck.body, &mut bytes, &mut spans);
            let h = tir_hash(&bytes, &tables);
            put_str(&mut tir, path);
            put_hash(&mut tir, h);
            write_tables(&tables, &mut tir);
            put_u32(&mut tir, u32::try_from(bytes.len()).expect("n"));
            tir.extend_from_slice(&bytes);
            put_u32(&mut tir, u32::try_from(spans.len()).expect("n"));
            tir.extend_from_slice(&spans);
            content = content.str(path).hash(h);
            for (p, ih) in &ck.deps {
                content = content.str(p).hash(*ih);
            }
        }
        let mut entry = Vec::new();
        put_u32(&mut entry, u32::try_from(errors.len()).expect("n"));
        for e in &errors {
            put_str(&mut entry, e);
        }
        put_hash(&mut entry, content.finish());
        let private: Vec<&HeaderItem> = prep.headers.iter().filter(|h| !h.public).collect();
        put_u32(&mut entry, u32::try_from(private.len()).expect("n"));
        for h in private {
            encode_item(&mut entry, h);
        }
        entry.extend_from_slice(&tir);
        let key = self.c.check_keys[&module];
        let entry = self.store.put("check", key, entry);
        // D2 reads TIR from the entry, never from the checker's memory.
        self.read_check_entry(mi, entry);
    }

    /// Reads diagnostics and the meta section only; the rest waits for
    /// `Collect` (walking skeleton, SK-N16).
    fn read_check_entry(&mut self, mi: usize, bytes: Arc<[u8]>) {
        let module = self.files[mi].module.clone();
        let mut r = Reader::new(&bytes);
        let ne = r.u32();
        for _ in 0..ne {
            let e = r.str();
            self.diagnostics.push(e);
        }
        self.tir_content.insert(module.clone(), r.hash());
        let rest = r.pos;
        self.pending.insert(module, PendingEntry { bytes, rest });
    }

    /// Decodes the headers and TIR sections of every module's entry.
    fn decode_pending(&mut self) {
        for (module, p) in std::mem::take(&mut self.pending) {
            let mut r = Reader::new(&p.bytes);
            r.pos = p.rest;
            let nh = r.u32();
            let headers: Vec<HeaderItem> = (0..nh).map(|_| decode_item(&mut r)).collect();
            load_items(&mut self.w, &headers, None);
            let nb = r.u32();
            for _ in 0..nb {
                let path = r.str();
                let h = r.hash();
                let rows = read_tables(&mut self.w, &mut r);
                let n = r.u32() as usize;
                let mut br = Reader::new(&r.bytes[r.pos..r.pos + n]);
                let mut body = read_body(&mut self.w, &rows, &mut br);
                r.pos += n;
                let ns = r.u32() as usize;
                r.pos += ns; // spans: locations only
                let def = self.w.def(&path);
                body.item = Some(def);
                self.tir.insert(def, body);
                self.tir_hashes.insert(def, h);
            }
            self.c.tir_decoded.push(module);
        }
    }

    fn package_result(&mut self, g: &mut TaskGraph) {
        if self.diagnostics.is_empty() {
            g.add(TaskKind::Ext(ExtTask::Collect), &[]);
        }
    }

    fn collect(&mut self, g: &mut TaskGraph) {
        // prog_key (codegen.md §11.3): TIR content of the modules.
        let modules: Vec<(&str, Hash128)> =
            self.tir_content.iter().map(|(m, h)| (m.as_str(), *h)).collect();
        self.prog_key = prog_key(self.toolchain, PIPELINE, &self.entry, &modules);
        if let Some(wasm) = self.lookup("link", self.prog_key) {
            self.wasm = Some(wasm.to_vec());
            return;
        }
        self.decode_pending();
        let Some(root) = self.w.lookup(&item_path(&self.entry, "main")) else {
            self.diagnostics.push(format!("no-main: `{}` has no `fn main`", self.entry));
            return;
        };
        let set = match collect(&mut self.w, &self.tir, root) {
            Ok(set) => set,
            Err(e) => {
                self.diagnostics.push(format!("collect: {e}"));
                return;
            }
        };
        self.instances = set.instances;
        self.callee_reps = set.callee_reps;
        self.data_types = set.types.into_iter().collect();
        self.imports = set.imports.into_iter().collect();
        self.codes = vec![None; self.instances.len()];
        let link = g.add(TaskKind::Ext(ExtTask::Link), &[]);
        let emits: Vec<TaskId> = (0..self.instances.len())
            .map(|i| g.add(TaskKind::Ext(ExtTask::Emit(u32::try_from(i).expect("i"))), &[]))
            .collect();
        for e in emits {
            g.edge(e, link);
        }
    }

    fn emit(&mut self, i: usize) {
        let (ikey, inst) = self.instances[i].clone();
        // Code key (§13.8): the stored TIR hash, no re-serialization.
        let ck = code_key(PIPELINE, ikey, self.tir_hashes[&inst.item], self.callee_reps[&ikey]);
        let code = if let Some(bytes) = self.lookup("code", ck) {
            Code::decode(&bytes)
        } else {
            self.c.emitted += 1;
            let body = &self.tir[&inst.item];
            let c = emit(&mut self.w, &self.tir, body, &inst);
            self.store.put("code", ck, c.encode());
            c
        };
        self.codes[i] = Some((ikey, code));
    }

    fn link(&mut self) {
        let root = self.w.lookup(&item_path(&self.entry, "main")).expect("main");
        let root_key = instance_key(&self.w, &Instance { item: root, ty_args: vec![] });
        let codes: Vec<(Hash128, Code)> =
            self.codes.iter_mut().map(|c| c.take().expect("emitted")).collect();
        let wasm = link(&self.w, &codes, root_key, &self.data_types, &self.imports);
        self.store.put("link", self.prog_key, wasm.clone());
        self.wasm = Some(wasm);
    }
}
