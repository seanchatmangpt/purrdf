use crate::error::{EyeronError, Result};
use purrdf_branchless::swar::byte_lane_in_range;

/// A lexical class of N3 source text.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// The `@forAll` quantifier keyword.
    AtForAll,
    /// The `@forSome` quantifier keyword.
    AtForSome,
    /// The `@prefix` directive spelled with a leading `@`.
    AtPrefix,
    /// The `@base` directive spelled with a leading `@`.
    AtBase,
    /// The `@version` directive spelled with a leading `@`.
    AtVersion,
    /// The SPARQL-style `PREFIX` directive keyword.
    Prefix,
    /// The SPARQL-style `BASE` directive keyword.
    Base,
    /// The SPARQL-style `VERSION` directive keyword.
    Version,
    /// A resolved `<...>` IRI reference carrying its text.
    Iri(String),
    /// A prefixed name such as `log:implies`, carrying its text.
    PName(String),
    /// A query-style variable such as `?x`, carrying its text.
    Var(String),
    /// A blank-node label such as `_:b1`, carrying its text.
    Blank(String),
    /// A double-quoted string literal, carrying its raw source text.
    String(String),
    /// A single-quoted string literal, carrying its raw source text.
    StringSingle(String),
    /// A long (`"""`) string literal, carrying its raw source text.
    StringLong(String),
    /// A long (`'''`) string literal, carrying its raw source text.
    StringLongSingle(String),
    /// A malformed `"""` string whose closing run has extra quotes, carrying its raw text.
    StringLongExtraQuote(String),
    /// A malformed `'''` string whose closing run has extra quotes, carrying its raw text.
    StringLongSingleExtraQuote(String),
    /// A language tag such as `@en`, carrying its text (without the `@`).
    Lang(String),
    /// A numeric literal, carrying its source text.
    Number(String),
    /// A boolean literal.
    Boolean(bool),
    /// The `a` keyword (shorthand for `rdf:type`).
    A,
    /// The statement-terminating `.`.
    Dot,
    /// The predicate-list `;` separator.
    Semicolon,
    /// The object-list `,` separator.
    Comma,
    /// The `{` opening a formula (quoted graph).
    LBrace,
    /// The `}` closing a formula (quoted graph).
    RBrace,
    /// The `{|` opening an RDF 1.2 triple-annotation block.
    LAnnotation,
    /// The `|}` closing an RDF 1.2 triple-annotation block.
    RAnnotation,
    /// The `[` opening a property list.
    LBracket,
    /// The `]` closing a property list.
    RBracket,
    /// The `(` opening a collection.
    LParen,
    /// The `)` closing a collection.
    RParen,
    /// The `<<` opening a reifier triple term.
    LTriple,
    /// The `>>` closing a reifier triple term.
    RTriple,
    /// The forward implication arrow `=>`.
    Arrow,
    /// The backward implication arrow `<=`.
    BackArrow,
    /// The `~` reverse-predicate marker.
    Reverse,
    /// The `^^` datatype marker.
    HatHat,
    /// The `=` equality/logical symbol.
    Equals,
    /// End of input.
    Eof,
}

/// A lexed token: its [`TokenKind`] and the byte offset where it starts in the input.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// The lexical class and payload of this token.
    pub kind: TokenKind,
    /// Byte offset of the token's first character in the source input.
    pub offset: usize,
}

/// Tokenizes N3 source text into a token stream, or reports a lexical error.
pub fn lex(input: &str) -> Result<Vec<Token>> {
    let mut lx = Lexer {
        input,
        pos: 0,
        tokens: Vec::new(),
    };
    lx.run()?;
    Ok(lx.tokens)
}

struct Lexer<'a> {
    input: &'a str,
    pos: usize,
    tokens: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn run(&mut self) -> Result<()> {
        loop {
            self.skip_ws_and_comments();
            let offset = self.pos;
            let Some(ch) = self.peek() else {
                self.tokens.push(Token {
                    kind: TokenKind::Eof,
                    offset,
                });
                return Ok(());
            };

            match ch {
                '<' if self.starts_with("<<") => {
                    self.bump();
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::LTriple,
                        offset,
                    });
                }
                '<' if self.starts_with("<=") => {
                    self.bump();
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::BackArrow,
                        offset,
                    });
                }
                '<' if self.starts_with("<-") => {
                    self.bump();
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::Reverse,
                        offset,
                    });
                }
                '<' => self.read_iri()?,
                '=' if self.starts_with("=>") => {
                    self.bump();
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::Arrow,
                        offset,
                    });
                }
                '=' => {
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::Equals,
                        offset,
                    });
                }
                '>' if self.starts_with(">>") => {
                    self.bump();
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::RTriple,
                        offset,
                    });
                }
                '^' if self.starts_with("^^") => {
                    self.bump();
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::HatHat,
                        offset,
                    });
                }
                '"' | '\'' => self.read_string()?,
                '.' if self.peek_next().is_some_and(|c| c.is_ascii_digit()) => {
                    self.read_number()?
                }
                '.' => {
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::Dot,
                        offset,
                    });
                }
                ';' => {
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::Semicolon,
                        offset,
                    });
                }
                ',' => {
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::Comma,
                        offset,
                    });
                }
                '{' if self.starts_with("{|") => {
                    self.bump();
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::LAnnotation,
                        offset,
                    });
                }
                '|' if self.starts_with("|}") => {
                    self.bump();
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::RAnnotation,
                        offset,
                    });
                }
                '{' => {
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::LBrace,
                        offset,
                    });
                }
                '}' => {
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::RBrace,
                        offset,
                    });
                }
                '[' => {
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::LBracket,
                        offset,
                    });
                }
                ']' => {
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::RBracket,
                        offset,
                    });
                }
                '(' => {
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::LParen,
                        offset,
                    });
                }
                ')' => {
                    self.bump();
                    self.tokens.push(Token {
                        kind: TokenKind::RParen,
                        offset,
                    });
                }
                _ if ch.is_ascii_digit()
                    || ((ch == '-' || ch == '+')
                        && self.peek_next().is_some_and(|c| {
                            c.is_ascii_digit()
                                || (c == '.'
                                    && self.peek_third().is_some_and(|d| d.is_ascii_digit()))
                        })) =>
                {
                    self.read_number()?
                }
                _ => self.read_word()?,
            }
        }
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            while self.peek().is_some_and(|c| c.is_whitespace()) {
                self.bump();
            }
            if self.peek() == Some('#') {
                while let Some(c) = self.peek() {
                    self.bump();
                    if c == '\n' {
                        break;
                    }
                }
                continue;
            }
            break;
        }
    }

    fn read_iri(&mut self) -> Result<()> {
        let offset = self.pos;
        self.bump();
        let mut value = String::new();
        while let Some(ch) = self.peek() {
            self.bump();
            if ch == '>' {
                self.tokens.push(Token {
                    kind: TokenKind::Iri(value),
                    offset,
                });
                return Ok(());
            }
            if ch == '\\' {
                let Some(esc) = self.peek() else {
                    return Err(EyeronError::at("unterminated IRI escape", offset));
                };
                self.bump();
                if esc == 'u' || esc == 'U' {
                    let count = if esc == 'u' { 4 } else { 8 };
                    let mut hex = String::new();
                    for _ in 0..count {
                        let Some(h) = self.peek() else {
                            return Err(EyeronError::at("unterminated Unicode escape", offset));
                        };
                        self.bump();
                        hex.push(h);
                    }
                    let code = u32::from_str_radix(&hex, 16)
                        .map_err(|_| EyeronError::at("invalid Unicode escape", offset))?;
                    let Some(c) = char::from_u32(code) else {
                        return Err(EyeronError::at("invalid Unicode scalar value", offset));
                    };
                    if is_forbidden_iri_char(c) {
                        return Err(EyeronError::at(
                            "forbidden character in IRI reference",
                            offset,
                        ));
                    }
                    value.push(c);
                } else {
                    return Err(EyeronError::at(
                        format!("invalid IRI escape \\{}", esc),
                        offset,
                    ));
                }
            } else {
                if is_forbidden_iri_char(ch) {
                    return Err(EyeronError::at(
                        "forbidden character in IRI reference",
                        offset,
                    ));
                }
                value.push(ch);
            }
        }
        Err(EyeronError::at("unterminated IRI reference", offset))
    }

    fn read_string(&mut self) -> Result<()> {
        let offset = self.pos;
        let quote = self.bump().unwrap();
        let triple = self.starts_with(&format!("{}{}", quote, quote));
        if triple {
            self.bump();
            self.bump();
        }
        let mut value = String::new();
        loop {
            let Some(ch) = self.peek() else {
                return Err(EyeronError::at("unterminated string literal", offset));
            };
            self.bump();
            if ch == quote {
                if triple {
                    // In a long string, a run of N quote characters ends the
                    // string with the final three quotes and contributes the
                    // preceding N-3 quotes to the value.  This handles the N3
                    // quote-edge cases in notation3tests without leaving a
                    // stray short-string token behind.
                    let mut run = 1usize;
                    while self.peek() == Some(quote) {
                        self.bump();
                        run += 1;
                    }
                    if run >= 3 {
                        for _ in 0..(run - 3) {
                            value.push(quote);
                        }
                        let kind = if run > 3 {
                            if quote == '"' {
                                TokenKind::StringLongExtraQuote(value)
                            } else {
                                TokenKind::StringLongSingleExtraQuote(value)
                            }
                        } else if quote == '"' {
                            TokenKind::StringLong(value)
                        } else {
                            TokenKind::StringLongSingle(value)
                        };
                        self.tokens.push(Token { kind, offset });
                        return Ok(());
                    }
                    for _ in 0..run {
                        value.push(quote);
                    }
                } else {
                    let kind = if quote == '"' {
                        TokenKind::String(value)
                    } else {
                        TokenKind::StringSingle(value)
                    };
                    self.tokens.push(Token { kind, offset });
                    return Ok(());
                }
            } else if !triple && matches!(ch, '\n' | '\r') {
                return Err(EyeronError::at("newline in short string literal", offset));
            } else if ch == '\\' {
                let Some(esc) = self.peek() else {
                    return Err(EyeronError::at("unterminated string escape", offset));
                };
                self.bump();
                match esc {
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    'b' => value.push('\u{0008}'),
                    'f' => value.push('\u{000C}'),
                    '"' => value.push('"'),
                    '\'' => value.push('\''),
                    '\\' => value.push('\\'),
                    'u' | 'U' => {
                        let count = if esc == 'u' { 4 } else { 8 };
                        let mut hex = String::new();
                        for _ in 0..count {
                            let Some(h) = self.peek() else {
                                return Err(EyeronError::at("unterminated Unicode escape", offset));
                            };
                            self.bump();
                            hex.push(h);
                        }
                        let code = u32::from_str_radix(&hex, 16)
                            .map_err(|_| EyeronError::at("invalid Unicode escape", offset))?;
                        let Some(c) = char::from_u32(code) else {
                            return Err(EyeronError::at("invalid Unicode scalar value", offset));
                        };
                        if is_forbidden_string_char(c) {
                            return Err(EyeronError::at(
                                "forbidden character in string literal",
                                offset,
                            ));
                        }
                        value.push(c);
                    }
                    other => {
                        return Err(EyeronError::at(
                            format!("invalid string escape \\{}", other),
                            offset,
                        ));
                    }
                }
            } else {
                value.push(ch);
            }
        }
    }

    fn read_number(&mut self) -> Result<()> {
        let offset = self.pos;
        let mut value = String::new();
        if matches!(self.peek(), Some('-' | '+')) {
            value.push(self.bump().unwrap());
        }
        // OPT-46/OPT-51: the three per-character digit loops are replaced by
        // one SWAR run scan (whole-word fast path, scalar tail); the token
        // text and every refusal site below are byte-identical because the
        // same characters are consumed in the same order.
        self.take_digit_run(&mut value);
        if self.peek() == Some('.') {
            // Accept .5 and 4.e2, but do not consume the statement dot in
            // ordinary terms such as `55.`.
            let after_dot_can_belong_to_number = self
                .peek_next()
                .is_some_and(|c| c.is_ascii_digit() || matches!(c, 'e' | 'E'));
            let leading_dot_number = value.is_empty() || value == "-" || value == "+";
            if after_dot_can_belong_to_number || leading_dot_number {
                value.push(self.bump().unwrap());
                self.take_digit_run(&mut value);
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            value.push(self.bump().unwrap());
            if matches!(self.peek(), Some('+' | '-')) {
                value.push(self.bump().unwrap());
            }
            let exponent_digits = self.take_digit_run(&mut value);
            if exponent_digits == 0 {
                // The RDF 1.2 validators already encode this grammar
                // (`is_double_lexical`); the terminal check here keeps
                // `1e` / `1e+` from minting a datatype-invalid
                // `"1e"^^xsd:double` on the N3 route (audit wave-2 F9).
                return Err(EyeronError::at(
                    "malformed numeric literal: exponent needs at least one digit",
                    offset,
                ));
            }
        }
        if self.peek() == Some('.') && self.peek_next().is_some_and(|c| c.is_ascii_digit()) {
            return Err(EyeronError::at("malformed numeric literal", offset));
        }
        self.tokens.push(Token {
            kind: TokenKind::Number(value),
            offset,
        });
        Ok(())
    }

    /// Consumes the maximal run of ASCII digits under the cursor, appending
    /// it to `value` with a single `push_str` per run, and returns the number
    /// of digits taken (0 when the cursor is not on a digit).
    ///
    /// Whole 8-byte words are classified lane-at-a-time with
    /// `purrdf_branchless::swar::byte_lane_in_range`; a full-digit word is
    /// consumed whole, and the first partial word contributes its leading
    /// digit lanes via `trailing_zeros` on the lane mask. Runs shorter than
    /// 8, and any tail straddling the end of input, fall through the same
    /// scalar loop the previous per-character body used — token text, cursor
    /// position and every downstream refusal are byte-identical.
    // from bcinr swar_str.rs:44-73 (`byte_lane_in_range`) via
    // purrdf_branchless::swar; wiring per OPT-46/OPT-51.
    fn take_digit_run(&mut self, value: &mut String) -> usize {
        const FULL_WORD_DIGITS: u64 = 0x8080_8080_8080_8080;
        let start = self.pos;
        let bytes = self.input.as_bytes();
        let mut end = start;
        while end + 8 <= bytes.len() {
            let mut word_bytes = [0u8; 8];
            word_bytes.copy_from_slice(&bytes[end..end + 8]);
            let mask = byte_lane_in_range(u64::from_le_bytes(word_bytes), b'0', b'9');
            if mask != FULL_WORD_DIGITS {
                // `trailing_zeros` is 64 on an all-zero mask; `& 63` maps
                // that to a zero-length run (e.g. `4.e2`, where the fraction
                // scan starts on 'e' with no digit lanes set).
                end += ((mask.trailing_zeros() & 63) >> 3) as usize;
                self.pos = end;
                value.push_str(&self.input[start..end]);
                return end - start;
            }
            end += 8;
        }
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        self.pos = end;
        value.push_str(&self.input[start..end]);
        end - start
    }

    fn read_word(&mut self) -> Result<()> {
        let offset = self.pos;
        let mut word = String::new();
        while let Some(ch) = self.peek() {
            if ch == '\\' {
                word.push(ch);
                self.bump();
                if let Some(next) = self.peek() {
                    word.push(next);
                    self.bump();
                }
                continue;
            }
            if ch.is_whitespace()
                || matches!(
                    ch,
                    '{' | '}' | '[' | ']' | '(' | ')' | ',' | ';' | '|' | '"' | '\''
                )
            {
                break;
            }
            if ch == '#' {
                break;
            }
            if word.starts_with("_:") && word.len() > 2 && ch == ':' {
                break;
            }
            if ch == '.' {
                match self.peek_next() {
                    None => break,
                    Some(next)
                        if next.is_whitespace()
                            || matches!(
                                next,
                                '{' | '}'
                                    | '['
                                    | ']'
                                    | '('
                                    | ')'
                                    | ','
                                    | ';'
                                    | '|'
                                    | '"'
                                    | '\''
                                    | '<'
                                    | '>'
                                    | '='
                                    | '#'
                            ) =>
                    {
                        // A dot followed by a word-break character closes the
                        // word BEFORE the dot: none of these are PN_CHARS, and
                        // `"`, `'`, `<`, `>`, `=`, `#` are not PN_LOCAL_ESC-
                        // escapable either, so swallowing them after a dot let
                        // string/IRI residue reach the minted IRI (B6).
                        break;
                    }
                    _ => {}
                }
            }
            if ch == '<' || ch == '>' || ch == '=' {
                break;
            }
            word.push(ch);
            self.bump();
        }
        if word.is_empty() {
            return Err(EyeronError::at(
                format!("unexpected character {:?}", self.peek()),
                offset,
            ));
        }
        let lower = word.to_ascii_lowercase();
        // Directives match by exact token only (audit C11/F7): the eager
        // `starts_with("@prefix")` split used to tokenize `@prefixfr:` into a
        // complete AtPrefix + PName("fr:") declaration and silently admit
        // invalid input.  A `@prefix*` word that is not exactly `@prefix` now
        // falls through to the Lang arm below.
        let kind = match lower.as_str() {
            _ if word == "@forAll" => TokenKind::AtForAll,
            _ if word == "@forSome" => TokenKind::AtForSome,
            _ if word == "@prefix" => TokenKind::AtPrefix,
            _ if word == "@base" => TokenKind::AtBase,
            _ if word == "@version" => TokenKind::AtVersion,
            "prefix" => TokenKind::Prefix,
            "base" => TokenKind::Base,
            "version" => TokenKind::Version,
            _ if word == "a" => TokenKind::A,
            "true" => TokenKind::Boolean(true),
            "false" => TokenKind::Boolean(false),
            _ if word.starts_with('@') => TokenKind::Lang(word[1..].to_string()),
            _ if word.starts_with('?') => TokenKind::Var(word[1..].to_string()),
            _ if word.starts_with("_:") => TokenKind::Blank(word[2..].to_string()),
            _ if word.contains(':') => TokenKind::PName(word),
            _ => TokenKind::PName(word),
        };
        self.tokens.push(Token { kind, offset });
        Ok(())
    }

    fn starts_with(&self, s: &str) -> bool {
        self.input[self.pos..].starts_with(s)
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn peek_next(&self) -> Option<char> {
        let mut it = self.input[self.pos..].chars();
        it.next()?;
        it.next()
    }

    fn peek_third(&self) -> Option<char> {
        let mut it = self.input[self.pos..].chars();
        it.next()?;
        it.next()?;
        it.next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }
}

fn is_forbidden_iri_char(ch: char) -> bool {
    ch.is_control()
        || ch.is_whitespace()
        || matches!(ch, '<' | '>' | '"' | '{' | '}' | '|' | '^' | '`' | '\\')
}

fn is_forbidden_string_char(_ch: char) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scalar oracle: the pre-SWAR per-character body of `read_number`'s
    /// digit loops, kept verbatim as the differential reference.
    fn scalar_run(input: &str, start: usize) -> (usize, String) {
        let mut pos = start;
        let mut value = String::new();
        while let Some(ch) = input[pos..].chars().next() {
            if !ch.is_ascii_digit() {
                break;
            }
            value.push(ch);
            pos += ch.len_utf8();
        }
        (pos - start, value)
    }

    fn swar_run(input: &str, start: usize) -> (usize, String) {
        let mut lx = Lexer {
            input,
            pos: start,
            tokens: Vec::new(),
        };
        let mut value = String::new();
        let n = lx.take_digit_run(&mut value);
        assert_eq!(lx.pos, start + n, "cursor must land on the run end");
        (n, value)
    }

    #[test]
    fn digit_run_differential_matches_scalar_oracle() {
        // Adversarial corpus: run lengths crossing the 8-byte word boundary
        // (7/8/9/16/17), multi-run documents, non-digit terminators, sign
        // and dot contexts, and multibyte neighbours.
        let corpus = [
            "",
            "0",
            "9",
            "1234567",
            "12345678",
            "123456789",
            "1234567890123456",
            "12345678901234567",
            "123456789012345678901234567890123456789",
            "007x",
            "12.34",
            ".5",
            "4.e2",
            "-42",
            "+42",
            "1e2",
            "1E-3",
            "1e+2",
            "55.",
            "12345678.",
            "12345678e",
            "9999999999999999999999999999999zzz",
            "1\u{00e9}2",
            "\u{00e9}123456789",
            "1234567\u{00e9}89012345",
            "1 2 3",
            "42 43 44",
            "1.2.3",
        ];
        for input in corpus {
            for start in 0..=input.len() {
                if !input.is_char_boundary(start) {
                    continue;
                }
                assert_eq!(
                    swar_run(input, start),
                    scalar_run(input, start),
                    "input {input:?} start {start}"
                );
            }
        }
    }

    #[test]
    fn multi_run_document_lexes_word_straddling_numbers() {
        // Multi-run document where several numbers start at non-word-aligned
        // offsets; every lexeme must come through byte-identical.
        let doc = "12 34567 12345678 123456789 0.5 1.25e-300 -98765432109876543210 .\n";
        let numbers: Vec<String> = lex(doc)
            .expect("document must lex")
            .into_iter()
            .filter_map(|t| match t.kind {
                TokenKind::Number(v) => Some(v),
                _ => None,
            })
            .collect();
        assert_eq!(
            numbers,
            [
                "12",
                "34567",
                "12345678",
                "123456789",
                "0.5",
                "1.25e-300",
                "-98765432109876543210"
            ]
        );
    }

    #[test]
    fn malformed_exponent_refusal_court() {
        // Refusals (F9, lexer.rs `exponent needs at least one digit`):
        let refused = ["1e", "1e+", "12e", "12E-", "1e 2"];
        for src in refused {
            assert!(
                lex(src).is_err(),
                "`{src}` must still refuse: malformed exponent"
            );
        }
        // Valid neighbours must still parse on the SWAR fast path:
        let neighbors = [
            ("1e2", "1e2"),
            ("1e+2", "1e+2"),
            ("1E-3", "1E-3"),
            (".5", ".5"),
            ("4.e2", "4.e2"),
            ("-0.0", "-0.0"),
            ("55.", "55"), // statement dot, not a fraction dot
        ];
        for (src, lexeme) in neighbors {
            let tokens = lex(src).unwrap_or_else(|e| panic!("`{src}` must lex, got {e:?}"));
            let nums: Vec<_> = tokens
                .iter()
                .filter_map(|t| match &t.kind {
                    TokenKind::Number(v) => Some(v.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(nums, [lexeme], "valid neighbour {src:?}");
        }
        // `55.` before a statement boundary keeps the Dot token:
        let tokens = lex("55. ").expect("`55. ` must lex");
        assert!(matches!(
            tokens.iter().map(|t| &t.kind).collect::<Vec<_>>()[..],
            [TokenKind::Number(_), TokenKind::Dot]
        ));
    }
}
