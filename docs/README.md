# Docs

Accepted language behavior is in the [specification](../spec/README.md), and
the user guide is in [website/guide/](../website/guide/README.md). This folder
holds everything else.

| Path | Holds |
| --- | --- |
| [compiler/](compiler/ARCHITECTURE.md) | The Rust compiler's design: the [architecture map](compiler/ARCHITECTURE.md) first, then the cross-cutting design docs. Per-crate designs are `crates/*/DESIGN.md` ([crates table](../crates/README.md)). |
| [future-work/](future-work/README.md) | Open and deferred designs, and the [open questions](https://github.com/HerringtonDarkholme/hd-lang/issues/833). |
| [audit/](audit/) | logs: [diagnostic notes](audit/diagnostic-notes.md), the [hd writing log](audit/hd-writing-log.md), the [2026-11-12 review](audit/review-2026-11-12.md), the [2026-11-13 metrics audit](audit/metrics-audit-2026-11-13.md), the [2026-11-13 layout and Wasm review](audit/layout-and-wat-2026-11-13.md), and the [2026-11-13 optimization audit](audit/optimization-audit-2026-11-13.md). |
| [use-scenarios.md](use-scenarios.md) | The two product scenarios and the needs that shape the design. |
