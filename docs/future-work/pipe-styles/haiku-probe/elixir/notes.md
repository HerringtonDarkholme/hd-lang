t01: Unsure whether a bang call (read_text!) may start a |> chain, and whether read_text! takes a plain string or a Path.
t02: Unsure whether args() is a bang call; I wrote it plain, as the sheet lists it. Assumed sum returns i64.
t03: Unsure whether the `join` pipe ends the chain correctly with a bare separator argument; I wrote join(", ") after the pipe.
t04: Ties in length have unspecified order, so which 3 words appear when lengths tie is unclear.
t05: Assumed at(row, i) returns an optional string and that unwrap_or works on it; unsure whether a missing column should be skipped instead of mapped to "".
t06: Unsure of run!'s argument types (command string plus List[string] args) and whether the row needs Process.
t07: Assumed average on an empty list is undefined; the sheet does not say what it returns.
t08: Unsure whether the sheet's pad counts the string's width in characters.
t09: files() is written without a bang, as the sheet lists it; the sheet mixes bang and non-bang I/O, so it may need files!().
t10: Assumed reverse_text reverses characters, not bytes, and that read_stdin!() returns all input at once.
t11: Assumed max_by on ties returns the first longest line; unsure whether length (characters) is the intended measure.
t12: words splits on spaces only; if data.txt has newlines, words may not split them, so I may miss words at line ends.
