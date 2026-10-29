---
title: Hello
---

# Hello

The program on the right prints how many minutes a day has.

Press **Run**, or Ctrl+Enter (Cmd+Enter on a Mac), to run it in your browser.

## Scripts

```hd example
println("A day has ${24 * 60} minutes")
```

A file may start with statements. Such a file is a **script**, and it needs
no `main`. `println` writes one line.

## Values inside strings

```hd example
"A day has ${24 * 60} minutes"
```

`${...}` evaluates an expression and puts its value **inside the string**.

**Run it.** `24 * 60` is 1440, so it prints `A day has 1440 minutes`.

```hd
# Press Run. Then change the calculation and run again.
println("A day has ${24 * 60} minutes")
```

```edit
replace: println("A day has ${24 * 60} minutes")
with: println("A day has ${hours * 60} minutes")
error: unknown-name: `hours` is not defined
```

```output
A day has 1440 minutes
```
