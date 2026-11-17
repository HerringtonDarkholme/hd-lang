//! Package host imports on Wasmtime (`cli.host.package.native-*`): the
//! module's contract indexed once at load, the native preflight that
//! resolves an embedder's registrations by nominal identity and checks the
//! root manifest's consent before linking, and the linked root methods
//! that call them.
//!
//! Cost. Load decodes the contract and indexes every import it describes
//! (`hd_host_abi::schema::Imports`): one pass over its methods. Preflight builds one identity map and one
//! trait-path map over the registrations, decodes the companion's contract
//! once and indexes it by module, then visits each contract interface once,
//! probing the consent index once per source path: linear in the
//! registrations plus the two contracts, times a log of the grant's keys.
//! Linking probes the index once per module import. A call costs its
//! argument and result bytes. A pending call adds one insert into the
//! reactor's operation map at `.start`, one queue push at its completion,
//! one map probe when the reactor takes that push, and one removal at
//! `.finish` or abort: nothing scans the pending calls.

use std::any::Any;

use hd_base::{HashMap, HashSet};
use hd_host_abi::schema::{Contract, Imports, Interface, Part, Value};
use hd_run::{Consent, StartError};
use wasmtime::Linker;

use super::reply::{bytes, handle, int_arg, int_val, put_bytes};
use super::{Host, Stop};
use crate::native::{Answer, Bindings, Dispatch, Unregistered};

/// A module's package host contract and the imports it describes.
pub(crate) struct Package {
    contract: Contract,
    imports: Imports,
}

impl Package {
    pub(crate) fn new(contract: Contract) -> Self {
        let imports = Imports::new(&contract);
        Self { contract, imports }
    }

    /// Whether the contract describes the import `module` `name`.
    pub(crate) fn describes(&self, module: &str, name: &str) -> bool {
        self.imports.describes(module, name)
    }

    /// The first root interface, the one a refusal names.
    fn first_root(&self) -> Option<&Interface> {
        self.contract.interfaces.iter().find(|i| !i.handle)
    }
}

/// One linked root provider, at its interface's index in the contract.
pub(crate) struct Native {
    provider: Box<dyn Any>,
    dispatch: Dispatch,
}

fn refused(message: String) -> StartError {
    StartError::Refused(message)
}

/// Resolves `bindings` against the module's contract before anything is
/// linked or the guest starts, and returns the providers in contract
/// interface order. With no registry, a module requiring a root trait is
/// refused (`cli.host.package.native-cli`). A registered provider runs
/// only when `consent`, the root manifest's, grants its trait
/// (`native-two-keys`): registration grants the guest nothing.
pub(crate) fn preflight(
    package: Option<&Package>,
    bindings: Option<Bindings>,
    consent: &Consent,
) -> Result<Vec<Native>, StartError> {
    let Some(bindings) = bindings else {
        return match package.and_then(Package::first_root) {
            Some(root) => Err(refused(
                Unregistered::new(&root.identity.to_string()).to_string(),
            )),
            None => Ok(Vec::new()),
        };
    };
    let companion = bindings.contract();
    let mut registrations: Vec<_> = bindings
        .into_registrations()
        .into_iter()
        .map(Some)
        .collect();
    // One map by nominal identity (`cli.host.package.duplicate`), and one
    // by trait path, which only names the registered identity when the
    // required one is missing (`native-mismatch`).
    let mut by_identity: HashMap<&'static str, usize> = HashMap::default();
    let mut by_trait: HashMap<&'static str, &'static str> = HashMap::default();
    for (n, r) in registrations.iter().flatten().enumerate() {
        if by_identity.insert(r.identity, n).is_some() {
            return Err(refused(format!(
                "duplicate native provider registrations for package host trait {:?}",
                r.identity
            )));
        }
        let trait_path = r.identity.rsplit_once('/').map_or(r.identity, |(_, t)| t);
        by_trait.entry(trait_path).or_insert(r.identity);
    }
    let interfaces = package.map_or(&[][..], |p| &p.contract.interfaces[..]);
    let mut natives = Vec::with_capacity(interfaces.len());
    // The companion's contract, by module, once.
    let generated: HashMap<String, Interface> = if interfaces.is_empty() {
        HashMap::default()
    } else {
        Contract::decode(companion)
            .map_err(|why| refused(format!("invalid native provider contract: {why}")))?
            .interfaces
            .into_iter()
            .map(|i| (i.module.clone(), i))
            .collect()
    };
    for interface in interfaces {
        let name = interface.identity.to_string();
        if interface.handle {
            // gap: #2082 keeps returned handle trait objects in the
            // instance's registry.
            return Err(refused(format!(
                "package host trait {name:?} crosses as a native handle, which wasmtime does not hold yet"
            )));
        }
        let Some(&n) = by_identity.get(name.as_str()) else {
            return Err(refused(
                match by_trait.get(interface.identity.trait_path.as_str()) {
                    Some(registered) => format!(
                        "native provider for package host trait {name:?} is registered as {registered:?}"
                    ),
                    None => Unregistered::new(&name).to_string(),
                },
            ));
        };
        let Some(registration) = registrations[n].take() else {
            unreachable!("ruled out: contract identities are unique (Contract::validate)")
        };
        // The second key, in the same visit: the app's own decision, which
        // no registration and no `--cap` flag replaces
        // (`manifest-consent`, `consent-preflight`).
        consent
            .package(interface)
            .map_err(|why| refused(why.message(interface)))?;
        // The companion's interface of the registered module: the same
        // methods in the same order, so a method index means one method.
        if generated.get(registration.module) != Some(interface) {
            return Err(refused(format!(
                "native provider for package host trait {name:?} was generated from a different contract"
            )));
        }
        natives.push(Native {
            provider: registration.provider,
            dispatch: registration.dispatch,
        });
    }
    if let Some(r) = registrations.iter().flatten().next() {
        return Err(refused(format!(
            "native provider for package host trait {:?} names no trait this module requires",
            r.identity
        )));
    }
    Ok(natives)
}

/// Defines each module import of a root method that preflight linked to
/// its provider, and records it in `defined`. Every other described import
/// is left to `define`, which names it. A suspending method is two
/// imports: `.start` answers at once or registers its pending call with
/// the instance's reactor, and `.finish` takes the delivered value
/// (`cli.host.package.native-completion-forms`).
pub(crate) fn define(
    linker: &mut Linker<Host>,
    module: &wasmtime::Module,
    package: Option<&Package>,
    defined: &mut HashSet<(String, String)>,
) -> Result<(), StartError> {
    let Some(package) = package else {
        return Ok(());
    };
    for import in module.imports() {
        let (m, n) = (import.module(), import.name());
        let (Some((interface, index, part)), Some(ty)) =
            (package.imports.call(m, n), import.ty().func().cloned())
        else {
            continue;
        };
        let declared = &package.contract.interfaces[interface];
        let method = &declared.methods[index];
        let label = format!("{}.{}", declared.identity, method.member);
        // `.start` and a plain call take one argument length when the
        // method takes any argument, `.finish` its handle. A plain call
        // has one result length unless it returns `()` or never returns,
        // `.start` a status and its length or handle, `.finish` a length.
        let takes = match part {
            Part::Finish => 1,
            _ => usize::from(!method.params.is_empty()),
        };
        let gives = match (part, &method.result) {
            (Part::Start, _) => Some(2),
            (_, Value::Never) => None,
            (Part::Call, Value::Void) => Some(0),
            _ => Some(1),
        };
        if ty.params().len() != takes || gives.is_some_and(|g| ty.results().len() != g) {
            return Err(refused(format!(
                "the import `{m}` `{n}` of package host method {label} has an unexpected signature"
            )));
        }
        let results: Vec<_> = ty.results().collect();
        let result = method.result.clone();
        let call = (interface, index);
        linker
            .func_new(m, n, ty, move |mut caller, params, out| {
                let fail = |why: &str| Stop::Contract(format!("{label}: {why}"));
                // The reply: `.start`'s status (0 ready, 1 pending) and its
                // result length or handle, or a call's or `.finish`'s
                // length. The signature check above fixed the counts.
                let reply = |out: &mut [wasmtime::Val], status: i64, value: i64| {
                    if let [s, v] = &results[..] {
                        out[0] = int_val(s, status);
                        out[1] = int_val(v, value);
                    } else if let [one] = &results[..] {
                        out[0] = int_val(one, value);
                    }
                };
                let value = match part {
                    Part::Finish => {
                        let h = handle(int_arg(params, 0)?);
                        caller
                            .data_mut()
                            .reactor
                            .finish_native(h, call)
                            .ok_or_else(|| fail("a finish of an operation that is not pending"))?
                            .map_err(|error| fail(&error.to_string()))?
                    }
                    Part::Call | Part::Start => {
                        let args = if takes == 0 {
                            Vec::new()
                        } else {
                            bytes(&mut caller, int_arg(params, 0)?)?
                        };
                        let native = &mut caller.data_mut().natives[interface];
                        let answer = (native.dispatch)(native.provider.as_mut(), index, &args)
                            .map_err(|error| fail(&error.to_string()))?;
                        match (part, answer) {
                            (_, Answer::Ready(value)) => value,
                            (Part::Start, Answer::Pending(pending)) => {
                                let h = caller
                                    .data_mut()
                                    .reactor
                                    .native(pending, call)
                                    .ok_or_else(|| {
                                        fail("the provider aborted the pending call it returned")
                                    })?;
                                reply(out, 1, i64::from(h));
                                return Ok(());
                            }
                            (_, Answer::Pending(_)) => {
                                return Err(fail(
                                    "a method that does not suspend returned a pending call",
                                )
                                .into());
                            }
                        }
                    }
                };
                // The result: its length, after the value is written to
                // the exchange buffer; `()` writes nothing, and its
                // `.start` or `.finish` length is 0.
                let length = match result {
                    Value::Void => 0,
                    Value::Never => {
                        return Err(fail(
                            "package host: the method never returns, but its provider returned",
                        )
                        .into());
                    }
                    _ => put_bytes(&mut caller, &value)?,
                };
                reply(out, 0, length);
                Ok(())
            })
            .map_err(|error| StartError::Engine(error.to_string()))?;
        defined.insert((m.to_owned(), n.to_owned()));
    }
    Ok(())
}
