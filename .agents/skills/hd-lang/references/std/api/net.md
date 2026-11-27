# `std.net`

Generated from `hd doc std.net`. Run `hd doc std.net.NAME` in a package for one item's full text.

## `NetError`

```text
pub enum NetError
```

Every failure of a lookup or a socket, as one error enum.

## `Net`

```text
pub trait Net
```

The host capability trait: DNS lookup, TCP, and UDP.

## `lookup`

```text
pub fn lookup!(host: string) -> Result[List[string], NetError] $ Net
```

Resolves `host` through the covering `Net` provider.

## `connect`

```text
pub fn connect!(host: string, port: u16) -> Result[*mut dyn TcpStream, NetError] $ Net
```

Connects through the covering `Net` provider.

## `listen`

```text
pub fn listen!(host: string, port: u16) -> Result[*mut dyn TcpListener, NetError] $ Net
```

Listens through the covering `Net` provider.

## `bind_udp`

```text
pub fn bind_udp!(host: string, port: u16) -> Result[*mut dyn UdpSocket, NetError] $ Net
```

Binds a UDP socket through the covering `Net` provider.

## `TcpStream`

```text
pub trait TcpStream < AsyncDisposable
```

A TCP stream: a closable handle that a `using!` binding disposes.

## `TcpListener`

```text
pub trait TcpListener < AsyncDisposable
```

A TCP listener: a closable handle that a `using!` binding disposes.

## `Datagram`

```text
pub struct Datagram
```

One UDP datagram with its sender.

## `UdpSocket`

```text
pub trait UdpSocket < AsyncDisposable
```

A UDP socket: a closable handle that a `using!` binding disposes.

## `ScriptedStream`

```text
pub struct ScriptedStream
```

A scripted TCP stream: it returns its chunks in order and records what is written.

- `pub fn new(reads: List[List[u8]]) -> *mut ScriptedStream`: Returns a stream that returns the chunks of `reads` in order, one per `read!`.
- `pub fn failing(reads: List[List[u8]], error: NetError) -> *mut ScriptedStream`: Returns a stream that returns the chunks of `reads`, then fails every `read!` and `write!` with `error`.
- `pub fn written(&self) -> List[u8]`: Every byte written to the stream, in order.
- `pub fn timeout(&self) -> Duration?`: The timeout that `set_timeout` stored last, or `.None` before any call.
- `pub fn nodelay(&self) -> bool`: Whether `set_nodelay` last turned nodelay on; `false` before any call.
- `pub fn keepalive(&self) -> Duration?`: The idle time `set_keepalive` stored last, or `.None` before any call or after `.None`.
- `pub fn is_closed(&self) -> bool`: Whether the stream is closed.

## `ScriptedListener`

```text
pub struct ScriptedListener
```

A scripted TCP listener: it accepts its streams in order.

- `pub fn new(connections: List[*mut ScriptedStream]) -> *mut ScriptedListener`: Returns a listener that accepts the streams of `connections` in order.
- `pub fn failing( connections: List[*mut ScriptedStream], error: NetError, ) -> *mut ScriptedListener`: Returns a listener that accepts the streams of `connections`, then fails every `accept!` with `error`.
- `pub fn timeout(&self) -> Duration?`: The timeout that `set_timeout` stored last, or `.None` before any call.
- `pub fn set_local_address(&mut self, host: string, port: u16) -> void`: Makes `local_address()` return `.Ok((host, port))`.
- `pub fn is_closed(&self) -> bool`: Whether the listener is closed.

## `ScriptedUdp`

```text
pub struct ScriptedUdp
```

A scripted UDP socket: it receives its datagrams in order and records what is sent.

- `pub fn new(incoming: List[Datagram]) -> *mut ScriptedUdp`: Returns a socket that receives the datagrams of `incoming` in order.
- `pub fn failing(incoming: List[Datagram], error: NetError) -> *mut ScriptedUdp`: Returns a socket that receives the datagrams of `incoming`, then fails every `receive!` with `error`; `send_to!` still works.
- `pub fn timeout(&self) -> Duration?`: The timeout that `set_timeout` stored last, or `.None` before any call.
- `pub fn set_local_address(&mut self, host: string, port: u16) -> void`: Makes `local_address()` return `.Ok((host, port))`.
- `pub fn sent(&self) -> List[Datagram]`: Every datagram sent, with its destination, in order.
- `pub fn is_closed(&self) -> bool`: Whether the socket is closed.

## `ScriptedNet`

```text
pub struct ScriptedNet
```

The deterministic `Net` provider: it answers from what the test added, records every call, and opens no socket.

- `pub fn new() -> *mut ScriptedNet`: Returns a provider with nothing scripted.
- `pub fn add_host(&mut self, host: string, addresses: List[string]) -> void`: Answers each `lookup!(host)` with `addresses`.
- `pub fn add_connection(&mut self, address: string, stream: *mut ScriptedStream) -> void`: Queues `stream` for one `connect!` of `address` (`host:port`).
- `pub fn fail_connection(&mut self, address: string, error: NetError) -> void`: Queues `error` for one `connect!` of `address` (`host:port`).
- `pub fn add_listener(&mut self, address: string, listener: *mut ScriptedListener) -> void`: Queues `listener` for one `listen!` of `address` (`host:port`).
- `pub fn add_udp(&mut self, address: string, socket: *mut ScriptedUdp) -> void`: Queues `socket` for one `bind_udp!` of `address` (`host:port`).
- `pub fn received(&self) -> List[string]`: The address of every call, in order, the failed ones included; a lookup records the host alone.
