//! The Mini language: the fuzzer's target.
//!
//! Mini is a deliberately small, fully specified imperative language with
//! 64-bit integers (checked overflow), booleans, lexical scoping, `if`,
//! `while` and `print`. It has a lexer, parser, canonical printer, static type
//! checker, reference interpreter and constant-folding optimizer: enough moving
//! parts to have real bugs, small enough to reason about every one of them.
//! The full grammar is in [`parser`]; the semantics are in `docs/LANGUAGE.md`.

pub mod ast;
pub mod interp;
pub mod lexer;
pub mod opt;
pub mod parser;
pub mod printer;
pub mod typeck;

/// A lexing or parsing error with the byte offset where it was detected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("parse error at byte {offset}: {message}")]
pub struct ParseError {
    /// Byte offset into the source.
    pub offset: usize,
    /// What went wrong.
    pub message: String,
}

impl ParseError {
    pub(crate) fn new(offset: usize, message: impl Into<String>) -> Self {
        ParseError { offset, message: message.into() }
    }
}
