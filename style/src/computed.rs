//! Les valeurs calculées : ce qui reste d'une valeur une fois résolues les
//! unités relatives (`2em` -> `32px`), l'héritage et les mots-clés.
//!
//! Les règles de calcul suivent Chromium (vérifiées par tests/oracle_cascade.rs),
//! y compris ses particularités : la taille des polices `monospace` (13px au lieu
//! de 16px) et la façon dont `getComputedStyle` écrit `line-height`.

use std::fmt;
use std::rc::Rc;

use lumen_css::properties::{SpecifiedValue, initial_value, is_inherited};
use lumen_css::values::{LengthPercentage, format_number};
use lumen_css::variables::CustomProperties;

/// Les propriétés longues que Lumen calcule, dans l'ordre de calcul
/// (`font-family` puis `font-size` d'abord : les `em` en dépendent).
pub const PROPERTIES: &[&str] = &[
    "font-family",
    "font-size",
    "font-weight",
    "line-height",
    "display",
    "position",
    "float",
    "box-sizing",
    "visibility",
    "opacity",
    "z-index",
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

const FONT_FAMILY: usize = 0;
const FONT_SIZE: usize = 1;

/// L'indice d'une propriété dans [`PROPERTIES`].
pub fn property_index(name: &str) -> Option<usize> {
    PROPERTIES.iter().position(|p| *p == name)
}

/// Lumen sait calculer cette propriété (longue ou raccourci).
pub fn is_supported(name: &str) -> bool {
    property_index(name).is_some() || matches!(name, "margin" | "padding" | "font")
}

/// Les propriétés longues qu'une déclaration de `name` définit.
pub fn longhands(name: &str) -> Vec<&'static str> {
    let sides = |i: usize| PROPERTIES[i..i + 4].to_vec();
    match name {
        "margin" => sides(property_index("margin-top").unwrap()),
        "padding" => sides(property_index("padding-top").unwrap()),
        "font" => PROPERTIES[..4].to_vec(),
        _ => property_index(name)
            .map(|i| vec![PROPERTIES[i]])
            .unwrap_or_default(),
    }
}

/// Une valeur calculée.
#[derive(Debug, Clone, PartialEq)]
pub enum Computed {
    /// `auto`, `block`, `normal`...
    Keyword(&'static str),
    /// Une longueur absolue, en px.
    Px(f64),
    /// Un pourcentage (résolu plus tard, à la mise en page).
    Percentage(f64),
    /// `calc(10% + 5px)`.
    Calc {
        /// La partie en pourcentage.
        percent: f64,
        /// La partie en px.
        px: f64,
    },
    /// Un nombre (`opacity`, `font-weight`, `line-height: 1.5`).
    Number(f64),
    /// Un entier (`z-index`).
    Integer(i32),
    /// Les familles de polices.
    FontFamily(Rc<Vec<String>>),
}

impl fmt::Display for Computed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Computed::Keyword(k) => write!(f, "{k}"),
            Computed::Px(v) => write!(f, "{}px", format_number(*v)),
            Computed::Percentage(p) => write!(f, "{}%", format_number(*p)),
            Computed::Calc { percent, px } => {
                let sign = if *px < 0.0 { '-' } else { '+' };
                let (p, x) = (format_number(*percent), format_number(px.abs()));
                write!(f, "calc({p}% {sign} {x}px)")
            }
            Computed::Number(n) => write!(f, "{}", format_number(*n)),
            Computed::Integer(i) => write!(f, "{i}"),
            Computed::FontFamily(list) => write!(f, "{}", list.join(", ")),
        }
    }
}

/// La taille de police, avec le mot-clé dont elle vient : Chromium le garde pour
/// recalculer la taille quand la police passe à `monospace` ou en revient.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontSize {
    /// La taille, en px.
    pub px: f64,
    /// Le mot-clé (indice dans la table : 0 = `xx-small`... 7 = `xxx-large`) et
    /// le multiplicateur appliqué depuis (`1.5em` d'un `medium` : (3, 1.5)).
    pub keyword: Option<(usize, f64)>,
}

const FONT_KEYWORDS: &[&str] = &[
    "xx-small",
    "x-small",
    "small",
    "medium",
    "large",
    "x-large",
    "xx-large",
    "xxx-large",
];
/// Les tailles des mots-clés dans Chromium, pour une police par défaut de 16px
/// et pour `monospace` (13px).
const KEYWORD_PX: [f64; 8] = [9.0, 10.0, 13.0, 16.0, 18.0, 24.0, 32.0, 48.0];
const KEYWORD_PX_MONOSPACE: [f64; 8] = [9.0, 10.0, 12.0, 13.0, 16.0, 20.0, 26.0, 39.0];

fn keyword_px(keyword: usize, monospace: bool) -> f64 {
    if monospace {
        KEYWORD_PX_MONOSPACE[keyword]
    } else {
        KEYWORD_PX[keyword]
    }
}

impl FontSize {
    /// `medium` : la taille initiale.
    pub const MEDIUM: FontSize = FontSize {
        px: 16.0,
        keyword: Some((3, 1.0)),
    };

    /// La taille héritée par un enfant, selon que sa police est `monospace`.
    fn for_child(self, monospace: bool) -> FontSize {
        match self.keyword {
            Some((k, m)) => FontSize {
                px: keyword_px(k, monospace) * m,
                keyword: self.keyword,
            },
            None => self,
        }
    }

    fn scaled(self, factor: f64) -> FontSize {
        FontSize {
            px: self.px * factor,
            keyword: self.keyword.map(|(k, m)| (k, m * factor)),
        }
    }
}

/// Le style calculé d'un élément.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    values: Vec<Computed>,
    /// La taille de police (déjà dans les valeurs ; gardée à part avec son mot-clé).
    pub font_size: FontSize,
    /// La police est exactement `monospace`.
    pub monospace: bool,
    /// Les propriétés personnalisées.
    pub custom: Rc<CustomProperties>,
}

impl ComputedStyle {
    /// La valeur calculée d'une propriété longue.
    pub fn get(&self, name: &str) -> Option<&Computed> {
        property_index(name).map(|i| &self.values[i])
    }

    /// La valeur telle que `getComputedStyle()` l'écrit, pour les propriétés
    /// dont cette « valeur résolue » ne dépend pas de la mise en page.
    /// `line-height: 1.5` s'écrit en px (1,5 × taille de police).
    pub fn resolved(&self, name: &str) -> Option<String> {
        if let Some(custom) = name.strip_prefix("--").map(|_| name) {
            return Some(self.custom.get(custom).unwrap_or("").to_string());
        }
        let value = self.get(name)?;
        Some(match (name, value) {
            ("line-height", Computed::Number(n)) => Computed::Px(n * self.font_size.px).to_string(),
            _ => value.to_string(),
        })
    }

    /// Le style d'un élément sans aucune déclaration, sous `parent` (ou à la racine).
    pub fn initial(parent: Option<&ComputedStyle>, ctx: &Context) -> ComputedStyle {
        compute(
            &vec![Cascaded::None; PROPERTIES.len()],
            parent,
            Rc::default(),
            ctx,
        )
    }
}

/// Ce que la cascade a retenu pour une propriété longue.
#[derive(Debug, Clone, PartialEq)]
pub enum Cascaded {
    /// Aucune déclaration : héritage ou valeur initiale.
    None,
    /// Une valeur.
    Value(SpecifiedValue),
    /// `initial`.
    Initial,
    /// `inherit`.
    Inherit,
    /// `unset` (et, pour l'instant, `revert` / `revert-layer`).
    Unset,
}

/// Le contexte de calcul : la fenêtre et la racine du document.
#[derive(Debug, Clone, Copy)]
pub struct Context {
    /// La largeur de la fenêtre, en px (pour `vw`).
    pub viewport_width: f64,
    /// La hauteur de la fenêtre, en px (pour `vh`).
    pub viewport_height: f64,
    /// La taille de police de l'élément racine (pour `rem`).
    pub root_font_size: f64,
    /// L'élément est la racine du document (`<html>`).
    pub is_root: bool,
    /// Le parent est un conteneur flex ou grid (ses enfants deviennent des blocs).
    pub parent_is_flex_or_grid: bool,
}

/// Une longueur en px. `em_base` : la taille de police de référence.
fn length_px(value: f64, unit: &str, em_base: f64, ctx: &Context) -> f64 {
    let (w, h) = (ctx.viewport_width / 100.0, ctx.viewport_height / 100.0);
    value
        * match unit {
            "px" => 1.0,
            "in" => 96.0,
            "cm" => 96.0 / 2.54,
            "mm" => 96.0 / 25.4,
            "q" => 96.0 / 101.6,
            "pt" => 96.0 / 72.0,
            "pc" => 16.0,
            "em" => em_base,
            "rem" => ctx.root_font_size,
            // Sans police chargée : approximations usuelles.
            "ex" | "ch" | "cap" => em_base / 2.0,
            "rex" | "rch" => ctx.root_font_size / 2.0,
            "ic" => em_base,
            "lh" => em_base * 1.2,
            "rlh" => ctx.root_font_size * 1.2,
            "vw" | "svw" | "lvw" | "dvw" | "vi" => w,
            "vh" | "svh" | "lvh" | "dvh" | "vb" => h,
            "vmin" => w.min(h),
            "vmax" => w.max(h),
            _ => 0.0,
        }
}

/// `<length-percentage>` calculé : les pourcentages restent des pourcentages.
fn length_percentage(lp: &LengthPercentage, em_base: f64, ctx: &Context) -> Computed {
    match lp {
        LengthPercentage::Length(l) => Computed::Px(length_px(l.value, l.unit, em_base, ctx)),
        LengthPercentage::Percentage(p) => Computed::Percentage(*p),
        LengthPercentage::Calc(sum) => {
            let px = sum
                .0
                .iter()
                .filter(|(u, _)| **u != "%")
                .map(|(u, v)| length_px(*v, u, em_base, ctx))
                .sum();
            match sum.0.get("%") {
                Some(&percent) => Computed::Calc { percent, px },
                None => Computed::Px(px),
            }
        }
    }
}

fn font_size(cascaded: &Cascaded, parent: FontSize, monospace: bool, ctx: &Context) -> FontSize {
    let inherited = parent.for_child(monospace);
    let value = match cascaded {
        Cascaded::None | Cascaded::Inherit | Cascaded::Unset => return inherited,
        Cascaded::Initial => return FontSize::MEDIUM.for_child(monospace),
        Cascaded::Value(v) => v,
    };
    let base = inherited.px;
    let absolute = |px: f64| FontSize {
        px: px.max(0.0),
        keyword: None,
    };
    match value {
        SpecifiedValue::Keyword("larger") => inherited.scaled(1.2),
        SpecifiedValue::Keyword("smaller") => inherited.scaled(1.0 / 1.2),
        SpecifiedValue::Keyword(k) => match FONT_KEYWORDS.iter().position(|x| x == k) {
            Some(i) => FontSize {
                px: keyword_px(i, monospace),
                keyword: Some((i, 1.0)),
            },
            None => inherited,
        },
        SpecifiedValue::LengthPercentage(LengthPercentage::Length(l)) if l.unit == "em" => {
            inherited.scaled(l.value)
        }
        SpecifiedValue::LengthPercentage(LengthPercentage::Percentage(p)) => {
            inherited.scaled(p / 100.0)
        }
        SpecifiedValue::LengthPercentage(LengthPercentage::Length(l)) => {
            absolute(length_px(l.value, l.unit, base, ctx))
        }
        SpecifiedValue::LengthPercentage(LengthPercentage::Calc(sum)) => absolute(
            sum.0
                .iter()
                .map(|(u, v)| match *u {
                    "%" => base * v / 100.0,
                    unit => length_px(*v, unit, base, ctx),
                })
                .sum(),
        ),
        _ => inherited,
    }
}

/// `bolder` / `lighter` (tables de CSS Fonts 4).
fn relative_weight(parent: f64, bolder: bool) -> f64 {
    if bolder {
        match parent {
            p if p < 350.0 => 400.0,
            p if p < 550.0 => 700.0,
            p => p.max(900.0),
        }
    } else {
        match parent {
            p if p < 100.0 => p,
            p if p < 550.0 => 100.0,
            p if p < 750.0 => 400.0,
            _ => 700.0,
        }
    }
}

/// `display` d'un élément qui flotte, est positionné, ou est la racine :
/// il devient un bloc (CSS Display, « blockification »).
fn blockify(display: &'static str) -> &'static str {
    match display {
        "inline" | "inline-block" | "run-in" | "ruby" | "ruby-text" | "table-row-group"
        | "table-header-group" | "table-footer-group" | "table-row" | "table-cell"
        | "table-column-group" | "table-column" | "table-caption" => "block",
        "inline-flex" => "flex",
        "inline-grid" => "grid",
        "inline-table" => "table",
        "math" => "block math",
        other => other,
    }
}

/// Calcule le style d'un élément à partir de la valeur retenue par la cascade
/// pour chaque propriété de [`PROPERTIES`].
pub fn compute(
    cascaded: &[Cascaded],
    parent: Option<&ComputedStyle>,
    custom: Rc<CustomProperties>,
    ctx: &Context,
) -> ComputedStyle {
    let root_initial;
    let parent = match parent {
        Some(p) => p,
        None => {
            root_initial = initial_root();
            &root_initial
        }
    };
    let mut values: Vec<Computed> = Vec::with_capacity(PROPERTIES.len());

    // 1. La police : famille, puis taille (dont dépendent les `em`).
    let family = match &cascaded[FONT_FAMILY] {
        Cascaded::Value(SpecifiedValue::FontFamily(list)) => {
            Computed::FontFamily(Rc::new(list.clone()))
        }
        Cascaded::Initial => Computed::FontFamily(Rc::new(vec!["serif".into()])),
        _ => parent.values[FONT_FAMILY].clone(),
    };
    let monospace = matches!(&family, Computed::FontFamily(list) if list.len() == 1 && list[0].eq_ignore_ascii_case("monospace"));
    values.push(family);
    let size = font_size(&cascaded[FONT_SIZE], parent.font_size, monospace, ctx);
    values.push(Computed::Px(size.px));
    let em = size.px;

    // 2. Les autres propriétés.
    for (i, name) in PROPERTIES.iter().enumerate().skip(2) {
        let inherit = || parent.values[i].clone();
        let specified = match &cascaded[i] {
            Cascaded::Value(v) => v.clone(),
            Cascaded::Inherit => {
                values.push(inherit());
                continue;
            }
            Cascaded::None | Cascaded::Unset if is_inherited(name) => {
                values.push(inherit());
                continue;
            }
            _ => initial_value(name).expect("propriété sans valeur initiale"),
        };
        let computed = match (*name, &specified) {
            ("font-weight", SpecifiedValue::Keyword(k)) => {
                let parent_weight = match parent.values[i] {
                    Computed::Number(n) => n,
                    _ => 400.0,
                };
                Computed::Number(match *k {
                    "bold" => 700.0,
                    "bolder" => relative_weight(parent_weight, true),
                    "lighter" => relative_weight(parent_weight, false),
                    _ => 400.0,
                })
            }
            ("line-height", SpecifiedValue::LengthPercentage(LengthPercentage::Percentage(p))) => {
                Computed::Px(em * p / 100.0)
            }
            ("line-height", SpecifiedValue::LengthPercentage(LengthPercentage::Calc(sum))) => {
                Computed::Px(
                    sum.0
                        .iter()
                        .map(|(u, v)| match *u {
                            "%" => em * v / 100.0,
                            unit => length_px(*v, unit, em, ctx),
                        })
                        .sum(),
                )
            }
            ("opacity", SpecifiedValue::Number(n)) => Computed::Number(n.clamp(0.0, 1.0)),
            (_, SpecifiedValue::Keyword(k)) => Computed::Keyword(k),
            (_, SpecifiedValue::LengthPercentage(lp)) => length_percentage(lp, em, ctx),
            (_, SpecifiedValue::Number(n)) => Computed::Number(*n),
            (_, SpecifiedValue::Integer(n)) => Computed::Integer(*n),
            (_, SpecifiedValue::FontFamily(list)) => Computed::FontFamily(Rc::new(list.clone())),
        };
        values.push(computed);
    }

    // 3. Les corrections entre propriétés.
    let index = |name| property_index(name).unwrap();
    let position = values[index("position")].clone();
    let positioned = matches!(position, Computed::Keyword("absolute" | "fixed"));
    if positioned {
        values[index("float")] = Computed::Keyword("none");
    }
    let floats = values[index("float")] != Computed::Keyword("none");
    if (positioned || floats || ctx.is_root || ctx.parent_is_flex_or_grid)
        && let Computed::Keyword(d) = values[index("display")]
    {
        values[index("display")] = Computed::Keyword(blockify(d));
    }

    ComputedStyle {
        values,
        font_size: size,
        monospace,
        custom,
    }
}

/// Le « parent » de la racine : toutes les valeurs initiales.
fn initial_root() -> ComputedStyle {
    let values = PROPERTIES
        .iter()
        .map(|name| match initial_value(name).unwrap() {
            SpecifiedValue::Keyword("medium") => Computed::Px(16.0),
            SpecifiedValue::Keyword("normal") if *name == "font-weight" => Computed::Number(400.0),
            SpecifiedValue::Keyword(k) => Computed::Keyword(k),
            SpecifiedValue::LengthPercentage(LengthPercentage::Length(l)) => Computed::Px(l.value),
            SpecifiedValue::Number(n) => Computed::Number(n),
            SpecifiedValue::FontFamily(list) => Computed::FontFamily(Rc::new(list)),
            other => unreachable!("valeur initiale inattendue : {other:?}"),
        })
        .collect();
    ComputedStyle {
        values,
        font_size: FontSize::MEDIUM,
        monospace: false,
        custom: Rc::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graisses_relatives() {
        let bolder: Vec<f64> = [100.0, 400.0, 600.0, 900.0]
            .iter()
            .map(|w| relative_weight(*w, true))
            .collect();
        assert_eq!(bolder, [400.0, 700.0, 900.0, 900.0]);
        // Les bornes des tables.
        assert_eq!(relative_weight(349.0, true), 400.0);
        assert_eq!(relative_weight(350.0, true), 700.0);
        assert_eq!(relative_weight(550.0, true), 900.0);
        assert_eq!(relative_weight(549.0, false), 100.0);
        assert_eq!(relative_weight(550.0, false), 400.0);
        assert_eq!(relative_weight(750.0, false), 700.0);
        let lighter: Vec<f64> = [100.0, 400.0, 600.0, 900.0]
            .iter()
            .map(|w| relative_weight(*w, false))
            .collect();
        assert_eq!(lighter, [100.0, 100.0, 400.0, 700.0]);
    }
}
