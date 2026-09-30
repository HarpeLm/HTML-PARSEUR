//! La mise en lignes du contenu en ligne (CSS 2 §10.8, CSS Text).
//!
//! 1. Les espaces fusionnent : une suite d'espaces, tabulations et retours à la
//!    ligne ne compte que pour une espace, même à cheval sur plusieurs éléments ;
//!    elles disparaissent en début et en fin de ligne (`white-space: normal`).
//! 2. Le texte est coupé en mots ; on ne peut passer à la ligne qu'à une espace.
//! 3. Les lignes sont remplies une à une, tant que le mot suivant tient.
//! 4. La hauteur d'une ligne : chaque boîte en ligne présente (et la « jambe de
//!    force » du bloc, sa police à lui) occupe sa hauteur de police plus la
//!    moitié de l'interligne au-dessus et au-dessous, alignée sur la ligne de
//!    base ; la ligne va du plus haut au plus bas.
//!
//! Comme Chromium, les mesures sont faites en pixels d'écran (taille × densité),
//! puis ramenées en px CSS : c'est ce qui donne ses hauteurs de ligne en
//! demi-pixels sur un écran Retina.

use std::rc::Rc;

use html_parseur::dom::NodeId;
use lumen_font::{Font, LineMetrics};
use lumen_style::{Computed, ComputedStyle};

use crate::tree::is_collapsible_space;
use crate::{BoxKind, Ctx, LayoutBox, Rect};

/// Une police à une taille, mesurée comme dans Chromium : en pixels d'écran.
pub(crate) struct SizedFont {
    font: Rc<Font>,
    /// La taille en pixels d'écran, coupée à deux décimales (comme Chromium).
    device_size: f64,
    dpr: f64,
}

impl SizedFont {
    pub(crate) fn new(font: Rc<Font>, css_size: f64, dpr: f64) -> SizedFont {
        let device_size = (css_size * dpr * 100.0 + 1e-6).floor() / 100.0;
        SizedFont {
            font,
            device_size,
            dpr,
        }
    }

    /// L'avance de chaque caractère d'un texte, crénage compris, en px CSS.
    fn advances(&self, text: &str) -> Vec<f64> {
        let scale = self.device_size / self.font.units_per_em as f64 / self.dpr;
        self.font
            .kerned_advances(text)
            .into_iter()
            .map(|a| a as f64 * scale)
            .collect()
    }

    /// Métriques de ligne, arrondies au pixel d'écran, en px CSS.
    fn metrics(&self) -> LineMetrics {
        let m = self.font.line_metrics(self.device_size);
        LineMetrics {
            ascent: m.ascent / self.dpr,
            descent: m.descent / self.dpr,
            line_gap: m.line_gap / self.dpr,
        }
    }
}

/// Une boîte en ligne : un élément (`<span>`), ou la boîte racine du bloc.
struct InlineBox {
    node: Option<NodeId>,
    parent: Option<usize>,
    metrics: LineMetrics,
    line_height: f64,
    /// Par ligne où elle apparaît : (début, fin) horizontaux.
    extents: Vec<(usize, f64, f64)>,
}

enum Piece {
    Word,
    Space,
}

struct Item {
    piece: Piece,
    owner: usize,
    text_node: NodeId,
    width: f64,
}

/// La hauteur de ligne d'une boîte (`line-height`).
fn line_height(style: &ComputedStyle, m: &LineMetrics) -> f64 {
    match style.get("line-height") {
        Some(Computed::Number(n)) => n * style.font_size.px,
        Some(Computed::Px(px)) => *px,
        _ => m.ascent + m.descent + m.line_gap,
    }
}

/// La place d'une boîte au-dessus et au-dessous de la ligne de base : sa police
/// plus la moitié de l'interligne. Comme Chromium (LayoutNG), en pixels d'écran :
/// la hauteur de ligne est arrondie au 64e de pixel, la moitié du haut arrondie
/// vers le bas au pixel entier, et le reste va dessous.
fn leading_split(bx: &InlineBox, dpr: f64) -> (f64, f64) {
    let m = bx.metrics;
    let (ascent, descent) = (m.ascent * dpr, m.descent * dpr);
    let line_height = (bx.line_height * dpr * 64.0).round() / 64.0;
    let half_leading = ((line_height - (ascent + descent)) / 2.0).floor();
    let above = ascent + half_leading;
    (above / dpr, (line_height - above) / dpr)
}

/// Arrondit une largeur vers le haut au 64e de pixel d'écran (`LayoutUnit`).
fn ceil_layout_unit(width: f64, dpr: f64) -> f64 {
    ((width * dpr * 64.0) - 1e-6).ceil() / 64.0 / dpr
}

struct Collector<'c, 'a> {
    ctx: &'c Ctx<'a>,
    boxes: Vec<InlineBox>,
    fonts: Vec<SizedFont>,
    items: Vec<Item>,
    after_space: bool,
}

impl Collector<'_, '_> {
    fn add_box(
        &mut self,
        node: Option<NodeId>,
        parent: Option<usize>,
        style: &ComputedStyle,
    ) -> usize {
        let font = self.ctx.font_for(style);
        let metrics = font.metrics();
        self.boxes.push(InlineBox {
            node,
            parent,
            metrics,
            line_height: line_height(style, &metrics),
            extents: Vec::new(),
        });
        self.fonts.push(font);
        self.boxes.len() - 1
    }

    fn collect(&mut self, b: &LayoutBox, owner: usize) {
        for child in &b.children {
            match child.kind {
                BoxKind::Text => {
                    let Some(node) = child.node else { continue };
                    let text = self.ctx.doc.text(node).unwrap_or("");
                    // Le texte après fusion des espaces, et ses morceaux (mots et
                    // espaces) avec leur nombre de caractères.
                    let mut collapsed = String::new();
                    let mut pieces: Vec<(Piece, usize)> = Vec::new();
                    for c in text.chars() {
                        if is_collapsible_space(c) {
                            if !self.after_space {
                                collapsed.push(' ');
                                pieces.push((Piece::Space, 1));
                            }
                            self.after_space = true;
                        } else {
                            collapsed.push(c);
                            match pieces.last_mut() {
                                Some((Piece::Word, n)) => *n += 1,
                                _ => pieces.push((Piece::Word, 1)),
                            }
                            self.after_space = false;
                        }
                    }
                    // Comme Chromium, le nœud texte est mesuré d'un bloc : le
                    // crénage joue entre ses caractères, espaces compris.
                    let advances = self.fonts[owner].advances(&collapsed);
                    let mut next = 0;
                    for (piece, count) in pieces {
                        let width = advances[next..next + count].iter().sum();
                        next += count;
                        self.items.push(Item {
                            piece,
                            owner,
                            text_node: node,
                            width,
                        });
                    }
                }
                BoxKind::Inline => {
                    let Some(style) = child.node.and_then(|n| self.ctx.styles.get(n)) else {
                        continue;
                    };
                    let index = self.add_box(child.node, Some(owner), style);
                    self.collect(child, index);
                }
                // Un bloc dans une ligne n'est pas encore géré.
                _ => {}
            }
        }
    }
}

/// Met en lignes le contenu en ligne de `b`, dans une largeur `width`. Renvoie
/// la hauteur totale des lignes (0 s'il n'y a aucune ligne : que des espaces),
/// et range la position et la taille de chaque boîte en ligne dans l'arbre.
pub(crate) fn layout_lines(b: &mut LayoutBox, strut: &ComputedStyle, ctx: &Ctx, width: f64) -> f64 {
    let mut c = Collector {
        ctx,
        boxes: Vec::new(),
        fonts: Vec::new(),
        items: Vec::new(),
        after_space: true,
    };
    let root = c.add_box(None, None, strut);
    c.collect(b, root);
    let Collector {
        mut boxes, items, ..
    } = c;

    // Les lignes : on ne coupe qu'aux espaces ; les mots collés (`a<b>b</b>`)
    // restent ensemble.
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut line: Vec<usize> = Vec::new();
    let mut line_width = 0.0;
    let mut pending_space: Option<usize> = None;
    let mut i = 0;
    while i < items.len() {
        if matches!(items[i].piece, Piece::Space) {
            if !line.is_empty() {
                pending_space = Some(i);
            }
            i += 1;
            continue;
        }
        let start = i;
        while i < items.len() && matches!(items[i].piece, Piece::Word) {
            i += 1;
        }
        let segment: f64 = items[start..i].iter().map(|it| it.width).sum();
        let space = pending_space.map_or(0.0, |s| items[s].width);
        if !line.is_empty() && line_width + space + segment > width + 1e-9 {
            lines.push(std::mem::take(&mut line));
            line_width = 0.0;
        } else if let Some(s) = pending_space {
            line.push(s);
            line_width += space;
        }
        line.extend(start..i);
        line_width += segment;
        pending_space = None;
    }
    if !line.is_empty() {
        lines.push(line);
    }

    // Chaque ligne : positions horizontales, puis hauteur et ligne de base.
    let mut top = 0.0;
    let mut baselines = Vec::with_capacity(lines.len());
    for (n, line) in lines.iter().enumerate() {
        // Les morceaux d'un même nœud texte forment un fragment, dont la largeur
        // est arrondie vers le haut au 64e de pixel d'écran, comme Chromium.
        let mut x = 0.0;
        let mut k = 0;
        while k < line.len() {
            let node = items[line[k]].text_node;
            let owner = items[line[k]].owner;
            let mut w = 0.0;
            while k < line.len() && items[line[k]].text_node == node {
                w += items[line[k]].width;
                k += 1;
            }
            let w = ceil_layout_unit(w, ctx.dpr);
            // Le fragment appartient à sa boîte et à toutes ses ancêtres.
            let mut owner = Some(owner);
            while let Some(o) = owner {
                match boxes[o].extents.last_mut() {
                    Some((l, _, end)) if *l == n => *end = x + w,
                    _ => boxes[o].extents.push((n, x, x + w)),
                }
                owner = boxes[o].parent;
            }
            x += w;
        }
        // La boîte racine (la « jambe de force ») compte même sans texte.
        if boxes[root].extents.last().is_none_or(|(l, _, _)| *l != n) {
            boxes[root].extents.push((n, 0.0, 0.0));
        }
        let (mut above, mut below) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for bx in boxes
            .iter()
            .filter(|bx| bx.extents.iter().any(|(l, _, _)| *l == n))
        {
            let (a, b) = leading_split(bx, ctx.dpr);
            above = above.max(a);
            below = below.max(b);
        }
        baselines.push(top + above);
        top += above + below;
    }

    // Le rectangle de chaque élément en ligne : de sa première à sa dernière
    // ligne, sur la hauteur de sa police (pas celle de l'interligne).
    let mut rects: Vec<Option<Rect>> = vec![None; boxes.len()];
    for (index, bx) in boxes.iter().enumerate().skip(1) {
        let (Some(first), Some(last)) = (bx.extents.first(), bx.extents.last()) else {
            continue;
        };
        let x0 = bx.extents.iter().map(|e| e.1).fold(f64::INFINITY, f64::min);
        let x1 = bx
            .extents
            .iter()
            .map(|e| e.2)
            .fold(f64::NEG_INFINITY, f64::max);
        let y0 = baselines[first.0] - bx.metrics.ascent;
        let y1 = baselines[last.0] + bx.metrics.descent;
        rects[index] = Some(Rect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        });
    }
    let by_node: Vec<(NodeId, Rect)> = boxes
        .iter()
        .zip(&rects)
        .filter_map(|(bx, r)| Some((bx.node?, (*r)?)))
        .collect();
    place_inline(b, &by_node, 0.0, 0.0);
    top
}

/// Range dans l'arbre la position de chaque boîte en ligne, relative à la boîte
/// de contenu de son parent (`origin` : la position absolue de ce parent dans
/// le bloc).
fn place_inline(b: &mut LayoutBox, rects: &[(NodeId, Rect)], ox: f64, oy: f64) {
    for child in &mut b.children {
        if child.kind != BoxKind::Inline {
            continue;
        }
        let rect = child
            .node
            .and_then(|n| rects.iter().find(|(id, _)| *id == n))
            .map(|(_, r)| *r);
        match rect {
            Some(r) => {
                child.offset = (r.x - ox, r.y - oy);
                child.content.width = r.width;
                child.content.height = r.height;
                place_inline(child, rects, r.x, r.y);
            }
            None => {
                child.offset = (0.0, 0.0);
                place_inline(child, rects, ox, oy);
            }
        }
    }
}
