# `std.validate`

Generated from `hd doc std.validate`. Run `hd doc std.validate.NAME` in a package for one item's full text.

## `PathStep`

```text
pub enum PathStep
```

One structural step in the path to an invalid value.

## `Problem`

```text
pub struct Problem
```

A stable validation code and its human-facing explanation.

## `Violation`

```text
pub struct Violation
```

One validation problem at a structural path.

- `pub fn display_with(&self, name: fn(&PathStep) -> string) -> string`: Renders this issue with names chosen by the reader that produced the value.

## `ValidationError`

```text
pub enum ValidationError
```

- `pub fn issues(self) -> List[Violation]`: The collected value violations; definition errors have none.

## `ValidationContext`

```text
pub struct ValidationContext
```

Mutable state passed to record-level validation callbacks.

- `pub fn reject(&mut self, code: string, message: string) -> void`: Records one value problem at the current path.

## `Validate`

```text
pub trait Validate
```

A value that can validate itself and its nested values.

- `fn record(&self, context: &mut ValidationContext) -> void`: Runs checks that require the completed record. It is skipped after the positive issue cap.
- `fn validated(self, max_errors: usize = 64) -> Result[Self, ValidationError]`: Returns the same value when valid, without coercion or normalization.

## `Rule`

```text
pub trait Rule[F]
```

Applies one statically dispatched rule. Concrete built-in facts contain only rule data.

## `HasLength`

```text
pub trait HasLength
```

Strings count Unicode scalars; collections count elements or entries.

## `Range`

```text
pub struct Range[F]
```

## `Min`

```text
pub struct Min[F]
```

## `Max`

```text
pub struct Max[F]
```

## `Length`

```text
pub struct Length[F]
```

## `MinLength`

```text
pub struct MinLength[F]
```

## `MaxLength`

```text
pub struct MaxLength[F]
```

## `ByteLength`

```text
pub struct ByteLength[F]
```

## `Pattern`

```text
pub struct Pattern[F]
```

## `Email`

```text
pub struct Email[F]
```

## `Url`

```text
pub struct Url[F]
```

## `Uuid`

```text
pub struct Uuid[F]
```

## `OneOf`

```text
pub struct OneOf[F]
```

## `Unique`

```text
pub struct Unique[F]
```

## `Check`

```text
pub struct Check[F]
```

## `range`

```text
pub fn range[F < PartialOrd](low: F, high: F, message: string? = .None) -> Range[F]
```

An inclusive lower and upper bound.

## `at_least`

```text
pub fn at_least[F < PartialOrd](bound: F, message: string? = .None) -> Min[F]
```

An inclusive lower bound.

## `max`

```text
pub fn max[F < PartialOrd](bound: F, message: string? = .None) -> Max[F]
```

An inclusive upper bound.

## `length`

```text
pub fn length[F < HasLength](minimum: usize, maximum: usize, message: string? = .None) -> Length[F]
```

An inclusive character or collection length range.

## `min_length`

```text
pub fn min_length[F < HasLength](minimum: usize, message: string? = .None) -> MinLength[F]
```

An inclusive minimum character or collection length.

## `max_length`

```text
pub fn max_length[F < HasLength](maximum: usize, message: string? = .None) -> MaxLength[F]
```

An inclusive maximum character or collection length.

## `byte_length`

```text
pub fn byte_length(minimum: usize, maximum: usize, message: string? = .None) -> ByteLength[string]
```

An inclusive UTF-8 byte-length range.

## `pattern`

```text
pub fn pattern(source: string, message: string? = .None) -> Pattern[string]
```

A regular expression that must match the text.

## `email`

```text
pub fn email(message: string? = .None) -> Email[string]
```

A WHATWG HTML valid email address.

## `url`

```text
pub fn url(message: string? = .None) -> Url[string]
```

An absolute HTTP(S) URL accepted by `std.http.Url.parse`.

## `uuid`

```text
pub fn uuid(message: string? = .None) -> Uuid[string]
```

A hyphenated 8-4-4-4-12 hexadecimal UUID.

## `one_of`

```text
pub fn one_of[F < Eq](values: List[F], message: string? = .None) -> OneOf[F]
```

Membership in the listed values.

## `unique`

```text
pub fn unique[F < Eq & Hash & Validate](message: string? = .None) -> Unique[List[F]]
```

No two list elements compare equal.

## `check`

```text
pub fn check[F, G < 'static & Fn(&F) -> Result[void, Problem]](
```

A custom field check; it runs after every built-in fact on the field.
