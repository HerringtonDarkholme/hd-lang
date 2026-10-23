//! A typed view of one parsed file (`crates/hd_syntax/DESIGN.md` §4.4 "typed views"): tokens,
//! text and spans of the full parser's green tree.

use hd_base::{FileId, Span, Symbol, TokenIdx};
use hd_syntax::{NodeRef, Parse, SyntaxKind, TokenKind};

#[derive(Clone, Copy)]
pub struct Src<'a> {
    pub parse: &'a Parse,
    pub text: &'a str,
    pub file: FileId,
}

impl<'a> Src<'a> {
    #[must_use]
    pub fn root(&self) -> NodeRef<'a> {
        self.parse.tree.root()
    }
    #[must_use]
    pub fn first(&self, n: NodeRef<'_>) -> TokenIdx {
        self.parse.tree.span_tokens(n.index()).0
    }
    #[must_use]
    pub fn last(&self, n: NodeRef<'_>) -> TokenIdx {
        self.parse.tree.span_tokens(n.index()).1
    }
    #[must_use]
    pub fn tkind(&self, t: TokenIdx) -> Option<TokenKind> {
        (t.get().is_some() && t.idx() < self.parse.tokens.len()).then(|| self.parse.tokens.kind(t))
    }
    #[must_use]
    pub fn text(&self, t: TokenIdx) -> &'a str {
        if self.tkind(t).is_none() {
            return "";
        }
        let text = self.parse.tokens.text(t, self.text);
        if self.parse.tokens.kind(t) == TokenKind::RawIdent {
            // A raw identifier's name is the text between the backticks.
            return text.trim_matches('`');
        }
        text
    }
    /// A token's symbol in this compiler run. Standalone parses have no
    /// run interner and retain the text-based path used by their callers.
    #[must_use]
    pub fn symbol(&self, t: TokenIdx, syms: &hd_intern::ShardedInterner) -> Symbol {
        self.parse
            .tokens
            .symbol(t)
            .unwrap_or_else(|| syms.intern(self.text(t)))
    }
    /// The tokens of a node, in order.
    pub fn tokens(&self, n: NodeRef<'_>) -> impl Iterator<Item = TokenIdx> + use<'a> {
        let (lo, hi) = self.parse.tree.span_tokens(n.index());
        let range = if lo.get().is_some() && hi.get().is_some() {
            lo.raw()..hi.raw() + 1
        } else {
            0..0
        };
        range.map(TokenIdx::from_raw)
    }
    /// The first identifier after the node's first keyword of `kw`.
    #[must_use]
    pub fn name_after(&self, n: NodeRef<'_>, kw: TokenKind) -> Option<TokenIdx> {
        let mut seen = false;
        for t in self.tokens(n) {
            match self.tkind(t) {
                // `struct` heads a data declaration as `data` does.
                Some(k) if k == kw || (kw == TokenKind::KwData && k == TokenKind::KwStruct) => {
                    seen = true;
                }
                Some(TokenKind::Ident | TokenKind::RawIdent) if seen => return Some(t),
                _ => {}
            }
        }
        None
    }
    #[must_use]
    pub fn first_ident(&self, n: NodeRef<'_>) -> Option<TokenIdx> {
        self.tokens(n)
            .find(|t| matches!(self.tkind(*t), Some(TokenKind::Ident | TokenKind::RawIdent)))
    }
    #[must_use]
    pub fn is_pub(&self, n: NodeRef<'_>) -> bool {
        n.direct_token(&self.parse.tokens, TokenKind::KwPub)
            .is_some()
    }
    /// The first child that is a type (a parameter's, field's or result's).
    #[must_use]
    pub fn type_child(n: NodeRef<'a>) -> Option<NodeRef<'a>> {
        n.children()
            .find(|c| c.kind().is_type() && c.kind() != SyntaxKind::RequirementRow)
    }
    #[must_use]
    pub fn child(n: NodeRef<'a>, kind: SyntaxKind) -> Option<NodeRef<'a>> {
        n.children().find(|c| c.kind() == kind)
    }
    #[must_use]
    pub fn span(&self, n: NodeRef<'_>) -> Span {
        let (lo, hi) = self.parse.tree.span_tokens(n.index());
        match (self.tkind(lo), self.tkind(hi)) {
            (Some(_), Some(_)) => Span {
                file: self.file,
                lo: self.parse.tokens.span(lo).0,
                hi: self.parse.tokens.span(hi).1,
            },
            _ => Span {
                file: self.file,
                lo: 0,
                hi: 0,
            },
        }
    }
    #[must_use]
    pub fn token_span(&self, t: TokenIdx) -> Span {
        if self.tkind(t).is_none() {
            return Span {
                file: self.file,
                lo: 0,
                hi: 0,
            };
        }
        let (lo, hi) = self.parse.tokens.span(t);
        Span {
            file: self.file,
            lo,
            hi,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Src;
    use hd_base::{FileId, TokenIdx};
    use hd_intern::ShardedInterner;
    use hd_syntax::{TokenKind, parse};

    #[test]
    fn standalone_and_run_parses_use_the_same_identifier_spelling() {
        let text = "fn `type`() -> i32: +1\n";
        let mut parsed = parse(text.as_bytes());
        assert!(parsed.is_ok());
        let syms = ShardedInterner::default();
        syms.intern("not the parser's first symbol");
        let t = parsed
            .tokens
            .kind
            .iter()
            .position(|k| *k == TokenKind::RawIdent)
            .map(|i| TokenIdx::from_raw(u32::try_from(i).unwrap()))
            .unwrap();
        let expected = syms.intern("type");
        let standalone = Src {
            parse: &parsed,
            text,
            file: FileId::from_raw(0),
        };
        assert_eq!(standalone.symbol(t, &syms), expected);
        assert_eq!(syms.resolve(standalone.symbol(TokenIdx::NONE, &syms)), "");
        parsed.tokens.intern_symbols(text, &syms);
        let in_run = Src {
            parse: &parsed,
            text,
            file: FileId::from_raw(0),
        };
        assert_eq!(in_run.symbol(t, &syms), expected);
        assert_eq!(syms.resolve(in_run.symbol(t, &syms)), "type");
    }
}
