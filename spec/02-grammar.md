# Grammar

Status: language specification draft.

This chapter collects the hd-lang grammar in EBNF.

1. r[grammar.scope.syntax] The grammar specifies syntactic form, not name resolution, typing, exhaustiveness, or runtime behavior.
2. r[grammar.scope.tokens] The grammar consumes the token stream produced by [Lexical Structure](01-lexical-structure.md).
3. r[grammar.scope.layout-tokens] `NEWLINE`, `INDENT`, `DEDENT`, `SUITE_END`, and `EOF` are abstract layout tokens.
4. r[grammar.scope.comments] Comments do not appear in this grammar.
5. r[grammar.scope.consolidated] The feature chapters refine the semantic constraints on these productions, but do not maintain separate extension grammars.

## Source Files And Suites

A source file is a sequence of top-level items:

```ebnf
source_file = { NEWLINE | top_level_item }, EOF ;

top_level_item = use_decl
               | tests_block
               | decorated_decl
               | declaration
               | top_level_statement
               ;

top_level_statement = suite_statement
                    | simple_statement, NEWLINE
                    ;
```

1. r[grammar.suite.same-line] A same-line `suite_body` ends at `SUITE_END`.
2. r[grammar.suite.indented] An indented `suite_body` begins on the following logical line.
3. r[grammar.suite.nonempty] The production requires at least one statement in an indented body; use `pass` when an explicit no-op body is required.
4. r[grammar.suite.local-declarations] Named declarations and implementations may also occur in executable block suites.
5. r[grammar.suite.use-top-level] Use declarations remain top-level items.
6. r[grammar.suite.methods] Methods occur inside trait and implementation declarations through their dedicated grammar productions.

## Test Blocks

A file may hold one `tests:` block, whose items are compiled only for tests:

```ebnf
tests_block = "tests", ":", NEWLINE, INDENT, tests_item, { tests_item },
              DEDENT ;

tests_item = use_decl
           | decorated_decl
           | declaration
           | top_level_statement
           ;
```

```text
use std.testing.assert_equal

fn late_fee(days: i32) -> i32:
    if days > 30: 5 else: 0

tests:
    fn overdue() -> i32: 31

    it("charges a fee after 30 days"):
        assert_equal(late_fee(overdue()), 5, reason="one day late")
```

1. r[grammar.tests.block] A `tests:` block is a top-level item that holds its module's test-only items.
2. r[grammar.tests.item-forms] Its items take the forms of top-level items: use declarations, decorated declarations, declarations, and statements.
3. r[grammar.tests.top-level] A `tests:` block may appear only at module top level. A `tests:` block inside a suite or inside another `tests:` block is an error. Error: `syntax-error`.
4. r[grammar.tests.once] A file may have at most one `tests:` block. A second block is an error. Error: `duplicate-tests-block`.
5. r[grammar.tests.statements] Each statement of the block must be a call of the prelude function `it`, as [Test Cases](10-modules.md#test-cases) specifies.
6. r[grammar.tests.keyword] `tests` is a reserved word, so it never names a declaration or binding.

```text
fn helper() -> void:
    tests:  # error: syntax-error
        pass

tests:
    it("first block"):
        pass

tests:  # error: duplicate-tests-block
    it("second block"):
        pass
```

> **Why.** Like Rust's `#[cfg(test)] mod tests`, one block groups a file's
> test-only code, and the normal build never sees it.

See also: [Tests Blocks](03-names-and-scopes.md#tests-blocks),
[Standard Testing](10-modules.md#standard-testing).

## Statements

Statements, including local declarations, follow this grammar:

```ebnf
statement = suite_statement
          | function_decl
          | data_decl
          | enum_decl
          | trait_decl
          | type_decl
          | impl_decl
          | simple_statement, NEWLINE
          ;

suite_statement = defer_statement
                | suite_expression
                | trailing_block_call
                | "_", ":=", suite_right_side
                | binding_target, ":=", { identifier, ":=" },
                  suite_right_side
                | "let", let_pattern, [ ":", type ], "=",
                  suite_right_side
                | postfix_expression, ( "=" | compound_assign_op ),
                  suite_right_side
                | "return", suite_right_side
                | "break", suite_right_side
                ;

suite_right_side = suite_expression | trailing_block_call ;

defer_statement = "defer", ":", suite_body ;

simple_statement = let_statement
                 | short_binding_statement
                 | discard_statement
                 | assignment_statement
                 | return_statement
                 | break_statement
                 | continue_statement
                 | expression_statement
                 ;

let_statement = "let", let_pattern, [ ":", type ], "=",
                closed_expression ;

short_binding_statement = binding_list, ":=", closed_expression ;

discard_statement = "_", ":=", closed_expression ;

assignment_statement = postfix_expression, ( "=" | "...=" | compound_assign_op ),
                       closed_expression ;

compound_assign_op = "+=" | "-=" | "*=" | "/=" | "%="
                   | "&=" | "|=" | "^=" | "<<=" | ">>="
                   ;

return_statement = "return", [ closed_expression ] ;
break_statement = "break", [ closed_expression ] ;
continue_statement = "continue" ;
expression_statement = closed_expression ;

binding_pattern = identifier, { ",", identifier } ;

binding_target = identifier | binding_list ;
binding_list = "(", identifier, ",", identifier, { ",", identifier }, ")" ;

let_pattern = let_name
            | "(", let_name, ",", let_name, { ",", let_name }, ")"
            ;
let_name = [ "mut" ], identifier ;

inline_statement = "let", let_pattern, [ ":", type ], "=", inline_expression
                 | binding_list, ":=", inline_expression
                 | "_", ":=", inline_expression
                 | postfix_expression, ( "=" | "...=" | compound_assign_op ),
                   inline_expression
                 | "return", [ inline_expression ]
                 | "break", [ inline_expression ]
                 | continue_statement
                 | inline_expression
                 ;
```

### Let Statements

1. r[grammar.stmt.let-mut-single] In a `let` statement with one name, `mut` may precede the name, as in `let mut user = ...`.
2. r[grammar.stmt.let-list] A multi-name `let` always puts its names in parentheses, with or without `mut`, as in `let (a, b) = ...`.
3. r[grammar.stmt.let-list.bare] A multi-name `let` without parentheses, as in `let a, b = ...` or `let mut log, db = ...`, is an error whose fix-it adds the parentheses. Error: `syntax-error`.
4. r[grammar.stmt.let-list.two-names] A parenthesized list must hold at least two names, so `let (a) = ...` is an error. Error: `syntax-error`.
5. r[grammar.stmt.let-mut-list] In a parenthesized list, `mut` may precede each name, as in `let (mut log, db) = ...`.
6. r[grammar.stmt.let-mut.per-name] A `mut` belongs to the one name it precedes. The access it requests is a semantic rule of [Binding Forms](04-type-system.md#binding-forms).
7. r[grammar.stmt.let-mut.only-let] Only `let` accepts it: `mut user := ...` and `for mut item in items:` are errors. Error: `syntax-error`.

```text
fn pair() -> (List[i32], List[i32]): ([1], [2])

fn invalid() -> void:
    let (first, second) = pair()  # valid: no mut needed
    mut total := 0  # error: syntax-error
    let mut log, db = pair()  # error: syntax-error
    let a, b = pair()  # error: syntax-error
    let (only) = pair()  # error: syntax-error
```

> **Why.** In `let mut log, db`, a reader may take `mut` as covering both
> names. The parentheses show that it belongs to `log` alone. A multi-name
> `let` uses them even without `mut`, since one shape reads better than two
> spellings.

### Short Binding Lists

1. r[grammar.stmt.bind-list] A multi-name `:=` binding always puts its names in parentheses, as in `(a, b) := pair`.
2. r[grammar.stmt.bind-list.bare] A multi-name `:=` binding without parentheses, as in `a, b := pair`, is an error whose fix-it adds the parentheses. Error: `syntax-error`.
3. r[grammar.stmt.bind-list.two-names] A parenthesized list must hold at least two names, so `(a) := pair` is an error. Error: `syntax-error`.
4. r[grammar.stmt.bind-list.not-tuple] At the start of a statement, `(`, two or more names separated by commas, `)`, and `:=` always form this binding. The parenthesized names are never a tuple expression.
5. r[grammar.stmt.bind-list.new-statement] A line that starts with `(` never continues the previous line as a call, as [`lex.continue.paren-line`](01-lexical-structure.md#r-lex.continue.paren-line) states. So a binding list on the line after `limit := low` is its own statement, never the call `low(first, second)`.

```text
fn pair() -> (i32, i32): (1, 2)

fn split() -> i32:
    (low, high) := pair()  # valid
    limit := low
    (first, second) := pair()  # valid: a new statement
    a, b := pair()  # error: syntax-error
    (only) := pair()  # error: syntax-error
    limit + high + first + second
```

> **Why.** A `let` list and a `:=` list put their names in parentheses the
> same way, as in `let (a, b) = pair` and `(a, b) := pair`. One shape reads
> better than two spellings.

### Discard And Defer Statements

1. r[grammar.stmt.discard] The dedicated discard forms make `_ := expression` a statement without making the placeholder `_` an identifier or a binding pattern.
2. r[grammar.stmt.discard.suite] The suite form of a discard exists for the same reason when the discarded expression owns an indented suite.
3. r[grammar.stmt.defer] `defer` is parsed wherever a suite statement is accepted; the semantic rules in [Control Flow](06-control-flow.md#deferred-cleanup) restrict it to executing cleanup scopes.

### Suite Statements

1. r[grammar.stmt.suite] A `suite_statement` is a statement whose outermost expression owns a suite.
2. r[grammar.stmt.suite.end] Its final `DEDENT`, or the `SUITE_END` of a same-line suite, terminates the statement; it does not require another `NEWLINE`.
3. r[grammar.stmt.suite.right-side] This separate production is what permits `value := if ...`, `let callback = fn ...`, and similar direct right-hand-side forms.
4. r[grammar.stmt.suite.trailing-block] Every right-hand side that accepts a suite expression, after `:=`, `let ... =`, `=`, `_ :=`, `return`, and `break`, also accepts a trailing block call.
5. r[grammar.stmt.chain] A chain of bindings continues only with single names, as in `a := b := if c: 1 else: 2`.
6. r[grammar.stmt.chain.multi-name-first] A multi-name pattern may only come first, so `(a, b) := (c, d) := pair` is a syntax error with or without a suite. Error: `syntax-error`.
7. r[grammar.stmt.suite.in-delimiters] A suite expression nested inside delimiters remains part of its enclosing expression, and the enclosing statement ends normally after the closing delimiter.

```text
fn pairs() -> void:
    (a, b) := (c, d) := fn() -> (i32, i32): (1, 2)  # error: syntax-error
    pass
```

### Statements Ending At A Newline

1. r[grammar.stmt.closed] A statement that ends at `NEWLINE` takes a `closed_expression`, which cannot end in a suite, because layout emits no `NEWLINE` after a suite's `SUITE_END` or `DEDENT`.
2. r[grammar.stmt.closed.suite-alternatives] Only the `suite_statement` alternatives may end in a suite.
3. r[grammar.stmt.closed.examples] Thus `y := if c: 1 else: 2` is a statement, but `_ := y := if c: 1 else: 2` and `return y := if c: 1 else: 2` are syntax errors. Error: `syntax-error`.
4. r[grammar.stmt.closed.parenthesized] Parenthesizing the inner binding makes them valid.

```text
fn choose(flag: bool) -> i32:
    _ := y := if flag: 1 else: 2  # error: syntax-error
    return y := if flag: 1 else: 2  # error: syntax-error
```

### Same-Line Suite Bodies

1. r[grammar.inline.statement] A same-line suite body is an `inline_statement`.
2. r[grammar.inline.closed-by-layout] Layout closes a same-line suite at the end of its logical line and at any comma at the suite's own delimiter depth.
3. r[grammar.inline.no-comma] The body therefore contains no comma at that depth and no indented suite.
4. r[grammar.inline.no-if] It also contains no same-line `if` at that depth: `if a: if b: 1 else: 2 else: 3`, `fn f() -> i32: if c: 1 else: 2`, and `defer: if flag: pass` are syntax errors. Error: `syntax-error`.
5. r[grammar.inline.nested-if] Parentheses nest a conditional, as in `if a: (if b: 1 else: 2) else: 3`, and an indented body may hold one.
6. r[grammar.inline.else-if] `else if` continues the same conditional rather than nesting one.
7. r[grammar.inline.loops] Same-line `for` and `while` loops may still appear directly in a same-line suite.
8. r[grammar.inline.multi-name-for] A `for` over several names needs an indented body.
9. r[grammar.inline.let-list] A parenthesized `let` list may be a same-line suite body, as in `if ok: let (a, b) = pair` and `if ok: let (mut log, db) = pair`, because its commas are inside parentheses.
10. r[grammar.inline.bind-list] A parenthesized `:=` list may be a same-line suite body for the same reason, as in `if ok: (a, b) := pair`.
11. r[grammar.inline.bare-comma] The bare comma forms still close the suite, so `if ok: a, b := pair` and `if ok: let a, b = pair` are syntax errors. Error: `syntax-error`.

```text
fn pair() -> (i32, i32): (1, 2)

fn pick(a: bool, b: bool) -> i32:
    v := if a: if b: 1 else: 2 else: 3  # error: syntax-error
    v

fn sign(x: i32) -> i32: if x < 0: -1 else: 1  # error: syntax-error

fn release(flag: bool) -> void:
    defer: if flag: pass  # error: syntax-error
    if flag: a, b := pair()  # error: syntax-error
    if flag: let a, b = pair()  # error: syntax-error
```

> **Note.** A binding is scoped to its block, so a name that a same-line
> suite binds is never read after the suite ends. The
> [`unused-local-binding`](06-control-flow.md#r-flow.unused.warning)
> warning reports it:

```text
fn pair() -> (i32, i32): (1, 2)

fn split(ready: bool) -> void:
    if ready: let (low, high) = pair()  # warning: unused-local-binding
    if ready: (first, second) := pair()  # warning: unused-local-binding
```

### Expressions Followed By Another Token

1. r[grammar.continued.positions] Where another token follows an expression inside brackets, the grammar uses `continued_expression`.
2. r[grammar.continued.position-list] Those positions are the header of a control-flow expression written directly inside brackets, a comprehension clause, a map key, a spread before `...`, and a parameter decorator.
3. r[grammar.continued.no-same-line-suite] A `continued_expression` cannot end in a same-line suite, because layout would extend that suite over the following token.
4. r[grammar.continued.layout] Layout ends a same-line suite only at a line boundary outside brackets, at a comma or closing delimiter at its depth, or before `else`.
5. r[grammar.continued.indented-suite] A `continued_expression` may end in an indented suite, except a closure body.
6. r[grammar.continued.no-closure] After an indented closure body inside brackets, the next line must start with `,` or a closing delimiter, so `indented_suite_expression` has no closure alternative.
7. r[grammar.closed.outside-brackets] Outside brackets, an expression followed by another token cannot end in any suite.
8. r[grammar.closed.header-positions] A control-flow header in a statement and a match guard therefore take a `closed_expression`.
9. r[grammar.closed.bracketed-suite] A suite may still appear inside brackets within the header, as in `if check(fn(x): ...):`.
10. r[grammar.closed.indented-header] But a statement `if fn() -> bool:`, followed by the closure's indented body and then a line beginning `: 1 else: 2`, is a syntax error, because its header ends in an indented suite. Error: `syntax-error`.
11. r[grammar.closed.nested-statements] The statements of a suite nested inside brackets follow the same rule, because they are statements too.

```text
fn pick() -> i32:
    if fn() -> bool:  # error: syntax-error
        true
    : 1 else: 2
```

See also: [Physical And Logical Lines](01-lexical-structure.md#physical-and-logical-lines).

### Semantic Statement Rules

1. r[grammar.stmt.semantic] Whether a statement may appear in a particular value-producing block is a semantic rule.
2. r[grammar.stmt.break] In particular, `break` is valid only inside a loop, and `break` with a value is valid only in a loop with an `else` suite.
3. r[grammar.stmt.assign-target] The left side of an assignment must resolve to a reassignable local, mutable field, or mutable indexed place; calls and other non-place postfix expressions are rejected semantically.
4. r[grammar.stmt.copy-assign] The copy assignment `place ...= value` is valid only when the place is an embedded field.
5. r[grammar.stmt.copy-assign.embedded] An embedded field is assigned only with `...=`.
6. r[grammar.stmt.compound-assign] A compound assignment `place op= value` takes the same left side as an assignment, and its right side follows the same forms as `=`. [Compound Assignment](05-expressions.md#compound-assignment) defines it.

See also: [Data Embedding](08-data-and-enums.md#data-embedding).

## Declarations

A declaration is an optionally public named declaration or an
implementation:

```ebnf
declaration = [ "pub" ], ( function_decl
                         | data_decl
                         | enum_decl
                         | trait_decl
                         | type_decl )
            | impl_decl
            ;
```

1. r[grammar.decl.impl-no-pub] `pub` is not accepted before an `impl` declaration.

> **Why.** Implementations are not independently named module members.

### Functions

```ebnf
function_decl = "fn", callable_name, [ function_generic_params ], parameter_clause,
                [ "->", result_type ], [ requirement_clause ], ":",
                suite_body ;

callable_name = identifier, [ "!" ] ;

suite_body = inline_suite_body
           | NEWLINE, INDENT, statement, { statement }, DEDENT
           ;

inline_suite_body = inline_statement, SUITE_END ;

parameter_clause = "(", [ parameter_list ], ")" ;
parameter_list = parameter, { ",", parameter }, [ "," ] ;

parameter = receiver_parameter
          | { parameter_decorator }, value_parameter
          ;

parameter_decorator = "@", continued_expression ;
value_parameter = identifier, ":", type, [ "=", expression ]
                | identifier, ":", type, "..."
                ;

receiver_parameter = "self" | "mut", "self" ;
```

1. r[grammar.fn.receiver] The receiver forms are valid only for methods.
2. r[grammar.fn.decorator-param-targets] Among function parameters, parameter decorators are valid only on value parameters of module-level named functions and of methods. Variant payload parameters also accept them, as [Enums](#enums) states.
3. r[grammar.fn.decorator.lines] Within a multiline parameter clause, each decorator may occupy its own prefix line; delimiter line breaks do not terminate the parameter.
4. r[grammar.fn.vararg] A vararg parameter ends in `...`; it must be the final positional parameter.
5. r[grammar.fn.vararg.value-pack] The final-parameter rule includes a value-pack parameter, whose nonfinal use is an error. Error: `nonfinal-positional-value-pack`.
6. r[grammar.fn.semantic] Default-argument ordering and the requirement-free rule are semantic constraints defined in [Functions](07-functions.md).

```text
fn invalid[Ts...](values: Ts..., tail: i32) -> void:  # error: nonfinal-positional-value-pack
    pass
```

### Data Types

```ebnf
data_decl = "data", identifier, [ type_params ], ":", data_suite ;

data_suite = "pass", SUITE_END
             | NEWLINE, INDENT,
               ( "pass", NEWLINE
               | data_member, { data_member } ), DEDENT
             ;

data_member = { decorator_line }, ( data_field | embedded_field ), NEWLINE
              ;

data_field = [ "pub" ], identifier, ":", type, [ "=", closed_expression ] ;
embedded_field = named_type ;
```

1. r[grammar.data.no-mut-modifier] `mut` is not a data-member modifier: `mut name: string` and `mut Base` are invalid.
2. r[grammar.data.mut-type] A named field may instead declare a mutable type, as in `friend: mut User`.
3. r[grammar.data.embedded] An embedded field must denote a data type and must not include `mut`.
4. r[grammar.data.embedded.no-pub] An embedded field takes no `pub` marker, so `pub Base` in a data body is an error. Error: `syntax-error`.
5. r[grammar.data.embedded.generic] An embedded field may instantiate a generic data type.
6. r[grammar.data.embedded.name] The type's final name, without its type arguments, is the embedded field name.
7. r[grammar.data.embedded.unique] Duplicate embedded names are rejected.
8. r[grammar.data.default] Data-field default expressions have the requirement-free constraint specified in [Data Types and Enums](08-data-and-enums.md#data-declarations).

```text
pub data Base:
    id: string

pub data Post:
    pub Base  # error: syntax-error
    pub title: string
```

> **Why.** An embedded field is always public.

See also: [Data Declarations](08-data-and-enums.md#data-declarations).

### Enums

```ebnf
enum_decl = "enum", identifier, [ type_params ],
            [ enum_parameter_clause ], ":",
            NEWLINE, INDENT, enum_variant, { enum_variant }, DEDENT ;

enum_variant = { decorator_line }, identifier, [ generic_params ], [ variant_parameter_clause ],
               [ "->", variant_result ], NEWLINE ;

enum_parameter_clause = "(", [ enum_parameter_list ], ")" ;
enum_parameter_list = enum_parameter, { ",", enum_parameter }, [ "," ] ;
enum_parameter = [ identifier, ":" ], type, [ "=", expression ] ;
variant_parameter_clause = "(", [ data_parameter_list ], ")" ;
data_parameter_list = data_parameter, { ",", data_parameter }, [ "," ] ;
data_parameter = { parameter_decorator }, [ identifier, ":" ], type ;

variant_result = named_type, [ argument_clause ] ;
```

1. r[grammar.enum.variant-result] The optional variant result initializes constructor data shared by every variant, as in `NotFound -> StatusCode(404)`.
2. r[grammar.enum.variant-result.refine] The optional variant result may refine the enclosing enum type as specified by the GADT rules.
3. r[grammar.enum.defaults] Only shared enum constructor parameters may declare defaults.
4. r[grammar.enum.defaults.rules] Their ordering and requirement-free constraints follow function-parameter defaults.
5. r[grammar.enum.payload-decorator] A variant payload parameter may carry parameter decorators, as in `Moved(to: string, @rename("why") reason: string)`. They attach [facts](14-annotations.md#facts) to that payload member.

### Traits And Implementations

```ebnf
trait_decl = "trait", identifier, [ type_params ],
             [ "<", supertrait_bounds ],
             ( NEWLINE
             | ":", NEWLINE, INDENT,
               trait_member, { trait_member }, DEDENT )
             ;

supertrait_bounds = bound_trait_type, { "&", bound_trait_type } ;

trait_member = associated_type_decl
             | { decorator_line }, "fn", callable_name, [ function_generic_params ],
               parameter_clause,
               "->", result_type, [ requirement_clause ],
               ( NEWLINE | ":", suite_body )
             ;

impl_decl = "impl", [ generic_params ], impl_header_types,
            [ "by", identifier ],
            ( NEWLINE
            | ":", NEWLINE, INDENT,
              impl_member, { impl_member }, DEDENT )
            ;

impl_header_types = trait_type, "for", type
                  | type
                  ;

impl_member = associated_type_decl
            | { decorator_line }, method_decl
            | derivation_line
            ;

derivation_line = ( identifier | "Self" ), ( "=" | "+=" ), closed_expression,
                  NEWLINE ;

method_decl = [ "pub" ], "fn", callable_name, [ function_generic_params ],
              parameter_clause, [ "->", result_type ],
              [ requirement_clause ], ":", suite_body ;

associated_type_decl = "type", identifier, [ "=", type ], NEWLINE ;
```

1. r[grammar.impl.inherent] `impl T:` is an inherent implementation.
2. r[grammar.impl.trait] `impl Trait for T:` is a trait implementation.
3. r[grammar.impl.delegation-field] Except when `E` is `Structure`, `impl Trait for T by E` delegates the trait to the embedded field `E` of `T` and may omit its body.
4. r[grammar.trait.marker] A trait declaration without a body is a marker trait.
5. r[grammar.impl.bodyless] A trait implementation may omit its body when the trait is a marker or when every trait method has a default.
6. r[grammar.impl.promoted] A method promoted from an embedded field never fills a trait method.
7. r[grammar.impl.pub-method] A `pub` method is permitted only in an inherent implementation; trait method visibility follows the trait. A `pub` trait method or trait implementation method is an error. Error: `trait-method-visibility`.
8. r[grammar.trait.method-end] A bodyless trait method ends at `NEWLINE`; a default method has `:` followed by a suite.
9. r[grammar.trait.supertrait] `trait Child < Parent:` declares `Parent` as a supertrait and opens the body with `:`.
10. r[grammar.decl.bound-vs-colon] In declarations, `<` introduces a bound (a supertrait or a generic parameter bound), while `:` means "has type" or opens a suite.
11. r[grammar.trait.method-kind] A function member whose first parameter is `self` or `mut self` is a method; a receiverless member is an associated function.
12. r[grammar.trait.associated-type] Associated type declarations omit `=` in a trait requirement and provide `= type` in an implementation.
13. r[grammar.impl.inline-bounds] Generic implementations state every bound inline in their generic parameter list; the language has no separate bound clause.
14. r[grammar.impl.by-structure] `impl Trait for T by Structure` declares a derivation template or a derivation block, as [Typed Derivation](14-annotations.md#typed-derivation) defines.
15. r[grammar.impl.traitless-by] `impl T by Structure`, without a trait, declares a trait-less derivation block, as [Trait-Less Derivation Blocks](14-annotations.md#trait-less-derivation-blocks) defines.
16. r[grammar.impl.traitless-by.other] In a header without a trait, `by` followed by any other name is a semantic error, not a grammar error, as [`trait.by.trait-less`](09-traits.md#r-trait.by.trait-less) defines.
17. r[grammar.impl.derivation-line] A `derivation_line` is a member line. Its placement and meaning are defined in [Member Lines](14-annotations.md#member-lines).
18. r[grammar.impl.derivation-line.forms] Which right sides a member line accepts is a semantic rule of [Member Lines](14-annotations.md#member-lines), not a grammar rule.

```text
pub trait Display:
    pub fn to_string(self) -> string  # error: trait-method-visibility

trait Named:
    fn name(self) -> string

trait Greeter: Named  # error: syntax-error
```

See also: [Trait Delegation](09-traits.md#trait-delegation).

### Type Declarations

```ebnf
type_decl = "type", identifier, [ type_params ],
            ( "=", ( type | row_alias_target ) | "(", type, ")" ), NEWLINE ;
row_alias_target = requirement_key, "+", requirement_list
                 | "$", "(", ")"
                 ;
```

1. r[grammar.type-decl.alias] The `=` form declares a transparent alias.
2. r[grammar.type-decl.newtype] The parenthesized form declares a nominal single-field newtype.
3. r[grammar.type-decl.row-alias] An `=` form whose right side joins requirement keys with `+`, as in `type AppRow = Db + Cache`, or is `$()`, declares a row alias.
4. r[grammar.type-decl.row-alias.one-key] A right side of one key, as in `type Store = Db`, is an ordinary `type`, and its use site gives its meaning.
5. r[grammar.type-decl.row-alias.no-and] `&` joins bounds only, so `type Both = Db & Cache` is an error. Error: `syntax-error`.

```text
trait Db

trait Cache

type AppRow = Db + Cache

type Both = Db & Cache  # error: syntax-error
```

See also: [Row Aliases](11-requirements-and-suspension.md#row-aliases).

## Generic Parameters And Bounds

```ebnf
type_params = "[", type_parameter, { ",", type_parameter }, [ "," ], "]" ;
generic_params = "[", generic_parameter,
                 { ",", generic_parameter }, [ "," ], "]" ;
function_generic_params = "[", function_generic_parameter,
                          { ",", function_generic_parameter }, [ "," ], "]" ;

type_parameter = [ variance ], identifier, [ "<", trait_bounds ],
                 [ type_default ] ;
generic_parameter = [ "reified" ], identifier, [ "..." ],
                    [ "<", trait_bounds ] ;
function_generic_parameter = generic_parameter
                           | [ "reified" ], identifier, [ "<", trait_bounds ],
                             type_default ;
type_default = "=", type_argument ;
variance = "+" | "-" ;

trait_bounds = [ "mut" ], bound_trait_type, { "&", bound_trait_type } ;
bound_trait_type = qualified_name, [ bound_type_arguments ] ;
bound_type_arguments = "[", bound_type_argument_list, [ "," ], "]" ;
bound_type_argument_list = type_argument, { ",", type_argument },
                           { ",", associated_type_binding }
                         | associated_type_binding,
                           { ",", associated_type_binding }
                         ;
associated_type_binding = identifier, "=", type ;
trait_type = qualified_name, [ type_arguments ] ;
```

### Multiple Bounds

1. r[grammar.generic.bound.and] Several trait bounds on one type are joined with `&`, as in `T < Eq & Hash`, and the type implements every one.
2. r[grammar.generic.bound.and.positions] `&` joins bounds in every bound position: a generic parameter, a supertrait list, and an implementation's generic parameters.
3. r[grammar.generic.bound.and.examples] For example, `trait Ord < Eq & PartialOrd` and `impl[T < Eq & Hash] Hash for Bag[T]` join bounds with `&`.
4. r[grammar.generic.bound.and.types-only] Bounds appear only in type positions, so a bound's `&` never conflicts with the bitwise `&` of an expression.
5. r[grammar.generic.bound.old-plus] A `+` between bounds, as in the former `T < A + B`, is an error whose fix-it writes `A & B`. Error: `old-bound-operator`.

```text
trait Named:
    fn name(self) -> string

trait Tagged:
    fn tag(self) -> string

fn label[T < Named + Tagged](value: T) -> string: value.name()  # error: old-bound-operator
```

> **Why.** `&` means both at once, as in Java `<T extends A & B>`, TypeScript
> and Scala 3 `A & B`, and Swift `P & Q`. It leaves `+` to requirement rows,
> so each operator has one meaning.

### Associated Type Bindings In Bounds

1. r[grammar.generic.binding] A trait in a generic parameter bound may end its bracketed arguments with associated type bindings: `I < Supplier[Item = T]` requires `I` to implement `Supplier` with `I::Item` equal to `T`.
2. r[grammar.generic.binding.order] Bindings follow every positional type argument.
3. r[grammar.generic.binding.positions-key] Bindings are valid in `trait_bounds`, in `supertrait_bounds`, as in `trait Summable < Add[Out = Self]`, in a `named_type`, as in the trait value type `Supplier[Item = i32]`, and in a `requirement_key`, as in `$ Store[Item = User]`.
4. r[grammar.generic.binding.trait-type-only] The trait of an implementation header, a trait-qualified call, and a method reference is a `trait_type`, whose arguments take no binding. A binding there is an error. Error: `syntax-error`.
5. r[grammar.generic.binding.named-type] Only a trait value type gives a binding in a `named_type` a meaning; [Binding Positions](09-traits.md#binding-positions) rejects one elsewhere.

```text
trait Supplier:
    type Item
    fn get(self) -> Self::Item

data Constant:
    value: string

impl Supplier[Item = string] for Constant:  # error: syntax-error
    fn get(self) -> string: self.value
```

See also: [Associated Type Bindings](09-traits.md#associated-type-bindings).

### Type-Argument Default Syntax

A generic parameter may end in `=` and a default type argument, after its
bound:

```text
trait Supplier:
    type Item
    fn get(self) -> Self::Item

data Constant: pass

impl Supplier for Constant:
    type Item = string
    fn get(self) -> string: "constant"

fn pick[T, I < Supplier[Item = T] = Constant](source: I) -> T:
    source.get()
```

1. r[grammar.generic.default] A `type_default` may end a generic parameter of a data type, enum, trait, `type` declaration, function, or method, after any bound.
2. r[grammar.generic.default.binding] A binding sits inside a bound trait's brackets, while a default follows them at the level of the parameter list. `I < Supplier[Item = T] = Constant` has both.
3. r[grammar.generic.default.positions] The generic parameters of an implementation and of an enum variant use `generic_params`, which has no default, and a type pack takes none. A default there is an error. Error: `syntax-error`.
4. r[grammar.generic.default.semantic] Default order, the names a default may use, and when it applies are semantic rules of [Type-Argument Defaults](04-type-system.md#type-argument-defaults).

```text
data Box[T]:
    value: T

impl[T = i32] Box[T]:  # error: syntax-error
    fn get(self) -> T: self.value
```

### Generic Parameter Modifiers

1. r[grammar.generic.reified-modifier] An unbackticked `reified` at the start of a `generic_parameter` is always the modifier, never the parameter name, so `[reified]` is an error. Error: `syntax-error`.
2. r[grammar.generic.reified-name] A parameter named reified is written `` [`reified`] ``.
3. r[grammar.generic.variance] Variance markers are valid on generic type declarations, not function generic parameters.
4. r[grammar.generic.reified-and-packs] `reified` and type packs are valid on function, method, variant, and generic-implementation parameters, not generic type declarations.

See also: [Keywords And Reserved Words](01-lexical-structure.md#keywords-and-reserved-words).

## Types

```ebnf
type = reference_access_type, { "?" }
     | function_type
     ;

reference_access_type = [ "mut" ], reference_type ;

reference_type = named_type
               | "Self"
               | tuple_type
               | grouped_type
               | associated_type_projection
               | context_type
               ;

named_type = qualified_name, [ bound_type_arguments ] ;
type_arguments = "[", type_argument,
                 { ",", type_argument }, [ "," ], "]" ;
type_argument = type, [ "..." ]
              | row_type_argument
              ;
row_type_argument = "$", requirement_row ;

tuple_type = "(", ")"
           | "(", type_element, ",",
             [ type_element, { ",", type_element }, [ "," ] ], ")"
           | "(", type, "...", ")"
           ;
type_element = type, [ "..." ] ;

grouped_type = "(", type, ")" ;

function_type = "fn", [ "!" ], "(", [ type_list ], ")",
                "->", type, [ requirement_clause ] ;
type_list = type_element, { ",", type_element }, [ "," ] ;

result_type = reference_access_type, { "?" }
            | result_function_type
            ;
result_function_type = "fn", [ "!" ], "(", [ type_list ], ")",
                       "->", result_type ;

associated_type_projection = ( qualified_name | "Self" ), "::", identifier ;

qualified_name = identifier, { ".", identifier } ;

requirement_clause = "$", requirement_row ;
requirement_row = requirement_list
                | "(", ")"
                ;
requirement_list = requirement_key, { "+", requirement_key } ;
requirement_key = bound_trait_type ;
```

### Requirement Clauses

1. r[grammar.type.row.plus-keys] A requirement row lists separate requirement keys joined by `+`, as in `$ Db + Cache`.
2. r[grammar.type.row.single] A single key is written alone, as in `$ Console`.
3. r[grammar.type.row.empty] `$()` is the empty row.
4. r[grammar.type.row.one-form] Every position writes a row the same way: a header, a bodyless trait method, a function type, a row type argument, and `$.Context[...]`.
5. r[grammar.type.row.one-form.examples] The header `fn load(id: UserId) -> User $ Db + Cache:` and the type `fn(UserId) -> User $ Db + Cache` write one row.
6. r[grammar.type.row.no-mut-key] A requirement key has no `mut` prefix, so `$ R + mut Logger` is an error. Error: `syntax-error`.
7. r[grammar.type.row.no-parentheses] Parentheses never surround a nonempty row, so the former `$(A + B)` is an error. Error: `syntax-error`.
8. r[grammar.type.row.in-type.comma] Inside a type, a comma after a key ends the row, so `fn f(cb: fn() -> i32 $ A, B) -> i32:` is an error. Error: `syntax-error`.
9. r[grammar.type.row.key-binding] A requirement key may end its bracketed arguments with associated type bindings, as a bound does: `$ Store[Item = User]`.
10. r[grammar.type.row.key-binding.everywhere] Every place that names a key takes the same form, including `$.use`, `$.with`, `$.context`, and `$.Context[...]`. In a provider entry the binding's `=` sits inside the brackets, as in `$.with(Store[Item = User]=store)`.

```text
trait Clock

trait Logger

fn grouped() -> void $(Clock + Logger): pass  # error: syntax-error

fn run(callback: fn() -> void $ Clock, Logger) -> void: pass  # error: syntax-error
```

### Row Operators

1. r[grammar.type.row.plus-only] `+` is the only row operator: it joins keys into one row.
2. r[grammar.type.row.plus-rows-only] `+` has no bound meaning; several bounds on one type are joined with `&`, as in `T < A & B`.
3. r[grammar.type.row.old-separator] A comma between requirement keys, as in the former `$ A, B` or `$(A, B)`, is an error whose fix-it writes `A + B`. Error: `old-row-separator`.
4. r[grammar.type.row.no-subtraction] A `-` between requirement keys, as in the former `$ R - K`, is an error. Error: `syntax-error`.
5. r[grammar.type.row.extension] Removing a key from a callback row is written by extension instead, as [Requirement Polymorphism](11-requirements-and-suspension.md#requirement-polymorphism) specifies.

```text
trait Clock

trait Logger

fn run(callback: fn() -> void $ Clock + Logger) -> void $ Clock, Logger: callback()  # error: old-row-separator

fn observed(callback: fn() -> void $(Clock, Logger)) -> void: pass  # error: old-row-separator

fn drop_logger[R](callback: fn() -> void $ R) -> void $ R - Logger: callback()  # error: syntax-error
```

### Row Type Arguments

1. r[grammar.type.row-argument] For a row-kinded generic parameter, a type argument may be a row after `$`, as in `Fn[(), void, $ Logger + Clock]`, or `$()` for the empty row.
2. r[grammar.type.row-argument.key] A single requirement key is syntactically also a type; the parameter kind selects its interpretation.
3. r[grammar.type.row-argument.alias] A bare name there that names a row alias is that alias's row, as [`req.row.alias.bare`](11-requirements-and-suspension.md#r-req.row.alias.bare) states.
4. r[grammar.type.row-argument.kind] Using a row argument for a type-kinded parameter, or a type for a row-kinded one, is an error. Error: `generic-kind-mismatch`.

```text
data Box[T]:
    value: T

fn invalid(value: Box[$()]) -> void: pass  # error: generic-kind-mismatch
```

### Modifiers, Optionality, And Grouping

1. r[grammar.type.mut] `mut` is a type modifier.
2. r[grammar.type.mut.semantic] Semantic rules reject meaningless or nested forms, including direct `mut mut T`.
3. r[grammar.type.mut.no-function] `mut` never directly precedes `fn`: `mut fn() -> i32` as a type, or `mut fn() -> i32:` as a closure header, is an error. Error: `syntax-error`.
4. r[grammar.type.optional] Optionality applies to the complete reference access type and may be nested.
5. r[grammar.type.optional.function] In `fn() -> T?`, `?` belongs to the innermost result type; an optional function type must be grouped, as in `(fn() -> T)?`.
6. r[grammar.type.group] Parentheses group types; unlike a one-element tuple type, grouping has no trailing comma.
7. r[grammar.type.row-owner] Inside a type, such as a parameter type, a field type, or a type argument, a requirement clause following nested function types likewise belongs to the innermost ungrouped function type.
8. r[grammar.type.row-owner.grouped] Parentheses select an outer owner.

### Header Requirement Clauses

A header owns the requirement clause at its end, so a function-typed result
with its own row is parenthesized:

```text
fn make() -> fn() -> i32 $ Console + Log:            # make requires both keys
    _ := $.use(Console, Log)
    fn() -> i32: 1

fn wrap() -> (fn() -> i32 $ Log + Trace) $ Console:  # the result requires both
    _ := $.use(Console)
    fn() -> i32 $ Log + Trace:
        _ := $.use(Log, Trace)
        2
```

1. r[grammar.type.header.owner] A declaration or closure header owns the requirement clause directly before its `:`.
2. r[grammar.type.header.trait-method] A bodyless trait method owns the clause directly before its line end.
3. r[grammar.type.header.result] Its result is a `result_type`, whose function types, however nested, carry no requirement clause, so the clause cannot attach to the result.
4. r[grammar.type.header.parenthesized-result] A function-typed result with its own row is parenthesized.
5. r[grammar.type.header.applies] The rule applies to named functions, methods, trait methods, and closures: in `fn() -> fn() -> i32 $ Console:`, the closure requires `Console`.
6. r[grammar.type.header.two-clauses] Writing `fn() -> i32 $ Log $ Console` as a declaration result is an error. Error: `syntax-error`.

```text
trait Log

fn make() -> fn() -> i32 $ Log $ Console:  # error: syntax-error
    fn() -> i32 $ Log: 1
```

See also: [Requirements and Suspension](11-requirements-and-suspension.md).

## Use Declarations

A use declaration imports a path or a group of names:

```ebnf
use_decl = "use", use_path, [ "as", identifier ], NEWLINE
         | [ "pub" ], "use", use_path, ".", use_group, NEWLINE
         ;

use_path = use_root, { ".", identifier } ;
use_root = "pkg"
         | "std"
         | "dep", ".", identifier
         | "self"
         | "super", { ".", "super" }
         | "tests"
         ;

use_group = "{", use_item, { ",", use_item }, [ "," ], "}" ;
use_item = identifier, [ "as", identifier ] ;
```

1. r[grammar.use.no-import-export] `import` and `export` are not declaration keywords.
2. r[grammar.use.old-import] A legacy `import path` form must be diagnosed. Error: `old-import-declaration`.
3. r[grammar.use.old-export] A legacy `export path` form must be diagnosed. Error: `old-export-declaration`.
4. r[grammar.use.contextual-words] `pkg`, `std`, `dep`, and `super` are contextual use-root words, and `as` is contextual before an alias.
5. r[grammar.use.needs-root] `use` begins a use declaration only when a use root follows it.
6. r[grammar.use.self-root] The reserved word `self` also acts as a relative use root.
7. r[grammar.use.tests-root] The reserved word `tests` also acts as a use root, which names integration test modules, as [Use Roots](10-modules.md#use-roots) specifies.

```text
import pkg.user.types.{User}  # error: old-import-declaration
export pkg.user.types.{User}  # error: old-export-declaration
```

See also: [Keywords And Reserved Words](01-lexical-structure.md#keywords-and-reserved-words).

## Expressions

The expression grammar is ordered from lowest to highest precedence.

```ebnf
expression = binding_expression ;

binding_expression = identifier, ":=", binding_expression
                   | conditional_expression
                   ;

conditional_expression = if_expression
                       | for_expression
                       | while_expression
                       | match_expression
                       | closure_expression
                       | context_scope
                       | logical_or_expression
                       ;

suite_expression = statement_if_expression
                 | statement_for_expression
                 | statement_while_expression
                 | statement_match_expression
                 | closure_expression
                 | context_scope
                 ;

closed_expression = identifier, ":=", closed_expression
                  | logical_or_expression
                  ;

inline_expression = identifier, ":=", inline_expression
                  | inline_suite_expression
                  | logical_or_expression
                  ;

continued_expression = identifier, ":=", continued_expression
                     | continued_conditional_expression
                     ;

continued_conditional_expression = indented_suite_expression
                                 | logical_or_expression
                                 ;

indented_suite_expression = indented_if_expression
                          | indented_for_expression
                          | indented_while_expression
                          | match_expression
                          | "$", ".", "with", "(", context_entries, ")",
                            ":", indented_suite_body
                          ;

inline_suite_expression = inline_for_expression
                        | inline_while_expression
                        | inline_closure_expression
                        | inline_context_scope
                        ;

logical_or_expression = logical_and_expression,
                        { "||", logical_and_expression } ;
logical_and_expression = comparison_expression,
                         { "&&", comparison_expression } ;

comparison_expression = pipe_expression,
                        [ comparison_operator, pipe_expression ] ;
comparison_operator = "==" | "!=" | "<" | "<=" | ">" | ">=" | "is" ;

pipe_expression = bitwise_or_expression,
                  { "|>", bitwise_or_expression } ;

bitwise_or_expression = bitwise_xor_expression,
                        { "|", bitwise_xor_expression } ;
bitwise_xor_expression = bitwise_and_expression,
                         { "^", bitwise_and_expression } ;
bitwise_and_expression = shift_expression,
                         { "&", shift_expression } ;
shift_expression = additive_expression,
                   { ( "<<" | ">>" ), additive_expression } ;
additive_expression = multiplicative_expression,
                      { ( "+" | "-" ), multiplicative_expression } ;
multiplicative_expression = unary_expression,
                            { ( "*" | "/" | "%" ), unary_expression } ;

unary_expression = ( "+" | "-" | "~" | "!" ), unary_expression
                 | power_expression
                 ;
power_expression = postfix_expression, [ "**", unary_expression ] ;

postfix_expression = primary_expression, { postfix_suffix } ;
postfix_suffix = ".", identifier, [ function_type_arguments ]
               | "[", expression, "]"
               | argument_clause
               | suspension_call_suffix
               | "?"
               ;
suspension_call_suffix = "!", [ function_type_arguments ], argument_clause ;
```

### Precedence And Associativity

1. r[grammar.expr.binding] `:=` is right-associative and has the lowest precedence.
2. r[grammar.expr.no-comparison-chain] Comparisons do not chain, so `a < b < c` is an error. Error: `comparison-chaining`.
3. r[grammar.expr.power] Exponentiation is right-associative.
4. r[grammar.expr.power.unary] The right operand of `**` may therefore begin with a unary operator.
5. r[grammar.expr.pipe] `|>` is left-associative and binds more tightly than comparison and more loosely than `|`.
6. r[grammar.expr.pipe.step] Each pipe step is a `bitwise_or_expression`, so a step with a comparison, `&&`, `||`, or a control-flow expression needs parentheses.

```text
inside := 0 < value < 10  # error: comparison-chaining
```

### Multi-Name Bindings

1. r[grammar.expr.multi-binding] A multi-name short binding such as `(a, b) := value` is a statement.
2. r[grammar.expr.multi-binding.wrapped] Used as a nested expression, including inside any delimiter, the complete binding goes in its own parentheses: `((a, b) := value)`. Error: `multi-binding-needs-parentheses`.
3. r[grammar.expr.multi-binding.no-grouped] The former grouped form `(a, b := value)`, whose `:=` stands inside the parentheses of the names, is an error, never a tuple whose final element is a binding. Error: `syntax-error`.
4. r[grammar.expr.multi-binding.no-grouped.fix] Its fix-it writes `(a, b) := value`, inside its own parentheses where the binding is nested.
5. r[grammar.expr.multi-binding.tuple-element] A tuple that contains a binding must parenthesize that element separately, as in `(a, (b := value))`.

```text
fn pair() -> (i32, i32): (1, 2)

values := [a, b := pair()]  # error: multi-binding-needs-parentheses
wrapped := [(a, b) := pair()]  # error: multi-binding-needs-parentheses
whole := ((low, high) := pair())  # valid
grouped := (first, second := pair())  # error: syntax-error
```

> **Why.** One shape reads better than two spellings. A statement and a
> nested use both write the names as `(a, b)` before `:=`.

### Bang And Dot Tokens

1. r[grammar.expr.bang-suffix] `!(` or `![` after a completed operand begins a suspension call suffix at ordinary call precedence.
2. r[grammar.expr.prefix-not] A `!` at the start of an operand is the prefix logical-not operator of `unary_expression`, so `!fetch!(id)` negates a suspending call's result.
3. r[grammar.expr.not-equal] `!=` is a single token by longest match: `f!=g` is the comparison `f != g`.
4. r[grammar.expr.member-identifier] A member suffix takes an identifier after `.`, so the tuple selection `t._0._1` is two member suffixes.
5. r[grammar.expr.no-numeric-member] An integer or floating-point literal after `.` forms no suffix, so `t.0` and `t.0.1` are errors. Error: `syntax-error`.

```text
first := pair.0       # error: syntax-error
second := nested.0.1  # error: syntax-error
```

### Method Type Arguments

1. r[grammar.expr.method-type-arguments] After member resolution, brackets immediately following a generic method name are parsed as `function_type_arguments`, not as an indexing suffix.
2. r[grammar.expr.method-type-arguments.valid] An explicit method type-argument list is valid only when the selected member is generic and the expression proceeds to an ordinary call.
3. r[grammar.expr.method-type-arguments.bang] A bang call writes the `!` on the name and the list after it, as the declaration `fn all![Ts...](...)` does: the calls are `all![i32, string](a, b)`, `parser.load![User](text)`, and `Store::load![User](key)`.

```text
data Identity: pass

impl Identity:
    fn echo![Value](value: Value) -> Value:
        value

fn run!() -> i32:
    Identity::echo[i32]!(42)  # error: syntax-error
```

### Primary Expressions

```ebnf
primary_expression = literal
                   | "self"
                   | string_expression
                   | generic_function_reference
                   | qualified_name
                   | contextual_variant_expression
                   | trait_qualified_call
                   | method_reference
                   | context_use
                   | context_create
                   | pack_map_expression
                   | grouped_binding_expression
                   | tuple_or_group_expression
                   | list_expression
                   | map_expression
                   | data_expression
                   | pipe_placeholder
                   | "pass"
                   ;

pipe_placeholder = "_" ;

generic_function_reference = qualified_name, function_type_arguments ;
function_type_arguments = "[", function_type_argument,
                          { ",", function_type_argument }, [ "," ], "]" ;
function_type_argument = type_argument | "_" ;
contextual_variant_expression = ".", identifier ;
trait_qualified_call = trait_type, "::", identifier,
                       ( [ function_type_arguments ], argument_clause
                       | suspension_call_suffix ) ;

method_reference = trait_type, "::", identifier,
                   [ function_type_arguments ] ;

pack_map_expression = "pack", ".", ( "map" | "map_list" ), "(",
                      expression, ",", qualified_name,
                      { ",", expression }, [ "," ], ")" ;

grouped_binding_expression = "(", binding_list, ":=",
                             binding_expression, ")" ;

literal = boolean_literal
        | suffixed_literal
        | float_literal
        | integer_literal
        | char_literal
        ;

string_expression = interpreted_string_expression
                  | interpreted_multiline_string_expression
                  | prefixed_string_expression
                  ;

interpreted_string_expression = '"', { string_segment }, '"' ;
interpreted_multiline_string_expression = '"""',
                                          { multiline_string_segment },
                                          '"""' ;
string_segment = string_text
               | escape_sequence
               | "$", identifier
               | "$", "self"
               | "${", expression, "}"
               ;
multiline_string_segment = multiline_string_text
                         | escape_sequence
                         | "$", identifier
                         | "$", "self"
                         | "${", expression, "}"
                         ;
prefixed_string_expression = string_prefix, '"',
                             { prefixed_string_segment }, '"'
                           | string_prefix, '"""',
                             { prefixed_multiline_segment }, '"""' ;
prefixed_string_segment = prefixed_string_character
                        | "$", identifier
                        | "$", "self"
                        | "${", expression, "}"
                        ;
prefixed_multiline_segment = prefixed_multiline_character
                           | "$", identifier
                           | "$", "self"
                           | "${", expression, "}"
                           ;

tuple_or_group_expression = "(", ")"
                          | "(", expression, ")"
                          | "(", tuple_element, ",",
                            [ tuple_element, { ",", tuple_element }, [ "," ] ], ")"
                          | "(", continued_expression, "...", ")"
                          ;
tuple_element = conditional_expression
              | continued_conditional_expression, "..."
              ;

list_expression = "[", [ list_items ], "]"
                | list_comprehension
                ;
list_items = list_item, { ",", list_item }, [ "," ] ;
list_item = expression
          | continued_expression, "..."
          ;

map_expression = "{", [ map_items ], "}"
               | map_comprehension
               ;
map_items = map_item, { ",", map_item }, [ "," ] ;
map_item = continued_expression, ":", expression ;

data_expression = named_type, "{", [ data_items ], "}" ;
data_items = [ "...", expression, "," ],
             data_field_item, { ",", data_field_item }, [ "," ]
             | "...", expression, [ "," ]
             ;
data_field_item = identifier, ":", [ "..." ], expression ;
```

#### Pipe Placeholder

1. r[grammar.primary.pipe-placeholder] `_` in expression position is the pipe placeholder.
2. r[grammar.primary.pipe-placeholder.semantic] The grammar accepts it as any primary expression; [Pipe Expressions](05-expressions.md#pipe-expressions) limits it to pipe steps.

#### Copies In Data Expressions

1. r[grammar.primary.field-copy] A `...` after a field label copies the value into an embedded field.
2. r[grammar.primary.field-copy.required] A field-label `...` is required for an embedded field and invalid for any other field, which the checker diagnoses.
3. r[grammar.primary.prefix-copy-meaning] A prefix `...` in a data expression, whether it begins a copy-update spread or follows a field label, always means "copy the named members of this value".

See also: [Data Embedding](08-data-and-enums.md#data-embedding).

#### Reflection

1. r[grammar.primary.no-reflection-syntax] Declaration reflection has no dedicated syntax.
2. r[grammar.primary.shape-intrinsics] `shape[User]()` and `shape_of(get_user)` are ordinary calls to prelude intrinsics specified in [Shape Intrinsics](14-annotations.md#shape-intrinsics).

#### Suffixed Literals

1. r[grammar.primary.suffixed-literal] A `suffixed_literal` token is a primary expression, which stands for a call as [Literal Suffixes](05-expressions.md#literal-suffixes) specifies.
2. r[grammar.pattern.no-suffixed-literal] `literal_pattern` does not admit a suffixed literal, so `5s` in a pattern is an error. Error: `syntax-error`.

```text
fn describe(count: i32) -> string:
    match count:
        5px => "five"  # error: syntax-error
        _ => "other"
```

See also: [Literal Suffixes](01-lexical-structure.md#literal-suffixes).

#### Prefixed Strings

1. r[grammar.primary.prefixed-string] A `prefixed_string_expression` is a primary expression, which stands for a call as [Prefixed Strings](05-expressions.md#prefixed-strings) specifies.
2. r[grammar.pattern.no-prefixed-string] `literal_pattern` does not admit a prefixed string, so `r"a"` in a pattern is an error. Error: `syntax-error`.

```text
fn describe(text: string) -> string:
    match text:
        r"a" => "letter"  # error: syntax-error
        _ => "other"
```

See also: [Prefixed Strings](01-lexical-structure.md#prefixed-strings).

#### Forms Resolved By Name

1. r[grammar.primary.resolution] Name resolution distinguishes a data expression from a map expression and an enum variant selection from ordinary field access.
2. r[grammar.primary.generic-reference] Name resolution also distinguishes a named generic-function reference from indexing: in `first[string](names)`, the bracketed form is parsed as function type arguments because `first` resolves to a named generic function.
3. r[grammar.primary.preserve-ambiguity] A parser may preserve this syntactic ambiguity until name resolution.
4. r[grammar.primary.function-type-argument] Each function type argument is a type, a type-pack expansion, or the inference placeholder `_`.
5. r[grammar.primary.placeholder] The placeholder is not part of ordinary `type_arguments` and therefore cannot occur in a type such as `List[_]`.
6. r[grammar.primary.qualified-type-arguments] In a qualified call such as `Type::name[T](...)`, `Trait::name[T](...)`, or `Type::name![T](...)`, type arguments of the qualifying type or trait stay before `::`, as in `Add[Money]::add`.
7. r[grammar.primary.member-type-arguments] Method-level type arguments follow the member name, as in the dot call `parser.parse[User](text)`.
8. r[grammar.primary.member-type-arguments.rules] Name resolution treats that bracket like any other generic reference: it is valid only when the selected member is generic, and it follows the explicit-list rules of [Generic Functions](07-functions.md#generic-functions).
9. r[grammar.primary.pack-map] The token sequences `pack . map (` and `pack . map_list (` always select `pack_map_expression`, even when a local or parameter named `pack` is in scope; a raw identifier `` `pack` `` never does.

#### Prefix And Suffix `...`

1. r[grammar.primary.list-spread] A list element ending in `...` is a spread that expands a list's elements in place.
2. r[grammar.primary.ellipsis-positions] The two positions of `...` never overlap.
3. r[grammar.primary.prefix-copies] A prefix `...` always copies: it copies the named members of a value in a copy-update spread and after an embedded field label, and `...=` stores a copy into an embedded field.
4. r[grammar.primary.suffix-spreads] A suffix `...` always spreads: it expands the elements or entries of its operand in arguments, list elements, tuple elements, pack expansions, and provider-context entries, as in `$.with(ctx...)`.
5. r[grammar.primary.prefix-elsewhere] A prefix `...` anywhere else, including before a provider-context entry, is an error. Error: `syntax-error`.

```text
trait Tag:
    fn name(self) -> string

fn widen(base: $.Context[Tag]) -> $.Context[Tag]:
    $.context(...base)  # error: syntax-error
```

See also: [List And Map Expressions](05-expressions.md#list-and-map-expressions).

#### Member References

1. r[grammar.primary.method-reference] A `::` member without an argument clause, such as `User::domain`, `Json::decode[User]`, or `user::domain`, is a `method_reference`.
2. r[grammar.primary.method-reference.meaning] [Method References](07-functions.md#method-references) gives its meaning; with an argument clause, the same form is a call.
3. r[grammar.primary.method-reference.no-bang] A reference with type arguments directly followed by `!(` is not a bang call of that reference: `Identity::echo[i32]!(42)` is an error, and the call is `Identity::echo![i32](42)`. Error: `syntax-error`.

### Calls And Arguments

```ebnf
argument_clause = "(", [ argument_list ], ")" ;
argument_list = positional_argument, { ",", positional_argument },
                [ ",", named_argument, { ",", named_argument } ], [ "," ]
              | named_argument, { ",", named_argument }, [ "," ]
              ;

positional_argument = expression
                    | continued_expression, "..."
                    ;
named_argument = identifier, "=", expression ;
```

1. r[grammar.call.positional-first] Positional arguments, including positional spreads, must precede named arguments. Error: `argument-order`.
2. r[grammar.call.named-vararg] A named vararg receives an ordinary list value and does not use spread syntax.
3. r[grammar.call.spread-final] Semantic rules require a positional spread to be the final positional argument and to feed a declared vararg parameter.

```text
resize(width=640, 480)  # error: argument-order
```

#### Trailing Blocks

A call whose final parameter is a zero-argument function may use an indented
trailing block as a complete statement or as the complete right-hand side of
`:=`, `let ... =`, `=`, `_ :=`, `return`, or `break`:

```ebnf
trailing_block_call = postfix_expression, ":", indented_suite_body ;
indented_suite_body = NEWLINE, INDENT, statement, { statement }, DEDENT ;
```

1. r[grammar.call.trailing-block] A call whose final parameter is a zero-argument function may use an indented trailing block as a complete statement or as the complete right-hand side of `:=`, `let ... =`, `=`, `_ :=`, `return`, or `break`.
2. r[grammar.call.trailing-block.no-arguments] When there are no ordinary arguments, the call omits `()`, as in `transaction:`.
3. r[grammar.call.trailing-block.accepted] This production is accepted only at delimiter depth zero when the call is the complete statement or one of those complete right-hand sides.
4. r[grammar.call.trailing-block.eligible] It is also accepted only when name and type resolution identify a callable with an eligible final parameter.
5. r[grammar.call.trailing-block.next-line] Its body must begin on the next logical line.
6. r[grammar.call.trailing-block.not-header] It is not accepted in an `if`, `while`, `for`, or `match` header or inside brackets. A trailing block there is an error. Error: `trailing-block-position`.

```text
fn run(callback: fn() -> i32) -> i32: callback()

values := [run:  # error: trailing-block-position
    1]
```

### Closures

```ebnf
closure_expression = closure_header, suite_body ;
inline_closure_expression = closure_header, inline_suite_body ;
closure_header = "fn", [ "!" ], closure_parameter_clause,
                 [ "->", result_type ], [ requirement_clause ], ":" ;

closure_parameter_clause = "(", [ closure_parameter_list ], ")" ;
closure_parameter_list = closure_parameter,
                         { ",", closure_parameter }, [ "," ] ;
closure_parameter = identifier, [ ":", type ] ;
```

1. r[grammar.closure.parameter-types] Omitted closure parameter types require an expected function type.
2. r[grammar.closure.result-type] A nonrecursive closure may infer its result type from its body.
3. r[grammar.closure.annotations] A standalone or otherwise ambiguous closure must provide enough annotations to determine its complete function type.

## Control-Flow Expressions

```ebnf
if_expression = "if", continued_expression, ":", suite_body,
                { "else", "if", continued_expression, ":", suite_body },
                [ "else", ":", suite_body ]
                ;

for_expression = "for", binding_pattern, "in", continued_expression, ":",
                 suite_body, [ "else", ":", suite_body ]
                 ;

while_expression = "while", continued_expression, ":", suite_body,
                   [ "else", ":", suite_body ]
                   ;

indented_if_expression = "if", continued_expression, ":", indented_suite_body
                       | "if", continued_expression, ":", suite_body,
                         { "else", "if", continued_expression, ":",
                           suite_body },
                         "else", [ "if", continued_expression ], ":",
                         indented_suite_body
                       ;

indented_for_expression = "for", binding_pattern, "in", continued_expression,
                          ":", ( indented_suite_body
                               | suite_body, "else", ":",
                                 indented_suite_body )
                          ;

indented_while_expression = "while", continued_expression, ":",
                            ( indented_suite_body
                            | suite_body, "else", ":", indented_suite_body )
                            ;

inline_for_expression = "for", identifier, "in", closed_expression, ":",
                        inline_suite_body, [ "else", ":", inline_suite_body ]
                        ;

inline_while_expression = "while", closed_expression, ":", inline_suite_body,
                          [ "else", ":", inline_suite_body ]
                          ;

match_expression = "match", continued_expression, ":", NEWLINE, INDENT,
                   match_arm, { match_arm }, DEDENT
                   ;

statement_if_expression = "if", closed_expression, ":", suite_body,
                          { "else", "if", closed_expression, ":",
                            suite_body },
                          [ "else", ":", suite_body ]
                          ;

statement_for_expression = "for", binding_pattern, "in", closed_expression,
                           ":", suite_body, [ "else", ":", suite_body ]
                           ;

statement_while_expression = "while", closed_expression, ":", suite_body,
                             [ "else", ":", suite_body ]
                             ;

statement_match_expression = "match", closed_expression, ":", NEWLINE,
                             INDENT, match_arm, { match_arm }, DEDENT
                             ;

match_arm = pattern, [ "if", closed_expression ], "=>", arm_body ;
arm_body = suite_expression
         | simple_statement, NEWLINE
         | NEWLINE, INDENT, statement, { statement }, DEDENT
         ;
```

1. r[grammar.flow.for-in-brackets] A `for` loop is an expression, so it may also appear inside brackets, as in `[for x in xs: body]` or `(for k, v in m: body)`.
2. r[grammar.flow.if-else] An `if` used where a value is required must have an `else`; statement-position `if` may omit it.
3. r[grammar.flow.loop-void] A loop without `else` has type `void`.
4. r[grammar.flow.semantic] The `if` and loop rules above are semantic rules, not separate grammar productions.

## Patterns

```ebnf
pattern = "_"
        | literal_pattern
        | binding_pattern_atom
        | variant_pattern
        | data_pattern
        | tuple_pattern
        ;

literal_pattern = boolean_literal
                | [ "-" ], ( integer_literal | float_literal )
                | string_literal
                | char_literal
                ;

binding_pattern_atom = identifier ;

variant_pattern = qualified_variant_name, [ pattern_argument_clause ]
                | ".", identifier, [ pattern_argument_clause ]
                | identifier, pattern_argument_clause
                ;
qualified_variant_name = identifier, ".", identifier,
                         { ".", identifier } ;
pattern_argument_clause = "(", [ pattern_argument_list ], ")" ;
pattern_argument_list = positional_pattern,
                        { ",", positional_pattern },
                        [ ",", named_pattern, { ",", named_pattern } ], [ "," ]
                      | named_pattern, { ",", named_pattern }, [ "," ]
                      ;
positional_pattern = pattern ;
named_pattern = identifier, "=", pattern ;

data_pattern = qualified_name, "{", [ data_pattern_fields ], "}" ;
data_pattern_fields = data_pattern_field,
                      { ",", data_pattern_field }, [ "," ] ;
data_pattern_field = identifier, [ ":", pattern ] ;

tuple_pattern = "(", pattern, ",",
                [ pattern, { ",", pattern }, [ "," ] ], ")" ;
```

1. r[grammar.pattern.variant] Variant patterns may use a qualified enum variant name or `.Variant` when the matched value's type supplies one enum.
2. r[grammar.pattern.bare-variant] The unqualified `identifier, pattern_argument_clause` form parses so that a checker can report it as an error. Error: `bare-variant-pattern`.
3. r[grammar.pattern.positional-names] Positional binding names need not match payload field names.
4. r[grammar.pattern.named] In a payload list, only `field=pattern` is a named pattern.
5. r[grammar.pattern.named-last] No positional pattern may follow a named pattern. Error: `pattern-order`.
6. r[grammar.pattern.data-field] In a data pattern, bare `field` binds that field's value to a new name of the same spelling.
7. r[grammar.pattern.data-field.nested] `field: pattern` matches the field against a nested pattern, and `field: name` binds it to `name`.
8. r[grammar.pattern.data-unlisted] Unlisted fields are ignored.

```text
enum Pair:
    Values(left: i32, right: i32)

fn value_or_zero(value: i32?) -> i32:
    match value:
        Some(number) => number  # error: bare-variant-pattern
        .None => 0

fn invalid(value: Pair) -> i32:
    match value:
        Pair.Values(left=l, r) => l + r  # error: pattern-order
```

See also: [Match Expressions](06-control-flow.md#match-expressions).

### Labels

1. r[grammar.label.bracket] Labels follow their brackets.
2. r[grammar.label.braces] Inside `Type { ... }`, in data expressions and data patterns alike, a field label is followed by `:`.
3. r[grammar.label.parentheses] Inside parentheses, in named arguments, variant payloads, and payload patterns, a label is followed by `=`.
4. r[grammar.label.data-pattern-equals] A data pattern written with `field = pattern` is an error. Error: `syntax-error`.

```text
data Point:
    x: i32
    y: i32

fn first(p: Point) -> i32:
    match p:
        Point { x = 0, y } => y  # error: syntax-error
        Point { x, y: _ } => x
```

## Comprehensions

```ebnf
list_comprehension = "[", comprehension_clauses, "=>", expression, "]" ;

map_comprehension = "{", comprehension_clauses, "=>",
                    continued_expression, ":", expression, "}" ;

comprehension_clauses = comprehension_for,
                        { comprehension_for | comprehension_if } ;
comprehension_for = "for", binding_pattern, "in", continued_expression ;
comprehension_if = "if", continued_expression ;
```

1. r[grammar.comprehension.first-for] The first clause must be `for`.
2. r[grammar.comprehension.order] Later `for` and `if` clauses execute from left to right.
3. r[grammar.comprehension.no-let] Comprehensions do not have a `let` clause; `:=` binding expressions may be used inside guards or result expressions.

## Requirements And Provider Contexts

```ebnf
context_use = "$", ".", "use", "(", requirement_key,
              { ",", requirement_key }, [ "," ], ")" ;

context_create = "$", ".", "context", "(", context_entries, ")" ;
context_type = "$", ".", "Context", "[",
               ( requirement_key | row_type_argument ), "]" ;
context_scope = "$", ".", "with", "(", context_entries, ")",
                ":", suite_body ;
inline_context_scope = "$", ".", "with", "(", context_entries, ")",
                       ":", inline_suite_body ;

context_entries = context_entry, { ",", context_entry }, [ "," ] ;
context_entry = requirement_key, "=", expression
              | continued_expression, "..."
              ;
```

1. r[grammar.row.sets] Requirement rows denote unordered sets of keys after name resolution.
2. r[grammar.row.parameter] A generic identifier used as a complete requirement key is a row parameter.
3. r[grammar.row.parameter.extension] Listing a row parameter beside other keys extends that row with them.

## Annotations

```ebnf
decorated_decl = decorator_line, { decorator_line },
                 ( [ "pub" ], ( data_decl | enum_decl | function_decl
                              | trait_decl | newtype_decl )
                 | impl_decl ) ;

decorator_line = "@", ( derive_decorator | closed_expression ), NEWLINE ;

newtype_decl = "type", identifier, [ type_params ], "(", type, ")", NEWLINE ;

derive_decorator = "derive", "(", qualified_name,
                   { ",", qualified_name }, [ "," ], ")" ;
```

1. r[grammar.annot.item-targets] Decorator lines may precede a data type, enum, function, trait, newtype, or implementation declaration.
2. r[grammar.annot.member-targets] Decorator lines may also precede a data field, an enum variant, and a method of a trait or implementation, as `data_member`, `enum_variant`, `trait_member`, and `impl_member` show.
3. r[grammar.annot.alias-no-decorator] Any decorator before a transparent alias is an error. Error: `syntax-error`.
4. r[grammar.annot.derive-traits] The names in a `derive_decorator` are traits. [Opting In](14-annotations.md#opting-in) defines which traits it accepts.

Member metadata written away from a declaration uses a trait-less
derivation block, an `impl_decl`, as [`grammar.impl.traitless-by`](#r-grammar.impl.traitless-by)
states.

See also: [Typed Derivation](14-annotations.md#typed-derivation).

## Pack Expansion

1. r[grammar.pack.declare] An ellipsis following a generic parameter declares a type pack.
2. r[grammar.pack.expand] In a type, parameter, tuple, or argument position, an ellipsis following a subtree that contains a pack reference expands that subtree once per pack element.
3. r[grammar.pack.vararg] The same token denotes an ordinary homogeneous vararg or list spread when no pack is referenced.
4. r[grammar.pack.resolution] Name and type resolution make the distinction; unresolved or mixed uses are compile-time errors.
