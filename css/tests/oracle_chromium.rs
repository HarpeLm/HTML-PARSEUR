//! Test différentiel contre Chromium : pour chaque déclaration de
//! tests/oracle/cases.json, notre parser doit trouver les mêmes propriétés
//! longues, avec la même valeur sérialisée, que Chromium (tests/oracle/chromium.json,
//! capturé avec tests/oracle/capture-chromium.js).
//!
//!   cargo test -p lumen-css --test oracle_chromium -- --nocapture

use std::collections::BTreeMap;
use std::path::Path;

use lumen_css::properties::parse_property;
use lumen_css::{Parser, preprocess};
use serde_json::Value;

#[test]
fn memes_valeurs_que_chromium() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/chromium.json");
    let oracle: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    println!("Référence : {}", oracle["browser"]);

    let mut failures = Vec::new();
    let results = oracle["results"].as_array().unwrap();
    for r in results {
        let (property, value) = (
            r["property"].as_str().unwrap(),
            r["value"].as_str().unwrap(),
        );
        let expected: BTreeMap<String, String> = r["longhands"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
            .collect();

        let css = preprocess(value);
        let values = Parser::new(&css).parse_component_value_list();
        let ours: BTreeMap<String, String> = parse_property(property, &values)
            .unwrap_or_default()
            .into_iter()
            .map(|(name, v)| (name.to_string(), v.to_string()))
            .collect();

        if ours != expected {
            failures.push(format!(
                "{property}: {value:?}\n   Chromium : {expected:?}\n   nous     : {ours:?}"
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
