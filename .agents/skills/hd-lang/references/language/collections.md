# Collections: Lists, Maps, Tuples, Loops, Iterators

`List[T]` and `Map[K, V]` need no import. Both are **values**: `b := a`
copies, and changing one never changes the other. A collection that is
changed in place is a `let mut` variable. The full method tables are in
[std/collections.md](../std/collections.md).

## Lists

```hd
let mut names: List[string] = []        # an empty literal needs a type
names.push("Grace")
names.push("Ada")
names.insert(0, "Linus")
first := names[0]                       # panics when out of range
maybe := names.get(9)                   # `string?`: `.None` when out of range
more := [names..., "Barbara"]           # spread into a new list
sorted := names.sorted()                # a new list; `names.sort()` sorts in place
head := sorted.slice(0, 2)              # a new list; there is no `xs[a..b]`
removed := names.remove_at(0)
println("$first ${debug(maybe)} ${more.len()} ${head.join(",")} $removed ${names.contains("Ada")}")
```

```output
Linus Option.None 4 Ada,Grace Linus true
```

## Maps

`{}` is the empty map and `{"key": value}` a literal. A key type needs `Eq`
and `Hash`; strings, numbers and `@derive(Eq, Hash)` types have them.

```hd
let mut stock: Map[string, i32] = {"tea": 3}
stock["cake"] = 1                       # insert or replace
stock["tea"] += 2                       # the key must exist, or it panics
*stock.get_or_insert("jam", 0) += 5     # insert the default when missing, then change
tea := stock.get("tea").unwrap_or(0)    # `get` gives `i32?`
gone := stock.remove("cake")
println("$tea ${debug(gone)} ${stock.contains_key("cake")} ${stock.len()} ${stock["jam"]}")
```

```output
5 Option.Some(1) false 2 5
```

Counting is the common shape: `counts[word] = counts.get(word).unwrap_or(0) + 1`.

## Tuples

```hd
pair := (+1, "one")
let (number, word) = pair               # destructuring uses `let`
println("${pair._0} ${pair._1} $number $word")
```

```output
1 one 1 one
```

## Loops

```hd
scores := {"ada": +10, "grace": +12}
names := ["ada", "grace"]

for name in names:                      # a list loop gives each element, as a copy
    println(name)

let total = +0
for (name, score) in scores:            # a map loop lends `(&K, &V)`
    total += score                      # operators read through a reference
    if *score > 10:                     # `*score` copies the value out
        println("high: $name")

for i in 0..3:                          # 0, 1, 2; `0..=3` includes 3
    println("$i")

for (index, name) in names.iter().enumerate():
    println("$index $name")

let mut prices = [+100, 250]
for price in &mut prices:               # `&mut`: change each element in place
    *price += 1

let n = 0
while n < 3:                            # there is no `loop`; write `while true:`
    n += 1
println("$total ${debug(prices)} $n")
```

```output
ada
grace
high: grace
0
1
2
0 ada
1 grace
22 [101, 251] 3
```

- A loop over a list or map walks the collection as it was when the loop
  began. Changing the collection inside the loop does not change the loop.
- `break` and `continue` work as usual. A loop with `else` is an expression
  ([functions.md](functions.md)).
- A value from a map loop is a reference. Pass `*score` where a function
  wants the value itself: `max(best, *score)`.

## Comprehensions

Clauses come first, then `=>`, then the produced value.

```hd
names := ["Ada", "Grace", "Linus"]
long := [for name in names if name.len() > 3 => name.upper()]
lengths := {for name in names => name: name.len()}
pairs := [for a in [1, 2] for b in [10, 20] => a * b]
println("${debug(long)} ${lengths["Ada"]} ${debug(pairs)}")
```

```output
["GRACE", "LINUS"] 3 [10, 20, 20, 40]
```

## Lazy iterators

`iter()` starts a lazy chain. Adapters such as `map` and `filter` describe
work; a draining call such as `collect`, `sum`, `count` or a `for` loop
runs it. A `filter` callback takes a reference, and a `map` callback the
value.

```hd
amounts := [+800, 1200, 2500]
let fees: List[i32] = amounts.iter().filter(fn(a): a >= 1000).map(fn(a): a / 10).collect()
total := amounts.iter().sum::[i32]()
big := amounts.iter().any(fn(a): a > 2000)
indexed := amounts.iter().enumerate().take(2).collect::[List[(usize, i32)]]()
println("${debug(fees)} $total $big ${debug(indexed)}")
```

```output
[120, 250] 4500 true [(0, 800), (1, 1200)]
```

`collect`, `sum` and `product` need to know their result type: give it
with a `let` annotation, the function's return type, or `::[T]`. Without
one the error is `cannot-infer-type`. A list also has eager helpers that
skip `iter()`: `xs.map(f)`, `xs.filter(f)`, `xs.any(f)`, `xs.find(f)`.
