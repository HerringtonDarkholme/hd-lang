//! Instance keys, A1 classes and Wasm layouts over the full type
//! representation (codegen.md §13.2, §13.3; wasm-layout.md §15.1, §15.2;
//! data-structures.md §3.22).

use hd_base::{DefId, Hash128, InstId, NotImplemented, StableHasher, Stage, StageResult};
use std::collections::{HashMap, HashSet};

use hd_types::{InternPool, Prim, Ty, TyData, TyList};

/// The A1 class of a move-only type argument (§13.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum A1Class {
    /// The parameter needs its exact representation.
    Exact,
    /// One non-null reference.
    Ref,
    /// One nullable reference.
    RefNull,
    /// A scalar class, by its Wasm value type.
    Scalar(ValType),
    Void,
}

/// Wasm value types the layouts use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValType {
    I32,
    I64,
    F32,
    F64,
    /// `(ref $T)` or `(ref null $T)`.
    Ref {
        nullable: bool,
    },
    /// `eqref`: erased storage.
    EqRef,
}

/// Layout classes (§15.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayoutClass {
    I32,
    I64,
    F32,
    F64,
    Ref,
    Multi,
    Erased,
    Void,
}

/// A layout: its class and its Wasm values in locals and results.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub class: LayoutClass,
    pub values: Vec<ValType>,
    /// Packed storage in a field or array element (`i8` for `bool`, ...), in bits.
    pub packed_bits: Option<u8>,
}

/// The value bound (§15.1): Wasm values after slot sharing; default 4.
pub const VALUE_BOUND: usize = 4;

fn one(class: LayoutClass, v: ValType) -> Layout {
    Layout {
        class,
        values: vec![v],
        packed_bits: None,
    }
}

/// The std types the backend gives a built-in shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StdKind {
    List,
    Map,
    Suspend,
    Other,
}

/// What layouts need to know about declared types (enum shapes).
pub trait LayoutEnv {
    /// Variant payload types of an enum, or `None` for a data type.
    fn enum_variants(&self, def: DefId, args: TyList) -> Option<Vec<Vec<Ty>>>;
    /// An enum's variant payload types as declared, over its own
    /// parameters; `None` for any other item.
    fn enum_declared(&self, def: DefId) -> Option<Vec<Vec<Ty>>>;
    /// Whether an enum is self-recursive (`recursive_enums`): every
    /// instance of it is boxed.
    fn recursive_enum(&self, def: DefId) -> bool;
    /// Which built-in std type `def` is, if any. Required: a default of
    /// `Other` would lay out `List`, `Map` and `Suspend` as plain data.
    fn std_kind(&self, def: DefId) -> StdKind;
}

/// The layout of a type (§15.1 and the value table of §15.2). Every type
/// form has a row; forms that need declared shapes ask `env`. A
/// self-recursive enum is one boxed reference (`recursive_enums`), so the
/// walk through value positions is finite. The enums `t` holds by value
/// are laid out first, innermost first (`value_enums`), so the recursion
/// is only as deep as tuples and optionals nest.
pub fn layout_of(pool: &InternPool, env: &dyn LayoutEnv, t: Ty) -> StageResult<Layout> {
    let mut memo: HashMap<Ty, Layout> = HashMap::new();
    for e in value_enums(pool, env, t) {
        let l = layout_in(pool, env, e, &memo)?;
        memo.insert(e, l);
    }
    layout_in(pool, env, t, &memo)
}

/// The enum instances `t` holds by value, `t` included, each after the
/// enums it holds: enum payloads, tuple elements and `T?`. A self-recursive
/// enum is a reference and ends the walk; it is not listed.
#[must_use]
pub fn value_enums(pool: &InternPool, env: &dyn LayoutEnv, t: Ty) -> Vec<Ty> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![(t, false)];
    while let Some((t, finished)) = stack.pop() {
        if finished {
            out.push(t);
            continue;
        }
        match pool.get(t) {
            TyData::Adt { def, args } => {
                if let Some(vs) = env.enum_variants(def, args)
                    && !env.recursive_enum(def)
                    && seen.insert(t)
                {
                    stack.push((t, true));
                    stack.extend(vs.into_iter().flatten().map(|f| (f, false)));
                }
            }
            TyData::Tuple { elems, rest } => {
                stack.extend(pool.list_items(elems).iter().map(|e| (*e, false)));
                stack.extend(rest.map(|r| (r, false)));
            }
            TyData::Option(i) | TyData::Mut(i) => stack.push((i, false)),
            _ => {}
        }
    }
    out
}

/// `layout_of`, with the enums `memo` holds already laid out.
fn layout_in(
    pool: &InternPool,
    env: &dyn LayoutEnv,
    t: Ty,
    memo: &HashMap<Ty, Layout>,
) -> StageResult<Layout> {
    if let Some(l) = memo.get(&t) {
        return Ok(l.clone());
    }
    let l = match pool.get(t) {
        TyData::Prim(p) => match p {
            Prim::Bool | Prim::I8 | Prim::U8 => Layout {
                packed_bits: Some(8),
                ..one(LayoutClass::I32, ValType::I32)
            },
            Prim::I16 | Prim::U16 => Layout {
                packed_bits: Some(16),
                ..one(LayoutClass::I32, ValType::I32)
            },
            Prim::I32 | Prim::U32 | Prim::Usize | Prim::Char => one(LayoutClass::I32, ValType::I32),
            Prim::I64 | Prim::U64 => one(LayoutClass::I64, ValType::I64),
            Prim::F32 => one(LayoutClass::F32, ValType::F32),
            Prim::F64 => one(LayoutClass::F64, ValType::F64),
            // A view: (ref $bytes, i64 span).
            Prim::String => Layout {
                class: LayoutClass::Multi,
                values: vec![ValType::Ref { nullable: false }, ValType::I64],
                packed_bits: None,
            },
            Prim::Void => Layout {
                class: LayoutClass::Void,
                values: vec![],
                packed_bits: None,
            },
        },
        TyData::Never => Layout {
            class: LayoutClass::Void,
            values: vec![],
            packed_bits: None,
        },
        TyData::Adt { def, args } => match env.enum_variants(def, args) {
            None => one(LayoutClass::Ref, ValType::Ref { nullable: false }),
            Some(vs) if vs.iter().all(Vec::is_empty) => one(LayoutClass::I32, ValType::I32),
            Some(_) if env.recursive_enum(def) => {
                one(LayoutClass::Ref, ValType::Ref { nullable: false })
            }
            Some(vs) => {
                // Slot sharing: a tag, then per value type the max count over variants.
                let mut slots: Vec<(ValType, usize)> = Vec::new();
                for v in &vs {
                    let mut here: Vec<(ValType, usize)> = Vec::new();
                    for f in v {
                        for val in layout_in(pool, env, *f, memo)?.values {
                            let val = if matches!(val, ValType::Ref { .. }) {
                                ValType::EqRef
                            } else {
                                val
                            };
                            match here.iter_mut().find(|s| s.0 == val) {
                                Some(s) => s.1 += 1,
                                None => here.push((val, 1)),
                            }
                        }
                    }
                    for (val, n) in here {
                        match slots.iter_mut().find(|s| s.0 == val) {
                            Some(s) => s.1 = s.1.max(n),
                            None => slots.push((val, n)),
                        }
                    }
                }
                let mut values = vec![ValType::I32];
                for (val, n) in slots {
                    values.extend(std::iter::repeat_n(val, n));
                }
                if values.len() > VALUE_BOUND {
                    one(LayoutClass::Ref, ValType::Ref { nullable: false })
                } else {
                    Layout {
                        class: LayoutClass::Multi,
                        values,
                        packed_bits: None,
                    }
                }
            }
        },
        TyData::Tuple { elems, rest: None } => {
            let mut values = Vec::new();
            for e in pool.list_items(elems).iter().copied() {
                values.extend(layout_in(pool, env, e, memo)?.values);
            }
            match values.len() {
                0 => Layout {
                    class: LayoutClass::Void,
                    values,
                    packed_bits: None,
                },
                n if n <= VALUE_BOUND => Layout {
                    class: LayoutClass::Multi,
                    values,
                    packed_bits: None,
                },
                _ => one(LayoutClass::Ref, ValType::Ref { nullable: false }),
            }
        }
        TyData::Tuple { rest: Some(_), .. } => {
            return Err(NotImplemented::new(
                Stage::Emit,
                "layout of an open tuple (rest element)",
            ));
        }
        TyData::Option(inner) => {
            let li = layout_in(pool, env, inner, memo)?;
            match (li.class, li.values.as_slice()) {
                (LayoutClass::Ref, [ValType::Ref { .. }]) => {
                    one(LayoutClass::Ref, ValType::Ref { nullable: true })
                }
                (LayoutClass::Multi, [ValType::Ref { .. }, ValType::I64])
                    if inner == Ty::STRING =>
                {
                    Layout {
                        class: LayoutClass::Multi,
                        values: vec![ValType::Ref { nullable: true }, ValType::I64],
                        packed_bits: None,
                    }
                }
                _ => {
                    let mut values = vec![ValType::I32];
                    values.extend(li.values);
                    if values.len() > VALUE_BOUND {
                        one(LayoutClass::Ref, ValType::Ref { nullable: true })
                    } else {
                        Layout {
                            class: LayoutClass::Multi,
                            values,
                            packed_bits: None,
                        }
                    }
                }
            }
        }
        TyData::Fn { .. } => one(LayoutClass::Ref, ValType::Ref { nullable: false }),
        // (eqref, (ref $VT)).
        TyData::TraitValue { .. } => Layout {
            class: LayoutClass::Multi,
            values: vec![ValType::EqRef, ValType::Ref { nullable: false }],
            packed_bits: None,
        },
        TyData::Mut(inner) => layout_in(pool, env, inner, memo)?,
        TyData::Row(_) => {
            return Err(NotImplemented::new(
                Stage::Emit,
                "the layout of a requirement row",
            ));
        }
        TyData::Param(_)
        | TyData::Assoc { .. }
        | TyData::Infer(_)
        | TyData::Canon(_)
        | TyData::Poison => {
            return Err(NotImplemented::new(
                Stage::Emit,
                "layout of a non-concrete type (substitution missing)",
            ));
        }
    };
    Ok(l)
}

/// The self-recursive enums among `enums`, the program's enum declarations
/// (wasm-layout.md §15.2): those whose declaration reaches itself through
/// value positions. Value positions are enum payloads, tuple elements and
/// `T?`, and another enum's argument when that enum holds the parameter
/// in a value position. Data, collections, functions and trait values are
/// references and end a path. A cycle decides, so a payload naming its
/// own enum at growing arguments (`E[T?]` in `E[T]`) is boxed like a
/// plain one, and every other enum's value layout is finite. A projection
/// in a value position is not resolved here, so its enum counts as
/// self-recursive.
#[must_use]
pub fn recursive_enums(pool: &InternPool, env: &dyn LayoutEnv, enums: &[DefId]) -> HashSet<DefId> {
    let mut g = ValueGraph {
        enums: enums.iter().copied().collect(),
        ..ValueGraph::default()
    };
    for &e in enums {
        for v in env.enum_declared(e).unwrap_or_default() {
            for f in v {
                g.walk(pool, e, f);
            }
        }
    }
    let index: HashMap<DefId, usize> = enums.iter().enumerate().map(|(i, d)| (*d, i)).collect();
    let succ: Vec<Vec<usize>> = enums
        .iter()
        .map(|e| {
            g.edges
                .get(e)
                .map(|to| to.iter().filter_map(|d| index.get(d).copied()).collect())
                .unwrap_or_default()
        })
        .collect();
    let comp = sccs(&succ);
    let mut size: HashMap<usize, usize> = HashMap::new();
    for c in &comp {
        *size.entry(*c).or_default() += 1;
    }
    let mut out = g.opaque;
    for (i, e) in enums.iter().enumerate() {
        if size[&comp[i]] > 1 || succ[i].contains(&i) {
            out.insert(*e);
        }
    }
    out
}

/// The enum graph of `recursive_enums`.
#[derive(Default)]
struct ValueGraph {
    enums: HashSet<DefId>,
    /// Enum to the enums it holds in a value position.
    edges: HashMap<DefId, Vec<DefId>>,
    /// An enum's parameters it holds in a value position.
    uses: HashSet<(DefId, u16)>,
    /// Arguments of an enum's parameter not known to be held yet: the
    /// declaration that wrote each, and the argument.
    waiting: HashMap<(DefId, u16), Vec<(DefId, Ty)>>,
    /// Enums with a projection in a value position.
    opaque: HashSet<DefId>,
}

impl ValueGraph {
    /// Walks `t`, a type `from` holds by value.
    fn walk(&mut self, pool: &InternPool, from: DefId, t: Ty) {
        let mut todo = vec![(from, t)];
        while let Some((f, t)) = todo.pop() {
            match pool.get(t) {
                TyData::Param(p) if p.owner == f => {
                    if self.uses.insert((f, p.index)) {
                        todo.extend(self.waiting.remove(&(f, p.index)).unwrap_or_default());
                    }
                }
                TyData::Adt { def, args } if self.enums.contains(&def) => {
                    self.edges.entry(f).or_default().push(def);
                    for (j, &a) in pool.list_items(args).iter().enumerate() {
                        let j = u16::try_from(j).expect("parameters");
                        if self.uses.contains(&(def, j)) {
                            todo.push((f, a));
                        } else {
                            self.waiting.entry((def, j)).or_default().push((f, a));
                        }
                    }
                }
                TyData::Tuple { elems, rest } => {
                    todo.extend(pool.list_items(elems).iter().map(|e| (f, *e)));
                    todo.extend(rest.map(|r| (f, r)));
                }
                TyData::Option(i) | TyData::Mut(i) => todo.push((f, i)),
                TyData::Assoc { .. } => {
                    self.opaque.insert(f);
                }
                _ => {}
            }
        }
    }
}

/// Strongly connected components of a graph given by successor lists:
/// per node, its component's number (Tarjan's algorithm, iterative).
#[must_use]
pub fn sccs(succ: &[Vec<usize>]) -> Vec<usize> {
    const UNSEEN: usize = usize::MAX;
    let n = succ.len();
    let (mut index, mut low, mut comp) = (vec![UNSEEN; n], vec![0; n], vec![UNSEEN; n]);
    let (mut stack, mut on) = (Vec::new(), vec![false; n]);
    let (mut next, mut ncomp) = (0, 0);
    for root in 0..n {
        if index[root] != UNSEEN {
            continue;
        }
        // (node, next successor to visit)
        let mut call = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on[root] = true;
        while let Some(&mut (v, ref mut k)) = call.last_mut() {
            if let Some(&w) = succ[v].get(*k) {
                *k += 1;
                if index[w] == UNSEEN {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on[w] = true;
                    call.push((w, 0));
                } else if on[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            call.pop();
            if let Some(&(p, _)) = call.last() {
                low[p] = low[p].min(low[v]);
            }
            if low[v] == index[v] {
                while let Some(w) = stack.pop() {
                    on[w] = false;
                    comp[w] = ncomp;
                    if w == v {
                        break;
                    }
                }
                ncomp += 1;
            }
        }
    }
    comp
}

/// The A1 class of a concrete type argument.
pub fn a1_class(pool: &InternPool, env: &dyn LayoutEnv, t: Ty) -> StageResult<A1Class> {
    let l = layout_of(pool, env, t)?;
    Ok(match (l.class, l.values.as_slice()) {
        (LayoutClass::Ref, [ValType::Ref { nullable: false }]) => A1Class::Ref,
        (LayoutClass::Ref, [ValType::Ref { nullable: true }]) => A1Class::RefNull,
        (LayoutClass::Void, _) => A1Class::Void,
        (_, [v]) => A1Class::Scalar(*v),
        _ => A1Class::Exact,
    })
}

/// `canon(T)`: the structural encoding over stable paths (§13.3). The
/// caller supplies each item's stable path hash.
pub fn canon(pool: &InternPool, path_hash: &dyn Fn(DefId) -> Hash128, t: Ty, h: &mut StableHasher) {
    match pool.get(t) {
        TyData::Prim(p) => {
            h.u8(0);
            h.u8(p as u8);
        }
        TyData::Never => h.u8(1),
        TyData::Adt { def, args } => {
            h.u8(2);
            h.hash(path_hash(def));
            canon_list(pool, path_hash, args, h);
        }
        TyData::Tuple { elems, rest } => {
            h.u8(3);
            canon_list(pool, path_hash, elems, h);
            if let Some(r) = rest {
                canon(pool, path_hash, r, h);
            }
        }
        TyData::Option(i) => {
            h.u8(4);
            canon(pool, path_hash, i, h);
        }
        TyData::Fn {
            params,
            result,
            row,
            suspends,
        } => {
            h.u8(5);
            canon_list(pool, path_hash, params, h);
            canon(pool, path_hash, result, h);
            canon_row(pool, path_hash, row, h);
            h.u8(u8::from(suspends));
        }
        TyData::TraitValue {
            def,
            args,
            bindings,
        } => {
            h.u8(6);
            h.hash(path_hash(def));
            canon_list(pool, path_hash, args, h);
            // Bindings in content order: (associated item path, canon).
            let mut bs: Vec<(Hash128, Hash128)> = bindings
                .into_iter()
                .map(|(a, t)| {
                    let mut kh = StableHasher::new("binding");
                    canon(pool, path_hash, t, &mut kh);
                    (path_hash(a), kh.finish())
                })
                .collect();
            bs.sort();
            h.u32(u32::try_from(bs.len()).expect("bindings"));
            for (a, t) in bs {
                h.hash(a);
                h.hash(t);
            }
        }
        TyData::Mut(i) => {
            h.u8(7);
            canon(pool, path_hash, i, h);
        }
        TyData::Row(r) => {
            h.u8(8);
            canon_row(pool, path_hash, r, h);
        }
        TyData::Poison => h.u8(9),
        TyData::Param(p) => {
            h.u8(10);
            h.hash(path_hash(p.owner));
            h.u16(p.index);
        }
        TyData::Assoc {
            assoc,
            trait_,
            self_ty,
            args,
        } => {
            h.u8(11);
            h.hash(path_hash(assoc));
            h.hash(path_hash(trait_));
            canon(pool, path_hash, self_ty, h);
            canon_list(pool, path_hash, args, h);
        }
        // Inference variables and canonical placeholders never reach an
        // instance key; encode the form only, never a run-local number.
        TyData::Infer(_) => h.u8(12),
        TyData::Canon(n) => {
            h.u8(13);
            h.u8(n);
        }
    }
}

/// A row's keys in content order (§6.5): each key's `canon` hash, sorted.
/// The pool lists keys in this run's interning order, which must not reach
/// an instance key.
fn canon_row(
    pool: &InternPool,
    path_hash: &dyn Fn(DefId) -> Hash128,
    r: hd_types::RowId,
    h: &mut StableHasher,
) {
    let mut keys: Vec<Hash128> = pool
        .row_data(r)
        .keys
        .into_iter()
        .map(|k| {
            let mut kh = StableHasher::new("row-key");
            canon(pool, path_hash, k, &mut kh);
            kh.finish()
        })
        .collect();
    keys.sort();
    h.u32(u32::try_from(keys.len()).expect("keys"));
    for k in keys {
        h.hash(k);
    }
    let mut params: Vec<(Hash128, u16)> = pool
        .row_data(r)
        .params
        .into_iter()
        .map(|p| (path_hash(p.owner), p.index))
        .collect();
    params.sort();
    h.u32(u32::try_from(params.len()).expect("params"));
    for (o, i) in params {
        h.hash(o);
        h.u16(i);
    }
}

fn canon_list(
    pool: &InternPool,
    path_hash: &dyn Fn(DefId) -> Hash128,
    l: TyList,
    h: &mut StableHasher,
) {
    let items = pool.list_items(l);
    h.u32(u32::try_from(items.len()).expect("list"));
    for &t in items {
        canon(pool, path_hash, t, h);
    }
}

/// A type argument as the instance key sees it: canon, or its class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyArg {
    Canon(Ty),
    Class(A1Class),
}

/// Which code of its item an instance is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sub {
    /// A body of the item's TIR: 0 for the item's own, `k` for its
    /// closure body `k`.
    Body(u16),
    /// A function reference's adapter, which has no TIR (codegen.md
    /// §13.11): its code forwards to one call that collection records as
    /// instruction 0.
    Adapter,
}

impl Sub {
    /// The kind, then the body index: an adapter never shares a key or a
    /// code-entry hash with a body.
    pub fn hash_into(self, h: &mut StableHasher) {
        match self {
            Sub::Body(k) => {
                h.u8(0);
                h.u16(k);
            }
            Sub::Adapter => h.u8(1),
        }
    }
}

impl std::fmt::Display for Sub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Sub::Body(k) => write!(f, "{k}"),
            Sub::Adapter => f.write_str("adapter"),
        }
    }
}

/// `instance_key = H("inst", item path, sub, [canon(arg) or class])` (§13.3).
pub fn instance_key(
    pool: &InternPool,
    path_hash: &dyn Fn(DefId) -> Hash128,
    item: DefId,
    sub: Sub,
    args: &[KeyArg],
) -> Hash128 {
    let mut h = StableHasher::new("inst");
    h.hash(path_hash(item));
    sub.hash_into(&mut h);
    h.u32(u32::try_from(args.len()).expect("args"));
    for a in args {
        match a {
            KeyArg::Canon(t) => {
                h.u8(0);
                canon(pool, path_hash, *t, &mut h);
            }
            KeyArg::Class(c) => {
                h.u8(1);
                h.str(&format!("{c:?}"));
            }
        }
    }
    h.finish()
}

/// The per-program instance table (data-structures.md §3.22).
#[derive(Default, Debug)]
pub struct InstanceTable {
    pub item: Vec<DefId>,
    pub sub: Vec<Sub>,
    pub args: Vec<TyList>,
    pub depth: Vec<u8>,
    pub parent: Vec<InstId>,
    pub key: Vec<Hash128>,
    index: std::collections::HashMap<(DefId, Sub, TyList), InstId>,
}

/// The instantiation depth limits (§13.4).
pub const MAX_DEPTH: u8 = 32;
pub const MAX_CHAIN: u32 = 256;

impl InstanceTable {
    /// Pushes an instance once; returns its id and whether it was new.
    pub fn push(
        &mut self,
        item: DefId,
        sub: Sub,
        args: TyList,
        depth: u8,
        parent: InstId,
        key: Hash128,
    ) -> StageResult<(InstId, bool)> {
        if let Some(&id) = self.index.get(&(item, sub, args)) {
            return Ok((id, false));
        }
        if depth > MAX_DEPTH {
            return Err(NotImplemented::new(
                Stage::Collect,
                "instantiation-too-deep diagnostic",
            ));
        }
        let id = InstId::from_raw(u32::try_from(self.item.len()).expect("instances"));
        self.item.push(item);
        self.sub.push(sub);
        self.args.push(args);
        self.depth.push(depth);
        self.parent.push(parent);
        self.key.push(key);
        self.index.insert((item, sub, args), id);
        Ok((id, true))
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.item.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.item.is_empty()
    }
    /// Content order: by instance key (§6.5), never by id.
    #[must_use]
    pub fn content_order(&self) -> Vec<InstId> {
        let mut ids: Vec<InstId> = (0..self.len())
            .map(|i| InstId::from_raw(u32::try_from(i).expect("i")))
            .collect();
        ids.sort_by_key(|i| self.key[i.idx()]);
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::{
        A1Class, InstanceTable, KeyArg, LayoutClass, LayoutEnv, StdKind, Sub, ValType, a1_class,
        instance_key, layout_of,
    };
    use hd_base::{DefId, Hash128, InstId};
    use hd_types::{InternPool, Ty, TyData, TyList};

    struct Env;
    impl LayoutEnv for Env {
        fn enum_variants(&self, def: DefId, _: TyList) -> Option<Vec<Vec<Ty>>> {
            match def.raw() {
                1 => Some(vec![vec![], vec![]]),
                2 => Some(vec![vec![Ty::I32], vec![Ty::STRING]]),
                _ => None,
            }
        }
        fn enum_declared(&self, def: DefId) -> Option<Vec<Vec<Ty>>> {
            self.enum_variants(def, TyList::EMPTY)
        }
        fn recursive_enum(&self, _: DefId) -> bool {
            false
        }
        fn std_kind(&self, _: DefId) -> StdKind {
            StdKind::Other
        }
    }

    /// Declared enums for `recursive_enums`, by raw id.
    struct Decls(Vec<(u32, Vec<Vec<Ty>>)>);
    impl LayoutEnv for Decls {
        fn enum_variants(&self, def: DefId, _: TyList) -> Option<Vec<Vec<Ty>>> {
            self.enum_declared(def)
        }
        fn enum_declared(&self, def: DefId) -> Option<Vec<Vec<Ty>>> {
            self.0
                .iter()
                .find(|(d, _)| *d == def.raw())
                .map(|(_, v)| v.clone())
        }
        fn recursive_enum(&self, _: DefId) -> bool {
            false
        }
        fn std_kind(&self, _: DefId) -> StdKind {
            StdKind::Other
        }
    }

    #[test]
    fn recursive_enums_follow_value_positions_only() {
        let p = InternPool::new();
        let d = DefId::from_raw;
        let param = |owner: u32, index: u16| {
            p.intern_ty(&TyData::Param(hd_types::ParamRef {
                owner: d(owner),
                index,
            }))
        };
        let adt = |def: u32, args: &[Ty]| {
            p.intern_ty(&TyData::Adt {
                def: d(def),
                args: p.list(args),
            })
        };
        let opt = |t: Ty| p.intern_ty(&TyData::Option(t));
        // 1: `Tree[T] = Leaf(T) | Node(Tree[T], Tree[T])`.
        let tree = adt(1, &[param(1, 0)]);
        // 2: `Grow[T] = More(Grow[T?]) | Done(T)`: growing arguments.
        let grow = adt(2, &[opt(param(2, 0))]);
        // 3: `Wrap[T] = W(T) | N`; 4: `Loop = A(Wrap[Loop])`.
        // 5: `Hold[T] = H(List[T])` (9 is a data type); 6: `Flat = F(Hold[Flat])`.
        let list_of = |t: Ty| adt(9, &[t]);
        let decls = Decls(vec![
            (1, vec![vec![param(1, 0)], vec![tree, tree]]),
            (2, vec![vec![grow], vec![param(2, 0)]]),
            (3, vec![vec![param(3, 0)], vec![]]),
            (4, vec![vec![adt(3, &[adt(4, &[])])]]),
            (5, vec![vec![list_of(param(5, 0))]]),
            (6, vec![vec![adt(5, &[adt(6, &[])])]]),
        ]);
        let all: Vec<DefId> = (1..=6).map(d).collect();
        let rec = super::recursive_enums(&p, &decls, &all);
        let mut got: Vec<u32> = rec.iter().map(|x| x.raw()).collect();
        got.sort_unstable();
        assert_eq!(got, [1, 2, 4]);
    }

    #[test]
    fn sccs_group_cycles() {
        let comp = super::sccs(&[vec![1], vec![0, 2], vec![2], vec![]]);
        assert_eq!(comp[0], comp[1]);
        assert_ne!(comp[0], comp[2]);
        assert_ne!(comp[2], comp[3]);
    }

    /// `canon` of a function type with a two-key row, the keys interned in
    /// the given order.
    fn fn_canon(flip: bool) -> Hash128 {
        let p = InternPool::new();
        let key = |d: u32| {
            p.intern_ty(&TyData::TraitValue {
                def: DefId::from_raw(d),
                args: TyList::EMPTY,
                bindings: vec![],
            })
        };
        let (a, b) = if flip {
            let b = key(6);
            (key(5), b)
        } else {
            let a = key(5);
            (a, key(6))
        };
        let row = p.row(&hd_types::RowData {
            keys: vec![a, b],
            params: vec![],
        });
        let f = p.intern_ty(&TyData::Fn {
            params: TyList::EMPTY,
            result: Ty::I32,
            row,
            suspends: false,
        });
        let mut h = hd_base::StableHasher::new("t");
        super::canon(&p, &|d| Hash128(u128::from(d.raw()) * 977), f, &mut h);
        h.finish()
    }

    /// `canon` of a tuple of a parameter, an associated projection and a
    /// trait value with a binding, interned in the given order. The owner
    /// ids differ per run; the path hash maps them to the same content.
    fn param_canon(flip: bool) -> Hash128 {
        let p = InternPool::new();
        // Run-local ids: `flip` swaps which raw id names which item.
        let (owner, trait_, assoc, other) = if flip {
            (40, 30, 20, 10)
        } else {
            (10, 20, 30, 40)
        };
        let path = move |d: DefId| {
            let name = match d.raw() {
                x if x == owner => 1u128,
                x if x == trait_ => 2,
                x if x == assoc => 3,
                _ => 4,
            };
            Hash128(name * 977)
        };
        let param = |i: u16| {
            p.intern_ty(&TyData::Param(hd_types::ParamRef {
                owner: DefId::from_raw(owner),
                index: i,
            }))
        };
        let (t0, t1) = if flip {
            let t1 = param(1);
            (param(0), t1)
        } else {
            let t0 = param(0);
            (t0, param(1))
        };
        let proj = p.intern_ty(&TyData::Assoc {
            assoc: DefId::from_raw(assoc),
            trait_: DefId::from_raw(trait_),
            self_ty: t0,
            args: TyList::EMPTY,
        });
        let tv = p.intern_ty(&TyData::TraitValue {
            def: DefId::from_raw(trait_),
            args: TyList::EMPTY,
            bindings: vec![
                (DefId::from_raw(assoc), t1),
                (DefId::from_raw(other), Ty::I32),
            ],
        });
        let tup = p.intern_ty(&TyData::Tuple {
            elems: p.list(&[proj, tv]),
            rest: None,
        });
        let mut h = hd_base::StableHasher::new("t");
        super::canon(&p, &path, tup, &mut h);
        h.finish()
    }

    #[test]
    fn canon_of_params_and_projections_ignores_interning_order() {
        assert_eq!(param_canon(false), param_canon(true));
    }

    #[test]
    fn canon_sees_trait_value_bindings() {
        let p = InternPool::new();
        let tv = |t: Ty| {
            p.intern_ty(&TyData::TraitValue {
                def: DefId::from_raw(1),
                args: TyList::EMPTY,
                bindings: vec![(DefId::from_raw(2), t)],
            })
        };
        let enc = |t: Ty| {
            let mut h = hd_base::StableHasher::new("t");
            super::canon(&p, &|d| Hash128(u128::from(d.raw())), t, &mut h);
            h.finish()
        };
        assert_ne!(enc(tv(Ty::I32)), enc(tv(Ty::STRING)));
    }

    #[test]
    fn canon_of_a_row_ignores_interning_order() {
        assert_eq!(fn_canon(false), fn_canon(true));
    }

    #[test]
    fn layouts_cover_every_concrete_form() {
        let p = InternPool::new();
        let adt = |d: u32| {
            p.intern_ty(&TyData::Adt {
                def: DefId::from_raw(d),
                args: TyList::EMPTY,
            })
        };
        let data = adt(9);
        assert_eq!(
            layout_of(&p, &Env, Ty::I32).expect("i32").class,
            LayoutClass::I32
        );
        assert_eq!(
            layout_of(&p, &Env, Ty::BOOL).expect("bool").packed_bits,
            Some(8)
        );
        assert_eq!(
            layout_of(&p, &Env, Ty::STRING).expect("str").values.len(),
            2
        );
        assert_eq!(
            layout_of(&p, &Env, adt(1)).expect("tag enum").class,
            LayoutClass::I32
        );
        let value_enum = layout_of(&p, &Env, adt(2)).expect("value enum");
        assert_eq!(value_enum.class, LayoutClass::Multi);
        assert_eq!(
            value_enum.values,
            [ValType::I32, ValType::I32, ValType::EqRef, ValType::I64]
        );
        let opt_data = p.intern_ty(&TyData::Option(data));
        assert_eq!(
            layout_of(&p, &Env, opt_data).expect("opt").values,
            [ValType::Ref { nullable: true }]
        );
        let opt_i32 = p.intern_ty(&TyData::Option(Ty::I32));
        assert_eq!(
            layout_of(&p, &Env, opt_i32).expect("opt i32").values.len(),
            2
        );
        let tv = p.intern_ty(&TyData::TraitValue {
            def: DefId::from_raw(3),
            args: TyList::EMPTY,
            bindings: vec![],
        });
        assert_eq!(
            layout_of(&p, &Env, tv).expect("dyn").values[0],
            ValType::EqRef
        );
        let big = p.intern_ty(&TyData::Tuple {
            elems: p.list(&[Ty::STRING, Ty::STRING, Ty::I32]),
            rest: None,
        });
        assert_eq!(
            layout_of(&p, &Env, big).expect("boxed tuple").class,
            LayoutClass::Ref
        );
        assert_eq!(a1_class(&p, &Env, data).expect("a1"), A1Class::Ref);
        assert_eq!(a1_class(&p, &Env, opt_data).expect("a1"), A1Class::RefNull);
        assert!(layout_of(&p, &Env, Ty::POISON).is_err());
    }

    #[test]
    fn class_instances_share_one_key() {
        let p = InternPool::new();
        let ph = |d: DefId| Hash128(u128::from(d.raw()) * 7919);
        let push = DefId::from_raw(4);
        let body = Sub::Body(0);
        let a = instance_key(&p, &ph, push, body, &[KeyArg::Class(A1Class::Ref)]);
        let b = instance_key(&p, &ph, push, body, &[KeyArg::Class(A1Class::Ref)]);
        let c = instance_key(&p, &ph, push, body, &[KeyArg::Canon(Ty::I32)]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        // An adapter and the last body index never share a key.
        let last = instance_key(&p, &ph, push, Sub::Body(u16::MAX), &[]);
        assert_ne!(last, instance_key(&p, &ph, push, Sub::Adapter, &[]));
        let mut t = InstanceTable::default();
        assert!(
            t.push(push, body, TyList::EMPTY, 0, InstId::NONE, a)
                .expect("push")
                .1
        );
        assert!(
            !t.push(push, body, TyList::EMPTY, 0, InstId::NONE, a)
                .expect("push")
                .1
        );
        assert!(
            t.push(push, Sub::Body(1), TyList::EMPTY, 33, InstId::NONE, c)
                .is_err()
        );
    }
}
