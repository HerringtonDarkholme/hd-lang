use hd_run::Grants;

use crate::codec::Enc;

pub fn sys(name: &str, grants: &Grants) -> Enc {
    sys_read(name, grants, |e| {
        match name {
            "os" => {
                e.str(match std::env::consts::OS {
                    "solaris" | "illumos" => "sunos",
                    other => other,
                });
            }
            "arch" => {
                e.str(match std::env::consts::ARCH {
                    "powerpc" => "ppc",
                    "powerpc64" => "ppc64",
                    "loongarch64" => "loong64",
                    other => other,
                });
            }
            // The name `hostname(1)` prints.
            #[cfg(unix)]
            "hostname" => {
                e.str(&rustix::system::uname().nodename().to_string_lossy());
            }
            #[cfg(windows)]
            "hostname" => match hostname::get() {
                Ok(name) => {
                    e.str(&name.to_string_lossy());
                }
                Err(_) => return false,
            },
            "cpu_count" => match std::thread::available_parallelism() {
                Ok(n) => {
                    e.leb(n.get() as u64);
                }
                Err(_) => return false,
            },
            _ => unreachable!("registered Sys method"),
        }
        true
    })
}

pub(crate) fn sys_read(name: &str, grants: &Grants, read: impl FnOnce(&mut Enc) -> bool) -> Enc {
    let mut e = Enc::default();
    if !grants.covers_name("Sys", name) {
        e.leb(1).leb(0).str(name);
        return e;
    }
    e.leb(0);
    if !read(&mut e) {
        e = Enc::default();
        e.leb(1).leb(1).str(name);
    }
    e
}
