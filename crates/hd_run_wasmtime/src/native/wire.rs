//! The value codecs a generated companion calls
//! (`cli.host.package.native-generated`, `module.host.package.native-values`):
//! one [`Decoder`] over a call's argument bytes and one [`Encoder`] for its
//! result, in the exchange-buffer encoding the JS host's codecs write
//! (`host/package-providers.ts`). The companion generates one codec per
//! contract type from the checked contract; these are its scalar steps.
//!
//! A value that fails its check is a `host-contract` failure of the call
//! (`cli.host.package.native-call-failure`), with the JS host's message for
//! the same failure (`module.host.package.native-parity`); the engine names
//! the trait and method.
//!
//! A private-field value crosses as the package host's typed token stream
//! (`std.host.package_encode`): JSON text of `["n"]`, `["b", bool]`,
//! `["s", text]`, `["i", decimal]`, `["u", decimal]`, `["f", 16 hex digits
//! of f64 bits]`, and the containers `["a", count]` and `["o", count]`, each
//! followed by its items in prefix order. It is read straight into a
//! [`Serde`] tree and written straight from one: one pass over the tokens,
//! with an explicit stack, so nesting never recurses natively.
//!
//! Cost: every step is linear in the bytes it reads or writes. A count read
//! from the guest reserves no more items than the bytes left could hold.

use std::any::Any;
use std::fmt::Write as _;

use hd_base::HashSet;

use super::{
    Answer, Dispatch, Dispatched, Encode, Failure, Handles, HostError, HostResult, NativeCall,
    Serde, Suspension,
};
use crate::codec::{Dec, Enc};

/// The JS host's failure text for `message`.
fn fail(message: &str) -> HostError {
    HostError(format!("package host: {message}"))
}

/// A truncated or malformed argument, as the JS host's `Dec` reports it.
fn undecodable() -> HostError {
    HostError("an argument does not decode".into())
}

/// The registered provider `T` behind a registration's `provider`.
pub fn provider<T: 'static>(provider: &mut dyn Any) -> HostResult<&mut T> {
    provider
        .downcast_mut::<T>()
        .ok_or_else(|| fail("the registered provider is not this dispatch's trait object"))
}

/// A method index the companion's interface does not have.
#[must_use]
pub fn unknown_method(method: usize) -> HostError {
    fail(&format!("no provider method at index {method}"))
}

/// A suspending method's answer: a ready value encoded at once, or the
/// pending call with the codec its `.finish` encodes the value with
/// (`cli.host.package.native-completion-forms`).
pub fn answer<T: 'static>(
    call: NativeCall<T>,
    encode: Encode<T>,
    handles: &mut Handles,
) -> Dispatched {
    match call {
        NativeCall::Ready(value) => Ok(Answer::Ready(encode(
            value.map_err(Failure::Threw)?,
            handles,
        )?)),
        NativeCall::Pending(call) => Ok(Answer::Pending(Suspension::new(call, encode))),
    }
}

/// The argument of a method whose one argument is a `string`: its UTF-8
/// bytes, with no length, as the JS host passes it.
#[must_use]
pub fn text(args: &[u8]) -> String {
    String::from_utf8_lossy(args).into_owned()
}

/// One step of a recursive group's `Debug`, which prints a value one
/// position `P` at a time, as a derived `Debug` prints it (`{:#?}`
/// included), so a value of any depth prints in constant native stack.
pub enum Show<P> {
    /// A position, at its indent depth.
    At(P, usize),
    /// What precedes a composite's child: its separator and label.
    Before {
        first: bool,
        label: &'static str,
        depth: usize,
    },
    /// A composite's closing text, pretty and compact, at its own depth.
    Close {
        pretty: &'static str,
        compact: &'static str,
        depth: usize,
    },
}

impl<P> Show<P> {
    /// Prints `root`; `at` prints one position, pushing its children.
    pub fn run(
        f: &mut std::fmt::Formatter<'_>,
        root: P,
        mut at: impl FnMut(P, usize, &mut Vec<Self>, &mut std::fmt::Formatter<'_>) -> std::fmt::Result,
    ) -> std::fmt::Result {
        let mut stack = vec![Self::At(root, 0)];
        while let Some(item) = stack.pop() {
            match item {
                Self::At(p, depth) => at(p, depth, &mut stack, f)?,
                Self::Before {
                    first,
                    label,
                    depth,
                } => {
                    if f.alternate() {
                        f.write_str(if first { "\n" } else { ",\n" })?;
                        indent(f, depth)?;
                    } else if !first {
                        f.write_str(", ")?;
                    }
                    f.write_str(label)?;
                }
                Self::Close {
                    pretty,
                    compact,
                    depth,
                } => {
                    if f.alternate() {
                        f.write_str(",\n")?;
                        indent(f, depth)?;
                        f.write_str(pretty)?;
                    } else {
                        f.write_str(compact)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Opens a composite at `depth` (`Name {`, `(`, `[`, `Some(`) and
    /// pushes its labelled children and its `close`; one without children
    /// is `open` and `close` alone, as `[]` is.
    pub fn open<I>(
        stack: &mut Vec<Self>,
        f: &mut std::fmt::Formatter<'_>,
        depth: usize,
        open: &'static str,
        close: &'static str,
        children: I,
    ) -> std::fmt::Result
    where
        I: IntoIterator<Item = (&'static str, P)>,
        I::IntoIter: DoubleEndedIterator + ExactSizeIterator,
    {
        let children = children.into_iter();
        f.write_str(open)?;
        if children.len() == 0 {
            return f.write_str(close);
        }
        let braced = open.ends_with('{');
        if braced && !f.alternate() {
            f.write_str(" ")?;
        }
        // A one-element tuple keeps its comma: `(x,)`.
        let compact = match (braced, open == "(" && children.len() == 1) {
            (true, _) => " }",
            (false, true) => ",)",
            (false, false) => close,
        };
        stack.push(Self::Close {
            pretty: close,
            compact,
            depth,
        });
        for (n, (label, p)) in children.enumerate().rev() {
            stack.push(Self::At(p, depth + 1));
            stack.push(Self::Before {
                first: n == 0,
                label,
                depth: depth + 1,
            });
        }
        Ok(())
    }
}

/// A leaf of a recursive group's `Debug` at `depth`: its own `Debug`, each
/// line of a pretty one indented to `depth`.
pub fn show_leaf(
    f: &mut std::fmt::Formatter<'_>,
    x: &dyn std::fmt::Debug,
    depth: usize,
) -> std::fmt::Result {
    if !f.alternate() {
        return write!(f, "{x:?}");
    }
    let text = format!("{x:#?}");
    for (n, line) in text.split('\n').enumerate() {
        if n != 0 {
            f.write_str("\n")?;
            indent(f, depth)?;
        }
        f.write_str(line)?;
    }
    Ok(())
}

fn indent(f: &mut std::fmt::Formatter<'_>, depth: usize) -> std::fmt::Result {
    for _ in 0..depth {
        f.write_str("    ")?;
    }
    Ok(())
}

/// A decoded argument holding a recursive value, which the provider
/// borrows, or the finished parts of one still being decoded: when the
/// dispatch or the decode ends, on every path (a later part or argument
/// failing, the provider's error or panic, success), its `dismantle`
/// takes it apart one level at a time through its group's codec, so a
/// value as deep as the guest makes it never drops recursively
/// (AGENTS.md "Performant"). A finished decode takes its value back with
/// [`Lent::into_inner`].
pub struct Lent<T> {
    /// `None` only inside `drop`.
    value: Option<T>,
    dismantle: fn(T),
}

impl<T> Lent<T> {
    pub fn new(value: T, dismantle: fn(T)) -> Self {
        Self {
            value: Some(value),
            dismantle,
        }
    }
}

impl<T> Lent<T> {
    /// The value, which its decode finished: nothing is taken apart.
    pub fn into_inner(mut self) -> T {
        match self.value.take() {
            Some(value) => value,
            None => unreachable!("ruled out: only `drop` takes the value"),
        }
    }
}

impl<T> std::ops::DerefMut for Lent<T> {
    fn deref_mut(&mut self) -> &mut T {
        match &mut self.value {
            Some(value) => value,
            None => unreachable!("ruled out: only `drop` takes the value"),
        }
    }
}

impl<T> std::ops::Deref for Lent<T> {
    type Target = T;

    fn deref(&self) -> &T {
        match &self.value {
            Some(value) => value,
            None => unreachable!("ruled out: only `drop` takes the value"),
        }
    }
}

impl<T> Drop for Lent<T> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            (self.dismantle)(value);
        }
    }
}

/// A call's argument bytes, decoded in contract order.
pub struct Decoder<'a> {
    d: Dec<'a>,
}

impl<'a> Decoder<'a> {
    #[must_use]
    pub fn new(args: &'a [u8]) -> Self {
        Self { d: Dec::new(args) }
    }

    /// Every argument byte was read.
    pub fn finish(&self) -> HostResult<()> {
        if self.d.rest() == 0 {
            Ok(())
        } else {
            Err(fail("trailing argument bytes"))
        }
    }

    /// The items to reserve for `count` read from the guest: no more than
    /// the bytes left could hold, so a forged count allocates nothing.
    #[must_use]
    pub fn capacity(&self, count: usize) -> usize {
        count.min(self.d.rest())
    }

    pub fn bool(&mut self) -> HostResult<bool> {
        Ok(self.tag(2)? == 1)
    }

    pub fn char(&mut self) -> HostResult<char> {
        let code = self.d.leb().map_err(|_| undecodable())?;
        u32::try_from(code)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| fail("invalid char"))
    }

    /// A signed integer of `T`'s width: zigzag LEB128.
    pub fn signed<T: TryFrom<i64>>(&mut self) -> HostResult<T> {
        let value = self.d.zz().map_err(|_| undecodable())?;
        T::try_from(value).map_err(|_| fail("integer is out of range"))
    }

    /// An unsigned integer of `T`'s width: LEB128.
    pub fn unsigned<T: TryFrom<u64>>(&mut self) -> HostResult<T> {
        let value = self.d.leb().map_err(|_| undecodable())?;
        T::try_from(value).map_err(|_| fail("integer is out of range"))
    }

    pub fn f32(&mut self) -> HostResult<f32> {
        Ok(f32::from_le_bytes(
            self.d.raw().map_err(|_| fail("truncated float"))?,
        ))
    }

    pub fn f64(&mut self) -> HostResult<f64> {
        Ok(f64::from_le_bytes(
            self.d.raw().map_err(|_| fail("truncated float"))?,
        ))
    }

    pub fn string(&mut self) -> HostResult<String> {
        self.d.str().map_err(|_| undecodable())
    }

    pub fn bytes(&mut self) -> HostResult<Vec<u8>> {
        Ok(self.d.bytes().map_err(|_| undecodable())?.to_vec())
    }

    /// A list's item count.
    pub fn list_len(&mut self) -> HostResult<usize> {
        self.count("invalid list length")
    }

    /// A map's entry count.
    pub fn map_len(&mut self) -> HostResult<usize> {
        self.count("invalid map size")
    }

    fn count(&mut self, invalid: &str) -> HostResult<usize> {
        let count = self.d.leb().map_err(|_| undecodable())?;
        if count > u64::from(u32::MAX) {
            return Err(fail(invalid));
        }
        usize::try_from(count).map_err(|_| fail(invalid))
    }

    /// An option's, a result's or an enum's tag, below `count`.
    pub fn tag(&mut self, count: usize) -> HostResult<usize> {
        let tag = self.d.leb().map_err(|_| undecodable())?;
        usize::try_from(tag)
            .ok()
            .filter(|tag| *tag < count)
            .ok_or_else(undecodable)
    }

    /// A value of an enum without variants: no tag decodes.
    pub fn absent<T>(&mut self) -> HostResult<T> {
        self.tag(0)?;
        Err(undecodable())
    }

    /// A host handle as a data argument, which the JS host refuses too.
    pub fn handle<T>(&mut self) -> HostResult<T> {
        Err(fail("a handle cannot be passed as a data argument"))
    }

    /// A private-field value: its token stream, read into a typed tree.
    pub fn serde(&mut self) -> HostResult<Serde> {
        let text = self.d.bytes().map_err(|_| undecodable())?;
        Tokens::new(&String::from_utf8_lossy(text)).read()
    }
}

/// A call's result bytes, and the handles they issued. An encoder dropped
/// before [`Encoder::into_bytes`], by a failure part way through a value,
/// releases the handles it issued, as the JS host's codec does: a guest
/// never sees their numbers.
pub struct Encoder<'a> {
    e: Enc,
    handles: &'a mut Handles,
    /// (number, interface) of each handle issued so far.
    issued: Vec<(u32, u32)>,
}

impl Drop for Encoder<'_> {
    fn drop(&mut self) {
        for (number, interface) in self.issued.drain(..) {
            // ruled out: each was issued by this encoder and no guest code
            // ran since, so each is live.
            let _ = self.handles.release(number, interface);
        }
    }
}

impl<'a> Encoder<'a> {
    /// An encoder issuing returned handles in `handles`, its instance's.
    pub fn new(handles: &'a mut Handles) -> Self {
        Self {
            e: Enc::default(),
            handles,
            issued: Vec::new(),
        }
    }

    pub fn bool(&mut self, value: bool) {
        self.e.leb(u64::from(value));
    }

    pub fn char(&mut self, value: char) {
        self.e.leb(u64::from(value));
    }

    pub fn signed(&mut self, value: i64) {
        self.e.zz(value);
    }

    pub fn unsigned(&mut self, value: u64) {
        self.e.leb(value);
    }

    pub fn f32(&mut self, value: f32) {
        self.e.f32(value);
    }

    pub fn f64(&mut self, value: f64) {
        self.e.f64(value);
    }

    pub fn string(&mut self, value: &str) {
        self.e.str(value);
    }

    pub fn bytes(&mut self, value: &[u8]) {
        self.e.bytes(value);
    }

    /// A list's item count or a map's entry count.
    pub fn count(&mut self, count: usize) {
        self.e.leb(count as u64);
    }

    pub fn tag(&mut self, tag: usize) {
        self.e.leb(tag as u64);
    }

    /// A returned host handle: `value`, the trait object of the handle
    /// trait at contract interface `interface` whose methods `dispatch`
    /// calls, moves into the instance's registry and its number is
    /// written (`module.host.package.native-handles`).
    pub fn handle(
        &mut self,
        value: Box<dyn Any>,
        interface: u32,
        dispatch: Dispatch,
    ) -> HostResult<()> {
        let number = self.handles.insert(interface, dispatch, value)?;
        self.issued.push((number, interface));
        self.e.leb(u64::from(number));
        Ok(())
    }

    /// A private-field value, written as its token stream.
    pub fn serde(&mut self, value: &Serde) -> HostResult<()> {
        let text = write_tokens(value)?;
        self.e.str(&text);
        Ok(())
    }

    /// The encoded value; the handles it issued stay live.
    #[must_use]
    pub fn into_bytes(mut self) -> Vec<u8> {
        self.issued.clear();
        std::mem::take(&mut self.e).into_bytes()
    }
}

/// The token stream of `root`, in prefix order. Each map's keys must be
/// distinct text, as a JSON object's are.
fn write_tokens(root: &Serde) -> HostResult<String> {
    let mut out = String::from("[");
    let mut first = true;
    let mut token = |out: &mut String, tag: &str| {
        if !first {
            out.push(',');
        }
        first = false;
        let _ = write!(out, "[\"{tag}\"");
    };
    let mut pending = vec![root];
    while let Some(value) = pending.pop() {
        match value {
            Serde::Unit | Serde::None => token(&mut out, "n"),
            Serde::Some(inner) => {
                pending.push(inner);
                continue;
            }
            Serde::Bool(b) => {
                token(&mut out, "b");
                let _ = write!(out, ",{b}");
            }
            Serde::I64(n) => {
                token(&mut out, "i");
                let _ = write!(out, ",\"{n}\"");
            }
            Serde::U64(n) => {
                token(&mut out, "u");
                let _ = write!(out, ",\"{n}\"");
            }
            Serde::F32(x) => float(&mut out, &mut token, f64::from(*x)),
            Serde::F64(x) => float(&mut out, &mut token, *x),
            Serde::Char(c) => {
                token(&mut out, "s");
                out.push(',');
                json_string(&mut out, c.encode_utf8(&mut [0; 4]));
            }
            Serde::String(s) => {
                token(&mut out, "s");
                out.push(',');
                json_string(&mut out, s);
            }
            Serde::Bytes(bytes) => {
                token(&mut out, "a");
                let _ = write!(out, ",{}]", bytes.len());
                for b in bytes {
                    token(&mut out, "u");
                    let _ = write!(out, ",\"{b}\"]");
                }
                continue;
            }
            Serde::Seq(items) => {
                token(&mut out, "a");
                let _ = write!(out, ",{}", items.len());
                pending.extend(items.iter().rev());
            }
            Serde::Map(entries) => {
                token(&mut out, "o");
                let _ = write!(out, ",{}", entries.len());
                let mut keys = HashSet::default();
                for (key, _) in entries {
                    let fresh = match key {
                        Serde::String(s) => keys.insert(s.clone()),
                        Serde::Char(c) => keys.insert(c.to_string()),
                        _ => false,
                    };
                    if !fresh {
                        return Err(fail("invalid private object key"));
                    }
                }
                for (key, value) in entries.iter().rev() {
                    pending.push(value);
                    pending.push(key);
                }
            }
        }
        out.push(']');
    }
    out.push(']');
    Ok(out)
}

/// A finite float as its f64 bits; a non-finite one as null, as the JS
/// host writes it.
fn float(out: &mut String, token: &mut impl FnMut(&mut String, &str), x: f64) {
    if x.is_finite() {
        token(out, "f");
        let _ = write!(out, ",\"{:016x}\"", x.to_bits());
    } else {
        token(out, "n");
    }
}

/// `s` as a JSON string literal.
fn json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0}'..='\u{1f}' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            _ => out.push(c),
        }
    }
    out.push('"');
}

/// A container being read: its items so far and how many are left.
enum Open {
    Seq(Vec<Serde>, usize),
    Map(Vec<(Serde, Serde)>, usize, Option<String>, HashSet<String>),
}

/// The reader of a token stream. It accepts the token grammar the guest
/// writes; other JSON is refused as a malformed transport or token.
struct Tokens<'a> {
    s: &'a [u8],
    at: usize,
    text: &'a str,
}

impl<'a> Tokens<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            s: text.as_bytes(),
            at: 0,
            text,
        }
    }

    fn read(mut self) -> HostResult<Serde> {
        let transport = || fail("invalid private value transport");
        self.space();
        self.expect(b'[').ok_or_else(transport)?;
        let mut open: Vec<Open> = Vec::new();
        let mut root = None;
        let mut first = true;
        loop {
            self.space();
            if self.eat(b']') {
                break;
            }
            if !first {
                self.expect(b',').ok_or_else(transport)?;
                self.space();
            }
            first = false;
            let value = self.token()?;
            if root.is_some() {
                return Err(fail("invalid private value framing"));
            }
            let mut done = match value {
                Item::Value(value) => value,
                Item::Seq(0) => Serde::Seq(Vec::new()),
                Item::Map(0) => Serde::Map(Vec::new()),
                Item::Seq(count) => {
                    open.push(Open::Seq(Vec::with_capacity(self.room(count)), count));
                    continue;
                }
                Item::Map(count) => {
                    let reserve = self.room(count);
                    open.push(Open::Map(
                        Vec::with_capacity(reserve),
                        count,
                        None,
                        HashSet::default(),
                    ));
                    continue;
                }
            };
            // Attach the finished value, closing each container it fills.
            loop {
                match open.last_mut() {
                    None => {
                        root = Some(done);
                        break;
                    }
                    Some(Open::Seq(items, left)) => {
                        items.push(done);
                        *left -= 1;
                        if *left != 0 {
                            break;
                        }
                    }
                    Some(Open::Map(entries, left, key, keys)) => {
                        if let Some(key) = key.take() {
                            entries.push((Serde::String(key), done));
                            *left -= 1;
                            if *left != 0 {
                                break;
                            }
                        } else {
                            let Serde::String(text) = done else {
                                return Err(fail("invalid private object key"));
                            };
                            if !keys.insert(text.clone()) {
                                return Err(fail("invalid private object key"));
                            }
                            *key = Some(text);
                            break;
                        }
                    }
                }
                done = match open.pop() {
                    Some(Open::Seq(items, _)) => Serde::Seq(items),
                    Some(Open::Map(entries, ..)) => Serde::Map(entries),
                    None => unreachable!("ruled out: the match above saw a container"),
                };
            }
        }
        self.space();
        if self.at != self.s.len() {
            return Err(transport());
        }
        if !open.is_empty() {
            return Err(fail("invalid private container length"));
        }
        root.ok_or_else(|| fail("invalid private value framing"))
    }

    /// The items a container of `count` may reserve: each takes at least
    /// one five-byte token (`["n"]`) of the text left.
    fn room(&self, count: usize) -> usize {
        count.min((self.s.len() - self.at) / 5)
    }

    /// One `[tag]` or `[tag, value]` token.
    fn token(&mut self) -> HostResult<Item> {
        let bad = || fail("invalid private value token");
        self.expect(b'[').ok_or_else(bad)?;
        self.space();
        let tag = self.string().ok_or_else(bad)?;
        self.space();
        let item = if tag == "n" {
            Item::Value(Serde::None)
        } else {
            self.expect(b',').ok_or_else(bad)?;
            self.space();
            match tag.as_str() {
                "b" => Item::Value(Serde::Bool(self.boolean().ok_or_else(bad)?)),
                "s" => Item::Value(Serde::String(self.string().ok_or_else(bad)?)),
                "i" | "u" | "f" => {
                    let digits = self.string().ok_or_else(bad)?;
                    Item::Value(number(&tag, &digits)?)
                }
                "a" | "o" => {
                    let count = self.count().ok_or_else(bad)?;
                    if tag == "a" {
                        Item::Seq(count)
                    } else {
                        Item::Map(count)
                    }
                }
                _ => return Err(bad()),
            }
        };
        self.space();
        self.expect(b']').ok_or_else(bad)?;
        Ok(item)
    }

    fn space(&mut self) {
        while matches!(self.s.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        let hit = self.s.get(self.at) == Some(&byte);
        if hit {
            self.at += 1;
        }
        hit
    }

    fn expect(&mut self, byte: u8) -> Option<()> {
        self.eat(byte).then_some(())
    }

    fn boolean(&mut self) -> Option<bool> {
        for (word, value) in [("true", true), ("false", false)] {
            if self.text[self.at..].starts_with(word) {
                self.at += word.len();
                return Some(value);
            }
        }
        None
    }

    /// A container count: a JSON integer no larger than JS's safe integers.
    fn count(&mut self) -> Option<usize> {
        let start = self.at;
        while self.s.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
        }
        let digits = &self.text[start..self.at];
        if digits.is_empty() || (digits.len() > 1 && digits.starts_with('0')) {
            return None;
        }
        let count: u64 = digits.parse().ok()?;
        if count > (1 << 53) - 1 {
            return None;
        }
        usize::try_from(count).ok()
    }

    /// A JSON string literal. A lone surrogate escape has no Rust text and
    /// fails.
    fn string(&mut self) -> Option<String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let start = self.at;
            while !matches!(self.s.get(self.at), None | Some(b'"' | b'\\' | 0..=0x1f)) {
                self.at += 1;
            }
            out.push_str(&self.text[start..self.at]);
            match self.s.get(self.at)? {
                b'"' => {
                    self.at += 1;
                    return Some(out);
                }
                b'\\' => {
                    self.at += 1;
                    let escape = *self.s.get(self.at)?;
                    self.at += 1;
                    out.push(match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.escape()?,
                        _ => return None,
                    });
                }
                _ => return None,
            }
        }
    }

    /// The character of a `\u` escape, joining a surrogate pair.
    fn escape(&mut self) -> Option<char> {
        let high = self.hex4()?;
        if !(0xd800..0xdc00).contains(&high) {
            return char::from_u32(high);
        }
        if !self.text[self.at..].starts_with("\\u") {
            return None;
        }
        self.at += 2;
        let low = self.hex4()?;
        if !(0xdc00..0xe000).contains(&low) {
            return None;
        }
        char::from_u32(0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00))
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.text.get(self.at..self.at + 4)?;
        let value = u32::from_str_radix(digits, 16).ok()?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        self.at += 4;
        Some(value)
    }
}

/// A token as read: a value, or a container with its item count.
enum Item {
    Value(Serde),
    Seq(usize),
    Map(usize),
}

/// The number of an `i`, `u` or `f` token, checked as the JS host does.
fn number(tag: &str, text: &str) -> HostResult<Serde> {
    let invalid = || fail("invalid private numeric token");
    if tag == "f" {
        if text.len() != 16 || !text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(invalid());
        }
        let bits = u64::from_str_radix(text, 16).map_err(|_| invalid())?;
        let value = f64::from_bits(bits);
        return if value.is_finite() {
            Ok(Serde::F64(value))
        } else {
            Err(fail("invalid private float"))
        };
    }
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty()
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
        || text == "-0"
    {
        return Err(invalid());
    }
    if tag == "i" {
        text.parse()
            .map(Serde::I64)
            .map_err(|_| fail("private signed integer out of range"))
    } else {
        text.parse()
            .map(Serde::U64)
            .map_err(|_| fail("private unsigned integer out of range"))
    }
}

#[cfg(test)]
mod tests {
    use super::{Decoder, Encoder, Serde, answer, fail, text};
    use crate::native::{
        Answer, Dispatched, Failure, Handles, HostError, HostResult, NativeCall, pending,
    };

    fn message<T: std::fmt::Debug>(result: Result<T, HostError>) -> String {
        result.unwrap_err().0
    }

    /// `text`'s token stream as the guest would pass it: a length-prefixed
    /// string.
    fn tokens(text: &str) -> Vec<u8> {
        let mut h = Handles::default();
        let mut e = Encoder::new(&mut h);
        e.string(text);
        e.into_bytes()
    }

    fn read(text: &str) -> Result<Serde, HostError> {
        Decoder::new(&tokens(text)).serde()
    }

    fn written(value: &Serde) -> Result<String, HostError> {
        let mut h = Handles::default();
        let mut e = Encoder::new(&mut h);
        e.serde(value)?;
        let bytes = e.into_bytes();
        Ok(Decoder::new(&bytes).string().unwrap())
    }

    #[test]
    fn a_token_stream_reads_into_the_typed_tree_and_back() {
        let stream = r#"[["o",3],["s","a\"\\\/\b\f\n\r\té😀"],["a",2],["i","-9223372036854775808"],["u","18446744073709551615"],["s","k"],["f","8000000000000000"],["s","m"],["o",0]]"#;
        let tree = Serde::Map(vec![
            (
                Serde::String("a\"\\/\u{8}\u{c}\n\r\té😀".into()),
                Serde::Seq(vec![Serde::I64(i64::MIN), Serde::U64(u64::MAX)]),
            ),
            (Serde::String("k".into()), Serde::F64(-0.0)),
            (Serde::String("m".into()), Serde::Map(vec![])),
        ]);
        let value = read(stream).unwrap();
        assert_eq!(value, tree);
        let Serde::Map(entries) = &value else {
            unreachable!()
        };
        assert!(matches!(entries[1].1, Serde::F64(x) if x.to_bits() == (-0.0_f64).to_bits()));
        // Whitespace between tokens is JSON's.
        assert_eq!(read(" [ [ \"n\" ] ]\n").unwrap(), Serde::None);
        assert_eq!(read(r#"[["b",false]]"#).unwrap(), Serde::Bool(false));
        assert_eq!(read(r#"[["a",0]]"#).unwrap(), Serde::Seq(vec![]));
        // What it writes reads back as the same tree.
        let text = written(&tree).unwrap();
        assert_eq!(read(&text).unwrap(), tree);
    }

    #[test]
    fn a_provider_tree_writes_as_the_js_host_writes_it() {
        let value = Serde::Seq(vec![
            Serde::Unit,
            Serde::Some(Box::new(Serde::Char('x'))),
            Serde::F32(1.5),
            Serde::F64(f64::NAN),
            Serde::F32(f32::INFINITY),
            Serde::Bytes(vec![0, 255]),
            Serde::I64(5),
            Serde::String("\u{1}".into()),
            Serde::Map(vec![(Serde::Char('k'), Serde::Bool(true))]),
        ]);
        assert_eq!(
            written(&value).unwrap(),
            r#"[["a",9],["n"],["s","x"],["f","3ff8000000000000"],["n"],["n"],["a",2],["u","0"],["u","255"],["i","5"],["s","\u0001"],["o",1],["s","k"],["b",true]]"#
        );
        for keys in [
            vec![(Serde::I64(1), Serde::Unit)],
            vec![
                (Serde::String("k".into()), Serde::Unit),
                (Serde::Char('k'), Serde::Unit),
            ],
        ] {
            assert_eq!(
                message(written(&Serde::Map(keys))),
                "package host: invalid private object key"
            );
        }
    }

    #[test]
    fn a_malformed_token_stream_fails_with_the_js_hosts_message() {
        for (stream, expected) in [
            ("", "invalid private value transport"),
            ("{}", "invalid private value transport"),
            (r#"[["n"]"#, "invalid private value transport"),
            (r#"[["n"]]x"#, "invalid private value transport"),
            (r#"[["n"] ["n"]]"#, "invalid private value transport"),
            ("[]", "invalid private value framing"),
            (r#"[["n"],["n"]]"#, "invalid private value framing"),
            (r#"[["a",2],["n"]]"#, "invalid private container length"),
            ("[1]", "invalid private value token"),
            (r#"[["n",1]]"#, "invalid private value token"),
            (r#"[["x",1]]"#, "invalid private value token"),
            (r#"[["b","true"]]"#, "invalid private value token"),
            (r#"[["s",1]]"#, "invalid private value token"),
            (r#"[["a",-1]]"#, "invalid private value token"),
            (r#"[["a",01]]"#, "invalid private value token"),
            (r#"[["a",9007199254740992]]"#, "invalid private value token"),
            (r#"[["s","\x"]]"#, "invalid private value token"),
            (r#"[["s","\ud800"]]"#, "invalid private value token"),
            (r#"[["s","\ud800A"]]"#, "invalid private value token"),
            (r#"[["s","\u00g1"]]"#, "invalid private value token"),
            ("[[\"s\",\"\u{1}\"]]", "invalid private value token"),
            (r#"[["s","open"#, "invalid private value token"),
            (r#"[["i","-0"]]"#, "invalid private numeric token"),
            (r#"[["i","01"]]"#, "invalid private numeric token"),
            (r#"[["i",""]]"#, "invalid private numeric token"),
            (r#"[["u","1x"]]"#, "invalid private numeric token"),
            (
                r#"[["f","3FF8000000000000"]]"#,
                "invalid private numeric token",
            ),
            (r#"[["f","00"]]"#, "invalid private numeric token"),
            (
                r#"[["i","9223372036854775808"]]"#,
                "private signed integer out of range",
            ),
            (r#"[["u","-1"]]"#, "private unsigned integer out of range"),
            (r#"[["f","7ff0000000000000"]]"#, "invalid private float"),
            (r#"[["o",1],["n"],["n"]]"#, "invalid private object key"),
            (
                r#"[["o",2],["s","k"],["n"],["s","k"],["n"]]"#,
                "invalid private object key",
            ),
        ] {
            assert_eq!(
                message(read(stream)),
                format!("package host: {expected}"),
                "{stream}"
            );
        }
        // A truncated string is the wire's own failure.
        assert_eq!(
            message(Decoder::new(&[5, b'[']).serde()),
            "an argument does not decode"
        );
    }

    #[test]
    fn each_scalar_is_checked_as_the_js_host_checks_it() {
        let mut h = Handles::default();
        let mut e = Encoder::new(&mut h);
        e.signed(300);
        e.unsigned(300);
        e.unsigned(0xd800);
        e.unsigned(u64::from(u32::MAX) + 1);
        e.unsigned(u64::from(u32::MAX) + 1);
        e.tag(1);
        e.unsigned(u64::from(u32::MAX));
        let bytes = e.into_bytes();
        let mut d = Decoder::new(&bytes);
        assert_eq!(
            message(d.signed::<i8>()),
            "package host: integer is out of range"
        );
        assert_eq!(
            message(d.unsigned::<u8>()),
            "package host: integer is out of range"
        );
        assert_eq!(message(d.char()), "package host: invalid char");
        assert_eq!(message(d.list_len()), "package host: invalid list length");
        assert_eq!(message(d.map_len()), "package host: invalid map size");
        assert_eq!(message(d.absent::<()>()), "an argument does not decode");
        assert_eq!(d.list_len().unwrap(), u32::MAX as usize);
        assert_eq!(
            d.capacity(u32::MAX as usize),
            0,
            "a forged count reserves nothing"
        );
        assert!(d.finish().is_ok());
        for fail in [message(d.f32()), message(d.f64())] {
            assert_eq!(fail, "package host: truncated float");
        }
        for fail in [
            message(d.bool()),
            message(d.signed::<i64>()),
            message(d.unsigned::<u64>()),
            message(d.char()),
            message(d.string()),
            message(d.bytes()),
            message(d.map_len()),
        ] {
            assert_eq!(fail, "an argument does not decode");
        }
        assert_eq!(
            message(d.handle::<u8>()),
            "package host: a handle cannot be passed as a data argument"
        );
        let mut h = Handles::default();
        let mut e = Encoder::new(&mut h);
        e.bool(true);
        e.char('é');
        e.f32(-0.0);
        e.f64(1.0);
        e.bytes(&[9]);
        let bytes = e.into_bytes();
        let mut d = Decoder::new(&bytes);
        assert!(d.bool().unwrap());
        assert_eq!(d.char().unwrap(), 'é');
        assert_eq!(d.f32().unwrap().to_bits(), (-0.0_f32).to_bits());
        assert_eq!(d.f64().unwrap().to_bits(), 1.0_f64.to_bits());
        assert_eq!(d.bytes().unwrap(), [9]);
        assert_eq!(text(&[b'h', 0xff]), "h\u{fffd}");
    }

    #[test]
    fn a_suspending_method_answers_ready_or_pending() {
        let encode: fn(u8, &mut Handles) -> HostResult<Vec<u8>> = |value, _| Ok(vec![value]);
        let h = &mut Handles::default();
        let Ok(Answer::Ready(bytes)) = answer(NativeCall::Ready(Ok(4)), encode, h) else {
            panic!("a ready value is encoded at once")
        };
        assert_eq!(bytes, [4]);
        // The provider's own error is its throw; the codec's is the host's.
        let failed = answer(NativeCall::Ready(Err(HostError("no".into()))), encode, h);
        assert_eq!(failed.err(), Some(Failure::Threw(HostError("no".into()))));
        let refuse: fn(u8, &mut Handles) -> HostResult<Vec<u8>> = |_, _| Err(fail("codec"));
        let failed = answer(NativeCall::Ready(Ok(4)), refuse, h);
        assert_eq!(failed.err(), Some(Failure::Host(fail("codec"))));
        let aborted = std::rc::Rc::new(std::cell::Cell::new(0));
        let hook = std::rc::Rc::clone(&aborted);
        let (call, _completion) = pending::<u8>(move || hook.set(hook.get() + 1));
        let Ok(Answer::Pending(suspension)) = answer(NativeCall::Pending(call), encode, h) else {
            panic!("a pending call stays pending")
        };
        assert_eq!(aborted.get(), 0);
        drop(suspension);
        assert_eq!(aborted.get(), 1, "dropped undelivered, it is aborted");
    }

    #[test]
    fn a_pending_calls_own_error_is_its_rejection_and_its_codecs_the_hosts() {
        let encode: fn(u8, &mut Handles) -> HostResult<Vec<u8>> = |value, _| {
            if value == 0 {
                Err(fail("codec"))
            } else {
                Ok(vec![value])
            }
        };
        let finished = |value: HostResult<u8>| {
            let (call, completion) = pending::<u8>(|| {});
            completion.complete(value);
            crate::native::Suspension::new(call, encode).finish(&mut Handles::default())
        };
        assert_eq!(finished(Ok(4)), Some(Ok(vec![4])));
        assert_eq!(
            finished(Err(HostError("no".into()))),
            Some(Err(Failure::Rejected(HostError("no".into()))))
        );
        assert_eq!(finished(Ok(0)), Some(Err(Failure::Host(fail("codec")))));
        // Each kind prints the JS host's text for it.
        let label = "p/m.T.wait";
        assert_eq!(
            Failure::Threw(HostError("no".into())).message(label),
            "p/m.T.wait: provider threw no"
        );
        assert_eq!(
            Failure::Rejected(HostError("no".into())).message(label),
            "package host: provider p/m.T.wait rejected: no"
        );
        assert_eq!(
            Failure::Host(fail("codec")).message(label),
            "p/m.T.wait: package host: codec"
        );
    }

    #[allow(clippy::unnecessary_wraps)]
    fn none(_: &mut dyn std::any::Any, _: usize, _: &[u8], _: &mut Handles) -> Dispatched {
        Ok(Answer::Ready(Vec::new()))
    }

    /// Drops count their value's drops.
    struct Counted(std::rc::Rc<std::cell::Cell<u32>>);

    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn a_handle_moves_into_the_registry_and_a_failed_encode_releases_it() {
        let drops = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut h = Handles::default();
        let mut e = Encoder::new(&mut h);
        e.handle(Box::new(Counted(drops.clone())), 2, none).unwrap();
        let bytes = e.into_bytes();
        let number = Decoder::new(&bytes).unsigned::<u32>().unwrap();
        assert_eq!(
            (h.len(), drops.get()),
            (1, 0),
            "a written handle stays live"
        );
        // A value that fails after a handle: the guest never sees its
        // number, and the trait object drops with no provider call.
        let mut e = Encoder::new(&mut h);
        e.handle(Box::new(Counted(drops.clone())), 2, none).unwrap();
        let failed: HostResult<()> = e.serde(&Serde::Map(vec![(Serde::Bool(true), Serde::Unit)]));
        assert!(failed.is_err());
        drop(e);
        assert_eq!((h.len(), drops.get()), (1, 1));
        h.release(number, 2).unwrap();
        assert_eq!((h.len(), drops.get()), (0, 2));
    }
}
