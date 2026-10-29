use hd_fmt::{MAX_COLUMNS, format};

fn formatted(source: &str) -> String {
    let once = format(source).unwrap_or_else(|diagnostics| panic!("input failed: {diagnostics:?}"));
    let twice = format(&once)
        .unwrap_or_else(|diagnostics| panic!("output failed: {diagnostics:?}\n{once}"));
    assert_eq!(twice, once, "formatter is not idempotent");
    once
}

#[test]
fn async_using_separates_the_effect_marker_from_the_binding() {
    let source = "fn f!():\n    using!value = open!()\n";
    let expected = "fn f!():\n    using! value = open!()\n";
    assert_eq!(formatted(source), expected);
}

#[test]
fn comment_only_files_keep_every_comment() {
    let source = "# first  \n# second\n";
    assert_eq!(formatted(source), source);
}

#[test]
fn keyword_spacing_distinguishes_postfix_and_leading_variants() {
    let source = "struct Item:\n    x: bool\nimpl Item:\n    fn f(&self, x: bool):\n        self.x\n        true.eq(x)\n        return .None\n";
    assert_eq!(formatted(source), source);
}

#[test]
fn self_receivers_glue_to_index_brackets() {
    let source = "impl Items:\n    fn first(self):\n        self[0]\n    fn first_type():\n        Self[0]\n";
    assert_eq!(formatted(source), source);
}

#[test]
fn prefix_ranges_keep_required_leading_space() {
    let source = "fn f(value: i32):\n    value = ..5\n    for item in ..:\n        use(item)\n";
    assert_eq!(formatted(source), source);
}

#[test]
fn keyword_and_reference_operands_keep_lexical_space() {
    let source = "fn f(xs: List[i32], v: i32):\n    return [v]\nfn g():\n    for x in [1, 2]:\n        pass\n    match [1].cmp([2]):\n        _ => pass\n    edit := &mut [v]\n";
    assert_eq!(formatted(source), source);
}

#[test]
fn adjacent_dereferences_do_not_become_a_power_operator() {
    let source = "fn f(x: i32):\n    x ** * *x\n";
    assert_eq!(formatted(source), source);
}

#[test]
fn a_header_wraps_only_its_outer_binary_expression() {
    let source = "fn f():\n    for _ in value := the_first_descriptive_parameter_name + the_second_descriptive_parameter_name * third + the_third_descriptive_parameter_name ^ fifth ^ sixth:\n        return\n";
    let output = formatted(source);
    assert!(output.contains("* third"), "{output}");
}

#[test]
fn a_suite_valued_comprehension_clause_keeps_the_next_clause_separate() {
    let source = "fn f():\n    { for _ in match true:\n        _ => return\n    for '\\n' in main := if true:\n        return\n    if true => false: false }\n";
    let output = formatted(source);
    assert!(output.contains("return\n    for"), "{output}");
}

#[test]
fn type_operators_use_node_directed_spacing() {
    let source = "fn order[T < Ord & Hash](value: T) -> void $ Console + FsRead:\n    pass\nimpl[T] Show for *T:\n    fn show(&self) -> string:\n        \"x\"\n";
    assert_eq!(formatted(source), source);
}

#[test]
fn typed_data_literal_keeps_space_before_brace() {
    let source = "fn f[T](o: T):\n    Walker::[T] { other: o }\n";
    assert_eq!(formatted(source), source);
}

#[test]
fn a_parent_column_comment_closes_deeper_suites() {
    let source = "fn f():\n    if ready:\n        x()\n    # next statement note\n    y()\n";
    assert_eq!(formatted(source), source);
    let nested = "fn f():\n    if outer:\n        while inner:\n            x()\n        # closes while, stays in if\n        y()\n";
    assert_eq!(formatted(nested), nested);
}

#[test]
fn each_standalone_comment_chooses_its_own_suite() {
    let source = "fn f():\n    if outer:\n        if inner:\n            work()\n        # outer suite\n    # function suite\n# root\nfinish()\n";
    assert_eq!(formatted(source), source);
}

#[test]
fn statement_if_looks_only_at_direct_else_clauses() {
    let source = "fn f():\n    if ready:\n        log(if value: +1 else: +2)\n";
    assert_eq!(formatted(source), source);
}

#[test]
fn pipe_step_suites_remain_on_one_line() {
    let long = "a".repeat(120);
    let source = format!("fn f(value: i32):\n    value |> map(fn(item): consume(item, {long}))\n");
    let output = formatted(&source);
    assert!(
        output.contains(&format!("|> map(fn(item): consume(item, {long}))")),
        "{output}"
    );
}

#[test]
fn binding_and_assignment_values_may_keep_inline_if_expressions() {
    let source =
        "fn f(c: bool):\n    let close: u8 = if c: 125 else: 93\n    x = if c: +1 else: +2\n";
    assert_eq!(formatted(source), source);
}

#[test]
fn short_binary_binding_values_stay_on_their_statement_line() {
    let source = "fn f(value: u32):\n    _ := value >= 0\n    _ := 0 <= value\n";
    assert_eq!(formatted(source), source);
}

#[test]
fn generated_comma_is_included_in_the_fit_decision() {
    let callee = "inner".repeat(16);
    let source = format!(
        "fn f():\n    consume({callee}(first, second), the_second_descriptive_parameter_name)\n"
    );
    let output = formatted(&source);
    assert!(
        output
            .lines()
            .all(|line| line.chars().count() <= MAX_COLUMNS),
        "{output}"
    );
}

#[test]
fn suffix_comment_separator_does_not_count_toward_code_width() {
    let name = "x".repeat(MAX_COLUMNS - "    consume()".len());
    let source = format!("fn f():\n    consume({name}) # note\n");
    assert_eq!(formatted(&source), source);
}

#[test]
fn continuation_wrappers_use_the_printed_column() {
    let left = "a".repeat(60);
    let right = "b".repeat(35);
    let source = format!("fn f():\n    value := ({left} + {right})\n");
    let output = formatted(&source);
    assert!(output.contains("value := (\n"), "{output}");
}

#[test]
fn binary_parentheses_are_added_only_in_whitelisted_slots() {
    let left = "a".repeat(60);
    let right = "b".repeat(60);
    let source = format!("fn f():\n    consume([{left} + {right}])\n");
    let output = formatted(&source);
    assert!(!output.contains("[\n        (\n"), "{output}");
    assert!(
        output.contains(&format!("{left}\n            + {right}")),
        "{output}"
    );
    assert!(
        output
            .lines()
            .all(|line| line.chars().count() <= MAX_COLUMNS),
        "{output}"
    );
}

#[test]
fn a_pipe_closure_chain_keeps_empty_arguments_and_uses_dot_lines() {
    let long = "x".repeat(90);
    let source = format!(
        "fn f(value: Item):\n    value |> map(fn(item): item.configuration().named(\"{long}\"))\n"
    );
    let output = formatted(&source);
    assert!(!output.contains("configuration(\n"), "{output}");
}

#[test]
fn a_postfix_chain_keeps_the_first_complete_call_as_its_head() {
    let long = "x".repeat(100);
    let source = format!("fn f(app: App):\n    app.configuration().named(\"{long}\")\n");
    let output = formatted(&source);
    assert!(
        output.contains("app.configuration()\n        .named("),
        "{output}"
    );
}

#[test]
fn a_call_with_a_data_argument_keeps_the_complete_call_head() {
    let field = "descriptive_field_name".repeat(5);
    let source = format!(
        "impl Builder:\n    fn f(&self):\n        self.frames.push(Frame {{ {field}: value, second: other }})\n"
    );
    let output = formatted(&source);
    assert!(output.contains("self.frames.push(\n"), "{output}");
    assert!(!output.contains("self.frames\n"), "{output}");
    assert!(!output.contains("\n            .push("), "{output}");
}

#[test]
fn long_signatures_break_parameters_before_result_types() {
    let source = "fn transform(first_descriptive_parameter: First, second_descriptive_parameter: Second, third_descriptive_parameter: Third) -> Result[Output, Error]:\n    pass\n";
    let output = formatted(source);
    assert!(output.starts_with("fn transform(\n"), "{output}");
    assert!(output.contains(") -> Result[Output, Error]:\n"), "{output}");
}

#[test]
fn equal_and_brace_spacing_have_one_canonical_spelling() {
    let source = "fn f(c: &Clock, o: Item, store: Store[Item = string]):\n    assert(false, reason=\"x\")\n    $.with(Clock = c):\n        Point { x: 0 }\n    match o:\n        Point { x: value } => value\n";
    assert_eq!(
        formatted(source),
        "fn f(c: &Clock, o: Item, store: Store[Item = string]):\n    assert(false, reason=\"x\")\n    $.with(Clock=c):\n        Point { x: 0 }\n    match o:\n        Point { x: value } => value\n"
    );
}
