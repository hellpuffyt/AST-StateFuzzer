//! Static type checker.
//!
//! Soundness claim tested by the fuzzer: if `check` accepts a program, running
//! it never produces [`RuntimeError::Type`](super::interp::RuntimeError::Type)
//! or [`RuntimeError::Unbound`](super::interp::RuntimeError::Unbound).
//! (Overflow, division by zero and running out of fuel are still possible.)

use std::collections::BTreeMap;

use super::ast::{BinOp, Expr, OpClass, Program, Stmt, Ty, UnOp};
use crate::planted::{Bugs, PlantedBug};

/// A type error with a human-readable explanation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("type error: {0}")]
pub struct TypeError(pub String);

/// Type-check a program.
pub fn check(p: &Program) -> Result<(), TypeError> {
    check_with(p, Bugs::NONE)
}

/// Type-check with planted bugs (only [`PlantedBug::TypeckEqMixed`] matters).
pub fn check_with(p: &Program, bugs: Bugs) -> Result<(), TypeError> {
    let mut tc = Checker { scopes: vec![BTreeMap::new()], bugs };
    tc.stmts(&p.stmts)
}

struct Checker {
    scopes: Vec<BTreeMap<String, Ty>>,
    bugs: Bugs,
}

impl Checker {
    fn lookup(&self, name: &str) -> Option<Ty> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied())
    }

    fn scoped(&mut self, stmts: &[Stmt]) -> Result<(), TypeError> {
        self.scopes.push(BTreeMap::new());
        let r = self.stmts(stmts);
        self.scopes.pop();
        r
    }

    fn stmts(&mut self, stmts: &[Stmt]) -> Result<(), TypeError> {
        stmts.iter().try_for_each(|s| self.stmt(s))
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), TypeError> {
        match s {
            Stmt::Let(n, e) => {
                let t = self.expr(e)?;
                if let Some(scope) = self.scopes.last_mut() {
                    scope.insert(n.clone(), t);
                }
                Ok(())
            }
            Stmt::Assign(n, e) => {
                let declared = self
                    .lookup(n)
                    .ok_or_else(|| TypeError(format!("assignment to undeclared variable `{n}`")))?;
                let t = self.expr(e)?;
                if t != declared {
                    return Err(TypeError(format!(
                        "cannot assign {t:?} to `{n}`, which was declared {declared:?}"
                    )));
                }
                Ok(())
            }
            Stmt::Print(e) => self.expr(e).map(|_| ()),
            Stmt::If(c, t, f) => {
                self.expect(c, Ty::Bool, "if condition")?;
                self.scoped(t)?;
                self.scoped(f)
            }
            Stmt::While(c, b) => {
                self.expect(c, Ty::Bool, "while condition")?;
                self.scoped(b)
            }
            Stmt::Block(b) => self.scoped(b),
        }
    }

    fn expect(&self, e: &Expr, want: Ty, what: &str) -> Result<(), TypeError> {
        let got = self.expr(e)?;
        if got == want { Ok(()) } else { Err(TypeError(format!("{what} must be {want:?}, found {got:?}"))) }
    }

    fn expr(&self, e: &Expr) -> Result<Ty, TypeError> {
        match e {
            Expr::Int(_) => Ok(Ty::Int),
            Expr::Bool(_) => Ok(Ty::Bool),
            Expr::Var(v) => {
                self.lookup(v).ok_or_else(|| TypeError(format!("use of undeclared variable `{v}`")))
            }
            Expr::Unary(UnOp::Neg, a) => {
                self.expect(a, Ty::Int, "operand of unary `-`")?;
                Ok(Ty::Int)
            }
            Expr::Unary(UnOp::Not, a) => {
                self.expect(a, Ty::Bool, "operand of `!`")?;
                Ok(Ty::Bool)
            }
            Expr::Binary(op, a, b) => self.binary(*op, a, b),
        }
    }

    fn binary(&self, op: BinOp, a: &Expr, b: &Expr) -> Result<Ty, TypeError> {
        let sym = op.symbol();
        match op.class() {
            OpClass::Arith => {
                self.expect(a, Ty::Int, &format!("left operand of `{sym}`"))?;
                self.expect(b, Ty::Int, &format!("right operand of `{sym}`"))?;
                Ok(Ty::Int)
            }
            OpClass::Order => {
                self.expect(a, Ty::Int, &format!("left operand of `{sym}`"))?;
                self.expect(b, Ty::Int, &format!("right operand of `{sym}`"))?;
                Ok(Ty::Bool)
            }
            OpClass::Logic => {
                self.expect(a, Ty::Bool, &format!("left operand of `{sym}`"))?;
                self.expect(b, Ty::Bool, &format!("right operand of `{sym}`"))?;
                Ok(Ty::Bool)
            }
            OpClass::Equality => {
                let ta = self.expr(a)?;
                let tb = self.expr(b)?;
                if ta != tb && !self.bugs.has(PlantedBug::TypeckEqMixed) {
                    return Err(TypeError(format!(
                        "`{sym}` compares {ta:?} with {tb:?}; both sides must have the same type"
                    )));
                }
                Ok(Ty::Bool)
            }
        }
    }
}
