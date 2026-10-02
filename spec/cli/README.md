# Command Line Specification

This directory holds the CLI tier of the specification. The numbered
chapters in [`lang/`](../README.md#contents) are the language tier, and
[`std/`](../std/README.md) holds the stdlib tier. The
[Tiers](../std/README.md#tiers) table says what each tier holds.

## Chapters

| File | Rule ID prefix | Scope |
| --- | --- | --- |
| [`command-line.md`](command-line.md) | `cli` | package mode, workspace mode, and single files; `hd`, `hd FILE`, `hd run`, `hd build`, `hd check`, and `hd test`; executables and package tasks; program arguments, host capabilities, machine output, and exit status; `hd new`, and the REPL |
