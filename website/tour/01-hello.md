---
title: Hello
---

# Run a script, print text, and put a value inside a string

This tour runs hd in your browser. Press **Run**, or Ctrl+Enter (Cmd+Enter on
a Mac). A script may start with statements, so the first program needs no
`main`. `println` writes one line, and `${...}` evaluates an expression inside
a string. The tour explains bindings, functions, and requirements later.

```hd
# Press Run. Then change the calculation and run again.
println("A day has ${24 * 60} minutes")

# Change `24` to `hours` and Run:
#     unknown-name: `hours` is not defined
```

```edit
replace: println("A day has ${24 * 60} minutes")
with: println("A day has ${hours * 60} minutes")
error: unknown-name: `hours` is not defined
```

```output
A day has 1440 minutes
```
