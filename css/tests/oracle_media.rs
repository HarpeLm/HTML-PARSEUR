//! Test différentiel contre Chromium : chaque media query de
//! tests/oracle/media_cases.json doit être sérialisée comme Chromium
//! (`matchMedia(q).media`) et donner le même résultat (`matches`) dans le même
//! environnement (enregistré avec la capture : tests/oracle/media_chromium.json).
//!
//!   cargo test -p lumen-css --test oracle_media -- --nocapture

use std::path::Path;

use lumen_css::media::{Environment, MediaQueryList};
use lumen_css::{Parser, preprocess};
use serde_json::Value;

fn environment(e: &Value) -> Environment {
    let number = |k: &str| e[k].as_f64().unwrap();
    Environment {
        media_type: "screen".into(),
        width: number("width"),
        height: number("height"),
        device_width: number("device_width"),
        device_height: number("device_height"),
        resolution: number("resolution"),
        color: number("color") as u32,
        font_size: 16.0,
        features: e["features"]
            .as_object()
            .unwrap()
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
            .collect(),
    }
}

#[test]
fn memes_media_queries_que_chromium() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/media_chromium.json");
    let oracle: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    println!("Référence : {}", oracle["browser"]);
    let env = environment(&oracle["environment"]);
    println!(
        "Environnement : {}×{} px, {}dppx",
        env.width, env.height, env.resolution
    );

    let mut failures = Vec::new();
    let results = oracle["results"].as_array().unwrap();
    for r in results {
        let query = r["query"].as_str().unwrap();
        let (media, matches) = (
            r["media"].as_str().unwrap(),
            r["matches"].as_bool().unwrap(),
        );
        let css = preprocess(query);
        let list = MediaQueryList::parse(&Parser::new(&css).parse_component_value_list());
        let (ours, ours_matches) = (list.to_string(), list.matches(&env));
        if ours != media || ours_matches != matches {
            failures.push(format!(
                "{query:?}\n   Chromium : {media:?} {matches}\n   nous     : {ours:?} {ours_matches}"
            ));
        }
    }
    for f in &failures {
        println!("❌ {f}");
    }
    println!(
        "{} / {} identiques à Chromium",
        results.len() - failures.len(),
        results.len()
    );
    assert!(
        failures.is_empty(),
        "{} différences avec Chromium",
        failures.len()
    );
}
