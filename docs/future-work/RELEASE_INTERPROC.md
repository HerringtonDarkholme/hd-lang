# Release-Only Interprocedural Optimization

Status: approved by the owner, 2026-10-25, release pipeline only (#681);
domain: compiler internals; being built slice by slice per Phasing.

Under review: the owner's direction of 2026-10-24 on
[#702](https://github.com/HerringtonDarkholme/hd-lang/issues/702), "plan
interproc optimization, but it will be only enabled in release pipeline";
the ownership passes and their `OwnCx` interface
([docs/compiler/tir-v2.md](../compiler/tir-v2.md#the-ownership-passes),
`crates/hd_driver/src/own.rs`); the pipeline table
([crates/hd_opt/DESIGN.md](../../crates/hd_opt/DESIGN.md) §3); the cache
keys ([crates/hd_cache/DESIGN.md](../../crates/hd_cache/DESIGN.md) §5.3);
the backend's calling convention
([crates/hd_wasm_linear/DESIGN.md](../../crates/hd_wasm_linear/DESIGN.md)
§23.25 to §23.27); and the findings on
[#681](https://github.com/HerringtonDarkholme/hd-lang/issues/681),
[#165](https://github.com/HerringtonDarkholme/hd-lang/issues/165) and
[#667](https://github.com/HerringtonDarkholme/hd-lang/issues/667).

## Contents

1. [Problem, users, use cases and today](#problem-users-use-cases-and-today)
2. [State of the art](#state-of-the-art)
3. [The rule](#the-rule)
4. [Semantics](#semantics)
5. [Options](#options)
6. [The recommended design](#the-recommended-design)
7. [Extension points](#extension-points)
8. [Cost](#cost)
9. [Stress test](#stress-test)
10. [Phasing](#phasing)
11. [Recommendation and approvals](#recommendation-and-approvals)
12. [Sources and parse log](#sources-and-parse-log)

## Problem, users, use cases and today

Every count decision in hd is local today. The ownership passes read a
callee's signature, never its body (tir-v2.md, "What the passes read").
So a parameter's passing mode is fixed by its declared type. A value
parameter of a counted type is **owned**: its caller gives it a count at
every call. Three costs follow, and no local rule can remove them,
because only the callee's body knows the fact that would.

| Cost | Where it shows | Why it is not local |
| --- | --- | --- |
| A read-only struct value passed by value pays a retain in the caller and a release in the callee | json: `Member`, `Field`, `Key`, `Members`, a `Parser`'s text, per the lane's 2026-10-25 json profile (about 8% of json's time in handle copies around calls) | the callee never keeps the value, but its signature says "owned" |
| A dropped chaining result pays a retain in the callee and a release in the caller | `StringBuilder.push` in string_build and map_count (#681) | the callee retains because it returns `self`; only the caller knows the result is dropped |
| A call through a reference stops a borrow unless the callee is a known pure intrinsic | `OwnCx::may_write` is false only for `sqrt` and seven string byte reads (`PURE_INTRINSICS`) | whether a callee writes memory is a fact of its body |

The users are hd programs built with `--release`, and the compiler team,
who read count baselines and bench tables. The language user writes
nothing new and sees no new diagnostic.

**Use case 1: json's read-only values.** std.serde passes `Member` (a
struct with a name, facts and a doc) by value to `member_name` and to
`Serializer.member`. Each call retains the strings and lists inside the
value and releases them at the callee's exit. A release build should
pass them with no count, since no callee keeps them. The engineer runs
`node bench/study/run.mjs --quick --only json` before and after, and reads
the `counts` baselines.

**Use case 2: a builder chain (#681).** `out.push("item-$i;")` in a loop:
`push` takes `*mut self` and returns `self`. Today the case
`counts/discarded-returned-reference` shows one `retain` in `Tally.add`
and one `release` in `Tally.fill`, per call. A release build should show
none.

**Use case 3: a reference read across a pure helper.** In a `Parser`
method, `self.text` read through `&self` stays borrowed across a call
only if the callee is known not to write. Commit 11972f1e3 listed the
string byte intrinsics in `PURE_INTRINSICS` by hand for this: json at
N=60,000 went from about 400 to 390 ms. A summary gives the same fact for
every helper written in hd.

**Use case 4: a counted local that never escapes.** Take `let mut t =
Tally { count: 0 }` followed by `t.add()`, where `add` takes `*mut self`.
The call makes a counted reference to `t`, so `t` lives in a counted heap
cell. Local references step 1 (#834, b013017fe) keeps a local in the frame
when every address of it goes to a `&T` or `&mut T` parameter. A release
build could also keep it there when the callee never keeps its `*mut T`.

**Use case 5: dev stays fast and incremental.** `hd FILE`, `hd check`,
`hd test` and the playground run the dev pipeline. They must emit exactly
what they emit today, at today's compile time.

**Today, probed.** At 11972f1e3, `counts/discarded-returned-reference`
(`@build: v8 release`) holds `retain &mut app.main.Tally` in `Tally.add`
and `release &mut app.main.Tally` in `Tally.fill`. Two probes on the
worktree's release CLI, 20 million calls each, five interleaved
fresh-process runs on Node 24.19 (medians, process wall time):

| Probe | Today | Without the counts | Difference per call |
| --- | ---: | ---: | ---: |
| `t.add()` returning `self`, result dropped, against `t.bump()` returning `void` | 318 ms | 251 ms | 3.4 ns |
| `weight(f)` with `f: Field` (a struct with one string) by value, against `weight(&f)` with a `&Field` parameter | 384 ms | 258 ms | 6.3 ns |

The probe sources are in the [parse log](#sources-and-parse-log).

## State of the art

Every mature reference-counted or escape-analyzing compiler computes
these facts from bodies, bottom-up over the call graph, in its optimizing
mode. Any design must answer nine items: the facts, their order of
computation, recursion, indirect calls, one convention per callee,
caching, explaining decisions, testing, and code size.

**Lean 4: inferred borrowed parameters.** Ullrich and de Moura's
"Counting Immutable Beans" (IFL 2019) infers which parameters of each
function are *borrowed*. The caller passes a borrowed parameter with no
count, and the callee may not consume it. Lean's `Borrow.lean` starts a
mutually recursive block with every parameter borrowed, then marks one
owned when the body consumes it, until nothing changes.

Consuming means storing, returning, passing to an owned position, or
resetting for reuse. The user writes nothing; `@&` forces a borrow. It
covers every first-order call, and calls of closures pass owned. This
design shares its limits: closure parameters stay owned, and a body
change can change its callers' code.

**Go: escape analysis bottom-up over SCCs.** Go's escape analysis
visits functions with `VisitFuncsBottomUp`, one strongly connected
component at a time. It records per parameter whether it leaks to the
heap or to a result ("leaking param content", "leaks to ~r0"). The tags
go into export data, so a caller in another package reads them.

Calls through interfaces and function values count as escaping. `go
build -gcflags=-m` prints each decision. Go runs the analysis in every
build, since it has no separate dev pipeline.

**Swift: guaranteed parameters and signature optimization.** Swift's
default convention for ordinary parameters is *guaranteed*. The caller
keeps the value alive, and the callee copies it only to keep it
(SE-0377 names the two conventions `borrowing` and `consuming`).
Initializers and setters default to *owned*, because they keep their
arguments. In `-O`, `FunctionSignatureOpts` changes a function's
convention from its body (owned to guaranteed, dead arguments,
exploded structs) and leaves a thunk with the old convention for
indirect callers and other modules. The cost is the thunks' code size.

**LLVM: inferred function attributes.** The `FunctionAttrs` pass runs
over the call graph's SCCs bottom-up, in the CGSCC pass manager. It
infers `nocapture` (now `captures(none)`), `readonly`, `memory(...)`
effects, and `returned` for a parameter the function always returns. The
attributes change only optimization, never what compiles; they are
inferred only for functions whose definition is exact (not replaceable
at link time). `-O0` infers none. Tests check the inferred attributes
with FileCheck.

**Koka: Perceus.** Reinking, Xie, de Moura and Leijen (PLDI 2021) insert
counts precisely in a local pass. Borrowed parameters carry a `^`
written in the source; Koka does not infer them. In hd a written mark
would be part of the signature, which is option B below.

| Item | This design |
| --- | --- |
| Facts | kept parameters (Lean's borrowed, Go's leaks, LLVM's `captures`), a returned parameter (LLVM's `returned`), writes (LLVM's `memory`) |
| Order | bottom-up over SCCs, as Go and LLVM |
| Recursion | conservative in the first version: a parameter passed to a call inside its own SCC is kept. Lean's fixed point is the next step ([Options](#options)) |
| Indirect calls | function values and `dyn` keep the declared convention at the call; the adapter or vtable slot converts, as Swift's thunks do, but with no new function |
| One convention per callee | every caller of an instance reads one table; generated callers too |
| Separate compilation and caching | none needed today: the release cache is per program (`prog_key`). For per-instance code entries, a caller's key hashes the summaries it read, as Go's export data carries tags |
| Explaining decisions | the `own` fixture view prints each body's summary; a `--stats` line later, like Go's `-m` |
| Testing | `counts` baselines in release, checked-mode conformance on the optimized pipeline, dev/release output equality |
| Code size | no thunks and no copies: the existing adapters and vtable slots already sit between indirect callers and callees |

Excluded: Swift's dead-argument removal and struct explosion, and LLVM's
interprocedural constant propagation. They change the Wasm signature of
a function, which every relocation and the closure ABI read, and no
measured hd cost points at them.

## The rule

**What stays declaration-only.** Type checking, inference, every
diagnostic and fix-it, the API a package exports, and whether a program
compiles. The checker never reads a summary, and a summary is never in a
`check` entry or a folder interface. This keeps the language's rule:
nothing a caller relies on is inferred from a body. Changing a body can
change its callers' release code, never their errors.

**What release may infer from a body.** Three codegen facts per
function, computed from its TIR after inlining and its callees'
summaries:

1. **Kept parameters.** A parameter is *kept* when the body may hold its
   value past the call. A parameter that is not kept is *lent*: its
   caller passes it with no count, as it passes a `&T` today.
2. **A returned parameter.** The body's result is always one parameter's
   value, unchanged (`self` in a builder). Its caller may then treat the
   result as its own argument, borrowed, with no count.
3. **Writes.** Whether the body may write memory its caller can reach.
   A call that writes nothing does not end a borrow through a reference.

These facts change count operations and where a local lives (frame slot
or cell). They never change what a program prints, its panics, or its
effect order, which tiering.md §4.1 already requires of every release
pass.

**Where it runs.** Only in the optimized pipeline: `hd build --release`,
`hd run --release`, `hd test --release`
([`cli.profile.release`](../../spec/cli/command-line.md#r-cli.profile.release),
[`cli.profile.test.release`](../../spec/cli/command-line.md#r-cli.profile.test.release)),
and conformance with `HD_CONFORMANCE_PIPELINE=optimized`. It keys on the
pipeline, not the profile, so a checked build on the optimized pipeline
audits it. Dev, check, test, the REPL and the web compiler run no part
of it.

## Semantics

**Values and copies.** Passing a value copies it
([`types.value.copy`](../../spec/lang/04-type-system.md#r-types.value.copy)).
A lent argument is still a copy by meaning. The callee sees the value as
it was at the call, and no change elsewhere may reach it. The danger is
a callee that changes the argument's *source place* through another
path: the uniqueness check sees one count and writes in place.

```text
fn grow_and_measure(a: List[i64], b: &mut List[i64]) -> usize:
    b.push(1)
    a.len()

fn demo() -> usize:
    let mut xs: List[i64] = [1, 2]
    grow_and_measure(xs, &mut xs)
```

`grow_and_measure` never keeps `a`, so a naive design lends it. Then
`push` finds the buffer unique, and the call returns 3 instead of 2. The
design is safe because a summary changes only the *pass mode* the caller
reads. Whether the argument takes a count stays the borrow pass's call.

The borrow pass applies its rules 1 to 4 (tir-v2.md, "Borrow") over a
span that includes the call. Here rule 2 fails: a mutable `Ref` of an
overlapping place stands in the span. So the caller copies `xs` (one
retain) and drops the copy after the call, as for a `&T` argument today.

**References.** A `&T` or `&mut T` parameter never escapes by
declaration
([Local References](../../spec/lang/13-values-and-references.md#local-references),
`ref.local.escape.*`), and is lent today. A `*T` or `*mut T` parameter is
lent too (the backend's `layout::Pass`), but the callee may keep it. For
a counted reference, "not kept" means the address does not outlive the
call, which is what use case 4 needs. `is` on references
([Identity](../../spec/lang/13-values-and-references.md#identity)) is
unchanged: a returned parameter is the same address.

**Generics and trait values.** The passes run once per generic body
(tir-v2.md, "Where and when the passes run"), so a summary is per
generic body and holds for every instance. A parameter whose declared
type names a type parameter, an associated type or an opaque type is
always kept. An instance may make it a reference, a scalar or a boxed
open value (§23.27), and the erased ABI owns open values. A call whose
callee is chosen per instance (a bound) or at run time (a trait value)
has no single body; [the design](#the-recommended-design) says what each
reads.

**Closures and captures.** A captured parameter is kept, whether the
capture is a copy or a shared cell: the closure may outlive the call.

```text
fn later(name: string) -> fn() -> usize:
    fn() -> usize: name.len()
```

A call of a function value (`CallValue`) reads the function type's
declared convention, since its callee is unknown when the caller's
passes run.

**Suspension.** A suspending function's frame holds a count of each
parameter, and its callers already lend every argument (`declared_pass`
in `own.rs`, §23.25). So a suspending function gets no summary, and a
bang call reads the declared convention. A call in a suspending caller
gets the summary as usual: the borrow pass's rule 4 already refuses a
borrow across a suspension point.

**Control flow.** `?`, `return`, `break` and a `defer` suite are
ordinary TIR in the body the summary reads. A `Return` of anything but
the parameter clears the returned-parameter fact. A panic is a trap
that runs no suite, so no count after it matters.

**Element access (decision E).** An element read is a place. A lent
argument read from an element is borrowed only under the borrow pass's
rules, and a `Ref` of an element keeps its pin. Summaries change neither.

**Lifetimes of objects.** A lent argument is released right after the
call instead of at the callee's exit (an owned parameter that its body
never keeps is released at the exit today, `received`'s `Owned` path).
Nothing runs between the two points. Release timing is unobservable
anyway: hd has no finalizers or weak references
([`data.repr.no-manual`](../../spec/lang/08-data-and-enums.md#r-data.repr.no-manual)).
The approved weak references (#720) take an object's count, so a frame
slot never holds an object a weak reference points at.

## Options

Each option is judged on the five use cases.

**A. Declaration only, plus more local rules (today).** No body is read.
Use cases 1, 2 and 4 stay as they are: conventions come from types, and
a caller cannot see its callee's retain. Use case 3 grows one
`PURE_INTRINSICS` entry at a time and never reaches helpers written in hd.

It costs nothing. It is what #681's "Blocked" comment of 2026-10-25
records.

**B. Lend every value parameter (Swift's default).** A local convention
change, in dev and release alike: every value parameter is lent, and a
callee that keeps one copies it. Use case 1 is solved with no summary.
But a callee that keeps its argument pays more. `StringBuilder.push`
stores `text`, so it would retain it, and its caller would release it.

That is two counts where the temporary moves in at none today. Swift
needed `__owned` and then `consuming` (SE-0377) for such callees. In hd
that is a new parameter modifier, which needs approval and adds a
mechanism. It does nothing for use cases 2 to 4.

**C. Release summaries (recommended).** Per-function summaries, computed
bottom-up over the call graph's SCCs in the optimized pipeline only. A
parameter is lent when its declaration or its body says so. Use cases 1
to 4 are solved where the callee is known, and use case 5 is untouched.

A callee that keeps its argument keeps today's owned convention, so no
call pays more.

**D. Inline more.** Widen the inliner: user code, a higher limit, or a
declaration-level `@inline`, the owner's preferred next step (#667,
2026-10-23). An inlined call has no boundary, so the local passes see
everything. But it reaches only small callees, and json's serde helpers
are not small.

Raising the limit to 40 made `for` loops slower and larger (#667,
2026-10-24). D complements C and does not replace it.

**E. Write local references in std's signatures (the no-feature
option).** Change std's hot read-only parameters to `&T`, as in
`member_name(m: &Member, ...)`. A local reference is never counted in a
call, by declaration, in dev too. It costs no compiler code.

But it changes std's API by hand wherever someone profiles. A trait
method such as `Serializer.member` changes for every implementation, and
generic code cannot take `&T` for a `T` value. User code keeps paying.

It does nothing for use case 2: a `&mut Self` result is a later
local-references step, not approved. It is worth doing for a few std
signatures where the API reads better, and C covers the rest.

**The smallest design that could work** is C with only the kept bit, on
direct calls (slices 2 to 4 below, about 360 Rust lines). It covers use
case 1 for plain functions and impl-resolved methods.

| | A | B | C | D | E |
| --- | --- | --- | --- | --- | --- |
| 1, read-only values | no | yes | yes, known callees | small callees | where edited |
| 2, builder result | no | no | yes | if inlined | no |
| 3, pure helpers | by hand | no | yes | if inlined | no |
| 4, counted locals | no | no | yes | if inlined | no |
| 5, dev unchanged | yes | no | yes | depends | yes |
| new language rule | no | a modifier | no | `@inline` | no |

## The recommended design

### Pipeline position

```
check entry (TIR) ──► Collect (serial per program)
                        │ body_of(def): decode ─► inline ─► [release: callees first] ─► own passes
                        │                                        ▲ reads Summaries   │ writes Summary
                        ▼                                        └────────────────────┘
                      Emit (reads Summaries for conventions) ─► Link
```

Summaries live where the ownership passes do: in the driver's `body_of`
(`crates/hd_driver/src/lib.rs`) and `Env` (`own.rs`), which already
answer the passes' questions. Collection is serial per program
(crates/hd_mono/DESIGN.md §11.2), so the bottom-up order needs no
locking across threads.

The summary is a by-product of the borrow pass, not a new analysis.
`lent_params` in `hd_tir/src/own/borrow.rs` already notes, per
parameter, a store, a capture, a counted address, an in-place change or
a read that keeps the value (`t.stores`, `t.captured`, `t.reached`,
`t.changed`, `t.kept_read`).

An owned parameter with none of these is "unowned" today, and the
backend releases it at the exit. That set is exactly the lent set. The
passes read callee conventions through `OwnCx::pass`, so passing a
parameter to a lending callee is no kept read.

### Data structures

```rust
/// What the optimized pipeline reads of one body (hd_tir::own).
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Summary {
    /// Bit k: the body may keep parameter k (receiver first) past the
    /// call. Parameters past 63, open types, and every parameter of a
    /// suspending body are always kept.
    pub kept: u64,
    /// The parameter every result of the body is, unchanged: a whole
    /// read of a parameter the body never stores.
    pub returns: Option<u8>,
    /// Whether the body may write memory its caller can reach: a store
    /// through a dereference or to a top-level binding, an in-place
    /// change of a place not rooted at its own locals, a host call, or a
    /// call that may write.
    pub writes: bool,
}

/// The run's summaries (hd_driver), release only. 16 bytes per body.
struct Summaries {
    by_item: Mutex<HashMap<DefId, Summary>>,
    /// The meet over every impl of a trait method in the program (slice 7).
    by_trait_method: Mutex<HashMap<DefId, Summary>>,
}
```

A `HashMap` keyed by `DefId` is only looked up, never iterated, so
output stays deterministic. The meet of two summaries is `kept | kept`,
`returns` equal or `None`, and `writes || writes`.

### What each caller reads

| Call | Convention read |
| --- | --- |
| `Callee::Item` (a function, an inherent method) | the callee's summary |
| `Callee::TraitMethod` chosen as an impl (`ChoiceKind::Impl`), resolved as `impl_target` does for the inliner | the impl method's summary |
| a trait call through a bound | declared (slices 2 to 6), then the trait method's meet (slice 7) |
| a trait call through a trait value (`dyn`), a `CallValue`, a bang call, an intrinsic | declared |
| a function reference's adapter (§13.11) | the item's summary: the adapter is a body of one direct `Call` that goes through the passes, so it receives owned by the function type and lends to the item |
| backend glue that calls an instance: vtable slots (`Helper::DynSlot`), erased thunks, map `KeyOps`, host codecs (`rt::own_arg`), and `callee_params` | the instance's convention, from the same table |

A callee receives parameter k lent exactly when its own summary clears
bit k (or slice 7's meet clears it, for an impl method). So every caller
and the callee agree, through one function:

```rust
fn convention(&self, item: DefId, k: usize, n: usize) -> Pass {
    let declared = self.declared_pass(item, k, n);
    if declared == Pass::Lent || !self.release_summaries() {
        return declared;
    }
    match self.summary_of(item) {
        Some(s) if k < 64 && s.kept & (1 << k) == 0 => Pass::Lent,
        _ => Pass::Owned,
    }
}
```

`OwnCx::pass`, `OwnCx::received` and the backend's `own_arg` callers
call it. `OwnCx::may_write` returns `summary.writes` for a summarized
callee, and today's answer otherwise.

### Data flow

1. Collection asks `body_of(def)` for a body it reached.
2. In release, the driver decodes the body and runs the inliner, as
   today. It then scans the inlined body for summarized callees (the
   first two rows of the table above).
3. It finds the SCCs of those callees with an iterative Tarjan walk, on
   demand, decoding and inlining each callee the same way.
4. It runs the ownership passes per SCC, callees' SCCs first. Inside an
   SCC, a call of another member reads the declared convention. The
   passes then return the body and its summary, and the driver records
   both.
5. Emission reads the conventions from `Summaries` for each instance's
   own parameters and for the glue it generates.

Every body the walk visits is one collection reaches anyway: a direct
call's callee is always an instance of a reached body. So the walk adds
no decoding, only an earlier order.

### Pseudo-code

```rust
/// Release: the owned body of `root`, with every callee's summary known
/// first. An iterative Tarjan walk, so a deep call chain cannot overflow
/// the Rust stack.
fn owned_bottom_up(&self, p: &ProgramTables, root: DefId) {
    if self.owned_ready(root) { return; }
    let mut index = HashMap::new();       // def -> (index, low link)
    let mut on_stack = Vec::new();        // the Tarjan stack
    let mut work = vec![(root, 0usize)];  // (def, next callee to visit)
    let mut next = 0u32;
    while let Some(&mut (def, ref mut at)) = work.last_mut() {
        if *at == 0 {
            index.insert(def, (next, next));
            next += 1;
            on_stack.push(def);
        }
        let callees = self.inlined_callees(p, def); // memoized scan
        if let Some(&c) = callees.get(*at) {
            *at += 1;
            if self.owned_ready(c) { continue; }
            match index.get(&c) {
                None => work.push((c, 0)),
                Some(&(ci, _)) if on_stack.contains(&c) => lower(&mut index, def, ci),
                Some(_) => {}
            }
            continue;
        }
        work.pop();
        if let Some(&(parent, _)) = work.last() {
            let low = index[&def].1;
            lower(&mut index, parent, low);
        }
        let (i, low) = index[&def];
        if i == low {
            // def roots an SCC: pop it, then own each member in order.
            let scc = pop_until(&mut on_stack, def);
            for &m in &scc {
                self.in_scc.insert(m, scc_id(&scc)); // calls inside read declared
            }
            for &m in sorted_by_path(&scc) {
                let (body, summary) = self.own_body_release(p, m);
                self.record(m, body, summary);
            }
        }
    }
}
```

`sorted_by_path` owns an SCC's members in stable path order. Under the
conservative rule no member reads another's summary, so the order cannot
change a result. It only fixes the order of the `own` view's output.

### Returned parameters (#681)

The callee side: when `summary.returns == Some(k)` and parameter k is
lent, a `Return` of `$k` is a lending use. So the callee takes no count
of its result. A scan of the inlined body decides `returns` before the
passes. Every `Return` operand and the tail must be a whole `Read [$k]`,
with no `Store` to `$k`.

The caller side: the call's result is a borrowed value whose root is
argument k's value. The borrow pass already ties a borrowed value to its
root ("a use of a borrow uses its root", tir-v2.md, stage 2 as built).
A dropped result costs nothing, and so does one with only lending uses
while rules 2 to 4 hold over its span.

A stored or returned result gets a `copy` right after the call. That
retain is today's callee retain, moved to the caller; the release when
it is dropped stays. A temporary argument keeps its drop until the
result's last use, by the same root rule.

Generated callers convert. An adapter's passes see a borrowed result and
copy it for their owned return, and a vtable slot retains a returned
parameter before returning.

### Writes

One walk of the owned body decides `writes` after the passes. It is set
by any of these:

- a `Store`, an `IsUnique` or an in-place collection change of a place
  rooted at a dereference or a top-level binding;
- a host call, a `CallValue` or a trait-value call;
- a call whose callee may write, or a call of another member of the
  same SCC.

A store rooted at a local does not count: the caller cannot reach the
callee's locals.

### Escape facts and frame slots (slice 8)

Local references step 1 keeps a local in an uncounted frame slot
(§23.29) under four conditions (b013017fe). Every address of it goes to a
`&T` or `&mut T` parameter; none is a bang call's argument; no closure
captures it; and no `*T` is made of it.

In release, an address passed to an unkept `*T` or `*mut T` parameter
counts as a local-reference argument. When the call `returns` that
parameter, the result is another address of the local, and must stay in
the call's statement. The pipeline table declares an `escape` pass with
no body (tiering.md §3.1); this gives it one.

Keeping a cursor in Wasm locals instead of a frame slot needs scalar
replacement after inlining. #667's 2026-10-24 finding priced that as a
general optimizer project. Summaries do not give it, and this design
leaves it out.

### Caching and incremental behaviour

**Today.** The only back-half cache entry is the program's `link` entry.
Its key, `prog_key`, holds the pipeline key and each module's content
hash, which covers every body's TIR hash (crates/hd_cache/DESIGN.md
§5.3). So a body edit in a release build already re-collects and
re-emits the program.

Summaries therefore need no new key. The optimized pipeline's key
changes once, when the pass is added, and every body a summary reads is
in `prog_key` already. std's bodies are in the toolchain key.

**When per-instance code entries are built.** `code_key` holds the
instance's TIR hash and its `callee_reps`. The hash `own_body` returns
already folds in each inlined callee's TIR hash (`inline_calls`). In
release it also folds in, for each summary the passes read, the callee's
path hash and the summary's 16 bytes. Then:

- a callee edit that keeps its summary re-emits the callee only (early
  cutoff, as Salsa's);
- a callee edit that changes its summary re-emits the callers that read
  it, and their callers only if their own summaries change;
- a slice 7 meet is hashed the same way, so adding an impl of a trait
  re-emits the bound callers whose meet it changes.

**Dev.** No summary is computed or read, and no dev key changes: dev's
pipeline key and code are byte-identical, which
`HD_CONFORMANCE_WASM_DIGEST=check` proves.

**Determinism.** A summary is a function of the body and its callees'
summaries. SCCs are a property of the call graph, not of the order in
which collection demands bodies. The conservative rule reads no summary
inside an SCC, so the result is the same in any demand order.

### Invariants and the verifier

`hd_tir::ir::verify_owned` runs in checked and debug builds. It already
checks that unowned parameters hold no count. Two clauses are added: a
lent parameter is never consumed (its uses all lend), and a borrowed
call result is never used after its root's drop. A third check, in
debug builds only, recomputes each summary from the final body and
compares it with the recorded one.

### Switch and rollback

A declared pass `summaries` joins the `OPTIMIZED` rows of
`hd_opt::passes::pipeline`, like `escape` today, and the driver reads
whether its pipeline holds it. Its name and version are in the pipeline
key. Removing it from the table restores today's release output byte
for byte; the digest check proves that. Each slice is a revert.

### Testing

- **Counts baselines** (`crates/hd_driver/tests/cases/counts/`, `@build:
  v8 release`): one case per stress row below, with a dev twin where the
  count must not change.
- **The `own` view** prints a body's summary line in release fixtures,
  so each decision is a reviewed diff.
- **Checked conformance on the optimized pipeline**
  (`HD_CONFORMANCE_CHECKED=1 HD_CONFORMANCE_PIPELINE=optimized`) audits
  every count of every runtime case.
- **Dev/release equality.** Every runtime fixture's output must match
  across the pipelines (tiering.md §4.3).
- **Determinism.** A unit test builds one program with its roots in two
  orders and compares the summaries.
- **Bench.** `node bench/study/run.mjs --quick` for json, string_build,
  map_count, dyn_iter and closures, before and after each slice.

## Extension points

There are none for users, and none are proposed. A user-visible borrow
mark would be part of a signature, which is option B's modifier or
option E's local references. A summary that users could write or
override would let a wrong annotation break memory safety. The one
planned addition is read-only: a line per summarized function in `hd
build --stats` (planned in tiering.md §6.5), which shows kept
parameters, as Go's `-gcflags=-m` does.

## Cost

### Measured today

| Measure | Value | Method |
| --- | ---: | --- |
| json, string_build, map_count | 100, 197, 180 ms | `bench/study/run.mjs --quick`, the worktree's release CLI, Node 24.19 |
| a returned-receiver count pair | 3.4 ns per call | the `chain`/`plain` probes above |
| an owned struct-with-string parameter | 6.3 ns per call | the `owned`/`lent` probes above |
| json's reached bodies, instructions after the passes | 279, 10,663 | tir-v2.md, "Compile time and code size" |
| the passes' rate; emission's rate | 0.49 and 2.1 µs per instruction | the same section |
| web compiler gzip | about 1,904,048 B | tir-v2.md, stage 3 after List commit 2 |

### Code, bottom-up

Rust code lines, counted per piece against the files they extend. No
existing code is deleted: `PURE_INTRINSICS` stays for intrinsics, which
have no body.

| Component | Where | Adds | Tests |
| --- | --- | ---: | ---: |
| `Summary`, its meet, `OwnCx::summary` with a default of none | `hd_tir/src/own.rs` | 35 | 20 |
| summary from the borrow tables (kept bits) and the `returns` scan | `own/borrow.rs`, `own.rs` | 55 | 40 |
| `writes` walk over the owned body | `own.rs` | 45 | 30 |
| callee scan, iterative Tarjan, SCC marks, `Summaries` | `hd_driver/src/own.rs`, `lib.rs` | 140 | 50 |
| `convention`, read by `pass`, `received`, `may_write` | `hd_driver/src/own.rs` | 30 | — |
| conventions to the backend: `own_arg` callers, `callee_params`, the exit release | `hd_wasm_linear` `emit.rs`, `rt.rs`, `emit/count.rs` | 70 | — |
| returned parameter: callee lending return, caller borrowed result, slot retain | `own/insert.rs`, `own/borrow.rs`, `emit/dyn_value.rs` | 120 | 40 |
| trait method meets (slice 7) | `hd_driver/src/own.rs` | 90 | 30 |
| escape: `*T` addresses to unkept parameters keep the frame slot (slice 8) | `hd_wasm_linear/src/emit.rs` root choice | 60 | 20 |
| verifier clauses and the debug recompute | `hd_tir/src/ir.rs` | 50 | — |
| pass declaration and the switch | `hd_opt/src/passes.rs`, `hd_driver` | 20 | 10 |
| `own` view summary line | `hd_driver/src/lib.rs` | 15 | — |
| **Total** | | **730** | **240** |

Fixtures: about 18 `counts` cases of 10 to 15 hd lines each, about 230
lines with their baselines. The optional by-address slice 9 would add
about 120 Rust lines; it is not in the total.

### Compile time

Release only; dev adds nothing.

- **Callee scan:** one walk of each reached body, cheaper than one pass
  (under 0.05 µs per instruction): about 0.5 ms on json's 10,663
  instructions.
- **Tarjan walk:** linear in reached bodies and call edges: 279 nodes
  for json, under 0.1 ms.
- **Summary extraction:** the kept bits are the borrow pass's tables,
  read once; `writes` is one more walk, about 0.5 ms on json.
- **Trait meets (slice 7):** the impl methods of a trait that no
  instance reaches are owned for their summaries. std has about ten
  `Serializer` and `Reader` impls; at about 50 instructions per method
  and 0.49 µs per instruction that is about 0.25 ms per trait method,
  a few ms in all for json.
- **Emission:** fewer count instructions, so slightly less.

So json's release build should grow by about 1 ms before slice 7 and a
few ms after it. Each slice reports the json and calc release build
times, as stage 2's commits did.

### Run time, per benchmark

Each estimate is per call cost times calls, from the probes, or a
share of a profile, and each slice re-measures it.

| Program | Mechanism | Calls | Estimate |
| --- | --- | ---: | --- |
| string_build | returned receiver (`push`) | 1.5 M | 1.5 M × 3.4 ns ≈ 5 ms of 197 ms, about 2.5% |
| map_count | returned receiver (`push`) | 1.3 M | ≈ 4.4 ms of 180 ms, about 2.5%; map operations are intrinsics, which already lend |
| json | lent parameters on direct calls (slice 4) | not yet split | part of the 8% handle-copy share; slice 1 measures the split between direct, bound and `dyn` calls |
| json | trait meets for `Serializer.member`, `Reader.next` (slice 7) | not yet split | the rest of the 8% |
| json | writes, for borrows through `&self` (slice 5) | — | about 2.5%, judged from 11972f1e3's 400 → 390 ms for the byte intrinsics alone |
| json | by-address lent structs (slice 9) | — | up to the 12% field-copy share; measured before it is built |

Taken together, string_build and map_count should gain about 2 to 3%.
json should gain about 5 to 10% with slices 4, 5 and 7. If slice 9
pays, json could gain up to about 20%.

### Emitted size and the web compiler

Each removed count is a call or an inline fast path less, so release
Wasm shrinks a little; no function is duplicated. The web compiler
builds only the dev pipeline. The release code sits behind a cargo
feature that `hd_web` leaves off, so its gzip size does not change.
Without the feature it would grow about 8 to 10 KB (730 lines at 11 to
14 B per Rust line, tir-v2.md's measure), still far under the 2 MB cap.

### Stop rule

A slice that runs past 1.5 times its line share stops for review.

## Stress test

Each case is a `counts` fixture in slice order, with the result the
design gives. Three of the programs the table names:

```text
struct Config:
    name: string
    depth: i64

fn walk(c: Config, n: i64) -> i64:
    if n == 0:
        return c.depth
    walk(c, n - 1) + 1

fn save!(m: Member) -> void:
    pass

fn pick[T](x: T, m: Member) -> T:
    x
```

| Case | What a naive design does wrong | This design |
| --- | --- | --- |
| `grow_and_measure(xs, &mut xs)` ([Semantics](#semantics)) | lends `a` and sees the push | rule 2 fails at the caller: one retain, as today |
| a callee that stores its parameter in a list | lends and frees too early | `t.stores` sets the kept bit: owned, as today |
| a captured parameter (`later`) | lends; the closure outlives the call | `t.captured`: kept |
| a parameter passed to a `CallValue` | reads a summary it cannot know | declared convention: kept |
| `emit(s: &mut dyn Sink, m: Member)` calling `s.put(m)` | lends to every `put` | declared at the call; the slot glue converts for a lending impl |
| a function value of a lending function, stored and called later | the adapter and the item disagree | the adapter is a direct caller: owned in, lent on |
| self recursion `walk(c, n - 1)` | assumes `c` lent before proving it | kept, conservatively; Lean's fixed point would lend it (next best) |
| mutual recursion across two modules | depends on visit order | SCC rule: no member reads another's summary |
| a bang function `save!(m: Member)` | lends into a frame that keeps it | no summary; its callers already lend |
| a generic `fn f[T](x: T, m: Member)` | lends `x`, an instance's reference | `x` open: kept; `m` closed: may be lent |
| an erased generic method through a trait value | boxes and lends an open value | open values keep the erased ABI |
| a std callee summarized, a user impl of the same trait (slice 7) | uses std's facts for the user impl | the meet includes the user impl; one keeping impl makes all owned |
| a host-implemented trait | assumes a hd body | no body, so no summary; host stubs already lend |
| a builder result kept: `let r = sb.push("a")` | returns no count and frees early | the caller copies the borrowed result: one retain |
| a builder on a temporary: `StringBuilder::new().push("a").build()` | drops the temporary before `build` | the root rule keeps the temporary until the result's last use |
| a returned receiver behind `?` or a `match` arm returning another value | claims `returns` | any other `Return` clears it |
| a `&self` helper that writes a top-level binding | reports `writes` false | a store to a top-level root sets `writes` |
| an inlined std callee | summarizes the copy twice | the inlined body is the caller's; its calls are scanned there |
| one body edit in release | stale summaries in callers | `prog_key` changes; later, the read summaries are in callers' keys |

What the stress test found:

1. **Bound calls carry json's values.** std.serde passes `Member` to
   `Serializer.member` and `Members` to `Reader.next`, trait methods
   called through a bound in generic derived code. Per-body passes
   cannot see one impl there, so slices 2 to 6 leave those calls owned.
   Slice 7's meet answers it, but it is a whole-program fact, and it
   needs the owner's approval.
2. **Conservative recursion loses recursive descent.** A parser's
   recursive functions pass their context to each other. The first
   version keeps such parameters owned. Lean's fixed point (about 30 more
   lines) would lend them; slice 3 records how many parameters the
   conservative rule keeps, to decide.
3. **The field copies are not counts.** A lent struct still passes its
   words. The lane's 12% field-copy share needs a large in-place struct
   passed by the address of the caller's place. That is sound only when
   the callee writes nothing the caller can reach (the `writes` fact),
   or the place is a private local. It is slice 9, gated on a
   measurement.
4. **`push` returns `*mut StringBuilder` from a `*mut self`.** After
   local references step 1 the receiver is a counted reference, which is
   lent already. So the returned-receiver rule is all #681 needs; no lent
   value parameter is involved.

## Phasing

Each slice is about one hour of agent work, independently green, and
measured. Every slice runs the counts and own baselines, conformance on
both pipelines (checked on the optimized one), and the bench rows named.

1. **Measure the split.** A temporary counter in the optimized pipeline
   counts owned arguments whose parameter the callee's borrow tables mark
   unowned. It splits them by callee kind: direct, impl-resolved, bound,
   `dyn` and function value. Done: json, string_build and map_count
   numbers posted on #702; no code change kept.
2. **Summary export.** `Summary` and its extraction from the borrow
   tables, plus the `returns` scan; the `own` view prints it in release
   fixtures. No consumer. Done: own baselines show summaries for the
   stress cases; dev output byte-identical.
3. **Bottom-up order.** The callee scan, iterative Tarjan, SCC marks and
   the `summaries` pipeline entry. No consumer yet. Done: the
   determinism unit test; release Wasm digests unchanged.
4. **Lent parameters on direct calls.** `convention` read by `pass`,
   `received` and the backend glue. Done: the lent and kept counts cases;
   checked optimized conformance green; json bench before and after.
5. **Writes.** The `writes` walk feeds `may_write`. Done: a case where a
   borrow through `&self` survives a pure hd helper; json bench.
6. **Returned parameters (#681).** Callee and caller sides, adapter and
   slot conversion. Done: `counts/discarded-returned-reference` shows no
   count operations; string_build and map_count bench.
7. **Trait method meets** (needs approval). Done: the bound-call cases,
   including a user impl that keeps; json bench.
8. **Escape facts to frame slots.** Done: `*mut self` on a local stays in
   the frame; the escaping boundary cases; dyn_iter and closures bench.
9. **By-address lent structs** (only if slice 1 and 7's numbers leave
   the 12% field-copy share in place). Done: a json gain over noise, else
   the slice is dropped.

## Recommendation and approvals

**Recommendation: option C.** Release summaries carry three facts: kept
parameters, a returned parameter, and writes. They are computed
bottom-up over SCCs in the optimized pipeline, with conservative
recursion, slices 1 to 6 first.

It gives up two things: dev and release no longer match count for
count, and release code depends on callees' bodies. It adds about 730
Rust lines and about 1 ms to a json release build. It should take 2 to
3% off string_build and map_count, and 5 to 10% off json.

The next best is option D, a declaration-level `@inline` on the hot
callees. It needs no whole-program fact, but it reaches only small
functions and needs its own approval.

The owner approved all four ideas on 2026-10-25, each for the release
pipeline only: release reads callee bodies to place counts (type checking
never does), the returned-receiver convention for #681, trait method
meets across every impl in the program for bound calls, and by-address
passing of large read-only structs (slice 9, kept only if it measures a
gain). The slices follow Phasing in order.

## Sources and parse log

**Sources.**

- Sebastian Ullrich and Leonardo de Moura, "Counting Immutable Beans:
  Reference Counting Optimized for Purely Functional Programming", IFL
  2019, <https://arxiv.org/abs/1908.05647>; Lean 4's
  `src/Lean/Compiler/IR/Borrow.lean`,
  <https://github.com/leanprover/lean4/blob/master/src/Lean/Compiler/IR/Borrow.lean>.
- Go's escape analysis, `src/cmd/compile/internal/escape/escape.go`,
  <https://github.com/golang/go/blob/master/src/cmd/compile/internal/escape/escape.go>,
  and `VisitFuncsBottomUp` in `src/cmd/compile/internal/ir/scc.go`.
- Swift Evolution SE-0377, "borrowing and consuming parameter ownership
  modifiers",
  <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0377-parameter-ownership-modifiers.md>;
  Swift's `FunctionSignatureOpts`,
  <https://github.com/swiftlang/swift/tree/main/lib/SILOptimizer/FunctionSignatureTransforms>.
- LLVM Language Reference, parameter attributes (`captures`, `returned`)
  and function attributes (`memory`),
  <https://llvm.org/docs/LangRef.html#parameter-attributes>; the
  `FunctionAttrs` pass, `llvm/lib/Transforms/IPO/FunctionAttrs.cpp`.
- Alex Reinking, Ningning Xie, Leonardo de Moura and Daan Leijen,
  "Perceus: Garbage Free Reference Counting with Reuse", PLDI 2021,
  <https://www.microsoft.com/en-us/research/publication/perceus-garbage-free-reference-counting-with-reuse/>.
- In this repository: docs/compiler/tir-v2.md, docs/compiler/tiering.md,
  crates/hd_opt/DESIGN.md, crates/hd_cache/DESIGN.md,
  crates/hd_mono/DESIGN.md §11 and §13.11, crates/hd_wasm_linear/DESIGN.md
  §23.8 and §23.25 to §23.27, docs/future-work/OBJECT_MODEL_TIER2.md Part
  3, and issues #165, #667, #681 and #702.

**The probes.** Run with `hd --release FILE` at 11972f1e3's worktree.
`plain.hd` is `chain.hd` with `t.add()` replaced by `t.bump()`, and
`lent.hd` is `owned.hd` with `weight(f: &Field)` and `weight(&f)`.

```text
struct Tally:
    count: usize

impl Tally:
    fn add(*mut self) -> *mut Tally:
        self.count = self.count + 1
        self

    fn bump(*mut self) -> void:
        self.count = self.count + 1

pub fn main() -> void $ Console:
    t := &mut Tally { count: 0 }
    for _ in 0..20_000_000:
        t.add()
    println(t.count)
```

```text
struct Field:
    name: string
    n: i64

fn weight(f: Field) -> i64:
    f.n + 1

pub fn main() -> void $ Console:
    f := Field { name: "abc${1}", n: 2 }
    let total: i64 = 0
    for _ in 0..20_000_000:
        total = total + weight(f)
    println(total)
```

**Parse log.** Every `text` block was parsed with
`scripts/hd-conformance.sh parse` at 11972f1e3. Parsing checks syntax
only; it does not type-check a block. The `rust` blocks are sketches and
were not parsed.

| Block | Section | Result |
| --- | --- | --- |
| `grow_and_measure` | Semantics | parses |
| `later` | Semantics | parses |
| `walk`, `save!`, `pick` | Stress test | parses |
| `label` | Approvals, question 1 | parses |
| the builder `main` | Approvals, question 2 | parses |
| `Sink` and `emit` | Approvals, question 3 | parses |
| the `chain` probe | this section | parses, and runs: prints 20000000 |
| the `owned` probe | this section | parses, and runs: prints 60000000 |

The data-flow diagram is a plain fence, not hd. Blocks nested in the
approval list were parsed after removing their indentation.
