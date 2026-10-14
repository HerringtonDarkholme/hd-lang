//! `value_flow` (opt-pipeline.md §2.1): the body-unit analysis behind
//! the known-value decisions. It reads only the body's own TIR, so it runs
//! once per body, never per instance.

use hd_tir::ir::{Body, Tag};

/// What `devirt_known` and `closure_spec` read of a body's data flow
/// (options A1 and A2 of closure-calls.md).
pub struct ValueFlow {
    /// Per local: how many `LocalSet`s write it, and the last one's value.
    sets: Vec<(u32, u32)>,
    /// Each instruction's sub-body (`Body::inst_subs`).
    subs: Vec<u32>,
    /// The item's own parameter locals, in parameter order: those of sub 0,
    /// as `hd_wasm`'s `signature` lists them.
    params: Vec<u32>,
    /// Per sub-body, the sub holding its defining closure (0 for sub 0).
    parent: Vec<u32>,
    /// Per sub-body, whether it is a generic local function's body.
    generic: Vec<bool>,
    /// Each call of an item parameter (`ValueFlow::param`): the parameter's
    /// index and the sub-body the call is in.
    param_calls: Vec<(u32, u32)>,
}

impl ValueFlow {
    #[must_use]
    pub fn new(body: &Body) -> Self {
        use hd_tir::ir::local_flags::PARAM;
        let mut sets: Vec<(u32, u32)> = vec![(0, u32::MAX); body.local_flags.len()];
        for (i, tag) in body.tags.iter().enumerate() {
            if *tag == Tag::LocalSet
                && let Some(s) = sets.get_mut(body.data[i][0] as usize)
            {
                *s = (s.0 + 1, body.data[i][1]);
            }
        }
        let mut in_sub = vec![false; body.local_flags.len()];
        for r in body.sub_params.iter().skip(1) {
            for &l in body.record(r.start) {
                if let Some(x) = in_sub.get_mut(l as usize) {
                    *x = true;
                }
            }
        }
        let params = (0..body.local_flags.len())
            .filter(|&l| body.local_flags[l] & PARAM != 0 && !in_sub[l])
            .map(|l| u32::try_from(l).expect("locals"))
            .collect();
        let subs = body.inst_subs();
        // A sub's parent is the sub holding its defining closure, opened
        // before it, so it has the smaller number (as in `hidden_insts`).
        let n = body.sub_root.len();
        let mut parent = vec![0u32; n];
        for (i, &s) in subs.iter().enumerate() {
            if body.tags[i] == Tag::Closure && !body.is_instantiation(i) {
                let k = body.data[i][0] as usize;
                if k < n && s < body.data[i][0] {
                    parent[k] = s;
                }
            }
        }
        let mut flow = Self {
            sets,
            subs,
            params,
            parent,
            generic: (0..n).map(|k| body.sub_is_generic(k)).collect(),
            param_calls: Vec::new(),
        };
        for i in 0..body.len() {
            if body.tags[i] == Tag::CallValue
                && let Some(p) = flow.param(body, body.data[i][0])
            {
                flow.param_calls.push((p, flow.subs[i]));
            }
        }
        flow
    }

    /// Whether item parameter `p` is called in sub-body `k` or in a sub
    /// nested in it, with no generic local function's body on the way: the
    /// calls an instance of `k` specialized on `p` makes direct, itself or
    /// through the sub-bodies it creates (closure-calls.md §3.1).
    #[must_use]
    pub fn called_under(&self, p: u32, k: u32) -> bool {
        self.param_calls.iter().any(|&(q, s)| {
            if q != p {
                return false;
            }
            let mut c = s as usize;
            loop {
                if self.generic.get(c).copied().unwrap_or(true) {
                    return false;
                }
                if c == k as usize {
                    return true;
                }
                let up = self.parent[c] as usize;
                if c == 0 || up >= c {
                    return false;
                }
                c = up;
            }
        })
    }

    /// The `Closure` or `ItemRef` instruction whose value the operand
    /// `word` of instruction `at` is, made in `at`'s sub-body: the operand
    /// itself, or a local whose only write is one `LocalSet` of it, not a
    /// parameter and never written through `mut` or from a closure. It has
    /// the operand's type. A closure made in an enclosing sub-body does not
    /// count: its key there may be under other type arguments than this
    /// instance's.
    #[must_use]
    pub fn made(&self, body: &Body, word: u32, at: usize) -> Option<usize> {
        use hd_tir::ir::local_flags::{CAPTURED_ASSIGNED, MUTATED, PARAM};
        let inst = |word: u32| hd_tir::ir::Ref(word).as_inst().map(|x| x.0 as usize);
        let operand = inst(word)?;
        let mut made = operand;
        if body.tags[made] == Tag::LocalGet {
            let l = body.data[made][0] as usize;
            let flags = body.local_flags.get(l).copied().unwrap_or(PARAM);
            let &(1, value) = self.sets.get(l)? else {
                return None;
            };
            if flags & (PARAM | MUTATED | CAPTURED_ASSIGNED) != 0 {
                return None;
            }
            made = inst(value)?;
        }
        (matches!(body.tags[made], Tag::Closure | Tag::ItemRef)
            && self.subs[made] == self.subs[at]
            && body.ty[made] == body.ty[operand])
            .then_some(made)
    }

    /// The index of the item parameter that operand `word` reads, when it
    /// is a `LocalGet` of a parameter nothing writes: in sub-body 0, or in a
    /// sub-body that captures it.
    #[must_use]
    pub fn param(&self, body: &Body, word: u32) -> Option<u32> {
        use hd_tir::ir::local_flags::{ASSIGNED, CAPTURED_ASSIGNED, MUTATED};
        let g = hd_tir::ir::Ref(word).as_inst()?.0 as usize;
        if body.tags[g] != Tag::LocalGet {
            return None;
        }
        let l = body.data[g][0];
        let p = self.params.iter().position(|&x| x == l)?;
        let flags = body.local_flags[l as usize];
        let written = self.sets.get(l as usize).is_some_and(|s| s.0 != 0);
        (flags & (ASSIGNED | MUTATED | CAPTURED_ASSIGNED) == 0 && !written)
            .then(|| u32::try_from(p).expect("params"))
    }
}
