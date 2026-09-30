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
use lumen_css::variables::{CustomProperties, substitute};
use lumen_css::{Parser, preprocess};

use crate::bloom::{AncestorFilter, MAX_HASHES, ancestor_hashes};
use crate::computed::{Cascaded, ComputedStyle, Context, PROPERTIES, compute, property_index};
use crate::sheet::{Declaration, Parsed, Stylesheet, parse_style_attribute};

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
    /// La racine du document (pas le premier élément d'un arbre fantôme).
    fn is_root(&self) -> bool {
        self.doc.node(self.id).parent == Some(NodeId::DOCUMENT)
    }
}

// ───────────── Les feuilles d'un document ─────────────

/// La feuille de style par défaut du navigateur (éléments HTML).
pub const USER_AGENT_CSS: &str = include_str!("ua.css");

/// La feuille par défaut des éléments MathML.
pub const MATHML_CSS: &str = include_str!("mathml.css");

/// La feuille par défaut des éléments SVG.
pub const SVG_CSS: &str = include_str!("svg.css");

/// Les feuilles `<style>` du document, dans l'ordre. Les `<link rel=stylesheet>`
/// ne sont pas chargées (pas encore de réseau dans Lumen).
pub fn author_stylesheets(doc: &Document, env: &Environment) -> Vec<Stylesheet> {
    stylesheets_under(doc, NodeId::DOCUMENT, env)
}

/// Les feuilles `<style>` d'un arbre (le document, ou un arbre fantôme).
fn stylesheets_under(doc: &Document, root: NodeId, env: &Environment) -> Vec<Stylesheet> {
    let mut sheets = Vec::new();
    for id in doc.descendants(root) {
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
/// (origine et importance, contexte, attribut `style`, spécificité, ordre)
type Priority = (u8, u8, bool, Specificity, u32);

/// D'où vient une règle, vu de l'élément : de l'arbre où il se trouve, ou de
/// l'arbre fantôme dont il est l'hôte (règles `:host`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeContext {
    Own,
    Inner,
}

/// L'arbre dans lequel se trouve un élément : le document, ou l'arbre fantôme
/// d'un hôte. Seules les feuilles de cet arbre (et celles du navigateur) le
/// visent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Document,
    Shadow(NodeId),
}

fn priority(
    origin: Origin,
    important: bool,
    context: TreeContext,
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
    // CSS Cascade 4, « contexte » : entre une règle de l'arbre de l'élément et
    // une règle `:host` de son arbre fantôme, la première gagne si elles sont
    // normales, la seconde si elles sont `!important`.
    let context = match (context, important) {
        (TreeContext::Own, false) | (TreeContext::Inner, true) => 1,
        _ => 0,
    };
    (level, context, inline, spec, order)
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
    /// Les empreintes que doivent avoir ses ancêtres (filtre de Bloom).
    ancestor_hashes: [u32; MAX_HASHES],
    ancestor_hash_count: u8,
}

impl Entry {
    /// Le filtre des ancêtres n'exclut pas ce sélecteur.
    fn may_match(&self, filter: &AncestorFilter) -> bool {
        self.ancestor_hashes[..self.ancestor_hash_count as usize]
            .iter()
            .all(|&h| filter.might_contain(h))
    }
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
    /// Les feuilles de l'arbre fantôme de chaque hôte (shadow DOM déclaratif) :
    /// leurs règles `:host` s'appliquent à l'hôte.
    shadow_sheets: HashMap<NodeId, Vec<Stylesheet>>,
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
            (
                Origin::UserAgent,
                Some(Namespace::Svg),
                Stylesheet::parse(SVG_CSS, env),
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
                    let (ancestor_hashes, ancestor_hash_count) = ancestor_hashes(selector);
                    let entry = Entry {
                        sheet: s as u32,
                        rule: r as u32,
                        selector: i as u32,
                        order,
                        specificity: selector.specificity(),
                        ancestor_hashes,
                        ancestor_hash_count,
                    };
                    index.insert(selector, entry);
                }
            }
        }
        // Les hôtes du document, puis ceux des arbres fantômes (imbriqués).
        let mut shadow_sheets = HashMap::new();
        let mut trees = vec![NodeId::DOCUMENT];
        while let Some(tree) = trees.pop() {
            for id in doc.descendants(tree) {
                if let Some(root) = doc.element(id).and_then(|e| e.shadow_root) {
                    shadow_sheets.insert(id, stylesheets_under(doc, root, env));
                    trees.push(root);
                }
            }
        }
        StyleEngine {
            sheets,
            index,
            shadow_sheets,
            env: env.clone(),
        }
    }

    /// Calcule le style de tous les éléments, du haut de l'arbre vers le bas.
    ///
    /// On suit l'« arbre plat » (CSS Scoping) : un hôte a pour enfants ceux de
    /// sa racine fantôme, et un `<slot>` les enfants de l'hôte qui lui sont
    /// assignés ; c'est de là que vient l'héritage. Un enfant de l'hôte assigné
    /// à aucun slot n'est pas affiché et n'a pas de style (comme dans Chromium).
    pub fn style_document(&self, doc: &Document) -> Styles {
        /// Une étape du parcours : entrer dans un nœud (avec le style de son
        /// parent et son arbre), ou sortir du sous-arbre d'un élément (ses
        /// empreintes, à retirer du filtre, commencent à cet indice de `hashes`).
        enum Step {
            Enter(NodeId, Option<Rc<ComputedStyle>>, Scope),
            Exit(usize),
        }
        let mut styles = Styles::default();
        let mut root_font_size = 16.0;
        let mut filter = AncestorFilter::default();
        // Les empreintes des ancêtres, en pile : une seule allocation.
        let mut hashes: Vec<u32> = Vec::new();
        // L'arbre de chaque hôte, et les nœuds assignés à chaque slot.
        let mut host_scope: HashMap<NodeId, Scope> = HashMap::new();
        let mut assigned: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        // Une pile plutôt que la récursion : pas de débordement, même sur un DOM
        // très profond.
        let mut stack: Vec<Step> = doc
            .children(NodeId::DOCUMENT)
            .map(|c| Step::Enter(c, None, Scope::Document))
            .collect();
        stack.reverse();
        while let Some(step) = stack.pop() {
            let (id, parent, scope) = match step {
                Step::Enter(id, parent, scope) => (id, parent, scope),
                Step::Exit(start) => {
                    filter.pop(&hashes[start..]);
                    hashes.truncate(start);
                    continue;
                }
            };
            let Some(element) = doc.element(id) else {
                continue;
            };
            let is_root = parent.is_none();
            let ctx = Context {
                viewport_width: self.env.width,
                viewport_height: self.env.height,
                device_pixel_ratio: self.env.resolution,
                root_font_size,
                is_root,
                parent_is_flex_or_grid: parent.as_ref().is_some_and(|p| {
                    matches!(
                        p.get("display").map(|d| d.to_string()).as_deref(),
                        Some("flex" | "inline-flex" | "grid" | "inline-grid")
                    )
                }),
            };
            let style =
                Rc::new(self.style_element(doc, id, parent.as_deref(), &ctx, &filter, scope));
            if is_root {
                root_font_size = style.font_size.px;
            }
            // Les enfants dans l'arbre plat.
            let children: Vec<(NodeId, Scope)> = if let Some(root) = element.shadow_root {
                host_scope.insert(id, scope);
                assign_slots(doc, id, root, &mut assigned);
                doc.children(root).map(|c| (c, Scope::Shadow(id))).collect()
            } else if let (Scope::Shadow(host), Some(nodes)) = (scope, assigned.get(&id)) {
                // Un slot qui a reçu des nœuds de l'hôte.
                let host_scope = host_scope.get(&host).copied().unwrap_or(Scope::Document);
                nodes.iter().map(|&n| (n, host_scope)).collect()
            } else {
                doc.children(id).map(|c| (c, scope)).collect()
            };
            // L'élément devient un ancêtre pour ses enfants.
            if !children.is_empty() {
                let start = hashes.len();
                filter.push(&DomElement::new(doc, id), &mut hashes);
                stack.push(Step::Exit(start));
                for (c, child_scope) in children.into_iter().rev() {
                    stack.push(Step::Enter(c, Some(style.clone()), child_scope));
                }
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
        filter: &AncestorFilter,
        scope: Scope,
    ) -> Vec<(Priority, &'s Declaration)> {
        let element = DomElement::new(doc, id);
        let ns = element.element().ns;
        let in_shadow = matches!(scope, Scope::Shadow(_));
        let mut candidates = Vec::new();
        self.index.candidates(&element, &mut candidates);
        // Les sélecteurs qui correspondent vraiment : le filtre des ancêtres
        // écarte d'abord la plupart des autres sans remonter l'arbre.
        let mut matched: Vec<Entry> = candidates
            .into_iter()
            .filter(|e| {
                let (origin, target, sheet) = &self.sheets[e.sheet as usize];
                // Dans un arbre fantôme, les feuilles du document ne s'appliquent pas.
                (!in_shadow || *origin == Origin::UserAgent)
                    && target.is_none_or(|t| t == ns)
                    && e.may_match(filter)
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
                    priority(
                        *origin,
                        d.important,
                        TreeContext::Own,
                        false,
                        e.specificity,
                        e.order,
                    ),
                    d,
                ));
            }
        }
        // Dans un arbre fantôme : les feuilles de cet arbre (peu de règles, pas
        // d'index). Pour un hôte : les règles `:host` de son propre arbre fantôme.
        let own = match scope {
            Scope::Shadow(host) => self.shadow_sheets.get(&host),
            Scope::Document => None,
        };
        let inner = self.shadow_sheets.get(&id);
        let mut order = 0;
        for (sheets, context) in [(own, TreeContext::Own), (inner, TreeContext::Inner)] {
            for rule in sheets.into_iter().flatten().flat_map(|s| &s.rules) {
                order += 1;
                let spec = rule
                    .selectors
                    .0
                    .iter()
                    .filter(|s| match context {
                        TreeContext::Own => s.matches(element),
                        TreeContext::Inner => s.matches_as_host(element),
                    })
                    .map(|s| s.specificity())
                    .max();
                let Some(spec) = spec else { continue };
                for d in &rule.declarations {
                    found.push((
                        priority(Origin::Author, d.important, context, false, spec, order),
                        d,
                    ));
                }
            }
        }
        let order = u32::MAX;
        for d in inline {
            found.push((
                priority(
                    Origin::Author,
                    d.important,
                    TreeContext::Own,
                    true,
                    (0, 0, 0),
                    order,
                ),
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
        filter: &AncestorFilter,
        scope: Scope,
    ) -> ComputedStyle {
        let inline = DomElement::new(doc, id)
            .attribute("style")
            .map(parse_style_attribute)
            .unwrap_or_default();
        let declarations = self.matching_declarations(doc, id, &inline, filter, scope);

        // Les propriétés personnalisées d'abord : les autres peuvent en dépendre.
        let empty = CustomProperties::default();
        let parent_custom = parent.map_or(&empty, |p| &*p.custom);
        let custom = CustomProperties::compute(
            declarations
                .iter()
                .filter(|(_, d)| d.parsed == Parsed::Custom)
                .map(|(_, d)| (d.name.as_str(), d.value.as_str())),
            parent_custom,
        );
        let custom = match parent {
            Some(p) if *p.custom == custom => p.custom.clone(),
            _ => Rc::new(custom),
        };

        // La déclaration gagnante de chaque propriété longue (la dernière).
        let mut cascaded = vec![Cascaded::None; PROPERTIES.len()];
        let mut set = |name: &str, value: Cascaded| {
            if let Some(i) = property_index(name) {
                cascaded[i] = value;
            }
        };
        for (_, d) in &declarations {
            match &d.parsed {
                Parsed::Custom | Parsed::Ignored => {}
                Parsed::Longhands(longhands) => {
                    for (name, value) in longhands {
                        set(name, Cascaded::Value(value.clone()));
                    }
                }
                Parsed::Keyword(keyword, targets) => {
                    let keyword = match *keyword {
                        "initial" => Cascaded::Initial,
                        "inherit" => Cascaded::Inherit,
                        _ => Cascaded::Unset,
                    };
                    for name in targets {
                        set(name, keyword.clone());
                    }
                }
                Parsed::Var(targets) => {
                    let substituted = substitute(&d.value, &custom).and_then(|text| {
                        parse_property(&d.name, &Parser::new(&text).parse_component_value_list())
                    });
                    match substituted {
                        Some(longhands) => {
                            for (name, value) in longhands {
                                set(name, Cascaded::Value(value));
                            }
                        }
                        // Invalide après substitution : `unset`.
                        None => {
                            for name in targets {
                                set(name, Cascaded::Unset);
                            }
                        }
                    }
                }
            }
        }
        compute(&cascaded, parent, custom, ctx)
    }
}

/// La valeur d'un attribut (sans espace de noms) d'un élément.
fn attribute<'d>(doc: &'d Document, id: NodeId, name: &str) -> Option<&'d str> {
    doc.element(id)?
        .attrs
        .iter()
        .find(|a| a.ns == AttrNamespace::None && a.name == name)
        .map(|a| a.value.as_str())
}

/// Assigne les enfants de `host` (éléments et textes) aux `<slot>` de son arbre
/// fantôme : au premier slot dont le nom (`name`, vide par défaut) est celui de
/// leur attribut `slot` (vide par défaut). DOM, « find a slot ».
fn assign_slots(
    doc: &Document,
    host: NodeId,
    root: NodeId,
    assigned: &mut HashMap<NodeId, Vec<NodeId>>,
) {
    let mut slots: HashMap<&str, NodeId> = HashMap::new();
    for id in doc.descendants(root) {
        let Some(e) = doc.element(id) else { continue };
        if e.ns == Namespace::Html && doc.atoms.name(e.name) == "slot" {
            let name = attribute(doc, id, "name").unwrap_or("");
            slots.entry(name).or_insert(id);
        }
    }
    for child in doc.children(host) {
        let name = match &doc.node(child).data {
            NodeData::Element(_) => attribute(doc, child, "slot").unwrap_or(""),
            NodeData::Text(_) => "",
            _ => continue,
        };
        if let Some(&slot) = slots.get(name) {
            assigned.entry(slot).or_default().push(child);
        }
    }
}

/// Les enfants d'un nœud dans l'« arbre plat » (CSS Scoping) : pour un hôte,
/// ceux de sa racine fantôme ; pour un `<slot>`, les nœuds qui lui sont
/// assignés (ou, s'il n'y en a pas, son contenu par défaut) ; sinon ses enfants.
/// C'est l'arbre que suivent l'héritage et la mise en page.
pub fn flat_children(doc: &Document, id: NodeId) -> Vec<NodeId> {
    if let Some(root) = doc.element(id).and_then(|e| e.shadow_root) {
        return doc.children(root).collect();
    }
    let is_slot = doc
        .element(id)
        .is_some_and(|e| e.ns == Namespace::Html && doc.atoms.name(e.name) == "slot");
    if is_slot {
        // L'hôte de l'arbre fantôme qui contient ce slot.
        let mut current = doc.node(id).parent;
        while let Some(n) = current {
            if let NodeData::ShadowRoot(info) = &doc.node(n).data {
                let mut assigned = HashMap::new();
                assign_slots(doc, info.host, n, &mut assigned);
                if let Some(nodes) = assigned.remove(&id) {
                    return nodes;
                }
                break;
            }
            current = doc.node(n).parent;
        }
    }
    doc.children(id).collect()
}

/// Raccourci : le style calculé de tous les éléments de `doc` dans `env`.
pub fn style_document(doc: &Document, env: &Environment) -> Styles {
    StyleEngine::new(doc, env).style_document(doc)
}
