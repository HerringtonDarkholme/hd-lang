//! `Http.send!`: `.start` decodes the
//! request and checks its URL and grant on the program thread, and a
//! refusal returns at once. Otherwise the
//! request runs as one task on the `hd-io` runtime, which sends it on the
//! runtime's blocking pool (ureq blocks) and delivers the response or the
//! failure for `.finish` to encode.
//!
//! Cancelling the operation drops the task, which signals the request's
//! cancel (`connector::Cancel`): the request's socket wait ends at once,
//! its thread drops the socket, and the server reads the end of the
//! connection (`std-http.send.cancel.close`).

use std::time::{Duration, Instant};

use hd_run::Grants;

use crate::codec::Enc;
use crate::host::Stop;
use crate::runtime::reactor::Done;

use super::connector::Cancel;
use super::request::{HttpError, Request, Response, encode, send};
use super::target::Target;

/// The settings this host honours (`std-http.settings.host-support`).
const SUPPORTED: &[&str] = &[
    "tls.roots",
    "tls.identity",
    "connect_timeout",
    "tls_handshake_timeout",
    "response_header_timeout",
    "body_read_timeout",
];

/// What `.start` decides.
pub enum Start {
    /// The encoded result of a request that is not sent.
    Ready(Enc),
    /// A request to send.
    Run(Job),
}

/// A request to send, with the grants each redirect is checked against
/// and the end of its timeout, which covers every redirect
/// (`std-http.send.timeout.whole`); `None` when that end is too far away
/// to represent.
pub struct Job {
    request: Box<Request>,
    grants: Grants,
    pub deadline: Option<Instant>,
}

/// What the task delivers: the response or the failure, or why the
/// request stopped with neither (its blocking thread panicked).
pub type Sent = Result<Result<Response, HttpError>, String>;

/// `send.start`'s checks, in the Node host's order: the URL and its
/// grant, then starts the provider's default whole-request timeout.
pub fn start(bytes: &[u8], grants: &Grants) -> wasmtime::Result<Start> {
    let started = Instant::now();
    let request = Request::decode(bytes)?;
    // Every changed setting this host cannot honour is refused before any
    // DNS or connection work (`std-http.settings.preflight`).
    if let Some(name) = request
        .settings
        .changed()
        .into_iter()
        .find(|name| !SUPPORTED.contains(name))
    {
        return Ok(Start::Ready(encode(Err(HttpError::UnsupportedSetting {
            name: name.into(),
        }))));
    }
    if let Err(failure) = granted_url(&request.url, grants) {
        return Ok(Start::Ready(encode(Err(failure))));
    }
    let timeout = Duration::from_secs(30);
    Ok(Start::Run(Job {
        request: Box::new(request),
        grants: grants.clone(),
        deadline: started.checked_add(timeout),
    }))
}

/// The encoded result of a start that cannot run its task.
pub(crate) fn not_run(error: &str) -> Enc {
    encode(Err(HttpError::Other {
        message: error.into(),
    }))
}

impl Job {
    /// The task: sends the request on the blocking pool. Dropping it before
    /// the request ends cancels the request.
    pub async fn run(self) -> Done {
        let cancel = Cancel::default();
        let _abort = CancelOnDrop(cancel.clone());
        let Self {
            request,
            grants,
            deadline,
        } = self;
        let sent =
            tokio::task::spawn_blocking(move || send(*request, &grants, deadline, &cancel)).await;
        Done::Http(sent.map_err(|error| format!("the Http request stopped: {error}")))
    }
}

/// Cancels the request when the task's work is dropped: at its abort, or
/// harmlessly once the request has ended.
struct CancelOnDrop(Cancel);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// `send.finish`'s result.
pub fn finish(sent: Sent) -> wasmtime::Result<Enc> {
    Ok(encode(sent.map_err(Stop::Internal)?))
}

pub(crate) fn granted_url(value: &str, grants: &Grants) -> Result<Target, HttpError> {
    let url = Target::parse(value).ok_or_else(|| HttpError::InvalidUrl { url: value.into() })?;
    if !grants.covers_host("Http", &url.host, Some(url.port)) {
        return Err(HttpError::NotGranted { host: url.host });
    }
    Ok(url)
}
