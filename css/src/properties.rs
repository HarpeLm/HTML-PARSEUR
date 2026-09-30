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
    /// Une liste de familles de polices (`font-family`), chacune déjà
    /// sérialisée : `"Times New Roman"` (chaîne), `Georgia`, `serif`.
    FontFamily(Vec<String>),
}

impl fmt::Display for SpecifiedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpecifiedValue::Keyword(k) => write!(f, "{k}"),
            SpecifiedValue::LengthPercentage(lp) => write!(f, "{lp}"),
            SpecifiedValue::Number(n) => write!(f, "{}", format_number(*n)),
            SpecifiedValue::Integer(i) => write!(f, "{i}"),
            SpecifiedValue::FontFamily(list) => write!(f, "{}", list.join(", ")),
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

/// `display` : un mot-clé, ou `block math` / `inline math` (MathML), que
/// Chromium écrit `block math` et `math`.
fn display(values: &[&ComponentValue]) -> Option<SpecifiedValue> {
    if let [outer, inner] = values
        && keyword(inner, &["math"]).is_some()
    {
        return match keyword(outer, &["block", "inline"])? {
            SpecifiedValue::Keyword("block") => Some(SpecifiedValue::Keyword("block math")),
            _ => Some(SpecifiedValue::Keyword("math")),
        };
    }
    single(values, DISPLAY, |v| keyword(v, &["math"]))
}

const BORDER_STYLES: &[&str] = &[
    "none", "hidden", "dotted", "dashed", "solid", "double", "groove", "ridge", "inset", "outset",
];
const OVERFLOW: &[&str] = &["visible", "hidden", "clip", "scroll", "auto"];

/// Une épaisseur de bordure : `thin`, `medium`, `thick` ou une longueur positive.
fn border_width(v: &ComponentValue) -> Option<SpecifiedValue> {
    keyword(v, &["thin", "medium", "thick"]).or_else(|| {
        parse_length_percentage(
            v,
            Allowed {
                percentage: false,
                negative: false,
            },
        )
        .map(SpecifiedValue::LengthPercentage)
    })
}

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
        "display" => ("display", display(values)?),
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
        "border-top-width" | "border-right-width" | "border-bottom-width" | "border-left-width" => {
            (static_name(name), single(values, &[], border_width)?)
        }
        "border-top-style" | "border-right-style" | "border-bottom-style" | "border-left-style" => {
            (static_name(name), single(values, BORDER_STYLES, none)?)
        }
        "overflow-x" | "overflow-y" => (static_name(name), single(values, OVERFLOW, none)?),
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
        "border-top-width",
        "border-right-width",
        "border-bottom-width",
        "border-left-width",
        "border-top-style",
        "border-right-style",
        "border-bottom-style",
        "border-left-style",
        "overflow-x",
        "overflow-y",
    ];
    NAMES.iter().find(|n| **n == name).copied().unwrap_or("?")
}

/// `margin`, `padding`, `border-width`, `border-style` : 1 à 4 valeurs, dans
/// l'ordre haut, droite, bas, gauche. `names` : les 4 propriétés longues ;
/// chaque valeur est parsée comme la première.
fn four_sides(names: [&str; 4], values: &[&ComponentValue]) -> Option<Vec<Longhand>> {
    let parsed: Vec<SpecifiedValue> = values
        .iter()
        .map(|v| longhand(names[0], &[v]).map(|(_, value)| value))
        .collect::<Option<_>>()?;
    let [t, r, b, l] = match parsed.len() {
        1 => [0, 0, 0, 0],
        2 => [0, 1, 0, 1],
        3 => [0, 1, 2, 1],
        4 => [0, 1, 2, 3],
        _ => return None,
    };
    Some(
        names
            .iter()
            .zip([t, r, b, l])
            .map(|(name, i)| (static_name(name), parsed[i].clone()))
            .collect(),
    )
}

fn side_names(pattern: &str) -> [String; 4] {
    ["top", "right", "bottom", "left"].map(|side| pattern.replace("{}", side))
}

/// `border`, `border-top`... : une épaisseur, un style et une couleur, dans
/// n'importe quel ordre, chacun au plus une fois. Ce qui manque reprend sa
/// valeur initiale (`medium`, `none`). La couleur est vérifiée mais pas encore
/// gardée (Lumen ne calcule pas encore les couleurs).
fn border_shorthand(sides: &[&str], values: &[&ComponentValue]) -> Option<Vec<Longhand>> {
    let (mut width, mut style, mut color) = (None, None, false);
    for v in values {
        if width.is_none()
            && let Some(w) = border_width(v)
        {
            width = Some(w);
        } else if style.is_none()
            && let Some(s) = keyword(v, BORDER_STYLES)
        {
            style = Some(s);
        } else if !color && crate::color::parse_color(std::slice::from_ref(*v)).is_some() {
            color = true;
        } else {
            return None;
        }
    }
    let width = width.unwrap_or(SpecifiedValue::Keyword("medium"));
    let style = style.unwrap_or(SpecifiedValue::Keyword("none"));
    let mut out = Vec::new();
    for side in sides {
        out.push((static_name(&format!("border-{side}-width")), width.clone()));
        out.push((static_name(&format!("border-{side}-style")), style.clone()));
    }
    Some(out)
}

/// `font-family` : des familles séparées par des virgules, chacune une chaîne
/// ou une suite d'identifiants (`Times New Roman`).
fn font_family(values: &[&ComponentValue]) -> Option<SpecifiedValue> {
    let mut families = Vec::new();
    for part in values.split(|v| matches!(v, ComponentValue::Token(Token::Comma))) {
        let family = match part {
            [ComponentValue::Token(Token::String(s))] => format!("\"{s}\""),
            _ => {
                let words: Vec<&str> = part
                    .iter()
                    .map(|v| match v {
                        ComponentValue::Token(Token::Ident(w)) => Some(w.as_ref()),
                        _ => None,
                    })
                    .collect::<Option<_>>()?;
                // Les mots-clés globaux ne peuvent pas être des noms de famille.
                let reserved = ["initial", "inherit", "unset", "default", "revert"];
                if words.is_empty()
                    || words
                        .iter()
                        .any(|w| reserved.iter().any(|k| w.eq_ignore_ascii_case(k)))
                {
                    return None;
                }
                words.join(" ")
            }
        };
        families.push(family);
    }
    Some(SpecifiedValue::FontFamily(families))
}

/// Le raccourci `font` : `[style || variant || weight || stretch]? size
/// [/ line-height]? family`. Il remet à leur valeur initiale les propriétés
/// qu'il ne précise pas (`line-height: normal`...).
fn font_shorthand(values: &[&ComponentValue]) -> Option<Vec<Longhand>> {
    const STYLE_LIKE: &[&str] = &[
        "normal",
        "italic",
        "oblique",
        "small-caps",
        "ultra-condensed",
        "extra-condensed",
        "condensed",
        "semi-condensed",
        "semi-expanded",
        "expanded",
        "extra-expanded",
        "ultra-expanded",
    ];
    let mut weight = SpecifiedValue::Keyword("normal");
    let mut i = 0;
    // Au plus 4 mots avant la taille.
    while i < values.len().min(4) {
        let v = values[i];
        if let Some((_, w)) = longhand("font-weight", &[v]) {
            weight = w;
        } else if keyword(v, STYLE_LIKE).is_none() {
            break;
        }
        i += 1;
    }
    let (_, size) = longhand("font-size", &[values.get(i)?])?;
    i += 1;
    let mut line_height = SpecifiedValue::Keyword("normal");
    if matches!(
        values.get(i),
        Some(ComponentValue::Token(Token::Delim('/')))
    ) {
        line_height = longhand("line-height", &[values.get(i + 1)?])?.1;
        i += 2;
    }
    let family = font_family(&values[i..])?;
    Some(vec![
        ("font-size", size),
        ("font-weight", weight),
        ("line-height", line_height),
        ("font-family", family),
    ])
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
        "margin" | "padding" => {
            let names = side_names(&format!("{name}-{{}}"));
            four_sides(names.each_ref().map(String::as_str), &values)
        }
        "border-width" | "border-style" => {
            let kind = name.trim_start_matches("border-");
            let names = side_names(&format!("border-{{}}-{kind}"));
            four_sides(names.each_ref().map(String::as_str), &values)
        }
        "border" => border_shorthand(&["top", "right", "bottom", "left"], &values),
        "border-top" | "border-right" | "border-bottom" | "border-left" => {
            border_shorthand(&[name.trim_start_matches("border-")], &values)
        }
        "overflow" => {
            let parsed: Vec<SpecifiedValue> = values
                .iter()
                .map(|v| keyword(v, OVERFLOW))
                .collect::<Option<_>>()?;
            match &parsed[..] {
                [both] => Some(vec![
                    ("overflow-x", both.clone()),
                    ("overflow-y", both.clone()),
                ]),
                [x, y] => Some(vec![("overflow-x", x.clone()), ("overflow-y", y.clone())]),
                _ => None,
            }
        }
        "font" => font_shorthand(&values),
        "font-family" => font_family(&values).map(|v| vec![("font-family", v)]),
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
        "font-family" => SpecifiedValue::FontFamily(vec!["serif".into()]),
        "border-top-width" | "border-right-width" | "border-bottom-width" | "border-left-width" => {
            Keyword("medium")
        }
        "border-top-style" | "border-right-style" | "border-bottom-style" | "border-left-style" => {
            Keyword("none")
        }
        "overflow-x" | "overflow-y" => Keyword("visible"),
        _ => return None,
    })
}

/// La propriété est héritée : sans déclaration (ou avec `unset`), elle prend la
/// valeur du parent plutôt que sa valeur initiale.
pub fn is_inherited(name: &str) -> bool {
    matches!(
        name,
        "visibility" | "font-size" | "font-weight" | "line-height" | "font-family"
    )
}
