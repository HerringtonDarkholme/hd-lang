# Job 8 audit: spec codes with no passing fixture

> HEAD `f86395db`. Read-only; no fixtures added, none edited.

## Scope

Job 2 (at `4636f546`) found 21 Diagnostics-table codes with no fixture.
Recomputed on this tree — table codes in `spec/README.md` (216) minus
codes with a `# diagnostic:` marker or a `reject:`/`warn:` expectation
in `spec/conformance` (201) — 15 codes are still uncovered. Five codes
gained fixtures in the conformance batch-3 commit (`494f29aa`), so they
are reconciled in their own section rather than given full entries:

| code | fixture since batch 3 | `hd check` today |
| --- | --- | --- |
| `impossible-gadt-pattern` | `typing/invalid/impossible-gadt-pattern.hd`, expects `reject:impossible-gadt-pattern` | exit 1, but `unsupported-gadt-result`, not the expected code (already in `test/portable/KNOWN_FAILURES.tsv:3`) |
| `invalid-default-variant` | `typing/invalid/derived-default-no-variant.hd`, expects `reject:invalid-default-variant` | exit 0 (`<file>: ok`); the sibling `derived-default-several-variants.hd` exits 1 with `unknown-import: module 'std.ops' declares no 'default'`. Neither is in KNOWN_FAILURES or the portable manifest |
| `qualified-string-prefix` | `typing/invalid/qualified-string-prefix.hd`, `qualified-string-prefix-call.hd`, `parse/invalid/qualified-string-prefix-value.hd`, all expecting `reject:qualified-string-prefix` | exit 1 with `syntax-error: expected a member name after '.'` on all three (already in KNOWN_FAILURES.tsv:82-84) |
| `unconstrained-impl-parameter` | `typing/invalid/unconstrained-impl-parameter.hd`, expects `reject:unconstrained-impl-parameter` | exit 0 (already in KNOWN_FAILURES.tsv:55) |
| `unsigned-comparison-always` | `typing/invalid/unsigned-comparison-countdown.hd`, `unsigned-comparison-explicit.hd`, expecting `reject:unsigned-comparison-always` | exit 0 on both (already in KNOWN_FAILURES.tsv:115-116) |

The 6th emitted-but-fixtureless code from Job 2's count is one of the
above (only `unmatched-delimiter` of that group is still uncovered).

Of the 15 below, 10 are never emitted by `src/` (no `code:`/`fail(`/
`report(` site) and 5 are emitted but fixtureless. `hd check` prints
`<file>: ok` on success; warnings print as `warning: <code>: ...` with
exit 0. All probes ran as `node bin/hd.js check [--tests] FILE` (plus
`--package-tree` where noted).

## 1. `boundary-cycle` — no naming rule (README table only)

`spec/README.md:91` lists it under Boundary failure; no chapter rule
names it (the closest prose, `r[module.boundary.cycle]`, names no
code). It is a runtime encoding failure, so no static program triggers
it.

```hd
pub fn main() -> void $ Console:
    println("hi")
```

`hd check` today: exit 0 (`baseline-main.hd: ok`).

## 2. `boundary-decoder-panic` — `r[module.boundary.decoder-panic]`

`spec/lang/10-modules.md:1674`: a panicking boundary decoder makes the
adapter report a boundary failure. Runtime-only; the trigger is a
misbehaving host at run time, not a program shape. Same trivial program
as §1.

`hd check` today: exit 0.

## 3. `broken-doc-link` — `r[cli.doc.link.broken]`

`spec/cli/command-line.md:408`: a doc `` [`NAME`] `` resolving to no
item warns. `hd check` does not resolve doc links (`hd doc` does).

```hd
## See [`NoSuchItem`] for details.
pub fn main() -> void:
    pass
```

`hd check` today: exit 0 (`doclink.hd: ok`).

## 4. `confusable-identifier` — `r[lex.ident.confusable.warning]`

`spec/lang/01-lexical-structure.md:412`: warn when an identifier is
visually confusable with another in the same scope. `src/` never emits
this code.

```hd
pub fn main() -> void $ Console:
    а := 1
    a := 2
    println(а)
    println(a)
```

(`а` is Cyrillic U+0430, `a` Latin U+0061; both lex and resolve.)
`hd check` today: exit 0, no warning (`confusable.hd: ok`).

## 5. `cyclic-test-dependency` — `r[module.test.cyclic-dev-unit]`

`spec/lang/10-modules.md:309`: a dev dependency that depends back on
the package must not be used from a `tests:` block. Needs manifest
dev-dependencies, which the prototype does not read (`hd check --help`:
"a package's layout should come from its hd.toml, which the prototype
does not read yet"). Closest checkable shape:

```hd
pub fn main() -> void:
    pass

tests:
    it("works"):
        pass
```

`hd check --tests` today: exit 0 (`testsblock.hd: ok`).

## 6. `duplicate-type` — no naming rule (README meaning only)

`spec/README.md:113`: "One module declares the same type name twice."
No chapter rule names the code. `src/` does emit it:

```hd
data User:
    id: string
data User:
    id: string
```

`hd check` today: exit 1,
`dup-type.hd:3:1: duplicate-type: type 'User' is already declared`.

## 7. `duplicate-variant` — `r[data.enum.unique]`

`spec/lang/08-data-and-enums.md:578`: variant names must be unique
within the enum.

```hd
enum Color:
    Red
    Red
```

`hd check` today: exit 1,
`dup-variant.hd:3:5: duplicate-variant: variant 'Red' is declared more than once`.

## 8. `missing-entry-point` — `r[cli.exe.missing-module]`

`spec/cli/command-line.md:103`: an executable whose `module` names no
package module is an error. Needs `hd.toml` `[[executable]]`, which the
prototype does not read. Closest checkable shape is a plain entry
module:

```hd
pub fn main() -> void $ Console:
    println("hi")
```

`hd check` today: exit 0.

## 9. `mixed-script-identifier` — `r[lex.ident.mixed-script.warning]`

`spec/lang/01-lexical-structure.md:413`: warn about an identifier that
suspiciously mixes scripts. `src/` never emits this code.

```hd
pub fn main() -> void $ Console:
    аbc := 1
    println(аbc)
```

(`а` is Cyrillic U+0430, `bc` Latin; lexes as one binding, per the
`unused-local-binding` message below.) `hd check` today: exit 0 with an
unrelated warning and no mixed-script warning:

```text
mixedscript.hd:2:5: warning: unused-local-binding: local binding 'аbc' is never read
mixedscript.hd: ok
```

(A second try, `аdmin`, behaves the same.)

## 10. `nonlocal-impl` — `r[trait.own.module.error]`

`spec/lang/09-traits.md:830`: an implementation in a module that
declares neither the trait, the target's outer constructor, nor a
qualifying trait argument is an error. Reachable over `--package-tree`
(no manifest needed): trait plus target in `src/alpha.hd`, the impl in
`src/beta.hd` (linked: `use pkg.beta.{helper}` from `src/main.hd`; a
control with a broken `beta.hd` fails with `unknown-name`, proving the
file is checked).

`src/alpha.hd`:

```hd
pub trait Greeter:
    fn greet(self) -> string

pub data Person:
    name: string
```

`src/beta.hd`:

```hd
use pkg.alpha.{Greeter, Person}

pub fn helper() -> string:
    "h"

impl Greeter for Person:
    fn greet(self) -> string:
        self.name
```

`hd check --package-tree <tree> --package-path src/main.hd` today:
exit 0, no diagnostic — the ownership rule is accepted silently.

## 11. `package-cycle` — `r[module.cycle.package]`

`spec/lang/10-modules.md:580`: the package dependency graph must be
acyclic. Package dependencies come from manifests, which the prototype
does not model (`src/README.md:1085`: "Package dependencies
(`package-cycle`) are not modeled"). Trivial program checks clean;
exit 0.

## 12. `unclosed-delimiter` — general code (README meaning only)

`spec/README.md:102`: an opening delimiter with no matching close. No
chapter owns general codes.

```hd
pub fn main() -> void:
    x := (1 + 2
```

`hd check` today: exit 1,
`delim-open.hd:2:10: unclosed-delimiter: unclosed '(' delimiter`.

## 13. `unknown-variant` — general code (README meaning only)

`spec/README.md:111`: an enum variant name resolving to no variant of
the expected enum.

```hd
enum Color:
    Red
c := Color.Blue
```

`hd check` today: exit 1,
`unknown-variant.hd:3:6: unknown-variant: enum 'Color' has no variant 'Blue'`.

## 14. `unmatched-delimiter` — general code (README meaning only)

`spec/README.md:103`: a closing delimiter with no matching open, or
closing a different kind (spelling fixed in Job A).

```hd
pub fn main() -> void:
    pass
]
```

`hd check` today: exit 1,
`delim-close.hd:3:1: unmatched-delimiter: unexpected ']' delimiter`.

## 15. `unselected-main` — `r[cli.exe.unselected-main]`

`spec/cli/command-line.md:104`: a public `main` no executable names
warns. Executable selection comes from `hd.toml`, which the prototype
does not read; a lone `pub fn main` checks clean. Same program as §8.

`hd check` today: exit 0.

## Incidental finding (out of scope, not fixed)

`use pkg.beta.{}` (empty braces) crashes the compiler instead of
diagnosing: `TypeError: Cannot read properties of undefined (reading
'name')` at `src/package.ts:277` in `linkPackage`. Found while forcing
package linkage for §10. Left untouched — Job E is read-only.
