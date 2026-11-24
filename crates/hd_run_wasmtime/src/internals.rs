//! What `tests/unit` reaches inside the engine: the feature `testing`,
//! which only the crate's own dev-dependency turns on, so a normal build
//! neither compiles nor exports it. The unit tests live outside `src` so
//! that a test build compiles this library once, not again in test mode
//! (`[lib] test = false`). It is not named `testing` because the crate
//! already has a public `testing` module (the test providers).
//!
//! One module per source module the tests reach, at the same path. A
//! private module's items the tests call are `pub` there and re-exported
//! here. An item of a public module (`native`, `prim`) or of a type the
//! crate root exports stays `pub(crate)` and is reached through a wrapper
//! here, so the crate's public API is the same with or without the
//! feature, except for this module. The extension traits keep the
//! method names, so a test calls `handles.insert(..)` as the engine does.
//!
//! Cost: nothing in a normal build; with the feature, the wrappers only
//! forward.

use std::sync::Arc;
use std::time::Instant;

use crate::host::Host;
use crate::testing::State;
use crate::{Module, NativeBodyRunner, TestEngine, TestModule, WasmtimeInstance};

/// A loaded [`Module`]'s compiled code.
pub trait ModuleExt {
    /// The code this module shares with every load of the same bytes and
    /// engine config (`Arc::ptr_eq` tells one compilation from two).
    fn code(&self) -> &Arc<wasmtime::Module>;
}

impl ModuleExt for Module {
    fn code(&self) -> &Arc<wasmtime::Module> {
        &self.0
    }
}

/// A [`TestEngine`]'s wasmtime engine.
pub trait TestEngineExt {
    /// The engine the pool runs on.
    fn wasmtime(&self) -> &wasmtime::Engine;
}

impl TestEngineExt for TestEngine {
    fn wasmtime(&self) -> &wasmtime::Engine {
        &self.0.engine
    }
}

/// A [`TestModule`]'s compiled code and its prepared imports.
pub trait TestModuleExt {
    /// The code, shared by every compile of the same bytes.
    fn code(&self) -> &Arc<wasmtime::Module>;
    /// The host kinds whose imports are resolved.
    fn prepared_hosts(&self) -> usize;
}

impl TestModuleExt for TestModule {
    fn code(&self) -> &Arc<wasmtime::Module> {
        &self.0.module
    }
    fn prepared_hosts(&self) -> usize {
        self.0
            .prepared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

/// A [`WasmtimeInstance`]'s host state.
pub trait WasmtimeInstanceExt {
    /// The host state of the instance's store.
    fn host(&self) -> &Host;
    /// Whether the last call trapped.
    fn last_trapped(&self) -> bool;
}

impl WasmtimeInstanceExt for WasmtimeInstance {
    fn host(&self) -> &Host {
        self.store.data()
    }
    fn last_trapped(&self) -> bool {
        self.last_trapped
    }
}

/// A [`NativeBodyRunner`]'s retained instance.
pub trait NativeBodyRunnerExt {
    /// The instance kept for the property's next body, if any.
    fn instance(&self) -> Option<&WasmtimeInstance>;
}

impl NativeBodyRunnerExt for NativeBodyRunner {
    fn instance(&self) -> Option<&WasmtimeInstance> {
        self.instance.as_ref()
    }
}

/// A case [`State`]'s host deadline.
pub trait StateExt {
    /// Sets the deadline of the case's host operations.
    fn set_io_deadline(&self, deadline: Option<Instant>);
}

impl StateExt for State {
    fn set_io_deadline(&self, deadline: Option<Instant>) {
        State::set_io_deadline(self, deadline);
    }
}

pub use crate::engine::wasmtime_config;
pub use crate::run::Shared;

/// `WasmtimeSession`'s test hooks, which it keeps `pub(crate)`.
pub trait SessionExt {
    /// Rebuilds the session in linked base `wasm` from another session's
    /// `journal`.
    ///
    /// # Errors
    /// `wasm` is not a linked session's module, or the replay failed.
    fn run_from(
        &mut self,
        journal: &[u8],
        wasm: &[u8],
        sources: &[(u32, &str)],
    ) -> Result<crate::Ran, String>;
    /// The host calls the session made, not served from a record.
    fn host_calls(&self) -> u64;
    /// Whether a host operation is pending in the session's reactor.
    fn pending(&self) -> bool;
}

impl SessionExt for crate::WasmtimeSession {
    fn run_from(
        &mut self,
        journal: &[u8],
        wasm: &[u8],
        sources: &[(u32, &str)],
    ) -> Result<crate::Ran, String> {
        crate::WasmtimeSession::run_from(self, journal, wasm, sources)
    }
    fn host_calls(&self) -> u64 {
        crate::WasmtimeSession::host_calls(self)
    }
    fn pending(&self) -> bool {
        crate::WasmtimeSession::pending(self)
    }
}
pub mod abi {
    pub use crate::abi::*;
}
pub mod cache {
    pub use crate::cache::*;
}
pub mod cancel {
    pub use crate::cancel::*;
}
pub mod checked_report {
    pub use crate::checked_report::*;
}
pub mod code_cache {
    pub use crate::code_cache::*;
}
pub mod codec {
    pub use crate::codec::*;
}
pub mod fs {
    pub use crate::fs::sandbox::Sandbox;
    pub mod api {
        pub use crate::fs::api::*;
        pub use crate::fs::sandbox::Sandbox;
    }
    #[cfg(windows)]
    pub mod io {
        pub use crate::fs::io::device_path;
    }
    pub mod ops {
        pub use crate::fs::ops::*;
    }
    pub mod sandbox {
        pub use crate::fs::sandbox::*;
    }
}
pub mod host {
    pub use crate::host::*;
    pub mod output {
        pub use crate::host::output::*;
        pub use crate::runtime::reactor::Stream;
    }
}
pub mod http {
    pub use crate::http::Headers;
    pub mod client {
        pub use crate::http::client::*;
        pub use crate::http::request::encode;
    }
    pub mod connector {
        pub use crate::http::connector::*;
    }
    pub mod request {
        pub use crate::http::request::*;
    }
    pub mod target {
        pub use crate::http::target::*;
    }
}
pub mod limits {
    pub use crate::limits::*;
}

/// `native`'s engine-only parts, which its public types keep
/// `pub(crate)`.
pub mod native {
    pub use crate::native::*;

    /// `Suspension::finish`, which only the engine calls.
    pub trait SuspensionExt {
        /// The delivered value, encoded, with any handle it holds issued
        /// in `handles`; `None` while pending.
        fn finish(&mut self, handles: &mut Handles) -> Option<Result<Vec<u8>, Failure>>;
    }

    impl SuspensionExt for Suspension {
        fn finish(&mut self, handles: &mut Handles) -> Option<Result<Vec<u8>, Failure>> {
            Suspension::finish(self, handles)
        }
    }

    /// The handle table's engine-side operations and its bounds.
    pub mod handles {
        use std::any::Any;

        use crate::native::handles as table;
        pub use crate::native::handles::*;
        use crate::native::{Dispatch, HostError};

        /// Slots per table.
        pub const SLOTS: u32 = table::SLOTS;
        /// Generations per slot.
        pub const GENERATIONS: u32 = table::GENERATIONS;

        /// The table's failure text for `message`.
        #[must_use]
        pub fn fail(message: &str) -> HostError {
            table::fail(message)
        }

        /// A handle's trait object, out of its slot for one method call.
        pub struct Lent(table::Lent);

        impl Lent {
            /// The trait object.
            #[must_use]
            pub fn value(&self) -> &dyn Any {
                &*self.0.value
            }
        }

        /// The engine's calls on a [`Handles`] table.
        pub trait HandlesExt {
            /// Issues a number for `value` of the handle trait at contract
            /// interface `interface`.
            ///
            /// # Errors
            /// The table is full.
            fn insert(
                &mut self,
                interface: u32,
                dispatch: Dispatch,
                value: Box<dyn Any>,
            ) -> Result<u32, HostError>;
            /// Takes `number`'s trait object out for one call.
            ///
            /// # Errors
            /// `number` is not a live handle of `interface`, or is lent.
            fn lend(&mut self, number: u32, interface: u32) -> Result<Lent, HostError>;
            /// Puts a lent trait object back.
            fn restore(&mut self, lent: Lent);
            /// Drops `number`'s trait object and retires its number.
            ///
            /// # Errors
            /// `number` is not a live handle of `interface`.
            fn release(&mut self, number: u32, interface: u32) -> Result<(), HostError>;
            /// The live entries.
            fn live(&self) -> usize;
            /// The slots issued so far, live or not.
            fn slot_count(&self) -> usize;
            /// Fills the table to `count` vacant slots at `generation`,
            /// none of them free.
            fn fill_vacant(&mut self, count: usize, generation: u32);
            /// Marks `slot` free for reuse.
            fn free_slot(&mut self, slot: u32);
        }

        impl HandlesExt for Handles {
            fn insert(
                &mut self,
                interface: u32,
                dispatch: Dispatch,
                value: Box<dyn Any>,
            ) -> Result<u32, HostError> {
                Handles::insert(self, interface, dispatch, value)
            }
            fn lend(&mut self, number: u32, interface: u32) -> Result<Lent, HostError> {
                Handles::lend(self, number, interface).map(Lent)
            }
            fn restore(&mut self, lent: Lent) {
                Handles::restore(self, lent.0);
            }
            fn release(&mut self, number: u32, interface: u32) -> Result<(), HostError> {
                Handles::release(self, number, interface)
            }
            fn live(&self) -> usize {
                Handles::len(self)
            }
            fn slot_count(&self) -> usize {
                Handles::slot_count(self)
            }
            fn fill_vacant(&mut self, count: usize, generation: u32) {
                Handles::fill_vacant(self, count, generation);
            }
            fn free_slot(&mut self, slot: u32) {
                Handles::free_slot(self, slot);
            }
        }
    }

    pub mod wire {
        pub use crate::native::Serde;
        pub use crate::native::wire::*;

        /// The JS host's failure text for `message`.
        #[must_use]
        pub fn fail(message: &str) -> super::HostError {
            crate::native::wire::fail(message)
        }
    }
}
pub mod net {
    pub use crate::net::*;
    pub mod client {
        pub use crate::net::client::*;
    }
    pub mod ops {
        pub use crate::abi::NetError;
        pub use crate::net::ops::*;
        pub use crate::net::socket::Udp;
    }
    pub mod socket {
        pub use crate::abi::NetError;
        pub use crate::net::CONNECT_ATTEMPT_TIMEOUT;
        pub use crate::net::socket::*;
    }
    pub mod validate {
        pub use crate::net::validate::*;
    }
}

/// The float printing's exact-integer parts, which `prim` keeps
/// `pub(crate)`: the tests hold the native printing to a reference built
/// from them.
pub mod prim {
    use std::cmp::Ordering;

    use crate::prim as p;
    pub use crate::prim::*;

    /// A non-negative integer of any size.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Big(p::Big);

    impl Big {
        #[must_use]
        pub fn from_u64(v: u64) -> Self {
            Self(p::Big::from_u64(v))
        }
        pub fn mul_small(&mut self, m: u32) {
            self.0.mul_small(m);
        }
        pub fn mul_pow10(&mut self, k: u32) {
            self.0.mul_pow10(k);
        }
        #[must_use]
        pub fn shl(&self, bits: u32) -> Self {
            Self(self.0.shl(bits))
        }
        #[must_use]
        pub fn shr(&self, bits: u32) -> Self {
            Self(self.0.shr(bits))
        }
        #[must_use]
        pub fn low_bits(&self, bits: u32) -> Self {
            Self(self.0.low_bits(bits))
        }
        #[must_use]
        pub fn add(&self, other: &Self) -> Self {
            Self(self.0.add(&other.0))
        }
        /// `self - other`, with `self >= other`.
        pub fn sub_assign(&mut self, other: &Self) {
            self.0.sub_assign(&other.0);
        }
        #[must_use]
        #[expect(
            clippy::should_implement_trait,
            reason = "the engine's own method name, by reference"
        )]
        pub fn cmp(&self, other: &Self) -> Ordering {
            self.0.cmp(&other.0)
        }
        #[must_use]
        pub fn to_decimal(&self) -> String {
            self.0.to_decimal()
        }
    }

    /// A finite float's parts: the value is `f * 2^e`.
    #[derive(Clone, Copy, Debug)]
    pub struct Parts {
        pub negative: bool,
        pub f: u64,
        pub e: i32,
    }

    /// The parts of the float whose bits are `raw`.
    #[must_use]
    pub fn split(raw: u64, mantissa: u32, exp_bits: u32, bias: i32) -> Parts {
        let p::Parts { negative, f, e } = p::split(raw, mantissa, exp_bits, bias);
        Parts { negative, f, e }
    }

    /// The text of a NaN or an infinity.
    #[must_use]
    pub fn special(value: f64) -> Option<String> {
        p::special(value)
    }

    /// The Display text of the digits `ds` whose first digit has the
    /// decimal exponent `k - 1`.
    #[must_use]
    pub fn layout(sign: &str, ds: &str, k: i32) -> String {
        p::layout(sign, ds, k)
    }
}
pub mod process {
    pub mod native {
        pub use crate::process::native::*;
    }
    pub mod test_child {
        pub use crate::process::test_child::*;
    }
}
pub mod runtime {
    pub mod io {
        pub use crate::runtime::io::*;
    }
    pub mod native_io {
        pub use crate::runtime::native_io::*;
    }
    pub mod reactor {
        pub use crate::runtime::reactor::*;
    }
}

/// The test engine's pool, which `TestEngine` and `TestModule` keep
/// `pub(crate)`.
pub mod sys {
    pub mod platform {
        pub use crate::sys::platform::*;
    }
    pub use crate::sys::random::random;
}

pub mod test_engine {
    pub use crate::test_engine::*;
    pub mod engine {
        pub use crate::test_engine::engine::*;
    }
}
pub mod testing {
    pub use crate::testing::*;
    pub mod files {
        pub use crate::testing::files::*;
    }
    pub mod property {
        pub use crate::testing::property::*;
    }
}
pub mod workers {
    pub use crate::workers::*;
}
