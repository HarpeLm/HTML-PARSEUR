//! Test différentiel contre Chromium : propriétés personnalisées et `var()`.
//! Chaque cas de tests/oracle/variables_cases.json donne le style d'un parent et
//! d'un enfant ; on calcule les valeurs de l'enfant et on les compare à celles
//! de `getComputedStyle` dans Chromium (tests/oracle/variables_chromium.json).
//!
//! La cascade n'existe pas encore : ce test en contient le strict nécessaire
//! (dernière déclaration gagnante, héritage, `unset` quand `var()` échoue).
//!
//!   cargo test -p lumen-css --test oracle_variables -- --nocapture

use std::collections::HashMap;
use std::path::Path;

use lumen_css::properties::{SpecifiedValue, initial_value, is_inherited, parse_property};
use lumen_css::values::LengthPercentage;
use lumen_css::variables::{
    CustomProperties, contains_var, is_custom_property, is_valid_value, raw_declarations,
    substitute,
};
use lumen_css::{Parser, preprocess};
use serde_json::Value;

/// Les valeurs calculées d'un élément.
#[derive(Default)]
struct Style {
    vars: CustomProperties,
    props: HashMap<&'static str, String>,
}

/// Ce qui a été déclaré pour une propriété longue.
enum Declared {
    Value(SpecifiedValue),
    /// Contient `var()` : on garde la propriété d'origine (peut-être un
    /// raccourci) et son texte, substitués au moment du calcul.
    Pending(String, String),
}

/// Les propriétés longues que définit `name` (le nom statique est celui que
/// renvoie le parser, obtenu en parsant la valeur initiale).
fn longhands(name: &str) -> Vec<&'static str> {
    let static_name = |n: &str| {
        let initial = initial_value(n)?.to_string();
        let values = Parser::new(&initial).parse_component_value_list();
        Some(parse_property(n, &values)?[0].0)
    };
    match name {
        "margin" | "padding" => ["top", "right", "bottom", "left"]
            .iter()
            .filter_map(|side| static_name(&format!("{name}-{side}")))
            .collect(),
        _ => static_name(name).into_iter().collect(),
    }
}

/// La valeur calculée telle que `getComputedStyle` l'écrit : un `calc()` qui se
/// réduit à des pixels devient une longueur.
fn computed(v: &SpecifiedValue) -> String {
    if let SpecifiedValue::LengthPercentage(LengthPercentage::Calc(sum)) = v
        && sum.0.keys().all(|u| *u == "px")
    {
        let px: f64 = sum.0.values().sum();
        return format!("{}px", lumen_css::values::format_number(px));
    }
    v.to_string()
}

fn compute(style: &str, parent: &Style) -> Style {
    let css = preprocess(style);
    let raw = raw_declarations(&css);
    let vars = CustomProperties::compute(
        raw.iter()
            .filter(|d| is_custom_property(&d.name))
            .map(|d| (d.name.as_ref(), d.value)),
        &parent.vars,
    );

    let mut declared: HashMap<&'static str, Declared> = HashMap::new();
    for d in raw.iter().filter(|d| !is_custom_property(&d.name)) {
        let name = d.name.to_ascii_lowercase();
        if contains_var(d.value) {
            if is_valid_value(d.value) {
                for l in longhands(&name) {
                    declared.insert(l, Declared::Pending(name.clone(), d.value.to_string()));
                }
            }
        } else if let Some(values) =
            parse_property(&name, &Parser::new(d.value).parse_component_value_list())
        {
            for (l, v) in values {
                declared.insert(l, Declared::Value(v));
            }
        }
    }

    let mut props = HashMap::new();
    for (longhand, d) in declared {
        let value = match d {
            Declared::Value(v) => Some(v),
            Declared::Pending(property, text) => substitute(&text, &vars).and_then(|s| {
                parse_property(&property, &Parser::new(&s).parse_component_value_list())?
                    .into_iter()
                    .find(|(l, _)| *l == longhand)
                    .map(|(_, v)| v)
            }),
        };
        // Invalide au moment du calcul : `unset` (valeur du parent si la
        // propriété est héritée, sinon valeur initiale) -> rien à enregistrer.
        if let Some(v) = value {
            props.insert(longhand, computed(&v));
        }
    }
    Style { vars, props }
}

fn read(style: &Style, parent: &Style, name: &str) -> String {
    if is_custom_property(name) {
        return style.vars.get(name).unwrap_or("").to_string();
    }
    if let Some(v) = style.props.get(name) {
        return v.clone();
    }
    if is_inherited(name) {
        return read(parent, &Style::default(), name);
    }
    computed(&initial_value(name).unwrap_or_else(|| panic!("propriété inconnue : {name}")))
}

#[test]
fn memes_variables_que_chromium() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/variables_chromium.json");
    let oracle: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    println!("Référence : {}", oracle["browser"]);

    let mut failures = Vec::new();
    let mut checks = 0;
    let results = oracle["results"].as_array().unwrap();
    for r in results {
        let (p, s) = (r["parent"].as_str().unwrap(), r["style"].as_str().unwrap());
        let parent = compute(p, &Style::default());
        let child = compute(s, &parent);
        for (name, expected) in r["computed"].as_object().unwrap() {
            checks += 1;
            let expected = expected.as_str().unwrap();
            let ours = read(&child, &parent, name);
            if ours != expected {
                failures.push(format!(
                    "parent {{{p}}} enfant {{{s}}} -> {name}\n   Chromium : {expected:?}\n   nous     : {ours:?}"
                ));
            }
        }
    }
    for f in &failures {
        println!("❌ {f}");
    }
    println!(
        "{} / {} valeurs identiques à Chromium ({} cas)",
        checks - failures.len(),
        checks,
        results.len()
    );
    assert!(
        failures.is_empty(),
        "{} différences avec Chromium",
        failures.len()
    );
}
