# How To: Translate Habits From Other Languages

**Scenario.** You know how to write it in Python, TypeScript, Rust or Go.
Find the habit on the left; the right column is the hd form, with the page
that explains it.

## Data

| You would write | In hd |
| --- | --- |
| a class with fields and methods | `struct` plus `impl` ([data](../language/data.md)) |
| a constructor, `__init__`, `new` | a literal `Point { x: 1, y: 2 }`, or an associated function `Point::new(...)` |
| inheritance, an interface, a protocol | a `trait`, with an explicit `impl Trait for Type` ([traits](../language/traits.md)) |
| fields shared through a base class | a struct of the shared fields, held as a field; hd has no inheritance |
| a tagged union, a sealed class, a Rust `enum` | `enum` with payloads, read with `match` |
| a dataclass copy with changes, `{...obj, x: 1}`, `..base` | `Point { ...base, x: 1 }`, spread first |
| `Vec<T>`, `list`, `T[]` | `List[T]`, literal `[1, 2]` |
| `HashMap`, `dict`, `Record`, `map[K]V` | `Map[K, V]`, literal `{"k": v}` |
| a tuple, `pair.0`, `pair[0]` | `(a, b)`, read with `pair._0` or `let (a, b) = pair` |
| `null`, `None`, `nil`, `undefined`, `Option<T>` | `T?` with `.Some(x)` and `.None` ([errors](../language/errors.md)) |
| `x as i32`, `int(x)`, `Number(x)` | `i32(x)`; to parse text, `text.parse::[i32]()` |
| `Rc<RefCell<T>>`, `Arc<Mutex<T>>`, any object reference | `box(value)`, a `*mut T` ([references](../language/references.md)) |
| `&mut x` in Rust, a pointer argument in Go | `&mut x`, with the variable declared `let mut` |

## Control flow

| You would write | In hd |
| --- | --- |
| `{ ... }` blocks, `end`, `;` | `header:` and indentation |
| `elif`, `else if` | `else if` |
| `a if c else b`, `c ? a : b` | `if c: a else: b` |
| `switch`, `match` with `A \| B` arms | `match`, one pattern per arm, `_` for the rest |
| `for i in range(n)`, `for (let i = 0; i < n; i++)` | `for i in 0..n:` |
| `for i, x in enumerate(xs)` | `for (i, x) in xs.iter().enumerate():` |
| `while True`, `loop {}`, `for {}` | `while true:` |
| `[f(x) for x in xs if c]`, `xs.filter(c).map(f)` | `[for x in xs if c => f(x)]`, or `xs.filter(c).map(f)` |
| `lambda x: x + 1`, `x => x + 1`, `\|x\| x + 1` | `fn(x): x + 1` |
| `not`, `and`, `or` | `!`, `&&`, `\|\|` |

## Errors

| You would write | In hd |
| --- | --- |
| `raise`, `throw` | `return .Err(MyError.Reason)` from a function that returns `Result[T, MyError]` |
| `try` / `except`, `try` / `catch` | `match call():` with `.Ok(v)` and `.Err(e)` arms |
| letting an exception propagate, `if err != nil { return err }` | `value := call()?` |
| an exception class hierarchy | an `@error enum` with one variant per failure, `@from` to wrap another error |
| `unwrap()`, `!`, a non-null assertion | `expect("why it is present")` |
| `x or default`, `x ?? default`, `unwrap_or` | `x.unwrap_or(default)` |
| `finally`, `with`, Go's `defer` | `defer(fn() -> void: cleanup())`, with `use std.ops.defer` |
| `assert` in program code | `if !ok: panic("...")`; `assert` exists in test code only |

## Text and output

| You would write | In hd |
| --- | --- |
| `f"{name} is {age}"`, `` `${name}` ``, `format!("{}", x)` | `"$name is ${age + 1}"` |
| `print(x)`, `console.log(x)`, `println!` | `println("$x")`, in a function with `$ Console` |
| printing a list or struct for debugging | `println(debug(value))`, with `@derive(Debug)` on your types |
| `len(s)` for characters | `s.chars().count()`; `s.len()` is bytes |
| `s[1:4]`, `s.substring(1, 4)`, `&s[1..4]` | `s.slice(1, 4)` |
| `s.upper()`, `toUpperCase()`, `to_uppercase()` | `s.upper()`, `s.lower()` |
| `",".join(xs)`, `xs.join(",")` | `xs.join(",")` |
| `s += piece` in a loop | `StringBuilder` or `parts.join("")` ([text](../std/text.md)) |

## Programs

| You would write | In hd |
| --- | --- |
| `async def` / `await`, `async fn` / `.await` | name the function with `!` and call it with `!`: `fn load!()`, `load!()` ([suspension](../language/suspension.md)) |
| `asyncio.gather`, `Promise.all`, a `WaitGroup` | `join!(a(), b())` |
| `Promise.race`, `select` | `race!(a(), b())` |
| a global, a singleton, a dependency-injection container | a trait in the row, `$.use(Trait)`, supplied with `$.with(Trait=value):` ([requirements](../language/requirements.md)) |
| `mock.patch`, a mocking library | a small struct that implements the trait, installed with `$.with` ([fakes](../testing/fakes.md)) |
| `import x`, `use x::y`, `require` | `use std.fs.{read_text}`, `use pkg.cart.total`, at the top of the file |
| `public` / `private`, `pub`, an exported name | `pub`; everything else is private to its file |
| `if __name__ == "__main__"`, `fn main()`, `func main()` | `pub fn main() -> void $ Console:` |
| `sys.argv`, `process.argv`, `os.Args` | `args()` from `std.cli`, with `$ Args` |
| `os.environ`, `process.env` | `env("NAME")` from `std.host`, with `$ Env` |
| `open(path).read()`, `fs.readFile` | `read_text!(Path(path))?`, with `$ FsRead` |
| `subprocess.run`, `exec.Command` | `Command::new("git").args([...]).run!()`, with `$ Process` |
| `pytest`, `#[test]`, `describe` / `it` | a `tests:` block with `it("name"):` cases ([unit tests](../testing/unit-tests.md)) |

## One example, side by side

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

def total(accounts):
    return sum(a.cents for a in accounts if a.cents > 0)
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

fn total(accounts: List[Account]) -> i64:
    [for account in accounts if account.cents > 0 => account.cents].sum()

let mut ada = Account { owner: "Ada", cents: 500 }
match ada.withdraw(900):
    .Ok(_) => println("done")
    .Err(error) => println("refused: $error")
_ := ada.withdraw(200)
println(total([ada, Account { owner: "Grace" }]))
```

```output
refused: Ada has only 500
300
```

What changed: the fields and their types are declared; the failure is a
value in the return type instead of an exception; the method that changes
the account takes `&mut self`, so the variable is `let mut`; and the
result of `withdraw` must be used or discarded with `_ :=`.
