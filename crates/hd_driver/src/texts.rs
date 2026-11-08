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
            sources: sources.into(),
            from,
        }
    }

    /// Texts given up front, one per module.
    pub(crate) fn ready(texts: Vec<Arc<str>>) -> Self {
        let sources: Arc<[(u16, String)]> = texts.iter().map(|_| (0, String::new())).collect();
        Texts {
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

impl std::ops::Index<usize> for Texts<'_> {
    type Output = Arc<str>;
    /// Module `m`'s text, read now if it was not; empty when it cannot be.
    fn index(&self, m: usize) -> &Arc<str> {
        self.get(m).unwrap_or_else(|| empty())
    }
}
