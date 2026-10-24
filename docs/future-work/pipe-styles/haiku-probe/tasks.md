# Tasks

Write one hd program per task, as `tNN.hd` in your output directory. Each program is a complete file with a `pub fn main!()` (or `main()`) entry and the requirement row it needs (`$ Console`, `$ FsRead`, ...). Use the pipe operator `|>` wherever a chain of steps reads naturally: these tasks are shell-style one-liners. Use only the functions on your sheet, plus ordinary hd syntax (closures, `match`, `if`, `let`, string interpolation).

1. Print how many lines of the file `app.log` contain `ERROR`.
2. The program arguments are numbers; print the sum of those that parse as `i64`, ignoring the rest.
3. Read `names.txt`, trim each line, drop empty lines, and print the rest joined with `, `.
4. Read `words.txt` (one word per line), and print the 3 longest words, longest first.
5. Read `users.csv` (lines of `name,age,active`); print the names of rows whose `active` column is `yes`, one per line.
6. Run `git status --short` and print how many output lines start with ` M`.
7. Read `prices.csv` (lines of `item,price`), parse column 2 as `f64`, and print the average.
8. Uppercase each program argument, pad it to width 10, and print each on its own line.
9. Given a list of paths (`files()`), keep those ending in `.hd`, sort them, and print them.
10. Read standard input to EOF, reverse each line's characters, and print the lines in reverse order.
11. Read `app.log`, find the longest line, and print its length and the line.
12. Read `data.txt`, split it into words on spaces, drop words shorter than 4 characters, lowercase them, remove duplicates, and print how many remain.
