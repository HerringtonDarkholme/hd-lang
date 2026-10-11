#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::io::{Read, Write as IoWrite};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use hd_run::{Grants, HostSetup, Limit, Limits};
use hd_run_wasmtime::{Captured, EngineConfig, run_captured};
use wasm_encoder::{
    BlockType, CodeSection, DataSection, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, ImportSection, Instruction as I, MemArg, MemorySection, MemoryType, Module,
    TypeSection, ValType,
};

const ARG_BASE: i32 = 1 << 20;
const HEX_BASE: i32 = 1 << 19;
pub const MEM: MemArg = MemArg {
    offset: 0,
    align: 0,
    memory_index: 0,
};

// An unrelated test's child can briefly inherit CLOEXEC socket descriptors
// between process creation and exec. Resource-release tests hold this same
// lock so peer EOF observes only descriptors owned by the instance.
pub static CHILD_SPAWN: Mutex<()> = Mutex::new(());

/// Reserves a TCP port that refuses unrelated connections without releasing
/// it for another parallel fixture to claim. Keep both established endpoints
/// alive: a bound but unlistening socket would time out instead on Darwin.
pub struct RefusedPort {
    pub address: std::net::SocketAddr,
    _connection: [TcpStream; 2],
}

impl RefusedPort {
    pub fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client =
            TcpStream::connect_timeout(&listener.local_addr().unwrap(), Duration::from_secs(1))
                .unwrap();
        let (peer, _) = listener.accept().unwrap();
        Self {
            address: client.local_addr().unwrap(),
            _connection: [client, peer],
        }
    }
}

/// Exchange-buffer encoding, independent of the implementation's codec.
#[derive(Default)]
pub struct Enc(pub Vec<u8>);

impl Enc {
    pub fn leb(mut self, mut value: u64) -> Self {
        loop {
            let byte = (value & 127) as u8;
            value >>= 7;
            self.0.push(byte | if value == 0 { 0 } else { 128 });
            if value == 0 {
                return self;
            }
        }
    }

    pub fn bytes(mut self, value: &[u8]) -> Self {
        self = self.leb(value.len() as u64);
        self.0.extend_from_slice(value);
        self
    }

    pub fn str(self, value: &str) -> Self {
        self.bytes(value.as_bytes())
    }
}

pub fn path(p: &Path) -> &str {
    p.to_str().expect("UTF-8 temporary path")
}

// Both providers append a literal slash to the requested directory, even
// when that directory uses native Windows path separators.
pub fn listed_path(dir: &Path, name: &str) -> String {
    let directory = path(dir);
    let slash = if directory.ends_with('/') { "" } else { "/" };
    format!("{directory}{slash}{name}")
}

pub fn fs_error(p: &Path, variant: u64) -> Vec<u8> {
    Enc::default().leb(1).leb(variant).str(path(p)).0
}

/// Tiny modules use the real import signatures and exports. Results are
/// rendered as hex so binary values, enum tags and lengths all remain visible.
pub struct Program {
    types: TypeSection,
    imports: ImportSection,
    import_count: u32,
    pub imported: BTreeMap<(String, String), u32>,
    type_count: u32,
    data: DataSection,
    next_data: i32,
    body: Vec<I<'static>>,
    pub expected: Vec<Vec<u8>>,
}

impl Program {
    pub fn new() -> Self {
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

    pub fn ty(&mut self, params: &[ValType], results: &[ValType]) -> u32 {
        let index = self.type_count;
        self.types.ty().function(params.to_vec(), results.to_vec());
        self.type_count += 1;
        index
    }

    pub fn import(
        &mut self,
        module: &str,
        name: &str,
        params: &[ValType],
        results: &[ValType],
    ) -> u32 {
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

    pub fn instructions(&mut self, instructions: &[I<'static>]) {
        self.body.extend_from_slice(instructions);
    }

    pub fn load_args(&mut self, bytes: &[u8]) {
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

    pub fn call(&mut self, module: &str, method: &str, args: impl AsRef<[u8]>, expected: Vec<u8>) {
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

    pub fn sys(&mut self, method: &str) {
        let index = self.import("hd:Sys", method, &[], &[ValType::I32]);
        self.instructions(&[I::Call(index), I::LocalSet(0)]);
        self.dump();
    }

    /// ABI call with scalar arguments followed by an optional exchange
    /// buffer argument. Waiting calls must complete synchronously here.
    pub fn abi_call(
        &mut self,
        module: &str,
        method: &str,
        scalars: &[i32],
        payload: Option<&[u8]>,
        waits: bool,
        expected: Vec<u8>,
    ) {
        let name = if waits {
            format!("{method}.start")
        } else {
            method.into()
        };
        let params = vec![ValType::I32; scalars.len() + usize::from(payload.is_some())];
        let results = if waits {
            vec![ValType::I32, ValType::I32]
        } else {
            vec![ValType::I32]
        };
        let index = self.import(module, &name, &params, &results);
        if let Some(bytes) = payload {
            self.load_args(bytes);
        }
        for scalar in scalars {
            self.instructions(&[I::I32Const(*scalar)]);
        }
        if let Some(bytes) = payload {
            self.instructions(&[I::I32Const(i32::try_from(bytes.len()).unwrap())]);
        }
        self.instructions(&[I::Call(index), I::LocalSet(0)]);
        if waits {
            self.instructions(&[
                I::LocalSet(1),
                I::LocalGet(1),
                I::If(BlockType::Empty),
                I::Unreachable,
                I::End,
            ]);
        }
        self.dump();
        self.expected.push(expected);
    }

    pub fn dump(&mut self) {
        // The dump function is the first defined function, whose index is
        // unknown until all imports have been appended. Patch it in finish.
        self.instructions(&[I::LocalGet(0), I::Call(u32::MAX)]);
    }

    pub fn finish(mut self, status: i32) -> (Vec<u8>, String) {
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

pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> Self {
        static ID: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "hd-core-parity-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        Self(fs::canonicalize(dir).unwrap())
    }

    pub fn at(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn parity(
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
    let wasm_owned = wasm.to_vec();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(run_captured(EngineConfig::default(), &wasm_owned, &setup));
    });
    let rust = receiver
        .recv_timeout(Duration::from_secs(20))
        .expect("wasmtime finishes within the fixture deadline")
        .expect("wasmtime runs ABI module");
    reset();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("host/run.mjs");
    let spawn = CHILD_SPAWN.lock().unwrap();
    let mut node = Command::new("node")
        .arg(root)
        .arg(file)
        .arg(config)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Node host starts");
    drop(spawn);
    let mut stdout = node.stdout.take().unwrap();
    let mut stderr = node.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut b = Vec::new();
        stdout.read_to_end(&mut b).unwrap();
        b
    });
    let err = std::thread::spawn(move || {
        let mut b = Vec::new();
        stderr.read_to_end(&mut b).unwrap();
        b
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = node.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            node.kill().unwrap();
            node.wait().unwrap();
            panic!("Node finishes within the fixture deadline");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let node_stdout = out.join().unwrap();
    let node_stderr = err.join().unwrap();
    assert_eq!(
        rust.status,
        u8::try_from(status.code().expect("Node exits normally")).unwrap(),
        "Node stderr: {}",
        String::from_utf8_lossy(&node_stderr)
    );
    assert_eq!(rust.stdout, node_stdout, "stdout differs");
    assert_eq!(rust.stderr, node_stderr, "stderr differs");
    rust
}

pub fn assert_program(scratch: &Scratch, p: Program, limits: Vec<(&str, Limit)>, reset: impl Fn()) {
    let (wasm, expected) = p.finish(7);
    let result = parity(scratch, &wasm, limits, reset);
    assert_eq!(result.status, 7);
    assert_eq!(result.stdout, b"core-parity\n");
    assert_eq!(result.stderr, expected.as_bytes());
}

pub fn one(p: &Path) -> Vec<u8> {
    Enc::default().str(path(p)).0
}

/// Loopback-only server with bounded connections and an explicit wake-up
/// for its accept loop. Its address stays fixed for both engine runs.
pub struct TcpFixture {
    pub address: std::net::SocketAddr,
    stopped: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TcpFixture {
    pub fn new(handler: impl Fn(TcpStream) + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = stopped.clone();
        let thread = std::thread::spawn(move || {
            for incoming in listener.incoming() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let stream = incoming.unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                handler(stream);
            }
        });
        Self {
            address,
            stopped,
            thread: Some(thread),
        }
    }
}

impl Drop for TcpFixture {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_secs(1));
        if let Some(thread) = self.thread.take()
            && let Err(error) = thread.join()
            && !std::thread::panicking()
        {
            std::panic::resume_unwind(error);
        }
    }
}

#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn values(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    }
}

pub struct HttpFixture {
    server: TcpFixture,
    pub seen: Arc<Mutex<Vec<HttpRequest>>>,
}

impl HttpFixture {
    pub fn new(reply: impl Fn(&HttpRequest) -> (Vec<u8>, Duration) + Send + 'static) -> Self {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let records = seen.clone();
        let server = TcpFixture::new(move |mut stream| {
            let Some(request) = read_http_request(&mut stream) else {
                return;
            };
            let (response, delay) = reply(&request);
            records.lock().unwrap().push(request);
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
            let _ = stream.write_all(&response);
        });
        Self { server, seen }
    }

    pub fn port(&self) -> u16 {
        self.server.address.port()
    }
    pub fn url(&self, target: &str) -> String {
        format!("http://127.0.0.1:{}{target}", self.port())
    }
}

fn read_http_request(stream: &mut TcpStream) -> Option<HttpRequest> {
    let mut raw = Vec::new();
    let boundary = loop {
        let mut bytes = [0; 4096];
        let n = stream.read(&mut bytes).ok()?;
        if n == 0 {
            return None;
        }
        raw.extend_from_slice(&bytes[..n]);
        if let Some(at) = raw.windows(4).position(|b| b == b"\r\n\r\n") {
            break at + 4;
        }
        assert!(raw.len() < 65536, "bounded fixture request headers");
    };
    let text = std::str::from_utf8(&raw[..boundary]).ok()?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(n, v)| (n.to_owned(), v.trim().to_owned()))
        .collect();
    let chunked = headers.iter().any(|(n, value)| {
        n.eq_ignore_ascii_case("transfer-encoding")
            && value
                .split(',')
                .any(|v| v.trim().eq_ignore_ascii_case("chunked"))
    });
    if chunked {
        let mut at = boundary;
        let mut body = Vec::new();
        loop {
            let end = loop {
                if let Some(n) = raw[at..].windows(2).position(|bytes| bytes == b"\r\n") {
                    break at + n;
                }
                let next = raw.len() + 1;
                fill_http(stream, &mut raw, next)?;
            };
            let size = usize::from_str_radix(std::str::from_utf8(&raw[at..end]).ok()?, 16).ok()?;
            assert!(size <= 262_144 - body.len(), "bounded fixture chunk body");
            at = end + 2;
            fill_http(stream, &mut raw, at + size + 2)?;
            if raw[at + size..at + size + 2] != *b"\r\n" {
                return None;
            }
            body.extend_from_slice(&raw[at..at + size]);
            at += size + 2;
            if size == 0 {
                return Some(HttpRequest {
                    method,
                    target,
                    headers,
                    body,
                });
            }
        }
    }
    let length = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    assert!(length <= 262_144, "bounded fixture body");
    fill_http(stream, &mut raw, boundary + length)?;
    Some(HttpRequest {
        method,
        target,
        headers,
        body: raw[boundary..boundary + length].to_vec(),
    })
}

fn fill_http(stream: &mut TcpStream, raw: &mut Vec<u8>, length: usize) -> Option<()> {
    while raw.len() < length {
        let mut bytes = [0; 4096];
        let n = stream.read(&mut bytes).ok()?;
        if n == 0 {
            return None;
        }
        raw.extend_from_slice(&bytes[..n]);
    }
    Some(())
}
