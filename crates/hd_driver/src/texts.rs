//! Module texts, read on first use (`crates/hd_cache/DESIGN.md` §5.5). A
//! warm run takes each unchanged file's skim from its source set's
//! last-run record, so it reads the text only of a module whose checking
//! or diagnostics it needs. The output keeps the same cells: a text the
//! run read is never read again, and one it did not read is read when a
//! caller (a diagnostic's position, a panic's line) asks for it.

use std::sync::{Arc, OnceLock};

use hd_base::HashMap;
use hd_project::SourceSet;

/// Where one package's texts come from.
#[derive(Clone)]
enum Origin<'a> {
    /// The set the run was given.
    Set(&'a dyn SourceSet),
    /// A handle that outlives the run (`SourceSet::detach`).
    Detached(Arc<dyn SourceSet + Send>),
    /// The embedded std, whose texts are made once per process.
    Embedded(&'static HashMap<String, Arc<str>>),
    /// None: the text of a module the run did not read is unknown.
    Gone,
}

/// Per module, its text: read on first use through its package's source
/// set, or given up front.
#[derive(Clone)]
pub struct Texts<'a> {
    cells: Arc<[OnceLock<Arc<str>>]>,
    /// Per module, its text's line index, built on first use.
    lines: Arc<[OnceLock<LineIndex>]>,
    /// Per module, its package and its path in that package's set.
    sources: Arc<[(u16, String)]>,
    /// Per package, where its texts come from.
    from: Arc<[Origin<'a>]>,
}

/// The empty text, for a module whose text cannot be read.
pub(crate) fn empty() -> &'static Arc<str> {
    static EMPTY: OnceLock<Arc<str>> = OnceLock::new();
    EMPTY.get_or_init(|| Arc::from(""))
}

impl<'a> Texts<'a> {
    /// Texts read through `sets` (by package; `None` for the embedded
    /// std, whose texts are `embedded`).
    pub(crate) fn lazy(
        sources: &[(u16, String)],
        sets: &[&'a dyn SourceSet],
        embedded: Option<(usize, &'static HashMap<String, Arc<str>>)>,
    ) -> Self {
        let from = sets
            .iter()
            .enumerate()
            .map(|(i, s)| match embedded {
                Some((at, texts)) if at == i => Origin::Embedded(texts),
                _ => Origin::Set(*s),
            })
            .collect();
        Texts {
            cells: sources.iter().map(|_| OnceLock::new()).collect(),
            lines: sources.iter().map(|_| OnceLock::new()).collect(),
            sources: sources.into(),
            from,
        }
    }

    /// Texts given up front, one per module.
    pub(crate) fn ready(texts: Vec<Arc<str>>) -> Self {
        let sources: Arc<[(u16, String)]> = texts.iter().map(|_| (0, String::new())).collect();
        Texts {
            lines: texts.iter().map(|_| OnceLock::new()).collect(),
            cells: texts.into_iter().map(OnceLock::from).collect(),
            sources,
            from: Arc::new([Origin::Gone]),
        }
    }

    /// The same cells, readable after the run: each package's set is
    /// detached, or its unread texts are gone.
    pub(crate) fn detach(&self) -> Texts<'static> {
        let from = self
            .from
            .iter()
            .map(|f| match f {
                Origin::Set(s) => s.detach().map_or(Origin::Gone, Origin::Detached),
                Origin::Detached(s) => Origin::Detached(Arc::clone(s)),
                Origin::Embedded(t) => Origin::Embedded(t),
                Origin::Gone => Origin::Gone,
            })
            .collect();
        Texts {
            cells: Arc::clone(&self.cells),
            lines: Arc::clone(&self.lines),
            sources: Arc::clone(&self.sources),
            from,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Module `m`'s text, read now if it was not; `None` past the end or
    /// when its set is gone and the run never read it.
    #[must_use]
    pub fn get(&self, m: usize) -> Option<&Arc<str>> {
        let cell = self.cells.get(m)?;
        if let Some(t) = cell.get() {
            return Some(t);
        }
        let (package, path) = &self.sources[m];
        let read = |set: &dyn SourceSet| {
            Arc::from(
                set.read(path)
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default(),
            )
        };
        let text = match self.from.get(usize::from(*package))? {
            Origin::Set(s) => read(*s),
            Origin::Detached(s) => read(&**s),
            Origin::Embedded(t) => Arc::clone(t.get(path).unwrap_or_else(|| empty())),
            Origin::Gone => return None,
        };
        Some(cell.get_or_init(|| text))
    }
}

impl Texts<'_> {
    /// The 1-based line and column of byte `offset` in module `m`'s text
    /// (`None` as for [`Self::get`]). The column counts characters after
    /// the line's last newline. The text's [`LineIndex`] is built on the
    /// first call, so locating d positions in a text of n bytes costs
    /// n + d log n.
    #[must_use]
    pub fn line_column(&self, m: usize, offset: usize) -> Option<(usize, usize)> {
        let text = self.get(m)?;
        let lines = self.lines[m].get_or_init(|| LineIndex::new(text.as_bytes()));
        Some(lines.line_column(offset.min(text.len())))
    }
}

thread_local! {
    static INDEXED: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Count the text bytes `run` indexes for line and column lookups on this
/// thread ([`Texts::line_column`]): a text's bytes once, however many
/// positions in it are located.
pub fn count_indexed_bytes<R>(run: impl FnOnce() -> R) -> (R, u64) {
    let previous = INDEXED.with(|c| c.replace(Some(0)));
    let result = run();
    let n = INDEXED.with(|c| c.replace(previous)).unwrap_or(0);
    (result, n)
}

/// Where a text's lines start and where its multi-byte characters are,
/// for locating a byte offset by binary search (as rustc's `SourceFile`
/// keeps its line starts).
#[derive(Debug, Default)]
pub(crate) struct LineIndex {
    /// The offset after each `\n`, after a first 0.
    starts: Vec<u32>,
    /// The offset of each UTF-8 continuation byte: a character is its
    /// bytes less these.
    continuations: Vec<u32>,
}

impl LineIndex {
    /// The index of `text`, in one pass.
    #[must_use]
    pub(crate) fn new(text: &[u8]) -> Self {
        INDEXED.with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + text.len() as u64));
            }
        });
        let mut lines = LineIndex {
            starts: vec![0],
            continuations: Vec::new(),
        };
        for (i, &b) in (0u32..).zip(text) {
            if b == b'\n' {
                lines.starts.push(i + 1);
            } else if b & 0xC0 == 0x80 {
                lines.continuations.push(i);
            }
        }
        lines
    }

    /// The 1-based line and column of byte `offset`, at most the text's
    /// length. A character `offset` splits counts as one.
    #[must_use]
    pub(crate) fn line_column(&self, offset: usize) -> (usize, usize) {
        let offset = u32::try_from(offset).unwrap_or(u32::MAX);
        // Each start at or before `offset` begins one line up to it.
        let line = self.starts.partition_point(|&s| s <= offset);
        let start = self.starts[line - 1];
        let multi = |at: u32| self.continuations.partition_point(|&c| c < at);
        let column = (offset - start) as usize - (multi(offset) - multi(start)) + 1;
        (line, column)
    }
}

impl std::ops::Index<usize> for Texts<'_> {
    type Output = Arc<str>;
    /// Module `m`'s text, read now if it was not; empty when it cannot be.
    fn index(&self, m: usize) -> &Arc<str> {
        self.get(m).unwrap_or_else(|| empty())
    }
}

#[cfg(test)]
mod tests {
    use super::LineIndex;

    /// Each byte offset's line and column, by scanning from the start.
    fn scanned(text: &str, offset: usize) -> (usize, usize) {
        let before = &text.as_bytes()[..offset];
        let line = before.split(|&b| b == b'\n').count();
        let start = before
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        let column = String::from_utf8_lossy(&before[start..]).chars().count() + 1;
        (line, column)
    }

    #[test]
    fn every_offset_locates_as_a_scan_from_the_start() {
        for text in [
            "",
            "\n",
            "a",
            "ab\ncd\n\nef",
            "x = \"\u{e9}t\u{e9}\"\r\n\u{1f600} y\n\u{4e2d}\u{6587}z",
        ] {
            let lines = LineIndex::new(text.as_bytes());
            for offset in 0..=text.len() {
                assert_eq!(
                    lines.line_column(offset),
                    scanned(text, offset),
                    "{text:?} at {offset}"
                );
            }
        }
    }
}
