# Compiler Repair Queue

Status: owner-requested implementation queue, refreshed 2026-10-03 after spec pass 88. Unchecked items are pending unless their text says deferred.

Deliver one root-cause repair at a time. Do not alter the specification, weaken fixtures, raise timeouts, or synthesize stdlib APIs in the compiler. Before every push, fetch and rebase on `origin/main`, run the full `pnpm run check`, and watch the main `Test` workflow. Every commit includes `Co-Authored-By: Codex <codex@openai.com>`.

## Step 0: Re-sweep

- [x] **Re-ran all 186 known-failure rows, including the ten added by spec pass 88.** Three now match their normative expectations and moved to `test/portable/cases.tsv`: `typing/valid/list-literal-compares-with-readonly-binding.hd`, `typing/invalid/error-anyval-newtype.hd`, and `typing/valid/dynamic-safety-implied-anyref.hd`. The known-failure count is now 183: 122 language-tier and 61 stdlib-tier rows. `src/KNOWN_ISSUES.md` is recounted from the ledger.
- [ ] **Deferred: `STD-1` acceptance.** The checker now preserves bounded inherent Map impls, but `lib/std/collections.hd` contains no `get_or`, `keys`, or `values`. The two rows cannot pass under a src-only repair without a compiler hack.

## P1: Big Unlocks

- [ ] **1. U32-SIZES (53).** Add the prelude alias `usize = u32`; make lengths, indices, and slice bounds unsigned; context integer literal indices before selecting `Index`; migrate std signatures without broad implicit conversions.
- [ ] **2. STD-LOADER (13) and STD-CLI (5).** Register `std.regex` and `std.cli`; retain JSON's `Number` declaration when a public JSON API needs it without an explicit user import.
- [ ] **3. ZIP-ARG / BOUND-INFERENCE (5).** Infer bound-only type parameters from the bounded argument's unique trait implementation to a fixed point; distinguish none, several, defaults, and disagreeing bounds.

After P1, record exact final WAT bytes for the tiny `main` program and a one-`it` test, using identical compiler options.

## P2: Hooks And Hosts

- [ ] **4. FLOAT-PARSE (4) and FIXED-HOOK (1).** Add correctly rounded `parse_f64` and `format_f64_fixed` host primitives; preserve ties-to-even, special values, and negative zero.
- [ ] **5. Host f64 boundary.** Preserve raw IEEE f64 values in live calls, serialize exact bits for replay, and implement `special-float-host` (`HOST-NAN`), `misbehaving-host` (`HOST-CONTRACT`), and `pending-write` (`PENDING-WRITE`).
- [ ] **6. HOST-CATALOG (7).** Implement Console error output and the remaining provider helpers by declaration identity; remove the checker-owned `ConsoleError` type.
- [ ] **7. Test runners.** Bind `TestRunner` for every test body; implement `snapshot_check`; update PropertyRunner/PropertyCase and discard behavior. Tags: `SNAPSHOT-ROW`, `RUNNER-SURFACE`.

## P3: Language And Runtime Correctness

- [ ] **8. LIST-POP (4).** Rename the list intrinsic from `append` to `push`; add `pop`, `insert`, `remove_at`, and `clear` over one truncation hook; remove the std forwarding method when non-src scope is permitted.
- [ ] **9. ALL-LIST (1).** Fix the illegal cast when a generic function calls an element of `List[fn() -> T]`.
- [ ] **10. ERR-HELPERS (1 remains).** The checker now enforces static `AnyRef`/`AnyVal` supertrait obligations and transitive category bounds without runtime dictionaries. Remaining: static TypeId evidence for `e.find::[Error]()` must not enter a forwarding adapter as a real receiver.
- [ ] **11. MODULE-PATH-VALUE.** Resolve module namespaces in every path expression, not only module-qualified calls.
- [ ] **12. Open Opus audit findings.** Repair O-02 dynamic safety in `resolveType`; O-03 written type-argument bounds; O-04 std orphan ownership; O-05 multiline-string test indentation; O-06 whole-state speculation; O-07 regex-based dynamic safety.
- [ ] **13. Smaller tags.** Resolve `STD-DEBUG`, `DERIVE-DEFAULT`, `METHOD-DEFAULT`, and the held `RETRY-WITH` row. The Default audit found two distinct repairs: declared-default members bypass source calls while retaining member positions, and Default enum derivation counts identity-bound `@default` facts. Current language rules conflict with the recorded declared-default owner decision; that spec drift is reported here, not edited.

## P4: Spec Pass 88

- [ ] Register testing calls by declaration identity for bare, renamed, and module-qualified forms.
- [ ] Continue a leading-dot expression only at deeper indentation.
- [ ] Contextualize variants across `==` and `!=` in either operand order.
- [ ] Compare `mut T` with `T` at `T`, and weaken inference joins to readonly.
- [ ] Contextualize an integer literal from the other operand in either order.
- [ ] Change the missing-let diagnostic to: `a binding with a type annotation must begin with 'let'`.
- [ ] Permit forward and mutual generic bounds while keeping defaults trailing.
- [ ] Parse qualified string prefixes such as `text.r"..."`.

## Delivery

- [ ] Report each item as fixed, deferred with a reason, or not reproducible.
- [x] Record Step 0's known-failure count before and after: 186 → 183.
- [ ] Record the post-P1 tiny-program and one-test WAT footprint.
- [ ] Keep main CI green after each pushed repair.
