//! Programme à lancer sous un profileur : tokenise une page en boucle.
//! Par défaut la page "blog" générée ; PAGE=benches/pages/xxx.html pour une vraie page.
//!
//!   cargo build --profile profiling --example profile_blog
//!   samply record target/profiling/examples/profile_blog

#[path = "../benches/docs/mod.rs"]
mod docs;

use std::hint::black_box;
use std::time::Instant;

fn main() {
    let html = match std::env::var("PAGE") {
        Ok(path) => std::fs::read_to_string(&path).expect("page introuvable"),
        Err(_) => docs::blog_page(1500),
    };
    // Assez de tours pour environ 1 Go traité (le profileur a le temps de mesurer).
    let iterations = (1_000_000_000 / html.len()).max(300);
    let start = Instant::now();
    let mut tokens = 0;
    for _ in 0..iterations {
        tokens += html_parseur::Tokenizer::new(black_box(&html)).count();
    }
    let secs = start.elapsed().as_secs_f64();
    let mb = (html.len() * iterations) as f64 / (1024.0 * 1024.0);
    println!("{tokens} tokens, {:.1} Mo/s", mb / secs);
}
