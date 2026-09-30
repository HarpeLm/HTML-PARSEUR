//! La comparaison avec Chromium, commune aux tests de mise en page.
//!
//! Un fichier de capture (voir tests/oracle/capture-chromium.js) contient, pour
//! chaque page, la boîte de bordure de chaque élément (`getBoundingClientRect()`)
//! dans l'ordre du document. On met la même page en page et on compare, à 1/64
//! de pixel d'écran près : Chromium range les positions en 64es de pixel d'écran
//! (`LayoutUnit`), soit 1/128 px CSS sur un écran de densité 2.

use std::path::Path;

use html_parseur::dom::NodeId;
use html_parseur::{ParseOptions, parse_document_with};
use lumen_css::media::Environment;
use lumen_font::FontDatabase;
use lumen_layout::{Rect, Viewport, layout_document};
use lumen_style::style_document;
use serde_json::Value;

/// Compare toutes les pages de `capture` ; renvoie (boîtes comparées, différences).
pub fn compare(capture: &str) -> (usize, Vec<String>) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/oracle")
        .join(capture);
    let oracle: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    println!("Référence : {}", oracle["browser"]);
    let e = &oracle["environment"];
    let env = Environment {
        width: e["width"].as_f64().unwrap(),
        height: e["height"].as_f64().unwrap(),
        resolution: e["resolution"].as_f64().unwrap(),
        ..Environment::default()
    };
    let viewport = Viewport {
        width: env.width,
        height: env.height,
        device_pixel_ratio: env.resolution,
    };
    let tolerance = 1.0 / 64.0 / env.resolution + 1e-9;
    let fonts = FontDatabase::system();

    let mut failures = Vec::new();
    let mut checked = 0;
    for (case, r) in oracle["results"].as_array().unwrap().iter().enumerate() {
        let html = r["html"].as_str().unwrap();
        let doc = parse_document_with(
            html,
            ParseOptions {
                scripting: true,
                declarative_shadow_roots: true,
            },
        );
        let styles = style_document(&doc, &env);
        let layout = layout_document(&doc, &styles, &viewport, &fonts);
        let ours: Vec<NodeId> = doc
            .descendants(NodeId::DOCUMENT)
            .filter(|&n| doc.element(n).is_some())
            .collect();
        let theirs = r["elements"].as_array().unwrap();
        assert_eq!(
            ours.len(),
            theirs.len(),
            "page {case} : pas le même arbre\n{html}"
        );
        for (&id, expected) in ours.iter().zip(theirs) {
            checked += 1;
            let name = doc.atoms.name(doc.element(id).unwrap().name);
            let n = |i: usize| expected[i].as_f64().unwrap();
            let want = Rect {
                x: n(1),
                y: n(2),
                width: n(3),
                height: n(4),
            };
            let got = layout.border_box(id).unwrap_or_default();
            let close = |a: f64, b: f64| (a - b).abs() <= tolerance;
            if !(close(got.x, want.x)
                && close(got.y, want.y)
                && close(got.width, want.width)
                && close(got.height, want.height))
            {
                failures.push(format!(
                    "page {case}, <{name}> : Chromium {:?}, nous {:?}\n   {html}",
                    (want.x, want.y, want.width, want.height),
                    (got.x, got.y, got.width, got.height)
                ));
            }
        }
    }
    for f in &failures {
        println!("❌ {f}");
    }
    println!(
        "{} / {checked} boîtes identiques à Chromium",
        checked - failures.len()
    );
    (checked, failures)
}
