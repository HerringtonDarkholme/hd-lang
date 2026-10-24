t01: Unsure whether a bang call such as read_text!(...) can start a |> chain; assumed yes.
t02: Unsure how sum behaves on an empty list (not on the sheet); closure return type i64? assumed valid.
t03: Assumed length(l) > 0 is fine since the literal 0 defaults to usize, matching length's result.
t04: sort_by direction is not stated (assumed ascending); tie order after reverse is undefined.
t05: Column trimming is applied only to the active column; the name column is used as split.
t06: Unsure whether run! takes a command string plus a List[string] of args; used that form. Row is $ Process per the sheet.
t07: Unsure of the f64? closure return syntax; average of an empty list is undefined.
t08: args() is listed without ! on the sheet, so assumed non-bang; main needs $ Args.
t09: files() is under the "bang functions" heading but is written without !, so assumed non-bang; sort on strings assumed lexical.
t10: Assumed reverse_text reverses characters; read_stdin!() is a bang call so main is main!.
t11: max_by tie behaviour undefined (first or last longest line); the optional result is handled in match.
t12: words splits on spaces only, so newlines may stay attached to words in whole-file text; used as the task says.
