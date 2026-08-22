//! Tokenizer for the awk sub-language.
//!
//! Newlines are treated as insignificant whitespace; statements are separated by
//! `;` or `}`. There are no regex literals (filter with se's own `g//`/`x//`
//! selectors), so `/` is unambiguously division and the classic awk
//! regex/division lexer hack is avoided entirely.

use anyhow::{Result, anyhow};

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Dollar,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    // assignment
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    PowAssign,
    // comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    // logical
    Not,
    And,
    Or,
    // arithmetic
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Incr,
    Decr,
    // misc
    Question,
    Colon,
}

pub fn lex(src: &str) -> Result<Vec<Tok>> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        match c {
            // whitespace (incl. newline) is insignificant
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
            }
            b'#' => {
                // comment to end of line
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                let (s, ni) = lex_string(b, i)?;
                out.push(Tok::Str(s));
                i = ni;
            }
            b'$' => {
                out.push(Tok::Dollar);
                i += 1;
            }
            b'(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            b')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            b'{' => {
                out.push(Tok::LBrace);
                i += 1;
            }
            b'}' => {
                out.push(Tok::RBrace);
                i += 1;
            }
            b'[' => {
                out.push(Tok::LBracket);
                i += 1;
            }
            b']' => {
                out.push(Tok::RBracket);
                i += 1;
            }
            b',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            b';' => {
                out.push(Tok::Semi);
                i += 1;
            }
            b'?' => {
                out.push(Tok::Question);
                i += 1;
            }
            b':' => {
                out.push(Tok::Colon);
                i += 1;
            }
            b'+' => {
                i += 1;
                if at(b, i, b'+') {
                    out.push(Tok::Incr);
                    i += 1;
                } else if at(b, i, b'=') {
                    out.push(Tok::AddAssign);
                    i += 1;
                } else {
                    out.push(Tok::Plus);
                }
            }
            b'-' => {
                i += 1;
                if at(b, i, b'-') {
                    out.push(Tok::Decr);
                    i += 1;
                } else if at(b, i, b'=') {
                    out.push(Tok::SubAssign);
                    i += 1;
                } else {
                    out.push(Tok::Minus);
                }
            }
            b'*' => {
                i += 1;
                if at(b, i, b'=') {
                    out.push(Tok::MulAssign);
                    i += 1;
                } else {
                    out.push(Tok::Star);
                }
            }
            b'/' => {
                i += 1;
                if at(b, i, b'=') {
                    out.push(Tok::DivAssign);
                    i += 1;
                } else {
                    out.push(Tok::Slash);
                }
            }
            b'%' => {
                i += 1;
                if at(b, i, b'=') {
                    out.push(Tok::ModAssign);
                    i += 1;
                } else {
                    out.push(Tok::Percent);
                }
            }
            b'^' => {
                i += 1;
                if at(b, i, b'=') {
                    out.push(Tok::PowAssign);
                    i += 1;
                } else {
                    out.push(Tok::Caret);
                }
            }
            b'=' => {
                i += 1;
                if at(b, i, b'=') {
                    out.push(Tok::Eq);
                    i += 1;
                } else {
                    out.push(Tok::Assign);
                }
            }
            b'!' => {
                i += 1;
                if at(b, i, b'=') {
                    out.push(Tok::Ne);
                    i += 1;
                } else {
                    out.push(Tok::Not);
                }
            }
            b'<' => {
                i += 1;
                if at(b, i, b'=') {
                    out.push(Tok::Le);
                    i += 1;
                } else {
                    out.push(Tok::Lt);
                }
            }
            b'>' => {
                i += 1;
                if at(b, i, b'=') {
                    out.push(Tok::Ge);
                    i += 1;
                } else {
                    out.push(Tok::Gt);
                }
            }
            b'&' => {
                i += 1;
                if at(b, i, b'&') {
                    out.push(Tok::And);
                    i += 1;
                } else {
                    return Err(anyhow!("awk: unexpected '&' (use '&&')"));
                }
            }
            b'|' => {
                i += 1;
                if at(b, i, b'|') {
                    out.push(Tok::Or);
                    i += 1;
                } else {
                    return Err(anyhow!("awk: unexpected '|' (pipes are not supported)"));
                }
            }
            _ if c.is_ascii_digit() || c == b'.' => {
                let (n, ni) = lex_number(b, i)?;
                out.push(Tok::Num(n));
                i = ni;
            }
            _ if c == b'_' || c.is_ascii_alphabetic() => {
                let start = i;
                while i < b.len() && (b[i] == b'_' || b[i].is_ascii_alphanumeric()) {
                    i += 1;
                }
                out.push(Tok::Ident(src[start..i].to_string()));
            }
            _ => return Err(anyhow!("awk: unexpected character '{}'", c as char)),
        }
    }
    Ok(out)
}

#[inline]
fn at(b: &[u8], i: usize, c: u8) -> bool {
    i < b.len() && b[i] == c
}

fn lex_string(b: &[u8], start: usize) -> Result<(String, usize)> {
    let mut i = start + 1; // skip opening quote
    let mut s = String::new();
    while i < b.len() {
        match b[i] {
            b'"' => return Ok((s, i + 1)),
            b'\\' if i + 1 < b.len() => {
                i += 1;
                match b[i] {
                    b'n' => s.push('\n'),
                    b't' => s.push('\t'),
                    b'r' => s.push('\r'),
                    b'\\' => s.push('\\'),
                    b'"' => s.push('"'),
                    b'/' => s.push('/'),
                    other => s.push(other as char),
                }
                i += 1;
            }
            other => {
                s.push(other as char);
                i += 1;
            }
        }
    }
    Err(anyhow!("awk: unterminated string literal"))
}

fn lex_number(b: &[u8], start: usize) -> Result<(f64, usize)> {
    let mut i = start;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    let text = std::str::from_utf8(&b[start..i]).unwrap();
    let n = text
        .parse::<f64>()
        .map_err(|_| anyhow!("awk: invalid number '{}'", text))?;
    Ok((n, i))
}
