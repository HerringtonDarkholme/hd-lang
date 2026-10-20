//! Maps run (#124, `crates/hd_mono/DESIGN.md` §13.12): iteration in entry order, removal
//! by tombstone, growth that keeps the order, keys hashed and compared
//! through their own `Hash` and `Eq`, and the iterator's invalidation
//! check. Programs build through the driver and run on V8
//! (`host/run.mjs`), as the conformance runner does.

use std::path::Path;
use std::process::{Command, Output as Ran};

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// Builds `main` as a program and runs it on V8.
fn ran(name: &str, main: &str) -> Ran {
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
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), &format!("map-run-{name}.wasm"));
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    ran
}

/// The standard output of a program that must succeed.
fn output_of(name: &str, main: &str) -> String {
    let r = ran(name, main);
    assert!(
        r.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    String::from_utf8(r.stdout).expect("UTF-8")
}

#[test]
fn a_for_loop_and_keys_and_values_follow_entry_order() {
    // A later duplicate key replaces the value in the first entry's place
    // (expr.map.duplicate.last, types.map.replace.in-place).
    let main = "\
pub fn main() -> void $ Console:
    let mut m: Map[string, i32] = {\"b\": 1, \"a\": 2, \"b\": 3, \"c\": 4}
    m[\"a\"] = 20
    m[\"d\"] = 5
    for (k, v) in m:
        println(\"${k}=${v}\")
    let line = \"\"
    for k in m.keys():
        line = line + k
    println(line)
    let sum = +0
    for v in m.values():
        sum = sum * 100 + v
    println(sum)
    println(m.len())
";
    assert_eq!(
        output_of("order", main),
        "b=3\na=20\nc=4\nd=5\nbacd\n3200405\n4\n"
    );
}

#[test]
fn removal_keeps_the_survivors_order_and_reinsertion_goes_last() {
    let main = "\
pub fn main() -> void $ Console:
    let mut m: Map[i32, string] = {1: \"one\", 2: \"two\", 3: \"three\"}
    println(m.remove(2) == .Some(\"two\"))
    println(m.remove(2).is_some())
    println(m.get(2).is_some())
    m[2] = \"again\"
    for (k, v) in m:
        println(\"${k}=${v}\")
    let mut big: Map[i64, i64] = {}
    for i in 0..1000:
        big[i64(i)] = i64(i) * 3
    for i in 0..1000:
        if i % 4 != 0:
            _ := big.remove(i64(i))
    for i in 1000..1100:
        big[i64(i)] = 0
    println(big.len())
    let last: i64 = -1
    let ordered = true
    for (k, _) in big:
        if k <= last:
            ordered = false
        last = k
    println(ordered)
    println(big[996])
    println(big.get(997).is_some())
";
    assert_eq!(
        output_of("remove", main),
        "true\nfalse\nfalse\n1=one\n3=three\n2=again\n350\ntrue\n2988\nfalse\n"
    );
}

#[test]
fn removing_during_a_loop_leaves_the_loop_on_its_snapshot() {
    // flow.for.snapshot.change: the loop visits the entries it began with.
    let main = "\
pub fn main() -> void $ Console:
    let mut m: Map[string, i32] = {\"a\": 1, \"b\": 2, \"c\": 3}
    for (k, _) in m:
        println(k)
        _ := m.remove(\"c\")
    println(m.len())
";
    assert_eq!(output_of("remove-during", main), "a\nb\nc\n2\n");
}

#[test]
fn a_removal_and_an_insertion_in_one_step_leave_the_loop_on_its_snapshot() {
    let main = "\
pub fn main() -> void $ Console:
    let mut m: Map[string, i32] = {\"a\": 1, \"b\": 2, \"c\": 3}
    for (k, _) in m:
        if k == \"a\":
            _ := m.remove(\"c\")
            m[\"d\"] = 4
        println(k)
    println(m.keys().len())
";
    assert_eq!(output_of("swap-during", main), "a\nb\nc\n3\n");
}

#[test]
fn a_growth_that_doubles_during_a_loop_leaves_it_on_its_snapshot() {
    // The four-entry literal is full, so the insertion doubles the entry
    // arrays; the loop goes on over the arrays it began with.
    let main = "\
pub fn main() -> void $ Console:
    let mut m: Map[string, i32] = {\"a\": 1, \"b\": 2, \"c\": 3, \"d\": 4}
    for (k, _) in m:
        println(k)
        _ := m.remove(\"c\")
        m[\"e\"] = 5
    println(m.len())
";
    assert_eq!(output_of("double-during", main), "a\nb\nc\nd\n4\n");
}

#[test]
fn a_growth_that_doubles_during_iteration_invalidates_the_cursor() {
    // flow.for.invalidate.map-rebuild: the four-entry literal is full, so
    // the insertion doubles the entry arrays and moves the entries; the
    // live count is restored, but the cursor's next call panics instead
    // of resuming from a moved slot.
    let main = "\
pub fn main() -> void $ Console:
    let mut m: Map[string, i32] = {\"a\": 1, \"b\": 2, \"c\": 3, \"d\": 4}
    let it: &mut dyn Iterator[(string, i32)] = m.iter()
    match it.next():
        .Some((k, _)) => println(k)
        .None => println(\"none\")
    _ := m.remove(\"c\")
    m[\"e\"] = 5
    _ := it.next()
";
    let r = ran("double-cursor", main);
    assert!(!r.status.success(), "the cursor must panic");
    assert_eq!(String::from_utf8_lossy(&r.stdout), "a\n");
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.contains("iterator-invalidated"), "{err}");
}

#[test]
fn a_compaction_in_place_during_iteration_invalidates_the_cursor() {
    // flow.for.invalidate.map-rebuild: fewer than half of the eight slots
    // are live, so the rebuild keeps the capacity and only compacts; the
    // cursor's next call panics all the same.
    let main = "\
pub fn main() -> void $ Console:
    let mut m: Map[string, i32] = {\"a\": 1, \"b\": 2, \"c\": 3, \"d\": 4, \"e\": 5, \"f\": 6, \"g\": 7, \"h\": 8}
    for k in [\"a\", \"b\", \"c\", \"d\", \"e\"]:
        _ := m.remove(k)
    let it: &mut dyn Iterator[(string, i32)] = m.iter()
    match it.next():
        .Some((k, _)) => println(k)
        .None => println(\"none\")
    _ := m.remove(\"f\")
    m[\"i\"] = 9
    println(m.len())
    _ := it.next()
";
    let r = ran("compact-during", main);
    assert!(!r.status.success(), "the cursor must panic");
    assert_eq!(String::from_utf8_lossy(&r.stdout), "f\n3\n");
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(err.contains("iterator-invalidated"), "{err}");
}

#[test]
fn a_data_key_hashes_and_compares_through_its_derived_impls() {
    let main = "\
@derive(Eq, Hash)
struct Point:
    x: i32
    label: string

pub fn main() -> void $ Console:
    let mut m: Map[Point, i32] = {}
    for i in 0..50:
        m[Point { x: i % 10, label: \"p\" }] = i
    println(m.len())
    println(m[Point { x: 3, label: \"p\" }])
    println(m.get(Point { x: 3, label: \"q\" }).is_some())
    println(m.remove(Point { x: 4, label: \"p\" }) == .Some(44))
    println(m.len())
    let xs = \"\"
    for (p, _) in m:
        xs = xs + \"${p.x}\"
    println(xs)
";
    assert_eq!(
        output_of("data-key", main),
        "10\n43\nfalse\ntrue\n9\n012356789\n"
    );
}

#[test]
fn derived_eq_and_debug_on_an_enum_holding_a_map() {
    let main = "\
@derive(Eq, Debug)
enum Shelf:
    Empty
    Stock(items: Map[string, i32])

pub fn main() -> void $ Console:
    a := Shelf.Stock({\"x\": 1, \"y\": 2})
    println(a == Shelf.Stock({\"y\": 2, \"x\": 1}))
    println(a == Shelf.Stock({\"x\": 1}))
    println(a == Shelf.Empty)
    println(debug(a))
    println(debug(Shelf.Empty))
";
    assert_eq!(
        output_of("enum-map", main),
        "true\nfalse\nfalse\nShelf.Stock(items={\"x\": 1, \"y\": 2})\nShelf.Empty\n"
    );
}

#[test]
fn every_map_method_takes_its_declared_parameter_names() {
    // module.method.normative: `get(key)`, `remove(key)`, and the std
    // methods `contains_key(key)` and `get_or(key, fallback)` take named
    // arguments; so do the list built-ins `push(value)`.
    let main = "\
pub fn main() -> void $ Console:
    let mut m: Map[string, i32] = {\"a\": 1, \"b\": 2}
    println(m.get(key=\"a\").is_some())
    println(m.contains_key(key=\"b\"))
    println(m.get_or(fallback=9, key=\"z\"))
    println(m.remove(key=\"a\").is_some())
    println(m.remove(key=\"a\").is_some())
    println(m.len())
    let mut l: List[i32] = []
    l.push(value=7)
    l.insert(value=8, index=0)
    println(l.remove_at(index=1))
    println(l.len())
";
    assert_eq!(
        output_of("named-args", main),
        "true\ntrue\n9\ntrue\nfalse\n1\n7\n1\n"
    );
}

#[test]
fn a_misnamed_map_argument_is_rejected() {
    let main = "\
pub fn main() -> void:
    let mut m: Map[string, i32] = {}
    _ := m.remove(value=\"a\")
";
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
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(
        out.render().contains("unknown-named-argument"),
        "{}",
        out.render()
    );
}

/// Keys that collide under the fixed FNV-1a-32 the map once bucketed
/// strings by (#353): each key picks one 6-letter block of each pair, and
/// both blocks of a pair lead from the same FNV state to the same state, so
/// all 2^16 keys had one bucket hash and every insertion probed all the
/// earlier ones, about 4 * 10^9 key comparisons. Keyed by the run's seed
/// (`std.hash.string_bucket_hash`), they spread, and the program ends in
/// well under the bound; the old hash took over ten seconds.
#[test]
fn keys_chosen_to_collide_under_a_fixed_hash_stay_fast() {
    let main = "\
PAIRS := [(\"etyryl\", \"mjidmt\"), (\"ufefyz\", \"ibubif\"), (\"olixgb\", \"gryfkj\"), (\"gdcnyr\", \"shsbin\"), (\"wjuped\", \"arsrez\"), (\"encval\", \"qbsjqp\"), (\"slinwp\", \"wjsbuf\"), (\"ohapah\", \"slqlqt\"), (\"apynmn\", \"yjityv\"), (\"cncvaj\", \"wbsjqv\"), (\"krsraz\", \"choder\"), (\"ctchqd\", \"whstax\"), (\"cvifmj\", \"wryzuv\"), (\"wvyfyh\", \"cjibit\"), (\"yrylsh\", \"mvihct\"), (\"wvwbyv\", \"czgfij\")]

pub fn main() -> void $ Console:
    let mut m: Map[string, usize] = {}
    let mask: usize = 0
    while mask < 65536:
        let mut parts: List[string] = []
        let b: usize = 0
        while b < 16:
            pair := PAIRS[b]
            parts.push(if (mask >> b) & 1 == 1: pair._1 else: pair._0)
            b = b + 1
        m[parts.join(\"\")] = mask
        mask = mask + 1
    println(m.len())
    println(m.get(\"etyrylufefyzolixgbgdcnyrwjupedencvalslinwpohapahapynmncncvajkrsrazctchqdcvifmjwvyfyhyrylshwvwbyv\").unwrap_or(1))
    println(m.get(\"mjidmtibubifgryfkjshsbinarsrezqbsjqpwjsbufslqlqtyjityvwbsjqvchoderwhstaxwryzuvcjibitmvihctczgfij\").unwrap_or(1))
";
    let start = std::time::Instant::now();
    assert_eq!(output_of("collide", main), "65536\n0\n65535\n");
    let took = start.elapsed();
    assert!(
        took < std::time::Duration::from_secs(8),
        "65,536 colliding keys took {took:?}"
    );
}

/// Integer keys that collided under the fixed mix the map once bucketed
/// integers by (#511): `x * 0x9E3779B1`, then `h ^ (h >> 16)`. Key `a`
/// is `p * 0x9E3779B1^-1` for `p = (a << 16) | a`, so its old hash was
/// `a << 16`, and all 65,536 keys fell into two runs of the index; as a
/// `u64` the key sits in the high half, which the old mix folded onto the
/// low one. Each insertion and lookup then probed half the earlier keys,
/// about 4 * 10^9 probes over the two maps. Keyed by the run's seed
/// (`std.hash.int_bucket_hash`), they spread.
#[test]
fn integer_keys_chosen_to_collide_under_a_fixed_mix_stay_fast() {
    let main = "\
pub fn main() -> void $ Console:
    let mut narrow: Map[u32, u64] = {}
    let mut wide: Map[u64, u64] = {}
    let a: u64 = 0
    while a < 65536:
        key := ((a << 16) | a) * 244002641 & 4294967295
        narrow[u32(key)] = a
        wide[key << 32] = a
        a = a + 1
    let found: u64 = 0
    a = 0
    while a < 65536:
        key := ((a << 16) | a) * 244002641 & 4294967295
        if narrow.get(u32(key)).unwrap_or(0) == a && wide.get(key << 32).unwrap_or(0) == a:
            found = found + 1
        a = a + 1
    println(narrow.len())
    println(wide.len())
    println(found)
";
    let start = std::time::Instant::now();
    assert_eq!(output_of("collide-int", main), "65536\n65536\n65536\n");
    let took = start.elapsed();
    assert!(
        took < std::time::Duration::from_secs(8),
        "65,536 colliding integer keys took {took:?}"
    );
}

/// Composite keys whose `Hash` bytes collide under the unkeyed FNV-1a-64
/// `hash_of` the map once bucketed them by (#511): each name picks one
/// 11-letter block of each pair, and both blocks of a pair lead from the
/// same 64-bit FNV state to the same state (found by Pollard's rho), so
/// all 2^15 tuples `(name, 7)`, and the derived `Entry` values that write
/// the same bytes, had one hash. Every insertion then compared the key
/// with all the earlier ones, about 10^9 `eq` calls over the two maps.
/// Keyed by the run's seed (`std.hash.key_bucket_hash`), they spread.
#[test]
fn composite_keys_chosen_to_collide_under_hash_of_stay_fast() {
    let main = "\
PAIRS := [(\"xLFsLJjiINP\", \"7Gg3RxEK0PE\"), (\"JdU9jHX4TqJ\", \"OM0YyM0u85J\"), (\"AugSUBgW75L\", \"cnjpGuWGzZD\"), (\"m5sdyq21BBG\", \"_giiglMbqlL\"), (\"4JKsnCMsoUG\", \"Gq8caPsJwuE\"), (\"ggLC_EYaqEH\", \"1OD6Nfm_0eN\"), (\"8m63a_auFPM\", \"wpZBVqlGciB\"), (\"U5oBHizR2DJ\", \"d7xyPoQOXFI\"), (\"NlD.hxb.YLB\", \"J.FHPO4huuF\"), (\"rdo2YOCChAO\", \"jnkEdU0xbkA\"), (\"SeahsXPklTG\", \".RY30hrErLK\"), (\"EXcYViwLM3N\", \"mk0qPgfDxJC\"), (\"fONkr.v1G5E\", \"0Wh3F4JFuQH\"), (\"qdyN48b8gHM\", \"UQManoGiTKL\"), (\"waw.LRB_iqI\", \"A34Z3EbN1.F\")]

@derive(Eq, Hash)
struct Entry:
    name: string
    n: i32

fn name_of(mask: usize) -> string:
    let mut parts: List[string] = []
    let b: usize = 0
    while b < 15:
        pair := PAIRS[b]
        parts.push(if (mask >> b) & 1 == 1: pair._1 else: pair._0)
        b = b + 1
    parts.join(\"\")

pub fn main() -> void $ Console:
    let mut pairs: Map[(string, i32), usize] = {}
    let mut entries: Map[Entry, usize] = {}
    let mask: usize = 0
    while mask < 32768:
        name := name_of(mask)
        pairs[(name, 7)] = mask
        entries[Entry { name: name, n: 7 }] = mask
        mask = mask + 1
    println(pairs.len())
    println(entries.len())
    println(pairs.get((\"xLFsLJjiINPJdU9jHX4TqJAugSUBgW75Lm5sdyq21BBG4JKsnCMsoUGggLC_EYaqEH8m63a_auFPMU5oBHizR2DJNlD.hxb.YLBrdo2YOCChAOSeahsXPklTGEXcYViwLM3NfONkr.v1G5EqdyN48b8gHMwaw.LRB_iqI\", 7)).unwrap_or(1))
    println(entries.get(Entry { name: \"7Gg3RxEK0PEOM0YyM0u85JcnjpGuWGzZD_giiglMbqlLGq8caPsJwuE1OD6Nfm_0eNwpZBVqlGciBd7xyPoQOXFIJ.FHPO4huuFjnkEdU0xbkA.RY30hrErLKmk0qPgfDxJC0Wh3F4JFuQHUQManoGiTKLA34Z3EbN1.F\", n: 7 }).unwrap_or(1))
";
    let start = std::time::Instant::now();
    assert_eq!(
        output_of("collide-composite", main),
        "32768\n32768\n0\n32767\n"
    );
    let took = start.elapsed();
    assert!(
        took < std::time::Duration::from_secs(8),
        "32,768 colliding composite keys took {took:?}"
    );
}
