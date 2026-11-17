//! The embedder surface for native package host providers
//! (`cli.host.package.native-registration`,
//! `docs/future-work/NATIVE_PACKAGE_HOST_BINDINGS.md`).
//!
//! A module's generated companion `NAME.host.rs` (`hd build --engine
//! wasmtime`) declares one provider trait per host interface over the types
//! here, and its `Bindings::builder()...build()` yields a [`Bindings`]: the
//! one registry, passed by value, that
//! [`WasmtimeEngine::instantiate_with_bindings`](crate::WasmtimeEngine::instantiate_with_bindings)
//! takes. Hd never discovers or loads a library
//! (`cli.host.package.native-no-discovery`).
//!
//! [`WasmtimeEngine::instantiate_with_bindings`](crate::WasmtimeEngine::instantiate_with_bindings)
//! resolves the registrations by nominal identity and links each root
//! method's import to its registration's [`Dispatch`] before the guest
//! starts. The companion's dispatch decodes each call's arguments and
//! encodes its result with the codecs of [`wire`]. What builds on this
//! surface: #2080 checks consent in the same preflight; #2081 drives
//! [`PendingCall`] from the reactor and gives [`CallContext`] the instance
//! identity; #2082 keeps returned handle trait objects in the instance's
//! registry.

use std::any::Any;
use std::sync::mpsc;

pub mod wire;

/// A provider method's failure: a `host-contract` failure of the call
/// (`cli.host.package.native-call-failure`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostError(pub String);

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for HostError {}

pub type HostResult<T> = Result<T, HostError>;

/// What a suspending provider method returns
/// (`cli.host.package.native-completion-forms`).
pub enum NativeCall<T> {
    Ready(HostResult<T>),
    Pending(PendingCall<T>),
}

/// The call side of a pending native call. It stays on the instance's
/// thread; only its [`Completion`] may cross threads
/// (`cli.host.package.native-thread`).
pub struct PendingCall<T> {
    result: Option<mpsc::Receiver<HostResult<T>>>,
    abort: Option<Box<dyn FnOnce()>>,
}

/// The one-shot completion sender of a [`PendingCall`]: `Send` when its
/// value is.
pub struct Completion<T> {
    sender: mpsc::SyncSender<HostResult<T>>,
}

/// A pending call and its completion sender; `abort` runs at most once, on
/// [`PendingCall::abort`] (`cli.host.package.native-abort`).
pub fn pending<T>(abort: impl FnOnce() + 'static) -> (PendingCall<T>, Completion<T>) {
    // Capacity one: the single completion never blocks its sender.
    let (sender, receiver) = mpsc::sync_channel(1);
    (
        PendingCall {
            result: Some(receiver),
            abort: Some(Box::new(abort)),
        },
        Completion { sender },
    )
}

impl<T> Completion<T> {
    /// Delivers the call's value. After the call's abort it is dropped
    /// (`cli.host.package.native-late-completion`).
    pub fn complete(self, value: HostResult<T>) {
        // A closed receiver is an aborted call: the value is ignored.
        let _ = self.sender.send(value);
    }
}

impl<T> PendingCall<T> {
    /// The delivered value, once; `None` while pending or after abort.
    pub fn take(&mut self) -> Option<HostResult<T>> {
        let value = self.result.as_ref()?.try_recv().ok()?;
        self.result = None;
        self.abort = None;
        Some(value)
    }

    /// Aborts the call: its hook runs once, its receiver closes, and a
    /// later completion is ignored. A second abort does nothing.
    pub fn abort(&mut self) {
        self.result = None;
        if let Some(hook) = self.abort.take() {
            hook();
        }
    }
}

/// The context of one suspending provider call. #2081 adds its instance
/// identity and readiness wake.
pub struct CallContext<'a> {
    _call: std::marker::PhantomData<&'a mut ()>,
}

impl CallContext<'_> {
    /// The context of a call being dispatched: the engine's, or a test
    /// driving a provider directly.
    #[must_use]
    pub fn new() -> Self {
        Self {
            _call: std::marker::PhantomData,
        }
    }
}

impl Default for CallContext<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// A private-field value as a typed Serde tree
/// (`module.host.package.native-private-serde`): integers keep their
/// width and sign, floats their bits.
///
/// A value from the guest holds the JSON data model its `Serialize` writes:
/// `None` for null, `Bool`, `I64` or `U64`, finite `F64`, `String`, `Seq`,
/// and `Map` with distinct `String` keys. A provider may also return
/// `Unit` (null), `Some` (its value), `F32` (widened), a non-finite float
/// (null, as the JS host writes it), `Char` (text) and `Bytes` (a list of
/// `u8`); a map key must be distinct `String` or `Char` text.
#[derive(Clone, Debug, PartialEq)]
pub enum Serde {
    Unit,
    Bool(bool),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    Char(char),
    String(String),
    Bytes(Vec<u8>),
    None,
    Some(Box<Self>),
    Seq(Vec<Self>),
    Map(Vec<(Self, Self)>),
}

/// A root host trait left without a provider when its registry was built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unregistered(pub String);

impl Unregistered {
    #[must_use]
    pub fn new(identity: &str) -> Self {
        Self(identity.to_owned())
    }
}

impl std::fmt::Display for Unregistered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no native provider registered for package host trait {:?}",
            self.0
        )
    }
}

impl std::error::Error for Unregistered {}

/// The byte-level entry of one root provider: calls the method at index
/// `method` of its interface in the contract, with that method's encoded
/// arguments, and returns its encoded result (empty for `()`). The
/// companion generates one per root trait; it downcasts `provider` to the
/// trait object it registered.
pub type Dispatch = fn(provider: &mut dyn Any, method: usize, args: &[u8]) -> HostResult<Vec<u8>>;

/// One root provider: its trait's nominal identity, its import module, the
/// generated trait object (`Box<dyn Trait>`, boxed again as `Any`), and its
/// dispatch.
pub struct Registration {
    pub identity: &'static str,
    pub module: &'static str,
    pub provider: Box<dyn Any>,
    pub dispatch: Dispatch,
}

/// The providers an embedder passes, by value, to one instantiation. The
/// companion's builder fills it in its contract's root order.
pub struct Bindings {
    contract: &'static [u8],
    registrations: Vec<Registration>,
}

impl Bindings {
    /// An empty registry for the module whose contract is `contract`.
    #[must_use]
    pub fn new(contract: &'static [u8]) -> Self {
        Self {
            contract,
            registrations: Vec::new(),
        }
    }

    /// Adds the provider of one root trait and the dispatch its calls go
    /// through.
    pub fn register_dispatch(
        &mut self,
        identity: &'static str,
        module: &'static str,
        provider: Box<dyn Any>,
        dispatch: Dispatch,
    ) {
        self.registrations.push(Registration {
            identity,
            module,
            provider,
            dispatch,
        });
    }

    /// The contract the companion was generated from.
    #[must_use]
    pub fn contract(&self) -> &'static [u8] {
        self.contract
    }

    #[must_use]
    pub fn registrations(&self) -> &[Registration] {
        &self.registrations
    }

    /// The registrations, for the instance that takes them over.
    #[must_use]
    pub fn into_registrations(self) -> Vec<Registration> {
        self.registrations
    }
}
