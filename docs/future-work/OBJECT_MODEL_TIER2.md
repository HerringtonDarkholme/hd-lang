# Object Model, Tier 2: Design Study

Status: parked proposal (#426, #163). Adoption is parked (owner, 2026-10-18); the
lifetime prototype was built and removed. Part 3's first step, local
references, is applied (#834); the rest is unchanged in the spec and the
compiler.

Tier 1 (the tier-1 design, `58c3d470f:docs/future-work/OBJECT_MODEL_TIER1.md`) gives hd Go's
object model on counted linear memory: structs are values, sharing comes
from global references `&T` and `&mut T`, and every reference is
counted, may escape and may alias. It is complete and safe, and an agent
writes it with no marks. Its costs are counts, cells and pins, listed in
the left column of the table below.

Tier 2 asks whether Rust's lifetimes can be laid onto tier 1, opt-in,
and what they buy. A reference type may carry the set of things it
depends on, `&{a, self.x} T`, and **global is one more lifetime**, the
one that outlives all others.

**Go-style global references and Rust's lifetimes do not compose
statically.** A static borrow is sound only when its path starts at
something local. Through a global reference, an alias the checker
cannot see may write while the borrow lives. So tier 2 is Rust's
lifetimes on the value world: locals, values, NonCopyable data and
scoped references. At shared references it has a RefCell-style
boundary, the one Rust itself has at `Rc<RefCell<T>>`. A scoped borrow
taken through a global reference sets a borrow flag in the object's
header, and a conflicting write through another alias panics. Each
declaration is compiled once, tier 1 code calls it with no marks, and
locals that never escape and values never pay for the flag.

| Tier 1 cost | With tier 2 marks |
| --- | --- |
| an increment and a decrement per view or stored reference | none on scoped references |
| `&mut local` passed to a call that is not inlined: a heap cell | the local stays in the frame when the parameter is `&{} mut T` |
| a count update through each stored interior reference | none through scoped references |
| element references pin the list; push tests the pin | a local list is frozen statically; a list behind a global reference is flagged |
| closure environments in counted objects | a scoped closure's environment is a record in the frame, unless it is returned past the frame |
| writable references may alias | a scoped `&{} mut` has no conflicting writer in any caller: statically from locals, by the flag through counted storage; other aliases may still read |
| release happens at count zero | a NonCopyable value is released at the end of its scope |

One rule binds both tiers: **nothing a caller relies on is inferred
from a body**. Lifetime sets, escaping, closure kinds, `global` bounds
and copyability are written in headers. The compiler checks each body
against its header and infers only inside it: the sets of locals, the
extent of each borrow, and placement.

**Dependencies.** Tier 2 builds on tier 1 as settled: global `&mut`
gets no exclusivity check (section 6.4); `List`, `Map` and `string` are
copy-on-write values whose nested write paths, such as `m[k].push(x)`,
write in place (section 6.1); tier 1 has no long-lived element
references, only call-scoped ones
([Call-Scoped Access](../../spec/lang/13-values-and-references.md#call-scoped-access)),
and tier 2 adds scoped element references, frozen statically; an entry
API may come with tier 2 lifetimes, as an ordinary std API with a written
lifetime, never with a global lifetime; `==` between references is an
error; and the new semantics run on Wasm
GC emulation first (section 16). From
[REFCOUNT_RUNTIME.md](REFCOUNT_RUNTIME.md): interior references are
plain addresses whose object is found through the page table; the
runtime is written in hd over memory intrinsics; an undisposed
`Disposable` at count zero is reported as a leak at once; and Wasm GC
is deleted once the linear backend reaches parity. Its unit costs,
measured on V8 only: an increment and decrement pair costs 2.1 ns,
borrowed reads and moves at last use bring counted programs within 4%
of uncounted ones, the page-table lookup measured no slower than a
base pointer, and a load-and-branch test before a write measured free.
Section 12 scores tier 2 against those numbers.

Part 1 designs tier 2 in an indentation syntax close to hd's; every
spelling is a placeholder and no code block is checked by a compiler.
Part 2 maps it onto hd and plans the work. Part 3 is local references,
tier 2's first cut, whose step 1 is applied.

# Part 1: From Scratch

## 1. What Tier 2 Must Serve

- **Parsers over borrowed input**: no count per token, no source kept
  alive by one token.
- **In-place algorithms and iterators** over a local buffer, which stays
  in the frame, with no allocation.
- **Callbacks** that borrow locals for the length of a call.
- **Values with a deterministic end**: a unique buffer, a guard.
- **One std for both tiers**: a tier 1 caller pays no more than tier
  1's costs, and a tier 2 caller pays nothing.

## 2. The Model On One Page

```text
struct Lexer[{s}]:                        # a lifetime parameter `s`
    src: &{s} string                      # a view that borrows from s: no count
    pos: usize

impl[{s}] Lexer[{s}]:
    fn next_word(&{} mut self) -> &{s} string:   # borrows self for the call; returns a view of s
        start := self.pos
        self.pos = word_end(*self.src, start)   # `*` reads the view; passing it costs no count
        self.src.slice(start, self.pos)

fn count_words(text: string) -> usize:
    let mut lexer = Lexer { src: &text, pos: 0 } # Lexer[{text}]: scoped, uncounted
    let n = 0
    while lexer.next_word().len() > 0:
        n += 1
    n
```

1. `&{a, b} T` and `&{a} mut T` are **scoped** references: they depend
   on `a` and `b`, hold no count, and cannot outlive them. `&T` with no
   set is **global**, as in tier 1.
2. A set names reference parameters, `self`, paths such as `self.items`, lifetime
   parameters, locals inside a body, and `global`. In a parameter, `{}`
   is that parameter's own lifetime.
3. A global reference converts to any scoped one. A view type whose
   bytes always live in a counted buffer (a string view, a span, an
   element reference to a global element type) is promoted when stored
   in a global place: one count, no copy; a value holding such views is
   converted, a count per view. A general `&{} T`, which may point into a
   stack frame, cannot be stored globally.
4. A type with a lifetime parameter, `Lexer[{s}]`, is scoped when its
   argument is. A value containing a scoped reference is itself scoped.
5. A scoped `&{} mut T` admits no other writer; a scoped `&{} T` freezes
   the place it borrows. A static checker enforces both inside each
   body; other aliases may still read.
6. Closure types carry capture sets, `fn(A) ->{xs} B`, and a kind:
   `fn`, `fn mut` or `fn once`. A closure type with no set is global.
7. A struct marked `~Copy` is NonCopyable: it moves, and is released at
   the end of its scope.
8. A scoped borrow taken through counted storage flags every storage on
   its path for its extent; a write through another alias that meets
   the flag panics (section 6.4).

## 3. Lifetime Sets

### 3.1 What a set may name

| Entry | Example | Means |
| --- | --- | --- |
| a reference parameter | `fn pick(a: &{} Node) -> &{a} string` | lives as long as `a`'s argument does: the result inherits `a`'s sources |
| `self` | `fn get(&{} self, i: usize) -> Option[&{self} T]` | as a reference parameter |
| a path | `fn name_of(u: &{} User) -> &{u.name} string` | borrows only that field; other fields stay writable |
| a lifetime parameter | `fn first[{l}](ts: List[Token[{l}]]) -> Token[{l}]` | a lifetime that no variable names, such as one inside a type argument |
| a local | `let t: &{src} string = lexer.next_word()` | inside a body only |
| `global` | `Token[global]`, `T < global` | outlives everything; `&{global} T` is `&T` |
| nothing: `{}` | `xs: &{} List[T]`, `let r: &{} Point = &ps[0]` | in a parameter, the argument's own lifetime; in a local's type, inferred from the initializer; elsewhere an error |

A path in a `pub` header names only `pub` fields, so a signature never
shows a private field. A field path borrows the field's place; when the
field holds a reference, `{u.owner}` follows Scala's monotonicity rule:
`{u}` covers `{u.owner}`, never the reverse.

A header set names only reference parameters. A value parameter is an
owned copy that ends with the call (section 5.1), so nothing can borrow
it beyond the body. The result depends on what the reference argument
depends on, and the argument variable itself may end, as Swift's
`copy a` dependency reads.

### 3.2 Results name their sources

A result that borrows writes its set; there is no elision. A
reference in a result with no set is global, and `{}` in a result is an
error, because nothing would bound it. Sets join by union and order by
inclusion:

- `&{a} T` converts to `&{a, b} T`: depending on fewer things is valid
  for longer. `&T` converts to every `&{S} T`.
- A body may return a value whose set is covered by the header's.
  `if c: a.name else: b.name` has the set `{a, b}`.
- Behind a writable reference the type is exact, as Rust's `&mut T` is
  invariant in `T`: `&{l} mut &{a} T` does not convert to
  `&{l} mut &{a, b} T`.

### 3.3 Inside a body

The checker infers what headers leave out, from this body only:

1. **Sets of locals.** A `:=` or `let` with no written type takes the
   set of its initializer; a `let` assigned again takes the union of
   everything assigned to it, as Rust's regions do.
2. **Extent.** A borrow lasts from its creation to the last use of any
   value whose set includes it, along control flow, as in Rust's
   non-lexical lifetimes (RFC 2094). It does not last to the end of its
   block.
3. **Which reference an `&` makes.** `&e` and `&mut e` make a scoped
   reference when the expected type is scoped: a scoped parameter or
   receiver, a field of a scoped struct literal, or a binding whose
   written type has a set. With no expected type, `&e` is global, as in
   tier 1, so `r := &p` keeps tier 1's meaning.
4. **Borrows through references.** A borrow through a scoped shared
   reference may last as long as that reference's sources. A borrow
   through a scoped writable reference is a reborrow: it lasts at most
   as long as the writable reference is set aside.
5. **Shrinking at scope ends.** When a value leaves the scope of a local
   its set names, the local is replaced by that local's own sources, as
   Scala's avoidance does: a view borrowed from a local `v: &{s} string`
   has the set `{v}` inside the body and `{s}` outside it. A local that
   owns the data has no sources to stand in, so the value cannot leave.
6. **Temporaries.** A temporary that a scoped result borrows lives to
   the end of its statement, or to the end of its block when the result
   is bound by `let` or `:=`.

### 3.4 Errors

```text
fn longest(a: &{} string, b: &{} string) -> &{a} string:
    if a.len() >= b.len(): a else: b
    # error: the result may borrow `b`, but the header names only `{a}`
    #   help: write `-> &{a, b} string`
```

## 4. Lifetimes In Structs And Enums

### 4.1 Declarations name lifetime parameters

A type declaration has no variables to name, so it declares lifetime
parameters in its bracket list, written as a set, and its fields use
them:

```text
enum Token[{s}]:
    Ident(text: &{s} string)
    Number(text: &{s} string, value: f64)
    Punct(ch: char)
    End

struct Parser[{s}]:
    lexer: Lexer[{s}]
    peeked: Token[{s}]
    errors: List[ParseError]

fn first[{l}](tokens: List[Token[{l}]]) -> Option[Token[{l}]]
```

A use passes a set, `Token[{src}]`, or `global`. In a written type a
missing argument is global: `Token` alone is `Token[global]`, so tier 1
code names these types with no marks. `Option[&{l} T]`,
`Result[&{l} T, E]` and `List[Token[{l}]]` need nothing new: a generic
type argument carries its own lifetime (section 8.2).

### 4.2 A value containing a scoped reference is scoped

A value is scoped when its type mentions a set other than `global`, at
any depth. A scoped value:

1. cannot be stored in a global place (a field of an object behind a
   global reference, a top-level binding, a provider, a task, a value
   that crosses a boundary), except by promotion: a view type whose
   bytes always live in a counted buffer (a string view, a span, an
   element reference to a global element type) is converted to its
   global form there
   (section 8.4). A general `&{} T` may point into a stack frame, so
   storing it globally is an error whose fix is a global `&T`;
2. cannot be returned beyond its sets, nor captured by a global closure;
3. is never counted: copying a `Token[{src}]` copies its bytes and
   touches no header, because the view inside it holds no count.

A scoped list, `List[Token[{src}]]`, still has a counted buffer, but
its elements hold no counts and it cannot outlive `src`.

**Variance.** A lifetime parameter is covariant: `Token[{a}]` converts
to `Token[{a, b}]`, and `Token[global]` to every instance, except behind
a writable reference, where the type is exact (section 3.2).

### 4.3 Methods carry the lifetime on `self`

```text
impl[{s}] Parser[{s}]:
    fn advance(&{} mut self) -> Token[{s}]:     # borrows the parser for the call only
        old := self.peeked
        self.peeked = self.lexer.next()
        old

    fn peek(&{} self) -> &{self} Token[{s}]:    # a borrow of the parser itself
        &self.peeked
```

`advance` returns a token that depends on the source, so the caller may
call `advance` again while it keeps it; `peek`'s result freezes the
parser while it lives.

### 4.4 Tier 1 code sees the global instance

`Parser[global]` is an ordinary tier 1 type: its views are counted,
because their lifetime argument is global (section 8.3), and its
values may be stored anywhere. A tree parsed with `Parser[{src}]` and
then stored in a cache is promoted to `Expr[global]` at the store: its
string views are always backed by `src`'s counted buffer, so each gets a
count and no byte is copied (section 8.4).

```text
fn remember(cache: &mut Cache, src: string) -> Result[void, ParseError]:
    let mut p = Parser.new(&src)          # Parser[{src}]: uncounted while parsing
    cache.last = .Some(p.expr()?)         # promoted to Expr[global]: one count per view
```

Parsing with `Parser[global]` from the start counts each view as it is
made instead.

### 4.5 A field that borrows another field of its struct

Rust needs crates such as `self_cell` for a struct holding views into
its own buffer. Here a set names the field, as in
`rows: List[&{self.text} string]` (section 11.6). The dependency is on
the buffer, not the struct's bytes: copying the struct increments the
shared buffer, and the views still point into it. Three rules:

1. The named field must keep its contents in a counted buffer or
   object: a string, a list, a map or a reference. A path to an inline
   struct field is an error, since the struct's own bytes move.
2. After construction the named field is frozen: assigning it, taking
   `&mut` to it, or calling a writing method on it is an error. The
   whole struct may be replaced.
3. In a literal, a field typed `{self.f}` accepts views of the variable
   whose value is given to `f`, since `f` then holds the same buffer.

Such a struct depends on nothing outside itself, so it is global. It
cannot express an arena that grows while views into it live.

Taking such a field out of its struct ties it to the owner: projection
replaces `self` with the owner, so `csv.cells` has type
`List[&{csv} string]`. The ordinary escape checks then apply: returning
`csv.cells` after `csv` dies is an error, and using it while `csv` lives
is free. Storing it in a global place promotes it as any scoped result
is promoted (section 8.4).

## 5. NonCopyable And NonEscapable Structs

### 5.1 NonCopyable

```text
struct Buffer ~Copy:
    bytes: List[u8]

impl Buffer:
    fn write(&{} mut self, b: string) -> void: self.bytes.append(b.bytes())
    fn finish(self) -> string: string.from_utf8_unique(self.bytes)   # takes ownership
    fn drop(&{} mut self) -> void: pass     # optional; runs at the end of the owner's scope

fn greet() -> string:
    let mut b = Buffer { bytes: [] }
    b.write("hi")                           # borrows `b`
    s := b.finish()                         # moves `b`
    b.write("!")                            # error: `b` was moved by `finish` on the line above
    s
```

The calling convention is the same for every type. A value parameter
`T` is an owned copy: the caller moves its argument at its last use,
and otherwise pays one increment. A reference parameter, `&T` or
`&{} T`, is a borrowed read, or a write with `mut`. Results are owned.
A NonCopyable argument cannot be copied, so passing it by value moves
it; no parameter mark is needed where Swift writes `consuming`.

1. `~Copy` on a struct suppresses copying, as Swift's `~Copyable` does.
   A struct with a NonCopyable field, an enum with a NonCopyable
   payload, and `Option[Buffer]` are NonCopyable and must say so with
   `~Copy`, so the header shows it.
2. A use that is not a borrow moves the value: binding it to another
   name, storing it, returning it, passing it to a value parameter,
   capturing it in a `fn once` closure. Moves are not marked at the
   use, as in Rust and Swift.
3. A use after a move is an error that names the move. A move on one
   branch only makes the variable moved after the join. Moving a field
   out of a struct is an error; `replace` and `take` swap in a value.
4. At the end of its owner's scope, or when overwritten, the value is
   released: its `drop` runs if declared, then its fields are released,
   newest binding first. `drop` cannot suspend; a value that must close
   with a suspending call is `Disposable` and bound with `using`.
5. A global reference may point at one: `&mut Buffer { ... }` is a
   counted object that owns the buffer, released at count zero. `*r`
   and a by-value `self` call through a reference are errors, since
   they would move out of a shared object.

It buys no count on the value, no collector entry, release at a known
point, and owning methods such as `finish` that reuse a buffer nothing
else can hold. It costs moves and their errors, `~Copy` in generics,
and no partial moves.

### 5.2 NonEscapable is a lifetime parameter

Swift needs `~Escapable` because its references carry no lifetime in
their type. Here a type that cannot outlive what it borrows has a
lifetime parameter, such as a span or an iterator:

```text
struct ListIter[{l}, T]:
    items: &{l} List[T]
    at: usize

impl[{l}, T] ListIter[{l}, T]:
    fn next(&{} mut self) -> Option[&{l} T]     # the element borrows the list, not the iterator
```

`ListIter[{xs}, T]` cannot outlive `xs`, and `ListIter[global, T]` is an
ordinary value; no separate marker exists. Swift's conditional
`Escapable where T: Escapable` is this rule.

### 5.3 Generics

A generic header says what it needs from `T`, and the defaults differ
on purpose:

| Property | Default for `T` | To change it | Why |
| --- | --- | --- | --- |
| copyable | yes | `T < ~Copy` lets `T` be NonCopyable | most generic code copies; requiring `T < Copy` everywhere would burden every std generic |
| scoped | allowed | `T < global` requires a global `T` | a scoped `T` flows only into positions whose type mentions `T`, which carry its lifetime |

`List` declares `struct List[T < ~Copy] ~Copy`: a list is copyable
exactly when its elements are, as Swift writes
`extension Wrapper: Copyable where T: Copyable`. Rust's opt-in `Copy`
would put a bound on nearly every std generic; Swift's default suits a
language whose tier 1 copies values freely.

`T < global` is needed only where `T` lands in a place whose type does
not mention it: a top-level binding, a provider, a task, a `dyn` value,
a channel to another instance. Scala 3 calls the other case capture
tunnelling: a capture set inside a type argument travels with the type.

### 5.4 Closures and enums

A closure that captures a NonCopyable value by move is `fn mut` or
`fn once` (section 7.3). A `match` by value on a NonCopyable enum
consumes it. Payloads stay non-places, as in tier 1; `Option.take`
moves one out and the program stores it back.

## 6. Exclusivity

### 6.1 The rules

While a scoped borrow is live, its source place is restricted:

| Live borrow of place `P` | Reading `P` | Writing `P`, or a call that changes a list's length | Another `&{} mut` of `P` | Another `&{} P` |
| --- | --- | --- | --- | --- |
| `&{} P` (shared) | yes | no: `P` is frozen | no | yes |
| `&{} mut P` (exclusive) | through the borrow; directly, no; through a global alias, yes (section 6.4) | only through the borrow | no | no |

A place is a local, a field path from a local, the elements of a list
taken together, a map's entries taken together, or the target of a
scoped reference. Nested write paths such as `m[k].push(x)` and
`xs[i].f = v` are writes to these places, done in place; a live borrow
of a map entry freezes the map, since an insert may rehash. Places
conflict when one is a prefix of the other: `p.pos` and `p` conflict,
`p.pos` and `p.vel` do not. A scoped writable reference cannot be
copied; passing it to a parameter reborrows it for the call. As in tier 1, `&{} mut x` needs a writable place: a `let mut`
variable, a field of one, or a place reached through a `&mut`.

For a local list, freezing replaces the pin. `r := xs.get(0)` with
`get`'s header `-> Option[&{self} T]` freezes `xs` while `r` lives, so
`xs.push(v)` there is a compile error, not a `collection-pinned` panic, and no pin
count is taken. An exclusive borrow of an element makes the buffer
unique when it starts, so copy-on-write needs no extra rule.

### 6.2 The static checker

Rust's borrow checker, reduced to what tier 2 needs:

1. **Loans.** Each scoped borrow in a body is a loan on a place, shared
   or exclusive. A call creates loans from the callee's header: a
   result with `{xs}` keeps the loan on the argument `xs` alive as long
   as the result lives.
2. **Liveness.** A loan is live while any value whose set includes it
   may still be used, computed by a backward dataflow over the body.
3. **Conflicts.** Every access is checked against the live loans with
   the table of section 6.1.
4. **Two-phase borrows.** `xs.push(xs.len())` evaluates the argument
   before the exclusive borrow of `xs` starts, as Rust does.

It does not need what makes Rust's checker large: moves of ordinary
values, drop elaboration beyond NonCopyable values, region inference
across functions, or `unsafe`. NLL's problem case #3 is rejected here
too.

### 6.3 Errors, and how often programs meet them

```text
fn bump_all(xs: &{} mut List[i32]) -> void:
    first := xs.get(0)          # `first` borrows `xs` (shared)
    xs.push(1)                  # error: `xs` is frozen while `first` is used
    print(first)                #   note: `first` is used here
    #   help: copy the value out, `first := xs[0]`, or push after the last use
```

Messages name the place, the loan, where it was created and the use
that keeps it live, as rustc does. Every scoped error has a fix Rust
lacks: drop the set, and the reference becomes global at tier 1's
cost. Unmarked application code meets none of these errors.

### 6.4 The borrow flag

A tier 1 caller passes `obj.items`, reached through a global reference,
to `get(&{} self) -> Option[&{self} T]`. No static check sees the other
global references to `obj`, so an alias may write while the borrow
lives. A flag in the object header keeps the borrow sound at run time,
as Rust's `RefCell` and Swift's dynamic exclusivity do:

1. Taking a scoped borrow through counted storage marks every counted
   storage on its path: the object that holds the borrowed field, and
   the buffer a view or element reference points into, nested
   copy-on-write buffers included. A reader count marks `&{}`, a writer
   bit `&{} mut`. The caller holds one count on every storage it marked,
   and clears each flag before releasing that count, so no flagged
   header is ever freed; a borrow along a path of k counted storages
   costs k count pairs. A result that borrows carries these counts with
   it, and its last copy clears the flags and releases them, on every
   exit, cancellation included. When an object's reader bits fill, the
   runtime moves its reader count to a side table rather than wrapping
   into the descriptor bits; no program sees a limit.
   Counted storage is anything a count reaches: a global reference, a
   local whose address escaped into a heap cell, and a buffer that an
   element reference pins.
2. A write through counted storage tests the flag of the storage it
   actually writes: the object for a field, the buffer for an element or
   a length change. A conflicting flag makes the write panic with
   `borrow-conflict`. So an older alias cannot invalidate a newer view:
   `alias.name = "other"` while a view of `doc.name` lives tests the
   flag on `doc`'s object and panics.
3. Taking a second borrow through a global reference tests the flag
   too: a shared borrow conflicts with the writer bit, an exclusive one
   with either.
4. The flag clears where the borrow's extent ends, as the static
   checker computed it. A result that borrows, such as what
   `obj.items.get(0)` returns, keeps the flag set while it lives.
5. Locals that never escape and values never touch the flag, nor do
   writes through the borrow itself or reads through any alias.

Borrows through one global variable are checked statically against
each other, so `swap(&{} mut a.x, &{} mut a.y)` sets one writer bit.
The flag is per object: borrowing `a.x` and `b.y` exclusively panics
when `a` and `b` refer to one object, as two `borrow_mut` calls on one
`RefCell` do.

So a scoped `&{} mut` is free of conflicting writes in every caller,
statically when its path starts at a local that never escaped and by
the flag otherwise. Reads through another alias are allowed and see the
current value: a callback that reads the object in the middle of an
exclusive borrow observes the latest write. The optimizer may therefore
assume no other writer, but not that nobody reads; it keeps a store that
an aliased read could observe. Full Rust exclusivity, where reads are
excluded too, would cost a flag test on every read through a shared
reference. **Global `&mut T` gets no check of its own**, as in tier 1:
a counted reference cannot dangle. Only a write that would break a live
scoped borrow fails.

Priced with REFCOUNT_RUNTIME.md's numbers:

| | Cost |
| --- | --- |
| Memory | a few bits of the header word that also holds the release-and-trace descriptor (REFCOUNT_RUNTIME.md), so no header growth beyond its 8 bytes; buffers carry the same bits |
| Per borrow through counted storage | the page-table lookup for an interior address (measured free), then per storage on the path an increment with the flag set and a clear with the decrement: about one 2.1 ns pair each, unmeasured as a whole |
| Per write through a global reference | a load and a branch, like the copy-on-write uniqueness test, which measured free (102 ms against 102) |
| Locals that never escape, values, scoped references from such locals | nothing: the static checker proves them |
| Code size | one body per declaration; the tests sit at writes through global references |

The panic needs a write to an object through one global reference while
a scoped borrow taken through another is live. Its common form,
pushing to a list while iterating it, already panics in tier 1 as
`collection-pinned`. A program that never writes an object through one
alias while it borrows it through another never meets it. A count for
the borrow alone would not do: an alias that overwrites a string field
frees the buffer a view borrows. Compiling each function again for
global callers would avoid the check, but `&{} mut` would then be
exclusive only for some callers, so a header's promise would depend on
its caller.

## 7. Closures

### 7.1 Capture sets

```text
fn(i32) -> i32                 # global, kind `fn`: tier 1's function type
fn(i32) ->{xs, n} i32          # may capture at most xs and n; no longer valid than they are
fn mut(&{} T) ->{} void        # in a parameter: may capture anything that outlives the call
fn once() ->{conn} Result[void, NetError]
```

The set follows the arrow, as in Scala 3's `A ->{c, d} B`. A closure
type with no set is global: it may capture what tier 1 allows, and
nothing scoped. In a parameter, `{}` is the parameter's own lifetime, as
for references. A set lists what the closure borrows; a value captured
by copy or move needs no entry. Section 3's rules apply: global converts
to scoped, sets order by inclusion, and a result names its sources, as
in `fn view_of(s: string) -> fn(usize) ->{s} &{s} string`.

A scoped closure's environment is a record in the creating frame, so
making one allocates nothing, and its value is uncounted. A literal
passed to a scoped parameter cannot escape, so the optimizer may
specialize the callee for it with no environment at all.

A scoped closure may still outlive the frame that made it, as long as
what it borrows lives on:

```text
fn adder(n: &{n} i32) -> fn(i32) ->{n} i32:
    fn(x: i32) ->{n} i32: x + *n
```

The pointer to `n` stays valid, but `adder`'s frame does not. A scoped
closure returned past its creator's frame therefore gets a counted heap
environment, which still carries its `{n}` dependency, so the caller's
checks are unchanged. Only closures that stay in their frame keep the
zero-allocation environment. A captured reference is copied into the
environment as a reference value, keeping its sources: the closure holds
the pointer to `n` itself, never the address of `adder`'s parameter slot
that held it.

### 7.2 What a scoped closure captures

A closure literal is checked against its expected type, never
generalized from its body:

1. Against a global `fn`, captures follow tier 1: a written `let` moves
   to a counted cell, and the closure stays kind `fn`.
2. Against a scoped type, each capture is a borrow of the variable, and
   the borrows must be covered by the type's set. A variable the
   closure only reads is a shared borrow. A variable it writes is an
   exclusive borrow, allowed only when the expected kind is `fn mut` or
   `fn once`.
3. A literal may write its own kind and set, `fn mut(x: i32) ->{total}
   void: ...`. With neither written nor expected, it is global.

### 7.3 Call kinds

| Kind | Calling it needs | Copyable | Typical capture |
| --- | --- | --- | --- |
| `fn` | shared access; any number of calls, possibly at once | yes | values, shared borrows, global references and cells |
| `fn mut` | exclusive access to the closure | no: it moves | an exclusive borrow `&{} mut`, a written local, an owned NonCopyable value |
| `fn once` | the closure itself; the call consumes it | no | a NonCopyable value moved out by the body |

`fn` converts to `fn mut`, which converts to `fn once`, and global to
scoped, so a tier 1 `fn` is accepted everywhere. The kind is written in
every header that takes, returns or stores a closure. Two live closures
that write one variable conflict, as in Rust:

```text
fn tally(xs: List[i32]) -> i32:
    let total = 0
    add := fn mut(x: i32) ->{total} void: total += x
    sub := fn mut(x: i32) ->{total} void: total -= x
    # error: `sub` borrows `total` exclusively while `add` still does
    #   help: give both a global type, `fn(i32) -> void`, which shares `total` through a cell
    for x in xs.iter():
        if *x > 0: add(*x) else: sub(*x)
    total
```

`fn once` serves one-shot callbacks that hand off a NonCopyable value:

```text
fn on_done(callback: fn once() ->{} void) -> void

fn send_later(buf: Buffer, sink: &{} mut Sink) -> void:
    on_done(fn once() ->{sink} void: sink.send(buf.finish()))   # moves `buf` in, then out
```

## 8. Library Code For Both Tiers

### 8.1 Unannotated parameters stay global

A parameter `r: &T` with no set is global, as in tier 1. std writes
`{}` or a set on every borrowing parameter; application code writes
nothing and may store its reference parameters, as
`add_child(parent: &mut Node)` does. Borrowing for the call by
default, as Swift does for closure parameters, would put an escape
mark on every such function.

### 8.2 Generic parameters may be scoped

`List[&{l} Point]` is a scoped list of uncounted references, and
`List.push` needs no `global` bound, because the list's type carries
`{l}` (section 5.3).

### 8.3 Monomorphization

Lifetimes are erased after checking, except whether a lifetime
argument is `global`. `Parser[global]` and `Parser[{src}]` are two
instances, as `List[i32]` and `List[&Node]` are, because copying a
global view counts and copying a scoped one does not. A function's own
reference parameters never split it: `&{} T` is uncounted whatever the
caller passes, and the flag of section 6.4 is set by the caller.

### 8.4 std, written lifetime-aware

```text
impl[T < ~Copy] List[T]:
    fn get(&{} self, i: usize) -> Option[&{self} T]
    fn push(&{} mut self, x: T) -> void
    fn iter(&{} self) -> ListIter[{self}, T]
    fn iter_mut(&{} mut self) -> ListIterMut[{self}, T]
    fn sort_by(&{} mut self, compare: fn(&{} T, &{} T) ->{} Ordering) -> void
    fn span(&{} self, from: usize, to: usize) -> Span[{self}, T]

impl[K < Eq & Hash, V] Map[K, V]:
    fn get(&{} self, key: &{} K) -> Option[&{self} V]

impl string:
    fn split(&{} self, sep: &{} string) -> List[&{self} string]
    fn slice(&{} self, from: usize, to: usize) -> &{self} string

struct Builder ~Copy:
    buf: List[u8]

impl Builder:
    fn add(&{} mut self, s: string) -> &{self} mut Builder        # chains: b.add("a").add("b")
    fn finish(self) -> string                                     # takes ownership; reuses the buffer
```

Read-only parameters are `&{} T`, since a value parameter is an owned
copy; `push` takes `x` by value because it keeps it. A tier 1 caller
writes `names.sort_by(fn(a, b): a.cmp(b))` and `line.split(",")` with
no marks. On a local, `split` returns uncounted views that freeze the
string's variable. On a string behind a global reference it returns
the same uncounted views, with the object flagged while they live
(section 6.4). Storing such views in a global place promotes them.
Promotion applies only to view types whose representation always has a
counted buffer behind it: string views, spans and element references
into counted buffers. A single view gets one count on its buffer, with
no copy, which works because interior references are plain addresses.
An element reference or span is promoted this way only when its element
type is global (`T < global`): counting a list's buffer keeps a
`Token[{source}]` slot alive but not the source bytes its `text`
borrows, so storing `parsed.get(0)` globally is an error; the fix is to
convert the token value itself, or to parse with `Token[global]`.
A value holding views, such as `split`'s `List[&{line} string]`, is
converted to its global instance: a new outer buffer when the old one is
shared, and a count per view, so promoting n views costs n increments
and one buffer, never a byte copy. Scoped aliases of the old value keep
their type and their borrow. So tier 1 code that writes
`cache.words = line.split(" ")` keeps compiling, at tier 1's cost. A
general scoped reference `&{} T` may point into a stack frame, so it is
never promoted: storing one globally is an error, fixed by taking a
global `&T`. A string is immutable, so a shared reference to one refers
to its bytes, which lets `slice` and `split` return references;
`&mut string` refers to the variable.

`iter_mut` yields items that may all be alive at once, as Rust's does:
`ListIterMut[{L}, T]` has `next(&{} mut self) -> Option[&{L} mut T]`, so
each item lives for the list's borrow `L`, not for the call to `next`.
No borrow checker can see that two such items are disjoint, so std
builds the iterator on one checked low-level intrinsic that splits a
`&{L} mut` span into its first element and the rest, two
non-overlapping parts (Rust's `split_first_mut` and `split_at_mut`,
written with `unsafe` inside Rust's core). The same primitive gives
`chunks_mut`, the two halves of a parallel sort, and swapping two
elements through references.

## 9. Escape Analysis And Marks

| | Without marks | With marks |
| --- | --- | --- |
| Address-taken local or new object on the stack | when every reference provably dies in this body, inlined callees included | also when it goes only to `{}` parameters of any callee, in any module |
| Count elision | moves at last use and borrowed reads, from REFCOUNT_RUNTIME.md | every scoped reference is uncounted |
| Closure environment | in the frame when the closure stays in this body | in the frame whenever the closure goes to a scoped parameter |
| Facts across modules | none: a reference passed to an opaque call escapes | the callee's header: `{}`, result sets, closure sets and kinds |

A body edit that starts storing a `{}` parameter is an error in that
body.

## 10. Suspension, Providers And Boundaries

- A suspending body that borrows its own local across a bang call
  allocates its frame on entry (the Rust model study's section 2.2).
- A cold suspension built from a call with scoped arguments is scoped:
  `Suspend[{conn}, Row]` cannot outlive `conn`. Children of `all!` may
  borrow the parent's locals; two children may not both hold
  `&{} mut` of one place.
- Providers, top-level bindings and values that cross a boundary are
  global. A scoped value there is an error.

## 11. Tour

### 11.1 A tokenizer whose tokens borrow the source

```text
fn tokens(src: &{} string) -> List[Token[{src}]]:
    let mut lexer = Lexer { src, pos: 0 }        # section 2's Lexer, with `next(&{} mut self) -> Token[{s}]`
    let mut out: List[Token[{src}]] = []
    loop:
        match lexer.next():
            .End => return out
            t => out.push(t)
```

Tier 1's version (its section 11.4) made two count updates per token,
and one kept token kept the whole source alive. Here no token is
counted and none can outlive `src`. A caller that keeps tokens longer
builds `Lexer[global]` and gets tier 1's views.

```text
fn first_token(path: &{} string) -> Token[{path}]:
    text := read_file(path)
    tokens(&text)[0]      # error: the token borrows `text`, a local that ends here
```

### 11.2 A parser over borrowed input

```text
enum Expr[{s}]:
    Name(text: &{s} string)
    Num(value: f64)
    Add(left: &Expr[{s}], right: &Expr[{s}])  # recursion through `&`, as in tier 1

impl[{s}] Parser[{s}]:
    fn expr(&{} mut self) -> Result[&Expr[{s}], ParseError]:
        let left = self.term()?               # a reference to the first term's node
        while self.eat('+'):                  # advances past a `+` token, if one is next
            left = &Expr.Add(left, self.term()?)   # a new node: `&` on a value allocates
        .Ok(left)
```

Names in the tree are views of the source. The boxes of `Add` are
allocated as in tier 1, but nothing inside them is counted. The parser
holds its lexer by value, so `&{} mut self` covers both. A method
declared `-> Token[{s}]` that returned `*self.peek()` would be fine; one
declared `-> &{s} Token[{s}]` that returned `&self.peeked` is rejected,
because that reference borrows `self`, not `s`.

### 11.3 An in-place sort

```text
fn sort_scores(scores: &{} mut List[Score]) -> void:
    scores.sort_by(fn(a, b): b.points.cmp(a.points))

fn report(path: string) -> string:
    let mut scores = load_scores(path)
    sort_scores(&mut scores)        # `scores` stays in this frame: no cell, no count
    render(scores)
```

In tier 1, `sort_scores` taking `&mut List[Score]` would move `scores`
to a heap cell, and the comparator would be a counted closure. Here the
borrow is local, the comparator has no captures, and `sort_by` may
assume `scores` has no alias. Holding `top := scores.get(0)`
across the call is an error: `scores` is frozen while `top` lives.

### 11.4 An iterator borrowing a list

```text
fn total_weight(items: List[Item]) -> f64:
    let sum = 0.0
    for it in items.iter():         # ListIter[{items}, Item]: yields &{items} Item
        sum += it.weight
    sum

fn restock(items: &{} mut List[Item]) -> void:
    for it in items.iter():
        if it.count == 0:
            items.push(it.reorder())  # error: `items` is frozen while the loop's iterator lives
```

The loop takes no count and no pin. The wrong program panics with
`collection-pinned` in tier 1; here it does not compile.

### 11.5 A callback taking a scoped closure

```text
fn each_line(text: string, f: fn mut(&{} string) ->{} void) -> void:
    for line in text.split("\n"):
        f(line)

fn longest_line(text: string) -> usize:
    let best = 0
    each_line(text, fn mut(line) ->{best} void:
        if line.len() > best: best = line.len())
    best
```

The closure writes `best` through an exclusive borrow, with no cell and
no environment object, and `each_line` cannot store `f`.

### 11.6 A struct holding views into its own buffer

```text
struct Csv:
    text: string
    header: List[&{self.text} string]
    rows: List[List[&{self.text} string]]

fn parse_csv(text: string) -> Csv:
    lines := text.split("\n")
    Csv { text, header: lines[0].split(","), rows: [for l in lines.skip(1) => l.split(",")] }
```

A `Csv` is a global value that may be cached or returned, yet its cells
are uncounted views into its own text. `c.text = other` is an error,
since `text` is frozen; building a new `Csv` is the fix.

## 12. What Tier 2 Removes From RC's Three Costs

Counting instead of tracing costs three things. REFCOUNT_RUNTIME.md
measured each on V8; shares are of total run time, start-up included.

| Cost | What it costs, measured | Scoped borrows | NonCopyable | NonEscapable | Left for unmarked tier 1 code |
| --- | --- | --- | --- | --- | --- |
| 1. Interior pointers find the object's header | the page-table lookup measured free: 3.43 ns per copy and read against 3.44 for a base pointer | the count update through the pointer goes, so the lookup goes with it | nothing: a unique value has no count | views inside a span or iterator are uncounted | the lookup, which costs nothing measurable |
| 2. Count traffic on copies | 2.1 ns per increment and decrement pair. With tier 1's elisions, counted runs were within 4% of uncounted ones (binary-trees 2,059 against 2,221 ms; points 144 against 138). Without them, 18% to 19%. Counted word views against uncounted ones in map-count: 161 against 121 ms | uncounted scoped views and references: map-count's 25%, and the counts that tier 1's elisions miss across calls, bounded by the 18% | no count on the value itself; a builder's `finish` reuses its buffer | iterators and spans touch no header | counts on what is stored or returned; borrowed reads and last-use moves already remove most |
| 3. Cycles | trial deletion only for cycle-capable types: 206 ms on the parent-pointer program against 52 with weak references, 75% of it; 4.6 KB to 6.0 KB of collector code. None of the 54 types in `examples/`, the playground and the tour is cycle-capable | nothing for graphs: a scoped reference cannot be a back edge in a structure that outlives its frame | nothing: a NonCopyable value inside a counted object can still be on a cycle | nothing | all of it, in programs that build graphs with back edges |

**Does std carry the gain into unmarked code?** For costs 1 and 2,
partly. Tier 1 code that calls `line.split(",")` on a local string,
`xs.iter()` on a local list or `m.get(k)` on a local map gets
uncounted views with no marks of its own. On data reached through
global references it gets the same views, at one flag set and clear
per borrow. What it stores keeps tier 1's counts: map-count with a
`Map[string, i32]` stores a counted view per new key, and reaching the
borrowed-keys time takes one mark in the application,
`Map[&{text} string, i32]`. For cost 3, no.

**Cycles are where tier 2 does not help.** Rust has the same gap: a
tree with parent links needs `Weak`, an arena, or indices, and borrows
cannot express it. A capture set only lets a closure field that
captures values stop making its struct cycle-capable. What would cut
cost 3 is `Weak[T]` (tier 1's open point), priced at 52 against 206
ms, or arenas in std.

So tier 2 removes up to a quarter of run time in view-heavy text code,
a few percent in allocation-heavy code where tier 1's elisions already
work, nothing measurable from interior pointers, and nothing from
cycles. Its larger gains are the ones these programs do not measure:
heap cells for `&mut` locals passed across calls, closure environments,
and pins. The flag gives back a little of it on data behind global
references: a 2.1 ns pair per marked storage on a borrow's path, and a
free test per write.

## 13. Costs

**Run time.** Scoped references remove the costs listed at the start,
priced in section 12. The borrow flag takes a few bits of the header
word that holds the release-and-trace descriptor, a count pair with a
flag set and clear per marked storage on a scoped borrow's path through
counted storage, and a load and branch per write through counted
storage (section 6.4). NonCopyable values add a release at each scope exit and
a drop flag where a move happens on some paths.

**Compile time**, priced per stage:

| Dimension | Cost |
| --- | --- |
| Granularity | per body, after type checking; headers checked per declaration |
| Contention | none: a body reads callee headers only |
| Locality | one dataflow over each body's places and control flow |
| Allocation | a set of live loans per program point, in bodies that borrow |
| Superlinear risk | loans times program points: quadratic in a long body with many borrows (estimate); bodies with no scoped borrow skip the pass |
| I/O | none |
| Incremental | sets, kinds and `~Copy` are in the interface hash; a body edit rechecks that body |
| Code size | one body per function; generic instances split on a `global` lifetime argument as on a type argument |

The Rust model study estimated a full Rust checker at 10k to 15k lines
in `hd_check`; without moves of ordinary values, cross-function region
inference and full drop elaboration, an estimate here is 5k to 8k.

**Surface.** Sets, `{}`, `global`, lifetime parameters, closure sets,
`fn mut`, `fn once`, `~Copy`, two bounds, self-relative paths, one
checker's errors and the `borrow-conflict` panic. All but the panic
are opt-in: a program with no set and no `~Copy` is a tier 1 program,
and meets the panic only through std's scoped headers.

# Part 2: Retrofit

## 14. What Survives, Changes And Is New

Tier 2 adds to tier 1's retrofit (the tier-1 design (`58c3d470f:docs/future-work/OBJECT_MODEL_TIER1.md`) section 14) and
removes nothing that tier 1 keeps.

| hd today, or tier 1 | Under tier 2 |
| --- | --- |
| tier 1 `&T`, `&mut T` | unchanged: global |
| a header that cannot promise not to keep a reference | `&{} T`, `&{} mut T`, result sets |
| element references pin | a scoped borrow of a local freezes statically; one through a global reference sets the borrow flag; global references still pin |
| an 8-byte header: the count, and the release-and-trace descriptor | borrow-flag bits in the descriptor word; the `borrow-conflict` panic |
| one `fn` type; `mut` before `fn` is a syntax error | `fn`, `fn mut`, `fn once`, with capture sets; the plain `fn` keeps its meaning |
| "no mutable closure type" (Mutable Closures) | still true for global closures; scoped closures get kinds |
| tier 1 `string` views counted | `&{s} string`: uncounted views |
| `Disposable` and `using` for resources | unchanged; `~Copy` adds deterministic release and an optional synchronous `drop` |
| Wasm GC backend | tier 2 checks run there; scoped references are emitted as tier 1 references and the flag as a field of each object, until the RC backend reaches parity and Wasm GC is deleted |

**Earlier designs return smaller.** `{}` on a parameter is the removed
`^` mode (chapter 13, deleted in R4), still never marked at the call;
sets add what `^` could not do: return a borrow, store one in a struct,
iterate without a closure. `^` was parked because Wasm GC could not
exploit it; counted linear memory can. The earlier lifetime sets
(`^{a}`, removed in `101344d4c`) return without elision and requirement
keys, with `static` renamed `global`. The earlier `own` mode (removed
in `a04c3497c` and `8a77d0273`) returns as `~Copy` on the type, which
replaces `OwnOnly`; `DropOwn.drop` becomes the optional synchronous
`drop`. Moves, use-after-move and drop flags return; freshness rules
and partial moves do not.

## 15. Migration

Tier 2 is opt-in: a program writes no marks unless it wants them. When
std gains tier 2 headers, a tier 1 program can change in two ways: a
few programs of the shapes below stop compiling, and a program whose
callback writes, through another alias, an object that std now borrows
for the call panics with `borrow-conflict` where it used to run (section
6.4). Neither change alters what a program that still compiles and runs
computes. std gains those headers by hand, chunk by chunk. A parameter that gains `{}` breaks no caller,
because global arguments still convert; it breaks only a std body that
stored what it now promises not to keep, which the checker reports in
that body. A result that gains a set and is stored in a global place
is promoted when its shape allows (a view type always backed by a
counted buffer, or a value holding such views, section 8.4); any other
scoped result stored globally is rejected. A caller
that holds a borrowed result across a change to its source is rejected,
and rightly:

```text
let mut xs = load()
first := xs.get(0)     # a reference into xs
xs.push(x)             # error: xs is frozen while `first` lives
use(first)
```

This program was already wrong: in tier 1 the push panics with
`collection-pinned`, because `first` pins `xs`. Tier 2 reports the same
bug at compile time. Replacing the whole list, `xs = []`, while `first`
lives is rejected too, as in Rust, though tier 1 would run it safely
with `first` reading the old buffer. Reading a reference into a list
just replaced is almost always a mistake, so tier 2 std may reject the
few safe tier 1 programs of this shape: a borrowed result held across a
change or replacement of its source. The fix-it copies the value out
first, when it can: a value that holds a `&mut` at any depth cannot be
copied out of a borrow, so there the fix is to take a global reference
in place of the borrowed result. Applications adopt marks where
measurement shows a cost.

## 16. Implementation Plan

Each job is about an hour, ships its spec rules and fixtures, and
keeps every check green. Jobs 1 to 17 are the language and
run on the current backend after tier 1's Wasm GC emulation: scoped
references are emitted as counted ones and the flag as an object field,
so run fixtures, the `borrow-conflict` panic included, pass while
gaining no speed. Jobs 18 to 20 migrate std. Jobs 21 to 24 follow
the RC backend of REFCOUNT_RUNTIME.md. Tier 2 starts after tier 1's
jobs 1 to 10.

| # | Job | Spec and fixtures |
| --- | --- | --- |
| 1 | Grammar: sets on `&`, `{}`, `global`, lifetime parameters `[{s}]` and arguments, closure sets `->{...}`, `fn mut`, `fn once`, `~Copy`; parser | a "Lifetimes" chapter skeleton and grammar productions; parse fixtures, valid and invalid |
| 2 | Header checks: names in sets (parameters, `self`, `pub` paths, lifetime parameters, `global`), `{}` placement, missing argument means global | header rules; a fixture per error |
| 3 | Set algebra: global to scoped, inclusion, joins, exactness behind `mut`, expected-type rule for `&e` | conversion rules; check fixtures |
| 4 | Escape checks in bodies: scoped values not captured by global closures, results covered by the header; a scoped value stored globally is promoted when its bytes are in a counted buffer, an error when they are on the stack; projection of self-relative fields | escape and promotion rules; a fixture per error, with fix-its |
| 5 | Lifetime parameters on structs and enums: scoped-contains rule, variance, `Token[global]` | type rules; check fixtures |
| 6 | Loans 1: shared and exclusive loans on local places, liveness, freezes, length-changing calls | exclusivity rules; a fixture per conflict |
| 7 | Loans 2: two-phase borrows, reborrows through scoped references, field disjointness, elements as one place | rules and fixtures |
| 8 | Loans at calls: loans from header sets, owned value parameters, temporaries | call rules; check and run fixtures |
| 9 | Borrow flag: scoped borrows through counted storage (global references, escaped locals, pinned buffers) mark every storage on the path; writes test the storage they write; `borrow-conflict` | flag rules; run fixtures for each conflict, an older alias writing under a newer view, and disjoint fields through one variable |
| 10 | Closures 1: capture sets, scoped literals checked against expected types | capture rules; fixtures |
| 11 | Closures 2: kinds, copyability, conversions, exclusive captures, two live `fn mut` | kind rules; a fixture per error |
| 12 | Generics: `T < global`, scoped type arguments, places that need `global` (providers, tasks, `dyn`, boundaries) | bound rules; fixtures |
| 13 | NonCopyable 1: `~Copy` on declarations, conditional copyability, `T < ~Copy` | rules; fixtures |
| 14 | NonCopyable 2: moves, value parameters that take ownership, use after move, no partial moves, `fn once` captures, moves out of references | move rules; a fixture per error |
| 15 | NonCopyable 3: release at scope exits, drop order, synchronous `drop`, `Disposable` interplay | release rules; run fixtures for order |
| 16 | Self-relative paths: `{self.f}`, frozen fields, literal rule | rules; fixtures |
| 17 | Suspension: borrows across bang calls, scoped cold suspensions, borrows in `all!` children | rules; fixtures |
| 18 | std 1: `List`, `Map`, iterators, `Span`, `sort_by` with tier 2 headers; the span-splitting intrinsic and `iter_mut` on it | std and intrinsic rules; fixtures, including two live `iter_mut` items |
| 19 | std 2: text (`slice`, `split`, `Builder`), formatting, encoding | std rules; fixtures |
| 20 | std 3: the rest of std's writing methods | std rules; fixtures |
| 21 | RC backend: uncounted scoped references; instances split on `global` lifetime arguments; the flag word and its tests | none (unobservable); count totals and flag tests on the tour programs |
| 22 | RC backend: locals behind `{}` borrows in the shadow-stack frame; scoped closure environments in the frame, a counted environment for one returned past its frame | allocation counts on the tour programs |
| 23 | RC backend: NonCopyable release glue and drop flags | run fixtures from job 15 on the new backend |
| 24 | No-writer optimization for `&{} mut`: the backend may keep values in registers across calls that cannot write them, keeping stores an aliased read could observe | benchmark: sort and points from the Rust study |

## 17. Open Points

- **Spelling.** `~Copy` against a word such as `@noncopy`; `[{s}]` for
  lifetime parameters against `[&s]`.
- **Shared references to strings refer to bytes**, so `r := &s` then
  `s = "x"` leaves `r` reading the old string. This differs from tier
  1's rule that a reference sees its slot; it is what lets `split`
  return references.
- **Payload borrows** of locals, as Rust's `if let Some(f) = &mut x`,
  which tier 1's non-place payloads forbid; until then std's lending
  callback (tier 1 section 6.1) changes a payload in place.
- **Non-local `return` from scoped trailing blocks**, as Kotlin's
  inline lambdas.

# Part 3: Local References

Local references are tier 2's first cut: a non-escaping reference that
needs no lifetime syntax ([#834](https://github.com/HerringtonDarkholme/hd-lang/issues/834)).

**Step 1 is applied.** Today's reference is spelled `*T` / `*mut T`, a
counted reference that may be stored, returned and captured. `&T` /
`&mut T` is a local reference that never escapes: a field, an element,
a result, a top-level binding, a closure capture or a type argument
that would keep one is `local-reference-escapes`, and `*T` lends to
`&T`, never the reverse. `&x` is local where a `&T` is expected and
counted elsewhere, so `let a = &s` is counted. The kind is a checking
rule only: counting and codegen are unchanged. The rules are in
[Local References](../../spec/lang/13-values-and-references.md#local-references)
(commits d354a1e49 and ea28c85b8).

**Later steps, each needing its own approval.** Step 2, interior places get only local references so a `*T` count skips `block_of` (#704), is not adopted: it needs too much language change. #704 shipped without it, as a release-only whole-program fact (`crates/hd_opt/src/whole.rs`): a `*T` counts at its object's start when the program makes no counted interior reference to `T`. #912 explores a separate creation form for `*T`. The design below goes
further, as the owner sketched it on 2026-10-24: local references are
never counted; an interior place (a field, an element, a map value) has
only a local reference, so a count of a `*T` always lands on an
object's start and needs no `block_of` (#704); local references appear
only as arguments and receivers; overlapping accesses in one call are a
compile-time error, with no exclusivity check at run time; a borrow
through a heap alias takes one fast-path count; element borrows keep the
pin; there is no `Generic[&T]`; a local receiver may return `self`, so
builders and the Debug writer return themselves; interpolated interior
parts are copied; `&fn` marks a closure parameter that is only called;
an impl may be more local than its trait; and #702's frame slots follow.
The full design, with its architecture, cost and slice plan, is in git
history at `591ee34a9:docs/future-work/LOCAL_REFERENCES.md`.

## 18. The Design

hd gets two kinds of reference. A **counted** reference, `*T` or
`*mut T`, is Go's pointer to a whole object. It may be stored, returned,
captured and used as a type argument. A **local** reference, `&T` or
`&mut T`, is Swift's `inout`: a parameter, a call argument or a
receiver, and never counted.

`&x` takes an address, and the expected type picks its kind. `*p`
dereferences either kind. A field or an element has only a local
reference. So every counted reference points at an object's start, and
every count takes the fast path, with no `block_of`.

The design is sound with no lifetimes and no flow analysis. A local
reference lives for one call. A compile-time check rejects an argument
that may replace what another argument of the call borrows. A borrow
reached through a heap alias takes one fast-path count of its object.

| | Counted: `*T`, `*mut T` | Local: `&T`, `&mut T` |
| --- | --- | --- |
| refers to | a whole object: a local, a value parameter, the referent of a `*T`, a new object | any place: a whole object, a field, an element, a map value |
| counted | yes, always on the fast path | never |
| written as a type | anywhere a type may be | a parameter's type only, a function type's parameter included |
| as a value | anywhere | a call argument, a receiver, or read and written through |
| stored, returned, bound, captured, a type argument | yes | no; a returned `self` and a closure passed to `&fn` are the two exceptions |
| made by | `&x` where a `*T`, a type variable or no type is expected | `&x` where a `&T` is expected, and every `&self` or `&mut self` call |
| receivers | `*self`, `*mut self`: methods that keep `self` | `&self`, `&mut self` |
| like | Go's `*T` | Swift's `inout` |

What it gives: a call that borrows a local, a field or an element of a
local list takes no count. Borrows behind a heap alias, and every count
of a `*T`, take the fast path. What it gives up: local results other
than `self`, bound aliases of fields and elements, local type arguments,
and in-place edits through a returned reference. [Escape
Hatches](#22-escape-hatches) shows what to write instead.

## 19. The Rules

The rules are numbered for this document; each carries the rule ID that
slices 1 and 2 give it in the spec. Comments in the examples name the
rule a line shows. Since step 1, `*T` and `*self` parse; lines that write `&fn`, or that
rely on a later step, end in `# hypothetical syntax`.

### 19.1 Two kinds

1. `ref.kind.counted` `*T` and `*mut T` are counted references. A
   counted reference always refers to a whole object.
2. `ref.kind.local` `&T` and `&mut T` are local references. A local
   reference may refer to any place, and is never counted.
3. `ref.kind.whole-object` A whole object is a local, a value parameter,
   the referent of a `*T`, or a new object. Every other place is an
   **interior place**: a field, a list element, a map value, or anything
   reached through a `&T`.
4. `ref.kind.interior-local` An interior place has only a local
   reference. So a count of a `*T` always lands on an object's start and
   takes the fast path, with no `block_of`.

```text
struct Body:
    x: f64
    v: f64

struct World:
    bodies: List[*mut Body]  # hypothetical syntax

fn push_x(x: &mut f64, by: f64) -> void:
    *x += by

fn demo(w: &mut World) -> void:
    let b = &mut Body { x: 0.0, v: 1.0 }  # 1, 3: a counted *mut Body to a new object
    w.bodies.push(b)  # 1: a counted reference may be stored
    push_x(&mut b.x, 1.0)  # 2, 4: b.x is an interior place, so the borrow is local
```

### 19.2 Taking an address and lending

5. `ref.expr.address` `&x` and `&mut x` take an address. Where a `&T` is
   expected the result is a local reference, and where a `*T` is
   expected a counted one.
6. `ref.expr.address.counted-default` Where a type variable is expected,
   or no type is, `&x` is counted. So `let a = &s`, `xs.push(&a)` and
   `[&a, &b]` make counted references.
7. `ref.expr.address.whole` A counted `&x` needs a whole object. On an
   interior place it is an error, so `let a = &obj.f` and
   `let a = &xs[i]` are errors. Error: `local-reference-escapes`.
8. `ref.expr.deref` `*p` dereferences either kind. Field reads, method
   calls and patterns read through either kind, as today.
9. `ref.convert.lend` A `*T` converts implicitly to `&T` where a local
   reference is expected, and a `*mut T` to `&mut T` and to `&T`. The
   conversion takes no count.
10. `ref.convert.no-raise` A `&T` never converts to a `*T`. Error:
    `local-reference-escapes`.
11. `ref.convert.weaken` `&mut T` weakens to `&T`, and `*mut T` to `*T`,
    as today. Weakening a local reference takes no count.

```text
struct P:
    n: i64

fn read(p: &P) -> i64:
    p.n

fn keep(p: *P) -> *P:  # hypothetical syntax
    p

fn demo(a: P, b: P) -> i64:
    let mut xs = []
    xs.push(&a)  # 6: T is a type variable, so xs is a List[*P]
    ys := [&a, &b]  # 6: also a List[*P]
    k := keep(&a)  # 5: *P is expected, and a is a whole object
    read(&a) + read(k) + read(xs[0])  # 5: local; 9: a *P lends to &P

fn invalid(p: &P, o: P) -> void:
    let a = &o.n  # 7: error: an interior place has no counted form
    k := keep(p)  # 10: error: a local reference never becomes a *P
```

### 19.3 Where a local reference may appear

12. `ref.local.positions` A local reference type is written only as the
    type of a parameter, a function type's parameter included. A local
    reference value appears only as a call argument, as a receiver, or
    as the operand of a read or a write through it.
13. `ref.local.parameter` A local reference parameter is valid for the
    whole call. The body may read and write through it, and pass it on
    as an argument any number of times, at no cost.
14. `ref.local.no-binding` A local reference is never bound to a name.
    That covers `let`, `:=`, an assignment, and a pattern that binds a
    whole local-reference scrutinee. `_ :=` discards a value and binds
    nothing. Error: `local-reference-escapes`.
15. `ref.local.no-result` A local reference is never a result, except a
    method's returned `self` (rule 21). Error: `local-reference-escapes`.
16. `ref.local.no-store` A local reference is never a field, an element,
    a payload, a tuple element or a top-level binding. Error:
    `local-reference-escapes`.
17. `ref.local.no-type-argument` A local reference type is never a type
    argument, as Swift's `inout` is no type. There is no `List[&T]`,
    `Option[&T]`, `&T?` or `T = &Foo`. Error: `local-reference-escapes`.
18. `flow.for.ref.binding` (**proposed**: #834 does not mention loops)
    `for p in &mut xs` binds a local reference to each element for one
    step. This keeps today's
    [Element Reference Loops](../../spec/lang/06-control-flow.md#element-reference-loops).
    The body uses `p` as it uses a local parameter.

```text
struct Lexer:
    src: string
    pos: usize

impl Lexer:
    fn at_end(&self) -> bool:
        self.pos >= self.src.len()

    fn skip_space(&mut self) -> void:
        while !self.at_end() && self.src[self.pos] == 32u8:  # 13: self passed on, free
            self.pos += 1

fn count_words(lexer: &mut Lexer) -> usize:  # 12: a local parameter
    let n: usize = 0
    while !lexer.at_end():
        lexer.skip_space()  # 13
        n += 1
    n

enum Expr:
    Num(n: i64)
    Neg(inner: *Expr)  # hypothetical syntax

struct Holder:
    lexer: &Lexer  # 16: error: write *Lexer

fn invalid(lexer: &mut Lexer, e: &Expr, all: List[&Lexer]) -> &mut Lexer:  # 15, 17: errors
    q := lexer  # 14: error: a local reference bound to a name
    match e:
        .Num(n) => pass  # a payload binding is a copy, as today
        whole => pass  # 14: error: whole would bind the local reference e
    q

struct Particle:
    x: f64
    v: f64

fn advance(ps: &mut List[Particle], dt: f64) -> void:
    for p in &mut ps:  # 18: one element per step
        p.x += p.v * dt
```

### 19.4 Receivers

19. `fn.method.receiver.local` `&self` and `&mut self` are local
    receivers. So a method call on a field works, as in
    `client.request.header(..)` and `self.items.push(x)`.
20. `fn.method.receiver.counted` A method that keeps `self` takes
    `*self` or `*mut self`, as Go's pointer receivers do. It needs a
    whole object, so a call on an interior place or through a local
    reference, `&dyn` included, is an error. Error:
    `local-reference-escapes`.
21. `fn.method.returns-self` A method with a local receiver may return
    `self`, with the result type `&Self` or `&mut Self`. The result has
    the caller's kind: counted when the receiver is a whole object, and
    local, for the rest of the calling expression, otherwise. Only
    `self` itself is returned this way.

```text
struct Request:
    url: string
    headers: List[(string, string)]

impl Request:
    fn header(&mut self, name: string, value: string) -> &mut Request:  # 21
        self.headers.push((name, value))
        self

    fn register(*mut self, log: &mut List[*mut Request]) -> void:  # hypothetical syntax
        log.push(self)

struct Client:
    request: Request

fn configure(client: &mut Client, token: string, log: &mut List[*mut Request]) -> void:  # hypothetical syntax
    _ := client.request.header("accept", "json").header("auth", token)  # 19, 21: local in this expression
    let mut r = Request { url: "/", headers: [] }
    rr := r.header("a", "b")  # 21: r is a whole object, so rr is a counted *mut Request
    r.register(log)  # 20: valid on a whole object
    client.request.register(log)  # 20: error: client.request is an interior place
```

### 19.5 Overlapping access

22. `ref.access.overlap` Within one call, an argument that may write a
    place conflicts with another argument that borrows a place inside
    it. The receiver counts as an argument. Error: `conflicting-access`,
    with the message "overlapping accesses to `o`".
23. `ref.access.write` An argument may write a place when it takes
    `&mut` of it, or calls a `&mut self` or `*mut self` method on it.
    These are the two rows of today's
    [`ref.elem.conflict`](../../spec/lang/13-values-and-references.md#call-scoped-access)
    table. A place is inside another when the other is a proper prefix
    of it, through fields, elements or `*T` referents.
24. `ref.access.disjoint` Two borrows of one place, or of places neither
    of which is inside the other, do not conflict. `&mut` stays
    non-exclusive otherwise
    ([`ref.write.no-exclusivity`](../../spec/lang/13-values-and-references.md#writing-through-a-reference)).
25. `ref.access.heap` The check follows places from a local or a
    parameter of the calling body. A borrow through a `*T` read out of a
    field or an element is out of its sight. Such a borrow takes one
    fast-path count of its object instead
    ([Counting Rules](#21-counting-rules)), and nothing is checked at run
    time.

```text
struct Obj:
    n: i64

fn add(p: &mut i64, x: i64) -> void:
    *p += x

fn reset(o: &mut (*mut Obj)) -> i64:  # hypothetical syntax
    *o = &mut Obj { n: 0 }
    5

fn both(a: &mut i64, b: &mut i64) -> void:
    *a += 1
    *b += 1

fn demo(i: usize, j: usize) -> void:
    let mut o: *mut Obj = &mut Obj { n: 1 }  # hypothetical syntax
    add(&mut o.n, reset(&mut o))  # 22, 23: error: overlapping accesses to `o`
    x := reset(&mut o)
    add(&mut o.n, x)  # valid: the write ended before the borrow
    let mut xs: List[i64] = [1, 2, 3]
    both(&mut xs[i], &mut xs[j])  # 24: valid, even when i == j
    add(&mut xs[0], xs.remove_at(1))  # 22, 23: error: remove_at writes xs
```

> **Why.** Without rule 22, `reset` could free the object that
> `&mut o.n` points into while `add` still writes through it. Rejecting
> only writes to an enclosing place keeps `swap(&mut xs[i], &mut xs[j])`
> valid, which Swift rejects.

### 19.6 Element borrows

26. `ref.elem.local` `&xs[i]`, `&mut xs[i]`, `&m[k]` and `&mut m[k]` are
    local references, so an element borrow lasts for one call, as today.
27. `ref.elem.access.pin` The borrow pins its collection and takes one
    fast-path count of the buffer while the call runs. A length change
    through an alias panics `collection-pinned`, and a copy of the
    pinned list gets elements of its own
    ([`ref.elem.copy`](../../spec/lang/13-values-and-references.md#element-references)).

```text
fn write_then_read(slot: &mut i64, copy: List[i64]) -> i64:
    *slot = 99
    copy[0]  # 1: the copy has elements of its own

fn demo() -> i64:
    let mut ys: List[i64] = [1, 2]
    write_then_read(&mut ys[0], ys)  # 27: the pin makes the copy eager
```

### 19.7 Closures

28. `fn.type.local` `&fn(A) -> B` is a non-escaping function parameter.
    The callee may only call it: storing, returning, binding or
    capturing it is an error. Error: `local-reference-escapes`.
29. `fn.type.lend` (**proposed**) A plain `fn` value may be passed where
    a `&fn` is expected, as a `*T` lends to `&T`. Without it, every call
    that passes a named function to an eager helper breaks.
30. `fn.closure.local-capture` A closure literal written as the argument
    for a `&fn` parameter may capture local references, `self` of a
    `&self` or `&mut self` method included.
31. `fn.closure.no-local-capture` Any other closure that captures a
    local reference is an error. A plain `fn(A) -> B` stays a storable
    value. Error: `local-reference-escapes`.

```text
struct Scaler:
    k: f64
    total: f64

fn each(xs: &List[f64], f: &fn(f64) -> void) -> void:  # hypothetical syntax
    for x in xs:
        f(x)  # 28: the callee only calls f

impl Scaler:
    fn add_all(&mut self, xs: List[f64]) -> void:
        each(&xs, fn(x: f64) -> void: self.total += x * self.k)  # 30: captures self
        lazy := xs.iter().map(fn(x: f64) -> f64: x * self.k)  # 31: error: map keeps a plain fn
        k := self.k
        later := xs.iter().map(fn(x: f64) -> f64: x * k)  # 31: valid: captures a copy
```

### 19.8 Generics, traits and trait values

32. `trait.dyn.kinds` `&dyn Tr` and `&mut dyn Tr` are local trait value
    references, and `*dyn Tr` and `*mut dyn Tr` counted ones. A `*dyn`
    lends to a `&dyn` by rule 9.
33. `trait.dyn.provider` A provider installed with `$.with` is a
    `*dyn` value, since a fetched provider may be captured
    ([`fn.capture.provider-value`](../../spec/lang/07-functions.md#captures)).
34. `trait.impl.more-local` An impl method may be more local than its
    trait: `&self` or `&T` where the trait says `*self` or `*T`. The
    reverse is an error. Error: `trait-method-signature`.
35. `trait.dyn.counted-receiver` A trait with a `*self` method is still
    usable as `&dyn Tr`. Calling that method through a `&dyn` is rule
    20's error.

A generic parameter such as `xs: &List[T]` is valid: the reference is
local, and `T` is an ordinary type argument. `id(p)` with a local `p`
would make `T = &P`, which is rule 17's error.

```text
trait Shape:
    fn area(&self) -> f64

trait Registry:
    fn register(*mut self) -> void  # hypothetical syntax

struct Sq:
    side: f64

impl Shape for Sq:
    fn area(&self) -> f64:
        self.side * self.side

impl Registry for Sq:
    fn register(&mut self) -> void:  # 34: more local than the trait
        self.side += 0.0

fn total(shapes: &List[*dyn Shape]) -> f64:  # hypothetical syntax
    let sum = 0.0
    for s in shapes:
        sum += s.area()  # 32: a *dyn Shape lends to &dyn Shape
    sum

fn id[T](x: T) -> T:
    x

fn twice(p: &i64) -> i64:
    *id(p)  # 17: error: T = &i64
```

### 19.9 Suspension

36. `req.suspend.local-arguments` A bang call `f!(..)` may pass local
    references. They stay valid across any number of suspensions,
    because the caller is suspended inside the call.
37. `req.suspend.cold.local` A plain call of a bang function builds a
    cold suspension that keeps its arguments
    ([`req.suspend.cold.captures`](../../spec/lang/11-requirements-and-suspension.md)).
    A local reference argument there is an error. Error:
    `local-reference-escapes`.

```text
use std.time.{Clock, sleep, ms}

fn bump!(p: &mut i64) -> void $ Clock:
    sleep!(ms(1))
    *p += 1

fn outer!(n: i64) -> i64 $ Clock:
    let mut local: i64 = n
    bump!(&mut local)  # 36: valid across the sleep
    local

fn later(p: &mut i64) -> mut Suspend[void] $ Clock:
    bump(p)  # 37: error: the suspension would keep p
```

### 19.10 Boundaries

38. `ref.local.boundary` (**proposed**: a review finding #834 does not
    decide) An exported or host-imported function takes no local
    reference parameter. The host may keep what it receives, and
    `boundary.rs` counts each reference that crosses. Write `*T` or a
    value. Error: `local-reference-escapes`.

```text
pub fn store_name(name: &string) -> void:  # 38: error if exported to the host; write *string
    pass
```

### 19.11 std

39. `std.ref.parameters` Every std reference parameter is `&T` or
    `&mut T`, except the about 17 that keep their argument, which are
    `*mut T`. std's 19 reference-typed fields and its returned
    references are `*T`. Its methods that keep `self` take `*self`.
40. `std.fn.eager` std's eager helpers take `&fn`, and its lazy adapters,
    such as `Iterator.map`, keep a plain `fn`. #834 names `fold`,
    `retain`, `sort_by`, `any` and `each`; in today's std the eager ones
    include `List.map`, `filter`, `any`, `all`, `find`, `sorted_by` and
    `Iterator.fold`.
41. `std.format.debug-writer` `debug_struct`, `debug_tuple`, `debug_list`
    and `debug_map` return the writer itself, `&mut DebugWriter`, under
    rule 21. The open struct or list state lives in `DebugWriter`, so no
    `debug` signature changes.
42. `std.text.template` The `r` prefix takes `Template[*dyn Display]`. A
    part that is not a whole object, such as `${p.name}`, is copied into
    a new object. That costs one allocation per such part, until #702's
    frame storage removes it.
43. `std.error.erased` The erased error is `Result[T, *dyn Error]`, since
    `&dyn Error` would be a type argument. `?` already copies an error
    value into a new object, so errors lose nothing but spelling.

Merging the four builders into `DebugWriter` puts two pairs of
same-named methods on one type. The struct's `field(name, value)` meets
the tuple's `field(value)`, and the list's `entry(value)` meets the
map's `entry(key, value)`. **Proposed** std naming: `field(name, value)`
for named fields (absorbing `named_field`), `item(value)` for positional
ones, `entry(key, value)` for map pairs, and one `finish()`.

```text
use std.format.DebugWriter

struct Point:
    x: i64
    y: i64

impl Debug for Point:
    fn debug(self, out: &mut DebugWriter) -> void:
        out.debug_struct("Point").field("x", self.x).field("y", self.y).finish()  # 41: unchanged
```

## 20. Error Messages And Fix-Its

`local-reference-escapes` is the one new code. Each situation has its own
message, and fix-its are listed in the order the diagnostic offers them.
**Proposed:** today's `element-reference-outlives-call` retires into it,
since an element borrow is now an ordinary local reference.

| # | Case | Written | Code | Message | Fix-its |
| ---: | --- | --- | --- | --- | --- |
| E1 | a `let` of an interior place (7) | `let a = &obj.f`, `let a = &xs[i]` | `local-reference-escapes` | "`obj.f` is a field, and `let` makes a counted reference, which needs a whole object" | copy: `let a = obj.f`; pass the borrow straight to the call: `bump(&obj.f)`; make the field its own object: `f: *T` |
| E2 | an interior place where a type variable is expected (6, 7) | `xs.push(&o.f)`, `[&o.f]`, `id(&o.f)` | `local-reference-escapes` | "a type parameter takes a counted reference, and `o.f` is a field" | copy into a local, `c := o.f`, then `xs.push(&c)`; make the field `*T` |
| E3 | a local reference where a `*T` is expected (10) | `keep(p)` with `p: &P` and `fn keep(p: *P)` | `local-reference-escapes` | "`p` is a local reference, and `keep` keeps its argument" | make the callee's parameter `&P` when it keeps nothing; copy: `c := *p`, then `keep(&c)`; take `p: *P` here too |
| E4 | a local reference bound to a name (14) | `q := p`, `r = p`, `whole =>` on a `&T` scrutinee | `local-reference-escapes` | "a local reference can't be bound to a name; use `p` itself" | use `p` at each use; copy the value: `q := *p` |
| E5 | a local reference as a result (15) | `fn f(p: &mut P) -> &mut P: p` | `local-reference-escapes` | "a local reference can't be returned; only a method's `self` can" | take and return `*mut P`; return a copy, `-> P` |
| E6 | a local reference type in a field, element, payload or type argument (16, 17) | `r: &T` in a struct, `List[&T]`, `&T?`, `.Some(p)`, `[p]`, `dbg(p)` | `local-reference-escapes` | "a field can't hold a local reference" (or "an element", "a type argument") | in a type, write `*T`; for a value, copy it with `*p` |
| E7 | a `*self` method on an interior place or through a local reference (20, 35) | `client.request.register(log)` | `local-reference-escapes` | "`register` keeps `self`, so it needs a whole object, and `client.request` is a field" | make the field its own object: `request: *mut Request`; make the method `&mut self` when it keeps nothing |
| E8 | a closure capturing a local reference, not passed to a `&fn` (31) | `xs.iter().map(fn(x: f64) -> f64: x * self.k)` | `local-reference-escapes` | "the closure captures `self`, a local reference, and `map` may keep the closure" | copy the field first: `k := self.k`; make the callee's parameter `&fn` when it only calls it; write a loop |
| E9 | a `&fn` parameter kept (28) | `self.handler = f` with `f: &fn(i64) -> void` | `local-reference-escapes` | "`f` is a `&fn` parameter, which can only be called" | make the parameter a plain `fn` |
| E10 | a plain call of a bang function with a local argument (37) | `fill(b)` with `b: &mut Buf` | `local-reference-escapes` | "a plain call of `fill!` builds a suspension that keeps `b`" | call it: `fill!(b)`; take `b: *mut Buf` |
| E11 | overlapping accesses in one call (22) | `add(&mut o.n, reset(&mut o))` | `conflicting-access` | "overlapping accesses to `o`: `reset(&mut o)` may replace what `&mut o.n` points into" | evaluate the writer first: `x := reset(&mut o)`, then `add(&mut o.n, x)` |
| E12 | an impl less local than its trait (34) | `fn area(*self) -> f64` for a trait's `fn area(&self)` | `trait-method-signature` | "`Shape.area` takes `&self`; an impl may be more local than its trait, never less" | take `&self` |
| E13 | a local reference in a boundary signature (38, proposed) | an exported `fn store_name(name: &string)` | `local-reference-escapes` | "a host may keep what an exported function receives, so `name` can't be a local reference" | write `*string`, or take the value |

At run time, a length change of a collection under an element borrow,
made through an alias the checker can't see, panics `collection-pinned`
as today.

## 21. Counting Rules

The rows say where a borrow's address is and what keeps that storage
alive for the call. They are the earlier R3″ table, reduced. With
local references only in argument position, there are no bound borrows,
no binding groups and no delayed drops. Every count is released when
the call returns.

| Row | The borrowed address is in | Example | How it stays valid | Cost per borrow |
| ---: | --- | --- | --- | --- |
| 1 | a local's or value parameter's own storage, or a local reference parameter's referent, through fields only | `energy(&b)`, `lexer.skip_space()`, `f(&mut p.pos)`, `self.items.push(x)` | the local, or the caller's lent referent, outlives the call; rule 22 stops another argument replacing it | none |
| 2 | an object reached through a `*T` that a local or parameter of this body holds, when no closure captures that binding and no `*T` refers to it | `bump(&mut o.n)`, `o.step(dt)` with `o: *mut Obj` | the binding's own count holds the object; only this body can write the binding, and rule 22 rejects an argument that does | none |
| 3 | an object reached through any other `*T`: a field, an element, a captured binding, or a binding some `*T` refers to | `bump(&mut h.leaf.n)`, `bump(&mut nodes[i].n)`, `self.leaf.step(dt)` | one fast-path count of the object, taken at the borrow and released when the call returns | one known pair |
| 4 | a list element or a map value, with no `*T` step after the element step | `ps[i].step(dt)`, `swap(&mut xs[i], &mut xs[j])`, `for p in &mut ps` | today's pin and one fast-path count of the buffer: a length change panics `collection-pinned`, and a copy of the pinned list copies its elements | one known pair and the pin; both omitted for an unaliased local list that no other argument names |

A borrow is classified by the steps after the last `*T` dereference in
its place, since those say which block the address is in. So
`bump(&mut nodes[i].n)` with `nodes: List[*mut Node]` counts the node
(row 3) and needs no pin. An element loop is one borrow for the whole
loop, as [`flow.for.ref.access`](../../spec/lang/06-control-flow.md#element-reference-loops)
states today.

Three rules hold for every row. Every count of a `*T`, whether a copy,
a store or a row 3 count, lands on an object's start and takes the known
path. Weakening a local reference, and passing a local parameter on,
emit no count. No count ever has a local reference as its operand,
which the ownership verifier checks.

```text
struct Leaf:
    n: i64

struct Holder:
    leaf: *mut Leaf  # hypothetical syntax
    counts: List[i64]

fn bump(p: &mut i64) -> void:
    *p += 1

fn demo(h: &mut Holder, o: *mut Leaf) -> void:  # hypothetical syntax
    let mut local = Leaf { n: 0 }
    bump(&mut local.n)  # row 1: free
    bump(&mut o.n)  # row 2: o's own count holds the leaf
    bump(&mut h.leaf.n)  # row 3: one fast-path count of the leaf
    bump(&mut h.counts[0])  # row 4: the pin and a fast-path count of the buffer
```

## 22. Escape Hatches

The owner's advice: "if users hate, just use *T or read, copy, write
back". Both are ordinary programs.

**Take a counted `*T`.** An object of its own may be stored, returned
and captured, at one fast-path count per copy.

```text
struct Counter:
    n: i64

struct Watch:
    counters: List[*mut Counter]  # hypothetical syntax

fn remember(w: &mut Watch, c: *mut Counter) -> void:  # hypothetical syntax
    w.counters.push(c)

fn demo() -> i64:
    let mut w = Watch { counters: [] }
    let c = &mut Counter { n: 0 }
    remember(&mut w, c)
    c.n += 1
    w.counters[0].n
```

**Copy the value out, change it, write it back.** This takes no
reference at all. A struct copy is a few word moves, and a list's
copy-on-write keeps it cheap.

```text
struct Player:
    name: string
    score: i32

fn reward(players: List[Player], best: usize) -> List[Player]:
    let mut out = players
    let mut p = out[best]  # a copy of the element
    p.score += 10
    out[best] = p  # written back
    out
```

What the first cut gives up, and what to write instead:

| Gone | Instead |
| --- | --- |
| a `let`-bound alias of a field or an element, `let r = &mut o.f` | name the place at each use, or pass `&mut o.f` to a helper |
| a returned local reference, as in serde_json's `pointer_mut` or Rust's `split_at_mut` | a `*T` node, indices, or a `&fn` callback |
| an in-place edit inside an enum payload, such as std's `Json` | copy out, change, write back: a payload is no place ([`ref.match.copy`](../../spec/lang/13-values-and-references.md#matching-through-a-reference)) |
| `List[&T]`, `Option[&T]`, `&T?` | `List[*T]`, `*T?`, or two functions |
| a lazy adapter's closure that reads `self` | copy the field first, `k := self.k` |
| a `*self` method called on a field | make the field `*mut T` |

Tier 2's lifetime parameters, or Hylo-style projections, would bring
back local results and bound aliases later.


## 23. Prior Art

| Language | Second-class reference | Stored reference | How a borrow stays valid | What hd takes |
| --- | --- | --- | --- | --- |
| Swift | `inout`, `borrowing` parameters, `mutating` methods | class references | the call; exclusivity, static for locals and dynamic for class properties (SE-0176) | the argument-position rule and a static overlap check, with no dynamic one |
| Rust | none: all references carry lifetimes | `&T` with a lifetime, `Rc<T>` | NLL liveness (RFC 2094), two-phase borrows (RFC 2025) | nothing; its checker is the cost tier 2 avoids |
| Hylo | `let`, `inout`, `sink` parameters; subscript projections | none: no shared mutable references | the call, or the projecting expression | local receivers; a returned `self` used within one expression |
| Mojo | `read`, `mut`, `ref` arguments | `ref` with origins | origins tracked at compile time | nothing beyond Swift's model |
| C# | `ref` parameters and `ref` locals | class references | ref-safe contexts, a static scope comparison | nothing: hd has no bound local references |
| Vale | region borrows in `pure` functions | generational references | a frozen region, or a generation check per dereference | nothing |
| Go | none | `*T`, interior pointers included | garbage collection; escape analysis | the `*T` spelling and pointer receivers |
| C++ | references | pointers | nothing: `push_back` may invalidate | the counterexample element pins prevent |

## Sources

- Swift: [SE-0176](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0176-enforce-exclusive-access-to-memory.md) (exclusivity),
  [Swift 5 exclusivity](https://www.swift.org/blog/swift-5-exclusivity/),
  [SE-0390](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0390-noncopyable-structs-and-enums.md) and
  [SE-0427](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0427-noncopyable-generics.md) (`~Copyable`),
  [SE-0446](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0446-non-escapable.md) (`~Escapable`),
  [SE-0447](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0447-span-access-shared-contiguous-storage.md) (`Span`),
  [`@_lifetime`](https://github.com/swiftlang/swift/blob/main/docs/ReferenceGuides/LifetimeAnnotation.md),
  [SE-0377](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0377-parameter-ownership-modifiers.md) (`consuming`).
- Rust: [RFC 2094](https://github.com/rust-lang/rfcs/blob/master/text/2094-nll.md) (NLL),
  [closures](https://doc.rust-lang.org/book/ch13-01-closures.html),
  [`RefCell`](https://doc.rust-lang.org/std/cell/struct.RefCell.html),
  [`self_cell`](https://crates.io/crates/self_cell), [`ouroboros`](https://crates.io/crates/ouroboros).
- Scala 3: [capture checking](https://docs.scala-lang.org/scala3/reference/experimental/cc.html).
  C#: [`ref struct`](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/builtin-types/ref-struct),
  [`scoped`](https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/statements/declarations#scoped-ref).
  OxCaml: [stack allocation](https://oxcaml.org/documentation/stack-allocation/intro/).
  D: [scope parameters](https://dlang.org/spec/function.html#scope-parameters).
- Hylo: [bindings](https://docs.hylo-lang.org/language-tour/bindings),
  [mutable value semantics](https://www.jot.fm/issues/issue_2022_02/article2.pdf) (JOT 2022).
  Mojo: [origins](https://docs.modular.com/mojo/manual/values/lifetimes/),
  [ownership](https://docs.modular.com/mojo/manual/values/ownership/).
  Kotlin: [inline functions](https://kotlinlang.org/docs/inline-functions.html),
  [`callsInPlace`](https://kotlinlang.org/api/core/kotlin-stdlib/kotlin.contracts/-contract-builder/calls-in-place.html).
- hd: the tier-1 design, `58c3d470f:docs/future-work/OBJECT_MODEL_TIER1.md`, [REFCOUNT_RUNTIME.md](REFCOUNT_RUNTIME.md),
  the Rust model study (`git show b3f5eed1a:docs/future-work/RUST_MODEL_STUDY.md`), [PARALLELISM_AND_LOCALITY.md](PARALLELISM_AND_LOCALITY.md),
  [07-functions.md](../../spec/lang/07-functions.md); the removed chapter 13 at
  `5d0b21e0a^` (`^`), `101344d4c^` (lifetime sets) and `a04c3497c^` (owned values).
