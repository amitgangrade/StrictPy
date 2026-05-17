//! Lexer. See spec §3.
//!
//! Emits a flat stream of `Token`s, including synthetic `Indent` / `Dedent` /
//! `Newline` tokens for the off-side rule (spec §3.3). 4-space indentation
//! only; tabs are an error.

use crate::ast::Span;
use crate::error::CompileError;

/// A lexed token: kind + source span.
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// All token kinds emitted by the lexer.
///
/// Keywords from spec §3.5, operators/punctuation from §3.8, literal kinds
/// from §3.7. `Indent` / `Dedent` / `Newline` are synthetic (spec §3.3).
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // ── Identifiers & literals ───────────────────────────────────────────
    Ident(String),
    IntLit { value: i128, suffix: Option<IntSuffix> },
    FloatLit { value: f64, suffix: Option<FloatSuffix> },
    StrLit(String),
    RawStrLit(String),
    BytesLit(Vec<u8>),
    FStrStart,
    FStrChunk(String),
    FStrExprStart,
    FStrExprEnd,
    FStrEnd,
    CharLit(char),

    // ── Keywords (spec §3.5) ─────────────────────────────────────────────
    KwAnd, KwAs, KwAssert, KwAsync, KwAwait, KwBreak,
    KwCase, KwClass, KwContinue, KwDef, KwDel, KwElif,
    KwElse, KwExcept, KwFinal, KwFinally, KwFn, KwFor,
    KwFrom, KwGlobal, KwIf, KwImport, KwIn, KwIs,
    KwLambda, KwMatch, KwNot, KwOpen, KwOr, KwPass,
    KwProtocol, KwRaise, KwReturn, KwSealed, KwTry, KwWhile,
    KwWith, KwYield, KwTrue, KwFalse, KwNone,

    // Reserved-but-unused (spec §3.5)
    KwEffect, KwRegion, KwUnsafe,

    // ── Operators & punctuation (spec §3.8) ──────────────────────────────
    Plus, Minus, Star, Slash, DoubleSlash, Percent, DoubleStar,
    Assign,
    PlusEq, MinusEq, StarEq, SlashEq, DoubleSlashEq, PercentEq, DoubleStarEq,
    EqEq, NotEq, Lt, Gt, LtEq, GtEq,
    Amp, Pipe, Caret, Tilde, Shl, Shr,
    AmpEq, PipeEq, CaretEq, ShlEq, ShrEq,
    LParen, RParen, LBracket, RBracket, LBrace, RBrace,
    Comma, Colon, Semicolon, Dot, Arrow, At, Question, QuestionQuestion,

    // ── Synthetic (spec §3.3, §10.2) ─────────────────────────────────────
    Indent,
    Dedent,
    Newline,
    Eof,
}

/// Suffix on an integer literal (`0i64`, `0u32`, ...). See spec §3.7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntSuffix {
    I8, I16, I32, I64,
    U8, U16, U32, U64,
}

/// Suffix on a float literal (`2.0f32`). See spec §3.7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatSuffix {
    F32,
    F64,
}

// ─────────────────────────────────────────────────────────────────────────
//  Internal state
// ─────────────────────────────────────────────────────────────────────────

/// Tracks one level of nested f-string expression.
///
/// When we hit `{` inside an f-string we push one of these; the lexer then
/// emits ordinary tokens until it sees the matching `}` (tracking inner
/// brace depth so e.g. `{ {1: 2}.get(x) }` works).
#[derive(Debug, Clone)]
struct FStringFrame {
    /// Whether the f-string was triple-quoted.
    triple: bool,
    /// The quote character (`"` or `'`).
    quote: u8,
    /// Brace depth inside the current `{...}` interpolation. 0 means we are
    /// in literal-chunk mode (not inside any `{...}`).
    brace_depth: u32,
}

/// The lexer. Owns nothing of the source beyond a borrowed `&str`; the lexed
/// tokens own their lexeme strings outright so the source can be dropped
/// after lexing.
pub struct Lexer<'src> {
    source: &'src str,
    bytes: &'src [u8],
    pos: usize,
    line: u32,
    col: u32,

    /// Indent levels in spaces, starting with 0. See spec §3.3.
    indent_stack: Vec<u32>,
    /// Implicit-line-continuation depth from `()`, `[]`, `{}` (spec §3.2).
    paren_depth: u32,
    /// True at the start of a logical line, before we've consumed any
    /// leading whitespace.
    at_line_start: bool,
    /// Queued DEDENT tokens to emit (one Pop per call until drained).
    pending_dedents: u32,
    /// We just popped indent and need to emit a synthetic Newline before
    /// the dedents — actually we always emit Newline first, then dedents.
    /// This holds whether we've already emitted the trailing Newline at EOF.
    emitted_final_newline: bool,
    /// Once true, every further `next_token` returns Eof.
    done: bool,

    /// Stack of f-string frames; non-empty means we are inside one or more
    /// nested f-strings.
    fstring_stack: Vec<FStringFrame>,
    /// If `Some`, the next call should emit this queued kind (used to break
    /// the f-string state machine across calls).
    queued: std::collections::VecDeque<Token>,
}

impl<'src> Lexer<'src> {
    pub fn new(source: &'src str) -> Self {
        // Strip UTF-8 BOM (spec §3.1).
        let source = source.strip_prefix('\u{FEFF}').unwrap_or(source);
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
            indent_stack: vec![0],
            paren_depth: 0,
            at_line_start: true,
            pending_dedents: 0,
            emitted_final_newline: false,
            done: false,
            fstring_stack: Vec::new(),
            queued: std::collections::VecDeque::new(),
        }
    }

    /// Advance the lexer and return the next token. Returns `Eof` repeatedly
    /// once the input is exhausted.
    pub fn next_token(&mut self) -> Result<Token, CompileError> {
        // Drain anything queued from a previous call (e.g. multi-token
        // f-string transitions).
        if let Some(t) = self.queued.pop_front() {
            return Ok(t);
        }

        // Drain pending dedents before doing anything else.
        if self.pending_dedents > 0 {
            self.pending_dedents -= 1;
            let sp = self.point_span();
            return Ok(Token { kind: TokenKind::Dedent, span: sp });
        }

        if self.done {
            return Ok(Token { kind: TokenKind::Eof, span: self.point_span() });
        }

        // Are we currently lexing an f-string's literal chunk?
        if let Some(frame) = self.fstring_stack.last().cloned() {
            if frame.brace_depth == 0 {
                // Literal-chunk mode.
                return self.next_fstring_chunk();
            }
            // Otherwise: we're inside `{...}`, fall through to normal lexing.
        }

        // Handle start-of-line indentation (only outside parens).
        if self.at_line_start && self.paren_depth == 0 && self.fstring_stack.is_empty() {
            if let Some(tok) = self.handle_line_start()? {
                return Ok(tok);
            }
        }

        loop {
            // Skip horizontal whitespace (not newlines).
            self.skip_inline_whitespace();

            // Comment? Consume to end-of-line, then continue loop so we hit
            // the newline branch below.
            if self.peek() == Some(b'#') {
                while let Some(c) = self.peek() {
                    if c == b'\n' { break; }
                    self.advance_byte();
                }
                continue;
            }

            // Line continuation: backslash + newline.
            if self.peek() == Some(b'\\') {
                if self.peek_at(1) == Some(b'\n') {
                    self.advance_byte(); // '\'
                    self.advance_newline();
                    continue;
                }
                if self.peek_at(1) == Some(b'\r') && self.peek_at(2) == Some(b'\n') {
                    self.advance_byte(); // '\'
                    self.advance_byte(); // '\r'
                    self.advance_newline();
                    continue;
                }
            }

            break;
        }

        // Newline (only meaningful when paren_depth==0).
        if matches!(self.peek(), Some(b'\n') | Some(b'\r')) {
            // Normalize CRLF.
            let start = self.pos;
            let line = self.line;
            let col = self.col;
            if self.peek() == Some(b'\r') && self.peek_at(1) == Some(b'\n') {
                self.advance_byte();
            }
            self.advance_newline();
            if self.paren_depth == 0 && !self.fstring_stack.is_empty() {
                // newlines inside f-string {...} are allowed (we're in an
                // expression here — fall through and re-enter the loop)
            }
            if self.paren_depth == 0 {
                self.at_line_start = true;
                let sp = Span { start: start as u32, end: self.pos as u32, line, col };
                return Ok(Token { kind: TokenKind::Newline, span: sp });
            } else {
                // Inside parens: newline is whitespace.
                return self.next_token();
            }
        }

        // End of input.
        if self.peek().is_none() {
            // Emit a final Newline if the last non-empty line didn't end in one.
            if !self.emitted_final_newline && self.paren_depth == 0 {
                self.emitted_final_newline = true;
                if !self.at_line_start {
                    self.at_line_start = true;
                    return Ok(Token { kind: TokenKind::Newline, span: self.point_span() });
                }
            }
            // Unwind indent stack.
            if self.indent_stack.len() > 1 {
                self.indent_stack.pop();
                return Ok(Token { kind: TokenKind::Dedent, span: self.point_span() });
            }
            self.done = true;
            return Ok(Token { kind: TokenKind::Eof, span: self.point_span() });
        }

        // Now lex one real token.
        self.lex_token()
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Cursor primitives
// ─────────────────────────────────────────────────────────────────────────

impl<'src> Lexer<'src> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
    fn peek_at(&self, k: usize) -> Option<u8> {
        self.bytes.get(self.pos + k).copied()
    }
    fn point_span(&self) -> Span {
        Span { start: self.pos as u32, end: self.pos as u32, line: self.line, col: self.col }
    }
    /// Advance one byte (caller must ensure it's not a newline byte).
    fn advance_byte(&mut self) {
        if let Some(_b) = self.peek() {
            self.pos += 1;
            self.col += 1;
        }
    }
    /// Consume a single LF and update line/col.
    fn advance_newline(&mut self) {
        debug_assert_eq!(self.peek(), Some(b'\n'));
        self.pos += 1;
        self.line += 1;
        self.col = 1;
    }
    /// Advance one full Unicode scalar value (for identifiers/strings; the
    /// spec restricts identifiers to ASCII but string contents can be any
    /// UTF-8). Returns the character and how many bytes were consumed.
    fn advance_char(&mut self) -> Option<char> {
        let s = &self.source[self.pos..];
        let mut chars = s.chars();
        let c = chars.next()?;
        let len = c.len_utf8();
        self.pos += len;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn skip_inline_whitespace(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' { self.advance_byte(); }
            else { break; }
        }
    }

    fn err(&self, msg: impl Into<String>) -> CompileError {
        CompileError::Parse {
            file: "<input>".to_string(),
            line: self.line,
            col: self.col,
            message: msg.into(),
        }
    }
    fn err_at(&self, line: u32, col: u32, msg: impl Into<String>) -> CompileError {
        CompileError::Parse {
            file: "<input>".to_string(),
            line,
            col,
            message: msg.into(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Indentation
// ─────────────────────────────────────────────────────────────────────────

impl<'src> Lexer<'src> {
    /// Called once per logical line at column 1, outside any paren depth.
    /// Returns Some(token) if an Indent/Dedent should be emitted now; None
    /// if the caller should continue with the normal lexing path.
    ///
    /// Handles blank lines and comment-only lines by silently consuming
    /// them (no NEWLINE emitted, indent state unchanged), per spec §3.3.
    fn handle_line_start(&mut self) -> Result<Option<Token>, CompileError> {
        // Loop so we skip past any run of blank / comment-only lines.
        let (spaces, line, col) = loop {
            let line = self.line;
            let col = self.col;
            let mut spaces: u32 = 0;
            loop {
                match self.peek() {
                    Some(b' ') => { spaces += 1; self.advance_byte(); }
                    Some(b'\t') => {
                        return Err(self.err_at(self.line, self.col,
                            "tab indentation is not permitted"));
                    }
                    _ => break,
                }
            }

            // Comment? Consume to end-of-line (but leave the newline so the
            // blank-line branch below handles it).
            if self.peek() == Some(b'#') {
                while let Some(c) = self.peek() {
                    if c == b'\n' { break; }
                    self.advance_byte();
                }
            }

            match self.peek() {
                Some(b'\n') => {
                    self.advance_newline();
                    continue; // re-scan next line's indentation
                }
                Some(b'\r') if self.peek_at(1) == Some(b'\n') => {
                    self.advance_byte();
                    self.advance_newline();
                    continue;
                }
                None => {
                    // EOF on a blank/comment line — leave at_line_start true so
                    // the EOF branch handles unwinding without emitting NL.
                    return Ok(None);
                }
                _ => break (spaces, line, col),
            }
        };

        self.at_line_start = false;
        let current = *self.indent_stack.last().unwrap();
        if spaces > current {
            if spaces - current != 4 {
                return Err(self.err_at(line, col,
                    "indentation must increase by exactly 4 spaces"));
            }
            self.indent_stack.push(spaces);
            let sp = Span { start: self.pos as u32, end: self.pos as u32, line, col };
            return Ok(Some(Token { kind: TokenKind::Indent, span: sp }));
        }
        if spaces < current {
            // Pop until we hit a level matching `spaces`.
            let mut popped = 0u32;
            while *self.indent_stack.last().unwrap() > spaces {
                self.indent_stack.pop();
                popped += 1;
            }
            if *self.indent_stack.last().unwrap() != spaces {
                return Err(self.err_at(line, col,
                    "dedent does not match any outer indentation level"));
            }
            // Emit one DEDENT now; queue the rest.
            self.pending_dedents = popped - 1;
            let sp = Span { start: self.pos as u32, end: self.pos as u32, line, col };
            return Ok(Some(Token { kind: TokenKind::Dedent, span: sp }));
        }
        Ok(None)
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Main token dispatch
// ─────────────────────────────────────────────────────────────────────────

impl<'src> Lexer<'src> {
    fn lex_token(&mut self) -> Result<Token, CompileError> {
        let start = self.pos;
        let line = self.line;
        let col = self.col;
        let b = self.peek().expect("caller ensured non-empty");

        // Identifier or keyword (ASCII; spec §3.6).
        if b == b'_' || b.is_ascii_alphabetic() {
            return self.lex_ident_or_keyword_or_prefixed_string(start, line, col);
        }

        // Numeric literal.
        if b.is_ascii_digit() {
            return self.lex_number(start, line, col);
        }
        // `.` followed by digit is a float (e.g. `.5`).
        if b == b'.' && self.peek_at(1).map(|c| c.is_ascii_digit()).unwrap_or(false) {
            return self.lex_number(start, line, col);
        }

        // String literal beginning with quote.
        if b == b'"' || b == b'\'' {
            return self.lex_string_or_char(start, line, col, /*prefix*/ None);
        }

        // Operator / punctuation.
        self.lex_op(start, line, col)
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Identifiers, keywords, and string-prefix dispatch
// ─────────────────────────────────────────────────────────────────────────

impl<'src> Lexer<'src> {
    fn lex_ident_or_keyword_or_prefixed_string(
        &mut self,
        start: usize,
        line: u32,
        col: u32,
    ) -> Result<Token, CompileError> {
        // First, peek: is this one of the string-prefix forms (r, b, f)?
        // The prefix must be immediately followed by a quote.
        let b = self.peek().unwrap();
        if matches!(b, b'r' | b'b' | b'f') {
            if let Some(q) = self.peek_at(1) {
                if q == b'"' || q == b'\'' {
                    let prefix = b as char;
                    self.advance_byte(); // consume prefix
                    return self.lex_string_or_char(start, line, col, Some(prefix));
                }
            }
        }

        // Normal identifier.
        while let Some(c) = self.peek() {
            if c == b'_' || c.is_ascii_alphanumeric() {
                self.advance_byte();
            } else {
                break;
            }
        }
        let text = &self.source[start..self.pos];
        let kind = keyword_kind(text).unwrap_or_else(|| TokenKind::Ident(text.to_string()));
        Ok(Token {
            kind,
            span: Span { start: start as u32, end: self.pos as u32, line, col },
        })
    }
}

fn keyword_kind(s: &str) -> Option<TokenKind> {
    Some(match s {
        "and" => TokenKind::KwAnd,
        "as" => TokenKind::KwAs,
        "assert" => TokenKind::KwAssert,
        "async" => TokenKind::KwAsync,
        "await" => TokenKind::KwAwait,
        "break" => TokenKind::KwBreak,
        "case" => TokenKind::KwCase,
        "class" => TokenKind::KwClass,
        "continue" => TokenKind::KwContinue,
        "def" => TokenKind::KwDef,
        "del" => TokenKind::KwDel,
        "elif" => TokenKind::KwElif,
        "else" => TokenKind::KwElse,
        "except" => TokenKind::KwExcept,
        "final" => TokenKind::KwFinal,
        "finally" => TokenKind::KwFinally,
        "fn" => TokenKind::KwFn,
        "for" => TokenKind::KwFor,
        "from" => TokenKind::KwFrom,
        "global" => TokenKind::KwGlobal,
        "if" => TokenKind::KwIf,
        "import" => TokenKind::KwImport,
        "in" => TokenKind::KwIn,
        "is" => TokenKind::KwIs,
        "lambda" => TokenKind::KwLambda,
        "match" => TokenKind::KwMatch,
        "not" => TokenKind::KwNot,
        "open" => TokenKind::KwOpen,
        "or" => TokenKind::KwOr,
        "pass" => TokenKind::KwPass,
        "protocol" => TokenKind::KwProtocol,
        "raise" => TokenKind::KwRaise,
        "return" => TokenKind::KwReturn,
        "sealed" => TokenKind::KwSealed,
        "try" => TokenKind::KwTry,
        "while" => TokenKind::KwWhile,
        "with" => TokenKind::KwWith,
        "yield" => TokenKind::KwYield,
        "true" => TokenKind::KwTrue,
        "false" => TokenKind::KwFalse,
        "none" => TokenKind::KwNone,
        "effect" => TokenKind::KwEffect,
        "region" => TokenKind::KwRegion,
        "unsafe" => TokenKind::KwUnsafe,
        _ => return None,
    })
}

// ─────────────────────────────────────────────────────────────────────────
//  Numeric literals
// ─────────────────────────────────────────────────────────────────────────

impl<'src> Lexer<'src> {
    fn lex_number(
        &mut self,
        start: usize,
        line: u32,
        col: u32,
    ) -> Result<Token, CompileError> {
        let first = self.peek().unwrap();

        // Hex / bin / oct integer.
        if first == b'0' {
            if let Some(p) = self.peek_at(1) {
                match p {
                    b'x' | b'X' => return self.lex_int_prefixed(start, line, col, 16, 2),
                    b'b' | b'B' => return self.lex_int_prefixed(start, line, col, 2, 2),
                    b'o' | b'O' => return self.lex_int_prefixed(start, line, col, 8, 2),
                    _ => {}
                }
            }
        }

        // Consume integer-part digits.
        if first == b'.' {
            // `.5`-style float: defer to float path.
        } else {
            self.consume_decimal_digits();
        }

        // Float?
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            // Look ahead: if next is a digit, it's a fractional float; if
            // it's an alphabetic/`_` char, could be method call on int.
            let next = self.peek_at(1);
            let is_attr = matches!(next, Some(c) if c == b'_' || c.is_ascii_alphabetic());
            if !is_attr {
                is_float = true;
                self.advance_byte(); // '.'
                self.consume_decimal_digits();
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            // Could be `0e10` or could be hex-like `0xeF`, but hex is handled above.
            is_float = true;
            self.advance_byte();
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.advance_byte();
            }
            if !matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                return Err(self.err("missing exponent digits in float literal"));
            }
            self.consume_decimal_digits();
        }

        if is_float {
            // Optional f32/f64 suffix.
            let suffix = self.try_consume_float_suffix();
            let body_end = self.pos - suffix_byte_len(&suffix);
            let raw: String = self.source[start..body_end].chars().filter(|c| *c != '_').collect();
            let value: f64 = raw.parse().map_err(|_| {
                self.err_at(line, col, format!("invalid float literal `{}`", &self.source[start..body_end]))
            })?;
            return Ok(Token {
                kind: TokenKind::FloatLit { value, suffix },
                span: Span { start: start as u32, end: self.pos as u32, line, col },
            });
        }

        // Integer (decimal). Optional integer suffix.
        let body_end = self.pos;
        let suffix = self.try_consume_int_suffix();
        let digits: String = self.source[start..body_end].chars().filter(|c| *c != '_').collect();
        let value: i128 = digits.parse().map_err(|_| {
            self.err_at(line, col, format!("integer literal `{}` out of range", &self.source[start..body_end]))
        })?;
        Ok(Token {
            kind: TokenKind::IntLit { value, suffix },
            span: Span { start: start as u32, end: self.pos as u32, line, col },
        })
    }

    fn lex_int_prefixed(
        &mut self,
        start: usize,
        line: u32,
        col: u32,
        radix: u32,
        prefix_len: usize,
    ) -> Result<Token, CompileError> {
        // Skip '0x' / '0b' / '0o'.
        for _ in 0..prefix_len { self.advance_byte(); }
        let digits_start = self.pos;
        while let Some(c) = self.peek() {
            let ok = match radix {
                16 => c.is_ascii_hexdigit(),
                8 => (b'0'..=b'7').contains(&c),
                2 => c == b'0' || c == b'1',
                _ => unreachable!(),
            } || c == b'_';
            if !ok { break; }
            self.advance_byte();
        }
        if self.pos == digits_start {
            return Err(self.err_at(line, col, "integer literal missing digits"));
        }
        let suffix = self.try_consume_int_suffix();
        let body_end = self.pos - suffix_byte_len_int(&suffix);
        let digits: String = self.source[digits_start..body_end].chars().filter(|c| *c != '_').collect();
        let value = i128::from_str_radix(&digits, radix).map_err(|_| {
            self.err_at(line, col, format!("integer literal `{}` out of range", &self.source[start..body_end]))
        })?;
        Ok(Token {
            kind: TokenKind::IntLit { value, suffix },
            span: Span { start: start as u32, end: self.pos as u32, line, col },
        })
    }

    fn consume_decimal_digits(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == b'_' {
                self.advance_byte();
            } else { break; }
        }
    }

    fn try_consume_int_suffix(&mut self) -> Option<IntSuffix> {
        let save = self.pos;
        let save_col = self.col;
        let s = self.source.get(self.pos..)?;
        let (suf, len) = if s.starts_with("i8") { (IntSuffix::I8, 2) }
            else if s.starts_with("i16") { (IntSuffix::I16, 3) }
            else if s.starts_with("i32") { (IntSuffix::I32, 3) }
            else if s.starts_with("i64") { (IntSuffix::I64, 3) }
            else if s.starts_with("u8") { (IntSuffix::U8, 2) }
            else if s.starts_with("u16") { (IntSuffix::U16, 3) }
            else if s.starts_with("u32") { (IntSuffix::U32, 3) }
            else if s.starts_with("u64") { (IntSuffix::U64, 3) }
            else { return None; };
        // Ensure the suffix isn't actually a longer identifier (`0i32x`).
        if let Some(after) = self.source.as_bytes().get(self.pos + len).copied() {
            if after == b'_' || after.is_ascii_alphanumeric() {
                self.pos = save;
                self.col = save_col;
                return None;
            }
        }
        self.pos += len;
        self.col += len as u32;
        Some(suf)
    }

    fn try_consume_float_suffix(&mut self) -> Option<FloatSuffix> {
        let s = self.source.get(self.pos..)?;
        let (suf, len) = if s.starts_with("f32") { (FloatSuffix::F32, 3) }
            else if s.starts_with("f64") { (FloatSuffix::F64, 3) }
            else { return None; };
        if let Some(after) = self.source.as_bytes().get(self.pos + len).copied() {
            if after == b'_' || after.is_ascii_alphanumeric() {
                return None;
            }
        }
        self.pos += len;
        self.col += len as u32;
        Some(suf)
    }
}

fn suffix_byte_len(s: &Option<FloatSuffix>) -> usize {
    match s { Some(_) => 3, None => 0 }
}
fn suffix_byte_len_int(s: &Option<IntSuffix>) -> usize {
    match s {
        None => 0,
        Some(IntSuffix::I8) | Some(IntSuffix::U8) => 2,
        Some(_) => 3,
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  String and char literals
// ─────────────────────────────────────────────────────────────────────────

impl<'src> Lexer<'src> {
    fn lex_string_or_char(
        &mut self,
        start: usize,
        line: u32,
        col: u32,
        prefix: Option<char>,
    ) -> Result<Token, CompileError> {
        let raw = prefix == Some('r');
        let bytes = prefix == Some('b');
        let fstr = prefix == Some('f');

        let quote = self.peek().unwrap();
        // Is this a triple-quoted string?
        let triple = quote == b'"'
            && self.peek_at(1) == Some(b'"')
            && self.peek_at(2) == Some(b'"');
        let triple_single = quote == b'\''
            && self.peek_at(1) == Some(b'\'')
            && self.peek_at(2) == Some(b'\'');

        // Char literal (single-quoted, not triple, no prefix).
        if quote == b'\'' && prefix.is_none() && !triple_single {
            return self.lex_char(start, line, col);
        }

        if fstr {
            return self.start_fstring(start, line, col, quote, triple);
        }

        // Consume opening quote(s).
        if triple || triple_single {
            self.advance_byte(); self.advance_byte(); self.advance_byte();
        } else {
            self.advance_byte();
        }

        // Body.
        let mut s_chars = String::new();
        let mut s_bytes: Vec<u8> = Vec::new();
        let actual_triple = triple || triple_single;

        loop {
            match self.peek() {
                None => return Err(self.err_at(line, col, "unterminated string literal")),
                Some(c) if !actual_triple && (c == b'\n') => {
                    return Err(self.err_at(line, col, "unterminated string literal"));
                }
                Some(c) if c == quote => {
                    if actual_triple {
                        if self.peek_at(1) == Some(quote) && self.peek_at(2) == Some(quote) {
                            self.advance_byte(); self.advance_byte(); self.advance_byte();
                            break;
                        } else {
                            self.advance_byte();
                            if bytes { s_bytes.push(c); } else { s_chars.push(c as char); }
                        }
                    } else {
                        self.advance_byte();
                        break;
                    }
                }
                Some(b'\\') if !raw => {
                    let esc_line = self.line;
                    let esc_col = self.col;
                    self.advance_byte();
                    let value = self.read_escape(esc_line, esc_col, bytes)?;
                    if bytes {
                        match value {
                            EscapeValue::Byte(b) => s_bytes.push(b),
                            EscapeValue::Char(c) => {
                                if (c as u32) > 0xFF {
                                    return Err(self.err_at(esc_line, esc_col,
                                        "byte string escape out of range (0..255)"));
                                }
                                s_bytes.push(c as u8);
                            }
                        }
                    } else {
                        match value {
                            EscapeValue::Byte(b) => s_chars.push(b as char),
                            EscapeValue::Char(c) => s_chars.push(c),
                        }
                    }
                }
                Some(_) => {
                    let c = self.advance_char().unwrap();
                    if bytes {
                        if (c as u32) > 0x7F {
                            return Err(self.err_at(line, col,
                                "byte string may only contain ASCII characters"));
                        }
                        s_bytes.push(c as u8);
                    } else {
                        s_chars.push(c);
                    }
                }
            }
        }

        let kind = if raw {
            TokenKind::RawStrLit(s_chars)
        } else if bytes {
            TokenKind::BytesLit(s_bytes)
        } else {
            TokenKind::StrLit(s_chars)
        };
        Ok(Token {
            kind,
            span: Span { start: start as u32, end: self.pos as u32, line, col },
        })
    }

    fn lex_char(
        &mut self,
        start: usize,
        line: u32,
        col: u32,
    ) -> Result<Token, CompileError> {
        // Consume opening '.
        self.advance_byte();
        let c = match self.peek() {
            None => return Err(self.err_at(line, col, "unterminated char literal")),
            Some(b'\'') => return Err(self.err_at(line, col, "empty char literal is illegal")),
            Some(b'\\') => {
                self.advance_byte();
                match self.read_escape(line, col, false)? {
                    EscapeValue::Byte(b) => b as char,
                    EscapeValue::Char(c) => c,
                }
            }
            Some(b'\n') => return Err(self.err_at(line, col, "unterminated char literal")),
            Some(_) => self.advance_char().unwrap(),
        };
        if self.peek() != Some(b'\'') {
            return Err(self.err_at(line, col,
                "char literal must contain exactly one character"));
        }
        self.advance_byte(); // closing '
        Ok(Token {
            kind: TokenKind::CharLit(c),
            span: Span { start: start as u32, end: self.pos as u32, line, col },
        })
    }

    /// Read the body of an escape sequence (the backslash has already been
    /// consumed). Spec §3.7.
    fn read_escape(
        &mut self,
        line: u32,
        col: u32,
        _in_bytes: bool,
    ) -> Result<EscapeValue, CompileError> {
        let b = match self.peek() {
            Some(b) => b,
            None => return Err(self.err_at(line, col, "unterminated escape sequence")),
        };
        Ok(match b {
            b'n' => { self.advance_byte(); EscapeValue::Char('\n') }
            b'r' => { self.advance_byte(); EscapeValue::Char('\r') }
            b't' => { self.advance_byte(); EscapeValue::Char('\t') }
            b'\\' => { self.advance_byte(); EscapeValue::Char('\\') }
            b'\'' => { self.advance_byte(); EscapeValue::Char('\'') }
            b'"' => { self.advance_byte(); EscapeValue::Char('"') }
            b'0' => { self.advance_byte(); EscapeValue::Char('\0') }
            b'x' => {
                self.advance_byte();
                let h1 = self.read_hex_digit(line, col)?;
                let h2 = self.read_hex_digit(line, col)?;
                EscapeValue::Byte((h1 * 16 + h2) as u8)
            }
            b'u' => {
                self.advance_byte();
                let mut v: u32 = 0;
                for _ in 0..4 {
                    v = v * 16 + self.read_hex_digit(line, col)?;
                }
                let c = char::from_u32(v).ok_or_else(|| {
                    self.err_at(line, col, format!("invalid Unicode escape \\u{:04X}", v))
                })?;
                EscapeValue::Char(c)
            }
            b'U' => {
                self.advance_byte();
                if self.peek() != Some(b'{') {
                    return Err(self.err_at(line, col, "expected `{` in \\U{...} escape"));
                }
                self.advance_byte();
                let mut v: u32 = 0;
                let mut count = 0;
                while let Some(c) = self.peek() {
                    if c == b'}' { break; }
                    v = v.checked_mul(16).and_then(|x| {
                        let d = match c {
                            b'0'..=b'9' => Some((c - b'0') as u32),
                            b'a'..=b'f' => Some((c - b'a' + 10) as u32),
                            b'A'..=b'F' => Some((c - b'A' + 10) as u32),
                            _ => None,
                        }?;
                        Some(x + d)
                    }).ok_or_else(|| self.err_at(line, col,
                        "invalid hex digit in \\U{...} escape"))?;
                    self.advance_byte();
                    count += 1;
                    if count > 8 {
                        return Err(self.err_at(line, col,
                            "\\U{...} escape has too many digits"));
                    }
                }
                if self.peek() != Some(b'}') {
                    return Err(self.err_at(line, col, "expected `}` in \\U{...} escape"));
                }
                self.advance_byte();
                let c = char::from_u32(v).ok_or_else(|| {
                    self.err_at(line, col, format!("invalid Unicode escape \\U{{{:X}}}", v))
                })?;
                EscapeValue::Char(c)
            }
            other => {
                self.advance_byte();
                return Err(self.err_at(line, col,
                    format!("unknown escape sequence `\\{}`", other as char)));
            }
        })
    }

    fn read_hex_digit(&mut self, line: u32, col: u32) -> Result<u32, CompileError> {
        let c = self.peek().ok_or_else(|| self.err_at(line, col, "unterminated hex escape"))?;
        let v = match c {
            b'0'..=b'9' => (c - b'0') as u32,
            b'a'..=b'f' => (c - b'a' + 10) as u32,
            b'A'..=b'F' => (c - b'A' + 10) as u32,
            _ => return Err(self.err_at(self.line, self.col, "invalid hex digit in escape")),
        };
        self.advance_byte();
        Ok(v)
    }
}

enum EscapeValue {
    Byte(u8),
    Char(char),
}

// ─────────────────────────────────────────────────────────────────────────
//  F-strings
// ─────────────────────────────────────────────────────────────────────────

impl<'src> Lexer<'src> {
    /// Called when we see `f"..."` or `f'...'`. Emits `FStrStart` and pushes
    /// an `FStringFrame`; subsequent calls to `next_token` will either lex
    /// the next literal chunk (yielding `FStrChunk`/`FStrExprStart`) or, if
    /// inside an interpolation, fall through to regular lexing until the
    /// matching `}`.
    fn start_fstring(
        &mut self,
        start: usize,
        line: u32,
        col: u32,
        quote: u8,
        triple: bool,
    ) -> Result<Token, CompileError> {
        if triple {
            self.advance_byte(); self.advance_byte(); self.advance_byte();
        } else {
            self.advance_byte();
        }
        self.fstring_stack.push(FStringFrame {
            triple,
            quote,
            brace_depth: 0,
        });
        Ok(Token {
            kind: TokenKind::FStrStart,
            span: Span { start: start as u32, end: self.pos as u32, line, col },
        })
    }

    /// Lex one chunk (literal-or-`{`-or-end) of the innermost f-string.
    fn next_fstring_chunk(&mut self) -> Result<Token, CompileError> {
        let frame = self.fstring_stack.last().cloned().unwrap();
        let start = self.pos;
        let line = self.line;
        let col = self.col;
        let mut buf = String::new();
        let quote = frame.quote;

        loop {
            match self.peek() {
                None => return Err(self.err_at(line, col, "unterminated f-string")),
                Some(c) if !frame.triple && c == b'\n' => {
                    return Err(self.err_at(line, col, "unterminated f-string"));
                }
                Some(c) if c == quote => {
                    if frame.triple {
                        if self.peek_at(1) == Some(quote) && self.peek_at(2) == Some(quote) {
                            // End of f-string.
                            if !buf.is_empty() {
                                // emit chunk first; queue FStrEnd
                                let chunk_end = self.pos;
                                let end_line = self.line;
                                let end_col = self.col;
                                self.advance_byte(); self.advance_byte(); self.advance_byte();
                                self.fstring_stack.pop();
                                self.queued.push_back(Token {
                                    kind: TokenKind::FStrEnd,
                                    span: Span { start: chunk_end as u32, end: self.pos as u32, line: end_line, col: end_col },
                                });
                                return Ok(Token {
                                    kind: TokenKind::FStrChunk(buf),
                                    span: Span { start: start as u32, end: chunk_end as u32, line, col },
                                });
                            }
                            let end_line = self.line;
                            let end_col = self.col;
                            self.advance_byte(); self.advance_byte(); self.advance_byte();
                            self.fstring_stack.pop();
                            return Ok(Token {
                                kind: TokenKind::FStrEnd,
                                span: Span { start: start as u32, end: self.pos as u32, line: end_line, col: end_col },
                            });
                        }
                        self.advance_byte();
                        buf.push(c as char);
                    } else {
                        if !buf.is_empty() {
                            let chunk_end = self.pos;
                            let end_line = self.line;
                            let end_col = self.col;
                            self.advance_byte();
                            self.fstring_stack.pop();
                            self.queued.push_back(Token {
                                kind: TokenKind::FStrEnd,
                                span: Span { start: chunk_end as u32, end: self.pos as u32, line: end_line, col: end_col },
                            });
                            return Ok(Token {
                                kind: TokenKind::FStrChunk(buf),
                                span: Span { start: start as u32, end: chunk_end as u32, line, col },
                            });
                        }
                        let end_line = self.line;
                        let end_col = self.col;
                        self.advance_byte();
                        self.fstring_stack.pop();
                        return Ok(Token {
                            kind: TokenKind::FStrEnd,
                            span: Span { start: start as u32, end: self.pos as u32, line: end_line, col: end_col },
                        });
                    }
                }
                Some(b'{') => {
                    if self.peek_at(1) == Some(b'{') {
                        // Escaped literal `{`.
                        self.advance_byte();
                        self.advance_byte();
                        buf.push('{');
                    } else {
                        // Open interpolation.
                        let brace_line = self.line;
                        let brace_col = self.col;
                        let brace_start = self.pos;
                        self.advance_byte();
                        // Mark current frame as in-expression by setting depth=1.
                        if let Some(f) = self.fstring_stack.last_mut() {
                            f.brace_depth = 1;
                        }
                        if !buf.is_empty() {
                            let chunk_end = brace_start;
                            self.queued.push_back(Token {
                                kind: TokenKind::FStrExprStart,
                                span: Span { start: brace_start as u32, end: self.pos as u32, line: brace_line, col: brace_col },
                            });
                            return Ok(Token {
                                kind: TokenKind::FStrChunk(buf),
                                span: Span { start: start as u32, end: chunk_end as u32, line, col },
                            });
                        }
                        return Ok(Token {
                            kind: TokenKind::FStrExprStart,
                            span: Span { start: brace_start as u32, end: self.pos as u32, line: brace_line, col: brace_col },
                        });
                    }
                }
                Some(b'}') => {
                    if self.peek_at(1) == Some(b'}') {
                        self.advance_byte();
                        self.advance_byte();
                        buf.push('}');
                    } else {
                        return Err(self.err_at(self.line, self.col,
                            "single `}` is not allowed in f-string literal text"));
                    }
                }
                Some(b'\\') => {
                    let esc_line = self.line;
                    let esc_col = self.col;
                    self.advance_byte();
                    let v = self.read_escape(esc_line, esc_col, false)?;
                    match v {
                        EscapeValue::Byte(b) => buf.push(b as char),
                        EscapeValue::Char(c) => buf.push(c),
                    }
                }
                Some(_) => {
                    let c = self.advance_char().unwrap();
                    buf.push(c);
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Operators / punctuation
// ─────────────────────────────────────────────────────────────────────────

impl<'src> Lexer<'src> {
    fn lex_op(&mut self, start: usize, line: u32, col: u32) -> Result<Token, CompileError> {
        // We're inside an f-string interpolation if the top frame has brace_depth>0.
        let in_fstr_expr = self.fstring_stack.last().map(|f| f.brace_depth > 0).unwrap_or(false);

        let b0 = self.peek().unwrap();
        let b1 = self.peek_at(1);
        let b2 = self.peek_at(2);

        // Try 3-char ops first (greedy match).
        let three: Option<TokenKind> = match (b0, b1, b2) {
            (b'*', Some(b'*'), Some(b'=')) => Some(TokenKind::DoubleStarEq),
            (b'/', Some(b'/'), Some(b'=')) => Some(TokenKind::DoubleSlashEq),
            (b'<', Some(b'<'), Some(b'=')) => Some(TokenKind::ShlEq),
            (b'>', Some(b'>'), Some(b'=')) => Some(TokenKind::ShrEq),
            _ => None,
        };
        if let Some(kind) = three {
            self.advance_byte(); self.advance_byte(); self.advance_byte();
            return Ok(Token { kind, span: Span { start: start as u32, end: self.pos as u32, line, col }});
        }

        // 2-char ops.
        let two: Option<TokenKind> = match (b0, b1) {
            (b'*', Some(b'*')) => Some(TokenKind::DoubleStar),
            (b'/', Some(b'/')) => Some(TokenKind::DoubleSlash),
            (b'<', Some(b'<')) => Some(TokenKind::Shl),
            (b'>', Some(b'>')) => Some(TokenKind::Shr),
            (b'=', Some(b'=')) => Some(TokenKind::EqEq),
            (b'!', Some(b'=')) => Some(TokenKind::NotEq),
            (b'<', Some(b'=')) => Some(TokenKind::LtEq),
            (b'>', Some(b'=')) => Some(TokenKind::GtEq),
            (b'+', Some(b'=')) => Some(TokenKind::PlusEq),
            (b'-', Some(b'=')) => Some(TokenKind::MinusEq),
            (b'*', Some(b'=')) => Some(TokenKind::StarEq),
            (b'/', Some(b'=')) => Some(TokenKind::SlashEq),
            (b'%', Some(b'=')) => Some(TokenKind::PercentEq),
            (b'&', Some(b'=')) => Some(TokenKind::AmpEq),
            (b'|', Some(b'=')) => Some(TokenKind::PipeEq),
            (b'^', Some(b'=')) => Some(TokenKind::CaretEq),
            (b'-', Some(b'>')) => Some(TokenKind::Arrow),
            (b'?', Some(b'?')) => Some(TokenKind::QuestionQuestion),
            _ => None,
        };
        if let Some(kind) = two {
            self.advance_byte(); self.advance_byte();
            return Ok(Token { kind, span: Span { start: start as u32, end: self.pos as u32, line, col }});
        }

        // 1-char ops.
        let one = match b0 {
            b'+' => TokenKind::Plus,
            b'-' => TokenKind::Minus,
            b'*' => TokenKind::Star,
            b'/' => TokenKind::Slash,
            b'%' => TokenKind::Percent,
            b'=' => TokenKind::Assign,
            b'<' => TokenKind::Lt,
            b'>' => TokenKind::Gt,
            b'&' => TokenKind::Amp,
            b'|' => TokenKind::Pipe,
            b'^' => TokenKind::Caret,
            b'~' => TokenKind::Tilde,
            b'(' => { self.paren_depth += 1; TokenKind::LParen }
            b')' => {
                if self.paren_depth == 0 {
                    return Err(self.err("unmatched `)`"));
                }
                self.paren_depth -= 1;
                TokenKind::RParen
            }
            b'[' => { self.paren_depth += 1; TokenKind::LBracket }
            b']' => {
                if self.paren_depth == 0 {
                    return Err(self.err("unmatched `]`"));
                }
                self.paren_depth -= 1;
                TokenKind::RBracket
            }
            b'{' => {
                if in_fstr_expr {
                    if let Some(f) = self.fstring_stack.last_mut() {
                        f.brace_depth += 1;
                    }
                }
                self.paren_depth += 1;
                TokenKind::LBrace
            }
            b'}' => {
                if in_fstr_expr {
                    // Decrement brace depth; if it returns to 1 (the outer
                    // marker), this `}` actually closes the interpolation.
                    let mut closed_interp = false;
                    if let Some(f) = self.fstring_stack.last_mut() {
                        if f.brace_depth == 1 {
                            f.brace_depth = 0;
                            closed_interp = true;
                        } else {
                            f.brace_depth -= 1;
                        }
                    }
                    if closed_interp {
                        // Don't consume as `RBrace`; emit FStrExprEnd instead.
                        self.advance_byte();
                        return Ok(Token {
                            kind: TokenKind::FStrExprEnd,
                            span: Span { start: start as u32, end: self.pos as u32, line, col },
                        });
                    }
                }
                if self.paren_depth == 0 {
                    return Err(self.err("unmatched `}`"));
                }
                self.paren_depth -= 1;
                TokenKind::RBrace
            }
            b',' => TokenKind::Comma,
            b':' => TokenKind::Colon,
            b';' => TokenKind::Semicolon,
            b'.' => TokenKind::Dot,
            b'@' => TokenKind::At,
            b'?' => TokenKind::Question,
            other => {
                return Err(self.err(format!("unexpected character `{}`", other as char)));
            }
        };
        self.advance_byte();
        Ok(Token { kind: one, span: Span { start: start as u32, end: self.pos as u32, line, col }})
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_all(src: &str) -> Vec<TokenKind> {
        let mut lex = Lexer::new(src);
        let mut out = Vec::new();
        loop {
            let t = lex.next_token().expect("lex error");
            let done = matches!(t.kind, TokenKind::Eof);
            out.push(t.kind);
            if done { break; }
        }
        out
    }

    fn lex_all_res(src: &str) -> Result<Vec<TokenKind>, CompileError> {
        let mut lex = Lexer::new(src);
        let mut out = Vec::new();
        loop {
            let t = lex.next_token()?;
            let done = matches!(t.kind, TokenKind::Eof);
            out.push(t.kind);
            if done { break; }
        }
        Ok(out)
    }

    #[test]
    fn test_keywords() {
        let src = "and as assert async await break case class continue def del elif \
                   else except final finally fn for from global if import in is \
                   lambda match not open or pass protocol raise return sealed try while \
                   with yield true false none effect region unsafe";
        let kinds = lex_all(src);
        let expected = [
            TokenKind::KwAnd, TokenKind::KwAs, TokenKind::KwAssert, TokenKind::KwAsync,
            TokenKind::KwAwait, TokenKind::KwBreak, TokenKind::KwCase, TokenKind::KwClass,
            TokenKind::KwContinue, TokenKind::KwDef, TokenKind::KwDel, TokenKind::KwElif,
            TokenKind::KwElse, TokenKind::KwExcept, TokenKind::KwFinal, TokenKind::KwFinally,
            TokenKind::KwFn, TokenKind::KwFor, TokenKind::KwFrom, TokenKind::KwGlobal,
            TokenKind::KwIf, TokenKind::KwImport, TokenKind::KwIn, TokenKind::KwIs,
            TokenKind::KwLambda, TokenKind::KwMatch, TokenKind::KwNot, TokenKind::KwOpen,
            TokenKind::KwOr, TokenKind::KwPass, TokenKind::KwProtocol, TokenKind::KwRaise,
            TokenKind::KwReturn, TokenKind::KwSealed, TokenKind::KwTry, TokenKind::KwWhile,
            TokenKind::KwWith, TokenKind::KwYield, TokenKind::KwTrue, TokenKind::KwFalse,
            TokenKind::KwNone, TokenKind::KwEffect, TokenKind::KwRegion, TokenKind::KwUnsafe,
        ];
        for (i, kw) in expected.iter().enumerate() {
            assert_eq!(&kinds[i], kw, "keyword #{i} mismatch");
        }
    }

    #[test]
    fn test_operators() {
        let src = "+ - * / // % ** = += -= *= /= //= %= **= == != < > <= >= \
                   & | ^ ~ << >> &= |= ^= <<= >>= ( ) [ ] { } , : ; . -> @ ? ??";
        let kinds = lex_all(src);
        let expected = [
            TokenKind::Plus, TokenKind::Minus, TokenKind::Star, TokenKind::Slash,
            TokenKind::DoubleSlash, TokenKind::Percent, TokenKind::DoubleStar,
            TokenKind::Assign, TokenKind::PlusEq, TokenKind::MinusEq, TokenKind::StarEq,
            TokenKind::SlashEq, TokenKind::DoubleSlashEq, TokenKind::PercentEq,
            TokenKind::DoubleStarEq, TokenKind::EqEq, TokenKind::NotEq, TokenKind::Lt,
            TokenKind::Gt, TokenKind::LtEq, TokenKind::GtEq, TokenKind::Amp, TokenKind::Pipe,
            TokenKind::Caret, TokenKind::Tilde, TokenKind::Shl, TokenKind::Shr,
            TokenKind::AmpEq, TokenKind::PipeEq, TokenKind::CaretEq, TokenKind::ShlEq,
            TokenKind::ShrEq, TokenKind::LParen, TokenKind::RParen, TokenKind::LBracket,
            TokenKind::RBracket, TokenKind::LBrace, TokenKind::RBrace, TokenKind::Comma,
            TokenKind::Colon, TokenKind::Semicolon, TokenKind::Dot, TokenKind::Arrow,
            TokenKind::At, TokenKind::Question, TokenKind::QuestionQuestion,
        ];
        for (i, op) in expected.iter().enumerate() {
            assert_eq!(&kinds[i], op, "operator #{i} mismatch");
        }
    }

    #[test]
    fn test_integer_literals() {
        let kinds = lex_all("0 42 0xff 0b1010 0o755 1_000_000 0i64 0u32");
        let lits: Vec<_> = kinds.iter().filter_map(|k| match k {
            TokenKind::IntLit { value, suffix } => Some((*value, *suffix)),
            _ => None,
        }).collect();
        assert_eq!(lits, vec![
            (0, None),
            (42, None),
            (0xff, None),
            (0b1010, None),
            (0o755, None),
            (1_000_000, None),
            (0, Some(IntSuffix::I64)),
            (0, Some(IntSuffix::U32)),
        ]);
    }

    #[test]
    fn test_float_literals() {
        let kinds = lex_all("3.14 .5 1e10 2.0f32");
        let lits: Vec<_> = kinds.iter().filter_map(|k| match k {
            TokenKind::FloatLit { value, suffix } => Some((*value, *suffix)),
            _ => None,
        }).collect();
        assert_eq!(lits.len(), 4);
        assert!((lits[0].0 - 3.14).abs() < 1e-9);
        assert!((lits[1].0 - 0.5).abs() < 1e-9);
        assert!((lits[2].0 - 1e10).abs() < 1.0);
        assert!((lits[3].0 - 2.0).abs() < 1e-9);
        assert_eq!(lits[3].1, Some(FloatSuffix::F32));
    }

    #[test]
    fn test_string_literals() {
        let kinds = lex_all("\"hello\" \"a\\nb\" r\"raw\\n\" b\"abc\" \"\"\"tri\nple\"\"\"");
        let strs: Vec<_> = kinds.iter().filter_map(|k| match k {
            TokenKind::StrLit(s) => Some(format!("S:{s}")),
            TokenKind::RawStrLit(s) => Some(format!("R:{s}")),
            TokenKind::BytesLit(b) => Some(format!("B:{}", String::from_utf8_lossy(b))),
            _ => None,
        }).collect();
        assert_eq!(strs[0], "S:hello");
        assert_eq!(strs[1], "S:a\nb");
        assert_eq!(strs[2], "R:raw\\n");
        assert_eq!(strs[3], "B:abc");
        assert_eq!(strs[4], "S:tri\nple");
    }

    #[test]
    fn test_char_literals() {
        let kinds = lex_all("'a' '\\n'");
        let chars: Vec<_> = kinds.iter().filter_map(|k| match k {
            TokenKind::CharLit(c) => Some(*c), _ => None }).collect();
        assert_eq!(chars, vec!['a', '\n']);

        assert!(lex_all_res("''").is_err());
        assert!(lex_all_res("'ab'").is_err());
    }

    #[test]
    fn test_fstring() {
        let kinds = lex_all("f\"x={x}\"");
        // Expect: FStrStart, FStrChunk("x="), FStrExprStart, Ident("x"),
        //         FStrExprEnd, FStrEnd, Newline, Eof
        let strip = |k: &TokenKind| -> String {
            match k {
                TokenKind::FStrStart => "FStrStart".into(),
                TokenKind::FStrChunk(s) => format!("FStrChunk({s})"),
                TokenKind::FStrExprStart => "FStrExprStart".into(),
                TokenKind::FStrExprEnd => "FStrExprEnd".into(),
                TokenKind::FStrEnd => "FStrEnd".into(),
                TokenKind::Ident(s) => format!("Ident({s})"),
                TokenKind::Newline => "Newline".into(),
                TokenKind::Eof => "Eof".into(),
                _ => format!("{:?}", k),
            }
        };
        let actual: Vec<String> = kinds.iter().map(strip).collect();
        assert_eq!(actual, vec![
            "FStrStart", "FStrChunk(x=)", "FStrExprStart", "Ident(x)",
            "FStrExprEnd", "FStrEnd", "Newline", "Eof",
        ]);
    }

    #[test]
    fn test_indent_dedent() {
        let src = "fn f():\n    a\n    b\nc\n";
        let kinds = lex_all(src);
        // fn f ( ) : NL INDENT a NL b NL DEDENT c NL EOF
        let strip = |k: &TokenKind| match k {
            TokenKind::KwFn => "fn".into(),
            TokenKind::Ident(s) => format!("id({s})"),
            TokenKind::LParen => "(".into(),
            TokenKind::RParen => ")".into(),
            TokenKind::Colon => ":".into(),
            TokenKind::Newline => "NL".into(),
            TokenKind::Indent => "INDENT".into(),
            TokenKind::Dedent => "DEDENT".into(),
            TokenKind::Eof => "EOF".into(),
            _ => format!("{:?}", k),
        };
        let s: Vec<String> = kinds.iter().map(strip).collect();
        assert_eq!(s, vec![
            "fn", "id(f)", "(", ")", ":", "NL",
            "INDENT", "id(a)", "NL", "id(b)", "NL",
            "DEDENT", "id(c)", "NL", "EOF",
        ]);

        // Bad indent: 2 spaces.
        let bad = "fn f():\n  a\n";
        assert!(lex_all_res(bad).is_err());

        // Tabs are an error.
        let bad2 = "fn f():\n\ta\n";
        assert!(lex_all_res(bad2).is_err());
    }

    #[test]
    fn test_paren_continuation() {
        let src = "[1,\n 2]\n";
        let kinds = lex_all(src);
        // No NL between the comma and the 2.
        assert!(!kinds.iter().take(4).any(|k| matches!(k, TokenKind::Newline)));
    }

    #[test]
    fn test_line_continuation() {
        let src = "a = 1 + \\\n  2\n";
        let kinds = lex_all(src);
        // Should be: id(a) = 1 + 2 NL EOF — no newline mid-expr.
        let newlines = kinds.iter().filter(|k| matches!(k, TokenKind::Newline)).count();
        assert_eq!(newlines, 1, "should have exactly one logical newline");
    }

    #[test]
    fn test_comments_ignored() {
        let src = "# foo\nx\n";
        let kinds = lex_all(src);
        // First token after `# foo\n` should be `x` (no token for comment).
        assert!(matches!(kinds[0], TokenKind::Ident(ref s) if s == "x"));
    }
}
