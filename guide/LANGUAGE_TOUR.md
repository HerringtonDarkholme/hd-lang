# hd-lang in Y Minutes

This is a draft language tour. It introduces hd-lang through small examples,
then briefly explains the design behind each feature. Syntax examples are
concrete enough to discuss while still allowing unsettled choices to evolve.

## Contents

1. [Hello hd-lang](#hello-hd-lang)
2. [Values and Types](#values-and-types)
3. [Control Flow and Expressions](#control-flow-and-expressions)
4. [Data Types](#data-types)
5. [Enums](#enums)
6. [Functions](#functions)
7. [Traits and Methods](#traits-and-methods)
8. [Type System](#type-system)
9. [Modules, Packages, and Use Declarations](#modules-packages-and-use-declarations)
10. [Program Entry Points](#program-entry-points)
11. [Tests](#tests)
12. [Requirements and Suspension](#requirements-and-suspension)
13. [Using Annotations](#using-annotations)
14. [Runtime and Library Features](#runtime-and-library-features)

## Hello hd-lang

An hd-lang script can be written as top-level code:

```text
# hello.hd

println("hello, hd-lang")
```

Run it as a script, a single file that may use only `std`:

```sh
hd hello.hd
```

A program with several files or dependencies is a package:
`hd new --app` creates one with a first test in `tests/`, `hd run` runs
it, and `hd test` runs that test
([Command Line](../spec/cli/command-line.md)).

The language uses indentation for structure, so blocks are introduced by a header ending in `:` followed by either an indented body or a same-line body:

```text
fn greet(name: string) -> void $ Console:
    println("hello, " + name)

fn test() -> void $ Console: println("hi")

greet("Ada")
```

Comments use `#`:

```text
# This is a line comment.
println("comments should feel familiar to Python users")
```

Top-level statements make hd-lang useful as an interactive scripting language for AI agents. The same file can later grow into typed functions, tool definitions, tests, and deployable workflows without switching to a different language model.

Scripts execute top-level statements. A file with neither top-level
statements nor `main` runs as an empty script: it does nothing and exits
with status 0. Executable packages use the explicit `main`
entry point described later in the tour. The exact CLI and standard-output API
spellings are tooling decisions rather than language syntax.

## Values and Types

hd-lang is statically typed, but local code should stay light. There are two binding forms.

Use `:=` for short bindings. The type is inferred, and the binding cannot be reassigned:

```text
name := "Ada"
age := 36
active := true

println(name)
```

`:=` is also an expression. It evaluates to the value being bound, and the inferred non-reassignable name is available in the nearest enclosing block:

```text
if (trimmed := input.trim()) != "":
    println(trimmed)
```

Use `let` for a local variable that may be reassigned. Type annotation is optional:

```text
let display_name: string = "Ada"
let nickname: string? = .None
let inferred = 1
let attempts = +0
let counter = 1

attempts = attempts + 1
counter = counter + 1
attempts = attempts + 1
```

For composite values (data, lists, maps), mutation permission is part of the type: `T` gives readonly access and `mut T` gives mutable access. Primitives such as `i32` and `bool` have no `mut` form; for them only reassignment matters, so `let mut n = 0` is an error (`mut-on-primitive`): write `let n = 0`. The binding forms fit together like this:

- `a := ...` makes a binding that cannot be reassigned, with a readonly type.
- `let a = ...` makes a reassignable binding, with a readonly type by default.
- To get a mutable type, request it after `let` (`let mut a = ...`) or write the type (`let a: mut T = ...`).

Rule of thumb: prefer `:=`, use `let` when you need to reassign, and write `mut` at most once; `let mut a: mut T` warns that one `mut` is redundant.

The warning's fix-it removes the `mut` before the name and keeps the type, because the type may be what solves a generic call. For a function `fn make[A, B]() -> (A, B)`, the annotation below both picks `A` and `B` and makes `b` mutable, with no `mut` before a name:

```text
let (a, b): (Read, mut Mut) = make()        # b: mut Mut
let (c, mut d): (Read, mut Mut) = make()    # warning: redundant-let-mut; the fix-it removes `mut` before d
```

```text
user := User { id: "user_123", email: "ada@example.com", display_name: "Ada" }   # readonly
let mut draft = User { id: "user_124", email: "bob@example.com", display_name: "Bob" }   # mut User
let names: mut List[string] = []            # the type says mut; `let mut names: List[string]` would contradict it
```

`let mut` can only request mutability the value already allows. A readonly composite reference cannot be upgraded to a mutable one, so copy it into a fresh value instead:

```text
let mut alias = user                   # error: User cannot become mut User
let mut copy = User { ...user }        # ok: a fresh value (fields declared `mut T` must be supplied fresh)
```

A mutable reference may be viewed as readonly, and that readonly alias can observe later changes made through an existing mutable alias:

```text
let mut user = User {
    id: "user_123",
    email: "ada@example.com",
    display_name: "Ada"
}

readonly := user
user.display_name = "Ada Lovelace"

println(readonly.display_name)  # "Ada Lovelace"
```

This is shared reference permission, not ownership or deep immutability. Multiple mutable aliases may exist, but mutation authority cannot be created from a readonly reference.

A plain `let` infers the readonly view, even of a fresh value, so `let draft = User { ... }` can be reassigned but not mutated. A multi-name `let` is a tuple pattern, as in `let (name, score) = pair`, and each name takes its own `mut`: `let (mut log, db) = pair`.

Types appear where they make interfaces between code clear: function parameters, return types, data fields, and public APIs.

```text
fn greeting(name: string) -> string:
    "hello, " + name
```

Primitive types include booleans, width-explicit numbers, strings, and chars:

```text
let ok: bool = true
let small: i8 = 1
let count: i32 = 42
let large: i64 = 9000
let octet: u8 = 255
let size: u64 = 1024
let ratio: f32 = 0.5
let precise: f64 = 0.5
large_float := 1e9
small_float := 1.5e-6
let name: string = "Ada"
let initial: char = 'A'
binary := 0b1010
mode := 0o755
color := 0xFF8800
million := 1_000_000
mask := 0x_FF_FF_00
```

A numeric literal may end in a suffix that a library declares, such as
`250ms`. The suffix is a function brought in with `use`, and the literal
calls it: `250ms` means `ms(250)`, a `std.time.Duration`. Only decimal and
floating-point literals take a suffix. A minus is ordinary negation, so
`-5s` is `-(5s)`, and `Duration` implements `Neg`. A `Duration` is a whole
number of milliseconds, and the standard library declares only `ms`, `s`,
`min`, and `h`:

```text
use std.time.{Duration, ms, s}

timeout := 5s
let backoff: Duration = 1_500ms
```

A library declares its own suffix by marking a function `@num_suffix`, as
in `@num_suffix fn px(count: i32) -> Pixels` after `use std.ops.num_suffix`;
see [Literal Suffixes](../spec/lang/05-expressions.md#literal-suffixes). Such a
**literal function** takes exactly one parameter. The literal is an
ordinary call, so the function may be generic or need providers, but it
must not suspend: `12px` has no place for `!`. The compiler checks the
marked function's shape where it is declared.

A name written directly before a string's quote is a string prefix, and
the string is a call of that function, a literal function marked
`@str_prefix`. Suffixes and prefixes follow the same rules. The standard prefix `r` keeps
backslashes and escape-looking text as written. The multiline form uses
three double quotes:

```text
use std.text.r

pattern := r"\d+\s+\w+"
prompt := r"""Summarize the input.
Return one paragraph."""
```

Regular triple-quoted strings also span lines and continue interpreting normal
escapes. Multiline strings preserve their source indentation and line breaks:

```text
message := """Hello,\n
This remains on the next source line."""
```

Ordinary strings use Kotlin-style interpolation. A simple name follows `$`
directly; other expressions use `${...}`. Interpolated values must implement
`std.format.Display`:

```text
use std.text.r

greeting := "Hello, $name"
summary := "User ${user.name} has ${posts.len()} posts"
price := "Cost: \$5"
raw := r"\d+ for $name"
```

Use `\$` for a literal dollar sign in an interpreted string. A prefixed
string also interpolates `$name` and `${...}`, but a `$` before anything
else is plain text. Its prefix function receives a `std.ops.Template` of the
raw text pieces and the values, so a library prefix such as `sql"..."` can
keep values apart from the text; see
[Prefixed Strings](../spec/lang/05-expressions.md#prefixed-strings).

Core types, traits, and functions such as `List`, `Map`, `Result`, `Display`,
`Ordering`, and `println` come from the prelude, so they need no import. A
declaration, type parameter, parameter, local binding, or `use` cannot bind a
prelude name to anything else. A `use` of the same declaration, such as
`use std.hash.Hash`, is allowed and changes nothing. The complete list is in the
[prelude specification](../spec/lang/10-modules.md#prelude).

There are no convenience aliases such as `int`, `uint`, or `float`. Use explicit-width numeric types. `decimal` is a standard-library type, not a primitive.

Text literals distinguish chars and strings:

```text
letter := 'A'          # char
name := "Ada"          # string
```

A string is immutable UTF-8 bytes, as in Go. `len()` counts bytes and
`s[i]` reads one byte as a `u8`, both in constant time. Lengths, indices,
and byte offsets are `usize`, an unsigned type of its own, 32 bits on
Wasm, as Rust's `usize` is: `usize(n)` and `u32(size)` convert. A string is not
iterable, so say what you walk: `chars()` yields each `char`,
`char_indices()` yields `(byte offset, char)` pairs like Go's `range`, and
`bytes()` yields each `u8`. `slice(start, end)` takes byte offsets, and so
do the positions that string methods return. An offset inside a
character, past the end, or a `start` after `end` panics with
`index-out-of-bounds`:

```text
fn first_word(text: string) -> string:
    for (offset, letter) in text.char_indices():
        if letter == ' ':
            return text.slice(0, offset)
    text

size := "héllo".len()  # 6: é is two bytes
```

A range index slices: `text[0..3]` is the first three bytes, `text[2..]`
the rest from offset 2, `text[..2]` the first two, `text[..=2]` the first
three, `text[1..=2]` the bytes at 1 and 2, and `text[..]` the whole
string. It shares bytes as `slice` does and panics on the same offsets. On
a list, `items[1..3]` is a new `mut List`, not a view, so
`let mut part = items[1..3]` can grow it; `items.view(1, 3)` is a
read-only window that panics once the list grows or shrinks. A map has no
slicing. An index or bound is unsigned, so `items[-1]` is a compile error,
and `items.len() - 1` on an empty list panics with `integer-overflow` in a debug or test build. See
[Slicing](../spec/lang/05-expressions.md#slicing) and
[Collections](../spec/std/collections.md).

A `for` loop takes the same patterns as `let`, so a loop over pairs uses
a tuple pattern, `for (offset, letter) in ...`, and a loop over points
may write `for Point { x, y } in points`. The bare
`for offset, letter in ...` is a syntax error. A pattern that may fail,
such as `.Some(v)`, is an error in a loop.

Numeric values support ordinary arithmetic operators:

```text
sum := a + b
diff := a - b
product := a * b
quotient := a / b
remainder := a % b
power := a ** b
negative := -value
```

For floating operands, `**` is IEEE 754-2019 `pow` (clause 9.2), including its
special cases, correctly rounded to the destination format. Integer and
floating operands do not mix in `**` without an explicit cast.

Integer `/` truncates toward zero. Integer overflow panics in a debug or test build and wraps in a release build, as in Rust; code that must not wrap uses the explicit `checked_*` APIs. A shift count is a `u32`, and a literal count needs no suffix; in a debug or test build it must be smaller than the shifted type's bit width at runtime unless the compiler can prove that statically, and a release build masks it.

Integer values also support bitwise operators:

```text
masked := flags & mask
combined := read | write
toggled := flags ^ verbose
inverted := ~mask
left := value << 2
right := value >> 1
```

Operator precedence follows a Python-like shape, from highest to lowest:

| Operators | Notes |
| --- | --- |
| `(expr)`, literals, list/map/data displays | atoms |
| `x.y`, `x[i]`, `x(args)`, `x!(args)`, `x?` | postfix operations, left to right |
| `**` | exponentiation, right-associative |
| `+x`, `-x`, `~x`, `!x` | unary operators; `!` is logical not |
| `*`, `/`, `%` | multiplicative |
| `+`, `-` | additive |
| `<<`, `>>` | shifts |
| `&` | bitwise and |
| `^` | bitwise xor |
| `\|` | bitwise or |
| `==`, `!=`, `<`, `<=`, `>`, `>=`, `is` | comparisons; no chaining |
| `&&` | logical and |
| `\|\|` | logical or |
| `if`, `match`, `for ... else`, `while ... else` | value-producing control flow |
| `fn(...) -> ...:` | closure expression |
| `:=` | binding expression, lowest precedence |

`==` uses `Eq`, while ordering uses `PartialOrd`; `Ord` expresses the
stronger total-order contract. Floats implement `Eq` with IEEE 754 semantics,
so NaN is unequal to itself, and `PartialOrd` but not `Ord`. Data and enums do not
gain equality automatically: implement the trait or request explicit
derivation. `is` checks whether two composite references point to the same
object, independently of their values. `!(a is b)` checks distinct identity.

> **Note.** `is` is identity, as in Python, not a type test as in Kotlin or
> Dart. To test a type, call `downcast` on an `Inspectable` value.

```text
@derive(Eq, PartialOrd, Ord, Hash)
data User:
    id: i64
    name: string
```

`@derive` is a compiler intrinsic: it generates ordinary trait
implementations from the data or enum shape. `@derive(Debug)` also works,
through the `Debug` trait's template; `debug(value)` then renders the value
as stable, multi-line text for tests and diagnostics, apart from the
user-facing `Display`. Derived equality compares every
declared field, including embedded fields; enum equality also distinguishes
variants. It does not detect reference cycles, so comparison may exhaust the
stack when it repeatedly traverses one.
Derived ordering compares data fields in declaration order. Enum variants
compare by declaration order before their shared data and payload fields.
Derived `Hash` hashes every declared data field, or the enum variant identity
followed by its shared data and payload fields. Each such field needs `Hash`;
`Eq & Hash` lets a user-defined type serve as a map key. This holds for a
payload-free enum too: it gets no automatic `Eq` or `Hash`, although `is`
already compares its canonical variants.
Deriving `Hash`, `PartialOrd`, or `Ord` needs `Eq` (and `Ord` also
`PartialOrd`) in the same `@derive` list; mixing a derived trait with a
hand-written partner is `mixed-derived-law`. A newtype may also derive, as in
`@derive(Eq, Hash)` before `type Mile(i32)`, reusing the base type's
implementations.

Library traits derive the same way when their library declares a template,
`impl[T] Encode for T by Structure:`, over the compiler-generated
`std.structure.Structure` view of a type's members. Other decorators attach
facts that the template reads, and a derivation block configures one
derivation with member lines. A module that writes `by Structure` imports
`std.structure.Structure`; `@derive` alone needs no import:

```text
use std.structure.Structure

@derive(Encode)
@style(prefix="user_")
data User:
    id: i64
    @rename("mail")
    email: string

impl Encode for Order by Structure:
    total_cents = [rename("total")]
    cache = pass
```

`cache = pass` leaves a member out, and `build` fills it from its default.
Shared metadata that every derivation sees can also be written away from the
declaration, in a trait-less block such as `impl User by Structure:` with the
same member lines; see [Using Annotations](#using-annotations).
Facts and member metadata are `List[Any]` values evaluated once at compile
time, so they must be requirement-free and may not reach `block_on`.
`@derive` before a function, trait, or implementation is an error. A
newtype has no derivation block: it derives only through its base type.
See [Typed Derivation](../spec/lang/14-annotations.md#typed-derivation).

`Error` is not in `@derive`'s list. An error type uses the separate
`@error` intrinsic, Rust's `thiserror` in hd. It generates `Display`,
`std.error.Error` with `cause`, and one `From` per `@from` member:

```text
@error
enum LoadError:
    @error("cannot read $path")
    Read(path: string, @source error: FsError)
    @error("bad config at line $_0")
    Syntax(i64)
    @error(transparent)
    Fs(@from error: FsError)
    Cancelled

@error("config $name is missing")
data MissingConfig:
    name: string
```

A message is an interpolated string over the variant's members, with
unnamed ones as `_0`, `_1`; a variant without one displays as its name.
`@source` marks the cause, `@from` also generates the conversion that `?`
uses, and `@error(transparent)` forwards both to the one payload. Writing
`Display` or `Error` by hand beside `@error` is `overlapping-impl`, and a
form before the wrong target, such as `@error` before a function, is
`decorator-target-kind`. `@error` needs no `use std.error.Error`; only
code that names `Error` imports it. See
[Error Derivation](../spec/lang/14-annotations.md#error-derivation).

Use parentheses when a binding expression appears inside a larger expression.
`:=` binds exactly one name; destructuring is a `let` statement, so
`(a, b) := value` is `missing-let`, and `((a, b) := value)` is a syntax
error. A list item or a tuple element may be a binding, so
`[a, b := value]` and `(a, b := value)` each hold `a` and `b := value`.

`void` is used for functions that return no useful value. It is another
name for the empty tuple type `()`, whose one value is `()`:

```text
fn log_start() -> void $ Console:
    println("start")
```

Tuples are built in for lightweight grouped values:

```text
point := (10, 20)                 # (usize, usize)
entry := ("Ada", 36, true)        # (string, usize, bool)
single := (1,)                    # (usize,)
empty := ()                       # empty tuple
```

Tuple fields are read as `_` followed by the zero-based index:

```text
x := point._0
y := point._1
```

Tuple destructuring is a `let` tuple pattern. `(x, y) := point` is
`missing-let`, since `:=` binds one name:

```text
let (x, y) = point
let (name, score) = ("Ada", 10)
```

`let` takes any `match` pattern. A pattern that may fail, such as
`.Some(user)`, needs a let-else block that leaves the function or loop.
A pattern that always matches takes no `else`; adding one is an error:

```text
let Point { x, y: py } = point
let .Some(user) = find(id) else: return .None
```

Use data types instead of named tuples when field names are part of the meaning.

Optional values use Swift-style `?`. A plain `string` must contain a string. A `string?` may be absent, written `.None`.

```text
data Profile:
    display_name: string
    nickname: string?
```

Optional values must be handled before they are used as non-optional values. Like Rust, `?` propagates absence from a function that also returns an optional:

```text
fn label(name: string?) -> string?:
    actual := name?
    "user: " + actual
```

`?` also propagates `Result` errors from a function that returns a compatible `Result[T, E]`.

> **Note.** `a?.b` is propagation followed by member access: when `a` is
> `.None`, the enclosing function returns `.None`. It is not Kotlin's or
> Swift's optional chaining, which would yield `.None` for the expression
> and carry on.

Collections are typed:

```text
names := ["Ada", "Grace", "Linus"]       # List[string]
scores := {"Ada": 10, "Grace": 12}       # Map[string, usize]
```

List comprehensions build lists from iterables:

```text
long_names := [for name in names if name.len() > 3 => name]
name_lengths := [for name in names => name.len()]
pairs := [for x in xs for y in ys => (x, y)]
labels := [for user in users if (label := user.name.trim().lower()) != "" => label]
```

Comprehensions put generator and filter clauses before `=>`, and the produced value after `=>`. Multiple `for` clauses run left to right. Use the `:=` binding expression to name intermediate values inside guards or result expressions; there is no separate `let` clause in comprehensions.

A later clause can use names introduced by earlier clauses. An `if` filters at the point where it appears:

```text
early_filter := [for x in xs if x.active for item in x.items => item]
pair_filter := [for x in xs for y in ys if x.id == y.owner_id => (x, y)]
```

The first form filters `x` before entering the inner `item` loop. The second form filters after both `x` and `y` exist. Later clauses are not visible to earlier clauses.

Map comprehensions build maps from key/value expressions:

```text
scores_by_name := {for user in users => user.name: user.score}
active_by_id := {for user in users if user.active => user.id: user}
```

If a map comprehension produces the same key more than once, the later value wins.

A comprehension follows the loops it abbreviates. In a suspending body it may make `!` calls, which run one at a time, in order; elsewhere a `!` call is `bang-call-outside-suspension`, as in a loop.

Use `:=` for ordinary readonly local values and `let` for variables that may be reassigned. Composite mutation permission is written in the type as `mut T`; explicit types remain important at boundaries that humans, tools, and AI agents need to review.

## Control Flow and Expressions

hd-lang makes value-producing code easy to read. Literals, calls, field access, arithmetic, closures, `if`, `match`, and `:=` bindings are expressions. `let` declarations and assignment are statements.

Block headers such as `fn`, `if`, `else if`, `else`, `for`, `while`, and `match` end with `:`. Blocks can evaluate to their last expression. This is why functions can return without an explicit `return`:

```text
fn score_label(score: i32) -> string:
    if score >= 90:
        "excellent"
    else if score >= 70:
        "passing"
    else:
        "needs work"
```

When `if` is used as an expression, every reachable branch must produce a
value of one compatible result type, and an `else` branch is required:

```text
label := if active:
    "active"
else:
    "inactive"
```

`else if` is one direct conditional-chain form, not a nested `else:` block containing a separate `if`.

Use `return` for early exits:

```text
fn find_name(names: List[string], prefix: string) -> string?:
    for name in names:
        if name.starts_with(prefix):
            return name
    .None
```

A range is a value: `0..n` holds `0` up to, not including, `n`; `1..=3`
holds `1` through `3`; and `5..` has no end. `for` iterates them, so
`for i in 0..` loops until a `break`. Bounds must be integers. The types
are `Range`, `RangeFrom`, `RangeTo`, and `RangeFull` in `std.ops`: `0..n`
and `1..=3` are both a `Range`, with an `inclusive` field, as `..n` and
`..=n` are both a `RangeTo`. `for` rejects `..n`, `..=n`, and `..`, which
have no start. A `match` on an integer takes `a..b`, `a..=b`, `a..`,
`..=b`, and `..b` as patterns, and arms that cover the whole type need no `_`. See
[Range Expressions](../spec/lang/05-expressions.md#range-expressions) and
[Range Patterns](../spec/lang/06-control-flow.md#range-patterns):

```text
fn sum_below(n: i32) -> i32:
    let total = 0
    for i in 0..n:
        total = total + i
    total

fn sign(n: i8) -> i32:
    match n:
        ..=-1 => -1
        0 => 0
        1.. => 1
```

Loops can be used for control flow. `break` exits a loop, and `continue` skips to the next iteration:

```text
let total = +0

for value in values:
    if value < 0:
        continue
    if value > 100:
        break
    total = total + value
```

`for` separates an ordinary iterable source from the iterator that tracks one traversal. An ordinary `Iterable` contains no per-traversal cursor or progress. Each call to its `iter()` creates an independent mutable `Iterator`, whose `next()` operation advances only that iterator. This is why nested traversal over the same source has independent progress:

```text
for left in values:
    for right in values:
        println("$left, $right")
```

The iterable may still contain data and may itself refer to mutable data; "stateless" here means only that traversal progress is not stored in an ordinary iterable source. `for` also accepts a mutable `Iterator` directly. The loop does not clone or reset it: iteration continues from the cursor's current position and leaves it exhausted when completed. Once `next` has returned `.None`, what a later `next` returns depends on the iterator, as in Rust, so stop at the first `.None`. A loop cannot advance a readonly iterator, and nothing else can: `Iterator` has no `iter()`, and `next` needs mutable access. An iterator is single-pass, with no `clone`: to traverse the items twice, call `iter()` on the collection again, or `collect()` the iterator first. Built-in list and map iterators are invalidated by insertion, removal, clearing, or another shape change, and their next advance panics. Replacing an existing element or value without changing collection shape does not invalidate the iterator.

`Iterator[T]` is one concrete type, a small data value that holds a private `step` closure, not a trait. `List` and `Map` implement the `Iterable` trait that `for` uses. `Iterator` does not, so a generic `I < Iterable[T]` parameter takes a list or a map, not an iterator. To make your own iterator, pass a closure that returns the next item, or `.None`, to `Iterator::from_fn`. It calls the closure on every `next`, even after a `.None`:

```text
fn countdown(start: i32) -> mut Iterator[i32]:
    let left: i32 = start
    Iterator::from_fn(fn() -> i32?:
        left = left - 1
        if left < 0: .None else: left + 1
    )
```

 Every iterator has adapter methods, as Rust's iterators do. `filter`, `take`, `enumerate`, and `map` wrap it in a new iterator that advances only when read; `fold` and `collect` drain it:

```text
fn first_evens(values: List[i32]) -> List[(usize, i32)]:
    values.iter().filter(fn(value): value % 2 == 0).enumerate().take(2).collect()

fn total_length(names: List[string]) -> usize:
    names.iter().map(fn(name): name.len()).fold(0, fn(sum, size): sum + size)
```

`collect` builds whatever the expected type names, and a `List` when nothing names one. Pairs collect into a `Map`, where the last value of a repeated key wins, and `Result` items collect all-or-nothing, stopping at the first error:

```text
fn index(names: List[string]) -> Map[string, usize]:
    names.iter().map(fn(name): (name, name.len())).collect()

fn parse_all(lines: List[string]) -> Result[List[i32], ParseError]:
    lines.iter().map(parse_port).collect()
```

A comprehension may use `?` too: `.Ok([for line in lines => parse_port(line)?])` returns the first error from the function, and the comprehension stops there.

Use `while` when the loop condition is not just iterating a collection:

```text
let index: usize = 0

while index < names.len():
    println(names[index])
    index = index + 1
```

`for` and `while` can also have `else` blocks. When a loop has an `else`, the loop becomes a value-producing expression: `break value` produces the loop value when the loop exits early, and the value of the `else` suite produces the loop value when the loop finishes normally.

```text
first_large := for value in values:
    if value > 100:
        break value
else:
    .None
```

The same rule applies to `while`:

```text
found := while index < names.len():
    if names[index].starts_with(prefix):
        break names[index]
    index = index + 1
else:
    .None
```

Without an `else` block, a loop evaluates to `void`, even if it contains plain `break`. `break value` is only valid in a value-producing loop with an `else`; use plain `break` in statement-only loops.

`match` is also an expression. A match must be exhaustive; a final `_` arm
covers any remaining values:

```text
message := match status:
    JobStatus.Queued => "waiting"
    JobStatus.Running => "working"
    JobStatus.Succeeded => "done"
    JobStatus.Failed => "failed"
```

`pass` is the no-op expression and evaluates to `void`. It is useful when syntax requires a body but no operation is needed, as in `data Empty: pass`.

### Pipes

The pipe `value |> step` feeds a value into the next step, so nested calls read left to right. A step either marks the value's slot with `_`, or is a bare function name, which is called with the value:

```text
fn tag(label: string, level: i32) -> string:
    "$label:$level"

fn clean(raw: string) -> string:
    raw.trim().lower()

fn label(raw: string) -> string:
    raw
        |> clean
        |> tag(_, 2)
        |> _.len() |> tag("size", _)
```

`raw |> clean` means `clean(raw)`, and `x |> tag(_, 2)` means `tag(x, 2)`. A bare step may be a method of a value: `x |> user.greet` means `user.greet(x)`. The value is always evaluated first. A step has exactly one `_`, and any expression may hold it: `_.len()`, `_ * 2`, or `Point { x: _, y: 0 }`.

Three things are errors, so a step never hides where the value goes:

- a call without `_`, such as `x |> tag(2)`. It does not mean `tag(x, 2)` or `tag(2)(x)`; write `x |> tag(_, 2)`;
- a bare step that suspends, such as `id |> fetch`; write `id |> fetch!(_)`;
- `_` anywhere outside a pipe step. `tag(_, 2)` is not a shorthand for a function; write a closure, `fn(x): tag(x, 2)`.

A pipe inside a step has its own `_`: in `x |> tag(_, y |> clean(_))`, the second `_` is `y`. A step fits on one line. A chain may continue on lines that start with `|>`, but not on lines that start with `.` once the first `|>` has appeared: write `|> _.len()` instead. A `.` line before the first `|>` is still part of the piped value.

### Deferred cleanup

Use `defer:` for synchronous cleanup at the end of the innermost executing
block. The suite is registered when execution reaches it, and multiple suites
run in reverse registration order. Cleanup also runs on `return`, `break`,
`continue`, `?` propagation, and cancellation, but not during a runtime panic.

```text
fn read_first!(path: string) -> Result[string, ResourceError[FileError]] $ Files:
    let handle: mut FileHandle = $.use(Files).open!(path)?
    defer:
        _ := handle.close()
    handle.read!()
```

The `_ :=` makes the deliberate discard of the close result visible. A defer
suite must produce `void` and cannot suspend, call `block_on`, propagate with
`?`, or use `return`, `break`, or `continue`. A return value or propagated value
is evaluated before cleanup runs. `defer` is block-scoped cleanup, not an
ownership system: another alias to the handle may still escape.

## Data Types

hd-lang has data types, not classes. The `data` declaration defines a nominal
product type with named, typed fields. Its composite values use shared references,
not value-type copies:

```text
data User:
    id: string
    email: string
    display_name: string? = .None
```

Construct a data value with a typed literal:

```text
user := User {
    id: "user_123",
    email: "ada@example.com"
}
```

A field may be written as its bare name when a binding of that name is in
scope, as in a data pattern: `User { id, email }` means
`User { id: id, email: email }`, and the two forms mix, as in
`User { id, email: "ada@example.com" }`.

Omitted fields use their declared defaults. Defaults must be pure and are
evaluated for each construction after explicit field expressions; a
copy-update spread supplies every field and skips defaults.

Field access uses dot syntax:

```text
println(user.email)
```

Data types can also be matched by field. Unlisted fields are ignored;
`field: pattern` can rename a binding or test a nested value:

```text
fn email_of(user: User) -> string:
    match user:
        User { email: address } => address
```

Composite fields may store either readonly or mutable references. Mutation through a path requires a mutable root and `mut` on every composite reference edge crossed by that path:

```text
data Profile:
    display_name: string

data Account:
    profile: mut Profile

let mut profile = Profile {
    display_name: "Ada"
}

account := Account { profile: profile }
account.profile.display_name = "Ada Lovelace"  # error: readonly root

let mut editable = Account { profile: profile }
editable.profile.display_name = "Ada Lovelace"  # mutable root + mutable edge
```

An ordinary composite field is a readonly edge. A `mut` outer object may replace
any field, even a readonly-typed one, but cannot mutate a referenced child through
a readonly edge. A readonly outer view cannot replace fields. It reads a direct
`friend: mut User` field as `User`, so it cannot mutate that child or call its
`mut self` methods. A readonly outer `User` may be constructed with a readonly
value in that direct field; constructing `mut User` requires a mutable value.
A generic field `value: T` instead retains its substituted type, including
`mut Child` when `T = mut Child`. Other mutable aliases may still change the
underlying object.
`mut` belongs in a named field's type (`friend: mut User`), never before its
name (`mut friend: User`). Embedded members never carry `mut`: write `Base`,
not `mut Base`, because access to an embedded part follows its container.

Mutable parameter permission is written in the type position:

```text
fn inspect(user: User) -> string:
    user.display_name

fn invalid(user: User) -> void:
    user.display_name = "new"  # error: User is readonly

fn normalize_user(user: mut User) -> User:
    user.email = user.email.trim().lower()
    user
```

The same rule composes through containers:

```text
fn edit_users(users: mut List[mut User]) -> void:
    users[0].display_name = "new"
```

Here the list is a mutable root and its element references are mutable edges.
`mut List[User]` can replace list elements but cannot mutate the referenced
users; `List[mut User]` cannot replace list elements but can mutate its
referenced users, because indexing retains the generic element type.

The two rules also compose through a readonly containing object:

```text
data Cart:
    items: mut List[mut LineItem]

fn inspect_cart(cart: Cart, item: mut LineItem) -> void:
    cart.items[0].quantity = 0  # allowed: element remains mut LineItem
    cart.items.push(item)     # error: readonly Cart weakens the direct list field
```

Iterating `List[mut T]` likewise yields `mut T`, even through a readonly list.

Container and element permissions are independent, so all four forms are meaningful: `List[User]`, `List[mut User]`, `mut List[User]`, and `mut List[mut User]`.

The same holds for a list of lists: the inner lists are readonly unless the
type says `mut` for them too. To push onto an inner list, write
`mut List[mut List[T]]`:

```text
fn bucket(words: List[string]) -> mut List[mut List[string]]:
    let out: mut List[mut List[string]] = [[], []]
    for word in words:
        out[word.len() % 2].push(word)  # needs the inner mut
    out

fn stuck(out: mut List[List[string]]) -> void:
    out.push(["new"])      # allowed: the outer list is mutable
    out[0].push("word")    # error: the inner List[string] is readonly
```

A readonly list view may weaken element permission because `List` declares its element parameter as covariant, conceptually `List[+T]`: `List[mut User]` can be used as `List[User]`. Mutable list views are invariant, so `mut List[mut User]` cannot become `mut List[User]`; that mutable view could insert a readonly `User` into storage requiring `mut User`.

Maps follow the same separation: a readonly `Map[K, mut User]` can yield
`mut User` from lookup or iteration, but replacing an entry requires a
`mut Map[K, mut User]`. `users[id]` returns `mut User` and panics with
`index-out-of-bounds` when the key is missing; `users.get(id)` returns
`mut User?` for a key that may be absent.
The same generic-content rule applies to `Result[mut User, E]`: propagating a
successful result with `?` yields `mut User`, not a weakened reference.

Use copy-update syntax when creating a modified value from an existing data value:

```text
renamed := User {
    ...user,
    display_name: "Ada Lovelace"
}
```

Copy-update is shallow. It creates a new outer data value, copies primitive fields
by value, and reuses composite field references unless an explicit replacement
provides a different value. Copied fields are read through the source view:
a readonly source exposes a direct `mut`-typed child as readonly. It can fill
that field in a readonly copy; a mutable copy requires an explicit mutable
replacement. Generic fields retain their substituted types in either view.

Data types can contain other data types:

```text
data Profile:
    avatar_url: string?
    bio: string?

data Account:
    user: User
    profile: Profile
```

Data embedding supports Go-style composition without inheritance. A bare type-name line embeds that data:

```text
data Timestamps:
    pub created_at: i64
    pub updated_at: i64

data Post:
    Timestamps
    id: string
    title: string
    author_id: string
```

Embedded data types are initialized with the embedded type name as the field key, followed by `...`, which copies the value into the part (see below). Their `pub` fields and `pub` methods are promoted for ordinary access:

```text
post := Post {
    Timestamps: ...Timestamps {
        created_at: 1700000000,
        updated_at: 1700000000
    },
    id: "post_123",
    title: "Hello",
    author_id: "user_123"
}

println(post.created_at)
```

An embedded field holds the outer value's own copy of the part. A part copy
is a copy-update: `Timestamps: ...stamps` stores `Timestamps { ...stamps }`,
so its ordinary fields are copied shallowly and its embedded parts in turn,
and the outer value never shares a part with the value it was built from.
Copy-update and assignment to an embedded field copy the same way; the
assignment is written `post.Timestamps ...= stamps`, and plain `Timestamps: stamps` or `post.Timestamps = stamps` is an error that
suggests the `...` form. A prefix `...` always means "copy the named members
of this value", while a suffix `...`, as in `f(xs...)` or `$.with(ctx...)`,
always spreads. Access to the part follows the container: through a `mut` outer
value the part, its promoted fields, and its promoted `mut self` methods are
mutable; through a readonly one they are readonly. Reading the part out does
not copy it:

```text
impl Timestamps:
    pub fn touch(mut self, at: i64) -> void:
        self.updated_at = at

let mut draft = Post { Timestamps: ...post.Timestamps, id: "p2", title: "Draft", author_id: "user_123" }
draft.touch(1700000100)          # changes draft's own copy, never post
let mut stamps = draft.Timestamps  # mut Timestamps: the same part as draft's
```

A readonly value may fill an embedded field. Its copy follows the
copy-update rules: it is mutable unless the part's type has a direct `mut`
field somewhere in it; then the copy, and the value built from it, is
readonly.

Generic data types can be embedded with type arguments. The construction key
is the type's final name, without arguments:

```text
data Box[T]:
    pub value: T

data Shipment[T]:
    Box[T]
    id: string

shipment := Shipment::[i32] {
    Box: ...Box::[i32] { value: 5 },
    id: "shipment_1",
}
shipment.value
```

Two embedded types with the same final name are rejected, even when their
type arguments differ.

Fields and methods are separate namespaces: `x.name` finds a field and `x.name(args)` finds a method, so a field and a method may share a name, and a function stored in a field is called as `(x.callback)(args)`. Embedding promotes the `pub` fields and `pub` inherent methods of every embedded part, at any depth, and for each name the shallowest member wins: the outer type's own members come first, and each embedded type decides its own names before its members are promoted further. As in Rust, a trait method counts only where its trait is in scope, wherever the impl is written; a trait that is not imported is invisible, and a call that finds nothing suggests the import. A trait method of the outer type never silently wins over a promoted method, or the reverse: when both exist, the call is ambiguous and is written `Trait::method(x)` or through the embedded field. An embedded type contributes its `pub` fields and inherent methods, never its trait methods, which are called through the embedded field, as in `page.Label.to_string()`. A private member of an embedded type is never promoted, even in its own module; reach it through the path, as in `post.Timestamps.secret`. A type has one view of its members, so a name means the same member in every module: a private member of the outer type is reported as `private-member` outside its module, and it may not share a name with a promoted member, because a private member never shadows one.

If two embedded data types promote the same name at the same depth, the outer declaration is rejected, even before anything uses the name. The same holds for one type embedded twice at the same depth, whose embedded field name itself clashes. A `pub` own member of that name hides both, and each part stays reachable through its embedded field. A private own member of that name is rejected instead:

```text
data CreatedBySystem:
    pub id: string

data CreatedByUser:
    pub id: string

data AuditRecord:
    CreatedBySystem
    CreatedByUser                # invalid: both promote `id` at depth 1

data AuditEntry:
    CreatedBySystem
    CreatedByUser
    pub id: string               # ok: the pub own field hides both

data AuditDraft:
    CreatedBySystem
    id: string                   # invalid: a private field cannot shadow a promoted one; pick another name

entry.id                         # the own field
entry.CreatedByUser.id           # the part's field
```

## Enums

Data types model product types: one value contains all listed fields. Enums model sum types: one value is exactly one of several variants.

Use `enum` for closed sets of variants:

```text
enum JobStatus:
    Queued
    Running
    Succeeded
    Failed
```

Construct a variant with the enum name and variant name:

```text
status := JobStatus.Queued
```

Variants can carry named payload fields:

```text
enum ToolError:
    NotFound(resource: string)
    Unauthorized(reason: string)
    RateLimited(retry_after_ms: i32)
    Internal(message: string)
```

Payload variant declarations use the compact `Variant(field: type)` form. Use a separate data type when the payload is large enough to need a full field block.

Payload variants are called like functions. Positional arguments come first, and
named arguments follow them:

```text
error := ToolError.NotFound(resource="user_123")
```

Use `match` to inspect an enum. A match must be exhaustive; a final `_` arm
covers any remaining values:

```text
fn status_label(status: JobStatus) -> string:
    match status:
        JobStatus.Queued => "queued"
        JobStatus.Running => "running"
        JobStatus.Succeeded => "succeeded"
        JobStatus.Failed => "failed"
```

An arm may use `if` to guard a pattern. Pattern bindings are visible in the guard;
when it is false, matching continues. Guarded arms do not count toward
exhaustiveness, so a later arm must cover the remaining values:

```text
fn queue_label(status: JobStatus, urgent: bool) -> string:
    match status:
        JobStatus.Queued if urgent => "urgent"
        JobStatus.Queued => "queued"
        JobStatus.Running => "running"
        JobStatus.Succeeded => "succeeded"
        JobStatus.Failed => "failed"
```

Enum variants can use `.Variant` where the expected type is known, including a typed binding, a return type, or a `match` subject. Without that expected type, use the qualified form such as `JobStatus.Queued`; the compiler does not guess an enum from a variant name. Enum variants cannot be named as items in a use declaration.

Payload fields can be bound in a match arm:

```text
fn error_message(error: ToolError) -> string:
    match error:
        ToolError.NotFound(resource) => "not found: " + resource
        ToolError.Unauthorized(reason) => "unauthorized: " + reason
        ToolError.RateLimited(retry_after_ms) => "rate limited, retry after " + retry_after_ms.to_string() + "ms"
        ToolError.Internal(message) => message
```

Enum variants can also be used for domain states that should be impossible to confuse:

```text
enum PaymentState:
    Draft
    Authorized(id: string)
    Captured(id: string, amount: i64)
    Refunded(id: string, amount: i64)
```

An enum can declare constructor parameters for data shared by every variant. Variants can call the enum constructor in their result expression:

```text
enum StatusCode(i32):
    Ok -> StatusCode(200)
    NotFound -> StatusCode(404)
    InternalError -> StatusCode(500)
```

Enum constructor definitions and calls follow the same parameter conventions as functions. In definitions, unnamed positional parameters come before named parameters. In calls, positional arguments come first, and named arguments must come after positional arguments.

```text
enum HttpStatus(code: i32, phrase: string, retryable: bool = false):
    Ok -> HttpStatus(200, phrase="OK")
    NotFound -> HttpStatus(404, phrase="Not Found")
    ServiceUnavailable -> HttpStatus(503, phrase="Service Unavailable", retryable=true)
```

Shared data belongs to the variant, like a Java enum constant's constructor
arguments: each variant's `->` expression is evaluated once, at compile time,
and cannot use the variant's payload. Shared enum constructor parameters may
have pure defaults, which follow function-parameter ordering. Variant payload
parameters remain required.

Shared named constructor data is available on every enum value as a
read-only field, such as `HttpStatus.NotFound.phrase`. Unnamed shared data
uses a zero-based tuple-style member, such as `StatusCode.NotFound._0`.
Variant-specific payloads remain available through pattern matching. An enum
value never changes once built.

Enums can be generic algebraic data types:

```text
enum Maybe[T]:
    Some(value: T)
    None
```

Enums can also be recursive:

```text
enum Tree[T]:
    Leaf(value: T)
    Branch(left: Tree[T], right: Tree[T])
```

For more precise modeling, a GADT variant can declare an explicit result type so pattern matching can recover more specific type information:

```text
enum Expr[T]:
    IntLit(value: i64) -> Expr[i64]
    BoolLit(value: bool) -> Expr[bool]
    Add(left: Expr[i64], right: Expr[i64]) -> Expr[i64]
    Sub(left: Expr[i64], right: Expr[i64]) -> Expr[i64]
    Scale(value: Expr[i64], factor: i64) -> Expr[i64]
    If[T](cond: Expr[bool], then_value: Expr[T], else_value: Expr[T]) -> Expr[T]
```

A GADT-style variant can also refine the enum type while passing data to an enum-level constructor:

```text
enum Box[T](contents: T):
    IntBox(n: i64) -> Box[i64](0)
    BoolBox(b: bool) -> Box[bool](false)
```

When a variant omits an explicit result type, it returns the enclosing enum with the enum's type arguments. When matching a GADT-style enum, the matched variant refines the enum type parameter inside that arm:

```text
fn eval[T](expr: Expr[T]) -> T:
    match expr:
        Expr.IntLit(value) => value
        Expr.BoolLit(value) => value
        Expr.Add(left, right) => eval(left) + eval(right)
        Expr.Sub(left, right) => eval(left) - eval(right)
        Expr.Scale(value, factor) => eval(value) * factor
        Expr.If(cond, then_value, else_value) =>
            if eval(cond):
                eval(then_value)
            else:
                eval(else_value)
```

In the `Expr.IntLit` arm, `T` is known to be `i64`, so returning `value: i64` is valid. In the `Expr.BoolLit` arm, `T` is known to be `bool`. The compiler uses those refinements for arm-local type checking and still checks that the whole `match` returns the function's declared `T`.

Enum payload patterns follow the same positional/named convention as function calls and enum constructor calls. Positional patterns come first. Only `field=pattern` counts as a named pattern, and named patterns come after positional patterns. Bare identifiers bind new names, while literals match exact values:

```text
fn eval_i64(expr: Expr[i64]) -> i64:
    match expr:
        Expr.Add(l, r) => eval_i64(l) + eval_i64(r)
        Expr.Sub(left=l, right=r) => eval_i64(l) - eval_i64(r)
        Expr.Scale(value, factor=2) => eval_i64(value) * 2
        Expr.Scale(value, factor) => eval_i64(value) * factor
        Expr.IntLit(value) => value
        Expr.If(cond, then_value, else_value) =>
            if eval(cond): eval_i64(then_value) else: eval_i64(else_value)
```

In `Expr.Add(l, r)`, `l` and `r` are positional patterns that bind new names; they do not need to match the payload field names `left` and `right`. In `Expr.Sub(left=l, right=r)`, `left=` and `right=` select payload fields by name, while `l` and `r` are still new binding patterns. `Expr.Scale(value, factor=2)` matches only a scale expression whose `factor` payload equals `2`. A positional pattern cannot appear after a named pattern. If the payload itself is an expression, match the nested variant explicitly, such as `right=Expr.IntLit(2)`.

Because the compiler knows every variant, it checks that callers handle every state. This matters for AI-generated code: missing cases become compiler diagnostics instead of latent production behavior.

`T?` is sugar for the prelude enum `Option[T]` (variants `Some(value)` and
`None`), and `Result[T, E]` is the prelude enum with variants `Ok(value)` and
`Err(error)`:

```text
let name: string? = .None
fn loaded() -> Result[User, DbError]:
    ...
```

`.None` (or `Option.None`) is the absent case, and `?` propagates `.None` or `Result` errors from the current function.
A plain `User` can be assigned to `User?` without writing `.Some(...)`; the
implicit wrap adds one layer only. In a `match`, `.Some(user)` matches a
present optional and binds `user` as `User`; `.None` matches absence. A bare
`user` pattern would bind the entire optional.

Construct `Result` values with its variants, written `.Ok(...)` and
`.Err(...)` where a `Result` type is expected, or `Result.Ok(...)`. The
variants are not prelude names, so a bare `Ok(user)` is an unknown name:

```text
return .Ok(user)
return .Err(db_error)
```

`Result[T, E]`, `T?`, and `mut Suspend[T]` are must-use values. Do not leave
one as an ignored statement, including as the final expression of a loop
or an `if` without `else`. Propagate it, match it, return it, or
store it for later. When discarding it is deliberate, make that decision
visible with `_ := expression`, or with `let _ = expression`:

```text
_ := cache.refresh()
let _ = cache.refresh()
```

The compiler warns when an ordinary local binding is never read. A name
beginning with `_` suppresses that warning for ordinary values, but it does not
silently discard a must-use value; use `_ :=` or `let _ =` for that.

## Functions

Functions use `fn`, typed parameters, and an explicit return type:

```text
fn add(a: i32, b: i32) -> i32:
    a + b
```

The function body is an indented block. The last expression is the return value:

```text
fn normalized_email(user: User) -> string:
    user.email.trim().lower()
```

Explicit `return` exists for early exits:

```text
fn first_name(name: string) -> string:
    if name == "":
        return "anonymous"
    name.split(" ")[0]
```

Use `void` when a function does not return a useful value:

```text
fn print_user(user: User) -> void $ Console:
    println(user.email)
```

Functions are ordinary values, so small scripts can grow into reusable tools without switching styles:

```text
fn normalize_email(email: string) -> string:
    email.trim().lower()

clean := normalize_email("Ada@Example.COM ")
```

Parameter names are non-reassignable bindings. A `mut T` parameter permits mutation
through the reference but cannot itself be rebound. Use a `let` local when the
algorithm needs reassignment:

```text
fn normalize(value: i32) -> i32:
    let current = value
    if current < 0: current = -current
    current
```

Calls can mix positional and named arguments. Positional arguments must come first; no positional argument can appear after a named argument:

```text
fn resize(width: i32, height: i32) -> (i32, i32):
    (width, height)

size := resize(640, height=480)
bad := resize(width=640, 480)        # invalid: positional after named
```

Parameters can have default values:

```text
fn connect(host: string, port: i32 = 443, tls: bool = true) -> Connection:
    ...

secure := connect(host="api.example.com")
local := connect("localhost", port=8080, tls=false)
```

Default values can use pure expressions and pure function calls. They cannot
require provider requirements or suspension, mutate parameters or captures,
or pass non-fresh mutable access to any call:

```text
fn default_port() -> i32:
    443

fn connect(host: string, port: i32 = default_port()) -> Connection:
    ...

fn bad_connect(host: string, token: string = read_secret!("TOKEN")) -> Connection:
    ...      # invalid: default value suspends and requires a capability
```

Function types do not carry purity, so a default cannot call through a
function value or dynamic trait method; a named callable must have a verified
purity summary.

A final function-typed parameter may follow defaulted parameters, as in
Kotlin and Swift. A trailing block or a named argument supplies it, so the
options before it can be left out:

```text
fn retry(times: i32, backoff: i32 = 100, body: fn() -> void) -> void:
    body()

fn run() -> void:
    retry(3):
        pass
    retry(3, backoff=10):
        pass
```

Use varargs when a function accepts zero or more positional arguments. Write `...` after the parameter name; the type after `:` is what the body sees:

```text
fn sum(values...: List[i32]) -> i32:
    let total = +0
    for value in values:
        total = total + value
    total

sum(1, 2, 3)

nums := [1, 2, 3]
sum(nums...)
```

Varargs must be the final positional parameter. `nums...` spreads a list into positional arguments at the call site, and spread syntax is positional only. If a vararg is passed by name, it accepts the collected value, here a list:

```text
fn tagged_sum(tag: string, values...: List[i32]) -> i32:
    ...

tagged_sum("score", 1, 2, 3)
tagged_sum("score", nums...)
tagged_sum(tag="score", values=nums)
tagged_sum(tag="score", nums...)      # invalid: positional spread after named arg
```

A vararg may also collect a tuple. With a type parameter bounded by `Tuple`, one function forwards any number of arguments of any types, and a spread of a tuple fills a function's parameters one element each:

```text
use std.function.{Fn, Tuple}

fn call[Args < Tuple, O, $R](f: Fn[Args, O, $ R], args...: Args) -> O $ R:
    f(args...)

fn add(a: i32, b: i32) -> i32: a + b

call(add, 1, 2)        # Args is (i32, i32)
pair := (1, 2)
add(pair...)           # a tuple spread fills a and b
```

A function value keeps a `List` vararg. Its inputs tuple ends in the rest element `List[i32]...`, so `f := sum` has type `fn(List[i32]...) -> i32` and is called as `f(1, 2, 3)`. That type differs from `fn(List[i32]) -> i32`, and a tuple spread must match the callee's inputs exactly:

```text
fn g(a: usize, b: usize, xs...: List[i32]) -> usize: a + b + xs.len()
fn h(a: usize, b: usize, xs: List[i32]) -> usize: a + b + xs.len()

let t: (usize, usize, List[i32]...) = (1, 2, 3, 4)  # the tail is collected
let u: (usize, usize, List[i32]) = (1, 2, [3, 4])
g(t...)                # ok
h(u...)                # ok
g(u...)                # invalid: u has no rest element
call(g, 1, 2, 3, 4)    # Args is (usize, usize, List[i32]...), from g
call(g, 1, 2, [3]...)  # collected as the tuple (1, 2, [3]...)
let (a, b, xs...) = t  # a spread pattern binds xs: List[i32]
```

A tuple expression may also end in a list spread, as in `(1, 2, xs...)`.
A rest tuple compares, prints, and displays like a tuple, with the rest's
items after the fixed ones: `"$t"` is `(1, 2, 3, 4)`.

Function types use `fn(...) -> ...`:

```text
fn apply(value: string, transform: fn(string) -> string) -> string:
    transform(value)
```

Each function type is sugar for a standard constructor from `std.function`:
`fn(string) -> string` is `Fn[(string,), string, $()]`, and
`fn!(A) -> O $ R` is `SuspendFn[(A,), O, $ R]`. The spelled names are imported
where they are written. Function types are ordinary implementation targets:

```text
use std.function.Fn

trait Describe:
    fn describe(self) -> string

impl Describe for fn(string) -> string:
    fn describe(self) -> string:
        "string transform"

impl Describe for Fn[(i32,), i32, $()]:
    fn describe(self) -> string:
        "integer step"
```

Function values have no `Eq`, and their identity is unspecified, so `==` and a
direct `is` on them are errors.

When a function's final parameter is a zero-argument callback, an indented trailing block can supply it without writing `fn()` or its return type:

```text
result := when(a, b):
    compute_result()

transaction:
    save_user()
    write_audit_log()
```

Ordinary arguments stay inside `()`. Calls with no ordinary arguments omit empty parentheses. The callback's return type and behavior are checked against the final parameter type. Callbacks with parameters continue to use explicit `fn(...)` syntax; trailing-block sugar is zero-argument only.

`return` inside a trailing block exits the generated callback, not the enclosing function:

```text
fn load(cached: User, use_cache: bool) -> User:
    user := transaction:
        if use_cache:
            return cached
        fetch_user()

    audit(user)
    user
```

Here `return cached` supplies the callback's result, after which `load` continues with `audit(user)`. Trailing blocks do not support non-local return.

Closures use anonymous `fn(...) -> ...` syntax. They are expressions, so they can be bound to names, passed to functions, or returned from functions:

```text
slugify := fn(text: string) -> string:
    text.trim().lower().replace(" ", "-")

slug := slugify("Hello hd-lang")
```

There is no separate short closure syntax. Use `fn(...) -> ...:` for closures.
Same-line closure bodies allow one simple statement:

```text
inc := fn(x: i32) -> i32: x + 1
add := fn(x: i32, y: i32) -> i32: x + y
make_id := fn() -> string: "id_123"
```

Inline closures may omit parameter and return types when the surrounding call provides an expected function type. Parenthesize each closure expression to pass multiple multiline callbacks inline:

```text
fn choice(
    first: fn(i32) -> void $ Console,
    second: fn(string) -> void $ Console,
) -> void $ Console:
    first(1)
    second("two")

choice(
    (
        fn(aa):
            println(aa)
    ),
    (
        fn(bb):
            println(bb)
    ),
)
```

The expected types infer `aa: i32`, `bb: string`, and a `void` return from both callbacks. A standalone nonrecursive closure needs explicit parameter types but may infer its return type from its body.

A directly bound local closure may call itself when it writes an explicit
return type. Parameter types can still come from an expected function type:

```text
let sum: fn(i32) -> i32 = fn(n) -> i32:
    if n == 0: 0
    else: n + sum(n - 1)
```

`sum := fn(n: i32): ...` cannot recurse because its return type is inferred.

Closures capture values from the surrounding lexical scope:

```text
prefix := "user:"

label_user := fn(id: string) -> string:
    prefix + id

label := label_user("123")
```

Closures may also assign captured locals and mutate captured mutable values. Such a closure has an ordinary function type, and calling it needs no mutable access:

```text
let count = +0

let next: fn() -> i32 = fn() -> i32:
    count = count + 1
    count

next()
```

A higher-order function therefore takes a plain `fn(...) -> T`, whether or not the callback mutates what it captured:

```text
fn repeat(times: i32, f: fn() -> void) -> void:
    let i = +0
    while i < times:
        f()
        i = i + 1
```

A closure never captures a provider from an enclosing `$.with` scope. Every
requirement its body uses goes into its function type's row, and each call
supplies it. To keep the provider in effect where the closure is written,
capture its value: `clock := $.use(Clock)`, then `fn(): clock.now()` (see
[Lexical or dynamic providers](#lexical-or-dynamic-providers)). Provider
values are ordinary captured values, so a requirement row is not a complete
authority escape report. Serializable closures are a separate deferred design area; the
language has not yet defined their compatibility or execution semantics.

When a closure is passed where a function type is already expected, parameter and return types can usually be inferred:

```text
fn map_names(names: List[string], f: fn(string) -> string) -> List[string]:
    ...

lower_names := map_names(names, fn(name):
    name.lower()
)
```

The same call can use a same-line closure:

```text
lower_names := map_names(names, fn(name): name.lower())
```

Shorthand argument closures such as `$0 + $1` are not supported; closures use named parameters.

A method or associated function can be passed by name with `::`. `Type::method` takes the receiver as its first argument, and `value::method` binds the receiver when the reference is made:

```text
data Counter:
    value: i32

impl Counter:
    fn read(self) -> i32:
        self.value

    fn bump(mut self, by: i32) -> void:
        self.value = self.value + by

fn readings(counters: List[Counter]) -> List[i32]:
    counters.map(Counter::read)

fn bumper(counter: mut Counter) -> fn(i32) -> void:
    counter::bump
```

A bound reference keeps the object it was made with, even if `counter` is later reassigned; a closure `fn(by): counter.bump(by)` would read the variable when it runs. Fields have no reference form: write `fn(user): user.email`. A suspending method is referenced without `!`, as `Store::load`, and has an `fn!` type.

Generic functions put generic arguments after the function name:

```text
fn first[T](items: List[T]) -> T?:
    if items.len() == 0:
        .None
    else:
        items[0]
```

Generic parameters, including row parameters, use uppercase names such as `T`, `U`, `K`, `V`, and `R`. This is a style rule only; a parameter's kind comes from its declaration, where a row parameter is written `$R`.

Generic arguments are inferred at call sites when the type is unambiguous. Callers can also write the generic arguments explicitly:

```text
names := ["Ada", "Grace"]

a := first(names)            # T inferred as string
b := first::[string](names)  # explicit generic argument
```

In an expression, the list follows `::`, so brackets right after a value
always index it: `handlers[0](event)` calls the first handler. A type
keeps plain brackets, as in `List[string]`, and so does a declaration's
own parameter list, as in `fn first[T]`.

An explicit list may stop early: `pair::[string]("left", 1)` writes `Left`
and leaves `Right` to inference. Use `_` to infer a slot before one you
write:

```text
fn convert[From, To](value: From) -> To:
    ...

user := convert::[_, User](payload)
```

Every `_` or omitted slot is determined by the call arguments, expected
result type, or generic constraints. It is not a type and cannot be used
in `List[_]`.

When several arguments solve one type parameter, their types may differ
only in `mut`: a `mut User` and a `User` give `T = User`. A call never
makes a trait value to match them, as Rust and Go don't, and no number ever
widens implicitly. Write the cast, or name the type:

```text
fn max[T < Ord](left: T, right: T) -> T:
    ...

fn cmp[T < Display](left: T, right: T) -> bool:
    ...

fn widest(small: i32, large: i64) -> i64:
    max(i64(small), large)     # max(small, large) is a type-mismatch

fn same(user: User, label: Display) -> bool:
    cmp::[Display](user, label)  # cmp(user, label) is no-common-type
```

A generic parameter may declare a default after its bound. The default
fills the parameter only when the use site leaves it unsolved, and a
written type that leaves out a trailing slot gets it:

```text
data AppError:
    message: string

type Outcome[T, E = AppError] = Result[T, E]

fn load(path: string) -> Outcome[string]:
    .Ok(path)

fn widen[T = i64](value: T) -> T:
    value
```

`widen(3)` is still an `i32`, because the argument decides first. That is
how `collect()` returns a `List` when nothing names a target: its
declaration is `collect[C < FromIterator[T] = List[T]]`. Defaults go on
functions, methods, data types, enums, traits, and `type` declarations,
not on `impl[...]`, and an `impl` repeats a trait method's default.

A generic function, or a generic enum's one-payload variant constructor,
passed as an argument takes its type arguments from the call.
`result.map_err(TaskError.Failed)` on a `Result[T, FsError]` builds
`TaskError[FsError]`, and a parameter the call leaves unsolved is an error.
Only use sites infer: a declaration always writes its own generic parameters
and signature.

Generic parameters are erased at runtime. A function that needs a type
parameter's runtime identity bounds it by `Inspectable`, which supplies that
identity with each call:

```text
use std.inspect.Inspectable

fn first_of[T < AnyRef & Inspectable](items: List[Inspectable]) -> T?:
    for item in items:
        match item.downcast::[T]():
            .Some(found) => return .Some(found)
            .None => pass
    .None
```

An explicit list may leave out trailing arguments, which are inferred, or
defaulted when nothing solves them.

Functions cannot be overloaded. Each function name resolves to one declaration in a scope.

Functions are the primary unit of behavior. Methods, tools, workflows, tests, and generated adapters attach to normal functions instead of requiring a separate object model.

## Traits and Methods

Traits describe shared behavior without classes or inheritance:

```text
trait Describe:
    fn describe(self) -> string
```

Traits can provide default implementations:

```text
trait Named:
    fn name(self) -> string

    fn label(self) -> string:
        "name: " + self.name()
```

Implement a trait for a data type with an `impl` block:

```text
impl Describe for User:
    fn describe(self) -> string:
        self.email
```

Trait methods can be called with dot syntax:

```text
label := user.describe()
```

When two implemented traits leave a dot call ambiguous, qualify the trait explicitly with `Trait::method(receiver, ...)`, such as `Describe::describe(user)`. Qualification selects that trait implementation directly; it does not perform inherent or embedded-method lookup.

Data types can also have inherent methods with `impl TypeName`:

```text
impl User:
    fn domain(self) -> string:
        self.email.split("@")[1]

    fn tagged[T](self, value: T) -> T:
        value

domain := user.domain()
label := user.tagged::[string]("admin")
```

Generic methods infer all arguments when brackets are omitted. Their explicit
lists may stop early and may use `_` in individual inferred slots, just like
module functions.

Traits can require multiple methods:

```text
trait Repository[T]:
    fn get!(self, id: string) -> T? $ Database
    fn save!(self, value: T) -> void $ Database
```

Functions can use trait constraints on generic parameters:

```text
fn show[T < Describe](value: T) -> string:
    value.describe()
```

Trait bounds compose like Rust:

```text
fn audit_label[T < Describe & Named](value: T) -> string:
    value.describe() + " / " + value.name()
```

Receivers are either `self` or `mut self`; there is no reference receiver spelling. Primitive parameters are passed by value. Composite parameters use reference permissions in the type position: `value: T` is readonly and `value: mut T` is mutable. `mut self` is receiver shorthand for `self: mut Self`. Data types, enums with storage, tuples, lists, maps, trait values, and closures are composite types.

Data types do not own behavior in the class sense. Behavior lives in inherent
`impl TypeName` blocks or trait implementations:

```text
data Money:
    amount: i64
    currency: string

trait Merge[T]:
    fn merge(self, other: T) -> T

impl Merge[Money] for Money:
    fn merge(self, other: Money) -> Money:
        Money {
            amount: self.amount + other.amount,
            currency: self.currency
        }
```

Trait implementation is explicit. A type does not implement a trait just because it has matching methods.

### Operators

Operators on your own types come from the `std.ops` traits, in Rust's
shape. The right operand's type is the trait argument, and the result is
the associated type `Out`. The argument defaults to `Self`, so
`impl Add for Money` means `impl Add[Money] for Money`. Operator syntax
needs no `use`; the `impl` does:

```text
use std.ops.{Add, Mul}

impl Add for Money:
    type Out = Money
    fn add(self, rhs: Money) -> Money:
        Money { amount: self.amount + rhs.amount, currency: self.currency }

impl Mul[i64] for Money:
    type Out = Money
    fn mul(self, rhs: i64) -> Money:
        Money { amount: self.amount * rhs, currency: self.currency }

fn total(price: Money, fee: Money) -> Money:
    price * 3 + fee
```

The twelve operators are `+ - * / %`, unary `-`, `& | ^ ~`, `<<`, and
`>>`. `==` and `<` keep using `Eq` and `PartialOrd`, and `**`, `&&`, and
`||` cannot be overloaded. The left operand chooses the implementation,
so write `price * 3`: `3 * price` types `3` as `i32` and looks for an
`impl Mul[Money] for i32`. A newtype such as `type Meters(f64)` inherits no
operators; implement the ones it needs by hand. Two primitive operands keep
the built-in typing, which never widens, then call the same trait methods, whose
standard implementations for numbers are compiler intrinsics. The trait of
`~` is `Not`, and `string` implements `Add`, so generic code bounded by
`Add` can concatenate strings.

Compound assignment, such as `total += x`, always means
`total = total + x`, for every type. It needs the operator and a place
that `=` could store into: a `let` local, a field of a mutable value, or
an element. On a `data` value it stores a new value, so another reference
to the old one is unaffected. Accumulators and builders change themselves
through ordinary methods, such as `sb.push(x)`. `Index` and `IndexSet`
give a type `grid[i]` and `grid[i] = v`, and `grid[i] += 1` reads, then
stores. `List`, `Map`, and `string` implement `Index`, and `List` and `Map`
implement `IndexSet`, so generic code bounded by them can index the
built-ins; through the bound, a missing `Map` key panics. On a `Map`, `counts[w] += 1` reads the entry as if the key must
exist, so it panics when `w` is absent; a plain read `counts[w]` still
gives an optional.

Generic numeric code uses the sealed `std.num` traits `Num`, `Integer`,
and `Float`, which only the primitive number types implement. Constants
come from `T::zero()`, `T::one()`, and `T::from_i64(n)`, which panics
when `n` does not fit. `Num` also brings `<`, `==`, and `"$x"`:

```text
use std.num.Num

fn sum[T < Num](items: List[T]) -> T:
    let total = T::zero()
    for item in items:
        total += item
    total
```

A newtype or library number is never `Num`. To share an algorithm between
primitives and your own types, declare your own trait over the operator
traits and implement it for each type:

```text
use std.ops.{Add, Mul}

trait Ring < Add[Out = Self] & Mul[Out = Self]:
    fn zero() -> Self

impl Ring for i64:
    fn zero() -> i64:
        0

fn square_sum[T < Ring](items: List[T]) -> T:
    let total = T::zero()
    for item in items:
        total += item * item
    total
```

Data embedding does not interact with traits. Embedding never grants trait conformance, an embedded type's trait methods are not promoted, and a promoted method never fills a trait method. An implementation that wants the embedded behavior delegates the trait to the embedded field, which forwards every method, or forwards by hand:

```text
impl Describe for C by A

impl Describe for D:
    fn describe(self) -> string:
        self.B.describe()
```

Dynamic dispatch follows the Go interface style: use the trait name as a value type, and method calls through that trait value dispatch to the concrete implementation at runtime.

```text
fn print_display(value: Describe) -> void $ Console:
    println(value.describe())

print_display(user)
```

This is different from generic static dispatch, where the compiler specializes the function for a concrete type:

```text
fn show_static[T < Describe](value: T) -> string:
    value.describe()
```

A trait with an associated type becomes a value type once the type binds
it, as Rust's `dyn Iterator<Item = T>` does. The binding makes every
method signature concrete, so one body still serves every caller:

```text
trait Supplier:
    type Item
    fn get(self) -> Self::Item

fn read(source: Supplier[Item = i32]) -> i32:
    source.get() + 1
```

`Supplier` alone, with `Item` unbound, is not a value type, and a trait
with an associated function never is. A bound may likewise fix an
associated type that a supertrait declares, as in
`I < NamedSupplier[Item = T]`.

## Type System

hd-lang is statically typed. The compiler knows the type of every expression before code runs, but local code can rely on inference when the type is obvious:

```text
name := "Ada"          # inferred string
count := 3             # inferred usize: no use fixes it, and it has no sign
```

Public boundaries stay explicit. Function parameters, return types, data fields, enum payloads, and trait methods carry type annotations so humans and AI agents can review interfaces without chasing implementation details:

```text
fn find_user(id: UserId) -> Result[User?, DbError]:
    ...

data User:
    id: UserId
    email: string
```

User-defined data types and enums are nominal types. Two types with the same fields are still different types:

```text
data UserRecord:
    value: string

data PostRecord:
    value: string

fn load_user(record: UserRecord) -> User?:
    ...

post := PostRecord { value: "post_123" }
load_user(post)        # invalid: PostRecord is not UserRecord
```

Tuples are structural. A value of type `(i32, i32)` is compatible with another `(i32, i32)` because tuple identity comes from its element types:

```text
let point: (i32, i32) = (10, 20)
let offset: (i32, i32) = point
```

Type aliases are transparent by default:

```text
type UserName = string

fn greet(name: UserName) -> void $ Console:
    println("hello, " + name)

let raw: string = "Ada"
greet(raw)              # ok: UserName is an alias for string
```

Use a nominal newtype when the compiler must prevent accidental mixing:

```text
type Mile(i32)
type Kilometer(i32)

fn drive(distance: Mile) -> void:
    ...

miles := Mile(10)
km := Kilometer(16)

drive(miles)            # ok
drive(km)               # invalid: Kilometer is not Mile
drive(10)               # invalid: i32 is not Mile

raw_distance := i32(miles)     # explicit cast back to base type
```

No number changes type implicitly, as in Go, Rust, and Swift. Widening,
narrowing, and crossing signedness are all written as casts. A literal still
takes the type it needs, so `small + 1` and `let x: i64 = 300` are fine:

```text
let small: i16 = 42
let large: i64 = small          # invalid: write i64(small)
let wide: i64 = i64(small)      # ok: an explicit widening
total := wide + small           # invalid: i64 + i16; write i64(small)

let huge: i64 = 9000
let smaller: i16 = huge         # invalid: narrowing
smaller_ok := i16(huge)         # ok: an integer cast wraps, like Go and Rust `as`
too_big := i16(40000)           # invalid: a literal argument is range-checked
```

A float-to-integer cast truncates toward zero and then saturates, as Rust
`as` does: `i8(x)` with `x = 300.0` gives 127, and NaN gives 0. No numeric
cast panics; a checked conversion is a library function returning `Result`.

A plain literal with no expected type, as in `let i = 0`, takes its width from the uses in its function: in `while i < names.len()`, `i` becomes a `usize`. The width follows the literal through a range, a list, a tuple, or a generic call, so in `for i in 0..10: items[i]` the range is a `Range[usize]`. Neutral uses, such as `i = i + 1` or a generic call like `show(i)`, fix nothing. When exactly one type can serve a call, as in `grow(5)` with only `impl Scale for i64`, that type fixes the width; with two or more, none does. With no such use in the function, a float is `f64`, and an integer is `usize`, unless a literal it meets is written with a sign, as `-1` or `+5`: then it is `i32`. A conflict names the line that fixed the width. A comparison that an unsigned type makes always true or false, such as `t >= 0`, is an error. An integer literal is never a float: write `1.0` for an `f64`. When there is an expected numeric type, the literal is checked against that type's range:

```text
x := 1                 # usize when no use needs another width
y := -1                # i32: the literal has a sign
let balance = +100     # i32, so balance - 150 is -50, not a panic
let small_ok: i8 = 1   # ok: 1 fits in i8
let bad: u8 = 300      # invalid: 300 is out of range for u8
```

Range diagnostics include the invalid literal, the target type range, and a repair hint:

```text
integer literal 300 does not fit in u8
valid range for u8 is 0..255
suggestion: use u16 if the value is intentional
```

Narrowing diagnostics point to an explicit cast:

```text
cannot assign i64 to i16 without an explicit cast
suggestion: use i16(huge) to keep the low bits
```

Generic types and functions use square brackets:

```text
let names: List[string] = ["Ada", "Grace"]
let scores: Map[string, i32] = {"Ada": 10, "Grace": 12}

fn first[T](items: List[T]) -> T?:
    if items.len() == 0:
        .None
    else:
        items[0]
```

Generic type declarations use `+T` for covariance, `-T` for contravariance, and unmarked `T` for invariance:

```text
data Producer[+T]:
    produce: fn() -> T

data Consumer[-T]:
    consume: fn(T) -> void

data Cell[T]:
    value: T
```

Trait generic parameters are always invariant, so `trait Source[+T]` is
`invalid-variance`.

Variance conversions apply to readonly outer views. A `mut Producer[T]`, `mut Consumer[T]`, or other mutable generic view remains invariant because its fields can be replaced.
They must also preserve representation. Function types declare variance too:
they are contravariant in each parameter, covariant in the result, and
invariant in the requirement row. Because only permission changes preserve
representation, `fn() -> mut User` converts to `fn() -> User`, but
`fn() -> i32` does not convert to `fn() -> Display`.

hd has no variadic generics. Code that works over any number of arguments of
any types uses an ordinary tuple: a vararg whose type is bounded by `Tuple`
collects the arguments into one tuple, and a spread passes a tuple back as
separate arguments. `call` is an ordinary hd function:

```text
use std.function.{Fn, Tuple}

fn call[Args < Tuple, O, $R](f: Fn[Args, O, $ R], args...: Args) -> O $ R:
    f(args...)

fn add(a: i32, b: i32) -> i32: a + b

fn three() -> i32: call(add, 1, 2)
```

Here `Args` is solved as `(i32, i32)`, the tuple of the argument types. The
same shape serves arity-generic adapters, such as a memoizing wrapper. Every
tuple whose elements implement `Eq` and `Hash` implements them too, so `Args`
can key a map:

```text
use std.function.{Fn, Tuple}

data Memo[Args < Tuple & Eq & Hash, O]:
    f: Fn[Args, O, $()]
    cache: mut Map[Args, O]

impl[Args < Tuple & Eq & Hash, O] Memo[Args, O]:
    pub fn call(mut self, args...: Args) -> O:
        match self.cache.get(args):
            .Some(hit) => hit
            .None => self.fill(args)

    fn fill(mut self, args: Args) -> O:
        out := (self.f)(args...)
        self.cache[args] = out
        out
```

The standard `std.task.all!` awaits children of different result types and
returns their results as one tuple. It is a compiler intrinsic with one typing
rule: children of types `mut Suspend[X_1]`, ..., `mut Suspend[X_n]` give
`(X_1, ..., X_n)`.

```text
use std.task.all

data User:
    name: string

fn load_user!(id: i64) -> User:
    User { name: "Ada" }

fn load_orders!(id: i64) -> List[i64]:
    [id]

fn page!(id: i64) -> string:
    let (user, orders) = all!(load_user(id), load_orders(id))
    "${user.name}: ${orders.len()}"
```

Pass each child as a plain call, which builds a cold suspension. Writing
`load_user!(id)` inside `all!` awaits it first, as any bang call does, so its
value is a `User`, not a suspension, and the call does not type-check.

Traits describe behavior, but trait implementation is explicit. A type does not satisfy a trait just because it has matching methods:

```text
trait Describe:
    fn describe(self) -> string

impl Describe for User:
    fn describe(self) -> string:
        self.email
```

Generic trait bounds use static dispatch:

```text
fn label[T < Describe](value: T) -> string:
    value.describe()
```

Using a trait name as a value type creates a Go-style trait value: a pair of concrete value plus method table, dispatched at runtime. There is no `dyn` marker:

```text
fn print_display(value: Describe) -> void $ Console:
    println(value.describe())
```

`Any` is the built-in universal empty trait, analogous to Go's `any`. Every value type, optionals included, satisfies it automatically. Use `Any` for an erased dynamic value and `T < Any` when generic code must preserve the concrete type:

```text
fn keep_erased(value: Any) -> Any:
    value

fn preserve[T < Any](value: T) -> T:
    value
```

`Any` has two sealed subtraits, and every value type implements exactly one. `AnyVal` covers the values without identity: primitives, `string`, and tuples. `AnyRef` covers the values with identity: data, enums, lists, maps, function values (whose identity is unspecified), and trait values. `is` on a type parameter needs `T < AnyRef`:

```text
fn same[T < AnyRef](left: T, right: T) -> bool:
    left is right
```

Mutable bounds combine access permission with trait conformance:

```text
trait Clear:
    fn clear(mut self) -> void

fn clear_value[T < mut Clear](value: T) -> void:
    value.clear()

fn accept_mutable[T < mut Any](value: T) -> void:
    keep_erased(value)
    pass
```

`mut Trait` is likewise a mutable dynamic trait view. `mut Any` preserves mutable access to an erased composite value, but provides no type-specific operation by itself. `mut List[User]` satisfies `mut Any`; `List[mut User]` does not, because its root is readonly.

There is no implicit nullability. `T` and `T?` are different types, and only optional values can be `.None`:

```text
let name: string = "Ada"
let nickname: string? = .None
let value: Any = .None       # invalid: .None needs an expected optional type
let maybe_value: Any? = .None
let erased: Any = nickname   # an optional erases to Any like any enum value
```

An optional is an ordinary enum value, so `T?` erases to `Any` and `is` compares optionals like other enum values. A bare `.None` still needs an expected optional type.

Data embedding is composition, not inheritance. It promotes fields and methods for convenience, but it does not make the outer data a subtype of the embedded data.

An explicit impl delegates to the embedded value when it wants that behavior. `by` names the embedded field, and every trait method with a receiver, defaults included, becomes the forwarding method you would write by hand, such as `fn describe(self) -> string: Describe::describe(self.Logger)`; the body may replace individual methods:

```text
data Logger:
    name: string

impl Describe for Logger:
    fn describe(self) -> string:
        self.name

data Service:
    Logger

impl Describe for Service by Logger

data Worker:
    Logger

impl Describe for Worker by Logger:
    fn describe(self) -> string:
        "worker " + self.Logger.describe()

fn show(value: Describe) -> void $ Console:
    println(value.describe())

service := Service {
    Logger: ...Logger { name: "api" }
}

show(service)       # ok: the impl forwards to Logger
service.describe()  # ok: one candidate, Service's Describe method
```

Embedding never grants trait conformance, and a promoted method never fills a
trait method: without `by`, `impl Describe for Service` must write
`describe`. When the embedded type has `describe` only as a `pub` inherent
method, forward by hand, as in `fn describe(self) -> string:
self.Logger.describe()`. Where `Describe` is in scope, `service.describe()`
is then ambiguous between `Service`'s `Describe` method and `Logger`'s
promoted method; call the implementation as `Describe::describe(service)`
and the embedded method as `service.Logger.describe()`.

### Small Typed Idioms

Generic implementation binders, associated functions, width conversion,
trait-valued collection literals, and inferred closure rows use the ordinary
forms shown here:

```text
trait Notifier:
    fn notify(self, message: string) -> void

impl[N < Notifier] Notifier for List[N]:
    fn notify(self, message: string) -> void:
        for notifier in self:
            notifier.notify(message)

data Period:
    day_count: i32

impl Period:
    fn days(count: i32) -> Period:
        Period { day_count: count }

month := Period::days(30)
let narrow: i16 = 12
let wide: i64 = 30
let total: i64 = narrow + wide

let labels: List[Display] = ["Ada", "Grace"]

fn invoke[$R](callback: fn() -> void $ R) -> void $ R:
    callback()

fn report() -> void $ Console:
    invoke(fn() -> void: println(labels.len()))
```

The `invoke` call infers the omitted closure row as `Console` from its body and
unifies it with `R`; an omitted row is not assumed empty.

## Modules, Packages, and Use Declarations

Modules are inferred from file paths under the package source root. There is no required `module` or `package` declaration:

```text
src/user/types.hd      # module user.types
src/user/service.hd    # module user.service
src/post/service.hd    # module post.service
```

Which source file a package designates as the executable `main` module, and
how a concrete host profile binds `Console`, remain package-tooling and runtime
configuration work tracked in Open Issues. The language-level signature still
exposes `$ Console`.

Module path components follow the same NFC Unicode identifier rules as source names. Module identities are case-sensitive, but a package is rejected when two paths collide after Unicode case folding and NFC normalization.

Packages use `hd.toml`. The default source root is `src`:

```toml
[package]
name = "my_app"

[source]
root = "src"

[dependencies]
billing = "github.com/acme/billing@1.2.0"
```

There is no package registry. A dependency is a repository path and a
minimum version, and versions are the repository's tags, such as
`v2.1.0`. The build takes the largest minimum any manifest asks for, and
`hd.sum` records each dependency's hash. Source still names the dependency
as `dep.billing`. On `github.com` the path is `github.com/OWNER/REPO`; on
any other host, mark where the repository ends with `.git`, as in
`git.example.com/shop/billing.git@0.4.2`. A manifest never states its own
path: a package is known by the path it is fetched from.

A package depends on another local package through its directory,
`billing = { path = "../billing" }`, in a workspace or not. A tagged
release cannot hold a bare path requirement, so a package that is released
adds the version a fetched copy should use, as Cargo does:
`billing = { path = "../billing", version = "0.4.2" }`.

A workspace root declares no package, so it has no `tasks/`. Tasks for the
whole repository live in a member of their own, such as `tools/`, as
Cargo's xtask pattern does: `hd run -p tools release` runs
`tools/tasks/release.hd` from the root, in the `tools` directory.

Directories define submodule namespaces only when they contain a `mod.hd` file. `mod.hd` is required for every directory module and acts as the public index:

```text
src/user/mod.hd        # module user
src/user/types.hd      # module user.types
src/user/service.hd    # module user.service
```

Make declarations public with `pub`:

```text
# src/user/types.hd

pub type UserId(string)

pub data User:
    pub id: UserId
    pub email: string
```

Use selected names with grouped use declarations:

```text
# src/post/service.hd

use pkg.user.types.{User, UserId}

fn author_id(user: User) -> UserId:
    user.id
```

Use the module namespace when qualification is clearer:

```text
use pkg.user.types

fn author_id(user: types.User) -> types.UserId:
    user.id
```

Use aliases for long paths or name conflicts:

```text
use pkg.user.types as user_types
use dep.billing.types.{UserId as BillingUserId}
use std.time.{Duration}
```

`pkg` means the current package root. `std` means the standard library. `dep.<name>` means an external dependency from `hd.toml`.

Use `self` and `super` for relative use paths. `self` is the current module, as in Rust, and `super` its parent; in `mod.hd`, `self` is the directory module. A root file starts at its root and has no `super`: in `src/lib.hd` and `src/main.hd`, `self.x` is `src/x.hd`, and in `tests/checkout.hd`, `self.common` is `tests/common/`:

```text
# src/user/service.hd

use self.types.{Query}          # src/user/service/types.hd
use super.types.{User, UserId}  # src/user/types.hd
use super.super.shared.{Email}  # src/shared.hd
```

Use `mod.hd` to define the directory module and expose a clean package-facing API:

```text
# src/user/mod.hd

pub use pkg.user.types.{User, UserId}
pub use pkg.user.service.{load_user, save_user}
```

`pub use` introduces the names into `pkg.user` and exposes them to other
modules. The source declarations must already be `pub`, and no new declaration
identity is created:

```text
use pkg.user.{User, UserId, load_user}
```

Submodules are not brought into scope automatically. Parent modules and child
modules both use explicit use declarations.

Files in one folder may use each other in a loop, such as a `mod.hd` facade
and the child files it re-exports. Folders must not: when a file in
`src/shop/` uses `src/error.hd` while `src/lib.hd` uses `pkg.shop`, the loop
`src -> src/shop -> src` is `folder-cycle`. The fix moves the shared file into
a leaf folder, `src/error/mod.hd`, which keeps the module name `error`. Uses
in test code do not count, and a `pub use` chain must end at a declaration
([Dependency Cycles](../spec/lang/10-modules.md#dependency-cycles)).

Declarations are module-private by default, and `pub` makes them public. Enum variants inherit the enum's visibility. Data fields and inherent methods remain private unless individually marked `pub`, even on a public data. Embedded fields take no marker and are always public, so a public data type may embed only public types. A public signature, including its `$` requirement row, cannot leak a module-private type or trait. A `pub` function or `pub` method writes its whole signature: it must declare its result type, and without a `$` clause its row is empty. Only a private function or method may leave its result type and row to inference, so a caller in another file never waits for a body. There is no package-private visibility modifier.

## Program Entry Points

`pub fn main` is the conventional default entry point for an executable package. It takes no source-level parameters. Process arguments, environment, console I/O, and other host services selected by the runtime profile are explicit requirement-row entries:

```text
pub fn main!() -> Result[void, ConsoleError] $ Console:
    let mut console = $.use(Console)
    console.write_line!("starting")?
    .Ok(())
```

`.Ok(())` is the success value of `Result[void, E]`: `void` is another name
for the empty tuple type `()`, whose one value is `()`.
`ConsoleError` implements `Display`, as required for an entry-point error type.

`Console.write_line!` takes `mut self`, so `$.use(Console)` gives mutable
access. A `:=` binding would expose only a readonly view, so code that keeps
the console in a local writes `let console: mut Console`. `println` needs
only `$ Console`. It is ordinary `std` code that drives the provider's
`write_line!` with `block_on`, so a recording provider receives each line,
and it panics when the write fails. It also has `block_on`'s rules: it
works under a running driver, such as `main!` or a test body, and it is
rejected in a `defer` suite. Suspending code may also write with
`$.use(Console).write_line!`, as `main!` does above.

An entry-point row may contain only host capability traits supplied by its
selected runtime profile, such as `Console` above. Application
traits such as `Database` are not injected merely because they appear on
`main`; bind them with `$.with` inside the entry point.

The ordinary function rules still apply. Use the `!` suffix only when `main` can suspend. A non-suspending entry point is named `main`. Its result type implements `std.process.Termination`: `void`, `std.process.ExitCode`, or `Result[T, E]` with `E < Display`; the generated host adapter maps an `.Err` result to a failed invocation.

On `.Err`, an error type that implements `std.error.Error` prints its message and then each cause as `caused by: ...`; any other error prints its `Display` text. The process then exits with status 1. A program that picks its own code returns an `ExitCode`, a `u8` where 0 means success, as in `main() -> Result[ExitCode, CliError]` returning `.Ok(ExitCode(2))`.

`pub` controls hd-lang module visibility, not Wasm export visibility. Other public functions are not automatically exported from the compiled component. Tools, workflows, and library-facing Wasm functions become host-visible only through explicit registration, which generates the required boundary adapter. The exact registration API is designed separately for each integration.

Registered Wasm boundaries accept only recursively boundary-safe structural values. The initial boundary-safe forms are primitive scalars, `string`, tuples, `List[T]`, `Map[K, V]`, data types, enums, `T?`, and `Result[T, E]`, provided every contained type is also boundary-safe:

```text
pub data LookupRequest:
    pub ids: List[UserId]
    pub filters: Map[string, string]

pub enum LookupError:
    InvalidId(id: string)
    Unavailable(message: string)

fn lookup_users!(request: LookupRequest) -> Result[List[User], LookupError] $ Database:
    ...
```

Mutable types, trait values, closures, and live runtime handles cannot appear anywhere in an exported parameter or result. So an error type that holds an erased `Error` cannot cross either; convert it with `std.error`'s `report_of` to an `ErrorReport` first. Requirement keys such as `Database` are host bindings and do not cross as serialized function arguments. Export registration checks the complete signature and generates the boundary conversion.

Maps iterate in insertion order. Replacing an existing key keeps its position;
removing and reinserting it moves it to the end. Map equality and boundary
meaning remain independent of that order, so consumers must not attach semantic
meaning to field order unless their own format explicitly does so.

## Tests

A file keeps its tests in one `tests:` block, compiled only by `hd test`. The
block sees the module's private names, and its own helpers are visible only
inside it. Each `it("name"):` call registers one test case, which runs in a
fresh program instance. `it` is a prelude name only in test code, so other
code never sees it. Its trailing block is a suspending body, so it may
bang-call directly.

Unit tests get no host providers: every requirement comes from a `$.with`
fake. Use ordinary provider scopes for mocks and `std.testing` for
assertions:

```text
use std.testing.assert_equal

@derive(Eq, Debug)
data DbError:
    message: string

trait Database:
    fn count!(self) -> Result[i32, DbError]

data MockDatabase: pass

impl Database for MockDatabase:
    fn count!(self) -> Result[i32, DbError]: .Ok(3)

tests:
    it("loads the count"):
        $.with(Database=MockDatabase {}):
            result := $.use(Database).count!()
            assert_equal(result, .Ok(3), reason="mock count is returned")
```

A test case passes when its body completes, and fails on a panic, including
a failed assertion. Test instances do not share top-level mutable state.
`assert_equal` needs `Eq` and `Debug` on the compared type, so a failure can
show both values; derive both, as `DbError` does.

A test body has a fixed result. Without `?` it is `void`. With `?` it is
`Result[void, Error]`, where `Error` is the erased `std.error.Error`, so the
body ends in `.Ok(())`. `?` converts any error type that implements `Error`,
but not a plain `string`. An `.Err` fails the test, and the runner prints it
with its cause chain:

```text
use std.error.Error
use std.testing.assert_equal

data DigitError:
    text: string

impl Display for DigitError:
    fn to_string(self) -> string: "not a digit: " + self.text

impl Error for DigitError

fn parse_digit(text: string) -> Result[i32, DigitError]:
    if text == "7": .Ok(7) else: .Err(DigitError { text: text })

tests:
    it("parses a digit"):
        digit := parse_digit("7")?
        assert_equal(digit, 7, reason="the digit parses")
        .Ok(())
```

`it` is an ordinary function whose defaulted options come before its final
`body` parameter. Its options are named arguments: `ignore="reason"` and
`expect_panic="category"` take string literals, and `timeout` takes any
`Duration`, such as `timeout=5s` with `use std.time.s`. For
table tests, `std.testing.it_each` registers one test case per row, named
`name[i]`, and takes the same options. Its body takes the row, so it is an
explicit `fn!` closure passed as `body=`:

```text
use std.testing.{assert_equal, it_each}

fn double(value: i32) -> i32: value * 2

fn first(items: List[i32]) -> i32: items[0]

tests:
    it("an empty list has no first item", expect_panic="index-out-of-bounds"):
        _ := first([])

    it_each("doubles", [1, 2, 3], body=fn!(value: i32):
        assert_equal(double(value), value + value, reason="doubling adds the value to itself")
    )
```

Property tests check a claim over many generated inputs.
`std.testing.it_prop_with` takes a generator, a function that draws values
from a `Choices` source, and the runner shrinks a failing input to a small
one. Several inputs are one tuple. A later draw may depend on an earlier
one, as in `ordered_pair`, and `examples` lists inputs that run first on
every run:

```text
use std.testing.{assert, assert_equal, Choices, it_prop_with}

fn to_local(utc: i64, offset: i32) -> i64: utc + i64(offset)

fn to_utc(local: i64, offset: i32) -> i64: local - i64(offset)

fn instant_and_offset(c: mut Choices) -> (i64, i32):
    (c.int(-1_000_000, 1_000_000), c.int(-720, 840))

fn ordered_pair(c: mut Choices) -> (i32, i32):
    low := c.int(0, 100)
    high := c.int(low, 100)
    (low, high)

tests:
    it_prop_with("local time converts back", gen=instant_and_offset, examples=[(0, 0), (0, 840)], prop=fn!(input: (i64, i32)):
        let (utc, offset) = input
        assert_equal(to_utc(to_local(utc, offset), offset), utc, reason="the offset cancels")
    )

    it_prop_with("the high end is never below the low end", gen=ordered_pair, prop=fn!(pair: (i32, i32)):
        let (low, high) = pair
        assert(low <= high, reason="high is drawn from low upward")
    )
```

A generator draws with `c.int(lo, hi)`, `c.float(lo, hi)`, `c.bool()`,
`c.choose(items)`, `c.string(max_chars=12)`, `c.list(max, item)`,
`c.map(max, key, value)`, and `c.draw::[T]()`. `int` and `float` take their
type from the bounds or the context. There is no size to tune: draws
already lean toward small values and edges. Only a generator discards a
case, with `c.assume(ok)`; a property body cannot.

`list`'s item generator takes its own `Choices`, as in
`c.list(3, fn(inner: mut Choices) -> i32: inner.int(0, 9))`. It is the
same `c`, passed back. That lets `list` mark each element's draws as one
span, which the shrinker deletes or simplifies whole. It also keeps the
closure from capturing the outer `mut c` while `list` is using it. A named
generator needs no closure, as in `c.list(50, digit)`.

A recursive generator puts its leaf first. Each case has a draw budget, and
once it is spent, every draw returns its simplest value: `0`, `false`, or
an empty list, map, or string. So `c.int(0, 5)` returns `0` and the
generator ends:

```text
use std.testing.Choices

enum Value:
    Null
    Flag(on: bool)
    Count(n: i32)
    Text(text: string)
    Items(items: List[Value])
    Fields(fields: Map[string, Value])

fn label(c: mut Choices) -> string:
    c.string(max_chars=8)

fn value(c: mut Choices) -> Value:
    match c.int(0, 5):
        0 => .Null
        1 => .Flag(c.bool())
        2 => .Count(c.int(-100, 100))
        3 => .Text(c.string(max_chars=12))
        4 => .Items(c.list(4, value))
        _ => .Fields(c.map(4, label, value))
```

`it_prop` uses the input type's default generator, its `Arbitrary`.
`@derive(Arbitrary)` derives one, and one member fact,
`with(gen)`, draws a member with a generator of your own. The
compiler does not check that generator against the member's type, so a
wrong one panics on the first case and names the member and both types.
Every member, tuned or not, must implement `Arbitrary` and be
inspectable; a type with a function-typed member writes its own
`impl Arbitrary` instead. The default
`f32` and `f64` generators include NaN, the infinities, and `-0.0`, while
`c.float(lo, hi)` stays finite:

```text
use std.testing.{Arbitrary, Choices, assert, it_prop, with}

fn cents(c: mut Choices) -> i32:
    c.int(0, 10_000)

@derive(Arbitrary, Debug)
data Item:
    name: string
    @with(cents)
    price: i32

tests:
    it_prop("prices are never negative", prop=fn!(item: Item):
        assert(item.price >= 0, reason="cents draws from 0 to 10_000")
    )
```

The derived `Arbitrary` is an ordinary `std.testing` template. It reads
each variant's and member's `self_ref` from `std.structure`: `.Absent`
when the member's type never holds the type being derived, `.Optional`
when it does only inside a list, map, or optional, and `.Required` when
it holds that type directly, or through tuples, data types, a `Result`'s
`.Ok`, or an enum whose every variant does. An enum's simplest variant is its first one that is not `.Required`. An
enum whose every variant is `.Required`, or a data type with a
`.Required` member, has no finite value, and its property panics on the
first case with a message such as `Loop has no finite value`.

The test-case functions are called only directly at the top level of test
code, never as values, so a tool can list every test without running it.
`snapshot(text, expect="...")` from `std.testing` compares text with a
literal that `hd test --update` rewrites. `hd check` checks test code only
with `--tests`; `hd test` always compiles it.

Larger suites get their own files. A file whose name ends in `_test.hd`,
such as `src/billing_test.hd`, is a test module: it sees the package's
public names and holds `it` calls at its top level, with no `tests:` block.
Integration tests live under `tests/`, see the package as a dependent does,
and get real providers from the test profile. Each file directly under
`tests/` is its own program, so shared helpers go in a subdirectory such as
`tests/common/`. There, `pkg.billing` names the library's public API, and
`use self.common` reaches `tests/common/mod.hd`, as a task reaches
`tasks/shared/`. Test commands start in the package root, so fixture paths
are relative to it. For example, `tests/inventory.hd` imports the public API
through `pkg`, reads `tests/fixtures/inventory-total.txt`, and puts its test
case directly at the top level rather than inside a `tests:` block:

```text
use pkg.{Item, total_value}
use std.fs.{FsRead, read_text}
use std.path.Path
use std.testing.assert_equal

it("totals the inventory fixture"):
    expected := read_text!(Path("tests/fixtures/inventory-total.txt"))?
    items := [
        Item { name: "notebook", price: +1200 },
        Item { name: "pen", price: +250 },
    ]
    assert_equal(total_value(items).to_string(), expected.trim(), reason="fixture total")
    .Ok(())
```

See [Test Modules](../spec/lang/10-modules.md#test-modules) and the test
runner notes.

## Requirements and Suspension

Requirements, provider contexts, and suspension are language features specified
separately from ordinary functions and `Result` error handling.

The problem they solve: a function that sends mail and reads the clock looks
like any other function, and a test can check it only by patching globals. In
hd the signature `fn welcome!(user: User) -> void $ Mailer + Clock` says what
it touches, and a test supplies a fake mailer and a fixed clock for one block:

```text
fn welcome!(user: User) -> void $ Mailer + Clock:
    let mut mailer = $.use(Mailer)
    mailer.send!(user.email, "Welcome ${user.name}, it is ${$.use(Clock).now()}")

tests:
    it("sends one welcome mail"):
        let outbox: mut Outbox = Outbox { sent: [] }
        $.with(Mailer=outbox, Clock=ManualClock::new(Timestamp::from_unix_milliseconds(0))):
            welcome!(User { name: "Ada", email: "ada@example.com" })
        assert_equal(outbox.sent.len(), 1, reason="one mail")
```

Leaving out `Clock=` is a `missing-requirement` error at the call.

hd-lang separates three concerns often grouped under algebraic effects:

1. `$` rows statically check which dependencies a function requires.
2. Provider scopes inject concrete providers for those requirements.
3. `fn!`, `Suspend[T]`, and bang calls provide one-shot suspension.

These mechanisms cooperate, but none implies the others. Dependency lookup does
not suspend, suspension does not represent normal errors, and an ordinary
dependency does not have to be a host capability. Normal error handling uses
`Result[T, E]`:

```text
data ParseError:
    message: string

fn parse_user(input: string) -> Result[User, ParseError]:
    ...
```

Dependencies are ordinary traits or capabilities that can appear in the `$` requirement row. This makes dependency injection explicit without a separate dependency declaration syntax:

```text
trait Database:
    fn get_user!(self, id: UserId) -> Result[User?, DbError]

trait Cache:
    fn get_user(self, id: UserId) -> User?

fn load_user!(id: UserId) -> Result[User?, DbError] $ Database + Cache:
    let (db, cache) = $.use(Database, Cache)
    match cache.get_user(id):
        .Some(user) => return .Ok(user)
        .None => pass
    db.get_user!(id)
```

A suspending declaration also creates a cold computation constructor:

```text
let pending: mut Suspend[Result[User?, DbError]] = load_user(id)

fn demo!() -> Result[User?, DbError] $ Database + Cache:
    load_user!(id)   # drive and suspend inside a suspending body
```

`fn load_user!(...) -> T` lowers conceptually to a function constructing `mut Suspend[T]`. Arguments are evaluated when the cold suspension is constructed, while the body is compiled into a resumable state machine and begins only when driven. `:=` weakens a stored suspension to readonly `Suspend[T]`; use `let pending: mut Suspend[T]` when it must later be polled or cancelled. Each nested bang call is a possible suspension point: the compiler saves the enclosing state, drives the child computation, and resumes with its result.

`Suspend[T]` is a single-execution, pollable state machine. Its driver polls for `Pending` or `Ready(T)` and uses a waker to arrange further progress. Exclusive driving is enforced at runtime: competing drivers, reentrant polling, and driving after completion or cancellation panic. Repeated polling while pending is normal; executing again requires constructing a new suspension. Cancelling a suspension while it or a descendant is active on the current poll stack also panics and leaves its state unchanged.

The caller must satisfy the function's dependency requirements when constructing the suspension. The selected providers are captured then, even though the body has not started, and are not replaced by a later driver context. Cancellation is synchronous and runs registered `defer` suites in the suspension's unfinished frames after cancelling an unfinished child. A started suspension must be cancelled before it is discarded; raw abandonment runs no cleanup. A stored suspension uses `s!()` in a suspending body. Non-suspending code first writes `use std.task.block_on`, then calls `block_on(s)`. A driver is active while its executor is evaluating or polling it on the current program-instance call stack; the test runner drives each test body as a suspension, so a driver is active for the whole test, while a host-held invocation between polls is unfinished but not active. Calling `block_on` under an active driver is valid: it drives only its own suspension to completion and never touches the outer driver's suspensions, so an inner suspension that needs the outer driver to progress hangs. It is transitively forbidden in defaults, `defer` suites, and non-entry module initialization.

Here `$.use(Database, Cache)` retrieves multiple providers from the current context in order. The `!` on `db.get_user!(id)` marks a possible suspension point. It does not mean that the call raises an error or performs dependency lookup.

Providers are ordinary values whose types implement the required trait:

```text
data MockDatabase:
    user: User

impl Database for MockDatabase:
    fn get_user!(self, id: UserId) -> Result[User?, DbError]:
        .Ok(self.user)

mock_db := MockDatabase {
    user: User {
        id: UserId("user_123"),
        email: "test@example.com",
        display_name: "Test User"
    }
}
```

Call sites provide requirements through provider scopes. `$.with(Requirement=provider)` binds a requirement key to a provider value for the indented body:

```text
fn demo_mock!() -> Result[User?, DbError] $ Cache:
    $.with(Database=mock_db):
        load_user!(UserId("user_123"))
```

### Wire application providers in `main`

A package's own requirements are not host capabilities. If the application
uses `Mailer` and `Repo`, its library code declares `$ Mailer + Repo`, while an
entry point is the composition root: it constructs concrete adapters and
removes those two requirements with `$.with`. The entry point's signature then
lists only capabilities supplied by the host.

Here is the shared application in `src/app.hd`:

```text
pub trait Repo:
    fn save(self, name: string) -> string

pub trait Mailer:
    fn send(self, to: string) -> string

pub fn register(name: string, email: string) -> string $ Mailer + Repo:
    saved := $.use(Repo).save(name)
    sent := $.use(Mailer).send(email)
    "$saved; $sent"
```

The adapters in `src/adapters.hd` are ordinary types with `pub` fields, so
the entry points can build them. Production and development implementations
need not share a concrete type:

```text
use super.app.{Mailer, Repo}

pub data FileRepo:
    pub root: string

impl Repo for FileRepo:
    fn save(self, name: string) -> string:
        "file:${self.root}/$name"

pub data MemoryRepo:
    pub namespace: string

impl Repo for MemoryRepo:
    fn save(self, name: string) -> string:
        "memory:${self.namespace}:$name"

pub data SmtpMailer:
    pub endpoint: string

impl Mailer for SmtpMailer:
    fn send(self, to: string) -> string:
        "smtp:${self.endpoint}:$to"

pub data OutboxMailer:
    pub label: string

impl Mailer for OutboxMailer:
    fn send(self, to: string) -> string:
        "outbox:${self.label}:$to"
```

The recommended layout uses two entry points. `src/main.hd` wires production:

```text
use pkg.{FileRepo, SmtpMailer, register}

pub fn main() -> void $ Console:
    $.with(Mailer=SmtpMailer { endpoint: "smtp.example.com" }, Repo=FileRepo { root: "data/users" }):
        println(register("Ada", "ada@example.com"))
```

`src/dev.hd` wires local adapters without changing the application module:

```text
use pkg.{MemoryRepo, OutboxMailer, register}

pub fn main() -> void $ Console:
    $.with(Mailer=OutboxMailer { label: "local" }, Repo=MemoryRepo { namespace: "dev" }):
        println(register("Ada", "ada@example.com"))
```

Re-export the library pieces from `src/lib.hd`, then select both programs in
`hd.toml`:

```text
pub use self.app.{Mailer, Repo, register}
pub use self.adapters.{FileRepo, MemoryRepo, OutboxMailer, SmtpMailer}
```

```toml
[package]
name = "wiring"

[[executable]]
name = "app"
module = "main"

[[executable]]
name = "dev"
module = "dev"
```

Run them with `hd run app` and `hd run dev`.

Sometimes one executable must choose at startup. This alternative
`src/main.hd` uses one `$.with` in each branch, so the two sets of adapters
still need no common concrete type:

```text
use pkg.{FileRepo, MemoryRepo, OutboxMailer, SmtpMailer, register}
use std.host.{Env, env}
use std.time.Clock

pub fn main() -> void $ Console + Env + Clock:
    mode := match env("APP_ENV"):
        .Some(value) => value
        .None => "production"
    if mode == "dev":
        seed := $.use(Clock).now().unix_milliseconds()
        $.with(Mailer=OutboxMailer { label: "run-$seed" }, Repo=MemoryRepo { namespace: "dev" }):
            println(register("Ada", "ada@example.com"))
    else:
        endpoint := match env("SMTP_URL"):
            .Some(value) => value
            .None => "smtp.example.com"
        $.with(Mailer=SmtpMailer { endpoint: endpoint }, Repo=FileRepo { root: "data/users" }):
            println(register("Ada", "ada@example.com"))
```

The cost of runtime selection is visible in its row: `main` lists the union
of host capabilities used by either branch (`Env` for configuration, `Clock`
for the development label, and `Console` for output), even though one branch
runs. `Mailer` and `Repo` remain absent because both branches install them.

An integration test is another composition root. A file directly under
`tests/` puts `it` at top level and wires test-specific adapters itself:

```text
use pkg.{MemoryRepo, OutboxMailer, register}
use std.testing.assert_equal

it("wires test adapters"):
    $.with(Mailer=OutboxMailer { label: "test" }, Repo=MemoryRepo { namespace: "case" }):
        assert_equal(
            register("Ada", "ada@example.com"),
            "memory:case:Ada; outbox:test:ada@example.com",
            reason="the app uses the installed test providers",
        )
```

This test needs no host profile entry for either application trait.

Reusable contexts are provider-map values typed by a requirement row:

```text
fn prod_context() -> $.Context[$ Metrics + Cache]:
    $.context(Metrics=metrics, Cache=cache)

fn demo_context!() -> Result[User?, DbError] $ Logger + Metrics + Cache:
    $.with(Database=mock_db, Logger=console_logger, prod_context()...):
        load_user!(UserId("user_123"))
```

`$.Context[$ Metrics + Cache]` is not a variadic generic. The `$ Metrics + Cache`
part is an unordered requirement row: separate keys joined with `+`, written
the same way in a header and inside a type. `$.context` creates a reusable context,
`prod_context()...` spreads providers into a lexical scope (a suffix `...`
spreads, as it does in calls and lists), and `$.use`
retrieves them in the requested order. Entries in these forms are trait-type
requirement keys, not ordinary named-argument labels. Duplicate concrete keys
cannot coexist in one scope; a nested binding for the same written key replaces
the outer provider. Distinct generic key expressions that could become equal
under substitution are rejected instead of relying on erasure or
specialization. A missing provider is a compile-time error below an entry
boundary. The `$` namespace is special context syntax, not an ordinary value
namespace.

Requirement polymorphism for higher-order functions preserves callback
requirements rather than erasing them. A row parameter is declared with `$`,
as `$R`, so its kind shows where it is declared:

```text
fn transform[T, U, $R](items: List[T], f: fn(T) -> U $ R) -> List[U] $ R:
    ...
```

A provider scope removes a locally supplied requirement by extension: the
callback row lists `Logger` beside the row parameter, and the helper's own row
is the plain row parameter:

```text
fn provide_logger[$R](callback: fn(string) -> void $ R + Logger) -> void $ R:
    $.with(Logger=logger):
        callback("str")
```

Here `callback` requires `Logger` plus the other requirements in `R`. The local
provider satisfies `Logger`, so callers see only `R`, the remaining row. The helper itself is
not named `provide_logger!` because its body has no suspension point.

A long row gets a name with an ordinary `type` alias, called a row alias. Its
right side is a row, so it starts with `$` too. It stands for its keys wherever
a row follows `$`. Inside a type's brackets every row is written after `$`,
even one key or one alias, as in `$.Context[$ Stack]`:

```text
type Stack = $ Database + Cache + Logger

fn get_order!(id: UserId) -> Result[User?, DbError] $ Stack + Clock:
    load_user!(id)
```

A function value fits a function type whose row is wider than its own, so
handlers with different rows share one list:

```text
fn health() -> string $ Clock:
    "ok"

fn orders() -> string $ Stack:
    "orders"

fn routes() -> List[fn() -> string $ Stack + Clock]:
    [health, orders]
```

Without an expected type, `handlers := [health, orders]` gets the union of
the rows, `$ Stack + Clock`. So does `if admin: orders else: health`, and
so do `match` arms and inferred closure results. A list keeps its element row, so a list with a
wider row is built by an explicit copy, as in
`let wide: List[fn() -> string $ Stack + Clock + Metrics] = [handlers...]`.

A key may bind the trait's associated types, as a bound does. The provider
value then has that bound type, so its methods return concrete types, and
the provider's implementation must bind the same type:

```text
trait Store:
    type Item
    fn load(self, id: string) -> Self::Item

fn find(id: string) -> User $ Store[Item = User]:
    $.use(Store[Item = User]).load(id)

fn serve(id: string) -> User:
    $.with(Store[Item = User]=UserStore {}):
        find(id)
```

`Store[Item = User]` and `Store[Item = Post]` are two different keys, as
`Repo[User]` and `Repo[Post]` are.

A bundle of providers is reused by an installer: an ordinary function that
installs the providers and extends its callback's row with the same keys.
The trailing block is the callback, so an installer reads like a scope:

```text
fn with_stack![T, $R](config: Config, body: fn!() -> T $ R + Stack) -> T $ R:
    db := SqlDatabase::connect(config.database_url)
    $.with(Database=db, Cache=MemoryCache {}, Logger=console_logger):
        body!()

fn main!() -> void $ Clock:
    with_stack!(Config::default()):
        _ := get_order!(UserId("user_123"))
```

`R` is inferred at each call as the block's own row less the installed keys,
so the caller needs only `Clock` here, not `Stack`. A block that uses only
some of the installed keys still fits. A caller may fix `R` to a row that
lists an installed key; the block then sees the installer's nearer
provider, as with nested `$.with` scopes. Each call builds the providers again;
to build them once, return a `$.Context[$ Stack]` value and spread it into
each scope with `ctx...`.

Providers come from an enclosing `$.with` scope or an entry point's permitted
runtime-profile configuration; there are no implicit provider defaults. Row
parameters combine with other keys by listing them, as in `$ R + Logger`;
there is no row subtraction. Additional
`Result[T, E]` convenience APIs belong to the standard library.

### Lexical or dynamic providers

A callback that keeps a key in its row gets that provider at each call, so a
callee's `$.with` can supply it. That is the dynamic form, and it costs
nothing to write:

```text
fn at_noon[$R](callback: fn() -> i32 $ R + Clock) -> i32 $ R:
    $.with(Clock=Fixed { hour: 12 }):
        callback()

fn dynamic() -> i32:
    $.with(Clock=Fixed { hour: 9 }):
        at_noon(fn(): $.use(Clock).now())  # 12: the callee's Clock
```

For the lexical form, capture the provider value when you write the closure.
Its row then omits the key, so no callee can change the provider it uses:

```text
fn lexical() -> i32:
    $.with(Clock=Fixed { hour: 9 }):
        clock := $.use(Clock)
        at_noon(fn(): clock.now())  # 9: the captured Clock
```

The `$.with` block around a closure never satisfies the closure's own row.
So a closure that must have the empty row, such as a `filter` callback or a
closure returned from the block, captures the value the same way.

## Using Annotations

Annotations attach typed values to declarations. They do not change a declaration's type,
behavior, name, or visibility, and they register nothing.

A decorator is a plain value. It may precede any item (a function, data
type, enum, trait, implementation, or newtype) or member (a field, variant,
parameter, or method). Before a data or enum declaration it attaches a
type-level fact, and before a field, variant, or parameter it attaches
member metadata:

```text
@style(prefix="user_")
data User:
    @max_len(80)
    display_name: string

data Post:
    @flatten()
    Timestamps

fn get_user(
    @description("User identifier")
    id: UserId,
) -> User:
    ...
```

The field decorator attaches `max_len(80)` to the metadata of
`display_name`, which a template reads through the field's handle. The
embedded-field
decorator attaches `flatten()` to `Timestamps` and does not decorate members
promoted from `Timestamps`. A parameter decorator attaches metadata to the
parameter; it is the only way to give a parameter metadata.

A decorator before a function attaches a value that code reads with
`facts_of`, imported from `std.annotation`. It accepts only the name of a
module-level function, not a closure or other function value. A bare name of a function with no parameters is called,
so a marker needs no parentheses. A fact type may limit where its values
go with `@annotate`:

```text
use std.annotation.{annotate, facts_of}

@annotate(.Fn)
data Route:
    path: string

fn route(path: string) -> Route:
    Route { path: path }

@route("/users")
fn list_users() -> string:
    "[]"

fn users_path() -> string:
    match facts_of(list_users).find::[Route]():
        .Some(found) => found.path
        .None => ""
```

A lookup whose type argument mentions a type parameter, as in a generic
`find::[M]()` helper, needs `M < Inspectable`, which supplies `M`'s
runtime identity. A handle's `h.fact::[M]()` exempts the handle's own `F`.
A `Route` value before anything but a function is `decorator-target-kind`.
A newtype is a kind too, `.Newtype`. The compiler checks only that kind; whatever reads a value checks that it suits its
target. See [Target Kinds](../spec/lang/14-annotations.md#target-kinds).

To write shared metadata away from a long declaration, use a trait-less
derivation block. It names no trait, derives nothing, and holds only member
lines, which every derivation of the type sees:

```text
use std.structure.Structure

data User:
    @max_len(80)
    display_name: string
    active: bool

impl User by Structure:
    display_name += [min_len(1)]
```

A `+=` line appends after the member's decorator values, and a `=` line
replaces them. A per-trait derivation block, such as
`impl Encode for User by Structure:`, then edits the result for that one
derivation. The block lives in the type's module, and a type has at most
one. For a generic type it declares the type's own parameters, without
bounds, as `impl[T] Box[T] by Structure:`. Decorators and trait-less
blocks are module-level, so local declarations cannot carry member metadata.

Metadata is contextually typed as `List[Any]`, so any value may be
attached, and metadata values and reusable lists are ordinary values:

```text
use std.structure.Structure

let display_name_metadata: List[Any] = [
    min_len(1),
    max_len(80),
]

data User:
    display_name: string

impl User by Structure:
    display_name = display_name_metadata
```

Metadata values are evaluated once, at compile time, and must be
requirement-free. Multiple entries with the same concrete type on one member,
or two type-level decorators of one type on a declaration, are rejected.
Whether a value suits its member's type is checked by the code that reads
it, not by the compiler.

A data type's or enum's members and their metadata are read by a template
over `Structure`, as the next part shows. There is no runtime descriptor of
a declaration's structure.

Information derived from a whole type, such as a validator, a schema, or a
form description, is an ordinary trait with an associated function, derived
through its library's template:

```text
trait Validate:
    fn validator() -> Validator

@derive(Validate)
data Signup:
    @max_len(80)
    display_name: string

signup_validator := Signup::validator()
```

The template reads each member's metadata as facts. See
[Typed Derivation](../spec/lang/14-annotations.md#typed-derivation).

## Runtime and Library Features

Testing, sandbox enforcement, persistence, replay, and observability build on the language features introduced above but are primarily standard-library, tooling, or runtime concerns. They are documented separately in Runtime and Library Design.
