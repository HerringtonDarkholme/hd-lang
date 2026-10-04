import type {
  HirComprehensionClause,
  HirEqualityDispatch,
  HirExpression,
  HirFunction,
  HirLocal,
  HirMatchArm,
  HirProviderContextEntry,
  HirStatement,
  HirTraitDictionaryPlan,
} from "../hir.ts";

export interface CaptureCellMapper {
  /**
   * Whether mapping `fn` could change it. Return false only when the mapped
   * function would be identical to the input; a false negative silently
   * skips conversion. Absent means every function is mapped.
   */
  shouldMap?(fn: HirFunction): boolean;
  local(local: HirLocal): HirLocal;
  expression(
    mapped: HirExpression,
    original: HirExpression,
    captureOperand: boolean,
  ): HirExpression;
  statement(mapped: HirStatement, original: HirStatement): HirStatement;
}

/** Typed capture-conversion traversal; scalar metadata is always retained unchanged. */
export function mapCapturedFunction(fn: HirFunction, mapper: CaptureCellMapper): HirFunction {
  if (mapper.shouldMap?.(fn) === false) return fn;
  const walk = captureTraversal(mapper);
  return {
    ...fn,
    parameters: fn.parameters.map(mapper.local),
    locals: fn.locals.map(mapper.local),
    captures: fn.captures.map((capture) => ({ ...capture, source: mapper.local(capture.source) })),
    body: fn.body.map(walk.statement),
  };
}

function captureTraversal(mapper: CaptureCellMapper) {
  const local = mapper.local;
  const statements = (body: readonly HirStatement[]): readonly HirStatement[] =>
    body.map(statement);
  const expressions = (values: readonly HirExpression[]): readonly HirExpression[] =>
    values.map((value) => expression(value));
  const dispatch = (value: HirEqualityDispatch): HirEqualityDispatch =>
    value.kind === "function"
      ? { ...value, ...(value.bounds ? { bounds: expressions(value.bounds) } : {}) }
      : value;
  const dictionary = (value: HirTraitDictionaryPlan): HirTraitDictionaryPlan => ({
    ...value,
    bounds: expressions(value.bounds),
    supertraits: value.supertraits.map(dictionary),
  });
  const entries = (
    values: readonly HirProviderContextEntry[],
  ): readonly HirProviderContextEntry[] =>
    values.map((entry) => {
      switch (entry.kind) {
        case "binding":
          return { ...entry, local: local(entry.local), value: expression(entry.value) };
        case "spread":
          return {
            ...entry,
            contextLocal: local(entry.contextLocal),
            value: expression(entry.value),
            providers: entry.providers.map((provider) => ({
              ...provider,
              local: local(provider.local),
            })),
          };
        default:
          return unreachable(entry);
      }
    });
  const clauses = (values: readonly HirComprehensionClause[]): readonly HirComprehensionClause[] =>
    values.map((clause) => {
      switch (clause.kind) {
        case "if":
          return { ...clause, condition: expression(clause.condition) };
        case "for":
          return {
            ...clause,
            iterable: expression(clause.iterable),
            bindings: clause.bindings.map(local),
          };
        default:
          return unreachable(clause);
      }
    });

  function statement(original: HirStatement): HirStatement {
    let mapped: HirStatement;
    switch (original.kind) {
      case "binding":
      case "assignment":
        mapped = { ...original, local: local(original.local), value: expression(original.value) };
        break;
      case "global-binding":
      case "global-assignment":
      case "discard":
        mapped = { ...original, value: expression(original.value) };
        break;
      case "return":
      case "break":
        mapped = original.value ? { ...original, value: expression(original.value) } : original;
        break;
      case "expression":
        mapped = { ...original, expression: expression(original.expression) };
        break;
      case "defer":
        mapped = { ...original, body: statements(original.body) };
        break;
      case "pass":
      case "continue":
        mapped = original;
        break;
      default:
        return unreachable(original);
    }
    return mapper.statement(mapped, original);
  }

  function expression(original: HirExpression, captureOperand = false): HirExpression {
    return mapExpression(original, captureOperand, mapper, {
      local,
      statements,
      expressions,
      dispatch,
      dictionary,
      entries,
      clauses,
      expression,
    });
  }

  return { statement, expression };
}

function mapExpression(
  original: HirExpression,
  captureOperand: boolean,
  mapper: CaptureCellMapper,
  walk: CaptureTraversalHelpers,
): HirExpression {
  const { local, statements, expressions, dispatch, dictionary, entries, clauses, expression } =
    walk;
  let mapped: HirExpression;
  switch (original.kind) {
    case "integer":
    case "float":
    case "string":
    case "character":
    case "boolean":
    case "global":
    case "capture":
    case "function-value":
    case "closure-self":
    case "trait-bound-dictionary":
    case "provider-use":
      mapped = original;
      break;
    case "local":
      mapped = { ...original, local: local(original.local) };
      break;
    case "closure":
      mapped = {
        ...original,
        captures: original.captures.map((capture) => expression(capture, true)),
      };
      break;
    case "string-build":
      mapped = { ...original, segments: expressions(original.segments) };
      break;
    case "list":
    case "tuple":
      mapped = { ...original, elements: expressions(original.elements) };
      break;
    case "list-comprehension":
      mapped = {
        ...original,
        clauses: clauses(original.clauses),
        value: expression(original.value),
      };
      break;
    case "map-comprehension":
      mapped = {
        ...original,
        clauses: clauses(original.clauses),
        key: expression(original.key),
        value: expression(original.value),
        ...(original.keyDispatch ? { keyDispatch: dispatch(original.keyDispatch) } : {}),
        ...(original.keyDictionary ? { keyDictionary: expression(original.keyDictionary) } : {}),
      };
      break;
    case "map":
      mapped = {
        ...original,
        entries: original.entries.map((entry) => ({
          ...entry,
          key: expression(entry.key),
          value: expression(entry.value),
        })),
        ...(original.keyDispatch ? { keyDispatch: dispatch(original.keyDispatch) } : {}),
        ...(original.keyDictionary ? { keyDictionary: expression(original.keyDictionary) } : {}),
      };
      break;
    case "value-equality":
    case "value-ordering":
      mapped = {
        ...original,
        left: expression(original.left),
        right: expression(original.right),
        strategy:
          original.strategy.kind === "dispatch"
            ? { ...original.strategy, dispatch: dispatch(original.strategy.dispatch) }
            : original.strategy,
      };
      break;
    case "binary":
      mapped = {
        ...original,
        left: expression(original.left),
        right: expression(original.right),
      };
      break;
    case "permission-weaken":
    case "propagate":
    case "unary":
      mapped = { ...original, operand: expression(original.operand) };
      break;
    case "binding-expression":
      mapped = {
        ...original,
        bindings: original.bindings.map(local),
        value: expression(original.value),
      };
      break;
    case "variant-wrap":
      mapped = original.payload ? { ...original, payload: expression(original.payload) } : original;
      break;
    case "cell-new":
    case "trait-bound":
    case "trait-upcast":
    case "embedded-copy":
      mapped = { ...original, value: expression(original.value) };
      break;
    case "cell-get":
      mapped = { ...original, cell: expression(original.cell) };
      break;
    case "cell-set":
      mapped = {
        ...original,
        cell: expression(original.cell),
        value: expression(original.value),
      };
      break;
    case "suspension-wrap":
    case "suspend-drive":
    case "suspension-drive":
    case "suspend-cancel":
    case "trait-suspend-drive":
    case "trait-suspend-cancel":
    case "suspension-cancel":
      mapped = { ...original, suspension: expression(original.suspension) };
      break;
    case "intrinsic-call":
      mapped = { ...original, arguments: expressions(original.arguments) };
      break;
    case "call":
    case "suspend-construct":
      mapped = {
        ...original,
        arguments: expressions(original.arguments),
        providers: expressions(original.providers),
        ...(original.bounds ? { bounds: expressions(original.bounds) } : {}),
      };
      break;
    case "trait-call":
    case "trait-suspend-construct":
      mapped = {
        ...original,
        receiver: expression(original.receiver),
        arguments: expressions(original.arguments),
        providers: expressions(original.providers),
        ...(original.bounds ? { bounds: expressions(original.bounds) } : {}),
      };
      break;
    case "closure-call":
      mapped = {
        ...original,
        callee: expression(original.callee),
        arguments: expressions(original.arguments),
        providers: expressions(original.providers),
      };
      break;
    case "trait-wrap":
      mapped = {
        ...original,
        value: expression(original.value),
        dictionary: dictionary(original.dictionary),
      };
      break;
    case "trait-dictionary":
      mapped = { ...original, dictionary: dictionary(original.dictionary) };
      break;
    case "inspect-type-id":
      mapped = { ...original, dictionary: expression(original.dictionary) };
      break;
    case "inspect-downcast":
      mapped = {
        ...original,
        value: expression(original.value),
        dictionary: expression(original.dictionary),
      };
      break;
    case "provider-pack":
      mapped = {
        ...original,
        providers: expressions(original.providers),
        bases: expressions(original.bases),
      };
      break;
    case "provider-context":
      mapped = { ...original, entries: entries(original.entries) };
      break;
    case "provider-with":
      mapped = {
        ...original,
        entries: entries(original.entries),
        body: statements(original.body),
      };
      break;
    case "data":
      mapped = {
        ...original,
        fields: expressions(original.fields),
        ...(original.spread ? { spread: expression(original.spread) } : {}),
      };
      break;
    case "enum":
      mapped = { ...original, fields: expressions(original.fields) };
      break;
    case "variant-tag":
    case "variant-payload":
    case "member":
    case "enum-member":
    case "list-length":
    case "list-iterator":
    case "iterator-next":
    case "map-iterator":
    case "tuple-index":
    case "map-length":
      mapped = { ...original, receiver: expression(original.receiver) };
      break;
    case "field-set":
    case "list-push":
      mapped = {
        ...original,
        receiver: expression(original.receiver),
        value: expression(original.value),
      };
      break;
    case "list-index":
    case "string-index":
    case "map-entry-key":
    case "map-entry-value":
      mapped = {
        ...original,
        receiver: expression(original.receiver),
        index: expression(original.index),
      };
      break;
    case "list-set":
      mapped = {
        ...original,
        receiver: expression(original.receiver),
        index: expression(original.index),
        value: expression(original.value),
      };
      break;
    case "map-index":
    case "map-remove":
      mapped = {
        ...original,
        receiver: expression(original.receiver),
        key: expression(original.key),
      };
      break;
    case "map-set":
      mapped = {
        ...original,
        receiver: expression(original.receiver),
        key: expression(original.key),
        value: expression(original.value),
      };
      break;
    case "panic":
      mapped = { ...original, message: expression(original.message) };
      break;
    case "if":
    case "for":
    case "while":
    case "match":
      mapped = mapControlExpression(original, walk);
      break;
    default:
      return unreachable(original);
  }
  return mapper.expression(mapped, original, captureOperand);
}

function mapControlExpression(
  original: Extract<HirExpression, { kind: "if" | "for" | "while" | "match" }>,
  walk: CaptureTraversalHelpers,
): HirExpression {
  const { expression, statements, local } = walk;
  let mapped: HirExpression;
  switch (original.kind) {
    case "if":
      mapped = {
        ...original,
        condition: expression(original.condition),
        thenBody: statements(original.thenBody),
        elseBody: statements(original.elseBody),
      };
      break;
    case "for":
      mapped = {
        ...original,
        iterable: expression(original.iterable),
        bindings: original.bindings.map(local),
        body: statements(original.body),
        elseBody: statements(original.elseBody),
      };
      break;
    case "while":
      mapped = {
        ...original,
        condition: expression(original.condition),
        body: statements(original.body),
        elseBody: statements(original.elseBody),
      };
      break;
    case "match":
      mapped = {
        ...original,
        subject: expression(original.subject),
        arms: original.arms.map((arm) => mapMatchArm(arm, local, expression, statements)),
      };
      break;
    default:
      return unreachable(original);
  }
  return mapped;
}

interface CaptureTraversalHelpers {
  local: CaptureCellMapper["local"];
  statements(body: readonly HirStatement[]): readonly HirStatement[];
  expressions(values: readonly HirExpression[]): readonly HirExpression[];
  dispatch(value: HirEqualityDispatch): HirEqualityDispatch;
  dictionary(value: HirTraitDictionaryPlan): HirTraitDictionaryPlan;
  entries(values: readonly HirProviderContextEntry[]): readonly HirProviderContextEntry[];
  clauses(values: readonly HirComprehensionClause[]): readonly HirComprehensionClause[];
  expression(value: HirExpression, captureOperand?: boolean): HirExpression;
}

function mapMatchArm(
  arm: HirMatchArm,
  local: CaptureCellMapper["local"],
  expression: (value: HirExpression) => HirExpression,
  statements: (body: readonly HirStatement[]) => readonly HirStatement[],
): HirMatchArm {
  return {
    ...arm,
    bindings: arm.bindings.map((binding) => ({ ...binding, local: local(binding.local) })),
    body: statements(arm.body),
    ...(arm.literal ? { literal: expression(arm.literal) } : {}),
    ...(arm.guard ? { guard: expression(arm.guard) } : {}),
    ...(arm.tests
      ? {
          tests: arm.tests.map((test) => ({
            ...test,
            ...(test.literal ? { literal: expression(test.literal) } : {}),
          })),
        }
      : {}),
  };
}

function unreachable(value: never): never {
  throw new Error(`unknown checked HIR node: ${String(value)}`);
}
