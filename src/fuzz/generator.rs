//! Grammar- and type-aware program generation.
//!
//! The generator tracks the variables in scope and their types, so almost
//! every program it emits is well-formed *and* well-typed. A configurable
//! fraction of operands is deliberately given the wrong type so the type
//! checker and the interpreter's dynamic checks are exercised too.

use super::rng::Source;
use crate::lang::ast::{BinOp, Expr, Program, Stmt, Ty, UnOp};

/// Knobs for generation. All budgets are hard limits, with one documented
/// allowance: a bounded loop is emitted as `let i = 0; while i < K { ..; i = i + 1; }`,
/// so one statement draw can add two statements to a block and one to the loop
/// body (a block holds at most `2 * max_stmts + 1` statements), and the loop
/// condition and increment have height 2 even when `max_expr_depth` is lower.
/// `tests/grammar.rs` checks these bounds for arbitrary seeds and settings.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GenConfig {
    /// Maximum expression height.
    pub max_expr_depth: u32,
    /// Maximum statement nesting (blocks inside blocks).
    pub max_block_depth: u32,
    /// Maximum statements per block.
    pub max_stmts: u32,
    /// Percent of operands generated with the wrong type.
    pub ill_typed_percent: u32,
    /// Percent of `while` loops that are *not* the bounded counter pattern
    /// (these may run until the fuel budget ends them).
    pub unbounded_loop_percent: u32,
    /// Statement weights: let, assign, print, if, while, block.
    pub stmt_weights: [u32; 6],
}

impl Default for GenConfig {
    fn default() -> Self {
        GenConfig {
            max_expr_depth: 5,
            max_block_depth: 3,
            max_stmts: 6,
            ill_typed_percent: 3,
            unbounded_loop_percent: 10,
            stmt_weights: [30, 15, 25, 12, 8, 3],
        }
    }
}

/// Integer literals that tend to sit on boundaries.
const INTERESTING: [i64; 12] = [0, 1, 2, 3, 7, 10, 64, 100, 255, 1 << 31, (1 << 32) + 1, i64::MAX];

/// Generate a complete program from `src`.
pub fn generate(src: &mut dyn Source, cfg: &GenConfig) -> Program {
    let mut g = Gen { src, cfg, scopes: vec![Vec::new()], next_var: 0 };
    let n = 1 + g.src.below(u64::from(cfg.max_stmts.max(1)));
    let stmts = g.stmts(n, 0);
    Program { stmts }
}

/// Generate a single expression of type `ty` using the given variables.
pub fn generate_expr(
    src: &mut dyn Source,
    cfg: &GenConfig,
    vars: &[(String, Ty)],
    ty: Ty,
    depth: u32,
) -> Expr {
    let scope: Vec<Var> = vars.iter().map(|(n, t)| Var { name: n.clone(), ty: *t, counter: false }).collect();
    let mut g = Gen { src, cfg, scopes: vec![scope], next_var: 0 };
    g.expr(ty, depth)
}

#[derive(Debug, Clone)]
struct Var {
    name: String,
    ty: Ty,
    /// Loop counters are never assigned by generated code, which is what
    /// keeps the counter-pattern loops bounded.
    counter: bool,
}

struct Gen<'a> {
    src: &'a mut dyn Source,
    cfg: &'a GenConfig,
    scopes: Vec<Vec<Var>>,
    next_var: u32,
}

impl Gen<'_> {
    fn visible(&self, ty: Ty) -> Vec<String> {
        self.scopes.iter().flatten().filter(|v| v.ty == ty).map(|v| v.name.clone()).collect()
    }

    fn fresh(&mut self, prefix: &str) -> String {
        let n = self.next_var;
        self.next_var += 1;
        format!("{prefix}{n}")
    }

    fn declare(&mut self, name: String, ty: Ty, counter: bool) {
        if let Some(s) = self.scopes.last_mut() {
            s.push(Var { name, ty, counter });
        }
    }

    fn any_ty(&mut self) -> Ty {
        if self.src.chance(65) { Ty::Int } else { Ty::Bool }
    }

    fn stmts(&mut self, n: u64, depth: u32) -> Vec<Stmt> {
        let mut out = Vec::new();
        for _ in 0..n {
            self.stmt(depth, &mut out);
        }
        out
    }

    fn scoped_block(&mut self, depth: u32) -> Vec<Stmt> {
        self.scopes.push(Vec::new());
        let n = self.src.below(u64::from(self.cfg.max_stmts.max(1)));
        let b = self.stmts(n, depth + 1);
        self.scopes.pop();
        b
    }

    fn stmt(&mut self, depth: u32, out: &mut Vec<Stmt>) {
        let can_nest = depth < self.cfg.max_block_depth;
        let mut w = self.cfg.stmt_weights;
        if !can_nest {
            w[3] = 0;
            w[4] = 0;
            w[5] = 0;
        }
        let assignable: Vec<(String, Ty)> =
            self.scopes.iter().flatten().filter(|v| !v.counter).map(|v| (v.name.clone(), v.ty)).collect();
        if assignable.is_empty() {
            w[1] = 0;
        }
        let d = self.cfg.max_expr_depth;
        match self.src.weighted(&w) {
            1 => {
                let (name, ty) = assignable[self.src.index(assignable.len())].clone();
                let e = self.expr(ty, d);
                out.push(Stmt::Assign(name, e));
            }
            2 => {
                let ty = self.any_ty();
                out.push(Stmt::Print(self.expr(ty, d)));
            }
            3 => {
                let c = self.expr(Ty::Bool, d);
                let t = self.scoped_block(depth);
                let f = if self.src.chance(50) { self.scoped_block(depth) } else { Vec::new() };
                out.push(Stmt::If(c, t, f));
            }
            4 => self.while_loop(depth, out),
            5 => out.push(Stmt::Block(self.scoped_block(depth))),
            _ => {
                let ty = self.any_ty();
                let e = self.expr(ty, d);
                let name = self.fresh("v");
                self.declare(name.clone(), ty, false);
                out.push(Stmt::Let(name, e));
            }
        }
    }

    /// Mostly `let i = 0; while i < K { ...; i = i + 1; }` (always terminates);
    /// sometimes an arbitrary condition that may need the fuel limit.
    fn while_loop(&mut self, depth: u32, out: &mut Vec<Stmt>) {
        if self.src.chance(self.cfg.unbounded_loop_percent) {
            let c = self.expr(Ty::Bool, self.cfg.max_expr_depth);
            let body = self.scoped_block(depth);
            out.push(Stmt::While(c, body));
            return;
        }
        let counter = self.fresh("i");
        let bound = i64::try_from(self.src.below(12)).unwrap_or(0);
        out.push(Stmt::Let(counter.clone(), Expr::Int(0)));
        self.declare(counter.clone(), Ty::Int, true);
        let cond = Expr::Binary(BinOp::Lt, Box::new(Expr::Var(counter.clone())), Box::new(Expr::Int(bound)));
        let mut body = self.scoped_block(depth);
        body.push(Stmt::Assign(
            counter.clone(),
            Expr::Binary(BinOp::Add, Box::new(Expr::Var(counter)), Box::new(Expr::Int(1))),
        ));
        out.push(Stmt::While(cond, body));
    }

    fn literal(&mut self, ty: Ty) -> Expr {
        match ty {
            Ty::Bool => Expr::Bool(self.src.chance(50)),
            Ty::Int => {
                if self.src.chance(40) {
                    Expr::Int(INTERESTING[self.src.index(INTERESTING.len())])
                } else {
                    Expr::Int(i64::try_from(self.src.below(1000)).unwrap_or(0))
                }
            }
        }
    }

    fn leaf(&mut self, ty: Ty) -> Expr {
        let vars = self.visible(ty);
        if !vars.is_empty() && self.src.chance(55) {
            let v = vars[self.src.index(vars.len())].clone();
            return Expr::Var(v);
        }
        self.literal(ty)
    }

    /// Possibly flip the requested type (to produce ill-typed programs).
    fn operand_ty(&mut self, ty: Ty) -> Ty {
        if self.src.chance(self.cfg.ill_typed_percent) {
            match ty {
                Ty::Int => Ty::Bool,
                Ty::Bool => Ty::Int,
            }
        } else {
            ty
        }
    }

    fn expr(&mut self, ty: Ty, depth: u32) -> Expr {
        // Choice 0 always means "leaf", so an exhausted ByteSource terminates.
        if depth <= 1 || self.src.below(4) == 0 {
            return self.leaf(ty);
        }
        let d = depth - 1;
        match ty {
            Ty::Int => {
                if self.src.chance(12) {
                    let t = self.operand_ty(Ty::Int);
                    return Expr::Unary(UnOp::Neg, Box::new(self.expr(t, d)));
                }
                const ARITH: [BinOp; 5] = [BinOp::Add, BinOp::Sub, BinOp::Mul, BinOp::Div, BinOp::Rem];
                let op = ARITH[self.src.weighted(&[30, 30, 20, 10, 10])];
                self.binary(op, Ty::Int, d)
            }
            Ty::Bool => match self.src.below(6) {
                0 => {
                    let t = self.operand_ty(Ty::Bool);
                    Expr::Unary(UnOp::Not, Box::new(self.expr(t, d)))
                }
                1 => self.binary(BinOp::Lt, Ty::Int, d),
                2 => self.binary(BinOp::Le, Ty::Int, d),
                3 => {
                    let op = if self.src.chance(50) { BinOp::Eq } else { BinOp::Ne };
                    let t = self.any_ty();
                    self.binary(op, t, d)
                }
                4 => self.binary(BinOp::And, Ty::Bool, d),
                _ => self.binary(BinOp::Or, Ty::Bool, d),
            },
        }
    }

    fn binary(&mut self, op: BinOp, operand: Ty, d: u32) -> Expr {
        let ta = self.operand_ty(operand);
        let a = self.expr(ta, d);
        let tb = self.operand_ty(operand);
        let b = self.expr(tb, d);
        Expr::Binary(op, Box::new(a), Box::new(b))
    }
}
