---
title: Where next
---

# Keep going in the playground, the guide, packages, or the specification

The tour introduced hd's everyday pieces in dependency order. To go deeper:

- [The playground](../playground) runs whole projects and shows their
  WebAssembly.
- [Learn hd in 10 Minutes](../guide/LEARN_IN_10_MINUTES.md) keeps the syntax
  on one page, while [Working With Data](../guide/WORKING_WITH_DATA.md) builds
  larger programs and packages.
- [The command guide](../guide/COMMANDS.md) covers `hd new`, `hd add`, package
  tasks, formatting, linting, tests, and builds.
- [The specification](../../spec/README.md) states every accepted rule and
  diagnostic.

This last editor uses only forms the tour already introduced. Change a label
and press **Run**.

```hd
enum Next:
    Playground
    Guide
    Spec

fn label(next: Next) -> string:
    match next:
        .Playground => "playground: whole projects"
        .Guide => "guide: everyday programs"
        .Spec => "spec: every accepted rule"

for next in [Next.Playground, Next.Guide, Next.Spec]:
    println(label(next))
```

```output
playground: whole projects
guide: everyday programs
spec: every accepted rule
```
