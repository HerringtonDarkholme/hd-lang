# Object Model, Tier 2: Design Study

Status: proposal (#426). The spec and the compiler are unchanged.

Tier 1 ([OBJECT_MODEL_TIER1.md](OBJECT_MODEL_TIER1.md)) gives hd Go's
object model on counted linear memory: structs are values, sharing comes
from global references `&T` and `&mut T`, and every reference is
counted, may escape and may alias. It is complete and safe, and an agent
writes it with no marks. Its costs are counts, cells and pins, listed in
the left column of the table below.

Tier 2 asks whether Rust's lifetimes can be laid onto tier 1, opt-in,
and what they buy. The answer here is yes, with one idea. A reference
type may carry the set of things it depends on, `&{a, self.x} T`, and
**global is one more lifetime**, the one that outlives all others. Every
declaration written with sets is checked once and compiled in two modes:
with every source local, uncounted and statically checked; or with some
source global, counted exactly as tier 1. So one library serves both
tiers, tier 1 code calls it with no marks, and no new run-time check is
needed.

| Tier 1 cost | With tier 2 marks |
| --- | --- |
| an increment and a decrement per view or stored reference | none on references whose sources are local |
| `&mut local` passed to a call that is not inlined: a heap cell | the local stays in the frame when the parameter is `&{} mut T` |
| a count update through each stored interior reference | none when its sources are local |
| element references pin the list; push tests the pin | the list is frozen statically; no pin is taken |
| closure environments in counted objects | a scoped closure's environment is a record in the frame |
| writable references may alias | a scoped `&{} mut` with local sources is exclusive: no-alias holds in that mode |
| release happens at count zero | a NonCopyable value is released at the end of its scope |

One rule binds both tiers: **nothing a caller relies on is inferred
from a body**. Lifetime sets, escaping, closure kinds, `global` bounds
and copyability are written in headers. The compiler checks each body
against its header and infers only inside it: the sets of locals, the
extent of each borrow, and placement.

**Dependencies.** The owner has settled tier 1's five questions, and
tier 2 builds on the answers: global `&mut` gets no exclusivity check
(sections 6 and 6.4); `List`, `Map` and `string` are copy-on-write
values whose nested write paths, such as `m[k].push(x)`, write in place
(the places and freezes of section 6.1); an element reference pins its
list, which tier 2 turns into a compile-time freeze for scoped
references, while the counted mode keeps the pin; `==` between
references is an error (no dependency); and the new semantics run on
Wasm GC first (the plan of section 16). Unit costs come from
[REFCOUNT_RUNTIME.md](REFCOUNT_RUNTIME.md), measured on V8 only: an
increment and decrement pair costs 2.1 ns, borrowed parameters at +0
and moves at last use bring counted programs within 4% of uncounted
ones, and the page-table lookup behind thin interior pointers measured
no slower than a base pointer. Section 12 scores tier 2 against those
numbers.

Part 1 designs tier 2 from scratch, in an indentation syntax close to
hd's; every spelling is a placeholder and no code block is checked by a
compiler. Part 2 maps it onto hd and plans the work.

# Part 1: From Scratch

## 1. What Tier 2 Must Serve

- **Parsers over borrowed input**: no count per token, no source kept
  alive by one token.
- **In-place algorithms and iterators** over a local buffer, which stays
  in the frame, with no allocation.
- **Callbacks** that borrow locals for the length of a call.
- **Values with a deterministic end**: a unique buffer, a guard.
- **One std for both tiers**: a tier 1 caller pays tier 1's costs, a
  tier 2 caller pays nothing, and a tier 1 program means the same and
  costs no more after std gains tier 2 headers.

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
    let lexer = Lexer { src: &text, pos: 0 }     # Lexer[{text}]: scoped, uncounted
    let n = 0
    while lexer.next_word().len() > 0:
        n += 1
    n
```

1. `&{a, b} T` and `&{a} mut T` are **scoped** references: they depend
   on `a` and `b`, hold no count, and cannot outlive them. `&T` with no
   set is **global**, as in tier 1.
2. A set names parameters, `self`, paths such as `self.items`, lifetime
   parameters, locals inside a body, and `global`. In a parameter, `{}`
   is that parameter's own lifetime, which no other position names
   except by naming the parameter.
3. A global reference converts to any scoped one. A scoped value never
   becomes global except by copying.
4. A type with a lifetime parameter, `Lexer[{s}]`, is scoped when its
   argument is. A value containing a scoped reference is itself scoped.
5. A scoped `&{} mut T` is exclusive; a scoped `&{} T` freezes the place
   it borrows. A static checker enforces both inside each body.
6. Closure types carry capture sets, `fn(A) ->{xs} B`, and a kind:
   `fn`, `fn mut` or `fn once`. A closure type with no set is global.
7. A struct marked `~Copy` is NonCopyable: it moves, and is released at
   the end of its scope.
8. Each declaration with sets compiles in two modes, local and counted;
   the caller picks one from its arguments.

## 3. Lifetime Sets

### 3.1 What a set may name

| Entry | Example | Means |
| --- | --- | --- |
| a value parameter | `fn first(xs: List[T]) -> Option[&{xs} T]` | borrows the caller's argument: the caller's place stays alive and unchanged |
| a reference parameter | `fn pick(a: &{} Node) -> &{a} string` | lives as long as `a`'s argument does: the result inherits `a`'s sources |
| `self` | `fn get(self, i: usize) -> Option[&{self} T]` | as a parameter |
| a path | `fn name_of(u: User) -> &{u.name} string` | borrows only that field; other fields stay writable |
| a lifetime parameter | `fn first[{l}](ts: List[Token[{l}]]) -> Token[{l}]` | a lifetime that no variable names, such as one inside a type argument |
| a local | `let t: &{src} string = lexer.next_word()` | inside a body only |
| `global` | `Token[global]`, `T < global` | outlives everything; `&{global} T` is `&T` |
| nothing: `{}` | `xs: &{} List[T]`, `let r: &{} Point = &ps[0]` | in a parameter, the argument's own lifetime; in a local's type, inferred from the initializer; elsewhere an error |

A path in a `pub` header names only `pub` fields, so a signature never
shows a private field. A field path borrows the field's place; when the
field holds a reference, `{u.owner}` follows Scala's monotonicity rule:
`{u}` covers `{u.owner}`, never the reverse.

Naming a parameter has two readings, one per kind of argument, as in
Swift's lifetime dependencies, which distinguish `borrow a` from `copy
a`. A value argument (a struct, a string, a list) is borrowed in place:
the caller passes its address and keeps the place frozen while the
result lives. A reference argument already depends on something; the
result depends on the same things, and the argument variable itself may
end.

### 3.2 Results name their sources

A result that borrows writes its set; there is no elision. A
reference in a result with no set is global, and `{}` in a result is an
error, because nothing would bound it. Sets join by union and order by
inclusion:

- `&{a} T` converts to `&{a, b} T`: depending on fewer things is valid
  for longer. `&T` converts to every `&{S} T`.
- A body may return a value whose set is covered by the header's.
  `if c: a.name else: b.name` has the set `{a, b}`.
- Behind a writable reference the type is exact. `&{l} mut &{a} T` does
  not convert to `&{l} mut &{a, b} T`, or a shorter view could be
  stored where a longer one is expected. This is Rust's invariance of
  `&mut T` in `T`.

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
   tier 1, so `r := &p` keeps tier 1's meaning. Locality shows in the
   source: in a header, in a written type, or in the callee being
   called.
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
fn longest(a: string, b: string) -> &{a} string:
    if a.len() >= b.len(): a.view() else: b.view()
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

1. cannot be stored in a global place: a field of an object behind a
   global reference, a top-level binding, a provider, a task, a value
   that crosses a boundary;
2. cannot be returned beyond its sets, nor captured by a global closure;
3. is never counted: copying a `Token[{src}]` copies its bytes and
   touches no header, because the view inside it holds no count.

A scoped list, `List[Token[{src}]]`, still has a counted buffer, since a
list's buffer lives in the heap; its elements hold no counts, and the
list cannot outlive `src`.

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
call `advance` again while it keeps the token. `peek` returns a
reference into the parser, which freezes the parser while it lives. The
header says which.

### 4.4 Tier 1 code sees the global instance

`Parser[global]` is an ordinary tier 1 type: its views are counted, its
values may be stored anywhere, and its methods run in the counted mode
(section 6.4). So the fix for storing a parsed tree in a cache is to
parse with `Parser[global]`, from the same source:

```text
fn remember(cache: &mut Cache, src: string) -> Result[void, ParseError]:
    let p = Parser.new(&src)              # Parser[{src}]
    cache.last = .Some(p.expr()?)
    # error: `Expr[{src}]` borrows `src`, and `cache.last` holds `Expr[global]`
    #   help: parse with `Parser[global]`, whose views are counted
```

### 4.5 A field that borrows another field of its struct

Rust needs helper crates such as `self_cell` or `ouroboros` for a struct
holding views into its own buffer. Sets can name the field, as in
`rows: List[&{self.text} string]` (section 11.6). It is sound because
the dependency is on the buffer, not on the struct's bytes: copying the
struct copies `text`, one increment on the shared buffer, and the views
still point into a live buffer. Three rules:

1. The named field must keep its contents in a counted buffer or
   object: a string, a list, a map or a reference. A path to an inline
   struct field is an error, since the struct's own bytes move.
2. After construction the named field is frozen: assigning it, taking
   `&mut` to it, or calling a writing method on it is an error. The
   whole struct may be replaced.
3. In a literal, a field typed `{self.f}` accepts views of the variable
   whose value is given to `f`, since `f` then holds the same buffer.

Such a struct depends on nothing outside itself, so it is global. It
saves one count per view, but cannot express an arena that grows while
views into it live.

## 5. NonCopyable And NonEscapable Structs

### 5.1 NonCopyable

```text
struct Buffer ~Copy:
    bytes: List[u8]

impl Buffer:
    fn write(&{} mut self, b: string) -> void: self.bytes.append(b.bytes())
    fn finish(consume self) -> string: string.from_utf8_unique(self.bytes)
    fn drop(&{} mut self) -> void: pass     # optional; runs at the end of the owner's scope

fn greet() -> string:
    let b = Buffer { bytes: [] }
    b.write("hi")                           # borrows `b`
    s := b.finish()                         # moves `b`
    b.write("!")                            # error: `b` was moved by `finish` on the line above
    s
```

1. `~Copy` on a struct suppresses copying, as Swift's `~Copyable` does.
   A struct with a NonCopyable field, an enum with a NonCopyable
   payload, and `Option[Buffer]` are NonCopyable and must say so with
   `~Copy`, so the header shows it.
2. A use that is not a borrow moves the value: binding it to another
   name, storing it, returning it, passing it to a `consume` parameter,
   capturing it in a `fn once` closure. A plain value parameter of a
   NonCopyable type borrows, so a call reads the same as for any value.
   Moves are not marked at the use, as in Rust and Swift.
3. A use after a move is an error that names the move. A move on one
   branch only makes the variable moved after the join. Moving a field
   out of a struct is an error; `replace` and `take` swap in a value.
4. At the end of its owner's scope, or when overwritten, the value is
   released: its `drop` runs if declared, then its fields are released,
   newest binding first. `drop` cannot suspend; a value that must close
   with a suspending call is `Disposable` and bound with `using`.
5. A global reference may point at one: `&mut Buffer { ... }` is a
   counted object that owns the buffer, released at count zero. `*r`
   and a `consume` call through a reference are errors, since they would
   copy or move out of a shared object.

It buys no count on the value, no collector entry, release at a known
point, and `consume` methods such as `finish` that reuse a buffer
nothing else can hold. It costs moves and their errors, `consume`,
`~Copy` in generics, and no partial moves.

### 5.2 NonEscapable is a lifetime parameter

Swift needs `~Escapable` because its references carry no lifetime in
their type; a lifetime dependency is attached to a function instead
(`@_lifetime(borrow a)`). Here a type that cannot outlive what it
borrows is a type with a lifetime parameter, such as a span or an
iterator:

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
`extension Wrapper: Copyable where T: Copyable`. Rust moves every type
and makes `Copy` opt-in; Mojo uses positive `Copyable` and `Movable`
traits and marks transfers with `x^`. Swift's default suits a language
whose tier 1 copies values freely.

`T < global` is needed only where `T` lands in a place whose type does
not mention it: a top-level binding, a provider, a task, a `dyn` value,
a channel to another instance. Scala 3 calls the other case capture
tunnelling: a capture set inside a type argument travels with the type.

### 5.4 Closures and enums

A closure that captures a NonCopyable value by move is NonCopyable: its
type is `fn mut` or `fn once` (section 7.3). An enum with a NonCopyable
payload is NonCopyable; a `match` on it by value consumes it. Payloads
stay non-places, as in tier 1, so a payload cannot be borrowed in place;
`Option.take` moves it out and the program stores it back.

## 6. Exclusivity

### 6.1 The rules

While a scoped borrow is live, its source place is restricted:

| Live borrow of place `P` | Reading `P` | Writing `P`, or a call that changes a list's length | Another `&{} mut` of `P` | Another `&{} P` |
| --- | --- | --- | --- | --- |
| `&{} P` (shared) | yes | no: `P` is frozen | no | yes |
| `&{} mut P` (exclusive) | only through the borrow | only through the borrow | no | no |

A place is a local, a field path from a local, the elements of a list
taken together, a map's entries taken together, or the target of a
scoped reference. Nested write paths such as `m[k].push(x)` and
`xs[i].f = v` are writes to these places, done in place; a live borrow
of a map entry freezes the map, since an insert may rehash. Places
conflict when one is a prefix of the other: `p.pos` and `p` conflict,
`p.pos` and `p.vel` do not. A scoped writable reference cannot be copied; passing
it to a parameter reborrows it for the call.

Freezing replaces the pin. `r := xs.get(0)` with `get`'s header
`-> Option[&{self} T]` freezes `xs` while `r` lives, so `xs.push(v)`
there is a compile error, not a `collection-pinned` panic, and no pin
count is taken. Copy-on-write needs no extra rule: an exclusive borrow
of an element makes the buffer unique when it starts, and nothing else
can copy the list while it lives.

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
across functions, or `unsafe`. NLL's problem case #3, returning a
borrow on one branch and writing the same place on another, is rejected
here too.

### 6.3 Errors, and how often programs meet them

```text
fn bump_all(xs: &{} mut List[i32]) -> void:
    first := xs.get(0)          # `first` borrows `xs` (shared)
    xs.push(1)                  # error: `xs` is frozen while `first` is used
    print(first)                #   note: `first` is used here
    #   help: copy the value out, `first := xs[0]`, or push after the last use
```

Messages name the place, the conflicting loan, where it was created and
the use that keeps it live, as rustc does. Every scoped error has a fix
Rust lacks: drop the set, and the reference becomes global, counted and
pinned, at tier 1's cost. Programs meet these errors in proportion to
the marks they write: unmarked application code meets none. The tour
hits two kinds, a view kept across a push and two closures writing one
variable.

### 6.4 A borrow through a global reference

A tier 1 caller passes `obj.items`, reached through a global reference,
to `get(self) -> Option[&{self} T]`. No static check can see the other
global references to `obj`, so an alias may write while the borrow
lives. Three ways to keep the borrow sound, priced:

| | A borrow flag in the object header | A count for the borrow's duration | The counted mode |
| --- | --- | --- | --- |
| Mechanism | Swift's dynamic enforcement, Rust's `RefCell`: set a reader count or writer bit when the borrow starts; conflicting accesses test it | increment the borrowed object at the start, decrement at the end | the callee is compiled a second time with every reference derived from a global source counted and pinned, as tier 1 |
| Memory | a word or spare count bits per object | none | none |
| Per borrow | header lookup, set and clear | header lookup, increment, decrement | tier 1's counts, inside the callee |
| Per access elsewhere | every tier 1 write that could conflict tests the flag: a load, a branch and a header lookup | none | none |
| Sound? | yes | no: the callee may borrow a string field of `obj`; an alias overwrites the field and frees the old buffer | yes, by tier 1's argument |
| New failure | a `borrow-conflict` panic | none | none: aliases meet tier 1's pin panic |
| Code size | none | none | up to two copies of a function that has both kinds of callers |

Swift checks class properties dynamically, and warns that the checks
"could affect the performance of the Release binary" in tight loops. A
count for the borrow protects only the outermost object, not what the
callee derives from it. The counted mode adds nothing at run time: a
body is checked once against its header and emitted per mode, and the
caller picks the mode statically:

- **Local mode** when every source the call borrows is local: a local
  variable that no global reference refers to, a value parameter of a
  function in local mode, or a scoped reference in local mode. Nothing
  is counted, borrows freeze statically, and exclusive references hold
  the no-alias guarantee.
- **Counted mode** otherwise. A set whose sources are all reached
  through global references is global in the caller, so tier 1 code
  may store what `get` returns, and it is a counted, pinning reference
  as in tier 1.

**Global `&mut T` gets no check**, as the owner settled for tier 1: a
counted reference cannot dangle.

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

fn send_later(buf: consume Buffer, sink: &{} mut Sink) -> void:
    on_done(fn once() ->{sink} void: sink.send(buf.finish()))   # moves `buf` in, then out
```

### 7.4 What is left out

Non-local `return` from scoped trailing blocks, as Kotlin's inline
lambdas allow, is not proposed. Serializable closures (#166, on hold)
would be global; a scoped closure never crosses a boundary.

## 8. Library Code For Both Tiers

### 8.1 Unannotated parameters stay global

What does a parameter `r: &T` with no set mean? Two choices:

- **Global, as the tier 1 rule says.** std writes `{}` or a set on every
  borrowing parameter, about one mark per writing method; application
  code writes nothing and may store its reference parameters.
- **Borrowed for the call**, as Swift does for closure parameters, with
  a mark for escaping. A tier 1 function that stores a parameter, such
  as `add_child(parent: &mut Node)`, would then need the mark.

This study takes the first: it keeps "unannotated is global" and "what
is local shows in the source". Section 9's report suggests std's marks.

### 8.2 Generic parameters may be scoped

`T` may be instantiated with a scoped type by default (section 5.3).
`List[&{l} Point]` is a scoped list of uncounted references; `List.push`
needs no `global` bound, because the list's type carries `{l}`. Only a
generic that puts `T` where its type does not appear writes
`T < global`.

### 8.3 Monomorphization

Lifetimes are erased after checking, except for the mode. Each generic
instance and each function with sets is emitted at most twice, local
and counted; one mode covers all of a function's sources, so copies do
not multiply with parameters. A function used from one mode, or whose
type arguments hold no reference, is emitted once.

### 8.4 std, written lifetime-aware

```text
impl[T < ~Copy] List[T]:
    fn get(self, i: usize) -> Option[&{self} T]
    fn push(&{} mut self, x: consume T) -> void
    fn iter(self) -> ListIter[{self}, T]
    fn iter_mut(&{} mut self) -> ListIterMut[{self}, T]
    fn sort_by(&{} mut self, compare: fn(&{} T, &{} T) ->{} Ordering) -> void
    fn span(self, from: usize, to: usize) -> Span[{self}, T]

impl[K < Eq & Hash, V] Map[K, V]:
    fn get(self, key: K) -> Option[&{self} V]

impl string:
    fn split(self, sep: string) -> List[&{self} string]
    fn slice(self, from: usize, to: usize) -> &{self} string

struct Builder ~Copy:
    buf: List[u8]

impl Builder:
    fn add(&{} mut self, s: string) -> &{self} mut Builder        # chains: b.add("a").add("b")
    fn finish(consume self) -> string                             # reuses the buffer: no copy
```

Read-only parameters stay values, since tier 1 already passes them
borrowed at +0; sets appear on results, writable receivers and closure
parameters. A tier 1 caller writes `names.sort_by(fn(a, b): a.cmp(b))`
and `line.split(",")` with no marks. A local list calls the local instances;
a list behind a global reference calls the counted ones, so `split`
there returns counted views, exactly tier 1's result. A string is
immutable, so a shared reference to one refers to its bytes, which lets
`slice` and `split` return references; `&mut string` refers to the
variable.

## 9. Escape Analysis And Marks

| | Without marks | With marks |
| --- | --- | --- |
| Address-taken local or new object on the stack | when every reference provably dies in this body, inlined callees included | also when it goes only to `{}` parameters of any callee, in any module |
| Count elision | moves at last use, +0 parameters, borrowed reads, from REFCOUNT_RUNTIME.md | every reference with local sources is uncounted |
| Closure environment | in the frame when the closure stays in this body | in the frame whenever the closure goes to a scoped parameter |
| Facts across modules | none: a reference passed to an opaque call escapes | the callee's header: `{}`, result sets, closure sets and kinds |

A body edit that starts storing a `{}` parameter is an error in that
body, never a change in a caller. `hd build --explain-escapes`, proposed
by tier 1, gains suggestions that read bodies but never change headers:

```text
shapes.hd:22: `Rect.translate` never keeps `self`; 14 call sites move a local to a heap cell
  help: write `&{} mut self` (fix-it available)
```

## 10. Suspension, Providers And Boundaries

- A suspending body that holds a borrow of its own local across a bang
  call keeps that local in its frame, which it then allocates on entry,
  as RUST_MODEL_STUDY.md section 2.2 found.
- A cold suspension built from a call with scoped arguments is scoped:
  `Suspend[{conn}, Row]` cannot outlive `conn`. Children of `all!` may
  borrow the parent's locals; two children may not both hold
  `&{} mut` of one place.
- Providers, top-level bindings and values that cross a boundary are
  global. A scoped value there is an error.

## 11. Tour

### 11.1 A tokenizer whose tokens borrow the source

```text
fn tokens(src: string) -> List[Token[{src}]]:
    let lexer = Lexer { src: &src, pos: 0 }      # section 2's Lexer, with `next(&{} mut self) -> Token[{s}]`
    let out: List[Token[{src}]] = []
    loop:
        match lexer.next():
            .End => return out
            t => out.push(t)
```

Tier 1's version (its section 11.4) made two count updates per token,
and one kept token kept the whole source alive. Here no token is
counted, none can outlive `src`, and the list's buffer is the only
counted object. A caller that keeps tokens longer builds
`Lexer[global]` and gets tier 1's views.

```text
fn first_token(path: string) -> Token[{path}]:
    text := read_file(path)
    tokens(text)[0]       # error: the token borrows `text`, a local that ends here
```

### 11.2 A parser over borrowed input

```text
enum Expr[{s}]:
    Name(text: &{s} string)
    Num(value: f64)
    Add(left: Expr[{s}], right: Expr[{s}])    # recursive: boxed, as in tier 1

impl[{s}] Parser[{s}]:
    fn expr(&{} mut self) -> Result[Expr[{s}], ParseError]:
        let left = self.term()?
        while self.eat('+'):                  # advances past a `+` token, if one is next
            left = .Add(left, self.term()?)
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
    let scores = load_scores(path)
    sort_scores(&mut scores)        # `scores` stays in this frame: no cell, no count
    render(scores)
```

In tier 1, `sort_scores` taking `&mut List[Score]` would move `scores`
to a heap cell, and the comparator would be a counted closure. Here the
borrow is local, the comparator has no captures, and `sort_by`'s local
instance may assume `scores` has no alias. Holding `top := scores.get(0)`
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

The loop takes no count and no pin. The wrong program is the case that
panics with `collection-pinned` in tier 1; here it does not compile, and
the fix collects reorders first and pushes after the loop.

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
no environment object, and `each_line` cannot store `f`. Were `f` a
shared `fn`, the literal would be rejected for writing `best`, with a
fix-it to `fn mut`.

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

Putting Go's model on counting instead of a tracing collector costs
three things. REFCOUNT_RUNTIME.md measured each on V8; the shares below
are of total run time on its programs, start-up of 27 ms included.

| Cost | What it costs, measured | Scoped borrows | NonCopyable | NonEscapable | Left for unmarked tier 1 code |
| --- | --- | --- | --- | --- | --- |
| 1. Interior pointers find the object's header | the page-table lookup measured free: 3.43 ns per copy and read against 3.44 for a base pointer | the count update through the pointer goes, so the lookup goes with it | nothing: a unique value has no count | views inside a span or iterator are uncounted | the lookup, which costs nothing measurable |
| 2. Count traffic on copies | 2.1 ns per increment and decrement pair. With tier 1's elisions, counted runs were within 4% of uncounted ones (binary-trees 2,059 against 2,221 ms; points 144 against 138). Without them, 18% to 19%. Counted word views against uncounted ones in map-count: 161 against 121 ms | uncounted views and references with local sources: map-count's 25%, and the counts that tier 1's elisions miss across calls, bounded by the 18% | no count on the value itself; a builder's `finish` reuses its buffer | iterators and spans touch no header | counts on what is stored or returned; borrowed parameters and last-use moves already remove most |
| 3. Cycles | trial deletion only for cycle-capable types: 206 ms on the parent-pointer program against 52 with weak references, 75% of it; 4.6 KB to 6.0 KB of collector code. None of the 54 types in `examples/`, the playground and the tour is cycle-capable | nothing for graphs: a scoped reference cannot be a back edge in a structure that outlives its frame | nothing: a NonCopyable value inside a counted object can still be on a cycle | nothing | all of it, in programs that build graphs with back edges |

**Does std carry the gain into unmarked code?** For costs 1 and 2,
partly. Tier 1 code that calls `line.split(",")` on a local string,
`xs.iter()` on a local list or `m.get(k)` on a local map gets std's
local instances with no marks of its own, so views it only reads are
uncounted. What it stores keeps tier 1's counts: map-count with a
`Map[string, i32]` stores a counted view per new key, and reaching the
borrowed-keys time takes one mark in the application,
`Map[&{text} string, i32]`. Data reached through global references
takes the counted instances. For cost 3, no.

**Cycles are where tier 2 does not help.** Rust has the same gap: a
tree with parent links needs `Weak`, an arena, or indices, and borrows
cannot express it. Tier 2 helps only at the edges. A capture set lets a
closure field that captures only values stop making its struct an open,
cycle-capable slot, so a struct of callbacks over plain data needs no
collector entry. The type check that finds cycle-capable types stays; it
runs at compile time from type declarations and costs nothing at run
time. What would cut cost 3 is `Weak[T]` (tier 1's open point), which
the measurements price at 52 against 206 ms, or arenas in std.

So tier 2 removes up to a quarter of run time in view-heavy text code,
a few percent in allocation-heavy code where tier 1's elisions already
work, nothing measurable from interior pointers, and nothing from
cycles. Its larger gains are the ones these programs do not measure:
heap cells for `&mut` locals passed across calls, closure environments,
and pins.

## 13. Costs

**Run time.** Local mode removes the costs listed at the start, priced
in section 12. Counted mode costs what tier 1 costs. NonCopyable values
add a release at each scope exit and a drop flag where a move happens on
some paths. Nothing new runs where no mark is written.

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
| Code size | up to two instances of functions used in both modes |

RUST_MODEL_STUDY.md estimated a full Rust checker at 10k to 15k lines
in `hd_check`; without moves of ordinary values, cross-function region
inference and full drop elaboration, an estimate here is 5k to 8k.

**Surface.** Sets, `{}`, `global`, lifetime parameters, closure sets,
`fn mut`, `fn once`, `~Copy`, `consume`, two bounds, self-relative
paths and one checker's errors, all opt-in: a program with no set and no
`~Copy` is a tier 1 program.

# Part 2: Retrofit

## 14. What Survives, Changes And Is New

Tier 2 adds to tier 1's retrofit (OBJECT_MODEL_TIER1.md section 14) and
removes nothing that tier 1 keeps.

| hd today, or tier 1 | Under tier 2 |
| --- | --- |
| tier 1 `&T`, `&mut T` | unchanged: global |
| a header that cannot promise not to keep a reference | `&{} T`, `&{} mut T`, result sets |
| element references pin | a scoped reference freezes statically; global ones still pin |
| one `fn` type; `mut` before `fn` is a syntax error | `fn`, `fn mut`, `fn once`, with capture sets; the plain `fn` keeps its meaning |
| "no mutable closure type" (Mutable Closures) | still true for global closures; scoped closures get kinds |
| tier 1 `string` views counted | `&{s} string`: uncounted views |
| `Disposable` and `using` for resources | unchanged; `~Copy` adds deterministic release and an optional synchronous `drop` |
| Wasm GC backend | tier 2 checks run there; scoped references are emitted as tier 1 references until the RC backend lands |

**The removed `^` mode** (chapter 13, deleted in R4) meant "stays in
this call", with no lifetimes, no `^` results, fields or type arguments,
and no mark at the call site. `{}` on a parameter is the old `^`, and a
borrow is still never marked at the call. Sets add what `^` could not
do: return a borrow, store one in a struct, iterate without a closure.
`^` was parked because Wasm GC could not exploit it; counted linear
memory can.

**Earlier lifetime sets** (`^{a}`, removed in `101344d4c`) named
lifetimes by variables, `pub` field paths, lifetime parameters
`data Parser[^doc]`, requirement keys and `static`, and elided a bare
`^` in results. Tier 2 keeps variables, `pub` paths and lifetime
parameters; it drops elision and requirement keys, renames `static` to
`global`, and writes parameter lists `[{s}]` as uses are written.

**The earlier `own` mode and drops** (removed in `a04c3497c` and
`8a77d0273`) return smaller. `own T` marked a position; `~Copy` belongs
to the type, as in Swift, and replaces `OwnOnly`. `DropOwn.drop` becomes
an optional synchronous `drop`. Moves, use-after-move and drop flags
return; freshness rules and partial moves do not.

## 15. Migration

Tier 2 is opt-in, so no program changes. std gains tier 2 headers by
hand, chunk by chunk; a header that gains `{}` or a result set breaks no
caller, because global arguments still convert. It breaks only a std
body that stored what it now promises not to keep, which the checker
reports in that body. Applications adopt marks where `--explain-escapes`
shows a cost.

## 16. Implementation Plan

Each job is about an hour, ships its spec rules and conformance
fixtures, and keeps every check green. Jobs 1 to 17 are the language and
run on the current backend after tier 1's Wasm GC bridge, as settled:
scoped references are emitted as counted ones, so run fixtures pass
while gaining no speed. Jobs 18 to 20 migrate std. Jobs 21 to 24 follow
the RC backend of REFCOUNT_RUNTIME.md. Tier 2 starts after tier 1's
jobs 1 to 10.

| # | Job | Spec and fixtures |
| --- | --- | --- |
| 1 | Grammar: sets on `&`, `{}`, `global`, lifetime parameters `[{s}]` and arguments, closure sets `->{...}`, `fn mut`, `fn once`, `~Copy`, `consume`; parser | a "Lifetimes" chapter skeleton and grammar productions; parse fixtures, valid and invalid |
| 2 | Header checks: names in sets (parameters, `self`, `pub` paths, lifetime parameters, `global`), `{}` placement, missing argument means global | header rules; a fixture per error |
| 3 | Set algebra: global to scoped, inclusion, joins, exactness behind `mut`, expected-type rule for `&e` | conversion rules; check fixtures |
| 4 | Escape checks in bodies: scoped values not stored globally, results covered by the header, scoped values not captured by global closures | escape rules; a fixture per error, with fix-its |
| 5 | Lifetime parameters on structs and enums: scoped-contains rule, variance, `Token[global]` | type rules; check fixtures |
| 6 | Loans 1: shared and exclusive loans on local places, liveness, freezes, length-changing calls | exclusivity rules; a fixture per conflict |
| 7 | Loans 2: two-phase borrows, reborrows through scoped references, field disjointness, elements as one place | rules and fixtures |
| 8 | Loans at calls: loans from header sets, value parameters borrowed by results passed by address, temporaries | call rules; check and run fixtures |
| 9 | Modes: local or counted at each call and instance, global collapse of sets reached through global references | mode rules; run fixtures showing tier 1 behavior in counted mode |
| 10 | Closures 1: capture sets, scoped literals checked against expected types | capture rules; fixtures |
| 11 | Closures 2: kinds, copyability, conversions, exclusive captures, two live `fn mut` | kind rules; a fixture per error |
| 12 | Generics: `T < global`, scoped type arguments, places that need `global` (providers, tasks, `dyn`, boundaries) | bound rules; fixtures |
| 13 | NonCopyable 1: `~Copy` on declarations, conditional copyability, `T < ~Copy` | rules; fixtures |
| 14 | NonCopyable 2: moves, `consume`, use after move, no partial moves, `fn once` captures, moves out of references | move rules; a fixture per error |
| 15 | NonCopyable 3: release at scope exits, drop order, synchronous `drop`, `Disposable` interplay | release rules; run fixtures for order |
| 16 | Self-relative paths: `{self.f}`, frozen fields, literal rule | rules; fixtures |
| 17 | Suspension: borrows across bang calls, scoped cold suspensions, borrows in `all!` children | rules; fixtures |
| 18 | std 1: `List`, `Map`, iterators, `Span`, `sort_by` with tier 2 headers | std rules; fixtures |
| 19 | std 2: text (`slice`, `split`, `Builder`), formatting, encoding | std rules; fixtures |
| 20 | std 3: the rest of std's writing methods; `--explain-escapes` suggestions | std rules; report-format fixtures |
| 21 | RC backend: local instances with uncounted references; counted instances; emitting each only when used | none (unobservable); count totals on the tour programs |
| 22 | RC backend: locals behind `{}` borrows in the shadow-stack frame; scoped closure environments in the frame | allocation counts on the tour programs |
| 23 | RC backend: NonCopyable release glue and drop flags | run fixtures from job 15 on the new backend |
| 24 | No-alias in local instances: the backend may keep values in registers across writes through other references | benchmark: sort and points from the Rust study |

## 17. Questions For The Owner

1. **What does a parameter reference with no set mean?** Global, as the
   tier 1 rule says, so std writes `{}` on every borrowing parameter; or
   borrowed for the call, as Swift's closure parameters are, with a mark
   for escaping. Recommendation: global. Application code stays
   mark-free, and what is local shows in the source.
2. **How is a borrow through a global reference kept sound?** A borrow
   flag in the object header tested by conflicting tier 1 writes, as
   Swift does; a count for the borrow's duration, which protects only
   the outermost object; or the counted mode, where a function with
   sets is compiled again for global callers and counts as tier 1 does.
   Recommendation: the counted mode, with no check on global `&mut`. It
   adds no run-time check and no panic, and costs code size only where a
   function has both kinds of callers.
3. **Should closures get three kinds with capture sets?** `fn`,
   `fn mut` and `fn once`, sets after the arrow, kinds written in every
   header, and a plain `fn` global with tier 1's shared cells.
   Recommendation: yes. Scoped closures need `fn mut` to write a
   borrowed local without a cell, and `fn once` to hand off a
   NonCopyable value.
4. **How do NonCopyable structs look?** Swift's model: types copyable by
   default, `~Copy` on the declaration, generics opt in with
   `T < ~Copy`, moves unmarked at the use, `consume` on parameters that
   take ownership, and an optional synchronous `drop` at scope end next
   to `Disposable`. Recommendation: yes. Rust's opt-in `Copy` would put
   a bound on nearly every std generic.
5. **May a field borrow another field of its struct?** `{self.text}`,
   with the named field frozen after construction and limited to fields
   whose contents live in a counted buffer. Recommendation: yes. It
   gives views into a struct's own buffer, which Rust cannot express,
   at the cost of one frozen-field rule.

## 18. Open Points

- **Spelling.** `~Copy` against a word such as `@noncopy`; `[{s}]` for
  lifetime parameters against `[&s]`; `consume` against `sink`.
- **Shared references to strings refer to bytes**, so `r := &s` then
  `s = "x"` leaves `r` reading the old string. This differs from tier
  1's rule that a reference sees its slot; it is what lets `split`
  return references.
- **Payload borrows** in local mode, as Rust's `if let Some(f) = &mut
  x`, which tier 1's non-place payloads forbid.
- **Non-local `return` from scoped trailing blocks**, as Kotlin's
  inline lambdas.
- **No-alias between two scoped parameters in counted mode**, which
  would need a mark at the conversion from global references.

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
- hd: [OBJECT_MODEL_TIER1.md](OBJECT_MODEL_TIER1.md), [REFCOUNT_RUNTIME.md](REFCOUNT_RUNTIME.md),
  [RUST_MODEL_STUDY.md](RUST_MODEL_STUDY.md), [PARALLELISM_AND_LOCALITY.md](PARALLELISM_AND_LOCALITY.md),
  [07-functions.md](../../spec/lang/07-functions.md); the removed chapter 13 at
  `5d0b21e0a^` (`^`), `101344d4c^` (lifetime sets) and `a04c3497c^` (owned values).
