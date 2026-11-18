//! #2031: a struct nested by value, built by `mk{i}` and read by
//! `get{i}` down a chain of K structs, costs each function code that does
//! not grow with the value's size: a parameter over 16 values passes as
//! one address, a result over 16 values is written into its caller's
//! block, and a literal headed there writes each field in place. So the
//! module grows linearly in K. Before, it grew 11x when K grew 4x.

use std::fmt::Write as _;

use hd_driver::{EngineTarget, Executor, Goal, Host, NoClock, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The issue's program in one file: `T{i}` holds `T{i-1}` and an `i32`.
fn program(k: usize) -> String {
    let mut text = String::from(
        "struct T0:\n    v: i32\n\nfn mk0() -> T0: T0 { v: +1 }\nfn get0(t: T0) -> i32: t.v\n",
    );
    for i in 1..k {
        let p = i - 1;
        let _ = write!(
            text,
            "\nstruct T{i}:\n    inner: T{p}\n    v: i32\n\n\
             fn mk{i}() -> T{i}: T{i} {{ inner: mk{p}(), v: +{i} }}\n\
             fn get{i}(t: T{i}) -> i32: get{p}(t.inner) * 3 + 1\n"
        );
    }
    let last = k - 1;
    let _ = write!(
        text,
        "\npub fn main() -> void $ Console:\n    println(\"${{get{last}(mk{last}())}}\")\n"
    );
    text
}

/// The counted variant (step 4): `T{i}` also holds a string. Each
/// `get{i}` takes its parameter by address, moves `inner` on and drops
/// the rest through `T{i}`'s release witness, which calls `T{i-1}`'s;
/// each `mk{i}` writes its literal, string included, into its caller's
/// block, `inner` in place.
fn counted_program(k: usize) -> String {
    let mut text = String::from(
        "struct T0:\n    v: i32\n    s: string\n\n\
         fn mk0() -> T0: T0 { v: +1, s: \"s\" }\nfn get0(t: T0) -> i32: t.v\n",
    );
    for i in 1..k {
        let p = i - 1;
        let _ = write!(
            text,
            "\nstruct T{i}:\n    inner: T{p}\n    v: i32\n    s: string\n\n\
             fn mk{i}() -> T{i}: T{i} {{ inner: mk{p}(), v: +{i}, s: \"s\" }}\n\
             fn get{i}(t: T{i}) -> i32: get{p}(t.inner) % 1000 + t.v\n"
        );
    }
    let last = k - 1;
    let _ = write!(
        text,
        "\npub fn main() -> void $ Console:\n    println(\"${{get{last}(mk{last}())}}\")\n"
    );
    text
}

fn module_bytes(k: usize) -> usize {
    build_bytes(&program(k))
}

fn build_bytes(source: &str) -> usize {
    build_wasm(source).len()
}

fn build_wasm(source: &str) -> Vec<u8> {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        target: EngineTarget::V8,
        ..Packages::default()
    };
    let output = build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!output.diags.has_errors(), "{}", output.render());
    output.wasm.unwrap_or_default()
}

#[test]
fn struct_chain_module_grows_linearly() {
    let worker = std::thread::Builder::new()
        .stack_size(hd_sched::pool::WORKER_STACK)
        .spawn(|| {
            let (small, large) = (module_bytes(50), module_bytes(200));
            eprintln!("struct chain module bytes: K=50 {small}, K=200 {large}");
            assert!(small > 0);
            assert!(large * 2 <= small * 9, "K=50 {small}, K=200 {large}");
        });
    assert!(worker.is_ok_and(|w| w.join().is_ok()));
}

#[test]
fn counted_struct_chain_module_grows_linearly() {
    let worker = std::thread::Builder::new()
        .stack_size(hd_sched::pool::WORKER_STACK)
        .spawn(|| {
            let (small, large) = (
                build_bytes(&counted_program(50)),
                build_bytes(&counted_program(200)),
            );
            eprintln!("counted struct chain module bytes: K=50 {small}, K=200 {large}");
            assert!(small > 0);
            assert!(large * 2 <= small * 9, "K=50 {small}, K=200 {large}");
        });
    assert!(worker.is_ok_and(|w| w.join().is_ok()));
}

/// `take` owns a struct of `f` strings by address and moves each field
/// on to `keep`, which owns it, so its drop skips each under the
/// field's flag. With `local`, the struct is a local of `take` kept in a
/// frame slot instead of its parameter.
fn moved_fields_program(f: usize, local: bool) -> String {
    let mut text = String::from("struct W:\n");
    for k in 0..f {
        let _ = writeln!(text, "    s{k}: string");
    }
    text.push_str("\nfn mk() -> W: W { ");
    for k in 0..f {
        let _ = write!(text, "{}s{k}: \"v{k}\"", if k == 0 { "" } else { ", " });
    }
    text.push_str(" }\n\nfn keep(s: string) -> string: s\n\n");
    text.push_str(if local {
        "fn take() -> i32:\n    let w = mk()\n    let n = 0\n"
    } else {
        "fn take(w: W) -> i32:\n    let n = 0\n"
    });
    for k in 0..f {
        let _ = writeln!(text, "    n += i32(keep(w.s{k}).len())");
    }
    let call = if local { "take()" } else { "take(mk())" };
    let _ = write!(
        text,
        "    n\n\npub fn main() -> void $ Console:\n    println(\"${{{call}}}\")\n"
    );
    text
}

/// Moving F fields out of one parameter kept by address costs O(F): each
/// move probes its flag by (local, path), and the drop groups the moved
/// paths by field once per level. Before, each move scanned the moved
/// list and each release step filtered it, O(F^2).
#[test]
fn moved_fields_cost_linear_work() {
    moved_fields_linear(false);
}

/// The same for a local kept in a frame slot, whose store also clears
/// each moved field's flag once.
#[test]
fn moved_local_fields_cost_linear_work() {
    moved_fields_linear(true);
}

fn moved_fields_linear(local: bool) {
    let worker = std::thread::Builder::new()
        .stack_size(hd_sched::pool::WORKER_STACK)
        .spawn(move || {
            let ((), small) = hd_wasm::work::count_moved_paths(|| {
                build_bytes(&moved_fields_program(40, local));
            });
            let ((), large) = hd_wasm::work::count_moved_paths(|| {
                build_bytes(&moved_fields_program(400, local));
            });
            eprintln!("moved field work (local {local}): F=40 {small}, F=400 {large}");
            assert!(small >= 40, "F=40 {small}");
            assert!(large <= small * 12, "F=40 {small}, F=400 {large}");
        });
    assert!(worker.is_ok_and(|w| w.join().is_ok()));
}

/// The `Drop` variant (#2031): `T{i}` also holds a string and has a
/// `Drop` hook, so it is a std module (only std may implement the
/// runtime trait). Each `get{i}` takes its parameter by address and
/// moves `inner` on; its drop copies the rest into a heap frame and runs
/// `T{i}`'s own hook under `inner`'s move flag. A drop that keeps
/// `inner` calls `T{i}`'s hook witness, which calls `T{i-1}`'s: per type
/// O(its own fields).
fn drop_program(k: usize) -> String {
    let mut text = String::from(
        "use std.rt.Drop\n\nlet seen: i32 = 0\n\n\
         struct T0:\n    v: i32\n    s: string\n\n\
         impl !Copy for T0\n\n\
         impl Drop for T0:\n    fn drop(&mut self) -> void:\n        seen += self.v\n\n\
         fn mk0() -> T0: T0 { v: +1, s: \"s\" }\nfn get0(t: T0) -> i32: t.v\n",
    );
    for i in 1..k {
        let p = i - 1;
        let _ = write!(
            text,
            "\nstruct T{i}:\n    inner: T{p}\n    v: i32\n    s: string\n\n\
             impl !Copy for T{i}\n\n\
             impl Drop for T{i}:\n    fn drop(&mut self) -> void:\n        seen += self.v\n\n\
             fn mk{i}() -> T{i}: T{i} {{ inner: mk{p}(), v: +{i}, s: \"s\" }}\n\
             fn get{i}(t: T{i}) -> i32: get{p}(t.inner) % 1000 + t.v\n\
             fn all{i}(t: T{i}) -> i32: t.v\n"
        );
    }
    let last = k - 1;
    let _ = write!(
        text,
        "\npub fn main() -> void:\n    seen += get{last}(mk{last}()) + all{last}(mk{last}())\n"
    );
    text
}

/// `source` without its `Drop` and `!Copy` implementations: the same
/// chain with nothing to run at a drop.
fn without_drop(source: &str) -> String {
    let mut out = String::new();
    let mut skip = 0;
    for line in source.lines() {
        if line.starts_with("impl Drop") {
            skip = 2;
            continue;
        }
        if skip > 0 {
            skip -= 1;
            continue;
        }
        if !line.starts_with("impl !Copy") {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// [`build_bytes`] of `source` as std's module `rt_tests.chain`, a
/// program from its `main`, so the linker drops unused wrappers.
fn std_build_bytes(source: &str) -> usize {
    let mut sources = hd_driver::std_sources();
    sources.insert("rt_tests/chain.hd", source);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        target: EngineTarget::V8,
        std_leaf: Some("rt_tests/chain.hd"),
        ..Packages::default()
    };
    let output = build_packages(
        &host,
        "std",
        &packages,
        &Goal::Program {
            entry: "rt_tests.chain".into(),
        },
    );
    assert!(!output.diags.has_errors(), "{}", output.render());
    output.wasm.as_deref().map_or(0, <[u8]>::len)
}

#[test]
fn drop_struct_chain_module_grows_linearly() {
    // #2069: constrain the complete std modules too, not only the
    // hooks' contribution after subtracting a quadratic no-Drop module.
    let worker = std::thread::Builder::new()
        .stack_size(hd_sched::pool::WORKER_STACK)
        .spawn(|| {
            let sizes = |k: usize| {
                let with = std_build_bytes(&drop_program(k));
                let without = std_build_bytes(&without_drop(&drop_program(k)));
                eprintln!("drop struct chain K={k}: {with} B, {without} B without hooks");
                (with, without)
            };
            let (small, large) = (sizes(50), sizes(200));
            for (label, small, large) in [
                ("with Drop", small.0, large.0),
                ("without Drop", small.1, large.1),
                (
                    "drop hooks",
                    small.0.saturating_sub(small.1),
                    large.0.saturating_sub(large.1),
                ),
            ] {
                eprintln!("{label} module bytes: K=50 {small}, K=200 {large}");
                assert!(small > 0);
                assert!(
                    large * 2 <= small * 9,
                    "{label}: K=50 {small}, K=200 {large}"
                );
            }
        });
    assert!(worker.is_ok_and(|w| w.join().is_ok()));
}

/// #2170's probe: a tuple doubled `depth` times through named types, so
/// the source is linear in `depth` while `D{depth}` holds 2^(depth+1)
/// `i32`s; one `make{i}` builds each level, a generic `keep` returns its
/// parameter whole and `first{i}` reads a field down the chain.
fn doubling_program(depth: usize) -> String {
    let mut text = String::from(
        "type D0 = (i32, i32)\n\nfn keep[T](x: T) -> T: x\n\nfn make0() -> D0: (1, 2)\n",
    );
    for i in 1..=depth {
        let p = i - 1;
        let _ = write!(
            text,
            "\ntype D{i} = (D{p}, D{p})\n\
             fn make{i}() -> D{i}: (make{p}(), make{p}())\n\
             fn first{i}(x: D{i}) -> D{p}: x._0\n"
        );
    }
    let mut chain = String::from("v");
    for i in (1..=depth).rev() {
        chain = format!("first{i}({chain})");
    }
    let _ = write!(
        text,
        "\npub fn main() -> void $ Console:\n    let v = keep(make{depth}())\n    \
         let p = {chain}\n    println(\"${{p._0}}\")\n"
    );
    text
}

/// The Wasm instructions of every function body in `wasm`.
fn instructions(wasm: &[u8]) -> usize {
    let mut n = 0;
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let Ok(wasmparser::Payload::CodeSectionEntry(body)) = payload {
            let mut reader = body.get_operators_reader().expect("operators");
            while !reader.eof() {
                reader.read().expect("operator");
                n += 1;
            }
        }
    }
    n
}

/// A `Memory` value read whole is copied with one `memory.copy` of its
/// size, never one load and store per scalar (#2170, rule 3 of the
/// backend-neutral TIR's layout). So `keep[D20]`, a value of two million
/// `i32`s, costs what `keep[D13]` does: the module's instructions and the
/// layouts computed grow linearly in the depth. Before, `keep` took the
/// scalar calling convention: 49,153 loads at depth 13, and depth 20 did
/// not build in two minutes.
#[test]
fn a_doubling_value_kept_whole_grows_linearly_in_depth() {
    let worker = std::thread::Builder::new()
        .stack_size(hd_sched::pool::WORKER_STACK)
        .spawn(|| {
            let measure = |depth: usize| {
                let (wasm, parts) =
                    hd_layout::work::count_layout_parts(|| build_wasm(&doubling_program(depth)));
                let n = instructions(&wasm);
                eprintln!("doubling keep, depth {depth}: {n} instructions, {parts} layouts");
                (n, usize::try_from(parts).expect("parts"))
            };
            // Depth 1 warms std up and is the fixed part: past it, each
            // level adds the same work, so the 19 levels past it at depth
            // 20 cost at most twice 19/12 of the 12 levels at depth 13.
            let (base, small, large) = (measure(1), measure(13), measure(20));
            for (k, what) in [(0, "instructions"), (1, "layouts")] {
                let pick = |m: (usize, usize)| if k == 0 { m.0 } else { m.1 };
                let (s, l) = (pick(small) - pick(base), pick(large) - pick(base));
                assert!(
                    s > 0 && l * 12 <= s * 19 * 2,
                    "{what}: {base:?} {small:?} {large:?}"
                );
            }
        });
    assert!(worker.is_ok_and(|w| w.join().is_ok()));
}
