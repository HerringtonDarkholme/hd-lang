//! `hd test --affected` (spec/cli/command-line.md, "Affected Tests"): each
//! test program's fingerprint (`cli.test.fingerprint`), and the records of
//! the fingerprints of the programs that last passed in full
//! (`cli.test.affected.record`), kept in the build directory
//! (`cli.test.affected.where`).
//!
//! A fingerprint must change whenever a change could change the program's
//! outcome, so it errs on the side of including an input: a stale skip
//! hides a failure, while an extra run only costs time.

use hd_base::SliceSort as _;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use hd_base::{Hash128, StableHasher};
use hd_driver::Output;
use hd_project::SourceSet as _;

use crate::caps::CapFlag;
use crate::disk;
use crate::engine::{EngineKind, ProgramLimits};

/// The records of one package: a file per test program that last passed
/// in full, under `build/test-records` (`cli.test.affected.where`), so
/// `hd clean` removes them with the rest of the build directory.
pub(crate) struct Records {
    dir: PathBuf,
}

impl Records {
    pub(crate) fn at(root: &Path) -> Records {
        Records {
            dir: root.join("build").join("test-records"),
        }
    }

    /// A program's record file: named by a hash of its id, which the file
    /// repeats, so two ids whose hashes meet never share a record.
    fn file(&self, id: &str) -> PathBuf {
        let mut h = StableHasher::new("test-record");
        h.str(id);
        self.dir.join(format!("{:032x}", h.finish().0))
    }

    fn text(id: &str, fingerprint: Hash128) -> String {
        format!("{id}\n{:032x}\n", fingerprint.0)
    }

    /// Whether `id` last passed in full with this fingerprint
    /// (`cli.test.affected`).
    pub(crate) fn passed(&self, id: &str, fingerprint: Hash128) -> bool {
        std::fs::read_to_string(self.file(id)).is_ok_and(|t| t == Records::text(id, fingerprint))
    }

    /// Records that `id` passed in full (`cli.test.affected.record`). A
    /// record that cannot be written is left out: the program then runs
    /// again next time, which is only slower.
    pub(crate) fn record(&self, id: &str, fingerprint: Hash128) {
        let file = self.file(id);
        let partial = file.with_extension("partial");
        let _ = std::fs::create_dir_all(&self.dir)
            .and_then(|()| std::fs::write(&partial, Records::text(id, fingerprint)))
            .and_then(|()| std::fs::rename(&partial, &file));
    }

    /// Removes the record of a program a test case of which failed
    /// (`cli.test.affected.failed`). A record that stays would skip a
    /// failing program, so a removal that fails is an error.
    pub(crate) fn forget(&self, id: &str) -> Result<(), String> {
        let file = self.file(id);
        match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{}: {e}", file.display()))
            }
            _ => Ok(()),
        }
    }
}

/// The content hash of each package file a fingerprint holds, by its change
/// stamp (`disk::stamp`), from the last `hd test --affected` run, kept in
/// `build/test-records/stamps` (`crates/hd_cache/DESIGN.md` §5.5). A file
/// whose stamp is the recorded one is not read: its recorded hash stands.
/// A fingerprint always holds content hashes, never stamps, so it does not
/// change when a file's stamp settles or its inode moves.
pub(crate) struct Stamps {
    /// `None` keeps no record: a run outside a package.
    file: Option<PathBuf>,
    old: hd_base::HashMap<String, (Hash128, Hash128)>,
    new: std::collections::BTreeMap<String, (Hash128, Hash128)>,
}

impl Stamps {
    /// The record of the package at `root`; none for a single file.
    pub(crate) fn at(root: &Path, single_file: bool) -> Stamps {
        let file = (!single_file).then(|| root.join("build").join("test-records").join("stamps"));
        let mut old = hd_base::HashMap::default();
        if let Some(text) = file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()) {
            for line in text.lines() {
                let mut parts = line.split('\t');
                let (Some(path), Some(stamp), Some(hash)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                if let (Ok(stamp), Ok(hash)) = (
                    u128::from_str_radix(stamp, 16),
                    u128::from_str_radix(hash, 16),
                ) {
                    old.insert(path.to_owned(), (Hash128(stamp), Hash128(hash)));
                }
            }
        }
        Stamps {
            file,
            old,
            new: std::collections::BTreeMap::new(),
        }
    }

    /// The content hash of the file at `path`, listed as `rel`, with
    /// change stamp `stamp`: the recorded one when the stamp matches, else
    /// its bytes' (`read`).
    fn hash(
        &mut self,
        rel: &str,
        stamp: Option<Hash128>,
        read: impl FnOnce() -> Vec<u8>,
    ) -> Hash128 {
        if let Some(stamp) = stamp
            && let Some(&(was, hash)) = self.old.get(rel)
            && was == stamp
        {
            self.new.insert(rel.to_owned(), (stamp, hash));
            return hash;
        }
        let hash = hd_base::hash128(&read());
        // A path that would break the line format is never recorded.
        if let Some(stamp) = stamp
            && self.file.is_some()
            && !rel.contains(['\t', '\n', '\r'])
        {
            self.new.insert(rel.to_owned(), (stamp, hash));
        }
        hash
    }

    /// Writes the record when it changed. A record that cannot be written
    /// is left out: the next run reads the files again, which is only
    /// slower.
    pub(crate) fn save(&self) {
        let Some(file) = &self.file else {
            return;
        };
        let same = self.new.len() == self.old.len()
            && self.new.iter().all(|(p, v)| self.old.get(p) == Some(v));
        if same {
            return;
        }
        let mut text = String::new();
        for (path, (stamp, hash)) in &self.new {
            let _ = writeln!(text, "{path}\t{:032x}\t{:032x}", stamp.0, hash.0);
        }
        let partial = file.with_extension("partial");
        let _ = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&partial, text))
            .and_then(|()| std::fs::rename(&partial, file));
    }
}

/// What the run's test options and environment add to every fingerprint of
/// one package (`cli.test.fingerprint`).
pub(crate) struct Settings<'a> {
    pub(crate) root: &'a Path,
    pub(crate) engine: EngineKind,
    pub(crate) release: bool,
    pub(crate) seed: Option<i64>,
    pub(crate) update: bool,
    pub(crate) limits: ProgramLimits,
    pub(crate) caps: &'a [CapFlag],
    pub(crate) program: &'a disk::Program,
}

/// The inputs every test program of one package shares: the `hd` binary,
/// the engine, the test options, the manifests, and the dependency set.
pub(crate) fn common(s: &Settings<'_>) -> Hash128 {
    let mut h = StableHasher::new("hd-test-common");
    // The `hd` version: the compiler's build id is in each output's
    // toolchain key; the binary also holds the runner and the JS host.
    h.str(env!("CARGO_PKG_VERSION"));
    executable(&mut h, std::env::current_exe().ok().as_deref());
    h.str(std::env::consts::OS);
    h.str(std::env::consts::ARCH);
    h.str(&format!("{:?}", s.engine));
    if s.engine == EngineKind::Node {
        executable(&mut h, on_path("node").as_deref());
    }
    h.u8(u8::from(s.release));
    h.str(&format!("{:?}", s.seed));
    h.u8(u8::from(s.update));
    h.str(&format!("{:?}", s.limits));
    for c in s.caps {
        h.str(&c.text);
    }
    // The manifest settings, `[dev-capabilities]` among them, and the
    // locked versions.
    for name in ["hd.toml", "hd.sum"] {
        file(&mut h, &s.root.join(name));
    }
    if let Some((ws, _)) = s.root.parent().and_then(disk::workspace_root) {
        file(&mut h, &ws.join("hd.toml"));
    }
    let p = s.program;
    h.str(&p.package);
    h.u8(u8::from(p.single_file));
    for d in &p.deps {
        h.str(&d.name);
        h.str(&d.identity);
    }
    h.finish()
}

/// The environment variables, names and values: what a program that
/// imports `std.host.Env` may read. Only their hash is kept.
pub(crate) fn environment() -> Hash128 {
    let mut vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os().collect();
    vars.sort_stable();
    let mut h = StableHasher::new("hd-test-environment");
    for (name, value) in vars {
        h.str(&name.to_string_lossy());
        h.str(&value.to_string_lossy());
    }
    h.finish()
}

/// Hashes an executable by its path, size and modification time: a new
/// build of it changes them.
fn executable(h: &mut StableHasher, path: Option<&Path>) {
    let Some(path) = path else {
        h.str("<none>");
        return;
    };
    h.str(&path.to_string_lossy());
    match std::fs::metadata(path) {
        Ok(m) => {
            h.u64(m.len());
            let since = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
            h.str(&format!("{since:?}"));
        }
        Err(_) => h.str("<unreadable>"),
    }
}

/// The first `name` on `PATH`, as the engine would start it.
fn on_path(name: &str) -> Option<PathBuf> {
    let exe = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(&exe))
        .find(|p| p.is_file())
}

/// Hashes a file's path and bytes, or that it is missing.
fn file(h: &mut StableHasher, path: &Path) {
    h.str(&path.to_string_lossy());
    match disk::read_regular(path) {
        Some(b) => {
            h.u8(1);
            h.bytes(&b);
        }
        None => h.u8(0),
    }
}

/// The files of the package that a test case may read at run time but
/// that are not its source: the snapshot and regression files of every
/// test case, and for an integration test or a doc test, which may read
/// the whole package directory (`cli.test.env.grant.fs-read`), every other
/// file in it but the build directory, hidden entries and the package's
/// source files, which each program's own hash covers.
pub(crate) struct Data {
    /// What a unit test can read: its snapshots and regressions.
    pub(crate) unit: Hash128,
    /// What an integration test or a doc test can read.
    pub(crate) package: Hash128,
}

pub(crate) fn data(
    root: &Path,
    sources: &disk::DiskSources,
    single_file: bool,
    stamps: &mut Stamps,
) -> Data {
    // Taken before any file is listed (`disk::stamp`).
    let now = std::time::SystemTime::now();
    let mut h = StableHasher::new("hd-test-snapshots");
    for dir in ["__snapshots__", "__regressions__"] {
        walk(&mut h, root, &root.join(dir), now, stamps, &|_| false);
    }
    let unit = h.finish();
    // Outside a package the directory is the FILE's, which may hold
    // anything; such a run keeps no records (`cli.test.affected.where`),
    // so that directory is never walked.
    let package = if single_file {
        unit
    } else {
        let listed: hd_base::HashSet<String> = sources
            .list()
            .into_iter()
            .map(|e| sources.display(&e.path))
            .collect();
        let mut h = StableHasher::new("hd-test-package-data");
        walk(&mut h, root, root, now, stamps, &|rel| listed.contains(rel));
        h.finish()
    };
    Data { unit, package }
}

/// Hashes the files below `dir`, in path order, by their path relative to
/// `root` and their content hash (`Stamps::hash`), skipping the build
/// directory, hidden entries and the files `skip` names. A symbolic link
/// to a directory is hashed by its target's path, so a loop of links ends.
fn walk(
    h: &mut StableHasher,
    root: &Path,
    dir: &Path,
    now: std::time::SystemTime,
    stamps: &mut Stamps,
    skip: &dyn Fn(&str) -> bool,
) {
    // A file is read only when its stamp is not the recorded one: it
    // changed, or it changed within `disk::SETTLE` of the run.
    // cost: per `hd test` run, one listing per directory and one `stat`
    // per file of the package: metadata only.
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_stable_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let name = e.file_name();
        let name = name.to_string_lossy();
        let path = e.path();
        if name.starts_with('.') || (dir == root && name == "build") {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let linked = e.file_type().is_ok_and(|t| t.is_symlink());
        match disk::kind_of(&e) {
            disk::Kind::Dir if !linked => walk(h, root, &path, now, stamps, skip),
            disk::Kind::Dir => {
                h.str(&rel);
                h.str(
                    &std::fs::read_link(&path)
                        .unwrap_or_default()
                        .to_string_lossy(),
                );
            }
            disk::Kind::File if !skip(&rel) => {
                h.str(&rel);
                let stamp = std::fs::metadata(&path)
                    .ok()
                    .and_then(|m| disk::stamp(&m, now));
                h.hash(stamps.hash(&rel, stamp, || {
                    disk::read_regular(&path).unwrap_or_default()
                }));
            }
            // A FIFO, socket or device is never opened.
            disk::Kind::File | disk::Kind::Other => {}
        }
    }
}

/// The source part of the fingerprints of one build: its toolchain and
/// pipeline keys (the `hd` version, std, the engine target and the
/// pipeline), and the text hash of every file it read outside the root
/// package, a dependency's or std's. Texts enter by the hashes the build's
/// skims hold, so no file is read again here.
pub(crate) struct Build<'a> {
    out: &'a Output,
    /// Whether each of the output's files is one of the root package's.
    own: Vec<bool>,
    outside: Hash128,
}

/// The hash of output file `i`'s text: its skim's, else its text's.
fn text_hash(out: &Output, i: usize) -> Hash128 {
    out.source_hashes
        .get(i)
        .copied()
        .flatten()
        .unwrap_or_else(|| hd_base::hash128(out.texts.get(i).map_or("", |t| t).as_bytes()))
}

impl<'a> Build<'a> {
    pub(crate) fn new(out: &'a Output, sources: &disk::DiskSources) -> Build<'a> {
        let listed: hd_base::HashSet<String> = sources.list().into_iter().map(|e| e.path).collect();
        let own: Vec<bool> = out.files.iter().map(|f| listed.contains(f)).collect();
        let mut h = StableHasher::new("hd-test-outside");
        h.hash(out.toolchain);
        h.hash(out.pipeline);
        for (i, f) in out.files.iter().enumerate() {
            if !own[i] {
                h.str(f);
                h.hash(text_hash(out, i));
            }
        }
        Build {
            out,
            own,
            outside: h.finish(),
        }
    }

    /// The source hash of a program that reads the root package's files
    /// `keep` accepts.
    fn hash(&self, keep: &dyn Fn(usize) -> bool) -> Hash128 {
        let mut h = StableHasher::new("hd-test-sources");
        h.hash(self.outside);
        for (i, f) in self.out.files.iter().enumerate() {
            if self.own[i] && keep(i) {
                h.str(f);
                h.hash(text_hash(self.out, i));
            }
        }
        h.finish()
    }

    /// The source hash of the whole build: an integration test file's or a
    /// doc test's program, which reads only its own module and what it
    /// uses (`cli.package.file`).
    pub(crate) fn all(&self) -> Hash128 {
        self.hash(&|_| true)
    }

    /// The source hash of the unit tests of `file`, one module of the
    /// unit-test program: that module and the modules its uses reach, its
    /// `tests:` block's included, as `hd test FILE` builds it
    /// (`cli.package.file`), from the uses the build's skims found.
    /// Implementations live in the module of their trait or their target,
    /// so nothing outside that closure changes what its test cases do.
    pub(crate) fn module(&self, file: &str) -> Hash128 {
        let mut seen = vec![false; self.out.files.len()];
        let mut stack: Vec<usize> = self
            .out
            .files
            .iter()
            .position(|f| f == file)
            .into_iter()
            .collect();
        while let Some(i) = stack.pop() {
            if !std::mem::replace(&mut seen[i], true) {
                stack.extend(self.out.uses.get(i).into_iter().flatten().copied());
            }
        }
        self.hash(&|i| seen[i])
    }
}

/// A test program's fingerprint (`cli.test.fingerprint`): its id, its
/// sources, the files it may read at run time, the package's common
/// inputs, and its test cases as the build lists them.
pub(crate) fn fingerprint(
    common: Hash128,
    id: &str,
    sources: &[Hash128],
    data: Hash128,
    cases: Hash128,
) -> Hash128 {
    let mut h = StableHasher::new("hd-test-fingerprint-v2");
    h.hash(common);
    h.str(id);
    for s in sources {
        h.hash(*s);
    }
    h.hash(data);
    h.hash(cases);
    h.finish()
}

/// What a fingerprint holds of a program's test cases ([`fingerprint`]),
/// which `hd test` stores with the program's build.
pub(crate) fn cases(cases: &[&hd_driver::TestCase]) -> Hash128 {
    let mut h = StableHasher::new("hd-test-cases");
    // What the source says of each case; never its export numbers, which
    // the other modules of the unit-test program shift.
    for c in cases {
        h.str(&c.file);
        h.u32(c.line);
        h.str(&c.name);
        h.str(&c.kind);
        h.str(&format!(
            "{:?}",
            (&c.ignore, &c.expect_panic, &c.unsupported)
        ));
    }
    h.finish()
}

/// Every source file of the package: what a program reads when it starts
/// the package's executables and tasks (`cli.test.process`), each by its
/// content hash (`Stamps::hash`).
pub(crate) fn all_sources(sources: &disk::DiskSources, stamps: &mut Stamps) -> Hash128 {
    let mut h = StableHasher::new("hd-test-all-sources");
    for e in sources.list() {
        h.str(&e.path);
        h.hash(stamps.hash(&sources.display(&e.path), e.stamp, || {
            sources
                .read(&e.path)
                .map(|b| b.to_vec())
                .unwrap_or_default()
        }));
    }
    h.finish()
}
