---
title: Enums and match
---

# A `match` covers every case, so a new case can't be forgotten

An `enum` is a closed list of cases, and a case can carry data, such as the
carrier of a shipped order. `match` picks the case and unpacks its data, and it
must cover every case. When the business adds refunds, the compiler lists every
`match` that does not handle them yet, before a customer finds the gap. A
catch-all `_ =>` arm would silence that list, so keep it for when "everything
else" really has one answer. Comparing two statuses with `==` is a separate
matter: an enum can be compared only if it asks with `@derive(Eq)`. Without
that line, `status == OrderStatus.Pending` is rejected. `match` needs no derive.

```hd
fn is_pending(status: OrderStatus) -> bool:
    status == OrderStatus.Pending

fn can_cancel(status: OrderStatus) -> bool:
    match status:
        .Pending => true
        .Paid(_) => true
        .Shipped(_) => false

@derive(Eq)  # ← lets `==` compare two statuses
enum OrderStatus:
    Pending
    Paid(cents: i32)
    Shipped(carrier: string)
    # Refunded(cents: i32)

# Uncomment `Refunded` above and Run:
#     nonexhaustive-match: match does not cover: Refunded
# Or delete the `@derive(Eq)` line and Run:
#     type-mismatch: type 'OrderStatus' does not implement Eq

# ── plumbing ──
pub fn main() -> void $ Console:
    println("paid: ${can_cancel(.Paid(4999))}")
    println("shipped: ${can_cancel(.Shipped("DHL"))}")
    println("pending: ${is_pending(.Pending)}")
```

```edit
replace:     # Refunded(cents: i32)
with:     Refunded(cents: i32)
error: nonexhaustive-match: match does not cover: Refunded
```

```output
paid: true
shipped: false
pending: true
```
