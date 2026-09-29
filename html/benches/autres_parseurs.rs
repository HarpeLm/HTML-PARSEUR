//! Comparaison avec d'AUTRES parsers Rust, sur les 5 vraies pages de benches/pages/.
//!
//!   cargo bench --bench autres_parseurs
//!
//! Attention, ils ne font pas tous le même travail :
//! - html5ever, html5gum : tokenizers conformes à la spec WHATWG ;
//! - lol_html (Cloudflare) : réécriture en STREAMING ; recopie la page en sortie
//!   et ne décode pas les entités du texte (travail différent) ;
//! - tl : DOM volontairement NON conforme à la spec (plus simple, plus rapide).

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

fn html5gum_tokenize(html: &str) -> usize {
    let mut count = 0;
    for token in html5gum::Tokenizer::new(html) {
        black_box(token.unwrap());
        count += 1;
    }
    count
}

/// lol_html n'analyse que ce qu'on lui demande : on lui demande TOUS les éléments
/// (avec leurs attributs) et TOUS les textes, pour qu'il fasse le travail complet.
fn lol_html_tokenize(html: &str) -> usize {
    use lol_html::{HtmlRewriter, Settings, doc_text, element};
    let count = Cell::new(0usize);
    let mut rewriter = HtmlRewriter::new(
        Settings::new()
            .append_element_content_handler(element!("*", |el| {
                count.set(count.get() + 1);
                for attr in el.attributes() {
                    black_box(attr.value());
                }
                Ok(())
            }))
            .append_document_content_handler(doc_text!(|t| {
                black_box(t.as_str());
                Ok(())
            })),
        |chunk: &[u8]| {
            black_box(chunk);
        },
    );
    rewriter.write(html.as_bytes()).unwrap();
    rewriter.end().unwrap();
    count.get()
}

fn bench(c: &mut Criterion) {
    for name in PAGES {
        let html = load(name);
        let tendril = StrTendril::from_slice(&html);

        // Nombre de nœuds : identique chez nous et chez html5ever (conformes),
        // différent chez tl (non conforme).
        let ours = 1 + html_parseur::parse_document(&html)
            .descendants(html_parseur::dom::NodeId::DOCUMENT)
            .count();
        let tl_nodes = tl::parse(&html, tl::ParserOptions::default())
            .unwrap()
            .nodes()
            .len();
        println!("{name} : {ours} nœuds chez nous, {tl_nodes} chez tl");

        let mut group = c.benchmark_group(format!("autres-{name}"));
        group.throughput(Throughput::Bytes(html.len() as u64));

        group.bench_function("tokenizer/html-parseur", |b| {
            b.iter(|| html_parseur::Tokenizer::new(black_box(&html)).count())
        });
        group.bench_function("tokenizer/html5ever", |b| {
            b.iter(|| html5ever_tokenize(black_box(tendril.clone())))
        });
        group.bench_function("tokenizer/html5gum", |b| {
            b.iter(|| html5gum_tokenize(black_box(&html)))
        });
        group.bench_function("tokenizer/lol_html", |b| {
            b.iter(|| lol_html_tokenize(black_box(&html)))
        });

        group.bench_function("dom/html-parseur", |b| {
            b.iter_batched(
                || html.clone(),
                |owned| black_box(html_parseur::parse_document_owned(owned)),
                BatchSize::LargeInput,
            )
        });
        group.bench_function("dom/html5ever", |b| {
            b.iter(|| {
                let arena = typed_arena::Arena::new();
                black_box(html5ever_arena::parse_tendril(
                    black_box(tendril.clone()),
                    &arena,
                ));
            })
        });
        group.bench_function("dom/tl (non conforme)", |b| {
            b.iter(|| black_box(tl::parse(black_box(&html), tl::ParserOptions::default())))
        });
        group.finish();
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
