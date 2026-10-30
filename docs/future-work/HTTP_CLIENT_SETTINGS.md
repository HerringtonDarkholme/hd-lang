# HTTP Transport Settings

Status: accepted 2026-10-29; domain: `std.http` providers and host transports. Settings chain on the provider (`Http.default().connect_timeout(s(5)).tls(roots)`).

This note addresses [#950](https://github.com/HerringtonDarkholme/hd-lang/issues/950).
It reviews the current [Client](../../spec/std/http.md#client),
[timeout policy](../../spec/std/task.md#timeout), and host transports.
Nothing here changes the specification or implementation.

## Proposal

Put DNS, TLS, and phase settings on the covering `Http` provider. Each method returns a new provider
and leaves its receiver unchanged. `Client`, `Request`, and the total timeout policy gain no setting.

```text
resolver := dns_servers(["10.0.0.2"])?
transport := $.use(Http)
    .resolver(resolver)
    .tls(roots=[internal_ca], identity=.Some(id))
    .connect_timeout(2s)
$.with(Http=transport):
    apply!(timeout(10s)):
        client.send!(&request)?
```

The provider also offers TLS-handshake, response-header, and body-read timeouts. These bound one
host-measured phase per redirect hop. `apply!(timeout(...))` remains the only whole-operation deadline.

The checked signatures and examples are in git history at 611fca589, `docs/future-work/http-client-settings/`; the build (#950) works from them.

A zero phase timeout expires immediately, as the existing timeout policy
already does. A configured DNS server is a literal IP address, so it never
needs the resolver it replaces. Extra roots extend the platform store;
replacing the store needs its own security design.

## Problems it solves

| Program | Today | Proposed |
| --- | --- | --- |
| Private discovery | HTTP always uses the host resolver | `resolver(dns_servers(...))` derives one provider |
| Private PKI or mTLS | No roots or identity reach the host | `tls(roots=..., identity=...)` carries both |
| Stuck peer | Only one fixed provider deadline exists | Four phase limits stop the exact host phase |
| Whole call | Callers compose `apply!(timeout(...))` | Unchanged across all phases and redirects |

## How it works

### Public surface

| Item | Purpose |
| --- | --- |
| `dns_servers(List[string])` | Validate literal resolver addresses |
| `Certificate::from_pem` | Validate one extra PEM root |
| `Identity::from_pem` | Validate a PEM chain and matching private key |
| `Http.resolver` | Replace the resolver on a derived provider |
| `Http.tls` | Replace extra roots and the optional identity |
| Four timeout methods | Bound connect, TLS handshake, headers, or body read |
| `Http.send!` | Remain the only I/O terminal |

`Client` stays today's copied cookie-and-redirect value. The same provider therefore serves direct
requests, free helpers, and every `Client`.

Each method returns `*mut dyn Http`. The host derives it from the receiver's authority and settings;
it never retains the local reference from `$.use(Http)`. Different setting kinds compose. A later
call for one setting replaces its earlier value, and a later `tls` replaces the whole TLS pair.

The methods have defaults for existing custom providers. A default returns a provider whose first
send is `UnsupportedSetting(name)` without calling the receiver. Existing implementations keep
compiling, while security settings are never ignored. `Identity` has no `Debug`, `Eq`, or serde traits.

### Timeouts

| Setting | Starts | Ends |
| --- | --- | --- |
| `connect_timeout` | Before DNS | At TCP connection |
| `tls_handshake_timeout` | After TCP | When TLS is ready |
| `response_header_timeout` | After request write | At complete headers |
| `body_read_timeout` | After headers | At the complete buffered body |

Each redirect resets applicable phase timers. A reused connection skips connect and TLS timers.
Any expiry is today's `HttpError.Timeout`. A total timeout may win first and cancels the suspension.

### Host contract

Settings travel in the internal host request DTO, not in public `Request`. The adapter checks every
non-default setting before DNS or connection work. An unsupported value returns
`UnsupportedSetting(name)`. Redirects carry the same settings and keep their ordinary origin checks.

`dns_servers` grants no authority. A supporting host allow-lists resolver endpoints when creating
its provider. An unlisted address fails before a DNS packet is sent.

`Support` below means the first implementation can honor the option. `Reject` means it returns
`UnsupportedSetting` during preflight, before resolution or connection.

| Host | DNS | Roots | Identity | Connect | TLS handshake | Headers | Body read |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Node | Support | Support | Support | Support | Support | Support | Support |
| Wasmtime CLI | Support | Support | Support | Support | Support | Support | Support |
| Browser | Reject | Reject | Reject | Reject | Reject | Reject | Reject |
| Cloudflare SDK | Reject | Reject | Reject | Reject | Reject | Reject | Reject |
| Vercel Node SDK | Reject | Reject | Reject | Reject | Reject | Reject | Reject |

Node can combine a custom [`dns.Resolver`](https://nodejs.org/api/dns.html#class-dnsresolver) lookup,
the `ca`, `cert`, and `key` options of
[`https.request`](https://nodejs.org/api/https.html#httpsrequestoptions-callback), and socket/request
events. The native Wasmtime host has a custom ureq resolver and connector; its implementation adds
the selected resolver and rustls configuration, ureq's connect/response/body deadlines, and one TLS
handshake deadline in the connector. Browser XHR and the Cloudflare and Vercel Fetch adapters expose
neither transport identity nor separate phase timers.

Node and Wasmtime support still needs implementation. Proxy settings wait for their own grant,
credential, bypass, redirect, and CONNECT design.

### Chaining versus `new_http(HttpConfig)`

The config alternative also derives authority from `$ Http`:

```text
http := new_http(HttpConfig {
    resolver: .Some(dns_servers(["10.0.0.2"])?),
    tls: .Some(TlsConfig { roots: [internal_ca], identity: .Some(id) }),
    connect_timeout: .Some(2s),
})  # $ Http
```

| Property | Chained provider | `new_http(HttpConfig)` |
| --- | --- | --- |
| Authority | Visible at `$.use(Http)` | Hidden in the function row |
| Shape | One operation per concern | One optional-field record |
| Static reading | Chosen settings read left to right | Defaults are implicit |
| Dynamic config | Needs a small application helper | Natural to decode |
| Evolution | Adds one orthogonal method | Grows one central schema |
| Std mechanisms | Provider methods only | Methods plus record and constructor |

**Recommendation: ship chaining only.** It exposes authority, has fewer invalid states, and reuses
`$.with`. Applications can apply decoded optional values in a local helper. That use does not yet
justify a second standard mechanism or a serializable secret-bearing record.

## Cost

| Component | Expected cost and gate |
| --- | --- |
| Code removed | None; `Client` and request APIs stay unchanged |
| Code added | Three values, six default methods, one error variant, DTO fields, and Node branches |
| Compatibility | Custom providers inherit deterministic unsupported defaults |
| Compile time | No compiler mechanism; measure std and one caller |
| Runtime | No branch for defaults; configured sends validate a small record |
| Wasm size | Unconfigured requests must remain byte-identical |
| Web compiler gzip | No compiler change; remain under the 2 MB cap |

This is one ordinary core-library surface plus host support. It needs no syntax, semantic exception,
or compiler intrinsic. This design PR has no implementation measurements.

## Appendix A: Stress test

### Private Kubernetes API

One provider carries cluster DNS, a root, and an identity. The public provider remains unchanged,
and the key never enters `Request` or `Client`.

```text
cluster := $.use(Http)
    .resolver(dns_servers([cluster_dns])?)
    .tls(roots=[cluster_ca], identity=.Some(workload_id))
$.with(Http=cluster):
    client.send!(&request)?
```

### Redirect and connection reuse

A cross-origin redirect repeats the grant check and resets each phase timer. The total deadline does
not reset. A reused connection skips connect and TLS without extending that deadline.

### Fetch-only deployment

Cloudflare receives a provider with an extra root. The send returns
`UnsupportedSetting("tls.roots")` before Fetch runs. Silent platform defaults would violate policy.

### Existing custom provider

A provider implementing only today's `send!` still compiles through defaults. Unconfigured sends
work; configured sends fail before its method runs.

### Aliases and dynamic configuration

Two derived providers keep independent timeouts because no setter mutates the base. A decoded config
needs an application helper; this is the recommendation's deliberate ergonomic cost.

## Appendix B: Prior art

### Go `net/http`

```go
transport := http.DefaultTransport.(*http.Transport).Clone()
transport.DialContext = (&net.Dialer{Timeout: 2 * time.Second}).DialContext
transport.TLSHandshakeTimeout = 3 * time.Second
transport.ResponseHeaderTimeout = 5 * time.Second
client := &http.Client{Transport: transport}
```

Go separates [`Client`](https://pkg.go.dev/net/http#Client) from
[`RoundTripper`](https://pkg.go.dev/net/http#RoundTripper) and
[`Transport`](https://pkg.go.dev/net/http#Transport). Wrapping a `RoundTripper` composes transport
policy without changing `Client`. Hd takes that ownership and composition split.

### Effect platform layers

```typescript
program.pipe(
  Effect.provide(NodeHttpClient.layerNodeHttpOptions({ ca, cert, key })),
)
```

Effect's
[`NodeHttpClient`](https://effect.website/docs/v4/api/platform-node/NodeHttpClient) supplies agents
and dispatchers as layers. Operation timeout remains separate. `$.with(Http=transport)` gives hd the
same scoped replacement without a general layer object.

### Rust reqwest

```rust
let client = reqwest::Client::builder()
    .connect_timeout(Duration::from_secs(2))
    .dns_resolver(resolver)
    .add_root_certificate(root)
    .identity(identity)
    .build()?;
```

Reqwest's
[`ClientBuilder`](https://docs.rs/reqwest/latest/reqwest/struct.ClientBuilder.html) owns transport
options because its client owns a transport and pool. Hd's `Client` owns only cookies and redirects,
so copying this surface would put settings on the wrong owner.

### Fetch

```javascript
await fetch(url, { method: "GET", headers, signal: controller.signal })
```

[`RequestInit`](https://fetch.spec.whatwg.org/#requestinit) exposes cancellation and message options,
but no DNS, root, identity, or phase-timeout settings. Fetch-only hosts therefore reject this design's
settings instead of claiming false portability.

## Appendix C: Alternatives considered

| Alternative | Rejected because |
| --- | --- |
| `new_http(HttpConfig)` | It adds a second mechanism and a central optional-field schema |
| Settings on `Client` | `Client` does not own hd's host transport |
| Mutating the provider | Aliases and outer scopes observe later changes |
| Settings on `Request` | Messages would carry policy and private keys |
| Provider total timeout | It duplicates `apply!(timeout(...))` and cleanup rules |
| Silent best effort | It falsely promises security or timing guarantees |

## Appendix D: Today in hd

[`Client`](../../lib/std/http.hd) contains only a public `CookieJar`. Its send reads `Clock`, follows
redirects, and calls the covering `Http` provider. The spec gives it
[no timeout setter](../../spec/std/http.md#r-std-http.client.policy-timeout).

[`host/http.mjs`](../../host/http.mjs) uses direct Node requests, no TLS options, and one fixed
30-second deadline. The native Wasmtime host uses ureq, a custom resolver and connector, and rustls.
The browser uses XHR. The server SDK uses Fetch.

## Appendix E: Details

- **Host ABI:** add resolver, TLS, and four timeout fields to each wire request.
- **Errors:** use stable names `resolver`, `tls.roots`, `tls.identity`, and each timeout method name.
- **Cancellation:** every phase timer destroys or aborts the active request through today's path.
- **Counting:** a derived provider is retained; requests, roots, and resolver descriptions are values.
- **Grants:** origin checks remain per hop; custom DNS is limited by a host allow-list.
- **Determinism:** a provider's settings are immutable, and later same-kind calls replace earlier ones.
- **Footprint:** measure no-setting identity, each setting alone, and the web compiler gzip cap.
- **Tests:** cover each host option, unsupported preflight, redirects, connection reuse, and cancellation.
- **Phases:** add std/error defaults, wire DTOs, Node support, unsupported hosts, then conformance.
- **Rollback:** remove the added trait defaults and DTO fields; `Client` and `Request` never change.
- **Parse log:** all three `text` blocks parsed with `scripts/hd-conformance.sh parse` on 2026-10-29;
  parsing checks syntax only.
