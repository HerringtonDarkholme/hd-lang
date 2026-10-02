# Hash

Status: standard library specification draft.

This chapter defines the part of `std.hash` that `lib/std` writes in
ordinary hd over the language tier:

- what a derived `Hash` hashes, through the trait's template;
- how tuples hash, through the trait's tuple template;
- how lists hash.

The language tier keeps `Hash` and `Hasher`, which map keys require
([Hashing](../lang/09-traits.md#hashing)), the `@derive` checks, and the
runtime's hash seed ([Derived Hashing](../lang/09-traits.md#derived-hashing)).

## Derived Hashing

`@derive(Hash)` hashes a value member by member:

```text
@derive(Eq, Hash)
enum Status:
    Ready(code: i32)
    Failed(code: i32, reason: string)

fn lookup_status(items: Map[Status, i32], status: Status) -> i32?:
    items.get(status)
```

1. r[std-hash.derive.hash.template] `std.hash` declares the [template](../lang/14-annotations.md#templates) of `Hash`, which `@derive(Hash)` instantiates.
2. r[std-hash.derive.hash.support] `@derive(Hash)` supports data and enums.
3. r[std-hash.derive.hash.data] It hashes every declared data field in declaration order, including embedded fields.
4. r[std-hash.derive.hash.enum] For an enum, it hashes the variant identity, then shared enum data in declaration order, then that variant's payload fields in declaration order.
5. r[std-hash.derive.hash.fields] Every hashed field must implement `Hash`; no field is implicitly excluded.
6. r[std-hash.derive.hash.cycles] Like derived equality, derived hashing does not detect cycles, so hashing a cyclic graph may exhaust the execution stack.

## Tuple Hashing

```text
fn index(counts: Map[(i32, string), i32]) -> i32:
    counts[(1, "a")]
```

1. r[std-hash.tuple.template] `std.hash` declares a [tuple template](../lang/14-annotations.md#tuple-templates) for `Hash`. A tuple implements `Hash` when every element does, at every size.
2. r[std-hash.tuple.combine] Tuple hashing combines the elements' hashes, in order.
3. r[std-hash.tuple.rest-list] A tuple's rest element hashes as its `List[T]`. So a rest tuple implements `Hash` when its fixed elements do and `T < Hash`, and it may be a map key.

## List Hashing

```text
fn count(counts: Map[List[i32], i32], key: List[i32]) -> i32:
    match counts.get(key):
        .Some(n) => n
        .None => 0
```

1. r[std-hash.list.impl] `std.hash` implements `Hash` for `List[T]` when `T < Hash`.
2. r[std-hash.list.combine] List hashing combines the items' hashes, in order, so two equal lists hash the same.
3. r[std-hash.list.not-element] A list whose item type does not implement `Hash`, such as `List[f64]`, does not implement `Hash`. Using it where `Hash` is required is an error. Error: `unsatisfied-trait-bound`.

```text
fn lookup(counts: Map[List[f64], i32]) -> i32:  # error: unsatisfied-trait-bound
    0
```

See also: [Derived Tuple Implementations](../lang/09-traits.md#derived-tuple-implementations),
[Cmp](cmp.md).
