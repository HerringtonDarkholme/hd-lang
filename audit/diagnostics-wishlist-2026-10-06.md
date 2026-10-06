# Diagnostics Wishlist For The New Compiler

Date: 2026-10-06. Source: `audit/hd-writing-log.md` — 111 rows whose
Helped? is `no` or `partly`, from Haiku, Sonnet, Opus, GPT, and Kimi
writing hd. The new compiler is judged by retries per program
(future-work/NEW_COMPILER_ARCHITECTURE.md, "Arena", pillar 1). Each
today-message was re-run with the compiler at 8f9b0fe1.

Already fixed on 2026-10-06 (verified by re-running, then left out of the
groups): the P1k hints (unknown-method did-you-mean, unknown-name/-type
std import hints, mutable-receiver-required's `let mut` fix,
type-used-as-value's provider hint, unknown-variant's variant list),
panic source locations (today `index-out-of-bounds` prints file:line:col),
the fresh-literal `==` mix (`Date` vs `mut:Date` is gone; today it is a
plain missing-`Eq`), `keys()` and `join` on `Map`/`List` exist, and the
integer-literal range message now fits the context's unsigned type.

## The ten changes that would save the most retries

1. `cannot-infer-type`'s fix-it must include `mut` when the binding is
   later mutated: today it suggests `let pieces: List[string] = []`, and
   the next `push` fails again (4 rows).
2. `missing-contextual-enum-type` should say the fix: give one side an
   expected type, or call `.is_some()` (6 rows; one is unfixed today:
   `match (a?, b?)` with `(.Ok(x), .Ok(y))` still has no hint).
3. `race!`/`all!` with a bang child should say "pass the cold value:
   write `slow()`" (3 rows, unchanged today).
4. Never print an internal name or location: `__std_hash_DefaultHasher`,
   `mut:List[char]`, or the user's line 1 for a std problem (9 rows,
   all from the old loader; the new compiler must not reproduce them).
5. A run with no entry point must not exit 0 silently: a private `main`
   runs nothing and prints nothing (3 rows, unchanged today).
6. `defer` cannot do I/O: `suspending-defer` should say to buffer into a
   list and flush after the scope, and the rule deserves one line in the
   tour's defer section (3 rows).
7. `report_of` for the cause chain: printing an error shows only the top
   message; nothing leads to it (2 rows). A doc line in the error section
   of the tour would do.
8. `Map.new()` and `{:}` should both point at `{}` (2 rows, unchanged
   today).
9. `type-used-as-value` should always try the provider hint, not only
   when the name is already a row key (2 rows; P1k covers the row-key
   case only).
10. `index-out-of-bounds` should name the missing key, not only the
    location (1 row; the location part is fixed).

## Group 1: Binding and `mut` forms (17 rows)

Newcomers mix up `let mut x`, `let x: mut T`, and `x :=`, and the errors
mostly help today (`mut-on-primitive`, `missing-let`,
`non-reassignable-binding` all name the fix). The one message that does
not: `cannot-infer-type`'s fix-it. Representative rows:

- probe 4: args tool (kimi): wrote `pieces := []`, then
  `pieces.push(text)`; got `cannot infer the element type of '[]';
  annotate the binding: 'let pieces: List[T] = ...'` — today, unchanged:
  `let pieces: List[string] = []` then fails again at `push`. An agent
  needs: "annotate with the mut form: `let pieces: mut List[string] =
  []`".
- #225 lib/std collections (opus-5.5): wrote `Deque::[T] { slots: [], ... }`;
  got `cannot infer T in List[T]` at the user's line 1:1. An agent needs:
  the inferred type named at the field, not at the `use`.
- #234 lib/std helpers (opus-5.5): `Set::[T] { entries: {} }`; same
  message, located at the program's `use` line, not in std.

## Group 2: Option/Result in matches and comparisons (6 rows)

`missing-contextual-enum-type` needs the fix spelled out. The `==`
half is fixed (today `x == .Some(3)` compiles). Representative rows:

- website examples #231 (sonnet-5.5): `(parse_i32(w), parse_i32(f))`
  matched with `(.Ok(w), .Ok(f))`; today, unchanged: `variant pattern
  '.Ok' requires an enum type, found 'Result[...]'`. An agent needs:
  "match each Result in its own arm, or give the tuple an expected type".
- #200 scratch (opus-5.5): `largest(maybe) == .Some(3)`; today this
  compiles — fixed on 2026-10-06.
- #243 lib/std regex (opus-5.5): `source.at(at) == .Some('|')`; same
  fix.

## Group 3: Std internals leak into messages (9 rows)

All from the old std loader: internal names (`__std_time_Clock`,
`mut:List[char]`), and locations at the user's first line for a std
problem. Representative rows:

- #222 lib/std collections (opus-5.5): `unsatisfied-trait-bound: type
  'generic:K' does not implement Eq and Hash` at line 3 of the user's
  program.
- #220 lib/std time (opus-5.5): `unknown-method: trait
  '__std_time_Clock' has no method 'now'` at line 1 of any program that
  reaches std.time.
- dogfood #199: calc (sonnet): `unknown-method: type 'mut:List[char]'
  has no supported method 'push'` — the internal spelling of the type.

The new compiler: no internal name ever prints, and a std problem points
at the std line or nowhere, never at the user's `use`.

## Group 4: Suspension call shapes (7 rows)

`bang-call-outside-suspension` helps today (names the fix). The cold-call
rule for the combinators does not. Representative rows:

- probe 5 and probe 6: concurrent fetcher (kimi): `race!(fetch_user(),
  slow!())`; today, unchanged: `expected mut Suspend[string], found
  string`. An agent needs: "race! takes cold suspensions; write
  `slow()`, without the bang".
- dogfood #199: tasks (sonnet): `all!(...items.map(...))`; today,
  unchanged: `expected an expression, found '...'`. An agent needs: "all!
  takes a fixed argument list; build it by name".

## Group 5: `defer` cannot do I/O (3 rows)

- probe 6: file import (kimi): `defer: _ := $.use(FsWrite).remove!(tmp)`;
  today, unchanged: `a defer suite cannot make a bang call`. An agent
  needs: "defer suites cannot suspend; collect the path and remove it
  after the scope".
- website examples #231 (sonnet-5.5): `println` inside a `defer` block;
  `suspension-forbidden-context`. Same fix.

## Group 6: Silent no-ops (4 rows)

- M6, #201, GADT variant results #278 (opus-5.5 ×3): a private `main`
  prints nothing and exits 0. Today: `hd check` warns
  (`private-main`), but a plain run still exits 0 with no output. An
  agent needs the run to fail or print the warning.
- perf microbench (kimi): `_ := fib(30)`; the build dropped the call
  silently. An agent needs a warning when the whole body is discarded.

## Group 7: Doc gaps, not compiler messages (8 rows)

- `loop:` for an endless loop (2 rows, opus and haiku): today
  `unknown-name: unknown function 'loop'`. The tour shows `while true:`;
  one line there would end these.
- probe 6: file import (kimi): `"$error"` hides the cause chain; nothing
  points to `report_of`. The error section of the tour should show it.
- dogfood #199: orders (sonnet): `now.millis`; `Timestamp` has no public
  accessor. The time section should show `since(...)` +
  `as_milliseconds()`.
- probe 6: pricing (kimi): a doc test needs its own imports; the error
  does not say so. One line in the doc-test rule would do.
- #243 regex scratch (opus-5.5): `Result` has `expect(message)`, not
  `unwrap`. A one-liner in the Result tour section.

## Group 8: CLI and entry confusion (3 rows)

- usability probe 2, slug CLI (haiku): `hd run "Hello, World!"` reads as
  a task name. `hd run -- args` is the rule; the error should suggest it.
- #247 range fixture (opus-5.5): `assert_equal(to == ..=5, ...)`; a
  bare range is not an expression. Bind it first.
- usability probe #77, shop with path deps (haiku): a path requirement
  must name a workspace member; today the message says so. Fixed.

## Group 9: Leftovers (misc, 1-2 rows each)

`integer-literal-range` naming `i64` for a `u64` context (3 rows) — fixed
today: the literal takes the context's unsigned type. `generic-arity` on
a qualified trait call forgetting `Rhs = Self` (1 row). `invalid-token`
for `///` doc comments (1 row: `##` is the spelling; the message should
say so). `Map.new()` (1 row, unchanged today: `unknown name 'Map'`).
