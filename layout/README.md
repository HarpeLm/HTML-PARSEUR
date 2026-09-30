# lumen-layout

La mise en page CSS : la position et la taille de chaque boîte. C'est une brique
de [Lumen](../README.md), un navigateur web écrit de zéro ; elle part du DOM de
[`html-parseur`](../html/) et des styles calculés de [`lumen-style`](../style/).

## Ce qui est fait

- **L'arbre des boîtes** : `display: none` (aucune boîte), `display: contents`
  (les enfants prennent la place), blocs anonymes autour du contenu en ligne
  voisin de blocs, texte fait d'espaces ignoré, arbre plat (shadow DOM, slots).
- **Les largeurs** (CSS 2 §10.3.3) : `width` en px, `%`, `calc()` ou `auto`,
  `min-width` / `max-width`, `box-sizing`, marges `auto` pour centrer, cas
  « trop large » où la marge droite cède.
- **Les hauteurs** (§10.6.3) : `auto` (d'après le contenu), `%` (seulement si la
  hauteur du parent est connue), `min-height` / `max-height`.
- **La fusion des marges verticales** (§8.3.1) : entre voisins, entre un bloc et
  son premier ou dernier enfant, à travers les blocs vides, avec les marges
  négatives ; bloquée par une bordure, un retrait, `overflow` autre que
  `visible`, `display: flow-root`, une hauteur fixée.
- Bordures (épaisseurs arrondies au pixel d'écran comme Chromium) et retraits.
- **Le texte** (CSS 2 §10.8, CSS Text) : espaces fusionnés (même à cheval sur
  plusieurs éléments, supprimés en début et fin de ligne), mots, lignes remplies
  une à une (coupure aux espaces seulement), éléments en ligne imbriqués, tailles
  mêlées sur une ligne, `line-height` (`normal`, nombre, longueur, `%`), ligne de
  base, crénage ; polices choisies avec [`lumen-font`](../font/).

```rust
use lumen_font::FontDatabase;
use lumen_layout::{Viewport, layout_document};
// doc : html_parseur::dom::Document ; styles : lumen_style::Styles
let viewport = Viewport { width: 800.0, height: 600.0, device_pixel_ratio: 2.0 };
let layout = layout_document(&doc, &styles, &viewport, &FontDatabase::system());
let rect = layout.border_box(node); // comme getBoundingClientRect()
```

## Vérification contre Chromium

Des pages de test chargées dans Chromium 152 (iframe de 800 × 600 px, écran de
densité 2) : pour chaque élément, `getBoundingClientRect()`.

| Test | Résultat |
|---|---|
| Blocs : 44 pages sans texte (`tests/oracle_blocks.rs`) | **292 / 292** boîtes identiques |
| Texte : 22 pages (`tests/oracle_texte.rs`) : 9 polices, hauteurs de ligne, tailles mêlées, gras, passage à la ligne, espaces, crénage | **187 / 187** boîtes identiques |
| Robustesse : pages aléatoires avec texte, et les 5 vraies pages (`tests/robustesse.rs`) | **100 000 pages, 0 panique, 0 valeur absurde** |

Tolérance : 1/64 de pixel d'écran (1/128 px CSS ici), la précision de
Chromium. Les polices sont celles de la machine (mesures prises sur macOS).

**Chromium met en page en pixels d'écran** (taille CSS × densité), et c'est ce
qu'il faut reproduire pour tomber juste : sur un écran de densité 2, une ligne
d'Arial 16px fait 18,5px (et non 18,4 ou 17), parce que la hauteur de la police
est arrondie à 29 + 7 pixels d'écran, puis divisée par 2. Les autres règles
apprises en comparant :

- toutes les longueurs sont tronquées au 64e de pixel d'écran (`LayoutUnit`) ;
- la largeur d'un morceau de texte est arrondie vers le haut au 64e de pixel
  d'écran, et la taille de police coupée à deux décimales ;
- la moitié d'interligne du haut est arrondie **vers le bas au pixel d'écran**,
  le reste va dessous (règle de LayoutNG) ;
- le crénage est appliqué dans la page (pas dans le canvas pour les polices au
  format Apple) ; chaque nœud texte est mis en forme d'un bloc, pas de crénage
  entre deux éléments.

Et pour les blocs :

- sur une page vide, `<html>` fait 8px de haut, pas 16 : les deux marges de 8px
  du `<body>` vide fusionnent *à travers* lui ;
- si `min-height` ou `max-height` change la hauteur d'un bloc, la marge basse de
  son dernier enfant disparaît : elle ne s'échappe pas, et n'est pas comptée
  (règle de LayoutNG, le moteur de mise en page de Chromium, plus précise que le
  texte de CSS 2).

## Ce qui n'est pas encore fait

- Dans le texte : `text-align`, `white-space` (`pre`, `nowrap`), `<br>`, coupure
  après un tiret (règles Unicode UAX 14), césure, ligatures, polices de secours
  (un emoji dans du texte en Arial), `font-style` (italique), `vertical-align`,
  marges et bordures des éléments en ligne, puces des listes.
- `inline-block`, images et autres éléments remplacés.
- Flottants, positionnement (`relative`, `absolute`, `fixed`), `clear`.
- Flexbox, grille, tableaux (mis en page comme des blocs pour l'instant).
- Débordement et défilement (`overflow` ne sert encore qu'à la fusion des marges).

## Licence

Au choix : [Apache 2.0](LICENSE-APACHE) ou [MIT](LICENSE-MIT).
