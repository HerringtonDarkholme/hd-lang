# Values And References

## Values copy

A struct, enum, tuple, `List`, `Map` and `string` are values. Assigning,
passing or returning one gives an independent copy.

```hd
struct Order:
    id: string
    cents: i32

let mut order = Order { id: "A-1", cents: 5000 }
saved := order                  # a copy
order.cents += 500              # changes `order` only
let mut a = [1, 2]
b := a                          # a copy of the list
a.push(3)
println("${saved.cents} ${order.cents} ${a.len()} ${b.len()}")
```

```output
5000 5500 3 2
```

So a function that gets a plain `List[T]` or struct and changes it changes
only its own copy. To change the caller's value, or to share one object,
write a reference.

## Which reference

| Type | Made with | Use it for |
| --- | --- | --- |
| `&T` | `&x` | reading a value in place for one call, with no copy |
| `&mut T` | `&mut x` | letting one call change the caller's variable |
| `*T`, `*mut T` | `box(value)` | an object that is shared or kept: a field, a list element, a return value |

## Local references: `&T` and `&mut T`

```hd
struct Stock:
    units: i32

fn units(stock: &Stock) -> i32:
    stock.units                     # field access goes through a reference

fn restock(stock: &mut Stock, added: i32) -> void:
    stock.units += added

fn add_all(target: &mut List[i32], more: List[i32]) -> void:
    for value in more:
        target.push(value)

let mut local = Stock { units: 2 }
restock(&mut local, 3)              # the call site writes `&mut` too
let mut numbers: List[i32] = [1]
add_all(&mut numbers, [2, 3])
println("${units(&local)} ${numbers.len()}")
```

```output
5 3
```

- `&mut x` needs a writable place: a `let mut` variable, a field of one, or
  something reached through a `&mut`. A `:=` binding, a plain `let` and a
  plain parameter are not writable (`not-writable`).
- A method call needs no mark: `local.restock(3)` lends the receiver by
  itself when the method takes `&mut self`.
- `*r` reads the whole value as a copy, and `*r = value` replaces it.
- Operators read through a reference: with `x: &i32`, `x + 1` and `x > 0`
  use the value.
- A local reference lasts for the call. It cannot be put in a field or a
  collection, or captured by a closure that is kept
  (`local-reference-escapes`).
- While a reference into a variable is still used, the variable cannot be
  changed another way (`conflicting-access`): finish with the reference
  first.

## Shared objects: `box`

`box(value)` puts a value in a new object and gives a `*mut T`. Copying
the reference shares the object, and the object lives while a reference
points to it.

```hd
struct Account:
    owner: string
    cents: i64

struct Bank:
    accounts: List[*mut Account]    # a field or element holds `*T` or `*mut T`

fn deposit(account: *mut Account, cents: i64) -> void:
    account.cents += cents

ada := box(Account { owner: "Ada", cents: 0 })
bank := Bank { accounts: [ada] }
deposit(ada, 500)
bank.accounts[0].cents += 250       # the same object
alias := ada
println("${ada.cents} ${alias.owner} ${ada is alias}")
```

```output
750 Ada true
```

- `box` copies its argument. Box at the declaration,
  `x := box(Point { ... })`, never `box(y)` on a variable you keep using:
  the box and `y` would then differ.
- A `*mut T` is accepted where `*T`, `&mut T` or `&T` is expected.
- `a == b` on two boxes is the error `reference-equality`. Write `a is b`
  for "the same object", or `*a == *b` for "equal values".
- To give out read-only access to an object, type the field or the return
  value `*T`.

Standard builders and fakes return boxes for you: `StringBuilder::new()`,
`MemoryConsole::new()`, `Command::new("git")`.

## Keeping a closure

A `fn(...)` value never holds a local reference: it captures values, shared
variables and counted references (`*T`), never a `&` or `&mut`. So a plain
`fn(...)` field, list element or map value can keep any function value,
including one that arrived as a parameter:

```hd
struct Button:
    label: string
    on_click: fn() -> void

fn button(label: string, on_click: fn() -> void) -> Button:
    Button { label, on_click }      # storing a fn parameter is fine

let clicks = 0
buttons := [button("ok", fn() -> void: clicks += 1), button("noop", fn() -> void: pass)]
for b in &buttons:
    (b.on_click)()                  # calling a field needs parentheses
(buttons[0].on_click)()
println("$clicks ${buttons[0].label}")
```

```output
2 ok
```

A closure that borrows a local (`&p`, or a `&`/`&mut` parameter) is not a
`fn(...)` value. It can only be passed straight to a parameter typed
`impl Fn(...)` (or a generic `F < Fn(...)`), which calls it and does not keep
it. To keep it instead, capture a copy (`p := *p`) or a counted reference.

## Changing elements

```hd
struct Task:
    name: string
    done: bool

let mut tasks = [Task { name: "write", done: false }, Task { name: "test", done: false }]
tasks[0].done = true                # through an index
for task in &mut tasks:             # or each element in place
    task.name = task.name.upper()
finished := [for task in tasks if task.done => task.name]
println("${finished.join(",")} ${tasks[1].name}")
```

```output
WRITE TEST
```

A plain `for task in tasks` gives copies: writing `task.done = true` there
is `not-writable`. Use `for task in &mut tasks`, or store `*mut Task`
elements.
