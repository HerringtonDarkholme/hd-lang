# Forward a parameter reference's copies and delete hoist's alias rule

Rule: #2369 step 3 (SWITCH.md 0a'): one reference rule, in the forwarding
pass. Lane 297 made `own::local_refs` forward `$self = &$xs` for any local
type; lane 296's `own::hoist::aliases` (an inlined `self` copied from a
never-stored parameter reference names it) is still a second rule.

Today: `hoist::aliases`, `unalias`, `Tables::{alias, plumb}` and the
verifier's `plumb` (`ir/owned.rs` `hoisted`) map `[*$self ..]` to
`[*$p ..]` for the hoist only. Every other pass still sees `$self`. It also
leaves a gap by reading: `facts` does not charge a kept read through
`$self` (a counted copy of `self.block` in the loop) to `$p`, so
`kept_through($p)` misses it. No repro yet: a user helper doing it is not
inlined in release (`fn snap(l: &PlainList[i64]) -> PlainList[i64]: *l`
in a write loop prints the right sum).

Where: `crates/hd_tir/src/own/local_refs.rs` (`forward`: the `Tag::Read`
arm of `values`, `through`), `crates/hd_tir/src/own/hoist.rs`,
`crates/hd_tir/src/ir/owned.rs`.

Tried (lane 297, reverted): a whole read of a PARAM local with no stores
gets `Origin { local: p, fields: [Deref] }`, and `through` accepts Deref
steps past the prefix (a `Ref`'s origin still takes fields only, since a
pointer it reads may change before its use). In
`fn double(xs: &mut PlainList[i64])`'s write loop nothing was forwarded
yet: some use still sets `bad[$0]`; find it (print `bad` with the
instruction that sets it). Then delete the hoist alias rule (about -75
lines).

Done: the alias rule is gone; `a-make-unique-write-loop-hoists-its-check`
and `an-indexed-write-loop-drops-its-tests` keep their WAT; net negative.
