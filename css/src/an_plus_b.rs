//! La micro-syntaxe `An+B` (spec CSS Syntax Level 3, §6), celle de
//! `:nth-child(2n+1)`.
//!
//! Elle est définie sur les TOKENS, pas sur le texte : le tokenizer découpe
//! `3n-1` en une seule dimension d'unité `n-1`, `n-1` en un seul identifiant,
//! `3n- 1` en une dimension d'unité `n-` suivie d'un nombre... d'où la
//! grammaire à plusieurs cas.

use crate::parser::ComponentValue;
use crate::tokenizer::{Numeric, Token};

/// Un nombre entier (type "integer" de la spec), et s'il est écrit avec un signe.
fn integer(n: &Numeric) -> Option<(i32, bool)> {
    if !n.is_integer {
        return None;
    }
    let signed = n.repr.starts_with(['+', '-']);
    Some((
        n.value.clamp(i32::MIN as f64, i32::MAX as f64) as i32,
        signed,
    ))
}

/// Après le `n` : rien, `-`, ou `-` suivi de chiffres (`n-12`).
enum Tail {
    Empty,
    Dash,
    DashDigits(i32),
}

fn tail(rest: &str) -> Option<Tail> {
    match rest {
        "" => Some(Tail::Empty),
        "-" => Some(Tail::Dash),
        _ => {
            let digits = rest.strip_prefix('-')?;
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                Some(Tail::DashDigits(
                    digits.parse::<i64>().ok()?.min(i32::MAX as i64) as i32,
                ))
            } else {
                None
            }
        }
    }
}

/// Un texte qui commence par `n` ou `N` : ce qui suit.
fn after_n(s: &str) -> Option<&str> {
    s.strip_prefix('n').or_else(|| s.strip_prefix('N'))
}

/// Parse `An+B` et renvoie `(A, B)`, ou `None` si la syntaxe est invalide.
pub fn parse_an_plus_b(values: &[ComponentValue]) -> Option<(i32, i32)> {
    // Les espaces sont permis entre les parties, sauf juste après un `+` initial.
    let mut items: Vec<(&Token, bool)> = Vec::new();
    let mut space_before = false;
    for v in values {
        match v {
            ComponentValue::Token(Token::Whitespace) => space_before = true,
            ComponentValue::Token(t) => {
                items.push((t, space_before));
                space_before = false;
            }
            _ => return None,
        }
    }
    let mut items = items.into_iter().peekable();
    let (first, _) = items.next()?;

    // `odd` et `even` : seuls.
    if let Token::Ident(s) = first {
        let keyword = if s.eq_ignore_ascii_case("odd") {
            Some((2, 1))
        } else if s.eq_ignore_ascii_case("even") {
            Some((2, 0))
        } else {
            None
        };
        if let Some(result) = keyword {
            return items.next().is_none().then_some(result);
        }
    }

    // 1. La partie A (et ce qui suit le `n`).
    let (a, rest) = match first {
        Token::Number(n) => {
            // Un entier seul : B.
            let (b, _) = integer(n)?;
            return items.next().is_none().then_some((0, b));
        }
        Token::Dimension { number, unit } => {
            let (a, _) = integer(number)?;
            (a, tail(after_n(unit)?))
        }
        Token::Ident(s) => match s.strip_prefix('-') {
            Some(rest) => (-1, tail(after_n(rest)?)),
            None => (1, tail(after_n(s)?)),
        },
        Token::Delim('+') => {
            // `+n...` : le `+` doit être collé à un identifiant qui commence par n.
            let (next, space) = items.next()?;
            match next {
                Token::Ident(s) if !space => (1, tail(after_n(s)?)),
                _ => return None,
            }
        }
        _ => return None,
    };

    // 2. La partie B.
    let b = match rest? {
        Tail::DashDigits(d) => -d,
        Tail::Dash => {
            // `3n- 1` : un entier SANS signe doit suivre.
            let (Token::Number(n), _) = items.next()? else {
                return None;
            };
            let (value, signed) = integer(n)?;
            if signed {
                return None;
            }
            -value
        }
        Tail::Empty => match items.next() {
            None => 0,
            // `3n +1` : entier AVEC signe.
            Some((Token::Number(n), _)) => {
                let (value, signed) = integer(n)?;
                if !signed {
                    return None;
                }
                value
            }
            // `3n + 1` : signe séparé, puis entier SANS signe.
            Some((Token::Delim(sign @ ('+' | '-')), _)) => {
                let (Token::Number(n), _) = items.next()? else {
                    return None;
                };
                let (value, signed) = integer(n)?;
                if signed {
                    return None;
                }
                if *sign == '-' { -value } else { value }
            }
            Some(_) => return None,
        },
    };
    items.next().is_none().then_some((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Parser, preprocess};

    fn parse(s: &str) -> Option<(i32, i32)> {
        let css = preprocess(s);
        parse_an_plus_b(&Parser::new(&css).parse_component_value_list())
    }

    #[test]
    fn odd_et_even_sont_seuls() {
        // Pas couvert par css-parsing-tests : un bug de la première version.
        assert_eq!(parse("even"), Some((2, 0)));
        assert_eq!(parse("odd"), Some((2, 1)));
        assert_eq!(parse("even +1"), None);
        assert_eq!(parse("odd 3"), None);
    }
}
