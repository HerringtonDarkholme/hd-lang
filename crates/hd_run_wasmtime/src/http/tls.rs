//! The TLS of an HTTPS hop. A provider with default settings uses ureq's
//! own rustls connector unchanged. A derived provider's extra roots
//! extend Mozilla's roots, ureq's default store, and its identity is the
//! client certificate (`std-http.settings.tls.extend`, `.tls.identity`).
//! Either way the handshake is one phase, with its own deadline when the
//! provider sets one (`std-http.settings.phase-timeouts`).

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, LazyBuffers, NextTimeout, RustlsConnector, Transport,
    TransportAdapter,
};

use super::connector::Phase;
use super::request::{HttpError, Settings};

/// The rustls configuration of a provider that adds roots or an identity,
/// or `None` for the host default. A root or identity rustls cannot use is
/// a TLS failure before any connection (`std-http.settings.pem.structure`).
pub(crate) fn client_config(settings: &Settings) -> Result<Option<Arc<ClientConfig>>, HttpError> {
    if settings.tls_roots.is_empty() && settings.tls_identity.is_none() {
        return Ok(None);
    }
    let failed = |message: String| HttpError::Tls { message };
    // ureq's own choice: the process default provider, else ring.
    let provider = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(rustls::crypto::ring::default_provider()));
    let mut roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    for pem in &settings.tls_roots {
        for root in CertificateDer::pem_slice_iter(pem.as_bytes()) {
            let root = root.map_err(|error| failed(format!("invalid TLS root: {error}")))?;
            roots
                .add(root)
                .map_err(|error| failed(format!("invalid TLS root: {error}")))?;
        }
    }
    let builder = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(rustls::ALL_VERSIONS)
        .map_err(|error| failed(error.to_string()))?
        .with_root_certificates(roots);
    let config = match &settings.tls_identity {
        None => builder.with_no_client_auth(),
        Some(identity) => {
            let chain = CertificateDer::pem_slice_iter(identity.certificate_chain.as_bytes())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| failed(format!("invalid TLS identity: {error}")))?;
            let key = PrivateKeyDer::from_pem_slice(identity.private_key.as_bytes())
                .map_err(|error| failed(format!("invalid TLS identity: {error}")))?;
            builder
                .with_client_auth_cert(chain, key)
                .map_err(|error| failed(format!("invalid TLS identity: {error}")))?
        }
    };
    Ok(Some(Arc::new(config)))
}

/// The hop's TLS connector, after the TCP connector: ureq's rustls
/// connector, or the same handshake over a derived provider's
/// configuration, within the handshake deadline.
#[derive(Debug)]
pub(crate) struct HopTls {
    pub(crate) config: Option<Arc<ClientConfig>>,
    pub(crate) handshake_timeout: Option<Duration>,
    pub(crate) phase: Phase,
    pub(crate) default: RustlsConnector,
}

impl Connector<Box<dyn Transport>> for HopTls {
    type Out = Box<dyn Transport>;

    fn connect(
        &self,
        details: &ConnectionDetails<'_>,
        chained: Option<Box<dyn Transport>>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let Some(transport) = chained else {
            return Ok(None);
        };
        if !details.needs_tls() || transport.is_tls() {
            return Ok(Some(transport));
        }
        // The handshake starts after the TCP connection and ends when TLS
        // is ready; the socket caps its waits at the phase's end.
        self.phase.set(self.handshake_timeout);
        let secured = match &self.config {
            None => self
                .default
                .connect(details, Some(transport))
                .map(|out| out.map(|out| Box::new(out) as Box<dyn Transport>)),
            Some(config) => handshake(config, details, transport).map(Some),
        };
        self.phase.set(None);
        secured
    }
}

/// ureq's rustls handshake (`RustlsConnector`) over `config`.
fn handshake(
    config: &Arc<ClientConfig>,
    details: &ConnectionDetails<'_>,
    transport: Box<dyn Transport>,
) -> Result<Box<dyn Transport>, ureq::Error> {
    let host = details
        .uri
        .host()
        .ok_or(ureq::Error::Tls("Rustls invalid dns name error"))?;
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    let name = ServerName::try_from(host.to_owned())
        .map_err(|_| ureq::Error::Tls("Rustls invalid dns name error"))?;
    let mut conn = ClientConnection::new(config.clone(), name)?;
    let mut sock = TransportAdapter::new(transport);
    sock.set_timeout(details.timeout);
    conn.complete_io(&mut sock)?;
    Ok(Box::new(TlsTransport {
        buffers: LazyBuffers::new(
            details.config.input_buffer_size(),
            details.config.output_buffer_size(),
        ),
        stream: StreamOwned { conn, sock },
    }))
}

/// A TLS connection over the hop's socket, as ureq's `RustlsTransport`.
struct TlsTransport {
    buffers: LazyBuffers,
    stream: StreamOwned<ClientConnection, TransportAdapter>,
}

impl std::fmt::Debug for TlsTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsTransport").finish_non_exhaustive()
    }
}

impl Transport for TlsTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.stream.get_mut().set_timeout(timeout);
        let output = &self.buffers.output()[..amount];
        self.stream.write_all(output)?;
        Ok(())
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        self.stream.get_mut().set_timeout(timeout);
        let input = self.buffers.input_append_buf();
        let amount = self.stream.read(input)?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        self.stream.get_mut().get_mut().is_open()
    }

    fn is_tls(&self) -> bool {
        true
    }
}
