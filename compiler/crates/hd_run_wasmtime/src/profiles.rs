//! Explicit conformance fixture providers, never enabled by the default host.
//! Their boundary values match `compiler/host/profiles.mjs`; the ordinary
//! Console provider already supplies the `pending-write` behavior.

use crate::codec::Enc;

/// An explicitly selected conformance fixture runtime profile.
/// These providers are for fixtures, not additional default capabilities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RuntimeProfile {
    #[default]
    Default,
    MisbehavingHost,
    SpecialFloatHost,
    SerdeVault,
    PendingWrite,
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
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Method {
    GaugeLevel,
    SensorReading,
    VaultKeep,
}

pub(crate) enum Reply {
    Encoded(Enc),
    EchoLength(i64),
}

/// Provider state belongs to one instance, including on a shared engine.
#[derive(Default)]
pub(crate) struct State {
    readings: u8,
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
        }
        Reply::Encoded(encoded)
    }
}
