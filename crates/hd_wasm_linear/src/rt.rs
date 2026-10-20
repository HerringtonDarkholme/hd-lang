//! Runtime pieces the compiler generates (`crates/hd_wasm/DESIGN.md`
//! §23.9): the entry exports, panics, the exchange-buffer copy, the bump
//! allocation that serves until the std allocator (job 5), integer text
//! and the host provider stubs. Each is a [`Helper`], built once on its
//! first request.

use hd_base::{DefId, Hash128, StageResult};
use hd_types::{Ty, TyData, TyList};
use wasm_encoder::{BlockType, Instruction};

use crate::layout::{Leaf, Walk};
use crate::map::{self, KeyOps};
use crate::module::{BUF, EMPTY, Func, Global, HEADER, HEAP, Import, Ins, Module, Sym, Val, mem};
use crate::{Entry, Input, unsupported};

/// A `TypeId` value (`emit/dyn_value.rs`): the key and the printable name
/// it holds, two views of static data that are never counted.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeIdParts {
    pub name: String,
    pub shown: String,
}

/// Where one value of an optional element comes from in
/// [`Helper::ListStep`]: the tag, a value of the element, or a zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OptVal {
    Tag,
    Elem(u32),
    Zero,
}

/// The layout of the value at a reference's address, which a vtable
/// slot reads to pass by value (`SlotRecv::ReadThrough`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RefRead {
    pub leaves: Vec<Leaf>,
}

/// A generated function, by what it does.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Helper {
    /// `hd.init`: the inits in order; a flagged one belongs to a module
    /// other than the entry's and runs as a forbidden context
    /// (`req.drive.block-on.forbidden-contexts`).
    EntryInit { inits: Vec<(Hash128, bool)> },
    /// `hd.poll(ctx)`: runs `main` with one host provider per vtable in
    /// `providers` (data 0), passes its results to `report` when there is
    /// one, then runs `then` as the script's later inits; returns the exit
    /// status (0 without a report).
    EntryPoll {
        main: Hash128,
        providers: Vec<(usize, Option<Hash128>)>,
        results: u32,
        report: Option<Hash128>,
        then: Vec<(Hash128, bool)>,
        /// A suspending root's result values: `hd.poll` builds its frame
        /// once and polls it (§23.21), answering -1 while it is Pending.
        bang: Option<Vec<Val>>,
    },
    /// `(ctx) -> status`: checked mode's entry poll (§23.24), `poll`'s
    /// answer. Once that is not Pending, it frees the wake table, releases
    /// every top-level binding ([`Helper::Teardown`]) and audits that no
    /// block is left. A test case's instance serves the next case, so its
    /// end then runs `reinit`, every module's init again, which gives the
    /// next case the bindings a fresh instance has.
    CheckedPoll {
        poll: Box<Helper>,
        reinit: Vec<Hash128>,
    },
    /// `()`: checked mode releases the counted references that each
    /// top-level binding holds, and zeroes them, before the audit at an
    /// entry's end.
    Teardown,
    /// `()`: the panic report `MESSAGE` (`CATEGORY: TEXT`), then a trap.
    Panic(String),
    /// `(text)`: the `explicit-panic` report of `panic(text)`.
    PanicStr,
    /// `(category, message)`: std's `panic_with`, whose report shows the
    /// message after the category when it is not empty
    /// (`flow.panic.report`).
    PanicWith,
    /// `(text) -> len`: copies a string into the exchange buffer at 0,
    /// growing the buffer to fit, and returns its length.
    ToBuf,
    /// `(size) -> address`: `size` bytes of the heap, 8-aligned, from the
    /// bump pointer; the heap grows to fit, and a refused growth is
    /// `heap-exhausted`.
    Alloc,
    /// `(size, descriptor) -> address`: a new counted object (§23.4) of
    /// `size` payload bytes, from std's allocator: its count 1 and its
    /// descriptor word, the table slot of its [`Helper::DropFields`] plus
    /// one, or 0 when its payload holds no counted reference. The address
    /// is its payload's.
    New,
    /// `(len) -> address`: a new string buffer (§23.7) of `len` bytes, a
    /// counted object with nothing to release in it.
    Str,
    /// `(values)`: one more count on each counted reference in a value
    /// laid out as `vals`, which `walk` finds.
    Retain { vals: Vec<Val>, walk: Walk },
    /// `(values)`: one count less on each, as [`Helper::ReleaseRef`] takes.
    Release { vals: Vec<Val>, walk: Walk },
    /// `(address)`: one count less on the object that holds `address`. At
    /// its last count, the references in its payload are released through
    /// its descriptor and its block is freed.
    ReleaseRef,
    /// `(payload)`: the release glue of an object whose payload is laid
    /// out as `leaves`: one count less on each counted reference in it.
    DropFields { leaves: Vec<Leaf>, walk: Walk },
    /// `(payload)`: one more count on each counted reference in a new
    /// object's payload, which its construction copied in.
    RetainFields { leaves: Vec<Leaf>, walk: Walk },
    /// `(list) -> bool`: whether an element reference pins the list: its
    /// header has an owner word and its buffer a count over 1 (§23.20).
    ListPinned,
    /// `(list) -> list`: a new list over a copy of the list's buffer,
    /// whose elements take their own counts: how [`Helper::ListCopy`]
    /// copies a pinned list, through the owner word's slot.
    ListClone { elem: Elem },
    /// `(buffer) -> list`: a new list header (§23.7) over `buffer`, whose
    /// count it takes over, with no owner word.
    ListHeader,
    /// `(capacity) -> buffer`: a new, empty list buffer with room for
    /// `capacity` elements laid out as `elem`.
    ListBuf { elem: Elem },
    /// `(payload)`: the release glue of a list buffer: one count less on
    /// each counted reference of each element.
    DropElems { elem: Elem },
    /// `(list) -> list`: a copy of a list (`types.coll.value`): a new
    /// header over the same buffer, which counts one more owner.
    ListCopy,
    /// `(at, n)`: the `n` elements at `at`, just copied from a buffer that
    /// keeps them too, take their own counts; each collection among an
    /// element's values becomes its copy ([`Helper::ListCopy`]).
    CopyElems { elem: Elem },
    /// `(list)`: makes the list's buffer its own before a change: a
    /// buffer it shares is copied, and with `push` a full one is moved to
    /// a larger one.
    ListUnique { elem: Elem, push: bool },
    /// `(capacity) -> table`: a new, empty map table (`map.rs`) with room
    /// for `capacity` entries laid out as `entry`, its index cleared.
    MapTable { entry: map::Entry },
    /// `(payload)`: the release glue of a map table: one count less on
    /// each counted reference of each live entry.
    DropEntries { entry: map::Entry },
    /// `(key) -> i32`: a key's bucket hash, made non-negative.
    MapHash { key: Vec<Val>, ops: KeyOps },
    /// `(table, hash, key) -> entry`: the number of the key's entry, or -1.
    MapFind { entry: map::Entry, ops: KeyOps },
    /// `(map, hash, key, value)`: puts the entry, taking over one count of
    /// the key and of the value; the map's table is its own already.
    MapPut { entry: map::Entry, ops: KeyOps },
    /// `(map)`: moves the map's full table to one with room.
    MapGrow { entry: map::Entry },
    /// `(map)`: makes the map's table its own before a change: a table it
    /// shares is copied, its entries taking their own counts.
    MapUnique { entry: map::Entry },
    /// `(map, pos, end) -> slot`: the first live entry in `pos..end`, or
    /// `end`.
    MapSlotNext,
    /// `(data, params..) -> results`: a vtable slot (`emit/dyn_value.rs`):
    /// calls `target` with the payload, loaded from the box at `data` as
    /// `leaves`, or `data` itself without them, then the parameters.
    DynSlot {
        target: Hash128,
        leaves: Option<Vec<Leaf>>,
        params: Vec<Val>,
        results: Vec<Val>,
    },
    /// `(ref, params..) -> results`: a vtable slot whose payload is a
    /// reference and whose target takes the value: reads it through the
    /// reference's `get`, calls `target`, then releases the value read.
    DynRead {
        target: Hash128,
        read: RefRead,
        params: Vec<Val>,
        results: Vec<Val>,
    },
    /// `(env) -> T?`: the step closure code of `List.iter()`
    /// (`emit/iter.rs`): `.Some` of the next element, with its own count,
    /// laid out as `opt` says, or `.None` at the end.
    ListStep {
        elem: Elem,
        opt: Vec<OptVal>,
        results: Vec<Val>,
    },
    /// `(env) -> (K, V)?`: the step closure code of `Map.iter()`
    /// (`map.rs`).
    MapStep {
        entry: map::Entry,
        opt: Vec<OptVal>,
        results: Vec<Val>,
    },
    /// `(list, n)`: std's `list_truncate`: a pinned list whose length
    /// would change panics; otherwise the buffer is made the list's own,
    /// the elements from `n` on are released and the length becomes `n`.
    ListTruncate { elem: Elem },
    /// `() -> TypeId`: a `TypeId` of static strings.
    TypeIdOf(TypeIdParts),
    /// `(data) -> TypeId`: a vtable's `runtime_type` slot, the `TypeId`
    /// of the type the vtable is built at.
    RuntimeType(TypeIdParts),
    /// A vtable slot no call reaches: it traps.
    DynTrap { params: Vec<Val>, results: Vec<Val> },
    /// `(list: List[u8]) -> string`: a new string of the list's bytes,
    /// which std has checked are UTF-8.
    StrFromBytes,
    /// `(list: List[string]) -> string`: every part's bytes in order,
    /// copied once into one new string.
    ConcatAll,
    /// `(value: i64) -> string`: an integer's decimal text, read as
    /// signed when `signed`.
    FmtInt { signed: bool },
    /// `(a, b) -> bool`: two strings' bytes are equal.
    StrEq,
    /// `(a, b) -> i32`: -1, 0 or 1 as `a` sorts before, with or after `b`,
    /// byte by byte and a prefix first (`expr.ord.string`).
    StrCmp,
    /// `(args) -> result`: the `hd:prim` import `name` (spec/std/README.md,
    /// "Standard library primitives"). A string argument goes in the
    /// exchange buffer, its length the import's parameter; a string result
    /// comes back there, in the host codec, and is copied into a new
    /// buffer.
    HostPrim {
        name: String,
        args: Vec<PrimArg>,
        result: Option<Val>,
    },
    /// The forbidden-context check of `block_on`
    /// (`req.drive.block-on.forbidden-contexts`).
    ForbidCheck,
    /// A host provider's vtable slot for a method that may wait
    /// (`crates/hd_run_wasmtime/DESIGN.md` §17.2): `(data, arguments) ->
    /// frame`, a cold leaf frame (`task.rs`) holding the arguments laid
    /// out as `area`, whose poll is [`Helper::HostPoll`].
    HostCold {
        module: String,
        method: String,
        args: Vec<crate::task::HostArg>,
        area: Elem,
        res: crate::task::HostRes,
    },
    /// `(frame) -> (ready, results)`: the leaf's `.start`, then its
    /// `.finish` once the wake table holds its handle.
    HostPoll {
        module: String,
        method: String,
        args: Vec<crate::task::HostArg>,
        area: Elem,
        res: crate::task::HostRes,
    },
    /// `(frame)`: a leaf's cancel, which aborts a waiting call.
    HostCancel,
    /// A host provider's vtable slot for a method that never waits:
    /// `(data, arguments) -> results`, one import call, its structured
    /// arguments and result in the exchange buffer (`boundary.rs`).
    HostCall {
        module: String,
        method: String,
        args: Vec<crate::task::HostArg>,
        result: crate::boundary::CallRes,
    },
    /// `(data, params) -> results`: a host handle's vtable slot, which
    /// calls `inner`, a host call or a cold leaf, with the handle number
    /// from the box at `data` first (`boundary.rs`).
    HandleSlot {
        inner: Box<Helper>,
        params: Vec<Val>,
        results: Vec<Val>,
    },
    /// `(end)`: grows the exchange buffer to `end` bytes.
    Fit,
    /// `(off, v: i64) -> off'`: unsigned LEB128 into the buffer.
    LebPut,
    /// `(off) -> (v: i64, off')`: unsigned LEB128 from the buffer.
    LebGet,
    /// `(off, values) -> off'`: a boundary value's encoder.
    Enc(crate::boundary::LTy),
    /// `(off) -> (values, off')`: a boundary value's decoder.
    Dec(crate::boundary::LTy),
    /// `() -> frame`: a new frame of the suspending instance (§23.21).
    Frame(Hash128),
    /// `(frame) -> i32`: a poll's runtime checks; 1 answers Pending.
    PollCheck,
    /// `(frame) -> i32`: a cancel's runtime checks; 0 leaves the frame.
    CancelCheck,
    /// `() -> i32`: starts a cancel's `ABANDONED` carry.
    CancelEnter,
    /// `(frame, seen)`: ends it.
    CancelLeave,
    /// `(n)`: `hd.wake(n)`, the completed handles into the wake table.
    WakeMark,
    /// `(h) -> i32`: whether handle `h` completed, which it takes.
    WakeTake,
    /// `hd.cancel` and `hd.abandon`: cancels the pending root, under
    /// `abandon()` for the second.
    EntryCancel { abandon: bool },
    /// `hd.scenario.cleanup`: `() -> i32`, a call of the scenario's
    /// top-level `cleanup_ran`.
    EntryCall { func: Hash128 },
    /// `(address)`: one more count on what the address is in, when the
    /// program counts.
    RetainAny,
    /// `(payload)`: one count less on a frame's child.
    ChildDrop,
    /// `(task) -> frame`: `cancel!`'s cleanup frame (`task.rs`).
    CleanupCold,
    /// `(frame) -> (ready, abandoned)`.
    CleanupPoll,
    /// `(frame)`.
    CleanupCancel,
    /// `() -> frame`: a frame that never completes, of results `results`.
    PendingCold { results: Vec<Val> },
    /// `(frame) -> (ready, results)`: always Pending.
    PendingPoll { results: Vec<Val> },
    /// `(frame)`.
    PendingCancel,
    /// `race!`'s frame over a list of tasks of results `results`: part 0
    /// `(list) -> frame`, 1 its poll, 2 its cancel.
    Race {
        part: u8,
        results: Vec<Val>,
        walk: Option<Walk>,
    },
    /// `(payload)`: a race frame's glue.
    RaceDrop {
        results: Vec<Val>,
        walk: Option<Walk>,
    },
}

/// A list's element as the list glue sees it: its stride, its values in
/// memory, which of them are counted, and which are collections that a
/// copy of the element copies (`layout::coll_leaves`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Elem {
    pub size: u32,
    pub leaves: Vec<Leaf>,
    pub walk: Option<Walk>,
    pub colls: Vec<u32>,
}

impl Elem {
    /// Whether an element copied out of a shared buffer needs more than
    /// its bytes: counts, or copies of its collections.
    #[must_use]
    pub fn counts(&self) -> bool {
        self.walk.is_some() || !self.colls.is_empty()
    }
}

/// A list header's payload (§23.7): the buffer's address, then the
/// owner word: 0 while the buffer may be shared, or, once an element
/// reference pinned the list, the function-table slot of the list's
/// [`Helper::ListClone`] plus one. A buffer whose header has an owner word
/// has that header as its one owner, so each count past the first is an
/// element reference's: the list is pinned while its buffer's count is
/// over 1 (§23.20).
pub const LIST_OWN: u64 = 4;

/// A list buffer's payload: the capacity in elements, the length, then
/// the elements inline from [`LIST_ELEMS`].
pub const LIST_LEN: u64 = 4;
pub const LIST_ELEMS: u32 = 8;

/// The address of the one static empty buffer, shared by every empty list
/// and map literal: its payload's 16 zero bytes are an empty list buffer
/// and an empty map table. Its count word is 2, so a change always copies
/// it first, and as static data it is never counted.
pub fn empty_buffer(m: &mut Module) -> u32 {
    let mut bytes = [0u8; 24];
    bytes[0] = 2;
    m.static_bytes(&bytes, 8) + HEADER
}

/// How a host primitive's argument crosses: a string, or a scalar as
/// itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PrimArg {
    Str,
    Scalar(Val),
}

/// Defines the entry exports of `input.entry` in `m`.
pub fn entry(input: &Input<'_>, m: &mut Module) -> StageResult<()> {
    let (main, key, before, after, report) = match &input.entry {
        Entry::Main {
            main,
            key,
            inits,
            report,
        } => (*main, *key, inits.clone(), Vec::new(), *report),
        Entry::Scenario {
            main,
            key,
            inits,
            report,
            cleanup,
            ..
        } => {
            if let Some(func) = cleanup {
                m.export(
                    "hd.scenario.cleanup",
                    Sym::Helper(Helper::EntryCall { func: *func }),
                );
            }
            (*main, *key, inits.clone(), Vec::new(), *report)
        }
        Entry::Script {
            init,
            key,
            before,
            after,
            report,
        } => (*init, *key, before.clone(), after.clone(), *report),
        Entry::Tests {
            cases,
            inits,
            runtime_inits,
        } => {
            // The module under test is not an entry module
            // (`module.init.tests.no-entry`), so every init is forbidden.
            for (j, ks) in inits.iter().enumerate() {
                let inits = ks.iter().map(|k| (*k, true)).collect();
                m.export(
                    format!("hd.init.{j}"),
                    Sym::Helper(Helper::EntryInit { inits }),
                );
            }
            let mut bang = false;
            let mut reinit: Vec<Hash128> = Vec::new();
            for k in inits.iter().flatten() {
                if !reinit.contains(k) && !runtime_inits.contains(k) {
                    reinit.push(*k);
                }
            }
            for (i, (def, key, report)) in cases.iter().enumerate() {
                let poll = poll(input, m, *def, *key, *report, Vec::new())?;
                let poll = checked(m, poll, reinit.clone());
                bang |= m.frames.contains_key(key);
                m.export(format!("hd.test.{i}"), poll);
            }
            if bang {
                m.export("hd.wake", Sym::Helper(Helper::WakeMark));
            }
            return Ok(());
        }
        Entry::Unsupported(what) => return unsupported(*what),
    };
    m.export("hd.init", Sym::Helper(Helper::EntryInit { inits: before }));
    let poll = poll(input, m, main, key, report, after)?;
    // A scenario's host reads its bindings through
    // `hd.scenario.cleanup` after the root has ended, so it keeps them
    // and has no audit.
    let poll = if matches!(input.entry, Entry::Scenario { .. }) {
        poll
    } else {
        checked(m, poll, Vec::new())
    };
    m.export("hd.poll", poll);
    if m.frames.contains_key(&key) {
        m.export("hd.wake", Sym::Helper(Helper::WakeMark));
        m.export(
            "hd.cancel",
            Sym::Helper(Helper::EntryCancel { abandon: false }),
        );
        m.export(
            "hd.abandon",
            Sym::Helper(Helper::EntryCancel { abandon: true }),
        );
    }
    Ok(())
}

/// The poll of a root: item `main`'s instance `key` with its host
/// providers, its result reported by `report`, then the inits `then`.
fn poll(
    input: &Input<'_>,
    m: &mut Module,
    main: DefId,
    key: Hash128,
    report: Option<Hash128>,
    then: Vec<(Hash128, bool)>,
) -> StageResult<Sym> {
    let mut providers = Vec::new();
    for k in input.env.row_keys(main, TyList::EMPTY) {
        let by_ref = input.host_refs.iter().find(|(t, _)| *t == k).map(|r| r.1);
        providers.push((host_vtable(input, m, k)?, by_ref));
    }
    let ret = input.env.ret(main).unwrap_or(Ty::VOID);
    let rvals = crate::layout::vals(input.pool, input.env, ret)?;
    let bang = m.frames.contains_key(&key).then(|| rvals.clone());
    Ok(Sym::Helper(Helper::EntryPoll {
        main: key,
        providers,
        results: u32::try_from(rvals.len()).expect("results"),
        report,
        then,
        bang,
    }))
}

/// In checked mode, `poll` behind the audit at its end
/// ([`Helper::CheckedPoll`]), then the inits `reinit`; else `poll` itself.
fn checked(m: &Module, poll: Sym, reinit: Vec<Hash128>) -> Sym {
    match poll {
        Sym::Helper(h) if m.runtime.is_some_and(|r| r.checked.is_some()) => {
            Sym::Helper(Helper::CheckedPoll {
                poll: Box::new(h),
                reinit,
            })
        }
        other => other,
    }
}

/// [`Helper::CheckedPoll`].
fn checked_poll(poll: &Helper, reinit: &[Hash128], m: &Module) -> Func {
    let mut f = Func::new("rt:checked_poll", vec![Val::I32], vec![Val::I32]);
    let Some((r, c)) = m.runtime.and_then(|r| Some((r, r.checked?))) else {
        return f;
    };
    let status = f.local(Val::I32);
    f.get(0);
    f.helper(poll.clone());
    f.set(status);
    f.get(status);
    f.i32(-1);
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    // The wake table is the runtime's own, grown on demand: no handle
    // waits once the root has ended.
    f.global_get(Global::WakeTab);
    f.op(Instruction::If(BlockType::Empty));
    f.global_get(Global::WakeTab);
    f.call(Sym::Inst(r.free));
    f.i32(0);
    f.global_set(Global::WakeTab);
    f.i32(0);
    f.global_set(Global::WakeCap);
    f.op(Instruction::End);
    f.helper(Helper::Teardown);
    f.i32(0);
    f.call(Sym::Inst(c.audit));
    for k in reinit {
        call_init(&mut f, *k, true);
    }
    f.op(Instruction::End);
    f.get(status);
    f
}

/// [`Helper::Teardown`]: each binding [`Module::stored`] names releases
/// its values, from its globals or its static slot, and is zeroed.
fn teardown(m: &Module) -> Func {
    let mut f = Func::new("rt:teardown", vec![], vec![]);
    for (h, s) in &m.stored {
        let ls: Vec<u32> = s.vals.iter().map(|v| f.local(*v)).collect();
        for (k, (v, l)) in s.vals.iter().zip(&ls).enumerate() {
            let g = Global::Storage(*h, u32::try_from(k).expect("k"), *v);
            if let Some((at, leaves)) = &s.slot {
                f.u32(*at);
                f.op(leaves[k].load(0));
            } else {
                f.global_get(g);
            }
            f.set(*l);
        }
        if let Walk::Ref(k) = s.walk {
            f.get(ls[k as usize]);
            f.helper(Helper::ReleaseRef);
        } else {
            for l in &ls {
                f.get(*l);
            }
            f.helper(Helper::Release {
                vals: s.vals.clone(),
                walk: s.walk.clone(),
            });
        }
        // Zeros release as nothing, so a later init's store of the
        // binding releases nothing twice.
        for (k, v) in s.vals.iter().enumerate() {
            let g = Global::Storage(*h, u32::try_from(k).expect("k"), *v);
            if let Some((at, leaves)) = &s.slot {
                f.u32(*at);
                crate::task::zero(&mut f, *v);
                f.op(leaves[k].store(0));
            } else {
                crate::task::zero(&mut f, *v);
                f.global_set(g);
            }
        }
    }
    f
}

/// Pushes each host provider: no data beside its vtable of host stubs; for
/// a key whose trait has a `&self` or `&mut self` method, that trait value
/// behind a new object (`req.ref.host.reference-values`), the reference that
/// `std.rt.host_provider` returns, beside the vtable again.
pub fn host_providers(f: &mut Func, providers: &[(usize, Option<Hash128>)]) {
    for (v, by_ref) in providers {
        f.i32(0);
        f.body.push(Ins::VTable(*v));
        if let Some(k) = by_ref {
            f.call(Sym::Inst(*k));
        }
    }
}

/// The vtable of the host's provider of capability `key`, in the trait's
/// method order (`ProgramEnv::trait_methods`): each slot a host call or a
/// host leaf over the exchange buffer's codecs (`boundary.rs`).
fn host_vtable(input: &Input<'_>, m: &mut Module, key: Ty) -> StageResult<usize> {
    let TyData::TraitValue { def, args, .. } = input.pool.get(key) else {
        return unsupported("a provider key that is not a trait");
    };
    let p = (input.path)(def).replace('/', ".");
    if args != TyList::EMPTY {
        return unsupported(format!("the default provider of `{p}`"));
    }
    let Some(host) = hd_host_abi::host_trait(&p) else {
        return profile_vtable(input, m, def, &p);
    };
    let mut slots = Vec::new();
    for method in input.env.trait_methods(def) {
        let mp = (input.path)(method);
        let name = mp.rsplit(['.', '/']).next().unwrap_or("");
        let Some(hm) = host.methods.iter().find(|x| x.name == name) else {
            return unsupported(format!("the host method `{name}`"));
        };
        let row = crate::boundary::Row::of(hm);
        slots.push(Sym::Helper(crate::boundary::host_slot(
            input, m, host.key, &row, method, false, None,
        )?));
    }
    Ok(m.vtable(slots))
}

/// The vtable of a host trait that a runtime profile declares itself
/// (`spec/conformance/README.md`, Runtime Profiles), which the ABI table
/// does not list: each method is imported from `hd:<Trait>` by its name
/// and waits when it suspends; every argument and result crosses in the
/// exchange buffer, so a result gets each check of its declared type
/// (`module.profile.host-result.*`), and a private-field type crosses as
/// its JSON text (`module.boundary.serialize.exact`).
fn profile_vtable(
    input: &Input<'_>,
    m: &mut Module,
    def: hd_base::DefId,
    p: &str,
) -> StageResult<usize> {
    use hd_host_abi::{Codec, Wait};
    let key = p.rsplit('.').next().unwrap_or("");
    let at = crate::boundary::AtProfile {
        codecs: &input.collected.boundary,
        out: false,
        handles: &[],
    };
    let full = input.pool.list(&[hd_mono::class_ref(input.pool)]);
    let mut slots = Vec::new();
    for method in input.env.trait_methods(def) {
        let mp = (input.path)(method);
        let name = mp.rsplit(['.', '/']).next().unwrap_or("").to_owned();
        let params = input.env.params(method).unwrap_or_default();
        let ret = input.env.ret(method).unwrap_or(Ty::VOID);
        let ret = hd_mono::subst(input.pool, input.env, method, full, ret);
        let codecs = vec![Codec::Buffer(""); params.len().saturating_sub(1)];
        let row = crate::boundary::Row {
            name: &name,
            params: &codecs,
            result: if crate::layout::vals(input.pool, input.env, ret)?.is_empty() {
                Codec::Void
            } else {
                Codec::Buffer("")
            },
            wait: if input.env.suspends(method) {
                Wait::May
            } else {
                Wait::Never
            },
        };
        slots.push(Sym::Helper(crate::boundary::host_slot(
            input,
            m,
            key,
            &row,
            method,
            false,
            Some(at),
        )?));
    }
    Ok(m.vtable(slots))
}

/// Adds `by` to the forbidden-context counter.
fn forbid_by(f: &mut Func, by: i32) {
    f.global_get(Global::Forbid);
    f.i32(by);
    f.op(Instruction::I32Add);
    f.global_set(Global::Forbid);
}

/// Calls init `k`, as a forbidden context when flagged.
fn call_init(f: &mut Func, k: Hash128, forbid: bool) {
    if forbid {
        forbid_by(f, 1);
    }
    f.call(Sym::Inst(k));
    if forbid {
        forbid_by(f, -1);
    }
}

fn rt_import(name: &str, params: &[Val], results: &[Val]) -> Sym {
    Sym::Import(Import::new("hd:rt", name, params, results))
}

/// [`Helper::ListStep`]'s function. The environment holds the list's
/// header at 0, the next index at 4 and the length at creation at 8.
fn list_step(elem: &Elem, opt: &[OptVal], results: &[Val]) -> Func {
    let mut f = Func::new("rt:list_step", vec![Val::I32], results.to_vec());
    let b = f.local(Val::I32);
    let i = f.local(Val::I32);
    let at = f.local(Val::I32);
    let out: Vec<u32> = results.iter().map(|v| f.local(*v)).collect();
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(b);
    // A length other than the one at creation is the
    // `iterator-invalidated` panic, checked on every call so an exhausted
    // iterator panics too.
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 8, 2)));
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    f.helper(Helper::Panic(
        "iterator-invalidated: list changed during iteration".into(),
    ));
    f.op(Instruction::End);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.set(i);
    f.get(i);
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.op(Instruction::I32LtU);
    f.op(Instruction::If(BlockType::Empty));
    f.get(0);
    f.get(i);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Store(mem(HEAP, 4, 2)));
    f.get(b);
    f.get(i);
    f.u32(elem.size);
    f.op(Instruction::I32Mul);
    f.op(Instruction::I32Add);
    f.set(at);
    let mut vals = Vec::new();
    for (p, o) in opt.iter().enumerate() {
        match o {
            OptVal::Tag => {
                f.i32(1);
                f.set(out[p]);
            }
            OptVal::Elem(k) => {
                f.get(at);
                f.op(elem.leaves[*k as usize].load(LIST_ELEMS));
                f.set(out[p]);
                vals.push((*k, out[p]));
            }
            OptVal::Zero => {}
        }
    }
    // The element leaves with a count of its own.
    if let Some(w) = &elem.walk {
        vals.sort_unstable();
        for (_, l) in &vals {
            f.get(*l);
        }
        f.helper(Helper::Retain {
            vals: elem.leaves.iter().map(|l| l.val).collect(),
            walk: w.clone(),
        });
    }
    f.op(Instruction::End);
    for l in &out {
        f.get(*l);
    }
    f
}

/// [`Helper::DynRead`]'s function: the value at the data's address, lent
/// to the call as the receiver.
fn dyn_read(target: Hash128, read: &RefRead, params: &[Val], results: &[Val]) -> Func {
    let mut all = vec![Val::I32];
    all.extend(params.iter().copied());
    let mut f = Func::new("rt:dyn_read", all, results.to_vec());
    for l in &read.leaves {
        f.get(0);
        f.op(l.load(0));
    }
    for k in 1..=params.len() {
        f.get(u32::try_from(k).expect("params"));
    }
    f.call(Sym::Inst(target));
    f
}

/// Writes the literal `text` to standard error.
fn write_lit(f: &mut Func, m: &mut Module, text: &str) {
    let at = m.static_bytes(text.as_bytes(), 1);
    f.u32(at);
    f.u32(u32::try_from(text.len()).expect("literal"));
    f.helper(Helper::ToBuf);
    f.call(rt_import("stderr", &[Val::I32], &[]));
}

/// Writes the string in locals `s` and `s + 1` to standard error.
fn write_str(f: &mut Func, s: u32) {
    f.get(s);
    f.get(s + 1);
    f.helper(Helper::ToBuf);
    f.call(rt_import("stderr", &[Val::I32], &[]));
}

/// Builds helper `h`; it may ask `m` for more helpers and static data.
pub fn helper(h: &Helper, m: &mut Module) -> Func {
    match h {
        Helper::Frame(_)
        | Helper::PollCheck
        | Helper::CancelCheck
        | Helper::CancelEnter
        | Helper::CancelLeave
        | Helper::WakeMark
        | Helper::WakeTake
        | Helper::HostCold { .. }
        | Helper::HostPoll { .. }
        | Helper::HostCancel
        | Helper::HostCall { .. }
        | Helper::EntryCancel { .. }
        | Helper::ChildDrop
        | Helper::CleanupCold
        | Helper::CleanupPoll
        | Helper::CleanupCancel
        | Helper::PendingCold { .. }
        | Helper::PendingPoll { .. }
        | Helper::PendingCancel
        | Helper::Race { .. }
        | Helper::RaceDrop { .. } => crate::task::helper(h, m),
        Helper::HandleSlot {
            inner,
            params,
            results,
        } => crate::boundary::handle_slot(inner, params, results),
        Helper::Fit => crate::boundary::fit(),
        Helper::LebPut => crate::boundary::leb_put(),
        Helper::LebGet => crate::boundary::leb_get(),
        Helper::Enc(b) => crate::boundary::enc(b, m),
        Helper::Dec(b) => crate::boundary::dec(b, m),
        Helper::EntryCall { func } => {
            let mut f = Func::new("rt:entry_call", vec![], vec![Val::I32]);
            f.call(Sym::Inst(*func));
            f
        }
        Helper::EntryInit { inits } => {
            let mut f = Func::new("rt:entry_init", vec![], vec![]);
            for (k, forbid) in inits {
                call_init(&mut f, *k, *forbid);
            }
            f
        }
        Helper::EntryPoll {
            main,
            providers,
            results: _,
            report,
            then: _,
            bang: Some(rvals),
        } => crate::task::root_poll(*main, providers, rvals, *report),
        Helper::EntryPoll {
            main,
            providers,
            results,
            report,
            then,
            bang: None,
        } => {
            // Parameter 0 is the driver's `PollContext`; a plain entry
            // ignores it.
            let mut f = Func::new("rt:entry_poll", vec![Val::I32], vec![Val::I32]);
            host_providers(&mut f, providers);
            f.call(Sym::Inst(*main));
            if let Some(k) = report {
                f.call(Sym::Inst(*k));
            } else {
                for _ in 0..*results {
                    f.op(Instruction::Drop);
                }
                // A script's later inits: its own bodies, which share the
                // first's result, and the others, as forbidden contexts.
                for (k, forbid) in then {
                    call_init(&mut f, *k, *forbid);
                    if !forbid {
                        for _ in 0..*results {
                            f.op(Instruction::Drop);
                        }
                    }
                }
                f.i32(0);
            }
            f
        }
        Helper::CheckedPoll { poll, reinit } => checked_poll(poll, reinit, m),
        Helper::Teardown => teardown(m),
        Helper::Panic(msg) => {
            let mut f = Func::new("rt:panic", vec![], vec![]);
            f.call(rt_import("panic", &[], &[]));
            write_lit(&mut f, m, &format!("{msg}\n"));
            f.op(Instruction::Unreachable);
            f
        }
        Helper::PanicStr => {
            let mut f = Func::new("rt:panic_str", vec![Val::I32, Val::I32], vec![]);
            f.call(rt_import("panic", &[], &[]));
            write_lit(&mut f, m, "explicit-panic: ");
            write_str(&mut f, 0);
            write_lit(&mut f, m, "\n");
            f.op(Instruction::Unreachable);
            f
        }
        Helper::PanicWith => {
            let mut f = Func::new("rt:panic_with", vec![Val::I32; 4], vec![]);
            f.call(rt_import("panic", &[], &[]));
            write_str(&mut f, 0);
            f.get(3);
            f.op(Instruction::If(BlockType::Empty));
            write_lit(&mut f, m, ": ");
            write_str(&mut f, 2);
            f.op(Instruction::End);
            write_lit(&mut f, m, "\n");
            f.op(Instruction::Unreachable);
            f
        }
        Helper::ToBuf => {
            let mut f = Func::new("rt:to_buf", vec![Val::I32, Val::I32], vec![Val::I32]);
            // Grow the buffer by the pages the text needs beyond its size.
            f.get(1);
            f.op(Instruction::MemorySize(BUF));
            f.i32(16);
            f.op(Instruction::I32Shl);
            f.op(Instruction::I32GtU);
            f.op(Instruction::If(BlockType::Empty));
            f.get(1);
            f.op(Instruction::MemorySize(BUF));
            f.i32(16);
            f.op(Instruction::I32Shl);
            f.op(Instruction::I32Sub);
            f.i32(0xffff);
            f.op(Instruction::I32Add);
            f.i32(16);
            f.op(Instruction::I32ShrU);
            f.op(Instruction::MemoryGrow(BUF));
            f.i32(-1);
            f.op(Instruction::I32Eq);
            f.op(Instruction::If(BlockType::Empty));
            f.helper(Helper::Panic(
                "heap-exhausted: the exchange buffer cannot grow".into(),
            ));
            f.op(Instruction::End);
            f.op(Instruction::End);
            f.i32(0);
            f.get(0);
            f.get(1);
            f.op(Instruction::MemoryCopy {
                src_mem: HEAP,
                dst_mem: BUF,
            });
            f.get(1);
            f
        }
        Helper::Alloc => alloc(),
        Helper::StrEq => str_eq(),
        Helper::New => new_object(m),
        Helper::Str => {
            let mut f = Func::new("rt:str", vec![Val::I32], vec![Val::I32]);
            f.get(0);
            f.i32(0);
            f.helper(Helper::New);
            f
        }
        Helper::Retain { vals, walk } => {
            let mut f = Func::new("rt:retain", vals.clone(), vec![]);
            let on = retain_call(m);
            walk_code(&mut f, walk, &|f, k| f.get(k), &on);
            f
        }
        Helper::Release { vals, walk } => {
            let mut f = Func::new("rt:release", vals.clone(), vec![]);
            walk_code(&mut f, walk, &|f, k| f.get(k), &|f| {
                f.helper(Helper::ReleaseRef);
            });
            f
        }
        Helper::ReleaseRef => release_ref(m),
        Helper::DropFields { leaves, walk } => fields_glue("rt:drop", leaves, walk, &|f| {
            f.helper(Helper::ReleaseRef);
        }),
        Helper::RetainFields { leaves, walk } => {
            fields_glue("rt:retain_fields", leaves, walk, &retain_call(m))
        }
        Helper::ListHeader => list_header(),
        Helper::ListPinned => list_pinned(),
        Helper::ListClone { elem } => list_clone(elem),
        Helper::ListBuf { elem } => list_buf(elem),
        Helper::DropElems { elem } => drop_elems(elem),
        Helper::ListCopy => list_copy(m),
        Helper::CopyElems { elem } => copy_elems(elem, m),
        Helper::ListUnique { elem, push } => list_unique(elem, *push, m),
        Helper::MapTable { entry } => map::table(entry),
        Helper::DropEntries { entry } => map::drop_entries(entry),
        Helper::MapHash { key, ops } => map::hash(key, ops),
        Helper::MapFind { entry, ops } => map::find(entry, ops),
        Helper::MapPut { entry, ops } => map::put(entry, ops),
        Helper::MapGrow { entry } => map::grow(entry, m),
        Helper::MapUnique { entry } => map::unique(entry, m),
        Helper::MapSlotNext => map::slot_next(),
        Helper::DynSlot {
            target,
            leaves,
            params,
            results,
        } => {
            let mut all = vec![Val::I32];
            all.extend(params.iter().copied());
            let mut f = Func::new("rt:dyn_slot", all, results.clone());
            match leaves {
                Some(ls) => {
                    for l in ls {
                        f.get(0);
                        f.op(l.load(0));
                    }
                }
                None => f.get(0),
            }
            for k in 1..=params.len() {
                f.get(u32::try_from(k).expect("params"));
            }
            f.call(Sym::Inst(*target));
            f
        }
        Helper::DynRead {
            target,
            read,
            params,
            results,
        } => dyn_read(*target, read, params, results),
        Helper::ListStep { elem, opt, results } => list_step(elem, opt, results),
        Helper::ListTruncate { elem } => list_truncate(elem),
        Helper::TypeIdOf(parts) => type_id(parts, vec![], m),
        Helper::RuntimeType(parts) => type_id(parts, vec![Val::I32], m),
        Helper::MapStep {
            entry,
            opt,
            results,
        } => map::step(entry, opt, results),
        Helper::DynTrap { params, results } => {
            let mut f = Func::new("rt:dyn_trap", params.clone(), results.clone());
            f.op(Instruction::Unreachable);
            f
        }
        Helper::StrFromBytes => str_from_bytes(),
        Helper::ConcatAll => concat_all(),
        Helper::StrCmp => str_cmp(),
        Helper::HostPrim { name, args, result } => host_prim(name, args, *result),
        Helper::FmtInt { signed } => fmt_int(*signed),
        Helper::ForbidCheck => {
            let mut f = Func::new("rt:forbid_check", vec![], vec![]);
            f.global_get(Global::Forbid);
            f.op(Instruction::If(BlockType::Empty));
            f.helper(Helper::Panic(
                "suspension-forbidden-context: block_on in a forbidden context".into(),
            ));
            f.op(Instruction::End);
            f
        }
        Helper::RetainAny => {
            let mut f = Func::new("rt:retain_any", vec![Val::I32], vec![]);
            if let Some(r) = m.runtime {
                f.get(0);
                f.call(Sym::Inst(r.retain));
            }
            f
        }
    }
}

/// [`Helper::Alloc`]: a bump allocation from `HeapTop` up to `HeapEnd`,
/// the end of the pages it owns. One that does not fit starts on pages it
/// grows for itself, so it never reaches a page that `std.rt.memory.grow`
/// gave std (`module.rt.memory.grow.owner`); the rest of its old pages is
/// left unused.
fn alloc() -> Func {
    let mut f = Func::new("rt:alloc", vec![Val::I32], vec![Val::I32]);
    let (at, n, pages) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    let exhausted = |f: &mut Func| {
        f.op(Instruction::If(BlockType::Empty));
        f.helper(Helper::Panic(
            "heap-exhausted: an allocation does not fit the heap".into(),
        ));
        f.op(Instruction::End);
    };
    // n = (size + 7) & !7, refusing a wrap past 4 GiB.
    f.get(0);
    f.i32(7);
    f.op(Instruction::I32Add);
    f.i32(-8);
    f.op(Instruction::I32And);
    f.set(n);
    f.get(n);
    f.get(0);
    f.op(Instruction::I32LtU);
    exhausted(&mut f);
    f.global_get(Global::HeapTop);
    f.set(at);
    // It fits when n <= HeapEnd - at.
    f.get(n);
    f.global_get(Global::HeapEnd);
    f.get(at);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32GtU);
    f.op(Instruction::If(BlockType::Empty));
    // pages = n / 64 KiB, rounded up.
    f.get(n);
    f.i32(16);
    f.op(Instruction::I32ShrU);
    f.get(n);
    f.i32(0xffff);
    f.op(Instruction::I32And);
    f.i32(0);
    f.op(Instruction::I32Ne);
    f.op(Instruction::I32Add);
    f.set(pages);
    f.get(pages);
    f.op(Instruction::MemoryGrow(HEAP));
    f.set(at);
    f.get(at);
    f.i32(-1);
    f.op(Instruction::I32Eq);
    exhausted(&mut f);
    f.get(at);
    f.get(pages);
    f.op(Instruction::I32Add);
    f.i32(16);
    f.op(Instruction::I32Shl);
    f.global_set(Global::HeapEnd);
    f.get(at);
    f.i32(16);
    f.op(Instruction::I32Shl);
    f.set(at);
    f.op(Instruction::End);
    f.get(at);
    f.get(n);
    f.op(Instruction::I32Add);
    f.global_set(Global::HeapTop);
    f.get(at);
    f
}

/// [`Helper::FmtInt`]: the digits go backward into a 20-byte buffer,
/// the most a `u64` or a signed `i64` needs; the string is its tail.
fn fmt_int(signed: bool) -> Func {
    let mut f = Func::new(
        if signed { "rt:fmt_i64" } else { "rt:fmt_u64" },
        vec![Val::I64],
        vec![Val::I32, Val::I32],
    );
    let (buf, at, neg, v) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I64),
    );
    f.i32(20);
    f.helper(Helper::Str);
    f.set(buf);
    f.get(buf);
    f.i32(20);
    f.op(Instruction::I32Add);
    f.set(at);
    f.get(0);
    f.set(v);
    if signed {
        // The magnitude of a negative value, read unsigned, is `0 - v`:
        // that holds for `i64::MIN` too.
        f.get(0);
        f.op(Instruction::I64Const(0));
        f.op(Instruction::I64LtS);
        f.set(neg);
        f.get(neg);
        f.op(Instruction::If(BlockType::Empty));
        f.op(Instruction::I64Const(0));
        f.get(0);
        f.op(Instruction::I64Sub);
        f.set(v);
        f.op(Instruction::End);
    }
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(at);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.set(at);
    f.get(at);
    f.get(v);
    f.op(Instruction::I64Const(10));
    f.op(Instruction::I64RemU);
    f.op(Instruction::I32WrapI64);
    f.i32(i32::from(b'0'));
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Store8(mem(HEAP, 0, 0)));
    f.get(v);
    f.op(Instruction::I64Const(10));
    f.op(Instruction::I64DivU);
    f.set(v);
    f.get(v);
    f.op(Instruction::I64Const(0));
    f.op(Instruction::I64Ne);
    f.op(Instruction::BrIf(0));
    f.op(Instruction::End);
    if signed {
        f.get(neg);
        f.op(Instruction::If(BlockType::Empty));
        f.get(at);
        f.i32(1);
        f.op(Instruction::I32Sub);
        f.set(at);
        f.get(at);
        f.i32(i32::from(b'-'));
        f.op(Instruction::I32Store8(mem(HEAP, 0, 0)));
        f.op(Instruction::End);
    }
    f.get(at);
    f.get(buf);
    f.i32(20);
    f.op(Instruction::I32Add);
    f.get(at);
    f.op(Instruction::I32Sub);
    f
}

/// [`Helper::StrEq`]: equal lengths, then each byte.
fn str_eq() -> Func {
    let mut f = Func::new("rt:str_eq", vec![Val::I32; 4], vec![Val::I32]);
    let k = f.local(Val::I32);
    f.get(1);
    f.get(3);
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(k);
    f.get(1);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    f.get(0);
    f.get(k);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Load8U(mem(HEAP, 0, 0)));
    f.get(2);
    f.get(k);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Load8U(mem(HEAP, 0, 0)));
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(k);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(k);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    f.i32(1);
    f
}

/// [`Helper::StrCmp`]: the first differing byte decides; when the shorter
/// is a prefix of the longer, the lengths do.
fn str_cmp() -> Func {
    let mut f = Func::new("rt:str_cmp", vec![Val::I32; 4], vec![Val::I32]);
    let (k, n, x, y) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    // The sign of `x - y` as -1, 0 or 1, for two values in locals.
    let sign = |f: &mut Func, x: u32, y: u32| {
        f.get(x);
        f.get(y);
        f.op(Instruction::I32GtU);
        f.get(x);
        f.get(y);
        f.op(Instruction::I32LtU);
        f.op(Instruction::I32Sub);
    };
    f.get(1);
    f.get(3);
    f.get(1);
    f.get(3);
    f.op(Instruction::I32LtU);
    f.op(Instruction::Select);
    f.set(n);
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(k);
    f.get(n);
    f.op(Instruction::I32GeU);
    f.op(Instruction::BrIf(1));
    f.get(0);
    f.get(k);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Load8U(mem(HEAP, 0, 0)));
    f.set(x);
    f.get(2);
    f.get(k);
    f.op(Instruction::I32Add);
    f.op(Instruction::I32Load8U(mem(HEAP, 0, 0)));
    f.set(y);
    f.get(x);
    f.get(y);
    f.op(Instruction::I32Ne);
    f.op(Instruction::If(BlockType::Empty));
    sign(&mut f, x, y);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(k);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(k);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
    sign(&mut f, 1, 3);
    f
}

/// [`Helper::HostPrim`]. The host encodes a string result as its LEB128
/// length and then its bytes, and answers the encoding's length, so the
/// bytes are what follows the prefix: the bytes up to the first without
/// its high bit.
fn host_prim(name: &str, args: &[PrimArg], result: Option<Val>) -> Func {
    let mut params = Vec::new();
    let mut imports = Vec::new();
    for a in args {
        match a {
            PrimArg::Str => {
                params.extend([Val::I32, Val::I32]);
                imports.push(Val::I32);
            }
            PrimArg::Scalar(v) => {
                params.push(*v);
                imports.push(*v);
            }
        }
    }
    let results = match result {
        Some(v) => vec![v],
        None => vec![Val::I32, Val::I32],
    };
    let mut f = Func::new(format!("rt:prim/{name}"), params, results);
    let mut p = 0;
    for a in args {
        f.get(p);
        p += 1;
        if *a == PrimArg::Str {
            f.get(p);
            p += 1;
            f.helper(Helper::ToBuf);
        }
    }
    let import = Import::new("hd:prim", name, &imports, &[result.unwrap_or(Val::I32)]);
    f.call(Sym::Import(import));
    if result.is_some() {
        return f;
    }
    let (n, at, len, out) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    f.set(n);
    // `at`: one past the length prefix.
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(at);
    f.op(Instruction::I32Load8U(mem(BUF, 0, 0)));
    f.get(at);
    f.i32(1);
    f.op(Instruction::I32Add);
    f.set(at);
    f.i32(0x80);
    f.op(Instruction::I32And);
    f.op(Instruction::BrIf(0));
    f.op(Instruction::End);
    f.get(n);
    f.get(at);
    f.op(Instruction::I32Sub);
    f.set(len);
    f.get(len);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::If(BlockType::Empty));
    f.u32(EMPTY);
    f.i32(0);
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(len);
    f.helper(Helper::Str);
    f.set(out);
    f.get(out);
    f.get(at);
    f.get(len);
    f.op(Instruction::MemoryCopy {
        src_mem: BUF,
        dst_mem: HEAP,
    });
    f.get(out);
    f.get(len);
    f
}

/// What retains the address on the stack: std's `retain`, or nothing when
/// the program counts nothing.
pub(crate) fn retain_call(m: &Module) -> impl Fn(&mut Func) {
    let retain = m.runtime.map(|r| r.retain);
    move |f: &mut Func| match retain {
        Some(k) => f.call(Sym::Inst(k)),
        None => f.op(Instruction::Drop),
    }
}

/// Emits `walk` over the values that `get(k)` pushes, running `on` with
/// each counted address on the stack: an enum's arms by its tag.
pub(crate) fn walk_code(
    f: &mut Func,
    walk: &Walk,
    get: &dyn Fn(&mut Func, u32),
    on: &dyn Fn(&mut Func),
) {
    match walk {
        Walk::Ref(k) => {
            get(f, *k);
            on(f);
        }
        Walk::Seq(ws) => {
            for w in ws {
                walk_code(f, w, get, on);
            }
        }
        Walk::Switch(tag, arms) => {
            for (v, arm) in arms.iter().enumerate() {
                let Some(arm) = arm else {
                    continue;
                };
                get(f, *tag);
                f.u32(u32::try_from(v).expect("variants"));
                f.op(Instruction::I32Eq);
                f.op(Instruction::If(BlockType::Empty));
                walk_code(f, arm, get, on);
                f.op(Instruction::End);
            }
        }
    }
}

/// A `(payload)` glue function: loads every leaf of the payload, then
/// walks them.
fn fields_glue(name: &str, leaves: &[Leaf], walk: &Walk, on: &dyn Fn(&mut Func)) -> Func {
    let mut f = Func::new(name, vec![Val::I32], vec![]);
    let ls: Vec<u32> = leaves.iter().map(|l| f.local(l.val)).collect();
    for (l, at) in leaves.iter().zip(&ls) {
        f.get(0);
        f.op(l.load(0));
        f.set(*at);
    }
    walk_code(&mut f, walk, &|f, k| f.get(ls[k as usize]), on);
    f
}

/// [`Helper::New`]: a block of the header and the payload from std's
/// `alloc`, or from the bump pointer when the program counts nothing.
fn new_object(m: &Module) -> Func {
    let mut f = Func::new("rt:new", vec![Val::I32, Val::I32], vec![Val::I32]);
    let block = f.local(Val::I32);
    f.get(0);
    f.u32(HEADER);
    f.op(Instruction::I32Add);
    match m.runtime {
        Some(r) => f.call(Sym::Inst(r.alloc)),
        None => f.helper(Helper::Alloc),
    }
    f.set(block);
    f.get(block);
    f.i32(1);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(block);
    f.get(1);
    f.op(Instruction::I32Store(mem(HEAP, 4, 2)));
    f.get(block);
    f.u32(HEADER);
    f.op(Instruction::I32Add);
    f
}

/// [`Helper::ReleaseRef`]: std's `release` gives the block at its last
/// count; then the descriptor's glue releases the payload's references and
/// std's `free` takes the block.
fn release_ref(m: &Module) -> Func {
    let mut f = Func::new("rt:release_ref", vec![Val::I32], vec![]);
    let Some(r) = m.runtime else {
        return f;
    };
    let (block, desc) = (f.local(Val::I32), f.local(Val::I32));
    f.get(0);
    f.call(Sym::Inst(r.release));
    f.set(block);
    f.get(block);
    f.op(Instruction::If(BlockType::Empty));
    f.get(block);
    f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
    f.set(desc);
    f.get(desc);
    f.op(Instruction::If(BlockType::Empty));
    f.get(block);
    f.u32(HEADER);
    f.op(Instruction::I32Add);
    f.get(desc);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.body.push(Ins::CallIndirect(vec![Val::I32], vec![]));
    f.op(Instruction::End);
    f.get(block);
    f.call(Sym::Inst(r.free));
    f.op(Instruction::End);
    f
}

/// The leaf of a list header's buffer address, which its release glue
/// releases.
const LIST_BUF_LEAF: Leaf = Leaf {
    offset: 0,
    val: Val::I32,
    bytes: 4,
    signed: false,
};

/// [`Helper::ListHeader`].
/// [`Helper::ListCopy`]: a new header over the same buffer, which counts
/// one more owner. A pinned list's buffer has one owner, so its copy is a
/// copy of the buffer (`ref.elem.pin`); an unpinned list whose header
/// still has an owner word gives the word up, as its buffer is shared now.
fn list_copy(m: &Module) -> Func {
    let mut f = Func::new("rt:list_copy", vec![Val::I32], vec![Val::I32]);
    let (b, own) = (f.local(Val::I32), f.local(Val::I32));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(b);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, LIST_OWN, 2)));
    f.set(own);
    f.get(own);
    f.op(Instruction::If(BlockType::Empty));
    f.get(b);
    f.u32(HEADER);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.i32(1);
    f.op(Instruction::I32GtU);
    f.op(Instruction::If(BlockType::Empty));
    f.get(0);
    f.get(own);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.body
        .push(Ins::CallIndirect(vec![Val::I32], vec![Val::I32]));
    f.op(Instruction::Return);
    f.op(Instruction::End);
    f.get(0);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, LIST_OWN, 2)));
    f.op(Instruction::End);
    f.get(b);
    retain_call(m)(&mut f);
    f.get(b);
    f.helper(Helper::ListHeader);
    f
}

/// [`Helper::ListPinned`].
fn list_pinned() -> Func {
    let mut f = Func::new("rt:list_pinned", vec![Val::I32], vec![Val::I32]);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, LIST_OWN, 2)));
    f.i32(0);
    f.op(Instruction::I32Ne);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.u32(HEADER);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.i32(1);
    f.op(Instruction::I32GtU);
    f.op(Instruction::I32And);
    f
}

/// [`Helper::ListClone`].
fn list_clone(elem: &Elem) -> Func {
    let mut f = Func::new("rt:list_clone", vec![Val::I32], vec![Val::I32]);
    let (b, n, nb) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(b);
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(n);
    f.get(n);
    f.helper(Helper::ListBuf { elem: elem.clone() });
    f.set(nb);
    f.get(nb);
    f.get(n);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
    f.get(nb);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(b);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(n);
    f.u32(elem.size);
    f.op(Instruction::I32Mul);
    f.op(Instruction::MemoryCopy {
        src_mem: HEAP,
        dst_mem: HEAP,
    });
    if elem.counts() {
        f.get(nb);
        f.u32(LIST_ELEMS);
        f.op(Instruction::I32Add);
        f.get(n);
        f.helper(Helper::CopyElems { elem: elem.clone() });
    }
    f.get(nb);
    f.helper(Helper::ListHeader);
    f
}

fn list_header() -> Func {
    let mut f = Func::new("rt:list_header", vec![Val::I32], vec![Val::I32]);
    let h = f.local(Val::I32);
    f.i32(8);
    f.body.push(Ins::TableSlot(Sym::Helper(Helper::DropFields {
        leaves: vec![LIST_BUF_LEAF],
        walk: Walk::Ref(0),
    })));
    f.i32(1);
    f.op(Instruction::I32Add);
    f.helper(Helper::New);
    f.set(h);
    f.get(h);
    f.get(0);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(h);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, LIST_OWN, 2)));
    f.get(h);
    f
}

/// [`Helper::ListBuf`]: a capacity whose bytes pass 4 GiB is
/// `heap-exhausted`.
fn list_buf(elem: &Elem) -> Func {
    let mut f = Func::new("rt:list_buf", vec![Val::I32], vec![Val::I32]);
    let b = f.local(Val::I32);
    if let Some(most) = (u32::MAX - LIST_ELEMS - HEADER).checked_div(elem.size) {
        f.get(0);
        f.u32(most);
        f.op(Instruction::I32GtU);
        f.op(Instruction::If(BlockType::Empty));
        f.helper(Helper::Panic(
            "heap-exhausted: a list does not fit the heap".into(),
        ));
        f.op(Instruction::End);
    }
    f.get(0);
    f.u32(elem.size);
    f.op(Instruction::I32Mul);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    if elem.walk.is_some() {
        f.body.push(Ins::TableSlot(Sym::Helper(Helper::DropElems {
            elem: elem.clone(),
        })));
        f.i32(1);
        f.op(Instruction::I32Add);
    } else {
        f.i32(0);
    }
    f.helper(Helper::New);
    f.set(b);
    f.get(b);
    f.get(0);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.get(b);
    f.i32(0);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
    f.get(b);
    f
}

/// Emits a loop over the `n` elements of `size` bytes from the address in
/// local `at`, which it advances: `body` runs with `at` at each element.
fn each_elem(f: &mut Func, at: u32, n: u32, size: u32, body: &dyn Fn(&mut Func)) {
    f.op(Instruction::Block(BlockType::Empty));
    f.op(Instruction::Loop(BlockType::Empty));
    f.get(n);
    f.op(Instruction::I32Eqz);
    f.op(Instruction::BrIf(1));
    body(f);
    f.get(at);
    f.u32(size);
    f.op(Instruction::I32Add);
    f.set(at);
    f.get(n);
    f.i32(1);
    f.op(Instruction::I32Sub);
    f.set(n);
    f.op(Instruction::Br(0));
    f.op(Instruction::End);
    f.op(Instruction::End);
}

/// A function of `params`, which it ignores, that returns a `TypeId`: its
/// key and its name, two views of static data.
fn type_id(parts: &TypeIdParts, params: Vec<Val>, m: &mut Module) -> Func {
    let mut f = Func::new("rt:type_id", params, vec![Val::I32; 4]);
    for text in [&parts.name, &parts.shown] {
        f.u32(m.static_bytes(text.as_bytes(), 1));
        f.u32(u32::try_from(text.len()).expect("type name"));
    }
    f
}

/// [`Helper::ListTruncate`].
fn list_truncate(elem: &Elem) -> Func {
    let mut f = Func::new("rt:list_truncate", vec![Val::I32, Val::I32], vec![]);
    let (b, at, n) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    f.get(0);
    f.helper(Helper::ListPinned);
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.get(1);
    f.op(Instruction::I32Ne);
    f.op(Instruction::I32And);
    f.op(Instruction::If(BlockType::Empty));
    f.helper(Helper::Panic(
        "collection-pinned: list length changed while pinned".into(),
    ));
    f.op(Instruction::End);
    f.get(0);
    f.helper(Helper::ListUnique {
        elem: elem.clone(),
        push: false,
    });
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(b);
    if let Some(walk) = &elem.walk {
        f.get(b);
        f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
        f.get(1);
        f.op(Instruction::I32Sub);
        f.set(n);
        f.get(b);
        f.u32(LIST_ELEMS);
        f.op(Instruction::I32Add);
        f.get(1);
        f.u32(elem.size);
        f.op(Instruction::I32Mul);
        f.op(Instruction::I32Add);
        f.set(at);
        let ls = load_elem_locals(&mut f, &elem.leaves);
        each_elem(&mut f, at, n, elem.size, &|f| {
            fill_elem(f, at, &elem.leaves, &ls);
            walk_code(f, walk, &|f, k| f.get(ls[k as usize]), &|f| {
                f.helper(Helper::ReleaseRef);
            });
        });
    }
    f.get(b);
    f.get(1);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
    f
}

/// [`Helper::DropElems`].
fn drop_elems(elem: &Elem) -> Func {
    let mut f = Func::new("rt:drop_elems", vec![Val::I32], vec![]);
    let (at, n) = (f.local(Val::I32), f.local(Val::I32));
    let Some(walk) = &elem.walk else {
        return f;
    };
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(n);
    f.get(0);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.set(at);
    let ls = load_elem_locals(&mut f, &elem.leaves);
    each_elem(&mut f, at, n, elem.size, &|f| {
        fill_elem(f, at, &elem.leaves, &ls);
        walk_code(f, walk, &|f, k| f.get(ls[k as usize]), &|f| {
            f.helper(Helper::ReleaseRef);
        });
    });
    f
}

/// Fresh locals for an element's leaves.
fn load_elem_locals(f: &mut Func, leaves: &[Leaf]) -> Vec<u32> {
    leaves.iter().map(|l| f.local(l.val)).collect()
}

/// Loads each leaf of the element at local `at` into its local of `ls`.
fn fill_elem(f: &mut Func, at: u32, leaves: &[Leaf], ls: &[u32]) {
    for (l, x) in leaves.iter().zip(ls) {
        f.get(at);
        f.op(l.load(0));
        f.set(*x);
    }
}

/// [`Helper::CopyElems`].
fn copy_elems(elem: &Elem, m: &Module) -> Func {
    let mut f = Func::new("rt:copy_elems", vec![Val::I32, Val::I32], vec![]);
    let ls = load_elem_locals(&mut f, &elem.leaves);
    let walk = elem.walk.as_ref().and_then(|w| w.without(&elem.colls));
    let retain = retain_call(m);
    each_elem(&mut f, 0, 1, elem.size, &|f| {
        fill_elem(f, 0, &elem.leaves, &ls);
        if let Some(w) = &walk {
            walk_code(f, w, &|f, k| f.get(ls[k as usize]), &retain);
        }
        for c in &elem.colls {
            let l = elem.leaves[*c as usize];
            f.get(0);
            f.get(ls[*c as usize]);
            f.helper(Helper::ListCopy);
            f.op(Instruction::I32Store(mem(HEAP, u64::from(l.offset), 2)));
        }
    });
    f
}

/// [`Helper::ListUnique`]: a shared buffer is copied at its length, or
/// at twice it plus 4 for a `push`; its elements take their own counts and
/// the old buffer one owner less. A full buffer of the list's own moves
/// to the larger one, and its old block is freed with no release, since
/// its elements moved.
fn list_unique(elem: &Elem, push: bool, m: &Module) -> Func {
    let mut f = Func::new("rt:list_unique", vec![Val::I32], vec![]);
    let (b, n, shared, nb) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(b);
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(n);
    f.get(b);
    f.u32(HEADER);
    f.op(Instruction::I32Sub);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.i32(1);
    f.op(Instruction::I32Ne);
    // A header with an owner word is its buffer's one owner; the other
    // counts are element references, which see the change in place.
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, LIST_OWN, 2)));
    f.op(Instruction::I32Eqz);
    f.op(Instruction::I32And);
    f.set(shared);
    f.get(shared);
    if push {
        f.get(n);
        f.get(b);
        f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
        f.op(Instruction::I32Eq);
        f.op(Instruction::I32Or);
    }
    f.op(Instruction::If(BlockType::Empty));
    f.get(n);
    if push {
        f.i32(2);
        f.op(Instruction::I32Mul);
        f.i32(4);
        f.op(Instruction::I32Add);
    }
    f.helper(Helper::ListBuf { elem: elem.clone() });
    f.set(nb);
    f.get(nb);
    f.get(n);
    f.op(Instruction::I32Store(mem(HEAP, LIST_LEN, 2)));
    f.get(nb);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(b);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(n);
    f.u32(elem.size);
    f.op(Instruction::I32Mul);
    f.op(Instruction::MemoryCopy {
        src_mem: HEAP,
        dst_mem: HEAP,
    });
    f.get(shared);
    f.op(Instruction::If(BlockType::Empty));
    if elem.counts() {
        f.get(nb);
        f.u32(LIST_ELEMS);
        f.op(Instruction::I32Add);
        f.get(n);
        f.helper(Helper::CopyElems { elem: elem.clone() });
    }
    f.get(b);
    f.helper(Helper::ReleaseRef);
    f.op(Instruction::Else);
    if let Some(r) = m.runtime {
        f.get(b);
        f.u32(HEADER);
        f.op(Instruction::I32Sub);
        f.call(Sym::Inst(r.free));
    }
    f.op(Instruction::End);
    f.get(0);
    f.get(nb);
    f.op(Instruction::I32Store(mem(HEAP, 0, 2)));
    f.op(Instruction::End);
    f
}

/// [`Helper::StrFromBytes`]: no bytes is the static empty string.
fn str_from_bytes() -> Func {
    let mut f = Func::new("rt:str_from_bytes", vec![Val::I32], vec![Val::I32; 2]);
    let (b, n, s) = (f.local(Val::I32), f.local(Val::I32), f.local(Val::I32));
    f.get(0);
    f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
    f.set(b);
    f.get(b);
    f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
    f.set(n);
    f.u32(EMPTY);
    f.set(s);
    f.get(n);
    f.op(Instruction::If(BlockType::Empty));
    f.get(n);
    f.helper(Helper::Str);
    f.set(s);
    f.get(s);
    f.get(b);
    f.u32(LIST_ELEMS);
    f.op(Instruction::I32Add);
    f.get(n);
    f.op(Instruction::MemoryCopy {
        src_mem: HEAP,
        dst_mem: HEAP,
    });
    f.op(Instruction::End);
    f.get(s);
    f.get(n);
    f
}

/// [`Helper::ConcatAll`]: one pass sums the lengths, a second copies;
/// no bytes is the static empty string.
fn concat_all() -> Func {
    let mut f = Func::new("rt:concat_all", vec![Val::I32], vec![Val::I32; 2]);
    let (at, n, total, s, to) = (
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
        f.local(Val::I32),
    );
    let start = |f: &mut Func| {
        f.get(0);
        f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
        f.set(at);
        f.get(at);
        f.op(Instruction::I32Load(mem(HEAP, LIST_LEN, 2)));
        f.set(n);
        f.get(at);
        f.u32(LIST_ELEMS);
        f.op(Instruction::I32Add);
        f.set(at);
    };
    start(&mut f);
    each_elem(&mut f, at, n, 8, &|f| {
        f.get(total);
        f.get(at);
        f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
        f.op(Instruction::I32Add);
        f.set(total);
    });
    f.u32(EMPTY);
    f.set(s);
    f.get(total);
    f.op(Instruction::If(BlockType::Empty));
    f.get(total);
    f.helper(Helper::Str);
    f.set(s);
    f.get(s);
    f.set(to);
    start(&mut f);
    each_elem(&mut f, at, n, 8, &|f| {
        f.get(to);
        f.get(at);
        f.op(Instruction::I32Load(mem(HEAP, 0, 2)));
        f.get(at);
        f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
        f.op(Instruction::MemoryCopy {
            src_mem: HEAP,
            dst_mem: HEAP,
        });
        f.get(to);
        f.get(at);
        f.op(Instruction::I32Load(mem(HEAP, 4, 2)));
        f.op(Instruction::I32Add);
        f.set(to);
    });
    f.op(Instruction::End);
    f.get(s);
    f.get(total);
    f
}
