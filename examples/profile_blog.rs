//! Programme à lancer sous un profileur : tokenise la page "blog" en boucle.
//!
//!   cargo build --profile profiling --example profile_blog
//!   samply record target/profiling/examples/profile_blog

#[path = "../benches/docs/mod.rs"]
mod docs;

use std::hint::black_box;
use std::time::Instant;

fn main() {
    let html = docs::blog_page(1500);
    let iterations = 300;
    let start = Instant::now();
    let mut tokens = 0;
    for _ in 0..iterations {
        tokens += html_tokenizer::Tokenizer::new(black_box(&html)).count();
    }
    let secs = start.elapsed().as_secs_f64();
    let mb = (html.len() * iterations) as f64 / (1024.0 * 1024.0);
    println!("{tokens} tokens, {:.1} Mo/s", mb / secs);
}
