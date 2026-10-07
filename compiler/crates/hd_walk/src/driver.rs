//! The driver: one run over a source set, through every stage, as tasks of
//! the serial scheduler, with every stage boundary through the cache.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{Duration, Instant};

use hd_base::Hash128;
use hd_syntax::subset::{SubsetParse, parse_subset};
use hd_syntax::{SyntaxKind, skim};

use crate::check::check_fn;
use crate::iface::{
    Cst, FolderIface, HeaderItem, Scope, build_iface, decode_iface, decode_item, encode_item,
    folder_of_module, item_path, load_items, lower_headers, module_scope,
};
use crate::mono::{Instance, collect};
use crate::sched::{ExtTask, TaskGraph, TaskId, TaskKind};
use crate::tir::{Tables, TirBody, read_body, read_tables, tir_hash, write_body, write_tables};
use crate::wasm::{Code, emit, link};
use crate::world::{DefId, KeyHasher, Reader, World, put_hash, put_str, put_u32};

// ------------------------------------------------------------------ cache

/// The in-memory `CacheStore` (cache.md §5): entries by (kind, key), bytes
/// only, except code entries, which hold symbolic code (no run IDs).
#[derive(Default)]
pub struct MemStore {
    entries: HashMap<(&'static str, u128), Vec<u8>>,
    code: HashMap<u128, Code>,
}

#[derive(Default, Debug, Clone)]
pub struct Counters {
    pub tasks: BTreeMap<&'static str, usize>,
    pub hits: BTreeMap<&'static str, usize>,
    pub misses: BTreeMap<&'static str, usize>,
    pub modules_checked: Vec<String>,
    pub ifaces_built: Vec<String>,
    pub parsed: Vec<String>,
    pub emitted: usize,
    pub stage_time: BTreeMap<&'static str, Duration>,
    pub deep_hashes: BTreeMap<String, Hash128>,
    pub check_keys: BTreeMap<String, Hash128>,
}

impl Counters {
    pub fn hit(&self, kind: &str) -> usize {
        self.hits.get(kind).copied().unwrap_or(0)
    }
    pub fn miss(&self, kind: &str) -> usize {
        self.misses.get(kind).copied().unwrap_or(0)
    }
    pub fn ran(&self, task: &str) -> usize {
        self.tasks.get(task).copied().unwrap_or(0)
    }
}

impl MemStore {
    fn get(&self, c: &mut Counters, kind: &'static str, key: Hash128) -> Option<Vec<u8>> {
        let v = self.entries.get(&(kind, key.0)).cloned();
        *if v.is_some() { c.hits.entry(kind) } else { c.misses.entry(kind) }.or_default() += 1;
        v
    }
    fn put(&mut self, kind: &'static str, key: Hash128, bytes: Vec<u8>) {
        self.entries.insert((kind, key.0), bytes);
    }
}

// ----------------------------------------------------------------- session

pub struct SourceFile {
    pub path: String,
    pub text: String,
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
    bodies: Vec<(String, crate::check::Checked)>,
}

struct Run<'s> {
    store: &'s mut MemStore,
    c: Counters,
    w: World,
    files: Vec<FileSlot>,
    sources: &'s [SourceFile],
    folders: Vec<String>,
    folder_uses: HashMap<String, BTreeSet<String>>,
    ifaces: HashMap<String, FolderIface>,
    iface_tasks: HashMap<String, TaskId>,
    prep: HashMap<u32, Prep>,
    tir: HashMap<DefId, TirBody>,
    tir_content: BTreeMap<String, Hash128>,
    diagnostics: Vec<String>,
    package_result: Option<TaskId>,
    entry: String,
    instances: Vec<(Hash128, Instance)>,
    codes: Vec<Option<(Hash128, Code)>>,
    data_types: Vec<String>,
    imports: Vec<u32>,
    link_task: Option<TaskId>,
    prog_key: Hash128,
    wasm: Option<Vec<u8>>,
}

fn toolchain_key() -> Hash128 {
    KeyHasher::new("tc").str("hd_walk 0").str("wasm32-gc").finish()
}
fn package_key() -> Hash128 {
    KeyHasher::new("root").str("pkg").finish()
}

pub fn module_path(file: &str) -> String {
    format!("pkg.{}", file.trim_end_matches(".hd").replace('/', "."))
}

/// One run: a fresh `World` (fresh run IDs) over a shared store.
pub fn run(store: &mut MemStore, sources: &[SourceFile], entry_module: &str) -> RunResult {
    let mut r = Run {
        store,
        c: Counters::default(),
        w: World::default(),
        files: Vec::new(),
        sources,
        folders: Vec::new(),
        folder_uses: HashMap::new(),
        ifaces: HashMap::new(),
        iface_tasks: HashMap::new(),
        prep: HashMap::new(),
        tir: HashMap::new(),
        tir_content: BTreeMap::new(),
        diagnostics: Vec::new(),
        package_result: None,
        entry: entry_module.to_owned(),
        instances: Vec::new(),
        codes: Vec::new(),
        data_types: Vec::new(),
        imports: Vec::new(),
        link_task: None,
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
            TaskKind::Parse(_) => {}
        }
    }

    fn skim(&mut self, f: usize) {
        let src = &self.sources[f].text;
        let sk = skim(src.as_bytes());
        let slot = &mut self.files[f];
        slot.source_hash = sk.source_hash;
        slot.api_text_hash = sk.api_text_hash;
        for (lo, hi) in sk.uses {
            let text = &src[lo as usize..hi as usize];
            if let Some(rest) = text.strip_prefix("use ") {
                let path = rest.split(".{").next().unwrap_or(rest).trim().to_owned();
                slot.uses.push(path);
            }
        }
    }

    /// Lazy full parse ("full parse if needed", M1): only on a miss.
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
        let mut files: Vec<usize> = (0..self.files.len()).filter(|&i| self.files[i].folder == folder).collect();
        files.sort_by(|a, b| self.files[*a].module.cmp(&self.files[*b].module));
        // iface_key (cache.md §5.3)
        let mut k = KeyHasher::new("iface").hash(toolchain_key()).hash(package_key()).str(&folder);
        for &f in &files {
            k = k.str(&self.files[f].module).str("lib").hash(self.files[f].api_text_hash);
        }
        let mut reach = self.closure(&folder);
        reach.remove(&folder);
        for d in &reach {
            k = k.str(d).hash(self.ifaces[d].deep_hash);
        }
        let key = k.finish();
        let iface = if let Some(blob) = self.store.get(&mut self.c, "iface", key) {
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
            let iface = build_iface(&folder, items, &self.ifaces);
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
        let mut k = KeyHasher::new("check")
            .hash(toolchain_key())
            .hash(package_key())
            .str(&f.module)
            .str("lib")
            .hash(f.source_hash);
        for c in self.closure(&f.folder) {
            k = k.str(&c).hash(self.ifaces[&c].deep_hash);
        }
        k.finish()
    }

    fn module_prep(&mut self, id: TaskId, m: u32, g: &mut TaskGraph) {
        let mi = m as usize;
        let key = self.check_key(mi);
        self.c.check_keys.insert(self.files[mi].module.clone(), key);
        // "compute key, look up, skip or run" is the first step (§6.1).
        if let Some(entry) = self.store.get(&mut self.c, "check", key) {
            self.read_check_entry(mi, &entry);
            return;
        }
        self.parse(mi);
        let module = self.files[mi].module.clone();
        let src = &self.sources[mi].text;
        let p = self.files[mi].parse.as_ref().expect("parsed");
        let cst = Cst { src, p };
        let scope = module_scope(&cst, &module, &self.ifaces);
        let (headers, errs) = lower_headers(&cst, &module, &scope);
        self.diagnostics.extend(scope.errors.iter().cloned());
        self.diagnostics.extend(errs);
        load_items(&mut self.w, &headers, None);
        self.prep.insert(m, Prep { scope, headers, bodies: Vec::new() });
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
        // The bodies of the module, in source order: functions, impl methods.
        let impl_paths: Vec<String> = prep
            .headers
            .iter()
            .filter(|h| matches!(h.item, crate::iface::CItem::Impl { .. }))
            .map(|h| h.path.clone())
            .collect();
        let mut next_impl = 0;
        for item in cst.root().children() {
            match item.kind() {
                SyntaxKind::FnDecl => {
                    let path = item_path(&module, cst.first_ident(item));
                    let def = self.w.def(&path);
                    let ck = check_fn(&mut self.w, &cst, &prep.scope, def, item);
                    prep.bodies.push((path, ck));
                }
                SyntaxKind::ImplDecl => {
                    let impl_path = impl_paths.get(next_impl).cloned();
                    next_impl += 1;
                    let Some(impl_path) = impl_path else { continue };
                    for f in item.children().filter(|c| c.kind() == SyntaxKind::FnDecl) {
                        let path = format!("{impl_path}.{}", cst.first_ident(f));
                        let def = self.w.def(&path);
                        let ck = check_fn(&mut self.w, &cst, &prep.scope, def, f);
                        prep.bodies.push((path, ck));
                    }
                }
                _ => {}
            }
        }
    }

    fn module_finish(&mut self, m: u32) {
        let mi = m as usize;
        let prep = self.prep.remove(&m).expect("prep");
        let module = self.files[mi].module.clone();
        self.c.modules_checked.push(module.clone());
        let mut errors = Vec::new();
        for (path, ck) in &prep.bodies {
            for e in &ck.errors {
                errors.push(format!("{module}: {path}: {e}"));
            }
        }
        // The check entry: diagnostics, private headers, TIR sections.
        let mut entry = Vec::new();
        put_u32(&mut entry, u32::try_from(errors.len()).expect("n"));
        for e in &errors {
            put_str(&mut entry, e);
        }
        put_u32(&mut entry, u32::try_from(prep.headers.len()).expect("n"));
        for h in &prep.headers {
            encode_item(&mut entry, h);
        }
        let has_tir = errors.is_empty();
        put_u32(&mut entry, u32::from(has_tir) * u32::try_from(prep.bodies.len()).expect("n"));
        if has_tir {
            for (path, ck) in &prep.bodies {
                let mut tables = Tables::default();
                let mut bytes = Vec::new();
                let mut spans = Vec::new();
                write_body(&self.w, &mut tables, &ck.body, &mut bytes, &mut spans);
                let h = tir_hash(&bytes, &tables);
                put_str(&mut entry, path);
                write_tables(&tables, &mut entry);
                put_u32(&mut entry, u32::try_from(bytes.len()).expect("n"));
                entry.extend_from_slice(&bytes);
                put_u32(&mut entry, u32::try_from(spans.len()).expect("n"));
                entry.extend_from_slice(&spans);
                put_hash(&mut entry, h);
                put_u32(&mut entry, u32::try_from(ck.deps.len()).expect("n"));
                for (p, ih) in &ck.deps {
                    put_str(&mut entry, p);
                    put_hash(&mut entry, *ih);
                }
            }
        }
        let key = self.c.check_keys[&module];
        self.store.put("check", key, entry.clone());
        // D2 reads TIR from the entry, never from the checker's memory.
        self.read_check_entry(mi, &entry);
    }

    fn read_check_entry(&mut self, mi: usize, entry: &[u8]) {
        let module = self.files[mi].module.clone();
        let mut r = Reader::new(entry);
        let ne = r.u32();
        for _ in 0..ne {
            let e = r.str();
            self.diagnostics.push(e);
        }
        let nh = r.u32();
        let headers: Vec<HeaderItem> = (0..nh).map(|_| decode_item(&mut r)).collect();
        load_items(&mut self.w, &headers, None);
        let nb = r.u32();
        let mut content = KeyHasher::new("tir-content");
        for _ in 0..nb {
            let path = r.str();
            let rows = read_tables(&mut self.w, &mut r);
            let n = r.u32() as usize;
            let body_bytes = r.bytes[r.pos..r.pos + n].to_vec();
            r.pos += n;
            let ns = r.u32() as usize;
            r.pos += ns; // spans: locations only
            let h = r.hash();
            let mut deps = Vec::new();
            for _ in 0..r.u32() {
                deps.push((r.str(), r.hash()));
            }
            let mut br = Reader::new(&body_bytes);
            let mut body = read_body(&mut self.w, &rows, &mut br);
            let def = self.w.def(&path);
            body.item = Some(def);
            self.tir.insert(def, body);
            content = content.str(&path).hash(h);
            for (p, ih) in deps {
                content = content.str(&p).hash(ih);
            }
        }
        self.tir_content.insert(module, content.finish());
    }

    fn package_result(&mut self, g: &mut TaskGraph) {
        if self.diagnostics.is_empty() {
            g.add(TaskKind::Ext(ExtTask::Collect), &[]);
        }
    }

    fn collect(&mut self, g: &mut TaskGraph) {
        // prog_key (codegen.md §11.3): TIR content of the reachable modules.
        let mut k = KeyHasher::new("prog").hash(toolchain_key()).str("dev").str(&self.entry);
        for (m, h) in &self.tir_content {
            k = k.str(m).hash(*h);
        }
        self.prog_key = k.finish();
        if let Some(wasm) = self.store.get(&mut self.c, "link", self.prog_key) {
            self.wasm = Some(wasm);
            return;
        }
        let root = self.w.lookup(&item_path(&self.entry, "main")).expect("main");
        let set = collect(&mut self.w, &self.tir, root);
        self.instances = set.instances;
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
        self.link_task = Some(link);
    }

    fn emit(&mut self, i: usize) {
        let (ikey, inst) = self.instances[i].clone();
        let body = &self.tir[&inst.item];
        // Code key (§13.8): instance key, the item's TIR content, pipeline.
        let mut tables = Tables::default();
        let mut bytes = Vec::new();
        write_body(&self.w, &mut tables, body, &mut bytes, &mut Vec::new());
        let ck = KeyHasher::new("code").str("dev").hash(ikey).hash(tir_hash(&bytes, &tables)).finish();
        let code = if let Some(c) = self.store.code.get(&ck.0).cloned() {
            *self.c.hits.entry("code").or_default() += 1;
            c
        } else {
            *self.c.misses.entry("code").or_default() += 1;
            self.c.emitted += 1;
            let body = self.tir[&inst.item].clone();
            let c = emit(&mut self.w, &self.tir, &body, &inst);
            self.store.code.insert(ck.0, c.clone());
            c
        };
        self.codes[i] = Some((ikey, code));
    }

    fn link(&mut self) {
        let root = self.w.lookup(&item_path(&self.entry, "main")).expect("main");
        let root_key = crate::mono::instance_key(&self.w, &Instance { item: root, ty_args: vec![] });
        let codes: Vec<(Hash128, Code)> = self.codes.iter().map(|c| c.clone().expect("emitted")).collect();
        let wasm = link(&self.w, &codes, root_key, &self.data_types, &self.imports);
        self.store.put("link", self.prog_key, wasm.clone());
        self.wasm = Some(wasm);
    }
}
