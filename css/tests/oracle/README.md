# Oracle Chromium

Pour les valeurs de propriétés, il n'existe pas de suite de tests utilisable hors
navigateur (les tests WPT correspondants s'exécutent en JavaScript). On compare
donc lumen-css à un vrai navigateur, Chromium, sur une liste de déclarations.

- `cases.json` : les déclarations testées, `[propriété, valeur]`.
- `chromium.json` : ce que Chromium en a compris (valide ou non, propriétés
  longues et leur valeur sérialisée), avec la version exacte du navigateur et la
  date de capture.
- `capture-chromium.js` : le script de capture.

Le test `tests/oracle_chromium.rs` compare nos résultats à `chromium.json`, sans
navigateur : `cargo test -p lumen-css --test oracle_chromium`.

## Ajouter des cas

1. Ajouter les déclarations à `cases.json`.
2. Dans la console d'un Chrome (sur n'importe quelle page), coller
   `capture-chromium.js`, puis exécuter
   `JSON.stringify(captureLumenOracle(<contenu de cases.json>))`.
3. Enregistrer le résultat dans `chromium.json`.

La référence est celle d'**un** navigateur, à **une** version : si une version
future de Chromium change sa sérialisation, ou si Firefox et Safari diffèrent, il
faudra le noter ici.
