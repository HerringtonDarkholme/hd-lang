//! Typed derivation bodies run (#253; annotations, "Walk, Describe, And
//! Build", "Facts", "Members And Variants"; `crates/hd_mono/DESIGN.md` §12.3, §13.6): the
//! generated `walk`, `describe`, `build` and `facts`, the facts each
//! derivation sees, one `Structure` shared by every derivation without
//! member lines, and the rebuild that a field edit forces. Programs build
//! through the driver and run on V8 (`host/run.mjs`), as the conformance
//! runner does.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn build_with(store: &MemoryStore, files: &[(&str, &str)]) -> Output {
    let mut src = MemorySources::default();
    for (name, text) in files {
        src.insert(name, text);
    }
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    // A block with member lines beside a `@derive` of another trait of
    // its package warns, and stays legal (`annot.line.drift.derive`,
    // `annot.line.drift.legal`); nothing else may be reported.
    assert!(
        out.diags
            .code
            .iter()
            .all(|c| *c == hd_diag::Code::DerivationLineDrift),
        "{}",
        out.render()
    );
    out
}

/// Runs a built program on V8 and returns its standard output.
fn run(out: &Output, name: &str) -> String {
    let wasm = out.wasm.as_ref().expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("structure-run-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}

fn output_of(name: &str, main: &str) -> String {
    run(
        &build_with(&MemoryStore::default(), &[("main.hd", main)]),
        name,
    )
}

/// `annot.walk.match`, `.members`, `.error`: the walk visits the value's
/// variant, then its members in declaration order, and the first `.Err`
/// ends it.
#[test]
fn walk_visits_members_in_order_and_stops_at_the_first_error() {
    let main = "\
use std.structure.{Structure, Field, Variant, Walker}

trait Trace:
    fn trace(self) -> string

struct Tracer:
    pub out: string

impl[S] Walker[S] for Tracer:
    type Error = string

    fn variant(&mut self, v: Variant[S]) -> Result[void, string]:
        self.out = self.out + v.info.name + \":\"
        .Ok(())

    fn member[F](&mut self, h: Field[S, F], value: F) -> Result[void, string]:
        if h.info.name == \"stop\":
            return .Err(\"stop@${h.info.position}\")
        self.out = self.out + h.info.name + \",\"
        .Ok(())

impl[T] Trace for T by Structure:
    fn trace(self) -> string:
        let mut w = Tracer { out: \"\" }
        match Structure::walk(self, &mut w):
            .Ok(_) => w.out + \"ok\"
            .Err(e) => w.out + e

@derive(Trace)
data Run:
    a: i32
    b: string
    stop: bool
    after: i32

@derive(Trace)
enum Step:
    Go(first: i32, second: i32)
    Halt(stop: i32, never: i32)

pub fn main() -> void $ Console:
    println(Run { a: 1, b: \"x\", stop: true, after: 2 }.trace())
    println(Step.Go(1, 2).trace())
    println(Step.Halt(1, 2).trace())
";
    assert_eq!(
        output_of("walk", main),
        "Run:a,b,stop@2\nGo:first,second,ok\nHalt:stop@0\n"
    );
}

/// `annot.describe.order`, `.no-value`, `annot.omit.skip`: a receiverless
/// template method describes the type without a value, every variant in
/// order, and an omitted member is not described.
#[test]
fn describe_lists_every_variant_without_a_value() {
    let main = "\
use std.structure.{Structure, Field, Variant, Describer}

trait Fields:
    fn fields() -> string

struct Names:
    pub out: string

impl[S] Describer[S] for Names:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        self.out = self.out + \"<\" + v.info.name + \">\"
        .Ok(())

    fn member[F](&mut self, h: Field[S, F]) -> Result[void, never]:
        self.out = self.out + \" \" + h.info.name
        .Ok(())

impl[T] Fields for T by Structure:
    fn fields() -> string:
        let mut d = Names { out: \"\" }
        _ := T::describe(&mut d)
        d.out

@derive(Fields)
enum Shape:
    Dot
    Pair(i32, i32)
    Box(width: i32, height: i32)

data Cached:
    id: i32
    cache: string = \"\"

impl Fields for Cached by Structure:
    cache = pass

pub fn main() -> void $ Console:
    println(Shape::fields())
    println(Cached::fields())
";
    assert_eq!(
        output_of("describe", main),
        "<Dot><Pair> _0 _1<Box> width height\n<Cached> id\n"
    );
}

/// `annot.build.next`, `.member`, `.missing`, `.construct`, `.errors`:
/// the source names some members, `missing` fills the others in
/// declaration order, an omitted member takes its declared default, and a
/// source's `.Err` ends the build.
#[test]
fn build_fills_the_members_the_source_never_names() {
    let main = "\
use std.ops.Default
use std.structure.{Structure, Field, Variant, Members, Key, Source}

trait Make:
    fn make(log: mut List[string]) -> Result[Self, string]

struct Partial[S]:
    pub log: mut List[string]
    pub named: bool

impl[S] Source[S] for Partial[S]:
    type Error = string

    fn variant(&mut self, choices: List[Variant[S]]) -> Result[Variant[S], string]:
        .Ok(choices[0])

    # Names the member at position 1 once, then ends.
    fn next(&mut self, members: Members[S]) -> Result[Key[S], string]:
        if self.named:
            return .Ok(members.end())
        self.named = true
        self.log.push(\"next\")
        .Ok(members.at(1))

    fn member[F < Default](&mut self, h: Field[S, F], previous: F?) -> Result[F, string]:
        self.log.push(\"member ${h.info.name}\")
        .Ok(F::default())

    fn missing[F](&mut self, h: Field[S, F]) -> Result[F, string]:
        self.log.push(\"missing ${h.info.name}\")
        match h.default():
            .Some(value) => .Ok(value)
            .None => .Err(\"no ${h.info.name}\")

impl[T] Make for T by Structure:
    fn make(log: mut List[string]) -> Result[T, string]:
        let mut s = Partial::[T] { log: log, named: false }
        T::build(&mut s)

@derive(Make)
data Conf:
    a: i32 = 7
    b: string
    c: i32 = 9

@derive(Make)
data Strict:
    a: i32
    b: string

data Kept:
    a: i32 = 1
    b: string
    skip: i32 = 5

impl Make for Kept by Structure:
    skip = pass

pub fn main() -> void $ Console:
    let log: mut List[string] = []
    match Conf::make(log):
        .Ok(c) => println(\"${c.a} '${c.b}' ${c.c}\")
        .Err(e) => println(e)
    println(log.join(\", \"))
    let more: mut List[string] = []
    match Strict::make(more):
        .Ok(_) => println(\"built\")
        .Err(e) => println(e)
    println(more.join(\", \"))
    let kept: mut List[string] = []
    match Kept::make(kept):
        .Ok(k) => println(\"${k.a} '${k.b}' ${k.skip}\")
        .Err(e) => println(e)
    println(kept.join(\", \"))
";
    assert_eq!(
        output_of("build", main),
        "7 '' 9\nnext, member b, missing a, missing c\n\
         no a\nnext, member b, missing a\n\
         1 '' 5\nnext, member b, missing a\n"
    );
}

/// The facts each derivation sees, in source order (`annot.fact.*`,
/// `annot.no-trait.after-decorators`, `annot.no-trait.then-blocks`,
/// `annot.line.*`, `annot.variant.data-facts`, `annot.fact.payload`): the
/// declaration's decorators, then its block with no trait, then the
/// derivation block's lines, which stay local to their block.
#[test]
fn facts_arrive_in_source_order_per_derivation() {
    let main = "\
use std.inspect.{Inspectable, downcast_val}
use std.structure.{Structure, Facts, Field, Variant, Walker}

data Tag:
    text: string

data Note:
    text: string

data Mark:
    text: string

data Flag:
    text: string

fn tag(text: string) -> Tag:
    Tag { text: text }

fn note(text: string) -> Note:
    Note { text: text }

fn mark(text: string) -> Mark:
    Mark { text: text }

fn flag(text: string) -> Flag:
    Flag { text: text }

# Each fact as `kind:text`, in the order `Facts` holds them.
fn show(facts: Facts) -> string:
    let parts: mut List[string] = []
    for item in facts.items:
        match downcast_val::[Tag](item):
            .Some(t) => parts.push(\"tag:\" + t.text)
            .None => pass
        match downcast_val::[Note](item):
            .Some(t) => parts.push(\"note:\" + t.text)
            .None => pass
        match downcast_val::[Mark](item):
            .Some(t) => parts.push(\"mark:\" + t.text)
            .None => pass
        match downcast_val::[Flag](item):
            .Some(t) => parts.push(\"flag:\" + t.text)
            .None => pass
    \"[\" + parts.join(\" \") + \"]\"

trait Probe:
    fn probe(self) -> string

trait Plain:
    fn plain(self) -> string

struct Prober:
    pub out: string

impl[S] Walker[S] for Prober:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        self.out = self.out + \" \" + v.info.name + show(v.info.facts)
        .Ok(())

    fn member[F](&mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + \" \" + h.info.name + show(h.info.facts)
        .Ok(())

impl[T] Probe for T by Structure:
    fn probe(self) -> string:
        let mut w = Prober { out: show(T::facts()) }
        _ := Structure::walk(self, &mut w)
        w.out

impl[T] Plain for T by Structure:
    fn plain(self) -> string:
        let mut w = Prober { out: show(T::facts()) }
        _ := Structure::walk(self, &mut w)
        w.out

@derive(Plain)
@tag(\"t\")
@note(\"n\")
data Point:
    @flag(\"x1\")
    @tag(\"x2\")
    x: i32
    @tag(\"y1\")
    y: i32
    z: i32

impl Point by Structure:
    Self += [mark(\"m\")]
    x += [note(\"x3\")]

impl Probe for Point by Structure:
    Self += [flag(\"f\")]
    x += [mark(\"x4\")]
    y = [note(\"y2\")]

@derive(Probe)
@tag(\"e\")
enum Event:
    @note(\"v0\")
    Start(@flag(\"p0\") at: i32, by: string)
    Stop

pub fn main() -> void $ Console:
    p := Point { x: 1, y: 2, z: 3 }
    println(p.probe())
    println(p.plain())
    println(Event.Start(1, \"a\").probe())
    println(Event.Stop.probe())
";
    assert_eq!(
        output_of("facts", main),
        "[tag:t note:n mark:m flag:f] Point[] x[flag:x1 tag:x2 note:x3 mark:x4] y[note:y2] z[]\n\
         [tag:t note:n mark:m] Point[] x[flag:x1 tag:x2 note:x3] y[tag:y1] z[]\n\
         [tag:e] Start[note:v0] at[flag:p0] by[]\n\
         [tag:e] Stop[]\n"
    );
}

/// `annot.tuple.*`: a tuple's `Structure` through a tuple template, beside
/// a data type's through the template, with one walker: the tuple has no
/// facts, no shared data and the name `\"\"`.
#[test]
fn a_tuple_target_walks_through_the_same_generator() {
    let main = "\
use std.function.Tuple
use std.structure.{Structure, Field, Variant, Walker}

trait Shape:
    fn shape(self) -> string

struct Lister:
    pub out: string

impl[S] Walker[S] for Lister:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        self.out = self.out + \"<\" + v.info.name + \" facts=${v.info.facts.items.len()} shared=${v.info.shared.len()}>\"
        .Ok(())

    fn member[F < Display](&mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + \" \" + h.info.name + \"=$value\"
        .Ok(())

impl[T] Shape for T by Structure:
    fn shape(self) -> string:
        let mut w = Lister { out: T::name() + \" ${T::facts().items.len()}\" }
        _ := Structure::walk(self, &mut w)
        w.out

impl[T < Tuple] Shape for T by Structure:
    fn shape(self) -> string:
        let mut w = Lister { out: \"'\" + T::name() + \"' ${T::facts().items.len()}\" }
        _ := Structure::walk(self, &mut w)
        w.out

@derive(Shape)
enum Size(code: i32):
    Small -> Size(+1)
    Large(scale: i32) -> Size(+2)

pub fn main() -> void $ Console:
    println((1, \"a\").shape())
    println(Size.Large(3).shape())
";
    assert_eq!(
        output_of("tuple", main),
        "'' 0< facts=0 shared=0> _0=1 _1=a\nSize 0<Large facts=0 shared=1> scale=3\n"
    );
}

const TWO: &str = "\
use std.structure.{Structure, Field, Variant, Walker}

trait Left:
    fn left(self) -> string

trait Right:
    fn right(self) -> string

struct Lister:
    pub out: string

impl[S] Walker[S] for Lister:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F < Display](&mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.out = self.out + h.info.name + \"=$value;\"
        .Ok(())

impl[T] Left for T by Structure:
    fn left(self) -> string:
        let mut w = Lister { out: \"L \" }
        _ := Structure::walk(self, &mut w)
        w.out

impl[T] Right for T by Structure:
    fn right(self) -> string:
        let mut w = Lister { out: \"R \" }
        _ := Structure::walk(self, &mut w)
        w.out

";

/// Builds `TWO` with `decl` and `main`, and returns its output and how many
/// instances it emitted.
fn two(name: &str, decl: &str, main: &str) -> (String, usize) {
    let text = format!("{TWO}{decl}\npub fn main() -> void $ Console:\n{main}");
    let out = build_with(&MemoryStore::default(), &[("main.hd", &text)]);
    (run(&out, name), out.counters.emitted)
}

/// `crates/hd_mono/DESIGN.md` §13.6, design Q2: two derivations of one target that see the
/// declaration's `Structure` call one `walk` instance per walker, and one
/// set of handles; a derivation block with member lines sees, and emits,
/// its own.
#[test]
fn two_derivations_of_one_target_share_their_bodies() {
    let shared = "@derive(Left, Right)\ndata Point:\n    x: i32\n    y: i32\n";
    let one = "    println(Point { x: 1, y: 2 }.left())\n";
    let both =
        "    println(Point { x: 1, y: 2 }.left())\n    println(Point { x: 1, y: 2 }.right())\n";
    let (out1, n1) = two("share-one", shared, one);
    let (out2, n2) = two("share-both", shared, both);
    assert_eq!(out1, "L x=1;y=2;\n");
    assert_eq!(out2, "L x=1;y=2;\nR x=1;y=2;\n");
    // The second derivation adds its own `right` instance and the `get`
    // and `set` closures of its `&mut w` (crates/hd_check/src/refs.rs),
    // and nothing else.
    assert_eq!(n2, n1 + 3, "shared walk and handles");
    // A block with a member line gets its own `Structure`: its own `walk`
    // and handles, so more instances.
    let own = "@derive(Left)\ndata Point:\n    x: i32\n    y: i32\n\nimpl Right for Point by Structure:\n    x += [\"k\"]\n";
    let (out3, n3) = two("share-own", own, both);
    assert_eq!(out3, "L x=1;y=2;\nR x=1;y=2;\n");
    assert!(
        n3 > n2 + 1,
        "a derivation with member lines shares nothing: {n3} vs {n2}"
    );
}

/// The cache claim of the design (`f1c55020:future-work/compiler/typed-derivation.md`,
/// "How they stay cached per type"): adding, removing, reordering or
/// renaming a field of a derived type changes the generated bodies' code
/// keys, so a rebuild through one store re-emits them and never reuses a
/// stale traversal; an unchanged rebuild hits the link entry. The type and
/// its derivations are in one module, the program that reads them in
/// another, which no edit touches.
#[test]
fn a_field_edit_rebuilds_the_generated_bodies() {
    let store = MemoryStore::default();
    let main = "\
use pkg.shapes.Point
use std.ops.Default

pub fn main() -> void $ Console:
    println(debug(Point::default()))
";
    let shapes = |fields: &str| {
        format!("use std.ops.Default\n\n@derive(Debug, Default)\npub data Point:\n{fields}")
    };
    let build_of = |fields: &str| {
        let text = shapes(fields);
        build_with(&store, &[("main.hd", main), ("shapes.hd", &text)])
    };
    let steps = [
        ("    x: i32\n    y: i32\n", "Point { x: 0, y: 0 }\n"),
        ("    y: i32\n    x: i32\n", "Point { y: 0, x: 0 }\n"),
        (
            "    y: i32\n    x: i32\n    label: string\n",
            "Point { y: 0, x: 0, label: \"\" }\n",
        ),
        (
            "    x: i32\n    label: string\n",
            "Point { x: 0, label: \"\" }\n",
        ),
        (
            "    w: i32\n    label: string\n",
            "Point { w: 0, label: \"\" }\n",
        ),
    ];
    for (k, (fields, want)) in steps.iter().enumerate() {
        let out = build_of(fields);
        assert_eq!(run(&out, &format!("edit-{k}")), *want, "step {k}");
        assert_eq!(out.counters.hit("link"), 0, "step {k} reused a stale link");
        assert!(out.counters.emitted > 0, "step {k} emitted nothing");
        // A rebuild with no edit reuses everything.
        let again = build_of(fields);
        assert_eq!(again.counters.hit("link"), 1, "step {k} rebuilt unchanged");
        assert_eq!(again.wasm, out.wasm);
    }
}

/// Templates that keep the information of every handle a traversal passes
/// (`annot.handle.escape`): `Seen` through `walk`, `Listed` through
/// `describe`. A handle's `info` is a struct value (`annot.structure.decl`),
/// so the test compares what it holds.
const KEEPERS: &str = "\
use std.structure.{Structure, Field, Variant, Walker, Describer, Member, VariantInfo}

struct Keeper:
    pub variants: mut List[VariantInfo]
    pub members: mut List[Member]

impl[S] Walker[S] for Keeper:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        self.variants.push(v.info)
        .Ok(())

    fn member[F](&mut self, h: Field[S, F], value: F) -> Result[void, never]:
        self.members.push(h.info)
        .Ok(())

impl[S] Describer[S] for Keeper:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        self.variants.push(v.info)
        .Ok(())

    fn member[F](&mut self, h: Field[S, F]) -> Result[void, never]:
        self.members.push(h.info)
        .Ok(())

trait Seen:
    fn seen(self) -> Keeper

impl[T] Seen for T by Structure:
    fn seen(self) -> Keeper:
        let mut k = Keeper { variants: [], members: [] }
        _ := Structure::walk(self, &mut k)
        k

trait Listed:
    fn listed() -> Keeper

impl[T] Listed for T by Structure:
    fn listed() -> Keeper:
        let mut k = Keeper { variants: [], members: [] }
        _ := T::describe(&mut k)
        k
";

/// `annot.handle.constants`, `annot.handle.escape`: each member and
/// variant has one handle. Two walks of different values, a `describe` by
/// a second derivation and a JSON encode in between all see the same
/// member and variant information, and another variant's handle holds
/// another.
#[test]
fn every_traversal_passes_the_same_handle_instances() {
    let main = format!(
        "{KEEPERS}
use std.json.encode
use std.serde.Serialize

@derive(Seen, Listed, Serialize)
data Order:
    id: i64
    customer: string

@derive(Seen)
enum Step:
    Go(first: i32)
    Halt(code: i32)

pub fn main() -> void $ Console:
    first := Order {{ id: 1, customer: \"Ada\" }}.seen()
    println(encode(Order {{ id: 2, customer: \"Bo\" }}))
    second := Order {{ id: 3, customer: \"Cy\" }}.seen()
    listed := Order::listed()
    println(\"${{first.variants[0].name == second.variants[0].name}} ${{first.members[0].name == second.members[0].name}} ${{first.members[1].position == second.members[1].position}}\")
    println(\"${{first.variants[0].name == listed.variants[0].name}} ${{first.members[1].name == listed.members[1].name}} ${{first.members[0].name == first.members[1].name}}\")
    go := Step.Go(1).seen()
    again := Step.Go(2).seen()
    halt := Step.Halt(3).seen()
    println(\"${{go.variants[0].index == again.variants[0].index}} ${{go.members[0].name == again.members[0].name}} ${{go.variants[0].index == halt.variants[0].index}}\")
"
    );
    assert_eq!(
        output_of("handle-constants", &main),
        "{\"id\":2,\"customer\":\"Bo\"}\ntrue true true\ntrue true false\ntrue true false\n"
    );
}

/// `annot.fact.eval.lazy`, `annot.fact.eval.unread`: a member's or
/// variant's facts run once, when a traversal first builds the handle
/// that carries them, even if no template reads them; later traversals
/// reuse the handle and run nothing.
#[test]
fn member_facts_run_once_when_the_handle_is_first_built() {
    let main = format!(
        "{KEEPERS}
let evaluations = +0

data Style:
    prefix: string = \"\"

fn style(prefix: string) -> Style:
    evaluations = evaluations + 1
    Style {{ prefix: prefix }}

@derive(Seen, Listed)
data Point:
    @style(\"x_\")
    x: i64
    y: i64

@derive(Seen)
enum Shape:
    @style(\"dot_\")
    Dot
    Line(length: i64)

pub fn main() -> void $ Console:
    point := Point {{ x: 1, y: 2 }}
    println(evaluations)
    _ := point.seen()
    println(evaluations)
    _ := Point {{ x: 3, y: 4 }}.seen()
    _ := Point::listed()
    println(evaluations)
    _ := Shape.Line(1).seen()
    println(evaluations)
    _ := Shape.Dot.seen()
    _ := Shape.Dot.seen()
    println(evaluations)
"
    );
    assert_eq!(output_of("handle-facts-once", &main), "0\n1\n1\n1\n2\n");
}

/// `annot.handle.constants` for a generic owner: one handle per
/// instantiation, shared by every value of it. The handles' information
/// is a struct value, so `Box[i32]`'s and `Box[string]`'s compare equal
/// field by field, as two instantiations' information is alike.
#[test]
fn a_generic_owner_has_one_handle_per_instantiation() {
    let main = format!(
        "{KEEPERS}
data Box[T]:
    value: T

impl[T] Seen for Box[T] by Structure

pub fn main() -> void $ Console:
    ints := Box {{ value: +1 }}.seen()
    more := Box {{ value: +2 }}.seen()
    strs := Box {{ value: \"a\" }}.seen()
    other := Box {{ value: \"b\" }}.seen()
    println(\"${{ints.members[0].name == more.members[0].name}} ${{strs.members[0].position == other.members[0].position}} ${{ints.members[0].name == strs.members[0].name}} ${{ints.variants[0].index == strs.variants[0].index}}\")
"
    );
    assert_eq!(output_of("handle-generic", &main), "true true true true\n");
}
