//! Benchmark : notre tokenizer CSS contre cssparser (le parser CSS de Servo et
//! Firefox).
//!
//!   cargo bench -p lumen-css
//!
//! cssparser ne construit pas d'arbre : il parcourt les tokens en streaming. On
//! compare donc tokenizer contre tokenizer (cssparser entre dans tous les blocs).
//! La construction de notre arbre de component values est mesurée à part.

use std::fmt::Write;
use std::hint::black_box;
use std::path::Path;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

/// Le CSS des balises <style> d'une des vraies pages de html/benches/pages/.
fn inline_css(page: &str) -> String {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../html/benches/pages/{page}.html"));
    let html = std::fs::read_to_string(path).unwrap();
    let mut css = String::new();
    let mut rest = html.as_str();
    while let Some(start) = rest.find("<style") {
        let after = &rest[start..];
        let open_end = after.find('>').unwrap() + 1;
        let close = after.find("</style>").unwrap();
        css.push_str(&after[open_end..close]);
        rest = &after[close..];
    }
    css
}

/// Une grande feuille de style typique, générée (reproductible).
fn generated_css(rules: usize) -> String {
    let mut css = String::from(
        "/* Feuille générée pour le benchmark */\n:root { --accent: #0b57d0; --gap: 1.5rem; }\n",
    );
    for i in 0..rules {
        if i % 50 == 0 {
            writeln!(
                css,
                "@media (min-width: {}px) and (prefers-color-scheme: dark) {{",
                320 + i
            )
            .unwrap();
        }
        writeln!(
            css,
            ".card-{i} > .title:not(.hidden), #main-{i} ul li:nth-child(2n+1) a[href^=\"https://\"] {{\n  \
             color: rgb({r}, {g}, 200); margin: 0 auto {m}px; padding: calc(var(--gap) * 2) 4%;\n  \
             background: url(\"/img/bg-{i}.webp\") no-repeat center / cover, linear-gradient(90deg, #fff 0%, #eee 100%);\n  \
             font: 600 1.125rem/1.4 \"Inter\", system-ui, sans-serif; transition: opacity .2s ease-in-out;\n}}",
            r = i % 256,
            g = (i * 7) % 256,
            m = i % 40,
        )
        .unwrap();
        if i % 50 == 49 {
            css.push_str("}\n");
        }
    }
    css
}

fn ours_tokens(css: &str) -> usize {
    lumen_css::Tokenizer::new(css).count()
}

fn ours_tree(css: &str) -> usize {
    lumen_css::Parser::new(css)
        .parse_component_value_list()
        .len()
}

/// cssparser : parcours complet, en entrant dans chaque bloc et fonction.
fn cssparser_tokens(css: &str) -> usize {
    fn walk(parser: &mut cssparser::Parser) -> usize {
        let mut count = 0;
        while let Ok(token) = parser.next_including_whitespace_and_comments() {
            count += 1;
            let is_block = matches!(
                token,
                cssparser::Token::Function(_)
                    | cssparser::Token::ParenthesisBlock
                    | cssparser::Token::SquareBracketBlock
                    | cssparser::Token::CurlyBracketBlock
            );
            black_box(token);
            if is_block {
                count += parser
                    .parse_nested_block(|p| Ok::<usize, cssparser::ParseError<()>>(walk(p)))
                    .unwrap_or(0);
            }
        }
        count
    }
    walk(&mut cssparser::Parser::new(css))
}

fn bench(c: &mut Criterion) {
    let corpus = [
        ("mdn-inline", inline_css("mdn-fr-table")),
        ("wikipedia-en-inline", inline_css("wikipedia-en-html")),
        ("genere-1mo", generated_css(2400)),
    ];
    for (name, css) in &corpus {
        let css = lumen_css::preprocess(css).into_owned();
        println!("{name} : {} Ko", css.len() / 1024);
        let mut group = c.benchmark_group(format!("css-{name}"));
        group.throughput(Throughput::Bytes(css.len() as u64));
        group.bench_function("tokenizer/lumen-css", |b| {
            b.iter(|| ours_tokens(black_box(&css)))
        });
        group.bench_function("tokenizer/cssparser", |b| {
            b.iter(|| cssparser_tokens(black_box(&css)))
        });
        group.bench_function("arbre/lumen-css", |b| b.iter(|| ours_tree(black_box(&css))));
        group.finish();
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
