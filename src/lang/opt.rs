//! A small optimizer (constant folding and dead-branch removal).
//!
//! It is deliberately conservative: a rewrite is only applied when it cannot
//! change the program's observable behaviour, *including which runtime error
//! it raises*. The fuzzer's `optimizer-equivalence` oracle checks that claim by
//! running the original and the optimized program and comparing outputs.

use super::ast::{BinOp, Expr, OpClass, Program, Stmt, UnOp};
use super::interp::arith;
use crate::planted::{Bugs, PlantedBug};

/// Optimize a program with the correct rules.
pub fn optimize(p: &Program) -> Program {
    optimize_with(p, Bugs::NONE)
}

/// Optimize with planted bugs (only [`PlantedBug::OptSubAssoc`] matters here).
pub fn optimize_with(p: &Program, bugs: Bugs) -> Program {
    Program { stmts: p.stmts.iter().map(|s| stmt(s, bugs)).collect() }
}

fn block(stmts: &[Stmt], bugs: Bugs) -> Vec<Stmt> {
    stmts.iter().map(|s| stmt(s, bugs)).collect()
}

fn stmt(s: &Stmt, bugs: Bugs) -> Stmt {
    match s {
        Stmt::Let(n, e) => Stmt::Let(n.clone(), expr(e, bugs)),
        Stmt::Assign(n, e) => Stmt::Assign(n.clone(), expr(e, bugs)),
        Stmt::Print(e) => Stmt::Print(expr(e, bugs)),
        Stmt::If(c, t, f) => match expr(c, bugs) {
            // Both branches open a scope, so the taken branch stays a block.
            Expr::Bool(true) => Stmt::Block(block(t, bugs)),
            Expr::Bool(false) => Stmt::Block(block(f, bugs)),
            c => Stmt::If(c, block(t, bugs), block(f, bugs)),
        },
        Stmt::While(c, b) => match expr(c, bugs) {
            Expr::Bool(false) => Stmt::Block(Vec::new()),
            c => Stmt::While(c, block(b, bugs)),
        },
        Stmt::Block(b) => Stmt::Block(block(b, bugs)),
    }
}

/// Build the literal for an integer value, respecting the AST invariant that
/// literals are non-negative. `None` if it cannot be written (`i64::MIN`).
fn int_literal(v: i64) -> Option<Expr> {
    if v >= 0 {
        Some(Expr::Int(v))
    } else {
        v.checked_neg().map(|n| Expr::Unary(UnOp::Neg, Box::new(Expr::Int(n))))
    }
}

/// The constant integer an expression denotes syntactically, if any.
fn const_int(e: &Expr) -> Option<i64> {
    match e {
        Expr::Int(n) => Some(*n),
        Expr::Unary(UnOp::Neg, a) => match **a {
            Expr::Int(n) => n.checked_neg(),
            _ => None,
        },
        _ => None,
    }
}

fn expr(e: &Expr, bugs: Bugs) -> Expr {
    match e {
        Expr::Int(_) | Expr::Bool(_) | Expr::Var(_) => e.clone(),
        Expr::Unary(op, a) => {
            let a = expr(a, bugs);
            match (op, &a) {
                (UnOp::Not, Expr::Bool(b)) => Expr::Bool(!b),
                (UnOp::Neg, Expr::Int(0)) => Expr::Int(0),
                _ => Expr::Unary(*op, Box::new(a)),
            }
        }
        Expr::Binary(op, a, b) => {
            let a = expr(a, bugs);
            let b = expr(b, bugs);
            if bugs.has(PlantedBug::OptSubAssoc) && *op == BinOp::Sub {
                if let Expr::Binary(BinOp::Sub, b1, b2) = &b {
                    // Planted bug: a - (b - c) => (a - b) - c   (correct: a - b + c)
                    let left = Expr::Binary(BinOp::Sub, Box::new(a), b1.clone());
                    return Expr::Binary(BinOp::Sub, Box::new(left), b2.clone());
                }
            }
            fold(*op, &a, &b).unwrap_or_else(|| Expr::Binary(*op, Box::new(a), Box::new(b)))
        }
    }
}

/// Fold an operator applied to two constants. Returns `None` (no rewrite)
/// whenever folding would hide a runtime error or a type error.
fn fold(op: BinOp, a: &Expr, b: &Expr) -> Option<Expr> {
    match op.class() {
        OpClass::Arith => {
            let v = arith(op, const_int(a)?, const_int(b)?, Bugs::NONE).ok()?;
            int_literal(v)
        }
        OpClass::Order => {
            let (x, y) = (const_int(a)?, const_int(b)?);
            Some(Expr::Bool(if op == BinOp::Lt { x < y } else { x <= y }))
        }
        OpClass::Equality => {
            let same = match (a, b) {
                (Expr::Bool(x), Expr::Bool(y)) => x == y,
                _ => const_int(a)? == const_int(b)?,
            };
            Some(Expr::Bool(if op == BinOp::Eq { same } else { !same }))
        }
        OpClass::Logic => match (a, b) {
            (Expr::Bool(x), Expr::Bool(y)) => {
                Some(Expr::Bool(if op == BinOp::And { *x && *y } else { *x || *y }))
            }
            _ => None,
        },
    }
}
