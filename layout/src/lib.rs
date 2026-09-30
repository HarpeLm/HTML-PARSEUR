//! # lumen-layout
//!
//! La mise en page CSS : la position et la taille de chaque boîte. C'est une
//! brique de Lumen, un navigateur web écrit de zéro ; elle part du DOM
//! (`html-parseur`) et des styles calculés (`lumen-style`).
//!
//! Première étape : les **boîtes de bloc** (CSS 2, chapitres 8 à 10). Largeurs
//! (`auto`, `%`, `min-`/`max-width`, `box-sizing`, marges `auto` pour centrer),
//! hauteurs, empilement, fusion des marges verticales. Le texte n'est pas encore
//! mis en page (il faut des polices) : un bloc qui ne contient que du contenu en
//! ligne a une hauteur de contenu nulle.
//!
//! Vérifié contre `getBoundingClientRect()` de Chromium (tests/oracle_blocks.rs).
//!
//! ```
//! use html_parseur::parse_document;
//! use html_parseur::dom::NodeId;
//! use lumen_css::media::Environment;
//! use lumen_layout::layout_document;
//! use lumen_style::style_document;
//!
//! let doc = parse_document("<body style='margin: 0'><div style='height: 50px; margin: 10px auto; width: 50%'></div>");
//! let env = Environment { width: 800.0, height: 600.0, ..Environment::default() };
//! let styles = style_document(&doc, &env);
//! let layout = layout_document(&doc, &styles, env.width, env.height);
//! let div = doc.descendants(NodeId::DOCUMENT)
//!     .find(|&n| doc.element(n).is_some_and(|e| doc.atoms.name(e.name) == "div"))
//!     .unwrap();
//! let r = layout.border_box(div).unwrap();
//! assert_eq!((r.x, r.y, r.width, r.height), (200.0, 10.0, 400.0, 50.0));
//! ```

#![warn(missing_docs)]

mod block;
mod tree;

use std::collections::HashMap;

use html_parseur::dom::{Document, NodeId};
use lumen_style::Styles;

use crate::block::{Containing, layout_block, place};

/// Un rectangle, en px CSS, dans le repère de la page (origine en haut à gauche).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    /// Bord gauche.
    pub x: f64,
    /// Bord haut.
    pub y: f64,
    /// Largeur.
    pub width: f64,
    /// Hauteur.
    pub height: f64,
}

/// Des épaisseurs pour les quatre côtés (marges, bordures, retraits).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Edges {
    /// Haut.
    pub top: f64,
    /// Droite.
    pub right: f64,
    /// Bas.
    pub bottom: f64,
    /// Gauche.
    pub left: f64,
}

/// La nature d'une boîte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxKind {
    /// Une boîte de bloc produite par un élément.
    Block,
    /// Un bloc anonyme, qui enveloppe du contenu en ligne voisin de blocs.
    Anonymous,
    /// Un élément en ligne (`<span>`, `inline-block`...) : pas encore mis en page.
    Inline,
    /// Du texte : pas encore mis en page.
    Text,
}

/// Une boîte et ses enfants, après mise en page.
#[derive(Debug, Clone)]
pub struct LayoutBox {
    /// Le nœud qui l'a produite (`None` : boîte anonyme).
    pub node: Option<NodeId>,
    /// Sa nature.
    pub kind: BoxKind,
    /// Marges utilisées (après résolution des `auto`).
    pub margin: Edges,
    /// Épaisseurs des bordures.
    pub border: Edges,
    /// Retraits.
    pub padding: Edges,
    /// La boîte de contenu, en position absolue.
    pub content: Rect,
    /// Position de la boîte de bordure par rapport à la boîte de contenu du parent.
    pub(crate) offset: (f64, f64),
    /// Les boîtes filles.
    pub children: Vec<LayoutBox>,
}

impl LayoutBox {
    /// La boîte de bordure (ce que renvoie `getBoundingClientRect()`).
    pub fn border_box(&self) -> Rect {
        Rect {
            x: self.content.x - self.padding.left - self.border.left,
            y: self.content.y - self.padding.top - self.border.top,
            width: self.content.width
                + self.padding.left
                + self.padding.right
                + self.border.left
                + self.border.right,
            height: self.content.height
                + self.padding.top
                + self.padding.bottom
                + self.border.top
                + self.border.bottom,
        }
    }
}

/// Le résultat de la mise en page d'un document.
#[derive(Debug, Default)]
pub struct Layout {
    /// La boîte de l'élément racine (`None` si `display: none`).
    pub root: Option<LayoutBox>,
    rects: HashMap<NodeId, Rect>,
}

impl Layout {
    /// La boîte de bordure d'un élément (`None` s'il n'a pas de boîte).
    pub fn border_box(&self, id: NodeId) -> Option<Rect> {
        self.rects.get(&id).copied()
    }
}

/// Met en page le document dans une fenêtre de `width` × `height` px.
pub fn layout_document(doc: &Document, styles: &Styles, width: f64, height: f64) -> Layout {
    let Some(root_id) = doc
        .children(NodeId::DOCUMENT)
        .find(|&c| doc.element(c).is_some())
    else {
        return Layout::default();
    };
    let mut boxes = Vec::new();
    tree::boxes_of(doc, styles, root_id, &mut boxes);
    let Some(mut root) = boxes.into_iter().next() else {
        return Layout::default();
    };
    // Le bloc conteneur de la racine est la fenêtre (« initial containing block »).
    let icb = Containing {
        width,
        height: Some(height),
    };
    layout_block(&mut root, styles.get(root_id), styles, icb, true);
    // Les marges de la racine ne fusionnent avec rien.
    let (x, y) = (root.margin.left, root.margin.top);
    place(&mut root, x, y);

    let mut rects = HashMap::new();
    let mut stack = vec![&root];
    while let Some(b) = stack.pop() {
        if let Some(node) = b.node
            && doc.element(node).is_some()
        {
            rects.insert(node, b.border_box());
        }
        stack.extend(&b.children);
    }
    Layout {
        root: Some(root),
        rects,
    }
}
