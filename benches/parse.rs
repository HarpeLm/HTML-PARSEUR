//! Benchmark du parsing COMPLET (tokenizer + construction du DOM) :
//! notre parser contre html5ever (Servo) avec un DOM en arène.
//!
//!   cargo bench --bench parse

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use html5ever::tendril::StrTendril;

mod docs;
mod html5ever_arena;

use docs::{blog_page, tag_heavy_page, text_heavy_page};

fn ours(html: &str) -> html_tokenizer::dom::Document {
    html_tokenizer::parse_document(html)
}

fn count_theirs(node: html5ever_arena::Ref<'_>) -> usize {
    let mut count = 1;
    let mut child = node.first_child.get();
    while let Some(c) = child {
        count += count_theirs(c);
        child = c.next_sibling.get();
    }
    count
}

fn count_ours(doc: &html_tokenizer::dom::Document) -> usize {
    1 + doc.descendants(html_tokenizer::dom::NodeId::DOCUMENT).count()
}

fn bench(c: &mut Criterion) {
    let documents = [
        ("blog", blog_page(1500)),
        ("balises", tag_heavy_page(8000)),
        ("texte", text_heavy_page(700)),
    ];

    for (name, html) in &documents {
        // Vérification : les deux parsers doivent produire le même nombre de nœuds,
        // sinon on ne compare pas le même travail.
        let arena = typed_arena::Arena::new();
        let (a, b) = (count_ours(&ours(html)), count_theirs(html5ever_arena::parse(html, &arena)));
        assert_eq!(a, b, "{name} : {a} nœuds chez nous, {b} chez html5ever");
        println!("{name} : {a} nœuds dans les deux DOM");

        let mut group = c.benchmark_group(format!("parse-{name}"));
        group.throughput(Throughput::Bytes(html.len() as u64));
        group.bench_function("html-tokenizer", |b| b.iter(|| ours(black_box(html))));
        group.bench_function("html5ever", |b| {
            b.iter(|| {
                let arena = typed_arena::Arena::new();
                let root = html5ever_arena::parse(black_box(html), &arena);
                black_box(root);
            })
        });

        // Variantes SANS copie de la page, des deux côtés. La préparation de
        // l'entrée (clone de la String, clone du tendril) n'est pas chronométrée.
        group.bench_function("html-tokenizer-sans-copie", |b| {
            b.iter_batched(
                || html.clone(),
                |owned| black_box(html_tokenizer::parse_document_owned(owned)),
                BatchSize::LargeInput,
            )
        });
        let tendril = StrTendril::from_slice(html);
        group.bench_function("html5ever-sans-copie", |b| {
            b.iter(|| {
                let arena = typed_arena::Arena::new();
                let root = html5ever_arena::parse_tendril(black_box(tendril.clone()), &arena);
                black_box(root);
            })
        });
        group.finish();
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
