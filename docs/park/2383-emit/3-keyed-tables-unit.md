# Link: the remaining emission writes and memos behind the unit (#2383)

Rule: #2383, emit each instance into its own unit and merge serially; the
`.wasm` byte-identical at any `-j` and to main.

Today (lane 303): `stored`, `storage_names`, `frames`, `frame_spills` and
the package contract go through `Module::edit` (`Edit`), and shim
requests and defines through `Unit`. Still written straight from an
instance's emission (lane 303 census, commit d79232c1d):
- `named_codecs.defs`/`at` (`boundary.rs:657-694`): numbered, and read
  back while the codec is built (`named_codecs.def(n.id)`), so it needs a
  local view (base, then unit) like `Symbols`, with ids renumbered where
  they reach a body or a name;
- memos, pure in their key: `row_key_graphs` (`emit/row.rs`), `dyn_paths`
  (a `RefCell`, not `Sync`), `indirect_params` (`emit/indirect.rs:111`),
  `body_facts` (`emit.rs:223`, `get_mut` on a lazy cell). For the
  parallel loop make each a shared memo filled once per key (an
  `OnceLock` per key, or computed in `link: emit prepare`), not a memo
  per unit, which would redo the work per instance.

Done check: no write of a `Module` field from emission outside `Unit`
and `Edit` (the census list); bytes identical (`target/lane303/bytes.sh`).
