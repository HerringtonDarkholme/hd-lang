//! The explicit lint command. Selection and reporting reuse checking.

use std::ffi::OsString;
use std::process::ExitCode;

use hd_lint::{Level, Policy};

pub(crate) fn command(args: &[OsString]) -> ExitCode {
    let mut policy = Policy::default();
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let text = args[i].to_string_lossy();
        let (flag, inline) = text
            .split_once('=')
            .map_or((text.as_ref(), None), |(k, v)| (k, Some(v)));
        let level = match flag {
            "--allow" => Some(Level::Allow),
            "--warn" => Some(Level::Warn),
            "--deny" => Some(Level::Deny),
            _ => None,
        };
        if let Some(level) = level {
            let name = if let Some(name) = inline {
                name.to_owned()
            } else {
                i += 1;
                let Some(name) = args.get(i) else {
                    return crate::fail(&format!("`{flag}` needs a lint name"));
                };
                name.to_string_lossy().into_owned()
            };
            if let Err(error) = policy.set(&name, level) {
                return crate::fail(&error);
            }
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    crate::check_cmd::lint_command(&rest, policy)
}
