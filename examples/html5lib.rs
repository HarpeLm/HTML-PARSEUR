//! Lance les tests officiels html5lib-tests/tokenizer sur notre tokenizer.
//!
//!   cargo run --example html5lib              -> score par fichier
//!   cargo run --example html5lib -- test1     -> seulement les fichiers contenant "test1"
//!   VERBOSE=1 cargo run --example html5lib    -> détail de chaque échec

use std::{fs, panic, path::Path};

use html_tokenizer::{Token, Tokenizer};
use serde_json::{json, Map, Value};

/// Convertit nos tokens au format JSON attendu par html5lib-tests.
fn tokens_to_json(tokens: Vec<Token>) -> Vec<Value> {
    let mut out = Vec::new();
    let mut text = String::new(); // les caractères consécutifs sont fusionnés

    for token in tokens {
        if let Token::Character(c) = token {
            text.push(c);
            continue;
        }
        if !text.is_empty() {
            out.push(json!(["Character", std::mem::take(&mut text)]));
        }
        match token {
            Token::StartTag(tag) => {
                let attrs: Map<String, Value> = tag
                    .attributes
                    .into_iter()
                    .map(|a| (a.name, Value::String(a.value)))
                    .collect();
                let mut v = vec![json!("StartTag"), json!(tag.name), Value::Object(attrs)];
                if tag.self_closing {
                    v.push(json!(true));
                }
                out.push(Value::Array(v));
            }
            Token::EndTag(tag) => out.push(json!(["EndTag", tag.name])),
            Token::Comment(data) => out.push(json!(["Comment", data])),
            Token::Doctype(d) => out.push(json!([
                "DOCTYPE",
                d.name,
                d.public_id,
                d.system_id,
                !d.force_quirks
            ])),
            Token::Eof => {}
            Token::Character(_) => unreachable!(),
        }
    }
    if !text.is_empty() {
        out.push(json!(["Character", text]));
    }
    out
}

fn panic_message(err: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = err.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = err.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "panic".to_string()
    }
}

fn main() {
    let filter = std::env::args().nth(1);
    let verbose = std::env::var("VERBOSE").is_ok();
    panic::set_hook(Box::new(|_| {})); // on affiche nous-mêmes les panics

    let dir = Path::new("html5lib-tests/tokenizer");
    let mut files: Vec<_> = fs::read_dir(dir)
        .expect("dossier html5lib-tests introuvable : fais le git clone d'abord")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "test"))
        .collect();
    files.sort();

    let (mut pass, mut fail, mut crash, mut skip) = (0, 0, 0, 0);

    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if filter.as_ref().is_some_and(|f| !name.contains(f.as_str())) {
            continue;
        }
        let data: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let Some(tests) = data["tests"].as_array() else {
            continue;
        };

        let (mut file_pass, mut file_total) = (0, 0);
        for test in tests {
            // Ignorés pour l'instant : entrées avec surrogates isolés, et états initiaux
            // autres que "Data state" (RCDATA, RAWTEXT... viendront plus tard).
            if test["doubleEscaped"].as_bool() == Some(true) {
                skip += 1;
                continue;
            }
            if let Some(states) = test["initialStates"].as_array() {
                if !states.iter().any(|s| s == "Data state") {
                    skip += 1;
                    continue;
                }
            }

            file_total += 1;
            let input = test["input"].as_str().unwrap();
            let expected = test["output"].as_array().unwrap();
            let result = panic::catch_unwind(|| tokens_to_json(Tokenizer::new(input).collect()));

            match result {
                Ok(got) if &got == expected => {
                    pass += 1;
                    file_pass += 1;
                }
                Ok(got) => {
                    fail += 1;
                    if verbose {
                        println!("❌ [{name}] {}", test["description"]);
                        println!("   entrée  : {input:?}");
                        println!("   attendu : {}", Value::Array(expected.clone()));
                        println!("   obtenu  : {}\n", Value::Array(got));
                    }
                }
                Err(err) => {
                    crash += 1;
                    if verbose {
                        println!("💥 [{name}] {}", test["description"]);
                        println!("   entrée  : {input:?}");
                        println!("   panic   : {}\n", panic_message(&err));
                    }
                }
            }
        }
        println!("{name:<28} {file_pass:>4} / {file_total}");
    }

    let total = pass + fail + crash;
    let pct = if total > 0 { 100.0 * pass as f64 / total as f64 } else { 0.0 };
    println!("\n✅ réussis : {pass}   ❌ faux : {fail}   💥 todo!/panic : {crash}   ⏭️  ignorés : {skip}");
    println!("Score : {pass}/{total} ({pct:.1} %)");
}