# Pipe rule: Elixir style

`x |> f(a, b)` calls `f(x, a, b)`: the piped value becomes the FIRST argument.
`x |> f` (a bare name) calls `f(x)`.
Every `sh` function takes its subject (the string or list it works on) as its first parameter, exactly as listed on the sheet.

    text |> lines |> filter(fn(l: string) -> bool: contains(l, "x")) |> count
    # = count(filter(lines(text), fn(l: string) -> bool: contains(l, "x")))
