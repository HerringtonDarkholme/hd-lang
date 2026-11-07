//! Every `hd` command test except the CLI conformance runner, linked once. They
//! spawn `hd` as a child process, so none shares process state with another.
//! `cli_conformance.rs` stays a separate target: it judges the whole CLI tier
//! against `KNOWN_FAILURES.tsv` and runs its own worker pool.

fn normalize_std_lines(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find("<std>/") {
        let frame = cursor + found;
        let Some(suffix) = text[frame..].find(".hd:") else {
            break;
        };
        let digits = frame + suffix + ".hd:".len();
        let end = text[digits..]
            .find(|character: char| !character.is_ascii_digit())
            .map_or(text.len(), |end| digits + end);
        if end == digits {
            cursor = digits;
            continue;
        }
        normalized.push_str(&text[cursor..digits]);
        normalized.push_str("LINE");
        cursor = end;
    }
    normalized.push_str(&text[cursor..]);
    normalized
}

/// Gives a test's cache the object store all these tests share, and returns it.
/// Only compiled, content-addressed objects are shared, as in `cli_conformance`,
/// so std is prepared once instead of once per test. A test that reads the store
/// calls `own_objects` first. A cache that cannot hold the link keeps its own store.
fn shared_objects<P: AsRef<std::path::Path>>(cache: P) -> P {
    static OBJECTS: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    let objects = OBJECTS.get_or_init(|| {
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("hd-shared-objects");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("shared object store");
        dir
    });
    let obj = cache.as_ref().join("obj");
    if std::fs::symlink_metadata(&obj).is_err() {
        let _ = std::fs::create_dir_all(cache.as_ref());
        #[cfg(unix)]
        let _ = std::os::unix::fs::symlink(objects, obj);
        // A Windows account without the right to make links keeps its own store.
        #[cfg(windows)]
        let _ = std::os::windows::fs::symlink_dir(objects, obj);
    }
    cache
}

/// Gives a test's cache an object store of its own, for a test that counts what
/// a build checks or publishes.
fn own_objects(cache: &std::path::Path) {
    std::fs::create_dir_all(cache.join("obj")).expect("test cache");
}

mod affected;
mod cache_gc;
mod check_cmd;
mod dbg;
mod deps;
mod doc_links;
mod doc_name;
mod doc_tests;
#[cfg(unix)]
mod fifo;
mod fixing;
mod host_traits;
mod install;
#[cfg(unix)]
mod interrupt_signal;
#[cfg(unix)]
mod metrics_fixtures;
mod node_host_exit;
mod panic_frames;
mod property_tests;
mod resource_limits;
mod run;
mod same_size_edit;
mod spread_position;
mod test_bodies;
mod test_cmd;
mod test_env;
mod test_only_uses;
mod tests_block_items;
#[cfg(feature = "wasmtime")]
mod wasmtime_engine;
