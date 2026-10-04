---
title: Where next
---

# You have seen the core of hd; the playground, the guide, and the spec go further

The tour showed one claim per page. To keep going:

- [The playground](../playground) runs whole projects in the browser, with
  several files, the WebAssembly the compiler makes, and share links. Its
  examples menu has longer programs, such as tests on a fake clock and
  property tests.
- [The Language Tour guide](../../guide/LANGUAGE_TOUR.md) covers the whole
  language on one page, with the details these pages leave out.
- [The specification](../../spec/README.md) states every rule, numbered, with
  the diagnostics the compiler reports and the conformance tests behind them.

Every tour page keeps your edits in this browser, so you can come back to
them; Reset brings the original back. This last editor is yours too: change
anything and Run.

```hd
enum Next:
    Playground
    Guide
    Spec

fn why(next: Next) -> string:
    match next:
        .Playground => "the playground: whole projects, the WebAssembly, share links"
        .Guide => "the guide: the whole language on one page"
        .Spec => "the spec: every rule, numbered, with its diagnostics"

pub fn main() -> void $ Console:
    for next in [Next.Playground, Next.Guide, Next.Spec]:
        println(why(next))
```

```output
the playground: whole projects, the WebAssembly, share links
the guide: the whole language on one page
the spec: every rule, numbered, with its diagnostics
```
