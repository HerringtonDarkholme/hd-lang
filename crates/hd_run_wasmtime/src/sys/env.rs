//! `Env`: a variable outside the grant reads as unset, with a notice
//! once per name (`cli.cap.env.notice`).

use wasmtime::{Caller, Val};

use crate::host::{Host, Ret, int_arg, text};

/// `Env.get`.
pub(super) fn get(caller: &mut Caller<'_, Host>, params: &[Val]) -> wasmtime::Result<Ret> {
    let name = text(caller, int_arg(params, 0)?)?;
    let host = caller.data_mut();
    let value = var(&host.env_overlay, &name);
    Ok(if host.grants.covers_name("Env", &name) {
        Ret::value(&value)
    } else {
        // Outside the grant, a variable reads as unset.
        if value.is_some() && host.noticed.insert(name.clone()) {
            host.err(&format!("{}\n", hd_run::env_notice(&name)));
        }
        Ret::value(&None::<String>)
    })
}

/// `Env.names`: the granted names of the process's variables.
pub(super) fn names(caller: &mut Caller<'_, Host>) -> Ret {
    let mut names: Vec<String> = std::env::vars_os()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    for (name, _) in &caller.data().env_overlay {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    let names: Vec<String> = names
        .into_iter()
        .filter(|n| caller.data().grants.covers_name("Env", n))
        .collect();
    Ret::value(&names)
}

/// A variable's value: the last pair of a test child's command for it, else
/// the process's own; a name the environment cannot hold is unset.
fn var(overlay: &[(String, String)], name: &str) -> Option<String> {
    if name.is_empty() || name.contains(['=', '\0']) {
        return None;
    }
    if let Some((_, value)) = overlay.iter().rev().find(|(set, _)| set == name) {
        return Some(value.clone());
    }
    std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
}
