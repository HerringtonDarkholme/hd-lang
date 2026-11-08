//! The text half of a module's skim ([`FileSkim`]), made once per run, and
//! the embedded std's, kept per toolchain in a `depfiles` entry
//! ([`StdSkims`], `crates/hd_cache/DESIGN.md` §5.5 rule 7). Std is about 500 KB of text: on a
//! warm run its skims were most of the run's time (#49).

use hd_base::HashMap;
use hd_base::SliceSort as _;
use std::sync::{Arc, OnceLock};

use hd_base::Hash128;
use hd_base::wire::{Reader, Writer};
use hd_cache::{CacheStore, EntryKind};
use hd_project::{ModuleTable, SourceSet};
use hd_syntax::{HeaderKind, skim};

/// A byte range of a text: start and end.
type Range = (u32, u32);

/// The spans an entry module's capability rows are reported at
/// (`Run::entry_capabilities`), from a full parse: a function of the text
/// and the module path, kept with the skim so a warm run does not parse
/// the entry module again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct EntryFacts {
    /// The top-level `main`'s header and its requirement row, if written.
    pub(crate) main: Option<(Range, Option<Range>)>,
    /// The first top-level statement, a task's init.
    pub(crate) init: Option<(u32, u32)>,
}

/// What `hd_syntax::skim` gives a file's text, before its module's uses
/// are resolved against the module table: a function of the text alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileSkim {
    pub(crate) source_hash: Hash128,
    pub(crate) api_text_hash: Hash128,
    /// What checking one of the file's templates reads of it
    /// (`HeaderSkeleton::template_text_hash`).
    pub(crate) template_text_hash: Hash128,
    pub(crate) token_hash: Hash128,
    /// The hash of the attached documentation, the only comments that
    /// affect generated metadata; plain comments keep cache reuse.
    pub(crate) doc_hash: Hash128,
    /// The text's length in bytes, which ends its last code line.
    pub(crate) len: u32,
    /// The start of each code line (`HeaderSkeleton::code_lines`).
    pub(crate) code_lines: Vec<u32>,
    /// The byte range of each `use` line outside a `tests:` block
    /// (`HeaderSkeleton::uses`): the uses that make folder edges.
    pub(crate) use_lines: Vec<(u32, u32)>,
    /// The module path each of `use_lines` names as written, its root not
    /// yet resolved; empty for a line that is no `use`. Kept so a module's
    /// uses resolve without its text.
    pub(crate) use_paths: Vec<String>,
    /// The path of each `use` line of a `tests:` block, as written. Test
    /// code makes no folder edge (`module.cycle.test-code`); these widen
    /// only the test role's closure.
    pub(crate) test_uses: Vec<String>,
    pub(crate) has_tests_block: bool,
    /// Whether the text holds both `@` and `host`, so it may declare a
    /// host trait, which `check_host_declarations` then parses it for.
    pub(crate) host_marker: bool,
    /// Whether the text names `derive` or `Structure`, so it may opt in to
    /// a derivation template (`Run::template_sources`).
    pub(crate) derives: bool,
    /// Whether the text holds a fence (`` ``` ``), so it may hold a doc
    /// test (`doc_test_files`).
    pub(crate) fences: bool,
    /// The header span of a top-level `pub fn main` or `main!`, which
    /// `cli.exe.unselected-main` is about.
    pub(crate) public_main: Option<(u32, u32)>,
    /// The header span of a top-level `main` or `main!` that is not
    /// `pub` (`module.entry.private-main.lint`).
    pub(crate) private_main: Option<(u32, u32)>,
    /// The offset of each newline, in a text that may register a test case
    /// (it holds `it(` or `it_`): what places a case on its line without
    /// the text (`Run::test_cases`). `None` in any other text.
    pub(crate) newlines: Option<Vec<u32>>,
    /// An entry module's [`EntryFacts`], set once a run parses it.
    pub(crate) entry: OnceLock<EntryFacts>,
}

impl FileSkim {
    pub(crate) fn of(text: &str) -> FileSkim {
        let sk = skim(text.as_bytes());
        let tests = || sk.bodies.iter().filter(|b| b.kind == HeaderKind::Tests);
        let in_tests = |at: u32| tests().any(|b| (b.body_start..b.body_end).contains(&at));
        let mut test_uses = Vec::new();
        for b in tests() {
            let block = text
                .get(b.body_start as usize..b.body_end as usize)
                .unwrap_or("");
            for line in block.lines() {
                let Some(rest) = line.trim().strip_prefix("use ") else {
                    continue;
                };
                let path = rest.split(".{").next().unwrap_or(rest);
                let path = path.split(" as ").next().unwrap_or(path).trim();
                test_uses.push(path.to_owned());
            }
        }
        let main_header = |public: bool| {
            sk.bodies
                .iter()
                .filter(|b| b.kind == HeaderKind::Function && b.header_indent == 0)
                .find(|b| {
                    let header = text
                        .get(b.header_start as usize..b.body_start as usize)
                        .unwrap_or("");
                    super::main_visibility(header) == Some(public)
                })
                .map(|b| (b.header_start, b.body_start))
        };
        let use_lines: Vec<(u32, u32)> = sk
            .uses
            .iter()
            .copied()
            .filter(|&(lo, _)| !in_tests(lo))
            .collect();
        let use_paths = use_lines
            .iter()
            .map(|&(lo, hi)| {
                written_path(text.get(lo as usize..hi as usize).unwrap_or(""))
                    .unwrap_or("")
                    .to_owned()
            })
            .collect();
        FileSkim {
            source_hash: sk.source_hash,
            api_text_hash: sk.api_text_hash,
            template_text_hash: sk.template_text_hash,
            token_hash: sk.token_hash,
            doc_hash: doc_hash(text),
            len: u32::try_from(text.len()).unwrap_or(u32::MAX),
            has_tests_block: tests().next().is_some(),
            host_marker: text.contains('@') && text.contains("host"),
            derives: text.contains("derive") || text.contains("Structure"),
            fences: text.contains("```"),
            public_main: main_header(true),
            private_main: main_header(false),
            newlines: (text.contains("it(") || text.contains("it_")).then(|| {
                text.bytes()
                    .enumerate()
                    .filter(|&(_, b)| b == b'\n')
                    .map(|(i, _)| u32::try_from(i).unwrap_or(u32::MAX))
                    .collect()
            }),
            entry: OnceLock::new(),
            code_lines: sk.code_lines,
            use_lines,
            use_paths,
            test_uses,
        }
    }

    fn write(&self, w: &mut Writer) {
        w.hash(self.source_hash);
        w.hash(self.api_text_hash);
        w.hash(self.template_text_hash);
        w.hash(self.token_hash);
        w.hash(self.doc_hash);
        w.u32(self.len);
        w.len_of(&self.code_lines);
        for &l in &self.code_lines {
            w.u32(l);
        }
        w.len_of(&self.use_lines);
        for &(lo, hi) in &self.use_lines {
            w.u32(lo);
            w.u32(hi);
        }
        for p in &self.use_paths {
            w.str(p);
        }
        w.len_of(&self.test_uses);
        for u in &self.test_uses {
            w.str(u);
        }
        w.u8(u8::from(self.has_tests_block));
        w.u8(u8::from(self.host_marker));
        w.u8(u8::from(self.derives));
        w.u8(u8::from(self.fences));
        let span = |w: &mut Writer, span: Option<(u32, u32)>| match span {
            Some((lo, hi)) => {
                w.u8(1);
                w.u32(lo);
                w.u32(hi);
            }
            None => w.u8(0),
        };
        span(w, self.public_main);
        span(w, self.private_main);
        match &self.newlines {
            Some(lines) => {
                w.u8(1);
                w.len_of(lines);
                for &l in lines {
                    w.u32(l);
                }
            }
            None => w.u8(0),
        }
        match self.entry.get() {
            Some(facts) => {
                w.u8(1);
                span(w, facts.main.map(|(head, _)| head));
                span(w, facts.main.and_then(|(_, row)| row));
                span(w, facts.init);
            }
            None => w.u8(0),
        }
    }

    fn read(r: &mut Reader<'_>) -> FileSkim {
        let source_hash = r.hash();
        let api_text_hash = r.hash();
        let template_text_hash = r.hash();
        let token_hash = r.hash();
        let doc_hash = r.hash();
        let len = r.u32();
        let code_lines = (0..r.count()).map(|_| r.u32()).collect();
        let use_lines: Vec<(u32, u32)> = (0..r.count()).map(|_| (r.u32(), r.u32())).collect();
        let use_paths = use_lines.iter().map(|_| r.str().to_owned()).collect();
        let test_uses = (0..r.count()).map(|_| r.str().to_owned()).collect();
        let has_tests_block = r.u8() != 0;
        let host_marker = r.u8() != 0;
        let derives = r.u8() != 0;
        let fences = r.u8() != 0;
        let span = |r: &mut Reader<'_>| (r.u8() != 0).then(|| (r.u32(), r.u32()));
        let public_main = span(r);
        let private_main = span(r);
        let newlines = (r.u8() != 0).then(|| (0..r.count()).map(|_| r.u32()).collect());
        let entry = OnceLock::new();
        if r.u8() != 0 {
            let head = span(r);
            let row = span(r);
            let init = span(r);
            let _ = entry.set(EntryFacts {
                main: head.map(|h| (h, row)),
                init,
            });
        }
        FileSkim {
            source_hash,
            api_text_hash,
            template_text_hash,
            token_hash,
            doc_hash,
            len,
            code_lines,
            use_lines,
            use_paths,
            test_uses,
            has_tests_block,
            host_marker,
            derives,
            fences,
            public_main,
            private_main,
            newlines,
            entry,
        }
    }
}

/// The module path a `use` line names as written (`pub use a.b.{c}` names
/// `a.b`), or `None` for a line that is no `use`.
pub(crate) fn written_path(line: &str) -> Option<&str> {
    let line = line.trim();
    let rest = line.strip_prefix("pub ").unwrap_or(line);
    let rest = rest.strip_prefix("use ")?;
    let path = rest.split(".{").next().unwrap_or(rest);
    Some(path.split(" as ").next().unwrap_or(path).trim())
}

/// The hash of `text`'s attached documentation: each doc comment's text
/// and the token it documents.
fn doc_hash(text: &str) -> Hash128 {
    let mut key = hd_base::StableHasher::new("own-docs-v1");
    if text.contains("##") {
        let lexed = hd_syntax::lex(text.as_bytes());
        let toks = &lexed.tokens;
        for (i, kind) in toks.com_kind.iter().enumerate() {
            if *kind != hd_syntax::CommentKind::Doc {
                continue;
            }
            let token = toks.start.partition_point(|start| *start < toks.com_end[i]);
            let token = u32::try_from(token).expect("token index");
            let doc = hd_syntax::doc_text(text, toks, hd_base::TokenIdx::from_raw(token));
            if !doc.is_empty() {
                key.u32(token);
                key.str(&doc);
            }
        }
    }
    key.finish()
}

/// The embedded std's file skims, by path, from the `depfiles` entry of
/// the toolchain. Std is embedded, so its texts are part of the toolchain
/// key and its skims never change under it. Without the entry each std
/// file is skimmed as any other, and the run records the entry.
pub(crate) struct StdSkims<'a> {
    store: &'a dyn CacheStore,
    toolchain: Hash128,
    key: Hash128,
    /// The entry's skims once read; `None` on a miss.
    table: OnceLock<Option<HashMap<String, Arc<FileSkim>>>>,
}

impl<'a> StdSkims<'a> {
    pub(crate) fn new(store: &'a dyn CacheStore, toolchain: Hash128) -> Self {
        StdSkims {
            store,
            toolchain,
            key: hd_cache::depfiles_key(toolchain, "std"),
            table: OnceLock::new(),
        }
    }

    fn get(&self, path: &str) -> Option<Arc<FileSkim>> {
        self.table
            .get_or_init(|| self.read())
            .as_ref()?
            .get(path)
            .cloned()
    }

    fn read(&self) -> Option<HashMap<String, Arc<FileSkim>>> {
        let bytes = self.store.get(EntryKind::Depfiles, &self.key)?;
        let (h, sections) = hd_cache::decode_entry(&bytes).ok()?;
        if h.key != self.key || h.toolchain != self.toolchain || h.layout != super::LAYOUT {
            return None;
        }
        let mut r = Reader::new(sections.first()?.1);
        let n = r.count();
        let mut table = HashMap::with_capacity_and_hasher(n, hd_base::FixedState::default());
        for _ in 0..n {
            let path = r.str().to_owned();
            table.insert(path, Arc::new(FileSkim::read(&mut r)));
        }
        (r.ok() && r.at_end()).then_some(table)
    }

    /// Records the entry after a miss: each file of `std`, with the skim
    /// in `known` when the run made one, else its own.
    fn record(&self, std: &dyn SourceSet, known: &HashMap<&str, &FileSkim>) {
        let files = std.list();
        let mut w = Writer::default();
        w.len_of(&files);
        for e in &files {
            w.str(&e.path);
            if let Some(s) = known.get(e.path.as_str()) {
                s.write(&mut w);
            } else {
                let text = std.read(&e.path).unwrap_or_else(|| Arc::from(&b""[..]));
                FileSkim::of(&String::from_utf8_lossy(&text)).write(&mut w);
            }
        }
        let bytes = hd_cache::encode_entry(
            EntryKind::Depfiles,
            super::LAYOUT,
            self.key,
            self.toolchain,
            0,
            &[(0, 0, &w.bytes)],
        );
        self.store.put(EntryKind::Depfiles, &self.key, &bytes);
    }
}

/// A record's rows: per path, the stamp and the skim.
type RecordRows = HashMap<String, (Hash128, Arc<FileSkim>)>;

/// A source set's last-run record (`crates/hd_cache/DESIGN.md` §5.5): per
/// file listed with a stamp, the stamp and the file's skim, in one
/// `depfiles` entry keyed by the toolchain and the set's identity. A file
/// whose stamp the record holds is not read to be skimmed.
struct SourceRecord {
    key: Hash128,
    /// The entry's files once read; `None` on a miss.
    table: OnceLock<Option<RecordRows>>,
    /// Whether this run skimmed a stamped file the record did not hold.
    fresh: std::sync::atomic::AtomicBool,
    /// Whether the run lists only part of the set's files
    /// (`SourceSet::partial`).
    partial: bool,
}

impl SourceRecord {
    fn files(&self, store: &dyn CacheStore, toolchain: Hash128) -> Option<&RecordRows> {
        self.table
            .get_or_init(|| {
                let bytes = store.get(EntryKind::Depfiles, &self.key)?;
                let (h, sections) = hd_cache::decode_entry(&bytes).ok()?;
                if h.key != self.key || h.toolchain != toolchain || h.layout != super::LAYOUT {
                    return None;
                }
                let mut r = Reader::new(sections.first()?.1);
                let n = r.count();
                let mut table =
                    HashMap::with_capacity_and_hasher(n, hd_base::FixedState::default());
                for _ in 0..n {
                    let path = r.str().to_owned();
                    let stamp = r.hash();
                    table.insert(path, (stamp, Arc::new(FileSkim::read(&mut r))));
                }
                (r.ok() && r.at_end()).then_some(table)
            })
            .as_ref()
    }
}

/// Each module's [`FileSkim`], made once per run: a std module's from the
/// `depfiles` entry when std is the embedded one, and an unchanged file's
/// from its source set's last-run record.
pub(crate) struct FileSkims<'a> {
    store: &'a dyn CacheStore,
    toolchain: Hash128,
    /// `None` when std is the root package, whose files are the user's.
    std: Option<StdSkims<'a>>,
    /// Per package, the record of its source set, when it has an identity.
    records: Vec<Option<SourceRecord>>,
    cells: Vec<OnceLock<Arc<FileSkim>>>,
}

impl<'a> FileSkims<'a> {
    /// `identities` holds each package's source-set identity
    /// (`SourceSet::identity`) and whether its listing is partial.
    pub(crate) fn new(
        store: &'a dyn CacheStore,
        toolchain: Hash128,
        std: Option<StdSkims<'a>>,
        identities: &[Option<(Hash128, bool)>],
        modules: usize,
    ) -> Self {
        FileSkims {
            store,
            toolchain,
            std,
            records: identities
                .iter()
                .map(|id| {
                    id.map(|(id, partial)| SourceRecord {
                        key: hd_cache::depfiles_key(toolchain, &format!("src:{:032x}", id.0)),
                        table: OnceLock::new(),
                        fresh: std::sync::atomic::AtomicBool::new(false),
                        partial,
                    })
                })
                .collect(),
            cells: (0..modules).map(|_| OnceLock::new()).collect(),
        }
    }

    /// Module `m`'s skim; `text` gives its text, read only when neither
    /// std's entry nor its set's record holds the skim.
    pub(crate) fn get(
        &self,
        table: &ModuleTable,
        m: usize,
        text: impl FnOnce() -> Arc<str>,
    ) -> Arc<FileSkim> {
        Arc::clone(self.cells[m].get_or_init(|| {
            let path = &table.sources[m].1;
            if let Some(s) = self.std.as_ref().filter(|_| is_std(table, m))
                && let Some(skim) = s.get(path)
            {
                return skim;
            }
            let record = self.records[usize::from(table.modules[m].package)].as_ref();
            if let (Some(record), Some(stamp)) = (record, table.stamps[m]) {
                let known = record
                    .files(self.store, self.toolchain)
                    .and_then(|files| files.get(path))
                    .filter(|(s, _)| *s == stamp);
                if let Some((_, skim)) = known {
                    return Arc::clone(skim);
                }
                record
                    .fresh
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            Arc::new(FileSkim::of(&text()))
        }))
    }

    /// Notes that module `m`'s skim gained facts this run
    /// ([`FileSkim::entry`]), so its set's record is written again.
    pub(crate) fn gained(&self, table: &ModuleTable, m: usize) {
        if table.stamps[m].is_some()
            && let Some(record) = &self.records[usize::from(table.modules[m].package)]
        {
            record
                .fresh
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// After the run: records the `depfiles` entry if it was missed, and
    /// says whether it was read (`Some(true)` on a hit), or `None` when the
    /// run skimmed no std module. Writes each source set's record again
    /// when its files changed.
    pub(crate) fn finish(&self, table: &ModuleTable) -> Option<bool> {
        for (p, record) in self.records.iter().enumerate() {
            if let Some(record) = record {
                self.record_package(table, p, record);
            }
        }
        let std = self.std.as_ref()?;
        let hit = std.table.get()?.is_some();
        if !hit {
            let known: HashMap<&str, &FileSkim> = (0..self.cells.len())
                .filter(|&m| is_std(table, m))
                .filter_map(|m| Some((table.sources[m].1.as_str(), &**self.cells[m].get()?)))
                .collect();
            std.record(&super::embedded_std().sources, &known);
        }
        Some(hit)
    }

    /// Writes package `p`'s record when this run skimmed a stamped file it
    /// did not hold or a file it holds is gone: each stamped module's skim,
    /// this run's or the old record's for a module the run did not skim.
    /// A module listed without a stamp is left out, so it is read again.
    /// A partial listing keeps the old rows of the files it does not list.
    /// Cost: one write of every stamped file's skim, only on a run that
    /// changed the package's files.
    fn record_package(&self, table: &ModuleTable, p: usize, record: &SourceRecord) {
        let old = record.files(self.store, self.toolchain);
        let mut rows: Vec<(&str, Hash128, &FileSkim)> = Vec::new();
        for m in (0..table.modules.len()).filter(|&m| usize::from(table.modules[m].package) == p) {
            let Some(stamp) = table.stamps[m] else {
                continue;
            };
            let path = table.sources[m].1.as_str();
            let skim = self.cells[m].get().map(|s| &**s).or_else(|| {
                old.and_then(|o| o.get(path))
                    .filter(|(s, _)| *s == stamp)
                    .map(|(_, s)| &**s)
            });
            if let Some(skim) = skim {
                rows.push((path, stamp, skim));
            }
        }
        if record.partial
            && let Some(old) = old
        {
            let listed: hd_base::HashSet<&str> = (0..table.modules.len())
                .filter(|&m| usize::from(table.modules[m].package) == p)
                .map(|m| table.sources[m].1.as_str())
                .collect();
            rows.extend(
                old.iter()
                    .filter(|(path, _)| !listed.contains(path.as_str()))
                    .map(|(path, (stamp, skim))| (path.as_str(), *stamp, &**skim)),
            );
            rows.sort_stable_by(|a, b| a.0.cmp(b.0));
        }
        let fresh = record.fresh.load(std::sync::atomic::Ordering::Relaxed);
        if !fresh && old.is_some_and(|o| o.len() == rows.len()) {
            return;
        }
        let mut w = Writer::default();
        w.len_of(&rows);
        for (path, stamp, skim) in &rows {
            w.str(path);
            w.hash(*stamp);
            skim.write(&mut w);
        }
        let bytes = hd_cache::encode_entry(
            EntryKind::Depfiles,
            super::LAYOUT,
            record.key,
            self.toolchain,
            0,
            &[(0, 0, &w.bytes)],
        );
        self.store.put(EntryKind::Depfiles, &record.key, &bytes);
    }
}

/// Whether module `m` is std's: std is the table's last package.
fn is_std(table: &ModuleTable, m: usize) -> bool {
    usize::from(table.modules[m].package) + 1 == table.packages.len()
}

#[cfg(test)]
mod tests {
    use super::FileSkim;
    use hd_base::wire::{Reader, Writer};

    #[test]
    fn a_file_skim_round_trips() {
        let text = "use std.text\n\npub fn main() -> void:\n    # note\n    println(\"hi\")\n\ntests:\n    use pkg.other as o\n    test \"a\":\n        assert(true)\n";
        let s = FileSkim::of(text);
        assert!(s.has_tests_block && s.public_main.is_some() && s.private_main.is_none());
        assert_eq!(s.test_uses, ["pkg.other"]);
        // The block's `use` line is no folder-edge line.
        let lines: Vec<&str> = s
            .use_lines
            .iter()
            .map(|&(lo, hi)| &text[lo as usize..hi as usize])
            .filter(|l| l.starts_with("use "))
            .collect();
        assert_eq!(lines, ["use std.text"]);
        let mut w = Writer::default();
        s.write(&mut w);
        let mut r = Reader::new(&w.bytes);
        assert_eq!(FileSkim::read(&mut r), s);
        assert!(r.ok() && r.at_end());
        let mut short = Reader::new(&w.bytes[..w.bytes.len() - 3]);
        let _ = FileSkim::read(&mut short);
        assert!(!short.ok());
    }
}
