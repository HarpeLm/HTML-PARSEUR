//! Les polices installées, et quelques mesures.
//!
//!   cargo run --release -p lumen-font --example polices [famille] [texte] [taille]

use std::time::Instant;

use lumen_font::FontDatabase;

fn main() {
    let t = Instant::now();
    let db = FontDatabase::system();
    println!(
        "{} polices trouvées en {:.0} ms",
        db.faces().len(),
        t.elapsed().as_secs_f64() * 1e3
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    let text = args.get(1).map_or("Hello", String::as_str);
    let size: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(16.0);
    let families: Vec<&str> = match args.first() {
        Some(f) => vec![f.as_str()],
        None => vec![
            "Arial",
            "serif",
            "sans-serif",
            "monospace",
            "Times New Roman",
            "Georgia",
        ],
    };
    for family in families {
        match db.query(family, 400, false) {
            Some(font) => {
                let m = font.hhea;
                println!(
                    "{family:<16} -> {} {} ({} unités/em) : « {text} » à {size}px = {} px ; hhea {}/{}/{}, arrondis {} / {}",
                    font.family,
                    font.subfamily,
                    font.units_per_em,
                    font.text_width(text, size),
                    m.ascender,
                    m.descender,
                    m.line_gap,
                    font.to_px(m.ascender as f64, size).round(),
                    font.to_px(-m.descender as f64, size).round(),
                );
            }
            None => println!("{family:<16} -> introuvable"),
        }
    }
}
