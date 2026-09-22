# Grammar

Status: core specification draft.

This chapter collects the core hd-lang grammar in EBNF. It specifies syntactic
form, not name resolution, typing, exhaustiveness, or runtime behavior.

The grammar consumes the token stream produced by
[Lexical Structure](01-lexical-structure.md). `NEWLINE`, `INDENT`, `DEDENT`,
`SUITE_END`, and `EOF` are abstract layout tokens. Comments do not appear in
this grammar.

Provisional features extend named productions from this chapter. Those
extensions are defined only in their own chapters and are not part of the core
grammar.

## Source Files And Suites

```ebnf
source_file = { NEWLINE | top_level_item }, EOF ;

top_level_item = import_decl
               | export_decl
               | test_decl
               | declaration
               | statement
               ;

suite = simple_suite | block_suite ;
simple_suite = simple_statement, SUITE_END ;
block_suite = NEWLINE, INDENT, statement, { statement }, DEDENT ;
```

A `simple_suite` is the body after a block header when that body appears on the
same physical line and ends at `SUITE_END`. A `block_suite` begins on the
following logical line. An
implementation must reject an empty `block_suite`; use `pass` when an explicit
no-op body is required.

Named declarations, imports, and exports are top-level items. Methods occur
inside trait and implementation declarations through their dedicated grammar
productions; arbitrary declarations do not occur in an executable suite.

## Test Blocks

Unit-test entry points use a module-level named block:

```ebnf
test_decl = "test", string_literal, ":", suite_body ;
```

The string is the test's human-readable name. A test body is an ordinary suite.
Its discovery, execution, and assertion APIs are standard-library and tooling
behavior specified in [`RUNTIME_AND_LIBRARY.md`](../RUNTIME_AND_LIBRARY.md).
`test` blocks are not permitted inside executable suites. `test` is contextual:
at module level it begins a test block only when followed by a string literal;
otherwise it remains an ordinary identifier.

## Statements

```ebnf
statement = suite_statement
          | simple_statement, NEWLINE
          ;

suite_statement = suite_expression
                | binding_pattern, ":=",
                  { binding_pattern, ":=" }, suite_expression
                | "let", binding_pattern, [ ":", type ], "=",
                  suite_expression
                | postfix_expression, "=", suite_expression
                | "return", suite_expression
                | "break", suite_expression
                ;

simple_statement = let_statement
                 | assignment_statement
                 | return_statement
                 | break_statement
                 | continue_statement
                 | expression_statement
                 ;

let_statement = "let", binding_pattern, [ ":", type ], "=", expression ;

assignment_statement = postfix_expression, "=", expression ;

return_statement = "return", [ expression ] ;
break_statement = "break", [ expression ] ;
continue_statement = "continue" ;
expression_statement = expression ;

binding_pattern = identifier, { ",", identifier } ;
```

A `suite_statement` is a statement whose outermost expression owns a suite.
Its final `DEDENT`, or the `SUITE_END` of a same-line suite, terminates the
statement; it does not require another `NEWLINE`. This separate production is
what permits `value := if ...`, `let callback = fn ...`, and similar direct
right-hand-side forms. A suite expression nested inside delimiters remains part
of its enclosing expression, and the enclosing statement ends normally after
the closing delimiter.

Whether a statement may appear in a particular value-producing block is a
semantic rule. In particular, `break` is valid only inside a loop, and `break`
with a value is valid only in a loop with an `else` suite.
The left side of an assignment must resolve to a reassignable local, mutable
field, or mutable indexed place; calls and other non-place postfix expressions
are rejected semantically.

## Declarations

```ebnf
declaration = [ "pub" ], ( function_decl
                         | struct_decl
                         | enum_decl
                         | trait_decl
                         | type_decl )
            | impl_decl
            ;
```

`pub` is not accepted before an `impl` declaration because implementations are
not independently named module members.

### Functions

```ebnf
function_decl = "fn", identifier, [ generic_params ], parameter_clause,
                "->", type, ":", suite_body ;

suite_body = simple_statement, SUITE_END
           | NEWLINE, INDENT, statement, { statement }, DEDENT
           ;

parameter_clause = "(", [ parameter_list ], ")" ;
parameter_list = parameter, { ",", parameter }, [ "," ] ;

parameter = receiver_parameter
          | value_parameter
          ;

value_parameter = identifier, ":", type, [ "=", expression ], [ "..." ] ;

receiver_parameter = "self" | "mut", "self" ;
```

The receiver forms are valid only for methods. A vararg parameter ends in
`...`; it must be the final positional parameter. Default-argument ordering and
purity are semantic constraints defined in [Functions](07-functions.md).

### Structs

```ebnf
struct_decl = "struct", identifier, [ type_params ], ":",
              NEWLINE, INDENT, struct_member, { struct_member }, DEDENT ;

struct_member = struct_field, NEWLINE
              | embedded_field, NEWLINE
              ;

struct_field = identifier, ":", type ;
embedded_field = type_name ;
```

An embedded field must denote a struct type and must not include generic
arguments or `mut` in this core spelling. The type's final name is also its
embedded field name.

### Enums

```ebnf
enum_decl = "enum", identifier, [ type_params ],
            [ enum_parameter_clause ], ":",
            NEWLINE, INDENT, enum_variant, { enum_variant }, DEDENT ;

enum_variant = identifier, [ variant_parameter_clause ],
               [ "->", enum_constructor ], NEWLINE ;

enum_parameter_clause = "(", [ data_parameter_list ], ")" ;
variant_parameter_clause = "(", [ data_parameter_list ], ")" ;
data_parameter_list = data_parameter, { ",", data_parameter }, [ "," ] ;
data_parameter = [ identifier, ":" ], type ;

enum_constructor = qualified_name, argument_clause ;
```

The optional variant result in the core grammar initializes constructor data
shared by every variant, as in `NotFound -> StatusCode(404)`. The provisional
GADT grammar extends the same position with refined result types and
variant-local generic parameters.

### Traits And Implementations

```ebnf
trait_decl = "trait", identifier, [ type_params ],
             ( NEWLINE
             | ":", NEWLINE, INDENT,
               trait_member, { trait_member }, DEDENT
             | ":", supertrait_bounds, ":", NEWLINE, INDENT,
               trait_member, { trait_member }, DEDENT )
             ;

supertrait_bounds = trait_type, { "+", trait_type } ;

trait_member = "fn", identifier, [ generic_params ], method_parameter_clause,
               "->", type,
               ( NEWLINE | ":", suite_body ) ;

impl_decl = "impl", type, [ "for", type ],
            ( NEWLINE
            | ":", NEWLINE, INDENT,
              method_decl, { method_decl }, DEDENT )
            ;

method_decl = "fn", identifier, [ generic_params ],
              method_parameter_clause, "->", type, ":", suite_body ;

method_parameter_clause = "(", receiver_parameter,
                          { ",", value_parameter }, [ "," ], ")" ;
```

`impl T:` is an inherent implementation. `impl Trait for T:` is a trait
implementation. A trait declaration without a body is a marker trait, and a
trait implementation without a body implements such a marker trait. A
bodyless trait method ends at `NEWLINE`; a default method has `:` followed by a
suite. `trait Child: Parent:` declares `Parent` as a supertrait and opens the
body with the second `:`. Every core trait or implementation method has an
explicit `self` or `mut self` receiver as its first parameter.

Generic `impl` parameters and `where` clauses are not supported in v1 and do
not appear in the core grammar.

### Type Declarations

```ebnf
type_decl = "type", identifier, [ type_params ],
            ( "=", type | "(", type, ")" ), NEWLINE ;
```

The `=` form declares a transparent alias. The parenthesized form declares a
nominal single-field newtype.

## Generic Parameters And Bounds

```ebnf
type_params = "[", type_parameter, { ",", type_parameter }, [ "," ], "]" ;
generic_params = "[", generic_parameter,
                 { ",", generic_parameter }, [ "," ], "]" ;

type_parameter = [ variance ], identifier, [ ":", trait_bounds ] ;
generic_parameter = [ "reified" ], identifier, [ ":", trait_bounds ] ;
variance = "+" | "-" ;

trait_bounds = [ "mut" ], trait_type, { "+", trait_type } ;
trait_type = qualified_name, [ type_arguments ] ;
```

Variance markers are valid on generic type declarations, not function generic
parameters. `reified` is valid on function generic parameters, not generic type
declarations. Variadic generic parameters are a provisional extension.

## Types

```ebnf
type = non_optional_type, { "?" } ;

non_optional_type = [ "mut" ], reference_type
                  | function_type
                  ;

reference_type = named_type | tuple_type | grouped_type ;

named_type = qualified_name, [ type_arguments ] ;
type_arguments = "[", type, { ",", type }, [ "," ], "]" ;

tuple_type = "(", ")"
           | "(", type, ",", [ type, { ",", type }, [ "," ] ], ")"
           ;

grouped_type = "(", type, ")" ;

function_type = [ "mut" ], "fn", "(", [ type_list ], ")", "->", type ;
type_list = type, { ",", type }, [ "..." ], [ "," ] ;

type_name = identifier ;
qualified_name = identifier, { ".", identifier } ;
```

`mut` is a type modifier. Semantic rules reject meaningless or nested forms,
including direct `mut mut T`. Optionality applies to the complete access type
and may be nested. Parentheses group types; unlike a one-element tuple type,
grouping has no trailing comma.
Requirement rows on function types are specified provisionally.

## Imports And Exports

```ebnf
import_decl = "import", import_path, [ "as", identifier ], NEWLINE
            | "import", import_path, ".", import_group, NEWLINE
            ;

export_decl = "export", import_path, ".", import_group, NEWLINE ;

import_path = import_root, { ".", identifier } ;
import_root = "pkg"
            | "std"
            | "dep", ".", identifier
            | "self"
            | "super", { ".", "super" }
            ;

import_group = "{", import_item, { ",", import_item }, [ "," ], "}" ;
import_item = identifier, [ "as", identifier ] ;
```

`pkg`, `std`, and `dep` are contextual import-root words. The lexical reserved
words `self` and `super` also act as relative import roots.

## Expressions

The expression grammar is ordered from lowest to highest precedence.

```ebnf
expression = binding_expression ;

binding_expression = binding_pattern, ":=", binding_expression
                   | conditional_expression
                   ;

conditional_expression = if_expression
                       | for_expression
                       | while_expression
                       | match_expression
                       | closure_expression
                       | trailing_block_call
                       | logical_or_expression
                       ;

suite_expression = if_expression
                 | for_expression
                 | while_expression
                 | match_expression
                 | closure_expression
                 | trailing_block_call
                 ;

logical_or_expression = logical_and_expression,
                        { "or", logical_and_expression } ;
logical_and_expression = comparison_expression,
                         { "and", comparison_expression } ;

comparison_expression = bitwise_or_expression,
                        [ comparison_operator, bitwise_or_expression ] ;
comparison_operator = "==" | "!=" | "<" | "<=" | ">" | ">=" ;

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

unary_expression = ( "-" | "~" | "not" ), unary_expression
                 | power_expression
                 ;
power_expression = postfix_expression, [ "**", unary_expression ] ;

postfix_expression = primary_expression, { postfix_suffix } ;
postfix_suffix = ".", ( identifier | integer_literal )
               | "[", expression, "]"
               | argument_clause
               | "?"
               ;
```

`:=` is right-associative and has the lowest precedence. Comparisons do not
chain in v1. Exponentiation is right-associative. The right operand of `**` may
therefore begin with a unary operator.

Suspension-call suffixes are added by the provisional requirements and
suspension grammar.

### Primary Expressions

```ebnf
primary_expression = literal
                   | generic_function_reference
                   | qualified_name
                   | trait_qualified_call
                   | tuple_or_group_expression
                   | list_expression
                   | map_expression
                   | struct_expression
                   | "pass"
                   ;

generic_function_reference = qualified_name, type_arguments ;
trait_qualified_call = trait_type, "::", identifier, argument_clause ;

literal = boolean_literal
        | nil_literal
        | float_literal
        | integer_literal
        | string_literal
        | char_literal
        ;

tuple_or_group_expression = "(", ")"
                          | "(", expression, ")"
                          | "(", expression, ",",
                            [ expression, { ",", expression }, [ "," ] ], ")"
                          ;

list_expression = "[", [ list_items ], "]"
                | list_comprehension
                ;
list_items = expression, { ",", expression }, [ "," ] ;

map_expression = "{", [ map_items ], "}"
               | map_comprehension
               ;
map_items = map_item, { ",", map_item }, [ "," ] ;
map_item = expression, ":", expression ;

struct_expression = named_type, "{", [ struct_items ], "}" ;
struct_items = [ "...", expression, "," ],
               struct_field_item, { ",", struct_field_item }, [ "," ]
             | "...", expression, [ "," ]
             ;
struct_field_item = identifier, ":", expression ;
```

Name resolution distinguishes a struct expression from a map expression and
an enum variant selection from ordinary field access. It also distinguishes a
named generic-function reference from indexing: in `first[string](names)`, the
bracketed form is parsed as type arguments because `first` resolves to a named
generic function. A parser may preserve this syntactic ambiguity until name
resolution, but it must parse every generic argument as a type.

### Calls And Arguments

```ebnf
argument_clause = "(", [ argument_list ], ")" ;
argument_list = positional_argument, { ",", positional_argument },
                [ ",", named_argument, { ",", named_argument } ], [ "," ]
              | named_argument, { ",", named_argument }, [ "," ]
              ;

positional_argument = expression, [ "..." ] ;
named_argument = identifier, "=", expression ;
```

Positional arguments, including positional spreads, must precede named
arguments. A named vararg receives an ordinary list value and does not use
spread syntax. Semantic rules require a positional spread to be the final
positional argument and to feed a declared vararg parameter.

A call whose final parameter is a zero-argument function may use a trailing
block:

```ebnf
trailing_block_call = postfix_expression, ":", suite_body ;
```

When there are no ordinary arguments, the call omits `()`, as in
`transaction:`. This production is accepted only when name and type resolution
identify a callable with an eligible final parameter.

### Closures

```ebnf
closure_expression = [ "mut" ], "fn", closure_parameter_clause,
                     [ "->", type ], ":", suite_body ;

closure_parameter_clause = "(", [ closure_parameter_list ], ")" ;
closure_parameter_list = closure_parameter,
                         { ",", closure_parameter }, [ "," ] ;
closure_parameter = identifier, [ ":", type ] ;
```

Omitted closure parameter and result types require an expected function type.
A standalone or otherwise ambiguous closure must provide enough annotations to
determine its complete function type.

## Control-Flow Expressions

```ebnf
if_expression = "if", expression, ":", suite_body,
                { "else", "if", expression, ":", suite_body },
                [ "else", ":", suite_body ]
                ;

for_expression = "for", binding_pattern, "in", expression, ":", suite_body,
                 [ "else", ":", suite_body ]
                 ;

while_expression = "while", expression, ":", suite_body,
                   [ "else", ":", suite_body ]
                   ;

match_expression = "match", expression, ":", NEWLINE, INDENT,
                   match_arm, { match_arm }, DEDENT
                   ;

match_arm = pattern, "=>", arm_body ;
arm_body = suite_expression
         | expression, NEWLINE
         | NEWLINE, INDENT, statement, { statement }, DEDENT
         ;
```

An `if` used where a value is required must have an `else`; statement-position
`if` may omit it. A loop without `else` has type `void`. These are semantic
rules, not separate grammar productions.

## Patterns

```ebnf
pattern = "_"
        | literal_pattern
        | binding_pattern_atom
        | variant_pattern
        | tuple_pattern
        ;

literal_pattern = boolean_literal
                | nil_literal
                | integer_literal
                | float_literal
                | string_literal
                | char_literal
                ;

binding_pattern_atom = identifier ;

variant_pattern = qualified_name, [ pattern_argument_clause ] ;
pattern_argument_clause = "(", [ pattern_argument_list ], ")" ;
pattern_argument_list = positional_pattern,
                        { ",", positional_pattern },
                        [ ",", named_pattern, { ",", named_pattern } ], [ "," ]
                      | named_pattern, { ",", named_pattern }, [ "," ]
                      ;
positional_pattern = pattern ;
named_pattern = identifier, "=", pattern ;

tuple_pattern = "(", pattern, ",",
                [ pattern, { ",", pattern }, [ "," ] ], ")" ;
```

Variant patterns must use a qualified enum variant name. Positional binding
names need not match payload field names. Only `field=pattern` is a named
pattern, and no positional pattern may follow a named pattern.

## Comprehensions

```ebnf
list_comprehension = "[", comprehension_clauses, "=>", expression, "]" ;

map_comprehension = "{", comprehension_clauses, "=>",
                    expression, ":", expression, "}" ;

comprehension_clauses = comprehension_for,
                        { comprehension_for | comprehension_if } ;
comprehension_for = "for", binding_pattern, "in", expression ;
comprehension_if = "if", expression ;
```

The first clause must be `for`. Later `for` and `if` clauses execute from left
to right. Comprehensions do not have a `let` clause; `:=` binding expressions
may be used inside guards or result expressions.

## Provisional Extension Points

The core grammar reserves these extension points without incorporating their
syntax:

1. [Requirements and Suspension](provisional/requirements-and-suspension.md)
   extends function declarations, function types, callable names, and postfix
   calls.
2. [Variadic Generics](provisional/variadic-generics.md) extends generic
   parameters, types, patterns, parameter lists, and argument lists with packs.
3. [Generalized Algebraic Data Types](provisional/gadts.md) extends enum
   variants and pattern-refined result types.
4. [Annotations](provisional/annotations.md) adds annotation declarations and
   annotation override blocks.
