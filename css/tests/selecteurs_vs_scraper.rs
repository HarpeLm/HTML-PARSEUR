//! Test différentiel : nos sélecteurs contre ceux de Servo (via `scraper`), sur
//! les 5 vraies pages de html/benches/pages/. Pour chaque sélecteur, les deux
//! doivent (1) être d'accord sur sa validité et (2) trouver exactement les mêmes
//! éléments.
//!
//!   cargo test --release -p lumen-css --test selecteurs_vs_scraper -- --nocapture

use std::collections::BTreeSet;
use std::path::Path;

use html_parseur::dom::{AttrNamespace, Document, Namespace, NodeData, NodeId};
use lumen_css::selectors::{Element, SelectorList, parse_selector_list};
use lumen_css::{Parser, preprocess};

// ───────────── Notre DOM, vu par les sélecteurs ─────────────

#[derive(Clone, Copy)]
struct El<'d> {
    doc: &'d Document,
    id: NodeId,
}

impl<'d> El<'d> {
    fn element(&self) -> &'d html_parseur::dom::Element {
        self.doc.element(self.id).expect("un élément")
    }

    fn wrap(&self, id: Option<NodeId>) -> Option<Self> {
        id.filter(|&n| self.doc.element(n).is_some())
            .map(|id| El { doc: self.doc, id })
    }

    fn sibling(&self, next: bool) -> Option<Self> {
        let mut current = self.id;
        loop {
            current = if next {
                self.doc.next_sibling(current)?
            } else {
                self.doc.prev_sibling(current)?
            };
            if self.doc.element(current).is_some() {
                return Some(El {
                    doc: self.doc,
                    id: current,
                });
            }
        }
    }
}

impl Element for El<'_> {
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

// ───────────── Sélecteurs à tester ─────────────

/// Sélecteurs écrits à la main : chaque fonctionnalité, et des invalides.
const HANDWRITTEN: &[&str] = &[
    "*",
    "div",
    "DIV",
    "p",
    "a",
    "span",
    "li",
    "ul > li",
    "div p",
    "div > p",
    "h2 + p",
    "h2 ~ p",
    "body *",
    "html",
    "head title",
    ":root",
    ":root > body",
    "p:empty",
    "div:empty",
    "li:first-child",
    "li:last-child",
    "li:only-child",
    "p:first-of-type",
    "p:last-of-type",
    "img:only-of-type",
    "li:nth-child(2n+1)",
    "li:nth-child(odd)",
    "li:nth-child(even)",
    "li:nth-child(3)",
    "li:nth-child(-n+3)",
    "li:nth-last-child(2)",
    "tr:nth-of-type(2n)",
    "td:nth-last-of-type(1)",
    "li:nth-child(2 of .toclevel-1)",
    "a[href]",
    "a[href^=\"http\"]",
    "a[href$=\".pdf\"]",
    "a[href*=wiki]",
    "[lang|=en]",
    "[class~=mw-headline]",
    "[type=\"TEXT\"]",
    "[type=text i]",
    "[id]",
    "[data-x]",
    "a:not([href])",
    "div:not(.x, .y)",
    ":is(h1, h2, h3)",
    ":where(ul, ol) li",
    "p:is(.a, :not(.b))",
    "a:hover",
    "a:focus",
    "input:focus-visible",
    "a:visited",
    "p::before",
    "p:after",
    "#content",
    "div#content",
    ".mw-parser-output p",
    "table.wikitable tr > td",
    "nav ul li a",
    "ol > li:first-child + li",
    "section h2 ~ p a",
    "span.mw-headline",
    "code, pre",
    "h1, h2, h3, h4",
    "meta[name]",
    "link[rel=stylesheet]",
    "script[src]",
    "svg",
    "path",
    "button[aria-label]",
    "input[type=checkbox]",
    // Invalides : doivent être rejetés des deux côtés.
    "",
    "div >",
    "> p",
    "p..a",
    "a[",
    "a[href=]",
    "::before p",
    "p:unknown-pseudo",
    ":not()",
    "li:nth-child(foo)",
    "#1abc",
    "a ,",
    ", a",
    "p!",
];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn pick<'a>(&mut self, items: &'a [String]) -> &'a str {
        &items[(self.next() % items.len() as u64) as usize]
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Sélecteurs générés à partir des balises, classes et ids réels de la page.
fn generated(doc: &Document, count: usize) -> Vec<String> {
    let mut tags = BTreeSet::new();
    let mut classes = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for n in doc.descendants(NodeId::DOCUMENT) {
        let Some(e) = doc.element(n) else { continue };
        if e.ns == Namespace::Html {
            tags.insert(doc.atoms.name(e.name).to_string());
        }
        for a in &e.attrs {
            let simple = |s: &str| {
                s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    && s.starts_with(|c: char| c.is_ascii_alphabetic())
            };
            if a.name == "class" {
                classes.extend(
                    a.value
                        .split_ascii_whitespace()
                        .filter(|c| simple(c))
                        .map(str::to_string),
                );
            } else if a.name == "id" && simple(&a.value) {
                ids.insert(a.value.clone());
            }
        }
    }
    let (tags, classes, ids): (Vec<_>, Vec<_>, Vec<_>) = (
        tags.into_iter().collect(),
        classes.into_iter().collect(),
        ids.into_iter().collect(),
    );
    let pseudos = [
        ":first-child",
        ":last-child",
        ":nth-child(2n)",
        ":nth-child(3n+1)",
        ":nth-of-type(2)",
        ":last-of-type",
        ":empty",
        ":not(:first-child)",
        ":only-child",
    ];
    let mut rng = Rng(0x5EED_CAFE);
    let compound = |rng: &mut Rng| {
        let mut s = String::new();
        match rng.below(4) {
            0 => s.push('*'),
            1 => {}
            _ => s.push_str(rng.pick(&tags)),
        }
        if !classes.is_empty() && rng.below(2) == 0 {
            s.push('.');
            s.push_str(rng.pick(&classes));
        }
        if !ids.is_empty() && rng.below(8) == 0 {
            s.push('#');
            s.push_str(rng.pick(&ids));
        }
        if rng.below(3) == 0 {
            s.push_str(pseudos[rng.below(pseudos.len() as u64) as usize]);
        }
        if s.is_empty() {
            s.push_str(rng.pick(&tags));
        }
        s
    };
    (0..count)
        .map(|_| {
            let mut s = compound(&mut rng);
            for _ in 0..rng.below(3) {
                s = format!(
                    "{}{}{s}",
                    compound(&mut rng),
                    [" ", " > ", " + ", " ~ "][rng.below(4) as usize]
                );
            }
            s
        })
        .collect()
}

// ───────────── Comparaison ─────────────

/// Fonctionnalités valides selon la spec, mais que scraper refuse : états
/// dynamiques et pseudo-éléments (inutiles pour extraire des données), et
/// `:nth-child(... of S)` (absent de sa version de `selectors`).
fn unsupported_by_scraper(selector: &str) -> bool {
    [
        ":hover", ":focus", ":visited", ":active", "::", ":before", ":after", " of ",
    ]
    .iter()
    .any(|f| selector.contains(f))
}

fn ours_parse(selector: &str) -> Option<SelectorList> {
    let css = preprocess(selector);
    let values = Parser::new(&css).parse_component_value_list();
    parse_selector_list(&values)
}

#[test]
fn memes_elements_que_servo() {
    let pages = Path::new(env!("CARGO_MANIFEST_DIR")).join("../html/benches/pages");
    let mut total_compared = 0;
    let mut not_comparable = 0;
    let mut failures = Vec::new();

    for name in [
        "wikipedia-fr-rust",
        "wikipedia-en-html",
        "whatwg-parsing",
        "rust-doc-vec",
        "mdn-fr-table",
    ] {
        let html = std::fs::read_to_string(pages.join(format!("{name}.html"))).unwrap();

        // Nos éléments, dans l'ordre du document. scraper (html5ever) parse avec
        // JavaScript activé : <noscript> devient du texte. On fait pareil.
        let doc = html_parseur::parse_document_with(
            &html,
            html_parseur::ParseOptions {
                scripting: true,
                ..Default::default()
            },
        );
        let ours: Vec<El> = doc
            .descendants(NodeId::DOCUMENT)
            .filter(|&n| doc.element(n).is_some())
            .map(|id| El { doc: &doc, id })
            .collect();

        // Ceux de scraper, hors contenu des <template> (rangé ailleurs chez nous).
        let theirs_doc = scraper::Html::parse_document(&html);
        let in_fragment =
            |n: ego_tree::NodeRef<scraper::Node>| n.ancestors().any(|a| a.value().is_fragment());
        let theirs: Vec<_> = theirs_doc
            .tree
            .root()
            .descendants()
            .filter(|n| n.value().is_element() && !in_fragment(*n))
            .map(|n| n.id())
            .collect();
        if ours.len() != theirs.len() {
            // Premier élément qui diffère, pour comprendre pourquoi.
            let names_ours: Vec<&str> = ours.iter().map(|e| e.local_name()).collect();
            let names_theirs: Vec<String> = theirs
                .iter()
                .map(|id| {
                    theirs_doc
                        .tree
                        .get(*id)
                        .unwrap()
                        .value()
                        .as_element()
                        .unwrap()
                        .name()
                        .to_string()
                })
                .collect();
            let first = (0..names_ours.len())
                .find(|&i| names_theirs.get(i).map(String::as_str) != Some(names_ours[i]));
            if let Some(i) = first {
                let parent = ours[i]
                    .parent_element()
                    .map(|p| p.local_name().to_string())
                    .unwrap_or_default();
                println!(
                    "{name} : premier écart à l'élément {i} : nous <{}> (parent <{parent}>), Servo <{}>",
                    names_ours[i],
                    names_theirs.get(i).map(String::as_str).unwrap_or("-")
                );
            }
            println!(
                "{name} : arbres différents ({} contre {} éléments), page ignorée",
                ours.len(),
                theirs.len()
            );
            continue;
        }

        let mut selectors: Vec<String> = HANDWRITTEN.iter().map(|s| s.to_string()).collect();
        selectors.extend(generated(&doc, 300));
        let mut compared = 0;
        for selector in &selectors {
            let mine = ours_parse(selector);
            let servo = scraper::Selector::parse(selector).ok();
            match (&mine, &servo) {
                (None, None) => continue,
                (Some(_), None) if unsupported_by_scraper(selector) => {
                    not_comparable += 1;
                    continue;
                }
                (Some(_), None) | (None, Some(_)) => {
                    failures.push(format!(
                        "[{name}] {selector:?} : validité différente (nous : {}, Servo : {})",
                        mine.is_some(),
                        servo.is_some()
                    ));
                    continue;
                }
                (Some(m), Some(s)) => {
                    let ours_matched: Vec<usize> = ours
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| m.matches(**e))
                        .map(|(i, _)| i)
                        .collect();
                    let theirs_ids: BTreeSet<_> = theirs_doc.select(s).map(|e| e.id()).collect();
                    let theirs_matched: Vec<usize> = theirs
                        .iter()
                        .enumerate()
                        .filter(|(_, id)| theirs_ids.contains(id))
                        .map(|(i, _)| i)
                        .collect();
                    compared += 1;
                    if ours_matched != theirs_matched {
                        failures.push(format!(
                            "[{name}] {selector:?} : {} éléments chez nous, {} chez Servo",
                            ours_matched.len(),
                            theirs_matched.len()
                        ));
                    }
                }
            }
        }
        println!(
            "{name} : {} éléments, {compared} sélecteurs comparés",
            ours.len()
        );
        total_compared += compared;
    }

    for f in &failures {
        println!("❌ {f}");
    }
    println!(
        "{total_compared} comparaisons, {} différences ({not_comparable} sélecteurs valides non gérés par scraper)",
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{} différences avec Servo",
        failures.len()
    );
}
