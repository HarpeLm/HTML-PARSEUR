//! Couleurs CSS (specs CSS Color Level 4 et 5) : parsing et sérialisation.
//!
//! `#f00`, `red`, `hsl()` et `hwb()` sont convertis en sRGB ; `lab()`, `lch()`,
//! `oklab()`, `oklch()` et `color()` restent dans leur espace de couleur (comme
//! le demande la spec), avec `none` conservé.

use crate::named_colors::NAMED_COLORS;
use crate::parser::ComponentValue;
use crate::tokenizer::Token;

/// Les espaces "Lab-like" : `lab()`/`lch()` et leurs versions "ok".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabSpace {
    /// CIE Lab.
    Lab,
    /// Oklab.
    Oklab,
}

/// Une couleur CSS. Les canaux `None` valent `none` (composante absente).
#[derive(Debug, Clone, PartialEq)]
pub enum Color {
    /// `currentcolor` : la valeur de la propriété `color` de l'élément.
    CurrentColor,
    /// sRGB, canaux de 0 à 255 (décimaux permis), alpha de 0 à 1.
    Rgb {
        /// Rouge.
        r: f64,
        /// Vert.
        g: f64,
        /// Bleu.
        b: f64,
        /// Opacité.
        alpha: f64,
    },
    /// `lab()` / `oklab()` : luminosité, a, b.
    Lab {
        /// Lab ou Oklab.
        space: LabSpace,
        /// Luminosité, a, b.
        channels: [Option<f64>; 3],
        /// Opacité.
        alpha: Option<f64>,
    },
    /// `lch()` / `oklch()` : luminosité, chroma, teinte (en degrés).
    Lch {
        /// Lab ou Oklab.
        space: LabSpace,
        /// Luminosité, chroma, teinte.
        channels: [Option<f64>; 3],
        /// Opacité.
        alpha: Option<f64>,
    },
    /// `color(espace c1 c2 c3)`, `device-cmyk(...)` et les espaces `--perso`.
    Function {
        /// Le nom de l'espace (`srgb`, `display-p3`, `xyz-d65`, `device-cmyk`...).
        space: String,
        /// Les canaux.
        channels: Vec<Option<f64>>,
        /// Opacité.
        alpha: Option<f64>,
    },
    /// `light-dark(clair, sombre)` : selon le thème de la page.
    LightDark(Box<Color>, Box<Color>),
}

// ───────────── Lecture des arguments ─────────────

/// Une valeur d'argument : nombre, pourcentage, angle (en degrés) ou `none`.
#[derive(Debug, Clone, Copy)]
enum Arg {
    Number(f64),
    Percent(f64),
    Angle(f64),
    None,
}

fn arg(v: &ComponentValue) -> Option<Arg> {
    match v {
        ComponentValue::Token(Token::Number(n)) => Some(Arg::Number(n.value)),
        ComponentValue::Token(Token::Percentage(n)) => Some(Arg::Percent(n.value)),
        ComponentValue::Token(Token::Dimension { number, unit }) => {
            let factor = match unit.to_ascii_lowercase().as_str() {
                "deg" => 1.0,
                "grad" => 360.0 / 400.0,
                "rad" => 180.0 / std::f64::consts::PI,
                "turn" => 360.0,
                _ => return None,
            };
            Some(Arg::Angle(number.value * factor))
        }
        ComponentValue::Token(Token::Ident(s)) if s.eq_ignore_ascii_case("none") => Some(Arg::None),
        _ => None,
    }
}

/// Les arguments d'une fonction de couleur, découpés selon sa syntaxe.
struct Args {
    values: Vec<Arg>,
    alpha: Option<Arg>,
    /// Syntaxe "ancienne", séparée par des virgules : `rgb(1, 2, 3)`.
    legacy: bool,
}

impl Args {
    /// Ancienne syntaxe : au-delà de `channels` valeurs, la suivante est l'alpha
    /// (`rgba(r, g, b, a)` : 3 canaux ; `device-cmyk(c, m, y, k, a)` : 4).
    fn with_legacy_alpha(mut self, channels: usize) -> Self {
        if self.legacy && self.values.len() == channels + 1 {
            self.alpha = self.values.pop();
        }
        self
    }
}

fn is_ws(v: &ComponentValue) -> bool {
    matches!(v, ComponentValue::Token(Token::Whitespace))
}

fn split_args(arguments: &[ComponentValue]) -> Option<Args> {
    let items: Vec<&ComponentValue> = arguments.iter().filter(|v| !is_ws(v)).collect();
    let legacy = items
        .iter()
        .any(|v| matches!(v, ComponentValue::Token(Token::Comma)));
    if legacy {
        // a, b, c[, alpha] : une virgule entre chaque valeur, pas de `none`.
        let mut values = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let is_comma = matches!(item, ComponentValue::Token(Token::Comma));
            if (i % 2 == 1) != is_comma {
                return None;
            }
            if !is_comma {
                let a = arg(item)?;
                if matches!(a, Arg::None) {
                    return None;
                }
                values.push(a);
            }
        }
        if items.len().is_multiple_of(2) {
            return None; // virgule finale
        }
        // Combien de canaux avant l'alpha : chaque fonction le décide (voir `with_legacy_alpha`).
        return Some(Args {
            values,
            alpha: None,
            legacy: true,
        });
    }
    // a b c [/ alpha]
    let slash = items
        .iter()
        .position(|v| matches!(v, ComponentValue::Token(Token::Delim('/'))));
    let (main, alpha) = match slash {
        Some(i) => {
            if items.len() != i + 2 {
                return None;
            }
            (&items[..i], Some(arg(items[i + 1])?))
        }
        None => (&items[..], None),
    };
    let values = main.iter().map(|v| arg(v)).collect::<Option<Vec<_>>>()?;
    Some(Args {
        values,
        alpha,
        legacy: false,
    })
}

/// Alpha : nombre ou pourcentage, borné entre 0 et 1.
fn alpha_value(a: Option<Arg>) -> Option<Option<f64>> {
    match a {
        None => Some(Some(1.0)),
        Some(Arg::Number(n)) => Some(Some(n.clamp(0.0, 1.0))),
        Some(Arg::Percent(p)) => Some(Some((p / 100.0).clamp(0.0, 1.0))),
        Some(Arg::None) => Some(None),
        Some(Arg::Angle(_)) => None,
    }
}

/// Nombre, ou pourcentage converti avec l'échelle `percent_scale` (valeur de 100 %).
fn scaled(a: Arg, percent_scale: f64) -> Option<Option<f64>> {
    match a {
        Arg::Number(n) => Some(Some(n)),
        Arg::Percent(p) => Some(Some(p / 100.0 * percent_scale)),
        Arg::None => Some(None),
        Arg::Angle(_) => None,
    }
}

fn hue(a: Arg) -> Option<Option<f64>> {
    match a {
        Arg::Number(n) | Arg::Angle(n) => Some(Some(n)),
        Arg::None => Some(None),
        Arg::Percent(_) => None,
    }
}

// ───────────── Conversions vers sRGB ─────────────

/// Conversion HSL -> RGB (0..1), comme `colorsys.hls_to_rgb` de Python, qui a
/// servi à générer les tests.
fn hls_to_rgb(h: f64, l: f64, s: f64) -> [f64; 3] {
    if s == 0.0 {
        return [l, l, l];
    }
    let m2 = if l <= 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let m1 = 2.0 * l - m2;
    let v = |hue: f64| {
        let hue = hue.rem_euclid(1.0);
        if hue < 1.0 / 6.0 {
            m1 + (m2 - m1) * hue * 6.0
        } else if hue < 0.5 {
            m2
        } else if hue < 2.0 / 3.0 {
            m1 + (m2 - m1) * (2.0 / 3.0 - hue) * 6.0
        } else {
            m1
        }
    };
    [v(h + 1.0 / 3.0), v(h), v(h - 1.0 / 3.0)]
}

fn rgb_from(channels: [f64; 3], alpha: Option<f64>) -> Color {
    Color::Rgb {
        r: channels[0],
        g: channels[1],
        b: channels[2],
        alpha: alpha.unwrap_or(0.0),
    }
}

// ───────────── Parsing ─────────────

/// Parse une couleur (une seule component value, espaces autour permis).
/// `None` si ce n'est pas une couleur valide.
pub fn parse_color(values: &[ComponentValue]) -> Option<Color> {
    let items: Vec<&ComponentValue> = values.iter().filter(|v| !is_ws(v)).collect();
    let [value] = items[..] else { return None };
    match value {
        ComponentValue::Token(Token::Ident(name)) => keyword(name),
        ComponentValue::Token(Token::Hash { value, .. }) => hex(value),
        ComponentValue::Function { name, arguments } => {
            function(&name.to_ascii_lowercase(), arguments)
        }
        _ => None,
    }
}

fn keyword(name: &str) -> Option<Color> {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "currentcolor" => return Some(Color::CurrentColor),
        "transparent" => {
            return Some(Color::Rgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                alpha: 0.0,
            });
        }
        _ => {}
    }
    let i = NAMED_COLORS
        .binary_search_by(|(n, _)| n.cmp(&lower.as_str()))
        .ok()?;
    let (r, g, b) = NAMED_COLORS[i].1;
    Some(Color::Rgb {
        r: r as f64,
        g: g as f64,
        b: b as f64,
        alpha: 1.0,
    })
}

fn hex(value: &str) -> Option<Color> {
    if !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let digit = |i: usize| u8::from_str_radix(&value[i..i + 1], 16).unwrap() as f64;
    let pair = |i: usize| u8::from_str_radix(&value[i..i + 2], 16).unwrap() as f64;
    let (r, g, b, a) = match value.len() {
        3 => (digit(0) * 17.0, digit(1) * 17.0, digit(2) * 17.0, 255.0),
        4 => (
            digit(0) * 17.0,
            digit(1) * 17.0,
            digit(2) * 17.0,
            digit(3) * 17.0,
        ),
        6 => (pair(0), pair(2), pair(4), 255.0),
        8 => (pair(0), pair(2), pair(4), pair(6)),
        _ => return None,
    };
    Some(Color::Rgb {
        r,
        g,
        b,
        alpha: a / 255.0,
    })
}

fn function(name: &str, arguments: &[ComponentValue]) -> Option<Color> {
    if name == "light-dark" {
        let mut parts = arguments.split(|v| matches!(v, ComponentValue::Token(Token::Comma)));
        let (Some(light), Some(dark), None) = (parts.next(), parts.next(), parts.next()) else {
            return None;
        };
        return Some(Color::LightDark(
            Box::new(parse_color(light)?),
            Box::new(parse_color(dark)?),
        ));
    }
    if name == "color" {
        // Commence par un nom d'espace (`srgb`) : découpage à part.
        return color_function(arguments);
    }
    let args = split_args(arguments)?;
    match name {
        "rgb" | "rgba" => rgb(&args.with_legacy_alpha(3)),
        "hsl" | "hsla" => hsl(&args.with_legacy_alpha(3)),
        "hwb" if !args.legacy => hwb(&args),
        "lab" | "oklab" | "lch" | "oklch" if !args.legacy => lab_like(name, &args),
        "device-cmyk" => device_cmyk(&args.with_legacy_alpha(4)),
        _ => None,
    }
}

fn rgb(args: &Args) -> Option<Color> {
    let [r, g, b] = args.values[..] else {
        return None;
    };
    let channel = |a: Arg| match a {
        Arg::Number(n) => Some(n.clamp(0.0, 255.0)),
        Arg::Percent(p) => Some((p / 100.0 * 255.0).clamp(0.0, 255.0)),
        Arg::None => Some(0.0),
        Arg::Angle(_) => None,
    };
    // Ancienne syntaxe : que des nombres, ou que des pourcentages.
    if args.legacy {
        let percents = [r, g, b]
            .iter()
            .filter(|a| matches!(a, Arg::Percent(_)))
            .count();
        if percents != 0 && percents != 3 {
            return None;
        }
    }
    let alpha = alpha_value(args.alpha)?;
    Some(rgb_from([channel(r)?, channel(g)?, channel(b)?], alpha))
}

fn hsl(args: &Args) -> Option<Color> {
    let [h, s, l] = args.values[..] else {
        return None;
    };
    // Ancienne syntaxe : saturation et luminosité en pourcentages.
    if args.legacy && !(matches!(s, Arg::Percent(_)) && matches!(l, Arg::Percent(_))) {
        return None;
    }
    let percent = |a: Arg| match a {
        Arg::Number(n) | Arg::Percent(n) => Some(n),
        Arg::None => Some(0.0),
        Arg::Angle(_) => None,
    };
    let h = hue(h)?.unwrap_or(0.0);
    let (s, l) = (percent(s)?, percent(l)?);
    let rgb = hls_to_rgb(h / 360.0, l / 100.0, s / 100.0).map(|c| c * 255.0);
    Some(rgb_from(rgb, alpha_value(args.alpha)?))
}

fn hwb(args: &Args) -> Option<Color> {
    let [h, w, b] = args.values[..] else {
        return None;
    };
    let percent = |a: Arg| match a {
        Arg::Number(n) | Arg::Percent(n) => Some(n),
        Arg::None => Some(0.0),
        Arg::Angle(_) => None,
    };
    let h = hue(h)?.unwrap_or(0.0);
    let (w, b) = (percent(w)?, percent(b)?);
    let rgb = if w + b >= 100.0 {
        [w / (w + b) * 255.0; 3]
    } else {
        hls_to_rgb(h / 360.0, 0.5, 1.0).map(|c| (c * (100.0 - w - b) + w) / 100.0 * 255.0)
    };
    Some(rgb_from(rgb, alpha_value(args.alpha)?))
}

fn lab_like(name: &str, args: &Args) -> Option<Color> {
    let [x, y, z] = args.values[..] else {
        return None;
    };
    let ok = name.starts_with("ok");
    let space = if ok { LabSpace::Oklab } else { LabSpace::Lab };
    let lightness_scale = if ok { 1.0 } else { 100.0 };
    let alpha = alpha_value(args.alpha)?;
    if name.ends_with("lab") {
        let ab_scale = if ok { 0.4 } else { 125.0 };
        let channels = [
            scaled(x, lightness_scale)?,
            scaled(y, ab_scale)?,
            scaled(z, ab_scale)?,
        ];
        Some(Color::Lab {
            space,
            channels,
            alpha,
        })
    } else {
        let chroma_scale = if ok { 0.4 } else { 150.0 };
        let h = hue(z)?.map(|h| h.rem_euclid(360.0));
        let channels = [scaled(x, lightness_scale)?, scaled(y, chroma_scale)?, h];
        Some(Color::Lch {
            space,
            channels,
            alpha,
        })
    }
}

fn color_function(arguments: &[ComponentValue]) -> Option<Color> {
    let items: Vec<&ComponentValue> = arguments.iter().filter(|v| !is_ws(v)).collect();
    let Some(ComponentValue::Token(Token::Ident(space))) = items.first() else {
        return None;
    };
    let space = space.to_ascii_lowercase();
    let custom = space.starts_with("--");
    let space = match space.as_str() {
        "srgb" | "srgb-linear" | "display-p3" | "a98-rgb" | "prophoto-rgb" | "rec2020"
        | "xyz-d50" | "xyz-d65" => space,
        "xyz" => "xyz-d65".to_string(),
        _ if custom => space,
        _ => return None,
    };
    let rest: Vec<ComponentValue> = arguments
        .iter()
        .skip_while(|v| !matches!(v, ComponentValue::Token(Token::Ident(_))))
        .skip(1)
        .cloned()
        .collect();
    let args = split_args(&rest)?;
    if args.legacy || (!custom && args.values.len() != 3) || args.values.is_empty() {
        return None;
    }
    let channels = args
        .values
        .iter()
        .map(|&a| scaled(a, 1.0))
        .collect::<Option<Vec<_>>>()?;
    Some(Color::Function {
        space,
        channels,
        alpha: alpha_value(args.alpha)?,
    })
}

fn device_cmyk(args: &Args) -> Option<Color> {
    if args.values.len() != 4 {
        return None;
    }
    // Ancienne syntaxe (virgules) : nombres uniquement.
    if args.legacy && args.values.iter().any(|a| !matches!(a, Arg::Number(_))) {
        return None;
    }
    let channels = args
        .values
        .iter()
        .map(|&a| scaled(a, 1.0).map(|c| c.map(|c| c.clamp(0.0, 1.0))))
        .collect::<Option<Vec<_>>>()?;
    Some(Color::Function {
        space: "device-cmyk".into(),
        channels,
        alpha: alpha_value(args.alpha)?,
    })
}

// ───────────── Sérialisation ─────────────

/// Nombre arrondi à 6 décimales, sans zéros inutiles (`0.5`, `12`, `209.525`).
fn number(v: f64) -> String {
    let rounded = ((v + 0.0000001) * 1_000_000.0).round() / 1_000_000.0;
    let rounded = if rounded == 0.0 { 0.0 } else { rounded }; // pas de "-0"
    format!("{rounded}")
}

fn channel(v: Option<f64>) -> String {
    v.map_or_else(|| "none".to_string(), number)
}

/// ` / alpha`, omis si l'alpha vaut 1.
fn alpha_suffix(alpha: Option<f64>) -> String {
    match alpha {
        Some(a) if a >= 1.0 => String::new(),
        a => format!(" / {}", channel(a)),
    }
}

impl std::fmt::Display for Color {
    /// La forme sérialisée de la couleur (CSS Color 4, §15).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Color::CurrentColor => write!(f, "currentcolor"),
            Color::Rgb { r, g, b, alpha } => {
                let (r, g, b) = (number(*r), number(*g), number(*b));
                if *alpha >= 1.0 {
                    write!(f, "rgb({r}, {g}, {b})")
                } else {
                    write!(f, "rgba({r}, {g}, {b}, {})", number(*alpha))
                }
            }
            Color::Lab {
                space,
                channels,
                alpha,
            }
            | Color::Lch {
                space,
                channels,
                alpha,
            } => {
                let name = match (self, space) {
                    (Color::Lab { .. }, LabSpace::Lab) => "lab",
                    (Color::Lab { .. }, LabSpace::Oklab) => "oklab",
                    (_, LabSpace::Lab) => "lch",
                    (_, LabSpace::Oklab) => "oklch",
                };
                let [x, y, z] = channels.map(channel);
                write!(f, "{name}({x} {y} {z}{})", alpha_suffix(*alpha))
            }
            Color::Function {
                space,
                channels,
                alpha,
            } => {
                let values: Vec<String> = channels.iter().map(|c| channel(*c)).collect();
                write!(
                    f,
                    "color({space} {}{})",
                    values.join(" "),
                    alpha_suffix(*alpha)
                )
            }
            Color::LightDark(light, dark) => write!(f, "light-dark({light}, {dark})"),
        }
    }
}

impl Color {
    /// Forme CSS Color 5 : le sRGB s'écrit `color(srgb r g b)` avec des canaux
    /// de 0 à 1 (c'est ainsi que `light-dark()` sérialise ses couleurs).
    pub fn to_css_color5(&self) -> String {
        match self {
            Color::Rgb { r, g, b, alpha } => format!(
                "color(srgb {} {} {}{})",
                number(r / 255.0),
                number(g / 255.0),
                number(b / 255.0),
                alpha_suffix(Some(*alpha))
            ),
            other => other.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Parser, parse_color, preprocess};

    fn color(s: &str) -> Option<String> {
        let css = preprocess(s);
        parse_color(&Parser::new(&css).parse_component_value_list()).map(|c| c.to_string())
    }

    #[test]
    fn rgb_ancienne_et_nouvelle_syntaxe() {
        // Aucun fichier de css-parsing-tests ne couvre rgb() lui-même.
        assert_eq!(color("rgb(255, 0, 0)").as_deref(), Some("rgb(255, 0, 0)"));
        assert_eq!(
            color("rgba(255, 0, 0, 0.5)").as_deref(),
            Some("rgba(255, 0, 0, 0.5)")
        );
        assert_eq!(
            color("rgb(100%, 50%, 0%)").as_deref(),
            Some("rgb(255, 127.5, 0)")
        );
        assert_eq!(
            color("rgb(255 0 0 / 50%)").as_deref(),
            Some("rgba(255, 0, 0, 0.5)")
        );
        assert_eq!(color("rgb(none 0 0)").as_deref(), Some("rgb(0, 0, 0)"));
        // Bornes : 0..255 et alpha 0..1.
        assert_eq!(color("rgb(300, -5, 0)").as_deref(), Some("rgb(255, 0, 0)"));
        assert_eq!(color("rgb(0 0 0 / 2)").as_deref(), Some("rgb(0, 0, 0)"));
    }

    #[test]
    fn rgb_invalides() {
        // Ancienne syntaxe : pas de mélange nombres / pourcentages, pas de none.
        assert_eq!(color("rgb(255, 50%, 0)"), None);
        assert_eq!(color("rgb(none, 0, 0)"), None);
        assert_eq!(color("rgb(255, 0, 0,)"), None);
        assert_eq!(color("rgb(255 0)"), None);
        assert_eq!(color("rgb(255, 0 0)"), None);
        assert_eq!(color("rgb(1 2 3 / 4 / 5)"), None);
    }

    #[test]
    fn currentcolor() {
        assert_eq!(color("CurrentColor").as_deref(), Some("currentcolor"));
    }
}
