//! Header lowering, name resolution and the folder interface
//! (resolution-and-interfaces.md §4.9 to §4.11), simplest forms.

use std::collections::{BTreeSet, HashMap};

use hd_base::{Hash128, NodeIdx, TokenIdx};
use hd_syntax::subset::SubsetParse;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};

use crate::world::{CTy, DefKind, FnSig, KeyHasher, Reader, World, put_hash, put_str, put_u32};

// ---------------------------------------------------------------- CST views

/// Typed-view helpers over the green tree. The green tree keeps only a node's
/// first and last token, so names and operators are found by token position.
pub struct Cst<'a> {
    pub src: &'a str,
    pub p: &'a SubsetParse,
}

impl<'a> Cst<'a> {
    pub fn root(&self) -> NodeRef<'a> {
        self.p.tree.root()
    }
    pub fn node(&self, i: NodeIdx) -> NodeRef<'a> {
        self.p.tree.node(i)
    }
    pub fn first(&self, n: NodeRef<'_>) -> TokenIdx {
        self.p.tree.span_tokens(n.index()).0
    }
    pub fn last(&self, n: NodeRef<'_>) -> TokenIdx {
        self.p.tree.span_tokens(n.index()).1
    }
    pub fn text(&self, t: TokenIdx) -> &'a str {
        self.p.tokens.text(t, self.src)
    }
    pub fn tkind(&self, t: TokenIdx) -> TokenKind {
        self.p.tokens.kind(t)
    }
    pub fn next(t: TokenIdx) -> TokenIdx {
        TokenIdx::from_raw(t.raw() + 1)
    }
    /// The first identifier token in the node's own range.
    pub fn first_ident(&self, n: NodeRef<'_>) -> &'a str {
        let (lo, hi) = self.p.tree.span_tokens(n.index());
        (lo.raw()..=hi.raw())
            .map(TokenIdx::from_raw)
            .find(|&t| self.tkind(t) == TokenKind::Ident)
            .map_or("", |t| self.text(t))
    }
    pub fn is_pub(&self, n: NodeRef<'_>) -> bool {
        self.tkind(self.first(n)) == TokenKind::KwPub
    }
    pub fn children(n: NodeRef<'a>) -> Vec<NodeRef<'a>> {
        n.children().collect()
    }
    pub fn child(n: NodeRef<'a>, kind: SyntaxKind) -> Option<NodeRef<'a>> {
        n.children().find(|c| c.kind() == kind)
    }
    pub fn span(&self, n: NodeRef<'_>) -> (u32, u32) {
        let (lo, hi) = self.p.tree.span_tokens(n.index());
        (self.p.tokens.span(lo).0, self.p.tokens.span(hi).1)
    }
}

// ------------------------------------------------------------- header items

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CSig {
    pub generics: Vec<(String, Option<String>)>,
    pub params: Vec<(String, CTy)>,
    pub ret: CTy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CItem {
    Fn(CSig),
    Data(Vec<(String, CTy)>),
    Trait(Vec<(String, CSig)>),
    Impl { trait_: String, target: CTy, methods: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderItem {
    pub path: String,
    pub public: bool,
    pub item: CItem,
}

/// Stable path of item `name` in module `module`.
pub fn item_path(module: &str, name: &str) -> String {
    format!("{module}::{name}")
}
pub fn module_of(path: &str) -> &str {
    path.split("::").next().unwrap_or(path)
}
pub fn folder_of_module(module: &str) -> &str {
    module.rsplit_once('.').map_or(module, |(f, _)| f)
}
pub fn folder_of(path: &str) -> &str {
    folder_of_module(module_of(path))
}

/// The module scope (§4.9): name to stable path.
#[derive(Default, Clone, Debug)]
pub struct Scope {
    pub names: HashMap<String, String>,
    pub errors: Vec<String>,
}

/// `use a.b.c.{X, y}`: the module path and the imported names.
pub fn use_decls(cst: &Cst<'_>) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for item in cst.root().children() {
        if item.kind() != SyntaxKind::UseDecl {
            continue;
        }
        let (lo, hi) = cst.p.tree.span_tokens(item.index());
        let mut path = Vec::new();
        let mut names = Vec::new();
        let mut in_braces = false;
        for t in (lo.raw() + 1..=hi.raw()).map(TokenIdx::from_raw) {
            match cst.tkind(t) {
                TokenKind::LBrace => in_braces = true,
                TokenKind::Ident if in_braces => names.push(cst.text(t).to_owned()),
                TokenKind::Ident => path.push(cst.text(t).to_owned()),
                _ => {}
            }
        }
        out.push((path.join("."), names));
    }
    out
}

/// Builds the module scope: own items, then imports resolved against the
/// frozen interfaces of the used folders.
pub fn module_scope(
    cst: &Cst<'_>,
    module: &str,
    ifaces: &HashMap<String, FolderIface>,
) -> Scope {
    let mut scope = Scope::default();
    for item in cst.root().children() {
        match item.kind() {
            SyntaxKind::FnDecl | SyntaxKind::DataDecl | SyntaxKind::TraitDecl => {
                let name = cst.first_ident(item);
                scope.names.insert(name.to_owned(), item_path(module, name));
            }
            _ => {}
        }
    }
    for (used, names) in use_decls(cst) {
        let folder = folder_of_module(&used);
        let Some(iface) = ifaces.get(folder) else {
            scope.errors.push(format!("unknown-import: folder `{folder}`"));
            continue;
        };
        for name in names {
            let path = item_path(&used, &name);
            if iface.items.iter().any(|i| i.path == path && i.public) {
                scope.names.insert(name, path);
            } else {
                scope.errors.push(format!("unknown-import: `{name}` in `{used}`"));
            }
        }
    }
    scope
}

struct Lower<'s> {
    scope: &'s Scope,
    generics: Vec<String>,
    errors: Vec<String>,
}

impl Lower<'_> {
    fn ty(&mut self, cst: &Cst<'_>, n: Option<NodeRef<'_>>) -> CTy {
        let Some(n) = n else { return CTy::Void };
        let t = cst.first(n);
        if cst.tkind(t) == TokenKind::KwSelfType {
            return CTy::SelfTy;
        }
        let name = cst.text(t);
        match name {
            "i32" => CTy::I32,
            "bool" => CTy::Bool,
            "void" => CTy::Void,
            _ => {
                if let Some(i) = self.generics.iter().position(|g| g == name) {
                    return CTy::Param(u32::try_from(i).expect("generics"));
                }
                if let Some(p) = self.scope.names.get(name) {
                    return CTy::Adt(p.clone());
                }
                self.errors.push(format!("unknown-type `{name}`"));
                CTy::Void
            }
        }
    }

    fn sig(&mut self, cst: &Cst<'_>, f: NodeRef<'_>, self_ty: Option<&CTy>) -> CSig {
        let mut generics = Vec::new();
        self.generics.clear();
        if let Some(gl) = Cst::child(f, SyntaxKind::GenericParameterList) {
            for g in gl.children() {
                self.generics.push(cst.first_ident(g).to_owned());
            }
            for g in gl.children() {
                let bound = Cst::child(g, SyntaxKind::NamedType).map(|b| {
                    let name = cst.text(cst.first(b));
                    self.scope.names.get(name).cloned().unwrap_or_else(|| {
                        self.errors.push(format!("unknown-trait `{name}`"));
                        String::new()
                    })
                });
                generics.push((cst.first_ident(g).to_owned(), bound));
            }
        }
        let mut params = Vec::new();
        if let Some(pl) = Cst::child(f, SyntaxKind::ParameterList) {
            for p in pl.children() {
                if cst.tkind(cst.first(p)) == TokenKind::KwSelfValue {
                    params.push(("self".to_owned(), self_ty.cloned().unwrap_or(CTy::SelfTy)));
                } else {
                    let ty = self.ty(cst, Cst::child(p, SyntaxKind::NamedType));
                    params.push((cst.first_ident(p).to_owned(), ty));
                }
            }
        }
        let ret_node = f.children().find(|c| c.kind() == SyntaxKind::NamedType);
        let ret = self.ty(cst, ret_node);
        CSig { generics, params, ret }
    }
}

/// Lowers every header of a module to content items (§4.10 step 3). Private
/// items are included; the folder interface keeps the public ones and impls.
pub fn lower_headers(cst: &Cst<'_>, module: &str, scope: &Scope) -> (Vec<HeaderItem>, Vec<String>) {
    let mut low = Lower { scope, generics: Vec::new(), errors: Vec::new() };
    let mut items = Vec::new();
    for item in cst.root().children() {
        let public = cst.is_pub(item);
        match item.kind() {
            SyntaxKind::FnDecl => {
                let sig = low.sig(cst, item, None);
                items.push(HeaderItem {
                    path: item_path(module, cst.first_ident(item)),
                    public,
                    item: CItem::Fn(sig),
                });
            }
            SyntaxKind::DataDecl => {
                low.generics.clear();
                let fields = item
                    .children()
                    .filter(|c| c.kind() == SyntaxKind::DataField)
                    .map(|f| {
                        let ty = low.ty(cst, Cst::child(f, SyntaxKind::NamedType));
                        (cst.first_ident(f).to_owned(), ty)
                    })
                    .collect();
                items.push(HeaderItem {
                    path: item_path(module, cst.first_ident(item)),
                    public,
                    item: CItem::Data(fields),
                });
            }
            SyntaxKind::TraitDecl => {
                let methods = item
                    .children()
                    .filter(|c| c.kind() == SyntaxKind::FnDecl)
                    .map(|f| (cst.first_ident(f).to_owned(), low.sig(cst, f, None)))
                    .collect();
                items.push(HeaderItem {
                    path: item_path(module, cst.first_ident(item)),
                    public,
                    item: CItem::Trait(methods),
                });
            }
            SyntaxKind::ImplDecl => {
                low.generics.clear();
                let tys: Vec<_> =
                    item.children().filter(|c| c.kind() == SyntaxKind::NamedType).collect();
                if tys.len() != 2 {
                    low.errors.push("inherent impls are outside the skeleton subset".into());
                    continue;
                }
                let trait_name = cst.text(cst.first(tys[0]));
                let trait_ = scope.names.get(trait_name).cloned().unwrap_or_default();
                let target = low.ty(cst, Some(tys[1]));
                let methods = item
                    .children()
                    .filter(|c| c.kind() == SyntaxKind::FnDecl)
                    .map(|f| cst.first_ident(f).to_owned())
                    .collect();
                let target_name = match &target {
                    CTy::Adt(p) => p.rsplit("::").next().unwrap_or("").to_owned(),
                    other => format!("{other:?}"),
                };
                let tname = trait_.rsplit("::").next().unwrap_or("").to_owned();
                items.push(HeaderItem {
                    path: item_path(module, &format!("impl.{tname}.{target_name}")),
                    public: true,
                    item: CItem::Impl { trait_, target, methods },
                });
            }
            _ => {}
        }
    }
    (items, low.errors)
}

// ------------------------------------------------------------------- blob

fn put_sig(out: &mut Vec<u8>, s: &CSig) {
    put_u32(out, u32::try_from(s.generics.len()).expect("n"));
    for (g, b) in &s.generics {
        put_str(out, g);
        put_str(out, b.as_deref().unwrap_or(""));
    }
    put_u32(out, u32::try_from(s.params.len()).expect("n"));
    for (p, t) in &s.params {
        put_str(out, p);
        t.encode(out);
    }
    s.ret.encode(out);
}
fn get_sig(r: &mut Reader<'_>) -> CSig {
    let ng = r.u32();
    let generics = (0..ng)
        .map(|_| {
            let g = r.str();
            let b = r.str();
            (g, if b.is_empty() { None } else { Some(b) })
        })
        .collect();
    let np = r.u32();
    let params = (0..np).map(|_| (r.str(), CTy::decode(r))).collect();
    CSig { generics, params, ret: CTy::decode(r) }
}

pub fn encode_item(out: &mut Vec<u8>, h: &HeaderItem) {
    put_str(out, &h.path);
    out.push(u8::from(h.public));
    match &h.item {
        CItem::Fn(s) => {
            out.push(0);
            put_sig(out, s);
        }
        CItem::Data(fields) => {
            out.push(1);
            put_u32(out, u32::try_from(fields.len()).expect("n"));
            for (f, t) in fields {
                put_str(out, f);
                t.encode(out);
            }
        }
        CItem::Trait(ms) => {
            out.push(2);
            put_u32(out, u32::try_from(ms.len()).expect("n"));
            for (m, s) in ms {
                put_str(out, m);
                put_sig(out, s);
            }
        }
        CItem::Impl { trait_, target, methods } => {
            out.push(3);
            put_str(out, trait_);
            target.encode(out);
            put_u32(out, u32::try_from(methods.len()).expect("n"));
            for m in methods {
                put_str(out, m);
            }
        }
    }
}
pub fn decode_item(r: &mut Reader<'_>) -> HeaderItem {
    let path = r.str();
    let public = r.u8() != 0;
    let item = match r.u8() {
        0 => CItem::Fn(get_sig(r)),
        1 => {
            let n = r.u32();
            CItem::Data((0..n).map(|_| (r.str(), CTy::decode(r))).collect())
        }
        2 => {
            let n = r.u32();
            CItem::Trait((0..n).map(|_| (r.str(), get_sig(r))).collect())
        }
        _ => {
            let trait_ = r.str();
            let target = CTy::decode(r);
            let n = r.u32();
            CItem::Impl { trait_, target, methods: (0..n).map(|_| r.str()).collect() }
        }
    };
    HeaderItem { path, public, item }
}

/// A folder interface (§4.10): the blob is the canonical form; `items` is a
/// decoded view of it (the design reads the blob in place).
#[derive(Clone, Debug)]
pub struct FolderIface {
    pub folder: String,
    pub blob: Vec<u8>,
    pub items: Vec<HeaderItem>,
    pub item_hashes: Vec<Hash128>,
    pub api_hash: Hash128,
    pub deep_hash: Hash128,
}

/// The blob: `deep_hash` first (it needs the dependencies' deep hashes, so it
/// is stored, not recomputed by a reader), then every interface item.
pub fn build_iface(
    folder: &str,
    items: Vec<HeaderItem>,
    deps: &HashMap<String, FolderIface>,
) -> FolderIface {
    let mut api = Vec::new();
    for h in &items {
        encode_item(&mut api, h);
    }
    let api_hash = KeyHasher::new("api").bytes(&api).finish();
    // mentions(F): folders whose items F's api names (§4.11.3).
    let mut mentions = BTreeSet::new();
    let mut note = |c: &CTy| {
        if let CTy::Adt(p) = c {
            mentions.insert(folder_of(p).to_owned());
        }
    };
    for h in &items {
        match &h.item {
            CItem::Fn(s) => {
                s.params.iter().for_each(|p| note(&p.1));
                note(&s.ret);
                for (_, b) in &s.generics {
                    if let Some(b) = b {
                        note(&CTy::Adt(b.clone()));
                    }
                }
            }
            CItem::Data(f) => f.iter().for_each(|p| note(&p.1)),
            CItem::Trait(ms) => ms.iter().for_each(|(_, s)| {
                s.params.iter().for_each(|p| note(&p.1));
                note(&s.ret);
            }),
            CItem::Impl { trait_, target, .. } => {
                note(&CTy::Adt(trait_.clone()));
                note(target);
            }
        }
    }
    mentions.remove(folder);
    let mut deep = KeyHasher::new("deep").hash(api_hash);
    for m in &mentions {
        let d = deps.get(m).map_or(Hash128(0), |i| i.deep_hash);
        deep = deep.str(m).hash(d);
    }
    let deep_hash = deep.finish();
    let mut blob = Vec::new();
    put_str(&mut blob, folder);
    put_hash(&mut blob, api_hash);
    put_hash(&mut blob, deep_hash);
    blob.extend_from_slice(&api);
    decode_iface(&blob)
}

pub fn decode_iface(blob: &[u8]) -> FolderIface {
    let mut r = Reader::new(blob);
    let folder = r.str();
    let api_hash = r.hash();
    let deep_hash = r.hash();
    let mut items = Vec::new();
    let mut item_hashes = Vec::new();
    while !r.done() {
        let start = r.pos;
        items.push(decode_item(&mut r));
        item_hashes.push(KeyHasher::new("item").bytes(&blob[start..r.pos]).finish());
    }
    FolderIface { folder, blob: blob.to_vec(), items, item_hashes, api_hash, deep_hash }
}

// ------------------------------------------------------------ world loading

fn lower_sig(w: &mut World, s: &CSig, self_ty: Option<&CTy>) -> FnSig {
    let fix = |c: &CTy| match (c, self_ty) {
        (CTy::SelfTy, Some(t)) => t.clone(),
        _ => c.clone(),
    };
    FnSig {
        generics: s
            .generics
            .iter()
            .map(|(g, b)| (w.sym(g), b.as_ref().map(|b| w.def(b))))
            .collect(),
        params: s.params.iter().map(|(p, t)| (w.sym(p), w.intern_canon(&fix(t)))).collect(),
        ret: w.intern_canon(&fix(&s.ret)),
    }
}

/// Loads header items into the run's tables. Traits before impls, since an
/// impl method's signature is its trait method's with `Self` replaced.
pub fn load_items(w: &mut World, items: &[HeaderItem], hashes: Option<&[Hash128]>) {
    let mut traits: HashMap<String, Vec<(String, CSig)>> = HashMap::new();
    for (i, h) in items.iter().enumerate() {
        let id = w.def(&h.path);
        if let Some(hs) = hashes {
            w.item_hash.insert(id, hs[i]);
        } else {
            let mut b = Vec::new();
            encode_item(&mut b, h);
            w.item_hash.insert(id, KeyHasher::new("item").bytes(&b).finish());
        }
        match &h.item {
            CItem::Fn(s) => {
                let sig = lower_sig(w, s, None);
                w.defs.insert(id, DefKind::Fn(sig));
            }
            CItem::Data(fs) => {
                let fields = fs.iter().map(|(f, t)| (w.sym(f), w.intern_canon(t))).collect();
                w.defs.insert(id, DefKind::Data(fields));
            }
            CItem::Trait(ms) => {
                traits.insert(h.path.clone(), ms.clone());
                let methods = ms.iter().map(|(m, s)| (w.sym(m), lower_sig(w, s, None))).collect();
                w.defs.insert(id, DefKind::Trait(methods));
            }
            CItem::Impl { .. } => {}
        }
    }
    for h in items {
        let CItem::Impl { trait_, target, methods } = &h.item else { continue };
        let id = w.def(&h.path);
        let tid = w.def(trait_);
        let target_ty = w.intern_canon(target);
        let trait_methods: Vec<(String, CSig)> = traits.get(trait_).cloned().unwrap_or_else(|| {
            match w.defs.get(&tid) {
                Some(DefKind::Trait(ms)) => ms
                    .iter()
                    .map(|(m, _)| (w.text(*m).to_owned(), CSig { generics: vec![], params: vec![], ret: CTy::Void }))
                    .collect(),
                _ => Vec::new(),
            }
        });
        let mut ms = Vec::new();
        for m in methods {
            let mid = w.def(&format!("{}.{m}", h.path));
            let index = trait_methods.iter().position(|(n, _)| n == m).unwrap_or(0);
            // The method's signature: the trait's, with Self as the target.
            let sig = match w.defs.get(&tid) {
                Some(DefKind::Trait(tms)) => tms.get(index).map(|(_, s)| s.clone()),
                _ => None,
            };
            let mut sig = sig.unwrap_or(FnSig { generics: vec![], params: vec![], ret: target_ty });
            for p in &mut sig.params {
                p.1 = w.subst(p.1, &[], Some(target_ty));
            }
            sig.ret = w.subst(sig.ret, &[], Some(target_ty));
            let sym = w.sym(m);
            w.defs.insert(
                mid,
                DefKind::ImplMethod { impl_: id, index: u32::try_from(index).expect("i"), sig },
            );
            ms.push((sym, mid));
        }
        w.defs.insert(id, DefKind::Impl { trait_: tid, target: target_ty, methods: ms });
        let list = w.impls.entry(tid).or_default();
        if !list.contains(&id) {
            list.push(id);
        }
    }
}
