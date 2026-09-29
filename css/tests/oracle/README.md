# Oracle Chromium

Pour les valeurs de propriétés, il n'existe pas de suite de tests utilisable hors
navigateur (les tests WPT correspondants s'exécutent en JavaScript). On compare
donc lumen-css à un vrai navigateur, Chromium, sur une liste de déclarations.

- `cases.json` : les déclarations testées, `[propriété, valeur]`.
- `chromium.json` : ce que Chromium en a compris (valide ou non, propriétés
  longues et leur valeur sérialisée), avec la version exacte du navigateur et la
  date de capture.
- `capture-chromium.js` : le script de capture.

Même principe pour deux autres briques :

- **Media queries** : `media_cases.json` (les requêtes) et `media_chromium.json`
  (pour chacune, `matchMedia(q).media` et `matchMedia(q).matches`). La capture
  enregistre aussi l'**environnement** (taille de la fenêtre, densité de pixels,
  écran, préférences) : `tests/oracle_media.rs` évalue nos requêtes dans ce même
  environnement. Certains cas sont pile à la limite de la fenêtre capturée
  (371 × 987 px, 2 dppx) pour vérifier `<=` et `>=` : si on recapture avec une
  autre taille, ces cas deviennent moins utiles (sans devenir faux).
- **`var()`** : `variables_cases.json` donne le style d'un parent et d'un enfant ;
  `variables_chromium.json` contient les valeurs calculées de l'enfant
  (`getComputedStyle`). `tests/oracle_variables.rs` les recalcule avec un
  minimum de cascade (la vraie cascade sera une brique à part).

Le test `tests/oracle_chromium.rs` compare nos résultats à `chromium.json`, sans
navigateur : `cargo test -p lumen-css --test oracle_chromium`.

## Ajouter des cas

1. Ajouter les déclarations à `cases.json`.
2. Dans la console d'un Chrome (sur n'importe quelle page), coller
   `capture-chromium.js`, puis exécuter
   `JSON.stringify(captureLumenOracle(<contenu de cases.json>))`.
3. Enregistrer le résultat dans `chromium.json`.

Pour les media queries et `var()` : `captureLumenMedia(<media_cases.json>)` et
`captureLumenVariables(<variables_cases.json>)`, dans le même script.

La référence est celle d'**un** navigateur, à **une** version : si une version
future de Chromium change sa sérialisation, ou si Firefox et Safari diffèrent, il
faudra le noter ici.
