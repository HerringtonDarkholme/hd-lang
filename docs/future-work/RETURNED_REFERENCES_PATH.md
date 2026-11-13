# The Path To Returned Local References

**Status:** accepted 2026-10-29; domain: references and lifetimes. Stages
1 to 9 are built and specified in
[spec chapter 13](../../spec/lang/13-values-and-references.md); the
checker's side is [crates/hd_check/DESIGN.md §15](../../crates/hd_check/DESIGN.md#15-lifetimes-one-region-solver).
This record keeps the stage map and the parts that are accepted but not
built (#1357). The full study, with stage 1's rules, the per-slice log
and the prior art, is in git history at 7ccbb4ea8,
`docs/future-work/RETURNED_REFERENCES_PATH.md`.

## The End State

Rust's lifetimes laid over hd's counted references (owner, 2026-10-29):

- A result, and a stored value, depends on a lifetime variable, as
  Rust's `&'a T` does. One form covers functions, struct and enum fields,
  function types, trait methods and stores into a caller's place.
- Lifetimes default by Rust's elision rules, so ordinary code writes none.
- A live result locks its whole source (stage 7); a result path (stage 9)
  narrows the lock to one field.
- Counts stay as the run-time fallback through a `*mut` the checker
  cannot see past.

Path origins (Mojo's `ref[self.names]`) were rejected: they need a
second form wherever the source is not a parameter with fields, and a
`pub` signature would publish private field paths.

## The Stages

Each stage is sound on its own, using run-time counting where its static
rules stop. "Hypothetical syntax" in the table is the wording of the
time; every stage's syntax is now specified.

| Stage | Adds | Unlocks | Static or run time |
| --- | --- | --- | --- |
| 1. Dependent results | a result may hold local references; its source is elided; origins are checked in the body | getters, `&mut Self` builders, `downcast_ref` on `&dyn`, `(&T)?` results | static escape check; memory safety by counts (cells) |
| 2. Scoped locals, then frame slots | first a compile error: a local reference is used only inside the scope of each local it may point into (Rust's E0597); then the backend drops the cells and counts that makes unnecessary, and inlines reference getters | `&` is truly local: no hidden counted cell, only `box` makes counted storage; the counts and cells stage 1 adds go away | static |
| 3. Lifetime variables | `fn f['a](x: &'a T) -> &'a T` (hypothetical); dependent function types; per-position precision | `choose(a, b)`, `get_or(&self, fallback)`, closures that return references | static |
| 4. Element results | reverses E item 4 (owner, 2026-10-29): element references flow into results and bindings | `Span.get`, `SpanMut.get_mut`, `r := &mut xs[i]` | pins panic on a length change (exists today) |
| 5. Generic locality | Rust's model: any `T` may hold a local reference; `T < 'static` (owner, 2026-10-29) only at persistent boundaries such as `box`, provider installs, detached tasks and `dyn` erasure | `Option` methods on `(&T)?`, `debug(&x)`, `id(&x)` (#987) | static |
| 6. Stored references | lifetime parameters on structs and enums; stores into a caller's place; scoped closures; guards; `Span` | the #978 items: phantom markers, guards, read-only captures, library `defer`/`ensure`, live spans | static, counts at heap crossings |
| 7. Exclusivity | Swift's `borrow`/`&` and Rust's rule: a live shared result freezes writes to its whole source, a live `&mut` result every other access; static where the checker sees the source, a per-place run-time check through an unseen `*mut` alias (Swift's dynamic exclusivity) | enum payload places (`as_mut`), exclusive guard captures, compile errors in place of pin panics | static plus pins |
| 8. Uncounted scoped references | the backend drops the copies and counts that stage 7 proves unnecessary (a value read through a `&`/`&mut` parameter across a call); no borrow flags: a reference crossing a `*mut` keeps its count (owner, 2026-10-31: a run-time loan costs about 6x the count pair it would replace, #1118) | the perf payoff: closures −15% to −28%, dyn_iter about −12% | none new; counts stay through `*mut` |
| 9. Field paths on results | a result names the field of its source it points into, `-> &'self.name string`; visible fields only; traits name `'self` | `n := u.name(); u.tags.push(x)` | static |

## Stage 9: Field Paths On Results

A result may name the field of its source it points into (owner,
2026-10-29). The syntax is `'param.path` in the lifetime position: a tick,
the parameter's name, and one or more `.field` steps, so it cannot be
confused with a lifetime variable such as `'a` (owner, 2026-10-31). The lifetime is still the source's; the path adds the place
the loan covers, which only stage 7's conflict check reads. So the caller
locks only that field, across the call:

```text
fn name(&self) -> &'self.name string
fn tags_mut(&mut self) -> &'self.tags mut List[string]

n := u.name()
u.age += 1          # valid: n locks only u.name
u.tags.push("x")    # valid
u.name = "x"        # error: n is live
```

- **Callee check:** the returned reference must come from inside the named
  path: `&self.name.first` is valid, `&self.tags` is `local-reference-escapes`.
- **Visible fields only:** a `pub` signature names only fields visible where
  it is written, so private layout never enters an API; a private function
  may name private fields.
- **Traits:** a trait method names `'self` only, and an impl is no more
  precise than its trait, since bound and `&dyn` callers see the trait.
- **Through `*mut`:** a path through a counted reference stays precise:
  `fn lead_name(t: &Team) -> &'t.lead.name string` locks only
  `t.lead.name`, and an unseen alias is caught per place at run time
  (stage 7).
- **Not inferred:** a caller's validity never depends on a callee's body.
- **Not now (owner, 2026-10-31):** the same for `&mut` parameters (Rust's
  view types, `fn bump(&mut self.count)`).

Slice 9a applied by #1119 ([Result Paths](../../spec/lang/13-values-and-references.md#result-paths)): the syntax in the lifetime position (`&'self.tags mut List[string]`, as `&'a mut T`), the parameter and field checks, the parameter's lifetime, the callee check and the trait rule. Interfaces carry each path's parameter and field names.

Slice 9b applied by #1126 (`ref.result.path.caller`): a call's result in a path position holds the argument's place followed by the path's fields, for `&` and `&mut` results and for chained places (`t.lead.name()` locks `t.lead.name`). The parameter's whole place stays locked when its lifetime is also on a result reference without a path or on another parameter; several paths of one parameter lock the fields they share.

Slice 9c applied by #1127 (`ref.result.path.counted`): a path may cross a `*mut` field (`&'t.lead.name string` with `lead: *mut User`), and the callee check and the caller's lock take the crossing as one more step of the place, as a written `&t.lead.name` does; so may a `*T` field. A step through a local `&T` stays not-yet. Next: 9d, the run-time loan of a path through `*mut` (after #1115).

## Accepted, Not Built

- **View types for `&mut` parameters** (Rust's view types,
  `fn bump(&mut self.count)`): stage 9's paths on a `&mut` parameter, so
  a caller locks only the named fields across the call. Not now (owner,
  2026-10-31); #1357 asks the owner to schedule or close it, with copies
  through `&mut`.
- **Phantom markers** (slice 6i, `Phantom[T]`, Rust's `PhantomData`): not
  now (owner, 2026-10-30). hd has no implicit destructor, its cleanup is
  explicit and LIFO, and variance is declared, so drop checking has
  nothing to protect. Reopens when hd gets an implicit destructor or a
  std type needs a lifetime its fields don't name.

Recorded open when the slices merged, not rechecked since: a direct
closure argument for a lifetime-bounded parameter of a function
(`fn on[F < Fn() & 'a, 'a](f: F)`, slice 6d3), a result capturing two
`fn(...)` parameters (`compose`), and slice 9d, the run-time loan of a
result path through `*mut`.
