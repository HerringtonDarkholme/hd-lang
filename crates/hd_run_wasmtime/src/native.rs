//! The embedder surface for native package host providers
//! (`cli.host.package.native-registration`,
//! `docs/future-work/NATIVE_PACKAGE_HOST_BINDINGS.md` at 10e74b76a).
//!
//! A module's generated companion `NAME.host.rs` (`hd build --engine
//! wasmtime`) declares one provider trait per host interface over the types
//! here, and its `Bindings::builder()...build()` yields a [`Bindings`]: the
//! one registry, passed by value, that
//! [`WasmtimeEngine::instantiate_with_bindings`](crate::WasmtimeEngine::instantiate_with_bindings)
//! takes. Hd loads a library only for a C provider that a package's
//! `[host]` table names ([`c`], `cli.host.package.native-library-scope`).
//!
//! [`WasmtimeEngine::instantiate_with_bindings`](crate::WasmtimeEngine::instantiate_with_bindings)
//! resolves the registrations by nominal identity and links each root
//! method's import to its registration's [`Dispatch`] before the guest
//! starts, and runs a registered provider only when the app manifest's
//! consent in `HostSetup::grants` grants its trait
//! (`cli.host.package.native-two-keys`): an embedder resolves it from its
//! manifest with `hd_run::Grants::resolve` or `hd_run::Consent::new`, as
//! `hd` does from `hd.toml`, and registration grants the guest nothing.
//! The companion's dispatch decodes each call's arguments and encodes its
//! result with the codecs of [`wire`]. A suspending method's
//! [`PendingCall`] is held by the instance's reactor, which its
//! [`Completion`] wakes from any thread. A returned handle trait object
//! lives in the instance's [`Handles`] until hd releases it.

use std::any::Any;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::runtime::reactor::Wake;

pub mod c;
pub(crate) mod c_call;
pub mod handles;
pub mod wire;

pub use handles::Handles;

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

/// Why a dispatched call failed. Each kind prints the JS host's text for
/// the same failure (`module.host.package.native-parity`), so the engine
/// takes it from the dispatch rather than from the message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The provider's own error, answered at once: a JS provider throwing.
    Threw(HostError),
    /// A pending call completed with the provider's error: a JS
    /// provider's promise rejecting.
    Rejected(HostError),
    /// The companion's codec or the host's own check failed.
    Host(HostError),
}

impl From<HostError> for Failure {
    fn from(error: HostError) -> Self {
        Self::Host(error)
    }
}

impl Failure {
    /// The `host-contract` panic message of this failure of the method
    /// `label` (`TRAIT.method`), as the JS host words it.
    #[must_use]
    pub fn message(&self, label: &str) -> String {
        match self {
            Self::Threw(HostError(why)) => format!("{label}: provider threw {why}"),
            Self::Rejected(HostError(why)) => {
                format!("package host: provider {label} rejected: {why}")
            }
            Self::Host(HostError(why)) => format!("{label}: {why}"),
        }
    }
}

/// A dispatch's result: its answer, or why the call failed.
pub type Dispatched = Result<Answer, Failure>;

/// What a suspending provider method returns
/// (`cli.host.package.native-completion-forms`).
pub enum NativeCall<T> {
    Ready(HostResult<T>),
    Pending(PendingCall<T>),
}

/// The one-shot slot a pending call and its completion share. Every
/// transition happens under its lock, so a completion either lands before
/// the call ends or finds it ended and wakes nothing
/// (`cli.host.package.native-late-completion`).
struct Slot<T>(Mutex<State<T>>);

enum State<T> {
    /// Not completed. Once the instance registered the call, the wake of
    /// its own reactor and handle: a completion reaches that instance and
    /// no other.
    Waiting(Option<Wake>),
    /// Completed; the instance has not taken the value.
    Done(HostResult<T>),
    /// Taken, aborted, or its instance closed: a completion is dropped.
    Ended,
}

impl<T> Slot<T> {
    fn lock(&self) -> MutexGuard<'_, State<T>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ends the call and returns what it held, for the caller to drop
    /// outside the lock.
    fn end(&self) -> State<T> {
        std::mem::replace(&mut *self.lock(), State::Ended)
    }
}

/// The call side of a pending native call. It stays on the instance's
/// thread; only its [`Completion`] may cross threads
/// (`cli.host.package.native-thread`). Dropping it undelivered aborts it,
/// so an instance's close runs the hook of every call it holds
/// (`cli.host.package.native-abort`).
pub struct PendingCall<T> {
    slot: Arc<Slot<T>>,
    abort: Option<Box<dyn FnOnce()>>,
}

/// The one-shot completion sender of a [`PendingCall`]: `Send` when its
/// value is.
pub struct Completion<T> {
    slot: Arc<Slot<T>>,
}

/// A pending call and its completion sender; `abort` runs at most once, on
/// [`PendingCall::abort`] or when the call is dropped undelivered
/// (`cli.host.package.native-abort`).
pub fn pending<T>(abort: impl FnOnce() + 'static) -> (PendingCall<T>, Completion<T>) {
    let slot = Arc::new(Slot(Mutex::new(State::Waiting(None))));
    (
        PendingCall {
            slot: Arc::clone(&slot),
            abort: Some(Box::new(abort)),
        },
        Completion { slot },
    )
}

impl<T> Completion<T> {
    /// Delivers the call's value and wakes its instance. After the call's
    /// abort or its instance's close, the value is dropped and nothing
    /// wakes (`cli.host.package.native-late-completion`).
    pub fn complete(self, value: HostResult<T>) {
        let mut state = self.slot.lock();
        if let State::Waiting(wake) = &*state {
            // The wake is sent under the slot's lock, so an abort cannot
            // end the call between the check and the wake.
            if let Some(wake) = wake {
                wake.wake();
            }
            *state = State::Done(value);
        } else {
            drop(state);
            drop(value);
        }
    }
}

impl<T> PendingCall<T> {
    /// The delivered value, once; `None` while pending or after abort.
    /// Taking it delivers the call: there is no abort left to run.
    pub fn take(&mut self) -> Option<HostResult<T>> {
        let mut state = self.slot.lock();
        if !matches!(*state, State::Done(_)) {
            return None;
        }
        let State::Done(value) = std::mem::replace(&mut *state, State::Ended) else {
            unreachable!("ruled out: matched as `Done` under the same lock")
        };
        drop(state);
        self.abort = None;
        Some(value)
    }

    /// Aborts the call: a later completion is ignored, a value delivered
    /// and not taken is dropped, and then the hook runs and its captures
    /// are released. A second abort, or one after `take`, does nothing.
    pub fn abort(&mut self) {
        if let Some(hook) = self.abort.take() {
            drop(self.slot.end());
            hook();
        }
    }

    /// Registers the call with its instance's `wake`.
    fn bind(&mut self, wake: Wake) -> Bound {
        let mut state = self.slot.lock();
        match &mut *state {
            State::Waiting(target) => {
                *target = Some(wake);
                Bound::Waiting
            }
            State::Done(_) => Bound::Done,
            State::Ended => Bound::Ended,
        }
    }
}

impl<T> Drop for PendingCall<T> {
    fn drop(&mut self) {
        self.abort();
    }
}

/// What registering a pending call with its instance found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bound {
    /// Pending: its completion wakes the instance.
    Waiting,
    /// Completed before it was registered: the instance wakes itself.
    Done,
    /// Aborted by its provider before it was returned: it never completes.
    Ended,
}

/// A suspending method's answer to a dispatch: its encoded value, or the
/// pending call the instance drives until its `.finish`.
pub enum Answer {
    Ready(Vec<u8>),
    Pending(Suspension),
}

/// A pending call with its value's codec, for the instance's reactor,
/// which holds every pending call alike.
pub struct Suspension(Box<dyn Suspended>);

impl Suspension {
    #[must_use]
    pub fn new<T: 'static>(call: PendingCall<T>, encode: Encode<T>) -> Self {
        Self(Box::new(Encoded { call, encode }))
    }

    pub(crate) fn bind(&mut self, wake: Wake) -> Bound {
        self.0.bind(wake)
    }

    /// The delivered value, encoded, with any handle it holds issued in
    /// `handles`; `None` while pending.
    pub(crate) fn finish(&mut self, handles: &mut Handles) -> Option<Result<Vec<u8>, Failure>> {
        self.0.finish(handles)
    }
}

/// A suspending method's result codec: it takes the value, since a
/// handle it holds moves into the instance's registry.
pub type Encode<T> = fn(T, &mut Handles) -> HostResult<Vec<u8>>;

trait Suspended {
    fn bind(&mut self, wake: Wake) -> Bound;
    fn finish(&mut self, handles: &mut Handles) -> Option<Result<Vec<u8>, Failure>>;
}

struct Encoded<T> {
    call: PendingCall<T>,
    encode: Encode<T>,
}

impl<T> Suspended for Encoded<T> {
    fn bind(&mut self, wake: Wake) -> Bound {
        self.call.bind(wake)
    }

    fn finish(&mut self, handles: &mut Handles) -> Option<Result<Vec<u8>, Failure>> {
        Some(match self.call.take()? {
            Ok(value) => (self.encode)(value, handles).map_err(Failure::Host),
            Err(error) => Err(Failure::Rejected(error)),
        })
    }
}

/// The context of one suspending provider call. A pending call's
/// completion gets its instance's identity when the instance registers
/// the call, not from here.
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

/// The byte-level entry of one provider trait: calls the method at index
/// `method` of its interface in the contract, with that method's encoded
/// arguments, and returns its encoded result (empty for `()`), or a
/// suspending method's pending call. A handle the result holds is issued
/// in `handles`. The companion generates one per trait; it downcasts
/// `provider` to the trait object it registered, or for a handle trait to
/// the one a result returned.
pub type Dispatch =
    fn(provider: &mut dyn Any, method: usize, args: &[u8], handles: &mut Handles) -> Dispatched;

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
