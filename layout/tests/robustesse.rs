//! Robustesse : des pages aléatoires (blocs imbriqués, styles tirés au hasard)
//! et les 5 vraies pages ne doivent ni faire paniquer la mise en page, ni
//! produire de valeur absurde (NaN, infini).
//!
//!   FUZZ_CAS=100000 cargo test --release -p lumen-layout --test robustesse

use std::panic;

use html_parseur::dom::NodeId;
use html_parseur::{ParseOptions, parse_document_with};
use lumen_css::media::Environment;
use lumen_layout::layout_document;
use lumen_style::style_document;

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

const DECLARATIONS: &[&str] = &[
    "height: 10px",
    "height: 0",
    "height: 50%",
    "height: calc(20% + 5px)",
    "width: 120px",
    "width: 150%",
    "width: 0",
    "width: calc(100% - 50px)",
    "min-width: 300px",
    "max-width: 40px",
    "min-height: 30px",
    "max-height: 5px",
    "margin: 10px",
    "margin: -15px 0",
    "margin: auto",
    "margin: 0 auto",
    "margin-top: 1e9px",
    "margin-bottom: -1e9px",
    "margin: 5% 10%",
    "padding: 7px",
    "padding: 10% 0",
    "border: 3px solid",
    "border: 0.3px dotted",
    "border-top: thick double",
    "box-sizing: border-box",
    "overflow: hidden",
    "display: flow-root",
    "display: none",
    "display: contents",
    "display: inline",
    "display: list-item",
    "float: left",
    "position: absolute",
];

fn random_page(rng: &mut Rng) -> String {
    let mut html = String::from("<!DOCTYPE html><body style='margin: 0'>");
    let mut depth = 0;
    for _ in 0..rng.below(40) {
        match rng.below(4) {
            0 if depth > 0 => {
                html.push_str("</div>");
                depth -= 1;
            }
            1 => html.push_str(rng.pick(&[" ", "texte", "<span>x</span>", "\n"])),
            _ => {
                html.push_str("<div style='");
                for _ in 0..rng.below(4) {
                    html.push_str(rng.pick(DECLARATIONS));
                    html.push(';');
                }
                html.push_str("'>");
                depth += 1;
            }
        }
    }
    html
}

fn check(html: &str, env: &Environment) {
    let doc = parse_document_with(
        html,
        ParseOptions {
            scripting: true,
            declarative_shadow_roots: true,
        },
    );
    let styles = style_document(&doc, env);
    let layout = layout_document(&doc, &styles, env.width, env.height);
    for id in doc.descendants(NodeId::DOCUMENT) {
        if let Some(r) = layout.border_box(id) {
            for v in [r.x, r.y, r.width, r.height] {
                assert!(v.is_finite(), "valeur non finie : {r:?}");
            }
            assert!(r.width >= 0.0 && r.height >= 0.0, "taille négative : {r:?}");
        }
    }
}

#[test]
fn pages_aleatoires_sans_panique() {
    let cases: u64 = std::env::var("FUZZ_CAS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5_000);
    let env = Environment {
        width: 800.0,
        height: 600.0,
        ..Environment::default()
    };
    for seed in 1..=cases {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let html = random_page(&mut rng);
        let result = panic::catch_unwind(|| check(&html, &env));
        assert!(result.is_ok(), "problème pour la graine {seed} : {html:?}");
    }
}

#[test]
fn vraies_pages_sans_panique() {
    let env = Environment {
        width: 1024.0,
        height: 768.0,
        ..Environment::default()
    };
    for page in [
        "wikipedia-fr-rust",
        "wikipedia-en-html",
        "whatwg-parsing",
        "rust-doc-vec",
        "mdn-fr-table",
    ] {
        let path = format!(
            "{}/../html/benches/pages/{page}.html",
            env!("CARGO_MANIFEST_DIR")
        );
        check(&std::fs::read_to_string(path).unwrap(), &env);
    }
}
