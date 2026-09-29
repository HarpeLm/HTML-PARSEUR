# lumen-style

La cascade CSS : le **style calculé** de chaque élément d'un document HTML.
C'est une brique de [Lumen](../README.md), un navigateur web écrit de zéro, qui
assemble les précédentes : le DOM de [`html-parseur`](../html/) et, de
[`lumen-css`](../css/), les sélecteurs, `@media`, `var()` et les propriétés.

1. **Collecter les règles** : la feuille par défaut du navigateur
   ([`src/ua.css`](src/ua.css), [`src/mathml.css`](src/mathml.css)), les
   `<style>` du document (avec `media="..."`, `@media`, `@supports`), l'attribut
   `style=""`.
2. **Trier les déclarations** : origine et `!important`, attribut `style`,
   spécificité, ordre d'apparition.
3. **Calculer les valeurs** : héritage, `initial` / `inherit` / `unset`, `var()`,
   unités converties en px (`em`, `rem`, `vw`, `calc()`...), mots-clés de taille
   de police, `bolder` / `lighter`, éléments flottants ou positionnés changés en
   blocs.

## Utilisation

```rust
use html_parseur::parse_document;
use lumen_css::media::Environment;
use lumen_style::style_document;

let doc = parse_document("<style>p { font-size: 2em }</style><p>Bonjour");
let styles = style_document(&doc, &Environment::default());
// styles.get(id).resolved("font-size") == Some("32px") pour le <p>
```

## Vérification contre Chromium

Pas de suite officielle utilisable hors navigateur : on compare à
`getComputedStyle()` de Chromium 152, élément par élément.

| Test | Résultat |
|---|---|
| 42 pages écrites pour chaque règle de la cascade (`tests/oracle_cascade.rs`) | **5 509 / 5 509** valeurs identiques |
| Wikipédia FR, *Rust* (5 995 éléments) | **83 929 / 83 929** |
| Wikipedia EN, *HTML* (7 864 éléments) | **110 085 / 110 085** |
| Spec WHATWG, *Parsing* (13 650 éléments) | **191 085 / 191 085** |
| Doc Rust, `Vec` (16 086 éléments) | **225 204 / 225 204** |
| MDN FR, `<table>` (2 166 éléments) | 30 313 / 30 324 (voir plus bas) |

Propriétés comparées : `display`, `position`, `float`, `visibility`,
`box-sizing`, `opacity`, `z-index`, `font-size`, `font-weight`, `line-height`,
`margin-top`, `margin-left`, `padding-top`, `padding-left`.

Méthode : chaque page est chargée par Chromium dans une iframe (800 × 600 px pour
les pages de test, 1024 × 768 pour les vraies pages). Les vraies pages sont
servies avec une politique de sécurité (CSP) qui bloque tout ce qui est externe :
Chromium n'applique donc, comme Lumen, que sa feuille par défaut, les `<style>` et
les attributs `style`. Scripts et capture : [tests/oracle/](tests/oracle/).

**Ce qui n'est pas encore vérifié ou pas encore géré** :

- Les marges et retraits en `auto` ou en `%` : `getComputedStyle` les donne après
  la mise en page, que Lumen ne fait pas encore (34 valeurs, non comparées).
- Le shadow DOM déclaratif (`<template shadowrootmode>`) : html-parseur ne le gère
  pas encore. Sur MDN, 11 éléments personnalisés (`<mdn-dropdown>`...) tirent leur
  `display` d'une règle `:host` de leur racine fantôme : ce sont les 11
  différences. Le test les identifie et échoue sur toute autre différence.
- Les feuilles `<link rel=stylesheet>` ne sont pas chargées (pas de réseau).
- `@layer` : le contenu est appliqué, mais sans l'ordre des couches. `revert`
  est traité comme `unset`. Pas de CSS imbriqué dans les règles.
- `@supports` avec une propriété que Lumen ne connaît pas : supposée gérée.
- Les unités qui dépendent de la police réelle (`ex`, `ch`) sont approchées.

## Temps de calcul

`cargo run --release -p lumen-style --example style_pages` (Apple M5, médiane de
20 passages, après parsing du HTML) :

| Page | Éléments | Règles | Cascade | Par élément |
|---|---|---|---|---|
| Wikipédia FR | 5 995 | 59 | 8,8 ms | 1,46 µs |
| Wikipedia EN | 7 864 | 228 | 36,9 ms | 4,69 µs |
| Spec WHATWG | 13 650 | 58 | 19,8 ms | 1,45 µs |
| Doc Rust | 16 086 | 58 | 22,1 ms | 1,38 µs |
| MDN FR | 2 184 | 58 | 3,1 ms | 1,44 µs |

Première version, **sans aucune optimisation** : chaque règle est testée sur
chaque élément, et le temps croît avec le nombre de règles. Les vrais moteurs
(Stylo dans Firefox, Blink) indexent les règles par id, classe et balise, et
partagent les styles entre éléments semblables. Pas encore de comparaison avec
eux : ce sera la prochaine étape, en mesurant d'abord.

## Licence

Au choix : [Apache 2.0](LICENSE-APACHE) ou [MIT](LICENSE-MIT).
