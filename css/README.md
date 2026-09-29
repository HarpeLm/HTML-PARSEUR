# lumen-css

Parser CSS écrit en Rust, de zéro, en suivant les specs
[CSS Syntax Level 3](https://www.w3.org/TR/css-syntax-3/) et
[Selectors Level 4](https://www.w3.org/TR/selectors-4/). C'est une brique de
[Lumen](../README.md), un navigateur web écrit de zéro.

- **Syntaxe complète** : tokenizer, blocs, fonctions, règles, at-rules,
  déclarations, `!important`, CSS imbriqué (*nesting*).
- **Sélecteurs** : parsing, spécificité et correspondance avec n'importe quel DOM
  (via le trait `selectors::Element`).
- **Couleurs** (CSS Color 4 et 5) : mots-clés, `#hex`, `rgb()`, `hsl()`, `hwb()`,
  `lab()`, `lch()`, `oklab()`, `oklch()`, `color()`, `device-cmyk()`, `light-dark()`.
- **Aucune dépendance** à l'exécution.

## Utilisation

```rust
use lumen_css::{preprocess, Item, Parser};
use lumen_css::selectors::parse_selector_list;

let css = preprocess("nav > a.actif { color: red !important }");
for item in Parser::new(&css).parse_stylesheet() {
    if let Item::QualifiedRule(rule) = item {
        let selectors = parse_selector_list(&rule.prelude).expect("sélecteur valide");
        println!("spécificité : {:?}", selectors.0[0].specificity()); // (0, 1, 2)
    }
}
```

## Conformité et vérifications

| Vérification | Résultat |
|---|---|
| [css-parsing-tests](https://github.com/SimonSapin/css-parsing-tests) (syntaxe, `An+B`, couleurs) | **8 338 / 8 338** |
| Sélecteurs : test différentiel contre le moteur de Servo (via `scraper`), 5 vraies pages | **1 825 comparaisons, 0 différence** |
| Spécificité : exemples de la spec Selectors 4 | 13 / 13 |
| Robustesse : CSS aléatoire (`tests/robustesse.rs`) | **2 000 000 feuilles, 0 panique** |

Pas encore traitées : les feuilles de style en octets (encodages, 28 tests). La
fonction `rgb()` n'étant couverte par aucun fichier de la suite, elle a ses propres
tests unitaires.

```bash
git submodule update --init                                   # tests officiels
cargo run --release -p lumen-css --example css_parsing_tests   # score
cargo test --release -p lumen-css                              # tous les tests
```

Les tests suivent la suite css-parsing-tests, qui mêle deux versions de la spec :
elle attend par exemple les tokens `unicode-range`, retirés de la spec en 2021 mais
gardés par cssparser (Servo).

## Benchmarks

Tokenizer contre [cssparser](https://github.com/servo/rust-cssparser), le parser
CSS de Servo et Firefox, sur un Apple M5 (`cargo bench -p lumen-css`). cssparser
ne construit pas d'arbre : les deux parcourent tous les tokens, blocs compris.

| CSS | lumen-css | cssparser | Rapport |
|---|---|---|---|
| MDN, `<style>` intégrés (65 Ko, réel) | 594 Mo/s | 587 Mo/s | ×1,01 |
| Wikipedia EN, `<style>` intégrés (20 Ko, réel) | 482 Mo/s | 515 Mo/s | ×0,94 |
| Feuille générée (883 Ko) | 402 Mo/s | 416 Mo/s | ×0,97 |

Point de départ (version « correcte d'abord ») : 224 à 264 Mo/s, soit la moitié
de cssparser. Les gains viennent surtout de la lecture par octets, sans décoder
l'UTF-8, des identifiants, des espaces, des chaînes et des `url()`. Détail et
mesures de chaque étape dans l'historique git.

## Licence

Au choix : [Apache 2.0](LICENSE-APACHE) ou [MIT](LICENSE-MIT).
