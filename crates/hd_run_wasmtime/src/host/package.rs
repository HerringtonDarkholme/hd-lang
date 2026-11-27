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
//! `.finish` or abort: nothing scans the pending calls. A returned handle
//! costs one insert into the instance's handle registry, and each method
//! call or release on it one slot probe (`native::handles`): nothing scans
//! the handles.

use std::any::Any;

use hd_base::{HashMap, HashSet};
use hd_host_abi::schema::{Contract, Imports, Interface, Part, Value};
use hd_run::{Consent, StartError};
use wasmtime::Linker;

use super::reply::{bytes, handle, int_arg, int_val, put_bytes};
use super::{Host, Stop};
use crate::native::c::{CTable, Library, Provider};
use crate::native::c_call::{Arg, Kind, Plan};
use crate::native::{Answer, Bindings, Dispatch, HostError, Unregistered};
use hd_host_abi::c::{Num, Param, Ret, Table};
use std::sync::Arc;

/// A module's package host contract and the imports it describes, with
/// the C table of each C-bound root interface (`hd_host_abi::c`).
pub(crate) struct Package {
    contract: Contract,
    imports: Imports,
    /// Per contract interface, its C table and each method's slot by name,
    /// or `None` for an interface that no C provider serves.
    c: Vec<Option<CInterface>>,
    /// Each C-bound handle trait's interface, by trait name, which a C
    /// method's handle result names (`Ret::Handle`).
    c_handles: HashMap<String, usize>,
}

/// A C-bound root interface's table, and each method's slot index and row
/// by method name, indexed once.
pub(crate) struct CInterface {
    table: Table,
    slots: HashMap<String, (usize, usize)>,
}

impl Package {
    /// Cost: one pass over the contract's methods and the tables' methods.
    pub(crate) fn new(contract: Contract, c_tables: Vec<Table>) -> Self {
        let imports = Imports::new(&contract);
        let mut by_module: HashMap<String, Table> = c_tables
            .into_iter()
            .map(|t| (t.module.clone(), t))
            .collect();
        let c = contract
            .interfaces
            .iter()
            .map(|i| {
                let table = by_module.remove(&i.module)?;
                let mut slots = HashMap::default();
                let mut at = 0;
                for (row, m) in table.methods.iter().enumerate() {
                    slots.insert(m.name.clone(), (at, row));
                    at += 1 + usize::from(m.suspends);
                }
                Some(CInterface { table, slots })
            })
            .collect::<Vec<_>>();
        let c_handles = c
            .iter()
            .enumerate()
            .filter_map(|(n, i)| {
                let i = i.as_ref().filter(|i| i.table.handle)?;
                Some((i.table.trait_name.clone(), n))
            })
            .collect();
        Self {
            contract,
            imports,
            c,
            c_handles,
        }
    }

    /// Interface `index`'s C table, when a C provider serves it.
    fn c_interface(&self, index: usize) -> Option<&CInterface> {
        self.c.get(index).and_then(Option::as_ref)
    }

    /// Whether any root interface is served by a C provider.
    pub(crate) fn has_c(&self) -> bool {
        self.c.iter().any(Option::is_some)
    }

    /// Whether the contract describes the import `module` `name`.
    pub(crate) fn describes(&self, module: &str, name: &str) -> bool {
        self.imports.describes(module, name)
    }

    /// The first root interface no C provider serves, the one a refusal
    /// names when no embedder registered a provider.
    fn first_root(&self) -> Option<&Interface> {
        self.contract
            .interfaces
            .iter()
            .enumerate()
            .find(|(n, i)| !i.handle && self.c_interface(*n).is_none())
            .map(|(_, i)| i)
    }
}

/// One linked root provider. The instance holds one slot per contract
/// interface, at its index; a handle trait's slot is empty, since each
/// handle carries its own trait object and dispatch.
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
/// refused (`cli.host.package.native-cli-binds-c`). A registered provider runs
/// only when `consent`, the root manifest's, grants its trait
/// (`native-two-keys`): registration grants the guest nothing.
pub(crate) fn preflight(
    package: Option<&Package>,
    bindings: Option<Bindings>,
    consent: &Consent,
) -> Result<Vec<Option<Native>>, StartError> {
    let Some(bindings) = bindings else {
        return match package.and_then(Package::first_root) {
            Some(root) => Err(refused(
                Unregistered::new(&root.identity.to_string()).to_string(),
            )),
            None => Ok(package.map_or_else(Vec::new, |p| {
                p.contract.interfaces.iter().map(|_| None).collect()
            })),
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
    // The companion's contract, by module, once, with each interface's
    // index: the companion names a handle trait by that index.
    let generated: HashMap<String, (usize, Interface)> = if interfaces.is_empty() {
        HashMap::default()
    } else {
        Contract::decode(companion)
            .map_err(|why| refused(format!("invalid native provider contract: {why}")))?
            .interfaces
            .into_iter()
            .enumerate()
            .map(|(n, i)| (i.module.clone(), (n, i)))
            .collect()
    };
    let differs = |name: &str| {
        refused(format!(
            "native provider for package host trait {name:?} was generated from a different contract"
        ))
    };
    for (index, interface) in interfaces.iter().enumerate() {
        let name = interface.identity.to_string();
        // A C provider serves it (`c_preflight`); no registration does.
        if package.is_some_and(|p| p.c_interface(index).is_some()) {
            natives.push(None);
            continue;
        }
        if interface.handle {
            // A handle trait has no registration: a result's handle brings
            // its trait object and the companion's dispatch, which names
            // the trait by its index, so the index must mean this trait.
            if !generated
                .get(&interface.module)
                .is_some_and(|(n, i)| *n == index && i == interface)
            {
                return Err(differs(&name));
            }
            natives.push(None);
            continue;
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
        if generated.get(registration.module).map(|(_, i)| i) != Some(interface) {
            return Err(differs(&name));
        }
        natives.push(Some(Native {
            provider: registration.provider,
            dispatch: registration.dispatch,
        }));
    }
    if let Some(r) = registrations.iter().flatten().next() {
        return Err(refused(format!(
            "native provider for package host trait {:?} names no trait this module requires",
            r.identity
        )));
    }
    Ok(natives)
}

/// Opens and creates the C provider of each C-bound root interface from
/// `libraries` before anything is linked or the guest starts, in contract
/// interface order (`cli.host.c.entry`, `cli.host.c.check`,
/// `cli.host.c.create`). A provider runs only with the app manifest's
/// consent to its trait (`native-two-keys`). Each method must have a slot
/// the library filled, and a suspending one its abort slot too. Every
/// check comes before any `create` runs.
///
/// Cost: one library open, one symbol probe and one `create` per C-bound
/// root, and one slot probe per called method.
pub(crate) fn c_preflight(
    package: Option<&Package>,
    libraries: &[Library],
    consent: &Consent,
) -> Result<CBound, StartError> {
    let Some(package) = package.filter(|p| p.has_c()) else {
        return Ok(CBound::default());
    };
    if !crate::native::c_call::supported() {
        return Err(refused(
            "this target has no C call bridge: C providers run on x86-64 and arm64".into(),
        ));
    }
    let by_module: HashMap<&str, &Library> =
        libraries.iter().map(|l| (l.module.as_str(), l)).collect();
    let interfaces = &package.contract.interfaces;
    let mut bound = CBound {
        providers: (0..interfaces.len()).map(|_| None).collect(),
        handles: (0..interfaces.len()).map(|_| None).collect(),
    };
    // Each opened library, and the handle traits to check, each with the
    // library of the trait that returns it, which it shares (decision 1).
    let mut opened: HashMap<&std::path::Path, Arc<libloading::Library>> = HashMap::default();
    let mut todo: Vec<(usize, &Library)> = Vec::new();
    // Each queued handle trait's library, and each library's entry names
    // taken so far: a conflict the check could not see, as between two
    // modules, refuses the run (`cli.host.c.handle-library`,
    // `cli.host.c.handle-entry`).
    let mut queued: HashMap<usize, &Library> = HashMap::default();
    let mut entries: HashMap<(&std::path::Path, &str), String> = HashMap::default();
    let check_slots = |index: usize, table: &CTable, library: &Library, name: &str| {
        let interface = &interfaces[index];
        let Some(c) = package.c_interface(index) else {
            return Ok(());
        };
        // Each called method's slot, and a suspending one's abort slot
        // after it (`cli.host.c.slot`, `cli.host.c.abort-slot`): the
        // contract lists only the methods the module calls.
        for m in &interface.methods {
            let slot = c.slots.get(&m.name).map(|&(slot, _)| slot);
            let empty = |at: Option<usize>| at.and_then(|at| table.slot(at)).is_none();
            let what = if empty(slot) {
                "slot"
            } else if m.suspends && empty(slot.map(|slot| slot + 1)) {
                "abort slot"
            } else {
                continue;
            };
            return Err(refused(format!(
                "the C provider `{}` of package `{}` leaves the {what} of {name:?}'s method `{}` empty",
                library.name, library.package, m.member
            )));
        }
        Ok(())
    };
    // Roots first, in contract order, then their handle traits; every
    // check and refusal comes before any `create` runs.
    let mut checked = Vec::new();
    for (index, interface) in interfaces.iter().enumerate() {
        let Some(c) = package.c_interface(index).filter(|_| !interface.handle) else {
            continue;
        };
        let name = interface.identity.to_string();
        consent
            .package(interface)
            .map_err(|why| refused(why.message(interface)))?;
        let library = *by_module.get(interface.module.as_str()).ok_or_else(|| {
            refused(format!(
                "no C provider library was selected for package host trait {name:?}"
            ))
        })?;
        let lib = if let Some(lib) = opened.get(library.path.as_path()) {
            Arc::clone(lib)
        } else {
            let lib = crate::native::c::load(library, &name).map_err(refused)?;
            opened.insert(library.path.as_path(), Arc::clone(&lib));
            lib
        };
        claim(&mut entries, library, &c.table.trait_name, &name)?;
        let table = crate::native::c::table(&lib, library, &name, &c.table).map_err(refused)?;
        check_slots(index, &table, library, &name)?;
        for m in &c.table.methods {
            if let Ret::Handle(handle) = &m.result
                && let Some(&h) = package.c_handles.get(handle)
            {
                queue(&mut queued, &mut todo, h, library, &interfaces[h])?;
            }
        }
        checked.push((index, table, library, name));
    }
    // Then each handle trait a checked trait returns, once, over a
    // worklist: one visit per handle trait and per method that returns one.
    // A handle trait no C-bound trait returns gets no table.
    while let Some((index, library)) = todo.pop() {
        let Some(c) = package.c_interface(index) else {
            continue;
        };
        let name = interfaces[index].identity.to_string();
        let Some(lib) = opened.get(library.path.as_path()).cloned() else {
            unreachable!("ruled out: a handle trait's library opened with its root")
        };
        claim(&mut entries, library, &c.table.trait_name, &name)?;
        let table = crate::native::c::table(&lib, library, &name, &c.table).map_err(refused)?;
        check_slots(index, &table, library, &name)?;
        for m in &c.table.methods {
            if let Ret::Handle(handle) = &m.result
                && let Some(&h) = package.c_handles.get(handle)
            {
                queue(&mut queued, &mut todo, h, library, &interfaces[h])?;
            }
        }
        bound.handles[index] = Some(table);
    }
    // Every table checked: each root's provider is created
    // (`cli.host.c.create`).
    for (index, table, library, name) in checked {
        bound.providers[index] =
            Some(crate::native::c::create(table, library, &name).map_err(refused)?);
    }
    Ok(bound)
}

/// Takes `library`'s entry name `trait_name` for the trait `label`, or
/// refuses the run when another C-bound trait took it
/// (`cli.host.c.handle-entry`).
fn claim<'a>(
    entries: &mut HashMap<(&'a std::path::Path, &'a str), String>,
    library: &'a Library,
    trait_name: &'a str,
    label: &str,
) -> Result<(), StartError> {
    if let Some(other) = entries.insert((library.path.as_path(), trait_name), label.to_owned()) {
        return Err(refused(format!(
            "{label:?} and {other:?} would both take the entry symbol `{}_{trait_name}` of the C provider `{}` of package `{}`",
            library.name, library.name, library.package
        )));
    }
    Ok(())
}

/// Queues the handle interface `h` with the library of the trait that
/// returns it, once, or refuses the run when traits of two libraries
/// return it (`cli.host.c.handle-library`).
fn queue<'a>(
    queued: &mut HashMap<usize, &'a Library>,
    todo: &mut Vec<(usize, &'a Library)>,
    h: usize,
    library: &'a Library,
    handle: &Interface,
) -> Result<(), StartError> {
    match queued.get(&h) {
        Some(other) if other.path != library.path => Err(refused(format!(
            "the handle trait {:?} is returned by C providers of two libraries, `{}` and `{}`",
            handle.identity.to_string(),
            other.name,
            library.name
        ))),
        Some(_) => Ok(()),
        None => {
            queued.insert(h, library);
            todo.push((h, library));
            Ok(())
        }
    }
}

/// The C providers of an instance's C-bound root interfaces and the checked
/// tables of its C-bound handle interfaces, by contract interface.
#[derive(Default)]
pub(crate) struct CBound {
    pub(crate) providers: Vec<Option<Provider>>,
    pub(crate) handles: Vec<Option<Arc<CTable>>>,
}

/// The C call kinds of a slot's parameters, `self` first, as its row of the
/// C value table passes them (`module.host.package.c-references`).
fn c_kinds(row: &hd_host_abi::c::Method) -> Vec<Kind> {
    let pointer = Kind::Int(u8::try_from(std::mem::size_of::<usize>()).unwrap_or(8));
    let mut kinds = vec![pointer];
    for (_, p) in &row.params {
        match *p {
            Param::Bool => kinds.push(Kind::Int(1)),
            Param::Num(Num::F32) => kinds.push(Kind::F32),
            Param::Num(Num::F64) => kinds.push(Kind::F64),
            Param::Num(Num::Usize) => kinds.push(pointer),
            Param::Num(n) => kinds.push(Kind::Int(n.bytes())),
            Param::Ref { .. } => kinds.push(pointer),
            Param::List { .. } | Param::Str => kinds.extend([pointer, pointer]),
        }
    }
    kinds
}

/// The guest heap's base address and size, which a C call's pointer
/// arguments are offsets into.
fn heap(caller: &mut wasmtime::Caller<'_, Host>) -> Result<(usize, usize), Stop> {
    match caller.data().c_heap.clone() {
        Some(wasmtime::Extern::Memory(memory)) => Ok((
            memory.data_ptr(&*caller) as usize,
            memory.data_size(&*caller),
        )),
        Some(wasmtime::Extern::SharedMemory(memory)) => {
            let data = memory.data();
            Ok((data.as_ptr() as usize, data.len()))
        }
        _ => Err(Stop::Contract(
            "a C provider's call needs the guest heap, which the module does not export".into(),
        )),
    }
}

/// Defines the import of the C-bound method `row`, method `index` and
/// slot `slot` of interface `interface`: each Wasm parameter becomes its C
/// argument, a pointer offset by the guest heap's base after a bounds
/// check, and the slot's result becomes the import's
/// (`module.host.package.c-types`). A suspending method's import is its
/// `.start`: the slot takes a new token second and returns at once, and
/// the call waits in the instance's reactor for the token's completion
/// (`native::c_token`).
#[allow(unsafe_code)]
fn define_c(
    linker: &mut Linker<Host>,
    (m, n): (&str, &str),
    ty: wasmtime::FuncType,
    (interface, index, on_handle, slot, label): (usize, usize, bool, usize, String),
    (row, returned): (&hd_host_abi::c::Method, Option<usize>),
) -> Result<(), StartError> {
    let mut kinds = c_kinds(row);
    // A suspending slot's completion, and its token after `self`.
    let expect = if row.suspends {
        let Some(expect) = crate::native::c_token::Expect::of(&row.result) else {
            unreachable!("ruled out: module.host.package.c-suspend (hd_check header::c_result)")
        };
        kinds.insert(1, kinds[0]);
        Some(expect)
    } else {
        None
    };
    let float = !row.suspends && matches!(row.result, Ret::Num(Num::F32 | Num::F64));
    let plan = Plan::new(&kinds, float).ok_or_else(|| {
        refused(format!(
            "{label} takes more arguments than this hd passes to a C provider: past the registers, at most {} stack words until #2388 step 6",
            crate::native::c_call::STACK_WORDS
        ))
    })?;
    // A handle method's import takes the handle's number first; a
    // `.start` gives its status and the pending call's number.
    let wasm_params = kinds.len() - 1 - usize::from(row.suspends) + usize::from(on_handle);
    let gives = if row.suspends {
        2
    } else {
        usize::from(!matches!(row.result, Ret::Void))
    };
    if matches!(row.result, Ret::Handle(_)) && returned.is_none() {
        return Err(refused(format!(
            "{label} returns a handle whose trait has no C table in this module"
        )));
    }
    if ty.params().len() != wasm_params || ty.results().len() != gives {
        return Err(refused(format!(
            "the import `{m}` `{n}` of C provider method {label} has an unexpected signature"
        )));
    }
    let params_of = row.params.iter().map(|(_, p)| *p).collect::<Vec<_>>();
    let result = row.result.clone();
    // The registry's interface number of this handle trait.
    let interface_index = u32::try_from(interface).unwrap_or(u32::MAX);
    linker
        .func_new(m, n, ty, move |mut caller, params, out| {
            let fail = |why: &str| Stop::Contract(format!("{label}: {why}"));
            let (base, size) = heap(&mut caller)?;
            let int = |k: usize| params.get(k).and_then(wasmtime::Val::i32).unwrap_or(0);
            let host = caller.data_mut();
            // The slot and its `self`: the root's provider, or the handle
            // object of a live number this instance issued for this trait
            // (`native-handle-check`).
            let (table, this) = if on_handle {
                let number = u32::try_from(int(0)).unwrap_or(0);
                let handle = host
                    .handles
                    .host::<crate::native::c::Handle>(number, interface_index)
                    .ok_or_else(|| {
                        Stop::Contract(crate::native::handles::fail("call of an invalid handle").0)
                    })?;
                (&handle.table, handle.this)
            } else {
                let Some(provider) = host.c_providers.get(interface).and_then(Option::as_ref)
                else {
                    unreachable!("ruled out: c_preflight created every C-bound root's provider")
                };
                (provider.table(), provider.this())
            };
            // A suspending method's abort slot follows its own.
            let (f, keep) = (table.slot(slot), expect.map(|_| Arc::clone(table)));
            let Some(f) = f else {
                unreachable!("ruled out: c_preflight refused an empty slot of a called method")
            };
            let mut args = Vec::with_capacity(plan_len(&params_of) + 1);
            args.push(Arg::Int(this as u64));
            let int = |k: usize| int(k + usize::from(on_handle));
            // An offset `at` of `bytes` bytes of a number `align` bytes wide,
            // aligned for it and inside the heap, as an address: C may not
            // read a misaligned number (`module.host.package.c-aligned`).
            // The message is the JavaScript glue's.
            let place = |at: i32, bytes: u64, align: u8| -> Result<u64, Stop> {
                let at = u64::from(at.cast_unsigned());
                if !at.is_multiple_of(u64::from(align)) {
                    return Err(Stop::Contract(
                        crate::native::wire::fail("misaligned C pointer argument").0,
                    ));
                }
                at.checked_add(bytes)
                    .filter(|end| *end <= size as u64)
                    .map(|_| base as u64 + at)
                    .ok_or_else(|| fail("a pointer argument leaves the guest heap"))
            };
            let mut k = 0;
            for p in &params_of {
                match *p {
                    Param::Bool => args.push(Arg::Int(u64::from(int(k) != 0))),
                    Param::Num(Num::F32) => {
                        args.push(Arg::F32(
                            params.get(k).and_then(wasmtime::Val::f32).unwrap_or(0.0),
                        ));
                    }
                    Param::Num(Num::F64) => {
                        args.push(Arg::F64(
                            params.get(k).and_then(wasmtime::Val::f64).unwrap_or(0.0),
                        ));
                    }
                    Param::Num(Num::I64 | Num::U64) => args.push(Arg::Int(
                        params
                            .get(k)
                            .and_then(wasmtime::Val::i64)
                            .unwrap_or(0)
                            .cast_unsigned(),
                    )),
                    Param::Num(num) => args.push(Arg::Int(extend(num, int(k)))),
                    Param::Ref { num, .. } => {
                        args.push(Arg::Int(place(
                            int(k),
                            u64::from(num.bytes()),
                            num.bytes(),
                        )?));
                    }
                    Param::List { num, .. } => {
                        let len = u64::from(int(k + 1).cast_unsigned());
                        let bytes = len
                            .checked_mul(u64::from(num.bytes()))
                            .ok_or_else(|| fail("a list argument leaves the guest heap"))?;
                        args.push(Arg::Int(place(int(k), bytes, num.bytes())?));
                        args.push(Arg::Int(len));
                        k += 1;
                    }
                    Param::Str => {
                        let len = u64::from(int(k + 1).cast_unsigned());
                        args.push(Arg::Int(place(int(k), len, 1)?));
                        args.push(Arg::Int(len));
                        k += 1;
                    }
                }
                k += 1;
            }
            if let (Some(expect), Some(keep)) = (expect, keep) {
                let Some(abort) = keep.slot(slot + 1) else {
                    unreachable!("ruled out: c_preflight refused an empty abort slot")
                };
                let slots = crate::native::c_token::Slots {
                    this,
                    call: f,
                    abort,
                    keep: Some(keep),
                };
                // SAFETY: as the call below; a suspending slot takes only
                // numbers (`module.host.package.c-suspend`), and its `self`
                // lives until the call ends, since the reactor that holds
                // the call drops before the providers and handles.
                let call = unsafe { crate::native::c_token::start(slots, &plan, args, expect) };
                let h = caller
                    .data_mut()
                    .reactor
                    .native(call, (interface, index))
                    .ok_or_else(|| fail("the call ended before it was registered"))?;
                if let [status, handle] = out {
                    *status = wasmtime::Val::I32(1);
                    *handle = wasmtime::Val::I32(h);
                }
                return Ok(());
            }
            // SAFETY: the table's contract hash matched the module's C
            // table (`c_preflight`), so the slot takes these parameters and
            // returns this result; each pointer lies inside the guest heap,
            // which only this instance's thread grows (`cli.host.c.thread`).
            let raw = unsafe { crate::native::c_call::call(f, &plan, &args) };
            if let Some(handle) = returned {
                // A returned handle object joins the registry under its
                // trait; its last release runs its table's `destroy`.
                // The result register holds a pointer of this target's width.
                let this = usize::try_from(raw).unwrap_or(0) as *mut std::ffi::c_void;
                if this.is_null() {
                    return Err(fail("the C provider returned no handle").into());
                }
                let host = caller.data_mut();
                let Some(table) = host.c_handles.get(handle).and_then(Option::as_ref) else {
                    unreachable!("ruled out: c_preflight checked every reachable handle table")
                };
                let value = crate::native::c::Handle {
                    table: Arc::clone(table),
                    this,
                };
                let index = u32::try_from(handle).unwrap_or(u32::MAX);
                let number = host
                    .handles
                    .insert_host(index, Box::new(value))
                    .map_err(|e| Stop::Contract(e.0))?;
                if let Some(slot) = out.first_mut() {
                    *slot = wasmtime::Val::I32(number.cast_signed());
                }
                return Ok(());
            }
            if let Some(slot) = out.first_mut() {
                *slot = c_result(&result, raw);
            }
            Ok(())
        })
        .map_err(|error| StartError::Engine(error.to_string()))?;
    Ok(())
}

/// How many C arguments `params` take after `self`.
fn plan_len(params: &[Param]) -> usize {
    params
        .iter()
        .map(|p| match p {
            Param::List { .. } | Param::Str => 2,
            _ => 1,
        })
        .sum()
}

/// A narrow integer argument, from its Wasm `i32`, extended to 64 bits by
/// its type's signedness. Each cast keeps the bits of the narrow type, as
/// meant.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn extend(num: Num, value: i32) -> u64 {
    match num {
        Num::I8 => i64::from(value as i8).cast_unsigned(),
        Num::I16 => i64::from(value as i16).cast_unsigned(),
        Num::I32 => i64::from(value).cast_unsigned(),
        Num::U8 => u64::from(value as u8),
        Num::U16 => u64::from(value as u16),
        _ => u64::from(value.cast_unsigned()),
    }
}

/// A C slot's result register `raw` as its Wasm value: `bool` from its low
/// byte, a narrow integer truncated and extended by its signedness, a
/// 64-bit one whole, a float from its register's bits. Each cast keeps the
/// bits of the result's type, as meant.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
fn c_result(result: &Ret, raw: u64) -> wasmtime::Val {
    let low = raw as u32;
    match result {
        Ret::Bool => wasmtime::Val::I32(i32::from(raw as u8 != 0)),
        Ret::Num(Num::F32) => wasmtime::Val::F32(low),
        Ret::Num(Num::F64) => wasmtime::Val::F64(raw),
        Ret::Num(Num::I64 | Num::U64) => wasmtime::Val::I64(raw.cast_signed()),
        Ret::Num(Num::I8) => wasmtime::Val::I32(i32::from(raw as i8)),
        Ret::Num(Num::I16) => wasmtime::Val::I32(i32::from(raw as i16)),
        Ret::Num(Num::U8) => wasmtime::Val::I32(i32::from(raw as u8)),
        Ret::Num(Num::U16) => wasmtime::Val::I32(i32::from(raw as u16)),
        Ret::Num(_) => wasmtime::Val::I32(low.cast_signed()),
        // ruled out: module.host.package.c-types gives a method that
        // returns at once no bytes or `Result`, and `define_c` gives a
        // `void` method no result.
        Ret::Void | Ret::Bytes | Ret::Result(_) | Ret::Handle(_) => wasmtime::Val::I32(0),
    }
}

/// The handle number a handle method or `$release` gets first. The
/// registry issues positive numbers only, so a negative one reads as 0,
/// which names no handle.
fn handle_number(params: &[wasmtime::Val]) -> wasmtime::Result<u32> {
    Ok(u32::try_from(int_arg(params, 0)?).unwrap_or(0))
}

/// Defines each module import of a method that preflight linked to
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
        // A C-bound method's call or `.start` takes C arguments
        // (`define_c`); its `.finish` and a handle's `$release` are any
        // package method's, below.
        if let Some(c) = package
            .c_interface(interface)
            .filter(|_| matches!(part, Part::Call | Part::Start))
        {
            let Some(&(slot, row)) = c.slots.get(&method.name) else {
                unreachable!("ruled out: a C table lists every method of its trait")
            };
            let row = &c.table.methods[row];
            let returned = match &row.result {
                Ret::Handle(name) => package.c_handles.get(name).copied(),
                _ => None,
            };
            define_c(
                &mut *linker,
                (m, n),
                ty,
                (interface, index, declared.handle, slot, label),
                (row, returned),
            )?;
            defined.insert((m.to_owned(), n.to_owned()));
            continue;
        }
        // `.start` and a plain call take one argument length when the
        // method takes any argument, after the handle a handle method is
        // called on; `.finish` and `$release` take a handle. A plain call
        // has one result length unless it returns `()` or never returns,
        // `.start` a status and its length or handle, `.finish` a length,
        // `$release` nothing.
        let on_handle = declared.handle && matches!(part, Part::Call | Part::Start);
        let takes = match part {
            Part::Finish | Part::Release => 1,
            _ => usize::from(on_handle) + usize::from(!method.params.is_empty()),
        };
        let gives = match (part, &method.result) {
            (Part::Release, _) => Some(0),
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
        // The handle trait's index in the registry; contracts hold far
        // fewer interfaces than `u32::MAX`.
        let trait_index = u32::try_from(interface).unwrap_or(u32::MAX);
        linker
            .func_new(m, n, ty, move |mut caller, params, out| {
                let fail = |why: &str| Stop::Contract(format!("{label}: {why}"));
                // The JS host names no method for a bad handle.
                let invalid = |error: HostError| Stop::Contract(error.0);
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
                    Part::Release => {
                        // The last hd reference is gone: the trait object
                        // drops with no provider call (`native-handle-release`).
                        let number = handle_number(params)?;
                        caller
                            .data_mut()
                            .handles
                            .release(number, trait_index)
                            .map_err(invalid)?;
                        return Ok(());
                    }
                    Part::Finish => {
                        let h = handle(int_arg(params, 0)?);
                        let host = caller.data_mut();
                        host.reactor
                            .finish_native(h, call, &mut host.handles)
                            .ok_or_else(|| fail("a finish of an operation that is not pending"))?
                            .map_err(|failure| Stop::Contract(failure.message(&label)))?
                    }
                    Part::Call | Part::Start => {
                        let args = if params.len() == usize::from(on_handle) {
                            Vec::new()
                        } else {
                            bytes(&mut caller, int_arg(params, usize::from(on_handle))?)?
                        };
                        let host = caller.data_mut();
                        let answer = if on_handle {
                            // A handle this instance issued for this trait
                            // and has not released, or no method runs
                            // (`native-handle-check`). Its trait object
                            // leaves its slot for the call, so a handle
                            // the result returns can enter the registry.
                            let mut lent = host
                                .handles
                                .lend(handle_number(params)?, trait_index)
                                .map_err(invalid)?;
                            let answer = (lent.dispatch)(
                                lent.value.as_mut(),
                                index,
                                &args,
                                &mut host.handles,
                            );
                            host.handles.restore(lent);
                            answer
                        } else {
                            let Some(native) = host.natives[interface].as_mut() else {
                                unreachable!("ruled out: preflight links every root interface")
                            };
                            (native.dispatch)(
                                native.provider.as_mut(),
                                index,
                                &args,
                                &mut host.handles,
                            )
                        }
                        .map_err(|failure| Stop::Contract(failure.message(&label)))?;
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
