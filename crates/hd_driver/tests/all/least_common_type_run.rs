//! The least common type at every site (spec 04 "Least Common Type",
//! `types.lct.*`) and the narrower join of a generic call's arguments
//! (`types.generic.infer.join`). Valid joins build and run on V8
//! (`host/run.mjs`); a value that does not join is `no-common-type`, or
//! `no-least-common-type`, reported once at that value.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", goal)
}

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("least-common-type-{name}.wasm"),
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

/// The diagnostic codes of a program checked without running.
fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

/// A plain value before an optional one widens a map literal's values, a
/// closure's inferred result, and a function's omitted result, as it does
/// an `if` (`types.lct.optional`); map keys join too (`types.lct.sites`).
#[test]
fn optional_values_widen_every_site() {
    let main = "\
fn late(flag: bool, count: i32, found: i32?):
    if flag:
        return count
    found

fn closure(count: i32, found: i32?) -> i32?:
    pick := fn(flag: bool):
        if flag:
            return count
        found
    pick(true)

fn values(count: i32, found: i32?) -> Map[string, i32?]:
    joined := {\"a\": count, \"b\": found}
    joined

fn keys(small: i16) -> Map[i16, string]:
    joined := {small: \"a\", 2: \"b\"}
    joined

fn show(v: i32?) -> string:
    match v:
        .Some(n) => \"some ${n}\"
        .None => \"none\"

pub fn main() -> void $ Console:
    println(show(late(true, 1, .None)))
    println(show(late(false, 1, .Some(5))))
    println(show(closure(3, .None)))
    let m = values(4, .None)
    println(show(m.get(\"a\").unwrap_or(.None)))
    println(keys(+1).len())
";
    assert_eq!(
        output_of("widen", main),
        "some 1\nsome 5\nsome 3\nsome 4\n2\n"
    );
}

/// Permission weakening, a `never` value, a nested literal joined by its own
/// type, and a generic call's permission join all keep working.
#[test]
fn weakening_never_and_nested_literals_join() {
    let main = "\
data User:
    name: string

fn weak(editable: mut User, shared: User) -> List[User]:
    both := [editable, shared]
    both

fn never_first(flag: bool, count: i32) -> i32:
    value := if flag: panic(\"no\") else: count
    value

fn nested(x: i64, flag: bool) -> List[List[i64]]:
    lists := [[1], [x]]
    picked := if flag: [x] else: [2]
    [picked, lists[1]]

fn same[T](left: T, right: T) -> bool:
    true

pub fn main() -> void $ Console:
    let u = User { name: \"a\" }
    println(weak(User { name: \"b\" }, u)[1].name)
    println(never_first(false, 9))
    println(nested(7, true)[1][0])
    println(same(User { name: \"c\" }, u))
";
    assert_eq!(output_of("weak", main), "a\n9\n7\ntrue\n");
}

/// Two values with no common type, at each site of `types.lct.sites`.
#[test]
fn every_site_reports_no_common_type() {
    let cases = [
        ("list", "fn f(a: i16, b: i64) -> void:\n    _v := [a, b]\n"),
        (
            "map key",
            "fn f(a: i16, b: i64) -> void:\n    _v := {a: 1, b: 2}\n",
        ),
        (
            "map value",
            "fn f(a: i32, b: bool) -> void:\n    _v := {\"a\": a, \"b\": b}\n",
        ),
        (
            "if",
            "fn f(c: bool, a: i32, b: string) -> void:\n    _v := if c: a else: b\n",
        ),
        (
            "match",
            "fn f(k: i32, a: i32, b: i32??) -> void:\n    _v := match k:\n        0 => b\n        _ => a\n",
        ),
        (
            "closure",
            "fn f(a: i32) -> void:\n    _g := fn(c: bool):\n        if c:\n            return \"x\"\n        a\n",
        ),
        (
            "omitted result",
            "fn f(c: bool, a: i32):\n    if c:\n        return \"x\"\n    a\n",
        ),
        (
            "nested list",
            "fn f(c: bool, x: i64) -> void:\n    _v := if c: [x] else: [\"x\"]\n",
        ),
    ];
    for (site, main) in cases {
        assert_eq!(codes_of(main), vec![Code::NoCommonType], "{site}");
    }
}

/// A site that restarts to widen to an optional reports a later value
/// that does not join as `no-common-type`, in any value order, not as a
/// mismatch against the widened type.
#[test]
fn a_restarted_site_reports_no_common_type() {
    let lists = [
        "[1, a, \"s\"]",
        "[1, \"s\", a]",
        "[\"s\", 1, a]",
        "[a, 1, \"s\"]",
        "[1, a, x]",
        "[1, x, a]",
        "[x, 1, a]",
        "[1, a, 2, x]",
        "[1, a, true, 2]",
    ];
    for list in lists {
        let main = format!("fn f(a: i32?, x: i32??) -> void:\n    _v := {list}\n");
        assert_eq!(codes_of(&main), vec![Code::NoCommonType], "{list}");
    }
    let cases = [
        (
            "map value",
            "fn f(a: i32?) -> void:\n    _v := {\"p\": 1, \"q\": a, \"r\": \"s\"}\n",
        ),
        (
            "map key",
            "fn f(a: i32?) -> void:\n    _v := {1: 0, a: 1, \"s\": 2}\n",
        ),
        (
            "match, string first",
            "fn f(k: i32, a: i32?) -> void:\n    _v := match k:\n        0 => \"s\"\n        1 => 1\n        _ => a\n",
        ),
        (
            "match",
            "fn f(k: i32, a: i32?) -> void:\n    _v := match k:\n        0 => 1\n        1 => a\n        _ => \"s\"\n",
        ),
        (
            "closure",
            "fn f(c: bool, a: i32?) -> void:\n    _g := fn():\n        if c:\n            return 1\n        if c:\n            return a\n        \"s\"\n",
        ),
        (
            "omitted result",
            "fn f(c: bool, a: i32?):\n    if c:\n        return 1\n    if c:\n        return a\n    \"s\"\n",
        ),
        (
            "deeper optional",
            "fn f(k: i32, a: i32?, x: i32??) -> void:\n    _v := match k:\n        0 => 1\n        1 => a\n        _ => x\n",
        ),
    ];
    for (site, main) in cases {
        assert_eq!(codes_of(main), vec![Code::NoCommonType], "{site}");
    }
    // A restart whose values all join still gives the optional type.
    let ok = "fn f(c: bool, a: i32?) -> void:\n    let _v: List[i32?] = [1, a, 2]\n    _w := [1, a, 2, a]\n    _m := {\"p\": 1, \"q\": a}\n    _i := if c: 1 else: a\n";
    assert_eq!(codes_of(ok), Vec::<Code>::new());
}

/// No trait-value conversion or supertrait widening joins values
/// (`types.lct.no-trait-value`, `types.lct.no-supertrait-widening`), in
/// either order, while an expected type may ask for one
/// (`types.lct.expected-trait`).
#[test]
fn trait_values_have_no_common_type_without_an_expected_type() {
    let decls = "\
data User:
    name: string

impl Display for User:
    fn to_string(self) -> string:
        self.name

trait Named:
    fn name(self) -> string

trait Shown < Named:
    fn show(self) -> string

trait Tagged < Named:
    fn tag(self) -> string
";
    for body in [
        "fn f(u: User, d: dyn Display) -> void:\n    _v := [d, u]\n",
        "fn f(u: User, d: dyn Display) -> void:\n    _v := [u, d]\n",
        "fn f(s: dyn Shown, t: dyn Tagged) -> void:\n    _v := [s, t]\n",
        "fn f(s: dyn Shown, n: dyn Named) -> void:\n    _v := [n, s]\n",
    ] {
        let main = format!("{decls}\n{body}");
        assert_eq!(codes_of(&main), vec![Code::NoCommonType], "{body}");
    }
    let main =
        format!("{decls}\nfn f(s: dyn Shown, t: dyn Tagged) -> List[dyn Named]:\n    [s, t]\n");
    assert_eq!(codes_of(&main), vec![]);
}

/// A generic call's arguments for one bare parameter: a trait-value
/// conversion or supertrait widening is `no-common-type`, in either order
/// (`types.generic.infer.join.no-trait-value`,
/// `types.generic.infer.join.no-supertrait-widening`); any other conflict
/// stays `type-mismatch` (`types.generic.infer.join.other-conflict`).
#[test]
fn generic_arguments_join_without_trait_value_conversions() {
    let decls = "\
data User:
    name: string

impl Display for User:
    fn to_string(self) -> string:
        self.name

trait Named:
    fn name(self) -> string

trait Shown < Named:
    fn show(self) -> string

trait Tagged < Named:
    fn tag(self) -> string

fn same[T](left: T, right: T) -> bool:
    true
";
    for body in [
        "fn f(u: User, d: dyn Display) -> bool:\n    same(u, d)\n",
        "fn f(u: User, d: dyn Display) -> bool:\n    same(d, u)\n",
        "fn f(s: dyn Shown, t: dyn Tagged) -> bool:\n    same(s, t)\n",
        "fn f(s: dyn Shown, n: dyn Named) -> bool:\n    same(n, s)\n",
        "fn f(u: User, d: dyn Display) -> bool:\n    same(left=d, right=u)\n",
    ] {
        let main = format!("{decls}\n{body}");
        assert_eq!(codes_of(&main), vec![Code::NoCommonType], "{body}");
    }
    let main = format!("{decls}\nfn f() -> bool:\n    same(1, true)\n");
    assert_eq!(codes_of(&main), vec![Code::TypeMismatch]);
}

/// Values with common types that the least common type cannot reach,
/// because one would need weakening and a variance step together
/// (`types.lct.no-combine`, `types.lct.no-least`); and an invariant
/// argument that differs in permission, which has no common type.
#[test]
fn weakening_with_variance_has_no_least_common_type() {
    let decls = "\
data User:
    name: string

data Box[T]:
    value: T
";
    for body in [
        "fn f(small: mut List[mut User], wide: List[User]) -> void:\n    _v := [small, wide]\n",
        "fn f(a: mut List[mut User], b: List[mut User], c: List[User]) -> void:\n    _v := [a, b, c]\n",
    ] {
        let main = format!("{decls}\n{body}");
        assert_eq!(codes_of(&main), vec![Code::NoLeastCommonType], "{body}");
    }
    let main =
        format!("{decls}\nfn f(a: Box[mut User], b: Box[User]) -> void:\n    _v := [a, b]\n");
    assert_eq!(codes_of(&main), vec![Code::NoCommonType]);
}

/// The value the least common type gives a mutable and a readonly value
/// is the readonly view, so an element cannot be mutated through it.
#[test]
fn a_weakened_join_is_readonly() {
    let main = "\
data User:
    name: string

fn f(editable: mut User, shared: User) -> void:
    both := [editable, shared]
    both[0].name = \"x\"
";
    assert_eq!(codes_of(main), vec![Code::ReadonlyRoot]);
}

/// A site reports once, at the first value that does not join; function
/// values join to their union row only at the site's level
/// (`req.row.union.literal.direct`, `req.row.union.literal.other-parts`).
#[test]
fn a_site_reports_once_and_rows_join_only_at_the_top() {
    let main = "\
fn f(c: bool, k: i32) -> void:
    _v := if c:
        1
    else if k > 0:
        \"a\"
    else:
        true
";
    assert_eq!(codes_of(main), vec![Code::NoCommonType]);
    let rows = "\
trait Db

trait Clock

fn health() -> string $ Clock:
    \"ok\"

fn orders() -> string $ Db:
    \"orders\"

fn count() -> i32 $ Clock:
    1
";
    for body in [
        "fn f() -> void:\n    _v := [[health], [orders]]\n",
        "fn f(c: bool) -> void:\n    _v := if c: [orders] else: [health]\n",
        "fn f() -> void:\n    _v := [orders, count]\n",
    ] {
        let main = format!("{rows}\n{body}");
        assert_eq!(codes_of(&main), vec![Code::NoCommonType], "{body}");
    }
    let main = format!("{rows}\nfn f() -> void:\n    _v := [health, orders]\n");
    assert_eq!(codes_of(&main), vec![]);
}

/// A value that leaves a site's type unchanged costs a constant, so a
/// 20000-element list literal checks in linear time. The bound is generous
/// for the dev profile: it catches a quadratic join, not a slow machine.
#[test]
fn a_long_list_literal_joins_in_linear_time() {
    let elems: Vec<String> = (0..20_000).map(|i| format!("+{i}")).collect();
    let main = format!(
        "fn f() -> usize:\n    xs := [{}]\n    xs.len()\n",
        elems.join(", ")
    );
    let start = std::time::Instant::now();
    assert_eq!(codes_of(&main), vec![]);
    let took = start.elapsed();
    assert!(
        took < std::time::Duration::from_secs(5),
        "20000 elements took {took:?}"
    );
}
