//! A generic list that reuses a name already in scope is one error per
//! reuse (`names.type-param.no-redeclare`), found through the scope's name
//! index (#1857), for type and const parameters alike.

use hd_diag::Code;

use crate::type_default_written_run::codes_of;

#[test]
fn a_generic_list_that_reuses_a_name_in_scope_is_one_error() {
    let main = "struct Pair[T, T]:
    a: i32

struct Sized[N < const usize, N < const usize]:
    a: i32

pub fn main() -> void: pass
";
    assert_eq!(codes_of(main), [Code::DuplicateBinding; 2]);
}
