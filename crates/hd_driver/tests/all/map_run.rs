//! Map work (#124, `crates/hd_mono/DESIGN.md` §13.12): keys chosen to
//! collide take linear map work. The programs run through `host/core.ts`,
//! to read the map work they count. The order, removal, growth and cursor
//! runs are stage cases under `tests/cases/map-run/`.

use std::ops::Range;
use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The module of `main`, built as a program.
fn wasm_of(main: &str) -> Vec<u8> {
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
    out.wasm.expect("wasm")
}

fn leb(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        out.push(if value == 0 { byte } else { byte | 0x80 });
        if value == 0 {
            break;
        }
    }
}

/// Whether a function is one of the map runtime's that `hd_wasm::map`
/// emits, one per entry type: `rt:map_find`, `rt:map_put`, `rt:map_grow`
/// and the loopless `rt:map_hash`, `rt:map_table` and `rt:map_seed`.
fn map_runtime(name: &str) -> bool {
    name.split('#')
        .next()
        .is_some_and(|n| n.starts_with("rt:map_"))
}

/// `wasm` with one more global, a mutable `i64` exported as `loops`, that
/// each loop of a function `counted` names adds 1 to per iteration. The
/// increment opens the loop's body, so every branch back to the loop counts.
/// Other bodies, indices and types stay as they were: the global takes the
/// next index, after every imported and defined one.
fn count_loops(
    wasm: &[u8],
    counted: &dyn Fn(&str) -> bool,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use wasmparser::{Operator, Parser, Payload, TypeRef};
    let at = |n: u64| usize::try_from(n);
    let mut sections: Vec<(u8, Range<usize>)> = Vec::new();
    let (mut imported_funcs, mut globals) = (0u32, 0u32);
    // A vector section: its count and where its items start and end.
    let mut global_items: Option<(u32, Range<usize>)> = None;
    let mut export_items: Option<(u32, Range<usize>)> = None;
    // Each body's range past its size, and where its loops' bodies open.
    let mut bodies: Vec<(Range<usize>, Vec<usize>)> = Vec::new();
    let mut names: hd_base::HashMap<u32, String> = hd_base::HashMap::default();
    let items = |range: Range<u64>| -> Result<(u32, Range<usize>), Box<dyn std::error::Error>> {
        let mut r =
            wasmparser::BinaryReader::new(&wasm[at(range.start)?..at(range.end)?], range.start);
        let count = r.read_var_u32()?;
        Ok((count, at(r.original_position())?..at(range.end)?))
    };
    for payload in Parser::new(0).parse_all(wasm) {
        let payload = payload?;
        if let Some((id, range)) = payload.as_section() {
            sections.push((id, at(range.start)?..at(range.end)?));
        }
        match payload {
            Payload::ImportSection(r) => {
                for import in r.into_imports() {
                    match import?.ty {
                        TypeRef::Func(_) | TypeRef::FuncExact(_) => imported_funcs += 1,
                        TypeRef::Global(_) => globals += 1,
                        _ => {}
                    }
                }
            }
            Payload::GlobalSection(r) => {
                globals += r.count();
                global_items = Some(items(r.range())?);
            }
            Payload::ExportSection(r) => export_items = Some(items(r.range())?),
            Payload::CodeSectionEntry(body) => {
                let mut ops = body.get_operators_reader()?;
                let mut loops = Vec::new();
                while !ops.eof() {
                    if let Operator::Loop { .. } = ops.read()? {
                        loops.push(at(ops.original_position())?);
                    }
                }
                let range = body.range();
                bodies.push((at(range.start)?..at(range.end)?, loops));
            }
            Payload::CustomSection(c) if c.name() == "name" => {
                let reader = wasmparser::BinaryReader::new(c.data(), c.data_offset());
                for sub in wasmparser::NameSectionReader::new(reader) {
                    if let wasmparser::Name::Function(map) = sub? {
                        for n in map {
                            let n = n?;
                            names.insert(n.index, n.name.to_owned());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let loops_global = globals;
    // global.get, i64.const 1, i64.add, global.set.
    let mut bump = vec![0x23];
    leb(&mut bump, loops_global);
    bump.extend_from_slice(&[0x42, 0x01, 0x7c, 0x24]);
    leb(&mut bump, loops_global);
    let mut code = Vec::new();
    leb(&mut code, u32::try_from(bodies.len())?);
    let mut found = 0;
    for (i, (range, loops)) in bodies.iter().enumerate() {
        let index = imported_funcs + u32::try_from(i)?;
        let loops: &[usize] = if names.get(&index).is_some_and(|n| counted(n)) {
            found += loops.len();
            loops
        } else {
            &[]
        };
        let mut body = Vec::with_capacity(range.len() + loops.len() * bump.len());
        let mut from = range.start;
        for &open in loops {
            body.extend_from_slice(&wasm[from..open]);
            body.extend_from_slice(&bump);
            from = open;
        }
        body.extend_from_slice(&wasm[from..range.end]);
        leb(&mut code, u32::try_from(body.len())?);
        code.extend_from_slice(&body);
    }
    if found == 0 {
        return Err("no loop in a function to count".into());
    }
    // A mutable i64 that starts at 0.
    let global = [0x7e, 0x01, 0x42, 0x00, 0x0b];
    let (count, rest) = global_items.clone().unwrap_or((0, 0..0));
    let mut global_section = Vec::new();
    leb(&mut global_section, count + 1);
    global_section.extend_from_slice(&wasm[rest]);
    global_section.extend_from_slice(&global);
    let (count, rest) = export_items.ok_or("no export section")?;
    let mut export_section = Vec::new();
    leb(&mut export_section, count + 1);
    export_section.extend_from_slice(&wasm[rest]);
    leb(&mut export_section, 5);
    export_section.extend_from_slice(b"loops");
    export_section.push(0x03);
    leb(&mut export_section, loops_global);
    let section =
        |out: &mut Vec<u8>, id: u8, bytes: &[u8]| -> Result<(), std::num::TryFromIntError> {
            out.push(id);
            leb(out, u32::try_from(bytes.len())?);
            out.extend_from_slice(bytes);
            Ok(())
        };
    let mut out = wasm[..8].to_vec();
    for (id, range) in sections {
        match id {
            6 => section(&mut out, 6, &global_section)?,
            7 => {
                // A module with no global section gets one here, in order.
                if global_items.is_none() {
                    section(&mut out, 6, &global_section)?;
                }
                section(&mut out, 7, &export_section)?;
            }
            10 => section(&mut out, 10, &code)?,
            _ => section(&mut out, id, &wasm[range])?,
        }
    }
    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all()).validate_all(&out)?;
    Ok(out)
}

/// Runs `main` on V8 with its map runtime's loops counted
/// ([`count_loops`]): its standard output, and the groups a find probes
/// and the matching slots it tests, plus the entries growth reinserts and
/// the groups each one probes. Unlike a time, the count depends on neither
/// the compiler's build nor the machine; the run's random hash seed moves
/// it by a few units.
fn map_loop_work(name: &str, main: &str) -> (String, u64) {
    let wasm = count_loops(&wasm_of(main), &map_runtime).expect("count the map's loops");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("map-work-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let core = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../host/core.ts")
        .canonicalize()
        .expect("host/core.ts");
    let script = format!(
        r"
import {{ readFileSync }} from 'node:fs';
import {{ pathToFileURL }} from 'node:url';
const {{ createCoreHost }} = await import(pathToFileURL({core:?}));
const module = new WebAssembly.Module(readFileSync({wasm:?}));
let out = '', err = '';
const host = createCoreHost({{ out: t => {{ out += t; }}, err: t => {{ err += t; }} }}, {{ module, grants: {{ Console: true }} }});
try {{
  const instance = new WebAssembly.Instance(module, host.imports);
  const {{ status }} = await host.run(instance, 'hd.init', 'hd.poll');
  if (status !== 0) throw Error(`status ${{status}}: ${{err}}`);
  process.stdout.write(`${{instance.exports.loops.value}}\n${{out}}`);
}} finally {{ host.close(); }}
",
        core = core.to_string_lossy(),
        wasm = path.to_string_lossy(),
    );
    // cost: one Node process per measured program.
    let r = Command::new("node")
        .args(["--input-type=module", "-e", &script])
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    assert!(
        r.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let text = String::from_utf8(r.stdout).expect("UTF-8");
    let (loops, out) = text.split_once('\n').expect("a count line");
    (out.to_owned(), loops.parse().expect("a count"))
}

/// The least and most keys a collision test measures: a linear map does
/// about 8 times the work for 8 times the keys, while one whose keys share
/// a probe sequence compares each key with the earlier ones, about 64
/// times the work.
const FEW: u32 = 9;
const MANY: u32 = 12;

/// Asserts that the map work of `run`, given a key count of `2^bits`,
/// grows linearly from [`FEW`] to [`MANY`] bits.
fn assert_linear(what: &str, run: &dyn Fn(u32) -> u64) {
    let (few, many) = (run(FEW), run(MANY));
    eprintln!("{what}: map loop iterations {few} for 2^{FEW} keys, {many} for 2^{MANY}");
    // Every operation probes at least one group.
    assert!(few >= 1 << FEW, "{what}: {few} loop iterations counted");
    assert!(
        many <= 12 * few,
        "{what}: 2^{FEW} keys took {few} map loop iterations, 2^{MANY} took {many}"
    );
}

/// Pairs of 6-letter blocks that lead from the same FNV-1a-32 state to the
/// same state.
const FNV32_PAIRS: [(&str, &str); 16] = [
    ("etyryl", "mjidmt"),
    ("ufefyz", "ibubif"),
    ("olixgb", "gryfkj"),
    ("gdcnyr", "shsbin"),
    ("wjuped", "arsrez"),
    ("encval", "qbsjqp"),
    ("slinwp", "wjsbuf"),
    ("ohapah", "slqlqt"),
    ("apynmn", "yjityv"),
    ("cncvaj", "wbsjqv"),
    ("krsraz", "choder"),
    ("ctchqd", "whstax"),
    ("cvifmj", "wryzuv"),
    ("wvyfyh", "cjibit"),
    ("yrylsh", "mvihct"),
    ("wvwbyv", "czgfij"),
];

/// Pairs of 11-letter blocks that lead from the same FNV-1a-64 state to
/// the same state (found by Pollard's rho).
const FNV64_PAIRS: [(&str, &str); 15] = [
    ("xLFsLJjiINP", "7Gg3RxEK0PE"),
    ("JdU9jHX4TqJ", "OM0YyM0u85J"),
    ("AugSUBgW75L", "cnjpGuWGzZD"),
    ("m5sdyq21BBG", "_giiglMbqlL"),
    ("4JKsnCMsoUG", "Gq8caPsJwuE"),
    ("ggLC_EYaqEH", "1OD6Nfm_0eN"),
    ("8m63a_auFPM", "wpZBVqlGciB"),
    ("U5oBHizR2DJ", "d7xyPoQOXFI"),
    ("NlD.hxb.YLB", "J.FHPO4huuF"),
    ("rdo2YOCChAO", "jnkEdU0xbkA"),
    ("SeahsXPklTG", ".RY30hrErLK"),
    ("EXcYViwLM3N", "mk0qPgfDxJC"),
    ("fONkr.v1G5E", "0Wh3F4JFuQH"),
    ("qdyN48b8gHM", "UQManoGiTKL"),
    ("waw.LRB_iqI", "A34Z3EbN1.F"),
];

/// `pairs` as an hd list literal, and the keys of masks 0 and
/// `2^bits - 1`: a mask picks the second block of pair `b` when its bit `b`
/// is set. Masks below `2^bits` pick the first block of every later pair,
/// so every key is as long as the full set's and leads through the same
/// states: the blocks collide only from the state they were found for,
/// which a key's length can set (`impl Hash for string`).
fn pairs_of(pairs: &[(&str, &str)], bits: u32) -> (String, String, String) {
    let list = pairs
        .iter()
        .map(|(a, b)| format!("(\"{a}\", \"{b}\")"))
        .collect::<Vec<_>>()
        .join(", ");
    let firsts: String = pairs.iter().map(|p| p.0).collect();
    let picked = pairs.iter().enumerate();
    let lasts: String = picked
        .map(|(b, p)| if b < bits as usize { p.1 } else { p.0 })
        .collect();
    (format!("[{list}]"), firsts, lasts)
}

/// Keys that collide under the fixed FNV-1a-32 the map once bucketed
/// strings by (#353): each key picks one block of each pair, and both
/// blocks of a pair lead from the same FNV state to the same state, so all
/// `2^bits` keys had one bucket hash and every insertion probed all the
/// earlier ones: about 4 * 10^9 key comparisons for 2^16 keys. With each
/// map's seed (`std.hash.hash_string`), they spread, and the map's work
/// grows linearly with the keys.
#[test]
fn keys_chosen_to_collide_under_a_fixed_hash_take_linear_map_work() {
    assert_linear("string keys", &|bits| {
        let (pairs, firsts, lasts) = pairs_of(&FNV32_PAIRS, bits);
        let n = 1usize << bits;
        let main = format!(
            "\
PAIRS := {pairs}

pub fn main() -> void $ Console:
    let mut m: Map[string, usize] = {{}}
    let mask: usize = 0
    while mask < {n}:
        let mut parts: List[string] = []
        let b: usize = 0
        while b < 16:
            pair := PAIRS[b]
            parts.push(if (mask >> b) & 1 == 1: pair._1 else: pair._0)
            b = b + 1
        m[parts.join(\"\")] = mask
        mask = mask + 1
    println(m.len())
    println(m.get(\"{firsts}\").unwrap_or(1))
    println(m.get(\"{lasts}\").unwrap_or(1))
"
        );
        let (out, work) = map_loop_work("collide", &main);
        assert_eq!(out, format!("{n}\n0\n{}\n", n - 1));
        work
    });
}

/// Integer keys that collided under the fixed mix the map once bucketed
/// integers by (#511): `x * 0x9E3779B1`, then `h ^ (h >> 16)`. Key `a`
/// is `p * 0x9E3779B1^-1` for `p = (a << 16) | a`, so its old hash was
/// `a << 16`, and the keys fell into two runs of the index; as a `u64` the
/// key sits in the high half, which the old mix folded onto the low one.
/// Each insertion and lookup then probed half the earlier keys: about
/// 4 * 10^9 probes over the two maps for 2^16 keys. With each map's seed
/// (`std.hash.hash_integer`), they spread.
#[test]
fn integer_keys_chosen_to_collide_under_a_fixed_mix_take_linear_map_work() {
    assert_linear("integer keys", &|bits| {
        let n = 1u64 << bits;
        let main = format!(
            "\
pub fn main() -> void $ Console:
    let mut narrow: Map[u32, u64] = {{}}
    let mut wide: Map[u64, u64] = {{}}
    let a: u64 = 0
    while a < {n}:
        key := ((a << 16) | a) * 244002641 & 4294967295
        narrow[u32(key)] = a
        wide[key << 32] = a
        a = a + 1
    let found: u64 = 0
    a = 0
    while a < {n}:
        key := ((a << 16) | a) * 244002641 & 4294967295
        if narrow.get(u32(key)).unwrap_or(0) == a && wide.get(key << 32).unwrap_or(0) == a:
            found = found + 1
        a = a + 1
    println(narrow.len())
    println(wide.len())
    println(found)
"
        );
        let (out, work) = map_loop_work("collide-int", &main);
        assert_eq!(out, format!("{n}\n{n}\n{n}\n"));
        work
    });
}

/// Composite keys whose `Hash` bytes collide under the unkeyed FNV-1a-64
/// `hash_of` the map once bucketed them by (#511): each name picks one
/// block of each pair, and both blocks of a pair lead from the same 64-bit
/// FNV state to the same state, so all `2^bits` tuples `(name, 7)`, and the
/// derived `Entry` values that write the same bytes, had one hash. Every
/// insertion then compared the key with all the earlier ones: about 10^9
/// `eq` calls over the two maps for 2^15 keys. With each map's seed
/// (`std.hash.hash_key`), they spread.
#[test]
fn composite_keys_chosen_to_collide_under_hash_of_take_linear_map_work() {
    assert_linear("composite keys", &|bits| {
        let (pairs, firsts, lasts) = pairs_of(&FNV64_PAIRS, bits);
        let n = 1usize << bits;
        let main = format!(
            "\
PAIRS := {pairs}

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
    let mut pairs: Map[(string, i32), usize] = {{}}
    let mut entries: Map[Entry, usize] = {{}}
    let mask: usize = 0
    while mask < {n}:
        name := name_of(mask)
        pairs[(name, 7)] = mask
        entries[Entry {{ name: name, n: 7 }}] = mask
        mask = mask + 1
    println(pairs.len())
    println(entries.len())
    println(pairs.get((\"{firsts}\", 7)).unwrap_or(1))
    println(entries.get(Entry {{ name: \"{lasts}\", n: 7 }}).unwrap_or(1))
"
        );
        let (out, work) = map_loop_work("collide-composite", &main);
        assert_eq!(out, format!("{n}\n{n}\n0\n{}\n", n - 1));
        work
    });
}
