# Pipe rule: F# style

`x |> e` evaluates `e` to a function and calls it with `x`.
`x |> f` (a bare name) calls `f(x)`. `x |> f(a)` calls `f(a)`, which returns a function, with `x`: `f(a)(x)`.

So in this variant the `sh` functions that take more than one argument are CURRIED: they take every argument except the subject, and return a function of the subject. For example:
- `filter(keep)` returns `fn(List[T]) -> List[T]`
- `map(f)` returns `fn(List[T]) -> List[U]`
- `split(sep)`, `pad(width)`, `starts_with(prefix)`, `ends_with(suffix)`, `contains(part)`, `join(sep)`, `take(n)`, `sort_by(key)`, `max_by(key)`, `at(i)` likewise.
- One-argument functions (`lines`, `words`, `trim`, `upper`, `lower`, `reverse_text`, `length`, `sort`, `reverse`, `unique`, `count`, `sum`, `average`, `keep_some`, `parse_i64`, `parse_f64`) are plain: `f(x)`.
- I/O functions are as on the sheet.

    text |> lines |> filter(fn(l: string) -> bool: contains("x")(l)) |> count
    # = count(filter(fn(l: string) -> bool: contains("x")(l))(lines(text)))

(Note: a curried function called directly, outside a pipe, needs both calls: `contains("x")(l)`.)
