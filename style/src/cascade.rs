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

/// Un sélecteur d'une règle, dans l'index.
#[derive(Debug, Clone, Copy)]
struct Entry {
    /// La feuille, la règle et le sélecteur (dans la liste de la règle).
    sheet: u32,
    rule: u32,
    selector: u32,
    /// L'ordre d'apparition de la règle, toutes feuilles confondues.
    order: u32,
    /// La spécificité du sélecteur, calculée une fois.
    specificity: Specificity,
}

/// Les sélecteurs rangés selon leur partie la plus à droite, comme dans tous les
/// moteurs : `#menu a.lien` ne peut viser qu'un élément de classe `lien`. Pour
/// un élément, on ne teste que les sélecteurs de son id, de ses classes, de sa
/// balise, et les « universels » (`*`, `[href]`, `:hover`...).
#[derive(Debug, Default)]
struct RuleIndex {
    by_id: HashMap<String, Vec<Entry>>,
    by_class: HashMap<String, Vec<Entry>>,
    /// Par nom de balise en minuscules (le test complet vérifie ensuite la casse).
    by_tag: HashMap<String, Vec<Entry>>,
    universal: Vec<Entry>,
}

impl RuleIndex {
    fn insert(&mut self, selector: &lumen_css::selectors::Selector, entry: Entry) {
        use lumen_css::selectors::Simple;
        let simple = &selector.subject.simple;
        let bucket = if let Some(id) = simple.iter().find_map(|s| match s {
            Simple::Id(id) => Some(id),
            _ => None,
        }) {
            self.by_id.entry(id.clone()).or_default()
        } else if let Some(class) = simple.iter().find_map(|s| match s {
            Simple::Class(c) => Some(c),
            _ => None,
        }) {
            self.by_class.entry(class.clone()).or_default()
        } else if let Some(tag) = simple.iter().find_map(|s| match s {
            Simple::Type(t) => Some(t),
            _ => None,
        }) {
            self.by_tag.entry(tag.to_ascii_lowercase()).or_default()
        } else {
            &mut self.universal
        };
        bucket.push(entry);
    }

    /// Les sélecteurs qui peuvent viser `element` (à vérifier ensuite).
    fn candidates(&self, element: &DomElement, out: &mut Vec<Entry>) {
        out.extend_from_slice(&self.universal);
        let name = element.local_name();
        let tag = if name.bytes().any(|b| b.is_ascii_uppercase()) {
            self.by_tag.get(&name.to_ascii_lowercase())
        } else {
            self.by_tag.get(name)
        };
        out.extend_from_slice(tag.map_or(&[][..], Vec::as_slice));
        if let Some(id) = element.attribute("id")
            && let Some(entries) = self.by_id.get(id)
        {
            out.extend_from_slice(entries);
        }
        if let Some(classes) = element.attribute("class") {
            for class in classes.split_ascii_whitespace() {
                if let Some(entries) = self.by_class.get(class) {
                    out.extend_from_slice(entries);
                }
            }
        }
    }
}

/// Le moteur de style d'un document : ses feuilles et l'environnement.
pub struct StyleEngine {
    /// Chaque feuille, son origine, et l'espace de noms qu'elle vise (feuilles
    /// du navigateur) ou `None` (tous les éléments).
    sheets: Vec<(Origin, Option<Namespace>, Stylesheet)>,
    index: RuleIndex,
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
        let mut index = RuleIndex::default();
        let mut order = 0u32;
        for (s, (_, _, sheet)) in sheets.iter().enumerate() {
            for (r, rule) in sheet.rules.iter().enumerate() {
                order += 1;
                for (i, selector) in rule.selectors.0.iter().enumerate() {
                    let entry = Entry {
                        sheet: s as u32,
                        rule: r as u32,
                        selector: i as u32,
                        order,
                        specificity: selector.specificity(),
                    };
                    index.insert(selector, entry);
                }
            }
        }
        StyleEngine {
            sheets,
            index,
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
        let mut candidates = Vec::new();
        self.index.candidates(&element, &mut candidates);
        // Les sélecteurs qui correspondent vraiment.
        let mut matched: Vec<Entry> = candidates
            .into_iter()
            .filter(|e| {
                let (_, target, sheet) = &self.sheets[e.sheet as usize];
                target.is_none_or(|t| t == ns)
                    && sheet.rules[e.rule as usize].selectors.0[e.selector as usize]
                        .matches(element)
            })
            .collect();
        // Une règle dont plusieurs sélecteurs correspondent (`a, .x`) compte une
        // fois, avec la plus forte spécificité.
        matched.sort_by_key(|e| (e.order, std::cmp::Reverse(e.specificity)));
        matched.dedup_by_key(|e| e.order);

        let mut found = Vec::new();
        for e in &matched {
            let (origin, _, sheet) = &self.sheets[e.sheet as usize];
            for d in &sheet.rules[e.rule as usize].declarations {
                found.push((
                    priority(*origin, d.important, false, e.specificity, e.order),
                    d,
                ));
            }
        }
        let order = u32::MAX;
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
