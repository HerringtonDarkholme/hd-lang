# Grammar

Status: language specification draft.

This chapter collects the hd-lang grammar in EBNF. It specifies syntactic
form, not name resolution, typing, exhaustiveness, or runtime behavior.

The grammar consumes the token stream produced by
[Lexical Structure](01-lexical-structure.md). `NEWLINE`, `INDENT`, `DEDENT`,
`SUITE_END`, and `EOF` are abstract layout tokens. Comments do not appear in
this grammar.

The feature chapters refine the semantic constraints on these productions, but
do not maintain separate extension grammars.

## Source Files And Suites

```ebnf
source_file = { NEWLINE | top_level_item }, EOF ;

top_level_item = import_decl
               | export_decl
               | test_decl
               | annotation_decl
               | decorated_decl
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

Named declarations and implementations may also occur in executable block
suites. Imports, exports, and annotations remain top-level items. Methods occur
inside trait and implementation declarations through their dedicated grammar
productions.

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
          | function_decl
          | data_decl
          | enum_decl
          | trait_decl
          | type_decl
          | impl_decl
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
                         | data_decl
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
function_decl = "fn", callable_name, [ generic_params ], parameter_clause,
                "->", type, [ requirement_clause ], ":", suite_body ;

callable_name = identifier, [ "!" ] ;

suite_body = simple_statement, SUITE_END
           | NEWLINE, INDENT, statement, { statement }, DEDENT
           ;

parameter_clause = "(", [ parameter_list ], ")" ;
parameter_list = parameter, { ",", parameter }, [ "," ] ;

parameter = receiver_parameter
          | { parameter_decorator }, value_parameter
          ;

parameter_decorator = "@", expression ;
value_parameter = identifier, ":", type, [ "=", expression ], [ "..." ] ;

receiver_parameter = "self" | "mut", "self" ;
```

The receiver forms are valid only for methods. Parameter decorators are valid
only on value parameters of module-level named functions. Within a multiline
parameter clause, each decorator may occupy its own prefix line; delimiter
line breaks do not terminate the parameter. A vararg parameter ends in
`...`; it must be the final positional parameter. Default-argument ordering and
purity are semantic constraints defined in [Functions](07-functions.md).

### Data Types

```ebnf
data_decl = "data", identifier, [ type_params ], ":", data_suite ;

data_suite = "pass", SUITE_END
             | NEWLINE, INDENT,
               ( "pass", NEWLINE
               | data_member, { data_member } ), DEDENT
             ;

data_member = { decorator_line }, data_field, NEWLINE
              | embedded_field, NEWLINE
              ;

data_field = [ "pub" ], identifier, ":", type, [ "=", expression ] ;
embedded_field = [ "pub" ], named_type ;
```

`mut` is not a data-member modifier: `mut name: string` and `mut Base` are
invalid. A named field may instead declare a mutable type, as in
`friend: mut User`. An embedded field must denote a data type and must not
include `mut`. It may
instantiate a generic data type. The type's final name, without its type
arguments, is the embedded field name; duplicate embedded names are rejected.
Data-field default expressions have the purity constraint specified in
[Data Types and Enums](08-data-and-enums.md#data-declarations).

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
data_parameter = [ identifier, ":" ], type ;

variant_result = named_type, [ argument_clause ] ;
```

The optional variant result initializes constructor data shared by every
variant, as in `NotFound -> StatusCode(404)`, and may refine the enclosing enum
type as specified by the GADT rules.
Only shared enum constructor parameters may declare defaults. Their ordering
and purity constraints follow function-parameter defaults.

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

trait_member = associated_type_decl
             | "fn", callable_name, [ generic_params ], parameter_clause,
               "->", type, [ requirement_clause ],
               ( NEWLINE | ":", suite_body )
             ;

impl_decl = "impl", [ generic_params ], type, [ "for", type ],
            [ where_clause ],
            ( NEWLINE
            | ":", NEWLINE, INDENT,
              impl_member, { impl_member }, DEDENT )
            ;

impl_member = associated_type_decl | method_decl ;

method_decl = [ "pub" ], "fn", callable_name, [ generic_params ],
              parameter_clause, "->", type, [ requirement_clause ],
              ":", suite_body ;

associated_type_decl = "type", identifier, [ "=", type ], NEWLINE ;

where_clause = "where", where_predicate,
               { ",", where_predicate }, [ "," ] ;
where_predicate = type, ":", trait_bounds ;
```

`impl T:` is an inherent implementation. `impl Trait for T:` is a trait
implementation. A trait declaration without a body is a marker trait, and a
trait implementation without a body implements such a marker trait. A
`pub` method is permitted only in an inherent implementation; trait method
visibility follows the trait. A
bodyless trait method ends at `NEWLINE`; a default method has `:` followed by a
suite. `trait Child: Parent:` declares `Parent` as a supertrait and opens the
body with the second `:`. A function member whose first parameter is `self` or
`mut self` is a method; a receiverless member is an associated function.
Associated type declarations omit `=` in a
trait requirement and provide `= type` in an implementation. Generic
implementations may put bounds inline or in a `where` clause.

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
generic_parameter = [ "reified" ], identifier, [ "..." ],
                    [ ":", trait_bounds ] ;
variance = "+" | "-" ;

trait_bounds = [ "mut" ], trait_type, { "+", trait_type } ;
trait_type = qualified_name, [ type_arguments ] ;
```

Variance markers are valid on generic type declarations, not function generic
parameters. `reified` and type packs are valid on function, method, variant, and
generic-implementation parameters, not generic type declarations.

## Types

```ebnf
type = non_optional_type, { "?" } ;

non_optional_type = [ "mut" ], reference_type
                  | function_type
                  ;

reference_type = named_type
               | tuple_type
               | grouped_type
               | associated_type_projection
               | context_type
               ;

named_type = qualified_name, [ type_arguments ] ;
type_arguments = "[", type_argument,
                 { ",", type_argument }, [ "," ], "]" ;
type_argument = type, [ "..." ] ;

tuple_type = "(", ")"
           | "(", type_element, ",",
             [ type_element, { ",", type_element }, [ "," ] ], ")"
           ;
type_element = type, [ "..." ] ;

grouped_type = "(", type, ")" ;

function_type = [ "mut" ], "fn", [ "!" ], "(", [ type_list ], ")",
                "->", type, [ requirement_clause ] ;
type_list = type_element, { ",", type_element }, [ "," ] ;

associated_type_projection = ( qualified_name | "Self" ), "::", identifier ;

qualified_name = identifier, { ".", identifier } ;

requirement_clause = "$", requirement_expression ;
requirement_expression = requirement_union,
                         { "-", requirement_key } ;
requirement_union = requirement_term, { "+", requirement_term } ;
requirement_term = requirement_key
                 | "(", requirement_expression, ")"
                 ;
requirement_key = trait_type ;
```

`mut` is a type modifier. Semantic rules reject meaningless or nested forms,
including direct `mut mut T`. Optionality applies to the complete access type
and may be nested. Parentheses group types; unlike a one-element tuple type,
grouping has no trailing comma.
Requirement rows on function types are specified in
[Requirements and Suspension](11-requirements-and-suspension.md).

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
                       | context_scope
                       | logical_or_expression
                       ;

suite_expression = if_expression
                 | for_expression
                 | while_expression
                 | match_expression
                 | closure_expression
                 | trailing_block_call
                 | context_scope
                 ;

logical_or_expression = logical_and_expression,
                        { "or", logical_and_expression } ;
logical_and_expression = comparison_expression,
                         { "and", comparison_expression } ;

comparison_expression = bitwise_or_expression,
                        [ comparison_operator, bitwise_or_expression ] ;
comparison_operator = "==" | "!=" | "<" | "<=" | ">" | ">=" | "is" ;

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

unary_expression = ( "+" | "-" | "~" | "not" ), unary_expression
                 | power_expression
                 ;
power_expression = postfix_expression, [ "**", unary_expression ] ;

postfix_expression = primary_expression, { postfix_suffix } ;
postfix_suffix = ".", identifier, [ function_type_arguments ]
               | ".", integer_literal
               | "[", expression, "]"
               | argument_clause
               | "!", argument_clause
               | "?"
               ;
```

`:=` is right-associative and has the lowest precedence. Comparisons do not
chain. Exponentiation is right-associative. The right operand of `**` may
therefore begin with a unary operator.

`!(` begins a suspension call suffix at ordinary call precedence.

After member resolution, brackets immediately following a generic method name
are parsed as `function_type_arguments`, not as an indexing suffix. An explicit
method type-argument list is valid only when the selected member is generic and
the expression proceeds to an ordinary or suspending call.

### Primary Expressions

```ebnf
primary_expression = literal
                   | string_expression
                   | generic_function_reference
                   | qualified_name
                   | contextual_variant_expression
                   | trait_qualified_call
                   | context_use
                   | context_create
                   | shape_expression
                   | annotation_runtime_access
                   | pack_map_expression
                   | tuple_or_group_expression
                   | list_expression
                   | map_expression
                   | data_expression
                   | "pass"
                   ;

generic_function_reference = qualified_name, function_type_arguments ;
function_type_arguments = "[", function_type_argument,
                          { ",", function_type_argument }, [ "," ], "]" ;
function_type_argument = type_argument | "_" ;
contextual_variant_expression = ".", identifier ;
trait_qualified_call = trait_type, "::", identifier, argument_clause ;

shape_expression = "shape", "(", shape_target, ")" ;
shape_target = type | qualified_name ;

annotation_runtime_access = qualified_name, "::", "annotation", "(",
                            annotation_target, ")" ;

pack_map_expression = "pack", ".", ( "map" | "map_list" ), "(",
                      expression, ",", qualified_name,
                      { ",", expression }, [ "," ], ")" ;

literal = boolean_literal
        | nil_literal
        | float_literal
        | integer_literal
        | char_literal
        ;

string_expression = interpreted_string_expression
                  | interpreted_multiline_string_expression
                  | raw_string_literal
                  | raw_multiline_string_literal
                  ;

interpreted_string_expression = '"', { string_segment }, '"' ;
interpreted_multiline_string_expression = '"""',
                                          { multiline_string_segment },
                                          '"""' ;
string_segment = string_text
               | escape_sequence
               | "$", identifier
               | "${", expression, "}"
               ;
multiline_string_segment = multiline_string_text
                         | escape_sequence
                         | "$", identifier
                         | "${", expression, "}"
                         ;

tuple_or_group_expression = "(", ")"
                          | "(", expression, ")"
                          | "(", tuple_element, ",",
                            [ tuple_element, { ",", tuple_element }, [ "," ] ], ")"
                          | "(", expression, "...", ")"
                          ;
tuple_element = expression, [ "..." ] ;

list_expression = "[", [ list_items ], "]"
                | list_comprehension
                ;
list_items = expression, { ",", expression }, [ "," ] ;

map_expression = "{", [ map_items ], "}"
               | map_comprehension
               ;
map_items = map_item, { ",", map_item }, [ "," ] ;
map_item = expression, ":", expression ;

data_expression = named_type, "{", [ data_items ], "}" ;
data_items = [ "...", expression, "," ],
             data_field_item, { ",", data_field_item }, [ "," ]
             | "...", expression, [ "," ]
             ;
data_field_item = identifier, ":", expression ;
```

Name resolution distinguishes a data expression from a map expression and
an enum variant selection from ordinary field access. It also distinguishes a
named generic-function reference from indexing: in `first[string](names)`, the
bracketed form is parsed as function type arguments because `first` resolves
to a named generic function. A parser may preserve this syntactic ambiguity
until name resolution. Each argument is a type, a type-pack expansion, or the
inference placeholder `_`. The placeholder is not part of ordinary
`type_arguments` and therefore cannot occur in a type such as `list[_]`.

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
closure_expression = [ "mut" ], "fn", [ "!" ], closure_parameter_clause,
                     [ "->", type ], [ requirement_clause ],
                     ":", suite_body ;

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

match_arm = pattern, [ "if", expression ], "=>", arm_body ;
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
        | optional_pattern
        | binding_pattern_atom
        | variant_pattern
        | data_pattern
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
optional_pattern = binding_pattern_atom, "?" ;

variant_pattern = ( qualified_name | ".", identifier ),
                  [ pattern_argument_clause ] ;
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
data_pattern_field = identifier, [ "=", pattern ] ;

tuple_pattern = "(", pattern, ",",
                [ pattern, { ",", pattern }, [ "," ] ], ")" ;
```

Variant patterns may use a qualified enum variant name or `.Variant` when the
matched value's type supplies one enum. Positional binding names need not match
payload field names. Only `field=pattern` is a named pattern, and no positional
pattern may follow a named pattern.
In a data pattern, bare `field` binds that field's value to a new name;
`field=pattern` matches it against a nested pattern. Unlisted fields are
ignored.

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

## Requirements And Provider Contexts

```ebnf
context_use = "$", ".", "use", "(", requirement_key,
              { ",", requirement_key }, [ "," ], ")" ;

context_create = "$", ".", "context", "(", context_entries, ")" ;
context_type = "$", ".", "Context", "[", requirement_expression, "]" ;
context_scope = "$", ".", "with", "(", context_entries, ")",
                ":", suite_body ;

context_entries = context_entry, { ",", context_entry }, [ "," ] ;
context_entry = requirement_key, "=", expression
              | "...", expression
              ;
```

Requirement expressions denote unordered rows after name resolution. A generic
identifier used as a complete requirement term is a requirement-row parameter;
subtraction removes one concrete key from such a row.

## Annotations

```ebnf
decorated_decl = decorator_line, { decorator_line },
                 [ "pub" ], ( data_decl | enum_decl | function_decl ) ;

decorator_line = "@", ( derive_decorator | expression ), NEWLINE ;

derive_decorator = "derive", "(", qualified_name,
                   { ",", qualified_name }, [ "," ], ")" ;

annotation_decl = member_metadata_decl | facet_annotation_decl ;

member_metadata_decl = "annotate", qualified_name, ":",
                       annotation_member_suite ;

facet_annotation_decl = "annotate", annotation_facet, "for", annotation_target, ":",
                        facet_annotation_suite ;

annotation_facet = type | expression ;

annotation_target = type | qualified_name ;

annotation_member_suite = "pass", SUITE_END
                        | NEWLINE, INDENT,
                          metadata_assignment,
                          { metadata_assignment }, DEDENT
                        ;

metadata_assignment = identifier, "=", expression, NEWLINE ;

facet_annotation_suite = "pass", SUITE_END
                       | NEWLINE, INDENT,
                         facet_override,
                         { facet_override }, DEDENT
                       ;

facet_override = metadata_assignment | function_decl ;
```

An `annotation_facet` that resolves as a type requests that stateless facet's
default empty value and is valid only when the type has no required fields. An
expression form is evaluated as a configured facet
value; its static type is the facet type used for coherence and
`Annotate[Facet]` generation. The syntactic overlap between a named type and a
name expression is resolved by ordinary name and type resolution.

## Pack Expansion

An ellipsis following a generic parameter declares a type pack. In a type,
parameter, tuple, or argument position, an ellipsis following a subtree that
contains a pack reference expands that subtree once per pack element. The same
token denotes an ordinary homogeneous vararg or list spread when no pack is
referenced. Name and type resolution make the distinction; unresolved or mixed
uses are compile-time errors.
