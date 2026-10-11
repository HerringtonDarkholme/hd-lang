//! Integer widths (codegen.md §13.15): sub-word values stay canonical, so
//! checked arithmetic panics at the type's own range, casts keep the
//! target's low bits (`types.cast.wrap`), shifts drop the bits past the
//! width, and the `std.num` methods written over those operators wrap,
//! saturate, count and rotate at the operand's width. Programs build
//! through the driver and run on V8 (`host/run.mjs`), as the conformance
//! runner does.

use std::path::{Path, PathBuf};
use std::process::{Command, Output as Run};

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// Builds `body` as the body of `main` and runs it.
fn run(name: &str, items: &str, body: &str) -> Run {
    let mut main = format!("{items}\npub fn main() -> void $ Console:\n");
    for line in body.lines() {
        main.push_str("    ");
        main.push_str(line);
        main.push('\n');
    }
    let mut src = MemorySources::default();
    src.insert("main.hd", &main);
    let store = MemoryStore::default();
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
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("int-widths-{name}.wasm"));
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    ran
}

/// The lines a program that must succeed prints.
fn prints(name: &str, items: &str, body: &str) -> Vec<String> {
    let ran = run(name, items, body);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout)
        .expect("UTF-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Asserts that a program panics with category `code`.
fn panics(name: &str, body: &str, code: &str) {
    let ran = run(name, "", body);
    let stderr = String::from_utf8_lossy(&ran.stderr);
    assert!(
        !ran.status.success() && stderr.contains(&format!("panic: {code}:")),
        "{name}: expected {code}, got status {:?}, stdout {:?}, stderr {stderr}",
        ran.status,
        String::from_utf8_lossy(&ran.stdout),
    );
}

#[test]
fn i8_overflow_panics_at_127_plus_1() {
    panics(
        "i8-add",
        "let top: i8 = 126\nlet next = top + 1\nprintln(next)\nprintln(next + 1)",
        "integer-overflow",
    );
    panics(
        "i8-neg",
        "let low: i8 = -128\nprintln(-low)",
        "integer-overflow",
    );
    panics(
        "u8-sub",
        "let zero: u8 = 0\nprintln(zero - 1)",
        "integer-overflow",
    );
    panics(
        "u16-mul",
        "let big: u16 = 65535\nprintln(big * big)",
        "integer-overflow",
    );
}

#[test]
fn u8_wrapping_add_wraps_at_the_width() {
    let out = prints(
        "u8-wrapping",
        "",
        "let full: u8 = 255\nprintln(full.wrapping_add(1))\nlet zero: u8 = 0\nprintln(zero.wrapping_sub(1))",
    );
    assert_eq!(out, ["0", "255"]);
}

#[test]
fn u16_saturating_sub_stops_at_zero() {
    let out = prints(
        "u16-saturating",
        "",
        "let small: u16 = 3\nprintln(small.saturating_sub(10))\nprintln(small.saturating_sub(1))\nlet top: u16 = 65530\nprintln(top.saturating_add(10))",
    );
    assert_eq!(out, ["0", "2", "65535"]);
}

#[test]
fn narrowing_casts_keep_the_low_bits() {
    let out = prints(
        "casts",
        "",
        "let wide: i64 = 300\nprintln(u8(wide))\nlet minus: i32 = -1\nprintln(u8(minus))\nprintln(i8(u8(200)))\nprintln(u16(minus))\nlet word: u32 = 0x12345678\nprintln(u8(word >> 16))",
    );
    assert_eq!(out, ["44", "255", "-56", "65535", "52"]);
}

#[test]
fn bit_counts_read_a_u8_at_its_width() {
    let out = prints(
        "bit-counts",
        "",
        "let flags: u8 = 0b1011_0000\nprintln(flags.count_ones())\nprintln(flags.leading_zeros())\nlet zero: u8 = 0\nprintln(zero.leading_zeros())\nlet minus: i8 = -1\nprintln(minus.count_ones())",
    );
    assert_eq!(out, ["3", "0", "8", "8"]);
}

#[test]
fn u16_rotate_left_turns_at_16_bits() {
    let out = prints(
        "rotate",
        "",
        "let half: u16 = 0x8001\nprintln(half.rotate_left(1))\nprintln(half.rotate_right(1))\nprintln(half << 1)",
    );
    assert_eq!(out, ["3", "49152", "2"]);
}

#[test]
fn u32_wrapping_multiply_and_add_as_sha256_does() {
    let out = prints(
        "u32-wrapping",
        "fn mul32(a: u32, b: u32) -> u32:\n    u32(u64(a) * u64(b))\n",
        "println(mul32(0x9e3779b9, 0x85ebca6b))\nlet h: u32 = 0x6a09e667\nprintln(h.wrapping_add(0xbb67ae85))",
    );
    assert_eq!(out, ["3238976083", "628200684"]);
}

#[test]
fn bitwise_not_stays_in_the_width() {
    let out = prints(
        "not",
        "",
        "let flags: u8 = 0b1011_0000\nprintln(~flags)\nlet signed: i8 = 5\nprintln(~signed)\nlet byte: i8 = -128\nprintln(byte >> 7)",
    );
    assert_eq!(out, ["79", "-6", "-1"]);
}

#[test]
fn shift_counts_are_checked_against_the_width() {
    panics(
        "u8-shift",
        "let one: u8 = 1\nlet count: u32 = 8\nprintln(one << count)",
        "invalid-shift",
    );
    panics(
        "u64-count",
        "let one: i32 = 1\nlet count: u64 = 4294967296\nprintln(one >> count)",
        "invalid-shift",
    );
}

#[test]
fn i64_multiply_and_minimum_division_overflow() {
    panics(
        "i64-mul",
        "let big: i64 = 4294967296\nprintln(big * big)",
        "integer-overflow",
    );
    panics(
        "i64-min-mul",
        "let low: i64 = -9223372036854775807 - 1\nlet minus: i64 = -1\nprintln(minus * low)",
        "integer-overflow",
    );
    panics(
        "i64-min-div",
        "let low: i64 = -9223372036854775807 - 1\nlet minus: i64 = -1\nprintln(low / minus)",
        "integer-overflow",
    );
    panics(
        "u64-mul",
        "let big: u64 = 4294967296\nprintln(big * big)",
        "integer-overflow",
    );
    let out = prints(
        "i64-mul-fits",
        "",
        "let low: i64 = -9223372036854775807 - 1\nlet one: i64 = 1\nprintln(low * one)\nlet minus: i64 = -1\nprintln(minus * 5)\nlet three: u64 = 3\nprintln(three * 6148914691236517205)\nprintln(low % minus)",
    );
    assert_eq!(
        out,
        ["-9223372036854775808", "-5", "18446744073709551615", "0"]
    );
}
