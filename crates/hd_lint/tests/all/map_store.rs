//! `map-get-store-back`: each reported shape, its fix-it's exact text, a
//! check of the fixed program, and the shapes that stay silent.

use hd_base::SliceSort as _;
use hd_cache::MemoryStore;
use hd_diag::{Code, DiagBuf, FixSafety};
use hd_driver::{Executor, Goal, Host, NoClock, Packages, build_packages_with_analysis};
use hd_lint::{ANALYZER, Policy};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn lint(source: &str) -> DiagBuf {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let output = build_packages_with_analysis(
        &host,
        "app",
        &Packages::default(),
        &Goal::Analyze,
        Some(&ANALYZER),
    );
    assert!(!output.diags.has_errors(), "{}\n{source}", output.render());
    Policy::default().apply(&output.analysis_diags)
}

/// The rule's findings: each message, and the source with every fix-it
/// applied.
fn findings(source: &str) -> (Vec<String>, String) {
    let all = lint(source);
    let mut messages = Vec::new();
    let mut edits = Vec::new();
    for row in 0..all.len() {
        if all.code[row] != Code::MapGetStoreBack {
            continue;
        }
        messages.push(all.get_text(all.message[row]).to_owned());
        assert_eq!(all.fixes[row].len, 1);
        let fix = all.fixes[row].start as usize;
        assert_eq!(all.fix_safety[fix], FixSafety::Exact);
        let range = all.fix_edits[fix];
        for edit in &all.edits[range.range()] {
            edits.push((edit.span.lo, edit.span.hi, all.get_text(edit.text)));
        }
    }
    edits.sort_stable_by_key(|&(lo, ..)| std::cmp::Reverse(lo));
    let mut fixed = source.to_owned();
    for (lo, hi, text) in edits {
        fixed.replace_range(lo as usize..hi as usize, text);
    }
    (messages, fixed)
}

/// The rule reports `count` findings, its fix-its rewrite the source to
/// `expected`, and the rewritten program checks with no finding left.
fn reports(source: &str, count: usize, expected: &str) {
    let (messages, fixed) = findings(source);
    assert_eq!(messages.len(), count, "{messages:?}\n{source}");
    assert_eq!(fixed, expected);
    let (left, _) = findings(&fixed);
    assert!(left.is_empty(), "{left:?}\n{fixed}");
}

fn silent(source: &str) {
    let (messages, _) = findings(source);
    assert!(messages.is_empty(), "{messages:?}\n{source}");
}

#[test]
fn unwrap_or_with_one_change_is_rewritten_to_a_change_in_place() {
    reports(
        "
fn group(n: i32) -> Map[i32, List[i32]]:
    let mut m: Map[i32, List[i32]] = {}
    for i in 0..n:
        let mut xs = m.get(0).unwrap_or([])
        xs.push(i)
        m[0] = xs
    m
",
        1,
        "
fn group(n: i32) -> Map[i32, List[i32]]:
    let mut m: Map[i32, List[i32]] = {}
    for i in 0..n:
        m.get_or_insert(0, []).push(i)
    m
",
    );
}

#[test]
fn the_message_names_the_copy_the_store_and_the_call() {
    let (messages, _) = findings(
        "
fn group(n: i32, m: &mut Map[string, List[i32]]) -> void:
    for i in 0..n:
        let mut xs = m.get(\"k\").unwrap_or([])
        xs.push(i)
        m[\"k\"] = xs
",
    );
    assert_eq!(
        messages,
        [
            "`m.get(\"k\")` copies a value that the map still holds, so changing `xs` copies the whole value before `m[\"k\"] = xs` stores it back; change it in place with `m.get_or_insert(\"k\", [])`"
        ]
    );
}

#[test]
fn a_match_on_get_in_either_arm_order_is_reported() {
    reports(
        "
fn group(n: i32, key: i32) -> Map[i32, List[i32]]:
    let mut m: Map[i32, List[i32]] = {}
    for i in 0..n:
        let mut ys = match m.get(key):
            .Some(v) => v
            .None => []
        ys.push(i)
        ys.push(i + 1)
        m[key] = ys
    for i in 0..n:
        let mut zs: List[i32] = match m.get(1):
            .None => [7]
            .Some(found) => found
        zs[0] = i
        m[1] = zs
    for i in 0..n:
        let mut ws = match m.get(2):
            .Some(w) => w
            _ => []
        ws.push(i)
        m[2] = ws
    m
",
        3,
        "
fn group(n: i32, key: i32) -> Map[i32, List[i32]]:
    let mut m: Map[i32, List[i32]] = {}
    for i in 0..n:
        ys := m.get_or_insert(key, [])
        ys.push(i)
        ys.push(i + 1)
    for i in 0..n:
        m.get_or_insert(1, [7])[0] = i
    for i in 0..n:
        m.get_or_insert(2, []).push(i)
    m
",
    );
}

#[test]
fn struct_values_fields_and_a_field_map_are_rewritten() {
    reports(
        "
struct Tally:
    count: i32
    names: List[string]

struct Book:
    tallies: Map[string, Tally]

impl Book:
    fn note(&mut self, name: string) -> void:
        let mut tally = self.tallies.get(name).unwrap_or(Tally { count: 0, names: [] })
        tally.count += 1
        # the name joins the list
        tally.names.push(\"seen\")
        self.tallies[name] = tally

fn count(words: List[string]) -> Map[string, Tally]:
    let mut t: Map[string, Tally] = {}
    for word in words:
        let mut tally = t.get(word).unwrap_or(Tally { count: 0, names: [] })
        tally.count += 1
        t[word] = tally
    t
",
        2,
        "
struct Tally:
    count: i32
    names: List[string]

struct Book:
    tallies: Map[string, Tally]

impl Book:
    fn note(&mut self, name: string) -> void:
        tally := self.tallies.get_or_insert(name, Tally { count: 0, names: [] })
        tally.count += 1
        # the name joins the list
        tally.names.push(\"seen\")

fn count(words: List[string]) -> Map[string, Tally]:
    let mut t: Map[string, Tally] = {}
    for word in words:
        t.get_or_insert(word, Tally { count: 0, names: [] }).count += 1
    t
",
    );
}

#[test]
fn crlf_lines_keep_their_endings() {
    let source = "fn group(n: i32) -> Map[i32, List[i32]]:\r\n    let mut m: Map[i32, List[i32]] = {}\r\n    for i in 0..n:\r\n        let mut xs = m.get(0).unwrap_or([])\r\n        xs.push(i)\r\n        xs.push(i)\r\n        m[0] = xs\r\n    for i in 0..n:\r\n        let mut ys = m.get(1).unwrap_or([])\r\n        ys.push(i)\r\n        m[1] = ys\r\n    m\r\n";
    reports(
        source,
        2,
        "fn group(n: i32) -> Map[i32, List[i32]]:\r\n    let mut m: Map[i32, List[i32]] = {}\r\n    for i in 0..n:\r\n        xs := m.get_or_insert(0, [])\r\n        xs.push(i)\r\n        xs.push(i)\r\n    for i in 0..n:\r\n        m.get_or_insert(1, []).push(i)\r\n    m\r\n",
    );
}

#[test]
fn interleaved_maps_and_a_reused_name_each_report_their_own_store() {
    reports(
        "
fn group(n: i32) -> Map[i32, List[i32]]:
    let mut a: Map[i32, List[i32]] = {}
    let mut b: Map[i32, List[i32]] = {}
    for i in 0..n:
        let mut xs = a.get(0).unwrap_or([])
        xs.push(i)
        a[0] = xs
        let mut xs = b.get(0).unwrap_or([])
        xs.push(i)
        b[0] = xs
    a
",
        2,
        "
fn group(n: i32) -> Map[i32, List[i32]]:
    let mut a: Map[i32, List[i32]] = {}
    let mut b: Map[i32, List[i32]] = {}
    for i in 0..n:
        a.get_or_insert(0, []).push(i)
        b.get_or_insert(0, []).push(i)
    a
",
    );
}

/// A generic body, a closure body and a nested block each report once.
#[test]
fn each_body_form_reports_once() {
    reports(
        "
fn add[K < Eq & Hash](m: &mut Map[K, List[i32]], key: K, n: i32) -> void:
    let mut xs = m.get(key).unwrap_or([])
    xs.push(n)
    m[key] = xs

fn group(n: i32) -> Map[i32, List[i32]]:
    let mut m: Map[i32, List[i32]] = {}
    add(&mut m, 1, 2)
    add(&mut m, 3, 4)
    note := fn(key: i32): add(&mut m, key, 0)
    note(5)
    if n > 0:
        let mut ys = m.get(n).unwrap_or([])
        ys.push(1)
        m[n] = ys
    m
",
        2,
        "
fn add[K < Eq & Hash](m: &mut Map[K, List[i32]], key: K, n: i32) -> void:
    m.get_or_insert(key, []).push(n)

fn group(n: i32) -> Map[i32, List[i32]]:
    let mut m: Map[i32, List[i32]] = {}
    add(&mut m, 1, 2)
    add(&mut m, 3, 4)
    note := fn(key: i32): add(&mut m, key, 0)
    note(5)
    if n > 0:
        m.get_or_insert(n, []).push(1)
    m
",
    );
}

#[test]
fn a_read_of_the_old_value_after_the_store_is_silent() {
    silent(
        "
fn group(n: i32) -> usize:
    let mut m: Map[i32, List[i32]] = {}
    let mut total: usize = 0
    for i in 0..n:
        let mut xs = m.get(0).unwrap_or([])
        xs.push(i)
        m[0] = xs
        total = total + xs.len()
    total
",
    );
}

#[test]
fn a_change_that_reads_the_map_or_moves_the_key_is_silent() {
    silent(
        "
fn group(n: i32) -> Map[i32, List[i32]]:
    let mut m: Map[i32, List[i32]] = {}
    let mut key = 0
    for i in 0..n:
        let mut xs = m.get(0).unwrap_or([])
        size := m.len()
        xs.push(i)
        m[0] = xs
    for i in 0..n:
        let mut ys = m.get(key).unwrap_or([])
        key = key + 1
        ys.push(i)
        m[key] = ys
    for i in 0..n:
        let mut zs = m.get(key).unwrap_or([])
        zs.push(i)
        m[key + 1] = zs
    for i in 0..n:
        let mut ws = m.get(1).unwrap_or(m.get(2).unwrap_or([]))
        ws.push(i)
        m[1] = ws
    m
",
    );
}

#[test]
fn an_early_exit_closure_or_other_use_between_is_silent() {
    silent(
        "
fn keep(xs: List[i32]) -> void:
    _ := xs

fn group(n: i32) -> Map[i32, List[i32]]?:
    let mut m: Map[i32, List[i32]] = {}
    let limits: Map[i32, i32] = {}
    for i in 0..n:
        let mut xs = m.get(0).unwrap_or([])
        if i > 3:
            break
        xs.push(i)
        m[0] = xs
    for i in 0..n:
        let mut xs = m.get(1).unwrap_or([])
        xs.push(limits.get(i)?)
        m[1] = xs
    for i in 0..n:
        let mut xs = m.get(2).unwrap_or([])
        xs.push(i)
        keep(xs)
        m[2] = xs
    for i in 0..n:
        let mut xs = m.get(3).unwrap_or([])
        add := fn(x: i32): x + 1
        xs.push(add(i))
        m[3] = xs
    for i in 0..n:
        let mut xs = m.get(4).unwrap_or([])
        xs = [i]
        m[4] = xs
    .Some(m)
",
    );
}

#[test]
fn other_matches_on_get_are_silent() {
    silent(
        "
fn group(n: i32) -> Map[i32, List[i32]]:
    let mut m: Map[i32, List[i32]] = {}
    for i in 0..n:
        let mut xs = match m.get(0):
            .Some(v) if v.len() > 2 => v
            .Some(v) => v
            .None => []
        xs.push(i)
        m[0] = xs
    for i in 0..n:
        let mut xs = match m.get(1):
            .Some(v) => [i]
            .None => []
        xs.push(i)
        m[1] = xs
    for i in 0..n:
        let mut xs = match m.get(2):
            .Some(_) => []
            .None => []
        xs.push(i)
        m[2] = xs
    for i in 0..n:
        let mut xs = match m.get(3):
            .None => []
            found => found.unwrap_or([])
        xs.push(i)
        m[3] = xs
    for i in 0..n:
        let mut xs = match m.get(4):
            .Some(v) => v
            .None => return m
        xs.push(i)
        m[4] = xs
    for i in 0..n:
        let mut xs = match m.get(5):
            .Some(v) => v
            .None =>
                fresh := [i]
                fresh
        xs.push(i)
        m[5] = xs
    m
",
    );
}

/// A computed key may run twice in the original and once in the fix, so
/// only a name or a literal key is rewritten.
#[test]
fn a_computed_key_is_silent() {
    silent(
        "
fn group(n: i32) -> Map[i32, List[i32]]:
    let mut m: Map[i32, List[i32]] = {}
    for i in 0..n:
        let mut xs = m.get(i % 4).unwrap_or([])
        xs.push(i)
        m[i % 4] = xs
    m
",
    );
}

#[test]
fn shapes_without_a_map_copy_are_silent() {
    silent(
        "
fn group(n: i32, mut lists: List[List[i32]], mut maps: List[Map[i32, List[i32]]]) -> Map[i32, List[i32]]:
    let mut m: Map[i32, List[i32]] = {}
    for i in 0..n:
        let mut xs = maps[0].get(0).unwrap_or([])
        xs.push(i)
        maps[0][0] = xs
    for i in 0..n:
        let mut xs = lists.get(0).unwrap_or([])
        xs.push(i)
        lists[0] = xs
    for i in 0..n:
        let mut xs = m.remove(0).unwrap_or([])
        xs.push(i)
        m[0] = xs
    for i in 0..n:
        xs := m.get(0).unwrap_or([])
        m[0] = xs
    for i in 0..n:
        let mut xs = m.get(0).unwrap_or([])
        m[0] = xs
        xs.push(i)
    let mut outer: List[i32] = []
    for i in 0..n:
        outer = m.get(5).unwrap_or([])
        outer.push(i)
        m[5] = outer
    m
",
    );
}
