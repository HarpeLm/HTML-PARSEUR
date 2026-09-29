//! Test différentiel contre Chromium : la cascade complète.
//!
//! Chaque page de tests/oracle/cascade_cases.json a été chargée dans Chromium
//! (iframe de 800 × 600 px) ; tests/oracle/cascade_chromium.json contient, pour
//! chaque élément, les valeurs de `getComputedStyle()`. On parse la même page,
//! on calcule les styles et on compare, élément par élément.
//!
//! Les marges et retraits en `auto` ou en `%` ne sont pas comparés :
//! `getComputedStyle` les donne après la mise en page (en px), que Lumen ne fait
//! pas encore. Ils sont comptés à part.
//!
//!   cargo test -p lumen-style --test oracle_cascade -- --nocapture

use std::collections::BTreeMap;
use std::path::Path;

use html_parseur::dom::{Document, NodeId};
use html_parseur::{ParseOptions, parse_document_with};
use lumen_css::media::Environment;
use lumen_style::{Computed, style_document};
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

fn elements(doc: &Document) -> Vec<NodeId> {
    doc.descendants(NodeId::DOCUMENT)
        .filter(|&n| doc.element(n).is_some())
        .collect()
}

/// Une valeur que `getComputedStyle` donne après mise en page.
fn needs_layout(property: &str, value: Option<&Computed>) -> bool {
    (property.starts_with("margin") || property.starts_with("padding"))
        && matches!(
            value,
            Some(Computed::Keyword(_) | Computed::Percentage(_) | Computed::Calc { .. })
        )
}

#[test]
fn memes_styles_que_chromium() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/cascade_chromium.json");
    let oracle: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    println!("Référence : {}", oracle["browser"]);
    let env = environment(&oracle["environment"]);
    let props: Vec<&str> = oracle["props"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();

    let mut failures = Vec::new();
    let (mut checked, mut layout) = (0, 0);
    let mut per_property: BTreeMap<&str, usize> = BTreeMap::new();
    for (case, r) in oracle["results"].as_array().unwrap().iter().enumerate() {
        let html = r["html"].as_str().unwrap();
        let doc = parse_document_with(html, ParseOptions { scripting: true });
        let styles = style_document(&doc, &env);
        let ours = elements(&doc);
        let theirs = r["elements"].as_array().unwrap();
        let names: Vec<&str> = ours
            .iter()
            .map(|&n| doc.atoms.name(doc.element(n).unwrap().name))
            .collect();
        let expected_names: Vec<&str> = theirs.iter().map(|e| e[0].as_str().unwrap()).collect();
        assert_eq!(
            names, expected_names,
            "page {case} : pas le même arbre\n{html}"
        );

        for ((&id, name), expected) in ours.iter().zip(&names).zip(theirs) {
            let style = styles.get(id).expect("élément sans style");
            for (i, property) in props.iter().enumerate() {
                let expected = expected[i + 1].as_str().unwrap();
                if needs_layout(property, style.get(property)) {
                    layout += 1;
                    continue;
                }
                checked += 1;
                let got = style.resolved(property).unwrap_or_default();
                if got != expected {
                    *per_property.entry(property).or_default() += 1;
                    failures.push(format!(
                        "page {case}, <{name}> {property} : Chromium {expected:?}, nous {got:?}\n   {html}"
                    ));
                }
            }
        }
    }
    for f in &failures {
        println!("❌ {f}");
    }
    println!("Différences par propriété : {per_property:?}");
    println!(
        "{} / {checked} valeurs identiques à Chromium ({layout} non comparées : mise en page)",
        checked - failures.len()
    );
    assert!(
        failures.is_empty(),
        "{} différences avec Chromium",
        failures.len()
    );
}
