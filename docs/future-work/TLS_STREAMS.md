# TLS Streams

Status: design exploration, 2026-10-23; nothing here is decided or in the specification.
This explores [#265](https://github.com/HerringtonDarkholme/hd-lang/issues/265), following the [current queue direction](https://github.com/HerringtonDarkholme/hd-lang/issues/58#issuecomment-6374047957).
The earlier [HTTP-only deferral](https://github.com/HerringtonDarkholme/hd-lang/issues/265#issuecomment-6229433932) remains an option; this report adds no TLS implementation.

The records under review are [Net](../../spec/std/net.md), [Http](../../spec/std/http.md), and [CLI grants](../../spec/cli/command-line.md#grant-scopes).
[Streaming Design](STREAMING_DESIGN.md) is related work, not a prerequisite or accepted TLS API.
[Host Embedding](HOST_EMBEDDING.md) remains deferred; this exploration does not decide its registration mechanism.

## Contents

- [The Domain](#the-domain)
- [Problem And What hd Has Today](#problem-and-what-hd-has-today)
- [Survey](#survey)
- [Option: Keep TLS Inside Http](#option-keep-tls-inside-http)
- [Option: Connect And Authenticate Together](#option-connect-and-authenticate-together)
- [Option: Transfer A Host Stream Into TLS](#option-transfer-a-host-stream-into-tls)
- [Option: Run A TLS Engine Over Any Byte Transport](#option-run-a-tls-engine-over-any-byte-transport)
- [Comparison](#comparison)
- [Recommendation](#recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources And Parse Log](#sources-and-parse-log)

## The Domain

TCP carries an ordered sequence of bytes between endpoints.
TLS establishes cryptographic state over that transport, authenticates a peer, and protects application bytes.
The handshake itself reads and writes before ordinary application traffic can begin.
These are different layers; successful TCP connection does not establish the requested service's identity.

| Term | Meaning here |
| --- | --- |
| Transport address | Where a connection goes, such as `192.0.2.10:993`. |
| Reference identity | The service the application intended to contact, such as `mail.example.test`. |
| Certificate chain | Evidence linking a peer's certificate to a configured trust anchor. |
| Trust roots | The authorities the verifier accepts; authorization to connect does not supply them. |
| SNI | A DNS name sent during the handshake to help the server choose its configuration. |
| ALPN | Negotiation of an application protocol over TLS; it is separate from selecting an endpoint. |
| Plaintext and ciphertext | Application bytes and their protected transport representation. |
| STARTTLS | An application protocol switches an existing plaintext connection into TLS at an agreed boundary. |

### A Worked Example

Suppose configuration names `mail.example.test`, but the connection is pinned to `192.0.2.10:993`.
The network grant controls that actual destination.
Certificate verification checks the configured service name, independently of the dial address.
The peer's certificate cannot choose the identity against which it will be checked.

[RFC 9525](https://www.rfc-editor.org/rfc/rfc9525.html#section-6) distinguishes reference identities from identifiers encountered during resolution.
DNS identities match DNS certificate entries; literal IP identities match IP entries exactly.
SNI assists routing; sending a name does not establish that the certificate authenticates it.
These distinctions matter when an application supplies an IP destination and a DNS service identity.

For implicit TLS, the client handshakes before reading the mail banner.
For STARTTLS, the application first parses the plaintext protocol's acceptance response, then starts the handshake.
A generic TLS wrapper cannot discover that application boundary from an arbitrary byte sequence.
If the handshake fails, falling back to plaintext would change the security promise.

TLS reads do not preserve application message boundaries.
Protocol traffic can require writes while servicing a read, and both encrypted and decrypted bytes may remain buffered.
[RFC 8446](https://www.rfc-editor.org/rfc/rfc8446.html#section-6.1) distinguishes authenticated closure from transport EOF.
Without the closure alert, the receiver cannot establish that all sent data arrived.

### Why A Simple Wrapper Is Incomplete

hd's references can be copied, including mutable stream references.
Giving a wrapper one reference does not remove another reference's access to the original stream.
The current library makes that sharing observable:

```text
use std.net.{ScriptedStream, TcpStream}

pub fn main!() -> void $ Console:
    original := ScriptedStream::new([[65u8, 66u8]])
    let alias: &mut dyn TcpStream = original
    first := original.read!(1).expect("scripted operation")
    second := alias.read!(1).expect("scripted operation")
    alias.close().expect("scripted operation")
    println("${first[0]} ${second[0]} ${original.is_closed()}")
```

This prints `65 66 true` on both engines.
The two reads share a cursor, and closing through the alias closes the original handle.
A TLS transfer therefore needs a policy for every old alias, pending operation, and buffered byte.
Reference counting alone does not prove exclusive access.

## Problem And What hd Has Today

The question is which boundary should provide authenticated byte streams beyond HTTP, while preserving existing grants, references, suspension, and cancellation.
This is a client-stream question.
Server certificates, client authentication, QUIC, session-ticket controls, and a general cryptography API wait for that boundary decision.

| Existing piece | Relevant constraint |
| --- | --- |
| [Net trait](../../spec/std/net.md#net-trait) | `connect!` returns `&mut dyn TcpStream`; it has no TLS operation. |
| [Socket handles](../../spec/std/net.md#socket-handles) | Copies share one socket; close affects pending operations and subsequent calls. |
| [Timeouts](../../spec/std/net.md#timeouts) | A cancelled read preserves undelivered bytes; a composed connect cancellation closes an unfinished socket. |
| [Net errors](../../spec/std/net.md#net-errors) | Existing errors describe networking, not certificate or TLS protocol failures. |
| [Scripted Net](../../spec/std/net.md#scripted-net) | Deterministic hd handles exercise byte flow and aliasing, without implementing TLS. |
| [Http](../../spec/std/http.md#sending) | HTTPS already exists, but exposes requests and whole responses, not arbitrary byte protocols. |
| [Grant scopes](../../spec/cli/command-line.md#grant-scopes) | Net grants cover host and port; partial denials return before an out-of-scope effect. |

At base `2e89fcb0bab43c22c009bd92871b35d1562b70fa`, [Node Net](../../host/net.mjs) retains queued chunks and unread tails.
[Native Net](../../crates/hd_run_wasmtime/src/net/socket.rs) retains unread bytes in stream state.
An upgrade that bypasses those owners can lose bytes or race existing reads.
Taking an operating-system socket alone is insufficient.

The existing [Node HTTP host](../../host/http.mjs) uses Node HTTPS.
The [native HTTP connector](../../crates/hd_run_wasmtime/src/http/request.rs) uses ureq's Rustls connector.
Those integrations provide maintenance precedents, but neither exposes a TLS stream contract.
They also do not establish identical trust-root policies across hosts.

### Fixed Use Cases

Every option below is assessed against the same five cases.
The common code example reads the first chunk of an implicitly protected mail banner; a complete mail client must still assemble protocol messages.

| Case | Required end-to-end behavior |
| --- | --- |
| U1: implicit TLS | Connect to `mail.example.test:993`, authenticate it, then read its banner; no application bytes before authentication. |
| U2: STARTTLS | Parse the plaintext acceptance response on port 587, then upgrade that same connection without plaintext fallback. |
| U3: private deployment | Dial `192.0.2.10:993`, verify `mail.example.test` under an explicit private root, and reject a wrong name or root. |
| U4: interrupted ownership | Account for a copied raw handle, an outstanding read, and cancellation during the handshake; never mix raw and protected I/O. |
| U5: custom transport | Use a scripted or relayed hd byte transport; distinguish deterministic API tests from real certificate and protocol tests. |

No unsettled registration decision prevents using the existing built-in Net host boundary.
A new general facility for guest callbacks or an imported cryptographic engine would require separate approval.
The last option makes that additional dependency explicit.

## Survey

These are integration models, not claims that hd already supports their APIs.
All rows cite maintained primary documentation.

| Tool | Interface and boundary | What it covers | Remaining responsibility |
| --- | --- | --- | --- |
| [Node TLS](https://nodejs.org/api/tls.html#tlsconnectoptions-callback) | `tls.connect` can dial or wrap a supplied `Duplex`. | Host sockets and compatible host streams. | Set `servername` explicitly for SNI; wrapping does not transfer unrelated application aliases automatically. |
| [Rustls](https://docs.rs/rustls/latest/rustls/) | `ClientConnection` separates encrypted I/O from plaintext reads and writes. | A maintained TLS engine independent of network I/O. | Supply trust roots and drive traffic, buffering, and connection state. |
| [Tokio Rustls](https://docs.rs/tokio-rustls/latest/tokio_rustls/client/struct.TlsConnector.html) | `connect(ServerName, IO)` performs an asynchronous handshake over `AsyncRead + AsyncWrite`. | Rust transports satisfying those traits. | Map deadlines, cancellation, and ownership into the host's operation model. |
| [Go crypto/tls](https://pkg.go.dev/crypto/tls) | `Client(net.Conn, Config)` wraps; `Dial` connects and handshakes. | Existing connections and direct dialing. | Configure service identity; absent explicit roots, Go uses host roots. |
| [Python ssl](https://docs.python.org/3/library/ssl.html) | A client context wraps a socket with `server_hostname`. | Socket-oriented TLS through OpenSSL. | Choose the client context deliberately; `PROTOCOL_TLS_CLIENT` requires chain and hostname checks. |
| [Java SSLEngine](https://docs.oracle.com/en/java/javase/25/docs/api/java.base/javax/net/ssl/SSLEngine.html) | `wrap` and `unwrap` process bytes independently of transport. | Arbitrary reliable I/O and application-managed buffers. | Drive handshake progress, delegated tasks, and shutdown; the engine does not perform transport I/O. |
| [.NET SslStream](https://learn.microsoft.com/en-us/dotnet/api/system.net.security.sslstream?view=net-10.0) | Wraps a `Stream`, then authenticates before application exchange. | Stream composition with explicit inner-stream lifetime controls. | Dispose failed authentication state and choose transport ownership deliberately. |

Three conclusions follow from these interfaces.
First, connect-only and upgrade APIs can share one host engine, but their ownership contracts differ.
Second, a transport-independent engine moves the I/O pump into the integration layer; it does not eliminate that work.
Third, trust selection and peer identity remain explicit responsibilities even when the handshake API is small.

## Option: Keep TLS Inside Http

This is the radical simplification: retain the current feature boundary.
Applications use HTTPS services or an application-specific gateway instead of speaking raw TLS protocols from hd.
A gateway is a separate deployment and trust boundary, not an equivalent implementation of U1.

```text
use std.http.{Http, HttpError, Response, ScriptedHttp, get}

fn banner!() -> Result[List[u8], HttpError] $ Http:
    .Ok(get!("https://gateway.example.test/mail-banner")?.body)

pub fn main!() -> void $ Console:
    http := ScriptedHttp::new()
    http.reply("https://gateway.example.test/mail-banner", .Ok(Response { status: 200, body: [79u8, 75u8] }))
    $.with(Http=http):
        println(banner!().expect("scripted operation").len())
```

The scripted response prints `2`; it proves only the existing HTTP API shape.
The gateway would need a separately specified mail operation.
An HTTPS URL cannot turn an arbitrary IMAP or SMTP service into an HTTP server.

| Case | Outcome |
| --- | --- |
| U1 | Dropped for direct mail access; a separate gateway changes the requirement. |
| U2 | Dropped; Http cannot upgrade an arbitrary existing TCP connection. |
| U3 | Existing Http behavior only; no new raw-stream trust configuration. |
| U4 | No new transfer state; existing Http cancellation remains its own contract. |
| U5 | ScriptedHttp tests HTTP clients, not TLS over arbitrary transports. |

No rules are added or retired in any tier.
Traits, requirement rows, derivation, RC representation, and suspension stay unchanged.
Implementation cost is zero for hd, with deployment costs shifted to applications that need a gateway.

## Option: Connect And Authenticate Together

The host owns dialing and the complete handshake, publishing only a verified stream.
The application never receives the underlying plaintext handle.
The following local traits demonstrate the proposed call shape; they are not std declarations or host implementations.

```text
trait ProtectedStream:
    fn read!(&mut self, max: usize) -> Result[List[u8], string]
    fn close(&mut self) -> void

trait TlsDial:
    fn connect!(&mut self, host: string, port: u16, peer: string) -> Result[&mut dyn ProtectedStream, string]

fn banner!(dial: &mut dyn TlsDial) -> Result[List[u8], string]:
    stream := dial.connect!("mail.example.test", 993u16, "mail.example.test")?
    defer: stream.close()
    stream.read!(4096)
```

The configured trust policy is omitted from this minimal sketch, not delegated to an insecure default.
Production errors would distinguish transport failures, untrusted certificates, name mismatch, protocol failure, and unauthenticated EOF.
The sketch uses `string` errors only to avoid proposing an enum before the boundary is chosen.

| Case | Outcome |
| --- | --- |
| U1 | Directly supported: grant check, dial, authenticate, publish, read. |
| U2 | Dropped; reconnecting is not an upgrade of the accepted plaintext session. |
| U3 | Separate dial address, peer identity, and configured roots support it. |
| U4 | Raw aliases cannot escape before handoff; cancellation closes an unpublished connection. |
| U5 | Script a secure-dial provider; arbitrary hd transports cannot be inserted beneath the host stack. |

A new public operation and secure-handle contract belong in the stdlib tier.
Built-in host recognition and ABI entries need language-tier accounting where the compiler knows their names.
Reusing Net's destination scope needs CLI-tier rules; a second ambient TLS grant is unnecessary under that proposal.
The local `TlsDial` name does not decide a new requirement trait.

Ordinary traits and bang calls express the API without syntax or a new reference rule.
A production helper would use the selected Net provider, so custom providers need a compatible extension strategy.
A separate secure-stream trait permits typed TLS errors without changing every plain TCP operation's result type.
No new derivation or RC mechanism follows from that trait.

## Option: Transfer A Host Stream Into TLS

An existing host-backed stream transfers its transport to a TLS engine.
The host revokes every plaintext view, then returns a distinct protected view after authentication.
This mechanism covers implicit TLS and protocols that negotiate an upgrade.
The local `HostStream` below stands for a proposed upgrade-capable host handle, not a runtime cast from any `dyn TcpStream`.

```text
trait ProtectedStream:
    fn read!(&mut self, max: usize) -> Result[List[u8], string]
    fn close(&mut self) -> void

trait HostStream:
    fn start_tls!(&mut self, peer: string) -> Result[&mut dyn ProtectedStream, string]

fn banner!(connected: &mut dyn HostStream) -> Result[List[u8], string]:
    stream := connected.start_tls!("mail.example.test")?
    defer: stream.close()
    stream.read!(4096)
```

For U1, the caller previously connected that handle to `mail.example.test:993` through Net.
For U2, it first exchanges the protocol's STARTTLS messages on the same handle.
A combined-connect convenience operation can use this mechanism while keeping the raw handle private.
That convenience spelling is not a fifth design option.

### Transfer States And Buffered Bytes

A possible transfer contract has these states; the owner must approve the contract before implementation.
Preflight failure and failure after protocol commitment have different consequences.

| State or event | Proposed treatment |
| --- | --- |
| Raw and idle | Only this state can begin transfer. |
| Pending read or write | Reject before commitment; never let raw operations continue alongside TLS. |
| Host-held undelivered bytes | Move into the TLS input exactly once when they belong after the protocol boundary. |
| Caller-held bytes after the boundary | Require an explicit prefix handoff, or reject this upgrade shape; never silently discard them. |
| Upgrading | All old raw aliases become unusable; no competing plaintext reads, writes, or closes. |
| Success | Publish a new protected view; old raw views remain invalid. |
| Failure, cancellation, or abandonment after commitment | Close the transport; do not restore a plaintext view. |
| Disposing an invalid old view | Do not close the new protected view; the transport now belongs to that view. |

STARTTLS makes read-ahead particularly important.
A parser may consume a chunk containing the acceptance response and bytes belonging to the next phase.
The host cannot recover bytes already returned to the caller without an explicit handoff.
An initial API can reject that case, but must document the resulting parser constraint.

The operation must synchronize with both hosts' existing stream queues and jobs.
A check followed by an unsynchronized socket take would still race a new raw operation.
Runtime view invalidation handles reference sharing; it is not a new exclusive-borrow language rule.
The effect on custom TcpStream implementations remains part of this option's cost.

| Case | Outcome |
| --- | --- |
| U1 | Net dials, the host transfers and authenticates, then the caller reads the protected view. |
| U2 | Supported with an exact protocol boundary and the prefix policy above. |
| U3 | Dial authority and verification identity remain separate; roots configure the engine. |
| U4 | Explicit state transitions revoke aliases and close interrupted handshakes. |
| U5 | Host-backed transports only; scripted upgrade handles can model the contract, but arbitrary hd streams cannot be taken as sockets. |

Stdlib-tier additions define upgrade, invalidated views, secure-handle errors, and custom-provider behavior.
CLI-tier rules define destination authorization; language-tier ABI rules cover any new built-in operation.
There is no new syntax, derivation scheme, or RC representation.
Suspension and cancellation need new operation-specific contracts rather than a change to the general scheduler.

## Option: Run A TLS Engine Over Any Byte Transport

A maintained TLS engine consumes and produces encrypted bytes, while an ordinary hd pump drives an arbitrary transport.
The abstraction works above sockets, relays, and scripted streams.
It requires an actual cryptographic engine integration; hd has no such ordinary library today.
Reimplementing cryptography in this exploration is not an alternative.

```text
use std.net.TcpStream

trait ProtectedStream:
    fn read!(&mut self, max: usize) -> Result[List[u8], string]
    fn close(&mut self) -> void

trait TlsEngine:
    fn wrap!(&mut self, transport: &mut dyn TcpStream, peer: string) -> Result[&mut dyn ProtectedStream, string]

fn banner!(engine: &mut dyn TlsEngine, connected: &mut dyn TcpStream) -> Result[List[u8], string]:
    stream := engine.wrap!(connected, "mail.example.test")?
    defer: stream.close()
    stream.read!(4096)
```

This local trait describes the combined engine-and-pump boundary.
Its missing implementation is substantial: handshake progression, encrypted buffers, plaintext buffers, trust verification, and transport ownership.
A Node `Duplex` can support a host-side adapter, but it cannot directly execute an arbitrary hd trait value.
That direction requires a callback bridge; a common guest engine requires its own approved integration.

| Case | Outcome |
| --- | --- |
| U1 | Net supplies bytes; the pump authenticates before exposing the protected stream. |
| U2 | Supported if the application transfers the transport and every buffered prefix at the agreed boundary. |
| U3 | Engine configuration supplies roots and reference identity independently of transport. |
| U4 | The wrapper cannot revoke arbitrary custom aliases automatically; exclusive-use discipline or a revocable transport contract remains necessary. |
| U5 | Directly supported by the abstraction, once a real engine and pump exist. |

The byte engine does not solve aliases merely by accepting `&mut dyn TcpStream`.
That reference remains shareable under today's language rules.
Adding uniqueness checking solely for TLS would introduce a semantic exception; this option does not propose it.
A wrapper that relies on disciplined exclusive use offers a weaker guarantee than host-wide revocation.

An ordinary pump and adapters are stdlib-tier additions if they require no name recognition.
A compiler-known crypto or callback ABI needs language-tier rules and separate approval.
Network authority stays on the underlying transport; verification alone does not authorize a new connection.
Existing traits, requirement rows, suspension, and RC can express the surface without new syntax or derivation.

## Comparison

The costs describe mechanisms, not measured estimates.
No host adapter, cryptographic integration, or browser transport was built for this report.

| Criterion | Http only | Combined connect | Host transfer | Byte engine |
| --- | --- | --- | --- | --- |
| U1: direct implicit TLS | No | Yes | Yes | Yes, after engine integration |
| U2: STARTTLS | No | No | Yes, exact boundary | Yes, exact boundary |
| U3: private roots and separate identity | No new stream API | Explicit configuration | Explicit configuration | Engine configuration |
| U4: aliases and interrupted handshake | No new boundary | Raw handle stays private | Runtime revocation and queue transfer | Requires transport discipline or revocation contract |
| U5: arbitrary hd transport | No | No | No | Yes |
| Rule accounting | None | Std API, host ABI, CLI scope | Same, plus invalidation and prefix contracts | Std pump, engine integration, possible callback ABI |
| Soundness burden | Existing HTTP contract | Verify before publish; close on failure | Atomic transfer; preserve bytes; no fallback | Correct pump plus alias discipline and vetted engine |
| Agent writing | Existing request API | Small connect/read shape | Explicit protocol boundary and handoff | More configuration and ownership choices |
| Human reading | Simple; fails direct-stream intent | Security begins at returned handle | Visible plaintext-to-TLS transition | Generic transport hides more integration work |
| Implementation cost | None in hd | Two host adapters and secure handles | Above, plus both queue/alias migrations | Engine delivery, pump, trust, buffering, adapters |
| Evolution | HTTP workloads only | Upgrade remains a separate later decision | Connect helper reuses transfer | Broadest transport composition |
| Costliest change kind | No change | Compiler-known host addition if built in | Compiler-known host addition if built in | Host/engine ABI addition unless an ordinary guest package suffices |

Under the owner's cost order, ordinary library additions outrank new intrinsics, semantic exceptions, and syntax.
No option needs new syntax.
The generic option cannot claim a pure-library cost until the missing engine has a concrete delivery route.
Connect and transfer share a host-stack cost; transfer adds more lifecycle contracts.

### Contracts Any TLS Option Must Resolve

These are proposed acceptance obligations, not additions to the specification.
They keep the boundary choice from hiding security and resource behavior.

| Topic | Contract to settle and test |
| --- | --- |
| Verification | Authenticate the configured identity under configured roots before publishing a usable stream or sending application bytes. |
| Destination grant | Check the actual dial destination under Net; a peer-name override must never authorize another destination. |
| Upgrade authority | Reuse the already-authorized transport; upgrade performs no hidden dial or certificate-network fetch. |
| Roots | Select host defaults, a shared versioned set, or explicit roots; default acceptance need not otherwise match across engines. |
| Private roots | Root bytes already held by the program need no implicit file read; reading a root file still needs FsRead. |
| Time | Host certificate validation uses host policy; TLS does not add Clock to application rows merely because verification uses time. |
| Cancellation | Close unfinished handshakes; preserve undelivered data during cancelled reads; a cancelled write may have sent a prefix. |
| Time bounds | Bound the whole operation rather than restarting a deadline for each TLS record; use caller-requested composition for explicit deadlines. |
| Shutdown | Distinguish authenticated closure from abrupt EOF; disposal releases resources without waiting indefinitely for a peer. |
| Testing | Use a controlled CA, wrong-name and wrong-root cases, fragmented traffic, cancellation, alias misuse, and prefix handoff on both engines. |

TLS 1.3 early data has replay risks described in [RFC 8446, section 8](https://www.rfc-editor.org/rfc/rfc8446.html#section-8).
The candidate initial boundary can omit early data and plaintext fallback, reducing observable modes.
Client certificates, custom verification callbacks, cipher menus, and key logging wait for a later use case.
Controlled-root fixtures would test shared behavior without asserting identical ambient root stores.

## Recommendation

Recommend the host-transfer mechanism if the owner now wants TLS over existing streams.
It addresses the issue's missing capability, including STARTTLS, through the existing host boundary.
A verified-connect helper can reuse it and keep the raw handle private for ordinary implicit TLS.

- Require one-way transfer, invalidated raw aliases, explicit buffered-prefix treatment, and close on an interrupted committed handshake.
- Use maintained host TLS implementations: Node TLS and Rustls through a native asynchronous adapter, subject to the same approved contract.
- Keep application Clock requirements out of implicit certificate validation; reuse Net authority for destinations.
- Choose roots explicitly before implementation; the host stacks' defaults do not establish a portable verification policy.

This gives up wrapping arbitrary hd transports in the first implementation.
The next-best option is combined connect if STARTTLS is explicitly deferred.
It reduces transfer complexity, but does not complete the existing-stream use case.
Keeping TLS inside Http remains the smallest answer if the owner retains the earlier feature deferral.

## Questions For The Owner

1. **Should hd expose TLS beyond Http now?** Choose host transfer, combined connect only, or continued deferral.
   Recommendation: host transfer when taking on #265; deferral remains valid if raw protocols are still out of scope.
   Example: `connected.start_tls!("mail.example.test")` would protect the already-connected mail session.
2. **Should upgrading revoke every old plaintext alias?** Choose one-way revocation, or wrapper-only exclusive-use discipline.
   Recommendation: revocation for host-backed streams, rejecting active raw operations before commitment.
   Example: after `stream := connected.start_tls!(peer)?`, an old `alias.read!(1)` cannot read the transport.
3. **How should read-ahead cross the upgrade boundary?** Choose an explicit prefix handoff, or require no caller-held bytes after that boundary.
   Recommendation: explicit handoff if supporting general STARTTLS parsers; otherwise document the restriction in the initial API.
   Example: a chunk containing the acceptance response and subsequent TLS bytes must contribute its suffix exactly once.
4. **Which default roots define trust?** Choose documented host defaults, a shared versioned root set, or explicit roots only.
   Recommendation: a shared versioned default plus explicit private-root replacement if cross-engine acceptance parity is required.
   Example: dialing `192.0.2.10` with peer `mail.example.test` verifies that DNS identity under the chosen roots.

The exact public trait placement, error enum, prefix parameter, and root representation follow these decisions.
No std API, grant, diagnostic, conformance exception, or language rule is changed by this exploration.

## Sources And Parse Log

The survey links are primary API documentation, accessed 2026-10-23.
[RFC 9525](https://www.rfc-editor.org/rfc/rfc9525.html) supplies service-identity terminology.
[RFC 8446](https://www.rfc-editor.org/rfc/rfc8446.html) supplies the TLS closure and early-data distinctions.
Project source claims use the base commit named above, not an assumption about another checkout's running compiler.

| Block | Validation | Limit |
| --- | --- | --- |
| Current alias control | Parsed, type-checked, formatted, and run on Node and wasmtime; exact output `65 66 true`. | Plain scripted bytes, not a TLS handshake. |
| Http-only control | Parsed, type-checked, formatted, and run on both engines; exact output `2`. | Scripted Http does not contact a gateway or prove certificate behavior. |
| Combined-connect shape | Parsed and type-checked as local traits and a function. | No provider implementation; not executed. |
| Host-transfer shape | Parsed and type-checked as local traits and a function. | No runtime revocation, handshake, or prefix-handoff implementation. |
| Byte-engine shape | Parsed and type-checked as local traits and a function. | No cryptographic engine, pump, or guest-to-host callback bridge. |

All five blocks use existing syntax; none requires hypothetical syntax.
The two executable controls have no network effects and use a frozen compiler built from this record's base.
Fresh scratch caches and temporary files remain inside the isolated worktree.
Parsing and type-checking demonstrate interface spelling, not security or lifecycle correctness of unimplemented TLS mechanisms.
