//! La mise en page des blocs (CSS 2, §10.3.3 et §10.6.3) : largeurs, hauteurs,
//! empilement vertical, et fusion des marges (§8.3.1).
//!
//! La fusion des marges est la règle la plus délicate : des marges verticales
//! qui se touchent ne s'additionnent pas, elles fusionnent (la plus grande
//! positive plus la plus négative). Elles se touchent entre deux blocs voisins,
//! entre un bloc et son premier (ou dernier) enfant quand rien ne les sépare
//! (bordure, retrait, nouveau contexte de formatage), et à travers un bloc vide.
//! Chaque bloc renvoie donc à son parent les marges qui « s'échappent » par le
//! haut et par le bas ; c'est le parent qui décide où elles s'appliquent.

use lumen_style::{Computed, ComputedStyle, Styles};

use crate::{BoxKind, Edges, LayoutBox};

/// Des marges qui fusionnent : la plus grande positive plus la plus négative.
#[derive(Debug, Clone, Copy, Default)]
struct CollapsedMargin {
    positive: f64,
    negative: f64,
}

impl CollapsedMargin {
    fn new(margin: f64) -> Self {
        let mut m = CollapsedMargin::default();
        m.add(margin);
        m
    }

    fn add(&mut self, margin: f64) {
        if margin > 0.0 {
            self.positive = self.positive.max(margin);
        } else {
            self.negative = self.negative.min(margin);
        }
    }

    fn merge(&mut self, other: CollapsedMargin) {
        self.positive = self.positive.max(other.positive);
        self.negative = self.negative.min(other.negative);
    }

    fn value(self) -> f64 {
        self.positive + self.negative
    }
}

/// Ce que la mise en page d'un bloc apprend à son parent.
pub(crate) struct Laid {
    /// La hauteur de sa boîte de bordure.
    pub height: f64,
    /// Sa marge du haut, fusionnée avec celles qui s'échappent de l'intérieur.
    top: CollapsedMargin,
    /// Sa marge du bas, idem.
    bottom: CollapsedMargin,
    /// Bloc vide que les marges traversent : ses marges du haut et du bas
    /// fusionnent entre elles.
    through: bool,
}

/// Le bloc conteneur : sa largeur, et sa hauteur si elle est connue d'avance
/// (pour les hauteurs en `%`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Containing {
    pub width: f64,
    pub height: Option<f64>,
}

/// Une longueur calculée, résolue par rapport à `basis` (les `%`). `None` pour
/// `auto`, `none`, `min-content`...
fn resolve(value: Option<&Computed>, basis: f64) -> Option<f64> {
    match value? {
        Computed::Px(px) => Some(*px),
        Computed::Percentage(p) => Some(basis * p / 100.0),
        Computed::Calc { percent, px } => Some(basis * percent / 100.0 + px),
        _ => None,
    }
}

/// Comme `resolve`, mais un `%` sans base connue (hauteur du parent `auto`)
/// vaut `auto`.
fn resolve_height(value: Option<&Computed>, basis: Option<f64>) -> Option<f64> {
    match value? {
        Computed::Px(px) => Some(*px),
        Computed::Percentage(p) => basis.map(|b| b * p / 100.0),
        Computed::Calc { percent, px } => basis.map(|b| b * percent / 100.0 + px),
        _ => None,
    }
}

/// Le bloc établit-il un nouveau contexte de formatage de bloc ? Ses marges ne
/// fusionnent alors pas avec celles de ses enfants.
fn establishes_bfc(style: Option<&ComputedStyle>, is_root: bool) -> bool {
    let Some(style) = style else { return false };
    let keyword = |name| match style.get(name) {
        Some(Computed::Keyword(k)) => *k,
        _ => "",
    };
    is_root
        || !matches!(keyword("overflow-x"), "visible" | "clip")
        || matches!(
            keyword("display"),
            "flow-root" | "flex" | "grid" | "table" | "block math"
        )
}

/// Met en page un bloc et ses descendants. Les tailles et marges sont rangées
/// dans la boîte ; la position de chaque enfant est relative à la boîte de
/// contenu de son parent (fixée ensuite par `place`).
pub(crate) fn layout_block(
    b: &mut LayoutBox,
    style: Option<&ComputedStyle>,
    styles: &Styles,
    cb: Containing,
    is_root: bool,
) -> Laid {
    let get = |name: &str| style.and_then(|s| s.get(name));
    let length = |name: &str| resolve(get(name), cb.width).unwrap_or(0.0);

    // Retraits et bordures (les `%` verticaux aussi se rapportent à la largeur).
    let padding = Edges {
        top: length("padding-top"),
        right: length("padding-right"),
        bottom: length("padding-bottom"),
        left: length("padding-left"),
    };
    let border = Edges {
        top: length("border-top-width"),
        right: length("border-right-width"),
        bottom: length("border-bottom-width"),
        left: length("border-left-width"),
    };
    let h_extra = padding.left + padding.right + border.left + border.right;
    let v_extra = padding.top + padding.bottom + border.top + border.bottom;
    let border_box = matches!(get("box-sizing"), Some(Computed::Keyword("border-box")));
    let content_w = |w: f64| {
        if border_box {
            (w - h_extra).max(0.0)
        } else {
            w
        }
    };
    let content_h = |h: f64| {
        if border_box {
            (h - v_extra).max(0.0)
        } else {
            h
        }
    };

    // Largeur (§10.3.3). Une marge `auto` vaut `None` ; une boîte anonyme n'a
    // ni marge ni style.
    let margin = |name: &str| match style {
        Some(_) => resolve(get(name), cb.width),
        None => Some(0.0),
    };
    let (margin_left, margin_right) = (margin("margin-left"), margin("margin-right"));
    let solve = |width: Option<f64>| -> (f64, f64, f64) {
        match width {
            None => {
                let (ml, mr) = (margin_left.unwrap_or(0.0), margin_right.unwrap_or(0.0));
                ((cb.width - ml - mr - h_extra).max(0.0), ml, mr)
            }
            Some(w) => {
                let fixed = w + h_extra;
                let known = fixed + margin_left.unwrap_or(0.0) + margin_right.unwrap_or(0.0);
                let (ml, mr) = if known > cb.width {
                    // Trop large : les `auto` valent 0, et la marge droite cède.
                    let ml = margin_left.unwrap_or(0.0);
                    (ml, cb.width - fixed - ml)
                } else {
                    match (margin_left, margin_right) {
                        (None, None) => ((cb.width - fixed) / 2.0, (cb.width - fixed) / 2.0),
                        (None, Some(r)) => (cb.width - fixed - r, r),
                        (Some(l), _) => (l, cb.width - fixed - l),
                    }
                };
                (w, ml, mr)
            }
        }
    };
    let width = resolve(get("width"), cb.width).map(content_w);
    let min_width = resolve(get("min-width"), cb.width).map_or(0.0, content_w);
    let max_width = resolve(get("max-width"), cb.width).map_or(f64::INFINITY, content_w);
    let (mut w, mut ml, mut mr) = solve(width);
    if w > max_width {
        (w, ml, mr) = solve(Some(max_width));
    }
    if w < min_width {
        (w, ml, mr) = solve(Some(min_width));
    }

    // Hauteur demandée (§10.6.3) ; `None` : `auto`, calculée d'après le contenu.
    let height = resolve_height(get("height"), cb.height).map(content_h);
    let min_height = resolve_height(get("min-height"), cb.height).map_or(0.0, content_h);
    let max_height = resolve_height(get("max-height"), cb.height).map_or(f64::INFINITY, content_h);
    let margin_top = resolve(get("margin-top"), cb.width).unwrap_or(0.0);
    let margin_bottom = resolve(get("margin-bottom"), cb.width).unwrap_or(0.0);

    // Les marges des enfants peuvent-elles s'échapper par le haut ou le bas ?
    let bfc = establishes_bfc(style, is_root);
    let top_open = !bfc && border.top == 0.0 && padding.top == 0.0;
    let bottom_open = !bfc && border.bottom == 0.0 && padding.bottom == 0.0 && height.is_none();

    // Les enfants.
    let child_cb = Containing {
        width: w,
        height: height.map(|h| h.min(max_height).max(min_height)),
    };
    let mut top = CollapsedMargin::new(margin_top);
    let mut pending = CollapsedMargin::default();
    let mut cursor = 0.0;
    let mut at_start = true;
    let inline_content =
        !b.children.is_empty() && b.children.iter().all(|c| c.kind != BoxKind::Block);
    if inline_content {
        // Contenu en ligne (texte...) : pas encore mis en page (il faut des
        // polices). Il compte comme du contenu, de hauteur nulle.
        at_start = false;
    } else {
        for child in &mut b.children {
            let child_style = child.node.and_then(|n| styles.get(n));
            let r = layout_block(child, child_style, styles, child_cb, false);
            child.offset.0 = child.margin.left;
            if at_start && top_open {
                // Rien encore au-dessus : ses marges rejoignent la nôtre.
                top.merge(r.top);
                child.offset.1 = 0.0;
                if r.through {
                    top.merge(r.bottom);
                } else {
                    cursor = r.height;
                    pending = r.bottom;
                    at_start = false;
                }
            } else {
                let mut m = pending;
                m.merge(r.top);
                child.offset.1 = cursor + m.value();
                if r.through {
                    // Sa position est celle qu'il aurait avec une bordure basse
                    // (§8.3.1) ; ses marges continuent de fusionner avec la suite.
                    m.merge(r.bottom);
                    pending = m;
                } else {
                    cursor = child.offset.1 + r.height;
                    pending = r.bottom;
                    at_start = false;
                }
            }
        }
    }

    // La hauteur, et la marge qui s'échappe par le bas.
    let (auto_height, mut bottom) = if bottom_open {
        let mut m = pending;
        m.add(margin_bottom);
        (cursor, m)
    } else {
        (
            cursor + pending.value(),
            CollapsedMargin::new(margin_bottom),
        )
    };
    let h = height
        .unwrap_or(auto_height)
        .min(max_height)
        .max(min_height);
    if bottom_open && h != auto_height {
        // `min-height` ou `max-height` a changé la hauteur : comme Chromium, la
        // marge du dernier enfant ne s'échappe plus, et elle n'est pas comptée.
        bottom = CollapsedMargin::new(margin_bottom);
    }
    let through = top_open
        && border.bottom == 0.0
        && padding.bottom == 0.0
        && at_start
        && height.is_none_or(|h| h == 0.0)
        && min_height == 0.0;

    b.margin = Edges {
        top: margin_top,
        right: mr,
        bottom: margin_bottom,
        left: ml,
    };
    b.border = border;
    b.padding = padding;
    b.content.width = w;
    b.content.height = h;
    Laid {
        height: h + v_extra,
        top,
        bottom,
        through,
    }
}

/// Fixe les positions absolues : `x`, `y` est le coin de la boîte de bordure.
pub(crate) fn place(b: &mut LayoutBox, x: f64, y: f64) {
    b.content.x = x + b.border.left + b.padding.left;
    b.content.y = y + b.border.top + b.padding.top;
    let (cx, cy) = (b.content.x, b.content.y);
    for child in &mut b.children {
        let (dx, dy) = child.offset;
        place(child, cx + dx, cy + dy);
    }
}
