# Learn hd-lang in 10 Minutes

hd-lang is a statically typed, indentation-based language that compiles to
WebAssembly GC. It is designed for code that AI agents write and humans
review: types sit at the boundaries, dependencies are declared in signatures,
and suspension points are marked at every call.

This page is a fast tour. Every example parses with the
[reference parser](../spec/reference-parser/). The [Language Tour](LANGUAGE_TOUR.md)
covers the same ground in more depth, and the
[specification](../spec/README.md) is the normative reference.

## Hello

A file can start with plain top-level statements. `#` starts a comment.

```hd
# hello.hd
println("hello, hd-lang")
```

Blocks open with a header that ends in `:`, followed by an indented body or a
same-line body.

```hd
fn greet(name: string) -> void $ Console:
    println("hello, $name")

fn shout(name: string) -> void $ Console: println(name.upper())

greet("Ada")
```

The `$ Console` part is a requirement row. It is explained
[below](#requirements-and-providers).

## Bindings and Values

`:=` binds a name that cannot be reassigned, with an inferred type. `let`
declares a variable that can be reassigned; its type annotation is optional.

```hd
name := "Ada"
age := 36
let attempts: i32 = 0
attempts = attempts + 1
```

Numbers have explicit widths: `i8` to `i64`, `u8` to `u64`, `f32`, and
`f64`. An integer literal defaults to `i32`. Other primitives are `bool`,
`char`, and `string`.

```hd
let big: i64 = 9000
let ratio: f64 = 0.5
let initial: char = 'A'
mask := 0xFF_00
million := 1_000_000
narrow := i16(big)
```

Strings interpolate `$name` and `${expression}`. Raw strings start with `r`
and never interpolate.

```hd
summary := "User ${user.name} has ${posts.len()} posts"
pattern := r"\d+ costs $5"
```

Logic uses `&&`, `||`, and prefix `!`. `is` compares the identity of two
references, while `==` compares values.

```hd
ok := ready && !failed || forced
same := left is right
```

## Collections

`List[T]` and `Map[K, V]` come from the prelude, and tuples are built in.
Comprehensions put the clauses before `=>` and the produced value after it.

```hd
names := ["Ada", "Grace", "Linus"]
scores := {"Ada": 10, "Grace": 12}
point := (10, 20)
x, y := point

long_names := [for name in names if name.len() > 3 => name]
by_name := {for user in users => user.name: user.score}
```

## Functions

Parameters and results are typed. The last expression of a body is its
value, and `return` is only needed for an early exit. Calls can mix
positional and named arguments, with positional ones first.

```hd
fn connect(host: string, port: i32 = 443, tls: bool = true) -> string:
    if host == "":
        return "no host"
    "$host:$port"

connect("localhost", port=8080, tls=false)
```

Closures use the same `fn` syntax. Generic parameters go in square brackets,
and a bound is written with `<`.

```hd
inc := fn(x: i32) -> i32: x + 1

fn first[T](items: List[T]) -> T?:
    if items.len() == 0:
        .None
    else:
        items[0]

fn show[T < Display](value: T) -> string:
    value.to_string()
```

## Control Flow

`if`, `match`, and loops with `else` are expressions. A `match` must be
exhaustive.

```hd
label := if score >= 90:
    "excellent"
else if score >= 70:
    "passing"
else:
    "needs work"

let total: i32 = 0
for value in values:
    if value < 0:
        continue
    total = total + value

found := for name in names:
    if name.starts_with("G"):
        break name
else:
    "nobody"
```

`defer:` registers cleanup that runs when the enclosing block exits.

```hd
fn read_first!(path: string) -> Result[string, FileError] $ Files:
    let handle: mut FileHandle = $.use(Files).open!(path)?
    defer:
        _ := handle.close()
    handle.read!()
```

## Data Types and Mutability

`data` declares a nominal record. Values are shared references. A field may
have a default, and construction uses a typed literal.

```hd
data User:
    id: string
    email: string
    nickname: string? = .None

user := User { id: "u1", email: "ada@example.com" }
renamed := User { ...user, nickname: "ada" }
```

Mutation permission is part of the type: `T` is readonly and `mut T` is
mutable. A readonly reference can never be upgraded to `mut`.

```hd
fn normalize(user: mut User) -> void:
    user.email = user.email.trim().lower()

fn inspect(user: User) -> string:
    user.email

let draft: mut User = User { id: "u2", email: " Grace@Example.com " }
normalize(draft)
```

A bare type name inside a `data` block embeds that type. Its `pub` fields
and methods are promoted onto the outer type. This is composition, not
inheritance. The `...` prefix copies a value into the embedded part.

```hd
data Timestamps:
    pub created_at: i64
    pub updated_at: i64

data Post:
    Timestamps
    title: string

post := Post {
    Timestamps: ...Timestamps { created_at: 0, updated_at: 0 },
    title: "Hello",
}
println(post.created_at)
```

## Enums, Optionals, and Results

An `enum` is a closed set of variants, and a variant may carry named
payload fields. Where the expected type is known, `.Variant` names a
variant.

```hd
enum Shape:
    Circle(radius: f64)
    Rect(width: f64, height: f64)

fn area(shape: Shape) -> f64:
    match shape:
        .Circle(radius) => 3.14159 * radius * radius
        .Rect(width, height) => width * height
```

`T?` is the prelude enum `Option[T]`, with variants `.Some(value)` and
`.None`. `Result[T, E]` has `.Ok(value)` and `.Err(error)`. A plain value is
accepted where an optional is expected.

```hd
fn find(names: List[string], prefix: string) -> string?:
    for name in names:
        if name.starts_with(prefix):
            return name
    .None

match find(names, "A"):
    .Some(name) => println(name)
    .None => println("not found")
```

Optionals and results are must-use values. Discard one on purpose with
`_ := expression`.

## Errors and `?`

Postfix `?` unwraps `.Some` or `.Ok` and returns `.None` or `.Err` from the
current function. When the error types differ, `?` converts the error once
through a `From` implementation on the function's error type.

```hd
use std.convert.From

enum FsError:
    NotFound(path: string)

enum SyncError:
    Fs(error: FsError)

impl From[FsError] for SyncError:
    fn from(value: FsError) -> SyncError: SyncError.Fs(value)

fn read_config(path: string) -> Result[string, FsError]:
    .Err(FsError.NotFound(path))

fn sync(path: string) -> Result[string, SyncError]:
    config := read_config(path)?
    .Ok(config)
```

`std.error.Error` is the standard error trait. `Result[T, Error]` holds any
error that implements it, much like `anyhow` in Rust.

```hd
use std.error.Error

impl Display for FsError:
    fn to_string(self) -> string:
        match self:
            FsError.NotFound(path) => "not found: " + path

impl Error for FsError

fn load(path: string) -> Result[string, Error]:
    text := read_config(path)?
    .Ok(text)
```

## Traits

Traits describe behavior, and every implementation is explicit. A generic
bound dispatches statically. A trait used as a value type dispatches
dynamically, with no `dyn` marker.

```hd
trait Describe:
    fn describe(self) -> string

    fn label(self) -> string:
        "item: " + self.describe()

impl Describe for User:
    fn describe(self) -> string:
        self.email

fn print_all(items: List[Describe]) -> void $ Console:
    for item in items:
        println(item.label())
```

Inherent methods live in `impl Type` blocks. `@derive` generates standard
trait implementations, and `by` delegates a trait to an embedded field.

```hd
@derive(Eq, Hash)
data Point:
    x: i32
    y: i32

impl Point:
    pub fn moved(self, dx: i32) -> Point:
        Point { ...self, x: self.x + dx }

data Service:
    Logger

impl Describe for Service by Logger
```

## Inspectable Values

`std.inspect.Inspectable` erases a value while remembering its concrete
type. Only `Inspectable` values, and values of traits that extend it such
as `Error`, can be downcast back.

```hd
use std.inspect.{Inspectable, TypeId}

fn describe_value(value: Inspectable) -> string:
    if value.runtime_type() == TypeId::of[i32]():
        return "an i32"
    match value.downcast[User]():
        .Some(user) => "user " + user.email
        .None => "something else"
```

## Requirements and Providers

A requirement row after `$` lists the traits a function needs from its
context. `$.use` retrieves a provider, and `$.with` supplies providers for a
block. There is no hidden global state: a missing provider is a compile-time
error.

```hd
trait Clock:
    fn now(self) -> i64

fn stamp(message: string) -> string $ Clock:
    clock := $.use(Clock)
    "${clock.now()}: $message"

data FixedClock:
    at: i64

impl Clock for FixedClock:
    fn now(self) -> i64: self.at

$.with(Clock=FixedClock { at: 42 }):
    println(stamp("ready"))
```

## Suspension with `fn!`

A name ending in `!` marks a function that can suspend, such as for I/O or
waiting. Every call that may suspend is written with `!` too, so suspension
points are visible in review. Calling a suspending function without `!`
builds a cold `Suspend[T]` value instead of running it.

```hd
trait Database:
    fn get_user!(self, id: string) -> Result[User?, DbError]

fn load_user!(id: string) -> Result[User?, DbError] $ Database:
    $.use(Database).get_user!(id)

fn load_both!(a: string, b: string) -> Result[(User?, User?), DbError] $ Database:
    first := load_user!(a)?
    second := load_user!(b)?
    .Ok((first, second))
```

Non-suspending code drives a suspension with `std.task.block_on`.

```hd
use std.task.block_on

fn load_now(id: string) -> Result[User?, DbError] $ Database:
    let pending: mut Suspend[Result[User?, DbError]] = load_user(id)
    block_on(pending)
```

## Modules, Entry Points, and Tests

Each file is a module named by its path under `src/`, and `mod.hd` indexes a
directory. Declarations are private unless marked `pub`. `use` roots are
`pkg`, `std`, `dep`, `self`, and `super`.

```hd
use pkg.user.types.{User, UserId}
use std.host.Args

pub fn main!() -> Result[void, ConsoleError] $ Args, Console:
    args, console := $.use(Args, Console)
    console.write_line!("starting " + args.program_name())?
    .Ok()
```

A `test` block runs in a fresh program instance and may call suspending
functions directly. Provider scopes replace dependencies with mocks.

```hd
use std.testing.assert_equal

test "stamps with the fixed clock":
    $.with(Clock=FixedClock { at: 1 }):
        assert_equal(stamp("go"), "1: go", reason="uses the provider")
```

## Annotations

Annotations attach typed metadata to declarations and derive information
for whole types, such as validators, schemas, or tool descriptions. The
result is an ordinary runtime value.

```hd
@Validation
data Signup:
    @max_len(80)
    display_name: string
    email: string

validator := Validation::annotation(Signup)
```

## Where Next

- The [Language Tour](LANGUAGE_TOUR.md) explains each feature with more
  examples and design notes.
- The [Language Overview](OVERVIEW.md) covers the purpose and priorities of
  the design.
- The [specification](../spec/README.md) is the precise reference, starting
  with the [Type System](../spec/04-type-system.md) and
  [Requirements and Suspension](../spec/11-requirements-and-suspension.md).
