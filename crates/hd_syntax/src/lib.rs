#![forbid(unsafe_code)]

mod docs;
mod green;
mod layout;
mod lexer;
mod names;
mod parser;
mod skim;

pub use docs::{doc_text, module_doc_text};
pub use green::*;
pub use layout::{Layout, LayoutCursor, SuiteKind, TokenOrLayout};
pub use lexer::{CommentKind, Lexed, LineFlags, TokenBuf, TokenKind, lex};
pub use names::{function_names, top_level_names};
pub use parser::{ItemIndex, ItemKind, Parse, parse};
pub use skim::{BodyRange, HeaderKind, HeaderSkeleton, skeleton_from_layout, skim};
