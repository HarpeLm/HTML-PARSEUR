//! Programme à lancer sous un profileur : parse une page en boucle
//! (tokenizer + construction du DOM).
//!
//!   ./profiling/profile.sh parse-v0 profile_parse            (page blog)
//!   DOC=texte ./profiling/profile.sh texte-v0 profile_parse  (page texte)

#[path = "../benches/docs/mod.rs"]
mod docs;

use std::hint::black_box;
use std::time::Instant;

fn main() {
    let (html, iterations) = match std::env::var("DOC").as_deref() {
        Ok("texte") => (docs::text_heavy_page(700), 20_000),
        Ok("balises") => (docs::tag_heavy_page(8000), 300),
        _ => (docs::blog_page(1500), 500),
    };
    let start = Instant::now();
    let mut nodes = 0;
    for _ in 0..iterations {
        let doc = html_tokenizer::parse_document(black_box(&html));
        nodes += doc.descendants(html_tokenizer::dom::NodeId::DOCUMENT).count();
    }
    let secs = start.elapsed().as_secs_f64();
    let mb = (html.len() * iterations) as f64 / (1024.0 * 1024.0);
    println!("{nodes} nœuds, {:.1} Mo/s", mb / secs);
}
