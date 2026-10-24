# The `sh` functions (hypothetical std module, `use std.sh.*` is implied)

Text and collections:
- `lines(text)`: the lines of a string, as `List[string]`
- `words(text)`: split on spaces, as `List[string]`
- `split(text, sep)`: split on `sep`, as `List[string]`
- `trim(s)`, `upper(s)`, `lower(s)`, `reverse_text(s)`: string to string
- `pad(s, width)`: pad `s` with spaces on the right to `width`
- `starts_with(s, prefix)`, `ends_with(s, suffix)`, `contains(s, part)`: bool
- `length(s)`: character count, `usize`
- `filter(xs, keep)`: keep elements where `keep(x)` is true
- `map(xs, f)`: apply `f` to each element
- `sort(xs)`, `sort_by(xs, key)`, `reverse(xs)`, `unique(xs)`: lists
- `take(xs, n)`: the first `n` elements
- `count(xs)`: `usize`
- `sum(xs)`, `average(xs)`: numbers (`average` of `List[f64]` is `f64`)
- `max_by(xs, key)`: the element with the largest `key(x)`, as an optional
- `join(xs, sep)`: one string
- `at(xs, i)`: the element at `i`, as an optional
- `parse_i64(s)`, `parse_f64(s)`: optionals
- `keep_some(xs)`: from `List[T?]`, the present values

I/O (bang functions):
- `read_text!(path)`: `string` (panics on error, for brevity) `$ FsRead`
- `read_stdin!()`: all of standard input as `string` `$ ConsoleInput`
- `run!(command, args)`: the command's standard output as `string` `$ Process`
- `args()`: the program arguments, `List[string]` `$ Args`
- `files()`: `List[string]` of paths `$ FsRead`
- `println(x)`: print a line `$ Console`

Closures are written `fn(x: string) -> bool: x.len() > 3`. Lists are `List[T]`, optionals are `T?` (`.Some(x)` / `.None`).
