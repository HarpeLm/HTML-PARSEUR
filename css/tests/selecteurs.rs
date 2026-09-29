//! Ce que le test différentiel avec scraper ne peut pas vérifier :
//! spécificité, `:nth-child(... of S)`, pseudo-éléments, états dynamiques.

use html_parseur::dom::{AttrNamespace, Document, NodeData, NodeId};
use lumen_css::selectors::{Element, SelectorList, parse_selector_list};
use lumen_css::{Parser, preprocess};

fn parse(selector: &str) -> Option<SelectorList> {
    let css = preprocess(selector);
    parse_selector_list(&Parser::new(&css).parse_component_value_list())
}

#[derive(Clone, Copy)]
struct El<'d>(&'d Document, NodeId);

impl Element for El<'_> {
    fn local_name(&self) -> &str {
        self.0.atoms.name(self.0.element(self.1).unwrap().name)
    }
    fn is_html(&self) -> bool {
        true
    }
    fn attribute(&self, name: &str) -> Option<&str> {
        let e = self.0.element(self.1).unwrap();
        e.attrs
            .iter()
            .find(|a| a.ns == AttrNamespace::None && a.name == name)
            .map(|a| a.value.as_str())
    }
    fn parent_element(&self) -> Option<Self> {
        self.0
            .node(self.1)
            .parent
            .filter(|&p| self.0.element(p).is_some())
            .map(|p| El(self.0, p))
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        let mut n = self.1;
        loop {
            n = self.0.prev_sibling(n)?;
            if self.0.element(n).is_some() {
                return Some(El(self.0, n));
            }
        }
    }
    fn next_sibling_element(&self) -> Option<Self> {
        let mut n = self.1;
        loop {
            n = self.0.next_sibling(n)?;
            if self.0.element(n).is_some() {
                return Some(El(self.0, n));
            }
        }
    }
    fn is_empty(&self) -> bool {
        !self.0.children(self.1).any(|c| {
            matches!(
                self.0.node(c).data,
                NodeData::Element(_) | NodeData::Text(_)
            )
        })
    }
    fn same_as(&self, other: &Self) -> bool {
        self.1 == other.1
    }
}

/// Les textes des éléments qui correspondent au sélecteur.
fn select(html: &str, selector: &str) -> Vec<String> {
    let doc = html_parseur::parse_document(html);
    let list = parse(selector).expect("sélecteur valide");
    doc.descendants(NodeId::DOCUMENT)
        .filter(|&n| doc.element(n).is_some() && list.matches(El(&doc, n)))
        .map(|n| doc.children(n).filter_map(|c| doc.text(c)).collect())
        .collect()
}

#[test]
fn specificite() {
    // Exemples de la spec Selectors 4, §17.
    let cases = [
        ("*", (0, 0, 0)),
        ("li", (0, 0, 1)),
        ("ul li", (0, 0, 2)),
        ("ul ol+li", (0, 0, 3)),
        ("h1 + *[rel=up]", (0, 1, 1)),
        ("ul ol li.red", (0, 1, 3)),
        ("li.red.level", (0, 2, 1)),
        ("#x34y", (1, 0, 0)),
        ("#s12:not(foo)", (1, 0, 1)),
        (".foo :is(.bar, #baz)", (1, 1, 0)),
        (":where(#a, .b) p", (0, 0, 1)),
        ("li:nth-child(2 of .x)", (0, 2, 1)),
        ("p::before", (0, 0, 2)),
    ];
    for (selector, expected) in cases {
        let list = parse(selector).unwrap_or_else(|| panic!("{selector} devrait être valide"));
        assert_eq!(list.0[0].specificity(), expected, "{selector}");
    }
}

#[test]
fn nth_child_of_selecteur() {
    let html = "<ul><li class=x>1<li>2<li class=x>3<li class=x>4<li>5</ul>";
    // Le 2e parmi les .x (et pas le 2e enfant).
    assert_eq!(select(html, "li:nth-child(2 of .x)"), ["3"]);
    assert_eq!(select(html, "li:nth-last-child(1 of .x)"), ["4"]);
    assert_eq!(select(html, "li:nth-child(2)"), ["2"]);
}

#[test]
fn pseudo_elements_et_etats_dynamiques() {
    let html = "<p>a</p><a href=x>lien</a>";
    // Valides, mais ne sélectionnent jamais l'élément lui-même.
    assert!(select(html, "p::before").is_empty());
    assert!(select(html, "p:after").is_empty());
    assert!(select(html, "a:hover").is_empty());
    // :not(:hover) est donc vrai.
    assert_eq!(select(html, "a:not(:hover)"), ["lien"]);
    // Un pseudo-élément doit être à la fin.
    assert!(parse("p::before span").is_none());
}

#[test]
fn is_et_where_sont_tolerants() {
    // Une partie invalide est ignorée dans :is() / :where(), pas dans :not().
    assert!(parse(":is(p, :inconnu)").is_some());
    assert!(parse(":where(p, !!)").is_some());
    assert!(parse(":not(p, :inconnu)").is_none());
    assert_eq!(select("<p>a</p><div>b</div>", ":is(p, :inconnu)"), ["a"]);
}
