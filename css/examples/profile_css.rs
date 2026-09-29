//! Programme à lancer sous un profileur : tokenise du CSS en boucle.
//! Par défaut le CSS intégré de la page MDN ; CSS=chemin pour un autre fichier.
//!
//!   ./profiling/profile.sh tokenizer-v0

use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

/// Le CSS des balises <style> d'une page HTML.
fn inline_css(html: &str) -> String {
    let mut css = String::new();
    let mut rest = html;
    while let Some(start) = rest.find("<style") {
        let after = &rest[start..];
        let open_end = after.find('>').unwrap() + 1;
        let close = after.find("</style>").unwrap();
        css.push_str(&after[open_end..close]);
        rest = &after[close..];
    }
    css
}

fn main() {
    let css = match std::env::var("CSS") {
        Ok(path) => std::fs::read_to_string(path).expect("fichier introuvable"),
        Err(_) => {
            let page = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../html/benches/pages/mdn-fr-table.html");
            inline_css(&std::fs::read_to_string(page).unwrap())
        }
    };
    let css = lumen_css::preprocess(&css).into_owned();
    let iterations = (1_000_000_000 / css.len()).max(100);
    let start = Instant::now();
    let mut tokens = 0;
    for _ in 0..iterations {
        tokens += lumen_css::Tokenizer::new(black_box(&css)).count();
    }
    let secs = start.elapsed().as_secs_f64();
    let mb = (css.len() * iterations) as f64 / (1024.0 * 1024.0);
    println!("{tokens} tokens, {:.1} Mo/s", mb / secs);
}
