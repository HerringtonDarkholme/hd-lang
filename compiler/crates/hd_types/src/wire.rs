//! Entry-local tables (data-structures.md §3.20.2): run IDs never reach a
//! cache entry. A writer collects the stable paths, types, type lists and
//! symbols one blob mentions and gives each a row; the reader re-interns
//! the rows into this run's tables. Rows refer only to earlier rows.

use std::collections::HashMap;

use hd_base::wire::{Reader, Writer};
use hd_base::{DefId, NotImplemented, PathId, Stage, StageResult, Symbol};
use hd_intern::{PathKind, PathTable, ShardedInterner};

use crate::pool::{InternPool, ParamRef, Prim, RowData, RowId, RowParamRef, Ty, TyData, TyList};

const NONE: u32 = u32::MAX;

/// Collects the rows of one blob's tables.
pub struct TableWriter<'a> {
    pool: &'a InternPool,
    paths: &'a PathTable,
    syms: &'a ShardedInterner,
    path_rows: Vec<(u32, u8, String)>,
    path_index: HashMap<PathId, u32>,
    ty_rows: Vec<(u8, Vec<u32>)>,
    ty_index: HashMap<u32, u32>,
    list_index: HashMap<u32, u32>,
    row_index: HashMap<u32, u32>,
    sym_rows: Vec<String>,
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
}

impl<'a> TableWriter<'a> {
    #[must_use]
    pub fn new(pool: &'a InternPool, paths: &'a PathTable, syms: &'a ShardedInterner) -> Self {
        Self {
            pool,
            paths,
            syms,
            path_rows: Vec::new(),
            path_index: HashMap::new(),
            ty_rows: Vec::new(),
            ty_index: HashMap::new(),
            list_index: HashMap::new(),
            row_index: HashMap::new(),
            sym_rows: Vec::new(),
            sym_index: HashMap::new(),
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
        self.path_rows.push((
            parent,
            self.paths.kind(p) as u8,
            self.paths.segment(p).to_owned(),
        ));
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
        self.sym_rows.push(self.syms.resolve(s).to_owned());
        self.sym_index.insert(s, row);
        row
    }

    fn row(&mut self, tag: u8, words: Vec<u32>) -> u32 {
        let row = u32::try_from(self.ty_rows.len()).expect("types");
        self.ty_rows.push((tag, words));
        row
    }

    pub fn list(&mut self, l: TyList) -> StageResult<u32> {
        if let Some(&r) = self.list_index.get(&l.0) {
            return Ok(r);
        }
        let mut words = Vec::new();
        for t in self.pool.list_items(l).iter().copied() {
            words.push(self.ty(t)?);
        }
        let row = self.row(tag::LIST, words);
        self.list_index.insert(l.0, row);
        Ok(row)
    }

    /// A row (§3.4): sorted keys, then row parameters as (owner, index).
    pub fn row_id(&mut self, r: RowId) -> StageResult<u32> {
        if let Some(&x) = self.row_index.get(&r.0) {
            return Ok(x);
        }
        let mut d = self.pool.row_data(r);
        // Content order (scheduler.md §6.5): the pool lists keys by this
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
        let mut words = vec![u32::try_from(d.keys.len()).expect("row keys")];
        for k in &d.keys {
            words.push(self.ty(*k)?);
        }
        for p in &d.params {
            words.push(self.def(p.owner));
            words.push(u32::from(p.index));
        }
        let row = self.row(tag::ROW, words);
        self.row_index.insert(r.0, row);
        Ok(row)
    }

    pub fn ty(&mut self, t: Ty) -> StageResult<u32> {
        if let Some(&r) = self.ty_index.get(&t.0) {
            return Ok(r);
        }
        let (tag, words) = match self.pool.get(t) {
            TyData::Prim(p) => (tag::PRIM, vec![p as u32]),
            TyData::Never => (tag::NEVER, vec![]),
            TyData::Poison => (tag::POISON, vec![]),
            TyData::Adt { def, args } => (tag::ADT, vec![self.def(def), self.list(args)?]),
            TyData::Tuple { elems, rest: None } => (tag::TUPLE, vec![self.list(elems)?]),
            TyData::Tuple {
                elems,
                rest: Some(r),
            } => (tag::TUPLE_REST, vec![self.list(elems)?, self.ty(r)?]),
            TyData::Option(i) => (tag::OPTION, vec![self.ty(i)?]),
            TyData::Param(p) => (tag::PARAM, vec![self.def(p.owner), u32::from(p.index)]),
            TyData::Mut(i) => (tag::MUT, vec![self.ty(i)?]),
            TyData::Fn {
                params,
                result,
                row,
                suspends,
            } => (
                tag::FN,
                vec![
                    self.list(params)?,
                    self.ty(result)?,
                    self.row_id(row)?,
                    u32::from(suspends),
                ],
            ),
            TyData::TraitValue {
                def,
                args,
                bindings,
            } => {
                let mut w = vec![self.def(def), self.list(args)?];
                let mut bindings = bindings;
                let paths = self.paths;
                bindings.sort_by_cached_key(|(d, _)| {
                    paths.display(PathId::from_raw(d.raw())).to_string()
                });
                for (d, b) in bindings {
                    w.push(self.def(d));
                    w.push(self.ty(b)?);
                }
                (tag::TRAIT_VALUE, w)
            }
            TyData::Assoc {
                assoc,
                trait_,
                self_ty,
                args,
            } => (
                tag::ASSOC,
                vec![
                    self.def(assoc),
                    self.def(trait_),
                    self.ty(self_ty)?,
                    self.list(args)?,
                ],
            ),
            TyData::Canon(i) => (tag::CANON, vec![u32::from(i)]),
            TyData::Row(r) => (tag::ROW_TY, vec![self.row_id(r)?]),
            other @ TyData::Infer(_) => {
                return Err(NotImplemented::new(
                    Stage::ModuleFinish,
                    format!("entry-local type row for {other:?}"),
                ));
            }
        };
        let row = self.row(tag, words);
        self.ty_index.insert(t.0, row);
        Ok(row)
    }

    /// Writes the tables: paths, symbols, then type rows.
    pub fn write(&self, w: &mut Writer) {
        w.len_of(&self.path_rows);
        for (parent, kind, seg) in &self.path_rows {
            w.u32(*parent);
            w.u8(*kind);
            w.str(seg);
        }
        w.len_of(&self.sym_rows);
        for s in &self.sym_rows {
            w.str(s);
        }
        w.len_of(&self.ty_rows);
        for (t, words) in &self.ty_rows {
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
                tag::PARAM => {
                    let index = u16::try_from(*words.get(1)?).ok()?;
                    pool.intern_ty(&TyData::Param(ParamRef {
                        owner: def(0)?,
                        index,
                    }))
                    .0
                }
                tag::MUT => pool.intern_ty(&TyData::Mut(Ty(row(0)?))).0,
                tag::FN => {
                    pool.intern_ty(&TyData::Fn {
                        params: TyList(row(0)?),
                        result: Ty(row(1)?),
                        row: RowId(row(2)?),
                        suspends: *words.get(3)? != 0,
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
