//! Line and column lookups for byte offsets in a text: one pass over the
//! text builds a [`LineIndex`], and each lookup is two binary searches, so
//! locating d positions in a text of n bytes costs n + d log n. Every
//! stage that turns an offset into a line and column (a diagnostic's
//! position, a `@track_caller` call site's record, a syntax error in
//! `hd fmt`) builds one index per text and asks it.

thread_local! {
    static INDEXED: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Count the text bytes `run` indexes for line and column lookups on this
/// thread ([`LineIndex::new`]): a text's bytes once, however many
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
#[derive(Clone, Debug, Default)]
pub struct LineIndex {
    /// The offset after each `\n`, after a first 0.
    starts: Vec<u32>,
    /// The offset of each UTF-8 continuation byte: a character is its
    /// bytes less these.
    continuations: Vec<u32>,
}

impl LineIndex {
    /// The index of `text`, in one pass.
    #[must_use]
    pub fn new(text: &[u8]) -> Self {
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

    /// The 1-based line and column, in characters, of byte `offset`, at
    /// most the text's length. A character `offset` splits counts as one.
    #[must_use]
    pub fn line_column(&self, offset: usize) -> (usize, usize) {
        let offset = u32::try_from(offset).unwrap_or(u32::MAX);
        // Each start at or before `offset` begins one line up to it.
        let line = self.starts.partition_point(|&s| s <= offset);
        let start = self.starts[line - 1];
        let multi = |at: u32| self.continuations.partition_point(|&c| c < at);
        let column = (offset - start) as usize - (multi(offset) - multi(start)) + 1;
        (line, column)
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
