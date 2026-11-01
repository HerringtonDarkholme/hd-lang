//! A thunk of an erased body runs at the caller's type arguments in both
//! optimization pipelines. Its call takes the open instruction's operands
//! as the thunk's parameters, so it runs the callee's plain copy: a
//! promoted copy (§23.40) would read a lent reference as the value.
use crate::owned_adapter_calls::{run, wasm_of};

const LENT_RECEIVER: &str = "\
trait Collector:
    fn show_all[T < Display](self, xs: List[T]) -> string

struct Simple:
    tag: string

impl Collector for Simple:
    fn show_all[T < Display](self, xs: List[T]) -> string:
        let s = self.tag
        for x in xs:
            s = s + x.to_string()
        s

fn run(c: &dyn Collector) -> void $ Console:
    println(c.show_all([+1, +2]))

pub fn main() -> void $ Console:
    run(&Simple { tag: \"s:\" })
";

#[test]
fn a_thunk_lends_its_operand_to_a_ref_self_method() {
    for optimized in [false, true] {
        let wasm = wasm_of(LENT_RECEIVER, optimized);
        assert_eq!(
            run("erased-thunk-lent", &wasm),
            "s:12\n",
            "optimized: {optimized}"
        );
    }
}
