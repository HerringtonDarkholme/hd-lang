//! Running a module on V8 through Node with the JS host
//! (`compiler/host/run.mjs`, embedded so the `hd` binary is relocatable).

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

/// The JS host: `hd.println_i32`, then the module's `main` export.
pub const HOST: &str = include_str!("../../../host/run.mjs");

/// Writes `wasm` and the host next to `wasm_path` and runs Node on them.
/// The `.wasm` file stays; the host script is removed afterwards.
pub fn run_wasm(wasm: &[u8], wasm_path: &Path) -> Result<Output, String> {
    fs::write(wasm_path, wasm).map_err(|e| format!("{}: {e}", wasm_path.display()))?;
    let host = wasm_path.with_extension("host.mjs");
    fs::write(&host, HOST).map_err(|e| format!("{}: {e}", host.display()))?;
    let out = Command::new("node")
        .arg(&host)
        .arg(wasm_path)
        .output()
        .map_err(|e| format!("node: {e}"));
    let _ = fs::remove_file(&host);
    out
}
