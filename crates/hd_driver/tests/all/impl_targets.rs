//! Implementation targets and ownership (spec 09 "Implementation
//! Targets", "Implementation Ownership", "Implementation Modules",
//! "Inherent Methods On `dyn` Types", "Overlap"; spec 14 `annot.no-trait.unique`
//! and `annot.template.tuple.overlap`): a rejected implementation gets
//! one error on its header and is then left out, so nothing in its body
//! is reported and no overlap follows. The accepted forms keep working.

use hd_diag::Code;
use hd_driver::{
    Dependency, Executor, Goal, Host, NoClock, Output, Packages, build, build_packages,
};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(files: &[(&str, &str)]) -> Output {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

/// The root package `main.hd` with the package `ext` as its dependency.
fn analyze_with_dependency(main: &str, ext: &str) -> Output {
    let mut root = MemorySources::default();
    root.insert("src/main.hd", main);
    let mut dep = MemorySources::default();
    dep.insert("src/lib.hd", ext);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &root,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        requires: vec![("ext".to_owned(), 1)],
        deps: vec![Dependency {
            name: "ext".to_owned(),
            identity: "ext".to_owned(),
            sources: &dep,
            requires: Vec::new(),
            fetched: false,
        }],
        ..Packages::default()
    };
    build_packages(&host, "app", &packages, &Goal::Analyze)
}

fn codes(out: &Output) -> Vec<Code> {
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

/// The files report exactly `code`, once.
#[track_caller]
fn rejects(files: &[(&str, &str)], code: Code) {
    let out = analyze(files);
    assert_eq!(codes(&out), vec![code], "{}", out.render());
}

#[track_caller]
fn rejects_main(main: &str, code: Code) {
    rejects(&[("main.hd", main)], code);
}

#[track_caller]
fn accepts(files: &[(&str, &str)]) {
    let out = analyze(files);
    assert_eq!(codes(&out), vec![], "{}", out.render());
}

#[track_caller]
fn accepts_main(main: &str) {
    accepts(&[("main.hd", main)]);
}

const DESCRIBE: &str = "trait Describe:\n    fn describe(self) -> string\n\n";

// trait.target.bare-parameter, trait.from.reflexive

#[test]
fn a_bare_parameter_target_is_one_error_and_its_body_is_left_out() {
    // The body names an unknown value, which is not reported.
    for header in [
        "impl[T] Describe for T:",
        "impl[T < Display] Describe for T:",
        "impl[T < Eq, U] Describe for T:",
    ] {
        let src =
            format!("{DESCRIBE}{header}\n    fn describe(self) -> string:\n        missing_name\n");
        rejects_main(&src, Code::BareParameterImplTarget);
    }
}

#[test]
fn a_bare_parameter_target_is_rejected_in_an_inherent_block_too() {
    rejects_main(
        "impl[T] T:\n    fn twice(self) -> i32:\n        1\n",
        Code::BareParameterImplTarget,
    );
}

#[test]
fn the_reflexive_from_is_a_bare_parameter_target() {
    rejects_main(
        "use std.convert.From\n\nimpl[T] From[T] for T:\n    fn from(value: T) -> T:\n        value\n",
        Code::BareParameterImplTarget,
    );
}

#[test]
fn a_bare_parameter_target_with_a_delegation_is_rejected() {
    rejects_main(
        "trait Describe:\n    fn describe(self) -> string\n\ndata Inner:\n    n: i32\n\nimpl[T] Describe for T by inner\n",
        Code::BareParameterImplTarget,
    );
}

#[test]
fn a_constructor_target_over_a_parameter_is_valid() {
    accepts_main(&format!(
        "{DESCRIBE}data Box[T]:\n    value: T\n\nimpl[T] Describe for Box[T]:\n    fn describe(self) -> string:\n        \"box\"\n\nimpl[T] Describe for T?:\n    fn describe(self) -> string:\n        \"maybe\"\n\nimpl[T] Describe for List[T]:\n    fn describe(self) -> string:\n        \"list\"\n"
    ));
}

#[test]
fn a_derivation_template_is_no_bare_parameter_target() {
    accepts_main(
        "use std.structure.{Structure, Field, Variant, Walker}\n\ntrait Encode:\n    fn encode(self) -> string\n\nimpl Encode for i64:\n    fn encode(self) -> string:\n        \"$self\"\n\nstruct Sink:\n    n: i32\n\nimpl[S] Walker[S] for Sink:\n    type Error = never\n\n    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:\n        .Ok(())\n\n    fn member[F](&mut self, h: Field[S, F], value: F) -> Result[void, never]:\n        self.n = self.n + 1\n        .Ok(())\n\nimpl[T] Encode for T by Structure:\n    fn encode(self) -> string:\n        let mut w = Sink { n: 0 }\n        _ := Structure::walk(self, &mut w)\n        \"\"\n",
    );
}

#[test]
fn the_standard_numeric_families_stay_valid() {
    accepts_main(
        "fn main_sum(a: i32, b: i32) -> i32:\n    a + b * 2 - (a % 3)\n\nfn flip(a: u8) -> u8:\n    a & 1\n",
    );
}

// trait.target.no-mut

#[test]
fn an_outer_mut_target_is_one_error_and_its_body_is_left_out() {
    rejects_main(
        "data Counter:\n    value: i32\n\nimpl mut Counter:\n    fn reset(mut self) -> void:\n        self.value = 0\n        missing_name\n",
        Code::MutableImplTarget,
    );
    rejects_main(
        "data Counter:\n    value: i32\n\ntrait Marker\n\nimpl Marker for mut Counter\n",
        Code::MutableImplTarget,
    );
}

#[test]
fn a_mut_inside_the_head_is_valid() {
    accepts_main(
        "data User:\n    name: string\n\ndata Shelf:\n    n: i32\n\ntrait Store[T]\n\nimpl Store[User] for Shelf\nimpl Store[mut User] for Shelf\n",
    );
}

// trait.own.orphan, trait.own.inherent

#[test]
fn a_foreign_trait_for_a_foreign_type_is_an_orphan() {
    rejects_main(
        "impl Display for i32:\n    fn to_string(self) -> string:\n        \"$self\"\n",
        Code::OrphanImpl,
    );
    rejects_main(
        "impl Display for fn() -> i32:\n    fn to_string(self) -> string:\n        \"function\"\n",
        Code::OrphanImpl,
    );
}

#[test]
fn an_orphan_impl_is_left_out_of_the_overlap_check() {
    // The same pair twice would overlap, but each is one orphan error.
    let out = analyze(&[(
        "main.hd",
        "impl Display for i32:\n    fn to_string(self) -> string:\n        \"a\"\n\nimpl Display for i32:\n    fn to_string(self) -> string:\n        \"b\"\n",
    )]);
    assert_eq!(
        codes(&out),
        vec![Code::OrphanImpl, Code::OrphanImpl],
        "{}",
        out.render()
    );
}

#[test]
fn a_local_trait_argument_gives_ownership() {
    accepts_main(
        "use std.ops.Add\n\ndata Money:\n    cents: i64\n\nimpl Add[Money] for i32:\n    type Out = Money\n    fn add(self, rhs: Money) -> Money:\n        rhs\n",
    );
}

#[test]
fn an_inherent_impl_of_a_foreign_type_is_an_orphan() {
    rejects_main(
        "impl i32:\n    fn twice(self) -> i32:\n        self * 2\n",
        Code::OrphanImpl,
    );
    rejects_main(
        "impl List[i32]:\n    fn first_or(self) -> i32:\n        0\n",
        Code::OrphanImpl,
    );
    rejects_main(
        "impl string?:\n    fn present(self) -> bool:\n        true\n",
        Code::OrphanImpl,
    );
}

#[test]
fn an_inherent_impl_of_a_dependency_type_is_an_orphan() {
    let out = analyze_with_dependency(
        "use dep.ext.Gadget\n\nimpl Gadget:\n    fn size(self) -> i32:\n        1\n",
        "pub data Gadget:\n    pub n: i32\n",
    );
    assert_eq!(codes(&out), vec![Code::OrphanImpl], "{}", out.render());
}

#[test]
fn a_dependency_trait_for_a_dependency_type_is_an_orphan() {
    let out = analyze_with_dependency(
        "use dep.ext.{Gadget, Sized}\n\nimpl Sized for Gadget:\n    fn size(self) -> i32:\n        1\n",
        "pub trait Sized:\n    fn size(self) -> i32\n\npub data Gadget:\n    pub n: i32\n",
    );
    assert_eq!(codes(&out), vec![Code::OrphanImpl], "{}", out.render());
}

#[test]
fn a_dependency_trait_for_a_local_type_is_valid() {
    let out = analyze_with_dependency(
        "use dep.ext.Sized\n\ndata Local:\n    n: i32\n\nimpl Sized for Local:\n    fn size(self) -> i32:\n        self.n\n",
        "pub trait Sized:\n    fn size(self) -> i32\n",
    );
    assert_eq!(codes(&out), vec![], "{}", out.render());
}

#[test]
fn an_inherent_impl_on_a_tuple_or_an_alias_is_invalid_not_an_orphan() {
    rejects_main(
        "impl (i32, i32):\n    fn sum(self) -> i32:\n        self._0 + self._1\n",
        Code::InvalidImplTarget,
    );
    rejects_main(
        "data User:\n    name: string\n\ntype Person = User\n\nimpl Person:\n    fn label(self) -> string:\n        self.name\n",
        Code::InvalidImplTarget,
    );
    let out = analyze_with_dependency(
        "use dep.ext.Gadget\n\ntype Alias = Gadget\n\nimpl Alias:\n    fn size(self) -> i32:\n        1\n",
        "pub data Gadget:\n    pub n: i32\n",
    );
    assert_eq!(
        codes(&out),
        vec![Code::InvalidImplTarget],
        "{}",
        out.render()
    );
}

#[test]
fn an_inherent_impl_on_a_local_type_is_valid() {
    accepts_main(
        "data Counter:\n    value: i32\n\nimpl Counter:\n    fn bump(self) -> i32:\n        self.value + 1\n\nimpl[T] Box[T]:\n    fn get(self) -> T:\n        self.value\n\ndata Box[T]:\n    value: T\n",
    );
}

// trait.own.module.*, trait.own.dyn-inherent.*

const ALPHA: &str = "pub trait Greeter:\n    fn greet(self) -> string\n\npub data Person:\n    pub name: string\n\npub data Money:\n    pub cents: i64\n";

#[test]
fn a_trait_impl_outside_the_module_of_the_trait_or_target_is_nonlocal() {
    // The body reads a private-looking name, which is not reported.
    rejects(
        &[
            ("alpha.hd", ALPHA),
            (
                "beta.hd",
                "use pkg.alpha.{Greeter, Person}\n\nimpl Greeter for Person:\n    fn greet(self) -> string:\n        missing_name\n",
            ),
        ],
        Code::NonlocalImpl,
    );
}

#[test]
fn a_trait_impl_in_the_module_of_the_trait_or_target_is_valid() {
    accepts(&[
        ("alpha.hd", ALPHA),
        (
            "beta.hd",
            "use pkg.alpha.Greeter\n\ndata Local:\n    n: i32\n\nimpl Greeter for Local:\n    fn greet(self) -> string:\n        \"hi\"\n",
        ),
        (
            "gamma.hd",
            "pub trait Named:\n    fn name(self) -> string\n",
        ),
        (
            "alpha2.hd",
            "pub data Gadget:\n    pub n: i32\n\nimpl Gadget:\n    pub fn size(self) -> i32:\n        self.n\n",
        ),
    ]);
    // The module of the target takes the trait of another module.
    accepts(&[
        (
            "gamma.hd",
            "pub trait Named:\n    fn name(self) -> string\n",
        ),
        (
            "delta.hd",
            "use pkg.gamma.Named\n\npub data Part:\n    pub n: i32\n\nimpl Named for Part:\n    fn name(self) -> string:\n        \"p\"\n",
        ),
    ]);
}

#[test]
fn a_trait_argument_gives_module_ownership_only_in_its_own_module() {
    let adder = "use std.ops.Add\n\n";
    accepts(&[(
        "alpha.hd",
        &format!(
            "{adder}pub data Money:\n    pub cents: i64\n\nimpl Add[Money] for i32:\n    type Out = Money\n    fn add(self, rhs: Money) -> Money:\n        rhs\n"
        ),
    )]);
    rejects(
        &[
            ("alpha.hd", "pub data Money:\n    pub cents: i64\n"),
            (
                "beta.hd",
                &format!(
                    "{adder}use pkg.alpha.Money\n\nimpl Add[Money] for i32:\n    type Out = Money\n    fn add(self, rhs: Money) -> Money:\n        rhs\n"
                ),
            ),
        ],
        Code::NonlocalImpl,
    );
}

#[test]
fn an_inherent_impl_outside_the_module_of_its_type_is_nonlocal() {
    rejects(
        &[
            ("alpha.hd", ALPHA),
            (
                "beta.hd",
                "use pkg.alpha.Person\n\nimpl Person:\n    fn shout(self) -> string:\n        missing_name\n",
            ),
        ],
        Code::NonlocalImpl,
    );
}

#[test]
fn a_dyn_inherent_impl_lives_in_the_module_of_its_trait() {
    let shape = "pub trait Shape:\n    fn area(self) -> f64\n\nimpl dyn Shape:\n    fn double(self) -> f64:\n        self.area() * 2.0\n";
    accepts(&[("alpha.hd", shape)]);
    rejects(
        &[
            ("alpha.hd", "pub trait Shape:\n    fn area(self) -> f64\n"),
            (
                "beta.hd",
                "use pkg.alpha.Shape\n\nimpl dyn Shape:\n    fn double(self) -> f64:\n        missing_name\n",
            ),
        ],
        Code::NonlocalImpl,
    );
}

#[test]
fn a_dyn_inherent_impl_in_another_package_is_an_orphan() {
    let out = analyze_with_dependency(
        "use dep.ext.Shape\n\nimpl dyn Shape:\n    fn double(self) -> f64:\n        self.area() * 2.0\n",
        "pub trait Shape:\n    fn area(self) -> f64\n",
    );
    assert_eq!(codes(&out), vec![Code::OrphanImpl], "{}", out.render());
}

// trait.overlap, annot.no-trait.unique, annot.template.tuple.overlap

const STRUCTURE: &str = "use std.structure.Structure\n\ndata Tag:\n    text: string\n\nfn tag(text: string) -> Tag:\n    Tag { text: text }\n\ndata User:\n    name: string\n    email: string\n\ndata Group:\n    title: string\n\n";

#[test]
fn a_second_derivation_block_with_no_trait_overlaps_the_first() {
    rejects_main(
        &format!(
            "{STRUCTURE}impl User by Structure:\n    name += [tag(\"n\")]\n\nimpl User by Structure:\n    email += [tag(\"e\")]\n"
        ),
        Code::OverlappingImpl,
    );
}

#[test]
fn a_third_block_is_a_second_error() {
    let out = analyze(&[(
        "main.hd",
        &format!(
            "{STRUCTURE}impl User by Structure:\n    name += [tag(\"n\")]\n\nimpl User by Structure:\n    email += [tag(\"e\")]\n\nimpl User by Structure\n"
        ),
    )]);
    assert_eq!(
        codes(&out),
        vec![Code::OverlappingImpl, Code::OverlappingImpl],
        "{}",
        out.render()
    );
}

#[test]
fn one_block_per_type_is_valid() {
    accepts_main(&format!(
        "{STRUCTURE}impl User by Structure:\n    name += [tag(\"n\")]\n\nimpl Group by Structure:\n    title += [tag(\"t\")]\n"
    ));
}

#[test]
fn a_misplaced_block_does_not_count_as_the_first() {
    // The block in `beta` is misplaced and reported as that only; the
    // block in `alpha` is the type's one block.
    let out = analyze(&[
        (
            "alpha.hd",
            &format!("{STRUCTURE}pub data Item:\n    pub name: string\n\nimpl Item by Structure\n"),
        ),
        (
            "beta.hd",
            "use std.structure.Structure\nuse pkg.alpha.Item\n\nimpl Item by Structure\n",
        ),
    ]);
    assert_eq!(
        codes(&out),
        vec![Code::MisplacedDerivation],
        "{}",
        out.render()
    );
}

const TUPLE_TEMPLATE: &str = "use std.function.Tuple\nuse std.structure.{Structure, Field, Variant, Walker}\n\ntrait Encode:\n    fn encode(self) -> string\n\nimpl Encode for i64:\n    fn encode(self) -> string:\n        \"$self\"\n\nstruct Encoder:\n    out: string\n\nimpl[S] Walker[S] for Encoder:\n    type Error = never\n\n    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:\n        .Ok(())\n\n    fn member[F < Encode](&mut self, h: Field[S, F], value: F) -> Result[void, never]:\n        self.out = self.out + value.encode() + \";\"\n        .Ok(())\n\nimpl[T < Tuple] Encode for T by Structure:\n    fn encode(self) -> string:\n        let mut w = Encoder { out: \"\" }\n        _ := Structure::walk(self, &mut w)\n        w.out\n\n";

#[test]
fn a_hand_written_tuple_impl_overlaps_the_tuple_template() {
    for target in ["(i64, i64)", "(i64, i64, i64)", "(i64,)"] {
        rejects_main(
            &format!(
                "{TUPLE_TEMPLATE}impl Encode for {target}:\n    fn encode(self) -> string:\n        \"hand\"\n"
            ),
            Code::OverlappingImpl,
        );
    }
}

#[test]
fn a_generic_tuple_impl_overlaps_the_tuple_template() {
    rejects_main(
        &format!(
            "{TUPLE_TEMPLATE}impl[A, B] Encode for (A, B):\n    fn encode(self) -> string:\n        \"hand\"\n"
        ),
        Code::OverlappingImpl,
    );
}

#[test]
fn the_tuple_template_beside_non_tuple_impls_is_valid() {
    accepts_main(&format!(
        "{TUPLE_TEMPLATE}data Pair:\n    a: i64\n\nimpl Encode for Pair:\n    fn encode(self) -> string:\n        \"pair\"\n\nimpl Encode for List[i64]:\n    fn encode(self) -> string:\n        \"list\"\n"
    ));
}

#[test]
fn a_tuple_impl_of_a_trait_without_a_template_is_valid() {
    accepts_main(
        "trait Mark:\n    fn mark(self) -> string\n\nimpl Mark for (i64, i64):\n    fn mark(self) -> string:\n        \"pair\"\n",
    );
}
