# Dependency Cycles: Stress Test Of The Folder Rule

Status: design review, 2026-09-28; changes no decision, design record, spec
text, or prototype code. The owner decided its questions on 2026-09-29, and
[Owner Decisions](#owner-decisions) lists where the specification applies
them. The rest of the report is the review as written; nothing in it is
accepted language behavior beyond those decisions.

The direction under test is
[Open Issues, Dependency Cycles](OPEN_ISSUES.md#dependency-cycles), points
1 to 5 (owner, 2026-09-28, commit 76fc1e9). This report calls them D1 to
D5. Spec sections relied on:
[Path-Inferred Modules](../spec/10-modules.md#path-inferred-modules),
[Test Modules](../spec/10-modules.md#test-modules),
[Relative Uses](../spec/10-modules.md#relative-uses),
[Use Declarations](../spec/03-names-and-scopes.md#use-declarations),
[Initialization Order](../spec/10-modules.md#initialization-order),
[Definite Initialization](../spec/10-modules.md#definite-initialization),
[Public Uses](../spec/10-modules.md#public-uses),
[Fully Annotated Declarations](../spec/10-modules.md#fully-annotated-declarations),
[Package Interfaces](../spec/10-modules.md#package-interfaces),
[Function Declarations](../spec/07-functions.md#declarations), and
[Omitted Requirement Clauses](../spec/11-requirements-and-suspension.md#omitted-requirement-clauses).
Design records relied on: [Packages](PACKAGES.md),
[Requirement Reuse RU9](REQUIREMENT_REUSE.md#owner-decisions), and
[Roadmap area 8](ROADMAP.md#8-tooling-for-agents) (the program database).

The owner's main goal is parallel compilation, and the owner worries about
a large cyclic "mud ball" becoming one compilation unit. Both are central
axes here. Problem IDs are DC-1 to DC-11; no earlier report exists.


## Owner Decisions

Decided 2026-09-29. This is option O6, with these answers:

1. **DC1: `pub` items have full signatures**, methods included. This was
   already decided earlier; the apply pass closes gap G1 by stating it for
   `pub` methods. It carries parallel compilation.
2. **DC2 (final, 2026-09-29): one merged rule, one code: the folder
   dependency graph is acyclic.** File import cycles inside one folder are
   therefore allowed, and package cycles are forbidden. The owner briefly
   revised this to a file-level-only rule (e0932d9), then went back to
   the folder rule the same day. The code name is left to the apply pass,
   for example `folder-cycle`.
3. **DC3: a file's folder is its directory,** `mod.hd` included. Nested
   folders are separate nodes.
4. **DC4: no parent/child exemption.**
5. **DC5: shared items go in a leaf folder.** The error's fix-it moves
   `x.hd` to `x/mod.hd`, which keeps the module name, so no `use` line
   changes. This is how a root facade plus a root `error.hd` is fixed, and
   it matches Go's leaf package idiom.
6. **DC6: test code's edges don't count** (`*_test.hd`, `tests/`, test
   helpers), as Go exempts `_test` packages.
7. **DC7: initialization inside a same-folder cycle uses Go's rule:**
   declarations are ordered by their dependencies across the group, then
   by file order. A true initialization cycle is an error.
8. **DC8: a `pub use` chain must end at a declaration.** This replaces the
   blanket `module.pub-use.cycles`.
9. **DC9: the diagnostic shows one shortest folder loop,** with the `use`
   line for each edge, the size of the tangle, and the fix-it.
10. **DC10: decorator facts that call other files are evaluated after
    bodies are checked,** and their values are hashed into the package
    interface.

11. **DC11 (2026-09-29): answers to Still Open 1-15.**
    - A `pub use` loop and a package-level cycle each get their own code,
      for example `re-export-loop` and `package-cycle`. The apply pass
      picks the names.
    - The multi-file rules get conformance fixtures through a
      **package-tree fixture header**, which declares several files of one
      package. It works like the package-role and `# fixture-test-layout:`
      headers.
    - The rest is kept as applied:
      - initialization order inside a group goes by dependency, then
        module identity and source position;
      - ready groups run in least-identity order;
      - a statement may run before an earlier one when that one waits on
        another file;
      - a file still can't read its own later binding;
      - a true initialization cycle reuses
        `top-level-read-before-initialization`;
      - an entry module in a group is interleaved;
      - test builds may form groups spanning folders;
      - `src/testkit/` is ordinary code;
      - the interface records the package's own facts, and a dependent may
        wait for the bodies its facts call.

**Applied 2026-09-29.** The specification now states each decision:

| Decision | Specification |
| --- | --- |
| DC1 | [`fn.decl.result-required-pub`](../spec/07-functions.md#r-fn.decl.result-required-pub), [`fn.decl.result-omitted-private`](../spec/07-functions.md#r-fn.decl.result-omitted-private), [`req.row.omitted.empty-pub`](../spec/11-requirements-and-suspension.md#r-req.row.omitted.empty-pub), and [`req.row.omitted.inferred-private`](../spec/11-requirements-and-suspension.md#r-req.row.omitted.inferred-private). |
| DC2, DC4, DC6 | [Folder Graph](../spec/10-modules.md#folder-graph): [`module.cycle.acyclic`](../spec/10-modules.md#r-module.cycle.acyclic) (new code `folder-cycle`), [`module.cycle.nested`](../spec/10-modules.md#r-module.cycle.nested), [`module.cycle.test-code`](../spec/10-modules.md#r-module.cycle.test-code), [`module.cycle.within-folder`](../spec/10-modules.md#r-module.cycle.within-folder), and [`module.cycle.package`](../spec/10-modules.md#r-module.cycle.package). In chapter 3, [`names.use.cycles-in-folder`](../spec/03-names-and-scopes.md#r-names.use.cycles-in-folder) and [`names.use.folder-cycle`](../spec/03-names-and-scopes.md#r-names.use.folder-cycle) replace `names.use.cycles`. |
| DC3 | [Folders](../spec/10-modules.md#folders). |
| DC5, DC9 | [Cycle Diagnostic](../spec/10-modules.md#cycle-diagnostic), and the leaf-folder Note under [Folder Graph](../spec/10-modules.md#folder-graph). |
| DC7 | [Initialization Order](../spec/10-modules.md#initialization-order) and [Order Inside A Group](../spec/10-modules.md#order-inside-a-group). |
| DC8 | [`module.pub-use.chain`](../spec/10-modules.md#r-module.pub-use.chain) and [`module.pub-use.chain.loop`](../spec/10-modules.md#r-module.pub-use.chain.loop) replace `module.pub-use.cycles`. |
| DC11 | [`module.pub-use.chain.loop`](../spec/10-modules.md#r-module.pub-use.chain.loop) (new code `re-export-loop`) and [`module.cycle.package`](../spec/10-modules.md#r-module.cycle.package) (new code `package-cycle`); the `# fixture-package-tree:` header in [Package Trees](../spec/conformance/README.md#package-trees), with nine fixtures for the folder rule, initialization groups, test edges, and `pub use` chains. |
| DC10 | [`module.interface.fact-values`](../spec/10-modules.md#r-module.interface.fact-values), [`module.interface.determined-facts`](../spec/10-modules.md#r-module.interface.determined-facts), [`module.interface.early-facts`](../spec/10-modules.md#r-module.interface.early-facts), and a Note under [Facts](../spec/14-annotations.md#facts). |

The prototype implements DC1 to DC6, DC8, and DC9 in its checker and
package linker. It joins an initialization group's modules by identity and
does not interleave their statements (DC7), so the fixture
`init-group-order.hd` is a known failure; the
[audit](../audit/README.md) lists the gap. The prototype CLI takes the
package-tree options; it does not model package dependencies, so it never
reports `package-cycle`.

## Still Open

Points the apply pass met (2026-09-29). Each waits for the owner; the
specification states the reading in the Applied column, so each can change
without breaking a decision.

| # | Point | Applied | **Recommendation** |
| --- | --- | --- | --- |
| 1 | DC7 says "then file order". Which order is that? | Module identity, then source position ([`module.init.group.earliest`](../spec/10-modules.md#r-module.init.group.earliest)), the order ready modules and supertrait cycles already use | **Decided (DC11):** kept as applied. |
| 2 | Which group goes first when several are ready? | The one whose least module identity is first ([`module.init.group.ready-order`](../spec/10-modules.md#r-module.init.group.ready-order)) | **Decided (DC11):** kept as applied. |
| 3 | Go's rule may run a module's statement before an earlier one of the same module, when the earlier one waits on another file. Is that intended? | Yes: each step runs the earliest ready statement ([`module.init.group.step`](../spec/10-modules.md#r-module.init.group.step)) | **Decided (DC11):** kept as applied. |
| 4 | Does the local forward-read check still apply inside a group? | Yes: [Definite Initialization](../spec/10-modules.md#definite-initialization) is unchanged, so `x := f()` before `let y` in one file stays an error even where Go would reorder | **Decided (DC11):** kept as applied. |
| 5 | Which code does a true initialization cycle get? | `top-level-read-before-initialization` ([`module.init.group.cycle`](../spec/10-modules.md#r-module.init.group.cycle)) | **Decided (DC11):** kept as applied. |
| 6 | Uses in test code make no folder edge, but they still order initialization in a test build. May a test build's group then span folders? | The group rules name no folder, so such a group follows the same order | **Decided (DC11):** kept as applied. |
| 7 | An entry module may sit in a group. Do its statements, which may use requirements, interleave with the others? | Yes: no rule singles it out | **Decided (DC11):** kept as applied. |
| 8 | DC6 names "test helpers". Which files are they? | Test code as [`module.test.code`](../spec/10-modules.md#r-module.test.code) defines it. A helper folder of ordinary code, such as `src/testkit/`, makes edges | **Decided (DC11):** kept as applied. |
| 9 | DC9's "size of the tangle": files or folders? | Folders in the loop's strongly connected component ([`module.cycle.diagnostic.size`](../spec/10-modules.md#r-module.cycle.diagnostic.size)) | **Decided (DC11):** kept as applied. |
| 10 | Which file does the fix-it move, and what if no single move breaks the loop? | Unspecified: "a file `x.hd` on the loop" ([`module.cycle.diagnostic.fix`](../spec/10-modules.md#r-module.cycle.diagnostic.fix)). The prototype picks the target of the first edge that is not a `mod.hd` | **Decided (DC11):** kept as applied. |
| 11 | Which code rejects a `pub use` loop? | `re-export-loop` ([`module.pub-use.chain.loop`](../spec/10-modules.md#r-module.pub-use.chain.loop)) | **Decided (DC11).** |
| 12 | Which code rejects a package cycle? | `package-cycle` ([`module.cycle.package`](../spec/10-modules.md#r-module.cycle.package)); no fixture, since a package tree is one package | **Decided (DC11).** |
| 13 | Which facts does a package interface record? | Each fact of its own declarations, by value ([`module.interface.fact-values`](../spec/10-modules.md#r-module.interface.fact-values)) | **Decided (DC11):** kept as applied. |
| 14 | DC10 makes an interface wait for the bodies its facts call. `module.interface.early` said a dependent never waits for bodies. | Replaced by [`module.interface.early-facts`](../spec/10-modules.md#r-module.interface.early-facts), which excepts those bodies | **Decided (DC11):** kept as applied. |
| 15 | The folder rule, group order, test edges, and `pub use` chains need several files, and the fixture format has none. | The `# fixture-package-tree: TREE/PATH` header ([Package Trees](../spec/conformance/README.md#package-trees)), with trees under `spec/conformance/trees/` | **Decided (DC11).** |

Points the DC11 apply pass met, each waiting for the owner:

| # | Point | Applied | **Recommendation** |
| --- | --- | --- | --- |
| 16 | A package tree's header value: one `TREE/PATH` string, or two headers? | One header, `# fixture-package-tree: TREE/PATH`, where `PATH` is the package path the fixture takes | Keep: one directive, as the other environments use. |
| 17 | A rule about several files, such as a folder loop, does not say which file reports it, but a line marker names one line of the fixture. | A tree case judges the code, not the line, and counts a diagnostic in any tree file, as a panic's line is not judged | Keep: it avoids specifying which line of a loop an error names. |
| 18 | `package-cycle` has no fixture: a tree holds one package, and the package-role packages cannot depend back on the fixture. | No fixture; the code is in the diagnostics table only | Add one when the manifest schema leaves tooling ([`module.manifest.tooling`](../spec/10-modules.md#r-module.manifest.tooling)). |
| 19 | A plain `use` whose name reaches a `pub use` loop: which code? | None: the loop's `pub use` lines are `re-export-loop`; the plain use is a missing declaration ([`module.use.private-or-missing`](../spec/10-modules.md#r-module.use.private-or-missing) names no code) | Leave it with the missing-declaration question. |

## Contents

- [Owner Decisions](#owner-decisions)
- [Still Open](#still-open)
1. [Surface Being Tested](#surface-being-tested)
2. [Method](#method)
3. [Summary](#summary)
4. [Parallel Compilation](#parallel-compilation)
5. [Cases](#cases) (C1-C16)
6. [Probes](#probes)
7. [Comparison With Other Languages](#comparison-with-other-languages)
8. [Problems, Ranked](#problems-ranked)
9. [Options, Ranked By Cost Order](#options-ranked-by-cost-order)
10. [Recommendation](#recommendation)
11. [Questions For The Owner](#questions-for-the-owner)
12. [Parse Log](#parse-log)
13. [Sources](#sources)

## Surface Being Tested

### The Owner Direction

| Id | Direction, as written | As this report reads it |
| --- | --- | --- |
| D1 | Package dependency cycles are forbidden. | Already in the draft resolver ([Packages 4.1](PACKAGES.md#41-algorithm), step 5). |
| D2 | A module stays one file with explicit `use`. A file import cycle is allowed only when every file in it is in one folder. A strongly connected component (SCC) spanning folders is an error that prints the loop. | The file `use` graph may have cycles. Each SCC must lie inside one folder. |
| D3 | The folder-level dependency graph must be acyclic. | A folder has an edge to another folder when a file in the first uses a module whose file is in the second. This graph must have no cycle. |
| D4 | Mutually recursive declarations inside one file stay allowed. Cyclic run-time values are unaffected. | No change to today's rules. |
| D5 | Shared items that children need go in a leaf such as `common.hd` or `types.hd`, which children import instead of their facade. | Guidance, not a rule. |

### What Today's Spec Allows

Today's spec is stricter than the direction. It forbids every `use` cycle
between files.

| Topic | Today's rule | What it says |
| --- | --- | --- |
| Module per file | [`module.path.name`](../spec/10-modules.md#r-module.path.name) | A file's path under the source root is its module name: `src/user/types.hd` is `user.types`. |
| Directory module | [`module.path.directory`](../spec/10-modules.md#r-module.path.directory) | A directory is a module only with a `mod.hd`, which is its facade. |
| No implicit nesting | [`module.path.no-child-import`](../spec/10-modules.md#r-module.path.no-child-import), [`module.path.no-parent-scope`](../spec/10-modules.md#r-module.path.no-parent-scope) | A parent does not import its children, and a child does not see its parent. |
| Relative uses | [`module.relative.base`](../spec/10-modules.md#r-module.relative.base) | `self` and `super` start at the file's directory module. |
| Use cycles | [`names.use.cycles`](../spec/STYLE.md#retired-rule-ids), [`module.pub-use.cycles`](../spec/STYLE.md#retired-rule-ids) | "Cycles involving `use` or `pub use` are rejected." No diagnostic code is named. |
| Init graph | [`module.init.graph`](../spec/STYLE.md#retired-rule-ids) | The compiler resolves an *acyclic* use graph from the entry. |
| Init order | [`module.init.once`](../spec/STYLE.md#retired-rule-ids), [`module.init.ready-order`](../spec/STYLE.md#retired-rule-ids) | A module initializes after every module it uses. Ties go by module identity. |
| Definite init | [`module.init.definite.local`](../spec/10-modules.md#r-module.init.definite.local) | The check is local to one module; it never needs another module's bodies. |
| No public bindings | [`module.package.no-pub-binding`](../spec/10-modules.md#r-module.package.no-pub-binding) | Top-level bindings cannot be `pub`. |
| Visibility | [`module.vis.no-package-private`](../spec/10-modules.md#r-module.vis.no-package-private) | Private means file-private. Another file sees only `pub`. |
| Public signatures | [`module.package.annotated`](../spec/10-modules.md#r-module.package.annotated), [`fn.decl.result-required`](../spec/STYLE.md#retired-rule-ids), [`req.row.omitted.empty`](../spec/STYLE.md#retired-rule-ids) | Every `pub` declaration is fully annotated; a `pub` function without a row clause has the empty row. |
| Private inference | [`fn.decl.result-omitted`](../spec/STYLE.md#retired-rule-ids), [`req.row.omitted.inferred`](../spec/STYLE.md#retired-rule-ids) | Only a non-public function may omit its result type and row. |
| Tests | [`module.test.module`](../spec/10-modules.md#r-module.test.module), [`module.test.integration`](../spec/10-modules.md#r-module.test.integration) | `*_test.hd` files sit beside code; `tests/` holds integration tests that see only the public API. |
| Entry | [Packages, manifest](PACKAGES.md#26-entry-points) | The default entry is `src/main.hd`; the library root is `src/mod.hd`. Both sit in the root folder. |

So D2 relaxes today's spec, and D3 adds a new restriction on top. D1 and
D4 change nothing.

### Assumptions

The direction leaves these open. Each case says which it uses.

| Id | Assumption | Why |
| --- | --- | --- |
| A1 | A file's folder is the directory that holds it. `src/shop/mod.hd` is in folder `shop/`, like its siblings. | `mod.hd`'s relative base is its own directory ([`module.relative.base`](../spec/10-modules.md#r-module.relative.base)). |
| A2 | Nested folders are distinct nodes. `shop/` and `shop/orders/` are different, and a parent has no implicit edge to its children. | Matches [`module.path.no-child-import`](../spec/10-modules.md#r-module.path.no-child-import). Called the literal reading. |
| A3 | Every `use` and `pub use` adds a folder edge, whether it names a module or a declaration. | D3 says "uses" with no exception. |
| A4 | Test code's edges count unless a case says otherwise. | D3 names no exception. C12 tests both readings. |
| A5 | Edges into `std` or a dependency are not folder edges. The package graph (D1) covers them. | Folders are within one package. |
| A6 | Every public declaration is fully annotated, as today. | [`module.package.annotated`](../spec/10-modules.md#r-module.package.annotated). |

## Method

**Cases.** Sixteen cases (C1-C16) translate real project layouts. They
approximate the shapes of Rust crates (regex-syntax, a `mod.rs` tree), Go
packages (net/http, client-go), a TypeScript app, a NestJS and TypeORM
service, a compiler front end, and Go's `database/sql` driver registry.
Operation cases cover tests, entry modules, initialization, re-exports,
file moves, and packages. Library shapes are approximations, not copies.

**Columns.** Each case gets three verdicts:

| Column | Rules applied |
| --- | --- |
| Today | Today's spec: no `use` cycle at all. |
| Direction | D1 to D5 as written, under A1 to A6. |
| Amended | The combination in [Recommendation](#recommendation): explicit signatures at file boundaries, D2 and D3 as one rule, and the fixes for DC-1 to DC-8. |

**Verdicts.** *Works*: the layout compiles unchanged. *Friction*: it
compiles after a mechanical change, such as moving one file, that keeps
behavior. *Breaks*: the layout is rejected and the fix changes the design,
or the rules give no answer. *Breaks (intended)* marks a rejection that is
the rule's purpose.

**Limits.** Parsing checks syntax only. No block here is claimed to type
check. Multi-file examples show one file per block, headed by a path
comment. Directory trees and diagnostics are in `console` fences and are
not hd code.

## Summary

| Case | Approximates | Today | Direction | Amended | At fault |
| --- | --- | --- | --- | --- | --- |
| C1 | regex-syntax `crate::error` | works | breaks | friction | D3 on the root folder, D5 |
| C2 | Rust `mod.rs` facade, nested child | works | breaks | friction | D3, D5 names a leaf file |
| C3 | AST: `Expr` and `Stmt` files | breaks | works | works | today's `names.use.cycles` |
| C4 | AST split into sub-folders | breaks | breaks | breaks (intended) | D2 |
| C5 | Pretty printer and `Display` | works | works | works | none |
| C6 | Go net/http, one folder | breaks | friction | friction | file-private visibility |
| C7 | Kubernetes client-go layers | friction | works | works | none |
| C8 | TypeScript models and services | breaks | breaks | breaks (intended) | D2 |
| C9 | NestJS feature folders with entities | breaks | breaks | friction | D2, D3 |
| C10 | Go `database/sql` drivers | works | breaks | friction | D3 |
| C11 | Entry `main.hd` and `config.hd` | works | breaks | friction | D3 on the root folder |
| C12 | Test helpers folder | works | breaks | works | A4, D3 |
| C13 | Initialization in a cycle | n/a | breaks | works | `module.init.graph` |
| C14 | Facade re-export loops | breaks | friction | works | `module.pub-use.cycles` |
| C15 | Agent moves a file | works | friction | friction | diagnostics |
| C16 | Folder extracted to a package | friction | works | works | none |

**Conclusions.** The direction holds up, but not for the stated reason.
Parallel compilation does not need either cycle rule, because hd already
requires full signatures on everything visible outside a file. What D2
buys is the worst-case bound: no SCC is larger than one folder. What D3
adds is folder layering, folder-sized build units, and simple
initialization across folders. D3's cost falls on file graphs that have no
cycle at all but zigzag between folders (C1, C2, C10, C11). A leaf *folder*,
not a leaf file, fixes each, and moving `x.hd` to `x/mod.hd` keeps every
`use` line valid. Three gaps need rules before either rule can ship:
initialization inside a cycle (DC-3), test edges (DC-4), and re-export
loops (DC-8).

## Parallel Compilation

### What Blocks Parallel Checking

A compiler can check two function bodies at the same time when each needs
only the other's signature. Parallelism is lost when checking one file
needs the *body* of a function in another file. That happens in three
ways:

1. A signature is inferred from a body, so the caller must wait for the
   callee's body. Kotlin's implicit public return types, Swift's
   unannotated properties, and TypeScript's inferred exports do this.
2. A compile-time evaluation calls another file's function.
3. A whole-program property, such as initialization order, needs body
   facts from several files at once.

Import cycles are not on this list. Java and Rust check arbitrary cycles
within a unit. In Java, files in a package "can refer to each other,
circularly". The compiler "must arrange to compile all such classes and
interfaces at the same time"
([JLS 7.6](https://docs.oracle.com/javase/specs/jls/se21/html/jls-7.html#jls-7.6)).
rustc checks a whole crate, cycles included, and parallelizes inside it
with its parallel front end and codegen units
([Rust blog, 2023](https://blog.rust-lang.org/2023/11/09/parallel-rustc.html);
[Cargo, `codegen-units`](https://doc.rust-lang.org/cargo/reference/profiles.html#codegen-units)).

### hd Already Has The Signature Lever

In hd, private means file-private
([`module.vis.no-package-private`](../spec/10-modules.md#r-module.vis.no-package-private)).
Only a non-public function may omit its result type or row
([`fn.decl.result-omitted`](../spec/STYLE.md#retired-rule-ids)).
Every `pub` declaration is fully annotated
([`module.package.annotated`](../spec/10-modules.md#r-module.package.annotated)),
and a `pub` function without a row clause has the empty row (RU9). Top-level
bindings cannot be `pub`.

So every edge between two files already goes through a written signature.
This is the "explicit signatures for anything visible outside its file"
lever, and hd has it today. TypeScript added the same lever as
`--isolatedDeclarations` so declaration files can be emitted per file in
parallel
([TypeScript 5.5](https://devblogs.microsoft.com/typescript/announcing-typescript-5-5/#isolated-declarations)).
OCaml gets it from `.mli` files, and Go gets it from export data.

Under this lever, a checker can run these phases whatever the cycle shape:

| Phase | Input it needs | Effect of a file cycle |
| --- | --- | --- |
| 1. Parse | one file | None. Parallel per file. |
| 2. Build the use graph and its SCCs | every file's `use` lines | None. Tarjan's algorithm is linear. |
| 3. Collect declarations and impl heads | one file's items | None. Parallel per file. |
| 4. Resolve signatures | names from used files | The files of one SCC resolve together. No bodies are read, so this is cheap. |
| 5. Check bodies | callee signatures only | None. Parallel per function. |
| 6. Infer private results and rows | same-file bodies | None. Inference never leaves a file. |
| 7. Generate code | one function, plus pack and reified bodies for specialization | None. Parallel per function, as in Go since 1.9. |

Go 1.9 added "compiling a package's functions in parallel", on top of
building separate packages in parallel
([Go 1.9 release notes](https://go.dev/doc/go1.9)).
Kotlin 1.6.20 added `-Xbackend-threads` to compile the files of one module
in parallel ([What's new in Kotlin 1.6.20](https://kotlinlang.org/docs/whatsnew1620.html)).
Neither needs an acyclic file graph inside the unit.

### Gaps In The Signature Lever

Three places still let a body cross a file boundary.

| Id | Gap | Where | Effect |
| --- | --- | --- | --- |
| G1 | A `pub` inherent method may omit its result type and row. | [`fn.decl.result-required`](../spec/STYLE.md#retired-rule-ids) lists "a public function, a trait method, and a method of a trait implementation". [`fn.decl.result-omitted`](../spec/STYLE.md#retired-rule-ids) allows "a non-public function, inherent method, or local `fn`". Whether "non-public" covers "inherent method" is unclear, and no fixture pins it. | If allowed, `cart.total()` in another file needs `total`'s body. Checking then waits across files, like Kotlin's implicit types. |
| G2 | A decorator's fact expression is evaluated at compile time and may call a function. | [`annot.fact.eval`](../spec/14-annotations.md#r-annot.fact.eval) | A fact in one file can depend on a body in another. Package interfaces carry facts, which conflicts with [`module.interface.determined`](../spec/STYLE.md#retired-rule-ids). |
| G3 | Initialization order inside a file cycle. | [`module.init.graph`](../spec/STYLE.md#retired-rule-ids) assumes no cycle. | Ordering needs read sets from several files' bodies (C13). |

G1 is the only one that matters for checking speed. G2 and G3 are ordering
passes over summaries, run after bodies are checked.

### What A Large Cycle Hurts

A "mud ball" is one SCC holding hundreds of files. The table assumes the
signature lever holds (A6) and G1 is closed.

| Concern | Harm from a large SCC in hd | Why |
| --- | --- | --- |
| Parallel body checking | Little | Phase 5 needs signatures only. |
| Signature resolution | A little | Phase 4 handles the SCC together, but reads no bodies. |
| Incremental rebuild | Bad, when the build unit is the SCC | A signature change rebuilds every file that must be compiled with it. Go, Java, and Swift rebuild the whole unit; a fine-grained query engine rechecks only direct users. |
| Separate compilation and caching | Bad | An SCC cannot be split into cached actions; one action compiles it (JLS 7.6). |
| Inference across files | Bad in general, none in hd | Kotlin and Swift must interleave body checks across a cycle. hd keeps inference inside a file. |
| Initialization order | Bad | Order inside an SCC needs cross-file analysis (C13). |
| Comprehension | Bad | Nothing in the cycle can be read, tested, or moved alone. |

### How Big Cycles Get In Practice

Large cycles are the normal state of code without a rule.

- Melton and Tempero studied cycles among classes in 78 Java
  applications. Among applications large enough, "about 45% have a cycle
  involving at least 100 classes", and about 10% one of at least 1,000
  classes. They also found the cycles are not caused by the domain
  ([Empirical Software Engineering 12(4), 2007, pp. 389-415](https://link.springer.com/article/10.1007/s10664-006-9033-1)).
- Al-Mutawa studied 103 open-source Java systems in the Qualitas Corpus.
  Package cycles "tend to form in branches of the package containment tree
  around parent packages"
  ([MSc thesis, Massey University, 2013](https://mro.massey.ac.nz/server/api/core/bitstreams/37d0316d-5318-488f-b89e-bf8619455c15/content);
  also [Al-Mutawa, Dietrich, Marsland, McCartin, ASWEC 2014](https://ieeexplore.ieee.org/document/6824106/)).

The second finding matters for C2. The most common package cycle is a
parent with its child, which D3 forbids under A2.

### What Acyclic Folders Add

Given the signature lever, D2 and D3 add these, and only these:

| Benefit | From D2 | From D3 |
| --- | --- | --- |
| Worst-case bound: no SCC exceeds one folder | yes | yes (implied) |
| A folder compiles from its dependencies' signatures only, as Go compiles a package from export data | no | yes |
| A folder is a cacheable build unit | no | yes |
| Initialization order across folders needs no body facts | no | yes |
| Folder layering a reader can trust | no | yes |
| A folder can become a package without a package cycle (D1) | no | yes |

Go's export data is "a serialized description of the API of a package",
which importers read instead of the source
([gcexportdata](https://pkg.go.dev/golang.org/x/tools/go/gcexportdata)).
D3 gives each hd folder the same property. D2 alone would allow folder `a/`
to use `b/` and `b/` to use `a/` through different files. Then neither
folder's signatures are complete before the other's.

### The Two Levers Compared

| Lever | Parallel checking | Bounded SCC | Incremental unit | Layering | Cost |
| --- | --- | --- | --- | --- | --- |
| Explicit signatures at file boundaries | yes | no | per file, with early cutoff | no | Already in the spec; close G1 |
| Folder rule (D2 and D3) | no effect | yes | per folder | yes | New rule and code; C1, C2, C10, C11 need a move |
| Both | yes | yes | per file inside, per folder outside | yes | Both |

The combination is the only row that answers all three of the owner's
worries. Without the signature lever, the folder rule would not save
parallel checking, because an inferred public result would still serialize
the files of one folder. Without the folder rule, the signature lever
keeps checking parallel but leaves the mud ball unbounded.

## Cases

### C1. A Crate-Root Error Type

Approximates regex-syntax, where `src/error.rs` defines `Error` and
submodules use `crate::error::Error`. Shape only, not a copy.

```rust
// src/lib.rs
pub use crate::error::Error;
pub mod error;
pub mod parser;
// src/parser/parse.rs
use crate::error::Error;
```

```text
# src/mod.hd: the library facade
pub use pkg.error.{Error}
pub use pkg.parser.parse.{parse}
```

```text
# src/error.hd
pub enum Error:
    Syntax(offset: i32, message: string)
    TooBig(limit: i32)
```

```text
# src/parser/parse.hd
use pkg.error.{Error}

pub fn parse(pattern: string) -> Result[i32, Error]:
    if pattern == "": .Err(Error.TooBig(0))
    else: .Ok(1)
```

The file graph is `mod -> error`, `mod -> parser.parse -> error`. It has no
cycle, so today's spec accepts it. Folder `src/` uses `src/parser/` (the
facade), and `src/parser/` uses `src/` (`error.hd`). D3 rejects the folder
cycle.

D5 says to put `Error` in a leaf file. It is already in a leaf file. The
leaf is in the wrong *folder*. The fix is to move `src/error.hd` to
`src/error/mod.hd`. The module identity stays `error`
([`module.path.directory`](../spec/10-modules.md#r-module.path.directory)),
so no `use` line changes.

```console
src/mod.hd            # uses error, parser.parse
src/error/mod.hd      # was src/error.hd; still module `error`
src/parser/parse.hd   # uses error
```

Verdict: today works, direction breaks, amended friction (one move).

### C2. A Nested Facade With A Shared Sibling

Approximates a Rust tree where a child uses `super::common`. The facade
`shop/mod.hd` re-exports from its child folder.

```rust
// src/shop/mod.rs
mod common;
pub mod orders;
pub use orders::Order;
// src/shop/orders/order.rs
use super::super::common::Money;
```

```text
# src/shop/mod.hd
pub use pkg.shop.orders.order.{Order}
```

```text
# src/shop/common.hd
pub type Money(i64)
```

```text
# src/shop/orders/order.hd
use super.common.{Money}

pub data Order:
    pub total: Money
```

This follows D5 exactly: the child uses the leaf `common.hd`, not the
facade. The file graph is a chain with no cycle. The folder graph still
has `shop/ -> shop/orders/` (from `mod.hd`) and `shop/orders/ -> shop/`
(from `order.hd`). D3 rejects it.

So D5's advice does not work for nested folders. A leaf file in the
facade's folder keeps that folder in the loop. The fix again is a leaf
folder: `src/shop/common.hd` moves to `src/shop/common/mod.hd`, and the
module stays `shop.common`. Rust accepts the original, because modules in
one crate may form any graph.

Verdict: today works, direction breaks, amended friction.

### C3. AST Types In Two Files

A compiler keeps `Expr` and `Stmt` in separate files. A block expression
holds statements, and a statement holds expressions.

```text
# src/ast/expr.hd
use self.stmt.{Stmt}

pub enum Expr:
    Number(i64)
    Block(List[Stmt])
```

```text
# src/ast/stmt.hd
use self.expr.{Expr}

pub enum Stmt:
    Let(name: string, value: Expr)
    Eval(Expr)
```

Today this is a `use` cycle and is rejected, so both types must share one
file. D2 allows it, since both files are in `ast/`. OCaml has the same
limit as today's hd and requires one file with `type ... and ...`; F#
requires a `rec` namespace or module in one file
([FS-1009](https://github.com/fsharp/fslang-design/blob/main/FSharp-4.1/FS-1009-mutually-referential-types-and-modules-single-scope.md)).

Verdict: today breaks, direction works, amended works. This is D2's main
gain.

### C4. AST Split Into Sub-Folders

The same types, split as `ast/expr/expr.hd` and `ast/stmt/stmt.hd`. The
file cycle spans two folders, so D2 rejects it, and D3 rejects the folder
cycle. The fix is C3's layout. This is the bound the owner wants: mutual
recursion is kept inside one folder.

Verdict: breaks under both today and the direction. Amended: breaks
(intended).

### C5. A Pretty Printer And `Display`

A diagnostics folder renders AST nodes. `Expr` wants `Display`, and the
renderer needs `Expr`.

```text
# src/diagnostics/render.hd
use pkg.ast.expr.{Expr}

pub fn render(expr: Expr) -> string:
    match expr:
        .Number(n) => "$n"
        .Block(items) => "{...}"

impl Display for Expr:
    fn to_string(self) -> string:
        render(self)
```

The implementation sits in `diagnostics/`, not `ast/`. Ownership is
checked per package
([`trait.own.rule`](../spec/09-traits.md#r-trait.own.rule)), so any file of
the package may hold it. `ast/` never names `diagnostics/`, so there is no
cycle. Rust needs the impl beside the type or the trait's crate; hd's
package-level ownership gives more room to break cycles.

Verdict: works in all three columns.

### C6. Go net/http: One Large Folder

Go's `net/http` is one package of many files. `client.go`, `transport.go`,
`request.go`, and `server.go` refer to one another freely. Sub-packages
`httptrace` and `internal` are used by `http`, and `httputil` uses `http`.

```go
// net/http/transport.go
func (t *Transport) roundTrip(req *Request) (*Response, error)
// net/http/client.go
func send(req *Request, rt RoundTripper, deadline time.Time) (*Response, ...)
```

```text
# src/http/client.hd
use self.request.{Request}
use self.transport.{Response}

pub fn send(request: Request) -> Response:
    Response { status: 200 }
```

```text
# src/http/transport.hd
use self.client.{send}
use self.request.{Request}

pub data Response:
    pub status: i32

pub fn retry(request: Request) -> Response:
    send(request)
```

The file cycle `client <-> transport` is inside `http/`, so D2 accepts it;
today's spec rejects it. The folder graph matches Go's package graph,
which Go already keeps acyclic, so D3 costs nothing here.

Friction remains in visibility. Go's lowercase names are package-private,
so `send` stays internal. In hd, `send` must be `pub` to cross files, and
`pub` means visible to the whole package and beyond. The unit that may be
cyclic (the folder) is not a unit of visibility (the file). This is noted,
not proposed: `module.vis.no-package-private` is a recorded rule.

Verdict: today breaks, direction friction, amended friction.

### C7. Kubernetes client-go Layers

client-go's `kubernetes/typed/core/v1` uses `rest` and the API types in
`k8s.io/api/core/v1`. `rest` never uses the typed clients. Every Go
package graph is acyclic by the Go spec
([Import declarations](https://go.dev/ref/spec#Import_declarations)).

```text
# src/typed/core/pods.hd
use pkg.rest.client.{RestClient}
use pkg.api.core.pod.{Pod}

pub fn get_pod(client: RestClient, name: string) -> Pod:
    client.get_pod(name)
```

A Go module translates one package per folder, and D3 holds by
construction. Inside a package, files refer to each other, which today's
spec rejects and D2 allows.

Verdict: today friction (files inside a package must be merged or
reordered), direction works, amended works.

### C8. TypeScript Models And Services

An active-record style app: `models/user.ts` calls `UserService.save`,
and `services/user_service.ts` takes a `User`. TypeScript accepts the
cycle, and ESLint's
[`import/no-cycle`](https://github.com/import-js/eslint-plugin-import/blob/main/docs/rules/no-cycle.md)
exists to reject it.

```ts
// models/user.ts
import { UserService } from "../services/user_service";
export class User { save() { UserService.save(this); } }
// services/user_service.ts
import { User } from "../models/user";
export class UserService { static save(user: User) { /* ... */ } }
```

```text
# src/models/user.hd
use pkg.services.user_service.{save_user}

pub data User:
    pub name: string

impl User:
    pub fn save(self) -> void:
        save_user(self)
```

```text
# src/services/user_service.hd
use pkg.models.user.{User}

pub fn save_user(user: User) -> void:
    pass
```

The file cycle spans `models/` and `services/`. D2 rejects it, and the
folder cycle alone would also trip D3. The fix moves `save` out of
`User`: callers write `save_user(user)`. This is the kind of loop the
rule exists to stop.

Verdict: breaks in all columns, intended.

### C9. NestJS Feature Folders With Entities

NestJS recommends one folder per feature: `users/`, `orders/`,
`products/`, each with an entity, a service, and a controller. TypeORM
entities refer to each other: `User` has `orders`, and `Order` has `user`.
NestJS offers `forwardRef()` for the resulting module cycles
([NestJS, circular dependency](https://docs.nestjs.com/fundamentals/circular-dependency)).

```text
# src/users/user.hd
use pkg.orders.order.{Order}

pub data User:
    pub id: i64
    pub orders: List[Order]
```

```text
# src/orders/order.hd
use pkg.users.user.{User}

pub data Order:
    pub id: i64
    pub buyer: User
```

The type cycle spans two folders, so D2 rejects it. The working layout is
layer-first for the entities: `model/user.hd`, `model/order.hd`,
`model/product.hd` in one folder, then feature folders for services that
use `model/`. That is TypeORM's default `entity/` folder, so the change is
a known layout. It still moves every entity when a team is used to
feature folders.

Verdict: today breaks (even in one folder), direction breaks, amended
friction (entities move into one folder).

### C10. Go `database/sql` Drivers And A Registry

Go's `database/sql` keeps a driver registry; a driver calls
`sql.Register` from its `init`, and the program imports the driver for
its side effect
([`sql.Register`](https://pkg.go.dev/database/sql#Register)). A naive hd
port puts the list of plugins in the core.

```text
# src/core/plugin.hd
pub trait Plugin:
    fn name(self) -> string
```

```text
# src/core/registry.hd
use pkg.core.plugin.{Plugin}
use pkg.plugins.json.{JsonPlugin}

pub fn default_plugins() -> List[Plugin]:
    [JsonPlugin {}]
```

```text
# src/plugins/json.hd
use pkg.core.plugin.{Plugin}

pub data JsonPlugin: pass

impl Plugin for JsonPlugin:
    fn name(self) -> string:
        "json"
```

The file graph has no cycle: `registry -> json -> plugin`. The folder
graph has `core/ <-> plugins/`, so D3 rejects it. The fix moves
`default_plugins` to the entry module or to an `app/` folder that uses
both. That is dependency inversion, and it is what Go's registry does.

```text
# src/main.hd
use pkg.core.plugin.{Plugin}
use pkg.plugins.json.{JsonPlugin}

fn plugins() -> List[Plugin]:
    [JsonPlugin {}]

pub fn main() -> void:
    for plugin in plugins():
        print(plugin.name())
```

Verdict: today works, direction breaks, amended friction.

### C11. Entry Module Beside A Shared File

The default layout puts `src/main.hd` in the root folder
([Packages](PACKAGES.md#26-entry-points)). A `config.hd` next to it is
used by `cli/`.

```text
# src/main.hd
use pkg.cli.args.{parse_args}

pub fn main() -> void:
    parse_args()
```

```text
# src/config.hd
pub data Config:
    pub verbose: bool
```

```text
# src/cli/args.hd
use pkg.config.{Config}

pub fn parse_args() -> Config:
    Config { verbose: false }
```

The file graph has no cycle; the folder graph has `src/ <-> src/cli/`. The
entry module is a sink, but its folder is not: any root-level file that a
child uses closes the loop. The fix is C1's move, `src/config.hd` to
`src/config/mod.hd`. Go avoids this by putting `main` packages in their
own `cmd/<name>/` folders.

Verdict: today works, direction breaks, amended friction.

### C12. A Test-Helpers Folder

A `testkit/` folder builds domain values for tests in many folders. A
test module beside the code uses it.

```text
# src/testkit/builders.hd
use pkg.shop.cart.{Cart}

pub fn empty_cart() -> Cart:
    Cart { items: [] }
```

```text
# src/shop/cart_test.hd
use std.testing.assert_equal
use pkg.testkit.builders.{empty_cart}

it("starts empty"):
    assert_equal(empty_cart().items.len(), 0, reason="new cart")
```

The file graph has no cycle. If test edges count (A4), the folder graph
has `shop/ -> testkit/` and `testkit/ -> shop/`, and D3 rejects a common
layout. Go solves this with external test packages. A file in
`package foo_test` is "compiled as a separate package", so it may import
packages that import `foo`
([go test packages](https://pkg.go.dev/cmd/go#hdr-Test_packages)).

If edges that start in test code are left out of the folder graph, the
layout works. Test code never runs in a normal build
([`module.test.code`](../spec/10-modules.md#r-module.test.code)), so its
edges add no build or initialization order. Integration tests under
`tests/` only use `pkg`, and non-test code cannot use them
([`module.test.tests-root-elsewhere`](../spec/10-modules.md#r-module.test.tests-root-elsewhere)),
so they never close a loop.

Verdict: today works, direction breaks, amended works (test edges
excluded).

### C13. Initialization Inside A File Cycle

Two files in `shop/` use each other. One has a top-level binding whose
initializer calls into the other.

```text
# src/shop/prices.hd
use self.catalog

let markup = 5

pub fn price_of(sku: string) -> i32:
    catalog.base_price(sku) + markup
```

```text
# src/shop/catalog.hd
use self.prices

let featured = prices.price_of("tea")

pub fn base_price(sku: string) -> i32:
    10
```

[`module.init.once`](../spec/STYLE.md#retired-rule-ids) says a module
initializes after the modules it uses. In a cycle, no module is ready
first. If `catalog` goes first, `featured` reads `markup` before it is
set. [`module.init.definite.local`](../spec/10-modules.md#r-module.init.definite.local)
cannot see this, because the read happens in another file.

Go orders package-level variables across all files of a package by their
dependencies, looking through function bodies, and rejects an
initialization cycle
([Package initialization](https://go.dev/ref/spec#Package_initialization)).
hd could do the same over each SCC, using the per-function read summaries
the definite-initialization note already describes.

Verdict: today n/a (cycles rejected), direction breaks (no rule), amended
works with an SCC-wide order rule (DC-3).

### C14. Facade Re-Export Loops

A facade re-exports its children, and a child uses the facade.

```text
# src/shop/mod.hd
pub use pkg.shop.cart.{Cart}
pub use pkg.shop.item.{Item}
```

```text
# src/shop/cart.hd
use pkg.shop.{Item}

pub data Cart:
    pub items: List[Item]
```

This is a file cycle `mod <-> cart` inside `shop/`. Today's spec rejects
it; D2 accepts it, and D5's advice becomes unnecessary within one folder.
A different loop must stay an error: two `pub use` lines that forward a
name to each other.

```text
# src/shop/a.hd
pub use pkg.shop.b.{Token}
```

```text
# src/shop/b.hd
pub use pkg.shop.a.{Token}
```

Neither names a declaration. Today this falls under
[`module.pub-use.cycles`](../spec/STYLE.md#retired-rule-ids),
which D2 replaces. A narrower rule is needed: a `pub use` chain must end
at a declaration.

Verdict: today breaks (first example), direction friction (the second
needs a new rule), amended works.

### C15. An Agent Moves A File

An agent moves `src/billing/refund.hd` into `src/shop/` to sit beside
`order.hd`. The file uses `pkg.billing.ledger`, and `billing/invoice.hd`
already uses `pkg.shop.order`. The move closes the loop
`shop/ -> billing/ -> shop/`, through two files the agent did not touch.

The module identity changes with the path
([`module.path.name`](../spec/10-modules.md#r-module.path.name)), so every
user of `billing.refund` is rewritten anyway. The folder check runs in
phase 2, before type checking, so the error arrives even when the program
does not yet type-check. A good diagnostic names one edge per step:

```console
error[folder-cycle]: folders shop/ and billing/ depend on each other
  shop/ -> billing/   src/shop/refund.hd:1       use pkg.billing.ledger
  billing/ -> shop/   src/billing/invoice.hd:2   use pkg.shop.order
  help: move src/shop/refund.hd back to billing/,
        or move the shared declarations into a folder that uses neither
```

The code name `folder-cycle` is a placeholder, not a decision. Go prints
the import stack for "import cycle not allowed". dune prints
"Dependency cycle between" and the modules.

Verdict: today works (a file move alone rarely makes a file cycle),
direction friction, amended friction.

### C16. A Folder Becomes A Package

A monorepo splits `src/billing/` into its own package, ahead of the
Go-style repository dependencies planned in
[Packages](PACKAGES.md). D1 forbids a package cycle. Under D3, a folder
with its sub-folders never uses a folder that uses it back. So extracting
any folder subtree that its parents do not use keeps the package graph
acyclic. Today, only the file graph is acyclic, and a folder pair
`a/ <-> b/` would become a package cycle on extraction.

Verdict: today friction, direction works, amended works.

## Probes

| Probe | Finding |
| --- | --- |
| Nested folders | Under A2, `shop/` and `shop/orders/` are separate nodes with no implicit edge. The facade `shop/mod.hd` re-exporting a child adds `shop/ -> shop/orders/`. Any child use of a file in `shop/` then closes a loop (C2). An "ancestors don't count" reading would allow C2, but a whole subtree could then form one SCC, which defeats the bound. |
| Facades | Within one folder, D2 allows the facade loop (C14). Across nested folders, D3 forbids it (C2). Re-export chains need their own rule (C14). |
| `src/shop.hd` beside `src/shop/` | Without `shop/mod.hd`, module `shop` is a file in `src/`, while `shop.cart` is in `shop/`. They are different folders even though the names nest. A1 handles it, but the spec should say so. |
| Test files | `*_test.hd` sit in the folder they test. Their edges out, to a helper folder, close loops (C12). `tests/` is safe. |
| Entry scripts | The entry module is a sink. Its *folder* is not, when it holds shared files (C11). |
| Diagnostics | D2 and D3 give two errors for one fact: a file cycle spanning folders always implies a folder cycle, because its projection is a closed walk over at least two folders. One code suffices (C15). |
| Incremental compilation | The folder check is linear in the use graph and needs only `use` lines. A body edit never changes it. A `use` edit reruns it cheaply. |
| Program database | The check is a recursive query over a `use` edge fact, so agents can ask for the loop before editing. Parse-level facts suffice, so a partial database still answers ([Roadmap area 8](ROADMAP.md#8-tooling-for-agents)). |
| Agent moves | A move can create a loop far from the moved file (C15). The diagnostic must name edges and suggest the leaf-folder move. |
| Monorepo and packages | D3 makes a folder subtree extractable into a package (C16), which fits D1. |

## Comparison With Other Languages

| Language or tool | Unit of cycle freedom | Rule | hd equivalent |
| --- | --- | --- | --- |
| Go | package = directory | Import cycles between packages are illegal ([spec](https://go.dev/ref/spec#Import_declarations)). Files of one package see each other freely. `_test` packages are exempt ([go test](https://pkg.go.dev/cmd/go#hdr-Test_packages)). | D2 and D3, with files as separate namespaces. |
| OCaml, dune | file (compilation unit) | No cycle between files; `module rec` only inside one file ([manual](https://ocaml.org/manual/5.3/recursivemodules.html); [Real World OCaml](https://dev.realworldocaml.org/files-modules-and-programs.html)). `.mli` files let users compile against interfaces. | Today's spec. |
| F# | file, in project order | A file sees only earlier files; `namespace rec` and `module rec` allow recursion inside one file ([FS-1009](https://github.com/fsharp/fslang-design/blob/main/FSharp-4.1/FS-1009-mutually-referential-types-and-modules-single-scope.md)). | Stricter than today's spec. |
| Haskell (GHC) | module | Cycles need an `hs-boot` interface file and a `SOURCE` import ([GHC guide](https://downloads.haskell.org/ghc/latest/docs/users_guide/separate_compilation.html#how-to-compile-mutually-recursive-modules)). | A hand-written signature file; hd needs none because `pub` is fully annotated. |
| Rust | crate | Modules in a crate may form any graph; crates must not (Cargo rejects a cyclic package graph). | D1 only. |
| Java | package, module (JPMS) | Classes and packages may be cyclic; the compiler compiles them together ([JLS 7.6](https://docs.oracle.com/javase/specs/jls/se21/html/jls-7.html#jls-7.6)). A JPMS module must not depend on itself ([JLS 7.7.1](https://docs.oracle.com/javase/specs/jls/se21/html/jls-7.html#jls-7.7.1)). | D1 only. |
| Kotlin | Gradle module | Files and packages may be cyclic; build modules may not. | D1 only. |
| Swift | module | Files in a module see each other; the whole module or file batches compile together ([CompilerPerformance.md](https://github.com/swiftlang/swift/blob/main/docs/CompilerPerformance.md)). | D1 only. |
| TypeScript, ESLint | file | Cycles are legal; `import/no-cycle` is an opt-in lint. | A lint. |
| dependency-cruiser | any path pattern | `no-circular` and path-based forbidden rules ([rules reference](https://github.com/sverweij/dependency-cruiser/blob/main/doc/rules-reference.md)). | A lint. |
| ArchUnit | "slices" of packages | `slices().matching("com.myapp.(*)..").should().beFreeOfCycles()` ([user guide](https://www.archunit.org/userguide/html/000_Index.html#_cycle_checks)). | D3 as a test. |

On compilation speed:

| Compiler | Unit it compiles | Parallelism inside the unit | Needs acyclic files? |
| --- | --- | --- | --- |
| rustc | crate | Parallel front end (`-Z threads`), codegen units | no |
| Go | package | Functions compiled in parallel since 1.9; packages in parallel from export data | no (inside), yes (between) |
| javac | the sources given together | Build tools parallelize by module | no |
| kotlinc | module | `-Xbackend-threads` in the JVM back end | no |
| Swift | module | Batch mode: one frontend per CPU; whole-module mode: one frontend | no; primary-file mode re-reads all files, "quadratically" in jobs |
| OCaml, dune | file | Files in parallel along the file graph, against `.cmi` interfaces | yes |

Go is the model the direction follows most closely. It is also the only
one whose cycle-free unit equals its compile unit and its visibility
unit. hd's direction splits them: files for names and visibility,
folders for cycles, packages for dependencies (C6).

## Problems, Ranked

| Rank | Id | Problem | Severity | Cases |
| --- | --- | --- | --- | --- |
| 1 | DC-1 | D3 rejects file graphs with no cycle that cross folders twice | High: the usual Rust crate layout | C1, C2, C10, C11 |
| 2 | DC-2 | D5 names a leaf file, but the node is a folder | High: the advice fails where it is needed | C1, C2 |
| 3 | DC-3 | Initialization order inside a file cycle is undefined | High: silent read of an unset binding | C13 |
| 4 | DC-4 | Test code's edges close folder loops | Medium | C12 |
| 5 | DC-5 | D2's spanning check is implied by D3, so one fact gets two errors | Medium | C4, C8, C9, C15 |
| 6 | DC-6 | G1: a `pub` inherent method may infer its result and row | Medium: serializes checking across files | Parallel Compilation |
| 7 | DC-7 | What a folder is: `mod.hd`, nested folders, `x.hd` beside `x/` | Medium | C2, probes |
| 8 | DC-8 | Re-export loops need a narrower rule than `module.pub-use.cycles` | Medium | C14 |
| 9 | DC-9 | Feature-folder layouts with cross-referencing entities are rejected | Low: intended, with a known layout | C9 |
| 10 | DC-10 | G2: a compile-time fact can depend on another file's body | Low | Parallel Compilation |
| 11 | DC-11 | The cyclic unit (folder) is not a visibility unit (file) | Low: noted only | C6 |

### DC-1. Acyclic File Graphs Rejected

**Effect.** C1, C2, C10, and C11 have no file cycle. Each is rejected
because two files in one folder sit on either side of a file in another
folder. The pattern is the usual Rust crate: a root facade, child folders,
and a shared `error.rs` or `config.rs` at the root.

**Candidates.**
- Keep D3, and give the diagnostic a fix-it that moves the shared file
  `x.hd` to `x/mod.hd`, which keeps its module identity.
- Exempt edges between a folder and its ancestors (hierarchical reading).
- Make D3 a default-on lint instead of a language rule.

### DC-2. The Leaf Advice Names A File

**Effect.** D5 says to put shared items in a leaf file. In C2, a leaf file
in `shop/` still leaves `shop/` in the loop. Only a leaf folder breaks it.

**Candidates.**
- Reword D5: shared items go in a leaf *folder*, often a lone `mod.hd`.
- Keep D5 as is and rely on the diagnostic's fix-it.

### DC-3. Initialization Inside A Cycle

**Effect.** In C13, `module.init.once` has no ready module, and the local
definite-initialization check misses a read in another file.

**Candidates.**
- Go's rule: order top-level bindings across the SCC by their
  dependencies through bodies; a dependency cycle is an error.
- Forbid top-level executable statements in any file of a multi-file SCC.
- Initialize SCC members by module identity and make an early read a
  run-time panic.

### DC-4. Test Edges

**Effect.** In C12, a test module's `use` of a helper folder closes a
folder loop, though no normal build sees that edge.

**Candidates.**
- Leave edges that start in test code out of the folder graph, like Go's
  `_test` packages.
- Count them, and ask for helper folders that use nothing under test.
- Check a separate test graph that includes them.

### DC-5. One Fact, Two Errors

**Effect.** Any file cycle spanning folders projects onto a folder cycle.
D2's error therefore never fires alone, and a user sees two codes for one
loop.

**Candidates.**
- One rule and one code: the folder graph is acyclic; a file cycle inside
  one folder is allowed.
- Keep two codes and report only the folder one when both apply.

### DC-6. `pub` Inherent Methods

**Effect.** If a `pub` inherent method may omit its result or row, a call
in another file needs that body. That is the one thing that makes
checking wait across files.

**Candidates.**
- Require the result type and row on every `pub` method, inherent or not.
- Keep the omission and accept ordered checking for those methods.

### DC-7. What A Folder Is

**Effect.** The direction does not say which folder `mod.hd` belongs to,
whether nesting adds edges, or how `src/shop.hd` relates to `src/shop/`.
Each reading changes which of C1, C2, and C11 pass.

**Candidates.**
- The literal reading, A1 and A2: a file's folder is its directory, and
  nesting adds no edge.
- The hierarchical reading: a folder includes its sub-folders.

### DC-8. Re-Export Loops

**Effect.** D2 replaces "cycles involving `pub use` are rejected". C14's
forwarding loop then needs its own rule, or its name resolves to nothing
without a clear error.

**Candidates.**
- A `pub use` chain must end at a declaration; a loop is an error.
- Keep `pub use` edges acyclic even inside a folder.

### DC-9. Feature Folders

**Effect.** In C9, cross-referencing entities in feature folders are
rejected. Teams must keep entities in one folder.

**Candidates.**
- Accept, and document the entity-folder layout.
- Allow type-only cycles across folders (a new exception).

### DC-10. Compile-Time Facts Across Files

**Effect.** A decorator fact that calls a function in another file makes a
package interface depend on that body, against
[`module.interface.determined`](../spec/STYLE.md#retired-rule-ids).

**Candidates.**
- Evaluate facts after body checking, and hash fact values into the
  interface for early cutoff.
- Limit fact expressions to literals and constructors.

### DC-11. Cycle Unit Versus Visibility Unit

**Effect.** In C6, helpers shared by files in one folder must be `pub`.
Go keeps them package-private. This is noted only, because
`module.vis.no-package-private` is a recorded rule and nothing here is new
evidence against it.

**Candidates.**
- Accept.
- Revisit folder-private visibility after the core module rules settle.

## Options, Ranked By Cost Order

Per [AGENTS.md](../AGENTS.md#design-cost-order), the costliest kind of
change decides the rank; ties go to the fewer changes. A tooling-only
change is outside the spec and counts as cheapest.

| Rank | Option | Costliest kind | Changes | Cases broken or needing a change |
| --- | --- | --- | --- | --- |
| 1 | O3. File-only acyclic (today's spec) | none | 0 | C3, C6, C7, C9, C14 |
| 2 | O4. D2 only: file cycles inside one folder; folder cycles allowed otherwise | semantic rule (kind 2) | 3: the SCC rule, re-export rule, init rule | C4, C8 (intended); C9 |
| 3 | O5. O4 plus D3 as a default-on lint over the program database | semantic rule (kind 2), plus tooling | 3, plus a lint | as O4; C1, C2, C10, C11 warn |
| 4 | O1. D1 to D5 as written | semantic rule (kind 2) | 4: two cycle rules, two codes; init and re-export rules still missing | C1, C2, C10, C11, C12, C13; C4, C8 (intended); C9 |
| 5 | O6. The combination, amended | semantic rule (kind 2) | 6: one folder rule and code, folder definition, test-edge exclusion, SCC init order, re-export rule, `pub` method signatures | C4, C8 (intended); C1, C2, C9, C10, C11 need a move |
| 6 | O2. Directory = module (Go) | module mapping and use paths (kind 1 and 2) | many: paths, `use`, visibility, facades | reverses D2's "a module stays one file" |

O6 ranks below O1 on count, but O1 is incomplete: it leaves C13 and C14
without an answer. O2 reopens D2, and C6 is the only evidence for it,
which is a visibility point rather than a cycle point.

## Recommendation

**Recommendation.** O6: keep explicit signatures at file boundaries as the
parallel-compilation lever, and keep the folder rule to bound the worst
case. The evidence favors it for three reasons.

1. Parallel checking is already safe. Every cross-file edge goes through a
   full signature, so closing G1 is enough.
2. The folder rule's value is the bound, not the speed. No SCC exceeds one
   folder, and every folder is a unit that compiles from its
   dependencies' signatures, as a Go package does. Melton and Tempero's
   data shows what happens without a bound.
3. No example broke O6 beyond a mechanical move. C1, C2, C10, and C11 each
   need one file moved into its own folder, and `x.hd` to `x/mod.hd`
   changes no `use` line. Agents do that well, and the diagnostic can
   offer it.

O6 amends the direction in these ways; each is a question below:
- one rule and one code instead of D2's and D3's two (Q2);
- the literal folder reading (Q3), with no ancestor exemption (Q4);
- D5 reworded to a leaf folder, with a fix-it (Q5);
- test code's edges left out (Q6);
- Go's initialization order inside an SCC (Q7);
- a re-export rule (Q8);
- full signatures on `pub` inherent methods (Q1).

If the owner prefers Rust's freedom over Go's layering, O5 is the
fallback: the bound stays, and D3 becomes a warning.

## Questions For The Owner

**Q1. Which lever carries parallel compilation?**
Effect: checking stays parallel only if no file needs another file's
bodies. The folder rule does not provide that; signatures do.
Candidates: (a) explicit signatures on everything visible outside a file,
with the folder rule for the bound; (b) the folder rule alone; (c) both,
plus requiring signatures on `pub` inherent methods (G1).
**Recommendation:** (c). hd has (a) except G1.

```text
impl Cart:
    pub fn total(self) -> i64:  # the result type is required under (c)
        0
```

**Q2. One rule and one code?**
Effect: a file cycle across folders always implies a folder cycle, so D2's
error never fires alone.
Candidates: (a) one rule, "the folder graph is acyclic", and one code;
(b) two rules and two codes.
**Recommendation:** (a).

```text
# src/ast/expr.hd: a cycle with stmt.hd is allowed inside ast/
use self.stmt.{Stmt}
```

**Q3. What is a file's folder?**
Effect: the answer decides whether C1, C2, and C11 pass.
Candidates: (a) the directory holding the file, `mod.hd` included, with
nested folders as separate nodes; (b) a folder includes its sub-folders.
**Recommendation:** (a), the reading every case above uses.

```text
# src/shop/mod.hd is in folder shop/, like src/shop/cart.hd
pub use pkg.shop.cart.{Cart}
```

**Q4. May a parent and child folder use each other?**
Effect: allowing it accepts C2 unchanged, which the Java data calls the
common case. It also lets a whole subtree become one SCC.
Candidates: (a) no exemption; (b) exempt ancestor edges.
**Recommendation:** (a). The exemption undoes the bound.

```text
# src/shop/orders/order.hd: uses a leaf folder, not shop/
use pkg.shop.common.{Money}
```

**Q5. Leaf file or leaf folder?**
Effect: D5's leaf file fails in C2.
Candidates: (a) reword D5 to a leaf folder, and have the diagnostic offer
the move `x.hd` to `x/mod.hd`; (b) keep D5.
**Recommendation:** (a).

```text
# src/error/mod.hd, moved from src/error.hd: still `use pkg.error.{Error}`
pub enum Error:
    TooBig(i32)
```

**Q6. Do test code's edges count?**
Effect: counting them rejects a shared test-helpers folder (C12).
Candidates: (a) leave out edges that start in test code, as Go does for
`_test` packages; (b) count them.
**Recommendation:** (a).

```text
# src/shop/cart_test.hd: this edge is not in the folder graph
use pkg.testkit.builders.{empty_cart}
```

**Q7. Initialization order inside a file cycle?**
Effect: without a rule, C13 reads `markup` before it is set.
Candidates: (a) Go's rule: order bindings across the SCC by dependencies
through bodies, and reject a cycle; (b) forbid top-level statements in a
file of a multi-file SCC; (c) module-identity order with a run-time panic.
**Recommendation:** (a). It is Go's shipped rule, and it runs after body
checking, so it does not serialize checking.

```text
let featured = prices.price_of("tea")  # ordered after prices.markup under (a)
```

**Q8. Re-export loops?**
Effect: with `pub use` cycles allowed in a folder, a forwarding loop needs
its own error.
Candidates: (a) a `pub use` chain must end at a declaration; (b) keep
`pub use` edges acyclic everywhere.
**Recommendation:** (a). It keeps the facade loop in C14.

```text
# src/shop/a.hd: an error under (a) when b.hd forwards Token back
pub use pkg.shop.b.{Token}
```

**Q9. Diagnostic content?**
Effect: an agent that moved one file needs to see the loop and a fix.
Candidates: (a) print one shortest folder loop, one `use` line per edge,
the SCC size, and the leaf-folder fix-it; (b) print every edge of the SCC.
**Recommendation:** (a), as Go's import stack does.

```text
use pkg.billing.ledger  # the diagnostic points here as the shop/ -> billing/ edge
```

**Q10. Compile-time facts that call other files?**
Effect: a fact makes a package interface depend on a body (G2).
Candidates: (a) evaluate facts after bodies and hash their values into the
interface; (b) limit fact expressions to literals and constructors.
**Recommendation:** (a). It keeps decorators as written and preserves early
cutoff.

```text
@label(default_label())  # default_label may live in another file under (a)
data Invoice:
    pub id: i64
```

## Parse Log

Every `text` block was parsed with `parseSource` from
[spec/reference-parser/parser.ts](../spec/reference-parser/parser.ts).
Parsing checks syntax only.

| Block | Where | Result |
| --- | --- | --- |
| 1 | C1 `src/mod.hd` | parse |
| 2 | C1 `src/error.hd` | parse |
| 3 | C1 `src/parser/parse.hd` | parse |
| 4 | C2 `src/shop/mod.hd` | parse |
| 5 | C2 `src/shop/common.hd` | parse |
| 6 | C2 `src/shop/orders/order.hd` | parse |
| 7 | C3 `src/ast/expr.hd` | parse |
| 8 | C3 `src/ast/stmt.hd` | parse |
| 9 | C5 `src/diagnostics/render.hd` | parse |
| 10 | C6 `src/http/client.hd` | parse |
| 11 | C6 `src/http/transport.hd` | parse |
| 12 | C7 `src/typed/core/pods.hd` | parse |
| 13 | C8 `src/models/user.hd` | parse |
| 14 | C8 `src/services/user_service.hd` | parse |
| 15 | C9 `src/users/user.hd` | parse |
| 16 | C9 `src/orders/order.hd` | parse |
| 17 | C10 `src/core/plugin.hd` | parse |
| 18 | C10 `src/core/registry.hd` | parse |
| 19 | C10 `src/plugins/json.hd` | parse |
| 20 | C10 `src/main.hd` | parse |
| 21 | C11 `src/main.hd` | parse |
| 22 | C11 `src/config.hd` | parse |
| 23 | C11 `src/cli/args.hd` | parse |
| 24 | C12 `src/testkit/builders.hd` | parse |
| 25 | C12 `src/shop/cart_test.hd` | parse |
| 26 | C13 `src/shop/prices.hd` | parse |
| 27 | C13 `src/shop/catalog.hd` | parse |
| 28 | C14 `src/shop/mod.hd` | parse |
| 29 | C14 `src/shop/cart.hd` | parse |
| 30 | C14 `src/shop/a.hd` | parse |
| 31 | C14 `src/shop/b.hd` | parse |
| 32 | Q1 | parse |
| 33 | Q2 | parse |
| 34 | Q3 | parse |
| 35 | Q4 | parse |
| 36 | Q5 | parse |
| 37 | Q6 | parse |
| 38 | Q7 | parse |
| 39 | Q8 | parse |
| 40 | Q9 | parse |
| 41 | Q10 | parse |

Reference-parser findings: none. The parser accepts a `use` cycle, as
expected, because cycle checks are a later phase.

## Sources

- Melton, H., and Tempero, E. "An empirical study of cycles among classes
  in Java." Empirical Software Engineering 12(4), 2007, pp. 389-415.
  [Springer](https://link.springer.com/article/10.1007/s10664-006-9033-1).
- Al-Mutawa, H. A. "On the classification of cyclic dependencies in Java
  programs." MSc thesis, Massey University, 2013.
  [PDF](https://mro.massey.ac.nz/server/api/core/bitstreams/37d0316d-5318-488f-b89e-bf8619455c15/content).
- Al-Mutawa, H. A., Dietrich, J., Marsland, S., McCartin, C. "On the shape
  of circular dependencies in Java programs." ASWEC 2014, pp. 48-57.
  [IEEE](https://ieeexplore.ieee.org/document/6824106/).
- The Go Programming Language Specification:
  [Import declarations](https://go.dev/ref/spec#Import_declarations),
  [Package initialization](https://go.dev/ref/spec#Package_initialization).
- [Go 1.9 release notes](https://go.dev/doc/go1.9),
  [go test packages](https://pkg.go.dev/cmd/go#hdr-Test_packages),
  [gcexportdata](https://pkg.go.dev/golang.org/x/tools/go/gcexportdata).
- [Faster compilation with the parallel front-end in nightly](https://blog.rust-lang.org/2023/11/09/parallel-rustc.html),
  and [Cargo profiles, `codegen-units`](https://doc.rust-lang.org/cargo/reference/profiles.html#codegen-units).
- [JLS chapter 7](https://docs.oracle.com/javase/specs/jls/se21/html/jls-7.html),
  sections 7.6 and 7.7.1.
- [What's new in Kotlin 1.6.20](https://kotlinlang.org/docs/whatsnew1620.html).
- [Swift CompilerPerformance.md](https://github.com/swiftlang/swift/blob/main/docs/CompilerPerformance.md).
- [TypeScript 5.5, isolated declarations](https://devblogs.microsoft.com/typescript/announcing-typescript-5-5/#isolated-declarations).
- [OCaml manual, recursive modules](https://ocaml.org/manual/5.3/recursivemodules.html);
  [Real World OCaml, files and modules](https://dev.realworldocaml.org/files-modules-and-programs.html).
- [F# FS-1009](https://github.com/fsharp/fslang-design/blob/main/FSharp-4.1/FS-1009-mutually-referential-types-and-modules-single-scope.md).
- [GHC user guide, mutually recursive modules](https://downloads.haskell.org/ghc/latest/docs/users_guide/separate_compilation.html#how-to-compile-mutually-recursive-modules).
- [eslint-plugin-import `no-cycle`](https://github.com/import-js/eslint-plugin-import/blob/main/docs/rules/no-cycle.md),
  [dependency-cruiser rules](https://github.com/sverweij/dependency-cruiser/blob/main/doc/rules-reference.md),
  [ArchUnit cycle checks](https://www.archunit.org/userguide/html/000_Index.html#_cycle_checks).
- [NestJS circular dependency](https://docs.nestjs.com/fundamentals/circular-dependency),
  [Go `sql.Register`](https://pkg.go.dev/database/sql#Register).
