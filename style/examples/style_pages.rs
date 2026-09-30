//! Temps de calcul du style sur les 5 vraies pages (médiane de 20 passages).
//!
//!   cargo run --release -p lumen-style --example style_pages

use std::time::Instant;

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

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let env = Environment {
        width: 1024.0,
        height: 768.0,
        ..Environment::default()
    };
    println!(
        "{:<20} {:>9} {:>9} {:>12} {:>12} {:>14}",
        "page", "éléments", "règles", "feuilles ms", "cascade ms", "µs / élément"
    );
    for page in PAGES {
        let path = format!(
            "{}/../html/benches/pages/{page}.html",
            env!("CARGO_MANIFEST_DIR")
        );
        let html = std::fs::read_to_string(path).unwrap();
        let doc = parse_document_with(
            &html,
            ParseOptions {
                scripting: true,
                declarative_shadow_roots: true,
            },
        );
        let (mut sheets, mut cascade) = (Vec::new(), Vec::new());
        let mut counts = (0, 0);
        for _ in 0..20 {
            let t = Instant::now();
            let engine = StyleEngine::new(&doc, &env);
            sheets.push(t.elapsed().as_secs_f64() * 1000.0);
            let t = Instant::now();
            let styles = engine.style_document(&doc);
            cascade.push(t.elapsed().as_secs_f64() * 1000.0);
            counts = (styles.len(), engine.rule_count());
        }
        let cascade = median(cascade);
        println!(
            "{page:<20} {:>9} {:>9} {:>12.2} {:>12.2} {:>14.2}",
            counts.0,
            counts.1,
            median(sheets),
            cascade,
            cascade * 1000.0 / counts.0 as f64
        );
    }
}
