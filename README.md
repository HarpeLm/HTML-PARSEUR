# Lumen

Un navigateur web écrit de zéro en Rust, brique par brique. Chaque brique est un
crate de ce workspace, testé contre les suites officielles (html5lib, WPT…) puis
optimisé en mesurant.

| Brique | Dossier | État |
|---|---|---|
| Parser HTML (tokenizer, arbre DOM) | [`html/`](html/) | ✅ v0.1 : 100 % html5lib et WPT, plus rapide que html5ever |
| Parser CSS | [`css/`](css/) | 🚧 en cours : syntaxe et `An+B` 277 / 277 tests ; sélecteurs identiques à Servo sur 1 825 comparaisons |
| DOM complet, styles, mise en page, rendu, réseau, JavaScript… | — | à venir |

## Démarrer

```bash
git clone --recurse-submodules https://github.com/HarpeLm/lumen.git
cd lumen
cargo test                    # tous les tests de toutes les briques
```

Chaque brique a son propre README (tests, benchmarks, limites) :
[html/README.md](html/README.md).

## Licence

Au choix : [Apache 2.0](LICENSE-APACHE) ou [MIT](LICENSE-MIT). Sauf mention
contraire, toute contribution proposée pour inclusion dans ce projet est placée
sous cette même double licence, sans condition supplémentaire.
