# Net

Status: standard library specification draft.

This chapter defines `std.net`, which `lib/std` writes in ordinary hd
over the language tier:

- the host capability trait `Net`: DNS lookup, TCP, and UDP;
- the handle traits `TcpStream`, `TcpListener`, and `UdpSocket`, and the
  struct value `Datagram`;
- the error enum `NetError`;
- the deterministic providers `ScriptedNet`, `ScriptedStream`,
  `ScriptedListener`, and `ScriptedUdp`.

Which provider a command binds, and which addresses its grant covers, is
CLI tier ([Host Capabilities](../cli/command-line.md#host-capabilities)).
For HTTP, [`std.http`](http.md) needs no socket.

## Net Errors

Every failure of a lookup or a socket is one error enum:

```text
pub enum NetError:
    NotGranted(address: string)
    InvalidAddress(address: string)
    Dns(host: string)
    Refused(address: string)
    Timeout
    Closed
    Other(message: string)
```

1. r[std-net.error.decl] `std.net` declares `NetError` as above. Code imports it, as in `use std.net.NetError`.
2. r[std-net.error.traits] `NetError` implements `Eq`, `Debug`, `Display`, and `std.error.Error`.
3. r[std-net.error.address] The address of a call is `host:port` for a socket call, and the host alone for a lookup.

| Rule | Variant | Means |
| --- | --- | --- |
| r[std-net.error.not-granted] Not granted | `NotGranted(address)` | the program's capability grant does not cover the call's address, by [Partial Deny](../cli/command-line.md#partial-deny) |
| r[std-net.error.invalid-address] Invalid address | `InvalidAddress(address)` | the host is not a name or an IP address |
| r[std-net.error.dns] DNS | `Dns(host)` | the host name did not resolve |
| r[std-net.error.refused] Refused | `Refused(address)` | the peer refused the connection, or the address is in use |
| r[std-net.error.timeout] Timeout | `Timeout` | a bounded wait ended before the peer answered, by [Timeouts](#timeouts) |
| r[std-net.error.closed] Closed | `Closed` | the handle was closed or disposed, by [Socket Handles](#socket-handles) |
| r[std-net.error.other] Other | `Other(message)` | any other failure, with the provider's message |

4. r[std-net.error.not-granted.plain] The `Display` text of `NotGranted(address)` is `net access to ADDRESS is not granted`, with the address for `ADDRESS`.
5. r[std-net.error.timeout.text] The `Display` text of `Timeout` is `the net operation timed out`.
6. r[std-net.error.closed.text] The `Display` text of `Closed` is `the socket is closed`.

## Net Trait

`Net` resolves names and opens sockets. Each socket is a handle, which the
call returns behind a trait value reference:

```text
pub trait Net:
    fn lookup!(&mut self, host: string) -> Result[List[string], NetError]
    fn connect!(&mut self, host: string, port: u16) -> Result[*mut dyn TcpStream, NetError]
    fn listen!(&mut self, host: string, port: u16) -> Result[*mut dyn TcpListener, NetError]
    fn bind_udp!(&mut self, host: string, port: u16) -> Result[*mut dyn UdpSocket, NetError]
```

1. r[std-net.trait.decl-composed] `std.net` declares the host capability trait `Net` with the methods above.
2. r[std-net.lookup] `lookup!(host)` returns the IP addresses of `host`, as text, in the resolver's order.
3. r[std-net.connect] `connect!(host, port)` opens a TCP connection to `host` and `port`, and returns its stream.
4. r[std-net.connect.policy-deadline] A connect bounded by `apply!(timeout(d))` returns `.Err(NetError.Timeout)` when the deadline wins.
5. r[std-net.connect.cancel] Cancelling a pending `connect!` leaves no socket open.
6. r[std-net.listen] `listen!(host, port)` listens for TCP connections on the local address `host` and `port`, and returns its listener.
7. r[std-net.listen.reuse] On Unix, `listen!` may bind an address whose old connections are in `TIME_WAIT`. Two live listeners never share an address: the second `listen!` returns `.Err(NetError.Refused(address))`.
8. r[std-net.bind-udp] `bind_udp!(host, port)` binds a UDP socket to the local address `host` and `port`, and returns it.
9. r[std-net.mut-ref.reference-values] Each method takes `&mut self`, so `Net` is a mutable requirement trait, `$.use(Net)` is a `&mut dyn Net` by [`req.ref.use.reference-values`](../lang/11-requirements-and-suspension.md#r-req.ref.use.reference-values), and a provider may record what it opens.

> **Why.** A connect has no handle yet, so its deadline composes through
> `apply!(timeout(...))` rather than a handle setting or another trait method.
> A restarted server rebinds its address at once on every host, without
> an option that would let a second socket take the port on Windows.

## Free Helpers

The free helpers call the `Net` provider that covers the call.
For example, `lookup!("example.test")` starts a lookup without an explicit
`$.use(Net)` expression.

1. r[std-net.helper.lookup.decl] `std.net` declares `pub fn lookup!(host: string) -> Result[List[string], NetError] $ Net`.
2. r[std-net.helper.lookup.forward] `lookup!(host)` returns `$.use(Net).lookup!(host)` without changing the result.
3. r[std-net.helper.connect.decl] `std.net` declares `pub fn connect!(host: string, port: u16) -> Result[*mut dyn TcpStream, NetError] $ Net`.
4. r[std-net.helper.connect.forward] `connect!(host, port)` returns `$.use(Net).connect!(host, port)` without changing the result.
5. r[std-net.helper.listen.decl] `std.net` declares `pub fn listen!(host: string, port: u16) -> Result[*mut dyn TcpListener, NetError] $ Net`.
6. r[std-net.helper.listen.forward] `listen!(host, port)` returns `$.use(Net).listen!(host, port)` without changing the result.
7. r[std-net.helper.bind-udp.decl] `std.net` declares `pub fn bind_udp!(host: string, port: u16) -> Result[*mut dyn UdpSocket, NetError] $ Net`.
8. r[std-net.helper.bind-udp.forward] `bind_udp!(host, port)` returns `$.use(Net).bind_udp!(host, port)` without changing the result.

## Socket Handles

A stream, a listener, and a UDP socket are closable handles, which a
`using` binding disposes:

```text
use std.ops.Disposable
use std.time.Duration

pub trait TcpStream < Disposable:
    fn read!(&mut self, max: usize) -> Result[List[u8], NetError]
    fn write!(&mut self, bytes: List[u8]) -> Result[void, NetError]
    fn set_timeout(&mut self, timeout: Duration?) -> Result[void, NetError]
    fn set_nodelay(&mut self, enabled: bool) -> Result[void, NetError]
    fn set_keepalive(&mut self, idle: Duration?) -> Result[void, NetError]
    fn close(&mut self) -> Result[void, NetError]

pub trait TcpListener < Disposable:
    fn accept!(&mut self) -> Result[*mut dyn TcpStream, NetError]
    fn set_timeout(&mut self, timeout: Duration?) -> Result[void, NetError]
    fn local_address(self) -> Result[(string, u16), NetError]
    fn close(&mut self) -> Result[void, NetError]

pub struct Datagram:
    pub bytes: List[u8]
    pub host: string
    pub port: u16

pub trait UdpSocket < Disposable:
    fn send_to!(&mut self, host: string, port: u16, bytes: List[u8]) -> Result[void, NetError]
    fn receive!(&mut self, max: usize) -> Result[Datagram, NetError]
    fn set_timeout(&mut self, timeout: Duration?) -> Result[void, NetError]
    fn local_address(self) -> Result[(string, u16), NetError]
    fn close(&mut self) -> Result[void, NetError]
```

1. r[std-net.handles.decl-ref] `std.net` declares `TcpStream`, `TcpListener`, `Datagram`, and `UdpSocket` as above. `Datagram` is a struct value: a copy is independent of the value it was copied from. Code imports them, as in `use std.net.TcpStream`.
2. r[std-net.handles.ref] A method that opens a handle returns it as a trait value reference: `&mut dyn TcpStream`, `&mut dyn TcpListener`, or `&mut dyn UdpSocket`. Every copy of the reference reaches the one socket, so a helper that takes it changes the caller's socket, and `is` tells whether two refer to the same one.
3. r[std-net.handles.closed] Each handle is a closable handle by [Closable Handles](../lang/10-modules.md#closable-handles), so every operation after `close` or a dispose returns `.Err(NetError.Closed)`.
4. r[std-net.close.release] `close` on a `TcpStream` releases its socket at once, as a full close of both directions.
5. r[std-net.close.pending-closed] An operation that is pending on a `TcpStream` when it closes completes with `.Err(NetError.Closed)`.
6. r[std-net.dispose] A handle's `dispose!` closes it as `close` does, and reports nothing. Disposing a closed handle does nothing.
7. r[std-net.read] `read!(max)` waits for at least one byte, and returns at most `max` bytes. At the end of the stream, it returns an empty list.
8. r[std-net.write] `write!(bytes)` completes once every byte is written.
9. r[std-net.accept] `accept!()` waits for the next connection, and returns its stream.
10. r[std-net.send-to] `send_to!(host, port, bytes)` sends `bytes` as one datagram to `host` and `port`.
11. r[std-net.send-to.grant] The grant covers each `send_to!` by its own address, as it covers `connect!`.
12. r[std-net.receive] `receive!(max)` waits for one datagram, and returns at most `max` of its bytes, with the sender's host and port.
13. r[std-net.cancel.read] Cancelling the suspension of `read!` or `receive!` keeps the bytes it has not returned for the next `read!` or `receive!` on that handle.
14. r[std-net.datagram.traits] `Datagram` implements `Eq` and `Debug`.

> **Why.** A socket stays open across calls, so it is a handle, as a file
> handle is. Closing it follows the existing closable handle rules, with
> no new mechanism.
>
> A full close frees the descriptor at once, so a program that opens many
> short connections does not run out of them.

## Timeouts

A timeout bounds each wait for the peer. `set_timeout` sets it on a
handle, and `apply!(timeout(...))` bounds a connect:

```text
use std.net.{Net, NetError}
use std.task.{apply, timeout}
use std.time.{Clock, s}

fn banner!(host: string, port: u16) -> Result[string, NetError] $ Net + Clock:
    let net: *mut dyn Net = $.use(Net)
    connected := apply!(timeout(3s)):
        net.connect!(host, port)
    stream := connected?
    match stream.set_timeout(.Some(5s)):
        .Ok(_) => pass
        .Err(_) => return .Err(.Other("set timeout failed"))
    reply := match stream.read!(128):
        .Ok(bytes) => bytes
        .Err(.Timeout) => return .Err(.Timeout)
        .Err(_) => return .Err(.Other("read failed"))
    _ := stream.close()
    .Ok(string::from_utf8(reply).unwrap_or(""))
```

1. r[std-net.timeout.set] `set_timeout(.Some(d))` bounds each later `read!`, `accept!`, or `receive!` on the handle to `d`, and no other call. `set_timeout(.None)` removes the bound, and a new handle has none.
2. r[std-net.timeout.error] A wait that its bound ends returns `.Err(NetError.Timeout)`. The handle stays open, and the bytes not yet returned stay for the next call.
3. r[std-net.timeout.cancel] Cancelling the suspension of a bounded call aborts its wait by the existing rules, and never returns `Timeout`.
4. r[std-net.timeout.host-time] A net timeout counts host time, never the `Clock` provider, so a `ManualClock` cannot end it.
5. r[std-net.timeout.zero] A zero timeout ends each bounded wait at once. A `Duration` is never negative, so no timeout is.

> **Why.** A read runs in a loop, so one setting beats an argument on
> every call, as Rust's `set_read_timeout` and Java's `setSoTimeout` do.
> A timed-out `write!` leaves an unknown number of bytes sent, so it has no
> timeout; a program bounds it with `race!` and `sleep!`. A timed-out read
> loses nothing, so the program may retry it.

## Socket Options

A `TcpStream` has two options, set on the handle once it exists:

```text
use std.net.{Net, NetError}
use std.time.s

fn open_chat!(host: string, port: u16) -> Result[void, NetError] $ Net:
    net := $.use(Net)
    stream := net.connect!(host, port)?
    match stream.set_nodelay(true):
        .Ok(_) => pass
        .Err(_) => return .Err(.Other("nodelay failed"))
    match stream.set_keepalive(.Some(30s)):
        .Ok(_) => pass
        .Err(_) => return .Err(.Other("keepalive failed"))
    match stream.write!("hello\n".to_utf8()):
        .Ok(_) => .Ok(())
        .Err(_) => .Err(.Other("write failed"))
```

1. r[std-net.option.nodelay] `set_nodelay(enabled)` turns `TCP_NODELAY` on or off, so small writes are sent without waiting to be joined.
2. r[std-net.option.keepalive] `set_keepalive(.Some(d))` turns TCP keep-alive on, with `d` of idle time before the first probe. `set_keepalive(.None)` turns it off.
3. r[std-net.option.default] A stream, connected or accepted, starts with each option at the system default. It inherits nothing from its listener.
4. r[std-net.option.after-close] Setting an option after `close` returns `.Err(NetError.Closed)`.
5. r[std-net.option.failure] An option that the system refuses returns `.Err(NetError.Other(message))`, with the system's message.

> **Why.** Nodelay serves a latency-sensitive line or RPC protocol, and
> keep-alive a long-lived connection behind NAT; every host can set both.
> Buffer sizes are left out, since Node cannot set them and systems round
> them. Address reuse is fixed by `std-net.listen.reuse` instead. Linger,
> TTL, broadcast, and multicast wait for a program that needs them.

## Local Address

A listener or a UDP socket reports the address it is bound to, such as
the port the system picked for port 0:

```text
use std.net.{Net, NetError}

fn start_server!() -> Result[u16, NetError] $ Net:
    net := $.use(Net)
    listener := net.listen!("127.0.0.1", 0)?
    match listener.local_address():
        .Ok((_, port)) => .Ok(port)
        .Err(_) => .Err(.Other("no local address"))
```

1. r[std-net.local-address] `local_address()` returns the bound host, as an IP address in text, and the port. After `listen!` or `bind_udp!` with port 0, the port is the one the system picked.
2. r[std-net.local-address.name] The host is the bound IP address, not the name the caller passed. So `listen!("localhost", 0)` reports `127.0.0.1` or `::1`, and `0.0.0.0` reports `0.0.0.0`.
3. r[std-net.local-address.after-close] `local_address()` after `close` returns `.Err(NetError.Closed)`.

> **Why.** `listen!(host, 0)` is how a test server gets a free port, as
> with Rust's `local_addr`. A `(string, u16)` pair reuses the host and port
> that `Datagram` carries, so no address type is added.

## Net Providers

A unit test binds the deterministic provider [`ScriptedNet`](#scripted-net)
with `$.with`. A test that needs other behavior declares its own provider,
as `NoNetwork` does:

```text
use std.net.{Net, NetError, TcpListener, TcpStream, UdpSocket}
struct NoNetwork: pass

impl Net for NoNetwork:
    fn lookup!(&mut self, host: string) -> Result[List[string], NetError]:
        .Err(.Dns(host))
    fn connect!(&mut self, host: string, port: u16) -> Result[*mut dyn TcpStream, NetError]:
        .Err(.Refused("${host}:${port}"))
    fn listen!(&mut self, host: string, port: u16) -> Result[*mut dyn TcpListener, NetError]:
        .Err(.Refused("${host}:${port}"))
    fn bind_udp!(&mut self, host: string, port: u16) -> Result[*mut dyn UdpSocket, NetError]:
        .Err(.Refused("${host}:${port}"))

fn addresses!(host: string) -> List[string] $ Net:
    match $.use(Net).lookup!(host):
        .Ok(found) => found
        .Err(_) => []

tests:
    use std.testing.assert_equal

    it("finds no address without a network"):
        net := box(NoNetwork {})
        $.with(Net=net):
            assert_equal(addresses!("example.com").len(), 0, reason="no lookup answers")
```

1. r[std-net.provider.scripted] The deterministic provider of `Net` is `ScriptedNet`, by [Scripted Net](#scripted-net).

## Scripted Net

`ScriptedNet` is the deterministic `Net` provider. A test builds each
socket handle, adds it to the provider, and keeps it to check what the
code under test did:

```text
use std.net.{Net, NetError, ScriptedNet, ScriptedStream}

fn text_of(bytes: List[u8]) -> string:
    match string::from_utf8(bytes):
        .Ok(text) => text
        .Err(_) => ""

fn lookup_value!(key: string) -> Result[string, NetError] $ Net:
    net := $.use(Net)
    stream := net.connect!("cache.local", 6379)?
    match stream.write!("GET ${key}\n".to_utf8()):
        .Ok(_) => pass
        .Err(_) => return .Err(.Other("write failed"))
    reply := match stream.read!(64):
        .Ok(bytes) => text_of(bytes)
        .Err(_) => return .Err(.Other("read failed"))
    _ := stream.close()
    .Ok(reply.trim())

tests:
    use std.testing.assert_equal

    it("sends the command and returns the reply"):
        stream := ScriptedStream::new(["VALUE 42\n".to_utf8()])
        net := ScriptedNet::new()
        net.add_connection("cache.local:6379", stream)
        $.with(Net=net):
            assert_equal(lookup_value!("answer"), .Ok("VALUE 42"), reason="reply")
        assert_equal(stream.written(), "GET answer\n".to_utf8(), reason="the command")
        assert_equal(stream.is_closed(), true, reason="closed")
        assert_equal(net.received(), ["cache.local:6379"], reason="one connect")

    it("reports a failed read"):
        stream := ScriptedStream::failing([], NetError.Other("connection reset"))
        net := ScriptedNet::new()
        net.add_connection("cache.local:6379", stream)
        $.with(Net=net):
            assert_equal(lookup_value!("answer"), .Err(.Other("read failed")), reason="reset")
```

1. r[std-net.scripted.decl] `std.net` declares `ScriptedNet`, `ScriptedStream`, `ScriptedListener`, and `ScriptedUdp`, with private fields. Each implements `Debug` and its trait: `Net`, `TcpStream`, `TcpListener`, or `UdpSocket`. Code imports them, as in `use std.net.ScriptedNet`.
2. r[std-net.scripted.new-ref] The constructors below each return a new object behind `*mut`. Adding it to the provider shares the object, so the handle that the provider returns is the one the test holds and checks.

| Constructor | Returns |
| --- | --- |
| `ScriptedNet::new() -> *mut ScriptedNet` | a provider with nothing scripted |
| `ScriptedStream::new(reads: List[List[u8]]) -> *mut ScriptedStream` | a stream that returns the chunks of `reads` |
| `ScriptedStream::failing(reads: List[List[u8]], error: NetError) -> *mut ScriptedStream` | a stream that returns the chunks of `reads`, then fails with `error` |
| `ScriptedListener::new(connections: List[*mut ScriptedStream]) -> *mut ScriptedListener` | a listener that accepts the streams of `connections` |
| `ScriptedUdp::new(incoming: List[Datagram]) -> *mut ScriptedUdp` | a UDP socket that receives the datagrams of `incoming` |

3. r[std-net.scripted.setup-ref] `ScriptedNet` has the setup methods below, and each `address` is `host:port` by [`std-net.error.address`](#r-std-net.error.address).

| Method | Answers |
| --- | --- |
| `add_host(&mut self, host: string, addresses: List[string]) -> void` | each `lookup!(host)`, with `.Ok(addresses)` |
| `add_connection(&mut self, address: string, stream: *mut ScriptedStream) -> void` | one `connect!` of `address`, with `.Ok(stream)` |
| `fail_connection(&mut self, address: string, error: NetError) -> void` | one `connect!` of `address`, with `.Err(error)` |
| `add_listener(&mut self, address: string, listener: *mut ScriptedListener) -> void` | one `listen!` of `address`, with `.Ok(listener)` |
| `add_udp(&mut self, address: string, socket: *mut ScriptedUdp) -> void` | one `bind_udp!` of `address`, with `.Ok(socket)` |

4. r[std-net.scripted.queue] The connections and failures added for one address are one queue, and each `connect!` of the address takes the next entry. Each `listen!` or `bind_udp!` likewise takes the next handle added for its address.
5. r[std-net.scripted.lookup-unknown] `lookup!` of a host that no `add_host` names returns `.Err(NetError.Dns(host))`.
6. r[std-net.scripted.refused] `connect!`, `listen!`, or `bind_udp!` of an address with no entry, or with an emptied queue, returns `.Err(NetError.Refused(address))`.
7. r[std-net.scripted.received-connect] `received(&self) -> List[string]` returns every call's address in order, failed calls included, by [`std-net.error.address`](#r-std-net.error.address).
8. r[std-net.scripted.read] A `ScriptedStream` returns the chunks of `reads` in order, one per `read!`.
9. r[std-net.scripted.read-max] A `read!(max)` returns at most `max` bytes of the front chunk, and keeps the rest at the front for the next `read!`.
10. r[std-net.scripted.eof] An empty chunk is the peer's close: that `read!` and every later one return an empty list.
11. r[std-net.scripted.exhausted] A `read!` past the last chunk of a `new` stream returns `.Err(NetError.Other(message))` at once, and `message` is `ScriptedStream has no more reads`.
12. r[std-net.scripted.write] `write!(bytes)` records `bytes` and returns `.Ok(())`. `written(&self) -> List[u8]` returns every recorded byte, in order.
13. r[std-net.scripted.failing] A `failing` stream returns `.Err(error)` from the `read!` past its last chunk, and from every later `read!` and `write!`.
14. r[std-net.scripted.accept] `accept!` returns the streams of `connections` in order. Past the last, it returns `.Err(NetError.Other(message))` at once, and `message` is `ScriptedListener has no more connections`.
15. r[std-net.scripted.udp] `receive!(max)` returns the datagrams of `incoming` in order, each cut to its first `max` bytes. Past the last, it returns `.Err(NetError.Other(message))` at once, and `message` is `ScriptedUdp has no more datagrams`. `send_to!` records its datagram and returns `.Ok(())`, and `sent(&self) -> List[Datagram]` returns each one, with the destination's host and port, in order.
16. r[std-net.scripted.closed-error] After `close` or a dispose, every operation of a scripted handle returns `.Err(NetError.Closed)`, so `close` returns `.Ok(())` only once. `is_closed(&self) -> bool` on each scripted handle tells whether it is closed.
17. r[std-net.scripted.dispose] A scripted handle's `dispose!` marks it closed, as `close` does, and returns nothing.
18. r[std-net.scripted.timeout] `set_timeout` on a scripted handle stores its argument and returns `.Ok(())`. `timeout(&self) -> Duration?` returns the stored value, and `.None` before any call.
19. r[std-net.scripted.options] `set_nodelay` and `set_keepalive` on a `ScriptedStream` store their argument and return `.Ok(())`. `nodelay(&self) -> bool` and `keepalive(&self) -> Duration?` return them, and `false` and `.None` before any call.
20. r[std-net.scripted.local-address] `set_local_address(&mut self, host: string, port: u16) -> void` on a `ScriptedListener` or `ScriptedUdp` makes `local_address()` return `.Ok((host, port))`.
21. r[std-net.scripted.local-address.unset] Without it, `local_address()` returns `.Err(NetError.Other(message))`. The `message` is `ScriptedListener has no local address` or `ScriptedUdp has no local address`.
22. r[std-net.scripted.no-host] A scripted provider opens no socket and consults no grant, so `NotGranted` appears only where a test scripts it.

A server accepts a scripted peer, and a client retries a refused
connection and pings over UDP:

```text
use std.net.{Datagram, Net, NetError, ScriptedListener, ScriptedNet, ScriptedStream, ScriptedUdp}

fn serve_one!() -> Result[usize, NetError] $ Net:
    net := $.use(Net)
    listener := net.listen!("0.0.0.0", 7)?
    stream := match listener.accept!():
        .Ok(connection) => connection
        .Err(_) => return .Err(.Other("accept failed"))
    let total: usize = 0
    while true:
        chunk := match stream.read!(1024):
            .Ok(bytes) => bytes
            .Err(_) => return .Err(.Other("read failed"))
        if chunk.is_empty():
            break
        total = total + chunk.len()
        match stream.write!(chunk):
            .Ok(_) => pass
            .Err(_) => return .Err(.Other("write failed"))
    _ := stream.close()
    _ := listener.close()
    .Ok(total)

fn connect_with_retry!(host: string, port: u16, attempts: usize) -> Result[void, NetError] $ Net:
    net := $.use(Net)
    let last = NetError.Refused("${host}:${port}")
    let tried: usize = 0
    while tried < attempts:
        match net.connect!(host, port):
            .Ok(_) => return .Ok(())
            .Err(error) => last = error
        tried = tried + 1
    .Err(last)

fn ping!(host: string, port: u16) -> Result[bool, NetError] $ Net:
    net := $.use(Net)
    socket := net.bind_udp!("0.0.0.0", 0)?
    match socket.send_to!(host, port, "ping".to_utf8()):
        .Ok(_) => pass
        .Err(_) => return .Err(.Other("send failed"))
    match socket.receive!(16):
        .Ok(datagram) => .Ok(datagram.bytes == "pong".to_utf8())
        .Err(_) => .Ok(false)

tests:
    use std.testing.assert_equal

    it("echoes every chunk until the peer closes"):
        peer := ScriptedStream::new(["he".to_utf8(), "llo".to_utf8(), []])
        net := ScriptedNet::new()
        net.add_listener("0.0.0.0:7", ScriptedListener::new([peer]))
        $.with(Net=net):
            assert_equal(serve_one!(), .Ok(5), reason="five bytes")
        assert_equal(peer.written(), "hello".to_utf8(), reason="echoed")

    it("retries after a refused connection"):
        net := ScriptedNet::new()
        net.fail_connection("db.local:5432", NetError.Refused("db.local:5432"))
        net.add_connection("db.local:5432", ScriptedStream::new([]))
        $.with(Net=net):
            assert_equal(connect_with_retry!("db.local", 5432, 3), .Ok(()), reason="second try works")
        assert_equal(net.received().len(), 2, reason="two connects")

    it("answers a ping with a scripted datagram"):
        socket := ScriptedUdp::new([Datagram { bytes: "pong".to_utf8(), host: "10.0.0.7", port: 9000 }])
        net := ScriptedNet::new()
        net.add_udp("0.0.0.0:0", socket)
        $.with(Net=net):
            assert_equal(ping!("10.0.0.7", 9000), .Ok(true), reason="pong")
        assert_equal(socket.sent(), [Datagram { bytes: "ping".to_utf8(), host: "10.0.0.7", port: 9000 }], reason="the ping")
```

> **Why.** A scripted call never waits, so a test cannot hang, and a call
> with nothing scripted fails with the error a real network gives. A test
> scripts a timeout as an error, with `ScriptedStream::failing(reads,
> NetError.Timeout)` or `fail_connection(address, NetError.Timeout)`.
>
> `written()` joins every write, since TCP keeps no write boundaries, while
> a datagram keeps its own. The test names a scripted local address, since
> echoing the matched `host:port` would report port 0.

See also: [Http](http.md), [Capability Grants](../cli/command-line.md#capability-grants),
[Unit Test Providers](testing.md#unit-test-providers).


## Timeout Conversion

1. r[std-net.error.from-timed-out] `NetError` implements `std.convert.From[std.task.TimedOut]`, converting every deadline error to `.Timeout`.
