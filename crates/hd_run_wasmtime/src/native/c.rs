//! C providers on Wasmtime (#2388 step 4, `docs/future-work/FFI.md`,
//! "Binding on Wasmtime"): before the guest starts, the host opens each
//! C-bound root trait's library, takes its one entry symbol, checks the
//! table's `abi_version` and `contract_hash` against the module's `hd.c`
//! table, and calls `create`; the instance's close calls `destroy` once,
//! trap or not (`cli.host.c.entry`, `cli.host.c.check`, `cli.host.c.create`).
//! Each called slot is linked to its import with the C arguments the
//! guest passes ([`super::c_call`]). Every slot runs on the instance's
//! thread (`cli.host.c.thread`).
//!
//! Cost. Startup opens each library once and probes one symbol per
//! trait; checking a table compares 16 bytes. A call converts each
//! argument once and makes one indirect call.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Arc;

use hd_host_abi::c::Table;

/// One C-bound root trait's library, as the invoker selected it from the
/// declaring package's `native/<target-triple>/` folder
/// (`cli.host.c.file`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Library {
    /// The trait's import module in the package host contract.
    pub module: String,
    /// The library file.
    pub path: PathBuf,
    /// The library's name, the `c` of the trait's `[host]` entry, which
    /// names its entry symbol `NAME_TRAIT`.
    pub name: String,
    /// The declaring package, as a refusal names it.
    pub package: String,
    /// The target triple whose folder holds the library.
    pub triple: String,
}

/// The fixed head of every C table: the C header's `abi_version`,
/// `contract_hash`, `create` and `destroy`, laid out as C lays them out.
#[repr(C)]
struct Head {
    abi_version: u32,
    contract_hash: [u8; 16],
    create: Option<unsafe extern "C" fn() -> *mut c_void>,
    destroy: Option<unsafe extern "C" fn(*mut c_void)>,
}

/// A created C provider of one root trait: its table, its `self`, and the
/// library that holds both, kept open while the provider lives.
pub(crate) struct Provider {
    table: *const Head,
    this: *mut c_void,
    _library: Arc<libloading::Library>,
}

// SAFETY: a provider moves with its instance's store, and every slot,
// `destroy` included, runs on the thread that drives the store
// (`cli.host.c.thread`); no two threads use one provider at once.
#[allow(unsafe_code)]
unsafe impl Send for Provider {}

impl Provider {
    /// The provider's `self`, which each slot takes first.
    pub(crate) fn this(&self) -> *mut c_void {
        self.this
    }

    /// The function pointer of slot `index`, or `None` when the library
    /// left it null.
    #[allow(unsafe_code)]
    pub(crate) fn slot(&self, index: usize) -> Option<*const c_void> {
        // SAFETY: `open` checked the table's hash, so the library built it
        // from the same C table: its head, then one pointer per slot.
        let slot = unsafe { self.table.add(1).cast::<*const c_void>().add(index).read() };
        (!slot.is_null()).then_some(slot)
    }
}

impl Drop for Provider {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: `open` checked the table, and `destroy` runs once, as the
        // provider drops with its instance (`cli.host.c.create`).
        unsafe {
            if let Some(destroy) = (*self.table).destroy {
                destroy(self.this);
            }
        }
    }
}

/// Opens the library of `library` for the trait `label`, takes its entry
/// symbol, checks its table against `table`, and creates its provider; or
/// says why the run is refused, naming the package, the triple and the
/// trait.
#[allow(unsafe_code)]
pub(crate) fn open(library: &Library, label: &str, table: &Table) -> Result<Provider, String> {
    let at = || {
        format!(
            "the C provider `{}` of package `{}` for target `{}` ({})",
            library.name,
            library.package,
            library.triple,
            library.path.display()
        )
    };
    // SAFETY: loading a library runs its initializers: a C provider has
    // the process's authority, which the app's root consent granted.
    let lib = unsafe { libloading::Library::new(&library.path) }
        .map_err(|why| format!("{} cannot be loaded for {label}: {why}", at()))?;
    let entry = format!("{}_{}", library.name, table.trait_name);
    // SAFETY: the entry symbol's type is the C header's,
    // `const hd_TRAIT *NAME_TRAIT(void)`.
    let head = unsafe {
        let symbol = lib
            .get::<unsafe extern "C" fn() -> *const Head>(entry.as_bytes())
            .map_err(|_| format!("{} exports no `{entry}` for {label}", at()))?;
        symbol()
    };
    if head.is_null() {
        return Err(format!(
            "{}'s `{entry}` returned no table for {label}",
            at()
        ));
    }
    // SAFETY: the entry returned a table, whose head every version shares.
    let (version, hash, create) =
        unsafe { ((*head).abi_version, (*head).contract_hash, (*head).create) };
    if version != hd_host_abi::c::ABI_VERSION {
        return Err(format!(
            "{} has C ABI version {version} for {label}; this hd needs version {}",
            at(),
            hd_host_abi::c::ABI_VERSION
        ));
    }
    let expected = table.hash();
    if hash != expected {
        return Err(format!(
            "{} was built for another version of {label}: its contract hash is {}, the module's is {}; rebuild it against the header `hd build` writes",
            at(),
            hex(&hash),
            hex(&expected)
        ));
    }
    let create = create.ok_or_else(|| format!("{}'s table for {label} has no `create`", at()))?;
    // SAFETY: the table's hash matched, so `create` is the C header's.
    let this = unsafe { create() };
    Ok(Provider {
        table: head,
        this,
        _library: Arc::new(lib),
    })
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}
