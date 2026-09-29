//! Une feuille de style prête pour la cascade : la liste des règles de style
//! (sélecteurs + déclarations), les `@media` et `@supports` déjà évalués.
//!
//! On lit la feuille token par token en gardant les positions : la valeur de
//! chaque déclaration reste le texte d'origine, ce dont `var()` a besoin.

use std::ops::Range;

use lumen_css::media::{Environment, MediaQueryList};
use lumen_css::properties::{Longhand, parse_property};
use lumen_css::selectors::{SelectorList, parse_selector_list};
use lumen_css::variables::{
    contains_var, css_wide_keyword, is_custom_property, is_valid_value, raw_declarations,
};
use lumen_css::{BlockKind, ComponentValue, Parser, Token, Tokenizer, preprocess};

/// `propriété: valeur [!important]`, la valeur gardée en texte.
#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    /// Le nom, en minuscules (sauf les propriétés personnalisées `--x`,
    /// sensibles à la casse).
    pub name: String,
    /// Le texte de la valeur (prétraité, espaces des bords retirés).
    pub value: String,
    /// `!important`.
    pub important: bool,
    /// La valeur analysée une fois, à la lecture de la feuille.
    pub parsed: Parsed,
}

/// Ce que donne la valeur d'une déclaration, analysée une fois pour toutes
/// (et non pour chaque élément auquel la règle s'applique).
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    /// `--nom: ...` : une propriété personnalisée (son texte suffit).
    Custom,
    /// Les propriétés longues définies et leur valeur.
    Longhands(Vec<Longhand>),
    /// `initial`, `inherit`, `unset`... pour ces propriétés longues.
    Keyword(&'static str, Vec<&'static str>),
    /// La valeur contient `var()` : elle dépend de l'élément, on la substitue
    /// au moment du calcul. Les propriétés longues visées.
    Var(Vec<&'static str>),
    /// Ignorée : valeur invalide, ou propriété que Lumen ne calcule pas encore.
    Ignored,
}

/// Analyse une déclaration.
fn analyze(name: &str, value: &str) -> Parsed {
    if is_custom_property(name) {
        return Parsed::Custom;
    }
    let targets = crate::computed::longhands(name);
    if targets.is_empty() {
        return Parsed::Ignored;
    }
    if let Some(keyword) = css_wide_keyword(value) {
        return Parsed::Keyword(keyword, targets);
    }
    if contains_var(value) {
        return if is_valid_value(value) {
            Parsed::Var(targets)
        } else {
            Parsed::Ignored
        };
    }
    match parse_property(name, &Parser::new(value).parse_component_value_list()) {
        Some(longhands) => Parsed::Longhands(longhands),
        None => Parsed::Ignored,
    }
}

/// `sélecteurs { déclarations }`.
#[derive(Debug, Clone)]
pub struct StyleRule {
    /// Les sélecteurs.
    pub selectors: SelectorList,
    /// Les déclarations, dans l'ordre.
    pub declarations: Vec<Declaration>,
}

/// Une feuille de style : ses règles de style, dans l'ordre de la feuille.
#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    /// Les règles qui s'appliquent dans l'environnement donné au parsing.
    pub rules: Vec<StyleRule>,
}

impl Stylesheet {
    /// Parse une feuille de style. Les blocs `@media` et `@supports` sont
    /// évalués tout de suite dans `env` : seules leurs règles qui s'appliquent
    /// sont gardées. Les autres at-rules (`@font-face`, `@keyframes`,
    /// `@import`...) sont ignorées.
    pub fn parse(css: &str, env: &Environment) -> Stylesheet {
        let css = preprocess(css);
        let mut rules = Vec::new();
        parse_rules(&css, env, &mut rules);
        Stylesheet { rules }
    }
}

/// Les déclarations d'un attribut `style="..."`.
pub fn parse_style_attribute(text: &str) -> Vec<Declaration> {
    declarations(&preprocess(text))
}

fn declarations(block: &str) -> Vec<Declaration> {
    raw_declarations(block)
        .into_iter()
        .map(|d| {
            let name = if is_custom_property(&d.name) {
                d.name.into_owned()
            } else {
                d.name.to_ascii_lowercase()
            };
            Declaration {
                parsed: analyze(&name, d.value),
                name,
                value: d.value.to_string(),
                important: d.important,
            }
        })
        .collect()
}

type Spanned<'a> = (Token<'a>, Range<usize>);

fn nesting(t: &Token) -> i32 {
    match t {
        Token::Function(_) | Token::OpenParen | Token::OpenSquare | Token::OpenCurly => 1,
        Token::CloseParen | Token::CloseSquare | Token::CloseCurly => -1,
        _ => 0,
    }
}

/// L'indice du premier token de profondeur 0 qui vérifie `stop`, à partir de `from`.
fn find_top_level(toks: &[Spanned], from: usize, stop: impl Fn(&Token) -> bool) -> Option<usize> {
    let mut depth = 0;
    for (i, (t, _)) in toks.iter().enumerate().skip(from) {
        if depth == 0 && stop(t) {
            return Some(i);
        }
        depth = (depth + nesting(t)).max(0);
    }
    None
}

/// L'indice du `}` qui ferme le `{` en `open` (ou la fin).
fn matching_close(toks: &[Spanned], open: usize) -> usize {
    let mut depth = 0;
    for (i, (t, _)) in toks.iter().enumerate().skip(open) {
        depth += nesting(t);
        if depth == 0 {
            return i;
        }
    }
    toks.len()
}

fn parse_rules(css: &str, env: &Environment, out: &mut Vec<StyleRule>) {
    let mut tokenizer = Tokenizer::new(css);
    let toks: Vec<Spanned> = std::iter::from_fn(|| tokenizer.next_with_span()).collect();
    let end_of = |i: usize| toks.get(i).map_or(css.len(), |t| t.1.start);
    let mut i = 0;
    while i < toks.len() {
        match &toks[i].0 {
            Token::Whitespace | Token::Cdo | Token::Cdc | Token::Semicolon => i += 1,
            Token::AtKeyword(name) => {
                let name = name.to_ascii_lowercase();
                let Some(stop) = find_top_level(&toks, i + 1, |t| {
                    matches!(t, Token::Semicolon | Token::OpenCurly)
                }) else {
                    return; // at-rule jamais terminée : ignorée
                };
                if matches!(toks[stop].0, Token::Semicolon) {
                    i = stop + 1; // @import, @charset... : ignorées
                    continue;
                }
                let close = matching_close(&toks, stop);
                let prelude = &css[toks[i].1.end..toks[stop].1.start];
                let block = &css[toks[stop].1.end..end_of(close)];
                let applies = match name.as_str() {
                    "media" => {
                        let values = Parser::new(prelude).parse_component_value_list();
                        MediaQueryList::parse(&values).matches(env)
                    }
                    "supports" => supports(prelude),
                    // Les couches ne sont pas encore ordonnées : leur contenu est
                    // traité comme s'il était hors couche.
                    "layer" => true,
                    _ => false,
                };
                if applies {
                    parse_rules(block, env, out);
                }
                i = close + 1;
            }
            _ => {
                let Some(open) = find_top_level(&toks, i, |t| matches!(t, Token::OpenCurly)) else {
                    return; // règle sans bloc à la fin : ignorée
                };
                let close = matching_close(&toks, open);
                let prelude = &css[toks[i].1.start..toks[open].1.start];
                let block = &css[toks[open].1.end..end_of(close)];
                let values = Parser::new(prelude).parse_component_value_list();
                if let Some(selectors) = parse_selector_list(&values) {
                    out.push(StyleRule {
                        selectors,
                        declarations: declarations(block),
                    });
                }
                i = close + 1;
            }
        }
    }
}

// ───────────── @supports ─────────────

fn is_ws(v: &ComponentValue) -> bool {
    matches!(v, ComponentValue::Token(Token::Whitespace))
}

fn is_word(v: &ComponentValue, word: &str) -> bool {
    matches!(v, ComponentValue::Token(Token::Ident(s)) if s.eq_ignore_ascii_case(word))
}

/// Évalue la condition d'un `@supports`.
fn supports(prelude: &str) -> bool {
    let values = Parser::new(prelude).parse_component_value_list();
    let toks: Vec<&ComponentValue> = values.iter().filter(|v| !is_ws(v)).collect();
    supports_condition(&toks).unwrap_or(false)
}

fn supports_condition(toks: &[&ComponentValue]) -> Option<bool> {
    let first = toks.first()?;
    if is_word(first, "not") {
        let [_, x] = toks else { return None };
        return supports_in_parens(x).map(|b| !b);
    }
    let mut result = supports_in_parens(first)?;
    let mut op: Option<bool> = None; // Some(true) : or
    for pair in toks[1..].chunks(2) {
        let [word, next] = pair else { return None };
        let is_or = if is_word(word, "and") {
            false
        } else if is_word(word, "or") {
            true
        } else {
            return None;
        };
        if op.is_some_and(|o| o != is_or) {
            return None;
        }
        op = Some(is_or);
        let value = supports_in_parens(next)?;
        result = if is_or {
            result || value
        } else {
            result && value
        };
    }
    Some(result)
}

fn supports_in_parens(v: &ComponentValue) -> Option<bool> {
    match v {
        ComponentValue::Block {
            kind: BlockKind::Paren,
            contents,
        } => {
            let inner: Vec<&ComponentValue> = contents.iter().filter(|v| !is_ws(v)).collect();
            if let Some(result) = supports_condition(&inner) {
                return Some(result);
            }
            Some(supports_declaration(contents))
        }
        ComponentValue::Function { name, arguments } if name.eq_ignore_ascii_case("selector") => {
            Some(parse_selector_list(arguments).is_some())
        }
        // Autres fonctions (`font-tech()`...) : non prises en charge.
        ComponentValue::Function { .. } => Some(false),
        _ => None,
    }
}

/// `(propriété: valeur)`. Pour une propriété que Lumen ne connaît pas encore,
/// on ne peut pas savoir : on suppose qu'un navigateur moderne la gère, sauf si
/// elle est préfixée pour un autre moteur (`-moz-`, `-ms-`, `-o-`).
fn supports_declaration(contents: &[ComponentValue]) -> bool {
    let toks: Vec<&ComponentValue> = contents.iter().filter(|v| !is_ws(v)).collect();
    let [
        ComponentValue::Token(Token::Ident(name)),
        ComponentValue::Token(Token::Colon),
        ..,
    ] = toks[..]
    else {
        return false;
    };
    let colon = contents
        .iter()
        .position(|v| matches!(v, ComponentValue::Token(Token::Colon)))
        .unwrap_or(0);
    let value = &contents[colon + 1..];
    let name = name.to_ascii_lowercase();
    if is_custom_property(&name) {
        return true;
    }
    if ["-moz-", "-ms-", "-o-"].iter().any(|p| name.starts_with(p)) {
        return false;
    }
    if crate::computed::is_supported(&name) {
        return parse_property(&name, value).is_some();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regles_media_supports() {
        let env = Environment {
            width: 800.0,
            ..Environment::default()
        };
        let sheet = Stylesheet::parse(
            "a { color: red; --x: 1px /*c*/ 2px }
             @media (min-width: 600px) { b { display: block } }
             @media (max-width: 600px) { i { display: block } }
             @supports (display: flex) and (not (display: bogus)) { u { display: flex } }
             @font-face { font-family: x }
             @import url(x.css);
             s { z-index: 1 !important }",
            &env,
        );
        let selectors: Vec<usize> = sheet.rules.iter().map(|r| r.declarations.len()).collect();
        assert_eq!(selectors, [2, 1, 1, 1]);
        assert_eq!(sheet.rules[0].declarations[1].value, "1px /*c*/ 2px");
        assert!(sheet.rules[3].declarations[0].important);
    }
}
