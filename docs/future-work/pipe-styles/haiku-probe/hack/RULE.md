# Pipe rule: Hack style

Every pipe step must contain the placeholder `_` exactly once; `_` is the piped value. There are no bare steps.

    text |> lines(_) |> filter(_, fn(l: string) -> bool: contains(l, "x")) |> count(_)
    # = count(filter(lines(text), fn(l: string) -> bool: contains(l, "x")))

The `sh` functions take their arguments exactly as listed on the sheet (subject first).
