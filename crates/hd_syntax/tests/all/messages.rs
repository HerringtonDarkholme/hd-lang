//! Every lexer and parser diagnostic carries its own message: the situation
//! in words, never the code. One row per code of the catalogue, and one per
//! common `syntax-error` site ("expected X, found Y").

use hd_base::SliceSort as _;
use hd_syntax::parse;

#[test]
fn migration_fixes_preserve_surrounding_source_and_parse() {
    for (source, corrected) in [
        (
            "# café\nfn f(pair: P): pair.12\n",
            "# café\nfn f(pair: P): pair._12\n",
        ),
        ("fn f(): {:}\n", "fn f(): {}\n"),
        (
            "use std.fs.{read_text!, FsRead}\n",
            "use std.fs.{read_text, FsRead}\n",
        ),
        (
            "fn f(pair: P):\n    (low, (high, _)) := pair\n",
            "fn f(pair: P):\n    let (low, (high, _)) = pair\n",
        ),
    ] {
        let parsed = parse(source.as_bytes());
        let fixes: Vec<_> = parsed
            .diagnostics
            .iter()
            .filter_map(|d| d.fix.as_ref())
            .collect();
        assert_eq!(fixes.len(), 1, "{source}: {:?}", parsed.diagnostics);
        let mut edits = fixes[0].edits.clone();
        edits.sort_stable_by_key(|(span, _)| std::cmp::Reverse(span.lo));
        let mut edited = source.to_owned();
        for (span, replacement) in edits {
            edited.replace_range(span.lo as usize..span.hi as usize, &replacement);
        }
        assert_eq!(edited, corrected);
        let checked = parse(edited.as_bytes());
        assert!(checked.is_ok(), "{edited}: {:?}", checked.diagnostics);
    }
}

#[test]
fn migration_fixes_do_not_guess_from_other_invalid_forms() {
    for source in [
        "fn f(pair: P): pair.0xA\n",
        "fn f(): { : value }\n",
        "use std.fs.{read_text!()}\n",
        "fn f(pair: P):\n    (low, +1) := pair\n",
        "fn f(pair: P):\n    (low, call()) := pair\n",
        "fn f(pair: P):\n    (low, self) := pair\n",
        "fn f(pair: P):\n    (low, (high)) := pair\n",
    ] {
        let parsed = parse(source.as_bytes());
        assert!(!parsed.is_ok(), "{source}");
        assert!(
            parsed.diagnostics.iter().all(|d| d.fix.is_none()),
            "{source}: {:?}",
            parsed.diagnostics
        );
    }
}

/// (source, code, message)
const ROWS: &[(&str, &str, &str)] = &[
    // ---- lexer
    (
        "fn f():\n\tpass\n",
        "tab-whitespace",
        "indentation must contain spaces only",
    ),
    (
        "x := 1 + \t2\n",
        "tab-whitespace",
        "tab characters are not permitted as whitespace",
    ),
    (
        "fn f():\n    x := \"a\tb\"\n",
        "tab-whitespace",
        "a tab inside a literal must be written as the '\\t' escape",
    ),
    ("x := \\\n", "invalid-token", "unexpected character '\\'"),
    (
        "x := 'ab'\n",
        "invalid-token",
        "a character literal must contain one Unicode scalar value",
    ),
    (
        "x := `_`\n",
        "invalid-token",
        "a backtick must enclose an identifier or reserved word",
    ),
    (
        "x := 1__0\n",
        "invalid-token",
        "invalid numeric literal '1__0'",
    ),
    (
        "x := \"bad\\xescape\"\n",
        "invalid-escape",
        "invalid escape sequence '\\x'",
    ),
    (
        "x := \"bad\\u{110000}\"\n",
        "invalid-escape",
        "invalid Unicode escape sequence",
    ),
    (
        "name := \"Ada\n",
        "unterminated-string",
        "unterminated literal",
    ),
    (
        "fn f():\n    x := \"${\n",
        "unterminated-string",
        "unterminated '${...}' interpolation",
    ),
    (
        "name := \"Ada\";\n",
        "reserved-semicolon",
        "semicolon is reserved and cannot separate statements",
    ),
    (
        "x := 1 \u{feff}+ 2\n",
        "unexpected-bom",
        "a byte-order mark is only allowed at the start of a file",
    ),
    (
        "x := (1 + 2\n",
        "unclosed-delimiter",
        "unclosed '(' delimiter",
    ),
    (
        "x := 1]\n",
        "unmatched-delimiter",
        "unexpected ']' delimiter",
    ),
    (
        "fn f():\n    x := \"$if\"\n",
        "syntax-error",
        "an unescaped '$' must be followed by an identifier, 'self', or '{'",
    ),
    ("x := 0x\n", "syntax-error", "invalid integer literal '0x'"),
    (
        "x := 5else\n",
        "syntax-error",
        "'else' is a reserved word, so '5else' needs a space: write '5 else'",
    ),
    (
        "x := return\"a\"\n",
        "syntax-error",
        "'return' is a reserved word, so it cannot prefix a string: write 'return \"...'",
    ),
    // ---- layout
    (
        "fn f():\n        x := +1\n      y := +2\n",
        "invalid-dedent",
        "column 7 is not an active indentation level",
    ),
    (
        "fn f():\n    y := g(a, if c:\n    1\n    else: 2)\n",
        "unexpected-indentation",
        "a nested suite's body must be deeper than the statement that contains its header",
    ),
    (
        "f(fn(x):\n    x)\n",
        "syntax-error",
        "expected a line ending before ')' after a closure body",
    ),
    (
        "y := f(fn(x):\n    a\n  b\n)\n",
        "syntax-error",
        "expected a closure body line at column 5, found a line at column 3",
    ),
    (
        "f := fn(x): x\n    .foo()\n",
        "syntax-error",
        "a leading-dot line cannot continue a line whose same-line suite is still open",
    ),
    (
        "x := a\n    |> f\n    .g()\n",
        "syntax-error",
        "a leading-dot line cannot continue a pipe chain; write the call as a step, such as '|> _.name()'",
    ),
    // ---- parser, one row per catalogue code
    (
        "fn f():\n    resize(width=640, 480)\n",
        "argument-order",
        "positional arguments must precede named arguments",
    ),
    (
        "fn f():\n    x := 0 < value < 10\n",
        "comparison-chaining",
        "comparisons do not chain",
    ),
    (
        "fn f():\n    @inline\n    fn g():\n        pass\n",
        "decorator-not-top-level",
        "decorators are only valid on top-level declarations",
    ),
    (
        "use pkg.status.{Status.Queued}\n",
        "direct-variant-use",
        "enum variants cannot be imported directly",
    ),
    (
        "fn f():\n    ## orphan\n    x := +1\n",
        "doc-comment-without-target",
        "documentation comments must attach to a declaration or member",
    ),
    (
        "fn f():\n    display_name: string = \"Ada\"\n",
        "missing-let",
        "a binding with a type annotation must begin with 'let'; write `let x: T = ...`, or `x := ...` without the type",
    ),
    (
        "fn f():\n    (a, b) := pair\n",
        "missing-let",
        "':=' binds one name; a pattern before it needs 'let', as in 'let (a, b) = pair'",
    ),
    (
        "struct P:\n    mut x: i32\n",
        "mutable-field-modifier",
        "data fields express mutable access in their type rather than with a field modifier",
    ),
    (
        "struct P:\n    mut Base\n",
        "mutable-field-modifier",
        "an embedded field takes no 'mut': access to an embedded part follows its container",
    ),
    (
        "fn f():\n    Pair.Values(left=l, r)\n",
        "argument-order",
        "positional arguments must precede named arguments",
    ),
    (
        "fn f():\n    match x:\n        .A(a=l, r) => 1\n",
        "pattern-order",
        "positional patterns must precede named patterns",
    ),
    (
        "fn f():\n    x := value.tag\"item\"\n",
        "qualified-string-prefix",
        "a string prefix is a bare name in scope: write 'tag\"...\"' without the qualifier",
    ),
    (
        "fn f():\n    if x:\n        a\n    : 1 else: 2\n",
        "trailing-block-position",
        "a call accepts only one trailing callback block",
    ),
    (
        "fn f():\n    g([x:\n        y\n    ])\n",
        "trailing-block-position",
        "a trailing callback block is not valid inside brackets",
    ),
    (
        "trait T:\n    pub fn f()\n",
        "trait-method-visibility",
        "trait methods inherit the trait's visibility and cannot be declared pub",
    ),
    (
        "impl A for B:\n    pub fn f():\n        pass\n",
        "trait-method-visibility",
        "trait methods inherit the trait's visibility and cannot be declared pub",
    ),
    (
        "enum E:\n    IntLit(v: i64) -> Expr[i64]\n",
        "variant-result-type-removed",
        "a variant result names its enum and gives arguments, not a result type",
    ),
    (
        "enum E:\n    Pair[U](a: i32)\n",
        "variant-result-type-removed",
        "enum variants take no type parameters",
    ),
    (
        "enum E:\n    Bad -> Other\n",
        "variant-result-type-removed",
        "a variant result names its enum and gives an argument clause",
    ),
    // ---- syntax-error sites: expected X, found Y
    (
        "fn f():\n    if a and b:\n        pass\n",
        "syntax-error",
        "expected ':', found 'and'",
    ),
    (
        "fn f()\n",
        "syntax-error",
        "expected ':' and a function body, found a line ending",
    ),
    (
        "fn f(x: ):\n    pass\n",
        "syntax-error",
        "expected a type, found ')'",
    ),
    (
        "fn f(x: i32,, y: i32):\n    pass\n",
        "syntax-error",
        "expected a parameter name, found ','",
    ),
    (
        "x :=\n",
        "syntax-error",
        "expected an expression, found a line ending",
    ),
    (
        "x := [1, 2 3]\n",
        "syntax-error",
        "expected ',' or ']', found '3'",
    ),
    (
        "x := (1 2)\n",
        "syntax-error",
        "expected ',' or ')', found '2'",
    ),
    (
        "x := {1: 2 3: 4}\n",
        "syntax-error",
        "expected ',' or '}', found '3'",
    ),
    (
        "x := f(a) b\n",
        "syntax-error",
        "expected a line ending, found 'b'",
    ),
    (
        "let a 1\n",
        "syntax-error",
        "expected ':' or '=' after the pattern, found '1'",
    ),
    (
        "let a, b = pair\n",
        "syntax-error",
        "several names after 'let' need parentheses: write 'let (a, b) = ...'",
    ),
    (
        "fn f():\n    x := a.\n",
        "syntax-error",
        "expected a member name after '.', found the end of the file",
    ),
    (
        "x := t.0\n",
        "syntax-error",
        "expected a member name after '.', found '0'",
    ),
    (
        "fn f():\npass\n",
        "syntax-error",
        "expected an indented suite, found a line that is not indented deeper",
    ),
    (
        "fn f():\n    x := f(a)\n        (b)\n",
        "syntax-error",
        "unexpected indentation",
    ),
    (
        "fn f():\n    x := match y: z\n",
        "syntax-error",
        "expected a line ending after the match header, found 'z'",
    ),
    (
        "fn f():\n    x := match y:\n    .A => 1\n",
        "syntax-error",
        "expected indented match arms, found a line that is not indented deeper",
    ),
    (
        "fn f():\n    x := match y:\n        .A =>\n        1\n",
        "syntax-error",
        "expected an indented match arm body, found a line that is not indented deeper",
    ),
    (
        "type T\n",
        "syntax-error",
        "expected '=' or '(', found a line ending",
    ),
    (
        "trait T:\n    x\n",
        "syntax-error",
        "expected 'fn' or 'type', found 'x'",
    ),
    (
        "impl T for U:\n    x\n",
        "syntax-error",
        "expected a method, an associated type, or a derivation, found 'x'",
    ),
    (
        "enum E:\n    A:\n        pass\n",
        "syntax-error",
        "expected a line ending after an enum variant, found ':'",
    ),
    (
        "fn f[T U]():\n    pass\n",
        "syntax-error",
        "expected ',' or ']', found 'U'",
    ),
    (
        "fn f() -> $.Foo[i32]:\n    pass\n",
        "syntax-error",
        "expected 'Context' after '$.', found 'Foo'",
    ),
    (
        "use dep.\n",
        "syntax-error",
        "expected a dependency name, found a line ending",
    ),
    (
        "fn f(values: i32...):\n    pass\n",
        "syntax-error",
        "a rest element is only allowed last in a tuple type or a function parameter list",
    ),
    (
        "fn f():\n    x := a |> b\n        |> c\n        .d\n",
        "syntax-error",
        "a leading-dot line cannot continue a pipe chain; write the call as a step, such as '|> _.name()'",
    ),
    (
        "fn f(x: i32) -> i32:\n    if fn() -> bool:\n        true\n    : 1\n",
        "syntax-error",
        "an expression followed by another token outside brackets cannot end in a suite",
    ),
    (
        "x := if c: if d: 1 else: 2 else: 3\n",
        "syntax-error",
        "a same-line suite cannot contain 'if'; put the body on an indented line",
    ),
    (
        "mut x := 1\n",
        "syntax-error",
        "'mut' is only accepted by 'let': write 'let mut name = ...'",
    ),
    (
        "tests:\n    tests:\n        pass\n",
        "syntax-error",
        "a 'tests:' block may appear only at module top level",
    ),
    (
        "pub use a.b\n",
        "syntax-error",
        "only the grouped use form accepts 'pub'; write 'pub use module.{Name}'",
    ),
    (
        "pub impl T:\n    pass\n",
        "syntax-error",
        "'pub' is not accepted before 'impl'",
    ),
    (
        "struct P:\n    pub Base\n",
        "syntax-error",
        "an embedded field takes no 'pub'",
    ),
    (
        "x := 1..=\n",
        "syntax-error",
        "an inclusive range needs an end bound: write 'a..=b'",
    ),
    (
        "x := a..b..c\n",
        "syntax-error",
        "ranges do not chain: parenthesize a range used as a bound",
    ),
    (
        "x := Box[i32] { value: 1 }\n",
        "syntax-error",
        "type arguments in an expression follow '::', as in 'Box::[i32] { ... }'",
    ),
    (
        "x := a::[T]!(1)\n",
        "syntax-error",
        "a bang call puts '!' before the type arguments: write 'name!::[T](...)'",
    ),
    (
        "x := (xs...)\n",
        "syntax-error",
        "a lone spread in a tuple keeps its trailing comma: write '(xs...,)'",
    ),
    (
        "x := (a..., b)\n",
        "syntax-error",
        "a spread must be the last element of a tuple",
    ),
    (
        "x := T { a: 1, ...b }\n",
        "syntax-error",
        "the copy-update spread '...base' comes first in a data expression",
    ),
    (
        "fn f():\n    x := $.foo(1)\n",
        "syntax-error",
        "unknown provider-context operation '$.foo'; expected 'use', 'context', 'with', or 'Context'",
    ),
    (
        "fn f(values...: i32 = 1):\n    pass\n",
        "syntax-error",
        "a vararg parameter takes no default",
    ),
    (
        "enum E:\n    A(x: i32 = 1)\n",
        "syntax-error",
        "only shared enum constructor parameters declare defaults",
    ),
    (
        "fn f[$R < Log]():\n    pass\n",
        "syntax-error",
        "a row parameter takes no bound",
    ),
    (
        "fn f[T: Log]():\n    pass\n",
        "syntax-error",
        "a generic parameter bound is written with '<', not ':'",
    ),
    (
        "impl[T = i32] X:\n    pass\n",
        "syntax-error",
        "generic parameters of an implementation take no default",
    ),
    (
        "let (a) = x\n",
        "syntax-error",
        "a one-element tuple pattern keeps its trailing comma: write '(p,)'",
    ),
    (
        "fn f():\n    match x:\n        .Some(mut u) => 1\n",
        "syntax-error",
        "'mut' before a binding name is only valid in a 'let' pattern",
    ),
    (
        "fn f():\n    match x:\n        P { a = b } => 1\n",
        "syntax-error",
        "a data pattern field is written 'field: pattern', not 'field = pattern'",
    ),
    (
        "fn f():\n    match x:\n        (a, xs..., b) => 1\n",
        "syntax-error",
        "a spread pattern must be the last element of a tuple pattern",
    ),
    (
        "fn f():\n    match x:\n        5s => 1\n",
        "syntax-error",
        "a suffixed literal cannot be a pattern",
    ),
    (
        "let x: mut fn() -> i32 = y\n",
        "syntax-error",
        "'mut' cannot directly precede 'fn'",
    ),
    (
        "let x: dyn mut Counter = y\n",
        "syntax-error",
        "'mut' goes before 'dyn': write 'mut dyn Trait'",
    ),
    (
        "let x: (T...) = y\n",
        "syntax-error",
        "a lone rest element keeps its trailing comma: write '(T...,)'",
    ),
    (
        "fn f() $(A + B):\n    pass\n",
        "syntax-error",
        "parentheses never surround a requirement row: write '$ A + B'",
    ),
    (
        "fn f() $ R + mut Logger:\n    pass\n",
        "syntax-error",
        "a requirement key or bound takes no 'mut' prefix",
    ),
    (
        "fn f(x: List[Name = i32, i32]):\n    pass\n",
        "syntax-error",
        "bindings follow every positional type argument",
    ),
    (
        "@derive(X)\ntype A = i32\n",
        "syntax-error",
        "a transparent alias takes no decorator",
    ),
];

#[test]
fn every_row_carries_its_message() {
    for &(source, code, message) in ROWS {
        let parsed = parse(source.as_bytes());
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|d| d.code.as_str() == code && d.message == message),
            "{source:?}: wanted `{code}: {message}`, got {:?}",
            parsed
                .diagnostics
                .iter()
                .map(|d| format!("{}: {}", d.code.as_str(), d.message))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn no_message_repeats_its_code() {
    for &(source, _, _) in ROWS {
        for d in &parse(source.as_bytes()).diagnostics {
            assert!(!d.message.is_empty(), "{source:?}: empty message");
            assert_ne!(
                d.message,
                d.code.as_str(),
                "{source:?}: message is the code"
            );
        }
    }
}

#[test]
fn source_that_is_not_utf8_says_so() {
    let parsed = parse(&[0xff, 0xfe, b'\n']);
    assert_eq!(parsed.diagnostics.len(), 1);
    assert_eq!(parsed.diagnostics[0].code.as_str(), "invalid-token");
    assert_eq!(
        parsed.diagnostics[0].message,
        "source text is not valid UTF-8"
    );
}

#[test]
fn source_nested_too_deep_names_the_limit() {
    let source = format!("x := {}1{}\n", "(".repeat(200), ")".repeat(200));
    let parsed = parse(source.as_bytes());
    assert!(parsed.diagnostics.iter().any(|d| {
        d.code.as_str() == "nesting-too-deep"
            && d.message == "source nests deeper than the implementation limit"
    }));
}

fn foreign_syntax(source: &str, code: &str, message: &str, marked: &str) {
    let parsed = parse(source.as_bytes());
    assert_eq!(parsed.diagnostics.len(), 1, "{:?}", parsed.diagnostics);
    let diagnostic = &parsed.diagnostics[0];
    assert_eq!(diagnostic.code.as_str(), code);
    assert_eq!(diagnostic.message, message);
    let start = source.find(marked).unwrap();
    assert_eq!(diagnostic.primary.lo as usize, start);
    assert_eq!(diagnostic.primary.hi as usize, start + marked.len());
}

#[test]
fn foreign_doc_comment_names_hd_spelling() {
    foreign_syntax(
        "/// doc\nfn f(): pass\n",
        "syntax-error",
        "documentation comments start with '##', not '///'",
        "/",
    );
}

#[test]
fn foreign_local_use_names_module_position() {
    foreign_syntax(
        "fn f():\n    use std.text\n",
        "syntax-error",
        "'use' goes at the top of the file or a 'tests:' block, not inside a function",
        "std",
    );
}

#[test]
fn foreign_newtype_names_hd_spelling() {
    foreign_syntax(
        "newtype Meters(f64)\n",
        "syntax-error",
        "a newtype is declared with 'type': write 'type Name(Base)'",
        "Meters",
    );
}

#[test]
fn foreign_or_pattern_suggests_separate_arms() {
    foreign_syntax(
        "fn f():\n    match n:\n        1 | 2 | 3 => pass\n",
        "syntax-error",
        "match patterns do not use '|'; write a separate arm for each pattern",
        "|",
    );
}

#[test]
fn foreign_postfix_bang_names_hd_spelling() {
    foreign_syntax(
        "fn g():\n    f()!\n",
        "syntax-error",
        "a bang call puts '!' before the arguments: write 'f!(...)'",
        "!",
    );
}

#[test]
fn foreign_cast_names_conversion_call() {
    foreign_syntax(
        "(1: i64)\n",
        "trailing-block-position",
        "casts are calls: write 'i64(1)'",
        ":",
    );
}

#[test]
fn foreign_context_operation_points_to_its_name() {
    foreign_syntax(
        "fn f():\n    $.nosuch()\n",
        "syntax-error",
        "unknown provider-context operation '$.nosuch'; expected 'use', 'context', 'with', or 'Context'",
        "nosuch",
    );
}

#[test]
fn foreign_cast_uses_written_operand_and_type() {
    foreign_syntax(
        "(x: f64)\n",
        "trailing-block-position",
        "casts are calls: write 'f64(x)'",
        ":",
    );
    foreign_syntax(
        "(x + 1: f64)\n",
        "trailing-block-position",
        "casts are calls: write 'Type(value)'",
        ":",
    );
}

#[test]
fn foreign_cast_keeps_a_signed_numeric_operand() {
    for (source, message) in [
        ("(+1: i64)\n", "casts are calls: write 'i64(+1)'"),
        ("(-2.5: f32)\n", "casts are calls: write 'f32(-2.5)'"),
        ("(+x: i64)\n", "casts are calls: write 'Type(value)'"),
    ] {
        foreign_syntax(source, "trailing-block-position", message, ":");
    }
}

#[test]
fn foreign_postfix_bang_uses_written_callee() {
    for (source, message) in [
        (
            "load(key)!\n",
            "a bang call puts '!' before the arguments: write 'load!(...)'",
        ),
        (
            "files.load(key)!\n",
            "a bang call puts '!' before the arguments: write 'files.load!(...)'",
        ),
        (
            "load::[i32](key)!\n",
            "a bang call puts '!' before the arguments: write 'load!::[i32](...)'",
        ),
    ] {
        foreign_syntax(source, "syntax-error", message, "!");
    }
}
