//! Boucle sur le calcul du style d'une page, pour le profileur.
//!
//!   cargo build --profile profiling -p lumen-style --example profile_style
//!   ./target/profiling/examples/profile_style wikipedia-en-html 10

use html_parseur::{ParseOptions, parse_document_with};
use lumen_css::media::Environment;
use lumen_style::StyleEngine;

fn main() {
    let mut args = std::env::args().skip(1);
    let page = args.next().unwrap_or_else(|| "wikipedia-en-html".into());
    let seconds: f64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let path = format!(
        "{}/../html/benches/pages/{page}.html",
        env!("CARGO_MANIFEST_DIR")
    );
    let html = std::fs::read_to_string(path).unwrap();
    let doc = parse_document_with(&html, ParseOptions { scripting: true });
    let env = Environment {
        width: 1024.0,
        height: 768.0,
        ..Environment::default()
    };
    let start = std::time::Instant::now();
    let mut runs = 0;
    while start.elapsed().as_secs_f64() < seconds {
        let engine = StyleEngine::new(&doc, &env);
        std::hint::black_box(engine.style_document(&doc));
        runs += 1;
    }
    println!(
        "{runs} passages, {:.2} ms par passage",
        start.elapsed().as_secs_f64() * 1e3 / runs as f64
    );
}
