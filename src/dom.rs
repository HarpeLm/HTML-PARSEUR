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
}

#[derive(Debug, Clone)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
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
        Document {
            nodes: vec![Node { parent: None, children: Vec::new(), data: NodeData::Document }],
            atoms: Interner::default(),
            quirks_mode: QuirksMode::NoQuirks,
        }
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
        self.nodes.push(Node { parent: None, children: Vec::new(), data });
        id
    }

    /// Retire un nœud de son parent (le nœud continue d'exister).
    pub fn detach(&mut self, child: NodeId) {
        if let Some(parent) = self.node_mut(child).parent.take() {
            self.node_mut(parent).children.retain(|&c| c != child);
        }
    }

    /// Ajoute `child` comme dernier enfant de `parent`.
    pub fn append(&mut self, parent: NodeId, child: NodeId) {
        self.detach(child);
        self.node_mut(child).parent = Some(parent);
        self.node_mut(parent).children.push(child);
    }

    /// Insère `child` dans `parent`, juste avant `before` (ou à la fin si `None`).
    pub fn insert_before(&mut self, parent: NodeId, child: NodeId, before: Option<NodeId>) {
        let Some(before) = before else {
            return self.append(parent, child);
        };
        self.detach(child);
        self.node_mut(child).parent = Some(parent);
        let children = &mut self.node_mut(parent).children;
        let pos = children.iter().position(|&c| c == before).unwrap_or(children.len());
        children.insert(pos, child);
    }

    /// Insère du texte. S'il y a déjà un nœud texte juste avant l'endroit
    /// d'insertion, on le prolonge au lieu d'en créer un nouveau (§13.2.6.1).
    pub fn insert_text(&mut self, parent: NodeId, before: Option<NodeId>, text: &str) {
        let children = &self.node(parent).children;
        let previous = match before {
            None => children.last().copied(),
            Some(b) => {
                let pos = children.iter().position(|&c| c == b).unwrap_or(0);
                pos.checked_sub(1).map(|p| children[p])
            }
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

    /// Déplace tous les enfants de `from` à la fin de `to`.
    pub fn reparent_children(&mut self, from: NodeId, to: NodeId) {
        let children = std::mem::take(&mut self.node_mut(from).children);
        for &child in &children {
            self.node_mut(child).parent = Some(to);
        }
        self.node_mut(to).children.extend(children);
    }

    /// Tous les descendants de `root`, dans l'ordre du document (sans `root`).
    pub fn descendants(&self, root: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let mut stack: Vec<NodeId> = self.node(root).children.iter().rev().copied().collect();
        std::iter::from_fn(move || {
            let node = stack.pop()?;
            stack.extend(self.node(node).children.iter().rev());
            Some(node)
        })
    }

    /// Copie profonde d'un nœud et de ses descendants (la copie est détachée).
    pub fn clone_subtree(&mut self, id: NodeId) -> NodeId {
        let data = self.node(id).data.clone();
        let copy = self.create(data);
        for child in self.node(id).children.clone() {
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
        for &child in &self.node(root).children {
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
                    for &child in &self.node(contents).children {
                        self.dump(child, depth + 2, out);
                    }
                }
                for &child in &self.node(id).children {
                    self.dump(child, depth + 1, out);
                }
                return;
            }
        }
        out.push('\n');
    }
}
