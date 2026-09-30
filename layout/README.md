# lumen-layout

La mise en page CSS : la position et la taille de chaque boîte. C'est une brique
de [Lumen](../README.md), un navigateur web écrit de zéro ; elle part du DOM de
[`html-parseur`](../html/) et des styles calculés de [`lumen-style`](../style/).

## Ce qui est fait : les boîtes de bloc

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

```rust
use lumen_layout::layout_document;
// doc : html_parseur::dom::Document ; styles : lumen_style::Styles
let layout = layout_document(&doc, &styles, 800.0, 600.0);
let rect = layout.border_box(node); // comme getBoundingClientRect()
```

## Vérification contre Chromium

44 pages de test (`tests/oracle/blocks_cases.json`) chargées dans Chromium 152
(iframe de 800 × 600 px) : pour chaque élément, `getBoundingClientRect()`.

| Test | Résultat |
|---|---|
| Boîtes des 44 pages de test (`tests/oracle_blocks.rs`) | **292 / 292** identiques, à 1/64 px près |
| Robustesse : pages aléatoires et les 5 vraies pages (`tests/robustesse.rs`) | **100 000 pages, 0 panique, 0 valeur absurde** |

Chromium range les positions en 64es de pixel (`LayoutUnit`), d'où la tolérance
de 1/64 px. Deux règles ont été apprises de Chromium en comparant :

- sur une page vide, `<html>` fait 8px de haut, pas 16 : les deux marges de 8px
  du `<body>` vide fusionnent *à travers* lui ;
- si `min-height` ou `max-height` change la hauteur d'un bloc, la marge basse de
  son dernier enfant disparaît : elle ne s'échappe pas, et n'est pas comptée
  (règle de LayoutNG, le moteur de mise en page de Chromium, plus précise que le
  texte de CSS 2).

## Ce qui n'est pas encore fait

- **Le texte** et tout le contenu en ligne (`<span>`, `inline-block`, images) :
  il faut des polices pour connaître la hauteur d'une ligne. Un bloc qui ne
  contient que du contenu en ligne a pour l'instant une hauteur de contenu nulle,
  et les pages de test n'ont pas de texte. C'est aussi pour cela qu'un `<li>`
  vide y porte `list-style: none` : sa puce est une ligne de texte.
- Flottants, positionnement (`relative`, `absolute`, `fixed`), `clear`.
- Flexbox, grille, tableaux (mis en page comme des blocs pour l'instant).
- Débordement et défilement (`overflow` ne sert encore qu'à la fusion des marges).

## Licence

Au choix : [Apache 2.0](LICENSE-APACHE) ou [MIT](LICENSE-MIT).
