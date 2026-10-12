//! The shape of an error type (spec 14 "Error Derivation";
//! codegen.md §13.14): which declarations `@error` makes error types,
//! each variant's message form, and each member's cause marker. Header
//! lowering reads it to declare the generated `Display`, `Error` and
//! `From` implementations with their bounds; the checker reads it to
//! write their bodies. Invalid forms are reported by the decorator check
//! (`lower::decorators`); here they simply mark nothing, so a rejected
//! line generates no second diagnostic.

use hd_base::DefId;
use hd_intern::PathKind;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};

use crate::iface::{Field, ItemData, Names};
use crate::view::Src;

/// What a variant's (or an error data type's) `@error` line says.
#[derive(Clone, Copy)]
pub enum Message<'t> {
    /// No `@error` line: the variant displays as its name
    /// (`annot.error.message.absent`).
    Absent,
    /// `@error("...")`: the message, a string expression.
    Text(NodeRef<'t>),
    /// `@error(transparent)` over exactly one member.
    Transparent,
}

/// A member's cause marker (`annot.error.form.from`,
/// `annot.error.form.source`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    None,
    From,
    Source,
}

/// One payload parameter or field, in declaration order.
#[derive(Clone, Copy)]
pub struct Member<'t> {
    pub node: NodeRef<'t>,
    pub marker: Marker,
}

/// One variant; an error data type is one variant.
#[derive(Clone)]
pub struct ErrVariant<'t> {
    pub node: NodeRef<'t>,
    pub message: Message<'t>,
    pub members: Vec<Member<'t>>,
}

impl ErrVariant<'_> {
    /// The cause member's index: the first valid `@from` or `@source`
    /// (`annot.error.cause.one` reports any later one).
    #[must_use]
    pub fn cause(&self) -> Option<usize> {
        self.members.iter().position(|m| m.marker != Marker::None)
    }
}

/// An error type's declaration as error derivation reads it.
#[derive(Clone)]
pub struct ErrorShape<'t> {
    /// The type's own `@error` line, where `Display` and `Error` sit.
    pub line: NodeRef<'t>,
    pub is_enum: bool,
    pub variants: Vec<ErrVariant<'t>>,
}

impl ErrorShape<'_> {
    /// Whether any variant has a cause: a cause member, or a transparent
    /// member's own. Without one, the generated `Error` writes no `cause`
    /// and keeps the trait's default `.None` (`annot.error.cause.explicit`).
    #[must_use]
    pub fn has_cause(&self) -> bool {
        self.variants
            .iter()
            .any(|v| matches!(v.message, Message::Transparent) || v.cause().is_some())
    }

    /// The implementations the type generates: `Display`, `Error`, then
    /// one `From` per `@from` member, in declaration order
    /// (`annot.error.generates`).
    #[must_use]
    pub fn generated(&self) -> Vec<Generated> {
        let mut out = vec![Generated::Display, Generated::Error];
        for (variant, v) in self.variants.iter().enumerate() {
            if let Some(member) = v.cause()
                && v.members[member].marker == Marker::From
            {
                out.push(Generated::From { variant, member });
            }
        }
        out
    }
}

/// One generated implementation of an error type (`annot.error.generates`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Generated {
    Display,
    Error,
    /// `From[P]` for the `@from` member `member` of variant `variant`.
    From {
        variant: usize,
        member: usize,
    },
}

impl Generated {
    /// The implementation's path segment, unique per error type.
    #[must_use]
    pub fn segment(self, type_name: &str) -> String {
        match self {
            Generated::Display => format!("error Display for {type_name}"),
            Generated::Error => format!("error Error for {type_name}"),
            Generated::From { variant, member } => {
                format!("error From {variant}.{member} for {type_name}")
            }
        }
    }

    /// The implementation's `DefId` for the error type `ty`.
    #[must_use]
    pub fn def(self, names: &Names<'_>, ty: DefId) -> DefId {
        let module = names.module(&names.module_of(ty));
        let seg = self.segment(names.display_name(ty));
        DefId::from_raw(names.paths.intern(module, PathKind::Impl, &seg).raw())
    }

    /// The trait method the implementation writes.
    #[must_use]
    pub fn method(self) -> &'static str {
        match self {
            Generated::Display => "to_string",
            Generated::Error => "cause",
            Generated::From { .. } => "from",
        }
    }
}

/// One decorator line: its name, whether it is bare, and its argument list.
fn line<'t>(src: &Src<'t>, d: NodeRef<'t>) -> Option<(&'t str, bool, Option<NodeRef<'t>>)> {
    let e = d.children().next()?;
    let (callee, args) = match e.kind() {
        SyntaxKind::CallExpr => (
            e.children().next()?,
            Src::child(e, SyntaxKind::ArgumentList),
        ),
        _ => (e, None),
    };
    if callee.kind() != SyntaxKind::NameExpr {
        return None;
    }
    let name = src.text(src.first_ident(callee)?);
    Some((name, e.kind() != SyntaxKind::CallExpr, args))
}

fn lines<'a, 't: 'a>(
    src: &'a Src<'t>,
    n: NodeRef<'t>,
) -> impl Iterator<Item = (&'t str, bool, Option<NodeRef<'t>>, NodeRef<'t>)> + 'a {
    n.children()
        .filter(|c| c.kind() == SyntaxKind::Decorator)
        .filter_map(move |d| line(src, d).map(|(a, b, c)| (a, b, c, d)))
}

/// The message form of an `@error(...)` line's arguments; `None` when they
/// are invalid (`annot.error.form.argument`, already reported).
fn message<'t>(src: &Src<'t>, args: Option<NodeRef<'t>>) -> Option<Message<'t>> {
    let mut all = args?.children();
    let (Some(one), None) = (all.next(), all.next()) else {
        return None;
    };
    let mut inner = one.children();
    let (Some(value), None) = (inner.next(), inner.next()) else {
        return None;
    };
    match value.kind() {
        SyntaxKind::StringExpr => Some(Message::Text(value)),
        SyntaxKind::NameExpr
            if src
                .first_ident(value)
                .is_some_and(|t| src.text(t) == "transparent") =>
        {
            Some(Message::Transparent)
        }
        _ => None,
    }
}

/// The members of a variant's parameter list or a data type's block.
fn members<'t>(src: &Src<'t>, holder: Option<NodeRef<'t>>) -> Vec<Member<'t>> {
    let nodes: Vec<NodeRef<'t>> = holder
        .into_iter()
        .flat_map(NodeRef::children)
        .filter(|c| {
            matches!(
                c.kind(),
                SyntaxKind::DataField | SyntaxKind::Parameter | SyntaxKind::EmbeddedField
            )
        })
        .collect();
    let only = nodes.len() == 1;
    let mut cause = false;
    nodes
        .iter()
        .map(|&node| {
            let mut marker = Marker::None;
            for (name, bare, _, _) in lines(src, node) {
                let m = match name {
                    "from" if only => Marker::From,
                    "source" => Marker::Source,
                    _ => continue,
                };
                // A marker with an argument or a second cause marks nothing
                // (`annot.error.form.marker-argument`, `annot.error.cause.one`).
                if bare && !cause {
                    marker = m;
                    cause = true;
                }
            }
            Member { node, marker }
        })
        .collect()
}

/// The variant's message: its `@error` line, if valid; a transparent line
/// over anything but one member is no message.
fn variant_message<'t>(src: &Src<'t>, n: NodeRef<'t>, count: usize) -> Message<'t> {
    let found = lines(src, n)
        .filter(|(name, bare, _, _)| *name == "error" && !bare)
        .find_map(|(_, _, args, _)| message(src, args));
    match found {
        Some(Message::Transparent) if count != 1 => Message::Absent,
        Some(m) => m,
        None => Message::Absent,
    }
}

/// The error shape of a data or enum declaration, or `None` when it is
/// not an error type (`annot.error.type`).
#[must_use]
pub fn shape<'t>(src: &Src<'t>, decl: NodeRef<'t>) -> Option<ErrorShape<'t>> {
    match decl.kind() {
        SyntaxKind::EnumDecl => {
            let (_, _, _, line) =
                lines(src, decl).find(|(n, bare, _, _)| *n == "error" && *bare)?;
            let variants = Src::child(decl, SyntaxKind::Block)
                .into_iter()
                .flat_map(NodeRef::children)
                .filter(|c| c.kind() == SyntaxKind::EnumVariant)
                .map(|v| {
                    let members = members(src, Src::child(v, SyntaxKind::ParameterList));
                    ErrVariant {
                        node: v,
                        message: variant_message(src, v, members.len()),
                        members,
                    }
                })
                .collect();
            Some(ErrorShape {
                line,
                is_enum: true,
                variants,
            })
        }
        SyntaxKind::DataDecl => {
            let (_, _, args, line) =
                lines(src, decl).find(|(n, bare, _, _)| *n == "error" && !*bare)?;
            message(src, args)?;
            let members = members(src, Src::child(decl, SyntaxKind::Block));
            let message = variant_message(src, decl, members.len());
            if matches!(message, Message::Absent) {
                return None;
            }
            Some(ErrorShape {
                line,
                is_enum: false,
                variants: vec![ErrVariant {
                    node: decl,
                    message,
                    members,
                }],
            })
        }
        _ => None,
    }
}

/// A member's name in a message: its declared name, or `_0`, `_1`... for
/// an unnamed payload member (`annot.error.message.scope`).
#[must_use]
pub fn label(names: &Names<'_>, f: &Field) -> String {
    let n = names.text(f.name);
    if n.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{n}")
    } else {
        n.to_owned()
    }
}

/// The member fields of each variant of the error type's item, aligned
/// with the shape's variants: an enum's payloads, a data type's fields.
#[must_use]
pub fn fields(data: &ItemData) -> Vec<&[Field]> {
    match data {
        ItemData::Enum { variants, .. } => variants.iter().map(|v| v.fields.as_slice()).collect(),
        ItemData::Data(fs) => vec![fs.as_slice()],
        _ => Vec::new(),
    }
}

/// The names a message interpolates: each `$name`, and each name inside a
/// `${...}` expression.
#[must_use]
pub fn interpolated(src: &Src<'_>, msg: NodeRef<'_>) -> Vec<String> {
    let mut out = Vec::new();
    for t in msg.direct_tokens() {
        if !matches!(
            src.tkind(t),
            Some(TokenKind::String | TokenKind::StrHead | TokenKind::StrMid | TokenKind::StrTail)
        ) {
            continue;
        }
        let mut chars = src.text(t).chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    chars.next();
                }
                '$' if chars.peek().is_some_and(|n| n.is_alphabetic() || *n == '_') => {
                    let mut name = String::new();
                    while let Some(&n) = chars.peek().filter(|n| n.is_alphanumeric() || **n == '_')
                    {
                        name.push(n);
                        chars.next();
                    }
                    out.push(name);
                }
                _ => {}
            }
        }
    }
    for e in msg
        .descendants()
        .filter(|c| c.kind() == SyntaxKind::NameExpr)
    {
        if let Some(t) = src.first_ident(e) {
            out.push(src.text(t).to_owned());
        }
    }
    out
}
