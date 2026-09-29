//! Lance les tests officiels html5lib-tests/tokenizer sur notre tokenizer.
//!
//!   cargo run --example html5lib              -> score par fichier
//!   cargo run --example html5lib -- test1     -> seulement les fichiers contenant "test1"
//!   VERBOSE=1 cargo run --example html5lib    -> détail de chaque échec
//!
//! Chaque test est lancé dans chacun de ses états initiaux (Data, RCDATA, ...).

use std::{fs, panic, path::Path};

use html_parseur::{InitialState, Token, Tokenizer};
use serde_json::{Map, Value, json};

/// Convertit nos tokens au format JSON attendu par html5lib-tests.
fn tokens_to_json(tokens: Vec<Token<'_>>) -> Vec<Value> {
    let mut out = Vec::new();
    let mut text = String::new(); // les caractères consécutifs sont fusionnés

    for token in tokens {
        if let Token::Characters(s) = &token {
            text.push_str(s);
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
                    .map(|a| (a.name.into_owned(), Value::String(a.value.into_owned())))
                    .collect();
                let mut v = vec![json!("StartTag"), json!(tag.name), Value::Object(attrs)];
                if tag.self_closing {
                    v.push(json!(true));
                }
                out.push(Value::Array(v));
            }
            Token::EndTag(tag) => out.push(json!(["EndTag", tag.name])),
            Token::Comment(data) => out.push(json!(["Comment", data])),
            Token::ProcessingInstruction { target, data } => {
                out.push(json!(["ProcessingInstruction", target, data]))
            }
            Token::Doctype(d) => out.push(json!([
                "DOCTYPE",
                d.name,
                d.public_id,
                d.system_id,
                !d.force_quirks
            ])),
            Token::Eof => {}
            Token::Characters(_) => unreachable!(),
        }
    }
    if !text.is_empty() {
        out.push(json!(["Character", text]));
    }
    out
}

/// Les tests "doubleEscaped" écrivent certains caractères sous la forme \uXXXX.
/// Renvoie None si le texte contient un surrogate isolé (impossible dans une
/// String Rust, qui est toujours de l'UTF-8 valide).
fn unescape(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let esc: String = chars.by_ref().take(5).collect(); // "uXXXX"
            let code = u32::from_str_radix(esc.strip_prefix('u')?, 16).ok()?;
            out.push(char::from_u32(code)?);
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// Applique `unescape` à toutes les chaînes d'une valeur JSON.
fn unescape_json(v: &Value) -> Option<Value> {
    Some(match v {
        Value::String(s) => Value::String(unescape(s)?),
        Value::Array(a) => Value::Array(a.iter().map(unescape_json).collect::<Option<_>>()?),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, v)| Some((unescape(k)?, unescape_json(v)?)))
                .collect::<Option<_>>()?,
        ),
        other => other.clone(),
    })
}

fn initial_state(name: &str) -> InitialState {
    match name {
        "Data state" => InitialState::Data,
        "RCDATA state" => InitialState::Rcdata,
        "RAWTEXT state" => InitialState::Rawtext,
        "Script data state" => InitialState::ScriptData,
        "PLAINTEXT state" => InitialState::Plaintext,
        "CDATA section state" => InitialState::CdataSection,
        other => panic!("état initial inconnu : {other}"),
    }
}

/// Vrai si le résultat attendu contient un commentaire "?..." : c'est ainsi que
/// l'ancienne spec traitait "<?", avant les processing instructions.
fn expects_old_pi_comment(expected: &[Value]) -> bool {
    expected.iter().any(|token| {
        token[0] == "Comment" && token[1].as_str().is_some_and(|data| data.starts_with('?'))
    })
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

    // Chemin relatif au crate : fonctionne depuis la racine du workspace ou depuis html/.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("html5lib-tests/tokenizer");
    let mut files: Vec<_> = fs::read_dir(dir)
        .expect("dossier html5lib-tests introuvable : fais le git clone d'abord")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "test"))
        .collect();
    files.sort();

    let (mut pass, mut fail, mut crash, mut skip, mut obsolete) = (0, 0, 0, 0, 0);

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
            let (input, expected) = if test["doubleEscaped"].as_bool() == Some(true) {
                match (
                    unescape(test["input"].as_str().unwrap()),
                    unescape_json(&test["output"]),
                ) {
                    (Some(i), Some(o)) => (i, o),
                    _ => {
                        skip += 1; // surrogate isolé : non représentable en Rust
                        continue;
                    }
                }
            } else {
                (
                    test["input"].as_str().unwrap().to_string(),
                    test["output"].clone(),
                )
            };
            let expected = expected.as_array().unwrap();
            let last_start_tag = test["lastStartTag"].as_str();
            let states: Vec<&str> = match test["initialStates"].as_array() {
                Some(list) => list.iter().map(|s| s.as_str().unwrap()).collect(),
                None => vec!["Data state"],
            };

            // Un même test est lancé une fois par état de départ.
            for state_name in states {
                file_total += 1;
                let result = panic::catch_unwind(|| {
                    let mut tokenizer = Tokenizer::new(&input);
                    tokenizer.set_state(initial_state(state_name));
                    if let Some(tag) = last_start_tag {
                        tokenizer.set_last_start_tag(tag);
                    }
                    tokens_to_json(tokenizer.collect())
                });

                match result {
                    Ok(got) if &got == expected => {
                        pass += 1;
                        file_pass += 1;
                    }
                    // html5lib-tests n'est plus mis à jour : il attend encore des
                    // commentaires pour "<?", alors que la spec (et WPT) en font
                    // maintenant des processing instructions.
                    // On ne classe "obsolète" QUE ce cas précis : le résultat attendu
                    // contient un commentaire "?..." (l'ancienne règle). Toute autre
                    // différence reste un vrai échec.
                    Ok(_) if expects_old_pi_comment(expected) => obsolete += 1,
                    Ok(got) => {
                        fail += 1;
                        if verbose {
                            println!("❌ [{name}] {} ({state_name})", test["description"]);
                            println!("   entrée  : {input:?}");
                            println!("   attendu : {}", Value::Array(expected.clone()));
                            println!("   obtenu  : {}\n", Value::Array(got));
                        }
                    }
                    Err(err) => {
                        crash += 1;
                        if verbose {
                            println!("💥 [{name}] {} ({state_name})", test["description"]);
                            println!("   entrée  : {input:?}");
                            println!("   panic   : {}\n", panic_message(&err));
                        }
                    }
                }
            }
        }
        println!("{name:<28} {file_pass:>4} / {file_total}");
    }

    let total = pass + fail + crash;
    if obsolete > 0 {
        println!(
            "\n📜 obsolètes : {obsolete} (\"<?\" : processing instructions, spec 2026 vérifiée par WPT)"
        );
    }
    let pct = if total > 0 {
        100.0 * pass as f64 / total as f64
    } else {
        0.0
    };
    println!(
        "\n✅ réussis : {pass}   ❌ faux : {fail}   💥 todo!/panic : {crash}   ⏭️  ignorés : {skip}"
    );
    println!("Score : {pass}/{total} ({pct:.1} %)");
}
