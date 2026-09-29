//! Lance les tests de construction d'arbre de WPT (fichiers .dat) sur notre parser.
//!
//!   ./tools/fetch_wpt_tests.sh                          -> télécharger les tests (une fois)
//!   cargo run --release --example tree_construction     -> score par fichier
//!   cargo run --release --example tree_construction -- tests1   -> un seul fichier
//!   VERBOSE=1 cargo run --release --example tree_construction -- tests1

use std::sync::mpsc;
use std::time::Duration;
use std::{fs, panic, path::Path, thread};

use html_tokenizer::dom::Namespace;
use html_tokenizer::{parse_document_with, parse_fragment, ParseOptions};

struct Test {
    data: String,
    document: String,
    /// Élément de contexte ("td", "svg path"...) pour les tests de fragment.
    fragment: Option<String>,
    script_on: bool,
    script_off: bool,
}

/// Découpe un fichier .dat en tests (format décrit dans le README de WPT).
fn parse_dat(content: &str) -> Vec<Test> {
    let mut tests = Vec::new();
    let mut section = "";
    let mut data: Vec<&str> = Vec::new();
    let mut document: Vec<&str> = Vec::new();
    let mut fragment: Option<String> = None;
    let (mut script_on, mut script_off) = (false, false);

    let mut flush = |data: &mut Vec<&str>,
                     document: &mut Vec<&str>,
                     fragment: &mut Option<String>,
                     script_on: &mut bool,
                     script_off: &mut bool| {
        if data.is_empty() && document.is_empty() {
            return;
        }
        // La ligne vide qui sépare deux tests n'appartient pas au document.
        while document.last() == Some(&"") {
            document.pop();
        }
        tests.push(Test {
            data: data.join("\n"),
            document: document.join("\n"),
            fragment: fragment.take(),
            script_on: *script_on,
            script_off: *script_off,
        });
        data.clear();
        document.clear();
        *script_on = false;
        *script_off = false;
    };

    for line in content.split('\n') {
        match line {
            "#data" => {
                flush(&mut data, &mut document, &mut fragment, &mut script_on, &mut script_off);
                section = "data";
            }
            "#errors" | "#new-errors" => section = "errors",
            "#document-fragment" => section = "fragment",
            "#script-on" => script_on = true,
            "#script-off" => script_off = true,
            "#document" => section = "document",
            _ => match section {
                "data" => data.push(line),
                "document" => document.push(line),
                "fragment" => fragment = Some(line.to_string()),
                _ => {}
            },
        }
    }
    flush(&mut data, &mut document, &mut fragment, &mut script_on, &mut script_off);
    tests
}

fn main() {
    let filter = std::env::args().nth(1);
    let verbose = std::env::var("VERBOSE").is_ok();
    panic::set_hook(Box::new(|_| {}));

    let dir = Path::new("wpt-tests/html/syntax/parsing/resources");
    let mut files: Vec<_> = fs::read_dir(dir)
        .expect("tests introuvables : lance d'abord ./tools/fetch_wpt_tests.sh")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "dat"))
        .collect();
    files.sort();

    let (mut pass, mut fail, mut crash, mut hang, mut needs_js) = (0, 0, 0, 0, 0);

    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if filter.as_ref().is_some_and(|f| !name.contains(f.as_str())) {
            continue;
        }
        let content = String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned();
        let (mut file_pass, mut file_total) = (0, 0);

        for test in parse_dat(&content) {
            // Sans indication, un test doit passer avec ET sans JavaScript.
            let modes: &[bool] = match (test.script_on, test.script_off) {
                (true, _) => &[true],
                (_, true) => &[false],
                _ => &[false, true],
            };
            // Les fichiers scripted_* exécutent du JavaScript pendant le parsing
            // (document.write...) : impossible sans moteur JS.
            if name.starts_with("scripted_") {
                needs_js += modes.len();
                continue;
            }
            for &scripting in modes {
            file_total += 1;
            let options = ParseOptions { scripting };
            let label = if scripting { " [script-on]" } else { "" };
            // Chaque test tourne dans son thread, avec un délai maximum : une boucle
            // infinie dans le parser ne bloque pas tout le banc.
            let (sender, receiver) = mpsc::channel();
            let (data, fragment) = (test.data.clone(), test.fragment.clone());
            thread::spawn(move || {
                let result = panic::catch_unwind(|| match &fragment {
                    None => parse_document_with(&data, options).to_test_string(),
                    Some(context) => {
                        let (ns, name) = match context.split_once(' ') {
                            Some(("svg", name)) => (Namespace::Svg, name),
                            Some(("math", name)) => (Namespace::MathMl, name),
                            _ => (Namespace::Html, context.as_str()),
                        };
                        let (doc, root) = parse_fragment(&data, ns, name, options);
                        doc.to_test_string_from(root)
                    }
                });
                let _ = sender.send(result);
            });
            let Ok(result) = receiver.recv_timeout(Duration::from_secs(2)) else {
                hang += 1;
                println!("⏱️  [{name}] {:?}{label} : boucle infinie", test.data);
                continue;
            };
            match result {
                Ok(got) if got == test.document => {
                    pass += 1;
                    file_pass += 1;
                }
                Ok(got) => {
                    fail += 1;
                    if verbose {
                        let ctx = test.fragment.as_deref().map(|c| format!(" (fragment dans <{c}>)")).unwrap_or_default();
                        println!("❌ [{name}] {:?}{ctx}{label}", test.data);
                        println!("--- attendu\n{}\n--- obtenu\n{}\n", test.document, got);
                    }
                }
                Err(_) => {
                    crash += 1;
                    if verbose {
                        println!("💥 [{name}] {:?}{label}\n", test.data);
                    }
                }
            }
            }
        }
        println!("{name:<44} {file_pass:>4} / {file_total}");
    }

    let total = pass + fail + crash + hang;
    if needs_js > 0 {
        println!("\n🟨 JS requis : {needs_js} (scripted_*.dat, à reprendre avec un moteur JavaScript)");
    }
    let pct = if total > 0 { 100.0 * pass as f64 / total as f64 } else { 0.0 };
    println!("\n✅ réussis : {pass}   ❌ faux : {fail}   💥 panic : {crash}   ⏱️  boucles : {hang}");
    println!("Score : {pass}/{total} ({pct:.1} %)");
}
