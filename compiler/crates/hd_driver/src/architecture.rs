//! The architecture driver: the full task graph of scheduler.md §6.1 over a
//! real package, every stage called in order, and a count per stage of how
//! far the package gets. Stages that the code does not carry yet answer
//! `NotImplemented`; a stage whose input failed is counted as blocked.
//!
//! Skim, parse, discovery and the folder graph run the real slice-1 and
//! slice-2 code. Folder interfaces, module prep and bodies run the walking
//! skeleton's subset pipeline, so any construct outside the subset is
//! reported as not implemented, with its first reason.

use std::collections::{BTreeMap, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};

use hd_base::{NotImplemented, Stage, StageResult};
use hd_check::stages::{ModuleFacts, coherence, init_order, test_overlay};
use hd_check::{A1Rule, Cst, Scope, check_module_bodies, lower_headers, module_scope};
use hd_iface::{FolderIface, HeaderItem, build_iface};
use hd_project::{FolderGraph, ModuleTable, SourceSet};
use hd_sched::{ExtTask, TaskGraph, TaskId, TaskKind};
use hd_syntax::subset::{SubsetParse, parse_subset};
use hd_syntax::{HeaderKind, HeaderSkeleton, skim};
use hd_tir::world::{World, load_items};

/// One stage's tally.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub ok: usize,
    pub not_implemented: usize,
    pub blocked: usize,
    pub first_reason: Option<String>,
}

/// Per-stage counts for one package.
#[derive(Clone, Debug, Default)]
pub struct PipelineReport {
    pub package: String,
    pub stages: BTreeMap<Stage, Tally>,
    /// Not-implemented reasons by frequency (first 60 characters).
    pub reasons: BTreeMap<String, usize>,
}

impl PipelineReport {
    fn ok(&mut self, s: Stage) {
        self.stages.entry(s).or_default().ok += 1;
    }
    fn blocked(&mut self, s: Stage) {
        self.stages.entry(s).or_default().blocked += 1;
    }
    fn record<T>(&mut self, s: Stage, r: &StageResult<T>) -> bool {
        match r {
            Ok(_) => {
                self.ok(s);
                true
            }
            Err(e) => {
                let t = self.stages.entry(s).or_default();
                t.not_implemented += 1;
                if t.first_reason.is_none() {
                    t.first_reason = Some(e.what.clone());
                }
                let short: String = format!("{}: {}", s.name(), e.what).chars().take(90).collect();
                *self.reasons.entry(short).or_default() += 1;
                false
            }
        }
    }

    #[must_use]
    pub fn tally(&self, s: Stage) -> Tally {
        self.stages.get(&s).cloned().unwrap_or_default()
    }

    /// A plain table, one row per stage in run order.
    #[must_use]
    pub fn render(&self) -> String {
        use std::fmt::Write as _;
        let mut o = format!("package {}\n{:<14} {:>5} {:>8} {:>8}  first reason\n", self.package, "stage", "ok", "not-impl", "blocked");
        for s in Stage::ALL {
            let t = self.tally(s);
            let reason: String = t.first_reason.unwrap_or_default().chars().take(80).collect();
            let _ = writeln!(o, "{:<14} {:>5} {:>8} {:>8}  {reason}", s.name(), t.ok, t.not_implemented, t.blocked);
        }
        o
    }
}

/// Facts the checking stages read from a module's skim.
#[must_use]
pub fn facts_of(module: &str, sk: &HeaderSkeleton) -> ModuleFacts {
    ModuleFacts {
        path: module.to_owned(),
        has_tests_block: sk.bodies.iter().any(|b| b.kind == HeaderKind::Tests),
        top_level_statements: sk.bodies.iter().filter(|b| b.kind == HeaderKind::Control && b.header_indent == 0).count(),
        impls: sk.bodies.iter().filter(|b| b.kind == HeaderKind::Impl && b.header_indent == 0).count(),
    }
}

struct File {
    module: String,
    text: String,
    skim: HeaderSkeleton,
    uses: Vec<String>,
    subset: Option<SubsetParse>,
    facts: ModuleFacts,
}

struct Analysis {
    r: PipelineReport,
    table: ModuleTable,
    files: Vec<File>,
    graph: FolderGraph,
    iface_ok: Vec<bool>,
    iface_task: Vec<Option<TaskId>>,
    ifaces: HashMap<String, FolderIface>,
    prep: HashMap<u32, (Scope, Vec<HeaderItem>)>,
    body_ok: Vec<bool>,
    package_result: Option<TaskId>,
    has_main: bool,
}

/// Runs every stage of the design over one package.
pub fn analyze_package(package: &str, sources: &dyn SourceSet) -> PipelineReport {
    let table = ModuleTable::discover(package, sources);
    let mut a = Analysis {
        r: PipelineReport { package: package.to_owned(), ..PipelineReport::default() },
        files: Vec::new(),
        graph: FolderGraph::default(),
        iface_ok: vec![false; table.folders.len()],
        iface_task: vec![None; table.folders.len()],
        ifaces: HashMap::new(),
        prep: HashMap::new(),
        body_ok: vec![false; table.modules.len()],
        package_result: None,
        has_main: false,
        table,
    };
    a.r.ok(Stage::Discover);
    let mut g = TaskGraph::default();
    let mut skims = Vec::new();
    for (i, path) in a.table.files.iter().enumerate() {
        let text = sources.read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        a.files.push(File {
            module: a.table.modules[i].path.clone(),
            text,
            skim: HeaderSkeleton::default(),
            uses: vec![],
            subset: None,
            facts: ModuleFacts::default(),
        });
        skims.push(g.add(TaskKind::Skim(u32::try_from(i).expect("files")), &[]));
        g.add(TaskKind::Parse(u32::try_from(i).expect("files")), &[]);
    }
    g.add(TaskKind::FolderGraph, &skims);
    g.run(&mut |id, kind, g| a.exec(id, kind, g));
    a.r
}

impl Analysis {
    fn exec(&mut self, id: TaskId, kind: TaskKind, g: &mut TaskGraph) {
        match kind {
            TaskKind::Skim(f) => self.skim(f as usize),
            TaskKind::Parse(f) => self.parse(f as usize),
            TaskKind::FolderGraph => self.folder_graph(g),
            TaskKind::FolderIface(f) => self.folder_iface(f as usize),
            TaskKind::HeaderCheck(f) => {
                let r = hd_resolve::header_check(&self.table, &self.graph, f as usize);
                self.r.record(Stage::HeaderCheck, &r);
            }
            TaskKind::ModulePrep(m) => self.module_prep(id, m, g),
            TaskKind::Body(m) => self.body(m as usize),
            TaskKind::ModuleFinish(m) => {
                if self.body_ok[m as usize] {
                    self.r.ok(Stage::ModuleFinish);
                } else {
                    self.r.blocked(Stage::ModuleFinish);
                }
            }
            TaskKind::TestOverlay(m) => {
                let r = test_overlay(&self.files[m as usize].facts);
                self.r.record(Stage::TestOverlay, &r);
            }
            TaskKind::Coherence => {
                let facts: Vec<ModuleFacts> = self.files.iter().map(|f| f.facts.clone()).collect();
                let r = coherence(&facts);
                self.r.record(Stage::Coherence, &r);
            }
            TaskKind::InitOrder(f) => {
                let folder = &self.table.folders[f as usize];
                let facts: Vec<&ModuleFacts> = folder.modules.iter().map(|m| &self.files[m.idx()].facts).collect();
                let r = init_order(&folder.path, &facts);
                self.r.record(Stage::InitOrder, &r);
            }
            TaskKind::PackageResult => self.package_result(g),
            TaskKind::Ext(ExtTask::Collect) => {
                let r: StageResult<()> = if self.has_main {
                    Err(NotImplemented::new(Stage::Collect, "collect over the full TIR (the subset path serves `hd run`)"))
                } else {
                    Err(NotImplemented::new(Stage::Collect, "no program root: a library package needs a test plan (§13.1)"))
                };
                self.r.record(Stage::Collect, &r);
                for s in [Stage::Emit, Stage::Link, Stage::Precompile, Stage::Run] {
                    self.r.blocked(s);
                }
            }
            TaskKind::Ext(_) => {}
        }
    }

    fn skim(&mut self, f: usize) {
        let sk = skim(self.files[f].text.as_bytes());
        let file = &mut self.files[f];
        file.uses = crate::source::use_paths(&file.text, &sk);
        file.facts = facts_of(&file.module, &sk);
        if file.text.lines().any(|l| l.starts_with("fn main(")) {
            self.has_main = true;
        }
        let r: StageResult<()> =
            if sk.broken { Err(NotImplemented::new(Stage::Skim, "lexer diagnostics in skim mode")) } else { Ok(()) };
        file.skim = sk;
        self.r.record(Stage::Skim, &r);
    }

    fn parse(&mut self, f: usize) {
        let p = hd_syntax::parse(self.files[f].text.as_bytes());
        let r: StageResult<()> = match p.diagnostics.first() {
            None => Ok(()),
            Some(d) => Err(NotImplemented::new(Stage::Parse, format!("full parser reports {}", d.code.as_str()))),
        };
        self.r.record(Stage::Parse, &r);
    }

    fn folder_graph(&mut self, g: &mut TaskGraph) {
        let uses: Vec<Vec<String>> = self.files.iter().map(|f| f.uses.clone()).collect();
        self.graph = FolderGraph::build(&self.table, &uses);
        let r: StageResult<()> = if self.graph.cycles.is_empty() {
            Ok(())
        } else {
            Err(NotImplemented::new(Stage::FolderGraph, "folder-cycle reported"))
        };
        self.r.record(Stage::FolderGraph, &r);
        let mut checks = Vec::new();
        for &f in &self.graph.order.clone() {
            let deps: Vec<TaskId> = self.graph.uses[f.idx()].iter().filter_map(|u| self.iface_task[u.idx()]).collect();
            let t = g.add_with_priority(TaskKind::FolderIface(f.raw()), &deps, self.graph.height[f.idx()]);
            self.iface_task[f.idx()] = Some(t);
            let hdeps: Vec<TaskId> = self.graph.closure[f.idx()].iter().filter_map(|c| self.iface_task[c.idx()]).collect();
            checks.push(g.add(TaskKind::HeaderCheck(f.raw()), &hdeps));
            checks.push(g.add(TaskKind::InitOrder(f.raw()), &[t]));
        }
        let all: Vec<TaskId> = self.iface_task.iter().flatten().copied().collect();
        checks.push(g.add(TaskKind::Coherence, &all));
        let mut preps = Vec::new();
        for m in 0..self.table.modules.len() {
            let folder = self.table.modules[m].folder;
            let deps: Vec<TaskId> = self.graph.closure[folder.idx()].iter().filter_map(|c| self.iface_task[c.idx()]).collect();
            preps.push(g.add(TaskKind::ModulePrep(u32::try_from(m).expect("m")), &deps));
        }
        preps.extend(checks);
        self.package_result = Some(g.add(TaskKind::PackageResult, &preps));
    }

    fn subset(&mut self, f: usize) -> Result<(), String> {
        if self.files[f].subset.is_none() {
            self.files[f].subset = Some(parse_subset(&self.files[f].text));
        }
        match self.files[f].subset.as_ref().and_then(|p| p.errors.first()) {
            Some(e) => Err(format!("subset parser: {e}")),
            None => Ok(()),
        }
    }

    fn folder_iface(&mut self, fi: usize) {
        let folder = self.table.folders[fi].clone();
        let deps_ok = self.graph.uses[fi].iter().all(|u| self.iface_ok[u.idx()]);
        if !deps_ok {
            self.r.blocked(Stage::FolderIface);
            return;
        }
        let mut items = Vec::new();
        for m in &folder.modules {
            let f = m.idx();
            if let Err(e) = self.subset(f) {
                self.r.record::<()>(Stage::FolderIface, &Err(NotImplemented::new(Stage::FolderIface, e)));
                return;
            }
            let file = &self.files[f];
            let p = file.subset.as_ref().expect("parsed");
            let cst = Cst { src: &file.text, p };
            let scope = module_scope(&cst, &file.module, &self.ifaces);
            let (hs, errs) = lower_headers(&cst, &file.module, &scope);
            if let Some(e) = scope.errors.first().or(errs.first()) {
                let what = format!("subset resolution: {e}");
                self.r.record::<()>(Stage::FolderIface, &Err(NotImplemented::new(Stage::FolderIface, what)));
                return;
            }
            items.extend(hs.into_iter().filter(|h| h.public));
        }
        let iface = build_iface(&folder.path, &items, &self.ifaces);
        self.ifaces.insert(folder.path.clone(), iface);
        self.iface_ok[fi] = true;
        self.r.ok(Stage::FolderIface);
    }

    fn module_prep(&mut self, id: TaskId, m: u32, g: &mut TaskGraph) {
        let mi = m as usize;
        let folder = self.table.modules[mi].folder.idx();
        let body = g.add(TaskKind::Body(m), &[id]);
        let finish = g.add(TaskKind::ModuleFinish(m), &[body]);
        let overlay = g.add(TaskKind::TestOverlay(m), &[id]);
        let pr = self.package_result.expect("package result");
        g.edge(finish, pr);
        g.edge(overlay, pr);
        if !self.iface_ok[folder] {
            self.r.blocked(Stage::ModulePrep);
            return;
        }
        let file = &self.files[mi];
        let p = file.subset.as_ref().expect("parsed with its folder");
        let cst = Cst { src: &file.text, p };
        let scope = module_scope(&cst, &file.module, &self.ifaces);
        let (headers, errs) = lower_headers(&cst, &file.module, &scope);
        if let Some(e) = scope.errors.first().or(errs.first()) {
            let what = format!("subset resolution: {e}");
            self.r.record::<()>(Stage::ModulePrep, &Err(NotImplemented::new(Stage::ModulePrep, what)));
            return;
        }
        self.prep.insert(m, (scope, headers));
        self.r.ok(Stage::ModulePrep);
    }

    fn body(&mut self, mi: usize) {
        let Some((scope, headers)) = self.prep.remove(&u32::try_from(mi).expect("m")) else {
            self.r.blocked(Stage::Body);
            return;
        };
        let file = &self.files[mi];
        let p = file.subset.as_ref().expect("parsed");
        let ifaces = &self.ifaces;
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let mut w = World::default();
            for i in ifaces.values() {
                load_items(&mut w, &i.items, Some(&i.item_hashes));
            }
            load_items(&mut w, &headers, None);
            let cst = Cst { src: &file.text, p };
            check_module_bodies(&mut w, &cst, &file.module, &scope, &headers, A1Rule::default())
        }));
        let r: StageResult<()> = match outcome {
            Ok(bodies) => match bodies.iter().find_map(|(path, ck)| ck.errors.first().map(|e| format!("{path}: {e}"))) {
                None => Ok(()),
                Some(e) => Err(NotImplemented::new(Stage::Body, format!("subset checker: {e}"))),
            },
            Err(_) => Err(NotImplemented::new(Stage::Body, "subset checker panicked (caught at the task boundary, §6.4)")),
        };
        self.body_ok[mi] = self.r.record(Stage::Body, &r);
    }

    fn package_result(&mut self, g: &mut TaskGraph) {
        self.r.ok(Stage::PackageResult);
        g.add(TaskKind::Ext(ExtTask::Collect), &[]);
    }
}

#[cfg(test)]
mod tests {
    use super::analyze_package;
    use hd_base::Stage;
    use hd_project::MemorySources;

    #[test]
    fn a_subset_program_gets_through_checking() {
        let mut s = MemorySources::default();
        s.insert("main.hd", "fn main():\n    println(42)\n");
        let r = analyze_package("pkg", &s);
        for st in [Stage::Skim, Stage::Parse, Stage::FolderGraph, Stage::FolderIface, Stage::ModulePrep, Stage::Body, Stage::ModuleFinish] {
            assert_eq!(r.tally(st).ok, 1, "{}", st.name());
        }
        assert_eq!(r.tally(Stage::HeaderCheck).not_implemented, 1);
        assert_eq!(r.tally(Stage::Collect).not_implemented, 1);
        assert!(r.render().contains("HeaderCheck"));
    }
}
