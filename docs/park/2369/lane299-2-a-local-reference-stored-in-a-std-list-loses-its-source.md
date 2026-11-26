# A local reference stored into a std List element loses its source (memory fault)

Rule: `ref.dep.scope`: a local reference stored in a slot of a local
list keeps its source alive until the list's last use; a reference read
through the old slot keeps the old source.

Today (lane 299 WIP, List is the std struct):
`spec/conformance/lang/references/dependent-results/local-scope/a-local-reference-stored-in-a-list-slot-keeps-its-source.hd`
faults `memory access out of bounds` in `StringBuilder.value_text`:
`d := &ps[0].name` then `ps[0] = &q` frees `p` before `*d` is read.
The builtin list kept the source through its `Proj::Elem` place; the
std path reaches the element through `Index::index`/`IndexSet::index_set`
call results, and the ownership passes do not charge `d` to `p`.
Also: `...-through-a-pointer.hd`, `...-in-every-form.hd`.

Where: `crates/hd_check/src/dep_results` (call origins of `index`'s
`&T` result over a `List[&P]`), `crates/hd_tir/src/own/derive.rs`.

Fix: a reference read through a `&`-typed value returned by a call
keeps that value's own source (Rust's loan stops at a `&` dereference),
whatever call returned it; the builtin's element rule as a call rule.

Done: the three cases print their expected stdout in dev, checked and
release.
