//! Benchmark sur de VRAIES pages du web (benches/pages/, voir SOURCES.md pour
//! leurs sources et licences) : notre parser contre html5ever, sans copie de la
//! page des deux côtés.
//!
//!   cargo bench --bench vraies_pages

use std::cell::Cell;
use std::hint::black_box;
use std::path::Path;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, Token as H5Token, TokenSink, TokenSinkResult, Tokenizer as H5Tokenizer,
};

mod html5ever_arena;

const PAGES: &[&str] = &[
    "wikipedia-fr-rust",
    "wikipedia-en-html",
    "whatwg-parsing",
    "rust-doc-vec",
    "mdn-fr-table",
];

fn load(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("benches/pages")
        .join(format!("{name}.html"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} : {e}", path.display()))
}

// ───────────── Tokenizer seul ─────────────

struct CountingSink(Cell<usize>);

impl TokenSink for CountingSink {
    type Handle = ();

    fn process_token(&self, token: H5Token, _line: u64) -> TokenSinkResult<()> {
        black_box(token);
        self.0.set(self.0.get() + 1);
        TokenSinkResult::Continue
    }
}

fn html5ever_tokenize(tendril: StrTendril) -> usize {
    let queue = BufferQueue::default();
    queue.push_back(tendril);
    let tokenizer = H5Tokenizer::new(CountingSink(Cell::new(0)), Default::default());
    let _ = tokenizer.feed(&queue);
    tokenizer.end();
    tokenizer.sink.0.get()
}

// ───────────── Vérification : même arbre des deux côtés ─────────────

fn count_theirs(node: html5ever_arena::Ref<'_>) -> usize {
    let mut count = 1;
    let mut child = node.first_child.get();
    while let Some(c) = child {
        count += count_theirs(c);
        child = c.next_sibling.get();
    }
    count
}

fn bench(c: &mut Criterion) {
    for name in PAGES {
        let html = load(name);
        let tendril = StrTendril::from_slice(&html);

        let ours = html_parseur::parse_document(&html);
        let ours_nodes = 1 + ours
            .descendants(html_parseur::dom::NodeId::DOCUMENT)
            .count();
        let arena = typed_arena::Arena::new();
        let theirs_nodes = count_theirs(html5ever_arena::parse_tendril(tendril.clone(), &arena));
        assert_eq!(
            ours_nodes, theirs_nodes,
            "{name} : {ours_nodes} nœuds chez nous, {theirs_nodes} chez html5ever"
        );
        println!(
            "{name} : {} Ko, {ours_nodes} nœuds dans les deux DOM",
            html.len() / 1024
        );

        let mut group = c.benchmark_group(format!("reel-{name}"));
        group.throughput(Throughput::Bytes(html.len() as u64));

        group.bench_function("tokenizer/html-parseur", |b| {
            b.iter(|| html_parseur::Tokenizer::new(black_box(&html)).count())
        });
        group.bench_function("tokenizer/html5ever", |b| {
            b.iter(|| html5ever_tokenize(black_box(tendril.clone())))
        });
        group.bench_function("parse/html-parseur", |b| {
            b.iter_batched(
                || html.clone(),
                |owned| black_box(html_parseur::parse_document_owned(owned)),
                BatchSize::LargeInput,
            )
        });
        group.bench_function("parse/html5ever", |b| {
            b.iter(|| {
                let arena = typed_arena::Arena::new();
                black_box(html5ever_arena::parse_tendril(
                    black_box(tendril.clone()),
                    &arena,
                ));
            })
        });
        group.finish();
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
