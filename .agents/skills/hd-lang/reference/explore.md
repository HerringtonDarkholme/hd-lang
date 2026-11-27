# When The Skill Has No Answer

Where to look next, cheapest first. hd changes, and the compiler is the
authority: when this skill and `hd` disagree, `hd` is right.

## In order

1. **The diagnostic.** Read the message and its `fix:` line
   ([reading output](../cli.md#reading-the-output)). Many messages say
   the fix outright.
2. **[diagnostics.md](diagnostics.md)**, by the diagnostic's code.
3. **A probe.** Pipe a few lines to `hd` and see what happens:
   `echo 'println(debug([3, 1, 2].sorted()))' | hd`. A probe answers
   "does this check" and "what does this print" faster than any page.
4. **[reference/std/](std/README.md)** for a std name or signature: one
   small file per module, so open the one you need (`reference/std/fs.txt`),
   or search them all: `grep -rh 'trim' reference/std/`.
5. **`hd doc`** for one item's full text and examples, inside any package:
   `hd doc std.task.retry`, `hd doc std.meta.Walker`. Outside a package,
   make a probe package once: `hd new /tmp/hd-probe --lib --vcs none`.
6. **The topic's page in this skill.** [SKILL.md](../SKILL.md) routes by
   situation.
7. **[limitations.md](limitations.md)**: what hd can't do yet, and what
   to do instead.
8. **The hd-lang sources** (online; a `cargo install` leaves no copy on
   disk):
   - the [specification](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/README.md):
     exact rules, one chapter per topic, with examples;
   - the [std specification](https://github.com/HerringtonDarkholme/hd-lang/tree/main/spec/std):
     one page per std module;
   - the [std source](https://github.com/HerringtonDarkholme/hd-lang/tree/main/lib/std):
     how std itself is written, in hd;
   - [examples/](https://github.com/HerringtonDarkholme/hd-lang/tree/main/examples):
     small complete programs;
   - the [tour](https://herringtondarkholme.github.io/hd-lang/) and the
     [playground](https://herringtondarkholme.github.io/hd-lang/playground.html).

## Spec chapters by topic

| Topic | Chapter |
| --- | --- |
| tokens, indentation, literals | [01 lexical structure](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/lang/01-lexical-structure.md) |
| the full grammar | [02 grammar](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/lang/02-grammar.md) |
| names, scopes, `use`, member lookup | [03 names and scopes](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/lang/03-names-and-scopes.md) |
| types, coercions, generics | [04 type system](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/lang/04-type-system.md) |
| operators, calls, comprehensions | [05 expressions](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/lang/05-expressions.md) |
| blocks, loops, `match`, `defer`, `using` | [06 control flow](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/lang/06-control-flow.md) |
| parameters, closures, captures | [07 functions](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/lang/07-functions.md) |
| structs, enums, embedding | [08 data and enums](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/lang/08-data-and-enums.md) |
| traits, dispatch | [09 traits](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/lang/09-traits.md) |
| packages, visibility, entry points | [10 modules](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/lang/10-modules.md) |
| rows, providers, `!`, `Suspend` | [11 requirements and suspension](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/lang/11-requirements-and-suspension.md) |
| `&`, `&mut`, `box`, `*T` | [13 values and references](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/lang/13-values-and-references.md) |
| decorators, facts, derive templates, `@error` | [14 annotations](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/lang/14-annotations.md) |
| the `hd` command, capabilities | [command line](https://github.com/HerringtonDarkholme/hd-lang/blob/main/spec/cli/command-line.md) |

## When to stop

After two failed attempts at one error, and once `hd doc` and a probe
have not settled it, stop. Say what you tried, what `hd` reported, and
what you need, instead of trying a third guess.
