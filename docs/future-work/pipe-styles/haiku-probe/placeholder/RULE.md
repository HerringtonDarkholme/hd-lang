# Pipe rule: placeholder style

`x |> f` (a bare name) calls `f(x)`.
`x |> f(a, _)` puts the piped value where `_` is: `f(a, x)`. A step with arguments must contain `_`.
Outside pipes, `_` in a call argument is shorthand for a one-parameter function: `map(xs, pad(_, 10))` is `map(xs, fn(s: string) -> string: pad(s, 10))`. The `_` belongs to the innermost call that contains it.

    text |> lines |> filter(_, fn(l: string) -> bool: contains(l, "x")) |> count
    # = count(filter(lines(text), fn(l: string) -> bool: contains(l, "x")))

The `sh` functions take their arguments exactly as listed on the sheet (subject first).
