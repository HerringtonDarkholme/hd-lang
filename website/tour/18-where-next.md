---
title: Where next
---

# Where next

The program on the right prints three places to go after the tour.

## Further reading

- [The playground](../playground) runs whole projects and shows their
  WebAssembly.
- [Learn hd in 10 Minutes](../guide/LEARN_IN_10_MINUTES.md) keeps the syntax
  on one page, while [Working With Data](../guide/WORKING_WITH_DATA.md) builds
  larger programs and packages.
- [The command guide](../guide/COMMANDS.md) covers `hd new`, `hd add`, package
  tasks, formatting, linting, tests, and builds.
- [The specification](../../spec/README.md) states every accepted rule and
  diagnostic.

## One last program

```hd example
for next in [Next.Playground, Next.Guide, Next.Spec]:
    println(label(next))
```

This program uses only forms the tour already introduced: an enum, a `match`
and a `for` loop.

**Run it.** It prints one line for each place. Change a label and press
**Run** again.

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
