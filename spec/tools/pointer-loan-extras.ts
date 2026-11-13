// The adversarial cases of spec/tools/generate-pointer-loan-matrix.ts:
// each one shape the reference-kind x path x lifetime x action matrix does
// not reach, with its source and the verdicts of both models.

/** One adversarial case: its name, verdict, description and source. */
export interface Extra {
  name: string;
  panic: boolean;
  what: string;
  source: string;
  /** The rule the case checks, when it is not its section's. */
  rule?: string;
}

/**
 * The adversarial cases. A source names the matrix's shared helpers and
 * tail as `@@HELPERS@@` and `@@TAIL@@`, which the generator fills in.
 */
export const extras: Extra[] = [
  {
    name: "own-path-other-field",
    panic: false,
    what: "a write to another field through the same pointer the reference crossed counts the reference's own loan",
    source: `struct Cell:
    value: string
    other: string

@@HELPERS@@
fn probe(p: *mut Cell) -> string:
    r := &p.value
    p.other = fresh()
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    seen := probe(box(Cell { value: orig(), other: fresh() }))
@@TAIL@@`,
  },
  {
    name: "own-path-loop-other-field",
    panic: false,
    what: "a loop over a list behind a pointer writes another field through the same pointer",
    source: `struct Bag:
    items: List[string]
    count: usize

@@HELPERS@@
fn probe(p: *mut Bag) -> string:
    let seen = ""
    for item in &p.items:
        p.count += 1
        seen = *item
    seen

pub fn main() -> void $ Console:
    seen := probe(box(Bag { items: [orig()], count: 0 }))
@@TAIL@@`,
  },
  {
    name: "root-pointer-last-use-before-reference",
    panic: false,
    what: "the local holding a reference's root pointer stays alive until the reference's last use",
    source: `use std.collections.UniqList

struct Outer:
    items: UniqList[string]

@@HELPERS@@
fn uniq() -> UniqList[string]:
    let mut items = UniqList::[string].new()
    items.push(orig())
    items

fn probe() -> string:
    p := box(Outer { items: uniq() })
    r := p.items.get_ref(0).expect("slot")
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    seen := probe()
@@TAIL@@`,
  },
  {
    name: "root-pointer-moved-into-a-task",
    panic: false,
    what: "a task's pointer parameter stays alive across a suspension while a reference through it lives",
    source: `use std.collections.UniqList
use std.task.join
use std.time.{Clock, sleep}

struct Outer:
    items: UniqList[string]

@@HELPERS@@
fn uniq() -> UniqList[string]:
    let mut items = UniqList::[string].new()
    items.push(orig())
    items

fn probe!(p: *mut Outer) -> string $ Clock:
    r := p.items.get_ref(0).expect("slot")
    sleep!(2ms)
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

fn idle!() -> string $ Clock:
    sleep!(1ms)
    "idle"

pub fn main!() -> void $ Console + Clock:
    p := box(Outer { items: uniq() })
    let (seen, _) = join!(probe(p), idle())
@@TAIL@@`,
  },
  {
    name: "callee-writes-other-field-panics",
    panic: true,
    what: "a callee writes another field of an object the caller's reference borrows",
    source: `struct Cell:
    value: string
    other: string

@@HELPERS@@
fn bump(alias: *mut Cell) -> void:
    alias.other = fresh() # panic: borrow-conflict

fn probe(p: *mut Cell, alias: *mut Cell) -> string:
    r := &p.value
    bump(alias)
    *r

pub fn main() -> void $ Console:
    p := box(Cell { value: orig(), other: fresh() })
    seen := probe(p, p)
@@TAIL@@`,
  },
  {
    name: "callee-only-reads",
    panic: false,
    what: "passing the pointer to a callee that only reads is no access",
    source: `struct Cell:
    value: string
    other: string

@@HELPERS@@
fn peek(alias: *mut Cell) -> usize:
    alias.other.len()

fn probe(p: *mut Cell, alias: *mut Cell) -> string:
    r := &p.value
    _ := peek(alias)
    *r

pub fn main() -> void $ Console:
    p := box(Cell { value: orig(), other: fresh() })
    seen := probe(p, p)
@@TAIL@@`,
  },
  {
    name: "later-argument-replaces-panics",
    panic: true,
    what: "a later argument replaces the object an earlier argument's reference points into",
    source: `struct Cell:
    value: string

struct Outer:
    next: *mut Cell

@@HELPERS@@
fn replace(alias: *mut Outer) -> usize:
    alias.next = box(Cell { value: fresh() }) # panic: borrow-conflict
    _ := churn()
    1

fn observe(r: &string, n: usize) -> string:
    *r

fn probe(p: *mut Outer, alias: *mut Outer) -> string:
    observe(&p.next.value, replace(alias))

pub fn main() -> void $ Console:
    p := box(Outer { next: box(Cell { value: orig() }) })
    seen := probe(p, p)
@@TAIL@@`,
  },
  {
    name: "receiver-argument-reads-the-receiver",
    panic: false,
    what: "an argument of a `&mut self` call reads the receiver's object through an alias, as `xs.push(xs.len())` does",
    source: `struct Bag:
    items: List[usize]

fn probe(p: *mut Bag, alias: *Bag) -> usize:
    p.items.push(alias.items.len())
    p.items[1]

pub fn main() -> void $ Console:
    p := box(Bag { items: [+0usize] })
    if probe(p, p) != 1:
        panic("the push read the wrong length")
    println("ok")
`,
  },
  {
    name: "captured-variable-grows-panics",
    panic: true,
    rule: "ref.excl.alias.shared-variable",
    what: "a closure that shares a variable grows its list while a loop holds element references",
    source: `pub fn main() -> void $ Console:
    let mut xs = [+1, 2, 3]
    grow := fn() -> void:
        for round in 0..16:
            xs.push(4) # panic: borrow-conflict
    for x in &mut xs:
        grow()
        *x += 1
    println("\${xs.len()}")
`,
  },
  {
    name: "captured-variable-replaced-panics",
    panic: true,
    rule: "ref.excl.alias.shared-variable",
    what: "a stored closure that shares a variable replaces it while a reference into it lives",
    source: `struct Hook:
    run: fn() -> void

@@HELPERS@@
pub fn main() -> void $ Console:
    let mut xs = [orig()]
    hook := Hook { run: fn() -> void: xs = [] } # panic: borrow-conflict
    r := &xs[0]
    (hook.run)()
    junk := churn()
    seen := *r
    _ := junk.len()
@@TAIL@@`,
  },
  {
    name: "deep-shared-loans",
    panic: false,
    what: "forty nested shared loans on one object, past an inline counter's width",
    source: `struct Cell:
    value: string

@@HELPERS@@
fn depth(p: *Cell, n: usize) -> usize:
    r := &p.value
    below := if n == 0: 0 else: depth(p, n - 1)
    r.len() + below

pub fn main() -> void $ Console:
    p := box(Cell { value: orig() })
    if depth(p, 40) != 41 * orig().len():
        panic("the nested reads disagree")
    println("ok")
`,
  },
  {
    name: "deep-shared-loans-kept",
    panic: false,
    what: "seventy nested shared loans on one object that a `&mut` elsewhere keeps, past an inline counter's width and into the bits above it",
    source: `struct Cell:
    value: string

@@HELPERS@@
fn depth(p: *Cell, w: *mut Cell, n: usize) -> usize:
    r := &p.value
    below := if n == 0: 0 else: depth(p, w, n - 1)
    if n > 1000:
        m := &mut w.value
        *m = fresh()
    r.len() + below

pub fn main() -> void $ Console:
    p := box(Cell { value: orig() })
    w := box(Cell { value: fresh() })
    if depth(p, w, 70) != 71 * orig().len():
        panic("the nested reads disagree")
    p.value = fresh()
    println("ok")
`,
  },
  {
    name: "deep-shared-loans-many-objects",
    panic: false,
    what: "seventy objects each over 31 shared loans at once, which a `&mut` elsewhere keeps",
    source: `struct Cell:
    value: string

@@HELPERS@@
fn hold(ps: &List[*Cell], w: *mut Cell, k: usize, n: usize) -> usize:
    if k == ps.len():
        return 0
    p := ps[k]
    r := &p.value
    below := if n == 0: hold(ps, w, k + 1, 32) else: hold(ps, w, k, n - 1)
    if k > 100000:
        m := &mut w.value
        *m = fresh()
    r.len() + below

pub fn main() -> void $ Console:
    let mut ps: List[*Cell] = []
    for _ in 0..70:
        ps.push(box(Cell { value: orig() }))
    w := box(Cell { value: fresh() })
    if hold(&ps, w, 0, 32) != 70 * 33 * orig().len():
        panic("the nested reads disagree")
    w.value = orig()
    println("ok")
`,
  },
  {
    name: "suspending-creator-closure-write-panics",
    panic: true,
    what: "a closure a suspending body made writes through its pointer while another function's reference to the object lives",
    source: `use std.time.{Clock, ms, sleep}

struct Cell:
    value: string
    other: string

fn hold(a: *Cell, f: fn() -> void) -> usize:
    x := &a.value
    f()
    x.len()

pub fn main!() -> void $ Console + Clock:
    p := box(Cell { value: "a", other: "o" })
    alias := p
    r := &p.other
    f := fn() -> void: p.value = "b" # panic: borrow-conflict
    println(*r)
    sleep!(ms(1))
    println("\${hold(alias, f)}")
`,
  },
  {
    name: "suspending-closure-writes-another-field",
    panic: false,
    what: "a suspending closure writes another field through its creator's pointer, across a suspension, while the creator's reference lives",
    source: `use std.time.{Clock, ms, sleep}

struct Cell:
    value: string
    other: string

fn run!(f: fn!() -> void $ Clock) -> void $ Clock:
    f!()

fn run_plain(f: fn() -> void) -> void:
    f()

pub fn main!() -> void $ Console + Clock:
    p := box(Cell { value: "a", other: "o" })
    r := &p.other
    run_plain(fn() -> void: p.value = "b")
    run!(fn!() -> void $ Clock:
        sleep!(ms(1))
        p.value = "c"
    )
    if *r != "o" || p.value != "c":
        panic("the writes disagree")
    println("ok")
`,
  },
  {
    name: "dyn-receiver-alias-write-panics",
    panic: true,
    what: "a `&mut self` call through a `*mut dyn` writes the object through an alias it was given",
    source: `trait Touch:
    fn touch(&mut self, alias: *mut Counter) -> void

struct Counter:
    n: i32

impl Touch for Counter:
    fn touch(&mut self, alias: *mut Counter) -> void:
        self.n += 1
        alias.n += 1 # panic: borrow-conflict

pub fn main() -> void $ Console:
    c := box(Counter { n: +0 })
    let t: *mut dyn Touch = c
    t.touch(c)
    println("\${c.n}")
`,
  },
  {
    name: "generic-argument-alias-write-panics",
    panic: true,
    what: "a generic callee writes through an alias of the object its `&mut T` argument points into",
    source: `trait Grow:
    fn grow(&mut self) -> void

struct Counter:
    n: i32

impl Grow for Counter:
    fn grow(&mut self) -> void:
        self.n += 1

fn twice[T < Grow](value: &mut T, alias: *mut Counter) -> void:
    value.grow()
    alias.n += 1 # panic: borrow-conflict
    value.grow()

fn probe(c: *mut Counter, alias: *mut Counter) -> void:
    twice(&mut *c, alias)

pub fn main() -> void $ Console:
    c := box(Counter { n: +0 })
    probe(c, c)
    println("\${c.n}")
`,
  },
  {
    name: "copy-under-element-loan-panics",
    panic: true,
    what: "an alias copies a list while a `&mut` element reference into it lives",
    source: `struct Bag:
    items: List[string]

@@HELPERS@@
fn probe(p: *mut Bag, alias: *Bag) -> string:
    r := &mut p.items[0]
    copy := alias.items # panic: borrow-conflict
    *r = fresh()
    copy[0]

pub fn main() -> void $ Console:
    p := box(Bag { items: [orig()] })
    seen := probe(p, p)
@@TAIL@@`,
  },
  {
    name: "sort-comparator-alias-write-panics",
    panic: true,
    what: "a comparator writes another field of the object whose list it sorts",
    source: `struct Bag:
    items: List[i32]
    calls: i32

fn probe(p: *mut Bag, alias: *mut Bag) -> void:
    compare := fn(left: &i32, right: &i32) -> Ordering:
        alias.calls += 1 # panic: borrow-conflict
        left.cmp(right)
    p.items.sort_by(compare)

pub fn main() -> void $ Console:
    p := box(Bag { items: [+3, 1, 2], calls: +0 })
    probe(p, p)
    println("\${p.calls}")
`,
  },
  {
    name: "elide-callback-panics",
    panic: true,
    what: "a callback passed in writes the borrowed object; a function value has no summary",
    source: `struct Cell:
    value: string

@@HELPERS@@
fn probe(p: *mut Cell, alias: *mut Cell, act: fn(*mut Cell) -> void) -> string:
    r := &p.value
    act(alias)
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    p := box(Cell { value: orig() })
    seen := probe(p, p, fn(c: *mut Cell) -> void: c.value = fresh()) # panic: borrow-conflict
@@TAIL@@`,
  },
  {
    name: "elide-dyn-call-panics",
    panic: true,
    what: "a trait-value call writes the borrowed object through a pointer it holds",
    source: `trait Touch:
    fn touch(&self) -> void

struct Cell:
    value: string

struct Toucher:
    target: *mut Cell

impl Touch for Toucher:
    fn touch(&self) -> void:
        self.target.value = fresh() # panic: borrow-conflict

@@HELPERS@@
fn probe(p: *mut Cell, t: &dyn Touch) -> string:
    r := &p.value
    t.touch()
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    p := box(Cell { value: orig() })
    toucher := Toucher { target: p }
    seen := probe(p, &toucher)
@@TAIL@@`,
  },
  {
    name: "elide-writes-holder-type-panics",
    panic: true,
    what: "a callee writes only the type that holds the pointer the reference crossed",
    source: `struct Cell:
    value: string

struct Outer:
    next: *mut Cell

@@HELPERS@@
fn rewire(alias: *mut Outer) -> void:
    alias.next = box(Cell { value: fresh() }) # panic: borrow-conflict

fn probe(p: *mut Outer, alias: *mut Outer) -> string:
    r := &p.next.value
    rewire(alias)
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    p := box(Outer { next: box(Cell { value: orig() }) })
    seen := probe(p, p)
@@TAIL@@`,
  },
  {
    name: "elide-writes-through-field-reference-panics",
    panic: true,
    what: "a callee takes `&mut` of the field of a type that contains the borrowed one",
    source: `struct Cell:
    value: string

struct Wrap:
    cell: Cell

@@HELPERS@@
fn fix(c: &mut Cell) -> void:
    c.value = fresh()

fn reset(alias: *mut Wrap) -> void:
    fix(&mut alias.cell) # panic: borrow-conflict

fn probe(w: *mut Wrap, alias: *mut Wrap) -> string:
    r := &w.cell.value
    reset(alias)
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    w := box(Wrap { cell: Cell { value: orig() } })
    seen := probe(w, w)
@@TAIL@@`,
  },
  {
    name: "elide-closure-captured-pointer-panics",
    panic: true,
    what: "a closure that captured the pointer writes the object",
    source: `struct Cell:
    value: string

@@HELPERS@@
fn probe(p: *mut Cell, alias: *mut Cell) -> string:
    bump := fn() -> void: alias.value = fresh() # panic: borrow-conflict
    r := &p.value
    bump()
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    p := box(Cell { value: orig() })
    seen := probe(p, p)
@@TAIL@@`,
  },
  {
    name: "elide-other-task-panics",
    panic: true,
    what: "another task replaces a `UniqList` while the reference waits across a bang call",
    source: `use std.collections.UniqList
use std.task.join
use std.time.{Clock, s, sleep}

struct Bag:
    items: UniqList[string]

@@HELPERS@@
fn uniq(value: string) -> UniqList[string]:
    let mut items = UniqList::[string].new()
    items.push(value)
    items

fn hold!(p: *mut Bag) -> string $ Clock:
    r := p.items.get_ref(0).expect("slot")
    # join! polls its tasks in argument order (req.schedule.all-unfinished),
    # so act! writes while this task waits; the panic ends the run.
    sleep!(s(60))
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

fn act!(alias: *mut Bag) -> string:
    alias.items = uniq(fresh()) # panic: borrow-conflict
    "acted"

pub fn main!() -> void $ Console + Clock:
    p := box(Bag { items: uniq(orig()) })
    let (seen, _) = join!(hold(p), act(p))
    println("\${p.items.len()}")
@@TAIL@@`,
  },
  {
    name: "elide-recursive-call-panics",
    panic: true,
    what: "a recursive callee writes the object at its deepest call",
    source: `struct Cell:
    value: string

@@HELPERS@@
fn walk(alias: *mut Cell, depth: i32) -> void:
    if depth == 0:
        alias.value = fresh() # panic: borrow-conflict
    else:
        walk(alias, depth - 1)

fn probe(p: *mut Cell, alias: *mut Cell) -> string:
    r := &p.value
    walk(alias, 3)
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

pub fn main() -> void $ Console:
    p := box(Cell { value: orig() })
    seen := probe(p, p)
@@TAIL@@`,
  },
  {
    name: "elide-read-then-print",
    panic: false,
    what: "a reference read and printed with no write in its live range",
    source: `struct Person:
    name: string

@@HELPERS@@
fn greet(p: *Person) -> void $ Console:
    n := &p.name
    println(*n)

pub fn main() -> void $ Console:
    p := box(Person { name: "ok" })
    greet(p)
`,
  },
  {
    name: "elide-getter-in-loop",
    panic: false,
    what: "a getter called in a loop that writes only another type",
    source: `struct Person:
    name: string

struct Log:
    lines: i32

impl Person:
    fn name_ref(&self) -> &string:
        &self.name

@@HELPERS@@
fn total(p: *Person, log: *mut Log) -> usize:
    let sum: usize = 0
    for round in 0..3:
        n := p.name_ref()
        log.lines += 1
        sum += n.len()
    sum

pub fn main() -> void $ Console:
    p := box(Person { name: orig() })
    log := box(Log { lines: +0 })
    seen := if total(p, log) == 3 * orig().len(): orig() else: fresh()
@@TAIL@@`,
  },
  {
    name: "elide-read-only-visitor",
    panic: false,
    what: "a read-only visitor called through a trait value over pointers",
    source: `trait Visitor:
    fn see(&mut self, name: &string) -> void

struct Shape:
    name: string

struct Scene:
    shapes: List[*Shape]

struct Counter:
    chars: usize

impl Visitor for Counter:
    fn see(&mut self, name: &string) -> void:
        self.chars += name.len()

@@HELPERS@@
fn walk(scene: *Scene, v: &mut dyn Visitor) -> void:
    for shape in &scene.shapes:
        n := &shape.name
        v.see(n)

pub fn main() -> void $ Console:
    scene := box(Scene { shapes: [box(Shape { name: orig() }), box(Shape { name: orig() })] })
    let mut counter = Counter { chars: 0 }
    walk(scene, &mut counter)
    seen := if counter.chars == 2 * orig().len(): orig() else: fresh()
@@TAIL@@`,
  },
  {
    name: "pass-read-with-shared",
    panic: false,
    what: "passing a pointer as a `*T` and a `&` of its field to one call: two reads",
    source: `struct Cell:
    value: string
    other: string

@@HELPERS@@
fn look(c: *Cell, f: &string) -> string:
    _ := c.other.len()
    *f

pub fn main() -> void $ Console:
    view := box(Cell { value: orig(), other: fresh() })
    seen := look(view, &view.value)
@@TAIL@@`,
  },
  {
    name: "pass-alias-mut-with-shared-panics",
    panic: true,
    what: "passing an alias as a `*mut` and a `&` of a field through another name",
    source: `struct Cell:
    value: string
    other: string

@@HELPERS@@
fn mutate(c: *mut Cell, f: &string) -> string:
    c.other = fresh() # panic: borrow-conflict
    *f

fn probe(boxed: *mut Cell, alias: *mut Cell) -> string:
    mutate(alias, &boxed.value)

pub fn main() -> void $ Console:
    boxed := box(Cell { value: orig(), other: fresh() })
    seen := probe(boxed, boxed)
@@TAIL@@`,
  },
  {
    name: "pass-alias-read-with-mut-panics",
    panic: true,
    what: "passing an alias as a `*T` and a `&mut` of a field through another name",
    source: `struct Cell:
    value: string
    other: string

@@HELPERS@@
fn inspect(c: *Cell, f: &mut string) -> string:
    _ := c.other.len() # panic: borrow-conflict
    *f

fn probe(view: *mut Cell, alias: *Cell) -> string:
    inspect(alias, &mut view.value)

pub fn main() -> void $ Console:
    view := box(Cell { value: orig(), other: fresh() })
    seen := probe(view, view)
@@TAIL@@`,
  },
  {
    name: "dependent-result-alias-panics",
    panic: true,
    what: "an alias writes the object while a call result that depends on the pointer lives",
    source: `struct Cell:
    value: string
    other: string

impl Cell:
    fn change(&mut self) -> void:
        self.other = fresh()

@@HELPERS@@
fn borrow(b: &Cell) -> &string:
    &b.value

fn probe(boxed: *mut Cell, alias: *mut Cell) -> string:
    r := borrow(boxed)
    alias.change() # panic: borrow-conflict
    *r

pub fn main() -> void $ Console:
    boxed := box(Cell { value: orig(), other: fresh() })
    seen := probe(boxed, boxed)
@@TAIL@@`,
  },
  {
    name: "other-task-field-write-panics",
    panic: true,
    what: "another task of a `join!` writes the object a task's reference holds across a suspension",
    source: `use std.task.join
use std.time.{Clock, s, sleep}

struct Cell:
    value: string
    other: string

@@HELPERS@@
fn hold!(p: *mut Cell) -> string $ Clock:
    r := &p.value
    # join! polls its tasks in argument order (req.schedule.all-unfinished),
    # so act! runs, and writes, while this task waits here.
    sleep!(s(60))
    junk := churn()
    seen := *r
    _ := junk.len()
    seen

fn act!(alias: *mut Cell) -> string:
    alias.other = fresh() # panic: borrow-conflict
    "acted"

pub fn main!() -> void $ Console + Clock:
    p := box(Cell { value: orig(), other: fresh() })
    let (seen, _) = join!(hold(p), act(p))
@@TAIL@@`,
  },
  {
    name: "abandoned-task-ends-its-loans",
    panic: false,
    what: "a task that `race!` abandons while it holds an exclusive loan across a suspension ends that loan, so a later write to the object runs",
    source: `use std.task.race
use std.time.{Clock, ms, s, sleep}

struct Cell:
    value: string
    other: string

@@HELPERS@@
fn hold!(p: *mut Cell) -> string $ Clock:
    r := &mut p.value
    sleep!(s(60))
    *r

fn quick!() -> string $ Clock:
    sleep!(ms(1))
    orig()

pub fn main!() -> void $ Console + Clock:
    p := box(Cell { value: orig(), other: orig() })
    winner := race!(hold(p), quick())
    p.value = fresh()
    seen := if p.value == fresh(): winner else: "lost"
@@TAIL@@`,
  },
  {
    name: "hidden-alias-in-another-object-panics",
    panic: true,
    what: "a callee writes another field through an alias it reaches in another object while a reference argument holds a loan",
    source: `struct Cell:
    value: string
    other: string

struct Writer:
    cell: *mut Cell

@@HELPERS@@
fn older(writer: &Writer, seen: &string) -> string:
    writer.cell.other = fresh() # panic: borrow-conflict
    *seen

fn probe(boxed: *mut Cell, writer: &Writer) -> string:
    older(writer, &boxed.value)

pub fn main() -> void $ Console:
    boxed := box(Cell { value: orig(), other: fresh() })
    writer := Writer { cell: boxed }
    seen := probe(boxed, &writer)
@@TAIL@@`,
  },
  {
    name: "pointer-lent-as-mut-argument-panics",
    panic: true,
    what: "a pointer passed where `&mut` is expected lends its whole object from its evaluation, so a later argument that writes through an alias panics",
    source: `struct Cell:
    value: string
    other: string

@@HELPERS@@
fn write_other(alias: *mut Cell) -> usize:
    alias.other = fresh() # panic: borrow-conflict
    1

fn observe(c: &mut Cell, n: usize) -> string:
    c.value

fn probe(boxed: *mut Cell, alias: *mut Cell) -> string:
    observe(boxed, write_other(alias))

pub fn main() -> void $ Console:
    boxed := box(Cell { value: orig(), other: fresh() })
    seen := probe(boxed, boxed)
@@TAIL@@`,
  },
];
/** Each adversarial case's columns: elision under loans (A), keep-alive (B). */
export const extraColumns: Record<string, [string, string]> = {
  "own-path-other-field": ["loan needed", "runs"],
  "own-path-loop-other-field": ["loan needed", "runs"],
  "root-pointer-last-use-before-reference": ["elided", "runs"],
  "root-pointer-moved-into-a-task": ["elided", "runs"],
  "callee-writes-other-field-panics": ["loan needed", "runs"],
  "callee-only-reads": ["elided", "runs"],
  "later-argument-replaces-panics": ["loan needed", "runs, stale"],
  "receiver-argument-reads-the-receiver": ["loan needed", "runs"],
  "captured-variable-grows-panics": ["loan needed", "runs, stale"],
  "captured-variable-replaced-panics": ["loan needed", "runs, stale"],
  "deep-shared-loans": ["elided", "runs"],
  "deep-shared-loans-kept": ["loan needed", "runs"],
  "deep-shared-loans-many-objects": ["loan needed", "runs"],
  "suspending-creator-closure-write-panics": ["loan needed", "runs"],
  "suspending-closure-writes-another-field": ["loan needed", "runs"],
  "dyn-receiver-alias-write-panics": ["loan needed", "runs, sees the write"],
  "generic-argument-alias-write-panics": ["loan needed", "runs, sees the write"],
  "copy-under-element-loan-panics": ["loan needed", "runs; the copy keeps its own elements"],
  "sort-comparator-alias-write-panics": ["loan needed", "runs"],
  "elide-callback-panics": ["loan needed", "runs, sees the write"],
  "elide-dyn-call-panics": ["loan needed", "runs, sees the write"],
  "elide-writes-holder-type-panics": ["loan needed", "runs, stale"],
  "elide-writes-through-field-reference-panics": ["loan needed", "runs, sees the write"],
  "elide-closure-captured-pointer-panics": ["loan needed", "runs, sees the write"],
  "elide-other-task-panics": ["loan needed", "panics collection-pinned"],
  "elide-recursive-call-panics": ["loan needed", "runs, sees the write"],
  "elide-read-then-print": ["elided", "runs"],
  "elide-getter-in-loop": ["elided", "runs"],
  "elide-read-only-visitor": ["elided", "runs"],
  "pass-read-with-shared": ["elided", "runs"],
  "dependent-result-alias-panics": ["loan needed", "runs"],
  "pass-alias-mut-with-shared-panics": ["loan needed", "runs, sees the write"],
  "pass-alias-read-with-mut-panics": ["loan needed", "runs"],
  "hidden-alias-in-another-object-panics": ["loan needed", "runs"],
  "other-task-field-write-panics": ["loan needed", "runs"],
  "abandoned-task-ends-its-loans": ["loan needed", "runs"],
  "pointer-lent-as-mut-argument-panics": ["loan needed", "runs"],
};
