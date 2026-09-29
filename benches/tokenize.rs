//! Benchmark : notre tokenizer contre celui de html5ever (Servo).
//!
//!   cargo bench                       -> tout
//!   cargo bench -- blog               -> un seul document
//!   rapport HTML : target/criterion/report/index.html

use std::cell::Cell;
use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, Token as H5Token, TokenSink, TokenSinkResult, Tokenizer as H5Tokenizer,
};

mod docs;
use docs::{blog_page, tag_heavy_page, text_heavy_page};

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
    run_html5ever_tendril(StrTendril::from_slice(input))
}

/// Même chose, mais la page est déjà dans un tendril : pas de copie mesurée.
/// C'est la comparaison équitable (nous non plus, on ne copie pas l'entrée).
fn run_html5ever_tendril(tendril: StrTendril) -> usize {
    let queue = BufferQueue::default();
    queue.push_back(tendril);
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
        // Cloner un tendril est gratuit (compteur de références), pas de copie.
        let tendril = StrTendril::from_slice(html);
        group.bench_function("html5ever-sans-copie", |b| {
            b.iter(|| run_html5ever_tendril(black_box(tendril.clone())))
        });
        group.finish();
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
