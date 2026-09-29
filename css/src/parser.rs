//! Parser CSS (spec CSS Syntax Level 3, §5) : assemble les tokens en
//! "component values" (blocs `{...}`, `[...]`, `(...)` et fonctions `rgb(...)`).

use std::borrow::Cow;
use std::iter::Peekable;

use crate::rules::{Cursor, Declaration, Item, consume_declaration};
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
    /// L'entrée n'est pas une règle / une déclaration valide.
    Invalid,
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

    /// "Parse a stylesheet" (§5.3.3) : les règles d'une feuille de style.
    pub fn parse_stylesheet(&mut self) -> Vec<Item<'a>> {
        Cursor::new(self.parse_component_value_list()).consume_rule_list(true)
    }

    /// "Parse a list of rules" (§5.3.4), par exemple le contenu d'un `@media`.
    pub fn parse_rule_list(&mut self) -> Vec<Item<'a>> {
        Cursor::new(self.parse_component_value_list()).consume_rule_list(false)
    }

    /// "Parse a list of declarations" (§5.3.8), par exemple un attribut `style`.
    pub fn parse_declaration_list(&mut self) -> Vec<Item<'a>> {
        Cursor::new(self.parse_component_value_list()).consume_declaration_list()
    }

    /// "Parse a block's contents" : déclarations et règles imbriquées (CSS nesting).
    pub fn parse_block_contents(&mut self) -> Vec<Item<'a>> {
        Cursor::new(self.parse_component_value_list()).consume_block_contents()
    }

    /// "Parse a rule" (§5.3.5) : exactement une règle.
    pub fn parse_rule(&mut self) -> Result<Item<'a>, ParseError> {
        let mut cursor = Cursor::new(self.parse_component_value_list());
        cursor.skip_whitespace();
        if cursor.at_end() {
            return Err(ParseError::Empty);
        }
        let item = cursor.consume_one_rule().ok_or(ParseError::Invalid)?;
        cursor.skip_whitespace();
        if !cursor.at_end() {
            return Err(ParseError::ExtraInput);
        }
        Ok(item)
    }

    /// "Parse a declaration" (§5.3.7) : exactement une déclaration (la valeur va
    /// jusqu'à la fin de l'entrée).
    pub fn parse_declaration(&mut self) -> Result<Declaration<'a>, ParseError> {
        let values = self.parse_component_value_list();
        let first = values
            .iter()
            .position(|v| !matches!(v, ComponentValue::Token(Token::Whitespace)));
        let Some(first) = first else {
            return Err(ParseError::Empty);
        };
        consume_declaration(values[first..].to_vec()).ok_or(ParseError::Invalid)
    }
}
