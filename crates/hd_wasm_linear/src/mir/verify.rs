//! The MIR verifier: structure (variables, projections, labels, counts on
//! counted variables only) and ownership (every count is released or
//! consumed exactly once on every path, nothing is read after its release,
//! and the paths agree at every join), as SIL's ownership verifier checks
//! OSSA.

use super::own::{Cfg, states};
use super::{Func, Kind, Operand, Stmt, each_stmt, reads};

/// The verifier's findings; empty when the function holds.
#[must_use]
pub fn verify(f: &Func) -> Vec<String> {
    let mut errs = Vec::new();
    structure(f, &f.body, &mut Vec::new(), &mut errs);
    structure(f, &f.exit, &mut Vec::new(), &mut errs);
    let g = Cfg::new(f);
    let _ = states(f, &g, &mut |e| errs.push(e));
    errs.sort();
    errs.dedup();
    errs
}

fn structure(f: &Func, l: &[Stmt], labels: &mut Vec<u32>, errs: &mut Vec<String>) {
    let n = f.vars.len();
    let operand = |o: &Operand, errs: &mut Vec<String>| {
        if let Operand::Var(v, p) = o {
            if *v as usize >= n {
                errs.push(format!("_{v}: no such variable"));
                return;
            }
            if let Some(p) = p {
                let width = f.vars[*v as usize].vals.len();
                if p.leaves.iter().any(|k| *k as usize >= width) {
                    errs.push(format!(
                        "{}{}: a projection past its values",
                        f.vars[*v as usize].name, p.path
                    ));
                }
            }
        }
    };
    for s in l {
        for v in reads(s) {
            operand(&Operand::Var(v, None), errs);
        }
        match &s.kind {
            Kind::Assign { dst, proj, .. } => {
                operand(&Operand::Var(*dst, proj.clone()), errs);
            }
            Kind::Count { var, .. } if (*var as usize) < n && !f.vars[*var as usize].counted => {
                errs.push(format!(
                    "s{}: a count of uncounted {}",
                    s.id, f.vars[*var as usize].name
                ));
            }
            Kind::Br { label } if !labels.contains(label) => {
                errs.push(format!(
                    "s{}: br to L{label}, which does not enclose it",
                    s.id
                ));
            }
            Kind::Block { label, body } | Kind::Loop { label, body } => {
                labels.push(*label);
                structure(f, body, labels, errs);
                labels.pop();
            }
            Kind::If { then, els, .. } => {
                structure(f, then, labels, errs);
                structure(f, els, labels, errs);
            }
            _ => {}
        }
    }
    let mut ids = Vec::new();
    each_stmt(l, &mut |s| ids.push(s.id));
    let len = ids.len();
    ids.sort_unstable();
    ids.dedup();
    if ids.len() != len {
        errs.push("two statements share an id".into());
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Count, Func, Kind, Mode, Operand, Rvalue, Var, VarKind, own};
    use crate::module::Val;

    /// A function with one owned `string` parameter and one temporary.
    fn func() -> Func {
        let mut f = Func::new("test".into());
        for (name, kind) in [("s", VarKind::Param(Mode::Owned)), ("t", VarKind::Temp)] {
            f.vars.push(Var {
                name: name.into(),
                tir_local: None,
                ty: "string".into(),
                vals: vec![Val::I32, Val::I32],
                counted: true,
                refs: 1,
                kind,
                cell: false,
            });
        }
        f.params.push(0);
        f
    }

    fn copy(f: &mut Func) {
        let a = f.stmt(
            0,
            Kind::Assign {
                dst: 1,
                proj: None,
                rv: Rvalue::Use(Operand::Var(0, None)),
            },
        );
        let r = f.stmt(
            0,
            Kind::Count {
                op: Count::Retain,
                var: 1,
                ty: "string".into(),
            },
        );
        f.body.extend([a, r]);
    }

    fn release(f: &mut Func, v: u32) {
        let s = f.stmt(
            0,
            Kind::Count {
                op: Count::Release,
                var: v,
                ty: "string".into(),
            },
        );
        f.body.push(s);
    }

    #[test]
    fn an_unreleased_copy_is_a_leak() {
        let mut f = func();
        copy(&mut f);
        release(&mut f, 0);
        let errs = super::verify(&f);
        assert!(errs.iter().any(|e| e.contains("t still owns")), "{errs:?}");
    }

    #[test]
    fn a_second_release_is_reported() {
        let mut f = func();
        release(&mut f, 0);
        release(&mut f, 0);
        let errs = super::verify(&f);
        assert!(errs.iter().any(|e| e.contains("release of s")), "{errs:?}");
    }

    #[test]
    fn a_balanced_copy_holds_and_the_passes_keep_it_balanced() {
        let mut f = func();
        copy(&mut f);
        release(&mut f, 1);
        release(&mut f, 0);
        assert_eq!(super::verify(&f), Vec::<String>::new());
        // The copy is the source's last read: it becomes a move, and only
        // the parameter's own count is released, once.
        own::run(&mut f);
        assert_eq!(super::verify(&f), Vec::<String>::new());
        let (mut retains, mut releases) = (0, 0);
        super::each_stmt(&f.body, &mut |s| match s.kind {
            Kind::Count {
                op: Count::Retain, ..
            } => retains += 1,
            Kind::Count {
                op: Count::Release, ..
            } => releases += 1,
            _ => {}
        });
        assert_eq!((retains, releases), (0, 1), "{:?}", f.body);
    }
}
