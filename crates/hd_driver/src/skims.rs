//! The text half of a module's skim ([`FileSkim`]), made once per run, and
//! the embedded std's, kept per toolchain in a `depfiles` entry
//! ([`StdSkims`], `crates/hd_cache/DESIGN.md` §5.5 rule 7). Std is about 500 KB of text: on a
//! warm run its skims were most of the run's time (#49).

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use hd_base::Hash128;
use hd_base::wire::{Reader, Writer};
use hd_cache::{CacheStore, EntryKind};
use hd_project::{ModuleTable, SourceSet};
use hd_syntax::{HeaderKind, skim};

/// What `hd_syntax::skim` gives a file's text, before its module's uses
/// are resolved against the module table: a function of the text alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileSkim {
    pub(crate) source_hash: Hash128,
    pub(crate) api_text_hash: Hash128,
    pub(crate) token_hash: Hash128,
    /// The start of each code line (`HeaderSkeleton::code_lines`).
    pub(crate) code_lines: Vec<u32>,
    /// The byte range of each `use` line outside a `tests:` block
    /// (`HeaderSkeleton::uses`): the uses that make folder edges.
    pub(crate) use_lines: Vec<(u32, u32)>,
    /// The path of each `use` line of a `tests:` block, as written. Test
    /// code makes no folder edge (`module.cycle.test-code`); these widen
    /// only the test role's closure.
    pub(crate) test_uses: Vec<String>,
    pub(crate) has_tests_block: bool,
    /// The header span of a top-level `pub fn main` or `main!`, which
    /// `cli.exe.unselected-main` is about.
    pub(crate) public_main: Option<(u32, u32)>,
    /// The header span of a top-level `main` or `main!` that is not
    /// `pub` (`module.entry.private-main.warn`).
    pub(crate) private_main: Option<(u32, u32)>,
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
        FileSkim {
            source_hash: sk.source_hash,
            api_text_hash: sk.api_text_hash,
            token_hash: sk.token_hash,
            has_tests_block: tests().next().is_some(),
            public_main: main_header(true),
            private_main: main_header(false),
            code_lines: sk.code_lines,
            use_lines: sk
                .uses
                .iter()
                .copied()
                .filter(|&(lo, _)| !in_tests(lo))
                .collect(),
            test_uses,
        }
    }

    fn write(&self, w: &mut Writer) {
        w.hash(self.source_hash);
        w.hash(self.api_text_hash);
        w.hash(self.token_hash);
        w.len_of(&self.code_lines);
        for &l in &self.code_lines {
            w.u32(l);
        }
        w.len_of(&self.use_lines);
        for &(lo, hi) in &self.use_lines {
            w.u32(lo);
            w.u32(hi);
        }
        w.len_of(&self.test_uses);
        for u in &self.test_uses {
            w.str(u);
        }
        w.u8(u8::from(self.has_tests_block));
        for span in [self.public_main, self.private_main] {
            match span {
                Some((lo, hi)) => {
                    w.u8(1);
                    w.u32(lo);
                    w.u32(hi);
                }
                None => w.u8(0),
            }
        }
    }

    fn read(r: &mut Reader<'_>) -> FileSkim {
        let source_hash = r.hash();
        let api_text_hash = r.hash();
        let token_hash = r.hash();
        let code_lines = (0..r.count()).map(|_| r.u32()).collect();
        let use_lines = (0..r.count()).map(|_| (r.u32(), r.u32())).collect();
        let test_uses = (0..r.count()).map(|_| r.str().to_owned()).collect();
        let has_tests_block = r.u8() != 0;
        let mut span = || (r.u8() != 0).then(|| (r.u32(), r.u32()));
        let public_main = span();
        let private_main = span();
        FileSkim {
            source_hash,
            api_text_hash,
            token_hash,
            code_lines,
            use_lines,
            test_uses,
            has_tests_block,
            public_main,
            private_main,
        }
    }
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
        let mut table = HashMap::with_capacity(n);
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

/// Each module's [`FileSkim`], made once per run: a std module's from the
/// `depfiles` entry when std is the embedded one.
pub(crate) struct FileSkims<'a> {
    /// `None` when std is the root package, whose files are the user's.
    std: Option<StdSkims<'a>>,
    cells: Vec<OnceLock<Arc<FileSkim>>>,
}

impl<'a> FileSkims<'a> {
    pub(crate) fn new(std: Option<StdSkims<'a>>, modules: usize) -> Self {
        FileSkims {
            std,
            cells: (0..modules).map(|_| OnceLock::new()).collect(),
        }
    }

    /// Module `m`'s skim, `text` being its text.
    pub(crate) fn get(&self, table: &ModuleTable, m: usize, text: &str) -> Arc<FileSkim> {
        Arc::clone(self.cells[m].get_or_init(|| {
            self.std
                .as_ref()
                .filter(|_| is_std(table, m))
                .and_then(|s| s.get(&table.sources[m].1))
                .unwrap_or_else(|| Arc::new(FileSkim::of(text)))
        }))
    }

    /// After the run: records the `depfiles` entry if it was missed, and
    /// says whether it was read (`Some(true)` on a hit), or `None` when the
    /// run skimmed no std module.
    pub(crate) fn finish(&self, table: &ModuleTable) -> Option<bool> {
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
