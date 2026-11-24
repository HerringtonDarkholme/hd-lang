//! Test FIFOs. rustix has no `mkfifo` on macOS, and std has none, so the
//! tests run `mkfifo(1)`.

use std::path::Path;

/// Makes a FIFO at `path`, readable and writable by the owner only.
pub(crate) fn make(path: &Path) {
    let status = std::process::Command::new("mkfifo")
        .args(["-m", "600"])
        .arg(path)
        .status()
        .expect("runs mkfifo(1)");
    assert!(
        status.success(),
        "mkfifo {} failed: {status}",
        path.display()
    );
}
