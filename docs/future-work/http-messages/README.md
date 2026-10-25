# HTTP message design probes

These are design evidence for [HTTP_MESSAGES.md](../HTTP_MESSAGES.md), not new std APIs or an accepted implementation.

- `before/`: current std.http client/server-shaped exchange, using ScriptedHttp.
- `model/`: ordinary local message types, enums and a captured adapter over an HD loopback fake;13 focused tests.
- `api-catalog.hd`: signature-only declarations, with explicit `panic("shape-only")` bodies for unimplemented methods. Do not execute this as a library.
- `cookie-model/`: tagged cookie provenance and a shared Body with pending form errors; six focused tests.
- `form-model/`: actual bounded Serializer/Deserializer and component-error model; eleven focused tests.

The selected model follows the message-level setters/readers, HeaderName/HttpStatus, ASCII canonicalization, From-only Headers conversion and deferred wire validation decisions.
Its Url accepts one fixture URL; its text conversion is strict and its Decode marker omits the proposed JsonError cause.
It does not implement arbitrary URL/form codecs, actual network providers, automatic profile binding, real SDK leases or production grant checks.
The revised design includes typed forms and whole queries through one codec, plus cookies in all four directions.
The original model predates that revision; its thirteen tests do not establish the new codec or cookie behavior.
The new form model follows one deepObject bracket level with scalar/optional-scalar properties, as clarified in6404702917.
It rejects deeper records, maps, record lists and deepObject child lists when those shapes are visited.
It assumes duplicate-scalar rejection, strict percent/UTF-8 decoding and absent lists becoming empty; those details remain proposed.
Unsupported values reject when visited; empty record lists and absent optional records are accepted under6404818736.
No declared-type preflight is required. Renamed bracket collisions remain unimplemented.
The cookie model injects a precomputed codec Result; it does not implement a cookie/date parser, request-cookie merging or the generic form codec.
Its fake transport proves guest preparation ordering and copying, not actual providers or SDK delivery.

The original model's source baseline is `6ad81d02e5002f86b5f97ea2d1508fa11555ffbb`.
The dev native compiler SHA-256 is `58721916b85652ad9801b45f95b462d5be8e9e97f24e7b263c1b2caf02aa3294`.
Model source SHA-256 is `2a0d111dcdc469c59d485d2cda051f2ab6fae13d71eb192ef9ea5bfcbb200848`.
Before source SHA-256 is `3096905d0321b7cbdecd11ab941a4278b9beffb82ea4263cbd79ee609a763cf9`.

The revision checks pin `8e7f4a9b482fbca478c775d8b9c6ad616fd46990` and a dev native compiler with Wasmtime.
Compiler SHA-256: `cc4d7e8340362f1203d288c8d8f7b4bf68dec9b0185762d2dbf2345ca6792b59`.
Cookie-model source SHA-256: `77dae4778f7333db6f29793f5fe64f6652e23862f6d0c682e97dc138855fb651`.
Form-model source SHA-256: `b614da090cc11ba7e8fc3d34559b7cf05ce5648f41a9115a2a624adb78253189`.

From each package directory, with a compiler providing Node and Wasmtime and an absolute scratch HD_CACHE:

```sh
hd check --all --format json
hd fmt --check
hd run --engine node
hd run --engine wasmtime
hd build
```

The before, original model and form-model mains print exactly `42` and a newline.
The cookie-model main does nothing; its six assertions execute through tests.
Run the focused tests from model, cookie-model or form-model:

```sh
hd test --engine node --format json
hd test --engine wasmtime --format json
```

Check the catalog from the repository root with `hd check --all --format json docs/future-work/http-messages/api-catalog.hd`.
Parsing alone proves syntax; checking the catalog proves type shapes, not stub behavior.
Program sizes depend on compiler identity, profile, package/source paths and debug names; the note reports the pinned scratch-package builds rather than claiming portable byte identity.
