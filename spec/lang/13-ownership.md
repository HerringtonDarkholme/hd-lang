# Ownership and Borrows

Status: language specification draft.

This chapter defines the modes in which a position holds its value: plain,
owned, or borrowed.

> **Note.** The chapter grows section by section with the ownership
> prototype (issue #163), and each section arrives with the conformance cases
> that test it. Until a section specifies how a mode is checked, an
> implementation diagnoses a program that uses it, as
> [Specification Status](../README.md#specification-status) requires.

## Modes

A **mode** says how a position holds its value. It is written on the type at
the position, as `mut` is, so one type can be plain in one place and owned in
another.

| Mode | Written | A value in this mode |
| --- | --- | --- |
| plain | `T`, `mut T` | keeps the object semantics of the other chapters: copying it makes an alias |
| owned | `own T` | is the only path to its value |
| borrowed | `^T`, `^mut T` | refers to a place it does not own, for a lifetime |

1. r[own.mode.plain] A type written without `own` or `^` is plain. A plain value keeps the semantics the other chapters define, and the rules of this chapter do not apply to it.

```text
data Counter:
    hits: mut List[i32]

fn record(c: mut Counter) -> void:
    c.hits.push(+1)

fn count_twice() -> usize:
    let first: mut Counter = Counter { hits: [] }
    let second: mut Counter = first    # a plain copy is an alias
    record(second)
    first.hits.len()                   # 1: both names reach one object
```

> **Why.** Code with no mark compiles and runs as it did before modes
> existed. The checks that ownership adds cost nothing where nothing is owned
> or borrowed.

See also: [Mutation Checks](04-type-system.md#mutation-checks).
