//! C providers on Wasmtime (#2388 step 4, `docs/future-work/FFI.md`,
//! "Binding on Wasmtime"): before the guest starts, the host opens each
//! C-bound root trait's library, takes its one entry symbol, checks the
//! table's `abi_version` and `contract_hash` against the module's `hd.c`
//! table, and calls `create`; the instance's close calls `destroy` once,
//! trap or not (`cli.host.c.entry`, `cli.host.c.check`, `cli.host.c.create`).
//! Each called slot is linked to its import with the C arguments the
//! guest passes ([`super::c_call`]). Every slot runs on the instance's
//! thread (`cli.host.c.thread`); a suspending method's slot takes a token,
//! which the library completes from any thread ([`super::c_token`]).
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

/// A checked C table: its head and slots, and the library that holds it,
/// kept open while the table is used. One per C-bound trait of an
/// instance, root or returned handle (`cli.host.c.check`).
pub(crate) struct CTable {
    head: *const Head,
    _library: Arc<libloading::Library>,
}

// SAFETY: a table is read-only C data the library keeps for its life,
// which the `Arc` holds; slots run only on the instance's thread
// (`cli.host.c.thread`).
#[allow(unsafe_code)]
unsafe impl Send for CTable {}
#[allow(unsafe_code)]
unsafe impl Sync for CTable {}

impl CTable {
    /// The function pointer of slot `index`, or `None` when the library
    /// left it null.
    #[allow(unsafe_code)]
    pub(crate) fn slot(&self, index: usize) -> Option<*const c_void> {
        // SAFETY: `table` checked the hash, so the library built this table
        // from the same C table: its head, then one pointer per slot.
        let slot = unsafe { self.head.add(1).cast::<*const c_void>().add(index).read() };
        (!slot.is_null()).then_some(slot)
    }

    /// Runs the table's `destroy` on `this`, when the library gave one.
    #[allow(unsafe_code)]
    fn destroy(&self, this: *mut c_void) {
        // SAFETY: `table` checked the table, and each owner calls `destroy`
        // once, as it drops (`cli.host.c.create`).
        unsafe {
            if let Some(destroy) = (*self.head).destroy {
                destroy(this);
            }
        }
    }
}

/// A created C provider of one root trait: its table and its `self`.
pub(crate) struct Provider {
    table: Arc<CTable>,
    this: *mut c_void,
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

    /// The provider's checked table.
    pub(crate) fn table(&self) -> &Arc<CTable> {
        &self.table
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.table.destroy(self.this);
    }
}

/// A returned C handle: its handle trait's table and the library's handle
/// object, in the instance's handle registry; its last release drops it,
/// which runs the table's `destroy` (`module.host.package.c-handles`).
pub(crate) struct Handle {
    pub(crate) table: Arc<CTable>,
    pub(crate) this: *mut c_void,
}

// SAFETY: as [`Provider`]: a handle lives in its instance's registry and
// is used only on the instance's thread.
#[allow(unsafe_code)]
unsafe impl Send for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        self.table.destroy(self.this);
    }
}

/// Opens the library of `library` once (`cli.host.c.file`); a library
/// several traits share is one handle.
#[allow(unsafe_code)]
pub(crate) fn load(library: &Library, label: &str) -> Result<Arc<libloading::Library>, String> {
    // SAFETY: loading a library runs its initializers: a C provider has
    // the process's authority, which the app's root consent granted.
    unsafe { libloading::Library::new(&library.path) }
        .map(Arc::new)
        .map_err(|why| format!("{} cannot be loaded for {label}: {why}", at(library)))
}

fn at(library: &Library) -> String {
    format!(
        "the C provider `{}` of package `{}` for target `{}` ({})",
        library.name,
        library.package,
        library.triple,
        library.path.display()
    )
}

/// Takes `table`'s entry symbol `NAME_TRAIT` from `lib`, and checks its
/// `abi_version` and `contract_hash` (`cli.host.c.entry`,
/// `cli.host.c.check`); or says why the run is refused, naming the package,
/// the triple and the trait `label`.
#[allow(unsafe_code)]
pub(crate) fn table(
    lib: &Arc<libloading::Library>,
    library: &Library,
    label: &str,
    table: &Table,
) -> Result<Arc<CTable>, String> {
    let entry = format!("{}_{}", library.name, table.trait_name);
    // SAFETY: the entry symbol's type is the C header's,
    // `const hd_TRAIT *NAME_TRAIT(void)`.
    let head = unsafe {
        let symbol = lib
            .get::<unsafe extern "C" fn() -> *const Head>(entry.as_bytes())
            .map_err(|_| format!("{} exports no `{entry}` for {label}", at(library)))?;
        symbol()
    };
    if head.is_null() {
        return Err(format!(
            "{}'s `{entry}` returned no table for {label}",
            at(library)
        ));
    }
    // SAFETY: the entry returned a table, whose head every version shares.
    let (version, hash) = unsafe { ((*head).abi_version, (*head).contract_hash) };
    if version != hd_host_abi::c::ABI_VERSION {
        return Err(format!(
            "{} has C ABI version {version} for {label}; this hd needs version {}",
            at(library),
            hd_host_abi::c::ABI_VERSION
        ));
    }
    let expected = table.hash();
    if hash != expected {
        return Err(format!(
            "{} was built for another version of {label}: its contract hash is {}, the module's is {}; rebuild it against the header `hd build` writes",
            at(library),
            hex(&hash),
            hex(&expected)
        ));
    }
    Ok(Arc::new(CTable {
        head,
        _library: Arc::clone(lib),
    }))
}

/// Creates the provider of a root trait's checked `table` with its
/// `create` (`cli.host.c.create`).
#[allow(unsafe_code)]
pub(crate) fn create(
    table: Arc<CTable>,
    library: &Library,
    label: &str,
) -> Result<Provider, String> {
    // SAFETY: `table` checked the head.
    let create = unsafe { (*table.head).create }
        .ok_or_else(|| format!("{}'s table for {label} has no `create`", at(library)))?;
    // SAFETY: the table's hash matched, so `create` is the C header's.
    let this = unsafe { create() };
    Ok(Provider { table, this })
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}
