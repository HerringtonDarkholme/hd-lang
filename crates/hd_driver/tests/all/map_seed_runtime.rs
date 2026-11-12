//! Internal seed oracles for compiled `Map` and `UniqMap` operations.
use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use std::collections::BTreeMap;
use std::io::Write as _;
use std::ops::Range;
use std::process::{Command, Stdio};

fn leb(bytes: &mut Vec<u8>, mut value: u32) {
    loop {
        let next = (value & 127) as u8;
        value >>= 7;
        bytes.push(next | if value == 0 { 0 } else { 128 });
        if value == 0 {
            break;
        }
    }
}

/// Add exports only: instruction bytes, indices, types and memory stay intact.
/// This lets the test inspect private runtime representation without an hd API.
fn expose(wasm: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let wanted = [
        "map_slot",
        "peer_slot",
        "copy_slot",
        "uniq_slot",
        "uniq_peer_slot",
        "map_insert",
        "map_remove",
        "map_copy",
        "uniq_insert",
        "uniq_remove",
        "uniq_reserve",
        "uniq_clear",
        "uniq_retain_none",
        "uniq_drain",
    ];
    let mut functions = BTreeMap::new();
    let mut sections = Vec::<(u8, Range<usize>)>::new();
    let mut exports = None;
    let mut read_names = |index, name: &str| {
        for wanted in wanted {
            if name
                .split('#')
                .next()
                .is_some_and(|name| name.ends_with(&format!("seed_probe_{wanted}")))
            {
                functions.insert(wanted, index);
            }
        }
    };
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        let payload = payload?;
        if let Some((id, range)) = payload.as_section() {
            sections.push((
                id,
                usize::try_from(range.start)?..usize::try_from(range.end)?,
            ));
        }
        match payload {
            wasmparser::Payload::ExportSection(section) => {
                let range = section.range();
                let mut reader = wasmparser::BinaryReader::new(
                    &wasm[usize::try_from(range.start)?..usize::try_from(range.end)?],
                    range.start,
                );
                let count = reader.read_var_u32()?;
                exports = Some((
                    count,
                    usize::try_from(reader.original_position())?,
                    usize::try_from(range.end)?,
                ));
            }
            wasmparser::Payload::CustomSection(section) if section.name() == "name" => {
                for subsection in wasmparser::NameSectionReader::new(wasmparser::BinaryReader::new(
                    section.data(),
                    section.data_offset(),
                )) {
                    if let wasmparser::Name::Function(names) = subsection? {
                        for name in names {
                            let name = name?;
                            read_names(name.index, name.name);
                        }
                    }
                }
            }
            wasmparser::Payload::CustomSection(section) if section.name() == "hd.names" => {
                let mut reader =
                    wasmparser::BinaryReader::new(section.data(), section.data_offset());
                for _ in 0..reader.read_var_u32()? {
                    let index = reader.read_var_u32()?;
                    read_names(index, reader.read_string()?);
                }
            }
            _ => {}
        }
    }
    let (count, start, end) = exports.ok_or("program has no exports")?;
    let mut extra = Vec::new();
    leb(&mut extra, count + u32::try_from(wanted.len())? + 1);
    extra.extend_from_slice(&wasm[start..end]);
    for name in wanted {
        leb(&mut extra, u32::try_from(name.len())?);
        extra.extend_from_slice(name.as_bytes());
        extra.push(0); // function
        leb(
            &mut extra,
            *functions
                .get(name)
                .ok_or_else(|| format!("no linked seed_probe_{name}: {functions:?}"))?,
        );
    }
    leb(&mut extra, 4);
    extra.extend_from_slice(b"heap");
    extra.push(2); // memory
    leb(&mut extra, 0); // hd_wasm::module::HEAP
    let mut bytes = wasm[..8].to_vec();
    for (id, range) in sections {
        bytes.push(id);
        let section = if id == 7 { &extra } else { &wasm[range] };
        leb(&mut bytes, u32::try_from(section.len())?);
        bytes.extend_from_slice(section);
    }
    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&bytes)?;
    Ok(bytes)
}

#[test]
fn compiled_maps_refresh_only_on_empty_transitions_and_replay_seeds()
-> Result<(), Box<dyn std::error::Error>> {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", include_str!("map_seed_runtime.hd"));
    let store = crate::warm_store();
    let host = Host {
        sources: &sources,
        store: &store,
        clock: &NoClock,
        render_tir: &[],
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let fixture = hd_driver::build_packages(
        &host,
        "app",
        &Packages::default(),
        &Goal::Fixture {
            module: "main".into(),
        },
    );
    assert!(!fixture.diags.has_errors(), "{}", fixture.render());
    let layout = fixture
        .fixture_layouts
        .iter()
        .find(|item| item.item.ends_with("SeedUniqLayout"))
        .ok_or("no UniqMap layout witness")?;
    let uniq_seed = layout
        .layout
        .fields
        .iter()
        .find(|field| field.path == "value.seed")
        .ok_or_else(|| format!("no nested seed field: {:?}", layout.layout))?
        .offset;
    eprintln!(
        "UniqMap layout: size={} align={} seed_offset={uniq_seed}",
        layout.layout.size, layout.layout.align
    );
    for level in [OptLevel::Dev, OptLevel::Optimized] {
        let out = hd_driver::build_packages(
            &host,
            "app",
            &Packages {
                level,
                build_profile: BuildProfile::Release,
                verify_own: true,
                ..Packages::default()
            },
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(!out.diags.has_errors(), "{level:?}: {}", out.render());
        let bytes = expose(out.wasm.as_deref().ok_or("no program module")?)?;
        let script = format!(
            r"
import assert from 'node:assert/strict';
const module = new WebAssembly.Module(new Uint8Array({bytes:?}));
function run(seed) {{
  let calls = 0;
  const imports = {{}};
  for (const item of WebAssembly.Module.imports(module)) {{
    (imports[item.module] ??= {{}})[item.name] = item.name === 'hash_seed'
      ? () => {{ calls++; return seed; }}
      : () => {{ throw Error('unexpected host import ' + item.module + '.' + item.name); }};
  }}
  const guest = new WebAssembly.Instance(module, imports).exports;
  guest['hd.init']();
  // These genuine counted owners remain alive for this instance's life.
  // Keep them once; raw reads do not manufacture local-reference lifetimes.
  const slots = Object.fromEntries(['map_slot', 'peer_slot', 'copy_slot', 'uniq_slot', 'uniq_peer_slot'].map(name => [name, guest[name]()]));
  const memory = () => new DataView(guest.heap.buffer);
  const pointer = (slot) => memory().getUint32(slots[slot], true);
  const mapSeed = (slot) => memory().getBigUint64(pointer(slot) + 24, true);
  const uniqSeed = (slot) => memory().getBigUint64(slots[slot] + {uniq_seed}, true);
  const a = mapSeed('map_slot'), b = mapSeed('peer_slot'), emptyCopy = mapSeed('copy_slot');
  const u = uniqSeed('uniq_slot'), v = uniqSeed('uniq_peer_slot');
  const trace = [a, b, emptyCopy, u, v];
  assert.equal(new Set(trace).size, trace.length, 'new empty maps need distinct seeds');
  guest.map_insert(1); guest.map_insert(2);
  assert.equal(mapSeed('map_slot'), a, 'growth must preserve the seed');
  guest.map_remove(77); guest.map_remove(1);
  assert.equal(mapSeed('map_slot'), a, 'missing or nonlast removal must not refresh');
  guest.map_copy();
  const shared = pointer('copy_slot');
  assert.equal(mapSeed('copy_slot'), a);
  guest.map_insert(3);
  assert.notEqual(pointer('map_slot'), shared, 'insertion must make the shared table unique');
  assert.equal(mapSeed('map_slot'), a, 'COW must preserve the seed');
  guest.map_remove(2);
  assert.equal(mapSeed('map_slot'), a);
  guest.map_remove(3);
  const fresh = mapSeed('map_slot'); trace.push(fresh);
  assert.notEqual(fresh, a, 'last removal must refresh');
  assert.equal(mapSeed('copy_slot'), a, 'refresh must not change a COW peer');
  guest.map_insert(4); assert.equal(mapSeed('map_slot'), fresh);
  guest.map_remove(4); trace.push(mapSeed('map_slot'));
  assert.equal(mapSeed('peer_slot'), b);
  guest.uniq_insert(1); guest.uniq_insert(2); guest.uniq_reserve();
  assert.equal(uniqSeed('uniq_slot'), u, 'UniqMap rehash must preserve the seed');
  guest.uniq_remove(77); guest.uniq_remove(1);
  assert.equal(uniqSeed('uniq_slot'), u);
  guest.uniq_remove(2); trace.push(uniqSeed('uniq_slot'));
  guest.uniq_insert(3); guest.uniq_clear(); trace.push(uniqSeed('uniq_slot'));
  let last = trace.at(-1); guest.uniq_clear(); assert.equal(uniqSeed('uniq_slot'), last);
  guest.uniq_insert(4); guest.uniq_insert(5); guest.uniq_retain_none(); trace.push(uniqSeed('uniq_slot'));
  last = trace.at(-1); guest.uniq_retain_none(); assert.equal(uniqSeed('uniq_slot'), last);
  guest.uniq_insert(6); guest.uniq_drain(); trace.push(uniqSeed('uniq_slot'));
  last = trace.at(-1); guest.uniq_drain(); assert.equal(uniqSeed('uniq_slot'), last);
  assert.equal(uniqSeed('uniq_peer_slot'), v);
  assert.equal(new Set(trace).size, trace.length, 'every empty transition needs a fresh seed');
  assert.equal(calls, 1, 'run seed zero must be initialized once');
  return trace;
}}
assert.deepEqual(run(0n), run(0n));
assert.deepEqual(run(19n), run(19n));
assert.notDeepEqual(run(0n), run(19n));
console.log('ok');
"
        );
        let mut node = Command::new("node")
            .args(["--input-type=module"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        node.stdin
            .take()
            .ok_or("Node stdin unavailable")?
            .write_all(script.as_bytes())?;
        let output = node.wait_with_output()?;
        assert!(
            output.status.success(),
            "{level:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout)?.trim(), "ok");
    }
    Ok(())
}
