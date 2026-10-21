# Weak References

Status: design approved by the owner (2026-10-21); not yet in the spec or
the compiler.

hd counts references and collects cycles. A weak reference adds one more
tool: a way to point at an object without keeping it alive. It is useful
whether or not the cycle collector ships:

- **Caches and registries.** A map from keys to objects that should
  disappear when nothing else uses them. The collector never frees these
  entries, because they are not cycles, only references that hold
  memory too long.
- **Back pointers and observers.** A child's `parent`, or an event
  source's listener list. With a weak reference, the parent goes as soon
  as its last owner drops it, not at the next collection.
- **Precedent.** CPython has reference counting, a cycle collector *and*
  the `weakref` module, for exactly these reasons. Swift has `weak` and
  `unowned`; Rust has `rc::Weak`.

## The API

A weak reference is a std type, `Weak[T]` in `std.weak`, not a new kind
of reference.

```hd
use std.weak.{Weak, weak}

struct Node:
    name: string
    parent: Weak[Node]?

fn parent_name(node: &Node) -> string:
    match node.parent:
        .Some(link) =>
            match link.upgrade():
                .Some(parent) => parent.name
                .None => "(gone)"
        .None => "(root)"

fn attach(child: &mut Node, parent: &Node) -> void:
    child.parent = .Some(weak(parent))
```

- `weak(r: &T) -> Weak[T]` and `weak_mut(r: &mut T) -> Weak[T]` make a
  weak reference from a reference.
- `upgrade(self) -> (&T)?` gives a reference when the object is still
  alive, and `.None` once it has been freed.
  - A weak made from `&mut T` also has `upgrade_mut(self) -> (&mut T)?`.
  - The reference `upgrade` returns is an ordinary counted reference: it
    keeps the object alive while it lives.
- A `Weak[T]` is copyable. Copies refer to the same object. `w1 is w2`
  compares targets, as `is` does for references.
- There is no way to read through a weak reference directly. Every use
  goes through `upgrade` and handles `.None`, so a program cannot forget
  that the object may be gone. This is Rust's choice over Swift's
  `weak var`, which silently turns nil.

## What a weak reference may point at

Only a whole counted object: a `&Node { ... }` object, a local's cell
(`&x` of a local that lives in a cell), or anything else that is the
start of a counted allocation.

A weak reference to an interior place (`&x.field`, `&xs[i]`, a place
reached through another reference) is a compile error,
`weak-interior-reference`. Two reasons:

- an interior reference already keeps its whole object alive, so the
  weak would track the containing object, not the place the program
  named;
- a weak element reference would conflict with pins: an element
  reference's count of the buffer *is* the pin (§23.20), and a weak one
  could not pin.

Value types (structs, lists, maps, strings) have no identity apart from
the place holding them. A program that wants a weak handle to one puts
it behind `&` first.

## The runtime: a side table on demand

Objects that are never weakly referenced pay nothing. This follows
Swift's side table, not Rust's weak count in every allocation.

- **The side entry.** The first `weak(r)` to an object allocates a small
  entry: the object's address, a weak count, and a live bit. One bit of
  the object's descriptor word marks that it has an entry; the entry is
  found through a small address-keyed table.
  - Today bits 0-24 of the descriptor word hold the release glue's slot.
    That shrinks to bits 0-23 (16M slots, far beyond any program), and
    bit 24 becomes `WEAK`.
- **A `Weak[T]` value** is the address of its side entry. Copying one
  increments the entry's weak count; dropping one decrements it.
- **Freeing an object that has `WEAK` set** (by count or by the cycle
  collector) clears the entry's live bit and its address before the
  memory is reused. The entry itself is freed when its weak count
  reaches zero.
- **`upgrade`** checks the live bit. If it is set, `upgrade` retains the
  object and returns its address; if not, it returns `.None`. That is
  one load and one branch on the fast path.
- **Cycle collection** treats weak references as non-edges. A cycle kept
  only by weak references is garbage, as it should be, and freeing it
  clears its entries like any other free.

The cost:
- objects never weakly referenced: zero bytes and zero instructions,
  apart from one bit test on the free path for types that can ever be
  weakly referenced. The compiler knows which types those are, so other
  types skip the test entirely;
- an object with weak references: one side entry (about 16 bytes)
  shared by all of its weak references;
- `weak(r)`: a lookup on first use, then an increment;
- `upgrade`: a load, a branch and a retain.

## What it is not

- **Not `unowned`.** There is no unchecked weak reference whose use
  traps or misbehaves after the object is gone. Every use is
  `upgrade`.
- **Not a replacement for the cycle collector.** That decision stands
  on its own (#713/#714/#715 measured and removed the runtime's own
  cycles). Weak references give programs control over lifetime;
  collection still catches the cycles nobody broke.
- **No interior weak references.** If needed later, they would be
  (object weak reference, path) pairs; nothing here prevents that.

## Spec and tests

The spec gains:
- a section on weak references: the type, `weak`/`weak_mut`/`upgrade`/
  `upgrade_mut`, `is`, copying, the `weak-interior-reference` error;
- a std page for `std.weak`.

Conformance cases:
- upgrade succeeds while a strong reference lives and fails after the
  last one is dropped;
- a parent/child tree with weak parent pointers is freed with no cycle
  collection;
- a cycle held only through weak references is freed;
- a cycle with a weak reference into it, freed by the collector: the
  weak then fails to upgrade;
- copies of a weak reference compare `is`-equal;
- `weak(&x.field)` and `weak(&xs[i])` are rejected;
- a checked build reports no leak, and the side entries are freed;
- an `upgrade` held across a suspension point keeps the object alive.

Bench: hello's size is unchanged, and programs that never take a weak
reference produce byte-identical Wasm.
