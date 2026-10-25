# Server SDK

An application owns its ordinary `main!`, one state object and a joined
group of workers. `server.HttpServer` is an SDK-owned package capability;
requests and responses use `std.http` values.

The implementation supplies the typed source, joined workers, bounded
request cleanup and a loopback Node listener. A request races its source
cancellation signal against a handler wrapped in `timeout!(20s, cleanup=100ms)`.
The source supplies its cleanup clock, so `serve!` adds no ambient Clock row.
Every accepted lease is released after response delivery or cleanup.
Lease-scoped outgoing Http dispatch, complete admission deadlines, supervision
and platform packaging remain subsequent #266 slices.
The current listener is a development adapter, not a completed deployment
profile.

From the repository root, build `cargo build -p hd_cli --features wasmtime`.
Then run `hd run` from `examples/server-api`, granting the SDK trait in the
application's manifest. The sample accepts a JSON object with a `message`
string; malformed JSON returns 400. `POST /slow` suspends for 150 ms and
demonstrates overlapping workers. `HD_SERVER_PORT` selects the loopback
port (8080 by default, 0 for an ephemeral test port). SIGTERM closes
admission and joins workers after their admitted requests finish.

Focused checks, with scratch `HD_CACHE`:

```sh
cd packages/server-sdk
hd check --all
hd test --engine node
hd test --engine wasmtime
```

The library tests bind an ordinary HD provider. The live Node check uses
the actual sample and package-provider boundary:

```sh
node --test packages/server-sdk/test/*.test.mjs
```

`HD_SDK_TEST_HD` can name a different compiled CLI. No test needs a public
listener, external network service or deployment credentials.
