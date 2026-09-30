//! Temps de calcul du style : lumen-style contre Stylo (le moteur de style de
//! Firefox, ici via blitz-dom), sur les 5 vraies pages de html/benches/pages/.
//!
//! Pour chaque moteur, on mesure seulement le calcul des styles (pas le parsing
//! du HTML), médiane de 20 passages, fenêtre de 1024 × 768.
//! - lumen : lecture des feuilles + cascade ;
//! - Stylo : `resolve_stylist` = indexation des règles + parcours de l'arbre
//!   (les feuilles sont déjà parsées à la construction du document).

use std::time::Instant;

use blitz_dom::{DocumentConfig, StyleThreading};
use blitz_html::HtmlDocument;
use blitz_traits::shell::{ColorScheme, Viewport};
use html_parseur::{ParseOptions, parse_document_with};
use lumen_css::media::Environment;
use lumen_style::StyleEngine;

const PAGES: &[&str] = &[
    "wikipedia-fr-rust",
    "wikipedia-en-html",
    "whatwg-parsing",
    "rust-doc-vec",
    "mdn-fr-table",
];
const RUNS: usize = 20;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn lumen(html: &str) -> (f64, f64, usize) {
    let env = Environment {
        width: 1024.0,
        height: 768.0,
        ..Environment::default()
    };
    let doc = parse_document_with(html, ParseOptions {
            scripting: true,
            declarative_shadow_roots: true,
        });
    let (mut sheets, mut cascade, mut n) = (Vec::new(), Vec::new(), 0);
    for _ in 0..RUNS {
        let t = Instant::now();
        let engine = StyleEngine::new(&doc, &env);
        sheets.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        let styles = engine.style_document(&doc);
        cascade.push(t.elapsed().as_secs_f64() * 1e3);
        n = styles.len();
    }
    (median(sheets), median(cascade), n)
}

fn stylo(html: &str, threading: StyleThreading) -> (f64, usize) {
    let mut times = Vec::new();
    let mut n = 0;
    for _ in 0..RUNS {
        let config = DocumentConfig {
            viewport: Some(Viewport::new(1024, 768, 1.0, ColorScheme::Light)),
            style_threading: threading,
            // Aucun réseau (pas de fournisseur) : les <link> ne sont pas chargés,
            // comme dans Lumen. L'URL sert seulement à résoudre les liens.
            base_url: Some("https://exemple.invalid/".into()),
            ..DocumentConfig::default()
        };
        let mut doc = HtmlDocument::from_html(html, config).into_inner();
        let t = Instant::now();
        doc.resolve_stylist(0.0);
        times.push(t.elapsed().as_secs_f64() * 1e3);
        n = doc
            .tree()
            .iter()
            .filter(|(_, node)| node.is_element() && node.primary_styles().is_some())
            .count();
    }
    (median(times), n)
}

fn main() {
    println!(
        "{:<18} {:>8} {:>13} {:>13} {:>12} {:>13} {:>13}",
        "page", "éléments", "lumen feuil.", "lumen casc.", "lumen total", "Stylo 1 fil", "Stylo multi"
    );
    for page in PAGES {
        let path = format!("{}/../../html/benches/pages/{page}.html", env!("CARGO_MANIFEST_DIR"));
        let html = std::fs::read_to_string(path).unwrap();
        let (sheets, cascade, n) = lumen(&html);
        let (seq, n_stylo) = stylo(&html, StyleThreading::Sequential);
        let (par, _) = stylo(&html, StyleThreading::Parallel);
        println!(
            "{page:<18} {n:>8} {sheets:>10.2} ms {cascade:>10.2} ms {:>9.2} ms {seq:>10.2} ms {par:>10.2} ms   (Stylo : {n_stylo} éléments)",
            sheets + cascade
        );
    }
}
