//! Tokenizer CSS (spec CSS Syntax Level 3, §4).
//!
//! Découpe une feuille de style en tokens : identifiants (`color`), nombres
//! (`12px`, `50%`), chaînes, `url(...)`, ponctuation... Comme pour le HTML, les
//! erreurs ne font jamais échouer : la spec dit toujours quoi produire.

use std::borrow::Cow;

/// Un nombre CSS, avec sa forme écrite d'origine (utile pour les tests et pour
/// resérialiser à l'identique).
#[derive(Debug, Clone, PartialEq)]
pub struct Numeric<'a> {
    /// Le texte d'origine (`+12`, `.5`, `1e3`...).
    pub repr: &'a str,
    /// La valeur.
    pub value: f64,
    /// Écrit sans `.` ni exposant (`12`, `-3`) : un "integer" au sens de la spec.
    pub is_integer: bool,
}

/// Les erreurs que la spec signale et que les tests attendent comme des tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    /// Fin du fichier au milieu d'une chaîne (`"abc`).
    EofInString,
    /// Fin du fichier au milieu d'une `url(`.
    EofInUrl,
}

/// Un token CSS (§4). Les textes sont empruntés à la feuille de style quand
/// c'est possible (`Cow::Borrowed`), copiés s'ils contiennent des échappements.
#[derive(Debug, Clone, PartialEq)]
pub enum Token<'a> {
    /// `color`, `-webkit-box`...
    Ident(Cow<'a, str>),
    /// `rgb(` : un nom de fonction, parenthèse comprise.
    Function(Cow<'a, str>),
    /// `@media`.
    AtKeyword(Cow<'a, str>),
    /// `#fff`, `#main`. `is_id` : la valeur est un identifiant valide.
    Hash {
        /// La valeur, sans le `#`.
        value: Cow<'a, str>,
        /// Vrai si la valeur est un identifiant (type "id" de la spec).
        is_id: bool,
    },
    /// `"texte"` ou `'texte'`.
    String(Cow<'a, str>),
    /// Chaîne coupée par un retour à la ligne.
    BadString,
    /// `url(image.png)` sans guillemets.
    Url(Cow<'a, str>),
    /// `url(` mal formée.
    BadUrl,
    /// Un caractère isolé (`>`, `+`, `.`...).
    Delim(char),
    /// `12`, `1.5`.
    Number(Numeric<'a>),
    /// `50%`.
    Percentage(Numeric<'a>),
    /// `12px`.
    Dimension {
        /// Le nombre.
        number: Numeric<'a>,
        /// L'unité (`px`, `em`...).
        unit: Cow<'a, str>,
    },
    /// `U+0-7F`, `U+4??`.
    UnicodeRange {
        /// Début de la plage.
        start: u32,
        /// Fin de la plage.
        end: u32,
    },
    /// `~=`
    IncludeMatch,
    /// `|=`
    DashMatch,
    /// `^=`
    PrefixMatch,
    /// `$=`
    SuffixMatch,
    /// `*=`
    SubstringMatch,
    /// `||`
    Column,
    /// Espaces, tabulations, retours à la ligne.
    Whitespace,
    /// `<!--`
    Cdo,
    /// `-->`
    Cdc,
    /// `:`
    Colon,
    /// `;`
    Semicolon,
    /// `,`
    Comma,
    /// `[`
    OpenSquare,
    /// `]`
    CloseSquare,
    /// `(`
    OpenParen,
    /// `)`
    CloseParen,
    /// `{`
    OpenCurly,
    /// `}`
    CloseCurly,
    /// Une erreur signalée par la spec (fin de fichier dans une chaîne...).
    Error(TokenError),
}

/// Prétraitement (§3.3) : CR, CRLF et FF deviennent LF ; NUL devient U+FFFD.
/// Pas de copie si la feuille de style n'en contient pas (le cas courant).
pub fn preprocess(input: &str) -> Cow<'_, str> {
    if !input.bytes().any(|b| matches!(b, b'\r' | b'\x0C' | b'\0')) {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\x0C' => out.push('\n'),
            '\0' => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}

fn is_ident_char(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit() || c == '-'
}

fn is_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n')
}

/// "Non-printable code point" (§4.2).
fn is_non_printable(c: char) -> bool {
    matches!(c, '\0'..='\x08' | '\x0B' | '\x0E'..='\x1F' | '\x7F')
}

/// "Two code points are a valid escape" (§4.3.8).
fn is_valid_escape(first: Option<char>, second: Option<char>) -> bool {
    first == Some('\\') && second != Some('\n')
}

/// "Three code points would start an ident sequence" (§4.3.9).
fn starts_ident(a: Option<char>, b: Option<char>, c: Option<char>) -> bool {
    match a {
        Some('-') => b.is_some_and(|b| is_ident_start(b) || b == '-') || is_valid_escape(b, c),
        Some('\\') => is_valid_escape(a, b),
        Some(a) => is_ident_start(a),
        None => false,
    }
}

/// "Three code points would start a number" (§4.3.10).
fn starts_number(a: Option<char>, b: Option<char>, c: Option<char>) -> bool {
    match a {
        Some('+' | '-') => {
            b.is_some_and(|b| b.is_ascii_digit())
                || (b == Some('.') && c.is_some_and(|c| c.is_ascii_digit()))
        }
        Some('.') => b.is_some_and(|b| b.is_ascii_digit()),
        Some(a) => a.is_ascii_digit(),
        None => false,
    }
}

/// Le tokenizer : un itérateur de [`Token`] sur une feuille de style déjà
/// prétraitée (voir [`preprocess`]).
pub struct Tokenizer<'a> {
    input: &'a str,
    pos: usize,
    /// Erreur à émettre juste après le token courant (fin de fichier dans une chaîne...).
    pending_error: Option<TokenError>,
}

impl<'a> Tokenizer<'a> {
    /// Un tokenizer sur `input`, qui doit déjà être prétraité avec [`preprocess`].
    pub fn new(input: &'a str) -> Self {
        Tokenizer {
            input,
            pos: 0,
            pending_error: None,
        }
    }

    fn peek_nth(&self, n: usize) -> Option<char> {
        self.input[self.pos..].chars().nth(n)
    }

    fn peek(&self) -> Option<char> {
        self.peek_nth(0)
    }

    fn consume(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    /// Recule d'un caractère (le "reconsume" de la spec).
    fn reconsume(&mut self, c: char) {
        self.pos -= c.len_utf8();
    }

    fn consume_comments(&mut self) {
        while self.input[self.pos..].starts_with("/*") {
            match self.input[self.pos + 2..].find("*/") {
                Some(end) => self.pos += 2 + end + 2,
                None => self.pos = self.input.len(),
            }
        }
    }

    /// "Consume an escaped code point" (§4.3.7), après le `\`.
    fn consume_escape(&mut self) -> char {
        match self.consume() {
            Some(c) if c.is_ascii_hexdigit() => {
                let mut value = c.to_digit(16).unwrap();
                for _ in 0..5 {
                    match self.peek() {
                        Some(d) if d.is_ascii_hexdigit() => {
                            value = value * 16 + d.to_digit(16).unwrap();
                            self.consume();
                        }
                        _ => break,
                    }
                }
                if self.peek().is_some_and(is_whitespace) {
                    self.consume();
                }
                match char::from_u32(value) {
                    Some(c) if value != 0 => c,
                    _ => '\u{FFFD}', // 0, surrogate ou au-delà de U+10FFFF
                }
            }
            Some(c) => c,
            None => '\u{FFFD}',
        }
    }

    /// "Consume an ident sequence" (§4.3.11). Empruntée si aucun échappement.
    fn consume_ident_sequence(&mut self) -> Cow<'a, str> {
        let start = self.pos;
        let mut owned: Option<String> = None;
        loop {
            match self.peek() {
                Some(c) if is_ident_char(c) => {
                    self.consume();
                    if let Some(s) = &mut owned {
                        s.push(c);
                    }
                }
                Some('\\') if is_valid_escape(Some('\\'), self.peek_nth(1)) => {
                    // Un échappement : on passe en String (copie du début déjà lu).
                    let mut s = owned
                        .take()
                        .unwrap_or_else(|| self.input[start..self.pos].to_string());
                    self.pos += 1; // le '\'
                    s.push(self.consume_escape());
                    owned = Some(s);
                }
                _ => break,
            }
        }
        match owned {
            Some(s) => Cow::Owned(s),
            None => Cow::Borrowed(&self.input[start..self.pos]),
        }
    }

    /// "Consume a number" (§4.3.12).
    fn consume_number(&mut self) -> Numeric<'a> {
        let start = self.pos;
        let mut is_integer = true;
        if matches!(self.peek(), Some('+' | '-')) {
            self.consume();
        }
        self.consume_digits();
        if self.peek() == Some('.') && self.peek_nth(1).is_some_and(|c| c.is_ascii_digit()) {
            self.consume();
            self.consume_digits();
            is_integer = false;
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            let after = self.peek_nth(1);
            let exponent = after.is_some_and(|c| c.is_ascii_digit())
                || (matches!(after, Some('+' | '-'))
                    && self.peek_nth(2).is_some_and(|c| c.is_ascii_digit()));
            if exponent {
                self.consume();
                if matches!(self.peek(), Some('+' | '-')) {
                    self.consume();
                }
                self.consume_digits();
                is_integer = false;
            }
        }
        let repr = &self.input[start..self.pos];
        let value = repr.parse::<f64>().unwrap_or(0.0);
        Numeric {
            repr,
            value,
            is_integer,
        }
    }

    fn consume_digits(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.consume();
        }
    }

    /// "Consume a numeric token" (§4.3.3).
    fn consume_numeric(&mut self) -> Token<'a> {
        let number = self.consume_number();
        if starts_ident(self.peek(), self.peek_nth(1), self.peek_nth(2)) {
            let unit = self.consume_ident_sequence();
            Token::Dimension { number, unit }
        } else if self.peek() == Some('%') {
            self.consume();
            Token::Percentage(number)
        } else {
            Token::Number(number)
        }
    }

    /// "Consume an ident-like token" (§4.3.4) : ident, fonction ou url.
    fn consume_ident_like(&mut self) -> Token<'a> {
        let name = self.consume_ident_sequence();
        if name.eq_ignore_ascii_case("url") && self.peek() == Some('(') {
            self.consume();
            // Au plus un espace laissé avant de regarder s'il y a des guillemets.
            while is_whitespace(self.peek().unwrap_or('x'))
                && is_whitespace(self.peek_nth(1).unwrap_or('x'))
            {
                self.consume();
            }
            let next = if self.peek().is_some_and(is_whitespace) {
                self.peek_nth(1)
            } else {
                self.peek()
            };
            if matches!(next, Some('"' | '\'')) {
                return Token::Function(name);
            }
            return self.consume_url();
        }
        if self.peek() == Some('(') {
            self.consume();
            return Token::Function(name);
        }
        Token::Ident(name)
    }

    /// "Consume a url token" (§4.3.6), après `url(`.
    fn consume_url(&mut self) -> Token<'a> {
        while self.peek().is_some_and(is_whitespace) {
            self.consume();
        }
        let mut value = String::new();
        loop {
            match self.consume() {
                Some(')') => return Token::Url(Cow::Owned(value)),
                None => {
                    self.pending_error = Some(TokenError::EofInUrl);
                    return Token::Url(Cow::Owned(value));
                }
                Some(c) if is_whitespace(c) => {
                    while self.peek().is_some_and(is_whitespace) {
                        self.consume();
                    }
                    match self.peek() {
                        Some(')') => {
                            self.consume();
                            return Token::Url(Cow::Owned(value));
                        }
                        None => {
                            self.pending_error = Some(TokenError::EofInUrl);
                            return Token::Url(Cow::Owned(value));
                        }
                        _ => {
                            self.consume_bad_url_remnants();
                            return Token::BadUrl;
                        }
                    }
                }
                Some('"' | '\'' | '(') => {
                    self.consume_bad_url_remnants();
                    return Token::BadUrl;
                }
                Some(c) if is_non_printable(c) => {
                    self.consume_bad_url_remnants();
                    return Token::BadUrl;
                }
                Some('\\') => {
                    if is_valid_escape(Some('\\'), self.peek()) {
                        value.push(self.consume_escape());
                    } else {
                        self.consume_bad_url_remnants();
                        return Token::BadUrl;
                    }
                }
                Some(c) => value.push(c),
            }
        }
    }

    /// "Consume the remnants of a bad url" (§4.3.14).
    fn consume_bad_url_remnants(&mut self) {
        loop {
            match self.consume() {
                Some(')') | None => return,
                Some('\\') if is_valid_escape(Some('\\'), self.peek()) => {
                    self.consume_escape();
                }
                _ => {}
            }
        }
    }

    /// "Consume a string token" (§4.3.5), après le guillemet ouvrant.
    fn consume_string(&mut self, ending: char) -> Token<'a> {
        let start = self.pos;
        let mut owned: Option<String> = None;
        loop {
            match self.consume() {
                Some(c) if c == ending => {
                    let value = match owned {
                        Some(s) => Cow::Owned(s),
                        None => Cow::Borrowed(&self.input[start..self.pos - c.len_utf8()]),
                    };
                    return Token::String(value);
                }
                None => {
                    self.pending_error = Some(TokenError::EofInString);
                    let value = match owned {
                        Some(s) => Cow::Owned(s),
                        None => Cow::Borrowed(&self.input[start..self.pos]),
                    };
                    return Token::String(value);
                }
                Some('\n') => {
                    self.reconsume('\n');
                    return Token::BadString;
                }
                Some('\\') => {
                    let s =
                        owned.get_or_insert_with(|| self.input[start..self.pos - 1].to_string());
                    match self.peek() {
                        None => {}
                        Some('\n') => {
                            self.consume(); // continuation de ligne
                        }
                        Some(_) => {
                            let c = self.consume_escape();
                            s.push(c);
                        }
                    }
                }
                Some(c) => {
                    if let Some(s) = &mut owned {
                        s.push(c);
                    }
                }
            }
        }
    }

    /// `U+...` (CSS Syntax 2014, §4.3.8 ; attendu par css-parsing-tests).
    fn consume_unicode_range(&mut self) -> Token<'a> {
        let mut hex = String::new();
        while hex.len() < 6 && self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
            hex.push(self.consume().unwrap());
        }
        let mut wildcards = 0;
        while hex.len() + wildcards < 6 && self.peek() == Some('?') {
            self.consume();
            wildcards += 1;
        }
        if wildcards > 0 {
            let start =
                u32::from_str_radix(&format!("{hex}{}", "0".repeat(wildcards)), 16).unwrap_or(0);
            let end =
                u32::from_str_radix(&format!("{hex}{}", "F".repeat(wildcards)), 16).unwrap_or(0);
            return Token::UnicodeRange { start, end };
        }
        let start = u32::from_str_radix(&hex, 16).unwrap_or(0);
        if self.peek() == Some('-') && self.peek_nth(1).is_some_and(|c| c.is_ascii_hexdigit()) {
            self.consume();
            let mut end_hex = String::new();
            while end_hex.len() < 6 && self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                end_hex.push(self.consume().unwrap());
            }
            let end = u32::from_str_radix(&end_hex, 16).unwrap_or(0);
            return Token::UnicodeRange { start, end };
        }
        Token::UnicodeRange { start, end: start }
    }

    /// "Consume a token" (§4.3.1).
    fn consume_token(&mut self) -> Option<Token<'a>> {
        if let Some(error) = self.pending_error.take() {
            return Some(Token::Error(error));
        }
        self.consume_comments();
        let c = self.consume()?;
        let token = match c {
            c if is_whitespace(c) => {
                while self.peek().is_some_and(is_whitespace) {
                    self.consume();
                }
                Token::Whitespace
            }
            '"' | '\'' => self.consume_string(c),
            '#' => {
                if self.peek().is_some_and(is_ident_char)
                    || is_valid_escape(self.peek(), self.peek_nth(1))
                {
                    let is_id = starts_ident(self.peek(), self.peek_nth(1), self.peek_nth(2));
                    let value = self.consume_ident_sequence();
                    Token::Hash { value, is_id }
                } else {
                    Token::Delim('#')
                }
            }
            '(' => Token::OpenParen,
            ')' => Token::CloseParen,
            '[' => Token::OpenSquare,
            ']' => Token::CloseSquare,
            '{' => Token::OpenCurly,
            '}' => Token::CloseCurly,
            ',' => Token::Comma,
            ':' => Token::Colon,
            ';' => Token::Semicolon,
            '+' | '.' => {
                if starts_number(Some(c), self.peek(), self.peek_nth(1)) {
                    self.reconsume(c);
                    self.consume_numeric()
                } else {
                    Token::Delim(c)
                }
            }
            '-' => {
                if starts_number(Some('-'), self.peek(), self.peek_nth(1)) {
                    self.reconsume(c);
                    self.consume_numeric()
                } else if self.peek() == Some('-') && self.peek_nth(1) == Some('>') {
                    self.pos += 2;
                    Token::Cdc
                } else if starts_ident(Some('-'), self.peek(), self.peek_nth(1)) {
                    self.reconsume(c);
                    self.consume_ident_like()
                } else {
                    Token::Delim('-')
                }
            }
            '<' => {
                if self.input[self.pos..].starts_with("!--") {
                    self.pos += 3;
                    Token::Cdo
                } else {
                    Token::Delim('<')
                }
            }
            '@' => {
                if starts_ident(self.peek(), self.peek_nth(1), self.peek_nth(2)) {
                    Token::AtKeyword(self.consume_ident_sequence())
                } else {
                    Token::Delim('@')
                }
            }
            '\\' => {
                if is_valid_escape(Some('\\'), self.peek()) {
                    self.reconsume(c);
                    self.consume_ident_like()
                } else {
                    Token::Delim('\\')
                }
            }
            '~' | '|' | '^' | '$' | '*' if self.peek() == Some('=') => {
                self.consume();
                match c {
                    '~' => Token::IncludeMatch,
                    '|' => Token::DashMatch,
                    '^' => Token::PrefixMatch,
                    '$' => Token::SuffixMatch,
                    _ => Token::SubstringMatch,
                }
            }
            '|' if self.peek() == Some('|') => {
                self.consume();
                Token::Column
            }
            'u' | 'U'
                if self.peek() == Some('+')
                    && self
                        .peek_nth(1)
                        .is_some_and(|c| c.is_ascii_hexdigit() || c == '?') =>
            {
                self.consume();
                self.consume_unicode_range()
            }
            c if c.is_ascii_digit() => {
                self.reconsume(c);
                self.consume_numeric()
            }
            c if is_ident_start(c) => {
                self.reconsume(c);
                self.consume_ident_like()
            }
            c => Token::Delim(c),
        };
        Some(token)
    }
}

impl<'a> Iterator for Tokenizer<'a> {
    type Item = Token<'a>;

    fn next(&mut self) -> Option<Token<'a>> {
        self.consume_token()
    }
}
