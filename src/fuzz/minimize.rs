//! Grammar-aware test-case reduction.
//!
//! The reducer only ever proposes *syntactically valid* smaller programs and
//! keeps a candidate if it still fails with the **same signature** (so a crash
//! is never "minimized" into a different crash). Passes, repeated to a fixed
//! point or until the check budget is spent:
//!
//! 1. **Statement delta debugging** (ddmin-style) inside every block: remove
//!    chunks of n/2, n/4, ..., 1 statements.
//! 2. **Statement unwrapping**: `if c {A} else {B}` → `{A}` / `{B}`,
//!    `while c {A}` → `{A}`, `{A}` → `A` spliced into the parent.
//! 3. **Expression reduction**: replace each node by one of its children, or
//!    by the smallest literal of a plausible type; shrink integer literals.
//!
//! A candidate is accepted only if it lowers the cost
//! `(node count, variable references, literal bits)` lexicographically, which
//! guarantees termination (each component is a non-negative integer).

use serde::Serialize;

use super::exec::{ExecConfig, check};
use crate::lang::ast::{
    Expr, Program, Stmt, block_at_mut, count_blocks, count_exprs, expr_at_mut, program_size,
};

/// What the minimizer did.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MinimizeStats {
    /// Node count before.
    pub original_size: usize,
    /// Node count after.
    pub minimized_size: usize,
    /// Oracle runs spent.
    pub checks: usize,
    /// Full passes over the program.
    pub rounds: usize,
    /// True if the check budget ran out before a fixed point.
    pub budget_exhausted: bool,
}

/// Reduce `p` while `check` keeps reporting `signature`.
pub fn minimize(
    p: &Program,
    signature: &str,
    cfg: &ExecConfig,
    max_checks: usize,
) -> (Program, MinimizeStats) {
    let mut m = Minimizer { signature, cfg, checks: 0, max_checks, best: p.clone() };
    let original_size = program_size(p);
    let mut rounds = 0;
    loop {
        rounds += 1;
        let before = cost(&m.best);
        m.ddmin_blocks();
        m.unwrap_stmts();
        m.reduce_exprs();
        if cost(&m.best) >= before || m.out_of_budget() {
            break;
        }
    }
    let stats = MinimizeStats {
        original_size,
        minimized_size: program_size(&m.best),
        checks: m.checks,
        rounds,
        budget_exhausted: m.out_of_budget(),
    };
    (m.best, stats)
}

/// `(node count, variable references, total bits of integer literals)`.
fn cost(p: &Program) -> (usize, usize, u32) {
    fn expr(e: &Expr, vars: &mut usize, bits: &mut u32) {
        match e {
            Expr::Int(n) => *bits += 64 - n.leading_zeros(),
            Expr::Var(_) => *vars += 1,
            _ => e.children().into_iter().for_each(|c| expr(c, vars, bits)),
        }
    }
    fn go(stmts: &[Stmt], vars: &mut usize, bits: &mut u32) {
        for s in stmts {
            s.exprs().into_iter().for_each(|e| expr(e, vars, bits));
            s.blocks().into_iter().for_each(|b| go(b, vars, bits));
        }
    }
    let (mut vars, mut bits) = (0, 0);
    go(&p.stmts, &mut vars, &mut bits);
    (program_size(p), vars, bits)
}

struct Minimizer<'a> {
    signature: &'a str,
    cfg: &'a ExecConfig,
    checks: usize,
    max_checks: usize,
    best: Program,
}

impl Minimizer<'_> {
    fn out_of_budget(&self) -> bool {
        self.checks >= self.max_checks
    }

    /// Accept `cand` if it is cheaper and still fails the same way.
    fn try_accept(&mut self, cand: Program) -> bool {
        if self.out_of_budget() || cost(&cand) >= cost(&self.best) {
            return false;
        }
        self.checks += 1;
        let same = check(&cand, self.cfg).failure.is_some_and(|f| f.signature == self.signature);
        if same {
            self.best = cand;
        }
        same
    }

    fn ddmin_blocks(&mut self) {
        let mut bi = 0;
        while bi < count_blocks(&self.best) && !self.out_of_budget() {
            let len = block_len(&mut self.best, bi);
            let mut chunk = len.div_ceil(2).max(1);
            while chunk >= 1 && !self.out_of_budget() {
                let mut start = 0;
                while start < block_len(&mut self.best, bi) {
                    let mut cand = self.best.clone();
                    if let Some(b) = block_at_mut(&mut cand, bi) {
                        let end = (start + chunk).min(b.len());
                        b.drain(start..end);
                    }
                    if !self.try_accept(cand) {
                        start += chunk;
                    }
                }
                if chunk == 1 {
                    break;
                }
                chunk = chunk.div_ceil(2);
            }
            bi += 1;
        }
    }

    fn unwrap_stmts(&mut self) {
        let mut bi = 0;
        while bi < count_blocks(&self.best) && !self.out_of_budget() {
            let mut si = 0;
            while si < block_len(&mut self.best, bi) {
                let stmt = block_at_mut(&mut self.best, bi).and_then(|b| b.get(si).cloned());
                let replacements: Vec<Vec<Stmt>> = match stmt {
                    Some(Stmt::If(_, t, f)) => vec![vec![Stmt::Block(t)], vec![Stmt::Block(f)]],
                    Some(Stmt::While(_, b)) => vec![vec![Stmt::Block(b)]],
                    Some(Stmt::Block(b)) => vec![b],
                    _ => Vec::new(),
                };
                for r in replacements {
                    let mut cand = self.best.clone();
                    if let Some(b) = block_at_mut(&mut cand, bi) {
                        b.splice(si..=si, r);
                    }
                    if self.try_accept(cand) {
                        break;
                    }
                }
                si += 1;
            }
            bi += 1;
        }
    }

    fn reduce_exprs(&mut self) {
        let mut i = 0;
        while i < count_exprs(&self.best) && !self.out_of_budget() {
            let node = expr_at_mut(&mut self.best, i).cloned();
            if let Some(node) = node {
                for replacement in expr_candidates(&node) {
                    let mut cand = self.best.clone();
                    if let Some(slot) = expr_at_mut(&mut cand, i) {
                        *slot = replacement;
                    }
                    if self.try_accept(cand) {
                        break;
                    }
                }
            }
            i += 1;
        }
    }
}

fn block_len(p: &mut Program, bi: usize) -> usize {
    block_at_mut(p, bi).map_or(0, |b| b.len())
}

/// Smaller replacements for one expression node, most aggressive first.
fn expr_candidates(e: &Expr) -> Vec<Expr> {
    let mut out: Vec<Expr> = e.children().into_iter().cloned().collect();
    match e {
        Expr::Int(n) => {
            if *n > 1 {
                out.push(Expr::Int(0));
                out.push(Expr::Int(1));
                out.push(Expr::Int(n / 2));
                out.push(Expr::Int(n - 1));
            } else if *n == 1 {
                out.push(Expr::Int(0));
            }
        }
        Expr::Bool(_) => {}
        // Same size, one fewer variable reference: lets the statement pass
        // then delete the now-unused `let`.
        Expr::Var(_) => out.extend([Expr::Int(0), Expr::Int(1), Expr::Bool(false), Expr::Bool(true)]),
        _ => {
            out.extend([Expr::Int(0), Expr::Int(1), Expr::Bool(false), Expr::Bool(true)]);
        }
    }
    out
}
