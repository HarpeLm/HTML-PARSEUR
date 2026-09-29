//! Test différentiel contre Chromium sur les 5 vraies pages de
//! html/benches/pages/ : le style calculé de chaque élément.
//!
//! Capture : chaque page chargée dans une iframe de 1024 × 768 px, servie avec
//! une politique de sécurité (CSP) qui bloque tout ce qui est externe (scripts,
//! feuilles `<link>`, images). Chromium n'applique donc que la feuille par
//! défaut, les `<style>` et les attributs `style` de la page, comme Lumen.
//! Résultat dans tests/oracle/pages_chromium.json (valeurs mises en dictionnaire).
//!
//!   cargo test --release -p lumen-style --test oracle_pages -- --nocapture

use std::collections::BTreeMap;
use std::path::Path;

use html_parseur::dom::NodeId;
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

/// Une valeur que `getComputedStyle` donne après mise en page.
fn needs_layout(property: &str, value: Option<&Computed>) -> bool {
    (property.starts_with("margin") || property.starts_with("padding"))
        && matches!(
            value,
            Some(Computed::Keyword(_) | Computed::Percentage(_) | Computed::Calc { .. })
        )
}

#[test]
fn vraies_pages_comme_chromium() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let oracle: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("tests/oracle/pages_chromium.json")).unwrap(),
    )
    .unwrap();
    println!("Référence : {}", oracle["browser"]);
    let props: Vec<&str> = oracle["props"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    let dictionary: Vec<&str> = oracle["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();

    let (mut total_checked, mut total_same, mut unexplained) = (0usize, 0usize, 0usize);
    for r in oracle["results"].as_array().unwrap() {
        let page = r["page"].as_str().unwrap();
        let html = std::fs::read_to_string(root.join(format!("../html/benches/pages/{page}.html")))
            .unwrap();
        let env = environment(&r["environment"]);
        let doc = parse_document_with(&html, ParseOptions { scripting: true });
        let styles = style_document(&doc, &env);
        // `<template shadowrootmode>` (shadow DOM déclaratif) : Chromium en fait
        // une racine fantôme et retire l'élément de l'arbre ; html-parseur ne le
        // gère pas encore et le garde. On l'écarte pour comparer les mêmes arbres.
        let ours: Vec<NodeId> = doc
            .descendants(NodeId::DOCUMENT)
            .filter(|&n| {
                doc.element(n).is_some_and(|e| {
                    doc.atoms.name(e.name) != "template"
                        || !e.attrs.iter().any(|a| a.name == "shadowrootmode")
                })
            })
            .collect();
        let names: Vec<&str> = r["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        let our_names: Vec<&str> = ours
            .iter()
            .map(|&n| doc.atoms.name(doc.element(n).unwrap().name))
            .collect();
        if our_names != names {
            let first = our_names.iter().zip(&names).position(|(a, b)| a != b);
            let at = first.unwrap_or(our_names.len().min(names.len()));
            let lo = at.saturating_sub(3);
            panic!(
                "{page} : arbres différents ({} éléments contre {}) à partir de l'élément {at}\n  nous     : {:?}\n  Chromium : {:?}",
                our_names.len(),
                names.len(),
                &our_names[lo..(at + 6).min(our_names.len())],
                &names[lo..(at + 6).min(names.len())]
            );
        }

        let (mut checked, mut layout) = (0usize, 0usize);
        let mut diffs: BTreeMap<(&str, String, String), usize> = BTreeMap::new();
        let mut per_property: BTreeMap<&str, usize> = BTreeMap::new();
        for ((&id, name), row) in ours.iter().zip(&names).zip(r["rows"].as_array().unwrap()) {
            assert_eq!(
                doc.atoms.name(doc.element(id).unwrap().name),
                *name,
                "{page} : arbres différents"
            );
            let style = styles.get(id).unwrap();
            for (i, property) in props.iter().enumerate() {
                if needs_layout(property, style.get(property)) {
                    layout += 1;
                    continue;
                }
                checked += 1;
                let expected = dictionary[row[i].as_u64().unwrap() as usize];
                let got = style.resolved(property).unwrap_or_default();
                if got != expected {
                    // Différence connue : un élément personnalisé (`<mdn-dropdown>`)
                    // dont le `display` vient d'une règle `:host` de sa racine
                    // fantôme (shadow DOM déclaratif, pas encore géré).
                    let shadow_host = *property == "display" && name.contains('-');
                    if !shadow_host {
                        unexplained += 1;
                    }
                    *per_property.entry(property).or_default() += 1;
                    *diffs
                        .entry((
                            property,
                            format!("<{name}> Chromium {expected}"),
                            format!("nous {got}"),
                        ))
                        .or_default() += 1;
                }
            }
        }
        let wrong: usize = per_property.values().sum();
        total_checked += checked;
        total_same += checked - wrong;
        println!(
            "\n{page} : {} éléments, {} / {checked} valeurs identiques ({:.3} %), {layout} non comparées",
            ours.len(),
            checked - wrong,
            100.0 * (checked - wrong) as f64 / checked as f64
        );
        if wrong > 0 {
            println!("  par propriété : {per_property:?}");
            let mut top: Vec<_> = diffs.into_iter().collect();
            top.sort_by_key(|entry| std::cmp::Reverse(entry.1));
            for ((property, theirs, ours), n) in top.into_iter().take(8) {
                println!("  {n:>6} × {property} {theirs}, {ours}");
            }
        }
    }
    println!(
        "\nTotal : {total_same} / {total_checked} valeurs identiques à Chromium ({:.3} %)",
        100.0 * total_same as f64 / total_checked as f64
    );
    assert_eq!(unexplained, 0, "différences inexpliquées avec Chromium");
}
