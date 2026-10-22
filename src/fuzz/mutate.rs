//! AST-level mutation operators.
//!
//! Mutations work on the tree, never on text, so every mutant is
//! syntactically valid. Types are respected where they can be inferred; a
//! mutant may still be ill-typed or use a variable out of scope, which is
//! useful input for the type checker and the interpreter's dynamic checks.

use std::collections::BTreeMap;

use super::generator::{GenConfig, generate_expr};
use super::rng::Source;
use crate::lang::ast::{
    BinOp, Expr, OpClass, Program, Stmt, Ty, block_at_mut, count_blocks, count_exprs, expr_at_mut,
};

/// The mutation operators, in a fixed order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mutation {
    /// Replace a random expression with a freshly generated one of the same type.
    ReplaceExpr,
    /// Replace an expression by one of its own children.
    Hoist,
    /// Swap a binary operator for another of the same family.
    SwapOp,
    /// Nudge an integer literal or flip a boolean.
    TweakLiteral,
    /// Delete a statement.
    DeleteStmt,
    /// Duplicate a statement in place.
    DuplicateStmt,
    /// Swap two adjacent statements.
    SwapStmts,
    /// Copy a statement from another corpus entry (crossover).
    Splice,
}

const OPS: [Mutation; 8] = [
    Mutation::ReplaceExpr,
    Mutation::Hoist,
    Mutation::SwapOp,
    Mutation::TweakLiteral,
    Mutation::DeleteStmt,
    Mutation::DuplicateStmt,
    Mutation::SwapStmts,
    Mutation::Splice,
];
const WEIGHTS: [u32; 8] = [25, 10, 15, 15, 8, 8, 7, 12];

/// Apply one random mutation; returns which one was attempted. `donor` is
/// used for [`Mutation::Splice`] and may be the same program.
pub fn mutate(p: &mut Program, donor: &Program, src: &mut dyn Source, cfg: &GenConfig) -> Mutation {
    let m = OPS[src.weighted(&WEIGHTS)];
    match m {
        Mutation::ReplaceExpr => replace_expr(p, src, cfg),
        Mutation::Hoist => hoist(p, src),
        Mutation::SwapOp => swap_op(p, src),
        Mutation::TweakLiteral => tweak_literal(p, src),
        Mutation::DeleteStmt => with_block(p, src, |b, src| {
            if !b.is_empty() {
                b.remove(src.index(b.len()));
            }
        }),
        Mutation::DuplicateStmt => with_block(p, src, |b, src| {
            if !b.is_empty() {
                let i = src.index(b.len());
                let s = b[i].clone();
                b.insert(i, s);
            }
        }),
        Mutation::SwapStmts => with_block(p, src, |b, src| {
            if b.len() >= 2 {
                let i = src.index(b.len() - 1);
                b.swap(i, i + 1);
            }
        }),
        Mutation::Splice => {
            let pick = donor.stmts.get(src.index(donor.stmts.len().max(1))).cloned();
            if let Some(s) = pick {
                with_block(p, src, |b, src| {
                    let at = src.index(b.len() + 1);
                    b.insert(at, s.clone());
                });
            }
        }
    }
    m
}

fn with_block(p: &mut Program, src: &mut dyn Source, f: impl FnOnce(&mut Vec<Stmt>, &mut dyn Source)) {
    let n = count_blocks(p);
    let idx = src.index(n);
    if let Some(b) = block_at_mut(p, idx) {
        f(b, src);
    }
}

/// Variables declared anywhere in the program, with their inferred types
/// (best effort: the type of the first `let` initializer we can type).
pub fn declared_vars(p: &Program) -> Vec<(String, Ty)> {
    let mut map: BTreeMap<String, Ty> = BTreeMap::new();
    fn infer(e: &Expr, map: &BTreeMap<String, Ty>) -> Option<Ty> {
        match e {
            Expr::Var(v) => map.get(v).copied(),
            _ => e.shape_type(),
        }
    }
    fn go(stmts: &[Stmt], map: &mut BTreeMap<String, Ty>) {
        for s in stmts {
            if let Stmt::Let(n, e) = s {
                if let Some(t) = infer(e, map) {
                    map.entry(n.clone()).or_insert(t);
                }
            }
            for b in s.blocks() {
                go(b, map);
            }
        }
    }
    go(&p.stmts, &mut map);
    map.into_iter().collect()
}

fn expr_type(e: &Expr, vars: &[(String, Ty)]) -> Ty {
    match e {
        Expr::Var(v) => vars.iter().find(|(n, _)| n == v).map_or(Ty::Int, |(_, t)| *t),
        _ => e.shape_type().unwrap_or(Ty::Int),
    }
}

fn replace_expr(p: &mut Program, src: &mut dyn Source, cfg: &GenConfig) {
    let n = count_exprs(p);
    if n == 0 {
        return;
    }
    let vars = declared_vars(p);
    let idx = src.index(n);
    if let Some(e) = expr_at_mut(p, idx) {
        let ty = expr_type(e, &vars);
        let depth = 1 + src.below(u64::from(cfg.max_expr_depth.max(1)));
        *e = generate_expr(src, cfg, &vars, ty, u32::try_from(depth).unwrap_or(1));
    }
}

fn hoist(p: &mut Program, src: &mut dyn Source) {
    let n = count_exprs(p);
    if n == 0 {
        return;
    }
    let idx = src.index(n);
    if let Some(e) = expr_at_mut(p, idx) {
        let kids: Vec<Expr> = e.children().into_iter().cloned().collect();
        if !kids.is_empty() {
            *e = kids[src.index(kids.len())].clone();
        }
    }
}

fn swap_op(p: &mut Program, src: &mut dyn Source) {
    let n = count_exprs(p);
    if n == 0 {
        return;
    }
    let idx = src.index(n);
    if let Some(Expr::Binary(op, _, _)) = expr_at_mut(p, idx) {
        let family: Vec<BinOp> = BinOp::ALL.into_iter().filter(|o| o.class() == op.class()).collect();
        // Equality and order are both Int × Int → Bool when the operands are ints;
        // allowing `<` ↔ `==` swaps exercises more comparisons.
        let family = if matches!(op.class(), OpClass::Order) {
            vec![BinOp::Lt, BinOp::Le, BinOp::Eq, BinOp::Ne]
        } else {
            family
        };
        *op = family[src.index(family.len())];
    }
}

fn tweak_literal(p: &mut Program, src: &mut dyn Source) {
    let n = count_exprs(p);
    if n == 0 {
        return;
    }
    let idx = src.index(n);
    match expr_at_mut(p, idx) {
        Some(Expr::Int(v)) => {
            *v = match src.below(6) {
                0 => v.saturating_add(1),
                1 => v.saturating_sub(1).max(0),
                2 => 0,
                3 => i64::MAX,
                4 => v.saturating_mul(2),
                _ => *v / 2,
            };
        }
        Some(Expr::Bool(b)) => *b = !*b,
        _ => {}
    }
}
