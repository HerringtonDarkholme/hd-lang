//! Entry-local tables (`docs/compiler/data-structures.md` §3.20.2): run IDs never reach a
//! cache entry. A writer collects the stable paths, types, type lists and
//! symbols one blob mentions and gives each a row; the reader re-interns
//! the rows into this run's tables. Rows refer only to earlier rows.

use hd_base::HashMap;

use hd_base::wire::{Reader, Writer};
use hd_base::{DefId, NotImplemented, PathId, Stage, StageResult, Symbol};
use hd_intern::{PathKind, PathTable, ShardedInterner};

use crate::pool::{
    Inputs, InternPool, ParamRef, Prim, RowData, RowId, RowParamRef, Ty, TyData, TyList,
};

const NONE: u32 = u32::MAX;

/// Collects the rows of one blob's tables.
pub struct TableWriter<'a> {
    pool: &'a InternPool,
    paths: &'a PathTable,
    syms: &'a ShardedInterner,
    path_rows: Vec<(u32, PathId)>,
    path_index: HashMap<PathId, u32>,
    ty_rows: Vec<(u8, std::ops::Range<usize>)>,
    ty_words: Vec<u32>,
    ty_index: HashMap<u32, u32>,
    list_index: HashMap<u32, u32>,
    row_index: HashMap<u32, u32>,
    sym_rows: Vec<Symbol>,
    sym_index: HashMap<Symbol, u32>,
}

/// Type-row tags.
mod tag {
    pub const PRIM: u8 = 0;
    pub const NEVER: u8 = 1;
    pub const POISON: u8 = 2;
    pub const ADT: u8 = 3;
    pub const TUPLE: u8 = 4;
    pub const OPTION: u8 = 5;
    pub const PARAM: u8 = 6;
    pub const MUT: u8 = 7;
    pub const FN: u8 = 8;
    pub const LIST: u8 = 9;
    pub const CANON: u8 = 10;
    pub const TUPLE_REST: u8 = 11;
    pub const TRAIT_VALUE: u8 = 12;
    pub const ASSOC: u8 = 13;
    pub const ROW: u8 = 14;
    pub const ROW_TY: u8 = 15;
    pub const CONTEXT: u8 = 16;
    // 17 was an owned type, `own T`, 18 a borrowed type, `^T`, and 19
    // and 20 a lifetime and a lifetime argument; they are not reused.
    pub const REF: u8 = 21;
    pub const REF_MUT: u8 = 22;
    pub const OPAQUE: u8 = 23;
    pub const CREF: u8 = 24;
    pub const CREF_MUT: u8 = 25;
}

impl<'a> TableWriter<'a> {
    #[must_use]
    pub fn new(pool: &'a InternPool, paths: &'a PathTable, syms: &'a ShardedInterner) -> Self {
        Self {
            pool,
            paths,
            syms,
            path_rows: Vec::new(),
            path_index: HashMap::default(),
            ty_rows: Vec::new(),
            ty_words: Vec::new(),
            ty_index: HashMap::default(),
            list_index: HashMap::default(),
            row_index: HashMap::default(),
            sym_rows: Vec::new(),
            sym_index: HashMap::default(),
        }
    }

    pub fn path(&mut self, p: PathId) -> u32 {
        if p.get().is_none() {
            return NONE;
        }
        if let Some(&r) = self.path_index.get(&p) {
            return r;
        }
        let parent = self.path(self.paths.parent(p));
        let row = u32::try_from(self.path_rows.len()).expect("paths");
        self.path_rows.push((parent, p));
        self.path_index.insert(p, row);
        row
    }

    pub fn def(&mut self, d: DefId) -> u32 {
        self.path(PathId::from_raw(d.raw()))
    }

    pub fn sym(&mut self, s: Symbol) -> u32 {
        if let Some(&r) = self.sym_index.get(&s) {
            return r;
        }
        let row = u32::try_from(self.sym_rows.len()).expect("symbols");
        self.sym_rows.push(s);
        self.sym_index.insert(s, row);
        row
    }

    fn row(&mut self, tag: u8, words: &[u32]) -> u32 {
        let start = self.ty_words.len();
        self.ty_words.extend_from_slice(words);
        self.finish_row(tag, start..self.ty_words.len())
    }

    /// Reserve the parent's words before recursive calls append child rows.
    /// Only the range is kept across recursion; Vec growth cannot invalidate it.
    fn reserve_words(&mut self, len: usize) -> std::ops::Range<usize> {
        let start = self.ty_words.len();
        self.ty_words.resize(start + len, 0);
        start..start + len
    }

    /// Assign the row ID after its dependencies, regardless of word storage order.
    fn finish_row(&mut self, tag: u8, words: std::ops::Range<usize>) -> u32 {
        let row = u32::try_from(self.ty_rows.len()).expect("types");
        self.ty_rows.push((tag, words));
        row
    }

    pub fn list(&mut self, l: TyList) -> StageResult<u32> {
        if let Some(&r) = self.list_index.get(&l.0) {
            return Ok(r);
        }
        let words = self.reserve_words(self.pool.list_items(l).len());
        for (index, t) in self.pool.list_items(l).iter().copied().enumerate() {
            let row = self.ty(t)?;
            self.ty_words[words.start + index] = row;
        }
        let row = self.finish_row(tag::LIST, words);
        self.list_index.insert(l.0, row);
        Ok(row)
    }

    /// A row (§3.4): sorted keys, then row parameters as (owner, index).
    pub fn row_id(&mut self, r: RowId) -> StageResult<u32> {
        if let Some(&x) = self.row_index.get(&r.0) {
            return Ok(x);
        }
        let mut d = self.pool.row_data(r);
        // Content order (`crates/hd_sched/DESIGN.md` §6.5): the pool lists keys by this
        // run's interning order, which must not reach a cache entry. Each
        // key sorts by its own encoding in a fresh table, which holds only
        // stable paths and content.
        let mut keyed: Vec<(Vec<u8>, Ty)> = Vec::with_capacity(d.keys.len());
        for k in &d.keys {
            let mut tmp = TableWriter::new(self.pool, self.paths, self.syms);
            let at = tmp.ty(*k)?;
            let mut w = Writer::default();
            tmp.write(&mut w);
            w.u32(at);
            keyed.push((w.bytes, *k));
        }
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        d.keys = keyed.into_iter().map(|(_, k)| k).collect();
        let paths = self.paths;
        d.params.sort_by_cached_key(|p| {
            (
                paths.display(PathId::from_raw(p.owner.raw())).to_string(),
                p.index,
            )
        });
        let words = self.reserve_words(1 + d.keys.len() + 2 * d.params.len());
        self.ty_words[words.start] = u32::try_from(d.keys.len()).expect("row keys");
        let mut at = words.start + 1;
        for k in &d.keys {
            let row = self.ty(*k)?;
            self.ty_words[at] = row;
            at += 1;
        }
        for p in &d.params {
            let owner = self.def(p.owner);
            self.ty_words[at] = owner;
            self.ty_words[at + 1] = u32::from(p.index);
            at += 2;
        }
        let row = self.finish_row(tag::ROW, words);
        self.row_index.insert(r.0, row);
        Ok(row)
    }

    pub fn ty(&mut self, t: Ty) -> StageResult<u32> {
        if let Some(&r) = self.ty_index.get(&t.0) {
            return Ok(r);
        }
        let row = match self.pool.get(t) {
            TyData::Prim(p) => self.row(tag::PRIM, &[p as u32]),
            TyData::Never => self.row(tag::NEVER, &[]),
            TyData::Poison => self.row(tag::POISON, &[]),
            TyData::Adt { def, args } => {
                let words = [self.def(def), self.list(args)?];
                self.row(tag::ADT, &words)
            }
            TyData::Tuple { elems, rest: None } => {
                let elems = self.list(elems)?;
                self.row(tag::TUPLE, &[elems])
            }
            TyData::Tuple {
                elems,
                rest: Some(r),
            } => {
                let words = [self.list(elems)?, self.ty(r)?];
                self.row(tag::TUPLE_REST, &words)
            }
            TyData::Option(i) => {
                let inner = self.ty(i)?;
                self.row(tag::OPTION, &[inner])
            }
            TyData::Opaque {
                owner,
                site,
                args,
                bound,
            } => {
                let words = [self.def(owner), site, self.list(args)?, self.ty(bound)?];
                self.row(tag::OPAQUE, &words)
            }
            TyData::Param(p) => {
                let owner = self.def(p.owner);
                self.row(tag::PARAM, &[owner, u32::from(p.index)])
            }
            TyData::Mut(i) => {
                let inner = self.ty(i)?;
                self.row(tag::MUT, &[inner])
            }
            TyData::Ref {
                inner,
                mutable,
                counted,
            } => {
                let inner = self.ty(inner)?;
                let t = match (counted, mutable) {
                    (false, false) => tag::REF,
                    (false, true) => tag::REF_MUT,
                    (true, false) => tag::CREF,
                    (true, true) => tag::CREF_MUT,
                };
                self.row(t, &[inner])
            }
            TyData::Fn {
                params,
                result,
                row,
                suspends,
                inputs,
            } => {
                let words = [
                    self.list(params)?,
                    self.ty(result)?,
                    self.row_id(row)?,
                    inputs.word(suspends),
                ];
                self.row(tag::FN, &words)
            }
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => {
                let words = self.reserve_words(2 + 2 * bindings.len());
                let def = self.def(def);
                let args = self.list(args)?;
                self.ty_words[words.start] = def;
                self.ty_words[words.start + 1] = args;
                let mut bindings = bindings;
                let paths = self.paths;
                bindings.sort_by_cached_key(|(d, _)| {
                    paths.display(PathId::from_raw(d.raw())).to_string()
                });
                for (index, (d, b)) in bindings.into_iter().enumerate() {
                    let def = self.def(d);
                    let ty = self.ty(b)?;
                    let at = words.start + 2 + 2 * index;
                    self.ty_words[at] = def;
                    self.ty_words[at + 1] = ty;
                }
                self.finish_row(tag::TRAIT_VALUE, words)
            }
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => {
                let words = [
                    self.def(assoc),
                    self.def(trait_),
                    self.ty(self_ty)?,
                    self.list(args)?,
                ];
                self.row(tag::ASSOC, &words)
            }
            TyData::Canon(i) => self.row(tag::CANON, &[u32::from(i)]),
            TyData::Row(r) => {
                let row = self.row_id(r)?;
                self.row(tag::ROW_TY, &[row])
            }
            TyData::Context(r) => {
                let row = self.row_id(r)?;
                self.row(tag::CONTEXT, &[row])
            }
            other @ TyData::Infer(_) => {
                return Err(NotImplemented::new(
                    Stage::ModuleFinish,
                    format!("entry-local type row for {other:?}"),
                ));
            }
        };
        self.ty_index.insert(t.0, row);
        Ok(row)
    }

    /// Writes the tables: paths, symbols, then type rows.
    pub fn write(&self, w: &mut Writer) {
        w.len_of(&self.path_rows);
        for (parent, path) in &self.path_rows {
            w.u32(*parent);
            w.u8(self.paths.kind(*path) as u8);
            w.str(self.paths.segment(*path));
        }
        w.len_of(&self.sym_rows);
        for s in &self.sym_rows {
            w.str(self.syms.resolve(*s));
        }
        w.len_of(&self.ty_rows);
        for (t, range) in &self.ty_rows {
            let words = &self.ty_words[range.clone()];
            w.u8(*t);
            w.len_of(words);
            for x in words {
                w.u32(*x);
            }
        }
    }
}

/// A blob's rows re-interned into this run.
#[derive(Default, Debug)]
pub struct Tables {
    pub paths: Vec<PathId>,
    pub syms: Vec<Symbol>,
    /// Type rows: a type, or a list for `LIST` rows.
    pub rows: Vec<u32>,
}

impl Tables {
    /// Reads the tables; `None` when a row is malformed (a cache miss).
    pub fn read(
        r: &mut Reader<'_>,
        pool: &InternPool,
        paths: &PathTable,
        syms: &ShardedInterner,
    ) -> Option<Tables> {
        let mut t = Tables::default();
        for _ in 0..r.count() {
            let parent = r.u32();
            let kind = PathKind::from_u8(r.u8())?;
            let seg = r.str();
            let parent = if parent == NONE {
                PathId::NONE
            } else {
                *t.paths.get(parent as usize)?
            };
            t.paths.push(paths.intern(parent, kind, seg));
        }
        for _ in 0..r.count() {
            t.syms.push(syms.intern(r.str()));
        }
        for _ in 0..r.count() {
            let tg = r.u8();
            let n = r.count();
            let words: Vec<u32> = (0..n).map(|_| r.u32()).collect();
            let row = |i: usize| -> Option<u32> { t.rows.get(*words.get(i)? as usize).copied() };
            let def = |i: usize| -> Option<DefId> {
                Some(DefId::from_raw(t.paths.get(*words.get(i)? as usize)?.raw()))
            };
            let v = match tg {
                tag::PRIM => {
                    pool.intern_ty(&TyData::Prim(*Prim::ALL.get(*words.first()? as usize)?))
                        .0
                }
                tag::NEVER => Ty::NEVER.0,
                tag::POISON => Ty::POISON.0,
                tag::ADT => {
                    pool.intern_ty(&TyData::Adt {
                        def: def(0)?,
                        args: TyList(row(1)?),
                    })
                    .0
                }
                tag::TUPLE => {
                    pool.intern_ty(&TyData::Tuple {
                        elems: TyList(row(0)?),
                        rest: None,
                    })
                    .0
                }
                tag::OPTION => pool.intern_ty(&TyData::Option(Ty(row(0)?))).0,
                tag::OPAQUE => {
                    pool.intern_ty(&TyData::Opaque {
                        owner: def(0)?,
                        site: *words.get(1)?,
                        args: TyList(row(2)?),
                        bound: Ty(row(3)?),
                    })
                    .0
                }
                tag::PARAM => {
                    let index = u16::try_from(*words.get(1)?).ok()?;
                    pool.intern_ty(&TyData::Param(ParamRef {
                        owner: def(0)?,
                        index,
                    }))
                    .0
                }
                tag::MUT => pool.intern_ty(&TyData::Mut(Ty(row(0)?))).0,
                tag::REF | tag::REF_MUT | tag::CREF | tag::CREF_MUT => {
                    pool.intern_ty(&TyData::Ref {
                        inner: Ty(row(0)?),
                        mutable: tg == tag::REF_MUT || tg == tag::CREF_MUT,
                        counted: tg == tag::CREF || tg == tag::CREF_MUT,
                    })
                    .0
                }
                tag::FN => {
                    pool.intern_ty(&TyData::Fn {
                        params: TyList(row(0)?),
                        result: Ty(row(1)?),
                        row: RowId(row(2)?),
                        suspends: *words.get(3)? & 1 != 0,
                        inputs: Inputs::from_bits(*words.get(3)?),
                    })
                    .0
                }
                tag::TUPLE_REST => {
                    pool.intern_ty(&TyData::Tuple {
                        elems: TyList(row(0)?),
                        rest: Some(Ty(row(1)?)),
                    })
                    .0
                }
                tag::TRAIT_VALUE => {
                    let mut bindings = Vec::new();
                    let mut i = 2;
                    while i + 1 < words.len() {
                        bindings.push((def(i)?, Ty(row(i + 1)?)));
                        i += 2;
                    }
                    pool.intern_ty(&TyData::TraitValue {
                        def: def(0)?,
                        args: TyList(row(1)?),
                        bindings,
                    })
                    .0
                }
                tag::ASSOC => {
                    pool.intern_ty(&TyData::Assoc {
                        assoc: def(0)?,
                        trait_: def(1)?,
                        self_ty: Ty(row(2)?),
                        args: TyList(row(3)?),
                    })
                    .0
                }
                tag::ROW => {
                    let n = *words.first()? as usize;
                    let keys: Option<Vec<Ty>> = (1..=n).map(|i| row(i).map(Ty)).collect();
                    let mut params = Vec::new();
                    let mut i = n + 1;
                    while i + 1 < words.len() {
                        params.push(RowParamRef {
                            owner: def(i)?,
                            index: u16::try_from(*words.get(i + 1)?).ok()?,
                        });
                        i += 2;
                    }
                    pool.row(&RowData {
                        keys: keys?,
                        params,
                    })
                    .0
                }
                tag::LIST => {
                    let items: Option<Vec<Ty>> = (0..words.len()).map(|i| row(i).map(Ty)).collect();
                    pool.list(&items?).0
                }
                tag::CANON => {
                    pool.intern_ty(&TyData::Canon(u8::try_from(*words.first()?).ok()?))
                        .0
                }
                tag::ROW_TY => pool.intern_ty(&TyData::Row(RowId(row(0)?))).0,
                tag::CONTEXT => pool.intern_ty(&TyData::Context(RowId(row(0)?))).0,
                _ => return None,
            };
            t.rows.push(v);
        }
        r.ok().then_some(t)
    }

    #[must_use]
    pub fn def(&self, row: u32) -> Option<DefId> {
        self.paths
            .get(row as usize)
            .map(|p| DefId::from_raw(p.raw()))
    }
    #[must_use]
    pub fn ty(&self, row: u32) -> Option<Ty> {
        self.rows.get(row as usize).map(|&x| Ty(x))
    }
    #[must_use]
    pub fn list(&self, row: u32) -> Option<TyList> {
        self.rows.get(row as usize).map(|&x| TyList(x))
    }
    #[must_use]
    pub fn sym(&self, row: u32) -> Option<Symbol> {
        self.syms.get(row as usize).copied()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn opaque_cache_rows_reintern_declaration_and_arguments() {
        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let owner = paths.item("app", "values", "make");
        let bound = pool.intern_ty(&TyData::TraitValue {
            def: paths.item("app", "values", "View"),
            args: pool.list(&[Ty::I32]),
            bindings: vec![],
        });
        let opaque = pool.intern_ty(&TyData::Opaque {
            owner,
            site: 17,
            args: pool.list(&[Ty::I32, Ty::STRING]),
            bound,
        });
        let mut tables = TableWriter::new(&pool, &paths, &syms);
        let row = tables.ty(opaque).expect("opaque row");
        let mut bytes = Writer::default();
        tables.write(&mut bytes);
        let (other, other_paths, other_syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        other_paths.item("unrelated", "earlier", "Path");
        let decoded = Tables::read(
            &mut Reader::new(&bytes.bytes),
            &other,
            &other_paths,
            &other_syms,
        )
        .expect("tables");
        let decoded_ty = decoded.ty(row).expect("opaque type");
        let TyData::Opaque {
            owner: decoded_owner,
            site,
            args,
            bound: decoded_bound,
        } = other.get(decoded_ty)
        else {
            panic!("lost opaque identity");
        };
        assert_eq!(decoded_owner, other_paths.item("app", "values", "make"));
        assert_eq!(site, 17);
        assert_eq!(other.list_items(args), &[Ty::I32, Ty::STRING]);
        assert!(
            matches!(other.get(decoded_bound), TyData::TraitValue { def, .. } if def == other_paths.item("app", "values", "View"))
        );
        let mut rewritten = TableWriter::new(&other, &other_paths, &other_syms);
        assert_eq!(rewritten.ty(decoded_ty).expect("rewritten opaque"), row);
        let mut output = Writer::default();
        rewritten.write(&mut output);
        assert_eq!(output.bytes, bytes.bytes);
    }

    use super::{TableWriter, Tables};
    use crate::pool::{InternPool, ParamRef, RowData, Ty, TyData};
    use hd_base::wire::{Reader, Writer};
    use hd_intern::{PathTable, ShardedInterner};

    /// A row of two trait keys, written after interning the traits in the
    /// given order: the bytes depend on content only.
    fn row_bytes(flip: bool) -> Vec<u8> {
        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let names = if flip {
            ["Tag", "Clock"]
        } else {
            ["Clock", "Tag"]
        };
        let keys: Vec<Ty> = names
            .iter()
            .map(|n| {
                pool.intern_ty(&TyData::TraitValue {
                    def: paths.item("app", "keys", n),
                    args: crate::pool::TyList::EMPTY,
                    bindings: vec![],
                })
            })
            .collect();
        let row = pool.row(&RowData {
            keys,
            params: vec![],
        });
        let mut w = TableWriter::new(&pool, &paths, &syms);
        w.row_id(row).expect("row");
        let mut out = Writer::default();
        w.write(&mut out);
        out.bytes
    }

    #[test]
    fn row_bytes_ignore_interning_order() {
        assert_eq!(row_bytes(false), row_bytes(true));
    }

    #[test]
    fn nested_rows_keep_the_existing_wire_bytes() {
        use crate::pool::{Inputs, RowParamRef, TyList};
        use hd_base::{DefId, PathId};
        use hd_intern::PathKind;

        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let point = paths.item("app", "geo", "Point");
        let bound = paths.item("app", "geo", "Bound");
        let member = |name| {
            DefId::from_raw(
                paths
                    .intern(PathId::from_raw(bound.raw()), PathKind::Member, name)
                    .raw(),
            )
        };
        let (zulu, alpha) = (member("Zulu"), member("Alpha"));
        let symbol = syms.intern("noteλ");
        let optional = pool.intern_ty(&TyData::Option(Ty::I32));
        let adt = pool.intern_ty(&TyData::Adt {
            def: point,
            args: pool.list(&[optional, Ty::BOOL]),
        });
        let row = pool.row(&RowData {
            keys: vec![Ty::BOOL, Ty::I32],
            params: vec![RowParamRef {
                owner: point,
                index: 2,
            }],
        });
        let result = pool.intern_ty(&TyData::Tuple {
            elems: pool.list(&[Ty::I32]),
            rest: Some(adt),
        });
        let function = pool.intern_ty(&TyData::Fn {
            params: pool.list(&[adt, optional]),
            result,
            row,
            suspends: true,
            inputs: Inputs::Fixed,
        });
        let trait_value = pool.intern_ty(&TyData::TraitValue {
            def: bound,
            args: TyList::EMPTY,
            bindings: vec![(zulu, Ty::BOOL), (alpha, optional)],
        });
        let mut tables = TableWriter::new(&pool, &paths, &syms);
        assert_eq!(tables.sym(symbol), 0);
        assert_eq!(tables.ty(adt).expect("adt"), 4);
        assert_eq!(tables.row_id(row).expect("row"), 5);
        assert_eq!(tables.ty(function).expect("function"), 9);
        assert_eq!(tables.ty(trait_value).expect("trait"), 11);
        let mut actual = Writer::default();
        tables.write(&mut actual);

        // Hand-specified legacy format: path records, symbols, then tagged
        // length-prefixed type words. No in-run ID or flat storage offset is written.
        let mut expected = Writer::default();
        expected.u32(6);
        for (parent, kind, segment) in [
            (u32::MAX, 0, "app"),
            (0, 1, "geo"),
            (1, 2, "Point"),
            (1, 2, "Bound"),
            (3, 3, "Alpha"),
            (3, 3, "Zulu"),
        ] {
            expected.u32(parent);
            expected.u8(kind);
            expected.str(segment);
        }
        expected.u32(1);
        expected.str("noteλ");
        let rows: &[(u8, &[u32])] = &[
            (0, &[2]),                  // i32
            (5, &[0]),                  // i32?
            (0, &[11]),                 // bool
            (9, &[1, 2]),               // type list
            (3, &[2, 3]),               // Point[i32?, bool]
            (14, &[2, 0, 2, 2, 2]),     // sorted keys and row parameter
            (9, &[4, 1]),               // function inputs
            (9, &[0]),                  // tuple's fixed prefix
            (11, &[7, 4]),              // tuple with a rest
            (8, &[6, 8, 5, 1]),         // suspending function
            (9, &[]),                   // empty trait arguments
            (12, &[3, 10, 4, 1, 5, 2]), // bindings sorted Alpha, Zulu
        ];
        expected.len_of(rows);
        for (tag, words) in rows {
            expected.u8(*tag);
            expected.len_of(words);
            for word in *words {
                expected.u32(*word);
            }
        }
        assert_eq!(actual.bytes, expected.bytes);

        let (pool2, paths2, syms2) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        paths2.item("other", "primed", "Different");
        syms2.intern("other");
        let mut reader = Reader::new(&actual.bytes);
        let decoded = Tables::read(&mut reader, &pool2, &paths2, &syms2).expect("tables");
        assert!(reader.ok() && reader.at_end());
        assert_eq!(syms2.resolve(decoded.sym(0).expect("symbol")), "noteλ");
        let TyData::Fn {
            row: decoded_row, ..
        } = pool2.get(decoded.ty(9).expect("function"))
        else {
            panic!("function row");
        };
        let mut rewritten = TableWriter::new(&pool2, &paths2, &syms2);
        rewritten.sym(decoded.sym(0).expect("symbol"));
        rewritten.ty(decoded.ty(4).expect("adt")).expect("adt");
        rewritten.row_id(decoded_row).expect("row");
        rewritten
            .ty(decoded.ty(9).expect("function"))
            .expect("function");
        rewritten.ty(decoded.ty(11).expect("trait")).expect("trait");
        let mut output = Writer::default();
        rewritten.write(&mut output);
        assert_eq!(
            output.bytes, actual.bytes,
            "fresh IDs retain the exact format"
        );
    }

    #[test]
    fn rows_survive_a_fresh_run() {
        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let point = paths.item("app", "geo.shapes", "Point");
        let adt = pool.intern_ty(&TyData::Adt {
            def: point,
            args: pool.list(&[Ty::I32]),
        });
        let param = pool.intern_ty(&TyData::Param(ParamRef {
            owner: point,
            index: 1,
        }));
        let mut w = TableWriter::new(&pool, &paths, &syms);
        let (a, p) = (w.ty(adt).expect("adt"), w.ty(param).expect("param"));
        let mut out = Writer::default();
        w.write(&mut out);
        let (pool2, paths2, syms2) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        paths2.item("other", "x", "Y");
        let t =
            Tables::read(&mut Reader::new(&out.bytes), &pool2, &paths2, &syms2).expect("tables");
        let point2 = paths2.item("app", "geo.shapes", "Point");
        assert_eq!(
            pool2.get(t.ty(a).expect("a")),
            TyData::Adt {
                def: point2,
                args: pool2.list(&[Ty::I32])
            }
        );
        assert_eq!(
            pool2.get(t.ty(p).expect("p")),
            TyData::Param(ParamRef {
                owner: point2,
                index: 1
            })
        );
    }
}
