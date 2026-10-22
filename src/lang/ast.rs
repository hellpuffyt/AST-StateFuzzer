//! Typed abstract syntax tree of the Mini language.
//!
//! Invariant: integer literals are **non-negative** (`0..=i64::MAX`). Negative
//! numbers are written `-n` and parse to `Unary(Neg, Int(n))`. Keeping this
//! invariant makes print → parse an exact structural round trip, which is one
//! of the fuzzer's oracles.

/// Static type of a Mini expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ty {
    /// 64-bit signed integer with checked (erroring) overflow.
    Int,
    /// Boolean.
    Bool,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnOp {
    /// Integer negation `-e`.
    Neg,
    /// Boolean negation `!e`.
    Not,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/` (truncating)
    Div,
    /// `%` (sign of the dividend, like Rust)
    Rem,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `==` (both operands of the same type)
    Eq,
    /// `!=`
    Ne,
    /// `&&` (short-circuit)
    And,
    /// `||` (short-circuit)
    Or,
}

/// Operator families used by mutation and typing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpClass {
    /// `+ - * / %`: Int × Int → Int
    Arith,
    /// `< <=`: Int × Int → Bool
    Order,
    /// `== !=`: T × T → Bool
    Equality,
    /// `&& ||`: Bool × Bool → Bool
    Logic,
}

impl BinOp {
    /// All binary operators, in a fixed order (used for deterministic choice).
    pub const ALL: [BinOp; 11] = [
        BinOp::Add,
        BinOp::Sub,
        BinOp::Mul,
        BinOp::Div,
        BinOp::Rem,
        BinOp::Lt,
        BinOp::Le,
        BinOp::Eq,
        BinOp::Ne,
        BinOp::And,
        BinOp::Or,
    ];

    /// The operator's family.
    pub fn class(self) -> OpClass {
        match self {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => OpClass::Arith,
            BinOp::Lt | BinOp::Le => OpClass::Order,
            BinOp::Eq | BinOp::Ne => OpClass::Equality,
            BinOp::And | BinOp::Or => OpClass::Logic,
        }
    }

    /// Source spelling.
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::And => "&&",
            BinOp::Or => "||",
        }
    }

    /// Binding strength (higher binds tighter).
    pub fn precedence(self) -> u8 {
        match self {
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Lt | BinOp::Le | BinOp::Eq | BinOp::Ne => 3,
            BinOp::Add | BinOp::Sub => 4,
            BinOp::Mul | BinOp::Div | BinOp::Rem => 5,
        }
    }

    /// Stable small integer id (coverage features, signatures).
    pub fn id(self) -> u32 {
        match self {
            BinOp::Add => 0,
            BinOp::Sub => 1,
            BinOp::Mul => 2,
            BinOp::Div => 3,
            BinOp::Rem => 4,
            BinOp::Lt => 5,
            BinOp::Le => 6,
            BinOp::Eq => 7,
            BinOp::Ne => 8,
            BinOp::And => 9,
            BinOp::Or => 10,
        }
    }
}

/// An expression. Expressions are pure: they can fail but never mutate state.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    /// Non-negative integer literal.
    Int(i64),
    /// Boolean literal.
    Bool(bool),
    /// Variable reference.
    Var(String),
    /// Unary operation.
    Unary(UnOp, Box<Expr>),
    /// Binary operation.
    Binary(BinOp, Box<Expr>, Box<Expr>),
}

/// A statement.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Stmt {
    /// `let name = expr;` declares in the current scope (shadowing allowed).
    Let(String, Expr),
    /// `name = expr;` updates the innermost binding of `name`.
    Assign(String, Expr),
    /// `print expr;` appends a value to the program output.
    Print(Expr),
    /// `if cond { .. } else { .. }` (an empty else is not printed).
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    /// `while cond { .. }`
    While(Expr, Vec<Stmt>),
    /// `{ .. }` a nested scope.
    Block(Vec<Stmt>),
}

/// A whole program: a list of statements in the top-level scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Program {
    /// Top-level statements.
    pub stmts: Vec<Stmt>,
}

impl Expr {
    /// Number of expression nodes in this tree.
    pub fn size(&self) -> usize {
        match self {
            Expr::Int(_) | Expr::Bool(_) | Expr::Var(_) => 1,
            Expr::Unary(_, a) => 1 + a.size(),
            Expr::Binary(_, a, b) => 1 + a.size() + b.size(),
        }
    }

    /// Height of this tree (a leaf has depth 1).
    pub fn depth(&self) -> usize {
        match self {
            Expr::Int(_) | Expr::Bool(_) | Expr::Var(_) => 1,
            Expr::Unary(_, a) => 1 + a.depth(),
            Expr::Binary(_, a, b) => 1 + a.depth().max(b.depth()),
        }
    }

    /// Direct children, left to right.
    pub fn children(&self) -> Vec<&Expr> {
        match self {
            Expr::Int(_) | Expr::Bool(_) | Expr::Var(_) => Vec::new(),
            Expr::Unary(_, a) => vec![a],
            Expr::Binary(_, a, b) => vec![a, b],
        }
    }

    /// The type this expression has *by construction of its root*, if the
    /// root alone decides it (variables do not).
    pub fn shape_type(&self) -> Option<Ty> {
        match self {
            Expr::Int(_) | Expr::Unary(UnOp::Neg, _) => Some(Ty::Int),
            Expr::Bool(_) | Expr::Unary(UnOp::Not, _) => Some(Ty::Bool),
            Expr::Var(_) => None,
            Expr::Binary(op, _, _) => Some(match op.class() {
                OpClass::Arith => Ty::Int,
                _ => Ty::Bool,
            }),
        }
    }
}

impl Stmt {
    /// Expressions directly owned by this statement (not nested statements').
    pub fn exprs(&self) -> Vec<&Expr> {
        match self {
            Stmt::Let(_, e) | Stmt::Assign(_, e) | Stmt::Print(e) => vec![e],
            Stmt::If(c, _, _) | Stmt::While(c, _) => vec![c],
            Stmt::Block(_) => Vec::new(),
        }
    }

    /// Nested statement blocks owned by this statement.
    pub fn blocks(&self) -> Vec<&Vec<Stmt>> {
        match self {
            Stmt::If(_, t, e) => vec![t, e],
            Stmt::While(_, b) | Stmt::Block(b) => vec![b],
            _ => Vec::new(),
        }
    }
}

/// Number of AST nodes (statements + expression nodes): the size metric used
/// by the generator budget and by the minimizer.
pub fn program_size(p: &Program) -> usize {
    stmts_size(&p.stmts)
}

fn stmts_size(stmts: &[Stmt]) -> usize {
    stmts
        .iter()
        .map(|s| {
            1 + s.exprs().iter().map(|e| e.size()).sum::<usize>()
                + s.blocks().iter().map(|b| stmts_size(b)).sum::<usize>()
        })
        .sum()
}

/// Deepest nesting of expressions or blocks anywhere in the program. Used to
/// keep recursive passes (parser, printer, interpreter) far from stack limits.
pub fn program_depth(p: &Program) -> usize {
    stmts_depth(&p.stmts)
}

fn stmts_depth(stmts: &[Stmt]) -> usize {
    stmts
        .iter()
        .map(|s| {
            let e = s.exprs().iter().map(|e| e.depth()).max().unwrap_or(0);
            let b = s.blocks().iter().map(|b| 1 + stmts_depth(b)).max().unwrap_or(0);
            1 + e.max(b)
        })
        .max()
        .unwrap_or(0)
}

/// Total number of expression nodes in the program (pre-order indexable).
pub fn count_exprs(p: &Program) -> usize {
    fn go(stmts: &[Stmt]) -> usize {
        stmts
            .iter()
            .map(|s| {
                s.exprs().iter().map(|e| e.size()).sum::<usize>()
                    + s.blocks().iter().map(|b| go(b)).sum::<usize>()
            })
            .sum()
    }
    go(&p.stmts)
}

/// The `idx`-th expression node in pre-order (statement order, then each
/// statement's own expressions, then its nested blocks).
pub fn expr_at_mut(p: &mut Program, idx: usize) -> Option<&mut Expr> {
    let mut i = idx;
    find_in_stmts(&mut p.stmts, &mut i)
}

fn find_in_stmts<'a>(stmts: &'a mut [Stmt], idx: &mut usize) -> Option<&'a mut Expr> {
    for s in stmts.iter_mut() {
        match s {
            Stmt::Let(_, e) | Stmt::Assign(_, e) | Stmt::Print(e) => {
                if let Some(x) = find_in_expr(e, idx) {
                    return Some(x);
                }
            }
            Stmt::If(c, t, f) => {
                if let Some(x) = find_in_expr(c, idx) {
                    return Some(x);
                }
                if let Some(x) = find_in_stmts(t, idx) {
                    return Some(x);
                }
                if let Some(x) = find_in_stmts(f, idx) {
                    return Some(x);
                }
            }
            Stmt::While(c, b) => {
                if let Some(x) = find_in_expr(c, idx) {
                    return Some(x);
                }
                if let Some(x) = find_in_stmts(b, idx) {
                    return Some(x);
                }
            }
            Stmt::Block(b) => {
                if let Some(x) = find_in_stmts(b, idx) {
                    return Some(x);
                }
            }
        }
    }
    None
}

fn find_in_expr<'a>(e: &'a mut Expr, idx: &mut usize) -> Option<&'a mut Expr> {
    if *idx == 0 {
        return Some(e);
    }
    *idx -= 1;
    match e {
        Expr::Unary(_, a) => find_in_expr(a, idx),
        Expr::Binary(_, a, b) => {
            if let Some(x) = find_in_expr(a, idx) {
                return Some(x);
            }
            find_in_expr(b, idx)
        }
        _ => None,
    }
}

/// Number of statement blocks (the top level counts as block 0).
pub fn count_blocks(p: &Program) -> usize {
    fn go(stmts: &[Stmt]) -> usize {
        1 + stmts.iter().flat_map(|s| s.blocks()).map(|b| go(b)).sum::<usize>()
    }
    go(&p.stmts)
}

/// The `idx`-th statement block in pre-order (block 0 is the top level).
pub fn block_at_mut(p: &mut Program, idx: usize) -> Option<&mut Vec<Stmt>> {
    let mut i = idx;
    find_block(&mut p.stmts, &mut i)
}

fn find_block<'a>(stmts: &'a mut Vec<Stmt>, idx: &mut usize) -> Option<&'a mut Vec<Stmt>> {
    if *idx == 0 {
        return Some(stmts);
    }
    *idx -= 1;
    for s in stmts.iter_mut() {
        match s {
            Stmt::If(_, t, f) => {
                if let Some(x) = find_block(t, idx) {
                    return Some(x);
                }
                if let Some(x) = find_block(f, idx) {
                    return Some(x);
                }
            }
            Stmt::While(_, b) | Stmt::Block(b) => {
                if let Some(x) = find_block(b, idx) {
                    return Some(x);
                }
            }
            _ => {}
        }
    }
    None
}
