//! The execution journal (`future-work/live-execution.md` §4): rows, their framing, a
//! torn-tail-tolerant reader, the payload of each row kind and a
//! [`Recorder`] that an engine's host import dispatch writes rows through.
//! [`crate::replay`] serves a recorded input's host calls again (§5);
//! resume (§6) is not built.
//!
//! `journal = Header Row*`; `Row = kind: u8, index: uleb, len: uleb,
//! payload, crc32: u32`.
//!
//! Payloads, in the order of their fields (`uleb` is unsigned LEB128):
//!
//! | Row | Payload |
//! | --- | --- |
//! | `Header` | format: uleb, execution id: 16 bytes |
//! | `Input` | input number: uleb, source hash: 16 bytes, module hash: 16 bytes |
//! | `Call` | method: uleb, [module and name when the method is new], request, reply |
//! | `Finish` | handle: uleb, reply |
//! | `Wake` | poll count: uleb, handle count: uleb, handles: uleb each |
//! | `Block` | handle count: uleb, handles: uleb each |
//! | `Abort` | handle: uleb |
//! | `Cancel` | poll count: uleb |
//! | `End` | outcome: u8, then an exit's status: u8 |
//!
//! A method is a host import, numbered by its first `Call` in the journal:
//! the row whose method number equals the count of methods named so far
//! names it, module then name, each as a uleb length and UTF-8 bytes. A
//! request is the import's arguments, its scalar parameters (integers as
//! uleb of their two's complement bits, floats as 8 bytes) then the
//! exchange buffer bytes it read: a uleb length, then the bytes when there
//! are 8 or fewer, else their [`request_digest`]. So output (`Console`
//! lines, standard error) is recorded by digest, never as text. A reply is
//! a tag byte (the [`Value`] kind, plus 4 when result bytes follow), the
//! value, then the result bytes as a uleb length and the bytes. Large
//! results stay inline in memory; the blob store of §4.3 comes with
//! journal files.

/// Row kinds (§4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RowKind {
    Header,
    Input,
    Call,
    Finish,
    Wake,
    Block,
    Abort,
    Cancel,
    End,
}

impl RowKind {
    const ALL: [RowKind; 9] = [
        RowKind::Header,
        RowKind::Input,
        RowKind::Call,
        RowKind::Finish,
        RowKind::Wake,
        RowKind::Block,
        RowKind::Abort,
        RowKind::Cancel,
        RowKind::End,
    ];
}

/// One row: its kind, event index and payload bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: RowKind,
    pub index: u64,
    pub payload: Vec<u8>,
}

/// Large payloads go to a blob store by content hash (§4.3).
pub const BLOB_THRESHOLD: usize = 4096;

/// The format number of a `Header` row.
pub const FORMAT: u64 = 1;

/// A CLI REPL session's journal cap (§4.6): past it, recording stops.
pub const SESSION_CAP: usize = 64 << 20;

/// The CRC-32 (IEEE) table, one entry per byte value.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut c = i;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
            bit += 1;
        }
        table[i as usize] = c;
        i += 1;
    }
    table
};

/// CRC-32 (IEEE): the journal checksum.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c = CRC_TABLE[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

pub fn put_uleb(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = u8::try_from(v & 0x7f).expect("7 bits");
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// The number of bytes `put_uleb` writes for `v`.
fn uleb_len(v: u64) -> usize {
    (64 - v.max(1).leading_zeros() as usize).div_ceil(7)
}

fn get_uleb(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b.get(*at)?;
        *at += 1;
        v |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// Appends one framed row.
pub fn append(out: &mut Vec<u8>, row: &Row) {
    append_parts(out, row.kind, row.index, &row.payload);
}

fn append_parts(out: &mut Vec<u8>, kind: RowKind, index: u64, payload: &[u8]) {
    let start = out.len();
    out.push(kind as u8);
    put_uleb(out, index);
    put_uleb(out, payload.len() as u64);
    out.extend_from_slice(payload);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_le_bytes());
}

/// Reads rows until the end or the first torn or corrupt row; returns the
/// rows and the length of the valid prefix, where a writer resumes (§4.3).
#[must_use]
pub fn read(b: &[u8]) -> (Vec<Row>, usize) {
    let mut rows = Vec::new();
    let mut at = 0;
    loop {
        let start = at;
        let Some(&k) = b.get(at) else {
            return (rows, start);
        };
        at += 1;
        let parsed = (|| {
            let kind = *RowKind::ALL.get(k as usize)?;
            let index = get_uleb(b, &mut at)?;
            let len = usize::try_from(get_uleb(b, &mut at)?).ok()?;
            let payload = b.get(at..at + len)?.to_vec();
            at += len;
            let crc = u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?);
            (crc == crc32(&b[start..at])).then_some(Row {
                kind,
                index,
                payload,
            })
        })();
        match parsed {
            Some(r) if r.index == rows.len() as u64 => {
                at += 4;
                rows.push(r);
            }
            _ => return (rows, start),
        }
    }
}

/// A 64-bit digest of a request's argument bytes (§4.2). The method is
/// compared on its own, by the row's method number.
#[must_use]
pub fn request_digest(args: &[u8]) -> u64 {
    let mut h = hd_base::StableHasher::new("req");
    h.bytes(args);
    u64::try_from(h.finish().0 & u128::from(u64::MAX)).expect("low half")
}

/// A host import's scalar result, as the import returned it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    Void,
    Int(i64),
    /// The IEEE 754 bits of an `f64`.
    F64(u64),
    /// Status and value: `(1, handle)` is pending on an operation, `(0,
    /// length)` ready with that many result bytes.
    Pair(i32, i32),
}

impl Value {
    fn tag(self) -> u8 {
        match self {
            Value::Void => 0,
            Value::Int(_) => 1,
            Value::F64(_) => 2,
            Value::Pair(..) => 3,
        }
    }
}

/// A host call's arguments: as they are when 8 bytes or fewer, else their
/// length and digest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Args(Vec<u8>),
    Digest { len: u64, digest: u64 },
}

impl Request {
    /// The request a `Call` row holds for the arguments `args`.
    #[must_use]
    pub fn of(args: &[u8]) -> Request {
        if args.len() <= 8 {
            Request::Args(args.to_vec())
        } else {
            Request::Digest {
                len: args.len() as u64,
                digest: request_digest(args),
            }
        }
    }
}

/// Appends an integer parameter to a call's arguments: the uleb of its
/// two's complement bits. Then come float parameters ([`put_float_arg`])
/// in their order among the parameters, and last the exchange buffer bytes
/// the call reads, as they are.
pub fn put_int_arg(args: &mut Vec<u8>, v: i64) {
    put_uleb(args, u64::from_le_bytes(v.to_le_bytes()));
}

/// Appends a float parameter to a call's arguments: its IEEE 754 bits.
pub fn put_float_arg(args: &mut Vec<u8>, bits: u64) {
    args.extend_from_slice(&bits.to_le_bytes());
}

/// How an input ended, in its `End` row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// It ended with this status: 0, or the program's own.
    Exit(u8),
    /// It panicked or trapped (status 3).
    Trapped,
    /// A stop interrupted it while Wasm ran (`cli.repl.stop`).
    Stopped,
    /// A stop cancelled it while it waited on the host (`cli.repl.cancel`).
    Cancelled,
    /// The engine could not run it to its end: an internal error.
    Failed,
}

/// One decoded row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Header {
        format: u64,
        execution: u128,
    },
    Input {
        number: u32,
        source: u128,
        module: u128,
    },
    Call {
        module: String,
        name: String,
        request: Request,
        value: Value,
        result: Option<Vec<u8>>,
    },
    Finish {
        handle: u32,
        value: Value,
        result: Option<Vec<u8>>,
    },
    Wake {
        poll: u64,
        handles: Vec<u32>,
    },
    Block {
        handles: Vec<u32>,
    },
    Abort {
        handle: u32,
    },
    Cancel {
        poll: u64,
    },
    End(End),
}

/// Decodes the rows of one journal, in order, or names the first row whose
/// payload does not decode.
pub fn decode(rows: &[Row]) -> Result<Vec<Event>, String> {
    let mut methods: Vec<(String, String)> = Vec::new();
    rows.iter()
        .map(|row| {
            decode_row(row, &mut methods)
                .ok_or_else(|| format!("row {} ({:?}) does not decode", row.index, row.kind))
        })
        .collect()
}

fn decode_row(row: &Row, methods: &mut Vec<(String, String)>) -> Option<Event> {
    let b = &row.payload;
    let mut at = 0;
    let at = &mut at;
    let u32_of = |v: u64| u32::try_from(v).ok();
    let event = match row.kind {
        RowKind::Header => Event::Header {
            format: get_uleb(b, at)?,
            execution: get_u128(b, at)?,
        },
        RowKind::Input => Event::Input {
            number: u32_of(get_uleb(b, at)?)?,
            source: get_u128(b, at)?,
            module: get_u128(b, at)?,
        },
        RowKind::Call => {
            let method = usize::try_from(get_uleb(b, at)?).ok()?;
            if method == methods.len() {
                let module = get_text(b, at)?;
                let name = get_text(b, at)?;
                methods.push((module, name));
            }
            let (module, name) = methods.get(method)?.clone();
            let len = get_uleb(b, at)?;
            let request = if len <= 8 {
                Request::Args(get_bytes(b, at, usize::try_from(len).ok()?)?)
            } else {
                Request::Digest {
                    len,
                    digest: u64::from_le_bytes(get_bytes(b, at, 8)?.try_into().ok()?),
                }
            };
            let (value, result) = get_reply(b, at)?;
            Event::Call {
                module,
                name,
                request,
                value,
                result,
            }
        }
        RowKind::Finish => {
            let handle = u32_of(get_uleb(b, at)?)?;
            let (value, result) = get_reply(b, at)?;
            Event::Finish {
                handle,
                value,
                result,
            }
        }
        RowKind::Wake => Event::Wake {
            poll: get_uleb(b, at)?,
            handles: get_handles(b, at)?,
        },
        RowKind::Block => Event::Block {
            handles: get_handles(b, at)?,
        },
        RowKind::Abort => Event::Abort {
            handle: u32_of(get_uleb(b, at)?)?,
        },
        RowKind::Cancel => Event::Cancel {
            poll: get_uleb(b, at)?,
        },
        RowKind::End => Event::End(match get_bytes(b, at, 1)?[0] {
            0 => End::Exit(get_bytes(b, at, 1)?[0]),
            1 => End::Trapped,
            2 => End::Stopped,
            3 => End::Cancelled,
            4 => End::Failed,
            _ => return None,
        }),
    };
    (*at == b.len()).then_some(event)
}

fn get_bytes(b: &[u8], at: &mut usize, len: usize) -> Option<Vec<u8>> {
    let bytes = b.get(*at..at.checked_add(len)?)?.to_vec();
    *at += len;
    Some(bytes)
}

fn get_u128(b: &[u8], at: &mut usize) -> Option<u128> {
    Some(u128::from_le_bytes(get_bytes(b, at, 16)?.try_into().ok()?))
}

fn get_text(b: &[u8], at: &mut usize) -> Option<String> {
    let len = usize::try_from(get_uleb(b, at)?).ok()?;
    String::from_utf8(get_bytes(b, at, len)?).ok()
}

fn get_handles(b: &[u8], at: &mut usize) -> Option<Vec<u32>> {
    let n = get_uleb(b, at)?;
    (0..n)
        .map(|_| u32::try_from(get_uleb(b, at)?).ok())
        .collect()
}

fn get_reply(b: &[u8], at: &mut usize) -> Option<(Value, Option<Vec<u8>>)> {
    let tag = get_bytes(b, at, 1)?[0];
    let low = |v: u64| i32::from_le_bytes(v.to_le_bytes()[..4].try_into().unwrap_or([0; 4]));
    let value = match tag & 3 {
        0 => Value::Void,
        1 => Value::Int(i64::from_le_bytes(get_uleb(b, at)?.to_le_bytes())),
        2 => Value::F64(u64::from_le_bytes(get_bytes(b, at, 8)?.try_into().ok()?)),
        _ => Value::Pair(low(get_uleb(b, at)?), low(get_uleb(b, at)?)),
    };
    let result = match tag >> 2 {
        0 => None,
        1 => {
            let len = usize::try_from(get_uleb(b, at)?).ok()?;
            Some(get_bytes(b, at, len)?)
        }
        _ => return None,
    };
    Some((value, result))
}

fn put_reply(out: &mut Vec<u8>, value: Value, result: Option<&[u8]>) {
    out.push(value.tag() | if result.is_some() { 4 } else { 0 });
    let bits = |v: i32| u64::from(u32::from_le_bytes(v.to_le_bytes()));
    match value {
        Value::Void => {}
        Value::Int(v) => put_uleb(out, u64::from_le_bytes(v.to_le_bytes())),
        Value::F64(bits) => out.extend_from_slice(&bits.to_le_bytes()),
        Value::Pair(status, v) => {
            put_uleb(out, bits(status));
            put_uleb(out, bits(v));
        }
    }
    if let Some(bytes) = result {
        put_uleb(out, bytes.len() as u64);
        out.extend_from_slice(bytes);
    }
}

fn put_handles(out: &mut Vec<u8>, handles: &[u32]) {
    put_uleb(out, handles.len() as u64);
    for &h in handles {
        put_uleb(out, u64::from(h));
    }
}

/// Writes one execution's journal in memory, as its host calls and driver
/// events happen (§4.1): the engine's import dispatch calls one of
/// [`call`], [`finish`], [`block`] or [`abort`] per host call; its driver
/// loop calls [`poll`], [`wake`] and [`cancel`]; a REPL session [`input`]
/// and [`end`] around each input.
///
/// Past its cap, recording stops for good and the journal is dropped: the
/// execution can no longer be rebuilt. [`take_notice`] then answers true
/// once, for the one message that says so.
///
/// [`call`]: Recorder::call
/// [`finish`]: Recorder::finish
/// [`block`]: Recorder::block
/// [`abort`]: Recorder::abort
/// [`poll`]: Recorder::poll
/// [`wake`]: Recorder::wake
/// [`cancel`]: Recorder::cancel
/// [`input`]: Recorder::input
/// [`end`]: Recorder::end
/// [`take_notice`]: Recorder::take_notice
#[derive(Debug)]
pub struct Recorder {
    bytes: Vec<u8>,
    cap: usize,
    /// The next row's event index.
    next: u64,
    /// Per call site of the engine, its method number plus one; 0 until
    /// its first call names it.
    sites: Vec<u32>,
    /// The number of methods named so far.
    methods: u32,
    /// The payload of the row being written.
    row: Vec<u8>,
    /// The polls of the input that runs, for `Wake` and `Cancel` rows.
    polls: u64,
    state: State,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    On,
    /// Past the cap; whether the notice was taken.
    Off {
        told: bool,
    },
}

impl Recorder {
    /// A journal of at most `cap` bytes, which starts with its `Header`
    /// row for the execution `execution`.
    #[must_use]
    pub fn new(cap: usize, execution: u128) -> Recorder {
        let mut recorder = Recorder {
            bytes: Vec::new(),
            cap,
            next: 0,
            sites: Vec::new(),
            methods: 0,
            row: Vec::new(),
            polls: 0,
            state: State::On,
        };
        put_uleb(&mut recorder.row, FORMAT);
        recorder.row.extend_from_slice(&execution.to_le_bytes());
        recorder.push(RowKind::Header);
        recorder
    }

    /// The journal so far, or `None` once recording stopped at the cap.
    #[must_use]
    pub fn journal(&self) -> Option<&[u8]> {
        (self.state == State::On).then_some(&self.bytes)
    }

    /// The most bytes the journal may hold.
    #[must_use]
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Changes the cap; a journal already past it stops at its next row.
    pub fn set_cap(&mut self, cap: usize) {
        self.cap = cap;
    }

    /// True once, the first time it is asked after recording stopped.
    pub fn take_notice(&mut self) -> bool {
        if self.state == (State::Off { told: false }) {
            self.state = State::Off { told: true };
            true
        } else {
            false
        }
    }

    /// A host call with the arguments `request` (see [`put_int_arg`]) that
    /// returned `value` and, when given, `result` bytes in the exchange
    /// buffer, through the engine's call site `site` for the import
    /// `module` `name`.
    pub fn call(
        &mut self,
        site: usize,
        module: &str,
        name: &str,
        request: &[u8],
        value: Value,
        result: Option<&[u8]>,
    ) {
        if self.state != State::On {
            return;
        }
        if self.sites.len() <= site {
            self.sites.resize(site + 1, 0);
        }
        let row = &mut self.row;
        match self.sites[site] {
            0 => {
                put_uleb(row, u64::from(self.methods));
                for text in [module, name] {
                    put_uleb(row, text.len() as u64);
                    row.extend_from_slice(text.as_bytes());
                }
                self.methods += 1;
                self.sites[site] = self.methods;
            }
            n => put_uleb(row, u64::from(n - 1)),
        }
        put_uleb(row, request.len() as u64);
        if request.len() <= 8 {
            row.extend_from_slice(request);
        } else {
            row.extend_from_slice(&request_digest(request).to_le_bytes());
        }
        put_reply(row, value, result);
        self.push(RowKind::Call);
    }

    /// A `.finish(handle)` call that returned `value` and `result`.
    pub fn finish(&mut self, handle: u32, value: Value, result: Option<&[u8]>) {
        if self.state == State::On {
            put_uleb(&mut self.row, u64::from(handle));
            put_reply(&mut self.row, value, result);
            self.push(RowKind::Finish);
        }
    }

    /// An `hd:rt` `block` call that returned `handles`.
    pub fn block(&mut self, handles: &[u32]) {
        if self.state == State::On {
            put_handles(&mut self.row, handles);
            self.push(RowKind::Block);
        }
    }

    /// An `hd:rt` `abort(handle)` call.
    pub fn abort(&mut self, handle: u32) {
        if self.state == State::On {
            put_uleb(&mut self.row, u64::from(handle));
            self.push(RowKind::Abort);
        }
    }

    /// One poll of the input that runs.
    pub fn poll(&mut self) {
        self.polls += 1;
    }

    /// A wake of the input with `handles`, after its polls so far.
    pub fn wake(&mut self, handles: &[u32]) {
        if self.state == State::On {
            put_uleb(&mut self.row, self.polls);
            put_handles(&mut self.row, handles);
            self.push(RowKind::Wake);
        }
    }

    /// The host cancels the input's root, after its polls so far.
    pub fn cancel(&mut self) {
        if self.state == State::On {
            put_uleb(&mut self.row, self.polls);
            self.push(RowKind::Cancel);
        }
    }

    /// A REPL input numbered `number` starts, with its source text and
    /// module bytes.
    pub fn input(&mut self, number: u32, source: &[u8], module: &[u8]) {
        self.polls = 0;
        if self.state == State::On {
            put_uleb(&mut self.row, u64::from(number));
            for bytes in [source, module] {
                self.row
                    .extend_from_slice(&hd_base::hash128(bytes).0.to_le_bytes());
            }
            self.push(RowKind::Input);
        }
    }

    /// The input that ran ends with `end`.
    pub fn end(&mut self, end: End) {
        if self.state == State::On {
            let (tag, status) = match end {
                End::Exit(status) => (0, Some(status)),
                End::Trapped => (1, None),
                End::Stopped => (2, None),
                End::Cancelled => (3, None),
                End::Failed => (4, None),
            };
            self.row.push(tag);
            self.row.extend(status);
            self.push(RowKind::End);
        }
    }

    /// Frames the row in `self.row` as `kind`, or stops recording when it
    /// would pass the cap.
    fn push(&mut self, kind: RowKind) {
        let len = self.row.len();
        let size = 1 + uleb_len(self.next) + uleb_len(len as u64) + len + 4;
        if self.bytes.len() + size > self.cap {
            self.state = State::Off { told: false };
            self.bytes = Vec::new();
            self.sites = Vec::new();
            self.row = Vec::new();
            return;
        }
        append_parts(&mut self.bytes, kind, self.next, &self.row);
        self.next += 1;
        self.row.clear();
    }
}

#[cfg(test)]
#[path = "journal_tests.rs"]
mod tests;
