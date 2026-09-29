//! Règles et déclarations (spec CSS Syntax Level 3, §5.3 et §5.4).
//!
//! Ces algorithmes travaillent sur la liste des component values déjà
//! construite : le nouvel algorithme des blocs (CSS imbriqué) doit pouvoir
//! revenir en arrière ("essayer une déclaration, sinon relire comme une règle").

use std::borrow::Cow;

use crate::parser::{BlockKind, ComponentValue};
use crate::tokenizer::Token;

/// `propriété: valeur !important`.
#[derive(Debug, Clone, PartialEq)]
pub struct Declaration<'a> {
    /// Le nom de la propriété.
    pub name: Cow<'a, str>,
    /// La valeur, espaces compris, sans `!important`.
    pub value: Vec<ComponentValue<'a>>,
    /// La déclaration se terminait par `!important`.
    pub important: bool,
}

/// `@media screen { ... }`, `@import "x.css";`.
#[derive(Debug, Clone, PartialEq)]
pub struct AtRule<'a> {
    /// Le nom, sans le `@`.
    pub name: Cow<'a, str>,
    /// Ce qui se trouve entre le nom et le bloc (ou le `;`).
    pub prelude: Vec<ComponentValue<'a>>,
    /// Le contenu du bloc `{ ... }`, s'il y en a un.
    pub block: Option<Vec<ComponentValue<'a>>>,
}

/// `div > p { color: red }` : un sélecteur (le prélude) et un bloc.
#[derive(Debug, Clone, PartialEq)]
pub struct QualifiedRule<'a> {
    /// Le prélude (le sélecteur, pour une règle de style).
    pub prelude: Vec<ComponentValue<'a>>,
    /// Le contenu du bloc `{ ... }`.
    pub block: Vec<ComponentValue<'a>>,
}

/// Un élément d'une liste de règles ou de déclarations.
#[derive(Debug, Clone, PartialEq)]
pub enum Item<'a> {
    /// Une déclaration.
    Declaration(Declaration<'a>),
    /// Une at-rule.
    AtRule(AtRule<'a>),
    /// Une règle qualifiée.
    QualifiedRule(QualifiedRule<'a>),
    /// Un morceau invalide, ignoré (la spec le signale comme erreur de parsing).
    Invalid,
}

fn is_whitespace(v: &ComponentValue) -> bool {
    matches!(v, ComponentValue::Token(Token::Whitespace))
}

fn is_token(v: &ComponentValue, t: &Token) -> bool {
    matches!(v, ComponentValue::Token(x) if x == t)
}

fn is_curly_block(v: &ComponentValue) -> bool {
    matches!(
        v,
        ComponentValue::Block {
            kind: BlockKind::Curly,
            ..
        }
    )
}

/// Un curseur sur une liste de component values, qu'on peut faire reculer.
pub(crate) struct Cursor<'a> {
    values: Vec<ComponentValue<'a>>,
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(values: Vec<ComponentValue<'a>>) -> Self {
        Cursor { values, pos: 0 }
    }

    fn peek(&self) -> Option<&ComponentValue<'a>> {
        self.values.get(self.pos)
    }

    fn next(&mut self) -> Option<ComponentValue<'a>> {
        let v = self.values.get(self.pos)?.clone();
        self.pos += 1;
        Some(v)
    }

    pub(crate) fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(is_whitespace) {
            self.pos += 1;
        }
    }

    pub(crate) fn at_end(&self) -> bool {
        self.pos >= self.values.len()
    }

    /// Lit jusqu'au prochain `;` de premier niveau (consommé) ou la fin.
    fn take_until_semicolon(&mut self) -> Vec<ComponentValue<'a>> {
        let mut out = Vec::new();
        while let Some(v) = self.next() {
            if is_token(&v, &Token::Semicolon) {
                break;
            }
            out.push(v);
        }
        out
    }

    /// "Consume an at-rule" (§5.4.2), après avoir lu son nom.
    fn consume_at_rule(&mut self, name: Cow<'a, str>) -> AtRule<'a> {
        let mut prelude = Vec::new();
        while let Some(v) = self.next() {
            match v {
                ComponentValue::Token(Token::Semicolon) => break,
                ComponentValue::Block {
                    kind: BlockKind::Curly,
                    contents,
                } => {
                    return AtRule {
                        name,
                        prelude,
                        block: Some(contents),
                    };
                }
                v => prelude.push(v),
            }
        }
        AtRule {
            name,
            prelude,
            block: None,
        }
    }

    /// "Consume a qualified rule" (§5.4.3). `nested` : à l'intérieur d'un bloc,
    /// où un `;` met fin à la tentative.
    fn consume_qualified_rule(&mut self, nested: bool) -> Option<QualifiedRule<'a>> {
        let mut prelude = Vec::new();
        loop {
            match self.next()? {
                ComponentValue::Block {
                    kind: BlockKind::Curly,
                    contents,
                } => {
                    return Some(QualifiedRule {
                        prelude,
                        block: contents,
                    });
                }
                ComponentValue::Token(Token::Semicolon) if nested => return None,
                v => prelude.push(v),
            }
        }
    }

    /// "Consume a list of rules" (§5.4.1). `top_level` : feuille de style, où
    /// `<!--` et `-->` sont ignorés entre les règles.
    pub(crate) fn consume_rule_list(&mut self, top_level: bool) -> Vec<Item<'a>> {
        let mut items = Vec::new();
        loop {
            self.skip_whitespace();
            let Some(v) = self.peek() else { return items };
            match v {
                ComponentValue::Token(Token::Cdo | Token::Cdc) if top_level => {
                    self.pos += 1;
                }
                ComponentValue::Token(Token::AtKeyword(_)) => {
                    let Some(ComponentValue::Token(Token::AtKeyword(name))) = self.next() else {
                        unreachable!()
                    };
                    items.push(Item::AtRule(self.consume_at_rule(name)));
                }
                _ => items.push(match self.consume_qualified_rule(false) {
                    Some(rule) => Item::QualifiedRule(rule),
                    None => Item::Invalid,
                }),
            }
        }
    }

    /// Une at-rule ou une règle qualifiée (pour "parse a rule").
    pub(crate) fn consume_one_rule(&mut self) -> Option<Item<'a>> {
        if let Some(ComponentValue::Token(Token::AtKeyword(_))) = self.peek() {
            let Some(ComponentValue::Token(Token::AtKeyword(name))) = self.next() else {
                unreachable!()
            };
            return Some(Item::AtRule(self.consume_at_rule(name)));
        }
        self.consume_qualified_rule(false).map(Item::QualifiedRule)
    }

    /// "Consume a list of declarations" (§5.4.5, algorithme classique).
    pub(crate) fn consume_declaration_list(&mut self) -> Vec<Item<'a>> {
        let mut items = Vec::new();
        loop {
            while self
                .peek()
                .is_some_and(|v| is_whitespace(v) || is_token(v, &Token::Semicolon))
            {
                self.pos += 1;
            }
            let Some(v) = self.peek() else { return items };
            match v {
                ComponentValue::Token(Token::AtKeyword(_)) => {
                    let Some(ComponentValue::Token(Token::AtKeyword(name))) = self.next() else {
                        unreachable!()
                    };
                    items.push(Item::AtRule(self.consume_at_rule(name)));
                }
                ComponentValue::Token(Token::Ident(_)) => {
                    let values = self.take_until_semicolon();
                    items.push(match consume_declaration(values) {
                        Some(d) => Item::Declaration(d),
                        None => Item::Invalid,
                    });
                }
                _ => {
                    self.take_until_semicolon();
                    items.push(Item::Invalid);
                }
            }
        }
    }

    /// "Consume a block's contents" (algorithme du CSS imbriqué) : déclarations,
    /// at-rules et règles mélangées. On essaie une déclaration ; si ce n'en est
    /// pas une, on revient en arrière et on relit comme une règle.
    pub(crate) fn consume_block_contents(&mut self) -> Vec<Item<'a>> {
        let mut items = Vec::new();
        loop {
            while self
                .peek()
                .is_some_and(|v| is_whitespace(v) || is_token(v, &Token::Semicolon))
            {
                self.pos += 1;
            }
            let Some(v) = self.peek() else { return items };
            if let ComponentValue::Token(Token::AtKeyword(_)) = v {
                let Some(ComponentValue::Token(Token::AtKeyword(name))) = self.next() else {
                    unreachable!()
                };
                items.push(Item::AtRule(self.consume_at_rule(name)));
                continue;
            }
            let mark = self.pos;
            let declaration = if matches!(v, ComponentValue::Token(Token::Ident(_))) {
                consume_declaration(self.take_until_semicolon())
                    .filter(|d| !mixes_block_and_values(d))
            } else {
                None
            };
            if let Some(d) = declaration {
                items.push(Item::Declaration(d));
                continue;
            }
            self.pos = mark;
            items.push(match self.consume_qualified_rule(true) {
                Some(rule) => Item::QualifiedRule(rule),
                None => Item::Invalid,
            });
        }
    }
}

/// Une valeur qui contient un bloc `{}` ET autre chose ressemble à une règle
/// (`a:hover { ... }`), pas à une déclaration. Les propriétés personnalisées
/// (`--x: { ... }`) font exception.
fn mixes_block_and_values(d: &Declaration) -> bool {
    !d.name.starts_with("--")
        && d.value.iter().any(is_curly_block)
        && d.value
            .iter()
            .any(|v| !is_curly_block(v) && !is_whitespace(v))
}

/// "Consume a declaration" (§5.4.6) sur des component values qui commencent
/// par l'identifiant du nom.
pub(crate) fn consume_declaration<'a>(values: Vec<ComponentValue<'a>>) -> Option<Declaration<'a>> {
    let mut values = values.into_iter();
    let Some(ComponentValue::Token(Token::Ident(name))) = values.next() else {
        return None;
    };
    let mut rest = values.skip_while(is_whitespace);
    if !rest.next().is_some_and(|v| is_token(&v, &Token::Colon)) {
        return None;
    }
    let mut value: Vec<ComponentValue<'a>> = rest.collect();

    // `!important` : les deux dernières valeurs non-espaces sont `!` et `important`.
    let non_ws: Vec<usize> = (0..value.len())
        .filter(|&i| !is_whitespace(&value[i]))
        .collect();
    let mut important = false;
    if let [.., bang, last] = non_ws[..] {
        let is_bang = is_token(&value[bang], &Token::Delim('!'));
        let is_important = matches!(&value[last], ComponentValue::Token(Token::Ident(s)) if s.eq_ignore_ascii_case("important"));
        if is_bang && is_important {
            value.truncate(bang);
            important = true;
        }
    }
    Some(Declaration {
        name,
        value,
        important,
    })
}
