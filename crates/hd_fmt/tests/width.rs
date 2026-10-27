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
            "fn f():\n    render(\n        {FIRST},\n        {SECOND},\n        {THIRD}\n    )\n"
        )
    );
    let record =
        format!("fn f():\n    Record {{ first: {FIRST}, second: {SECOND}, third: {THIRD} }}\n");
    assert_eq!(
        narrow(&record),
        format!(
            "fn f():\n    Record {{\n        first: {FIRST},\n        second: {SECOND},\n        third: {THIRD}\n    }}\n"
        )
    );
    let list = format!("fn f():\n    [{FIRST}, {SECOND}, {THIRD}]\n");
    assert_eq!(
        narrow(&list),
        format!("fn f():\n    [\n        {FIRST},\n        {SECOND},\n        {THIRD}\n    ]\n")
    );
    let parameters = format!("fn f({FIRST}: i64, {SECOND}: i64, {THIRD}: i64) -> i64:\n    +1\n");
    assert_eq!(
        narrow(&parameters),
        format!(
            "fn f(\n    {FIRST}: i64,\n    {SECOND}: i64,\n    {THIRD}: i64\n) -> i64:\n    +1\n"
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
        formatted.contains(&format!("    short()  # {long}  \n")),
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
        format!("fn f():\n    consume(\n        \"{first}\",\n        \"{second}\"\n    )\n")
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
    assert!(formatted.contains("](+1,)"), "{formatted}");
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
fn an_existing_multiline_call_moves_closure_bodies_with_their_headers() {
    let source = format!(
        "fn f():\n    consume({FIRST}, {SECOND}, {THIRD}, fn(value):\n        use_value(value)\n    )\n"
    );
    let formatted = narrow(&source);
    assert!(
        formatted.contains("        fn(value):\n            use_value(value)"),
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
