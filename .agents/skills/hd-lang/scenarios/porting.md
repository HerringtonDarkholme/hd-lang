# Porting from Go, Python, Rust and TypeScript

**Scenario.** You have working code in another language and want the same
program in hd. This page lists what changes, in the order a port hits it:
value semantics first, then dependencies, errors, contracts, waiting, and
stored callbacks. Syntax habits come last, as a lookup table.

## Values copy: sharing is explicit

In Go, Python and TypeScript, assigning a slice, map, list or object shares
it. In hd a struct, enum, tuple, `List`, `Map` and `string` are values:
assigning, passing and returning one gives a copy. The rules are in
[references](../reference/references.md). What to write for each source habit:

| Source habit | In hd |
| --- | --- |
| Go `*T` receiver, a pointer argument; Rust `&mut self`, `&mut T` | `&mut self`, `&mut T`; the variable is `let mut`, the call site writes `&mut x` |
| Go `*T` or Python object stored in two places; Rust `Rc<RefCell<T>>`, `Arc<Mutex<T>>` | `box(value)`, a `*mut T`, held by each owner |
| Go `[]T` or `map[K]V` aliased by two variables; a Python list passed in and appended to | a `List` or `Map` you lend as `&mut List[T]`, or share as `*mut List[T]` made with `box([...])` |
| Go `append(s, x)` returning a slice; TypeScript `xs = [...xs, x]` | `xs.push(x)` on a `let mut` list; the copy is lazy, so this is not a cost |
| Go `s[1:3]` that shares memory; Python `xs[1:3]` | `xs.slice(1, 3)` is a copy; `xs.view(1, 3)` reads without copying ([performance](performance.md)) |
| a field that points back to its parent, a linked list | `(*mut Node)?` for one link; a `List[Node]` field for children |
| Rust `&T` held in a struct, a lifetime parameter | store the value, or a `*T`; a `&T` lasts for one call only |
| Rust `Box<T>` for a recursive type | not needed for `List[Tree]` fields; `box` only when the object is shared |

A shared-mutable-state port. Python, where two objects append to one list:

```text
class Logger:
    def __init__(self, lines):
        self.lines = lines
    def note(self, text):
        self.lines.append(text)

class Cart:
    def __init__(self, log):
        self.log = log
        self.items = []
    def add(self, item):
        self.items.append(item)
        self.log.append(f"added {item}")

shared = ["start"]
logger = Logger(shared)
cart = Cart(shared)
```

In hd the list is boxed once, and each holder keeps the `*mut`. A plain
`List[string]` field would give each holder its own copy, so the log would
never merge:

```hd
struct Logger:
    lines: *mut List[string]

struct Cart:
    log: *mut List[string]
    items: List[string]

impl Logger:
    fn note(&self, text: string) -> void:
        self.lines.push(text)

impl Cart:
    fn add(&mut self, item: string) -> void:
        self.items.push(item)
        self.log.push("added $item")

pub fn main() -> void $ Console:
    shared := box(["start"])
    logger := Logger { lines: shared }
    let mut cart = Cart { log: shared, items: [] }
    cart.add("tea")
    logger.note("checked out")
    println(shared.join("; "))
    # a Go map or slice alias becomes a copy
    let mut a: Map[string, i32] = {"x": 1}
    b := a
    a["y"] = 2
    println("${a.len()} ${b.len()}")
```

```output
start; added tea; checked out
2 1
```

What changed: `box(["start"])` makes the one shared object. `Logger::note`
needs only `&self`, because writing through a `*mut` field does not change
`Logger`. When one function at a time needs the data, skip the box and lend
it with `&mut`. [Shared state](shared-state.md) compares the choices.

## Stored callbacks become data

A closure received as a parameter cannot be stored in a field
([limitations](../reference/limitations.md)). Named functions and closures
written in place can be stored in a `fn` field. Ports of event handlers,
command tables and visitor registries hit this. Write the choices as an
`enum` and one function that acts on it:

```hd
enum Action:
    Add(amount: i32)
    Reset
    Say(text: string)

struct Button:
    label: string
    action: Action

struct Counter:
    total: i32

impl Counter:
    fn apply(&mut self, action: &Action) -> void $ Console:
        match action:
            .Add(amount) => self.total += amount
            .Reset => self.total = 0
            .Say(text) => println(text)

pub fn main() -> void $ Console:
    buttons := [
        Button { label: "+5", action: .Add(5) },
        Button { label: "hello", action: .Say("hello") },
        Button { label: "clear", action: .Reset },
    ]
    let mut counter = Counter { total: 0 }
    for button in &buttons:
        counter.apply(&button.action)
        println("${button.label} -> ${counter.total}")
```

```output
+5 -> 5
hello
hello -> 5
clear -> 0
```

The enum can be compared, printed and serialized with derives; a closure
cannot. When callers must supply arbitrary code, store a `fn(...)` field, as in
[keeping a closure](../reference/references.md#keeping-a-closure).

## Features that map to one hd feature

| Source habit | In hd | Read |
| --- | --- | --- |
| a dependency-injection container, a global, a singleton, a Go interface passed to every constructor | a trait in the function's row (`$ Clock`), used with `$.use`, supplied with `$.with` | [effects](../design/effects.md) |
| `mock.patch`, gomock, `jest.mock` | `MemoryFs`, `MemoryConsole`, `ManualClock`, `ScriptedHttp`, or your own struct that implements the trait | [testing](../design/testing.md) |
| decorators, Rust derive and attribute macros, Go struct tags, `serde` | `@derive(...)` with field facts; your own derive template for a new trait | [derive](../design/derive.md) |
| `raise`, `throw`, `try`/`except`, Go `if err != nil`, Rust `?` | `Result[T, E]`, an `@error enum`, `?` | [errors](../design/errors.md) |
| `assert`, `debug_assert!`, a guard clause that panics | `require` for the caller's duty, `ensure` for the function's promise | [contracts](../design/contracts.md) |
| `async`/`await`, goroutines, promises, `asyncio.gather` | `!` functions, `join!`, `race!` | [concurrency](../design/concurrency.md) |
| a program that reads `os.environ`, files or sockets freely | a grant in `hd.toml` | [capabilities](../design/capabilities.md) |

Two details that surprise ports. A Go zero value does not exist: give each
field a default (`cents: i64 = 0`) or write every field. A Python
`assert` in program code becomes `require`; the `assert` statement exists
in test code only.

## A class, ported

Python:

```text
class Account:
    def __init__(self, owner):
        self.owner = owner
        self.cents = 0

    def withdraw(self, cents):
        if cents > self.cents:
            raise ValueError(f"{self.owner} has only {self.cents}")
        self.cents -= cents

```

hd:

```hd
struct Account:
    owner: string
    cents: i64 = 0

@error
enum AccountError:
    @error("$owner has only $cents")
    Insufficient(owner: string, cents: i64)

impl Account:
    fn withdraw(&mut self, cents: i64) -> Result[void, AccountError]:
        if cents > self.cents:
            return .Err(.Insufficient(self.owner, self.cents))
        self.cents -= cents
        .Ok(())

let mut ada = Account { owner: "Ada", cents: 500 }
match ada.withdraw(900):
    .Ok(_) => println("done")
    .Err(error) => println("refused: $error")
_ := ada.withdraw(200)
println(ada.cents)
```

```output
refused: Ada has only 500
300
```

What changed: the failure is a value in the return type, the method that
changes the account takes `&mut self` so the variable is `let mut`, and the
result of `withdraw` must be used or discarded with `_ :=`.

## More habits

The core syntax habits (blocks, `for`, comprehensions, lambdas,
interpolation, `Option`, derives, `main`) are in
[language](../language.md#habits-from-other-languages). The rest:

| You would write | In hd |
| --- | --- |
| a TypeScript `interface`, a Go interface, a Rust trait | a `trait`, with an explicit `impl Trait for Type` ([traits](../reference/traits.md)) |
| `serde`, `json.loads` into a dataclass, `JSON.parse` with a type, `encoding/json` tags | `@derive(Serialize, Deserialize)`, then `decode::[T](text)` and `encode(value)`; `@rename("key")` for a tag |
| pydantic, zod, `validator` tags | `@derive(Validate)` with `@range(1, 99)`, `@length(1, 80)`, `@email()`, then `value.validated()` ([configuration](configuration.md)) |
| `for i, x in enumerate(xs)` | `for (i, x) in xs.enumerate():` |
| `try` / `except`, `try` / `catch` | `match call():` with `.Ok(v)` and `.Err(e)` arms |
| `unwrap()`, `!`, a non-null assertion | `expect("why it is present")` |
| `finally`, `with`, Go `defer` | `defer(fn() -> void: cleanup())`, with `use std.ops.defer` |
| `len(s)` for characters | `s.chars().count()`; `s.len()` is bytes |
| `s[1:4]`, `s.substring(1, 4)`, `&s[1..4]` | `s.slice(1, 4)` |
| `s += piece` in a loop | `s += piece` is fine; `s = "$s$piece"` copies each time ([performance](performance.md#building-text)) |
| named capture groups into a struct | named groups, then `pattern.decode::[T](line)` ([std](../std.md)) |
| `argparse`, `click`, `clap`, `commander`, `flag` | `@derive(Cli)`, then `Options::parse_args()`, with `$ Args` ([cli-tool](cli-tool.md)) |
| `sys.argv`, `process.argv`, `os.Args` | `args()` from `std.cli`, with `$ Args` |
| `os.environ`, `process.env`, `envconfig` | `env_decode::[Settings](prefix="APP_")` from `std.host`, with `$ Env`; one variable is `env("NAME")` |
| `open(path).read()`, `fs.readFile` | `read_text!(Path(path))?`, with `$ FsRead` |
| `subprocess.run`, `exec.Command` | `Command::new("git").args([...]).run!()`, with `$ Process` ([run-commands](run-commands.md)) |
| `pytest`, `#[test]`, `describe` / `it`, `proptest` | a `tests:` block with `it`, `it_each`, `it_prop` ([testing](../design/testing.md)) |
| reading a large input as a stream | not yet: input is read whole ([limitations](../reference/limitations.md)) |
