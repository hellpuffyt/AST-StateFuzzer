//! Canonical pretty-printer. `parse(print(p)) == p` for every program that
//! respects the AST invariants; the fuzzer checks exactly that.

use super::ast::{BinOp, Expr, Program, Stmt, UnOp};
use crate::planted::{Bugs, PlantedBug};

/// Print a program in canonical form (4-space indentation, minimal parens).
pub fn print_program(p: &Program) -> String {
    print_program_with(p, Bugs::NONE)
}

/// Print with planted bugs enabled (only [`PlantedBug::PrintSubParens`] matters here).
pub fn print_program_with(p: &Program, bugs: Bugs) -> String {
    let mut out = String::new();
    let pr = Printer { bugs };
    pr.stmts(&p.stmts, 0, &mut out);
    out
}

/// Print a single expression in canonical form.
pub fn print_expr(e: &Expr) -> String {
    let mut out = String::new();
    Printer { bugs: Bugs::NONE }.expr(e, &mut out);
    out
}

struct Printer {
    bugs: Bugs,
}

impl Printer {
    fn stmts(&self, stmts: &[Stmt], indent: usize, out: &mut String) {
        for s in stmts {
            self.stmt(s, indent, out);
        }
    }

    fn block(&self, stmts: &[Stmt], indent: usize, out: &mut String) {
        out.push_str("{\n");
        self.stmts(stmts, indent + 1, out);
        pad(indent, out);
        out.push('}');
    }

    fn stmt(&self, s: &Stmt, indent: usize, out: &mut String) {
        pad(indent, out);
        match s {
            Stmt::Let(n, e) => {
                out.push_str("let ");
                out.push_str(n);
                out.push_str(" = ");
                self.expr(e, out);
                out.push(';');
            }
            Stmt::Assign(n, e) => {
                out.push_str(n);
                out.push_str(" = ");
                self.expr(e, out);
                out.push(';');
            }
            Stmt::Print(e) => {
                out.push_str("print ");
                self.expr(e, out);
                out.push(';');
            }
            Stmt::If(c, t, f) => {
                out.push_str("if ");
                self.expr(c, out);
                out.push(' ');
                self.block(t, indent, out);
                if !f.is_empty() {
                    out.push_str(" else ");
                    self.block(f, indent, out);
                }
            }
            Stmt::While(c, b) => {
                out.push_str("while ");
                self.expr(c, out);
                out.push(' ');
                self.block(b, indent, out);
            }
            Stmt::Block(b) => self.block(b, indent, out),
        }
        out.push('\n');
    }

    fn expr(&self, e: &Expr, out: &mut String) {
        match e {
            Expr::Int(n) => out.push_str(&n.to_string()),
            Expr::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Expr::Var(v) => out.push_str(v),
            Expr::Unary(op, a) => {
                out.push(match op {
                    UnOp::Neg => '-',
                    UnOp::Not => '!',
                });
                self.wrapped(a, matches!(**a, Expr::Binary(..)), out);
            }
            Expr::Binary(op, a, b) => {
                let p = op.precedence();
                let comparison = p == 3;
                // Left operand: parenthesize looser operators, and any
                // comparison under a comparison (they do not chain).
                let lp = binary_prec(a);
                self.wrapped(a, lp < p || (comparison && lp == p), out);
                out.push(' ');
                out.push_str(op.symbol());
                out.push(' ');
                // Right operand: operators are left-associative, so an
                // equal-precedence right child needs parentheses.
                let rp = binary_prec(b);
                let mut need = rp <= p;
                if self.bugs.has(PlantedBug::PrintSubParens)
                    && *op == BinOp::Sub
                    && matches!(**b, Expr::Binary(BinOp::Sub | BinOp::Add, ..))
                {
                    need = false; // planted bug: a - (b - c) printed as a - b - c
                }
                self.wrapped(b, need, out);
            }
        }
    }

    fn wrapped(&self, e: &Expr, parens: bool, out: &mut String) {
        if parens {
            out.push('(');
            self.expr(e, out);
            out.push(')');
        } else {
            self.expr(e, out);
        }
    }
}

/// Precedence of a binary node, or "tighter than anything" for other nodes.
fn binary_prec(e: &Expr) -> u8 {
    match e {
        Expr::Binary(op, _, _) => op.precedence(),
        _ => u8::MAX,
    }
}

fn pad(indent: usize, out: &mut String) {
    for _ in 0..indent {
        out.push_str("    ");
    }
}
