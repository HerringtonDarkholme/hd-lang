t01: `contains` is curried, so I wrote `l |> contains("ERROR")` inside the closure; unsure the pipe form is what the rule intends for a closure body.
t02: `args()` is listed under "bang functions" but written without `!` in the sheet; I kept it non-bang and used `main()`.
t03: `join(", ")` and `map(trim)` are plain; no open question.
t04: `sort_by(length)` assumed ascending, then `reverse`; tie order unspecified; lines not trimmed.
t05: no column selector, so I used `ends_with(",yes")` (assumes `active` is the last column) and `split` plus `at(0)` with `keep_some`.
t06: `run!` args written as a list literal; assumed exit status is ignored; leading space in `" M"` is intentional.
t07: no header skip; two optional steps needed per column; `average` on an empty list is undefined.
t08: `pad` assumed to pad on the right to width 10 with no truncation; `args()` non-bang as in t02.
t09: `files()` is listed under bang functions but written without `!`; kept non-bang as the task writes it.
t10: `reverse_text` assumed character-based; requirement row uses `$ ConsoleInput + Console`.
t11: `max_by` tie-breaking unspecified; `length` returns `usize` and interpolates directly.
t12: `words` splits on spaces only, so tabs or punctuation stay attached; `unique` assumed to keep first occurrence.
