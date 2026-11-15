# Verified pilot findings (main-loop check of Haiku flags)

| batch | path | haiku | my verdict | why |
|---|---|---|---|---|
| 2 | annotations/typed-derivation/derived-bounds/derive-keeps-declared-parameter-bounds.hd | FIX B1 | REJECT | Declared bounds are also required for Pair[T] to be well-formed, so the rule is hard to observe; the suggested fix wouldn't isolate it either. |
| 2 | annotations/typed-derivation/derivation-blocks/structure-without-use.hd | FIX B2 | REJECT | Structure is unresolved, so derivability is never judged; unknown-trait is the only error. |
| 2 | C2 x3 (auto-closing.target, cfg.target, cfg.argument) | REPORT | NOISE | The reviewer itself says the accept forms are covered under cfg.pair. C2 is per-anchor and misses that. |
| 3 | annotations/error-derivation/error-form-other-rule.hd ~ error-argument-number.hd | MERGE C1 | PLAUSIBLE | Both are invalid @error arguments (identifier vs number); form.other defers to form.argument for this case. Owner call. |
| 3 | annotations/typed-derivation/facts/fact-evaluation-reads-itself.hd | UNSURE B2 | CONFIRMED (minor) | Rule says panic is at the second read (line 12, in own_path); marker is on main's line 21. README: the line is not judged but the marker documents it. |
| 3 | annotations/typed-derivation/facts/fact-evaluation-cycle.hd | UNSURE B2 | CONFIRMED (minor) | Same as above. |
| 3 | annotations/two-annotate-forms/facts-of-literal-generic-none.hd | FIX B3 | REJECT | types.option.wrap: a string literal is accepted where string? is expected. |
| 3 | typing/warnings/derivation-block-decorator{,-std}.hd | UNSURE B1 | CONFIRMED (minor) | Accept cases in typing/warnings; README Layout says that dir holds warn fixtures. Move to typing/valid, or make them warn cases. |
| 3 | C2 error.bound.interpolated, unused-block-decorator.lint | REPORT | keep as report | Plausible coverage gaps. |
| 1 | annotations/two-annotate-forms/values-are-found-by-type-in-source-order.hd | FIX B1 | CONFIRMED | Each fact type appears once, so "source order" (first match wins) is never exercised. Title overclaims. |
| 1 | annotations/error-derivation/error-generated-bounds.hd | FIX B1 | PLAUSIBLE | Title names the transparent bound; no generic use needs it. Not checked in depth. |
| 1 | annotations/typed-derivation/walk-describe-and-build/structure-build-fills-the-members-the-source-never-names.hd | UNSURE B1 | REJECT | `hd check` accepts the &mut / *mut pair (exit 0, not in KNOWN_FAILURES). |
| 1 | annotations/two-annotate-forms/different-facts-are-evaluated-independently.hd | FIX B5 | CONFIRMED (minor) | facts_of import and no_prefix are unused. |
| 1 | annotations/typed-derivation/derivation-blocks/member-line-lists-self-line-takes-a-list-expression.hd | FIX B5 | not checked | dead-code claim |
| 1 | annotations/two-annotate-forms/annotations.hd | FIX B5 | not checked | dead-code claim |
| 1 | annotations/two-annotate-forms/member-metadata-a-derivation-may-omit-a-member-with-a-default.hd | FIX B5 | CONFIRMED (minor) | struct Audit unused. |
| 1 | C2 generated-error-bounds | REPORT | keep as report | |
| 5 | annotations/typed-derivation/derivation-blocks/extending-a-member-line-cannot-take-pass.hd | UNSURE B2 | REJECT | `hd check` reports exactly one error, on the marked line. |
| 5 | annotations/typed-derivation/derivation-blocks/trait-less-derivation-block.hd | FIX B1 | CONFIRMED | `email = [...]` "replaces" metadata, but email has no decorator, so nothing is replaced. |
| 5 | annotations/typed-derivation/derivation-blocks/member-line-list-expression.hd | FIX B1 | PLAUSIBLE | title "any list-typed expression"; not checked in depth. |
| 5 | annotations/typed-derivation/derivation-blocks/trait-less-block-declares-no-method.hd | FIX B5 | CONFIRMED (minor) | Note and note() unused. |
| 5 | C2 no-trait.generic.rename, no-trait.not-inherent, metadata.eval-as-fact | REPORT | keep as report | |
| 4 | annotations/two-annotate-forms/facts-of-the-result-of-a-function-declaration-is-its-facts.hd | FIX B1 | CONFIRMED | An accept fixture cites target.error, an error-only rule. Wrong spec column. |
| 4 | typing/invalid/facts-of-check-{a-missing,an-extra}-argument-...-{30,31}.hd | FIX B1 | CONFIRMED | Expect argument-count but cite target.error (which gives invalid-facts-of-target). Wrong spec column. |
| 4 | annotations/two-annotate-forms/facts-of-an-unknown-name-is-unknown.hd | UNSURE B2 | SPEC QUESTION | target.error says "any other argument" -> invalid-facts-of-target; fixture expects unknown-name. Spec doesn't say which wins. |
| 4 | annotations/typed-derivation/handles/facts-and-handles-build-once.hd | FIX B1 | PLAUSIBLE | Tests fact laziness; cites handle.constants. |
| 4 | annotations/typed-derivation/handles/structure-a-generic-owner-has-one-handle-per-instantiation.hd | FIX B3 | CONFIRMED | Prints name/position/index equalities that are true whether handles are shared or per-instantiation. |
| 4 | annotations/typed-derivation/handles/tuple-handle-instances.hd | FIX B1 | CONFIRMED | Cites handle.shared-target but never observes sharing. |
| 4 | typing/warnings/derivation-line-drift{,-beside-derive}.hd | FIX B1 | CONFIRMED | Same class as batch 3: accept cases in typing/warnings citing a lint rule the case cannot check. |
| 4 | annotations/two-annotate-forms/inline-and-cold.hd | UNSURE B6 | REJECT | `hd check` ok. |
| 4 | C2 x6 | REPORT | keep as report | |
| 6 | annotations/typed-derivation/templates/template-outside-trait-module.hd, template-reexported-from-other-package.hd | FIX B2 | CONFIRMED | Template writes `fn debug(self) -> string`; std Debug is `debug(&self, out: *mut DebugWriter) -> void`. A second error hides behind the marked one (hd stops at misplaced-derivation). |
| 6 | annotations/typed-derivation/the-structure-trait/structure-calls-are-pure.hd | FIX B1 | CONFIRMED | Title claims walk, describe and build; code calls only describe. |
| 6 | annotations/two-annotate-forms/annotations-a-field-annotation-on-a-struct-field-is-valid.hd | UNSURE B1 | not checked | Accept case whose title reads like a rejection. |
| 6 | C2 x4 | REPORT | keep as report | |
| 7 | annotations/two-annotate-forms/typed-fact-function.hd | FIX B1 | PLAUSIBLE | Title says parameters are written as bounds; wrap[T] is unbounded. |
| 8 | annotations/typed-derivation/misplaced-derivation-a-template-outside-its-traits-module-is-misplaced.hd | FIX B2 | CONFIRMED | Template body is "x" with no walk/describe/build, so annot.template.body.error (marker-template) also applies. |
| 8 | annotations/typed-derivation/walkers-describers-and-sources/walker-member-through-a-generic-structure-parameter-is-callable.hd | FIX B1 | CONFIRMED | Accept case titled "errors"; the walker is concrete. Retitle. (DELETE C1 rejected: generic S differs from 41.) |
| 8 | annotations/typed-derivation/misplaced-derivation-a-structure-block-without-lines-for-a-struct-is-valid.hd | FIX B1 | CONFIRMED | Legal twin kept the reject twin's title. |
| 8 | annotations/typed-derivation/misplaced-derivation-a-trait-less-block-may-rename-the-type-parameter.hd | DELETE C1 | REJECT | The renamed parameter (impl[U] Box[U] for struct Box[T]) is the point of the case. |
| 8 | annotations/typed-derivation/walkers-describers-and-sources/generic-data-member-call.hd ~ generic-member-call.hd | DELETE C1 | PLAUSIBLE | Differ only in hook name (data_member vs member). Owner call: one case per hook, or one total. |
| 8 | annotations/typed-derivation/typed-derivation.hd ~ annotations/typed-derivation/walk-describe-and-build/typed-derivation-walk.hd | DELETE C1 | PLAUSIBLE | not checked in depth |
| 8 | annotations/typed-derivation/a-method-named-like-a-source-member-releases-its-owned-string.hd | UNSURE B1 | not checked | release only visible in checked mode |
| 8 | annotations/typed-derivation/derive-a-generic-target-meets-the-bound-through-its-derived-bound.hd | FIX B5 | not checked | |
