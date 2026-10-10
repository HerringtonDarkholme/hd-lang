//! Exercise the host ABI directly: the identical Wasm and grant configuration
//! run on Node and wasmtime, with their complete streams and status compared.
//! No compiler lowering or hd syntax is involved in these provider tests.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use hd_run::{Grants, HostSetup, Limit, Limits};
use hd_run_wasmtime::{Captured, EngineConfig, run_captured};
use wasm_encoder::{
    BlockType, CodeSection, DataSection, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, ImportSection, Instruction as I, MemArg, MemorySection, MemoryType, Module,
    TypeSection, ValType,
};

const ARG_BASE: i32 = 1 << 20;
const HEX_BASE: i32 = 1 << 19;
const MEM: MemArg = MemArg {
    offset: 0,
    align: 0,
    memory_index: 0,
};

/// Exchange-buffer encoding, independent of the implementation's codec.
#[derive(Default)]
struct Enc(Vec<u8>);

impl Enc {
    fn leb(mut self, mut value: u64) -> Self {
        loop {
            let byte = (value & 127) as u8;
            value >>= 7;
            self.0.push(byte | if value == 0 { 0 } else { 128 });
            if value == 0 {
                return self;
            }
        }
    }

    fn bytes(mut self, value: &[u8]) -> Self {
        self = self.leb(value.len() as u64);
        self.0.extend_from_slice(value);
        self
    }

    fn str(self, value: &str) -> Self {
        self.bytes(value.as_bytes())
    }
}

fn path(p: &Path) -> &str {
    p.to_str().expect("UTF-8 temporary path")
}

// Both providers append a literal slash to the requested directory, even
// when that directory uses native Windows path separators.
fn listed_path(dir: &Path, name: &str) -> String {
    let directory = path(dir);
    let slash = if directory.ends_with('/') { "" } else { "/" };
    format!("{directory}{slash}{name}")
}

fn fs_error(p: &Path, variant: u64) -> Vec<u8> {
    Enc::default().leb(1).leb(variant).str(path(p)).0
}

/// Tiny modules use the real import signatures and exports. Results are
/// rendered as hex so binary values, enum tags and lengths all remain visible.
struct Program {
    types: TypeSection,
    imports: ImportSection,
    import_count: u32,
    imported: BTreeMap<(String, String), u32>,
    type_count: u32,
    data: DataSection,
    next_data: i32,
    body: Vec<I<'static>>,
    expected: Vec<Vec<u8>>,
}

impl Program {
    fn new() -> Self {
        let mut p = Self {
            types: TypeSection::new(),
            imports: ImportSection::new(),
            import_count: 0,
            imported: BTreeMap::new(),
            type_count: 0,
            data: DataSection::new(),
            next_data: ARG_BASE,
            body: Vec::new(),
            expected: Vec::new(),
        };
        assert_eq!(p.import("hd:rt", "stderr", &[ValType::I32], &[]), 0);
        let console = p.import(
            "hd:Console",
            "write_line.start",
            &[ValType::I32],
            &[ValType::I32, ValType::I32],
        );
        let block = p.import("hd:rt", "block", &[], &[ValType::I32]);
        let finish = p.import(
            "hd:Console",
            "write_line.finish",
            &[ValType::I32],
            &[ValType::I32],
        );
        p.load_args(b"core-parity");
        p.instructions(&[
            I::I32Const(11),
            I::Call(console),
            I::LocalSet(1),
            I::Drop,
            I::Call(block),
            I::Drop,
            I::LocalGet(1),
            I::Call(finish),
            I::Drop,
        ]);
        p
    }

    fn ty(&mut self, params: &[ValType], results: &[ValType]) -> u32 {
        let index = self.type_count;
        self.types.ty().function(params.to_vec(), results.to_vec());
        self.type_count += 1;
        index
    }

    fn import(&mut self, module: &str, name: &str, params: &[ValType], results: &[ValType]) -> u32 {
        let key = (module.to_owned(), name.to_owned());
        if let Some(index) = self.imported.get(&key) {
            return *index;
        }
        let ty = self.ty(params, results);
        let index = self.import_count;
        self.imports.import(module, name, EntityType::Function(ty));
        self.import_count += 1;
        self.imported.insert(key, index);
        index
    }

    fn instructions(&mut self, instructions: &[I<'static>]) {
        self.body.extend_from_slice(instructions);
    }

    fn load_args(&mut self, bytes: &[u8]) {
        let offset = self.next_data;
        self.next_data += i32::try_from(bytes.len()).unwrap() + 1;
        self.data.active(
            0,
            &wasm_encoder::ConstExpr::i32_const(offset),
            bytes.iter().copied(),
        );
        self.instructions(&[
            I::I32Const(0),
            I::I32Const(offset),
            I::I32Const(i32::try_from(bytes.len()).unwrap()),
            I::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
        ]);
    }

    fn call(&mut self, module: &str, method: &str, args: impl AsRef<[u8]>, expected: Vec<u8>) {
        let args = args.as_ref();
        let index = self.import(
            module,
            &format!("{method}.start"),
            &[ValType::I32],
            &[ValType::I32, ValType::I32],
        );
        self.load_args(args);
        self.instructions(&[
            I::I32Const(i32::try_from(args.len()).unwrap()),
            I::Call(index),
            I::LocalSet(0),
            I::LocalSet(1),
            I::LocalGet(1),
            I::If(BlockType::Empty),
            I::Unreachable,
            I::End,
        ]);
        self.dump();
        self.expected.push(expected);
    }

    fn sys(&mut self, method: &str) {
        let index = self.import("hd:Sys", method, &[], &[ValType::I32]);
        self.instructions(&[I::Call(index), I::LocalSet(0)]);
        self.dump();
    }

    fn dump(&mut self) {
        // The dump function is the first defined function, whose index is
        // unknown until all imports have been appended. Patch it in finish.
        self.instructions(&[I::LocalGet(0), I::Call(u32::MAX)]);
    }

    fn finish(mut self, status: i32) -> (Vec<u8>, String) {
        self.body.push(I::End);
        let dump_type = self.ty(&[ValType::I32], &[]);
        let init_type = self.ty(&[], &[]);
        let poll_type = self.ty(&[], &[ValType::I32]);
        let mut functions = FunctionSection::new();
        functions
            .function(dump_type)
            .function(init_type)
            .function(poll_type);
        let mut exports = ExportSection::new();
        exports.export("hd.x", ExportKind::Memory, 0);
        exports.export("hd.init", ExportKind::Func, self.import_count + 1);
        exports.export("hd.poll", ExportKind::Func, self.import_count + 2);
        let mut memory = MemorySection::new();
        memory.memory(MemoryType {
            minimum: 64,
            maximum: Some(64),
            memory64: false,
            shared: false,
            page_size_log2: None,
        });

        let mut dump = Function::new([(2, ValType::I32)]);
        let hex = self.next_data;
        self.data.active(
            0,
            &wasm_encoder::ConstExpr::i32_const(hex),
            b"0123456789abcdef".iter().copied(),
        );
        for instruction in [
            I::I32Const(0),
            I::LocalSet(1),
            I::Block(BlockType::Empty),
            I::Loop(BlockType::Empty),
            I::LocalGet(1),
            I::LocalGet(0),
            I::I32GeU,
            I::BrIf(1),
            I::LocalGet(1),
            I::I32Load8U(MEM),
            I::LocalSet(2),
            I::I32Const(HEX_BASE),
            I::LocalGet(1),
            I::I32Const(2),
            I::I32Mul,
            I::I32Add,
            I::I32Const(hex),
            I::LocalGet(2),
            I::I32Const(4),
            I::I32ShrU,
            I::I32Add,
            I::I32Load8U(MEM),
            I::I32Store8(MEM),
            I::I32Const(HEX_BASE + 1),
            I::LocalGet(1),
            I::I32Const(2),
            I::I32Mul,
            I::I32Add,
            I::I32Const(hex),
            I::LocalGet(2),
            I::I32Const(15),
            I::I32And,
            I::I32Add,
            I::I32Load8U(MEM),
            I::I32Store8(MEM),
            I::LocalGet(1),
            I::I32Const(1),
            I::I32Add,
            I::LocalSet(1),
            I::Br(0),
            I::End,
            I::End,
            I::I32Const(HEX_BASE),
            I::LocalGet(0),
            I::I32Const(2),
            I::I32Mul,
            I::I32Add,
            I::I32Const(10),
            I::I32Store8(MEM),
            I::I32Const(0),
            I::I32Const(HEX_BASE),
            I::LocalGet(0),
            I::I32Const(2),
            I::I32Mul,
            I::I32Const(1),
            I::I32Add,
            I::MemoryCopy {
                src_mem: 0,
                dst_mem: 0,
            },
            I::LocalGet(0),
            I::I32Const(2),
            I::I32Mul,
            I::I32Const(1),
            I::I32Add,
            I::Call(0),
            I::End,
        ] {
            dump.instruction(&instruction);
        }

        // Locals: result length, status, scratch, and two random i64s.
        let mut init = Function::new([(3, ValType::I32), (2, ValType::I64)]);
        for instruction in self.body {
            let instruction = match instruction {
                I::Call(u32::MAX) => I::Call(self.import_count),
                other => other,
            };
            init.instruction(&instruction);
        }
        let mut poll = Function::new([]);
        poll.instruction(&I::I32Const(status)).instruction(&I::End);
        let mut code = CodeSection::new();
        code.function(&dump).function(&init).function(&poll);
        let mut module = Module::new();
        module
            .section(&self.types)
            .section(&self.imports)
            .section(&functions)
            .section(&memory)
            .section(&exports)
            .section(&code)
            .section(&self.data);
        let expected = self
            .expected
            .iter()
            .map(|bytes| {
                let mut line = String::new();
                for byte in bytes {
                    write!(line, "{byte:02x}").unwrap();
                }
                line.push('\n');
                line
            })
            .collect();
        (module.finish(), expected)
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "hd-core-parity-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        Self(fs::canonicalize(dir).unwrap())
    }

    fn at(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn parity(
    scratch: &Scratch,
    wasm: &[u8],
    limits: Vec<(&str, Limit)>,
    reset: impl Fn(),
) -> Captured {
    let limits = limits
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect::<Vec<_>>();
    let json_string = |s: &str| {
        let mut quoted = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => quoted.push_str("\\\""),
                '\\' => quoted.push_str("\\\\"),
                c if c <= '\u{1f}' => write!(quoted, "\\u{:04x}", u32::from(c)).unwrap(),
                c => quoted.push(c),
            }
        }
        quoted.push('"');
        quoted
    };
    let grants = limits
        .iter()
        .map(|(key, limit)| {
            let value = match limit {
                Limit::Deny => "false".into(),
                Limit::Entries(entries) => format!(
                    "[{}]",
                    entries
                        .iter()
                        .map(|s| json_string(s))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            };
            format!("{}:{value}", json_string(key))
        })
        .collect::<Vec<_>>()
        .join(",");
    let config = scratch.at("run.json");
    let file = scratch.at("program.wasm");
    fs::write(&config, format!("{{\"grants\":{{{grants}}}}}")).unwrap();
    fs::write(&file, wasm).unwrap();
    let setup = HostSetup {
        providers: vec![],
        grants: Grants { limits },
        limits: Limits::default(),
        args: vec![],
        program: String::new(),
    };
    reset();
    let rust =
        run_captured(EngineConfig::default(), wasm, &setup).expect("wasmtime runs ABI module");
    reset();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("host/run.mjs");
    let node = Command::new("node")
        .arg(root)
        .arg(file)
        .arg(config)
        .stdin(Stdio::null())
        .output()
        .expect("Node host starts");
    assert_eq!(
        rust.status,
        u8::try_from(node.status.code().expect("Node exits normally")).unwrap(),
        "Node stderr: {}",
        String::from_utf8_lossy(&node.stderr)
    );
    assert_eq!(rust.stdout, node.stdout, "stdout differs");
    assert_eq!(rust.stderr, node.stderr, "stderr differs");
    rust
}

fn assert_program(scratch: &Scratch, p: Program, limits: Vec<(&str, Limit)>, reset: impl Fn()) {
    let (wasm, expected) = p.finish(7);
    let result = parity(scratch, &wasm, limits, reset);
    assert_eq!(result.status, 7);
    assert_eq!(result.stdout, b"core-parity\n");
    assert_eq!(result.stderr, expected.as_bytes());
}

fn one(p: &Path) -> Vec<u8> {
    Enc::default().str(path(p)).0
}

#[test]
fn fs_read_all_methods_binary_text_bom_entries_and_errors() {
    let scratch = Scratch::new();
    let dir = scratch.at("entries");
    fs::create_dir(&dir).unwrap();
    let file = dir.join("a");
    fs::write(&file, b"hello\0world").unwrap();
    let sub = dir.join("z");
    fs::create_dir(&sub).unwrap();
    let text = scratch.at("text");
    fs::write(&text, "\u{feff}héllo").unwrap();
    let bad = scratch.at("bad");
    fs::write(&bad, [0xff, 0xfe]).unwrap();
    let missing = scratch.at("absent");
    let notdir = file.join("child");
    let mut p = Program::new();
    p.call(
        "hd:FsRead",
        "read_bytes",
        one(&file),
        Enc::default().leb(0).bytes(b"hello\0world").0,
    );
    p.call(
        "hd:FsRead",
        "read_text",
        one(&text),
        Enc::default().leb(0).str("héllo").0,
    );
    p.call("hd:FsRead", "read_text", one(&bad), fs_error(&bad, 6));
    p.call(
        "hd:FsRead",
        "read_bytes",
        one(&missing),
        fs_error(&missing, 0),
    );
    p.call(
        "hd:FsRead",
        "read_text",
        one(&missing),
        fs_error(&missing, 0),
    );
    p.call(
        "hd:FsRead",
        "stat",
        one(&file),
        Enc::default()
            .leb(0)
            .leb(1)
            .str(path(&file))
            .leb(0)
            .leb(11)
            .0,
    );
    p.call(
        "hd:FsRead",
        "stat",
        one(&sub),
        Enc::default().leb(0).leb(1).str(path(&sub)).leb(1).leb(0).0,
    );
    p.call("hd:FsRead", "stat", one(&missing), vec![0, 0]);
    p.call("hd:FsRead", "stat", one(&notdir), vec![0, 0]);
    p.call(
        "hd:FsRead",
        "list_dir",
        one(&dir),
        Enc::default()
            .leb(0)
            .leb(2)
            .str(&listed_path(&dir, "a"))
            .leb(0)
            .leb(11)
            .str(&listed_path(&dir, "z"))
            .leb(1)
            .leb(0)
            .0,
    );
    p.call(
        "hd:FsRead",
        "list_dir",
        one(&missing),
        fs_error(&missing, 0),
    );
    p.call("hd:FsRead", "list_dir", one(&file), fs_error(&file, 4));
    p.call(
        "hd:FsRead",
        "read_bytes",
        one(&notdir),
        fs_error(&notdir, 4),
    );
    p.call("hd:FsRead", "read_bytes", one(&dir), fs_error(&dir, 5));
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn fs_write_all_methods_success_and_os_errors() {
    let scratch = Scratch::new();
    let area = scratch.at("area");
    let binary = area.join("binary");
    let text = area.join("text");
    let nested = area.join("nested/child");
    let moved = area.join("moved");
    let missing = area.join("missing");
    let mut p = Program::new();
    p.call(
        "hd:FsWrite",
        "write_bytes",
        Enc::default().str(path(&binary)).bytes(&[0, 255, 42]).0,
        vec![0],
    );
    p.call(
        "hd:FsRead",
        "read_bytes",
        one(&binary),
        Enc::default().leb(0).bytes(&[0, 255, 42]).0,
    );
    p.call(
        "hd:FsWrite",
        "write_text",
        Enc::default().str(path(&text)).str("hé").0,
        vec![0],
    );
    p.call(
        "hd:FsWrite",
        "append_text",
        Enc::default().str(path(&text)).str("llo").0,
        vec![0],
    );
    p.call(
        "hd:FsRead",
        "read_text",
        one(&text),
        Enc::default().leb(0).str("héllo").0,
    );
    p.call("hd:FsWrite", "create_dir_all", one(&nested), vec![0]);
    p.call("hd:FsWrite", "create_dir_all", one(&nested), vec![0]);
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default().str(path(&text)).str(path(&moved)).0,
        vec![0],
    );
    p.call(
        "hd:FsRead",
        "read_text",
        one(&moved),
        Enc::default().leb(0).str("héllo").0,
    );
    p.call("hd:FsWrite", "remove", one(&moved), vec![0]);
    p.call("hd:FsWrite", "remove", one(&nested), vec![0]);
    p.call("hd:FsWrite", "remove", one(&missing), fs_error(&missing, 0));
    p.call(
        "hd:FsWrite",
        "create_dir_all",
        one(&binary),
        fs_error(&binary, 4),
    );
    p.call(
        "hd:FsWrite",
        "write_text",
        Enc::default().str(path(&area)).str("no").0,
        fs_error(&area, 5),
    );
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default().str(path(&missing)).str(path(&moved)).0,
        fs_error(&moved, 0),
    );
    let message = format!("ENOTEMPTY: directory not empty, rmdir '{}'", path(&area));
    p.call(
        "hd:FsWrite",
        "remove",
        one(&area),
        Enc::default().leb(1).leb(7).str(&message).0,
    );
    assert_program(&scratch, p, vec![], || {
        if area.exists() {
            fs::remove_dir_all(&area).unwrap();
        }
        fs::create_dir(&area).unwrap();
    });
}

#[test]
fn filesystem_grants_check_every_method_and_both_rename_paths() {
    let scratch = Scratch::new();
    let allowed = scratch.at("allowed");
    let outside = scratch.at("allowed-sibling");
    fs::create_dir(&allowed).unwrap();
    fs::create_dir(&outside).unwrap();
    let inside_file = allowed.join("file");
    let outside_file = outside.join("file");
    fs::write(&inside_file, "inside").unwrap();
    fs::write(&outside_file, "outside").unwrap();
    let traversal = allowed.join("../allowed-sibling/file");
    let mut p = Program::new();
    for method in ["read_bytes", "read_text", "list_dir", "stat"] {
        p.call(
            "hd:FsRead",
            method,
            one(&outside_file),
            fs_error(&outside_file, 2),
        );
    }
    for method in [
        "write_bytes",
        "write_text",
        "append_text",
        "create_dir_all",
        "remove",
    ] {
        let args = if method.contains("text") || method == "write_bytes" {
            Enc::default().str(path(&outside_file)).str("changed").0
        } else {
            one(&outside_file)
        };
        p.call("hd:FsWrite", method, args, fs_error(&outside_file, 2));
    }
    p.call(
        "hd:FsRead",
        "read_text",
        one(&traversal),
        fs_error(&traversal, 2),
    );
    p.call(
        "hd:FsWrite",
        "write_text",
        Enc::default().str(path(&traversal)).str("changed").0,
        fs_error(&traversal, 2),
    );
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default()
            .str(path(&outside_file))
            .str(path(&inside_file))
            .0,
        fs_error(&outside_file, 2),
    );
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default()
            .str(path(&inside_file))
            .str(path(&outside_file))
            .0,
        fs_error(&outside_file, 2),
    );
    p.call(
        "hd:FsRead",
        "read_text",
        one(&inside_file),
        Enc::default().leb(0).str("inside").0,
    );
    assert_program(
        &scratch,
        p,
        vec![
            ("FsRead", Limit::Entries(vec![path(&allowed).into()])),
            ("FsWrite", Limit::Entries(vec![path(&allowed).into()])),
        ],
        || {
            assert_eq!(fs::read_to_string(&inside_file).unwrap(), "inside");
            assert_eq!(fs::read_to_string(&outside_file).unwrap(), "outside");
        },
    );
    assert_eq!(fs::read_to_string(outside_file).unwrap(), "outside");
}

#[cfg(unix)]
#[test]
fn filesystem_grants_resolve_symlinks_and_report_symlink_entries() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new();
    let allowed = scratch.at("allowed");
    let outside = scratch.at("outside");
    fs::create_dir(&allowed).unwrap();
    fs::create_dir(&outside).unwrap();
    let file = allowed.join("file");
    let secret = outside.join("secret");
    fs::write(&file, "hello").unwrap();
    fs::write(&secret, "secret").unwrap();
    let good = allowed.join("good");
    let bad = allowed.join("bad");
    symlink(&file, &good).unwrap();
    symlink(&outside, &bad).unwrap();
    let escaped = bad.join("secret");
    let nonexistent = bad.join("new/child");
    let mut p = Program::new();
    p.call(
        "hd:FsRead",
        "read_text",
        one(&good),
        Enc::default().leb(0).str("hello").0,
    );
    p.call(
        "hd:FsRead",
        "stat",
        one(&good),
        Enc::default()
            .leb(0)
            .leb(1)
            .str(path(&good))
            .leb(2)
            .leb(path(&file).len() as u64)
            .0,
    );
    p.call(
        "hd:FsRead",
        "list_dir",
        one(&allowed),
        Enc::default()
            .leb(0)
            .leb(3)
            .str(&listed_path(&allowed, "bad"))
            .leb(2)
            .leb(path(&outside).len() as u64)
            .str(&listed_path(&allowed, "file"))
            .leb(0)
            .leb(5)
            .str(&listed_path(&allowed, "good"))
            .leb(2)
            .leb(path(&file).len() as u64)
            .0,
    );
    p.call(
        "hd:FsRead",
        "read_text",
        one(&escaped),
        fs_error(&escaped, 2),
    );
    p.call("hd:FsRead", "stat", one(&bad), fs_error(&bad, 2));
    p.call(
        "hd:FsWrite",
        "create_dir_all",
        one(&nonexistent),
        fs_error(&nonexistent, 2),
    );
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default().str(path(&file)).str(path(&escaped)).0,
        fs_error(&escaped, 2),
    );
    assert_program(
        &scratch,
        p,
        vec![
            ("FsRead", Limit::Entries(vec![path(&allowed).into()])),
            ("FsWrite", Limit::Entries(vec![path(&allowed).into()])),
        ],
        || {},
    );
    assert_eq!(fs::read_to_string(secret).unwrap(), "secret");
    assert!(!outside.join("new").exists());
}

#[cfg(unix)]
#[test]
fn fs_create_dir_broken_symlink_reports_not_found() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new();
    let broken = scratch.at("broken");
    symlink(scratch.at("missing-target"), &broken).unwrap();
    let mut p = Program::new();
    p.call(
        "hd:FsWrite",
        "create_dir_all",
        one(&broken),
        fs_error(&broken, 0),
    );
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn filesystem_invalid_paths_match_node_error_argument_names() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    let nul = "a\0b";
    let other = |argument: &str| {
        Enc::default().leb(1).leb(7).str(&format!(
        "The argument '{argument}' must be a string, Uint8Array, or URL without null bytes. Received 'a\\x00b'"
    )).0
    };
    p.call(
        "hd:FsRead",
        "read_text",
        Enc::default().str(nul).0,
        other("path"),
    );
    p.call(
        "hd:FsWrite",
        "write_text",
        Enc::default().str(nul).str("content").0,
        other("path"),
    );
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default().str(nul).str("to").0,
        other("oldPath"),
    );
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default().str("from").str(nul).0,
        other("newPath"),
    );
    p.call(
        "hd:FsWrite",
        "create_dir_all",
        Enc::default().str("").0,
        Enc::default().leb(1).leb(0).str("").0,
    );
    assert_program(&scratch, p, vec![], || {});
}

#[cfg(unix)]
#[test]
fn filesystem_long_paths_preserve_errno_and_syscall() {
    let scratch = Scratch::new();
    let too_long = scratch.at(&"x".repeat(300));
    let mut p = Program::new();
    for (module, method, syscall) in [
        ("hd:FsRead", "read_bytes", "open"),
        ("hd:FsRead", "stat", "lstat"),
        ("hd:FsRead", "list_dir", "scandir"),
        ("hd:FsWrite", "create_dir_all", "mkdir"),
    ] {
        let error = format!(
            "ENAMETOOLONG: name too long, {syscall} '{}'",
            path(&too_long)
        );
        p.call(
            module,
            method,
            one(&too_long),
            Enc::default().leb(1).leb(7).str(&error).0,
        );
    }
    assert_program(&scratch, p, vec![], || {});
}

#[cfg(target_os = "linux")]
#[test]
fn filesystem_write_failure_names_the_write_syscall_without_path() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    p.call(
        "hd:FsWrite",
        "write_bytes",
        Enc::default().str("/dev/full").bytes(b"full").0,
        Enc::default()
            .leb(1)
            .leb(7)
            .str("ENOSPC: no space left on device, write")
            .0,
    );
    assert_program(&scratch, p, vec![], || {});
}

#[cfg(unix)]
#[test]
fn filesystem_permission_denied_matches_node() {
    use std::os::unix::fs::PermissionsExt;
    struct RestorePermissions(PathBuf, fs::Permissions);
    impl Drop for RestorePermissions {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, self.1.clone());
        }
    }
    let scratch = Scratch::new();
    let locked = scratch.at("locked");
    fs::write(&locked, "locked").unwrap();
    let original = fs::metadata(&locked).unwrap().permissions();
    let _restore = RestorePermissions(locked.clone(), original);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o0)).unwrap();
    // A privileged test runner may legitimately bypass Unix mode bits.
    if fs::read(&locked).is_ok() {
        return;
    }
    let mut p = Program::new();
    p.call(
        "hd:FsRead",
        "read_bytes",
        one(&locked),
        fs_error(&locked, 1),
    );
    p.call("hd:FsRead", "read_text", one(&locked), fs_error(&locked, 1));
    for method in ["write_bytes", "write_text", "append_text"] {
        p.call(
            "hd:FsWrite",
            method,
            Enc::default().str(path(&locked)).str("changed").0,
            fs_error(&locked, 1),
        );
    }
    assert_program(&scratch, p, vec![], || {});
}

fn process_args(name: &str, argv: &[&str], input: &[u8]) -> Vec<u8> {
    let mut e = Enc::default().str(name).leb(argv.len() as u64);
    for arg in argv {
        e = e.str(arg);
    }
    e.bytes(input).0
}

#[test]
fn process_binary_input_lossy_outputs_status_and_missing_program() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    let script = "const fs=require('node:fs');process.stdout.write(fs.readFileSync(0));process.stderr.write(Buffer.from([255,0,195,169]));process.exitCode=23";
    p.call(
        "hd:Process",
        "run",
        process_args("node", &["-e", script], &[0, 255, 195, 169]),
        Enc::default().leb(0).str("\0�é").str("�\0é").leb(46).0,
    );
    p.call(
        "hd:Process",
        "run",
        process_args("hd-missing-program-parity-778811", &[], b""),
        vec![1, 0],
    );
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn process_drains_both_pipes_while_child_waits_for_large_input() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    let script = "const fs=require('node:fs');fs.writeSync(1,Buffer.alloc(70000,65));fs.writeSync(2,Buffer.alloc(70000,66));const b=fs.readFileSync(0);fs.writeSync(1,String(b.length))";
    let input = vec![42; 100_000];
    let out = format!("{}100000", "A".repeat(70_000));
    p.call(
        "hd:Process",
        "run",
        process_args("node", &["-e", script], &input),
        Enc::default()
            .leb(0)
            .str(&out)
            .str(&"B".repeat(70_000))
            .leb(0)
            .0,
    );
    assert_program(&scratch, p, vec![], || {});
}

#[cfg(unix)]
#[test]
fn process_child_closing_stdin_returns_epipe() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    p.call(
        "hd:Process",
        "run",
        process_args(
            "/bin/sh",
            &["-c", "exec 0<&-; sleep 0.1; exit 0"],
            &vec![42; 262_144],
        ),
        Enc::default()
            .leb(1)
            .leb(3)
            .str("spawnSync /bin/sh EPIPE")
            .0,
    );
    assert_program(&scratch, p, vec![], || {});
}

#[cfg(unix)]
#[test]
fn filesystem_sparse_file_size_bound_matches_node() {
    let scratch = Scratch::new();
    let file = scratch.at("too-large");
    // This allocates no file contents: both hosts must reject from metadata
    // before reading the sparse file or reserving a multi-GiB result.
    fs::File::create(&file)
        .unwrap()
        .set_len(0x8000_0000)
        .unwrap();
    let mut p = Program::new();
    for method in ["read_bytes", "read_text"] {
        p.call(
            "hd:FsRead",
            method,
            one(&file),
            Enc::default()
                .leb(1)
                .leb(7)
                .str("File size (2147483648) is greater than 2 GiB")
                .0,
        );
    }
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn process_scope_is_exact_and_refusal_precedes_lookup() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    p.call(
        "hd:Process",
        "run",
        process_args("node", &["-e", "process.stdout.write('ok')"], b""),
        Enc::default().leb(0).str("ok").str("").leb(0).0,
    );
    p.call(
        "hd:Process",
        "run",
        process_args("hd-missing-program-parity-778811", &[], b""),
        vec![1, 2],
    );
    p.call(
        "hd:Process",
        "run",
        process_args("node-other", &[], b""),
        vec![1, 2],
    );
    assert_program(
        &scratch,
        p,
        vec![("Process", Limit::Entries(vec!["node".into()]))],
        || {},
    );
}

#[cfg(unix)]
#[test]
fn process_signal_status_and_permission_denied() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let script = scratch.at("not-executable");
    fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o600)).unwrap();
    let mut p = Program::new();
    p.call(
        "hd:Process",
        "run",
        process_args("node", &["-e", "process.kill(process.pid,'SIGTERM')"], b""),
        Enc::default().leb(0).str("").str("").leb(256).0,
    );
    p.call(
        "hd:Process",
        "run",
        process_args(path(&script), &[], b""),
        vec![1, 1],
    );
    assert_program(&scratch, p, vec![], || {});
}

#[cfg(unix)]
#[test]
fn process_executable_without_shebang_matches_node_platform_behavior() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let script = scratch.at("plain-executable");
    fs::write(
        &script,
        "printf 'out:%s' \"$1\"\nprintf 'err' >&2\nexit 19\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let mut p = Program::new();
    // Node's spawn behavior depends on its platform/libuv version. Use
    // the identical Node invocation as the oracle, including ENOEXEC.
    p.call(
        "hd:Process",
        "run",
        process_args(path(&script), &["argument with spaces"], b""),
        vec![],
    );
    let (wasm, _) = p.finish(7);
    let result = parity(&scratch, &wasm, vec![], || {});
    assert_eq!(result.status, 7);
    assert_eq!(result.stdout, b"core-parity\n");
    assert!(result.stderr.starts_with(b"00") || result.stderr.starts_with(b"0103"));
}

#[test]
fn process_invalid_names_and_arguments_return_typed_errors() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    p.call("hd:Process", "run", process_args("", &[], b""), vec![1, 0]);
    for (name, args, argument) in [("a\0b", vec![], "file"), ("node", vec!["a\0b"], "args[0]")] {
        let message = format!(
            "The argument '{argument}' must be a string without null bytes. Received 'a\\x00b'"
        );
        p.call(
            "hd:Process",
            "run",
            process_args(name, &args, b""),
            Enc::default().leb(1).leb(3).str(&message).0,
        );
    }
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn process_grant_refusal_precedes_invalid_name_and_argument_validation() {
    let scratch = Scratch::new();
    for (name, args) in [("", vec![]), ("a\0b", vec![]), ("node", vec!["a\0b"])] {
        let bytes = process_args(name, &args, b"");
        // Refusal must occur before spawnSync validates its program/argv.
        let mut p = Program::new();
        p.call("hd:Process", "run", bytes, vec![1, 2]);
        assert_program(
            &scratch,
            p,
            vec![("Process", Limit::Entries(vec![]))],
            || {},
        );
    }
}

#[test]
fn repeated_identical_import_declarations_link_on_both_engines() {
    let scratch = Scratch::new();
    let file = scratch.at("file");
    fs::write(&file, "same").unwrap();
    let mut p = Program::new();
    for _ in 0..2 {
        p.call(
            "hd:FsRead",
            "read_text",
            one(&file),
            Enc::default().leb(0).str("same").0,
        );
        // Deliberately emit two import declarations of the same function.
        p.imported
            .remove(&("hd:FsRead".into(), "read_text.start".into()));
    }
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn synchronous_provider_finish_rejects_stale_handles() {
    let scratch = Scratch::new();
    for (module, methods) in [
        (
            "hd:FsRead",
            vec!["read_bytes", "read_text", "list_dir", "stat"],
        ),
        (
            "hd:FsWrite",
            vec![
                "write_bytes",
                "write_text",
                "append_text",
                "create_dir_all",
                "remove",
                "rename",
            ],
        ),
        ("hd:Process", vec!["run"]),
    ] {
        for method in methods {
            let mut p = Program::new();
            let finish = p.import(
                module,
                &format!("{method}.finish"),
                &[ValType::I32],
                &[ValType::I32],
            );
            p.instructions(&[I::I32Const(123_456), I::Call(finish), I::Drop]);
            let (wasm, _) = p.finish(0);
            let result = parity(&scratch, &wasm, vec![], || {});
            assert_eq!(result.status, 3);
            assert_eq!(
                result.stderr,
                b"panic: host-contract: a finish of an operation that is not pending\n"
            );
        }
    }
}

#[test]
fn malformed_structured_arguments_report_host_contract() {
    let scratch = Scratch::new();
    for (module, method, bytes) in [
        ("hd:FsRead", "read_bytes", vec![5, b'x']),
        ("hd:FsWrite", "rename", vec![1, b'x']),
        ("hd:Process", "run", vec![4, b'n']),
    ] {
        let mut p = Program::new();
        let call = p.import(
            module,
            &format!("{method}.start"),
            &[ValType::I32],
            &[ValType::I32, ValType::I32],
        );
        p.load_args(&bytes);
        p.instructions(&[
            I::I32Const(i32::try_from(bytes.len()).unwrap()),
            I::Call(call),
            I::Drop,
            I::Drop,
        ]);
        let (wasm, _) = p.finish(0);
        let result = parity(&scratch, &wasm, vec![], || {});
        assert_eq!(result.status, 3);
        assert_eq!(
            result.stderr,
            b"panic: host-contract: an argument does not decode\n"
        );
    }
    // fsOp checks its path grant before decoding the write payload.
    let outside = scratch.at("not-granted");
    let mut p = Program::new();
    p.call(
        "hd:FsWrite",
        "write_bytes",
        one(&outside),
        fs_error(&outside, 2),
    );
    assert_program(
        &scratch,
        p,
        vec![("FsWrite", Limit::Entries(vec![]))],
        || {},
    );
    assert!(!outside.exists());
}

#[test]
fn sys_all_reads_match_and_scope_refusals_name_the_method() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    for method in ["os", "arch", "hostname", "cpu_count"] {
        p.sys(method);
    }
    let (wasm, _) = p.finish(0);
    let result = parity(&scratch, &wasm, vec![], || {});
    assert_eq!(result.status, 0);
    let stderr = std::str::from_utf8(&result.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 4);
    assert!(
        stderr
            .lines()
            .all(|line| line.starts_with("00") && line.len() > 2)
    );
    for allowed in [vec![], vec!["os".into()]] {
        let mut p = Program::new();
        for method in ["os", "arch", "hostname", "cpu_count"] {
            if allowed.iter().any(|name| name == method) {
                continue;
            }
            p.sys(method);
            p.expected.push(Enc::default().leb(1).leb(0).str(method).0);
        }
        assert_program(&scratch, p, vec![("Sys", Limit::Entries(allowed))], || {});
    }
}

#[test]
fn random_fill_sizes_and_next_u64_have_observable_entropy() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    let fill = p.import("hd:Random", "fill", &[ValType::I32], &[ValType::I32]);
    let next = p.import("hd:Random", "next_u64", &[], &[ValType::I64]);
    for (count, encoded) in [(0, 1), (1, 2), (64, 65), (128, 130)] {
        p.instructions(&[
            I::I32Const(count),
            I::Call(fill),
            I::I32Const(encoded),
            I::I32Eq,
            I::LocalSet(2),
            I::LocalGet(2),
            I::I32Const(0),
            I::I32Load8U(MEM),
            I::I32Const(if count < 128 { count } else { 128 }),
            I::I32Eq,
            I::I32And,
            I::LocalSet(2),
        ]);
        if count == 128 {
            p.instructions(&[
                I::LocalGet(2),
                I::I32Const(1),
                I::I32Load8U(MEM),
                I::I32Const(1),
                I::I32Eq,
                I::I32And,
                I::LocalSet(2),
            ]);
        }
        p.instructions(&[
            I::I32Const(0),
            I::LocalGet(2),
            I::I32Store8(MEM),
            I::I32Const(1),
            I::LocalSet(0),
        ]);
        p.dump();
        p.expected.push(vec![1]);
    }
    // No random bytes are compared between engines. Assert 64 bytes are
    // not all zero (failure probability 2^-512) and two u64s differ (2^-64).
    p.instructions(&[
        I::I32Const(64),
        I::Call(fill),
        I::Drop,
        I::I32Const(1),
        I::LocalSet(0),
        I::I32Const(0),
        I::LocalSet(2),
        I::Block(BlockType::Empty),
        I::Loop(BlockType::Empty),
        I::LocalGet(2),
        I::LocalGet(0),
        I::I32Load8U(MEM),
        I::I32Or,
        I::LocalSet(2),
        I::LocalGet(0),
        I::I32Const(1),
        I::I32Add,
        I::LocalTee(0),
        I::I32Const(65),
        I::I32LtU,
        I::BrIf(0),
        I::End,
        I::End,
        I::I32Const(0),
        I::LocalGet(2),
        I::I32Eqz,
        I::I32Eqz,
        I::I32Store8(MEM),
        I::I32Const(1),
        I::LocalSet(0),
    ]);
    p.dump();
    p.expected.push(vec![1]);
    p.instructions(&[
        I::Call(next),
        I::LocalSet(3),
        I::Call(next),
        I::LocalSet(4),
        I::I32Const(0),
        I::LocalGet(3),
        I::LocalGet(4),
        I::I64Ne,
        I::I32Store8(MEM),
        I::I32Const(1),
        I::LocalSet(0),
    ]);
    p.dump();
    p.expected.push(vec![1]);
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn filesystem_listing_uses_utf8_byte_order_and_preserves_trailing_slash() {
    let scratch = Scratch::new();
    let dir = scratch.at("listing");
    fs::create_dir(&dir).unwrap();
    for name in ["ä", "z", "A"] {
        fs::write(dir.join(name), b"x").unwrap();
    }
    let mut expected = Enc::default().leb(0).leb(3);
    for name in ["A", "z", "ä"] {
        expected = expected.str(&listed_path(&dir, name)).leb(0).leb(1);
    }
    let mut p = Program::new();
    p.call(
        "hd:FsRead",
        "list_dir",
        Enc::default().str(&format!("{}/", path(&dir))).0,
        expected.0,
    );
    assert_program(&scratch, p, vec![], || {});
}
