# Weak References

Status: standard library specification draft.

This chapter defines `std.weak`: the copyable weak-reference types
`Weak[T]` and `WeakMut[T]`, their constructors, and their upgrades. The
language tier defines the [Weak Reference Runtime](../lang/13-values-and-references.md#weak-reference-runtime)
that these ordinary standard-library items use.

## Weak Types And Construction

`weak` makes a read-only weak value, while `weak_mut` needs writable access
and makes a weak value that may upgrade to writable access:

```text
use std.weak.{Weak, weak}

struct Node:
    name: string

fn remember(node: &Node) -> Weak[Node]?:
    weak(node)

fn name(link: Weak[Node]) -> string?:
    match link.upgrade():
        .Some(node) => .Some(node.name)
        .None => .None
```

1. r[std-weak.module.items] `std.weak` declares `WeakEntry[T]`, `WeakMutEntry[T]`, `Weak[T]`, `WeakMut[T]`, `weak`, and `weak_mut`; none is a prelude name.
2. r[std-weak.type.entry] `WeakEntry[T]` and `WeakMutEntry[T]` are opaque side-entry types with private fields. Their distinct generic types prevent user code from constructing an entry, changing its target type, or adding writable access.
3. r[std-weak.type.readonly] `Weak[T]` is a copyable, read-only weak identity for a whole counted allocation whose stored concrete type is `T`.
4. r[std-weak.type.mutable] `WeakMut[T]` is a copyable, writable weak identity for such an allocation.
5. r[std-weak.new.readonly] `weak[T](target: &T) -> Weak[T]?` performs the weak-target check of [`ref.weak.target.whole`](../lang/13-values-and-references.md#r-ref.weak.target.whole).
6. r[std-weak.new.mutable] `weak_mut[T](target: &mut T) -> WeakMut[T]?` performs the same check and requires writable access to `target`.
7. r[std-weak.new.refuse] Either constructor returns `.None` without panicking when the check refuses, including for interior fields, elements, zero-offset first fields, and parameters that refer to them.
8. r[std-weak.new.some] A successful constructor returns `.Some` of the shared weak entry for the target allocation.
9. r[std-weak.new.non-owning] Constructing or copying either weak type does not keep the target alive, by [`ref.weak.non-owning`](../lang/13-values-and-references.md#r-ref.weak.non-owning).

> **Why.** The optional result makes the whole-allocation condition visible
> at every construction site. Distinct read-only and writable types ensure
> that a weak value made from shared access can never gain write access.

## Upgrading

An upgrade turns a live weak value into an ordinary counted reference:

1. r[std-weak.upgrade.readonly] `Weak[T].upgrade(self) -> (*T)?` returns `.Some` of a read-only counted reference while the target is live, and `.None` after it is freed.
2. r[std-weak.upgrade.mutable] `WeakMut[T].upgrade(self) -> (*mut T)?` returns `.Some` of a writable counted reference while the target is live, and `.None` after it is freed.
3. r[std-weak.upgrade.retain] A successful upgrade retains the target before it returns, by [`ref.weak.upgrade.live`](../lang/13-values-and-references.md#r-ref.weak.upgrade.live).
4. r[std-weak.upgrade.suspension] The returned counted reference keeps the target alive across a suspension as any counted reference does.
5. r[std-weak.upgrade.only-access] Neither weak type reads or writes `T` directly; upgrading is its only access to the target.

## Weakening And Identity

`WeakMut[T]` may lose write access, but `Weak[T]` never gains it:

1. r[std-weak.weaken.from] `Weak[T]` implements `From[WeakMut[T]]`; `Weak::from(value)` names the same weak entry with read-only access.
2. r[std-weak.weaken.no-reverse] There is no conversion from `Weak[T]` to `WeakMut[T]`; using one where the other is required is a `type-mismatch`.
3. r[std-weak.identity.copy] A copy of a weak value has the same identity as the original, so `copy is original` is true.
4. r[std-weak.identity.target] Weak values made for the same live target have the same identity because they share one entry; values with different entries are not identical.
5. r[std-weak.identity.dead] Copies remain identical after their target dies, until their last shared weak entry is released.
6. r[std-weak.identity.reuse] A dead weak value is not identical to a weak value for a later allocation, even when the allocator reuses the target's address.

## Collection And Release

1. r[std-weak.release.target] Releasing the last strong reference may free a weak target; every weak value then remains valid but upgrades to `.None`.
2. r[std-weak.release.cycle] Weak values are non-edges during cycle collection, so a cycle kept only by weak values is collectible.
3. r[std-weak.release.collected] When cycle collection frees a target, its weak values are invalidated before the target's storage is reused.
4. r[std-weak.release.entry] Releasing the last `Weak[T]` or `WeakMut[T]` frees its weak entry, including after the target has died.

> **Why.** A weak value controls neither target lifetime nor collection.
> Its only lifetime promise is that it remains safe to upgrade and reports
> absence after the target has gone.

See also: [Boxes](../lang/13-values-and-references.md#boxes),
[Identity](../lang/13-values-and-references.md#identity).
