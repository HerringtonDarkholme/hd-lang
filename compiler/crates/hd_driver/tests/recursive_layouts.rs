//! Recursive type layouts (#120; wasm-layout.md §15.2, §15.3): a
//! self-recursive enum is boxed (flat or subtypes), and data types and
//! trait values that name themselves lay out as Wasm GC recursion groups.
//! Programs build through the driver and run on V8 (`host/run.mjs`), as the
//! conformance runner does, so each module also validates there.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// Builds `main` as the entry module, the same bytes from the serial and
/// pool executors and from a warm cache (whose code entries decode their
/// recursion groups), and returns its standard output on V8.
fn output_of(name: &str, main: &str) -> String {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let wasm = |store: &MemoryStore, executor| {
        let host = Host {
            render_tir: &[],
            sources: &src,
            store,
            clock: &NoClock,
            executor,
        };
        let out = build(
            &host,
            "app",
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(out.diags.is_empty(), "{name}: {}", out.render());
        (out.wasm.expect("wasm"), out.counters.emitted)
    };
    let store = MemoryStore::default();
    let (serial, _) = wasm(&store, Executor::Serial(SerialOrder::Priority));
    let (pool, _) = wasm(&MemoryStore::default(), Executor::Pool(4));
    assert_eq!(pool, serial, "{name}: pool bytes");
    let (again, emitted) = wasm(&store, Executor::Serial(SerialOrder::Priority));
    assert_eq!(emitted, 0, "{name}: re-emitted");
    assert_eq!(again, serial, "{name}: warm bytes");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("recursive-{name}.wasm"));
    std::fs::write(&path, &serial).expect("write wasm");
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

#[test]
fn a_generic_binary_tree_inserts_walks_and_derives() {
    let main = "\
@derive(Eq, Debug)
enum Tree[T]:
    Leaf
    Node(left: Tree[T], value: T, right: Tree[T])

fn insert[T < Ord](tree: Tree[T], x: T) -> Tree[T]:
    match tree:
        .Leaf => Tree.Node(left=Tree.Leaf, value=x, right=Tree.Leaf)
        .Node(left, value, right) =>
            if x < value: Tree.Node(left=insert(left, x), value=value, right=right)
            else: Tree.Node(left=left, value=value, right=insert(right, x))

fn walk[T](tree: Tree[T], out: mut List[T]) -> void:
    match tree:
        .Leaf => pass
        .Node(left, value, right) =>
            walk(left, out)
            out.push(value)
            walk(right, out)

fn build(xs: List[i32]) -> Tree[i32]:
    let t: Tree[i32] = Tree.Leaf
    for x in xs:
        t = insert(t, x)
    t

pub fn main() -> void $ Console:
    t := build([5, 2, 8, 1, 9, 3])
    let out: mut List[i32] = []
    walk(t, out)
    println(debug(out))
    println(t == build([5, 2, 8, 1, 9, 3]))
    println(t == build([5, 2, 8, 1, 9]))
    println(debug(build([2, 1])))
    let names: mut List[string] = []
    walk(insert(insert(insert(Tree.Leaf, \"m\"), \"a\"), \"z\"), names)
    println(debug(names))
";
    assert_eq!(
        output_of("tree", main),
        "[1, 2, 3, 5, 8, 9]\ntrue\nfalse\n\
         Tree.Node(left=Tree.Node(left=Tree.Leaf, value=1, right=Tree.Leaf), value=2, right=Tree.Leaf)\n\
         [\"a\", \"m\", \"z\"]\n"
    );
}

#[test]
fn a_json_like_value_holds_lists_and_maps_of_itself() {
    // Five payload fields: the box is a base and one subtype per variant.
    let main = "\
enum Value:
    Null
    Flag(on: bool)
    Num(n: i64)
    Text(s: string)
    Items(items: List[Value])
    Fields(fields: Map[string, Value])

fn keys() -> List[string]: [\"name\", \"tags\", \"nested\"]

fn count(v: Value) -> i32:
    match v:
        .Items(items) =>
            let n = +1
            for item in items:
                n = n + count(item)
            n
        .Fields(fields) =>
            let n = +1
            for key in keys():
                match fields.get(key):
                    .Some(inner) => n = n + count(inner)
                    .None => pass
            n
        _ => 1

fn render(v: Value) -> string:
    match v:
        .Null => \"null\"
        .Flag(on) => if on: \"true\" else: \"false\"
        .Num(n) => \"$n\"
        .Text(s) => \"'$s'\"
        .Items(items) =>
            let parts: mut List[string] = []
            for item in items:
                parts.push(render(item))
            \"[\" + parts.join(\",\") + \"]\"
        .Fields(fields) =>
            let parts: mut List[string] = []
            for key in keys():
                match fields.get(key):
                    .Some(inner) => parts.push(key + \":\" + render(inner))
                    .None => pass
            \"{\" + parts.join(\",\") + \"}\"

pub fn main() -> void $ Console:
    let inner: mut Map[string, Value] = {}
    inner[\"name\"] = Value.Null
    let fields: mut Map[string, Value] = {}
    fields[\"name\"] = Value.Text(\"hd\")
    fields[\"tags\"] = Value.Items([Value.Flag(true), Value.Null, Value.Num(25)])
    fields[\"nested\"] = Value.Fields(inner)
    doc := Value.Fields(fields)
    println(count(doc))
    println(render(doc))
    println(render(Value.Items([])))
";
    assert_eq!(
        output_of("value", main),
        "8\n{name:'hd',tags:[true,null,25],nested:{name:null}}\n[]\n"
    );
}

#[test]
fn a_linked_list_through_an_optional_field() {
    let main = "\
data Node:
    value: i32
    next: Node?

fn push(list: Node?, value: i32) -> Node:
    Node { value: value, next: list }

fn total(list: Node?) -> i32:
    match list:
        .Some(node) => node.value + total(node.next)
        .None => 0

fn reverse(list: Node?) -> Node?:
    let out: Node? = .None
    let at = list
    while true:
        match at:
            .Some(node) =>
                out = .Some(Node { value: node.value, next: out })
                at = node.next
            .None => return out
    out

fn first(list: Node?) -> i32:
    match list:
        .Some(node) => node.value
        .None => -1

pub fn main() -> void $ Console:
    let list: Node? = .None
    list = push(list, 1)
    list = push(list, 2)
    list = push(list, 3)
    println(total(list))
    println(first(list))
    println(first(reverse(list)))
";
    assert_eq!(output_of("list", main), "6\n3\n1\n");
}

#[test]
fn mutually_recursive_expressions_and_statements() {
    let main = "\
@derive(Eq, Debug)
enum Expr:
    Lit(value: i64)
    Var(name: string)
    Add(left: Expr, right: Expr)
    Block(body: Stmt, result: Expr)

@derive(Eq, Debug)
enum Stmt:
    Let(name: string, value: Expr)
    Seq(first: Stmt, second: Stmt)

fn lookup(env: List[(string, i64)], name: string) -> i64:
    let found: i64 = 0
    for entry in env:
        if entry._0 == name:
            found = entry._1
    found

fn eval(e: Expr, env: mut List[(string, i64)]) -> i64:
    match e:
        .Lit(value) => value
        .Var(name) => lookup(env, name)
        .Add(left, right) => eval(left, env) + eval(right, env)
        .Block(body, result) =>
            run(body, env)
            eval(result, env)

fn run(s: Stmt, env: mut List[(string, i64)]) -> void:
    match s:
        .Let(name, value) =>
            v := eval(value, env)
            env.push((name, v))
        .Seq(first, second) =>
            run(first, env)
            run(second, env)

pub fn main() -> void $ Console:
    program := Expr.Block(
        body=Stmt.Seq(first=Stmt.Let(name=\"x\", value=Expr.Lit(40)), second=Stmt.Let(name=\"y\", value=Expr.Lit(2))),
        result=Expr.Add(left=Expr.Var(\"x\"), right=Expr.Var(\"y\")),
    )
    let env: mut List[(string, i64)] = []
    println(eval(program, env))
    println(Expr.Add(left=Expr.Lit(1), right=Expr.Var(\"z\")) == Expr.Add(left=Expr.Lit(1), right=Expr.Var(\"z\")))
    println(Stmt.Let(name=\"a\", value=Expr.Lit(1)) == Stmt.Let(name=\"a\", value=Expr.Lit(2)))
    println(debug(Expr.Block(body=Stmt.Let(name=\"a\", value=Expr.Lit(1)), result=Expr.Var(\"a\"))))
";
    assert_eq!(
        output_of("expr", main),
        "42\ntrue\nfalse\n\
         Expr.Block(body=Stmt.Let(name=\"a\", value=Expr.Lit(value=1)), result=Expr.Var(name=\"a\"))\n"
    );
}

#[test]
fn closures_trait_values_and_boxed_tuples_that_name_their_data() {
    // Each recursion runs through a type that is not data: a closure's
    // code type, a vtable's slot type, a tuple's box.
    let main = "\
data Step:
    name: string
    weight: i32
    then: fn(Step) -> i32

fn weigh(step: Step) -> i32:
    step.then(step)

trait Link:
    fn label(self) -> string
    fn next(self) -> dyn Link?

data Stop:
    name: string
    after: dyn Link?

impl Link for Stop:
    fn label(self) -> string: self.name
    fn next(self) -> dyn Link?: self.after

fn walk(link: dyn Link) -> string:
    let out = link.label()
    let at = link.next()
    while true:
        match at:
            .Some(l) =>
                out = out + \">\" + l.label()
                at = l.next()
            .None => return out
    out

data Pair:
    left: (Pair?, Pair?, string, i32)

fn depth(p: Pair?) -> i32:
    match p:
        .Some(x) =>
            a := depth(x.left._0)
            b := depth(x.left._1)
            1 + (if a > b: a else: b)
        .None => 0

pub fn main() -> void $ Console:
    s := Step { name: \"s\", weight: 7, then: fn(x: Step) -> i32: x.weight * 6 }
    println(weigh(s))
    let c: dyn Link = Stop { name: \"c\", after: .None }
    let b: dyn Link = Stop { name: \"b\", after: .Some(c) }
    let a: dyn Link = Stop { name: \"a\", after: .Some(b) }
    println(walk(a))
    leaf := Pair { left: (.None, .None, \"leaf\", 0) }
    mid := Pair { left: (.Some(leaf), .None, \"mid\", 1) }
    println(depth(.Some(Pair { left: (.Some(mid), .Some(leaf), \"top\", 2) })))
";
    assert_eq!(output_of("groups", main), "42\na>b>c\n3\n");
}

#[test]
fn an_enum_payload_at_growing_arguments_is_boxed() {
    // `E[T]` holds `E[T?]`: no instance's value layout is finite unless
    // the enum is boxed, which a cycle of declarations decides.
    let main = "\
enum Grow[T]:
    More(inner: Grow[T?])
    Done(value: T)

fn depth[T](g: Grow[T]) -> i32:
    match g:
        .More(_inner) => 1
        .Done(_value) => 0

pub fn main() -> void $ Console:
    let g: Grow[i32] = Grow.More(Grow.Done(.Some(+3)))
    println(depth(g))
    let d: Grow[i32] = Grow.Done(+3)
    println(depth(d))
";
    assert_eq!(output_of("grow", main), "1\n0\n");
}

#[test]
fn a_large_recursion_group_and_long_chains_of_types() {
    // One group of `N` data types (a ring), a chain of `N` data types and a
    // chain of `N` enums each held by value: layout, encoding and link
    // walk them with loops, so their depth is no native stack depth.
    const N: usize = 1500;
    let mut main = String::new();
    for i in 0..N {
        let ring = (i + 1) % N;
        // The chain's last type holds an `i32` where the others hold the next.
        let (chain, held) = if i + 1 < N {
            (format!("C{}", i + 1), format!("E{}", i + 1))
        } else {
            ("i32".to_owned(), "i32".to_owned())
        };
        writeln!(main, "data R{i}:\n    v: i32\n    next: R{ring}?\n").expect("write");
        writeln!(main, "data C{i}:\n    v: i32\n    next: {chain}?\n").expect("write");
        writeln!(main, "enum E{i}:\n    Stop\n    Go(next: {held})\n").expect("write");
    }
    main.push_str(
        "\
fn around(r: R0) -> i32:
    match r.next:
        .Some(n) => n.v
        .None => 0

fn along(c: C0) -> i32:
    match c.next:
        .Some(n) => n.v
        .None => 0

fn tag(e: E0) -> i32:
    match e:
        .Stop => 0
        .Go(_n) => 1

pub fn main() -> void $ Console:
    println(around(R0 { v: +1, next: .Some(R1 { v: +2, next: .None }) }))
    println(along(C0 { v: +3, next: .Some(C1 { v: +4, next: .None }) }))
    println(tag(E0.Go(E1.Stop)))
",
    );
    assert_eq!(output_of("long", &main), "2\n4\n1\n");
}
