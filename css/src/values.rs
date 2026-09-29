//! Valeurs CSS de base (spec CSS Values and Units Level 4) : nombres,
//! longueurs, pourcentages et `calc()`.
//!
//! La sérialisation suit celle de Chromium (vérifiée par tests/oracle) :
//! 6 chiffres significatifs, `0` écrit `0px`, `calc()` simplifié.

use std::collections::BTreeMap;
use std::fmt;

use crate::parser::{BlockKind, ComponentValue};
use crate::tokenizer::Token;

/// Un nombre sérialisé comme les navigateurs : 6 chiffres significatifs,
/// sans zéros inutiles (`33.3333`, `133.795`, `0.1`, `12`).
pub fn format_number(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        // Les nombres sont bornés au parsing ; un calcul peut encore donner
        // l'infini (`calc(1e308px * 10)`) : on ne panique pas.
        return if v.is_nan() || v == 0.0 {
            "0".into()
        } else if v > 0.0 {
            "3.40282e+38".into()
        } else {
            "-3.40282e+38".into()
        };
    }
    let integer_digits = v.abs().log10().floor() as i32 + 1;
    let decimals = (6 - integer_digits).max(0) as usize;
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    };
    if s == "-0" { "0".into() } else { s }
}

/// Unités de longueur reconnues, et leur valeur en pixels pour les unités absolues.
fn length_unit(unit: &str) -> Option<(&'static str, Option<f64>)> {
    Some(match unit {
        "px" => ("px", Some(1.0)),
        "in" => ("in", Some(96.0)),
        "cm" => ("cm", Some(96.0 / 2.54)),
        "mm" => ("mm", Some(96.0 / 25.4)),
        "q" => ("q", Some(96.0 / 101.6)),
        "pt" => ("pt", Some(96.0 / 72.0)),
        "pc" => ("pc", Some(16.0)),
        "em" => ("em", None),
        "rem" => ("rem", None),
        "ex" => ("ex", None),
        "rex" => ("rex", None),
        "ch" => ("ch", None),
        "rch" => ("rch", None),
        "cap" => ("cap", None),
        "ic" => ("ic", None),
        "lh" => ("lh", None),
        "rlh" => ("rlh", None),
        "vw" => ("vw", None),
        "vh" => ("vh", None),
        "vmin" => ("vmin", None),
        "vmax" => ("vmax", None),
        "vi" => ("vi", None),
        "vb" => ("vb", None),
        "svw" => ("svw", None),
        "svh" => ("svh", None),
        "lvw" => ("lvw", None),
        "lvh" => ("lvh", None),
        "dvw" => ("dvw", None),
        "dvh" => ("dvh", None),
        _ => return None,
    })
}

/// Pixels par unité, pour les unités absolues (`in` -> 96) ; `None` pour les
/// unités relatives (`em`, `vw`...) et inconnues.
pub(crate) fn px_per_unit(unit: &str) -> Option<f64> {
    length_unit(unit)?.1
}

/// Une longueur : `10px`, `1.5em`, `3vw`.
#[derive(Debug, Clone, PartialEq)]
pub struct Length {
    /// La valeur.
    pub value: f64,
    /// L'unité, en minuscules.
    pub unit: &'static str,
}

impl fmt::Display for Length {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", format_number(self.value), self.unit)
    }
}

/// Une somme simplifiée issue de `calc()` : unité -> coefficient.
/// `"%"` désigne les pourcentages ; les unités absolues sont converties en `px`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CalcSum(pub BTreeMap<&'static str, f64>);

impl fmt::Display for CalcSum {
    /// Ordre de la spec : pourcentage d'abord, puis unités par ordre alphabétique.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut terms: Vec<(&str, f64)> = self.0.iter().map(|(u, v)| (*u, *v)).collect();
        terms.sort_by_key(|(u, _)| (*u != "%", *u));
        if terms.is_empty() {
            return write!(f, "calc(0px)");
        }
        write!(f, "calc(")?;
        for (i, (unit, value)) in terms.iter().enumerate() {
            if i == 0 {
                write!(f, "{}{unit}", format_number(*value))?;
            } else {
                let sign = if *value < 0.0 { '-' } else { '+' };
                write!(f, " {sign} {}{unit}", format_number(value.abs()))?;
            }
        }
        write!(f, ")")
    }
}

/// `<length-percentage>` : une longueur, un pourcentage ou un `calc()`.
#[derive(Debug, Clone, PartialEq)]
pub enum LengthPercentage {
    /// `10px`.
    Length(Length),
    /// `50%` (valeur 50).
    Percentage(f64),
    /// `calc(50% - 2em)`.
    Calc(CalcSum),
}

impl fmt::Display for LengthPercentage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LengthPercentage::Length(l) => write!(f, "{l}"),
            LengthPercentage::Percentage(p) => write!(f, "{}%", format_number(*p)),
            LengthPercentage::Calc(c) => write!(f, "{c}"),
        }
    }
}

// ───────────── Parsing ─────────────

/// Ce qu'une valeur a le droit d'être.
#[derive(Debug, Clone, Copy)]
pub struct Allowed {
    /// Les pourcentages sont permis.
    pub percentage: bool,
    /// Les valeurs négatives sont permises (hors `calc()`, vérifié plus tard).
    pub negative: bool,
}

/// Parse `<length-percentage>` (ou `<length>` si les pourcentages sont interdits)
/// à partir d'UNE component value.
pub fn parse_length_percentage(
    value: &ComponentValue,
    allowed: Allowed,
) -> Option<LengthPercentage> {
    let lp = match value {
        ComponentValue::Token(Token::Dimension { number, unit }) => {
            let (unit, _) = length_unit(&unit.to_ascii_lowercase())?;
            LengthPercentage::Length(Length {
                value: number.value,
                unit,
            })
        }
        ComponentValue::Token(Token::Percentage(n)) if allowed.percentage => {
            LengthPercentage::Percentage(n.value)
        }
        // `0` sans unité est une longueur.
        ComponentValue::Token(Token::Number(n)) if n.value == 0.0 => {
            LengthPercentage::Length(Length {
                value: 0.0,
                unit: "px",
            })
        }
        ComponentValue::Function { name, arguments } if name.eq_ignore_ascii_case("calc") => {
            let sum = match parse_calc(arguments)? {
                Calc::Terms(t) => t,
                Calc::Number(_) => return None,
            };
            if !allowed.percentage && sum.0.contains_key("%") {
                return None;
            }
            return Some(LengthPercentage::Calc(sum));
        }
        _ => return None,
    };
    let negative = match &lp {
        LengthPercentage::Length(l) => l.value < 0.0,
        LengthPercentage::Percentage(p) => *p < 0.0,
        LengthPercentage::Calc(_) => false,
    };
    (allowed.negative || !negative).then_some(lp)
}

/// Résultat intermédiaire d'un `calc()` : un nombre pur, ou une somme d'unités.
#[derive(Debug, Clone)]
enum Calc {
    Number(f64),
    Terms(CalcSum),
}

fn is_ws(v: &ComponentValue) -> bool {
    matches!(v, ComponentValue::Token(Token::Whitespace))
}

/// Parse le contenu de `calc( ... )`.
fn parse_calc(values: &[ComponentValue]) -> Option<Calc> {
    // Espaces permis juste après la parenthèse ouvrante et avant la fermante.
    let mut pos = values
        .iter()
        .position(|v| !is_ws(v))
        .unwrap_or(values.len());
    let result = parse_sum(values, &mut pos)?;
    while values.get(pos).is_some_and(is_ws) {
        pos += 1;
    }
    (pos == values.len()).then_some(result)
}

/// somme := produit ( [ ' + ' | ' - ' ] produit )* — les signes DOIVENT être
/// entourés d'espaces (sinon `-2px` serait ambigu).
fn parse_sum(values: &[ComponentValue], pos: &mut usize) -> Option<Calc> {
    let mut acc = parse_product(values, pos)?;
    loop {
        let start = *pos;
        if !values.get(*pos).is_some_and(is_ws) {
            return Some(acc);
        }
        while values.get(*pos).is_some_and(is_ws) {
            *pos += 1;
        }
        let sign = match values.get(*pos) {
            Some(ComponentValue::Token(Token::Delim('+'))) => 1.0,
            Some(ComponentValue::Token(Token::Delim('-'))) => -1.0,
            _ => {
                *pos = start;
                return Some(acc);
            }
        };
        *pos += 1;
        if !values.get(*pos).is_some_and(is_ws) {
            return None;
        }
        while values.get(*pos).is_some_and(is_ws) {
            *pos += 1;
        }
        let rhs = parse_product(values, pos)?;
        acc = match (acc, rhs) {
            (Calc::Number(a), Calc::Number(b)) => Calc::Number(a + sign * b),
            (Calc::Terms(mut a), Calc::Terms(b)) => {
                for (unit, v) in b.0 {
                    *a.0.entry(unit).or_insert(0.0) += sign * v;
                }
                Calc::Terms(a)
            }
            _ => return None, // `10px + 5` : types incompatibles
        };
    }
}

/// produit := valeur ( [ '*' | '/' ] valeur )*
fn parse_product(values: &[ComponentValue], pos: &mut usize) -> Option<Calc> {
    let mut acc = parse_calc_value(values, pos)?;
    loop {
        let start = *pos;
        while values.get(*pos).is_some_and(is_ws) {
            *pos += 1;
        }
        let op = match values.get(*pos) {
            Some(ComponentValue::Token(Token::Delim(c @ ('*' | '/')))) => *c,
            _ => {
                *pos = start;
                return Some(acc);
            }
        };
        *pos += 1;
        while values.get(*pos).is_some_and(is_ws) {
            *pos += 1;
        }
        let rhs = parse_calc_value(values, pos)?;
        acc = match (op, acc, rhs) {
            ('*', Calc::Number(a), Calc::Number(b)) => Calc::Number(a * b),
            ('*', Calc::Number(n), Calc::Terms(t)) | ('*', Calc::Terms(t), Calc::Number(n)) => {
                Calc::Terms(scale(t, n))
            }
            ('/', Calc::Number(a), Calc::Number(b)) if b != 0.0 => Calc::Number(a / b),
            ('/', Calc::Terms(t), Calc::Number(b)) if b != 0.0 => Calc::Terms(scale(t, 1.0 / b)),
            _ => return None, // longueur * longueur, division par une longueur ou par 0
        };
    }
}

fn scale(t: CalcSum, n: f64) -> CalcSum {
    CalcSum(t.0.into_iter().map(|(u, v)| (u, v * n)).collect())
}

fn parse_calc_value(values: &[ComponentValue], pos: &mut usize) -> Option<Calc> {
    let v = values.get(*pos)?;
    *pos += 1;
    let single =
        |unit: &'static str, value: f64| Calc::Terms(CalcSum(BTreeMap::from([(unit, value)])));
    Some(match v {
        ComponentValue::Token(Token::Number(n)) => Calc::Number(n.value),
        ComponentValue::Token(Token::Percentage(n)) => single("%", n.value),
        ComponentValue::Token(Token::Dimension { number, unit }) => {
            let (unit, to_px) = length_unit(&unit.to_ascii_lowercase())?;
            match to_px {
                // Unités absolues : converties en pixels, comme les navigateurs.
                Some(factor) => single("px", number.value * factor),
                None => single(unit, number.value),
            }
        }
        ComponentValue::Block {
            kind: BlockKind::Paren,
            contents,
        } => parse_calc(contents)?,
        ComponentValue::Function { name, arguments } if name.eq_ignore_ascii_case("calc") => {
            parse_calc(arguments)?
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::format_number;

    #[test]
    fn nombres_extremes() {
        // Trouvé par le fuzzing : `-1e30810px` faisait paniquer (log10 de l'infini).
        assert_eq!(format_number(33.333333), "33.3333");
        assert_eq!(format_number(133.7952755), "133.795");
        assert_eq!(format_number(0.1), "0.1");
        assert_eq!(format_number(-0.0), "0");
        let _ = format_number(f64::MAX);
        let _ = format_number(f64::INFINITY);
        let _ = format_number(f64::NAN);
    }
}
