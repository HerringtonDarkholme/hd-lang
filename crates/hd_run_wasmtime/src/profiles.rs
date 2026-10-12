//! Explicit conformance fixture providers, never enabled by the default host.
//! Their boundary values match `host/profiles.mjs`; the ordinary
//! Console provider already supplies the `pending-write` behavior. The
//! `disposed-file` and `pending-gate` providers follow the spec's Runtime
//! Profiles (`spec/conformance/README.md`); Node has no providers for them
//! yet either, so the spec text is the reference.

use std::collections::BTreeMap;

use crate::codec::Enc;

/// An explicitly selected conformance fixture runtime profile.
/// These providers are for fixtures, not additional default capabilities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RuntimeProfile {
    #[default]
    Default,
    MisbehavingHost,
    SpecialFloatHost,
    SerdeVault,
    PendingWrite,
    DisposedFile,
    PendingGate,
}

impl RuntimeProfile {
    /// Resolves the Node fixture profile name, or the ordinary default host.
    pub fn from_name(name: &str) -> Result<Self, String> {
        match name {
            "default" => Ok(Self::Default),
            "misbehaving-host" => Ok(Self::MisbehavingHost),
            "special-float-host" => Ok(Self::SpecialFloatHost),
            "serde-vault" => Ok(Self::SerdeVault),
            "pending-write" => Ok(Self::PendingWrite),
            "disposed-file" => Ok(Self::DisposedFile),
            "pending-gate" => Ok(Self::PendingGate),
            _ => Err(format!("no runtime profile named {name}")),
        }
    }

    pub(crate) fn method(self, module: &str, name: &str) -> Option<Method> {
        match (self, module, name) {
            (Self::MisbehavingHost, "hd:Gauge", "level") => Some(Method::GaugeLevel),
            (Self::SpecialFloatHost, "hd:Sensor", "reading") => Some(Method::SensorReading),
            (Self::SerdeVault, "hd:Vault", "keep") => Some(Method::VaultKeep),
            _ => None,
        }
    }

    /// A suspending fixture call: the logical method and whether it is the
    /// `.finish` half of its `.start`/`.finish` pair.
    pub(crate) fn waiting(self, module: &str, name: &str) -> Option<(Method, bool)> {
        let (method, finish) = match (self, module, name) {
            (Self::DisposedFile, "hd:Files", "open.start") => (Method::FilesOpen, false),
            (Self::DisposedFile, "hd:Files", "open.finish") => (Method::FilesOpen, true),
            (Self::DisposedFile, "hd:FileHandle", "read.start") => (Method::FileHandleRead, false),
            (Self::DisposedFile, "hd:FileHandle", "read.finish") => (Method::FileHandleRead, true),
            (Self::DisposedFile, "hd:FileHandle", "close.start") => {
                (Method::FileHandleClose, false)
            }
            (Self::DisposedFile, "hd:FileHandle", "close.finish") => {
                (Method::FileHandleClose, true)
            }
            (Self::PendingGate, "hd:Gate", "wait.start") => (Method::GateWait, false),
            (Self::PendingGate, "hd:Gate", "wait.finish") => (Method::GateWait, true),
            _ => return None,
        };
        Some((method, finish))
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Method {
    GaugeLevel,
    SensorReading,
    VaultKeep,
    FilesOpen,
    FileHandleRead,
    FileHandleClose,
    GateWait,
}

impl Method {
    /// Whether the method runs as a `.start`/`.finish` pair rather than one
    /// call.
    pub(crate) fn is_waiting(self) -> bool {
        !matches!(
            self,
            Method::GaugeLevel | Method::SensorReading | Method::VaultKeep
        )
    }
}

pub(crate) enum Reply {
    Encoded(Enc),
    EchoLength(i64),
}

/// Provider state belongs to one instance, including on a shared engine.
#[derive(Default)]
pub(crate) struct State {
    readings: u8,
    /// `disposed-file` handles by number: whether each is still open.
    files: BTreeMap<i32, bool>,
    next_file: i32,
}

impl State {
    /// A fresh open `disposed-file` handle number, never reused.
    pub(crate) fn open_file(&mut self) -> i32 {
        self.next_file = self.next_file.wrapping_add(1);
        if self.next_file == 0 {
            self.next_file = 1;
        }
        self.files.insert(self.next_file, true);
        self.next_file
    }

    /// `read!` of the handle: `.Ok("")` while open, `.Err(Disposed)` after
    /// its close. An unknown number is a guest bug.
    pub(crate) fn read_file(&self, file: i32) -> Result<Vec<u8>, String> {
        match self.files.get(&file) {
            Some(true) => {
                let mut encoded = Enc::default();
                encoded.leb(0).str("");
                Ok(encoded.into_bytes())
            }
            Some(false) => Ok(disposed()),
            None => Err(format!("no disposed-file handle numbered {file}")),
        }
    }

    /// `close` of the handle: `.Ok(())` once, `.Err(Disposed)` after.
    pub(crate) fn close_file(&mut self, file: i32) -> Result<Vec<u8>, String> {
        match self.files.get_mut(&file) {
            Some(open @ true) => {
                *open = false;
                let mut encoded = Enc::default();
                encoded.leb(0);
                Ok(encoded.into_bytes())
            }
            Some(false) => Ok(disposed()),
            None => Err(format!("no disposed-file handle numbered {file}")),
        }
    }
}

/// `.Err(ResourceError.Disposed)`: `Operation` is variant 0, `Disposed`
/// variant 1 (`std.resource`), crossed as unsigned LEB128 like every
/// `Result` error (`fs::api`).
fn disposed() -> Vec<u8> {
    let mut encoded = Enc::default();
    encoded.leb(1).leb(1);
    encoded.into_bytes()
}

impl State {
    pub(crate) fn call(&mut self, method: Method, length: i64) -> Reply {
        let mut encoded = Enc::default();
        match method {
            Method::GaugeLevel => {
                encoded.leb(300);
            }
            Method::SensorReading => {
                let value = match self.readings {
                    0 => f64::NAN,
                    1 => f64::INFINITY,
                    _ => -0.0,
                };
                self.readings = self.readings.saturating_add(1);
                encoded.f64(value);
            }
            // Node returns len unchanged, even when it is negative or past
            // memory. The generated guest decoder enforces its boundary;
            // this deliberately transparent provider does not read or copy it.
            Method::VaultKeep => return Reply::EchoLength(length),
            Method::FilesOpen
            | Method::FileHandleRead
            | Method::FileHandleClose
            | Method::GateWait => {
                unreachable!("waiting fixture methods run as .start/.finish, never one call")
            }
        }
        Reply::Encoded(encoded)
    }
}
