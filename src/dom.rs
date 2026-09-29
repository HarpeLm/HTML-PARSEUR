//! L'arbre DOM construit par le parser.
//!
//! Les nœuds sont rangés dans un seul `Vec` (une "arène") et se désignent par leur
//! indice (`NodeId`). C'est plus simple et plus rapide en Rust qu'un arbre de
//! pointeurs `Rc<RefCell<...>>` : pas de compteur de références, pas d'emprunts à
//! l'exécution, et les nœuds sont côte à côte en mémoire.
//!
//! Contrairement aux tokens, le DOM POSSÈDE ses données (des `String`) : il doit
//! pouvoir vivre après la page HTML d'origine et être modifié par JavaScript.

use crate::atoms::{Atom, Interner};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// Le nœud Document, racine de l'arbre, est toujours le premier.
    pub const DOCUMENT: NodeId = NodeId(0);

    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    Html,
    Svg,
    MathMl,
}

/// Espace de noms d'un attribut (seuls les attributs SVG/MathML en ont un).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrNamespace {
    None,
    XLink,
    Xml,
    Xmlns,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub ns: AttrNamespace,
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct Element {
    pub ns: Namespace,
    pub name: Atom,
    pub attrs: Vec<Attribute>,
    /// Pour `<template>` : le fragment qui contient son contenu.
    pub template_contents: Option<NodeId>,
}

#[derive(Debug, Clone)]
pub enum NodeData {
    Document,
    DocumentFragment,
    Doctype {
        name: String,
        public_id: String,
        system_id: String,
    },
    Element(Element),
    Text(String),
    Comment(String),
    ProcessingInstruction {
        target: String,
        data: String,
    },
}

/// Un nœud et ses liens dans l'arbre. Les enfants forment une liste chaînée
/// (premier/dernier enfant, frère précédent/suivant), comme dans les vrais
/// navigateurs : insérer ou retirer un nœud ne demande aucune allocation.
/// Pour parcourir : `Document::children`, `Document::descendants`.
#[derive(Debug, Clone)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub(crate) first_child: Option<NodeId>,
    pub(crate) last_child: Option<NodeId>,
    pub(crate) prev_sibling: Option<NodeId>,
    pub(crate) next_sibling: Option<NodeId>,
    pub data: NodeData,
}

/// Le mode de rendu choisi d'après le DOCTYPE (§13.2.6.4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuirksMode {
    #[default]
    NoQuirks,
    LimitedQuirks,
    Quirks,
}

#[derive(Debug, Clone)]
pub struct Document {
    nodes: Vec<Node>,
    pub atoms: Interner,
    pub quirks_mode: QuirksMode,
}

impl Default for Document {
    fn default() -> Self {
        let mut doc = Document { nodes: Vec::new(), atoms: Interner::default(), quirks_mode: QuirksMode::NoQuirks };
        doc.create(NodeData::Document);
        doc
    }
}

impl Document {
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    pub fn element(&self, id: NodeId) -> Option<&Element> {
        match &self.node(id).data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn element_mut(&mut self, id: NodeId) -> Option<&mut Element> {
        match &mut self.node_mut(id).data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    /// Crée un nœud détaché (sans parent).
    pub fn create(&mut self, data: NodeData) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node {
            parent: None,
            first_child: None,
            last_child: None,
            prev_sibling: None,
            next_sibling: None,
            data,
        });
        id
    }

    /// Les enfants d'un nœud, dans l'ordre.
    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut next = self.node(id).first_child;
        std::iter::from_fn(move || {
            let current = next?;
            next = self.node(current).next_sibling;
            Some(current)
        })
    }

    /// Retire un nœud de son parent (le nœud continue d'exister). O(1) : on
    /// raccroche simplement ses deux voisins entre eux.
    pub fn detach(&mut self, child: NodeId) {
        let Some(parent) = self.node(child).parent else { return };
        let (prev, next) = (self.node(child).prev_sibling, self.node(child).next_sibling);
        match prev {
            Some(p) => self.node_mut(p).next_sibling = next,
            None => self.node_mut(parent).first_child = next,
        }
        match next {
            Some(n) => self.node_mut(n).prev_sibling = prev,
            None => self.node_mut(parent).last_child = prev,
        }
        let node = self.node_mut(child);
        node.parent = None;
        node.prev_sibling = None;
        node.next_sibling = None;
    }

    /// Ajoute `child` comme dernier enfant de `parent`.
    pub fn append(&mut self, parent: NodeId, child: NodeId) {
        self.insert_before(parent, child, None);
    }

    /// Insère `child` dans `parent`, juste avant `before` (ou à la fin si `None`).
    pub fn insert_before(&mut self, parent: NodeId, child: NodeId, before: Option<NodeId>) {
        self.detach(child);
        let prev = match before {
            Some(b) => self.node(b).prev_sibling,
            None => self.node(parent).last_child,
        };
        {
            let node = self.node_mut(child);
            node.parent = Some(parent);
            node.prev_sibling = prev;
            node.next_sibling = before;
        }
        match prev {
            Some(p) => self.node_mut(p).next_sibling = Some(child),
            None => self.node_mut(parent).first_child = Some(child),
        }
        match before {
            Some(b) => self.node_mut(b).prev_sibling = Some(child),
            None => self.node_mut(parent).last_child = Some(child),
        }
    }

    /// Insère du texte. S'il y a déjà un nœud texte juste avant l'endroit
    /// d'insertion, on le prolonge au lieu d'en créer un nouveau (§13.2.6.1).
    pub fn insert_text(&mut self, parent: NodeId, before: Option<NodeId>, text: &str) {
        let previous = match before {
            None => self.node(parent).last_child,
            Some(b) => self.node(b).prev_sibling,
        };
        if let Some(prev) = previous {
            if let NodeData::Text(existing) = &mut self.node_mut(prev).data {
                existing.push_str(text);
                return;
            }
        }
        let node = self.create(NodeData::Text(text.to_string()));
        self.insert_before(parent, node, before);
    }

    /// Retire tous les enfants d'un nœud.
    pub fn remove_children(&mut self, id: NodeId) {
        while let Some(child) = self.node(id).first_child {
            self.detach(child);
        }
    }

    /// Déplace tous les enfants de `from` à la fin de `to`.
    pub fn reparent_children(&mut self, from: NodeId, to: NodeId) {
        while let Some(child) = self.node(from).first_child {
            self.append(to, child);
        }
    }

    /// Tous les descendants de `root`, dans l'ordre du document (sans `root`).
    /// Parcours sans pile : on descend au premier enfant, sinon on passe au frère
    /// suivant, sinon on remonte.
    pub fn descendants(&self, root: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut next = self.node(root).first_child;
        std::iter::from_fn(move || {
            let current = next?;
            next = self.node(current).first_child.or_else(|| {
                let mut node = current;
                loop {
                    if node == root {
                        return None;
                    }
                    if let Some(sibling) = self.node(node).next_sibling {
                        return Some(sibling);
                    }
                    node = self.node(node).parent?;
                }
            });
            Some(current)
        })
    }

    /// Copie profonde d'un nœud et de ses descendants (la copie est détachée).
    pub fn clone_subtree(&mut self, id: NodeId) -> NodeId {
        let data = self.node(id).data.clone();
        let copy = self.create(data);
        let children: Vec<NodeId> = self.children(id).collect();
        for child in children {
            let child_copy = self.clone_subtree(child);
            self.append(copy, child_copy);
        }
        copy
    }

    /// Sérialise l'arbre au format des tests html5lib/WPT :
    ///
    /// ```text
    /// | <html>
    /// |   <head>
    /// |   <body>
    /// |     <p class="x">
    /// ```
    pub fn to_test_string(&self) -> String {
        self.to_test_string_from(NodeId::DOCUMENT)
    }

    /// Même chose, pour les enfants d'un nœud donné (fragments).
    pub fn to_test_string_from(&self, root: NodeId) -> String {
        let mut out = String::new();
        for child in self.children(root) {
            self.dump(child, 0, &mut out);
        }
        if out.ends_with('\n') {
            out.pop();
        }
        out
    }

    fn dump(&self, id: NodeId, depth: usize, out: &mut String) {
        let indent = |out: &mut String, depth: usize| {
            out.push_str("| ");
            for _ in 0..depth {
                out.push_str("  ");
            }
        };
        indent(out, depth);
        match &self.node(id).data {
            NodeData::Document | NodeData::DocumentFragment => out.push_str("#document"),
            NodeData::Doctype { name, public_id, system_id } => {
                out.push_str("<!DOCTYPE ");
                out.push_str(name);
                if !public_id.is_empty() || !system_id.is_empty() {
                    out.push_str(&format!(" \"{public_id}\" \"{system_id}\""));
                }
                out.push('>');
            }
            NodeData::Text(text) => {
                out.push('"');
                out.push_str(text);
                out.push('"');
            }
            NodeData::Comment(text) => {
                out.push_str("<!-- ");
                out.push_str(text);
                out.push_str(" -->");
            }
            NodeData::ProcessingInstruction { target, data } => {
                out.push_str(&format!("<?{target} {data}?>"));
            }
            NodeData::Element(e) => {
                out.push('<');
                out.push_str(match e.ns {
                    Namespace::Html => "",
                    Namespace::Svg => "svg ",
                    Namespace::MathMl => "math ",
                });
                out.push_str(self.atoms.name(e.name));
                out.push_str(">\n");

                let mut attrs: Vec<(String, &str)> = e
                    .attrs
                    .iter()
                    .map(|a| {
                        let prefix = match a.ns {
                            AttrNamespace::None => "",
                            AttrNamespace::XLink => "xlink ",
                            AttrNamespace::Xml => "xml ",
                            AttrNamespace::Xmlns => "xmlns ",
                        };
                        (format!("{prefix}{}", a.name), a.value.as_str())
                    })
                    .collect();
                attrs.sort();
                for (name, value) in attrs {
                    indent(out, depth + 1);
                    out.push_str(&format!("{name}=\"{value}\"\n"));
                }
                if let Some(contents) = e.template_contents {
                    indent(out, depth + 1);
                    out.push_str("content\n");
                    for child in self.children(contents) {
                        self.dump(child, depth + 2, out);
                    }
                }
                for child in self.children(id) {
                    self.dump(child, depth + 1, out);
                }
                return;
            }
        }
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(doc: &mut Document, s: &str) -> NodeId {
        doc.create(NodeData::Text(s.to_string()))
    }

    fn names(doc: &Document, parent: NodeId) -> Vec<String> {
        doc.children(parent)
            .map(|c| match &doc.node(c).data {
                NodeData::Text(t) => t.clone(),
                _ => "?".into(),
            })
            .collect()
    }

    #[test]
    fn liste_chainee_des_enfants() {
        let mut doc = Document::default();
        let root = NodeId::DOCUMENT;
        let (a, b, c) = (text(&mut doc, "a"), text(&mut doc, "b"), text(&mut doc, "c"));
        doc.append(root, a);
        doc.append(root, c);
        doc.insert_before(root, b, Some(c));
        assert_eq!(names(&doc, root), ["a", "b", "c"]);

        // Déplacer un nœud le retire d'abord de sa place actuelle.
        doc.insert_before(root, c, Some(a));
        assert_eq!(names(&doc, root), ["c", "a", "b"]);

        doc.detach(a);
        assert_eq!(names(&doc, root), ["c", "b"]);
        assert_eq!(doc.node(a).parent, None);

        let other = doc.create(NodeData::DocumentFragment);
        doc.reparent_children(root, other);
        assert!(names(&doc, root).is_empty());
        assert_eq!(names(&doc, other), ["c", "b"]);
    }

    #[test]
    fn descendants_dans_l_ordre_du_document() {
        let mut doc = Document::default();
        let root = NodeId::DOCUMENT;
        let frag = doc.create(NodeData::DocumentFragment);
        let (a, b, c) = (text(&mut doc, "a"), text(&mut doc, "b"), text(&mut doc, "c"));
        doc.append(root, frag);
        doc.append(frag, a);
        doc.append(frag, b);
        doc.append(root, c);
        let order: Vec<NodeId> = doc.descendants(root).collect();
        assert_eq!(order, [frag, a, b, c]);
        // Les descendants d'un sous-arbre ne débordent pas sur ses frères.
        assert_eq!(doc.descendants(frag).collect::<Vec<_>>(), [a, b]);
    }
}
