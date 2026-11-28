# Changelog

## v0.0.4 — 2026-11-28

Changes since v0.0.3.

### Breaking

- Retired `lex.ident.mixed-script.warning` (lang/01-lexical-structure.md).
- Retired `lex.keyword.reserved-words` (lang/01-lexical-structure.md).
- Retired `lex.contextual.fixed` (lang/01-lexical-structure.md).
- Retired `lex.literal-fn.meaning` (lang/01-lexical-structure.md).
- Retired `lex.op.token-list-assign` (lang/01-lexical-structure.md).
- Retired `lex.op.longest.examples` (lang/01-lexical-structure.md).
- Retired `lex.op.bang-call` (lang/01-lexical-structure.md).
- Retired `lex.op.range.longest` (lang/01-lexical-structure.md).
- Retired `grammar.stmt.defer` (lang/02-grammar.md).
- Retired `grammar.stmt.copy-assign` (lang/02-grammar.md).
- Retired `grammar.data.mut-type` (lang/02-grammar.md).
- Retired `grammar.enum.variant-result.refine` (lang/02-grammar.md).
- Retired `grammar.trait.method-kind` (lang/02-grammar.md).
- Retired `grammar.impl.inline-bounds` (lang/02-grammar.md).
- Retired `grammar.impl.traitless-by` (lang/02-grammar.md).
- Retired `grammar.impl.traitless-by.other` (lang/02-grammar.md).
- Retired `grammar.type-decl.row-alias.one-key` (lang/02-grammar.md).
- Retired `grammar.generic.bound.old-plus` (lang/02-grammar.md).
- Retired `grammar.generic.binding.positions-key` (lang/02-grammar.md).
- Retired `grammar.generic.default` (lang/02-grammar.md).
- Retired `grammar.generic.default.positions` (lang/02-grammar.md).
- Retired `grammar.type.row.plus-rows-only` (lang/02-grammar.md).
- Retired `grammar.type.row.old-separator` (lang/02-grammar.md).
- Retired `grammar.type.mut` (lang/02-grammar.md).
- Retired `grammar.type.mut.semantic` (lang/02-grammar.md).
- Retired `grammar.type.mut.no-function` (lang/02-grammar.md).
- Retired `grammar.use.old-import` (lang/02-grammar.md).
- Retired `grammar.use.old-export` (lang/02-grammar.md).
- Retired `grammar.primary.field-copy` (lang/02-grammar.md).
- Retired `grammar.primary.prefix-copy-meaning` (lang/02-grammar.md).
- Retired `grammar.primary.prefix-copies` (lang/02-grammar.md).
- Retired `names.use.cycles-in-folder` (lang/03-names-and-scopes.md).
- Retired `names.use.folder-cycle` (lang/03-names-and-scopes.md).
- Retired `names.literal-fn.bare-in-scope` (lang/03-names-and-scopes.md).
- Retired `names.literal-fn.unknown-name` (lang/03-names-and-scopes.md).
- Retired `names.local.mutation` (lang/03-names-and-scopes.md).
- Retired `names.self.mut` (lang/03-names-and-scopes.md).
- Retired `names.take-part.definition` (lang/03-names-and-scopes.md).
- Retired `names.take-part.uniform` (lang/03-names-and-scopes.md).
- Retired `types.forms.set` (lang/04-type-system.md).
- Retired `types.gadt-equalities` (lang/04-type-system.md).
- Retired `types.never.expressions` (lang/04-type-system.md).
- Retired `types.prim.no-mut` (lang/04-type-system.md).
- Retired `types.prim.no-mut.error` (lang/04-type-system.md).
- Retired `types.string.split-scalar` (lang/04-type-system.md).
- Retired `types.literal.local.group` (lang/04-type-system.md).
- Retired `types.literal.local.join` (lang/04-type-system.md).
- Retired `types.literal.local.join.signed` (lang/04-type-system.md).
- Retired `types.literal.local.join.none` (lang/04-type-system.md).
- Retired `types.literal.local.erased` (lang/04-type-system.md).
- Retired `types.literal.call-result` (lang/04-type-system.md).
- Retired `types.tuple.no-mut` (lang/04-type-system.md).
- Retired `types.tuple.element-permission` (lang/04-type-system.md).
- Retired `types.alias.usize` (lang/04-type-system.md).
- Retired `types.alias.usize.prelude` (lang/04-type-system.md).
- Retired `types.alias.usize.sizes` (lang/04-type-system.md).
- Retired `types.alias.usize.cast` (lang/04-type-system.md).
- Retired `types.newtype.construct-permission` (lang/04-type-system.md).
- Retired `types.newtype.unwrap-permission` (lang/04-type-system.md).
- Retired `types.option.any` (lang/04-type-system.md).
- Retired `types.option.identity` (lang/04-type-system.md).
- Retired `types.option.identity.none` (lang/04-type-system.md).
- Retired `types.option.identity.some` (lang/04-type-system.md).
- Retired `types.option.variance.mutable` (lang/04-type-system.md).
- Retired `types.assign.weaken` (lang/04-type-system.md).
- Retired `types.assign.trait-value` (lang/04-type-system.md).
- Retired `types.assign.supertrait` (lang/04-type-system.md).
- Retired `types.assign.trait-value.mut` (lang/04-type-system.md).
- Retired `types.composite.kinds` (lang/04-type-system.md).
- Retired `types.composite.shared` (lang/04-type-system.md).
- Retired `types.composite.aliases` (lang/04-type-system.md).
- Retired `types.view.readonly` (lang/04-type-system.md).
- Retired `types.view.mutable` (lang/04-type-system.md).
- Retired `types.view.term` (lang/04-type-system.md).
- Retired `types.mut.permission` (lang/04-type-system.md).
- Retired `types.mut.no-field-reassign` (lang/04-type-system.md).
- Retired `types.mut.mut-field` (lang/04-type-system.md).
- Retired `types.mut.generic-field` (lang/04-type-system.md).
- Retired `types.mut.other-forms` (lang/04-type-system.md).
- Retired `types.mut.weaken` (lang/04-type-system.md).
- Retired `types.mut.no-upgrade` (lang/04-type-system.md).
- Retired `types.readonly.blocks` (lang/04-type-system.md).
- Retired `types.readonly.not-deep` (lang/04-type-system.md).
- Retired `types.readonly.nested` (lang/04-type-system.md).
- Retired `types.readonly.deep-api` (lang/04-type-system.md).
- Retired `types.readonly.alias` (lang/04-type-system.md).
- Retired `types.readonly.no-guarantee` (lang/04-type-system.md).
- Retired `types.readonly.structural` (lang/04-type-system.md).
- Retired `types.readonly.results` (lang/04-type-system.md).
- Retired `types.fresh.mutable-outer` (lang/04-type-system.md).
- Retired `types.fresh.tuple` (lang/04-type-system.md).
- Retired `types.fresh.weaken` (lang/04-type-system.md).
- Retired `types.fresh.weaken.meet` (lang/04-type-system.md).
- Retired `types.fresh.not-recursive` (lang/04-type-system.md).
- Retired `types.fresh.element-permission` (lang/04-type-system.md).
- Retired `types.fresh.element-no-weaken` (lang/04-type-system.md).
- Retired `types.fresh.readonly-field` (lang/04-type-system.md).
- Retired `types.fresh.mut-literal` (lang/04-type-system.md).
- Retired `types.fresh.spread` (lang/04-type-system.md).
- Retired `types.fresh.expected-readonly` (lang/04-type-system.md).
- Retired `types.fresh.expected-mut` (lang/04-type-system.md).
- Retired `types.fresh.expected-mut.sites` (lang/04-type-system.md).
- Retired `types.fresh.generic-field` (lang/04-type-system.md).
- Retired `types.bind.short` (lang/04-type-system.md).
- Retired `types.bind.let-mut` (lang/04-type-system.md).
- Retired `types.bind.let-readonly` (lang/04-type-system.md).
- Retired `types.bind.let-mut-infer` (lang/04-type-system.md).
- Retired `types.bind.let-mut-upgrade` (lang/04-type-system.md).
- Retired `types.bind.let-mut-primitive` (lang/04-type-system.md).
- Retired `types.bind.let-mut-tuple` (lang/04-type-system.md).
- Retired `types.bind.let-mut-tuple.hint` (lang/04-type-system.md).
- Retired `types.bind.let-mut-optional` (lang/04-type-system.md).
- Retired `types.bind.let-mut-expected` (lang/04-type-system.md).
- Retired `types.bind.let-mut-copy` (lang/04-type-system.md).
- Retired `types.bind.let-mut-annotated` (lang/04-type-system.md).
- Retired `types.bind.let-mut-annotated.warning` (lang/04-type-system.md).
- Retired `types.bind.let-mut-annotated.fix` (lang/04-type-system.md).
- Retired `types.bind.let-mut-annotation` (lang/04-type-system.md).
- Retired `types.bind.let-mut-annotation.fix` (lang/04-type-system.md).
- Retired `types.bind.let-pattern-mut.data` (lang/04-type-system.md).
- Retired `types.bind.let-mut-pattern.annotated` (lang/04-type-system.md).
- Retired `types.bind.let-mut-pattern.redundant` (lang/04-type-system.md).
- Retired `types.bind.call-result` (lang/04-type-system.md).
- Retired `types.bind.call-result.argument` (lang/04-type-system.md).
- Retired `types.path.access-type` (lang/04-type-system.md).
- Retired `types.path.root` (lang/04-type-system.md).
- Retired `types.path.call` (lang/04-type-system.md).
- Retired `types.path.field` (lang/04-type-system.md).
- Retired `types.path.field.mutable-edge` (lang/04-type-system.md).
- Retired `types.path.field.readonly-edge` (lang/04-type-system.md).
- Retired `types.path.field.embedded` (lang/04-type-system.md).
- Retired `types.path.field.generic` (lang/04-type-system.md).
- Retired `types.path.field.mutable-edge.removed` (lang/04-type-system.md).
- Retired `types.path.field.generic.example` (lang/04-type-system.md).
- Retired `types.path.field.substituted` (lang/04-type-system.md).
- Retired `types.path.collection` (lang/04-type-system.md).
- Retired `types.path.collection.example` (lang/04-type-system.md).
- Retired `types.path.contents` (lang/04-type-system.md).
- Retired `types.path.enum-payload` (lang/04-type-system.md).
- Retired `types.path.access.mut-type` (lang/04-type-system.md).
- Retired `types.path.access.mut-bound` (lang/04-type-system.md).
- Retired `types.path.access.mut-self` (lang/04-type-system.md).
- Retired `types.path.mutation` (lang/04-type-system.md).
- Retired `types.path.mutation.needs-access` (lang/04-type-system.md).
- Retired `types.path.mutation.only` (lang/04-type-system.md).
- Retired `types.path.mutation.receiver` (lang/04-type-system.md).
- Retired `types.path.store.readonly-edge` (lang/04-type-system.md).
- Retired `types.path.store.readonly-root` (lang/04-type-system.md).
- Retired `types.path.readonly-root.cases` (lang/04-type-system.md).
- Retired `types.path.reassign` (lang/04-type-system.md).
- Retired `types.path.reassign.old-value` (lang/04-type-system.md).
- Retired `types.path.store-mut-edge` (lang/04-type-system.md).
- Retired `types.path.store-readonly` (lang/04-type-system.md).
- Retired `types.path.store-embedded` (lang/04-type-system.md).
- Retired `types.path.container-element` (lang/04-type-system.md).
- Retired `types.path.generic-args` (lang/04-type-system.md).
- Retired `types.path.loses-mut` (lang/04-type-system.md).
- Retired `types.path.patterns` (lang/04-type-system.md).
- Retired `types.param.readonly` (lang/04-type-system.md).
- Retired `types.param.mut` (lang/04-type-system.md).
- Retired `types.param.mut-self` (lang/04-type-system.md).
- Retired `types.return.mut` (lang/04-type-system.md).
- Retired `types.return.readonly` (lang/04-type-system.md).
- Retired `types.generic.complete` (lang/04-type-system.md).
- Retired `types.generic.mut-argument` (lang/04-type-system.md).
- Retired `types.generic.direct` (lang/04-type-system.md).
- Retired `types.generic.no-mut-t` (lang/04-type-system.md).
- Retired `types.generic.mut-bound` (lang/04-type-system.md).
- Retired `types.generic.mut-any` (lang/04-type-system.md).
- Retired `types.generic.mut-trait` (lang/04-type-system.md).
- Retired `types.generic.trait-bound` (lang/04-type-system.md).
- Retired `types.generic.infer.join.outer-permission` (lang/04-type-system.md).
- Retired `types.generic.infer.join.explicit` (lang/04-type-system.md).
- Retired `types.generic.default.form` (lang/04-type-system.md).
- Retired `types.variance.gadt` (lang/04-type-system.md).
- Retired `types.variance.gadt.example` (lang/04-type-system.md).
- Retired `types.variance.repr.weakening` (lang/04-type-system.md).
- Retired `types.trait.value` (lang/04-type-system.md).
- Retired `types.trait.value.contents` (lang/04-type-system.md).
- Retired `types.trait.value.no-dyn` (lang/04-type-system.md).
- Retired `types.trait.value.bound` (lang/04-type-system.md).
- Retired `types.trait.value.bindings` (lang/04-type-system.md).
- Retired `types.trait.safe` (lang/04-type-system.md).
- Retired `types.trait.safe.one-copy` (lang/04-type-system.md).
- Retired `types.trait.safe.members-bound` (lang/04-type-system.md).
- Retired `types.trait.safe.assoc-bound` (lang/04-type-system.md).
- Retired `types.trait.safe.method-type-param-implied` (lang/04-type-system.md).
- Retired `types.trait.safe.one-body` (lang/04-type-system.md).
- Retired `types.trait.safe.convert-value` (lang/04-type-system.md).
- Retired `types.trait.safe.trait-generic` (lang/04-type-system.md).
- Retired `types.trait.safe.static` (lang/04-type-system.md).
- Retired `types.any.erase` (lang/04-type-system.md).
- Retired `types.any.mut` (lang/04-type-system.md).
- Retired `types.sealed.anyval-types` (lang/04-type-system.md).
- Retired `types.sealed.anyref` (lang/04-type-system.md).
- Retired `types.sealed.permission` (lang/04-type-system.md).
- Retired `types.map-key.no-mut` (lang/04-type-system.md).
- Retired `types.map.order` (lang/04-type-system.md).
- Retired `types.map.replace` (lang/04-type-system.md).
- Retired `types.map.key-view` (lang/04-type-system.md).
- Retired `types.map.ghost` (lang/04-type-system.md).
- Retired `types.lct.no-any` (lang/04-type-system.md).
- Retired `types.lct.expected-trait` (lang/04-type-system.md).
- Retired `types.lct.no-least` (lang/04-type-system.md).
- Retired `types.unsupported.no-unions` (lang/04-type-system.md).
- Retired `expr.place.field` (lang/05-expressions.md).
- Retired `expr.place.tuple-element` (lang/05-expressions.md).
- Retired `expr.assign.compound.index-read-write` (lang/05-expressions.md).
- Retired `expr.interp.append` (lang/05-expressions.md).
- Retired `expr.suffix.fn-call` (lang/05-expressions.md).
- Retired `expr.map.duplicate` (lang/05-expressions.md).
- Retired `expr.map.iteration` (lang/05-expressions.md).
- Retired `expr.update.access-view` (lang/05-expressions.md).
- Retired `expr.update.readonly-source` (lang/05-expressions.md).
- Retired `expr.update.readonly-fill` (lang/05-expressions.md).
- Retired `expr.update.generic` (lang/05-expressions.md).
- Retired `expr.update.mutable-source` (lang/05-expressions.md).
- Retired `expr.update.shallow` (lang/05-expressions.md).
- Retired `expr.update.no-upgrade` (lang/05-expressions.md).
- Retired `expr.member.readonly-root` (lang/05-expressions.md).
- Retired `expr.member.readonly-generic` (lang/05-expressions.md).
- Retired `expr.member.other-forms` (lang/05-expressions.md).
- Retired `expr.index.expected.range` (lang/05-expressions.md).
- Retired `expr.index.negative-literal` (lang/05-expressions.md).
- Retired `expr.index.slice.half-open` (lang/05-expressions.md).
- Retired `expr.index.slice.from` (lang/05-expressions.md).
- Retired `expr.index.slice.to` (lang/05-expressions.md).
- Retired `expr.index.slice.inclusive` (lang/05-expressions.md).
- Retired `expr.index.slice.to-inclusive` (lang/05-expressions.md).
- Retired `expr.index.slice.full` (lang/05-expressions.md).
- Retired `expr.index.slice.call` (lang/05-expressions.md).
- Retired `expr.index.slice.string` (lang/05-expressions.md).
- Retired `expr.index.slice.string.shared` (lang/05-expressions.md).
- Retired `expr.index.slice.string.range` (lang/05-expressions.md).
- Retired `expr.index.slice.string.reversed` (lang/05-expressions.md).
- Retired `expr.index.slice.string.boundary` (lang/05-expressions.md).
- Retired `expr.index.slice.list` (lang/05-expressions.md).
- Retired `expr.index.slice.list.type` (lang/05-expressions.md).
- Retired `expr.index.slice.list.range` (lang/05-expressions.md).
- Retired `expr.index.slice.list.reversed` (lang/05-expressions.md).
- Retired `expr.index.slice.no-store` (lang/05-expressions.md).
- Retired `expr.index.slice.unsigned` (lang/05-expressions.md).
- Retired `expr.index.trait.std` (lang/05-expressions.md).
- Retired `expr.index.trait.read-other` (lang/05-expressions.md).
- Retired `expr.index.trait.write` (lang/05-expressions.md).
- Retired `expr.index.trait.no-use` (lang/05-expressions.md).
- Retired `expr.index.trait.mut` (lang/05-expressions.md).
- Retired `expr.index.trait.place` (lang/05-expressions.md).
- Retired `expr.index.trait.independent` (lang/05-expressions.md).
- Retired `expr.index.std.list.usize` (lang/05-expressions.md).
- Retired `expr.index.std.map` (lang/05-expressions.md).
- Retired `expr.index.std.string.range.unsigned` (lang/05-expressions.md).
- Retired `expr.index.std.list.range.unsigned` (lang/05-expressions.md).
- Retired `expr.index.std.string.full` (lang/05-expressions.md).
- Retired `expr.index.std.list.full` (lang/05-expressions.md).
- Retired `expr.index.std.intrinsic` (lang/05-expressions.md).
- Retired `expr.call.spread.one` (lang/05-expressions.md).
- Retired `expr.call.apply.std` (lang/05-expressions.md).
- Retired `expr.call.apply.write` (lang/05-expressions.md).
- Retired `expr.call.apply.mut` (lang/05-expressions.md).
- Retired `expr.call.apply.readonly` (lang/05-expressions.md).
- Retired `expr.try.convert.assignable.covers` (lang/05-expressions.md).
- Retired `expr.try.convert.strip-mut` (lang/05-expressions.md).
- Retired `expr.try.convert.message` (lang/05-expressions.md).
- Retired `expr.try.convert.single-rule` (lang/05-expressions.md).
- Retired `expr.try.convert.before-defer` (lang/05-expressions.md).
- Retired `expr.try.test.with-try` (lang/05-expressions.md).
- Retired `expr.try.test.converts` (lang/05-expressions.md).
- Retired `expr.shift.count-u32` (lang/05-expressions.md).
- Retired `expr.shift.count-other` (lang/05-expressions.md).
- Retired `expr.power.float.pow` (lang/05-expressions.md).
- Retired `expr.eq.readonly-view` (lang/05-expressions.md).
- Retired `expr.ord.unsigned-zero` (lang/05-expressions.md).
- Retired `expr.is.heap` (lang/05-expressions.md).
- Retired `expr.is.function-unspecified` (lang/05-expressions.md).
- Retired `expr.is.box` (lang/05-expressions.md).
- Retired `expr.is.box.identity` (lang/05-expressions.md).
- Retired `expr.is.box.distinct` (lang/05-expressions.md).
- Retired `expr.is.canonical` (lang/05-expressions.md).
- Retired `expr.is.canonical.same` (lang/05-expressions.md).
- Retired `expr.is.shared-data-canonical` (lang/05-expressions.md).
- Retired `expr.is.none` (lang/05-expressions.md).
- Retired `expr.is.some` (lang/05-expressions.md).
- Retired `expr.is.primitive` (lang/05-expressions.md).
- Retired `expr.is.compatible` (lang/05-expressions.md).
- Retired `expr.is.permissions` (lang/05-expressions.md).
- Retired `expr.is.incompatible` (lang/05-expressions.md).
- Retired `expr.is.function` (lang/05-expressions.md).
- Retired `expr.is.function.generic` (lang/05-expressions.md).
- Retired `expr.op.std.shift-u32` (lang/05-expressions.md).
- Retired `expr.range.declared.four` (lang/05-expressions.md).
- Retired `expr.range.bound.integer` (lang/05-expressions.md).
- Retired `expr.unsupported.any-fallback` (lang/05-expressions.md).
- Retired `flow.unused.warning` (lang/06-control-flow.md).
- Retired `flow.for.iterator-type` (lang/06-control-flow.md).
- Retired `flow.for.iterator-private` (lang/06-control-flow.md).
- Retired `flow.for.iterator-from-fn` (lang/06-control-flow.md).
- Retired `flow.for.iterator-next` (lang/06-control-flow.md).
- Retired `flow.for.iterator-from-fn.no-fuse` (lang/06-control-flow.md).
- Retired `flow.for.iterator-not-iterable` (lang/06-control-flow.md).
- Retired `flow.for.accepts` (lang/06-control-flow.md).
- Retired `flow.for.iterable` (lang/06-control-flow.md).
- Retired `flow.for.iterator-direct` (lang/06-control-flow.md).
- Retired `flow.for.iterator.position` (lang/06-control-flow.md).
- Retired `flow.for.iterator-mut` (lang/06-control-flow.md).
- Retired `flow.for.not-iterable-or-iterator` (lang/06-control-flow.md).
- Retired `flow.for.iterator-no-bound` (lang/06-control-flow.md).
- Retired `flow.for.map` (lang/06-control-flow.md).
- Retired `flow.for.map.value` (lang/06-control-flow.md).
- Retired `flow.for.no-root-grant` (lang/06-control-flow.md).
- Retired `flow.for.length` (lang/06-control-flow.md).
- Retired `flow.for.invalidate` (lang/06-control-flow.md).
- Retired `flow.for.invalidate.panic` (lang/06-control-flow.md).
- Retired `flow.for.replace` (lang/06-control-flow.md).
- Retired `flow.for.replace.observed` (lang/06-control-flow.md).
- Retired `flow.loop.void` (lang/06-control-flow.md).
- Retired `flow.match.payload.name-mismatch` (lang/06-control-flow.md).
- Retired `flow.match.variant.shorthand-rules` (lang/06-control-flow.md).
- Retired `flow.match.no-ownership` (lang/06-control-flow.md).
- Retired `flow.match.bind.composite` (lang/06-control-flow.md).
- Retired `flow.match.data.bind-composite` (lang/06-control-flow.md).
- Retired `flow.match.data.readonly-mut` (lang/06-control-flow.md).
- Retired `flow.let.tuple-arity` (lang/06-control-flow.md).
- Retired `flow.defer.register` (lang/06-control-flow.md).
- Retired `flow.defer.not-run` (lang/06-control-flow.md).
- Retired `flow.defer.run` (lang/06-control-flow.md).
- Retired `flow.defer.loop` (lang/06-control-flow.md).
- Retired `flow.defer.result-saved` (lang/06-control-flow.md).
- Retired `flow.defer.operands` (lang/06-control-flow.md).
- Retired `flow.defer.captures` (lang/06-control-flow.md).
- Retired `flow.defer.scopes` (lang/06-control-flow.md).
- Retired `flow.defer.not-scopes` (lang/06-control-flow.md).
- Retired `flow.defer.outside` (lang/06-control-flow.md).
- Retired `flow.defer.void` (lang/06-control-flow.md).
- Retired `flow.defer.restricted` (lang/06-control-flow.md).
- Retired `flow.defer.suspend` (lang/06-control-flow.md).
- Retired `flow.defer.control` (lang/06-control-flow.md).
- Retired `flow.defer.block-on` (lang/06-control-flow.md).
- Retired `flow.defer.panic` (lang/06-control-flow.md).
- Retired `flow.defer.cleanup-panic` (lang/06-control-flow.md).
- Retired `flow.defer.alias-escape` (lang/06-control-flow.md).
- Retired `flow.unreachable.warning` (lang/06-control-flow.md).
- Retired `flow.panic.stable-categories` (lang/06-control-flow.md).
- Retired `fn.param.non-reassignable` (lang/07-functions.md).
- Retired `fn.param.access` (lang/07-functions.md).
- Retired `fn.param.composite` (lang/07-functions.md).
- Retired `fn.type.ctor.decl` (lang/07-functions.md).
- Retired `fn.ref.unbound.mut-self` (lang/07-functions.md).
- Retired `fn.ref.bound.mut` (lang/07-functions.md).
- Retired `fn.capture.storage.shared` (lang/07-functions.md).
- Retired `fn.capture.serializable-deferred` (lang/07-functions.md).
- Retired `fn.method.form` (lang/07-functions.md).
- Retired `fn.method.self` (lang/07-functions.md).
- Retired `fn.method.mut-self` (lang/07-functions.md).
- Retired `fn.method.no-sigil` (lang/07-functions.md).
- Retired `data.kind.data` (lang/08-data-and-enums.md).
- Retired `data.empty.canonical` (lang/08-data-and-enums.md).
- Retired `data.recursive.direct` (lang/08-data-and-enums.md).
- Retired `data.ref.shared` (lang/08-data-and-enums.md).
- Retired `data.ref.views` (lang/08-data-and-enums.md).
- Retired `data.decl.no-struct` (lang/08-data-and-enums.md).
- Retired `data.update.access-view` (lang/08-data-and-enums.md).
- Retired `data.update.readonly-source` (lang/08-data-and-enums.md).
- Retired `data.update.mutable-result` (lang/08-data-and-enums.md).
- Retired `data.update.generic` (lang/08-data-and-enums.md).
- Retired `data.access.mut-root` (lang/08-data-and-enums.md).
- Retired `data.access.readonly-root` (lang/08-data-and-enums.md).
- Retired `data.access.nested` (lang/08-data-and-enums.md).
- Retired `data.access.readonly-mut-field` (lang/08-data-and-enums.md).
- Retired `data.access.construct-mut-field` (lang/08-data-and-enums.md).
- Retired `data.access.generic` (lang/08-data-and-enums.md).
- Retired `data.embed.data-only` (lang/08-data-and-enums.md).
- Retired `data.part.construct` (lang/08-data-and-enums.md).
- Retired `data.part.copy-update` (lang/08-data-and-enums.md).
- Retired `data.part.marker-required` (lang/08-data-and-enums.md).
- Retired `data.part.copy-time` (lang/08-data-and-enums.md).
- Retired `data.part.access` (lang/08-data-and-enums.md).
- Retired `data.part.aliases-untracked` (lang/08-data-and-enums.md).
- Retired `data.part.unobservable` (lang/08-data-and-enums.md).
- Retired `data.shared.compile-time` (lang/08-data-and-enums.md).
- Retired `data.shared.default.requirement-free` (lang/08-data-and-enums.md).
- Retired `data.shared.permissions` (lang/08-data-and-enums.md).
- Retired `data.shared.readonly` (lang/08-data-and-enums.md).
- Retired `data.enum.generic` (lang/08-data-and-enums.md).
- Retired `data.enum.generic.result` (lang/08-data-and-enums.md).
- Retired `data.repr.backend` (lang/08-data-and-enums.md).
- Retired `data.repr.no-manual` (lang/08-data-and-enums.md).
- Retired `data.repr.runtime-only` (lang/08-data-and-enums.md).
- Retired `data.repr.defer` (lang/08-data-and-enums.md).
- Retired `data.gadt.declare` (lang/08-data-and-enums.md).
- Retired `data.gadt.refine` (lang/08-data-and-enums.md).
- Retired `data.derive.gadt` (lang/08-data-and-enums.md).
- Retired `trait.decl.method` (lang/09-traits.md).
- Retired `trait.derive.partners.hash` (lang/09-traits.md).
- Retired `trait.derive.partners.partial-ord` (lang/09-traits.md).
- Retired `trait.derive.partners.ord` (lang/09-traits.md).
- Retired `trait.derive.partners.same-list` (lang/09-traits.md).
- Retired `trait.derive.partners.no-mix` (lang/09-traits.md).
- Retired `trait.derive.partners.error` (lang/09-traits.md).
- Retired `trait.from.no-fit` (lang/09-traits.md).
- Retired `trait.error.supertraits-anyref` (lang/09-traits.md).
- Retired `trait.error.complete-reference` (lang/09-traits.md).
- Retired `trait.error.not-anyref` (lang/09-traits.md).
- Retired `trait.error.cause` (lang/09-traits.md).
- Retired `trait.error.chain-methods` (lang/09-traits.md).
- Retired `trait.error.erased` (lang/09-traits.md).
- Retired `trait.error.result` (lang/09-traits.md).
- Retired `trait.error.entry-point` (lang/09-traits.md).
- Retired `trait.error.boundary` (lang/09-traits.md).
- Retired `trait.error.convert-first` (lang/09-traits.md).
- Retired `trait.error.downcast` (lang/09-traits.md).
- Retired `trait.debug.method` (lang/09-traits.md).
- Retired `trait.impl.override` (lang/09-traits.md).
- Retired `trait.impl.generics.fixed-bounds` (lang/09-traits.md).
- Retired `trait.target.trait-value.argument` (lang/09-traits.md).
- Retired `trait.target.no-mut` (lang/09-traits.md).
- Retired `trait.target.permission` (lang/09-traits.md).
- Retired `trait.target.both-views` (lang/09-traits.md).
- Retired `trait.target.mut-self` (lang/09-traits.md).
- Retired `trait.target.inner-mut` (lang/09-traits.md).
- Retired `trait.target.inner-mut.distinct` (lang/09-traits.md).
- Retired `trait.own.std.function` (lang/09-traits.md).
- Retired `trait.own.inherent.target-kinds` (lang/09-traits.md).
- Retired `trait.own.inherent.trait-value` (lang/09-traits.md).
- Retired `trait.own.module.inherent.std` (lang/09-traits.md).
- Retired `trait.resolve.fits` (lang/09-traits.md).
- Retired `trait.resolve.fits.expected` (lang/09-traits.md).
- Retired `trait.resolve.one-fit` (lang/09-traits.md).
- Retired `trait.resolve.literal-arg.no-fit` (lang/09-traits.md).
- Retired `trait.resolve.many-fit` (lang/09-traits.md).
- Retired `trait.resolve.no-fit` (lang/09-traits.md).
- Retired `trait.bound.mut` (lang/09-traits.md).
- Retired `trait.bound.mut-any` (lang/09-traits.md).
- Retired `trait.dyn.form` (lang/09-traits.md).
- Retired `trait.dyn.contents` (lang/09-traits.md).
- Retired `trait.dyn.no-dyn` (lang/09-traits.md).
- Retired `trait.dyn.methods-supertraits` (lang/09-traits.md).
- Retired `trait.dyn.safe` (lang/09-traits.md).
- Retired `trait.dyn.safe.one-copy` (lang/09-traits.md).
- Retired `trait.dyn.safe.error` (lang/09-traits.md).
- Retired `trait.dyn.safe.assoc-type` (lang/09-traits.md).
- Retired `trait.dyn.safe.assoc-function` (lang/09-traits.md).
- Retired `trait.dyn.safe.implied-anyref-param` (lang/09-traits.md).
- Retired `trait.dyn.safe.self` (lang/09-traits.md).
- Retired `trait.dyn.safe.suspending` (lang/09-traits.md).
- Retired `trait.dyn.safe.row-parameter` (lang/09-traits.md).
- Retired `trait.dyn.static-still` (lang/09-traits.md).
- Retired `trait.dyn.binding.form` (lang/09-traits.md).
- Retired `trait.dyn.binding.complete` (lang/09-traits.md).
- Retired `trait.dyn.binding.convert` (lang/09-traits.md).
- Retired `trait.dyn.binding.widen` (lang/09-traits.md).
- Retired `trait.dyn.binding.no-function` (lang/09-traits.md).
- Retired `trait.dyn.bound` (lang/09-traits.md).
- Retired `trait.dyn.bound.projection` (lang/09-traits.md).
- Retired `trait.dyn.bound.binding` (lang/09-traits.md).
- Retired `trait.dyn.bound.instantiation` (lang/09-traits.md).
- Retired `trait.dyn.bound.dispatch` (lang/09-traits.md).
- Retired `trait.dyn.bound.mut` (lang/09-traits.md).
- Retired `trait.dyn.bound.no-impl` (lang/09-traits.md).
- Retired `trait.dyn.convert` (lang/09-traits.md).
- Retired `trait.dyn.mut` (lang/09-traits.md).
- Retired `trait.any.erases` (lang/09-traits.md).
- Retired `trait.any.optional` (lang/09-traits.md).
- Retired `trait.any.none` (lang/09-traits.md).
- Retired `trait.any.mut` (lang/09-traits.md).
- Retired `trait.sealed.anyval-types` (lang/09-traits.md).
- Retired `trait.sealed.anyref` (lang/09-traits.md).
- Retired `trait.sealed.suspend` (lang/09-traits.md).
- Retired `trait.rtti.import` (lang/09-traits.md).
- Retired `trait.rtti.surface` (lang/09-traits.md).
- Retired `trait.rtti.defaults` (lang/09-traits.md).
- Retired `trait.inspect.safe` (lang/09-traits.md).
- Retired `trait.inspect.supplied.members` (lang/09-traits.md).
- Retired `trait.typeid.opaque` (lang/09-traits.md).
- Retired `trait.identity.inner-mut` (lang/09-traits.md).
- Retired `trait.identity.outer-mut` (lang/09-traits.md).
- Retired `trait.identity.inner-permission` (lang/09-traits.md).
- Retired `trait.identity.no-conversion` (lang/09-traits.md).
- Retired `trait.typeid.name.inner-mut` (lang/09-traits.md).
- Retired `trait.typeid.name.outer-mut` (lang/09-traits.md).
- Retired `trait.inspectable.dynamic` (lang/09-traits.md).
- Retired `trait.inspectable.argument-only` (lang/09-traits.md).
- Retired `trait.inspectable.argument-examples` (lang/09-traits.md).
- Retired `trait.inspectable.dynamic-values` (lang/09-traits.md).
- Retired `trait.inspectable.one-way` (lang/09-traits.md).
- Retired `trait.erase.expected` (lang/09-traits.md).
- Retired `trait.erase.recorded` (lang/09-traits.md).
- Retired `trait.erase.weakened` (lang/09-traits.md).
- Retired `trait.erase.bound-required` (lang/09-traits.md).
- Retired `trait.erase.widen` (lang/09-traits.md).
- Retired `trait.erase.mut` (lang/09-traits.md).
- Retired `trait.erase.mut.source` (lang/09-traits.md).
- Retired `trait.downcast.some` (lang/09-traits.md).
- Retired `trait.downcast.mut` (lang/09-traits.md).
- Retired `trait.downcast.val` (lang/09-traits.md).
- Retired `trait.downcast.val.readonly` (lang/09-traits.md).
- Retired `trait.downcast.exact` (lang/09-traits.md).
- Retired `trait.downcast.optional` (lang/09-traits.md).
- Retired `trait.downcast.same-reference` (lang/09-traits.md).
- Retired `trait.downcast.unboxed` (lang/09-traits.md).
- Retired `trait.downcast.ordinary` (lang/09-traits.md).
- Retired `trait.downcast.val-ordinary` (lang/09-traits.md).
- Retired `trait.downcast.no-special` (lang/09-traits.md).
- Retired `trait.downcast.pass-on` (lang/09-traits.md).
- Retired `trait.downcast.readonly` (lang/09-traits.md).
- Retired `trait.downcast.mut-receiver` (lang/09-traits.md).
- Retired `trait.downcast.bound.examples` (lang/09-traits.md).
- Retired `trait.limit.signatures` (lang/09-traits.md).
- Retired `trait.limit.deterministic` (lang/09-traits.md).
- Retired `trait.by.trait-less` (lang/09-traits.md).
- Retired `trait.by.trait-less.error` (lang/09-traits.md).
- Retired `trait.by.generated` (lang/09-traits.md).
- Retired `trait.unsupported.list` (lang/09-traits.md).
- Retired `module.dep.requirement-value` (lang/10-modules.md).
- Retired `module.repo.private-pattern` (lang/10-modules.md).
- Retired `module.workspace.path-requirement` (lang/10-modules.md).
- Retired `module.test.integration.beside-dir` (lang/10-modules.md).
- Retired `module.test.integration.shared-unused` (lang/10-modules.md).
- Retired `module.test.doc.row` (lang/10-modules.md).
- Retired `module.folder.holder` (lang/10-modules.md).
- Retired `module.folder.parent-file` (lang/10-modules.md).
- Retired `module.folder.interchangeable` (lang/10-modules.md).
- Retired `module.folder.parent-child` (lang/10-modules.md).
- Retired `module.folder.mod-file` (lang/10-modules.md).
- Retired `module.folder.nested` (lang/10-modules.md).
- Retired `module.cycle.folder-edge` (lang/10-modules.md).
- Retired `module.cycle.nested` (lang/10-modules.md).
- Retired `module.cycle.acyclic` (lang/10-modules.md).
- Retired `module.cycle.within-folder` (lang/10-modules.md).
- Retired `module.cycle.diagnostic.loop` (lang/10-modules.md).
- Retired `module.cycle.diagnostic.size` (lang/10-modules.md).
- Retired `module.cycle.diagnostic.fix` (lang/10-modules.md).
- Retired `module.prelude.in-scope` (lang/10-modules.md).
- Retired `module.prelude.test-only` (lang/10-modules.md).
- Retired `module.prelude.inspect` (lang/10-modules.md).
- Retired `module.console.write-line-mut` (lang/10-modules.md).
- Retired `module.console.write-error-line` (lang/10-modules.md).
- Retired `module.console.write-error-line.default` (lang/10-modules.md).
- Retired `module.console.error.closed` (lang/10-modules.md).
- Retired `module.console.println-write` (lang/10-modules.md).
- Retired `module.console.println-drive.block-on` (lang/10-modules.md).
- Retired `module.console.println-drive.pending` (lang/10-modules.md).
- Retired `module.console.println-error` (lang/10-modules.md).
- Retired `module.console.println-block-on` (lang/10-modules.md).
- Retired `module.console.println-block-on.under-driver` (lang/10-modules.md).
- Retired `module.console.println-block-on.contexts` (lang/10-modules.md).
- Retired `module.console.println-script` (lang/10-modules.md).
- Retired `module.process.declares` (lang/10-modules.md).
- Retired `module.process.run` (lang/10-modules.md).
- Retired `module.process.ok-output` (lang/10-modules.md).
- Retired `module.process.not-found` (lang/10-modules.md).
- Retired `module.process.permission-denied` (lang/10-modules.md).
- Retired `module.process.mut` (lang/10-modules.md).
- Retired `module.process.import.names` (lang/10-modules.md).
- Retired `module.prelude.anyref` (lang/10-modules.md).
- Retired `module.prelude.anyref-not` (lang/10-modules.md).
- Retired `module.prelude.anyval-types` (lang/10-modules.md).
- Retired `module.method.list-remove-at` (lang/10-modules.md).
- Retired `module.method.list-remove-at.range` (lang/10-modules.md).
- Retired `module.testing.exports` (lang/10-modules.md).
- Retired `module.testing.reason` (lang/10-modules.md).
- Retired `module.testing.shows-reason` (lang/10-modules.md).
- Retired `module.testing.reg.functions` (lang/10-modules.md).
- Retired `module.testing.reg.body-result` (lang/10-modules.md).
- Retired `module.testing.reg.body-closure-result` (lang/10-modules.md).
- Retired `module.testing.skipped` (lang/10-modules.md).
- Retired `module.testing.pass` (lang/10-modules.md).
- Retired `module.testing.fail` (lang/10-modules.md).
- Retired `module.testing.err-print` (lang/10-modules.md).
- Retired `module.testing.snapshot.literal` (lang/10-modules.md).
- Retired `module.init.statements-and-main` (lang/10-modules.md).
- Retired `module.init.group` (lang/10-modules.md).
- Retired `module.init.group.once` (lang/10-modules.md).
- Retired `module.init.group.ready-order` (lang/10-modules.md).
- Retired `module.init.group.dependency` (lang/10-modules.md).
- Retired `module.init.group.step` (lang/10-modules.md).
- Retired `module.init.group.earliest` (lang/10-modules.md).
- Retired `module.init.group.cycle` (lang/10-modules.md).
- Retired `module.init.requirement-free` (lang/10-modules.md).
- Retired `module.init.requirement-free.forms` (lang/10-modules.md).
- Retired `module.init.script-row` (lang/10-modules.md).
- Retired `module.init.script-row.report` (lang/10-modules.md).
- Retired `module.init.script-not-driver` (lang/10-modules.md).
- Retired `module.package.no-pub-binding` (lang/10-modules.md).
- Retired `module.interface.dictionaries` (lang/10-modules.md).
- Retired `module.interface.fact-values` (lang/10-modules.md).
- Retired `module.interface.determined-facts` (lang/10-modules.md).
- Retired `module.interface.early-facts` (lang/10-modules.md).
- Retired `module.entry.private-main.warn` (lang/10-modules.md).
- Retired `module.entry.err-dynamic` (lang/10-modules.md).
- Retired `module.entry.err-render-chain.line` (lang/10-modules.md).
- Retired `module.entry.err-render-display` (lang/10-modules.md).
- Retired `module.entry.termination.err-code` (lang/10-modules.md).
- Retired `module.profile.trait-access` (lang/10-modules.md).
- Retired `module.boundary.pub` (lang/10-modules.md).
- Retired `module.instance.thread` (lang/10-modules.md).
- Retired `module.instance.parallel` (lang/10-modules.md).
- Retired `module.host.runtime` (lang/10-modules.md).
- Retired `module.host.component-model` (lang/10-modules.md).
- Retired `module.resource.error` (lang/10-modules.md).
- Retired `module.resource.result` (lang/10-modules.md).
- Retired `module.resource.disposed` (lang/10-modules.md).
- Retired `module.resource.close` (lang/10-modules.md).
- Retired `req.row.syntax.old-separator` (lang/11-requirements-and-suspension.md).
- Retired `req.row.param.no-data` (lang/11-requirements-and-suspension.md).
- Retired `req.row.param.no-data.marked` (lang/11-requirements-and-suspension.md).
- Retired `req.row.param.no-data.error` (lang/11-requirements-and-suspension.md).
- Retired `req.row.param.no-newtype` (lang/11-requirements-and-suspension.md).
- Retired `req.row.alias.one-key` (lang/11-requirements-and-suspension.md).
- Retired `req.row.alias.no-mut` (lang/11-requirements-and-suspension.md).
- Retired `req.key.dynamically-safe` (lang/11-requirements-and-suspension.md).
- Retired `req.key.binding.value` (lang/11-requirements-and-suspension.md).
- Retired `req.use.value.ordinary` (lang/11-requirements-and-suspension.md).
- Retired `req.use.value.access` (lang/11-requirements-and-suspension.md).
- Retired `req.with.type` (lang/11-requirements-and-suspension.md).
- Retired `req.with.type.binding` (lang/11-requirements-and-suspension.md).
- Retired `req.mut.trait` (lang/11-requirements-and-suspension.md).
- Retired `req.mut.install-mutable` (lang/11-requirements-and-suspension.md).
- Retired `req.mut.install-readonly-trait` (lang/11-requirements-and-suspension.md).
- Retired `req.mut.use-mutable` (lang/11-requirements-and-suspension.md).
- Retired `req.mut.use-readonly-trait` (lang/11-requirements-and-suspension.md).
- Retired `req.mut.use.binding` (lang/11-requirements-and-suspension.md).
- Retired `req.suspend.type.equivalent` (lang/11-requirements-and-suspension.md).
- Retired `req.suspend.type.readonly-binding` (lang/11-requirements-and-suspension.md).
- Retired `req.bang.driver-contexts` (lang/11-requirements-and-suspension.md).
- Retired `req.bang.top-level` (lang/11-requirements-and-suspension.md).
- Retired `req.drive.block-on` (lang/11-requirements-and-suspension.md).
- Retired `req.drive.block-on.forbidden-contexts` (lang/11-requirements-and-suspension.md).
- Retired `req.drive.block-on.transitive` (lang/11-requirements-and-suspension.md).
- Retired `req.drive.block-on.unprovable` (lang/11-requirements-and-suspension.md).
- Retired `req.drive.block-on.error` (lang/11-requirements-and-suspension.md).
- Retired `req.host-wait.leaf` (lang/11-requirements-and-suspension.md).
- Retired `req.protocol.sealed` (lang/11-requirements-and-suspension.md).
- Retired `req.protocol.implementers` (lang/11-requirements-and-suspension.md).
- Retired `req.check.poll-cancelled` (lang/11-requirements-and-suspension.md).
- Retired `req.check.code.invalid-state` (lang/11-requirements-and-suspension.md).
- Retired `req.discard.abandoned` (lang/11-requirements-and-suspension.md).
- Retired `req.cancel.synchronous` (lang/11-requirements-and-suspension.md).
- Retired `req.cancel.defer` (lang/11-requirements-and-suspension.md).
- Retired `req.cancel.hook-synchronous` (lang/11-requirements-and-suspension.md).
- Retired `req.combinator.race-losers` (lang/11-requirements-and-suspension.md).
- Retired `req.combinator.all-bang-child` (lang/11-requirements-and-suspension.md).
- Retired `req.determinism.weak` (lang/11-requirements-and-suspension.md).
- Retired `gadt.intro.refine` (lang/13-gadts.md).
- Retired `gadt.intro.recover` (lang/13-gadts.md).
- Retired `gadt.result.declare` (lang/13-gadts.md).
- Retired `gadt.result.owner` (lang/13-gadts.md).
- Retired `gadt.result.refine` (lang/13-gadts.md).
- Retired `gadt.result.shared-data` (lang/13-gadts.md).
- Retired `gadt.result.default` (lang/13-gadts.md).
- Retired `gadt.shared.assignable` (lang/13-gadts.md).
- Retired `gadt.construct.syntax` (lang/13-gadts.md).
- Retired `gadt.construct.order` (lang/13-gadts.md).
- Retired `gadt.construct.infer` (lang/13-gadts.md).
- Retired `gadt.construct.no-explicit` (lang/13-gadts.md).
- Retired `gadt.construct.ambiguous` (lang/13-gadts.md).
- Retired `gadt.refine.equalities` (lang/13-gadts.md).
- Retired `gadt.refine.arm-local` (lang/13-gadts.md).
- Retired `gadt.refine.scope` (lang/13-gadts.md).
- Retired `gadt.refine.match-type` (lang/13-gadts.md).
- Retired `gadt.refine.impossible` (lang/13-gadts.md).
- Retired `gadt.refine.exhaustive` (lang/13-gadts.md).
- Retired `gadt.pattern.syntax` (lang/13-gadts.md).
- Retired `gadt.pattern.positional` (lang/13-gadts.md).
- Retired `gadt.pattern.named` (lang/13-gadts.md).
- Retired `gadt.pattern.literal` (lang/13-gadts.md).
- Retired `gadt.pattern.order` (lang/13-gadts.md).
- Retired `gadt.check.owner` (lang/13-gadts.md).
- Retired `gadt.check.well-formed` (lang/13-gadts.md).
- Retired `gadt.check.payload` (lang/13-gadts.md).
- Retired `gadt.check.construct` (lang/13-gadts.md).
- Retired `gadt.check.no-escape` (lang/13-gadts.md).
- Retired `gadt.existential.def` (lang/13-gadts.md).
- Retired `gadt.existential.fresh` (lang/13-gadts.md).
- Retired `gadt.existential.bounds` (lang/13-gadts.md).
- Retired `gadt.existential.no-escape` (lang/13-gadts.md).
- Retired `gadt.runtime.compile-time` (lang/13-gadts.md).
- Retired `gadt.runtime.tag` (lang/13-gadts.md).
- Retired `gadt.runtime.erased` (lang/13-gadts.md).
- Retired `gadt.runtime.evidence` (lang/13-gadts.md).
- Retired `gadt.runtime.evidence.match` (lang/13-gadts.md).
- Retired `gadt.unify.first-order` (lang/13-gadts.md).
- Retired `gadt.unify.aliases` (lang/13-gadts.md).
- Retired `gadt.unify.solvable` (lang/13-gadts.md).
- Retired `gadt.unify.nominal` (lang/13-gadts.md).
- Retired `gadt.unify.solution` (lang/13-gadts.md).
- Retired `gadt.unify.exhaustive` (lang/13-gadts.md).
- Retired `gadt.unify.catch-all` (lang/13-gadts.md).
- Retired `gadt.unify.nested` (lang/13-gadts.md).
- Retired `gadt.unify.contradiction` (lang/13-gadts.md).
- Retired `gadt.unify.variance` (lang/13-gadts.md).
- Retired `gadt.unify.no-cast` (lang/13-gadts.md).
- Retired `gadt.erasure.dynamic` (lang/13-gadts.md).
- Retired `gadt.erasure.no-descriptor` (lang/13-gadts.md).
- Retired `annot.decorator.bare-call` (lang/14-annotations.md).
- Retired `annot.facts-of.result` (lang/14-annotations.md).
- Retired `annot.metadata.list-any` (lang/14-annotations.md).
- Retired `annot.metadata.any-value` (lang/14-annotations.md).
- Retired `annot.metadata.eval` (lang/14-annotations.md).
- Retired `annot.typed-fact.untyped` (lang/14-annotations.md).
- Retired `annot.typed-fact.infer.weaken` (lang/14-annotations.md).
- Retired `annot.structure.module` (lang/14-annotations.md).
- Retired `annot.structure.facts-contents` (lang/14-annotations.md).
- Retired `annot.structure.find-lookup` (lang/14-annotations.md).
- Retired `annot.structure.members-api` (lang/14-annotations.md).
- Retired `annot.structure.no-names` (lang/14-annotations.md).
- Retired `annot.structure.receivers` (lang/14-annotations.md).
- Retired `annot.structure.build-fresh` (lang/14-annotations.md).
- Retired `annot.block.gadt` (lang/14-annotations.md).
- Retired `annot.line.typed` (lang/14-annotations.md).
- Retired `annot.line.drift` (lang/14-annotations.md).
- Retired `annot.line.drift.derive` (lang/14-annotations.md).
- Retired `annot.traitless.form` (lang/14-annotations.md).
- Retired `annot.traitless.not-inherent` (lang/14-annotations.md).
- Retired `annot.traitless.lines-only` (lang/14-annotations.md).
- Retired `annot.traitless.no-omit` (lang/14-annotations.md).
- Retired `annot.traitless.after-decorators` (lang/14-annotations.md).
- Retired `annot.traitless.self` (lang/14-annotations.md).
- Retired `annot.traitless.declaration-facts` (lang/14-annotations.md).
- Retired `annot.traitless.then-blocks` (lang/14-annotations.md).
- Retired `annot.traitless.module` (lang/14-annotations.md).
- Retired `annot.traitless.local` (lang/14-annotations.md).
- Retired `annot.traitless.target` (lang/14-annotations.md).
- Retired `annot.traitless.by` (lang/14-annotations.md).
- Retired `annot.traitless.generic` (lang/14-annotations.md).
- Retired `annot.traitless.generic.rename` (lang/14-annotations.md).
- Retired `annot.traitless.generic.error` (lang/14-annotations.md).
- Retired `annot.traitless.unique` (lang/14-annotations.md).
- Retired `annot.fact.eval` (lang/14-annotations.md).
- Retired `annot.fact.unused-non-std` (lang/14-annotations.md).
- Retired `annot.fact.unused-std` (lang/14-annotations.md).
- Retired `annot.fact.unused-self-line` (lang/14-annotations.md).
- Retired `annot.fact.unused-self-line.per-trait` (lang/14-annotations.md).
- Retired `annot.fact.unused-block-decorator` (lang/14-annotations.md).
- Retired `annot.fact.no-block-on` (lang/14-annotations.md).
- Retired `annot.fact.no-block-on.unprovable` (lang/14-annotations.md).
- Retired `annot.fact.no-block-on.error` (lang/14-annotations.md).
- Retired `annot.walk.match` (lang/14-annotations.md).
- Retired `annot.walk.members` (lang/14-annotations.md).
- Retired `annot.walk.value` (lang/14-annotations.md).
- Retired `annot.walk.nested` (lang/14-annotations.md).
- Retired `annot.build.variant` (lang/14-annotations.md).
- Retired `annot.build.next` (lang/14-annotations.md).
- Retired `annot.build.member` (lang/14-annotations.md).
- Retired `annot.build.missing` (lang/14-annotations.md).
- Retired `annot.variant.info` (lang/14-annotations.md).
- Retired `annot.variant.shared` (lang/14-annotations.md).
- Retired `annot.handle.read-type` (lang/14-annotations.md).
- Retired `annot.handle.declared-type` (lang/14-annotations.md).
- Retired `annot.handle.get` (lang/14-annotations.md).
- Retired `annot.handle.get.views` (lang/14-annotations.md).
- Retired `annot.walker.strengthen-member` (lang/14-annotations.md).
- Retired `annot.walker.strengthen-rest` (lang/14-annotations.md).
- Retired `annot.walker.generic-member-call` (lang/14-annotations.md).
- Retired `annot.walker.generic-rest-call` (lang/14-annotations.md).
- Retired `annot.error.cause.method` (lang/14-annotations.md).
- Retired `annot.error.transparent.cause` (lang/14-annotations.md).
- Retired `annot.error.bound.transparent` (lang/14-annotations.md).
- Retired `std-cli.cli.new` (std/cli.md).
- Retired `std-cli.cli.flag` (std/cli.md).
- Retired `std-cli.cli.option` (std/cli.md).
- Retired `std-cli.cli.positional` (std/cli.md).
- Retired `std-cli.cli.decl` (std/cli.md).
- Retired `std-cli.cli.builder` (std/cli.md).
- Retired `std-cli.cli.names` (std/cli.md).
- Retired `std-cli.cli.duplicate` (std/cli.md).
- Retired `std-cli.cli.positional.order` (std/cli.md).
- Retired `std-cli.parse.decl` (std/cli.md).
- Retired `std-cli.parse-args` (std/cli.md).
- Retired `std-cli.form.long-flag` (std/cli.md).
- Retired `std-cli.form.short-flag` (std/cli.md).
- Retired `std-cli.form.end` (std/cli.md).
- Retired `std-cli.form.dash` (std/cli.md).
- Retired `std-cli.form.positional` (std/cli.md).
- Retired `std-cli.form.no-clusters` (std/cli.md).
- Retired `std-cli.form.repeat-flag` (std/cli.md).
- Retired `std-cli.form.repeat-option` (std/cli.md).
- Retired `std-cli.error.missing-positional` (std/cli.md).
- Retired `std-cli.error.decl` (std/cli.md).
- Retired `std-cli.error.name` (std/cli.md).
- Retired `std-cli.error.first` (std/cli.md).
- Retired `std-cli.error.help` (std/cli.md).
- Retired `std-cli.parsed.decl` (std/cli.md).
- Retired `std-cli.parsed.flags` (std/cli.md).
- Retired `std-cli.parsed.values` (std/cli.md).
- Retired `std-cli.parsed.positionals` (std/cli.md).
- Retired `std-cli.parsed.flag` (std/cli.md).
- Retired `std-cli.parsed.value` (std/cli.md).
- Retired `std-cli.parsed.undeclared` (std/cli.md).
- Retired `std-cli.usage.decl` (std/cli.md).
- Retired `std-cli.usage.line` (std/cli.md).
- Retired `std-cli.usage.options` (std/cli.md).
- Retired `std-cli.usage.entry` (std/cli.md).
- Retired `std-cli.usage.no-help` (std/cli.md).
- Retired `std-cli.usage.no-help-flag` (std/cli.md).
- Retired `std-cmp.min-max.tie` (std/cmp.md).
- Retired `std-cmp.reverse.decl` (std/cmp.md).
- Retired `std-cmp.ordering.debug` (std/cmp.md).
- Retired `std-collections.list.chunks` (std/collections.md).
- Retired `std-collections.view.to-list.mut` (std/collections.md).
- Retired `std-collections.view.writes` (std/collections.md).
- Retired `std-collections.view.invalidate` (std/collections.md).
- Retired `std-collections.view.invalidate.anywhere` (std/collections.md).
- Retired `std-collections.view.invalid-use` (std/collections.md).
- Retired `std-collections.helper.group-by.order` (std/collections.md).
- Retired `std-collections.helper.find` (std/collections.md).
- Retired `std-collections.helper.flat-map` (std/collections.md).
- Retired `std-collections.helper.windows` (std/collections.md).
- Retired `std-collections.helper.contains` (std/collections.md).
- Retired `std-collections.helper.min-max.first` (std/collections.md).
- Retired `std-collections.counts.order` (std/collections.md).
- Retired `std-collections.map.keys` (std/collections.md).
- Retired `std-collections.map.values` (std/collections.md).
- Retired `std-collections.map.snapshot` (std/collections.md).
- Retired `std-collections.map.get-or` (std/collections.md).
- Retired `std-collections.set.decl` (std/collections.md).
- Retired `std-collections.set.contains` (std/collections.md).
- Retired `std-collections.set.invalidate` (std/collections.md).
- Retired `std-collections.deque.decl` (std/collections.md).
- Retired `std-collections.deque.ends` (std/collections.md).
- Retired `std-collections.deque.get` (std/collections.md).
- Retired `std-collections.deque.invalidate` (std/collections.md).
- Retired `std-collections.heap.decl` (std/collections.md).
- Retired `std-collections.heap.peek` (std/collections.md).
- Retired `std-collections.access.get` (std/collections.md).
- Retired `std-collections.access.first-last` (std/collections.md).
- Retired `std-console.eprintln.write` (std/console.md).
- Retired `std-console.eprintln.like-println` (std/console.md).
- Retired `std-console.input.mut` (std/console.md).
- Retired `std-console.error.debug` (std/console.md).
- Retired `std-console.buffer.decl` (std/console.md).
- Retired `std-console.buffer.new` (std/console.md).
- Retired `std-console.buffer.output` (std/console.md).
- Retired `std-console.buffer.error-output` (std/console.md).
- Retired `std-console.scripted.new` (std/console.md).
- Retired `std-digest.import` (std/digest.md).
- Retired `std-digest.sha256` (std/digest.md).
- Retired `std-digest.sha256-hex` (std/digest.md).
- Retired `std-digest.sha256.fips` (std/digest.md).
- Retired `std-digest.sha256.length` (std/digest.md).
- Retired `std-digest.sha256-hex.text` (std/digest.md).
- Retired `std-digest.no-panic` (std/digest.md).
- Retired `std-error.chain.decl` (std/error.md).
- Retired `std-error.chain.order` (std/error.md).
- Retired `std-error.chain.end` (std/error.md).
- Retired `std-error.chain.argument` (std/error.md).
- Retired `std-error.root-cause` (std/error.md).
- Retired `std-error.find` (std/error.md).
- Retired `std-error.find.same-reference` (std/error.md).
- Retired `std-error.find.exact` (std/error.md).
- Retired `std-error.report.decl` (std/error.md).
- Retired `std-error.report.of` (std/error.md).
- Retired `std-error.report.causes` (std/error.md).
- Retired `std-error.report.display` (std/error.md).
- Retired `std-error.context.decl` (std/error.md).
- Retired `std-error.context.err` (std/error.md).
- Retired `std-error.context.type` (std/error.md).
- Retired `std-error.context.display` (std/error.md).
- Retired `std-error.context.cause` (std/error.md).
- Retired `std-error.context.chain` (std/error.md).
- Retired `std-error.context.debug` (std/error.md).
- Retired `std-error.context.debug.fields` (std/error.md).
- Retired `std-error.std-types.impl` (std/error.md).
- Retired `std-error.std-types.cause` (std/error.md).
- Retired `std-error.std-types.resource` (std/error.md).
- Retired `std-format.debug.builder.types` (std/format.md).
- Retired `std-format.debug.builder.chain` (std/format.md).
- Retired `std-format.debug.derive-builders.mapping` (std/format.md).
- Retired `std-format.debug.derive-builders.record` (std/format.md).
- Retired `std-format.debug.derive-builders.tuple` (std/format.md).
- Retired `std-format.debug.derive-builders.unit` (std/format.md).
- Retired `std-format.debug.derive-builders.mixed` (std/format.md).
- Retired `std-format.debug.std-types.context-error` (std/format.md).
- Retired `std-fs.error.decl` (std/fs.md).
- Retired `std-fs.write.decl` (std/fs.md).
- Retired `std-fs.write.mut` (std/fs.md).
- Retired `std-fs.memory.new` (std/fs.md).
- Retired `std-fs.memory.keys` (std/fs.md).
- Retired `std-fs.memory.dirs` (std/fs.md).
- Retired `std-fs.memory.list-order` (std/fs.md).
- Retired `std-hash.bytes.sequence` (std/hash.md).
- Retired `std-hash.default.decl` (std/hash.md).
- Retired `std-hash.default.algorithm` (std/hash.md).
- Retired `std-hash.default.new` (std/hash.md).
- Retired `std-hash.default.finish` (std/hash.md).
- Retired `std-hash.default.fixed` (std/hash.md).
- Retired `std-hash.hash-of` (std/hash.md).
- Retired `std-hash.default.map` (std/hash.md).
- Retired `std-host.args.decl` (std/host.md).
- Retired `std-host.args.program` (std/host.md).
- Retired `std-host.args.list` (std/host.md).
- Retired `std-host.args.helper` (std/host.md).
- Retired `std-host.map-args.decl` (std/host.md).
- Retired `std-host.map-args.new` (std/host.md).
- Retired `std-host.map-env.names` (std/host.md).
- Retired `std-iter.adapter.filter` (std/iter.md).
- Retired `std-iter.adapter.take-first` (std/iter.md).
- Retired `std-iter.adapter.enumerate-usize` (std/iter.md).
- Retired `std-iter.adapter.map` (std/iter.md).
- Retired `std-iter.adapter.fold` (std/iter.md).
- Retired `std-iter.adapter.collect-defaulted` (std/iter.md).
- Retired `std-iter.adapter.methods` (std/iter.md).
- Retired `std-iter.adapter.lazy.next` (std/iter.md).
- Retired `std-iter.adapter.fold.drain` (std/iter.md).
- Retired `std-iter.adapter.mut-receiver` (std/iter.md).
- Retired `std-iter.adapter.skip-first` (std/iter.md).
- Retired `std-iter.adapter.take-while` (std/iter.md).
- Retired `std-iter.adapter.zip-iterable` (std/iter.md).
- Retired `std-iter.adapter.chain-iterable` (std/iter.md).
- Retired `std-iter.adapter.flat-map` (std/iter.md).
- Retired `std-iter.adapter.any` (std/iter.md).
- Retired `std-iter.adapter.all` (std/iter.md).
- Retired `std-iter.adapter.find` (std/iter.md).
- Retired `std-iter.adapter.count-remaining` (std/iter.md).
- Retired `std-iter.adapter.iterable-arg.iterator` (std/iter.md).
- Retired `std-iter.adapter.stop-early` (std/iter.md).
- Retired `std-iter.adapter.count.drain` (std/iter.md).
- Retired `std-iter.collect.trait` (std/iter.md).
- Retired `std-iter.collect.target` (std/iter.md).
- Retired `std-iter.collect.target-default` (std/iter.md).
- Retired `std-iter.collect.map` (std/iter.md).
- Retired `std-iter.collect.drain` (std/iter.md).
- Retired `std-iter.collect.stop` (std/iter.md).
- Retired `std-json.import.typed` (std/json.md).
- Retired `std-json.value.object-order` (std/json.md).
- Retired `std-json.value.accessors.number` (std/json.md).
- Retired `std-json.number.decl` (std/json.md).
- Retired `std-json.parse.object.order` (std/json.md).
- Retired `std-json.parse.object.duplicate` (std/json.md).
- Retired `std-json.parse.number.float` (std/json.md).
- Retired `std-json.parse.number.out-of-range` (std/json.md).
- Retired `std-json.error.variants` (std/json.md).
- Retired `std-json.error.expected.object` (std/json.md).
- Retired `std-json.to-json.trait` (std/json.md).
- Retired `std-json.from-json.trait` (std/json.md).
- Retired `std-json.from-json.call` (std/json.md).
- Retired `std-json.from-json.decode-errors` (std/json.md).
- Retired `std-json.from-json.relative` (std/json.md).
- Retired `std-json.decode` (std/json.md).
- Retired `std-json.decode.parse-error` (std/json.md).
- Retired `std-json.encode.bound` (std/json.md).
- Retired `std-json.decode.bound` (std/json.md).
- Retired `std-json.std.map` (std/json.md).
- Retired `std-json.std.map-keys` (std/json.md).
- Retired `std-json.derive.to-json.template` (std/json.md).
- Retired `std-json.derive.to-json.data` (std/json.md).
- Retired `std-json.derive.to-json.enum` (std/json.md).
- Retired `std-json.derive.to-json.unit` (std/json.md).
- Retired `std-json.derive.to-json.positional` (std/json.md).
- Retired `std-json.derive.to-json.embedded` (std/json.md).
- Retired `std-json.derive.to-json.members` (std/json.md).
- Retired `std-json.derive.from-json.template` (std/json.md).
- Retired `std-json.derive.from-json.member` (std/json.md).
- Retired `std-json.derive.from-json.no-default` (std/json.md).
- Retired `std-json.derive.from-json.payload` (std/json.md).
- Retired `std-json.derive.from-json.members` (std/json.md).
- Retired `std-num.parse.i32` (std/num.md).
- Retired `std-num.parse.i64` (std/num.md).
- Retired `std-num.parse.import` (std/num.md).
- Retired `std-num.parse-f64` (std/num.md).
- Retired `std-num.parse-f64.import` (std/num.md).
- Retired `std-num.parse-f64.no-out-of-range` (std/num.md).
- Retired `std-num.parse-f64.round-trip` (std/num.md).
- Retired `std-process.error.display.not-found` (std/process.md).
- Retired `std-process.error.display.permission` (std/process.md).
- Retired `std-process.error.debug` (std/process.md).
- Retired `std-process.error.eq` (std/process.md).
- Retired `std-process.output.eq` (std/process.md).
- Retired `std-process.debug` (std/process.md).
- Retired `std-process.scripted.new` (std/process.md).
- Retired `std-process.scripted.run` (std/process.md).
- Retired `std-process.scripted.not-found` (std/process.md).
- Retired `std-random.decl` (std/random.md).
- Retired `std-random.mut` (std/random.md).
- Retired `std-random.seeded.decl` (std/random.md).
- Retired `std-random.seeded.new` (std/random.md).
- Retired `std-random.rng.next` (std/random.md).
- Retired `std-random.rng.int` (std/random.md).
- Retired `std-random.rng.float` (std/random.md).
- Retired `std-random.rng.bool` (std/random.md).
- Retired `std-random.rng.choose` (std/random.md).
- Retired `std-random.rng.shuffle` (std/random.md).
- Retired `std-random.rng.sample` (std/random.md).
- Retired `std-random.rng.decl` (std/random.md).
- Retired `std-random.rng.new` (std/random.md).
- Retired `std-random.rng.from-random` (std/random.md).
- Retired `std-random.rng.mut` (std/random.md).
- Retired `std-regex.regex.decl` (std/regex.md).
- Retired `std-regex.syntax.dot` (std/regex.md).
- Retired `std-regex.syntax.escape.control` (std/regex.md).
- Retired `std-regex.syntax.escape.space` (std/regex.md).
- Retired `std-regex.syntax.escape.other` (std/regex.md).
- Retired `std-regex.syntax.class.item` (std/regex.md).
- Retired `std-regex.syntax.class.bad-range` (std/regex.md).
- Retired `std-regex.syntax.begin` (std/regex.md).
- Retired `std-regex.syntax.end` (std/regex.md).
- Retired `std-regex.syntax.anchor.text` (std/regex.md).
- Retired `std-regex.syntax.group.number` (std/regex.md).
- Retired `std-regex.syntax.group.unsupported` (std/regex.md).
- Retired `std-regex.repeat.greedy` (std/regex.md).
- Retired `std-regex.match.decl` (std/regex.md).
- Retired `std-regex.find-all.anchors` (std/regex.md).
- Retired `std-regex.captures.decl` (std/regex.md).
- Retired `std-regex.error.declared` (std/regex.md).
- Retired `std-regex.error.traits` (std/regex.md).
- Retired `std-regex.error.display` (std/regex.md).
- Retired `std-task.combinator.retry.decl-usize` (std/task.md).
- Retired `std-task.combinator.retry.loop` (std/task.md).
- Retired `std-task.combinator.retry.last-error` (std/task.md).
- Retired `std-task.combinator.retry.zero-once` (std/task.md).
- Retired `std-task.combinator.retry.cancel` (std/task.md).
- Retired `std-task.backoff.decl-usize` (std/task.md).
- Retired `std-task.retry-with.decl` (std/task.md).
- Retired `std-task.retry-with.loop` (std/task.md).
- Retired `std-task.retry-with.last-error` (std/task.md).
- Retired `std-task.retry-with.zero-once` (std/task.md).
- Retired `std-task.retry-with.sleep` (std/task.md).
- Retired `std-task.retry-with.delay` (std/task.md).
- Retired `std-task.retry-with.cancel` (std/task.md).
- Retired `std-task.all-list` (std/task.md).
- Retired `std-task.all-list.children` (std/task.md).
- Retired `std-task.all-list.order` (std/task.md).
- Retired `std-task.all-list.empty` (std/task.md).
- Retired `std-task.all-list.like-all` (std/task.md).
- Retired `std-testing.choices.int-generic` (std/testing.md).
- Retired `std-testing.choices.float-generic` (std/testing.md).
- Retired `std-testing.choices.list-max` (std/testing.md).
- Retired `std-testing.choices.map-max` (std/testing.md).
- Retired `std-testing.choices.declare` (std/testing.md).
- Retired `std-testing.it-prop` (std/testing.md).
- Retired `std-testing.prop.report` (std/testing.md).
- Retired `std-testing.budget` (std/testing.md).
- Retired `std-testing.budget.no-api` (std/testing.md).
- Retired `std-testing.budget.every-draw` (std/testing.md).
- Retired `std-testing.budget.simplest.int` (std/testing.md).
- Retired `std-testing.budget.simplest.float` (std/testing.md).
- Retired `std-testing.budget.simplest.bool` (std/testing.md).
- Retired `std-testing.budget.simplest.choose` (std/testing.md).
- Retired `std-testing.budget.simplest.empty` (std/testing.md).
- Retired `std-testing.budget.simplest.optional` (std/testing.md).
- Retired `std-testing.budget.simplest.result` (std/testing.md).
- Retired `std-testing.budget.simplest.tuple` (std/testing.md).
- Retired `std-testing.arbitrary.with` (std/testing.md).
- Retired `std-testing.arbitrary.with.module-typed` (std/testing.md).
- Retired `std-testing.arbitrary.with.checked` (std/testing.md).
- Retired `std-testing.hd-run.import` (std/testing.md).
- Retired `std-testing.hd-run.runs` (std/testing.md).
- Retired `std-testing.hd-run.output` (std/testing.md).
- Retired `std-testing.hd-run.missing-name` (std/testing.md).
- Retired `std-testing.runner.declares` (std/testing.md).
- Retired `std-testing.runner.case-type` (std/testing.md).
- Retired `std-text.utf8.error.declared` (std/text.md).
- Retired `std-text.utf8.error.truncated` (std/text.md).
- Retired `std-text.utf8.error.display.truncated` (std/text.md).
- Retired `std-text.prefix.std.r` (std/text.md).
- Retired `std-text.join.decl` (std/text.md).
- Retired `std-text.join.result` (std/text.md).
- Retired `std-text.builder.decl` (std/text.md).
- Retired `std-text.builder.new` (std/text.md).
- Retired `std-text.builder.push` (std/text.md).
- Retired `std-text.builder.build` (std/text.md).
- Retired `std-text.prefix-helper.escape-error` (std/text.md).
- Retired `std-time.suffix.std.duration-neg` (std/time.md).
- Retired `std-time.prelude.time-suffixes` (std/time.md).
- Retired `std-time.duration.sub` (std/time.md).
- Retired `std-time.duration.overflow` (std/time.md).
- Retired `std-time.timestamp.add.overflow` (std/time.md).
- Retired `std-time.duration.text.negative` (std/time.md).
- Retired `std-time.duration.text.min-value` (std/time.md).
- Retired `std-time.suffix.std.min` (std/time.md).
- Retired `std-time.suffix.std.fn` (std/time.md).
- Retired `std-time.suffix.std.only-four` (std/time.md).
- Retired `std-time.suffix.std.import` (std/time.md).
- Retired `std-time.suffix.std.overflow` (std/time.md).
- Retired `std-time.clock.decl` (std/time.md).
- Retired `std-time.clock.sleep.negative` (std/time.md).
- Retired `std-time.clock.mut` (std/time.md).
- Retired `std-time.timestamp.api-milliseconds` (std/time.md).
- Retired `std-time.timestamp.since` (std/time.md).
- Retired `std-time.manual.decl` (std/time.md).
- Retired `std-time.manual.state` (std/time.md).
- Retired `std-time.manual.new` (std/time.md).
- Retired `std-time.manual.sleep` (std/time.md).
- Retired `std-time.manual.sleep.panics` (std/time.md).
- Retired `std-time.date.decl` (std/time.md).
- Retired `std-time.parse-error.declared` (std/time.md).
- Retired `cli.command.positional` (cli/command-line.md).
- Retired `cli.mode.member.workspace` (cli/command-line.md).
- Retired `cli.exe.unselected-main` (cli/command-line.md).
- Retired `cli.task.beside-dir` (cli/command-line.md).
- Retired `cli.task.shared-unused` (cli/command-line.md).
- Retired `cli.host.entry-row` (cli/command-line.md).
- Retired `cli.host.default-profile.row` (cli/command-line.md).
- Retired `cli.host.default-profile.other` (cli/command-line.md).
- Retired `cli.host.default-profile.console-closed` (cli/command-line.md).
- Retired `cli.build.output` (cli/command-line.md).
- Retired `cli.test.summary.skipped` (cli/command-line.md).
- Retired `cli.test.deny-skipped` (cli/command-line.md).
- Retired `cli.test.process.missing` (cli/command-line.md).
- Retired `cli.test.doc.update` (cli/command-line.md).
- Retired `cli.profile.flag-only` (cli/command-line.md).
- Retired `cli.doc.link.broken` (cli/command-line.md).
- Retired `cli.json.diagnostic.file` (cli/command-line.md).
- Retired `cli.json.test` (cli/command-line.md).
- Retired `cli.json.test.fields` (cli/command-line.md).
- Retired `cli.json.summary` (cli/command-line.md).
- Retired `cli.json.summary.fields` (cli/command-line.md).
- Retired `cli.dep.add` (cli/command-line.md).
- Retired `cli.manifest.unknown-key` (cli/command-line.md).

### [#57](https://github.com/HerringtonDarkholme/hd-lang/issues/57) Finish the flaky run::fetch_with_path_requirements_only_has_nothing_to_do

- Give each ran() call its own HD_CACHE directory (M50) (`e4b8bfb61`).

### [#6](https://github.com/HerringtonDarkholme/hd-lang/issues/6) TestRunner in registration body closures (it_each, it_prop, it_prop_with)

- Let it_prop and it_prop_with bodies use TestRunner (`632e4dde7`).

### [#60](https://github.com/HerringtonDarkholme/hd-lang/issues/60) Muse: list Node vs wasmtime mismatches in conformance

- Record per-engine conformance skips in audit/compiler/engine-skips.md (`378a40d56`).

### [#7](https://github.com/HerringtonDarkholme/hd-lang/issues/7) Init groups: statements run in dependency order across a module cycle

- Order a module cycle's top-level statements by dependency (`22fab1759`).

### [#52](https://github.com/HerringtonDarkholme/hd-lang/issues/52) wasmtime perf leftovers

- Profile wasmtime closures gap and binary size in audit report (`68dfb5516`).

### [#3](https://github.com/HerringtonDarkholme/hd-lang/issues/3) Weaken fn! values to their constructor type fn(..) -> mut Suspend[T]

- Weaken a suspending function value to its constructor type (`c016d623d`).

### [#4](https://github.com/HerringtonDarkholme/hd-lang/issues/4) Generic local functions

- Check generic local functions once and instantiate them per use (`5dd9ff46e`).

### [#8](https://github.com/HerringtonDarkholme/hd-lang/issues/8) Decision: fixture folder-cycle-leaf-folder.hd writes `dyn Error` for an enum

- Fix three fixtures that contradicted the spec's own rules (`8643b6135`).

### [#12](https://github.com/HerringtonDarkholme/hd-lang/issues/12) Decision: fixture expected-i32-found-usize.hd declares `pub fn main() -> i32`

- Fix three fixtures that contradicted the spec's own rules (`8643b6135`).

### [#10](https://github.com/HerringtonDarkholme/hd-lang/issues/10) Decision: trait aliases, `type Store = Db` or `type Store = dyn Db`

- Stop resolving a requirement key through a type alias (`64f4df1e2`).

### [#20](https://github.com/HerringtonDarkholme/hd-lang/issues/20) hd reads a program from stdin when stdin is not a terminal

- Run piped stdin as a single-file program with bare hd (`5ff9a77e7`).

### [#64](https://github.com/HerringtonDarkholme/hd-lang/issues/64) Local functions with parameter defaults

- Support parameter defaults and named arguments on local functions (`24a45a20d`).

### [#61](https://github.com/HerringtonDarkholme/hd-lang/issues/61) wasmtime: add the disposed-file and pending-gate fixture profiles

- Add disposed-file and pending-gate fixture profiles on wasmtime (`2606a214d`).

### [#29](https://github.com/HerringtonDarkholme/hd-lang/issues/29) TypeId display name for same-named packages

- Print a TypeId by its name, apart from its identity key (`6408a056c`).

### [#55](https://github.com/HerringtonDarkholme/hd-lang/issues/55) Flaky tests under heavy load

- Widen suspend sample all! timer gap from 15ms to 245ms (`121d30a19`).

### [#15](https://github.com/HerringtonDarkholme/hd-lang/issues/15) Release profile: overflow wraps under --release (per-profile codegen and cache keys)

- Wrap integer overflow in the release profile (`be28bb44e`).

### [#69](https://github.com/HerringtonDarkholme/hd-lang/issues/69) hd_cli: one helper for the --max-memory / HD_MAX_MEMORY fallback

- Merge the duplicated HD_MAX_MEMORY fallback into memory_limit (`39f4862d2`).

### [#70](https://github.com/HerringtonDarkholme/hd-lang/issues/70) dbg-in-release: a dbg call in a release build of user code is an error

- Reject dbg in the user's own code in a release build (`52e52391a`).

### [#89](https://github.com/HerringtonDarkholme/hd-lang/issues/89) discarded-must-use-value: an unused cold suspension is not reported

- Report an unread suspension binding as discarded-must-use-value (`b7b598e25`).

### [#93](https://github.com/HerringtonDarkholme/hd-lang/issues/93) closure-parameter-needs-annotation is never reported

- Require an annotation on closure parameters without an expected type (`7d4173831`).

### [#94](https://github.com/HerringtonDarkholme/hd-lang/issues/94) A float range bound is accepted

- Require an integer type for each range bound (`af9a35893`).

### [#95](https://github.com/HerringtonDarkholme/hd-lang/issues/95) An unresolved generic return placeholder is accepted

- Report an unsolved call type argument as cannot-infer-type (`229cf7bb4`).

### [#27](https://github.com/HerringtonDarkholme/hd-lang/issues/27) Tasks: shared task modules (tasks/<dir>/*.hd) do not resolve

- Include shared task modules in each task's program (`586fcc0ea`).

### [#97](https://github.com/HerringtonDarkholme/hd-lang/issues/97) Workspace mode: hd run NAME picks the one member with NAME

- Test workspace hd run NAME and -p selection rules (`3c8987de7`).

### [#21](https://github.com/HerringtonDarkholme/hd-lang/issues/21) hd doc: write the documentation pages

- Write the hd doc page set from sources and interfaces (`778af5905`).

### [#99](https://github.com/HerringtonDarkholme/hd-lang/issues/99) hd doc NAME: print one item, module or std entry

- Print one item, module or std entry with hd doc NAME (`45b9786c4`).

### [#98](https://github.com/HerringtonDarkholme/hd-lang/issues/98) hd doc: [`NAME`] links and broken-doc-link

- Resolve [`NAME`] links in hd doc pages (`b885a2a42`).

### [#76](https://github.com/HerringtonDarkholme/hd-lang/issues/76) Huge integer literal reads as a float before integer-literal-range (1 fixture)

- Keep over-u64 decimal literals as integers so range check fires (`0588c088c`).

### [#91](https://github.com/HerringtonDarkholme/hd-lang/issues/91) List.view panics carry no message, so the report has no `CODE: message`

- Give std index-out-of-bounds panics a message (`0f90c6184`).

### [#16](https://github.com/HerringtonDarkholme/hd-lang/issues/16) Panic frames in hd test reports

- Print symbolized panic frames in hd test reports (`1b79a1a4e`).

### [#96](https://github.com/HerringtonDarkholme/hd-lang/issues/96) Decision: is an unread Result / T? binding an error, or only an unread suspension?

- Narrow must-use binding rule and fact evaluation timing (`9ac196c4f`).

### [#13](https://github.com/HerringtonDarkholme/hd-lang/issues/13) Decision: should member and variant facts evaluate lazily on `info.facts`

- Narrow must-use binding rule and fact evaluation timing (`9ac196c4f`).

### [#100](https://github.com/HerringtonDarkholme/hd-lang/issues/100) Decision: approve fixing two hd doc fixtures that fail type checking

- Use +0 for the i32 literal in doc fixtures (`2b5e86886`).

### [#102](https://github.com/HerringtonDarkholme/hd-lang/issues/102) Decision: three stale CLI fixtures (ambiguous-import JSON, hd --help, test-outcomes text)

- Name the use path's root in the ambiguous-import message (`d8862383e`).

### [#103](https://github.com/HerringtonDarkholme/hd-lang/issues/103) Structure handles are rebuilt on every call; the spec says each member has one constant handle

- Build each structure handle once per owner instance (`5e2dc1d86`).

### [#110](https://github.com/HerringtonDarkholme/hd-lang/issues/110) Mixed-script identifier never warns (1 fixture). `typing/warnings/mixed-script-identifier.hd` expects `warn...

- Warn on mixed-script identifiers in the lexer (`d45da60f6`).

### [#119](https://github.com/HerringtonDarkholme/hd-lang/issues/119) Multi-line pipe step never reports (1 fixture). `typing/invalid/pipe-multi-line-step.hd` expects `reject:mu...

- Reject indented suites in pipe steps (`646149a8c`).

### [#133](https://github.com/HerringtonDarkholme/hd-lang/issues/133) Repeated literal match arm never reports unreachable-match-arm (1 fixture). `typing/invalid/duplicate-liter...

- Model char literals as scalar points in match usefulness (`24ba6c276`).

### [#71](https://github.com/HerringtonDarkholme/hd-lang/issues/71) Parser errors carry cascade extras (14 fixtures)

- Skip a failed statement's fallout instead of reporting it (`e704081a6`).

### [#113](https://github.com/HerringtonDarkholme/hd-lang/issues/113) Tests-block shape violations never report (3 fixtures). `typing/invalid/duplicate-tests-block.hd` expects `...

- Report duplicate and misplaced tests blocks (`140be4453`).

### [#106](https://github.com/HerringtonDarkholme/hd-lang/issues/106) Integer literal overflow: carry an overflow bit instead of re-parsing source text

- Carry integer overflow as a bit instead of re-reading source (`4e6a04b54`).

### [#104](https://github.com/HerringtonDarkholme/hd-lang/issues/104) Panic frames show internal function names (p16/t/pick, $test0)

- Name panic frames in hd terms and collapse repeated frames (`177a2289c`).

### [#105](https://github.com/HerringtonDarkholme/hd-lang/issues/105) Decision: cap the frames printed for a stack-exhausted panic?

- Name panic frames in hd terms and collapse repeated frames (`177a2289c`).

### [#77](https://github.com/HerringtonDarkholme/hd-lang/issues/77) Context-free None reports cannot-infer-type / no-common-type instead of missing-contextual-enum-type (7 fixtures)

- Report missing-contextual-enum-type for context-free variants (`c6bb3fe5b`).

### [#72](https://github.com/HerringtonDarkholme/hd-lang/issues/72) Report mixed-signedness instead of type-mismatch (4 fixtures)

- Report mixed-signedness for signed/unsigned operand pairs (`42961c72d`).

### [#75](https://github.com/HerringtonDarkholme/hd-lang/issues/75) Mixed-width operands report two type-mismatches instead of one (4 fixtures)

- Report one type-mismatch for a numeric operand width mismatch (`809066e62`).

### [#83](https://github.com/HerringtonDarkholme/hd-lang/issues/83) Call arguments: report duplicate/unknown named arguments, not argument-count (3 fixtures)

- Name argument errors suppress the argument-count report (`a790ddafb`).

### [#114](https://github.com/HerringtonDarkholme/hd-lang/issues/114) By-Structure template shape violations never report (4 fixtures). `typing/invalid/structure-outside-templat...

- Report missing-result-type for a private function a template names (`63791206b`).
- Resolve marker-template by Structure calls; drop the std exemption (`a7e013abe`).

### [#78](https://github.com/HerringtonDarkholme/hd-lang/issues/78) Row parameters in the wrong place report unknown-trait/type instead of generic-kind-mismatch (5 fixtures)

- Report generic-kind-mismatch for row/type kind errors (`66157914a`).

### [#88](https://github.com/HerringtonDarkholme/hd-lang/issues/88) Bare-variant and duplicate-field patterns add one cascade error (S+T, 4 fixtures)

- Bind a rejected pattern's names and skip its arm in reachability (`bf7736b85`).

### [#135](https://github.com/HerringtonDarkholme/hd-lang/issues/135) Decision: two valid fixtures call trait methods without importing the trait

- Import the traits the valid fixtures call methods of (`60e046636`).

### [#140](https://github.com/HerringtonDarkholme/hd-lang/issues/140) Decision: two nonfinal-positional-spread fixtures contradict the spec

- Make the nonfinal-spread fixtures give exactly one nonfinal-positional-spread (`80b5999dc`).

### [#92](https://github.com/HerringtonDarkholme/hd-lang/issues/92) Decision: four defective fixtures from triage group Z

- Fix the extra errors in four invalid fixtures (`2a402eac7`).

### [#73](https://github.com/HerringtonDarkholme/hd-lang/issues/73) Report variance-representation-change instead of type-mismatch (3 fixtures)

- Report variance-representation-change for element conversions (`96b18b4d5`).

### [#74](https://github.com/HerringtonDarkholme/hd-lang/issues/74) Report missing-derived-bound instead of type-mismatch (2 fixtures)

- Report missing-derived-bound for unmet derived bounds at operators (`e57f0d108`).

### [#80](https://github.com/HerringtonDarkholme/hd-lang/issues/80) Report requirement-in-default instead of missing-requirement (2 fixtures)

- Report requirement-in-default for every kind of default (`222c81d0f`).

### [#121](https://github.com/HerringtonDarkholme/hd-lang/issues/121) Required enum-constructor default never reports requirement-in-default (1 fixture). `typing/invalid/enum-de...

- Report requirement-in-default for every kind of default (`222c81d0f`).

### [#141](https://github.com/HerringtonDarkholme/hd-lang/issues/141) Flaky CI: memory_cap allocation_trip_child retained-guest asserts an exact process-wide byte count

- Count memory-cap test balance per thread, not process-wide (`23cf7a109`).

### [#79](https://github.com/HerringtonDarkholme/hd-lang/issues/79) misplaced-test-case never fires (2 fixtures)

- Resolve prelude it in test helpers so misuse is misplaced-test-case (`1e064ea70`).

### [#84](https://github.com/HerringtonDarkholme/hd-lang/issues/84) Non-numeric sum reports unknown-method instead of unsatisfied-trait-bound (2 fixtures)

- Report unsatisfied-trait-bound for a method whose impl bound fails (`0ed4b54a0`).

### [#142](https://github.com/HerringtonDarkholme/hd-lang/issues/142) Move the Rust compiler from compiler/ to the repo root

- Fix paths for the Rust workspace at the repository root (`7a3b2d84f`).

### [#146](https://github.com/HerringtonDarkholme/hd-lang/issues/146) Consolidate top-level folders (spikes, docs, tests, samples)

- Fix paths after consolidating top-level folders (`41cd5b3d4`).

### [#147](https://github.com/HerringtonDarkholme/hd-lang/issues/147) Playground W1: threads feature on hd_driver; CI builds hd_web for wasm32

- Gate hd_driver's thread pool behind a threads feature (`943670f17`).

### [#148](https://github.com/HerringtonDarkholme/hd-lang/issues/148) Playground W2: hd_web compile API with diagnostics shared with the CLI

- Add the hd_web compile session and share diagnostics with the CLI (`7852dedda`).

### [#111](https://github.com/HerringtonDarkholme/hd-lang/issues/111) Discarded debug text never warns (1 fixture). `typing/warnings/unused-debug-text.hd` expects `warn:unused-d...

- Warn on discarded debug text in expression statements (`530df64fa`).

### [#117](https://github.com/HerringtonDarkholme/hd-lang/issues/117) Never-typed binding never reports (1 fixture). `typing/invalid/uninhabited-binding.hd` (`found := panic(......

- Reject unannotated bindings of never-typed initializers (`be4546b6e`).

### [#149](https://github.com/HerringtonDarkholme/hd-lang/issues/149) Playground W3: wasm exports, web profile, smoke test and size gate

- Add hd_web wasm exports, web profile and compiler smoke test (`21d2c4020`).

### [#118](https://github.com/HerringtonDarkholme/hd-lang/issues/118) Bad module paths never report (2 fixtures). `typing/invalid/module-named-pkg.hd` expects `reject:reserved-m...

- Reject reserved and non-identifier module paths (`ed1b30539`).

### [#150](https://github.com/HerringtonDarkholme/hd-lang/issues/150) Playground W4: split host/core.mjs into a neutral core and a Node layer

- Split host/core.mjs into a neutral core and a Node layer (`ab85dfc00`).

### [#136](https://github.com/HerringtonDarkholme/hd-lang/issues/136) pub-use loop fixtures: locus mismatch and wrong code

- Report each re-export loop once, quietly downstream (`b2c19b453`).

### [#137](https://github.com/HerringtonDarkholme/hd-lang/issues/137) for over a non-iterable stops at Body (unsupported) instead of erroring

- Check for loops over open literals through Iterable (`6927205a3`).

### [#151](https://github.com/HerringtonDarkholme/hd-lang/issues/151) Playground W5: Check and Run in the worker on the Rust compiler

- Run and Check in the playground on the Rust compiler (`a6dc47b3c`).

### [#155](https://github.com/HerringtonDarkholme/hd-lang/issues/155) uninhabited-binding: replace the diagnostic-count guard with POISON recovery

- Recover from binding errors with poison, not never (`bce01e74d`).

### [#156](https://github.com/HerringtonDarkholme/hd-lang/issues/156) if without else accepted as a value in a binding

- Reject value ifs without else outside tails (`54587fc24`).

### [#152](https://github.com/HerringtonDarkholme/hd-lang/issues/152) Playground W6: Test mode in the browser

- Run Test in the playground on the Rust compiler (`f05e7d2cb`).

### [#81](https://github.com/HerringtonDarkholme/hd-lang/issues/81) test-only-use never reported (2 fixtures)

- Report test-only-use for test-only modules and hd_run (`952b0cbd6`).

### [#153](https://github.com/HerringtonDarkholme/hd-lang/issues/153) Playground W8: WAT view through wasmprinter

- Print the playground WAT from the compiled module with wasmprinter (`0b33e4a8e`).

### [#154](https://github.com/HerringtonDarkholme/hd-lang/issues/154) Playground W7: remove src/ from the website

- Cut the TS compiler out of the playground (W7) (`23412b49a`).

### [#144](https://github.com/HerringtonDarkholme/hd-lang/issues/144) Website checks and build: use the Rust hd instead of src/

- Website stops using the TS compiler; checks run the Rust hd (`596805424`).

### [#145](https://github.com/HerringtonDarkholme/hd-lang/issues/145) Retire the TS compiler: delete src/ and the portable TS test tier

- Point checks and docs at the Rust hd after the TS compiler (`f2495f93a`).

### [#158](https://github.com/HerringtonDarkholme/hd-lang/issues/158) unused-derivation-fact reported as an error; driver harness ignores severity

- Keep stage-A warning severity across iface cache hits (`347c55db8`).

### [#82](https://github.com/HerringtonDarkholme/hd-lang/issues/82) Derive-law violations report missing-supertrait-implementation (2 fixtures)

- Report underivable traits and replace supertrait errors on derives (`23cd8cca7`).

### [#85](https://github.com/HerringtonDarkholme/hd-lang/issues/85) Tuple index / self field report unknown-data-field instead of unknown-method (2 fixtures)

- Report unknown-method for tuple and default-body member misses (`67ed7d535`).

### [#86](https://github.com/HerringtonDarkholme/hd-lang/issues/86) Bad data literals add a spurious missing-required-field (2 fixtures)

- Suppress missing-required-field when a literal already fails (`5449e39b2`).

### [#87](https://github.com/HerringtonDarkholme/hd-lang/issues/87) Variant payload/pattern field errors give the wrong code (2 fixtures)

- Report unknown variant pattern fields as unknown-data-field (`0eabb8f9c`).

### [#90](https://github.com/HerringtonDarkholme/hd-lang/issues/90) trailing-block-ineligible and tuple-binding-arity give neighboring codes (2 fixtures)

- Name the code for ineligible trailing blocks and let arity (`f1e8ad21a`).

### [#107](https://github.com/HerringtonDarkholme/hd-lang/issues/107) Dead code after return / infinite loop never warns unreachable-code (2 fixtures). `typing/warnings/unreacha...

- Warn unreachable-code after diverging statements (`9c8b81514`).

### [#63](https://github.com/HerringtonDarkholme/hd-lang/issues/63) Module cycles: entry module or script split by another module

- Run an entry module's statements split by its module cycle (`72c962da5`).

### [#138](https://github.com/HerringtonDarkholme/hd-lang/issues/138) Frame names: derive from structured item data, not by parsing impl path text

- Name panic frames from recorded item data, not path text (`43adb3a4c`).

### [#45](https://github.com/HerringtonDarkholme/hd-lang/issues/45) Closure reading a later top-level binding reports unknown-name

- Report a read of a later top-level binding as read-before-init (`506330d47`).

### [#47](https://github.com/HerringtonDarkholme/hd-lang/issues/47) Internal names in messages (`use_it#0`, `Index#0`, `Outcome#1`)

- Print type parameters by their written name in messages (`008f1e7eb`).

### [#68](https://github.com/HerringtonDarkholme/hd-lang/issues/68) fn.default.later-parameter: a default naming a later parameter is not rejected

- Reject a parameter default that names a later parameter (`459d4c506`).

### [#157](https://github.com/HerringtonDarkholme/hd-lang/issues/157) Runtime function names in the wasm name section are Debug dumps

- Name runtime helpers by kind in the wasm name section (`cbca59caa`).

### [#139](https://github.com/HerringtonDarkholme/hd-lang/issues/139) expect(): choose mixed-signedness from a typed position, not the label text

- Select expect error codes by position, not label text (`fcf3e2b5f`).

### [#120](https://github.com/HerringtonDarkholme/hd-lang/issues/120) Blocking call in a parameter default never reports suspension-forbidden-context (1 fixture). `typing/invali...

- Recategorize indirect default block_on as runtime panic (`b5fdeaf74`).

### [#48](https://github.com/HerringtonDarkholme/hd-lang/issues/48) Test prelude names in whole test modules

- Flag test prelude shadowing in test modules and integration files (`c756e9ab0`).

### [#162](https://github.com/HerringtonDarkholme/hd-lang/issues/162) Optimization passes as pipelines keyed by target and level

- Measure closure_spec knobs per target; keep 64/2 everywhere (`6da94c89a`).

### [#30](https://github.com/HerringtonDarkholme/hd-lang/issues/30) Stop at build when an intrinsic is unlowered

- Reject unlowered intrinsics during the build (`b7b482c30`).

### [#42](https://github.com/HerringtonDarkholme/hd-lang/issues/42) Mono: an impl over a function type with a row parameter

- Bind row parameters in function impl heads (`5b733a5ae`).

### [#36](https://github.com/HerringtonDarkholme/hd-lang/issues/36) @error leftovers

- Check transparent error member arity (`157cfe0db`).

### [#37](https://github.com/HerringtonDarkholme/hd-lang/issues/37) Decorators recognised by resolution, not text

- Resolve decorator identities instead of matching text (`b2bbd5b39`).

### [#164](https://github.com/HerringtonDarkholme/hd-lang/issues/164) std: flatten nested matches with early returns

- Flatten JSON variant payload handling (`2ca4de7be`).

### [#101](https://github.com/HerringtonDarkholme/hd-lang/issues/101) hd doc std.NAME: render from lib/std sources, not a table scraped from spec Markdown

- Render hd doc std.NAME from lib/std sources (`19c65109d`).

### [#25](https://github.com/HerringtonDarkholme/hd-lang/issues/25) Affected tests in hd test

- Implement hd test --affected with test program fingerprints (`67da8b2de`).

### [#41](https://github.com/HerringtonDarkholme/hd-lang/issues/41) Privacy leftovers

- Cross profile boundaries through Serialize and Deserialize (`b7612c093`).

### [#172](https://github.com/HerringtonDarkholme/hd-lang/issues/172) hd FILE panic reports have no source location or fallback note

- Report panic locations and usize-fallback notes from hd FILE (`0efb25ff0`).

### [#24](https://github.com/HerringtonDarkholme/hd-lang/issues/24) hd cache gc and compiled-cache eviction

- Evict compiled entries by LRU under a cap and add hd cache gc (`9b60ea1b0`).

### [#50](https://github.com/HerringtonDarkholme/hd-lang/issues/50) Erase newtypes at runtime as designed

- Erase newtypes at runtime and drop derived-newtype adapters (`c85ee4e0d`).

### [#173](https://github.com/HerringtonDarkholme/hd-lang/issues/173) Signed literal under ** and dbg() with no arguments

- Write nothing for dbg() with no arguments (`600fe6f32`).

### [#23](https://github.com/HerringtonDarkholme/hd-lang/issues/23) hd fix

- Add hd fix: apply safe fix-its in verified rounds (`9159a594f`).

### [#190](https://github.com/HerringtonDarkholme/hd-lang/issues/190) Move future-work/ and audit/ into docs/, guide/ into website/

- Move future-work and audit into docs, guide into website (`3209d690d`).

### [#170](https://github.com/HerringtonDarkholme/hd-lang/issues/170) Checker accepts invalid programs found by the coverage audit

- Name child-trait widening a representation change (`c952f30f1`).

### [#171](https://github.com/HerringtonDarkholme/hd-lang/issues/171) folder-cycle diagnostic lacks its required content

- Show the loop, uses, tangle and fix in folder-cycle diagnostics (`289ea984a`).

### [#191](https://github.com/HerringtonDarkholme/hd-lang/issues/191) Merge the three learn-hd texts

- Merge the learn-hd texts: tour lessons, a short quick start, no long tour (`6c269a442`).

### [#167](https://github.com/HerringtonDarkholme/hd-lang/issues/167) Engine selection: match cli.engine.select and the limit rules

- Let --engine win over HD_ENGINE and explain the Node heap gap (`d1c0c1458`).

### [#238](https://github.com/HerringtonDarkholme/hd-lang/issues/238) Check incremental soundness of same-package test-only uses

- Guard incremental soundness of uses only test code makes (`80b375274`).

### [#276](https://github.com/HerringtonDarkholme/hd-lang/issues/276) A tests: block's use makes a false folder-cycle

- Give the test role its own closure for tests: block uses (`2157761bb`).

### [#211](https://github.com/HerringtonDarkholme/hd-lang/issues/211) Implementing a trait method with a row parameter is rejected

- Match trait method row parameters by position (`9c8ca6427`).

### [#206](https://github.com/HerringtonDarkholme/hd-lang/issues/206) hd_cache ledger test depends on the 20 ms eviction bound

- Make the cross-process ledger test independent of eviction timing (`a2b6ed416`).

### [#207](https://github.com/HerringtonDarkholme/hd-lang/issues/207) An operator with a literal operand skips the instantiation choice

- Choose operator instantiations using a literal operand (`751b28bfa`).

### [#239](https://github.com/HerringtonDarkholme/hd-lang/issues/239) File grants: open granted directories once, resolve under them

- Resolve file operations under opened grant directories (`e4d3ddd2f`).

### [#270](https://github.com/HerringtonDarkholme/hd-lang/issues/270) An impl declared inside a tests: block crashes hd test at Collect

- Give a test program's Collect the impls of its tests: blocks (`db921faa3`).

### [#209](https://github.com/HerringtonDarkholme/hd-lang/issues/209) A closure in a generic method called through dyn fails to build

- Instantiate closures of erased bodies at the caller's types (`7962eae67`).

### [#229](https://github.com/HerringtonDarkholme/hd-lang/issues/229) Node: a blocked host read keeps the process alive past --time-limit

- End a Node run whose worker is stuck in a blocking read (`354ac8541`).

### [#210](https://github.com/HerringtonDarkholme/hd-lang/issues/210) A suspending generic method called through dyn fails to build

- Suspend erased generic methods with their witness (`209f16eee`).

### [#278](https://github.com/HerringtonDarkholme/hd-lang/issues/278) Agent skill: writing hd code and using the hd CLI

- Fold the Haiku probe's gaps into the hd skills (`339f3a14d`).

### [#282](https://github.com/HerringtonDarkholme/hd-lang/issues/282) A script's check result depends on whether hd check or hd FILE ran first

- Check a script's body as its command runs it, keyed by that choice (`b5f94873e`).

### [#281](https://github.com/HerringtonDarkholme/hd-lang/issues/281) hd test hangs on a FIFO inside the package directory

- Open only regular files in package walks and fingerprints (`a9d0051a1`).

### [#284](https://github.com/HerringtonDarkholme/hd-lang/issues/284) hd FILE inside a package checks the whole package

- Load only a single-file program's own file, not its package (`785855501`).

### [#280](https://github.com/HerringtonDarkholme/hd-lang/issues/280) hd check --tests skips library-role errors

- Check both roles under hd check --tests (`6b9b88d77`).

### [#279](https://github.com/HerringtonDarkholme/hd-lang/issues/279) A tests: block impl overlapping a library impl is not reported

- Report overlap of a tests: block impl in the test role (`dafa96bab`).

### [#192](https://github.com/HerringtonDarkholme/hd-lang/issues/192) Report trait-resolution-depth for goals in headers

- Report trait-resolution-depth for goals in headers (`107e8d1f7`).

### [#193](https://github.com/HerringtonDarkholme/hd-lang/issues/193) missing-result-type for trait method declarations

- Require a result type on every trait method declaration (`1ca6f664d`).

### [#218](https://github.com/HerringtonDarkholme/hd-lang/issues/218) One unclosed bracket hides every later syntax error

- End an unclosed bracket's scope at the next column-0 item (`20b8e0914`).

### [#194](https://github.com/HerringtonDarkholme/hd-lang/issues/194) Conformance fixtures for the associated-type codes

- Add fixtures for binding name reach and duplicate bindings (`db5deb07e`).

### [#240](https://github.com/HerringtonDarkholme/hd-lang/issues/240) Run runtime conformance on the optimized pipeline in CI

- Run driver conformance on the optimized pipeline in CI (`d629a5c5b`).

### [#195](https://github.com/HerringtonDarkholme/hd-lang/issues/195) A bare key in a row type-argument slot reports the wrong code

- Report a bare key in an explicit row slot as generic-kind-mismatch (`6fbc5e3eb`).

### [#219](https://github.com/HerringtonDarkholme/hd-lang/issues/219) Report one folder-cycle per strongly connected component

- Report one folder-cycle per strongly connected component (`3d126042c`).

### [#244](https://github.com/HerringtonDarkholme/hd-lang/issues/244) Walk omitted-result inference with an explicit stack

- Check acyclic omitted-result chains callees first (`50502db00`).

### [#283](https://github.com/HerringtonDarkholme/hd-lang/issues/283) --format is called a global flag but hd --format json check FILE fails

- Accept a global --format before the command name (`6fda45641`).

### [#245](https://github.com/HerringtonDarkholme/hd-lang/issues/245) Warn once when the compiled cache cannot be written

- Warn once when the compiled cache cannot be written (`2e9f55302`).

### [#277](https://github.com/HerringtonDarkholme/hd-lang/issues/277) Engines differ on data/missing/../a.txt under a file grant

- Fail data/missing/../a.txt as the OS does under a file grant (`08114d475`).

### [#247](https://github.com/HerringtonDarkholme/hd-lang/issues/247) Remove the no-op TestOverlay task

- Remove the no-op TestOverlay task (`e560b9919`).

### [#260](https://github.com/HerringtonDarkholme/hd-lang/issues/260) Singleton globals for payloadless subtype-boxed variants

- Read payloadless subtype-box variants from constant globals (`05c40ec7b`).

### [#252](https://github.com/HerringtonDarkholme/hd-lang/issues/252) Copy boundary data in 8-byte steps

- Copy boundary data in 8-byte steps (`c0c38a9a4`).

### [#262](https://github.com/HerringtonDarkholme/hd-lang/issues/262) ref.eq fast path in string equality

- Return early from StrEq when both views are one view (`972d8d7bc`).

### [#241](https://github.com/HerringtonDarkholme/hd-lang/issues/241) Test runners: read the panic category instead of parsing stderr

- Judge panics by the host's recorded category, not stderr (`e0db2bff8`).

### [#285](https://github.com/HerringtonDarkholme/hd-lang/issues/285) Package walks follow symlinked directories without a cycle guard

- Skip symlinked directories that loop back in package walks (`24f749352`).

### [#208](https://github.com/HerringtonDarkholme/hd-lang/issues/208) Test-role bodies solve over the ordinary impl universe

- Test that a tests: block solves over its test-only folders' impls (`96616a661`).

### [#275](https://github.com/HerringtonDarkholme/hd-lang/issues/275) hd build writes resolved source positions

- Write resolved frame positions into modules hd build writes (`08ce516e6`).

### [#215](https://github.com/HerringtonDarkholme/hd-lang/issues/215) Make in-run hash tables deterministic and lint their iteration

- Use fixed-seed hash tables and ban std's random ones (`091410cd1`).

### [#243](https://github.com/HerringtonDarkholme/hd-lang/issues/243) Postpone call arguments that need the expected type

- Check a closure argument against types a later argument solves (`3c1c74e49`).

### [#288](https://github.com/HerringtonDarkholme/hd-lang/issues/288) Fixtures for uncited rules in the type system (types.forms, types.never, types.no-execute…)

- Add fixtures for uncited type-system rules (`7f66278cd`).

### [#286](https://github.com/HerringtonDarkholme/hd-lang/issues/286) Fixtures for uncited rules in expressions (expr.assign, expr.interp, expr.literal…)

- Add fixtures for uncited rules in expressions (`16c1300e7`).

### [#287](https://github.com/HerringtonDarkholme/hd-lang/issues/287) Fixtures for uncited rules in expressions (expr.collection, expr.member, expr.postfix…)

- Add fixtures for uncited member, prefix and postfix rules (`5f4610fc8`).

### [#289](https://github.com/HerringtonDarkholme/hd-lang/issues/289) Fixtures for uncited rules in the type system (types.literal, types.tuple)

- Add fixtures for uncited literal and tuple rules (`05f142b4a`).

### [#290](https://github.com/HerringtonDarkholme/hd-lang/issues/290) Fixtures for uncited rules in modules (module.dep, module.kind, module.manifest…)

- Add fixtures for uncited rules in modules (`1d64873c0`).

### [#292](https://github.com/HerringtonDarkholme/hd-lang/issues/292) Fixtures for uncited rules in requirements and suspension (req.model)

- Add a fixture for req.model.runtimes (`246f18670`).

### [#291](https://github.com/HerringtonDarkholme/hd-lang/issues/291) Fixtures for uncited rules in modules (module.select, module.sum, module.version)

- Add CLI cases for version, selection and hd.sum rules (`e89f8f12e`).

### [#294](https://github.com/HerringtonDarkholme/hd-lang/issues/294) Fixtures for uncited rules in annotations (annot.decorator, annot.derive, annot.metadata…)

- Add fixtures for uncited annotation and template rules (`79faa44ec`).

### [#293](https://github.com/HerringtonDarkholme/hd-lang/issues/293) Fixtures for uncited rules in requirements and suspension (req.row)

- Add fixtures for uncited requirement row rules (`0ca387be7`).

### [#295](https://github.com/HerringtonDarkholme/hd-lang/issues/295) Fixtures for uncited rules in annotations (annot.block, annot.line, annot.no-trait…)

- Add fixtures for derivation blocks, member lines and omits (`e32d0ff2f`).

### [#253](https://github.com/HerringtonDarkholme/hd-lang/issues/253) hd check FILE checks only the folders it reaches

- Check only the modules hd check FILE reaches (`75007f549`).

### [#246](https://github.com/HerringtonDarkholme/hd-lang/issues/246) Emit one task per folder group, not per instance

- Emit one task per folder group, not per instance (`a9b2e4914`).

### [#258](https://github.com/HerringtonDarkholme/hd-lang/issues/258) One literal table per module

- Pool string literals in one table per linked module (`18ee9c8dc`).

### [#250](https://github.com/HerringtonDarkholme/hd-lang/issues/250) Print diagnostics before publishing cache entries

- Print diagnostics before publishing cache entries (`df262fb07`).

### [#257](https://github.com/HerringtonDarkholme/hd-lang/issues/257) Lower for over a list or map to a counted loop

- Lower for over a list or a map to a counted loop (`8c6c9866c`).

### [#271](https://github.com/HerringtonDarkholme/hd-lang/issues/271) Vtables as immutable constant globals

- Read trait-value vtables from constant globals (`79dc8bb4a`).

### [#268](https://github.com/HerringtonDarkholme/hd-lang/issues/268) Trial checkpoints without copying body state

- Roll trials back through an undo log, not body copies (`c95c2c059`).

### [#269](https://github.com/HerringtonDarkholme/hd-lang/issues/269) Compile matches to a decision tree

- Compile matches to a decision tree (`8366d6f05`).

### [#300](https://github.com/HerringtonDarkholme/hd-lang/issues/300) Windows CI: data/missing/../a.txt is found, not NotFound

- Find no file after .. past a missing name on every OS (`a38fe0d60`).

### [#301](https://github.com/HerringtonDarkholme/hd-lang/issues/301) Windows CI: cli/dep-sum-mismatch prints an extra JSON line

- Read hd.sum lines that end with CRLF (`59f5bbb26`).

### [#307](https://github.com/HerringtonDarkholme/hd-lang/issues/307) A code cache key misses a type layout (enum vs data with one path)

- Hash a declaration's kind and variant shape in layout_hash (`534e243f6`).

### [#302](https://github.com/HerringtonDarkholme/hd-lang/issues/302) A string value is accepted where never is expected

- Reject ordinary values where never is expected (`61c8f7f22`).

### [#273](https://github.com/HerringtonDarkholme/hd-lang/issues/273) Per-instance layout hash in the code key

- Key each instance's code by the layouts it uses (`e5f513f17`).

### [#304](https://github.com/HerringtonDarkholme/hd-lang/issues/304) A contextual variant with payload and no call reports the wrong code

- Report an unsaturated contextual variant as Enum.Variant does (`9d2186f6b`).

### [#308](https://github.com/HerringtonDarkholme/hd-lang/issues/308) Make the hd.positions section compact

- Make the hd.positions section compact (`cafc80126`).

### [#303](https://github.com/HerringtonDarkholme/hd-lang/issues/303) A second template for one trait is not reported

- Report a second template for one trait as an overlap (`bd1ddf276`).

### [#306](https://github.com/HerringtonDarkholme/hd-lang/issues/306) No derivation-line-drift warning beside a @derive of another trait

- Warn on a block with member lines beside a @derive (`9737263fa`).

### [#298](https://github.com/HerringtonDarkholme/hd-lang/issues/298) TIR: control flow inside a rejected defer suite breaks invariant 6

- Keep rejected control flow out of defer suites in TIR (`0d19ee451`).

### [#309](https://github.com/HerringtonDarkholme/hd-lang/issues/309) A return inside a closure in a defer suite is rejected

- Accept a return inside a closure in a defer suite (`0f5de2255`).

### [#299](https://github.com/HerringtonDarkholme/hd-lang/issues/299) TIR: it_prop registration drops default arguments (invariant 15)

- Pass a test registration's arguments to its case body unchanged (`9d39cb9f5`).

### [#242](https://github.com/HerringtonDarkholme/hd-lang/issues/242) Report item-too-complex when body fuel runs out

- Report item-too-complex for init, test and fact bodies (`0eadd7628`).

### [#297](https://github.com/HerringtonDarkholme/hd-lang/issues/297) TIR: LocalSet values and loop types break verifier invariant 4

- Make stored and break values carry their target's type (`f404fb380`).

### [#212](https://github.com/HerringtonDarkholme/hd-lang/issues/212) A method with a row parameter cannot be called through dyn

- Call a method with a row parameter through a trait value (`bdd08400f`).

### [#267](https://github.com/HerringtonDarkholme/hd-lang/issues/267) Join sites widen without rechecking values

- Check each value of a least-common-type site once (`67f445c9c`).

### [#272](https://github.com/HerringtonDarkholme/hd-lang/issues/272) Wakers with wake masks: all! polls only woken children

- Poll only the woken children of an all! (`5cf9ae7fd`).

### [#274](https://github.com/HerringtonDarkholme/hd-lang/issues/274) Non-null reference locals

- Declare non-null reference locals written at declaration (`e2d3ad654`).

### [#176](https://github.com/HerringtonDarkholme/hd-lang/issues/176) Row-polymorphic functions are compiled once per row

- Compile a row-polymorphic function once for every row (`097a279be`).

### [#221](https://github.com/HerringtonDarkholme/hd-lang/issues/221) Hash only API text for the folder interface key

- Leave top-level statements out of the api text hash (`6a8b352c2`).

### [#223](https://github.com/HerringtonDarkholme/hd-lang/issues/223) Keep impls of private types out of the dependents' interface hash

- Keep impls of private types out of the interface hash dependents read (`3cf9099c6`).

### [#310](https://github.com/HerringtonDarkholme/hd-lang/issues/310) A warm run replays header findings a parse error suppresses

- Block a cached folder interface past a parse error (`c623a799c`).

### [#224](https://github.com/HerringtonDarkholme/hd-lang/issues/224) Make the coherence overlap check near-linear

- Unify each impl head only with heads its constructors can match (`96df48a20`).

### [#231](https://github.com/HerringtonDarkholme/hd-lang/issues/231) Link the forbidden-context check only when a bracketed default exists

- Link block_on's forbidden-context check only when a context raises it (`6e007603c`).

### [#311](https://github.com/HerringtonDarkholme/hd-lang/issues/311) hd build --release panics on a program with @derive

- Map no line for a derived instance's body in hd.positions (`41f3f9929`).

### [#222](https://github.com/HerringtonDarkholme/hd-lang/issues/222) Carry template bodies and hidden items in folder interfaces

- Key template users on what a template's check reads (`39ac5db8e`).

### [#254](https://github.com/HerringtonDarkholme/hd-lang/issues/254) Run playground programs in their own worker

- Run playground programs in a worker of their own (`7f13bbd99`).

### [#312](https://github.com/HerringtonDarkholme/hd-lang/issues/312) A template's private helper infers its omitted row instead of the empty row

- Give a template's private helper the empty row when it writes none (`0a94a56b4`).

### [#232](https://github.com/HerringtonDarkholme/hd-lang/issues/232) Inline constant default arguments

- Inline a constant default argument at the call (`655dcec61`).

### [#213](https://github.com/HerringtonDarkholme/hd-lang/issues/213) Read folder interfaces and cache entries lazily

- Read impls and traits through the interface's item index (`369893e51`).

### [#216](https://github.com/HerringtonDarkholme/hd-lang/issues/216) Pack code entries per folder group and share relocation targets

- Pack each folder group's code into one cache entry (`4c6a9ad52`).

### [#313](https://github.com/HerringtonDarkholme/hd-lang/issues/313) Link the all! wake-node runtime only when a program uses all!

- Link the all! waker-node runtime only when a program polls one (`0245af547`).

### [#234](https://github.com/HerringtonDarkholme/hd-lang/issues/234) TypeId is allocated on every runtime_type call

- Build each TypeId once and read it from a global after (`d5adfe808`).

### [#235](https://github.com/HerringtonDarkholme/hd-lang/issues/235) Resume suspended bodies through a br_table dispatch

- Resume a suspending block list through one br_table (`906632ed4`).

### [#236](https://github.com/HerringtonDarkholme/hd-lang/issues/236) Save only live locals at suspension points

- Save only the locals a resume reads (`8bc937215`).

### [#237](https://github.com/HerringtonDarkholme/hd-lang/issues/237) Use i31ref for small erased scalars

- Erase a small i32 as an i31ref (`730d7754d`).

### [#249](https://github.com/HerringtonDarkholme/hd-lang/issues/249) One graph cache entry per package for coherence and init order

- Keep coherence and init order in one graph entry per package (`78221dad9`).

### [#226](https://github.com/HerringtonDarkholme/hd-lang/issues/226) Verify every TIR invariant

- Verify the TIR invariants that need the checker's facts (`8afd63ef4`).

### [#314](https://github.com/HerringtonDarkholme/hd-lang/issues/314) CI red: REPL member_lines_survive_a_rebuild fails since d5adfe808

- Keep a TypeId's cached global local to its module (`b92c4c546`).

### [#305](https://github.com/HerringtonDarkholme/hd-lang/issues/305) A derivation block for a trait with no template is accepted

- Reject a derivation block for a trait with no template (`8d5046e88`).

### [#9](https://github.com/HerringtonDarkholme/hd-lang/issues/9) A host trait returning a handle stops at Link (resource-disposed-result)

- Return a profile's own handle trait value from a host method (`a1145c1b3`).

### [#248](https://github.com/HerringtonDarkholme/hd-lang/issues/248) Key folder interfaces by what they mention

- Key folder interfaces by what their headers read (`c44179dc8`).

### [#316](https://github.com/HerringtonDarkholme/hd-lang/issues/316) The link cache key misses inputs that decide link-time errors

- Key a program's link by the declarations and registrations it reads (`d06858049`).

### [#318](https://github.com/HerringtonDarkholme/hd-lang/issues/318) Fixtures for uncited rules in expressions (expr.index, expr.literal, expr.member)

- Add fixtures for the uncited indexing rules of chapter 05 (`113dc27c7`).

### [#319](https://github.com/HerringtonDarkholme/hd-lang/issues/319) Fixtures for uncited rules in expressions (expr.call)

- Add fixtures for the uncited call rules of chapter 05 (`64545e220`).

### [#320](https://github.com/HerringtonDarkholme/hd-lang/issues/320) Fixtures for uncited rules in the type system (types.alias, types.fn, types.forms…)

- Add fixtures for the uncited type-system rules of chapter 04 (`21a8def65`).

### [#321](https://github.com/HerringtonDarkholme/hd-lang/issues/321) Fixtures for uncited rules in the type system (types.arith, types.assign, types.cast…)

- Add fixtures for the cast, display and assignability rules of chapter 04 (`78e0536a8`).

### [#322](https://github.com/HerringtonDarkholme/hd-lang/issues/322) Fixtures for uncited rules in modules (module.dep, module.manifest, module.repo…)

- Add CLI cases for the uncited package rules of chapter 10 (`373067fb1`).

### [#324](https://github.com/HerringtonDarkholme/hd-lang/issues/324) Fixtures for uncited rules in control flow (flow.block, flow.for, flow.if…)

- Add fixtures for the uncited control-flow rules of chapter 06 (`89fd56d12`).

### [#326](https://github.com/HerringtonDarkholme/hd-lang/issues/326) Fixtures for uncited rules in functions (fn.arg, fn.body, fn.decl…)

- Add fixtures for the uncited function rules of chapter 07 (`eefb3c386`).

### [#328](https://github.com/HerringtonDarkholme/hd-lang/issues/328) Fixtures for uncited rules in names and scopes (names.category, names.exec, names.init…)

- Add fixtures for the uncited name and scope rules of chapter 03 (`1dc4d7b27`).

### [#323](https://github.com/HerringtonDarkholme/hd-lang/issues/323) Fixtures for uncited rules in modules (module.path, module.path-dep, module.root…)

- Add fixtures for the uncited package and test-module rules of chapter 10 (`b941b3f14`).

### [#327](https://github.com/HerringtonDarkholme/hd-lang/issues/327) Fixtures for uncited rules in functions (fn.ref, fn.type)

- Add fixtures for the uncited function type rules of chapter 07 (`d52efca66`).

### [#325](https://github.com/HerringtonDarkholme/hd-lang/issues/325) Fixtures for uncited rules in control flow (flow.match)

- Add fixtures for the uncited match rules of chapter 06 (`eec95f290`).

### [#329](https://github.com/HerringtonDarkholme/hd-lang/issues/329) Fixtures for uncited rules in names and scopes (names.capture, names.comp, names.generic…)

- Add fixtures for the uncited local and member rules of chapter 03 (`8b9289e4c`).

### [#228](https://github.com/HerringtonDarkholme/hd-lang/issues/228) REPL: rebuild the session from the journal past a memory limit

- Rebuild a REPL session past 1000 inputs into one instance (`856159f75`).

### [#317](https://github.com/HerringtonDarkholme/hd-lang/issues/317) Single-copy join and StringBuilder, word-wide StrEq, the Go way

- Join strings with one copy, and compare them eight bytes a step (`0b1429b47`).

### [#179](https://github.com/HerringtonDarkholme/hd-lang/issues/179) Single-pass string join (concat_all), waits on a spec edit

- Join strings with one copy, and compare them eight bytes a step (`0b1429b47`).

### [#251](https://github.com/HerringtonDarkholme/hd-lang/issues/251) Generate host stubs and JS glue from hd_host_abi

- Generate the wasmtime host's codecs and method table from the ABI model (`69ccf1dae`).

### [#333](https://github.com/HerringtonDarkholme/hd-lang/issues/333) Init order: a method call can read a binding before it is initialized

- Follow method calls to their bodies in the init-order check (`117b6f99f`).

### [#332](https://github.com/HerringtonDarkholme/hd-lang/issues/332) Init order: a function value read before init reports the wrong code

- Follow function values to their bodies in the init-order check (`ee2e06be7`).

### [#334](https://github.com/HerringtonDarkholme/hd-lang/issues/334) A value parameter may reuse a type parameter's name

- Reject a parameter that reuses a type parameter's name (`b1790837a`).

### [#337](https://github.com/HerringtonDarkholme/hd-lang/issues/337) Two value parameters may share a name

- Reject two value parameters that share a name (`edfd7f84a`).

### [#330](https://github.com/HerringtonDarkholme/hd-lang/issues/330) Index with a literal key does not pick Index[usize]

- Choose Index and IndexSet by a literal key's default type (`543e95f6e`).

### [#331](https://github.com/HerringtonDarkholme/hd-lang/issues/331) No fix-it for a narrower integer where a wider one is expected

- Offer a fix-it that wraps a narrower number in the wider type (`bdf75acaa`).

### [#336](https://github.com/HerringtonDarkholme/hd-lang/issues/336) A dependency's minimum hd version is not checked

- Check the minimum toolchain of every reached path dependency (`55c7bf485`).

### [#335](https://github.com/HerringtonDarkholme/hd-lang/issues/335) Driver conformance does not compile a package tree's doc tests

- Check a package-tree fixture's doc tests in driver conformance (`538114ef9`).

### [#338](https://github.com/HerringtonDarkholme/hd-lang/issues/338) string(x) passes the checker and fails at emit

- Reject constructor-style calls that are not numeric casts (`f5e5e7bac`).

### [#296](https://github.com/HerringtonDarkholme/hd-lang/issues/296) Remove --max-heap and --max-memory; say hd fix edits sources only

- Remove --max-heap and --max-memory; hd fix edits sources only (`b077b9f40`).

### [#255](https://github.com/HerringtonDarkholme/hd-lang/issues/255) Cache compiled wasmtime code across runs

- Keep Cranelift machine code across runs in clpack entries (`624df3f3b`).

### [#177](https://github.com/HerringtonDarkholme/hd-lang/issues/177) Finer cache keys for Coherence and InitOrder

- Check coherence again only for the traits whose impls changed (`9f5c5d905`).

### [#256](https://github.com/HerringtonDarkholme/hd-lang/issues/256) std.net: a deterministic Net provider for tests

- Specify ScriptedNet, net timeouts, options, and local address (`c60d0173c`).

### [#339](https://github.com/HerringtonDarkholme/hd-lang/issues/339) Implement std.net timeouts and connect_timeout! on both hosts

- Implement std.net timeouts and connect_timeout! on both hosts (`7283ddcd9`).

### [#340](https://github.com/HerringtonDarkholme/hd-lang/issues/340) Implement TcpStream nodelay and keep-alive on both hosts

- Implement TcpStream nodelay and keep-alive on both hosts (`6b40de480`).

### [#341](https://github.com/HerringtonDarkholme/hd-lang/issues/341) Implement local_address on TcpListener and UdpSocket

- Implement local_address on TcpListener and UdpSocket on both hosts (`b5272a529`).

### [#315](https://github.com/HerringtonDarkholme/hd-lang/issues/315) Profile host-trait results skip the boundary-private-field check

- Add a host-trait private-field fixture for ResourceError results (`cfbd9f1e4`).

### [#342](https://github.com/HerringtonDarkholme/hd-lang/issues/342) A tuple index key leaves Index's Out unresolved

- Type a literal tuple index key before reading Index's Out (`5d9e3f7d6`).

### [#343](https://github.com/HerringtonDarkholme/hd-lang/issues/343) An unknown method before two diverging matches hits a TIR verifier error

- Close join sites after filling them; report internal checks as bugs (`9afd09500`).

### [#344](https://github.com/HerringtonDarkholme/hd-lang/issues/344) Show how hd meets the three agentic pillars, with measured numbers

- Record the first goal-metrics baseline and cite it in the README (`1dfcadc0b`).

### [#346](https://github.com/HerringtonDarkholme/hd-lang/issues/346) One trait with one impl reports ambiguous-method in a large package

- Count a module's own trait once in its method index (`493f95da4`).

### [#347](https://github.com/HerringtonDarkholme/hd-lang/issues/347) Fix metric harness defects: stale corpus path, --jobs, HD_CACHE probe

- Fix the metric harness defects and refresh the baseline (`673c034e1`).

### [#345](https://github.com/HerringtonDarkholme/hd-lang/issues/345) A top-level let with a pattern is unsupported

- Support any pattern in a top-level let (`caa69d93a`).

### [#351](https://github.com/HerringtonDarkholme/hd-lang/issues/351) A return nested in top-level code is accepted and ends init

- Report a return nested in top-level code as return-outside-function (`68eceb031`).

### [#352](https://github.com/HerringtonDarkholme/hd-lang/issues/352) Substring search is O(n·m); use Two-Way

- Search substrings with Two-Way instead of trying every start (`79abfba8b`).

### [#363](https://github.com/HerringtonDarkholme/hd-lang/issues/363) Embed std without plain comments (web gzip gate has 145 B left)

- Embed lib/std without plain comments, lexer-aware (`c904f58f6`).

### [#364](https://github.com/HerringtonDarkholme/hd-lang/issues/364) HTTP: status as an error, get_text!, json and post_json!

- Add Response.ok, get_text!, Response.json and post_json! to std.http (`3fbf87919`).

### [#356](https://github.com/HerringtonDarkholme/hd-lang/issues/356) Float text fast paths in both hosts

- Print and parse floats with the engines' native code (`a66c3c1c8`).

### [#358](https://github.com/HerringtonDarkholme/hd-lang/issues/358) Number parsers on bytes instead of split("")

- Read number text as bytes in the std parsers (`813bb1f15`).

### [#359](https://github.com/HerringtonDarkholme/hd-lang/issues/359) JSON encode straight to text, render runs as slices

- Encode JSON straight to text and escape strings by runs (`b9e0256df`).

### [#360](https://github.com/HerringtonDarkholme/hd-lang/issues/360) JSON decode on the string's bytes with lazy error paths

- Parse JSON on the input's bytes and read it with lazy error paths (`7c95d1eb7`).

### [#354](https://github.com/HerringtonDarkholme/hd-lang/issues/354) Regex: VM state once per search, bytes not a Haystack copy

- Run the regex Pike VM on the text's bytes with reused sparse sets (`7cfa0bc1c`).

### [#355](https://github.com/HerringtonDarkholme/hd-lang/issues/355) Regex literal prefilter

- Skip to candidate starts in std regex with a literal prefilter (`b2b264f22`).

### [#365](https://github.com/HerringtonDarkholme/hd-lang/issues/365) Free helpers for every std.fs and std.process host method

- Add free bang helpers for every std.fs and std.process method (`34227f893`).

### [#366](https://github.com/HerringtonDarkholme/hd-lang/issues/366) One-off errors: error.message and Option.context

- Add std.error.message and Option.context for one-off errors (`6e0072889`).

### [#367](https://github.com/HerringtonDarkholme/hd-lang/issues/367) std.fs.walk! and glob!

- Add std.fs.walk! and glob! in plain hd over FsRead (`564884b42`).

### [#368](https://github.com/HerringtonDarkholme/hd-lang/issues/368) Collection and text atoms (Set collect, entries, max_by_key, dedup, most_common, rfind, splitn)

- Add Set collect, Map.entries, by-key extremes, dedup, rfind, splitn (`ef561df70`).

### [#369](https://github.com/HerringtonDarkholme/hd-lang/issues/369) Concurrency helpers: timeout!, bounded map_all!, retry when-predicate

- Add std.task timeout!, map_all!, and a retry when predicate (`2ead79887`).

### [#370](https://github.com/HerringtonDarkholme/hd-lang/issues/370) Time helpers: monotonic(), elapsed(), Duration::parse, ISO-8601 and format

- Add std.time monotonic, elapsed, Duration::parse, to_iso8601, format (`cc8ad6ead`).

### [#371](https://github.com/HerringtonDarkholme/hd-lang/issues/371) JSON: declared defaults on decode, pointer, as_i64, encode_pretty

- Add JSON decode defaults, Json.pointer, as_i64, encode_pretty (`c1142ddc0`).

### [#372](https://github.com/HerringtonDarkholme/hd-lang/issues/372) A Parse trait and string.parse::[T]()

- Add Parse trait, string.parse, cli value_as and host env_as (`85a63cd93`).

### [#373](https://github.com/HerringtonDarkholme/hd-lang/issues/373) Integer abs/pow and float floor/round/abs in std

- Add integer abs/pow and exact float trunc/floor/ceil/round/abs (`d04113dbf`).

### [#374](https://github.com/HerringtonDarkholme/hd-lang/issues/374) Fix the 16 design findings of the stdlib audit

- Fix stdlib audit design findings: process errors, names, Rng, cli doc (`ebcb9a06d`).

### [#380](https://github.com/HerringtonDarkholme/hd-lang/issues/380) Checker crash: loop break values with different rows

- Join loop break values like if branches, fixing a checker crash (`86d03d467`).

### [#399](https://github.com/HerringtonDarkholme/hd-lang/issues/399) Node host: a link followed by .. escapes a file grant

- Resolve a link before the .. after it in file grant checks (`1d72c5d8e`).

### [#383](https://github.com/HerringtonDarkholme/hd-lang/issues/383) A header goal that runs out of solver fuel reports nothing

- Report item-too-complex when a header goal runs out of fuel (`236719949`).

### [#401](https://github.com/HerringtonDarkholme/hd-lang/issues/401) Node host: unread process input returns Err instead of Ok

- Node host: a child that leaves stdin unread still returns Ok (`5c8954c63`).

### [#348](https://github.com/HerringtonDarkholme/hd-lang/issues/348) hd check and hd build reject --jobs N

- Accept --jobs N in hd check, build and the run commands (`2bf621283`).

### [#386](https://github.com/HerringtonDarkholme/hd-lang/issues/386) A host panic during a fact is not wrapped as fact-evaluation-failed

- Wrap host-raised panics in a fact as fact-evaluation-failed (`97cc7b541`).

### [#382](https://github.com/HerringtonDarkholme/hd-lang/issues/382) A long else-if chain reports nesting-too-deep

- Parse an else-if chain as one node, so it adds no nesting (`f92a6b295`).

### [#402](https://github.com/HerringtonDarkholme/hd-lang/issues/402) Node host: TLS alerts such as unknown_ca are classified as Connect

- Node host: classify TLS alerts as Tls (`5b3b36a7e`).

### [#403](https://github.com/HerringtonDarkholme/hd-lang/issues/403) Node host: response headers are cut at 1000 pairs

- Node host: keep every response header pair (`d847f94b5`).

### [#404](https://github.com/HerringtonDarkholme/hd-lang/issues/404) Node host: a read maximum with the high bit set reads as negative

- Node host: read a usize maximum as unsigned (`78db02c36`).

### [#379](https://github.com/HerringtonDarkholme/hd-lang/issues/379) process.output! and ProcessError.Exited for non-zero status

- Add std.process.output! and ProcessError.Exited (`d854d6345`).

### [#411](https://github.com/HerringtonDarkholme/hd-lang/issues/411) Rename unknown-data-field to unknown-field; tuple selection past the end uses it

- Rename unknown-data-field to unknown-field; tuple select uses it (`5be3959c7`).

### [#410](https://github.com/HerringtonDarkholme/hd-lang/issues/410) Spec edits from the #183 answers that need no code

- Apply owner answers D5, D8, D10, D18, D19 to the spec (`31c1e54c2`).

### [#412](https://github.com/HerringtonDarkholme/hd-lang/issues/412) Diagnostic codes from the #183 answers

- Report impl of Suspend as an unknown trait, D22 (`cbb8ea1c8`).

### [#413](https://github.com/HerringtonDarkholme/hd-lang/issues/413) Reject polymorphic recursion whose type arguments grow

- Reject a growing generic call cycle at check time, D1 (`ca3a1c5cb`).

### [#414](https://github.com/HerringtonDarkholme/hd-lang/issues/414) private-type-leak for an associated type binding

- Report a private type bound by a public impl's associated type, D2 (`40752d976`).

### [#415](https://github.com/HerringtonDarkholme/hd-lang/issues/415) Copy-update snapshots its source at the spread

- Read a copy-update's copied fields when the spread is evaluated (`08d273c5a`).

### [#431](https://github.com/HerringtonDarkholme/hd-lang/issues/431) std adds impl Error for string

- std adds impl Error for string (`4488f6585`).

### [#452](https://github.com/HerringtonDarkholme/hd-lang/issues/452) std.path: file_stem and with_extension

- Add Path.file_stem and Path.with_extension (`d0639eef4`).

### [#442](https://github.com/HerringtonDarkholme/hd-lang/issues/442) Error.exit_code chooses the exit status; Termination reads it

- Error.exit_code chooses the exit status; Termination reads it (`7f3ac4534`).

### [#453](https://github.com/HerringtonDarkholme/hd-lang/issues/453) std.path: impl Parse for Path

- Add impl Parse for Path (`e4d917da5`).

### [#460](https://github.com/HerringtonDarkholme/hd-lang/issues/460) std.time: remove Duration::milliseconds and Duration::seconds

- Remove Duration::milliseconds and Duration::seconds (`d03a11fe5`).

### [#461](https://github.com/HerringtonDarkholme/hd-lang/issues/461) std.time: one ParseTimeError for all time text

- Merge ParseDurationError into ParseTimeError (`f33392e25`).

### [#462](https://github.com/HerringtonDarkholme/hd-lang/issues/462) std.time: Parse for Duration and Timestamp

- Add Parse for Duration and Timestamp (`0358d410f`).

### [#464](https://github.com/HerringtonDarkholme/hd-lang/issues/464) StringBuilder: push_char, and push returns the builder

- Add StringBuilder.push_char and make push chain (`ef7bd7d37`).

### [#509](https://github.com/HerringtonDarkholme/hd-lang/issues/509) Web compiler embeds std without ## doc comments (gzip gate crossed)

- Drop std ## doc comments from the web build's embedded std (`5fc29bc06`).

### [#472](https://github.com/HerringtonDarkholme/hd-lang/issues/472) std.encoding: Base64 enum for the four alphabets

- Add std.encoding Base64 enum with encode and decode (`71d594c89`).

### [#473](https://github.com/HerringtonDarkholme/hd-lang/issues/473) std.digest: Digest with constant-time equality; sha256 returns it

- std.digest: Digest with constant-time equality; sha256 returns it (`2816e1f48`).

### [#481](https://github.com/HerringtonDarkholme/hd-lang/issues/481) std.digest: Sha256 streams with update and finish

- std.digest: streaming Sha256; sha256 wraps it without a message copy (`d41290b69`).

### [#497](https://github.com/HerringtonDarkholme/hd-lang/issues/497) std.digest: Sha256::hmac keys the hash

- std.digest: Sha256::hmac keys the hasher for HMAC-SHA256 (`3440fa757`).

### [#476](https://github.com/HerringtonDarkholme/hd-lang/issues/476) std.num: wrapping_mul on every integer

- std.num: wrapping_mul on every integer width (`f14c1a5fe`).

### [#478](https://github.com/HerringtonDarkholme/hd-lang/issues/478) std.num: total_cmp on floats; is_nan and is_finite for f32

- std.num: total_cmp on floats; is_nan and is_finite on f32 (`39f918650`).

### [#485](https://github.com/HerringtonDarkholme/hd-lang/issues/485) std.option: Option.or

- std.option: Option.or layers fallbacks (`eb6af611f`).

### [#486](https://github.com/HerringtonDarkholme/hd-lang/issues/486) Result.expect panic shows the message and the error

- std.result: expect shows the error in its panic (`1181d679a`).

### [#484](https://github.com/HerringtonDarkholme/hd-lang/issues/484) std.cmp: Ordering.then

- std.cmp: Ordering.then chains comparator keys (`726e4f09b`).

### [#420](https://github.com/HerringtonDarkholme/hd-lang/issues/420) hd test --update rewrites failing in-code snapshots

- Rewrite failing in-code snapshots under hd test --update (`5c59bb9e0`).
- Let an update run accept every in-code snapshot in one pass (`cf5d3e1f0`).
- Restore the list numbers after the snapshot update rules (`17cbdaefb`).

### [#353](https://github.com/HerringtonDarkholme/hd-lang/issues/353) Map's string hash is unseeded FNV-1a (hash flooding)

- Key Map's string hash with a per-run seed (HalfSipHash-1-3) (`8d8240a8f`).

### [#510](https://github.com/HerringtonDarkholme/hd-lang/issues/510) hd test --update does not rewrite doc-test snapshots

- Rewrite failing doc-test snapshots under hd test --update (`c8dd1dd84`).

### [#507](https://github.com/HerringtonDarkholme/hd-lang/issues/507) Duration becomes non-negative

- Make Duration non-negative, as Rust's std::time::Duration (`fd6ef8da6`).

### [#441](https://github.com/HerringtonDarkholme/hd-lang/issues/441) Serialize and Deserialize for Set and tuples

- Serialize and Deserialize for Set and tuples (`f10aefaef`).

### [#511](https://github.com/HerringtonDarkholme/hd-lang/issues/511) Map integer and composite keys still bucket by an unkeyed hash

- Key Map's integer and composite bucket hashes with the run's seed (`2a274b8ad`).

### [#477](https://github.com/HerringtonDarkholme/hd-lang/issues/477) std.num: to_radix and parse_radix

- std.num radix: restore the cases.tsv rows; cite exact rule IDs (`ac3339343`).

### [#463](https://github.com/HerringtonDarkholme/hd-lang/issues/463) std.json: JsonObject.remove; drop Json.as_number

- std.json: add JsonObject.remove, drop Json.as_number (`2c342fe9e`).

### [#465](https://github.com/HerringtonDarkholme/hd-lang/issues/465) std.text: Utf8Error.Truncated carries a position; drop the free join

- std.text: Utf8Error.Truncated carries a position; drop the free join (`a2123994f`).

### [#449](https://github.com/HerringtonDarkholme/hd-lang/issues/449) std.fs: remove exists!

- std.fs: remove exists!; use stat!(path)?.is_some() (`cade77dce`).

### [#474](https://github.com/HerringtonDarkholme/hd-lang/issues/474) std.random: free fill; remove token_hex

- std.random: add free fill, remove token_hex (`40b32178f`).

### [#435](https://github.com/HerringtonDarkholme/hd-lang/issues/435) Integer converts to and from u64

- std.num: Integer converts to and from u64 (`4acc3adbb`).

### [#471](https://github.com/HerringtonDarkholme/hd-lang/issues/471) std.regex: one RegexError enum with a position in each variant

- std.regex: one RegexError enum with a position in each variant (`d181c4fac`).

### [#482](https://github.com/HerringtonDarkholme/hd-lang/issues/482) std.random: Rng.int is generic over the integer type

- std.random: Rng.int is generic over the integer type (`8ea029375`).

### [#434](https://github.com/HerringtonDarkholme/hd-lang/issues/434) Hasher gains write_u64 and write_str

- Hasher gains write_u64 and write_str with default bodies (`82a37fa31`).

### [#357](https://github.com/HerringtonDarkholme/hd-lang/issues/357) Integer text without a string per digit

- Show integers through the runtime's IntToStr with format_int (`51b2646d4`).

### [#424](https://github.com/HerringtonDarkholme/hd-lang/issues/424) sqrt as an intrinsic method on f32 and f64

- Add sqrt as an intrinsic method on f32 and f64 (`dfbc7b62e`).

### [#483](https://github.com/HerringtonDarkholme/hd-lang/issues/483) std.testing: Choices.int and Choices.float take a range

- std.testing: Choices.int and Choices.float take a range (`b828762f5`).

### [#416](https://github.com/HerringtonDarkholme/hd-lang/issues/416) Panic when a map compacts during a live iteration

- Panic when a map rebuilds its entries during a live iteration (`28d0cb4d7`).

### [#513](https://github.com/HerringtonDarkholme/hd-lang/issues/513) Range expressions accept float bounds

- Accept float range bounds; a float range stays non-iterable (`356784329`).

### [#417](https://github.com/HerringtonDarkholme/hd-lang/issues/417) Named error for a type-parameter field in shared enum data

- Name the error for a type-parameter field in shared enum data (`be63e07bd`).

### [#418](https://github.com/HerringtonDarkholme/hd-lang/issues/418) One requirement key when a supertrait fixes the binding

- One requirement key when a supertrait fixes the binding (`f7b835813`).

### [#419](https://github.com/HerringtonDarkholme/hd-lang/issues/419) println in non-entry module init reports both errors

- Report both errors for println in non-entry module init (`e97d9b11d`).

### [#421](https://github.com/HerringtonDarkholme/hd-lang/issues/421) A general code for hd.toml errors without one

- Give hd.toml errors without a code one general code (`cff224c75`).
- Report a TOML syntax error in hd.toml as invalid-manifest (`e3d78deeb`).

### [#422](https://github.com/HerringtonDarkholme/hd-lang/issues/422) A conformance fixture format for REPL sessions

- Add REPL session fixtures: hd repl --script and a transcript check (`d50644dd1`).

### [#423](https://github.com/HerringtonDarkholme/hd-lang/issues/423) Detect a fact that reads itself

- Panic at once when a fact reads itself (`ccdb1ef79`).

### [#454](https://github.com/HerringtonDarkholme/hd-lang/issues/454) A test whose result holds .Err fails whatever its exit code

- Fail a test case whose result holds .Err, whatever its exit code (`931be7c43`).

### [#433](https://github.com/HerringtonDarkholme/hd-lang/issues/433) ConsoleInput.read_all! reads all of stdin in one call

- Add ConsoleInput.read_all! and the read_all! helper (`7bf584221`).

### [#517](https://github.com/HerringtonDarkholme/hd-lang/issues/517) Scripts get the REPL's top level: bang calls and the REPL's capability grant

- Scripts get the REPL's top level: bang calls and `?` (`e17cc69ac`).

### [#444](https://github.com/HerringtonDarkholme/hd-lang/issues/444) Command builder; Process.run! takes a Command

- Command builder: Process.run! takes a Command (`d9c1d029a`).

### [#519](https://github.com/HerringtonDarkholme/hd-lang/issues/519) Cancellation 1: cancel!() waits, one cleanup per job

- cancel!() waits for the cleanup; one cleanup per suspension (`1464721d8`).

### [#520](https://github.com/HerringtonDarkholme/hd-lang/issues/520) Cancellation 2: abandon() unwinds and kills waiting disposes

- abandon() unwinds a job at once and kills waiting disposes (`a5a61282f`).

### [#521](https://github.com/HerringtonDarkholme/hd-lang/issues/521) Cancellation 3: combinators fan out cleanup; awaiting a cancelled job cancels the awaiter

- Awaiting a cancelled job cancels the awaiter; combinators fan out (`a2708d121`).

### [#522](https://github.com/HerringtonDarkholme/hd-lang/issues/522) Cancellation 4: timeout! cleanup bound and TimedOut.cleanup

- timeout! gains a cleanup bound; TimedOut reports how cleanup ended (`70acd38d9`).

### [#523](https://github.com/HerringtonDarkholme/hd-lang/issues/523) Cancellation 5: block_on returns BlockOn[T]

- block_on returns BlockOn[T]; a cancelled drive ends after its cleanup (`6907a3a5c`).

### [#528](https://github.com/HerringtonDarkholme/hd-lang/issues/528) A cleanup that folds an abandoned job's cleanup ends .Finished, not .Abandoned

- An await of an abandoned job carries .Abandoned to the awaiter (`7d7cb1c5a`).

### [#529](https://github.com/HerringtonDarkholme/hd-lang/issues/529) Tier 1 T1: struct declares a value type

- struct declares a value type, copied on every binding and call (`e60234d86`).

### [#530](https://github.com/HerringtonDarkholme/hd-lang/issues/530) Tier 1 T2: reference syntax &T, &mut T, &e, *r

- Reference syntax: &T, &mut T, &e, &mut e and *e parse and format (`2e363e5c1`).

### [#531](https://github.com/HerringtonDarkholme/hd-lang/issues/531) Tier 1 T1b: mut parameters and mut self as mutable copies; lost-write warning

- mut parameters and mut self are the callee's own changeable copy (`e37fc3db7`).

### [#525](https://github.com/HerringtonDarkholme/hd-lang/issues/525) Node host: a child that stops reading stdin can fail with ENOTCONN on macOS

- Handle closed child stdin reporting ENOTCONN on macOS (`6b9b80955`).

### [#524](https://github.com/HerringtonDarkholme/hd-lang/issues/524) Cancellation 6: host cancellation of the entry with a default cleanup bound

- Host cancels the entry under a cleanup bound; Ctrl-C exits 130 (`89901b692`).

### [#527](https://github.com/HerringtonDarkholme/hd-lang/issues/527) timeout! without cleanup still links the cleanup-bound path (+14.7 KB)

- Specialize constant defaults before collecting callees (`769af8124`).

### [#532](https://github.com/HerringtonDarkholme/hd-lang/issues/532) Tier 1 T3: references in the checker, emulated on Wasm GC

- References are checked and run: &e, &mut e and *e on Wasm GC (`a6cead348`).

### [#375](https://github.com/HerringtonDarkholme/hd-lang/issues/375) Flaky: no_shebang_executable test races on ETXTBSY in CI

- Create no-shebang fixture outside the test process (`34bf0498e`).

### [#385](https://github.com/HerringtonDarkholme/hd-lang/issues/385) Module path case folding is not the full Unicode fold

- Use full Unicode case folding for module paths (`4ae94663d`).

### [#384](https://github.com/HerringtonDarkholme/hd-lang/issues/384) f32 literals are rounded twice (decimal to f64 to f32)

- Round f32 literals directly at their inferred width (`85296cf93`).

### [#398](https://github.com/HerringtonDarkholme/hd-lang/issues/398) dbg-in-dependency is never reported

- Suppress dbg output from fetched dependencies (`88f4c79f1`).

### [#534](https://github.com/HerringtonDarkholme/hd-lang/issues/534) SIGINT sent only to hd is not forwarded to the Node engine's child

- Pass SIGINT and SIGTERM sent to hd on to the Node engine's child (`d21c86ed5`).

### [#533](https://github.com/HerringtonDarkholme/hd-lang/issues/533) Tier 1 T4: identity and equality (is, structural derived Eq/Hash, deep_equal, Id[T])

- Reference identity and equality: is, derived Eq/Hash, deep_equal, Id (`8674510b6`).

### [#538](https://github.com/HerringtonDarkholme/hd-lang/issues/538) RC1: linear-memory layout record in crates/hd_wasm/DESIGN.md

- Record the linear-memory layout in hd_wasm's design (`8f2932813`).

### [#381](https://github.com/HerringtonDarkholme/hd-lang/issues/381) REPL: a derivation built after a type's first derivation misses later facts

- Keep each REPL derivation's facts snapshot independent (`9debb1025`).

### [#393](https://github.com/HerringtonDarkholme/hd-lang/issues/393) Messages for panics and test module paths

- Explain string boundaries and test import failures (`731047497`).

### [#409](https://github.com/HerringtonDarkholme/hd-lang/issues/409) Grant refusal hint names the unresolved link path

- Name resolved targets in filesystem grant refusal hints (`550ece4e3`).

### [#537](https://github.com/HerringtonDarkholme/hd-lang/issues/537) Tier 1 T5: &self and &mut self receivers; structs implement Iterator

- &self and &mut self receivers; calls take a place's address (`77fc7523f`).

### [#429](https://github.com/HerringtonDarkholme/hd-lang/issues/429) Metrics: string-build compares unequal work

- Compare string-build throughput at equal text sizes (`988fcc265`).

### [#405](https://github.com/HerringtonDarkholme/hd-lang/issues/405) Native DNS lookups cannot be cancelled

- Cancel HTTP DNS waits before the OS lookup finishes (`f402a1242`).

### [#389](https://github.com/HerringtonDarkholme/hd-lang/issues/389) Messages for syntax written in another language

- Explain common foreign syntax with the hd spelling (`d65a2e158`).
- Build foreign-syntax suggestions from the written input (`8b5fe3270`).
- Explain endless loops and signed literal casts (`4904c2841`).

### [#539](https://github.com/HerringtonDarkholme/hd-lang/issues/539) RC2: --backend linear switch and conformance on both backends

- Add --backend linear and run conformance on both backends (`d9d7ef792`).
- Run only listed linear-backend cases in default conformance (`4564d1b50`).

### [#543](https://github.com/HerringtonDarkholme/hd-lang/issues/543) Tier 1 T6: trait values as &dyn and &mut dyn; AnyVal and AnyRef redefined

- Trait values as &dyn and &mut dyn; AnyVal and AnyRef redefined (`aa508d49e`).

### [#544](https://github.com/HerringtonDarkholme/hd-lang/issues/544) Tier 1 T7: recursive types through &; match reads through references

- Recursive enums go through &; match reads through references (`15165b62a`).

### [#546](https://github.com/HerringtonDarkholme/hd-lang/issues/546) examples/dogfood/calc.hd: recursive Expr must go through &

- Put calculator expression trees behind references (`a21c26257`).

### [#406](https://github.com/HerringtonDarkholme/hd-lang/issues/406) Native resolver does not pass address-family hints like Node

- Filter DNS candidates by configured address families (`4167eb490`).

### [#540](https://github.com/HerringtonDarkholme/hd-lang/issues/540) RC3: linear backend module skeleton; hello and integer programs run

- Run hello and integer programs on the linear backend (`d637c6625`).

### [#548](https://github.com/HerringtonDarkholme/hd-lang/issues/548) Cut cargo test wall time: the four slowest test binaries

- Avoid quadratic doc scans and cut test-suite overhead (`d107b1730`).

### [#541](https://github.com/HerringtonDarkholme/hd-lang/issues/541) RC4: memory intrinsics in a privileged std module

- Add std.rt.memory's heap intrinsics for the linear backend (`19060a9b3`).

### [#425](https://github.com/HerringtonDarkholme/hd-lang/issues/425) Run property cases in one instance

- Reuse successful property instances and isolate failure replays (`b67070ea2`).

### [#408](https://github.com/HerringtonDarkholme/hd-lang/issues/408) cpu-count under nested Linux cgroup quotas differs between engines

- Cap Node CPU count by every ancestor cgroup quota (`d16d81faf`).

### [#547](https://github.com/HerringtonDarkholme/hd-lang/issues/547) Tier 1 T8: List and Map as copy-on-write values; snapshot for

- List and Map are copy-on-write values; for iterates a snapshot (`4d70bd230`).

### [#545](https://github.com/HerringtonDarkholme/hd-lang/issues/545) Tier 1: &mut on a let mut enum binding is mutable-upgrade

- An enum in a writable place is writable: &mut and *r = v (`00030df96`).

### [#542](https://github.com/HerringtonDarkholme/hd-lang/issues/542) RC5: allocator in std (size classes, pages, page table, large runs)

- Write the linear backend's heap allocator in std.rt.alloc (`f7ce85694`).

### [#394](https://github.com/HerringtonDarkholme/hd-lang/issues/394) A failed initializer reports a follow-on uninhabited-binding

- Suppress uninhabited-binding after a failed initializer (`374a61ad0`).

### [#395](https://github.com/HerringtonDarkholme/hd-lang/issues/395) An index key of the wrong type reports two errors

- Poison associated types when their trait goal fails (`bcdda65df`).

### [#396](https://github.com/HerringtonDarkholme/hd-lang/issues/396) A missing From import reports a second error at the ? use

- Retain poisoned impl targets after unresolved trait errors (`52e79bf3c`).

### [#550](https://github.com/HerringtonDarkholme/hd-lang/issues/550) Tier 1 T9: element references pin the list; for over &xs and &mut xs

- Element references pin their list; for over &xs and &mut xs (`f5f002abd`).

### [#387](https://github.com/HerringtonDarkholme/hd-lang/issues/387) Two different types with one name print identically in a mismatch

- Qualify colliding type names in mismatch diagnostics (`7f222d879`).

### [#557](https://github.com/HerringtonDarkholme/hd-lang/issues/557) Tier 1: index, call and loop through references to lists

- Index, call and loop through references to lists (`78e97e20c`).

### [#555](https://github.com/HerringtonDarkholme/hd-lang/issues/555) Index with no fitting impl reports unsatisfied-trait-bound, spec says type-mismatch

- Report unmatched index implementations as type mismatch (`d74e0abdf`).

### [#397](https://github.com/HerringtonDarkholme/hd-lang/issues/397) unused-derivation-fact has no fix-it

- Offer a fix to move unused derivation facts into their block (`6da83156c`).

### [#558](https://github.com/HerringtonDarkholme/hd-lang/issues/558) Tier 1 T10: closure captures restated for values, cells and references

- Restate closure captures as copies and cells; fresh cell per loop round (`b5b58d2fb`).

### [#392](https://github.com/HerringtonDarkholme/hd-lang/issues/392) Messages for suspension and defer mistakes

- Explain cold suspension arguments and deferred cleanup (`bf3c5449f`).

### [#391](https://github.com/HerringtonDarkholme/hd-lang/issues/391) Messages for mutable access and integer mixing

- Explain mutable bindings and signedness mismatches (`e6f3f140c`).

### [#549](https://github.com/HerringtonDarkholme/hd-lang/issues/549) Tier 1: value list arguments are not marked, so a callee sees the caller's later changes

- Copy value arguments only where an alias can change the source (`12794dcd4`).

### [#390](https://github.com/HerringtonDarkholme/hd-lang/issues/390) Messages for unsatisfied bounds and trait mismatches

- Show trait arguments and concrete fixes in bound errors (`4c2d09b12`).

### [#388](https://github.com/HerringtonDarkholme/hd-lang/issues/388) Messages for unknown names: suggest the import, the near name or the hd spelling

- Suggest imports and near names for unknown items (`f11a63733`).

### [#560](https://github.com/HerringtonDarkholme/hd-lang/issues/560) Tier 1 T11: hd fix --object-model, the conservative rewrite

- Add hd fix --object-model, the conservative rewrite to references (`f379e7370`).

### [#556](https://github.com/HerringtonDarkholme/hd-lang/issues/556) Tier 1: escaping element references do not keep the list pinned

- Cover escaped element-reference pins until the RC backend (`8c126a422`).

### [#559](https://github.com/HerringtonDarkholme/hd-lang/issues/559) A value list argument sees a later sibling argument's change

- Snapshot value arguments before later siblings change their source (`40a2cc8ed`).

### [#562](https://github.com/HerringtonDarkholme/hd-lang/issues/562) Tier 1: a &mut S value cannot be a requirement provider

- Accept &S and &mut S as requirement providers (`49aa1ae24`).

### [#561](https://github.com/HerringtonDarkholme/hd-lang/issues/561) Tier 1: &mut and & join in list literals; for over a reference to a map

- Join reference permissions and iterate through map references (`53bdcd817`).

### [#349](https://github.com/HerringtonDarkholme/hd-lang/issues/349) Fix-its for the common-mistakes corpus (0 of 72 today)

- Read UTF-8 fix edits and repair existing mut receivers (`fddbc8663`).

### [#563](https://github.com/HerringtonDarkholme/hd-lang/issues/563) Tier 1 T12: std migration 1 (collections, iter, option, result)

- std migration 1: collections and iterators as structs behind &mut (`434c22dbc`).

### [#564](https://github.com/HerringtonDarkholme/hd-lang/issues/564) Tier 1 T13a: std migration 2a (format, hash, cmp, digest, annotation)

- std migration 2a: format, hash, cmp, digest, annotation off data (`f78f306d6`).

### [#565](https://github.com/HerringtonDarkholme/hd-lang/issues/565) Tier 1 T13w: structure protocol (Walker) to &mut self; std walkers off data

- std migration 2w: structure protocol on &mut; walkers are structs (`9cb7540c1`).

### [#566](https://github.com/HerringtonDarkholme/hd-lang/issues/566) Tier 1 T13b: std migration 2b (serde, json, text)

- std migration 2b: serde, json and text on &mut; records are values (`5fc5c5d94`).

### [#551](https://github.com/HerringtonDarkholme/hd-lang/issues/551) RC6: structs, tuples and enums inline on the linear backend

- Lay out structs, tuples, enums and data on the linear backend (`9f454509f`).

### [#567](https://github.com/HerringtonDarkholme/hd-lang/issues/567) Tier 1 T13c: std migration 2c (regex, time, random, error, inspect, cli, ops)

- std migration 2c: regex, time, random, error, cli, ops off data (`0ccfc7e23`).

### [#552](https://github.com/HerringtonDarkholme/hd-lang/issues/552) RC7: strings on the linear backend

- Lower strings on the linear backend (`cc632b7aa`).

### [#570](https://github.com/HerringtonDarkholme/hd-lang/issues/570) Tier 1: trait value references need a written & (no copy from a value)

- Trait value references need a written &; no copy from a value (`05ae12b57`).

### [#568](https://github.com/HerringtonDarkholme/hd-lang/issues/568) Linear backend: key shared generic instances by the backend's layout

- Use backend layouts when sharing generic instances (`95e4a4a00`).

### [#576](https://github.com/HerringtonDarkholme/hd-lang/issues/576) Tier 1 T15a: examples/ on the new object model

- Move examples/ to the tier 1 object model (`e148fe9cd`).

### [#577](https://github.com/HerringtonDarkholme/hd-lang/issues/577) Tier 1 T15b: guide pages and the write-hd skill on the new object model

- Move the guide pages, README and write-hd skill to the tier 1 object model (`13189cf7b`).

### [#571](https://github.com/HerringtonDarkholme/hd-lang/issues/571) Tier 1: host providers for &mut self std traits (Clock, Random) bind as references

- Install host providers of &mut self traits as references (`86c1b3c3e`).

### [#578](https://github.com/HerringtonDarkholme/hd-lang/issues/578) Tier 1 T15c: tour and playground examples on the new object model

- Move the tour and playground examples to the tier 1 object model (`c503cb2d4`).

### [#553](https://github.com/HerringtonDarkholme/hd-lang/issues/553) RC8: object headers and naive reference counting on the linear backend

- Count objects naively on the linear backend (`ace202378`).

### [#554](https://github.com/HerringtonDarkholme/hd-lang/issues/554) RC9: lists on the linear backend

- Lower lists on the linear backend (`f6e03c3dd`).

### [#572](https://github.com/HerringtonDarkholme/hd-lang/issues/572) RC10: maps on the linear backend

- Lower maps on the linear backend (`556aa10d8`).

### [#582](https://github.com/HerringtonDarkholme/hd-lang/issues/582) Tier 1 T14b: std migration 3b (net handles, task, structure, testing, Disposable/IndexSet/Update)

- std migration 3b: net handles, Disposable, IndexSet, Update, testing (`21bab5c8d`).

### [#536](https://github.com/HerringtonDarkholme/hd-lang/issues/536) &mut T to &T does not convert inside List and Option

- Allow readonly container references to weaken nested references (`cb50439bc`).

### [#573](https://github.com/HerringtonDarkholme/hd-lang/issues/573) RC11: closures and dyn on the linear backend

- Lower closures and trait values on the linear backend (`9ab0b1baf`).

### [#574](https://github.com/HerringtonDarkholme/hd-lang/issues/574) RC12: references and interior references on the linear backend

- Lower references, cleanup scopes and pins on the linear backend (`34633c37b`).

### [#535](https://github.com/HerringtonDarkholme/hd-lang/issues/535) Read-only references are shallow, as in Swift

- Make read-only references shallow (`15ab03f5c`).

### [#585](https://github.com/HerringtonDarkholme/hd-lang/issues/585) Integer and float literals take primitive type suffixes (1u32, 1.5f32)

- Support primitive numeric literal suffixes (`c2e5666b2`).

### [#588](https://github.com/HerringtonDarkholme/hd-lang/issues/588) Clock journal test flakes on the reading's size and can hang the wasmtime test binary

- Bound clock journal rows and propagate session test panics (`2963dfda0`).

### [#430](https://github.com/HerringtonDarkholme/hd-lang/issues/430) Growing generic call cycles across files pass hd check

- Check growing generic call cycles across folder modules (`e4cd17803`).

### [#575](https://github.com/HerringtonDarkholme/hd-lang/issues/575) RC13: suspension frames, the waker and the entry driver on the linear backend

- Lower suspension frames, the waker and the entry driver on the linear backend (`4f28ef4fd`).

### [#583](https://github.com/HerringtonDarkholme/hd-lang/issues/583) Tier 1 T16: remove data, permission views, freshness rules and readonly edges

- T16 step 3: remove views, freshness, edges, part copies and migration (`2dc2c0e53`).

### [#584](https://github.com/HerringtonDarkholme/hd-lang/issues/584) Remove copy assignment ...= and embedded part-copy markers

- T16 step 3: remove views, freshness, edges, part copies and migration (`2dc2c0e53`).

### [#586](https://github.com/HerringtonDarkholme/hd-lang/issues/586) Recover a concrete type with downcast_ref and downcast_mut only

- Recover erased values through reference methods (`92b1bd8f6`).
- Keep reference permission out of host handle codecs (`4803010a7`).
- Migrate error examples and tests to reference recovery APIs (`f120b7cbe`).

### [#589](https://github.com/HerringtonDarkholme/hd-lang/issues/589) &mut local passed to a call in an erased trait method body fails V8 validation

- Preserve shared cells across erased method thunks (`b9eccf209`).

### [#590](https://github.com/HerringtonDarkholme/hd-lang/issues/590) Trait method references reject a &dyn or &mut dyn receiver

- Dispatch method references through trait reference receivers (`23d1d6bad`).

### [#592](https://github.com/HerringtonDarkholme/hd-lang/issues/592) Remove bare dyn trait values; trait values are &dyn and &mut dyn only

- Remove bare dyn values and use trait references throughout (`6d2236e53`).
- Share copied provider reference construction (`d2e9bc557`).

### [#587](https://github.com/HerringtonDarkholme/hd-lang/issues/587) Lower references to places as addresses, not get/set closures

- Lower references to places as addresses on the linear backend (`69a00df7c`).

### [#508](https://github.com/HerringtonDarkholme/hd-lang/issues/508) Duration literals with a unit suffix: 500ms, 30s

- Make duration units import-free (`0ef40fb63`).

### [#506](https://github.com/HerringtonDarkholme/hd-lang/issues/506) Derived and transparent errors choose their exit code

- Derive error exit codes and forward transparent errors (`43f5f3f5d`).

### [#597](https://github.com/HerringtonDarkholme/hd-lang/issues/597) Linear backend: #592's reference forms keep closure forms after #587

- Repair the linear backend after #592 and #587 (`9dc86000a`).

### [#595](https://github.com/HerringtonDarkholme/hd-lang/issues/595) RC14: host codecs over linear memory in the Node host

- Lower host codecs and handles on the linear backend (`bf6316c3f`).

### [#593](https://github.com/HerringtonDarkholme/hd-lang/issues/593) deep_eq traps with an illegal cast on wasmtime only

- Normalize reference storage before recursive GC layout (`b915bcbec`).
- Keep handle codec identity separate from reference storage (`146913b53`).

### [#598](https://github.com/HerringtonDarkholme/hd-lang/issues/598) Remove derived and trait exit codes; main chooses its ExitCode through Termination

- Remove Error.exit_code; main picks its ExitCode, CliError.exit gives 2 (`a6958a150`).

### [#491](https://github.com/HerringtonDarkholme/hd-lang/issues/491) std.cli: ArgParser, a mut builder with one Arg record

- Replace Cli with an indexed mutable ArgParser (`cfbc258c8`).

### [#599](https://github.com/HerringtonDarkholme/hd-lang/issues/599) std uses @derive instead of hand-written Debug/Eq/Display impls

- Use derives for equivalent standard-library implementations (`a0f7697d6`).

### [#601](https://github.com/HerringtonDarkholme/hd-lang/issues/601) FsError and Method Eq compare text, so different values are equal

- Compare filesystem errors and HTTP methods structurally (`0a897ffca`).

### [#600](https://github.com/HerringtonDarkholme/hd-lang/issues/600) eprintln writes to standard output on the default engine

- Route eprintln to standard error through the host Console (`a19772127`).

### [#603](https://github.com/HerringtonDarkholme/hd-lang/issues/603) Typed JSON decode builds the whole untyped tree first

- Decode typed JSON directly from validated input (`1e18225e6`).

### [#499](https://github.com/HerringtonDarkholme/hd-lang/issues/499) std.cli: repeated options and choices

- Retain repeated CLI option values and validate choices (`f920df6a8`).

### [#594](https://github.com/HerringtonDarkholme/hd-lang/issues/594) RC16: checked mode for the linear backend

- Add checked mode for the linear backend (`3bf0ebb56`).

### [#500](https://github.com/HerringtonDarkholme/hd-lang/issues/500) std.cli: subcommands in ArgParser

- Add scoped command parsers to std.cli (`1976c66b4`).

### [#501](https://github.com/HerringtonDarkholme/hd-lang/issues/501) std.cli: -h and --help stop parsing with CliError.Help(usage)

- Provide automatic command-scoped CLI help (`f2da0201e`).

### [#610](https://github.com/HerringtonDarkholme/hd-lang/issues/610) Check cache entries depend on build order since #491 (CI red)

- Rewrite derived instances' callee records in place (`4ec9e1480`).

### [#505](https://github.com/HerringtonDarkholme/hd-lang/issues/505) CliError is an Error: Help exits 0 on stdout, other errors 2

- Document CLI error and help exit handling (`44a4de3c5`).

### [#493](https://github.com/HerringtonDarkholme/hd-lang/issues/493) std.collections: chunks and windows return views

- Return borrowed list views from chunks and windows (`e4c5a7adc`).

### [#596](https://github.com/HerringtonDarkholme/hd-lang/issues/596) RC17: count elision 1 on the linear backend

- Elide counts: owned and borrowed parameters, moves (`f3f3b6dbe`).

### [#609](https://github.com/HerringtonDarkholme/hd-lang/issues/609) Every Console program pays for an unused write_error_line host stub (+416 B)

- Link only the host methods a program calls (`eda8a6df7`).

### [#613](https://github.com/HerringtonDarkholme/hd-lang/issues/613) RC: requirement rows and context values on the linear backend

- Lower requirement rows and context values on the linear backend (`e9227bc03`).

### [#518](https://github.com/HerringtonDarkholme/hd-lang/issues/518) hd --repl runs on the Node engine

- Run scripted REPL sessions on Node (`e07526788`).

### [#614](https://github.com/HerringtonDarkholme/hd-lang/issues/614) RC: integer and float ** and float % on the linear backend

- Implement linear integer power and portable float operators (`14bc85c59`).

### [#615](https://github.com/HerringtonDarkholme/hd-lang/issues/615) RC: dynamic calls of generic methods and erased bodies on the linear backend

- Lower erased generic methods on the linear backend (`4ad4412cf`).

### [#616](https://github.com/HerringtonDarkholme/hd-lang/issues/616) RC: facts and handle bodies build once on the linear backend

- Cache linear facts and structure handles once per instance (`461b0a0d1`).

### [#617](https://github.com/HerringtonDarkholme/hd-lang/issues/617) RC: two erased-method shapes still refused on the linear backend

- Run kept erased suspensions and boxed open references on linear (`394057fb8`).

### [#618](https://github.com/HerringtonDarkholme/hd-lang/issues/618) RC: cycle collector (trial deletion) for the linear backend

- Collect reference cycles on the linear backend (`666199387`).

### [#624](https://github.com/HerringtonDarkholme/hd-lang/issues/624) Function types: &mut U to &U variance (known failure)

- Fix reference permission variance in function types (`1f5549bcd`).

### [#625](https://github.com/HerringtonDarkholme/hd-lang/issues/625) RC: iterative release of deep recursive values

- Release recursive RC values through an iterative worklist (`80ee51430`).

### [#626](https://github.com/HerringtonDarkholme/hd-lang/issues/626) Make the RC backend the default everywhere

- Lower function references to intrinsics and trait-value methods on linear (`84095aa9e`).

### [#611](https://github.com/HerringtonDarkholme/hd-lang/issues/611) Iterator is a trait with adapter structs

- Make Iterator a trait with owned adapter values (`9dbbc20a9`).

### [#628](https://github.com/HerringtonDarkholme/hd-lang/issues/628) REPL sessions on the RC backend

- Run REPL sessions on the linear backend by replaying the journal (`dc0bcfdd6`).

### [#619](https://github.com/HerringtonDarkholme/hd-lang/issues/619) RC: references to boxed open values beyond call arguments

- Keep erased reference aliases live beyond generic calls (`cee1c2304`).
- Read and write through references inline, not via flattened helpers (`eb364c6ce`).

### [#627](https://github.com/HerringtonDarkholme/hd-lang/issues/627) Delete the Wasm GC backend

- Remove Wasm GC and backend selection (`dc358a56e`).
- Remove obsolete GC comparisons from the runtime design (`a65afa1b9`).

### [#635](https://github.com/HerringtonDarkholme/hd-lang/issues/635) RC: count_tests page bound fails in checked mode

- Keep page-reuse assertions specific to the normal allocator (`06405ff54`).

### [#629](https://github.com/HerringtonDarkholme/hd-lang/issues/629) RC: pass very wide values in memory (V8 1000-parameter limit)

- Pass wide signatures through a shadow stack (`c3498668d`).

### [#639](https://github.com/HerringtonDarkholme/hd-lang/issues/639) Website build: tour snippets that stopped checking after #611

- Fix the website tour's iterator sum result type (`ba817e351`).

### [#636](https://github.com/HerringtonDarkholme/hd-lang/issues/636) Remove the remaining Wasm GC and --backend mentions

- Remove stale Wasm backend references (`963b63648`).

### [#637](https://github.com/HerringtonDarkholme/hd-lang/issues/637) RC: collect cycles that pass through suspension frames

- Collect reference cycles through idle suspension frames (`e3d0b212d`).

### [#641](https://github.com/HerringtonDarkholme/hd-lang/issues/641) RC: count elision round 2 (field and loop borrows, cancelling pairs)

- Elide counts for immutable reads and loop elements (`544817d01`).

### [#630](https://github.com/HerringtonDarkholme/hd-lang/issues/630) RC: all! polls only the woken children

- Poll only woken all! children on the linear backend (`ca4b2066d`).

### [#642](https://github.com/HerringtonDarkholme/hd-lang/issues/642) RC perf: cycle hooks only for cycle-capable types

- Cycle-collect only cycle-capable types (`615991356`).

### [#643](https://github.com/HerringtonDarkholme/hd-lang/issues/643) RC perf: no page-table lookup when the block start is known

- Integrate known RC block starts with cycle eligibility (`6c8ac494d`).

### [#648](https://github.com/HerringtonDarkholme/hd-lang/issues/648) Bench: add Python references to bench/study

- Add CPython references to the runtime study benchmarks (`d8da4d939`).

### [#647](https://github.com/HerringtonDarkholme/hd-lang/issues/647) Iterable has an associated cursor type (Rust IntoIterator shape)

- Give Iterable an associated cursor type, IntoIter (`261d69ebd`).

### [#649](https://github.com/HerringtonDarkholme/hd-lang/issues/649) RC: link no cycle collector when no type can form a cycle

- Link no cycle collector when no object can take part (`90f369257`).

### [#645](https://github.com/HerringtonDarkholme/hd-lang/issues/645) RC perf: borrow reads through references and self projections

- Borrow parameter projections without extra reference counts (`fce872a45`).

### [#650](https://github.com/HerringtonDarkholme/hd-lang/issues/650) &mut self calls on temporaries are allowed

- Allow mutable method calls on temporary receivers (`a0d862d51`).

### [#654](https://github.com/HerringtonDarkholme/hd-lang/issues/654) Perf: json and interpolation stop allocating per item

- Reuse JSON object buffers and append interpolation parts directly (`3000a4b0f`).

### [#640](https://github.com/HerringtonDarkholme/hd-lang/issues/640) RC: field reads of wide values copy the whole value (locals blow up)

- Keep wide values in shadow storage and read only selected fields (`f50a19134`).

### [#644](https://github.com/HerringtonDarkholme/hd-lang/issues/644) RC perf: inline retain/release fast paths

- Inline the known-start count fast paths on wasmtime (`2aa312b7e`).

### [#660](https://github.com/HerringtonDarkholme/hd-lang/issues/660) Stage fixtures 1: harness, format, tokens/tree/TIR views

- Add stage fixtures: tokens, tree and TIR views of a marked subject (`bdf8c3845`).

### [#661](https://github.com/HerringtonDarkholme/hd-lang/issues/661) Stage fixtures 2: per-instruction source spans kept through opt

- Preserve instruction origins through Wasm optimization (`5c0688ea7`).

### [#665](https://github.com/HerringtonDarkholme/hd-lang/issues/665) Stage fixtures: readable tir view (field names, no flags, no Block lists)

- Make the TIR view readable: field names, no flags, no block lists (`f5c67a30a`).

### [#668](https://github.com/HerringtonDarkholme/hd-lang/issues/668) Known-bad TIR patterns as stage fixtures (IR design evidence)

- Record known bad TIR patterns as focused stage fixtures (`18679c2df`).

### [#663](https://github.com/HerringtonDarkholme/hd-lang/issues/663) Stage fixtures 4: emit every function a case defines

- Emit every function defined by a development fixture (`cd51a8ffd`).

### [#662](https://github.com/HerringtonDarkholme/hd-lang/issues/662) Stage fixtures 3: normalized WAT dump

- Add the stage fixtures' wat view and the first emitter topic (`bdde8e932`).

### [#673](https://github.com/HerringtonDarkholme/hd-lang/issues/673) Stage fixtures: wat view follow-ups (fixture goal, field-named locals, instance names)

- Stage fixtures: wat view on the fixture goal, field and instance names (`c163ee2c0`).

### [#664](https://github.com/HerringtonDarkholme/hd-lang/issues/664) Stage fixtures 5: wat, counts and layout views, first emitter topics

- Add focused layout and count views for stage fixtures (`708b1e011`).

### [#674](https://github.com/HerringtonDarkholme/hd-lang/issues/674) Bench: retune bench/study to ~300 ms per program, report ms

- Retune bench/study to about 300 ms per program, report integer ms (`bdca9abb7`).

### [#675](https://github.com/HerringtonDarkholme/hd-lang/issues/675) Stage fixtures: TypeScript-style cases and generated baselines

- Separate stage source cases from generated baselines (`12ff67a6a`).

### [#682](https://github.com/HerringtonDarkholme/hd-lang/issues/682) RC: arithmetic and constant cleanups (#656 patterns 8, 14, 15)

- RC: native remainder, constant folds, interpolation test (`012dfbc3c`).

### [#699](https://github.com/HerringtonDarkholme/hd-lang/issues/699) Test infra cleanup: one bless variable, simpler stage harness

- Test infra: one HD_BLESS variable, simpler stage harness (`611aba797`).

### [#684](https://github.com/HerringtonDarkholme/hd-lang/issues/684) Convert the #658 a.stage Rust tests to stage cases

- Migrate sampled compiler assertions to stage cases (`1346b5d6e`).

### [#676](https://github.com/HerringtonDarkholme/hd-lang/issues/676) RC: borrow place reads used within one statement or call (#656 pattern 1)

- Borrow local reads for the span of their uses (#656 pattern 1) (`0cb733797`).

### [#680](https://github.com/HerringtonDarkholme/hd-lang/issues/680) RC: for over a returned list does not copy it (#656 pattern 9)

- Iterate owned temporary collections without a snapshot (`fc145debf`).

### [#677](https://github.com/HerringtonDarkholme/hd-lang/issues/677) RC: drop releases of locals that are zero on every path (#656 pattern 2)

- Remove releases proved null across control flow (`e12926be3`).
- Remove null releases and retain a real scope-exit origin check (`9610df3e5`).

### [#685](https://github.com/HerringtonDarkholme/hd-lang/issues/685) hd_syntax: retire parse-gaps (17 duplicates, fold 18 into parse/valid)

- Move parser gap repros into conformance fixtures (`31239d8d0`).

### [#686](https://github.com/HerringtonDarkholme/hd-lang/issues/686) hd_syntax: move tree snapshots and parse-code tests to stage cases

- Migrate parser snapshots to focused stage cases (`2509c82b6`).

### [#701](https://github.com/HerringtonDarkholme/hd-lang/issues/701) RC: &x at x's last access takes the local's count (#678 item b)

- RC: &x at x's last access takes the local's count of its cell (`d85f6e395`).

### [#687](https://github.com/HerringtonDarkholme/hd-lang/issues/687) hd_driver: move run tests to conformance, batch 1 of 8

- Move run test batch one into conformance (`0c1915a96`).

### [#679](https://github.com/HerringtonDarkholme/hd-lang/issues/679) RC: matching an owned temporary moves its payload (#656 pattern 6)

- Move payload ownership out of consuming matches (`aa6581eb7`).

### [#688](https://github.com/HerringtonDarkholme/hd-lang/issues/688) hd_driver: move run tests to conformance, batch 2 of 8

- Move trait run tests into conformance (`940b653f5`).

### [#706](https://github.com/HerringtonDarkholme/hd-lang/issues/706) RC bug: a suspending match guard that returns false leaks one block (checked count-audit)

- Emit guard conditions through normal block handling (`630731c4a`).

### [#689](https://github.com/HerringtonDarkholme/hd-lang/issues/689) hd_driver: move run tests to conformance, batch 3 of 8

- Move facts, numeric and index run tests to conformance (`709039761`).

### [#705](https://github.com/HerringtonDarkholme/hd-lang/issues/705) RC: size-class fast paths for constant-size alloc/free

- Cancel balanced count changes through local tee chains (`89839e060`).

### [#707](https://github.com/HerringtonDarkholme/hd-lang/issues/707) RC: value-only recursion is acyclic; decide closure environments per literal (cycle.rs)

- Keep value-only recursion out of cycle collection (`82867db4f`).

### [#669](https://github.com/HerringtonDarkholme/hd-lang/issues/669) RC perf: json within 1.2x of Wasm GC (strings as interior addresses)

- Skip discarded physical reference reads (`d0568b2c7`).

### [#708](https://github.com/HerringtonDarkholme/hd-lang/issues/708) Conformance runner: an entry-only package-tree fixture silently runs as a cancellation scenario

- Run entry-only package trees as ordinary programs (`bb59b2ddd`).

### [#671](https://github.com/HerringtonDarkholme/hd-lang/issues/671) RC perf: closures within 1.2x of Wasm GC (static drop glue, closure calls)

- Store reference fields without rebuilding their parent (`66aa049d9`).

### [#709](https://github.com/HerringtonDarkholme/hd-lang/issues/709) Stage wat view: every .wat baseline is a valid WAT file

- Print focused stage baselines as valid WAT modules (`f4cf0a5c0`).

### [#710](https://github.com/HerringtonDarkholme/hd-lang/issues/710) RC: hoist list uniqueness and buffer/length loads out of change-free loops (#656 patterns 7, 13)

- Cache stable list projections across clean loops (`c43a7f906`).

### [#712](https://github.com/HerringtonDarkholme/hd-lang/issues/712) std: List.iter steps its index directly, not through a step closure (#656 pattern 5)

- Step list cursors directly without a closure (`feb3456a7`).

### [#690](https://github.com/HerringtonDarkholme/hd-lang/issues/690) hd_driver: move run tests to conformance, batch 4 of 8

- Move driver run cases into conformance, batch four (`113d962cf`).

### [#692](https://github.com/HerringtonDarkholme/hd-lang/issues/692) hd_driver: move run tests to conformance, batch 6 of 8

- Move driver run cases into conformance, batch six (`255c8b327`).

### [#714](https://github.com/HerringtonDarkholme/hd-lang/issues/714) RC: count collected cycles by kind in checked builds, and measure them across the corpus

- Report checked cycle frees by descriptor in the hosts (`293b4f03d`).

### [#716](https://github.com/HerringtonDarkholme/hd-lang/issues/716) Find and delete dead and deprecated code

- Remove stale emitter scaffolding and rollout documentation (`149e8b9ee`).

### [#713](https://github.com/HerringtonDarkholme/hd-lang/issues/713) RC: recursive := local functions call themselves through their own environment, not a cell (no runtime cycle)

- Lower immutable recursive closures through their own environment (`a3c8cdb5f`).

### [#715](https://github.com/HerringtonDarkholme/hd-lang/issues/715) RC: audit async frame/waker links so the runtime makes no owning back-links

- Make enclosing waker targets generation-checked and non-owning (`5bd4cf0d4`).

### [#700](https://github.com/HerringtonDarkholme/hd-lang/issues/700) Minimize the web compiler's size (measure, then cut)

- Compress browser std sources with a static dictionary (`8df6b0545`).

### [#719](https://github.com/HerringtonDarkholme/hd-lang/issues/719) RC bug: copying a list whose nested element is referenced shares the pinned inner list (r[ref.elem.copy])

- Isolate copies of lists with referenced descendants (`344a24057`).

### [#717](https://github.com/HerringtonDarkholme/hd-lang/issues/717) RC: a field read through & borrows for a match span with no calls or writes (local part of #703)

- Borrow reference fields across matches without calls (`3cb30ec93`).

### [#711](https://github.com/HerringtonDarkholme/hd-lang/issues/711) RC: range for tests its bound once; no bounds check for 0..xs.len() over an unshrinkable list (#656 pattern 12)

- Streamline range loops and prove exact list indices (`a509cdebf`).

### [#691](https://github.com/HerringtonDarkholme/hd-lang/issues/691) hd_driver: move run tests to conformance, batch 5 of 8

- Move local and module run tests into conformance (`9046d04f6`).

### [#693](https://github.com/HerringtonDarkholme/hd-lang/issues/693) hd_driver: move run tests to conformance, batch 7 of 8

- Move slicing and suspension run tests into conformance (`e7dcb4e92`).

### [#694](https://github.com/HerringtonDarkholme/hd-lang/issues/694) hd_driver: move run tests to conformance, batch 8 of 8

- Move written-type and function-value tests into conformance (`e343b59c4`).

### [#695](https://github.com/HerringtonDarkholme/hd-lang/issues/695) hd_driver: move diagnostic-code tests to conformance, batch 1 of 4

- Move checker diagnostic tests into conformance (`0a43b66e8`).

### [#696](https://github.com/HerringtonDarkholme/hd-lang/issues/696) hd_driver: move diagnostic-code tests to conformance, batch 2 of 4

- Move inference and ownership checks into conformance (`d759b22d6`).

### [#697](https://github.com/HerringtonDarkholme/hd-lang/issues/697) hd_driver: move diagnostic-code tests to conformance, batch 3 of 4

- Move literal, row and pattern checks into conformance (`1a42cbd90`).

### [#698](https://github.com/HerringtonDarkholme/hd-lang/issues/698) hd_driver: move diagnostic-code tests to conformance, batch 4 of 4

- Move remaining diagnostic-code tests into conformance (`bdb2b54df`).

### [#447](https://github.com/HerringtonDarkholme/hd-lang/issues/447) std.net free helpers: lookup!, connect!, listen!, bind_udp!

- Add free Net helpers that forward to the covering provider (`e906def80`).

### [#456](https://github.com/HerringtonDarkholme/hd-lang/issues/456) hd_run! takes a Command; RunOutput is removed

- Run integration programs from Command and return ProcessOutput (`aa846249a`).

### [#445](https://github.com/HerringtonDarkholme/hd-lang/issues/445) Request builder for std.http

- Build HTTP requests with fluent setters and redirect limits (`9967329e9`).

### [#448](https://github.com/HerringtonDarkholme/hd-lang/issues/448) Test providers share new, reply and received

- Rename ScriptedNet request inspection to received (`c76cff00e`).

### [#480](https://github.com/HerringtonDarkholme/hd-lang/issues/480) std.regex: Regex.decode and Captures.decode through Deserialize

- Decode named regex captures through Deserialize (`f8de0db7d`).

### [#718](https://github.com/HerringtonDarkholme/hd-lang/issues/718) TIR v2 stage 1: places as the single form (docs/compiler/tir-v2.md)

- TIR v2 stage 1: delete the forms places replaced (`b4c5768e3`).

### [#475](https://github.com/HerringtonDarkholme/hd-lang/issues/475) std.num: remove the six parse_ functions

- Remove free number parsers in favor of typed Parse calls (`9e3ab2a45`).

### [#487](https://github.com/HerringtonDarkholme/hd-lang/issues/487) std.error: remove the free chain; ContextError becomes private

- Make error context private and keep chain as a method (`2f6664fa9`).

### [#602](https://github.com/HerringtonDarkholme/hd-lang/issues/602) Remove the --cap hint from std's NotGranted messages

- Remove host grant advice from std error messages (`22fb19fe7`).

### [#727](https://github.com/HerringtonDarkholme/hd-lang/issues/727) Flaky hang: session_tests::a_stop_during_a_fifo_read_cancels_the_input

- Bound and synchronize the FIFO cancellation regression (`54c1ea088`).

### [#378](https://github.com/HerringtonDarkholme/hd-lang/issues/378) MemoryFs keys ./x and x as different files

- Normalize MemoryFs keys across file operations (`6025a44cd`).

### [#458](https://github.com/HerringtonDarkholme/hd-lang/issues/458) HTTP: send! status-neutral; only a free get!(url) that fails on 4xx/5xx

- Retain checked HTTP responses and remove free send (`dbae658a3`).

### [#495](https://github.com/HerringtonDarkholme/hd-lang/issues/495) std.collections: FromIterator for Deque and Heap

- Collect owned iterators into deques and heaps (`71804b526`).

### [#469](https://github.com/HerringtonDarkholme/hd-lang/issues/469) ScriptedHttp::new, reply and received

- Queue scripted HTTP replies and record received requests (`f1c902d2a`).

### [#725](https://github.com/HerringtonDarkholme/hd-lang/issues/725) Browser HTTP provider ignores Request.redirects(0)

- Reject disabled redirects in the browser host (`bdd31fcee`).

### [#467](https://github.com/HerringtonDarkholme/hd-lang/issues/467) std.http Client with base URL and default headers

- Add an HTTP client with base URL and default headers (`12cd6e82e`).

### [#490](https://github.com/HerringtonDarkholme/hd-lang/issues/490) std.iter: Iterable for Option; Iterator.flat_map takes any Iterable

- Flatten iterable results with owned inner cursors (`8e837e2c9`).

### [#488](https://github.com/HerringtonDarkholme/hd-lang/issues/488) std.task: one retry! takes its Backoff policy as data

- Unify retries around a defaulted Backoff policy (`0fa716330`).

### [#496](https://github.com/HerringtonDarkholme/hd-lang/issues/496) std.collections: List.flat_map takes any Iterable result

- Let List.flat_map collect any Iterable result (`d75974e25`).

### [#459](https://github.com/HerringtonDarkholme/hd-lang/issues/459) std.host absorbs std.sys; EnvError and SysError become one HostError

- Move system reads into std.host with a shared HostError (`5858019ab`).

### [#457](https://github.com/HerringtonDarkholme/hd-lang/issues/457) Request.json and Request.form encode typed bodies

- Add typed JSON and form request bodies (`ebca52379`).

### [#439](https://github.com/HerringtonDarkholme/hd-lang/issues/439) Prelude: Poll, PollContext, Waker and Hasher leave; assert joins the test prelude

- Separate implementer names from the test assertion prelude (`00193ede0`).

### [#755](https://github.com/HerringtonDarkholme/hd-lang/issues/755) std.task: timeout! keeps the work's error (From[TimedOut]); with_timeout! for any work

- Add flat Result deadlines with with_timeout! (`03c1cc12e`).

### [#451](https://github.com/HerringtonDarkholme/hd-lang/issues/451) std.fs: Entry.modified

- Report filesystem modification times on Entry (`efcc41d34`).

### [#450](https://github.com/HerringtonDarkholme/hd-lang/issues/450) std.fs: FsWrite.copy! and a free copy!

- Copy files through the filesystem host (`e13c30d73`).

### [#437](https://github.com/HerringtonDarkholme/hd-lang/issues/437) Structure sources end with Key?; positions are usize

- End Structure sources with optional member keys (`13ce7c4d5`).

### [#455](https://github.com/HerringtonDarkholme/hd-lang/issues/455) Process output: explicit ok() and a checked free run!

- Add explicit checks for process output (`f136d763e`).

### [#432](https://github.com/HerringtonDarkholme/hd-lang/issues/432) Console keeps only write!; println, eprintln and print are std functions

- Make console writes raw and move line endings into std (`f1acaa60d`).

### [#362](https://github.com/HerringtonDarkholme/hd-lang/issues/362) Smaller stdlib fixes (14-22, 24, 25 of the impl review)

- Split the remaining stdlib performance follow-ups (`671785a92`).

### [#763](https://github.com/HerringtonDarkholme/hd-lang/issues/763) Escape Debug strings by copying unchanged runs

- Copy ordinary Debug string text in runs (`9be335e6e`).

### [#768](https://github.com/HerringtonDarkholme/hd-lang/issues/768) Build regex replacement expansions without prefix copying

- Join regex replacement fragments once (`e2432c168`).

### [#764](https://github.com/HerringtonDarkholme/hd-lang/issues/764) Encode and decode hex without intermediate text conversions

- Convert hex with direct bytes and exact output buffers (`234e92b27`).

### [#766](https://github.com/HerringtonDarkholme/hd-lang/issues/766) Skip the variant-index probe when comparing structs

- Probe enum variant indices only when they differ (`1a93d1283`).

### [#767](https://github.com/HerringtonDarkholme/hd-lang/issues/767) Append to MemoryFs files without copying their full contents

- Reuse stored MemoryFs buffers when appending (`2c3493947`).

### [#761](https://github.com/HerringtonDarkholme/hd-lang/issues/761) Store Heap elements directly and sift with a hole

- Store Heap elements directly and sift through a hole (`b804667de`).

### [#762](https://github.com/HerringtonDarkholme/hd-lang/issues/762) Reuse a scratch buffer during stable list sorting

- Reuse buffers across stable list sort merge passes (`268e2066e`).

### [#479](https://github.com/HerringtonDarkholme/hd-lang/issues/479) std.http: cookie jar, redirects followed by the client, Client.cookie

- Keep client cookies and follow HTTP redirects in std (`8d8ac4a05`).

### [#772](https://github.com/HerringtonDarkholme/hd-lang/issues/772) Rename serialized members with a field fact

- Honor serde member rename facts across std formats (`3fa4340ee`).

### [#775](https://github.com/HerringtonDarkholme/hd-lang/issues/775) Omit absent optional serde members with skip_none

- Omit absent optional serde members with skip_none (`c46409049`).

### [#773](https://github.com/HerringtonDarkholme/hd-lang/issues/773) Skip serde members while retaining their derive bounds

- Compose serde skip with rename and skip_none (`49b119e1f`).

### [#774](https://github.com/HerringtonDarkholme/hd-lang/issues/774) Apply serde rename_all styles to snake_case member names

- Centralize serde naming and add rename_all styles (`a23e536c1`).

### [#777](https://github.com/HerringtonDarkholme/hd-lang/issues/777) Read typed CLI records through a Cli trait and derive

- Add typed CLI records and scalar readers (`55d7d9ca2`).

### [#724](https://github.com/HerringtonDarkholme/hd-lang/issues/724) Report cold-process and warm-module benchmark times separately

- Distinguish fresh-process and warm-module benchmark times (`448b6a0d1`).

### [#778](https://github.com/HerringtonDarkholme/hd-lang/issues/778) Derive CLI value sets from payload-free enums

- Derive typed CLI choices from payload-free enums (`6278705a6`).

### [#780](https://github.com/HerringtonDarkholme/hd-lang/issues/780) Configure typed CLI fields with a Short fact

- Configure typed CLI short options through a field fact (`081fadbb8`).

### [#634](https://github.com/HerringtonDarkholme/hd-lang/issues/634) Bench: &mut dyn Iterator vs concrete adapter chains on RC

- Measure iterator erasure cost and allocations on RC (`99568a488`).

### [#781](https://github.com/HerringtonDarkholme/hd-lang/issues/781) Configure typed CLI positionals with a field fact

- Read typed CLI positionals and final rest lists (`84ea3e0f7`).

### [#779](https://github.com/HerringtonDarkholme/hd-lang/issues/779) Derive CLI subcommands from enums with payloads

- Derive scoped CLI subcommands from payload enums (`70f94b146`).

### [#782](https://github.com/HerringtonDarkholme/hd-lang/issues/782) Use type and member documentation in typed CLI help

- Build typed CLI help from declaration documentation (`636b5196d`).

### [#606](https://github.com/HerringtonDarkholme/hd-lang/issues/606) Drop impl Error for string; message(text) makes an error from text

- Require explicit text errors in entries and test bodies (`67bf08140`).

### [#607](https://github.com/HerringtonDarkholme/hd-lang/issues/607) One builder style: &mut builders across std's configuration types

- Document independent copies of mutable configuration builders (`7aab1a02c`).

### [#605](https://github.com/HerringtonDarkholme/hd-lang/issues/605) Hasher gets typed write_* methods so map keys stop allocating

- Hash scalar map-key bytes through typed Hasher methods (`e0ccdd01f`).

### [#377](https://github.com/HerringtonDarkholme/hd-lang/issues/377) Derived build allocates member metadata on every call

- Cache derived metadata and tuple handles per concrete target (`db25a6415`).

### [#498](https://github.com/HerringtonDarkholme/hd-lang/issues/498) std.host: decode a typed environment through Deserialize

- Decode typed environment records through Deserialize (`8d49253b7`).

### [#791](https://github.com/HerringtonDarkholme/hd-lang/issues/791) Remove Map.get_or

- Remove Map.get_or in favor of get and unwrap_or (`139ececce`).

### [#492](https://github.com/HerringtonDarkholme/hd-lang/issues/492) std.error: rename cause to source (method, report field, printed line)

- Rename Error.cause to source throughout the error API (`680fbe113`).

### [#789](https://github.com/HerringtonDarkholme/hd-lang/issues/789) Stress tests: number text, radix, encodings and SHA-256

- Stress number text, encodings and SHA-256 against reference models (`8b6ae7144`).

### [#790](https://github.com/HerringtonDarkholme/hd-lang/issues/790) Stress tests: std.time and std.path edge cases

- Stress time and path behavior against independent models (`774f1f742`).

### [#786](https://github.com/HerringtonDarkholme/hd-lang/issues/786) Stress tests: std.text properties and adversarial UTF-8

- Stress text operations with independent UTF-8 models (`7b0a6336d`).

### [#785](https://github.com/HerringtonDarkholme/hd-lang/issues/785) Stress tests: collections against reference models

- Stress collections against independent List models (`6da8691c6`).

### [#788](https://github.com/HerringtonDarkholme/hd-lang/issues/788) Stress tests: regex against literal search and backtracking inputs

- Stress regex search and captures against independent models (`f4533e081`).

### [#787](https://github.com/HerringtonDarkholme/hd-lang/issues/787) Stress tests: JSON and serde round trips and malformed input

- Stress JSON and serde round trips and malformed inputs (`74cb49f2e`).

### [#746](https://github.com/HerringtonDarkholme/hd-lang/issues/746) hd_mono: strip_newtype stops allocating a HashSet per call

- Strip repeated newtype wrappers without allocating a cycle set (`be659e7a0`).

### [#753](https://github.com/HerringtonDarkholme/hd-lang/issues/753) hd_intern: Interner stores each string once

- Store interned strings once in an owned arena (`7cda0f089`).

### [#737](https://github.com/HerringtonDarkholme/hd-lang/issues/737) hd_intern: PathTable lookup without allocating, and fill def_module

- Intern stable paths with compact symbol keys (`8aec521fb`).

### [#749](https://github.com/HerringtonDarkholme/hd-lang/issues/749) hd_driver: Run::lookup hands out cache sections without copying

- Share loaded cache entry sections without payload copies (`b4894fa6a`).
- Derive cached payload ranges from decoder slices (`de50c286d`).

### [#738](https://github.com/HerringtonDarkholme/hd-lang/issues/738) Resolver: Lower::sig stops deep-cloning Gen per signature

- Reuse generic scopes across signature lowering (`c5879dc07`).

### [#742](https://github.com/HerringtonDarkholme/hd-lang/issues/742) Types: InferTable::shallow returns global types without decoding

- Skip global type decoding in shallow inference lookup (`0cc697d5f`).

### [#743](https://github.com/HerringtonDarkholme/hd-lang/issues/743) Types: tag-only accessors so hot callers stop building an owned TyData

- Read type heads without decoding owned payloads (`8e7d987cb`).

### [#744](https://github.com/HerringtonDarkholme/hd-lang/issues/744) Types: TableWriter writes flat words, not one Vec or String per row

- Flatten type wire table row storage (`242514f38`).

### [#740](https://github.com/HerringtonDarkholme/hd-lang/issues/740) Resolver: one dense DefId table instead of searching each folder's by_def

- Index interface items once per compiler run (`e978bcc34`).

### [#739](https://github.com/HerringtonDarkholme/hd-lang/issues/739) Resolver: key tables by (ModuleId, Symbol) instead of strings

- Use module identities for resolver lookup keys (`f64b9a814`).

### [#748](https://github.com/HerringtonDarkholme/hd-lang/issues/748) hd_driver: ProgramTables.items as a dense table indexed by DefId

- Store program items in a dense declaration table (`ef5391fd5`).

### [#751](https://github.com/HerringtonDarkholme/hd-lang/issues/751) hd_cache: fewer files per build (one file per entry costs 8-15% of a build)

- Measure loose and packed cache stores before changing layout (`84db1696a`).

### [#770](https://github.com/HerringtonDarkholme/hd-lang/issues/770) Flaky: net_parity tests fail under load (Node handle errors, timeouts)

- Keep timed-out socket probes on tracked Tokio I/O (`b590a8b2d`).
- Observe queued socket readiness before zero-timeout probes (`6bd314ea3`).
- Synchronize queued accept parity through a control stream (`3cddf4a86`).
- Let zero-bound socket probes observe queued Node I/O (`e068042c7`).
- Run the queued Node accept regression in host CI (`b84809fff`).
- Keep UDP fixtures open until their explicit wake-up (`de1facb03`).
- Probe queued UDP input independently of cached readiness (`fd34a283d`).

### [#757](https://github.com/HerringtonDarkholme/hd-lang/issues/757) Node host: let a composed timeout cancel a pending connect

- Let Node connect suspend and cancel its pending socket (`0c75c9ce3`).

### [#792](https://github.com/HerringtonDarkholme/hd-lang/issues/792) Build cache: per-run packs with a shared index (layout A from #751)

- Pack build cache entries behind an atomic shared index (`b7b9cd832`).
- Test packed cache entries through the shared index (`554a63054`).

### [#446](https://github.com/HerringtonDarkholme/hd-lang/issues/446) std.net: remove connect_timeout!; timeout! around connect! replaces it

- Remove bounded connect in favor of composed deadlines (`884ef88de`).

### [#726](https://github.com/HerringtonDarkholme/hd-lang/issues/726) Node host: asynchronous HTTP so timeout! can cancel a send

- Let Node HTTP sends suspend and cancel their requests (`08f0eacf9`).

### [#758](https://github.com/HerringtonDarkholme/hd-lang/issues/758) Flaky: pipeline_tests::a_test_run_and_its_executables_build_for_its_engine

- Keep unread REPL skims out of the shared std pack (`8a9db83c2`).

### [#604](https://github.com/HerringtonDarkholme/hd-lang/issues/604) Wrapping arithmetic and integer MIN/MAX as intrinsics

- Lower integer wrapping arithmetic and bounds as intrinsics (`f6285c4c0`).

### [#760](https://github.com/HerringtonDarkholme/hd-lang/issues/760) Lower integer wrapping and bit counts to Wasm instructions

- Replace integer bit loops with native count intrinsics (`55568272c`).

### [#400](https://github.com/HerringtonDarkholme/hd-lang/issues/400) Node host: a link swapped between the grant check and the open escapes

- Say which engine holds file grants against a link swap (`f3d492785`).

### [#735](https://github.com/HerringtonDarkholme/hd-lang/issues/735) Positions: line lookups use a line table instead of scanning from the file start

- Cache source positions from line tables and retained build data (`a89267ca2`).

### [#736](https://github.com/HerringtonDarkholme/hd-lang/issues/736) hd_base: replace the fixed-seed SipHash with a fast seeded hasher

- Use a fast seeded hasher for compiler tables (`5caaf2e51`).

### [#776](https://github.com/HerringtonDarkholme/hd-lang/issues/776) Derive drops a generic parameter's other bounds: Serialize on [T < Serialize & Deserialize] fails

- Keep a generic type's declared parameter bounds in its derived impls (`a060004d2`).

### [#44](https://github.com/HerringtonDarkholme/hd-lang/issues/44) Let a derivation source declare that it fills declared member defaults

- Move the Default derive's declared-default exemption onto a source fact (`1949d9a90`).

### [#771](https://github.com/HerringtonDarkholme/hd-lang/issues/771) Derive: a skipped member is exempt from the derived trait's bound

- Exempt serde-skipped members from the derive bounds through a skips fact (`04931aa7d`).

### [#407](https://github.com/HerringtonDarkholme/hd-lang/issues/407) Windows: a process started before job assignment survives an abort

- Assign Windows children to a Job before resuming them (`70eabb82e`).
- Assign Windows children to their Job before resuming (`74146d30e`).

### [#183](https://github.com/HerringtonDarkholme/hd-lang/issues/183) Spec audit pass: apply or reject the spec rows in diagnostic-notes

- Close the #183 spec audit pass: drop finished diagnostic-notes rows (`07a545efd`).
- Reject a Hash derivation block, as trait.derive.cmp-every-member says (`edba46c32`).

### [#185](https://github.com/HerringtonDarkholme/hd-lang/issues/185) Stateful property testing in std.testing

- Add model-driven command histories to property tests (`62451a8a5`).

### [#798](https://github.com/HerringtonDarkholme/hd-lang/issues/798) TIR v2: make the new ownership path the default

- TIR v2 stage 2: the new ownership path is the default (commit 13c) (`6a396a587`).

### [#722](https://github.com/HerringtonDarkholme/hd-lang/issues/722) Avoid List header count churn in borrowed scalar field stores

- TIR v2 stage 2: the new ownership path is the default (commit 13c) (`6a396a587`).

### [#799](https://github.com/HerringtonDarkholme/hd-lang/issues/799) TIR v2: delete live.rs and count.rs's count decisions

- TIR v2 stage 2: delete live.rs and count.rs's count decisions (commit 14) (`67e3cac23`).

### [#494](https://github.com/HerringtonDarkholme/hd-lang/issues/494) Collections: elements are places; &mut on an element is call-scoped (decision E)

- Align the tier 1 study with call-scoped element access (`c6b79dc48`).
- Check conflicting-access for element references (`b949125b7`).

### [#800](https://github.com/HerringtonDarkholme/hd-lang/issues/800) TIR v2: delete the old path's loans and list caches

- TIR v2 stage 2: delete the old path's uniqueness and cache code (commit 14, second part) (`a6ea8819b`).

### [#801](https://github.com/HerringtonDarkholme/hd-lang/issues/801) TIR v2: delete the checker's old ownership mode

- TIR v2 stage 2: delete the checker's old ownership mode (commit 14, third part) (`62b833aa5`).

### [#784](https://github.com/HerringtonDarkholme/hd-lang/issues/784) Derived Deserialize cannot build a zero-field struct

- Give a field-less value its locals when a block's tail flows into it (`fe26ca77a`).

### [#729](https://github.com/HerringtonDarkholme/hd-lang/issues/729) Backend: a 4-byte SymId instead of a 272-byte Sym in Ins, funcs and the call graph

- Intern backend symbols and compact copyable instructions (`b1977bd09`).

### [#747](https://github.com/HerringtonDarkholme/hd-lang/issues/747) hd_mono: per-instance inst_ty side table instead of subst per instruction

- Cache instance instruction types before Wasm emission (`06ac831c7`).

### [#745](https://github.com/HerringtonDarkholme/hd-lang/issues/745) TIR wire: write_body pushes into one Vec instead of one Vec per instruction

- Collect TIR wire IDs in one body buffer (`e44e63808`).

### [#731](https://github.com/HerringtonDarkholme/hd-lang/issues/731) Backend: zero_releases keeps facts at block boundaries, not one Vec per instruction

- Store zero-release facts at block boundaries (`1fd4877b5`).

### [#651](https://github.com/HerringtonDarkholme/hd-lang/issues/651) hd lint: a separate lint crate that owns the compiler's warnings

- Propose hd lint and its first temporary-receiver warning (`5cdef5f66`).
- Expand the lint design with checked facts and stress cases (`8c5aada08`).
- Design a separate crate for hd lint warnings (`6c0dc2a31`).

### [#750](https://github.com/HerringtonDarkholme/hd-lang/issues/750) Checker: one MethodIndex per folder interface, not per module

- Share method indexes for identical interface closures (`e8a0d7bc8`).

### [#51](https://github.com/HerringtonDarkholme/hd-lang/issues/51) Design the host embedding API (registration, wire format, panic status)

- Propose explicit host registration and embedding adapters (`404f1eed0`).
- Bind package-declared host traits through generated JS glue (`c9b94cb48`).
- Bind package host providers through manifest consent (`792f3101c`).
- Merge current main for package-host CI review (`3b7640bd9`).

### [#526](https://github.com/HerringtonDarkholme/hd-lang/issues/526) Tree-shake cancellation-only runtime code from programs that cannot cancel

- Prune unreachable cancellation paths from linear Wasm (`d0c0d6bf2`).

### [#28](https://github.com/HerringtonDarkholme/hd-lang/issues/28) Design an hd subcommand for releases and SemVer

- Propose release and SemVer tooling for packages (`d4b85c0be`).
- Revise release design with typed compatibility rules (`ad3edc0f2`).
- Stress-test release API rules and local tag transactions (`9540e723a`).

### [#182](https://github.com/HerringtonDarkholme/hd-lang/issues/182) Module hot reload

- Propose module hot reload with retained active bodies (`7ef655b95`).

### [#646](https://github.com/HerringtonDarkholme/hd-lang/issues/646) REPL: compact the journal into a checkpoint module when it fills

- Propose measured REPL journal checkpoints (`3955c59b5`).
- Checkpoint REPL sessions with initialized linked modules (`c59783d76`).

### [#181](https://github.com/HerringtonDarkholme/hd-lang/issues/181) Program database instead of a language server

- Propose a snapshot-based program database (`6c007975b`).

### [#18](https://github.com/HerringtonDarkholme/hd-lang/issues/18) Design resume (durable execution) on the REPL journal

- Propose durable resume over the execution journal (`1e33ff388`).
- Explore durable execution recovery boundaries (`2e89fcb0b`).
- Plan private whole-run recording and closed replay (`48548a2f5`).

### [#184](https://github.com/HerringtonDarkholme/hd-lang/issues/184) hd bench: benchmarks with a host clock and their own registration

- Propose explicit benchmark suites and batched timing (`dfe4e2a6f`).
- Expand benchmark design with statistics and paired comparisons (`bb05a850b`).

### [#783](https://github.com/HerringtonDarkholme/hd-lang/issues/783) Design: composable time and failure policies (shape 1: policy values + one block, or shape 2: one API for block and combinator)

- Compare policy values with reusable bang-policy adapters (`41e072bb9`).
- Preserve function rows in associated type bindings (`42b11acc3`).
- Rewrite policy composition around lean values and one apply block (`25e83a9cd`).

### [#265](https://github.com/HerringtonDarkholme/hd-lang/issues/265) std.net: TLS over a stream

- Explore TLS stream boundaries and transfer ownership (`938c828e0`).

### [#489](https://github.com/HerringtonDarkholme/hd-lang/issues/489) std.task: remove all_list!; map_all! uses the flat engine

- Drive map_all workers through a flat readiness tree (`621482264`).

### [#266](https://github.com/HerringtonDarkholme/hd-lang/issues/266) HTTP server: an hd request handler deployable to Vercel and Cloudflare (Node / V8 + Wasm)

- Design portable HTTP request handlers and hosting adapters (`ab29bd794`).
- Prepare Vercel Node deployment through the server SDK (`b79af4d49`).
- Reject lifecycle timer budgets above Node’s supported maximum (`9ae65337d`).

### [#741](https://github.com/HerringtonDarkholme/hd-lang/issues/741) Checker and resolver: one Symbol per identifier token, interned once per file

- Cache identifier symbols in parsed token columns (`f4ac87f15`).

### [#515](https://github.com/HerringtonDarkholme/hd-lang/issues/515) dbg renders through std's derivation instead of a compiler renderer

- Render dbg values through private std Structure printers (`7212f9e27`).

### [#806](https://github.com/HerringtonDarkholme/hd-lang/issues/806) Design: a server application model (lifecycle, handlers, state, hosting)

- Design the server application model with executable controls (`d1903e60f`).

### [#810](https://github.com/HerringtonDarkholme/hd-lang/issues/810) Node host: TcpStream.read, accept and receive must return Pending instead of finishing synchronously

- Make Node socket waits cooperative and cancellation safe (`36e94171f`).

### [#811](https://github.com/HerringtonDarkholme/hd-lang/issues/811) Checker: a struct's mut self impl of a trait method fails trait-method-signature

- Match struct mut self trait receivers as value copies (`8652a0627`).

### [#612](https://github.com/HerringtonDarkholme/hd-lang/issues/612) impl Trait 1: whole return type of free, inherent and nested functions

- Support opaque impl Trait results for named functions (`aafe3fe75`).

### [#818](https://github.com/HerringtonDarkholme/hd-lang/issues/818) Pipe steps without _: a standalone expression the pipe calls (evaluate and stress-test)

- Stress-test standalone pipe steps and policy builders (`692158c1e`).

### [#807](https://github.com/HerringtonDarkholme/hd-lang/issues/807) Design: a GUI application model (may live outside the core language)

- Explore GUI models and an external browser framework (`5d8c1d92d`).

### [#804](https://github.com/HerringtonDarkholme/hd-lang/issues/804) Design: inline union types (A | B), a sum written like a tuple

- Stress-test positional union errors and explicit constructors (`bcb7e3b9e`).

### [#812](https://github.com/HerringtonDarkholme/hd-lang/issues/812) Node host: Process.run! must return Pending instead of spawnSync

- Keep Node Process.run pending while a child runs (`909442f8c`).

### [#821](https://github.com/HerringtonDarkholme/hd-lang/issues/821) Conformance: HD_CONFORMANCE_ONLY exits 0 when a selected case fails

- Fail filtered conformance runs on unexpected failures (`c54b95d34`).

### [#814](https://github.com/HerringtonDarkholme/hd-lang/issues/814) Node host: Net lookup, listen, bind_udp, write and send_to must return Pending

- Keep remaining Node Net I/O calls pending while waiting (`5519d81d2`).

### [#820](https://github.com/HerringtonDarkholme/hd-lang/issues/820) all!: a wake during a sibling's block_on is lost; zero sleep plus println deadlocks

- Rewake the outer driver after a block_on takes host completions (`0c906f422`).

### [#815](https://github.com/HerringtonDarkholme/hd-lang/issues/815) Node host: filesystem calls on special files must return Pending

- Keep Node special-file I/O pending while peers wait (`319c6a03f`).

### [#813](https://github.com/HerringtonDarkholme/hd-lang/issues/813) Node host: console read_line! and read_all! must return Pending

- Keep Node console input pending while stdin is empty (`195c0f499`).

### [#816](https://github.com/HerringtonDarkholme/hd-lang/issues/816) Node host: console output and session protocol writes must not block on a full pipe

- Keep both hosts polling while output pipes are full (`70b77665d`).
- Discard unwritten console output when pending writes are aborted (`fb0fff3a8`).

### [#832](https://github.com/HerringtonDarkholme/hd-lang/issues/832) lowering-catalog.md still shows Wasm GC lowering

- Replace stale lowering catalog with linear-memory examples (`371284a90`).

### [#829](https://github.com/HerringtonDarkholme/hd-lang/issues/829) Node host: worker call can read an empty port (flaky net-pending-write)

- Correlate Node worker replies and tolerate early wakeups (`021d9ab33`).

### [#827](https://github.com/HerringtonDarkholme/hd-lang/issues/827) Require hd.toml [capabilities] to list every capability main uses

- Require declared capabilities for package main functions (`b5d730b89`).
- Cover latent package mains in required capability checks (`ca170d8a6`).
- Separate development capability grants for tests and tasks (`b60d8dae7`).
- Collect task initializer capabilities alongside main (`d8f53bad2`).
- List Process in the executable pipeline test fixture (`c156d2ac2`).
- Apply development grants to the root task module (`16fd4789b`).

### [#834](https://github.com/HerringtonDarkholme/hd-lang/issues/834) Investigate local references (first cut of #704): use cases, edge cases, prior art, stress test

- Fold local references into Object Model, Tier 2 as Part 3 (#834) (`648582698`).

### [#822](https://github.com/HerringtonDarkholme/hd-lang/issues/822) Pipe styles: stress-test Elixir, F#, Hack and placeholder pipes for shell-style use

- Study four pipe styles with inference and runtime evidence (`a4172f0f5`).

### [#825](https://github.com/HerringtonDarkholme/hd-lang/issues/825) TIR verifier: Balance's loop back-edge check never fires (loop_of is always NONE)

- TIR topology: a child block's parent is its owner, so loop_of finds loops (`1199a3ec2`).

### [#702](https://github.com/HerringtonDarkholme/hd-lang/issues/702) RC: non-escaping &mut locals live on the shadow stack, not in a counted cell (#678 item c)

- RC: a local only local references lend lives in the frame, not a cell (`b013017fe`).

### [#831](https://github.com/HerringtonDarkholme/hd-lang/issues/831) Daily goal metrics on GitHub Actions, stored on a metrics-data branch

- Add daily goal metrics and retain run history (`dd691b5a3`).
- Move the metrics' hd programs into checked fixtures (`c485beba3`).

### [#723](https://github.com/HerringtonDarkholme/hd-lang/issues/723) Avoid per-element capacity work in fixed-count List construction

- Backend: inline fast path for List push (`07170b987`).

### [#819](https://github.com/HerringtonDarkholme/hd-lang/issues/819) Node host on Windows: aborting Process.run! kills descendants (Job object)

- Own Windows Node process trees before the child runs (`7e790c160`).
- Finish Windows replies before inherited protocol pipes close (`5537cd854`).

### [#823](https://github.com/HerringtonDarkholme/hd-lang/issues/823) CLI: hd install puts a package's executables on PATH

- Install package executables as managed PATH commands (`14df4aae2`).

### [#428](https://github.com/HerringtonDarkholme/hd-lang/issues/428) SIMD byte-scan primitives for std text and JSON

- Multi-byte searches skip to first/last byte pairs 16 offsets at a time (`7eca37660`).

### [#826](https://github.com/HerringtonDarkholme/hd-lang/issues/826) Study: embedded V8 as the CLI's Wasm engine (moonrun prior art)

- Study embedded V8 costs and portable host reuse (`75e4c5fc9`).

### [#631](https://github.com/HerringtonDarkholme/hd-lang/issues/631) impl Trait 2: nested in return types

- Support opaque occurrences nested in ordinary return types (`1461d5be4`).

### [#844](https://github.com/HerringtonDarkholme/hd-lang/issues/844) Runtime benchmark: retain the 20M map-count control

- Add a longer map-count runtime benchmark (`e69490749`).

### [#759](https://github.com/HerringtonDarkholme/hd-lang/issues/759) Convert UTF-8 bytes without per-byte std/runtime calls

- to_utf8 copies the string's bytes once (`b3df2f49b`).

### [#845](https://github.com/HerringtonDarkholme/hd-lang/issues/845) Implement policy composition: Layer, tuples and one apply! block (POLICY_COMPOSITION.md phasing)

- Document policy composition and add measured cost guardrails (`1f6a58518`).

### [#847](https://github.com/HerringtonDarkholme/hd-lang/issues/847) Design note: std.http Request and Response usable in both directions (decode requests, build responses)

- Design symmetric HTTP messages and internal wire adapters (`c295ea6a4`).
- Revise HTTP message design for cookies and typed form queries (`f151e8d8f`).
- Document visited form shapes and cover empty records (`51d48ea12`).
- Add HTTP message core types and cookie/URL handling (`5ee6c5096`).
- Refresh server SDK schema after HTTP core additions (`2d2840b27`).
- Add read-once HTTP bodies and typed query/form conversion (`5ae6347ce`).
- Regenerate server SDK schema for HTTP body/query slice (`fb2a97f0e`).
- Bridge HTTP messages through internal wire providers (`d5a28f88e`).
- Refresh server SDK schema for HTTP wire adapters (`59b8b43a5`).
- Exercise reviewed HTTP examples against std (`b451c9fdd`).

### [#843](https://github.com/HerringtonDarkholme/hd-lang/issues/843) Top-level code that uses a provider fails to compile (rt:entry_init, Link)

- Fix script initializer providers and require pure entry init (`24b04a683`).

### [#846](https://github.com/HerringtonDarkholme/hd-lang/issues/846) std data validation: design note (owner approval), then Validate derive and checked readers

- Design validation and post-decode checked readers (`a4f0007e5`).
- Record validation signature writing feedback (`5beb3dedb`).
- Record validation review writing feedback (`b0a1c08b4`).
- Record validation review writing feedback (`96fcdf3a5`).
- Validation: accepted design and checked signature package (PR #849) (`3101395f4`).

### [#632](https://github.com/HerringtonDarkholme/hd-lang/issues/632) impl Trait 3: trait methods returning impl Trait

- Allow opaque return types in trait methods (`a6e8f00db`).

### [#851](https://github.com/HerringtonDarkholme/hd-lang/issues/851) Release pipeline: 15 impl Trait conformance cases fail with unsupported:Collect

- Keep inlined copies well typed and verify release conformance TIR (`d5888a394`).

### [#850](https://github.com/HerringtonDarkholme/hd-lang/issues/850) Linear backend rejects a panic used as a call argument

- End a block at a never-typed argument in the linear backend (`4124acf92`).

### [#852](https://github.com/HerringtonDarkholme/hd-lang/issues/852) Web size: load the WAT printer as a separate module on first use

- Load the WAT printer only when the playground requests it (`2f7b14aa9`).

### [#853](https://github.com/HerringtonDarkholme/hd-lang/issues/853) Web size: NFC check through the host instead of linked Unicode tables

- Move web Unicode normalization to the host (`2d7b4913c`).

### [#856](https://github.com/HerringtonDarkholme/hd-lang/issues/856) Inliner: one selection rule for std and user functions

- Inline std and user functions by one rule (`49ff2f1fa`).

### [#863](https://github.com/HerringtonDarkholme/hd-lang/issues/863) Bench: a nested-Display program to measure Display.write's gain

- Add a nested Display benchmark and matching JS/MoonBit ports (`3a2b281fa`).

### [#865](https://github.com/HerringtonDarkholme/hd-lang/issues/865) Discarded host error strings still pull in the allocator

- Skip owned decoding of discarded host string payloads (`3d9b3c21b`).
- Handle dynamic operands before discarded payload proofs (`d5d58cd26`).

### [#883](https://github.com/HerringtonDarkholme/hd-lang/issues/883) Abandoning a cold host call loses its abandoned cleanup result

- Preserve abandonment of cold host calls (`e8b2f935c`).

### [#887](https://github.com/HerringtonDarkholme/hd-lang/issues/887) RC: generic methods lend parameters they only read (open-type parameters are always kept today)

- Release summaries: lend open-type parameters a body only reads (`f53b05393`).

### [#864](https://github.com/HerringtonDarkholme/hd-lang/issues/864) Immediate host drives still allocate on the Pending path

- Avoid heap frames for synchronous pending host drives (`52e5b7c94`).

### [#872](https://github.com/HerringtonDarkholme/hd-lang/issues/872) Unused cold vtable slots keep host frame allocators linked

- Prune unused normal host slots for immediate drives (`4e841e866`).

### [#888](https://github.com/HerringtonDarkholme/hd-lang/issues/888) Synchronous host wake bookkeeping still retains the allocator

- Keep synchronous host wake registrations in caller-local leaves (`35cef9c63`).

### [#900](https://github.com/HerringtonDarkholme/hd-lang/issues/900) Flaky: SDK Vercel relocation test fails with fetch EPIPE under load

- Stabilize Vercel declared-body rejection control (`e67713fa5`).

### [#897](https://github.com/HerringtonDarkholme/hd-lang/issues/897) Serde: walker and reader member methods take &Member, not Member

- Borrow serde member and variant metadata in place (`e05b4e030`).

### [#898](https://github.com/HerringtonDarkholme/hd-lang/issues/898) RC: an inlined read through a reference borrows instead of copying the list

- Borrow a binding local's read through a reference, keeping its drops (`38e478d4c`).

### [#903](https://github.com/HerringtonDarkholme/hd-lang/issues/903) Server SDK schema: refresh source hashes after #897 changed lib/std/http.hd

- Refresh server SDK schema source provenance (`3b0e4cb4b`).

### [#905](https://github.com/HerringtonDarkholme/hd-lang/issues/905) Conformance: a checked case that catches a leaked view release in a borrowed binding

- Cover borrowed erased-view binding releases in checked mode (`c9bd5db83`).

### [#909](https://github.com/HerringtonDarkholme/hd-lang/issues/909) RC: json's non-string payload-start counts still search the page table

- Whole-object scan: a lent field reference makes no counted address (`bf2cfa318`).

### [#906](https://github.com/HerringtonDarkholme/hd-lang/issues/906) Design: an architecture for hd fmt

- Design: hd fmt architecture, one fixed style printed from the syntax tree (#911) (`6db9428a3`).

### [#908](https://github.com/HerringtonDarkholme/hd-lang/issues/908) Design: an architecture for hd lint

- Design: hd lint architecture, one registry and one analysis walk (#917) (`010157271`).

### [#916](https://github.com/HerringtonDarkholme/hd-lang/issues/916) Checker: a method named debug escapes the void-tail type check

- Checker: identify prelude debug by selected callee (`e5d7c33c3`).

### [#882](https://github.com/HerringtonDarkholme/hd-lang/issues/882) Checker: a ? hint's solved type argument doesn't solve a parameter named only in bounds

- Infer bound-only call parameters from try hints (`97279d70c`).
- Generalize try-hint argument recovery (`a7f665650`).

### [#902](https://github.com/HerringtonDarkholme/hd-lang/issues/902) Process launcher protocol test times out during concurrent compilation

- Stabilize process launcher protocol control (`ff524f56b`).

### [#886](https://github.com/HerringtonDarkholme/hd-lang/issues/886) Restore layout caches after a caught compiler panic

- Restore layout caches after compiler panics (`6091021c2`).

### [#928](https://github.com/HerringtonDarkholme/hd-lang/issues/928) Flaky: SDK cleanup test with a 100ms logical Clock fails under load

- Make SDK cleanup clock deterministic (`820a39262`).

### [#899](https://github.com/HerringtonDarkholme/hd-lang/issues/899) hd fmt: a maximum line width that breaks long literals, calls and signatures

- Refresh canonical-format maintenance data (`d8b533e6d`).

### [#923](https://github.com/HerringtonDarkholme/hd-lang/issues/923) CLI conformance: a wasmtime-engine case should be unsupported, not failed, in a build without wasmtime

- Treat unavailable Wasmtime cases as unsupported (`3b9774ac3`).

### [#931](https://github.com/HerringtonDarkholme/hd-lang/issues/931) Wasmtime journal expects one panic stderr write

- Fix Wasmtime panic journal expectation (`42c9eae07`).

### [#930](https://github.com/HerringtonDarkholme/hd-lang/issues/930) Layout: Option[*T] is a plain pointer, null for .None

- Lay out an optional reference as one nullable address (`b357c39e4`).

### [#933](https://github.com/HerringtonDarkholme/hd-lang/issues/933) Docs: bring the guide, tour, playground and agent skills up to date

- Refresh the hd writing and CLI skills (`70e5ff8cb`).

### [#921](https://github.com/HerringtonDarkholme/hd-lang/issues/921) Size: json links encode's tree fallback, 8.75 KB it never executes

- Prune unreachable JSON encode fallbacks (`25ab3448e`).

### [#873](https://github.com/HerringtonDarkholme/hd-lang/issues/873) Language idea: @inline(.Always / .Never) to override the inliner

- Implement always-inline copying (`6704824fc`).
- Update the inline ownership baseline (`7313a507a`).

### [#891](https://github.com/HerringtonDarkholme/hd-lang/issues/891) Static println arguments keep generic count and free code linked

- Elide proved static println counts (`34563c2e5`).
- Keep static count proofs sound (`0d501c506`).

### [#868](https://github.com/HerringtonDarkholme/hd-lang/issues/868) Host start-up: an empty program costs 0.23 ms in host.run and about 6 ms of host loading

- Cut host startup to the guest execution ceiling (`841b12864`).
- Refresh the portable runtime after the host split (`f87957920`).

### [#937](https://github.com/HerringtonDarkholme/hd-lang/issues/937) Bench: re-tune every program's input to about 300 ms on today's hd

- Retune benchmark inputs for current V8 (`d5c904978`).

### [#936](https://github.com/HerringtonDarkholme/hd-lang/issues/936) Console: output is synchronous (write! becomes write_sync)

- Console output is synchronous: write_sync (`a95f162b6`).

### [#922](https://github.com/HerringtonDarkholme/hd-lang/issues/922) Size: custom sections are a quarter of json.wasm

- Strip resolved names from written Wasm (`374e18a39`).

### [#938](https://github.com/HerringtonDarkholme/hd-lang/issues/938) Perf: 8-byte header for cycle-capable objects, and no allocation counter in release

- Keep the cycle collector's state in the descriptor word (`a3937df9f`).

### [#943](https://github.com/HerringtonDarkholme/hd-lang/issues/943) Tour: a saved edit survives a lesson's source change and shows stale code

- Version saved tour edits by lesson source (`d950dfca2`).

### [#944](https://github.com/HerringtonDarkholme/hd-lang/issues/944) Flaky: a REPL stop that lands in rt:alloc reports an error, not stopped

- Keep REPL stops out of session setup (`9fe26e6a8`).

### [#946](https://github.com/HerringtonDarkholme/hd-lang/issues/946) std.host: env configuration leaves Deserialize; env_decode stays, with a prefix

- Move environment configuration out of serde (`fccac3840`).

### [#947](https://github.com/HerringtonDarkholme/hd-lang/issues/947) Perf: reuse a map read's hash and slot for the following store of the same key

- Reuse map probes for same-key updates (`42c5550a9`).

### [#817](https://github.com/HerringtonDarkholme/hd-lang/issues/817) hd lint: warn on any dropped change to a mut parameter or receiver, push included

- Add lost-write lint for mutable parameter copies (`057b255dc`).
- Avoid lost-write warnings for loop-carried reads (`0d4fc192f`).

### [#855](https://github.com/HerringtonDarkholme/hd-lang/issues/855) hd lint: move every compiler warning out of check, build and test

- Move broken documentation links into hd lint (`ad94bba9d`).

### [#842](https://github.com/HerringtonDarkholme/hd-lang/issues/842) serde: Serialize and Deserialize for Result

- Add Result serde implementations (`c7d9877eb`).
- Use newtype variants for Result serde (`425f95882`).

### [#939](https://github.com/HerringtonDarkholme/hd-lang/issues/939) Perf: one collector check per drain and no descriptor reload on free

- Wasm: streamline generic release drains (`0c71c705f`).

### [#963](https://github.com/HerringtonDarkholme/hd-lang/issues/963) std.testing: assertion reasons are optional

- Testing: make assertion reasons optional (`d469716da`).

### [#942](https://github.com/HerringtonDarkholme/hd-lang/issues/942) Tour: rewrite into the 18-lesson structure

- Rewrite the language tour as 18 lessons (`ba713929b`).
- Tour: simplify the 18-lesson examples (`f631428f2`).

### [#975](https://github.com/HerringtonDarkholme/hd-lang/issues/975) Perf: partially evaluate generated Structure walk bodies per type (walk only)

- Specialize generated Structure walks (`5e909a438`).
- Restore Structure data-hook specialization (`0d42ca4aa`).

### [#953](https://github.com/HerringtonDarkholme/hd-lang/issues/953) serde: derived enum variants follow Rust's forms (newtype bare, tuple as array)

- Split serde struct and enum protocols (`b4dc3ec2b`).
- Require serde variant format hooks (`278ac359b`).

### [#880](https://github.com/HerringtonDarkholme/hd-lang/issues/880) Stdlib idea: design-by-contract require, ensure and invariant

- Add always-on contract require (`bcb259716`).

### [#805](https://github.com/HerringtonDarkholme/hd-lang/issues/805) CLI: fold hd fix into hd check --fix and hd lint --fix

- CLI: fold fix into check and lint (`6d8119f6b`).

### [#890](https://github.com/HerringtonDarkholme/hd-lang/issues/890) Console: heap-free ConsoleError (codes, no host strings) so dev hello links no allocator

- Make console errors heap-free (`8e9be2776`).

### [#955](https://github.com/HerringtonDarkholme/hd-lang/issues/955) serde: non-string map keys through a MapKey trait; JSON follows serde_json

- Support scalar map keys in serde (`dcfedd83a`).

### [#979](https://github.com/HerringtonDarkholme/hd-lang/issues/979) #912 S1: restore the counted-reference spec and amend it (Q4, list views, error wording)

- Spec: restore counted references (#912 S1) and amend them (`f91b9df05`).

### [#980](https://github.com/HerringtonDarkholme/hd-lang/issues/980) #912 S2: the box intrinsic with its no-local-reference check

- Add the box intrinsic (#912 S2) (`1d917a663`).

### [#977](https://github.com/HerringtonDarkholme/hd-lang/issues/977) serde: SkipNone as a phantom fact; omission test via a defaulted Serialize.is_absent

- Serde: make SkipNone a phantom fact (`10197d063`).

### [#935](https://github.com/HerringtonDarkholme/hd-lang/issues/935) RC: acyclic types drop collector bookkeeping (size win; fix the TurboFan regression first)

- Wasm: omit collector state for acyclic types (`8f13c8e86`).
- Keep polymorphic recursive types cycle-capable (`bafb9f0ce`).
- Wasm: bound recursive cycle unfolding (`796907d04`).

### [#981](https://github.com/HerringtonDarkholme/hd-lang/issues/981) #912 S3: rewrite std for local-only &

- std: box new objects and shared state for local-only & (#912 S3) (`a05eeefd3`).

### [#982](https://github.com/HerringtonDarkholme/hd-lang/issues/982) #912 S4: list views by value (Swift ArraySlice snapshots)

- List views by value: ListView holds a copy-on-write list snapshot (`33b1b8491`).

### [#965](https://github.com/HerringtonDarkholme/hd-lang/issues/965) Build @cfg(.Debug) / @cfg(.Release) profile-selected declarations

- Build profile-selected cfg declarations (`ceda3f9fc`).

### [#962](https://github.com/HerringtonDarkholme/hd-lang/issues/962) Build sync using / using! (Disposable, AsyncDisposable), on_exit, and the unbound-disposable lint

- Split synchronous and asynchronous using cleanup (`ba78a0b8d`).

### [#983](https://github.com/HerringtonDarkholme/hd-lang/issues/983) #912 S5a: rewrite runtime/valid fixtures a-l

- Conformance: rewrite runtime/valid a-l fixtures for local-only & (#912 S5a) (`1c3bd6af7`).

### [#948](https://github.com/HerringtonDarkholme/hd-lang/issues/948) std.http: build the accepted multipart/form-data bodies

- Add buffered HTTP multipart forms (`02ac2bd53`).
- Follow up multipart review (`275c65045`).
- Correct multipart boundary generation (`000a9f852`).

### [#990](https://github.com/HerringtonDarkholme/hd-lang/issues/990) Checker: a derivation member's fact type should reach the receiver of a method chain

- Checker: a typed fact built by a method chain checks at its expected fact type (`ee887277a`).

### [#961](https://github.com/HerringtonDarkholme/hd-lang/issues/961) Language: row parameters on structs, enums and traits ($R), so stored callbacks keep their row

- Allow requirement rows on data declarations (`5b82474f0`).

### [#984](https://github.com/HerringtonDarkholme/hd-lang/issues/984) #912 S5b: rewrite runtime/valid fixtures m-z

- Conformance: rewrite runtime/valid m-z fixtures for local-only & (#912 S5b) (`910c459ef`).

### [#985](https://github.com/HerringtonDarkholme/hd-lang/issues/985) #912 S5c: rewrite typing, panic, cli and tree fixtures

- Conformance: rewrite typing, panic, cli and tree fixtures for local-only & (#912 S5c) (`ccd0893b0`).

### [#991](https://github.com/HerringtonDarkholme/hd-lang/issues/991) Cleanup protocols carry a requirement row: Disposable[$R], then ScopeGuard[$R] and on_exit

- Carry cleanup requirement rows through using (`9592ba495`).

### [#992](https://github.com/HerringtonDarkholme/hd-lang/issues/992) Runtime: repeated build and drop of lists holding erased counted references traps (collection-pinned, unreachable)

- Linear: a coercion of a lent reference takes no count (`9d897b09a`).

### [#986](https://github.com/HerringtonDarkholme/hd-lang/issues/986) #912 S6: rewrite examples, packages, tour, guide and skill

- Rewrite examples, packages, guide, skill and test snippets for local-only & (#912 S6) (`2dc55b507`).

### [#987](https://github.com/HerringtonDarkholme/hd-lang/issues/987) #912 S7: switch & to local-only and *self on places to an error

- Switch & to local-only and *self on places to an error (#912 S7) (`628292d73`).

### [#720](https://github.com/HerringtonDarkholme/hd-lang/issues/720) Weak references: std Weak[T] with an on-demand side table (spec + runtime + std)

- Add weak references with on-demand side entries (`b818e7cfa`).

### [#949](https://github.com/HerringtonDarkholme/hd-lang/issues/949) std.http: build the accepted host-side gzip/deflate decompression

- Decode compressed HTTP responses in host transports (`20872555b`).

### [#976](https://github.com/HerringtonDarkholme/hd-lang/issues/976) std: implement the accepted validation design (Validate, rule facts, validated())

- Implement standard validation (`c6d7ffe6a`).
- Validation: preserve contextual optional facts (`36ba6f408`).

### [#988](https://github.com/HerringtonDarkholme/hd-lang/issues/988) #912 S8: runtime cleanup (whole-object scan, place-*T lowering)

- Lower *T to a whole-object carrier; remove the whole-object scan (#912 S8) (`88bc244fb`).

### [#997](https://github.com/HerringtonDarkholme/hd-lang/issues/997) Perf: inliner keeps the Counted carrier of a bound `&mut self` (structure_walk +3% after #988)

- Keep counted carriers through inlined bindings (`594365215`).

### [#1000](https://github.com/HerringtonDarkholme/hd-lang/issues/1000) Runtime: debug of a boxed `*Point` erased to `*dyn Debug` prints the pointer word as the struct

- Dispatch boxed references through trait value slots (`e2a25e4d2`).

### [#994](https://github.com/HerringtonDarkholme/hd-lang/issues/994) Size: fold fact lookups after monomorphization so unused facts cost nothing

- Fold absent declaration facts per instance (`5eb7f7eef`).
- Finish folding generated absent facts (`7a18e845e`).
- Accept either lowered fact reference carrier (`30658c55f`).
- Invalidate mutated fact-list provenance (`87b78c64c`).
- Restrict fact proofs to Facts.items (`8047441b6`).

### [#1004](https://github.com/HerringtonDarkholme/hd-lang/issues/1004) Remove the "write `*`" fix-it from local-reference-escapes

- Remove local reference escape fix-its (`5a155e7ba`).

### [#165](https://github.com/HerringtonDarkholme/hd-lang/issues/165) Runtime perf against JS and MoonBit (lane index)

- Bench: refresh runtime against JavaScript (`260872e3c`).

### [#1003](https://github.com/HerringtonDarkholme/hd-lang/issues/1003) Returned references stage 1: no local reference as a generic type argument (rule 9, #987)

- Reject local references in generic call arguments (`e31c4b7c4`).

### [#1008](https://github.com/HerringtonDarkholme/hd-lang/issues/1008) Returned references stage 4: element references in bindings and dependent results

- Allow element references in dependent results (`1586412c5`).

### [#1006](https://github.com/HerringtonDarkholme/hd-lang/issues/1006) Returned references stage 2b: frame-slot results and inlined reference getters

- Inliner: copy reference getters (#1006) (`f857f7005`).

### [#1010](https://github.com/HerringtonDarkholme/hd-lang/issues/1010) Returned references stage 3a: lifetime variable syntax ('a), parsed and resolved

- Returned references stage 3a: lifetime variables, parsed and resolved (`bdcfc7a6f`).

### [#1009](https://github.com/HerringtonDarkholme/hd-lang/issues/1009) std: in-place element accessors (List get_mut/first_mut/last_mut, Map get_mut)

- Add mutable collection accessors (`e40edc82f`).

### [#996](https://github.com/HerringtonDarkholme/hd-lang/issues/996) Downcast: one &self / &mut self form returning &T? / (&mut T)? (Rust's)

- Use local references for downcast (`3bd05d50f`).

### [#1011](https://github.com/HerringtonDarkholme/hd-lang/issues/1011) Returned references stage 3b: lifetime variables in signatures, per-slot provenance

- Returned references stage 3b: lifetime variables checked per reference position (`b5bfd3ec0`).

### [#1013](https://github.com/HerringtonDarkholme/hd-lang/issues/1013) std: drop #912-era boxes that only share a value within a call (local references now return)

- Use local references for transient std state (`85aec2687`).

### [#1012](https://github.com/HerringtonDarkholme/hd-lang/issues/1012) Returned references stage 3c: lifetimes in function types, trait methods and &dyn

- Returned references stage 3c: lifetimes in function types, trait methods, trait impls and &dyn calls (`884f442e0`).

### [#1015](https://github.com/HerringtonDarkholme/hd-lang/issues/1015) Lifetimes: accept more general function values and trait impls (Rust subtyping)

- Allow more general lifetime dependencies (`2410f9331`).

### [#1033](https://github.com/HerringtonDarkholme/hd-lang/issues/1033) Soundness: an opaque impl Trait result can hide a local reference

- Opaque results that borrow: an impl Trait result is a dependent result (`eb99ba00f`).

### [#1014](https://github.com/HerringtonDarkholme/hd-lang/issues/1014) std: Map.get_or_insert(&mut self, key, default) -> &mut V

- Add Map.get_or_insert (`b93bc1f83`).
- Lower mutable map lookups with one probe (`22b4b32ed`).

### [#993](https://github.com/HerringtonDarkholme/hd-lang/issues/993) #875 stage 1: spec scalar const generic parameters, and split the build into slices

- Specify scalar const generic parameters (`4bfc9d6f7`).

### [#1017](https://github.com/HerringtonDarkholme/hd-lang/issues/1017) Returned references 6a1: struct lifetime parameters and `&'a` fields

- Returned references 6a1: struct lifetime parameters and `&'a` fields (`e33a177d2`).

### [#1018](https://github.com/HerringtonDarkholme/hd-lang/issues/1018) Returned references 6a2: field reads of a lifetime struct carry its origins

- Returned references 6a2: field reads of a lifetime struct carry its origins (`def0994ea`).

### [#1027](https://github.com/HerringtonDarkholme/hd-lang/issues/1027) Returned references 6d2: Fn[I, O, $R] trait with Fn(A, B) -> R sugar

- Add static Fn bounds (`937eca14f`).

### [#1034](https://github.com/HerringtonDarkholme/hd-lang/issues/1034) Const generics build slice 1: syntax and resolver

- Parse and resolve const generic slots (`ac52ca6e2`).

### [#1019](https://github.com/HerringtonDarkholme/hd-lang/issues/1019) Returned references 6a3: elided lifetime arguments on struct instances in signatures

- Returned references 6a3: elided lifetime arguments on struct instances in signatures (`7da90985c`).

### [#1037](https://github.com/HerringtonDarkholme/hd-lang/issues/1037) Fn bounds: accept suspending functions, solve row arguments, infer closure parameters

- Infer generic Fn bounds from function values (`09c71c3a1`).

### [#1020](https://github.com/HerringtonDarkholme/hd-lang/issues/1020) Returned references 6a4: enum lifetime parameters

- Returned references 6a4: enum lifetime parameters (`a46490a5f`).

### [#1035](https://github.com/HerringtonDarkholme/hd-lang/issues/1035) std: stored callbacks become generic F < Fn(...) (before fn parameters may capture locals)

- Make stored std callbacks generic over Fn (`6ece56f07`).

### [#1021](https://github.com/HerringtonDarkholme/hd-lang/issues/1021) Returned references 6b1: stores into a caller's place through a shared lifetime

- Returned references 6b1: stores into a caller's place through a shared lifetime (`2e3e646b9`).

### [#1026](https://github.com/HerringtonDarkholme/hd-lang/issues/1026) Returned references 6d1: fn parameters may capture local references

- Let fn parameters carry scoped reference captures (`22a85aff4`).

### [#1022](https://github.com/HerringtonDarkholme/hd-lang/issues/1022) Returned references 6b2: assigning a lifetime field through &mut

- Returned references 6b2: field stores through a struct of two lifetimes (`27b1f3bfa`).

### [#1038](https://github.com/HerringtonDarkholme/hd-lang/issues/1038) Shared data may not declare lifetime parameters

- Shared enum data may not name the enum's lifetimes (`0fc7aa948`).

### [#1028](https://github.com/HerringtonDarkholme/hd-lang/issues/1028) Returned references 6d3: stored capturing closures, Fn() -> R & 'a

- Returned references 6d3: stored capturing closures through a lifetime-bounded parameter (`61b6c9963`).

### [#1040](https://github.com/HerringtonDarkholme/hd-lang/issues/1040) Returned references 6d4: closure argument for a generic Fn parameter

- Returned references 6d4: closure arguments for a generic Fn parameter (`48d3ac2a1`).

### [#1057](https://github.com/HerringtonDarkholme/hd-lang/issues/1057) Returned references 6d5: capturing closures for F inside another type (iterator adapters)

- Returned references 6d5: capturing closures for F inside another type (`b4b3446ba`).

### [#1042](https://github.com/HerringtonDarkholme/hd-lang/issues/1042) Returned references 6e4: require's condition is read-only

- Returned references 6e4: require's condition is read-only (`1f6a497d7`).

### [#1058](https://github.com/HerringtonDarkholme/hd-lang/issues/1058) Returned references 6d6: capturing closures in a struct literal for an unbounded type parameter

- Returned references 6d6: capturing closures in a struct literal for an unbounded F (`1c8dd4662`).

### [#1036](https://github.com/HerringtonDarkholme/hd-lang/issues/1036) Dependent results: an auto-referenced method receiver adds its own storage to the result's origins

- Dependent results: an auto-addressed receiver has the explicit &p's origins (`e77e0c2d3`).

### [#1053](https://github.com/HerringtonDarkholme/hd-lang/issues/1053) Body bound check repeats errors for default-filled arguments

- Checker: check written type bounds once (`e32681a5c`).

### [#1044](https://github.com/HerringtonDarkholme/hd-lang/issues/1044) Trial rollback recomputes the env key even with no new clauses

- Checker: keep unchanged trial environment keys (`dc38a9f78`).

### [#1043](https://github.com/HerringtonDarkholme/hd-lang/issues/1043) Trait method calls clone the callee FnSig three times

- Checker: borrow trait method signatures for reads (`3958ed8b9`).

### [#1055](https://github.com/HerringtonDarkholme/hd-lang/issues/1055) Fs, Net and Http workers import node.mjs for one helper each

- Node host: move worker helpers below node layer (`ab551e0dc`).

### [#1060](https://github.com/HerringtonDarkholme/hd-lang/issues/1060) Lifetime solver S1: slot layouts and field slot maps

- Lifetime solver S1: slot layouts and field slot maps (`42f65b28e`).

### [#1054](https://github.com/HerringtonDarkholme/hd-lang/issues/1054) Node capability workers duplicate the reply loop and fault shape

- Node host: centralize capability worker replies (`58d4032c9`).

### [#1061](https://github.com/HerringtonDarkholme/hd-lang/issues/1061) Lifetime solver S2: region table, constraint graph and SCC solver (not wired)

- Lifetime solver S2: region table, constraint graph and SCC solver (`d4784ee9d`).

### [#1023](https://github.com/HerringtonDarkholme/hd-lang/issues/1023) Returned references 6c1: trait lifetime parameters

- Checker: support trait lifetime parameters (`421586bb9`).

### [#1024](https://github.com/HerringtonDarkholme/hd-lang/issues/1024) Returned references 6c2: type aliases with lifetimes

- Resolver: support type alias lifetime parameters (`980e8b3db`).

### [#1062](https://github.com/HerringtonDarkholme/hd-lang/issues/1062) Lifetime solver S3: constraints for places, stores, constructions and control flow (shadow)

- Lifetime solver S3: constraints for places, stores, constructions and control flow (shadow) (`46487d3e4`).

### [#1025](https://github.com/HerringtonDarkholme/hd-lang/issues/1025) Returned references 6c3: lifetimes in impl targets

- Resolver: support lifetimes in implementation targets (`2ed713c22`).

### [#1056](https://github.com/HerringtonDarkholme/hd-lang/issues/1056) Node host: each console write sleeps 1 ms under --time-limit

- Node host: finish synchronous output in one round trip (`8a1d5e73d`).

### [#950](https://github.com/HerringtonDarkholme/hd-lang/issues/950) std.http: build the accepted transport settings on the Http provider (DNS, TLS, phase timeouts)

- Server SDK: finish transport settings integration for #950 (`60703e965`).

### [#1066](https://github.com/HerringtonDarkholme/hd-lang/issues/1066) Const arguments in headers report unsupported for bad names

- Report bad const names in headers (`991025682`).

### [#1067](https://github.com/HerringtonDarkholme/hd-lang/issues/1067) Poisoned const type argument reaches the TIR verifier

- Contain poisoned type arguments before TIR (`e125a320c`).

### [#1064](https://github.com/HerringtonDarkholme/hd-lang/issues/1064) Lifetime solver S4: constraints for calls and closures (shadow)

- Lifetime solver S4: constraints for calls and closures (shadow) (`b50682354`).

### [#1083](https://github.com/HerringtonDarkholme/hd-lang/issues/1083) Lifetime solver S6: switch diagnostics to the new solver and delete the old paths

- Lifetime solver S6: diagnostics come from the region solver (`b34aca5b7`).

### [#1063](https://github.com/HerringtonDarkholme/hd-lang/issues/1063) Lifetime solver S7: stores through a reborrowed sub-place

- Lifetime solver S7: stores through a reborrowed sub-place (`a92a81d8f`).

### [#1041](https://github.com/HerringtonDarkholme/hd-lang/issues/1041) Returned references: a fn result capturing two fn parameters is ambiguous

- Lifetime solver S8: a fn result capturing two fn parameters is ambiguous (`70a160271`).

### [#1090](https://github.com/HerringtonDarkholme/hd-lang/issues/1090) Lifetime solver S9: a reborrow of a part keeps its own lifetimes (shared and &mut)

- Lifetime solver S9: a reborrow of a part keeps its own lifetimes (`2367fcb45`).

### [#1068](https://github.com/HerringtonDarkholme/hd-lang/issues/1068) Const generics build slice 5: mono and stable instance keys

- Monomorphize const generic arguments (`f5dbb6294`).

### [#1092](https://github.com/HerringtonDarkholme/hd-lang/issues/1092) Lifetime solver S11: drop the whole-referent union of ref.lifetime.store

- Lifetime solver S11: a store through a reference keeps each position (`bd5ac6da6`).

### [#1087](https://github.com/HerringtonDarkholme/hd-lang/issues/1087) TypeScript: host sources in plain .ts (not .mts/.mjs)

- Migrate host sources to plain TypeScript (`28c91d92b`).

### [#1093](https://github.com/HerringtonDarkholme/hd-lang/issues/1093) Lifetime solver S12: drop ref.generic.call.unaligned

- Lifetime solver S12: drop ref.generic.call.unaligned (`912a7547a`).

### [#1095](https://github.com/HerringtonDarkholme/hd-lang/issues/1095) Lifetimes: a trait value keeps its lifetime arguments

- Lifetime solver: a trait value keeps its lifetime arguments (`56a09178b`).

### [#1096](https://github.com/HerringtonDarkholme/hd-lang/issues/1096) Lifetimes 6f: on_exit guards may capture local references

- Lifetimes 6f: on_exit guards may capture local references (`e14e1e944`).

### [#1076](https://github.com/HerringtonDarkholme/hd-lang/issues/1076) Migrate file and process workers to .mts

- Type file and process workers (`eb97f5c5c`).

### [#1077](https://github.com/HerringtonDarkholme/hd-lang/issues/1077) Migrate network worker modules to .mts

- Type network host workers (`5678f31ab`).

### [#1078](https://github.com/HerringtonDarkholme/hd-lang/issues/1078) Migrate the Node host root and CLI embedding to .mts

- Type Node host root and session (`300119efb`).

### [#1079](https://github.com/HerringtonDarkholme/hd-lang/issues/1079) Migrate host startup and run modules to .mts

- Type startup and run orchestration (`26de8edda`).

### [#1080](https://github.com/HerringtonDarkholme/hd-lang/issues/1080) Migrate host test orchestration to .mts

- Type host test orchestration (`a0a207015`).

### [#1081](https://github.com/HerringtonDarkholme/hd-lang/issues/1081) Finish host .mts integration and enforce the source invariant

- Enforce typed plain-TypeScript host sources (`dedbb1b15`).

### [#1088](https://github.com/HerringtonDarkholme/hd-lang/issues/1088) TypeScript: server SDK sources in plain .ts

- Migrate server SDK sources to TypeScript (`4349471ad`).

### [#1089](https://github.com/HerringtonDarkholme/hd-lang/issues/1089) TypeScript: spec, bench, scripts and test helpers in plain .ts

- Migrate remaining tooling sources to TypeScript (`2b86e4fb4`).

### [#1098](https://github.com/HerringtonDarkholme/hd-lang/issues/1098) Lifetimes stage 7a: a live shared reference blocks writes to its place

- Lifetimes stage 7a: a live shared reference blocks writes to its place (`d4934f4d3`).

### [#1100](https://github.com/HerringtonDarkholme/hd-lang/issues/1100) Lifetimes: an impl's associated type may name the impl's lifetimes

- Lifetimes: an impl's associated type may name the impl's lifetimes (`4e2b9ae71`).

### [#1086](https://github.com/HerringtonDarkholme/hd-lang/issues/1086) Lint: no one-letter identifiers (id-length)

- Ban one-letter JavaScript and TypeScript identifiers (`260e7b630`).

### [#1094](https://github.com/HerringtonDarkholme/hd-lang/issues/1094) Lifetimes: a suspension holds its arguments' references

- Lifetimes: a suspension holds its arguments' references (`b729d6128`).
- Lifetimes: a suspension that borrows a local stays where its type can hold it (`0c170b47e`).

### [#1102](https://github.com/HerringtonDarkholme/hd-lang/issues/1102) Lifetimes: Suspend[R, 'a], a stored suspension's lifetime

- Lifetimes: Suspend[R, 'a] names a suspension's held position (`aa411e789`).

### [#1097](https://github.com/HerringtonDarkholme/hd-lang/issues/1097) Lifetimes 6h: Span[T, 'a], a read-only live list view

- Add borrowed Span views to std collections (`c3743ee7d`).

### [#1103](https://github.com/HerringtonDarkholme/hd-lang/issues/1103) Lifetimes: an impl's trait arguments may name the impl's lifetimes

- Lifetimes: an impl's trait arguments may name the impl's lifetimes (`e661e7b61`).

### [#1105](https://github.com/HerringtonDarkholme/hd-lang/issues/1105) Lifetimes: a for loop over a local cursor keeps the items' origins

- Lifetimes: a for loop's items keep the origins their cursor holds (`3031de7fc`).

### [#1106](https://github.com/HerringtonDarkholme/hd-lang/issues/1106) Lifetimes stage 7b: a call's live result locks its whole source

- Lifetimes stage 7b: a call's live shared result locks its whole source (`bcb77de0e`).

### [#1107](https://github.com/HerringtonDarkholme/hd-lang/issues/1107) Lifetimes stage 7c: a live &mut excludes every other use of its place

- Lifetimes stage 7c: a live &mut excludes every other use of its place (`a407dd272`).

### [#1099](https://github.com/HerringtonDarkholme/hd-lang/issues/1099) Lifetimes 6g1: AutoClosing and @auto_closing registration

- Implement AutoClosing statement registration (`ed9335121`).
- Leave aborted cleanup blocks to the discarded body (`3ce63e3fc`).

### [#1104](https://github.com/HerringtonDarkholme/hd-lang/issues/1104) Lifetimes 6h: Span.iter()

- Add borrowed Span iteration (`feb3c34c2`).

### [#1030](https://github.com/HerringtonDarkholme/hd-lang/issues/1030) Language: argument-position impl Trait (sugar for an anonymous type parameter)

- Implement argument-position impl Trait (`e91ac436e`).

### [#1108](https://github.com/HerringtonDarkholme/hd-lang/issues/1108) Lifetimes stage 7c2: a call's &mut argument excludes its other arguments' access to that place

- Lifetimes stage 7c2: a call's &mut argument excludes its other arguments' access (`74e9a1e2c`).

### [#1101](https://github.com/HerringtonDarkholme/hd-lang/issues/1101) Lifetimes 6g2: library defer and ensure

- Add library defer and ensure (`bbb3e2145`).

### [#1111](https://github.com/HerringtonDarkholme/hd-lang/issues/1111) Lifetimes stage 7c3: a call's & argument excludes a later argument's write to that place

- Lifetimes stage 7c3: a call's & argument excludes a later argument's write (`130e02997`).

### [#1109](https://github.com/HerringtonDarkholme/hd-lang/issues/1109) Lifetimes 6h: SpanMut[T, 'a], a mutable live list view

- Add mutable list spans (`7deb680b7`).

### [#1112](https://github.com/HerringtonDarkholme/hd-lang/issues/1112) Lifetimes stage 7d: a seen length change while an element reference is live is a compile error

- Lifetimes stage 7d: a seen length change while an element reference is live is a compile error (`aebc64f2f`).

### [#1113](https://github.com/HerringtonDarkholme/hd-lang/issues/1113) Lifetimes stage 7d2: an element reference beside a &mut of its collection in one call is a compile error

- Lifetimes stage 7d2: an element reference beside a &mut of its collection in one call is a compile error (`e786fc9e3`).

### [#1110](https://github.com/HerringtonDarkholme/hd-lang/issues/1110) Const generics build slice 6: verify backend integration

- Verify const generic backend integration (`5d043178d`).

### [#1114](https://github.com/HerringtonDarkholme/hd-lang/issues/1114) Lifetimes stage 7e: a conflicting access through an unseen *mut alias panics borrow-conflict

- Lifetimes stage 7e: a conflicting access through an unseen *mut alias panics borrow-conflict (`a32642150`).

### [#1116](https://github.com/HerringtonDarkholme/hd-lang/issues/1116) Remove defer: (1/2): convert every use to library defer(...)

- Convert defer statements to library calls (`431aebccb`).

### [#1117](https://github.com/HerringtonDarkholme/hd-lang/issues/1117) Remove defer: (2/2): drop the statement from the language

- Remove defer statement syntax (`22f389658`).

### [#1120](https://github.com/HerringtonDarkholme/hd-lang/issues/1120) Unions U1: parse and print A | B in types

- Parse and format union type syntax (`e2cddc3b7`).

### [#1119](https://github.com/HerringtonDarkholme/hd-lang/issues/1119) Lifetimes stage 9a: 'param.path names the field a result points into

- Lifetimes stage 9a: 'param.path names the field a result points into (`3bbdc487f`).

### [#1121](https://github.com/HerringtonDarkholme/hd-lang/issues/1121) Unions U2: ordered union type identity and aliases

- Add ordered union type identity (`66cc286a0`).

### [#1122](https://github.com/HerringtonDarkholme/hd-lang/issues/1122) Unions U3: positional constructors and match

- Add positional union constructors and matching (`eaa3abe01`).

### [#1115](https://github.com/HerringtonDarkholme/hd-lang/issues/1115) Lifetimes stage 7e2: a call result through *mut registers its source at run time

- Lifetimes stage 7e2: a call's result through *mut registers its source at run time (`786400a00`).

### [#1126](https://github.com/HerringtonDarkholme/hd-lang/issues/1126) Lifetimes stage 9b: a call's path result locks only that path

- Lifetimes stage 9b: a call's path result locks only that path (`0d18e7770`).

### [#1123](https://github.com/HerringtonDarkholme/hd-lang/issues/1123) Unions U4: lower unions as enums

- Lower positional unions through enum runtime paths (`88bbf2fbe`).

### [#1127](https://github.com/HerringtonDarkholme/hd-lang/issues/1127) Lifetimes stage 9c: a result path may cross a *mut field

- Lifetimes stage 9c: a result path may cross a *mut field (`425e64781`).

### [#1124](https://github.com/HerringtonDarkholme/hd-lang/issues/1124) Unions U5: trait forwarding for unions

- Forward traits through positional unions (`29d2e19b0`).

### [#1129](https://github.com/HerringtonDarkholme/hd-lang/issues/1129) Lifetimes stage 7c4: a &mut result from a &self call holds an exclusive loan

- Lifetimes stage 7c4: a &mut result from a &self call holds an exclusive loan (`70b3e45bf`).

### [#1125](https://github.com/HerringtonDarkholme/hd-lang/issues/1125) Unions U6: conformance, parity and size report

- Finish positional union parity (`487517b86`).

### [#1134](https://github.com/HerringtonDarkholme/hd-lang/issues/1134) Run-time loans: a call result held in a suspending body

- Lifetimes 7e3: a call's result in a suspending body registers its loan (`f29fe5c3a`).

### [#1137](https://github.com/HerringtonDarkholme/hd-lang/issues/1137) Moves M1: a &mut value moves; calls reborrow it

- Moves M1: a &mut value moves; typed sites reborrow it (`cc862252e`).

### [#1132](https://github.com/HerringtonDarkholme/hd-lang/issues/1132) Run-time loans: a reference held inside an Option, a tuple or a struct

- Lifetimes 7e4: contained references register loans (`3519306a2`).

### [#1138](https://github.com/HerringtonDarkholme/hd-lang/issues/1138) Moves M2: a type holding a &mut is non-copyable

- Moves M2: a type holding a &mut is not copied (`760482386`).

### [#1136](https://github.com/HerringtonDarkholme/hd-lang/issues/1136) Run-time loans: a bang call's arguments as accesses

- Lifetimes 7e5: check bang arguments once (`575352208`).

### [#1133](https://github.com/HerringtonDarkholme/hd-lang/issues/1133) Run-time loans: a reference through a *T

- Lifetimes 7e6: loan through shared counted refs (`0c0a07d9c`).

### [#1140](https://github.com/HerringtonDarkholme/hd-lang/issues/1140) SpanMut methods borrow the span (&self / &mut self), as Rust's &mut [T]

- SpanMut methods borrow the span: get(&self), get_mut/slice_mut(&mut self) (`815fe6a4f`).

### [#1142](https://github.com/HerringtonDarkholme/hd-lang/issues/1142) Moves: moving a struct after copying its `&mut` field out leaves two live `&mut`

- Moves: partial moves of a non-copyable field (ref.move.field) (`b8eac990e`).

### [#1131](https://github.com/HerringtonDarkholme/hd-lang/issues/1131) Run-time loans: a reference a call takes for one call

- Lifetimes 7e7: loan references for a call (`8300fa8d6`).

### [#1135](https://github.com/HerringtonDarkholme/hd-lang/issues/1135) Run-time loans: replacing a *mut on a live loan's path

- Lifetimes 7e8: guard live loan pointer paths (`968e42c5c`).

### [#1143](https://github.com/HerringtonDarkholme/hd-lang/issues/1143) Moves: matching a non-copyable payload does not move the scrutinee

- Moves: a pattern moves a non-copyable payload out (ref.move.pattern) (`e6941e171`).

### [#1144](https://github.com/HerringtonDarkholme/hd-lang/issues/1144) Moves: `*r` copies a non-copyable value out of a reference

- Moves: no move out of a place behind a reference (ref.move.out-of-place) (`1e2d2242e`).

### [#1150](https://github.com/HerringtonDarkholme/hd-lang/issues/1150) Perf: lending a parameter boxes it into a heap cell; let parameters use frame slots

- Frame slots: a parameter only local references lend lives in the frame (`fa5a3b6de`).

### [#1145](https://github.com/HerringtonDarkholme/hd-lang/issues/1145) Moves: a generic `[T]` body copies a non-copyable argument

- Spec: the `Copy` marker, `impl !Copy`, `?Copy`, and copyable collections (`d882b0e6a`).

### [#1146](https://github.com/HerringtonDarkholme/hd-lang/issues/1146) Moves: `List[Holder]` copies, so indexing duplicates a `&mut`

- Spec: the `Copy` marker, `impl !Copy`, `?Copy`, and copyable collections (`d882b0e6a`).

### [#1153](https://github.com/HerringtonDarkholme/hd-lang/issues/1153) Perf: self.items[self.f()] copies the whole list before indexing

- Index through a pure call: self.items[self.pick()] reads in place (`782cca762`).

### [#1155](https://github.com/HerringtonDarkholme/hd-lang/issues/1155) hd help test says --release wraps overflow; it keeps checked overflow

- hd help test: --release keeps checked overflow (`21adac9e4`).

### [#1157](https://github.com/HerringtonDarkholme/hd-lang/issues/1157) lib/std/http.hd and lib/std/iter.hd are not hd fmt clean

- Server SDK: refresh the server schema for the std fmt pass (`22fb9e051`).

### [#1154](https://github.com/HerringtonDarkholme/hd-lang/issues/1154) A struct literal or variant does not check its type's bounds

- Check every declared bound at a struct literal or variant (`119dc1e2e`).

### [#1156](https://github.com/HerringtonDarkholme/hd-lang/issues/1156) Three examples/dogfood programs fail hd check with local-reference-escapes

- Dogfood examples: box what outlives its expression (`530e143ad`).

### [#1158](https://github.com/HerringtonDarkholme/hd-lang/issues/1158) ?Copy on a local function's type parameter is unsupported

- ?Copy on a local function's type parameter (`479cda1a6`).

### [#1159](https://github.com/HerringtonDarkholme/hd-lang/issues/1159) A too-deep trait proof in a method lookup reports unsupported

- Report trait-resolution-depth for a too-deep method lookup (`d0f1d5843`).

### [#1163](https://github.com/HerringtonDarkholme/hd-lang/issues/1163) A closure for an Fn-bounded parameter does not take the bound's row

- A value for an Fn-bounded parameter takes the bound's row (`44b8c1569`).

### [#1165](https://github.com/HerringtonDarkholme/hd-lang/issues/1165) hd fmt collapses a closure with a trailing-block body into code its parser rejects

- hd fmt: a trailing callback block keeps its enclosing suite (`8b79b7115`).

### [#1166](https://github.com/HerringtonDarkholme/hd-lang/issues/1166) hd fmt puts a lone comma on its own line after a multi-line closure argument

- hd fmt: no lone comma after a multi-line closure argument (`421893ebb`).

### [#1172](https://github.com/HerringtonDarkholme/hd-lang/issues/1172) The formatter's 3x parse budget test fails at random

- hd fmt: cut formatter time and measure the budget by fastest round (`bb4a1add8`).

### [#1173](https://github.com/HerringtonDarkholme/hd-lang/issues/1173) Release build fails on i32(*m) for a newtype read through a reference

- Promoted references keep their referent's newtype (`6f3512553`).

### [#1174](https://github.com/HerringtonDarkholme/hd-lang/issues/1174) move-out-of-place is not reported for *r or r.field as a function's tail value

- move-out-of-place at a function's tail value (`e32d6ff25`).

### [#1175](https://github.com/HerringtonDarkholme/hd-lang/issues/1175) move-out-of-place is not reported for a pattern that moves a payload out of *r

- move-out-of-place for a pattern that binds a payload of *r (`b52d819b6`).

### [#1177](https://github.com/HerringtonDarkholme/hd-lang/issues/1177) A chain of distinct closures builds in quadratic time and memory

- Mono: a closure's instance scans only its own sub-body (`0b36f1aef`).

### [#1161](https://github.com/HerringtonDarkholme/hd-lang/issues/1161) Inspectable.runtime_type takes self; make it &self like the other read traits

- Inspectable.runtime_type borrows its receiver: fn runtime_type(&self) (`9c02f1c20`).

### [#1178](https://github.com/HerringtonDarkholme/hd-lang/issues/1178) node_time_limit_cuts_a_socket_wait_after_flushing_output fails at random

- Node host: a time limit counts from the program's start (`ec745685b`).

### [#1171](https://github.com/HerringtonDarkholme/hd-lang/issues/1171) hd fmt lib/std again after the hugged-closure layout

- Server SDK: refresh the server schema after the std fmt pass (`a904cafa2`).

### [#1180](https://github.com/HerringtonDarkholme/hd-lang/issues/1180) Debug hd overflows its stack on a 16,000-link method chain (local_refs::walk)

- local_refs walks the syntax tree with a work stack (`ec0718a04`).

### [#1168](https://github.com/HerringtonDarkholme/hd-lang/issues/1168) Std callbacks that look at an item take fn(&T); add in-place retain and sort

- Std looking callbacks take fn(&T); List gains retain, sort, swap (`6f3b15785`).

### [#1179](https://github.com/HerringtonDarkholme/hd-lang/issues/1179) Matching through a reference binds a non-copyable payload by value; it should bind a reference

- Match through a reference binds a non-copied payload by reference (`c84e70adf`).

### [#1181](https://github.com/HerringtonDarkholme/hd-lang/issues/1181) Cut the web compiler's size: 17 KB left under the 2 MB gzip cap

- Web compiler: 14 crates at opt-level "z" in the web profile (`a04be7fb7`).

### [#1169](https://github.com/HerringtonDarkholme/hd-lang/issues/1169) Option gains as_ref, as_mut and copied

- Option gains as_ref and as_mut (`06e47e904`).

### [#1182](https://github.com/HerringtonDarkholme/hd-lang/issues/1182) &*dyn Error does not parse as a type; method calls should read through both

- & takes a counted reference type without parentheses: &*T (`e05f1b82b`).

### [#1164](https://github.com/HerringtonDarkholme/hd-lang/issues/1164) No in-language wait: a task cannot block until another task signals it

- std.task gains a bounded channel woken by hd code (`98bebbdf9`).

### [#1183](https://github.com/HerringtonDarkholme/hd-lang/issues/1183) Field access and method calls auto-dereference through every reference level

- Field access and method calls read through every reference level (`d7ef3ca64`).

### [#1184](https://github.com/HerringtonDarkholme/hd-lang/issues/1184) Rename all! to join!, with a tuple form and a list-spread form; map_all! to map_join!

- Rename all! to join!, with a list-spread form; map_all! to map_join! (`4db5f460c`).

### [#1167](https://github.com/HerringtonDarkholme/hd-lang/issues/1167) Read-only std methods take &self (len, is_empty, is_some, contains, ...)

- Read-only std methods take &self (len, is_empty, is_some, contains, ...) (`7f326edcd`).

### [#1186](https://github.com/HerringtonDarkholme/hd-lang/issues/1186) Remaining read-only std receivers take &self (Map, Json, Regex, Path, builders)

- Remaining read-only std receivers take &self (`e0d642701`).

### [#1193](https://github.com/HerringtonDarkholme/hd-lang/issues/1193) hd fmt breaks a long if in a struct field into code its parser rejects

- hd fmt keeps non-final value ifs flat in literals (`ca1026673`).

### [#1190](https://github.com/HerringtonDarkholme/hd-lang/issues/1190) hd test rejects a host-mapped package when the test file imports nothing from it

- Host maps resolve traits from package sources (`978e0f31f`).

### [#1191](https://github.com/HerringtonDarkholme/hd-lang/issues/1191) Package host traits cannot be bound by a native host, so no non-JavaScript renderer is possible

- Design: native package host bindings (accepted, not built) (`ee7e6382a`).

### [#1194](https://github.com/HerringtonDarkholme/hd-lang/issues/1194) A trailing block for an Fn-bounded parameter does not take the bound row

- Trailing Fn-bound blocks adopt the bound row (`c1291aaab`).

### [#1170](https://github.com/HerringtonDarkholme/hd-lang/issues/1170) Copy-on-write collections: accessors copy, no _ref or _mut forms

- Make collection accessors copy values (`b33476b12`).

### [#1196](https://github.com/HerringtonDarkholme/hd-lang/issues/1196) UniqList: a non-copyable list with no copy-on-write

- Add UniqList core collection (`5f761caa9`).

### [#1199](https://github.com/HerringtonDarkholme/hd-lang/issues/1199) UniqMap: a non-copyable map with no copy-on-write

- Add UniqMap core collection (`6cbd4a8f5`).

### [#1202](https://github.com/HerringtonDarkholme/hd-lang/issues/1202) Buffer item references: an intrinsic to lend an element of std.rt.memory.Buffer

- Lend Buffer items through references (`bfb973128`).

### [#1197](https://github.com/HerringtonDarkholme/hd-lang/issues/1197) UniqList: element references and borrowing iteration

- Add UniqList reference operations (`a752435ea`).

### [#1203](https://github.com/HerringtonDarkholme/hd-lang/issues/1203) A type-argument default is checked where it is used

- collect has no default target; _ infers a type argument in a body (`0cbec9c65`).

### [#1204](https://github.com/HerringtonDarkholme/hd-lang/issues/1204) UniqList and UniqMap move items one by one; UniqMap is slower than Map

- Move unique collection storage in bulk (`8e98476af`).

### [#1198](https://github.com/HerringtonDarkholme/hd-lang/issues/1198) UniqList: consuming iteration and conversions to and from List

- Support consuming UniqList iteration (`fe13a4863`).

### [#1208](https://github.com/HerringtonDarkholme/hd-lang/issues/1208) Measure string, List and Map operations against JS in release

- Use TypeScript for cost audit tools (`6aaf0b7b0`).

### [#1200](https://github.com/HerringtonDarkholme/hd-lang/issues/1200) UniqMap: value references and iteration

- Add UniqMap reference access and iteration (`c4bdfc19d`).

### [#1207](https://github.com/HerringtonDarkholme/hd-lang/issues/1207) Compile time is superlinear in a struct's field count: a 1,100-field struct takes 20 s to build

- Deduplicate local coalescing conflicts (`fac51f5c9`).

### [#1244](https://github.com/HerringtonDarkholme/hd-lang/issues/1244) Benchmarks for short-string keys and keyword matches

- Benchmark short string keys and keyword matches (`e53c59524`).

### [#1227](https://github.com/HerringtonDarkholme/hd-lang/issues/1227) Buffer's reading methods take `&self`

- Borrow Buffer metadata access (`25ef87cce`).

### [#1239](https://github.com/HerringtonDarkholme/hd-lang/issues/1239) Buffer[T] is !Copy; its writing methods take &mut self

- Make Buffer non-copyable; set_len takes &mut self (`d4e0ab564`).

### [#1241](https://github.com/HerringtonDarkholme/hd-lang/issues/1241) Inline std.text's per-character scalar helpers

- Text: inline measured scalar helpers (`394b30379`).

### [#1242](https://github.com/HerringtonDarkholme/hd-lang/issues/1242) String bucket hash reads whole words

- Hash: read string buckets a word at a time (`7dd734ba2`).

### [#1236](https://github.com/HerringtonDarkholme/hd-lang/issues/1236) Quote the failing argument in Display hints for any call, not only println

- Checker: quote call arguments in Display hints (`11d6f7e94`).

### [#1250](https://github.com/HerringtonDarkholme/hd-lang/issues/1250) Short string == compares words, not bytes

- Wasm: compare short strings by words (`81cab6f34`).

### [#1228](https://github.com/HerringtonDarkholme/hd-lang/issues/1228) Honor explicit `@inline` hints on std.rt functions

- Inline std.rt like the rest of std; no runtime hints (`41a20f30f`).

### [#1214](https://github.com/HerringtonDarkholme/hd-lang/issues/1214) Append in place for `s = s + x` on a uniquely held string

- Append unique strings in place (`262379bcc`).

### [#1263](https://github.com/HerringtonDarkholme/hd-lang/issues/1263) Float ** within 1 ULP with fdlibm's pow, as JS does

- Float **: port fdlibm pow, within 1 ULP instead of correctly rounded (`095844105`).

### [#1231](https://github.com/HerringtonDarkholme/hd-lang/issues/1231) Make `Hash.hash` generic over the hasher, as Rust's `Hash`

- Make Hash generic over the hasher (`90477754f`).

### [#1237](https://github.com/HerringtonDarkholme/hd-lang/issues/1237) Serde data boundaries inline by hint instead of by name

- Use inline hints for serde data boundaries (`0df4c9608`).

### [#1229](https://github.com/HerringtonDarkholme/hd-lang/issues/1229) Inlining a `&self` method adds counts on the receiver

- Borrow an inlined &self copy's receiver in place (`fe535909f`).

### [#1253](https://github.com/HerringtonDarkholme/hd-lang/issues/1253) Closures share a non-copyable capture's cell

- Share noncopyable closure captures (`92202420c`).

### [#1252](https://github.com/HerringtonDarkholme/hd-lang/issues/1252) Checked builds trap on any retain of a Buffer

- Checked builds trap on any retain of a buffer (`b8382fa46`).

### [#1254](https://github.com/HerringtonDarkholme/hd-lang/issues/1254) Move or borrow every remaining Buffer copy the tripwire finds

- Borrow a local outside its returned reference's window (`c7a4516cb`).

### [#1266](https://github.com/HerringtonDarkholme/hd-lang/issues/1266) Constant scalar list literals compile to static data

- Compile scalar list literals as static data (`ef0f98f7a`).

### [#1218](https://github.com/HerringtonDarkholme/hd-lang/issues/1218) Compare a substring without allocating it

- Compare substrings without a string temporary (`74900b562`).

### [#1230](https://github.com/HerringtonDarkholme/hd-lang/issues/1230) An element reference matched inside a loop iteration takes no count

- A loop's element loan that a match decides or a read lends takes no count (`d33f80f6a`).

### [#1255](https://github.com/HerringtonDarkholme/hd-lang/issues/1255) Cycle collector frees buffers inside unreachable objects

- Collect a buffer as part of the object that holds it (`6981a2816`).

### [#1279](https://github.com/HerringtonDarkholme/hd-lang/issues/1279) Every buffer release goes through its holder's glue

- Hand a dynamic call the count of each open value argument (`f4888ee93`).

### [#1269](https://github.com/HerringtonDarkholme/hd-lang/issues/1269) Write string concatenation kernel in hd

- Write bytes_concat in hd (`6be97ade4`).

### [#1189](https://github.com/HerringtonDarkholme/hd-lang/issues/1189) Template placement: the trait's module or a same-package re-export

- Template placement: the trait's module or a same-package re-export (`371e3d919`).

### [#1281](https://github.com/HerringtonDarkholme/hd-lang/issues/1281) Reference views over a non-copyable erased value never copy it

- Move a value through a reference view instead of copying it (`31ae4d8ee`).

### [#1272](https://github.com/HerringtonDarkholme/hd-lang/issues/1272) Write concat_all and bytes_append in hd

- Write concat_all in hd (`df208fe0a`).

### [#1283](https://github.com/HerringtonDarkholme/hd-lang/issues/1283) Moving a place while a reference into it is live is an error

- A move while a reference into the place is live is a conflicting access (`4ea8fbc1f`).
- Move a frame local's value at its last whole read in a branch (`68f8368e7`).

### [#1287](https://github.com/HerringtonDarkholme/hd-lang/issues/1287) A thunk lending a &mut open argument to a &self call leaks its view

- Release erased thunk argument views (`32379fc3f`).

### [#1285](https://github.com/HerringtonDarkholme/hd-lang/issues/1285) A raw write into a body's own new allocation is not a write its caller sees

- Summarize writes to local allocations (`6d73ce3c6`).

### [#1288](https://github.com/HerringtonDarkholme/hd-lang/issues/1288) A match through a reference copies its non-copyable scrutinee

- The ownership passes never copy a non-copyable value (`806a1b156`).

### [#1296](https://github.com/HerringtonDarkholme/hd-lang/issues/1296) The ownership pass never copies a non-copyable value

- The ownership passes never copy a non-copyable value (`806a1b156`).
- The copy verifier exempts no type (`30c4eb032`).

### [#1298](https://github.com/HerringtonDarkholme/hd-lang/issues/1298) Raw Buffer; UniqList owns its length and elements

- A Ptr lives on what it was read from, so element references keep their owner (`13a9ad8c6`).

### [#1310](https://github.com/HerringtonDarkholme/hd-lang/issues/1310) Moving a loop-local non-copyable value emits a forbidden copy

- Test loop-local unique lists moving each round; clear six matrix rows (`fcbc5b016`).

### [#1299](https://github.com/HerringtonDarkholme/hd-lang/issues/1299) An owner outlives every reference into it, cell locals too

- A cell local outlives what points into it, as a frame local does (`64b5ce2ed`).

### [#1320](https://github.com/HerringtonDarkholme/hd-lang/issues/1320) References crossing no *T take no count

- A UniqList element reference through a *T pins the room (`d360b0da1`).

### [#1328](https://github.com/HerringtonDarkholme/hd-lang/issues/1328) Instantiation of a doubling type hangs since std.rt.Drop (CI workspace job)

- Bound type formatting for failed monomorphization lookups (`156cba91c`).

### [#1329](https://github.com/HerringtonDarkholme/hd-lang/issues/1329) A field move after a local borrow becomes an illegal copy

- Move addressed fields after their overlapping borrows end (`50cc17266`).

### [#1335](https://github.com/HerringtonDarkholme/hd-lang/issues/1335) Checked build: http-form-query-writers fails since #1320

- Keep closure parameter cells alive through borrowed calls (`fe76ece3e`).

### [#1317](https://github.com/HerringtonDarkholme/hd-lang/issues/1317) Collections: basic API gaps against Rust and Go (List/Map remaining)

- Complete the basic UniqList and UniqMap APIs (`7d569cab9`).
- Add List and Map capacity, drain, split, search and bulk APIs (`0ae3679b6`).

### [#1325](https://github.com/HerringtonDarkholme/hd-lang/issues/1325) Inline Drop hooks run again after a value has moved

- Run an inline Drop hook once after a value moves (`f95fb7ba4`).

### [#1292](https://github.com/HerringtonDarkholme/hd-lang/issues/1292) UniqList.sort is an insertion sort: give it List's merge sort

- UniqList sorts with the shared stable driftsort (`e199795ca`).

### [#1324](https://github.com/HerringtonDarkholme/hd-lang/issues/1324) Fast StringBuilder: unique storage, pre-sizing, digits into the buffer

- Give StringBuilder unique bytes and write integers into caller room (`326d4b13f`).

### [#1341](https://github.com/HerringtonDarkholme/hd-lang/issues/1341) Inlining a field borrow into a local adds reference counts

- Give StringBuilder unique bytes and write integers into caller room (`326d4b13f`).

### [#1318](https://github.com/HerringtonDarkholme/hd-lang/issues/1318) Rank the causes of hd's gap to MoonBit across all workloads

- Measure MoonBit gaps with default-tier V8 (`9f9f58108`).

### [#1331](https://github.com/HerringtonDarkholme/hd-lang/issues/1331) Two findings from the pointer-loan matrix: optimized bang move, join! loan verdict

- Move a cell that lives between suspension points in a suspending body (`826db0eb4`).

### [#1343](https://github.com/HerringtonDarkholme/hd-lang/issues/1343) Local-reference closure arguments take unnecessary counts

- Let lifted closures borrow their reference parameters (`a74fd7895`).

### [#1327](https://github.com/HerringtonDarkholme/hd-lang/issues/1327) A UniqList element reference outlives the *T it was taken through

- Keep a reference's pointer root alive until the reference's last use (`b5bebf79c`).

### [#1346](https://github.com/HerringtonDarkholme/hd-lang/issues/1346) Small pointer-address helpers remain calls in hot loops

- Inline small reference readers while keeping their reference (`9ba405354`).

### [#1344](https://github.com/HerringtonDarkholme/hd-lang/issues/1344) Short byte appends pay bulk memory-copy overhead

- Use exact-width loads and stores for short byte copies (`a6ed076f4`).
- Use exact-width loads and stores for short byte copies (`01aaf3887`).

### [#1345](https://github.com/HerringtonDarkholme/hd-lang/issues/1345) Temporary mutable pointer loans keep redundant counts

- Stage case: an immediate element store takes no referent count (`ac711edc6`).

### [#1321](https://github.com/HerringtonDarkholme/hd-lang/issues/1321) A room is raw memory: drop its head

- A Buffer's room keeps no count; its head is only a string header (#1321) (`1cce86305`).

### [#1347](https://github.com/HerringtonDarkholme/hd-lang/issues/1347) Every bulk memory copy and fill takes the small-size path

- Lower short bulk copies and fills through shared helpers (`418afa806`).

### [#1289](https://github.com/HerringtonDarkholme/hd-lang/issues/1289) Release summaries: infer which by-value parameters a function only reads

- Infer borrowable parameters through recursive components (`0c85a2dc4`).

### [#1232](https://github.com/HerringtonDarkholme/hd-lang/issues/1232) Pass a by-value parameter borrowed when the callee never moves it

- Pass a recursive component's borrowed parameters with no count (`fd44950b0`).

### [#1322](https://github.com/HerringtonDarkholme/hd-lang/issues/1322) Drop without a box; IntoIter on Drop; checked rooms

- Audit raw buffer rooms in checked builds (`6975b34e8`).

### [#1286](https://github.com/HerringtonDarkholme/hd-lang/issues/1286) Re-measure Display.write through a builder in release

- Size value interpolations once and retain inlined constants (`85feb21e5`).

### [#1400](https://github.com/HerringtonDarkholme/hd-lang/issues/1400) Runtime bench: eight workloads for the features the bench misses

- Add runtime workloads for ownership, tasks and generic APIs (`71c332047`).

### [#1372](https://github.com/HerringtonDarkholme/hd-lang/issues/1372) No front removal from a list in a loop: UniqList consuming cursor, net test queues

- Consume unique lists and scripted queues without front shifts (`d791c9482`).

### [#1349](https://github.com/HerringtonDarkholme/hd-lang/issues/1349) Read traits and derives accept non-copyable types

- Borrow read-trait derivations for non-copyable values (`c74edcd5e`).
- Repair read-trait callers and newtype debug generation (`918a564ca`).
- Cover the borrowed Display declaration in the example index (`458494efb`).
- Measure memory growth before comparison helpers allocate (`1aa482c08`).
- Keep iterator allocation checks independent of Option equality (`8767d1143`).

### [#1401](https://github.com/HerringtonDarkholme/hd-lang/issues/1401) *T and &T impls are separate: an impl on a reference type covers only its declared kind

- An impl on a reference type covers only its declared kind (`20850b84f`).

### [#1348](https://github.com/HerringtonDarkholme/hd-lang/issues/1348) Map loops bind (&K, &V) and (&K, &mut V)

- Borrow real map entry slots in shared and mutable loops (`c38346c1c`).

### [#1403](https://github.com/HerringtonDarkholme/hd-lang/issues/1403) A call through a reference to a function value reads it

- Read a referenced function value before calling it (`1cb21b427`).

### [#1351](https://github.com/HerringtonDarkholme/hd-lang/issues/1351) Index returns a reference for element access; range indexing becomes a method

- Make indexing lend element places and move slicing to methods (`0b44dd70c`).
- Fix indexed temporary receivers and associated Copy checks (`70520300c`).

### [#1369](https://github.com/HerringtonDarkholme/hd-lang/issues/1369) String += through &mut appends in place

- Reuse string buffers when appending through references and cells (`d853d1109`).

### [#1374](https://github.com/HerringtonDarkholme/hd-lang/issues/1374) `xs[..]` copies a list element by element instead of sharing it

- Share full List slices and reserve partial selections once (`1db02a441`).

### [#1389](https://github.com/HerringtonDarkholme/hd-lang/issues/1389) A runtime type test compares type names as strings

- Compare runtime type identities through canonical descriptors (`04e8d380f`).

### [#1368](https://github.com/HerringtonDarkholme/hd-lang/issues/1368) Small std readers inline through a reference

- Inline small readers with traceable reference arguments (`b06b35078`).

### [#1378](https://github.com/HerringtonDarkholme/hd-lang/issues/1378) `a is b` walks views with `rt:ref_serial` for plain references

- Compare physical reference identities without walking views (`08ec78cd7`).

### [#1377](https://github.com/HerringtonDarkholme/hd-lang/issues/1377) `Option` equality and `partial_cmp` results are never a tag test

- Lower optional None and ordering comparisons to tag tests (`f35f489f5`).

### [#1361](https://github.com/HerringtonDarkholme/hd-lang/issues/1361) Checked builds report count pairs that could have been elided

- Report redundant count pairs in checked builds (`b0fa262e5`).
- Skip count-audit source work outside checked builds (`e6c4936d1`).

### [#1363](https://github.com/HerringtonDarkholme/hd-lang/issues/1363) CI tracks each bench program's dynamic count-operation total

- Gate benchmark count-operation totals in CI (`4574128b3`).

### [#1404](https://github.com/HerringtonDarkholme/hd-lang/issues/1404) Web compiler gzip is over the 2 MB hard cap

- Leave unused fixture tools and host glue out of the web compiler (`40b96c985`).

### [#1388](https://github.com/HerringtonDarkholme/hd-lang/issues/1388) Constant shared enum data lives in globals filled at startup

- Lower constant enum data to static lookup tables (`21ad77a60`).

### [#1405](https://github.com/HerringtonDarkholme/hd-lang/issues/1405) Optimized conformance aborts with a stack overflow on main

- Read initializer contracts without re-entering ownership (`5654a5a18`).

### [#1373](https://github.com/HerringtonDarkholme/hd-lang/issues/1373) `m[k] op= v` retains the key that `map_put` then releases

- Replace a found map value without counting its key (`8c3bb8f5d`).

### [#1384](https://github.com/HerringtonDarkholme/hd-lang/issues/1384) A default argument is a function call that takes earlier parameters owned

- Inline value defaults through the existing body copy (`5e6fe38ce`).

### [#1387](https://github.com/HerringtonDarkholme/hd-lang/issues/1387) Calling a closure value retains and releases it each time

- Borrow closure cells and immutable self from the environment (`b0e861f03`).

### [#1381](https://github.com/HerringtonDarkholme/hd-lang/issues/1381) A match arm with no counted payload still releases the value

- Remove drops on paths whose enum variant holds no count (`a8c67f0fa`).

### [#1356](https://github.com/HerringtonDarkholme/hd-lang/issues/1356) A &mut call stores other arguments only when its callee can

- Keep call argument roots only when their lifetimes can be stored (`08627377a`).

### [#1358](https://github.com/HerringtonDarkholme/hd-lang/issues/1358) Release inliner leaves one-match helpers like unwrap_or as calls

- Size small matches while preserving loan boundaries (`0bf876429`).
- Keep non-Copy inline arguments behind their references (`3a281b8ab`).

### [#1362](https://github.com/HerringtonDarkholme/hd-lang/issues/1362) Every emitted count operation carries its reason, std bodies included

- Inventory conservative count operations across benchmarks (`94da98a3d`).

### [#1391](https://github.com/HerringtonDarkholme/hd-lang/issues/1391) block_on of a known suspending call allocates a frame

- Drive known cold calls directly under block_on (`4f4eec238`).
- End cold argument loans before a fused first drive (`ea2dd3816`).

### [#1416](https://github.com/HerringtonDarkholme/hd-lang/issues/1416) #1399 step 31: CI reports conservative count sites and their number may only fall

- Gate conservative ownership sites by reason on CI (`53c0e57f6`).

### [#1417](https://github.com/HerringtonDarkholme/hd-lang/issues/1417) #1399 step 30a: copy reads' reasons are verified, and copy_reasons is linear

- Verify copy-read reasons in native checked builds (`cd67f62a5`).

### [#1418](https://github.com/HerringtonDarkholme/hd-lang/issues/1418) #1399 step 20b: short loans are a TIR fact

- Move short-loan decisions into a late TIR pass (`a31a286c3`).

### [#1419](https://github.com/HerringtonDarkholme/hd-lang/issues/1419) #1399 steps 21a and 21b: loan elision is a TIR pass; delete elide.rs

- Use TIR link-loan decisions and delete emitter elision (`770f032b0`).

### [#1342](https://github.com/HerringtonDarkholme/hd-lang/issues/1342) Node host: a time-limit panic can print twice

- Let the supervisor alone print program timeout diagnostics (`ebb03574e`).

### [#1429](https://github.com/HerringtonDarkholme/hd-lang/issues/1429) A local reference stored in a list slot does not keep its source alive

- A local reference stored in a collection slot keeps its source alive (`505bfbcff`).
- A local reference stored in a collection slot keeps its source alive (`df5b7d3c2`).

### [#1440](https://github.com/HerringtonDarkholme/hd-lang/issues/1440) A generic struct update shares a kept list in release builds

- A field shared by a copy-update or a suspending call keeps its check (`8a587a983`).

### [#1439](https://github.com/HerringtonDarkholme/hd-lang/issues/1439) A consumed borrow of a struct field is not attributed to that field

- A field shared by a copy-update or a suspending call keeps its check (`8a587a983`).

### [#1494](https://github.com/HerringtonDarkholme/hd-lang/issues/1494) Variance across a representation change reports type-mismatch

- Use a concrete reference in the function variance example (`b7330f064`).

### [#1495](https://github.com/HerringtonDarkholme/hd-lang/issues/1495) A newtype deriving through itself reports the wrong code and line

- Supply the base implementation in the newtype derive example (`3c6f8c699`).

### [#1496](https://github.com/HerringtonDarkholme/hd-lang/issues/1496) Overlapping derive and impl is reported on the derive line

- Locate the written implementation conflict at derive (`2a712d2bf`).

### [#1498](https://github.com/HerringtonDarkholme/hd-lang/issues/1498) A boundary type with a private field is accepted

- Classify the host boundary excerpt with its required context (`0ce5c85f9`).

### [#1499](https://github.com/HerringtonDarkholme/hd-lang/issues/1499) A reference to a range slice reports type-mismatch, not references-not-yet-checked

- Use the range-index diagnostic in the reference example (`c23cd0e62`).

### [#1493](https://github.com/HerringtonDarkholme/hd-lang/issues/1493) An empty map literal with no expected type is accepted

- Reject each empty collection without an expected type (`dd0cdec66`).
- Defer empty collection errors until inference finishes (`96d7fbb33`).

### [#1497](https://github.com/HerringtonDarkholme/hd-lang/issues/1497) A prelude-name shadow through use ... as is reported on the wrong line

- Report a prelude shadow at its imported alias (`455454cb1`).

### [#1506](https://github.com/HerringtonDarkholme/hd-lang/issues/1506) A list spread that needs an optional wrap with no expected type is unsupported

- Convert inferred list spreads after their element join closes (`6a32be626`).

### [#1507](https://github.com/HerringtonDarkholme/hd-lang/issues/1507) An unknown type in a return type adds a second cannot-infer-type error

- Absorb inference cascades from nested poisoned return types (`f6ba9530b`).

### [#1424](https://github.com/HerringtonDarkholme/hd-lang/issues/1424) #1399 steps 23a and 23b: emitter glue counts become TIR statements

- Delete six compact operations with no producer (`d1819edd0`).

### [#1512](https://github.com/HerringtonDarkholme/hd-lang/issues/1512) Recursive functions with omitted rows are refused: row inference needs a fixpoint

- Infer recursive requirement rows through function values (`8dab59039`).

### [#1522](https://github.com/HerringtonDarkholme/hd-lang/issues/1522) join! takes at most 31 children: a checker rule, not a backend refusal

- Check join's child limit before emitting its completion mask (`5b4ab1129`).

### [#1521](https://github.com/HerringtonDarkholme/hd-lang/issues/1521) Coverage check fails when covered code is deleted

- coverage --check: fail on an uncovered changed line, not on a falling ratio (`72e697223`).

### [#1529](https://github.com/HerringtonDarkholme/hd-lang/issues/1529) Omitted-row body retry is exponential in nested recursive local functions

- Solve local omitted rows as one lexical constraint group (`efa12ec8b`).

### [#1530](https://github.com/HerringtonDarkholme/hd-lang/issues/1530) hd check overflows its stack on a 400-function ring with omitted rows

- Queue unfinished row signatures without nested body checks (`1bc11a967`).

### [#1301](https://github.com/HerringtonDarkholme/hd-lang/issues/1301) List.sort: port driftsort

- Finish the shared stable driftsort kernel (`44d5cbad3`).

### [#1535](https://github.com/HerringtonDarkholme/hd-lang/issues/1535) drive.rs refuses vtable slots of another shape for host traits with supertraits

- Lower host supertrait drives after their parent vtable slots (`6ed27b66d`).
- Accept the optional checked audit in the host drive fixture (`37daa255e`).

### [#1534](https://github.com/HerringtonDarkholme/hd-lang/issues/1534) profile_tree refuses diamond host supertraits, not only cycles

- Rely on checked acyclic supertraits when lowering host diamonds (`5b1f62b91`).

### [#1545](https://github.com/HerringtonDarkholme/hd-lang/issues/1545) A field write through an associated enum payload is rejected

- Normalize associated projections along a written place (#1545) (`697434aaf`).

### [#1547](https://github.com/HerringtonDarkholme/hd-lang/issues/1547) A field write through a struct field declared `S::Item` is rejected

- Normalize associated projections along a written place (#1545) (`697434aaf`).

### [#1544](https://github.com/HerringtonDarkholme/hd-lang/issues/1544) Associated enum payloads cannot be borrowed through references

- Lay out an enum that holds a projection from its instance (#1544) (`b473967b3`).

### [#1546](https://github.com/HerringtonDarkholme/hd-lang/issues/1546) usize(value) on a map loop's value: 'usize is not a function'

- Name the argument of a numeric cast that is no number (#1546) (`4769fe2a4`).

### [#1543](https://github.com/HerringtonDarkholme/hd-lang/issues/1543) dbg refuses tuple arguments with a runtime List rest

- Print dbg tuple arguments with runtime List rests (`7d65ae2e1`).

### [#1548](https://github.com/HerringtonDarkholme/hd-lang/issues/1548) A struct with no fields in a host trait signature fails the build

- Cross a struct with no fields at the host boundary (`cb5b05240`).

### [#1551](https://github.com/HerringtonDarkholme/hd-lang/issues/1551) A returned host handle trait whose method returns the same trait fails the build

- Reserve a host handle's vtable before building its slots (`1f661ef1e`).

### [#1550](https://github.com/HerringtonDarkholme/hd-lang/issues/1550) A recursive data type in a host trait signature fails the build

- Cross a recursive type at the host boundary through one codec pair (`4ebf6b37c`).

### [#1553](https://github.com/HerringtonDarkholme/hd-lang/issues/1553) Vtable deduplication is a linear scan per request: quadratic in the number of vtables

- Index vtables and inlined-frame paths by content, not by scanning (`eef518ff4`).

### [#1549](https://github.com/HerringtonDarkholme/hd-lang/issues/1549) A `Map` in a host trait signature fails the build

- Cross a map at the host boundary as its count, then its entries (`d6ae48fb7`).

### [#1552](https://github.com/HerringtonDarkholme/hd-lang/issues/1552) A host trait signature with a type that cannot cross passes `hd check` and fails the build

- Reject a host signature with a type that cannot cross: boundary-not-safe (`675ea4e05`).

### [#1554](https://github.com/HerringtonDarkholme/hd-lang/issues/1554) Associated payload cycles bypass recursive-value-type

- Reject concrete value cycles through associated payloads (`2b7732175`).

### [#1566](https://github.com/HerringtonDarkholme/hd-lang/issues/1566) #1490: zero `unsupported` in hd_wasm_linear emit/closure.rs (1 left)

- Read moved-cell cleanup from its stated count packet (`373e9854a`).

### [#1557](https://github.com/HerringtonDarkholme/hd-lang/issues/1557) A tuple with a rest element is refused at the host boundary

- Cross a tuple with a rest element at the host boundary as its members (`0a8e2e1df`).

### [#1565](https://github.com/HerringtonDarkholme/hd-lang/issues/1565) #1490: zero `unsupported` in hd_wasm_linear emit/borrow.rs (2 left)

- Read the borrow check's root object slot and root read where they exist (#1490) (`56951fe46`).

### [#1555](https://github.com/HerringtonDarkholme/hd-lang/issues/1555) A counted reference to data in a host signature passes `hd check` and fails the build

- Cross a counted reference at the host boundary as its referent's tree (`3630a57e8`).

### [#1561](https://github.com/HerringtonDarkholme/hd-lang/issues/1561) #1490: zero `unsupported` in hd_wasm_linear emit/refs.rs (8 left)

- Require the actual one-word address in reference walks (`c59906069`).

### [#1563](https://github.com/HerringtonDarkholme/hd-lang/issues/1563) #1490: zero `unsupported` in hd_wasm_linear emit/ctx.rs (3 left)

- Consume required context providers and their release packets (`4dd0ebcf3`).

### [#1569](https://github.com/HerringtonDarkholme/hd-lang/issues/1569) A generic `@host` trait passes `hd check` and fails the build

- Reject generic host traits at their type parameters (`a60c9fa77`).

### [#1556](https://github.com/HerringtonDarkholme/hd-lang/issues/1556) A host method returning `never` passes `hd check` and fails the build

- Support never as a host method result (`8d06aaa0f`).

### [#1567](https://github.com/HerringtonDarkholme/hd-lang/issues/1567) #1490: zero `unsupported` in hd_wasm_linear drive.rs (1 left)

- Type vtable slots as helpers, so a drive slot always wraps one (`8336c1c92`).

### [#1562](https://github.com/HerringtonDarkholme/hd-lang/issues/1562) #1490: zero `unsupported` in hd_wasm_linear rt.rs (5 left)

- Read host provider keys and supertraits as the trait values they are (`81a7f6692`).

### [#1560](https://github.com/HerringtonDarkholme/hd-lang/issues/1560) #1490: zero `unsupported` in hd_wasm_linear emit.rs (16 left)

- Remove the remaining emitter contract refusals (`b8c8e5eb1`).
- State the emitter arm bound and existing fixture coverage (`b56d6972f`).

### [#1571](https://github.com/HerringtonDarkholme/hd-lang/issues/1571) A runtime profile's handle trait with supertraits fails the build

- Build a host handle's supertrait tree once, and assert boundary.rs's ruled-out cases (`112d82b64`).

### [#1572](https://github.com/HerringtonDarkholme/hd-lang/issues/1572) A newtype of a type that cannot cross passes `hd check` in a host signature

- Check a newtype's base in a host signature (`d7441f02e`).

### [#1541](https://github.com/HerringtonDarkholme/hd-lang/issues/1541) block_on stalls on a Promise-returning package host method

- Use JSPI only for Promise-backed package host waits (`98c857802`).
- Deliver deadline groups without consuming later I/O readiness (`d87822a45`).

### [#1570](https://github.com/HerringtonDarkholme/hd-lang/issues/1570) Opaque results nested in function types become POISON in codegen

- Preserve opaque facts in function constructor types (`bf5c5cd90`).

### [#1564](https://github.com/HerringtonDarkholme/hd-lang/issues/1564) #1490: zero `unsupported` in hd_wasm_linear layout.rs (2 left)

- Lay out verified fields directly and remove layout refusals (`360aa515d`).

### [#1574](https://github.com/HerringtonDarkholme/hd-lang/issues/1574) A provider context type in a host signature passes `hd check` and fails the build

- Reject a provider context in a host signature (`d55b7ec98`).

### [#1575](https://github.com/HerringtonDarkholme/hd-lang/issues/1575) A package host trait that inherits one generic trait at two argument lists passes `hd check` and fails the build

- Reject a host trait that reaches one trait at two argument lists (`6534e0bf5`).

### [#1576](https://github.com/HerringtonDarkholme/hd-lang/issues/1576) A host trait with an associated type passes `hd check` and fails the build

- Reject a host trait with an associated type (`41e6f0e0c`).

### [#1573](https://github.com/HerringtonDarkholme/hd-lang/issues/1573) A union type in a host signature passes `hd check` and fails the build

- Cross a union at the host boundary as the enum of its cases (`636688512`).

### [#1559](https://github.com/HerringtonDarkholme/hd-lang/issues/1559) #1490: zero `unsupported` in hd_wasm_linear boundary.rs (18 left)

- Cross a union at the host boundary as the enum of its cases (`636688512`).

### [#1585](https://github.com/HerringtonDarkholme/hd-lang/issues/1585) Read mandatory backend facts through shared total accessors

- Read backend ownership and place facts through total accessors (`c36b3f735`).

### [#1526](https://github.com/HerringtonDarkholme/hd-lang/issues/1526) std intrinsics used as function values reach the backend's unsupported arm

- Infer delayed function-value arguments before checking call rows (`25704f552`).

### [#1589](https://github.com/HerringtonDarkholme/hd-lang/issues/1589) REPL session builds discard requested instruction origins

- Honor requested instruction origins in REPL session modules (`56d26f2d2`).

### [#1587](https://github.com/HerringtonDarkholme/hd-lang/issues/1587) Call inference solves a type parameter to a bare `dyn Tr`, so bare trait values exist

- Never solve a type parameter to a bare dyn descriptor (#1442) (`35b55fa9f`).

### [#1588](https://github.com/HerringtonDarkholme/hd-lang/issues/1588) Least common type weakens references inside tuples, as assignment does

- Weaken a reference inside a tuple at the least common type (#1442) (`816fc4bd5`).

### [#1590](https://github.com/HerringtonDarkholme/hd-lang/issues/1590) `dbg` of a closure prints raw definition numbers for its nominal types

- Name a dbg closure's types as source writes them (#1442) (`69da94036`).

### [#1591](https://github.com/HerringtonDarkholme/hd-lang/issues/1591) hd_check DESIGN.md still lists value-to-trait-value coercions the checker no longer emits

- Describe the trait value coercions the checker emits (#1577) (`120dfc286`).

### [#1593](https://github.com/HerringtonDarkholme/hd-lang/issues/1593) Backend docs still describe implicit boxed recursive enums

- Describe recursive enums through their explicit reference links (`191c7279f`).

### [#1594](https://github.com/HerringtonDarkholme/hd-lang/issues/1594) Trait-value docs still describe bare values and getter closures

- Document trait references through their data and vtable pair (`78fe3e715`).

### [#1595](https://github.com/HerringtonDarkholme/hd-lang/issues/1595) Provider lowering docs use obsolete with syntax and lifetime claims

- Correct provider binding syntax and scope documentation (`d9896c44d`).

### [#1598](https://github.com/HerringtonDarkholme/hd-lang/issues/1598) Layout module comment incorrectly gives every enum a tag

- Describe tagged and niche-filled enum layouts (`a9c73d946`).

### [#1596](https://github.com/HerringtonDarkholme/hd-lang/issues/1596) Enum analysis comment incorrectly says data is held by reference

- Correct the enum-instance walk boundary comment (`9f0931b83`).

### [#1599](https://github.com/HerringtonDarkholme/hd-lang/issues/1599) Boundary docs still list supported types as Link refusals

- Document current boundary forms and codec ownership (`fd49d0dbf`).

### [#1600](https://github.com/HerringtonDarkholme/hd-lang/issues/1600) Reference address docs name removed Ref and Counted types

- Use current local and counted reference names in backend docs (`63bbefe74`).

### [#1601](https://github.com/HerringtonDarkholme/hd-lang/issues/1601) Backend inspection docs still store TypeId as two strings

- Document TypeId as its canonical descriptor address (`ccb6fc850`).

### [#1602](https://github.com/HerringtonDarkholme/hd-lang/issues/1602) A local function's `impl Trait` parameter is rejected as a misplaced result annotation

- Give a local function's impl parameter an anonymous generic (#1577) (`e3f9c54c9`).

### [#1604](https://github.com/HerringtonDarkholme/hd-lang/issues/1604) Compiler-tests skill description is invalid YAML

- Quote the compiler-tests skill description as YAML (`df34fd51c`).

### [#1603](https://github.com/HerringtonDarkholme/hd-lang/issues/1603) REPL fixture docs omit the supported WAT view

- Document both REPL fixture inspection views (`3ba26d9c0`).

### [#1605](https://github.com/HerringtonDarkholme/hd-lang/issues/1605) Write-hd recovery instructions reject supported local references

- Teach local-reference recovery in the write-hd skill (`cfc08ff3b`).

### [#1611](https://github.com/HerringtonDarkholme/hd-lang/issues/1611) Host char decoder accepts surrogates and values above U+10FFFF

- Validate Unicode scalar host results before narrowing (`c19d97f74`).

### [#1612](https://github.com/HerringtonDarkholme/hd-lang/issues/1612) A numeric cast of a numeric newtype is accepted though the spec makes it an error

- Reject a numeric cast of a numeric newtype (#1577) (`eb182e7a1`).

### [#1614](https://github.com/HerringtonDarkholme/hd-lang/issues/1614) A tuple vararg given too few values prints `(?)` as the found type

- Show an open literal inside a mismatched type at its default type (#1577) (`42f739fe8`).

### [#1615](https://github.com/HerringtonDarkholme/hd-lang/issues/1615) A REPL input can't read a private top-level binding of the package's src/lib.hd

- Let a REPL input read src/lib.hd's top-level bindings (#1577) (`8eabf475f`).

### [#1616](https://github.com/HerringtonDarkholme/hd-lang/issues/1616) A REPL session holding a `src/lib.hd` with a top-level binding cannot `use self.util`

- Keep an unread REPL module's placeholder skim out of its source record (#1577) (`95a05ad0a`).

### [#1617](https://github.com/HerringtonDarkholme/hd-lang/issues/1617) A tuple of the wrong arity reports its element and itself: two errors for one mistake

- Report a tuple of the wrong arity once, not once more per element (#1577) (`b7827fa58`).

### [#1618](https://github.com/HerringtonDarkholme/hd-lang/issues/1618) A message shows a one-element tuple type as `(i32)` and a rest-only tuple type as `(, List[i32]...)`

- Show a one-element tuple type with its trailing comma (#1578) (`ca947c041`).

### [#1619](https://github.com/HerringtonDarkholme/hd-lang/issues/1619) An `if` value without `else` reports the missing `else` and then a `()` mismatch: two errors for one mistake

- Report an `if` value without `else` once, not again as a `()` mismatch (#1577) (`cf8f68fd5`).

### [#1620](https://github.com/HerringtonDarkholme/hd-lang/issues/1620) `&T is &dyn Trait` is rejected although `T` implements the trait

- Let `is` compare a reference with a trait value reference to its type (#1577) (`45468e958`).

### [#1621](https://github.com/HerringtonDarkholme/hd-lang/issues/1621) `hd fmt` writes a const generic parameter as `N<const usize`

- Space a const generic parameter's `<` as a bound's (#1580) (`becf45cd1`).

### [#1622](https://github.com/HerringtonDarkholme/hd-lang/issues/1622) `Impls::All`'s doc says codegen uses it; only the solver's unit tests do

- Say that only the solver's tests read `Impls::All` (#1579) (`f9871a5d4`).

### [#1623](https://github.com/HerringtonDarkholme/hd-lang/issues/1623) The `debug(...)` hint for an interpolated collection names the wrong value

- Name the interpolated value in the `debug(...)` hint (#1577) (`c6dc4bcf8`).

### [#1625](https://github.com/HerringtonDarkholme/hd-lang/issues/1625) A shorthand field `P { x }` of a narrower number gets no conversion fix-it

- Offer the conversion fix-it on a shorthand field too (#1577) (`ee944f1e4`).

### [#1626](https://github.com/HerringtonDarkholme/hd-lang/issues/1626) The rest tuple count message says "takes 1 elements"

- Say "takes 1 element" in the rest tuple count message (#1577) (`56743127e`).

### [#1627](https://github.com/HerringtonDarkholme/hd-lang/issues/1627) The solver charges a body twice for an asked `Fn` goal

- Charge a supplied `Fn` goal once (#1579) (`7137389d7`).

### [#1628](https://github.com/HerringtonDarkholme/hd-lang/issues/1628) `static_row`'s `strip` closure strips nothing

- Drop `static_row`'s identity `strip` closure (#1579) (`5e86fc83b`).

### [#1629](https://github.com/HerringtonDarkholme/hd-lang/issues/1629) An item whose header and body both run out of fuel is reported twice

- Report an item whose header and body both run out of fuel once (#1577) (`124565d4f`).

### [#1630](https://github.com/HerringtonDarkholme/hd-lang/issues/1630) A local declaration that names its function's type parameter is `unsupported`

- Let a local type take its function's type parameters (#1578) (`0a77fdda6`).

### [#1633](https://github.com/HerringtonDarkholme/hd-lang/issues/1633) A field write on a binding an earlier error left unknown adds `unknown-field`

- Report no field write on a binding an error left unknown (#1577) (`97967b8b3`).

### [#1634](https://github.com/HerringtonDarkholme/hd-lang/issues/1634) A row argument for a data literal's type parameter also fails the field it fills

- Poison a data literal's type argument of the wrong kind (#1577) (`67ba6fec5`).

### [#1635](https://github.com/HerringtonDarkholme/hd-lang/issues/1635) Type arguments written after a variant name are ignored

- Report type arguments written after a variant name (#1577) (`1f7496837`).

### [#1636](https://github.com/HerringtonDarkholme/hd-lang/issues/1636) `*` on a value says "this is a i32"

- Name the type in the `*` on a value message without an article (#1577) (`949338ea6`).

### [#1637](https://github.com/HerringtonDarkholme/hd-lang/issues/1637) A negated literal in an unsigned const slot says it "has type `usize`, but `i64` is expected"

- Say a negated literal does not fit an unsigned const slot (#1578) (`c36fe9543`).

### [#1631](https://github.com/HerringtonDarkholme/hd-lang/issues/1631) Coverage counts always-on assertion checks as program code

- Classify always-on compiler assertions as invariants (`c4de8ad52`).

### [#1638](https://github.com/HerringtonDarkholme/hd-lang/issues/1638) A misspelled `@auto_closing` adds a follow-up `unregistered-auto-closing` error at its result

- A misspelled `@auto_closing` still marks its function (#1578) (`fd5d6cee2`).

### [#1641](https://github.com/HerringtonDarkholme/hd-lang/issues/1641) The checker's `VerifyFacts` methods answer questions the TIR verifier never asks of a checked body

- Remove unused checker fact queries from TIR verification (`11f1b1036`).

### [#1639](https://github.com/HerringtonDarkholme/hd-lang/issues/1639) A union written as a type argument, `List[i64 | bool]`, is read as a const expression

- Read a `|` chain of names in a generic argument as a union type (#1580) (`2fe53e3d9`).

### [#1642](https://github.com/HerringtonDarkholme/hd-lang/issues/1642) A row argument in a const slot is reported as filling a type parameter

- Name a const slot's own kind when a row fills it (#1578) (`940aec0a8`).

### [#1643](https://github.com/HerringtonDarkholme/hd-lang/issues/1643) A const expression in a row alias's row slot is reported twice

- Report a wrong-kind row alias argument once (#1578) (`2cf0f013f`).
- Check a row slot's kind before lowering a bare name in it (#1578) (`caaec4ec1`).

### [#1608](https://github.com/HerringtonDarkholme/hd-lang/issues/1608) A generic local function that builds a struct over its own type parameter panics in wasm layout

- Scope loan discovery while preserving captured pointer loans (`de29083d0`).

### [#1644](https://github.com/HerringtonDarkholme/hd-lang/issues/1644) References into temporary lists skip bounds checks

- Check temporary-list indices before taking element references (`f1f3b738b`).

### [#1640](https://github.com/HerringtonDarkholme/hd-lang/issues/1640) `Types::display` is a debug printer whose compound arms only invariant-break messages reach

- Remove the unused recursive debug type printer (`ac9129ce7`).

### [#1649](https://github.com/HerringtonDarkholme/hd-lang/issues/1649) A const expression in a body's type or row slot reads differently from the same error in a signature

- Word a body's kind error as a signature's (#1577) (`d588f5a13`).

### [#1648](https://github.com/HerringtonDarkholme/hd-lang/issues/1648) `overlapping-impl` names each impl as space-separated tokens

- Spell an impl's path as hd fmt writes its header (#1578) (`a43711e66`).

### [#1647](https://github.com/HerringtonDarkholme/hd-lang/issues/1647) `Layout::SuiteEnd` is documented as a layout token but the cursor never makes one

- Delete the layout token no cursor makes, and assert every node finishes (#1580) (`5e9c462ab`).

### [#1646](https://github.com/HerringtonDarkholme/hd-lang/issues/1646) Opaque results inside union members fail during collection

- Reveal opaque results through positional union members (`4ed66f717`).

### [#1624](https://github.com/HerringtonDarkholme/hd-lang/issues/1624) A program using an impl whose head binds a parameter inside a union or a tuple rest is `unsupported`

- Instantiate union and tuple-rest implementation parameters (`1c3f29ac3`).

### [#1650](https://github.com/HerringtonDarkholme/hd-lang/issues/1650) Remove unused Sub display formatter after emitter body dispatch

- Use the selected body index for emitted function names (`a5f7f2ad7`).

### [#1651](https://github.com/HerringtonDarkholme/hd-lang/issues/1651) Remove unused instance-pass execution callbacks

- Remove unused instance-pass execution callbacks (`58438068b`).

### [#1592](https://github.com/HerringtonDarkholme/hd-lang/issues/1592) `Coercion::ToTraitValue` and `Coercion::ToAny` have no producer, but the backend still handles them

- Remove producerless trait-value coercions (`c1ddcd589`).

### [#1597](https://github.com/HerringtonDarkholme/hd-lang/issues/1597) An f32 literal compiles to f64.const plus f32.demote_f64 instead of one f32.const

- Emit f32 literals with a direct f32.const (`0f591b17e`).

### [#1652](https://github.com/HerringtonDarkholme/hd-lang/issues/1652) An argument-count error for one parameter says "takes 1 arguments"

- Word an argument count of one as "1 argument" (#1577) (`ccf84439e`).

### [#1657](https://github.com/HerringtonDarkholme/hd-lang/issues/1657) Stale doc comments sit on call.rs's registration `impl` block

- Move call.rs's stray doc comments to their function or delete them (#1577) (`1a9d5bc14`).

### [#1653](https://github.com/HerringtonDarkholme/hd-lang/issues/1653) An unknown variant through an enum alias reports `type-used-as-value`

- Report an unknown variant through an enum alias as the enum does (#1577) (`b901a2a35`).

### [#1654](https://github.com/HerringtonDarkholme/hd-lang/issues/1654) A type applied to a row argument is not `Inspectable`, so `@error` on it fails

- Let a type applied to a row argument be Inspectable (#1579) (`5113e55ed`).

### [#1655](https://github.com/HerringtonDarkholme/hd-lang/issues/1655) A unary operator on a wrong-typed const operand says "has type `bool`, but `bool` is expected"

- Name the operator when a const unary operand has the wrong type (#1578) (`a3ae83a98`).

### [#1656](https://github.com/HerringtonDarkholme/hd-lang/issues/1656) A body type accepts a primitive or a type parameter where a trait goes

- Reject a primitive or a type parameter where a body type names a trait (#1577) (`403b1e1f1`).

### [#1659](https://github.com/HerringtonDarkholme/hd-lang/issues/1659) The `it` test form names no error for extra arguments or a second body

- State the errors of an it call with no body, an extra argument or two bodies (#1577) (`e6c240ba7`).

### [#1660](https://github.com/HerringtonDarkholme/hd-lang/issues/1660) A tuple pattern of the wrong arity is `pattern-arity` in a match but `type-mismatch` in a let

- Report a tuple pattern of another arity as type-mismatch in every position (#1577) (`d84dadc6a`).

### [#1645](https://github.com/HerringtonDarkholme/hd-lang/issues/1645) A reference into a temporary map outlives its freed buffer

- Keep temporary owners alive through local-reference uses (`6f1cc45bc`).

### [#1658](https://github.com/HerringtonDarkholme/hd-lang/issues/1658) Forwarding a suspension loses its held local origins

- Preserve held origins when forwarding suspensions (`5236d8f94`).

### [#1613](https://github.com/HerringtonDarkholme/hd-lang/issues/1613) A plain `join(...)` builds a cold `Suspend`, as `race(...)` does (owner decision)

- Build cold suspensions for plain tuple and list joins (`b1ed0a3ee`).

### [#1663](https://github.com/HerringtonDarkholme/hd-lang/issues/1663) Opaque local-function facts scan sub-body roots quadratically

- Index opaque closure facts by their existing sub-body identity (`ce2c76eda`).

### [#1665](https://github.com/HerringtonDarkholme/hd-lang/issues/1665) A map literal keyed by an unbounded type parameter skips the key bound

- Check a map literal's key bound for a type parameter too (#1577) (`4135ec185`).

### [#1664](https://github.com/HerringtonDarkholme/hd-lang/issues/1664) A row-generic method called from a row-generic function traps at run time

- Forward inherited method rows through the existing context ABI (`466e87ba9`).

### [#1668](https://github.com/HerringtonDarkholme/hd-lang/issues/1668) Active runtime docs still describe deleted reference views

- Correct active descriptions of deleted reference views (`b87a0557c`).

### [#1662](https://github.com/HerringtonDarkholme/hd-lang/issues/1662) Distinguish implemented closure copies from the proposed chain schedule

- Distinguish implemented closure copies from the proposed chain schedule (`4563a0b20`).

### [#1670](https://github.com/HerringtonDarkholme/hd-lang/issues/1670) Print the erased open row by name in stage instances

- Print the open row by name in compiler presentation output (`4cc075a0a`).

### [#1661](https://github.com/HerringtonDarkholme/hd-lang/issues/1661) Rules still justified by reference views, which no longer exist

- Complete physical reference rules and count-reason facts (`fea1b9838`).

### [#1666](https://github.com/HerringtonDarkholme/hd-lang/issues/1666) A supertrait chain longer than 17 traits loses the root trait's methods and associated types

- Cut supertrait cycles in the resolver and walk supertraits without a depth cap (`6193851b7`).

### [#1667](https://github.com/HerringtonDarkholme/hd-lang/issues/1667) A chain of more than 64 named constants is reported as a binding that refers to itself

- Evaluate named-constant chains of any length, with a visiting set for cycles (`57e2fe28f`).

### [#1669](https://github.com/HerringtonDarkholme/hd-lang/issues/1669) Compiler test skill forbids required checked conformance

- Require focused dev and checked conformance in the compiler-tests skill (`8c9e91a9c`).

### [#1409](https://github.com/HerringtonDarkholme/hd-lang/issues/1409) std: char.to_u32 gives a char's scalar value

- Add char.to_u32, a char's Unicode scalar value (`96122a9f7`).

### [#1408](https://github.com/HerringtonDarkholme/hd-lang/issues/1408) std: search for a byte sequence in a Span

- Add Span.index_of_span, a search for an element sequence in a span (`0a7ec9248`).

### [#1671](https://github.com/HerringtonDarkholme/hd-lang/issues/1671) A named constant's initializer reads names in the use's scope

- Read a named constant's initializer in its module's top-level scope (`f8bea9508`).

### [#1451](https://github.com/HerringtonDarkholme/hd-lang/issues/1451) A constant passed to an owned string parameter is retained by fallback

- Show that a static string argument needs no count (`80961b38f`).

### [#1452](https://github.com/HerringtonDarkholme/hd-lang/issues/1452) Weak.upgrade's retain has no TIR statement

- State both weak upgrade retains in ownership IR (`57779553a`).

### [#1453](https://github.com/HerringtonDarkholme/hd-lang/issues/1453) Classify the 27 unclassified conservative Call count sites

- State counts for loaded dynamic receivers and map cursors (`fb8845b1a`).

### [#1672](https://github.com/HerringtonDarkholme/hd-lang/issues/1672) The tir and own views print a closure's operand as a raw number that moves when std grows

- Omit closure bookkeeping from TIR stage views (`daf3b06eb`).

### [#1674](https://github.com/HerringtonDarkholme/hd-lang/issues/1674) Release Wasm multiplies list indices by a constant 1 and keeps empty blocks

- Fold identity scaling and empty Wasm blocks (`c40ea434b`).

### [#1531](https://github.com/HerringtonDarkholme/hd-lang/issues/1531) Generated pathological-input scaling tests for every compiler stage

- Guard compiler scaling with generated work counters (`bd5059acd`).

### [#1632](https://github.com/HerringtonDarkholme/hd-lang/issues/1632) A local trait, or a local declaration naming an enclosing `Self`, local function's, row or const parameter, is `unsupported`

- A local impl names an enclosing parameter only through its target or trait (`f3b7f4b3e`).

### [#1528](https://github.com/HerringtonDarkholme/hd-lang/issues/1528) Integer and float literal classes stay open across statements, settled by inference

- Keep a literal class open across statements until its body settles (`d8e7fd385`).
- Keep a literal class open across statements until its body settles (`e56f96055`).
- Keep a literal class open across statements until its body settles (`fbb47899c`).

### [#1673](https://github.com/HerringtonDarkholme/hd-lang/issues/1673) Each named-constant read scans every top-level statement of the module

- Index a module's top-level bindings once for named-constant reads (`e7deab856`).

### [#1395](https://github.com/HerringtonDarkholme/hd-lang/issues/1395) A closure reading a captured &List copies the list

- Borrow projected reads through copied closure references (`8852c4fdb`).

### [#1678](https://github.com/HerringtonDarkholme/hd-lang/issues/1678) Long flat addition chains overflow the checker stack

- Check flat binary expression chains without stack growth (`f85703cb3`).

### [#1446](https://github.com/HerringtonDarkholme/hd-lang/issues/1446) Uniqueness checks don't hoist out of for loops or while-else

- Hoist uniqueness through loop exits and pattern bindings (`2c08996b9`).
- Keep loop alias checks at the collection write (`5f3182ade`).

### [#1386](https://github.com/HerringtonDarkholme/hd-lang/issues/1386) A store into a known-empty place still releases it

- Release branch-local counts on the arm that keeps them (`16fa34d53`).

### [#1413](https://github.com/HerringtonDarkholme/hd-lang/issues/1413) Explain conservative copy-read counts

- Transfer the frame reference count into a provider conversion (`aed0d6758`).

### [#1396](https://github.com/HerringtonDarkholme/hd-lang/issues/1396) A list element write re-checks uniqueness and bounds

- Reuse list checks across a mutable element call (`ddf76d2bc`).

### [#1511](https://github.com/HerringtonDarkholme/hd-lang/issues/1511) Generated structure records copy their lists once since #1443 (+0.1% size)

- Remove the unread witness from generated structure fields (`8e9ecfbb5`).

### [#1683](https://github.com/HerringtonDarkholme/hd-lang/issues/1683) Normalizing a projection stops silently at depth 32

- Report projection binding cycles and chains past 64 bindings (`93c2b8302`).

### [#1684](https://github.com/HerringtonDarkholme/hd-lang/issues/1684) Associated-type bounds are elaborated eagerly to depth 16

- Elaborate associated-type bounds per projection a body names (`e366ed837`).

### [#1447](https://github.com/HerringtonDarkholme/hd-lang/issues/1447) Shared instance bodies: TIR and the emitter disagree on argument passing

- Keep loan-mode references out of owned shared instances (`941fb42f6`).

### [#1685](https://github.com/HerringtonDarkholme/hd-lang/issues/1685) Wasm stack scheduling recounts a body for every edit

- Schedule Wasm local moves and store offsets in bounded walks (`93509231e`).

### [#1681](https://github.com/HerringtonDarkholme/hd-lang/issues/1681) State ABI copies of arguments still owned by the caller

- State ABI copies of caller-owned temporary arguments (`1b7bcee49`).

### [#1677](https://github.com/HerringtonDarkholme/hd-lang/issues/1677) A read-only parser field still copies its list before len

- End local loans after calls returning only counted references (`43a84ac01`).

### [#1682](https://github.com/HerringtonDarkholme/hd-lang/issues/1682) Interpolating a still-open literal class takes the generic integer formatter

- Keep a literal class open across statements until its body settles (`fbb47899c`).

### [#1382](https://github.com/HerringtonDarkholme/hd-lang/issues/1382) A `using` binding or `&mut []` default becomes a heap cell

- Place fresh default objects in their caller frame (`d5ca6bc8b`).

### [#1390](https://github.com/HerringtonDarkholme/hd-lang/issues/1390) Installing a fresh provider object retains and releases it

- Transfer final scoped provider owners into call contexts (`f806d60b3`).

### [#1687](https://github.com/HerringtonDarkholme/hd-lang/issues/1687) A binding in an associated type's bound is accepted and never used

- Read an associated type's bound bindings for a projection self type (`13114cc2a`).

### [#1688](https://github.com/HerringtonDarkholme/hd-lang/issues/1688) A written type's argument is not checked against its parameter's bound bindings

- Check a written type's argument against its parameter's bound bindings (`245e84bea`).

### [#1690](https://github.com/HerringtonDarkholme/hd-lang/issues/1690) A call's argument does not infer a parameter named through a projection (Pair[G, G::Item])

- Infer a call's type parameter named beside its projection in a parameter type (`f706c9e9f`).

### [#1680](https://github.com/HerringtonDarkholme/hd-lang/issues/1680) Release Wasm keeps void blocks with no branch target

- Remove unused void Wasm labels after lowering (`fb2a20f5f`).

### [#1691](https://github.com/HerringtonDarkholme/hd-lang/issues/1691) Owner question: may a non-Copy field move out through a counted reference (*T, *mut T)?

- Reject moving a field that is not copied out through a counted reference (`23c436e44`).

### [#1367](https://github.com/HerringtonDarkholme/hd-lang/issues/1367) A read at a value's last use moves: spread and closure capture

- Transfer named counted-reference arguments while preserving function variance (`20ad4016d`).

### [#1692](https://github.com/HerringtonDarkholme/hd-lang/issues/1692) Returning a varianced function value breaks inlining

- Preserve result type views when inlining function factories (`be78c405b`).

### [#1676](https://github.com/HerringtonDarkholme/hd-lang/issues/1676) Release keeps storage for a nonescaping scalar hasher

- Forward visible local-reference writes into scalar places (`24aa4e789`).

### [#1393](https://github.com/HerringtonDarkholme/hd-lang/issues/1393) A suspension point saves every local, not only live ones

- Save and restore suspension slots by their live state (`881ced801`).

### [#1380](https://github.com/HerringtonDarkholme/hd-lang/issues/1380) A loop over `&List` runs a uniqueness check per element

- Share loop pin proofs with write-effect analysis (`3edb8a405`).

### [#1359](https://github.com/HerringtonDarkholme/hd-lang/issues/1359) Bulk helpers: no exact-8 branch, one wrap check, overlap classes to 64 bytes

- Serve dynamic bulk copies by length class, with no length of its own (`a4952c7c9`).

### [#1353](https://github.com/HerringtonDarkholme/hd-lang/issues/1353) Hoist the copy-on-write uniqueness check out of loops for a list in memory

- Hoist a list's uniqueness check out of a loop when its local is in a frame slot (`73b6113ac`).

### [#1434](https://github.com/HerringtonDarkholme/hd-lang/issues/1434) list-sort is 7% slower with a count test in code that runs once

- Record why list-sort moves with the call-site count test (`8f9a1ec30`).

### [#1695](https://github.com/HerringtonDarkholme/hd-lang/issues/1695) Hoisting's borrow-across-entry check searches an unsorted list

- Sort hoist borrow facts before indexed loop-entry checks (`f8580944e`).

### [#1694](https://github.com/HerringtonDarkholme/hd-lang/issues/1694) progs/list-sort's TurboFan code is 3x slower than on 2026-10-29

- Time hd's bench runs with Node's compile cache for the TypeScript host (`f0224eaae`).

### [#1696](https://github.com/HerringtonDarkholme/hd-lang/issues/1696) Constant-length copies of 17 to 31 bytes still use memory.copy

- Inline constant bulk copies and fills of 17 to 31 bytes as the helper's last class (`354660405`).

### [#1697](https://github.com/HerringtonDarkholme/hd-lang/issues/1697) `hd run` on Node strips the host's TypeScript on every run

- hd_cli: embed the Node host as JavaScript stripped at build time (`8812a047f`).

### [#1699](https://github.com/HerringtonDarkholme/hd-lang/issues/1699) bench/wasmtime/run.sh points at a `compiler/` directory that no longer exists

- bench/wasmtime: validate every bench program and sample, time nothing (`991179085`).

### [#1702](https://github.com/HerringtonDarkholme/hd-lang/issues/1702) host/node.test.ts: "an I/O retry delay cannot hide readiness when another operation wakes" fails

- host: order delivery by when a result can be ready, not by probe retry (`4d9b2c2ad`).

### [#1700](https://github.com/HerringtonDarkholme/hd-lang/issues/1700) Checked auditing faults after a zero-call captured writer

- Keep a captured root alive until its dependent result's last use (`6bcac83d7`).

### [#1693](https://github.com/HerringtonDarkholme/hd-lang/issues/1693) Function variance over tuple inputs leaks counted references

- Borrow function-value inputs that hold a thin reference by value (`377b51296`).

### [#1508](https://github.com/HerringtonDarkholme/hd-lang/issues/1508) The clpack store re-lists a cache shard directory on every run

- Avoid scanning known under-budget private cache shards (`ab37f8e88`).

### [#1437](https://github.com/HerringtonDarkholme/hd-lang/issues/1437) Wasteful patterns in emitted WAT, from scripts/ir-audit.py

- hd_wasm: drop branches and empty elses that only fall through, then clean locals across them (`38237ec2c`).

### [#1689](https://github.com/HerringtonDarkholme/hd-lang/issues/1689) UniqList indexing is 2.7x slower than List: bounds checks and header loads stay in the loop

- hd_wasm: prove inlined index guards false from loop facts over unchanged headers (`ca21cf1f9`).

### [#1701](https://github.com/HerringtonDarkholme/hd-lang/issues/1701) hd_wasm: five coverage sites no program reaches (one is a dead no-runtime mode)

- Require backend state and preserve opaque host argument owners (`66dd978c2`).

### [#1354](https://github.com/HerringtonDarkholme/hd-lang/issues/1354) A &dyn reference counts nothing

- hd_wasm, hd_tir: a `&dyn` reference counts nothing (`81df19649`).

### [#1704](https://github.com/HerringtonDarkholme/hd-lang/issues/1704) Wasm cleanup can't see a runtime helper's arity before the helper is generated

- Declare runtime helper shapes before control cleanup (`0c47927e5`).

### [#1703](https://github.com/HerringtonDarkholme/hd-lang/issues/1703) A cold frame does not count a thin local reference under an enum discriminant

- Keep conditional local-reference owners in escaped cold frames (`78bc5dbae`).

### [#1394](https://github.com/HerringtonDarkholme/hd-lang/issues/1394) A row-polymorphic callback call allocates a provider context

- Inline scoped provider callbacks without allocating contexts (`eebcbe313`).

### [#1707](https://github.com/HerringtonDarkholme/hd-lang/issues/1707) A suspending body's local an awaited call borrows lives in a heap cell

- hd_tir, hd_wasm: an awaited call's reference argument lives in its caller's frame (`f6c5a5c43`).

### [#1355](https://github.com/HerringtonDarkholme/hd-lang/issues/1355) A suspending function's reference parameters count nothing

- hd_tir, hd_wasm: a cold frame only awaited in its body counts no referent (`64f3c30ce`).

### [#1568](https://github.com/HerringtonDarkholme/hd-lang/issues/1568) #1490: zero `unsupported` in hd_wasm module.rs (2 left)

- hd_wasm: number release glues first in the function table (`5e84d5f7f`).

### [#1710](https://github.com/HerringtonDarkholme/hd-lang/issues/1710) A relocated doc-test diagnostic keeps the doc program's end position

- Map a relocated doc-test diagnostic's end to its source file (`416708c38`).

### [#1708](https://github.com/HerringtonDarkholme/hd-lang/issues/1708) hd_wasm coverage fell below its baseline: cover or delete 51 uncovered ranges

- Cover Wasm guard and local-reference ownership paths (`c8aa10799`).

### [#1510](https://github.com/HerringtonDarkholme/hd-lang/issues/1510) Warm no-change run still does O(modules) stat, decode and lookups

- Replay a warm single-file run, check and test from one stored entry (`c4e40c4bf`).

### [#1709](https://github.com/HerringtonDarkholme/hd-lang/issues/1709) Map and UniqMap: foldhash-fast bucket hash with a seed per map

- Use foldhash with a seed per map (`0091fc1d9`).

### [#1713](https://github.com/HerringtonDarkholme/hd-lang/issues/1713) Index guards: prove a masked index i & (len - 1) below the length

- Prove masked List and UniqList indices below the length after a length test (`4c0be8644`).

### [#1711](https://github.com/HerringtonDarkholme/hd-lang/issues/1711) Map and UniqMap: a Swiss-style index over the dense entries

- Index maps with Swiss groups of 8 control bytes (`f13fdb35c`).

### [#1714](https://github.com/HerringtonDarkholme/hd-lang/issues/1714) hd check of a file under lib/std panics instead of reporting

- Check a single file in a directory named std against the embedded std (`effa6bd41`).

### [#1715](https://github.com/HerringtonDarkholme/hd-lang/issues/1715) hd FILE names a single file's diagnostics without its directory in text output

- Name a single file's diagnostics as written in text output too (`c51285a84`).

### [#1533](https://github.com/HerringtonDarkholme/hd-lang/issues/1533) ManualClock: should concurrent sleeps overlap in virtual time?

- Overlap ManualClock sleeps in virtual time (`39443906d`).

### [#1717](https://github.com/HerringtonDarkholme/hd-lang/issues/1717) TIR: one reading convention for a closure's sub_params

- Read closure parameters through one TIR accessor (`334b1ee1f`).

### [#1716](https://github.com/HerringtonDarkholme/hd-lang/issues/1716) Owner question: may a package be named std?

- Reject a package named std (`48d4394c4`).

### [#1718](https://github.com/HerringtonDarkholme/hd-lang/issues/1718) Owner question: should the spec name a text diagnostic's file path?

- Name a diagnostic's file by one rule for text and JSON output (`84cdf4e57`).

### [#1719](https://github.com/HerringtonDarkholme/hd-lang/issues/1719) Drop a program's own empty module init

- Leave out a program's own init that does nothing (`39ba29679`).

### [#1712](https://github.com/HerringtonDarkholme/hd-lang/issues/1712) Choose map seed scope from measured cost

- Keep per-map seeds after the eager-seed comparison (`c3b4ebdd1`).

### [#1706](https://github.com/HerringtonDarkholme/hd-lang/issues/1706) Hoist loads through a shared reference, or a local header, that a loop never writes

- hd_wasm: load loop-invariant header fields once before their loop (`7bed8863a`).

### [#1723](https://github.com/HerringtonDarkholme/hd-lang/issues/1723) A Drop hook's writes are missing from its caller's effect summary (use after free)

- Document and screen std Drop snapshot confinement (#1723) (`ea336129d`).

### [#1724](https://github.com/HerringtonDarkholme/hd-lang/issues/1724) hash_of: foldhash, deterministic per toolchain; FNV-1a as a named stable hasher

- hash_of: foldhash with one fixed seed, deterministic per toolchain; Fnv1a keeps FNV-1a (`87dbee658`).

### [#1705](https://github.com/HerringtonDarkholme/hd-lang/issues/1705) Constructing a local-reference payload leaks its counted source

- Borrow counted reference payloads through ordinary references (`536238f3d`).
- Test implicit lends across closure and cancellation actors (`766454b5b`).

### [#1210](https://github.com/HerringtonDarkholme/hd-lang/issues/1210) Decide: a call-site location primitive so dbg and assertions are plain hd

- dbg is ordinary @track_caller hd over one intrinsic, debug_texts (`22c7a9f47`).

### [#1379](https://github.com/HerringtonDarkholme/hd-lang/issues/1379) A read-only comparison marks the caller `writes` and keeps both operands

- Borrow tuple comparisons through complete call effects (#1379) (`bce674c3a`).
- Lend parameters used only by read-through projections (`8738cd86f`).

### [#1729](https://github.com/HerringtonDarkholme/hd-lang/issues/1729) Release keeps constant-condition branches and zero shifts after inlining

- Fold inlined constants through Wasm locals (`3b97ce3ee`).
- Preserve compact constant tees after propagation (`06cc136fe`).

### [#1376](https://github.com/HerringtonDarkholme/hd-lang/issues/1376) An argument passed to a function value is kept

- Borrow Copy inputs at function-value calls (`36bcecaca`).

### [#1730](https://github.com/HerringtonDarkholme/hd-lang/issues/1730) A trait value's local-reference type argument in a parameter type is not rejected

- Reject a trait value's local-reference type argument in any written type (`6cae819ef`).

### [#1313](https://github.com/HerringtonDarkholme/hd-lang/issues/1313) Release: delete loops whose body has no effect

- Delete finite empty induction loops after Wasm folding (`2edff7c7f`).

### [#1731](https://github.com/HerringtonDarkholme/hd-lang/issues/1731) Remove weak references entirely

- Delete weak references from the compiler and runtime (`cc94ff4e6`).

### [#1732](https://github.com/HerringtonDarkholme/hd-lang/issues/1732) Delete unique_mut, make_unique and the unique_test pass (no users after #1219)

- Delete unique_mut, make_unique and the unique_test pass (`be075fd8d`).

### [#1726](https://github.com/HerringtonDarkholme/hd-lang/issues/1726) Spec: std's raw Ptr writes never touch a place a live &T holds

- Spec: std's Ptr writes and raw stores never reach a live &T's referent (`8b3276272`).

### [#1720](https://github.com/HerringtonDarkholme/hd-lang/issues/1720) A path dependency named std is accepted

- A path dependency named std is an invalid manifest (`d1c0ed466`).

### [#1733](https://github.com/HerringtonDarkholme/hd-lang/issues/1733) An `impl Trait[&T]` parameter reports `reference-result-without-source` on a scalar result

- An impl parameter's bound records no reference slot of the signature (`8d4bc8d1a`).

### [#1728](https://github.com/HerringtonDarkholme/hd-lang/issues/1728) The snapshot_update host hooks capture backtraces nothing reads

- Drop the backtrace the snapshot_update hooks captured for nothing (`f6e960264`).

### [#1735](https://github.com/HerringtonDarkholme/hd-lang/issues/1735) A session extension re-emits its entry init for a reason that is gone

- Reuse keyed session entry initializers (`6dd534544`).

### [#1737](https://github.com/HerringtonDarkholme/hd-lang/issues/1737) A list's IsUnique is hoisted past a conditional break, copying a shared list the loop never writes

- Keep list uniqueness checks behind iteration exits (`8d928bfcf`).
- Delay loop uniqueness until the first reached write (`0b1d0b41f`).

### [#1734](https://github.com/HerringtonDarkholme/hd-lang/issues/1734) The guard proof treats a loop as leaving its block when it breaks out of itself

- Preserve guard proofs past loops with local exits (`9ff2cf868`).

### [#1725](https://github.com/HerringtonDarkholme/hd-lang/issues/1725) Emit && and || that feed a branch as branch chains (race find: 13 ms, and 55 ms at some addresses, on wasmtime)

- Emit && and || that feed a branch as branch chains (`fe587b7da`).

### [#1727](https://github.com/HerringtonDarkholme/hd-lang/issues/1727) A @track_caller function that never reads its call site still gets a record at every call

- Calls of a @track_caller function that reads no call site pass no record (`58d777860`).

### [#1736](https://github.com/HerringtonDarkholme/hd-lang/issues/1736) A trait bound with an unnamed &T is not rejected; an impl header's unnamed &T gets a fresh lifetime

- Spec and checker: Rust's rule for references in bounds and impl headers (`a92c35bea`).

### [#1738](https://github.com/HerringtonDarkholme/hd-lang/issues/1738) Span.index_of_span reloads self.items and needle.items every iteration

- Hoist invariant shared-reference field prefixes before loops (`8b6bfcbda`).

### [#1739](https://github.com/HerringtonDarkholme/hd-lang/issues/1739) A call through a bound `S < Store[&'a T]` stores a shorter-lived reference unchecked

- Checker: a bound's written lifetime holds at calls through it and at calls that instantiate it (`883dd56e6`).

### [#1741](https://github.com/HerringtonDarkholme/hd-lang/issues/1741) A call through an associated type's bound `type Item < Store[&'b T]` stores a shorter-lived reference unchecked

- Checker: an associated type's bound's written lifetime holds at calls through it and at calls that pass the projection (`6d0cb88e0`).

### [#1740](https://github.com/HerringtonDarkholme/hd-lang/issues/1740) Optimized build panics "writes after the passes" on a pure function that stores through a bound into its own binding

- Ownership effects: a copy of a whole value points where the value does (`dc43da12d`).

### [#1743](https://github.com/HerringtonDarkholme/hd-lang/issues/1743) A nested projection over a trait with a lifetime argument is "local reference escapes to a type argument"

- Checker: a projection holds its bound's trait arguments at the top, as a generic instance does (`5deda1ab8`).

### [#1742](https://github.com/HerringtonDarkholme/hd-lang/issues/1742) Returning a projection over a trait with a lifetime argument is `local-reference-escapes`

- Checker: a projection-typed parameter returns as its projection, at the projection's lifetimes (`7507095fb`).

### [#1744](https://github.com/HerringtonDarkholme/hd-lang/issues/1744) A trait method's projection result elides to its receiver, so a result built from another parameter is rejected

- Checker: a projection result is at its projection's trait arguments, not the elided lifetime (`a14eb96d1`).

### [#1745](https://github.com/HerringtonDarkholme/hd-lang/issues/1745) A trait's default method returning `Self::Item` is never checked for local references

- Checker: a trait's own bodies and a bound's lifetime arguments pass at the caller's lifetimes (`d3acb0522`).

### [#1746](https://github.com/HerringtonDarkholme/hd-lang/issues/1746) A value with no slot of its own passed through a type parameter loses its origins

- Checker: a value with no slot of its own passes whole through a type parameter (`e8d536618`).

### [#1747](https://github.com/HerringtonDarkholme/hd-lang/issues/1747) An implementation can't return a value at a lifetime its trait type argument writes

- Checker: an implementation's trait arguments are at the lifetimes its header writes (`b7868123f`).

### [#1748](https://github.com/HerringtonDarkholme/hd-lang/issues/1748) A `&mut` to a value with no slot of its own is no place the caller owns

- Checker: a `&mut` place with no slot of its own is a place the caller owns (`e3ad09ae7`).

### [#1750](https://github.com/HerringtonDarkholme/hd-lang/issues/1750) A value of a bare type parameter holds no origins, so a bound's trait argument returns a local

- Checker: a call checks each argument it passes at a bound's lifetime where it is made (`d88b62e51`).

### [#1751](https://github.com/HerringtonDarkholme/hd-lang/issues/1751) Single-use Wasm forwarding repeatedly shifts and scans the body

- Wasm: forward single-use copies with indexed queries and one compaction (`737065ac3`).

### [#1065](https://github.com/HerringtonDarkholme/hd-lang/issues/1065) chat

- Wasm: forward single-use copies with indexed queries and one compaction (`737065ac3`).

### [#1753](https://github.com/HerringtonDarkholme/hd-lang/issues/1753) Carry visited nodes through the iterative call-graph walk

- Call graph: carry visit indices and component boundaries through DFS (`121a9a34b`).

### [#1523](https://github.com/HerringtonDarkholme/hd-lang/issues/1523) Drive sloppy code down: expect, std paths, catch-alls, unreachable, unwrap, panic, for-now

- Call graph: carry visit indices and component boundaries through DFS (`121a9a34b`).
- Ownership: retain drop-effect payloads in the matching enum arms (`c7c4af614`).
- Inliner: find held local references through the complete type shape (`fbbcc3025`).
- Mono: classify collections beyond the old struct-depth cap (`cb5f51840`).

### [#1755](https://github.com/HerringtonDarkholme/hd-lang/issues/1755) Carry the drop-effect variant through ownership classification

- Ownership: retain drop-effect payloads in the matching enum arms (`c7c4af614`).

### [#1754](https://github.com/HerringtonDarkholme/hd-lang/issues/1754) Inliner loses held local references after a depth cap and in sums

- Inliner: find held local references through the complete type shape (`fbbcc3025`).

### [#1752](https://github.com/HerringtonDarkholme/hd-lang/issues/1752) A function value's written lifetime is not checked at the call when the result holds no slot

- Checker: a call through a function value checks each argument at the lifetime its type writes (`33e81712f`).

### [#1756](https://github.com/HerringtonDarkholme/hd-lang/issues/1756) Collection classification silently stops after 16 struct layers

- Mono: classify collections beyond the old struct-depth cap (`cb5f51840`).

### [#1760](https://github.com/HerringtonDarkholme/hd-lang/issues/1760) A counted scan over a list checks bounds at every element

- Wasm: version counted loops with proven index ranges (`9f9919915`).

### [#1757](https://github.com/HerringtonDarkholme/hd-lang/issues/1757) A function value's written lifetime is checked only when the call reads the parameter itself

- Checker: a function value's written lifetime follows the value, not only a parameter read (`4e87a9764`).

### [#1759](https://github.com/HerringtonDarkholme/hd-lang/issues/1759) lib/std/collections.hd is not hd-fmt clean

- Std: every source is hd-fmt clean, and hd_fmt's std test keeps it so (`26d2b1499`).

### [#1758](https://github.com/HerringtonDarkholme/hd-lang/issues/1758) A function type whose result is a type parameter rejects a function value whose result depends on its input

- Types: a function type's type-parameter result may depend on the inputs that write a lifetime (`02c700386`).

### [#1761](https://github.com/HerringtonDarkholme/hd-lang/issues/1761) Deep reference holders silently lose their run-time loans

- Wasm: preserve loans in deeply nested reference holders (`5264fe177`).

### [#1772](https://github.com/HerringtonDarkholme/hd-lang/issues/1772) `Duration::seconds` wraps to a negative Duration in release builds

- Std: Duration unit constructors and addition check overflow in every build (`12407497e`).

### [#1773](https://github.com/HerringtonDarkholme/hd-lang/issues/1773) A method of `impl[T < Ord] List[T]` checks on `List[f64]`, then the build fails

- Checker: an inherent method's impl bounds hold for arguments inference decides later (`7ccbb4ea8`).

### [#1819](https://github.com/HerringtonDarkholme/hd-lang/issues/1819) hd_wasm's design says both that references count and that they count nothing

- Cleanup: align ownership records and parked design status (`96066855f`).

### [#1774](https://github.com/HerringtonDarkholme/hd-lang/issues/1774) A chain of private functions with omitted result types overflows the native stack

- Checker: an omitted result's body checks the pending bodies it reaches first, callees first (`e576b379d`).

### [#1776](https://github.com/HerringtonDarkholme/hd-lang/issues/1776) A chain of type aliases is quadratic, then overflows the stack

- Checker: each alias of a chain checks its own right-hand side, with an explicit stack (`63ac1a088`).

### [#1808](https://github.com/HerringtonDarkholme/hd-lang/issues/1808) The allocator's page search is quadratic in heap pages after the heap grows

- Allocator: retain failed-search proofs across heap growth (`e593585af`).

### [#1778](https://github.com/HerringtonDarkholme/hd-lang/issues/1778) Every block over 2 KiB takes whole 64 KiB pages, exhausting memory at 65,536 blocks

- Allocator: share medium blocks in four-page class spans (`143594118`).

### [#1810](https://github.com/HerringtonDarkholme/hd-lang/issues/1810) The TIR verifier runs on every body in release builds

- TIR: select checked-body verification once per compiler run (`94945c4ea`).

### [#1775](https://github.com/HerringtonDarkholme/hd-lang/issues/1775) Long binary-operator and method chains overflow the stack in check and fmt

- Parser: the browser build limits a chain to 500 links, below its engine's stack (`a73a4d924`).

### [#1812](https://github.com/HerringtonDarkholme/hd-lang/issues/1812) Short copies from immutable static data load the bytes at run time

- Fold short immutable static copies into constant stores (`7ccfd4d75`).

### [#1827](https://github.com/HerringtonDarkholme/hd-lang/issues/1827) A type with a long `?` suffix overflows the stack

- Parser: a type's `?` suffix counts against the chain limit (`16f76690c`).

### [#1814](https://github.com/HerringtonDarkholme/hd-lang/issues/1814) Taking a shared element reference runs a uniqueness check that may copy the list

- Skip copy-on-write splits through shared element loans (`09efc57bb`).

### [#1828](https://github.com/HerringtonDarkholme/hd-lang/issues/1828) The inline-containment walk is quadratic in a chain of structs

- Checker: the containment walk keeps its finite types across the header pass (`273c11077`).

### [#1829](https://github.com/HerringtonDarkholme/hd-lang/issues/1829) Building a program with a deeply nested value type is superpolynomial in hd_wasm::cycle

- Wasm: cache cycle summaries with an iterative SCC worklist (`932476967`).

### [#1509](https://github.com/HerringtonDarkholme/hd-lang/issues/1509) Server SDK publish re-inspects every retained output generation

- SDK: validate only active and destination output on publish (`398f4e386`).

### [#1833](https://github.com/HerringtonDarkholme/hd-lang/issues/1833) Mono: cover new reachable paths and remove dead fallback bodies

- Mono: test reachable analysis paths and remove dead fallbacks (`aef55c80f`).

### [#1818](https://github.com/HerringtonDarkholme/hd-lang/issues/1818) Code comments cite future-work docs that the docs consolidation moved or deleted

- Docs: point code citations at the consolidated design records (`95e520f13`).

### [#1816](https://github.com/HerringtonDarkholme/hd-lang/issues/1816) Collect prints Rust debug text when a bound fails at selection

- Mono: report missed trait selection as a located compiler bug (`64da7729c`).

### [#1831](https://github.com/HerringtonDarkholme/hd-lang/issues/1831) Storing a function-typed parameter whose type writes a lifetime reports a captured local reference

- Lifetimes: a fn parameter stored in a field names why; a store through a reference to a local keeps it (`018c177c6`).

### [#1823](https://github.com/HerringtonDarkholme/hd-lang/issues/1823) Mono: the instantiation chain limit stops a plain call chain of 257 functions

- Mono: count only generic requests in the instantiation limit (`88b2c67f6`).

### [#1834](https://github.com/HerringtonDarkholme/hd-lang/issues/1834) A projection past PROJECTION_DEPTH reports two or three errors for one cause

- Checker: a type past a depth limit reports one trait-resolution-depth per use (`7ac7cea07`).

### [#1835](https://github.com/HerringtonDarkholme/hd-lang/issues/1835) Returning a function value read through a reference to a struct reports a false dependent result

- Lifetimes: a field's written function type gives a read no captured origins (`3e43d75c9`).

### [#1811](https://github.com/HerringtonDarkholme/hd-lang/issues/1811) A list's capacity ignores its block's slack, so it reallocates inside one page

- Lists use all allocator-provided buffer capacity (`a1918ce1a`).

### [#1836](https://github.com/HerringtonDarkholme/hd-lang/issues/1836) One syntax error stops every check of its module and folder

- Checker: a syntax error stops only the top-level item that holds it (`ed2e295ed`).

### [#1838](https://github.com/HerringtonDarkholme/hd-lang/issues/1838) A depth error inside an instantiation-choice trial is reported as "no instantiation fits"

- Checker: a depth error in an instantiation trial is the call's one error (`8560bcf11`).

### [#1824](https://github.com/HerringtonDarkholme/hd-lang/issues/1824) Checker: a long cycle of omitted results still nests body checks before its error

- Checker: a body never checks another inside its own check (`423dd883b`).

### [#1837](https://github.com/HerringtonDarkholme/hd-lang/issues/1837) Inlining a long call chain overflows the compiler stack

- Inline callees with a resumable work stack (`1c626b9bb`).

### [#1841](https://github.com/HerringtonDarkholme/hd-lang/issues/1841) 23:00 cleanup: retire finished artifacts and refresh records

- Cleanup: goals.md says a written release module carries hd.positions (`4918a4235`).

### [#1842](https://github.com/HerringtonDarkholme/hd-lang/issues/1842) rt_tests/scan_tests.hd exits at run time on one CI run and passes on the next, same commit

- Use Node 26 with corrected Wasm wrapper lifetimes (`338d31f87`).
- Use corrected Node 26 wrapper lifetimes with portable driver modes (`1b7863215`).

### [#1765](https://github.com/HerringtonDarkholme/hd-lang/issues/1765) `fn(T) -> T` instantiated with a reference rejects a function whose result depends on its input

- Types: an instance of `fn(T) -> T` shares `T`'s lifetimes between input and result (`928a54bc1`).

### [#1840](https://github.com/HerringtonDarkholme/hd-lang/issues/1840) Named calls: a type parameter's result positions depend on its input positions one to one

- Spec: a generic call's result position depends on its type argument's same position (`92fd01f4a`).

### [#1844](https://github.com/HerringtonDarkholme/hd-lang/issues/1844) The waiting-body runtime fixture never makes a body wait

- Conformance: the waiting-body fixture names a later unannotated binding, so its chain waits (`848d65ea9`).

### [#1832](https://github.com/HerringtonDarkholme/hd-lang/issues/1832) Reachability of a flat tuple or struct match is quadratic in its arms

- Checker: a match's reachability and coverage are one usefulness pass over its matrix (`58d6f8bfa`).

### [#1825](https://github.com/HerringtonDarkholme/hd-lang/issues/1825) Checker: a chain of public aliases is quadratic

- Checker: a public alias chain's interface and signature walks keep each type's answer (`9bfb77e32`).

### [#1847](https://github.com/HerringtonDarkholme/hd-lang/issues/1847) A private type inside a union in a public signature does not leak-check

- Checker: a public signature's leak walk reads union members, through the one type-parts walk (`a555fac13`).

### [#1845](https://github.com/HerringtonDarkholme/hd-lang/issues/1845) A flat literal match's decision tree nests one test per arm, quadratic and recursive

- Checker: a match's decision tree is built from a work stack, and a literal column is partitioned by value once (`cbcded8f1`).

### [#1846](https://github.com/HerringtonDarkholme/hd-lang/issues/1846) Locating each diagnostic scans its file from the start

- Driver: a diagnostic's line and column come from one line index per file (`e51b4e99e`).

### [#1767](https://github.com/HerringtonDarkholme/hd-lang/issues/1767) Deep value constructions silently discard reference-loan origins

- fix(wasm): index and propagate holder origins without depth limits (`d1ecff7a8`).

### [#1768](https://github.com/HerringtonDarkholme/hd-lang/issues/1768) pathological iterator-chain: grow chains of 32 stages, not one longer chain

- Scale pathological iterator work by fixed-depth chains (`e8a546cde`).

### [#1850](https://github.com/HerringtonDarkholme/hd-lang/issues/1850) Two more line-and-column helpers scan from the file start per position

- Locate tracked call sites and fmt syntax errors through one line index per file (`ed117f9e4`).

### [#1849](https://github.com/HerringtonDarkholme/hd-lang/issues/1849) A literal column headed by a range tests one row per step over every row left

- Split an integer match column's values and ranges once, not one range test per row (`abb2dcae3`).

### [#1855](https://github.com/HerringtonDarkholme/hd-lang/issues/1855) Enum boxing rebuilds overlapping declaration graphs for each type

- Memoize enum boxing across declaration and type graphs (`4bd70606b`).

### [#1763](https://github.com/HerringtonDarkholme/hd-lang/issues/1763) println makes one host write per line

- Make println write its text and newline in one synchronous call (`8b915b50e`).

### [#1848](https://github.com/HerringtonDarkholme/hd-lang/issues/1848) A flat literal match's TIR is an If chain one level per arm, and a later stage overflows at 50,000 arms

- Emit deep conditional chains with heap continuations (`581de2b39`).

### [#1853](https://github.com/HerringtonDarkholme/hd-lang/issues/1853) `hd fmt` joins a from-range pattern to its guard: `20.. if g` becomes `20..if g`

- hd fmt: keep a space before a match guard's `if` after a from-range pattern (`c4a9cbb55`).

### [#1852](https://github.com/HerringtonDarkholme/hd-lang/issues/1852) A negative float literal pattern is opaque, so a match on them is tested row by row and repeats are not reported

- Checker: read a negative float literal pattern as its value (`cd68ff05a`).

### [#1854](https://github.com/HerringtonDarkholme/hd-lang/issues/1854) A declaration's 65,535th lifetime parameter and every one after share one number

- Resolver: a named limit of 1,024 generic parameters in scope, reported past it (`382e2b883`).

### [#1858](https://github.com/HerringtonDarkholme/hd-lang/issues/1858) Checker error paths return `never` instead of the poison type

- Return poison, not never, from the checker's error paths (`83b74566a`).

### [#1859](https://github.com/HerringtonDarkholme/hd-lang/issues/1859) An unknown-variant arm does not poison its match, so errors cascade

- Reject a match arm whose pattern names an unknown variant or field (`90300c8c0`).

### [#1860](https://github.com/HerringtonDarkholme/hd-lang/issues/1860) An unknown type in a signature cascades into the body and the trait check

- Keep a signature's unresolved type from cascading into bodies and impls (`d73a9b675`).

### [#1863](https://github.com/HerringtonDarkholme/hd-lang/issues/1863) Two bad arms in one match report the same root cause twice

- Checker: arms of one match naming unknown variants report once (`abb6acbc0`).

### [#1861](https://github.com/HerringtonDarkholme/hd-lang/issues/1861) One diagnostic can print a 5.2 MB type

- Diagnostics: a type a message names is cut at 100 bytes and 8 levels (`2d40e0572`).

### [#1839](https://github.com/HerringtonDarkholme/hd-lang/issues/1839) Checker: `solve_ready` recomputes every waiting row's blocked state per settle

- Checker: a settle solves omitted rows by searching from what changed (`28be47d08`).

### [#1857](https://github.com/HerringtonDarkholme/hd-lang/issues/1857) A generic parameter's name is found by a linear scan of the parameters in scope, per use

- Resolver: a generic parameter's name is found through an index, not a scan of the scope (`7f57fd116`).

### [#1851](https://github.com/HerringtonDarkholme/hd-lang/issues/1851) `hd check --fix` tests each taken fix-it's edits against every fix-it taken before it

- CLI: `--fix` tests a fix-it's edits against the taken ranges around them, not every one taken (`c157e2c54`).

### [#1856](https://github.com/HerringtonDarkholme/hd-lang/issues/1856) Cached count walks copy every descendant at each nested type

- Share count recipes without allocating scalar-only cache scopes (`7f4d9c873`).

### [#1817](https://github.com/HerringtonDarkholme/hd-lang/issues/1817) `impl[N < Integer] N` comment says other std code never calls the helpers

- Remove the stale claim that std avoids generic integer helpers (`13f94845f`).

### [#1815](https://github.com/HerringtonDarkholme/hd-lang/issues/1815) ManualClock `sleep!` wraps its deadline in a release build

- Check timestamp additions and sleep deadlines in every build (`90ab68519`).

### [#1872](https://github.com/HerringtonDarkholme/hd-lang/issues/1872) Time arithmetic overflow panics in every build, as in Rust (Duration stays i64)

- Check timestamp additions and sleep deadlines in every build (`90ab68519`).

### [#1873](https://github.com/HerringtonDarkholme/hd-lang/issues/1873) Two with_outer_args branches in lower.rs looked unreachable

- Resolver: name the programs that reach a local impl's enclosing const and row arguments (`b12d655ae`).

### [#1874](https://github.com/HerringtonDarkholme/hd-lang/issues/1874) Lower::outer finds an enclosing parameter by scanning a Vec<String> per use

- Resolver: a local declaration finds an enclosing parameter by one hash probe (`0d1d1a9c9`).

### [#1862](https://github.com/HerringtonDarkholme/hd-lang/issues/1862) "add `use std.x.Y`" hints are prose with no fix-it

- Diagnostics: an "add `use std.x.Y`" hint carries the fix-it that adds the `use` (`d718f670f`).

### [#1875](https://github.com/HerringtonDarkholme/hd-lang/issues/1875) hd check --fix rechecks the whole package once per fix-it when a round is unsafe

- CLI: an unsafe --fix round finds its bad fix-its by halving, in O(k log n) checks (`b34ab6823`).

### [#1865](https://github.com/HerringtonDarkholme/hd-lang/issues/1865) `validate.Pattern` compiles its regex again on every `apply`

- Compile each validation Pattern once at construction (`6d692c92e`).

### [#1879](https://github.com/HerringtonDarkholme/hd-lang/issues/1879) Missing enum name before impl panics during recovery

- Parser: an item's own parse error breaks it wherever it is reported (`d70c3b8d5`).

### [#1871](https://github.com/HerringtonDarkholme/hd-lang/issues/1871) DiagBuf::push scans every row for a rooted diagnostic

- Diagnostics: index rows by root, one probe per rooted push (`2e4891eb5`).

### [#1870](https://github.com/HerringtonDarkholme/hd-lang/issues/1870) The checker's layout walk expands a shared type, exponential in its depth

- Checker: the slot layout walks a part with no slots once per check (`a2c03a082`).

### [#1764](https://github.com/HerringtonDarkholme/hd-lang/issues/1764) A written lifetime in a function type nested in another type is not recorded

- Lifetimes: a closure parameter's type that writes a lifetime is checked, not refused (`8eca61ad4`).

### [#1878](https://github.com/HerringtonDarkholme/hd-lang/issues/1878) local_at filters every block-local declaration on each lookup

- Resolver and checker: a module's locals are indexed by name and byte, not filtered per use (`493ab4f0c`).

### [#1877](https://github.com/HerringtonDarkholme/hd-lang/issues/1877) is_enclosing_parameter walks from the module root and every enclosing parameter per use

- Checker: a name that reaches nothing asks one index of enclosing parameters (`bf197c67d`).

### [#1876](https://github.com/HerringtonDarkholme/hd-lang/issues/1876) Each unknown name scans every std export to build its import hint

- Import hints: each interface's std exports are indexed by name once (`ab0ccfdf1`).

### [#1880](https://github.com/HerringtonDarkholme/hd-lang/issues/1880) Generated scale check: every grammar construct at n and 4n, flagging superlinear work

- Metrics: a generated scale check runs every node kind at n and 4n on deterministic work counters (`0954ceb11`).

### [#1881](https://github.com/HerringtonDarkholme/hd-lang/issues/1881) Skimming a file is quadratic in its size

- Skim: a file's use lines are placed by one merge, not a scan of its bodies per line (`833649598`).

### [#1888](https://github.com/HerringtonDarkholme/hd-lang/issues/1888) Resolver: each trait impl finds its trait by a scan of every item of the folder

- Resolver: an implementation finds its trait by one probe of the folder's items (`2f75e9fef`).

### [#1889](https://github.com/HerringtonDarkholme/hd-lang/issues/1889) Checker: an omitted-result plan links each identifier to every callable of that name

- Checker: the omitted-result plan links a shared name once, not to every callable of it (`a5b5b15b2`).

### [#1890](https://github.com/HerringtonDarkholme/hd-lang/issues/1890) Resolver: each local header walks down from its top-level declaration, scanning siblings

- Resolver: a local header reads the declarations around it from one walk of the module (`262f00979`).

### [#1866](https://github.com/HerringtonDarkholme/hd-lang/issues/1866) `JsonObject.remove` rewrites every later key's index, so n removals are quadratic

- Make ordered JSON object removal amortized constant time (`8c7d3196d`).

### [#1930](https://github.com/HerringtonDarkholme/hd-lang/issues/1930) Checker: each method call filters every trait that declares its name

- Checker: a method call asks the traits bound where it is, by method name, not every trait declaring it (`eb90ba082`).

### [#1929](https://github.com/HerringtonDarkholme/hd-lang/issues/1929) Resolver: same-named inherent members are compared pairwise

- Resolver: an inherent member is compared only with same-named members whose targets share a head (`0bbea3352`).

### [#1928](https://github.com/HerringtonDarkholme/hd-lang/issues/1928) Derivations: each `@derive` finds its trait's template by a scan of every item

- Derivations: a derivation finds its template, heads and block by one probe each, not a scan of the items (`7f105e9a8`).

### [#1903](https://github.com/HerringtonDarkholme/hd-lang/issues/1903) net_parity's local-address test exits 3 instead of 7 on one macOS run

- Use live socket addresses in the local-address parity regression (`96f61db06`).

### [#1910](https://github.com/HerringtonDarkholme/hd-lang/issues/1910) Scale check: `hd build`/`hd check` work is superlinear in n enums of three variants, each matched by a function main calls

- Checker: the enum tagging pass reads each enum's derivation blocks from one index (`bebc0446f`).

### [#1926](https://github.com/HerringtonDarkholme/hd-lang/issues/1926) Scale check: `hd build`/`hd check` work is superlinear in one struct of n fields, built with every field and read field by field

- Checker and emitter: a wide struct's fields cost one probe per use, not one walk per use (`8dd6b77fa`).

### [#1921](https://github.com/HerringtonDarkholme/hd-lang/issues/1921) Scale check: `hd build`/`hd check` work is superlinear in one function whose requirement row names n traits, using each with `$.use`

- Checker: a row's keys are indexed once per body, by trait and supertrait (`c08d93de1`).

### [#1923](https://github.com/HerringtonDarkholme/hd-lang/issues/1923) Scale check: `hd build`/`hd check` work is superlinear in n copies of the mixed item, type, statement, expression and pattern forms of shape `syntax-items` (type aliases, unions, const generics, embedded fields, shared variant data, lifetimes, projections, rest parameters, defaults, bang functions, `using`, `$.with`, derivations, comprehensions, spreads, data and named patterns)

- Checker: a trait call with a chosen impl reaches that impl's method, and a scope is found up its parent links (`31b554cff`).

### [#1925](https://github.com/HerringtonDarkholme/hd-lang/issues/1925) Scale check: `hd check`/`hd build` work is superlinear in n traits with a defaulted type parameter

- Checker: a method call on a receiver of a known head asks only the traits with impl rows for that head (`eb8a941e9`).

### [#1969](https://github.com/HerringtonDarkholme/hd-lang/issues/1969) The scale check's counters differ between runs of the same shape, so `paren-nest check` flips between pass and fail

- Metrics: the disk cache's own work is not counted, so a shape's counts repeat between runs (`bc12360e8`).

### [#1894](https://github.com/HerringtonDarkholme/hd-lang/issues/1894) Comparing struct values re-walks every field per body

- Checker: a closed type found clean by one body's containment walk is not walked again by the next (`6284fdc26`).

### [#1893](https://github.com/HerringtonDarkholme/hd-lang/issues/1893) Definite-initialization summaries make n generic impls quadratic, and a warm run does not reuse them

- Checker and driver: a body names each dispatch set by one key, and a module's summary lists each key's methods once (`5f9892910`).

### [#1892](https://github.com/HerringtonDarkholme/hd-lang/issues/1892) Method choice re-checks the argument per candidate, so nesting is exponential

- Checker: a method choice narrows its candidates by the heads of the argument and the expected result, from an index made once per context (`c06a646a4`).

### [#1937](https://github.com/HerringtonDarkholme/hd-lang/issues/1937) Checker: a method call on a type parameter scans the parameter's bounds in order

- Checker: a call on a bounded type finds its method's clause by one probe of the body's index by method name (`f07503333`).

### [#1915](https://github.com/HerringtonDarkholme/hd-lang/issues/1915) Scale check: `hd build` work is superlinear in a list type and literal nested n deep

- Collection: a type's layout hash reads its reached declarations from one Merkle hash per distinct type (`8101a8c29`).

### [#1954](https://github.com/HerringtonDarkholme/hd-lang/issues/1954) Decide (spec): a possible soundness gap: `function-value-written-lifetime-call-result-field.hd` (b071)

- Checker: a call through a field is checked against the field's type, never the stored function (`c5cba78bd`).

### [#1959](https://github.com/HerringtonDarkholme/hd-lang/issues/1959) The checker rejects returning a reference lent from a counted pattern binding, against ref.dep.origin.lend

- Checker: a counted reference read through a binding is lent from the places its values were read from (`6246979e1`).

### [#1970](https://github.com/HerringtonDarkholme/hd-lang/issues/1970) `T::f(..)` through a type parameter scans the parameter's bounds per call

- Checker: `T::f()` on a type parameter finds its declaring bounds by one probe of the body's bound index (`f5ec0ccbd`).

### [#1882](https://github.com/HerringtonDarkholme/hd-lang/issues/1882) opt::locals loops nine rewrite rules to a fixpoint with no stated round bound

- Bound local cleanup to two sweeps and expose liveness early (`32e41f94e`).

### [#1904](https://github.com/HerringtonDarkholme/hd-lang/issues/1904) Scale check: `hd build` work is superlinear in one expression of n `+` terms

- Bound local cleanup to two sweeps and expose liveness early (`32e41f94e`).

### [#1891](https://github.com/HerringtonDarkholme/hd-lang/issues/1891) Type resolution walks the interned type DAG as a tree, so doubled types are exponential

- Checker: a use resolves a type's head, writeback and the occurs check resolve each node once per body (`ff232d9f4`).

### [#1974](https://github.com/HerringtonDarkholme/hd-lang/issues/1974) A `for` loop's counted element binding is not lent from the collection's place

- Checker: a `for` loop's counted element binding is lent from the collection's place (`5705dd8b9`).

### [#1905](https://github.com/HerringtonDarkholme/hd-lang/issues/1905) Scale check: `hd build` work is superlinear in comparisons and logic nested n parentheses deep

- Emitter: rule 6 rewrites every value `if` in one pass, and rule 20 copies only for a rewrite (`87ca438ec`).

### [#1913](https://github.com/HerringtonDarkholme/hd-lang/issues/1913) Scale check: `hd build` work is superlinear in inline `if` expressions nested n deep

- Emitter: rule 6 rewrites every value `if` in one pass, and rule 20 copies only for a rewrite (`87ca438ec`).

### [#1977](https://github.com/HerringtonDarkholme/hd-lang/issues/1977) A key just published can be invisible to a new reader when 16 writers publish it at once

- Cache: a reader between a shard's first fold and its new header finds the fold's keys (`49ed13630`).

### [#1976](https://github.com/HerringtonDarkholme/hd-lang/issues/1976) Conformance fixture and package-tree names reflect implementation history, not the spec

- Conformance std/text: name the trait-value bound cases for what they check (`0cb6d92dd`).

### [#1916](https://github.com/HerringtonDarkholme/hd-lang/issues/1916) Scale check: `hd build` work is superlinear in `for` (and `while`) loops nested n deep

- Emitter: a loop's list proofs read its instructions from one preorder of the body's block tree (`c5673ef29`).

### [#1978](https://github.com/HerringtonDarkholme/hd-lang/issues/1978) Local cleanup: rule 20 (and rule 5 on a targeted `if`) rewrite one site per pass over the body

- Emitter: rule 20 rewrites every branch-feeding `&&` and `||` value in one pass (`14549a4b4`).

### [#1886](https://github.com/HerringtonDarkholme/hd-lang/issues/1886) Two more checker walks expand a shared type: `InferTable::occurs` and `Ck::normalize_deep`

- Checker: passing a shared type on walks each distinct node once (`185314d21`).

### [#1944](https://github.com/HerringtonDarkholme/hd-lang/issues/1944) Remove duplicate conformance fixtures

- Conformance: remove the C1 duplicates that a diff confirms (`e0db2ce5f`).

### [#1981](https://github.com/HerringtonDarkholme/hd-lang/issues/1981) Cache claims: concurrent runs compute an entry more than once on Windows, and a claim wait out of dependency order deadlocks hd run

- Cache: keep check claims within one computation and lock run membership portably (`0c23ca492`).
- Cache: release rejected claims, keep data readable, retain waiter interest (`846afbb18`).

### [#1907](https://github.com/HerringtonDarkholme/hd-lang/issues/1907) Scale check: `hd build` work is superlinear in n closures in one body, each capturing a local and called once

- Share immutable body analyses across emitted closure instances (`dce0982b2`).

### [#1918](https://github.com/HerringtonDarkholme/hd-lang/issues/1918) Scale check: `hd build`/`hd check` work is superlinear in an option type, value and pattern nested n deep

- Emitter: a release's Drop-call walk skips parts that hold no Drop value (`7b2b321fc`).

### [#1912](https://github.com/HerringtonDarkholme/hd-lang/issues/1912) Scale check: `hd build`/`hd check` work is superlinear in a generic struct instantiated n deep

- Checker, ownership, emitter: each level of a generic nest walks one new node (`95e0f5476`).

### [#1920](https://github.com/HerringtonDarkholme/hd-lang/issues/1920) Scale check: `hd build` work is superlinear in a chain of n functions with a `$ Console` row, each calling the next

- Scale check: row-chain build is linear, so its KNOWN row goes (`45ce8c6f2`).

### [#1956](https://github.com/HerringtonDarkholme/hd-lang/issues/1956) A compound store through a call place without Apply reports type-mismatch instead of not-callable

- Compound store through a call place without Apply reports not-callable (`9e2c3fb6f`).

### [#1957](https://github.com/HerringtonDarkholme/hd-lang/issues/1957) Decide (spec): covariant list joins against `types.lct.no-combine` (b029)

- Least common type: no permission weakening under a variance step (`f549b7d76`).

### [#1960](https://github.com/HerringtonDarkholme/hd-lang/issues/1960) Decide (spec): `unknown-name` against `invalid-facts-of-target` precedence (pilot)

- facts_of: an argument that names nothing is unknown-name (`daac7c1e7`).

### [#1961](https://github.com/HerringtonDarkholme/hd-lang/issues/1961) Decide (spec): the `*mut`-to-`&mut` loan claim (b120)

- Spec: converting *mut T to &mut T or &T takes a run-time loan (`75a5c39fa`).

### [#1958](https://github.com/HerringtonDarkholme/hd-lang/issues/1958) hd counts trait bound depth differently from trait.bound.depth (64 vs 66), and per bound shape

- Spec: a worked trait.bound.depth example and boundary fixtures per bound shape (`cbf4abd01`).

### [#1955](https://github.com/HerringtonDarkholme/hd-lang/issues/1955) Decide (spec): nesting and chain limits for prefix `*` and `**` (b008)

- Parser: prefix operators and ** count against the chain limit, in a loop (`a4b523e9c`).

### [#1962](https://github.com/HerringtonDarkholme/hd-lang/issues/1962) hd fmt writes the empty row as `$ ()`; the spec spells it `$()`

- hd fmt writes the empty row as $(), and the repo's sources follow (`bcc821d88`).

### [#1948](https://github.com/HerringtonDarkholme/hd-lang/issues/1948) Decide: does a conformance panic marker sit on the raising line or the entry call?

- Conformance: a panic marker sits on the line that raises the panic (`9d811bdef`).

### [#1950](https://github.com/HerringtonDarkholme/hd-lang/issues/1950) Decide: exact float results in three fixtures against `expr.power.float.within-ulp`

- Conformance: the three exact-float power cases compare within 1 ULP (`aaba160f4`).

### [#1949](https://github.com/HerringtonDarkholme/hd-lang/issues/1949) Decide: how conformance fixtures that depend on the target (usize width, heap size) are marked

- Conformance: `# fixture-target: wasm32` runs a case only on its target (`98d5b6060`).

### [#1953](https://github.com/HerringtonDarkholme/hd-lang/issues/1953) Decide: conformance fixtures that differ only in message text

- Conformance: texts the runner never judges move to a cli case (`c118ed0e5`).

### [#1952](https://github.com/HerringtonDarkholme/hd-lang/issues/1952) Decide: the 21 accept cases in `typing/warnings/` and how lint expectations are tested

- Conformance: the 21 former typing/warnings cases move to lint cases or become warning-free (`3875591c7`).

### [#1997](https://github.com/HerringtonDarkholme/hd-lang/issues/1997) Map.get_or_insert through a *mut does not panic while a live loan holds the map

- Pointer loans: a map probe's `&mut self` receiver lends the map it reaches (`a81d38f39`).

### [#1990](https://github.com/HerringtonDarkholme/hd-lang/issues/1990) The ownership passes' `Copy` walk recurses natively and answers "copies" for a type nested more than 32 declarations deep

- Ownership: the `Copy` walk is one stack walk that memoizes every type (`e3f248ead`).

### [#1991](https://github.com/HerringtonDarkholme/hd-lang/issues/1991) The solver's canonical-fold memo skips types with more than 8 variables, so wide generic nests stay quadratic

- Solver: the canonical-fold memo keeps every fold, whatever its variables (`9c993afff`).

### [#2005](https://github.com/HerringtonDarkholme/hd-lang/issues/2005) The inference table's open-resolution memo skips types with more than 4 unbound variables, so wide generic nests stay quadratic

- Inference: open resolutions are kept whatever their variables, and a binding kills what read it (`d659051ce`).

### [#1993](https://github.com/HerringtonDarkholme/hd-lang/issues/1993) hd fmt writes a space after = when a named argument's value is a variant shorthand

- Formatter: a variant shorthand after a named argument's `=` keeps `x=.None` (`1323cc603`).

### [#2001](https://github.com/HerringtonDarkholme/hd-lang/issues/2001) Conformance still accepts warn: expectations and # warning: markers that no case uses

- Conformance: no case expects a warning, and the index rejects one that does (`6f3793942`).

### [#1988](https://github.com/HerringtonDarkholme/hd-lang/issues/1988) Cache pack store on Windows: readers miss published entries and five disk tests fail

- Cache: release rejected claims, keep data readable, retain waiter interest (`846afbb18`).

### [#1868](https://github.com/HerringtonDarkholme/hd-lang/issues/1868) Swiss-group indexing made small-map hits 28% dearer

- Wasm: bypass hashing for small builtin map lookups (`05fee6382`).

### [#2000](https://github.com/HerringtonDarkholme/hd-lang/issues/2000) Release builds drop the run-time loan of a method's result reference passed directly as a call argument, so the alias write reads freed storage

- Wasm: carry inlined method-result loans through consuming calls (`0c3f01e1c`).

### [#2008](https://github.com/HerringtonDarkholme/hd-lang/issues/2008) A use of a growing recursive instance such as G[i32] adds item-too-complex to the recursive-value-type error

- Checker: an instance of a declaration that holds itself is not walked (`9972c56fa`).

### [#2007](https://github.com/HerringtonDarkholme/hd-lang/issues/2007) Tier-crossing rows list std items that stdlib-items.tsv does not list, and no check catches it

- Spec tiers: a crossing row lists only stdlib-tier std items (`201337469`).

### [#2011](https://github.com/HerringtonDarkholme/hd-lang/issues/2011) A struct with two growing fields hangs the header check: Containment::held walks 2^k instances up to TYPE_DEPTH

- Checker: the value rule decides each declaration, never its instances (`74ff55452`).

### [#1951](https://github.com/HerringtonDarkholme/hd-lang/issues/1951) Decide: citations of the 148 pointer-loan fixtures forced by the tier rule

- Conformance: the pointer-loan matrix runs on language-tier types and cites its rules (`1c89e444d`).

### [#1999](https://github.com/HerringtonDarkholme/hd-lang/issues/1999) The conformance batch formatter finds 285 fixtures it would change on main

- Conformance: the suite is formatted, and spec/check.sh keeps it so (`518461cd8`).

### [#2013](https://github.com/HerringtonDarkholme/hd-lang/issues/2013) A generic call on an instance of a self-holding struct hangs the body check: the solver's Copy proof walks growing instances

- Solver: the Copy row stops at an instance of a declaration that holds itself (`14760c1f0`).

### [#1998](https://github.com/HerringtonDarkholme/hd-lang/issues/1998) A panic report's frame names the line before the one that panicked, or drops the callee's frame

- Panic frames: an added check and a call after a folded branch keep their line (`b9d421cae`).

### [#1946](https://github.com/HerringtonDarkholme/hd-lang/issues/1946) Conformance checks that cannot fail

- Conformance: checks that cannot fail now can (`958d8bc8c`).

### [#1947](https://github.com/HerringtonDarkholme/hd-lang/issues/1947) Conformance fixtures that depend on map and set order

- Conformance: map and set fixtures stop assuming insertion order (`15a1066cb`).

### [#1945](https://github.com/HerringtonDarkholme/hd-lang/issues/1945) Conformance fixture titles that describe the wrong case

- Conformance: titles and names state the case's rule, not its history (`bfd89d15c`).

### [#1924](https://github.com/HerringtonDarkholme/hd-lang/issues/1924) Scale check: `hd build`/`hd check` work is superlinear in a tuple type, value and pattern nested n deep

- Scale check: tuple-nest build is accepted as bounded by MAX_NESTING (`d8b1551b4`).

### [#1931](https://github.com/HerringtonDarkholme/hd-lang/issues/1931) The scale check measures nest shapes far past realistic depth

- Scale check: tuple-nest build is accepted as bounded by MAX_NESTING (`d8b1551b4`).

### [#2010](https://github.com/HerringtonDarkholme/hd-lang/issues/2010) A dependent argument loan ends before later arguments run

- Wasm: retain dependent argument loans across evaluation and exits (`c4a767bcd`).

### [#2012](https://github.com/HerringtonDarkholme/hd-lang/issues/2012) Conditional reference arguments lose their runtime loan

- Wasm: retain dependent argument loans across evaluation and exits (`c4a767bcd`).

### [#2002](https://github.com/HerringtonDarkholme/hd-lang/issues/2002) A newtype over a !Copy type is Copy, so its copies share one unique value and release it twice

- Ownership: a newtype holds its base, so a newtype over a !Copy type moves (`b9cb22073`).

### [#2004](https://github.com/HerringtonDarkholme/hd-lang/issues/2004) A reachable cyclic newtype such as type A(A) hangs the build

- Checker: a newtype that holds itself by value is recursive-value-type (`c85887e41`).

### [#1989](https://github.com/HerringtonDarkholme/hd-lang/issues/1989) Releasing a newtype over a value with a `Drop` impl never runs that `Drop`

- Release glue: a newtype's release runs its layers' and its base's `Drop` (`1a960744b`).

### [#2003](https://github.com/HerringtonDarkholme/hd-lang/issues/2003) Releasing a map never runs its values' Drop

- Release glue: a map runs each live entry's key and value Drop (`08bc757e4`).

### [#2018](https://github.com/HerringtonDarkholme/hd-lang/issues/2018) A reused HD_CACHE makes `hd` panic in the disk cache's claim check during CLI conformance

- Cache: a task run inside a claim holder's join computes instead of waiting (`413121854`).

### [#1985](https://github.com/HerringtonDarkholme/hd-lang/issues/1985) A nested generic struct pays its depth squared in `Copy` goals

- Types: a Copy walk keeps the answer of every node it can, so a chain asked top down walks each level once (`c6a42c96e`).

### [#1830](https://github.com/HerringtonDarkholme/hd-lang/issues/1830) The containment walk is still quadratic where a recursive value type sits below a chain

- Checker: rings below chains are counted at 2,000 and 20,000 and enter their size (`bfe01d29c`).

### [#2021](https://github.com/HerringtonDarkholme/hd-lang/issues/2021) A claimed `check` or `iface` miss whose computation stops on an internal error keeps its claim

- Cache: a claimed miss's computation ends its claim on every return, through a guard that abandons on drop (`e187ebcc7`).

### [#1887](https://github.com/HerringtonDarkholme/hd-lang/issues/1887) A doubled tuple of references has an exponential slot layout, with no limit

- Checker: a type past 1,024 origin slots is type-too-large, and the escape walk enters each distinct part once (`6ab02204c`).

### [#1935](https://github.com/HerringtonDarkholme/hd-lang/issues/1935) hd_wasm: local coalescing builds a pairwise interference graph, quadratic in simultaneously live locals

- Scale: the interpolation build line is linear, so its KNOWN row goes (`25c479e80`).

### [#1914](https://github.com/HerringtonDarkholme/hd-lang/issues/1914) Scale check: `hd build` work is superlinear in one string of n interpolations

- Scale: the interpolation build line is linear, so its KNOWN row goes (`25c479e80`).

### [#1917](https://github.com/HerringtonDarkholme/hd-lang/issues/1917) Scale check: `hd build` work is superlinear in n `match` expressions with guards in one body

- Scale check: matches build is linear, so its KNOWN row goes (`108369669`).

### [#2024](https://github.com/HerringtonDarkholme/hd-lang/issues/2024) `Types::ref_slots` and `Types::fn_written_slots` walk a type as a tree, so a doubled type costs 2^n steps per call

- Types: each pool item keeps its reference slot counts, set once from its parts' counts when it is interned (`74decd75b`).

### [#1934](https://github.com/HerringtonDarkholme/hd-lang/issues/1934) `hd check` of n same-named inherent members still grows faster than linear past 8,000, outside the member comparison

- Checker: a method call on a struct tries the inherent methods whose impl targets that struct, through an index by name and head (`980beb281`).

### [#1908](https://github.com/HerringtonDarkholme/hd-lang/issues/1908) Scale check: `hd build` work is superlinear in n structs implementing one trait, each boxed as `*dyn` into one list

- Ownership: exclude retaining references from extra cell liveness (`ad573d088`).

### [#2025](https://github.com/HerringtonDarkholme/hd-lang/issues/2025) Local coalescing gives a bundle a new local when only a class other than the one it probes has room between its ranges

- Emitter: local coalescing keeps its order by first position; the one-probe class choice is a priced limit (`2cf842e0a`).

### [#1982](https://github.com/HerringtonDarkholme/hd-lang/issues/1982) A list loop whose proof passes still does several whole-body passes per loop, so n sequential index loops are quadratic

- Emitter: list loop proofs read one set of body facts and walk each list's aliases once, and dominance is an interval test (`67d636b68`).

### [#2026](https://github.com/HerringtonDarkholme/hd-lang/issues/2026) The backend's hash-consed live sets raise peak memory on long bodies by a table entry per trie node

- Wasm optimizer: the live sets' hash-consing table holds node indices, hashed by the node, instead of a copy of each node (`52690c3f8`).

### [#1895](https://github.com/HerringtonDarkholme/hd-lang/issues/1895) One body with many string temporaries is quadratic in `opt::locals`

- Regress string temporary coalescing at 1000 to 10000 locals (`15784cfba`).

### [#1909](https://github.com/HerringtonDarkholme/hd-lang/issues/1909) Scale check: `hd build` work is superlinear in one enum of n variants and one match with an arm each

- Memoize enum payload types across layout and ownership queries (`d6aed139c`).

### [#1902](https://github.com/HerringtonDarkholme/hd-lang/issues/1902) The Node test host rebuilds every provider and copies all exports per unit test case

- Test programs: one `hd.test` and one `hd.test.init` export dispatch by number, so an instance's exports are constant in the case count (`d4a32dffc`).

### [#2016](https://github.com/HerringtonDarkholme/hd-lang/issues/2016) The std spec promises insertion order for Set serialization, Map.extend and counts ties, while the language leaves map and set order unspecified

- Std: map and set rules promise only the iteration order, never insertion order (`8363ac4c1`).

### [#1996](https://github.com/HerringtonDarkholme/hd-lang/issues/1996) types.lct.conversions lists declared readonly variance, which types.lct.no-combine now leaves no case for

- Types: retire `types.lct.no-least` and its `no-least-common-type` code; the join has one failure (`8846a88fb`).

### [#1995](https://github.com/HerringtonDarkholme/hd-lang/issues/1995) facts_of(m.missing) reports unknown-name while an ordinary use of m.missing reports unknown-import

- Checker: a `facts_of` argument that names nothing reports the resolver's own error for its form (`035d0466b`).
- Checker: a `facts_of` field-access argument reports the error the same expression reports anywhere else (`89a4f449d`).

### [#2009](https://github.com/HerringtonDarkholme/hd-lang/issues/2009) The spec's named payload patterns write left=l, which hd fmt rewrites to left = l

- Fmt: a named field in a pattern has no spaces around `=`, as a named argument (`7b4ae056d`).

### [#2017](https://github.com/HerringtonDarkholme/hd-lang/issues/2017) The spec still says "dynamically safe" and names a rule after all!, though every trait is a dyn type now and the combinator is join!

- Spec and checker: `trait-not-dynamically-safe` becomes `invalid-dyn-type`, and the stale "dynamically safe" and `all!` names go (`3e0f15d4e`).

### [#2037](https://github.com/HerringtonDarkholme/hd-lang/issues/2037) A concurrent `hd check` over one shared cache can wait for a claim while holding a claim that is not its ancestor, tripping hd_cache's debug assertion

- Cache: a deferred store's second read of a contested miss ends the first read's claim when it hits (`d5a011767`).

### [#2019](https://github.com/HerringtonDarkholme/hd-lang/issues/2019) A Copy map key with a Drop implementation runs its hook for the compiler's own copies of it

- Checker: `impl Drop` on a declaration that can be `Copy` for some type arguments is `drop-on-copy-type` (`b2b43e8e8`).

### [#2039](https://github.com/HerringtonDarkholme/hd-lang/issues/2039) Spec 09 keeps the heading "Dynamic Safety" and three rule IDs named for dynamic safety, though no trait is unsafe as a dyn type

- Spec: the "Dynamic Safety" section of chapter 09 becomes "Dyn Types", and its "safe" rule IDs are retired for `dyn` names (`623a14c0d`).

### [#2046](https://github.com/HerringtonDarkholme/hd-lang/issues/2046) Spec 04 keeps the heading "Dynamic Safety" and three `types.trait.safe.*` rule IDs, though #2039 renamed the same names in spec 09

- Spec: chapter 04's "Dynamic Safety" section becomes "Dyn Types", and its `types.trait.safe.*` rule IDs are retired for `dyn` names (`465ab2996`).

### [#2045](https://github.com/HerringtonDarkholme/hd-lang/issues/2045) The lane gate tests hd_cache without its `disk` feature, so a change to the disk store runs none of its ~70 disk tests

- Lane gate: a change to hd_cache runs its disk tests with the `disk` feature, as CI does (`abd525372`).

### [#2043](https://github.com/HerringtonDarkholme/hd-lang/issues/2043) An explicit const argument to a method's const generic parameter, as in `s.helper::[3]()`, is `generic-kind-mismatch`

- Checker: a member's explicit list is checked against the generics of the member its call resolves to, so `s.helper::[3]()` fills a const slot (`196f7a4ce`).

### [#2038](https://github.com/HerringtonDarkholme/hd-lang/issues/2038) The cache size-cap test waits for a detached collection by wall-clock polling, so it fails on a loaded machine

- CLI tests: the cap test waits for the detached collection's own end, and runs no command after it (`055980ea6`).

### [#2050](https://github.com/HerringtonDarkholme/hd-lang/issues/2050) `commands_evict_over_the_cap_from_hd_cache_max_size` failed once in a full `hd_cli --test all` run and passes alone

- CLI tests: the cap test waits for the detached collection's own end, and runs no command after it (`055980ea6`).

### [#2055](https://github.com/HerringtonDarkholme/hd-lang/issues/2055) The hd_cache spawn guard is opt-in per test, so a new lock-checking test can miss it and flake

- Cache: every file lock ends at an explicit unlock, so a spawned child never holds one a test dropped (`6fe658694`).

### [#2054](https://github.com/HerringtonDarkholme/hd-lang/issues/2054) A store sometimes misses a key it published while another store merges the same shard, so `two_writers_merging_keep_every_indexed_pack_readable` fails once in about 25 loaded runs

- Cache: a reader reads a shard's log again only when a fold committed since, and a fold writes its header before it shortens the log (`ccf018139`).

### [#2053](https://github.com/HerringtonDarkholme/hd-lang/issues/2053) Sixteen `hd check` runs of one program sometimes check 93 header types twice, so `processes_checking_one_program_together_compute_each_entry_once` fails under load

- Cache: a contested miss's request to publish before it waits belongs to the thread that asked (`f67c8607c`).

### [#2029](https://github.com/HerringtonDarkholme/hd-lang/issues/2029) A method call scans every trait its receiver implements, so k traits cost k²

- Checker: a method call finds its receiver head's traits by the method's name (`54391fadf`).

### [#2032](https://github.com/HerringtonDarkholme/hd-lang/issues/2032) The serial executor scans the whole ready queue per pop, so n ready tasks cost n²

- Scheduler: the ready queue pops and un-readies a task without scanning it (`3523a7dbb`).

### [#2030](https://github.com/HerringtonDarkholme/hd-lang/issues/2030) Collect builds a layout cache per body, so a deep by-value struct costs depth²

- Ownership passes: the bodies of a run share one layout cache per thread (`11aa6e2b1`).

### [#1938](https://github.com/HerringtonDarkholme/hd-lang/issues/1938) Checker: the copy question walks each nested type's whole structure, and a scope question walks up each binding's depth

- Checker: a copy question memoizes every member type's answer, and a scope question climbs each node once per body (`8018bf193`).

### [#1933](https://github.com/HerringtonDarkholme/hd-lang/issues/1933) Checker: each unknown-method hint asks every trait of the program that declares the name

- Checker: an unknown-method hint asks only the traits and methods of the receiver's head (`83a62ffd2`).

### [#1936](https://github.com/HerringtonDarkholme/hd-lang/issues/1936) Checker: a trait call through a bound or a trait value copies every impl's method into the caller's initialization facts

- Driver tests: a call through a trait value names its dispatch set once per body, counted (`6a661ee21`).

### [#1984](https://github.com/HerringtonDarkholme/hd-lang/issues/1984) A chain of generic aliases expands as a tree in the resolver

- Resolver and checker: an alias names its parameters by position, so a generic alias chain is linear (`e7dac498d`).

### [#1987](https://github.com/HerringtonDarkholme/hd-lang/issues/1987) Building a dyn call is exponential in the depth of a supertrait diamond chain

- Share instantiated dynamic supertrait graphs on the fixed CI base (`400cb0d74`).

### [#1994](https://github.com/HerringtonDarkholme/hd-lang/issues/1994) The wasm optimizer's liveness rules take time quadratic in nested ** depth

- Count deep power-chain liveness and build the maximum chain on the CI-fixed base (`be34f5300`).

### [#2049](https://github.com/HerringtonDarkholme/hd-lang/issues/2049) On Node 24 a unit-test program of 20,000 cases crashes the test host, because `hd.test` dispatches every case from one function whose optimizing compile runs V8 out of memory

- Test program: the case and init dispatchers split into a tree of functions of at most 256 arms; hd test refuses a Node older than package.json's engines (`70fba1271`).

### [#2062](https://github.com/HerringtonDarkholme/hd-lang/issues/2062) The constants pass looks up each function's init key by a linear scan of every init an entry names, so it is quadratic in program size

- Wasm: the constants pass indexes init keys once and probes once per function (`2dfcf95df`).

### [#2064](https://github.com/HerringtonDarkholme/hd-lang/issues/2064) The driver's collect roots look up each init and each extra root by a linear scan of a list

- Driver: the collect step indexes its extra roots, init keys and host-ref keys once, so each lookup is one probe (`0f73c1cbe`).

### [#2063](https://github.com/HerringtonDarkholme/hd-lang/issues/2063) A test program's init dispatch lists every init each test module reaches, so its size and the passes over it are quadratic in a chain of modules

- Wasm: a test program names each init once and hd.test.init walks a unit graph in static memory (`495f4a919`).

### [#1898](https://github.com/HerringtonDarkholme/hd-lang/issues/1898) Suspension lowering emits code quadratic in the number of awaits

- Keep suspension values resident and account for fixed inert cleanup work (`bbcd94bba`).

### [#1939](https://github.com/HerringtonDarkholme/hd-lang/issues/1939) Codegen panics when a row key is supplied through a supertrait key

- Freeze supertrait row suppliers and adapt function values (`953e66b45`).

### [#1986](https://github.com/HerringtonDarkholme/hd-lang/issues/1986) A function value whose row a supertrait key entails traps when called through the wider type

- Freeze supertrait row suppliers and adapt function values (`953e66b45`).

### [#2014](https://github.com/HerringtonDarkholme/hd-lang/issues/2014) A panic-frame anchor between Wasm instructions keeps the local rules from removing a local.tee

- Keep panic-frame anchors on calls, outside optimizer adjacency (`8f91d5600`).

### [#2033](https://github.com/HerringtonDarkholme/hd-lang/issues/2033) The liveness rules' hash-consed sets hash about log n nodes per local access, so `bit-nest build` grows hashes 4.5x per 4x of nesting

- Count that liveness hashes no set nodes per access (`8c0c80c41`).

### [#2067](https://github.com/HerringtonDarkholme/hd-lang/issues/2067) A last-use read of a counted field of a value with a Drop hook becomes a move, so the value's own hook never runs

- Own: a last-use read of a copied part of a value with its own Drop hook stays a copy (`231083463`).

### [#2068](https://github.com/HerringtonDarkholme/hd-lang/issues/2068) A field moved into a callee that a release build lends stays in place, so the enclosing value's Drop hook runs in release but not in dev

- Own: a release summary keeps every parameter that holds a Drop value, so release runs hooks where dev does (`684439bad`).

### [#1971](https://github.com/HerringtonDarkholme/hd-lang/issues/1971) `ImplTable.arg_key` is built and never read

- Types: delete ImplTable.arg_key, which was built and never read (`8be711d21`).

### [#1927](https://github.com/HerringtonDarkholme/hd-lang/issues/1927) SyntaxKind::SkippedBody has no constructor

- Syntax: delete SyntaxKind::SkippedBody, which no parser path builds (`40dd21166`).

### [#1769](https://github.com/HerringtonDarkholme/hd-lang/issues/1769) Retire the lookup-latency goal metric

- Metrics: retire the lookup-latency goal metric (`70e23ee6e`).

### [#1992](https://github.com/HerringtonDarkholme/hd-lang/issues/1992) Diagnostic labels have no producer, yet every cache entry and DiagBuf carries their columns

- Diag: delete diagnostic label columns, which no stage produced (`fbe793ef3`).

### [#2069](https://github.com/HerringtonDarkholme/hd-lang/issues/2069) In a std module build, a chain of struct constructors over 16 values is inlined into callers that load the callee's result leaf by leaf, so the module grows quadratically

- Forward indirect results through inlined value wrappers (`820313cd0`).

### [#1967](https://github.com/HerringtonDarkholme/hd-lang/issues/1967) State the fixture rules in the conformance README

- Spec: state the fixture rules in the conformance README (`8c8801489`).

### [#1965](https://github.com/HerringtonDarkholme/hd-lang/issues/1965) Guard the conformance suite with the review's cheap script checks in spec/check.sh

- Spec: spec/check.sh guards the fixture rules the review's script checks find (`5d55bcba8`).

### [#1966](https://github.com/HerringtonDarkholme/hd-lang/issues/1966) Fix the pointer-loan matrix generator and the example splitter that wrote the bad series

- Conformance: the pointer-loan matrix titles its local column honestly (`f1a1df3ce`).

### [#1943](https://github.com/HerringtonDarkholme/hd-lang/issues/1943) Conformance reject cases that pass for the wrong reason: break only the rule under test

- Conformance: reject cases break only the rule under test (`2ee2ad5fa`).

### [#1867](https://github.com/HerringtonDarkholme/hd-lang/issues/1867) Count operations copy every operand through one shared scratch local

- Reuse count operands with independent computed temporaries (`182bab33e`).

### [#1964](https://github.com/HerringtonDarkholme/hd-lang/issues/1964) Triage the remaining plausible rows of the conformance fixture review

- Conformance: the last forty-four B1 rows and the B2 row get a verdict; no B1 or B2 row lacks one (`2d9dbdf4d`).

### [#1975](https://github.com/HerringtonDarkholme/hd-lang/issues/1975) Source-in, diagnostics-out Rust tests become stage-case fixtures: add `diagnostics` and `stdout` views

- Driver tests: the target's doc says which checks stay Rust (`f3f8872d8`).

### [#2044](https://github.com/HerringtonDarkholme/hd-lang/issues/2044) A Map or List element's own `Drop` hook call in the release and put glue has no program left that reaches it, since a type with `Drop` is never `Copy`

- Remove unreachable inline Drop hooks from Copy collections (`6afb591bc`).

### [#2070](https://github.com/HerringtonDarkholme/hd-lang/issues/2070) The browser compiler carries about 1.4 MB of duplicated generic std code (hash tables, BTreeMaps, iterators, drop glue)

- Reduce duplicated collections in the browser compiler (`4b207519e`).

### [#1869](https://github.com/HerringtonDarkholme/hd-lang/issues/1869) Taking `&param` makes a by-value parameter owned, costing a retain and release per call

- Lend immutable parameter addresses proved local to the call (`d5c5967c7`).

### [#1963](https://github.com/HerringtonDarkholme/hd-lang/issues/1963) Fix about 290 conformance citations and A9's 158 heading-level leads

- Loans: a pointer receiver read through a shared slot holds that slot for its call, as a reference argument does (`71372a1e3`).

### [#1900](https://github.com/HerringtonDarkholme/hd-lang/issues/1900) Reading all of stdin on the Node host copies the buffer per chunk

- Keep stdin fragments until each result is complete (`2cbd9d84f`).

### [#2015](https://github.com/HerringtonDarkholme/hd-lang/issues/2015) A panic raised by a call written over several lines reports the call's first line, while conformance markers sit on its last line

- Report a multi-line call at its closing argument line (`f8cc404d5`).

### [#2060](https://github.com/HerringtonDarkholme/hd-lang/issues/2060) A generic alias's argument that breaks the alias's parameter bound is accepted

- Check: a bounded alias's arguments meet its bounds where it is written (`270fde43e`).

### [#2066](https://github.com/HerringtonDarkholme/hd-lang/issues/2066) A variable inside a function type's requirement-row key is never decided, so `fn() -> i32 $ Gen[?U]` mismatches `fn() -> i32 $ Gen[i32]`

- Unify: a variable in a function type's row key is decided by the other row (`23ff4cbbd`).

### [#1972](https://github.com/HerringtonDarkholme/hd-lang/issues/1972) `hd check` and the conformance dev pipeline report a different number of `recursive-value-type` errors

- Tests: pin that every driver goal reports an associated-payload cycle per body (`ed06cb256`).

### [#2072](https://github.com/HerringtonDarkholme/hd-lang/issues/2072) An impl head's function type matches a goal's function type whose row keys differ, so `impl Takes[(fn() -> i32 $ Gen[i32])?]` satisfies `Takes[(fn() -> i32 $ Gen[string])?]`

- Solver: an impl head's function type matches a goal's only with the same row keys (`675fcd693`).

### [#1770](https://github.com/HerringtonDarkholme/hd-lang/issues/1770) disk metric: count per-worktree artifacts, record the toolchain apart

- Metrics: `disk` judges per-worktree artifacts and records the toolchain apart (`520432b17`).
- Metrics: `runtime` times each hd case and each Node case inside one warm process, by one batch method (`2255ff7b7`).

### [#1807](https://github.com/HerringtonDarkholme/hd-lang/issues/1807) Audit actions: Metrics harness, docs and dead structure

- Metrics: `runtime` times each hd case and each Node case inside one warm process, by one batch method (`2255ff7b7`).

### [#2073](https://github.com/HerringtonDarkholme/hd-lang/issues/2073) Apply two owner decisions: Uniq buffer-moving ops name borrow-conflict; same-named packages show their dependency key

- Spec and diagnostics: Uniq buffer-moving ops name borrow-conflict; same-named packages show their dependency key (`36184f3cb`).

### [#2056](https://github.com/HerringtonDarkholme/hd-lang/issues/2056) A run that probed alone computes its miss without a claim, so a run that joins during that computation computes the same entry

- Cache: a run that joins a lone run waits until the lone run claims what it computes (`a4e0306cb`).
- Cache: a joiner waits for a lone run at most a deadline, then computes the entry itself (`5d7810deb`).

### [#2059](https://github.com/HerringtonDarkholme/hd-lang/issues/2059) `cli_conformance` fails `host-binding-block-on-promise` when the PATH's Node lacks JSPI

- Tests: one pinned Node per test process; an older one fails once at setup, naming .node-version (`df897a4fe`).

### [#2051](https://github.com/HerringtonDarkholme/hd-lang/issues/2051) The test-runner design's "price of one program" names only constant globals, though each instance also pays per defined function

- Docs: the test program's price names the per-function instance cost on Node and the decided chunking (`4f6d711ed`).

### [#2006](https://github.com/HerringtonDarkholme/hd-lang/issues/2006) Thirty tier-crossing rows call a timer the only host wait a language-tier fixture can reach

- Conformance: 24 timer fixtures move to pending-tick; the six that need waits of different lengths say so (`e868ff566`).

### [#1901](https://github.com/HerringtonDarkholme/hd-lang/issues/1901) Node host: `read_text!` resolves every path component, `list_dir!` allocates per comparison

- Node host: read_text! skips path resolution under an unrestricted grant and stats once; list_dir! sorts without allocating (`30ea509d4`).

### [#2028](https://github.com/HerringtonDarkholme/hd-lang/issues/2028) Every build reruns the ownership passes on unchanged package bodies

- Ownership: a dev build takes a package body's owned form from the store while the body and its reads are unchanged (`0dc1b9e74`).

### [#1973](https://github.com/HerringtonDarkholme/hd-lang/issues/1973) ir-audit can't read the bench programs, so no finding can be priced against bench time or size

- CLI: HD_WAT=1 hd build writes the module's text beside the .wasm, every function by its hd name, so ir-audit reads the bench programs (`4e7137050`).

### [#2071](https://github.com/HerringtonDarkholme/hd-lang/issues/2071) Core coverage: hd_tir fell below its baseline (96.77% vs 96.96%)

- Cover core TIR ownership and closure-copy paths (`e3fbf1534`).

### [#1864](https://github.com/HerringtonDarkholme/hd-lang/issues/1864) The growth helper std/rt.alloc grown is inlined when callers shrink, costing binary_trees 10%

- Keep entry-path memory growth out of automatic inline copies (`fb93023b2`).

### [#1798](https://github.com/HerringtonDarkholme/hd-lang/issues/1798) Lint: get, change, store back on a Map points to get_or_insert, with a fix-it

- Lint: map-get-store-back reports a Map value copied out, changed and stored back, with a get_or_insert fix-it (`2f1f125b6`).

### [#2077](https://github.com/HerringtonDarkholme/hd-lang/issues/2077) Native host bindings: generate the Rust companion and the embedder API

- Native host bindings: hd build --engine wasmtime writes the Rust companion, and hd_run_wasmtime has the embedder surface it registers through (`aafb381b5`).

### [#2078](https://github.com/HerringtonDarkholme/hd-lang/issues/2078) Native host bindings: preflight resolves registered providers by identity before guest start

- Native host bindings: instantiation resolves registered providers by nominal identity before the guest starts, and links their ready methods (`754d40573`).

### [#2079](https://github.com/HerringtonDarkholme/hd-lang/issues/2079) Native host bindings: owned Rust value codecs and method schema checks

- Native host bindings: the generated companion decodes each call's arguments and encodes its result through codecs generated from the contract, and registers its dispatch (`c5a27fb2f`).

### [#2085](https://github.com/HerringtonDarkholme/hd-lang/issues/2085) hd_wasm's record_package_interface scans every recorded package host interface for each new one

- Index package host interfaces during module construction (`9f552cb38`).

### [#2080](https://github.com/HerringtonDarkholme/hd-lang/issues/2080) Native host bindings: a registered provider runs only with root manifest consent

- Native host bindings: a registered provider runs only with the app manifest's consent, checked through the one consent index every host asks (`88250667c`).

### [#2081](https://github.com/HerringtonDarkholme/hd-lang/issues/2081) Native host bindings: pending calls abort once and ignore late completions

- Native host bindings: a suspending provider method answers at once or returns a pending call that the instance's reactor holds, whose Send completion wakes that instance alone, aborted at most once on abort or close (`020a271f8`).

### [#2082](https://github.com/HerringtonDarkholme/hd-lang/issues/2082) Native host bindings: handle registry checks owner and trait, and release drops the value

- Native host bindings: a returned handle's trait object lives in its instance's registry, which checks each call's and release's number, trait and generation, and the last release drops it with no provider call (`df5906b29`).

### [#2083](https://github.com/HerringtonDarkholme/hd-lang/issues/2083) Native host bindings: run the host-binding CLI cases with native providers and match Node

- Native host bindings: every host-binding CLI case that reaches a provider runs on Wasmtime with a Rust provider and matches Node, and a provider's own failure prints the JS host's text (`bb71dad99`).

### [#1822](https://github.com/HerringtonDarkholme/hd-lang/issues/1822) Design: native package host bindings on wasmtime (accepted, build it)

- Native host bindings: every host-binding CLI case that reaches a provider runs on Wasmtime with a Rust provider and matches Node, and a provider's own failure prints the JS host's text (`bb71dad99`).

### [#2089](https://github.com/HerringtonDarkholme/hd-lang/issues/2089) Grants::limit scans every resolved limit on each host operation's grant check

- Capability grants: a host operation's grant check probes the limits keyed once per run instead of scanning every resolved limit (`a118e0f23`).

### [#2086](https://github.com/HerringtonDarkholme/hd-lang/issues/2086) The generated native codecs of a recursive package host type recurse natively to the depth of the value the guest or the provider passes.

- JS host: a recursive package host type's codecs read and write a value of any depth through one explicit stack instead of recursing once per nesting level (`582ed3bd8`).

### [#2094](https://github.com/HerringtonDarkholme/hd-lang/issues/2094) A native provider's deep recursive host value drops, clones, compares and prints recursively.

- Native host bindings: a recursive argument is lent to its provider and taken apart by the engine, and a recursive type clones, compares and prints on one explicit stack (`35a963c17`).

### [#1780](https://github.com/HerringtonDarkholme/hd-lang/issues/1780) Design: one instance form between hd_mono and emission (generic TIR for check, instance TIR for ownership and emission)

- Design: instance TIR, one instance form between hd_mono and emission (`efa3cbc94`).

### [#2101](https://github.com/HerringtonDarkholme/hd-lang/issues/2101) Instance TIR step 1: a `phase` column, and `verify_instance` on every driver-built instance body

- Instance TIR step 1: a `phase` column on TIR bodies, and `verify_instance` on every body the driver owns for an instance (`e54ccb9f9`).

### [#2074](https://github.com/HerringtonDarkholme/hd-lang/issues/2074) Twelve stage snapshots still take count operations their `expected:` line rules out

- Keep a count owner borrowed across trivial field writes (`caa633dd8`).

### [#2093](https://github.com/HerringtonDarkholme/hd-lang/issues/2093) Each Node package-host call that waits on a provider Promise costs a 1 ms timer tick, even when the Promise is already settled.

- Wake pending package calls when their Promise settles (`e581d8c1d`).

### [#2096](https://github.com/HerringtonDarkholme/hd-lang/issues/2096) hd selects Node at startup for a module that needs a package host trait

- CLI: without `--engine`, `hd run` and `hd FILE.wasm` run a module that needs a package host trait on Node, decided from its contract before it loads (`b28bf263a`).

### [#2090](https://github.com/HerringtonDarkholme/hd-lang/issues/2090) The JS host scans every pending operation on each completion turn and each deadline query

- Queue notified host operations instead of scanning every pending call (`1dd14af33`).

### [#2100](https://github.com/HerringtonDarkholme/hd-lang/issues/2100) hd_cli's a_test_run_and_its_executables_build_for_its_engine can hang forever in `cargo test -p hd_cli --bins`

- Run nested task graphs on their caller without yielding to outer jobs (`8b347f5e4`).

### [#2097](https://github.com/HerringtonDarkholme/hd-lang/issues/2097) The checker scans every top-level binding of the module each time it asks whether a top-level binding is `let mut`

- Use the module name index for global mutability questions (`9db0af8b6`).

### [#2111](https://github.com/HerringtonDarkholme/hd-lang/issues/2111) A by-value type larger than 4 GiB panics the Wasm layout with an add overflow instead of a diagnostic

- Layout: a type whose values pass 2^31 - 1 bytes is `type-too-large`, found from its shape before any leaf is copied (`7e2aaedc1`).

### [#2098](https://github.com/HerringtonDarkholme/hd-lang/issues/2098) Two hd_cli cache-claim tests hit the debug assertion "waiting for a claim while holding ... which are not its ancestors" when other cargo and hd processes run at the same time

- Keep lone-miss state visible until its claim is installed (`3b7ccf324`).

### [#1762](https://github.com/HerringtonDarkholme/hd-lang/issues/1762) The released hd binary runs programs on wasmtime by default

- Ship hd with Wasmtime as its default engine (`7279d9db6`).

### [#2112](https://github.com/HerringtonDarkholme/hd-lang/issues/2112) The frame pass reruns its fixed point over the whole body once per 64 locals that a local reference points into, so a body with n such locals costs O(n^2)

- Solve cell-reference origins for all locals together (`f48afe027`).

### [#2103](https://github.com/HerringtonDarkholme/hd-lang/issues/2103) Instance TIR step 3: `hd_mono::instantiate` replaces the inliner's substitution

- Instance TIR step 3: `hd_mono::instantiate` builds each generic instance's body, and a bound's call at a concrete receiver is chosen as its impl or builtin (`b56bc40aa`).

### [#2104](https://github.com/HerringtonDarkholme/hd-lang/issues/2104) Instance TIR step 4: generic local functions are instantiated in instance bodies

- Instance TIR step 4: a generic local function is one plain sub-body per distinct type arguments in its declaring context, so no pass after hd_mono sees one (`96a648c7c`).

### [#1771](https://github.com/HerringtonDarkholme/hd-lang/issues/1771) Rank alternative fix-its, likeliest first, and let the mistakes metric apply the first

- Rank uncertain fix-its and apply the first in mistakes (`5de6a2337`).

### [#2125](https://github.com/HerringtonDarkholme/hd-lang/issues/2125) Instance TIR step 4b: compiler-supplied bodies are owned per instance

- Instance TIR step 4b: a compiler-supplied body is owned per instance, at its own arguments (`6666529d0`).

### [#2105](https://github.com/HerringtonDarkholme/hd-lang/issues/2105) Instance TIR step 5: the emitter lowers instance TIR only

- Instance TIR step 5: the emitter lowers instance TIR only and substitutes no body type (`e36cddae7`).

### [#2106](https://github.com/HerringtonDarkholme/hd-lang/issues/2106) Instance TIR step 6: code cache keyed by instance plus the facts it read

- Ownership: a dev build's owned keys hold each module's checked items, so a body edit that infers the same signature keeps every other entry (`1b607a259`).

### [#2128](https://github.com/HerringtonDarkholme/hd-lang/issues/2128) Instance TIR step 7a: each local's storage is decided in TIR, not in the emitter

- Instance TIR 7a: storage rows are final in TIR before emission (`92fc1f83b`).

### [#2120](https://github.com/HerringtonDarkholme/hd-lang/issues/2120) A reference returned from a call that is never inlined loses its loan, so an aliasing grow lets it read freed memory

- Preserve call-result loans through generic carriers (`976473332`).

### [#2123](https://github.com/HerringtonDarkholme/hd-lang/issues/2123) Loan-ending in excl.rs has no test that a real conflict through a call still panics

- Preserve call-result loans through generic carriers (`976473332`).

### [#2124](https://github.com/HerringtonDarkholme/hd-lang/issues/2124) The release inliner and the structure walk copy and substitute callee bodies outside `hd_mono::instantiate`

- Use the shared instance builder for release copies (`d0b73b165`).

### [#2132](https://github.com/HerringtonDarkholme/hd-lang/issues/2132) Instance TIR consolidation: one body copy-with-remap replaces the four in inline, local_fns, splice and closures

- Instance TIR: one body copy-with-remap replaces the four in inline, local_fns, splice and closures (`7e802f123`).

### [#2118](https://github.com/HerringtonDarkholme/hd-lang/issues/2118) The owned verifier walks the whole body once per sub-body, so checked builds are quadratic in a body's closures

- Limit verifier flow states to each sub-body's facts (`56399cf8b`).

### [#2122](https://github.com/HerringtonDarkholme/hd-lang/issues/2122) A binding mutated then moved into an owning struct still takes an extra copy, hidden by a raised count budget

- Keep frame loads borrowed across read-only uses (`e64cb3f6d`).

### [#2133](https://github.com/HerringtonDarkholme/hd-lang/issues/2133) A function returning `impl Trait` of a concrete type reports `unsupported` instead of checking

- Check operator and index calls on an opaque result through its declared bound (`219598699`).

### [#2134](https://github.com/HerringtonDarkholme/hd-lang/issues/2134) Exclusive-loan checking rescans the whole body once per element loan per argument

- Read a call's exclusivity accesses from per-body tables, not a body scan per loan (`f46e86882`).

### [#2121](https://github.com/HerringtonDarkholme/hd-lang/issues/2121) The inliner refuses helpers by matching the mem_grow intrinsic name instead of costing non-removable entry work

- Price retained entry work in automatic inline copies (`e4589b533`).

### [#2153](https://github.com/HerringtonDarkholme/hd-lang/issues/2153) A literal tuple key indexing a bounded parameter or opaque result defaults to `usize` instead of taking the bound's key type

- Give a bounded receiver's literal key the bound's key type (`ad992b0a5`).

### [#2140](https://github.com/HerringtonDarkholme/hd-lang/issues/2140) The resolver's `gap()` is reached silently: a same-folder alias in a trait header lowers to `POr`

- Lower trait headers in the alias order, and delete the resolver's silent gap (`8d7449613`).

### [#2155](https://github.com/HerringtonDarkholme/hd-lang/issues/2155) A trait argument that breaks the trait parameter's bound is accepted

- Check a trait reference's arguments against the trait's parameter bounds (`408f84364`).

### [#2156](https://github.com/HerringtonDarkholme/hd-lang/issues/2156) A bound `IndexMut[K] & Index[K, Out = i32]` leaves `T::Out` unbound

- Merge a bound's bindings into the clause a supertrait already elaborated (`a7a230159`).

### [#2136](https://github.com/HerringtonDarkholme/hd-lang/issues/2136) Move checking builds the same per-body facts twice

- Build the move checks' per-body facts once (`70c348ad8`).

### [#2126](https://github.com/HerringtonDarkholme/hd-lang/issues/2126) Apply four owner decisions on native host bindings: refusal code, source names, Map row, dropped completion

- Retain source names in native package host contracts (`819bae58b`).

### [#2137](https://github.com/HerringtonDarkholme/hd-lang/issues/2137) `ref.elem.conflict` is checked twice: on expressions and again on TIR loans

- Check ref.elem.conflict once, on TIR loans (`0a000af70`).

### [#2138](https://github.com/HerringtonDarkholme/hd-lang/issues/2138) Decorator path resolution exists three times, and each copy recurses

- Resolve decorator and fact paths with one iterative resolver (`bd82a4420`).

### [#2135](https://github.com/HerringtonDarkholme/hd-lang/issues/2135) Seven recursive strongly-connected-component walks recurse natively to an input-controlled depth

- Find strongly connected components with one iterative Tarjan walk (`e28a8d6f4`).

### [#2144](https://github.com/HerringtonDarkholme/hd-lang/issues/2144) Owned-or-lent for parameters and arguments is decided twice, in the passes and in the emitter

- Record boundary ownership once in instance TIR (`5c188e1c4`).
- Restore type-work counting in the shared Copy walk (`75465df7f`).

### [#2143](https://github.com/HerringtonDarkholme/hd-lang/issues/2143) The pointer-loan holder analysis runs twice per instance, at link time and in the emitter

- Reuse link-time holder facts during emission (`f1b40662d`).

### [#2146](https://github.com/HerringtonDarkholme/hd-lang/issues/2146) Env is built by hand four times, and with_env rebuilds folders and ImplView per body

- Build every Env through with_env, and the program view once per run (`2ae903d8f`).

### [#2142](https://github.com/HerringtonDarkholme/hd-lang/issues/2142) The ownership pipeline runs about 20 steps where its doc names six; frame_reference_uses clones the whole body

- Batch final ownership counts and query frames without cloning (`5d02edf1d`).

### [#2141](https://github.com/HerringtonDarkholme/hd-lang/issues/2141) TIR dead columns: `Body.live` is never produced, and the `regions` column is reserved and unused

- Remove unused live and region columns from TIR bodies (`6c1358319`).

### [#2145](https://github.com/HerringtonDarkholme/hd-lang/issues/2145) A function value's input has three encodings between the passes and the emitter

- Encode function-value inputs once in TIR (`3750ca661`).

### [#2129](https://github.com/HerringtonDarkholme/hd-lang/issues/2129) Writing a body's storage rows scans every instruction and every local once per closure sub-body

- Index closure storage once and clear only touched copy marks (`8e69d31d3`).

### [#2130](https://github.com/HerringtonDarkholme/hd-lang/issues/2130) A checked-report build rescans the whole body for closure-shared locals at every cell it makes

- Reuse the prepared shared-local set for checked cell tags (`027001cba`).

### [#2147](https://github.com/HerringtonDarkholme/hd-lang/issues/2147) Ownership hand-matches `Facts.find` as a special case

- Remove the privileged Facts.find lookup recognizer (`ca4b00fd9`).

### [#2131](https://github.com/HerringtonDarkholme/hd-lang/issues/2131) Asking whether an item is a test entry root scans every test case

- Index test entry roots once per build (`3c967c081`).

### [#2157](https://github.com/HerringtonDarkholme/hd-lang/issues/2157) An invariant-24 internal error points at a line past the end of the user's file

- Preserve dependency diagnostic files in a single-file invocation (`59dd258d0`).

### [#2161](https://github.com/HerringtonDarkholme/hd-lang/issues/2161) Four type walks still recurse once per type level, allowed by the TYPE_DEPTH doc

- Walk types on explicit stacks in the checker's conversion and origin walks (`bc0051583`).

### [#2154](https://github.com/HerringtonDarkholme/hd-lang/issues/2154) The region solver stores each component's loans as a dense bitset, quadratic in the loans of a body

- Store a region component's loans as a set that costs its members (`b6163f0f6`).

### [#2162](https://github.com/HerringtonDarkholme/hd-lang/issues/2162) collect_fixture scans every collected instance once per definition

- Find a fixture's collected definitions in one index of the instances (`e120384a3`).

### [#2163](https://github.com/HerringtonDarkholme/hd-lang/issues/2163) Small duplicates left after the consolidations: push_record x6, LEB128 in hd_host_abi, exact-copy helpers

- Consolidate the repeated compiler and host ABI helpers (`fb9c68f5e`).

### [#2167](https://github.com/HerringtonDarkholme/hd-lang/issues/2167) About 25 type walks in hd_types, hd_check and hd_resolve still recurse once per type level

- Finish the plain opaque-return and self-reference conversions (`bf3112ea1`).

### [#2186](https://github.com/HerringtonDarkholme/hd-lang/issues/2186) A while-counter loop below the list length keeps its bounds check in release

- Drop the bounds check of a while counter below a length its test reads (`5a6305e82`).

### [#2178](https://github.com/HerringtonDarkholme/hd-lang/issues/2178) The folder-cycle diagnostic runs a breadth-first search over every folder from each folder of the cycle

- Use one sparse breadth-first search for folder-cycle diagnostics (`5b6a577b1`).

### [#2113](https://github.com/HerringtonDarkholme/hd-lang/issues/2113) The instantiation-cycle check matches an implementation head against a type part by part with no memo, so an impl over a tuple doubled through n aliases costs 2^n

- Run concrete impl heads over 13- and 130-level shared aliases (`5ce609b03`).

### [#2114](https://github.com/HerringtonDarkholme/hd-lang/issues/2114) `type-too-large` from the Wasm layout has no source location

- Cover layout diagnostic positions in stages and CLI JSON (`97e5ab560`).

### [#2184](https://github.com/HerringtonDarkholme/hd-lang/issues/2184) The scale metric's summary row shows a ratio over its printed 5x target and still says PASS

- Cover scale summary floors and printed limits (`a15a55153`).

### [#2160](https://github.com/HerringtonDarkholme/hd-lang/issues/2160) Two online Tarjan walks, the checker's ready-row search and the inliner's bottom-up visit, keep their own copies of the algorithm

- Remove the remaining compiler SCC copies (`d68ae1e1a`).

### [#2190](https://github.com/HerringtonDarkholme/hd-lang/issues/2190) A by-value list parameter's length region closes at a call that cannot change it

- Give a whole local's region row the twin's externality (#2190) (`bf59c218c`).

### [#2191](https://github.com/HerringtonDarkholme/hd-lang/issues/2191) Checking a call with a doubling type walks every leaf: dep_results::line_up_in has no pair memo (30 s at depth 29; red main)

- Count physical layout copies on oversized shared types (`dd3fdb2c1`).

### [#2182](https://github.com/HerringtonDarkholme/hd-lang/issues/2182) A release build folds a constant-making call into its full value at any size, writing one store per scalar

- Keep Memory constants behind their address boundary (`bce0c38c5`).

### [#2189](https://github.com/HerringtonDarkholme/hd-lang/issues/2189) An explicit Memory return expands its projected value into scalars

- Forward explicit Memory returns through their existing address (`96f97511d`).

### [#2187](https://github.com/HerringtonDarkholme/hd-lang/issues/2187) A counter below the length of a list in a struct field keeps its bounds checks

- Key the list proofs' lists by place, so a field list's counter drops its check (`cb7da51ce`).

### [#2175](https://github.com/HerringtonDarkholme/hd-lang/issues/2175) Backend-neutral TIR step 6: an explicit element check in TIR (6a), then one prove pass in hd_opt (6b)

- Make a stable list's header and length TIR values, deleting the emitter's list caches (`ccafe753a`).

### [#2119](https://github.com/HerringtonDarkholme/hd-lang/issues/2119) The stage `wat` view names a renumbered body's locals from the checker's TIR, so a generic local function's copies show wrong parameter names

- Name WAT locals from the body emitted after instantiation (`36419f9b6`).

### [#2148](https://github.com/HerringtonDarkholme/hd-lang/issues/2148) Leftovers and stale text after renames and deleted designs

- Finish the compiler leftover audit with shared test policies (`3e8c2cc51`).

### [#2139](https://github.com/HerringtonDarkholme/hd-lang/issues/2139) Two type lowerers: hd_check/ty.rs and hd_resolve/lower.rs share 26 rule IDs

- Share type-form construction between headers and body annotations (`092f88867`).

### [#2109](https://github.com/HerringtonDarkholme/hd-lang/issues/2109) An integration test's `hd_run!` of an executable that needs a package host trait fails on both engines

- Run named test children with their package providers and chosen engine (`daa9eb6ec`).

### [#2188](https://github.com/HerringtonDarkholme/hd-lang/issues/2188) Two writers merging cache packs can leave an indexed key unreadable (intermittent test failure)

- Publish folded cache logs with an atomic inode replacement (`af80d59d7`).

### [#2194](https://github.com/HerringtonDarkholme/hd-lang/issues/2194) Last-use dataflow is exponential in loop nesting depth: own::moves::last_uses re-walks nested loop bodies (28 nested loops take over 120 s)

- Count last-use work across loop nesting and candidate width (`b5c9ab69f`).

### [#2195](https://github.com/HerringtonDarkholme/hd-lang/issues/2195) Step 3a's Box statement breaks six std conformance cases in the runner's build (list index out of bounds)

- Clear cells whose declarations a sibling branch can skip (`a60f5f5b1`).
- Keep cell allocation live through ownership cleanup (`7e736497a`).

### [#2183](https://github.com/HerringtonDarkholme/hd-lang/issues/2183) Frame blocks live in a separate STACK memory: a borrow cannot point into them, and stack overflow reports an internal error

- Share checked frame storage with heap memory (`0c4a565b5`).

### [#2192](https://github.com/HerringtonDarkholme/hd-lang/issues/2192) Self::Item in an impl binding loses the declaring trait

- Retain the impl trait on Self associated bindings (`586dfcfa0`).

### [#2198](https://github.com/HerringtonDarkholme/hd-lang/issues/2198) Closures copy mut parameters instead of sharing their cell

- Share captured mutable parameters after their first write (`dff3064ad`).
- Update mutable receiver mutation baselines (`cf9482002`).

### [#2199](https://github.com/HerringtonDarkholme/hd-lang/issues/2199) Optimized nested mutable-parameter capture crashes the splicer

- List an inlined closure's cleanup suite in the call's block (`eab32130a`).

### [#2200](https://github.com/HerringtonDarkholme/hd-lang/issues/2200) Optimized captured collection mutation violates IsUnique ordering

- Record cell boxes before uniqueness checks in the ownership batch (`492e681e2`).

### [#2214](https://github.com/HerringtonDarkholme/hd-lang/issues/2214) Defer ABI wrappers for unused host import namespaces

- Defer unused host ABI namespace wrappers (`9da24a47f`).

### [#2216](https://github.com/HerringtonDarkholme/hd-lang/issues/2216) Encode exactly representable host integers without BigInt arithmetic

- Encode host integers with exact Number arithmetic (`a8408135c`).

### [#2215](https://github.com/HerringtonDarkholme/hd-lang/issues/2215) Share compiled test modules across a bounded Node test batch

- Run Node test programs in one bounded batch (`cc559101f`).

### [#2218](https://github.com/HerringtonDarkholme/hd-lang/issues/2218) Defer unused Node provider state for fresh host instances

- Defer unused Node provider state in fresh hosts (`0eb19ec3e`).

### [#2219](https://github.com/HerringtonDarkholme/hd-lang/issues/2219) Avoid dynamic wrappers for native scalar host calls

- Bind native scalar host calls without dynamic adapters (`4b44b4013`).

### [#2220](https://github.com/HerringtonDarkholme/hd-lang/issues/2220) Skip filesystem cleanup for unrequested test directories

- Skip cleanup of unrequested test temporary directories (`553a3c7cf`).

### [#2217](https://github.com/HerringtonDarkholme/hd-lang/issues/2217) reserve through &mut puts a local list in a stack frame instead of lowering by value

- Lower a list change through a local's &mut by value (`67ea2a50c`).

### [#2221](https://github.com/HerringtonDarkholme/hd-lang/issues/2221) Opaque result identity omits row and const arguments

- Fix opaque identity capture of row and const arguments (`b3f930607`).

### [#2209](https://github.com/HerringtonDarkholme/hd-lang/issues/2209) Reserve known output lengths in collection materializers

- Reserve exact collection materialization lengths (`9de468ed9`).
- Record measured reservation count sites (`a45368e7e`).

### [#2223](https://github.com/HerringtonDarkholme/hd-lang/issues/2223) Centralize std paths outside the backend in known items

- Centralize std paths in the known-items registry (`6fc15452c`).

### [#2224](https://github.com/HerringtonDarkholme/hd-lang/issues/2224) Mark the package-host test helper directly as test-only

- Expose the package-host helper's test-only guard (`bef397c49`).

### [#2225](https://github.com/HerringtonDarkholme/hd-lang/issues/2225) Type-check arguments of consumed annotate decorators

- Validate consumed annotate arguments through ordinary call checks (`13a434e39`).

### [#2226](https://github.com/HerringtonDarkholme/hd-lang/issues/2226) Carry selected source-function inputs as a checked type

- Carry checked source-function inputs into body checking (`67eb69a3d`).

### [#2229](https://github.com/HerringtonDarkholme/hd-lang/issues/2229) Refresh backend progress and retire finished chat artifacts

- Cleanup: refresh backend progress and retire finished chat artifacts (`67f97d819`).

### [#2230](https://github.com/HerringtonDarkholme/hd-lang/issues/2230) Carry checked Structure declaration inputs through generation

- Carry checked Structure inputs through generation (`8b1c9ac0b`).

### [#2228](https://github.com/HerringtonDarkholme/hd-lang/issues/2228) Debug string escaping decodes every ordinary Unicode scalar

- Scan Debug string escapes as ASCII byte runs (`ef8b7462b`).

### [#2233](https://github.com/HerringtonDarkholme/hd-lang/issues/2233) Release WAT views lose runtime helper names across feature sets

- Read compact release names in stage WAT views (`ad3519987`).

### [#2231](https://github.com/HerringtonDarkholme/hd-lang/issues/2231) Handle malformed Facts metadata at the checker boundary

- Handle malformed Facts metadata at the checker boundary (`3ef27f231`).

### [#2232](https://github.com/HerringtonDarkholme/hd-lang/issues/2232) Carry checked template headers into derive instantiation

- Carry checked template headers into derive instantiation (`42f8db1ee`).

### [#2234](https://github.com/HerringtonDarkholme/hd-lang/issues/2234) Handle missing interpolation runtime metadata without panicking

- Propagate interpolation metadata failures through checking (`7f6f96a14`).

### [#2235](https://github.com/HerringtonDarkholme/hd-lang/issues/2235) Remove the nonempty assumption from range-pattern tests

- Handle all range-pattern comparison combinations (`d4ea031b8`).

### [#2236](https://github.com/HerringtonDarkholme/hd-lang/issues/2236) Propagate missing error-derivation metadata without panicking

- Propagate missing error-derivation metadata (`15007b6d6`).

### [#2237](https://github.com/HerringtonDarkholme/hd-lang/issues/2237) Check named-closure bindings and generic-entry metadata

- Check named-closure binding and generic-entry metadata (`65b0ec8c2`).

### [#2238](https://github.com/HerringtonDarkholme/hd-lang/issues/2238) Validate solver-candidate table rows before call inference

- Validate solver-candidate table rows before call inference (`ba433aa16`).

### [#2239](https://github.com/HerringtonDarkholme/hd-lang/issues/2239) Carry checked method-reference function and trait metadata

- Carry checked method-reference metadata (`71d0c93e0`).

### [#2240](https://github.com/HerringtonDarkholme/hd-lang/issues/2240) Use checked proof outcomes for immediate and delayed bounds

- Use checked proof outcomes for immediate and delayed bounds (`91711b3a0`).

### [#2241](https://github.com/HerringtonDarkholme/hd-lang/issues/2241) Check map-loop pair metadata before emitting loop state

- Check map-loop pair metadata before loop emission (`c8976cccb`).

### [#2242](https://github.com/HerringtonDarkholme/hd-lang/issues/2242) Report invalid settled pattern-decision input without panicking

- Report invalid settled pattern input without panicking (`ef81e687b`).

### [#2243](https://github.com/HerringtonDarkholme/hd-lang/issues/2243) Cover resolver variance through tuple tails and hidden type fields

- Cover resolver variance through tuple tails and hidden fields (`95d4d13b6`).

### [#2244](https://github.com/HerringtonDarkholme/hd-lang/issues/2244) Generic rest-tuple implementation heads evade overlap checks

- Compare rest tails in implementation head unification (`ae6027bd9`).

### [#2245](https://github.com/HerringtonDarkholme/hd-lang/issues/2245) Cover CRLF normalization in prefixed string raw parts

- Cover CRLF normalization in prefixed string raw parts (`6401538b5`).

### [#2246](https://github.com/HerringtonDarkholme/hd-lang/issues/2246) Cover const evaluation rejection and error recovery paths

- Cover const evaluation rejection and recovery inputs (`74b5cb447`).

### [#2247](https://github.com/HerringtonDarkholme/hd-lang/issues/2247) A pass initializer silently poisons a scalar const argument

- Reject no-op initializers used as scalar const values (`f3591e701`).

### [#2248](https://github.com/HerringtonDarkholme/hd-lang/issues/2248) Cover opaque argument captures and reuse through hd stages

- Cover opaque argument reuse through captured identities (`1965c0e70`).

### [#2249](https://github.com/HerringtonDarkholme/hd-lang/issues/2249) Local types inside an impl capture invisible outer parameters

- Preserve visible captures across nested local impls (`a7f368b3e`).

### [#2250](https://github.com/HerringtonDarkholme/hd-lang/issues/2250) Local function declarations drop const generic parameters

- Keep every generic kind in local function metadata (`afc5094fb`).

### [#2251](https://github.com/HerringtonDarkholme/hd-lang/issues/2251) Cover resolver interface names and generic default recovery

- Cover resolver interface names and invalid defaults (`b9428f1de`).

### [#2252](https://github.com/HerringtonDarkholme/hd-lang/issues/2252) Cover alias lifetime slot ordering and default arguments

- Cover alias lifetime slots through source signatures (`7ff809279`).

### [#2253](https://github.com/HerringtonDarkholme/hd-lang/issues/2253) Cover qualified type paths and reexport error recovery

- Cover qualified type paths and reexport error recovery (`7ceb54961`).

### [#2254](https://github.com/HerringtonDarkholme/hd-lang/issues/2254) Generated impl headers treat const parameters as types

- Keep const kinds in generated implementation headers (`280d969cc`).

### [#2255](https://github.com/HerringtonDarkholme/hd-lang/issues/2255) Cover resolver generic header error recovery

- Cover generic header errors through HD source (`1e0acbc7c`).

### [#2256](https://github.com/HerringtonDarkholme/hd-lang/issues/2256) Cover resolver implementation signature contexts

- Cover implementation parameter contexts through HD source (`de87b1f6c`).

### [#2222](https://github.com/HerringtonDarkholme/hd-lang/issues/2222) A call to a lifted generic local function is not inlined in release

- Inline a call of a lifted generic local function as any function's (`8b8b3d7d7`).

### [#2185](https://github.com/HerringtonDarkholme/hd-lang/issues/2185) Backend-neutral TIR step 2b: lower Scope/Defer in TIR and delete emit/scope.rs (after step 5)

- Lower cleanup scopes and cancel exits in TIR; delete the emitter's scope ladder (`7fc4cc6b5`).

### [#1435](https://github.com/HerringtonDarkholme/hd-lang/issues/1435) A static lifetime on references: &'static T

- Retain static trait proof requirements for region checking (`7fbefa25d`).

### [#2259](https://github.com/HerringtonDarkholme/hd-lang/issues/2259) Impl matching loses static qualifiers in the written head

- fix(check): discharge static slots retained from impl heads (`49f3c4bef`).

### [#2265](https://github.com/HerringtonDarkholme/hd-lang/issues/2265) Trait-value conversion drops deferred implementation requirements

- fix(check): share deferred proof projection across value conversions (`0f809416c`).

### [#2266](https://github.com/HerringtonDarkholme/hd-lang/issues/2266) AutoClosing marker proofs discard deferred lifetime requirements

- fix(check): query conditional AutoClosing proofs in value regions (`79d881895`).

### [#2272](https://github.com/HerringtonDarkholme/hd-lang/issues/2272) `MemoryFs` refuses a write below a missing directory, as the hosts do

- Make MemoryFs require explicit destination directories (`125c14295`).

### [#2290](https://github.com/HerringtonDarkholme/hd-lang/issues/2290) A tuple match after an await copies a moved non-Copy payload (internal error)

- Move a tuple match's non-Copy payload out along its read chain (`0fc569412`).

### [#2286](https://github.com/HerringtonDarkholme/hd-lang/issues/2286) Dot lookup picks the referent's value impl over a reference's own trait impl

- Take a reference's own by-value method before reading through it (`49bc39924`).

### [#2291](https://github.com/HerringtonDarkholme/hd-lang/issues/2291) A guarded arm that binds a non-Copy payload stops the build (internal error)

- Bind a guarded arm's moving payload by view, and each binding at its leaf (`194b3882c`).

### [#2292](https://github.com/HerringtonDarkholme/hd-lang/issues/2292) A catch-all row that binds a whole non-Copy value reached on two paths stops the build (internal error)

- Move a whole non-Copy match binding out of the matched place (`dd25f6b15`).

### [#2303](https://github.com/HerringtonDarkholme/hd-lang/issues/2303) A guard that changes a non-Copy matched local stops the build (internal error)

- Reject a match guard that mutably borrows the place the match reads (`4bd07ff13`).

### [#2296](https://github.com/HerringtonDarkholme/hd-lang/issues/2296) std: contains takes &T on every collection

- std: borrow the needle in every collection contains call (`155abfb30`).

### [#2297](https://github.com/HerringtonDarkholme/hd-lang/issues/2297) std: dedup mutates in place on List and UniqList

- std: make List dedup mutate in place and return void (`d69dea608`).

### [#2298](https://github.com/HerringtonDarkholme/hd-lang/issues/2298) std: Map.keys and values return iterators

- std: return lazy cursors from Map keys and values (`0a44f9d55`).

### [#2293](https://github.com/HerringtonDarkholme/hd-lang/issues/2293) std: float math with Rust's names (sin, ln, exp, powf, consts)

- std: provide full-range floating-point math at both widths (`b08d3db79`).

### [#2299](https://github.com/HerringtonDarkholme/hd-lang/issues/2299) std: one closure-parameter shape (generic FnMut) and &self for non-mutating List helpers

- Use generic mutable callbacks throughout collection consumers (`0a84c38c6`).

### [#2311](https://github.com/HerringtonDarkholme/hd-lang/issues/2311) Imported numeric families clash with unrelated inherent targets

- Index inherent numeric families by their sealed primitive members (`ab69ceb65`).

### [#2305](https://github.com/HerringtonDarkholme/hd-lang/issues/2305) A Send crossing closure that captures a value with a Drop hook fails to build

- A crossing closure's body takes its captures, so each has one owner (`94accd5dc`).

### [#2306](https://github.com/HerringtonDarkholme/hd-lang/issues/2306) Cached owned Buffer branch fails drop verification

- Verify an elaborated drop under its drop flag as a drop on both paths (`965441ee6`).

### [#2308](https://github.com/HerringtonDarkholme/hd-lang/issues/2308) The shipped compiler's checked-report feature changes plain codegen

- A checked build links the checked report only when it asks for it (`8ce520de0`).

### [#2301](https://github.com/HerringtonDarkholme/hd-lang/issues/2301) std: rename std.time.min and std.validate.min away from cmp's min

- Rename duration and validation min constructors and their hint callers (`ede22f8c7`).

### [#2320](https://github.com/HerringtonDarkholme/hd-lang/issues/2320) A let mut copy of a moved Drop parameter drops the parameter unchanged

- Own a let mut copy whose hooked value a field write changes (Fixes #2320) (`0248c472b`).

### [#2323](https://github.com/HerringtonDarkholme/hd-lang/issues/2323) `hd fmt` removes the spaces around `& 'a` in an `impl` type

- fix(fmt): preserve impl lifetime bound spacing (`c8cf4529d`).

### [#2324](https://github.com/HerringtonDarkholme/hd-lang/issues/2324) Stage `stdout` and `functions` views ignore `std` in `@build`

- hd_testkit stages: the stdout and functions views honour std in @build (`0bb97ceef`).

### [#2327](https://github.com/HerringtonDarkholme/hd-lang/issues/2327) A *mut field read through Arc.get() then copied and pushed traps out of bounds

- fix(own): keep an address's origin through integer arithmetic (`ae3d7df25`).

### [#2330](https://github.com/HerringtonDarkholme/hd-lang/issues/2330) Check output spends tokens on empty fields and a fixed summary

- fix(cli): compact check diagnostics and measure JSON size (`ec12f358b`).

### [#2332](https://github.com/HerringtonDarkholme/hd-lang/issues/2332) Name, type, field and variant typos get no suggestion, and method hints mislead

- Diagnostics: one typo helper suggests and fixes misspelt names (`c8cdeee83`).

### [#2333](https://github.com/HerringtonDarkholme/hd-lang/issues/2333) Checker and parser messages that name one edit carry no fix-it

- Diagnostics: messages that name one edit carry it as a fix-it (`e717e218c`).

### [#2337](https://github.com/HerringtonDarkholme/hd-lang/issues/2337) A $.use(K) result escapes through a free result lifetime

- fix(check): a $.use result no longer escapes through a free lifetime (`0ae8be79d`).

### [#2331](https://github.com/HerringtonDarkholme/hd-lang/issues/2331) Some diagnostics print full paths and fixed explanation text

- fix(diagnostics): shorten messages and label conflicting impls (`5f60579ca`).

### [#2328](https://github.com/HerringtonDarkholme/hd-lang/issues/2328) Arena probes: forced inlining loses a write through a lent value; a top-level arena panics in threaded init

- fix(own): a binding's declaring store releases nothing (`ae1c82717`).

### [#2334](https://github.com/HerringtonDarkholme/hd-lang/issues/2334) Common forms from other languages get no pointer to the hd form

- Add direct repairs for common foreign language forms (`4473fa672`).

### [#2338](https://github.com/HerringtonDarkholme/hd-lang/issues/2338) website:e2e fails at its theme colour check, so the playground browser steps never run

- Repair stale website browser assertions and WAT refresh waits (`9c6803ef7`).

### [#2340](https://github.com/HerringtonDarkholme/hd-lang/issues/2340) Function-type lifetime diagnostics rescan every slot for each slot

- Make function lifetime diagnostic naming linear (`5d6ed4ad7`).

### [#2341](https://github.com/HerringtonDarkholme/hd-lang/issues/2341) The coverage check compares with a 13-day-old baseline and fails on deleted code

- Compare coverage with main's previous measured uncovered lines (`44da86c5e`).

### [#2351](https://github.com/HerringtonDarkholme/hd-lang/issues/2351) Two generic instances of one Wire crossing literal share a crossing number (internal error)

- fix: number crossing literals by their full instances (`8f4dc5172`).

### [#2352](https://github.com/HerringtonDarkholme/hd-lang/issues/2352) Boxing a counted trait reference, then erasing it to *dyn Inspectable, frees the new box early

- fix: keep the chosen data owner through trait erasure (`aaf854996`).

### [#2353](https://github.com/HerringtonDarkholme/hd-lang/issues/2353) A fix-it for unknown-variant on a borrowed Result moves out of the reference

- Preserve payload semantics in borrowed Result repairs (`e0a67f75e`).

### [#2325](https://github.com/HerringtonDarkholme/hd-lang/issues/2325) `hd fmt` adds parentheses around a broken chain in a call argument

- Avoid extra wrappers around binary call arguments (`181abbf3e`).

### [#2362](https://github.com/HerringtonDarkholme/hd-lang/issues/2362) Local functions with an omitted result type may recurse; fn.decl.omitted-recursion only checks top-level functions

- Fix result-cycle recording after row settling and on wasm32 (`5038d6171`).

### [#2364](https://github.com/HerringtonDarkholme/hd-lang/issues/2364) CI never builds the bench packages (json-decode, records, traits), so privacy broke two unnoticed

- Check every bench package and expected output in CI (`7c04a6103`).

### [#2355](https://github.com/HerringtonDarkholme/hd-lang/issues/2355) Impl placement is never checked inside std, and std already breaks it (TextRoom)

- Enforce standard-library impl ownership in the declaring modules (`b35224b72`).
- Keep std stage diagnostics independent of impl placement (`826170444`).

### [#2365](https://github.com/HerringtonDarkholme/hd-lang/issues/2365) Keep inlined &mut self struct locals in Wasm locals (scalar replacement), part 1 of List/Map in std

- Scalar replacement of inlined struct locals: forward references, spill at cold calls (`c309007a2`).

### [#2373](https://github.com/HerringtonDarkholme/hd-lang/issues/2373) Wasm emitter drops a value block's fall-through tail inside an inlined Scope (stale result local)

- Preserve fallthrough values at labelled Wasm block joins (`20b8b6c59`).

### [#2361](https://github.com/HerringtonDarkholme/hd-lang/issues/2361) std.host: a program can't handle SIGINT/SIGTERM (add a Signals capability)

- Add scoped Signals and per-kind scripted signal waits (`2edb36c43`).

### [#2375](https://github.com/HerringtonDarkholme/hd-lang/issues/2375) Finite recursion with a concrete mutable-reference type is rejected

- Reuse normalized instances for finite reference recursion (#2375) (`1c9268dcd`).

### [#2377](https://github.com/HerringtonDarkholme/hd-lang/issues/2377) Leading Copy bounds merge disjoint numeric inherent members

- Select numeric families from complete semantic bounds (`8dd522989`).

### [#2378](https://github.com/HerringtonDarkholme/hd-lang/issues/2378) Dotted package names alias unrelated dependency module prefixes

- Reject dots in package names and dependency keys (`82db56b4c`).

### [#2376](https://github.com/HerringtonDarkholme/hd-lang/issues/2376) Changing a facade re-export leaves stale private-import fix-its

- Track facade dependencies in private-import repairs (`3a7539107`).

### [#2381](https://github.com/HerringtonDarkholme/hd-lang/issues/2381) Release build panics in plan_in_place: Scope wrapper value pushed without an as_inst check

- Fix in-place planning of constant scope tails (`56c8d52f6`).

### [#2363](https://github.com/HerringtonDarkholme/hd-lang/issues/2363) Every program parses all of std: the language-layer ring makes cold builds scale with std

- Load std through real dependencies and fixed built-in homes (`353a76f3e`).

### [#2379](https://github.com/HerringtonDarkholme/hd-lang/issues/2379) No way to build a char from a u32 scalar value

- Add fallible Unicode scalar conversion for char (`0b6733ca5`).

### [#2380](https://github.com/HerringtonDarkholme/hd-lang/issues/2380) hd fmt splits a nested let-else so the else no longer parses

- Use each else clause's direct owner for formatter layout (`28b2fe589`).

### [#2386](https://github.com/HerringtonDarkholme/hd-lang/issues/2386) Single-threaded Cranelift makes a large program's cold start 1.4 s (0.28 s in parallel)

- Enable caller-budgeted parallel Cranelift compilation (`63586050d`).

### [#2387](https://github.com/HerringtonDarkholme/hd-lang/issues/2387) hd FILE.wasm runs a release-built module with Cranelift OptLevel::None by default

- Run prebuilt Wasm at its recorded optimization level (`b4186657a`).

### [#2385](https://github.com/HerringtonDarkholme/hd-lang/issues/2385) Release builds for wasmtime emit a precompiled .cwasm; warm start translates nothing

- Build and run native release artifacts for Wasmtime (`5fac7e19d`).

### [#2358](https://github.com/HerringtonDarkholme/hd-lang/issues/2358) std has no cryptography: add std.crypto (HMAC, AES, GCM, secure random)

- Expose crypto random bytes through the existing provider (`ac89fdfa6`).

### [#2389](https://github.com/HerringtonDarkholme/hd-lang/issues/2389) An index assignment `r[k] = v` through a user IndexSet lets a local reference escape

- Apply the generic store rule to trait calls through a generic implementation (`76e6b3c37`).

### [#2390](https://github.com/HerringtonDarkholme/hd-lang/issues/2390) Core coverage is red at de0f3149b: 15 newly uncovered ranges since f0cd1f4eb

- Cover the fifteen core ranges from the scheduled regression (`06c94e5fe`).

### [#2399](https://github.com/HerringtonDarkholme/hd-lang/issues/2399) A partly moved struct is released whole, O(depth) per type (#2031, #2392)

- Release a value kept by address in code linear in what it releases (`53c574898`).

### [#2400](https://github.com/HerringtonDarkholme/hd-lang/issues/2400) A nested tuple pattern copies every suffix tuple (#2392)

- Read a part of a value from the value's own locals (`295626f8c`).

### [#2395](https://github.com/HerringtonDarkholme/hd-lang/issues/2395) Wasm emitter panics in emit/indirect.rs:313 (index out of bounds) passing a struct with an optional field to a $ Console function

- Forward every trailing worker parameter once in leaf ABI wrappers (`3f0be85dd`).

### [#2357](https://github.com/HerringtonDarkholme/hd-lang/issues/2357) Cold builds rebuild each module unit's context over its whole closure (500-module chain 138 -> 197 ms)

- Share dependency metadata and replace per-module checking readers (`e80d28e16`).

### [#2413](https://github.com/HerringtonDarkholme/hd-lang/issues/2413) Core coverage is red at 50aaa1716: ten newly uncovered ranges from the 2026-11-26 merges

- Reach or remove the ten new core coverage gaps (`80d4f0e75`).

### [#2411](https://github.com/HerringtonDarkholme/hd-lang/issues/2411) std: no streaming reader for files or stdin; inputs can only be read whole (or stdin by line)

- Add std.encoding.json.decode_stream: JSON values one at a time from a Read (`6b7f6a99e`).

### [#2417](https://github.com/HerringtonDarkholme/hd-lang/issues/2417) std.io's File and Stdin are released on drop instead of disposed with using!, unlike every other std resource

- std.fs.File: an AsyncDisposable handle that using! closes, as std.net's sockets (`7c921a536`).

### [#2408](https://github.com/HerringtonDarkholme/hd-lang/issues/2408) f64.powf(-1, 1e20) is inf (should be 1); exp(1) and other float built-ins are 1 ulp off libm/Go

- Fix huge even unit powers and add float infinity classification (`1b8d6bbb5`).

### [#2398](https://github.com/HerringtonDarkholme/hd-lang/issues/2398) std.console.read_all! rewrites \r\n to \n in stdin (files keep the bytes)

- Preserve stdin CRLF and add lossy UTF-8 conversion (`918628032`).

### [#2393](https://github.com/HerringtonDarkholme/hd-lang/issues/2393) std.regex: inline flags, Perl classes (\d \w \s \b), escapes, and group names

- Extend std.regex with the decided RE2 flags and Unicode syntax (`5d12e2d9f`).

### [#2416](https://github.com/HerringtonDarkholme/hd-lang/issues/2416) Node's line-reader `ConsoleInput` (REPL, closed stdin) serves `stdin()` as lines plus `\n`

- fix(host): preserve raw stdin bytes in Node REPL sessions (`64cb5d2d4`).

### [#2397](https://github.com/HerringtonDarkholme/hd-lang/issues/2397) std.cli: grouped short flags, attached short values, multi-value options, dash-leading positionals, mode switches, opt-out help

- feat(cli): parse POSIX short groups and attached values (`25aa49ee7`).

### [#2424](https://github.com/HerringtonDarkholme/hd-lang/issues/2424) Core coverage is red at 80d4f0e75: newly uncovered ranges from the 2026-11-26 evening merges

- Reach or delete the twenty core ranges of coverage run 45932505681 (`87646f509`).

### [#2396](https://github.com/HerringtonDarkholme/hd-lang/issues/2396) Call stack size is fixed (4 MiB Wasm, 1 MiB shadow): deep recursion cannot be configured

- Raise the default call stacks: Wasm 4 to 16 MiB, shadow 1 to 8 MiB (`493b76e96`).

### [#2405](https://github.com/HerringtonDarkholme/hd-lang/issues/2405) A module-level constant cannot be exported: pub NAME := value is a syntax error

- Import a public binding by its written type; require the type (`7b56291f8`).

### [#2371](https://github.com/HerringtonDarkholme/hd-lang/issues/2371) CI: hd_command's all-tests binary is SIGKILLed about 24 s in, right after interrupt_signal tests (twice)

- fix(host): isolate Node groups and validate test signal targets (`61d380134`).

### [#2432](https://github.com/HerringtonDarkholme/hd-lang/issues/2432) hdjq delpaths fails ownership verification before execution

- Hold no borrow of a moved subject's part across a suspension (`4bae237a9`).

### [#2394](https://github.com/HerringtonDarkholme/hd-lang/issues/2394) std.time: broken-down time, strftime/strptime layouts, sub-ms precision, local offset

- feat(time): add fixed-offset DateTime parsing and formatting (`d4ddb4bfc`).

### [#2431](https://github.com/HerringtonDarkholme/hd-lang/issues/2431) Two cache writers merging packs can lose a published key (flaky hd_cache test)

- Keep published cache keys readable through splits and folds (`fe13e6906`).

### [#2406](https://github.com/HerringtonDarkholme/hd-lang/issues/2406) No way to profile an hd program: hd run has no profile option and release builds carry no symbols

- Add named Wasmtime guest sampling with hd run --profile (`c80f24da8`).
- Fix profiling CI while preserving release artifacts (`241f2e654`).

### [#2435](https://github.com/HerringtonDarkholme/hd-lang/issues/2435) Node on macOS: FIFO stdin never reaches end of input

- Fix Node FIFO stdin EOF on macOS (`f4a94ed05`).

### [#2436](https://github.com/HerringtonDarkholme/hd-lang/issues/2436) Unit cache key doesn't force new `Facts` fields into the key

- Require exhaustive Facts audit in emitted-unit keys (#2436) (`aff514ec4`).

### [#2402](https://github.com/HerringtonDarkholme/hd-lang/issues/2402) A nested tuple pattern of counted parts still copies each suffix (#2400)

- Answer match-reference reads from one tour of their roots (`1f474cee7`).

### [#2403](https://github.com/HerringtonDarkholme/hd-lang/issues/2403) A by-value release shim copies its parameter before counting its parts

- Name a borrowed local's own locals when it is read only by parts (`85abacf6a`).

### [#2434](https://github.com/HerringtonDarkholme/hd-lang/issues/2434) f32 `x ** y` and `x.powf(y)` give different bits

- Use same-width libm for f32 power (`e33a5d8c3`).
- Collect f32 power only from reachable operators (`eba7a2b77`).

### [#2419](https://github.com/HerringtonDarkholme/hd-lang/issues/2419) A `!` method that never suspends still gets a full frame (perf, compiler)

- Lower a bang call of a body that never waits as a plain call (`d0026abde`).

### [#2433](https://github.com/HerringtonDarkholme/hd-lang/issues/2433) Float `**` and `%` helpers are rooted in every program, so pow can't use table data

- Collect float operators on demand and pack pow's tables (`e6dfd9923`).

### [#2444](https://github.com/HerringtonDarkholme/hd-lang/issues/2444) Scale: fn-items and impls-of-one-trait builds are quadratic in use_facts (15x per 4x)

- Keep one read-after-move bit per moved place, not per move (`be3f2eda2`).

### [#2401](https://github.com/HerringtonDarkholme/hd-lang/issues/2401) Suspension liveness settles once per loop level (#1898, #2392)

- Answer the resume entry's liveness in one forward pass (`28c6e44ec`).

### [#2445](https://github.com/HerringtonDarkholme/hd-lang/issues/2445) A bang body that completes at once still takes the frame ABI

- Emit ready bang bodies with a plain function ABI (`554063e12`).
- Preserve cold-frame stage coverage after the ready ABI change (`c626495e9`).

### [#2453](https://github.com/HerringtonDarkholme/hd-lang/issues/2453) Local sharing's liveness (coalesce, rule 13 rows) settles once per loop level

- Settle every block's liveness with no round per loop level (`c41d18199`).

### [#2446](https://github.com/HerringtonDarkholme/hd-lang/issues/2446) Provider calls through $.use keep their callers' frames when every provider completes at once

- Propagate ready drive slots through the existing call graph (`b390d9efb`).
- Preserve worker and remote frames in the ready-slot fixtures (`3585201c9`).
- Align ready-slot snapshots with the merged plain-body ABI (`2e5f3645b`).

### [#2343](https://github.com/HerringtonDarkholme/hd-lang/issues/2343) Repeated loads through one pointer aren't reused: arena alloc loads its state 3-4 times

- Read a load through a parameter's reference once while nothing can write it (`65a466c8f`).

### [#2374](https://github.com/HerringtonDarkholme/hd-lang/issues/2374) Loop load hoisting stops at any non-pure call: needs a per-function memory-write summary (whole-program)

- Hoist loop loads across calls that write nothing, with one key per definition (`32463281d`).

### [#2455](https://github.com/HerringtonDarkholme/hd-lang/issues/2455) Release keeps a self copy local.get $x; local.set $x of a split parameter

- Remove self copies even when local indices stay unchanged (`7fbdd0745`).

### [#2454](https://github.com/HerringtonDarkholme/hd-lang/issues/2454) Liveness step 2 visits a block once per loop whose tail a multi-level exit reaches (#2453 follow-up)

- Settle each loop header from its exits, with no visit per loop level (`a12044b43`).

### [#2447](https://github.com/HerringtonDarkholme/hd-lang/issues/2447) Release inliner skips bang calls of bodies that complete at once

- Inline driven bang calls only when their copies cannot wait (`d89b484ed`).

### [#2458](https://github.com/HerringtonDarkholme/hd-lang/issues/2458) Warm dev builds are slower than cold and get slower every run: cache publish grows with total cache state

- Fold a cache shard only when its rows pay for the pages, spilling to segments (`0aaa99433`).

### [#2423](https://github.com/HerringtonDarkholme/hd-lang/issues/2423) Warm builds redo the whole back end: cache each instance's emitted unit and make the link a merge

- Key each stored unit on the declarations it reaches; read frame lines without a parse (`32465ef96`).

### Other changes

- Audit: answer Muse's AM scope question; queue AQ (supertrait bounds, regression from AL) and AR (user type named T/E breaks std) (`a652aac8c`).
- Dependencies: fetch, hash, and verify pseudo-versions (`fcf8632ab`).
- Dependencies: hd.sum manifest lines, and selection from every workspace member (`1e249f6ba`).
- Workspaces: root commands act on the members, -p selects them, hd new adds a member (`3b27f2c00`).
- Checker: package ownership across linked packages (`00e2e04bc`).
- REPL and hd doc: dep.KEY reaches the package's dependencies (`70a740c9a`).
- Audit: dependency and package files open to Muse again (#310 chunk 2 merged) (`f7906cb9d`).
- AM: migrate batch 4 TS tests to conformance fixtures (`77bd35c9c`).
- Audit: queue Muse job AS (cli.test.ts to the CLI conformance tier) (`2a9689c97`).
- Audit: Muse avoids dependency, package and REPL files while #319 runs (`6c77a3634`).
- Spec: dependencies chunk 3 and five owner decisions (task #319) (`88c1e8651`).
- Guide: a path requirement names any local package (task #319) (`f6cd430dc`).
- Prototype: dependencies chunk 3 and five owner decisions (task #319) (`78355cdaf`).
- Test: hd fetch at a workspace root fetches, so the member error names the member (task #319) (`c1766a58e`).
- Spec: the default-profile paragraph links the REPL section by path, so the glossary resolves it (task #319) (`e3d956ff3`).
- Audit: Muse avoids serde files while #320 runs; dependency and REPL files open again (`ee9840029`).
- Cleanup: delete the finished CLI-promises audit, list the owner's compiler audit in its README, trim answered OPEN_ISSUES rows (`40f2a97be`).
- AN: usize display, ambiguous import, std child paths (`ee489e286`).
- AO: portable runner selects cases, retries timeouts alone (`3ce7c3453`).
- Examples: realign examples.tsv after AN's new examples (`c8c35a7e1`).
- Spec: one format-neutral serialization consent, std.serde, read by JSON and the host boundary (task #320) (`c634c61b7`).
- Prototype: std.serde consent, JSON over it, and the boundary consent check (task #320) (`1c7c8c9ac`).
- Website: render the std.serde chapter (`77477c118`).
- Audit: Muse avoids test-runner files while #321 runs; serde files open again; realign examples.tsv rows when a spec block moves (`845219a44`).
- Audit: queue Muse job AT (child-path error names the real problem; review of AN) (`1e9764475`).
- Audit: queue Muse blocker AU (usize stopped being u32 after AN) and AV (runner crash beside a timeout passes) (`c2c502869`).
- Audit: queue Muse job AW (Timestamp serializes as RFC 3339 text, owner decision) (`1eab803ff`).
- AP: hints from the usability probe (`cc5b1679c`).
- Audit: queue Muse job AX (top-level it_each in an integration test is rejected) (`5dd3bea37`).
- Website e2e: the REPL and playground show a bare literal's type as usize (AN's display change) (`83a21e95d`).
- Audit: take AU (usize identity) back from Muse; its files are off-limits while the fix and the speed-gate regressions run (`fdddcd6fa`).
- Spec: test host tiers, unit tests on fakes and integration tests on the real host (task #321) (`2e6f8e097`).
- Prototype: test host tiers, temp_dir, task tests, and hd_run! of a task (task #321) (`a6f21068b`).
- Test: hd new missing-executable message names tasks too (task #321) (`9e23eea26`).
- Audit: test-runner files open to Muse again (#321 merged) (`a5ab0aa3d`).
- Audit: queue Muse job AY (review of AP: one intrinsic list, clearer empty-list message) (`955a4738e`).
- AU: usize is u32 again; its spelling is display metadata (task #325) (`8e03da043`).
- Audit: usize and checker-speed files open to Muse again (#325 merged); queue AZ (usize display follow-ups, F-611) (`ae33277e8`).
- Audit: Muse avoids REPL printing, the prelude and std.format while dbg (#323) runs (`5e9770ccc`).
- AV: a crash beside a timeout fails instead of passing on retry (`e6de8e497`).
- Audit: queue Muse job BA (defer the dead-fact warnings, owner decision) (`68aaa5d4c`).
- Audit: job BA parks the removed dead-fact rules verbatim in OPEN_ISSUES for a later revisit (`51d990ed6`).
- AQ: a bound implies its supertraits in every bound check (`58a357ffe`).
- AR: generic parameters shadow outer type names in the leak check (`7217c1a08`).
- Spec: the dbg intrinsic (task #323) (`e0b7c3dab`).
- Prototype: the dbg intrinsic (task #323) (`6c56cd2a2`).
- dbg merge: fix README warning row, update REPL value expectations to the shared printer (`16df2295b`).
- Audit: dbg files open to Muse again (#323 merged); job AW also gives the time types readable Debug (`35b261752`).
- AS: migrate test/cli.test.ts verdicts to the CLI tier (`fbcc2e3e9`).
- Spec: while true is an infinite loop for reachability (task #324) (`c74e0a81e`).
- Prototype: while true is an infinite loop for reachability (task #324) (`cfe67e504`).
- Audit: Muse avoids value-printing files while #328 runs (`6804addbe`).
- Audit: order Muse's jobs by user impact (AW, AZ, AT, AY, AX, AS, BA) (`e802212bb`).
- AT: child-module paths name the child and its import (`91b389875`).
- OPEN_ISSUES: uninhabited-binding waits for the error-code revamp (owner, after #324) (`059d16c46`).
- Audit: queue Muse job BB (messages never show hidden __std_/__pkg_ names; found reviewing AT) (`dd43ced14`).
- Audit: remove finished job AT (91b38987, reviewed: messages right in value and type positions, std and packages; hints compile) (`bfed26c54`).
- README: annotations and derivation as one headline feature, with a runnable Table template and serde example (`723181462`).
- Writing log: a leading-dot line after an inline if/else continues the else value (`e7b35fb90`).
- AW: Timestamp serializes as RFC 3339 text, time types gain text Debug (`c982ee580`).
- Audit: queue Muse job BC (a leading _ must silence unused-local-binding; broken since at least e0401f5b) (`9334eb199`).
- Spec: values print in hd syntax; dbg returns void with one ordinary signature (task #328) (`01f411ecb`).
- Prototype: values print in hd syntax; dbg is an ordinary void call (task #328) (`799876889`).
- dbg obeys only its declaration; derived newtype Debug prints the base value (task #328) (`d3670a926`).
- dbg-in-release covers every reference to dbg, a function value included (task #328) (`e9e9886ae`).
- Audit: value-printing files open to Muse again (#328 merged) (`7caceabc1`).
- Usability probe 2: writing log (`d57def82c`).
- Audit: queue Muse job BD (hints and guide text from usability probe 2) (`8e27a6207`).
- Audit: queue Muse job BE (small known failures for the compiler baseline: ALIAS-MISSING, DERIVE-MISSING, PRIMITIVE-LEFT-TRAIT, MVP-PATTERN) (`fb1452a9a`).
- BB: messages show hidden renames as writable paths (`ed257da3f`).
- CI: pushes that touch only audit/ don't start the test run, so queue edits stop cancelling real checks (`6e3a63e6d`).
- Audit: Muse removes its worktree and branch after each job lands (symlinked node_modules first, never --force on uncommitted work) (`b59e9bcb4`).
- Audit: queue Muse job BF (a missing-requirement error hides an unknown name; found by #281) (`c948a92bf`).
- Spec std batch (task #281): join method, unsigned parsing, sum, Path operations, rotation, property edge values (`a3812cbd6`).
- lib/std: join method, unsigned parsing, sum, Path operations, rotation, property edge values (task #281) (`a6784b934`).
- Property fixtures skip i32 MIN where 0 - MIN overflows or has no literal (calc round trip, derive price round trip) (`8d227750f`).
- Audit: queue Muse job BG (fix the examples' MIN overflow instead of skipping it) (`85841182c`).
- BE: ALIAS-MISSING resolves unused alias targets at declaration (`af1eef3a3`).
- BE: DERIVE-MISSING reports unknown-trait for empty derive names (`e8b78ed1b`).
- BE: PRIMITIVE-LEFT-TRAIT searches traits from a primitive left (`ead3d3b69`).
- CI: a running check on main always finishes; only the queued run is replaced (main went hours without a finished check) (`38bcbd289`).
- BE: MVP-PATTERN supports let data patterns on generic values (`d3ed71f3c`).
- Audit: queue Muse blocker BH (PRIMITIVE-LEFT-TRAIT doubled overload-candidates checking time) (`0b00c65d7`).
- BG: fix the examples' MIN overflow instead of skipping it (`10e7d0336`).
- Test registration by declaration identity: renamed and qualified it, it_each, it_prop (TEST-REG-ID) (`f5b5b8991`).
- Module documentation: the first ## block with a blank line after it documents the module (MODULE-DOC) (`4690a81f6`).
- Doc tests: hd test runs each fenced hd block of a ## comment as its own program (DOC-TESTS) (`57f8061fc`).
- JSON output: hd test --format json keeps stdout to JSON lines (CLI-57) (`57670ba96`).
- Skipped test cases: an integration test whose row names an unbound trait is skipped (CLI-53) (`b748cfc0b`).
- BH: keep two-primitive operators on the fast path (`e2d2f3fc3`).
- Audit: queue Muse job BI (only host traits make an integration test skipped; review of #274) (`2e941cb56`).
- BF: report the unknown name when it masks a requirement (`fa92d2bef`).
- Spec: hd add --dev and hd clean [--cache] (#322) (`a83ec4e20`).
- Spec: hd clean prints each member's build directory in workspace mode (#322) (`03b9171a7`).
- hd add --dev, hd clean, hd clean --cache (#322) (`40dc37b9f`).
- Select the five #322 CLI cases in the portable case list (`0342aee0b`).
- Audit: Muse job BI becomes 'no implicit skips' (owner): an unbound capability in a test is missing-requirement; remove module.testing.skipped and --deny-skipped (`e09f35105`).
- Audit: Muse job BI becomes 'no implicit skips' (owner): an unbound capability in a test is missing-requirement; remove module.testing.skipped and --deny-skipped; park the design (`32198c261`).
- Audit: job BI parks no skip design; the host binds every capability its tests need (owner) (`d18dba73a`).
- Audit: queue Muse job BJ (hd add across tables is an error; drop emptied table headers) (`c4ddd3c22`).
- Audit: job BI drops the host Why line (owner) (`2723753df`).
- Audit: queue Muse job BK (guide: wiring providers in main for dev and prod) (`d3cdc020b`).
- OPEN_ISSUES: attach the host-config sketch to the provider configuration item (case closed for test/dev/prod wiring) (`2274288c1`).
- Revert "BE: MVP-PATTERN supports let data patterns on generic values" (`991260754`).
- Audit: Muse retired; its open jobs move to the orchestrator's tasks #333-#336 (`04473e93a`).
- Audit: Muse's queue becomes Codex's (audit/codex_task.md): one long-lived worktree, never write in the shared main checkout, push with HEAD:main (`6700f0f61`).
- Tests: reject unbound integration requirements (BI) (`25edef67b`).
- CLI: reject dependency table moves (BJ) (`99638b5f8`).
- SHADOW-TPARAM: a name reused in a type parameter's scope is duplicate-binding (`360005d78`).
- VARIANCE-MUT-SELF: a mut self method counts toward declared variance (`a21796aba`).
- AMBIGUOUS-TYPE: inference with several solutions is ambiguous-type (`f6a9864f8`).
- BOUND-AMBIGUOUS: a bound several instantiations meet leaves an ambiguous parameter (`ff54a0a6d`).
- TYPE-GAPS: argument joins, constrained impl heads, supertrait impl bounds (`fca8ceab1`).
- VOID-UNIT: void is the empty tuple (), one type in checker and emitter (`3ecbc0d90`).
- REPL: a void input runs as a statement now that void is the empty tuple; () shows as void (`7f9b18a05`).
- Diagnostics: guide integration test fixes (BD) (`853b2b13a`).
- MVP-PATTERN: data patterns on generic data read fields through the member-access conversion (`737693abd`).
- Guide: wire application providers in main (BK) (`2428183aa`).
- Checker: silence unused underscore bindings (BC) (`464884180`).
- FACT-PATTERN: a typed fact's @annotate argument is a pattern matched against its target (`317759d77`).
- Writing log: missing Console on main in the typed-fact pattern user program (`3c42bf460`).
- Audit: Codex job AZ also covers a tuple-pattern literal shown as u32 (found reviewing BC) (`3861149a0`).
- Fix usize diagnostic spelling follow-ups (`0e6c1c9b3`).
- Unify built-in method hints (`85039faf4`).
- Cover integration test registration forms (`f79b6b9cf`).
- Audit: queue Codex job BL (nested variant pattern before a positional binding loses the binding; found by #278) (`5e85bff63`).
- Checker: GADT variant results (F-250) (`25236165d`).
- Writing log: three mistakes in the GADT expression-tree program (#278) (`20e2bf961`).
- BA: Defer dead-fact warnings (`e33346ddc`).
- Audit: queue Codex job BM (usize in method-receiver messages; review of AY) (`ad2cf92ca`).
- Audit: drop Codex job BM; usize becomes a distinct primitive (owner), which removes the display gap at its root (`065989cb4`).
- Audit: queue Codex job BN (readonly argument to a generic mut parameter; found by #335) (`f6aadef7e`).
- Checker: recheck an open-formal argument against its solved formal (#335) (`15742692f`).
- Audit: Codex avoids usize/numeric files while #336 makes usize distinct (`b066ff39d`).
- BL: Preserve bindings after nested patterns (`f96361229`).
- BN: Solve generic mut diagnostics from readonly shape (`6c8344b53`).
- Audit: offload to Codex: BO (speed-gate baselines recorded on CI) and BP (top-level effects in a non-entry module of a script package) (`4fda68a7e`).
- Audit: offload to Codex: BQ (intern types for compile speed; after the usize change frees src/types.ts) and BR (microbenchmarks vs Python/Node and Wasm size, measure only) (`4277540b7`).
- Spec: usize is a distinct primitive, not an alias of u32 (task #336) (`05404d28d`).
- Checker: usize is a distinct primitive; delete the display spelling machinery (task #336) (`4567de8aa`).
- Guide: usize is a distinct type, not the prelude's name for u32 (task #336) (`f96d2607e`).
- Audit: usize files open to Codex again (#336 merged; BQ can start); Codex avoids module-visibility files while P1a runs (`4d35ba844`).
- Audit: queue Codex job BS first (owner: a shift count may be any unsigned type) (`6f6d00f2b`).
- J: Review new src commits (`df23af828`).
- Audit: Kimi task queue (prose and doc jobs: split long spec sentences, the long 06 paragraph, examples for std rules) (`8a8826d0b`).
- Checker: module visibility within one package (task P1a) (`0563a3885`).
- Guide, known issues: module visibility within one package (task P1a) (`14dbea5a9`).
- Audit: Codex avoids module name-scope files while P1a2 runs (`2eb715942`).
- BS: Accept every unsigned shift count (`d0e5440ea`).
- Spec: split long sentences in std/testing.md (K1) (`31b4b5831`).
- Spec: split long sentences in std/regex.md (K1) (`ab3bc9e6c`).
- Spec: split long sentences in std/time.md (K1) (`baa9d1832`).
- Spec: split long sentences in std/json.md (K1) (`72ee56c15`).
- Spec: split long sentences in std/num.md (K1) (`1066e5599`).
- Spec: split long sentences in std/format.md (K1) (`6adb186c8`).
- Spec: split long sentences in std/path.md (K1) (`94d5c7fca`).
- Spec: split long sentences in std/collections.md (K1) (`0d0aef5d9`).
- Spec: split long sentences in std/cli.md (K1) (`418036bdf`).
- Spec: split long sentences in std/fs.md (K1) (`c902facb6`).
- Spec: split long sentences in std/iter.md (K1) (`c6ad63724`).
- Spec: split long sentences in std/option.md (K1) (`079043d94`).
- Spec: split long sentences in std/result.md (K1) (`eea259cbf`).
- Spec: split long sentences in std/text.md; K1 done (K1) (`bc86ae193`).
- Spec: restore the location of the second BadRange case (review of Kimi K1) (`111a9a0b6`).
- Audit: Kimi rule: a split sentence keeps every qualifier on each part (from the K1 review) (`8615327c4`).
- Spec: split long sentences in cli/command-line.md; K2 done (K2) (`e7ef2c0b0`).
- Spec: split the long Why paragraph in 06-control-flow.md; K3 done (K3) (`7bed0f4f0`).
- Checker: top-level names stay in their module (task P1a2) (`234c1ce16`).
- Guide: wiring examples import their requirement traits (task P1a2) (`63f686f04`).
- BO: Record performance baselines on CI (`d2dec1f0b`).
- BP: Keep non-entry module initialization requirement-free (`df4dcb464`).
- Audit: move job BR (microbenchmarks and Wasm size) from Codex to Kimi as K5 (`1eb7fbce0`).
- Audit: queue Codex job BT (record the perf baseline across five runners, not five runs on one; review of BO) (`800c36f39`).
- Spec: add examples to std/json.md sections that had none (K4) (`c8095bf95`).
- Spec: add examples to cli/command-line.md sections that had none (K4) (`0f3786714`).
- Spec: add examples to std/regex.md sections that had none (K4) (`54c0b8b62`).
- Spec: add examples to std/random.md sections that had none (K4) (`2340c30ff`).
- Spec: add examples to std/testing.md sections that had none (K4) (`2acd892b2`).
- Spec: add an example to std/cli.md Argument Forms (K4) (`62f24b93b`).
- Spec: add an example to std/digest.md's opening section (K4) (`564cd6951`).
- Spec: add examples to std/encoding.md sections that had none (K4) (`7276af627`).
- Spec: add an example to std/fs.md Suspension (K4) (`838ff69b8`).
- Spec: add an example to std/iter.md List And Optional Map (K4) (`2e06a65fe`).
- Spec: add an example to std/serde.md Reading (K4) (`806ec1dc5`).
- Spec: add an example to std/time.md Timestamps And Instants; K4 done (K4) (`ea961d10a`).
- Spec: a module's scope is its own declarations, the prelude, and its uses (codifies P1a2; task P1a3) (`e8ff9ce34`).
- Linker: std imports are per module (task P1a3) (`a0b0e721b`).
- CI: record each perf baseline run on its own runner, then merge p90 (P1c2) (`d534f37fa`).
- BQ: Memoize structural type parsing (`fe4983f48`).
- BT: Close cross-runner baseline recording (`c488689b2`).
- Perf: runtime microbenchmarks and Wasm size findings; K5 done (K5) (`a745b5465`).
- BQ: Intern parsed type structure (`24adec54a`).
- Audit: refill Kimi's queue: K6 (split long sentences in the language chapters) and K7 (usability probe 3, read-only) (`73aa0c594`).
- Perf: speed-gate baseline recorded on CI (5 runners, p90; run 37413926842) (`31ba0ecd1`).
- P1e: Build strings in near-linear time (join by halves) (`b1dee393f`).
- P1f: Remove the remaining quadratic string appends in lib/std (`9c385f387`).
- P1i: Runtime panic reports name the operation that panicked (`91bbf3006`).
- P1i: Panic sites for suspension polls, none in generated closures (`2498eac21`).
- P1i: Tests that read emitted code strip panic-site lines (`eac86a475`).
- P1d: Baseline report of the toy compiler and CLI against the spec (`34a2cb925`).
- Audit: usability probe 3 (todo CLI, package with tests, word counter, typed Expr) (`f6aecd14f`).
- CI: e2e expects P1i's located panic text (`d20b6b13d`).
- Audit: refill Kimi's queue: K0 (push its reviewed K6 chapters), K6 rest, K8 (guide examples), K9 (usability probe 4), K10 (spec examples) (`82380fd13`).
- Format website/e2e.ts (oxfmt line width) (`83b74a087`).
- P1j: Unbounded recursion is a stack-exhausted panic (F-161) (`f2781c6a8`).
- P1j: hd build FILE writes to the build directory; help matches the spec (`d9e34fec7`).
- P1j: The playground binds Clock and Random, and names what it lacks (`385957f84`).
- P1j: Select cli/test-timeout and cli/test-snapshot-file; guard the selection (`658313b7f`).
- P1j: Bring src/KNOWN_ISSUES.md case counts up to date (`87104c8fc`).
- Log hd writing mistakes from the P1j probes (`64868a1c3`).
- Roadmap: start the new compiler; freeze src/ as a test oracle (owner, 2026-10-06) (`f5deddd4e`).
- P1k: Diagnostic hints that the 2026-10-06 probes asked for (`a6597f87f`).
- Spec: split long sentences in lang/06-control-flow.md (K6) (`a6f289f58`).
- Spec: split long sentences in lang/08-data-and-enums.md (K6) (`c2e099bf9`).
- Spec: split long sentences in lang/07-functions.md (K6) (`c15e95b2a`).
- Spec: split long sentences in lang/01-lexical-structure.md (K6) (`4dd96eab2`).
- Spec: split long sentences in lang/02-grammar.md (K6) (`54f677317`).
- Spec: split long sentences in lang/03-names-and-scopes.md (K6) (`17606afe6`).
- Spec: split long sentences in lang/14-annotations.md (K6) (`1db4954aa`).
- Spec: split long sentences in lang/09-traits.md (K6) (`eed0f398a`).
- Spec: split the remaining long sentence in lang/09-traits.md (K6) (`c2209684d`).
- Spec: split long sentences in lang/11-requirements-and-suspension.md (K6) (`4b8c877b0`).
- Spec: split long sentences in lang/05-expressions.md (K6) (`8cd0b0d9b`).
- Spec: split long sentences in lang/04-type-system.md (K6) (`ba3fc2345`).
- Spec: split long sentences in lang/10-modules.md; K6 done (K6) (`3540ab4d7`).
- F1: Cite chapter 08 rule IDs in the fixtures that already test them (`336cdff22`).
- F1: Report an embedded-field default as syntax-error (`95a6f882c`).
- F1: Fixtures for uncited chapter 08 rules (`9df4a4648`).
- Spec: restore two K6 conditions and one referent (review of Kimi's K6) (`5d3df32de`).
- F2: Cite chapter 13 rule IDs in the fixtures that already test them (`6f2af4977`).
- F2: Fixtures for uncited chapter 13 rules (`114d995af`).
- F2: Pin F-615, branches checked against an expected type (`b2b0ff926`).
- Guide: fix five LANGUAGE_TOUR blocks that no longer compiled; K8 done (K8) (`9858ff7c1`).
- Probe: usability probe 4 log; K9 done (K9) (`e711555d0`).
- Audit: top up Kimi's queue: K11 (examples for the remaining language chapters), K12 (usability probe 5) (`cb1a04c69`).
- F3: Cite serde and JSON rule IDs in the fixtures that already test them (`34eefeae4`).
- F3: Derive Serialize and Deserialize on a newtype (`27378cde5`).
- F3: Fixtures for uncited serde rules (`f3218a17c`).
- F3: Pin F-616, a derived build fills a defaulted member without reading it (`9455e1d5d`).
- Spec: add examples to 13-gadts.md sections that had none (K10) (`034ba3c79`).
- Spec: add examples to 07-functions.md sections that had none (K10) (`79dd5a058`).
- Spec: add examples to 14-annotations.md sections that had none (K10) (`d58d606d6`).
- Spec: add examples to 02-grammar.md sections that had none (K10) (`b770e7458`).
- Spec: add examples to 08-data-and-enums.md sections that had none (K10) (`8f59ef29b`).
- Queue: K10 done (K10) (`ff9adafaa`).
- Audit: K12 notes F-616 is fixed (`00e83f28a`).
- B1: Fix F-616, only Default's build fills a declared default unread (`b4d59ce44`).
- D1: Record the owner's decisions of 2026-10-06 in the spec (`9b5c92178`).
- D1: hd test runs every case in text mode too; close F-403 (`1e7c33f8f`).
- D1: hd build FILE writes build/<profile>/files/STEM.wasm (`b45e409ef`).
- F4: Cite chapter 09 rule IDs in the fixtures that already test them (`835773865`).
- F4: Report unknown-method for a missing associated function (`1fa8a3a5b`).
- F4: Fixtures for uncited chapter 09 rules (`875de2608`).
- F5: Cite chapter 10 rule IDs in the fixtures that already test them (`3c3e91e44`).
- F5: Cite more chapter 10 rules in existing fixtures (`14c5871ba`).
- F5: Report private-type-leak for public fields, payloads, and bounds (`aecd41080`).
- F5: Fixtures for uncited chapter 10 rules (`4bd31db23`).
- Spec: add examples to 01-lexical-structure.md sections that had none (K11) (`1d0882274`).
- Spec: add examples to 06-control-flow.md sections that had none (K11) (`7adb07dfc`).
- Spec: add examples to 03-names-and-scopes.md sections that had none (K11) (`256b4adf4`).
- Audit: Kimi's queue is empty; K11 rest and K12 moved to the orchestrator (`f5e5375e4`).
- Audit: refill Kimi's queue: K0 (reset; K11 01/06/03 landed), K11 rest, K12 (usability probe 5) (`2589d2277`).
- F-620: TypeId spells std declarations by their qualified names (`982db3777`).
- F-618: a trait value satisfies a bound on its own instantiation (`e7336c2d1`).
- F-619: T < mut Any requires a mutable-root argument (`1a144f828`).
- F-617: Trait::f() infers Self for an associated function (`85cc954d4`).
- Cache: publish a version's tree before its hash record (`687fdb0e2`).
- hd.sum: read only lines that cli.sum.line and cli.sum.order allow (`d263bea1a`).
- Fixture: the ExitCode test body in termination-report.hd is a closure (`5d9142531`).
- A mut-bounded value has mutable access when passed on (`2978cab62`).
- Trait::f references an associated function, Self from the expected type (`5da01224d`).
- Spec: add examples to 05-expressions.md sections that had none (K11) (`f15143a63`).
- Spec: add examples to 11-requirements-and-suspension.md sections that had none (K11) (`936dd9b35`).
- Spec: add examples to 09-traits.md sections that had none (K11) (`a77a357d6`).
- Spec: add examples to 04-type-system.md sections that had none (K11) (`1bb9e6613`).
- Spec: add examples to 10-modules.md sections that had none (K11) (`6990db1b3`).
- Probe: usability probe 5 log; K12 done (K12) (`ce44eb1f3`).
- Queue: K0 and K11 done (K0/K11) (`927bba8a9`).
- Audit: refill Kimi's queue: K13 (website and playground examples), K14 (usability probe 6), K15 (guide coverage report) (`8b5892075`).
- Guide: website and playground examples all run; K13 done (K13) (`15ceb94d6`).
- Probe: usability probe 6 log; K14 done (K14) (`d5de61454`).
- Audit: guide coverage report; K15 done (K15) (`ee23c4eb3`).
- Spec: merge ten diagnostic codes; entry Err report goes to stderr (SC-Q2, SC-Q3) (`d837d0c41`).
- Fixtures: merged-code markers; CLI cases for the entry Err report (`dbba4e36f`).
- Prototype: emit the merged codes; write an entry Err report to stderr (`b15ba5979`).
- Audit: error-code proposal for the rest of the revamp (E1 Part B) (`e2a1a5fa0`).
- Audit: refill Kimi's queue: K16, K17 (guide sections for the top coverage gaps) (`9322ffa25`).
- Future work: new compiler architecture notes (owner goals; the Agentic Programming Language Arena) (`ad9a6337d`).
- Future work: record the defer decision (keep for v1; revisit with NonEscapable) (`d8a7b4184`).
- Audit: Kimi queue gains K18 (diagnostics wishlist for the new compiler) (`843efce62`).
- Audit: Kimi queue gains K19 (a human guide to the hd command), first (`01f0bc471`).
- Guide: Working With Data, section 1: JSON config (K16) (`7f64622c7`).
- Guide: Working With Data, section 2: doc tests (K16) (`1362cb4c5`).
- Guide: Working With Data, section 3: race! and timeouts (K16) (`18309d5ac`).
- Guide: Working With Data, section 4: file I/O (K16) (`99480d484`).
- Audit: Kimi queue: K16-fix first (missing JSON section, duplicated intro, heading spacing in WORKING_WITH_DATA.md) (`3ad8ac8ce`).
- Website: render guide/WORKING_WITH_DATA.md (fixes CI run 37511552524) (`4acf0ee5b`).
- Audit: Kimi rule: list new guide pages in website/src/pages.ts and run the site test (`4f4b2d66f`).
- Guide: Working With Data, section 5: regex log scanning; K16 done (K16) (`03f305d05`).
- Guide: repair WORKING_WITH_DATA.md; K16-fix done (K16-fix) (`8f9b0fe18`).
- Error codes: apply the E1 proposal (spec, fixtures, prototype) (`8a104e093`).
- Error codes: close the revamp's open issues; delete the proposal (`6f3b0f833`).
- TypeId: name package and single-file declarations (F-620) (`2d9277805`).
- Traits: a trait-qualified call names the declaring trait (`e12974799`).
- Testing: the test runner reports a test's Err on standard output (`48a5cefcc`).
- Future work: prototype baselines (runtime microbenchmarks, check/build latency, suite time) in the new compiler notes (`be3a702b6`).
- Future work: why the integer loop is 11x slower (range iterator protocol, boxed cells, Option allocation) and what the new compiler needs (`fedb3e084`).
- Guide: COMMANDS.md, a task-oriented guide to the hd command; K19 done (K19) (`f91725991`).
- Future work: why map and sort are slow; owner's i31ref note for small integers (`436d23749`).
- Future work: proposed goal metrics for the new compiler, per Arena pillar (`e39975e12`).
- Future work: goal metrics as compiler-only proxies (no agent runs) (`d6672ee64`).
- Guide: Working With Data, section 6: testing the whole program (K17) (`656fae44f`).
- Guide: Working With Data, section 7: publishing a package (K17) (`84fc345d5`).
- Guide: Working With Data, section 8: deeper collections (K17) (`0da0e9a7f`).
- Guide: Working With Data, section 9: points and intervals in time (K17) (`09fb5a63d`).
- Guide: Working With Data, section 10: building text; K17 done (K17) (`3664edf30`).
- Future work: goal metrics as read-only scripts; drop prototype baselines; add pathological-compile and proptest-perf metrics (`445e88cd9`).
- Audit: diagnostics wishlist for the new compiler; K18 done (K18) (`fbdf4eebe`).
- Cleanup (test): trim MIGRATED.md to live rows; delete the Job U timing probe (`8b1b403f6`).
- Cleanup (audit): fold opus.md and perf-audit.md into KNOWN_ISSUES (`47966c09b`).
- Cleanup (future-work): delete HOST_ENTRY_DRIVER.md; trim answered rows (`b582a9c7a`).
- Future work: pathological compile cases measure pillar 1 (compile time is agent wait time) (`0ab12eab9`).
- Future work: pillar 3 measures the user's program; conformance becomes a prerequisite gate (`3187487eb`).
- Future work: pillar 3 gains unit-test-perf and integration-test-perf (owner) (`1ee9c7626`).
- Future work: owner-approved brainstorm metrics (pillars 1-3, incremental-soundness gate); reproducible builds parked (`962316080`).
- Design (D2): hd doc redesign proposal (`3f2fe63f8`).
- Design (D2): owner decision: hd doc prints an outline, hd doc --html builds the site; no queries (`71097292c`).
- Design (D2): owner's final call: hd doc builds HTML and Markdown; no query, outline or server (`69a664c8d`).
- Design (D2): hd doc --open opens the HTML index in the browser (`b968533a4`).
- Design (D2): why no agent mode: agents read source (git-distributed) and build/doc/md (`d3b136347`).
- Design (N1): host capabilities and permissions (`e5e733721`).
- Design (N1): owner feedback (grant-all default, gate every capability, two deny tiers, same-origin playground, rename) and proposals (`67a1dc6ec`).
- Design (N1): total deny is a startup error from the Wasm import list (hd run takes prebuilt Wasm); keep sockets (Net); approve the rest (`382a3c1d2`).
- Design (N1): the grant table is [capabilities], keyed by trait names (owner) (`6887a547a`).
- Cleanup (owner, 2026-10-06): delete job1 profiles, test/MIGRATED.md, future-work/PACKAGES.md; hd doc layout (`df92cc40f`).
- Audit: refill Kimi's queue: K20 (## doc summaries for lib/std, comment lines only), K21 (probe 7 on the new guide pages), K22 (fix what it finds) (`ed3b98ed4`).
- Spec (N2): host capability grants, std.http, std.net, std.sys (owner, 2026-10-06) (`eef209797`).
- Issues (N2): open points after the host capability spec; mark the design applied (`bd3afc4cc`).
- Website (W1): implement the Refined design (owner pick, 2026-10-06) (`bec179395`).
- Prototype (N3, session 1): std.http, ScriptedHttp, the Http host provider, and --cap Http= (`e661c2d37`).
- Prototype (N3, session 2): capability grants, scope checks, the test grant, and the startup refusal (`34916323a`).
- Future work: host-capability items the new compiler inherits beyond the prototype (`3dc6bc9a8`).
- Prototype (N3, session 3): Process behind its grant, the playground's same-origin Http, and the REPL refusal (`837a992fc`).
- Design (N1): Http stays its own host trait, not layered on Net (owner) (`343501bc2`).
- Prototype (B4): a trait-qualified call takes a bounded or trait-value receiver (F-624) (`7ee7c3140`).
- Prototype (B4): hd test reports a test case's .Err and its cause chain on stdout (F-625) (`1d9eda2ee`).
- Prototype (B4): cite types.num.binary.literal-join, not the retired rule (`7365fd2e4`).
- Spec (B4): module.init.histories cites REPL and Determinism, not a retired file (`c2adedbc1`).
- Future work (B4): move Iterator Performance into the new compiler's notes (`b4d8b16df`).
- Metrics (M1a): shared metric harness and the pillar 1 scripts (`19bb294a7`).
- Future work: architecture brainstorm from the metrics (big features A-E, per-pillar ideas, correctness tooling); owner notes (`e3f809d7c`).
- Spec (R1): hd FILE.wasm runs a prebuilt module that hd build wrote (`9d77d0b3f`).
- Prototype (R1): hd FILE.wasm runs a module that hd build wrote (`91cade6ec`).
- Future work: owner requirement: multiple backends (Wasm, Cranelift; LLVM/JS low priority) (`98d1d51d6`).
- Future work: owner must-haves: parallel checking, incremental checking (`47e5065cf`).
- Future work: candidate feature set for the new toolchain (all in until cost says otherwise) (`65cb7464c`).
- Future work: program database replaces the language server as the agent-facing feature (owner) (`df1eb3b7f`).
- Future work: pillar 1 features approved; owner adds tiered compilation and module hot reloading (`50b435dd6`).
- Prototype (B6): scope derive origins and map-key implementations to one compilation (`e123a72ea`).
- Prototype (B5): hd test reports every failure line once, on stdout, with its own reason (`1d72b2376`).
- Playground: keep the failure reason in a failed test case's line after B5 (`4446eb682`).
- Future work: pillar 3 features (all in, triage later) (`7ebe78644`).
- Audit: Kimi back; K20 resume note (re-check the uncommitted collections summaries; List.pop was wrong) (`0a4933625`).
- Future work: pillar 2 features (all in, deprioritized, triage later) (`b7419063d`).
- Future work: more pillar 1 features (all in, triage later) (`d6ab45baa`).
- Docs: ## summaries for every pub declaration of lib/std/collections.hd (K20) (`4dfeacb0a`).
- Docs: ## summaries for every pub declaration of lib/std/text.hd (K20) (`faa5f93f3`).
- Docs: ## summaries for every pub declaration of lib/std/json.hd (K20) (`50a5ac542`).
- Docs: ## summaries for every pub declaration of lib/std/fs.hd (K20) (`6fea3de63`).
- Docs: ## summaries for every pub declaration of lib/std/time.hd (K20) (`131a9afb3`).
- Docs: ## summaries for every pub declaration of lib/std/testing.hd (K20) (`702a22979`).
- Docs: ## summaries for the pub declarations of std.testing.arbitrary (K20) (`c2c19067a`).
- Future work: restructure NEW_COMPILER_ARCHITECTURE into one deduplicated outline (`9f41dce7d`).
- Metrics (M1b): the pillar 2 scripts, compiler CPU and memory (`9e0edb280`).
- Docs: ## summaries for every pub declaration of lib/std/structure.hd (K20) (`87b360d9d`).
- Docs: ## summaries for every pub declaration of lib/std/ops.hd (K20) (`1b9f5d35a`).
- Docs: ## summaries for every pub declaration of lib/std/num.hd (K20) (`2f2b306e2`).
- Docs: ## summaries for every pub declaration of lib/std/format.hd (K20) (`85db70d9d`).
- Docs: ## summaries for every pub declaration of lib/std/regex.hd (K20) (`491a56d48`).
- Future work: triage the pillar 1 features of the new compiler (`cb8fc9544`).
- Future work: triage the pillar 3 features; v1 targets Wasm only (`74efb2f89`).
- Future work: triage the toolchain-wide features (`36eef5bad`).
- Test: read the structure.hd location from the file, not a fixed offset (`ee7888d07`).
- Docs: ## summaries for every pub declaration of lib/std/iter.hd (K20) (`a2585d7e4`).
- Docs: ## summaries for every pub declaration of lib/std/cmp.hd (K20) (`5bc8ad815`).
- Docs: ## summaries for every pub declaration of lib/std/cli.hd (K20) (`3f6fbaa1c`).
- Docs: ## summaries for every pub declaration of lib/std/random.hd (K20) (`4372237a0`).
- Docs: ## summaries for every pub declaration of lib/std/console.hd (K20) (`606b4fe98`).
- Docs: ## summaries for every pub declaration of lib/std/process.hd (K20) (`6d4647b2e`).
- Docs: ## summaries for every pub declaration of lib/std/serde.hd (K20) (`f2ef14f00`).
- Docs: ## summaries for every pub declaration of lib/std/result.hd (K20) (`a741590e4`).
- Docs: ## summaries for every pub declaration of lib/std/error.hd (K20) (`32a69b3aa`).
- Docs: ## summaries for every pub declaration of lib/std/host.hd (K20) (`49d6b9bf8`).
- Docs: ## summaries for every pub declaration of lib/std/option.hd (K20) (`c3768c9d7`).
- Docs: ## summaries for every pub declaration of lib/std/hash.hd (K20) (`2548929a1`).
- Docs: ## summaries for every pub declaration of lib/std/annotation.hd (K20) (`018431b83`).
- Docs: ## summaries for every pub declaration of lib/std/encoding.hd (K20) (`53f07b6d3`).
- Docs: ## summaries for every pub declaration of lib/std/path.hd (K20) (`2a26aa957`).
- Docs: ## summaries for every pub declaration of lib/std/task.hd (K20) (`e1640d218`).
- Docs: ## summaries for every pub declaration of lib/std/digest.hd (K20) (`ef25836b4`).
- Docs: ## summaries for every pub declaration of lib/std/inspect.hd (K20) (`c719c34c5`).
- Docs: ## summaries for every pub declaration of lib/std/convert.hd (K20) (`238806863`).
- Docs: ## summaries for every pub declaration of lib/std/function.hd (K20) (`476afff13`).
- Docs: ## summaries for every pub declaration of lib/std/resource.hd (K20) (`1c9cc995f`).
- Docs: ## summaries for every pub declaration of lib/std/http.hd (K20) (`78d54ea6b`).
- Audit: close K20 (## doc summaries for lib/std complete) (`dd7814cb2`).
- Std: keep Heap.pop's implementation comment above its doc block (`31b0209f1`).
- Future work: API diff, migrate and audit are deferred, not ruled third-party (`8afc093bd`).
- Probe 7: usability of the new guide pages (log only) (`903a4154f`).
- Test: read std locations from the file in all three location tests (`f7ed00bc2`).
- Guide: fix the WORKING_WITH_DATA gaps probe 7 found (K22) (`e40105a97`).
- Guide: add mixed-signedness to the common errors (K22) (`0375d34eb`).
- Metrics: move the process-spawning session test out of pnpm test (`747dd4ef5`).
- Metrics (M1c): the pillar 3 scripts and the correctness gate (`c19b46912`).
- Future work: the new compiler must run in the web playground (`ccd75eb5c`).
- Guide: ? converts an error through From, so mixed errors don't need a match (`a47860acd`).
- Audit: queue K23 for Kimi (guide section on calling an HTTP API and capability grants) (`b40c3915b`).
- Research: new compiler architecture, part 1 (front half) (`63774d3be`).
- Research: new compiler architecture, part 2 (back half) (`df01fe948`).
- Research: Q4b, pre-parsing and header extraction for the new compiler (`06c703b00`).
- Future work: record the owner's architecture answers (questions 1-14) (`0fbaaf178`).
- Cleanup: delete the finished guide coverage report; fix a dead perf-audit link (`eff2dc4f1`).
- Open issues: note that the v1 host ABI is hd's own imports, not the Component Model (`71804f58b`).
- Open issues: write "first release" instead of a version label (`d692889dc`).
- Future work: fix four contradictions the requirements check found (`78eba2d44`).
- Research: known issues of prior compiler front ends (part A) (`4e0a91b22`).
- Design: new compiler pipeline and front half (part D1) (`e64e9976c`).
- Design: back half of the new compiler (part D2), and D1 revised (`440964bff`).
- Docs: move the new-compiler design into future-work/compiler/ and split it by topic (`121aaef0e`).
- Compiler design: record the 2026-10-07 answers; fix the README links of the split files (`0dff9ac1e`).
- Design: type checking for the new compiler (frontend lane) (`8bb6860dc`).
- Design: full data-structure designs for the new compiler (backend lane) (`613c7b7a7`).
- Design: the trait solver for the new compiler (frontend lane) (`85f5c683f`).
- Design: answer the Codex review in the backend lane (`6e861dbb1`).
- Design: answer the Codex review in the frontend lane (`2144bd224`).
- Compiler design: record the third round of 2026-10-07 answers; index the review responses (`71ef2204e`).
- Research: enums as identity-free values, and how cycles still work (`42fe2a998`).
- Compiler design: apply the frontend lane's backend follow-ups (items 15 to 26) (`1fa78c899`).
- Spec S1a: apply the 2026-10-06/07 compiler-design decisions to the language chapters (`bf27a2b62`).
- Prototype: spec-sync for S1a (function identity code, new panic categories) (`b520fabaa`).
- Design records: point links at the rule IDs that S1a introduced (`61fadb590`).
- Compiler design: every enum is an identity-free value; Error drops AnyRef (`005fcef1b`).
- Compiler design: drop the AnyRef restriction on generic methods through trait values (`6a7e8c1a4`).
- Spec S1b: apply the 2026-10-07 compiler-design decisions to the CLI chapter (`2a1fac37a`).
- Conformance: CLI cases for S1b, and polymorphic recursion as a build-time case (`6bffd8d83`).
- Prototype: spec-sync for S1b (obj in the cache directory) (`54bb93677`).
- Spec S1b: keep the cache.obj rule's link valid where the glossary quotes it (`50bd18cb3`).
- Compiler design: trait values are written dyn Trait; drop the dynamic-safety gate (`86359c0eb`).
- Compiler design: Error helpers become inherent methods on dyn Error (`7450f7ded`).
- Compiler design: remove GADTs from the language (`43b7adebd`).
- Compiler design: keep erased dyn generic methods, restrict T to bare or optional uses (`b0fe75ade`).
- Compiler design: dyn generic methods stay erased with type witnesses, no restriction (`54f249b70`).
- Spec S1c: enums and functions are identity-free values; Error drops AnyRef; any method type argument through a trait value (`f65b7e120`).
- Prototype and std: spec-sync for S1c (value categories, find through downcast_val, dbg cycle tracking, method type parameters) (`828448899`).
- Conformance: fixtures for S1c (enum and function identity, value categories, method type arguments) (`7aeb1e922`).
- S1c: one expectation per fixture; citations and design records follow the retired IDs (`7f3cd194c`).
- S1c: template-helper fixture (known failure S1C); the language tour follows the value categories (`e93b33f49`).
- Test: reference-identity no longer asserts an enum global (enums are identity-free, S1c) (`128bc8097`).
- Compiler design: live execution (REPL, replay, resume) on one execution journal (`636192f04`).
- Compiler design: Codex re-review N2, N3, N4, N-B1, N-B6, N-B8, N-D2, N-S1, N-S3 (`a73d5ddf9`).
- Compiler design: Codex re-review N5 witness ABI, N9, N-I2, N-C1, N-C3 (`5fafa0ecb`).
- Compiler design: Codex re-review N6, N-A2, N-B2, N-C2, N-S2, questions 3 and 4 (`8fd54eb86`).
- Compiler design: Codex re-review N-B3, N-B4, N-B5, N-B7 (`d4080780e`).
- Compiler design: Codex re-review N-I1, N-I3 (`f16c79e10`).
- Compiler design: Codex re-review N-R1, N-P1, question 5 (`517027df4`).
- Compiler design: response to the Codex re-review, backend rows (`325ff33a3`).
- Compiler design: the witness cost of builtin downcast through dyn (`b4ef8697a`).
- Compiler design: frontend answers to Codex re-review N1, N7, N8, N10, N-A1, N-T1 to N-T5 (`0e604fc74`).
- Compiler design: frontend answers to Codex re-review N-T6, N-T7, N-D1, N-A3 (`83f892045`).
- Compiler design: frontend files follow the GADT removal and dyn decisions (`5a0afe1cd`).
- Compiler design: frontend response to the Codex re-review through 54f249b7 (`9457dd8af`).
- Compiler design: link the frontend re-review response (`db86a5be2`).
- Compiler design: frontend follows the backend re-review changes on main (`4defee550`).
- Compiler design: closure-listed cache keys, head-hash rank, impl universes per context (`86ccdb8ef`).
- Compiler design: codegen select runs Bind steps; GADT evidence and CallDyn bound evidence out of codegen (`d02ded152`).
- Compiler design: closure CallerRow, deferred slot fills, GADT removal in TIR and data structures (`c55a16fe9`).
- Compiler design: checker scratch table follows type-checking.md §13 (`e319e509a`).
- Compiler design: tests for impl universes, template layout, swapped overlaps and trial growth (`8904e7156`).
- Compiler design: template indentation-width test row (`740ddb495`).
- Compiler design: record the frontend follow-ups applied in the backend response (`744628a74`).
- Compiler design: backend response rows follow the deferred slot fills (`20ae63205`).
- Compiler design: type-checking follows the deferred_fills column, not a builder fill log (`2c048ebd6`).
- Audit: queue Codex C1-C3 (new compiler slice 1 in Rust; back-end prior-art research) (`d317c9bd4`).
- Compiler design: facts are runtime values; REPL redefinition is shadowing (`16dc3b236`).
- Compiler: add Rust lexer, layout and skim slice (C1) (`b224b04c6`).
- Audit: queue C2a review fixes for Codex's C1 (`4f4fd3e89`).
- Compiler design: dyn types bind every associated type; supertrait bindings merge when equal (`52206f21e`).
- Audit: speed targets for Codex's lexer, skimmer and parser (`dc86145e4`).
- S1d (wip): dyn Trait syntax, per-member availability, impl dyn blocks, bare trait types migrated (`5557599ea`).
- S1d: dyn Trait rules, per-member availability, impl dyn, Error helpers on dyn Error (`9b0ca9794`).
- S1d: supertrait binding merge and conflict; dyn prose sweep; known issues (`f0e4b2b1b`).
- Compiler: add parser and lossless green tree (C2) (`3eb3fa575`).
- Audit: queue C2b for Codex (C2's conformance test fails on dyn-mut-order) (`6b7c7fa9b`).
- Audit: queue C2c for Codex (complete parser, zero ledger, then speed); answer its questions (`3452715cc`).
- S1e: remove GADTs (owner, 2026-10-07) (`d1b0d4aa9`).
- S1e: facts are runtime values (owner, 2026-10-07) (`a5aba5fab`).
- S1e: compiler design follows runtime facts (owner, 2026-10-07) (`de00def5b`).
- S1e: REPL redefinition, interruption and rebuild rules (owner, 2026-10-07) (`e6d7b2f19`).
- S1e: format website/src/pages.ts (`294e22652`).
- Audit: C2b also covers S1e's GADT-removal parse fixtures (`67c9ca4ae`).
- Compiler: reject removed dyn and GADT forms (C2b) (`0274c7cfe`).
- Compiler: harden and accelerate syntax slice (C2a) (`2002e08b4`).
- Study: runtime data representation for the new compiler (runtime side) (`1dd2c63c3`).
- Compiler: representation study, compile side (instance counts, Cranelift cache, type-only numbering) (`9ec94dfaf`).
- Audit: queue spike 0c (representation benchmarks) for Codex after C2b (`3c0e34670`).
- Compiler design: items are the logical unit, modules the scheduling unit (no per-item task spawning) (`ea6476479`).
- Compiler design: Body tasks are per module everywhere (task graph never has one node per item) (`afdce187e`).
- Compiler design: the numbers behind module-level scheduling; measure before fixing the split size (`9a57e955c`).
- Compiler: systems performance review of the new design (per-file cache cost, test fan-out, cutoffs, eviction, browser step) (`629c55c3b`).
- Compiler: lowering catalog, data types (merges both representation studies) (`e52b332e9`).
- Compiler: lowering catalog, syntax forms, study inconsistencies and owner questions (`e2fb0e9d6`).
- Compiler: wasm-layout states the lowering design (one representation, A1, numbering, literals, sites) (`ce7ba5892`).
- Compiler: codegen states the lowering design (A1 at collection, specialization, size versus speed policy, stable numbering, packs) (`6010bdd45`).
- Compiler: cache packs, filtered test builds, heap sizing and host copies from the lowering pass (`29685a1bc`).
- Compiler: lowering design as representation plus baseline emission, optimizations as named passes; per-tier rules provisional (`fb8602bc7`).
- Compiler: list the lowering catalog in the design index (`6184a57c2`).
- Compiler: complete slice-one parse conformance (C2c) (`ab758961e`).
- Compiler design: tiered compilation and the pass architecture (dev and optimized pipelines, pass costs, spike T1 to T5) (`dcb78155d`).
- Compiler design: shared string slices, map order relaxed, dead-code re-based after S7, no name section in release (`bb3b23af1`).
- Compiler design: tier decisions (optimizations optimized-only, metric split, --release reused for tests); spike 0c gains T1-T5 (`e325ae838`).
- Compiler: cache entries as sections and packs, last-run record, early cutoff by recorded reads, per-shard eviction, print then publish (`286a0ac7f`).
- Compiler: one unit-test program per package, prog_key from TIR content, positions as anchors (`bd1348c16`).
- Compiler: one body per browser step, lock-free memo and intern hits, measured body sizes (`b6560cf8a`).
- Compiler: one literal table and getter per module, module-local numbers; relocation kinds in the code entry (`0adf98686`).
- Compiler: GC pause metrics, systems measurements per slice, the Mac as an I/O gate, review disposition (`3850a3b7e`).
- Compiler design: apply the tier decisions; check-cost and dev-speed (`cb8a1ac00`).
- Spec: hd test --release keeps checks; pipelines are unobservable (`e27b66957`).
- Spec: map iteration order is deterministic, not insertion order (`eea14a1c6`).
- Compiler design: Go-style string views; map, dead-code and name decisions (`6e450bd1d`).
- std.json: an Object keeps its members in insertion order (`7b496e04b`).
- Metrics: split release-check-cost into check-cost and dev-speed (`081c37eff`).
- Metrics: format check-cost.ts (`989799a5c`).
- Compiler design: erased integers use i31ref when the value fits (`21eda0a18`).
- Audit: queue C2e for Codex (C2c's parser has no expression tree and text-heuristic diagnostics) (`e63b43375`).
- Spec tools: spec coverage reports rule IDs cited by fixtures (`4d04c5353`).
- Conformance: chapter 08 fixtures (data and enums), 84% to 91% cited (`5823c376f`).
- Perf is eyeballed, not gated, during first implementation; the orchestrator takes the parser rewrite (`c71d29a67`).
- Wasm first: V8 is the primary engine; Cranelift tuning, cwasm and Binaryen wait for the optimized pipeline (`b56694e69`).
- Audit: Codex moves to spec and fixture work (S1-S4); compiler/ Rust is the orchestrator's lane (`0d0cd7aff`).
- Compiler: add Wasm representation spike (C2d) (`fc76d91b8`).
- Conformance: chapters 01 and 02 fixtures, 12.8% to 88.5% and 26.2% to 94.8% cited (`8f8f62c46`).
- Compiler: allow clippy::too_many_lines workspace-wide; drop its per-function allows (`19a9aeb2a`).
- Compiler: green main; ledger 7 parse cases the old heuristic parser fails; allow clippy size lints (`5c92b7279`).
- Spec: assign diagnostics to invalid rules (S1) (`f1a266a63`).
- Conformance: cite chapter 03 name rules (`2201179a7`).
- Compiler: ledger S1's four new parse cases for the old parser; Codex fixture jobs run the Rust parse tests (`adc74c9db`).
- Walking skeleton: hello world through every stage to Wasm GC on V8 (`62ccff069`).
- Walking skeleton: data, generic and trait programs on V8; incremental and determinism exit tests (`45c70dfbd`).
- Walking skeleton: A1 class instances with casts, code entries with padded relocations (`1c95b5733`).
- Walking skeleton: hd-walk run and bench harness (`ed8afced5`).
- Skeleton findings: what the walking skeleton found about the compiler design (`a92cccffa`).
- Walking skeleton: clippy fixes in hd_walk check.rs (`84e1d32b4`).
- Conformance: cite chapter 07 function rules (`e3c2afb59`).
- Conformance: cite chapter 11 suspension rules (`e0bdcb743`).
- Conformance: cite chapter 06 control-flow rules (`962623689`).
- Audit: Codex profiles the new compiler after the spec round (P1, standing) (`f61214c26`).
- Conformance: cite chapter 04 type-system rules (`f584c5c88`).
- Conformance: cite chapter 05 expression rules (`5fbc4f1f4`).
- Conformance: cover thin stdlib modules (`abcc4702a`).
- Compiler: split hd_walk into the design's crates; fix SK-1 to SK-3 and the skeleton's slips (`09081c081`).
- Compiler: hd run, hd build and hd bench on hd_driver (`f4a94979d`).
- Compiler design: record walking-skeleton fixes SK-1 to SK-4; SK-5 kept as an owner question (`c36379095`).
- Compiler: allow three pedantic lints workspace-wide instead of per-item expects (`b89ceffec`).
- Compiler design: delete the first-round review responses (`4b8df4a25`).
- Compiler: profile walking skeleton 84e1d32b (`f91622aa1`).
- Audit: Codex builds the compiler stage contracts (A1) (`fc4ce9aa1`).
- Audit: Codex gets small scoped jobs (Q1-Q4); the stage contracts stay with the orchestrator (`f2e6bac8a`).
- Compiler: profile split driver b89ceffe (`fef3ca5a3`).
- Compiler: the hd CLI carries product commands only; the bench is a cargo example (`33718d91e`).
- Q1: raise annotation fixture coverage (`a0212467b`).
- Q2: raise CLI fixture coverage (`dcbffa640`).
- Q3: clarify tuple and vararg spreads (`8f154ac91`).
- Compiler skeleton: hd_base IDs, AppendVec, StableHasher, Fuel, NotImplemented; path trie; DiagBuf (`64c536c73`).
- Compiler skeleton: hd_project (sources, discovery, folder graph, closures) and hd_types (InternPool, unifier, trait solver types) (`123500be3`).
- Compiler skeleton: full TIR catalog (59 tags, operand schema), TirSink builder, text printer and round-tripping parser, verifier (`1fdd12f33`).
- Compiler skeleton: hd_sched (all task kinds, priority, serial/shuffled/stepping/pool executors) and hd_cache (CacheStore, entry framing, disk store, eviction, manifest) (`d6f14c5f5`).
- Compiler skeleton: hd_mono layouts for every type form, A1 classes, instance keys and table, suspension state machine types, pass manager (`d399ae805`).
- Compiler skeleton: hd_host_abi (ABI table, import shapes, handles) and hd_wasm's hd.runtime section (`d436332d8`).
- Compiler skeleton: hd_run (embedding API, poll/wake driver, grants, test case model and release cursor, journal framing) (`2e3beb843`).
- Compiler skeleton: hd_resolve (scopes, exports, orphan rule, stage-B entry), hd_check stages (test overlay, coherence, init order), hd_fmt, hd_run_wasmtime (`00b0b134f`).
- Compiler skeleton: driver runs the full task graph (HeaderCheck, TestOverlay, Coherence, InitOrder); architecture driver with per-stage counts on lib/std; hd_doc, hd_stdpack, hd_web, hd_testkit (`b87eb2dae`).
- Compiler design: footprint table per design-doc section; SK-6 to SK-15 and the lib/std stage counts (`b4c14e391`).
- Q4: add compiler benchmark inputs (`8e1a8b172`).
- Audit: queue Q5-Q7 for Codex (parse-gap repros, thin stdlib fixtures, profile the foundation) (`f28ba6365`).
- Compiler design: reconciliation of compiler/crates against the design at b4c14e39 (`c75a14604`).
- Audit: queue D1 for Codex (apply the reconciliation's 13 design changes and the footprint corrections) (`4037715c6`).
- Audit: drop Codex's fixture jobs (Q6) (`f8ff6ff6f`).
- D1: apply compiler reconciliation (`e572d7b0b`).
- Q5: capture stdlib parser gaps (`9903c53d2`).
- Q7: profile the compiler foundation (`d871baaeb`).
- Compiler: cargo fmt (`98cdb9473`).
- Repo: pre-commit hook rejects unformatted Rust; pnpm install enables it (`5c4722629`).
- docs: record backend and runtime prior art (`85cb3cf72`).
- P1: profile formatted compiler (`2294dc6c2`).
- Audit: queue Q8 (fixture parse-gap repros), Q9 (diagnostic codes generated from the spec), D2 (reconcile after each milestone) for Codex (`a0a1c9ae1`).
- M1: one pipeline on the designed types; per-kind slots, Spawn, rayon pool, disk cache, audited AppendVec (`07c748923`).
- Audit: Q8 no longer uses the removed hd parse (`23314867e`).
- Q8: catalog parser gaps in valid fixtures (`bd2cb58a7`).
- Repo: pre-commit hook also runs CI's clippy step on staged Rust (`54ee8f8e5`).
- Q9: generate diagnostic codes from the spec (`fb24caafc`).
- Audit: queue D2a (reconcile M1 with its six findings) and Q10 (std bootstrap inventory for M3) for Codex (`b4e60d850`).
- Audit: Q9 is done (fb24caaf); remove its section (`91055f6ae`).
- D2a: reconcile compiler design after M1 (`191597b31`).
- Q10: inventory standard-library bootstrap (`f64ca5370`).
- P1: profile the M1 compiler (`e3c6eb6e5`).
- Syntax: one recursive-descent parser over the layout cursor; interpolation tokens; drop line heuristics and validators (`0b3a03eb3`).
- Syntax: tree snapshots, mutation fuzz, parse-gap test; one report per delimiter mistake (`2b068b985`).
- M2: the parser accepts the whole language; lib/std parses 36/36 (`187a85f80`).
- Audit: queue D2b for Codex (reconcile M2; fix three fixture conflicts it found) (`7f6e8b1cb`).
- Audit: queue Q11 (TIR text corpus, one case per instruction) and Q12 (CLI forms versus spec) for Codex; Q10 done (`31479a8d5`).
- D2b: reconcile the M2 parser (`6ce7cb3ff`).
- Q11: add the TIR text corpus (`92baf79f3`).
- Q12: audit CLI forms against the spec (`6c2390c1a`).
- hd_diag: use the generated codes.rs in place of the hand-written Code enum (`087f0b7f5`).
- Resolve and lower every std header; std reaches programs through the prelude (`6c58f366d`).
- M3: std reaches programs: slice 2's exit test, a std-typed sample (`20ea63422`).
- Audit: queue D2c for Codex (reconcile M3; correct std-bootstrap.md) (`c190ebed8`).
- D2c: reconcile M3 interfaces and std bootstrap (`a08df5409`).
- P1: profile M3 std interfaces (`27a82e01c`).
- Audit: queue Q13 for Codex (work estimate from the TS prototype: inventory, coverage, checklists, job list) (`54a4bad45`).
- Q13: estimate compiler work from TS prototype (`1f73a015a`).
- Audit: queue S5 for Codex (shared enum data initializes with its module; owner decision) (`392bc1adb`).
- Open issues: park the owner's @pure idea (bans capturing) beside NonEscapable capturing (`0be620e63`).
- Body checker over every lib/std body construct; table solver; rows checked (`db8921a4b`).
- Body 37/37 on lib/std: defaults, varargs, all!, suspension values, typed facts (`66e28162f`).
- Checker diagnostics: bang calls, unreachable arms, arguments, literal ranges (`fedd7efde`).
- Module initialization: top-level bindings, init facts, definite initialization (`c65f39331`).
- Defer suites make a cleanup Scope; mut-on-primitive and mut-on-tuple (`088ed47f4`).
- Pipes and list and map comprehensions in the checker (`1bf1df431`).
- $.with provider blocks; bounds that fail after literal defaults (`d6e613539`).
- M4a: every lib/std body checks to verified TIR; rows, exhaustiveness, init (`037cd289e`).
- Compiler: the toolchain key carries a compiler build id (`82d0beaef`).
- S5: initialize shared enum data with its module (`73db0c2be`).
- Audit: queue D2d for Codex (reconcile M4a) (`bb11c8ec6`).
- Open issues: park deep immutability (frozen graphs, thread safety) beside @pure and NonEscapable (`3d226986f`).
- Back half: collection crosses into std; a layout-driven emitter (`019f55b7c`).
- M4b: a std-using exit program runs end to end on V8 (`74e2f624a`).
- Audit: queue D2e for Codex (reconcile M4b) (`7ce1f7240`).
- Audit: queue Q14 (hello size breakdown) and Q15 (phase-2 job briefs) for Codex (`8e772e191`).
- D2d: reconcile M4a checker design (`5c1090d7a`).
- Audit: queue Q16 (compiler README map) and Q17 (spec clarifications the new parser raised) for Codex (`ade856372`).
- D2e: reconcile M4b backend design (`32d0f278a`).
- Q14: attribute the M4b hello size (`caa4cfc6c`).
- Back half: module init, suspension state machines and the defer exit ladder (`b0bee5356`).
- Samples: module init, suspension and defer, with exact output on Node (`6a5febbef`).
- M4d: init, suspension and defer run end to end on V8 (`f288b05c6`).
- Audit: queue D2f for Codex (reconcile M4d) (`d9e76b355`).
- Q15: write phase-2 compiler job briefs (`212f4de93`).
- Audit: queue Q18 (conformance runner as a cargo test), Q19 (diagnostic rendering versus the spec), Q20 (one known-gaps backlog) for Codex (`94f0e3624`).
- Q16: map the new compiler (`86212ea83`).
- Q17: clarify layout and parser recovery (`94e641a7b`).
- Audit: queue T1 (CI gates for the new compiler), T2 (fixture phase-tag audit), T3 (rule coverage by the new compiler), S6 (hd test output format) for Codex (`b48b53dcb`).
- D2f: reconcile M4d suspension and init (`103dd1e38`).
- Q18: add new compiler conformance gate (`39a367d3d`).
- Q19: compare diagnostic rendering with spec (`b824c3cf6`).
- Q20: consolidate compiler gaps backlog (`bd21eb841`).
- T1: gate new compiler in CI (`95bca03f7`).
- T1: disable automatic package-manager cache (`5fd1b2eaf`).
- Back half: M4d's four gaps, std.rt entry reports and the test plan (`b8f8eaa94`).
- M4c: hd test runs tests: blocks end to end (`4c4b3b922`).
- Audit: queue D2g (reconcile M4c; phase 1 complete), S7 (use lines in tests: blocks), T4 (conformance triage for phase 2) for Codex (`759512bfb`).
- T2: audit fixture diagnostic phases (`4e8cf506a`).
- T3: report new compiler rule coverage (`577ba96a3`).
- Audit: queue P2 for Codex (side-by-side timing, TS prototype versus the new compiler) (`fa5e234dd`).
- S6: specify hd test text reports (`af2849cd7`).
- D2g: reconcile M4c; phase 1 complete (rescued from Codex's worktree) (`18f03371c`).
- Audit: Codex died; its open jobs (S7, T4, P2, D2, P1) move to audit/muse_task.md (`05118a007`).
- S7: specify use lines inside tests: blocks (`561efcfb7`).
- T4: triage conformance buckets to phase-2 jobs (`86fab98c3`).
- P2-1a: runner runs tests: blocks; operators, loops, bindings; total 497 -> 1022 of 2938 (`022cdadeb`).
- Checker: a pattern that does not fit its type skips the refutability cascade; ledger T2's alias case (`044207f99`).
- Run scripts: an entry module with top-level statements and no main (`6c0204be5`).
- hd run FILE: build one file when no hd.toml is above it (`6d277a792`).
- Unsupported checker errors name the file and position of the construct (`60f37949e`).
- Audit: Haiku usability probe (5 programs from the guide; mistakes logged; old vs new compiler timings) (`4263df603`).
- P2: side-by-side TS prototype versus new compiler (`fcc4566bc`).
- Audit: queue T4b (triage on current numbers), D2h (reconcile P2-1a and the Haiku fixes), S8 (where req.row.alias.no-mut is detected) for Muse (`99f3d0382`).
- Audit: queue S9 for Muse (tests: imports may shadow; owner decisions on S7 and Q17) (`691c7d083`).
- T4b: retriage on current numbers (1,055 pass) (`630d782ee`).
- D2h: reconcile P2-1a and the Haiku fixes (`1dfee837c`).
- CI: drop the TypeScript prototype's jobs; keep the Rust compiler and the spec check (`6df631998`).
- S8: req.row.alias.no-mut is a type-phase check with its own code (`6cf2a0349`).
- Cleanup (audit): delete five superseded profile reports; the M3 report is current (`7e9345c55`).
- Cleanup (open issues): drop the resolved GADTs entry; the spec holds grammar.enum.no-result-type (`d972fdd17`).
- Open issues: the host boundary keeps the field tree (owner, 2026-10-07) (`96ee175e6`).
- Open issues: reopen boundary encoding through the consent; field-tree crossing bypasses a hand-written consent's redaction (`73e026bec`).
- S9: tests-block imports may shadow (owner decision) (`a58bb9b07`).
- Audit: queue S10 for Muse (consented types cross a boundary only through their consent; no type chosen by data) (`f2740f282`).
- Audit: S10 also drops the term "consent" from the spec (owner: a bad term) (`0056a8914`).
- Audit: queue S11 for Muse (spec terminology sweep: no invented words; owner) (`d95656514`).
- P1: profile post-M4c compiler (Body +109%, Emit +156% on breadth) (`b9d08239d`).
- Fix reassigned := bindings in samples, benches and the bench generator; benches declare $ Console (`55bae2b4b`).
- Bench generator: names that cannot collide with prelude types (f32), and main declares $ Console (`8393d07fa`).
- S10: boundary crossing goes through serialize/deserialize; drop consent (`4c6b64c47`).
- P2-1b: access permission, variance, trait signatures; no-diagnostic 509 -> 354; total 1022 -> 1178 of 2942 (`f4ac0a158`).
- Audit: queue D2i for Muse (reconcile P2-1b; two fixtures that contradict the spec) (`847b836d4`).
- Audit: S11 also renames the consent-vault test profile (`faa2d4147`).
- S11: terminology inventory keeps all 186 terms; STYLE names the rule (`7c77f888f`).
- Audit: queue S11b (terminology sweep redone with evidence), S12 (glossary hygiene), Q21 (writing-log diagnostics audit) for Muse (`6f4d6d9a6`).
- hd_cli: run and build take the spec's forms (cli.run.*, cli.build.*) (`cd2085141`).
- D2i: reconcile P2-1b; fix two fixtures contradicting the spec (`acff6c503`).
- S11b: terminology with evidence; ghost entry becomes stale entry (`5a9693b68`).
- Header-stage findings get real source positions (`c20e4ca6c`).
- Conformance: drop a stale pass-list path (D2i moved unused-cold-suspension to typing/invalid) (`6c8867358`).
- S12: glossary hygiene, every entry linked, duplicates checked (`24a18f1bc`).
- Q21: diagnostics from the writing log, checked on the new compiler (`f273584a6`).
- P1: profile current compiler, flat since last report (`e2c30615a`).
- missing-trait-method: report trait methods an impl omits (`4e5c96181`).
- Audit: queue S11c for Muse (apply five term replacements the orchestrator decided; list other compound coinages) (`0d943414c`).
- P1: profile missing-trait-method compiler, flat again (`1c84bdc21`).
- S11c: apply five term replacements, retire 27 rule IDs (`ccaf92560`).
- coherence: skip impls the orphan check rejected (`9e65c999b`).
- Audit: queue S11d for Muse (eight more term replacements, orchestrator decision) (`301da30ef`).
- decorators: check targets (decorator-target-kind) (`12d1d2c78`).
- literals: suffixes, string prefixes, negative literal ranges (`f3addbbc4`).
- literals: default types per spec (bare usize, signed i32) (`2eeba4a08`).
- Fixtures: write +N where a bare literal returns as i32 (types.literal.local.default) (`f580ff4fa`).
- diagnostics: render severity, line:column and code per spec (`0116cd37f`).
- embedded parts: member promotion (ambiguous-promoted-member) (`499c78ca7`).
- Audit: queue S13 for Muse (embedded fields count as promoted members) (`34384382f`).
- diagnostics: messages no longer repeat their code (`1e3342d40`).
- rows: row parameters, closure rows and row subsumption in the checker (`81e10e773`).
- rows: providers per instance and a provider context for closures (`3b46ef2ea`).
- rows: the list cursor takes the closure context; conformance 1309 (`2b7738e0b`).
- rows: canonical key order is content order (determinism) (`ffd0d7ad3`).
- determinism: content encodings for params, associated types and trait-value bindings (`8e4e85451`).
- projections: normalize T::Item in checker and mono; bindings infer and check at use sites (`8404af722`).
- projections: driver test for S::Item across two impls and binding mismatches (`5cf1645e4`).
- projections: a binding check skips a bound that already failed (`0d33478a4`).
- assoc types: conformance 1321 (projections, bindings, trait-value projections) (`21aee16d1`).
- emit: StrIndex (string indexing and slicing on views) (`7fcadeb74`).
- emit: shared cells for mutably captured locals (`83fe81996`).
- emit: char and byte-string intrinsics (char_scalar, char_from_scalar, bytes_concat, string_from_bytes) (`79ddadaaf`).
- hd check: the check command per spec (`9330c6ce8`).
- Audit: Muse is back; queue D2j (reconcile overnight compiler work), Q22 (runtime vs Node), Q23 (mistake corpus and diag-location), S14 (hd check summary spec) (`570da89c6`).
- S11d: apply eight term replacements, retire 34 rule IDs (`7e5ab9136`).
- S13: a part's embedded field counts as a promoted member (`3694b104b`).
- Spec: names.promote.embedded says embedded fields are always public, not never pub (`4f6af2a12`).
- pool: one flat extra column and allocation-free dedup (`f84972acd`).
- pool: 64 dedup shards and a per-thread read-through table (`d92ce9b53`).
- unify: union by rank, undone by the trail (`793154ec4`).
- pool: list_items lends a slice instead of copying (`b16b1fb8b`).
- pool: inference variables live in a body-local pool (`97ef2c303`).
- solver: owner lookup, candidate directory and impl universes (`dac36c824`).
- Audit: queue D2k (design text after the pool and solver-lookup work) (`eefd5cd46`).
- known: compiler-known std items resolve once to DefIds (`5a52b2354`).
- layout: std_kind is a required LayoutEnv method (`5e7afe1b4`).
- D2j: reconcile the overnight compiler work (c20e4ca6 to 97ef2c30) (`6572b51dd`).
- Audit: queue S15 (owner decision: mut-on-type-parameter for types.generic.no-mut-t) (`45a21f083`).
- Q22: runtime vs Node: six programs, geomean hd/JS 6.95x (`6699bd49c`).
- Cleanup: drop superseded profile and compare reports; OPEN_ISSUES names Serialize/Deserialize, not consent (`57f3cbac9`).
- Audit: queue S16 (literal style in the Q22 runtime programs) (`7f0cad938`).
- Q23: mistake corpus: 27/40 codes, 29/40 lines (goal >= 95% missed) (`9ffea4162`).
- check: a qualified trait call keeps its written trait arguments (`fa0ee5188`).
- solver: commit on the head, never backtrack among impls (TS-2) (`676b5d403`).
- check: Instantiations and Methods goals; the instantiation choice by trials (`6d891ea3a`).
- Audit: D2k also records the solver commit and trials design gaps (`1a53d8b4c`).
- S16: +N and := literal style in the Q22 runtime programs; Muse queue paused (`8d245fbd4`).
- S15: mut-on-type-parameter for types.generic.no-mut-t (`abf2bd782`).
- Audit: S15 done (abf2bd78) (`13071ec34`).
- solver: memoize canonical goals per body and per run (trait-solver §7, #65) (`9c5a33d35`).
- S14: hd check text summary line and --summary line format (`e30f66cd2`).
- denied-capability: the code of the total-deny startup refusal (owner, 2026-10-08) (`7d02f99d6`).
- solver: scope memo answers that read unowned rows to their universe (#87) (`210f9875c`).
- overlap: a numeric-family member replaces the parameter in the trait arguments too (#88) (`e0384fe6b`).
- resolve: fill trait-argument defaults wherever a trait reference is formed (#89) (`833c83bdf`).
- Fixture: generic-parameter-forms names its unread Sink binding _sink (`b0e18794a`).
- check: header checks ask the solver (#68) (`aa7167598`).
- codegen: select through owner lookup, the head index and a build-long memo (#66) (`38b69f5ef`).
- Spec 03: Tests Blocks example imports test helpers inside the block (`b6a5d361b`).
- Audit: Muse is back; queue U1 (use std.testing inside tests: blocks), D2k extended, T1, Q24, Q25 (`9c97c2d9f`).
- emit: the primitives' intrinsic operator and comparison methods share the operator lowering (#91) (`546ef2d5a`).
- Audit: queue P2 (hello-world size growth bisect) for Muse (`105e7c71e`).
- emit: integer overflow panics with the integer-overflow category (#99) (`a164afdf2`).
- U1 fixtures: move `use std.testing…` inside `tests:` blocks (808 fixtures) (`1cc7a5ce5`).
- U1 spec Markdown: move `use std.testing…` inside `tests:` blocks (`59318cd3c`).
- U1 guide and website: move `use std.testing…` inside `tests:` blocks (`1870d9fb3`).
- U1 rest: move `use std.testing…` inside `tests:` blocks, drop U1 from queue (`77fa7a4a6`).
- check: HeaderCheck findings are cached by hdr_key (#95) (`f7eefc5ad`).
- test_cmd: cart.hd line numbers after U1 moved its std.testing import into the tests block (`324bad4ca`).
- Audit: Muse runs the whole Rust suite for any change under compiler/ or to a fixture (`ad7a57f38`).
- D2k: design text matches pool and solver-lookup code (`6339ed019`).
- T1: bench generator writes a program that checks (`439adb05f`).
- resolve: mut over an unconstrained type parameter reports mut-on-type-parameter (#70) (`ec6de6062`).
- resolve: mut Self is exempt from mut-on-type-parameter (#70) (`7d1407b1c`).
- Fixture: typed-derivation-build bounds its mut parameters with mut Any (`1614765da`).
- Conformance: mut-on-type-parameter passes (#70) (`86344c77a`).
- hd check: summary line, --summary, --max-errors (#85) (`9f0ce24c5`).
- Q24: runtime vs Node with release builds (report) (`b42839606`).
- Conformance: run the CLI tier against the hd binary (#103) (`69f577769`).
- Q25: hd check --tests does not exist; mistake shares with --tests (report) (`c4e173750`).
- Audit: queue R1-R3 (failure triage), D2l, Q26 (runtime hotspots), S17 (release-mode rules) for Muse (`2582415b0`).
- Audit: drop R1-R3 from Muse's queue (the orchestrator runs the triage directly) (`1c7e5df1e`).
- Module paths: use roots for the source, test and task roots and path dependencies (#106) (`2f881253b`).
- D2l: design text after the afternoon's compiler work (`efc99933f`).
- Q26: hotspots in map-count and string-build (report) (`aa7177650`).
- Resolver: a failed use binds its names as poison (#110) (`e7850f3f9`).
- S17: every build-mode rule already states release behavior (table) (`095aa48ae`).
- Audit: queue Y1 (string append design options), L1 (std inventory), D2m (module paths and poison design text) for Muse (`9f7bc2c5a`).
- Y1: rope-vs-builder proposal for quadratic string append (research) (`823fe7c69`).
- L1: std inventory, spec vs lib/std (report) (`4e7621664`).
- D2m: design text for module paths and poison names (`f331909df`).
- Spec: integration test programs, shared test modules and tasks have no module path (owner 2026-10-08) (`3734e2be1`).
- Design: owner decision on string appends — builder now, rope in phase 3 (`5969f4389`).
- Checker: the instantiated-template check at each derivation opt-in (`2f8cf1044`).
- Design: module paths per the owner's no-path decision; L1 findings moved to task #115 (`0bfaba798`).
- Audit: queue L2/L3 (plain-hd std gaps from L1) for Muse (`ba04a7830`).
- Checker: derive-field-missing-trait at the field for derived Eq/PartialOrd/Ord/Hash (`4080e7260`).
- Anchors: a derived implementation copies only its type's own rows (`b98551dd5`).
- Audit: queue T2 (KnownItems test), D2n (derivation check design text), Y2 (join intrinsic proposal), Y3 (derived newtype codegen note) for Muse (`328c2d7b7`).
- Resolver: invalid-error-marker for @error/@from/@source forms (`ee68f49eb`).
- Checker: misplaced-derivation placement rules for templates, derivation blocks and member lines (`23d1a153b`).
- P2: hello-world size growth, bisected (report) (`c0656fe26`).
- Resolver: mixed-derived-law for related derived traits (`790b56fca`).
- Diagnostics: name items as the user writes them, not by stable path (#116) (`23004de4f`).
- Spec: mixed-derived-law replaces the supertrait error on the @derive line (owner 2026-10-08) (`517696e97`).
- Resolver: unused-derivation-fact on a decorator before a derivation block (`e20221620`).
- Conformance: count the new derived-ord-without-partial-ord fixture (fails until #126) (`059bb2924`).
- Audit: diagnostic notes log (owner: log message/code findings, don't change spec or fixtures) (`3399a6b56`).
- Audit: diagnostic notes take over tasks #126, #127, #75, #51 (`c40a4d1e0`).
- Audit: drop Muse's standing D2 and move standing P1 last, so Muse never stalls on a standing job (`1871d4089`).
- Codegen: derived implementations instantiate their template (#119) (`71117825d`).
- Audit: diagnostic note (code-only message); queue Muse D2o (design text for #119) (`4c5013e32`).
- Audit: diagnostic notes take over task #94 (`73d47fa72`).
- Queue: surface L2/L3 blocker questions (Q-L3, unblock call) (`c859c896e`).
- Solver: compiler-supplied trait rows (§3.9); codegen selects them (#121) (`2fc229763`).
- Design: a tuple's Structure is a Builtin answer at codegen, generated per tuple type by the shared Structure generator (codegen.md §13.6) (`18ea3ed55`).
- Audit: answer Muse's L2/L3 questions (dyn in net.md, KNOWN_FAILURES rows for the frozen prototype, checker.rs count, retry_with! stays on the owner's hold) (`6b20dca01`).
- Audit: owner lifted the retry_with!/Backoff hold; queue F1 (main -> i32 fixtures become -> void) for Muse (`826587b5e`).
- L2: add Backoff/retry_with!, Rng::from_seed to lib/std (`f967dae0e`).
- L3: add std.sys and std.net declarations (`76c42b8dc`).
- T2: test that every KnownItems field names a real std item (`14d775c34`).
- D2n: design text for the derivation instance check (`f84e931b0`).
- Y2: single-pass join intrinsic proposal (research) (`9dc9931a3`).
- Codegen: a tuple's Structure is generated per tuple type (#126) (`e5f6cf01f`).
- Design: trait-solver §3.9/§16.2 record the tuple Structure row (#126) (`2fc5870e5`).
- Conformance: regenerate after rebasing #126 onto L2/L3 (1640 to 1657) (`2f4223733`).
- Y3: derived newtype methods reuse the base implementation (research) (`a23fa61b8`).
- D2o: design text for derive codegen (#119) (`68940e14f`).
- Audit: opus_task.md, a second Opus lane for the CLI and project layer (owner 2026-10-08) (`1ac3eda8c`).
- F1 (partial 34/122): fn main() -> i32 becomes -> void where safe (`3b3a9181c`).
- O1: hd check --format json reports manifest and discovery errors (#105) (`a6f907e84`).
- Revert F1 partial (3b3a9181): private main -> i32 fixtures are valid (`c018b4ad1`).
- Audit: Muse jobs R1-R4 (false-error and wrong-run triage, design notes for function values and spreads) (`57bcc3322`).
- P1: profile 3b3a9181 vs 4e5c9618 (#119/#121/#126, L2/L3) (`31d0ce5ed`).
- O2: --format json on hd run, hd build, hd test and hd FILE (#104) (`d789aa202`).
- O3: hd check --tests and --all (#73) (`89a1435a0`).
- O4: test and task code is named by file, never by module path (#114) (`e876c96b6`).
- Cleanup: delete finished audit reports and the retired Codex queue (`e54d9f3a8`).
- O5: hd run and hd FILE refuse a totally denied capability (#86) (`1ed65e395`).
- Audit: Muse Y4, accept concat_all (A1) as a spec row (#112); proceeding on the recommendation (`338c041fc`).
- Audit: withdraw Muse Y4 (concat_all needs the owner's approval); F1 on hold for the owner, not withdrawn (`5d5838005`).
- Audit: owner answers 2026-10-08 23:50 — concat_all A1 approved (Muse Y4 spec row), F1 closed (fixtures stay -> i32), no Muse compiler lane yet (`1dbba3b99`).
- Audit: Muse Y4 follows the primitives table's columns (name concat_all, no @intrinsic text in the spec) (`f4f80de01`).
- Audit: spec frozen (owner 2026-10-08 night) — Muse Y4 removed, both task files log findings in diagnostic-notes.md; concat_all logged, not added (`ef689219d`).
- O6: hd.toml [[executable]], [workspace], [test], [toolchain], unknown keys (#107) (`2b553dea7`).
- R1: triage 160 accept-fails into 38 root causes (`fab67720c`).
- O7: programs leave test code out; the root's dev dependencies load (#111) (`dd035ea00`).
- O8 (help): hd help, hd --help, hd help COMMAND, hd COMMAND --help (#108) (`7f7669f7f`).
- O8 (--release): hd --release FILE and hd test --release (#108) (`20950b915`).
- O8 (new): hd new (#108) (`42dbf1ac8`).
- Emit: recursive type layouts as Wasm GC recursion groups (#120) (`bc654b565`).
- O8 (clean): hd clean and hd clean --cache (#108) (`c703a8a0b`).
- Audit: answers to the Opus questions (O1-O8); the codegen and resolver gaps are orchestrator tasks (`7d808b75b`).
- R2: triage 46 wrong-run accepts (36 exit + 10 stdout) (`6cd221025`).
- O8 (remove): hd remove; commands.md §7.2 says what hd check FILE checks (#108) (`1edf18b67`).
- Audit: Opus jobs O9-O12 (test reports, test environments and #43, capability grants, workspaces); hd_run host side joins its lane (`2424a61a7`).
- O8 (fetch): hd fetch for a selection with nothing to fetch (#108) (`88f04ff66`).
- R3: function-reference adapters (§13.11) + 2 questions (`16e140c3c`).
- Audit: refill side queues — Opus O13-O16 (folder cycles, dev deps in the resolver, doc tests, coherence cache), Muse R5-R8 (missing-error triage; design notes for trailing blocks, Map iteration, default arguments) (`546329ba3`).
- R4: spread lowering (CopyData/NewList/NewTuple-a/call desugar) + emission + 2 questions (`07bd04215`).
- O8 (FILE.wasm): hd FILE.wasm, and program arguments after -- (#108) (`9bc12b3c5`).
- R5: triage 250 missing-error accepts into 21 cause rows (`7e4ce7fc0`).
- Audit: answers to Muse Q-R3/Q-R4; log the copy-update read-time gap (spec frozen) (`81d57b9aa`).
- Audit: Muse R9-R10 (design notes for @error derivation and property/snapshot tests) (`d9da0dd44`).
- Audit: drop finished Muse jobs R1, R2, R5 (reports pushed: fab67720, 6cd22102, 7e4ce7fc) (`68dd9ce1f`).
- Entry selection requires pub main; warn on a private main (`ef8d31f5a`).
- Read main's visibility from header tokens; warn verdict ignores other warnings (`a411c3a62`).
- O8 (fmt): hd fmt; O8 done, hd doc asked (#108) (`76ce3953b`).
- R6: trailing-block lowering (Call+Closure, TestCase registration) + 1 question (`0f4797c65`).
- R7: Map iteration/removal design (§13.12) + 2 questions (`b8e9d31cb`).
- R8: omitted-arg emission via default thunks (§13.13) + 2 questions (`cf14ef5a3`).
- O9: test report summary and repro escaping; triage of the test cases (`3aaecbec0`).
- R9: @error derivation via generated templates (§13.14) + 2 questions (`84135f4df`).
- R10: property/snapshot test design (registration split) + 2 questions (`788067aef`).
- O10: package-role fixtures, hd test FILE scope, untested entries (#43) (`53d3f4fed`).
- Audit: answers to Muse Q-R6..Q-R10; log three spec gaps (trailing-block code, map iteration under mutation, default property case count) (`d0f50e469`).
- Audit: Muse R11-R15 (intrinsic census, fallthrough-trap shrink, design notes for integer widths, supertrait calls through trait values, release profile); drop finished R6-R10 (`15e922a77`).
- Audit: answers to Opus O8-O10 questions (fetch, fmt layout and hd doc wait; checker/emitter stops are orchestrator tasks) (`63e7b307d`).
- P1: profile 788067ae vs 3b3a9181 (O2-O9, entry selection, #120) (`514397323`).
- Check a path used as a value: function and method references (#150) (`a487bdd9b`).
- R11: census of 18 intrinsic-panic programs (7 stubs/groups) (`b5c2d5b76`).
- Audit: Muse R16-R17 (downcast exhaustiveness shrink, static generic call design); drop finished R11 (`94beaab53`).
- O11: capability grants on the host side (cli.cap.*) (`a4d4fca36`).
- R12: shrink 5 fallthru traps to one shared shape (R2 §C) (`d318e8019`).
- R13: integer width sequences (§13.15) + 2 questions (`306a0066f`).
- O12 (-p): select workspace members with -p NAME (cli.workspace.select.*) (`556c19e79`).
- R14: supertrait calls via parent vtable fields (§13.16) + 2 questions (`4e35ff8dd`).
- R15: build profiles/pipelines in codegen (§13.17) + 2 questions (`ba6b7f37f`).
- O12 ([source] root): a package's source root may be another directory (`da8e47714`).
- R16: downcast exhaustiveness is a resolution cascade (R1 cause 4) (`971caecce`).
- R17: qualified associated-call resolution design + 2 questions (`d449ed276`).
- P1: profile d449ed27 vs 788067ae (O10, #150, O11, O12) (`3ec68a300`).
- O13: folder-cycle recovery (#133) (`0dd302198`).
- Emit function references through adapter instances (#46) (`625b56302`).
- Name an instance's kind with `Sub`, not a sentinel sub index (#46) (`596ed7f52`).
- P1: profile 0dd30219 vs d449ed27 (O13 folder-cycle recovery) (`9cd50980d`).
- P1: profile 9cd50980 vs 0dd30219 (#46 adapter Emit) (`4273408ef`).
- O14: dev dependencies in the resolver (#134) (`704644ce5`).
- P1: profile 704644ce vs 9cd50980 (O14 dev-deps in resolver) (`76063c66b`).
- O15: doc-test extraction (#135); wiring waits on #74 (`bee60a57c`).
- P1: profile bee60a57 vs 704644ce (O15 doc-test extraction) (`ee93d4eb8`).
- Audit: refill side queues — Opus O17-O22 (test unit, harness stdout decoding, doc-test wiring and integration env after #74, resolver lookups, workspace leftovers); Muse R18-R22; answers to Q-R13..Q-R17 and Opus O13 (`22d8123cc`).
- O16: a warm run reads Coherence from the cache (#102) (`bb52571e2`).
- R18: shrink 7 wrong-value programs to minimals (R2 §B/§F) (`80610890f`).
- O17: the test unit is one folder per package (`51c1834bc`).
- R19: provider keys carry type args (§13.18) + 2 questions (`6c27ddd54`).
- O18: the conformance harness decodes expected standard output (#136) (`31b6f04e6`).
- R20: row unions at the four join sites + 1 question (`6c74293f9`).
- O21: resolver lowering looks items up by DefId (#118) (`ef1aedbde`).
- O22: workspace leftovers need designs the spec or commands.md lack (`0e53f9548`).
- Audit: Opus O20 takes #74's CLI follow-ups (per-file integration programs, default TestRunner provider, test exit codes) (`bc3c6a8f4`).
- R21: multi-trait method choice (§6.6) + 2 questions (`3df81d448`).
- Check trailing-block calls and top-level test registrations (#74) (`b2e0bbfff`).
- R22: plan remaining Body (186) and Emit (100) stops (`88ec3a4ff`).
- P1: profile 88ec3a4f vs bee60a57 (O16-O18, O21, #74) (`afc95bea6`).
- Audit: refill side queues — Opus O23-O26 (comment-edit keys, InitOrder cache, instantiation depth limit, CLI triage), Muse R23-R27; answers to Q-R19..Q-R21; log loop-join and promotion-tie gaps (`ae0a003ad`).
- Map iteration, removal and non-int/string keys (#124, codegen.md §13.12) (`4bd48f8dd`).
- #122a: data-literal spread (copy-update) in Check and Emit (`dbd6d3ecc`).
- Check: string literal nested in ${...} no longer printed twice (#130) (`53f6a43c2`).
- #32: default arguments at run time (DefaultCall at Collect and Emit) (`08bc15dd1`).
- #122b: positional spread arguments `f(xs...)` in Check (`2fe7c4879`).
- Emit suspending closures through the named-function state machine (#158) (`2bab0b513`).
- Emit: resume test of an if with a bang call in its then block (#146) (`b884ea58f`).
- @error derivation generates Display, Error and From (#137) (`b0ba4f7a6`).
- Design: codegen.md §13.14 records how @error derivation was built (#137) (`0104d75ce`).
- Call supertrait methods through trait values (#131) (`c191c1965`).
- Design: codegen.md §13.16 widening reads the parent vtable field, as built (#131) (`1ff11bd0e`).
- Emit identity comparison `is` and canonical fieldless data (#164) (`e44243038`).
- Lower downcast_val, downcast and downcast_mut (#166) (`d5afa9612`).
- Key providers by the whole trait reference, Repo[User] (#155) (`dccf40bda`).
- Union function rows at least-common-type join sites (#48) (`c544e32ab`).
- Infer omitted result types of private callables in M1 (#80) (`821f969a2`).
- Audit: solver_task.md, a third Opus lane for the trait solver (S1-S8: #76, #77, #78, #79, #84, #90, #97, #83) (`a35ee5c73`).
- R23: local items lift to hidden module items + 2 questions (`3878632f5`).
- Audit: answers to Muse Q-R23 (local items design) (`7f0999de5`).
- O19: Wire doc tests into hd test and hd check --tests (`8ba325a46`).
- O26: CLI tier triage after #74 and O19 (`fbb33e855`).
- Audit: later.md holds parked compiler work, kept out of the priority task list (`ee2604115`).
- Coerce open literal classes into optionals and trait values (#30) (`4e3b1598a`).
- Audit: remove solver_task.md; there is no third Opus (owner), solver tasks return to the orchestrator (`8a3bc3f6f`).
- O24: a warm run reads InitOrder from the cache (#102) (`ffa393747`).
- O23: comment edits reuse the check entry (token-hash check key) (`2833cfef7`).
- O20: the integration test environment of hd test (`dd4498d05`).
- Emit sub-word integer widths canonically (#147) (`c952b221b`).
- Link: always declare memory; export hd.wake only when suspending (`2a69299ae`).
- Audit: Opus O27-O29 (host providers for capability methods, skim cost, rebuild scope of edits) (`1c4b963a9`).
- Audit: wrap up the second Opus lane (offline after O25): O27-O29 return to the orchestrator; answer O16/O20/O23/O24; cache.md records O23's check key as built (`688a4d219`).
- O25: Stop collection at the instantiation limits with instantiation-too-deep (`5f9ea5480`).
- Audit: close the second Opus lane (offline); its open items go to the orchestrator (`8b923c600`).
- Local declarations in bodies as hidden items (#172) (`a238f1c7d`).
- Audit: Muse R28-R29 (design notes for header bounds and row aliases in interfaces) (`5a4608ec1`).
- Hash types as memoized Merkle digests, not trees (#218) (`b8c441594`).
- Host ABI codecs and providers for Args, Env, Fs, Process, ConsoleInput (`1eb9c21c7`).
- Default TestRunner provider: temp dirs, snapshots, hd test --update (`af1b80b90`).
- Test runner's Process: start the package's executables and tasks (`211ff72cc`).
- Cleanup: delete superseded profile reports and the stale stops plan; drop answered question blocks from the Muse and Opus queues (`40324ba66`).
- Spec: trailing-block-ineligible for a trailing block on an ineligible call (owner, 2026-10-09) (`60d16179d`).
- Audit: log the spec change made against the freeze (60d16179) (`a9359f847`).
- Audit: later.md, a spec audit pass collects every spec change (owner, 2026-10-09) (`0aedb01e8`).
- Enforce field, method and declaration privacy across modules (#208) (`d78087e14`).
- std.testing property and snapshot API: it_each, it_prop, it_prop_with, snapshot (#177) (`e8f49f498`).
- R24: small Body gaps table (8 constructs with rules + TIR) (`348e2adef`).
- R25: type names in bodies resolve first + 2 lowering rows (`58ee4d9b1`).
- R26: 17 proposed [check] task rows for unmatched R5 causes (`d67b01dfa`).
- R27: sample/prog runtimes; map-count fixed 60x to 1.1x (`6393e5b72`).
- R28: header bounds environments (§4.5) + 1 question (`0db6bb1f5`).
- Check the items of a tests: block as test-only module items (`289f90ef7`).
- Conformance: check typing fixtures with --tests, as the README says (`1f4577c04`).
- Conformance: regenerate after checking typing fixtures with --tests (#225) (`c01d49610`).
- R29: row aliases in interfaces (record/expand/hash/print) (`e6de8fa3d`).
- P1: profile e6de8fa3 vs 88ec3a4f (test overlays, doc wiring, more) (`28c06b5df`).
- Muse queue: R30-R34 (test bodies, trait-value associated types, arity-generic fn types, private-fn rows, re-triage); Q-R28 answered (`f78696a04`).
- TS: call the new prop_case!/prop_with_case! signatures; property bodies take no row (`16fc0301a`).
- dbg prints its lines with call site and structure (#180) (`bb68f14a7`).
- Conformance: regenerate after dbg printing (#180) (`0e39f01d9`).
- Check and compile the power operator ** (#173a) (`8958267ca`).
- Conformance: regenerate after the power operator (#173a) (`23cac7774`).
- Checker: open range patterns and the full-range expression (`6e4c0d673`).
- Conformance: regenerate after open range patterns (#173b) (`8731028cc`).
- std.format: cite real rule IDs in dbg_show's comment (spec check red since #180) (`3c279c133`).
- TestRunner timeouts: report_timeout fails a body past its timeout (`138ad0f9f`).
- Random host provider: next_u64 and fill from the OS source (`c9a27c465`).
- Sys host provider: os, arch, hostname, cpu_count by grant (`265ae006e`).
- Http host provider: send! under the grant, tuples at the boundary (`c8625c89d`).
- Net host provider: lookup, TCP and UDP sockets as host handles (`f7596a629`).
- hd_wasm: HandleSlot takes helper tag 33; PowF64 (#173a) already holds 32 (`3f2f201be`).
- Profile host traits: Gauge, Sensor, Vault in the fixture runner (`2a24e0336`).
- Conformance: regenerate after the host providers (#222) (`53275671d`).
- Tuple rest elements: spread patterns and rest tuple values (#173c) (`3690e0aa6`).
- Check and compile assignment through a call (#173d) (`01f2070fa`).
- Conformance: regenerate after tuple rest and call-place assignment (#173c, #173d) (`556247325`).
- Lower row aliases and $.Context types in headers (#175) (`2e0990dd3`).
- Design: as-built note for row aliases and $.Context (#175) (`c55f72322`).
- Conformance: regenerate after row aliases and $.Context in headers (#175) (`f076c1b4b`).
- std: every std Walker implements rest; bounded walkers bound T as member bounds F (`835af8ad1`).
- Qualified Option variants resolve through the alias (#178) (`b714e25c1`).
- A failed match scrutinee reports nothing further (#178) (`d7722ce99`).
- Test downcast on trait values; regenerate conformance (#178) (`8000463ab`).
- Conformance: regenerate after the downcast cascade fixes (#178) (`2bb98002d`).
- Declare TypeId::of and lower it; reject long type-arg lists (`75781de9f`).
- TS: skip std methods marked @intrinsic("name") when loading std.inspect (`eb4b4fbd6`).
- Audit: log the Walker.rest default with a bounded member (#173c, #182) (`77717a2bd`).
- Conformance: regenerate after TypeId::of (#182) (`5f1a299c3`).
- Emit f32 literals from their f64 bits, rounded to f32 (`e9ad6f52c`).
- Lower format_f64 and format_f32 as hd:prim host imports (`a05ebbe10`).
- Lower format_f64_fixed as an hd:prim host import (`183f403f4`).
- Conformance: regenerate after float text; record it as built (#181a) (`e09eebc86`).
- Conformance: regenerate after float text (#181a) (`cba53ddff`).
- Emit f32 arithmetic, negation and comparison (#181b) (`b7558bfbd`).
- Emit float casts: widths, integer conversions, saturation (#181b) (`845c075d8`).
- Emit float % as an exact fmod helper, for f32 and f64 (#181b) (`28efdd4ce`).
- Conformance: regenerate after f32, casts and float % (#181b) (`87f1296f1`).
- Lower string_lower and string_upper as hd:prim host imports (`dc59dee98`).
- Lower parse_f64 as an hd:prim host import (`803b1f0a8`).
- Conformance: regenerate after case mapping and parse_f64 (#179a) (`f808db3e7`).
- Bind impl params in head matches; locate dependency errors (#233) (`db3f2d15a`).
- Serialize walker writes rest with member's bound (#233) (`16e6cde26`).
- Select tuple rest lists and unnamed shared enum data with _N (#187) (`0f535b236`).
- Check bare pipe steps as the call path(value) (#188) (`7b8a7dc7e`).
- Conformance: regenerate after bare pipe steps (#188) (`cba323ef6`).
- Check one-payload variant constructors as function values (#186) (`298dd5ace`).
- Keep a List vararg as a rest element of function types (#186) (`d4ae1af42`).
- Conformance: regenerate after vararg function values and variant constructors (#186) (`4e43d8728`).
- Decode the escaped quote in a character literal (#185a) (`2b77016d1`).
- Negate float literal patterns as floats (#185a) (`b71f68897`).
- Test that a map literal keeps the last duplicate key (#185a) (`87c6995b0`).
- Test nested string literals in interpolation (#185a) (`ba8cc718b`).
- Conformance: regenerate after the char escape and float pattern fixes (#185a) (`f328a3036`).
- Give slice bounds usize and pick the slice by the key type (#191) (`28866b309`).
- Normalize a projection whose self type is still being inferred (#191) (`1fae0db0c`).
- Give slice panics a message and test slicing (#191) (`13bb0cf8e`).
- Conformance: regenerate after slicing (#191) (`bbc61dce0`).
- Check and emit spreads in list literals (#196) (`61f369682`).
- Design: as-built note for list spreads (#196) (`4d4fb4c94`).
- Conformance: regenerate after list spreads (#196) (`4753c9d04`).
- Fill method type-argument defaults so collect finds List (#189) (`a464472c7`).
- Conformance: regenerate after method type-argument defaults (#189) (`16902dc66`).
- Give the ? operand its expected-type hint (#234) (`96e0b332e`).
- Conformance: regenerate after the ? operand hint (#234) (`dc099ce44`).
- Design: test bodies, their rows, results and timeouts (#183) (`3ce6fbe24`).
- Run test bodies that use ? and report an Err in hd test (#183) (`7ebc4ae1e`).
- Report a ? error that does not convert as invalid-result-propagation (`7c37d6289`).
- Run an explicit closure as an it test body (#183) (`9fc8b5651`).
- Test the timeout of an explicit it body (#183) (`93971cba3`).
- Diagnostic notes: the text step of cli/test-outcomes (#183) (`b68e97e6a`).
- Conformance: regenerate after test bodies (#183) (`fbc49185c`).
- Design arity-generic function types (#197) (`62599e7e3`).
- Support arity-generic function types Fn[Args, O, $ R] (#197) (`f479e4d7d`).
- Conformance: regenerate after arity-generic function types (#197) (`c37995772`).
- Design context values: layout, building, installing (#229) (`0c533cc9a`).
- Convert each $.with provider to its key's trait value in checking (`fe1dc738e`).
- Build context values with $.context (#229) (`6ae02d132`).
- Spread context values in $.with and $.context (#229) (`d90b593a6`).
- Unify context types as key sets (#229) (`4f7594834`).
- Expect type-mismatch for a $.with provider that does not convert (`4f1a0d95b`).
- Report a provider entry in error once (#229) (`1e97e7423`).
- Key a body's code by its own signature too (`05d0a1265`).
- Conformance: regenerate after context values (#229) (`581c9a25f`).
- Make void the empty tuple type in the type pool (#235) (`8b10e258a`).
- Conformance: regenerate after void is the empty tuple (#235) (`3fde9737f`).
- Dispatch bound calls at a trait-value Self through its vtable (#184) (`53b823f8a`).
- Widen a trait value that reaches a trait-value conversion (#184) (`889f8c526`).
- Conformance: regenerate after generic code at trait values (#184) (`5622365d4`).
- Key trait-value vtables by their associated bindings (#193) (`4661e1ec9`).
- Name the declaring trait in projections and bindings (#193) (`27eca9b31`).
- Generate the members of a delegating implementation (#193) (`dd4ab6410`).
- Reject a trait-value conversion whose bindings differ (#193) (`67b93755b`).
- Conformance: regenerate after associated bindings on trait values (#193) (`671e87979`).
- Check generic key collisions across provider spreads and blocks (`d24c12013`).
- Call a top-level function value by its name in a body (`fbd26c05f`).
- Conformance: regenerate after key collisions and top-level function values (#237) (`fa651178c`).
- Require a trait value type to bind every associated type (#239) (`1996d4fb9`).
- A dyn type fails a bound on a trait with an associated function (#239) (`c63a4519f`).
- Check supertrait bindings in implementations and trait lists (#239) (`4b082f0ec`).
- Reject a second binding of one associated type in a bound (#239) (`7e953d56f`).
- Check an implementation's associated types against its trait (#239) (`5fd70cb08`).
- Notes: associated-type bindings the spec leaves open (#239) (`8dedc5989`).
- Conformance: regenerate after associated-type validation (#239) (`c86d27d23`).
- Pass a trait method's row providers through its vtable slot (`9013e6f7d`).
- Call generic methods through dyn values via erased bodies (`3e20d4e6e`).
- Conformance: regenerate after generic and row-carrying dyn calls (#238) (`5ea44a2df`).
- facts_of returns the Facts of a decorated function (#231) (`cdf58a75c`).
- Conformance: regenerate after facts_of (#231) (`59b431cde`).
- Method lookup considers only available traits (`207896326`).
- Conformance: regenerate after trait availability in method lookup (#190) (`ce588e344`).
- Design M3 inferred rows as built for private callables (`bcc9bc8b9`).
- Infer the rows of private functions and inherent methods (`c5608f3aa`).
- Design: point type-checking.md 5.5 at the as-built M3 rows (#194) (`9788ecc98`).
- List cursors check the captured length (#192) (`77d0b5461`).
- Conformance: regenerate after list iterator invalidation (#192) (`533d058c0`).
- Http sends Host from the URL authority; real servers answered 400 (`abbd5395a`).
- Join a plain branch and an optional one at the final type (#202) (`0c719443c`).
- misc: add sublime syntax (`f78acd3be`).
- Audit: one task file, audit/muse.md, with M1 (parallel conformance runners) (`695e4fad0`).
- Derived newtype methods apply the base method and rewrap (#195) (`18d739443`).
- Design: as-built note for derived newtype adapters (#195) (`66037fd5f`).
- Conformance: regenerate after derived newtype methods (#195) (`25b2b3c96`).
- Audit: Muse M2, run programs in long-lived Node servers instead of one process each (`cc5323ae3`).
- Owner: commit to wasmtime (no Node requirement for the CLI); drop Muse M2, unpark wasmtime (`630236aea`).
- Lower function references to intrinsics, built-ins and dyn as calls (`2fffebbe8`).
- Reference List and Map built-in methods as closures (`05289766e`).
- Audit: Muse M2-M7 (wasmtime audit and spike, host port design, re-triage, release profile, panic frames, spec audit prep) (`d54cc9cdb`).
- Audit: rename audit/muse.md back to audit/muse_task.md (owner) (`f1e33621f`).
- Run the conformance suites on a worker thread pool (`7ed3cf504`).
- Audit wasmtime features and spike the CLI run (`9d8713542`).
- Reject block_on and println written in forbidden contexts (`f823ad4c3`).
- Check the order and scope of generic and parameter defaults (`1f7052c91`).
- Reject written types that omit a slot without a default (`009d6b148`).
- Check omitted derivation members and derived Default variants (`88d065c93`).
- Read top-level bindings in order, through trait dispatch too (`c6aa5db9b`).
- Reject uses of bindings a short-circuit may have skipped (`13c621b6c`).
- Reject a vararg that is not the last parameter (`efb585f22`).
- Reject cycles in the supertrait graph (`5627514d9`).
- Check that implementation heads are well formed (`edaf7c765`).
- Reject `?` that would leave a defer suite (`e37cac7a3`).
- Reject declarations that shadow a prelude name (`965f60462`).
- Require a result type on public callables (`7503e2dd5`).
- Reject a child module path through a used module (`9568df4a2`).
- Reject a written type with more arguments than parameters (`2be4f4577`).
- Check the kind of generic arguments and row parameters (`6e7851ab8`).
- Test that a restarted join site keeps the skipped bindings; format (`5f9caad70`).
- Conformance: regenerate after the missing-error checks (#248) (`de59b4e17`).
- Design the Node host port to Rust (`fa387b6f3`).
- Re-triage the conformance failure buckets (`a490d915b`).
- Complete the release-profile codegen design (`c226eb61f`).
- Design the line table for panic frames (`d1243ebc1`).
- Draft spec audit proposals for all notes rows (`79bf96f4d`).
- Run programs on wasmtime 49.0.2 in hd_run_wasmtime (`4a2b2b243`).
- Add hd run --engine wasmtime and HD_ENGINE, with parity tests (`dacfb29b1`).
- Check the wasmtime host in float_text and conformance runs (`09ed8c5ff`).
- Note the wasmtime engine as built (W1) in engines §18.7 (`7b60511d5`).
- Audit: implementation gaps against the spec, ranked (2026-10-10) (`3a7306eae`).
- Audit: queue Muse jobs M8-M12 from the gap audit (`b67d71a8c`).
- Triage missing and wrong conformance errors (`f6ea8278e`).
- Design the test runner on wasmtime (`0e11c3185`).
- Catalogue diagnostics without a message (`52fba1f96`).
- Add hd_deps: version selection, hd.sum, tree hashes, cache (`869c86e21`).
- Fetch dependency versions with the system git into the cache (`0388068d5`).
- Fetch, verify and build remote dependencies in hd (`676bd2d54`).
- Design the hd doc implementation (`bd2229aa9`).
- Edit requirements in place, select an edited manifest, list tags (`ab3ea5fea`).
- Add hd add and hd update, and tidy hd.sum in every dependency command (`9c6272b7f`).
- Record dependency command conformance and notes (`ce60d7cbf`).
- Probe untested rules of chapters 04-07 (`5469606cb`).
- Fetch dependencies over HTTPS first, then SSH (`da8536ea7`).
- Show the cause of a git failure, not its generic trailer (`80ded89e0`).
- Audit: queue Muse jobs M13-M17 (`df78a6413`).
- Design typed derivation lowering (`434e19ff7`).
- Design package identity in the driver (`496600023`).
- Design resource limits on wasmtime (`3159e201c`).
- Audit: give Muse the wasmtime lane (M18-M21) (`21d990990`).
- Give every lexer and parser diagnostic its own message (`2d230486c`).
- Make hd check report stages that stop as unsupported (`1708258d8`).
- Judge conformance on checks without their unsupported errors (`6917d7436`).
- Run core host providers on wasmtime (`3707b290b`).
- Check variant patterns that mix positional and named fields (`60473bd2e`).
- Check bang calls that write type arguments after the bang (`11eebc07b`).
- Check type names in calls, annotations and trait default methods (`abcd0911b`).
- Run Http and Net providers on wasmtime (`43993c728`).
- Flush prompts before console input on both engines (`91337f6fd`).
- Check and store the copy assignment ...= on embedded parts (`188235586`).
- Record the copy assignment fixtures in the conformance report (`d9806bc3d`).
- Check module-qualified types and variants as values and call bases (`c1ebadc53`).
- Record the module-qualified fixtures in the conformance report (`f7d93cce3`).
- Report a trait used as a value as type-used-as-value (`356f9c10d`).
- Audit: queue Muse M22-M23, hd test on wasmtime and wasmtime as default (`221265f8e`).
- M21: enforce Wasmtime runtime and compiler resource limits (`0f319ffc8`).
- Audit: queue M22a conformance on wasmtime; park wasmtime quality (`c9f11a3f3`).
- Resume suspension points in and/or operands and match guards (`5bfe95ed6`).
- Audit: queue M21b, fix the racy UDP port test (`90e1604a1`).
- M19: fix Node closed UDP handle error precedence (`0613e39f0`).
- M21b: prove network cleanup without reusing freed ports (`3459768f0`).
- Generate derivation facts and share Structure bodies per owner (`19893e21e`).
- Test derived Structure bodies, sharing and field-edit rebuilds (`14b1ea926`).
- Conformance: ten typed derivation fixtures pass (`00018201f`).
- M22a: run conformance entry programs in-process on Wasmtime (`ec5178986`).
- Add Suspend.cancel over the frame's own cancel function (`f48dbe4ba`).
- Conformance: eleven cancellation fixtures pass (`a0ef3174d`).
- Audit: queue M24, Muse reviews its own wasmtime work (`2da2f2c43`).
- Report duplicate declarations the checker accepted (`d580e486e`).
- Check embedding limits, copy markers and member-line forms (`9af5ad5fb`).
- Conformance: cloud session A's declaration checks (#205a) (`bbfe02b07`).
- Check binding chains as a sequence of single bindings (`ac9590e92`).
- Bind a closure's own name inside its body for recursion (`c3f965e82`).
- Type a multi-key $.use as the tuple of its providers (`988e53c13`).
- M22: run tests natively on Wasmtime with per-case limits (`576e07f07`).
- Test in the dev profile, never --release (`dadb063bb`).
- Audit: Muse stops after M24; M23, M16, M17 return to the task list (`b4ba1a9a3`).
- Parse mut names inside a let variant pattern's payload (`960c6157d`).
- Bind a mut field in a let data pattern as the field's value (`e24806887`).
- Audit: add the Linux CI failure to Muse's M24 review (`8997eb5e6`).
- M24: review native host quality and prioritize small fixes (`53f476b4c`).
- M24: clean up failed Process worker starts and fixture races (`48fd496a0`).
- M24: enable compiler heap tracking only for requested caps (`a198c5aae`).
- M24: use native provider errors instead of Node emulation (`daceee94d`).
- M24: bound HTTP and pipe fixtures without readiness races (`7ac92e3cb`).
- M24: centralize reply, wake and termination delivery (`947daed89`).
- M24: record dev gates and complete the host quality review (`8895174b2`).
- Run the pending-first-poll scenario in the driver conformance runner (`2fe9893f2`).
- Bound every git child hd fetch starts with a wall-clock limit (`4fac7643f`).
- Make the hd_cli tests that name remote requirements offline (`b63bb827b`).
- Bound the doubling-type test by exponential work, not one second (`0a8da3d5f`).
- Give the dev-profile Rust CI job 45 minutes (`31513cc01`).
- Check the memory report flag before the reporter parks (`f9aa5ac9a`).
- Exit with libc::_exit after a compiler memory-cap trip (`887ed34b0`).
- Run package-tree test fixtures in the driver conformance runner (`b6ad74a20`).
- Remove each deps test's directory when the test ends (`7a1d6cb8f`).
- Test in release in CI (`3039428c6`).
- Cache only third-party Rust build output in CI (`2cf87b60e`).
- Link the driver's integration tests into one binary (`6113e0128`).
- Share one cache store between driver tests that only read outputs (`4db51f2c5`).
- Link the hd_cli, wasmtime parity and syntax tests into one binary each (`82ff32c37`).
- Split the Rust CI job into lint, tests and conformance jobs (`046a8d721`).
- Build the dev profile at opt-level 1 (`1933643d3`).
- Reject float literals that are not finite in their type (`f6600db72`).
- Put the wasmtime engine behind an off-by-default cargo feature (`fda612f1a`).
- Run CI and the pre-commit lint with all features, document wasmtime (`c1f688e88`).
- Lint default members only in the pre-commit hook (`54cebcf08`).
- Remove each test's scratch directory when the test ends (`5874b720c`).
- Reject unary minus on operands that turn out unsigned (`ef6400ad8`).
- Note the unsigned default under unary minus (`93949965b`).
- Check the operands of `is` once and for compatibility (`502f34734`).
- Reject method calls that read a function-typed field (`3e9730d6f`).
- Count each pipe step's own placeholders and report one error (`48402e638`).
- Check Map key bounds wherever a type is written (`01d4a0bbb`).
- Hold tuple templates only for tuples whose members meet the walker (`5e28dfcb5`).
- Key header checks on user tuple-template sources (`a2efcb4c7`).
- Queue four small jobs for Muse (`84a2136ed`).
- Join least-common-type sites by one routine reporting no-common-type (`f1c2dec92`).
- Keep least-common-type sites per join variable so joins are linear (`b73eaad74`).
- Note two least-common-type readings from #204a (`77782cc19`).
- Make wasmtime host timing tests deterministic (#293) (`dab14d07e`).
- Fix the microbench runner (#297) (`6f03b7121`).
- Inject a T? into a T?? by one layer at assignment and join sites (`b5b538cf6`).
- Note the nested optional join reading from #291h (`ca486dbe4`).
- Keep a function type's row exact inside type arguments (`cd84f5746`).
- Map guest stack overflow to stack-exhausted on Node (M27) (`bcbec8e76`).
- Report no-common-type from a restarted join site (`0f4dbe84b`).
- Map only V8's stack-overflow RangeError to stack-exhausted (`ef9813b67`).
- Measure wasmtime conformance time (M28) (`caa97fddc`).
- Solve a nested row parameter exactly from the argument's row (`7bfb6f477`).
- Queue M29 and M30 for Muse (`89ae40287`).
- Reject user impls of every sealed trait, and sealed member names (`d7ea534e0`).
- Log the owner's hd add and release command requests (`2a692573b`).
- Cache compiled modules in the wasmtime engine (M29) (`1ff24e414`).
- Check impl targets and ownership at the impl header, drop rejected impls (`7a708c467`).
- Note the inherent-impl locality reading from #275 (`42977e7e7`).
- Queue M31: bound the wasmtime module caches (`3292f16c6`).
- Grant a closure its self-name only in statement-form bindings (`4c9b71369`).
- Report cannot-infer-type for variant patterns on an unsolved scrutinee (`913f96863`).
- Record the owner's hd add decisions (`64c467036`).
- Reject never and Inspectable keys where the spec excludes them (`15cd27a99`).
- Note the never-only inference reading from #232a (`0dcddc7c2`).
- Namespace test scratch dirs per process (M30) (`55e1f886c`).
- Bound the wasmtime module caches (M31) (`16b08171e`).
- Queue M32 to M38 for Muse (`69cc45f82`).
- Measure Cranelift opt levels for wasmtime (M32) (`d566492b6`).
- Spec the easy hd add form: targets, queries, inferred keys (`37aef471c`).
- Add CLI fixtures for the new hd add and hd update forms (`9568cb8b1`).
- Drop wasmtime parallel-compilation feature (M33) (`67559b70f`).
- Report what holds 3.5 GB in the wasmtime suite (M34) (`96747a185`).
- Serve CLI fixture remotes from git fast-import streams (`6184277c5`).
- Parse hd add targets and resolve version queries (`214df4820`).
- Implement the new hd add and hd update NAME@QUERY forms (`82ef3bb97`).
- Add success-path CLI fixtures for hd add and hd update (`4a7eedfeb`).
- Note four hd add readings from #307b (`1f5658464`).
- Split createHost into per-provider functions (M35) (`5dc5498b3`).
- Key packages by identity, not manifest name, in the module table (`2c4d4845c`).
- Note the same-name TypeId deviation from #279 (`673994306`).
- Fill declared defaults in derived Default's build, not via the source (`91dfd0837`).
- Note the Default-only declared-default exemption from #284 (`a072f1039`).
- Split http.rs into the http module (M36) (`13a8de93c`).
- Profile closures on wasmtime vs V8 (M37) (`3c5749eaf`).
- Measure Node per-run cost in conformance (M38) (`3967baadd`).
- Check typed facts where they attach and read them through handles (`d1a869e25`).
- Note the Collect-side Inspectable choice from #283 (`bbee987ca`).
- Queue M39 and M40 for Muse (`b0f71d520`).
- Evaluate facts once at run time and report their panics (`4849c24dc`).
- Note three fact-evaluation readings from #285a (`10819dbb6`).
- Report a repeated concrete fact type at its later value (`92c06b09b`).
- Reuse one Node worker per thread for entry runs (M39) (`01d5a3171`).
- Report hd test worker estimate, skip implementation (M40) (`b11a06ff1`).
- Type member-line right sides as list expressions (`5d291bdcf`).
- Queue M41 to M44 for Muse (`23b21f6a0`).
- Lock release profile to Cranelift Speed (M41) (`515c5bdc6`).
- Check every facts body at check time, read or not (#312) (`b8ca40092`).
- Support shared enum data of generic enums, one copy per variant (`ee57a053d`).
- Split net.rs into the net module (M42) (`cf747a9ab`).
- Split fs.rs into the fs module (M43) (`d454d3972`).
- Add P291 probe results for spec chapters 08 to 11 to the gap audit (`56f25fe4d`).
- Review Codex M24 commits, report only (M44) (`0f0d4b551`).
- Queue M45 to M47 for Muse (`2f4517c5b`).
- Count implicit calls in definite initialization (#313) (`b52033a61`).
- Reject trait impl methods the trait does not declare (#314) (`6be82844f`).
- Report private-type-leak at every public signature position (#315) (`4ee1c205c`).
- Validate the entry point at check time (#316) (`4631abb05`).
- Give associated functions no vtable slot and reject them on dyn values (`6622e9ec3`).
- Report Self-naming methods as unavailable on dyn values (`4ff08271e`).
- Report prelude-name-shadow at every binding form (`82a158272`).
- Reject let mut on primitives and tuples at module level (`652673937`).
- Check the call forms of race! and all! at the call (`f770abc9c`).
- Reject PollContext construction in user code via a private field (`e4ff69cef`).
- Reject module paths that collide after case folding and NFC (`789592fd4`).
- Report trait-used-as-type for bare trait names in body types (`2dcfc1630`).
- Fix M44 wasmtime review findings 1-4 (M45) (`1cfe3424a`).
- Panic on block_on reached from defer suites, facts and non-entry init (`b8b9d8b5d`).
- Check type arguments inside written bounds for declared bounds (`49fe06bda`).
- Rearm the compiler memory cap for each workspace member's compile (`6c0f37187`).
- Name raw identifiers by their text without the backticks (`fe137decd`).
- Decode a CRLF in a multiline string as one line feed (`d21402293`).
- Name built-in Map get and remove parameters key (`94b951e0b`).
- Read uppercase radix prefixes 0B, 0O and 0X as integer literals (`af6cb801e`).
- Select a primitive left operand's operator impl by the right operand (`382627f5b`).
- Split core.rs into the core module (M46) (`2e2b527b9`).
- Stop the lost-wakeup test from hanging when recv spends the token (`da89c6e8d`).
- Split host.rs into the host module (M47a) (`454b1d7a4`).
- Split test_engine.rs into the test_engine module (M47b) (`033171a60`).
- Fill type-argument defaults in written types (`603fab1a8`).
- Report partial alias arguments; fill defaults on assoc-fn paths (`f75e934e7`).
- Queue M48 to M50 for Muse (`b308f5053`).
- Report argument-count for too many written type arguments (`ee76eea53`).
- Split lib.rs into engine, instance and run modules (M48) (`9076c18ae`).
- Absorb expressions checked against a poisoned expected type (`bbb7702c7`).
- Poison own-module types written with the wrong argument count (`af8465673`).
- Split testing.rs into the testing module (M49) (`f6e9a7e18`).
- Choose a trait method's impl once inference fixes its receiver (`0f3ae39e0`).
- Run an entry module with no main and no statements as an empty script (`09056c246`).
- Add the flaky fetch test diagnosis to Muse M50 (`34a0d180c`).
- Register test cases by declaration identity, not spelling (`f50c0150f`).
- Move the task list to GitHub issues (`ecb37c5cf`).
- Delete audit/muse_task.md; Muse works from GitHub issues (`e4d290ec8`).
- Remove trait aliases from type: keys are named by row aliases only (`1313b0013`).
- Log two hd-writing mistakes from the closures probes (`7bb59634f`).
- Align CLI fixtures with the spec's JSON fields, help, and test report (`7adabbc4a`).
- Test nonfinal-positional-spread and the vararg-duplicate exception (`9c9644b4d`).
- Fold the innermost ping/pong frames in recursion tests (`0998f4870`).
- Report structure-outside-template for a Structure parameter bound (`aa2960107`).
- Report marker-template for a template that reads no member (`2bdbca15a`).
- Report template-names-binding for a template naming a top-level binding (`fb82440a7`).
- Stay silent when a call's callee type already failed to resolve (`70bb1e5d0`).
- Move the Rust compiler from compiler/ to the repo root (`9895c6ce6`).
- Add an Install section to the README (`16553a7a7`).
- Plan the playground on the Rust compiler built for Wasm (`14b6710d1`).
- Move spikes, docs, tests, and samples into their owners (`e974636ad`).
- Record playground decisions: no_mangle exports, keep WAT tab (`a237b8818`).
- Delete the TS compiler, its tests, and TS-only tools (`56e4b75e9`).
- Plan the REPL on the Rust compiler: crates, cost, questions, jobs (`117291685`).
- Print the playground WAT folded (`e76f16057`).
- Measure wasmtime memory per REPL input in the plan (`ad626bbdf`).
- Add a session link mode to hd_wasm::link for per-input modules (`a5ecd1fe3`).
- Stop session collection at instances earlier modules export (`c2f96a313`).
- Add OverlayStore, a session cache store over an inner store (`92b71c215`).
- REPL: no redeclaration; record owner decisions on the plan (`2f7b03fc8`).
- Add WasmtimeSession, one store that keeps every REPL input's instance (`c6b38ecab`).
- Add Goal::ReplInput, one synthetic module per REPL input (`4091fb251`).
- Share top-level bindings across REPL inputs (`0e3d5aab4`).
- Reject REPL inputs that redeclare a session name (`95e2f2686`).
- Warn variant-binding-name-mismatch for swapped positional bindings (Fixes #108) (`6bde5c47e`).
- Warn unsigned-comparison-always for fixed unsigned comparisons with zero (Fixes #109) (`653d193a5`).
- Show REPL expression values and allow top-level bang calls (`fe4b5c5c2`).
- Warn derivation-line-drift for differing member lines (Fixes #112) (`dd8d4185e`).
- Report unknown annotation members and generic member calls (Fixes #115) (`c25bf435a`).
- Count bound depth from the use and report trait-resolution-depth (Fixes #116) (`6d892ba12`).
- Add a REPL session prelude so small inputs stay small (`76f7ff430`).
- Stop a computing REPL input without ending the session (`7747eb048`).
- Raise the web compiler raw size gate for REPL support (`c3a398800`).
- Infer one join per generic from its first bare argument (Fixes #123) (`4f3da2f1d`).
- Docs: drop hd repl and REPL redefinition from guide, bench, design (`116da3668`).
- Open the REPL when hd runs with no arguments on a terminal (`4f0424d1b`).
- Link scenario builds with hd.cancel, a scenario-only root-cancel export (Refs #56) (`36eab0a20`).
- Poll the root with a driver context and reject competing drivers (Refs #56) (`aa60fcd86`).
- Call the entry cleanup function from a scenario-only export (Refs #56) (`ef37d7ef6`).
- Give hand-built test modules the hd.poll(ctx) signature (`ec2dd021a`).
- Cancel a REPL input that waits on the host (`32188e20b`).
- Treat a REPL session as one scope for member visibility (`673b56c0d`).
- Treat a REPL session as one module for implementation placement (`d8a9f06fd`).
- Keep the wasmtime engine tests' cache out of examples/ (`8438f5d64`).
- Record a REPL session's host calls in an in-memory journal (`1e7e5a181`).
- Rebuild a REPL session from its journal and add :reload (`c4c5c32ae`).
- Colour the REPL input line from the lexer's tokens (`767a83626`).
- Complete names with Tab at the REPL prompt (`a37ac9f49`).
- Measure a 1,000-input REPL session and drive long-session from it (`9b3d68efc`).
- Spec: REPL commands and value stream; spec tools after the TS compiler (`50af3fedb`).
- Read earlier REPL inputs from the session, not per build (`eb661c126`).
- Set long-session to under 100 KiB per kept REPL input (`68f935cc9`).
- Write the REPL value line to standard output (`2e690cda2`).
- Treat a REPL session as one module for derivations and templates (`1f4baf121`).
- Build REPL session modules in the browser compiler over the wire (`857ac3bb6`).
- Run REPL sessions in the JS host with a journal that wasmtime replays (`2fee46556`).
- Add the REPL panel to the playground and site, with Stop and rebuild (`b9e129b32`).
- Format two files my plumbing commits left unformatted (`73d48868b`).
- Save REPL sessions across reloads and share them by link (`776357165`).
- Carry a REPL input's module uses to later inputs (`4f58a4ad4`).
- Let REPL member lines edit an earlier input's type's facts (`3ab604283`).
- Report duplicate-fact across REPL inputs (`f101ce358`).
- Report derivation-line-drift across REPL inputs (`f80077740`).
- Show a REPL input only its own warnings; raise the web gzip gate (`4d4c81671`).
- Plan direct calls of known closures with a cost model (`0957f8450`).
- Build only what a REPL input reaches and load src/lib.hd lazily (`7848962c5`).
- Drive cancellation-cleanup and competing-drivers scenarios (`ae2d23069`).
- Hash the optimized pipeline into code keys under --release (`feecabccd`).
- Call closures made in the same body directly under --release (`fad87c2b2`).
- Drive the reentrant-poll scenario with an active hold stub\n\nA scenario hold can leave the held frame flagged active, so the next\nsame-driver poll reports suspension-reentrant-poll through the real\nflag check. The conformance runner holds the reentrant root active\nand drives the second poll on both engines. The last fixture flips\nfrom unsupported:RunCase to pass.\n\nRefs #56 (`1f7a2f757`).
- Expect a broken lib.hd to drop only the inputs that name it (`662bfcd71`).
- Copy small callees per closure literal under --release (`f8705596a`).
- Carry known closures into the closures a copy creates (`ebdd6ad8a`).
- Record that wasmtime's inliner does not pay in release (`5641bfc36`).
- Store closure code as an abstract funcref and cast it at calls (`d2e81fa52`).
- Offer the dyn fix-it on trait-used-as-type and carry fix-its through the pipeline\n\nBoth trait-used-as-type sites (signatures, bodies) now attach an Exact\nfix-it inserting dyn before the trait name. DiagBuf::append carries\nfix-its, and the check entry encodes them, so they reach build --format\njson; the entry layout version is bumped so the wider entries miss\ninstead of misreading.\n\nFixes #46 (`98cf1945c`).
- Treat a tests-block script as module init under CheckTests\n\nA test run runs no entry behavior, so a module under test is not an\nentry module even when it is a script (module.init.tests.no-entry).\nUnder CheckTests, which checks as a test run checks, a module with a\ntests: block now checks its top level requirement-free; scripts\nwithout one still check as entry.\n\nFixes #122 (`1422980b0`).
- Audit the wasmtime host for blocking calls, deps and std-spec gaps (`0908797a9`).
- Check inherited supertrait arguments on dyn widening\n\nThe Supertrait coercion accepted any def-level supertrait relation.\nIt now substitutes the child's arguments through the supertrait chain\nand unifies the inherited arguments with the target's, so an\nincompatible parent upcast reports type-mismatch (trait.dyn.widen).\n\nFixes #124 (`8e03635be`).
- Select operator implementations by exact function type\n\nA function-typed left operand selects implementations by its own type:\nrow subsumption does not apply (expr.op.left-dispatch.exact-function).\nAfter the solver finds a winner, a function operand whose type differs\nfrom the winner's instantiated self type reports type-mismatch, with a\nmessage suggesting a binding of the implementation's row-rendered type.\n\nFixes #125 (`cc85ccb15`).
- Design optimization pipelines keyed by engine target and level (`93632b89d`).
- Reject non-collection rest and vararg types\n\nA function type's rest element must be a List (types.tuple.rest.list),\nchecked where tuple rest elements are. A vararg parameter's type must\nbe a List, a tuple type, or a Tuple-bounded type parameter\n(fn.vararg.type.kinds), checked at signature lowering under mut as\nelsewhere. An existing default-order example now uses a valid vararg.\n\nFixes #126 (`b4ceeb9dc`).
- Spec host operation rules S1-S12 in the std tier (`84972f113`).
- Reject bare keys in explicit row slots\n\nThe slot of a row parameter takes a row type argument written after\n\$, so a bare key or row alias there is generic-kind-mismatch\n(fn.generic.explicit.row-dollar). fresh_generics validates written\nrow slots; inferred and poisoned slots stay to inference or their own\nerror.\n\nFixes #127 (`cb8dd71f5`).
- Move the pass manager into a new hd_opt crate (`ae183938c`).
- Check inferred method results against impl variance\n\nLowering's impl_variance walks declared signatures only, so methods\nthat omit their result type escaped it. finish_with now checks an\nomitted, settled result's polarity against the markers its parameters\nderive from the impl target (types.variance.surface).\n\nFixes #128 (`0edf70acd`).
- Resolve associated-call ambiguity and undetermined Self\n\nT::f with two bounds declaring f is ambiguous-method\n(trait.assoc-call.parameter.one) instead of silently taking the first.\nTrait::f records its fresh Self for end-of-body closing: still\nunsolved, it is cannot-infer-type naming the Type:: and T:: forms\n(trait.assoc-call.trait.undetermined). A stale qualified-call example\nnow uses the Type:: form.\n\nFixes #129 (`ac7d1030b`).
- Move known-closure calls (A1) into hd_opt body passes (`f22671b91`).
- Check defaults against bounds once, and row fits for ambiguity\n\nA generic default must satisfy its parameter's bounds for every\ninstantiation: header stage B proves each default, reporting at the\nheader, and uses filled from it no longer repeat the error\n(types.generic.default.bound, .checked-once). Fitting requirement\nrows with two distinct key solutions is ambiguous-type instead of a\nsilent first match (types.infer.ambiguous).\n\nFixes #130 (`420cffd91`).
- Resolve misplaced decorator names\n\nThe decorator check now rejects bodiless @intrinsic outside the\nstandard library with unknown-name (trait.impl.intrinsic.std-only).\n@from and @source outside an error type are ordinary decorators that\nmust resolve, instead of falling silent (annot.error.marker.outside).\n\nFixes #131 (`e4cfa2291`).
- Require the Structure import on derivation blocks\n\nIn an implementation header, by Structure must name\nstd.structure.Structure imported with use (annot.block.structure-use);\nwithout it the header is unknown-trait. Test sources that relied on\nthe silent form now import it.\n\nFixes #132 (`031d77428`).
- Add NonEscapable and lifetimes design study (`7db843f0a`).
- Move per-literal closure copies (A2) into the closure_spec pass (`9a2a96ce9`).
- Key pipelines by engine target and optimization level (`7cb9c1f74`).
- Pass each build path its engine target and level (`fbf62a4b3`).
- Add round-2 NonEscapable design and stress tests (`fabf9d96b`).
- Give V8 and Browser typed closure code; keep E1 for wasmtime (`bedca57d4`).
- Rewrite NonEscapable design as numbered rules with stress tests (`405d9df05`).
- Flush held Console output before host waits (`0b6309e99`).
- Reject non-void final expressions in void functions\n\nExpression statements discard their value, so a void tail never met\nits declared type. The final statement of a declared-void function\nbody now reports type-mismatch on a non-void value (fn.body.void-final).\nOmitted results infer, poison already reports, and must-use and debug\ntails keep their own discard diagnostics.\n\nFixes #33 (`bb7af67bc`).
- Fold 2026-10-14 owner decisions D1-D8 into NonEscapable design (`e031a67d3`).
- Lock test-code placement errors with regression tests\n\nMisplaced test cases, invalid test statements, misplaced tests blocks\nand test-only-use of test modules all already report correctly;\nmisplaced-tests-block coverage lives in tests_block_shape. This adds\nthe missing driver tests for the other three families.\n\nFixes #35 (`09923cec7`).
- Rework the wasmtime host fix plan around tokio (`cf4522c12`).
- Add the lazy hd-io tokio runtime and offloaded reactor ops (`71de74b39`).
- Run Process.run! as an hd-io task that kills its group on abort (`bf849b578`).
- Fold NonEscapable owner answers Q1-Q16 into design record (`8f7180bc8`).
- Report 128 plus the signal number for a signal-killed child (`d5a5db695`).
- Check and test the host crates on Windows and macOS (`cf3002b37`).
- Add the adversarial stress test of the NonEscapable design (`2fd33f745`).
- Run Net as hd-io tasks over a host-side socket table (`0ad03ebe7`).
- Fix the wasmtime host crates' Windows lint and test build (`71c24faa9`).
- Use signal trap handlers on macOS so SIGCHLD cannot abort wasmtime (`0a6b5e3cb`).
- Expect the platform separator in the host-config test (`095454560`).
- Close a socket at once and end its pending operations (`ab4297890`).
- Run Http.send! as an hd-io task that closes its connection on abort (`30daff879`).
- Run Fs special files as hd-io tasks on the blocking pool (`1ff3610f5`).
- Wait for a handle a previous due call already completed (`9ba6c29e5`).
- Match path grants by the host separator and drop the verbatim prefix (`79850908d`).
- Run test-engine children on their own threads, off the call (`c5b5aad9c`).
- Wait for the /dev/full write in the Linux parity test (`fea08cf97`).
- Charge host-held data to the program's heap budget (`e099ea609`).
- Canonicalize dependency paths without the Windows verbatim prefix (`90d113b0f`).
- Report every failing host test binary on Windows and macOS (`e032e54af`).
- Parse Http URLs with http::Uri and drop the url crate (`6218367bf`).
- Fix Windows grant paths and fast-import fixture line endings (`ddbc239db`).
- Park closing and apply NE-S decisions to the NonEscapable design (`0af722244`).
- Fix the seven Windows wasmtime failures of the first CI run (`919a76f95`).
- Use ureq's own headers and drop the raw-header transport (`e038981f9`).
- Drop nix, libc and Unix hostname from hd_run_wasmtime (`95100bf01`).
- Display the FIFO path in the mkfifo assertion (`b82419f59`).
- Fix the two Windows fs failures left in the wasmtime host (`579590b3e`).
- Format the mkfifo assertion (`1f6c649df`).
- Charge Fs special-file data to the heap budget (`0f39716ff`).
- Take Http hosts as written on the Node and web hosts (`edf185af7`).
- Pick a free UDP port for the Net test and bind it first (`125a9db0a`).
- Report a path below a file as NotADirectory on the Node host (`dc030b5f1`).
- Feed the balance test one FIFO write per read (`1fc902cc1`).
- Fold the 2026-10-14 owner decisions into the NonEscapable design (`25def5875`).
- Read the page origin from location.origin in the web host (`968a1d850`).
- Expect the OS resolver's answer for a granted 127.1 host (`c8c13e648`).
- Enforce --time-limit on the Node host and exit after a stopped run (`49dd82437`).
- Flush program output before the Node host waits (`851192c6e`).
- Enforce --time-limit in hd test on the Node host (`0a46baa50`).
- Record that fresh handles may reach global positions (`dfb53fe89`).
- End an aborted Http request's socket wait instead of shutting it down (`e4d5a87c9`).
- Add the second adversarial review of the NonEscapable design (`35ff04869`).
- Flatten the cause-chain loop in std error Refs #164 (`1e38dbbac`).
- Flatten the option lookup in std cli parse Refs #164 (`50b78db4a`).
- Flatten the escape arms in std text unescape Refs #164 (`81b6eccfc`).
- Flatten filter and flat-map iterator pulls (`33067823e`).
- Flatten map equality's value lookup (`fc0d21d78`).
- Flatten regex repeat-count parsing (`9969be6b2`).
- Record only a REPL input's own module uses for the session (`d1757baaf`).
- Test REPL member lines across a rebuild and in the browser (`f34084e5c`).
- Emit capture-free closures as immutable constant globals (`dc591a270`).
- Probe the Set once per insert (`c46604c56`).
- Measure and test rebuild scope of body and signature edits (`fbae20621`).
- Read std's skims from a per-toolchain depfiles entry (`177a344b8`).
- Decode TIR bodies lazily, only the ones Collect reaches (`d6fea42ce`).
- Write each code entry's types once, in a table rows name by place (`1e28dc183`).
- Update the guide's command page for this week's CLI features (`85c35ea47`).
- Call decode_vts with one argument in the Json boundary decoder (`388308935`).
- Log the idea of implementing Map in hd (`eaeea5c54`).
- Fold review-2 owner decisions into the NonEscapable design (`6fcce0959`).
- Specify --engine and HD_ENGINE in the CLI chapter (`b5fbe98ec`).
- Add the serializable closure and remote call design study (`c4e553673`).
- Stop the Node test supervisor from cutting a case after its result (`c784a0eb9`).
- Check negated bare literals, erased poly-recursion, and empty projections (`44c14e595`).
- Audit spec coverage and probe 300 untested rules (`7680168b3`).
- Return the account in the dbg value example (`7b70d6762`).
- Require an arm for every enum variant, never payloads included (`9f4cdfe01`).
- Add adversarial review 3 of the NonEscapable design (`80624dd9e`).
- Raise the CLI conformance command limit to 60 seconds (`c843a4f37`).
- Write the CLI conformance limit as one minute (`44890c303`).
- Delete finished audit reports and keep the live notes (`f1c550208`).
- Move compiler design docs to docs/compiler and crate DESIGN.md (`d91250ec1`).
- Delete finished future-work records and later.md (`41d5613e3`).
- Replace CONFORMANCE.md with an exact KNOWN_FAILURES.tsv list (`11a2116e1`).
- Flatten audit/ and scan docs/ for dead rule citations (`66cf21676`).
- Sign the literal under ** negated by unary minus (`2ed53b2b8`).
- Add the write-issue skill for concise, actionable issues (`a58eadf85`).
- Raise the web compiler size gates to 4.9 MB raw and 1.55 MB gzip (`c60caf246`).
- Reject Self outside a trait or impl body (`c2fbd6346`).
- Reject pub on local declarations (`f8649a03f`).
- Report duplicate string, float, tuple and payload arms (`da9a71040`).
- Refresh the entry docs for the new layout (`53a1f947c`).
- Fix factual errors in the quick start and tour lessons (`e73bbeafb`).
- Align the agent rules with how merges and CI work now (`db5015f9d`).
- Check the cross-cutting compiler docs against the code (`07652febc`).
- Check the hd_syntax, hd_resolve and hd_tir designs against the code (`2d812cc46`).
- Check the hd_check design against the code (`1c5105a40`).
- Check the runtime, cache, scheduler, CLI and web designs against the code (`808fc5223`).
- Check lowering, engines, goals and tiering docs against the code (`69ea9916e`).
- Check the hd_mono, hd_opt, hd_wasm and hd_stdpack designs against the code (`cd7b339fc`).
- Restore the executable bit on spec/check.sh (`7bcbf283a`).
- Fix stale cross-references left by the design-doc check (`b2cebd90a`).
- Keep the two-worker playground design with its differs note (`5f94f67ff`).
- Add write-hd and hd-cli agent skills (`130774397`).
- hd-cli skill: hd FILE is a single-file program even in a package (`b3b4381cf`).
- Log four diagnostic findings from today's fixes and the Haiku probe (`e209f15cb`).
- Log a closure-body syntax mistake from the #212 probe (`112785ea9`).
- Log the recommended remote-call design and its discussion for review (`8d75e5857`).
- Point known failures from the coverage work at their bug issues (`618d48f42`).
- Fold review 3 decisions into the NonEscapable design (`3931c8de4`).
- Report item-too-complex when a body's fuel runs out (`1cb160586`).
- Verify TIR invariants 3, 7 and 12 (`c31a9ca3a`).
- Record which all! child each host leaf waits under (`0504cffdd`).
- Raise the web compiler size gates to 5.0 MB raw and 1.6 MB gzip (`afb424a60`).
- Check a module's function bodies in parallel (`7ecf47245`).
- Decode a folder interface's items on first use (`028ae0b1e`).
- Record why interface builds still parse bodies (`e17e99c01`).
- Check in the host ABI model both hosts' codecs will come from (`5cede69c5`).
- Point the coverage placeholders at their bug issues (`0c3f62546`).
- Raise the web compiler's raw size cap to 5.2 MB (`1d423b055`).
- Generate the JS host's codecs from the ABI model (`18725e077`).
- Fold the round-7 follow-up decisions into the lifetime design (`d86f079fd`).
- Stop the e2e's endless loop only once its run worker loads (`05103af42`).
- Send a payload in the parity case of a write outside the grant (`9938ec888`).
- Reframe the remote-call study around places and portable closures (`61ed39f51`).
- Add the ownership-and-borrows design study for the lifetime redirect (`9c4a4b2ab`).
- Redo the ownership study with own written at the usage site (`70a3b79a2`).
- Rewrite the ownership study as a readable proposal (`19f3f25c1`).
- Rewrite the serializable closure study as a readable proposal (`190fdc93c`).
- Delete the NonEscapable design; ownership replaces it (`fbcde2bf4`).
- Log the mistakes of the bug-hunt probe programs (`5704772a8`).
- Add the adversarial review of the ownership study (`90450f29b`).
- Apply the owner's ownership answers and the review to the study (`e43621d30`).
- Record the ScriptedNet stdlib call (`f1bb6d6f8`).
- Use Drop for owned values in the ownership study (`f1f797a47`).
- Record the std.net timeout, option, and local-address calls (`fef708af0`).
- Record the owner's answers to every ownership open point (`bdade3ba8`).
- Add the second ownership review and fix the stale lines it found (`18c6b554b`).
- Apply the owner's answers to the second ownership review (`165d25944`).
- Let a generic holder carry an owned value, which makes it owned (`72333cc67`).
- Reflow the ownership study's appendix sentence (`d7d32438b`).
- Mark always-owned types with the OwnOnly trait (`7e85e38b0`).
- Add the third ownership review (`3559e668d`).
- Settle generic modes and overlapping OwnOnly impls (`00c23454e`).
- Let a defer body call only code that needs no requirement row (`8f0ea8841`).
- Add OxCaml to the ownership study's prior art (`9c367f4eb`).
- Derive generic ownership by Structure instead of conditional impls (`281c34d5c`).
- Tidy the ownership study's appendix sentence (`334aec98d`).
- Add the fourth ownership review (`b4a1619c1`).
- Apply the owner's answers to the fourth ownership review (`b3711481e`).
- Add the fifth ownership review (`be81783c1`).
- Make only owner-capable values move-only in generic bodies (`5f6412d66`).
- Settle guard moves, replace, and the OwnOnly drafts (`5130db6cd`).
- Log bughunt2 writing mistakes (`e73902a62`).
- Show how hd serves the three agentic pillars in the README (`d7226fd44`).
- Name compile time in the README's first agentic pillar (`899540502`).
- Add the sixth ownership review (`e3d909070`).
- Apply the owner's answers to the sixth ownership review (`001c12542`).
- Log bughunt3 writing mistakes and diagnostic notes (`12ab76043`).
- Split the wasmtime host's providers by concern (`70b552e47`).
- Say min and max return the first value on a tie (`843f5f7c1`).
- Apply the owner's nine #166 answers to the closure design (`cc0164c40`).
- Review the stdlib implementation for naive algorithms (`943fa452b`).
- Audit the stdlib against common tasks and batteries (`fb8ace92a`).
- Record the stdlib audit calls (`dd7a2da8a`).
- Sort the diagnostic-notes rows for the spec audit pass (`1f311d1cb`).
- Spec: apply 33 audit wording edits where the code is right (#183 B) (`c914df255`).
- Record group B results in the spec audit and diagnostic notes (`e7f630651`).
- Mark group C of the spec audit with issues #381-#408 (`60e4ffe38`).
- Record the owner's answers to the spec audit and stdlib questions (`10b91b296`).
- Closure design: remote!(f, args), config wins, no pipelining (`70ee02d32`).
- Propose an I/O and host stdlib redesign against the #350 bar (`293b78417`).
- Propose a data stdlib redesign against the #350 bar (`3771fe70e`).
- Revise I/O stdlib proposal for typed interfaces; derive std.cli (`161042b10`).
- Record the owner's answers to the data redesign questions (`9c418be27`).
- Propose a structures and control stdlib redesign against #350 (`7a03e7d5e`).
- Move env reads out of the CLI derive; propose a typed EnvConfig (`a71ac6123`).
- Fix doc comments of List.contains and Result.expect (`b31491b95`).
- Reject a prefixed string as an @error message, D9 (`09e52638a`).
- Report a bound that contradicts its own supertraits, D13 (`64f8ca377`).
- Report a variant payload field matched twice, D14 (`a060e9af7`).
- Report repeated parameter names in an enum header or variant, D15 (`4491ea5a8`).
- Record the owner's answers to the stdlib redesign and #166 questions (`10746c486`).
- Study non-success outcomes and how an error picks its exit status (`d98be4719`).
- Record the owner's answers to the outcomes and exit status study (`f924026a9`).
- Study the six open ownership points as Still To Decide (`02165fe14`).
- Record the owner's answers to the six open ownership points (`ef853b2bc`).
- Clarify that cloning an own value deep-copies plain fields too (`60d5324bf`).
- Plan the ownership prototype in hour-sized slices (`681c97013`).
- Index the ownership prototype plan; mark the design decided (`55a6faffd`).
- Record the owner's answers to the ownership prototype questions (`2c7d56686`).
- Add the Ownership and Borrows spec chapter and its fixture area (`8f0367020`).
- Revert "Remove Duration::milliseconds and Duration::seconds" (`07dc51fd9`).
- Record the Duration constructor and non-negative decisions (`7cb530ccb`).
- Drop a duplicate path-parse.hd row from the conformance manifest (`f0389ef03`).
- Parse own, ^ and lifetime marks; spec their syntax with fixtures (`d0a1a9642`).
- Replace path lifetimes with function lifetime parameters (`32cadf42e`).
- Keep path lifetimes as pub field paths for partial capture (`b99c568e9`).
- Stress-test one Deserialize model for CLI, env and formats (`fb41ae474`).
- Index the unified Deserialize stress test (`7b845792f`).
- Check ownership modes and lifetimes in declaration headers (`7ddb48ed7`).
- Record the owner's answers to the unified Deserialize stress test (`3800f1b7d`).
- Add OwnOnly and DropOwn in std.ownership with their header rules (`b155f590a`).
- Delete finished research files; keep their decisions in the designs (`8f3f65839`).
- Check ownership by structure for generic types (`76fab3f3e`).
- std.num: to_bits and from_bits on f32 and f64 (`1bfe853b0`).
- Check owned and borrowed values in bodies (`4c06f52fa`).
- Find Result.expect and is_ok across inherent impls for JSON decode (`96e6c1ee0`).
- Propose SIMD byte-scan primitives for std (`1ed199a76`).
- hd-conformance.sh: find hd under CARGO_TARGET_DIR when set (`7194167fe`).
- Propose one owned reader and writer shape for streaming (`7bd86cc26`).
- Record the owner's SIMD decisions: names approved, probe first (`1d78c07ca`).
- Record the SIMD layout probe results (`860cfd559`).
- Keep the (array i8) string layout for now (`5d992a516`).
- Reject the ownership prototype's limits and non-plain providers (`7f30ea3fd`).
- total_cmp fixture: build its positive NaN from bits (`a9bc76c5c`).
- Carry drops and moves in TIR for owned bodies (`03506589e`).
- Build the web compiler at opt-level z with compressed relocations (`03fbdcc63`).
- Raise the web compiler's gzip gate to 2 MB (`b1101fe92`).
- Build the web compiler at opt-level s again (`e51d2c7ab`).
- Check moves of owned locals over the finished TIR (`358f94518`).
- Doc snapshot fixture: compare the source with LF line ends (`6bce97745`).
- Give new spec rule IDs to rules whose meaning changed in #507, #510 (`fb4490999`).
- Record owner decisions: host handles, Closed, two primitives (`d4c8a5c34`).
- Record owner decisions: ^mut closures repeat; streaming accepted (`863e7c56f`).
- Cite the new rule IDs after #507 and #510 (retire the old ones) (`bf73e5b91`).
- Record: ^mut closures stay exclusive, for future concurrency (`c3389fe24`).
- Run drop glue at scope exits, overwrites and statement ends (`47978d3ec`).
- Inventory num.md's radix example in examples.tsv (`66a7dc4ff`).
- Record owner decision: resources use Disposable and using (`9cc8b9298`).
- Streaming design: plain Disposable handles bound with using (`db07954db`).
- Study parallelism and locality after the Disposable decision (`69e70a760`).
- Study a native backend against wasmtime AOT executables (`788ef2c11`).
- Plan performance work by mechanism with ranked hour-sized steps (`6a424131e`).
- Record owner decisions on the parallelism and locality study (`689e85a5a`).
- Plan the serializable closure prototype in hour-sized slices (`232b58d6e`).
- Propose fn~ to mark closures that change captured state (`162489165`).
- Record owner decisions: prototype re-plan, native path, perf primitives, suspending Disposable (`db98cf185`).
- Record owner decisions: no changing-closure marker; Disposable cancellation (`e3b152867`).
- Record: native backend work is not first; phase 1 waits (`fd621b91d`).
- Cite the std hash rule for the Hasher methods fixture (`2c5f4d790`).
- Inventory the Hasher methods fixture for the trait block (`d4dc539f2`).
- Record: remote work on hold for the owner's review (`319f5859c`).
- Restore changes that the sqrt commit reverted by mistake (`5fb199c0b`).
- Record owner answers to the performance roadmap questions (`f0c0a11f7`).
- Delete the ownership docs; open each design doc with its problem (`73c0e0478`).
- Parallelism doc: park the local mode; stage A stays the plan (`8d0653fec`).
- Add the Disposable trait and using bindings (`32f81001b`).
- Delete the ownership design docs; point links at what replaced them (`954b42fb5`).
- Net handles use Disposable and NetError.Closed (`5709ad007`).
- Update two tests for std.resource and the Disposable handle traits (`991b29771`).
- hd test fails a test case that leaves host handles open (`8b6e4be11`).
- Remove the owned mode: own marks, freshness and moves (`a04c3497c`).
- Remove drops: OwnOnly, DropOwn, drop, drop glue and DropTemp (`8a77d0273`).
- Remove lifetimes; a borrow mark goes only on parameters and bindings (`101344d4c`).
- Add a study of Rust's memory model and Gc[T] for hd (`a998835db`).
- Propose graceful cancel with a Cleanup and explicit abandon (`ec86f870b`).
- Cancellation proposal: cancel returns a suspension; abandon on any (`5dfd0f6f0`).
- Measure where Rust's speed comes from on Wasm, against hd (`4e5fc735c`).
- Cancellation proposal: cancel!() waits, abandon() kills (`90354ef02`).
- Remove ^ lending: borrow marks, Lend coercions and mode checks (`5d0b21e0a`).
- Cancellation proposal: fold in the stress test's six fixes (`180bac390`).
- Propose value structs as named tuples, with layout measurements (`1403077ec`).
- Cancellation proposal: abandonment scope, cooperative bound, sticky result (`16a5b3ae6`).
- Value structs: keyword, derived equality, no size bound (`6f2d368e3`).
- Keep the line structure in read_all! (`6abab2f4e`).
- Cancellation proposal: names settled, host waits with a default bound (`d60cf385d`).
- Session scripts: hd --repl FILE replaces hd repl --script FILE (`90b380612`).
- Point the Node session known failures at #518 (`78fd5d8c4`).
- Object model tier 1: Go's model on counted linear memory (`0081d425b`).
- Object model tier 1: owner answers folded in (`6fd99b046`).
- Price the reference-counting runtime in linear memory (`7044fe322`).
- Cancellation proposal: the third stress test folded in (`3978c2579`).
- Object model tier 2: opt-in lifetimes on tier 1 (`1e146fa96`).
- Cancellation proposal: an undisposed resource at count zero is a leak report (`caae88e90`).
- Cancellation proposal: dispose dependencies and block_on's result (`c6587abf5`).
- Cancellation proposal: block_on returns BlockOn[T] (`8d22598a5`).
- Cancellation proposal: completed job with an abandoned dispose; println (`1fda5aafd`).
- Object model tier 2: fold the owner's answers (`4fbd83877`).
- Cancellation proposal: kept result after a natural finish; custom providers (`cd8e0bc09`).
- Cancellation proposal: drop links to the retired synchronous cancel (`b9f0a5542`).
- Object model docs: fold in the stress test answers (`95b730b80`).
- Object model docs: let mut stays; &mut needs a writable place (`c278a9623`).
- Object model docs: fold in the second stress test (`6c46c59f6`).
- Object model docs: fold in the third stress test (`2be2beabb`).
- Object model tier 2: migration wording from the fourth review (`0c3250ae3`).
- race! is cancelled as soon as one child turns out cancelled (`f3556ad65`).
- Object model tier 2: migration names runtime borrow conflicts (`d1367220d`).
- Object model: recursion through &, structural derived Eq and Hash (`d5a21771d`).
- Object model tier 1: derived Eq and Hash on every type; deep_equal for cycles (`bb1b7818a`).
- Delete the superseded value-structs study (`e3ff99f6b`).
- Node host: a child's closed stdin reports EOF on Windows (`352d46fa2`).
- test-leak-after-abandon: open the listener before the race (`0112c419c`).
- References print as their values: Debug, Display and dbg through &T (`befb217cd`).
- Fix T8 regressions: dyn suspending generic calls; linear ValueCopy (`b41692a2a`).
- Check out offset-judging CLI fixtures with LF on every platform (`0cf6baad7`).
- Carry parser fix edits through diagnostics and warm checks (`616eb7c06`).
- Offer exact edits for five parser migrations (`372fa89ff`).
- Offer parser fixes for bangs, spreads and documentation (`1f5e5921c`).
- Offer an exact edit for elif continuations (`0263c377f`).
- Offer checker fixes for loop and List.append calls (`5794ee040`).
- Offer syntax fixes for typed bindings, casts and patterns (`c85e64eda`).
- Log the calculator reference-cycle mistake in the writing log (`a4c41fe12`).
- Offer a let declaration edit for local reassignment (`c7311ec60`).
- Offer pub fixes for private entry functions (`af2be753b`).
- Log two object-model stress-report syntax mistakes in the writing log (`d63fbc171`).
- Offer safe fixes for discarded debug calls (`00279d729`).
- Preserve dyn in trait-reference diagnostics (`fac05eb37`).
- std migration 3a: console, fs, process, http, host, sys off data (`cd72ec727`).
- Fix unambiguous empty Map constructor calls (`cb8cc0536`).
- Offer debug formatting fixes for collection interpolation (`85bd4f042`).
- T16 step 1: convert spec examples and fixtures off data; struct embedding (`20cf5f929`).
- Move std's Console to &mut self; host providers behind references on the linear backend (`7aee2a620`).
- Delete fixtures for the removed transitive read-only rule (`a6b590711`).
- T16 step 2: data declarations and composite mut types are errors (`90f25ee94`).
- Remove copy-assignment statements (`3e57bbeba`).
- Shrink linear-backend code: a local cleanup pass and a tighter allocator (`60d4cad9f`).
- List three more linear-backend passes (`29d8a7247`).
- List four #586 merge failures against #592 (`5aaf21884`).
- Unlist two #592-parked cases from LINEAR_PASSES.tsv (`a3afa9d81`).
- List two wasmtime-only deep_eq traps against #593 (`da2c3744e`).
- Park wasmtime in CI until the language is stable (`3b68d495a`).
- Unlist the #593 cases now that CI runs Node only (`c6fd9e159`).
- Ignore the shared-row-body test until #592's regression is fixed (`aeee07caa`).
- Re-enable wasmtime in CI (`b7369cdec`).
- List the #593 deep_eq traps for the wasmtime engine only (`a40570e23`).
- Repair CI after #592 met #587: park 139 linear rows, fix closure_tests (`3cf10c453`).
- Revert "Derive error exit codes and forward transparent errors" (`7f608d71b`).
- Count std.rt.checked in the std body check (`d90b95fa5`).
- Remove get_text! and post_json! from std.http (`6761e225c`).
- Close linear-backend gaps that the default switch exposes (`e7aa2cf94`).
- Make the linear backend the default (`a7402866f`).
- Pin the all! wake-count tests to Wasm GC until RC has targeted wakes (`608ae2144`).
- Expect remove_at's new std line in the panic frame test (`86470d05b`).
- Drop --explain-escapes, --explain-copies and the large-struct warning from the plans (`e8636d4b3`).
- Pass joined REPL inputs to collection; drop the GC session collector (`5317f6d2f`).
- Benchmark the RC backend: runtime suite, #634 iterator bench, results (`fffd86b04`).
- Keep the web compiler's raw gate at the approved 5.2 MB (`bebf06ad6`).
- Store one element field in place; fold constant branches (`de67c20fb`).
- Benchmark the RC backend again after #642 to #655 (`08953e598`).
- Build the conformance binaries before the timed spec runs (`d46d5e34e`).
- Add the compiler-tests skill: stage fixtures and benchmarks (`0e640f052`).
- compiler-tests skill: final #675 layout, multi-module cases, tir-flags (`958a54dff`).
- Editors: highlight struct instead of data, and the using contextual word (`b222ddae5`).
- Record bad RC emitted-code patterns as focused cases (`1efeadd67`).
- TIR dumps name local flags; fold tir-flags into tir (`8e05e7c54`).
- Stage tests: fail on orphan baselines; blessing deletes them (`a1c7c649a`).
- Cover native-width integer remainders and zero divisors (`b921e0efa`).
- Raise the web compiler's raw size gate to 5.25 MB (`59fa12826`).
- Revert "Remove releases proved null across control flow" (`1256e62a5`).
- RC: read a cell directly where its declaration dominates the use (`ab9291b72`).
- RC: a cycle candidate's last release leaves its block to the collector (`e1d721bc1`).
- Use size-class fast paths for fixed object allocations (`332e8d4af`).
- bench: closures.js calls three function values per element, like hd and Go (`f7549b935`).
- bench: results after the RC optimization round (all eleven within 1.2x of Wasm GC) (`5816d6800`).
- Remove obsolete list iterator step closure lowering (`55ee483b7`).
- bench: closures.js uses idiomatic array map/filter/reduce (`ae2d7382f`).
- bench: closures.js uses the Int32Array's map/filter/reduce (`fb50df62b`).
- bench: README describes the Int32Array closures.js and its Node re-run (226 ms) (`33dc61c87`).
- Remove unused compiler helpers and dependency edges (`ef4b99118`).
- Remove unproduced counted-loop TIR tags (`b7c0267f5`).
- Compare five directions for replacing TIR (`48cdf9c21`).
- Design TIR v2: places, explicit ownership, staged migration (`f530dc561`).
- TIR v2 design: exposure along the whole path, aliasing conformance cases (`b2d51b8b0`).
- Design: weak references as std Weak[T] with an on-demand side table (`4b4440aea`).
- Web compiler: gate only the download (gzip) size (`17ed03110`).
- Revert the browser std-text dictionary compression (#700) (`714c279b7`).
- TIR v2 stage 1: place encoding in hd_tir (`004941c7e`).
- TIR v2 stage 1: Read, Store and Ref tags (`8196f73b4`).
- TIR v2 stage 1: the emitter's place resolver (`8a7c8c952`).
- TIR v2 stage 1: references without closure forms (`44686c9cc`).
- TIR v2 stage 1: reads through references as Read places (`92597e57a`).
- TIR v2 stage 1: writes through references as Store places (`35d5432a8`).
- TIR v2 design: the checker lowers only what needs type information (`7fc8af3fe`).
- AGENTS.md: real hd code needs a full-capacity model, not Sonnet only (`26516f898`).
- TIR v2 stage 1: one place for every write (`596461d38`).
- TIR v2 stage 1: element writes as Store places (`ab648d1c7`).
- write-hd skill: hd_run! takes a Command (`b0f54ace1`).
- TIR v2 stage 1: every local, global and field write is a Store (`66593a307`).
- Raise the web compiler gzip ceiling to 2 MB (`2a41b00b5`).
- tir-v2: data layout rules for the stage 2 ownership passes (`f7d55c890`).
- tir-v2: typed memory primitives approved; inliner decided after stage 2 (`4ff03ce06`).
- TIR v2 stage 2: plain local and global reads are Read (`1e30a80b5`).
- TIR v2 stage 2: field, tuple and payload reads are Read (`f4d9e04be`).
- TIR v2 stage 2: element reads are Read (`321289630`).
- TIR v2 stage 2: ownership tags, modes and the own view (`ec05b581e`).
- CLI case: the HTTP timeout composition passes only on a real timeout (`611791220`).
- Fix the Windows build of the copy file-identity check (`c8c10d1d0`).
- TIR v2 stage 2: insertion, OwnCx and the switch to its counts (`1b5506ae4`).
- Expect the program name in child_signals' process results (`4d04ef5a8`).
- TIR v2 stage 2: insertion's uniqueness checks and pins (`e619b3f83`).
- TIR v2 stage 2: the move pass (`c72ab7050`).
- TIR v2 stage 2: the borrow pass (`68f668b44`).
- Reject field facts on typed CLI commands (`72310753d`).
- Proposal: composing work with combinators, policies and provider decorators (`10e6bef1e`).
- TIR v2 stage 2: pair cancellation (`4a20be7be`).
- Mark panic helper calls as never returning (`420b57278`).
- Return an Error from the playground main! result test (`199a0165b`).
- Format the playground main! result test (`a037a7c6c`).
- TIR v2 stage 2: known-null (`a3e09f509`).
- Describe the entry error fixture's message error correctly (`ca3cf2e69`).
- TIR v2 stage 2: suspension guard of move's rule 4 reads the body (`4e6dbb258`).
- TIR v2 stage 2: hoisting and the stable-list table (`da6dc5e0d`).
- Record why refcount frees are not deferred (`394e28827`).
- TIR v2 stage 2: the checker's switch for the ownership passes' path (`eb684b163`).
- TIR v2 stage 2: places rooted at the local (commit 12a) (`1f698d267`).
- TIR v2 stage 2: the checker leaves pins and element write marks to the passes (commit 12b) (`1d4b95c5e`).
- TIR v2 stage 2: insertion makes the header copies (commit 13a) (`aed8facdd`).
- Spec decision E: collection elements are places, element references are call-scoped (`5a6aab3de`).
- TIR v2 stage 2: parity of the new path's counts (commit 13b) (`2bdc507c3`).
- Report a named argument to a function value as unknown-named-argument (`d4517444b`).
- Say comparison traits have templates that only @derive instantiates (`39e4acbab`).
- Drop the move fix-it from beside-directory errors (`a594f3f14`).
- Print each fix-it on a fix: line in text diagnostics (`3e81c10a5`).
- Satisfy Windows clippy in the Windows process tree (`e1728006f`).
- Revert "Satisfy Windows clippy in the Windows process tree" (`fa30b5183`).
- Revert the Windows job-object change from #407 and its lint fix (`59209d5d8`).
- TIR v2 stage 2: measure the ownership passes' rebuilds (#802) (`81151c12b`).
- Check element-reference-outlives-call (`7f0724d7e`).
- Spec: a reference parameter is one declared as a reference (`84cca5ade`).
- Make map values places (`ec1198a3a`).
- Spec: a loop over element references conflicts only on a length change (`490bcf5a7`).
- TIR v2: defer the inliner; stage 3 follows Go's model (`71113b2c0`).
- TIR v2: the inliner is approved; stage 3 moves List, Map and strings to std (`328bfa853`).
- TIR v2: a bottom-up size and cost budget for stage 3 (`6792b1bfa`).
- TIR v2 stage 3: the inliner's body copy (commit 1 of 4) (`bc3ffb05a`).
- Revert the inliner's body copy (stage 3 commit 1) until copies keep their panic frames (`08a0f2b27`).
- Prune unreachable Wasm functions after final lowering (`6ee8ebd60`).
- TIR v2 stage 3: the inliner's body copy (commit 1 of 4) (`1792757af`).
- TIR v2 stage 3: the inliner's trait calls and inlined frames (commit 2 of 4) (`9c39889b6`).
- std.http: Response.json returns JsonError; HttpError loses Json (`68c4b2db9`).
- TIR v2 stage 3: the inliner binds parameters by pass mode (commit 3 of 4) (`d3cc9d138`).
- TIR v2 stage 3: one splice per body, copy checks, limit 8 (inliner commit 4 of 4) (`cd5926ad2`).
- Delete the program database proposal until there are real users (`2e2525c68`).
- Keep the program database proposal (`ce7699086`).
- TIR v2 stage 3: typed memory primitives (commit 1 of 2) (`5597ac42e`).
- Regenerate the host ABI model after HttpError lost Json (`9f875ca2e`).
- Note a cascaded unknown-type after std-only-use on Ptr (`0aed9e00d`).
- TIR v2 stage 3: typed memory primitives (commit 2 of 2) (`87e1258c3`).
- Add a design-doc skill (`17eee69e5`).
- design-doc skill: use cases end to end and the state of the art in depth (`17e31c341`).
- design-doc skill: prior art by design, interface, coverage and limits (`289912977`).
- TIR v2 stage 3: List changes name their place (List commit 1 of 4) (`5369383ba`).
- Skills: explore an unfamiliar domain before designing (`a00c6b374`).
- design-doc skill: per-domain checklists and the issue's deliverable first (`f8efa99df`).
- TIR v2 stage 3: map values are places for a change (List commit 2 of 4) (`b3129d255`).
- write-hd skill: what to do instead of keeping an element reference (`185a3c941`).
- Checker: an element receiver of push opens its access before the arguments (`a93482cb9`).
- Solver: bind head row parameters and stop committing on unfixed parameters (`da5a1f75a`).
- TIR v2 stage 3: one bottom-up budget for List and Map buffers (`2b0db8db4`).
- Rewrite the policy composition design against #783's deliverable (`adaeaea81`).
- TIR v2 stage 3: places at top-level bindings (List and Map buffers, 3a) (`a7be48598`).
- TIR v2 stage 3: copy-out of a mut self collection receiver (List and Map buffers, 3b) (`41db36254`).
- TIR v2 stage 3: the conflicting-access check for copy-out (List and Map buffers, 3c) (`8be00eebf`).
- Release design: one hd release command; a tag needs an exact version (`9e4b4a063`).
- Release design: a bare hd release reports; only hd release VERSION tags (`4d2e52d27`).
- Durable resume: record the owner's direction (`16bb10081`).
- hd_stdpack design: dbg_printer replaces dbg_text and dbg_show (#515) (`a96ebb9eb`).
- TIR v2 stage 3: every change names its place, read in one walk (List and Map buffers, 3d) (`4e2edce47`).
- TIR v2 stage 3: a mut self method on a list or a map gets its own copy (List and Map buffers, 3b and 3c removed) (`185631377`).
- Checker: a list's or a map's mut self method is treated as a struct's (List and Map buffers) (`8a5a6c385`).
- TIR v2 stage 3: the owner word in the buffer (List and Map buffers, 3e) (`f54c7b322`).
- Move lost-write from the compiler to the hd lint design (`8ab824b29`).
- Checker: a closure shares a captured collection that a change reaches in place (List and Map buffers) (`83ceb7093`).
- TIR v2 stage 3: a store or a change through a kept base read names its place (List and Map buffers) (`424c34c44`).
- TIR v2 stage 3: the buffer address (List and Map buffers, 3f) (`59682131d`).
- TIR v2 stage 3: no header copies (List and Map buffers, 3g) (`dad199407`).
- TIR v2 stage 3: 3h measured, list kernels stay in Rust (`c2d053f8d`).
- Pipe styles: Haiku usability probe of four pipe rules (`0d739a8cb`).
- Borrow a binding local read into another binding local (`1a8ce1c5c`).
- Union types: record the owner's decisions and park the design (`b0ef3058b`).
- GUI exploration: parked; the owner doubts the recommended direction (`02d4381ff`).
- Delete the dead inliner binding chain and share the tiny helpers (`a0ce4cf35`).
- Build one Topology per body for the verifier and consumers (`266a6e1ea`).
- Share one Topology across the ownership passes and the emitter (`4fa8bfa2f`).
- hd lint: plan a lint for unused capabilities in hd.toml (`ac12bc655`).
- Decode places with one PlaceView and walk cleanup suites once (`0f2414078`).
- Share part slots between known-null and the verifier, and one bit arena (`327e26082`).
- Run moves and the verifier's borrow walk on one backward dataflow engine (`f28e5f065`).
- Guard the suspending sub-body climb with nested suspension cases (`fb4dbf949`).
- Run the verifier's read-after-move and one-consumer walks on one forward engine (`4d21f2cfc`).
- Run known-null on the shared forward dataflow walk (`da8b2224d`).
- Call known-null's pending-edit hook only while a block has edits left (`343347b40`).
- Replace a counted value through one emitter helper, and find loop tests once (`f04a7f1c0`).
- Clean up docs/audit: delete finished reports, drop obsolete notes (`5f097bec6`).
- Restore the goal metrics baseline and its README summary (`ece2a2f13`).
- Bring future-work docs up to date with current decisions (`4fb75a62a`).
- docs/compiler: drop the removed ^ mode and native backend, update TIR v2 status (`802417e9c`).
- Fold the release stress test into RELEASE.md (`23bc71240`).
- Merge HTTP_SERVER into SERVER_APPLICATIONS (`f00e93606`).
- Merge the serializable-closure prototype plan into its design doc (`24a83a0f4`).
- Node net: bound a stream's read-ahead so writers meet backpressure (`28072b8a1`).
- Remove parked design docs; their issues hold them (`6069a2eaa`).
- Merge the stdlib redesign and calls log into STDLIB.md (`5b3c00f06`).
- future-work README: point at the removed-ideas umbrella #833 (`07bdf2ced`).
- Lower every store through the place walk (`1decd653f`).
- CI: run the full check on muse/ branches (`a8e7765ca`).
- Lower every reference and place read through the place walk (`3c045d320`).
- Restore the tier-2 lifetime design; local references build on it (`b3f5eed1a`).
- Delete the finished Rust model study; point its citations at history (`7ada89051`).
- Read every instruction's operands through one slot schema (`58c3d470f`).
- Delete the finished tier-1 object model study; cite it from history (`0c1b055d2`).
- Fold the SIMD byte-scan proposal into the performance roadmap (`d0f00cdb7`).
- Delete OPEN_ISSUES.md; open questions live in GitHub issues (`9efce1c49`).
- Merge #51: package-declared host capabilities (green CI run 40134436152) (`2b4246d15`).
- Support pollable package-provider operations (`173d8cc45`).
- Add SDK-owned HTTP request source and serving loops (`f85b86f1b`).
- Merge #266 slice 1: SDK-owned HTTP request source and serving loops (green CI run 40140113532) (`4af8b01db`).
- Investigate local references: use cases, edge cases, prior art, stress test (`7c969d711`).
- Add synchronous host turns for request-scoped adapters (`6db910a3d`).
- Review local references: brainstorm, stress test, implementation (`d9f79e1a8`).
- Merge #266: synchronous host turns for request-scoped adapters (green CI run 40144872922) (`a592c5591`).
- Bound server SDK request cancellation and cleanup (`15819cca2`).
- Merge #266: bounded request cancellation and cleanup (green CI run 40146035143) (`13d896fd4`).
- Local references: final design and implementation plan (`591ee34a9`).
- Spec: counted *T references and non-escaping &T (step 1) (`a30e29b9d`).
- Dispatch server lease I/O in its owning request coroutine (`f6c02042a`).
- Parse *T and *self as today's reference (step 1) (`d5fcdd786`).
- Merge #266: lease-scoped I/O dispatcher (green CI run 40154081349) (`12a1b7e8e`).
- Merge #827: required capabilities table (green CI run 40154474826) (`f75a079f9`).
- Docs: current status of the server SDK and local-references step 1 (`22ad8e065`).
- Integrate lease-owned HTTP transport with current SDK adapters (`6ce29ef59`).
- Respell stored references to *T (step 1) (`d354a1e49`).
- Check that local references don't escape (step 1) (`ea28c85b8`).
- Website: quote the *T spellings after the step 1 respelling (#834) (`e31970fcb`).
- Merge #266: lease-owned HTTP transport (green CI run 40160222501) (`58ede43ea`).
- Driver: hash docs once per file text; analysis skips the embedded std's eager parse (`cfea7d5c0`).
- Conformance: run test programs in the long-lived Node worker (`03f09cc5c`).
- Driver test: the package-host handle is a counted *mut dyn Events (#834) (`d482aedde`).
- wasmtime parity test: the timed connect yields a counted *mut dyn TcpStream (#834) (`9e2e568fc`).
- Stage tests: run the cases on a thread pool (`8dc699203`).
- Merge #827: [dev-capabilities] for tests and tasks (green CI run 40162625877) (`0a92d430b`).
- Bind server handlers to lease-owned HTTP clients (`cfe0dadeb`).
- Merge #822: pipe styles evaluation (green CI run 40163416861) (`6b9a8f901`).
- Merge #266: server handlers bound to lease-owned HTTP clients (green CI run 40164474997) (`6b9fc7467`).
- TIR: a Ref keeps its local-reference kind as ref_word::LOCAL (`a87e1f9c9`).
- Bench: the study packages' hd.toml lists Console and Clock (`d68a5b898`).
- Respell & type arguments the step 1 pass missed (#834) (`3f80c9ede`).
- Add Fetch mapping and supervised cleanup backstop (`3a2b0075c`).
- Merge #831: daily goal metrics (green CI run 40167833622) (`8bb53595e`).
- Merge #266: Fetch mapping and supervised cleanup backstop (green CI run 40167978464) (`22254be84`).
- Backend: rule 10 finds its run start in one backward walk (`24bdbb55c`).
- Bench: map-count reads with get and unwrap_or (`8b0200ba1`).
- Ownership: a store's base read holds no count across its IsUnique (`085df5772`).
- Declare the web compiler build module exports (`27a3b0bc9`).
- Merge #819: Windows Node process trees owned by a Job (green CI run 40173380135) (`94bd4595b`).
- Node host: the output writer boots while the module compiles (`3d397bbe5`).
- from_utf8 skips ASCII bytes in one step (`456f4b736`).
- Serialize SDK fixture files inside parallel CLI tests (`08935227b`).
- JSON scans skip plain bytes 16 at a time (`1a3fe898e`).
- Merge #823: hd install puts package executables on PATH (green CI run 40177110116) (`771b01c87`).
- Merge #266: serialize SDK fixture files in parallel CLI tests (green CI run 40177482891) (`d1376bc6f`).
- One-byte searches skip 16 bytes at a time (`cccf90cc8`).
- String comparison skips 16 bytes at a time (`55be97f6f`).
- UTF-8 validation skips ASCII 64 bytes at a time (`ec03fd492`).
- Associate server application logs with request leases (`78fe92ff0`).
- Merge #266: server application logs associated with request leases (green CI run 40182378089) (`0a1caa450`).
- JSON frames take their facts on open instead of allocating empty ones (`5d2244028`).
- Derived serde skips fact searches for members with no facts (`f66af1441`).
- Ownership: an operand read waits for later operands that read its local (`1872757d8`).
- Merge #826: embedded V8 study (green CI run 40187568910) (`b776dbe23`).
- Places: fold element reads under an impl's type parameters (`51e23dd0c`).
- Borrow: a binding local lends a part to a consumer (`5b6683706`).
- Ownership: a string's byte reads write nothing, so a read borrows across them (`11972f1e3`).
- Design release-only interprocedural optimization (`b89321410`).
- Checker stage baseline: a variant field read folds under an impl's type parameters (`1c47fd0c2`).
- Merge #631: impl Trait nested in return types (green CI run 40194206364) (`a604eacf3`).
- std.json: decode reads a structural index instead of rescanning (`5f6285741`).
- std.json: find distinct record keys from a hint, borrow parser text (`05efdacec`).
- std.json: one record search and one hold, for size (`8a51ecfbd`).
- std.json: no index words for scalar list items (`e58c5608d`).
- Merge #844: map-count long benchmark pair (its only red CI job was main's checker baseline, fixed in 1c47fd0c2) (`78b1cc7cc`).
- Node host: write output directly until a pipe is full (`7d13e31b3`).
- Node host: a lean entry module for the output thread (`ac6f6f27e`).
- Node host: builtins through process.getBuiltinModule (`1efee4086`).
- Linear: drive slots for bang methods awaited at once (`08ddabc44`).
- Hosts: a Console write that completes at start is ready (`8ae82b7c5`).
- Conformance: a mode where every Console write is pending (`0eec7dfb2`).
- Read server deployment grants from the root production manifest (`c7b5b6fae`).
- Merge #266: deployment grants read from the root production manifest (green CI run 40203111478) (`8b276c65e`).
- Cleanup: drop finished pipe and embedded-V8 studies; record today's decisions in future-work docs (`c4745e220`).
- wasmtime session test: Windows writes stay on the writer thread (`34b343860`).
- Release summaries: export each body's summary from the borrow tables (`c8a3f90ff`).
- Release summaries: own bodies callees first, by strongly connected component (`19988ad52`).
- wasmtime session test: the panic stub's events follow two more on Windows (`708ab4938`).
- Ship an SDK-owned portable runtime snapshot for deploy tasks (`74b1ec4c5`).
- Merge #266: SDK-owned portable runtime snapshot for deploy tasks (its only red CI job was main's Windows session test, fixed in 708ab4938) (`9a5ac48b5`).
- Make SDK cleanup timing checks deterministic under load (`25271d165`).
- Merge #266: deterministic SDK cleanup timing checks under load (green CI run 40213086292) (`0c0bca94c`).
- Expose checked interface identity to package providers (`e1b898471`).
- Release summaries: a call that writes nothing keeps borrows across it (`fdf93f25f`).
- Merge #266: provider context carries the checked interface identity (green CI run 40214348736) (`581b165de`).
- Release summaries: lend the parameters a callee's summary does not keep (`6bca6832d`).
- Prepare Cloudflare deployment through the installed server SDK (`327a56a87`).
- Fix Cloudflare deployment controls on CRLF checkouts (`f34ac0194`).
- Merge #266: Cloudflare deployment through the installed server SDK (green CI run 40223737391) (`b2c0ec222`).
- Merge #266: Vercel Node deployment through the installed server SDK (green CI run 40225826480) (`f556b42df`).
- Add public policy layers and apply to std.task (`8b263e56d`).
- Expose timeout and retry policy builders and migrate callers (`b5868e54b`).
- Add fallback, circuit breaker and hedge policy layers (`d8f54b57e`).
- Take over an unshared list buffer in string_from_bytes; StringBuilder byte path (`6ad81d02e`).
- List.extend: one reserve and one memory.copy (`20ed738e8`).
- StringBuilder as one byte buffer; push_byte replaces push_ascii (`81c8b35c7`).
- Update the panic-frame test for the moved List.remove_at line (`8e7f4a9b4`).
- StringBuilder: push_ascii again instead of push_byte (`e54ace398`).
- Add list_reserve intrinsic; StringBuilder.reserve grows once (`f84627a7f`).
- Run daily metrics at 00:00 and 12:00 UTC, one data file per run (`7b47bb9e2`).
- Show the daily metrics on the website (`69407d1f1`).
- Daily Metrics page: trend charts instead of a table (`70b89d462`).
- Prove falling list indices in bounds; elements are no list aliases (`8cf4f3686`).
- Hoist an inner loop's uniqueness check out of the outer loop (`d55a59b07`).
- HTTP messages: signatures only, for review (`10e8d2d05`).
- HTTP signatures: drop Headers.append (`3dc1c33fd`).
- HTTP signatures: fix review findings (`8af3edf48`).
- Move the HTTP signature draft into PR #848 for review (`bf1e78a2d`).
- design-doc skill: short, human-ordered designs; stdlib as signatures + examples (`a74d069a3`).
- Routine cleanup: drop the finished policy design, refresh interproc status (`932f49a96`).
- HTTP messages: signatures and usage examples for review (#847) (#848) (`072147650`).
- Daily Metrics page: explain the pillars, one trend chart, one goal chart per pillar (`48f782bce`).
- design-doc skill: the handoff is a short doc plus, for stdlib, signatures and examples (`faf53316f`).
- Daily Metrics page: correctness gate apart from the three pillars; link the announcement (`9d0c73433`).
- Daily Metrics page: link the Arena name instead of a separate X call-out (`6a039a07f`).
- design-doc skill: most important first, required stress test, prior art with code (`b07e1d493`).
- Inline a constant-decided if arm instead of wrapping it in a block (`a77e3d3d7`).
- Delete local stores that no path reads before the next write (`d979f6576`).
- Keep a value read once on the stack by liveness, not by counts (`821e949e5`).
- Let the local rules see through float, conversion and memory ops (`0e7e4a541`).
- Share one local among locals whose values are never live at once (`c3379255e`).
- Apply rules 13 and 14 together, in fewer liveness passes (`42f4726dd`).
- Take a trait method's parameter defaults from the trait declaration (`f82d56b49`).
- Inline small user leaf functions in release builds (`890f20f9b`).
- Call cold runtime paths through the table, out of V8's inlining reach (`9b500b458`).
- Keep dbg suppression on inlined bodies from fetched packages (`4881bc53b`).
- Regenerate the host ABI model for the new std.http types (`61d2747cb`).
- Resync the server SDK's portable runtime snapshot with the new ABI glue (`c2483c387`).
- Sort slices with a compact stable sort instead of std's (`8c89d7a1d`).
- Build BTreeMaps and BTreeSets by insertion, not collect (`169f3dd4b`).
- Gate the sort import in the wasmtime engine tests on unix (`14bf7a0b6`).
- Reserve known-length list comprehensions once before their loop (`ba1f5c542`).
- Keep a local in its frame slot when its last reference takes it (`f76cc1099`).
- Migrate task-fallback's fake Http and restore the bare-CR fixture (`6b704beef`).
- Count std's new http_wire module in the std body test (`baee670ce`).
- Count known payload starts inside aggregates without the page table (`e0590dde3`).
- Skip the page-table lookup for null and static counts (`9dfb705d0`).
- Move a consumed value into a store through a reference (`0ae263b09`).
- Pass a value through a coercion that keeps it, and borrow through it (`1351d2658`).
- Use local references for StringBuilder mutations (`96f0620eb`).
- Zero a known-class object's collector word without std's track (`58e7f6c0c`).
- Test the candidate count alone before each allocation (`a153a4a24`).
- Call the collector's collect and visit through the table (`17391f696`).
- Give the known-class allocation path no size and one tail (`6f8c443e6`).
- Make StringBuilder build borrow its receiver locally (`316bbb508`).
- Run the outermost release's block in an out-of-line drain (`e19015b1f`).
- Call recursive types' release glue directly from the drain (`bdd1d72df`).
- Add MoonBit ports of the 18 study programs and an hd vs MoonBit runner (`8b4ef2f77`).
- Write the benchmark programs in idiomatic hd (`6e2a9123e`).
- Grant Console in the runtime bench's generated hd.toml (`d99d036ba`).
- Specify the hd lint boundary and warning migration (`8d72b09af`).
- Inline small functions bottom-up in the release pipeline (`0f75f9815`).
- Cleanup: point docs at today's closures and blocker tickets (`e3653159a`).
- Add hd lint and move unsigned comparison advice (`d884f712a`).
- Cleanup: docs follow the 2026-10-26 issue triage (`67273c3d2`).
- Cleanup: delete server, HTTP and GUI experiment folders (`61e444462`).
- Add a cleanup skill for the routine repository cleanup (`ac1370c9f`).
- List the compiler-tests and cleanup skills in the skills README (`875dbda22`).
- Drop the credential-file rule from the shared skill rules (`8a4adb617`).
- compiler-tests skill: the standard gates and perf/size report (`2aaeb78c6`).
- Use += for the hit counter in the server example and its twins (`83df9f49f`).
- Move unused binding advice into hd lint (`90c6d7ee5`).
- compiler-tests skill: the perf report covers 19 programs (#863) (`d077048eb`).
- Move binding mutability and pattern advice into hd lint (`aa0ae0c58`).
- Route metrics warning fixtures through hd lint (`39f22c0b7`).
- Hoist uniqueness out of a loop with no entry test (`9066c9288`).
- Make the heap memory 0 and the exchange buffer memory 1 (`5706518d8`).
- Playground WAT test: check no longer reports unused-local-binding (`bd03cb5d3`).
- Tests for the validation proposal's compiler gaps (`8fbfcd74a`).
- Move flow and discarded-debug advice into hd lint (`c70bcf389`).
- Drop discarded values together with what pushed them (`9c070a4d7`).
- Design: @inline, @inline(.Always/.Never) and @cold decorators (#885) (`9677816a9`).
- Move mixed-script identifier advice into hd lint (`abe18388d`).
- Declare @inline and @cold in std.annotation and check their lines (`170a3c017`).
- Inliner: never copy a function marked @inline(.Never) or @cold (`750c2b2a1`).
- Lower each direct call of a @cold function through the function table (`754143578`).
- compiler-tests skill: MoonBit compare once per round of merges, not per commit (`ee2cbc043`).
- compiler-tests skill: per-commit reports are hd only; all reference languages once per round (`278f55f0b`).
- Structure: a Field handle is one reference to a static record (`aef70b367`).
- Apply two-write println rules and console fixtures (`6eac2928c`).
- Refresh println journal and schema callers (`d9076696b`).
- Structure: Variant, Members and Key are references to static records (`1ea0d8e6a`).
- Move unselected-main advice into hd lint (`4b056d62d`).
- Release: count whole-object references at their payload start (`f541f35e7`).
- Cancel a general count against a known one on the same local (`abdacd098`).
- Fail hd doc when the browser cannot be opened (`f480dbb24`).
- Wrap hd formatting at 100 columns (`93215bb0e`).
- Integrate resolver lint facts with module advice (`11d1fe217`).
- serde: read metadata emptiness with is_empty again (`57a7c19cc`).
- compiler-tests skill: report every bench program, not a fixed 19 (`c8712b9cd`).
- End broken formatter groups with a trailing comma (`6c322a5e3`).
- MoonBit compare: a wasm-gc target and the map-count-long port (`f4ddc82ee`).
- Cleanup: docs follow today's #704, #884, #889 and lint/fmt redesign outcomes (`d3ef70052`).
- Drop counts whose operands are all constants (`c6c76cbad`).
- Release summaries: downcasts, string intrinsics and member_name write nothing (`bde1c90d4`).
- Borrow: a local's reference lends across a call that writes nothing (`2000125a5`).
- For loops hold their source list as an ordinary value, with no snapshot count (`a0f86b594`).
- Writing log: a Muse probe mistake from the #908 design work (`265f75e69`).
- Panic helpers report through rt:panic_with's table slot (`c1a4f6b76`).
- Member directory: read each handle's information once (`bb824cd7b`).
- Serde: a member's fact tests run out of line (`924ca9a1d`).
- compiler-tests skill: perf jobs attribute, then run a ceiling experiment, then build (`edfe1ae39`).
- Locals: coalesce the two ends of a multi-value copy (`7fec0bd02`).
- Locals: a branch to the exit after its values is a return (`d456b6da4`).
- compiler-tests skill: local shuffles cost bytes, not TurboFan nodes (#926) (`d0b165ce9`).
- Link: a table slot plus a constant is one constant (`96fb73c41`).
- Build the canonical syntax-tree formatter (`fb982b245`).
- Fix canonical formatter review regressions (`4bf0c4401`).
- Finish the opinionated formatter policy (`036e20d8f`).
- Apply the canonical hd format (`edea876ae`).
- Cleanup: docs follow today's fmt/lint approvals and the perf triage (`698fd3de8`).
- Centralize the lint registry and policy (`f3509e4ce`).
- Move the inliner into hd_opt (`341ab36b9`).
- Move the whole-object scan into hd_opt (`25141ca6e`).
- Free known-class blocks onto one freed list per class (`2b1b90c44`).
- std.format: drop the list and map cycle check that never matched (`9b1bb59c9`).
- Spec: box(e) makes counted references; &e is always local (#912) (`3b601d7d4`).
- Revert the #912 spec: box(e) and local-only & are shelved (`568feafd1`).
- compiler-tests skill: the 3% bar gates building, not merging a small pure win (`e6fd277b2`).
- Check whether a collection is due only on the class slow path (`a0d59b87a`).
- Node host: read random bytes from /dev/urandom, not webcrypto (`81c767638`).
- Cleanup: refresh lint and performance status (`b702a14c4`).
- Update the README for current hd (`4fc0e26a0`).
- Refresh the user guide for current hd (`ab37e2780`).
- Update the tour for current compilation (`ca9b09518`).
- Refresh playground compilation examples (`9b0100559`).
- Add the lint analysis walk (`0912c916c`).
- Server SDK: refresh the portable runtime snapshot for the urandom host change (`ed219f90d`).
- Fix host contract errors after startup split (`a23fd4e5b`).
- Cache lint findings per module (`8e64f8a56`).
- Spec: Console output is synchronous (`0c08b1454`).
- Server SDK: regenerate the server schema for Console write_sync (`15f0018a4`).
- println is allowed in defer and non-entry init: update check-script and write-hd (`97243a7f8`).
- compiler-tests: pick the bench run count by purpose (`69367d5bf`).
- bench/runtime: take --runs N (`0e0067726`).
- Expose borrowed checked views to lint analysis (`6c0e6e609`).
- Register lint checks on analysis events (`c17b6b757`).
- Finish lint policy and source suppression (`946ab281f`).
- Reject unknown manifest keys on load (`be94a8b18`).
- Drop the allocator's running total from unchecked builds (`c6697219a`).
- Run ref_tests checked in every mode; delete the GIVEN probe (`92f651ed9`).
- Move private main advice into lint (`a121c3ffd`).
- docs/use-scenarios: struct, not the retired data keyword (`32b618032`).
- Move fetched dbg advice into hd lint (`2e2090f7f`).
- compiler-tests: size measurements at about 300 ms, report throughput (`16fd3907c`).
- Error types: one cause marker per member; @from already marks the cause (`4ff9631e4`).
- Record validation benchmark writing feedback (`06ddb0ad2`).
- Skills: follow prior art by default; differences need an anti case (`0021299ef`).
- Cleanup: status lines for the lint migration and the measured RC misses (`c77c4d04c`).
- spec/std/host: env_decode uppercases field names, as the code does (`717e3c3d2`).
- Tour: introduce tuples where lesson 05 first uses them; log combinator diagnostics (`895fe1bba`).
- Const values: accepted direction for typed const parameters and const fn (PR #957) (`a7985fe45`).
- HTTP multipart: accepted design and checked signature package (PR #969) (`b86abcc0a`).
- Build profiles: accepted @cfg direction (PR #973) (`3e16b79cb`).
- Union types: complete positional design (PR #959) (`7b8fbcaa8`).
- Enum tagging: accepted design for serde tag/content/untagged facts (PR #968) (`885e6251c`).
- Decimal: accepted design, kept as a future package (PR #960) (`5d6992009`).
- Preserve Structure enum accessor checks (`6bf1748f6`).
- HTTP decompression: accepted host-side decoding design (PR #970) (`b5ed9dbec`).
- Contracts: accepted require design (PR #958) (`fd40bd52b`).
- Scope guards: accepted design (PR #967) (`628a7f92e`).
- Box: infer T from the argument when a *mut dyn is expected (`baa1fde0e`).
- Server SDK: refresh the server schema for the std box rewrite (`c0738458e`).
- Cleanup: future-work statuses and two finished design packages (`d820ec517`).
- Server SDK: refresh the server schema for the using split (`19d8af37e`).
- Scope guards: on_exit waits for #961; the first build is merged (`68813f340`).
- Tour: short headline, one small example per feature, a plain Try it line (`48889acab`).
- Tour: lessons 1-18 follow the lesson 3 style (`a956da761`).
- Tour: lesson 7 boxes its shared object; contents lists each page's sections (`bedf29334`).
- Server SDK: refresh the server schema for multipart (`27f3bd336`).
- Server SDK: refresh the server schema for cleanup rows (`107be5f83`).
- Server SDK: refresh the server schema for the #912 S6 rewrite (`38d67422a`).
- Wasmtime session tests: box the aliased counter (#912 S7 follow-up) (`16d8c70c4`).
- Playground REPL test: box the aliased counter (#912 S7 follow-up) (`32155152d`).
- Server SDK: refresh the server schema for decompression (`1aa4715db`).
- Serde: implement enum tagging facts (`f74465d7f`).
- Server SDK: refresh the runtime snapshot and schema for enum tagging (`9cc1ef0bf`).
- Downcast takes a counted receiver only (#988) (`e0b7616e2`).
- Server SDK: refresh the server schema and runtime for #988 (`cc63fb077`).
- Design: the staged path to returned local references (`bd95ae5ba`).
- Design: candidate source-set syntax for stage 3, study first (#1005) (`637f690ac`).
- Cleanup: status lines after today's merges; delete the built validation package (`53402b43d`).
- Design: inner-block locals referenced from outer bindings stay valid (`33f063544`).
- Design: stage 2 first rejects a local reference outliving its local (E0597), then drops the cells (`d2a1e06ab`).
- Cleanup: stage 2a (#1007) in the returned-references status line (`206a5d667`).
- Dependent results: a result may borrow from its elided source (#1002) (`58118b24b`).
- Backend: a dependent call's argument keeps its cell (#1002) (`7089fd863`).
- Cleanup: remove implemented design records (`abb3d0cd4`).
- Conformance: box the regex VMs' thread lists at declaration (#1007) (`94f7928e0`).
- Local scope: a local reference may not outlive its local (#1007) (`2a29f58dc`).
- Design: how the lifetime position grows to parameter sources, sets and paths (`d53486f67`).
- Design: a lifetime path through a reference names its referent (`e8f9d6d50`).
- Study returned-reference source sets (`e520426d4`).
- Design: stage 3 is single lifetime variables (#1005) (`c30cb2957`).
- Frame slots: bound references and dependent calls keep their local in the frame (#1006) (`fb971e8b9`).
- Design: no paths in signatures; bodies track field paths from stage 7, as Rust (`e36209371`).
- Design: stage 9 is field paths on results (`e9c9944d6`).
- Design: stage 9 reference in the end-state recommendation (`354cf0319`).
- Design: places through *mut stay precise; unseen aliases checked per place, as Swift (`2a198d4c5`).
- Keep unterminated-string a parse-phase code with its own fixture (`5364ce757`).
- Server SDK: refresh the server schema and runtime for #996 (`b7d7853dd`).
- Move lifetime-order from the compiler to hd lint (`6d644422f`).
- CLI conformance: line numbers after the #1013 std rewrite (`506bfd9a7`).
- Server SDK: refresh the server schema and runtime for #1013 (`ef22dbb1a`).
- Design: stage 5's persistent-boundary bound is T < 'static (`e9561b168`).
- Design: stage 6 broken into slices 6a-6i (`c209890c1`).
- Design: stage 6 in one-rule slices (`f98e35528`).
- Design: capturing closures are Fn[I, O, $R] / FnMut bounds; fn(A) -> R never captures a local reference (`ab64dde0d`).
- Design: closures carry a capture bound on the arrow, fn(A) ->'a R (`84fa3660b`).
- Design: the capture bound follows fn, fn'a(A) -> R (`6652c68cc`).
- Design: closures are Rust's Fn traits with hd's elision; fn parameters may capture locals (`d67d08ab1`).
- Returned references stage 5a: the 'static bound (`c00c0b2a2`).
- Returned references stage 5b: local references as type arguments (`0e1e00b91`).
- Returned references stage 5c: restore the #1003 fixture rewrites (`4975db7fe`).
- Server SDK: refresh the server schema and runtime for #1016 (`fef217f1c`).
- Design: HTTP transport settings on the provider (#950) (`611fca589`).
- REPL completion: offer nothing inside a lifetime token (`65c98c681`).
- Server SDK: refresh the server schema and runtime for #1014 (`01c0c1ac2`).
- Cleanup: status lines, KNOWN_FAILURES issues, merged design package (`d751d1934`).
- Server SDK: refresh the server schema and runtime for #1035 (`c1a1854dd`).
- Server SDK: refresh the server schema and runtime for #1026 (`6a28ba077`).
- Web: raise the gzip tripwire to 1.9 MB after the tier 2 checks (`2d10656f8`).
- Carry const generic parameters through interfaces (`4e49af898`).
- Evaluate closed const generic arguments (`c9d2d0f51`).
- Solve const generic parameters only from argument types (`12ccc1c23`).
- Returned references 6e1: the FnMut trait (`a2b28d9a7`).
- Returned references 6e2: iterator adapters and retry predicates take FnMut (`3834c8bfe`).
- Returned references 6e3: Fn closures may not change their captures (`e104438b9`).
- Server SDK: refresh the server schema and runtime for #1029 (`e1264e9be`).
- A closure for an Fn-bounded parameter has the bound's signature (`ed8aa8e36`).
- Server SDK: refresh the server schema and runtime for #1042 (`022ec1b1b`).
- require: check again that the condition runs exactly once (`30b321e4c`).
- Cleanup: tier 2 status lines, MoonBit goal reached (`3c4e58aef`).
- Returned references 6b3: nested and multi-lifetime field stores (`620c1188a`).
- compiler-tests skill: tiered checks only; the full suite runs on CI (`7c6341f32`).
- std.http: transport settings on derived Http providers (`3ed6ccd46`).
- oxlint: require braces for every if, else, for and while body (`f73ce662f`).
- oxlint: enable the promise plugin's correctness rules (`b5717d9f3`).
- oxlint: enable prefer-const, no-implicit-coercion and four more; keep vendor code untouched (`503ba352f`).
- oxlint: max-params, max-depth, complexity, no-nested-ternary, max-nested-callbacks (`6a639d289`).
- Format .oxlintrc.json (`c8a862e4b`).
- Add typed host emission path (#1069) (`2b97c3ef2`).
- Migrate host protocol primitives to TypeScript (#1070) (`d1d0b97ac`).
- Migrate package host runtime to TypeScript (#1071) (`6206626a8`).
- Migrate host journal to TypeScript (#1072) (`47d867f13`).
- Migrate core host to TypeScript (#1073) (`516f5261b`).
- Migrate session and web hosts to TypeScript (#1074) (`d492c58d5`).
- Migrate console workers to TypeScript (#1075) (`3388a1f20`).
- Server SDK: refresh the portable runtime snapshot for the host TypeScript migration (`202fb9aff`).
- CI: run the Test workflow on lane/** agent branches too (`104cecb1f`).
- hd_driver tests: import package-providers.mts after the host TypeScript migration (`3af4c5fe2`).
- Oxlint: clear host migration curly errors (#1084) (`a62c960f5`).
- Server SDK: refresh the portable runtime snapshot for #1084 (`e7dd23670`).
- Format scripts cover host/ (.mjs and .mts) (`2704e944d`).
- Lifetime solver S5: universal regions, checked edges and categories (shadow) (`fa4587073`).
- Design: one lifetime solver for tier 2, accepted (`085ce8859`).
- CI: run oxlint and the format check on every push (`b079139f9`).
- Returned references: decide ensure's result access and the Span API (`22cbe948b`).
- Returned references: ensure keeps no result access; no named return values (`ed94c96ca`).
- Returned references: named return values, if ever, use definite initialization (`28e64dbf9`).
- Returned references: drop the hypothetical named-return note (`ddb8351ca`).
- float_text tests: run host/float.mts after the host TypeScript migration (`534854877`).
- Oxlint: clear max-nested-callbacks errors (#1085) (`70f3c56e3`).
- Oxlint: flatten host max-depth paths (#1085) (`7382a2aa3`).
- Oxlint: flatten tools and site max-depth paths (#1085) (`d9587a409`).
- Oxlint: reduce host parameter lists (#1085) (`85b6457f6`).
- Oxlint: reduce tools parameter lists (#1085) (`b332cb5a2`).
- Oxlint: reduce benchmark parameter lists (#1085) (`7457fc1ad`).
- Oxlint: remove host and SDK nested ternaries (#1085) (`ce30f1bc4`).
- Oxlint: remove tools and site nested ternaries (#1085) (`dd1a6a971`).
- Oxlint: simplify benchmark metrics paths (#1085) (`1268b5f5f`).
- Oxlint: simplify benchmark study paths (#1085) (`103b10956`).
- Oxlint: simplify exact float reference (#1085) (`68302a1cd`).
- Oxlint: simplify website build paths (#1085) (`c6d62cb08`).
- Oxlint: simplify playground control paths (#1085) (`2bf2928c0`).
- Oxlint: simplify SDK packaging validation (#1085) (`608d24322`).
- Oxlint: simplify SDK output validation (#1085) (`7a6c8bf7f`).
- Oxlint: simplify SDK fetch paths (#1085) (`6cfc16d4f`).
- Oxlint: simplify SDK supervisor paths (#1085) (`e0f320417`).
- Oxlint: simplify SDK Node adapter paths (#1085) (`0a0602163`).
- Oxlint: simplify spec helper paths (#1085) (`0922cdcfd`).
- Oxlint: simplify fuzzer control paths (#1085) (`3c7697285`).
- Oxlint: split spec command dispatch (#1085) (`14cf69466`).
- Oxlint: split conformance CLI expectations (#1085) (`8161e2ed9`).
- Oxlint: simplify conformance parsing (#1085) (`078d09b8c`).
- Oxlint: simplify conformance case execution (#1085) (`3a45164a9`).
- Oxlint: simplify conformance CLI execution (#1085) (`5f367e911`).
- Oxlint: simplify host utility paths (#1085) (`fb91e4205`).
- Oxlint: simplify package contract validation (#1085) (`c75ccbdb1`).
- Oxlint: simplify package provider validation (#1085) (`388a9852f`).
- Oxlint: simplify host transport paths (#1085) (`87b132116`).
- Fix host TypeScript stack regressions (#1084) (`bea5115cd`).
- Oxlint: simplify process and journal decoding (#1085) (`79121a945`).
- Oxlint: simplify test case execution (#1085) (`2899cd576`).
- Oxlint: simplify linked session execution (#1085) (`896238337`).
- Oxlint: simplify session replay (#1085) (`a89d984ae`).
- Oxlint: simplify asynchronous host driving (#1085) (`1c093f71f`).
- Fix pending host call sites (#1084) (`a5beefdfb`).
- Oxlint: simplify core host construction (#1085) (`5d9b8f948`).
- Test: share Node host helpers (#1085) (`842d30451`).
- Test: stage Node network concurrency cases (#1085) (`7e8357767`).
- Test: stage Node file cases (#1085) (`ed250814d`).
- Test: stage Node console and output cases (#1085) (`89c6d1733`).
- Test: split Node network concurrency cases (#1085) (`c56b98f39`).
- Test: split Node file cases (#1085) (`eb70cbea1`).
- Test: split Node console and output cases (#1085) (`05b95a468`).
- Test: reuse Node host support helpers (#1085) (`13977b3f0`).
- Fix property outcome type after lint rewrite (#1085) (`d2ecbcc65`).
- Fix Node scratch runtime package module (#1085) (`93a6d446b`).
- Fix migrated float host test path (#1085) (`b21a22abb`).
- Refresh SDK runtime for host split (#1085) (`67172978b`).
- Server SDK: refresh the portable runtime snapshot after #1085 (`60026e48d`).
- Lifetime solver S10: call constraints from slot layouts (`ad4a66c3f`).
- Tier 2 decisions: ensure is a postcondition, SpanMut after stage 7c, no phantom markers yet (`75982e68e`).
- Rename one-letter host identifiers (`501d5ce77`).
- Rename one-letter spec tool identifiers (`7d55129d9`).
- Rename one-letter website identifiers (`d44430cb9`).
- Rename one-letter benchmark identifiers (`a7159d818`).
- Rename one-letter server SDK identifiers (`f1a18cfd7`).
- Make Span indexing panic explicit (`049e38e95`).
- Cleanup: refresh tier 2 status after S12, 6f, 7a, Span and suspensions (`052f874d0`).
- Web: raise the gzip tripwire to 1.95 MB (measured 1.90 MB with #1099) (`f1eef2874`).
- Spec: a &mut is exclusive while live; only *mut aliases freely (`a6db6ed41`).
- Website: orbit logo, new palette, and a home page built on the effect pillars (`2403b8cf7`).
- README: restore the derivation example, Why hd, and the three pillars (`92465f38c`).
- Home page: one effects walkthrough, typed derivation, and a std for correct code (`1bf1d2000`).
- Stage 8: no borrow flags; references through *mut keep their counts (`9197d9766`).
- Home page: linked derivation explainer and steady tab panels (`ff060c08c`).
- Stage 8a: count fixtures for a closure field read across a call (`8363fc5a1`).
- Stage 8b: exclusive mode in the borrow pass, off (`ccf33521e`).
- Stage 8c: run the passes twice for bodies that rely on exclusive mode (`3443155f8`).
- Stage 8d: direct calls with exclusive arguments run the exclusive copy (`ee00c27f4`).
- Cleanup: tier 2 status after stages 7d-8, const generics stage 1 built (`d7a7999c8`).
- Stage 9: the path syntax is 'param.path (owner, 2026-10-31) (`604190416`).
- Cover deeply nested union syntax (`d90ed5bd9`).
- Stage 9 docs: a &mut result path keeps the lifetime before mut (`d260dc437`).
- Spec: a call result held in a suspending body takes no run-time loan (`f217025f9`).
- Cache: layout 29, since #1115's held-args column and 9a's result paths both bumped to 28 (`61040b83c`).
- Home page: agentic pillars strip and a cleaner logo (`23d7338c8`).
- Logo: a glowing ring, edge highlights, a glass satellite; glossary anchors (`c6dbc3012`).
- Lifetimes stage 9c: a result path may cross a *T field too (`273ddb0e5`).
- Fix the generated glossary loan link (`545643849`).
- Tier 2 records: delete the superseded study and the stage 3 records; stages 1-9 built (`57d5b0b6c`).
- Tier 2 status: list the unchecked run-time loan cases (#1131-#1136) and #1130 (`69bbf8597`).
- Tier 2: parameter view types are not now (owner, 2026-10-31) (`b421f0f81`).
- Home hero: a WebGL sky and a satellite that laps the logo (`d5ee36e7e`).
- Spec: cite ref.excl.alias.counted-access after #1136's rename (`2c76c9f5e`).
- Wasm baseline: re-bless the *T loan case after #1132's holder locals (`78c1ee262`).
- Index.index borrows its receiver: fn index(&self, key: K) (`cb3e2d7cb`).
- Home hero: the satellite's lap takes 3.09 seconds (`4b54ad077`).
- Eq.eq borrows both operands: fn eq(&self, other: &Self) -> bool (`b6b4805ed`).
- Server SDK: refresh the server schema for Eq taking &self (`7e47e6d81`).
- Design: non-copyable types, accepted (#1147) (`c3533c31c`).
- PartialOrd and Ord borrow both operands: fn cmp(&self, other: &Self) (`405f29c87`).
- Hash and DeepEq borrow their receivers: fn hash(&self, state: ...) (`10666006a`).
- Server SDK: refresh the server schema for Ord and Hash taking &self (`3b59ccc2c`).
- Non-copyable types: a trait's Self is Copy by default, as in Swift; collections take copyable items (`86fff7283`).
- Exclusivity: an implicit reborrow is a loan like &mut *c (`b2a6b59fa`).
- Lifetimes 7e8: guard pointer slots under a &T root (`4487ac55f`).
- References to tuple elements: &pair._0 is a place, as a field is (`2a6a48382`).
- Std: compare tuple keys by reference, a._0.cmp(&b._0) (`442ffa650`).
- Tuple elements are writable places, as struct fields are (`38e6cffd5`).
- Server SDK: refresh the server schema after tuple element places (`8d507c603`).
- Cleanup: status lines after tonight's moves and loan work (`54cd387ce`).
- Copy marker: parse `impl !Copy` and `?Copy`, answer `Copy` from structure (`a828fce65`).
- Copy marker: default `Copy` bounds, trait `Self` is `Copy`, collections copy (`e2c59ac4d`).
- Server SDK: refresh the server schema for the Copy marker (`8a8d6de04`).
- Copy marker: REPL completion offers the prelude's Copy; web gzip guard 1.97 MB (`8895cf9bc`).
- Copy marker: the structural Copy walk visits a shared type argument once (`d0d1af6b9`).
- Non-copyable types: &mut dyn and plain trait bounds; the relaxed-trait question parked (`803ec58a0`).
- Pool workers get a main thread's 8 MiB stack (`2463e9aaa`).
- Checker: a method chain's receivers check in a loop (`99452465e`).
- Match: a tag switch and the arm checks read each row once (`79293ea4c`).
- JSON decode: read short decimals without the host float parse (`7b7968620`).
- JSON encode: write short decimal floats in hd, not through the host (`90d59f412`).
- JSON text: append to one byte buffer instead of joining pieces (`40060f144`).
- JSON decode: index the reader's frames with a local, not self.top() (`bc42b1da3`).
- JSON encode: write a short float's digits straight into the buffer (`8a5c4ed80`).
- Server SDK: refresh the schema provenance after the JSON std changes (`336a43e71`).
- Checked arithmetic: a constant operand is one range test (`9d19f29c3`).
- Checked i64 product: divide only when an operand passes 32 bits (`422a0c3e6`).
- Server SDK: refresh the schema provenance after the json.hd change (`e54c7eb9b`).
- Argument promotion: a primitive comparison reads a local's value (`54692a4d6`).
- Argument promotion: a direct call runs a by-value copy of its callee (`7c9659bb1`).
- Web gzip guard 1.98 MB: argument promotion adds about 9 KB (1,972,630 B) (`d2d33ac72`).
- Argument promotion: the inliner copies a callee that takes small references (`35b02273a`).
- Argument promotion: a counted referent is lent with no count (`9eae3ebb4`).
- Argument promotion: lend a counted value only while its local lives (`cf5040b4a`).
- Linear backend: a newtype's field read through a reference (`ae5a528a8`).
- Display and Error borrow their receivers: fn to_string(&self) -> string (`2ce01f3bd`).
- Debug borrows its receiver: fn debug(&self, out: *mut DebugWriter) (`42c90d493`).
- Server SDK: refresh the server schema for Display and Debug taking &self (`9c4a7d76f`).
- Collection: a thunk's call runs its callee's plain copy (`9c560f430`).
- Stage baselines: interpolation lends its values to Display.to_string (`4b5b3fce6`).
- Serialize and MapKey borrow their receivers: fn serialize[W](&self, ...) (`836a7a749`).
- Server SDK: refresh the server schema for Serialize and MapKey taking &self (`1a3fd70ab`).
- CI: run the Test workflow on claude/** agent branches too (`e0be6996f`).
- Cleanup: NON_COPYABLE status after the Copy marker and the &self read traits (`8d4cbb557`).
- Design: the standard library for references, accepted (#1162) (`d86c1f06d`).
- std: hd fmt http.hd and iter.hd (`884d3f46d`).
- Operators read through a &T operand (STDLIB_REFERENCES language rule 1) (`c0984cb6b`).
- Argument promotion: a temporary moves into the promoted copy (`8e3279d77`).
- Release: a `&self` receiver inlines and folds like `self` (`9c98a42f5`).
- Web gzip guard 1.99 MB; the 2 MB cap leaves about 17 KB (size cut filed) (`4ade9937a`).
- Linear opt: sparse liveness and interference in coalesce (`5df7cae81`).
- Inlining and zero_releases stay linear on a long chain (`d58ffe876`).
- Emit: a large enum's layout once per body, not once per use (`9f4f87764`).
- std: hd fmt after the hugged-closure layout (`2c9aca532`).
- CLI fixture: std line numbers after the std fmt pass (`137c02c7e`).
- Standard library for references: auto-deref for fields and methods; &*T without parentheses (`649b35b10`).
- Driver test: pass Named::label to List.map directly again (`e02cfea50`).
- Option: (&T)? gains copied() (`301335bd2`).
- hd fmt keeps the space in impl['a, T] (&'a T)?: (`48fcca05e`).
- Server SDK: refresh the server schema after Option.as_ref/as_mut (`fb164e286`).
- Standard library for references: loops follow Rust's three IntoIterator forms (`cfb3a2326`).
- Server SDK: refresh the server schema after std.task channels (`403cb6565`).
- Erased bodies: element references and uniqueness use the concrete layout (`e2be1e53a`).
- std.task: map_join_worker; channel pool fixture uses map_join!; schema refresh (`464d1e7fd`).
- join!: channel fixtures, std task example, write-hd skill and the guide use join! (`9815d89d1`).
- spec/std/task.md: the channel section links Map Join (`1074386a5`).
- List accessors return references: get, first, last, find, min, max (`60040a839`).
- Deque front, back, get and Heap.peek return references (`b2c3d5094`).
- CLI fixture: args().get(0) is a reference now; copy it out (`1a8aa0669`).
- Wasm stage case: List.get is a reference now; copy it out (`a655f2088`).
- Server SDK: refresh the server schema after the std accessors (`3e95efb79`).
- Release: a read-only receiver behind a reference or in a list inlines by value (`199b9bad8`).
- Argument promotion: a match on a `&self` reference reads it by value (`972944ad9`).
- Stdlib references: Json.get, at and as_text take &self and return copies (`3c07cfa6f`).
- Untrack the node_modules symlink; ignore node_modules links too (`18286c557`).
- join-list-order: channels set the finishing order, not 10 ms sleeps (`e5932f8a8`).
- tier-crossings: join-list-order uses channels now (`9d815839b`).
- Tour 17: the join! section imports join and prints join: (`c1cbc722a`).
- Dev inliner binds read-only reference parameters by value (`3304060c6`).
- Stdlib references: get copies, get_ref and get_mut borrow (`d93e378fd`).
- Folder cycles in std and dependencies give a diagnostic (`08597bee1`).
- std.core declares Copy, Eq, Hash and Hasher, which List and Map name (`224fa3459`).
- The entry glue moves to std.rt.entry; the runtime folder is a leaf (`18d9baf90`).
- std.rt.bytes declares each shared byte, text and panic primitive once (`100a2acb1`).
- cli/property-stateful-replay: std.testing's stack lines after the std.rt.bytes import (`e7f260c35`).
- Checked builds root std's checked runtime folders (`f5cc84306`).
- Map.get returns a reference into the value slot (`690d0c71c`).
- Map.get copies again; the reference lookup is Map.get_ref (`f5fa4020b`).
- (&T)?.copied inlines, and a map store still reuses the probe (`b7bb1b771`).
- A short get_ref or get_or_insert loan takes no count of the table (`e7f2784b7`).
- A short loan of a list element takes no count, uniqueness check or pin (`9b741c2b0`).
- Server SDK schema: refresh for the get_ref branch (`717b76ad8`).
- Server SDK: refresh the schema after read-only receivers (`0ac5673e8`).
- Cleanup: join! names in docs, a stale status line, the merged references package (`9af95bd2f`).
- Stdlib references: two collection families (`1667c9391`).
- Stdlib references: copy-on-write collections have no _ref or _mut accessors (`49462b571`).
- Counts: a short element-place loan takes no count (`c57222f05`).
- CI: codex/** branches run the Test workflow (`b2f8be986`).
- Stdlib references: iterators over non-copyable items (`bf316521b`).
- Stdlib references: UniqList and UniqMap have no literal form (`d2bf774e3`).
- Audit: sweep every intrinsic, runtime helper, built-in type and known item (`2c8037b3f`).
- CI: run the slow suites as parallel shards (`ce171b8be`).
- CI: each conformance mode runs only the spec suite (`e198f57b4`).
- Conformance: workers take the costliest cases first (`8dfd4a928`).
- CI: build the CLI before the Windows host tests (`77bf60b55`).
- CI: fewer jobs per run, cancel superseded branch runs (`4c5e3a6ea`).
- Remove accidental iterator methods (`caece8ae2`).
- Audit: intrinsics review results and next steps (`44677c272`).
- Measure string and collection costs (`e98cd333b`).
- Audit: record the intrinsic review decisions (`e36ac955c`).
- Move floating helpers into std.num (`9a7a93bee`).
- Coalesce all function sizes (`f02cff980`).
- Lower float helpers to Wasm instructions (`2fdfa1174`).
- Benchmark float power in bench/micro (`c2b2b70df`).
- Web compiler size gate: cap at the 2 MB ceiling (`5a2da2e24`).
- Inliner: build explicit copies bottom-up in dev (`8f593ff25`).
- Inliner: honor nested explicit hints past auto limit (`beee5a557`).
- Trust hd-lang version in the server schema (`ad9591c63`).
- Move a borrowed local at its last whole read (wip for #1254) (`91011ecef`).
- Leave an uncounted cell payload in place on a move (`35be60034`).
- Borrow a match through a reference past an addressed binding (`873df22aa`).
- Address substring comparison review (`18ac25ef2`).
- Logo: clean the h's leg and the d's joint (`a43fc723f`).
- Trap a buffer that a plain release reaches (`413dd5d5d`).
- Box an open value whose one address is a buffer (`a64a554b7`).
- Design: raw buffers, lists own their elements, no copy of a non-copyable value (`aa7b6642f`).
- Design: raw buffers: the field-move copy #1283 found (`1a908eb80`).
- Design: raw buffers accepted: Buffer owns raw memory, lists own their elements (`1783a8e92`).
- Verify after the ownership passes that no non-copyable value is copied (`a4fec8170`).
- Borrow match payloads through references (`3be1c5a2a`).
- Add std runtime drop glue (`2b458ad34`).
- test Drop across owned placements (`94015b49c`).
- Generate erased-call conformance matrix (`b03e19f7c`).
- Keep ownership when a tentative parameter loan fails (`4043f2def`).
- AGENTS.md: performance work fixes general causes, not cases (`6ba32cc76`).
- AGENTS.md: intrinsics are the fallback after hd and the optimizer (`7d1a286ce`).
- Collect erased closures only through concrete construction thunks (`984991022`).
- Preserve open trait references across erased calls (`7aa107af0`).
- Raw Buffer: UniqList and UniqMap own their lengths and elements (`6b3ee7460`).
- Skip Drop instances at open types; document the raw buffer (`249334517`).
- Run Drop when a store through a reference replaces a value (`2b2383ed4`).
- Count a pointer read as a write in the release write walk (`4dadfdc1b`).
- Bless a vtable slot number one function lower (`9b7518a14`).
- Keep std.rt.memory free of top-level bindings (`e0b4d2e49`).
- Drop in place, and read a Buffer through its owner's reference (`e16563338`).
- Remove Ptr.drop_in_place; keep UniqList's drop a plain hd loop (`1398d97a8`).
- A whole store ends a celled local's life, so a loop-local moves each round (`1f236ae34`).
- compiler-tests skill: report the default V8 tier only (`6021f959b`).
- CI: run conformance in checked builds too (`6e7976b7d`).
- Move inline Drop arguments through synchronous dyn calls (`fdcb2b041`).
- AGENTS.md: test what the change touches; the full suite is CI's (`8b6407ad1`).
- AGENTS.md: a local full suite is only the last step of a cross-cutting change (`523278376`).
- Count std.rt among the std modules the checker test expects (`36071e13d`).
- A loop over a &mut binding's list sees that list whole (`decb2546e`).
- A reference into a local's cell takes no count (`fdaf36f3e`).
- Design: pointer aliasing model A, its stress test and the pointer-loan matrix (`ab10e2aca`).
- Narrow the descriptor's glue slot to 16 bits and reserve the borrow state (`40faf5f25`).
- Check the borrow state on accesses through a pointer (`7f0ee59cc`).
- Take pointer loans on bound references (`26b914f51`).
- Spec: run-time loans hold whole objects (`78cce681d`).
- CLI: a release build panics on a write to a borrowed object (`13a222a6d`).
- A frame slot zeroes both header words (`75a4ff224`).
- Keep a hand-written conformance row outside the generated matrix blocks (`96f4a6902`).
- Move non-copyable temporaries without counted data (`f438543e9`).
- An implementation on a pointer type keeps its declared pointer parameters (`060a4cd61`).
- Structure walkers hold a &'a mut instead of a *mut (`5b259b077`).
- Take call loans, with two-phase receivers (`a7a5deab6`).
- Elide pointer loans nothing can meet (`499472e27`).
- Model A: append and drain through an alias panic borrow-conflict (`2e944be70`).
- Returned and reborrowed references keep their loans (`427eacb87`).
- Cleanup: RAW_BUFFER phase status (`2f94b8742`).
- Hold pointer loans across suspension and end them with an abandoned task (`2a2987e2c`).
- Order the join! loan extra by poll order, not wall-clock time (`9d0b61c52`).
- Hold loans on the cell of a variable a closure shares (`2cfb47aeb`).
- Make a written top-level binding a reference reaches an object (`920134fb4`).
- Element references: drop the pin's length panics, check them statically (`834489ecd`).
- UniqList and UniqMap element references hold pointer loans; drop the room pins (`d69e87bcd`).
- Order the elide-other-task loan extra by poll order, not wall-clock time (`ea2587373`).
- Remove the loan table; pointer loans alone check aliases (`6d68c012e`).
- Keep a reference's root alive through element places and in bodies with no frame candidate (`90c5c68e6`).
- Lend a counted reference coerced to a local one (`63b61bcdb`).
- Follow stores a call makes through a `&mut` of a local, and places rooted at a whole-local read (`5a4d2a157`).
- A local reference never takes its local's cell (`db3f45e01`).
- Point a local reference to a closure's captured copy into its environment (`a55911a52`).
- Let a local reference a sub-body returns take its cell; re-record println's TIR (`3d71b6373`).
- Keep roots alive through calls' dependent results, closures and suspending bodies (`94d86e1e1`).
- Keep a suspending call's lent arguments until its await (`5f00ae360`).
- Revert "Use exact-width loads and stores for short byte copies" (`dee0e2ed9`).
- A local reference counts nothing; only a view owns (#1336 phase 10) (`e522af694`).
- Release counts a counted pointer carries into a local reference (`80e0ebf72`).
- Remove the element-reference pin (#1336 phase 10) (`b1db0d7ee`).
- Move the observer-notify loan case to the same-root typing set (`9c79afde7`).
- A call result through a *T is a local reference to the pointer's root (`9b2ccdaf1`).
- A closure checks a captured pointer against its creator's own loans (`7cc1994b2`).
- A *mut dyn receiver takes a call loan on the object at its data address (`5e336542f`).
- Spill shared loans past 31 to a side table (#1336 phase 12) (`68d5b880b`).
- Grow the loan spill map on demand; keep suspending creators' cells live (`740d7c6f3`).
- A suspending closure checks against its creator's own loans too (`b30e05109`).
- STDLIB_REFERENCES: slices 1-4 built, 5-9 tracked (#1348-#1352) (`34f861549`).
- STDLIB_REFERENCES: Index is element access; range indexing becomes a method (`17df74582`).
- Drop calls of runtime helpers that do nothing (`f64d7691c`).
- Drop inline values through local stack slots (`ceb33c64f`).
- Feature snapshots for the static count audit (#1364) (`896f1dfde`).
- Unique collection cursors release their rest through field glue (`5ab5fa357`).
- Feature snapshots, round 2: chapters 11, 13, 14 and the std surfaces (#1364) (`dae8d82c7`).
- Brainstorm: ownership compilation architecture (`4336341bd`).
- Brainstorm: measure Option 1 against Option 2 (size, compile time, memory) (`d1e8f3bc5`).
- Design: decide ownership once per instance in TIR (Option 1) (`dd69c323d`).
- Accept Ownership Per Instance; lift once-per-generic-body for inlining and ownership (`0aa1bdf93`).
- Decide a view-free module before emission; release no thin reference there (`b5947c509`).
- Revert "Decide a view-free module before emission; release no thin reference there" (`1e2b7fd0c`).
- Give every emitted count operation its reason (`b8144ac98`).
- Design: ownership step reports use default-tier V8 only (`e9d9ce202`).
- Re-measure Display.write after the Drop follow-ups (`67ebff332`).
- Never copy a non-Copy value in a match through a reference or an inlined Buffer receiver (`d0076deec`).
- Default conformance runs on CI only (`daa725a23`).
- AGENTS.md: never run the full suite locally (`f750ee3ce`).
- Revert "AGENTS.md: never run the full suite locally" (`8f25d3467`).
- Expect Display.write's frame in a panic from to_string (`968c9dba9`).
- Cleanup: pointer loans built; status lines follow #1336 and #1399 (`20ceef931`).
- An implementation on a pointer type returns its box counted (`fa99431f2`).
- Keep projected views alive through borrowing bindings (`193ab659a`).
- Keep the JSON tree fallbacks out of the inliner with @cold (`84eb77b39`).
- Own each release generic instance once, through its own body (`c8e8e765a`).
- Migrate remaining map-loop test callers (`01f175571`).
- Copy the borrowed map value in the std runtime test (`cd1493e19`).
- Own each dev generic instance on its own body (`d2d319ba7`).
- The ownership passes' counted answers yes or no (`b7019954c`).
- STDLIB_REFERENCES: generics stay copyable by default; movers add no intrinsic (`915ea9f5b`).
- Classify value locals; the own view prints them (`2c2b27c6d`).
- Keep new call fixtures before generated matrix rows (`983f68a83`).
- Playground test: a walker member borrows its value (`a43d657c6`).
- Cleanup: STDLIB_REFERENCES status follows #1348-#1352 (`dcc7d6aee`).
- Forward a value local's one read to its stored value (`25ee271bf`).
- Check the ownership migration plan against the feature map (`d1ef9c69b`).
- plancheck: function-scoped evidence and --show (`0ee7295f6`).
- Borrow pattern payloads without emitting discarded value reads (`e88013ca9`).
- Forward each re-stored value to its one read (`44aaa69e9`).
- scripts/lane-gate.sh: the per-step gate for #1399 (`e4b79d14b`).
- Classify a local stored on more than one path as a place (`3dbf95fbc`).
- Print each value local's uses, consuming or lending (`a62428254`).
- Print each value's last use on its path (`12469b8d3`).
- Move a whole value before a field store that refills all of it (`7b80d2962`).
- Borrow a list element that a local holds the list of over the read (`85f051f36`).
- Label each needed copy read with its reason (`d189d14d5`).
- Borrow a value into a call-only binding by sinking its source's drop (`a1c4820ef`).
- plancheck: an open step with empty evidence is a warning (`92799beb3`).
- Borrow a never-stored parameter across a suspension point (`738594ff3`).
- Lend a bang body's parameters from its frame when it never suspends (`64d046662`).
- Borrow a closure's whole-read captures from its environment (`bf486f6ef`).
- Keep a lent reference parameter lent when a closure copies it (`10f7a41ba`).
- lane-gate: select suspension and await cases (`ff2dd613c`).
- Count only a parameter's own stores in the suspension borrow check (`bd31fb3b1`).
- Mark a fresh buffer's uniqueness checks as needing no check (`144600d3b`).
- Mark a check that repeats an earlier one in its block (`1e7dd44ab`).
- Hoist a loop's uniqueness check past its &mut element references (`73ccb786e`).
- lane-gate: include hd_opt's clippy and tests (`68ed461cc`).
- Move a capture at its last use into the closure's environment (`4f0ab498c`).
- Call a known function reference's item directly (`e688a798e`).
- plancheck: check code anchors with ast-grep (`7fdf09d7e`).
- Let a default argument's function lend the parameters it doesn't keep (`d042f26fa`).
- Keep a using binding in a frame slot (`a722339cb`).
- plancheck: accept sub-step ids such as 19a1 (`3b00cb387`).
- Preserve live test environments when installing count reports (`3f76ea572`).
- Release nothing at a frame slot's declaration that finds it empty (`96a289928`).
- Print the function values a call may keep (`7e970f5e3`).
- Refresh dynamic count-operation baseline (`a8fabc5be`).
- CI: stop running agent checks on muse/** branches (`3a211d9bc`).
- Skills: chat replaces Muse in write-issue and cleanup (`5524da365`).
- Print the shared locals that could live in frame slots (`ccc727069`).
- Keep a frame cell's local alive over its closure's uses (`aee48be66`).
- Keep a frame cell's local in a frame slot (`8691b27b9`).
- Leave checked-build auditing and the ownership verifier out of the web compiler (`c61c344dc`).
- Borrow a trait value read through a pointer field in the exclusive copy (`9caede556`).
- lane-gate: run the optimized pipeline too, and cover panic/fact/default/init cases (`8a4970989`).
- A repeated uniqueness check behind a parameter reference runs no test (`d67e10c83`).
- Add a feature case for an element passed on through a reference (`6971ebc78`).
- Refresh dynamic count-operation baseline (`18d690904`).
- Hoist a loop's check past a kept copy of another part of its reference (`7c9de6951`).
- Record which collection fields have one owner, as facts (`0f0e725c5`).
- Cleanup: #1351 slice 8 is built (`fcd9b6758`).
- Refresh dynamic count-operation baseline (`cf6f02548`).
- Borrow the trait value a provider reference names across calls (`9dc6902ad`).
- Mark a single-owner field's uniqueness checks at link: no check runs (`f65e17cf3`).
- A `&` loop pins its list once (`0344801e0`).
- Refresh dynamic count-operation baseline (`7eceb6c62`).
- Assert a single-owner field's count in checked builds instead of skipping (`6f3004562`).
- A &mut List parameter that every call passes a single-owner field needs no check (`808c08b6a`).
- Add a feature case for a list parameter two loops write in turn (`256883cac`).
- A constant list literal is static data only as a top-level binding (`fb3fa35de`).
- A call through a trait value lends its arguments in TIR (`4428ac729`).
- Counts: record 15f's measured increases (inventory +1,536, structure_walk +1) (`4e86cfedb`).
- Revert "A call through a trait value lends its arguments in TIR" (`d72c2160a`).
- lane-gate: run std's own hd tests on the linear backend (`bcc70c791`).
- A call through a trait value lends its arguments in TIR (`dd0dc0a91`).
- A list literal's spread and a map literal's key lend their values in TIR (`4c93856c4`).
- Spec: regenerate matrix rows in cases.tsv; record a tier crossing (`e9697ea6e`).
- A scoped loan bound to a loan local moves into it (`15d0b27b8`).
- A direct call's generic parameter takes its instance's pass (`beafcbe2a`).
- Spec: drop duplicate manifest rows left by a merge (`d38aadff4`).
- Delete release_untaken: TIR lends what the emitter lends in every pipeline (`0a37096e4`).
- Stage tests: HD_TEST_JOBS caps the fixture pool (`f9030ce29`).
- Refresh dynamic count-operation baseline (`db09e514e`).
- A loop's shared element references no longer block hoisting a change's check (`d591b388a`).
- An exclusive copy hoists a loop's check over a call that may write (`83c87b718`).
- A map literal takes the keys it consumes (`6ef6231fa`).
- Refresh dynamic count-operation baseline (`5a90b6889`).
- A copy's own calls in a loop are copied in later rounds (`a5e29b1ac`).
- A copy's reference parameter takes a fresh local Ref at its local type (`ff4872899`).
- UniqList.push is inline, with its growth a call (`b4d5385d0`).
- A small &mut callee of plain memory work inlines at a call in a loop (`8a30eb809`).
- A loop's &mut copy keeps to callees that return nothing (`1cdfbc362`).
- An inclusive range loop runs one branch per round (`710cef163`).
- A LoanEnd statement ends a holder's loans and releases nothing (`8c4a400ec`).
- A reference that can own nothing ends with a LoanEnd, not a Drop (`e13e7af25`).
- A capture at its value's last use moves from step 8's facts (`52419838f`).
- A reference to a parameter or a capture's copy keeps its Drop (`8e88faa98`).
- A value local that only renames another reads the other (`364a49fa7`).
- A list parameter's loop keeps its range proof (`e7788ceee`).
- Report the web compiler's gzip over 2 MB without failing (`4756a460e`).
- A payload field at one offset of its enum is a plain address (`15d75fcea`).
- The lane gate runs a step's own conformance cases (`e3d6a4c64`).
- A method's own type or row parameter makes it unavailable on dyn (`02c84142d`).
- Delete erased bodies, thunks and witnesses (`e29dfdf1f`).
- Refresh ownership count budgets (`739478436`).
- Check always-inline functions without decoding every body (`e78003879`).
- Local cleanup settles chains of copies in one walk (`9a687e709`).
- Remove what erased bodies left in the ownership passes (`c229a1974`).
- Drop the CLI test of a generic method through dyn (`14281c89b`).
- Refresh ownership count budgets (`2fad471f3`).
- A function value's rename reads the other local; sink_drops goes (`30fb6233e`).
- Refresh hd_check TIR baselines after erased bodies, and gate them (`e90564fb3`).
- A bless is a review; correct one stale expected note (`fe66ee6d5`).
- Liveness settles a forward-only body in one pass (`555f167e2`).
- A build on std does not re-check std's always-inline promises (`45e1f18e1`).
- lane-gate: a caller that just ran the stage tests skips them (`07c512582`).
- A counted part has its own liveness bit; pairs.rs goes (`311fd4e7f`).
- Stage views share one store, so std is prepared once per build setting (`08b30d619`).
- A std stage case stays out of the toolchain key, so std is checked once (`e8531eae9`).
- spec/check.sh builds only the parser (`e5052d6c4`).
- Borrow windows go: a reference that outlives its call blocks the borrow (`59bce6ee8`).
- A pointer read written back moves out and in (`d951a0dd2`).
- Std's check entries no longer depend on the root package's name (`28cf40078`).
- hd command tests share one object store, so std is prepared once (`6e693d9e3`).
- A reference into a lent parameter needs no mark; lent_refs goes (`ee5bf0e53`).
- Show both copies in the case for reads after a call result (`88611381e`).
- A program decodes only the items it reads (`6fd491f2d`).
- A body's table entry allocates its decoded forms on first use (`d9eb65bcc`).
- A CLI case runs @cfg functions, defaults and methods in every profile (`9ffdbebaf`).
- Compare link-time TIR pointer-loan decisions in checked builds (`3c28f2b49`).
- CI: one Checks job, one default and checked conformance job (`dc0abd3e4`).
- A derivation of borrows runs beside the old passes and reports differences (`0d3c7949c`).
- Cleanup: docs stop describing erased bodies and pairs.rs as current (`919b2cdfd`).
- A build reads a std module's check entry when something first asks for it (`de20e73e3`).
- Executor determinism covers real bench programs (`f4ca08abe`).
- Finding dead accesses needs no sorted liveness sets (`a13541661`).
- The derivation's sources stop at a binding that owns, and it can decide (`8162a865e`).
- The derivation's tables are per value and per local (`7f652d7e5`).
- Inline the append vector's lookup (`7f1e9254f`).
- The pointer-loan pass reads each body once (`114a2a6e1`).
- CI: each host shard keeps its own Rust cache (`6e23d1e51`).
- Windows hd command tests share one object store too (`27e36a55f`).
- CI: Windows and macOS run on host code and twice a day (`af4370e03`).
- The derivation drops what its other facts already cover (`7d9be8820`).
- Cleanup skill: hd-ir-redesign is no longer a standing checkout (`a79fc07b5`).
- A test that counts what a warm check rechecks keeps its own object store (`d0a876d5a`).
- Two cases for what the derivation's write rules rest on (`8a3951c2b`).
- A retain moved on to its consumer is stored, and a read can give its count (`cf51da5d5`).
- Refresh ownership count budgets (`27761ac38`).
- State byte-slice result retains explicitly in TIR (`4829b36e7`).
- State provider lookup retains explicitly in TIR (`cbe3bd5a8`).
- State direct trait coercion result retains in TIR (`87239364e`).
- State materialized trait widening retains in TIR (`1b1be943a`).
- Keep dependent pointer loans rooted at their collection slot (`544973891`).
- Keep dependent pointer-call loans on their containing objects (`8572e3f93`).
- Cover dependent pointer loans on closure-shared roots (`1c7c9b02b`).
- Trace dependent pointer loans through borrowed element bindings (`9a27a4a6d`).
- A part of a frame local moves at its last use (`c06aacc7c`).
- Refresh ownership count budgets (`5197fd0cd`).
- State indirect trait widening retains in TIR (`c2b940d5a`).
- The derivation sees frame references and what points into a binding (`2541a59e6`).
- The verifier counts what points into a binding as a use of its borrow (`b80c7f378`).
- The derivation runs its extra facts only where they can decide (`ae0d8d5d2`).
- The derivation is the only one that decides which reads borrow (`44179ca00`).
- The derivation's rows need no generic sort (`bd04dd0df`).
- Refresh ownership count budgets (`c53d11287`).
- Revert "A local reference stored in a collection slot keeps its source alive" (`004ce9c7e`).
- Resolve function-root origins through checked TIR anchors (`e9319bad9`).
- State ABI-owned parameter exit cleanup in TIR (`edd85dc97`).
- State persistent cache payload copies in TIR (`df723c554`).
- State cold-frame argument copies in TIR (`6ca97d68c`).
- A store states the release of what it overwrites (`86f4ea8f0`).
- A store known to find its place empty releases nothing (`f15326679`).
- Refresh ownership count budgets (`19e67cfe2`).
- An abandoned frame releases what the body says its points hold (`50261b608`).
- A store of zero into a known zero local goes in any body (`e9ee0e56c`).
- A table column goes when the next instruction drops the result (`900f6a661`).
- Refresh ownership count budgets (`2de70902b`).
- A moved temporary is zeroed only in a suspending body (`a9e11b6a3`).
- State cold-frame requirement-row copies in TIR (`0316eb939`).
- Consolidate boundary counts into Retain and Release (`90c1249e9`).
- State MapSet key copies with operation Retains (`ea42c6a80`).
- State fresh suspension parameter copies at Entry (`d424699a6`).
- State source-frame materialization row copies (`c1703a6c3`).
- The derivation needs no walk of its own to find a read that may move (`0a913bfe3`).
- The derivation stops keeping values live through other uses (`b8b6e1cc8`).
- The move pass's lending guard goes: it never changes an answer (`eb3657b59`).
- A suspending body's parameter lives in its cell, and its cells move (`e4c248cc4`).
- A frame keeps the count of what its body borrows, and releases it itself (`e934f3ed6`).
- Refresh ownership count budgets (`dac1b9b1b`).
- A suspension's arguments are copied at the call by every route: checked by sabotage (`07aa6cb61`).
- An `if` with nothing in it drops its test (`c12a7437c`).
- State what the late-read pass is for, as measured (`a8e2023cb`).
- A spread's base lends (`a7def8d18`).
- Refresh ownership count budgets (`669d2ce63`).
- Complete suspension row contracts and remove row fallbacks (`e596a8d63`).
- Follow suspension row ABI facts for test bodies (`5a7a73970`).
- The count table shows calls that find no count (`ec4f4e0b3`).
- State suspending closure parameter copies in TIR (`d77bdf619`).
- A trait value read through a reference is the reference's own words (`dbc53bf88`).
- State awaited child frame counts in TIR (`037c59656`).
- A list local nothing shares takes no uniqueness check (`6118ea2ac`).
- Bench counts execute only modules they have not seen (`a41f01656`).
- State join waker ownership through frame teardown (`7df9a9495`).
- A small compiler edit rebuilds the dev binaries in a third of the instructions (`72d5ceb89`).
- Refresh ownership count budgets (`7b5f9493b`).
- State the frame-owned row teardown release in TIR (`84af6fc86`).
- A binding's borrow is no copy of a struct, and the two rules share their walk (`496dd3cd9`).
- Refresh ownership count budgets (`f5111bb31`).
- A stage case's views share one build (`a5ea4be61`).
- Std fixtures in conformance share the rest of std's checks (`2a4b3047e`).
- A count call stands behind a test of what it would answer (`ed5d04e2d`).
- hd_driver unit tests reuse one prepared std (`f17898f5f`).
- Count calls that find no count have a limit per program (`ca42ee826`).
- State the frame child-slot teardown release in TIR (`eee3782a2`).
- A store that keeps fields of its own place stores only what changed (`b60e5e50a`).
- Bench count notes pass the JS lint (`98dcde12b`).
- Refresh ownership count budgets (`7d5e4dfda`).
- A uniqueness check that runs says its buffer may be shared (`a940ce40b`).
- State a cold frame's reference-parameter object counts (`68a6aec1e`).
- Refresh ownership count budgets (`ad7b0621e`).
- Audit emitted WAT and own-view IR for wasteful code (`a93885bec`).
- Test the IR audit from outside, and fix what that found (`2571e3e6a`).
- Fixtures for unique lists ending once in frames and on every path (`6e925b325`).
- Two ownership rules that never fire go (`37c186903`).
- Cleanup: audit tools in the compiler tests skill, measured status in the ownership design (`09090fbf3`).
- State closure environment capture copies in TIR (`1914d40bd`).
- State the key copied by an inserting map probe (`1a2b73351`).
- A store into a one-field struct's field stops counting as a whole store (`9ed3829a4`).
- A moved part leaves the base's drop whole: moves.rs rest goes (`826c7fa0c`).
- A temporary's drop stays where insertion put it: moves.rs sink goes (`5c6a2dd2b`).
- A closure's body can't move a captured value out (`453b07fdc`).
- A count that would only cancel is never written: count_pairs goes (`a842017f2`).
- Refresh ownership count budgets (`f843ea442`).
- State frame-dependent local reference copies in TIR (`8cc8f956f`).
- Every count op in a compiled body has a reason, or a checked build panics (`fe35560d8`).
- A named release must have its fact, or a checked build panics (`9187b16bd`).
- State a cold closure frame's copied capture packet (`e4b7d7d01`).
- State the provider copies a context owns (`948e69979`).
- Audit stated exit releases the output doesn't have (`17bb09339`).
- State the row counts of a function-value call (`a588b6f86`).
- Keep tuple-spread argument copies named (`994b59dee`).
- State the row counts of direct and dynamic calls (`33bd27b41`).
- A copy of a local bound to a field's borrow shares that field (`69ced8290`).
- State function-value spread argument copies in TIR (`af91581a2`).
- Checked builds panic when a started frame is freed or collected (`b8d580b0d`).
- CI runs conformance in checked optimized, and std's runtime modules checked (`27ed2f24a`).
- State inherited struct-update copies in TIR (`ecb9041d7`).
- A parameter stored in a field is its caller's, not a fresh buffer (`e3055c294`).
- A body that runs over a frame gives its references no frame slot (`9c516502c`).
- State map lookup hit payload copies in TIR (`88a3e0fc9`).
- Preserve map probes across nonexecuting count metadata (`c1f57a00a`).
- A local reference is `Ref[T]` wherever its address lies (`154d2c0be`).
- Only what glue calls lends nothing, not every std.rt item (`7d92656f0`).
- A provider bound to a local and read only through takes no count (`86aee2e2f`).
- State map removal hit ownership in TIR (`034f616dc`).
- Refresh ownership count budgets (`bfe659758`).
- frame_loans goes (`06ec58fba`).
- What the carrier needed goes (`8a54513d3`).
- Refresh println's golden TIR for local references (`fe517f202`).
- Delete the one-owner field rule (`c7f2b12db`).
- Delete the OWNED uniqueness word and its checked assertion (`051fa2c56`).
- Record the count rises from deleting the one-owner rule (`0168db871`).
- State where each local lives, and assert it against the emitter's answer (`3bd6ce84c`).
- The emitter reads where each local lives (`e06912c6a`).
- State completed join-result cleanup in TIR (`6391ed54d`).
- A slot's parameter reference ends at LoanEnd (`50125e4d8`).
- A frame's .drop releases no value row, and a moved temporary keeps its bits (`aac27b833`).
- State what a suspension frame holds at each point (`eb3093eeb`).
- Revert "A frame's .drop releases no value row, and a moved temporary keeps its bits" (`d7f21d083`).
- A cancelled cleanup scope releases a moved temporary once (`efc8283ac`).
- A frame's .drop releases no value row (`45f14933c`).
- A suspension frame saves only what each point holds (`a5ae3c52a`).
- State borrowed direct-call argument copies in TIR (`94e53a149`).
- State copied-left string-append cleanup in TIR (`8c20fda31`).
- A frame's .drop releases no waker or join! result (`d841b1e7c`).
- A frame's .drop ends no call loan, and a suspending body keeps no loan delta (`2577da2b6`).
- A frame's .drop releases only what a cold frame holds (`397712ed2`).
- Test that a later point's cancel releases no earlier point's temporary (`800c3206f`).
- Refresh ownership count budgets (`3fbcbbc3d`).
- The emitter takes a slot row as it stands (`ed35557c5`).
- A local's reference ends at LoanEnd without a frame slot (`1acb3d776`).
- The local passes no longer handle a frame save or reload (`365724bd4`).
- The emitter reads a closure's captures from the storage pass (`65489f866`).
- Decide lent parameters in one pass after collection (`dba061c1b`).
- Delete four public functions nothing calls (`666be63ab`).
- Delete hd_web's legacy run goal byte (`b269735dc`).
- Await and CopyStruct glue names no count (`f613261f1`).
- A body's result leaves with its tail's count (`ce2049d6f`).
- ir-audit: ir-exit-unlowered is information, not a finding (`341af6072`).
- A block's unread tail has no release of the emitter's own (`b8a7b77b9`).
- Delete the pending-edit rows that no pass before the splice inserts (`189aa5caf`).
- A stated reference copy is not checked against the emitter's own test (`6ccb395c1`).
- Map removals and joins release only what their statements say (`82581e81d`).
- Copy statements are not checked against the emitter's counted test (`f91c6ee60`).
- Fold no constant into a store's offset; rule 9 already does (`969d3ad5a`).
- Delete the copy verifier's survey mode and its file report (`3fccc1d72`).
- A place append lowers its left as its statement says (`7001097d1`).
- A whole move leaves its emptied Drop to known-null (`a632774be`).
- Delete the emitter's Drop of a part of a place (`6b42021a4`).
- Delete the data-object layout: every declared data type is a struct (`ddd5fdccb`).
- A suspension frame saves none of the emitter's locals a resume rewrites (`b207f7c54`).
- Delete Summary::meet, which only its own test calls (`31848358a`).
- Document each kind of type the pool holds (`b0b70be2d`).
- Drop the data-object leftovers from the emitter's docs and place refs (`dab09dcf1`).
- Delete the impossible checked-poll fallback (`6e32bf830`).
- Delete the fixed-class allocator fallback paths (`de660a670`).
- A dead set leaves its pure push to rule 16 (`5a143e487`).
- Delete the fixed-class free fallback paths (`eea23747c`).
- Delete the local cleanup's rule 8, which never fires (`1c8dc7793`).
- Delete nested release helpers runtime-absent returns (`2f78a34d6`).
- State cell-owner cleanup after whole-local moves (`f6ae09379`).
- Pin hoisting out of a `while true` loop left only by return (`173e96396`).
- State plain parameter entry copies in TIR (`1e2617f5e`).
- Forwarding names no ownership statement: none exists before insertion (`ba37bb55c`).
- Splice skips no block it never visits (`3cc1f9573`).
- Delete duplicate collection drop-helper fallbacks (`e7f6e81c1`).
- State closure capture entry copies in TIR (`976eaf7dc`).
- State deferred map probe uniqueness in TIR (`413c473c7`).
- The ownership passes and the emitter handle no CallDyn: nothing makes one (`5155e7d86`).
- Pin a late read moving past a closure that copies its local (`34173a297`).
- State a default's unused parameter release and delete the emitter's unowned-parameter path (`b9b031dc1`).
- REPL session tests build linked modules, as the REPL does (`8e16dc37f`).
- Delete the REPL's whole-history build and replay mode (`12fdabc2c`).
- Remove the stored struct flag and old data-object paths (`064bf1dc7`).
- Label a closure-shared local's copy reads from the cell, in one shared rule (`dddbbb882`).
- Rename the struct item kind from Data to Struct (`c44903ef8`).
- Let a `$.with` trait value take its stored local copy's count (`8b2d91a26`).
- Delete the impossible uncounted reference-parameter fallback (`94fa79b2c`).
- A copy read lives through the loans of its value, in one shared use walk (`009c35c33`).
- Move block_on's completed result into its variant (`1fbe323f7`).
- State an inserting map probe's default copy in TIR (`8603b4bc2`).
- Refresh ownership count budgets (`94e2847f9`).
- Delete the impossible empty slot-ownership fallback (`bff73bd2a`).
- Cover boxed enums with associated-type payloads (`67158cfa6`).
- Lend a map lookup's value while only read (#1366) (`108c40af4`).
- Replace cell-move unit tests with source-stage coverage (`7012d1573`).
- Cleanup: drop data-object text and a finished audit (`c4796508e`).
- Replace parameter-copy unit tests with source-stage coverage (`e8c64e71a`).
- Website: give the playground fallback a command that runs (`a8de22f95`).
- Convert the last four .js scripts to typed .ts, and check it (`73c99ce02`).
- CI runs the TypeScript type and source checks (`fa50203ad`).
- Replace capture-entry unit tests with source-stage coverage (`f4285c6a1`).
- Check code against the spec: stale rule citations and removed forms (`479f47b75`).
- Ignore the owner's local .claude hooks and settings (`750293d00`).
- Audit language specification correctness and consistency (`ed2029dac`).
- Measure core line coverage, and check it on a schedule (#1442) (`a20a4baba`).
- Coverage includes hd_driver's integration tests (`d55bcb086`).
- Check spec examples as printed (#1478) (`69918bade`).
- Spec example: identity with two shared references (#1474) (`a4531fb57`).
- Spec example: the handle example borrows for get (#1475) (`024c60f68`).
- Spec examples: signed literals where the text means i32 (#1476) (`0e789951b`).
- Record the core coverage baseline from its first CI run (#1442) (`1016ac1cd`).
- A type holding a local reference is not Inspectable (#1455) (`dd964bb24`).
- hd_check: drop unsupported guards on shapes the parser never builds (#1489) (`a895467ad`).
- hd_check: an explicit-type-argument call of an unknown name is unknown-name (#1489) (`e71467172`).
- hd_check: a trailing block for a vararg is trailing-block-ineligible (#1489) (`bfe3d50dc`).
- Write suspensions as Suspend[T]; a type cannot start with mut (#1443 step a) (`26ae280b2`).
- cancel! and abandon take self in their rules (#1443 step a) (`8f6387b8b`).
- Fix main after #1443 step a: the annotation lint arm, expectations, schema hash (`c8c5cc52e`).
- Spec: numeric comparisons give bool, separate from operand unification (#1464) (`2b552e5a3`).
- Spec: & and &mut bind tighter than every binary operator except ** (#1470) (`afd1f03d2`).
- Spec: the bang-call token rule allows explicit type arguments (#1469) (`f376291e5`).
- Spec: the scalar-boundary panic applies to range operations only (#1463) (`7760f89cc`).
- Spec grammar: add deref assignment target, Fn bound sugar and plain dollar text (#1468) (`eeccf80eb`).
- Spec examples: stop assuming unsuffixed literals are i32 (#1479) (`3e344671a`).
- Spec example check runs two jobs, not eight (`3b96141a1`).
- Spec: mut self is the method's own copy on every dispatch path (#1458) (`973d7ee73`).
- Spec: fn values read from parameters or fields implement only FnMut (#1460) (`99c093766`).
- Spec: retire the blanket rejection of lifetime trait and alias arguments (#1462) (`8ad03b46e`).
- Spec: any trait may be dyn except Fn and FnMut (#1466) (`c9b7ce537`).
- Spec: match and negative-impl statements allow the !Copy cases (#1471) (`2b4ccd822`).
- Spec: delegation passes the field in the receiver's form (#1472) (`953d42635`).
- Spec: examples that pass value closures where std passes references (#1480) (`e3965feb7`).
- Spec: examples where a local reference escapes (#1483) (`2ec08732f`).
- hd_check: call.rs unsupported refusals, folded or re-coded (#1489) (`60c811d7d`).
- hd_check: expr.rs unsupported refusals, folded or re-coded (#1489) (`9b778066f`).
- hd_check: drop unsupported guards on statement and pattern shapes the parser fixes (#1489) (`da38954dd`).
- conformance: keep the generated pointer-loan rows last in cases.tsv (#1489) (`f47e78412`).
- hd_check: drop the empty-prefix guard on a prefixed string (#1489) (`4476ae06a`).
- Spec: handle record types are opaque outside std.structure (#1456) (`1d068e19c`).
- Spec: enum type parameters are out of scope in shared initializers (#1459) (`514452f37`).
- Spec: the folder-cycle fix-it applies only when a flat file exists (#1473) (`1c62e43cb`).
- Spec: std-num.u64.checked is a panic, not an error (#1477) (`00eec966b`).
- Spec: interpolation may hand write the destination builder (#1457) (`3f072cf0e`).
- Spec: race! polls its children in argument order on every poll (#1467) (`a18543356`).
- Declare Inspectable with ?Copy so impl !Copy types are inspectable (#1461) (`83b3d2cb7`).
- Delete req.suspend.type.equivalent: fn! and fn -> Suspend are distinct types (#1465) (`8f8b4acfd`).
- Import std names in examples that used them unimported and classify them complete (#1481) (`f624f945a`).
- Classify the Display and Hasher declaration examples as fragments (#1482) (`5ead6ac84`).
- Fix examples that broke a rule of their own chapter and classify them complete (#1484) (`607b3e146`).
- Show the conceptual suspension frame as a plain struct with an inherent poll(&mut self) (#1443) (`677be7867`).
- Check error examples as printed: marked lines and codes exactly (#1486) (`8c8d6a7cd`).
- Check the hd blocks of the docs outside spec/ as printed (#1487) (`5943de209`).
- Let downcast_ref and downcast_mut recover a non-Copy target (#1461) (`c93ae94ea`).
- Name a counted reference *T or *mut T in TypeId names (#1491) (`483886e39`).
- Delete applied future-work designs and fix stale citations in the rest (#1488) (`0b0cd8f51`).
- Sweep docs/compiler: retired rule IDs, type-position mut, deleted tests, Wasm GC engine settings (#1488) (`d501a2315`).
- Sweep crate DESIGN files and crates/README against the current spec and tree (#1488) (`dbd2ccedb`).
- check:spec-sync reads the docs: stale rule citations, removed forms, dead paths (#1488) (`7e04fcd77`).
- Skip the pack-directory walk when the recorded total fits the cap (#1501) (`6a772c87f`).
- Skills: a cost model for every per-run path (`ea3fdc97e`).
- Reject delegating a trait with a counted receiver method (#1472) (`6c82f7dad`).
- Name the five unsupported outcomes the owner decided (#1492) (`f744437e8`).
- Report a projection with no selecting bound at the projection (#1500) (`25fd49d2d`).
- check:cost: ast-grep rules for directory walks and per-item processes (`f73f02c1f`).
- hd_cache: count a warm run's file-system work, flat at ten times the entries (`e498c940f`).
- hd_driver: a warm no-change check reads no source file (ignored, #1503) (`b6eb5cb87`).
- CI: time budgets for spec/check.sh and each check:* step (`4ecd83dd7`).
- Cost allowlist: the two untracked rows are #1508 and #1509 (`863f65bae`).
- Move the ast-grep config and cost rules into .ast-grep/ (`494c0538f`).
- hd_cache: a paged index, so a run reads and writes index bytes for the keys it touches (#1502) (`3dbc55a68`).
- Keep a joined collection type available to empty literals (`ad78aaf17`).
- Replace map-probe synthetic unit tests with source stages (`4339b41bd`).
- Remove the unproduced two-argument map probe path (`8fbf51447`).
- Check spec and doc examples in the Rust conformance harness; delete the TS runner (#1504) (`aec24d8d7`).
- lane gate: select steps from the diff, crates by reverse dependency (#1505) (`63dfd51d2`).
- Cleanup: the ownership design notes that #1399 closed (`cade6b491`).
- Cleanup: the finished spec audit report, a stale ownership status line (`f551db9a1`).
- Stat manifest: a warm run reads and hashes only changed source files (#1503) (`c8b2be044`).
- hd_cli: the stamp constant and stamp tests are Unix-only (#1503) (`b279efd9a`).
- Lower &mut T to its own carrier, not Ref[Mut[T]] (#1443) (`6672e3917`).
- Mark a `mut self` receiver on FnSig, not as a `mut Self` parameter (#1443) (`418d73f45`).
- Delete the checker's readonly-view model (#1443) (`f034fe340`).
- Drop the structure generator's fresh `mut` marks (#1443) (`3ee95faf9`).
- Delete TyData::Mut (#1443) (`d562517cc`).
- removed-forms: TyData::Mut and has_mut_access are gone (#1443) (`cf74e251f`).
- Drop "`mut` or not" from type-test doc comments (#1443) (`f7128398b`).
- Fix main after #1443 step b: a test's RefMut spelling, json's recorded count (`208ca8593`).
- hd_check: drop unsupported guards the parser and the header check rule out (#1489) (`22a4f031c`).
- hd_check: a top-level compound assignment, and body type names that reach no type (#1489) (`b5fe5e960`).
- hd_check: a positional spread fills a variant's shared parameters (#1489) (`3b33756e2`).
- Record the std.text tier crossing of a #1489 fixture (`456bf2c6a`).
- hd_wasm_linear: drop emit.rs unsupported guards earlier stages rule out (#1490) (`1719f7464`).
- hd_wasm_linear: drop refs.rs field-step guards the types rule out (#1490) (`891a0d0fe`).
- Refresh ownership count budgets (`c06268aab`).
- std.rt.bytes: bytes_index_set, a byte-set scan std.json calls with its escape set (#1445) (`88348a29e`).
- hd_mono: a no-slot member referenced at a trait value runs its default (#1490) (`1b233f23f`).
- hd_wasm_linear: drop dyn_value.rs and emit.rs guards earlier stages rule out (#1490) (`c76c8f276`).
- hd_wasm_linear: drop emit.rs, closure.rs and ctx.rs guards earlier stages rule out (#1490) (`427343b6d`).
- hd_wasm_linear: drop value.rs, refs.rs, susp.rs and text.rs guards earlier stages rule out (#1490) (`c779608d0`).
- Replace 24 checker refusals with diagnostics and invariants (`388076d7b`).
- Resolve tuple rest types before publishing checked bodies (`2d2bac041`).
- Fold twelve checker refusals into existing stage contracts (`7b2602fd7`).
- Diagnose pending proof depth and named optional payload fields (`08c640726`).
- hd_wasm_linear: a trait value's row arguments decide its slots' context (#1519) (`45007ee9a`).
- hd_wasm_linear: delete the string and to_string builtin arms no std intrinsic produces (#1513) (`31c8fd64c`).
- std.host writes and reads private-field boundary values; delete the backend's JSON codec (#1445) (`6df9eaf16`).
- Coverage baseline at c76c8f276 (`9603df710`).
- hd_wasm_linear: a trait's slots take one context shape for every row argument (#1520) (`5730e3f3a`).
- hd_wasm_linear: a constant default of any scalar layout, as a literal's (#1490) (`ed7a7d736`).
- hd_wasm_linear: drop map.rs, list.rs and emit.rs guards earlier stages rule out (#1490) (`7783fc37f`).
- hd_wasm_linear: drop text.rs, scope.rs, count.rs and other guards earlier stages rule out (#1490) (`67f91ab52`).
- Ban unsupported: a ratchet on each crate's refusal sites (`2f258f32e`).
- ast-grep rule ban-unsupported; the ratchet counts its matches (`5308df1b4`).
- Ratchet every kind of sloppy code, not only unsupported (`46280ceee`).
- Fix main: drop a test that pinned the banned unsupported refusal (`08b388f2e`).
- Sloppy-code rules cover the compiler core only, never tests (`94e7c75f6`).
- hd_types: every TyData variant cites its spec rule; Poison, Infer and Canon are internal (#1444) (`d0e02eb7b`).
- Rename the data declaration's syntax and head kinds to the struct form (#1444) (`7e36dd53c`).
- hd_resolve: the seeded reference carriers have no fields (#1444) (`549f4a2cd`).
- hd_resolve: one-kind Item fields move into their ItemData kinds (#1444) (`daee11547`).
- hd_resolve: seed List, Map, Suspend and the reference carriers as ItemData::Builtin (#1444) (`f03a842db`).
- hd_resolve: list the item kinds impl !Copy cannot mark (#1444) (`a8958c629`).
- Check every std @intrinsic declaration against its intrinsic's shape (#1524) (`4d7663143`).
- Lower the hd_check ban-unsupported ceiling to 29 after #1512 (`ae1bec419`).
- check-sloppy: a falling count passes; the merge lowers the ceiling (`c506d5a21`).
- Model declined fact-body generation as an optional result (`de29c8368`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`b08b4f0e2`).
- Lower the sloppy-code ceilings to the merged counts (#1522) (`1b37a3bd0`).
- Design: general enum layout on rustc's model (#1422) (`5009750ad`).
- Design: apply the owner's five decisions to the enum layout doc (#1422) (`d4ff7baec`).
- Read derivation block names and member lines in hd_lint, not hd_resolve (#1517) (`116c7a429`).
- Derive zero literals and discarded call shapes in hd_lint (#1517) (`13291a1d3`).
- Read mut tokens, payload binding names and later parameter uses in hd_lint (#1517) (`db5775749`).
- Lower the sloppy-code ceilings to the merged counts (#1517) (`950529de6`).
- Check constructor spreads and trailing callbacks as stored arguments (`dff9cd68c`).
- Preserve opaque occurrence types when placing constructor arguments (`365208e20`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`717212d41`).
- Resolve enum method references through ordinary member lookup (`17172b4b9`).
- ir-audit: ir-ref-drop is information, not a cost (`8b697cd2c`).
- Check and instantiate row parameters of local functions (`8bc84c368`).
- Pass resolved default slots into body checking (`93da1c269`).
- Check function-input spread operands without a type hint (`3b3175514`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`bcc8ad3c7`).
- Reuse the Iterable call item type in loop checking (`9321dd73e`).
- Infer propagation returns at the existing result join (`3921fe84a`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`1ad864290`).
- Reject non-value names in const generic argument slots (`3dc55a831`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`70b7fb932`).
- Register local declarations in every executable source root (`7a366140a`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`03b428c25`).
- Lay an enum payload out as one struct at one offset, in word cells (#1422) (`71a2a6c6a`).
- Delete the views on & now that payloads are physical (#1448) (`eb3cfaf05`).
- Give each enum leaf its own Wasm value, never two in one (#1422) (`bf19e815a`).
- Branch a match's decision once on the tag: br_if or br_table (#1422) (`ca58d4378`).
- Read a match through & in place: the tag once, each arm's fields (#1422) (`63c1a6358`).
- Keep the checked store of a unique buffer behind a reference (#1448) (`f255ea3d0`).
- Make "is this an enum" explicit in layout::flat, with no refusal (#1422) (`71b9d1636`).
- Delete the duplicate enum layout probe that came in by accident (#1422) (`f51c35eaf`).
- Lower the sloppy-code ceilings to the merged counts (#1422) (`6058b1768`).
- Use resolved declaration contracts in constructor checking (`9e7290801`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`e1c7391fc`).
- Refresh ownership count budgets (`cbf4fc7fe`).
- Check rest inputs in body-written function types (`9fb23ddfc`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`8dd63f62e`).
- fixture_views: expect the reordered struct offsets (#1422); drop ty.rs's unused import (`04791956f`).
- shared-rules: a retry or fixpoint states width and depth bounds, with a generated test (`ef6ea91af`).
- AGENTS.md: every commit is performant, well thought, and state of the art (`7d3999d09`).
- Infer unknown propagation kinds from later function returns (`1f8683be9`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`641fef571`).
- Diagnose unknown receiver types at field reads (`dde9ca237`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`341f29652`).
- Lay enums out with niches, rustc's layout_of_enum (#1422) (`af9f1a1b3`).
- An enum's absent variants take no discriminant (#1422) (`eb7ec3662`).
- DESIGN §23.3: enums by layout_of_enum, with niches and absent variants (#1422) (`3e913aa2d`).
- A moved niche field leaves its enum saying the variant it left (#1422) (`06a3541fa`).
- Lower the sloppy-code ceilings to the merged counts (#1422) (`e9e098dec`).
- README: a short satellite example for effects and derive (`da84bee44`).
- Cleanup: enum layout and views are done; removed-forms follows (`6feebe94b`).
- hd_wasm_linear DESIGN: one §23.17-23.20, and references as plain addresses (#1490) (`52c3d40c0`).
- A function value of box, assert_equal, identity_of, Ptr.read/write or Fn.call runs its call's lowering (#1526) (`88d631808`).
- hd_wasm_linear: drop the float-math intrinsic key test no key reaches (#1490) (`d8a8ec959`).
- hd_wasm_linear: intrinsic()'s last arm is ruled out, not unsupported (#1526) (`6047fc4c0`).
- hd_wasm_linear: a bulk chunk's width is a Width, so loads and stores match it whole (#1490) (`0b8f17980`).
- hd_wasm_linear: numeric conversions and float operators match their cases whole (#1490) (`eac035c24`).
- hd_wasm_linear: the layout view walks nested fields on a stack, with no depth cap (#1490) (`4ec8ef999`).
- hd_wasm_linear: `is` compares its operands' first words, with no value guard (#1490) (`5bd7185c9`).
- hd_wasm_linear: drop the coercion and weak-result layout guards checking and std rule out (#1490) (`73b1245e0`).
- hd_wasm_linear: a reference identity needs no layout guard (#1490) (`44aa67c94`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`9628f54a6`).
- Index the four function-value fixtures of #1526 in cases.tsv (`eeec3064b`).
- CI checks the change's output quality, on the changed lines only (`18206bb05`).
- Fold literal checking into lexer and parser contracts (`5c9a771d7`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`8325b5f39`).
- sloppy-std-path: the known-items table is exempt, as the rule's own remedy (`c1a5b686c`).
- Option is the prelude enum; T? builds Option[T] (#1444) (`23a64c673`).
- Fold expression dispatch into the parser contract (`a42e165dd`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`c1b7aebd0`).
- removed-forms: track interior *T code and the reference carriers under #1444 (`2bf097c31`).
- Fold call lookup guards into their stage contracts (`3484eba13`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`2a6485de7`).
- CI: retry the ast-grep download (`95df19f29`).
- CI: fall back to npm's copy of ast-grep when GitHub release downloads fail (`c67330781`).
- Receiver addresses take their reference kind when made (#1444) (`a0e93e1a5`).
- Return optional signatures from callable lookup (`f1b6935e9`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`9b56e243b`).
- Diagnose unresolved receiver trait choices at lookup (`f023edad2`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`410714487`).
- Bound method references take a local reference to their receiver (#1539) (`a3b35d248`).
- Reject unknown method receiver types before lookup (`284a0e137`).
- Align receiver diagnostic tests with lookup timing (`5d0be428d`).
- Landing page reads the rewritten README (`7122fe22e`).
- function_reference_builtin_methods: pass bound references to locals directly (#1539) (`166911329`).
- Report generated-body fuel exhaustion on its item (`7b309a9d0`).
- Return values directly for resolved declarations (`6106e041c`).
- Lower the sloppy-code ceilings to the merged counts (#1489) (`967df6f78`).
- References stay TyData::Ref through mono, opt and the backend (#1444) (`079884f90`).
- Mono subst and trait selection stop raising references (#1444) (`faef43d7d`).
- Delete the reference carriers, RefCells and raise_refs (#1444) (`818bc4e7f`).
- Review overrides for #1444 (`a677ea57a`).
- Lower the sloppy-code ceilings to the merged counts (#1444) (`b0df89c56`).
- function_references: expect &mut Counter, not the deleted RefMut carrier (#1444) (`794701190`).
- A reader older than a merge reads the index again (#1538) (`72a8657ed`).
- Cycle walks: one rule for every type's parts, Option included (#1537) (`14dae8413`).
- cold_moves walks every value enum's payloads, not only Option's (#1537) (`0298bd4ff`).
- hd_mono: delete the unreachable Wasm GC layout and its Option arm (#1537) (`3617e60f7`).
- Readonly payload weakening for every enum, not only Option (#1537) (`d3995d87c`).
- Lower the sloppy-code ceilings to the merged counts (#1537) (`b5bd55efa`).
- hd_types: test that the fn-deps erasure reaches every holder form (`e830515fa`).
- Core coverage baseline at e830515fa (#1442) (`b75360600`).
- One br_table per niche cell for nested enum matches (#1532) (`76e1bc25e`).
- Decide reads in place through & from a per-body use fact, not a body scan (#1540) (`190029d7a`).
- hd_check: the last three unsupported sites become real outcomes (#1489) (`674d43088`).
- check-change: a typing, parse or CLI conformance fixture counts as a fix's test (`2eecc5013`).
- Format check-change.ts (`eff477f14`).
- Fold a static trait call's constant result and drop the arm it rules out (#1518) (`49201b119`).
- Lower the sloppy-code ceilings to the merged counts (#1518) (`b1aa5591d`).
- A folded decision whose arm returns ends its block (#1518) (`82924fb98`).
- Record the owned sort call and comparator in WAT (`b6545bedd`).
- check-change: test code is a Rust file's top-level #[cfg(test)] item on (`3540ec47d`).
- Lower the sloppy-code ceilings to the merged counts (#1535) (`a69fe0015`).
- Lower the sloppy-code ceilings to the merged counts (#1534) (`1207519ab`).
- A static bool method's re-entrant call reads true: its cycle is its greatest fixpoint (#1445) (`176420e11`).
- Fold a derived flag that runs a describer: dead ? arms, void payloads, quiet copies (#1445) (`ba41429d6`).
- The inliner's cycles are instances, not functions (#1445) (`6b578137a`).
- std.json picks its fast path in hd: Serialize.regular() (#1445 step 2) (`849ffeba5`).
- Lower the sloppy-code ceilings to the merged counts (#1445) (`76b708167`).
- Derive map cursor layouts from the fixed intrinsic shape (`920cfc0f3`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`237767b55`).
- Use the collector's proof for promoted argument references (`ffa8f9a4b`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`6fe75ae2d`).
- Index std.serde's regular-walk example in the examples inventory (#1445) (`d0f5867f1`).
- Cleanup: the intrinsics audit no longer lists the deleted JSON intrinsics (`a5e3c5e24`).
- Record the reachable tuple-rest dbg body gap (`486da9246`).
- Use recorded cleanup suites when registering defer flags (`29aea9129`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`4e89316e0`).
- Move the lint walk and its events into hd_lint; checked views answer only checker facts (#1517) (`0b7f606a4`).
- Group parameter writes by statement in hd_lint, not the checker (#1517) (`6c593896c`).
- Gather the driver's analysis hooks behind one module; 2 feature gates remain (#1517) (`ff6da4568`).
- Lower the sloppy-code ceilings to the merged counts (#1517) (`1c570de63`).
- Document the helper-only drive slot invariant (`0000ab3d0`).
- Audit row context invariants after monomorphization (`4438c1ad0`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`4916879f9`).
- Audit map runtime and probe ownership invariants (`7d59a1527`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`a2696adf4`).
- Trust checked function-value argument layouts (`3b6461b29`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`d2de5580a`).
- Fold ProgramItems::defs into its one fixture caller (#1517) (`8c38b8559`).
- Solve flag-method cycles at compile time in release (#1542) (`e8b4af178`).
- Solve flag cycles without a lock or a thread check (#1542) (`12e32e25f`).
- Separate optional variant metadata queries (`bc5e7f226`).
- Use the ownership producer proof for whole-value drops (`a3000d2d7`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`08b75ee16`).
- Use checked list runtime and element-argument invariants (`e5f838c28`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`c09e6fe57`).
- Build a module of many items in time linear in its items (#1536) (`6eccf317f`).
- Record associated enum payload reference gaps (`158207901`).
- Use reference ownership and payload shape invariants (`1e31dac1c`).
- Complete the reference place invariant audit (`2c8ed84da`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`caeadcb80`).
- Use the established suspension cancellation target (`38005c0a7`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`992f7a52e`).
- Use seeded Structure signatures and metadata invariants (`e160f78aa`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`6440ada02`).
- Assert canonical whole-local decoding in debug builds (`69a153d89`).
- Lower the sloppy-code ceilings to the merged counts (#1543) (`4f5ea21eb`).
- Ask Dark Reader to leave the website alone (`51b20f8a3`).
- Use checked enum payloads without a backend refusal (`a8c23a812`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`fbdedfd58`).
- Use verified result and frame count contracts (`03ddf543f`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`212df1462`).
- Lower map probe uniqueness under its verified count contract (`fe8bbbc46`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`c9d314ff0`).
- Let stage cases select a runtime profile and print host codecs (#1490) (`cb9f399ef`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`94e94989f`).
- Use verified context allocator and provider-copy contracts (`40279b53b`).
- Read checked List element types without a backend refusal (`0d05a6b81`).
- Use verified reference ownership and storage contracts (`b4db0232d`).
- Read checked reference targets without backend refusals (`75f94857b`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`5d32eb78e`).
- Keep reference count invariants checked in every compiler build (`140e0e674`).
- Read the stated map uniqueness reason with an always-on guard (`8d03e82d2`).
- Use the stated provider-copy reasons in both context paths (`55a8d0ad7`).
- Consume suspension count contracts without backend refusals (`564fc9603`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`6bbeef7f1`).
- Use suspension state contracts without backend refusals (`dc1a907bc`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`423d0ada0`).
- Read checked Structure types without backend refusals (`87634f55b`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`82bdc3975`).
- Lower the sloppy-code ceilings to the merged counts (#1550) (`4d399fdcc`).
- Refresh the server SDK's portable runtime snapshot after #1550 (`da99b7f77`).
- Read required Structure metadata without backend refusals (`affac4c9d`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`915286a88`).
- Consume checked reference projections without backend refusals (`949b0a371`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`7d2a1d2f2`).
- Separate value locations from store-only Wasm globals (`20412f105`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`219b361ad`).
- Refresh the server SDK portable runtime snapshot for the boundary map codec (#1549) (`e7f44a847`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`bcdecbdde`).
- A host method may return a std capability or bridged trait value (#1552) (`ac6cd0000`).
- Web compiler gzip cap: 2.5 MB, and it fails the build again (#1558) (`4649be3ba`).
- Consume checked payload memory and field paths (`7938908ab`).
- Require value-backed roots for reference walks (`ea35accac`).
- Read each context key's checked source entry (`1f54ab7a1`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`f7df828a0`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`61c59e70b`).
- Read a boundary list's one element type directly (#1490) (`9270e93b7`).
- Split a script's inits around its root without a refused entry (#1490) (`6df9c80cb`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`6515ee270`).
- Lower the sloppy-code ceilings to the merged counts (#1561) (`4e78d5ee8`).
- Lower the sloppy-code ceilings to the merged counts (#1563) (`951258dd1`).
- ir-audit: load-dropped and pure-dropped are size-only leads, by measurement (`ef7c4eb9e`).
- ir-audit: repeat-bounds is information, it predicts no V8 time (`17205c216`).
- Reach or remove the uncovered lines today's commits added (#1490) (`6882d7431`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`da8fdb5b2`).
- Lower the sloppy-code ceilings to the merged counts (#1560) (`20b297fac`).
- Spec: a boundary cycle on hd's side is a boundary-cycle panic, and hd tracks counted references (`85ebfbe11`).
- DESIGN §23.4: a static object has count 0, not 2^31 (`0ff018fcb`).
- Price a handle's shared supertrait in a .wat baseline (`bd4070161`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`1b89e9155`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`3d9a8f1ca`).
- Lower the sloppy-code ceilings to the merged counts (#1490) (`e1a38906a`).
- Cover hd_check's diagnostic and place paths with stage cases (#1577) (`cb21c79ce`).
- Lower the sloppy-code ceilings to the merged counts (#1585) (`e808e9264`).
- Move the checked fact views from hd_check into hd_lint (#1577) (`1a0e379fc`).
- Cover the value-less tail and unsolved composite zonk paths with fixtures (#1442) (`f6807318d`).
- Fill defaulted trait arguments through the body's pool (#1442) (`924ccf415`).
- Cover REPL-session derivation layers with stage cases; delete a dead fact-site branch (#1442) (`38d1a091e`).
- Cover opaque trait-value arguments, vararg spreads and REPL redeclarations (#1442) (`bda94d249`).
- Delete hd_syntax's unused typed-node accessors (#1442) (`d402b6302`).
- Cover const-argument operand types, reference impl overlap and REPL session paths (#1442) (`3c18adeda`).
- Spec: counted references cross host boundaries, and a Promise block_on needs JSPI (`266e51b94`).
- Cleanup: REFCOUNT_RUNTIME's static objects have count 0, not a count from 2^31 (`f1cb8c1c5`).
- Resolve derivation-drift names in hd_lint, not in lowering (#1578) (`81b890777`).
- Delete hd_types entry points no caller reaches (#1579) (`6bb7455b0`).
- Rebuild types through one shared structural map (#1577) (`f7afce3c3`).
- Reach hd_types' rest-tuple, union and Self-in-signature rows (#1579) (`727165975`).
- Normalize concrete projections through the shared map (#1579) (`bf3d11370`).
- List every type form in the bare-dyn referent walk (#1577) (`8def8231a`).
- Delete two hd_resolve helpers no caller reaches (#1578) (`f6d4b1f47`).
- Walk a type's children through one shared visitor (#1577) (`011639113`).
- Drop the opaque arm of the outlives position walk (#1577) (`9c85ac04c`).
- Lower the sloppy-code ceilings to the merged counts (#1442) (`0494c0a7c`).
- Build a function type's inputs tuple in one place (#1579) (`faff4ae38`).
- Run opaque results through each conversion arm (#1577) (`6dd9a395e`).
- Find unused derivation facts in hd_lint, not in lowering (#1578) (`92fb35971`).
- Report type arguments on a name that declares no generic parameters (#1578) (`107c898a8`).
- Skip a function body whose name a later type declares again (#1578) (`873d1b478`).
- Reach hd_resolve's kind, decorator and const evaluation errors (#1578) (`18589c98e`).
- Keep only the typed nodes a caller names (#1580) (`fafad0468`).
- Reach hd_syntax's parse-error paths with parse/invalid fixtures (#1580) (`809b8c452`).
- Give utf8-bom.hd its byte-order mark back (#1580) (`6b6a5385f`).
- Delete hd_syntax's unreachable list guards and escape fallbacks (#1580) (`bfb3901be`).
- Drop the green tree's unread layout and error columns (#1580) (`d2a949ae7`).
- Describe the typed syntax views that exist (#1580) (`f4dfbbcaa`).
- Cover the tests:-block errors that only --tests reports (#1578) (`f807dbd57`).
- Drop the derived-implementation anchor's unreachable skips (#1578) (`4260650af`).
- Count the CLI conformance cases in core coverage (#1442) (`45fe48fd4`).
- Reach hd_check call.rs's error paths with typing fixtures (#1577) (`45b1778fb`).
- Drop the anchor pass's skip for a head off the root (#1578) (`882333d5c`).
- Record the std.task import of the qualified-value fixture as a tier crossing (#1577) (`f91d29d82`).
- Reach hd_syntax's REPL names and skim's end-of-file header (#1580) (`0bd8fcb43`).
- corpus: check-header-colon-ends-the-file is a deliberate parse failure (`6f5f253f2`).
- Drop the anchor pass's skip for a nameless member function (#1578) (`432736964`).
- Record core coverage at c19d97f74 as the baseline (#1442) (`042842557`).
- Cover expr.rs's builder push, operator, identity and range-count paths (#1577) (`1476a6df6`).
- Cover body.rs's assignment, return and statement-call paths (#1577) (`e7e8314f7`).
- Cover a const char argument's escapes; drop an unused trait default (#1579) (`e1f50daa5`).
- Cover the solver's rest-tuple head match and the UnionType row (#1579) (`9f8932e30`).
- Delete the solver's unasked `Project` goal and its `Normalized` answer (#1579) (`ba271c18e`).
- Cover a `Self` data literal and a diverging copy-update source (#1577) (`77ce4cfa4`).
- Cover the solver's out-of-fuel exits with `@fuel` stage cases (#1579) (`eba5bb136`).
- Cover a negative `Copy` implementation outside its target's module (#1578) (`8a6be4143`).
- Delete the `append` to `push` hint and fix-it (#1577) (`c62ef998f`).
- Delete the field store on a value that is neither a struct nor a tuple (#1577) (`49b36db70`).
- Cover a call result that projects from a type fixed later (#1577) (`a51728a17`).
- Cover the cast hint's bracketed type and schematic fallbacks (#1580) (`f602f9b32`).
- Cover a signed float exponent in a pattern and a data pattern's missing comma (#1580) (`6d1ddb682`).
- Delete the struct parser's `data` name message (#1580) (`760233fbd`).
- Cover a reference to a promoted field and an `is` against an `AnyRef` parameter (#1577) (`9cf5cd4d8`).
- Cover a reference result a private function infers with no one source (#1577) (`8332778e3`).
- Cover `is` in a const argument (#1578) (`4a3b4e55b`).
- Cover `@auto_closing`'s target, spelling and duplicate errors (#1578) (`aa1341436`).
- Name the open issue on the local declaration gaps (#1632) (`9c0733d9a`).
- Say that a tuple with a never element keeps its own type (`e50b6882b`).
- Cover impl-head matching, clause choice and `?Copy` arguments (#1579) (`69fcae933`).
- Delete the associated-type default a trait cannot declare (#1577) (`4b2c52dd3`).
- Cover calls through a type and a parameter, and drop a dead branch (#1577) (`0ed36dde4`).
- Drop the `is_empty` companions no caller asked for (#1579) (`513de7283`).
- Split the trait-value qualified-call fixture, one error each (#1577) (`10f6710eb`).
- Lower the sloppy-code ceilings to the merged counts (#1442) (`8973a601d`).
- Lower the sloppy-code ceilings to the merged counts (#1641) (`7c0a9ed9c`).
- Record core coverage at 8973a601d as the baseline (#1442) (`ebb5d0564`).
- Lower declaration types through a closed type-form enum (#1578) (`eecf23b7b`).
- Read a declaration's name from its own name token (#1578) (`bc3e6ea72`).
- Delete `Impls::All`, the scan of every impl table (#1579) (`bcbeaf630`).
- Drop the canonical-variable row from cache-entry type tables (#1579) (`dcfcabe9d`).
- Drop the trait-value arm of the `'static` row (#1579) (`8a0eaed65`).
- Alias a least-common-type site by its variable (#1579) (`b80d6feae`).
- Match a goal's arguments against a head without an arity test (#1579) (`ea4719288`).
- Say what the candidate order keeps without the deleted table scan (#1579) (`d4571402b`).
- Cover the kind errors of types written in a body (#1577) (`7b7ec0caf`).
- Cover body function-type inputs, body lifetimes and shorthand auto-closing fields (#1577) (`931f8d671`).
- Cover const powers of 0, 1 and -1 and a literal past every integer (#1578) (`e67f82bad`).
- Cover row-argument impl overlap and an opaque result under a row parameter (#1578) (`acae53e4e`).
- Cleanup: data-structures.md lists the green tree columns that exist (`835fdc0b6`).
- Revert "Report a wrong-kind row alias argument once (#1578)" (`a9c8a3bf1`).
- Build one integration-test binary per crate for hd_lint, hd_fmt, hd_web and hd_resolve (`1ab3641a8`).
- lane gate: a crate selected only as a dependent runs its unit tests (`b692d4a18`).
- Rename hd_wasm_linear to hd_wasm (`845153956`).
- lane gate: type-check dependents in clippy; run hd_cli tests only for hd_cli changes (`98bec9d8e`).
- Cover a plain call where a Suspend is wanted and a wrapped reference-equality fix (#1577) (`5caa79b8a`).
- Cover an i16 const slot's bitwise not, unary plus and integer comparison (#1578) (`9d19724b7`).
- Build hd_driver with analysis and checked-report by default (`7486a2f93`).
- hd_check DESIGN.md: drop the debug type printer #1640 deleted (`a0b74ab6b`).
- Revert "Build hd_driver with analysis and checked-report by default" (`f96ff4586`).
- Let a local type take enclosing const and local-function parameters and an impl's Self (#1578) (`06624a89a`).
- Hand every body context its method index instead of building one on demand (#1577) (`42bcf07fb`).
- Cover a numeric alias conversion, a Union value path and for over float and negative literals (#1577) (`980dc27ed`).
- Cover the omit forms a member line cannot take for Self and for += (#1578) (`c5371f22a`).
- Cover a union that names Self in a trait signature and holds a reference (#1579) (`c25e6469a`).
- Cover an identifier cut by a non-identifier character and a mutable rest binding (#1580) (`cd3d2713b`).
- Delete the body type check for a type in a row slot, which is reported where it is written (#1577) (`155cfb2d5`).
- Cover a local function's default rules and signature checks, and an i16 range match (#1577) (`d68b2c3d9`).
- Cover a shared enum constructor's named, duplicate and spread argument errors (#1577) (`8740740f8`).
- Cover an adjacent tagging content name that is no string literal (#1577) (`14bb500d9`).
- Drop a citation of a rule the spec does not have from takes_arguments (#1577) (`d82dbfee2`).
- Remove stale reference-view text from the compiler's comments and docs (`e05ce7a32`).
- hd_cli tests: expect "takes 1 argument" after #1652 (`b43ae6052`).
- Let a local type take an enclosing row parameter (#1578) (`2737713db`).
- Cover a later binding read by an inferred body and a struct after dyn in a body (#1577) (`0ca237700`).
- Cover const argument evaluation of each operator kind and its operand type errors (#1578) (`2b1d260c8`).
- Cover the body type checks the checker owns, and word a struct literal's row-slot error as a written type's (#1577) (`f1d0f7b8e`).
- Cover a newtype call of the wrong arity, a struct called with an unknown argument and a race with an extra name (#1577) (`0f9f49360`).
- Record core coverage at d82dbfee2 as the baseline (#1442) (`9a5fb44a2`).
- Update defer stages for inherited row contexts (`372d41f14`).
- Cover mono scalar defaults and remove unused layout helpers (`be1ef1e9e`).
- WIP: publish physical reference ownership cleanup (`1c3bab9d4`).
- Let a local trait and a local type in a trait take the parameters around them (`47cbcd524`).
- Report no mismatch for a dyn widening across a cut supertrait edge (`5c879f715`).
- Point stale literal-class text at the body-level rule (`7483fbb4f`).
- Fold collection copies of proven static buffers (`5d0e94324`).
- Refresh ownership count budgets (`8bce4876f`).
- Revert the open literal class (#1528) and its doc follow-up (`61fafdc89`).
- lane gate: run hd_lint's tests when it is selected as a dependent (`cecb4f30c`).
- ir-audit: label empty-block as size only, with the measured saving (`98c29c301`).
- Refresh ownership count budgets (`860c82488`).
- lane gate: format the hd_lint step (cecb4f30c failed the format check) (`e1324467e`).
- ir-audit: repeat-load is information, it predicts no V8 time (`7da695bd6`).
- Restore the pick_len borrowing expectation (`3707b3ef6`).
- Index pointer-loan access ranges (`0c7a40907`).
- Transfer stable by-value arguments into cold calls (`1f159e8c5`).
- hd_types: carry the cases in the types instead of expect and catch-alls (`bfaad8bf4`).
- hd_syntax: name the parser's token check, and keep the root block apart (`d264b8c0e`).
- hd_resolve: list the skipped cases of the type walks and binding kinds (`77a463047`).
- hd_check: name the type check, and carry the opaque bound with its result (`d437db0bc`).
- Refresh ownership count budgets (`896c5a681`).
- Revert the #1528 redo: it fails optimized conformance and the analysis view (`04a15e38e`).
- Transfer direct provider operands into reusable contexts (`07c725082`).
- Refresh ownership count budgets (`5d1d25400`).
- Check a union's members for 'static and for a held local reference (`12f179cd7`).
- Count a type parameter in a row argument's key as invariant (`25f9116fa`).
- Check the bounds of a union member written in a signature (`5f7ec5fe6`).
- Find a struct that holds itself through a union member (`4a477abd4`).
- hd_resolve: list the skipped cases of the type walks, and handle three expects (`29b1d6728`).
- hd_check: check an opaque type's and a projection's parts in a signature, and list the skipped cases (`3374ec3d7`).
- Bind the union fixture's local as an i32 (`6600320dc`).
- Move kept fields from last-use struct update sources (`77a08fbad`).
- hd_check: check an associated type's bound arguments, give a generic trait's Self projections their bounds, and list the skipped cases (`7609284ca`).
- Read a tuple element through a reference as a place of the referent (`f93f69a69`).
- hd_tir: drop data-object wording from Proj::Deref and read steps (`317402690`).
- Lower the sloppy-code ceilings to the merged counts (#lane-tickets-6) (`57ab0f3f0`).
- Track concrete hash inlining with private writer and reader proofs (`f9d5e404b`).
- Test integer hashes against independent fixed digests and bytes (`e8b8b509b`).
- Start the Node test thread and worker process before a case needs them (`b4618c9d5`).
- Transfer cold argument owners through methods, host slots and closures (`bf179d656`).
- Share a conformance case's decoded folder interfaces across its builds (`2761491dd`).
- Let one conformance thread build a missing std entry while the others wait (`206aa7f7e`).
- Refresh ownership count budgets (`6e950e4f3`).
- Borrow read-only comparison operands and classify private storage writes (`a7c83fbfd`).
- hd_tir: cite pinned_elements, which replaced pins_once in 3edb8a405 (`e1a915862`).
- Lower the sloppy-code ceilings to the merged counts (#lane-tickets-10) (`f8255f04d`).
- Avoid buffer splits for call-scoped read-only element references (`c0b9f88e6`).
- hd_types: cover the solver lines the coverage check flagged (`7048c75e6`).
- hd_wasm: cover or remove the lines the coverage check flagged (`26a413a55`).
- Inline visible local closures while retaining capture semantics (`ef1f6285a`).
- Inline bounded consumers of known closure literals (`8f5c40feb`).
- Cleanup: future-work README no longer lists closed issues as open (`4c9cd0572`).
- Build two count stage cases unchecked, so their WAT matches every feature set (`c09e82610`).
- Take a stored package result on a warm no-change check (#1510) (`177d3c21e`).
- Stop dumping WAT for the tuple-input variance case: it differs under checked-report (`fd9a5625d`).
- hd_wasm: prove index guards false in optimized builds only (`ba0df983d`).
- Refresh ownership count budgets (`e88515a23`).
- Lower the sloppy-code ceilings to the merged counts (#22) (`ff124e6ae`).
- lane-gate: run hd_check's stage tests when any compiler crate is selected (`d4fac6358`).
- hd_run_wasmtime: let the test pool hold a 2^17-function table (`257df9d6d`).
- Replay a warm check's diagnostics from the stored package result (#1510) (`f188e912b`).
- Replay a warm hd check or hd lint report from one stored entry (#1510) (`92f46b81a`).
- Write a warm hd build's stored artifact (#1510) (`5456dc0c9`).
- Run a warm hd run's stored module, building positions only on a panic (`9939439b2`).
- Replay a warm hd test's build half from one stored entry (`b1b3f35ea`).
- ir-audit: count a body's result as a use; verifier skips loop continues; relabel store-then-read (`cbc587a73`).
- hd_tir wire: round-trip owned bodies in an owned section (`106a8fa01`).
- hd_tir wire: write only the extra records a body reaches (`87a055f6a`).
- Cache std's owned bodies across builds in the store (`ec133afc7`).
- hd_driver own: say which steps read other bodies in the module header (`6b6b1fe25`).
- hd_wasm opt: skip settled local rules and settle liveness from a worklist (`d17109886`).
- Cleanup: mark closed ownership issues done; drop a code-cache comment that names no cache (`c1b01ce27`).
- Keep hash bits in the map index so probes skip other keys' entries (`71aa383a6`).
- website: keep tour prose inside its column on a phone (`dd88b0f20`).
- Count the work of two complexity tests instead of reading the clock (`d6a0e04e3`).
- Count the map work of three collision tests instead of reading the clock (`ecb7c28ce`).
- Skip a runtime init whose call does nothing (`7eb10ab0a`).
- Cleanup: refresh collection status and package-build guidance (`dc54956bf`).
- std: sort through a statically dispatched comparator, and partition in one pass (`2e31df8b0`).
- Refresh ownership count budgets (`68ad7f73a`).
- Add @track_caller and std.debug call-site locations (`bbd3016c4`).
- Keep argument texts through a @track_caller chain across modules (`9cc75fa24`).
- Drop the compile-time literal check of a snapshot's expect (`c9b4b113b`).
- Point std comments at the files that implement them (`b13330c30`).
- Cover the @track_caller chain pass, and drop its unreachable branches (`f65341d90`).
- Cleanup: the intrinsics audit no longer waits on #1205 (`29318e01a`).
- Leave @track_caller functions out of panic frames; assert_equal is ordinary hd (`896de8d99`).
- Cover named arguments in caller_args(); snapshot takes expect's span from it (`82c59149f`).
- Align runtime fixtures with implicit borrow exclusivity (`26b05bb27`).
- Extend std Drop confinement to live references (`e777ff8be`).
- Release a @track_caller function's arguments: count its hidden record as a parameter (`f77240dfb`).
- Lower the sloppy-code ceilings to the merged counts (#42) (`8248be556`).
- Audit: #1219 no longer waits on #1364; name the missing uniqueness test (`e1e84fe34`).
- std.rt.memory.unique_mut: the copy-on-write test of a box (`e1689a211`).
- Refresh ownership count budgets (`7919f3d00`).
- Refresh ownership count budgets (`a25d1ce6e`).
- Cover std.rt.memory.unique_mut, and drop its unreachable wide result (`86f864a6c`).
- A box and a list may hold local references, as any generic type does (`568593245`).
- server-sdk: bound the shared state by 'static (`ced59c875`).
- Spec: a local box of an opaque result is no longer an escape (`0b5d05651`).
- server-sdk: record server.hd's new source hash in the schema asset (`b3948e30d`).
- Keep a length saved before a write when a later loop reads it (`17d3dc66e`).
- Load a box-held list's header once before a loop that keeps its holder (`d5698bea6`).
- std.rt.memory.make_unique: test and copy a box once before a write loop (`e6858eaa3`).
- Cover make_unique's loop hoist: the loops it hoists out of and those it leaves (`30f04cbc3`).
- Run a hoisted make_unique only when its loop runs an iteration (`94bd8e55c`).
- Record the #1219 list-block decisions and stress-test them (`4de2cdc59`).
- Record the conservative source-call sites of #1376's new study program (`6c4e3a99e`).
- Hoist make_unique only past statements no exit of the iteration can skip (`be50a721c`).
- Drop weak references as a liveness oracle from tests that are not about them (`83a278a91`).
- Retire weak references from the spec, std and their tests (`0bef99277`).
- Lower the sloppy-code ceilings to the merged counts (#51) (`d4bda464c`).
- Drop static initialization owned only by unreachable functions (`f7ef3a889`).
- Cleanup: List and Map stay built-in in the intrinsics audit and raw-buffer docs (`35d31352d`).
- Drop the removed-forms rows for weak references and the box primitives (`dbffc4daf`).
- Revert "Keep list uniqueness checks behind iteration exits" (`3e307f3fd`).
- Cleanup: AGENTS.md stops citing the closed #1206 for intrinsics (`a5ec9e65b`).
- Audit: compiler, language and std review of 2026-11-12 (`a45fdfc17`).
- Forward borrowed results through inline value wrappers (`70b014d4d`).
- Docs: the back half has no Emit task or code entry; mark code packs planned (`812ce20c2`).
- Docs: tiering.md states the dev pipeline's TIR rewrites, link cleanup and Winch (`649990dc4`).
- Docs: hd_wasm §23 and opt.rs and count.rs docs match the code (`17ad1c886`).
- Docs: tir-v2.md and the ownership records state the built pipeline (`35e6a15f9`).
- Docs: goals.md and shared-rules agree on using; README's edit latency is the daily figure (`f3e4f4350`).
- Docs: string appends and borrowing loops cost what the code does now (`ea464fb34`).
- Spec: std README and iter.md match lib/std; both intrinsic inventories list every key (`e33aa3f75`).
- Docs: the intrinsics audit counts and lists what the tree has (`fb5d5450f`).
- Audit: recount the design docs' notes after the doc round (`c355ea5ea`).
- Docs: §23.30 cites §23.16's renamed uniqueness bullet (`101151bf0`).
- Refresh ownership count budgets (`e077d6096`).
- Forward read-only parameter loans through conditional selections (`b4d9e4dca`).
- Ownership: forward pass-through returns without a counted round trip (`1b435f48b`).
- Revert "Ownership: forward pass-through returns without a counted round trip" (`c5c97e6b2`).
- Checker docs: a trait-own projection and a slotless type argument as the code treats them (`0373ccbb9`).
- Ownership: forward pass-through results and count only owned adapter components (`4917527e2`).
- Ownership: lend unchanged inputs while their needed copies own the counts (`821e3239c`).
- Linker: carry resolved imports and functions through encoding (`6a3341bb9`).
- Docs: retire collection pin and owner-word descriptions (`4d9b9d3e6`).
- Lower the sloppy-code ceilings to the merged counts (#63) (`7311ca84e`).
- Audit: drop review item F7, fixed by e80190e0c (`4029abaa2`).
- Std: Span.index_of_span scans for the needle's first element before the Morris-Pratt step (`1c2d5b5a7`).
- Audit: metrics, layout and Wasm, and optimization audits of 2026-11-13 (`c460d8351`).
- Docs: the lifetime solver moves to hd_check's design, the returned-references record keeps only what is not built (`0930073e0`).
- Docs: ownership, pointer loans and raw buffers merge into docs/compiler/ownership.md; the refcount study goes (`84ba7fa16`).
- Docs: the built enum layout and release summaries move to hd_wasm's and hd_opt's designs (`3e5de6100`).
- Docs: live execution and package host capabilities are built, so they move to docs/compiler (`e7c495e50`).
- Docs: delete the GC-era performance roadmap and the TIR v2 alternatives record (`125831094`).
- Docs: delete the closed float power report and point the docs index at where built designs live (`aaa48c891`).
- Docs: fold the parallelism study into PR #1139; link RELEASE.md to #1820 (`db37d4743`).
- Docs: SERVER_APPLICATIONS.md points to its parked remainder, #1821 (`87456c657`).
- Docs: NATIVE_PACKAGE_HOST_BINDINGS.md is tracked by #1822 (`347cd8d5a`).
- Parser: a chain past 10,000 links is nesting-too-deep, and hd runs below it on 32 MiB stacks (`a16cf7b7c`).
- Checker: usefulness and the Copy blocker walk a work stack, not a depth cap (`79a76f206`).
- Types: the Copy row walks a work stack, not a 32-deep cap (`a8ea304cd`).
- Remove the server SDK test caches committed by a8ea304cd, and ignore them (`43a5d15f1`).
- Checker: type walks stop at a type depth of 1,000, past which a type is type-too-large (`44d28fa41`).
- Checker: value and supertrait walks use work stacks, not depth caps (`0737b6568`).
- Solver: concrete normalization reports a chain past PROJECTION_DEPTH (`fcdf2f6ad`).
- Limits: file-too-large and match-too-complex have emitters (`271f1fd98`).
- Lifetimes: a function type nested in another type records what its inputs write (`5034c0d8e`).
- Lower the sloppy-code ceilings to the merged counts (#1764) (`ae977c179`).
- Coverage: tests reach lanes 73 and 74's projection-depth and match-fuel paths (`fe34a7820`).
- Lifetimes: a struct argument's function field ties the call's lifetime to the caller's (`41a0b4ff1`).
- IR audit: set-then-get is bytes only, about one byte per site (`cae41c234`).
- Refresh ownership count budgets (`abbffb81b`).
- Coverage: tests reach lane 76's depth-error paths; a written capture slot's store is gone (`9ab6a467f`).
- Spec: examples with a syntax error declare what their other items use (`e5822be34`).
- Cover inliner cache publication and list capacity boundaries (`a0732427a`).
- Cleanup: remove the audit items fixed in the last 12 hours (`1881d7133`).
- Revert "Use Node 26 with corrected Wasm wrapper lifetimes" (`31574e859`).
- Checker: cover or delete the three lines 423dd883b left uncovered (`f375861b7`).
- Checker: a field of a function value's result passes its argument at the lifetime that result writes (`364d51ad3`).
- Cleanup skill: issue states are labels, and blockers are linked as GitHub dependencies (`c600dca1a`).
- Cleanup: retire closed records and finished build artifacts (`0d606d191`).
- Conformance: the nameless-item recovery fixtures are parse-phase cases (`d2d254df8`).
- Diagnostics: a missing import's `use` takes the file's line ending (`07179743b`).
- Docs: data-structures.md describes Zig's InternPool correctly (`f0ef7f4d3`).
- Cache: the keys of cached diagnostics cover the file's line endings (`72f832f78`).
- CI: scheduled coverage runs no longer share a concurrency group with pushes (`cd69d8a4b`).
- Metrics: the scale check's known superlinear lines name their issues (`c48f7b963`).
- CI: the planted scale run records its expected exit 1 instead of stopping the step (`0196ecaf0`).
- Issues: a labelling guide where priority is the place in the work queue (`de4cba71b`).
- Types: resolving, the occurs check and the solver's folds walk each distinct node of a type once (`e60c1a7dc`).
- Cleanup: delete scratch hd caches and report the shared one's size (`40153d2c1`).
- Metrics: every scale run asserts its own scratch HD_CACHE, and the warm cache is removed after (`c24bb1ce3`).
- Metrics: measuredEnv returns a string record; paren-nest's check line is #1919's (`44b6c182f`).
- Docs: add the owner's 2026-11-14 conformance fixture review and compile time and layout audit (`9ba8fa972`).
- Cache: no command walks the pack directory; collection runs detached, once per interval (`6cea3e9c1`).
- Audit: record what the conformance fixture review's findings were verified against (`d6219fce6`).
- check-change: a new test in an existing file counts as a fix's test (`09e6def34`).
- Cache: publishers share no lock; the index is 16 shards with append logs (`da06593d7`).
- Cache: a reader finds a key a fold moved, and a run reads no total it just wrote (`8d6f1001b`).
- CLI: the wasmtime code cache opens the bare disk store (`00d7e91ef`).
- Reject runtime conformance cases without an executable entry (#1941) (`7a694626e`).
- Fix the counted-copy origin from 6246979e1 (`d85b39452`).
- Restore conformance source forms and protect suite formatting (#1942) (`7ca4ae3bf`).
- AGENTS.md: every command an agent runs has a timeout (`0486cdeb3`).
- Cache: a run waits only for the entries it misses that another run is computing (`09bc7dac9`).
- Cache: publish a claimed entry early only when a run waits for one (`a3216774e`).
- Driver: a run that misses the std skims beside others records them at once (`04ecf4581`).
- Cache: a claim opens its file first and makes the claims directory only when missing (`d8aee10ed`).
- Conformance lang/02: name the chain cases for the link they test (`01580969f`).
- Conformance lang/13: name the caret cases for where the caret stands (`2cc83973e`).
- Conformance std: name the std-cited cases for what they show (`77a0e1578`).
- Conformance lang/03: name the scope cases for what each one varies (`24a2b7d4a`).
- Conformance lang/05: name the expression cases for what each one varies (`77dc37cdb`).
- Conformance lang/06: name the match and tail cases for what each one varies (`fcaf3aae4`).
- Conformance lang/07: name the function cases for what each one varies (`c5f75fe58`).
- Conformance lang/08: name the data and enum cases for what each one varies (`90db69fa2`).
- Conformance lang/14: name the annotation cases and their package trees (`ca3d461cb`).
- Conformance lang/10: name the module cases and their package trees (`25b5adab3`).
- Conformance lang/11: name the requirement cases and their package trees (`7ca425cfc`).
- Conformance lang/09: name the trait cases and their package trees (`d15b9b853`).
- Conformance lang/04: name the type-system cases for what each one varies (`bc90b19ea`).
- Conformance lang/02: name the where-clause case for what it rejects (`12befddb6`).
- Conformance lang/05: name the word-operator cases for what they reject (`62436a40b`).
- Conformance lang/06: name the import-scrutinee package tree for its case (`158ab11ed`).
- Conformance std: name the block-on cases, their trees and two reference cases (`c466cd437`).
- Conformance lang/03: name the bundled accept cases for what they accept (`bb679c664`).
- Conformance lang/04: name the bundled accept cases for what they accept (`1ffc111d0`).
- Conformance lang/05: name the bundled accept cases for what they accept (`6b3293e55`).
- Conformance lang/07: name the bundled accept cases for what they accept (`d2cb97a68`).
- Conformance lang/08: name the bundled accept cases for what they accept (`23276c590`).
- Conformance lang/09: name the bundled accept cases for what they accept (`c2026bc08`).
- Conformance lang/11: name the bundled accept cases for what they accept (`9a06527b1`).
- Conformance lang/13: name the bundled accept cases for what they accept (`b1d0f9878`).
- Conformance lang/14: name the bundled accept cases for what they accept (`d4157c138`).
- Conformance: no harness or script reads phase from a fixture's folder (`0ce742a6b`).
- Conformance lang/01: move the lexical cases into topic folders (`a59028185`).
- Conformance lang/02: move the Grammar cases into topic folders (`73c2eb0c4`).
- Conformance lang/03: move the Names and Scopes cases into topic folders (`0177579d7`).
- Conformance lang/14: move the Annotations cases into topic folders (`07118df5c`).
- Conformance lang/11: move the Requirements and Suspension cases into topic folders (`f452b268c`).
- Conformance std/cli: move the std/cli cases into topic folders (`4d92a0815`).
- Conformance std/cmp: move the std/cmp cases into topic folders (`31b49174d`).
- Conformance std/collections: move the std/collections cases into topic folders (`af87865cb`).
- Conformance std/console: move the std/console cases into topic folders (`19b4149e1`).
- Conformance std/contract: move the std/contract cases into topic folders (`34ff2aa26`).
- Conformance std/digest: move the std/digest cases into topic folders (`176fe2ad8`).
- Conformance std/encoding: move the std/encoding cases into topic folders (`af1bf46f7`).
- Conformance std/error: move the std/error cases into topic folders (`f092ee13c`).
- Conformance std/format: move the std/format cases into topic folders (`d810b22a8`).
- Conformance std/fs: move the std/fs cases into topic folders (`c1d4f1a4f`).
- Conformance std/hash: move the std/hash cases into topic folders (`12bd9c11c`).
- Conformance std/host: move the std/host cases into topic folders (`6d54e31b1`).
- Conformance std/http: move the std/http cases into topic folders (`838acd94c`).
- Conformance std/iter: move the std/iter cases into topic folders (`015459a49`).
- Conformance std/json: move the std/json cases into topic folders (`8cfda4157`).
- Conformance std/net: move the std/net cases into topic folders (`cd1c019e4`).
- Conformance std/num: move the std/num cases into topic folders (`f0f4a8964`).
- Conformance std/ops: move the std/ops cases into topic folders (`32e544355`).
- Conformance std/option: move the std/option cases into topic folders (`78755c772`).
- Conformance std/path: move the std/path cases into topic folders (`c9dbf4dd0`).
- Conformance std/process: move the std/process cases into topic folders (`8420353e1`).
- Conformance std/random: move the std/random cases into topic folders (`a92bde88b`).
- Conformance std/regex: move the std/regex cases into topic folders (`13fb1b36d`).
- Conformance std/result: move the std/result cases into topic folders (`efbf49822`).
- Conformance std/serde: move the std/serde cases into topic folders (`a6492850a`).
- Conformance std/server-sdk: move the std/server-sdk cases into topic folders (`77272653d`).
- Conformance std/task: move the std/task cases into topic folders (`e7495db76`).
- Conformance std/testing: move the std/testing cases into topic folders (`d534d51ee`).
- Conformance std/text: move the std/text cases into topic folders (`a10a54414`).
- Conformance std/time: move the std/time cases into topic folders (`5e16f8361`).
- Conformance std/validate: move the std/validate cases into topic folders (`5284f3bfb`).
- Conformance README: move the Conformance Language cases into topic folders (`803481010`).
- Conformance lang/08: move the Data and Enums cases into topic folders (`c412d6b51`).
- Conformance lang/06: move the Control Flow cases into topic folders (`e403324c6`).
- Conformance lang/07: move the Functions cases into topic folders (`9ab3b50bd`).
- Conformance lang/10: move the Modules cases into topic folders (`0c5daafa8`).
- Conformance lang/05: move the Expressions cases into topic folders (`ac2e7a371`).
- Conformance lang/09: move the Traits cases into topic folders (`c9c621cb5`).
- Conformance lang/04: move the Type System cases into topic folders (`8da3a091a`).
- Conformance lang/13: move the Values and References cases into topic folders (`0240288b2`).
- Conformance lang/13: the generated pointer-loan matrix writes into its topic folder (`0b2b25294`).
- Conformance std/testing: the property regression files move with their cases (`7350c767d`).
- Conformance: format the pointer-loan generator (`6c1ef4adf`).
- Checker: a copy question memoizes every member type its walk clears (`89e9a8210`).
- Types: resolve keeps an open result with its few unbound variables (`33b8e01ed`).
- Lower the sloppy-code ceilings to the merged counts (#94) (`b15c352d8`).
- Web compiler check: read the hello fixture from its topic folder (`ceb488bca`).
- Stage tests: `diagnostics` and `stdout` views (`b088d3d1c`).
- AGENTS.md: source-in, diagnostics-or-output-out checks are stage cases (`a36de56d5`).
- Conformance: the language topic folders sit under lang/ (`48d9ed6a1`).
- Conformance: each tier keeps its own package trees and packages (`e5642651f`).
- Conformance: format the scripts the three-tier move touched (`38412d064`).
- Conformance: remove the duplicate fixtures and trees that A1, A2 and A14 find (`5f49d0cdf`).
- Spec check: fail on duplicate fixtures and identical package trees (`2e2041d70`).
- Metrics: option-nest check is linear, so its KNOWN row goes (`9f8d79449`).
- Cleanup: old conformance paths in docs; lane branches and scratch removed (`63b022f45`).
- Stage tests: private-type-leak checks become diagnostics cases (`15e346070`).
- Stage tests: privacy and private-main checks become stage cases (`0438516a0`).
- Stage tests: promotion, prelude shadowing and decorator checks become cases (`af94a4233`).
- Stage tests: unmet-bound checks become diagnostics cases (`31e207204`).
- Stage tests: trait, impl and row checks become diagnostics cases (`e4102effa`).
- Stage tests: fact, warning and message checks become diagnostics cases (`99a48fd7a`).
- Stage tests: run-output checks become stdout cases (`73d0fdaaa`).
- Stage tests: checks that build a program become stdout cases (`92c7173d5`).
- Stage tests: generics, typed-fact, pipe and row checks become cases (`d22892c5e`).
- Stage tests: the program build of private_main's analysis test is a case (`abbddae3f`).
- Stage tests: the checker and entry-point source checks become cases (`9bae5ecb2`).
- Stage tests: pipe, closure, method-choice and generic checks become cases (`7e30c894d`).
- Stage tests: test-block, derivation and std-interface checks become cases (`8cdf2dfe3`).
- Stage tests: poisoned-expectation and generic-scope checks become cases (`35703cfc6`).
- Stage tests: module-name collision and tests-block checks become cases (`ecc885305`).
- Stage tests: map run output and provider-key checks become cases (`a34d295b8`).
- Stage tests: drop moved cases a conformance fixture already checks (`40befac45`).
- Lower the sloppy-code ceilings to the merged counts (#C1907) (`65f2ad608`).
- CI: select the platform jobs for disk-cache changes (`a8e27fbe3`).
- Stage tests: the `stdout` view runs every program on one Node host (`b2950cfc1`).
- Stage tests: a `tests` view, package directives, and test-run checks become cases (`7d8a18e45`).
- Stage tests: format the shared Node stage host (`3c2806bbd`).
- A listed key supplies its supertraits' keys, at their bound arguments (`dcd71969e`).
- Stage tests: fix-its in the diagnostics view, spans and fixed views (`2c5c5c2d3`).
- Stage tests: a @filter directive for the tests view (`c1d19bf22`).
- Stage tests: several builds of one case, and the four-build runs moved (`f5fd05438`).
- Stage tests: private main, iterator adapters and trial choice become cases (`927f0bd48`).
- Stage tests: a run's text leaves out the checked report's cycles line (`a2c373897`).
- check-change: conformance fixtures under lang/, std/ and cli/ count (`6b657dd04`).
- Syntax corpus: the cast-hint text case's sources are deliberately unparsable (`02e35b32f`).
- Conformance: two reject cases break only the rule they test (`10979a6f8`).
- Skills: review-change, the bar a change meets before it merges (`432ee40f4`).
- Stage tests: re-bless #2010's two loan baselines after #1998's anchors (`57a5652ca`).
- Lower the sloppy-code ceilings to the merged counts (#L109) (`e5c316652`).
- Spec: a type with a Drop implementation is not Copy in any instance (`e83d6832f`).
- Skills: a cost issue carries a runnable program at two sizes (`5f03d7039`).
- Emitter: an inlined frame's index is one probe, and each anchor's chain is walked once (`1594d6435`).
- AGENTS.md and review-change: read a production implementation before designing, and name it (`68826db71`).
- Emitter: liveness keeps hash-consed persistent sets, and local coalescing is a linear scan over ranges with holes paid for by accesses (`a7fb982e5`).
- Emitter: rule 11's zero facts are persistent sets met by intersection, not a dense row per block (`876cdf2fb`).
- Mono: argument promotion reads a body's census once, not the whole body per call site (`48da513e1`).
- Scale check: a counter may grow 5x for 4x the program (allocated bytes 10x) (`2fa7ef4db`).
- Emitter: rule 11's zero facts make the entry set in one pass, carry a block's writes in a dense row, and hash no set node (`a7665bfe3`).
- Resolver and checker: a chain of generic aliases is walked by its distinct nodes, not its paths (`df5d4da29`).
- Lower the sloppy-code ceilings to the merged counts (#L116) (`4248d225b`).
- Refresh ownership count budgets (`f4bfe9626`).
- Test host: a case's export view inherits the instance's exports instead of copying one entry per test case (`a099d816c`).
- Wasm optimizer: liveness summarises each block once, rule 13 carries its dead accesses across rounds, and rule 11 settles loops lowest block first (`f9069b697`).
- Skills: a cost issue's done check is counted work or a same-machine A/B, never a sample share (`9daad5f54`).
- Driver: a test program's root modules, init groups and runtime inits answer membership from a set, not a list scan (`648b7b808`).
- Run: `Instance::call_test` takes the case number that `hd.test` dispatches on, not an export number (`627e02fa9`).
- Checker: `facts_of(Box::[i32]::name)` resolves through its owner `Box`, not through a local named like the member (`2188b504c`).
- Checker: a field or path chain whose base is not a name, as `make().helper`, names no path (`c7fb33953`).
- Stage tests: the drop-hook WAT case builds std, not std checked (`c5609621a`).
- Cache tests: the leftover-claim test runs only where debug assertions do (`8bac6a3a7`).
- Cache tests: a test that spawns a child and a test that asserts on its dropped file locks never overlap (`e552017cd`).
- Checker: a local or a field read with a type-argument list is read as the value it is, not reported as an undefined path (`a21af0c85`).
- Lane gate: hd_driver's analysis tests run with their feature when they change (`1a31014a7`).
- Cache tests: three lock tests decide on a signal, not the clock, and four run apart from spawns (`1eda37e67`).
- Cache tests: the lost-wakeup test fails on an unpublished entry before any worker blocks, not on a 500 ms deadline (`d4946381d`).
- Driver and solver: a module's impl view holds its closure's folders, not a slot per folder of the program (`fc9a73c52`).
- Checker: "did you mean" names a near method of a parameter's bound or a trait value's trait (`f2fa82f44`).
- Lower the sloppy-code ceilings to the merged counts (#L127) (`51c991823`).
- Cleanup: delete the concluded intrinsics inventory (`c82c1292e`).
- check-change: the diffs start at the merge base, so a base that moved on blames none of its own edits on the branch (`972c4ecca`).
- Stage harness: a test result's stderr leaves out the checked report too (`76db48848`).
- Driver tests: the two init-chain counting tests use their own store (`8fa2ad9e2`).
- Checker: the head-table lookups take the key codes head_tables already made, so no table probe re-derives them (`51a4c750f`).
- Types: drop_above pops an edge's user without an Option check, since depend adds every edge to users with it (`93cf95154`).
- Types: the Copy walk marks an open node as Unknown, so neither a revisit nor a parameter needs an Open arm (`67adb30d5`).
- Solver: stage cases for goals that wait on a variable inside a union, a trait value, a function type, and an impl's Fn bound (`d61012608`).
- Solver: a goal whose argument holds a context with an undecided row key waits instead of failing (`7210c5329`).
- Types: unifying two provider contexts indexes each row's keys by trait once, so pairing them is linear in the keys (`ea04099c2`).
- Driver: the init order is one global order built once per collect, and each root's order is its restriction (`07744551c`).
- Driver: a joined REPL build deals its init order to its inputs in one pass (`15aa2c255`).
- Driver: a REPL build's failed inputs are a set, so each input asks once (`422b9ef4e`).
- Driver: a script entry's init comment names the global init order that replaced group_init_order (`f50809851`).
- Wasm: a struct over 16 values passes to its callee's worker as one address (`d6507e665`).
- Wasm: a result over 16 values is written into its caller's block, a literal headed there in place (`fd2040e2b`).
- Wasm: a closure's emitter scans only its own sub-body for kept results, and a body counts its value uses once (`e9b9be083`).
- Wasm: a local over 16 values stays in a frame block on the shadow stack, copied by one memory.copy (`0dd515d9f`).
- Wasm: a counted struct over 16 values passes to its owning callee by address, released through its type's witness (`9fc4d46ec`).
- Wasm: a counted result over 16 values is written into its caller's block (`c47dc2f6f`).
- Wasm: a move out of a parameter kept by address finds its flag by (local, path), and a drop groups moved paths by field once per level (`45269f3f7`).
- Wasm: a counted local over 16 values stays in a frame slot, released from it through its type's witness under a live flag (`a7cc2bb82`).
- Wasm: an emitter's exit release visits only its own sub-body's exit rows (`14bdf02c8`).
- Wasm: a counted local over 16 values passed whole to a worker stays in its frame slot, a copy taking its counts from the address (`398e2047f`).
- Metrics: the scale check measures struct-chain-values' build line (`9b24f6ba8`).
- Docs: Body::exit_drops is carried by the wire's owned section (`38dba56ae`).
- Wasm: a struct over 16 values with a Drop hook lives by address, its hooks run on a heap copy (`dae411d4c`).
- Conformance: 15 reject cases cite the rule that names their code (`0709b7c39`).
- Conformance: triage the review's B2 rows; eight more reject cases isolated (`cc7cdc15f`).
- Conformance: triage the review's B2 UNSURE rows; four more cases isolated (`7c5ff198a`).
- Docs: record a verdict for each of the review's 118 C1 rows (`f5b9d6eee`).
- Conformance: twenty-eight B1 cases test what their titles say (`b5e405f0b`).
- Conformance: forty-nine more B1 rows get a verdict; the literal first-use series decides with a non-default type (`2a9fb1b81`).
- Conformance: forty-one more B1 rows get a verdict; titles state what the code tests, and eleven cases test what their titles claim (`432dd7cdb`).
- Conformance: eighty-six heading-level citations cite the rule that names their code (`3ace27301`).
- Conformance: forty-seven B7 rows re-cite the rule their review names; 172 B7 rows get a verdict (`73b2d529a`).
- Conformance: twenty-two more B7 rows re-cite the rule their review names; 31 more get a verdict (`8b3467cd4`).
- Conformance: three accept cases cite the rule their review names (`78dcb2768`).
- Conformance: an unknown bound in a body projection cites the rule that names unknown-trait (`79dc508e3`).
- Conformance: two fixture headers cite only the rules their code exercises (`98cecb06a`).
- Conformance: an assignment to a List call cites expr.call.apply.place (`be969283c`).
- hd check --tests: a diagnostic both roles report is shown once, with the test role's fix-it (`4b2c3697b`).
- Conformance: a user Fn impl cites trait.sealed.user-impl, which names its code (`4c1b1f091`).
- Conformance: an overflowing integer power cites types.arith.checked (`ea8c5175f`).
- Stage cases: trailing blocks, test registrations and TypeId names move from Rust tests (`ae880eccb`).
- Stage cases: a functions view lists the linked program; constant defaults and json.encode linking move from Rust tests (`d5fafff25`).
- Stage cases: a dependency directive declares a fetched or path package; dbg in a fetched dependency moves from a Rust test (`ee63bde2e`).
- Stage cases: checker.rs's user-program tests move from Rust tests (`4b52c7252`).
- Stage cases: map-buffer copies and cycle-collector linking move from Rust tests (`f0e854be3`).
- Stage cases: a dependency's key may differ from its name; two same-named packages print alike, moved from a Rust test (`40671b1ca`).
- Stage harness: the diagnostics view prints the user's text, not internal package roots (`e768cb3e3`).
- Stage cases: tests-block shape and one-body row polymorphism move from Rust tests (`8b507a405`).
- Docs: the cycle-collector linking checks are stage cases now (`4e6afa42a`).
- Driver: a tests block in an integration test module names that kind of module (`82fbf9203`).
- Stage cases: unbound reference types and an orphan impl over std move from Rust tests (`d8595a9ce`).
- Cargo.lock: name hd_base's hashbrown dependency by version (`2b8fc41a8`).
- Spec: assignability, void-final, division, unknown-module and UniqMap bulk rules name their codes (`98e6547c5`).
- Error recovery cases move from conformance to hd_check stage cases (`59dc69277`).
- Resolve: an impl Structure line reports structure-outside-template beside its sealed error (`3867ae4a6`).
- Check: a missing argument of a callee with defaults names the parameter, not a count (`ce129fc1d`).
- Error recovery: the last two README-cited recovery cases move to hd_check stage cases (`540498604`).
- Loans: a receiver read from a written top-level binding makes it an object its call loan holds (`aee249eae`).
- CI: core coverage in its own workflow, every 3 hours (`1a711b27f`).
- Solver: a concrete projection's normal form is kept per part across a walk (`aedb4001e`).
- Check: the function-operand hint finds its implementation by its own lookup (`49f1e0abe`).
- Resolve: a shown type groups what its grammar needs, so `(fn() -> i32)?` no longer prints as `fn() -> i32?` (`b26681323`).
- Build: the shipped hd binary has a dist profile, stripped and ThinLTO (`7b7e1dea1`).
- Cache, opt: delete the dead cache kinds, the passes nothing runs, and the stale target name (`181494de7`).
- CLI: `hd help build` says FILE is a file of the package, and where --release writes it (`7e44ff6dc`).
- Spec: an interface carries every generic body a downstream instantiation needs (`aabd2d9ac`).
- Metrics: generated packages settle before warm runs; startup reports just-written and settled checks (`bbcd11216`).
- Tests: `branch-values` says a returned by-value parameter is owned (`5ace8ec5d`).
- Metrics: every hd the suite runs uses its own cache, never the user's shared one (`1275cc509`).
- hd_mono: the subst projection-depth gap names its own issue (#2075) (`edd3b09b2`).
- Spec: UniqList insert and extend, UniqMap get_or_insert, extend and index-set name borrow-conflict (`8e90b7227`).
- Diagnostics: a same-named package's key shows as hd.toml spells it (`3159b66ba`).
- Tests: every hd and Node the hd_cli tests start runs the pinned Node from PATH (`22a05f68e`).
- Tests: .node-version and package.json's engines.node must name one Node (`d3fbb518d`).
- hd_cli: its own unit tests start the pinned Node, not PATH's (`2074928f8`).
- Revert "Cache: a run that joins a lone run waits until the lone run claims what it computes" (`2bc9ee8a1`).
- Review overrides for #Orevert (`079a70d7d`).
- Conformance: 16 more timer fixtures move to pending-tick; the two that need waits of different lengths say so (`4b09a1165`).
- Tests: hd_wasm, hd_run_wasmtime and hd_host_abi start the pinned Node, not PATH's (`5cf7fd73c`).
- hd_run: resolve_path finds the existing prefix by binary search, not one OS call per missing name (`ca77fe740`).
- Server SDK: refresh the runtime snapshot for #1901's Node host (`ffed4de58`).
- Node host: a file past 2 GiB is rejected from its stat again, as wasmtime does (`43e1abf58`).
- Driver: the entry roots are a set, so asking whether a body is one is one probe (`2ebf4a5b1`).
- hd_mono: delete instruction_types, which nothing calls (`7aa5db188`).
- add writing log for ui framework experiment (`4210d76e7`).
- hd_check: name every std item through the known-items table (`d4e4c3f76`).
- hd_check: carry the scope and return stacks' base frame in their type, and drop 19 expects (`9d2b0f029`).
- hd_types: narrow table indices through one checked helper and decode stored words exactly (`dd1abeb4c`).
- hd_driver, hd_resolve: name std items through the known-items table, and narrow TIR positions through u32_of (`ddf79ebcc`).
- hd_mono: hash length prefixes through StableHasher::len, and narrow TIR positions through u32_of (`e3f5e1120`).
- hd_check: hand a dynamic method reference its selected trait value (`385bc461a`).
- std.json: a regular walk's encode keeps no keys (`7cebbf36f`).
- Refresh ownership count budgets (`ab19ec8be`).
- Checker: matches over Tag and TyData name the arms they ignore, and guarded Option matches become exhaustive (`7c5c2d062`).
- Spec: native package providers on Wasmtime, registered by an embedder, with the hd command still refusing (`2e958a719`).
- Package hosts: one identity display, so a local trait is `local:<hash>/trait` on both engines (`bf6de7f0a`).
- Server SDK: refresh the runtime snapshot for the shared package-trait name (`80bbf1ddd`).
- gitattributes: every CLI fixture source and expected file checks out with LF (`45dfd0114`).
- CLI conformance: a step naming --engine wasmtime needs the engine whatever its command (`9e0188b6d`).
- gitattributes: the native companion snapshots check out with LF (`08dfe24ad`).
- Package host contracts: one import index replaces the per-import scan of every interface and method (`5ed40fd26`).
- Lower exhaustive matches and preserve forwarded reference loans (`71eaec265`).
- Refresh ownership count budgets (`085df1ab9`).
- Capability grants: resolution groups each key's table and flag values in one pass instead of scanning both lists per key (`e13e3a5a7`).
- Node package providers: selection and load index the host packages, their entries and the contract's roots once instead of scanning them per trait (`025f4c769`).
- CLI tests: the native companion's end-to-end run consents to its trait (`d134eaff3`).
- Preserve borrowing across wrapper and argument forwarding (`f155744f5`).
- Native host bindings: a union case's positional payload is the Rust field `_0`, so the companion of a union-typed method compiles (`bb018bf6f`).
- Parity tests: the FIFO writer's wait for the sibling task starts after the program is built, so a slow build under load is not a failed order check (`072f89f5f`).
- gitattributes: every text file checks out with LF; delete a committed profraw (`1671693cc`).
- Native host bindings: a recursive package host type's generated codec reads and writes a value of any depth through one explicit stack instead of recursing once per nesting level (`5c0c27877`).
- Native host bindings: an argument that fails to decode part way takes its finished deep recursive parts apart instead of dropping them recursively (`9e52acc8e`).
- Borrow aggregates only through their proven forwarding lifetime (`3e5d810c1`).
- Refresh ownership count budgets (`bb10f9f20`).
- Keep indexed loans tied to their backing frame owner (`48bece8e9`).
- Design: JSON decode in one pass with a serde-style key loop (#1809) (`ed9461cb1`).
- Design: JSON decode records the owner's decisions (#1809) (`18d877f05`).
- Cleanup: design status lines match what landed today (`bba5fe473`).
- Spec: a static reference `&'static T` is a type, made only from a top-level immutable binding, a constant literal or compiler static data (`8181e9921`).
- Checker: a static reference `&'static T` is checked as a lifetime, one checked edge into 'static per static source (`c85195714`).
- Static references: `&<literal>` refers to its constant, which the backend places in static data once per distinct value (`9e9786d54`).
- Static references: a function value whose type writes `&'static T` takes only a static source there, reported as static-reference-source (`2762758af`).
- Static references: `'static` written inside a `let` annotation's type is checked per reference slot (`17a37a370`).
- Lower the sloppy-code ceilings to the merged counts (#L175) (`d541a0817`).
- Promote aggregate readers without flattening nested literals (`7d115e5e8`).
- Member directories: `members.id()` is a compiler-assigned `usize`, one per directory, the same at every use (`8fcb8a729`).
- std.serde: `Deserializer.skip()` reads one value of any kind and drops it (`cc7e758c0`).
- std comments: std.structure and derived Arbitrary describe the Rust compiler, not the deleted TypeScript prototype's early copy (`8758e20e2`).
- std.serde: a keyed reader names a record's members in text order; derived Deserialize reads them in any order, absent ones last, and a repeated key is the reader's duplicate error (`7e159a843`).
- Spec: the language chapter's `std.serde` block says it is abridged, links the std chapter for every method, and gives `member` and `Serializer.begin_variant` the reference parameters lib/std has (`340da8618`).
- std.json: `decode` reads the text once through a keyed stream reader, takes record keys in any order, reads internal, adjacent and untagged enums, and the indexed TextReader is deleted (`e70be2212`).
- Counts: record the JSON decode reader's skipped count operations (#1809) (`79b64bd18`).
- Refresh ownership count budgets (`6c8a95a6a`).
- hd test: `[host]` entries resolve against the root's every source file, so an executable's `@host` trait no longer reads as invalid-manifest (`896857559`).
- Ownership passes: a type's reference facts are one entry of the run's layout memo per type id, made from its parts' entries (`646981613`).
- Driver: the layout memo's ready check compiles for the web target (`15264ba90`).
- Ownership passes: the run's layout memo is a table indexed by type id, so a question is an index, not a hash (`f92bac195`).
- Declarations over a doubling type: the variance walk, the implementation-head search and the signature check's projection search visit each distinct type once (`e844d3926`).
- A body with no type arguments chooses its bound calls at a closed receiver before the passes, so an opaque result's or a template `Self`'s call no longer fails invariant 24 (`aad0c7768`).
- Web compiler: the gzip ceiling is 2.6 MB (owner, 2026-11-17) (`276403e56`).
- Release: a local function whose parameter type names its generic item's type parameter stays a call in the generic body, so its layout is never asked (`b78fd80fa`).
- IR audit: ir-unused-read says it reads a dump slice (`5f791c49d`).
- Delete superseded and unreferenced code (`38dbc941c`).
- Instance TIR step 5, part 1: instance bodies close their opaque results, and the handle-fact copy uses the one builder (`02acb7627`).
- Index owned-verifier entry and exit metadata by sub-body (`3160c1b5f`).
- Ownership: a generic function's facts body is marked and verified as its one instance (`f33a72b75`).
- Preserve returned reference loans across non-inlined calls (`877489e35`).
- Ownership: a dev build takes each generic instance's owned form from the store while its key and recorded body reads are unchanged (`dc78896a9`).
- Cleanup: instance TIR status and step 7a match what landed today (`b2e43db00`).
- Lower the sloppy-code ceilings to the merged counts (#L190) (`eb621bbc1`).
- Lower the sloppy-code ceilings to the merged counts (#L191) (`472f363da`).
- Check a compound assignment's operator result against the place's type (`53438d364`).
- Re-choose trait calls through an opaque bound once a body's opaque results are opened (`3630801ef`).
- Compare an impl method's types with a trait type that names an associated type (`1479dd95b`).
- Lay out a type's origin slots with an explicit stack (`cfe81bd80`).
- Delete the old cache namespaces' retirement, report collection's invariant errors as bugs, and fix stale names (`d3eea647c`).
- One unsigned LEB128 encoder, decoder and length in hd_base (`560106dd9`).
- Stop storing the always-zero list and map word (`d535e06ed`).
- Write drop flags as TIR statements on generic owned bodies (`2a5236de0`).
- Save only a cold frame's own drop flags for its destructor (`07e1e5ce8`).
- Move cell drop flags onto TIR flag statements (`69fa9f72b`).
- Lower the sloppy-code ceilings to the merged counts (#L200) (`e2457b9cf`).
- Say what drop_flags.rs keeps in INSTANCE_TIR.md's step 7 row (`421bcec83`).
- Design: backend-neutral TIR, a Lowered phase that is all a backend reads (`80fb5a213`).
- Backend-neutral TIR: record the owner's acceptance and the seven decisions (`b51cdc117`).
- Backend-neutral TIR: price step 5, the temporary destructor code, generated bodies and Box (`514bc60c0`).
- Review overrides for #D2168 (`754357d4b`).
- Backend-neutral TIR: name no deleted or future path (`6eea64683`).
- Count a folder cycle's tangle as its component and find its fix file in a set (`f6218de4c`).
- Say in hd_check's design that a region's loan set costs its members (`d343e6c32`).
- Walk trait Self queries iteratively (`cb9a21805`).
- Walk solver variable and clause queries on explicit stacks (`d032fdfba`).
- Make shared type erasure iterative (`04421ecb2`).
- Fill associated arguments with an iterative memoized walk (`e5afa6d7c`).
- Run resolver type walks on the shared stack driver (`869f4677b`).
- Walk instance heads and row-key depths on explicit stacks (`7c0473156`).
- Trim the owned verifier to checks no pass constructs; fold the instance check into it (`08c694a28`).
- Collect the owned verifier's hoisted places once, not per collection change (`2c1874ae9`).
- Backend-neutral TIR: verify every precedent with the real compilers; lift generic local functions as Swift does (`5d892f83a`).
- Backend-neutral TIR: step 0 landed at -365 (`d9f8943c4`).
- Review overrides for #2169 (`ebf777d8f`).
- Lower the sloppy-code ceilings to the merged counts (#2169) (`18c00519a`).
- Walk strict opaque-return queries iteratively (`10e74b76a`).
- Cleanup: built designs leave future-work; stale ticket markers follow the backend-neutral TIR (`84d894a2d`).
- Match solver type heads with explicit continuations (`dc0a283ca`).
- Move the target-independent layout and type facts into hd_layout (`377945c23`).
- Make enums over 16 values Memory and probe by_address by a scalar count (`b167d3528`).
- Load and store a value's leaves at an address through four Func helpers (`8746a5626`).
- Say that enums over 16 values pass by address in the layout docs (`6d6559673`).
- Store a value's leaves from a source through one Em helper (`949698d81`).
- Lower the sloppy-code ceilings to the merged counts (#2170b) (`026826749`).
- Backend-neutral TIR: measure layout and value representation against Go, rustc, Zig, MoonBit and Swift (`3efd6745e`).
- Lay out each type once into an interned Layouts table (`1c8a0c827`).
- Build a struct's release plan from its fields' layouts (`199d8669f`).
- Resolve inference types and propagate poison on explicit stacks (`f6003d788`).
- Unify inference types with ordered stack frames and row dependencies (`71c02dfa0`).
- Build newtype conversions with explicit continuation frames (`3bbd0433b`).
- Carry typed resolution results and checked conversion plans (`88f441dc6`).
- Normalize projection-bearing type graphs with explicit frames (`e4a4946d9`).
- Compute tuple meets and conversions with explicit pair walks (`9e46c8506`).
- Share the iterative unifier for join shape constraints (`13ed9a30a`).
- Lower the sloppy-code ceilings to the merged counts (#C2167e) (`f6bf4db08`).
- Collect projection lifetimes with shared iterative bound queries (`e10b721cf`).
- Find self references over a finite raw declaration graph (`ea2e0e69e`).
- Keep a parameter over 16 values read whole by address (`1df7de530`).
- Own every generic instance at its own types, whatever its size (`3c229234e`).
- Backend-neutral TIR: the 4,096-leaf fallback is deleted (`1cdad6de0`).
- Pin #2182 and #2183 with reproducing tests (`0b0616f04`).
- Pin the scale row that shows a counter under its own floor (`e414fa15a`).
- Backend-neutral TIR: split step 2 into cancel blocks now and scope lowering after step 5 (`d305a7f91`).
- Give each suspension point a cancel block that known-null fills (`ed579e47b`).
- Backend-neutral TIR: re-plan steps 3 to 8 from a resolved call graph (`3d1052025`).
- Pin the bounds check a while-counter loop keeps in release (`33377dc0f`).
- Mark proven element accesses unchecked in TIR and move the repeated-check proof to hd_opt (`119d18ce1`).
- Remove impossible missing-value-flow states from body facts (`a1350a129`).
- Cover inliner boundaries and share structure-copy admission (`0b71089b1`).
- Move collision and projection conversions onto the shared stack (`94279316c`).
- Pin the bounds checks a call or a field list keeps under a loop counter (`fa5fc056d`).
- Trust the hoisting pass's stable lists in the rising-counter proof (`e3a931e0d`).
- Define folder-cycle loops through the tangle's first folder (`6c28a04ea`).
- Count shared impl-head pairs at the reported alias depths (`530a870b3`).
- Walk an arm or outer loop around a counted loop in its prelude proof (`6030a9ff6`).
- Locate oversized layouts at the requesting item or binding (`df0ddabde`).
- Show the selected scale counter against its own threshold (`66f7a1fa2`).
- Share online and batch SCC discovery in hd_base (`4e9cfb0b0`).
- Lower the mono expect ceiling after SCC consolidation (`dfd4da311`).
- Bound each list's region around its loop in the hoisting pass (`5ea216a92`).
- Keep a list no reference reaches in its length row across calls (`990b4aac4`).
- Ask the hoisting pass's rows in the falling and masked list proofs (`f5ffafdce`).
- Lower the sloppy-code ceilings to the merged counts (#2175b) (`0231090c9`).
- Compute dominance and loop nesting once per body in TIR (#2175 step 6b) (`78ff7d135`).
- Memoize completed dependent-result type pairs (`a145b0638`).
- Count shared type-pair work and preserve positional results (`739a9bba3`).
- Poison failed post-check projection normalization (`d88df343e`).
- Check static lifetime annotations on module bindings (`71a05de18`).
- Cleanup: the backend-neutral TIR record says what is built; no doc names the deleted alias walk (`5a23cee6d`).
- Skip physical layouts for empty ownership walks (`1715dd686`).
- Move the list loop proofs from the emitter into hd_opt::prove (`1a6f7f43f`).
- Pin a counted loop's element store that keeps its bounds check (`d4dc21508`).
- Say in the docs that the list proofs key a list by its place (`19b0add67`).
- Clear a counted loop's element store as its reads, in the rising proof (`a65618262`).
- Delete the emitter's guard pass, guards.rs, and its wiring (#2175 step 6) (`d5e8f3e95`).
- Lower the sloppy-code ceilings to the merged counts (#2175d) (`024414c71`).
- Delete the unreachable absent-fact specialization path (`37bdf030f`).
- State the existing subprocess launch cost at its call site (`6af70cd63`).
- Compare command-child working directories by canonical identity (`14884cf31`).
- Solve backward ownership dataflow with composed block transfers (`154fa9eb0`).
- Count the emitter's decisions behind a ledger feature, with a runner (`2b1864b11`).
- Fold the emitter's memory-local flags into TIR drop flags (#2172 step 3c, item 1) (`b92637d66`).
- Cover bounds-proof boundaries and share the element-store decode (`998c99d96`).
- Cover implicit and explicit inliner result queries (`f3d97a020`).
- Count the emitter's drop flag reads in the ledger (#2172 step 3c) (`c6dd9f1cc`).
- Move a part's empty pattern into hd_layout (#2172 step 3c, item 2) (`a2d9f8c05`).
- Decide static cells in the storage pass (#2172 step 3a) (`4648c8e29`).
- A suspending body reads known-null's made-cell rows (#2172 step 3a) (`7f1fa90de`).
- A heap cell's box outlives a move of its value (#2172 step 3a) (`0a659c5fd`).
- Lower the sloppy-code ceilings to the merged counts (#2172e) (`57003045d`).
- Test a frame cancelled before its first poll that holds parameter cells (`359a304d1`).
- Known-null reads each sub-body's own storage row for a box (`3ee7942e1`).
- Keep a slot's address after a whole move under a TIR flag local (`dc56fa559`).
- Make each cell's box where its scope starts with a Box statement (`46f2d30d2`).
- Lower the sloppy-code ceilings to the merged counts (#2172i) (`c4f7976dd`).
- Pin pointer-loan lifetimes and the recursive rebind failure (`58aedb445`).
- Regenerate loan baselines with the clean fixture compiler (`1a0a83c3b`).
- Keep checked loan execution separate from WAT snapshots (`776a2e601`).
- Update checker WAT baselines for shared frame storage (`aea6a2ca2`).
- Measure allocator reuse beyond the actual reserved prefix (`a74b45962`).
- Place run-time pointer loans in the checker as LoanBegin/LoanClose statements (`e2d7e2ae2`).
- Keep run-time loan facts linear on bodies with many sub-bodies and nested places (`fcd2e944a`).
- Lower a loan whose site elision keeps nothing to no code, and rebless for a74b45962 (`eaa9ab1ce`).
- Drive a cold call at once past its loan statements, closing its argument loans first (`758f85b26`).
- Cover LoanBegin and LoanClose in the TIR text corpus (`fe583cf2d`).
- Let a quiet constant body hold run-time loan statements (`0fb38651e`).
- Give a statement inserted before a loan close the anchor of where it runs (`57431440e`).
- Close a run-time loan before the Drop that may release its object (`e1510fdb8`).
- Count loan placement work per axis, with a many-breaks shape (`fcc54cf76`).
- Bound loan placement work by loans times branches (`8be347a32`).
- Rebless the hd_check receiver-loan wat for a close before the release (`351550d76`).
- Lower the sloppy-code ceilings to the merged counts (#STEP5A) (`d63923f1e`).
- Record mutable-parameter capture failures (#2198) (`34f57f787`).
- Pin optimized capture failures to their TIR follow-ups (`37b4b3085`).
- Pin audit #1803's open back-half items as stage cases (`769445034`).
- Check an access against the crossings the link pass decided (`ec9e439fa`).
- Fix each loan slot's own-loan key before emission (`3ecf650ba`).
- Review overrides for #STEP5B1 (`5749a2cae`).
- Lower the sloppy-code ceilings to the merged counts (#STEP5B1) (`007e8498a`).
- Publish the CLI as `hd-lang`; rename hd_cli to hd_command (`943cb1606`).
- Build the crate packages from nothing on every publish run (`ba16b09c6`).
- Record steps 1, 3 and 5a as built in the backend-neutral TIR design (`b0a325248`).
- State the per-run cost of the publish script's processes (`e0c841ada`).
- Format the micro-bench scripts the rename left unformatted (`c00f00fce`).
- Count the own loans a closure shares in a TIR counter local (`160c70b6d`).
- Decide each access's crossings and root binding in the link pass (`ec6255e76`).
- Record step 5 as built in the backend-neutral TIR design (`9d75596f8`).
- Wrap the sort benches' generator explicitly, so dev builds don't panic (`adf1d4e99`).
- State copy counts as Retain statements and fold the emitter's three stores into one (`ea0400725`).
- Review overrides for #STEP4A1 (`eba3f2e57`).
- Refresh ownership count budgets (`26a505c3a`).
- Say `cargo install hd-lang` on the website (`bbe542e8d`).
- Record the lent values in the TIR boundary rows, not in the emitter (`d3e3ef7ee`).
- Stop releasing unused values in the emitter: only lent ones reached it (`b7150d301`).
- Review overrides for #STEP4A2 (`b16e6f6f4`).
- Lower the sloppy-code ceilings to the merged counts (#STEP4A2) (`71aca226a`).
- Pin reserved-list uniqueness checks across callbacks (`8e6e8fb77`).
- Decide promoted arguments' types and loans in the TIR boundary pass (`65dcdb6fb`).
- Review overrides for #STEP4A3 (`27ac0f0e6`).
- Specify SDK wire conversions and hide the HTTP host constructor (`124949db2`).
- Specify the public HTTP wire transport contract (`7ad1eb8ac`).
- Review overrides for #T2208 (`7894458c9`).
- Mark short loans at link from shared ownership facts, not in the emitter (`3150a0dc4`).
- Decide each parameter's entry in the TIR boundary pass, not the prologue (`73a268dec`).
- Review overrides for #STEP4A5 (`36f974ddd`).
- Lower the sloppy-code ceilings to the merged counts (#STEP4A5) (`30ad2dfb2`).
- Keep ASCII string case conversion inside Wasm (`aff1834ae`).
- Pin the shared ASCII case helper and its module byte cost (`3af3cbf60`).
- Keep a reserved list fresh across a callback that never receives it (`f9cfcf7d6`).
- Refresh ownership count budgets (`05e91a344`).
- Pin the stack frame introduced by fresh list reservation (`04a360dfe`).
- Say how to install hd in the command guide and the learn page (`e7fab4b39`).
- Exercise const boolean comparisons and 64-bit shifts (`9b5dcc77a`).
- Exercise intrinsic signature diagnostics in std stage tests (`c901ded20`).
- Exercise const operand errors and cyclic comparisons (`4acb59d4c`).
- Cover floating and prefixed string const argument diagnostics (`bbaacd059`).
- Exercise overlap checks for union, trait-value and row heads (`74cb26def`).
- Cover derived implementations carrying row arguments (`09566b317`).
- Reserve the initial points in MoonBit's reference, as hd and Go do (`0afc5d13a`).
- Cover interface codec damage and lazy metadata boundaries (`c79bd551a`).
- Lift generic local functions as Swift does and delete local_fns.rs (`28fb8575a`).
- Cover parser recovery boundaries and exhausted cursor guards (`53935f01f`).
- Build local-function templates in linear time (`798483205`).
- Review overrides for #LIFTCOST (`65c414121`).
- Pin collection reservations before materialization fills (`e80f14fa9`).
- Review overrides for #T2209 (`3a95fb489`).
- Revert "Build local-function templates in linear time" (`fc8021e6d`).
- Review overrides for #REVERTLIFT (`75a6f16ac`).
- Cover syntax source boundaries as one area batch (`3966143a7`).
- Refresh ownership count budgets (`ee2be1a69`).
- Build local-function templates in linear time (`91e76c316`).
- Review overrides for #LIFTCOST2 (`1c334daa8`).
- Cover resolver source and declaration query contracts (`55ce55bec`).
- Show how to use hd as a plugin, a sandbox and a workflow (`7a68866b9`).
- Cover type-pool mapping and wire rejection boundaries (`3b02ef1fe`).
- Cover inference joins and data-row equality boundaries (`524f80658`).
- Cover solver recovery answers and memoized fuel boundaries (`445f89c12`).
- Cover trait-proof helper contracts and watch cancellation (`7da3c9584`).
- Clarify that backend step 3b is built (`66754eb61`).
- Pin the Debug escape loop and refresh string data addresses (`3e6ee842f`).
- Keep the escape fixture independent of helper numbering (`d3bb4ef09`).
- Review overrides for #T2228 (`d98cfa375`).
- Run dev's generic instances through release's stage order (`da2feb0ec`).
- Check an instance body's settled types in one place (`e606557c7`).
- Let the effect view's counted answer come from the environment alone (`a44d8b375`).
- Read a generic item's checked body for collection's shape analyses (`c12e519ec`).
- Key the inliner's callees by one identity, and pin the escape gap (`7748ebc86`).
- Dump the lifted-call cases' WAT from the release build alone (`73ba60e31`).
- Count each lifted copy the inliner builds as an instance (`21fc0e06f`).
- Count values through per-type shims; delete the walk route (Refs #2173, 4b1 + 4b2) (`62ae1844a`).
- Review overrides for #STEP4B (`8405b6f84`).
- Lower the sloppy-code ceilings to the merged counts (#STEP4B) (`e0a3b609f`).
- Pin three using! stage cases before cleanup scopes lower in TIR (`d369a5cad`).
- Share each suspending body's Pending and cancel exits; load the frame flags once (`a4fead55a`).
- Lower the sloppy-code ceilings to the merged counts (#STEP7AB) (`4514c8ddd`).
- Look up a deferred suite by value so hd_tir builds on wasm32 (`614fc6a42`).
- Point the lowering catalog's scope links at the TIR scope lowering (`1ee5d11c1`).
- Docs: make docs/compiler and the backend plan's status match the code (`a4c1442f5`).
- Docs: make crate DESIGN.md files match the code (`4f2f988f4`).
- Spec: drop two notes about the deleted reference prototype (`0ff5207dd`).
- Comments: fix comments that name deleted code or old behaviour (`b1c16fc25`).
- Review overrides for #DOCS244 (`2c364232c`).
- Cite the TIR v2 alternatives record by commit, not by a deleted path (`a462ef7a0`).
- Count an empty optional's one address only when it is set (Refs #2173) (`1d4d92f5a`).
- Review overrides for #NOOP4B (`f48622e8f`).
- Record json's and structure_walk's skipped count tests after 4b (Refs #2173) (`1b65f74fe`).
- Refresh ownership count budgets (`9261054b1`).
- Count a struct kept by address in place or by value, by where it lives (`805f1350d`).
- Count a collection's copy as its buffer's address, deleting the copy route (Refs #2173) (`17cb99b35`).
- State a small struct's counts inline in runtime glue (Refs #2173) (`68236c9e9`).
- Lower the sloppy-code ceilings to the merged counts (#STEP4CLOSE) (`c6158ceaa`).
- Record step 4's conservative source-call sites as a reclassification (Refs #2173) (`c4eca103d`).
- Free a fresh, solely owned buffer at its drop with no test (Refs #2201) (`f0077e6ef`).
- Review overrides for #T2201 (`8fb3e1c31`).
- Refresh ownership count budgets (`11cbca893`).
- Borrow a function value called through a reference (Refs #1436) (`bc27a986a`).
- Refresh ownership count budgets (`878fc0483`).
- Add the execution-boundaries design record (PR #1139) (`14cc0e65f`).
- Borrow a function value called with a reference to a local (Refs #1436) (`1b6c098f4`).
- Review overrides for #T1436B2 (`e4bd6b82e`).
- Refresh ownership count budgets (`eea9ab8ab`).
- Point the execution-boundaries record's parallelism link at its own Appendix D (`12f68d7ee`).
- Refuse a local conformance run that selects more than half the spec (`87e0bf13d`).
- Keep an awaited call's argument loans for the whole call (Fixes #2197) (`a2da05417`).
- Lower the exit set, drop elaboration, the frame drop and cell drops to TIR (step 7c + 7d) (`ae8b1fd98`).
- Record step 7c + 7d's count budgets: no-op and skipped calls fall (`d0959e614`).
- Review overrides for #STEP7CD (`6fa8c72d9`).
- Lower the sloppy-code ceilings to the merged counts (#STEP7CD) (`dc40fb16d`).
- Refresh ownership count budgets (`4b4a8c43a`).
- WIP step 8b A+B: an elaborated body is Phase::Elaborated, checked by the lowered-TIR contract (invariant 25) (`90101b1a1`).
- Sleep through the Clock row in the server-sdk serve tests (Refs #2197) (`864c38ee7`).
- Key a parameter environment by its clauses as they change (Fixes #2257) (`a2106fdcc`).
- WIP step 8b C: a move's emptying is a TIR statement (Intrinsic Empty), checked by L3'; owned.rs's instance tests in their own file (`793bc586a`).
- Review overrides for #STEP8C (`6a18710f7`).
- Keep a function value's input lifetime needs at its declared types (Fixes #2258) (`1030f603c`).
- Check function values' input lifetimes at every declared function type (Fixes #2269) (`f27519156`).
- Renumber a drop flag statement's value root, so a matched payload's hook runs once (`72c3dfd61`).
- Flag a hooked local a store may overwrite, so its hook never runs on an empty slot (`63b6ce96f`).
- Reject a move out of a captured binding in its own scope too (ref.move.out-of-place.cell) (`8dc53990b`).
- Check function values' input lifetimes through locals (Refs #2258, #2269) (`338242165`).
- Run the old value's Drop hook when a closure overwrites a shared binding (`5afc9fcdf`).
- Replace the write-hd skill with hd-lang from the hd-skill repo (`d55037a9f`).
- Split the static-proof walk off the body checker, so a header can run it (`ee6005bd8`).
- Cite the static-proof walk's real rule, ref.generic.static.impl (`c9f6fdb83`).
- Let an explicit type argument write `'static` (Fixes #2260) (`7dd5045e9`).
- Check a generic default against its bounds' outlives requirements (Fixes #2261) (`f9bcc6e23`).
- Reword a fixture comment the spec check read as a header directive (`c219a8401`).
- Check an associated assignment against its bounds' outlives requirements (Fixes #2263) (`21da20e92`).
- Check a supertrait against its outlives requirements at the impl (Fixes #2262) (`1025a76fa`).
- Count the header outlives walk and pin dependency headers (Refs #2261 #2262 #2263) (`a97228638`).
- Check a delegated field against its outlives requirements (Refs #2264) (`dd1f87620`).
- Keep the delegated field's header error as the delegation diagnostic (Fixes #2264) (`b1cc0ecf4`).
- Review overrides for #T2264 (`14ca2860b`).
- Move the server SDK to its own repository; std keeps HTTP in both directions (`9e338b7be`).
- Carry a bound's written trait-argument lifetimes through its proof (Fixes #2267) (`d932f9563`).
- Walk a template's member steps in an outlives proof (Fixes #2268) (`811960166`).
- Docs: mark the backend-neutral TIR plan built through step 8; drop pointers to closed #49 (`04ae768f8`).
- Line up an outlives goal part by part, never refusing one (Refs #2263) (`52c3da5d8`).
- Review overrides for #REDTIMEOUT (`701b6ae63`).
- Learn a deferred bound's binding once its self type is inferred (Fixes #2270) (`556c4732f`).
- Trace a called function value from a call's result or a struct field (Fixes #2269) (`657cbff56`).
- Mark two audit-1803 fixtures' tickets as closed on measurement (`3037864c9`).
- Stop splitting a match class its first row already covers (Fixes #2274) (`0150490ec`).
- Build the checker's scale programs and bound their emitted size (Refs #1800) (`994af934e`).
- Delete the unreachable implicit enum box route (Refs #1442) (`63c9e8a35`).
- Decode no module's items the whole-program checks do not read (Refs #1801) (`ced10e122`).
- Design exploration: what a warm hd check should cost (Refs #1801) (#2284) (`33c17e446`).
- Count internal compiler errors apart in core coverage (Refs #1442) (`404e68593`).
- Drop duplicate lowering of expanded copy updates (Refs #1442) (`466268cae`).
- Reuse normalized type facts instead of repeating nominal queries (Refs #1442) (`14bbe28c1`).
- Name the repository on warm-check citations of other projects' files (`6250d24ee`).
- Key disk files by their texts' hashes, so a resave keeps the fast path (Refs #1801) (`19614e1a7`).
- Review overrides for #D44 (`a5e2ec12a`).
- Cover row-adapter keys made equal by substitution (Refs #1442) (`a062367d7`).
- Compare two just-saved runs in the serial work-report test, with no wait (Refs #1801) (`4086ba292`).
- Answer `hd doc std.NAME` after the check `hd check` makes, without a build (Refs #1801) (`91174edc7`).
- List a dependency's library alone, not its tests and tasks (Refs #1801) (`82746f662`).
- Check every edit kind warm against clean, by edit script (Refs #1801) (`625af1406`).
- Take an unchanged module's check facts from its row in the last-run record (Refs #1801) (`1b1906399`).
- Review overrides for #ROWS (`254ced6ab`).
- Discover a plain check's modules once, and hand the table to the build (Refs #1801) (`fbb6cc248`).
- Review overrides for #DISC1 (`57c32c989`).
- Save the listed files before each compared run of the dependency-tests count (Refs #1802) (`c9e74ce0a`).
- Allocate through mimalloc under the counting allocator (Refs #1802) (`1be9bbeda`).
- Write a timing trace of tasks and serial phases with HD_TRACE (Refs #1802) (`92ba6ab51`).
- Size the default pool from the cores other processes leave free (Refs #1802) (`02700564d`).
- Put each path and symbol shard on its own cache lines (Refs #1802) (`ff24ac97f`).
- Expect no load average on Windows in the default-pool test (Refs #1802) (`5a0ed6765`).
- Read, hash and skim a cold run's files on the pool before its task graph (Refs #1802) (`d23e03d4e`).
- Keep the path and symbol shards on the heap (Refs #1802) (`6ad042962`).
- Answer a float's text from the Node host without an encoder (Refs #1805) (`b564f66a7`).
- Borrow the regex program's lists in each thread step, not copy them (Refs #1805) (`26ac7f5dc`).
- Review overrides for #REGEX (`a599b3b48`).
- Add a regex runtime benchmark over a generated request log (Refs #1805) (`69b147133`).
- Review overrides for #REBENCH (`d0c19b0d4`).
- Compile a regex to a byte automaton with byte classes, for a lazy DFA (Refs #1805) (`326e6f6a3`).
- Add a lazy DFA search over the byte automaton, forward and reverse (Refs #1805) (`d31cf5af8`).
- Hold the Pike VM's program and scratch lists in UniqList (Refs #1805) (`0540c7897`).
- Search with the lazy DFA, and keep one VM per regex across calls (Refs #1805) (`65ae495c4`).
- Pin that a comma ending a closure's last body line stays in the body (Refs #1805) (`a560020ed`).
- Record the regex DFA's conservative sites in regex-scan (Refs #1805) (`f81358632`).
- Add method `where` bounds on a trait's or impl's own parameters (Refs #2287) (`a5f62ab4e`).
- Review overrides for #WHERE (`b182eac4e`).
- Keep the space between an optional result and its `where` clause (Refs #2287) (`96aacd8f0`).
- Review overrides for #FMTQ (`f8f848958`).
- Add Iterator.min and Iterator.max over a method where bound (Refs #2287, #2283) (`326ef8d33`).
- Break min and max ties as Rust does, everywhere in std (Refs #2287, #2283) (`30c5bb44f`).
- Show what Iterator.max and min emit, in a WAT stage case (Refs #2287, #2283) (`17cbf4886`).
- Mark the execution-boundaries record accepted and link its tracking issue (Refs #2288) (`0f782313f`).
- Cover method where bounds in the scale syntax shape (Refs #2287) (`28305755d`).
- Drop min and max from dyn Iterator (Refs #2287) (`3f4d5222a`).
- Refresh ownership count budgets (`b95d98140`).
- Specify execution boundaries: markers, crossing closures, frozen loans, Remote (stage 0) (`e09b4b2eb`).
- Plan execution-boundaries stages 1 to 6 with sites, prices and costs (`e1ef69ae4`).
- Review overrides for #EB0 (`f8be5a301`).
- Cite the two retired rules in the boundaries plan as plain text (Refs #2288) (`ebeb69a57`).
- Name the boundaries plan's not-yet-written files as plain text (Refs #2288) (`44b87a7a1`).
- Answer Send, Wire and library markers from a type's parts (stage 1) (`0f2ebea9e`).
- Parse and record crossing parameters' marker bounds (stage 2) (`012277225`).
- Reprice the boundaries plan: stage 1 built at +327, the codec moves to stage 3 (`b20c64500`).
- Delete dead items: hd_stdpack, the stepping scheduler, hd_run's Provider (`754d842fa`).
- Review overrides for #RED1 (`54bc93516`).
- Count std.marker among the std modules that check clean (Refs #2288) (`46f785f0b`).
- Check crossing arguments: the literal, its captures' marker, one call (`bd0f34f4c`).
- Review overrides for #EB2B (`4b9a307be`).
- Copy or move a crossing closure's captures, never share a cell (`0ac302011`).
- Review overrides for #EB2C (`8323f230d`).
- Bind impl heads with selection's one matcher; reuse select's arguments (`579cded8f`).
- Lower the sloppy-code ceilings to the merged counts (#RED2) (`e4548d5ef`).
- Label copy reads with one copy_reasons on every target (`790b940be`).
- Review overrides for #RED3 (`dfbe98a12`).
- Lower the sloppy-code ceilings to the merged counts (#RED3) (`7f1629bb5`).
- Count std.marker in the std module-finish tally too (Refs #2288) (`ef7b90811`).
- Reject a crossing closure that reaches a mutable top-level binding (`d96903dd8`).
- Check no capture against a marker bound that names no trait (`aa25be410`).
- Read crossing roots' facts without a catch-all, runtime modules by lookup (`f80bb5cad`).
- Review overrides for #EB2D (`9484971f7`).
- Scan closures and joins once for their count packets (`2865b2716`).
- Review overrides for #RED4 (`b8ecc2aae`).
- Cover crossing parameters' marker bound in the scale syntax shape (Refs #2288) (`b856002c6`).
- Put a link's layout scope in place with Scope, not a copy of it (`bda2643d3`).
- Review overrides for #RED5 (`98163bd53`).
- Freeze a crossing closure's captured references for its bound's lifetime (`b7ce0c6ce`).
- Review overrides for #EB2E (`35daedfe6`).
- Find a place's root read with one walk in coll.rs (`9144f5fd0`).
- Mark BACKEND_NEUTRAL_TIR's step 1 rows open as the code stands (`67ebbd0d5`).
- Review overrides for #RED6 (`83a4572bb`).
- Follow crossing closures' calls across the package's modules (`a95e191a5`).
- Record stage 2 of the boundaries plan as built at +792 (`06ec597e0`).
- Review overrides for #EB2F (`1a92ccad7`).
- Take the fork's init borrow once when draining a body's results (Refs #2288) (`f2a6575f1`).
- Declare std.remote's language items and check R1 and R2 (`5c9646dac`).
- List each dispatch key once in the crossing reach summary (`af345eb71`).
- Review overrides for #EB3A (`69dcfaf21`).
- Give a remote implementation method the trait's public result (`def5ce485`).
- Check remote::[T] and handle_call!'s root (`a1e2c3236`).
- Order stage 3's codec before stubs in the execution boundaries plan (`8a81244fc`).
- Review overrides for #EB3B (`485f17a8a`).
- Show the R2 widening's cost at each exit in a WAT stage case (`c64c20eec`).
- Map the Wire codec's sites in the execution boundaries plan (`2b6f38b80`).
- Review overrides for #EB3C (`a6a60dc7a`).
- Declare inline and cold in std.core so the runtime hash keeps its hints (Fixes #2208) (`82f958321`).
- Generate the Wire codec as std code, selected per type like dbg's printer (`5bc00da65`).
- Record that the host boundary move fails its size gate (`0fba5c9a4`).
- Lower remote::[T] and handle_call! to generated stubs and dispatchers (`84380cd9f`).
- Record that folding host handles into stubs waits on the size gate (`ed8836a82`).
- Review overrides for #EB3D (`aae4605c0`).
- Cite the nbody bench by its file in the boundaries plan (Refs #2288) (`1e69f8507`).
- Write std.remote's homes, served functions and HTTP adapters in hd (stage 4) (`e8d5eb832`).
- Review overrides for #EB4 (`c67549491`).
- Serve objects a remote method returns, with tokens and leases (R3) (`851ba415b`).
- Review overrides for #EBR3 (`ef1ed21e9`).
- Format std.remote with hd fmt (Refs #2288) (`68da0fb0c`).
- Build each program from only the modules its entry reaches (stage 5) (`be23abd4e`).
- Review overrides for #EB5 (`b5ed03f49`).
- Re-price stage 6 on std Arc with counts never atomic (`cd8604b94`).
- Add established Option and Result fallback methods (`1361d128c`).
- Add Rust-style min and max on both float widths (`410893075`).
- std: add Map.insert and Map.clear for #2283 (`e0780e643`).
- Update unwrap fixtures and guidance for the accepted std API (`f97b06732`).
- std: add value-consuming iterator methods for #2283 (`169404292`).
- test: pin mutable-reference dot dispatch for #2286 (`154fb60a2`).
- std: collect characters into strings and enumerate lists (`c93e79b6a`).
- test: refresh optional method name suggestion after unwrap lands (`7ea6bcc55`).
- Carry the regex DFA budget through the std API rebase (`8f528fb00`).
- Review overrides for #S2283D (`f825d2603`).
- Expose integer limits through Integer methods (#2289) (`78a7c3345`).
- Build a program's reached folders whole and root its walk at the runtime (`554a46f14`).
- Review overrides for #REACHFIX (`841392851`).
- Scope a program by module bodies only, and leave std's own programs whole (`43acea349`).
- Review overrides for #REACHFIX2 (`39922c858`).
- Reject a function with a crossing parameter as a function value (`00c7771f9`).
- Lift a Wire crossing literal to a function of its captures (`e04252fcd`).
- Run a lifted crossing literal on a worker through std.remote.Workers (`0e680470d`).
- Number a lifted crossing literal by its item's path, not its DefId (`41ae6ad02`).
- List std/remote.md's workers example in the examples inventory (Refs #2288) (`538ead9a3`).
- Review overrides for #EB6W (`7bae8ea21`).
- Web compiler: the gzip ceiling is 3 MB (owner, 2026-11-22) (`7e0e7b0de`).
- Encode a Wire crossing's captures only when a domain sends them (`46432ab03`).
- Run Workers crossings in a second instance on Node and wasmtime (`97121846a`).
- Review overrides for #EB6H (`23303e2e3`).
- Keep the borrow of a non-Copy part read that goes behind a reference (`7bfe2497f`).
- Spec: Sync, never-atomic counts, and unique buffers at a Send crossing (`a6ad9992e`).
- Review overrides for #FIX2290 (`da2f020dc`).
- Represent unions as anonymous ADTs and remove the Union tags (Part of #1306) (`f5fb2bc52`).
- Represent tuples as anonymous ADT products (`9ceae48ed`).
- Preserve product join binding order and nominal structure guards (`87994174e`).
- Preserve fixed-product boundaries in type walkers (`809b58d30`).
- Add lazy Set union and intersection snapshot iterators (`0ebb2d2ff`).
- Use remove for indexed List and UniqList removal (`a2db1bc05`).
- Add Unicode char.is_uppercase using the existing range search (`42f72809f`).
- Describe TyData as one ADT form in the compiler docs (Refs #1306) (`5744d1b9f`).
- Check Sync: the marker walk's new kind, and Send of a shared reference (`1ddd73fd5`).
- Review overrides for #FIX2291 (`a53c740c0`).
- Review overrides for #FIX2292 (`3c78ccf92`).
- Add lazy double ended iteration and reversal across std cursors (`4a379f22b`).
- Record the shared iterator end word in the JSON cursor size (#2294) (`f4726cfbf`).
- Review overrides for #FIX2303 (`a6876708b`).
- Add std.sync with Arc and Weak over atomic counts (Refs #2288) (`6c2c6e971`).
- Review overrides for #ARC (`b16f5ede8`).
- Fix reverse adapter stepping and noncopy cursor storage (#2294) (`774453bea`).
- Expect no sealed row for Send and Wire on an anonymous sum (Refs #2288) (`ef32f9cfe`).
- Share the heap with threads: wait, notify and std.rt.thread (Refs #2288) (`5d7b1b275`).
- Review overrides for #SHARED (`2e226f724`).
- Give each thread a heap of its own, with frees across threads (Refs #2288) (`5bf16a04b`).
- Review overrides for #ALLOC (`214c7e7dc`).
- Generalize Display join and preserve empty string presence (#2295) (`af46e49c8`).
- Record streaming join raw-buffer cleanup sites (#2295) (`550250e07`).
- Bring the execution-boundaries plan's status up to date (Refs #2288) (`41798fe9c`).
- CI: cache workspace crates with sccache and split hd_driver's slow test binaries (`5e86f6ed1`).
- Normalize empty owned-buffer strings in std (#2295) (`0821dba33`).
- CI: shard the slow conformance modes and the host jobs (`63085eaaf`).
- CI: take the sharding back out; keep sccache (`0ac067035`).
- Take a Send crossing's buffers unique as they cross (Refs #2288) (`4be8f5589`).
- Review overrides for #UNIQUE (`21461b33e`).
- Record the owned Buffer arm verifier failure (#2295, #2306) (`9205e09e2`).
- CI: stop repeating conformance work (`a90809d68`).
- CI: enable sccache only once its server has started (`3f2681644`).
- test: borrow the contains needle in the lint fixture (`35f32ba70`).
- CI: build the workspace once per run (`ba6b82835`).
- CI: build each crate package once in the package check (`07ffe5fe8`).
- Tests: shrink hd_driver's work tests to what their checks need (`b9761c5c6`).
- CI: one host job per system, built with one feature set (`1952b1521`).
- CI: cache the package check's compiles with sccache (`1077b9569`).
- Review overrides for #CICUT (`7f8eaf5d3`).
- Merge main 7f8eaf5d3 into the borrowed-contains branch (`a1f1fef01`).
- test: account for the new float constant modules (`b9e4372ef`).
- std: name Horner steps so float kernel lines fit (`369884025`).
- Review overrides for #FLOAT (`13a82ed5f`).
- CI: print each test binary's wall and CPU seconds (`79f1077bd`).
- CI: make target/tmp before running the test binaries, as cargo does (`f45361a0d`).
- CI: run the wasmtime host's tests in the CLI's test job (`ca4dca5b1`).
- Tests: link the wasmtime host's integration tests once (`a868744f2`).
- Review overrides for #CICUT2 (`b74337925`).
- std: consume map-entry projections without extra copies (`230baf521`).
- Count 59 std modules: #2293's f32/f64 on top of thread and crossing (`b4af3f47f`).
- Merge main into the Map cursor branch (`71b168a1e`).
- Merge the std module-count correction into Map cursors (`2ba82d10f`).
- Merge main and refresh generic callback ownership call names (`90d97098f`).
- hd_fmt tests: drop Rust-string cases that spec/conformance/cli/fmt-width already checks (`ed426bbc2`).
- hd_lint tests: drop Rust-string cases that CLI fixtures already check (`6eb1aa44b`).
- hd_command tests: drop Rust-string cases that CLI fixtures already check (`c8b4d615b`).
- Review overrides for #DUPTESTS (`f7a2cd27f`).
- Merge Map cursors with the generic callback shape (`b93987345`).
- Import the debug-only test's place types inside that test (`245ec0390`).
- CI: run optimized conformance on V8 only (`65f79bc3b`).
- Count std modules from lib/std in the checker test (`0db1d1267`).
- Delete the golden std TIR test (`0ab23034a`).
- test(std): reconcile generic callback golden and count budget (`a117f00d6`).
- Share the numeric-family work test and reduce its sample widths (`31e450d8a`).
- Change quality: an hd program added to a Rust string of a test fails (`a2db12476`).
- hd_fmt tests: formatter cases move from Rust strings into cli/fmt-width (`8fcefb825`).
- Review overrides for #BUGS3 (`b780fa263`).
- Lower the sloppy-code ceilings to the merged counts (#BUGS3) (`e32b2f015`).
- Merge #2299 (generic FnMut callbacks) into main (`be894fe2c`).
- hd_lint tests: lint programs move from Rust strings into cli/lint-* cases (`b5eaa645b`).
- Merge main into Display join after the owned-cache verifier fix (`3849f55e9`).
- CI: run the scale check on the Node engine (`b87c06b31`).
- test(resolve): preserve result paths across interface codecs (`a305828aa`).
- Merge main and reconcile Display join formatter and stage controls (`3d9f63a99`).
- test(resolve): load the result-path codec program from an hd fixture (`cce61ba9e`).
- Merge main into the result-path interface regression (`98cb4a753`).
- Add std.thread: scope, Scope.spawn and Handle.join (Refs #2288) (`cb1e38da9`).
- Review overrides for #SCOPE (`63b2c32a6`).
- Change quality: an hd program added to a Rust string of a test fails (`d48bfb261`).
- hd_fmt tests: formatter cases move from Rust strings into cli/fmt-width (`37a7cd95d`).
- hd_lint tests: lint programs move from Rust strings into cli/lint-* cases (`545521ce4`).
- Review overrides for #RUSTSTR (`87343a329`).
- CI: run the Test workflow on pushes to tests/** branches (`06b3fbf67`).
- Website: list the std.thread chapter (`97625bc52`).
- hd_command tests: programs move from Rust strings into stage and cli cases (`1d66eda81`).
- hd_syntax tests: programs move from Rust strings into stage cases (`ac917456d`).
- Merge commit '98cb4a753' into HEAD (`befbe709c`).
- Review overrides for #COV1442 (`9aa8d338f`).
- Conformance: put a-handle-is-never-wire's diagnostic marker on the call line (`18808787a`).
- hd_driver float_text.rs: the four programs move into std-num and std-text stage cases (`d09fefd95`).
- hd_driver unlowered_intrinsic.rs: the bad declaration is a diagnostics stage case (`79e96c7ef`).
- hd_driver count_audit_tests.rs: std's always-inline check is a diagnostics stage case (`700744457`).
- hd_driver lib.rs: the four bad `@host` markers are one decorator-contracts stage case (`940875035`).
- hd_driver recursive_layouts.rs: the six fixed programs are .hd files read with include_str! (`58b876b00`).
- hd_driver init_cycle_run.rs: the six module cycles are folders of .hd files read with include_str! (`3ed4c99cc`).
- hd_driver function_reference_lowered.rs: the three programs are .hd files read with include_str! (`d78129b1c`).
- hd_driver function_references.rs: the program is one .hd file read with include_str! (`4acfa337a`).
- hd_driver generics_determinism.rs: the 5 programs are .hd files read with include_str! (`79ee1d155`).
- hd_driver rows_determinism.rs: the 3 programs are .hd files read with include_str! (`abbac4b2c`).
- hd_driver header_goals.rs: the 4 programs are .hd files read with include_str! (`9d4280abb`).
- hd_driver impl_universe.rs: the 4 programs are .hd files read with include_str! (`c0bc8aa2a`).
- hd_driver reuse_ifaces.rs: the 4 programs are .hd files read with include_str! (`96c76838d`).
- hd_driver dbg_release.rs: the 3 programs are .hd files read with include_str! (`0745dcf80`).
- hd_driver test_plan.rs: the 2 programs are .hd files read with include_str! (`912c07ce0`).
- hd_driver profile_handles.rs: the 6 programs are .hd files read with include_str! (`6522756e9`).
- hd_driver track_caller_dependency.rs: the 2 programs are .hd files read with include_str! (`1d126adbf`).
- hd_driver row_aliases.rs: the 2 programs are .hd files read with include_str! (`84778a430`).
- hd_driver tests_block_impls.rs: the program is a .hd file read with include_str! (`82e982b3b`).
- hd_driver resume_dispatch.rs: the program is a .hd file read with include_str! (`b6b91a1b9`).
- hd_driver structure_specialize.rs: the program is a .hd file read with include_str! (`7fad2eed6`).
- hd_driver supplied_tuple_specialize.rs: the program is a .hd file read with include_str! (`74c2ed915`).
- hd_driver shared_heap.rs: the program is a .hd file read with include_str! (`067b24979`).
- hd_driver same_name_packages.rs: the 2 programs are .hd files read with include_str! (`bda048ae3`).
- hd_driver release_profile_run.rs: the 2 programs are .hd files read with include_str! (`89ccdf7e2`).
- hd_driver entry_point.rs: the 3 programs are .hd files read with include_str! (`050a5462a`).
- hd_driver structure_run.rs: the 2 programs are .hd files read with include_str! (`503df1e81`).
- hd_driver derive_run.rs: the program is a .hd file read with include_str! (`3a6df6639`).
- hd_driver context_values.rs: the 2 programs are .hd files read with include_str! (`8402990d0`).
- hd_driver scenario_cancel.rs: the 2 programs are .hd files read with include_str! (`60e6285ca`).
- hd_driver scenario_cleanup.rs: the 2 programs are .hd files read with include_str! (`f8433b97f`).
- hd_driver competing_drivers.rs: the program is a .hd file read with include_str! (`280379041`).
- hd_driver session_store.rs: the program is a .hd file read with include_str! (`08b4916ad`).
- hd_driver lazy_ifaces.rs: the 2 programs are .hd files read with include_str! (`6bbf6bfb9`).
- hd_driver iface_put_order.rs: the program is a .hd file read with include_str! (`85a6397c9`).
- hd_driver instruction_origins.rs: the 2 programs are .hd files read with include_str! (`3fb09377d`).
- hd_driver claim_order.rs: the 2 programs are .hd files read with include_str! (`0460a011f`).
- hd_driver allocator_init.rs: the 2 programs are .hd files read with include_str! (`0bffae0ac`).
- hd_driver std_interfaces.rs: the 7 programs are .hd files read with include_str! (`d0ffbe68d`).
- hd_driver fixture_roots.rs: the 7 programs are .hd files read with include_str! (`a66a6774e`).
- hd_driver host_drive_slots.rs: the 7 programs are .hd files read with include_str! (`7ced8cc02`).
- hd_driver fixture_views.rs: the program is a .hd file read with include_str! (`5384910c1`).
- hd_driver std_drop_invariant.rs: the program is a .hd file read with include_str! (`332f19c3c`).
- test(resolve): cover union-template uniqueness per trait (`c662c6525`).
- Review overrides for #COV1442B (`9312959fd`).
- Compare ownership counts with main's stored measurement, not committed numbers (`f511a9162`).
- Pass the gate's options as one map, for oxlint's parameter limit (`dd37c00e9`).
- Review overrides for #COSTGATES (`f0b3d1767`).
- fix(fmt): preserve nested suites and break parenthesized arithmetic (`f0a5a894e`).
- Merge main into formatter fix (`38d124716`).
- Record focused formatter validation for main integration (`842c7a8ea`).
- fix(check): reuse known local-address provenance for runtime loans (`9a9e3f218`).
- fix(own): reuse known-empty store facts across match leaves (`9ebc91833`).
- fix(parser): require a glued bang-call suffix delimiter (`2f52eac67`).
- Add std.thread.background: a task awaits a thread's end (Refs #2288) (`ff04ebec0`).
- Inventory the third std.thread example (background!) (`b8ed6ca91`).
- Review overrides for #BACKGROUND (`b56e9cdcf`).
- Merge remote-tracking branch 'origin/main' into codex/2304-disjoint-overwrites (`df04bb3f6`).
- Review overrides for #FIX2320 (`6712c5923`).
- test(own): accept reviewed empty-store release reductions (`0164623b3`).
- fix(verify): index emptied places by root and projection path (`5486462a1`).
- Merge commit '0164623b3' into HEAD (`a8182f6e9`).
- Review overrides for #EMPTYSTORE (`fa3e130f0`).
- test(resolve): reject and memoize unknown cached item kinds (`e84562157`).
- CLI cases: providers that must never load are plain .ts files (Refs #2316) (`029d48356`).
- Merge remote-tracking branch 'origin/main' into tmp-2312 (`2257bba4c`).
- test(resolve): keep runtime reexports out of import advice (`f78148c89`).
- fix(parser): delete an extra bang after an already-bang call (`a25bb09d5`).
- test(syntax): verify the already-bang deletion repair (`78d78c425`).
- Review overrides for #BANG2 (`bcfd54e5f`).
- test(resolve): cover recorded module-node reads (`e9d83c6b0`).
- hd_driver lib.rs: the fixed programs are .hd files read with include_str! (`3b957979f`).
- hd_driver tests/analysis.rs: the fixed programs are .hd files read with include_str! (`edbd5592e`).
- check-change: judge fixes per commit, not merge commits (`344272e3a`).
- hd_driver tests/conformance.rs: the fixed programs are .hd files read with include_str! (`d9a9160da`).
- hd_driver tests/stages.rs: the four programs are .hd files read with include_str! (`79c7f2b70`).
- hd_driver positions.rs: the retained-sources program is a .hd file read with include_str! (`898ac5daa`).
- Merge main e9d83c6b0 for runtime import hint tests (`7f1c454f1`).
- Format the two never-loaded CLI providers (Refs #2316) (`ba8551503`).
- hd_driver skims.rs: the two skimmed files are .hd files read with include_str! (`ea25db3bd`).
- hd_driver diagnostics.rs: the user program is a .hd file read with include_str! (`6068c2030`).
- Format the two never-loaded CLI providers with oxfmt (Refs #2316) (`660cc61e7`).
- Merge remote-tracking branch 'origin/tests/hd-out-of-rust-strings' into tests/hd-out-of-rust-strings-2 (`0a92b5bb0`).
- refactor(resolve): remove unused top-binding size queries (`194119aed`).
- hd_driver derive_members.rs: the opt-in program is a .hd file read with include_str! (`0dcd18aa3`).
- hd_driver block_decorator.rs: the Style declarations are a .hd file read with include_str! (`646fbd061`).
- hd_driver inferred_rows_run.rs: the tag declarations are a .hd file read with include_str! (`24f2d7a69`).
- hd_driver import_fix_line_endings.rs: the LF program is a .hd file read with include_str! (`44696afd4`).
- hd_driver inlined_callee_edit.rs: the caller program is a .hd file read with include_str! (`926331303`).
- hd_testkit stages: the std-build test program is a .hd file read with include_str! (`fb8e3282a`).
- Merge main 344272e3a into the formatter branch (`d34d6173b`).
- refactor(resolve): render opaque bounds without prefix rewriting (`6265c502c`).
- Merge commit '7f1c454f1' into HEAD (`2a7015362`).
- Merge commit 'd34d6173b' into HEAD (`d53a83613`).
- Review overrides for #FMTSUITES (`732467144`).
- Merge remote-tracking branch 'origin/main' into tmp-2312 (`75ce9c3b7`).
- Merge remote-tracking branch 'origin/tests/hd-out-of-rust-strings-2' into tmp-2312 (`81d193640`).
- test(resolve): cover lifetime arguments on newtypes (`ec6666600`).
- refactor(resolve): let unknown trait ids use the common fallback (`82744e180`).
- test(references): reproduce an Arc dropped before its get() reference's call (`158950c25`).
- Review overrides for #FIX2327 (`5e3e77709`).
- Merge commit '81d193640' into HEAD (`56caa8146`).
- Review overrides for #RUSTSTR3 (`0d8fa2897`).
- test(resolve): cover trait query misses and recovery (`363420be2`).
- test(resolve): cover std header import-advice suppression (`82c4addf3`).
- CI: stop a hung test binary after 10 minutes and name it (`2b606476d`).
- test(requirements): a $.use result escapes through a free lifetime (`3c4bb11ca`).
- test(syntax): the named-edits check case is deliberately unparsable (`31c816ce4`).
- Handle overlap cache misses and update REPL label assertions (`007f4c579`).
- Test resolved error-shape field and message queries (`27779603d`).
- test(sync): a write through a *mut field of a get() reference is kept (`7aad5fc70`).
- fix(wasm): fold top-level constants again whenever the first round folds (`1f8193cda`).
- Review overrides for #FIX2328 (`526c4bb6b`).
- Plan: stage 6 status as built, and what is left (Refs #2288) (`bffa1722b`).
- Require clear boundaries before offering foreign-form edits (`dfa3381fc`).
- Overlap Workers sends on Node and drop a cancelled send's frame (Refs #2288) (`4b1f29a42`).
- Gate: drop plancheck, which checked the closed #1399 plan (`17ba15124`).
- Test generic and impl metadata across interface caching (`e804f21d4`).
- Run Workers crossings in a browser worker on the playground (Refs #2288) (`d5bfbbec3`).
- Remove the unreachable local declaration reordering fallback (`390f7e62e`).
- Cover inherent-member wildcard recovery with stage baselines (`dcdaf6364`).
- Cover requirement-row recovery in traitless derivation headers (`5f064a60c`).
- Keep impl-table rows tied to their validated source records (`ba4ac7dbd`).
- CI: run Coverage twice a day, not every 3 hours (`a7675d81d`).
- test(ref): a reference made through a pointer is local (Refs #2339) (`bd0cedee3`).
- fix(ref): a reference made through a pointer is local to its function (Fixes #2339) (`54f917b7c`).
- refactor(loans): delete the INNER loan a call took for its result (Refs #2339) (`4b7bb326b`).
- Gate: name std's cases for std_rt, and run hd_wasm's stages for hd_check (`9066e1083`).
- Lower the sloppy-code ceilings to the merged counts (#FIX2339) (`7f126464d`).
- fix(ref): a call's result holds its source with the kind the call took (Refs #1410) (`ff66e1167`).
- Retain per-set coverage evidence and omit repeated scale runs (`95404106b`).
- Preserve completed coverage sets when a later set times out (`804944bc1`).
- Remove unused graph-wide coherence wrappers (`de679c602`).
- Cover local and module struct-literal typo suggestions (`91b3b571b`).
- feat(rt): `align_of[T]()` and `needs_drop[T]()` in std.rt.memory (Refs #1410) (`c52e5333c`).
- feat(std): std.arena, an untyped arena of values that live and die together (Refs #1410) (`a9f55fa2e`).
- test(arena): a release wat baseline of alloc's inlined bump (Refs #1410) (`311172046`).
- Remove obsolete unwrap repair advice (`5a7c9da09`).
- fix(std): arena's drop entry is DropEntry, and its drop hook's free is reviewed (`7a10d757e`).
- Compile the counted-reference invariant only in debug builds (`e366660b2`).
- Retain coverage attribution on scheduled runs (`af5b6507d`).
- Delete the unrequested TakeBox runtime helper (`8eabdddc2`).
- Cover deferred diagnostics for declared row-parameter names (`4aab5f226`).
- test(markers): known failures, UniqList and UniqMap are Send and Sync whatever they hold (Refs #2345) (`816c4aa98`).
- test(iface): known failures, a private field type of another module is invisible to Copy and the markers (Refs #2345) (`314a923e8`).
- fix(iface): a folder interface carries the private types its headers reach (Refs #2345) (`83bdaa0f0`).
- fix(rs): a cell's drop slots stop at a part with no drop work (Refs #2345) (`6491453f8`).
- refactor(std): Buffer[T] holds its room as a Ptr[T] (Refs #2345) (`fa4391396`).
- fix(std): TypeId states its Send and Sync (Refs #2345) (`5287eaa09`).
- fix(rs): a marker counts a Ptr[T]'s T as held (Fixes #2345) (`2b8b3b739`).
- test(arena): known failures, std.arena's drop hook frees and forgets while the collector traces (Refs #2345) (`f6571a3cb`).
- Review overrides for #FIX2345 (`19b1cfd76`).
- Cover intersection of two indexed trait argument heads (`3d835ffb4`).
- Cover Self-fixed bindings in function-row adapters (`3cdbe567a`).
- test: repair main after #2345 (run 45343646079) (`01de0e532`).
- Cover recursive reference-permission analysis in collection (`8dce6d4ad`).
- fix(std): std.arena's drop hook changes nothing while the collector marks (Fixes #2348) (`b1618532d`).
- Review overrides for #FIX2348 (`df9b39198`).
- fix(check): a private type an interface carries is private-import through a module (Fixes #2349) (`d014ecf25`).
- Cover map probe function-value key selection (`606f17d28`).
- Cover the shallow generic request-chain limit (`8e422c578`).
- Delete unselected ProgramEnv default method bodies (`2d757917c`).
- Cover returned Http handles at package host boundaries (`46fc7c188`).
- ci: the Build job keeps cargo's timings report (`96e2a6f25`).
- refactor: delete pub items no code names (`c20b5d5f5`).
- Fix host registrations in normal stage builds (`e33af6a02`).
- Delete the unused mono host-import set (`7932bc031`).
- Cover nested known-callback specialization (`9367d08cd`).
- test: cover collection across nested closure bodies (`7cb7493fa`).
- test: cover void defaults outside the scalar fast path (`3f0b09181`).
- test: cover generic permissions in returned row adapters (`44253edc9`).
- test: cover intrinsic defaults on std trait members (`a7184b8cb`).
- refactor: reuse the proven promotion candidate mask (`fa87cc7ea`).
- test: keep type truncation on a UTF-8 boundary (`fec524be6`).
- test: reveal opaque arguments in provider contexts and rows (`b516b9cd2`).
- refactor: bind the body index before scanning instances (`75a75656b`).
- test: reveal opaque results from generic impl methods (`dfcc135d0`).
- test: hd_check's unit tests move to tests/unit over a testing facade (`e005cd934`).
- test: hd_check unit tests read their parse inputs from .hd files (`d1bbe85d5`).
- ci: the Checks job restores the Rust cache before it fetches the build (`f3b9b05fe`).
- test: hd_driver's unit tests move to tests/unit over a testing facade (`d90145fd1`).
- CI: tell a killed test binary from one that ran past its limit (`d1418375d`).
- test: record generic crossing instance number collisions (`4bede7aca`).
- refactor: remove unused suspension side records (`08d30152d`).
- test: hd_wasm's unit tests move to tests/unit over a testing facade (`ad34f94eb`).
- Lower the sloppy-code ceilings to the merged counts (#WASMUNIT) (`32ff35814`).
- fix: use the crossing map entry type on Wasm (`73cdc6acb`).
- refactor: cache pure match decisions in the forward value pass (`962407221`).
- test: replay nested Wire number coverage on merged main (`f92b7e00e`).
- test: record premature release of a boxed trait reference (`6b5e9a428`).
- test: hd_run_wasmtime's unit tests move to tests/unit over an internals facade (`14a199242`).
- test: cover boxed references borrowed as dyn values (`301181295`).
- Keep opaque diagnostic identity in active frames (`5b22448b2`).
- Remove the redundant direct-call plan guard (`0ec0e84f8`).
- Remove unused explicit reference roots from collection (`35b9d1eea`).
- Cover local-trait indexing during failed parameter lookup (`11c14d137`).
- Cover type-qualified calls through local trait scopes (`599554039`).
- Cover named-argument append correction edits (`94335da38`).
- Cover parenthesized propagation correction edits (`d1f4f078d`).
- docs(design): module graph and visibility (#2347, #2346) (`76f1dd412`).
- docs: record the owner's decisions on the modules design (PR #2350) (`484f35d14`).
- Cover missing-else edits after a trailing comment (`e65ec5bda`).
- Record invalid borrowed-Result optional repair (`cc8e89475`).
- spec: modules loop only among siblings; edges go to the declaring module (#2347) (`c059da0cb`).
- Add approved borrowed Result payload views (`ad1c16efa`).
- Add approved writable Result payload views (`3e108a75c`).
- Cover missing-else repairs when Default is shadowed (`9b24ea444`).
- Cover Default and non-Display diagnostic repairs (`0d76f4a41`).
- Cover readonly provider repair creation paths (`d143ac2fd`).
- Cover incompatible Result-to-Option repair payloads (`342b7d009`).
- Cover append advice receiver and element boundaries (`696a40992`).
- Cleanup: align the module known-failure note with owner direction (`13c54b3b9`).
- Cover named function values in REPL sessions (`c10b7e32e`).
- Cover relaxed trait Self move explanations (`959419224`).
- Cover global and generic reference receiver diagnostics (`ed4bbed4e`).
- Cover prefixed template global and missing names (`306c14b3e`).
- Use uploaded coverage measurements regardless of check verdict (`0dadefcc0`).
- Cover cleanup scope and disposal diagnostics (`b9862493a`).
- Drop redundant checked conformance from default coverage (`c8d834614`).
- CI: cap the Coverage job at 40 minutes (owner exception) (`4ff2010bf`).
- CI: Coverage may run up to 60 minutes (owner: it can be longer) (`de17d7630`).
- Cover initialization pattern and shared-argument diagnostics (`b75359cf2`).
- spec: a module loop is legal only among siblings of one parent; edges go to the named module (#2347) (`4dd7d5498`).
- compiler: module graph and units replace the folder graph (#2347) (`6e80c52a8`).
- docs: compile units are module groups, not folders (#2347) (`7df587c8b`).
- test: REPL session function numbers shift with the module units (#2347) (`79ac15a0d`).
- compiler: the compile unit is named a unit, not a folder (#2347) (`3dad41ba7`).
- test: driver tests name units by their first module (#2347) (`e1f6f8f78`).
- compiler: known std.collections items name their declaring child modules (#2346) (`55c9ac5dc`).
- std: split std.collections into a folder module behind a pure facade (#2346) (`2b99f3847`).
- compiler: unchanged units take last run's rows; keys hash closure digests (#2347) (`7ce5091c5`).
- compiler: handle the three sloppy sites phase 2 added in hd_driver (`d7ceae2f0`).
- compiler: no expect on an interface location; the layout test's program in a .hd file (`ccccac482`).
- Review overrides for #MODULES (`f89f03ced`).
- Lower the sloppy-code ceilings to the merged counts (#MODULES) (`4c7367ab2`).
- Cover trait-reference field repairs and nonimplementing values (`7413ad2e5`).
- Cover test-position registration identity and unknown-call hints (`d98db2a1a`).
- Cover noncallable explicit test bodies in both argument forms (`a1302c966`).
- Cover collection receiver and nested-parameter permission diagnostics (`41aafd999`).
- Move TextRoom implementations into their declaring runtime module (`54012e354`).
- Retire the obsolete std.core leaf restriction (`7c38989f8`).
- Refresh TextRoom linked-function baselines after relocation (`6b67fe7e0`).
- Refresh test-plan string offsets after the TextRoom move (`4629c3809`).
- Cover caller-argument scope and poisoned call-place recovery (`c4ee79d13`).
- Cover the callable check of a diverging function result (`6d9f548d6`).
- Cover member promotion recovery after invalid embedded types (`cf94dcaff`).
- Cover local anonymous-parameter lifetime diagnostics (`81d6599fd`).
- Cover reference source selection in local function returns (`856f3ecee`).
- Cover empty row defaults on local generic calls (`17e686185`).
- Include the empty diagnostics companion for local row defaults (`0e3fde69f`).
- Cover signed powers with parenthesized literal bases (`644eb5689`).
- Cover expected tuple slots in multi-provider access (`446e236d7`).
- Cover literal fallback in non-iterable aggregate sources (`2f902294b`).
- Remove an unreachable reference walk in mutable map loops (`bb44b4ec7`).
- Cover singleton tuple arity diagnostics across pattern positions (`52d906745`).
- compiler: a single use of a module depends on its parent (`74191328c`).
- Fix omitted-result cycles across item and local functions (#2362) (`82697d266`).
- syntax: a use group continued on later lines is skimmed to its `}` (`a68f55c56`).
- compiler, std: modules are private to their parent, and packages to their root (`8858cfd70`).
- Fix module privacy's tail: backtrace lines, std fmt, nested-path scale shape (`eef28b641`).
- compiler: the parent edge serves written uses of a parent that exports items (`1bf168de3`).
- Review overrides for #PRIVACY (`bcf02e4d6`).
- website: the playground's multi-file tests give models/ a facade (`4fb33d590`).
- compiler: a count shim's shape tells an optional's address from a plain one (`e64595de8`).
- std: merge std.function into std.ops (#2359) (`793c8ca37`).
- std: merge std.http_wire into std.http (#2359) (`55dabb7a7`).
- std: std.digest becomes std.crypto with std.crypto.sha256 (#2359) (`9278881d7`).
- std: merge annotation, structure, inspect, marker and debug into std.meta (#2359) (`f7609ac23`).
- spec: delete module.rt.leaf (#2359) (`4694615bc`).
- std: InlineHint, inline, Cold and cold move from std.core to std.rt.hint, exported by std.meta (#2359) (`c3dbe3f1e`).
- std: f32/f64 constants become f64::pi(), f64::e(), f32::pi(), f32::e() (#2359) (`8c6dc5ad3`).
- std: Args, args() and ScriptedArgs move from std.host to std.cli (#2359) (`9227857c0`).
- std: serde, json and encoding.hd become std.encoding (#2359) (`683cf6f1e`).
- bench: records and traits give their module folders a facade (`632378df6`).
- conformance: lint-fix-derivation-fact's fix-it offsets follow the std.meta import (#2359) (`67b1e02ed`).
- Review overrides for #STDORG (`ca7612c3e`).
- website: the std chapter list names crypto, not digest (`1c070a0ab`).
- Cover parser recovery independently of mutable std fuzz inputs (`0a28d0656`).
- State the bench checker process cost in the gate format (`ecc9f92e6`).
- Exercise metadata emptiness through an opaque API call (`22881fe7f`).
- Select the moved character collector in its WAT fixture (`b6c9a0172`).
- Place core byte/scalar intrinsics and record current std units (`dded5c1d7`).
- Collect evidence for the intermittent CLI harness SIGKILL (`cc1b28a69`).
- Keep sroa's cold-call spill off on wasmtime, whose every loop carries a call (`20f6be98f`).
- Test that a store into a moved-out hooked slot drops only a live value (`f03ca5391`).
- Narrow a spread store whose kept fields come from a copy of the place (`6b59dee0d`).
- UniqListIter reads its element once its cursor is in bounds (`31ab20ea6`).
- Pin the wasmtime pipeline's sroa knob in its tests and a stage case (`40c200f4f`).
- Spill sroa's copy only around a call whose result cannot be an address (`57a7e5486`).
- A frame-run body owns a parameter that holds a Drop value (`9c6254c09`).
- Handle sroa's refusal as an error and its matches without empty catch-alls (`81eb4a930`).
- A reference made from a raw address in a local's storage keeps the local (`13e3fff5a`).
- Judge a loop's exit test hot in sroa's cold-call spill (`dfc4e60f8`).
- Record sroa's early Drop of a buffer read through a raw slot address (`579bda7c6`).
- Review overrides for #SROA (`a0845e819`).
- Attribute runtime release wrappers to their caller sites (`c5471b61f`).
- Add TryFrom and checked integer conversions in std (`0d0ac7f7b`).
- Replace repeated integer conversion assertions with a pair table (`e5d663b79`).
- Use numeric family casts for one generic integer conversion impl (`9d0aa0e98`).
- Use the current std.meta inline export for numeric conversions (`69fa3ff15`).
- Fix f32 source precision and scalar match decision summaries (`3da18c195`).
- Keep conversion writing-log rows clear of the landed append (`dfe9a0590`).
- Refresh two conversion baselines after SROA integration (`4085fb2a3`).
- Require resolved items at the checker call boundary (`e1c8acbe9`).
- Keep a raw memory access's owner alive to its drop, never past a move (`94cb502e2`).
- Review overrides for #RAWREAD (`8229385f4`).
- Send an inlined body's known constructors straight to their match arms (`810958238`).
- Review overrides for #KNOWNCTOR (`f0cd1f4eb`).
- Hoist loads out of read-only loops and drop the bounds guards their test decides (`b3a9959e3`).
- Review overrides for #LOOPLOADS (`9e95fee5c`).
- Refresh CLI capability-list expectations for Signals (#2361) (`e26d725df`).
- Cleanup: modules design record done, optimizer passes listed (`226c59e19`).
- Remove the obsolete trait-reference upcast search (#1442) (`9125b4bc5`).
- Cover non-sign operands in numeric family casts (#1442) (`5fe1e1dea`).
- Use registered local declarations for result dependencies (`c73f51fd5`).
- Use declaration names in result-cycle diagnostics (`0649fed08`).
- Test scalar fold domains without duplicate validation (`231c841c2`).
- Select primitive folds with the shared eligibility guard (`fc689c3a6`).
- Cover private import fix eligibility and qualified paths (`4b33d3ee2`).
- Exercise reference-table head and owner queries (`ecc49e5aa`).
- Cover missing-colon and inline-member parser recovery (`f2784baba`).
- Cover inherent wildcard indexing with numeric Copy bounds (`0dfbe561a`).
- Cover partial prelude exports through resolver scopes (`09d523d0e`).
- Cover result-cycle deduplication across row rechecks (`a63b1330d`).
- Cover import prefixes hidden inside admitted dependency roots (`18dbc803b`).
- Represent emitter integer widths with an infallible layout type (`ceeb5d1d7`).
- Key a set-once reference local by its target in loop_loads (#2368) (`3330876e7`).
- Carry a constant address addend through a later addition to its load or store (`38cfa07b3`).
- Read a hoisted loop load before the loop's waiting checks (`cd1c50178`).
- Review overrides for #OPTSPLIT (`bbc329ece`).
- Add an A/B bench workflow over two compiler refs (#2369) (`c9b4eb1c9`).
- CI: remove the A/B bench workflow (owner: measure locally with longer cases) (`a5d14ed49`).
- Move StringBuilder byte storage into std.rt (`e5dffdc29`).
- Refresh StringBuilder storage stage baselines (`a28998e98`).
- Review overrides for #SBROOM (`38a380abe`).
- Unreserve data, which starts no declaration (#2384) (`07b53bcfc`).
- Cleanup skill: check removed language forms in every keyword list (`480612ba2`).
- Remove the leftovers of the defer keyword (`3838c79a8`).
- Intern the empty display name of synthetic provider locals (`a6a244af3`).
- Preserve namespace interfaces and cheap import diagnostics (`7236baba5`).
- CI: give scheduled coverage 120 minutes and print the runner's CPU (`8655cdecc`).
- Remove per-interface import advice and implicit fallbacks (`b5da6fcc8`).
- Cleanup: refresh resolver and parked release documentation (`f7debdd0c`).
- Update frame-section checks for recorded Wasm profiles (`63782f6a4`).
- Keep automatic cache collection from overwriting command traces (`84411fe32`).
- Merge main into parallel Cranelift delivery (`86fa28823`).
- Record first timeout signal and check cancellation groups (`0741eda33`).
- Request portable Wasm in artifact inspection tests (`7bb4d7226`).
- Consolidate native artifact build and publication paths (`868e8cb40`).
- Add pure-hd SHA-512 and its keyed HMAC constructor (`7fc8116d7`).
- Add constant-time pure-hd AES block encryption (`de0f3149b`).
- Record an index assignment that lets a local reference escape (#2389) (`77d2cf3fd`).
- Add generic GCM and CTR with allocation-free block loops (`98c656e26`).
- Cleanup: refresh native artifact design descriptions (`591f20959`).
- Trace the back end's phases: ownership bottom-up, inlining, reach, emit, link finish (`24e2fda1a`).
- Own release components a level at a time on the scheduler's threads (`af2ba591b`).
- Clean up linked functions on the scheduler's threads (`3fd861061`).
- Read frozen symbols without a lock, with no expect on a miss (`84a1766b0`).
- Answer the ownership passes' drop, copy and return-loan queries per thread (`08d34e4ea`).
- Put linked functions' count calls behind tests on the scheduler's threads (`42df8bd00`).
- Lower the sloppy-code ceilings to the merged counts (#lane302-2383b) (`303478110`).
- update deps (`54224e6ff`).
- Explain the ownership test builder's verified shape (`5a7e6feec`).
- Cleanup: record #2369's park in the ownership design (`0c40c741e`).
- Inline each instance inside the run's layout cache (`33fde1b03`).
- Build the inliner's caller analyses on first use, once per round (`63aff2009`).
- Read each tag's operand layout from a compile-time table (`d7c311fff`).
- Walk each local write's producers once per function, not per look (`1d898ed8a`).
- Skip the operand walk for tags that run no block in `for_blocks` (`6f11a5b3b`).
- Solve rule 13's liveness on dense bit rows when they fit (`78b64ff33`).
- Share one CFG between Zero and Live in a sweep of `locals` (`76a34da0d`).
- Carry rule 11's zero facts on dense bit rows when they fit (`b66ed81f6`).
- Skip the work counters' thread-local lookup when no count is taken (`b67854339`).
- Copy the runs a rule 13 round left alone in bulk (`400e8bd4b`).
- Give the dev inliner the run's layout cache (`3fb179b32`).
- Resolve the crossing items once per collection (`c225f2bdd`).
- Trace the dev pipeline's owning as release's (`ecae178fe`).
- Cover the remaining checker entries from #2390's full run (`067ef5ff6`).
- Own dev bodies through release's bottom-up walk (`3246df72f`).
- Own a dev walk's components as one level (`4bba55408`).
- Run only the passes valid code needs in dev links (`15419808e`).
- hd-lang skill: derive command-line parsers with @derive(Cli) (`ed1ec885a`).
- Revert "Run only the passes valid code needs in dev links" (`ae5dc0551`).
- Update the hd-lang skill from hd-skill a148524 (npx skills update) (`50aaa1716`).
- Run only the passes valid code needs in dev links (`ba1ba871e`).
- Cleanup: dev links run only the passes valid code needs (tiering.md) (`c38694a89`).
- Refresh the wrapper regression WAT for main's minimal dev finish (`0601f4688`).
- Update the hd-lang skill from hd-skill 043be37 (npx skills update) (`3d9160fe3`).
- Update the hd-lang skill from hd-skill 5ccaa45 (npx skills update) (`a32d6b598`).
- Keep canonical interfaces in indexed records without per-unit cache reads (`2c208b304`).
- Emit each instance against a local view of the symbol table (`97991879f`).
- Queue an instance's count shim requests in its unit, merged in order (`61c3cd344`).
- Queue an instance's stored, storage-name and frame edits in its unit (`74c6f1061`).
- Queue an instance's package host interface records in its unit (`29aed35d9`).
- Place an instance's static data in its unit, merged in request order (`a969a3456`).
- Emit instances in parallel, each on a module of its own, merged in order (`e0da6f256`).
- Lower the sloppy-code ceilings to the merged counts (#lane313-emit) (`27e467edd`).
- Read files and stdin in chunks: std.io.Read, std.fs.open!, std.console.stdin() (`638641966`).
- Add std.io.BufReader: lines with their endings kept, over any Read (`c44b54905`).
- Split the input worker's serve into a byte read and a text read (`b4702df4e`).
- Website: list spec/std/io.md among the std chapters (`6a47b2084`).
- Fix main's red from #2411: host-crate clippy casts and json.hd format (`93fac7911`).
- Skip cycle read closure when initializers call no functions (`30af69d8f`).
- Cleanup: refresh streaming and dev-pipeline status (`2804600d1`).
- update skills (`12849810f`).
- Trace the CLI harness signal senders outside its process group (`4b55ba3ae`).
- Run the link's per-function serial loops on the task's threads (`f3482dd2a`).
- Rule 13's later rounds read only the windows the round before touched (`8744354ab`).
- Share one lookup-counting cache store across the warm-run tests (`d6e2165fb`).
- Share the ownership verifier's contract-breaking test loops (`e5c6bb9ca`).
- Derive the peephole's pure operand counts from the stack-effect table (`7bd377d70`).
- Emit the list runtime's buffer copy from one function (`9e8860d96`).
- Mark Appendix D's kept and finished paths in BACKEND_NEUTRAL_TIR.md (`af0d077b1`).
- Run the Node tests in CI and fix the six they hid (#2418) (`234a82f5f`).
- Answer a File read pending at its dispose with Closed (spec, #2421) (`0845434f3`).
- Read a special File off the program thread on Node; Closed at dispose (#2414, #2421) (`3fa2c3c7e`).
- Port musl 1.2.5's libm to std.num, bit for bit at both widths (#2407) (`7321523d5`).
- Vendor hd-skill c4c36ac: the musl libm float methods (#2407) (`b9a24614e`).
- Log three hd mistakes from the #2434 and #2435 probes (`273898f41`).
- Name a hooked type's count shim by its canon digest, not its pool number (`97deba2bb`).
- Emit each instance's unit against tables of its own (`36fbc9015`).
- Store each emitted unit in dev links and merge it while its inputs hold (`72ea6ef92`).
- Key a dyn-calling unit on the methods the program's dyn calls name (`977b46b07`).
- Revert the emitted-unit cache (#2423): main's CI is red (`3167bc296`).
- Keep a vtable slot only for a method some trait-value call names (#2415, #2420) (`69030cfed`).
- Fold the final slot facts into each instance's callee_reps (#2415) (`3fa217742`).
- List the non-trait types in note_slot_user; read the reps test's program from a file (`ef0a10b7a`).
- Name a hooked type's count shim by its canon digest, not its pool number (`660fc6edf`).
- Emit each instance's unit against tables of its own (`f3c158bc7`).
- Store each emitted unit in dev links and merge it while its inputs hold (`c644e5714`).
- Key a dyn-calling unit on the methods the program's dyn calls name (`32792b239`).
- Hash each distinct body once for the unit keys (`85848311d`).
- Keep emitted units out of the tests' shared store (`3a9ce27e9`).
- Key every unit on its slot facts; drop the program-wide dyn-method key (`63667b8b8`).
- Read the slot tests' programs from files; store no unit naming a vector type (`a536cd42a`).
- Parse `pub let NAME = value`; reject `pub let mut` and the other forms (`6b71e9c72`).
- Order a module cycle in which one module has statements (`e92ace6ff`).
- Count a use through a binding root as a use of the borrow it holds (`8525bdf45`).
- Let a repeated uniqueness check stand past a retain of a value made after the first (`8cc30a74e`).
- Move a nested column's parts out of the column's own words (`16dbea00f`).
- Design native FFI: std declares C functions, hd calls them (#2388) (`ad29bc7ae`).
- Rework the FFI design: C as a provider kind for host traits (#2388) (`d2f008c10`).
- Record the FFI header and late-abort decisions (#2388) (`a37bf736b`).
- Mark the FFI design accepted (#2388) (`5da964b0f`).
- Wait for an aborted request's socket to close even after a reset (`8738d8143`).
- Ask only for the count shim an in-place count calls (`1147feab0`).
- Update the hd-lang skill: POSIX short groups (#2397) and --profile (#2406) (`8d94464df`).
- FFI design: the terminal is a package, not std (#2382, #2388) (`fbe369111`).
- Walk each borrow source once, folding the parent's in (`00f382fc1`).
- std: query terminal streams and inherit child stdio (#2382) (`dc7c0cf4b`).
- Point the skill lock at hd-skill main 03afabb (#2382 docs) (`649404f31`).
- Fix stdin worker close without closing borrowed descriptors (#2435) (`5955d81ab`).
- Bind a host trait to a C provider in [host] and check the C value table (`f54e72273`).
- Cite only spec rules that exist in the [host] table comments (`330306d91`).
- Lower a C-bound host trait's calls as C arguments and record its C table (`e0ba9f720`).
- Call a C-bound trait's JavaScript provider through its C-argument imports (`0b03070d6`).
- Keep a moved part's locals in a tuple or struct literal (`245bf3cf1`).
- json-decode: wrap the LCG step explicitly, so the dev build runs (`28797fb9a`).
- settle: check again after each wait, since a timer can fire early (`9d6225af5`).
- ci-tests: accept strace's unfinished form of the group kill (`305585499`).
- ci: reduce optimization of Windows host test binaries (`9ccd4b95a`).
- ci: preserve the dev lint profile for Windows hosts (`46d48f5bd`).
- Cover emitted-unit records and the missed core paths (`3b677568e`).
- Record what a bang call that never waits links today (Refs #2419) (`21624c74c`).
- Round-trip DynSlot's body flag in the unit wire test (`a8cea20e6`).
- Keep the frame-path cases waiting, and test shielded trait-value disposes (`cd155183e`).
- Lower the sloppy-code ceilings to the merged counts (#lane326-usefacts) (`6c5630218`).
- Read a C-bound method's import parameters with a cursor (`d75c6304b`).
- Write a package's C header from its checked C-bound traits on hd build (`0bfc8bf47`).
- Take the C tables from the artifact build's own check (`18c5d58dc`).
- State the cost of the header test's C compiler process (`f8480935b`).
- Index C-bound host entries once per run (`ab3b6ceb5`).
- Check the C table contract on the dev and the release build (`38b4a0a3a`).
- Give C number types their codes as discriminants (`f6cd4a66b`).
- Bind a package's C provider on Wasmtime: open, check, create, call, destroy (`08df14d63`).
- Give the native rules whose meaning changed new IDs, and record decision 16 (`e3d8aa094`).
- Build the C provider test library with the build's own rustc (`8d3933dcb`).
- Build C provider test libraries with an installed rustc, once at a time (`8be063122`).
- Refuse a misaligned C pointer argument (`16d521f72`).
- Return C handles from a C provider, and bind C providers in hd test (`4ce842fed`).
- Refuse C entry-symbol collisions and handle traits from two libraries (`13daf4f69`).
- Test the startup refusal of an unseen C provider conflict (`bad4cacf2`).
- Hold the test's own handle on a C test library; build it with the test kit (`47beebeab`).
- Answer a C-bound trait's library and entry from the run's index (`efc44e594`).
- Bind a C provider's suspending methods on Wasmtime: tokens, completions, aborts (`6c62c3f54`).
- Call C provider slots through Cranelift trampolines, one per signature (`3f421219a`).
- Plan every C slot's trampoline in preflight, before any create (`3275fe72d`).
- hd_cache: bound rows per publish by the merge rule, not a ratio of averages (`d78880834`).
- Add tagged hd release workflow and local version-cut command (`82fb6df9c`).
- hd_cache: skip the segment reader stress test on Windows until #2458 is fixed (`f23db5dce`).
- Preserve oversized release notes as a checksummed asset (`529654715`).
