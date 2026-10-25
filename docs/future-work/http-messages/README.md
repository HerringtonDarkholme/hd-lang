# HTTP message design probes

These are design evidence for [HTTP_MESSAGES.md](../HTTP_MESSAGES.md), not new std APIs or an accepted implementation.

- `before/`: current std.http client/server-shaped exchange, using ScriptedHttp.
- `model/`: ordinary local message types, enums and a captured adapter over an HD loopback fake;13 focused tests.
- `api-catalog.hd`: signature-only declarations, with explicit `panic("shape-only")` bodies for unimplemented methods. Do not execute this as a library.

The selected model follows the message-level setters/readers, HeaderName/HttpStatus, ASCII canonicalization, From-only Headers conversion and deferred wire validation decisions.
Its Url accepts one fixture URL; its text conversion is strict and its Decode marker omits the proposed JsonError cause.
It does not implement arbitrary URL/form codecs, actual network providers, automatic profile binding, real SDK leases or production grant checks.
The final scope keeps outgoing set_form and reads incoming forms through text/bytes; there is no typed form reader.

The source baseline is `6ad81d02e5002f86b5f97ea2d1508fa11555ffbb`.
The dev native compiler SHA-256 is `58721916b85652ad9801b45f95b462d5be8e9e97f24e7b263c1b2caf02aa3294`.
Model source SHA-256 is `2a0d111dcdc469c59d485d2cda051f2ab6fae13d71eb192ef9ea5bfcbb200848`.
Before source SHA-256 is `3096905d0321b7cbdecd11ab941a4278b9beffb82ea4263cbd79ee609a763cf9`.

From each package directory, with a compiler providing Node and Wasmtime and an absolute scratch HD_CACHE:

```sh
hd check --all --format json
hd fmt --check
hd run --engine node
hd run --engine wasmtime
hd build
```

Both main programs print exactly `42` and a newline.
Only the model package has tests:

```sh
hd test --engine node --format json
hd test --engine wasmtime --format json
```

Check the catalog from the repository root with `hd check --all --format json docs/future-work/http-messages/api-catalog.hd`.
Parsing alone proves syntax; checking the catalog proves type shapes, not stub behavior.
Program sizes depend on compiler identity, profile, package/source paths and debug names; the note reports the pinned scratch-package builds rather than claiming portable byte identity.
