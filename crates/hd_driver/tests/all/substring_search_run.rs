//! Substring search (std-text.more.find, std-text.more.contains,
//! std-text.count, std-text.string.split, std-text.string.replace,
//! std-text.split-once): every substring operation in std.text agrees with
//! a naive byte-by-byte reference on random haystacks and needles over
//! ASCII and multi-byte UTF-8 alphabets, needles cut from the haystack,
//! needles longer than the haystack, empty needles, and periodic needles
//! that defeat a shift table (#352, the Two-Way searcher in
//! `lib/std/text.hd`). The program builds through the driver and runs on V8
//! (`host/run.mjs`).

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The standard output of a `main` program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
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
    let path = Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), &format!("{name}.wasm"));
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

/// The program: fixed adversarial cases, then 3,000 seeded random ones.
const PROGRAM: &str = r#"# Substring operations against a naive reference: random haystacks and
# needles over small alphabets (ASCII and multi-byte UTF-8), needles cut
# from the haystack, and adversarial periodic cases.

fn naive_find(text: string, needle: string, from: usize) -> usize?:
    hay := text.to_utf8()
    pin := needle.to_utf8()
    let start = from
    while start + pin.len() <= hay.len():
        let index: usize = 0
        while index < pin.len() && hay[start + index] == pin[index]:
            index = index + 1
        if index == pin.len():
            return .Some(start)
        start = start + 1
    .None

fn naive_count(text: string, needle: string) -> usize:
    if needle.len() == 0:
        return text.chars().count() + 1
    let found: usize = 0
    let start: usize = 0
    while true:
        match naive_find(text, needle, start):
            .Some(at) =>
                found = found + 1
                start = at + needle.len()
            .None => break
    found

fn naive_split(text: string, separator: string) -> List[string]:
    let pieces: mut List[string] = []
    let start: usize = 0
    while true:
        match naive_find(text, separator, start):
            .Some(at) =>
                pieces.push(text[start..at])
                start = at + separator.len()
            .None => break
    pieces.push(text[start..])
    pieces

fn naive_replace(text: string, old: string, new: string) -> string:
    naive_split(text, old).join(new)

data Rng:
    state: u64

impl Rng:
    fn below(mut self, bound: usize) -> usize:
        self.state = (self.state * 1103515245 + 12345) % 2147483648
        usize((self.state / 65536) % u64(bound))

fn word(rng: mut Rng, alphabet: List[string], size: usize) -> string:
    let parts: mut List[string] = []
    for _ in 0..size:
        parts.push(alphabet[rng.below(alphabet.len())])
    parts.join("")

fn agree(text: string, needle: string, failures: mut List[string]) -> void:
    case := "${debug(text)} ${debug(needle)}"
    if text.find(needle) != naive_find(text, needle, 0):
        failures.push("find $case")
    if text.contains(needle) != naive_find(text, needle, 0).is_some():
        failures.push("contains $case")
    if text.count(needle) != naive_count(text, needle):
        failures.push("count $case")
    if needle.len() > 0:
        if text.split(needle) != naive_split(text, needle):
            failures.push("split $case")
        if text.replace(needle, "<>") != naive_replace(text, needle, "<>"):
            failures.push("replace $case")
    let once: (string, string)? = match naive_find(text, needle, 0):
        .Some(at) => .Some((text[..at], text[at + needle.len()..]))
        .None => .None
    if text.split_once(needle) != once:
        failures.push("split_once $case")

# The Fibonacci words, whose prefixes stress critical factorizations.
fn fibonacci(steps: usize) -> string:
    let a = "a"
    let b = "ab"
    for _ in 0..steps:
        next := b + a
        a = b
        b = next
    b

pub fn main() -> void $ Console:
    let failures: mut List[string] = []
    let checked: usize = 0
    fixed := [
        ("a".repeat(50) + "b", "aab"),
        ("a".repeat(3000) + "b", "a".repeat(1000) + "b"),
        (("ab".repeat(99) + "b").repeat(20) + "ab".repeat(100), "ab".repeat(100)),
        ("a".repeat(50) + "b", "a".repeat(10) + "b"),
        ("a".repeat(50), "a".repeat(10) + "b"),
        ("a".repeat(50), "b" + "a".repeat(10)),
        ("ab".repeat(30) + "c", "abababc"),
        ("ab".repeat(30), "abababc"),
        ("aaaa", "aa"),
        ("abc", "abcd"),
        ("", "a"),
        ("", ""),
        ("abc", ""),
        ("héllo wörld héllo", "llo"),
        ("héllo wörld héllo", "ö"),
        ("日本語日本語の日本", "日本"),
        ("日本語日本語の日本", "本語の"),
        ("aXbXcX", "X"),
        ("banana", "ana"),
        ("banana", "nan"),
        ("abcabcabd", "abcabd"),
        ("aabaabaaab", "aabaaab"),
        ("zzazzazzz", "zzz"),
        (fibonacci(8), fibonacci(5)),
        (fibonacci(8) + "c", fibonacci(5) + "c"),
        (fibonacci(9), "baab"),
    ]
    for (text, needle) in fixed:
        agree(text, needle, failures)
        checked = checked + 1
    alphabets := [["a", "b"], ["a", "b", "c"], ["a", "é", "ü"], ["x", "日", "本", "ab"], ["a"]]
    let rng: mut Rng = Rng { state: 7 }
    for round in 0..3000:
        alphabet := alphabets[round % alphabets.len()]
        text := word(rng, alphabet, rng.below(40))
        needle := if rng.below(2) == 0 && text.len() > 0:
            # Cut at scalar boundaries.
            chars := text.chars().collect::[List[char]]()
            from := rng.below(chars.len())
            to := from + rng.below(chars.len() - from + 1)
            [for c in chars[from..to] => "$c"].join("")
        else:
            word(rng, alphabet, rng.below(8))
        agree(text, needle, failures)
        checked = checked + 1
    for failure in failures:
        println("FAIL $failure")
    println("checked $checked failures ${failures.len()}")
"#;

#[test]
fn substring_operations_agree_with_a_naive_search() {
    let out = output_of("substring-search", PROGRAM);
    assert_eq!(out, "checked 3026 failures 0\n");
}
