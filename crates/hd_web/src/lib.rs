#![forbid(unsafe_code)]
//! `hd_web`: the browser front end (design-overview.md §2.1; scheduler.md
//! §6.2 "Stepping"; cache.md §5.8). The JS `SourceSet` and `CacheStore`
//! are the in-memory ones the host fills; the session checks in slices
//! with the stepping scheduler. wasm-bindgen glue is not a dependency yet
//! (SK-10): this crate is plain Rust with the session's shape.

use hd_cache::MemoryStore;
use hd_driver::{PipelineReport, analyze_package};
use hd_project::MemorySources;
use hd_sched::{CancelFlag, Progress};

/// One playground worker's state: sources from JS, entries from `IndexedDB`.
#[derive(Default)]
pub struct WebSession {
    pub sources: MemorySources,
    pub store: MemoryStore,
    pub cancel: CancelFlag,
}

impl WebSession {
    /// "source changed": replaces a file and cancels the running slice.
    pub fn set_source(&mut self, path: &str, text: &str) {
        self.cancel.cancel();
        self.sources.insert(path, text);
        self.cancel = CancelFlag::default();
    }

    /// One check. The stepping executor's slices are not wired yet, so the
    /// whole graph runs as one slice.
    pub fn check(&self) -> (Progress, PipelineReport) {
        (Progress::Done, analyze_package("playground", &self.sources))
    }
}

#[cfg(test)]
mod tests {
    use super::WebSession;
    use hd_base::Stage;

    #[test]
    fn a_session_checks_its_sources() {
        let mut s = WebSession::default();
        s.set_source("main.hd", "pub fn main():\n    println(1)\n");
        let (_, r) = s.check();
        assert_eq!(r.tally(Stage::Body).ok, 1);
    }
}
