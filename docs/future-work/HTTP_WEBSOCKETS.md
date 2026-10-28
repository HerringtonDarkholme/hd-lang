# HTTP WebSockets

Status: design proposal, 2026-10-28; domain: standard library HTTP and server SDK; not accepted.

This proposal addresses [#951](https://github.com/HerringtonDarkholme/hd-lang/issues/951).
It adds WebSocket client connections and server upgrades over existing HTTP messages.
The [checked package](http-websockets/) contains every proposed std signature and its usage.

## Proposal

A `WebSocket` is a scoped message handle bound with the accepted `using` form.
Its suspending `read!` and `write!` mirror the streaming reader/writer shape while preserving message boundaries.
Client handshakes start from `Request`; server upgrades expose their original `Request` and accept response `Headers`.

```text
request := Request::get(url).set_header(.Authorization, token)
using socket = connect_websocket!(request, protocols=["chat.v1"])?
socket.write!(.Text("hello"))?
reply := socket.read!()?
socket.finish!(.NormalClosure, "done")?
```

The checked examples use today's accepted `Disposable` / `using` protocol.
If [#967](https://github.com/HerringtonDarkholme/hd-lang/pull/967) accepts its split, `WebSocket` becomes `AsyncDisposable` and every socket binding changes mechanically to `using!`.

## Open questions

1. Approve a separate `$ WebSocketClient` requirement using the existing `Http` host grant? **Recommended: yes**, because connecting returns a live handle rather than a buffered response.
2. Should server SDKs route upgrades to a separate handler instead of the ordinary 20-second response handler? **Recommended: yes**, so sessions are not request-timeout exceptions.
3. Hide ping, pong, and close frames behind `ping!`, automatic replies, and `finish!`? **Recommended: yes**, following nhooyr's safer application surface.
4. Use a 1 MiB default read limit, adjustable per socket? **Recommended: yes**, matching the streaming design's bounded line reader.

## Problems it solves

| Today | With this proposal |
| --- | --- |
| `Request.send!` can return only one buffered response | `connect_websocket!` upgrades a request to a bidirectional handle |
| Server handlers cannot outlive their HTTP request deadline | The server SDK routes upgrades to a separate session handler |
| Applications would need to parse frames and control messages | `read!`, `write!`, `ping!`, and `finish!` own protocol details |
| Raw streams lose text/binary message boundaries | `WebSocketMessage` retains one complete text or binary message |

## How it works

### Public surface

| Item | Purpose |
| --- | --- |
| `WebSocketMessage` | complete `Text(string)` or `Binary(List[u8])` message |
| `WebSocket` | scoped handle with message read/write, ping, limit, and graceful finish |
| `WebSocketCloseCode` | named RFC close statuses plus application and received future codes |
| `WebSocketError` | HTTP, unsupported-host, handshake, protocol, limit, close, and closed-handle failures |
| `WebSocketClient` | explicit client host operation, authorized by the existing HTTP grant |
| `connect_websocket!` | client upgrade from a `Request`, offered protocols, and redirect limit |
| `WebSocketUpgrade` | server request, offered protocols, accept headers, and ordinary rejection |

The full declarations are in [`http-websockets/src/websockets.hd`](http-websockets/src/websockets.hd).
They are stdlib and host APIs, with no syntax, intrinsic, or compiler-known name.

### Client handshake

The request URL uses `ws` or `wss`; its ordinary headers carry authentication, cookies, origin, and application metadata.
The client owns `Connection`, `Upgrade`, `Sec-WebSocket-Key`, `Sec-WebSocket-Version`, and protocol headers.
Supplying one of those reserved fields is a handshake error rather than an override.

The client follows at most `redirects`, resolves locations like `Client`, and strips authorization and cookies across hosts.
A successful handle exposes response headers and the negotiated protocol.
A non-101 response is `WebSocketError.Rejected(response)`, retaining its status, headers, and read-once body.

`WebSocketClient` is a separate requirement because `Http.send!` returns a completed value.
Its manifest authorization is still the existing host-scoped `Http` grant, including redirects.
There is no second network allow-list.

### Server upgrade

The server adapter validates the HTTP upgrade framing and delivers a `WebSocketUpgrade` to a dedicated handler.
Its `request` is the existing public `Request`; offered subprotocols are parsed in client order.
The application either rejects with an ordinary `Response` or accepts with `Headers`, one offered protocol, and extra exact origins.

With no extra origin, browser requests must have an Origin matching the request host; non-browser requests may omit Origin.
The adapter owns reserved handshake headers and writes the 101 response.
An accepted socket leaves the ordinary HTTP request deadline and lives until its handler and cleanup scope end.
That scope is `using!` under #967 because `dispose!` suspends; it remains `using` under today's accepted protocol.

This requires a server-SDK handler beside `serve!`, not a `Response` variant or a hidden detached task.
The SDK keeps explicit state and requirement rows while its adapter supplies each pending upgrade.
Platforms without server upgrade support reject the deployment or request explicitly.

### Messages and control frames

`read!` reassembles fragments and returns one text or binary message.
Text is valid UTF-8; invalid text closes with `InvalidPayload` and returns `Protocol`.
The read limit applies to the reassembled message and closes an oversized peer with `MessageTooBig`.

`write!` writes and flushes one message.
The host may fragment it, but another writer cannot interleave a message.
One read and one write may run together; later operations in the same direction wait in call order.

Incoming ping receives an automatic pong even when application code is not reading.
`ping!` sends an internal nonce and completes only after its matching pong; unsolicited pongs are ignored.
Control payloads never appear as application messages.

A peer close is answered automatically and reported as `Closed(code, reason)`.
`finish!` validates a sendable code and a reason fitting the 125-byte control-frame limit, then waits for the peer close.
`dispose!` is best-effort: it starts `GoingAway`, aborts without reporting if needed, and is a no-op after finish.

### Timeouts, cancellation, and limits

Callers apply `std.task.timeout` around a read, ping, finish, or whole session, as for other streaming handles.
There is no socket-specific clock or timeout setting.
A cancelled read retains partial fragments for the next read; a cancelled write leaves protocol progress unknown, so callers dispose the socket.

The default maximum reassembled message is 1 MiB; `set_read_limit` changes later reads.
Compression extensions are not negotiated initially, avoiding decompression bombs and context-retention settings.
The host validates masking direction, reserved bits, opcodes, frame lengths, UTF-8, and close payloads.

### Host support

| Host | Client connect | Server accept | Unsupported result |
| --- | --- | --- | --- |
| Node | yes | yes | ordinary `WebSocketError` or `ServerError` from the attempted operation |
| Wasmtime | yes | no inbound server | handler registration returns `ServerError.Other("WebSocket upgrades are not supported by this host")` |
| Browser playground | URL and subprotocols only; response `headers` is empty | no server | application headers return `UnsupportedHost("custom client headers")`; `ping!` returns `UnsupportedHost("ping")` |
| Cloudflare | yes | yes | `ping!` returns `WebSocketError.UnsupportedHost("ping")`; other operations use ordinary errors |
| Vercel adapter | no in the first implementation | no in the first implementation | connect returns `WebSocketError.UnsupportedHost("client connections")`; handler registration returns the server error above |

Node exposes upgraded sockets to clients and servers through its
[`upgrade` event](https://nodejs.org/api/http.html#event-upgrade).
The [browser constructor](https://developer.mozilla.org/en-US/docs/Web/API/WebSocket/WebSocket) accepts only a URL and protocols, so it cannot honor arbitrary `Request` headers.
[Cloudflare Workers](https://developers.cloudflare.com/workers/examples/websockets/) supports client connections and server `WebSocketPair` upgrades.

The Wasmtime host has outbound HTTP but no inbound server capability.
The current [Vercel adapter](../../packages/server-sdk/adapters/vercel-node.mjs) converts one request to one Fetch response and cannot retain an upgraded socket.
The Vercel row is an adapter limit, not a claim about every Vercel runtime.

## Cost

| Component | Cost |
| --- | --- |
| Handle | one host descriptor plus response headers, protocol, and read limit |
| Read | one buffer up to the message limit; one copy into Wasm per complete message |
| Write | one host transfer; host framing and masking, with bounded control buffers |
| Idle | descriptor only; no hd-side polling or queue |
| Unused program | 0 Wasm bytes expected because the std wrappers remain unreachable |
| Host | client operations on Node, Wasmtime, browser, and Cloudflare; server operations on Node and Cloudflare; explicit unsupported paths elsewhere |
| Web compiler | browser client binding; measure gzip against the 2 MB cap before merge |

The signature package uses `panic("todo")` host bodies, so implementation footprint remains unmeasured.

## Appendix A: Stress test

The checked client sends and receives text, pings, then closes normally.
The server handler authorizes a request, negotiates one protocol, echoes either message kind, and accepts binary through the same loop.

Adversarial cases shape the design:

- A text code point split across frames is validated only after reassembly.
- Empty text and binary messages remain distinct complete messages.
- A fragmented message crossing the read limit closes with `MessageTooBig` before unbounded growth.
- Ping receives a pong while the application is idle or blocked in a write.
- Two simultaneous writes remain whole; a read and write progress together.
- Both peers starting close still produce one terminal `Closed` result and one cleanup.
- No close frame yields `Closed(None, "")`; forbidden wire codes never become sendable defaults.
- A close reason over 123 UTF-8 bytes is `InvalidClose`, not a truncated reason.
- A rejected handshake retains its HTTP body for authentication diagnostics.
- A cross-host redirect strips credentials and rechecks the HTTP grant.
- An unoffered server protocol and a browser Origin outside the allow-list reject before 101.
- A server session longer than 20 seconds is unaffected by the ordinary HTTP handler deadline.
- Browser application headers and browser/Cloudflare pings fail before an unsupported operation starts.
- Wasmtime and Vercel server registration fail before admission; Vercel client connect fails before network I/O.

The design does not expose frames, extensions, streaming message payloads, or unreliable delivery.
Large messages that should not be buffered wait for the shared streaming design's future framed-reader work.

## Appendix B: Prior art

### Go `nhooyr.io/websocket`

```go
c, response, err := websocket.Dial(ctx, url, &websocket.DialOptions{
    HTTPHeader: headers, Subprotocols: []string{"chat.v1"},
})
defer c.CloseNow()
err = c.Write(ctx, websocket.MessageText, []byte("hello"))
kind, data, err := c.Read(ctx)
err = c.Ping(ctx)
err = c.Close(websocket.StatusNormalClosure, "done")
```

nhooyr uses HTTP requests and responses, message-level read/write, a read limit, ping waiting, and explicit close.
Its server `Accept` checks origins and negotiates subprotocols.
hd takes that high-level control-frame behavior and combines `CloseNow` with scoped `dispose!`.
[Package documentation](https://pkg.go.dev/nhooyr.io/websocket).

### Rust `tokio-tungstenite`

```rust
let (mut socket, response) = connect_async(request).await?;
socket.send(Message::Text("hello".into())).await?;
if let Some(message) = socket.next().await {
    println!("{:?}", message?);
}
socket.close(Some(CloseFrame { code: CloseCode::Normal, reason: "done".into() })).await?;
```

tokio-tungstenite exposes a `Stream`/`Sink` of text, binary, ping, pong, and close messages.
It automatically queues ping and close replies but requires later I/O to flush them.
hd keeps message boundaries but hides control frames so applications cannot forget or duplicate protocol replies.
[tokio-tungstenite](https://docs.rs/tokio-tungstenite/latest/tokio_tungstenite/),
[tungstenite messages](https://docs.rs/tungstenite/latest/tungstenite/protocol/enum.Message.html).

### Browser WebSocket

```javascript
const socket = new WebSocket("wss://example.test/chat", ["chat.v1"]);
socket.binaryType = "arraybuffer";
socket.send("hello");
socket.close(1000, "done");
```

The browser API exposes text/binary messages, negotiated protocol, buffered writes, and close codes, but not ping/pong.
hd follows its data-message surface while making liveness checks explicit with `ping!` on non-browser hosts.
[WebSocket API](https://developer.mozilla.org/en-US/docs/Web/API/WebSocket).

## Appendix C: Alternatives considered

**Expose every control frame in `WebSocketMessage`.** This matches tungstenite and enables custom heartbeats.
It also lets an application forget pong or send two close replies; `ping!` covers measured liveness without that hazard.

**Implement byte `Reader` and `Writer`.** Generic copy helpers would work, but message boundaries and text validity disappear.
The proposal uses the same scoped pull/write shape at the message level instead.

**Run sessions inside ordinary HTTP handlers.** This avoids a server-SDK entry point.
It makes every long session a special exception to HTTP deadlines, response delivery, and worker admission.

**Add a second manifest grant.** This distinguishes WebSockets from HTTP.
Both contact the same URL hosts through the same handshake, so two allow-lists can drift without reducing authority.

**Enable compression immediately.** `permessage-deflate` reduces repetitive text traffic.
It adds memory, CPU, context-takeover, and decompression-limit decisions before the base protocol has evidence.

## Appendix D: Today in hd

[`Request` and `Headers`](../../spec/std/http.md#requests-and-responses) model buffered HTTP messages.
The [`Http`](../../spec/std/http.md#client) requirement returns a completed `Response`; it cannot return a live upgraded connection.
The [server SDK](../../spec/std/server-sdk.md) runs ordinary handlers under a twenty-second deadline and expects one response.

[`Disposable` and `using`](../../spec/lang/06-control-flow.md#using-bindings) define today's scoped suspending cleanup.
The pending #967 split would rename this protocol to `AsyncDisposable` and its binding to `using!`.
The [streaming design](STREAMING_DESIGN.md) supplies the handle, timeout, concurrency, finish, and leak-report conventions.
There is no WebSocket client host operation, server upgrade route, frame codec, scripted fake, or close error today.

## Appendix E: Implementation phases

1. Add std message, close, error, handle, and client-provider signatures; test close-code and limit validation.
2. Add Node and Wasmtime client adapters; test handshake rejection, redirects, text, binary, ping, close, and grants.
3. Add Node and Cloudflare server upgrades; test origin, subprotocol, rejection, deadlines, and cancellation.
4. Add the browser client and explicit unsupported paths; test headers and every host-table result.
5. Add `ScriptedWebSocket` and upgrade fakes; test concurrency, automatic control replies, fragmentation, and limits.
6. Measure optimized Wasm and web-compiler gzip; stop for review if the unused or ordinary HTTP surfaces grow.

This design adds no grammar itself; it consumes #967's separate `using!` spelling if that proposal is accepted.
No inference, pattern, ownership, or compiler-cache change is needed.
If accepted, std behavior belongs in `spec/std/http.md`; SDK routing belongs in `spec/std/server-sdk.md`.

## Parse log

The shared-rules Markdown extractor feeds the proposal's `text` block to `scripts/hd-conformance.sh parse`.
This validates syntax only; package checks validate the signatures and examples.
