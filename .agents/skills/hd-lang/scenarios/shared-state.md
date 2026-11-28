# Share And Update State

**Scenario.** Data that more than one part of a program must see change:
a running total filled in by a helper, a cache used by two functions, a
tree whose nodes point at each other.

hd copies values by default, so each case is a choice of **who else must
see the change**.

| The change must be seen by | Use |
| --- | --- |
| nobody else | a plain value; return the new one |
| the caller, for the length of one call | `&mut T` |
| several holders, for as long as they live | `box(...)`, a `*mut T` |
| any function down the call chain, without passing it | a provider: `$.with` and `$.use` ([effects](../design/effects.md)) |

## Return a new value

```hd
fn with_tax(prices: List[i32], percent: i32) -> List[i32]:
    prices.map(fn(price): price + price * percent / 100)

prices := [+100, 250]
taxed := with_tax(prices, 10)
println("${debug(prices)} ${debug(taxed)}")
```

```output
[100, 250] [110, 275]
```

## Let one call change the caller's value: `&mut`

```hd
struct Report:
    lines: List[string]
    errors: usize

fn check(report: &mut Report, name: string, ok: bool) -> void:
    if !ok:
        report.errors += 1
    report.lines.push(if ok: "ok   $name" else: "FAIL $name")

let mut report = Report { lines: [], errors: 0 }     # `let mut`: it will be lent
check(&mut report, "disk", true)
check(&mut report, "network", false)
println("${report.lines.join("; ")} (${report.errors} failed)")
```

```output
ok   disk; FAIL network (1 failed)
```

## Several holders of one object: `box`

```hd
struct Cache:
    hits: usize
    entries: Map[string, string]

struct Search:
    cache: *mut Cache               # a kept reference is `*mut T`, never `&mut T`

struct Preview:
    cache: *mut Cache

fn lookup(cache: *mut Cache, key: string) -> string:
    match cache.entries.get(key):
        .Some(found) =>
            cache.hits += 1
            found
        .None =>
            value := key.upper()    # stands in for slow work
            cache.entries[key] = value
            value

cache := box(Cache { hits: 0, entries: {} })    # box where it is declared
search := Search { cache: cache }
preview := Preview { cache: cache }
first := lookup(search.cache, "tea")
second := lookup(preview.cache, "tea")           # the same object: a hit
println("$first $second ${cache.hits}")
```

```output
TEA TEA 1
```

## Objects that point at each other

A struct cannot contain itself by value, so a tree holds its children
through references.

```hd
struct Folder:
    name: string
    size: u64
    children: List[*mut Folder]

fn add(parent: *mut Folder, name: string, size: u64) -> *mut Folder:
    child := box(Folder { name: name, size: size, children: [] })
    parent.children.push(child)
    child

fn total(folder: *Folder) -> u64:
    let sum = folder.size
    for child in folder.children:
        sum += total(child)
    sum

root := box(Folder { name: "/", size: 1, children: [] })
docs := add(root, "docs", 10)
_ := add(docs, "drafts", 100)
_ := add(root, "tmp", 1000)
println("${total(root)} ${total(docs)} ${root.children.len()}")
```

```output
1111 110 2
```

## State reached from anywhere: a provider

When a counter or a registry would have to be passed through every
function, make it a requirement instead.

```hd
pub trait Metrics:
    fn count(&mut self, name: string) -> void

struct Counters:
    seen: Map[string, usize]

impl Metrics for Counters:
    fn count(&mut self, name: string) -> void:
        *self.seen.get_or_insert(name, 0) += 1

fn parse(line: string) -> usize $ Metrics:
    $.use(Metrics).count("lines")
    if line == "":
        $.use(Metrics).count("blank")
    line.len()

pub fn main() -> void $ Console:
    counters := box(Counters { seen: {} })
    $.with(Metrics=counters):
        for line in ["a", "", "bc"]:
            _ := parse(line)
    println("${counters.seen["lines"]} lines, ${counters.seen["blank"]} blank")
```

```output
3 lines, 1 blank
```

## What goes wrong

- Changing a copy and expecting the original to change: `b := a` copies,
  and so does passing a plain struct or list to a function.
- `box(existing_variable)` copies the variable. Box the literal.
- Putting a `&mut` in a field: it is `local-reference-escapes`. A kept
  reference is a box.
- Changing a list while a reference into it is still used:
  `conflicting-access`. Finish with the reference first, or use an index.

More in [references](../reference/references.md). Tasks that share a `box` need no lock: see [concurrency](../design/concurrency.md).
