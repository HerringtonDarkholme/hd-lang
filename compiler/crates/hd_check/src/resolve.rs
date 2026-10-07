//! Header lowering and name resolution (resolution-and-interfaces.md §4.9,
//! §4.10): typed views over the green tree, the module scope, and lowering of
//! headers to interface items.

use std::collections::HashMap;

use hd_base::{NodeIdx, TokenIdx};
use hd_iface::{CItem, CSig, CTy, FolderIface, HeaderItem, folder_of_module, item_path};
use hd_syntax::subset::SubsetParse;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};

// ---------------------------------------------------------------- CST views

/// Typed-view helpers over the green tree. The green tree keeps only a node's
/// first and last token, so names and operators are found by token position.
pub struct Cst<'a> {
    pub src: &'a str,
    pub p: &'a SubsetParse,
}

impl<'a> Cst<'a> {
    #[must_use]
    pub fn root(&self) -> NodeRef<'a> {
        self.p.tree.root()
    }
    #[must_use]
    pub fn node(&self, i: NodeIdx) -> NodeRef<'a> {
        self.p.tree.node(i)
    }
    #[must_use]
    pub fn first(&self, n: NodeRef<'_>) -> TokenIdx {
        self.p.tree.span_tokens(n.index()).0
    }
    #[must_use]
    pub fn last(&self, n: NodeRef<'_>) -> TokenIdx {
        self.p.tree.span_tokens(n.index()).1
    }
    #[must_use]
    pub fn text(&self, t: TokenIdx) -> &'a str {
        self.p.tokens.text(t, self.src)
    }
    #[must_use]
    pub fn tkind(&self, t: TokenIdx) -> TokenKind {
        self.p.tokens.kind(t)
    }
    #[must_use]
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
    #[must_use]
    pub fn is_pub(&self, n: NodeRef<'_>) -> bool {
        self.tkind(self.first(n)) == TokenKind::KwPub
    }
    #[must_use]
    pub fn children(n: NodeRef<'a>) -> Vec<NodeRef<'a>> {
        n.children().collect()
    }
    #[must_use]
    pub fn child(n: NodeRef<'a>, kind: SyntaxKind) -> Option<NodeRef<'a>> {
        n.children().find(|c| c.kind() == kind)
    }
    #[must_use]
    pub fn span(&self, n: NodeRef<'_>) -> (u32, u32) {
        let (lo, hi) = self.p.tree.span_tokens(n.index());
        (self.p.tokens.span(lo).0, self.p.tokens.span(hi).1)
    }
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
#[must_use]
#[expect(clippy::implicit_hasher, reason = "the run's tables use the std hasher only")]
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
            if iface.public_item(&path).is_some() {
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
#[must_use]
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
