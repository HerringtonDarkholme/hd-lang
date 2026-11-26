mod origin_tests {
    use hd_wasm::testing::op::Op as Instruction;
    use hd_wasm::testing::task::*;

    use hd_wasm::testing::module::{Ins, Module, Val};

    #[test]
    fn frame_guard_uses_scope_for_setup_and_return_for_early_exit() {
        let m = Module::default();
        let mut f = m.func("guarded", vec![Val::I32], vec![]);
        f.body = vec![Ins::Op(Instruction::Return)];
        f.origins = Some(vec![Some(10)]);
        f.origin_root = Some(99);
        let mut plain = f.clone();
        plain.origins = None;
        guard_frame(&mut plain);
        guard_frame(&mut f);
        assert_eq!(format!("{:?}", f.body), format!("{:?}", plain.body));
        let spans = f.origins.as_ref().unwrap();
        assert_eq!(spans.len(), f.body.len());
        assert_eq!(spans.first(), Some(&Some(99)));
        assert_eq!(spans.last(), Some(&Some(99)));
        assert!(
            f.body
                .iter()
                .zip(spans)
                .any(|(i, s)| matches!(i, Ins::Op(Instruction::Return)) && *s == Some(10))
        );
        assert!(spans.iter().all(|s| matches!(s, Some(10 | 99))));
    }
}

mod tests {
    #[test]
    fn synchronous_wake_helper_graph_has_no_allocation_edge() {
        use hd_wasm::testing::module::{Ins, Sym};
        let mut m = hd_wasm::testing::module::Module::default();
        m.cancellable = true;
        let mut pending = vec![
            hd_wasm::Helper::WakeMark,
            hd_wasm::Helper::HostDrive {
                blocking: true,
                module: "test".into(),
                method: "write".into(),
                args: vec![],
                area: hd_wasm::testing::rt::Elem {
                    size: 0,
                    leaves: vec![],
                    counted: hd_wasm::testing::rt::Counted::None,
                    parts: vec![],
                    cyclic: false,
                },
                counts: Box::new(hd_wasm::testing::rt::HostCounts {
                    borrowed: Vec::new(),
                    owned: Vec::new(),
                }),
                res: hd_wasm::testing::task::HostRes::Void,
            },
        ];
        #[allow(
            clippy::mutable_key_type,
            reason = "Walk keys identify immutable recipes; mutexes protect append-only nodes and memoized results only."
        )]
        let mut seen = hd_base::HashSet::default();
        while let Some(helper) = pending.pop() {
            assert!(
                !matches!(
                    helper,
                    hd_wasm::Helper::Alloc
                        | hd_wasm::Helper::New
                        | hd_wasm::Helper::WakeSlot
                        | hd_wasm::Helper::WakeRegister
                        | hd_wasm::Helper::HostCold { .. }
                ),
                "{helper:?}"
            );
            if !seen.insert(helper.clone()) {
                continue;
            }
            let f = hd_wasm::testing::rt::helper(&helper, &mut m);
            for ins in &f.body {
                if let Ins::Call(id, _) | Ins::TableSlot(id) = ins
                    && let Sym::Helper(callee) = f.symbols.get(*id)
                {
                    pending.push(callee.clone());
                }
            }
        }
        assert!(seen.contains(&hd_wasm::Helper::HostCancel));
    }

    #[test]
    fn scoped_host_wakes_preserve_nested_async_stale_cancel_and_reset_paths() {
        use hd_wasm::testing::module::Global;
        let mut m = Module::default();
        m.cancellable = true;
        m.export("cancellation", Sym::Helper(Helper::CancellationRoot));
        let area = hd_wasm::testing::rt::Elem {
            size: 0,
            leaves: vec![],
            counted: hd_wasm::testing::rt::Counted::None,
            parts: vec![],
            cyclic: false,
        };
        for blocking in [false, true] {
            for (name, helper) in [
                (
                    if blocking { "drive" } else { "asyncDrive" },
                    Helper::HostDrive {
                        blocking,
                        module: "test".into(),
                        method: "write".into(),
                        args: vec![],
                        area: area.clone(),
                        counts: Box::new(hd_wasm::testing::rt::HostCounts {
                            borrowed: area.parts.clone(),
                            owned: Vec::new(),
                        }),
                        res: hd_wasm::testing::task::HostRes::Void,
                    },
                ),
                (
                    if blocking { "poll" } else { "asyncPoll" },
                    Helper::HostPoll {
                        blocking,
                        module: "test".into(),
                        method: "write".into(),
                        args: vec![],
                        area: area.clone(),
                        res: hd_wasm::testing::task::HostRes::Void,
                    },
                ),
            ] {
                m.export(name, Sym::Helper(helper));
            }
        }
        for (name, helper) in [
            ("wake", Helper::WakeMark),
            ("cancel", Helper::HostCancel),
            ("reset", Helper::EntryReset),
        ] {
            m.export(name, Sym::Helper(helper));
        }
        for (number, (name, global)) in [
            ("head", Global::BlockingWakes),
            ("capacity", Global::WakeCap),
        ]
        .into_iter()
        .enumerate()
        {
            let mut f = m.func(name, vec![], vec![Val::I32]);
            f.global_get(global);
            let key = Sym::Inst(hd_base::Hash128(90_100 + number as u128));
            m.define(key.clone(), f);
            m.export(name, key);
        }
        let mut f = m.func("peek", vec![Val::I32; 2], vec![Val::I32]);
        f.get(0);
        f.get(1);
        f.op(I::I32Add);
        f.op(I::I32Load(mem(HEAP, 0, 2)));
        let key = Sym::Inst(hd_base::Hash128(90_102));
        m.define(key.clone(), f);
        m.export("peek", key);
        let mut f = m.func("abandon", vec![Val::I32], vec![]);
        f.get(0);
        f.global_set(Global::Aband);
        let key = Sym::Inst(hd_base::Hash128(90_103));
        m.define(key.clone(), f);
        m.export("abandon", key);
        let wasm = m.finish().expect("scoped host wake module");
        let script = r"
const fs = require('node:fs'), assert = require('node:assert/strict');
const module_ = new WebAssembly.Module(fs.readFileSync(0)), imports = {};
let next = 0, fail = false;
const finished = [], aborted = [];
for (const i of WebAssembly.Module.imports(module_))
  (imports[i.module] ??= {})[i.name] = () => { throw Error(i.module + ':' + i.name); };
imports.test['write.start'] = () => [1, next];
imports.test['write.finish'] = h => { if (fail) throw Error('failed finish'); finished.push(h); return 0; };
imports['hd:rt'].abort = h => aborted.push(h);
const w = new WebAssembly.Instance(module_, imports).exports;
const mark = (...handles) => { new Uint32Array(w['hd.x'].buffer).set(handles); w.wake(handles.length); };
const drive = (h, frame) => { next = h; assert.equal(w.drive(0, frame), frame); };
const a = 16384, b = 32768, h1 = 0x100003, h2 = 0x100005, h3 = 0x100007;
mark(0x7fffffff);
assert.equal(w.capacity(), 0, 'unknown wakes never grow the indexed table');
drive(h1, a); drive(h2, b);
assert.equal(w.head(), b);
next = h3; const async = w.asyncDrive(0), capacity = w.capacity();
assert.ok(capacity > 0, 'escaping async uses the indexed registration');
mark(h1, h2 + 0x100000, h3, 0x7fffffff);
assert.equal(w.capacity(), capacity);
assert.equal(w.poll(a), 1, 'an outer completion survives a nested registration');
assert.equal(w.head(), b, 'removing an outer node retains the inner one');
assert.equal(w.poll(b), 0, 'the stale generation did not complete the inner call');
assert.equal(w.asyncPoll(async), 1, 'the same batch also woke an async registration');
drive(h1 + 0x100000, a);
mark(h1);
assert.equal(w.poll(a), 0, 'reuse of caller storage preserves generation checks');
mark(h1 + 0x100000); assert.equal(w.poll(a), 1); assert.equal(w.head(), b);
w.abandon(1); w.cancel(b); w.abandon(0);
assert.equal(w.head(), 0); assert.equal(w.peek(b, 12) & 40, 40, 'abandoned cleanup status survives');
w.cancel(b); assert.deepEqual(aborted, [h2], 'cancel and unlink occur once');
mark(h2); assert.equal(w.head(), 0, 'late wake after cancel cannot retain a frame');
drive(h3 + 0x100000, a); w.cancel(a);
assert.equal(w.head(), 0); assert.equal(w.peek(a, 12) & 32, 0, 'ordinary cancellation is not abandonment');
assert.deepEqual(aborted, [h2, h3 + 0x100000]);
next = h3 + 0x200000; const stopped = w.asyncDrive(0); w.cancel(stopped);
assert.deepEqual(aborted, [h2, h3 + 0x100000, h3 + 0x200000], 'indexed cancellation still aborts its registration');
mark(h3 + 0x200000); assert.equal(w.asyncPoll(stopped), 0);
drive(h2 + 0x100000, b); mark(h2 + 0x100000); fail = true;
assert.throws(() => w.poll(b), /failed finish/);
assert.equal(w.head(), 0, 'unlink precedes the host finish call'); fail = false;
drive(h1 + 0x200000, a);
w.reset(); assert.equal(w.head(), 0, 'session reset discards scopes skipped by a trap');
drive(h2 + 0x200000, b); mark(h1 + 0x200000);
assert.equal(w.poll(b), 0, 'old input wake cannot touch the next scoped call');
mark(h2 + 0x200000); assert.equal(w.poll(b), 1); assert.equal(w.head(), 0);
assert.deepEqual(finished, [h1, h3, h1 + 0x100000, h2 + 0x200000]);
";
        let mut child = hd_testkit::node::command()
            .args(["-e", script])
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("node");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(&wasm)
            .expect("write wasm");
        let result = child.wait_with_output().expect("node completed");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    fn blocking_host_drive_has_no_heap_frame_edge_and_uses_reserved_leaf_word() {
        use hd_wasm::testing::layout::Leaf;
        use hd_wasm::testing::module::{HEAP, Ins};
        let m = hd_wasm::testing::module::Module::default();
        let area = hd_wasm::testing::rt::Elem {
            size: 8,
            leaves: [0, 4]
                .into_iter()
                .map(|offset| Leaf {
                    offset,
                    val: hd_wasm::testing::module::Val::I32,
                    bytes: 4,
                    signed: false,
                })
                .collect(),
            counted: hd_wasm::testing::rt::Counted::Address {
                index: 0,
                known: false,
            },
            parts: vec![(
                0,
                hd_wasm::testing::rt::Counted::Address {
                    index: 0,
                    known: false,
                },
            )],
            cyclic: false,
        };
        for blocking in [false, true] {
            let f = hd_wasm::testing::task::host_drive(
                &m,
                blocking,
                &hd_wasm::testing::task::HostLeaf {
                    module: "test:writer",
                    method: "write",
                    args: &[hd_wasm::testing::task::HostArg::Str],
                    area: &area,
                    counts: &hd_wasm::testing::rt::HostCounts {
                        borrowed: area.parts.clone(),
                        owned: Vec::new(),
                    },
                    res: &hd_wasm::testing::task::HostRes::Void,
                },
            );
            let heap = f.body.iter().any(|ins| {
                matches!(ins, Ins::Call(id, _)
                if matches!(f.symbols.get(*id), hd_wasm::testing::module::Sym::Helper(
                    hd_wasm::testing::rt::Helper::HostCold { .. } | hd_wasm::testing::rt::Helper::New)))
            });
            assert_eq!(heap, !blocking);
            assert_eq!(f.params.len(), if blocking { 4 } else { 3 });
            let drop_stores: Vec<_> = f
                .body
                .windows(2)
                .filter(|pair| {
                    matches!(pair[1],
                    Ins::Op(hd_wasm::testing::op::Op::I32Store(arg))
                        if arg.memory_index == HEAP && arg.offset == hd_wasm::testing::task::BORROWED_DROP)
                })
                .collect();
            assert_eq!(drop_stores.len(), usize::from(blocking));
            if blocking {
                assert!(matches!(
                    drop_stores[0][0],
                    Ins::Op(hd_wasm::testing::op::Op::I32Const(-1))
                ));
                assert!(!f.body.iter().any(|ins| matches!(ins,
                    Ins::Call(id, _) if matches!(f.symbols.get(*id), hd_wasm::testing::module::Sym::Helper(
                        hd_wasm::testing::rt::Helper::RetainAny | hd_wasm::testing::rt::Helper::CopyElems { .. }))
                )));
            }
        }
        let cold = hd_wasm::testing::task::host_cold(
            &m,
            &hd_wasm::testing::task::HostLeaf {
                module: "test:writer",
                method: "write",
                args: &[hd_wasm::testing::task::HostArg::Str],
                area: &area,
                counts: &hd_wasm::testing::rt::HostCounts {
                    borrowed: area.parts.clone(),
                    owned: Vec::new(),
                },
                res: &hd_wasm::testing::task::HostRes::Void,
            },
        );
        assert!(!cold.body.iter().any(|ins| matches!(ins,
            Ins::Op(hd_wasm::testing::op::Op::I32Store(arg)) if arg.offset == hd_wasm::testing::task::BORROWED_DROP)));
    }

    #[test]
    fn pending_frames_link_cancel_slots_only_when_needed() {
        let mut m = hd_wasm::testing::module::Module::default();
        for enabled in [false, true] {
            m.cancellable = enabled;
            let f = hd_wasm::testing::task::pending_cold(&m, &[]);
            let linked = f.body.iter().any(|ins| {
                matches!(ins, hd_wasm::testing::module::Ins::TableSlot(id)
                    if matches!(f.symbols.get(*id), hd_wasm::testing::module::Sym::Helper(
                        hd_wasm::testing::rt::Helper::CleanupCancel | hd_wasm::testing::rt::Helper::PendingCancel)))
            });
            assert_eq!(linked, enabled, "{}", f.name);
        }
    }

    use std::io::Write as _;
    use std::process::Stdio;

    use hd_wasm::Helper;
    use hd_wasm::testing::module::{HEAP, Module, Sym, Val, mem};
    use hd_wasm::testing::op::Op as I;
    use hd_wasm::testing::task::{W_READY, W_SLOT};

    #[test]
    fn flat_all_drains_woken_children_in_order_without_rescanning() {
        use hd_wasm::testing::layout::Leaf;
        use hd_wasm::testing::module::{Global, Import};
        use hd_wasm::testing::rt::Elem;

        let mut m = Module::default();
        let word = Leaf {
            offset: 0,
            val: Val::I32,
            bytes: 4,
            signed: false,
        };
        let elem = Elem {
            size: 4,
            leaves: vec![word],
            counted: hd_wasm::testing::rt::Counted::None,
            parts: vec![],
            cyclic: false,
        };
        for (name, part) in [("all", 0), ("poll", 1), ("cancel", 2)] {
            m.export(
                name,
                Sym::Helper(Helper::All {
                    part,
                    elem: elem.clone(),
                }),
            );
        }
        m.export("wake", Sym::Helper(Helper::WakeSignal));
        let poll = Sym::Inst(hd_base::Hash128(48_901));
        let cancel = Sym::Inst(hd_base::Hash128(48_902));
        let mut f = m.func("test:child_poll", vec![Val::I32], vec![Val::I32; 2]);
        let (ready, value) = (f.local(Val::I32), f.local(Val::I32));
        hd_wasm::testing::task::all_field(&mut f, 0, u64::from(hd_wasm::testing::task::F_SAVED));
        f.global_get(Global::Waker);
        f.global_get(Global::WakeBit);
        f.call(Sym::Import(Import::new(
            "test",
            "poll",
            &[Val::I32; 3],
            &[Val::I32; 2],
        )));
        f.set(value);
        f.set(ready);
        f.get(ready);
        f.op(I::If(wasm_encoder::BlockType::Empty));
        hd_wasm::testing::task::flags_or(&mut f, 0, hd_wasm::testing::task::DONE);
        f.op(I::End);
        f.get(ready);
        f.get(value);
        m.define(poll.clone(), f);
        let mut f = m.func("test:child_cancel", vec![Val::I32], vec![]);
        hd_wasm::testing::task::all_field(&mut f, 0, u64::from(hd_wasm::testing::task::F_SAVED));
        f.call(Sym::Import(Import::new("test", "cancel", &[Val::I32], &[])));
        hd_wasm::testing::task::flags_or(&mut f, 0, hd_wasm::testing::task::CANCELLED);
        m.define(cancel.clone(), f);
        let mut f = m.func("test:tasks", vec![Val::I32], vec![Val::I32]);
        let buffer = f.local(Val::I32);
        f.get(0);
        f.helper(Helper::ListBuf {
            elem: Elem {
                cyclic: true,
                ..elem.clone()
            },
        });
        f.set(buffer);
        f.get(buffer);
        f.get(0);
        f.op(I::I32Store(mem(HEAP, hd_wasm::testing::rt::LIST_LEN, 2)));
        hd_wasm::testing::task::all_for(&mut f, 0, |f, index| {
            let child = hd_wasm::testing::task::new_zeroed(
                f,
                hd_wasm::testing::task::F_SAVED + 4,
                None,
                false,
            );
            for (offset, target) in [
                (hd_wasm::testing::task::F_POLL, poll.clone()),
                (hd_wasm::testing::task::F_CANCEL, cancel.clone()),
            ] {
                f.get(child);
                f.table_slot(target);
                f.op(I::I32Store(mem(HEAP, offset, 2)));
            }
            f.get(child);
            f.get(index);
            f.op(I::I32Store(mem(
                HEAP,
                u64::from(hd_wasm::testing::task::F_SAVED),
                2,
            )));
            f.get(buffer);
            f.get(index);
            f.i32(4);
            f.op(I::I32Mul);
            f.op(I::I32Add);
            f.get(child);
            f.op(I::I32Store(mem(
                HEAP,
                u64::from(hd_wasm::testing::rt::LIST_ELEMS),
                2,
            )));
        });
        f.get(buffer);
        let tasks = Sym::Inst(hd_base::Hash128(48_903));
        m.define(tasks.clone(), f);
        m.export("tasks", tasks);
        let mut f = m.func("test:peek", vec![Val::I32; 2], vec![Val::I32]);
        f.get(0);
        f.get(1);
        f.i32(4);
        f.op(I::I32Mul);
        f.op(I::I32Add);
        f.op(I::I32Load(mem(
            HEAP,
            u64::from(hd_wasm::testing::rt::LIST_ELEMS),
            2,
        )));
        let peek = Sym::Inst(hd_base::Hash128(48_904));
        m.define(peek.clone(), f);
        m.export("peek", peek);
        let wasm = m.finish().expect("flat all module");
        let script = r"
const fs = require('node:fs'), assert = require('node:assert/strict');
const module_ = new WebAssembly.Module(fs.readFileSync(0));
for (const n of [0, 1, 31, 32, 961, 962, 2000]) {
  const calls = [], regs = new Map(), ready = new Set(), done = new Set(), cancelled = [];
  const imports = {test: {
    poll: (id, node, bit) => {
      assert(!done.has(id), 'completed child polled again');
      calls.push(id); regs.set(id, [node, bit]);
      if (!ready.delete(id)) return [0, 0];
      done.add(id); return [1, id * 3];
    }, cancel: id => { cancelled.push(id); },
  }};
  for (const im of WebAssembly.Module.imports(module_)) {
    (imports[im.module] ??= {})[im.name] ??= () => { throw Error(im.module + ':' + im.name); };
  }
  const w = new WebAssembly.Instance(module_, imports).exports;
  const frame = w.all(w.tasks(n));
  let [complete, values] = w.poll(frame);
  assert.deepEqual(calls, Array.from({length: n}, (_, i) => i));
  if (!n) { assert.equal(complete, 1); continue; }
  assert.equal(complete, 0);
  calls.length = 0; w.poll(frame); assert.deepEqual(calls, [], 'no wake means no child poll');
  const batch = [...new Set([n - 1, 0, Math.floor(n / 2), 30, 31, 961].filter(i => i < n))];
  for (const id of batch) { ready.add(id); w.wake(...regs.get(id)); w.wake(...regs.get(id)); }
  [complete, values] = w.poll(frame);
  assert.deepEqual(calls, [...batch].sort((a, b) => a - b), 'wakes coalesce and poll in input order');
  for (let id = 0; id < n; id++) {
    if (done.has(id)) continue;
    calls.length = 0; ready.add(id); w.wake(...regs.get(id));
    [complete, values] = w.poll(frame);
    assert.deepEqual(calls, [id], 'one wake visits exactly one child');
  }
  assert.equal(complete, 1);
  for (let id = 0; id < n; id++) assert.equal(w.peek(values, id), id * 3);
  assert.equal(Number(w['hd.all_polls'].value), n * 2);
  w.cancel(frame); assert.deepEqual(cancelled, [], 'completed children are not cancelled');
  const other = w.all(w.tasks(n)); calls.length = 0; regs.clear(); done.clear();
  w.poll(other); w.cancel(other);
  assert.deepEqual(cancelled, Array.from({length: n}, (_, i) => i));
  calls.length = 0; w.poll(other); assert.deepEqual(calls, [], 'cancelled all never polls children');
}
";
        let mut child = hd_testkit::node::command()
            .args(["-e", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("node");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(&wasm)
            .expect("module");
        let result = child.wait_with_output().expect("flat all result");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    fn weak_waiters_ignore_reused_and_exhausted_generations() {
        let mut m = Module::default();
        for (name, helper) in [
            ("newNode", Helper::WakerNew),
            ("dropNode", Helper::WakerDrop),
            ("live", Helper::WakerLive),
            ("link", Helper::WakeLink),
        ] {
            m.export(name, Sym::Helper(helper));
        }
        for (index, (name, offset)) in [("ready", W_READY), ("slot", W_SLOT)]
            .into_iter()
            .enumerate()
        {
            let mut f = m.func(name, vec![Val::I32], vec![Val::I32]);
            f.get(0);
            f.op(I::I32Load(mem(HEAP, offset, 2)));
            let key = Sym::Inst(hd_base::Hash128(index as u128 + 1));
            m.define(key.clone(), f);
            m.export(name, key);
        }
        let mut f = m.func("notify", vec![Val::I32], vec![]);
        f.get(0);
        f.op(I::I32Load(mem(HEAP, 0, 2)));
        f.helper(Helper::WakeNotify);
        let key = Sym::Inst(hd_base::Hash128(3));
        m.define(key.clone(), f);
        m.export("notify", key);
        let mut f = m.func("generation", vec![Val::I32; 2], vec![]);
        f.get(0);
        f.helper(Helper::WakerSlot);
        f.get(1);
        f.op(I::I32Store(mem(HEAP, 4, 2)));
        let key = Sym::Inst(hd_base::Hash128(4));
        m.define(key.clone(), f);
        m.export("generation", key);
        let mut f = m.func("peek", vec![Val::I32; 2], vec![Val::I32]);
        f.get(0);
        f.get(1);
        f.op(I::I32Add);
        f.op(I::I32Load(mem(HEAP, 0, 2)));
        let key = Sym::Inst(hd_base::Hash128(5));
        m.define(key.clone(), f);
        m.export("peek", key);
        let wasm = m.finish().expect("weak-waker module");
        let script = r"
const fs = require('node:fs');
const assert = require('node:assert/strict');
const module_ = new WebAssembly.Module(fs.readFileSync(0));
const imports = {};
for (const i of WebAssembly.Module.imports(module_)) {
  (imports[i.module] ??= {})[i.name] = () => { throw Error(i.module + ':' + i.name); };
}
const w = new WebAssembly.Instance(module_, imports).exports;
const parent = w.newNode(0), child = w.newNode(0);
const old = w.slot(parent);
w.link(child, parent, 1);
w.dropNode(parent);
const replacement = w.newNode(0);
assert.equal(w.slot(replacement), old, 'reuse the weak slot');
assert.equal(w.live(old, 1), 0, 'old generation is dead');
assert.equal(w.live(old, 2), replacement, 'new generation resolves');
w.notify(child);
assert.equal(w.ready(replacement), 0, 'a stale edge cannot wake the new occupant');
w.link(child, replacement, 2);
const other = w.newNode(0);
w.link(child, other, 4);
w.link(child, replacement, 8);
w.notify(child);
assert.equal(w.ready(replacement), 10, 'merge only one target generation');
assert.equal(w.ready(other), 4, 'keep distinct live awaiters');
w.dropNode(other);
w.link(child, replacement, 32);
let copies = 0;
for (let edge = w.peek(child, 0); edge; edge = w.peek(edge, 8)) {
  if (w.peek(edge, 0) === old && w.peek(edge, 4) === 2) copies++;
}
assert.equal(copies, 1, 'a stale head cannot hide an existing live target');
const otherAgain = w.newNode(0);
w.link(child, otherAgain, 64);
w.notify(child);
assert.equal(w.ready(otherAgain), 64, 'reuse stale storage after searching all live targets');
w.generation(old, -1);
w.link(child, replacement, 16);
assert.equal(w.live(old, -1), replacement);
w.dropNode(replacement);
const fresh = w.newNode(0);
assert.notEqual(w.slot(fresh), old, 'exhausted generation retires the slot');
assert.equal(w.live(old, -1), 0);
w.notify(child);
assert.equal(w.ready(fresh), 0, 'retired-generation edges remain dead');
let last;
for (let i = 0; i < 96; i++) last = w.newNode(0);
assert.equal(w.live(w.slot(child), 1), child, 'growth preserves old live rows');
w.link(child, last, 32);
w.notify(child);
assert.equal(w.ready(last), 32, 'weak lookup survives registry growth');
";
        let mut child = hd_testkit::node::command()
            .args(["-e", script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("node");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(&wasm)
            .expect("wasm input");
        let out = child.wait_with_output().expect("weak-waker test");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

mod native_leaf_drop_tests {
    use hd_wasm::testing::task::*;

    use hd_wasm::testing::layout::Leaf;
    use hd_wasm::testing::module::{Sym, Val};
    use hd_wasm::testing::rt::Elem;
    use hd_wasm::testing::rt::Helper;

    /// A leaf frame's glue releases each argument through its type's
    /// count, its leaves past the frame's header fields.
    #[test]
    fn a_leaf_frame_releases_its_arguments_by_parts() -> Result<(), String> {
        let part = hd_wasm::testing::rt::Counted::Address {
            index: 0,
            known: false,
        };
        let area = Elem {
            size: 4,
            leaves: vec![Leaf {
                offset: 0,
                val: Val::I32,
                bytes: 4,
                signed: false,
            }],
            counted: hd_wasm::testing::rt::Counted::None,
            parts: vec![(0, part.clone())],
            cyclic: false,
        };
        let Some(Sym::Helper(Helper::DropParts { leaves, parts })) = leaf_drop(&area) else {
            return Err("a leaf with a counted argument must have release glue".into());
        };
        assert_eq!(leaves[0].offset, LEAF_ARGS);
        assert_eq!(parts, vec![(0, part)]);
        Ok(())
    }
}
