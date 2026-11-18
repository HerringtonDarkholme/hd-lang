import type { Extra } from "./pointer-loan-extras.ts";

/** Generic carriers exercise logical origins across concrete layouts. */
export const genericExtras: Extra[] = [
  {
    name: "through-generic-carrier-some",
    panic: true,
    rule: "ref.excl.alias.loan",
    what: "a generic optional carrier returns its present reference and an alias grows that source",
    source: `use std.annotation.inline

struct Slots:
    items: List[string]

impl Slots:
    fn get_ref['a](&'a self) -> (&'a string)?:
        if self.items.len() > 0: .Some(&self.items[0]) else: .None

    fn push(&mut self) -> void:
        for round in 0..64:
            self.items.push("new-" + "x".repeat(40 + round))

@inline(.Never)
fn keep[T](value: T?, fallback: T) -> T:
    match value:
        .Some(value) => value
        .None => fallback

fn through_call(p: *mut Slots, q: *mut Slots, alias: *mut Slots) -> string:
    r := keep(.Some(p.get_ref().expect("slot")), q.get_ref().expect("slot"))
    alias.push() # panic: borrow-conflict
    *r

pub fn main() -> void $ Console:
    p := box(Slots { items: ["original-" + "z".repeat(40)] })
    q := box(Slots { items: ["other-" + "q".repeat(40)] })
    println(through_call(p, q, p))
`,
  },
  {
    name: "through-generic-carrier-fallback",
    panic: true,
    rule: "ref.excl.alias.loan",
    what: "a generic optional carrier returns its fallback reference and an alias grows that source",
    source: `use std.annotation.inline

struct Slots:
    items: List[string]

impl Slots:
    fn get_ref['a](&'a self) -> (&'a string)?:
        if self.items.len() > 0: .Some(&self.items[0]) else: .None

    fn push(&mut self) -> void:
        for round in 0..64:
            self.items.push("new-" + "x".repeat(40 + round))

@inline(.Never)
fn keep[T](value: T?, fallback: T) -> T:
    match value:
        .Some(value) => value
        .None => fallback

fn through_call(p: *mut Slots, q: *mut Slots, alias: *mut Slots) -> string:
    r := keep(.None, q.get_ref().expect("slot"))
    alias.push() # panic: borrow-conflict
    *r

pub fn main() -> void $ Console:
    p := box(Slots { items: ["original-" + "z".repeat(40)] })
    q := box(Slots { items: ["other-" + "q".repeat(40)] })
    println(through_call(p, q, q))
`,
  },
  {
    name: "through-generic-carrier-struct",
    panic: true,
    rule: "ref.excl.alias.loan",
    what: "a generic padded struct carrier forwards its reference to a result slot",
    source: `use std.annotation.inline

struct Slots:
    items: List[string]

impl Slots:
    fn get_ref['a](&'a self) -> (&'a string)?:
        if self.items.len() > 0: .Some(&self.items[0]) else: .None

    fn push(&mut self) -> void:
        for round in 0..64:
            self.items.push("new-" + "x".repeat(40 + round))

struct Carrier[T]:
    padding: usize
    value: T

@inline(.Never)
fn keep[T](carrier: Carrier[T]) -> T:
    carrier.value

fn through_call(p: *mut Slots, alias: *mut Slots) -> string:
    r := keep(Carrier { padding: 42, value: p.get_ref().expect("slot") })
    alias.push() # panic: borrow-conflict
    *r

pub fn main() -> void $ Console:
    p := box(Slots { items: ["original-" + "z".repeat(40)] })
    println(through_call(p, p))
`,
  },
  {
    name: "through-generic-two-reference-slots",
    panic: false,
    rule: "ref.excl.alias.loan",
    what: "a generic tuple result keeps its second reference while the unused first source changes",
    source: `use std.annotation.inline
struct Slots:
    items: List[string]
impl Slots:
    fn get_ref['a](&'a self) -> (&'a string)?:
        if self.items.len() > 0: .Some(&self.items[0]) else: .None
    fn push(&mut self) -> void:
        for round in 0..64:
            self.items.push("new-" + "x".repeat(40 + round))
@inline(.Never)
fn keep[T < ?Copy](value: T) -> T:
    value
fn probe(p: *mut Slots, q: *mut Slots, alias: *mut Slots) -> string:
    pair := keep((p.get_ref().expect("slot"), q.get_ref().expect("slot")))
    r := pair._1
    alias.push()
    *r
pub fn main() -> void $ Console:
    p := box(Slots { items: ["left-" + "p".repeat(40)] })
    q := box(Slots { items: ["right-" + "q".repeat(40)] })
    seen := probe(p, q, p)
    if seen != "right-" + "q".repeat(40):
        panic("the unrelated mutation changed the selected reference")
    println("ok")
`,
  },
];
