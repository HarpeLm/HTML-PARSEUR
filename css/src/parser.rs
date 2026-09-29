//! Parser CSS (spec CSS Syntax Level 3, §5) : assemble les tokens en
//! "component values" (blocs `{...}`, `[...]`, `(...)` et fonctions `rgb(...)`).

use std::borrow::Cow;
use std::iter::Peekable;

use crate::tokenizer::{Token, Tokenizer};

/// Le type d'un bloc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// `{ ... }`
    Curly,
    /// `[ ... ]`
    Square,
    /// `( ... )`
    Paren,
}

/// Une "component value" (§5.2) : un token, un bloc ou une fonction.
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentValue<'a> {
    /// Un token isolé.
    Token(Token<'a>),
    /// Un bloc et son contenu.
    Block {
        /// `{}`, `[]` ou `()`.
        kind: BlockKind,
        /// Le contenu, sans les délimiteurs.
        contents: Vec<ComponentValue<'a>>,
    },
    /// Une fonction : `rgb(1, 2, 3)`.
    Function {
        /// Le nom, sans la parenthèse.
        name: Cow<'a, str>,
        /// Les arguments, virgules et espaces compris.
        arguments: Vec<ComponentValue<'a>>,
    },
}

/// Erreur d'un "parse a component value" (§5.3.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// Rien à lire (que des espaces ou des commentaires).
    Empty,
    /// Quelque chose après la valeur attendue.
    ExtraInput,
}

/// Le parser : une source de tokens avec un token d'avance.
pub struct Parser<'a> {
    tokens: Peekable<Tokenizer<'a>>,
}

impl<'a> Parser<'a> {
    /// Un parser sur une feuille de style déjà prétraitée
    /// (voir [`crate::preprocess`]).
    pub fn new(input: &'a str) -> Self {
        Parser {
            tokens: Tokenizer::new(input).peekable(),
        }
    }

    fn skip_whitespace(&mut self) {
        while self.tokens.next_if(|t| *t == Token::Whitespace).is_some() {}
    }

    /// "Consume a component value" (§5.4.7), à partir du premier token.
    fn consume_component_value(&mut self, first: Token<'a>) -> ComponentValue<'a> {
        match first {
            Token::OpenCurly => self.consume_block(BlockKind::Curly),
            Token::OpenSquare => self.consume_block(BlockKind::Square),
            Token::OpenParen => self.consume_block(BlockKind::Paren),
            Token::Function(name) => {
                let arguments = self.consume_until(Token::CloseParen);
                ComponentValue::Function { name, arguments }
            }
            token => ComponentValue::Token(token),
        }
    }

    /// "Consume a simple block" (§5.4.8).
    fn consume_block(&mut self, kind: BlockKind) -> ComponentValue<'a> {
        let end = match kind {
            BlockKind::Curly => Token::CloseCurly,
            BlockKind::Square => Token::CloseSquare,
            BlockKind::Paren => Token::CloseParen,
        };
        let contents = self.consume_until(end);
        ComponentValue::Block { kind, contents }
    }

    /// Lit des component values jusqu'au token `end` (consommé) ou la fin du
    /// fichier : un bloc non fermé se termine simplement là.
    fn consume_until(&mut self, end: Token<'a>) -> Vec<ComponentValue<'a>> {
        let mut values = Vec::new();
        while let Some(token) = self.tokens.next() {
            if token == end {
                break;
            }
            values.push(self.consume_component_value(token));
        }
        values
    }

    /// "Parse a list of component values" (§5.3.10).
    pub fn parse_component_value_list(&mut self) -> Vec<ComponentValue<'a>> {
        let mut values = Vec::new();
        while let Some(token) = self.tokens.next() {
            values.push(self.consume_component_value(token));
        }
        values
    }

    /// "Parse a component value" (§5.3.9) : exactement une valeur, espaces autour permis.
    pub fn parse_component_value(&mut self) -> Result<ComponentValue<'a>, ParseError> {
        self.skip_whitespace();
        let first = self.tokens.next().ok_or(ParseError::Empty)?;
        let value = self.consume_component_value(first);
        self.skip_whitespace();
        match self.tokens.peek() {
            None => Ok(value),
            Some(_) => Err(ParseError::ExtraInput),
        }
    }
}
