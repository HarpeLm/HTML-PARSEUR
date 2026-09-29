//! Media queries (spec Media Queries Level 4 et 5) :
//! `@media screen and (min-width: 600px)`, `(400px < width <= 800px)`...
//!
//! Une requête est parsée, sérialisée et évaluée dans un [`Environment`] (la
//! taille de la fenêtre, la densité de pixels, les préférences de l'utilisateur).
//! Comme dans les navigateurs, une partie inconnue (`(truc: 5px)`) ne rend pas
//! la requête invalide : elle vaut "inconnu", et la logique est à trois valeurs
//! (`(color) or (truc)` est vrai).
//!
//! Vérifié contre Chromium, sérialisation et évaluation (tests/oracle_media.rs).
//!
//! ```
//! use lumen_css::media::{Environment, MediaQueryList};
//! use lumen_css::{Parser, preprocess};
//!
//! let css = preprocess("screen and (min-width:600px), print");
//! let list = MediaQueryList::parse(&Parser::new(&css).parse_component_value_list());
//! assert_eq!(list.to_string(), "screen and (min-width: 600px), print");
//! let env = Environment { width: 800.0, ..Environment::default() };
//! assert!(list.matches(&env));
//! ```

use std::collections::BTreeMap;
use std::fmt;

use crate::parser::{BlockKind, ComponentValue};
use crate::tokenizer::Token;
use crate::values::{
    Allowed, LengthPercentage, format_number, parse_length_percentage, px_per_unit,
};

/// Ce dans quoi une media query est évaluée.
#[derive(Debug, Clone, PartialEq)]
pub struct Environment {
    /// `screen` ou `print`.
    pub media_type: String,
    /// Largeur de la fenêtre, en px CSS (barre de défilement comprise).
    pub width: f64,
    /// Hauteur de la fenêtre, en px CSS.
    pub height: f64,
    /// Largeur de l'écran, en px CSS.
    pub device_width: f64,
    /// Hauteur de l'écran, en px CSS.
    pub device_height: f64,
    /// Pixels de l'appareil par px CSS (`devicePixelRatio`).
    pub resolution: f64,
    /// Bits par composante de couleur (0 : écran monochrome).
    pub color: u32,
    /// Taille de police initiale, pour `em` et `rem` (16px).
    pub font_size: f64,
    /// Les caractéristiques "discrètes" : `hover` -> `hover`,
    /// `prefers-color-scheme` -> `dark`... Absente : la valeur "fausse".
    pub features: BTreeMap<String, String>,
}

impl Default for Environment {
    /// Un écran d'ordinateur ordinaire : 1024×768, souris, thème clair.
    fn default() -> Self {
        let features = [
            ("hover", "hover"),
            ("any-hover", "hover"),
            ("pointer", "fine"),
            ("any-pointer", "fine"),
            ("prefers-color-scheme", "light"),
            ("scripting", "enabled"),
            ("update", "fast"),
            ("overflow-block", "scroll"),
            ("overflow-inline", "scroll"),
            ("color-gamut", "srgb"),
            ("dynamic-range", "standard"),
            ("video-dynamic-range", "standard"),
            ("display-mode", "browser"),
        ];
        Environment {
            media_type: "screen".into(),
            width: 1024.0,
            height: 768.0,
            device_width: 1024.0,
            device_height: 768.0,
            resolution: 1.0,
            color: 8,
            font_size: 16.0,
            features: features
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }
}

// ───────────── Structure ─────────────

/// Une liste de requêtes séparées par des virgules : vraie si l'une l'est
/// (ou si elle est vide).
#[derive(Debug, Clone, PartialEq)]
pub struct MediaQueryList(pub Vec<MediaQuery>);

/// Une requête.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaQuery {
    /// Une requête invalide : elle ne correspond jamais (sérialisée `not all`).
    Invalid,
    /// `[not | only]? type [and condition]?`, ou une condition seule.
    Query {
        /// `not` ou `only`.
        qualifier: Option<Qualifier>,
        /// Le type de média, en minuscules (`screen`, `print`...).
        media_type: Option<String>,
        /// La condition.
        condition: Option<Condition>,
    },
}

/// `not` ou `only` devant le type de média.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Qualifier {
    /// `not screen`.
    Not,
    /// `only screen` (sans effet, pour les vieux navigateurs).
    Only,
}

/// Une condition : `not (a)`, `(a) and (b)`, `(a) or (b)`.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// `not (a)`.
    Not(InParens),
    /// `(a) and (b) and ...` (ou un seul élément).
    And(Vec<InParens>),
    /// `(a) or (b) or ...`.
    Or(Vec<InParens>),
}

/// Ce qui est entre parenthèses.
#[derive(Debug, Clone, PartialEq)]
pub enum InParens {
    /// `((a) and (b))`.
    Condition(Box<Condition>),
    /// `(min-width: 600px)`.
    Feature(Feature),
    /// Tout le reste (`(truc: 5px)`, `f(x)`) : toujours "inconnu", gardé tel
    /// qu'écrit.
    Unknown(String),
}

/// `min-` ou `max-`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prefix {
    /// `min-width` : au moins.
    Min,
    /// `max-width` : au plus.
    Max,
}

/// Une comparaison de la syntaxe d'intervalle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `=`
    Eq,
}

/// Une caractéristique testée.
#[derive(Debug, Clone, PartialEq)]
pub enum Feature {
    /// `(color)` : vraie si la valeur n'est pas "nulle".
    Boolean(&'static str),
    /// `(min-width: 600px)`.
    Plain {
        /// Le nom écrit, en minuscules (`min-width`).
        name: String,
        /// La caractéristique (`width`).
        feature: &'static str,
        /// `min-` ou `max-`.
        prefix: Option<Prefix>,
        /// La valeur.
        value: Value,
    },
    /// `(width >= 600px)`, `(400px < width <= 800px)`.
    Range {
        /// La caractéristique.
        feature: &'static str,
        /// `valeur op` à gauche du nom.
        left: Option<(Value, Comparison)>,
        /// `op valeur` à droite du nom.
        right: Option<(Comparison, Value)>,
    },
}

/// La valeur d'une caractéristique.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `600px`, `37.5em`, `calc(...)`.
    Length(LengthPercentage),
    /// `8`, `0`.
    Number(f64),
    /// `16 / 9`.
    Ratio(f64, f64),
    /// `2dppx`, `192dpi`, `2x`.
    Resolution(f64, &'static str),
    /// `portrait`, `dark`...
    Keyword(&'static str),
}

// ───────────── Les caractéristiques connues ─────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Type {
    Length,
    Ratio,
    Resolution,
    Integer,
    /// `-webkit-device-pixel-ratio` : un nombre, comparé à la résolution.
    Number,
    /// 0 ou 1.
    Grid,
    /// Des mots-clés ; `false_value` rend `(nom)` faux ; `ordered` : une valeur
    /// implique les précédentes (`color-gamut: p3` implique `srgb`).
    Keywords {
        values: &'static [&'static str],
        false_value: Option<&'static str>,
        ordered: bool,
    },
}

impl Type {
    fn is_range(self) -> bool {
        matches!(
            self,
            Type::Length | Type::Ratio | Type::Resolution | Type::Integer | Type::Number
        )
    }
}

const fn kw(values: &'static [&'static str], false_value: Option<&'static str>) -> Type {
    Type::Keywords {
        values,
        false_value,
        ordered: false,
    }
}

const NONE_HOVER: &[&str] = &["none", "hover"];
const POINTERS: &[&str] = &["none", "coarse", "fine"];
const PREFERENCE: &[&str] = &["no-preference", "reduce"];
const RANGES: &[&str] = &["standard", "high"];

/// Les caractéristiques que Chromium connaît (hors expérimentales).
const FEATURES: &[(&str, Type)] = &[
    ("width", Type::Length),
    ("height", Type::Length),
    ("device-width", Type::Length),
    ("device-height", Type::Length),
    ("aspect-ratio", Type::Ratio),
    ("device-aspect-ratio", Type::Ratio),
    ("resolution", Type::Resolution),
    ("color", Type::Integer),
    ("color-index", Type::Integer),
    ("monochrome", Type::Integer),
    ("-webkit-device-pixel-ratio", Type::Number),
    ("grid", Type::Grid),
    ("orientation", kw(&["portrait", "landscape"], None)),
    ("hover", kw(NONE_HOVER, Some("none"))),
    ("any-hover", kw(NONE_HOVER, Some("none"))),
    ("pointer", kw(POINTERS, Some("none"))),
    ("any-pointer", kw(POINTERS, Some("none"))),
    ("prefers-color-scheme", kw(&["light", "dark"], None)),
    (
        "prefers-reduced-motion",
        kw(PREFERENCE, Some("no-preference")),
    ),
    (
        "prefers-reduced-transparency",
        kw(PREFERENCE, Some("no-preference")),
    ),
    (
        "prefers-contrast",
        kw(
            &["no-preference", "less", "more", "custom"],
            Some("no-preference"),
        ),
    ),
    ("forced-colors", kw(&["none", "active"], Some("none"))),
    (
        "scripting",
        kw(&["none", "initial-only", "enabled"], Some("none")),
    ),
    ("update", kw(&["none", "slow", "fast"], Some("none"))),
    (
        "overflow-block",
        kw(&["none", "scroll", "paged"], Some("none")),
    ),
    ("overflow-inline", kw(&["none", "scroll"], Some("none"))),
    (
        "color-gamut",
        Type::Keywords {
            values: &["srgb", "p3", "rec2020"],
            false_value: None,
            ordered: true,
        },
    ),
    (
        "dynamic-range",
        Type::Keywords {
            values: RANGES,
            false_value: None,
            ordered: true,
        },
    ),
    (
        "video-dynamic-range",
        Type::Keywords {
            values: RANGES,
            false_value: None,
            ordered: true,
        },
    ),
    (
        "display-mode",
        kw(
            &[
                "browser",
                "fullscreen",
                "standalone",
                "minimal-ui",
                "picture-in-picture",
                "window-controls-overlay",
            ],
            None,
        ),
    ),
];

fn feature_type(name: &str) -> Option<(&'static str, Type)> {
    FEATURES.iter().find(|(n, _)| *n == name).copied()
}

/// `min-width` -> (`width`, Min) ; `-webkit-max-device-pixel-ratio` aussi.
fn split_prefix(name: &str) -> (Option<Prefix>, String) {
    for (prefix, p) in [("min-", Prefix::Min), ("max-", Prefix::Max)] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return (Some(p), rest.to_string());
        }
        if let Some(rest) = name.strip_prefix(&format!("-webkit-{prefix}")[..]) {
            return (Some(p), format!("-webkit-{rest}"));
        }
    }
    (None, name.to_string())
}

// ───────────── Parsing ─────────────

fn is_ws(v: &ComponentValue) -> bool {
    matches!(v, ComponentValue::Token(Token::Whitespace))
}

fn ident<'v>(v: &'v ComponentValue) -> Option<&'v str> {
    match v {
        ComponentValue::Token(Token::Ident(s)) => Some(s),
        _ => None,
    }
}

fn is_word(v: &ComponentValue, word: &str) -> bool {
    ident(v).is_some_and(|s| s.eq_ignore_ascii_case(word))
}

impl MediaQueryList {
    /// Parse une liste de requêtes (le prélude d'un `@media`, ou le texte donné
    /// à `matchMedia()`). Une requête invalide devient [`MediaQuery::Invalid`]
    /// sans toucher aux autres.
    pub fn parse(values: &[ComponentValue]) -> MediaQueryList {
        if values.iter().all(is_ws) {
            return MediaQueryList(Vec::new());
        }
        let queries = values
            .split(|v| matches!(v, ComponentValue::Token(Token::Comma)))
            .map(|part| parse_query(part).unwrap_or(MediaQuery::Invalid))
            .collect();
        MediaQueryList(queries)
    }

    /// Vrai si l'une des requêtes correspond (une liste vide correspond toujours).
    pub fn matches(&self, env: &Environment) -> bool {
        self.0.is_empty() || self.0.iter().any(|q| q.matches(env))
    }
}

fn parse_query(values: &[ComponentValue]) -> Option<MediaQuery> {
    let toks: Vec<&ComponentValue> = values.iter().filter(|v| !is_ws(v)).collect();
    if toks.is_empty() {
        return None;
    }
    if let Some(condition) = parse_condition(&toks, true) {
        return Some(MediaQuery::Query {
            qualifier: None,
            media_type: None,
            condition: Some(condition),
        });
    }
    let mut i = 0;
    let qualifier = if is_word(toks[0], "not") {
        Some(Qualifier::Not)
    } else if is_word(toks[0], "only") {
        Some(Qualifier::Only)
    } else {
        None
    };
    if qualifier.is_some() {
        i += 1;
    }
    let media_type = ident(toks.get(i)?)?.to_ascii_lowercase();
    if ["only", "not", "and", "or", "layer"].contains(&media_type.as_str()) {
        return None;
    }
    i += 1;
    let condition = match toks.get(i) {
        None => None,
        Some(v) if is_word(v, "and") => Some(parse_condition(&toks[i + 1..], false)?),
        Some(_) => return None,
    };
    Some(MediaQuery::Query {
        qualifier,
        media_type: Some(media_type),
        condition,
    })
}

fn parse_condition(toks: &[&ComponentValue], allow_or: bool) -> Option<Condition> {
    let first = toks.first()?;
    if is_word(first, "not") {
        let [_, x] = toks else { return None };
        return Some(Condition::Not(parse_in_parens(x)?));
    }
    let mut items = vec![parse_in_parens(first)?];
    let mut or = None;
    let mut rest = toks[1..].chunks(2);
    for pair in &mut rest {
        let [word, next] = pair else { return None };
        let is_or = if is_word(word, "and") {
            false
        } else if is_word(word, "or") && allow_or {
            true
        } else {
            return None;
        };
        if or.is_some_and(|o| o != is_or) {
            return None; // `(a) and (b) or (c)` est interdit
        }
        or = Some(is_or);
        items.push(parse_in_parens(next)?);
    }
    Some(if or == Some(true) {
        Condition::Or(items)
    } else {
        Condition::And(items)
    })
}

fn parse_in_parens(v: &ComponentValue) -> Option<InParens> {
    match v {
        ComponentValue::Block {
            kind: BlockKind::Paren,
            contents,
        } => {
            let inner: Vec<&ComponentValue> = contents.iter().filter(|v| !is_ws(v)).collect();
            if let Some(c) = parse_condition(&inner, true) {
                return Some(InParens::Condition(Box::new(c)));
            }
            Some(match parse_feature(contents) {
                Some(f) => InParens::Feature(f),
                None => InParens::Unknown(v.to_css()),
            })
        }
        ComponentValue::Function { .. } => Some(InParens::Unknown(v.to_css())),
        _ => None,
    }
}

/// Un token significatif, et s'il suit un espace (`>=` doit être collé).
type Item<'v, 'a> = (&'v ComponentValue<'a>, bool);

fn parse_feature(contents: &[ComponentValue]) -> Option<Feature> {
    let mut items: Vec<Item> = Vec::new();
    let mut after_ws = false;
    for v in contents {
        if is_ws(v) {
            after_ws = true;
        } else {
            items.push((v, after_ws));
            after_ws = false;
        }
    }
    if let [(v, _)] = items[..] {
        let name = ident(v)?.to_ascii_lowercase();
        return feature_type(&name).map(|(n, _)| Feature::Boolean(n));
    }
    if let [(name, _), (ComponentValue::Token(Token::Colon), _), ..] = items[..] {
        let name = ident(name)?.to_ascii_lowercase();
        let (prefix, base) = split_prefix(&name);
        let (feature, ty) = match prefix.and_then(|_| feature_type(&base)) {
            Some(found) if found.1.is_range() => found,
            Some(_) => return None,
            None => feature_type(&name)?,
        };
        let prefix = if feature == name { None } else { prefix };
        let value = parse_value(ty, &items[2..])?;
        return Some(Feature::Plain {
            name,
            feature,
            prefix,
            value,
        });
    }
    parse_range(&items)
}

fn parse_range(items: &[Item]) -> Option<Feature> {
    let mut segments: Vec<Vec<Item>> = vec![Vec::new()];
    let mut ops = Vec::new();
    let mut i = 0;
    while i < items.len() {
        let delim = match items[i].0 {
            ComponentValue::Token(Token::Delim(c @ ('<' | '>' | '='))) => Some(*c),
            _ => None,
        };
        match delim {
            Some(c) => {
                let then_eq = c != '='
                    && matches!(
                        items.get(i + 1),
                        Some((ComponentValue::Token(Token::Delim('=')), false))
                    );
                if then_eq {
                    i += 1;
                }
                ops.push(match (c, then_eq) {
                    ('<', false) => Comparison::Lt,
                    ('<', true) => Comparison::Le,
                    ('>', false) => Comparison::Gt,
                    ('>', true) => Comparison::Ge,
                    _ => Comparison::Eq,
                });
                segments.push(Vec::new());
            }
            None => segments.last_mut().unwrap().push(items[i]),
        }
        i += 1;
    }
    if segments.iter().any(Vec::is_empty) {
        return None;
    }
    let range_feature = |seg: &[Item]| -> Option<(&'static str, Type)> {
        let [(v, _)] = seg else { return None };
        let found = feature_type(&ident(v)?.to_ascii_lowercase())?;
        (found.1.is_range() && found.1 != Type::Number).then_some(found)
    };
    match (&segments[..], &ops[..]) {
        ([a, b], [op]) => {
            if let Some((feature, ty)) = range_feature(a) {
                let value = parse_value(ty, b)?;
                Some(Feature::Range {
                    feature,
                    left: None,
                    right: Some((*op, value)),
                })
            } else {
                let (feature, ty) = range_feature(b)?;
                let value = parse_value(ty, a)?;
                Some(Feature::Range {
                    feature,
                    left: Some((value, *op)),
                    right: None,
                })
            }
        }
        ([a, name, b], [op1, op2]) => {
            let less = |o: &Comparison| matches!(o, Comparison::Lt | Comparison::Le);
            let greater = |o: &Comparison| matches!(o, Comparison::Gt | Comparison::Ge);
            if !(less(op1) && less(op2) || greater(op1) && greater(op2)) {
                return None;
            }
            let (feature, ty) = range_feature(name)?;
            Some(Feature::Range {
                feature,
                left: Some((parse_value(ty, a)?, *op1)),
                right: Some((*op2, parse_value(ty, b)?)),
            })
        }
        _ => None,
    }
}

fn non_negative_number(v: &ComponentValue) -> Option<f64> {
    match v {
        ComponentValue::Token(Token::Number(n)) if n.value >= 0.0 => Some(n.value),
        _ => None,
    }
}

fn parse_value(ty: Type, items: &[Item]) -> Option<Value> {
    let single = match items {
        [(v, _)] => Some(*v),
        _ => None,
    };
    match ty {
        Type::Length => {
            let v = single?;
            if let ComponentValue::Token(Token::Number(n)) = v {
                return (n.value == 0.0).then_some(Value::Number(0.0));
            }
            let allowed = Allowed {
                percentage: false,
                negative: true,
            };
            parse_length_percentage(v, allowed).map(Value::Length)
        }
        Type::Ratio => match items {
            [(a, _)] => Some(Value::Ratio(non_negative_number(a)?, 1.0)),
            [
                (a, _),
                (ComponentValue::Token(Token::Delim('/')), _),
                (b, _),
            ] => Some(Value::Ratio(
                non_negative_number(a)?,
                non_negative_number(b)?,
            )),
            _ => None,
        },
        Type::Resolution => match single? {
            ComponentValue::Token(Token::Dimension { number, unit }) if number.value >= 0.0 => {
                let unit = ["dppx", "x", "dpi", "dpcm"]
                    .into_iter()
                    .find(|u| unit.eq_ignore_ascii_case(u))?;
                Some(Value::Resolution(number.value, unit))
            }
            _ => None,
        },
        Type::Integer => match single? {
            ComponentValue::Token(Token::Number(n)) if n.is_integer && n.value >= 0.0 => {
                Some(Value::Number(n.value))
            }
            _ => None,
        },
        Type::Number => non_negative_number(single?).map(Value::Number),
        Type::Grid => match single? {
            ComponentValue::Token(Token::Number(n)) if n.is_integer && n.value <= 1.0 => {
                non_negative_number(single?).map(Value::Number)
            }
            _ => None,
        },
        Type::Keywords { values, .. } => {
            let word = ident(single?)?;
            values
                .iter()
                .find(|k| word.eq_ignore_ascii_case(k))
                .map(|k| Value::Keyword(k))
        }
    }
}

// ───────────── Évaluation (logique à trois valeurs : None = inconnu) ─────────────

impl MediaQuery {
    /// Vrai si la requête correspond à l'environnement (l'inconnu compte comme faux).
    pub fn matches(&self, env: &Environment) -> bool {
        let MediaQuery::Query {
            qualifier,
            media_type,
            condition,
        } = self
        else {
            return false;
        };
        let type_ok = match media_type.as_deref() {
            None | Some("all") => true,
            Some(t) => t == env.media_type,
        };
        let condition = condition.as_ref().map_or(Some(true), |c| c.eval(env));
        let result = and([Some(type_ok), condition]);
        let result = match qualifier {
            Some(Qualifier::Not) => result.map(|b| !b),
            _ => result,
        };
        result.unwrap_or(false)
    }
}

fn and(values: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
    let mut result = Some(true);
    for v in values {
        match v {
            Some(false) => return Some(false),
            None => result = None,
            Some(true) => {}
        }
    }
    result
}

fn or(values: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
    let mut result = Some(false);
    for v in values {
        match v {
            Some(true) => return Some(true),
            None => result = None,
            Some(false) => {}
        }
    }
    result
}

impl Condition {
    fn eval(&self, env: &Environment) -> Option<bool> {
        match self {
            Condition::Not(x) => x.eval(env).map(|b| !b),
            Condition::And(items) => and(items.iter().map(|x| x.eval(env))),
            Condition::Or(items) => or(items.iter().map(|x| x.eval(env))),
        }
    }
}

impl InParens {
    fn eval(&self, env: &Environment) -> Option<bool> {
        match self {
            InParens::Condition(c) => c.eval(env),
            InParens::Feature(f) => Some(f.eval(env)),
            InParens::Unknown(_) => None,
        }
    }
}

/// Égalité à un millionième près : `75.5906dpcm` vaut bien `2dppx`.
fn compare(a: f64, op: Comparison, b: f64) -> bool {
    let eq = (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0);
    match op {
        Comparison::Eq => eq,
        Comparison::Lt => a < b && !eq,
        Comparison::Le => a < b || eq,
        Comparison::Gt => a > b && !eq,
        Comparison::Ge => a > b || eq,
    }
}

fn length_px(value: f64, unit: &str, env: &Environment) -> f64 {
    if let Some(factor) = px_per_unit(unit) {
        return value * factor;
    }
    let (w, h, em) = (env.width / 100.0, env.height / 100.0, env.font_size);
    value
        * match unit {
            "em" | "rem" | "ic" => em,
            // Sans police chargée : approximations usuelles.
            "ex" | "rex" | "ch" | "rch" | "cap" => em / 2.0,
            "lh" | "rlh" => em * 1.2,
            "vw" | "svw" | "lvw" | "dvw" | "vi" => w,
            "vh" | "svh" | "lvh" | "dvh" | "vb" => h,
            "vmin" => w.min(h),
            "vmax" => w.max(h),
            _ => 0.0,
        }
}

impl Value {
    fn to_number(&self, env: &Environment) -> Option<f64> {
        Some(match self {
            Value::Number(n) => *n,
            Value::Length(LengthPercentage::Length(l)) => length_px(l.value, l.unit, env),
            Value::Length(LengthPercentage::Calc(sum)) => {
                sum.0.iter().map(|(unit, v)| length_px(*v, unit, env)).sum()
            }
            Value::Length(LengthPercentage::Percentage(_)) => return None,
            Value::Ratio(a, b) => a / b,
            Value::Resolution(v, unit) => match *unit {
                "dpi" => v / 96.0,
                "dpcm" => v * 2.54 / 96.0,
                _ => *v,
            },
            Value::Keyword(_) => return None,
        })
    }
}

impl Feature {
    fn eval(&self, env: &Environment) -> bool {
        match self {
            Feature::Boolean(name) => match feature_type(name).map(|(_, t)| t) {
                Some(Type::Keywords { false_value, .. }) => {
                    let current = keyword_value(name, env);
                    false_value.is_none_or(|f| current.as_deref().is_some_and(|c| c != f))
                }
                _ => numeric_value(name, env).is_some_and(|v| v != 0.0),
            },
            Feature::Plain {
                feature,
                prefix,
                value,
                ..
            } => {
                if let Value::Keyword(k) = value {
                    return keyword_matches(feature, k, env);
                }
                let (Some(current), Some(v)) = (numeric_value(feature, env), value.to_number(env))
                else {
                    return false;
                };
                let op = match prefix {
                    Some(Prefix::Min) => Comparison::Ge,
                    Some(Prefix::Max) => Comparison::Le,
                    None => Comparison::Eq,
                };
                compare(current, op, v)
            }
            Feature::Range {
                feature,
                left,
                right,
            } => {
                let Some(current) = numeric_value(feature, env) else {
                    return false;
                };
                let left_ok = left.as_ref().is_none_or(|(v, op)| {
                    v.to_number(env).is_some_and(|v| compare(v, *op, current))
                });
                let right_ok = right.as_ref().is_none_or(|(op, v)| {
                    v.to_number(env).is_some_and(|v| compare(current, *op, v))
                });
                left_ok && right_ok
            }
        }
    }
}

fn numeric_value(feature: &str, env: &Environment) -> Option<f64> {
    Some(match feature {
        "width" => env.width,
        "height" => env.height,
        "device-width" => env.device_width,
        "device-height" => env.device_height,
        "aspect-ratio" => env.width / env.height,
        "device-aspect-ratio" => env.device_width / env.device_height,
        "resolution" | "-webkit-device-pixel-ratio" => env.resolution,
        "color" => env.color as f64,
        // Pas d'écran à palette ni monochrome.
        "color-index" | "monochrome" => 0.0,
        "grid" => 0.0,
        _ => return None,
    })
}

fn keyword_value(feature: &str, env: &Environment) -> Option<String> {
    if feature == "orientation" {
        let portrait = env.height >= env.width;
        return Some(if portrait { "portrait" } else { "landscape" }.into());
    }
    env.features.get(feature).cloned()
}

fn keyword_matches(feature: &str, keyword: &str, env: &Environment) -> bool {
    let Some(current) = keyword_value(feature, env) else {
        return false;
    };
    match feature_type(feature).map(|(_, t)| t) {
        Some(Type::Keywords {
            values,
            ordered: true,
            ..
        }) => {
            let index = |k: &str| values.iter().position(|v| *v == k);
            matches!((index(keyword), index(&current)), (Some(a), Some(b)) if a <= b)
        }
        _ => current == keyword,
    }
}

// ───────────── Sérialisation (comme `mediaText` dans les navigateurs) ─────────────

impl fmt::Display for MediaQueryList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, q) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{q}")?;
        }
        Ok(())
    }
}

impl fmt::Display for MediaQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let MediaQuery::Query {
            qualifier,
            media_type,
            condition,
        } = self
        else {
            return write!(f, "not all");
        };
        let mut parts = Vec::new();
        match qualifier {
            Some(Qualifier::Not) => parts.push("not".to_string()),
            Some(Qualifier::Only) => parts.push("only".to_string()),
            None => {}
        }
        match (media_type.as_deref(), qualifier, condition) {
            // `all and (color)` s'écrit `(color)`.
            (Some("all"), None, Some(_)) | (None, _, _) => {}
            (Some(t), _, _) => parts.push(t.to_string()),
        }
        if let Some(c) = condition {
            if !parts.is_empty() {
                parts.push("and".into());
            }
            parts.push(c.to_string());
        }
        write!(f, "{}", parts.join(" "))
    }
}

impl fmt::Display for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (items, sep) = match self {
            Condition::Not(x) => return write!(f, "not {x}"),
            Condition::And(items) => (items, " and "),
            Condition::Or(items) => (items, " or "),
        };
        for (i, x) in items.iter().enumerate() {
            if i > 0 {
                write!(f, "{sep}")?;
            }
            write!(f, "{x}")?;
        }
        Ok(())
    }
}

impl fmt::Display for InParens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InParens::Condition(c) => write!(f, "({c})"),
            InParens::Feature(x) => write!(f, "{x}"),
            InParens::Unknown(s) => write!(f, "{s}"),
        }
    }
}

impl fmt::Display for Comparison {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Comparison::Lt => "<",
            Comparison::Le => "<=",
            Comparison::Gt => ">",
            Comparison::Ge => ">=",
            Comparison::Eq => "=",
        })
    }
}

impl fmt::Display for Feature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Feature::Boolean(name) => write!(f, "({name})"),
            Feature::Plain { name, value, .. } => write!(f, "({name}: {value})"),
            Feature::Range {
                feature,
                left,
                right,
            } => {
                write!(f, "(")?;
                if let Some((v, op)) = left {
                    write!(f, "{v} {op} ")?;
                }
                write!(f, "{feature}")?;
                if let Some((op, v)) = right {
                    write!(f, " {op} {v}")?;
                }
                write!(f, ")")
            }
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Length(l) => write!(f, "{l}"),
            Value::Number(n) => write!(f, "{}", format_number(*n)),
            Value::Ratio(a, b) => write!(f, "{} / {}", format_number(*a), format_number(*b)),
            Value::Resolution(v, unit) => write!(f, "{}{unit}", format_number(*v)),
            Value::Keyword(k) => write!(f, "{k}"),
        }
    }
}
