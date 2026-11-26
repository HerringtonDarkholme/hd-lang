# The lower layers' builtin list paths are still there (step 1's deletions)

Rule: #2369 step 1 deletes the builtin list path in the same commit
(SWITCH.md §5 group 7); the branch must end net negative over main.

Today (lane 299 WIP): the checker emits no `NewList`, `ListLen`,
`ListReserve` or list `Proj::Elem`, and std has no `list_truncate`,
`list_extend`, `list_reserve` intrinsics. Still present: `IntrinsicOp::
{ListLen, ListReserve}` and `Tag::NewList` with their own/opt/wasm arms,
`prove.rs` `Lists`, `emit/list.rs` but push, `rt.rs` list helpers but
push's, `emit/iter.rs` list arm, `StdKind::List` (the driver still
gives `known.list` the builtin layout, so the struct is laid out and
dropped as a builtin buffer), `coll_kind` list, driver list facts,
`Indexed`'s users of the map half only. Compiler src: branch +1,410 over
the merge base (73270e86c was +1,270; this WIP +140 net, std -145).

Where: SWITCH.md §1 rows hd_tir, hd_opt, hd_layout, hd_wasm, hd_driver.

Fix: after issues 1-5, delete each dead arm; `StdKind::List` last, since
it changes the struct's layout and glue (issue 4's causes 2 and 3).

Done: `rg 'ListLen|ListReserve|Tag::NewList|StdKind::List' crates`
empty but push's and the map's; branch net negative over main.
