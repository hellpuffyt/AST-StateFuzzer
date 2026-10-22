//! Recursive-descent parser for Mini.
//!
//! Grammar (EBNF):
//! ```text
//! program  = { stmt } ;
//! stmt     = "let" IDENT "=" expr ";" | IDENT "=" expr ";" | "print" expr ";"
//!          | "if" expr block [ "else" block ] | "while" expr block | block ;
//! block    = "{" { stmt } "}" ;
//! expr     = or ;
//! or       = and { "||" and } ;
//! and      = cmp { "&&" cmp } ;
//! cmp      = add [ ( "<" | "<=" | "==" | "!=" ) add ] ;   (* non-associative *)
//! add      = mul { ( "+" | "-" ) mul } ;
//! mul      = unary { ( "*" | "/" | "%" ) unary } ;
//! unary    = ( "-" | "!" ) unary | primary ;
//! primary  = INT | "true" | "false" | IDENT | "(" expr ")" ;
//! ```
//! Nesting is limited to [`MAX_NESTING`] so hostile input cannot overflow the
//! stack of this or any later recursive pass.

use super::ParseError;
use super::ast::{BinOp, Expr, Program, Stmt, UnOp};
use super::lexer::{Spanned, Tok, lex};

/// Maximum nesting of blocks plus expressions accepted by the parser.
pub const MAX_NESTING: usize = 200;

/// Parse a complete program.
pub fn parse(src: &str) -> Result<Program, ParseError> {
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0, depth: 0, end: src.len() };
    let mut stmts = Vec::new();
    while !p.at_end() {
        stmts.push(p.stmt()?);
    }
    Ok(Program { stmts })
}

struct Parser {
    toks: Vec<Spanned>,
    pos: usize,
    depth: usize,
    end: usize,
}

impl Parser {
    fn at_end(&self) -> bool {
        self.pos >= self.toks.len()
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.tok)
    }

    fn offset(&self) -> usize {
        self.toks.get(self.pos).map_or(self.end, |t| t.at)
    }

    fn err<T>(&self, msg: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError::new(self.offset(), msg))
    }

    fn describe(&self) -> String {
        match self.peek() {
            None => "end of input".into(),
            Some(Tok::Int(n)) => format!("`{n}`"),
            Some(Tok::Ident(s)) => format!("`{s}`"),
            Some(Tok::Sym(s)) => format!("`{s}`"),
            Some(t) => format!("`{}`", format!("{t:?}").to_lowercase()),
        }
    }

    fn eat_sym(&mut self, s: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Sym(x)) if *x == s) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_sym(&mut self, s: &str) -> Result<(), ParseError> {
        if self.eat_sym(s) { Ok(()) } else { self.err(format!("expected `{s}`, found {}", self.describe())) }
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            return self.err(format!("nesting deeper than {MAX_NESTING}"));
        }
        Ok(())
    }

    fn ident(&mut self) -> Result<String, ParseError> {
        match self.peek() {
            Some(Tok::Ident(s)) => {
                let s = s.clone();
                self.pos += 1;
                Ok(s)
            }
            _ => self.err(format!("expected identifier, found {}", self.describe())),
        }
    }

    fn stmt(&mut self) -> Result<Stmt, ParseError> {
        match self.peek() {
            Some(Tok::Let) => {
                self.pos += 1;
                let name = self.ident()?;
                self.expect_sym("=")?;
                let e = self.expr()?;
                self.expect_sym(";")?;
                Ok(Stmt::Let(name, e))
            }
            Some(Tok::Print) => {
                self.pos += 1;
                let e = self.expr()?;
                self.expect_sym(";")?;
                Ok(Stmt::Print(e))
            }
            Some(Tok::If) => {
                self.pos += 1;
                let c = self.expr()?;
                let t = self.block()?;
                let f = if matches!(self.peek(), Some(Tok::Else)) {
                    self.pos += 1;
                    self.block()?
                } else {
                    Vec::new()
                };
                Ok(Stmt::If(c, t, f))
            }
            Some(Tok::While) => {
                self.pos += 1;
                let c = self.expr()?;
                let b = self.block()?;
                Ok(Stmt::While(c, b))
            }
            Some(Tok::Sym("{")) => Ok(Stmt::Block(self.block()?)),
            Some(Tok::Ident(_)) => {
                let name = self.ident()?;
                self.expect_sym("=")?;
                let e = self.expr()?;
                self.expect_sym(";")?;
                Ok(Stmt::Assign(name, e))
            }
            _ => self.err(format!("expected a statement, found {}", self.describe())),
        }
    }

    fn block(&mut self) -> Result<Vec<Stmt>, ParseError> {
        self.enter()?;
        self.expect_sym("{")?;
        let mut stmts = Vec::new();
        while !self.eat_sym("}") {
            if self.at_end() {
                return self.err("unterminated block: expected `}`");
            }
            stmts.push(self.stmt()?);
        }
        self.depth -= 1;
        Ok(stmts)
    }

    fn expr(&mut self) -> Result<Expr, ParseError> {
        self.enter()?;
        let e = self.or();
        self.depth -= 1;
        e
    }

    fn left_assoc(
        &mut self,
        ops: &[(&str, BinOp)],
        next: fn(&mut Self) -> Result<Expr, ParseError>,
    ) -> Result<Expr, ParseError> {
        // Every fold deepens the (left-leaning) tree by one, so it counts
        // against the nesting budget just like a recursive call would.
        let base = self.depth;
        let result = (|| -> Result<Expr, ParseError> {
            let mut lhs = next(self)?;
            'outer: loop {
                for (sym, op) in ops {
                    if self.eat_sym(sym) {
                        self.enter()?;
                        let rhs = next(self)?;
                        lhs = Expr::Binary(*op, Box::new(lhs), Box::new(rhs));
                        continue 'outer;
                    }
                }
                return Ok(lhs);
            }
        })();
        self.depth = base;
        result
    }

    fn or(&mut self) -> Result<Expr, ParseError> {
        self.left_assoc(&[("||", BinOp::Or)], Self::and)
    }

    fn and(&mut self) -> Result<Expr, ParseError> {
        self.left_assoc(&[("&&", BinOp::And)], Self::cmp)
    }

    fn cmp(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.add()?;
        for (sym, op) in [("<=", BinOp::Le), ("<", BinOp::Lt), ("==", BinOp::Eq), ("!=", BinOp::Ne)] {
            if self.eat_sym(sym) {
                let rhs = self.add()?;
                if matches!(self.peek(), Some(Tok::Sym("<" | "<=" | "==" | "!="))) {
                    return self.err("comparisons do not chain; add parentheses");
                }
                return Ok(Expr::Binary(op, Box::new(lhs), Box::new(rhs)));
            }
        }
        Ok(lhs)
    }

    fn add(&mut self) -> Result<Expr, ParseError> {
        self.left_assoc(&[("+", BinOp::Add), ("-", BinOp::Sub)], Self::mul)
    }

    fn mul(&mut self) -> Result<Expr, ParseError> {
        self.left_assoc(&[("*", BinOp::Mul), ("/", BinOp::Div), ("%", BinOp::Rem)], Self::unary)
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        let op = if self.eat_sym("-") {
            UnOp::Neg
        } else if self.eat_sym("!") {
            UnOp::Not
        } else {
            return self.primary();
        };
        self.enter()?;
        let inner = self.unary();
        self.depth -= 1;
        Ok(Expr::Unary(op, Box::new(inner?)))
    }

    fn primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek().cloned() {
            Some(Tok::Int(n)) => {
                self.pos += 1;
                Ok(Expr::Int(n))
            }
            Some(Tok::True) => {
                self.pos += 1;
                Ok(Expr::Bool(true))
            }
            Some(Tok::False) => {
                self.pos += 1;
                Ok(Expr::Bool(false))
            }
            Some(Tok::Ident(s)) => {
                self.pos += 1;
                Ok(Expr::Var(s))
            }
            Some(Tok::Sym("(")) => {
                self.pos += 1;
                let e = self.expr()?;
                self.expect_sym(")")?;
                Ok(e)
            }
            _ => self.err(format!("expected an expression, found {}", self.describe())),
        }
    }
}
