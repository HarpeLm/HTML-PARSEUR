//! Test de robustesse ("fuzzing" maison) : du HTML aléatoire et tordu ne doit
//! jamais faire paniquer le parser, et le DOM produit doit rester cohérent.
//!
//!   cargo test --release --test robustesse                   -> 20 000 pages
//!   FUZZ_CAS=2000000 cargo test --release --test robustesse   -> campagne longue
//!
//! Le générateur est reproductible : un cas qui échoue est rejouable avec sa graine.

use std::panic;

use html_parseur::dom::{Document, Namespace, NodeData, NodeId};
use html_parseur::{ParseOptions, parse_document_with, parse_fragment};

/// Générateur pseudo-aléatoire xorshift64* : simple, rapide, reproductible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

/// Les noms qui déclenchent les cas les plus délicats de la spec.
const TAGS: &[&str] = &[
    "a",
    "b",
    "i",
    "nobr",
    "font",
    "p",
    "div",
    "span",
    "table",
    "tbody",
    "tr",
    "td",
    "th",
    "caption",
    "colgroup",
    "col",
    "template",
    "my-el",
    "svg",
    "math",
    "foreignObject",
    "desc",
    "mi",
    "annotation-xml",
    "select",
    "option",
    "optgroup",
    "button",
    "selectedcontent",
    "form",
    "li",
    "ul",
    "dd",
    "dt",
    "h1",
    "h2",
    "pre",
    "textarea",
    "title",
    "script",
    "style",
    "noscript",
    "plaintext",
    "frameset",
    "frame",
    "body",
    "head",
    "html",
    "input",
    "hr",
    "br",
    "image",
    "xmp",
    "iframe",
    "marquee",
    "object",
    "ruby",
    "rt",
    "rp",
    "applet",
];

const PIECES: &[&str] = &[
    "texte",
    " ",
    "\n",
    "\r\n",
    "\0",
    "&amp;",
    "&eacute",
    "&#0;",
    "&#x110000;",
    "&notit;",
    "&",
    "<",
    ">",
    "<!-- c -->",
    "<!--",
    "-->",
    "<!DOCTYPE html>",
    "<!doctype>",
    "<![CDATA[x]]>",
    "<?xml a?>",
    "<?pi d>",
    "</>",
    "<//>",
    "é€😀",
    "\"",
    "'",
    "=",
];

fn random_html(rng: &mut Rng) -> String {
    let mut html = String::new();
    for _ in 0..rng.below(40) {
        match rng.below(6) {
            0 | 1 => {
                html.push('<');
                html.push_str(rng.pick(TAGS));
                for _ in 0..rng.below(3) {
                    html.push_str(rng.pick(&[
                        " x=1",
                        " type=hidden",
                        " encoding=text/html",
                        " color=red",
                        " selected",
                        " a=\"<b>\"",
                        " shadowrootmode=open",
                        " shadowrootmode=closed",
                    ]));
                }
                html.push_str(rng.pick(&[">", "/>", " >", ""]));
            }
            2 => {
                html.push_str("</");
                html.push_str(rng.pick(TAGS));
                html.push('>');
            }
            _ => html.push_str(rng.pick(PIECES)),
        }
    }
    html
}

/// Vérifie la cohérence de l'arbre : chaque enfant désigne bien son parent.
fn check_tree(doc: &Document, root: NodeId) {
    for node in doc.descendants(root) {
        let parent = doc
            .node(node)
            .parent
            .expect("un descendant a forcément un parent");
        assert!(
            doc.children(parent).any(|c| c == node),
            "enfant absent de son parent"
        );
        // Les arbres fantômes aussi, et chaque racine désigne bien son hôte.
        if let Some(shadow) = doc.element(node).and_then(|e| e.shadow_root) {
            match &doc.node(shadow).data {
                NodeData::ShadowRoot(info) => assert_eq!(info.host, node, "mauvais hôte"),
                _ => panic!("shadow_root n'est pas une racine fantôme"),
            }
            check_tree(doc, shadow);
        }
    }
}

#[test]
fn html_aleatoire_sans_panique() {
    let cases: u64 = std::env::var("FUZZ_CAS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000);
    let contexts = [
        (Namespace::Html, "div"),
        (Namespace::Html, "table"),
        (Namespace::Html, "tr"),
        (Namespace::Html, "template"),
        (Namespace::Html, "select"),
        (Namespace::Html, "title"),
        (Namespace::Svg, "svg"),
        (Namespace::MathMl, "mi"),
    ];
    for seed in 1..=cases {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let html = random_html(&mut rng);
        let scripting = rng.below(2) == 0;
        let (ns, context) = contexts[rng.below(contexts.len())];
        let result = panic::catch_unwind(|| {
            let options = ParseOptions {
                scripting,
                declarative_shadow_roots: seed % 2 == 0,
            };
            let doc = parse_document_with(&html, options);
            check_tree(&doc, NodeId::DOCUMENT);
            let (frag, root) = parse_fragment(&html, ns, context, options);
            check_tree(&frag, root);
        });
        assert!(
            result.is_ok(),
            "panique pour la graine {seed}, contexte <{context}> : {html:?}"
        );
    }
}
