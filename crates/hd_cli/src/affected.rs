//! `hd test --affected` (spec/cli/command-line.md, "Affected Tests"): each
//! test program's fingerprint (`cli.test.fingerprint`), and the records of
//! the fingerprints of the programs that last passed in full
//! (`cli.test.affected.record`), kept in the build directory
//! (`cli.test.affected.where`).
//!
//! A fingerprint must change whenever a change could change the program's
//! outcome, so it errs on the side of including an input: a stale skip
//! hides a failure, while an extra run only costs time.

use std::path::{Path, PathBuf};

use hd_base::{Hash128, StableHasher};
use hd_driver::Output;
use hd_project::{MemorySources, SourceSet as _};

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
    // The manifest settings, `[test.capabilities]` among them, and the
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
    vars.sort();
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
    match std::fs::read(path) {
        Ok(b) => {
            h.u8(1);
            h.bytes(&b);
        }
        Err(_) => h.u8(0),
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

pub(crate) fn data(root: &Path, sources: &disk::DiskSources, single_file: bool) -> Data {
    let mut h = StableHasher::new("hd-test-snapshots");
    for dir in ["__snapshots__", "__regressions__"] {
        walk(&mut h, root, &root.join(dir), &|_| false);
    }
    let unit = h.finish();
    // Outside a package the directory is the FILE's, which may hold
    // anything; such a run keeps no records (`cli.test.affected.where`),
    // so that directory is never walked.
    let package = if single_file {
        unit
    } else {
        let listed: std::collections::HashSet<String> = sources
            .list()
            .into_iter()
            .map(|e| sources.display(&e.path))
            .collect();
        let mut h = StableHasher::new("hd-test-package-data");
        walk(&mut h, root, root, &|rel| listed.contains(rel));
        h.finish()
    };
    Data { unit, package }
}

/// Hashes the files below `dir`, in path order, by their path relative to
/// `root` and their bytes, skipping the build directory, hidden entries
/// and the files `skip` names. A symbolic link to a directory is hashed by
/// its target's path, so a loop of links ends.
fn walk(h: &mut StableHasher, root: &Path, dir: &Path, skip: &dyn Fn(&str) -> bool) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
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
        let Ok(kind) = e.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(h, root, &path, skip);
        } else if kind.is_symlink() && path.is_dir() {
            h.str(&rel);
            h.str(
                &std::fs::read_link(&path)
                    .unwrap_or_default()
                    .to_string_lossy(),
            );
        } else if !skip(&rel) {
            h.str(&rel);
            h.bytes(&std::fs::read(&path).unwrap_or_default());
        }
    }
}

/// The source part of the fingerprints of one build: its toolchain and
/// pipeline keys (the `hd` version, std, the engine target and the
/// pipeline), and the text of every file it read outside the root package,
/// a dependency's or std's.
pub(crate) struct Build<'a> {
    out: &'a Output,
    /// Whether each of the output's files is one of the root package's.
    own: Vec<bool>,
    /// The root package's files as the build read them, which a module's
    /// uses are found in.
    texts: MemorySources,
    outside: Hash128,
}

impl<'a> Build<'a> {
    pub(crate) fn new(out: &'a Output, sources: &disk::DiskSources) -> Build<'a> {
        let listed: std::collections::HashSet<String> =
            sources.list().into_iter().map(|e| e.path).collect();
        let own: Vec<bool> = out.files.iter().map(|f| listed.contains(f)).collect();
        let mut texts = MemorySources::default();
        let mut h = StableHasher::new("hd-test-outside");
        h.hash(out.toolchain);
        h.hash(out.pipeline);
        for (i, f) in out.files.iter().enumerate() {
            let text = out.texts.get(i).map_or("", |t| t);
            if own[i] {
                texts.insert(f, text);
            } else {
                h.str(f);
                h.str(text);
            }
        }
        Build {
            out,
            own,
            texts,
            outside: h.finish(),
        }
    }

    /// The source hash of a program that reads the root package's files
    /// `keep` accepts.
    fn hash(&self, keep: &dyn Fn(&str) -> bool) -> Hash128 {
        let mut h = StableHasher::new("hd-test-sources");
        h.hash(self.outside);
        for (i, f) in self.out.files.iter().enumerate() {
            if self.own[i] && keep(f) {
                h.str(f);
                h.str(self.out.texts.get(i).map_or("", |t| t));
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
    /// unit-test program: that module and the modules its uses reach, as
    /// `hd test FILE` builds it (`cli.package.file`). Implementations live
    /// in the module of their trait or their target, so nothing outside
    /// that closure changes what its test cases do.
    pub(crate) fn module(&self, package: &str, file: &str) -> Hash128 {
        let closure = hd_driver::use_closure(package, &self.texts, file);
        self.hash(&|f| closure.iter().any(|c| c == f))
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
    cases: &[&hd_driver::TestCase],
) -> Hash128 {
    let mut h = StableHasher::new("hd-test-fingerprint");
    h.hash(common);
    h.str(id);
    for s in sources {
        h.hash(*s);
    }
    h.hash(data);
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
/// the package's executables and tasks (`cli.test.process`).
pub(crate) fn all_sources(sources: &disk::DiskSources) -> Hash128 {
    let mut h = StableHasher::new("hd-test-all-sources");
    for e in sources.list() {
        h.str(&e.path);
        h.bytes(&sources.read(&e.path).unwrap_or_default());
    }
    h.finish()
}
