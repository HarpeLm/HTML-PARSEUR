//! Test différentiel contre Chromium : la mesure du texte.
//!
//! Pour chaque cas de tests/oracle/mesures_cases.json (famille, taille, texte),
//! tests/oracle/mesures_chromium.json contient ce que mesure Chromium
//! (`measureText` d'un canvas, sans crénage ni ligatures) : la largeur, et les
//! hauteurs au-dessus et au-dessous de la ligne de base. On mesure la même chose
//! avec les mêmes fichiers de police, ceux installés sur la machine.
//!
//! Les polices ne font pas partie du dépôt : une famille absente de la machine
//! est signalée et sautée (les mesures ont été prises sur macOS).
//!
//!   cargo test --release -p lumen-font --test oracle_mesures -- --nocapture

use std::collections::BTreeSet;
use std::path::Path;

use lumen_font::FontDatabase;
use serde_json::Value;

#[test]
fn memes_mesures_que_chromium() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/mesures_chromium.json");
    let oracle: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    println!("Référence : {}", oracle["browser"]);
    let db = FontDatabase::system();

    let mut failures = Vec::new();
    let mut missing = BTreeSet::new();
    let mut checked = 0;
    for r in oracle["results"].as_array().unwrap() {
        let family = r["family"].as_str().unwrap();
        let size = r["size"].as_f64().unwrap();
        let text = r["text"].as_str().unwrap();
        let Some(font) = db.query(family, 400, false) else {
            missing.insert(family.to_string());
            continue;
        };
        checked += 1;
        let width = font.text_width(text, size);
        let metrics = font.line_metrics(size);
        let (w, a, d) = (
            r["width"].as_f64().unwrap(),
            r["ascent"].as_f64().unwrap(),
            r["descent"].as_f64().unwrap(),
        );
        // Chromium calcule les largeurs en flottants 32 bits.
        let close = (width - w).abs() <= 1e-4 * w.max(1.0);
        if !close || metrics.ascent != a || metrics.descent != d {
            failures.push(format!(
                "{family} {size}px {text:?} : Chromium {w} ({a} / {d}), nous {width} ({} / {})",
                metrics.ascent, metrics.descent
            ));
        }
    }
    for f in &failures {
        println!("❌ {f}");
    }
    if !missing.is_empty() {
        println!("⏭️  polices absentes de cette machine : {missing:?}");
    }
    println!(
        "{} / {checked} mesures identiques à Chromium",
        checked - failures.len()
    );
    assert!(
        failures.is_empty(),
        "{} différences avec Chromium",
        failures.len()
    );
}
