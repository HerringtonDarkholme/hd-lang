---
title: Hello
---

# Run a script, print text, and put a value inside a string

This tour runs hd in your browser. Press **Run**, or Ctrl+Enter (Cmd+Enter on
a Mac). A script may start with statements, so the first program needs no
`main`. `println` writes one line, and `${...}` evaluates an expression inside
a string. These are the few bootstrap forms used before the tour explains
bindings, functions, and requirements.

```hd
# Press Run. Then change Ada and run again.
println("Hello, ${"Ada"}!")

# Change the line to `println("Hello, $customer!")` and Run:
#     unknown-name: `customer` is not defined
```

```edit
replace: println("Hello, ${"Ada"}!")
with: println("Hello, $customer!")
error: unknown-name: `customer` is not defined
```

```output
Hello, Ada!
```
