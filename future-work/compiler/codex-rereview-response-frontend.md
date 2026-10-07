# New Compiler Design: Codex Re-Review Response, Frontend Lane

Status: Response to the Codex re-review through 54f249b7, frontend lane rows, 2026-10-07.

Part of the [compiler design](README.md). The backend lane answers its
findings in its own response file. The re-review is `codex_review.md`,
which is not committed; only its current findings are answered here, not
its archived appendix.

**How each row was checked.** Each finding was read against the frontend
files as they stood after S1c (HEAD 636192f0) and against the spec,
looking for a reason the claim is false. The quoted sentence is the one
at issue, before this pass. Verdicts:

- `accepted-fixed`: true, and fixed in a frontend file;
- `partly`: true in part; the row says which part;
- `needs-owner`: the fix changes the language; the design follows the
  recommendation until the owner answers;
- `needs-backend`: the fix, or part of it, lands in a backend file.

No finding was rejected.

The same pass applied two owner decisions recorded in
[goals.md](goals.md#summary) that the re-review counts under N-C1:

- **GADTs are removed.** Refinement, existentials, `Rigid`, birth marks,
  arm equalities and their memo, rollback and TIR rules are gone from
  [type-checking.md §6.1](type-checking.md#61-arm-local-equalities),
  §1.4, §1.6, §3.2, §3.5, §13 and §16, from
  [trait-solver.md](trait-solver.md) §2.2, §2.3, §3.1, §4.3, §7.1 and
  §8.2, and from [resolution-and-interfaces.md](resolution-and-interfaces.md).
  Headings stay, so links into them still work.
- **Backend changes on main (b4ef8697) followed here.** `CallDyn` has no
  evidence operands and the vtable shape has no `dyn_bounds`: the erased
  body gets bound methods from the type witness (codegen.md §13.5.1, N5).
  Row subsumption is no instruction (N-I2). Slots are durable
  instructions with a `fills` log (N-I1). The `is` text follows S1c.
  trait-solver.md §14.2 lists the solver-only adversarial fixtures that
  N-R1's gate 4 gives the frontend lane.
- **`dyn` types and per-member availability.**
  [syntax.md §4.4](syntax.md#44-parser-and-green-tree) parses `dyn Tr` as
  a type form;
  [trait-solver.md §9.1](trait-solver.md#91-which-traits-can-be-values)
  replaces the per-trait gate with per-member availability; §9.2's erased
  slot takes a type witness per method type parameter.

## Rows

| Id | Verdict | Fixed in | Reason |
| --- | --- | --- | --- |
| N1 | accepted-fixed | [trait-solver.md §3.2](trait-solver.md#32-owner-modules), §1.1, §2.2, §7.1, §14.2; [type-checking.md §1.6](type-checking.md#16-the-trait-solver-interface) | Quoted: "the global memo lives for one run: one frozen set of interfaces". True: `Instantiations { Receiver, Pick }` has no placeholder, so it went to the global memo, while its answer depends on the asker's closure. Every context now carries an interned `ImplUniverseId` (the folders in its closure with argument-owned impls), keyed into every `Instantiations` and `Methods` goal. Other goals provably never read the directory; a debug bit asserts it. |
| N7 | accepted-fixed, needs-backend | [trait-solver.md §8.3](trait-solver.md#83-what-codegen-does-with-it), §1.3 | Quoted: "`select` therefore matches heads in the owner modules and returns the impl and its arguments". True: matching `Feed[ConcreteStore]` fixes `I` but not `T`. `select` now runs the plan's `Bind` steps after the head match, without proving `Bound` steps, and keeps its answers in a codegen table apart from the proof memo. codegen.md §13.2 must say the same. |
| N8 | accepted-fixed, needs-backend | [type-checking.md §5.4](type-checking.md#54-closure-rows), §5.2, §5.3, §5.5 | Quoted: "A closure that calls an omitted-row function records its facts under the enclosing callable's `RowVar`". True. Instead of a new row variable per closure, the closure's row is its own `BodyRow` (keys plus pending parts), its calls name `Closure(sub_body)` as caller, and a body records facts only where it invokes a function value. The row sweep resolves a returned closure's type. Cold suspensions keep their capture rule. |
| N10 | accepted-fixed, needs-backend | [type-checking.md §3.5](type-checking.md#35-the-trail-and-the-one-rollback-contract), §1.5, §3.6, §13 | Quoted: "`scopes`, `loops`, `fns` ... equal depth at the trial's end, asserted". True. Join operands and held classes are now rows of an append-only `join_items` column with a trailed head per frame. A slot reserved before a trial and filled during it is recorded in the builder's `fills` log, which the backend added for N-I1, and rollback empties it. Also found: the statement's open literal classes were a stack that trials grew; now a column. The verifier hashes frame contents and older slots. |
| N-A1 | partly, needs-backend | [resolution-and-interfaces.md §4.10](resolution-and-interfaces.md#410-folder-interface-construction), §4.10.1, §4.12.1; [trait-solver.md §3.2](trait-solver.md#32-owner-modules) step 4, change 21 | Quoted: "Such an impl may live in a folder whose public API, and so whose deep hash, does not change" (cache.md). The two documents did disagree. Resolution's reading holds: an argument-owned impl has a nameable trait and target, so its head is in `api_hash` and the deep hash. The `argc` graph is redundant and should go; the solver memo keeps the per-context universe (N1). |
| N-A3 | accepted-fixed | [syntax.md §4.5](syntax.md#45-header-extraction-and-the-api-text-hash) | Quoted: "the hash of the token kinds and texts", with whitespace left out. True: indentation is syntax in kept template bodies. The hash now reads the green tree's tokens with its virtual layout tokens (`Newline`, `Indent`, `Dedent`, `SuiteEnd`), so equal lexical tokens with different blocks hash differently. A test checks it. |
| N-D1 | accepted-fixed, needs-backend | [trait-solver.md §5.3](trait-solver.md#53-why-no-global-index), §3.3, §5.2, §11; [resolution-and-interfaces.md §4.12.3](resolution-and-interfaces.md#4123-coherence) | Quoted: "Each trait is one task keyed by its sorted head hashes". True: the report picks the later impl by rank, which the key did not cover. A head hash is now `H(head, rank)`. Also found: the rank used a byte offset, which a body edit above the impl moves without rebuilding the interface; it is now the item index. |
| N-T1 | accepted-fixed | [trait-solver.md §4.3](trait-solver.md#43-normalization-lazy-at-three-points) | Quoted: "select the impl by head (section 3.4), read its binding for `Name`". True: `Box[NoDisplay]::Item` normalized through an impl that does not apply. A source `Project` on a known base now proves `Implements` first, as a memoized goal; codegen's `normalize_concrete` stays a separate, proof-free mode. Rigid projections are unchanged. |
| N-T2 | accepted-fixed | [trait-solver.md §7.4](trait-solver.md#74-fuel), §6.1, §7.1, §7.5, §7.6, §12; [type-checking.md §1.6](type-checking.md#16-the-trait-solver-interface), §11.1 | Quoted: "sum of cost(c) over the distinct children c of g". True: the sum counts shared descendants once per path, `2^n` for `2n` goals. Memo entries now store their children; a body is charged `1 + heads` once per distinct proof node, by a DAG walk that is the same on a hit and a miss. |
| N-T3 | accepted-fixed, needs-owner | [trait-solver.md §4.2](trait-solver.md#42-elaboration-of-the-environment), §13, §7.6 | Quoted: "A trait reference already present is skipped." True: a diamond could drop `Item = i32`. Clauses are keyed by trait reference and their bindings merge; the result is order-free. Two different bindings for one projection are a header error; reusing `duplicate-associated-binding` for elaborated bindings is a small language question. |
| N-T4 | accepted-fixed | [trait-solver.md §6.5](trait-solver.md#65-ambiguity), §1.2; [type-checking.md §2.5](type-checking.md#25-methods-and-operators) | Quoted: "The solver checks each candidate's bound plan and drops those that fail." True: a bound on a parameter fixed only by the trait arguments cannot be decided there. Candidates are now schemes with residual bounds, instantiated with fresh variables inside each trial. `Many` is returned even for one candidate, and each outcome is in a table. |
| N-T5 | accepted-fixed | [type-checking.md §3.6](type-checking.md#36-literal-widths) step 2 | Quoted: "When a `return` or `break` operand's type is an open literal class". True, and [`types.literal.local.form.join-open`](../../spec/lang/04-type-system.md#r-types.literal.local.form.join-open) says any class that reaches the join stays open. Every open class reachable through the operand's type is now held. A class that a `let` of the same statement reaches still defaults at its statement end. |
| N-T6 | partly | [trait-solver.md §5.2](trait-solver.md#52-the-overlap-check), §3.3; [resolution-and-interfaces.md §4.12.3](resolution-and-interfaces.md#4123-coherence) | Quoted: "Ground heads ... insert it into a hash set", and "Each head queries the tree and the set before it is inserted". The solver's numbered steps can be read as ground-first phases, so its order counterexample needed the resolution text. But the text was ambiguous, the two files disagreed, a hash set was queried like a trie, and "at most one row per head key" was false. Now: explicit two phases with a ground trie, a probe of every bucket row, and an honest cost statement. |
| N-T7 | accepted-fixed | [type-checking.md §2.5](type-checking.md#25-methods-and-operators), §11.3; [trait-solver.md §7.6](trait-solver.md#76-bounded-is-not-linear) | Quoted: "20 nested levels of two candidates cost about 80 trials, not a million". True: distinct outer expected types defeat the memo, and tainted trials are never cached. The bound is restated as distinct contexts plus tainted trials. The pathological suite gains context-growth and tainted-closure cases that expect `item-too-complex`. |
| Question 2 | needs-owner | [trait-solver.md §9.1](trait-solver.md#91-which-traits-can-be-values) | A `dyn` type with an unbound associated type. Recommendation: per-member unavailability. Members that mention the unbound type are unavailable, with the error at the call. The type satisfies no bound on its trait, and no call makes an existential. |

## Needs Owner

1. **Decided (owner, 2026-10-07): keep the rule.** A `dyn` type must bind
   every associated type; [`trait.dyn.binding.complete`](../../spec/lang/09-traits.md#r-trait.dyn.binding.complete)
   is unchanged. The question as it was asked:
   **A `dyn` type with an unbound associated type (authors' question 2).**
   Today [`trait.dyn.binding.complete`](../../spec/lang/09-traits.md#r-trait.dyn.binding.complete)
   makes `dyn Supplier` without `Item = ...` an error.
   **Recommendation:** fold this into S1d with the per-member rule.
   `dyn Supplier` is a valid type; each member whose signature mentions
   `Item` is unavailable on it, with the error at the call and a fix-it
   that adds the binding; members that do not mention `Item` work. The
   type satisfies no bound on `Supplier` (Swift: an existential does not
   conform to its own protocol), since generic code could name
   `T::Item`. No existential values, no opening. **Alternative:** keep
   the rule as it is (Rust). That needs no language change but leaves
   one per-type gate after the owner dropped the per-trait one. The
   solver's work is the same either way.
2. **Decided (owner, 2026-10-07): as recommended**, applied in S1d as
   [`trait.binding.super.merge`](../../spec/lang/09-traits.md#r-trait.binding.super.merge)
   and [`trait.binding.super.conflict`](../../spec/lang/09-traits.md#r-trait.binding.super.conflict).
   The question as it was asked:
   **Conflicting bindings found by elaboration (N-T3).**
   [`trait.binding.once`](../../spec/lang/09-traits.md#r-trait.binding.once)
   rejects a second *written* binding of one projection. Bindings that
   arrive through supertraits, as in `T < A & B` where `A < Supplier[Item
   = i32]` and `B < Supplier[Item = string]`, are not covered, and inside
   the body they would make `i32` and `string` equal.
   **Recommendation:** equal elaborated bindings merge silently;
   different ones are `duplicate-associated-binding` on the bound list.
   One added rule, an existing code.

## Changes for the backend lane

1. **cache.md §5.3 (N-A1).** Remove `argc` and `arg_impls_closure_hash`
   from `check_key` and `hdr_key`, and the "candidate directory"
   rationale bullet: the deep hashes cover argument-owned impls
   ([resolution-and-interfaces.md §4.10](resolution-and-interfaces.md#410-folder-interface-construction)).
   This holds only if the folder lists in `check_key`, `hdr_key` and
   `test_key` are the transitive dependency closure, the same closure
   that filters the directory; say so in the key text. If the backend
   prefers to keep `argc`, then `test_key` must add the test closure's
   `argc` too.
2. **cache.md §5.3, `coh_key` (N-D1).** "Head hash" means `H(canonical
   head, content rank)`, with the rank as `(package, module path, item
   index)` ([trait-solver.md §5.3](trait-solver.md#53-why-no-global-index)).
   The formula is unchanged; cite the definition.
3. **codegen.md §13.2 step 4 (N7).** `select` is a head match followed
   by the bound plan's `Bind` steps through `normalize_concrete`; it
   returns every impl argument. Its table is separate from the proof
   memo.
4. **checking-and-tir.md §4.13.4 (N8).** `CallerRow` gains
   `Closure(sub_body)`. A closure's row belongs to its function type; a
   call of a function value whose row holds pending parts records facts
   at the invoking body. No fact puts a closure's keys into its creator.
5. **data-structures.md §3.9.5, with N-I1 (N10).** Done on main: slots
   are durable `Slot` instructions, and the `fills` log undoes
   pre-checkpoint fills at rollback. type-checking.md §1.5 and §3.5 now
   use that log instead of a checker-side column.
6. **Driver and scheduler (N1).** After M1 builds the closure bit sets,
   compute one `ImplUniverseId` per solving context: each module, each
   module's test overlay, each `HeaderCheck(F)`. Intern them per run.
7. **GADT removal in backend files.** data-structures.md §3.4 drops
   `Rigid` and `HAS_RIGID`; checking-and-tir.md drops §4.13.6's
   refinement, the `Refine` coercion, the `Evidence` callee and
   `NewVariant` evidence choices; codegen.md §13.5 drops GADT evidence.
8. **testing-the-compiler.md.** Add the A/B universe case in both orders
   (N1), interior layout edits of template bodies (N-A3), swapped
   overlapping impls (N-D1), and the trial context-growth and
   tainted-closure cases (N-T7).
9. **README.md.** List this file beside the backend lane's response.
