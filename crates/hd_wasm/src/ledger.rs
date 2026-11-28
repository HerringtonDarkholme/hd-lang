//! The decision ledger (`ledger` feature): how often each emitter decision
//! fired in one link, printed as `LEDGER<TAB>key<TAB>count` rows for
//! `tools/tasks/ledger.hd`. It counts only with `HD_LEDGER` set, so CI's
//! every-feature test build prints nothing more.

use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

static HITS: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());
static ON: LazyLock<bool> = LazyLock::new(|| std::env::var_os("HD_LEDGER").is_some());

fn hits() -> MutexGuard<'static, BTreeMap<String, usize>> {
    HITS.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn add(key: &str, n: usize) {
    if !*ON {
        return;
    }
    *hits().entry(key.to_owned()).or_default() += n;
}

/// A value's variant name, without its fields.
pub(crate) fn variant(v: impl std::fmt::Debug) -> String {
    let name = format!("{v:?}");
    name.split('(').next().unwrap_or_default().to_owned()
}

/// Prints and clears the counts of the link that just finished.
pub(crate) fn flush() {
    for (key, n) in std::mem::take(&mut *hits()) {
        eprintln!("LEDGER\t{key}\t{n}");
    }
}
