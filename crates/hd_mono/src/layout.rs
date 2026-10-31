//! Instance keys, A1 classes and Wasm layouts over the full type
//! representation (`crates/hd_mono/DESIGN.md` §13.2, §13.3; `crates/hd_wasm_linear/DESIGN.md` §15.1, §15.2;
//! `docs/compiler/data-structures.md` §3.22).

use hd_base::SliceSort as _;
use hd_base::{DefId, Hash128, InstId, NotImplemented, StableHasher, Stage, StageResult};
use hd_base::{HashMap, HashSet};

use hd_types::{InternPool, ParamRef, Prim, Ty, TyData, TyList};

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
    /// `std.rt.memory.Buffer[T]`: the address of a counted object holding
    /// a capacity word, a length word, then the values, as a list's
    /// buffer is laid out (`module.rt.buffer.type`).
    Buffer,
    Other,
}

/// Whether a map key type hashes and compares inline (`crates/hd_mono/DESIGN.md` §13.12):
/// an integer, `bool`, `char` or `string`, whose `Eq` and `Hash` are
/// std's fixed ones, erasing newtypes to their base. Every other key
/// calls its own impls.
#[must_use]
pub fn inline_map_key(pool: &InternPool, env: &dyn LayoutEnv, t: Ty) -> bool {
    matches!(
        pool.get(strip_newtype(pool, env, t)),
        TyData::Prim(p) if !matches!(p, Prim::F32 | Prim::F64)
    )
}

/// Follows nominal newtypes to their bases, substituting arguments: a
/// newtype erases to its base at run time (`types.newtype.construct-ref`,
/// `types.newtype.construct-value`), so value layouts see the base. Type
/// identity stays nominal; only layouts strip.
#[must_use]
pub fn strip_newtype(pool: &InternPool, env: &dyn LayoutEnv, t: Ty) -> Ty {
    let Some((mut def, mut args)) = pool.adt(t) else {
        return t;
    };
    let Some(mut base) = env.newtype_base(def) else {
        return t;
    };
    let mut t = t;
    // Cyclic newtype declarations have no values. Bound their traversal
    // far above real nesting, without stopping valid repeated wrappers
    // such as `Foo[Foo[i32]]` at a repeated declaration.
    for _ in 0..1_000 {
        let substitutions = pool.list_items(args);
        t = pool.subst(base, &|p: ParamRef| {
            (p.owner == def)
                .then(|| substitutions.get(usize::from(p.index)).copied())
                .flatten()
        });
        let Some((next_def, next_args)) = pool.adt(t) else {
            break;
        };
        let Some(next_base) = env.newtype_base(next_def) else {
            break;
        };
        def = next_def;
        args = next_args;
        base = next_base;
    }
    t
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
    /// The base type a nominal newtype erases to at run time, over its
    /// own parameters (`types.newtype.construct-ref`,
    /// `types.newtype.construct-value`); `None` for any other item.
    fn newtype_base(&self, def: DefId) -> Option<Ty>;
}

/// The layout of a type (§15.1 and the value table of §15.2). Every type
/// form has a row; forms that need declared shapes ask `env`. A
/// self-recursive enum is one boxed reference (`recursive_enums`), so the
/// walk through value positions is finite. The enums `t` holds by value
/// are laid out first, innermost first (`value_enums`), so the recursion
/// is only as deep as tuples and optionals nest.
pub fn layout_of(pool: &InternPool, env: &dyn LayoutEnv, t: Ty) -> StageResult<Layout> {
    let t = strip_newtype(pool, env, t);
    let mut memo: HashMap<Ty, Layout> = HashMap::default();
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
    let mut seen = HashSet::default();
    let mut stack = vec![(strip_newtype(pool, env, t), false)];
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
            TyData::Option(i) | TyData::Mut(i) => {
                stack.push((i, false));
            }
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
    let t = strip_newtype(pool, env, t);
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
        // A rest element is the tuple's last member, a `List`.
        TyData::Tuple { elems, rest } => {
            let mut values = Vec::new();
            for e in pool.list_items(elems).iter().copied().chain(rest) {
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
        TyData::Union(_) => {
            return Err(NotImplemented::new(Stage::Emit, "union type layout"));
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
        // The checker lowers every reference type to `std.core.Ref[T]`
        // (`hd_types::Types::lower_refs`).
        TyData::Ref { .. } => {
            return Err(NotImplemented::bug(
                Stage::Emit,
                "layout of a reference type the checker did not lower",
            ));
        }
        TyData::Row(_) => {
            return Err(NotImplemented::new(
                Stage::Emit,
                "the layout of a requirement row",
            ));
        }
        // A const argument has no values (`types.generic.const.runtime`).
        TyData::Const(_) => {
            return Err(NotImplemented::new(
                Stage::Emit,
                "the layout of a const generic argument",
            ));
        }
        // One providers array (`crates/hd_mono/DESIGN.md` §12.4, "Context values").
        TyData::Context(_) => one(LayoutClass::Ref, ValType::Ref { nullable: false }),
        TyData::Opaque { .. }
        | TyData::Param(_)
        | TyData::Assoc { .. }
        | TyData::Infer(_)
        | TyData::Canon(_)
        | TyData::Poison => {
            return Err(NotImplemented::bug(
                Stage::Emit,
                "layout of a non-concrete type (substitution missing)",
            ));
        }
    };
    Ok(l)
}

/// The self-recursive enums among `enums`, the program's enum declarations
/// (`crates/hd_wasm_linear/DESIGN.md` §15.2): those whose declaration reaches itself through
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
    let mut size: HashMap<usize, usize> = HashMap::default();
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
                TyData::Option(i) | TyData::Mut(i) => {
                    todo.push((f, i));
                }
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

/// Each type's `canon` digest, for one build: a type is encoded once, and
/// every type that holds it reads its digest back.
#[derive(Default, Debug)]
pub struct CanonMemo(HashMap<Ty, Hash128>);

/// `canon(T)`: the structural encoding over stable paths (§13.3), as a
/// Merkle digest. Each part of a compound type enters as its own digest,
/// memoized in `memo`, so equal types hash equal however they were built,
/// and the walk is linear in the distinct types of `t`, never in its tree
/// (a tuple that doubles per level has 2^d leaves but d + 1 types). The
/// caller supplies each item's stable path hash.
///
/// A requirement key's content hash is `canon` of the whole key, the trait
/// with its type arguments and bindings (`req.key.binding.identity`,
/// `crates/hd_mono/DESIGN.md` §13.18); content order of a row's keys is its order.
pub fn canon(
    pool: &InternPool,
    path_hash: &dyn Fn(DefId) -> Hash128,
    memo: &mut CanonMemo,
    t: Ty,
) -> Hash128 {
    if let Some(&d) = memo.0.get(&t) {
        return d;
    }
    let mut h = StableHasher::new("canon");
    match pool.get(t) {
        TyData::Prim(p) => {
            h.u8(0);
            h.u8(p as u8);
        }
        TyData::Never => h.u8(1),
        TyData::Adt { def, args } => {
            h.u8(2);
            h.hash(path_hash(def));
            canon_list(pool, path_hash, memo, args, &mut h);
        }
        TyData::Tuple { elems, rest } => {
            h.u8(3);
            canon_list(pool, path_hash, memo, elems, &mut h);
            if let Some(r) = rest {
                h.hash(canon(pool, path_hash, memo, r));
            }
        }
        TyData::Union(members) => {
            h.u8(20);
            canon_list(pool, path_hash, memo, members, &mut h);
        }
        TyData::Option(i) => {
            h.u8(4);
            h.hash(canon(pool, path_hash, memo, i));
        }
        TyData::Fn {
            params,
            result,
            row,
            suspends,
            inputs,
            // Erased before monomorphization (`Types::lower_refs`).
            deps: _,
        } => {
            h.u8(5);
            canon_list(pool, path_hash, memo, params, &mut h);
            h.hash(canon(pool, path_hash, memo, result));
            canon_row(pool, path_hash, memo, row, &mut h);
            h.u32(inputs.word(suspends));
        }
        TyData::TraitValue {
            def,
            args,
            bindings,
        } => {
            h.u8(6);
            h.hash(path_hash(def));
            canon_list(pool, path_hash, memo, args, &mut h);
            // Bindings in content order: (associated item path, canon).
            let mut bs: Vec<(Hash128, Hash128)> = bindings
                .into_iter()
                .map(|(a, t)| (path_hash(a), canon(pool, path_hash, memo, t)))
                .collect();
            bs.sort_stable();
            h.u32(u32::try_from(bs.len()).expect("bindings"));
            for (a, t) in bs {
                h.hash(a);
                h.hash(t);
            }
        }
        TyData::Mut(i) => {
            h.u8(7);
            h.hash(canon(pool, path_hash, memo, i));
        }
        // Not in an instance key: the checker lowers reference types.
        // The kind is erased (`ref.type.kind-runtime`).
        TyData::Ref { inner, mutable, .. } => {
            h.u8(17);
            h.u8(u8::from(mutable));
            h.hash(canon(pool, path_hash, memo, inner));
        }
        TyData::Row(r) => {
            h.u8(8);
            canon_row(pool, path_hash, memo, r, &mut h);
        }
        TyData::Poison => h.u8(9),
        TyData::Opaque {
            owner,
            site,
            args,
            bound,
            // A function of `owner`.
            borrows: _,
        } => {
            h.u8(18);
            h.hash(path_hash(owner));
            h.u32(site);
            canon_list(pool, path_hash, memo, args, &mut h);
            h.hash(canon(pool, path_hash, memo, bound));
        }
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
            h.hash(canon(pool, path_hash, memo, self_ty));
            canon_list(pool, path_hash, memo, args, &mut h);
        }
        // Inference variables and canonical placeholders never reach an
        // instance key; encode the form only, never a run-local number.
        TyData::Infer(_) => h.u8(12),
        TyData::Canon(n) => {
            h.u8(13);
            h.u8(n);
        }
        TyData::Context(r) => {
            h.u8(14);
            canon_row(pool, path_hash, memo, r, &mut h);
        }
        // A string is its text's stable path.
        TyData::Const(hd_types::ConstValue::Str(p)) => {
            h.u8(19);
            h.u8(Prim::String as u8);
            h.hash(path_hash(DefId::from_raw(p.raw())));
        }
        TyData::Const(v) => {
            h.u8(19);
            h.u8(v.prim() as u8);
            h.u64(v.bits());
        } // 15 was an owned type, `own T`, and 16 a borrowed type, `^T`;
          // they are not reused.
    }
    let d = h.finish();
    memo.0.insert(t, d);
    d
}

/// A row's keys in content order (§6.5): each key's `canon` digest, sorted.
/// The pool lists keys in this run's interning order, which must not reach
/// an instance key.
fn canon_row(
    pool: &InternPool,
    path_hash: &dyn Fn(DefId) -> Hash128,
    memo: &mut CanonMemo,
    r: hd_types::RowId,
    h: &mut StableHasher,
) {
    let mut keys: Vec<Hash128> = pool
        .row_data(r)
        .keys
        .into_iter()
        .map(|k| canon(pool, path_hash, memo, k))
        .collect();
    keys.sort_stable();
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
    params.sort_stable();
    h.u32(u32::try_from(params.len()).expect("params"));
    for (o, i) in params {
        h.hash(o);
        h.u16(i);
    }
}

fn canon_list(
    pool: &InternPool,
    path_hash: &dyn Fn(DefId) -> Hash128,
    memo: &mut CanonMemo,
    l: TyList,
    h: &mut StableHasher,
) {
    let items = pool.list_items(l);
    h.u32(u32::try_from(items.len()).expect("list"));
    for &t in items {
        h.hash(canon(pool, path_hash, memo, t));
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
    /// A function reference's adapter, which has no TIR (`crates/hd_mono/DESIGN.md`
    /// §13.11): its code forwards to one call that collection records as
    /// instruction 0.
    Adapter,
    /// The erased body of a generic method that a vtable slot holds
    /// (`crates/hd_mono/DESIGN.md` §13.5.1): the item's body at its owner's concrete
    /// arguments, its own type parameters open (`open_param`).
    Erased,
    /// Thunk `k` of an erased body: its open instruction `k`, in TIR
    /// order, at concrete method type arguments (§13.5.1).
    Thunk(u16),
}

impl Sub {
    /// The kind, then the body or thunk index: an adapter, an erased body
    /// or a thunk never shares a key or a code-entry hash with a body.
    pub fn hash_into(self, h: &mut StableHasher) {
        match self {
            Sub::Body(k) => {
                h.u8(0);
                h.u16(k);
            }
            Sub::Adapter => h.u8(1),
            Sub::Erased => h.u8(2),
            Sub::Thunk(k) => {
                h.u8(3);
                h.u16(k);
            }
        }
    }
}

impl std::fmt::Display for Sub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Sub::Body(k) => write!(f, "{k}"),
            Sub::Adapter => f.write_str("adapter"),
            Sub::Erased => f.write_str("erased"),
            Sub::Thunk(k) => write!(f, "thunk{k}"),
        }
    }
}

/// `instance_key = H("inst", item path, sub, [canon(arg) or class])` (§13.3).
pub fn instance_key(
    pool: &InternPool,
    path_hash: &dyn Fn(DefId) -> Hash128,
    memo: &mut CanonMemo,
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
                h.hash(canon(pool, path_hash, memo, *t));
            }
            KeyArg::Class(c) => {
                h.u8(1);
                h.str(&format!("{c:?}"));
            }
        }
    }
    h.finish()
}

/// The known closures of a specialized instance (`crates/hd_opt/DESIGN.md` (known closure calls), option
/// A2): per parameter index the instance's caller always passes one closure
/// literal for, that literal's code instance key, sorted by index. Empty
/// for every other instance.
pub type Known = Box<[(u32, Hash128)]>;

/// The table's index word for `known`: 0 when empty, so a plain instance
/// keeps its identity.
pub(crate) fn known_digest(known: &[(u32, Hash128)]) -> Hash128 {
    if known.is_empty() {
        return Hash128(0);
    }
    let mut h = StableHasher::new("known");
    h.u32(u32::try_from(known.len()).expect("known"));
    for &(p, k) in known {
        h.u32(p);
        h.hash(k);
    }
    h.finish()
}

/// A specialized instance's key: its plain instance key, then its known
/// closures (A2's key component). Each literal is an identity, its code
/// instance key, never a hash of its body. An empty `known` is the plain
/// key itself.
#[must_use]
pub fn known_key(plain: Hash128, known: &[(u32, Hash128)]) -> Hash128 {
    if known.is_empty() {
        return plain;
    }
    let mut h = StableHasher::new("inst-known");
    h.hash(plain);
    h.hash(known_digest(known));
    h.finish()
}

/// The per-program instance table (`docs/compiler/data-structures.md` §3.22).
#[derive(Default, Debug)]
pub struct InstanceTable {
    pub item: Vec<DefId>,
    pub sub: Vec<Sub>,
    pub args: Vec<TyList>,
    /// The known closures of a specialized instance; empty for a plain one.
    pub known: Vec<Known>,
    /// Pure omitted defaults used by each instance.
    pub defaults: Vec<Box<[(u32, crate::defaults::Value)]>>,
    /// The nesting depth of the instance's deepest type argument (§13.4).
    pub depth: Vec<u8>,
    /// The instance that first requested it, `NONE` for a root. Collection
    /// is breadth-first, so the parents spell a shortest chain from a root.
    pub parent: Vec<InstId>,
    pub key: Vec<Hash128>,
    index: hd_base::HashMap<(DefId, Sub, TyList, Hash128), InstId>,
}

/// The instantiation limits (§13.4): the nesting depth of an instance's
/// type arguments, and the length of its request chain from a root.
pub const MAX_DEPTH: u8 = 32;
pub const MAX_CHAIN: u32 = 256;

impl InstanceTable {
    /// The instance of `item`'s code `sub` at `args`, if collected.
    #[must_use]
    pub fn get(&self, item: DefId, sub: Sub, args: TyList) -> Option<InstId> {
        self.get_known(item, sub, args, &[])
    }

    /// The instance of `item`'s code `sub` at `args` specialized on
    /// `known` (plain when empty), if collected.
    #[must_use]
    pub fn get_known(
        &self,
        item: DefId,
        sub: Sub,
        args: TyList,
        known: &[(u32, Hash128)],
    ) -> Option<InstId> {
        self.get_with_defaults(item, sub, args, known, &[])
    }

    /// The instance at a complete specialization, including omitted defaults.
    #[must_use]
    pub fn get_with_defaults(
        &self,
        item: DefId,
        sub: Sub,
        args: TyList,
        known: &[(u32, Hash128)],
        defaults: &[(u32, crate::defaults::Value)],
    ) -> Option<InstId> {
        self.index
            .get(&(item, sub, args, crate::defaults::digest(known, defaults)))
            .copied()
    }

    /// Pushes an instance once; returns its id and whether it was new.
    /// The caller checks the limits first.
    pub fn push(
        &mut self,
        item: DefId,
        sub: Sub,
        args: TyList,
        known: &[(u32, Hash128)],
        depth: u8,
        parent: InstId,
        key: Hash128,
    ) -> (InstId, bool) {
        self.push_with_defaults(item, sub, args, known, &[], depth, parent, key)
    }

    /// Pushes a value-specialized instance without changing plain instance keys.
    pub fn push_with_defaults(
        &mut self,
        item: DefId,
        sub: Sub,
        args: TyList,
        known: &[(u32, Hash128)],
        defaults: &[(u32, crate::defaults::Value)],
        depth: u8,
        parent: InstId,
        key: Hash128,
    ) -> (InstId, bool) {
        if let Some(id) = self.get_with_defaults(item, sub, args, known, defaults) {
            return (id, false);
        }
        let id = InstId::from_raw(u32::try_from(self.item.len()).expect("instances"));
        self.item.push(item);
        self.sub.push(sub);
        self.args.push(args);
        self.known.push(known.into());
        self.defaults.push(defaults.into());
        self.depth.push(depth);
        self.parent.push(parent);
        self.key.push(key);
        self.index.insert(
            (item, sub, args, crate::defaults::digest(known, defaults)),
            id,
        );
        (id, true)
    }

    /// The number of requests from a root to `id`: 0 for a root.
    #[must_use]
    pub fn chain_len(&self, id: InstId) -> u32 {
        let mut n = 0;
        let mut p = self.parent[id.idx()];
        while p != InstId::NONE {
            n += 1;
            p = self.parent[p.idx()];
        }
        n
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
        ids.sort_stable_by_key(|i| self.key[i.idx()]);
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::{
        A1Class, CanonMemo, InstanceTable, KeyArg, LayoutClass, LayoutEnv, StdKind, Sub, ValType,
        a1_class, inline_map_key, instance_key, known_key, layout_of, strip_newtype,
    };
    use hd_base::SliceSort as _;
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
        fn newtype_base(&self, _: DefId) -> Option<Ty> {
            None
        }
    }

    struct Newtypes(Vec<(DefId, Ty)>);
    impl LayoutEnv for Newtypes {
        fn enum_variants(&self, _: DefId, _: TyList) -> Option<Vec<Vec<Ty>>> {
            None
        }
        fn enum_declared(&self, _: DefId) -> Option<Vec<Vec<Ty>>> {
            None
        }
        fn recursive_enum(&self, _: DefId) -> bool {
            false
        }
        fn std_kind(&self, _: DefId) -> StdKind {
            StdKind::Other
        }
        fn newtype_base(&self, def: DefId) -> Option<Ty> {
            self.0.iter().find(|(d, _)| *d == def).map(|(_, t)| *t)
        }
    }

    #[test]
    fn strip_newtype_preserves_non_newtypes() {
        let p = InternPool::new();
        let ordinary = p.intern_ty(&TyData::Adt {
            def: DefId::from_raw(1),
            args: TyList::EMPTY,
        });
        for t in [Ty::I32, Ty::STRING, ordinary] {
            assert_eq!(strip_newtype(&p, &Env, t), t);
        }
    }

    #[test]
    fn strip_newtype_fully_strips_repeated_generic_wrappers() {
        let p = InternPool::new();
        let def = DefId::from_raw(1);
        let base = p.intern_ty(&TyData::Param(hd_types::ParamRef {
            owner: def,
            index: 0,
        }));
        let env = Newtypes(vec![(def, base)]);
        for scalar in [Ty::I32, Ty::STRING] {
            let mut wrapped = scalar;
            for _ in 0..32 {
                wrapped = p.intern_ty(&TyData::Adt {
                    def,
                    args: p.list(&[wrapped]),
                });
            }
            assert_eq!(strip_newtype(&p, &env, wrapped), scalar);
            assert!(inline_map_key(&p, &env, wrapped));
            assert_eq!(layout_of(&p, &env, wrapped), layout_of(&p, &env, scalar));
        }
    }

    #[test]
    fn strip_newtype_bounds_cyclic_declarations() {
        let p = InternPool::new();
        let adt = |raw| {
            p.intern_ty(&TyData::Adt {
                def: DefId::from_raw(raw),
                args: TyList::EMPTY,
            })
        };
        let (a, b) = (adt(1), adt(2));
        let direct = Newtypes(vec![(DefId::from_raw(1), a)]);
        assert_eq!(strip_newtype(&p, &direct, a), a);
        let mutual = Newtypes(vec![(DefId::from_raw(1), b), (DefId::from_raw(2), a)]);
        assert_eq!(strip_newtype(&p, &mutual, a), a);
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
        fn newtype_base(&self, _: DefId) -> Option<Ty> {
            None
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
        got.sort_stable();
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
            inputs: hd_types::Inputs::Fixed,
            deps: hd_types::FnDeps::NONE,
        });
        let ph = |d: DefId| Hash128(u128::from(d.raw()) * 977);
        super::canon(&p, &ph, &mut CanonMemo::default(), f)
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
        super::canon(&p, &path, &mut CanonMemo::default(), tup)
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
            let ph = |d: DefId| Hash128(u128::from(d.raw()));
            super::canon(&p, &ph, &mut CanonMemo::default(), t)
        };
        assert_ne!(enc(tv(Ty::I32)), enc(tv(Ty::STRING)));
    }

    #[test]
    fn canon_preserves_union_order_duplicates_and_nesting() {
        let p = InternPool::new();
        let union = |members: &[Ty]| p.intern_ty(&TyData::Union(p.list(members)));
        let encode = |t| super::canon(&p, &|_| Hash128(0), &mut CanonMemo::default(), t);
        let ab = union(&[Ty::I32, Ty::STRING]);
        let ba = union(&[Ty::STRING, Ty::I32]);
        let duplicate = union(&[Ty::I32, Ty::I32]);
        let flat = union(&[Ty::I32, Ty::STRING, Ty::BOOL]);
        let nested = union(&[ab, Ty::BOOL]);

        assert_ne!(encode(ab), encode(ba));
        assert_ne!(encode(ab), encode(duplicate));
        assert_ne!(encode(flat), encode(nested));
    }

    #[test]
    fn canon_of_a_row_ignores_interning_order() {
        assert_eq!(fn_canon(false), fn_canon(true));
    }

    /// `T` paired with itself `levels` times over: a tree of 2^levels
    /// leaves, but `levels + 1` distinct types in the pool.
    fn doubled(p: &InternPool, levels: u32) -> Ty {
        (0..levels).fold(Ty::I32, |t, _| {
            p.intern_ty(&TyData::Tuple {
                elems: p.list(&[t, t]),
                rest: None,
            })
        })
    }

    #[test]
    fn canon_walks_distinct_types_not_the_tree() {
        // 2^64 leaves: a tree walk would never end.
        let p = InternPool::new();
        let ph = |d: DefId| Hash128(u128::from(d.raw()));
        let mut memo = CanonMemo::default();
        let deep = doubled(&p, 64);
        let start = std::time::Instant::now();
        let k = instance_key(
            &p,
            &ph,
            &mut memo,
            DefId::from_raw(1),
            Sub::Body(0),
            &[KeyArg::Canon(deep)],
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
        assert_eq!(memo.0.len(), 65);
        assert_ne!(k, Hash128(0));
    }

    #[test]
    fn equal_types_built_two_ways_hash_equal() {
        // `((i32, i32), (i32, i32))` from the shared pair, and from two
        // pairs built in another pool in another order: one digest, and
        // one with no memo shared between them.
        let ph = |d: DefId| Hash128(u128::from(d.raw()));
        let p = InternPool::new();
        let shared = doubled(&p, 2);
        let q = InternPool::new();
        let _noise = q.intern_ty(&TyData::Option(Ty::STRING));
        let pair = |q: &InternPool| {
            q.intern_ty(&TyData::Tuple {
                elems: q.list(&[Ty::I32, Ty::I32]),
                rest: None,
            })
        };
        let (left, right) = (pair(&q), pair(&q));
        let built = q.intern_ty(&TyData::Tuple {
            elems: q.list(&[left, right]),
            rest: None,
        });
        let a = super::canon(&p, &ph, &mut CanonMemo::default(), shared);
        let b = super::canon(&q, &ph, &mut CanonMemo::default(), built);
        assert_eq!(a, b);
        // A warm memo gives the same digest as a cold one.
        let mut memo = CanonMemo::default();
        let _ = super::canon(&q, &ph, &mut memo, left);
        assert_eq!(super::canon(&q, &ph, &mut memo, built), b);
        // And a different type a different one.
        let other = doubled(&p, 3);
        assert_ne!(super::canon(&p, &ph, &mut CanonMemo::default(), other), a);
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
        let m = &mut CanonMemo::default();
        let a = instance_key(&p, &ph, m, push, body, &[KeyArg::Class(A1Class::Ref)]);
        let b = instance_key(&p, &ph, m, push, body, &[KeyArg::Class(A1Class::Ref)]);
        let c = instance_key(&p, &ph, m, push, body, &[KeyArg::Canon(Ty::I32)]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        // An adapter and the last body index never share a key.
        let last = instance_key(&p, &ph, m, push, Sub::Body(u16::MAX), &[]);
        assert_ne!(last, instance_key(&p, &ph, m, push, Sub::Adapter, &[]));
        let mut t = InstanceTable::default();
        let (root, new) = t.push(push, body, TyList::EMPTY, &[], 0, InstId::NONE, a);
        assert!(new);
        assert!(!t.push(push, body, TyList::EMPTY, &[], 0, InstId::NONE, a).1);
        assert_eq!(t.get(push, body, TyList::EMPTY), Some(root));
        // The chain counts requests from the root.
        let (inner, _) = t.push(push, Sub::Body(1), TyList::EMPTY, &[], 0, root, c);
        assert_eq!(t.chain_len(root), 0);
        assert_eq!(t.chain_len(inner), 1);
        // A specialized instance (A2) sits beside the plain one; the same
        // known closures find it again, other ones make another.
        let k1 = [(2, Hash128(11))];
        let k2 = [(2, Hash128(12))];
        assert_eq!(known_key(a, &[]), a);
        assert_ne!(known_key(a, &k1), known_key(a, &k2));
        let (copy, new) = t.push(push, body, TyList::EMPTY, &k1, 0, root, known_key(a, &k1));
        assert!(new && copy != root);
        assert_eq!(t.get_known(push, body, TyList::EMPTY, &k1), Some(copy));
        assert_eq!(t.get_known(push, body, TyList::EMPTY, &k2), None);
        assert_eq!(t.get(push, body, TyList::EMPTY), Some(root));
    }
}
