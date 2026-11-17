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
//! What builds on this surface: #2078 indexes the registrations by
//! identity and links the package imports at instantiation; #2079 adds the
//! generated codecs and the typed [`Serde`] transport; #2080 checks consent
//! in the same preflight; #2081 drives [`PendingCall`] from the reactor and
//! gives [`CallContext`] the instance identity; #2082 keeps returned handle
//! trait objects in the instance's registry.

use std::any::Any;
use std::sync::mpsc;

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
/// width and sign, floats their bits. #2079 encodes and decodes it.
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

/// One root provider: its trait's nominal identity, its import module, and
/// the generated trait object (`Box<dyn Trait>`, boxed again as `Any`).
pub struct Registration {
    pub identity: &'static str,
    pub module: &'static str,
    pub provider: Box<dyn Any>,
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

    /// Adds the provider of one root trait.
    pub fn register(
        &mut self,
        identity: &'static str,
        module: &'static str,
        provider: Box<dyn Any>,
    ) {
        self.registrations.push(Registration {
            identity,
            module,
            provider,
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
