# Understanding Diagnostics

A diagnostic names the edit to make. This page explains the rules behind
some common messages. The [specification](../../spec/README.md#diagnostics)
lists every diagnostic code; [The `hd` Command](COMMANDS.md) explains text,
JSON, and fix-it output.

## Opaque Types

An `impl Trait` result has an identity belonging to the function that returns
it. Two functions returning the same trait can return different opaque types.
When a mismatch needs that distinction, the diagnostic includes the short
function name after `returned by`. If short names collide, paths distinguish
them. Use the expected function's result, or
accept a reference to the trait when the API permits one.

## Type Mismatch

`type-mismatch` shows the expected type and the type supplied. Add the expected
type annotation or convert the value explicitly. Arguments solving one type
parameter may differ only by their outer permission; convert incompatible
arguments to one intended type before the call. When the types differ only
in their requirement rows, the diagnostic prints those rows too.

Permissions inside a type convert only through its declared variance. A
function type nested inside a type keeps its requirement row: widening an
outer function value's row does not change nested function types. Convert or
rebuild the containing value with the required element type.

A function result's reference lifetime also matters. A result borrowing from
an input cannot fit a function type that promises no such borrowing. The
message identifies the input; adjust the result lifetime annotation to the
intended contract, or supply a function satisfying the expected contract.

## Move Out Of Place

`move-out-of-place` identifies a value that cannot be taken from its current
place. A local reference borrows a place without owning it. Counted references
can share an object, so another reference may still read its value. Borrow the
value through an appropriate reference, pass the existing reference onward,
or construct a separate value.

A closure can run repeatedly. Moving a non-`Copy` capture would remove the
value needed for a later call, and a shared capture can still be read by
another closure. Borrow the captured value instead.

A match guard reads bindings in place before the match commits to an arm.
If the guard is false, another arm still matches the same value. Borrow in
the guard and move in the arm. A closure made in the guard must also capture
a reference; alternatively, create that closure in the arm.

The diagnostic names the part preventing a copy: a type declared `impl !Copy`,
a parameter or associated type without a `Copy` bound, or a mutable local
reference. Rebuilding a value is different from an implicit `Copy` operation.

## Cannot Infer Type

For `cannot-infer-type`, write an explicit type argument or annotate the
binding producing the value. The message names the particular argument or
binding when it is known.

A const parameter absent from argument types cannot be inferred from the
expected result. Write it in the call's type argument list, such as
`f::[4](...)`.

A bare trait descriptor, such as `dyn Show`, is not a concrete type argument.
Pass a concrete reference, or write a reference type argument such as
`f::[&dyn Show](...)`. A missing `Self` is supplied with `Type::method(...)`
or `T::method(...)`.

## Overlapping Impl

`overlapping-impl` identifies a type for which two implementations apply.
Its primary location points to one implementation; a labelled secondary
location points to the other. Remove an implementation or narrow their
applicability so they no longer overlap. A test block's implementations
must also avoid overlapping the module implementations visible to it.

JSON keeps secondary locations in the diagnostic's `labels` array. Each
label has a message and a source span; it is part of the same diagnostic,
not another error. An unchanged warm check preserves these labels.
