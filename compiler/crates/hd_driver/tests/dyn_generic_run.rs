//! Generic methods called through a trait value (codegen.md §13.5.1, the
//! erased method ABI; trait.dyn.safe.method-type-param): a vtable slot
//! holds one erased body per implementation, whose open instructions call
//! the thunks of the witness the caller passes for its method type
//! arguments. Packed and reference type arguments cross as `eqref`,
//! caller-owned containers are changed in place, and values keep their
//! identity. Programs build through the driver and run on V8
//! (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(main);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("dyn-generic-{name}.wasm"));
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

/// The design's first test (build-order.md §22, slice 6b), but for the
/// closure built in the erased body: a packed `T = i32` and a reference
/// `T`, a caller-owned `mut List[(T, T?)]`, a user `Buffer[T]` built and
/// returned, a generic helper, a caller's `fn(T) -> T`, a bound call in a
/// loop over an iterator, and a trait default, over two implementations.
#[test]
fn erased_bodies_run_at_packed_and_reference_type_arguments() {
    let main = "\
data Buffer[T]:
    items: List[T]
    len: i32

data User:
    name: string

impl Display for User:
    fn to_string(self) -> string: \"user ${self.name}\"

fn twice[T](x: T) -> List[T]:
    [x, x]

trait Collector:
    fn collect[T](self, x: T, out: mut List[(T, T?)]) -> Buffer[T]
    fn apply[T](self, x: T, f: fn(T) -> T) -> T
    fn show_all[T < Display](self, xs: List[T]) -> string
    fn pick[T](self, xs: List[T]) -> T?:
        if xs.len() > 0: xs[0]
        else: .None

data Simple:
    tag: string

impl Collector for Simple:
    fn collect[T](self, x: T, out: mut List[(T, T?)]) -> Buffer[T]:
        out.push((x, x))
        Buffer { items: twice(x), len: 2 }

    fn apply[T](self, x: T, f: fn(T) -> T) -> T:
        f(f(x))

    fn show_all[T < Display](self, xs: List[T]) -> string:
        let s = self.tag
        for x in xs:
            s = s + x.to_string()
        s

data Other:
    n: i32

impl Collector for Other:
    fn collect[T](self, x: T, out: mut List[(T, T?)]) -> Buffer[T]:
        out.push((x, .None))
        Buffer { items: [x], len: self.n }

    fn apply[T](self, x: T, f: fn(T) -> T) -> T:
        f(x)

    fn show_all[T < Display](self, xs: List[T]) -> string:
        \"other ${xs.len()}\"

fn run(c: dyn Collector) -> void $ Console:
    let ints: mut List[(i32, i32?)] = []
    b := c.collect(+5, ints)
    println(\"${b.len} ${b.items.len()} ${ints.len()}\")
    let users: mut List[(User, User?)] = []
    u := User { name: \"ada\" }
    ub := c.collect(u, users)
    println(\"${ub.items[0].name} ${users.len()}\")
    tenfold := fn(x: i32) -> i32: x * 10
    suffix := fn(x: string) -> string: x + \"b\"
    println(c.apply(+3, tenfold))
    println(c.apply(\"a\", suffix))
    println(c.show_all([+1, +2]))
    println(c.show_all([u]))
    match c.pick([+9]):
        .Some(v) => println(\"picked ${v}\")
        .None => println(\"none\")
    match c.pick(ints):
        .Some(v) =>
            let (first, _) = v
            println(\"picked pair ${first}\")
        .None => println(\"no pair\")

pub fn main() -> void $ Console:
    run(Simple { tag: \"s:\" })
    run(Other { n: 7 })
";
    assert_eq!(
        output_of("collector", main),
        "2 2 1\nada 1\n300\nabb\ns:12\ns:user ada\npicked 9\npicked pair 5\n\
         7 1 1\nada 1\n30\nab\nother 2\nother 1\npicked 9\npicked pair 5\n"
    );
}

/// An erased body that calls another generic method through a trait
/// value of a generic trait reaches that method's witness at its own
/// type arguments; wide integers, floats, enums with payloads, optionals
/// and tuples cross boxed.
#[test]
fn a_dyn_generic_call_inside_an_erased_body_reaches_its_own_witness() {
    let main = "\
trait Store[K]:
    fn put[V](self, key: K, value: V, into: mut List[(K, V)]) -> V

data Plain:
    n: i32

impl Store[string] for Plain:
    fn put[V](self, key: string, value: V, into: mut List[(string, V)]) -> V:
        into.push((key, value))
        value

trait Relay:
    fn relay[T](self, inner: dyn Store[string], value: T) -> T

data Hop:
    tag: string

impl Relay for Hop:
    fn relay[T](self, inner: dyn Store[string], value: T) -> T:
        let pairs: mut List[(string, T)] = []
        got := inner.put(self.tag, value, pairs)
        let (k, _) = pairs[0]
        if k != self.tag:
            panic(\"lost the key\")
        got

enum Shape:
    Dot
    Box(w: f64, h: f64)

pub fn main() -> void $ Console:
    let store: dyn Store[string] = Plain { n: 1 }
    let r: dyn Relay = Hop { tag: \"hop\" }
    println(r.relay(store, +7))
    let big: i64 = 1 << 40
    println(r.relay(store, big))
    println(r.relay(store, 2.5))
    shape := Shape.Box(w = 1.5, h = 2.0)
    s := r.relay(store, shape)
    match s:
        .Dot => println(\"dot\")
        .Box(w, h) => println(\"box ${w} ${h}\")
    let o: i32? = r.relay(store, .Some(+3))
    match o:
        .Some(v) => println(\"some ${v}\")
        .None => println(\"none\")
    t := r.relay(store, (+1, \"x\"))
    let (a, b) = t
    println(\"${a} ${b}\")
";
    assert_eq!(
        output_of("relay", main),
        "7\n1099511627776\n2.5\nbox 1.5 2.0\nsome 3\n1 x\n"
    );
}

/// A reference argument crosses without a copy, so the value an erased
/// body returns is the caller's own (`is`), through a direct `dyn` call
/// and through a bound at `C = dyn Keeper`.
#[test]
fn values_keep_their_identity_through_an_erased_body() {
    let main = "\
data User:
    name: string

trait Keeper:
    fn keep[T](self, x: T) -> T
    fn size[T](self, xs: List[T]) -> usize

data K:
    n: i32

impl Keeper for K:
    fn keep[T](self, x: T) -> T: x
    fn size[T](self, xs: List[T]) -> usize: xs.len() + 100

fn via[C < Keeper](c: C, u: User) -> bool:
    c.keep(u) is u

pub fn main() -> void $ Console:
    let k: dyn Keeper = K { n: 100 }
    u := User { name: \"ada\" }
    println(k.keep(u) is u)
    println(via(k, u))
    println(k.size([\"a\", \"b\"]))
    println(k.keep(\"text\"))
";
    assert_eq!(output_of("identity", main), "true\ntrue\n102\ntext\n");
}
