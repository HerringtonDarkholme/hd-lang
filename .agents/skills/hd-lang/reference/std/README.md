# The Standard Library, One File Per Module

Each file lists one module's public items, one per line: full name,
signature, and a one-line summary. Generated from `hd doc` by
`scripts/gen-std-api.mjs`; don't edit by hand.

Open the one module you need, or search them all:

```sh
grep -rh 'split' reference/std/          # a name in any module
grep -h 'pub fn' reference/std/time.txt  # the functions of one module
```

For what to use for a job, read [std.md](../../std.md) first. For one
item's full text and examples, run `hd doc std.time.sleep` inside any package.

| Module | File | Items | First names |
| --- | --- | ---: | --- |
| `std.text` | [text.txt](text.txt) | 60 | `r`, `interpolate`, `EscapeError`, `process_escapes`, `Utf8Error`, `Parse`, `ParseBoolError`, `ParseCharError`, ... |
| `std.num` | [num.txt](num.txt) | 100 | `Num`, `Integer`, `Float`, `ParseNumberError`, `TryFromIntError`, `N`, `i8`, `i16`, ... |
| `std.convert` | [convert.txt](convert.txt) | 3 | `From`, `TryFrom`, `CharTryFromError` |
| `std.cmp` | [cmp.txt](cmp.txt) | 14 | `Ordering`, `PartialOrd`, `Ord`, `min`, `max`, `clamp`, `Reverse`, `Id`, ... |
| `std.ops` | [ops.txt](ops.txt) | 52 | `NumSuffix`, `num_suffix`, `StrPrefix`, `str_prefix`, `Template`, `Add`, `Sub`, `Mul`, ... |
| `std.result` | [result.txt](result.txt) | 15 | `Result` |
| `std.collections.list` | [collections.list.txt](collections.list.txt) | 60 | `before`, `counts`, `ListDrain`, `list_reserve`, `List` |
| `std.collections.map` | [collections.map.txt](collections.map.txt) | 15 | `MapKeys`, `MapValues`, `MapDrain`, `Map` |
| `std.collections.set` | [collections.set.txt](collections.set.txt) | 12 | `Set`, `SetIter`, `Union`, `Intersection` |
| `std.collections.deque` | [collections.deque.txt](collections.deque.txt) | 13 | `Deque`, `DequeIter` |
| `std.collections.heap` | [collections.heap.txt](collections.heap.txt) | 8 | `Heap` |
| `std.iter` | [iter.txt](iter.txt) | 53 | `Iterator`, `ByRef`, `Sum`, `Product`, `DynIterator`, `FromFn`, `from_fn`, `MapIter`, ... |
| `std.console` | [console.txt](console.txt) | 15 | `Console`, `ConsoleInput`, `read_line`, `read_all`, `MemoryConsole`, `println`, `print`, `eprintln`, ... |
| `std.fs` | [fs.txt](fs.txt) | 25 | `FsError`, `EntryKind`, `Entry`, `FsRead`, `FsWrite`, `read_text`, `write_text`, `copy`, ... |
| `std.path` | [path.txt](path.txt) | 7 | `Path` |
| `std.cli` | [cli.txt](cli.txt) | 28 | `ArgKind`, `Arg`, `ArgParser`, `Parsed`, `CliError`, `Cli`, `Short`, `short`, ... |
| `std.host` | [host.txt](host.txt) | 14 | `Signal`, `Signals`, `ScriptedSignals`, `Env`, `env`, `env_as`, `env_decode`, `HostError`, ... |
| `std.process` | [process.txt](process.txt) | 28 | `ExitCode`, `Termination`, `ProcessOutput`, `ProcessError`, `Command`, `Process`, `run`, `ScriptedProcess` |
| `std.time` | [time.txt](time.txt) | 36 | `Duration`, `ms`, `s`, `minutes`, `h`, `Clock`, `now`, `monotonic`, ... |
| `std.random` | [random.txt](random.txt) | 14 | `Random`, `fill`, `SeededRandom`, `Rng`, `rng` |
| `std.error` | [error.txt](error.txt) | 6 | `Error`, `ErrorReport`, `report_of`, `message` |
| `std.format` | [format.txt](format.txt) | 24 | `Display`, `Debug`, `DebugWriter`, `DebugStruct`, `DebugTuple`, `DebugList`, `DebugMap`, `debug`, ... |
| `std.regex` | [regex.txt](regex.txt) | 22 | `Regex`, `Match`, `Captures`, `RegexError` |
| `std.encoding.json` | [encoding.json.txt](encoding.json.txt) | 35 | `Json`, `JsonObject`, `JsonObjectIter`, `Number`, `JsonError`, `parse`, `to_json`, `from_json`, ... |
| `std.encoding.base64` | [encoding.base64.txt](encoding.base64.txt) | 3 | `Base64` |
| `std.encoding.hex` | [encoding.hex.txt](encoding.hex.txt) | 2 | `hex_encode`, `hex_decode` |
| `std.validate` | [validate.txt](validate.txt) | 41 | `PathStep`, `Problem`, `Violation`, `ValidationError`, `ValidationContext`, `Validate`, `Rule`, `HasLength`, ... |
| `std.task` | [task.txt](task.txt) | 32 | `Layer`, `apply`, `race`, `BlockOn`, `block_on`, `CleanupEnd`, `Backoff`, `TimedOut`, ... |
| `std.sync` | [sync.txt](sync.txt) | 13 | `Arc`, `Weak` |
| `std.thread` | [thread.txt](thread.txt) | 6 | `Scope`, `Handle`, `scope`, `background` |
| `std.http` | [http.txt](http.txt) | 117 | `HttpStatus`, `HeaderName`, `HeaderError`, `Body`, `BodyError`, `FormError`, `UrlError`, `Headers`, ... |
| `std.net` | [net.txt](net.txt) | 39 | `NetError`, `Net`, `lookup`, `connect`, `listen`, `bind_udp`, `TcpStream`, `TcpListener`, ... |
| `std.crypto` | [crypto.txt](crypto.txt) | 2 | `Digest` |
| `std.crypto.sha256` | [crypto.sha256.txt](crypto.sha256.txt) | 6 | `sha256`, `Sha256` |
| `std.crypto.sha512` | [crypto.sha512.txt](crypto.sha512.txt) | 6 | `sha512`, `Sha512` |
| `std.crypto.aes` | [crypto.aes.txt](crypto.aes.txt) | 7 | `AesError`, `Aes` |
| `std.crypto.cipher` | [crypto.cipher.txt](crypto.cipher.txt) | 13 | `Block`, `CipherError`, `Gcm`, `Ctr` |
| `std.hash` | [hash.txt](hash.txt) | 7 | `Fnv1a`, `hash_of`, `DefaultHasher` |
| `std.meta` | [meta.txt](meta.txt) | 68 | `Marker`, `Send`, `Sync`, `Wire`, `facts_of`, `Target`, `Annotate`, `annotate`, ... |
| `std.contract` | [contract.txt](contract.txt) | 4 | `EnsureGuard`, `ensure`, `require` |
| `std.arena` | [arena.txt](arena.txt) | 4 | `Arena` |
| `std.testing` | [testing.txt](testing.txt) | 28 | `TestRunner`, `PropertyCase`, `PropertyRunner`, `Choices`, `Arbitrary`, `it_each`, `it_prop`, `it_prop_with`, ... |
