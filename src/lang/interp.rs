//! Reference interpreter: the system under test.
//!
//! Execution is bounded by **fuel** (one unit per statement and per expression
//! node evaluated) and by an optional wall-clock limit checked every 1024
//! steps. Both bounds produce a [`RuntimeError`] instead of hanging. Untrusted
//! programs therefore only ever run inside this in-process interpreter; nothing
//! is handed to a shell or compiled to native code.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use super::ast::{BinOp, Expr, Program, Stmt, UnOp};
use crate::coverage::{Coverage, Feature};
use crate::planted::{Bugs, PlantedBug};

/// A runtime value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Value {
    /// Integer.
    Int(i64),
    /// Boolean.
    Bool(bool),
}

/// Why execution stopped early.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeError {
    /// Checked arithmetic overflowed.
    Overflow,
    /// Division or remainder by zero.
    DivByZero,
    /// An operator was applied to a value of the wrong type.
    Type,
    /// A variable was read or assigned before being declared.
    Unbound,
    /// The step budget ran out (possible infinite loop).
    OutOfFuel,
    /// The wall-clock budget ran out.
    WallTimeout,
}

impl RuntimeError {
    /// True for errors that mean "the budget ran out" rather than "the program
    /// did something wrong".
    pub fn is_timeout(self) -> bool {
        matches!(self, RuntimeError::OutOfFuel | RuntimeError::WallTimeout)
    }

    fn feature_id(self) -> u32 {
        match self {
            RuntimeError::Overflow => 0,
            RuntimeError::DivByZero => 1,
            RuntimeError::Type => 2,
            RuntimeError::Unbound => 3,
            RuntimeError::OutOfFuel => 4,
            RuntimeError::WallTimeout => 5,
        }
    }
}

/// Execution limits.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Maximum number of steps.
    pub fuel: u64,
    /// Optional wall-clock budget.
    pub wall: Option<Duration>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { fuel: 20_000, wall: Some(Duration::from_secs(2)) }
    }
}

/// Everything observable about one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Values printed, in order (kept even when execution later fails).
    pub output: Vec<Value>,
    /// `Ok` or the error that stopped execution.
    pub result: Result<(), RuntimeError>,
    /// Steps consumed.
    pub steps: u64,
}

/// Run a program with the correct semantics.
pub fn run(p: &Program, limits: Limits) -> (Outcome, Coverage) {
    run_with(p, limits, Bugs::NONE)
}

/// Run with planted bugs (only [`PlantedBug::InterpRemPanic`] matters here).
pub fn run_with(p: &Program, limits: Limits, bugs: Bugs) -> (Outcome, Coverage) {
    let mut m = Machine {
        scopes: vec![BTreeMap::new()],
        output: Vec::new(),
        steps: 0,
        limits,
        start: Instant::now(),
        cov: Coverage::default(),
        bugs,
    };
    let result = m.block_body(&p.stmts);
    if let Err(e) = result {
        m.cov.hit(Feature::Error(e.feature_id()));
    }
    let outcome = Outcome { output: m.output, result, steps: m.steps };
    (outcome, m.cov)
}

struct Machine {
    scopes: Vec<BTreeMap<String, Value>>,
    output: Vec<Value>,
    steps: u64,
    limits: Limits,
    start: Instant,
    cov: Coverage,
    bugs: Bugs,
}

type R<T> = Result<T, RuntimeError>;

impl Machine {
    fn tick(&mut self) -> R<()> {
        self.steps += 1;
        if self.steps > self.limits.fuel {
            return Err(RuntimeError::OutOfFuel);
        }
        if self.steps % 1024 == 0 {
            if let Some(wall) = self.limits.wall {
                if self.start.elapsed() > wall {
                    return Err(RuntimeError::WallTimeout);
                }
            }
        }
        Ok(())
    }

    fn scoped(&mut self, stmts: &[Stmt]) -> R<()> {
        self.scopes.push(BTreeMap::new());
        let r = self.block_body(stmts);
        self.scopes.pop();
        r
    }

    fn block_body(&mut self, stmts: &[Stmt]) -> R<()> {
        stmts.iter().try_for_each(|s| self.stmt(s))
    }

    fn lookup(&self, name: &str) -> R<Value> {
        self.scopes.iter().rev().find_map(|s| s.get(name).copied()).ok_or(RuntimeError::Unbound)
    }

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        self.tick()?;
        match s {
            Stmt::Let(n, e) => {
                self.cov.hit(Feature::Stmt(0));
                let v = self.expr(e)?;
                if let Some(scope) = self.scopes.last_mut() {
                    scope.insert(n.clone(), v);
                }
                Ok(())
            }
            Stmt::Assign(n, e) => {
                self.cov.hit(Feature::Stmt(1));
                let v = self.expr(e)?;
                let slot =
                    self.scopes.iter_mut().rev().find_map(|s| s.get_mut(n)).ok_or(RuntimeError::Unbound)?;
                *slot = v;
                Ok(())
            }
            Stmt::Print(e) => {
                self.cov.hit(Feature::Stmt(2));
                let v = self.expr(e)?;
                self.cov.hit(Feature::Printed(value_class(v)));
                self.output.push(v);
                Ok(())
            }
            Stmt::If(c, t, f) => {
                let b = self.bool(c)?;
                self.cov.hit(Feature::Branch(u32::from(b)));
                if b { self.scoped(t) } else { self.scoped(f) }
            }
            Stmt::While(c, body) => {
                let mut iterations: u64 = 0;
                while self.bool(c)? {
                    iterations += 1;
                    self.scoped(body)?;
                    self.tick()?;
                }
                self.cov.hit(Feature::Loop(bucket(iterations)));
                Ok(())
            }
            Stmt::Block(b) => {
                self.cov.hit(Feature::Stmt(5));
                self.scoped(b)
            }
        }
    }

    fn bool(&mut self, e: &Expr) -> R<bool> {
        match self.expr(e)? {
            Value::Bool(b) => Ok(b),
            Value::Int(_) => Err(RuntimeError::Type),
        }
    }

    fn int(&mut self, e: &Expr) -> R<i64> {
        match self.expr(e)? {
            Value::Int(n) => Ok(n),
            Value::Bool(_) => Err(RuntimeError::Type),
        }
    }

    fn expr(&mut self, e: &Expr) -> R<Value> {
        self.tick()?;
        match e {
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Var(v) => self.lookup(v),
            Expr::Unary(UnOp::Neg, a) => {
                let n = self.int(a)?;
                n.checked_neg().map(Value::Int).ok_or(RuntimeError::Overflow)
            }
            Expr::Unary(UnOp::Not, a) => Ok(Value::Bool(!self.bool(a)?)),
            Expr::Binary(op, a, b) => {
                for (side, child) in [(0, a), (1, b)] {
                    if let Expr::Binary(c, _, _) = &**child {
                        self.cov.hit(Feature::OpPair(op.id(), side, c.id()));
                    }
                }
                let r = self.binary(*op, a, b);
                self.cov.hit(Feature::Op(op.id(), r.as_ref().map_or(3, |v| value_class(*v))));
                r
            }
        }
    }

    fn binary(&mut self, op: BinOp, a: &Expr, b: &Expr) -> R<Value> {
        match op {
            BinOp::And => Ok(Value::Bool(self.bool(a)? && self.bool(b)?)),
            BinOp::Or => Ok(Value::Bool(self.bool(a)? || self.bool(b)?)),
            BinOp::Eq | BinOp::Ne => {
                let x = self.expr(a)?;
                let y = self.expr(b)?;
                let same = match (x, y) {
                    (Value::Int(x), Value::Int(y)) => x == y,
                    (Value::Bool(x), Value::Bool(y)) => x == y,
                    _ => return Err(RuntimeError::Type),
                };
                Ok(Value::Bool(if op == BinOp::Eq { same } else { !same }))
            }
            BinOp::Lt => Ok(Value::Bool(self.int(a)? < self.int(b)?)),
            BinOp::Le => Ok(Value::Bool(self.int(a)? <= self.int(b)?)),
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                let x = self.int(a)?;
                let y = self.int(b)?;
                arith(op, x, y, self.bugs).map(Value::Int)
            }
        }
    }
}

/// Checked integer arithmetic shared by the interpreter and the optimizer's
/// constant folder (so both agree by construction on the correct path).
pub fn arith(op: BinOp, x: i64, y: i64, bugs: Bugs) -> Result<i64, RuntimeError> {
    match op {
        BinOp::Add => x.checked_add(y).ok_or(RuntimeError::Overflow),
        BinOp::Sub => x.checked_sub(y).ok_or(RuntimeError::Overflow),
        BinOp::Mul => x.checked_mul(y).ok_or(RuntimeError::Overflow),
        BinOp::Div if y == 0 => Err(RuntimeError::DivByZero),
        BinOp::Div => x.checked_div(y).ok_or(RuntimeError::Overflow),
        BinOp::Rem if y == 0 => Err(RuntimeError::DivByZero),
        BinOp::Rem => {
            let r = x.checked_rem(y).ok_or(RuntimeError::Overflow)?;
            if bugs.has(PlantedBug::InterpRemPanic) && r < 0 {
                planted_rem_panic(r);
            }
            Ok(r)
        }
        _ => Err(RuntimeError::Type),
    }
}

/// Planted bug: index a lookup table with a negative remainder. The cast turns
/// a negative `r` into a huge index and the bounds check panics.
#[allow(clippy::cast_sign_loss, clippy::indexing_slicing)]
fn planted_rem_panic(r: i64) {
    let table = [0u8; 8];
    let idx = r as usize;
    std::hint::black_box(table[idx]);
}

/// Coarse class of a value: 0 = zero/false, 1 = positive/true, 2 = negative.
fn value_class(v: Value) -> u32 {
    match v {
        Value::Bool(b) => u32::from(b),
        Value::Int(0) => 0,
        Value::Int(n) if n > 0 => 1,
        Value::Int(_) => 2,
    }
}

/// Logarithmic loop-iteration bucket: 0, 1, 2-3, 4-7, 8-15, 16+.
fn bucket(n: u64) -> u32 {
    match n {
        0 => 0,
        1 => 1,
        2..=3 => 2,
        4..=7 => 3,
        8..=15 => 4,
        _ => 5,
    }
}
