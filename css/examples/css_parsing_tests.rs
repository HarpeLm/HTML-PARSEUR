//! Lance les tests officiels css-parsing-tests (sous-module git) sur lumen-css.
//!
//!   cargo run --release -p lumen-css --example css_parsing_tests
//!   VERBOSE=1 cargo run -p lumen-css --example css_parsing_tests -- component_value_list

use std::{fs, panic, path::Path};

use lumen_css::{
    AtRule, BlockKind, ComponentValue, Declaration, Item, Numeric, ParseError, Parser,
    QualifiedRule, Token, TokenError, preprocess,
};
use serde_json::{Value, json};

fn number_json(kind: &str, n: &Numeric, unit: Option<&str>) -> Value {
    let kind_of = if n.is_integer { "integer" } else { "number" };
    let mut v = vec![json!(kind), json!(n.repr), json!(n.value), json!(kind_of)];
    if let Some(unit) = unit {
        v.push(json!(unit));
    }
    Value::Array(v)
}

/// Notre résultat, au format décrit dans le README de css-parsing-tests.
fn to_json(value: &ComponentValue) -> Value {
    match value {
        ComponentValue::Block { kind, contents } => {
            let name = match kind {
                BlockKind::Curly => "{}",
                BlockKind::Square => "[]",
                BlockKind::Paren => "()",
            };
            let mut v = vec![json!(name)];
            v.extend(contents.iter().map(to_json));
            Value::Array(v)
        }
        ComponentValue::Function { name, arguments } => {
            let mut v = vec![json!("function"), json!(name)];
            v.extend(arguments.iter().map(to_json));
            Value::Array(v)
        }
        ComponentValue::Token(token) => match token {
            Token::Ident(v) => json!(["ident", v]),
            Token::AtKeyword(v) => json!(["at-keyword", v]),
            Token::Hash { value, is_id } => {
                json!(["hash", value, if *is_id { "id" } else { "unrestricted" }])
            }
            Token::String(v) => json!(["string", v]),
            Token::BadString => json!(["error", "bad-string"]),
            Token::Url(v) => json!(["url", v]),
            Token::BadUrl => json!(["error", "bad-url"]),
            Token::Delim(c) => json!(c.to_string()),
            Token::Number(n) => number_json("number", n, None),
            Token::Percentage(n) => number_json("percentage", n, None),
            Token::Dimension { number, unit } => number_json("dimension", number, Some(unit)),
            Token::UnicodeRange { start, end } => json!(["unicode-range", start, end]),
            Token::IncludeMatch => json!("~="),
            Token::DashMatch => json!("|="),
            Token::PrefixMatch => json!("^="),
            Token::SuffixMatch => json!("$="),
            Token::SubstringMatch => json!("*="),
            Token::Column => json!("||"),
            Token::Whitespace => json!(" "),
            Token::Cdo => json!("<!--"),
            Token::Cdc => json!("-->"),
            Token::Colon => json!(":"),
            Token::Semicolon => json!(";"),
            Token::Comma => json!(","),
            Token::CloseSquare => json!(["error", "]"]),
            Token::CloseParen => json!(["error", ")"]),
            Token::CloseCurly => json!(["error", "}"]),
            Token::Error(TokenError::EofInString) => json!(["error", "eof-in-string"]),
            Token::Error(TokenError::EofInUrl) => json!(["error", "eof-in-url"]),
            // Jamais isolés : le parser en fait des blocs ou des fonctions.
            Token::OpenSquare | Token::OpenParen | Token::OpenCurly | Token::Function(_) => {
                json!(["error", "?"])
            }
        },
    }
}

fn list_json(values: &[ComponentValue]) -> Value {
    Value::Array(values.iter().map(to_json).collect())
}

fn declaration_json(d: &Declaration) -> Value {
    json!(["declaration", d.name, list_json(&d.value), d.important])
}

fn item_json(item: &Item) -> Value {
    match item {
        Item::Declaration(d) => declaration_json(d),
        Item::AtRule(AtRule {
            name,
            prelude,
            block,
        }) => {
            json!([
                "at-rule",
                name,
                list_json(prelude),
                block.as_ref().map(|b| list_json(b))
            ])
        }
        Item::QualifiedRule(QualifiedRule { prelude, block }) => {
            json!(["qualified rule", list_json(prelude), list_json(block)])
        }
        Item::Invalid => json!(["error", "invalid"]),
    }
}

fn error_json(e: ParseError) -> Value {
    match e {
        ParseError::Empty => json!(["error", "empty"]),
        ParseError::ExtraInput => json!(["error", "extra-input"]),
        ParseError::Invalid => json!(["error", "invalid"]),
    }
}

/// Les nombres sont comparés par leur valeur : 0 et 0.0 sont égaux.
fn normalize(v: &Value) -> Value {
    match v {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.iter().map(normalize).collect()),
        other => other.clone(),
    }
}

/// Exécute une "fonction" de la spec sur une entrée ; `None` = pas encore gérée.
fn run(file: &str, input: &str) -> Option<Value> {
    let css = preprocess(input);
    let mut parser = Parser::new(&css);
    Some(match file {
        "component_value_list" => Value::Array(
            parser
                .parse_component_value_list()
                .iter()
                .map(to_json)
                .collect(),
        ),
        "one_component_value" => match parser.parse_component_value() {
            Ok(value) => to_json(&value),
            Err(e) => error_json(e),
        },
        "one_declaration" => match parser.parse_declaration() {
            Ok(d) => declaration_json(&d),
            Err(e) => error_json(e),
        },
        "one_rule" => match parser.parse_rule() {
            Ok(item) => item_json(&item),
            Err(e) => error_json(e),
        },
        "declaration_list" => Value::Array(
            parser
                .parse_declaration_list()
                .iter()
                .map(item_json)
                .collect(),
        ),
        "blocks_contents" => Value::Array(
            parser
                .parse_block_contents()
                .iter()
                .map(item_json)
                .collect(),
        ),
        "rule_list" => Value::Array(parser.parse_rule_list().iter().map(item_json).collect()),
        "An+B" => match lumen_css::parse_an_plus_b(&parser.parse_component_value_list()) {
            Some((a, b)) => json!([a, b]),
            None => Value::Null,
        },
        f if f.starts_with("color_") => {
            let values = parser.parse_component_value_list();
            match lumen_css::parse_color(&values) {
                Some(lumen_css::Color::LightDark(light, dark)) => {
                    json!([light.to_css_color5(), dark.to_css_color5()])
                }
                Some(color) => json!(color.to_string()),
                // light-dark() invalide : les tests attendent deux null.
                None if input
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("light-dark(") =>
                {
                    json!([null, null])
                }
                None => Value::Null,
            }
        }
        "stylesheet" => Value::Array(parser.parse_stylesheet().iter().map(item_json).collect()),
        _ => return None,
    })
}

fn main() {
    let filter = std::env::args().nth(1);
    let verbose = std::env::var("VERBOSE").is_ok();
    panic::set_hook(Box::new(|_| {}));

    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("css-parsing-tests");
    let mut files: Vec<_> = fs::read_dir(&dir)
        .expect("css-parsing-tests introuvable : git submodule update --init")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();

    let (mut pass, mut fail, mut crash, mut skip) = (0, 0, 0, 0);
    for path in files {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        if filter.as_ref().is_some_and(|f| !name.contains(f.as_str())) {
            continue;
        }
        let tests: Vec<Value> = match serde_json::from_str(&fs::read_to_string(&path).unwrap()) {
            Ok(t) => t,
            Err(e) => {
                println!("{name:<24} illisible : {e}");
                continue;
            }
        };
        let (mut file_pass, mut file_total) = (0, 0);
        for pair in tests.chunks(2) {
            let Some(input) = pair[0].as_str() else {
                skip += 1; // entrée en octets (stylesheet_bytes) : pas encore gérée
                continue;
            };
            let expected = normalize(&pair[1]);
            let result = panic::catch_unwind(|| run(&name, input));
            match result {
                Ok(None) => {
                    skip += 1;
                    continue;
                }
                Ok(Some(got)) if normalize(&got) == expected => {
                    pass += 1;
                    file_pass += 1;
                }
                Ok(Some(got)) => {
                    fail += 1;
                    if verbose {
                        println!(
                            "❌ [{name}] {input:?}\n   attendu : {expected}\n   obtenu  : {}\n",
                            normalize(&got)
                        );
                    }
                }
                Err(_) => {
                    crash += 1;
                    if verbose {
                        println!("💥 [{name}] {input:?}\n");
                    }
                }
            }
            file_total += 1;
        }
        if file_total > 0 {
            println!("{name:<24} {file_pass:>4} / {file_total}");
        }
    }
    let total = pass + fail + crash;
    let pct = if total > 0 {
        100.0 * pass as f64 / total as f64
    } else {
        0.0
    };
    println!(
        "\n✅ réussis : {pass}   ❌ faux : {fail}   💥 panic : {crash}   ⏭️  pas encore gérés : {skip}"
    );
    println!("Score : {pass}/{total} ({pct:.1} %)");
}
