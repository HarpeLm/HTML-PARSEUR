//! Lance les tests de construction d'arbre de WPT (fichiers .dat) sur notre parser.
//!
//!   ./tools/fetch_wpt_tests.sh                          -> télécharger les tests (une fois)
//!   cargo run --release --example tree_construction     -> score par fichier
//!   cargo run --release --example tree_construction -- tests1   -> un seul fichier
//!   VERBOSE=1 cargo run --release --example tree_construction -- tests1

use std::{fs, panic, path::Path};

use html_tokenizer::parse_document;

struct Test {
    data: String,
    document: String,
    fragment: bool,
    script_on: bool,
}

/// Découpe un fichier .dat en tests (format décrit dans le README de WPT).
fn parse_dat(content: &str) -> Vec<Test> {
    let mut tests = Vec::new();
    let mut section = "";
    let mut data: Vec<&str> = Vec::new();
    let mut document: Vec<&str> = Vec::new();
    let (mut fragment, mut script_on) = (false, false);

    let mut flush = |data: &mut Vec<&str>, document: &mut Vec<&str>, fragment: &mut bool, script_on: &mut bool| {
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
            fragment: *fragment,
            script_on: *script_on,
        });
        data.clear();
        document.clear();
        *fragment = false;
        *script_on = false;
    };

    for line in content.split('\n') {
        match line {
            "#data" => {
                flush(&mut data, &mut document, &mut fragment, &mut script_on);
                section = "data";
            }
            "#errors" | "#new-errors" => section = "errors",
            "#document-fragment" => {
                fragment = true;
                section = "fragment";
            }
            "#script-on" => script_on = true,
            "#script-off" => {}
            "#document" => section = "document",
            _ => match section {
                "data" => data.push(line),
                "document" => document.push(line),
                _ => {}
            },
        }
    }
    flush(&mut data, &mut document, &mut fragment, &mut script_on);
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

    let (mut pass, mut fail, mut crash, mut skip) = (0, 0, 0, 0);

    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if filter.as_ref().is_some_and(|f| !name.contains(f.as_str())) {
            continue;
        }
        let content = String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned();
        let (mut file_pass, mut file_total) = (0, 0);

        for test in parse_dat(&content) {
            // Pas encore gérés : fragments (innerHTML) et JavaScript activé.
            if test.fragment || test.script_on {
                skip += 1;
                continue;
            }
            file_total += 1;
            let result = panic::catch_unwind(|| parse_document(&test.data).to_test_string());
            match result {
                Ok(got) if got == test.document => {
                    pass += 1;
                    file_pass += 1;
                }
                Ok(got) => {
                    fail += 1;
                    if verbose {
                        println!("❌ [{name}] {:?}", test.data);
                        println!("--- attendu\n{}\n--- obtenu\n{}\n", test.document, got);
                    }
                }
                Err(_) => {
                    crash += 1;
                    if verbose {
                        println!("💥 [{name}] {:?}\n", test.data);
                    }
                }
            }
        }
        println!("{name:<44} {file_pass:>4} / {file_total}");
    }

    let total = pass + fail + crash;
    let pct = if total > 0 { 100.0 * pass as f64 / total as f64 } else { 0.0 };
    println!("\n✅ réussis : {pass}   ❌ faux : {fail}   💥 panic : {crash}   ⏭️  ignorés : {skip}");
    println!("Score : {pass}/{total} ({pct:.1} %)");
}
