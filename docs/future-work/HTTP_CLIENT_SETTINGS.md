# HTTP Client Settings

Status: proposal, 2026-10-28; domain: `std.http` clients and host transports; not accepted.

This note addresses [#950](https://github.com/HerringtonDarkholme/hd-lang/issues/950).
It reviews the current [Client](../../spec/std/http.md#client),
[timeout policy](../../spec/std/task.md#timeout), and host transports.
Nothing here changes the specification or implementation.

## Proposal

Make `Client` a reusable counted builder with one I/O terminal, `send!`.
Its noun setters configure a default timeout, one HTTP CONNECT proxy, extra roots, or one client identity.
Timeout calls the existing `std.task.timeout`; transport settings travel to the host with each send.
Unsupported hosts return a typed error before network I/O and never ignore a security setting.

```text
use std.http.{Certificate, Client, Http, HttpError, Request, Response, Url}
use std.time.{Clock, s}

fn get!() -> Result[Response, HttpError] $ Http + Clock:
    root := Certificate::from_pem("-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----")?
    client := Client::new().timeout(5s).root_certificate(root)
    client.send!(&Request::get(Url::parse("https://api.internal/")?))
```

The checked [signatures and examples](http-client-settings/) cover every proposed public item.

## Open questions

1. **Timeout range. Recommendation: shorter-only.** Keep the host's 30-second safety bound, so a client timeout can shorten but not extend it.
2. **Unsupported transports. Recommendation: typed send error.** Add `HttpError.UnsupportedSetting(name)` instead of silently ignoring proxy or TLS settings.
3. **Proxy authority. Recommendation: require both grants.** The host must grant the proxy endpoint and every origin, including redirect targets.
4. **Client identity. Recommendation: one identity.** A later `identity` call replaces the earlier one; multiple candidate certificates wait for evidence.

## Problems it solves

| Program | Today | Proposed |
| --- | --- | --- |
| Service call with a five-second default | Every call repeats `apply!(timeout(5s))` | `Client::new().timeout(5s)` applies the same policy to every send |
| Corporate egress | The host always connects directly | `proxy(Proxy::http(url)?)` uses HTTP CONNECT after both grants pass |
| Private service | Only the host trust store is available | `root_certificate(root)` merges one validated PEM root |
| Mutual TLS | No client certificate reaches the transport | `identity(Identity::from_pem(chain, key)?)` supplies one protected identity |

The timeout is a convenience over an existing policy, not another timer model.
The TLS methods never disable certificate or hostname verification.

## How it works

### Public surface

| Item | Purpose |
| --- | --- |
| `Client::new`, `Client::with_jar` | Return one retained `*mut Client` builder |
| `Client.timeout(Duration)` | Store the existing `TimeLimit` policy for every later send |
| `Client.proxy(Proxy)` | Store one validated HTTP proxy endpoint |
| `Client.root_certificate(Certificate)` | Append one PEM root to the ordinary trust store |
| `Client.identity(Identity)` | Replace the client's PEM chain and private key |
| `Client.send!` | The only I/O terminal; cookies, redirects, timeout, and transport settings share one operation |

The counted client is justified because its cookie jar and configuration outlive one call.
Requests and certificate values remain plain values; ordinary readers keep local `&T` references.
The public `jar` field remains available.
`new` and `with_jar` now return a counted client, so copying a client aliases its jar instead of duplicating it.
Unconfigured sends and direct `Request.send!` otherwise keep today's behavior.

`Proxy::http` accepts only an absolute `http` URL without user information.
The first version has no SOCKS, proxy authentication, bypass list, or environment lookup.
Those features need separate security and authority designs.

`Certificate::from_pem` validates one certificate.
`Identity::from_pem` validates a certificate chain and matching private key.
Neither `Identity` nor `Client` implements `Debug`, `Eq`, `Serialize`, or `Deserialize`.

### Timeout and sending

`Client.send!` wraps the complete cookie and redirect operation with `apply!(timeout(limit))` when configured.
It therefore covers DNS, connect, redirects, final body buffering, and cancellation cleanup.
The existing `From[TimedOut] for HttpError` produces `HttpError.Timeout`.

The provider's 30-second host-time bound remains a safety ceiling.
A configured policy uses `Clock`, so `ManualClock` controls it in tests.
This differs from reqwest, whose client timeout may be any total deadline.

### Host contract

The host request DTO gains proxy, root-certificate, and identity fields.
Plain `Request.send!` and unconfigured clients send empty settings, preserving today's path.
The `Http` adapter must reject unsupported nonempty fields as `UnsupportedSetting` before DNS or connection work.

| Host | Extra roots | Client identity | HTTP proxy | Required work |
| --- | --- | --- | --- | --- |
| Node default host | Node accepts `ca` | Node accepts `cert` and `key` | Not implemented | Pass TLS options; add CONNECT and proxy grants |
| Wasmtime host | rustls can load roots | rustls can load a client identity | ureq supports proxy configuration | Build a combined root store and extend the custom connector; it currently disables proxy |
| Server SDK and browser Fetch | Not exposed | Not exposed | Not exposed | Return `UnsupportedSetting`; timeout still works |

The origin grant remains mandatory because the proxy must not broaden destination authority.
The proxy grant is also mandatory because it names the actual network peer.

## Cost

| Component | Expected cost and gate |
| --- | --- |
| Code removed | None initially; repeated call-site timeout wrappers may disappear later |
| Code added | Four configuration types, builder methods, DTO fields, Node and Wasmtime transport branches |
| Compile time | No new compiler mechanism; measure the checked package and one configured caller |
| Runtime | One policy branch per send; PEM parsing once per constructor; proxy adds its normal handshake |
| Wasm size | Unconfigured client must stay byte-identical; each setting is measured separately |
| Web compiler gzip | No compiler change; remain under the 2 MB cap |

The design PR adds only documentation and a checked package, so it has no implementation measurements.

## Appendix A: Stress test

### Private Kubernetes API

An operator uses a cluster root and a short client default.
The proposal keeps public roots and adds the private root rather than replacing the store.

```text
root := Certificate::from_pem(cluster_ca)?
client := Client::new().timeout(10s).root_certificate(root)
response := client.send!(&Request::get(api_url))?
```

The identity variant also covers clusters requiring mutual TLS.
The private key cannot appear through `Debug`, serialization, or equality.

### Corporate proxy and redirects

One client calls two origins through one proxy, then follows a cross-origin redirect.
The host checks the proxy grant once per send and each origin grant before that hop.
Removing the origin check would let one proxy grant bypass HTTP destination policy.

### Long polling

A client requests a 90-second timeout for a long poll.
The current provider still ends it at 30 seconds.
This is the proposal's deliberate difference from reqwest and its largest usability cost.

The anti-case is an untrusted program retaining a host socket indefinitely.
Allowing longer deadlines needs a separate owner decision about the host safety ceiling.

### Fetch-only deployment

A Cloudflare-style Fetch host receives a client with an extra root.
Fetch has no request option for trust roots, identities, or proxies.
The send fails before network I/O instead of appearing configured while using platform defaults.

### Aliased client

Two bindings copy one `*mut Client` and send sequential requests.
Both see the same jar and settings, matching reqwest's shared reusable client.
A local `&mut Client` remains the receiver used within each terminal call.

## Appendix B: Prior art

### Rust reqwest

```rust
let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(5))
    .proxy(reqwest::Proxy::all("http://proxy.internal:8080")?)
    .add_root_certificate(root)
    .identity(identity)
    .build()?;
```

Reqwest puts total timeout, proxy, roots, and identity on one reusable
[`ClientBuilder`](https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html).
Hd takes those typed concerns, but uses its existing timeout policy and one `send!` terminal.

Reqwest also exposes dangerous verifier switches and many transport knobs.
Hd rejects those initially because they weaken a security invariant or lack a demonstrated common case.

### Go net/http

```go
transport := http.DefaultTransport.(*http.Transport).Clone()
transport.Proxy = http.ProxyURL(proxyURL)
transport.TLSClientConfig = &tls.Config{RootCAs: roots, Certificates: identities}
client := &http.Client{Transport: transport, Timeout: 5 * time.Second}
```

Go separates high-level [`Client`](https://pkg.go.dev/net/http#Client) behavior from
[`Transport`](https://pkg.go.dev/net/http#Transport) proxy and TLS configuration.
Hd keeps one public client because its host owns the transport, but retains separate typed settings.

Go reads proxy environment variables by default.
Hd does not: hidden environment policy would create action at a distance and bypass explicit package authority.

### WHATWG Fetch

```javascript
const response = await fetch(url, {
  method: "GET",
  headers,
  signal: controller.signal,
});
```

[`RequestInit`](https://fetch.spec.whatwg.org/#requestinit) exposes cancellation and message options,
but no caller-selected proxy, root store, or client identity.
Hd therefore reports unsupported settings on Fetch-only hosts instead of promising false portability.

### Current host libraries

```javascript
https.request({ hostname, ca: roots, cert: chain, key: privateKey });
```

Node's [`https.request`](https://nodejs.org/api/https.html#httpsrequestoptions-callback)
accepts `ca`, `cert`, and `key` from `tls.connect`.
Ureq exposes typed [`proxy`](https://docs.rs/ureq/latest/ureq/config/struct.ConfigBuilder.html#method.proxy)
and `tls_config` settings, so the Wasmtime libraries can represent the same concerns.

## Appendix C: Alternatives considered

### Settings on every Request

This matches reqwest's per-request override, but repeats defaults and copies private-key configuration into message values.
It also makes server-side request values capable of carrying client transport secrets.

### Environment and system proxy discovery

Go and ureq support this default.
Hd rejects it because the same package would change destinations and credentials through undeclared host environment state.

### A string-keyed transport options map

One map would adapt to host extensions without API growth.
It loses compile-time spelling, value types, secret handling, and unsupported-setting diagnostics.

### A second host timeout field

Passing milliseconds beside every wire request would let clients exceed the provider ceiling.
It duplicates `std.task.timeout`, diverges under `ManualClock`, and adds a second cancellation rule.

### No portable TLS settings

Applications could supply a custom `Http` provider for every private PKI deployment.
That is simpler for std, but forces ordinary certificate configuration to replace the whole transport.

## Appendix D: Today in hd

[`Client`](../../lib/std/http.hd) is a copied value containing only a public `CookieJar`.
Its send reads `Clock` once, follows redirects itself, and calls the covering `Http` provider per hop.
The spec explicitly says it has [no timeout setter](../../spec/std/http.md#r-std-http.client.policy-timeout).

The Node worker in [`host/http.mjs`](../../host/http.mjs) uses direct `http.request` or `https.request`.
It sets `agent: false`, has one fixed 30-second deadline, and passes no TLS credentials.

The Wasmtime request path in
[`request.rs`](../../crates/hd_run_wasmtime/src/http/request.rs) builds one ureq agent per hop.
It calls `.proxy(None)` and chains the default `RustlsConnector` after its custom direct connector.

The server SDK in
[`adapters/http.mjs`](../../packages/server-sdk/adapters/http.mjs) uses platform Fetch.
Its options cover the message, redirects, abort signal, and fixed deadline only.

## Appendix E: Details and phases

- **API tier:** every proposed item passes the tier test and belongs in `spec/std/http.md`.
- **Language changes:** none; builders, counted references, requirements, policy layers, and errors already exist.
- **Ownership:** `Client` is counted because settings and cookies escape calls; request and configuration inputs remain values.
- **Secrets:** identities have no inspection traits; errors name only the setting, never PEM text or keys.
- **Cancellation:** timeout cancellation follows the existing `TimeLimit` and `Http.send!` abort rules.
- **Redirects:** one outer timeout covers every hop; each target repeats origin and proxy grant checks.
- **Host parity:** Node and Wasmtime implement supported settings; Fetch-only hosts reject them explicitly.
- **Testing:** scripted transport cases cover settings, grant order, unsupported hosts, cancellation, and secret-free errors.
- **Rollback:** remove the new builder methods and DTO fields; unconfigured sends retain today's wire shape.

Implementation phases, each about one hour:

1. Add std types, signatures, `UnsupportedSetting`, and focused conformance cases.
2. Add wire DTO fields and scripted-provider observations; keep empty settings byte-identical.
3. Apply `TimeLimit` once around the complete client send; test `ManualClock` and cleanup.
4. Implement Node PEM options and CONNECT proxying; test both grants and redirect targets.
5. Implement Wasmtime rustls and ureq settings; test invalid PEM, private roots, mTLS, and proxying.
6. Reject settings in Fetch adapters; run Node, Wasmtime, server-SDK, size, and web-compiler gates.

## Parse log

Both `text` blocks in this note parsed with `scripts/hd-conformance.sh parse` on 2026-10-28.
Parsing checks syntax only; the proposal is not implemented and the blocks do not type-check against current std.
