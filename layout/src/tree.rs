//! L'arbre des boîtes (CSS Display) : les boîtes que produit chaque élément.
//!
//! - `display: none` : aucune boîte, ni pour ses descendants ;
//! - `display: contents` : pas de boîte, ses enfants prennent sa place ;
//! - un bloc qui contient à la fois des blocs et du contenu « en ligne » (texte,
//!   `<span>`...) : chaque suite de contenu en ligne est enveloppée dans une
//!   boîte de bloc anonyme ;
//! - le texte fait uniquement d'espaces entre deux blocs ne produit rien.

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
        kind,
        margin: Edges::default(),
        border: Edges::default(),
        padding: Edges::default(),
        content: crate::Rect::default(),
        offset: (0.0, 0.0),
        children,
    }
}

/// Les boîtes produites par un nœud (0, 1, ou celles de ses enfants).
pub fn boxes_of(doc: &Document, styles: &Styles, id: NodeId, out: &mut Vec<LayoutBox>) {
    match &doc.node(id).data {
        NodeData::Text(_) => {
            let text = doc.text(id).unwrap_or("");
            // Des espaces seuls (hors `white-space: pre`, pas encore géré) ne
            // produisent aucune ligne.
            if !text
                .chars()
                .all(|c| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}'))
            {
                out.push(new_box(Some(id), BoxKind::Text, Vec::new()));
            }
        }
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
                _ => out.push(new_box(Some(id), BoxKind::Inline, Vec::new())),
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
    let has_inline = boxes.iter().any(|b| b.kind != BoxKind::Block);
    if !(has_block && has_inline) {
        return boxes;
    }
    let mut result = Vec::new();
    let mut run = Vec::new();
    for b in boxes {
        if b.kind == BoxKind::Block {
            if !run.is_empty() {
                result.push(new_box(None, BoxKind::Anonymous, std::mem::take(&mut run)));
            }
            result.push(b);
        } else {
            run.push(b);
        }
    }
    if !run.is_empty() {
        result.push(new_box(None, BoxKind::Anonymous, run));
    }
    result
}
