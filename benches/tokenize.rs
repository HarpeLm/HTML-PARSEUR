//! Benchmark : notre tokenizer contre celui de html5ever (Servo).
//!
//!   cargo bench                       -> tout
//!   cargo bench -- blog               -> un seul document
//!   rapport HTML : target/criterion/report/index.html

use std::cell::Cell;
use std::fmt::Write;
use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, Token as H5Token, TokenSink, TokenSinkResult, Tokenizer as H5Tokenizer,
};

// ───────────── Documents de test (générés, donc reproductibles) ─────────────

const LOREM: &str = "Le navigateur lit la page caractère par caractère et construit des tokens. \
                     Chaque balise, chaque attribut et chaque morceau de texte passe par la machine à états. ";

/// Page de blog typique : un mélange de tout.
fn blog_page(articles: usize) -> String {
    let mut s = String::from(
        "<!DOCTYPE html>\n<html lang=\"fr\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>Mon blog &mdash; accueil</title>\n\
         <link rel=\"stylesheet\" href=\"/style.css?v=3&amp;theme=dark\">\n\
         <style>body { margin: 0 } .post > h2 { color: #333 }</style>\n</head>\n<body>\n",
    );
    for i in 0..articles {
        write!(
            s,
            "<article class=\"post post-{i}\" id=\"article-{i}\" data-author=\"auteur{a}\">\n\
             <!-- article {i} -->\n\
             <h2><a href=\"/articles/{i}?ref=home&amp;page=1\">Titre de l&apos;article n&deg;{i}</a></h2>\n\
             <p class=\"meta\">Publié le <time datetime=\"2026-09-{d:02}\">{d} septembre</time> &middot; 5&nbsp;min</p>\n\
             <p>{LOREM}{LOREM}<strong>Important</strong> : l&rsquo;entité &eacute; et &#233; et &#x20AC;.</p>\n\
             <ul><li>Premier point</li><li>Deuxième point</li><li><em>Troisième</em> point</li></ul>\n\
             <img src=\"/img/{i}.webp\" alt=\"Illustration {i}\" width=640 height=360 loading=lazy>\n\
             </article>\n",
            a = i % 7,
            d = i % 28 + 1,
        )
        .unwrap();
    }
    s.push_str("<script>\nfor (let i = 0; i < 10; i++) { if (i < 5 && i > 1) console.log(\"<b>\" + i); }\n</script>\n</body>\n</html>\n");
    s
}

/// Beaucoup de balises et d'attributs, très peu de texte.
fn tag_heavy_page(rows: usize) -> String {
    let mut s = String::from("<!DOCTYPE html><table class=\"data\">\n");
    for i in 0..rows {
        write!(
            s,
            "<tr id=\"r{i}\" class=\"row\"><td class=\"c1\">{i}</td><td class=\"c2\" data-v=\"{v}\">{v}</td>\
             <td><input type=\"checkbox\" name=\"sel\" value=\"{i}\" checked></td><td><br/></td></tr>\n",
            v = i * 3
        )
        .unwrap();
    }
    s.push_str("</table>");
    s
}

/// Presque uniquement du texte.
fn text_heavy_page(paragraphs: usize) -> String {
    let mut s = String::from("<!DOCTYPE html><body>\n");
    for _ in 0..paragraphs {
        s.push_str("<p>");
        for _ in 0..8 {
            s.push_str(LOREM);
        }
        s.push_str("</p>\n");
    }
    s
}

// ───────────── Les deux concurrents ─────────────

fn run_ours(input: &str) -> usize {
    html_tokenizer::Tokenizer::new(input).count()
}

/// html5ever envoie ses tokens à un "sink" : le nôtre se contente de les compter.
struct CountingSink(Cell<usize>);

impl TokenSink for CountingSink {
    type Handle = ();

    fn process_token(&self, token: H5Token, _line: u64) -> TokenSinkResult<()> {
        black_box(token);
        self.0.set(self.0.get() + 1);
        TokenSinkResult::Continue
    }
}

fn run_html5ever(input: &str) -> usize {
    let queue = BufferQueue::default();
    queue.push_back(StrTendril::from_slice(input));
    let tokenizer = H5Tokenizer::new(CountingSink(Cell::new(0)), Default::default());
    let _ = tokenizer.feed(&queue);
    tokenizer.end();
    tokenizer.sink.0.get()
}

// ───────────── Mesures ─────────────

fn bench(c: &mut Criterion) {
    let documents = [
        ("blog", blog_page(1500)),
        ("balises", tag_heavy_page(8000)),
        ("texte", text_heavy_page(700)),
    ];

    for (name, html) in &documents {
        let mut group = c.benchmark_group(*name);
        // Criterion affichera un débit en Mo/s en plus du temps.
        group.throughput(Throughput::Bytes(html.len() as u64));
        group.bench_function("html-tokenizer", |b| b.iter(|| run_ours(black_box(html))));
        group.bench_function("html5ever", |b| b.iter(|| run_html5ever(black_box(html))));
        group.finish();
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
