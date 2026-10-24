t01: read_text! takes a plain string here, while the write-hd skill passes Path("..."); I used a string.
t02: args() is listed under "bang functions" on the sheet but written as a plain call; sum of an empty list is undefined; parse_i64 passed bare.
t03: read_text! path type as in t01; empty-line test uses length, which counts characters.
t04: sort_by then reverse; order of equal-length words is unspecified, so ties may come out either way.
t05: at() returns an optional, so I used unwrap_or; a header row would be dropped by the "yes" filter; CRLF or padded spaces are not trimmed.
t06: run! takes the command as a string here, and the $ Process row name is not in the write-hd std table; both assumed.
t07: a header row parses to none and is skipped by keep_some; average of an empty list is undefined.
t08: a nested _ inside map(_, pad(_, 10)) relies on the innermost-call rule; I followed the rule.
t09: files() is listed as a bang function but written as a plain call, so I used main() with no bang calls.
t10: read_stdin! needs $ ConsoleInput, which I assumed from the write-hd table.
t11: max_by tie-breaking (first or last longest line) is unspecified; length is passed bare as the key.
t12: words splits only on spaces, so words on separate lines stay joined to newlines; I followed the task literally.
