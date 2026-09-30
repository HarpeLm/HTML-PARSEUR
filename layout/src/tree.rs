//! L'arbre des boîtes (CSS Display) : les boîtes que produit chaque élément.
//!
//! - `display: none` : aucune boîte, ni pour ses descendants ;
//! - `display: contents` : pas de boîte, ses enfants prennent sa place ;
//! - un élément en ligne (`<span>`) garde son contenu : texte et éléments en
//!   ligne imbriqués, mis en lignes par le module `inline` ;
//! - un bloc qui contient à la fois des blocs et du contenu en ligne : chaque
//!   suite de contenu en ligne est enveloppée dans une boîte de bloc anonyme ;
//!   une suite faite seulement d'espaces entre deux blocs ne produit rien.

use html_parseur::dom::{Document, NodeData, NodeId};
use lumen_style::{Computed, Styles, flat_children};

use crate::{BoxKind, Edges, LayoutBox};

/// `display` produit-il une boîte de niveau bloc ?
fn is_block_level(display: &str) -> bool {
    !matches!(
        display,
        "inline"
            | "inline-block"
            | "inline-flex"
            | "inline-grid"
            | "inline-table"
            | "ruby"
            | "math"
    )
}

fn new_box(node: Option<NodeId>, kind: BoxKind, children: Vec<LayoutBox>) -> LayoutBox {
    LayoutBox {
        node,
        style_node: node,
        kind,
        margin: Edges::default(),
        border: Edges::default(),
        padding: Edges::default(),
        content: crate::Rect::default(),
        offset: (0.0, 0.0),
        children,
    }
}

/// Un texte fait seulement d'espaces « fusionnables ».
pub(crate) fn is_collapsible_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

fn is_blank_text(doc: &Document, b: &LayoutBox) -> bool {
    b.kind == BoxKind::Text
        && b.node
            .and_then(|n| doc.text(n))
            .is_none_or(|t| t.chars().all(is_collapsible_space))
}

/// Les boîtes produites par un nœud (0, 1, ou celles de ses enfants).
pub fn boxes_of(doc: &Document, styles: &Styles, id: NodeId, out: &mut Vec<LayoutBox>) {
    match &doc.node(id).data {
        NodeData::Text(_) => out.push(new_box(Some(id), BoxKind::Text, Vec::new())),
        NodeData::Element(_) => {
            let Some(style) = styles.get(id) else { return };
            let display = match style.get("display") {
                Some(Computed::Keyword(d)) => *d,
                _ => "inline",
            };
            match display {
                "none" => {}
                "contents" => {
                    for child in flat_children(doc, id) {
                        boxes_of(doc, styles, child, out);
                    }
                }
                d if is_block_level(d) => {
                    let children = block_children(doc, styles, id);
                    out.push(new_box(Some(id), BoxKind::Block, children));
                }
                _ => {
                    let mut children = Vec::new();
                    for child in flat_children(doc, id) {
                        boxes_of(doc, styles, child, &mut children);
                    }
                    out.push(new_box(Some(id), BoxKind::Inline, children));
                }
            }
        }
        _ => {}
    }
}

/// Les boîtes filles d'un bloc : que des blocs, ou que du contenu en ligne (le
/// contenu en ligne mêlé à des blocs est enveloppé dans des blocs anonymes).
pub fn block_children(doc: &Document, styles: &Styles, id: NodeId) -> Vec<LayoutBox> {
    let mut boxes = Vec::new();
    for child in flat_children(doc, id) {
        boxes_of(doc, styles, child, &mut boxes);
    }
    let has_block = boxes.iter().any(|b| b.kind == BoxKind::Block);
    if !has_block {
        return boxes;
    }
    let mut result = Vec::new();
    let mut run: Vec<LayoutBox> = Vec::new();
    let flush = |run: &mut Vec<LayoutBox>, result: &mut Vec<LayoutBox>| {
        // Une suite d'espaces entre deux blocs ne produit aucune ligne.
        if !run.is_empty() && !run.iter().all(|b| is_blank_text(doc, b)) {
            let mut anonymous = new_box(None, BoxKind::Anonymous, std::mem::take(run));
            // Le bloc anonyme hérite du style de son parent (police des lignes).
            anonymous.style_node = Some(id);
            result.push(anonymous);
        }
        run.clear();
    };
    for b in boxes {
        if b.kind == BoxKind::Block {
            flush(&mut run, &mut result);
            result.push(b);
        } else {
            run.push(b);
        }
    }
    flush(&mut run, &mut result);
    result
}
