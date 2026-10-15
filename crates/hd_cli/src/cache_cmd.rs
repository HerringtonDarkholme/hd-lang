//! `hd cache gc` (Compiled Cache, `cli.cache.gc`, `cli.cache.gc.anywhere`):
//! evicts the least recently used compiled entries down to the size cap
//! now, with no time bound, and prints their size before and after. It
//! reads no manifest, so it works anywhere.

use std::ffi::OsString;
use std::process::ExitCode;

use hd_cache::DiskStore;

use crate::{cache_cap, cache_dir, fail};

pub(crate) fn command(args: &[OsString]) -> ExitCode {
    match args {
        [gc] if gc == "gc" => {}
        [gc, extra, ..] if gc == "gc" => {
            return fail(&format!(
                "`hd cache gc` takes nothing, not `{}`",
                extra.to_string_lossy()
            ));
        }
        _ => return fail("`hd cache` has one command: `hd cache gc` evicts compiled entries"),
    }
    let cap = match cache_cap() {
        Ok(cap) => cap,
        Err(error) => return fail(&error),
    };
    let sizes = DiskStore::new(cache_dir()).with_cap(cap).collect();
    println!("compiled cache before: {}", size(sizes.before));
    println!(
        "compiled cache after: {} (cap {})",
        size(sizes.after),
        size(cap)
    );
    ExitCode::SUCCESS
}

/// Bytes in binary units with one decimal, as `21.0 MiB`.
fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut unit = 0;
    let mut scale = 1024u64;
    while unit + 1 < UNITS.len() && bytes / scale >= 1024 {
        unit += 1;
        scale *= 1024;
    }
    let tenths = (u128::from(bytes) * 10 + u128::from(scale) / 2) / u128::from(scale);
    format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::size;

    #[test]
    fn sizes_print_in_binary_units() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(1023), "1023 B");
        assert_eq!(size(1024), "1.0 KiB");
        assert_eq!(size(1536), "1.5 KiB");
        assert_eq!(size(21 * 1024 * 1024), "21.0 MiB");
        assert_eq!(size(10 << 30), "10.0 GiB");
    }
}
