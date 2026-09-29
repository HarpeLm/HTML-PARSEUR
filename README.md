# html-parseur

Tokenizer et parser HTML écrits en Rust, de zéro, en suivant la
[spec WHATWG](https://html.spec.whatwg.org/multipage/parsing.html).
C'est la première brique d'un navigateur web.

- **100 % conforme** : tous les tests officiels du tokenizer
  ([html5lib-tests](https://github.com/html5lib/html5lib-tests)) et de la
  construction d'arbre ([WPT](https://github.com/web-platform-tests/wpt/tree/master/html/syntax/parsing)).
- **Rapide** : plus rapide que [html5ever](https://github.com/servo/html5ever)
  (le parser de Servo) sur des pages réalistes. Voir les [benchmarks](#benchmarks).
- **Aucune dépendance** à l'exécution.
- Suit la spec la plus récente : nouveau `<select>` personnalisable (2025),
  processing instructions `<?cible données?>` (2026).

## Utilisation

```rust
use html_parseur::{parse_document, dom::NodeId};

let doc = parse_document("<p>Bonjour <b>le monde</b>");

// Tous les textes de la page
for node in doc.descendants(NodeId::DOCUMENT) {
    if let Some(text) = doc.text(node) {
        println!("{text}");
    }
}

// L'arbre au format des tests html5lib :
// | <html>
// |   <head>
// |   <body>
// |     <p>
// |       "Bonjour "
// |       <b>
// |         "le monde"
println!("{}", doc.to_test_string());
```

Autres points d'entrée :

| Fonction | Usage |
|---|---|
| `parse_document(&str)` | page complète (copie la page une fois) |
| `parse_document_owned(String)` | page complète, **sans copie** (la page est reprise par le document) |
| `parse_fragment(html, ns, contexte, options)` | fragment, comme `element.innerHTML = html` |
| `ParseOptions { scripting }` | JavaScript activé ou non (change l'interprétation de `<noscript>`) |
| `Tokenizer::new(&str)` | le tokenizer seul, un itérateur de `Token` |

La documentation complète : `cargo doc --open`.

## Conformité

| Suite | Résultat |
|---|---|
| html5lib-tests, tokenizer | **7017 / 7017** |
| WPT, construction d'arbre (documents, fragments, avec et sans JavaScript) | **3870 / 3870** |
| Robustesse : HTML aléatoire (`tests/robustesse.rs`) | **2 000 000 pages, 0 panique** |

Mis de côté, et affiché comme tel par les bancs de test :

- 📜 11 tests html5lib « obsolètes » : ils attendent encore des commentaires
  pour `<?...>`, alors que la spec (et WPT) en font des processing instructions ;
- 🟨 6 tests WPT « JS requis » (`scripted_*.dat`) : ils exécutent du JavaScript
  pendant le parsing ;
- 4 tests html5lib contenant des surrogates UTF-16 isolés, qu'une `String` Rust
  ne peut pas représenter.

## Comment ça marche

- **Tokenizer** : la machine à états de la spec (§13.2.5, ~80 états), avec des
  chemins rapides qui copient le texte par blocs.
- **Scan SIMD** (`src/scan.rs`) : recherche du prochain caractère spécial 16 octets
  à la fois avec NEON sur ARM64 (boucle simple ailleurs).
- **Zero-copy** : les tokens empruntent des morceaux de la page (`Cow<str>`) ; les
  nœuds texte du DOM sont des *plages* de la page, pas des copies.
- **Parser** (§13.2.6) : 23 modes d'insertion, *adoption agency algorithm*,
  *foster parenting*, contenu SVG/MathML, templates, fragments.
- **Interning** (`src/atoms.rs`) : chaque nom de balise devient un `u32`.
- **DOM en arène** (`src/dom.rs`) : un `Vec` de nœuds, enfants en liste chaînée.

## Tests

```bash
# Tokenizer : html5lib-tests (sous-module git)
git submodule update --init
cargo run --release --example html5lib

# Parser : tests WPT (téléchargement partiel, une fois)
./tools/fetch_wpt_tests.sh
cargo run --release --example tree_construction

# Tests unitaires, exemples de la documentation et robustesse
cargo test

# Robustesse : campagne longue de HTML aléatoire (aucune panique tolérée)
FUZZ_CAS=2000000 cargo test --release --test robustesse
```

Le test de robustesse (`tests/robustesse.rs`) génère du HTML tordu (formatage
mal imbriqué, tableaux, templates, SVG, entités, `\0`...) et vérifie que le
parser ne panique jamais et que l'arbre reste cohérent, en document et en
fragment. Dernière campagne : 2 millions de pages, aucune panique.

Le code passe `cargo fmt --check` et
`cargo clippy --all-targets --all-features -- -D warnings` sans avertissement.

`VERBOSE=1` affiche le détail de chaque échec ; un argument filtre les fichiers
(`cargo run --release --example tree_construction -- template`).

## Benchmarks

Débits sur un Apple M5, mesurés avec [criterion](https://github.com/criterion-rs/criterion.rs)
contre html5ever 0.40 (DOM en arène de son exemple officiel), sur trois pages
générées (`benches/docs/`). Les deux parsers sont mesurés dans le même lancement ;
le bruit de mesure est d'environ ±3 %.

**Parsing complet** (tokenizer + DOM), sans copie de la page des deux côtés :

| Page | html-parseur | html5ever | Rapport |
|---|---|---|---|
| Blog (texte, balises, entités) | 130,5 Mo/s | 120,1 Mo/s | ×1,09 |
| Balises (grand tableau) | 103,9 Mo/s | 79,7 Mo/s | ×1,30 |
| Texte (longs paragraphes) | 5,25 Go/s | 5,59 Go/s | ×0,94 |

**Tokenizer seul** :

| Page | html-parseur | html5ever | Rapport |
|---|---|---|---|
| Blog | 204 Mo/s | 149 Mo/s | ×1,36 |
| Balises | 180 Mo/s | 96 Mo/s | ×1,88 |
| Texte | 8,88 Go/s | 7,98 Go/s | ×1,11 |

Attention : html5ever fait plus de travail que nous (streaming, numéros de ligne,
erreurs de parsing) ; voir les limites ci-dessous.

```bash
cargo bench --bench parse      # parsing complet
cargo bench --bench tokenize   # tokenizer seul
```

Profilage (macOS, nécessite `cargo install inferno rustfilt`) :

```bash
./profiling/profile.sh parse-v5 profile_parse       # génère profiling/parse-v5.svg
DOC=texte ./profiling/profile.sh texte-v5 profile_parse
```

## Limites actuelles

- Toute la page doit être en mémoire (pas de **streaming**).
- L'entrée doit être de l'UTF-8 (pas de détection ni de décodage d'**encodage**).
- Les scripts ne sont pas exécutés (pas de pause du parser sur `</script>`,
  pas de `document.write`).
- Pas de numéros de ligne ni de messages d'erreur de parsing.
- Le scan SIMD n'existe que pour ARM64.
- Le DOM est pensé comme résultat de parsing : il ne libère pas les nœuds supprimés.

## Structure

```
src/
├── lib.rs            API publique
├── tokenizer.rs      machine à états du tokenizer
├── token.rs          types de tokens
├── scan.rs           recherche SIMD (NEON)
├── char_ref.rs       références de caractères (&amp; &#233;)
├── entities.rs       table des 2231 entités (générée par tools/gen_entities.py)
├── tree_builder.rs   construction de l'arbre (modes d'insertion)
├── foreign.rs        tables SVG/MathML
├── atoms.rs          interning des noms de balises
└── dom.rs            l'arbre DOM
examples/             bancs de test (html5lib, WPT) et programmes de profilage
benches/              benchmarks criterion
tools/                génération des entités, récupération des tests WPT
profiling/            script de profilage et historique des flame graphs
```

## Crédits

- Tests : [html5lib-tests](https://github.com/html5lib/html5lib-tests) et
  [web-platform-tests](https://github.com/web-platform-tests/wpt).
- `benches/html5ever_arena/` reprend l'exemple `arena.rs` de
  [html5ever](https://github.com/servo/html5ever) (MIT / Apache-2.0), uniquement
  pour les benchmarks.
