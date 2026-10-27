use hd_fmt::{MAX_COLUMNS, format};

fn wrapped(source: &str) -> String {
    let formatted = format(source).expect("input parses");
    assert!(
        hd_syntax::parse(formatted.as_bytes()).is_ok(),
        "{formatted}"
    );
    assert_eq!(
        format(&formatted).expect("formatted output parses"),
        formatted
    );
    formatted
}

fn narrow(source: &str) -> String {
    let formatted = wrapped(source);
    for line in formatted.lines() {
        assert!(
            line.chars().count() <= MAX_COLUMNS,
            "{} columns: {line}",
            line.chars().count()
        );
    }
    formatted
}

const FIRST: &str = "the_first_descriptive_parameter_name";
const SECOND: &str = "the_second_descriptive_parameter_name";
const THIRD: &str = "the_third_descriptive_parameter_name";

#[test]
fn calls_structs_lists_and_parameters_put_one_element_on_each_line() {
    let call = format!("fn f():\n    render({FIRST}, {SECOND}, {THIRD})\n");
    assert_eq!(
        narrow(&call),
        format!(
            "fn f():\n    render(\n        {FIRST},\n        {SECOND},\n        {THIRD},\n    )\n"
        )
    );
    let record =
        format!("fn f():\n    Record {{ first: {FIRST}, second: {SECOND}, third: {THIRD} }}\n");
    assert_eq!(
        narrow(&record),
        format!(
            "fn f():\n    Record {{\n        first: {FIRST},\n        second: {SECOND},\n        third: {THIRD},\n    }}\n"
        )
    );
    let list = format!("fn f():\n    [{FIRST}, {SECOND}, {THIRD}]\n");
    assert_eq!(
        narrow(&list),
        format!("fn f():\n    [\n        {FIRST},\n        {SECOND},\n        {THIRD},\n    ]\n")
    );
    let parameters = format!("fn f({FIRST}: i64, {SECOND}: i64, {THIRD}: i64) -> i64:\n    +1\n");
    assert_eq!(
        narrow(&parameters),
        format!(
            "fn f(\n    {FIRST}: i64,\n    {SECOND}: i64,\n    {THIRD}: i64,\n) -> i64:\n    +1\n"
        )
    );
}

#[test]
fn binary_chains_use_transparent_parentheses_and_preserve_precedence() {
    let chain = format!("fn f():\n    {FIRST} + {SECOND} * third + {THIRD}\n");
    assert_eq!(
        narrow(&chain),
        format!(
            "fn f():\n    (\n        {FIRST}\n        + {SECOND} * third\n        + {THIRD}\n    )\n"
        )
    );
    let grouped = format!("fn f():\n    ({FIRST} + {SECOND} + {THIRD})\n");
    assert_eq!(
        narrow(&grouped),
        format!("fn f():\n    (\n        {FIRST}\n        + {SECOND}\n        + {THIRD}\n    )\n")
    );
    let condition = format!("fn f():\n    if {FIRST} && {SECOND} && {THIRD}:\n        pass\n");
    let formatted = narrow(&condition);
    assert!(formatted.contains("if (\n"), "{formatted}");
    assert!(formatted.contains("    ):\n        pass"), "{formatted}");
}

#[test]
fn nested_groups_repeat_until_every_breakable_line_fits() {
    let source = format!(
        "fn f():\n    consume(Record {{ first: [{FIRST}, {SECOND}, {THIRD}], second: [{FIRST}, {SECOND}, {THIRD}] }}, {FIRST})\n"
    );
    let formatted = narrow(&source);
    assert!(formatted.contains("consume(\n"), "{formatted}");
    assert!(formatted.contains("Record {\n"), "{formatted}");
    assert!(formatted.contains("first: [\n"), "{formatted}");
}

#[test]
fn comments_and_literal_contents_remain_exact_even_when_long() {
    let long = "a".repeat(140);
    let source = format!(
        "# {long}  \nfn f():\n    literal := \"{long}  \"\n    short()  # {long}  \n# markdown  "
    );
    let formatted = wrapped(&source);
    assert!(formatted.contains(&format!("# {long}  \n")), "{formatted}");
    assert!(formatted.contains(&format!("\"{long}  \"")), "{formatted}");
    assert!(
        formatted.contains(&format!("    short() # {long}  \n")),
        "{formatted}"
    );
    assert!(formatted.ends_with("# markdown  \n"), "{formatted}");
    let interpolated = format!(
        "fn f():\n    \"{dollar}{{{FIRST} + {SECOND} + {THIRD}}}\"\n",
        dollar = "$"
    );
    assert_eq!(wrapped(&interpolated), interpolated);
}

#[test]
fn literal_arguments_split_without_splitting_either_literal() {
    let first = "x".repeat(60);
    let second = "y".repeat(60);
    let source = format!("fn f():\n    consume(\"{first}\", \"{second}\")\n");
    assert_eq!(
        narrow(&source),
        format!("fn f():\n    consume(\n        \"{first}\",\n        \"{second}\",\n    )\n")
    );
}

#[test]
fn trailing_commas_generic_arguments_and_use_groups_stay_valid() {
    let source = format!(
        "use pkg.module.{{{FIRST}, {SECOND}, {THIRD}}}\nfn f():\n    consume::[{FIRST}, {SECOND}, {THIRD}](+1,)\n"
    );
    let formatted = narrow(&source);
    assert!(formatted.contains("use pkg.module.{\n"), "{formatted}");
    assert!(formatted.contains("consume::[\n"), "{formatted}");
    assert!(formatted.contains("](+1)"), "{formatted}");
}

#[test]
fn inline_closures_and_suites_keep_their_expression_boundaries() {
    for expression in [
        format!("apply(fn({FIRST}, {SECOND}, {THIRD}): {FIRST} + {SECOND})"),
        format!("if ready: apply({FIRST}, {SECOND}, {THIRD}) else: missing"),
        format!(
            "match value:\n        .Some(found) => consume({FIRST}, {SECOND}, {THIRD})\n        .None => pass"
        ),
    ] {
        let source = format!("fn f():\n    {expression}\n");
        let formatted = narrow(&source);
        assert_ne!(formatted, source);
    }
}

#[test]
fn exactly_one_hundred_columns_stay_on_one_line() {
    let prefix = "    consume(";
    let name = "a".repeat(MAX_COLUMNS - prefix.len() - 1);
    let source = format!("fn f():\n{prefix}{name})\n");
    assert_eq!(wrapped(&source), source);
    let source = format!("fn f():\n{prefix}{name}x)\n");
    assert!(narrow(&source).contains("consume(\n"));
}

#[test]
fn an_existing_multiline_closure_joins_and_its_separator_dedents() {
    let source = format!(
        "fn f():\n    consume({FIRST}, {SECOND}, {THIRD}, fn(value):\n        use_value(value)\n    )\n"
    );
    let formatted = narrow(&source);
    assert!(
        formatted.contains("        fn(value): use_value(value)\n        ,\n    )"),
        "{formatted}"
    );
}

#[test]
fn inline_argument_comments_and_multiline_literals_keep_their_text() {
    let source = format!(
        "fn f():\n    consume({FIRST}, {SECOND}, {THIRD}, # attached  \n        fn(value):\n            use_value(value)\n    )\n"
    );
    let formatted = narrow(&source);
    assert!(
        formatted.contains(&format!("{THIRD}, # attached  \n")),
        "{formatted}"
    );
    let source = format!(
        "fn f():\n    consume({FIRST}, {SECOND}, {THIRD}, \"\"\"alpha\n  literal indentation\nend\"\"\")\n"
    );
    let formatted = narrow(&source);
    assert!(
        formatted.contains("\"\"\"alpha\n  literal indentation\nend\"\"\""),
        "{formatted}"
    );
}

#[test]
fn a_short_broken_group_gets_its_last_comma_before_the_inline_comment() {
    let source = "fn f():\n    consume(\n        value # last argument  \n    )\n";
    assert_eq!(
        wrapped(source),
        "fn f():\n    consume(\n        value, # last argument  \n    )\n"
    );
    let source = "fn f():\n    consume(\n        value, # already has its comma  \n    )\n";
    assert_eq!(wrapped(source), source);
}

#[test]
fn short_groups_join_and_remove_optional_trailing_commas() {
    for (source, expected) in [
        (
            "fn f(\n    value: i64,\n):\n    pass\n",
            "fn f(value: i64):\n    pass\n",
        ),
        (
            "fn f():\n    consume(\n        value,\n    )\n",
            "fn f():\n    consume(value)\n",
        ),
        (
            "fn f():\n    Item {\n        value: +1,\n    }\n",
            "fn f():\n    Item { value: +1 }\n",
        ),
    ] {
        assert_eq!(wrapped(source), expected);
    }
}

#[test]
fn a_parenthesized_value_and_single_context_argument_never_become_tuples() {
    let source = "fn f():\n    (\n        +1\n    )\n";
    assert_eq!(wrapped(source), "fn f():\n    (+1)\n");
    let source = "fn f(ctx: $.Context[\n    $ Console\n]):\n    pass\n";
    let formatted = wrapped(source);
    assert!(!formatted.contains("Console,"), "{formatted}");
    let source = "fn f():\n    $.Context[\n        $ Console\n    ]\n";
    assert_eq!(wrapped(source), "fn f():\n    $.Context[$ Console]\n");
}

#[test]
fn a_required_comma_keeps_an_unsplittable_literal_exact_at_the_width_boundary() {
    let literal = "x".repeat(90);
    let source = format!("fn f():\n    consume(\n        \"{literal}\"\n    )\n");
    let formatted = wrapped(&source);
    assert_eq!(
        formatted,
        format!("fn f():\n    consume(\n        \"{literal}\",\n    )\n")
    );
    assert_eq!(
        formatted.lines().nth(2).unwrap().chars().count(),
        MAX_COLUMNS + 1
    );
}

#[test]
fn canonical_spacing_and_short_groups_ignore_authored_layout() {
    let source = "fn sum( a:i32,b :i32 )->i32:\n    pair := Item{first:a,second:b}\n    consume(\n        pair,\n    )\n    a+b\n";
    assert_eq!(
        wrapped(source),
        "fn sum(a: i32, b: i32) -> i32:\n    pair := Item { first: a, second: b }\n    consume(pair)\n    a + b\n"
    );
}

#[test]
fn value_if_may_join_but_statement_if_stays_vertical() {
    let source = "fn choose(value: bool) -> i32:\n    answer := if value:\n        +1\n    else:\n        +2\n    if value: consume(answer)\n    answer\n";
    assert_eq!(
        wrapped(source),
        "fn choose(value: bool) -> i32:\n    answer := if value: +1 else: +2\n    if value:\n        consume(answer)\n    answer\n"
    );
}

#[test]
fn tail_if_classification_is_syntax_only() {
    let source = "fn inferred(flag: bool):\n    if flag:\n        +1\n    else:\n        +2\nfn explicit(flag: bool) -> void:\n    if flag: finish() else: fail()\nfn incomplete(flag: bool):\n    if flag: finish()\n";
    assert_eq!(
        wrapped(source),
        "fn inferred(flag: bool):\n    if flag: +1 else: +2\nfn explicit(flag: bool) -> void:\n    if flag:\n        finish()\n    else:\n        fail()\nfn incomplete(flag: bool):\n    if flag:\n        finish()\n"
    );
}

#[test]
fn inline_grammar_boundaries_stay_parseable() {
    let recursive = "fn sum_to(limit: i32) -> i32:\n    sum := fn(n: i32) -> i32:\n        if n == 0: 0\n        else: n + sum(n - 1)\n    sum(limit)\n";
    assert_eq!(
        wrapped(recursive),
        "fn sum_to(limit: i32) -> i32:\n    sum := fn(n: i32) -> i32:\n        if n == 0: 0 else: n + sum(n - 1)\n    sum(limit)\n"
    );

    let let_else = "fn read(value: i32?) -> i32?:\n    let .Some(found) = value else: return .None\n    .Some(found)\n";
    assert_eq!(
        wrapped(let_else),
        "fn read(value: i32?) -> i32?:\n    let .Some(found) = value else:\n        return .None\n    .Some(found)\n"
    );

    let comprehension = "fn doubled(xs: List[i32]):\n    [for x in xs:\n        x * 2\n    ]\n";
    assert_eq!(
        wrapped(comprehension),
        "fn doubled(xs: List[i32]):\n    [\n        for x in xs:\n            x * 2\n    ]\n"
    );
}

#[test]
fn lexical_separation_and_named_argument_spacing_are_canonical() {
    let source = "fn f(a: i32):\n    left := & &a\n    assert_equal(left, &a, reason = \"same\")\n";
    assert_eq!(
        wrapped(source),
        "fn f(a: i32):\n    left := & &a\n    assert_equal(left, &a, reason=\"same\")\n"
    );
}

#[test]
fn parenthesized_multiline_closures_keep_delimiter_lines() {
    let source = "fn pick(flag: bool):\n    choice(\n        flag,\n        (\n            fn(a):\n                doubled := a * 2\n                doubled + 22\n        ),\n        (\n            fn(b):\n                halved := b / 2\n                halved - 5\n        ),\n    )\n";
    let formatted = wrapped(source);
    assert!(
        formatted.contains("doubled + 22\n        ),"),
        "{formatted}"
    );
    assert!(
        formatted.contains("halved - 5\n        )\n        ,\n    )"),
        "{formatted}"
    );
}

#[test]
fn broken_value_if_argument_dedents_before_its_comma() {
    let source =
        format!("fn f():\n    consume(if ready: {FIRST} + {SECOND} else: {SECOND} + {THIRD})\n");
    let formatted = narrow(&source);
    assert!(
        formatted.contains(&format!(
            "        if ready:\n            {FIRST} + {SECOND}\n        else:\n            {SECOND} + {THIRD}\n        ,\n    )"
        )),
        "{formatted}"
    );
}

#[test]
fn obsolete_continuation_parens_leave_but_call_target_parens_stay() {
    let source = "fn f(a: i32, b: i32, handler: Handler, event: Event):\n    sum := (a + b)\n    other := (\n        handler\n        .value\n        + a\n    )\n    (handler.callback)(event)\n";
    assert_eq!(
        wrapped(source),
        "fn f(a: i32, b: i32, handler: Handler, event: Event):\n    sum := a + b\n    other := handler.value + a\n    (handler.callback)(event)\n"
    );
}

#[test]
fn long_suite_header_chain_gets_one_complete_wrapper() {
    let source = "fn f(source: Source):\n    if source.configuration_for_the_current_authenticated_application().has_valid_authorization_for_the_selected_request():\n        pass\n";
    assert_eq!(
        narrow(source),
        "fn f(source: Source):\n    if (\n        source.configuration_for_the_current_authenticated_application()\n            .has_valid_authorization_for_the_selected_request()\n    ):\n        pass\n"
    );
}

#[test]
fn long_pipe_chain_breaks_before_each_pipe() {
    let source = "fn pipeline() -> i32:\n    +1 |> double_the_current_authenticated_request_count |> increment_the_current_authenticated_request_count\n";
    assert_eq!(
        narrow(source),
        "fn pipeline() -> i32:\n    +1\n        |> double_the_current_authenticated_request_count\n        |> increment_the_current_authenticated_request_count\n"
    );
}

#[test]
fn comment_bytes_roles_and_suite_ownership_stay_exact() {
    let source = "## module doc  \r\nfn f():\r\n    if ready:\r\n        consume(value)  # suffix  \r\n    # after inner suite  \r\n    finish()\r\n# root tail  \r\n\r\nfn g():\r\n    pass\r\n    # function tail  \r\n";
    assert_eq!(
        wrapped(source),
        "## module doc  \nfn f():\n    if ready:\n        consume(value) # suffix  \n    # after inner suite  \n    finish()\n# root tail  \n\nfn g():\n    pass\n    # function tail  \n"
    );
}

#[test]
fn pass_suite_keeps_its_syntax_owned_inline_or_vertical_shape() {
    let inline = "struct Empty: pass\n";
    let vertical = "struct Empty:\n    pass\n";
    assert_eq!(format(inline).expect("inline pass parses"), inline);
    assert_eq!(format(vertical).expect("vertical pass parses"), vertical);
}

#[test]
fn authored_blank_lines_are_bounded_and_generated_indent_is_spaces() {
    let source = "fn f():\n        first()\n\n\n\n        second()\n\n\nfn g():\n        pass\n";
    assert_eq!(
        wrapped(source),
        "fn f():\n    first()\n\n    second()\n\nfn g():\n    pass\n"
    );
}

#[test]
fn unicode_scalars_not_utf8_bytes_set_the_width_boundary() {
    let head = "界".repeat(43);
    let source = format!("fn f():\n    consume(\"{head}\", +1)\n");
    assert_eq!(wrapped(&source), source);
}
