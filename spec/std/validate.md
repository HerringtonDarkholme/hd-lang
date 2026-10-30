# Validation

Status: standard library specification draft.

This chapter defines `std.validate`: value validation after a reader has
constructed a typed value. Validation observes the value without coercion or
normalization. It reports structural paths so JSON, HTTP, command-line, and
environment callers can choose their own field spellings.

## Values And Errors

| Rule | Item | Meaning |
| --- | --- | --- |
| r[std-validate.problem.decl] `Problem` | `pub struct Problem { pub code: string, pub message: string }` | one stable code and explanation |
| r[std-validate.path.decl] `PathStep` | `Field(member: Member)`, `Index(value: usize)`, `Key(name: string)` | one structural path step |
| r[std-validate.violation.decl] `Violation` | `pub struct Violation { pub path: List[PathStep], pub problem: Problem }` | one problem at one path |
| r[std-validate.error.decl] `ValidationError` | `Invalid(issues: List[Violation], truncated: bool)`, `Definition(path: List[PathStep], reason: string)` | invalid data or an invalid rule definition |

1. r[std-validate.violation.display-with] `Violation.display_with(name: fn(&PathStep) -> string)` begins with `$` and appends `name(step)` for each path step in order. A `Field` snapshots the original `Member` metadata, including its facts. The function borrows each step so a reader can render an external field name without consuming the stored path.
2. r[std-validate.error.issues] `ValidationError.issues(self) -> List[Violation]` consumes the error and returns the issues of `Invalid`, in visit order, or an empty list for `Definition`. A caller that inspects the result more than once binds that returned list once.
3. r[std-validate.error.display] `Invalid` displays one source-named `path: message` per line and, when `truncated` is true, a final limit line. `Definition` displays `invalid validation rule: ` followed by its reason.

## Validate

```text
use std.validate.{Validate, ValidationContext}

impl Validate for Account by Structure:
    name = [length(1, 80)]
    fn record(&self, context: &mut ValidationContext) -> void:
        if self.password != self.confirmation:
            context.reject("confirmation", "passwords must match")
```

1. r[std-validate.trait.decl] `std.validate` declares `pub trait Validate` with `collect`, the defaulted completed-value callback `record`, and `validated(self, max_errors: usize = 64) -> Result[Self, ValidationError]`.
2. r[std-validate.trait.dispatch] `Validate` default-defines `length_of`, `text_of`, `partial_cmp_of`, `equal_to`, `unique_items`, and `path_name` as unavailable queries. Standard implementations override the operations they support. Its default `apply_attached` reads the member's exact rule facts through the `Field` handle; the optional implementation instead reads facts typed for its payload.
3. r[std-validate.validated.same-value] `validated` returns the same value in `Ok` when no rule rejects it. It performs no conversion, normalization, or promise about a later mutation.
4. r[std-validate.validated.positive-cap] `max_errors` must be positive. Zero returns `Definition`. A positive cap retains at most that many issues and stops starting later checks once it is reached.
5. r[std-validate.validated.truncated] `Invalid.truncated` is true exactly when validation skipped a later check because the cap was already reached. A record callback that has started completes normally; an issue it reports beyond the cap is not retained.
6. r[std-validate.context.reject] `ValidationContext.reject(code, message)` reports a `Problem` at the callback's current structural path. A completed-record callback runs at that record's root.
7. r[std-validate.derive.structure] `impl Validate for T by Structure` visits the active variant's included members in declaration order. It applies their validation facts in [`std-validate.rule.builtin-order`](#r-std-validate.rule.builtin-order), recursively validates each member, and then runs that nested value's `record` callback.
8. r[std-validate.container.list] `List[T]` implements `Validate` when `T` does, visiting its elements in index order under `Index` steps.
9. r[std-validate.container.map] `Map[K, V]` implements `Validate` when its key and value types do and the key also implements `Eq & Hash`. It visits entries in map order under a `Key` step, using the key text where available and `<key>` otherwise.
10. r[std-validate.container.option] `T?` implements `Validate` when `T` does. `.Some(value)` recursively validates `value`; `.None` performs no payload check. A member fact inferred for `T` by [`annot.typed-fact.check.optional-payload`](../lang/14-annotations.md#r-annot.typed-fact.check.optional-payload) applies to the `.Some` payload and is skipped for `.None`.
11. r[std-validate.container.leaves] Strings, characters, booleans, and number types implement `Validate` as leaves.

## Rule Facts

Every built-in rule is a data-only typed fact on a field. Its target parameter
is unbounded on the fact declaration; the builder carries the semantic bound.
The `message` named argument replaces the rule's default message without
changing its code. `Check[F]` is the sole rule fact that stores executable
logic.

| Rule | Builder and fact | Rejection |
| --- | --- | --- |
| r[std-validate.rule.range] `range` | `range[F < PartialOrd](low: F, high: F, message: string? = .None) -> Range[F]` | outside the inclusive bounds; code `range` |
| r[std-validate.rule.min] `min` | `min[F < PartialOrd](bound: F, message: string? = .None) -> Min[F]` | below the inclusive bound; code `min` |
| r[std-validate.rule.max] `max` | `max[F < PartialOrd](bound: F, message: string? = .None) -> Max[F]` | above the inclusive bound; code `max` |
| r[std-validate.rule.length] `length` | `length[F < HasLength](minimum: usize, maximum: usize, message: string? = .None) -> Length[F]` | outside the inclusive length range; code `length` |
| r[std-validate.rule.min-length] `min_length` | `min_length[F < HasLength](minimum: usize, message: string? = .None) -> MinLength[F]` | shorter than the minimum; code `min-length` |
| r[std-validate.rule.max-length] `max_length` | `max_length[F < HasLength](maximum: usize, message: string? = .None) -> MaxLength[F]` | longer than the maximum; code `max-length` |
| r[std-validate.rule.byte-length] `byte_length` | `byte_length(minimum: usize, maximum: usize, message: string? = .None) -> ByteLength[string]` | UTF-8 byte length outside the inclusive range; code `byte-length` |
| r[std-validate.rule.pattern] `pattern` | `pattern(source: string, message: string? = .None) -> Pattern[string]` | the regex finds no match; code `pattern` |
| r[std-validate.rule.email] `email` | `email(message: string? = .None) -> Email[string]` | not a valid address; code `email` |
| r[std-validate.rule.url] `url` | `url(message: string? = .None) -> Url[string]` | not an accepted absolute URL; code `url` |
| r[std-validate.rule.uuid] `uuid` | `uuid(message: string? = .None) -> Uuid[string]` | not 8-4-4-4-12 hexadecimal UUID text; code `uuid` |
| r[std-validate.rule.one-of] `one_of` | `one_of[F < Eq](values: List[F], message: string? = .None) -> OneOf[F]` | unequal to every listed value; code `one-of` |
| r[std-validate.rule.unique] `unique` | `unique[F < Eq & Hash & Validate](message: string? = .None) -> Unique[List[F]]` | two list elements compare equal; code `unique` |
| r[std-validate.rule.check.capture-free] `check` | `check[F, G < 'static & Fn(&F) -> Result[void, Problem]](test: G, message: string? = .None) -> Check[F]`; stores a concrete `fn` wrapper around `test` | the callback's `Err(problem)`; its code and message, with `message` replacing only the message |

1. r[std-validate.rule.declarations] The table's fact types are structs parameterized by `F`. Each carries `@annotate::[F](.Field)`.
2. r[std-validate.rule.builtin-order] Distinct rule facts on one member run in the table's declaration order. Thus `Check[F]` runs after every built-in rule. The ordinary duplicate-fact rule rejects two facts of the same concrete fact type.
3. r[std-validate.rule.definition] A reversed range, reversed length range, or invalid regular expression returns `Definition` at the current path. The same applies when the required operation is unavailable. These failures do not reject the value.
4. r[std-validate.rule.optional] Each rule builder used on a `T?` member keeps its payload fact type. It checks `.Some` as `T` and skips `.None`, including `Check[T]`.

## Length And Text Grammars

1. r[std-validate.length.trait] `std.validate` declares `pub trait HasLength { fn length(&self) -> usize }`. Strings count Unicode scalar values, lists count elements, and maps count entries. An optional delegates to its payload and gives zero for `.None`.
2. r[std-validate.length.bytes] `byte_length` counts the UTF-8 bytes of a string, separately from scalar length.
3. r[std-validate.pattern.search] `pattern` compiles `source` with `std.regex.Regex::new` and uses `is_match`, so anchors are required for a whole-text constraint.
4. r[std-validate.email.whatwg] `email` accepts exactly the WHATWG HTML valid e-mail address grammar: an ASCII atom local part and DNS-style ASCII domain. Plus addressing and a one-label domain are accepted; quoted local parts, spaces, IP-literal domains, and IDN conversion are not.
5. r[std-validate.url.absolute] `url` accepts exactly the absolute HTTP or HTTPS URL text that `std.http.Url.parse` accepts. A relative URL is rejected.
6. r[std-validate.uuid.syntax] `uuid` accepts five hexadecimal groups of lengths 8, 4, 4, 4, and 12 separated by hyphens, case-insensitively. It does not restrict version or variant bits.

## Reader Names

1. r[std-validate.reader.names] Validation stores source `Member` identities, not JSON, CLI, form, query, or environment spellings. A caller renders those spellings with `Violation.display_with`; opaque callers use the source-name display of `ValidationError`.
2. r[std-validate.reader.boundary] Readers retain their existing decoding and parse errors. Applications call `.validated()` after a successful JSON, HTTP body, CLI, environment, form, or query read; validation errors are not decoding errors.
