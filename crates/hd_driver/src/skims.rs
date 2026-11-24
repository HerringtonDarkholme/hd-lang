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

/// A library `use` line past its path: whether it is `pub`, whether it is
/// grouped, and each grouped name with the local name it binds
/// (`use M.{a, b as c}` gives `(a, a)` and `(b, c)`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UseShape {
    pub public: bool,
    pub grouped: bool,
    pub names: Vec<(String, String)>,
}

impl UseShape {
    /// The shape of the `use` line at `lo..hi` of `text` (its range starts
    /// at `use`, after any `pub`). Cost: one lex of the line.
    fn of(text: &str, (lo, hi): Range) -> UseShape {
        let (lo, hi) = (lo as usize, (hi as usize).min(text.len()));
        let line_start = text[..lo].rfind('\n').map_or(0, |i| i + 1);
        let public = text[line_start..lo].trim() == "pub";
        let line = &text[lo..hi];
        let Some(open) = line.find('{') else {
            return UseShape {
                public,
                ..UseShape::default()
            };
        };
        let group = &line[open + 1..];
        let lexed = hd_syntax::lex(group.as_bytes());
        let toks = &lexed.tokens;
        let mut names = Vec::new();
        // Each item is `name` or `name as local`, items split by commas.
        let mut item: Vec<&str> = Vec::new();
        // Whether the item holds a token no well-formed item has.
        let mut malformed = false;
        let mut flush = |item: &mut Vec<&str>, malformed: &mut bool| {
            if !*malformed && let Some(&name) = item.first() {
                let local = if item.len() >= 3 && item[1] == "as" {
                    item[2]
                } else {
                    name
                };
                names.push((name.to_owned(), local.to_owned()));
            }
            item.clear();
            *malformed = false;
        };
        for i in 0..toks.kind.len() {
            let t = &group[toks.start[i] as usize..toks.end[i] as usize];
            match toks.kind[i] {
                hd_syntax::TokenKind::Ident => item.push(t),
                hd_syntax::TokenKind::RawIdent => item.push(t.trim_matches('`')),
                hd_syntax::TokenKind::Comma => flush(&mut item, &mut malformed),
                hd_syntax::TokenKind::RBrace => break,
                // Any other token makes the item malformed, which the
                // parser reports; it names nothing the module graph may
                // follow. The lexer keeps comments and line breaks apart,
                // so they are no tokens here.
                _ => malformed = true,
            }
        }
        flush(&mut item, &mut malformed);
        UseShape {
            public,
            grouped: true,
            names,
        }
    }

    fn write(&self, w: &mut Writer) {
        w.u8(u8::from(self.public) | (u8::from(self.grouped) << 1));
        w.len_of(&self.names);
        for (name, local) in &self.names {
            w.str(name);
            w.str(local);
        }
    }

    fn read(r: &mut Reader<'_>) -> UseShape {
        let flags = r.u8();
        UseShape {
            public: flags & 1 != 0,
            grouped: flags & 2 != 0,
            names: (0..r.count())
                .map(|_| (r.str().to_owned(), r.str().to_owned()))
                .collect(),
        }
    }
}

/// What `hd_syntax::skim` gives a file's text, before its module's uses
/// are resolved against the module table: a function of the text alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSkim {
    pub source_hash: Hash128,
    pub api_text_hash: Hash128,
    /// What checking one of the file's templates reads of it
    /// (`HeaderSkeleton::template_text_hash`).
    pub template_text_hash: Hash128,
    pub token_hash: Hash128,
    /// The hash of the attached documentation, the only comments that
    /// affect generated metadata; plain comments keep cache reuse.
    pub doc_hash: Hash128,
    /// The text's length in bytes, which ends its last code line.
    pub len: u32,
    /// The start of each code line (`HeaderSkeleton::code_lines`).
    pub code_lines: Vec<u32>,
    /// The byte range of each `use` line outside a `tests:` block
    /// (`HeaderSkeleton::uses`): the uses that make unit edges.
    pub use_lines: Vec<(u32, u32)>,
    /// The module path each of `use_lines` names as written, its root not
    /// yet resolved. Kept so a module's uses resolve without its text.
    pub use_paths: Vec<String>,
    /// What each of `use_lines` names past its path ([`UseShape`]): the
    /// module graph follows `pub use` chains through it without the text.
    pub use_shapes: Vec<UseShape>,
    /// The path of each `use` line of a `tests:` block, as written. Test
    /// code makes no unit edge (`module.cycle.test-code`); these widen
    /// only the test role's closure.
    pub test_uses: Vec<String>,
    pub has_tests_block: bool,
    /// Whether the text holds both `@` and `host`, so it may declare a
    /// host trait, which `check_host_declarations` then parses it for.
    pub host_marker: bool,
    /// Whether the text names `derive` or `Structure`, so it may opt in to
    /// a derivation template (`Run::template_sources`).
    pub derives: bool,
    /// Whether the text holds a fence (`` ``` ``), so it may hold a doc
    /// test.
    pub fences: bool,
    /// The header span of a top-level `pub fn main` or `main!`, which
    /// `cli.exe.unselected-main` is about.
    pub public_main: Option<(u32, u32)>,
    /// The header span of a top-level `main` or `main!` that is not
    /// `pub` (`module.entry.private-main.lint`).
    pub private_main: Option<(u32, u32)>,
    /// The offset of each newline, in a text that may register a test case
    /// (it holds `it(` or `it_`): what places a case on its line without
    /// the text (`Run::test_cases`). `None` in any other text.
    pub newlines: Option<Vec<u32>>,
    /// An entry module's [`EntryFacts`], set once a run parses it.
    pub(crate) entry: OnceLock<EntryFacts>,
}

impl FileSkim {
    #[must_use]
    pub fn of(text: &str) -> FileSkim {
        let sk = skim(text.as_bytes());
        let path_of = |&(lo, hi): &(u32, u32)| {
            written_path(text.get(lo as usize..hi as usize).unwrap_or("")).to_owned()
        };
        let test_uses = sk.test_uses.iter().map(path_of).collect();
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
        let use_paths = sk.uses.iter().map(path_of).collect();
        let use_shapes = sk.uses.iter().map(|&r| UseShape::of(text, r)).collect();
        FileSkim {
            source_hash: sk.source_hash,
            api_text_hash: sk.api_text_hash,
            template_text_hash: sk.template_text_hash,
            token_hash: sk.token_hash,
            doc_hash: doc_hash(text),
            len: u32::try_from(text.len()).unwrap_or(u32::MAX),
            has_tests_block: sk.bodies.iter().any(|b| b.kind == HeaderKind::Tests),
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
            use_lines: sk.uses,
            use_paths,
            use_shapes,
            test_uses,
        }
    }

    pub fn write(&self, w: &mut Writer) {
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
        for shape in &self.use_shapes {
            shape.write(w);
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

    pub fn read(r: &mut Reader<'_>) -> FileSkim {
        let source_hash = r.hash();
        let api_text_hash = r.hash();
        let template_text_hash = r.hash();
        let token_hash = r.hash();
        let doc_hash = r.hash();
        let len = r.u32();
        let code_lines = (0..r.count()).map(|_| r.u32()).collect();
        let use_lines: Vec<(u32, u32)> = (0..r.count()).map(|_| (r.u32(), r.u32())).collect();
        let use_paths = use_lines.iter().map(|_| r.str().to_owned()).collect();
        let use_shapes = use_lines.iter().map(|_| UseShape::read(r)).collect();
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
            use_shapes,
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

/// The module path a `use` line (`HeaderSkeleton::uses`, from its `use`)
/// names as written: `use a.b.{c}` and `use a.b as c` name `a.b`.
fn written_path(line: &str) -> &str {
    let rest = line.strip_prefix("use").unwrap_or(line);
    let path = rest.split(".{").next().unwrap_or(rest);
    path.split(" as ").next().unwrap_or(path).trim()
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
    /// The run missed the entry and recorded it at once, for the runs
    /// waiting for its claim (#1932). Still a miss; nothing to record at
    /// the end.
    early: std::sync::atomic::AtomicBool,
}

impl<'a> StdSkims<'a> {
    pub(crate) fn new(store: &'a dyn CacheStore, toolchain: Hash128) -> Self {
        StdSkims {
            store,
            toolchain,
            key: hd_cache::depfiles_key(toolchain, "std"),
            table: OnceLock::new(),
            early: std::sync::atomic::AtomicBool::new(false),
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
        let Some(bytes) = self.store.get_claimed(EntryKind::Depfiles, &self.key) else {
            return self.skim_all();
        };
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

    /// On a miss that this run claimed, other runs wait for the entry:
    /// skims every std file now and records it at once, so they read it
    /// rather than skim std too. A run alone claims nothing and skims each
    /// std file as it reads it. Cost: the skims the miss's record at the
    /// end would make anyway, made first.
    fn skim_all(&self) -> Option<HashMap<String, Arc<FileSkim>>> {
        if !self.store.claims(EntryKind::Depfiles, &self.key) {
            return None;
        }
        let std = &super::embedded_std().sources;
        let table: HashMap<String, Arc<FileSkim>> = std
            .list()
            .iter()
            .map(|e| {
                let text = std.read(&e.path).unwrap_or_else(|| Arc::from(&b""[..]));
                let skim = FileSkim::of(&String::from_utf8_lossy(&text));
                (e.path.clone(), Arc::new(skim))
            })
            .collect();
        let known: HashMap<&str, &FileSkim> =
            table.iter().map(|(p, s)| (p.as_str(), &**s)).collect();
        self.record(std, &known);
        self.early.store(true, std::sync::atomic::Ordering::Relaxed);
        drop(known);
        Some(table)
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
        let read = std.table.get()?.is_some();
        if !read {
            let known: HashMap<&str, &FileSkim> = (0..self.cells.len())
                .filter(|&m| is_std(table, m))
                .filter_map(|m| Some((table.sources[m].1.as_str(), &**self.cells[m].get()?)))
                .collect();
            std.record(&super::embedded_std().sources, &known);
        }
        Some(read && !std.early.load(std::sync::atomic::Ordering::Relaxed))
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
    use super::UseShape;

    fn names(line: &str) -> Vec<(String, String)> {
        let at = u32::try_from(line.find("use").expect("a use line")).expect("short");
        let end = u32::try_from(line.len()).expect("short");
        UseShape::of(line, (at, end)).names
    }

    /// A grouped use's names with the local names they bind; an item
    /// holding a token no well-formed item has names nothing.
    #[test]
    fn grouped_names_and_malformed_items() {
        let pair = |n: &str, l: &str| (n.to_owned(), l.to_owned());
        assert_eq!(
            names("use pkg.shop.{a, b as c, `d`,}"),
            [pair("a", "a"), pair("b", "c"), pair("d", "d")]
        );
        assert_eq!(
            names("use pkg.shop.{a, b.c, d}"),
            [pair("a", "a"), pair("d", "d")]
        );
        assert_eq!(
            names("use pkg.shop.{a, b"),
            [pair("a", "a"), pair("b", "b")]
        );
    }
}
