# Net

Status: standard library specification draft.

This chapter defines `std.net`, which `lib/std` writes in ordinary hd
over the language tier:

- the host capability trait `Net`: DNS lookup, TCP, and UDP;
- the handle traits `TcpStream`, `TcpListener`, and `UdpSocket`, and the
  data type `Datagram`;
- the error enum `NetError`.

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
| r[std-net.error.other] Other | `Other(message)` | any other failure, with the provider's message |

4. r[std-net.error.not-granted.text] The `Display` text of `NotGranted(address)` is `net access to ADDRESS is not granted; run with --cap Net=ADDRESS`, with the address for `ADDRESS`.

## Net Trait

`Net` resolves names and opens sockets. Each socket is a handle, which the
call returns:

```text
pub trait Net:
    fn lookup!(mut self, host: string) -> Result[List[string], NetError]
    fn connect!(mut self, host: string, port: u16) -> Result[mut TcpStream, NetError]
    fn listen!(mut self, host: string, port: u16) -> Result[mut TcpListener, NetError]
    fn bind_udp!(mut self, host: string, port: u16) -> Result[mut UdpSocket, NetError]
```

1. r[std-net.trait.decl] `std.net` declares the host capability trait `Net` with the methods above. Code imports it, as in `use std.net.Net`.
2. r[std-net.lookup] `lookup!(host)` returns the IP addresses of `host`, as text, in the resolver's order.
3. r[std-net.connect] `connect!(host, port)` opens a TCP connection to `host` and `port`, and returns its stream.
4. r[std-net.listen] `listen!(host, port)` listens for TCP connections on the local address `host` and `port`, and returns its listener.
5. r[std-net.bind-udp] `bind_udp!(host, port)` binds a UDP socket to the local address `host` and `port`, and returns it.
6. r[std-net.mut] Each method takes `mut self`, so `Net` is a mutable requirement trait, and a provider may record what it opens.

## Socket Handles

A stream, a listener, and a UDP socket are closable handles:

```text
use std.resource.ResourceError

pub trait TcpStream:
    fn read!(mut self, max: usize) -> Result[List[u8], ResourceError[NetError]]
    fn write!(mut self, bytes: List[u8]) -> Result[void, ResourceError[NetError]]
    fn close(mut self) -> Result[void, ResourceError[NetError]]

pub trait TcpListener:
    fn accept!(mut self) -> Result[mut dyn TcpStream, ResourceError[NetError]]
    fn close(mut self) -> Result[void, ResourceError[NetError]]

pub data Datagram:
    pub bytes: List[u8]
    pub host: string
    pub port: u16

pub trait UdpSocket:
    fn send_to!(mut self, host: string, port: u16, bytes: List[u8]) -> Result[void, ResourceError[NetError]]
    fn receive!(mut self, max: usize) -> Result[Datagram, ResourceError[NetError]]
    fn close(mut self) -> Result[void, ResourceError[NetError]]
```

1. r[std-net.handles.decl] `std.net` declares `TcpStream`, `TcpListener`, `Datagram`, and `UdpSocket` as above. Code imports them, as in `use std.net.TcpStream`.
2. r[std-net.handles.closable] Each handle is a closable handle by [Closable Handles](../lang/10-modules.md#closable-handles), so every operation after `close` returns `.Err(ResourceError.Disposed)`.
3. r[std-net.read] `read!(max)` waits for at least one byte, and returns at most `max` bytes. At the end of the stream, it returns an empty list.
4. r[std-net.write] `write!(bytes)` completes once every byte is written.
5. r[std-net.accept] `accept!()` waits for the next connection, and returns its stream.
6. r[std-net.send-to] `send_to!(host, port, bytes)` sends `bytes` as one datagram to `host` and `port`.
7. r[std-net.send-to.grant] The grant covers each `send_to!` by its own address, as it covers `connect!`.
8. r[std-net.receive] `receive!(max)` waits for one datagram, and returns at most `max` of its bytes, with the sender's host and port.
9. r[std-net.datagram.traits] `Datagram` implements `Eq` and `Debug`.

> **Why.** A socket stays open across calls, so it is a handle, as a file
> handle is. Closing it follows the existing closable handle rules, with
> no new mechanism.

## Net Providers

`std.net` declares no deterministic provider. A unit test declares its
own, and binds it with `$.with`:

```text
use std.net.{Net, NetError, TcpListener, TcpStream, UdpSocket}
use std.testing.assert_equal

data NoNetwork: pass

impl Net for NoNetwork:
    fn lookup!(mut self, host: string) -> Result[List[string], NetError]:
        .Err(.Dns(host))
    fn connect!(mut self, host: string, port: u16) -> Result[mut TcpStream, NetError]:
        .Err(.Refused("${host}:${port}"))
    fn listen!(mut self, host: string, port: u16) -> Result[mut TcpListener, NetError]:
        .Err(.Refused("${host}:${port}"))
    fn bind_udp!(mut self, host: string, port: u16) -> Result[mut UdpSocket, NetError]:
        .Err(.Refused("${host}:${port}"))

fn addresses!(host: string) -> List[string] $ Net:
    match $.use(Net).lookup!(host):
        .Ok(found) => found
        .Err(_) => []

tests:
    it("finds no address without a network"):
        let mut net = NoNetwork {}
        $.with(Net=net):
            assert_equal(addresses!("example.com").len(), 0, reason="no lookup answers")
```

1. r[std-net.provider.none] `std.net` declares no provider of `Net`, so a test that needs one implements the trait itself.

See also: [Http](http.md), [Capability Grants](../cli/command-line.md#capability-grants).
