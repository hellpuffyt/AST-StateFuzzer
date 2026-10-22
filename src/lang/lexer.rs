//! Tokenizer for Mini source text.

use super::ParseError;

/// A lexical token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    /// Non-negative integer literal.
    Int(i64),
    /// Identifier (not a keyword).
    Ident(String),
    /// `let`
    Let,
    /// `print`
    Print,
    /// `if`
    If,
    /// `else`
    Else,
    /// `while`
    While,
    /// `true`
    True,
    /// `false`
    False,
    /// Punctuation or operator, e.g. `+`, `<=`, `{`.
    Sym(&'static str),
}

/// A token with its byte offset in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned {
    /// The token.
    pub tok: Tok,
    /// Byte offset of its first character.
    pub at: usize,
}

/// Symbols, longest first so `<=` wins over `<`.
const SYMBOLS: [&str; 18] =
    ["<=", "==", "!=", "&&", "||", "+", "-", "*", "/", "%", "<", "!", "=", ";", "(", ")", "{", "}"];

/// Maximum accepted source size (1 MiB). Larger inputs are rejected, not truncated.
pub const MAX_SOURCE_BYTES: usize = 1 << 20;

/// Split `src` into tokens. `#` starts a comment that runs to end of line.
pub fn lex(src: &str) -> Result<Vec<Spanned>, ParseError> {
    if src.len() > MAX_SOURCE_BYTES {
        return Err(ParseError::new(0, format!("source is larger than {MAX_SOURCE_BYTES} bytes")));
    }
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'#' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let text = &src[start..i];
            let n: i64 = text.parse().map_err(|_| {
                ParseError::new(start, format!("integer literal `{text}` does not fit in i64"))
            })?;
            out.push(Spanned { tok: Tok::Int(n), at: start });
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &src[start..i];
            let tok = match word {
                "let" => Tok::Let,
                "print" => Tok::Print,
                "if" => Tok::If,
                "else" => Tok::Else,
                "while" => Tok::While,
                "true" => Tok::True,
                "false" => Tok::False,
                _ => Tok::Ident(word.to_string()),
            };
            out.push(Spanned { tok, at: start });
            continue;
        }
        let rest = &src[i..];
        match SYMBOLS.iter().find(|s| rest.starts_with(**s)) {
            Some(sym) => {
                out.push(Spanned { tok: Tok::Sym(sym), at: i });
                i += sym.len();
            }
            None => {
                let ch = rest.chars().next().unwrap_or('?');
                return Err(ParseError::new(i, format!("unexpected character `{ch}`")));
            }
        }
    }
    Ok(out)
}
