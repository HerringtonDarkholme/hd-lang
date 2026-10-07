# Compiler benchmark inputs

These programs exercise the current compiler subset with inputs larger and
more varied than `compiler/samples/`. Run a checked-in case from `compiler/`:

```sh
cargo run --release -p hd_cli -- run bench/tokenizer
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
cargo run --release -p hd_cli -- run bench/generated/out/package
```

Generated sources live under `generated/out/` and are intentionally ignored.
The generator accepts another output directory as its first argument.
