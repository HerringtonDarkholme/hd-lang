# Compile Time And Data Layout Audit, 2026-11-14

Status: an audit log. Nothing here is accepted behavior or an owner
decision. Every direction is a proposal for the owner. Each claim is
marked **[V]** (verified in code, by a run, or by a measurement) or
**[I]** (inferred).

The question was whether the compiler's data layout is struct-of-arrays
(SoA) in the way of the Yuku parser and the Zig compiler, and whether
that matters for compile time. The audit first finds the stage that
costs the most, then reviews that stage's layout most closely.

## Verdict

1. **The front half is SoA, and so is TIR. The back half is not. [V]**
   Tokens, the green tree, the InternPool and TIR are columns with `u32`
   indices, as in Zig. The checker's state, the ownership passes' side
   tables, the Wasm emitter and the Wasm-level optimizer use hash maps
   keyed by dense indices, heap `Vec`s per entry, and a list of
   instructions of up to 56 bytes each.
2. **Layout does not decide compile time today. [V]** After a body edit,
   `Link` and `Collect` take 63% to 98% of the build, on both workloads
   below. The checker takes 0.3% to 13%. Lexing, parsing and skim take
   about 7% of samples on the synthetic workload. The largest costs are
   re-emitting the whole program on each build, the link-time Wasm
   optimizer, two quadratic loops, and allocation in the emitter.
3. **Yuku is a weaker SoA reference than Zig. [V]** Yuku's tokens are
   an array of 16-byte structs. Its nodes are a `MultiArrayList` of two
   columns: a 44-byte tagged union and a span. Zig splits tag, main
   token and an 8-byte data word into separate columns, in the AST, ZIR,
   AIR and MIR.

## Sources And Method

| Item | Value |
| --- | --- |
| Source | local `main` at `13f94845f` |
| Machine | Apple arm64, 14 cores, load average 9 to 15 during the runs |
| Build | `cargo build --release -j 2`; harnesses built against the workspace `target/` |
| Timing | `hd_driver::Counters::stage_ns`, serial executor (`SerialOrder::Priority`), in-memory store, one run per size |
| Profiles | macOS `sample`, 1 ms interval, over a whole harness run (cold, warm, body edit, comment edit) |
| Zig | `master` sources read on 2026-11-14: `lib/std/zig/Ast.zig`, `Zir.zig`, `src/Air.zig`, `src/Air/Liveness.zig`, `src/InternPool.zig`, `src/Sema.zig`, `src/codegen/wasm/{CodeGen,Mir}.zig`, `src/codegen/x86_64/CodeGen.zig`, `src/codegen/llvm.zig` |
| Yuku | `main` of <https://github.com/yuku-toolchain/yuku>: `src/parser/{ast,token,parser}.zig` |

Wall times are good for ratios between runs taken together, not as
absolute numbers. Sample shares are inclusive and overlap, because one
function calls another.

### Workload 1: Small Integer Functions

`bench/profile-signature` (`hd_driver::bench::sources`): two folders of
n functions each, every function a short `i32` loop, plus std.

| n per folder | cold total | Body (checker) | Link | Collect | body-edit total | edit: Link + Collect |
| --- | --- | --- | --- | --- | --- | --- |
| 1,000 | 555 ms | 218 ms | 122 ms | 49 ms | 215 ms | 163 ms (76%) |
| 2,000 | 818 ms | 254 ms | 252 ms | 98 ms | 535 ms | 416 ms (78%) |
| 4,000 | 1,633 ms | 433 ms | 508 ms | 214 ms | 962 ms | 714 ms (74%) |
| 8,000 | 3,727 ms | 626 ms | 1,482 ms | 802 ms | 2,483 ms | 1,552 ms (63%) |

A warm run with no edit takes 17, 38, 102 and 476 ms at these sizes,
while its timed stages add up to about 40 ms at 8,000. Item C3 explains
the rest. **[V]**

Profile at n = 8,000 (10,231 samples, inclusive): emission
(`hd_wasm::emit`) 56%, `Collect` 22%, ownership passes (`hd_tir::own`)
22%, Wasm optimizer (`hd_wasm::opt`) 20%, `FileSkim::of` 18%, checker
(`hd_check`) 8%, `hd_syntax` 7%, malloc, free and realloc 16%, hash-map
operations 3%. **[V]**

### Workload 2: Generic And Trait-Heavy Modules

A generated package of one `base` module and N modules of about 80
lines each, plus a `main` that calls every module. `base` holds a
generic `Stack[T]` with an impl, `Pair[A, B]`, a trait with a default
method, a trait with an associated type, bounded generic functions,
`Map[i64, List[T]]` grouping and a `*dyn` trait-value list. Each module
adds a struct, an enum with payloads, an `@error` enum, two trait impls,
closures over iterator chains (`filter`, `map`, `sum`, `collect`), a
comprehension, `Result` and `?`-style early returns, `Option` chains and
`box`. Module K calls module K-1, so every module instantiates the base
generics at its own types. The generator is in the appendix.

| Modules | Lines | cold total | Body | Link | Collect | Wasm | body-edit total | edit: Link + Collect |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 25 | 2,134 | 571 ms | 210 ms | 148 ms | 65 ms | 221 KB | 224 ms | 210 ms (94%) |
| 50 | 4,209 | 804 ms | 238 ms | 290 ms | 120 ms | 426 KB | 410 ms | 393 ms (96%) |
| 100 | 8,359 | 1,275 ms | 305 ms | 560 ms | 225 ms | 835 KB | 794 ms | 771 ms (97%) |
| 200 | 16,659 | 2,581 ms | 423 ms | 1,335 ms | 581 ms | 1,657 KB | 1,617 ms | 1,581 ms (98%) |

- The checker's cold time is about 190 ms of std plus about 1.2 ms per
  module. A body edit rechecks one module in about 5 ms. **[V]**
- `Link` costs about 5.6 to 6.7 ms per 80-line module, and grows 2.4x
  from 100 to 200 modules. **[V]**
- Each module emits about 8 KB of Wasm. **[V]**

Profile at 200 modules (3,532 samples under `hd_driver::build`,
inclusive):

| Part | Samples | Share |
| --- | --- | --- |
| `hd_wasm::emit`, all of `Link` | 2,657 | 75% |
| `Module::finish_link`, the link-time optimizer | 1,361 | 39% |
| of which `hd_wasm::opt::locals` | 958 | 27% |
| `hd_mono::collect` | 821 | 23% |
| `hd_tir::own`, the ownership passes | 780 | 22% |
| malloc, free and realloc | 580 | 16% |
| `hd_types::pool` reads | 378 | 11% |
| `hd_check`, mostly std's cold check | 338 | 10% |
| hash-map operations | 118 | 3% |
| trait solver (`hd_types::solver`) | 40 | 1% |

Generic and trait-heavy code moves no time into the checker or the
solver. It moves time into the back half, because each instantiation is
emitted and then optimized at link. **[V]**

## Layout By Stage

| Stage | hd | Zig | Yuku |
| --- | --- | --- | --- |
| Tokens | columns `kind`, `start`, `end`, line and comment columns (`crates/hd_syntax/src/lexer.rs:164`) | `MultiArrayList{tag, start}` | an array of 16-byte `extern struct` tokens |
| Syntax tree | columns `kind`, `first_token`, `last_token`, `subtree_len`, 14 bytes a node (`crates/hd_syntax/src/green.rs:143`) | `MultiArrayList{tag, main_token, data}` and `extra_data` | `MultiArrayList{data: 44-byte union, span}` and `extras` |
| Types | columns `tag`, `data`, `meta`, `extra`, `tys` behind one append lock and 64 dedup shards (`crates/hd_types/src/pool.rs:499`, `:689`) | per-thread item and extra lists (`locals`), thread id in the top bits of an `Index`, sharded dedup maps | none |
| Checker output | TIR columns `tags`, `data: [u32; 2]`, `ty`, `syn`, `extra` and side columns (`crates/hd_tir/src/ir.rs:906`) | AIR `MultiArrayList{tag: u8, data: 8 bytes}` and `extra` | none |
| Checker state | `Ck`: 128 fields, 19 of them hash maps or sets, one `HashMap<Symbol, LocalId>` per scope (`crates/hd_check/src/body.rs:249`) | Sema: `InstMap`, a dense array from ZIR to AIR; about 7 hash maps for rare facts | none |
| Inference | columns `parent`, `rank`, `binding`, `kind` and a trail (`crates/hd_types/src/unify.rs:55`) | no inference variables | none |
| TIR passes | read TIR columns; side state in `HashMap<u32, _>` and `HashSet<u32>` at 19 sites in `hd_tir/src/own` and `hd_opt` | Liveness: a packed column of 4 bits an instruction, and a hash map for special cases | none |
| Emitter state | `Em`: 85 fields, 35 hash maps or sets; `vals`, `locals` and `param_vals` are `HashMap<_, Vec<u32>>` (`crates/hd_wasm/src/emit.rs:1251`) | `values: AutoArrayHashMap(Ref, WValue)`, a small value per entry, no allocation | none |
| Emitted code | `Func::body: Vec<Ins>`, each `Ins` asserted at most 56 bytes (`crates/hd_wasm/src/module.rs:522`, `:537`) | Wasm MIR `MultiArrayList{tag, data}`, `data` 4 bytes in release builds | none |

Zig's back ends also map an instruction to its value through a hash map:
the Wasm back end's `values`, the x86_64 back end's `inst_tracking`, and
the LLVM back end's `func_inst_table`. The difference in hd is what each
entry holds, not the map. **[V]**

## Findings

Items already filed elsewhere are cited, and only new evidence is added
here.

1. **C1. The whole program is emitted again on every build. [V, review
   A1, optimization audit S8]** On workload 2, a one-module body edit
   spends 94% to 98% of its time in `Link` and `Collect`. Per-instance
   code entries are planned and not built
   ([data-structures.md §3.22](../compiler/data-structures.md#322-codegen-the-instance-table-and-code-entries)).
   This is the largest cause on both workloads.

2. **C2. The link-time Wasm optimizer is 39% of a generic-heavy build.
   [V, review A5]** `Module::finish_link` runs on every link. Its
   `opt::locals` (`crates/hd_wasm/src/opt.rs:385`) loops over nine
   rewrite rules until none changes the function, and alone takes 27% of
   samples on workload 2. No bound on the number of rounds is stated, as
   the AGENTS.md commit bar asks of a fixpoint.

3. **C3. Skimming a file is quadratic in its size. [V, new]**
   `use_ranges` (`crates/hd_syntax/src/skim.rs:220`) records every line
   whose first token is an identifier. It never compares the token's
   text with `use`. `FileSkim::of` (`crates/hd_driver/src/skims.rs:114`)
   then runs `in_tests` on each such line, and `in_tests` walks every
   body of the file. The cost is identifier-led lines times bodies. At
   8,000 functions in one file it is 18% of all samples. **[I]** By the
   profile, it is also most of the warm run's untimed 430 ms. It runs on
   one thread before any task, so it also explains part of optimization
   audit S4. **[I]** Every
   identifier-led line also gets an empty use path in
   `FileSkim::use_paths`. What that changes was not traced.

4. **C4. `promotable_args` scans the whole body once per call. [V,
   review F21]** `crates/hd_tir/src/own/promote.rs:235` runs
   `b.tags.iter().any(is_suspension)` on each call, and `Collect` calls
   it for each call site (`crates/hd_mono/src/lib.rs:1702`). The cost is
   calls times instructions. One flag per body removes the scan.

5. **C5. The emitter allocates per value. [V, new]** `Em::vals`,
   `locals` and `param_vals` hold a heap `Vec<u32>` for each TIR value
   or local. `Em` holds 35 hash maps and sets. Allocation and copying
   under emission and the Wasm optimizer took about 980 samples on
   workload 1, about 10% of the run. Zig's Wasm back end keeps a small
   inline value per entry. **[I]** A dense column indexed by
   instruction, with a short inline list and an `extra` overflow, would
   remove most of these allocations.

6. **C6. Emitted code is a list of fat enums. [V, new]** `Func::body`
   is `Vec<Ins>`, with `Ins` asserted at 56 bytes or less. The link-time
   optimizer walks and rewrites these lists repeatedly. Zig's Wasm MIR
   is a 1-byte tag column and a 4-byte data column. **[I]** The size
   difference matters to C2's passes, which read every instruction on
   every round.

7. **C7. The growth is above linear at the largest sizes. [V]** From
   4,000 to 8,000 functions a folder, `Collect` grows 3.7x and `Link`
   2.9x. From 100 to 200 modules on workload 2, `Link` grows 2.4x and
   `Collect` 2.6x. C3 and C4 account for part of it. The rest was not
   isolated.

8. **C8. The checker's state is not SoA, and does not cost much yet.
   [V]** `Ck` has 128 fields, 19 of them hash maps keyed mostly by
   `LocalId`, `NodeIdx` or `u32`. The checker is 8% to 13% of samples
   here, most of it std's cold check. Revisit it after C1 to C5.

9. **C9. The design record misdescribes Zig's InternPool. [V]**
   [data-structures.md §3.9.2](../compiler/data-structures.md#392-the-internpool)
   says one append lock "is Zig 0.14's scheme". Zig keeps one `Local`
   item list per thread (`locals: []Local`) and puts the thread id in
   the top bits of each `Index`. Only the dedup maps are sharded and
   locked. hd's pool did not appear as a lock cost in these serial
   runs, so this is a wording fix for now.

## Caveats

- Both workloads are generated. Workload 2 uses the generics and traits
  above but no async code, no derives beyond `Eq` and `Debug`, and no
  large match trees.
- The executor was serial. The pool executor would spread `Body` tasks,
  but `Link` is one task, so it would raise `Link`'s share.
- Sampling adds overhead. The timing tables come from runs without the
  sampler.
- The Zig and Yuku facts come from field declarations and a few code
  paths. Zig's `Parse.zig`, `Zcu/PerThread.zig` and most of `Sema.zig`
  were not read.

## Appendix: Reproducing

Workload 1: `cargo run --release --manifest-path
bench/profile-signature/Cargo.toml -- N`.

Workload 2: put `gen.py`, `module.tmpl` and `base.hd` in one folder, run
`python3 gen.py OUT N`, and time it with the harness below, which reads
`OUT/src` into `MemorySources`. Pass the package folder and the module
to edit, as `hd-genharness OUT m100`. Its `Cargo.toml` is the one of
`bench/profile-signature` with the package renamed and absolute paths to
the four crates.

`gen.py`:

```python
# Generate a generic- and trait-heavy hd package: base.hd + N modules + main.
import sys, os
out, n = sys.argv[1], int(sys.argv[2])
here = os.path.dirname(os.path.abspath(__file__))
tmpl = open(os.path.join(here, 'module.tmpl')).read()
os.makedirs(os.path.join(out, 'src'), exist_ok=True)
open(os.path.join(out, 'hd.toml'), 'w').write('[package]\nname = "genbench"\n\n[capabilities]\nConsole = true\n')
open(os.path.join(out, 'src', 'base.hd'), 'w').write(open(os.path.join(here, 'base.hd')).read())
for k in range(n):
    if k == 0:
        use, up = '', '    upstream: i64 = 0'
        up = '    let upstream: i64 = 0'
    else:
        j = k - 1
        use = f'use pkg.m{j}.{{Widget{j}, score{j}, make{j}}}'
        up = f'    upstream := score{j}(make{j}(n))'
    text = tmpl.replace('@USE@', use).replace('@UP@', up).replace('@K@', str(k))
    open(os.path.join(out, 'src', f'm{k}.hd'), 'w').write(text)
main = ''.join(f'use pkg.m{k}.run{k}\n' for k in range(n))
main += '\npub fn main() -> void $ Console:\n    let sum: i64 = 0\n'
main += ''.join(f'    sum = sum + run{k}(+5)\n' for k in range(n))
main += '    println("$sum")\n'
open(os.path.join(out, 'src', 'main.hd'), 'w').write(main)
```

`base.hd`:

```text
# Shared generic building blocks.

pub trait Measure:
    fn measure(&self) -> i64
    fn tag(&self) -> string: "item"

pub trait Merge:
    type Out
    fn merge(&self, other: &Self) -> Self::Out

pub struct Stack[T]:
    items: List[T]

impl[T] Stack[T]:
    pub fn new() -> Stack[T]: Stack { items: [] }
    pub fn push(&mut self, value: T) -> void: self.items.push(value)
    pub fn pop(&mut self) -> T?: self.items.pop()
    pub fn len(&self) -> usize: self.items.len()
    pub fn peek(&self) -> T?: self.items.get(self.items.len() - 1)

pub struct Pair[A, B]:
    pub first: A
    pub second: B

pub fn swap[A, B](pair: Pair[A, B]) -> Pair[B, A]:
    Pair { first: pair.second, second: pair.first }

pub fn total[T < Measure](values: List[T]) -> i64:
    values.iter().map(fn(value): value.measure()).sum::[i64]()

pub fn heaviest[T < Measure](values: List[T]) -> T?:
    let best: T? = .None
    for value in values:
        best = match best:
            .Some(current) if current.measure() >= value.measure() => .Some(current)
            _ => .Some(value)
    best

pub fn bucket[T < Measure](values: List[T], width: i64) -> Map[i64, List[T]]:
    let mut out: Map[i64, List[T]] = {}
    for value in values:
        key := value.measure() / width
        let mut row = out.get(key).unwrap_or([])
        row.push(value)
        out[key] = row
    out

pub fn fold_all[T < Merge](values: List[T], start: T::Out, step: fn(T::Out, T::Out) -> T::Out) -> T::Out:
    let acc = start
    for i in 1..values.len():
        acc = step(acc, values[i - 1].merge(&values[i]))
    acc

pub fn describe_all(values: List[*dyn Measure]) -> string:
    values.iter().map(fn(value): "${value.tag()}=${value.measure()}").collect::[List[string]]().join(",")
```

`module.tmpl`, where `@K@` is the module number, `@USE@` the import of
module K-1 and `@UP@` the call into it:

```text
# Module @K@: widgets measured through generic helpers.
use pkg.base.{Measure, Merge, Stack, Pair, swap, total, heaviest, bucket, fold_all, describe_all}
@USE@

@derive(Eq, Debug)
pub struct Widget@K@:
    pub id: i32
    pub label: string
    pub weight: i64

pub enum Event@K@:
    Added(widget: Widget@K@)
    Removed(id: i32)
    Renamed(id: i32, label: string)

@error
pub enum Fault@K@:
    @error("unknown widget $id")
    Unknown(id: i32)
    @error("too heavy: $weight")
    Heavy(weight: i64)

impl Measure for Widget@K@:
    fn measure(&self) -> i64: self.weight * i64(self.id + 1)
    fn tag(&self) -> string: self.label

impl Merge for Widget@K@:
    type Out = i64
    fn merge(&self, other: &Widget@K@) -> i64: self.weight + other.weight

pub fn make@K@(n: i32) -> List[Widget@K@]:
    [for i in 0..usize(n) => Widget@K@ { id: i32(i), label: "w@K@_$i", weight: i64(i) * 3 + 1 }]

fn apply@K@(widgets: &mut List[Widget@K@], events: List[Event@K@]) -> Result[usize, Fault@K@]:
    let changed: usize = 0
    for event in events:
        match event:
            .Added(widget) =>
                if widget.weight > 1000:
                    return .Err(Fault@K@.Heavy(widget.weight))
                widgets.push(widget)
            .Removed(id) =>
                before := widgets.len()
                *widgets = widgets.iter().filter(fn(w): w.id != id).collect()
                if widgets.len() == before:
                    return .Err(Fault@K@.Unknown(id))
            .Renamed(id, label) =>
                for i in 0..widgets.len():
                    if widgets[i].id == id:
                        widgets[i] = Widget@K@ { ...widgets[i], label: label }
        changed = changed + 1
    .Ok(changed)

fn stacked@K@(widgets: List[Widget@K@]) -> i64:
    let mut stack = Stack::[Widget@K@]::new()
    for widget in widgets:
        stack.push(widget)
    let sum: i64 = 0
    while true:
        match stack.pop():
            .Some(widget) => sum = sum + widget.measure()
            .None => break
    sum

pub fn score@K@(widgets: List[Widget@K@]) -> i64:
    widgets.iter().filter(fn(w): w.weight % 2 == 1).map(fn(w): w.measure()).sum::[i64]()

pub fn run@K@(n: i32) -> i64:
    let mut widgets = make@K@(n)
    events := [Event@K@.Added(Widget@K@ { id: 99, label: "new", weight: 7 }), Event@K@.Renamed(1, "renamed"), Event@K@.Removed(0)]
    applied := match apply@K@(&mut widgets, events):
        .Ok(count) => i64(count)
        .Err(fault) => -1
    pair := swap(Pair { first: total(widgets), second: score@K@(widgets) })
    top := heaviest(widgets).map(fn(w): w.weight).unwrap_or(0)
    groups := bucket(widgets, 10)
    merged := fold_all(widgets, 0, fn(a, b): a + b)
    let shown: List[*dyn Measure] = [box(Widget@K@ { id: 1, label: "a", weight: 2 })]
    text := describe_all(shown)
@UP@
    applied + pair.first + pair.second + top + i64(groups.len()) + merged + i64(text.len()) + stacked@K@(widgets) + upstream
```

The harness, `src/main.rs`:

```rust
// Stage timings of a package read from disk: cold, warm, body edit, comment edit.
use std::time::Instant;
use hd_cache::MemoryStore;
use hd_driver::{Clock, Counters, Executor, Goal, Host, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

struct Wall(Instant);
impl Clock for Wall {
    fn now_ns(&self) -> u64 { u64::try_from(self.0.elapsed().as_nanos()).unwrap_or(u64::MAX) }
}

fn report(label: &str, total: std::time::Duration, c: &Counters) {
    let mut stages: Vec<_> = c.stage_ns.iter().map(|(k, v)| (*k, *v / 1_000_000)).collect();
    stages.sort_by_key(|(_, ms)| std::cmp::Reverse(*ms));
    let sum: u64 = c.stage_ns.values().sum::<u64>() / 1_000_000;
    println!("== {label}: total {} ms, stages {sum} ms {stages:?}", total.as_millis());
}

fn compile(store: &MemoryStore, clock: &Wall, s: &MemorySources, ex: Executor) -> Output {
    let host = Host { sources: s, store, clock, executor: ex, render_tir: &[] };
    build(&host, "genbench", &Goal::Program { entry: "main".into() })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = std::path::PathBuf::from(&args[0]).join("src");
    let edit_module = &args[1];
    let ex = args.get(2).and_then(|v| v.parse().ok()).map_or(Executor::Serial(SerialOrder::Priority), Executor::Pool);
    let mut files: Vec<(String, String)> = std::fs::read_dir(&dir).unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&p).unwrap()))
        .collect();
    files.sort();
    let mk = |edit: &dyn Fn(&str, &str) -> String| {
        let mut s = MemorySources::default();
        for (name, text) in &files { s.insert(name, &edit(name, text)); }
        s
    };
    let store = MemoryStore::default();
    let clock = Wall(Instant::now());
    let t = Instant::now();
    let cold = compile(&store, &clock, &mk(&|_, t| t.to_owned()), ex);
    report("cold", t.elapsed(), &cold.counters);
    if !cold.diags.is_empty() { eprintln!("{}", cold.render()); std::process::exit(1); }
    println!("   wasm {} bytes", cold.wasm.as_ref().map_or(0, Vec::len));
    let t = Instant::now();
    let warm = compile(&store, &clock, &mk(&|_, t| t.to_owned()), ex);
    report("warm", t.elapsed(), &warm.counters);
    let target = format!("{edit_module}.hd");
    let t = Instant::now();
    let body = compile(&store, &clock, &mk(&|n, t| if n == target { t.replace("* 3 + 1", "* 3 + 2") } else { t.to_owned() }), ex);
    report("body edit", t.elapsed(), &body.counters);
    let t = Instant::now();
    let com = compile(&store, &clock, &mk(&|n, t| if n == target { t.replacen("    let changed", "    # note\n    let changed", 1) } else { t.to_owned() }), ex);
    report("comment edit", t.elapsed(), &com.counters);
}
```
