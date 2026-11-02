# Execution Domains Simplification

Status: simplification review, 2026-11-02; changes no decision, spec
text, or compiler code. It reviews
[EXECUTION_BOUNDARIES.md](EXECUTION_BOUNDARIES.md) at 22577cd58 together
with the findings of
[round 2](EXECUTION_BOUNDARIES_STRESS_TEST_2.md). The record at dfec0e865
takes K1, K2 and K3; K4 is not taken.

- [Summary](#summary)
- [Inventory](#inventory)
- [Cuts](#cuts)
- [Cuts Considered And Rejected](#cuts-considered-and-rejected)
- [Parse Log](#parse-log)

## Summary

| Rank | Cut | Absorbed by | Removes | Cost | Soundness |
| ---: | --- | --- | --- | --- | --- |
| 1 | K1: a handle is a served object | `Remote` (R1 to R3) and one std generic, `Served[T]` | the `Handle` marker, `Action`, the stream and process special cases | a call through a handle away from home is a round trip, as before | holds with condition: device code still needs `WrongHome` |
| 2 | K2: drop `Sync` | `Send`, with the existing loan exclusivity | one marker, one table column | none in hd, which has no interior mutability through `&` but boxes | holds |
| 3 | K3: drop `try_run!` | `?` converts through `From` | one method on every domain type | `run!(...)??` for code that returns a `Result` | holds |
| 4 | K4: `CrossError` by whether the call ran | the four cases, regrouped | nothing: a regrouping | one more level in a `match` | holds |

## Inventory

| Item | Kind | Tier | Used by |
| --- | --- | --- | --- |
| `Send` | marker, structural | language | shared-memory domains |
| `Sync` | marker, structural | language | `&T` being `Send` |
| `Wire` | marker, structural | language | separate-memory domains, `Remote` |
| `Handle` | marker | language | textures, caches, actions, streams |
| `Remote` | marker; R1 to R3 | language | services, devices |
| closure rules | a written capture is not `Send`; reached top-level bindings are immutable and `Wire` | language | every closure crossing |
| `CrossError` | enum: `Unreachable`, `Unknown`, `OtherBuild`, `WrongHome` | std | every contact-losing crossing |
| `serve!`, `remote::[T]` | generated intrinsics | std | services, devices |
| `run!`, `try_run!`, `spawn`, `after!` | domain methods | std | same-build domains |
| `Action[A, T, E]` | std handle | std | server actions |
| `workers.at(h)` | std method | std | code goes to the data |
| B1 to B5 | build rules | CLI and driver | every target |

## Cuts

### K1. A Handle Is A Served Object

**Before.** A handle is a value of a type with the `Handle` marker. Its
home holds a resource, and the record leaves open how code reaches it
(round 2, N5). Separately, `Action` is a served closure, and a process or
a stream would each need their own type (N7).

**After.** A handle is a `Served[T]`: a reference to an object that its
home serves, for a `Remote` trait `T`. `remote::[T](handle)` gives its
methods; each call goes to the home, and at the home it is a local call.
A texture is a `Served[Texture]`, an action a `Served[Call[A, T, E]]`, a
stream a `Served[Next[Event]]`, a process a `Served[Account]`.

```text
# hypothetical syntax; before and after for one texture
tex := gpu.upload!(photo)?                  # before: tex: Texture, a Handle type
tex := gpu.upload!(photo)?                  # after: tex: Served[Texture], home: this GPU
let mut texture = remote::[Texture](tex)
size := texture.size!()?                    # a crossing to the GPU, a local call there
```

**Absorbed by.** R1 to R3: the object is a `Remote` implementation that
its home serves, and the caller uses a stub, as for a service.

**Cost.** None beyond today's record: every operation on a handle away
from home was already a crossing. A served object needs a lifetime rule,
which round 2's N8 asks for anyway: a lease its home drops after a
timeout.

**Soundness.** The `Handle` rule protected the resource from use away
from home. A `Served[T]` exposes only `T`'s methods, and each runs at the
home, so the resource is never touched elsewhere. The one place a local
resource is required is device code, which cannot make crossings: a
kernel given a `Served[Texture]` of another GPU is still refused with
`WrongHome`. **Holds with that condition.**

| Item | Change |
| --- | --- |
| `Handle` marker | deleted; merged into `Served[T]` |
| `Action[A, T, E]` | merged: `Served[Call[A, T, E]]` |
| `WrongHome` | unchanged; now only device code and forged tokens hit it |
| the handle bullets in section 3 | reworded |

**Other languages.** Cap'n Proto capabilities, Erlang pids and Orleans
grain references are location-transparent references to served objects.

### K2. Drop `Sync`

**Before.** `&T` is `Send` exactly when `T < Sync`, and the bound table
has a `Sync` column.

**After.** `&T` is `Send` when `T < Send`.

```text
# hypothetical syntax; lines: &List[string]
s.spawn(fn() -> i32: count_quarter(lines, 0))   # before: &List[string] is Send because List[string] < Sync
s.spawn(fn() -> i32: count_quarter(lines, 0))   # after: because List[string] < Send
```

**Absorbed by.** `Send` and loan exclusivity: while `&p` is lent, the
source does not write `p`
([`ref.excl.shared`](../../spec/lang/13-values-and-references.md#r-ref.excl.shared)).

**Cost.** None in hd. Rust needs `Sync` for `Cell` and `RefCell`, which
mutate through `&`. hd's only mutation through `&` is through a box field
([`ref.read.shallow`](../../spec/lang/13-values-and-references.md#r-ref.read.shallow)), and a box is not `Send`.

**Soundness.** `Sync` protected a shared read from a concurrent write.
Exclusivity rules out a write by the owner during the loan, and a box,
the one path to a write through `&`, is not `Send`. **Holds.**

| Item | Change |
| --- | --- |
| `Sync` marker | deleted |
| the `Sync` column | deleted |
| "`&T` is `Send` when `T < Sync`" | reworded: when `T < Send` |

### K3. Drop `try_run!`

**Before.** `try_run!` takes code returning `Result[T, E]` with
`E < From[CrossError]` and returns a flat `Result[T, E]`.

**After.** `run!` alone; `?` converts a `CrossError` into the caller's
error through `From`
([`expr.try.convert.from`](../../spec/lang/05-expressions.md#r-expr.try.convert.from)).

```text
# hypothetical syntax; the caller returns Result[Image, ImageError], and ImageError < From[CrossError]
small := workers.try_run!(fn() -> Result[Image, ImageError]: shrink(photo, 256))?   # before
small := workers.run!(fn() -> Result[Image, ImageError]: shrink(photo, 256))??      # after
```

**Absorbed by.** `?` and `From`.

**Cost.** Two `?` where one did. **Soundness.** Nothing it protected.
**Holds.**

| Item | Change |
| --- | --- |
| `try_run!` on every domain type | deleted |

### K4. `CrossError` By Whether The Call Ran

**Before.** Four flat cases: `Unreachable`, `Unknown`, `OtherBuild`,
`WrongHome`.

**After.** Two outcomes a caller acts on: `NotRun(reason)`, where
`reason` is `Unreachable`, `OtherBuild` or `WrongHome`, and `Unknown`.
A caller retries `NotRun` safely and checks the state after `Unknown`.

```text
# hypothetical syntax
match orders.place_order!(cart):
    .Ok(receipt) => show(receipt)
    .Err(.Cross(.NotRun(.OtherBuild))) => reload!()
    .Err(.Cross(.NotRun(_))) => retry!()
    .Err(.Cross(.Unknown)) => check_order_status!(cart.id)
    .Err(error) => show_error("$error")
```

**Absorbed by.** The same four facts, regrouped.

**Cost.** One more pattern level. **Soundness.** Nothing removed.
**Holds.** It changes the spelling of `OtherBuild`, which was chosen as a
flat case, so it is an owner question rather than an edit.

## Cuts Considered And Rejected

| Cut | Why not |
| --- | --- |
| merge `Send` into `Wire` | a scoped thread borrows (`&T` is not `Wire`), and a background thread shares a buffer instead of copying it |
| drop the row parameter on domain types | it is what keeps `$ Ui` off a background thread |
| merge `Background` into `Scope` | std, not language; and their lifetimes differ (`'static` against `'s`) |
| fold `WrongHome` into a panic | a server refusing a forged handle must answer the caller, not crash |
| replace `serve!` with `$.with` | `serve!` is the explicit list of what another build may call |

## Parse Log

All 4 `text` blocks were parsed on 2026-11-02 with
`scripts/hd-conformance.sh parse`, which checks syntax only.

| Block | Cut | Result |
| --- | --- | --- |
| 1 | K1 | parses; `# hypothetical syntax` |
| 2 | K2 | parses; `# hypothetical syntax` |
| 3 | K3 | parses; `# hypothetical syntax` |
| 4 | K4 | parses; `# hypothetical syntax` |
