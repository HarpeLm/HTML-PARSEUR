//! Propriétés CSS : la grammaire de chaque propriété, et la décomposition des
//! raccourcis (`margin: 10px auto` -> `margin-top: 10px`, `margin-right: auto`...).
//!
//! Premier ensemble de propriétés (boîte, position, affichage, police). Les
//! résultats sont vérifiés contre Chromium (tests/oracle).

use std::fmt;

use crate::parser::ComponentValue;
use crate::tokenizer::Token;
use crate::values::{Allowed, LengthPercentage, format_number, parse_length_percentage};

/// La valeur "spécifiée" d'une propriété longue, après parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedValue {
    /// Un mot-clé (`auto`, `block`, `bold`...), en minuscules.
    Keyword(&'static str),
    /// Une longueur, un pourcentage ou un `calc()`.
    LengthPercentage(LengthPercentage),
    /// Un nombre (`opacity: 0.5`, `line-height: 1.5`).
    Number(f64),
    /// Un entier (`z-index: 10`).
    Integer(i32),
}

impl fmt::Display for SpecifiedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpecifiedValue::Keyword(k) => write!(f, "{k}"),
            SpecifiedValue::LengthPercentage(lp) => write!(f, "{lp}"),
            SpecifiedValue::Number(n) => write!(f, "{}", format_number(*n)),
            SpecifiedValue::Integer(i) => write!(f, "{i}"),
        }
    }
}

/// Une déclaration de propriété longue : (`margin-top`, `10px`).
pub type Longhand = (&'static str, SpecifiedValue);

// ───────────── Briques de grammaire ─────────────

fn keyword(v: &ComponentValue, allowed: &[&'static str]) -> Option<SpecifiedValue> {
    let ComponentValue::Token(Token::Ident(s)) = v else {
        return None;
    };
    allowed
        .iter()
        .find(|k| s.eq_ignore_ascii_case(k))
        .map(|k| SpecifiedValue::Keyword(k))
}

fn lp(v: &ComponentValue, negative: bool) -> Option<SpecifiedValue> {
    parse_length_percentage(
        v,
        Allowed {
            percentage: true,
            negative,
        },
    )
    .map(SpecifiedValue::LengthPercentage)
}

fn number(v: &ComponentValue) -> Option<f64> {
    match v {
        ComponentValue::Token(Token::Number(n)) => Some(n.value),
        _ => None,
    }
}

/// Une seule valeur : `f` ou un des `keywords`.
fn single(
    values: &[&ComponentValue],
    keywords: &[&'static str],
    f: impl Fn(&ComponentValue) -> Option<SpecifiedValue>,
) -> Option<SpecifiedValue> {
    let [v] = values[..] else { return None };
    keyword(v, keywords).or_else(|| f(v))
}

const DISPLAY: &[&str] = &[
    "none",
    "contents",
    "block",
    "inline",
    "inline-block",
    "flow-root",
    "flex",
    "inline-flex",
    "grid",
    "inline-grid",
    "list-item",
    "table",
    "inline-table",
    "table-row-group",
    "table-header-group",
    "table-footer-group",
    "table-row",
    "table-cell",
    "table-column-group",
    "table-column",
    "table-caption",
    "ruby",
    "ruby-text",
];

const FONT_SIZE: &[&str] = &[
    "xx-small",
    "x-small",
    "small",
    "medium",
    "large",
    "x-large",
    "xx-large",
    "xxx-large",
    "larger",
    "smaller",
];

const SIZES: &[&str] = &["min-content", "max-content", "fit-content"];

/// Parse une propriété longue (une seule valeur).
fn longhand(name: &str, values: &[&ComponentValue]) -> Option<(&'static str, SpecifiedValue)> {
    let none = |_: &ComponentValue| None;
    let (name, value): (&'static str, SpecifiedValue) = match name {
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => (
            static_name(name),
            single(values, &["auto"], |v| lp(v, true))?,
        ),
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
            (static_name(name), single(values, &[], |v| lp(v, false))?)
        }
        "top" | "right" | "bottom" | "left" => (
            static_name(name),
            single(values, &["auto"], |v| lp(v, true))?,
        ),
        "width" | "height" => (
            static_name(name),
            single(
                values,
                &["auto", "min-content", "max-content", "fit-content"],
                |v| lp(v, false),
            )?,
        ),
        "min-width" | "min-height" => {
            let kw = ["auto", SIZES[0], SIZES[1], SIZES[2]];
            (static_name(name), single(values, &kw, |v| lp(v, false))?)
        }
        "max-width" | "max-height" => {
            let kw = ["none", SIZES[0], SIZES[1], SIZES[2]];
            (static_name(name), single(values, &kw, |v| lp(v, false))?)
        }
        "display" => ("display", single(values, DISPLAY, none)?),
        "position" => (
            "position",
            single(
                values,
                &["static", "relative", "absolute", "fixed", "sticky"],
                none,
            )?,
        ),
        "float" => (
            "float",
            single(
                values,
                &["none", "left", "right", "inline-start", "inline-end"],
                none,
            )?,
        ),
        "box-sizing" => (
            "box-sizing",
            single(values, &["content-box", "border-box"], none)?,
        ),
        "visibility" => (
            "visibility",
            single(values, &["visible", "hidden", "collapse"], none)?,
        ),
        "opacity" => (
            "opacity",
            single(values, &[], |v| match v {
                ComponentValue::Token(Token::Percentage(p)) => {
                    Some(SpecifiedValue::Number(p.value / 100.0))
                }
                _ => number(v).map(SpecifiedValue::Number),
            })?,
        ),
        "font-size" => ("font-size", single(values, FONT_SIZE, |v| lp(v, false))?),
        "font-weight" => (
            "font-weight",
            single(values, &["normal", "bold", "bolder", "lighter"], |v| {
                number(v)
                    .filter(|n| (1.0..=1000.0).contains(n))
                    .map(SpecifiedValue::Number)
            })?,
        ),
        "line-height" => (
            "line-height",
            single(values, &["normal"], |v| {
                number(v)
                    .filter(|n| *n >= 0.0)
                    .map(SpecifiedValue::Number)
                    .or_else(|| lp(v, false))
            })?,
        ),
        "z-index" => (
            "z-index",
            single(values, &["auto"], |v| match v {
                ComponentValue::Token(Token::Number(n)) if n.is_integer => Some(
                    SpecifiedValue::Integer(n.value.clamp(i32::MIN as f64, i32::MAX as f64) as i32),
                ),
                _ => None,
            })?,
        ),
        _ => return None,
    };
    Some((name, value))
}

/// Le nom de propriété en `&'static str` (pour les propriétés connues).
fn static_name(name: &str) -> &'static str {
    const NAMES: &[&str] = &[
        "margin-top",
        "margin-right",
        "margin-bottom",
        "margin-left",
        "padding-top",
        "padding-right",
        "padding-bottom",
        "padding-left",
        "top",
        "right",
        "bottom",
        "left",
        "width",
        "height",
        "min-width",
        "min-height",
        "max-width",
        "max-height",
    ];
    NAMES.iter().find(|n| **n == name).copied().unwrap_or("?")
}

/// `margin` / `padding` : 1 à 4 valeurs, dans l'ordre haut, droite, bas, gauche.
fn four_sides(prefix: &str, values: &[&ComponentValue]) -> Option<Vec<Longhand>> {
    let sides = ["top", "right", "bottom", "left"];
    let parsed: Vec<SpecifiedValue> = values
        .iter()
        .map(|v| longhand(&format!("{prefix}-top"), &[v]).map(|(_, value)| value))
        .collect::<Option<_>>()?;
    let [t, r, b, l] = match parsed.len() {
        1 => [0, 0, 0, 0],
        2 => [0, 1, 0, 1],
        3 => [0, 1, 2, 1],
        4 => [0, 1, 2, 3],
        _ => return None,
    };
    Some(
        sides
            .iter()
            .zip([t, r, b, l])
            .map(|(side, i)| (static_name(&format!("{prefix}-{side}")), parsed[i].clone()))
            .collect(),
    )
}

/// Parse la valeur d'une propriété (longue ou raccourci) et renvoie les
/// propriétés longues qu'elle définit. `None` si la valeur est invalide (la
/// déclaration est alors ignorée) ou si la propriété est inconnue.
pub fn parse_property(name: &str, value: &[ComponentValue]) -> Option<Vec<Longhand>> {
    let name = name.to_ascii_lowercase();
    let values: Vec<&ComponentValue> = value
        .iter()
        .filter(|v| !matches!(v, ComponentValue::Token(Token::Whitespace)))
        .collect();
    if values.is_empty() {
        return None;
    }
    match name.as_str() {
        "margin" | "padding" => four_sides(&name, &values),
        _ => longhand(&name, &values).map(|l| vec![l]),
    }
}

/// La valeur initiale d'une propriété longue (celle qu'elle prend sans
/// déclaration, ou avec `initial`).
pub fn initial_value(name: &str) -> Option<SpecifiedValue> {
    use SpecifiedValue::Keyword;
    let zero = || {
        SpecifiedValue::LengthPercentage(LengthPercentage::Length(crate::values::Length {
            value: 0.0,
            unit: "px",
        }))
    };
    Some(match name {
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => zero(),
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => zero(),
        "top" | "right" | "bottom" | "left" | "width" | "height" => Keyword("auto"),
        "min-width" | "min-height" => Keyword("auto"),
        "max-width" | "max-height" => Keyword("none"),
        "display" => Keyword("inline"),
        "position" => Keyword("static"),
        "float" => Keyword("none"),
        "box-sizing" => Keyword("content-box"),
        "visibility" => Keyword("visible"),
        "opacity" => SpecifiedValue::Number(1.0),
        "font-size" => Keyword("medium"),
        "font-weight" => Keyword("normal"),
        "line-height" => Keyword("normal"),
        "z-index" => Keyword("auto"),
        _ => return None,
    })
}

/// La propriété est héritée : sans déclaration (ou avec `unset`), elle prend la
/// valeur du parent plutôt que sa valeur initiale.
pub fn is_inherited(name: &str) -> bool {
    matches!(
        name,
        "visibility" | "font-size" | "font-weight" | "line-height"
    )
}
