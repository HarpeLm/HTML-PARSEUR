# lumen-style

La cascade CSS : le **style calculé** de chaque élément d'un document HTML.
C'est une brique de [Lumen](../README.md), un navigateur web écrit de zéro, qui
assemble les précédentes : le DOM de [`html-parseur`](../html/) et, de
[`lumen-css`](../css/), les sélecteurs, `@media`, `var()` et les propriétés.

1. **Collecter les règles** : la feuille par défaut du navigateur
   ([`src/ua.css`](src/ua.css), [`src/mathml.css`](src/mathml.css), [`src/svg.css`](src/svg.css)), les
   `<style>` du document (avec `media="..."`, `@media`, `@supports`), l'attribut
   `style=""`, et les `<style>` des arbres fantômes (shadow DOM déclaratif).
2. **Trier les déclarations** : origine et `!important`, contexte (règles `:host`
   d'un arbre fantôme), attribut `style`, spécificité, ordre d'apparition.
3. **Calculer les valeurs** : héritage, `initial` / `inherit` / `unset`, `var()`,
   unités converties en px (`em`, `rem`, `vw`, `calc()`...), mots-clés de taille
   de police, `bolder` / `lighter`, éléments flottants ou positionnés changés en
   blocs. L'héritage suit l'« arbre plat » : un élément placé dans un `<slot>`
   hérite du slot, pas de son hôte.

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
| 54 pages écrites pour chaque règle de la cascade, dont 10 de shadow DOM (`tests/oracle_cascade.rs`) | **10 157 / 10 157** valeurs identiques |
| Wikipédia FR, *Rust* (5 995 éléments) | **131 889 / 131 889** |
| Wikipedia EN, *HTML* (7 864 éléments) | **172 997 / 172 997** |
| Spec WHATWG, *Parsing* (13 650 éléments) | **300 285 / 300 285** |
| Doc Rust, `Vec` (16 086 éléments) | **353 892 / 353 892** |
| MDN FR, `<table>` (2 166 éléments, 18 arbres fantômes) | **47 652 / 47 652** |

Propriétés comparées (22) : `display`, `position`, `float`, `visibility`,
`box-sizing`, `opacity`, `z-index`, `font-size`, `font-weight`, `line-height`,
`margin-top`, `margin-left`, `padding-top`, `padding-left`, les épaisseurs des
4 bordures, `border-top-style`, `border-left-style`, `overflow-x`, `overflow-y`.

Méthode : chaque page est chargée par Chromium dans une iframe (800 × 600 px pour
les pages de test, 1024 × 768 pour les vraies pages). Les vraies pages sont
servies avec une politique de sécurité (CSP) qui bloque tout ce qui est externe :
Chromium n'applique donc, comme Lumen, que sa feuille par défaut, les `<style>` et
les attributs `style`. Scripts et capture : [tests/oracle/](tests/oracle/).

**Ce qui n'est pas encore vérifié ou pas encore géré** :

- Les marges et retraits en `auto` ou en `%` : `getComputedStyle` les donne après
  la mise en page, que Lumen ne fait pas encore (34 valeurs, non comparées).
- Shadow DOM : `:host`, `:host(...)`, les slots et l'arbre plat sont gérés ;
  pas encore `::slotted()`, `:host-context()` ni `::part()`. Les feuilles d'arbres
  fantômes identiques sont parsées une fois chacune (les navigateurs les
  partagent).
- Les feuilles `<link rel=stylesheet>` ne sont pas chargées (pas de réseau).
- `@layer` : le contenu est appliqué, mais sans l'ordre des couches. `revert`
  est traité comme `unset`. Pas de CSS imbriqué dans les règles.
- `@supports` avec une propriété que Lumen ne connaît pas : supposée gérée.
- Les unités qui dépendent de la police réelle (`ex`, `ch`) sont approchées.

## Temps de calcul

`cargo run --release -p lumen-style --example style_pages` (Apple M5, médiane de
20 passages, après parsing du HTML). Temps de la cascade, en ms :

| Étape | Wikipédia FR | Wikipedia EN | WHATWG | Doc Rust | MDN |
|---|---|---|---|---|---|
| v0 : chaque règle testée sur chaque élément | 8,8 | 36,9 | 19,8 | 22,1 | 3,1 |
| 1 : index des règles par id, classe, balise | 6,1 | 22,7 | 15,2 | 16,8 | 2,4 |
| 2 : l'environnement n'est plus copié par élément | 3,6 | 19,4 | 8,7 | 9,1 | 1,4 |
| 3 : chaque déclaration analysée une seule fois | 3,0 | 16,5 | 5,9 | 6,7 | 0,9 |
| 4 : filtre de Bloom des ancêtres | 3,1 | **4,4** | 6,1 | 7,0 | 1,0 |

Chaque étape a été choisie d'après le profil (`./profiling/profile.sh`, flame
graphs dans [profiling/](profiling/)) et vérifiée : styles toujours identiques à
Chromium. Le filtre de Bloom divise par 3,7 le temps de Wikipedia EN (pleine de
sélecteurs comme `.mw-parser-output .reference`) ; sur les pages sans feuille
d'auteur, il ne coûte que son entretien (dans le bruit de mesure, ±5 %).

### Contre Stylo et Chromium

Même machine, mêmes pages, fenêtre de 1024 × 768, feuilles externes non chargées
des trois côtés. Temps pour calculer le style de toute la page, en ms :

| Page | lumen-style | Stylo (Firefox), 1 fil | Stylo, plusieurs fils | Chromium 152 |
|---|---|---|---|---|
| Wikipédia FR | 3,4 | 3,4 | 5,7 | 4,1 |
| Wikipedia EN | **5,0** | 5,5 | 6,2 | 7,3 |
| Spec WHATWG | 6,4 | **5,4** | 10,1 | 9,8 |
| Doc Rust | 7,5 | **6,6** | 13,7 | 10,7 |
| MDN FR | 1,1 * | 1,1 | 1,7 | 1,3 |

\* Mesuré avant le shadow DOM déclaratif. Depuis, Lumen calcule aussi le style
des 146 éléments des arbres fantômes de MDN et parse leurs 18 feuilles : 2,0 ms.

- **Stylo** : via [blitz-dom](https://github.com/dioxuslabs/blitz), qui l'utilise
  avec un DOM html5ever ([comparaisons/stylo](../comparaisons/stylo/),
  `cargo run --release --manifest-path comparaisons/stylo/Cargo.toml`). On mesure
  `resolve_stylist` (indexation des règles + parcours) ; pour Lumen, lecture des
  feuilles + cascade. En plusieurs fils, Stylo est plus lent ici : ces pages sont
  trop petites pour que la répartition du travail soit rentable.
- **Chromium** : mesuré depuis JavaScript
  ([comparaisons/chromium](../comparaisons/chromium/recalcul-style.js)) : une
  règle `* {}` ajoutée force le recalcul du style de tous les éléments. La mesure
  inclut la mise à jour de l'arbre de rendu (pas la mise en page).

**Ce n'est pas le même travail**, et la comparaison flatte Lumen :

- Stylo et Chromium calculent **toutes** les propriétés CSS (plus de 400) ; Lumen,
  29 pour l'instant. Chaque propriété ajoutée coûtera du temps.
- Ils ne calculent pas le style des éléments dans un sous-arbre `display: none`
  (`<head>`...) ; Lumen, si. Leur feuille par défaut n'est pas la même.
- Ils gèrent `::slotted()` et `::part()`, les animations, les pseudo-éléments, le style
  incrémental (ne recalculer que ce qui a changé)...

Ce que ces chiffres montrent : l'algorithme de Lumen est au niveau des vrais
moteurs sur la partie qu'il fait. Pas qu'il est plus rapide qu'eux.

## Licence

Au choix : [Apache 2.0](LICENSE-APACHE) ou [MIT](LICENSE-MIT).
