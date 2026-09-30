//! Test différentiel contre Chromium : la mise en page des blocs.
//!
//! Chaque page de tests/oracle/blocks_cases.json (sans texte : il faudrait des
//! polices) a été chargée dans Chromium (iframe de 800 × 600 px) ;
//! tests/oracle/blocks_chromium.json contient, pour chaque élément, sa boîte de
//! bordure (`getBoundingClientRect()`). On met la même page en page et on
//! compare, à 1/64 px près : Chromium range les positions en 64es de pixel.
//!
//!   cargo test -p lumen-layout --test oracle_blocks -- --nocapture

use std::path::Path;

use html_parseur::dom::NodeId;
use html_parseur::{ParseOptions, parse_document_with};
use lumen_css::media::Environment;
use lumen_layout::{Rect, layout_document};
use lumen_style::style_document;
use serde_json::Value;

/// La précision des positions de Chromium (`LayoutUnit` : 1/64 px).
const TOLERANCE: f64 = 1.0 / 64.0 + 1e-9;

#[test]
fn memes_boites_que_chromium() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/blocks_chromium.json");
    let oracle: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    println!("Référence : {}", oracle["browser"]);
    let e = &oracle["environment"];
    let env = Environment {
        width: e["width"].as_f64().unwrap(),
        height: e["height"].as_f64().unwrap(),
        resolution: e["resolution"].as_f64().unwrap(),
        ..Environment::default()
    };

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
        let layout = layout_document(&doc, &styles, env.width, env.height);
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
            let close = |a: f64, b: f64| (a - b).abs() <= TOLERANCE;
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
    assert!(
        failures.is_empty(),
        "{} différences avec Chromium",
        failures.len()
    );
}
