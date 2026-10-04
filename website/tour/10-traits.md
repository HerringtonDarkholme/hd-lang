---
title: Traits and methods
---

# A trait is a checked contract: leave out a method and the type doesn't compile

A trait names the methods a type must have, such as an email that can give its
subject and its body. `send_all` works for every type that implements `Email`
and needs nothing else from it. A type opts in with `impl Email for ...`, and
the compiler checks that the impl has every method, with the trait's
signature. A new kind of email that forgets its subject is caught when you
build it, not when a customer gets a blank message. Methods outside a trait go
in a plain `impl Type:` block.

```hd
trait Email:
    fn subject(self) -> string
    fn body(self) -> string

impl Email for Shipped:
    fn subject(self) -> string: "Order ${self.order} has shipped"
    fn body(self) -> string: "It is on its way with ${self.carrier}."

fn send_all[E < Email](outbox: List[E]) -> void $ Console:
    for email in outbox:
        println("${email.subject()} | ${email.body()}")

# Delete the `fn subject` line of the impl and Run:
#     missing-trait-method: Shipped does not implement Email.subject

# ── plumbing ──
data Shipped:
    order: string
    carrier: string

pub fn main() -> void $ Console:
    send_all([Shipped { order: "A-1042", carrier: "DHL" }])
```

```edit
replace:     fn subject(self) -> string: "Order ${self.order} has shipped"
with:
error: missing-trait-method: Shipped does not implement Email.subject
```

```output
Order A-1042 has shipped | It is on its way with DHL.
```
