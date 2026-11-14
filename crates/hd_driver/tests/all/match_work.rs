//! `flow.match.limit` and `lex.limit.file-size`: a match whose coverage
//! and reachability checks need more than `hd_base::limits::MATCH_WORK`
//! is one `match-too-complex` on the match, and a source file over
//! `hd_base::limits::FILE_SIZE` bytes is one `file-too-large`.

use hd_base::limits::{FILE_SIZE, MATCH_WORK};
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use std::fmt::Write as _;

/// A match over `(i32, i32, i32)` whose usefulness work is exactly
/// `work`, one plus the rows of each class the pass splits: `g` guarded
/// wildcard arms, `a` arms `(0, k, _)`, `b` arms `(0, k, 0)` and a final
/// `_` cost `(2g + 9)a + (3g + 11)b + 8g + 16`, since every class holds
/// the guarded rows. With `g + 2` not a multiple of 5 the two arm costs
/// are coprime, so some `a` and `b` give any work past their product.
fn match_of_work(work: u64) -> String {
    let (g, a, b) = (0..=700u64)
        .rev()
        .filter(|g| (g + 2) % 5 != 0)
        .find_map(|g| {
            let left = work.checked_sub(8 * g + 16)?;
            let (ca, cb) = (2 * g + 9, 3 * g + 11);
            (0..ca)
                .take_while(|b| cb * b <= left)
                .find(|b| (left - cb * b) % ca == 0)
                .map(|b| (g, (left - cb * b) / ca, b))
        })
        .expect("a match of this work");
    let mut s =
        String::from("fn pick(a: i32, b: i32, c: i32, g: bool) -> i32:\n    match (a, b, c):\n");
    for _ in 0..g {
        s.push_str("        _ if g => 0\n");
    }
    for k in 0..a {
        let _ = writeln!(s, "        (0, {k}, _) => {k}");
    }
    for k in a..a + b {
        let _ = writeln!(s, "        (0, {k}, 0) => {k}");
    }
    let _ = writeln!(s, "        _ => {}", a + b);
    s
}

fn codes(text: &str) -> Vec<String> {
    let mut src = MemorySources::default();
    src.insert("main.hd", text);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Pool(2),
    };
    let out = build(&host, "app", &Goal::Analyze);
    out.diags
        .code
        .iter()
        .map(|c| c.as_str().to_owned())
        .collect()
}

#[test]
fn a_match_at_the_work_limit_checks_and_one_step_more_is_too_complex() {
    let limit = u64::from(MATCH_WORK);
    assert_eq!(codes(&match_of_work(limit)), Vec::<String>::new());
    assert_eq!(codes(&match_of_work(limit + 1)), ["match-too-complex"]);
}

/// A match over a tuple of `bool`s whose arms encode a 3-SAT formula:
/// its usefulness work grows exponentially with the columns, and the
/// limit stops it as one diagnostic on the match, with no
/// `item-too-complex` from building its decision tree.
#[test]
fn an_exponential_match_is_too_complex_on_the_match() {
    let n = 30;
    let mut s = String::from("fn sat(t: (");
    s.push_str(&vec!["bool"; n].join(", "));
    s.push_str(")) -> i32:\n    match t:\n");
    // A fixed pseudo-random formula: 4.3 clauses per variable.
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = |m: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % m
    };
    for k in 0..(n * 43 / 10) {
        let mut cells = vec!["_"; n];
        for _ in 0..3 {
            let v = usize::try_from(next(n as u64)).unwrap();
            cells[v] = if next(2) == 0 { "true" } else { "false" };
        }
        let _ = writeln!(s, "        ({}) => {k}", cells.join(", "));
    }
    s.push_str("        _ => 0\n");
    assert_eq!(codes(&s), ["match-too-complex"]);
}

/// A file of `FILE_SIZE` bytes checks; one byte more is one
/// `file-too-large`.
#[test]
fn a_file_at_the_size_limit_checks_and_one_byte_more_is_too_large() {
    let limit = usize::try_from(FILE_SIZE).unwrap();
    let head = "fn main():\n    pass\n#";
    let mut at = String::with_capacity(limit + 1);
    at.push_str(head);
    at.extend(std::iter::repeat_n('x', limit - head.len()));
    assert_eq!(codes(&at), Vec::<String>::new());
    let past = format!("{at}x");
    assert_eq!(codes(&past), ["file-too-large"]);
}

/// The codes of `text` checked with a body fuel of `fuel`, under the
/// default `Fuel::BODY_DEFAULT`.
fn codes_with_fuel(text: &str, fuel: u64) -> Vec<String> {
    let mut src = MemorySources::default();
    src.insert("main.hd", text);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Pool(2),
    };
    let packages = hd_driver::Packages {
        body_fuel: Some(fuel),
        ..hd_driver::Packages::default()
    };
    let out = hd_driver::build_packages(&host, "app", &packages, &Goal::Analyze);
    out.diags
        .code
        .iter()
        .map(|c| c.as_str().to_owned())
        .collect()
}

/// A body whose fuel left is under `MATCH_WORK` stops at its fuel, as
/// `item-too-complex`, not `match-too-complex`: in a match's checks, and
/// in a `let` pattern's refutability question.
#[test]
fn a_match_or_pattern_past_the_body_fuel_is_item_too_complex() {
    let fuel = 20_000;
    assert_eq!(
        codes_with_fuel(&match_of_work(1_000), fuel),
        Vec::<String>::new()
    );
    assert_eq!(
        codes_with_fuel(&match_of_work(40_000), fuel),
        ["item-too-complex"]
    );
    let n = 400;
    let mut s = String::from("fn split(t: (");
    s.push_str(&vec!["i32"; n].join(", "));
    s.push_str(")) -> i32:\n    let (");
    s.push_str(&vec!["_"; n].join(", "));
    s.push_str(") = t\n    0\n");
    assert_eq!(codes_with_fuel(&s, 1_000_000), Vec::<String>::new());
    assert_eq!(codes_with_fuel(&s, 300), ["item-too-complex"]);
}
