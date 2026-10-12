# Compiler benchmark inputs

These programs exercise the current compiler subset with inputs larger and
more varied than `compiler/samples/`. Run a checked-in case from `compiler/`:

```sh
cargo run --release -p hd_cli -- bench/tokenizer/main.hd
```

Compare its stdout with the adjacent `expected.out`. The hand-written cases
are:

- `tokenizer`: an integer-encoded tokenizer state machine;
- `math`: iterative integer kernels;
- `records`: records crossing a generic helper;
- `traits`: dispatch through generic trait bounds.

Generate the fifth case, an exact 10,000-line package spread across four
nested folders, then run it:

```sh
node bench/generated/generate.mjs
(cd bench/generated/out/package && cargo run --release --manifest-path ../../../../Cargo.toml -p hd_cli -- run)
```

Generated sources live under `generated/out/` and are intentionally ignored.
The generator accepts another output directory as its first argument.

The reusable profiler avoids benchmark item names that collide with the
prelude. It runs cold, warm, body-edit and comment-edit cases, optionally
on the pool:

```sh
cargo run --release --manifest-path bench/profile-signature/Cargo.toml -- 200
cargo run --release --manifest-path bench/profile-signature/Cargo.toml -- 200 8
```

Its `signature` mode fills the edit case that the driver's built-in
benchmark does not yet measure:

```sh
cargo run --release --manifest-path bench/profile-signature/Cargo.toml -- 200 signature
```
