//! La cascade : pour chaque élément, trouver les déclarations qui s'appliquent,
//! garder la gagnante pour chaque propriété (CSS Cascade 4), puis calculer.
//!
//! L'ordre de priorité, du plus faible au plus fort :
//! 1. l'origine et l'importance : navigateur, auteur, auteur `!important`,
//!    navigateur `!important` ;
//! 2. l'attribut `style=""`, qui l'emporte sur les feuilles ;
//! 3. la spécificité du sélecteur ;
//! 4. l'ordre d'apparition.

use std::collections::HashMap;
use std::rc::Rc;

use html_parseur::dom::{AttrNamespace, Document, Namespace, NodeData, NodeId};
use lumen_css::media::{Environment, MediaQueryList};
use lumen_css::properties::parse_property;
use lumen_css::selectors::{Element, Specificity};
use lumen_css::variables::{
    CustomProperties, contains_var, css_wide_keyword, is_custom_property, is_valid_value,
    substitute,
};
use lumen_css::{Parser, preprocess};

use crate::computed::{Cascaded, ComputedStyle, Context, PROPERTIES, compute, longhands};
use crate::sheet::{Declaration, Stylesheet, parse_style_attribute};

// ───────────── Le DOM vu par les sélecteurs ─────────────

/// Un élément d'un [`Document`], pour les sélecteurs.
#[derive(Clone, Copy)]
pub struct DomElement<'d> {
    doc: &'d Document,
    id: NodeId,
}

impl<'d> DomElement<'d> {
    /// L'élément `id` de `doc` (qui doit être un élément).
    pub fn new(doc: &'d Document, id: NodeId) -> Self {
        DomElement { doc, id }
    }

    fn element(&self) -> &'d html_parseur::dom::Element {
        self.doc
            .element(self.id)
            .expect("DomElement sur un nœud qui n'est pas un élément")
    }

    fn wrap(&self, id: Option<NodeId>) -> Option<Self> {
        let id = id?;
        self.doc
            .element(id)
            .map(|_| DomElement { doc: self.doc, id })
    }

    fn sibling(&self, next: bool) -> Option<Self> {
        let mut cur = self.id;
        loop {
            cur = if next {
                self.doc.next_sibling(cur)?
            } else {
                self.doc.prev_sibling(cur)?
            };
            if self.doc.element(cur).is_some() {
                return Some(DomElement {
                    doc: self.doc,
                    id: cur,
                });
            }
        }
    }
}

impl Element for DomElement<'_> {
    fn local_name(&self) -> &str {
        self.doc.atoms.name(self.element().name)
    }
    fn is_html(&self) -> bool {
        self.element().ns == Namespace::Html
    }
    fn attribute(&self, name: &str) -> Option<&str> {
        self.element()
            .attrs
            .iter()
            .find(|a| a.ns == AttrNamespace::None && a.name == name)
            .map(|a| a.value.as_str())
    }
    fn parent_element(&self) -> Option<Self> {
        self.wrap(self.doc.node(self.id).parent)
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        self.sibling(false)
    }
    fn next_sibling_element(&self) -> Option<Self> {
        self.sibling(true)
    }
    fn is_empty(&self) -> bool {
        !self.doc.children(self.id).any(|c| {
            matches!(
                self.doc.node(c).data,
                NodeData::Element(_) | NodeData::Text(_)
            )
        })
    }
    fn same_as(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

// ───────────── Les feuilles d'un document ─────────────

/// La feuille de style par défaut du navigateur (éléments HTML).
pub const USER_AGENT_CSS: &str = include_str!("ua.css");

/// La feuille par défaut des éléments MathML.
pub const MATHML_CSS: &str = include_str!("mathml.css");

/// Les feuilles `<style>` du document, dans l'ordre. Les `<link rel=stylesheet>`
/// ne sont pas chargées (pas encore de réseau dans Lumen).
pub fn author_stylesheets(doc: &Document, env: &Environment) -> Vec<Stylesheet> {
    let mut sheets = Vec::new();
    for id in doc.descendants(NodeId::DOCUMENT) {
        let Some(el) = doc.element(id) else { continue };
        // `<style>` en HTML, et aussi dans un `<svg>`.
        if el.ns == Namespace::MathMl || doc.atoms.name(el.name) != "style" {
            continue;
        }
        let attr = |name: &str| {
            el.attrs
                .iter()
                .find(|a| a.ns == AttrNamespace::None && a.name == name)
                .map(|a| a.value.as_str())
        };
        if attr("type").is_some_and(|t| !t.is_empty() && !t.eq_ignore_ascii_case("text/css")) {
            continue;
        }
        if let Some(media) = attr("media") {
            let css = preprocess(media);
            let list = MediaQueryList::parse(&Parser::new(&css).parse_component_value_list());
            if !list.matches(env) {
                continue;
            }
        }
        let text: String = doc.children(id).filter_map(|c| doc.text(c)).collect();
        sheets.push(Stylesheet::parse(&text, env));
    }
    sheets
}

// ───────────── La cascade ─────────────

/// L'origine d'une déclaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    UserAgent,
    Author,
}

/// La clé de tri : plus elle est grande, plus la déclaration est prioritaire.
type Priority = (u8, bool, Specificity, u32);

fn priority(
    origin: Origin,
    important: bool,
    inline: bool,
    spec: Specificity,
    order: u32,
) -> Priority {
    let level = match (origin, important) {
        (Origin::UserAgent, false) => 0,
        (Origin::Author, false) => 1,
        (Origin::Author, true) => 2,
        (Origin::UserAgent, true) => 3,
    };
    (level, inline, spec, order)
}

/// Les styles calculés de tous les éléments d'un document.
#[derive(Debug, Default)]
pub struct Styles {
    styles: HashMap<NodeId, Rc<ComputedStyle>>,
}

impl Styles {
    /// Le style calculé d'un élément.
    pub fn get(&self, id: NodeId) -> Option<&ComputedStyle> {
        self.styles.get(&id).map(|s| &**s)
    }

    /// Le nombre d'éléments stylés.
    pub fn len(&self) -> usize {
        self.styles.len()
    }

    /// Aucun élément.
    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }
}

/// Le moteur de style d'un document : ses feuilles et l'environnement.
pub struct StyleEngine {
    /// Chaque feuille, son origine, et l'espace de noms qu'elle vise (feuilles
    /// du navigateur) ou `None` (tous les éléments).
    sheets: Vec<(Origin, Option<Namespace>, Stylesheet)>,
    env: Environment,
}

impl StyleEngine {
    /// Le nombre total de règles de style (navigateur et document).
    pub fn rule_count(&self) -> usize {
        self.sheets.iter().map(|(_, _, s)| s.rules.len()).sum()
    }

    /// Prépare les feuilles : celles du navigateur, puis celles du document.
    pub fn new(doc: &Document, env: &Environment) -> StyleEngine {
        let mut sheets = vec![
            (
                Origin::UserAgent,
                Some(Namespace::Html),
                Stylesheet::parse(USER_AGENT_CSS, env),
            ),
            (
                Origin::UserAgent,
                Some(Namespace::MathMl),
                Stylesheet::parse(MATHML_CSS, env),
            ),
        ];
        sheets.extend(
            author_stylesheets(doc, env)
                .into_iter()
                .map(|s| (Origin::Author, None, s)),
        );
        StyleEngine {
            sheets,
            env: env.clone(),
        }
    }

    /// Calcule le style de tous les éléments, du haut de l'arbre vers le bas.
    pub fn style_document(&self, doc: &Document) -> Styles {
        let mut styles = Styles::default();
        let mut root_font_size = 16.0;
        // Pile de (nœud, style du parent) : pas de récursion, même sur un DOM très profond.
        let mut stack: Vec<(NodeId, Option<Rc<ComputedStyle>>)> =
            doc.children(NodeId::DOCUMENT).map(|c| (c, None)).collect();
        stack.reverse();
        while let Some((id, parent)) = stack.pop() {
            if doc.element(id).is_none() {
                continue;
            }
            let is_root = parent.is_none();
            let ctx = Context {
                env: self.env.clone(),
                root_font_size,
                is_root,
                parent_is_flex_or_grid: parent.as_ref().is_some_and(|p| {
                    matches!(
                        p.get("display").map(|d| d.to_string()).as_deref(),
                        Some("flex" | "inline-flex" | "grid" | "inline-grid")
                    )
                }),
            };
            let style = Rc::new(self.style_element(doc, id, parent.as_deref(), &ctx));
            if is_root {
                root_font_size = style.font_size.px;
            }
            let children: Vec<NodeId> = doc.children(id).collect();
            for c in children.into_iter().rev() {
                stack.push((c, Some(style.clone())));
            }
            styles.styles.insert(id, style);
        }
        styles
    }

    /// Les déclarations qui s'appliquent à un élément, de la moins à la plus
    /// prioritaire.
    fn matching_declarations<'s>(
        &'s self,
        doc: &Document,
        id: NodeId,
        inline: &'s [Declaration],
    ) -> Vec<(Priority, &'s Declaration)> {
        let element = DomElement::new(doc, id);
        let ns = element.element().ns;
        let mut found = Vec::new();
        let mut order = 0u32;
        for (origin, target, sheet) in &self.sheets {
            let skip = target.is_some_and(|t| t != ns);
            for rule in &sheet.rules {
                order += 1;
                if skip {
                    continue;
                }
                let spec = rule
                    .selectors
                    .0
                    .iter()
                    .filter(|s| s.matches(element))
                    .map(|s| s.specificity())
                    .max();
                if let Some(spec) = spec {
                    for d in &rule.declarations {
                        found.push((priority(*origin, d.important, false, spec, order), d));
                    }
                }
            }
        }
        order += 1;
        for d in inline {
            found.push((
                priority(Origin::Author, d.important, true, (0, 0, 0), order),
                d,
            ));
        }
        // Tri stable : à priorité égale, l'ordre des déclarations dans la règle.
        found.sort_by_key(|(p, _)| *p);
        found
    }

    fn style_element(
        &self,
        doc: &Document,
        id: NodeId,
        parent: Option<&ComputedStyle>,
        ctx: &Context,
    ) -> ComputedStyle {
        let inline = DomElement::new(doc, id)
            .attribute("style")
            .map(parse_style_attribute)
            .unwrap_or_default();
        let declarations = self.matching_declarations(doc, id, &inline);

        // Les propriétés personnalisées d'abord : les autres peuvent en dépendre.
        let empty = CustomProperties::default();
        let parent_custom = parent.map_or(&empty, |p| &*p.custom);
        let custom = CustomProperties::compute(
            declarations
                .iter()
                .filter(|(_, d)| is_custom_property(&d.name))
                .map(|(_, d)| (d.name.as_str(), d.value.as_str())),
            parent_custom,
        );
        let custom = match parent {
            Some(p) if *p.custom == custom => p.custom.clone(),
            _ => Rc::new(custom),
        };

        // La déclaration gagnante de chaque propriété longue (la dernière).
        let mut cascaded = vec![Cascaded::None; PROPERTIES.len()];
        for (_, d) in declarations
            .iter()
            .filter(|(_, d)| !is_custom_property(&d.name))
        {
            let targets = longhands(&d.name);
            if targets.is_empty() {
                continue; // propriété que Lumen ne calcule pas encore
            }
            let values: Vec<(&'static str, Cascaded)> = if let Some(k) = css_wide_keyword(&d.value)
            {
                let keyword = match k {
                    "initial" => Cascaded::Initial,
                    "inherit" => Cascaded::Inherit,
                    _ => Cascaded::Unset,
                };
                targets.iter().map(|t| (*t, keyword.clone())).collect()
            } else if contains_var(&d.value) {
                if !is_valid_value(&d.value) {
                    continue;
                }
                // Invalide après substitution : `unset` (décidé au calcul).
                let substituted = substitute(&d.value, &custom).and_then(|text| {
                    parse_property(&d.name, &Parser::new(&text).parse_component_value_list())
                });
                match substituted {
                    Some(longs) => longs
                        .into_iter()
                        .map(|(n, v)| (n, Cascaded::Value(v)))
                        .collect(),
                    None => targets.iter().map(|t| (*t, Cascaded::Unset)).collect(),
                }
            } else {
                match parse_property(&d.name, &Parser::new(&d.value).parse_component_value_list()) {
                    Some(longs) => longs
                        .into_iter()
                        .map(|(n, v)| (n, Cascaded::Value(v)))
                        .collect(),
                    None => continue, // invalide : la déclaration est ignorée
                }
            };
            for (name, value) in values {
                if let Some(i) = crate::computed::property_index(name) {
                    cascaded[i] = value;
                }
            }
        }
        compute(&cascaded, parent, custom, ctx)
    }
}

/// Raccourci : le style calculé de tous les éléments de `doc` dans `env`.
pub fn style_document(doc: &Document, env: &Environment) -> Styles {
    StyleEngine::new(doc, env).style_document(doc)
}
