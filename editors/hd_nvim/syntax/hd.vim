" Vim syntax file
" Language:    hd (hd-lang)
" Maintainer:  hd-lang
" Reference:   spec/lang/01-lexical-structure.md; src/highlight.ts is the
"              website's highlighter and makes the same choices.

if exists("b:current_syntax")
  finish
endif

let s:cpo_save = &cpo
set cpo&vim

" Reserved words: KEYWORDS in src/lexer.ts. Keep this list in sync:
"   Self break continue data defer else enum false fn for if impl in is
"   let match mut pass pub return self tests trait true type while
syn keyword hdConditional if else match
syn keyword hdRepeat      for while in
syn keyword hdStatement   return break continue pass defer
syn keyword hdStructure   data enum trait impl type
syn keyword hdStorage     pub mut
syn keyword hdKeyword     fn let is tests
syn keyword hdBoolean     true false
syn keyword hdSelf        self Self

" Primitive types (src/highlight.ts PRIMITIVE_TYPES).
syn keyword hdPrimitive i8 i16 i32 i64 u8 u16 u32 u64 f32 f64
syn keyword hdPrimitive bool char string void never

" Capitalized names are types; `.Variant` shorthand (a leading dot with no
" receiver before it) is an enum variant.
syn match hdType    "\<\u\w*\>"
syn match hdVariant +\%(\%(^\|[^[:alnum:]_)\]}'"`]\)\.\)\@<=\u\w*\>+

" Function names: a declaration after `fn`, and a call before `(`, `!(`,
" or a generic argument list.
syn match hdFunctionCall "\<[a-z_]\w*\ze!\=\%(\[[^\]]*\]\)\=("
syn match hdFunction     "\%(\<fn\s\+\)\@<=\h\w*"

" The suspension-call suffix `!` in `name!(...)`.
syn match hdBang "\w\@1<=![(\[]\@="

" Contextual words (spec/lang/01-lexical-structure.md#contextual-words).
syn match hdInclude "\%(^\s*\%(pub\s\+\)\=\)\@<=use\ze\s\+\%(pkg\|std\|dep\|self\|super\)\>"
syn match hdInclude "\%(^\s*\%(pub\s\+\)\=use\s.*\)\@<=\<\%(as\|super\)\>"
syn match hdKeyword "\%(^\s*impl\>.*\s\)\@<=by\ze\s\+\S"

" The requirement row `$` and the provider operations `$.use`, `$.with`,
" and `$.context`.
syn match hdRequirement "\$"
syn match hdProvider    "\%(\$\.\)\@<=\%(use\|with\|context\|Context\)\>"

" Operators worth marking: pipe, path, arrows, binding, spread, optional.
syn match hdOperator "|>\|::\|->\|=>\|:=\|\.\.\.=\=\|?"

" Decorators: `@name`, `@derive(...)`, and a bare `@` before a fact.
syn match hdDecorator "@\%(\h\w*\)\="

" A raw identifier such as `type` is a plain name.
syn match hdRawIdent "`\h\w*`"

" Numbers. Decimal and floating-point literals may carry `_` separators
" and a literal suffix (`5ms`, `1.5kb`); radix literals take no suffix.
syn match hdNumber "\<\d\+\%(_\d\+\)*\%(\.\d\+\%(_\d\+\)*\)\=\%([eE][+-]\=\d\+\%(_\d\+\)*\)\=\%(\a\w*\)\="
syn match hdNumber "\<0[xX]_\=\x\+\%(_\x\+\)*\>"
syn match hdNumber "\<0[oO]_\=\o\+\%(_\o\+\)*\>"
syn match hdNumber "\<0[bB]_\=[01]\+\%(_[01]\+\)*\>"

" Character literals.
syn match hdCharacter "'\%([^'\\]\|\\u{\x\+}\|\\.\)'" contains=hdEscape

" Everything an interpolation `${...}` may hold.
syn cluster hdExpr contains=hdConditional,hdRepeat,hdStatement,hdStructure,hdStorage,hdKeyword,hdBoolean,hdSelf,hdPrimitive,hdType,hdVariant,hdFunctionCall,hdBang,hdRequirement,hdProvider,hdOperator,hdRawIdent,hdNumber,hdCharacter,hdString,hdTripleString,hdStringPrefix

" Interpolation: `$name`, `$self`, and `${expression}` with nested braces.
syn match  hdInterpName "\$\h\w*" contained
syn region hdInterp matchgroup=hdInterpDelim start="\${" end="}" contained contains=@hdExpr,hdInterpBraces
syn region hdInterpBraces start="{" end="}" contained transparent contains=@hdExpr,hdInterpBraces

syn match hdEscape +\\\%([\\"'nrt0$]\|u{\x\{1,6}}\)+ contained

" Interpreted strings: `"..."` on one line and `"""..."""` across lines.
syn region hdString       start=+"+   skip=+\\\\\|\\"+ end=+"+   oneline contains=hdEscape,hdInterpName,hdInterp
syn region hdTripleString start=+"""+ skip=+\\\\\|\\"+ end=+"""+ contains=hdEscape,hdInterpName,hdInterp

" Prefixed strings such as `r"\d+"` and `sql"""..."""`: raw text, where a
" backslash only keeps the next quote or `$` from acting. A reserved word
" before a quote is not a prefix; keywords win over this match.
syn match  hdStringPrefix +\<\h\w*\ze"+ nextgroup=hdRawString,hdRawTripleString
syn region hdRawString       start=+"+   skip=+\\.+ end=+"+   oneline contained contains=hdInterpName,hdInterp
syn region hdRawTripleString start=+"""+ skip=+\\.+ end=+"""+ contained contains=hdInterpName,hdInterp

" Comments: `#` to end of line; `##` opening a line is a doc comment.
syn keyword hdTodo TODO FIXME XXX NOTE contained
syn match hdComment    "#.*$" contains=hdTodo,@Spell
syn match hdDocComment "\%(^\s*\)\@<=##.*$" contains=hdTodo,@Spell

syn sync minlines=200

hi def link hdConditional     Conditional
hi def link hdRepeat          Repeat
hi def link hdStatement       Statement
hi def link hdStructure       Keyword
hi def link hdStorage         Keyword
hi def link hdKeyword         Keyword
hi def link hdInclude         Include
hi def link hdBoolean         Boolean
hi def link hdSelf            Constant
hi def link hdPrimitive       Type
hi def link hdType            Type
hi def link hdVariant         Constant
hi def link hdFunction        Function
hi def link hdFunctionCall    Function
hi def link hdBang            Special
hi def link hdRequirement     Special
hi def link hdProvider        Keyword
hi def link hdOperator        Operator
hi def link hdDecorator       PreProc
hi def link hdNumber          Number
hi def link hdCharacter       Character
hi def link hdString          String
hi def link hdTripleString    String
hi def link hdStringPrefix    Special
hi def link hdRawString       String
hi def link hdRawTripleString String
hi def link hdEscape          SpecialChar
hi def link hdInterpName      Identifier
hi def link hdInterpDelim     Special
hi def link hdComment         Comment
hi def link hdDocComment      SpecialComment
hi def link hdTodo            Todo

let b:current_syntax = "hd"

let &cpo = s:cpo_save
unlet s:cpo_save
