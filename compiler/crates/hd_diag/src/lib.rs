#![forbid(unsafe_code)]

use hd_base::Span;

pub mod buf;
pub use buf::{CauseKind, DiagBuf, FixEdit, FixSafety, RootKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Code {
    ArgumentOrder,
    ComparisonChaining,
    DecoratorNotTopLevel,
    DirectVariantUse,
    DocCommentWithoutTarget,
    InvalidToken,
    UnexpectedBom,
    TabWhitespace,
    InvalidEscape,
    UnterminatedString,
    UnmatchedDelimiter,
    UnclosedDelimiter,
    InvalidDedent,
    UnexpectedIndentation,
    ReservedSemicolon,
    SyntaxError,
    MissingLet,
    MutableFieldModifier,
    OldBoundOperator,
    OldExportDeclaration,
    OldImportDeclaration,
    OldRowSeparator,
    OldStructDeclaration,
    PatternOrder,
    QualifiedStringPrefix,
    TrailingBlockPosition,
    TraitMethodVisibility,
    VariantResultTypeRemoved,
    // name and type phase (spec/lang chapters 3, 4, 7, 8, 9, 10)
    UnknownName,
    UnknownType,
    UnknownTrait,
    UnknownImport,
    UnknownModule,
    UnknownMethod,
    UnknownDataField,
    TypeMismatch,
    ArgumentCount,
    MissingRequiredField,
    UnsatisfiedTraitBound,
    FolderCycle,
    MissingEntryPoint,
    OverlappingImpl,
    /// Internal: a stage this build needs answered "not implemented".
    Unsupported,
}

impl Code {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ArgumentOrder => "argument-order",
            Self::ComparisonChaining => "comparison-chaining",
            Self::DecoratorNotTopLevel => "decorator-not-top-level",
            Self::DirectVariantUse => "direct-variant-use",
            Self::DocCommentWithoutTarget => "doc-comment-without-target",
            Self::InvalidToken => "invalid-token",
            Self::UnexpectedBom => "unexpected-bom",
            Self::TabWhitespace => "tab-whitespace",
            Self::InvalidEscape => "invalid-escape",
            Self::UnterminatedString => "unterminated-string",
            Self::UnmatchedDelimiter => "unmatched-delimiter",
            Self::UnclosedDelimiter => "unclosed-delimiter",
            Self::InvalidDedent => "invalid-dedent",
            Self::UnexpectedIndentation => "unexpected-indentation",
            Self::ReservedSemicolon => "reserved-semicolon",
            Self::SyntaxError => "syntax-error",
            Self::MissingLet => "missing-let",
            Self::MutableFieldModifier => "mutable-field-modifier",
            Self::OldBoundOperator => "old-bound-operator",
            Self::OldExportDeclaration => "old-export-declaration",
            Self::OldImportDeclaration => "old-import-declaration",
            Self::OldRowSeparator => "old-row-separator",
            Self::OldStructDeclaration => "old-struct-declaration",
            Self::PatternOrder => "pattern-order",
            Self::QualifiedStringPrefix => "qualified-string-prefix",
            Self::TrailingBlockPosition => "trailing-block-position",
            Self::TraitMethodVisibility => "trait-method-visibility",
            Self::VariantResultTypeRemoved => "variant-result-type-removed",
            Self::UnknownName => "unknown-name",
            Self::UnknownType => "unknown-type",
            Self::UnknownTrait => "unknown-trait",
            Self::UnknownImport => "unknown-import",
            Self::UnknownModule => "unknown-module",
            Self::UnknownMethod => "unknown-method",
            Self::UnknownDataField => "unknown-data-field",
            Self::TypeMismatch => "type-mismatch",
            Self::ArgumentCount => "argument-count",
            Self::MissingRequiredField => "missing-required-field",
            Self::UnsatisfiedTraitBound => "unsatisfied-trait-bound",
            Self::FolderCycle => "folder-cycle",
            Self::MissingEntryPoint => "missing-entry-point",
            Self::OverlappingImpl => "overlapping-impl",
            Self::Unsupported => "unsupported",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "argument-order" => Self::ArgumentOrder,
            "comparison-chaining" => Self::ComparisonChaining,
            "decorator-not-top-level" => Self::DecoratorNotTopLevel,
            "direct-variant-use" => Self::DirectVariantUse,
            "doc-comment-without-target" => Self::DocCommentWithoutTarget,
            "invalid-dedent" => Self::InvalidDedent,
            "invalid-escape" => Self::InvalidEscape,
            "invalid-token" => Self::InvalidToken,
            "missing-let" => Self::MissingLet,
            "mutable-field-modifier" => Self::MutableFieldModifier,
            "old-bound-operator" => Self::OldBoundOperator,
            "old-export-declaration" => Self::OldExportDeclaration,
            "old-import-declaration" => Self::OldImportDeclaration,
            "old-row-separator" => Self::OldRowSeparator,
            "old-struct-declaration" => Self::OldStructDeclaration,
            "pattern-order" => Self::PatternOrder,
            "qualified-string-prefix" => Self::QualifiedStringPrefix,
            "reserved-semicolon" => Self::ReservedSemicolon,
            "syntax-error" => Self::SyntaxError,
            "tab-whitespace" => Self::TabWhitespace,
            "trailing-block-position" => Self::TrailingBlockPosition,
            "trait-method-visibility" => Self::TraitMethodVisibility,
            "variant-result-type-removed" => Self::VariantResultTypeRemoved,
            "unknown-name" => Self::UnknownName,
            "unknown-type" => Self::UnknownType,
            "unknown-trait" => Self::UnknownTrait,
            "unknown-import" => Self::UnknownImport,
            "unknown-module" => Self::UnknownModule,
            "unknown-method" => Self::UnknownMethod,
            "unknown-data-field" => Self::UnknownDataField,
            "type-mismatch" => Self::TypeMismatch,
            "argument-count" => Self::ArgumentCount,
            "missing-required-field" => Self::MissingRequiredField,
            "unsatisfied-trait-bound" => Self::UnsatisfiedTraitBound,
            "folder-cycle" => Self::FolderCycle,
            "missing-entry-point" => Self::MissingEntryPoint,
            "overlapping-impl" => Self::OverlappingImpl,
            "unsupported" => Self::Unsupported,
            "unclosed-delimiter" => Self::UnclosedDelimiter,
            "unexpected-bom" => Self::UnexpectedBom,
            "unexpected-indentation" => Self::UnexpectedIndentation,
            "unmatched-delimiter" => Self::UnmatchedDelimiter,
            "unterminated-string" => Self::UnterminatedString,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: Code,
    pub severity: Severity,
    pub primary: Span,
}

impl Diagnostic {
    #[must_use]
    pub const fn error(code: Code, primary: Span) -> Self {
        Self {
            code,
            severity: Severity::Error,
            primary,
        }
    }
}
