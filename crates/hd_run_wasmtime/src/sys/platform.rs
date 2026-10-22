//! `Sys`: the platform's name, architecture, host name and CPU count,
//! each behind its own grant entry.

use hd_run::Grants;

use crate::abi::HostError;
use crate::codec::{Enc, Wire, encode};

/// `Sys.<name>`'s result: the value, `.NotGranted` outside the grant, or
/// `.Unsupported` when the platform cannot tell.
pub(super) fn sys(name: &str, grants: &Grants) -> Enc {
    match name {
        "os" => sys_read(name, grants, || {
            Some(
                match std::env::consts::OS {
                    "solaris" | "illumos" => "sunos",
                    other => other,
                }
                .to_owned(),
            )
        }),
        "arch" => sys_read(name, grants, || {
            Some(
                match std::env::consts::ARCH {
                    "powerpc" => "ppc",
                    "powerpc64" => "ppc64",
                    "loongarch64" => "loong64",
                    other => other,
                }
                .to_owned(),
            )
        }),
        // The name `hostname(1)` prints.
        #[cfg(unix)]
        "hostname" => sys_read(name, grants, || {
            Some(
                rustix::system::uname()
                    .nodename()
                    .to_string_lossy()
                    .into_owned(),
            )
        }),
        #[cfg(windows)]
        "hostname" => sys_read(name, grants, || {
            hostname::get()
                .ok()
                .map(|name| name.to_string_lossy().into_owned())
        }),
        "cpu_count" => sys_read(name, grants, || {
            std::thread::available_parallelism()
                .ok()
                .map(|n| u32::try_from(n.get()).unwrap_or(u32::MAX))
        }),
        _ => unreachable!("registered Sys method"),
    }
}

pub(super) fn sys_read<T: Wire>(
    name: &str,
    grants: &Grants,
    read: impl FnOnce() -> Option<T>,
) -> Enc {
    let result = if grants.covers_name("Sys", name) {
        read().ok_or_else(|| HostError::Unsupported {
            name: name.to_owned(),
        })
    } else {
        Err(HostError::NotGranted {
            subject: name.to_owned(),
        })
    };
    encode(&result)
}
